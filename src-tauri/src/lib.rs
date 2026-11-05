//! Clauracle: Tauri commands the page calls. The work lives in claude.rs (sessions) and store.rs (everything else).
mod claude;
mod store;

use claude::{emit, session, Input, State};
use serde_json::{json, Value};
use std::sync::atomic::Ordering::SeqCst;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State as S};

type St<'a> = S<'a, Arc<State>>;
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
        .filter_map(|p: u32| live.get(&p).filter(|n| ["claude", "node", "cmd", "sh", "bash"].contains(&n.as_str())).map(|n| (p, n.clone())))
        .collect()
}

/// Orphans from a crashed earlier run; `kill` ends their trees.
#[tauri::command]
fn orphans(kill: bool) -> Vec<Value> {
    let mut o = ORPHANS.lock().unwrap();
    if kill { o.drain(..).for_each(|(p, _)| kill_tree(p)); }
    o.iter().map(|(p, n)| json!({"pid": p, "name": n})).collect()
}

#[tauri::command]
fn state(st: St) -> Value {
    let cfg = st.cfg.lock().unwrap();
    let usage: serde_json::Map<String, Value> =
        cfg.accounts.iter().map(|a| (a.id.clone(), json!(store::usage(&cfg, &a.id)))).collect();
    // when each usage window resets (unix secs), per account: shown in the usage cards, toast when one passes
    let resets: serde_json::Map<String, Value> = cfg.usage.iter().map(|(id, w)|
        (id.clone(), json!(w.iter().filter_map(|(k, v)| v.resets_at.map(|r| (k.clone(), r))).collect::<std::collections::HashMap<_, _>>()))).collect();
    let live = st.live.lock().unwrap();
    let busy: Vec<&String> = live.iter().filter(|(_, l)| l.busy.load(SeqCst)).map(|(k, _)| k).collect();
    json!({"projects": cfg.projects, "accounts": cfg.accounts, "usage": usage, "resets": resets,"active": cfg.active, "busy": busy,
           "default_account": cfg.default_account, "agents_on": cfg.agents_on, "web": !cfg.off.iter().any(|x| x == "web"), "lessons": cfg.lessons,"notify": cfg.notify, "drafts": store::drafts(),
           "projects_dir": store::projects_root(&cfg).to_string_lossy(),
           // next steps from PLAN.md (ready bar, Tasks tab), every plan file with tasks (Plan tab ⓘ / merge)
           // + the last few done from CHANGES.md (Plan tab)
           "queues": cfg.projects.iter().map(|p| (p.id.clone(), json!(claude::queue(&p.path))))
               .collect::<serde_json::Map<String, Value>>(),
           "plans": cfg.projects.iter().map(|p| (p.id.clone(), json!(claude::plans(&p.path))))
               .collect::<serde_json::Map<String, Value>>(),
           "done": cfg.projects.iter().map(|p| (p.id.clone(), json!(claude::done(&p.path, 3))))
               .collect::<serde_json::Map<String, Value>>(),
           "claude": store::which("claude").is_some(), "gh": store::which("gh").is_some()})
}

#[tauri::command]
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
    Ok(json!({"branch": store::git_branch(&p.path), "busy": live.busy.load(SeqCst), "history": history}))
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
    let p = mark(&mut cfg, &p.id, who.as_ref());
    store::save(&cfg);
    Ok(json!(p))
}

fn mark(cfg: &mut store::Config, pid: &str, who: Option<&store::Who>) -> store::Project {
    let p = cfg.projects.iter_mut().find(|p| p.id == pid).unwrap();
    if let Some(w) = who {
        (p.owner, p.shared) = (w.owner.clone(), w.shared);
    }
    p.clone()
}

/// After opening: refresh owner/shared, and say whether the suggestion differs from what the project uses.
#[tauri::command]
async fn who(st: St<'_>, pid: String) -> R<Value> {
    let path = st.project(&pid).ok_or("no such project")?.path;
    let Some(w) = detect(&st, path).await else { return Ok(Value::Null) };
    let mut cfg = st.cfg.lock().unwrap();
    let p = mark(&mut cfg, &pid, Some(&w));
    store::save(&cfg);
    let differs = !p.keep && !w.gh.is_empty() && (!w.gh.eq_ignore_ascii_case(&p.gh) || w.account != p.account);
    Ok(json!({"who": w, "differs": differs}))
}

#[tauri::command]
fn remove_project(st: St, pid: String) {
    let mut cfg = st.cfg.lock().unwrap();
    cfg.projects.retain(|p| p.id != pid);
    store::save(&cfg);
    if let Some(l) = st.live.lock().unwrap().remove(&pid) {
        l.inbox.front(vec![Input::Reconnect]); // its loop sees the project gone and ends
    }
}

/// Merge changed fields into the project; anything that shapes the claude process reconnects it (after the
/// current reply, same session).
#[tauri::command]
fn update_project(app: AppHandle, st: St, pid: String, changes: Value) -> R<Value> {
    let quiet = ["name", "context_limit", "build", "keep", "services", "allow"]; // read live per call: no reconnect
    let p = st.with_project(&pid, |p| {
        let mut v = json!(p);
        for (k, val) in changes.as_object().cloned().unwrap_or_default() {
            if !["id", "path", "session", "questions"].contains(&k.as_str()) {
                v[k] = val;
            }
        }
        if let Ok(n) = serde_json::from_value(v) {
            *p = n;
        }
        p.clone()
    }).ok_or("no such project")?;
    if changes.as_object().map_or(false, |o| o.keys().any(|k| !quiet.contains(&k.as_str()))) {
        if let Some(l) = st.live.lock().unwrap().get(&pid) {
            l.inbox.front(vec![Input::Reconnect]);
        }
    }
    emit(&app, "project", json!(p));
    Ok(json!(p))
}

/// images: pasted ones as [{media_type, data(base64)}]; files: paths, mentioned so Claude reads only what it needs.
#[tauri::command]
fn send(app: AppHandle, st: St, pid: String, text: String, images: Option<Vec<Value>>, files: Option<Vec<String>>) {
    let live = session(&app, &st, &pid);
    let (images, files) = (images.unwrap_or_default(), files.unwrap_or_default());
    if text.trim() == "/clear" && images.is_empty() && files.is_empty() {
        return claude::fresh(&app, &st, &pid, &live, false);
    }
    let text = if files.is_empty() { text } else {
        format!("{text}\n\nAttached files (read them as needed):\n{}", files.iter().map(|f| format!("- {f}")).collect::<Vec<_>>().join("\n"))
    };
    if images.is_empty() {
        return live.inbox.push(Input::Msg(text));
    }
    let mut blocks: Vec<Value> = images.iter().map(|i| json!({"type": "image", "source": {"type": "base64",
        "media_type": i["media_type"], "data": i["data"]}})).collect();
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

#[tauri::command]
async fn interrupt(st: St<'_>, pid: String) -> R<()> {
    let conn = st.live.lock().unwrap().get(&pid).and_then(|l| l.conn.lock().unwrap().clone());
    if let Some(c) = conn {
        c.request(json!({"subtype": "interrupt"}), 10).await?;
    }
    Ok(())
}

#[tauri::command]
fn new_session(app: AppHandle, st: St, pid: String) {
    let live = session(&app, &st, &pid);
    claude::fresh(&app, &st, &pid, &live, false);
}

/// The Continue button: clear the context and carry on from the handover (with any answers).
/// QR code as an SVG string (Browser tab, phone preview).
#[tauri::command]
fn qr(text: String) -> R<String> {
    let c = qrcode::QrCode::new(text.as_bytes()).map_err(|e| e.to_string())?;
    // 2px per module, no quiet zone (the page pads it): sharp at 1:1, scaled up on click
    Ok(c.render::<qrcode::render::svg::Color>().module_dimensions(2, 2).quiet_zone(false).build())
}

/// This machine's LAN address (the route a phone on the same Wi-Fi would use); UDP connect sends nothing.
#[tauri::command]
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
fn continue_now(app: AppHandle, st: St, pid: String, extra: Option<String>) {
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
    claude::fresh(&app, &st, &pid, &live, true);
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
#[tauri::command]
fn answer_questions(app: AppHandle, st: St, pid: String, answers: Vec<Value>) {
    let ids: Vec<&str> = answers.iter().filter_map(|a| a["id"].as_str()).collect();
    let (text, send_now) = st.with_project(&pid, |p| {
        let text = claude::answers_text(&p.questions, &answers);
        p.questions.retain(|q| !ids.contains(&q.id.as_str()));
        (text, p.cont == "off" && !p.ready)
    }).unwrap_or_default();
    let live = session(&app, &st, &pid);
    if send_now && !live.busy.load(SeqCst) {
        live.inbox.push(Input::Msg(text));
    } else {
        st.with_project(&pid, |p| p.answers = [p.answers.as_str(), text.as_str()].join("\n").trim().to_string());
    }
    emit(&app, "project", json!(st.project(&pid)));
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
    let items = tauri::async_runtime::spawn_blocking(move || store::catalog(&dir)).await.map_err(|e| e.to_string())?;
    let mut cfg = st.cfg.lock().unwrap();
    cfg.catalog = items;
    store::save(&cfg);
    Ok(json!(cfg.catalog))
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
    claude_add_token(st, nick, token)
}

#[tauri::command]
fn claude_add_token(st: St, nick: String, token: String) -> R<Value> {
    if !store::token_ok(&token) {
        return Err("That doesn't look like a token from `claude setup-token` (sk-ant-oat…).".into());
    }
    let mut cfg = st.cfg.lock().unwrap();
    let a = store::add_account(&mut cfg, &nick, &token);
    store::save(&cfg);
    Ok(json!(a))
}

#[tauri::command]
fn claude_remove(st: St, id: String) {
    let mut cfg = st.cfg.lock().unwrap();
    store::remove_account(&mut cfg, &id);
    store::save(&cfg);
}

#[tauri::command]
fn set_notify(st: St, notify: Value) {
    let mut cfg = st.cfg.lock().unwrap();
    if let Ok(n) = serde_json::from_value(notify) {
        cfg.notify = n;
    }
    store::save(&cfg);
}

#[tauri::command]
fn test_notify(app: AppHandle, st: St) {
    claude::notify(&app, &st, "Clauracle", "Notifications work.");
}

#[tauri::command]
async fn install(what: String) -> R<()> {
    tauri::async_runtime::spawn_blocking(move || match what.as_str() {
        "gh" => store::install_gh(), "gcloud" => store::install_gcloud(), _ => store::install_claude() })
        .await.map_err(|e| e.to_string())?
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
async fn gh_login(app: AppHandle) -> R<bool> {
    store::forget_gh_users();
    tauri::async_runtime::spawn_blocking(move || {
        use std::io::BufRead;
        let gh = store::which("gh").ok_or("gh isn't installed")?;
        let mut child = store::command(&gh)
            .args(["auth", "login", "--web", "-h", "github.com", "-p", "https", "--skip-ssh-key"])
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
        Ok(child.wait().map(|s| s.success()).unwrap_or(false))
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn gh_logout(user: String) -> bool {
    store::forget_gh_users();
    tauri::async_runtime::spawn_blocking(move || store::run("gh", &["auth", "logout", "-h", "github.com", "-u", &user], None, &[]).ok)
        .await.unwrap_or(false)
}

#[tauri::command]
fn set_projects_dir(st: St, dir: String) {
    let mut cfg = st.cfg.lock().unwrap();
    cfg.projects_dir = dir;
    store::save(&cfg);
}

/// A new, local-only project: <root>/local/<name> (publish it to GitHub later from the GitHub tab).
#[tauri::command]
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
               "issues": store::gh_json(&["issue", "list", "--json", f], &p.gh, Some(&p.path)),
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
async fn update_check() -> R<Value> {
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
async fn release(st: St<'_>, pid: String, tag: String, files: Vec<String>) -> R<String> {
    let p = st.project(&pid).ok_or("no such project")?;
    tauri::async_runtime::spawn_blocking(move || store::release(&p.path, &p.gh, &tag, &files)).await.map_err(|e| e.to_string())?
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

#[tauri::command]
async fn task_do(st: St<'_>, pid: String, action: String, arg: String) -> R<()> {
    let p = st.project(&pid).ok_or("no such project")?;
    tauri::async_runtime::spawn_blocking(move || store::task_do(&p.path, &p.gh, &action, &arg)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn gh_repos(user: String) -> Value {
    tauri::async_runtime::spawn_blocking(move || store::gh_json(&["repo", "list", "--limit", "300", "--json", "nameWithOwner,description"], &user, None))
        .await.unwrap_or(Value::Null)
}

#[tauri::command]
async fn clone_repo(st: St<'_>, user: String, repo: String) -> R<Value> {
    // <root>/<owner>/<repo>; already cloned there = just add it
    let dest = store::projects_root(&st.cfg.lock().unwrap()).join(repo.split('/').collect::<std::path::PathBuf>()).to_string_lossy().to_string();
    if let Some(p) = st.cfg.lock().unwrap().projects.iter().find(|p| p.path == dest) {
        return Ok(json!(p));
    }
    if !std::path::Path::new(&dest).join(".git").exists() {
        let (d, u) = (dest.clone(), user.clone());
        let out = tauri::async_runtime::spawn_blocking(move || store::run("gh", &["repo", "clone", &repo, &d], None, &store::gh_env(&u)))
            .await.map_err(|e| e.to_string())?;
        if !out.ok {
            return Err(out.stderr.trim().to_string());
        }
    }
    let who = detect(&st, dest.clone()).await;
    let mut cfg = st.cfg.lock().unwrap();
    // the suggested gh if any (owner's), else the account that cloned it
    let gh = who.as_ref().map(|w| w.gh.clone()).filter(|g| !g.is_empty()).unwrap_or(user);
    let p = store::new_project(&mut cfg, &dest, who.as_ref().map(|w| w.account.as_str()), Some(&gh));
    let p = mark(&mut cfg, &p.id, who.as_ref());
    store::save(&cfg);
    Ok(json!(p))
}

// ---- build ----

#[tauri::command]
async fn run_step(app: AppHandle, st: St<'_>, pid: String, step: String) -> R<bool> {
    Ok(claude::run_step(&app, &st, &pid, &step).await.0)
}

#[tauri::command]
fn build_guess(st: St, pid: String) -> R<store::Build> {
    Ok(store::guess_build(&st.project(&pid).ok_or("no such project")?.path))
}

#[tauri::command]
fn stop_run(pid: String) -> bool {
    claude::stop_run(&pid)
}

// ---- .env ----

#[tauri::command]
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
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    std::fs::write(&path, store::env_set(&text, &key, value.as_deref())).map_err(|e| e.to_string())
}

/// Add the file to .gitignore.
#[tauri::command]
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

/// Images Claude read (screenshots) → chat thumbnails. Any path ∵ screenshots live in temp/scratchpad, not the project;
/// ∴ image extensions only, ≤ 20 MB. Raw bytes (no base64 dep) → page gets an ArrayBuffer.
#[tauri::command]
fn read_image(path: String) -> R<tauri::ipc::Response> {
    let ext = std::path::Path::new(&path).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !["png", "jpg", "jpeg", "gif", "webp", "bmp"].contains(&ext.as_str()) { return Err("not an image".into()); }
    if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 20_000_000 { return Err("too large".into()); }
    Ok(tauri::ipc::Response::new(std::fs::read(&path).map_err(|e| e.to_string())?))
}

// ---- browsers: open pages in a system browser profile (you stay logged in there) ----

#[tauri::command]
fn browsers() -> Value {
    json!(store::browsers())
}

#[tauri::command]
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

/// In-app browser: one child webview "web" laid over the Browser tab's placeholder. Profile dir ~/.clauracle/browser,
/// shared by all projects ∴ logins persist + are shared. rect None → hide. nav → load url (else only move/resize).
/// No IPC for it: capabilities need `remote.urls` for external pages. Its navigations → event `weburl` (address bar).
#[tauri::command]
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

/// Permission card › "Always allow everywhere": tool → ~/.claude/settings.json permissions.allow (plain Claude too).
/// Unparsable file → error, never overwritten. Key order kept (serde_json preserve_order).
#[tauri::command]
fn allow_global(tool: String) -> R<()> {
    if tool.is_empty() || !tool.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') { return Err("bad tool name".into()); }
    let f = dirs::home_dir().ok_or("no home")?.join(".claude/settings.json");
    let txt = std::fs::read_to_string(&f).unwrap_or_else(|_| "{}".into());
    let mut v: Value = serde_json::from_str(txt.trim_start_matches('\u{feff}')).map_err(|e| format!("~/.claude/settings.json: {e}"))?;
    let o = v.as_object_mut().ok_or("settings.json is not an object")?;
    let p = o.entry("permissions").or_insert(json!({})).as_object_mut().ok_or("permissions is not an object")?;
    let a = p.entry("allow").or_insert(json!([])).as_array_mut().ok_or("permissions.allow is not a list")?;
    if !a.iter().any(|x| x == tool.as_str()) { a.push(json!(tool)); }
    std::fs::write(&f, serde_json::to_string_pretty(&v).unwrap()).map_err(|e| e.to_string())
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
#[tauri::command]
fn homepage(exe: String, profile: String) -> String {
    store::browsers().iter().find(|b| b["exe"] == exe.as_str())
        .and_then(|b| std::fs::read_to_string(std::path::Path::new(b["data"].as_str()?).join(if profile.is_empty() { "Default" } else { &profile }).join("Preferences")).ok())
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|p| p["homepage"].as_str().or(p["session"]["startup_urls"][0].as_str()).map(String::from))
        .filter(|u| u.starts_with("http")).unwrap_or_default()
}

/// Start `exe` (detached, it outlives us) and quit; the new instance resumes cut-off turns (claude::resume_interrupted).
/// Main window geometry → config (close, quit, restart). Maximized keeps the last normal rect ∵ restore needs it.
fn save_win(app: &AppHandle) {
    let (Some(w), Some(st)) = (app.get_webview_window("main"), app.try_state::<Arc<State>>()) else { return };
    if w.is_minimized().unwrap_or(false) { return; }
    let max = w.is_maximized().unwrap_or(false);
    let mut cfg = st.cfg.lock().unwrap();
    let r = match (max, cfg.win, w.outer_position(), w.inner_size()) {
        (true, Some(o), ..) => [o[0], o[1], o[2], o[3], 1],
        (_, _, Ok(p), Ok(s)) => [p.x, p.y, s.width as i32, s.height as i32, max as i32],
        _ => return,
    };
    if cfg.win != Some(r) { cfg.win = Some(r); store::save(&cfg); }
}

/// Saved geometry back, unless its top-left is on no monitor any more (unplugged screen).
fn restore_win(app: &tauri::App, r: [i32; 5]) {
    let Some(w) = app.get_webview_window("main") else { return };
    let on = w.available_monitors().unwrap_or_default().iter().any(|m| {
        let (p, s) = (m.position(), m.size());
        (p.x..p.x + s.width as i32).contains(&(r[0] + 40)) && (p.y..p.y + s.height as i32).contains(&(r[1] + 10))
    });
    if on {
        let _ = w.set_position(tauri::PhysicalPosition::new(r[0], r[1]));
    }
    let _ = w.set_size(tauri::PhysicalSize::new(r[2].max(900) as u32, r[3].max(560) as u32));
    if r[4] == 1 { let _ = w.maximize(); }
}

pub fn relaunch(app: &AppHandle, exe: std::path::PathBuf, args: &[String]) {
    save_win(app);
    for pid in CHILDREN.lock().unwrap().drain(..) {
        kill_tree(pid); // sessions end before the new instance resumes them
    }
    save_children(&[]);
    let mut c = std::process::Command::new(exe);
    c.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x8 | 0x200); // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
    }
    if c.spawn().is_ok() {
        app.exit(0);
    }
}

/// Swap helper (a new build started with `--swap-into <installed exe>`): copy itself next to the target, rename it
/// over the target once the old app has exited (rename fails while Windows holds the exe), start it, exit.
/// ponytail: ~30s retry, then starts whatever is there (old build) so the app always comes back.
fn swap_into(dest: &std::path::Path) {
    let tmp = dest.with_extension("new");
    if let Ok(me) = std::env::current_exe() {
        if std::fs::copy(&me, &tmp).is_ok() {
            for _ in 0..150 {
                if std::fs::rename(&tmp, dest).is_ok() { break; }
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

#[tauri::command]
fn restart(app: AppHandle) {
    if let Ok(me) = std::env::current_exe() {
        relaunch(&app, me, &[]);
    }
}

fn kill_tree(pid: u32) {
    if cfg!(windows) {
        let _ = store::command(std::path::Path::new("taskkill")).args(["/F", "/T", "/PID", &pid.to_string()]).output();
    } else {
        let _ = std::process::Command::new("kill").arg(pid.to_string()).output();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--swap-into") {
        return swap_into(std::path::Path::new(args.get(2).map_or("", String::as_str)));
    }
    let app = tauri::Builder::default()
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
            // warm the slow lookups so Settings opens instantly: gh accounts, and the catalog on a first run
            let st2 = st.clone();
            // housekeeping: old update download; caches of projects idle ≥ IDLE_CLEAN_DAYS (a build regenerates them)
            let dirs: Vec<String> = st.cfg.lock().unwrap().projects.iter().map(|p| p.path.clone()).collect();
            tauri::async_runtime::spawn_blocking(move || {
                let _ = std::fs::remove_dir_all(std::env::temp_dir().join("clauracle-update"));
                for d in dirs.iter().filter(|d| store::idle_days(d).map_or(false, |n| n >= store::IDLE_CLEAN_DAYS)) { store::clean(d, &[]); }
            });
            tauri::async_runtime::spawn(async move {
                let _ = tauri::async_runtime::spawn_blocking(store::gh_users).await;
                if st2.cfg.lock().unwrap().catalog.is_empty() {
                    let dir = st2.plugin_dir.clone();
                    if let Ok(items) = tauri::async_runtime::spawn_blocking(move || store::catalog(&dir)).await {
                        let mut cfg = st2.cfg.lock().unwrap();
                        cfg.catalog = items;
                        store::save(&cfg);
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            state, open_project, add_project, remove_project, update_project, send, interrupt, new_session,
            continue_now, set_lessons, qr, lan_ip, prepare_handover, answer_ask, answer_questions, settings, refresh_catalog, set_off,
            update_account, set_default_account, claude_login, claude_add_token, claude_remove, set_notify,
            test_notify, install, gh_login, gh_logout, gh_info, gh_repos, clone_repo, run_step, build_guess, stop_run,
            env_list, env_set, env_ignore, list_dir, read_file, read_image, browsers, open_in_browser, web_view, homepage, reach, allow_global, agents, set_agents, who, save_draft, restart, publish_repo, gcloud_info, gcloud_login, gcloud_do, team, task_do, git_do, release, disk, disk_clean, update_check, update_run, set_items, set_projects_dir, new_local, set_zoom, orphans])
        .build(tauri::generate_context!())
        .expect("failed to start Clauracle");
    app.run(|app, e| {
        if let tauri::RunEvent::WindowEvent { event: tauri::WindowEvent::CloseRequested { .. }, .. } | tauri::RunEvent::ExitRequested { .. } = e {
            save_win(app);
        }
        if let tauri::RunEvent::Exit = e {
            // claude processes and run steps don't die with us on Windows: take their whole trees down
            for pid in CHILDREN.lock().unwrap().drain(..) {
                kill_tree(pid);
            }
            save_children(&[]);
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
