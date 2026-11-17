//! The `delegate` tool (#610): one session hands a task to another account's AI (any provider, Claude CLI or OpenCode) and
//! gets the answer back as the tool result. A fresh one-shot session on the target, in the project folder; read-only unless
//! `write` is set. The caller's engine doesn't matter: both reach this through `control::mcp_any`.
use super::*;

const SECS: u64 = 600; // ponytail: a helper that outlives this is left running until it ends; the caller just stops waiting

/// Account named by the caller: its id or nick, case-insensitive.
fn find<'a>(cfg: &'a Config, name: &str) -> Option<&'a store::Account> {
    let n = name.trim().to_lowercase();
    cfg.accounts.iter().find(|a| a.id.to_lowercase() == n || a.nick.to_lowercase() == n)
}

pub(crate) async fn run(app: &AppHandle, st: &Arc<State>, pid: &str, a: &Value) -> String {
    let (name, task, write) = (a["account"].as_str().unwrap_or(""), a["task"].as_str().unwrap_or("").trim().to_string(), a["write"] == true);
    if task.is_empty() { return "Refused: `task` is empty.".into(); }
    let Some(p) = st.project(pid) else { return "project removed".into() };
    let acc = match find(&st.cfg.lock().unwrap(), name) {
        Some(a) => a.clone(),
        None => {
            let list: Vec<String> = st.cfg.lock().unwrap().accounts.iter().map(|a| a.nick.clone()).collect();
            return format!("No account named {name:?}. Accounts: {}.", list.join(", "));
        }
    };
    line(app, pid, "note", &format!("Delegated to {}: {}", acc.nick, task.chars().take(80).collect::<String>().replace('\n', " ")));
    let r = tokio::time::timeout(std::time::Duration::from_secs(SECS + 30), one_shot(app, &acc, &p.path, &p, &task, write)).await
        .unwrap_or_else(|_| Err("the helper took too long".into()));
    match r {
        Ok(t) => { line(app, pid, "note", &format!("{} answered ({} characters)", acc.nick, t.chars().count())); t }
        Err(e) => { line(app, pid, "note", &format!("{} could not do it: {e}", acc.nick)); format!("Delegation to {} failed: {e}", acc.nick) }
    }
}

async fn one_shot(app: &AppHandle, acc: &store::Account, dir: &str, p: &Project, task: &str, write: bool) -> Result<String, String> {
    if store::is_claude(acc) {
        let claude = store::which("claude").ok_or("Claude Code isn't installed")?;
        let (env, dir, task) = ([store::account_env(&acc.id, acc), store::gh_env(&p.gh)].concat(), dir.to_string(), task.to_string());
        return tokio::task::spawn_blocking(move || claude_once(&claude, env, &dir, &task, write)).await.map_err(|e| e.to_string())?;
    }
    store::which("opencode").ok_or("OpenCode isn't installed")?;
    let key = store::get_token(&acc.id).filter(|t| t != "none").unwrap_or_default();
    let local = if acc.provider == "local" {
        let (m, app2) = (acc.model.clone(), app.clone());
        Some(tokio::task::spawn_blocking(move || crate::local::ensure_noting(&app2, "", &m).map(|u| { let c = crate::local::ctx(&u); (u, c) }))
            .await.map_err(|e| e.to_string())??)
    } else { None };
    let (acc, dir, task) = (acc.clone(), dir.to_string(), task.to_string());
    tokio::task::spawn_blocking(move || opencode_once(&acc, &key, local, &dir, &task, write)).await.map_err(|e| e.to_string())?
}

fn claude_once(claude: &std::path::Path, env: Vec<(String, String)>, dir: &str, task: &str, write: bool) -> Result<String, String> {
    let mut c = store::command(claude);
    for k in ["CLAUDE_CODE_OAUTH_TOKEN", "ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL", "CLAUDECODE"] { c.env_remove(k); }
    let mode: &[&str] = if write { &["--permission-mode", "acceptEdits"] } else { &["--allowedTools", "Read,Glob,Grep,WebFetch,WebSearch"] };
    let o = c.current_dir(dir).envs(env).args(["-p", task, "--output-format", "text"]).args(mode).output().map_err(|e| e.to_string())?;
    let (out, err) = (String::from_utf8_lossy(&o.stdout).trim().to_string(), String::from_utf8_lossy(&o.stderr).trim().to_string());
    if o.status.success() && !out.is_empty() { Ok(out) } else { Err(if err.is_empty() { "no answer".into() } else { err.chars().take(300).collect() }) }
}

/// Blocking: a fresh OpenCode server for this account (no clauracle MCP: a helper doesn't delegate on), one task, then stop.
pub(crate) fn opencode_once(acc: &store::Account, key: &str, local: Option<(String, u64)>, dir: &str, task: &str, write: bool) -> Result<String, String> {
    let (model, entry) = opencode::provider_for(acc, key, local.as_ref().map(|(u, c)| (u.as_str(), *c)))?;
    let key = if entry.is_some() { "" } else { key };
    let deny: &[&str] = if write { &["*"] } else { &["!edit", "!write", "!patch", "!shell", "!bash"] }; // write: asks are refused, so nothing runs unasked
    let srv = opencode::Server::start_with(dir, deny, &model, key, &format!("clauracle-{}", acc.id), &opencode::config(entry, ""), "")?;
    let r = srv.run_once(task, SECS);
    srv.stop();
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn finds_by_id_or_nick() {
        let mut cfg = Config::default();
        cfg.accounts = vec![store::Account { id: "OR1".into(), nick: "OpenRouter 1".into(), ..Default::default() }];
        assert!(find(&cfg, "openrouter 1").is_some() && find(&cfg, " or1 ").is_some() && find(&cfg, "x").is_none());
    }
    /// Real: a delegated read-only task on the free OpenCode account. `cargo test delegate_live -- --ignored --nocapture` (needs `opencode`).
    #[test] #[ignore] fn delegate_live() {
        let acc = store::Account { id: "OpenCode".into(), nick: "OpenCode (free)".into(), provider: "opencode".into(), ..Default::default() };
        let dir = std::env::temp_dir().to_string_lossy().to_string();
        let out = opencode_once(&acc, "", None, &dir, "Reply with exactly: DELEGATE OK", false).unwrap();
        println!("answer={out}");
        assert!(out.to_uppercase().contains("DELEGATE OK"));
    }
    /// Real: a delegated read-only task on the Claude login (Main). `cargo test delegate_claude_live -- --ignored --nocapture`.
    #[test] #[ignore] fn delegate_claude_live() {
        let dir = std::env::temp_dir().to_string_lossy().to_string();
        let mut c = store::command(&store::which("claude").unwrap());
        c.env_remove("CLAUDECODE");
        drop(c);
        let out = claude_once(&store::which("claude").unwrap(), vec![], &dir, "Reply with exactly: CLAUDE OK", false).unwrap();
        println!("answer={out}");
        assert!(out.to_uppercase().contains("CLAUDE OK"));
    }
}
