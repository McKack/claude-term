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

/// Build steps Clauracle runs itself (plain code, no tokens). after: "" | build | build_run | build_deploy
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
    pub team_auto: bool,     // every task from a prompt becomes a team task (GitHub issue), claimed + closed by Claude
}
impl Default for Project {
    fn default() -> Self {
        Project {
            id: String::new(), name: String::new(), path: String::new(), account: MAIN.into(), gh: String::new(),
            off: vec![], on: None, mode: "default".into(), model: String::new(), effort: String::new(), ponytail: true, cont: "manual".into(),
            context_limit: 50.0, session: None, prev_session: None, ready: false, questions: vec![], answers: String::new(),
            build: Build::default(), hide_env: true, terse: true,
            owner: String::new(), shared: false, keep: false, used: 0.0, working: false, gaccount: String::new(), gcp: String::new(), services: vec![], team_auto: false, allow: vec![], ai_credit: false,
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

pub fn account_env(id: &str) -> Vec<(String, String)> {
    if id == MAIN {
        return vec![];
    }
    keyring::Entry::new(KEYRING, id).ok().and_then(|e| e.get_password().ok())
        .map(|t| vec![("CLAUDE_CODE_OAUTH_TOKEN".into(), t)]).unwrap_or_default()
}

/// "Claude 2": the AI's name plus the next number not taken by an account already named that way.
pub fn default_nick(cfg: &Config, provider: &str) -> String {
    let ai = match provider { _ => "Claude" }; // ponytail: one provider; a match arm per new AI
    let n = (1..).find(|i| !cfg.accounts.iter().any(|a| a.nick.eq_ignore_ascii_case(&format!("{ai} {i}")))).unwrap();
    format!("{ai} {n}")
}

pub fn add_account(cfg: &mut Config, nick: &str, token: &str) -> Account {
    let nick = if nick.trim().is_empty() { default_nick(cfg, "claude") } else { nick.trim().to_string() };
    let nick = nick.as_str();
    let base: String = Regex::new(r"\W+").unwrap().replace_all(nick.trim(), "-").trim_matches('-').to_string();
    let base = if base.is_empty() { "account".to_string() } else { base };
    let id = (0..1000).map(|i| if i == 0 { base.clone() } else { format!("{base}-{i}") })
        .find(|c| !cfg.accounts.iter().any(|a| &a.id == c)).unwrap();
    set_token(&id, token);
    let a = Account { id, nick: nick.into(),
                      provider: "claude".into(), on: Some(vec![]), ..Default::default() };
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
                 "C:/Program Files (x86)/Google/Cloud SDK/google-cloud-sdk/bin".into(), "/opt/homebrew/bin".into(), "/usr/local/bin".into(), "/usr/bin".into()]);
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ".bat"] } else { &[""] };
    dirs.iter().flat_map(|d| exts.iter().map(move |e| d.join(format!("{name}{e}")))).find(|p| p.is_file())
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
pub fn restrictions(cfg: &Config, p: &Project) -> (Vec<String>, Option<Vec<String>>) {
    let has = |l: &[String], id: &str| l.iter().any(|x| x == id);
    let enabled = |id: &str| !has(&cfg.off, id) && (id.starts_with("skill:clauracle:") || match &p.on {
        Some(on) => has(on, id),
        None => !has(&p.off, id),
    });
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
const CACHE_DIRS: [&str; 12] = ["target", "src-tauri/target", "dist", "build", "out", ".next", ".nuxt", ".turbo", ".parcel-cache",
    ".gradle", "node_modules/.cache", ".pytest_cache"];
pub const IDLE_CLEAN_DAYS: u64 = 14; // ponytail: fixed; a setting if someone wants caches kept longer

fn dir_size(p: &Path) -> u64 {
    // symlink_metadata: links/junctions count as themselves, never followed
    std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| match e.path().symlink_metadata() {
        Ok(m) if m.is_dir() => dir_size(&e.path()),
        Ok(m) => m.len(),
        _ => 0,
    }).sum()
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

/// GitHub release `tag` with `files` attached and notes generated from the commits since the last one.
pub fn release(dir: &str, user: &str, tag: &str, files: &[String]) -> Result<String, String> {
    if tag.is_empty() || tag.starts_with('-') { return Err("bad tag".into()); }
    let mut a = vec!["release", "create", tag, "--title", tag, "--generate-notes"];
    a.extend(files.iter().map(String::as_str));
    let o = run("gh", &a, Some(dir), &gh_env(user));
    if o.ok { Ok(o.stdout.trim().to_string()) } else { Err(o.stderr.trim().to_string()) }
}

// ---- team tasks: GitHub issues labelled `clauracle`; taken = assignee + `in progress` (no commits, all live on GitHub) ----

pub const TASK_LABEL: &str = "clauracle";
pub const DOING_LABEL: &str = "in progress";

pub fn tasks(dir: &str, user: &str) -> serde_json::Value {
    gh_json(&["issue", "list", "--label", TASK_LABEL, "--state", "open", "--limit", "100",
              "--json", "number,title,url,assignees,labels,updatedAt"], user, Some(dir))
}

/// gh issue actions: new (title) | claim | release | done (number).
pub fn task_do(dir: &str, user: &str, action: &str, arg: &str) -> Result<(), String> {
    let env = gh_env(user);
    let gh = |a: &[&str]| { let o = run("gh", a, Some(dir), &env); if o.ok { Ok(()) } else { Err(o.stderr.trim().to_string()) } };
    match action {
        "new" => {
            let _ = gh(&["label", "create", TASK_LABEL, "--color", "D77655", "--description", "Task tracked in Clauracle"]);
            gh(&["issue", "create", "--title", arg, "--body", "", "--label", TASK_LABEL])
        }
        "claim" => {
            let _ = gh(&["label", "create", DOING_LABEL, "--color", "4F5BD5", "--description", "Someone is on it"]);
            gh(&["issue", "edit", arg, "--add-assignee", "@me", "--add-label", DOING_LABEL])
        }
        "release" => gh(&["issue", "edit", arg, "--remove-assignee", "@me", "--remove-label", DOING_LABEL]),
        "done" => gh(&["issue", "close", arg]),
        _ => Err("unknown task action".into()),
    }
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
        Some(serde_json::json!({"name": name, "exe": exe.to_string_lossy(), "profiles": profiles}))
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
