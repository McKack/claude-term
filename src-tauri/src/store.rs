//! Persistence and CLI helpers: projects, AI accounts (tokens in the OS keychain), usage limits and pools,
//! GitHub through gh, the Connections & Skills catalog, and fetching `claude`/`gh` when they're missing.
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::io::Read;
use std::process::Command;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAIN: &str = "Main"; // the account `claude` itself is logged in as (no stored token)
const KEYRING: &str = "clauracle";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Limits {
    pub five_hour: f64, // % of the window; at it the loop switches account (pool) or stops
    pub seven_day: f64,
}
impl Default for Limits {
    fn default() -> Self {
        Limits { five_hour: 90.0, seven_day: 90.0 }
    }
}

/// An AI account: the login, its limits and pool. Its gh/off are only the starting point for new projects.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Account {
    pub id: String,
    pub nick: String,
    pub provider: String, // ponytail: always "claude" until a second provider exists
    pub gh: String,
    pub off: Vec<String>,       // legacy deny-list, used only while `on` is None
    pub on: Option<Vec<String>>, // what new projects start with (connections/skills); Some([]) = nothing yet
    pub limits: Limits,
    pub pool: String, // accounts sharing a pool take over from each other at their limits
    pub base_url: String, // "" = Anthropic login (setup-token); else custom endpoint, keychain holds its API key
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Opt {
    pub label: String,
    pub description: String,
    pub recommended: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Question {
    pub id: String,
    pub question: String,
    pub options: Vec<Opt>,
    pub multi: bool,
    pub blocking: bool,
}

/// Build steps Clauracle runs itself (plain code, no tokens). after: "" | smart (#89, new projects) | build | build_run | build_deploy
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Build {
    pub build: String,
    pub run: String,
    pub deploy: String,
    pub after: String, // after each finished task
    pub fix: bool,     // a failed step goes back to Claude to fix
}

/// The project is the source of truth: its own GitHub account, connections and skills, whatever AI account runs it.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub account: String,
    pub gh: String,
    pub off: Vec<String>,        // legacy deny-list, used only while `on` is None (projects from before allow-lists)
    pub on: Option<Vec<String>>, // the connections/skills the user picked for this project; nothing else is enabled
    pub mode: String,  // permission mode
    pub model: String, // "" = the CLI's default
    pub effort: String, // --effort low|medium|high|xhigh|max; "" = the CLI's default
    pub ponytail: bool,
    pub cont: String,         // continue mode: manual | auto | auto_answer
    pub context_limit: f64,   // % of context; past it the session prepares a handover
    pub session: Option<String>,
    pub prev_session: Option<String>, // the one before the last clear: shown faded above the current after a restart
    pub ready: bool,         // handover written, waiting for Continue (manual) or the limits (auto)
    pub questions: Vec<Question>, // waiting for the user
    pub answers: String,      // answered, to go into the next message
    pub build: Build,
    pub hide_env: bool,       // Claude may not read .env files
    pub terse: bool,          // dense "AI comments"
    pub owner: String,        // GitHub owner of origin; "" = not on GitHub
    pub shared: bool,         // owner isn't one of our gh accounts (collaboration)
    pub keep: bool,           // user kept their own gh/account over the suggestion
    pub used: f64,            // last opened, for "recent"
    pub gaccount: String,     // Google account for gcloud ("" = gcloud's active one)
    pub gcp: String,          // Google Cloud project id
    pub services: Vec<String>, // Services tab: what it signs in with, gets paid through, ships to (ids in the UI's list)
    pub working: bool,        // a turn is running; still true at startup = it was cut off (crash/restart): resume it
    pub ai_credit: bool,      // let Claude credit itself in commits/PRs (Co-Authored-By etc.); off by default
    pub allow: Vec<String>,   // tools this project never asks for ("Always allow" on a permission card)
    pub stage: String,        // release wizard: "" | alpha | beta | rc | published
    pub rel_auto: bool,       // Release panel tick: successful build + version ahead of last tag → gh release (UI does it)
    pub order: Vec<u64>,      // task issue numbers in Plan tab order (local; unlisted issues follow by number)
    pub targets: Vec<String>, // target platforms: windows|macos|linux|ios|android; [] = not picked → UI guesses (ships_mac)
    pub site: serde_json::Value, // #40 website: {host, domain, pages: [landing|privacy|terms]}; UI-owned shape, secrets stay in .env
}
impl Default for Project {
    fn default() -> Self {
        Project {
            id: String::new(), name: String::new(), path: String::new(), account: MAIN.into(), gh: String::new(),
            off: vec![], on: None, mode: "default".into(), model: String::new(), effort: String::new(), ponytail: true, cont: "manual".into(),
            context_limit: 50.0, session: None, prev_session: None, ready: false, questions: vec![], answers: String::new(),
            build: Build { after: "smart".into(), fix: true, ..Default::default() }, hide_env: true, terse: true,
            owner: String::new(), shared: false, keep: false, used: 0.0, working: false, gaccount: String::new(), gcp: String::new(), services: vec![], allow: vec![], ai_credit: false, stage: String::new(), rel_auto: false, order: vec![], targets: vec![], site: serde_json::Value::Null,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Item {
    pub id: String, // "mcp:<server>" or "skill:<name>"
    pub label: String,
    pub kind: String,
    pub note: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Window {
    pub utilization: f64, // 0..1
    #[serde(rename = "resetsAt")]
    pub resets_at: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Notify {
    pub desktop: bool,
    pub ntfy: String, // ntfy.sh topic for phone pushes; "" = off
}

/// A helper agent Claude Code can hand work to (a subagent), usually on a cheaper model.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Agent {
    pub name: String,
    pub model: String, // haiku | sonnet | opus | inherit
    pub description: String, // when Claude should use it
    pub prompt: String,
    pub on: bool,
}

pub fn default_agents() -> Vec<Agent> {
    let a = |name: &str, model: &str, description: &str, prompt: &str| Agent {
        name: name.into(), model: model.into(), description: description.into(), prompt: prompt.into(), on: true };
    vec![
        a("scout", "haiku", "Finding things: searching the codebase, reading files, looking up docs. Use it instead of \
           reading many files yourself; it returns only what matters.",
          "You find things fast and report back tersely: file paths with line numbers, the few relevant lines, a one-line \
           answer. Never paste whole files. Never change anything."),
        a("runner", "haiku", "Running builds, tests and linters. Use it to run them and get back only failures.",
          "Run the commands you're given. Report only: pass/fail, and for failures the error lines with file:line. \
           No logs of passing steps. Never change code."),
        a("reviewer", "sonnet", "A second look at a finished change before it's handed over: bugs, edge cases, bloat.",
          "Review the change you're pointed at. List real problems only (bug, missed edge case, needless code), each \
           with file:line and a one-line fix. Nothing found: say so in one line."),
    ]
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Config {
    pub agents: Option<Vec<Agent>>, // None = the defaults
    pub agents_on: bool,            // master switch for Settings › Agents; off (default) = none are passed to Claude
    pub projects: Vec<Project>,
    pub accounts: Vec<Account>,
    pub usage: HashMap<String, HashMap<String, Window>>,
    pub active: Option<String>,
    pub default_account: String,
    pub catalog: Vec<Item>,
    pub off: Vec<String>, // switched off everywhere
    pub lessons: Vec<String>, // never-again rules (mcp lesson tool / Settings › Accounts), in every session's prompt
    pub win: Option<[i32; 5]>, // main window at last close/restart: x, y, w, h (physical px), maximized 0|1
    pub notify: Notify,
    pub projects_dir: String, // where clones and new projects go ("" = ~/Clauracle): <root>/<owner>/<repo>, <root>/local/<name>
}

/// Projects root: clones land in <root>/<owner>/<repo> (owner = the repo's, so collaborations sit under their owner).
pub fn projects_root(cfg: &Config) -> PathBuf {
    if cfg.projects_dir.trim().is_empty() { dirs::home_dir().unwrap_or_default().join("Clauracle") } else { PathBuf::from(cfg.projects_dir.trim()) }
}

pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".clauracle")
}

/// Per AI account Claude Code settings (#29): layered into each session's --settings file (claude::args). Id → filename: safe chars only.
pub fn account_settings(id: &str) -> Option<PathBuf> {
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || "-_".contains(c))).then(|| home().join(format!("account-{id}.json")))
}

/// a ← b: b wins; objects merge one level deep, arrays concat (permissions.allow from account + project both apply).
pub fn merge_settings(a: &mut serde_json::Value, b: &serde_json::Value) {
    let (Some(a), Some(b)) = (a.as_object_mut(), b.as_object()) else { return };
    for (k, v) in b {
        match (a.get_mut(k), v) {
            (Some(serde_json::Value::Object(x)), serde_json::Value::Object(y)) => for (k2, v2) in y {
                match (x.get_mut(k2), v2) {
                    (Some(serde_json::Value::Array(p)), serde_json::Value::Array(q)) => p.extend(q.iter().cloned()),
                    _ => { x.insert(k2.clone(), v2.clone()); }
                }
            },
            (Some(serde_json::Value::Array(p)), serde_json::Value::Array(q)) => p.extend(q.iter().cloned()),
            _ => { a.insert(k.clone(), v.clone()); }
        }
    }
}

static SAVE: Mutex<()> = Mutex::new(());

pub fn load() -> Config {
    let path = home().join("config.json");
    let mut cfg: Config = match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(s.trim_start_matches('\u{feff}')).unwrap_or_else(|_| {
            // unreadable: keep it aside instead of letting the next save wipe it
            let _ = std::fs::copy(&path, home().join("config.broken.json"));
            Config::default()
        }),
        Err(_) => {
            let mut c = Config { notify: Notify { desktop: true, ntfy: String::new() }, ..Default::default() };
            import_claude_term(&mut c);
            c
        }
    };
    if !cfg.accounts.iter().any(|a| a.id == MAIN) {
        let nick = default_nick(&cfg, "claude");
        cfg.accounts.insert(0, Account { id: MAIN.into(), nick, ..Default::default() });
    }
    for a in cfg.accounts.iter_mut() {
        if a.provider.is_empty() {
            a.provider = "claude".into();
        }
    }
    if cfg.default_account.is_empty() {
        cfg.default_account = MAIN.into();
    }
    cfg
}

pub fn save(cfg: &Config) {
    let _g = SAVE.lock().unwrap();
    let _ = std::fs::create_dir_all(home());
    let tmp = home().join("config.json.tmp"); // write then rename: a crash never leaves half a config
    if std::fs::write(&tmp, serde_json::to_string_pretty(cfg).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&tmp, home().join("config.json"));
    }
}

/// Unsent input per project (text + attachments), so a crash or restart never loses what was typed.
pub fn drafts() -> serde_json::Map<String, serde_json::Value> {
    std::fs::read_to_string(home().join("drafts.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save_draft(pid: &str, draft: serde_json::Value) {
    let _g = SAVE.lock().unwrap();
    let mut all = drafts();
    if draft.is_null() { all.remove(pid); } else { all.insert(pid.into(), draft); }
    let _ = std::fs::create_dir_all(home());
    let tmp = home().join("drafts.json.tmp");
    if std::fs::write(&tmp, serde_json::Value::Object(all).to_string()).is_ok() {
        let _ = std::fs::rename(&tmp, home().join("drafts.json"));
    }
}

/// First run: bring over the accounts claude-term already set up.
fn import_claude_term(cfg: &mut Config) {
    let Ok(s) = std::fs::read_to_string(dirs::home_dir().unwrap_or_default().join(".claude-term/config.json")) else {
        return;
    };
    let Ok(old) = serde_json::from_str::<serde_json::Value>(&s) else { return };
    for a in old["accounts"].as_array().cloned().unwrap_or_default() {
        let id = a["id"].as_str().unwrap_or_default();
        if id.is_empty() || id == MAIN {
            continue;
        }
        let tok = keyring::Entry::new(KEYRING, id).ok().and_then(|e| e.get_password().ok())
            .or_else(|| keyring::Entry::new("claude-term", id).ok().and_then(|e| e.get_password().ok()));
        if let Some(tok) = tok {
            set_token(id, &tok);
            let nick = a["nick"].as_str().unwrap_or(id).to_string();
            cfg.accounts.push(Account { id: id.into(), nick, provider: "claude".into(), ..Default::default() });
        }
    }
}

pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

pub fn new_id() -> String {
    format!("{:x}", (now() * 1e6) as u64 ^ std::process::id() as u64)
}

/// A new project is set up by an AI account and copies its GitHub account and connection/skill choices. From then
/// on they belong to the project.
pub fn new_project(cfg: &mut Config, path: &str, account: Option<&str>, gh: Option<&str>) -> Project {
    let acct = account_or_main(cfg, account.unwrap_or(&cfg.default_account.clone())).clone();
    let name = Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(path.into());
    let p = Project {
        id: new_id(), name, path: path.into(), account: acct.id.clone(),
        gh: gh.map(String::from).unwrap_or(acct.gh.clone()),
        on: Some(acct.on.clone().unwrap_or_default()), // the account's picks, else nothing: the page asks
        ..Default::default()
    };
    cfg.projects.push(p.clone());
    p
}

pub fn account_or_main<'a>(cfg: &'a Config, id: &str) -> &'a Account {
    cfg.accounts.iter().find(|a| a.id == id).unwrap_or(&cfg.accounts[0])
}

// ---- tokens --------------------------------------------------------------------------

pub fn set_token(id: &str, token: &str) {
    if let Ok(e) = keyring::Entry::new(KEYRING, id) {
        let _ = e.set_password(token.trim());
    }
}

pub fn account_env(id: &str, base_url: &str) -> Vec<(String, String)> {
    if id == MAIN {
        return vec![];
    }
    let Some(t) = keyring::Entry::new(KEYRING, id).ok().and_then(|e| e.get_password().ok()) else { return vec![] };
    if base_url.is_empty() {
        return vec![("CLAUDE_CODE_OAUTH_TOKEN".into(), t)];
    }
    // gateways differ: x-api-key vs Bearer → send both; CLAUDE_CODE_OAUTH_TOKEN removed by caller ∵ it'd win
    vec![("ANTHROPIC_BASE_URL".into(), base_url.into()), ("ANTHROPIC_API_KEY".into(), t.clone()), ("ANTHROPIC_AUTH_TOKEN".into(), t)]
}

/// "Claude 2": the AI's name plus the next number not taken by an account already named that way.
pub fn default_nick(cfg: &Config, provider: &str) -> String {
    let ai = match provider { _ => "Claude" }; // ponytail: one provider; a match arm per new AI
    let n = (1..).find(|i| !cfg.accounts.iter().any(|a| a.nick.eq_ignore_ascii_case(&format!("{ai} {i}")))).unwrap();
    format!("{ai} {n}")
}

pub fn add_account(cfg: &mut Config, nick: &str, token: &str, base_url: &str) -> Account {
    let nick = if nick.trim().is_empty() { default_nick(cfg, "claude") } else { nick.trim().to_string() };
    let nick = nick.as_str();
    let base: String = Regex::new(r"\W+").unwrap().replace_all(nick.trim(), "-").trim_matches('-').to_string();
    let base = if base.is_empty() { "account".to_string() } else { base };
    let id = (0..1000).map(|i| if i == 0 { base.clone() } else { format!("{base}-{i}") })
        .find(|c| !cfg.accounts.iter().any(|a| &a.id == c)).unwrap();
    set_token(&id, token);
    let a = Account { id, nick: nick.into(),
                      provider: "claude".into(), on: Some(vec![]), base_url: base_url.trim().trim_end_matches('/').into(), ..Default::default() };
    cfg.accounts.push(a.clone());
    a
}

pub fn remove_account(cfg: &mut Config, id: &str) {
    if id == MAIN {
        return;
    }
    cfg.accounts.retain(|a| a.id != id);
    for p in cfg.projects.iter_mut().filter(|p| p.account == id) {
        p.account = MAIN.into();
    }
    if cfg.default_account == id {
        cfg.default_account = MAIN.into();
    }
    if let Ok(e) = keyring::Entry::new(KEYRING, id) {
        let _ = e.delete_credential();
    }
}

// ---- usage, limits and pools ---------------------------------------------------------------

/// Keep the windows from a rate-limit event (`rate_limit_info`).
pub fn record_usage(cfg: &mut Config, account: &str, raw: &serde_json::Value) {
    let mut windows: Vec<(String, f64, Option<f64>)> = vec![];
    if let Some(obj) = raw["unifiedWindows"].as_object() {
        for (k, v) in obj {
            windows.push((k.clone(), v["utilization"].as_f64().unwrap_or(0.0), v["resetsAt"].as_f64()));
        }
    } else if let Some(t) = raw["rateLimitType"].as_str() {
        windows.push((t.into(), raw["utilization"].as_f64().unwrap_or(0.0), raw["resetsAt"].as_f64()));
    }
    if raw["status"] == "rejected" {
        // hit a wall: that window is full until it resets
        let t = raw["rateLimitType"].as_str().unwrap_or("five_hour").to_string();
        windows.retain(|w| w.0 != t);
        windows.push((t, 1.0, raw["resetsAt"].as_f64()));
    }
    let entry = cfg.usage.entry(account.into()).or_default();
    for (k, u, r) in windows {
        entry.insert(k, Window { utilization: u, resets_at: r });
    }
}

/// Live % per window ("five_hour", "seven_day"), windows that already reset count as 0.
pub fn usage(cfg: &Config, account: &str) -> HashMap<String, f64> {
    let t = now();
    cfg.usage.get(account).map(|w| w.iter().map(|(k, v)| {
        let live = v.resets_at.map_or(true, |r| r > t);
        (k.clone(), if live { (v.utilization * 100.0).round() } else { 0.0 })
    }).collect()).unwrap_or_default()
}

/// The window that is at or past this account's limit, if any.
pub fn over_limit(cfg: &Config, account: &str) -> Option<String> {
    let a = account_or_main(cfg, account);
    let u = usage(cfg, account);
    [("five_hour", a.limits.five_hour), ("seven_day", a.limits.seven_day)].iter()
        .find(|(k, lim)| u.get(*k).copied().unwrap_or(0.0) >= *lim).map(|(k, _)| k.to_string())
}

/// Another account of the same pool that's under its limits, the one with the most left.
pub fn pool_pick(cfg: &Config, account: &str) -> Option<String> {
    let pool = &account_or_main(cfg, account).pool;
    if pool.is_empty() {
        return None;
    }
    cfg.accounts.iter()
        .filter(|a| &a.pool == pool && a.id != account && over_limit(cfg, &a.id).is_none())
        .map(|a| (a.id.clone(), usage(cfg, &a.id).values().cloned().fold(0.0, f64::max)))
        .min_by(|x, y| x.1.partial_cmp(&y.1).unwrap()).map(|x| x.0)
}

// ---- finding and running CLIs ----------------------------------------------------------------

#[cfg(windows)]
const NO_WINDOW: u32 = 0x0800_0000;

pub fn bin_dir() -> PathBuf {
    home().join("bin") // where Clauracle puts a gh it downloaded
}

/// `claude`/`gh` on PATH, or where their installers put them (a .app doesn't get the login shell's PATH).
pub fn which(name: &str) -> Option<PathBuf> {
    let h = dirs::home_dir().unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    dirs.extend([bin_dir(), h.join(".local/bin"), h.join(".claude/local"), h.join(".npm-global/bin"),
                 h.join("google-cloud-sdk/bin"), dirs::data_local_dir().unwrap_or_default().join("Google/Cloud SDK/google-cloud-sdk/bin"),
                 "C:/Program Files (x86)/Google/Cloud SDK/google-cloud-sdk/bin".into(), "/opt/homebrew/bin".into(), "/usr/local/bin".into(), "/usr/bin".into(),
                 "C:/Program Files/Git/usr/bin".into()]); // last: Git for Windows' openssl (Mac signing)
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ".bat"] } else { &[""] };
    dirs.iter().flat_map(|d| exts.iter().map(move |e| d.join(format!("{name}{e}")))).find(|p| p.is_file())
}

/// #88: app launched from Start/Finder lacks shell PATH additions (rustup's ~/.cargo/bin…) → build steps/terminals
/// can't find cargo. Append existing toolchain dirs once at startup (before threads) → every child inherits.
pub fn fix_path() {
    let h = dirs::home_dir().unwrap_or_default();
    let mut p: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    for d in [h.join(".cargo/bin"), h.join(".local/bin"), h.join(".bun/bin"), h.join("go/bin"),
              "/opt/homebrew/bin".into(), "/usr/local/bin".into()] {
        if d.is_dir() && !p.contains(&d) { p.push(d); }
    }
    if let Ok(v) = std::env::join_paths(p) { std::env::set_var("PATH", v); }
}

/// Toolchains for local builds (#28): id → found. Only ones this OS can use; other OSes build in CI (#27).
/// ponytail: presence only, no version checks; Android paths = Android Studio defaults.
pub fn deps() -> serde_json::Value {
    let h = dirs::home_dir().unwrap_or_default();
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from).filter(|p| p.is_dir());
    let sdk = env("ANDROID_HOME").or_else(|| env("ANDROID_SDK_ROOT")).or_else(|| [dirs::data_local_dir().unwrap_or_default().join("Android/Sdk"),
        h.join("Library/Android/sdk"), h.join("Android/Sdk")].into_iter().find(|p| p.is_dir()));
    let ndk = env("NDK_HOME").is_some() || sdk.as_ref().is_some_and(|s| std::fs::read_dir(s.join("ndk")).is_ok_and(|mut r| r.next().is_some()));
    let mut m = serde_json::json!({"node": which("node").is_some(), "rust": which("cargo").is_some() || h.join(".cargo/bin").is_dir(),
        "jdk": which("java").is_some() || env("JAVA_HOME").is_some(), "android_sdk": sdk.is_some(), "android_ndk": ndk});
    if cfg!(target_os = "macos") { m["xcode"] = run("xcode-select", &["-p"], None, &[]).ok.into(); }
    if cfg!(target_os = "linux") { m["webkit"] = run("pkg-config", &["--exists", "webkit2gtk-4.1"], None, &[]).ok.into(); }
    m
}

pub fn command(program: &Path) -> Command {
    // .cmd shims (npm installs) must go through cmd
    let mut c = if program.extension().map_or(false, |e| e == "cmd" || e == "bat") {
        let mut c = Command::new("cmd");
        c.arg("/c").arg(program);
        c
    } else {
        Command::new(program)
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(NO_WINDOW);
    }
    c
}

pub struct Out {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

pub fn run(name: &str, args: &[&str], cwd: Option<&str>, env: &[(String, String)]) -> Out {
    let Some(p) = which(name) else {
        return Out { ok: false, stdout: String::new(), stderr: format!("{name} not found") };
    };
    let mut c = command(&p);
    c.args(args).envs(env.iter().cloned());
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    match c.output() {
        Ok(o) => Out { ok: o.status.success(), stdout: String::from_utf8_lossy(&o.stdout).into(),
                       stderr: String::from_utf8_lossy(&o.stderr).into() },
        Err(e) => Out { ok: false, stdout: String::new(), stderr: e.to_string() },
    }
}

// ---- GitHub: every account's token is handed to whoever needs it; nobody touches `gh auth switch` ----

static GH_USERS: Mutex<Option<Vec<String>>> = Mutex::new(None);

/// gh_accounts, remembered: `gh auth status` checks every account with GitHub (~1-2s). Forgotten on login/logout.
pub fn gh_users() -> Vec<String> {
    if let Some(u) = GH_USERS.lock().unwrap().clone() {
        return u;
    }
    let u = gh_accounts();
    *GH_USERS.lock().unwrap() = Some(u.clone());
    u
}

pub fn forget_gh_users() {
    *GH_USERS.lock().unwrap() = None;
}

pub fn gh_accounts() -> Vec<String> {
    let o = run("gh", &["auth", "status", "--json", "hosts"], None, &[]);
    serde_json::from_str::<serde_json::Value>(&o.stdout).ok()
        .and_then(|v| v["hosts"]["github.com"].as_array().cloned()).unwrap_or_default().iter()
        .filter(|a| a["state"] == "success").filter_map(|a| a["login"].as_str().map(String::from)).collect()
}

/// GH_TOKEN for gh, and git told to ask gh for github.com credentials (which then uses that same token).
pub fn gh_env(user: &str) -> Vec<(String, String)> {
    if user.is_empty() {
        return vec![];
    }
    let tok = run("gh", &["auth", "token", "-h", "github.com", "-u", user], None, &[]).stdout.trim().to_string();
    if tok.is_empty() {
        return vec![];
    }
    let mut env: Vec<(String, String)> = [("GH_TOKEN", tok.as_str()), ("GIT_CONFIG_COUNT", "2"),
     ("GIT_CONFIG_KEY_0", "credential.https://github.com.helper"), ("GIT_CONFIG_VALUE_0", ""),
     ("GIT_CONFIG_KEY_1", "credential.https://github.com.helper"), ("GIT_CONFIG_VALUE_1", "!gh auth git-credential")]
        .iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    // commit identity = this account (env beats .git/config user.email, which GitHub may map to another account)
    if let Some(email) = gh_noreply(user, &env) {
        for k in ["GIT_AUTHOR_NAME", "GIT_COMMITTER_NAME"] { env.push((k.into(), user.into())); }
        for k in ["GIT_AUTHOR_EMAIL", "GIT_COMMITTER_EMAIL"] { env.push((k.into(), email.clone())); }
    }
    env
}

static GH_NOREPLY: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// `{id}+{login}@users.noreply.github.com`: GitHub always attributes it to exactly this account. Cached per login.
fn gh_noreply(user: &str, env: &[(String, String)]) -> Option<String> {
    if let Some((_, e)) = GH_NOREPLY.lock().unwrap().iter().find(|(u, _)| u == user) {
        return Some(e.clone());
    }
    let id = run("gh", &["api", "user", "--jq", ".id"], None, env).stdout.trim().to_string();
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let e = format!("{id}+{user}@users.noreply.github.com");
    GH_NOREPLY.lock().unwrap().push((user.into(), e.clone()));
    Some(e)
}

pub fn gh_json(args: &[&str], user: &str, cwd: Option<&str>) -> serde_json::Value {
    let o = run("gh", args, cwd, &gh_env(user));
    if o.ok { serde_json::from_str(&o.stdout).unwrap_or(serde_json::Value::Null) } else { serde_json::Value::Null }
}

pub fn git_branch(cwd: &str) -> String {
    let o = run("git", &["status", "--porcelain", "-b"], Some(cwd), &[]);
    let lines: Vec<&str> = o.stdout.lines().collect();
    let Some(first) = lines.first() else { return String::new() };
    let branch = first.trim_start_matches("## ").split("...").next().unwrap_or("").to_string();
    if lines.len() > 1 { format!("{branch} · {} changed", lines.len() - 1) } else { branch }
}

/// owner, repo from a GitHub remote url (https or ssh)
pub fn parse_remote(url: &str) -> Option<(String, String)> {
    let re = Regex::new(r"github\.com[:/]([^/\s]+)/([^/\s]+?)(?:\.git)?/?$").unwrap();
    re.captures(url.trim()).map(|c| (c[1].to_string(), c[2].to_string()))
}

pub fn repo_of(cwd: &str) -> Option<(String, String)> {
    parse_remote(&run("git", &["remote", "get-url", "origin"], Some(cwd), &[]).stdout)
}

// ---- organize (#35): every project under root as <owner>/<repo> (GitHub origin) or local/<name> ----

/// Where `dir` belongs under `root`: SDK/tool → dependencies/<name>; GitHub clone named like its repo → <owner>/<repo>;
/// else local/<name> (#51: a renamed clone, e.g. AseShop from aseprite/aseprite = own project on someone's code).
pub fn place_of(root: &Path, dir: &str) -> PathBuf {
    let name = Path::new(dir).file_name().unwrap_or_default();
    if is_dep(Path::new(dir)) { return root.join("dependencies").join(name); }
    match repo_of(dir) {
        Some((o, r)) if r.eq_ignore_ascii_case(&name.to_string_lossy()) => root.join(o).join(r),
        _ => root.join("local").join(name),
    }
}

/// #51 SDK / tool install, not a project. ponytail: known names + "no project file and only binaries/one versioned
/// folder/bin"; misses → user's Move to….
pub fn is_dep(d: &Path) -> bool {
    const KNOWN: &[&str] = &["android-sdk", "flutter", "jdk", "ghidra", "nuget", "vcpkg", "emsdk", "depot_tools", "gradle", "maven",
        "llvm", "mingw64", "msys64", "cmake", "ue5", "unreal", "dotnet", ".gradle", ".pub-cache"];
    let n = d.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
    if KNOWN.contains(&n.as_str()) || n.starts_with("jdk-") || n.starts_with("android-sdk") { return true; }
    if d.join(".git").exists() { return false; }
    let kids: Vec<PathBuf> = std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path()).collect();
    const PROJ: &[&str] = &["package.json", "cargo.toml", "pyproject.toml", "go.mod", "cmakelists.txt", "makefile", "pubspec.yaml",
        "build.gradle", "build.gradle.kts", "src", "readme.md", "index.html"];
    let lower = |p: &PathBuf| p.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
    if kids.is_empty() || kids.iter().any(|k| { let l = lower(k); PROJ.contains(&l.as_str()) || [".sln", ".csproj", ".uproject"].iter().any(|e| l.ends_with(e)) }) { return false; }
    let ver = Regex::new(r"\d+\.\d+").unwrap();
    let bin = |k: &PathBuf| k.is_file() && [".exe", ".dll", ".zip", ".nupkg", ".msi", ".jar"].iter().any(|e| lower(k).ends_with(e));
    kids.iter().all(bin) || (kids.len() == 1 && kids[0].is_dir() && ver.is_match(&lower(&kids[0]))) || d.join("bin").is_dir()
}
pub fn same_path(a: &Path, b: &Path) -> bool {
    let n = |p: &Path| { let s = p.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_string(); if cfg!(windows) { s.to_lowercase() } else { s } };
    n(a) == n(b)
}

/// Folders under `root` that look like projects: git repos (depth ≤ 3, not descended into) + plain folders straight in
/// root or root/local. Owner folders (holding repos) are containers, not projects. ponytail: depth cap 3.
pub fn scan_projects(root: &Path) -> Vec<PathBuf> {
    fn walk(d: &Path, depth: u8, root: &Path, out: &mut Vec<PathBuf>) -> bool { // → found a repo below
        let kids: Vec<PathBuf> = std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path())
            .filter(|p| p.symlink_metadata().is_ok_and(|m| m.is_dir()) && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))).collect();
        let mut any = false;
        for k in kids {
            if k.join(".git").exists() { out.push(k); any = true; continue; }
            let below = depth < 3 && walk(&k, depth + 1, root, out);
            let loose = (depth == 1 && k.file_name().is_some_and(|n| n != "local" && n != "dependencies"))
                || (depth == 2 && (same_path(d, &root.join("local")) || same_path(d, &root.join("dependencies"))));
            if !below && loose { out.push(k); }
            any |= below;
        }
        any
    }
    let mut out = vec![];
    walk(root, 1, root, &mut out);
    out
}

/// Claude Code keeps a project's transcripts in ~/.claude/projects/<path, every non-alphanumeric char → '-'>.
pub fn claude_dir_of(path: &Path) -> PathBuf {
    let enc: String = path.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    dirs::home_dir().unwrap_or_default().join(".claude/projects").join(enc)
}

/// Move a folder: rename (same drive, atomic) else copy then delete (other drive; a failed copy is removed, source kept).
/// Claude's transcript folder follows so sessions still resume. Caches git ignores are dropped first (rebuilt anyway).
pub fn move_dir(from: &Path, to: &Path) -> Result<(), String> {
    if !from.is_dir() { return Err(format!("{} is not a folder", from.display())); }
    if to.exists() { return Err(format!("{} already exists", to.display())); }
    if to.starts_with(from) { return Err("can't move a folder into itself".into()); }
    std::fs::create_dir_all(to.parent().ok_or("bad target")?).map_err(|e| e.to_string())?;
    if std::fs::rename(from, to).is_err() {
        clean(&from.to_string_lossy(), &[]);
        let (f, t) = (from.to_string_lossy().to_string(), to.to_string_lossy().to_string());
        let ok = if cfg!(windows) {
            // robocopy exit < 8 = copied; /SL keeps links as links; /NFL /NDL /NJH /NJS quiet
            command(Path::new("robocopy")).args([f.as_str(), t.as_str(), "/E", "/SL", "/COPY:DAT", "/R:1", "/W:1", "/NFL", "/NDL", "/NJH", "/NJS"])
                .status().is_ok_and(|s| s.code().is_some_and(|c| c < 8))
        } else { run("cp", &["-a", &f, &t], None, &[]).ok };
        if !ok { let _ = std::fs::remove_dir_all(to); return Err("copy failed (a file in use?); nothing moved".into()); }
        let _ = std::fs::remove_dir_all(from); // ponytail: a locked leftover stays behind; the copy is complete
    }
    let (a, b) = (claude_dir_of(from), claude_dir_of(to));
    if a.is_dir() && !b.exists() { let _ = std::fs::rename(&a, &b); }
    Ok(())
}

/// Who should run a repo, by code: the gh account that owns it, else the first with push access;
/// then the AI account tied to that gh (else the default). shared = owner isn't one of ours.
#[derive(Serialize, Debug, PartialEq)]
pub struct Who { pub owner: String, pub shared: bool, pub gh: String, pub account: String }
pub fn suggest(cfg: &Config, owner: &str, users: &[String], can_push: impl Fn(&str) -> bool) -> Who {
    let mine = users.iter().find(|u| u.eq_ignore_ascii_case(owner));
    let gh = mine.or_else(|| users.iter().find(|u| can_push(u))).cloned().unwrap_or_default();
    let account = cfg.accounts.iter().find(|a| !gh.is_empty() && a.gh.eq_ignore_ascii_case(&gh))
        .map_or(cfg.default_account.clone(), |a| a.id.clone());
    Who { owner: owner.into(), shared: mine.is_none(), gh, account }
}

pub fn can_push(user: &str, owner: &str, repo: &str) -> bool {
    gh_json(&["api", &format!("repos/{owner}/{repo}")], user, None)["permissions"]["push"] == true
}

// ---- Connections & Skills ---------------------------------------------------------------------

pub fn mcp_line(line: &str) -> Option<(String, String)> {
    let re = Regex::new(r"^(\S.*?): .* - (✔|✓|!|✘|✗) ?(.*)$").unwrap();
    re.captures(line.trim()).map(|c| (c[1].to_string(), c[3].to_string()))
}

pub fn server_id(label: &str) -> String {
    Regex::new(r"\W").unwrap().replace_all(label, "_").to_string() // "claude.ai Gmail" -> mcp__claude_ai_Gmail__*
}

/// MCP servers (from `claude mcp list`, the one place that also knows claude.ai connectors) and skills
/// (yours, installed plugins', Clauracle's own).
pub fn catalog(plugin_dir: &Path) -> Vec<Item> {
    let mut items = vec![];
    for line in run("claude", &["mcp", "list"], None, &[]).stdout.lines() {
        if let Some((label, note)) = mcp_line(line) {
            items.push(Item { id: format!("mcp:{}", server_id(&label)), label, kind: "mcp".into(), note });
        }
    }
    let h = dirs::home_dir().unwrap_or_default();
    let mut roots = vec![(String::new(), h.join(".claude/skills")), ("clauracle:".into(), plugin_dir.join("skills"))];
    if let Ok(s) = std::fs::read_to_string(h.join(".claude/plugins/installed_plugins.json")) {
        let v: serde_json::Value = serde_json::from_str(&s).unwrap_or_default();
        for (name, inst) in v["plugins"].as_object().cloned().unwrap_or_default() {
            if let Some(path) = inst[0]["installPath"].as_str() {
                roots.push((format!("{}:", name.split('@').next().unwrap_or(&name)), Path::new(path).join("skills")));
            }
        }
    }
    // plugins + inline mods as items of their own: one only loads (hooks, MCP, agents, skills) when it or one of its
    // skills is picked. ponytail@ponytail never: Clauracle bundles its own (else Ponytail is injected twice).
    let mut plugins: Vec<String> = std::fs::read_to_string(h.join(".claude/plugins/installed_plugins.json")).ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["plugins"].as_object().map(|o| o.keys().cloned().collect())).unwrap_or_default();
    plugins.extend(std::fs::read_dir(h.join(".claude/mods")).into_iter().flatten().flatten()
        .filter(|e| e.path().is_dir()).map(|e| format!("{}@inline", e.file_name().to_string_lossy())));
    plugins.sort();
    for k in plugins.into_iter().filter(|k| k != PONYTAIL_PLUGIN) {
        let name = k.split('@').next().unwrap_or(&k).to_string();
        let note = if k.ends_with("@inline") { "mod".into() } else { format!("plugin · {}", k.split('@').nth(1).unwrap_or("")) };
        items.push(Item { id: format!("plugin:{k}"), label: name, kind: "plugin".into(), note });
    }
    for (prefix, root) in roots {
        let mut names: Vec<String> = std::fs::read_dir(&root).into_iter().flatten().flatten()
            .filter(|e| e.path().join("SKILL.md").is_file())
            .map(|e| e.file_name().to_string_lossy().to_string()).collect();
        names.sort();
        for n in names {
            let note = if prefix.is_empty() { "your skills".into() } else { prefix.trim_end_matches(':').to_string() };
            items.push(Item { id: format!("skill:{prefix}{n}"), label: format!("{prefix}{n}"), kind: "skill".into(), note });
        }
    }
    items
}

/// Disabled MCP servers become deny rules; skills an allowlist for `initialize` (None = all of them).
/// A project with `on` gets only what it picked (Clauracle's own skills always); a legacy one (`on` None)
/// everything but its `off`. The global `off` switches things off for every project.
const PONYTAIL_PLUGIN: &str = "ponytail@ponytail";

/// Picked for this project? (global `off` wins; Clauracle's own skills always on; legacy `on` None = all but `off`)
fn is_on(cfg: &Config, p: &Project, id: &str) -> bool {
    let has = |l: &[String], id: &str| l.iter().any(|x| x == id);
    !has(&cfg.off, id) && (id.starts_with("skill:clauracle:") || match &p.on {
        Some(on) => has(on, id),
        None => !has(&p.off, id),
    })
}

/// The project's picks at the source, as CLI `--settings`: unpicked MCP servers (claude.ai connectors too) never start
/// (deniedMcpServers), unpicked plugins/mods don't load at all (enabledPlugins false: no hooks, MCP, agents, skills).
/// ponytail: deny-by-name from the catalog → a server added after the last scan runs until a rescan.
pub fn cli_settings(cfg: &Config, p: &Project) -> serde_json::Value {
    let denied: Vec<serde_json::Value> = cfg.catalog.iter().filter(|i| i.kind == "mcp" && !is_on(cfg, p, &i.id))
        .map(|i| serde_json::json!({"serverName": i.label})).collect();
    let mut plugins = serde_json::Map::new();
    plugins.insert(PONYTAIL_PLUGIN.into(), false.into());
    for i in cfg.catalog.iter().filter(|i| i.kind == "plugin") {
        let key = &i.id["plugin:".len()..];
        let skill_pre = format!("skill:{}:", key.split('@').next().unwrap_or(key));
        let on = is_on(cfg, p, &i.id) || cfg.catalog.iter().any(|s| s.id.starts_with(&skill_pre) && is_on(cfg, p, &s.id));
        if !on { plugins.insert(key.into(), false.into()); }
    }
    serde_json::json!({"deniedMcpServers": denied, "enabledPlugins": plugins})
}

pub fn restrictions(cfg: &Config, p: &Project) -> (Vec<String>, Option<Vec<String>>) {
    let enabled = |id: &str| is_on(cfg, p, id);
    let has = |l: &[String], id: &str| l.iter().any(|x| x == id);
    // ponytail: built from the catalog, so a server added after the last scan stays on until a rescan
    let ids: Vec<String> = cfg.catalog.iter().map(|i| i.id.clone()).chain(cfg.off.iter().cloned()).chain(p.off.iter().cloned()).collect();
    let mut deny: Vec<String> = ids.iter().filter(|i| !enabled(i)).filter_map(|i| i.strip_prefix("mcp:")).map(|s| format!("mcp__{s}")).collect();
    deny.sort();
    deny.dedup();
    if has(&cfg.off, "web") { // composer's globe: web search off everywhere
        deny.extend(["WebSearch", "WebFetch"].map(String::from));
    }
    if p.hide_env {
        deny.extend(["Read(**/.env*)", "Edit(**/.env*)", "Write(**/.env*)", "Bash(cat *.env*)"].map(String::from));
    }
    let skills = cfg.catalog.iter().filter(|i| i.kind == "skill");
    let allow = (p.on.is_some() || skills.clone().any(|i| !enabled(&i.id)))
        .then(|| skills.filter(|i| enabled(&i.id)).map(|i| i.id["skill:".len()..].to_string()).collect());
    (deny, allow)
}

// ---- `claude setup-token` in a pseudo-terminal (it draws with Ink, which prints nothing through pipes) ----

pub fn setup_token(on_url: impl Fn(String)) -> Result<String, String> {
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    let claude = which("claude").ok_or("Claude Code isn't installed yet")?;
    let pty = native_pty_system();
    let pair = pty.openpty(PtySize { rows: 50, cols: 1000, pixel_width: 0, pixel_height: 0 }) // wide: no wrapping
        .map_err(|e| e.to_string())?;
    let mut cmd = CommandBuilder::new(claude);
    cmd.arg("setup-token");
    let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let result = read_setup_token(&mut reader, on_url);
    let _ = child.kill();
    result.ok_or_else(|| "setup-token ended without a token".into())
}

pub fn read_setup_token(reader: &mut dyn std::io::Read, on_url: impl Fn(String)) -> Option<String> {
    // both need a character after the match, so a read that stops mid-token doesn't return half of it
    let url_re = Regex::new(r"https://\S+?/oauth/authorize\?[^\s\x1b\x07]+[\s\x1b\x07]").unwrap();
    let tok_re = Regex::new(r"sk-ant-oat\d+-[\w-]{20,}[^\w-]").unwrap();
    let (mut out, mut seen_url, mut buf) = (String::new(), false, [0u8; 4096]);
    loop {
        let n = reader.read(&mut buf).ok()?;
        if n == 0 {
            return None;
        }
        out.push_str(&String::from_utf8_lossy(&buf[..n]));
        if !seen_url {
            if let Some(m) = url_re.find(&out) {
                seen_url = true;
                on_url(m.as_str()[..m.as_str().len() - 1].to_string());
            }
        }
        if let Some(m) = tok_re.find(&out) {
            return Some(m.as_str()[..m.as_str().len() - 1].to_string());
        }
    }
}

pub fn token_ok(t: &str) -> bool {
    Regex::new(r"^sk-ant-oat\d+-[\w-]{20,}$").unwrap().is_match(t.trim())
}

// ---- nothing to install by hand: fetch the official Claude Code installer / the gh release ----

// ---- Google Cloud (gcloud) ----

/// A project's Google account + Cloud project, per process (like GH_TOKEN): never `gcloud config set`.
pub fn gcloud_env(account: &str, project: &str) -> Vec<(String, String)> {
    let mut v = vec![];
    if !account.is_empty() { v.push(("CLOUDSDK_CORE_ACCOUNT".to_string(), account.to_string())); }
    if !project.is_empty() { v.push(("CLOUDSDK_CORE_PROJECT".to_string(), project.to_string())); }
    v
}

pub fn gcloud(args: &[&str], account: &str, project: &str) -> Result<serde_json::Value, String> {
    let mut a = args.to_vec();
    a.extend(["--format=json", "--quiet"]);
    let o = run("gcloud", &a, None, &gcloud_env(account, project));
    if o.ok {
        Ok(serde_json::from_str(&o.stdout).unwrap_or(serde_json::Value::Null))
    } else {
        Err(o.stderr.lines().filter(|l| !l.trim().is_empty()).last().unwrap_or("gcloud failed").to_string())
    }
}

/// Google's own installer: silent NSIS on Windows (per user), install script elsewhere (~/google-cloud-sdk).
pub fn install_gcloud() -> Result<(), String> {
    let o = if cfg!(windows) {
        let exe = std::env::temp_dir().join("GoogleCloudSDKInstaller.exe");
        let mut body = vec![];
        ureq::get("https://dl.google.com/dl/cloudsdk/channels/rapid/GoogleCloudSDKInstaller.exe").call()
            .map_err(|e| e.to_string())?.into_reader().read_to_end(&mut body).map_err(|e| e.to_string())?;
        std::fs::write(&exe, body).map_err(|e| e.to_string())?;
        command(&exe).args(["/S", "/noreporting", "/nostartmenu", "/nodesktop"]).output()
    } else {
        Command::new("bash").args(["-c", "curl -fsSL https://sdk.cloud.google.com | bash -s -- --disable-prompts --install-dir=$HOME"]).output()
    }.map_err(|e| e.to_string())?;
    if which("gcloud").is_some() { Ok(()) } else { Err(String::from_utf8_lossy(&o.stderr).trim().to_string() + " (gcloud not found after install)") }
}

pub fn install_claude() -> Result<(), String> {
    let o = if cfg!(windows) {
        let mut c = command(Path::new("powershell"));
        c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", "irm https://claude.ai/install.ps1 | iex"]);
        c.output()
    } else {
        Command::new("bash").args(["-c", "curl -fsSL https://claude.ai/install.sh | bash"]).output()
    }.map_err(|e| e.to_string())?;
    if which("claude").is_some() { Ok(()) } else { Err(String::from_utf8_lossy(&o.stderr).into()) }
}

pub fn install_gh() -> Result<(), String> {
    let rel: serde_json::Value = ureq::get("https://api.github.com/repos/cli/cli/releases/latest")
        .set("User-Agent", "clauracle").call().map_err(|e| e.to_string())?.into_string().ok()
        .and_then(|s| serde_json::from_str(&s).ok()).ok_or("couldn't read the gh release list")?;
    let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "amd64" };
    let want = if cfg!(windows) { format!("_windows_{arch}.zip") }
               else if cfg!(target_os = "macos") { "_macOS_universal.zip".into() }
               else { format!("_linux_{arch}.tar.gz") };
    let asset = rel["assets"].as_array().into_iter().flatten()
        .find(|a| a["name"].as_str().map_or(false, |n| n.ends_with(&want))).ok_or("no gh build for this system")?;
    let tmp = home().join("gh-download");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let file = tmp.join(asset["name"].as_str().unwrap());
    let mut body = ureq::get(asset["browser_download_url"].as_str().unwrap()).call().map_err(|e| e.to_string())?.into_reader();
    std::io::copy(&mut body, &mut std::fs::File::create(&file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    // bsdtar unpacks zip and tar.gz alike and ships with Windows 10+, macOS and Linux
    let mut t = command(Path::new("tar"));
    t.arg("-xf").arg(&file).arg("-C").arg(&tmp).output().map_err(|e| e.to_string())?;
    let exe = if cfg!(windows) { "gh.exe" } else { "gh" };
    let found = find_file(&tmp, exe).ok_or("gh not found in the download")?;
    std::fs::create_dir_all(bin_dir()).map_err(|e| e.to_string())?;
    std::fs::copy(&found, bin_dir().join(exe)).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(bin_dir().join(exe), std::fs::Permissions::from_mode(0o755));
    }
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(f) = find_file(&p, name) {
                return Some(f);
            }
        } else if p.file_name().map_or(false, |n| n == name) && p.parent().map_or(false, |d| d.ends_with("bin")) {
            return Some(p);
        }
    }
    None
}

// ---- .env files: keys are shown, values stay masked in the UI until revealed ----

pub fn env_files(dir: &str) -> Vec<String> {
    let mut f: Vec<String> = std::fs::read_dir(dir).into_iter().flatten().flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n == ".env" || n.starts_with(".env.")).collect();
    f.sort();
    f
}

/// KEY=value pairs in file order; comments and blank lines are kept on write.
pub fn env_parse(text: &str) -> Vec<(String, String)> {
    text.lines().filter_map(|l| {
        let l = l.trim().trim_start_matches("export ");
        if l.starts_with('#') { return None; }
        let (k, v) = l.split_once('=')?;
        let v = v.trim();
        let v = if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
            unescape(&v[1..v.len() - 1])
        } else if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') {
            v[1..v.len() - 1].to_string()
        } else {
            v.to_string()
        };
        Some((k.trim().to_string(), v))
    }).collect()
}

fn unescape(s: &str) -> String {
    let (mut out, mut it) = (String::new(), s.chars());
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some(o) => out.push(o),
            None => out.push('\\'),
        }
    }
    out
}

/// Set (Some) or delete (None) one key, keeping everything else in the file as it was.
pub fn env_set(text: &str, key: &str, value: Option<&str>) -> String {
    let quote = |v: &str| if v.chars().any(|c| c.is_whitespace() || "#\"'\\$".contains(c)) {
        format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n"))
    } else { v.to_string() };
    let mut found = false;
    let mut out: Vec<String> = text.lines().filter_map(|l| {
        let t = l.trim().trim_start_matches("export ");
        if t.split_once('=').map_or(false, |(k, _)| k.trim() == key) && !t.starts_with('#') {
            found = true;
            return value.map(|v| format!("{key}={}", quote(v)));
        }
        Some(l.to_string())
    }).collect();
    if !found {
        if let Some(v) = value { out.push(format!("{key}={}", quote(v))); }
    }
    out.join("\n") + "\n"
}

/// Is the file ignored by git (so secrets don't get committed)? None outside a repo.
/// A local-only folder becomes a new GitHub repo (`name` or `owner/name`) as `user`: git init if needed,
/// .env* git-ignored before anything is committed, a first commit if there is none, then create + push.
pub fn publish(dir: &str, user: &str, name: &str, private: bool) -> Result<String, String> {
    let env = gh_env(user);
    let git = |args: &[&str]| run("git", args, Some(dir), &env);
    let fail = |o: Out| Err(if o.stderr.trim().is_empty() { o.stdout.trim().to_string() } else { o.stderr.trim().to_string() });
    if !Path::new(dir).join(".git").exists() {
        let o = git(&["init", "-b", "main"]);
        if !o.ok { return fail(o); }
    }
    if git_ignored(dir, ".env") == Some(false) {
        let gi = Path::new(dir).join(".gitignore");
        let mut t = std::fs::read_to_string(&gi).unwrap_or_default();
        if !t.is_empty() && !t.ends_with('\n') { t.push('\n'); }
        std::fs::write(&gi, t + ".env*\n").map_err(|e| e.to_string())?;
    }
    if !git(&["rev-parse", "--verify", "HEAD"]).ok {
        git(&["add", "-A"]);
        let o = git(&["commit", "-m", "Initial commit"]);
        if !o.ok { return fail(o); }
    }
    let vis = if private { "--private" } else { "--public" };
    let o = run("gh", &["repo", "create", name, vis, "--source", ".", "--remote", "origin", "--push"], Some(dir), &env);
    if o.ok { Ok(o.stdout.trim().to_string()) } else { fail(o) }
}

/// Branch actions from the header pill, as the project's gh account: list | switch <b> | new <b> | push.
pub fn git_do(dir: &str, user: &str, action: &str, arg: &str) -> Result<serde_json::Value, String> {
    let env = gh_env(user);
    let git = |args: &[&str]| run("git", args, Some(dir), &env);
    // a branch name never starts with '-' (would read as a flag)
    if arg.starts_with('-') && action != "commit" { return Err("bad branch name".into()); }
    let o = match action {
        // uncommitted/unpushed state for the GitHub tab
        "status" => {
            let files: Vec<String> = git(&["status", "--porcelain"]).stdout.lines().map(|l| l.get(3..).unwrap_or("").to_string()).collect();
            let up = git(&["rev-list", "--count", "@{u}..HEAD"]);
            return Ok(serde_json::json!({"files": files, "ahead": if up.ok { up.stdout.trim().parse::<u32>().unwrap_or(0) } else { 0 },
                                         "upstream": up.ok}));
        }
        // everything, one commit, pushed (`-m` keeps a message starting with '-' a message)
        "commit" => {
            git(&["add", "-A"]);
            let c = git(&["commit", "-m", arg]);
            if !c.ok { return Err(if c.stdout.trim().is_empty() { c.stderr } else { c.stdout }.trim().to_string()); }
            git(&["push", "-u", "origin", "HEAD"])
        }
        "list" => {
            let b = git(&["branch", "--format=%(refname:short)"]).stdout;
            return Ok(serde_json::json!({"current": git(&["branch", "--show-current"]).stdout.trim(),
                                         "branches": b.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>()}));
        }
        "switch" => git(&["switch", arg]),
        "new" => git(&["switch", "-c", arg]),
        "push" => git(&["push", "-u", "origin", "HEAD"]),
        _ => return Err(format!("unknown action {action}")),
    };
    // git writes progress to stderr even on success
    if o.ok { Ok(serde_json::json!(o.stderr.trim())) } else { Err(if o.stderr.trim().is_empty() { o.stdout } else { o.stderr }.trim().to_string()) }
}

/// After a turn: push the current branch if it's ahead of its upstream → Some(n pushed). Never force.
/// ponytail: only branches with an upstream (a never-pushed local branch may be private); failure = Err for a note.
pub fn auto_push(dir: &str, user: &str) -> Result<u32, String> {
    let env = gh_env(user);
    let up = run("git", &["rev-list", "--count", "@{u}..HEAD"], Some(dir), &env);
    let n = if up.ok { up.stdout.trim().parse::<u32>().unwrap_or(0) } else { 0 };
    if n == 0 { return Ok(0); }
    let o = run("git", &["push"], Some(dir), &env);
    if o.ok { Ok(n) } else { Err(o.stderr.trim().lines().last().unwrap_or("").to_string()) }
}

/// Recent build outputs worth releasing (installers, packages, archives) in the usual output folders, newest first.
/// ponytail: fixed folder list + depth 5; add a per-project artifact glob in Build if a project hides them elsewhere
pub fn builds(dir: &str) -> Vec<serde_json::Value> {
    const DIRS: [&str; 9] = ["dist", "build", "out", "release", "publish", "bin", "target/release/bundle", "src-tauri/target/release/bundle", "app/build/outputs"];
    const EXT: [&str; 13] = ["exe", "msi", "msix", "dmg", "pkg", "appimage", "deb", "rpm", "apk", "aab", "ipa", "zip", "tar.gz"];
    fn walk(p: &Path, depth: u8, out: &mut Vec<(std::time::SystemTime, std::path::PathBuf, u64)>) {
        let Ok(rd) = std::fs::read_dir(p) else { return };
        for e in rd.flatten() {
            let path = e.path();
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() { if depth > 0 { walk(&path, depth - 1, out); } continue; }
            let n = path.to_string_lossy().to_lowercase();
            if EXT.iter().any(|x| n.ends_with(&format!(".{x}"))) {
                out.push((m.modified().unwrap_or(std::time::UNIX_EPOCH), path, m.len()));
            }
        }
    }
    let mut out = vec![];
    for d in DIRS { walk(&Path::new(dir).join(d), 5, &mut out); }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out.dedup_by(|a, b| a.1 == b.1);
    out.into_iter().take(12).map(|(t, p, size)| serde_json::json!({"path": p.to_string_lossy(), "name": p.file_name().map(|n| n.to_string_lossy().to_string()),
        "size": size, "mtime": t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)})).collect()
}

// ---- disk: build/cache folders a build regenerates. Safety: only fixed names AND git-ignored (no git → never touched) ----
const CACHE_DIRS: [&str; 17] = ["target", "src-tauri/target", "dist", "build", "out", ".next", ".nuxt", ".turbo", ".parcel-cache",
    ".gradle", "node_modules/.cache", ".pytest_cache", ".mypy_cache", ".ruff_cache", "coverage", "test-results", "playwright-report"];
pub const IDLE_CLEAN_DAYS: u64 = 14; // ponytail: fixed; a setting if someone wants caches kept longer

fn dir_size(p: &Path) -> u64 {
    // symlink_metadata: links/junctions count as themselves, never followed
    std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| match e.path().symlink_metadata() {
        Ok(m) if m.is_dir() => dir_size(&e.path()),
        Ok(m) => m.len(),
        _ => 0,
    }).sum()
}

/// Build tab auto-setup: guess build/run/deploy from project files, no tokens. Most specific first (tauri > npm > cargo…);
/// a field stays empty when nothing fits. ponytail: root files only; monorepos/subfolders → user edits or asks Claude.
pub fn guess_build(dir: &str) -> Build {
    let p = Path::new(dir);
    let read = |f: &str| std::fs::read_to_string(p.join(f)).unwrap_or_default();
    let has = |f: &str| p.join(f).exists();
    let mut b = Build::default();
    let set = |b: &mut Build, build: &str, run: &str, deploy: &str| {
        for (slot, v) in [(&mut b.build, build), (&mut b.run, run), (&mut b.deploy, deploy)] {
            if slot.is_empty() { *slot = v.to_string(); }
        }
    };
    if has("src-tauri/tauri.conf.json") { set(&mut b, "npx tauri build", "npx tauri dev", ""); }
    if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&read("package.json")) {
        let pm = if has("pnpm-lock.yaml") { "pnpm" } else if has("yarn.lock") { "yarn" }
            else if has("bun.lockb") || has("bun.lock") { "bun" } else { "npm" };
        let s = |k: &str| pkg["scripts"].get(k).map(|_| format!("{pm} run {k}")).unwrap_or_default();
        let run = ["dev", "start", "serve"].iter().map(|k| s(k)).find(|c| !c.is_empty()).unwrap_or_default();
        set(&mut b, &s("build"), &run, &s("deploy"));
    }
    if has("Cargo.toml") { set(&mut b, "cargo build --release", "cargo run", ""); }
    if has("go.mod") { set(&mut b, "go build ./...", "go run .", ""); }
    if has("project.godot") { set(&mut b, "", "godot --path .", ""); }
    if let Some(f) = ["main.py", "app.py"].into_iter().find(|f| has(f)) { set(&mut b, "", &format!("python {f}"), ""); }
    let mk = read("Makefile");
    let target = |t: &str| mk.lines().any(|l| l.starts_with(&format!("{t}:")));
    if !mk.is_empty() {
        set(&mut b, "make", if target("run") { "make run" } else { "" }, if target("deploy") { "make deploy" } else { "" });
    }
    b
}

/// HEAD sha ("" outside git / no commits).
pub fn head(dir: &str) -> String {
    run("git", &["rev-parse", "HEAD"], Some(dir), &[]).stdout.trim().to_string()
}

/// #89 smart build: why the changes since `base` (sha; "" = HEAD) deserve build + run, "" = skip.
pub fn smart_why(dir: &str, base: &str) -> String {
    let base = if base.is_empty() { "HEAD" } else { base };
    let stat = run("git", &["diff", "--numstat", base], Some(dir), &[]).stdout;
    let log = run("git", &["log", "--format=%s", &format!("{base}..HEAD")], Some(dir), &[]).stdout;
    smart_pick(&stat, &log)
}

/// Not code: docs and meta files (smart build skip, #92 stale-build check).
pub fn is_doc(f: &str) -> bool { f.starts_with("docs/") || [".md", ".txt", ".rst", ".adoc", "LICENSE", ".gitignore"].iter().any(|x| f.ends_with(x)) }

/// #92: unix time of the newest commit touching code (≠ docs, ≠ lock files: the post-build commit only bumps Cargo.lock…), 0 = none.
/// ponytail: last 200 commits; older code change → 0 → no stale warning.
pub fn code_time(dir: &str) -> u64 {
    let lock = |f: &str| f.ends_with(".lock") || f.ends_with("-lock.json") || f.ends_with("-lock.yaml");
    let out = run("git", &["log", "-n", "200", "--format=%x01%ct", "--name-only"], Some(dir), &[]).stdout;
    out.split('\u{1}').find_map(|c| { let mut l = c.lines(); let t = l.next()?.trim().parse::<u64>().ok()?;
        l.any(|f| !f.is_empty() && !is_doc(f) && !lock(f)).then_some(t) }).unwrap_or(0)
}

/// Doc-only → skip; a fix (commit subject words) → build; else ≥30 changed lines or ≥3 code files.
/// ponytail: heuristic on numstat + subjects; untracked files unseen (Claude commits). Tune thresholds if it nags.
pub fn smart_pick(numstat: &str, log: &str) -> String {
    let (mut files, mut lines) = (0u64, 0u64);
    for l in numstat.lines() {
        let c: Vec<&str> = l.split('\t').collect();
        if c.len() < 3 || is_doc(c[2]) { continue; }
        files += 1;
        lines += c[0].parse::<u64>().unwrap_or(1) + c[1].parse::<u64>().unwrap_or(0); // binary "-" = 1
    }
    if files == 0 { return String::new(); }
    let low = log.to_lowercase();
    if ["fix", "crash", "critical", "broken", "bug", "regression"].iter().any(|w| low.contains(w)) {
        return format!("a fix in {files} code file{}", if files > 1 { "s" } else { "" });
    }
    if lines >= 30 || files >= 3 { format!("{lines} changed lines in {files} code files") } else { String::new() }
}

#[test]
fn smart_pick_works() {
    assert_eq!(smart_pick("3\t1\tREADME.md\n200\t0\tdocs/a.html\n", "Fix typo"), "");
    assert_eq!(smart_pick("2\t1\tsrc/a.rs\n", "Tweak label"), "");
    assert!(!smart_pick("2\t1\tsrc/a.rs\n", "Fix crash on open").is_empty());
    assert!(!smart_pick("25\t10\tsrc/a.rs\n", "Add thing").is_empty());
    assert!(!smart_pick("1\t0\ta.rs\n1\t0\tb.rs\n-\t-\tc.png\n", "x").is_empty());
}

/// (folder relative to the project, bytes) for each cache folder present and git-ignored.
pub fn caches(dir: &str) -> Vec<(String, u64)> {
    CACHE_DIRS.iter().filter(|d| Path::new(dir).join(d).symlink_metadata().map_or(false, |m| m.is_dir()))
        .filter(|d| run("git", &["check-ignore", "-q", d], Some(dir), &[]).ok)
        .map(|d| (d.to_string(), dir_size(&Path::new(dir).join(d)))).collect()
}

/// Delete the cache folders named in `which` (empty = all); bytes freed. Re-checked here: the page isn't trusted with paths.
pub fn clean(dir: &str, which: &[String]) -> u64 {
    caches(dir).into_iter().filter(|(d, _)| which.is_empty() || which.contains(d))
        .filter(|(d, _)| std::fs::remove_dir_all(Path::new(dir).join(d)).is_ok()).map(|(_, n)| n).sum()
}

/// Days since the project's git index changed (add/commit/checkout); None without git.
pub fn idle_days(dir: &str) -> Option<u64> {
    let m = std::fs::metadata(Path::new(dir).join(".git/index")).ok()?.modified().ok()?;
    Some(m.elapsed().ok()?.as_secs() / 86400)
}

// ---- Storage Janitor (#26): startup + every JANITOR_EVERY. Rules: idle ≥ IDLE_CLEAN_DAYS → clean; caches > CACHE_CAP → clean;
// drive low (free < max(10 GB, 10%)) → clean most-idle projects first till not low, then old Claude transcripts. ----
pub const JANITOR_EVERY: u64 = 6 * 3600;
const CACHE_CAP: u64 = 10 << 30; // ponytail: fixed caps; settings if someone needs other numbers
const GB: u64 = 1 << 30;

/// (free, total) bytes of the drive holding `dir`.
#[cfg(windows)]
pub fn disk_free(dir: &str) -> Option<(u64, u64)> {
    use std::os::windows::ffi::OsStrExt;
    let w: Vec<u16> = std::ffi::OsStr::new(dir).encode_wide().chain([0]).collect();
    let (mut free, mut total) = (0u64, 0u64);
    // SAFETY: w is NUL-terminated; out-params are valid u64s
    let ok = unsafe { windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(w.as_ptr(), &mut free, &mut total, std::ptr::null_mut()) };
    (ok != 0).then_some((free, total))
}
#[cfg(not(windows))]
pub fn disk_free(dir: &str) -> Option<(u64, u64)> {
    let o = run("df", &["-Pk", dir], None, &[]); // POSIX: 2nd line = fs total used avail …, in KiB
    let f: Vec<u64> = o.stdout.lines().nth(1)?.split_whitespace().skip(1).take(3).filter_map(|x| x.parse().ok()).collect();
    (f.len() == 3).then(|| (f[2] * 1024, f[0] * 1024))
}
pub fn disk_low(dir: &str) -> bool { disk_free(dir).is_some_and(|(f, t)| f < (10 * GB).max(t / 10)) }

/// One janitor pass over (path, turn running → left alone); keep = session ids Clauracle resumes. Returns bytes freed.
pub fn janitor(projects: &[(String, bool)], keep_sessions: &[String]) -> u64 {
    let mut freed = 0;
    let mut idle: Vec<(u64, &String)> = projects.iter().filter(|(_, busy)| !busy).map(|(d, _)| (idle_days(d).unwrap_or(0), d)).collect();
    idle.sort_by(|a, b| b.0.cmp(&a.0)); // most idle first
    for (days, d) in &idle {
        let total: u64 = caches(d).iter().map(|c| c.1).sum();
        if total > 0 && (*days >= IDLE_CLEAN_DAYS || total > CACHE_CAP || disk_low(d)) { freed += clean(d, &[]); }
    }
    let Some(home) = dirs::home_dir() else { return freed };
    let low = disk_low(&home.to_string_lossy());
    freed + claude_clean(&home, keep_sessions, low)
}

/// Claude Code leftovers: installer versions but the newest (it keeps several, ~300 MB each); when the home drive is low,
/// transcripts (projects/*/<uuid>.jsonl + <uuid>/ dir) untouched 14+ days that no Clauracle project resumes.
fn claude_clean(home: &Path, keep: &[String], low: bool) -> u64 {
    let mut freed = 0;
    let mut vs: Vec<_> = std::fs::read_dir(home.join(".local/share/claude/versions")).into_iter().flatten().flatten()
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path()))).collect();
    vs.sort();
    vs.pop(); // newest stays (a running older one fails to delete on Windows: fine)
    for (_, p) in vs { let n = p.metadata().map_or(0, |m| m.len()); if std::fs::remove_file(&p).is_ok() { freed += n; } }
    if !low { return freed; }
    let old = |p: &Path| p.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|e| e.as_secs() > 14 * 86400);
    for proj in std::fs::read_dir(home.join(".claude/projects")).into_iter().flatten().flatten() {
        for f in std::fs::read_dir(proj.path()).into_iter().flatten().flatten().map(|e| e.path()) {
            let Some(id) = f.file_stem().map(|s| s.to_string_lossy().to_string()) else { continue };
            if f.extension().is_none_or(|x| x != "jsonl") || keep.contains(&id) || !old(&f) { continue; }
            let n = f.metadata().map_or(0, |m| m.len());
            if std::fs::remove_file(&f).is_ok() { freed += n; }
            let sub = proj.path().join(&id); // subagent transcripts / tool results of that session
            if sub.is_dir() { freed += dir_size(&sub); let _ = std::fs::remove_dir_all(sub); }
        }
    }
    freed
}

/// GitHub release `tag` with `files` attached and notes generated from the commits since the last one.
pub fn release(dir: &str, user: &str, tag: &str, files: &[String]) -> Result<String, String> {
    if tag.is_empty() || tag.starts_with('-') { return Err("bad tag".into()); }
    let mut a = vec!["release", "create", tag, "--title", tag, "--generate-notes"];
    a.extend(files.iter().map(String::as_str));
    let o = run("gh", &a, Some(dir), &gh_env(user));
    if o.ok { Ok(o.stdout.trim().to_string()) } else { Err(o.stderr.trim().to_string()) }
}

// ---- Clauracle's issues: label `clauracle` (= ours, ≠ hand-filed repo issues) + a type label (task/feature/fix/
// suggestion); taken = assignee + `in progress` (no commits, all live on GitHub) ----

pub const OUR_LABEL: &str = "clauracle";
pub const TASK_LABEL: &str = "task";
pub const DOING_LABEL: &str = "in progress";
pub const IDEA_LABEL: &str = "suggestion"; // Claude's own idea awaiting the user's yes (Issues list Accept/Dismiss)

pub fn tasks(dir: &str, user: &str) -> serde_json::Value {
    let list = |l: &str| gh_json(&["issue", "list", "--label", l, "--state", "open", "--limit", "100",
              "--json", "number,title,url,assignees,labels,updatedAt,author"], user, Some(dir));
    let v = trusted(list(OUR_LABEL), user, || {
        // ∵ issue templates can auto-label outsiders' issues → only push-access authors queue work for the AI
        gh_json(&["api", "repos/{owner}/{repo}/collaborators", "--jq", "[.[] | select(.permissions.push) | .login]"], user, Some(dir))
            .as_array().into_iter().flatten().filter_map(|l| l.as_str().map(str::to_lowercase)).collect()
    });
    // ponytail: migration (2026-11-05): none ours yet → tag open `task` issues `clauracle` once; drop later
    if v.as_array().is_some_and(|a| a.is_empty()) {
        let old = list(TASK_LABEL);
        let ns: Vec<String> = old.as_array().into_iter().flatten().filter_map(|x| x["number"].as_u64()).map(|n| n.to_string()).collect();
        if !ns.is_empty() {
            let env = gh_env(user);
            run("gh", &["label", "create", OUR_LABEL, "--color", "D77655", "--description", "Tracked in Clauracle"], Some(dir), &env);
            for n in &ns { run("gh", &["issue", "edit", n, "--add-label", OUR_LABEL], Some(dir), &env); }
            return list(OUR_LABEL);
        }
    }
    v
}

/// Keep issues by `me` or a push collaborator. Collaborators fetched only if a stranger's issue shows up (solo repo = 0
/// extra calls); fetch fails → only mine. ponytail: per-poll fetch then; cache if foreign issues get common.
fn trusted(v: serde_json::Value, me: &str, collab: impl FnOnce() -> Vec<String>) -> serde_json::Value {
    let me = me.to_lowercase();
    let who = |x: &serde_json::Value| x["author"]["login"].as_str().unwrap_or("").to_lowercase();
    let Some(a) = v.as_array() else { return v };
    if a.iter().all(|x| who(x) == me) { return v; }
    let ok = collab();
    serde_json::Value::Array(a.iter().filter(|x| { let w = who(x); w == me || ok.contains(&w) }).cloned().collect())
}

#[test]
fn trusted_works() {
    let v = serde_json::json!([{"number": 1, "author": {"login": "Me"}}, {"number": 2, "author": {"login": "bob"}}, {"number": 3, "author": {"login": "eve"}}]);
    let n = |v: serde_json::Value| v.as_array().unwrap().iter().map(|x| x["number"].as_u64().unwrap()).collect::<Vec<_>>();
    assert_eq!(n(trusted(v.clone(), "me", || vec!["bob".into()])), [1, 2]);
    assert_eq!(n(trusted(v.clone(), "me", Vec::new)), [1]);
    assert_eq!(n(trusted(serde_json::json!([{"number": 1, "author": {"login": "me"}}]), "me", || panic!("no fetch"))), [1]);
}

/// gh issue actions: new (title) | claim | release | done (number).
pub fn task_do(dir: &str, user: &str, action: &str, arg: &str) -> Result<String, String> { // → gh stdout ("new": issue URL)
    let env = gh_env(user);
    let gh = |a: &[&str]| { let o = run("gh", a, Some(dir), &env); if o.ok { Ok(o.stdout.trim().to_string()) } else { Err(o.stderr.trim().to_string()) } };
    match action {
        "new" => {
            let _ = gh(&["label", "create", OUR_LABEL, "--color", "D77655", "--description", "Tracked in Clauracle"]);
            let (title, body) = split_task(arg);
            let mut a = vec!["issue", "create", "--title", &title, "--body", &body, "--label", OUR_LABEL, "--label", TASK_LABEL];
            let tags = hash_tags(arg); // #71: written #Tags → labels (made if missing; color = gh default)
            for t in &tags { let _ = gh(&["label", "create", t]); a.extend(["--label", t.as_str()]); }
            gh(&a)
        }
        "claim" => {
            let _ = gh(&["label", "create", DOING_LABEL, "--color", "4F5BD5", "--description", "Someone is on it"]);
            gh(&["issue", "edit", arg, "--add-assignee", "@me", "--add-label", DOING_LABEL])
        }
        "release" => gh(&["issue", "edit", arg, "--remove-assignee", "@me", "--remove-label", DOING_LABEL]),
        "done" => gh(&["issue", "close", arg]),
        "accept" => gh(&["issue", "edit", arg, "--remove-label", IDEA_LABEL]), // suggestion → queued task
        "dismiss" => gh(&["issue", "close", arg, "--reason", "not planned"]),
        _ => Err("unknown task action".into()),
    }
}

/// Task text → (issue title, body). Short → title only; long → title = lead up to the first ": " / ". " (≤100 chars,
/// else cut at a word + "…"), body = full text ∵ issues came truncated with an empty body.
pub fn split_task(t: &str) -> (String, String) {
    let t = t.trim();
    if let Some((h, _)) = t.split_once('\n') { // multi-line: line 1 = title (#93 card "Title: summary\n\nNote: …")
        if !h.trim().is_empty() && h.chars().count() <= 100 { return (h.trim().to_string(), t.to_string()); }
    }
    if t.chars().count() <= 100 { return (t.to_string(), String::new()); }
    let lead = [": ", ". ", " (", " — "].iter().filter_map(|s| t.find(s)).filter(|&i| i >= 8 && t[..i].chars().count() <= 100).min();
    let title = match lead {
        Some(i) => t[..i].to_string(),
        None => { let c: String = t.chars().take(97).collect(); format!("{}…", c.rsplit_once(' ').map_or(c.as_str(), |(a, _)| a)) }
    };
    (title, t.to_string())
}

/// Issue-mode queue: open `task` issues (from `tasks()`) minus suggestions and ones someone else took, in the
/// project's `order` (unlisted → after, by number). Same rule as the UI's issueQ().
pub fn task_queue(list: &serde_json::Value, me: &str, order: &[u64]) -> Vec<serde_json::Value> {
    let me = me.to_lowercase();
    let mut q: Vec<serde_json::Value> = list.as_array().into_iter().flatten().filter(|x| {
        let names = |k: &str, f: &str| x[k].as_array().into_iter().flatten().filter_map(|l| l[f].as_str().map(str::to_lowercase)).collect::<Vec<_>>();
        let who = names("assignees", "login");
        !names("labels", "name").iter().any(|l| l == IDEA_LABEL) && (who.is_empty() || who.contains(&me))
    }).cloned().collect();
    let n = |x: &serde_json::Value| x["number"].as_u64().unwrap_or(0);
    q.sort_by_key(|x| (order.iter().position(|&o| o == n(x)).unwrap_or(usize::MAX), n(x)));
    q
}

/// `#Critical` → "critical": # after start/space, then a letter (∴ #62 refs, C#, `# heading` skipped); deduped.
pub fn hash_tags(s: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for w in s.split_whitespace().filter_map(|w| w.strip_prefix('#')) {
        let t: String = w.chars().take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect::<String>().to_lowercase();
        if t.starts_with(|c: char| c.is_alphabetic()) && !v.contains(&t) { v.push(t); }
    }
    v
}

#[test]
fn hash_tags_works() {
    assert_eq!(hash_tags("#Critical fix #62, C# and #bug-fix. # head #critical"), ["critical", "bug-fix"]);
}

#[test]
fn task_queue_works() {
    let v = serde_json::json!([
        {"number": 3, "title": "c", "labels": [], "assignees": []},
        {"number": 1, "title": "a", "labels": [{"name": "suggestion"}], "assignees": []},
        {"number": 2, "title": "b", "labels": [], "assignees": [{"login": "Bob"}]},
        {"number": 4, "title": "d", "labels": [], "assignees": [{"login": "Me"}]},
        {"number": 5, "title": "e", "labels": [], "assignees": []}]);
    let nums = |q: Vec<serde_json::Value>| q.iter().map(|x| x["number"].as_u64().unwrap()).collect::<Vec<_>>();
    assert_eq!(nums(task_queue(&v, "me", &[5])), [5, 3, 4]);
    assert_eq!(nums(task_queue(&v, "me", &[])), [3, 4, 5]);
    assert!(task_queue(&serde_json::Value::Null, "me", &[]).is_empty());
}

#[test]
fn split_task_works() {
    assert_eq!(split_task("Fix the bug"), ("Fix the bug".into(), String::new()));
    assert_eq!(split_task("Fix it\n\nNote: soon"), ("Fix it".into(), "Fix it\n\nNote: soon".into()));
    let long = format!("Tasks from GitHub Issues by default: {}", "x ".repeat(60));
    assert_eq!(split_task(&long), ("Tasks from GitHub Issues by default".into(), long.trim().into()));
    let (t, b) = split_task(&"word ".repeat(40));
    assert!(t.ends_with('…') && t.chars().count() <= 100 && b.len() > 100);
}

/// Files my work touches: uncommitted changes + commits on this branch since it left the default branch.
pub fn my_files(dir: &str) -> Vec<String> {
    let mut files: Vec<String> = run("git", &["status", "--porcelain"], Some(dir), &[]).stdout.lines()
        .filter_map(|l| l.get(3..)).map(|f| f.rsplit(" -> ").next().unwrap_or(f).trim_matches('"').to_string()).collect();
    for base in ["origin/HEAD", "origin/main", "origin/master"] {
        let o = run("git", &["diff", "--name-only", &format!("{base}...HEAD")], Some(dir), &[]);
        if o.ok {
            files.extend(o.stdout.lines().map(String::from));
            break;
        }
    }
    files.sort();
    files.dedup();
    files
}

/// Someone else's open PRs that change the same files as my work: [{number, title, url, author, files}].
pub fn overlap(mine: &[String], prs: &serde_json::Value, me: &str, branch: &str) -> Vec<serde_json::Value> {
    prs.as_array().into_iter().flatten().filter(|pr| {
        !pr["author"]["login"].as_str().unwrap_or("").eq_ignore_ascii_case(me) && pr["headRefName"] != branch
    }).filter_map(|pr| {
        let both: Vec<&str> = pr["files"].as_array().into_iter().flatten().filter_map(|f| f["path"].as_str())
            .filter(|f| mine.iter().any(|m| m == f)).collect();
        (!both.is_empty()).then(|| serde_json::json!({"number": pr["number"], "title": pr["title"], "url": pr["url"],
                                                       "author": pr["author"]["login"], "files": both}))
    }).collect()
}

pub fn conflicts(dir: &str, user: &str) -> Vec<serde_json::Value> {
    let mine = my_files(dir);
    if mine.is_empty() {
        return vec![];
    }
    let branch = run("git", &["branch", "--show-current"], Some(dir), &[]).stdout.trim().to_string();
    let prs = gh_json(&["pr", "list", "--state", "open", "--limit", "50", "--json", "number,title,url,author,files,headRefName"], user, Some(dir));
    overlap(&mine, &prs, user, &branch)
}

pub fn git_ignored(dir: &str, file: &str) -> Option<bool> {
    if !Path::new(dir).join(".git").exists() { return None; }
    Some(run("git", &["check-ignore", "-q", file], Some(dir), &[]).ok)
}

/// Installed Chromium browsers and their profiles (from each one's "Local State").
/// [{name, exe, profiles: [{dir, name}]}]
pub fn browsers() -> Vec<serde_json::Value> {
    let h = dirs::home_dir().unwrap_or_default();
    let local = dirs::data_local_dir().unwrap_or_default();
    let pf = |v: &str| PathBuf::from(std::env::var(v).unwrap_or_default());
    // (name, exe candidates, user data dir)
    let list: Vec<(&str, Vec<PathBuf>, PathBuf)> = if cfg!(windows) {
        vec![("Chrome", vec![pf("ProgramFiles").join("Google/Chrome/Application/chrome.exe"), local.join("Google/Chrome/Application/chrome.exe")], local.join("Google/Chrome/User Data")),
             ("Edge", vec![pf("ProgramFiles(x86)").join("Microsoft/Edge/Application/msedge.exe"), pf("ProgramFiles").join("Microsoft/Edge/Application/msedge.exe")], local.join("Microsoft/Edge/User Data")),
             ("Brave", vec![pf("ProgramFiles").join("BraveSoftware/Brave-Browser/Application/brave.exe"), local.join("BraveSoftware/Brave-Browser/Application/brave.exe")], local.join("BraveSoftware/Brave-Browser/User Data"))]
    } else if cfg!(target_os = "macos") {
        let sup = h.join("Library/Application Support");
        vec![("Chrome", vec!["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".into()], sup.join("Google/Chrome")),
             ("Edge", vec!["/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge".into()], sup.join("Microsoft Edge")),
             ("Brave", vec!["/Applications/Brave Browser.app/Contents/MacOS/Brave Browser".into()], sup.join("BraveSoftware/Brave-Browser"))]
    } else {
        vec![("Chrome", vec!["/usr/bin/google-chrome".into(), "/usr/bin/google-chrome-stable".into()], h.join(".config/google-chrome")),
             ("Chromium", vec!["/usr/bin/chromium".into(), "/usr/bin/chromium-browser".into()], h.join(".config/chromium")),
             ("Edge", vec!["/usr/bin/microsoft-edge".into()], h.join(".config/microsoft-edge")),
             ("Brave", vec!["/usr/bin/brave-browser".into()], h.join(".config/BraveSoftware/Brave-Browser"))]
    };
    list.into_iter().filter_map(|(name, exes, data)| {
        let exe = exes.into_iter().find(|e| e.is_file())?;
        let state: serde_json::Value = std::fs::read_to_string(data.join("Local State")).ok()
            .and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let profiles: Vec<serde_json::Value> = state["profile"]["info_cache"].as_object().into_iter().flatten()
            .map(|(dir, i)| serde_json::json!({"dir": dir, "name": i["name"].as_str().unwrap_or(dir),
                                               "user": i["user_name"].as_str().unwrap_or("")})).collect();
        Some(serde_json::json!({"name": name, "exe": exe.to_string_lossy(), "data": data.to_string_lossy(), "profiles": profiles}))
    }).collect()
}

/// Phone push through ntfy.sh (free, no account): the topic is the only secret.
pub fn push_phone(topic: &str, title: &str, body: &str) {
    if topic.is_empty() {
        return;
    }
    let _ = ureq::post(&format!("https://ntfy.sh/{topic}")).set("Title", title).send_string(body);
}

#[cfg(test)]
mod tests {
    /// #77: temp (C:) → target/ (D: here) = cross-drive copy path; same drive elsewhere = rename path. Read-only file ≈ git objects.
    #[test]
    fn move_dir_moves_across_drives() {
        let (a, b) = (std::env::temp_dir().join("clauracle-move-src"), std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/clauracle-move-dst"));
        let _ = std::fs::remove_dir_all(&a); let _ = std::fs::remove_dir_all(&b);
        std::fs::create_dir_all(a.join(".git/objects/ab")).unwrap();
        let ro = a.join(".git/objects/ab/cd");
        std::fs::write(&ro, "obj").unwrap();
        let mut perm = std::fs::metadata(&ro).unwrap().permissions(); perm.set_readonly(true); std::fs::set_permissions(&ro, perm).unwrap();
        std::fs::write(a.join("main.rs"), "fn main(){}").unwrap();
        super::move_dir(&a, &b.join("p")).unwrap();
        assert!(!a.exists(), "source left behind");
        assert_eq!(std::fs::read_to_string(b.join("p/main.rs")).unwrap(), "fn main(){}");
        assert!(b.join("p/.git/objects/ab/cd").is_file());
        assert!(super::move_dir(&b.join("p"), &b.join("p/inner")).is_err());
        let mut perm = std::fs::metadata(b.join("p/.git/objects/ab/cd")).unwrap().permissions(); #[allow(clippy::permissions_set_readonly_false)] perm.set_readonly(false);
        let _ = std::fs::set_permissions(b.join("p/.git/objects/ab/cd"), perm); let _ = std::fs::remove_dir_all(&b);
    }
    /// #79: janitor beyond versions: idle project's ignored cache cleaned, busy one untouched; transcripts only when low + not kept + old.
    #[test]
    fn janitor_cleans_idle_and_old_transcripts() {
        let d = std::env::temp_dir().join("clauracle-janitor-test");
        let _ = std::fs::remove_dir_all(&d);
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(20 * 86400);
        let age = |p: &std::path::Path| std::fs::File::options().write(true).open(p).unwrap().set_modified(old).unwrap();
        let mk = |n: &str| { let p = d.join(n); std::fs::create_dir_all(p.join("target")).unwrap();
            std::fs::write(p.join("target/f"), "12345").unwrap(); std::fs::write(p.join(".gitignore"), "target/\n").unwrap();
            let s = p.to_string_lossy().to_string();
            assert!(super::run("git", &["init", "-q"], Some(&s), &[]).ok && super::run("git", &["add", ".gitignore"], Some(&s), &[]).ok);
            age(&p.join(".git/index")); s };
        let (idle, busy) = (mk("idle"), mk("busy"));
        super::janitor(&[(idle.clone(), false), (busy.clone(), true)], &[]);
        assert!(!d.join("idle/target").exists() && d.join("busy/target/f").exists());
        let h = d.join("home"); let pj = h.join(".claude/projects/x");
        std::fs::create_dir_all(pj.join("gone")).unwrap(); std::fs::write(pj.join("gone/sub"), "s").unwrap();
        for f in ["gone", "kept", "fresh"] { std::fs::write(pj.join(format!("{f}.jsonl")), "{}").unwrap(); }
        age(&pj.join("gone.jsonl")); age(&pj.join("kept.jsonl"));
        assert_eq!(super::claude_clean(&h, &[], false), 0); // drive fine → transcripts stay
        assert_eq!(super::claude_clean(&h, &["kept".into()], true), 3); // gone.jsonl (2) + gone/sub (1)
        assert!(!pj.join("gone").exists() && pj.join("kept.jsonl").exists() && pj.join("fresh.jsonl").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
    #[test]
    fn is_dep_spots_sdks_not_projects() {
        let d = std::env::temp_dir().join("clauracle-dep-test");
        let _ = std::fs::remove_dir_all(&d);
        let mk = |p: &str| { let f = d.join(p); std::fs::create_dir_all(f.parent().unwrap()).unwrap(); if p.ends_with('/') { std::fs::create_dir_all(&f).unwrap() } else { std::fs::write(&f, "").unwrap() } };
        mk("ghidra2/ghidra_12.1.4_PUBLIC/"); mk("nug/nuget.exe"); mk("app/package.json"); mk("app/bin/x.exe"); mk("jdk/x/");
        assert!(super::is_dep(&d.join("ghidra2")) && super::is_dep(&d.join("nug")) && super::is_dep(&d.join("jdk")));
        assert!(!super::is_dep(&d.join("app")));
        let _ = std::fs::remove_dir_all(&d);
    }
    #[test]
    fn guess_build_picks_most_specific() {
        let d = std::env::temp_dir().join("clauracle-guess-test");
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("package.json"), r#"{"scripts":{"build":"x","start":"y","deploy":"z"}}"#).unwrap();
        std::fs::write(d.join("yarn.lock"), "").unwrap();
        std::fs::write(d.join("Cargo.toml"), "").unwrap();
        let b = super::guess_build(&d.to_string_lossy());
        assert_eq!((b.build.as_str(), b.run.as_str(), b.deploy.as_str()), ("yarn run build", "yarn run start", "yarn run deploy"));
        std::fs::remove_file(d.join("package.json")).unwrap();
        std::fs::write(d.join("Makefile"), "run:\n\tx\ndeploy: a\n").unwrap();
        let b = super::guess_build(&d.to_string_lossy()); // cargo wins build/run, make fills deploy
        assert_eq!((b.build.as_str(), b.run.as_str(), b.deploy.as_str()), ("cargo build --release", "cargo run", "make deploy"));
        let _ = std::fs::remove_dir_all(&d);
    }
    #[test]
    fn clean_only_touches_ignored_caches() {
        let d = std::env::temp_dir().join("clauracle-clean-test");
        let _ = std::fs::remove_dir_all(&d);
        for sub in ["target/x", "dist"] { std::fs::create_dir_all(d.join(sub)).unwrap(); }
        std::fs::write(d.join("target/x/f"), "12345").unwrap();
        std::fs::write(d.join("dist/keep"), "1").unwrap();
        std::fs::write(d.join(".gitignore"), "target/\n").unwrap();
        let s = d.to_string_lossy().to_string();
        assert!(super::run("git", &["init", "-q"], Some(&s), &[]).ok);
        assert_eq!(super::caches(&s), vec![("target".to_string(), 5)]); // dist exists but isn't ignored
        assert_eq!(super::clean(&s, &[]), 5);
        assert!(!d.join("target").exists() && d.join("dist/keep").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
    #[test]
    fn scan_and_move_projects() {
        let r = std::env::temp_dir().join("clauracle-scan-test");
        let _ = std::fs::remove_dir_all(&r);
        for d in ["McKack/app/.git", "loose", "local/old", "deep/a/b/.git", ".hidden"] { std::fs::create_dir_all(r.join(d)).unwrap(); }
        let mut got: Vec<String> = super::scan_projects(&r).iter().map(|p| p.strip_prefix(&r).unwrap().to_string_lossy().replace('\\', "/")).collect();
        got.sort();
        assert_eq!(got, ["McKack/app", "deep/a/b", "local/old", "loose"]); // owner/deep folders = containers, not projects
        assert!(super::same_path(&super::place_of(&r, &r.join("loose").to_string_lossy()), &r.join("local/loose")));
        super::move_dir(&r.join("loose"), &r.join("local/loose")).unwrap();
        assert!(r.join("local/loose").is_dir() && !r.join("loose").exists());
        assert!(super::move_dir(&r.join("local"), &r.join("local/x")).is_err()); // into itself
        assert!(super::claude_dir_of(std::path::Path::new("D:\\Dev\\a-b")).ends_with("D--Dev-a-b"));
        let _ = std::fs::remove_dir_all(&r);
    }
    #[test]
    fn merge_settings_layers() {
        let mut a = serde_json::json!({"permissions": {"allow": ["A"], "defaultMode": "plan"}, "env": {"X": "1"}, "model": "a"});
        super::merge_settings(&mut a, &serde_json::json!({"permissions": {"allow": ["B"]}, "env": {"Y": "2"}, "model": "b"}));
        assert_eq!(a, serde_json::json!({"permissions": {"allow": ["A", "B"], "defaultMode": "plan"}, "env": {"X": "1", "Y": "2"}, "model": "b"}));
        assert!(super::account_settings("../x").is_none() && super::account_settings("Main").is_some());
    }
    #[test]
    fn disk_free_reads_the_drive() {
        let (f, t) = super::disk_free(&std::env::temp_dir().to_string_lossy()).expect("free space");
        assert!(f > 0 && f <= t);
    }
    #[test]
    fn default_nicks_count_up() {
        let mut cfg = Config::default();
        assert_eq!(super::default_nick(&cfg, "claude"), "Claude 1");
        cfg.accounts.push(Account { id: "a".into(), nick: "Claude 1".into(), ..Default::default() });
        cfg.accounts.push(Account { id: "b".into(), nick: "Claude 3".into(), ..Default::default() });
        assert_eq!(super::default_nick(&cfg, "claude"), "Claude 2");
    }

    use super::*;

    #[test]
    fn project_keeps_its_github_and_choices() {
        let mut cfg = Config::default();
        cfg.accounts.push(Account { id: MAIN.into(), gh: "octo".into(), on: Some(vec!["mcp:x".into()]), ..Default::default() });
        cfg.default_account = MAIN.into();
        let p = new_project(&mut cfg, "/x/My App", None, None);
        assert_eq!((p.name.as_str(), p.gh.as_str(), p.on.clone()), ("My App", "octo", Some(vec!["mcp:x".to_string()])));
        cfg.accounts[0].on = None; // an account that never picked: its new projects start with nothing
        assert_eq!(new_project(&mut cfg, "/y", None, None).on, Some(vec![]));
        cfg.accounts[0].gh = "other".into();
        assert_eq!(cfg.projects[0].gh, "octo");
        assert_eq!(new_project(&mut cfg, "y", None, Some("cloner")).gh, "cloner");
    }

    #[test]
    fn limits_and_pools() {
        let mut cfg = Config::default();
        for (id, pool) in [("A", "team"), ("B", "team"), ("C", "team"), ("D", "")] {
            cfg.accounts.push(Account { id: id.into(), pool: pool.into(), ..Default::default() });
        }
        let soon = now() + 3600.0;
        let ev = |u: f64| serde_json::json!({"status": "allowed", "unifiedWindows": {
            "five_hour": {"utilization": u, "resetsAt": soon}, "seven_day": {"utilization": 0.1, "resetsAt": soon}}});
        record_usage(&mut cfg, "A", &ev(0.95));
        record_usage(&mut cfg, "B", &ev(0.50));
        record_usage(&mut cfg, "C", &ev(0.20));
        assert_eq!(over_limit(&cfg, "A").as_deref(), Some("five_hour"));
        assert_eq!(over_limit(&cfg, "B"), None);
        assert_eq!(pool_pick(&cfg, "A").as_deref(), Some("C"), "the one with the most left");
        assert_eq!(pool_pick(&cfg, "D"), None, "no pool, no switch");
        record_usage(&mut cfg, "C", &serde_json::json!({"status": "rejected", "rateLimitType": "seven_day", "resetsAt": soon}));
        assert_eq!(pool_pick(&cfg, "A").as_deref(), Some("B"));
        cfg.usage.get_mut("A").unwrap().get_mut("five_hour").unwrap().resets_at = Some(now() - 1.0);
        assert_eq!(over_limit(&cfg, "A"), None, "a reset window no longer counts");
    }

    #[test]
    fn restrictions_from_off_lists() {
        let mut cfg = Config::default();
        cfg.catalog = ["skill:a", "skill:p:b", "mcp:rea"].iter()
            .map(|i| Item { id: i.to_string(), kind: if i.starts_with("mcp") { "mcp" } else { "skill" }.into(), ..Default::default() }).collect();
        let mut p = Project::default();
        p.hide_env = false;
        assert_eq!(restrictions(&cfg, &p), (vec![], None), "nothing off: no filter at all");
        cfg.off = vec!["mcp:rea".into()];
        p.off = vec!["skill:a".into()];
        assert_eq!(restrictions(&cfg, &p), (vec!["mcp__rea".to_string()], Some(vec!["p:b".to_string()])));
    }

    #[test]
    fn picked_items_only() {
        let mut cfg = Config::default();
        cfg.catalog = ["skill:a", "skill:clauracle:ponytail", "mcp:rea", "mcp:gh"].iter()
            .map(|i| Item { id: i.to_string(), kind: if i.starts_with("mcp") { "mcp" } else { "skill" }.into(), ..Default::default() }).collect();
        let mut p = Project { hide_env: false, on: Some(vec![]), ..Default::default() };
        // nothing picked: every server denied, only Clauracle's own skills
        assert_eq!(restrictions(&cfg, &p), (vec!["mcp__gh".to_string(), "mcp__rea".to_string()], Some(vec!["clauracle:ponytail".to_string()])));
        p.on = Some(vec!["mcp:gh".into(), "skill:a".into()]);
        cfg.off = vec!["skill:a".into()]; // switched off globally wins over a pick
        assert_eq!(restrictions(&cfg, &p), (vec!["mcp__rea".to_string()], Some(vec!["clauracle:ponytail".to_string()])));
    }

    #[test]
    fn cli_settings_load_only_picks() {
        let mut cfg = Config::default();
        cfg.catalog = [("mcp:claude_ai_Gmail", "claude.ai Gmail", "mcp"), ("mcp:rea", "rea", "mcp"), ("plugin:unreal@x", "unreal", "plugin"),
            ("plugin:room@y", "room", "plugin"), ("plugin:cc@inline", "cc", "plugin"), ("skill:room:share", "room:share", "skill")].iter()
            .map(|(i, l, k)| Item { id: i.to_string(), label: l.to_string(), kind: k.to_string(), ..Default::default() }).collect();
        let p = Project { on: Some(vec!["mcp:rea".into(), "plugin:cc@inline".into(), "skill:room:share".into()]), ..Default::default() };
        let s = cli_settings(&cfg, &p);
        assert_eq!(s["deniedMcpServers"], serde_json::json!([{"serverName": "claude.ai Gmail"}]), "claude.ai connectors by their listed name");
        assert_eq!(s["enabledPlugins"], serde_json::json!({"ponytail@ponytail": false, "unreal@x": false}),
                   "picked mod stays; a plugin with a picked skill stays; Clauracle's Ponytail replaces the plugin");
    }

    #[test]
    fn env_round_trip() {
        let text = "# keys\nexport A=1\nB=\"two words\"\n";
        assert_eq!(env_parse(text), vec![("A".to_string(), "1".to_string()), ("B".to_string(), "two words".to_string())]);
        let t = env_set(text, "B", Some("x"));
        assert_eq!(t, "# keys\nexport A=1\nB=x\n", "comments and other keys stay");
        let t = env_set(&t, "C", Some("has \"quote\"\nand a line"));
        assert_eq!(env_parse(&t)[2], ("C".to_string(), "has \"quote\"\nand a line".to_string()));
        assert_eq!(env_parse(&env_set(&t, "A", None)).len(), 2);
    }

    #[test]
    fn conflicting_prs() {
        let prs = serde_json::json!([
            {"number": 1, "author": {"login": "anna"}, "headRefName": "a", "files": [{"path": "src/auth.ts"}, {"path": "x"}]},
            {"number": 2, "author": {"login": "Me"}, "headRefName": "b", "files": [{"path": "src/auth.ts"}]},
            {"number": 3, "author": {"login": "bo"}, "headRefName": "mine", "files": [{"path": "src/auth.ts"}]},
            {"number": 4, "author": {"login": "cy"}, "headRefName": "c", "files": [{"path": "README.md"}]}]);
        let c = overlap(&["src/auth.ts".into(), "y".into()], &prs, "me", "mine");
        assert_eq!(c.len(), 1); // not mine, not my branch, not without shared files
        assert_eq!((c[0]["number"].as_i64(), c[0]["files"][0].as_str()), (Some(1), Some("src/auth.ts")));
    }

    #[test]
    fn who_runs_a_repo() {
        let mut cfg = Config::default();
        cfg.accounts.push(Account { id: "w".into(), nick: "Work".into(), gh: "acme-dev".into(), ..Default::default() });
        let users = vec!["me".to_string(), "acme-dev".to_string()];
        // owned by one of ours: that gh, and the AI account tied to it
        let w = suggest(&cfg, "ACME-dev", &users, |_| false);
        assert_eq!((w.gh.as_str(), w.account.as_str(), w.shared), ("acme-dev", "w", false));
        // someone else's: the gh with push access, shared
        let w = suggest(&cfg, "client", &users, |u| u == "acme-dev");
        assert_eq!((w.gh.as_str(), w.account.as_str(), w.shared), ("acme-dev", "w", true));
        // no push, no tie: default account
        let w = suggest(&cfg, "client", &users, |_| false);
        assert_eq!((w.gh.as_str(), w.account, w.shared), ("", cfg.default_account.clone(), true));
        assert_eq!(parse_remote("git@github.com:a-b/c.d.git\n"), Some(("a-b".into(), "c.d".into())));
        assert_eq!(parse_remote("https://github.com/a/b"), Some(("a".into(), "b".into())));
        assert_eq!(parse_remote("https://gitlab.com/a/b"), None);
    }

    #[test]
    fn parsing() {
        let (l, n) = mcp_line("claude.ai Gmail: https://gmailmcp.googleapis.com/mcp/v1 - ✔ Connected").unwrap();
        assert_eq!((server_id(&l).as_str(), n.as_str()), ("claude_ai_Gmail", "Connected"));
        assert!(mcp_line("Checking MCP server health…").is_none());
        let tok = format!("sk-ant-oat01-{}", "Ab_9-".repeat(18));
        let screen = format!("\x1b]8;;https://claude.com/cai/oauth/authorize?code=true&x=1\x07more\r\ntoken:\x1b[33m{tok}\x1b[39mStore");
        let urls = std::cell::RefCell::new(vec![]);
        // one byte at a time: the token arrives split across reads
        struct Slow<'a>(&'a [u8]);
        impl std::io::Read for Slow<'_> {
            fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
                if self.0.is_empty() { return Ok(0); }
                b[0] = self.0[0];
                self.0 = &self.0[1..];
                Ok(1)
            }
        }
        assert_eq!(read_setup_token(&mut Slow(screen.as_bytes()), |u| urls.borrow_mut().push(u)), Some(tok.clone()));
        assert_eq!(urls.borrow().as_slice(), ["https://claude.com/cai/oauth/authorize?code=true&x=1"]);
        assert!(token_ok(&tok) && !token_ok("nope"));
    }
}
