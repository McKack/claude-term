//! Persistence and CLI helpers: projects, AI accounts (tokens in the OS keychain), usage limits and relays,
//! GitHub through gh, the Connections & Skills catalog, and fetching `claude`/`gh` when they're missing.
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::io::Read;
use std::process::Command;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
mod mac;
pub use mac::*;
mod proc;
pub use proc::*;
mod cli;
pub use cli::*;
mod env;
pub use env::*;
mod build;
pub use build::*;
mod tasks;
pub use tasks::*;

pub const MAIN: &str = "Main"; // the account `claude` itself is logged in as (no stored token)
const KEYRING: &str = "clauracle";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Limits {
    pub five_hour: f64, // % of the window; at it the loop switches account (relay) or stops
    pub seven_day: f64,
}
impl Default for Limits {
    fn default() -> Self {
        Limits { five_hour: 90.0, seven_day: 90.0 }
    }
}

/// An AI account: the login, its limits and relay. Its gh/off are only the starting point for new projects.
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
    #[serde(alias = "pool")] // #383: was "pool"; old configs still load
    pub relay: String, // accounts in one relay take over from each other at their limits
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

/// Build steps Clauracle runs itself (plain code, no tokens). after (#182): "" | <build|smart>[_run][_deploy];
/// "smart_build" = smart, nothing after; legacy "smart" = smart_run
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Build {
    pub build: String,
    pub run: String,
    pub deploy: String,
    pub after: String, // after each finished task
    pub fix: bool,     // a failed step goes back to Claude to fix
    pub local: String, // #288 this-machine build (after tasks, /build); "" = fast debug twin of `build` (claude::fast_cmd)
    pub release: bool, // #288 true = local builds use the full `build` cmd
}
impl Default for Build { // #229 every new project (add/clone/new_local, partial JSON) = Smart build + Run; saved ones keep theirs
    fn default() -> Self { Build { build: String::new(), run: String::new(), deploy: String::new(), after: "smart_run".into(), fix: true, local: String::new(), release: false } }
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
    pub work: u64,            // issue being worked on (0 = none): persisted ∴ a clear/crash/restart resumes it first, never re-picks the queue head
    pub targets: Vec<String>, // target platforms: windows|macos|linux|ios|android; [] = not picked → UI guesses (ships_mac)
    pub wired: Vec<String>,   // #259 in-app services already handed to Claude to integrate (checked or tasked) → not counted in "Integrate N"
    pub license: String,      // #264 SPDX id picked in Settings › Projects ("" = never picked; release row says Choose/Confirm)
    pub site: serde_json::Value, // #40 website: {host, domain, pages: [landing|privacy|terms]}; UI-owned shape, secrets stay in .env
}
impl Default for Project {
    fn default() -> Self {
        Project {
            id: String::new(), name: String::new(), path: String::new(), account: MAIN.into(), gh: String::new(),
            off: vec![], on: None, mode: "default".into(), model: String::new(), effort: String::new(), ponytail: true, cont: "manual".into(),
            context_limit: 50.0, session: None, prev_session: None, ready: false, questions: vec![], answers: String::new(),
            build: Build::default(), hide_env: true,
            owner: String::new(), shared: false, keep: false, used: 0.0, working: false, gaccount: String::new(), gcp: String::new(), services: vec![], allow: vec![], ai_credit: false, stage: String::new(), rel_auto: false, order: vec![], work: 0, targets: vec![], wired: vec![], license: String::new(), site: serde_json::Value::Null,
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

/// #256 Accounts › You: nick → prompt name everywhere; tags/prefs → ~/.clauracle/you.md, read by the AI only when it matters (token use)
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct You {
    pub nick: String,
    pub tags: Vec<String>,
    pub prefs: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Notify {
    pub desktop: bool,
    pub ntfy: String, // ntfy.sh topic for phone pushes; "" = off
    pub snooze: u64,  // #402 unix secs: no toast popup/desktop/phone before this; ∞ = 9999999999 (JS-safe, ≠ u64::MAX), 0 = off
}

/// #147 Network Mac: builds macOS over SSH. Auth = ~/.ssh/config + agent/keys only (BatchMode) ∴ no password ever stored.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct NetMac {
    pub host: String, // ssh alias or IP; "" = off
    pub user: String, // "" = ssh config/default user
    pub dir: String,  // remote build root; "" = ~/clauracle-build
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
    pub face: String, // seed for its pixel face; "" = drawn from the name
}

pub fn default_agents() -> Vec<Agent> {
    let a = |name: &str, model: &str, description: &str, prompt: &str| Agent {
        name: name.into(), model: model.into(), description: description.into(), prompt: prompt.into(), on: true, face: String::new() };
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
    pub you: You,
    pub lessons: Vec<String>, // never-again rules (mcp lesson tool / Settings › Accounts), in every session's prompt
    pub win: Option<[i32; 5]>, // main window at last close/restart: x, y, w, h (physical px), mode 0 windowed|1 maximized|2 fullscreen
    pub notify: Notify,
    pub ci_builds: bool, // #126: GitHub Actions builds non-local targets; off (default) = local builds only ∵ CI minutes cost money
    pub mac: NetMac, // #147 Network Mac (SSH)
    pub update_every: Option<u32>, // #183 auto update check, hours: None = 24, 0 = never
    pub update_at: u64, // last update check (unix secs): start checks only when due
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
    // ∵ any read error used to = "first run" → next save wiped every project: only NotFound is a first run
    let parse = |p: &std::path::Path| std::fs::read(p).ok()
        .and_then(|b| serde_json::from_str::<Config>(String::from_utf8_lossy(&b).trim_start_matches('\u{feff}')).ok());
    let mut cfg: Config = match std::fs::metadata(&path) {
        Ok(_) => parse(&path).or_else(|| parse(&home().join("config.json.tmp"))).unwrap_or_else(|| {
            // unreadable (locked, bad bytes, bad JSON): keep it aside, timestamped ∴ a 2nd failure can't overwrite the 1st backup
            let _ = std::fs::copy(&path, home().join(format!("config.broken-{}.json", now() as u64)));
            Config::default()
        }),
        Err(_) => {
            let mut c = Config { notify: Notify { desktop: true, ntfy: String::new(), snooze: 0 }, ..Default::default() };
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
    let r = std::fs::File::create(&tmp).and_then(|mut f| { // sync ∵ power loss after rename could leave it empty
        std::io::Write::write_all(&mut f, serde_json::to_string_pretty(cfg).unwrap_or_default().as_bytes())?; f.sync_all() })
        .and_then(|_| std::fs::rename(&tmp, home().join("config.json")));
    if let Err(e) = r { eprintln!("config save failed: {e}"); }
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

// ---- usage, limits and relays ---------------------------------------------------------------

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
    if raw["unifiedWindows"].is_object() {
        entry.clear(); // full report: nothing older (e.g. another account's numbers) survives it
    }
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
    over_by(cfg, account, 0.0)
}

/// Same, with the limit raised by `extra` points (never past 100: a full window stays full).
fn over_by(cfg: &Config, account: &str, extra: f64) -> Option<String> {
    let a = account_or_main(cfg, account);
    let u = usage(cfg, account);
    [("five_hour", a.limits.five_hour), ("seven_day", a.limits.seven_day)].iter()
        .find(|(k, lim)| u.get(*k).copied().unwrap_or(0.0) >= (lim + extra).min(100.0)).map(|(k, _)| k.to_string())
}

/// What an auto run does when its account may be at a limit.
#[derive(Debug, PartialEq)]
pub enum Route {
    Stay,                 // under its limit: carry on
    Switch(String),       // another relay account has room
    Stretch(String, f64), // every account is at its set limit: the one with the most left goes `extra` points over it
    Pause(f64),           // nothing left anywhere: wait until this time (unix secs), the earliest reset in the relay
}

/// Limit hit → another account of the relay with room, else stretch (+5, then +10 points over the set limit, never
/// past 100) to keep going until a window resets, else pause until the earliest reset. No relay = the account alone.
pub fn relay_route(cfg: &Config, account: &str) -> Route {
    if over_limit(cfg, account).is_none() { return Route::Stay; }
    if let Some(n) = relay_pick(cfg, account) { return Route::Switch(n); }
    let me = account_or_main(cfg, account);
    let members: Vec<&Account> = if me.relay.is_empty() { vec![me] } else { cfg.accounts.iter().filter(|a| a.relay == me.relay).collect() };
    let most = |id: &str| usage(cfg, id).values().cloned().fold(0.0, f64::max);
    for extra in [5.0, 10.0] {
        if let Some(a) = members.iter().filter(|a| over_by(cfg, &a.id, extra).is_none())
            .min_by(|x, y| most(&x.id).partial_cmp(&most(&y.id)).unwrap()) {
            return Route::Stretch(a.id.clone(), extra);
        }
    }
    Route::Pause(relay_free_at(cfg, account))
}

/// When the relay (or a lone account) has room again: each account is free once its last blocking window resets,
/// the relay once its first account is. No reset time known → look again in an hour.
pub fn relay_free_at(cfg: &Config, account: &str) -> f64 {
    let me = account_or_main(cfg, account);
    let members: Vec<&Account> = if me.relay.is_empty() { vec![me] } else { cfg.accounts.iter().filter(|a| a.relay == me.relay).collect() };
    let t = now();
    let free_at = |a: &Account| cfg.usage.get(&a.id).map(|w| w.iter()
        .filter(|(k, v)| v.resets_at.is_some_and(|r| r > t) && v.utilization * 100.0 >= match k.as_str() { "five_hour" => a.limits.five_hour, "seven_day" => a.limits.seven_day, _ => 100.0 })
        .filter_map(|(_, v)| v.resets_at).fold(0.0, f64::max)).unwrap_or(0.0);
    let until = members.iter().map(|a| free_at(a)).filter(|r| *r > t).fold(f64::INFINITY, f64::min);
    if until.is_finite() { until } else { t + 3600.0 }
}

/// The CLI said a limit was hit (result text) but no rate-limit event marked the window full: mark it now, or the
/// loop sends the next turn into the same wall. Reset time unknown → kept if known, else checked again in an hour.
pub fn mark_limit_hit(cfg: &mut Config, account: &str, text: &str) {
    let w = if text.to_lowercase().contains("week") { "seven_day" } else { "five_hour" };
    let t = now();
    let reset = cfg.usage.get(account).and_then(|u| u.get(w)).and_then(|v| v.resets_at).filter(|r| *r > t).unwrap_or(t + 3600.0);
    cfg.usage.entry(account.into()).or_default().insert(w.into(), Window { utilization: 1.0, resets_at: Some(reset) });
}

/// Another account of the same relay that's under its limits, the one with the most left.
pub fn relay_pick(cfg: &Config, account: &str) -> Option<String> {
    let relay = &account_or_main(cfg, account).relay;
    if relay.is_empty() {
        return None;
    }
    cfg.accounts.iter()
        .filter(|a| &a.relay == relay && a.id != account && over_limit(cfg, &a.id).is_none())
        .map(|a| (a.id.clone(), usage(cfg, &a.id).values().cloned().fold(0.0, f64::max)))
        .min_by(|x, y| x.1.partial_cmp(&y.1).unwrap()).map(|x| x.0)
}

// ---- finding and running CLIs ----------------------------------------------------------------

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
    /// #166: deploy key → both .env keys, multi-line private key round-trips, rerun = same pubkey
    #[test]
    fn deploy_key_fills_env_once() {
        if super::which("ssh-keygen").is_none() { return; }
        let d = std::env::temp_dir().join(format!("clauracle-dk-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        let env = d.join(".env");
        std::fs::write(&env, "A=1\n").unwrap();
        let k = super::deploy_key(&env, "t").unwrap();
        assert!(k.starts_with("ssh-ed25519 "));
        let kv = super::env_parse(&std::fs::read_to_string(&env).unwrap());
        let pk = &kv.iter().find(|(x, _)| x == "SITE_SSH_PRIVATE_KEY").unwrap().1;
        assert!(pk.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----\n") && pk.ends_with("-----END OPENSSH PRIVATE KEY-----"));
        assert_eq!(super::deploy_key(&env, "t").unwrap(), k);
        let _ = std::fs::remove_dir_all(&d);
    }
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
    /// #221: churn = added + deleted over 30 days, commit list newest first, untouched file = 0
    #[test]
    fn file_heat_counts_churn() {
        let d = std::env::temp_dir().join(format!("clauracle-heat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d); std::fs::create_dir_all(&d).unwrap();
        let ds = d.to_str().unwrap();
        let git = |a: &[&str]| assert!(run("git", &[&["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"], a].concat(), Some(ds), &[]).ok, "git {a:?}");
        git(&["init", "-q"]);
        std::fs::write(d.join("a.txt"), "1\n2\n").unwrap(); std::fs::write(d.join("b.txt"), "x\n").unwrap();
        git(&["add", "."]); git(&["commit", "-qm", "first (#1)"]);
        std::fs::write(d.join("a.txt"), "1\n3\n").unwrap();
        git(&["commit", "-qam", "second (#2)"]);
        let v = file_heat(ds);
        let f = |n: &str| v["files"].as_array().unwrap().iter().find(|x| x[0] == n).unwrap().clone();
        assert_eq!(f("a.txt")[2], 4); // 2 added + (1 added, 1 deleted)
        assert_eq!(f("b.txt")[2], 1);
        assert_eq!(f("a.txt")[1], 4);
        let subj = |i: &serde_json::Value| v["commits"][i.as_u64().unwrap() as usize][2].clone();
        let a = f("a.txt"); let cs = a[3].as_array().unwrap();
        assert_eq!((subj(&cs[0]), subj(&cs[1])), (serde_json::json!("second (#2)"), serde_json::json!("first (#1)")));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// #131: pid sanitized (no shell injection via path chars), build cmd + copy-back present
    #[test]
    fn mac_script_env_only_and_quoted() {
        let m = NetMac { host: "h".into(), dir: "~/b; rm -rf /".into(), ..Default::default() };
        assert_eq!(mac_dir(&m, "p 1"), "~/brm-rf/p1");
        assert_eq!(mac_dir(&NetMac::default(), "x"), "clauracle-build/x");
        let s = mac_script(&m, "x", "npx tauri build", &[("APPLE_PASSWORD".into(), "a'b".into()), ("AZURE_X".into(), "z".into())]);
        assert!(s.contains("export APPLE_PASSWORD='a'\\''b'\n") && !s.contains("AZURE") && s.contains("npx tauri build\n") && s.contains("notarytool submit"));
    }

    /// live: CLAURACLE_MAC=mac-mini cargo test mac_roundtrip -- --ignored (sync → script w/ env → fake dmg pulled back)
    #[test]
    #[ignore]
    fn mac_roundtrip() {
        let m = NetMac { host: std::env::var("CLAURACLE_MAC").unwrap(), ..Default::default() };
        let d = std::env::temp_dir().join("clauracle-mac-rt");
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a.txt"), "hi").unwrap();
        std::fs::write(d.join(".env"), "SECRET=1").unwrap();
        let dir = d.to_string_lossy().to_string();
        mac_sync(&m, "rt", &dir).unwrap();
        let cmd = "test ! -e .env; test \"$APPLE_X\" = \"q'z\"; mkdir -p src-tauri/target/release/bundle/dmg; cat a.txt > src-tauri/target/release/bundle/dmg/T.dmg";
        let mut c = mac_ssh(&m, "zsh -l -s");
        c.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped());
        let mut ch = c.spawn().unwrap();
        { use std::io::Write; ch.stdin.take().unwrap().write_all(mac_script(&m, "rt", cmd, &[("APPLE_X".into(), "q'z".into())]).as_bytes()).unwrap(); }
        assert!(ch.wait_with_output().unwrap().status.success());
        mac_pull(&m, "rt", &dir).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("src-tauri/target/release/bundle/dmg/T.dmg")).unwrap(), "hi");
        let _ = mac_ssh(&m, "rm -rf clauracle-build/rt").output();
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn wsl_script_shape() {
        let s = wsl_script("a$b;c-1", "npx tauri build");
        assert!(s.contains(".clauracle-build/abc-1\"") && s.contains("; npx tauri build; ") && s.contains("cp --parents {} \"$s/\" \\;"));
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
        // temp folder: old loose file and old session go; kept session, fresh session and a session with one fresh file stay
        let t = d.join("tmp"); let h16 = t.join("0123456789abcdef");
        for s in ["old", "kept", "new", "mixed"] { std::fs::create_dir_all(h16.join(s)).unwrap(); std::fs::write(h16.join(s).join("f"), "12").unwrap(); }
        std::fs::write(h16.join("mixed/g"), "1").unwrap(); std::fs::write(t.join("loose"), "123").unwrap();
        for f in ["old/f", "kept/f", "mixed/f"] { age(&h16.join(f)); } age(&t.join("loose"));
        assert_eq!(super::tmp_clean(&t, &["kept".into()]), 5); // old/f (2) + loose (3)
        assert!(!h16.join("old").exists() && h16.join("kept").exists() && h16.join("new").exists() && h16.join("mixed").exists());
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
        assert_eq!(new_project(&mut cfg, "/z", None, None).build.after, "smart_run"); // #229
        assert_eq!(serde_json::from_str::<Project>(r#"{"build":{"after":""}}"#).unwrap().build.after, ""); // saved choice kept
        cfg.accounts[0].gh = "other".into();
        assert_eq!(cfg.projects[0].gh, "octo");
        assert_eq!(new_project(&mut cfg, "y", None, Some("cloner")).gh, "cloner");
    }

    #[test]
    fn limits_and_relays() {
        let mut cfg = Config::default();
        for (id, relay) in [("A", "team"), ("B", "team"), ("C", "team"), ("D", "")] {
            cfg.accounts.push(Account { id: id.into(), relay: relay.into(), ..Default::default() });
        }
        let soon = now() + 3600.0;
        let ev = |u: f64| serde_json::json!({"status": "allowed", "unifiedWindows": {
            "five_hour": {"utilization": u, "resetsAt": soon}, "seven_day": {"utilization": 0.1, "resetsAt": soon}}});
        record_usage(&mut cfg, "A", &ev(0.95));
        record_usage(&mut cfg, "B", &ev(0.50));
        record_usage(&mut cfg, "C", &ev(0.20));
        assert_eq!(over_limit(&cfg, "A").as_deref(), Some("five_hour"));
        assert_eq!(over_limit(&cfg, "B"), None);
        assert_eq!(relay_pick(&cfg, "A").as_deref(), Some("C"), "the one with the most left");
        assert_eq!(relay_pick(&cfg, "D"), None, "no relay, no switch");
        record_usage(&mut cfg, "C", &serde_json::json!({"status": "rejected", "rateLimitType": "seven_day", "resetsAt": soon}));
        assert_eq!(relay_pick(&cfg, "A").as_deref(), Some("B"));
        cfg.usage.get_mut("A").unwrap().get_mut("five_hour").unwrap().resets_at = Some(now() - 1.0);
        assert_eq!(serde_json::from_str::<Account>(r#"{"pool":"x"}"#).unwrap().relay, "x", "#383 old key still loads");
        assert_eq!(over_limit(&cfg, "A"), None, "a reset window no longer counts");
    }

    #[test]
    fn relay_routes_stretch_then_pause() {
        let mut cfg = Config::default();
        for id in ["A", "B"] { cfg.accounts.push(Account { id: id.into(), relay: "team".into(), ..Default::default() }); }
        let (soon, later) = (now() + 3600.0, now() + 7200.0);
        let ev = |f: f64, w: f64, r: f64| serde_json::json!({"status": "allowed", "unifiedWindows": {
            "five_hour": {"utilization": f, "resetsAt": r}, "seven_day": {"utilization": w, "resetsAt": later}}});
        record_usage(&mut cfg, "A", &ev(0.5, 0.1, soon));
        assert_eq!(relay_route(&cfg, "A"), Route::Stay);
        record_usage(&mut cfg, "A", &ev(1.0, 0.1, soon)); // A full, B over its set 90 but not 95
        record_usage(&mut cfg, "B", &ev(0.2, 0.92, soon));
        assert_eq!(relay_route(&cfg, "A"), Route::Stretch("B".into(), 5.0));
        record_usage(&mut cfg, "B", &ev(0.2, 0.97, soon));
        assert_eq!(relay_route(&cfg, "A"), Route::Stretch("B".into(), 10.0));
        record_usage(&mut cfg, "B", &ev(0.2, 1.0, soon));
        assert_eq!(relay_route(&cfg, "A"), Route::Pause(soon), "A's 5-hour window resets first");
        mark_limit_hit(&mut cfg, "B", "You've hit your weekly limit");
        assert_eq!(cfg.usage["B"]["seven_day"].resets_at, Some(later), "a known reset is kept");
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
        cfg.off.push("agent:Plan".into()); // #407
        assert_eq!(restrictions(&cfg, &p).0, ["mcp__rea", "Agent(Plan)", "Task(Plan)"].map(String::from).to_vec());
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
    fn sign_env_picks_signing_keys() {
        let d = std::env::temp_dir().join("clauracle-sign-env");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(".env"), "APPLE_ID=a@b.c\nAPPLE_CERTIFICATE=\nDB_URL=x\nAZURE_CLIENT_ID=1\n").unwrap();
        let k: Vec<String> = sign_env(&d.to_string_lossy()).into_iter().map(|(k, _)| k).collect();
        assert_eq!(k, ["APPLE_ID", "AZURE_CLIENT_ID"]);
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
        assert_eq!(server_id("claude.ai Gmail"), "claude_ai_Gmail");
        let mut cfg = Config::default();
        let init = serde_json::json!({"mcp_servers": [{"name": "claude.ai Gmail", "source": "claudeai"}, {"name": "clauracle", "source": "sdk"}, {"name": "cc", "source": "plugin"}]});
        assert!(mcp_seen(&mut cfg, &init) && !mcp_seen(&mut cfg, &init));
        assert_eq!(cfg.catalog.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["mcp:claude_ai_Gmail"]);
        assert_eq!(mcp_items(&[], &cfg.catalog).iter().filter(|i| i.label == "claude.ai Gmail").count(), 1); // connector kept across rescans
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
