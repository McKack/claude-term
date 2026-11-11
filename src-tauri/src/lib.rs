//! Clauracle: Tauri commands the page calls, one file per area (projects, setup, tasks, build, release, license, report, files). Sessions live in claude/, everything else in store/
mod claude;
mod store;

use claude::{emit, session, Input, State};
use serde_json::{json, Value};
use std::sync::atomic::Ordering::SeqCst;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State as S};
mod projects;
use projects::*;
mod setup;
use setup::*;
mod release;
use release::*;
mod tasks;
use tasks::*;
mod build;
use build::*;
mod license;
use license::*;
mod report;
use report::*;
mod files;
use files::*;

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

// #390 spelling in our own right-click menu (Windows/WebView2 only; JS gets no suggestions). Page doesn't preventDefault
// in editable fields → WebView2 ContextMenuRequested: always Handled (never the native menu); editable target → emit
// `ctxmenu` {sp:[{l,id}]} = leading group of Name "other" items (= suggestions + Add to dictionary) before the 1st separator.
// Suggestions → deferral kept in CTXD till JS ctx_pick(id) (-1 = dismissed); id runs Chromium's own replace/add.
#[cfg(windows)]
mod ctxm {
    use webview2_com::{take_pwstr, ContextMenuRequestedEventHandler, Microsoft::Web::WebView2::Win32::*};
    use windows_core::{Interface, BOOL, PWSTR};
    thread_local! { pub static CTXD: std::cell::RefCell<Option<(ICoreWebView2ContextMenuRequestedEventArgs, ICoreWebView2Deferral)>> = Default::default(); }
    pub fn pick(id: i32) {
        if let Some((a, d)) = CTXD.with(|c| c.borrow_mut().take()) { unsafe { if id >= 0 { let _ = a.SetSelectedCommandId(id); } let _ = d.Complete(); } }
    }
    pub fn hook(wv: &tauri::Webview, app: tauri::AppHandle) {
        let _ = wv.with_webview(move |pw| unsafe {
            let Ok(core) = pw.controller().CoreWebView2().and_then(|c| c.cast::<ICoreWebView2_11>()) else { return };
            let h = ContextMenuRequestedEventHandler::create(Box::new(move |_, args| {
                let Some(a) = args else { return Ok(()) };
                a.SetHandled(true)?;
                let mut ed = BOOL(0);
                a.ContextMenuTarget()?.IsEditable(&mut ed)?;
                if !ed.as_bool() { return Ok(()); }
                let items = a.MenuItems()?;
                let mut n = 0u32;
                items.Count(&mut n)?;
                let mut sp = vec![];
                for i in 0..n {
                    let it = items.GetValueAtIndex(i)?;
                    let mut k = COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND::default();
                    it.Kind(&mut k)?;
                    if k == COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_SEPARATOR { break; }
                    let (mut nm, mut lb, mut id) = (PWSTR::null(), PWSTR::null(), 0i32);
                    it.Name(&mut nm)?; it.Label(&mut lb)?; it.CommandId(&mut id)?;
                    if take_pwstr(nm) != "other" || k != COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_COMMAND { continue; }
                    sp.push(serde_json::json!({"l": take_pwstr(lb).replace('&', ""), "id": id}));
                }
                if !sp.is_empty() { pick(-1); let d = a.GetDeferral()?; CTXD.with(|c| *c.borrow_mut() = Some((a, d))); }
                crate::claude::emit(&app, "ctxmenu", serde_json::json!({"sp": sp}));
                Ok(())
            }));
            let mut tok = 0i64;
            let _ = core.add_ContextMenuRequested(&h, &mut tok);
        });
    }
}
/// #390: spelling item picked (id) or our menu closed (-1). Sync = main thread, where CTXD lives.
#[tauri::command]
fn ctx_pick(id: i32) { #[cfg(windows)] ctxm::pick(id); #[cfg(not(windows))] let _ = id; }

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
            #[cfg(windows)] if let Some(w) = app.get_webview("main") { ctxm::hook(&w, app.handle().clone()); }
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
                    let freed = janitor_pass(&st3);
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
        .invoke_handler(tauri::generate_handler![rel_fix, lic_credits, lic_write, ctx_pick, 
            fullscreen, state, ui_reply, open_project, add_project, remove_project, update_project, send, interrupt,
            continue_now, set_lessons, set_you, report_save, report_get, log_save, report_pdf, save_as, qr, ext_update, lan_ip, avatar, prepare_handover, answer_ask, answer_questions, settings, refresh_catalog, ext_find, ext_add, set_off,
            update_account, set_default_account, claude_login, claude_add_token, claude_remove, set_notify,
            test_notify, install, fix_missing, deps, place_of, scan_folder, move_folder, gh_login, gh_logout, gh_info, actions_usage, gh_repos, gh_branches, clone_repo, run_step, build_guess, stop_run, stop_build, restart_into, send_mid,
            env_list, env_set, env_ignore, apple_csr, apple_cer_find, apple_p12, ships_mac, bundle_id, bundle_id_set, site_check, site_auto, site_key_install, version_set, release_check, plan_move, plan_wait, gh_secrets, gh_secrets_set,list_dir, file_heat, read_file, write_file, write_bytes, lint, notes, shell_open, shell_write, shell_resize, shell_kill, read_image, browsers, open_in_browser, web_view, homepage, reach, claude_settings, claude_settings_set, agents, set_agents, who, save_draft, restart, quit, publish_repo, gcloud_info, gcloud_accounts, gcloud_login, gcloud_do, team, task_do, git_do, release, stage, disk, disk_clean, janitor, update_check, update_run, set_update_every, set_items, set_ci_builds, set_mac, mac_test, mac_setup, mac_key, mac_xcode, set_projects_dir, new_local, set_zoom, orphans])
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
#[test] #[ignore]
fn lic_scan_here() { let d = lic_deps(std::path::Path::new(".."), "GPL-3.0"); let bad: Vec<_> = d.iter().filter(|x| x["s"] != "ok").collect();
    println!("{} deps, non-ok: {:?}\nmiss: {:?}", d.len(), bad, credits_miss(std::path::Path::new(".."), &d)); }
