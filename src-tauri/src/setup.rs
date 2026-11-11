//! Commands: settings, catalog, accounts, gcloud, gh, network Mac, folders, Claude settings.
use crate::*;

#[tauri::command]
pub(crate) async fn settings(st: St<'_>) -> R<Value> {
    if st.cfg.lock().unwrap().catalog.is_empty() {
        refresh_catalog(st.clone()).await?;
    }
    let gh = tauri::async_runtime::spawn_blocking(store::gh_users).await.map_err(|e| e.to_string())?;
    let cfg = st.cfg.lock().unwrap();
    Ok(json!({"catalog": cfg.catalog, "off": cfg.off, "gh_users": gh}))
}

#[tauri::command]
pub(crate) async fn refresh_catalog(st: St<'_>) -> R<Value> {
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
pub(crate) async fn ext_find(kind: String, q: String) -> R<Value> {
    let u = if kind == "skill" { "https://skills.sh/api/search?limit=20&q=" } else { "https://registry.modelcontextprotocol.io/v0/servers?limit=30&search=" };
    let url = format!("{u}{}", q.bytes().map(|c| if c.is_ascii_alphanumeric() || b"-_.".contains(&c) { (c as char).to_string() } else { format!("%{c:02X}") }).collect::<String>());
    tauri::async_runtime::spawn_blocking(move || ureq::get(&url).call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string()))
        .await.map_err(|e| e.to_string())?
        .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
}

/// Skills installed from their source follow it: the skills CLI re-fetches every global skill that changed upstream.
#[tauri::command]
pub(crate) async fn ext_update() -> R<()> {
    let home = dirs::home_dir().unwrap_or_default().to_string_lossy().to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let o = store::run("npx", &["-y", "skills", "update", "-g", "-y"], Some(&home), &[]);
        if o.ok { Ok(()) } else { Err(o.stderr.chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect()) }
    }).await.map_err(|e| e.to_string())?
}

/// #213 install user-wide. skill: a = "owner/repo", b = skill id → `npx skills add` (Claude Code only, copied ∵ Windows
/// symlinks need admin). mcp: a = name, b = http(s) url (remote) | npm package (stdio via npx). Names from the web → validated.
#[tauri::command]
pub(crate) async fn ext_add(kind: String, a: String, b: String) -> R<()> {
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
pub(crate) fn set_off(st: St, item: String, off: bool, scope: String) {
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
pub(crate) fn set_items(st: St, scope: String, items: Vec<String>) {
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
pub(crate) fn update_account(st: St, id: String, changes: Value) {
    let mut cfg = st.cfg.lock().unwrap();
    if let Some(a) = cfg.accounts.iter_mut().find(|a| a.id == id) {
        let mut v = json!(a);
        for (k, val) in changes.as_object().cloned().unwrap_or_default() {
            if ["nick", "gh", "limits", "relay"].contains(&k.as_str()) {
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
pub(crate) fn set_default_account(st: St, id: String) {
    let mut cfg = st.cfg.lock().unwrap();
    cfg.default_account = id;
    store::save(&cfg);
}

#[tauri::command]
pub(crate) async fn claude_login(app: AppHandle, st: St<'_>, nick: String) -> R<Value> {
    let a2 = app.clone();
    let token = tauri::async_runtime::spawn_blocking(move || {
        store::setup_token(|url| emit(&a2, "login_url", json!({"url": url})))
    }).await.map_err(|e| e.to_string())??;
    claude_add_token(st, nick, token, None)
}

#[tauri::command(async)]
pub(crate) fn claude_add_token(st: St, nick: String, token: String, base_url: Option<String>) -> R<Value> {
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
pub(crate) fn claude_remove(st: St, id: String) {
    let mut cfg = st.cfg.lock().unwrap();
    let pids: Vec<String> = cfg.projects.iter().filter(|p| p.account == id).map(|p| p.id.clone()).collect();
    store::remove_account(&mut cfg, &id);
    store::save(&cfg);
    drop(cfg);
    // their sessions kept running on the removed account's token until some later reconnect
    for (pid, l) in st.live.lock().unwrap().iter() { if pids.contains(pid) { l.inbox.front(vec![claude::Input::Reconnect]); } }
}

#[tauri::command]
pub(crate) fn set_notify(st: St, notify: Value) {
    let mut cfg = st.cfg.lock().unwrap();
    if let Ok(n) = serde_json::from_value(notify) {
        cfg.notify = n;
    }
    store::save(&cfg);
}

#[tauri::command(async)]
pub(crate) fn test_notify(app: AppHandle, st: St) {
    claude::notify(&app, &st, "Clauracle", "Notifications work.");
}

#[tauri::command(async)]
pub(crate) fn deps() -> Value { store::deps() }

#[tauri::command]
pub(crate) async fn install(what: String) -> R<()> {
    tauri::async_runtime::spawn_blocking(move || match what.as_str() {
        "gh" => store::install_gh(), "gcloud" => store::install_gcloud(), _ => store::install_claude() })
        .await.map_err(|e| e.to_string())?
}

/// #167 one-click fix for what UI missOf() spotted in a tool result. Names come from tool output → validated, never a shell.
#[tauri::command]
pub(crate) async fn fix_missing(st: St<'_>, pid: String, kind: String, name: String, feat: Option<String>, pkg: Option<String>) -> R<()> {
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
pub(crate) async fn gcloud_info(st: St<'_>, pid: String) -> R<Value> {
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
pub(crate) async fn gcloud_login() -> R<()> {
    tauri::async_runtime::spawn_blocking(|| {
        let o = store::run("gcloud", &["auth", "login", "--brief", "--update-adc=false"], None, &[]);
        if o.ok { Ok(()) } else { Err(o.stderr.trim().lines().last().unwrap_or("sign-in failed").to_string()) }
    }).await.map_err(|e| e.to_string())?
}

/// Signed-in Google accounts for the Accounts page (no project needed); null when gcloud is missing.
#[tauri::command]
pub(crate) async fn gcloud_accounts() -> R<Value> {
    if store::which("gcloud").is_none() { return Ok(Value::Null); }
    tauri::async_runtime::spawn_blocking(|| store::gcloud(&["auth", "list"], "", "")).await.map_err(|e| e.to_string())?
}

/// Project actions: create a Cloud project, switch an API on/off, link billing.
#[tauri::command]
pub(crate) async fn gcloud_do(st: St<'_>, pid: String, action: String, arg: String, on: Option<bool>) -> R<Value> {
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
pub(crate) async fn gh_login(app: AppHandle, scopes: Option<String>) -> R<bool> { // #128 scopes e.g. "user" → re-login adds them
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
pub(crate) async fn gh_logout(user: String) -> bool {
    store::forget_gh_users();
    let ok = tauri::async_runtime::spawn_blocking(move || store::run("gh", &["auth", "logout", "-h", "github.com", "-u", &user], None, &[]).ok)
        .await.unwrap_or(false);
    store::forget_gh_users(); // again: see gh_login
    ok
}

#[tauri::command]
pub(crate) fn set_update_every(st: St, hours: u32) {
    let mut cfg = st.cfg.lock().unwrap();
    cfg.update_every = Some(hours);
    store::save(&cfg);
}

#[tauri::command]
pub(crate) fn set_ci_builds(st: St, on: bool) {
    let mut cfg = st.cfg.lock().unwrap();
    cfg.ci_builds = on;
    store::save(&cfg);
}

#[tauri::command]
pub(crate) fn set_mac(st: St, mac: store::NetMac) {
    let mut cfg = st.cfg.lock().unwrap();
    cfg.mac = mac;
    store::save(&cfg);
}

/// #148 install Mac build deps over SSH; progress lines → event `mac_setup` {line}; returns ok.
#[tauri::command]
pub(crate) async fn mac_setup(app: AppHandle, mac: store::NetMac) -> bool {
    tauri::async_runtime::spawn_blocking(move || store::mac_setup(&mac, |l| emit(&app, "mac_setup", json!({"line": l})))).await.unwrap_or(false)
}

/// #160 Set up SSH: key + authorized_keys via password once, then save cfg.mac (dir kept).
#[tauri::command]
pub(crate) async fn mac_key(st: St<'_>, mac: store::NetMac, pw: String) -> Result<(), String> {
    let m = mac.clone();
    tauri::async_runtime::spawn_blocking(move || store::mac_key_setup(&m, &pw)).await.map_err(|e| e.to_string())??;
    let mut cfg = st.cfg.lock().unwrap();
    cfg.mac = mac;
    store::save(&cfg);
    Ok(())
}

#[tauri::command]
pub(crate) async fn mac_xcode(mac: store::NetMac) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || store::mac_xcode(&mac)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn mac_test(mac: store::NetMac) -> serde_json::Value {
    tauri::async_runtime::spawn_blocking(move || store::mac_test(&mac)).await.unwrap_or_default()
}

#[tauri::command]
pub(crate) fn set_projects_dir(st: St, dir: String) {
    let mut cfg = st.cfg.lock().unwrap();
    cfg.projects_dir = dir;
    store::save(&cfg);
}

/// #35: where a folder belongs under the projects root (<owner>/<repo> or local/<name>).
#[tauri::command(async)]
pub(crate) fn place_of(st: St, path: String) -> String {
    let root = store::projects_root(&st.cfg.lock().unwrap());
    store::place_of(&root, &path).to_string_lossy().into()
}

/// #35/#36: projects found under `dir` ("" = projects root): path, where it belongs, and its project id if added.
#[tauri::command(async)]
pub(crate) fn scan_folder(st: St, dir: String) -> Vec<Value> {
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
pub(crate) async fn move_folder(st: St<'_>, from: String, to: String) -> R<String> {
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
pub(crate) fn new_local(st: St, name: String) -> R<Value> {
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
pub(crate) async fn gh_info(st: St<'_>, pid: String) -> R<Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    tauri::async_runtime::spawn_blocking(move || {
        let f = "number,title,url,author";
        json!({"repo": store::gh_json(&["repo", "view", "--json", "nameWithOwner,url"], &p.gh, Some(&p.path)),
               "git": store::git_do(&p.path, &p.gh, "status", "").unwrap_or_default(),
               "builds": store::builds(&p.path),
               "release": store::gh_json(&["release", "view", "--json", "tagName,url,publishedAt,assets"], &p.gh, Some(&p.path)),
               "prs": store::gh_json(&["pr", "list", "--json", f], &p.gh, Some(&p.path)),
               "issues": store::gh_json(&["issue", "list", "--search", "-label:clauracle", "--json", f], &p.gh, Some(&p.path)), // clauracle ones = the task list
               "remote": store::repo_of(&p.path).is_some(), "users": store::gh_users()})
    }).await.map_err(|e| e.to_string())
}

/// Local project → new GitHub repo; the project then runs as that gh account.
#[tauri::command]
pub(crate) async fn publish_repo(st: St<'_>, pid: String, user: String, name: String, private: bool) -> R<String> {
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
pub(crate) async fn team(st: St<'_>, pid: String) -> R<Value> {
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
pub(crate) async fn git_do(st: St<'_>, pid: String, action: String, arg: String) -> R<Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    tauri::async_runtime::spawn_blocking(move || store::git_do(&p.path, &p.gh, &action, &arg)).await.map_err(|e| e.to_string())?
}

/// In-app browser: one child webview "web" laid over the Browser tab's placeholder. Profile dir ~/.clauracle/browser,
/// shared by all projects ∴ logins persist + are shared. rect None → hide. nav → load url (else only move/resize).
/// No IPC for it: capabilities need `remote.urls` for external pages. Its navigations → event `weburl` (address bar).
#[tauri::command(async)]
pub(crate) fn web_view(app: AppHandle, url: String, rect: Option<[f64; 4]>, zoom: f64, nav: bool) -> R<()> {
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
pub(crate) fn cs_file(st: &St, scope: &str, pid: &str) -> R<std::path::PathBuf> {
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
pub(crate) fn claude_settings(st: St, scope: String, pid: String) -> R<Value> {
    let f = cs_file(&st, &scope, &pid)?;
    let txt = std::fs::read_to_string(&f).unwrap_or_else(|_| "{}".into());
    let v: Option<Value> = serde_json::from_str(txt.trim_start_matches('\u{feff}')).ok().filter(Value::is_object);
    Ok(json!({"path": f.to_string_lossy(), "text": txt.trim_start_matches('\u{feff}'), "json": v}))
}

/// Settings editor: write a whole object (key order kept: serde_json preserve_order). Non-object → refused.
#[tauri::command]
pub(crate) fn claude_settings_set(st: St, scope: String, pid: String, value: Value) -> R<()> {
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
pub(crate) async fn reach(host: String, port: u16) -> bool {
    use std::net::ToSocketAddrs;
    tauri::async_runtime::spawn_blocking(move || (host.as_str(), port).to_socket_addrs().ok().and_then(|mut a| a.next())
        .is_some_and(|a| std::net::TcpStream::connect_timeout(&a, std::time::Duration::from_millis(600)).is_ok()))
        .await.unwrap_or(false)
}

/// Start page for the in-app browser: the browser profile's homepage (or first startup page) from its Preferences.
#[tauri::command(async)]
pub(crate) fn homepage(exe: String, profile: String) -> String {
    store::browsers().iter().find(|b| b["exe"] == exe.as_str())
        .and_then(|b| std::fs::read_to_string(std::path::Path::new(b["data"].as_str()?).join(if profile.is_empty() { "Default" } else { &profile }).join("Preferences")).ok())
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|p| p["homepage"].as_str().or(p["session"]["startup_urls"][0].as_str()).map(String::from))
        .filter(|u| u.starts_with("http")).unwrap_or_default()
}
