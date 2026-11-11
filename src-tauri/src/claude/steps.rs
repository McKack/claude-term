//! Build, run and deploy steps: spawning, stopping, resuming.
use super::*;

pub(crate) fn step_cmd(p: &Project, step: &str) -> String {
    match step { "build" | "local" => &p.build.build, "run" => &p.build.run, _ => &p.build.deploy }.clone()
}

/// Windows: a running exe can't be overwritten (cargo: "failed to remove file … Access is denied"), but it can be
/// renamed. A copy of the app still open from target/ (e.g. Clauracle building itself) → moved aside to *.old,
/// the running copy keeps working, the build writes a fresh exe. Old *.old files are deleted once nothing runs them.
pub(crate) fn free_locked_exes(project: &str) {
    for d in ["src-tauri/target/debug", "src-tauri/target/release", "target/debug", "target/release"] {
        let dir = std::path::Path::new(project).join(d);
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let f = e.path();
            match f.extension().and_then(|x| x.to_str()) {
                Some("old") => { let _ = std::fs::remove_file(&f); }
                Some("exe") if std::fs::OpenOptions::new().write(true).open(&f).is_err() => {
                    let _ = std::fs::rename(&f, f.with_extension(format!("{}.exe.old", store::now() as u64)));
                }
                _ => {}
            }
        }
    }
}

/// #288 full build cmd → fast this-machine twin (debug, no installers). ponytail: Tauri + cargo only; others unchanged
pub fn fast_cmd(cmd: &str) -> String {
    if cmd.contains("tauri build") && !cmd.contains("--debug") { return cmd.replacen("tauri build", "tauri build --debug --no-bundle", 1); }
    if cmd.contains("cargo build") { return cmd.replace(" --release", ""); }
    cmd.to_string()
}

pub(crate) fn shell(cmd: &str) -> tokio::process::Command {
    let mut c = if cfg!(windows) {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(cmd);
        c
    } else {
        let mut c = tokio::process::Command::new("sh");
        c.arg("-c").arg(cmd);
        c
    };
    #[cfg(windows)]
    c.creation_flags(0x0800_0000);
    #[cfg(unix)]
    c.process_group(0); // own group ∴ kill_tree's `kill -- -pid` takes npm → node → cargo → app down, not just sh
    c
}

/// #262 desktop build cmd → its mobile twin (ponytail: Tauri + Flutter only; others → None = "unknown" error)
pub(crate) fn mobile_cmd(cmd: &str, t: &str) -> Option<String> {
    if cmd.contains("tauri build") { return Some(cmd.replacen("tauri build", &format!("tauri {t} build"), 1)); }
    if cmd.contains("flutter build") { return Some(format!("flutter build {}", if t == "ios" { "ipa" } else { "apk" })); }
    None
}

/// Runs one step. `run` stays up (a dev server or the app): it's started, not awaited, and replaces the last one.
/// Returns (ok, last 40 lines of output).
pub async fn run_step(app: &AppHandle, st: &Arc<State>, pid: &str, step: &str) -> (bool, String) {
    let Some(p) = st.project(pid) else { return (false, String::new()) };
    // "local" (/build, "build and run"): the build command on THIS machine only, no WSL / Network Mac passes
    // #262 "build:<os>" (/build windows|linux|macos|ios|android): only that target's pass; host os = same as local
    let fast = step == "local" && !p.build.release; // #288 after-task / smart / /build: debug unless Release is on
    let only = step.strip_prefix("build:").filter(|t| *t != std::env::consts::OS);
    let local = step == "local" || step.starts_with("build:") && only.is_none();
    let step = if local || only.is_some() { "build" } else { step };
    let mut cmd = step_cmd(&p, step);
    if fast { cmd = if p.build.local.trim().is_empty() { fast_cmd(&cmd) } else { p.build.local.clone() }; }
    if let Some(t @ ("ios" | "android")) = only { cmd = match mobile_cmd(&cmd, t) { Some(c) => c, None => {
        let e = format!("No {t} build command known for `{cmd}` (Tauri / Flutter only)"); emit(app, "build", json!({"pid": pid, "step": step, "line": e, "done": false})); return (false, e); } }; }
    if only.is_some_and(|t| !["ios", "android", "linux", "macos"].contains(&t) || t == "ios" && !cfg!(target_os = "macos")) {
        let e = format!("Can't build {} on this computer", only.unwrap()); emit(app, "build", json!({"pid": pid, "step": step, "line": e, "done": false})); return (false, e); }
    if cmd.trim().is_empty() {
        return (true, String::new());
    }
    let say = |t: &str, done: Option<bool>| emit(app, "build", json!({"pid": pid, "step": step, "line": t, "done": done, "local": local})); // #361 local = this-machine pass only (UI: "Local build", no bundle list)
    // #175 marker: still there at the next start = the app died mid-build → resume_builds() starts it again
    let marker = store::home().join(format!("building-{pid}"));
    if step == "build" { let _ = std::fs::write(&marker, ""); }
    if cfg!(windows) && step == "build" { free_locked_exes(&p.path); }
    let sign = store::sign_env(&p.path);
    let mut cmds = if matches!(only, Some("linux" | "macos")) { vec![] } else { vec![(cmd.clone(), shell(&cmd), None::<String>)] }; // linux/macos alone = only their WSL / Mac pass
    // #131: Windows + Linux target + WSL → 2nd pass in WSL (args set by Rust, no cmd.exe quoting); WSLENV forwards signing keys
    if cfg!(windows) && step == "build" && (only == Some("linux") || only.is_none() && !local && p.targets.iter().any(|t| t == "linux")) && store::wsl_ok() {
        let mut w = tokio::process::Command::new("wsl");
        w.args(["-e", "bash", "-lc", &store::wsl_script(pid, &cmd)])
            .env("WSLENV", sign.iter().map(|(k, _)| k.as_str()).chain(std::env::var("WSLENV").ok().as_deref()).collect::<Vec<_>>().join(":"));
        #[cfg(windows)]
        w.creation_flags(0x0800_0000);
        cmds.push((format!("wsl: {cmd} (Linux)"), w, None));
    }
    // #149: non-Mac host + macos target + Network Mac → sync tree, build over ssh (script + #150 signing env on stdin), pull bundles
    let mac = st.cfg.lock().unwrap().mac.clone();
    let remote = !cfg!(target_os = "macos") && step == "build" && (only == Some("macos") || only.is_none() && !local && p.targets.iter().any(|t| t == "macos")) && !mac.host.trim().is_empty();
    if remote {
        let mut c = tokio::process::Command::new("ssh");
        c.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=8", "-o", "StrictHostKeyChecking=accept-new",
            &if mac.user.trim().is_empty() { mac.host.trim().to_string() } else { format!("{}@{}", mac.user.trim(), mac.host.trim()) }, "zsh -l -s"]);
        #[cfg(windows)]
        c.creation_flags(0x0800_0000);
        cmds.push((format!("ssh {}: {cmd} (macOS)", mac.host.trim()), c, Some(store::mac_script(&mac, pid, &cmd, &sign))));
    }
    if cmds.is_empty() { // linux/macos alone with no WSL / Network Mac
        let e = format!("Can't build {} here: {}", only.unwrap_or("it"), if only == Some("macos") { "set up a Network Mac (Settings › Build & Release)" } else { "needs WSL on Windows" });
        emit(app, "build", json!({"pid": pid, "step": step, "line": e, "done": false})); return (false, e); }
    let mut tail: VecDeque<String> = VecDeque::new();
    let mut ok = true;
    for (shown, mut c, input) in cmds {
    say(&format!("$ {shown}"), None);
    if input.is_some() { // the Mac pass: copy the tree over first
        say(&format!("syncing source → {}:{}", mac.host.trim(), store::mac_dir(&mac, pid)), None);
        let (m2, pid2, dir2) = (mac.clone(), pid.to_string(), p.path.clone());
        if let Err(e) = tauri::async_runtime::spawn_blocking(move || store::mac_sync(&m2, &pid2, &dir2)).await.unwrap_or(Err("sync panicked".into())) {
            say(&format!("sync failed: {e}"), None);
            ok = false;
            tail.push_back(format!("Mac sync failed: {e}"));
            break;
        }
    }
    c.current_dir(&p.path).envs(sign.iter().cloned()).envs(store::gh_env(&p.gh)).envs(store::gcloud_env(&p.gaccount, &p.gcp))
        .stdin(if input.is_some() { std::process::Stdio::piped() } else { std::process::Stdio::null() })
        .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    let mut child = match c.spawn() {
        Ok(mut c) => {
            c.id().map(crate::track);
            crate::adopt(&c);
            if let (Some(t), Some(mut i)) = (&input, c.stdin.take()) {
                let t = t.clone();
                tauri::async_runtime::spawn(async move { let _ = i.write_all(t.as_bytes()).await; }); // drop → EOF → script runs
            }
            c
        }
        Err(e) => {
            say(&e.to_string(), Some(false));
            return (false, e.to_string());
        }
    };
    if step != "run" { if let Some(id) = child.id() { BUILDING.lock().unwrap().insert(pid.to_string(), id); } } // stop_build target
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    for out in [child.stdout.take().map(|o| Box::new(o) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
                child.stderr.take().map(|o| Box::new(o) as Box<dyn tokio::io::AsyncRead + Unpin + Send>)].into_iter().flatten() {
        let tx = tx.clone();
        tauri::async_runtime::spawn(async move {
            let mut l = BufReader::new(out).lines();
            while let Ok(Some(s)) = l.next_line().await {
                let _ = tx.send(s);
            }
        });
    }
    drop(tx);
    if step == "run" {
        let (app2, pid2) = (app.clone(), pid.to_string());
        static GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let gen = GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let old = RUNNING.lock().unwrap().insert(pid.to_string(), (gen, child));
        if let Some((_, c)) = old { kill(c); } // replace = stop the previous run's whole tree
        tauri::async_runtime::spawn(async move {
            while let Some(s) = rx.recv().await {
                emit(&app2, "build", json!({"pid": pid2, "step": "run", "line": s, "done": null}));
            }
            // output closed = app exited (or stopped) → UI swaps Stop back to Run; skip if a newer run replaced this one
            let mut r = RUNNING.lock().unwrap();
            if r.get(&pid2).map_or(true, |(g, _)| *g == gen) {
                r.remove(&pid2);
                drop(r);
                emit(&app2, "build", json!({"pid": pid2, "step": "run", "line": null, "done": true}));
            }
        });
        return (true, String::new());
    }
    while let Some(s) = rx.recv().await {
        say(&s, None);
        tail.push_back(s);
        if tail.len() > 40 {
            tail.pop_front();
        }
    }
    ok = child.wait().await.map(|s| s.success()).unwrap_or(false);
    BUILDING.lock().unwrap().remove(pid);
    if ok && input.is_some() {
        say("pulling Mac bundles back", None);
        let (m2, pid2, dir2) = (mac.clone(), pid.to_string(), p.path.clone());
        if let Err(e) = tauri::async_runtime::spawn_blocking(move || store::mac_pull(&m2, &pid2, &dir2)).await.unwrap_or(Err("pull panicked".into())) {
            say(&format!("pull failed: {e}"), None);
            ok = false;
        }
    }
    if !ok { break; }
    }
    if step == "build" { let _ = std::fs::remove_file(&marker); }
    let stopped = STOPPED.lock().unwrap().remove(pid);
    emit(app, "build", json!({"pid": pid, "step": step, "line": if ok { "✓ done" } else if stopped { "■ stopped" } else { "✗ failed" }, "done": ok,
        "own": step == "build" && only.is_none() && ok && own_repo(st, pid)}));
    (ok, tail.into_iter().collect::<Vec<_>>().join("\n"))
}

/// Statusline/Build tab Stop: ends the running build/deploy step's whole tree; its run_step then reports "stopped".
pub fn stop_build(pid: &str) -> bool {
    let id = BUILDING.lock().unwrap().remove(pid);
    if let Some(id) = id { STOPPED.lock().unwrap().insert(pid.to_string()); crate::kill_tree(id); }
    id.is_some()
}
pub(crate) static BUILDING: std::sync::LazyLock<Mutex<HashMap<String, u32>>> = std::sync::LazyLock::new(Default::default);
pub(crate) static STOPPED: std::sync::LazyLock<Mutex<std::collections::HashSet<String>>> = std::sync::LazyLock::new(Default::default);

/// #175 at startup: builds cut off by a close/crash (marker left) start again. Re-attaching is impossible (the build
/// died with us); cargo/tauri caches make the rerun cheap ∴ only what's missing really rebuilds. Delay: page listening.
pub fn resume_builds(app: &AppHandle, st: &Arc<State>) {
    let ids: Vec<String> = st.cfg.lock().unwrap().projects.iter().map(|p| p.id.clone()).collect();
    for pid in ids {
        let m = store::home().join(format!("building-{pid}"));
        if !m.exists() { continue; }
        let _ = std::fs::remove_file(&m);
        let (app, st) = (app.clone(), st.clone());
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(6)).await;
            line(&app, &pid, "note", "A build was cut off when Clauracle closed: building again.");
            run_step(&app, &st, &pid, "build").await;
        });
    }
}

pub fn stop_run(pid: &str) -> bool {
    let c = RUNNING.lock().unwrap().remove(pid);
    c.map(|(_, c)| kill(c)).is_some()
}

/// kill_on_drop only stops the `cmd /C` shell; the app it started (npm → node → cargo → app) kept running → tree kill.
pub(crate) fn kill(c: tokio::process::Child) {
    if let Some(id) = c.id() { crate::kill_tree(id); }
}

pub(crate) static RUNNING: std::sync::LazyLock<Mutex<HashMap<String, (u64, tokio::process::Child)>>> =
    std::sync::LazyLock::new(Default::default);

// ---- history of a resumed session ------------------------------------------------------------------

