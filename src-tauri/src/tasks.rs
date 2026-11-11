//! Commands: task triage and filing, gh repos and clones, plan moves.
use crate::*;

// #336 triage: line 1 = class word; QUESTION/REMARK → never filed. Unparsable line 1 → TASK (doubt → file, #212)
pub(crate) fn triage(s: &str) -> (&'static str, &str) {
    let (h, rest) = s.split_once('\n').unwrap_or((s, ""));
    match h.trim().trim_matches(|c: char| !c.is_alphabetic()).to_ascii_uppercase().as_str() {
        "QUESTION" => ("question", ""), "REMARK" => ("remark", ""), "STATUS" => ("status", ""), "TASK" => ("task", rest.trim()), _ => ("task", s.trim()),
    }
}
#[test]
pub(crate) fn triage_works() {
    assert_eq!(triage("REMARK"), ("remark", ""));
    assert_eq!(triage("**Question**\n"), ("question", ""));
    assert_eq!(triage("TASK\nFix X\n\n- a"), ("task", "Fix X\n\n- a"));
    assert_eq!(triage("Fix X\n\n- a"), ("task", "Fix X\n\n- a"));
    assert_eq!(triage("STATUS"), ("status", ""));
    assert_eq!(triage("status.\nthanks"), ("status", ""));
    assert!(neutral_prompt("hi", true).contains("STATUS"));
}

pub(crate) fn neutral_prompt(ask: &str, chat: bool) -> String { // chat=false (accepted card) → always TASK
    let pick = if chat { "" } else { " It was accepted as a task: line 1 = TASK." };
    format!("Triage this chat message to a software assistant.{pick} Line 1 = exactly one word: TASK (asks for a change/fix/feature/work), \
        QUESTION (only asks for information/an answer), REMARK (comment, feedback, thanks, opinion, small talk: asks for nothing) or \
        STATUS (a short status or unblock reply about something the user did, e.g. \"done, try again\", \"I turned the Mac on\"; asks for no new work). \
        A message that contains any request for a change, even alongside a question or remark, is TASK; when unsure, TASK. \
        Not TASK → output only that word, nothing else. TASK → after line 1: rewrite this request to a software assistant as a GitHub issue. It goes on a shared repo, so write it neutrally: \
        no quotes of the request, no personal remarks, names, emotions or private details; keep every technical fact, file path and \
        requirement. If it doesn't name which screen/element it means (vague \"buttons\", \"this\"), never guess: end the summary with \
        \"Target unclear: confirm which UI element with the user (screenshot) before changing anything.\" Output ONLY plain text: line 1 = TASK, line 2 = imperative title (≤ 80 chars), blank line, then a short task summary (bullets ok). \
        No preamble, no code fences.\n\nRequest:\n{ask}")
}

#[tauri::command]
pub(crate) async fn task_do(st: St<'_>, pid: String, action: String, mut arg: String, images: Option<Vec<Value>>) -> R<String> {
    let p = st.project(&pid).ok_or("no such project")?;
    let mut err = String::new(); // "new": Sonnet failure → 3rd line of the reply → UI toast
    let chat = action == "ask"; // #336 "ask" = chat message → triaged first, then filed as "new"
    let action = if chat { "new".to_string() } else { action };
    let imgs = images.unwrap_or_default();
    let mut saved: Vec<String> = vec![];
    let mut ask_path: Option<String> = None;
    if action == "new" {
        // The ask file is the durable record: exact text plus per-image notes, written before anything can fail.
        let opt = store::att_save_opt(&p.path, &imgs);
        let shots: Vec<(String, String)> = opt.iter().zip(&imgs).filter_map(|(f, x)| Some((f.clone()?, match &x["notes"] {
            Value::String(s) => s.clone(),
            Value::Array(a) => a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join("\n"),
            _ => String::new(),
        }))).collect();
        saved = shots.iter().map(|s| s.0.clone()).collect();
        ask_path = store::ask_save(&p.path, &arg, &shots);
    }
    if action == "new" { // privacy: raw ask never reaches GitHub (edit history keeps first bodies), Sonnet's neutral rewrite is filed
        let tags: String = store::hash_tags(&arg).iter().map(|t| format!(" #{t}")).collect(); // raw #Tags still become labels
        let (st2, p2, raw) = (st.inner().clone(), p.clone(), arg.clone());
        let dir = store::home(); // = ~/.clauracle (joining again gives a missing dir, os error 267)
        let triaged = chat && imgs.is_empty(); // a message with images is always a task
        let r = tauri::async_runtime::spawn_blocking(move || claude_pt(&st2, &p2, &["--model", "sonnet", "--effort", "low"], &dir, vec![], &neutral_prompt(&raw, triaged), Some(std::time::Duration::from_secs(60)))).await.map_err(|e| e.to_string())?
            .map(|s| s.trim_matches('`').trim().to_string()).and_then(|s| if s.is_empty() { Err("empty reply".into()) } else { Ok(s) });
        // Not a task: nothing filed, the UI gets "triage:<class>". Sonnet down: filed anyway, a lost ask costs more.
        let r = match r { Ok(s) => match triage(&s) { ("task", b) if !b.is_empty() => Ok(b.to_string()), ("task", _) => Err("empty reply".to_string()), (c, _) if triaged => {
            if let Some(a) = &ask_path { store::ask_note(&p.path, a, &format!("Not filed: {c}")); }
            return Ok(format!("triage:{c}"))
        }, _ => Err("not a task".to_string()) }, e => e };
        // ponytail: Sonnet down gives a placeholder title; the session working it reads the ask file. err goes to a UI toast.
        if let Err(e) = &r { err = format!("\nNeutral rewrite failed ({}): placeholder title filed", e.lines().last().unwrap_or("no output")); }
        arg = r.unwrap_or_else(|_| "New task from Clauracle (summary pending)".into()) + &tags;    } else { saved = store::att_save(&p.path, &imgs); }
    if !saved.is_empty() {
        arg += &format!("\n\nScreenshots (local to the project folder, not on GitHub; open them with the Read tool from the repo root; drawn markup/notes on them = instructions, keep these lines when editing or splitting):\n{}", saved.iter().map(|f| format!("- {f}")).collect::<Vec<_>>().join("\n"));
    }
    // "new" returns "URL, newline, filed title" (the UI row shows the neutral title, not the raw ask)
    tauri::async_runtime::spawn_blocking(move || store::task_do(&p.path, &p.gh, &action, &arg)
        .map(|u| if action == "new" {
            if let Some(a) = &ask_path { store::ask_filed(&p.path, a, u.lines().next().unwrap_or("").trim()); }
            format!("{u}\n{}{err}", store::split_task(&arg).0)
        } else { u })).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn gh_repos(user: String) -> Value {
    // user/repos ∵ `gh repo list` = owned only; this adds collaborator + org repos. --paginate → one array per page (a line each)
    let q = "[.[] | {nameWithOwner: .full_name, description, branch: .default_branch}]";
    tauri::async_runtime::spawn_blocking(move || {
        let o = store::run("gh", &["api", "--paginate", "user/repos?per_page=100&sort=pushed&affiliation=owner,collaborator,organization_member", "--jq", q], None, &store::gh_env(&user));
        if !o.ok { return Value::Null; }
        Value::Array(o.stdout.lines().filter_map(|l| serde_json::from_str::<Vec<Value>>(l).ok()).flatten().collect())
    }).await.unwrap_or(Value::Null)
}

#[tauri::command]
pub(crate) async fn gh_branches(user: String, repo: String) -> Vec<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let o = store::run("gh", &["api", "--paginate", &format!("repos/{repo}/branches?per_page=100"), "--jq", ".[].name"], None, &store::gh_env(&user));
        o.stdout.lines().map(String::from).collect()
    }).await.unwrap_or_default()
}

/// `branch` = existing branch to check out (None = default); `new_branch` = made from it after cloning.
#[tauri::command]
pub(crate) async fn clone_repo(st: St<'_>, user: String, repo: String, branch: Option<String>, new_branch: Option<String>) -> R<Value> {
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

/// Plan tab drag: reorders the primary plan's Queue (claude::queue_move); to = null → removes the task (ticked done).
#[tauri::command]
pub(crate) fn plan_move(st: St, pid: String, from: usize, to: Option<usize>, text: Option<String>) -> R<()> {
    claude::queue_move(&st.project(&pid).ok_or("no such project")?.path, from, to, text.as_deref())
}

/// Plan tab suggestion i: accept → moved to the Queue's end, else dismissed (claude::wait_take).
#[tauri::command]
pub(crate) fn plan_wait(st: St, pid: String, i: usize, accept: bool) -> R<()> {
    claude::wait_take(&st.project(&pid).ok_or("no such project")?.path, i, accept)
}
