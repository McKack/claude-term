//! Commands: files, lint, notes, terminal shells, images, browsers.
use crate::*;

/// A path inside the project (no escaping it with ..).
pub(crate) fn inside(root: &str, rel: &str) -> R<std::path::PathBuf> {
    let root = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
    let p = std::fs::canonicalize(root.join(rel)).map_err(|e| e.to_string())?;
    if p.starts_with(&root) { Ok(p) } else { Err("outside the project".into()) }
}

#[tauri::command]
pub(crate) fn list_dir(st: St, pid: String, rel: String) -> R<Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    let dir = inside(&p.path, &rel)?;
    let skip = [".git", "node_modules", "target", "dist", "build", "__pycache__", ".venv", ".next"];
    // #377 t = mtime (s), size = bytes: UI sorts by date/size
    let mut items: Vec<(bool, String, u64, u64)> = std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten()
        .map(|e| { let m = e.metadata().ok();
            let t = m.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
            (e.path().is_dir(), e.file_name().to_string_lossy().to_string(), t, m.map_or(0, |m| m.len())) })
        .filter(|(d, n, ..)| !(*d && skip.contains(&n.as_str()))).collect();
    items.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.to_lowercase().cmp(&b.1.to_lowercase())));
    Ok(json!(items.iter().map(|(d, n, t, s)| json!({"name": n, "dir": d, "t": t, "size": s})).collect::<Vec<_>>()))
}

/// #221 Hot files: sizes + 30-day churn (async: git + many stats)
#[tauri::command(async)]
pub(crate) fn file_heat(st: St, pid: String) -> R<Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    Ok(store::file_heat(&p.path))
}

/// Appearance › Size: browser-style zoom, so text, panels and mouse math scale together.
#[tauri::command]
pub(crate) fn set_zoom(w: tauri::WebviewWindow, scale: f64) -> R<()> {
    w.set_zoom(scale.clamp(0.7, 1.6)).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn read_file(st: St, pid: String, rel: String) -> R<String> {
    let p = st.project(&pid).ok_or("no such project")?;
    let f = inside(&p.path, &rel)?;
    // #433 .env readable here (user's viewer); hide_env blocks only Claude's tools (store.rs deny rules)
    let bytes = std::fs::read(&f).map_err(|e| e.to_string())?;
    if bytes.len() > 2_000_000 || bytes.iter().take(8000).any(|b| *b == 0) {
        return Err("Binary or too large to show.".into());
    }
    Ok(String::from_utf8_lossy(&bytes).into())
}

/// #285 Edit view: save a tab back to its (existing, in-project) file. New files → save_as.
#[tauri::command(async)]
pub(crate) fn write_file(st: St, pid: String, rel: String, text: String) -> R<()> {
    let p = st.project(&pid).ok_or("no such project")?;
    std::fs::write(inside(&p.path, &rel)?, text).map_err(|e| e.to_string())
}

/// #435 Edit view: image tab saved with its marks (flattened by the page). ponytail: bytes arrive as a JSON array.
#[tauri::command(async)]
pub(crate) fn write_bytes(st: St, pid: String, rel: String, data: Vec<u8>) -> R<()> {
    let p = st.project(&pid).ok_or("no such project")?;
    std::fs::write(inside(&p.path, &rel)?, data).map_err(|e| e.to_string())
}

/// #374 Edit view lint → [{l, s: "e"|"w", m}]. js-family: project eslint (stdin, own config) if installed, else `node --check` (syntax only);
/// py: py_compile; rs: `cargo check` at the nearest Cargo.toml (reads the SAVED file, whole crate ∴ UI calls it on open/save only). Tool missing → [].
/// HTML: UI blanks non-script lines and sends ext js/mjs (line numbers kept); eslint skipped then (rel ≠ .ext).
#[tauri::command(async)]
pub(crate) fn lint(st: St, pid: String, rel: String, text: String, ext: String) -> R<Vec<Value>> {
    let p = st.project(&pid).ok_or("no such project")?;
    let root = std::path::Path::new(&p.path);
    let js = matches!(ext.as_str(), "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx");
    if ext == "rs" {
        let f = inside(&p.path, &rel)?;
        let rc = std::fs::canonicalize(root).unwrap_or_default(); // f is canonical (\\?\ on Windows)
        let Some(dir) = f.ancestors().skip(1).take_while(|d| d.starts_with(&rc)).find(|d| d.join("Cargo.toml").is_file()) else { return Ok(vec![]) };
        let o = store::run("cargo", &["check", "--message-format", "short", "--color", "never"], Some(&dir.to_string_lossy()), &[]);
        return Ok(cargo_marks(&o.stderr, &rel));
    }
    let es = root.join("node_modules/.bin").join(if cfg!(windows) { "eslint.cmd" } else { "eslint" });
    if js && es.is_file() && rel.ends_with(&format!(".{ext}")) {
        use std::io::Write;
        let mut c = store::command(&es);
        c.args(["--stdin", "--stdin-filename", &rel, "-f", "json"]).current_dir(root)
            .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
        if let Ok(mut ch) = c.spawn() {
            if let Some(mut i) = ch.stdin.take() { let _ = i.write_all(text.as_bytes()); }
            if let Ok(o) = ch.wait_with_output() {
                if let Ok(v) = serde_json::from_slice::<Value>(&o.stdout) { // not JSON (no config…) → syntax check below
                    return Ok(v[0]["messages"].as_array().into_iter().flatten().map(|m| json!({"l": m["line"].as_u64().unwrap_or(1),
                        "s": if m["severity"] == 2 { "e" } else { "w" }, "m": format!("{} ({})", m["message"].as_str().unwrap_or(""), m["ruleId"].as_str().unwrap_or("eslint"))})).collect());
                }
            }
        }
    }
    let tool = match ext.as_str() { "js" | "mjs" | "cjs" => "node", "py" => if cfg!(windows) { "python" } else { "python3" }, _ => return Ok(vec![]) };
    let id: String = pid.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let tmp = std::env::temp_dir().join(format!("clauracle-lint-{id}.{ext}"));
    std::fs::write(&tmp, &text).map_err(|e| e.to_string())?;
    let t = tmp.to_string_lossy();
    let o = if tool == "node" { store::run(tool, &["--check", &t], None, &[]) } else { store::run(tool, &["-m", "py_compile", &t], None, &[]) };
    let _ = std::fs::remove_file(&tmp);
    Ok(if o.ok { vec![] } else { syn_marks(&o.stderr) })
}

/// node --check ("file:LINE" … "SyntaxError: msg") / py_compile ('File "…", line N' … "SyntaxError: msg") → one error mark.
pub(crate) fn syn_marks(out: &str) -> Vec<Value> {
    let num = |s: &str| s.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse::<u64>().ok();
    let line = out.lines().find_map(|x| x.split_once(", line ").and_then(|(_, n)| num(n))
        .or_else(|| x.trim_end().rsplit_once(':').and_then(|(_, n)| if n.chars().all(|c| c.is_ascii_digit()) { num(n) } else { None })));
    let msg = out.lines().map(str::trim).find(|x| x.contains("Error:")).unwrap_or("Syntax error");
    line.map(|l| vec![json!({"l": l, "s": "e", "m": msg})]).unwrap_or_default()
}

/// cargo --message-format short: "src\lib.rs:12:5: warning: msg" → marks for `rel` (suffix match ∵ paths are crate-relative).
pub(crate) fn cargo_marks(out: &str, rel: &str) -> Vec<Value> {
    let rel = rel.replace('\\', "/");
    out.lines().filter_map(|x| {
        let mut it = x.splitn(4, ':');
        let f = it.next()?.replace('\\', "/");
        let l = it.next()?.parse::<u64>().ok()?;
        it.next()?;
        let rest = it.next()?.trim();
        if f.is_empty() || !(rel == f || rel.ends_with(&format!("/{f}"))) { return None; }
        let s = if rest.starts_with("error") { "e" } else if rest.starts_with("warning") { "w" } else { return None };
        Some(json!({"l": l, "s": s, "m": rest.split_once(": ").map_or(rest, |r| r.1)}))
    }).collect()
}

#[cfg(test)]
#[test]
pub(crate) fn lint_parsers() {
    let n = syn_marks("C:\\t\\x.js:3\nfoo(;\n    ^\n\nSyntaxError: Unexpected token ';'\n    at wrap (node:internal/x:1378:20)");
    assert_eq!((n[0]["l"].as_u64(), n[0]["m"].as_str()), (Some(3), Some("SyntaxError: Unexpected token ';'")));
    let p = syn_marks("  File \"C:\\t\\x.py\", line 2\n    x = (\n        ^\nSyntaxError: '(' was never closed");
    assert_eq!(p[0]["l"].as_u64(), Some(2));
    assert!(syn_marks("node not found").is_empty());
    let c = cargo_marks("src\\lib.rs:12:5: warning: unused variable: `x`\nsrc/main.rs:2:1: error[E0425]: nope\nwarning: `x` (lib) generated 1 warning", "src-tauri/src/lib.rs");
    assert_eq!(c.len(), 1);
    assert_eq!((c[0]["l"].as_u64(), c[0]["s"].as_str(), c[0]["m"].as_str()), (Some(12), Some("w"), Some("unused variable: `x`")));
}

/// #371 Notes tab:`<project>/NOTES.md` (missing → ""). text = save; first save adds `/NOTES.md` to .git/info/exclude (local-only, repo untouched).
#[tauri::command(async)]
pub(crate) fn notes(st: St, pid: String, text: Option<String>) -> R<String> {
    let p = st.project(&pid).ok_or("no such project")?;
    let root = std::path::Path::new(&p.path);
    let Some(t) = text else { return Ok(std::fs::read_to_string(root.join("NOTES.md")).unwrap_or_default()) };
    let ex = root.join(".git/info/exclude");
    if root.join(".git").is_dir() && !std::fs::read_to_string(&ex).unwrap_or_default().lines().any(|l| l.trim() == "/NOTES.md" || l.trim() == "NOTES.md") {
        let _ = std::fs::create_dir_all(root.join(".git/info"));
        let mut s = std::fs::read_to_string(&ex).unwrap_or_default();
        if !s.is_empty() && !s.ends_with('\n') { s.push('\n'); }
        let _ = std::fs::write(&ex, s + "/NOTES.md\n");
    }
    std::fs::write(root.join("NOTES.md"), t).map_err(|e| e.to_string())?;
    Ok(String::new())
}

// ---- #285 Shell tab: the OS shell in a pty, one per project; bytes → event `shell` {pid, d} / {pid, exit} ----
pub(crate) struct Shell { master: Box<dyn portable_pty::MasterPty + Send>, w: Box<dyn std::io::Write + Send>, child: Box<dyn portable_pty::Child + Send + Sync> }
pub(crate) static SHELLS: std::sync::LazyLock<Mutex<std::collections::HashMap<String, Shell>>> = std::sync::LazyLock::new(Default::default);

/// Starts it if not running (false = was running already). async ∵ gh_env spawns gh.
#[tauri::command(async)]
pub(crate) fn shell_open(app: AppHandle, st: St, pid: String, cols: u16, rows: u16) -> R<bool> {
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    if SHELLS.lock().unwrap().contains_key(&pid) { return Ok(false); }
    let p = st.project(&pid).ok_or("no such project")?;
    let size = PtySize { rows, cols, pixel_width: 0, pixel_height: 0 };
    let pair = native_pty_system().openpty(size).map_err(|e| e.to_string())?;
    let sh = if cfg!(windows) { store::which("pwsh").map(|p| p.to_string_lossy().into_owned()).unwrap_or("powershell.exe".into()) }
             else { std::env::var("SHELL").unwrap_or("/bin/sh".into()) };
    let mut cmd = CommandBuilder::new(sh);
    cmd.cwd(&p.path);
    // dogfooding: app may run under Claude Code → its CLAUDE*/NO_COLOR vars would leak into `claude` typed here
    for (k, _) in std::env::vars() { if k.starts_with("CLAUDE") || k == "NO_COLOR" { cmd.env_remove(k); } }
    cmd.env("TERM", "xterm-256color");
    for (k, v) in store::gh_env(&p.gh).into_iter().chain(store::gcloud_env(&p.gaccount, &p.gcp)) { cmd.env(k, v); }
    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    drop(pair.slave);
    if let Some(id) = child.process_id() { track(id); } // exit cleanup + orphan check
    let mut r = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let w = pair.master.take_writer().map_err(|e| e.to_string())?;
    SHELLS.lock().unwrap().insert(pid.clone(), Shell { master: pair.master, w, child });
    std::thread::spawn(move || {
        use std::io::Read;
        let (mut buf, mut rest) = ([0u8; 8192], Vec::new());
        while let Ok(n) = r.read(&mut buf) {
            if n == 0 { break; }
            rest.extend_from_slice(&buf[..n]);
            // keep a UTF-8 char split across reads for the next chunk
            let ok = match std::str::from_utf8(&rest) { Ok(_) => rest.len(), Err(e) if e.error_len().is_none() => e.valid_up_to(), Err(_) => rest.len() };
            let d = String::from_utf8_lossy(&rest[..ok]).into_owned();
            rest.drain(..ok);
            emit(&app, "shell", json!({"pid": pid, "d": d}));
        }
        SHELLS.lock().unwrap().remove(&pid);
        emit(&app, "shell", json!({"pid": pid, "exit": true}));
    });
    Ok(true)
}

#[tauri::command]
pub(crate) fn shell_write(pid: String, d: String) -> R<()> {
    use std::io::Write;
    let mut m = SHELLS.lock().unwrap();
    let s = m.get_mut(&pid).ok_or("shell not running")?;
    s.w.write_all(d.as_bytes()).and_then(|_| s.w.flush()).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn shell_resize(pid: String, cols: u16, rows: u16) {
    if let Some(s) = SHELLS.lock().unwrap().get(&pid) {
        let _ = s.master.resize(portable_pty::PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
    }
}

#[tauri::command]
pub(crate) fn shell_kill(pid: String) {
    if let Some(mut s) = SHELLS.lock().unwrap().remove(&pid) { let _ = s.child.kill(); }
}

/// Images Claude read (screenshots) → chat thumbnails. Any path ∵ screenshots live in temp/scratchpad, not the project;
/// ∴ image extensions only, ≤ 20 MB. Raw bytes (no base64 dep) → page gets an ArrayBuffer.
#[tauri::command]
pub(crate) fn read_image(path: String) -> R<tauri::ipc::Response> {
    let ext = std::path::Path::new(&path).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    // #247 + pdf: File viewer (blob → iframe); name kept ∵ callers
    if !["png", "jpg", "jpeg", "gif", "webp", "bmp", "pdf"].contains(&ext.as_str()) { return Err("not an image or PDF".into()); }
    if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 50_000_000 { return Err("too large".into()); }
    Ok(tauri::ipc::Response::new(std::fs::read(&path).map_err(|e| e.to_string())?))
}
/// #247 File viewer "Save as…": copy `from` or write `text` to `to` (picked in the save dialog).
#[tauri::command(async)]
pub(crate) fn save_as(to: String, from: Option<String>, text: Option<String>) -> R<()> {
    match (from, text) { (Some(f), _) => std::fs::copy(f, &to).map(|_| ()), (_, Some(t)) => std::fs::write(&to, t), _ => return Err("nothing to save".into()) }
        .map_err(|e| e.to_string())
}

// ---- browsers: open pages in a system browser profile (you stay logged in there) ----

#[tauri::command(async)]
pub(crate) fn browsers() -> Value {
    json!(store::browsers())
}

#[tauri::command(async)]
pub(crate) fn open_in_browser(exe: String, profile: String, url: String) -> R<()> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("http(s) links only".into());
    }
    let known = store::browsers().iter().any(|b| b["exe"] == exe.as_str());
    if !known {
        return Err("unknown browser".into());
    }
    let mut c = std::process::Command::new(&exe);
    if !profile.is_empty() {
        c.arg(format!("--profile-directory={profile}"));
    }
    c.arg(url).spawn().map(|_| ()).map_err(|e| e.to_string())
}

// ---- #214 task reports: UI builds the plain report from the log; kept 7 days outside the project ----
