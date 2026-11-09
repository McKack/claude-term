//! Clauracle: Tauri commands the page calls. The work lives in claude.rs (sessions) and store.rs (everything else).
mod claude;
mod store;

use claude::{emit, session, Input, State};
use serde_json::{json, Value};
use std::sync::atomic::Ordering::SeqCst;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State as S};

type St<'a> = S<'a, Arc<State>>;
// Sync commands run on the window's main thread: anything that spawns (git/gh/openssl), hits the network/keychain or
// creates a webview (WebView2 deadlock) is #[tauri::command(async)], else the whole window freezes while it waits.
type R<T> = Result<T, String>;

/// Windows: claude sessions and run steps go into one kill-on-close job → the OS ends their whole trees when Clauracle
/// dies any way (crash, force-kill), not only on a clean exit. ponytail: a grandchild spawned in the µs before adopt()
/// escapes; spawn suspended if that ever shows up. Elsewhere a dead parent closes claude's stdin → it exits.
#[cfg(windows)]
pub fn adopt(child: &tokio::process::Child) {
    use windows_sys::Win32::System::JobObjects::*;
    static JOB: std::sync::LazyLock<usize> = std::sync::LazyLock::new(|| unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(job, JobObjectExtendedLimitInformation, &info as *const _ as _, std::mem::size_of_val(&info) as u32);
        job as usize
    });
    if let Some(h) = child.raw_handle() {
        unsafe { AssignProcessToJobObject(*JOB as _, h as _) };
    }
}
#[cfg(not(windows))]
pub fn adopt(_: &tokio::process::Child) {}

pub static CHILDREN: std::sync::LazyLock<Mutex<Vec<u32>>> = std::sync::LazyLock::new(Default::default);
/// Left alive by a crashed earlier run (pid, image name): found at startup, ended only when the user says so.
static ORPHANS: Mutex<Vec<(u32, String)>> = Mutex::new(Vec::new());

/// Child pids persist to ~/.clauracle/children.txt → the next start can spot what a crash left behind (the Windows job
/// normally ends them; older builds / other OSes may not). ponytail: list never shrinks within a run; stale pids are
/// filtered at next start by liveness + name (claude/node/shell) — a reused pid with such a name = false positive, user confirms.
pub fn track(id: u32) {
    let mut c = CHILDREN.lock().unwrap();
    c.push(id);
    save_children(&c);
}
fn save_children(c: &[u32]) {
    let _ = std::fs::write(store::home().join("children.txt"), c.iter().map(u32::to_string).collect::<Vec<_>>().join("\n"));
}

/// Running processes: pid → image name without .exe, lowercase.
fn procs() -> std::collections::HashMap<u32, String> {
    let out = if cfg!(windows) {
        store::command(std::path::Path::new("tasklist")).args(["/FO", "CSV", "/NH"]).output()
    } else {
        std::process::Command::new("ps").args(["-A", "-o", "pid=,comm="]).output()
    };
    let text = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    text.lines().filter_map(|l| {
        let (name, pid) = if cfg!(windows) {
            let f: Vec<&str> = l.split("\",\"").collect(); // "name.exe","pid",…
            (f.first()?.trim_start_matches('"').to_string(), f.get(1)?.to_string())
        } else {
            let (p, n) = l.trim().split_once(' ')?;
            (n.trim().rsplit('/').next()?.to_string(), p.to_string())
        };
        Some((pid.parse().ok()?, name.to_lowercase().trim_end_matches(".exe").to_string()))
    }).collect()
}

fn find_orphans() -> Vec<(u32, String)> {
    let Ok(t) = std::fs::read_to_string(store::home().join("children.txt")) else { return vec![] };
    let live = procs();
    t.lines().filter_map(|l| l.trim().parse().ok())
        .filter_map(|p: u32| live.get(&p).filter(|n| CHILDISH.contains(&n.as_str())).map(|n| (p, n.clone())))
        .collect()
}

/// Orphans from a crashed earlier run; `kill` ends their trees.
#[tauri::command(async)]
fn orphans(kill: bool) -> Vec<Value> {
    let mut o = ORPHANS.lock().unwrap();
    if kill { o.drain(..).for_each(|(p, _)| kill_tree(p)); }
    o.iter().map(|(p, n)| json!({"pid": p, "name": n})).collect()
}

// async + locks dropped before the file reads (PLAN/HANDOVER/CHANGES per project): ran on the window thread holding cfg
#[tauri::command(async)]
fn state(st: St) -> Value {
    let (mut out, paths) = {
        let cfg = st.cfg.lock().unwrap();
        let usage: serde_json::Map<String, Value> =
            cfg.accounts.iter().map(|a| (a.id.clone(), json!(store::usage(&cfg, &a.id)))).collect();
        // when each usage window resets (unix secs), per account: shown in the usage cards, toast when one passes
        let resets: serde_json::Map<String, Value> = cfg.usage.iter().map(|(id, w)|
            (id.clone(), json!(w.iter().filter_map(|(k, v)| v.resets_at.map(|r| (k.clone(), r))).collect::<std::collections::HashMap<_, _>>()))).collect();
        let busy: Vec<String> = st.live.lock().unwrap().iter().filter(|(_, l)| l.busy.load(SeqCst)).map(|(k, _)| k.clone()).collect();
        let paths: Vec<(String, String)> = cfg.projects.iter().map(|p| (p.id.clone(), p.path.clone())).collect();
        (json!({"projects": cfg.projects, "accounts": cfg.accounts, "usage": usage, "resets": resets, "active": cfg.active, "busy": busy,
           "default_account": cfg.default_account, "agents_on": cfg.agents_on, "ci_builds": cfg.ci_builds, "mac": cfg.mac, "web": !cfg.off.iter().any(|x| x == "web"), "lessons": cfg.lessons, "you": cfg.you, "notify": cfg.notify,
           "update_every": cfg.update_every.unwrap_or(24), "update_at": cfg.update_at,
           "projects_dir": store::projects_root(&cfg).to_string_lossy()}), paths)
    };
    let per = |f: &dyn Fn(&str) -> Value| paths.iter().map(|(id, p)| (id.clone(), f(p))).collect::<serde_json::Map<String, Value>>();
    // next steps from PLAN.md (ready bar, Tasks tab), every plan file with tasks (Plan tab ⓘ / merge), last few done (CHANGES.md),
    // PLAN.md "## Waiting on the user" → Plan tab suggestions (Accept/Dismiss), parse only
    out["queues"] = json!(per(&|p| json!(claude::queue(p))));
    out["plans"] = json!(per(&|p| json!(claude::plans(p))));
    out["done"] = json!(per(&|p| json!(claude::done(p, 3))));
    out["waits"] = json!(per(&|p| json!(claude::waiting(p))));
    out["drafts"] = json!(store::drafts());
    out["claude"] = json!(store::which("claude").is_some());
    out["gh"] = json!(store::which("gh").is_some());
    out
}

#[tauri::command(async)]
fn open_project(app: AppHandle, st: St, pid: String) -> R<Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    {
        let mut cfg = st.cfg.lock().unwrap();
        cfg.active = Some(pid.clone());
        if let Some(p) = cfg.projects.iter_mut().find(|p| p.id == pid) {
            p.used = store::now();
        }
        store::save(&cfg);
    }
    let live = session(&app, &st, &pid);
    // previous session (before the last clear), faded, then the current one
    let mut history: Vec<Value> = p.prev_session.as_deref().map(claude::history).unwrap_or_default().into_iter()
        .map(|mut e| { e["old"] = json!(true); e }).collect();
    history.extend(p.session.as_deref().map(claude::history).unwrap_or_default());
    // #243 saved = the page's own log (UI-only notes like task reports too), survives clear + restart; null = never saved
    let saved: Value = log_file(&pid).ok().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    Ok(json!({"branch": store::git_branch(&p.path), "busy": live.busy.load(SeqCst), "history": history, "saved": saved}))
}

/// Who should run the repo at `path` (store::suggest); None if it isn't on GitHub. gh calls, so off the main thread.
async fn detect(st: &St<'_>, path: String) -> Option<store::Who> {
    let cfg = st.cfg.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (owner, repo) = store::repo_of(&path)?;
        Some(store::suggest(&cfg, &owner, &store::gh_users(), |u| store::can_push(u, &owner, &repo)))
    }).await.ok().flatten()
}

/// A new project starts with the suggested GitHub + AI account (the repo owner's, if it's one of ours).
#[tauri::command]
async fn add_project(st: St<'_>, path: String) -> R<Value> {
    let who = detect(&st, path.clone()).await;
    let mut cfg = st.cfg.lock().unwrap();
    let p = match &who {
        Some(w) => store::new_project(&mut cfg, &path, Some(&w.account), Some(w.gh.as_str()).filter(|g| !g.is_empty())),
        None => store::new_project(&mut cfg, &path, None, None),
    };
    let p = mark(&mut cfg, &p.id, who.as_ref()).ok_or("no such project")?;
    store::save(&cfg);
    Ok(json!(p))
}

/// None = project removed meanwhile (was unwrap under the cfg lock → poisoned Mutex → every command dead)
fn mark(cfg: &mut store::Config, pid: &str, who: Option<&store::Who>) -> Option<store::Project> {
    let p = cfg.projects.iter_mut().find(|p| p.id == pid)?;
    if let Some(w) = who {
        (p.owner, p.shared) = (w.owner.clone(), w.shared);
    }
    Some(p.clone())
}

/// After opening: refresh owner/shared, and say whether the suggestion differs from what the project uses.
#[tauri::command]
async fn who(st: St<'_>, pid: String) -> R<Value> {
    let path = st.project(&pid).ok_or("no such project")?.path;
    let Some(w) = detect(&st, path).await else { return Ok(Value::Null) };
    let mut cfg = st.cfg.lock().unwrap();
    let p = mark(&mut cfg, &pid, Some(&w)).ok_or("no such project")?;
    store::save(&cfg);
    let differs = !p.keep && !w.gh.is_empty() && (!w.gh.eq_ignore_ascii_case(&p.gh) || w.account != p.account);
    Ok(json!({"who": w, "differs": differs}))
}

#[tauri::command]
fn remove_project(st: St, pid: String) {
    let mut cfg = st.cfg.lock().unwrap();
    cfg.projects.retain(|p| p.id != pid);
    store::save(&cfg);
    claude::stop_run(&pid); // its dev server/app kept running until exit
    if let Some(l) = st.live.lock().unwrap().remove(&pid) {
        l.inbox.front(vec![Input::Reconnect]); // its loop sees the project gone and ends
    }
}

/// Merge changed fields into the project; anything that shapes the claude process reconnects it (after the
/// current reply, same session).
#[tauri::command]
fn update_project(app: AppHandle, st: St, pid: String, changes: Value) -> R<Value> {
    let quiet = ["name", "context_limit", "build", "keep", "services", "allow", "stage", "rel_auto", "order", "work", "targets", "site", "wired", "license"]; // read live per call: no reconnect
    let p = st.with_project(&pid, |p| {
        let mut v = json!(p);
        for (k, val) in changes.as_object().cloned().unwrap_or_default() {
            if !["id", "path", "session", "questions"].contains(&k.as_str()) {
                v[k] = val;
            }
        }
        // bad value (e.g. NaN → null) used to be dropped silently while the UI thought it saved
        serde_json::from_value(v).map(|n| { *p = n; p.clone() }).map_err(|e| format!("not saved: {e}"))
    }).ok_or("no such project")??;
    if changes.as_object().map_or(false, |o| o.keys().any(|k| !quiet.contains(&k.as_str()))) {
        if let Some(l) = st.live.lock().unwrap().get(&pid) {
            l.inbox.front(vec![Input::Reconnect]);
        }
    }
    emit(&app, "project", json!(p));
    Ok(json!(p))
}

/// images: pasted ones as [{media_type, data(base64)}]; files: paths, mentioned so Claude reads only what it needs.
/// work: the issue this message starts (urgent ask / ask into an empty queue) → persisted ∴ a handover or crash resumes it.
#[tauri::command]
fn send(app: AppHandle, st: St, pid: String, text: String, images: Option<Vec<Value>>, files: Option<Vec<String>>, work: Option<u64>) {
    if let Some(n) = work.filter(|&n| n > 0) { st.with_project(&pid, |p| p.work = n); }
    let live = session(&app, &st, &pid);
    let (images, files) = (images.unwrap_or_default(), files.unwrap_or_default());
    if text.trim() == "/clear" && images.is_empty() && files.is_empty() {
        return claude::fresh(&app, &st, &pid, &live, None);
    }
    let text = if files.is_empty() { text } else {
        format!("{text}\n\nAttached files (read them as needed):\n{}", files.iter().map(|f| format!("- {f}")).collect::<Vec<_>>().join("\n"))
    };
    if images.is_empty() {
        return live.inbox.push(Input::Msg(text));
    }
    // each image is labelled with its name first ∴ a "[name]" reference in the text (#189) points at the right one
    let mut blocks: Vec<Value> = images.iter().flat_map(|i| [json!({"type": "text", "text": format!("[{}]", i["name"].as_str().unwrap_or("image"))}),
        json!({"type": "image", "source": {"type": "base64", "media_type": i["media_type"], "data": i["data"]}})]).collect();
    blocks.push(json!({"type": "text", "text": if text.trim().is_empty() { "(see the image)".to_string() } else { text }}));
    live.inbox.push(Input::Rich(json!(blocks)));
}

#[tauri::command]
fn agents(st: St) -> Value {
    json!(st.cfg.lock().unwrap().agents.clone().unwrap_or_else(store::default_agents))
}

/// Takes effect when a session next connects (new session / continue / settings change).
#[tauri::command]
fn set_agents(st: St, agents: Vec<store::Agent>, on: Option<bool>) {
    let mut cfg = st.cfg.lock().unwrap();
    cfg.agents = Some(agents);
    if let Some(on) = on { cfg.agents_on = on; }
    store::save(&cfg);
    for l in st.live.lock().unwrap().values() {
        l.inbox.front(vec![Input::Reconnect]);
    }
}

/// Message typed while a turn runs → straight into it (true), or false = page keeps it queued for the turn end.
#[tauri::command]
async fn send_mid(st: St<'_>, pid: String, text: String, work: Option<u64>) -> R<bool> {
    let Some(live) = st.live.lock().unwrap().get(&pid).cloned() else { return Ok(false) };
    let ok = claude::send_mid(&live, &text).await;
    if ok { if let Some(n) = work.filter(|&n| n > 0) { st.with_project(&pid, |p| p.work = n); } }
    Ok(ok)
}

#[tauri::command]
async fn interrupt(st: St<'_>, pid: String) -> R<()> {
    let conn = st.live.lock().unwrap().get(&pid).and_then(|l| l.conn.lock().unwrap().clone());
    if let Some(c) = conn {
        c.request(json!({"subtype": "interrupt"}), 10).await?;
    }
    Ok(())
}

/// The Continue button: clear the context and carry on from the handover (with any answers).
/// QR code as an SVG string (Browser tab, phone preview).
#[tauri::command]
fn qr(text: String) -> R<String> {
    let c = qrcode::QrCode::new(text.as_bytes()).map_err(|e| e.to_string())?;
    // 2px per module, no quiet zone (the page pads it): sharp at 1:1, scaled up on click
    Ok(c.render::<qrcode::render::svg::Color>().module_dimensions(2, 2).quiet_zone(false).build())
}

/// GitHub avatar bytes, disk-cached 1 day in ~/.clauracle/avatars ∵ webview re-fetched github.com/x.png on every
/// card open for some users (redirect not cached). UI turns bytes → blob URL once per run.
#[tauri::command]
async fn avatar(login: String) -> R<tauri::ipc::Response> {
    if login.is_empty() || !login.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '[' || c == ']') { return Err("bad login".into()); }
    let f = store::home().join("avatars").join(format!("{login}.png"));
    let fresh = std::fs::metadata(&f).and_then(|m| m.modified()).ok()
        .and_then(|t| t.elapsed().ok()).is_some_and(|age| age.as_secs() < 86400);
    if !fresh {
        let got = ureq::get(&format!("https://github.com/{login}.png?size=88")).set("User-Agent", "Clauracle").timeout(std::time::Duration::from_secs(10)).call()
            .map_err(|e| e.to_string()).and_then(|r| { let mut b = vec![]; std::io::Read::read_to_end(&mut r.into_reader(), &mut b).map_err(|e| e.to_string())?; Ok(b) });
        match got {
            Ok(b) => { let _ = std::fs::create_dir_all(f.parent().unwrap()); let _ = std::fs::write(&f, &b); return Ok(tauri::ipc::Response::new(b)); }
            Err(e) if !f.exists() => return Err(e),
            Err(_) => {} // offline → stale copy
        }
    }
    std::fs::read(&f).map(tauri::ipc::Response::new).map_err(|e| e.to_string())
}

/// This machine's LAN address (the route a phone on the same Wi-Fi would use); UDP connect sends nothing.
#[tauri::command(async)]
fn lan_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0").and_then(|s| { s.connect("8.8.8.8:80")?; s.local_addr() })
        .map(|a| a.ip().to_string()).unwrap_or_default()
}

#[tauri::command]
fn set_lessons(st: St, lessons: Vec<String>) {
    let mut cfg = st.cfg.lock().unwrap();
    cfg.lessons = lessons.into_iter().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
    store::save(&cfg);
}

#[tauri::command]
fn set_you(st: St, you: store::You) {
    let mut cfg = st.cfg.lock().unwrap();
    cfg.you = store::You {nick: you.nick.trim().into(), prefs: you.prefs.trim().into(),
        tags: you.tags.into_iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect()};
    let md = format!("# About the user
Interests: {}

{}
", cfg.you.tags.join(", "), cfg.you.prefs);
    let _ = store::write_atomic(&store::home().join("you.md"), md.as_bytes());
    store::save(&cfg);
}

#[tauri::command(async)] // async ∵ cont_text runs gh in issue mode
fn continue_now(app: AppHandle, st: St, pid: String, extra: Option<String>) {
    if st.project(&pid).is_none() { return; } // removed: no stray session, no unwrap panic below
    let live = session(&app, &st, &pid);
    if let Some(x) = extra.filter(|x| !x.trim().is_empty()) { // user messages sent while it waited: ride along with "continue"
        st.with_project(&pid, |p| p.answers = if p.answers.is_empty() { x } else { format!("{}

{x}", p.answers) });
    }
    let p = st.project(&pid).unwrap();
    let over = store::over_limit(&st.cfg.lock().unwrap(), &p.account);
    if over.is_some() {
        let pick = store::pool_pick(&st.cfg.lock().unwrap(), &p.account); // own statement: an if-let would hold the lock into with_project → deadlock
        if let Some(n) = pick {
            st.with_project(&pid, |p| p.account = n);
        }
    }
    let p = st.project(&pid).unwrap(); // after a pool switch: same path/gh anyway
    // #220: open question = runner paused in every mode (auto_answer holds them by design) ∵ next task went on unanswered
    if !p.questions.is_empty() && p.cont != "auto_answer" {
        claude::line(&app, &pid, "note", "Paused: a question needs you. Answering it carries on.");
        emit(&app, "status", json!({"pid": pid, "busy": false})); // #302 ends the UI's "Reading handover…" pill (no turn starts)
        emit(&app, "questions", json!({"pid": pid, "questions": p.questions}));
        return;
    }
    // #87: no open task + no user text → no fresh turn (was: old PLAN.md "continue" → AI found nothing, stopped)
    let Some(msg) = claude::cont_text(&p).or_else(|| (!p.answers.is_empty()).then(|| claude::CONTINUE_BARE.into())) else {
        claude::line(&app, &pid, "note", "No open tasks: nothing to continue. Send a new request to start again.");
        emit(&app, "status", json!({"pid": pid, "busy": false})); // #302 same
        return;
    };
    if live.continuing.swap(true, SeqCst) { return; } // double click / Continue racing an answer: one fresh session, not two
    claude::fresh(&app, &st, &pid, &live, Some(msg));
}

/// Clear & continue on demand: Claude writes the handover first, then it's ready.
#[tauri::command]
fn prepare_handover(app: AppHandle, st: St, pid: String) {
    let live = session(&app, &st, &pid);
    claude::prepare(&app, &live, &pid, "");
}

#[tauri::command]
fn answer_ask(st: St, id: String, answer: Value) {
    if let Some(tx) = st.asks.lock().unwrap().remove(&id) {
        let _ = tx.send(answer);
    }
}

/// The question card's Confirm. Answers go into the next Continue, or straight to Claude when it's idle.
#[tauri::command(async)] // async ∵ resume → continue_now → cont_text (gh)
fn answer_questions(app: AppHandle, st: St, pid: String, answers: Vec<Value>) {
    let ids: Vec<&str> = answers.iter().filter_map(|a| a["id"].as_str()).collect();
    let (text, send_now, resume) = st.with_project(&pid, |p| {
        let text = claude::answers_text(&p.questions, &answers);
        p.questions.retain(|q| !ids.contains(&q.id.as_str()));
        // #144: answers wake an idle session in any mode: mid-task (!ready) → sent as a turn; ready → Continue.
        // #9/#67 exception: auto run paused on open questions waits for the last one
        (text, !p.ready, p.ready && (p.questions.is_empty() || p.cont == "auto_answer")) // #220: last answer resumes
    }).unwrap_or_default();
    let live = session(&app, &st, &pid);
    if send_now && !live.busy.load(SeqCst) {
        live.inbox.push(Input::Msg(text));
    } else {
        st.with_project(&pid, |p| p.answers = [p.answers.as_str(), text.as_str()].join("\n").trim().to_string());
        // turn ended between the busy check and parking → its pickup took nothing: send now (else answers sat unread)
        if send_now && !live.busy.load(SeqCst) {
            let t = st.with_project(&pid, |p| std::mem::take(&mut p.answers)).unwrap_or_default();
            if !t.is_empty() { live.inbox.push(Input::Msg(t)); }
        }
    }
    emit(&app, "project", json!(st.project(&pid)));
    let acct = st.project(&pid).map(|p| p.account).unwrap_or_default();
    let room = store::over_limit(&st.cfg.lock().unwrap(), &acct).is_none(); // limit pause stays a limit pause
    if resume && room && !live.busy.load(SeqCst) {
        continue_now(app, st, pid, None);
    }
}

// ---- settings ----

#[tauri::command]
async fn settings(st: St<'_>) -> R<Value> {
    if st.cfg.lock().unwrap().catalog.is_empty() {
        refresh_catalog(st.clone()).await?;
    }
    let gh = tauri::async_runtime::spawn_blocking(store::gh_users).await.map_err(|e| e.to_string())?;
    let cfg = st.cfg.lock().unwrap();
    Ok(json!({"catalog": cfg.catalog, "off": cfg.off, "gh_users": gh}))
}

#[tauri::command]
async fn refresh_catalog(st: St<'_>) -> R<Value> {
    let dir = st.plugin_dir.clone();
    let (paths, old) = { let c = st.cfg.lock().unwrap(); (c.projects.iter().map(|p| p.path.clone()).collect::<Vec<_>>(), c.catalog.clone()) };
    let items = tauri::async_runtime::spawn_blocking(move || store::catalog(&dir, &paths, &old)).await.map_err(|e| e.to_string())?;
    let mut cfg = st.cfg.lock().unwrap();
    cfg.catalog = items;
    store::save(&cfg);
    Ok(json!(cfg.catalog))
}

/// #213 Extensions › Find: search skills.sh (skills) | MCP registry (servers). Raw JSON → UI maps it.
#[tauri::command]
async fn ext_find(kind: String, q: String) -> R<Value> {
    let u = if kind == "skill" { "https://skills.sh/api/search?limit=20&q=" } else { "https://registry.modelcontextprotocol.io/v0/servers?limit=30&search=" };
    let url = format!("{u}{}", q.bytes().map(|c| if c.is_ascii_alphanumeric() || b"-_.".contains(&c) { (c as char).to_string() } else { format!("%{c:02X}") }).collect::<String>());
    tauri::async_runtime::spawn_blocking(move || ureq::get(&url).call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string()))
        .await.map_err(|e| e.to_string())?
        .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
}

/// #213 install user-wide. skill: a = "owner/repo", b = skill id → `npx skills add` (Claude Code only, copied ∵ Windows
/// symlinks need admin). mcp: a = name, b = http(s) url (remote) | npm package (stdio via npx). Names from the web → validated.
#[tauri::command]
async fn ext_add(kind: String, a: String, b: String) -> R<()> {
    let ok = regex::Regex::new(r"^@?[A-Za-z0-9_][A-Za-z0-9_./:@-]*$").unwrap();
    if !ok.is_match(&a) || !(ok.is_match(&b) || b.starts_with("https://") && !b.contains(char::is_whitespace)) { return Err("odd name, not installing".into()); }
    let home = dirs::home_dir().unwrap_or_default().to_string_lossy().to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let o = if kind == "skill" {
            store::run("npx", &["-y", "skills", "add", &a, "-s", &b, "-g", "-a", "claude-code", "--copy", "-y"], Some(&home), &[])
        } else {
            let name = a.rsplit('/').next().unwrap_or(&a).to_string(); // registry names are reverse-DNS "io.github.x/name"
            if b.starts_with("http") { store::run("claude", &["mcp", "add", "-s", "user", "--transport", "http", &name, &b], Some(&home), &[]) }
            else { // Windows: npx is a .cmd shim → cmd /c (what `claude mcp add` itself asks for)
                let mut c = vec!["mcp", "add", "-s", "user", &name, "--"];
                if cfg!(windows) { c.extend(["cmd", "/c"]); }
                c.extend(["npx", "-y", &b]);
                store::run("claude", &c, Some(&home), &[]) }
        };
        if o.ok { return Ok(()); }
        // CLI spinners → strip ANSI, keep the tail (the actual error is last)
        let t = regex::Regex::new(r"\x1b\[[0-9;?]*[A-Za-z]").unwrap().replace_all(&format!("{}{}", o.stdout, o.stderr), "").trim().to_string();
        let c: Vec<char> = t.chars().collect();
        Err(c[c.len().saturating_sub(300)..].iter().collect())
    }).await.map_err(|e| e.to_string())?
}

/// scope: "global", an account id (defaults for new projects) or "project:<id>".
#[tauri::command]
fn set_off(st: St, item: String, off: bool, scope: String) {
    let mut cfg = st.cfg.lock().unwrap();
    let list = if scope == "global" {
        Some(&mut cfg.off)
    } else if let Some(pid) = scope.strip_prefix("project:") {
        cfg.projects.iter_mut().find(|p| p.id == pid).map(|p| &mut p.off)
    } else {
        cfg.accounts.iter_mut().find(|a| a.id == scope).map(|a| &mut a.off)
    };
    if let Some(l) = list {
        l.retain(|i| i != &item);
        if off {
            l.push(item);
        }
    }
    store::save(&cfg);
    let live = st.live.lock().unwrap();
    for (pid, l) in live.iter() {
        if scope == "global" || scope == format!("project:{pid}") {
            l.inbox.front(vec![Input::Reconnect]);
        }
    }
}

/// The picker's result: exactly these connections/skills for a project (`project:<id>`) or an account's new projects.
#[tauri::command]
fn set_items(st: St, scope: String, items: Vec<String>) {
    let mut cfg = st.cfg.lock().unwrap();
    if let Some(pid) = scope.strip_prefix("project:") {
        if let Some(p) = cfg.projects.iter_mut().find(|p| p.id == pid) {
            p.on = Some(items);
        }
        if let Some(l) = st.live.lock().unwrap().get(pid) {
            l.inbox.front(vec![Input::Reconnect]);
        }
    } else if let Some(a) = cfg.accounts.iter_mut().find(|a| a.id == scope) {
        a.on = Some(items);
    }
    store::save(&cfg);
}

#[tauri::command]
fn update_account(st: St, id: String, changes: Value) {
    let mut cfg = st.cfg.lock().unwrap();
    if let Some(a) = cfg.accounts.iter_mut().find(|a| a.id == id) {
        let mut v = json!(a);
        for (k, val) in changes.as_object().cloned().unwrap_or_default() {
            if ["nick", "gh", "limits", "pool"].contains(&k.as_str()) {
                v[k] = val;
            }
        }
        if let Ok(n) = serde_json::from_value(v) {
            *a = n;
        }
    }
    store::save(&cfg);
}

#[tauri::command]
fn set_default_account(st: St, id: String) {
    let mut cfg = st.cfg.lock().unwrap();
    cfg.default_account = id;
    store::save(&cfg);
}

#[tauri::command]
async fn claude_login(app: AppHandle, st: St<'_>, nick: String) -> R<Value> {
    let a2 = app.clone();
    let token = tauri::async_runtime::spawn_blocking(move || {
        store::setup_token(|url| emit(&a2, "login_url", json!({"url": url})))
    }).await.map_err(|e| e.to_string())??;
    claude_add_token(st, nick, token, None)
}

#[tauri::command(async)]
fn claude_add_token(st: St, nick: String, token: String, base_url: Option<String>) -> R<Value> {
    let base = base_url.unwrap_or_default();
    if !base.is_empty() {
        if !base.trim().starts_with("http") || token.trim().is_empty() {
            return Err("Needs an http(s) base URL and an API key.".into());
        }
    } else if !store::token_ok(&token) {
        return Err("That doesn't look like a token from `claude setup-token` (sk-ant-oat…).".into());
    }
    let mut cfg = st.cfg.lock().unwrap();
    let a = store::add_account(&mut cfg, &nick, &token, &base);
    store::save(&cfg);
    Ok(json!(a))
}

#[tauri::command(async)]
fn claude_remove(st: St, id: String) {
    let mut cfg = st.cfg.lock().unwrap();
    let pids: Vec<String> = cfg.projects.iter().filter(|p| p.account == id).map(|p| p.id.clone()).collect();
    store::remove_account(&mut cfg, &id);
    store::save(&cfg);
    drop(cfg);
    // their sessions kept running on the removed account's token until some later reconnect
    for (pid, l) in st.live.lock().unwrap().iter() { if pids.contains(pid) { l.inbox.front(vec![claude::Input::Reconnect]); } }
}

#[tauri::command]
fn set_notify(st: St, notify: Value) {
    let mut cfg = st.cfg.lock().unwrap();
    if let Ok(n) = serde_json::from_value(notify) {
        cfg.notify = n;
    }
    store::save(&cfg);
}

#[tauri::command(async)]
fn test_notify(app: AppHandle, st: St) {
    claude::notify(&app, &st, "Clauracle", "Notifications work.");
}

#[tauri::command(async)]
fn deps() -> Value { store::deps() }

#[tauri::command]
async fn install(what: String) -> R<()> {
    tauri::async_runtime::spawn_blocking(move || match what.as_str() {
        "gh" => store::install_gh(), "gcloud" => store::install_gcloud(), _ => store::install_claude() })
        .await.map_err(|e| e.to_string())?
}

/// #167 one-click fix for what UI missOf() spotted in a tool result. Names come from tool output → validated, never a shell.
#[tauri::command]
async fn fix_missing(st: St<'_>, pid: String, kind: String, name: String, feat: Option<String>, pkg: Option<String>) -> R<()> {
    let dir = st.project(&pid).ok_or("no such project")?.path;
    let ok = regex::Regex::new(r"^@?[A-Za-z0-9_][A-Za-z0-9_./-]*$").unwrap();
    if [Some(&name), feat.as_ref(), pkg.as_ref()].into_iter().flatten().any(|s| !ok.is_match(s)) { return Err("odd name, not installing".into()); }
    tauri::async_runtime::spawn_blocking(move || {
        let o = match kind.as_str() {
            "cargo" => { // root crate or a Tauri app's src-tauri
                let m = ["Cargo.toml", "src-tauri/Cargo.toml"].iter().map(|f| std::path::Path::new(&dir).join(f)).find(|f| f.exists()).ok_or("no Cargo.toml")?;
                store::run("cargo", &["add", &name, "--features", feat.as_deref().unwrap_or_default(), "--manifest-path", &m.to_string_lossy()], Some(&dir), &[])
            }
            "npm" => store::run("npm", &["install", &name], Some(&dir), &[]),
            "pip" => store::run(if cfg!(windows) { "python" } else { "python3" }, &["-m", "pip", "install", &name], Some(&dir), &[]),
            "tool" => match name.as_str() {
                "gh" => return store::install_gh(), "gcloud" => return store::install_gcloud(), "claude" => return store::install_claude(),
                _ => { let p = pkg.ok_or("no package known")?;
                    if cfg!(windows) { store::run("winget", &["install", "-e", "--id", &p, "--accept-package-agreements", "--accept-source-agreements"], None, &[]) }
                    else if cfg!(target_os = "macos") { store::run("brew", &["install", &p], None, &[]) }
                    else { return Err(format!("install {name} with your package manager")) } } // ponytail: apt needs sudo
            },
            _ => return Err("unknown kind".into()),
        };
        if o.ok { Ok(()) } else { Err(format!("{}{}", o.stderr.trim(), o.stdout.lines().last().unwrap_or_default())) }
    }).await.map_err(|e| e.to_string())?
}

// ---- Google Cloud ----

/// Everything the Cloud tab shows, fetched in parallel (each gcloud call takes a second or two).
#[tauri::command]
async fn gcloud_info(st: St<'_>, pid: String) -> R<Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    if store::which("gcloud").is_none() {
        return Ok(json!({"installed": false}));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let (a, g) = (p.gaccount.as_str(), p.gcp.as_str());
        let r = |x: Result<Value, String>| x.unwrap_or_else(|e| json!({"error": e}));
        std::thread::scope(|s| {
            let accounts = s.spawn(|| r(store::gcloud(&["auth", "list"], "", "")));
            let projects = s.spawn(|| r(store::gcloud(&["projects", "list"], a, "")));
            let billing = s.spawn(|| r(store::gcloud(&["billing", "accounts", "list"], a, "")));
            let (apis, linked) = if g.is_empty() { (Value::Null, Value::Null) } else {
                let apis = s.spawn(|| r(store::gcloud(&["services", "list", "--enabled"], a, g)));
                let linked = r(store::gcloud(&["billing", "projects", "describe", g], a, ""));
                (apis.join().unwrap_or_default(), linked)
            };
            json!({"installed": true, "accounts": accounts.join().unwrap_or_default(),
                   "projects": projects.join().unwrap_or_default(), "billing": billing.join().unwrap_or_default(),
                   "apis": apis, "linked": linked})
        })
    }).await.map_err(|e| e.to_string())
}

/// gcloud's browser sign-in; the new account joins `gcloud auth list` (gcloud keeps the credentials).
#[tauri::command]
async fn gcloud_login() -> R<()> {
    tauri::async_runtime::spawn_blocking(|| {
        let o = store::run("gcloud", &["auth", "login", "--brief", "--update-adc=false"], None, &[]);
        if o.ok { Ok(()) } else { Err(o.stderr.trim().lines().last().unwrap_or("sign-in failed").to_string()) }
    }).await.map_err(|e| e.to_string())?
}

/// Project actions: create a Cloud project, switch an API on/off, link billing.
#[tauri::command]
async fn gcloud_do(st: St<'_>, pid: String, action: String, arg: String, on: Option<bool>) -> R<Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    let (a, g) = (p.gaccount.clone(), p.gcp.clone());
    tauri::async_runtime::spawn_blocking(move || match action.as_str() {
        "create" => store::gcloud(&["projects", "create", &arg, &format!("--name={arg}")], &a, ""),
        "api" => store::gcloud(&["services", if on.unwrap_or(true) { "enable" } else { "disable" }, &arg], &a, &g),
        "billing" => store::gcloud(&["billing", "projects", "link", &g, &format!("--billing-account={arg}")], &a, ""),
        _ => Err("unknown action".into()),
    }).await.map_err(|e| e.to_string())?
}

// ---- GitHub ----

#[tauri::command]
async fn gh_login(app: AppHandle, scopes: Option<String>) -> R<bool> { // #128 scopes e.g. "user" → re-login adds them
    store::forget_gh_users();
    tauri::async_runtime::spawn_blocking(move || {
        use std::io::BufRead;
        let gh = store::which("gh").ok_or("gh isn't installed")?;
        let mut child = store::command(&gh)
            .args(["auth", "login", "--web", "-h", "github.com", "-p", "https", "--skip-ssh-key"]).args(scopes.iter().flat_map(|s| ["-s", s.as_str()]))
            .env("GH_BROWSER", "echo").env("BROWSER", "echo")
            .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped())
            .spawn().map_err(|e| e.to_string())?;
        // without a terminal gh prints the one-time code and polls
        let re = regex::Regex::new(r"one-time code: (\w{4}-\w{4})").unwrap();
        for out in [child.stderr.take().map(|o| Box::new(o) as Box<dyn std::io::Read + Send>),
                    child.stdout.take().map(|o| Box::new(o) as Box<dyn std::io::Read + Send>)].into_iter().flatten() {
            let (app, re) = (app.clone(), re.clone());
            std::thread::spawn(move || {
                for l in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                    if let Some(c) = re.captures(&l) {
                        emit(&app, "gh_code", json!({"code": &c[1], "url": "https://github.com/login/device"}));
                    }
                }
            });
        }
        let ok = child.wait().map(|s| s.success()).unwrap_or(false);
        store::forget_gh_users(); // again ∵ a gh_users() during the login re-cached the old list
        Ok(ok)
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn gh_logout(user: String) -> bool {
    store::forget_gh_users();
    let ok = tauri::async_runtime::spawn_blocking(move || store::run("gh", &["auth", "logout", "-h", "github.com", "-u", &user], None, &[]).ok)
        .await.unwrap_or(false);
    store::forget_gh_users(); // again: see gh_login
    ok
}

#[tauri::command]
fn set_update_every(st: St, hours: u32) {
    let mut cfg = st.cfg.lock().unwrap();
    cfg.update_every = Some(hours);
    store::save(&cfg);
}

#[tauri::command]
fn set_ci_builds(st: St, on: bool) {
    let mut cfg = st.cfg.lock().unwrap();
    cfg.ci_builds = on;
    store::save(&cfg);
}

#[tauri::command]
fn set_mac(st: St, mac: store::NetMac) {
    let mut cfg = st.cfg.lock().unwrap();
    cfg.mac = mac;
    store::save(&cfg);
}

/// #148 install Mac build deps over SSH; progress lines → event `mac_setup` {line}; returns ok.
#[tauri::command]
async fn mac_setup(app: AppHandle, mac: store::NetMac) -> bool {
    tauri::async_runtime::spawn_blocking(move || store::mac_setup(&mac, |l| emit(&app, "mac_setup", json!({"line": l})))).await.unwrap_or(false)
}

/// #160 Set up SSH: key + authorized_keys via password once, then save cfg.mac (dir kept).
#[tauri::command]
async fn mac_key(st: St<'_>, mac: store::NetMac, pw: String) -> Result<(), String> {
    let m = mac.clone();
    tauri::async_runtime::spawn_blocking(move || store::mac_key_setup(&m, &pw)).await.map_err(|e| e.to_string())??;
    let mut cfg = st.cfg.lock().unwrap();
    cfg.mac = mac;
    store::save(&cfg);
    Ok(())
}

#[tauri::command]
async fn mac_xcode(mac: store::NetMac) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || store::mac_xcode(&mac)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn mac_test(mac: store::NetMac) -> serde_json::Value {
    tauri::async_runtime::spawn_blocking(move || store::mac_test(&mac)).await.unwrap_or_default()
}

#[tauri::command]
fn set_projects_dir(st: St, dir: String) {
    let mut cfg = st.cfg.lock().unwrap();
    cfg.projects_dir = dir;
    store::save(&cfg);
}

/// #35: where a folder belongs under the projects root (<owner>/<repo> or local/<name>).
#[tauri::command(async)]
fn place_of(st: St, path: String) -> String {
    let root = store::projects_root(&st.cfg.lock().unwrap());
    store::place_of(&root, &path).to_string_lossy().into()
}

/// #35/#36: projects found under `dir` ("" = projects root): path, where it belongs, and its project id if added.
#[tauri::command(async)]
fn scan_folder(st: St, dir: String) -> Vec<Value> {
    let root = store::projects_root(&st.cfg.lock().unwrap());
    let base = if dir.trim().is_empty() { root.clone() } else { std::path::PathBuf::from(dir.trim()) };
    let ps: Vec<(String, String)> = st.cfg.lock().unwrap().projects.iter().map(|p| (p.id.clone(), p.path.clone())).collect();
    store::scan_projects(&base).into_iter().map(|d| {
        let s = d.to_string_lossy().to_string();
        let t = store::place_of(&root, &s);
        let pid = ps.iter().find(|(_, p)| store::same_path(std::path::Path::new(p), &d)).map(|x| x.0.clone());
        json!({"path": s, "name": d.file_name().map(|n| n.to_string_lossy().to_string()), "target": t.to_string_lossy(), "placed": store::same_path(&t, &d), "project": pid})
    }).collect()
}

/// #35: move a folder; if it's a project: refused mid-turn, its session is closed first (Windows locks a running cwd),
/// path updated after. The page reopens it.
#[tauri::command]
async fn move_folder(st: St<'_>, from: String, to: String) -> R<String> {
    let pid = st.cfg.lock().unwrap().projects.iter().find(|p| store::same_path(std::path::Path::new(&p.path), std::path::Path::new(&from))).map(|p| (p.id.clone(), p.working));
    if let Some((pid, working)) = &pid {
        if *working { return Err("a turn is running in this project: move it when it's idle".into()); }
        if let Some(l) = st.live.lock().unwrap().remove(pid) { l.inbox.front(vec![Input::Reconnect]); } // loop ends (like remove_project)
        claude::stop_run(pid);
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await; // child exits, releases the folder
    }
    let (f, t) = (std::path::PathBuf::from(&from), std::path::PathBuf::from(&to));
    let dep = t.parent().is_some_and(|p| p.file_name().is_some_and(|n| n == "dependencies"));
    let (f2, t2) = (f.clone(), t.clone());
    tauri::async_runtime::spawn_blocking(move || store::move_dir(&f2, &t2)).await.map_err(|e| e.to_string())??;
    // #51 SDKs: link left at the old path ∵ PATH/JAVA_HOME/ANDROID_HOME point there. Windows junction = no admin needed
    if dep {
        #[cfg(windows)] { let _ = store::run("cmd", &["/c", "mklink", "/J", &from, &to], None, &[]); }
        #[cfg(unix)] { let _ = std::os::unix::fs::symlink(&t, &f); }
    }
    if let Some((pid, _)) = pid { st.with_project(&pid, |p| p.path = to.clone()); }
    Ok(to)
}

/// A new, local-only project: <root>/local/<name> (publish it to GitHub later from the GitHub tab).
#[tauri::command(async)]
fn new_local(st: St, name: String) -> R<Value> {
    let name = name.trim();
    if name.is_empty() || name.contains(['/', '\\', ':']) || name.starts_with('.') {
        return Err("Use a plain folder name".into());
    }
    let mut cfg = st.cfg.lock().unwrap();
    let dir = store::projects_root(&cfg).join("local").join(name);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let p = store::new_project(&mut cfg, &dir.to_string_lossy(), None, None);
    store::save(&cfg);
    Ok(json!(p))
}

#[tauri::command]
async fn gh_info(st: St<'_>, pid: String) -> R<Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    tauri::async_runtime::spawn_blocking(move || {
        let f = "number,title,url,author";
        json!({"repo": store::gh_json(&["repo", "view", "--json", "nameWithOwner,url"], &p.gh, Some(&p.path)),
               "git": store::git_do(&p.path, &p.gh, "status", "").unwrap_or_default(),
               "builds": store::builds(&p.path),
               "release": store::gh_json(&["release", "view", "--json", "tagName,url,publishedAt"], &p.gh, Some(&p.path)),
               "prs": store::gh_json(&["pr", "list", "--json", f], &p.gh, Some(&p.path)),
               "issues": store::gh_json(&["issue", "list", "--search", "-label:clauracle", "--json", f], &p.gh, Some(&p.path)), // clauracle ones = the task list
               "remote": store::repo_of(&p.path).is_some(), "users": store::gh_users()})
    }).await.map_err(|e| e.to_string())
}

/// Local project → new GitHub repo; the project then runs as that gh account.
#[tauri::command]
async fn publish_repo(st: St<'_>, pid: String, user: String, name: String, private: bool) -> R<String> {
    let p = st.project(&pid).ok_or("no such project")?;
    let u = user.clone();
    let url = tauri::async_runtime::spawn_blocking(move || store::publish(&p.path, &u, &name, private))
        .await.map_err(|e| e.to_string())??;
    let who = detect(&st, st.project(&pid).ok_or("no such project")?.path).await;
    let mut cfg = st.cfg.lock().unwrap();
    if let Some(p) = cfg.projects.iter_mut().find(|p| p.id == pid) {
        p.gh = user;
    }
    mark(&mut cfg, &pid, who.as_ref());
    store::save(&cfg);
    Ok(url)
}

/// Team view, polled by the page: open `clauracle` issues + other people's PRs touching my files.
#[tauri::command]
async fn team(st: St<'_>, pid: String) -> R<Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    tauri::async_runtime::spawn_blocking(move || {
        if store::repo_of(&p.path).is_none() {
            return json!({"repo": false});
        }
        let (tasks, conflicts) = std::thread::scope(|s| {
            let t = s.spawn(|| store::tasks(&p.path, &p.gh));
            (t.join().unwrap_or_default(), store::conflicts(&p.path, &p.gh))
        });
        json!({"repo": true, "me": p.gh, "tasks": tasks, "conflicts": conflicts})
    }).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn git_do(st: St<'_>, pid: String, action: String, arg: String) -> R<Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    tauri::async_runtime::spawn_blocking(move || store::git_do(&p.path, &p.gh, &action, &arg)).await.map_err(|e| e.to_string())?
}

// ---- updates: Clauracle's own GitHub releases; via gh when signed in (the repo may be private), else plain HTTPS ----
const REPO: &str = "McKack/claude-term";
fn latest_release() -> Value {
    match store::gh_users().first() {
        Some(u) => store::gh_json(&["api", &format!("repos/{REPO}/releases/latest")], u, None),
        None => ureq::get(&format!("https://api.github.com/repos/{REPO}/releases/latest")).set("User-Agent", "Clauracle")
            .call().ok().and_then(|r| r.into_string().ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
    }
}
/// "0.2.0" > "0.1.9"; non-numbers count as 0
fn newer(a: &str, b: &str) -> bool {
    let v = |s: &str| s.trim_start_matches('v').split('.').map(|x| x.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>();
    v(a) > v(b)
}
#[tauri::command]
async fn update_check(st: St<'_>) -> R<Value> {
    { let mut cfg = st.cfg.lock().unwrap(); cfg.update_at = store::now() as u64; store::save(&cfg); }
    tauri::async_runtime::spawn_blocking(|| {
        let r = latest_release();
        let tag = r["tag_name"].as_str().unwrap_or("");
        json!({"current": env!("CARGO_PKG_VERSION"), "latest": tag, "newer": !tag.is_empty() && newer(tag, env!("CARGO_PKG_VERSION")),
               "url": r["html_url"], "notes": r["body"]})
    }).await.map_err(|e| e.to_string())
}
/// Windows: fetch the release's *-setup.exe, start it, quit (the installer replaces this exe). Elsewhere the page opens the release.
#[tauri::command]
async fn update_run(app: AppHandle) -> R<()> {
    let exe = tauri::async_runtime::spawn_blocking(|| -> R<std::path::PathBuf> {
        let r = latest_release();
        let tag = r["tag_name"].as_str().ok_or("no release")?;
        let dir = std::env::temp_dir().join("clauracle-update");
        let _ = std::fs::create_dir_all(&dir);
        let asset = r["assets"].as_array().into_iter().flatten().find(|a| a["name"].as_str().map_or(false, |n| n.ends_with("-setup.exe"))).ok_or("no installer in the release")?;
        let path = dir.join(asset["name"].as_str().unwrap_or("setup.exe"));
        if let Some(u) = store::gh_users().first() {
            let d = dir.to_string_lossy();
            let o = store::run("gh", &["release", "download", tag, "-R", REPO, "-p", "*-setup.exe", "-D", &d, "--clobber"], None, &store::gh_env(u));
            if !o.ok { return Err(o.stderr.trim().to_string()); }
        } else {
            let resp = ureq::get(asset["browser_download_url"].as_str().unwrap_or("")).call().map_err(|e| e.to_string())?;
            let mut f = std::fs::File::create(&path).map_err(|e| e.to_string())?;
            std::io::copy(&mut resp.into_reader(), &mut f).map_err(|e| e.to_string())?;
        }
        Ok(path)
    }).await.map_err(|e| e.to_string())??;
    std::process::Command::new(exe).spawn().map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}

#[tauri::command]
async fn release(st: St<'_>, pid: String, tag: String, files: Vec<String>, main: Option<bool>, sha: Option<String>) -> R<String> {
    let p = st.project(&pid).ok_or("no such project")?;
    let ci = st.cfg.lock().unwrap().ci_builds;
    tauri::async_runtime::spawn_blocking(move || store::release(&p.path, &p.gh, &tag, &files, ci, main.unwrap_or(false), sha.as_deref())).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn stage(st: St<'_>, pid: String, action: String, v: Option<String>, files: Option<Vec<String>>) -> R<Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    tauri::async_runtime::spawn_blocking(move || store::stage(&p.path, &action, v.as_deref().unwrap_or(""), &files.unwrap_or_default())).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn disk(st: St<'_>, pid: String) -> R<Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    tauri::async_runtime::spawn_blocking(move || json!(store::caches(&p.path).into_iter()
        .map(|(d, n)| json!({"dir": d, "size": n})).collect::<Vec<_>>())).await.map_err(|e| e.to_string())
}
#[tauri::command]
async fn disk_clean(st: St<'_>, pid: String, dirs: Vec<String>) -> R<u64> {
    let p = st.project(&pid).ok_or("no such project")?;
    tauri::async_runtime::spawn_blocking(move || store::clean(&p.path, &dirs)).await.map_err(|e| e.to_string())
}

fn neutral_prompt(ask: &str) -> String {
    format!("Rewrite this request to a software assistant as a GitHub issue. It goes on a shared repo, so write it neutrally: \
        no quotes of the request, no personal remarks, names, emotions or private details; keep every technical fact, file path and \
        requirement. Output ONLY plain text: line 1 = imperative title (≤ 80 chars), blank line, then a short task summary (bullets ok). \
        No preamble, no code fences.\n\nRequest:\n{ask}")
}

#[tauri::command]
async fn task_do(st: St<'_>, pid: String, action: String, mut arg: String, images: Option<Vec<Value>>) -> R<String> {
    let p = st.project(&pid).ok_or("no such project")?;
    if action == "new" { // #309 privacy: raw ask never reaches GitHub (edit history keeps first bodies) → Sonnet's neutral rewrite filed
        let tags: String = store::hash_tags(&arg).iter().map(|t| format!(" #{t}")).collect(); // raw #Tags → labels still
        let (st2, p2, raw) = (st.inner().clone(), p.clone(), arg.clone());
        let dir = store::home().join(".clauracle");
        arg = tauri::async_runtime::spawn_blocking(move || sonnet(&st2, &p2, "low", &dir, &neutral_prompt(&raw))).await.map_err(|e| e.to_string())?
            .map(|s| s.trim_matches('`').trim().to_string()).ok().filter(|s| !s.is_empty())
            // ponytail: Sonnet down → placeholder title, the session working it writes the summary (never the raw text)
            .unwrap_or_else(|| "New task from Clauracle (summary pending)".into()) + &tags;
    }
    // #220: "new" + ask images → saved locally, paths in the body ∴ the session that works the issue sees them
    let saved = store::att_save(&p.path, images.as_deref().unwrap_or_default());
    if !saved.is_empty() {
        arg += &format!("\n\nScreenshots (local to the project folder, not on GitHub; open them with the Read tool from the repo root):\n{}", saved.iter().map(|f| format!("- {f}")).collect::<Vec<_>>().join("\n"));
    }
    // "new" → "URL\nfiled title" (UI's row shows the neutral title, not the raw ask)
    tauri::async_runtime::spawn_blocking(move || store::task_do(&p.path, &p.gh, &action, &arg)
        .map(|u| if action == "new" { format!("{u}\n{}", store::split_task(&arg).0) } else { u })).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn gh_repos(user: String) -> Value {
    // user/repos ∵ `gh repo list` = owned only; this adds collaborator + org repos. --paginate → one array per page (a line each)
    let q = "[.[] | {nameWithOwner: .full_name, description, branch: .default_branch}]";
    tauri::async_runtime::spawn_blocking(move || {
        let o = store::run("gh", &["api", "--paginate", "user/repos?per_page=100&sort=pushed&affiliation=owner,collaborator,organization_member", "--jq", q], None, &store::gh_env(&user));
        if !o.ok { return Value::Null; }
        Value::Array(o.stdout.lines().filter_map(|l| serde_json::from_str::<Vec<Value>>(l).ok()).flatten().collect())
    }).await.unwrap_or(Value::Null)
}

#[tauri::command]
async fn gh_branches(user: String, repo: String) -> Vec<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let o = store::run("gh", &["api", "--paginate", &format!("repos/{repo}/branches?per_page=100"), "--jq", ".[].name"], None, &store::gh_env(&user));
        o.stdout.lines().map(String::from).collect()
    }).await.unwrap_or_default()
}

/// `branch` = existing branch to check out (None = default); `new_branch` = made from it after cloning.
#[tauri::command]
async fn clone_repo(st: St<'_>, user: String, repo: String, branch: Option<String>, new_branch: Option<String>) -> R<Value> {
    // <root>/<owner>/<repo>; already cloned there = just add it
    let dest = store::projects_root(&st.cfg.lock().unwrap()).join(repo.split('/').collect::<std::path::PathBuf>()).to_string_lossy().to_string();
    if !std::path::Path::new(&dest).join(".git").exists() {
        let (d, u) = (dest.clone(), user.clone());
        let out = tauri::async_runtime::spawn_blocking(move || {
            let mut a = vec!["repo", "clone", &repo, &d];
            if let Some(b) = branch.as_deref().filter(|b| !b.is_empty()) { a.extend(["--", "-b", b]); }
            store::run("gh", &a, None, &store::gh_env(&u))
        }).await.map_err(|e| e.to_string())?;
        if !out.ok {
            return Err(out.stderr.trim().to_string());
        }
    }
    // also on an existing clone: the user asked for a fresh branch
    if let Some(nb) = new_branch.as_deref().map(str::trim).filter(|b| !b.is_empty()) {
        let o = store::run("git", &["checkout", "-b", nb], Some(&dest), &[]);
        if !o.ok { return Err(o.stderr.trim().to_string()); }
    }
    if let Some(p) = st.cfg.lock().unwrap().projects.iter().find(|p| p.path == dest) {
        return Ok(json!(p));
    }
    let who = detect(&st, dest.clone()).await;
    let mut cfg = st.cfg.lock().unwrap();
    // the suggested gh if any (owner's), else the account that cloned it
    let gh = who.as_ref().map(|w| w.gh.clone()).filter(|g| !g.is_empty()).unwrap_or(user);
    let p = store::new_project(&mut cfg, &dest, who.as_ref().map(|w| w.account.as_str()), Some(&gh));
    let p = mark(&mut cfg, &p.id, who.as_ref()).ok_or("no such project")?;
    store::save(&cfg);
    Ok(json!(p))
}

// ---- build ----

#[tauri::command]
async fn run_step(app: AppHandle, st: St<'_>, pid: String, step: String) -> R<bool> {
    Ok(claude::run_step(&app, &st, &pid, &step).await.0)
}

#[tauri::command(async)]
fn build_guess(st: St, pid: String) -> R<store::Build> {
    Ok(store::guess_build(&st.project(&pid).ok_or("no such project")?.path))
}

#[tauri::command(async)] // taskkill blocks
fn stop_run(pid: String) -> bool {
    claude::stop_run(&pid)
}

/// "Built!" note / Builds card: swap into Clauracle's freshly built exe (own repo only); turns resume after it.
#[tauri::command(async)]
fn restart_into(app: AppHandle, st: St, pid: String) -> R<()> {
    if !claude::own_repo(&st, &pid) { return Err("not Clauracle's own repo".into()); }
    let p = st.project(&pid).ok_or("no such project")?;
    let (t, a) = claude::restart_target(&claude::own_exe(&p.path))?;
    emit(&app, "restarting", json!({}));
    relaunch(&app, t, &a);
    Ok(())
}

#[tauri::command(async)] // taskkill blocks
fn stop_build(pid: String) -> bool {
    claude::stop_build(&pid)
}

// ---- .env ----

#[tauri::command(async)] // git check-ignore per file
fn env_list(st: St, pid: String) -> R<Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    Ok(json!(store::env_files(&p.path).iter().map(|f| {
        let text = std::fs::read_to_string(std::path::Path::new(&p.path).join(f)).unwrap_or_default();
        json!({"file": f, "ignored": store::git_ignored(&p.path, f), "keys": store::env_parse(&text)})
    }).collect::<Vec<_>>()))
}

#[tauri::command]
fn env_set(st: St, pid: String, file: String, key: String, value: Option<String>) -> R<()> {
    let p = st.project(&pid).ok_or("no such project")?;
    if !(file == ".env" || file.starts_with(".env.")) || file.contains(['/', '\\']) {
        return Err("only .env files in the project root".into());
    }
    if !regex::Regex::new(r"^[A-Za-z_][A-Za-z0-9_.]*$").unwrap().is_match(&key) {
        return Err("keys are letters, digits and _".into());
    }
    let path = std::path::Path::new(&p.path).join(&file);
    store::env_put(&path, &[(&key, value.as_deref())])
}

/// Add the file to .gitignore.
#[tauri::command(async)]
fn env_ignore(st: St, pid: String, file: String) -> R<()> {
    let p = st.project(&pid).ok_or("no such project")?;
    let gi = std::path::Path::new(&p.path).join(".gitignore");
    let mut text = std::fs::read_to_string(&gi).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text += &format!("{file}\n");
    std::fs::write(gi, text).map_err(|e| e.to_string())
}

// ---- Mac signing (Developer ID) from any OS, via openssl ----
// key + CSR (~/.clauracle/apple/<pid>, never in the repo) → user uploads CSR at Apple, downloads .cer → .p12 →
// base64 + identity/team into .env → gh secrets (tauri-action signs + notarizes when APPLE_* secrets exist).

fn apple_dir(pid: &str) -> R<std::path::PathBuf> {
    let d = dirs::home_dir().ok_or("no home")?.join(".clauracle/apple").join(pid);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    Ok(d)
}

fn ssl(args: &[&str]) -> R<String> {
    let o = store::run("openssl", args, None, &[]);
    if o.ok { Ok(o.stdout) } else { Err(format!("openssl: {}", o.stderr.trim())) }
}

/// Signing request; reuses an existing key (a new key would orphan a cert already made from the old one).
/// Email = project's git email (Apple rewrites the subject anyway → no prompt). Copied to Downloads so Apple's upload
/// dialog finds it at once; returns that copy's path.
#[tauri::command(async)]
fn apple_csr(st: St, pid: String) -> R<String> {
    let p = st.project(&pid).ok_or("no such project")?;
    let d = apple_dir(&pid)?;
    let (key, csr) = (d.join("devid.key"), d.join("devid.csr"));
    if !key.is_file() || !csr.is_file() {
        let email = store::run("git", &["config", "user.email"], Some(&p.path), &[]).stdout.trim().replace(['/', '='], "");
        let email = if email.is_empty() { "dev@example.com".into() } else { email };
        ssl(&["req", "-new", "-newkey", "rsa:2048", "-nodes", "-keyout", &key.to_string_lossy(), "-out", &csr.to_string_lossy(),
              "-subj", &format!("/emailAddress={email}/CN=Developer ID")])?;
    }
    let Some(dl) = dirs::download_dir() else { return Ok(csr.to_string_lossy().into()) };
    let out = dl.join("devid.csr");
    std::fs::copy(&csr, &out).map_err(|e| e.to_string())?;
    Ok(out.to_string_lossy().into())
}

/// Newest .cer in Downloads made after our key (= the one Apple just gave for our CSR), if any. UI polls this.
#[tauri::command(async)]
fn apple_cer_find(pid: String) -> Option<String> {
    let since = std::fs::metadata(apple_dir(&pid).ok()?.join("devid.key")).ok()?.modified().ok()?;
    std::fs::read_dir(dirs::download_dir()?).ok()?.flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("cer")))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .filter(|(t, _)| *t > since).max().map(|(_, p)| p.to_string_lossy().into())
}

/// Apple's .cer + our key → .p12 → .env (APPLE_CERTIFICATE base64, password, signing identity, team id). Returns the identity.
#[tauri::command(async)]
fn apple_p12(st: St, pid: String, cer: String) -> R<String> {
    let p = st.project(&pid).ok_or("no such project")?;
    let d = apple_dir(&pid)?;
    let s = |x: &str| d.join(x).to_string_lossy().into_owned();
    if !d.join("devid.key").is_file() { return Err("make the signing request first".into()); }
    ssl(&["x509", "-inform", "DER", "-in", &cer, "-out", &s("devid.pem")])
        .or_else(|_| ssl(&["x509", "-in", &cer, "-out", &s("devid.pem")]))?; // .cer is DER; PEM accepted too
    let subj = ssl(&["x509", "-in", &s("devid.pem"), "-noout", "-subject", "-nameopt", "sep_multiline,utf8"])?;
    let field = |k: &str| subj.lines().find_map(|l| l.trim().strip_prefix(k)).unwrap_or("").to_string();
    let (cn, team) = (field("CN="), field("OU="));
    if !cn.starts_with("Developer ID Application") {
        return Err(format!("that's a \"{cn}\" certificate: make a Developer ID Application one"));
    }
    let pw = ssl(&["rand", "-hex", "16"])?.trim().to_string();
    let args = |legacy| { let mut a = vec!["pkcs12", "-export", "-inkey"]; if legacy { a.insert(2, "-legacy"); } a };
    let (k, pem, p12, pass) = (s("devid.key"), s("devid.pem"), s("devid.p12"), format!("pass:{pw}"));
    let rest = ["-in", pem.as_str(), "-out", p12.as_str(), "-passout", pass.as_str()];
    // -legacy (OpenSSL 3) ∵ macOS `security import` can't read its default AES p12; OpenSSL 1.x has no flag → retry without
    ssl(&[args(true).as_slice(), &[k.as_str()], &rest].concat()).or_else(|_| ssl(&[args(false).as_slice(), &[k.as_str()], &rest].concat()))?;
    let b64 = ssl(&["base64", "-A", "-in", &p12])?.trim().to_string();
    let env = std::path::Path::new(&p.path).join(".env");
    store::env_put(&env, &[("APPLE_CERTIFICATE", Some(b64.as_str())), ("APPLE_CERTIFICATE_PASSWORD", Some(pw.as_str())), ("APPLE_SIGNING_IDENTITY", Some(cn.as_str())), ("APPLE_TEAM_ID", Some(team.as_str()))])?;
    if store::git_ignored(&p.path, ".env") == Some(false) { env_ignore(st, pid, ".env".into())?; }
    Ok(cn)
}

/// Does this project build for macOS (a CI workflow mentions macos, or it's a Tauri app)? → Build tab offers Mac signing.
#[tauri::command(async)]
fn ships_mac(st: St, pid: String) -> R<bool> {
    let root = std::path::PathBuf::from(st.project(&pid).ok_or("no such project")?.path);
    Ok(root.join("src-tauri/tauri.conf.json").is_file() || std::fs::read_dir(root.join(".github/workflows")).into_iter().flatten().flatten()
        .any(|e| std::fs::read_to_string(e.path()).is_ok_and(|t| t.to_lowercase().contains("macos"))))
}

/// Bundle ID (= Apple App ID) from the project's config: Tauri `identifier`, Capacitor `appId`, Expo ios.bundleIdentifier.
/// {id, file, suggest}: suggest = com.<repo owner>.<repo name> (never from the email domain), for the user to accept/edit.
#[tauri::command(async)]
fn bundle_id(st: St, pid: String) -> R<serde_json::Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    let root = std::path::PathBuf::from(&p.path);
    let j = |f: &str| std::fs::read_to_string(root.join(f)).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).unwrap_or_default();
    // ponytail: JSON configs only; Xcode pbxproj / Info.plist / Gradle when a project needs them
    let found = [("src-tauri/tauri.conf.json", j("src-tauri/tauri.conf.json")["identifier"].clone()), ("capacitor.config.json", j("capacitor.config.json")["appId"].clone()),
        ("app.json", j("app.json")["expo"]["ios"]["bundleIdentifier"].clone())].into_iter().find_map(|(f, v)| v.as_str().map(|s| (f, s.to_string())));
    let url = store::run("git", &["remote", "get-url", "origin"], Some(&p.path), &[]).stdout;
    let seg = |s: &str| s.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect::<String>();
    let mut parts = url.trim().trim_end_matches(".git").rsplit(['/', ':']);
    let (name, owner) = (parts.next().map(seg).unwrap_or_default(), parts.next().map(seg).unwrap_or_default());
    // app name: last part of the current ID (keeps "clauracle" over repo "claude-term") → repo name → folder
    let name = found.as_ref().and_then(|f| f.1.rsplit('.').next().map(seg)).filter(|s| !s.is_empty())
        .unwrap_or(if name.is_empty() { seg(&root.file_name().unwrap_or_default().to_string_lossy()) } else { name });
    // #40: website domain (clauracle.arctim.no) wins → reversed (no.arctim.clauracle); a bare domain gets the app name appended
    let dom: Vec<String> = p.site["domain"].as_str().unwrap_or("").split('.').map(seg).filter(|s| !s.is_empty()).rev().collect();
    let suggest = if dom.len() >= 3 { dom.join(".") } else if dom.len() == 2 { format!("{}.{name}", dom.join(".")) }
        else if owner.is_empty() { format!("com.example.{name}") } else { format!("com.{owner}.{name}") };
    Ok(serde_json::json!({"id": found.as_ref().map(|f| &f.1), "file": found.as_ref().map(|f| f.0), "suggest": suggest}))
}

/// #161 website host setup done by code instead of the user: ghpages = Pages on w/ Actions source (gh api);
/// cfpages = CLOUDFLARE_ACCOUNT_ID from the token; netlify = new site → NETLIFY_SITE_ID; #164 services supabase/firebase too. → value shown/filled.
#[tauri::command(async)]
fn site_auto(st: St, pid: String, host: String) -> R<String> {
    let p = st.project(&pid).ok_or("no such project")?;
    let envp = std::path::Path::new(&p.path).join(".env");
    let text = std::fs::read_to_string(&envp).unwrap_or_default();
    let get = |k: &str| store::env_parse(&text).into_iter().find(|(x, v)| x == k && !v.is_empty()).map(|(_, v)| v).ok_or(format!("fill {k} first"));
    let fill = |k: &str, v: &str| store::env_put(&envp, &[(k, Some(v))]).map(|_| v.to_string()); // fresh read ∵ text is from before the API call
    let err = |e: ureq::Error| match e { ureq::Error::Status(c, r) => format!("{c}: {}", r.into_string().unwrap_or_default()), e => e.to_string() };
    match host.as_str() {
        "ghpages" => { // POST = create; 409 = exists → PUT switches its source to Actions
            let env = store::gh_env(&p.gh);
            let a = ["-f", "build_type=workflow"];
            let o = store::run("gh", &[&["api", "-X", "POST", "repos/{owner}/{repo}/pages"], &a[..]].concat(), Some(&p.path), &env);
            if o.ok { return Ok("on".into()); }
            let o2 = store::run("gh", &[&["api", "-X", "PUT", "repos/{owner}/{repo}/pages"], &a[..]].concat(), Some(&p.path), &env);
            if o2.ok { Ok("on".into()) } else { Err(format!("{} {}", o.stderr.trim(), o2.stderr.trim())) }
        }
        "cfpages" => {
            let v: Value = ureq::get("https://api.cloudflare.com/client/v4/accounts").set("Authorization", &format!("Bearer {}", get("CLOUDFLARE_API_TOKEN")?))
                .call().map_err(err)?.into_string().ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
            // ponytail: first account; several → user edits the key
            fill("CLOUDFLARE_ACCOUNT_ID", v["result"][0]["id"].as_str().ok_or("token sees no account: give it Account › Cloudflare Pages: Edit")?)
        }
        "netlify" => {
            let v: Value = ureq::post("https://api.netlify.com/api/v1/sites").set("Authorization", &format!("Bearer {}", get("NETLIFY_AUTH_TOKEN")?))
                .set("Content-Type", "application/json").send_string("{}").map_err(err)?.into_string().ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
            fill("NETLIFY_SITE_ID", v["id"].as_str().ok_or("Netlify returned no site id")?)
        }
        "stripe" => { // #163 webhook endpoint on the website address → STRIPE_WEBHOOK_SECRET (secret only in the create response)
            let d = p.site["domain"].as_str().filter(|d| !d.is_empty()).ok_or("set the website address under Website first")?;
            let url = format!("https://{d}/api/stripe/webhook"); // ponytail: fixed path; the AI wires its handler there
            let ev = ["checkout.session.completed", "customer.subscription.created", "customer.subscription.updated", "customer.subscription.deleted", "invoice.payment_failed"];
            let mut form = vec![("url", url.as_str())];
            form.extend(ev.iter().map(|e| ("enabled_events[]", *e)));
            let v: Value = ureq::post("https://api.stripe.com/v1/webhook_endpoints").set("Authorization", &format!("Bearer {}", get("STRIPE_SECRET_KEY")?))
                .send_form(&form).map_err(err)?.into_string().ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
            fill("STRIPE_WEBHOOK_SECRET", v["secret"].as_str().ok_or("Stripe returned no signing secret")?).map(|_| format!("STRIPE_WEBHOOK_SECRET ({url})"))
        }
        "sshkey" => { // #166 deploy key → .env; → pubkey (UI copies it for the panel)
            let name = std::path::Path::new(&p.path).file_name().unwrap_or_default().to_string_lossy().to_string();
            store::deploy_key(&envp, &format!("clauracle-deploy-{name}"))
        }
        "supabase" => { // #164 Management API w/ a personal token → URL + legacy anon/service_role keys
            let tok = format!("Bearer {}", get("SUPABASE_ACCESS_TOKEN")?);
            let api = |path: &str| -> R<Value> { ureq::get(&format!("https://api.supabase.com/v1/{path}")).set("Authorization", &tok)
                .call().map_err(err)?.into_string().map_err(|e| e.to_string()).and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string())) };
            let ps = api("projects")?;
            let ps = ps.as_array().cloned().unwrap_or_default();
            // pick: ref in a filled SUPABASE_URL → only project → name = folder name; ponytail: no create (needs org, region, db password)
            let want = get("SUPABASE_URL").ok().and_then(|u| u.split("//").nth(1)?.split('.').next().map(String::from));
            let dir = std::path::Path::new(&p.path).file_name().unwrap_or_default().to_string_lossy().to_lowercase();
            let pr = ps.iter().find(|x| want.as_deref().is_some_and(|w| x["id"] == w))
                .or(if ps.len() == 1 { ps.first() } else { None })
                .or_else(|| ps.iter().find(|x| x["name"].as_str().is_some_and(|n| n.to_lowercase() == dir)))
                .ok_or_else(|| if ps.is_empty() { "make a Supabase project first".to_string() } else {
                    format!("several projects ({}): fill SUPABASE_URL with the one to use", ps.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(", ")) })?;
            let r = pr["id"].as_str().ok_or("project without id")?;
            let keys = api(&format!("projects/{r}/api-keys?reveal=true"))?;
            let key = |n: &str| keys.as_array().and_then(|a| a.iter().find(|k| k["name"] == n)).and_then(|k| k["api_key"].as_str()).map(String::from).ok_or(format!("no {n} key"));
            let (anon, svc) = (key("anon")?, key("service_role")?);
            let url = format!("https://{r}.supabase.co");
            store::env_put(&envp, &[("SUPABASE_URL", Some(url.as_str())), ("SUPABASE_ANON_KEY", Some(anon.as_str())), ("SUPABASE_SERVICE_ROLE_KEY", Some(svc.as_str()))])?;
            Ok("SUPABASE_URL + keys".into())
        }
        "firebase" => { // #164 Firebase Management API w/ the project's gcloud account: addFirebase if needed → web app → its config
            if p.gcp.is_empty() { return Err("pick the Google Cloud project under the Google service first".into()); }
            let g = p.gcp.clone();
            let _ = store::gcloud(&["services", "enable", "firebase.googleapis.com"], &p.gaccount, &g); // already on → no-op
            let o = store::run("gcloud", &["auth", "print-access-token"], None, &store::gcloud_env(&p.gaccount, ""));
            if !o.ok { return Err(format!("gcloud: {}", o.stderr.trim())); }
            let tok = format!("Bearer {}", o.stdout.trim());
            let fb = |m: &str, path: &str, body: Option<&str>| -> R<Value> {
                let rq = ureq::request(m, &format!("https://firebase.googleapis.com/v1beta1/{path}")).set("Authorization", &tok).set("x-goog-user-project", &g);
                let rs = match body { Some(b) => rq.set("Content-Type", "application/json").send_string(b), None => rq.call() };
                rs.map_err(err)?.into_string().map_err(|e| e.to_string()).and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
            };
            // long-running op → poll ≤60s
            let wait = |mut v: Value| -> R<Value> {
                for _ in 0..30 {
                    if v["done"] == true { return if v["error"].is_null() { Ok(v["response"].clone()) } else { Err(v["error"]["message"].as_str().unwrap_or("failed").into()) }; }
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    v = fb("GET", v["name"].as_str().ok_or("no operation name")?, None)?;
                }
                Err("Firebase took too long: try again in a minute".into())
            };
            let apps = match fb("GET", &format!("projects/{g}/webApps"), None) {
                Ok(v) => v,
                Err(e) if e.starts_with("404") || e.starts_with("403") => { wait(fb("POST", &format!("projects/{g}:addFirebase"), Some("{}"))?)?; Value::Null }
                Err(e) => return Err(e),
            };
            let id = match apps["apps"][0]["appId"].as_str() {
                Some(a) => a.to_string(),
                None => wait(fb("POST", &format!("projects/{g}/webApps"), Some(&serde_json::json!({"displayName": g}).to_string()))?)?["appId"].as_str().ok_or("no app id")?.to_string(),
            };
            let c = fb("GET", &format!("projects/{g}/webApps/{id}/config"), None)?;
            let s = |k: &str| c[k].as_str().unwrap_or("").to_string();
            let kv = [("FIREBASE_API_KEY", s("apiKey")), ("FIREBASE_AUTH_DOMAIN", s("authDomain")), ("FIREBASE_PROJECT_ID", s("projectId")), ("FIREBASE_APP_ID", s("appId"))];
            store::env_put(&envp, &kv.iter().map(|(k, v)| (*k, Some(v.as_str()))).collect::<Vec<_>>())?;
            Ok("Firebase web config".into())
        }
        "play" => { // #165 service account in the project's Cloud project → JSON key (compact, one .env line) → email (UI copies it for the Play Console invite; no API for that)
            if p.gcp.is_empty() { return Err("pick the Google Cloud project under the Google service first".into()); }
            let g = p.gcp.clone();
            let _ = store::gcloud(&["services", "enable", "androidpublisher.googleapis.com"], &p.gaccount, &g);
            if let Err(e) = store::gcloud(&["iam", "service-accounts", "create", "clauracle-play", "--display-name=Google Play releases"], &p.gaccount, &g) {
                if !e.contains("already exists") { return Err(format!("gcloud: {e}")); }
            }
            let email = format!("clauracle-play@{g}.iam.gserviceaccount.com");
            let tmp = std::env::temp_dir().join(format!("clauracle-play-{}.json", std::process::id()));
            let r = store::gcloud(&["iam", "service-accounts", "keys", "create", &tmp.to_string_lossy(), &format!("--iam-account={email}")], &p.gaccount, &g)
                .and_then(|_| std::fs::read_to_string(&tmp).map_err(|e| e.to_string()));
            let _ = std::fs::remove_file(&tmp); // private key: never leave it in temp
            let key: Value = serde_json::from_str(&r.map_err(|e| format!("gcloud: {e}"))?).map_err(|e| e.to_string())?;
            fill("PLAY_SERVICE_ACCOUNT_JSON", &key.to_string())?;
            Ok(email)
        }
        _ => Err("nothing to automate for this host".into()),
    }
}

/// #166 plain SSH/VPS: deploy pubkey → SITE_SSH_USER@SITE_SSH_HOST's authorized_keys w/ one password login (never stored).
#[tauri::command(async)]
fn site_key_install(st: St, pid: String, pw: String) -> R<()> {
    let p = st.project(&pid).ok_or("no such project")?;
    let env = store::env_parse(&std::fs::read_to_string(std::path::Path::new(&p.path).join(".env")).unwrap_or_default());
    let get = |k: &str| env.iter().find(|(x, v)| x == k && !v.is_empty()).map(|(_, v)| v.clone()).ok_or(format!("fill {k} first"));
    let (host, user, k) = (get("SITE_SSH_HOST")?, get("SITE_SSH_USER")?, get("SITE_SSH_PUBLIC_KEY")?);
    store::ssh_key_install(&format!("{user}@{host}"), &k, &pw).map_err(|e| e.replace("{who}", "SSH"))
}

/// #40: HTTP status per URL (0 = unreachable); the release check uses it for /privacy + /terms being live.
#[tauri::command(async)]
fn site_check(urls: Vec<String>) -> Vec<u16> {
    let a = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(8)).build();
    urls.iter().map(|u| match a.get(u).set("User-Agent", "Clauracle").call() {
        Ok(r) => r.status(), Err(ureq::Error::Status(c, _)) => c, Err(_) => 0 }).collect()
}

/// App version + the file it's in: package.json / tauri.conf.json "version", Cargo.toml / pyproject.toml `version = "…"`.
// ponytail: first `version =` line in a toml (≈ [package]/[project]); workspace-inherited versions → not found
/// #264 licence text → SPDX id ("" = unknown). Order matters: LGPL/AGPL before GPL.
fn lic_guess(t: &str) -> &'static str {
    let t = t.to_lowercase();
    [("mit license", "MIT"), ("apache license", "Apache-2.0"), ("lesser general public", "LGPL-3.0"), ("affero general public", "AGPL-3.0"),
     ("general public license", "GPL-3.0"), ("mozilla public license", "MPL-2.0"), ("bsd 3-clause", "BSD-3-Clause"), ("redistributions of source", "BSD-3-Clause"),
     ("unlicense", "Unlicense"), ("isc license", "ISC"), ("permission is hereby granted, free of charge", "MIT")]
        .iter().find(|(k, _)| t.contains(k)).map_or("", |x| x.1)
}
#[test]
fn lic_guess_ids() { assert_eq!(lic_guess("MIT License\n\nCopyright"), "MIT"); assert_eq!(lic_guess("GNU LESSER GENERAL PUBLIC LICENSE"), "LGPL-3.0"); assert_eq!(lic_guess("hi"), ""); }

fn version_of(root: &std::path::Path) -> Option<(String, &'static str)> {
    ["src-tauri/tauri.conf.json", "package.json", "Cargo.toml", "src-tauri/Cargo.toml", "pyproject.toml"].into_iter().find_map(|f| {
        let t = std::fs::read_to_string(root.join(f)).ok()?;
        let v = if f.ends_with(".json") { serde_json::from_str::<Value>(&t).ok()?["version"].as_str()?.to_string() } else { toml_version(&t)? };
        // tauri.conf "version": "../package.json" = path ref → not a version, try the next file
        v.starts_with(|c: char| c.is_ascii_digit()).then_some((v, f))
    })
}
/// PNG size from the IHDR header (bytes 16..24: width, height BE) → shorter side; 0 = unreadable
fn png_px(f: &std::path::Path) -> u32 {
    use std::io::Read;
    let mut b = [0u8; 24];
    std::fs::File::open(f).and_then(|mut h| h.read_exact(&mut b)).map_or(0, |_| if &b[1..4] == b"PNG" { u32::from_be_bytes(b[16..20].try_into().unwrap()).min(u32::from_be_bytes(b[20..24].try_into().unwrap())) } else { 0 })
}
fn toml_version(t: &str) -> Option<String> {
    t.lines().find_map(|l| l.trim().strip_prefix("version")?.trim_start().strip_prefix('=')?.trim().strip_prefix('"')?.split('"').next().map(String::from))
}

/// Release wizard facts, plain checks, no AI: files present, version vs last tag, git clean, last CI run, Mac secrets.
#[tauri::command]
async fn release_check(st: St<'_>, pid: String) -> R<Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    tauri::async_runtime::spawn_blocking(move || {
        let root = std::path::PathBuf::from(&p.path);
        let files: Vec<(String, std::path::PathBuf)> = std::fs::read_dir(&root).into_iter().flatten().flatten().map(|e| (e.file_name().to_string_lossy().to_uppercase(), e.path())).collect();
        let has = |pre: &[&str]| files.iter().any(|(n, _)| pre.iter().any(|x| n.starts_with(x)));
        // #264 licence id guessed from the file text → preselect on Confirm. ponytail: first-match keywords, not SPDX matching
        let lic_id = files.iter().find(|(n, _)| ["LICENSE", "LICENCE", "COPYING"].iter().any(|x| n.starts_with(x))).and_then(|(_, f)| std::fs::read_to_string(f).ok()).map_or("", |t| lic_guess(&t));
        let readme = files.iter().find(|(n, _)| n.starts_with("README")).and_then(|(_, f)| std::fs::read_to_string(f).ok()).map_or(0, |t| t.lines().count());
        let ver = version_of(&root);
        let git = |a: &[&str]| store::run("git", a, Some(&p.path), &[]);
        // #119: highest version tag incl. remote ones (gh release create tags on GitHub only → describe saw v0.1.1 after v0.1.2)
        let _ = store::run("git", &["fetch", "-q", "--tags"], Some(&p.path), &store::gh_env(&p.gh));
        let tag = git(&["tag", "--list", "v[0-9]*", "--sort=-v:refname"]);
        let tag = store::Out { stdout: tag.stdout.lines().next().unwrap_or("").to_string(), ok: tag.ok && !tag.stdout.trim().is_empty(), ..tag };
        // #237 release icon source: SVG wins, else biggest PNG (px = shorter side); px None = vector / can't tell (ico, Android res dir)
        let icon = ["assets/icon.svg", "src-tauri/icons/icon.svg", "icon.svg", "public/favicon.svg", "src-tauri/icons/icon.png", "assets/icon.png", "icon.png", "public/favicon.ico", "app/src/main/res"]
            .iter().filter(|f| root.join(f).exists()).map(|f| (*f, if f.ends_with(".png") { Some(png_px(&root.join(f))) } else if f.ends_with(".svg") { None } else { Some(0) }))
            .max_by_key(|(_, px)| px.unwrap_or(u32::MAX));
        let remote = store::repo_of(&p.path).is_some();
        // target platforms the CI workflows build (#27): plain text match on runner/target names. ponytail: no YAML parse
        let wf: String = std::fs::read_dir(root.join(".github/workflows")).into_iter().flatten().flatten()
            .filter_map(|e| std::fs::read_to_string(e.path()).ok()).collect::<Vec<_>>().join("\n").to_lowercase();
        let ci_os: Vec<&str> = [("windows", "windows-"), ("macos", "macos-"), ("linux", "ubuntu-"), ("ios", "ios"), ("android", "android")]
            .into_iter().filter(|(_, k)| wf.contains(k)).map(|(t, _)| t).collect();
        // #123: last run; failed → first job annotation as `why` (billing/spending-limit refusals show only there)
        let mut ci = if remote { store::gh_json(&["run", "list", "--limit", "1", "--json", "databaseId,conclusion,status,workflowName,url,headBranch"], &p.gh, Some(&p.path)).get(0).cloned() } else { None };
        if let Some(r) = ci.as_mut().filter(|r| r["conclusion"] == "failure") {
            let id = r["databaseId"].to_string();
            let job = store::run("gh", &["run", "view", &id, "--json", "jobs", "--jq", ".jobs[0].databaseId"], Some(&p.path), &store::gh_env(&p.gh)).stdout.trim().to_string();
            if !job.is_empty() {
                let why = store::run("gh", &["api", &format!("repos/{{owner}}/{{repo}}/check-runs/{job}/annotations"), "--jq", ".[0].message // \"\""], Some(&p.path), &store::gh_env(&p.gh)).stdout.trim().to_string();
                // #133 billing block → exact budgets page (user vs org). ponytail: no user budget API ∴ link, not fix-in-app
                let w = why.to_lowercase();
                if ["spending limit", "billing", "payment", "budget"].iter().any(|k| w.contains(k)) {
                    if let Some((owner, _)) = store::repo_of(&p.path) {
                        r["bill"] = json!(if owner.eq_ignore_ascii_case(&p.gh) { "https://github.com/settings/billing/budgets".to_string() }
                                          else { format!("https://github.com/organizations/{owner}/settings/billing/budgets") });
                    }
                }
                r["why"] = json!(why);
            }
        }
        json!({"readme": readme, "ci_os": ci_os, "license": has(&["LICENSE", "LICENCE", "COPYING"]), "lic_id": lic_id, "changelog": has(&["CHANGELOG", "HISTORY", "RELEASES"]),
               "version": ver.as_ref().map(|v| &v.0), "vfile": ver.as_ref().map(|v| v.1), "tag": if tag.ok { tag.stdout.trim() } else { "" },
               "clean": git(&["status", "--porcelain"]).stdout.trim().is_empty(), "icon": icon.map(|i| i.0), "icon_px": icon.and_then(|i| i.1), "remote": remote, "code_t": store::code_time(&p.path),
               "ci": ci, "sign": store::sign_env(&p.path).into_iter().map(|(k, _)| k).collect::<Vec<_>>()})
    }).await.map_err(|e| e.to_string())
}

/// #128 Actions usage this month for the repo owner (user or org) via the billing usage report. Needs the token's `user`
/// scope (org: admin) → `{scope}` when missing. ponytail: no budget/spending-limit API for users → UI links the billing page.
#[tauri::command(async)]
fn actions_usage(st: St, pid: String, year: u32, month: u32) -> R<Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    let (owner, _) = store::repo_of(&p.path).ok_or("not on GitHub")?;
    let me = owner.eq_ignore_ascii_case(&p.gh);
    let base = if me { format!("users/{owner}") } else { format!("organizations/{owner}") };
    let env = store::gh_env(&p.gh);
    let o = store::run("gh", &["api", &format!("{base}/settings/billing/usage?year={year}&month={month}")], None, &env);
    if !o.ok {
        let e = format!("{}{}", o.stdout, o.stderr);
        if e.contains("scope") || e.contains("404") || e.contains("403") { return Ok(json!({"owner": owner, "org": !me, "scope": true, "why": e.lines().last().unwrap_or("").trim()})); }
        return Err(e.trim().to_string());
    }
    let v: Value = serde_json::from_str(&o.stdout).unwrap_or_default();
    Ok(json!({"owner": owner, "org": !me, "plan": if me { store::run("gh", &["api", "user", "--jq", ".plan.name"], None, &env).stdout.trim().to_string() } else { String::new() },
              "u": actions_sum(&v)}))
}

/// usageItems → {min, sku: {name: minutes}, gross, net} for product "actions" (minutes rows only for min/sku).
fn actions_sum(v: &Value) -> Value {
    let (mut min, mut gross, mut net, mut sku) = (0.0, 0.0, 0.0, serde_json::Map::new());
    for i in v["usageItems"].as_array().into_iter().flatten().filter(|i| i["product"].as_str().is_some_and(|s| s.eq_ignore_ascii_case("actions"))) {
        gross += i["grossAmount"].as_f64().unwrap_or(0.0);
        net += i["netAmount"].as_f64().unwrap_or(0.0);
        if i["unitType"].as_str().is_some_and(|s| s.eq_ignore_ascii_case("minutes")) {
            let q = i["quantity"].as_f64().unwrap_or(0.0);
            min += q;
            let k = i["sku"].as_str().unwrap_or("?").trim_start_matches("Actions ").to_string();
            let e = sku.entry(k).or_insert(json!(0.0));
            *e = json!(e.as_f64().unwrap_or(0.0) + q);
        }
    }
    json!({"min": min, "sku": sku, "gross": gross, "net": net})
}

#[test]
fn actions_sum_adds_minutes_per_sku() {
    let v = json!({"usageItems": [
        {"product": "actions", "sku": "Actions Linux", "unitType": "Minutes", "quantity": 100.0, "grossAmount": 0.8, "netAmount": 0.0},
        {"product": "actions", "sku": "Actions Linux", "unitType": "Minutes", "quantity": 20.0, "grossAmount": 0.16, "netAmount": 0.0},
        {"product": "actions", "sku": "Actions macOS 3-core", "unitType": "Minutes", "quantity": 5.0, "grossAmount": 0.4, "netAmount": 0.4},
        {"product": "actions", "sku": "Actions storage", "unitType": "GigabyteHours", "quantity": 9.0, "grossAmount": 0.1, "netAmount": 0.1},
        {"product": "git_lfs", "sku": "LFS", "unitType": "Minutes", "quantity": 7.0, "grossAmount": 1.0, "netAmount": 1.0}]});
    let s = actions_sum(&v);
    assert_eq!(s["min"], 125.0);
    assert_eq!(s["sku"]["Linux"], 120.0);
    assert!((s["net"].as_f64().unwrap() - 0.5).abs() < 1e-9);
}

/// Page → the `ui` self-test tool's waiting call (claude::ui_tool).
#[tauri::command]
fn ui_reply(id: u64, text: String) {
    let mut w = claude::UI_WAIT.lock().unwrap();
    if let Some(i) = w.iter().position(|(n, _)| *n == id) { let _ = w.remove(i).1.send(text); }
}

/// Release panel "Set version": swaps the version in every version file that exists (first `version` key/line only,
/// formatting kept). Cargo.lock follows on the next cargo build.
#[tauri::command(async)]
fn version_set(st: St, pid: String, v: String) -> R<Vec<String>> {
    if v.is_empty() || !v.chars().all(|c| c.is_ascii_alphanumeric() || ".-+".contains(c)) { return Err("bad version".into()); }
    let root = std::path::PathBuf::from(&st.project(&pid).ok_or("no such project")?.path);
    let mut done = vec![];
    for f in ["src-tauri/tauri.conf.json", "package.json", "Cargo.toml", "src-tauri/Cargo.toml", "pyproject.toml"] {
        let Ok(t) = std::fs::read_to_string(root.join(f)) else { continue };
        if let Some(n) = swap_version(&t, &v) { std::fs::write(root.join(f), n).map_err(|e| e.to_string())?; done.push(f.to_string()); }
    }
    if done.is_empty() { Err("no version found (tauri.conf.json / package.json / Cargo.toml / pyproject.toml)".into()) } else { Ok(done) }
}
/// json: first `"version": "x"`; toml: first line `version = "x"` (= [package] in practice). None = no version there.
fn swap_version(t: &str, v: &str) -> Option<String> {
    let at = t.find("\"version\"").or_else(|| t.split_inclusive('\n').scan(0, |o, l| { let s = *o; *o += l.len(); Some((s, l)) }) /* ≠ lines(): +1 was off by one per CRLF */
        .find(|(_, l)| l.starts_with("version") && l[7..].trim_start().starts_with('=')).map(|(s, _)| s))?;
    let q1 = at + t[at..].find(['=', ':'])? + 1;
    let q1 = q1 + t[q1..].find('"')? + 1;
    let q2 = q1 + t[q1..].find('"')?;
    Some(format!("{}{v}{}", &t[..q1], &t[q2..]))
}
#[cfg(test)]
#[test]
fn swap_version_works() {
    assert_eq!(swap_version("{\n  \"name\": \"a\",\n  \"version\": \"0.1.0\"\n}", "0.1.1").unwrap(), "{\n  \"name\": \"a\",\n  \"version\": \"0.1.1\"\n}");
    assert_eq!(swap_version("[package]\nname = \"x\"\nversion = \"0.1.0\"\n[dependencies]\nfoo = { version = \"1\" }", "0.2.0").unwrap(),
               "[package]\nname = \"x\"\nversion = \"0.2.0\"\n[dependencies]\nfoo = { version = \"1\" }");
    assert!(swap_version("[workspace]\nmembers = []", "1.0.0").is_none());
}
#[cfg(test)]
#[test]
fn png_px_reads_header() {
    let d = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    assert_eq!(png_px(&d.join("icons/32x32.png")), 32);
    assert_eq!(png_px(&d.join("Cargo.toml")), 0);
}

/// Writes a new bundle ID into the config file it was found in (string swap keeps formatting).
#[tauri::command(async)]
fn bundle_id_set(st: St, pid: String, id: String) -> R<()> {
    let ok = id.split('.').count() >= 2 && id.split('.').all(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    if !ok { return Err("use reverse-domain form: letters, digits, - and dots, like com.you.app".into()); }
    let cur = bundle_id(st.clone(), pid.clone())?;
    let (Some(old), Some(file)) = (cur["id"].as_str(), cur["file"].as_str()) else { return Err("no config with a bundle ID (tauri.conf.json identifier / capacitor appId / app.json)".into()) };
    let path = std::path::Path::new(&st.project(&pid).ok_or("no such project")?.path).join(file);
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    std::fs::write(&path, text.replacen(&format!("\"{old}\""), &format!("\"{id}\""), 1)).map_err(|e| e.to_string())
}

/// Plan tab drag: reorders the primary plan's Queue (claude::queue_move); to = null → removes the task (ticked done).
#[tauri::command]
fn plan_move(st: St, pid: String, from: usize, to: Option<usize>, text: Option<String>) -> R<()> {
    claude::queue_move(&st.project(&pid).ok_or("no such project")?.path, from, to, text.as_deref())
}

/// Plan tab suggestion i: accept → moved to the Queue's end, else dismissed (claude::wait_take).
#[tauri::command]
fn plan_wait(st: St, pid: String, i: usize, accept: bool) -> R<()> {
    claude::wait_take(&st.project(&pid).ok_or("no such project")?.path, i, accept)
}

/// Names of the repo's GitHub Actions secrets.
#[tauri::command(async)]
fn gh_secrets(st: St, pid: String) -> R<Vec<String>> {
    let p = st.project(&pid).ok_or("no such project")?;
    let v = store::gh_json(&["secret", "list", "--json", "name"], &p.gh, Some(&p.path));
    Ok(v.as_array().ok_or("gh couldn't list secrets (no repo / no access?)")?.iter().filter_map(|x| x["name"].as_str().map(String::from)).collect())
}

/// .env values of `keys` → repo secrets, via a temp dotenv file (no values on command lines), deleted right after.
#[tauri::command(async)]
fn gh_secrets_set(st: St, pid: String, keys: Vec<String>) -> R<usize> {
    let p = st.project(&pid).ok_or("no such project")?;
    let text = std::fs::read_to_string(std::path::Path::new(&p.path).join(".env")).unwrap_or_default();
    let vals: Vec<(String, String)> = store::env_parse(&text).into_iter().filter(|(k, v)| keys.contains(k) && !v.is_empty()).collect();
    if vals.is_empty() { return Err("none of those keys have a value in .env".into()); }
    let tmp = apple_dir(&pid)?.join("secrets.env");
    std::fs::write(&tmp, vals.iter().map(|(k, v)| format!("{k}={v}\n")).collect::<String>()).map_err(|e| e.to_string())?;
    let o = store::run("gh", &["secret", "set", "-f", &tmp.to_string_lossy()], Some(&p.path), &store::gh_env(&p.gh));
    let _ = std::fs::remove_file(&tmp);
    if o.ok { Ok(vals.len()) } else { Err(o.stderr.trim().to_string()) }
}

// ---- code view ----

/// A path inside the project (no escaping it with ..).
fn inside(root: &str, rel: &str) -> R<std::path::PathBuf> {
    let root = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
    let p = std::fs::canonicalize(root.join(rel)).map_err(|e| e.to_string())?;
    if p.starts_with(&root) { Ok(p) } else { Err("outside the project".into()) }
}

#[tauri::command]
fn list_dir(st: St, pid: String, rel: String) -> R<Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    let dir = inside(&p.path, &rel)?;
    let skip = [".git", "node_modules", "target", "dist", "build", "__pycache__", ".venv", ".next"];
    let mut items: Vec<(bool, String)> = std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten()
        .map(|e| (e.path().is_dir(), e.file_name().to_string_lossy().to_string()))
        .filter(|(d, n)| !(*d && skip.contains(&n.as_str()))).collect();
    items.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.to_lowercase().cmp(&b.1.to_lowercase())));
    Ok(json!(items.iter().map(|(d, n)| json!({"name": n, "dir": d})).collect::<Vec<_>>()))
}

/// #221 Hot files: sizes + 30-day churn (async: git + many stats)
#[tauri::command(async)]
fn file_heat(st: St, pid: String) -> R<Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    Ok(store::file_heat(&p.path))
}

/// Appearance › Size: browser-style zoom, so text, panels and mouse math scale together.
#[tauri::command]
fn set_zoom(w: tauri::WebviewWindow, scale: f64) -> R<()> {
    w.set_zoom(scale.clamp(0.7, 1.6)).map_err(|e| e.to_string())
}

#[tauri::command]
fn read_file(st: St, pid: String, rel: String) -> R<String> {
    let p = st.project(&pid).ok_or("no such project")?;
    let f = inside(&p.path, &rel)?;
    let name = f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    if p.hide_env && (name == ".env" || name.starts_with(".env.")) {
        return Err("Open .env files in the .env tab.".into());
    }
    let bytes = std::fs::read(&f).map_err(|e| e.to_string())?;
    if bytes.len() > 2_000_000 || bytes.iter().take(8000).any(|b| *b == 0) {
        return Err("Binary or too large to show.".into());
    }
    Ok(String::from_utf8_lossy(&bytes).into())
}

/// #285 Edit view: save a tab back to its (existing, in-project) file. New files → save_as.
#[tauri::command(async)]
fn write_file(st: St, pid: String, rel: String, text: String) -> R<()> {
    let p = st.project(&pid).ok_or("no such project")?;
    std::fs::write(inside(&p.path, &rel)?, text).map_err(|e| e.to_string())
}

// ---- #285 Shell tab: the OS shell in a pty, one per project; bytes → event `shell` {pid, d} / {pid, exit} ----
struct Shell { master: Box<dyn portable_pty::MasterPty + Send>, w: Box<dyn std::io::Write + Send>, child: Box<dyn portable_pty::Child + Send + Sync> }
static SHELLS: std::sync::LazyLock<Mutex<std::collections::HashMap<String, Shell>>> = std::sync::LazyLock::new(Default::default);

/// Starts it if not running (false = was running already). async ∵ gh_env spawns gh.
#[tauri::command(async)]
fn shell_open(app: AppHandle, st: St, pid: String, cols: u16, rows: u16) -> R<bool> {
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
fn shell_write(pid: String, d: String) -> R<()> {
    use std::io::Write;
    let mut m = SHELLS.lock().unwrap();
    let s = m.get_mut(&pid).ok_or("shell not running")?;
    s.w.write_all(d.as_bytes()).and_then(|_| s.w.flush()).map_err(|e| e.to_string())
}

#[tauri::command]
fn shell_resize(pid: String, cols: u16, rows: u16) {
    if let Some(s) = SHELLS.lock().unwrap().get(&pid) {
        let _ = s.master.resize(portable_pty::PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
    }
}

#[tauri::command]
fn shell_kill(pid: String) {
    if let Some(mut s) = SHELLS.lock().unwrap().remove(&pid) { let _ = s.child.kill(); }
}

/// Images Claude read (screenshots) → chat thumbnails. Any path ∵ screenshots live in temp/scratchpad, not the project;
/// ∴ image extensions only, ≤ 20 MB. Raw bytes (no base64 dep) → page gets an ArrayBuffer.
#[tauri::command]
fn read_image(path: String) -> R<tauri::ipc::Response> {
    let ext = std::path::Path::new(&path).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    // #247 + pdf: File viewer (blob → iframe); name kept ∵ callers
    if !["png", "jpg", "jpeg", "gif", "webp", "bmp", "pdf"].contains(&ext.as_str()) { return Err("not an image or PDF".into()); }
    if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 50_000_000 { return Err("too large".into()); }
    Ok(tauri::ipc::Response::new(std::fs::read(&path).map_err(|e| e.to_string())?))
}
/// #247 File viewer "Save as…": copy `from` or write `text` to `to` (picked in the save dialog).
#[tauri::command(async)]
fn save_as(to: String, from: Option<String>, text: Option<String>) -> R<()> {
    match (from, text) { (Some(f), _) => std::fs::copy(f, &to).map(|_| ()), (_, Some(t)) => std::fs::write(&to, t), _ => return Err("nothing to save".into()) }
        .map_err(|e| e.to_string())
}

// ---- browsers: open pages in a system browser profile (you stay logged in there) ----

#[tauri::command(async)]
fn browsers() -> Value {
    json!(store::browsers())
}

#[tauri::command(async)]
fn open_in_browser(exe: String, profile: String, url: String) -> R<()> {
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

const REP_KEEP: u64 = 7 * 86400;
fn log_file(pid: &str) -> R<std::path::PathBuf> { rep_dir(pid)?; /* validates pid */ Ok(store::home().join("logs").join(format!("{pid}.json"))) }
/// #243 chat log of a project as the page shows it (capped by the page), read back by open_project.
#[tauri::command(async)]
fn log_save(pid: String, log: Value) -> R<()> {
    let f = log_file(&pid)?;
    std::fs::create_dir_all(f.parent().unwrap()).map_err(|e| e.to_string())?;
    store::write_atomic(&f, log.to_string().as_bytes()).map_err(|e| e.to_string())
}

fn rep_dir(pid: &str) -> R<std::path::PathBuf> {
    if pid.is_empty() || !pid.chars().all(|c| c.is_ascii_alphanumeric() || "-_".contains(c)) { return Err("bad project id".into()); }
    Ok(store::home().join("reports").join(pid))
}
/// Reports of the last 7 days, oldest first; older files deleted on the way.
fn rep_list(pid: &str) -> R<Vec<Value>> {
    let d = rep_dir(pid)?;
    let mut l: Vec<(std::path::PathBuf, Value)> = std::fs::read_dir(&d).into_iter().flatten().flatten().filter_map(|f| {
        let p = f.path();
        if p.extension()? != "json" { return None; }
        let age = f.metadata().ok()?.modified().ok()?.elapsed().map(|e| e.as_secs()).unwrap_or(0);
        if age > REP_KEEP { let _ = std::fs::remove_file(&p); return None; }
        Some((p.clone(), serde_json::from_str(&std::fs::read_to_string(&p).ok()?).ok()?))
    }).collect();
    l.sort_by(|a, b| a.0.cmp(&b.0)); // names = ms timestamps
    Ok(l.into_iter().map(|x| x.1).collect())
}

#[tauri::command(async)]
fn report_save(pid: String, rep: Value) -> R<()> {
    let d = rep_dir(&pid)?;
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let id = rep["id"].as_u64().unwrap_or((store::now() * 1000.0) as u64); // = UI rid (Date.now) → report_get
    std::fs::write(d.join(format!("{id}.json")), rep.to_string()).map_err(|e| e.to_string())?;
    rep_list(&pid).map(|_| ()) // prunes
}

#[tauri::command(async)]
fn report_get(pid: String, id: u64) -> R<Value> {
    let t = std::fs::read_to_string(rep_dir(&pid)?.join(format!("{id}.json"))).map_err(|e| e.to_string())?;
    serde_json::from_str(&t).map_err(|e| e.to_string())
}

/// One-shot `claude -p` on Sonnet with the project's AI account → trimmed stdout. cwd = a dir with no CLAUDE.md/hooks ∴ cheap; no tools.
fn sonnet(st: &State, p: &store::Project, effort: &str, cwd: &std::path::Path, prompt: &str) -> R<String> {
    use std::io::Write;
    let claude = store::which("claude").ok_or("Claude Code isn't installed")?;
    let base = st.cfg.lock().unwrap().accounts.iter().find(|a| a.id == p.account).map(|a| a.base_url.clone()).unwrap_or_default();
    let mut c = store::command(&claude);
    for k in ["CLAUDE_CODE_OAUTH_TOKEN", "ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL", "CLAUDECODE"] { c.env_remove(k); }
    c.args(["-p", "--model", "sonnet", "--effort", effort, "--output-format", "text"]).envs(store::account_env(&p.account, &base))
        .current_dir(cwd).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let mut ch = c.spawn().map_err(|e| e.to_string())?;
    track(ch.id());
    ch.stdin.take().unwrap().write_all(prompt.as_bytes()).map_err(|e| e.to_string())?;
    let o = ch.wait_with_output().map_err(|e| e.to_string())?;
    let out = String::from_utf8_lossy(&o.stdout).trim().to_string();
    if !o.status.success() || out.is_empty() { return Err(String::from_utf8_lossy(&o.stderr).trim().to_string()); }
    Ok(out)
}

/// rep = one task's report, None = the 7-day report. Sonnet (medium effort) writes the prose as an HTML fragment, a
/// headless Chromium (Edge/Chrome) prints it → PDF in ~/.clauracle/reports/<pid>/pdf, opened. ponytail: no browser → Err.
#[tauri::command(async)]
fn report_pdf(st: St, pid: String, rep: Option<Value>) -> R<String> {
    let p = st.project(&pid).ok_or("project removed")?;
    let week = rep.is_none();
    let reps = match rep { Some(r) => vec![r], None => rep_list(&pid)? };
    if reps.is_empty() { return Err("No reports from the last 7 days yet: one is saved each time a task finishes.".into()); }
    let mut data = serde_json::to_string(&reps).unwrap_or_default();
    if data.len() > 80_000 { data = data.chars().take(80_000).collect(); } // ponytail: cut, Sonnet still sees most of the week
    let what = if week { "the last 7 days of work (one JSON object per finished task)" } else { "one finished task" };
    let prompt = format!("Write a short, sleek report for a team meeting about {what} on the software project \"{}\". \
        Output ONLY an HTML fragment (no <html>/<head>/<body>, no markdown, no code fences). Sections as <h2>: Summary (2-3 sentences), \
        What was done (<ul>), Problems spotted (failed commands, missing tools; skip if none), Time (a small <table> of tasks and durations), \
        Suggestions (2-4 concrete next steps). Plain business tone, no filler. Data:\n{data}", p.name);
    let d = rep_dir(&pid)?.join("pdf");
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let body = sonnet(&st, &p, "medium", &d, &prompt).map_err(|e| format!("Report writer failed: {e}"))?;
    let body = body.trim_start_matches("```html").trim_matches('`').trim().to_string();
    let logo = include_str!("../../assets/icon.svg");
    let logo = &logo[logo.find("<svg").unwrap_or(0)..];
    let date = chrono_ish();
    let html = format!(r#"<!doctype html><meta charset="utf-8"><title>{n} report</title><style>
@page{{margin:18mm}}body{{font:11pt/1.5 "Segoe UI",system-ui,sans-serif;color:#222;max-width:720px;margin:auto}}
header{{display:flex;gap:14px;align-items:center;border-bottom:3px solid #D77655;padding-bottom:12px;margin-bottom:8px}}
header svg{{width:48px;height:48px}}h1{{margin:0;font-size:20pt}}header small{{color:#777}}
h2{{color:#D77655;font-size:13pt;margin:20px 0 6px}}table{{border-collapse:collapse;width:100%}}td,th{{border-bottom:1px solid #eee;padding:4px 6px;text-align:left}}
code{{background:#f4f1ef;padding:0 4px;border-radius:3px}}</style>
<header>{logo}<div><small>Clauracle · {kind}</small><h1>{n}</h1><small>{date}</small></div></header>{body}"#,
        n = html_esc(&p.name), kind = if week { "7-day report" } else { "Task report" });
    let stem = format!("{}-{}", if week { "7-day" } else { "task" }, (store::now()) as u64);
    let hf = d.join(format!("{stem}.html"));
    let pf = d.join(format!("{stem}.pdf"));
    std::fs::write(&hf, html).map_err(|e| e.to_string())?;
    let exe = store::browsers().iter().find_map(|b| b["exe"].as_str().map(String::from)).ok_or("No Chrome/Edge found to print the PDF")?;
    let r = std::process::Command::new(&exe).args(["--headless=new", "--disable-gpu", "--no-pdf-header-footer"])
        .arg(format!("--print-to-pdf={}", pf.display())).arg(format!("file:///{}", hf.display().to_string().replace('\\', "/"))).output();
    if !pf.exists() { return Err(format!("PDF print failed{}", r.err().map(|e| format!(": {e}")).unwrap_or_default())); }
    Ok(pf.to_string_lossy().into())
}
fn html_esc(s: &str) -> String { s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;") }
/// "2026-11-08 14:05 UTC" w/o a date crate: civil-from-days (Hinnant).
fn chrono_ish() -> String { civil(store::now() as i64) }
fn civil(s: i64) -> String {
    let (z, t) = (s.div_euclid(86400) + 719468, s.rem_euclid(86400));
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let dd = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{}-{m:02}-{dd:02} {:02}:{:02} UTC", yoe + era * 400 + (m <= 2) as i64, t / 3600, t % 3600 / 60)
}

/// In-app browser: one child webview "web" laid over the Browser tab's placeholder. Profile dir ~/.clauracle/browser,
/// shared by all projects ∴ logins persist + are shared. rect None → hide. nav → load url (else only move/resize).
/// No IPC for it: capabilities need `remote.urls` for external pages. Its navigations → event `weburl` (address bar).
#[tauri::command(async)]
fn web_view(app: AppHandle, url: String, rect: Option<[f64; 4]>, zoom: f64, nav: bool) -> R<()> {
    use tauri::{LogicalPosition as P, LogicalSize as Z};
    let e = |e: tauri::Error| e.to_string();
    let wv = app.get_webview("web");
    let Some([x, y, w, h]) = rect else { return wv.map_or(Ok(()), |v| v.hide().map_err(e)) };
    let u: tauri::Url = url.parse().map_err(|_| "bad address")?;
    if !matches!(u.scheme(), "http" | "https") { return Err("http(s) links only".into()); }
    let v = match wv {
        Some(v) => {
            v.set_position(P::new(x, y)).map_err(e)?;
            v.set_size(Z::new(w, h)).map_err(e)?;
            if nav { v.navigate(u).map_err(e)?; }
            v
        }
        None => {
            let a = app.clone();
            let b = tauri::webview::WebviewBuilder::new("web", tauri::WebviewUrl::External(u))
                .data_directory(dirs::home_dir().unwrap_or_default().join(".clauracle/browser"))
                .on_navigation(move |u| { emit(&a, "weburl", json!(u.as_str())); true });
            app.get_window("main").ok_or("no main window")?.add_child(b, P::new(x, y), Z::new(w, h)).map_err(e)?
        }
    };
    v.set_zoom(zoom).map_err(e)?;
    v.show().map_err(e)
}

/// Claude settings file: scope user = ~/.claude/settings.json, project/local = <proj>/.claude/settings(.local).json.
fn cs_file(st: &St, scope: &str, pid: &str) -> R<std::path::PathBuf> {
    let dir = |p: &str| std::path::PathBuf::from(p).join(".claude");
    if let Some(id) = scope.strip_prefix("account:") {
        if !st.cfg.lock().unwrap().accounts.iter().any(|a| a.id == id) { return Err("no such account".into()); }
        return store::account_settings(id).ok_or_else(|| "bad account id".into());
    }
    Ok(match scope {
        "user" => dirs::home_dir().ok_or("no home")?.join(".claude"),
        "project" | "local" => dir(&st.project(pid).ok_or("no such project")?.path),
        _ => return Err("bad scope".into()),
    }.join(if scope == "local" { "settings.local.json" } else { "settings.json" }))
}

/// Settings editor: read → {path, text, json}; missing file = {}. Unparsable → json null + raw text (editor shows raw, never overwrites silently).
#[tauri::command]
fn claude_settings(st: St, scope: String, pid: String) -> R<Value> {
    let f = cs_file(&st, &scope, &pid)?;
    let txt = std::fs::read_to_string(&f).unwrap_or_else(|_| "{}".into());
    let v: Option<Value> = serde_json::from_str(txt.trim_start_matches('\u{feff}')).ok().filter(Value::is_object);
    Ok(json!({"path": f.to_string_lossy(), "text": txt.trim_start_matches('\u{feff}'), "json": v}))
}

/// Settings editor: write a whole object (key order kept: serde_json preserve_order). Non-object → refused.
#[tauri::command]
fn claude_settings_set(st: St, scope: String, pid: String, value: Value) -> R<()> {
    if !value.is_object() { return Err("settings must be a JSON object".into()); }
    let f = cs_file(&st, &scope, &pid)?;
    std::fs::create_dir_all(f.parent().unwrap()).map_err(|e| e.to_string())?;
    store::write_atomic(&f, (serde_json::to_string_pretty(&value).unwrap() + "\n").as_bytes()).map_err(|e| e.to_string())?; // crash mid-write ≠ truncated ~/.claude/settings.json
    // account file is read at spawn → its sessions reconnect after the current reply (user/project files: the CLI reads them live)
    if let Some(id) = scope.strip_prefix("account:") {
        let pids: Vec<String> = st.cfg.lock().unwrap().projects.iter().filter(|p| p.account == id).map(|p| p.id.clone()).collect();
        for (pid, l) in st.live.lock().unwrap().iter() { if pids.contains(pid) { l.inbox.front(vec![claude::Input::Reconnect]); } }
    }
    Ok(())
}

/// Phone preview: does host:port accept a TCP connection (dev server bound to 0.0.0.0, not only localhost)?
#[tauri::command]
async fn reach(host: String, port: u16) -> bool {
    use std::net::ToSocketAddrs;
    tauri::async_runtime::spawn_blocking(move || (host.as_str(), port).to_socket_addrs().ok().and_then(|mut a| a.next())
        .is_some_and(|a| std::net::TcpStream::connect_timeout(&a, std::time::Duration::from_millis(600)).is_ok()))
        .await.unwrap_or(false)
}

/// Start page for the in-app browser: the browser profile's homepage (or first startup page) from its Preferences.
#[tauri::command(async)]
fn homepage(exe: String, profile: String) -> String {
    store::browsers().iter().find(|b| b["exe"] == exe.as_str())
        .and_then(|b| std::fs::read_to_string(std::path::Path::new(b["data"].as_str()?).join(if profile.is_empty() { "Default" } else { &profile }).join("Preferences")).ok())
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|p| p["homepage"].as_str().or(p["session"]["startup_urls"][0].as_str()).map(String::from))
        .filter(|u| u.starts_with("http")).unwrap_or_default()
}

/// Main window geometry → config (close, quit, restart): [x, y, w, h, mode] physical px, mode 0 windowed | 1 maximized | 2 fullscreen.
/// Max/fullscreen keep the last normal rect ∵ restore needs it, moved onto the CURRENT monitor if it sits elsewhere
/// (window dragged to another screen then maximized) → restore maximizes on the right monitor.
fn save_win(app: &AppHandle) {
    let (Some(w), Some(st)) = (app.get_window("main"), app.try_state::<Arc<State>>()) else { return };
    if w.is_minimized().unwrap_or(false) { return; }
    let mode = if w.is_fullscreen().unwrap_or(false) { 2 } else { w.is_maximized().unwrap_or(false) as i32 };
    let mut cfg = st.cfg.lock().unwrap();
    let r = match (mode, cfg.win, w.outer_position(), w.inner_size()) {
        (1 | 2, Some(o), ..) => {
            let mut o = [o[0], o[1], o[2], o[3], mode];
            if let Ok(Some(m)) = w.current_monitor() {
                let (p, s) = (m.position(), m.size());
                if !in_mon(p.x, p.y, s.width, s.height, o[0], o[1]) {
                    o[2] = o[2].min(s.width as i32 - 80);
                    o[3] = o[3].min(s.height as i32 - 80);
                    o[0] = p.x + 40;
                    o[1] = p.y + 40;
                }
            }
            o
        }
        (_, _, Ok(p), Ok(s)) => [p.x, p.y, s.width as i32, s.height as i32, mode],
        _ => return,
    };
    if cfg.win != Some(r) { cfg.win = Some(r); store::save(&cfg); }
}

/// Is the window's top-left (+40,+10 grab margin) on the monitor at (mx, my) of size w×h?
fn in_mon(mx: i32, my: i32, w: u32, h: u32, x: i32, y: i32) -> bool {
    (mx..mx + w as i32).contains(&(x + 40)) && (my..my + h as i32).contains(&(y + 10))
}

/// Saved geometry back, unless its top-left is on no monitor any more (unplugged screen); then max/fullscreen
/// (Windows applies both on the monitor the window sits on).
fn restore_win(app: &tauri::App, r: [i32; 5]) {
    let Some(w) = app.get_window("main") else { return };
    let on = w.available_monitors().unwrap_or_default().iter().any(|m| {
        let (p, s) = (m.position(), m.size());
        in_mon(p.x, p.y, s.width, s.height, r[0], r[1])
    });
    if on {
        let _ = w.set_position(tauri::PhysicalPosition::new(r[0], r[1]));
    }
    let _ = w.set_size(tauri::PhysicalSize::new(r[2].max(900) as u32, r[3].max(560) as u32));
    match r[4] { 1 => { let _ = w.maximize(); } 2 => { let _ = w.set_fullscreen(true); } _ => {} }
}

/// F11: fullscreen on/off (saved by save_win as mode 2).
#[tauri::command]
fn fullscreen(app: AppHandle) {
    if let Some(w) = app.get_window("main") { let _ = w.set_fullscreen(!w.is_fullscreen().unwrap_or(false)); }
}

/// Start `exe` (detached, it outlives us) and quit; the new instance resumes cut-off turns (claude::resume_interrupted).
pub fn relaunch(app: &AppHandle, exe: std::path::PathBuf, args: &[String]) {
    save_win(app);
    kill_children(); // sessions end before the new instance resumes them
    let mut c = std::process::Command::new(exe);
    c.args(args).args(["--after", &std::process::id().to_string()]); // new one waits for us ∵ single-instance guard
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x8 | 0x200); // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
    }
    if c.spawn().is_ok() {
        app.exit(0);
        // app.exit can hang (seen 2026-11-05: old instance stayed → helper timed out → 2 Clauracles) → hard exit
        std::thread::spawn(|| { std::thread::sleep(std::time::Duration::from_secs(3)); std::process::exit(0) });
    }
}

/// Swap helper (a new build started with `--swap-into <installed exe>`): copy itself next to the target, rename it
/// over the target once the old app has exited (rename fails while Windows holds the exe), start it, exit.
/// Still locked after ~8s → an instance didn't exit: kill every other clauracle.exe (never this helper), retry ~8s.
/// No /T ∵ helper = child of the old instance; its leftover claude.exe → startup orphan check offers to end them.
/// Then starts whatever is there (old build if all failed) so the app always comes back — as the ONLY instance.
fn swap_into(dest: &std::path::Path) {
    let tmp = dest.with_extension("new");
    if let Ok(me) = std::env::current_exe() {
        if std::fs::copy(&me, &tmp).is_ok() {
            for i in 0..80 {
                if std::fs::rename(&tmp, dest).is_ok() { break; }
                if i == 40 && cfg!(windows) {
                    let (img, me_pid) = (dest.file_name().unwrap_or_default().to_string_lossy().into_owned(), std::process::id());
                    let _ = store::command(std::path::Path::new("taskkill")).args(["/F", "/FI", &format!("IMAGENAME eq {img}"), "/FI", &format!("PID ne {me_pid}")]).output();
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            let _ = std::fs::remove_file(&tmp);
        }
    }
    let mut c = std::process::Command::new(dest);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x8 | 0x200);
    }
    let _ = c.spawn();
}

#[tauri::command]
fn save_draft(pid: String, draft: Value) {
    store::save_draft(&pid, draft);
}

/// #283 Start menu › Shut down (Exit event kills child trees)
#[tauri::command]
fn quit(app: AppHandle) { app.exit(0) }

#[tauri::command(async)]
fn restart(app: AppHandle) {
    if let Ok(me) = std::env::current_exe() {
        relaunch(&app, me, &[]);
    }
}

/// Ends every tracked child tree, but only pids still alive with a child-like name ∵ the list never shrinks and
/// Windows reuses pids fast → a blind taskkill /T could take down the user's browser/IDE.
fn kill_children() {
    let pids: Vec<u32> = CHILDREN.lock().unwrap().drain(..).collect();
    if !pids.is_empty() {
        let live = procs();
        for p in pids { if live.get(&p).is_some_and(|n| CHILDISH.contains(&n.as_str())) { kill_tree(p); } } }
    save_children(&[]);
}
const CHILDISH: [&str; 9] = ["claude", "node", "cmd", "sh", "bash", "zsh", "ssh", "wsl", "npm"];

pub(crate) fn kill_tree(pid: u32) {
    if cfg!(windows) {
        let _ = store::command(std::path::Path::new("taskkill")).args(["/F", "/T", "/PID", &pid.to_string()]).output();
    } else {
        // group first (build/run steps lead their own, claude::shell); plain pid for the rest (claude CLI)
        let _ = std::process::Command::new("kill").args(["-TERM", "--", &format!("-{pid}")]).output();
        let _ = std::process::Command::new("kill").arg(pid.to_string()).output();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--swap-into") {
        return swap_into(std::path::Path::new(args.get(2).map_or("", String::as_str)));
    }
    store::fix_path();
    // relaunched: old instance still exiting (≤3s hard exit) → wait ≤10s, else the guard below would hand off to it and quit
    if let Some(old) = args.iter().position(|a| a == "--after").and_then(|i| args.get(i + 1)?.parse::<u32>().ok()) {
        for _ in 0..50 {
            if !procs().contains_key(&old) { break; }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
    // Bundle ID moved no.salnorth → no.arctim (v0.1.1): carry WebView2 data (localStorage drafts/folds) once.
    // ponytail: Windows only; mac/Linux had no real users before the move. Drop after a few releases.
    #[cfg(windows)]
    if let Ok(la) = std::env::var("LOCALAPPDATA") {
        let (old, new) = (std::path::Path::new(&la).join("no.salnorth.clauracle"), std::path::Path::new(&la).join("no.arctim.clauracle"));
        // retry ≤3s ∵ old instance's msedgewebview2 children may still hold the folder right after a swap
        for _ in 0..10 {
            if !old.is_dir() || new.exists() || std::fs::rename(&old, &new).is_ok() { break; }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
    }
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(w) = app.get_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let plugin_dir = app.path().resource_dir().map(|d| d.join("plugin")).ok().filter(|d| d.exists())
                .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../plugin")); // dev runs
            app.manage(Arc::new(State { cfg: Mutex::new(store::load()), live: Default::default(),
                                        asks: Default::default(), plugin_dir }));
            let st = app.state::<Arc<State>>().inner().clone();
            *ORPHANS.lock().unwrap() = find_orphans(); // before resume spawns (track overwrites the file)
            if let Some(r) = st.cfg.lock().unwrap().win { restore_win(app, r); }
            claude::resume_interrupted(app.handle(), &st);
            claude::resume_builds(app.handle(), &st);
            // warm the slow lookups so Settings opens instantly: gh accounts, and the catalog on a first run
            let st2 = st.clone();
            // housekeeping: old update download; Storage Janitor now + every JANITOR_EVERY (rules: store::janitor)
            let st3 = st.clone();
            let app3 = app.handle().clone();
            std::thread::spawn(move || {
                let _ = std::fs::remove_dir_all(std::env::temp_dir().join("clauracle-update"));
                std::thread::sleep(std::time::Duration::from_secs(60)); // page listening by then → toast not lost
                loop {
                    let (ps, keep): (Vec<(String, bool)>, Vec<String>) = {
                        let c = st3.cfg.lock().unwrap();
                        (c.projects.iter().map(|p| (p.path.clone(), p.working)).collect(),
                         c.projects.iter().flat_map(|p| [p.session.clone(), p.prev_session.clone()]).flatten().collect())
                    };
                    let freed = store::janitor(&ps, &keep);
                    if freed >= 100 << 20 { claude::emit(&app3, "janitor", serde_json::json!({"freed": freed})); } // ≥100 MB → toast; less = noise
                    std::thread::sleep(std::time::Duration::from_secs(store::JANITOR_EVERY));
                }
            });
            tauri::async_runtime::spawn(async move {
                let _ = tauri::async_runtime::spawn_blocking(store::gh_users).await;
                // every start (background; files only, no server launched #155): new servers/plugins/skills get known → denied/off
                // unless picked. Changed → sessions reconnect after their current reply so their CLI settings follow.
                let dir = st2.plugin_dir.clone();
                let (paths, old) = { let c = st2.cfg.lock().unwrap(); (c.projects.iter().map(|p| p.path.clone()).collect::<Vec<_>>(), c.catalog.clone()) };
                if let Ok(items) = tauri::async_runtime::spawn_blocking(move || store::catalog(&dir, &paths, &old)).await {
                    let ids = |c: &[store::Item]| c.iter().map(|i| i.id.clone()).collect::<Vec<_>>();
                    let changed = {
                        let mut cfg = st2.cfg.lock().unwrap();
                        let changed = ids(&cfg.catalog) != ids(&items);
                        cfg.catalog = items;
                        store::save(&cfg);
                        changed
                    };
                    if changed {
                        for l in st2.live.lock().unwrap().values() { l.inbox.push(claude::Input::Reconnect); }
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            fullscreen, state, ui_reply, open_project, add_project, remove_project, update_project, send, interrupt,
            continue_now, set_lessons, set_you, report_save, report_get, log_save, report_pdf, save_as, qr, lan_ip, avatar, prepare_handover, answer_ask, answer_questions, settings, refresh_catalog, ext_find, ext_add, set_off,
            update_account, set_default_account, claude_login, claude_add_token, claude_remove, set_notify,
            test_notify, install, fix_missing, deps, place_of, scan_folder, move_folder, gh_login, gh_logout, gh_info, actions_usage, gh_repos, gh_branches, clone_repo, run_step, build_guess, stop_run, stop_build, restart_into, send_mid,
            env_list, env_set, env_ignore, apple_csr, apple_cer_find, apple_p12, ships_mac, bundle_id, bundle_id_set, site_check, site_auto, site_key_install, version_set, release_check, plan_move, plan_wait, gh_secrets, gh_secrets_set,list_dir, file_heat, read_file, write_file, shell_open, shell_write, shell_resize, shell_kill, read_image, browsers, open_in_browser, web_view, homepage, reach, claude_settings, claude_settings_set, agents, set_agents, who, save_draft, restart, quit, publish_repo, gcloud_info, gcloud_login, gcloud_do, team, task_do, git_do, release, stage, disk, disk_clean, update_check, update_run, set_update_every, set_items, set_ci_builds, set_mac, mac_test, mac_setup, mac_key, mac_xcode, set_projects_dir, new_local, set_zoom, orphans])
        .build(tauri::generate_context!())
        .expect("failed to start Clauracle");
    app.run(|app, e| {
        if let tauri::RunEvent::WindowEvent { event: tauri::WindowEvent::CloseRequested { .. }, .. } | tauri::RunEvent::ExitRequested { .. } = e {
            save_win(app);
        }
        if let tauri::RunEvent::Exit = e {
            // claude processes and run steps don't die with us on Windows: take their whole trees down
            kill_children();
        }
    });
}

#[cfg(test)]
#[test]
fn procs_lists_self() {
    let me = std::process::id();
    assert!(procs().contains_key(&me), "own pid missing from process list");
}

#[cfg(test)]
#[test]
fn newer_versions() {
    assert!(newer("v0.2.0", "0.1.9") && newer("0.1.10", "0.1.9") && !newer("v0.1.0", "0.1.0") && !newer("0.0.9", "0.1.0"));
}

#[cfg(test)]
#[test]
fn toml_versions() {
    assert_eq!(toml_version("[package]\nname = \"x\"\nversion = \"0.1.2\"\n").as_deref(), Some("0.1.2"));
    assert_eq!(toml_version("[package]\nversion.workspace = true\n"), None);
}

#[cfg(test)]
mod rep_tests {
    #[test]
    fn civil_dates() {
        assert_eq!(super::civil(0), "1970-01-01 00:00 UTC");
        assert_eq!(super::civil(1_794_150_300), "2026-11-08 15:05 UTC");
        assert_eq!(super::civil(951_782_400), "2000-02-29 00:00 UTC");
    }
}
