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
    assert!(manager_prompt("hi", true).contains("STATUS"));
    assert!(!manager_prompt("hi", false).contains("STATUS"));
}

/// A chat message may be triaged as not-a-task only while it is not a wall of text (reply-reference quotes and attached-file
/// lists not counted): a long one that mixes remarks with asks must not lose its asks to a "remark" verdict (#519).
/// A filed non-task costs one "not planned" close; a lost ask costs more. 40 words filed ordinary questions, so 120.
pub(crate) fn may_triage(ask: &str) -> bool {
    let own = ask.split("\n\n(#[").next().unwrap_or(ask).split("\n\nAttached files:").next().unwrap_or(ask);
    own.split_whitespace().count() <= 120
}
#[test]
fn may_triage_works() {
    assert!(may_triage("done, try again"));
    assert!(may_triage(&format!("thanks!\n\n(#[ab12] = reply reference to your earlier message: \"{}\")", "word ".repeat(80))));
    assert!(may_triage(&"word ".repeat(60)));
    assert!(!may_triage(&"word ".repeat(121)));
}

/// #597 the one model call per chat message (The Manager, Haiku by default): line 1 = what it is, line 2 = a short title.
/// The issue body is the user's own words and images, so the title is all the model writes. `classify` false = always a task.
pub(crate) fn manager_prompt(ask: &str, classify: bool) -> String {
    let what = if classify { "Line 1 = exactly one word: TASK (asks for a change/fix/feature/work), QUESTION (only asks for information/an answer,         e.g. why something happened or how something works), REMARK (comment, feedback, thanks, opinion, small talk: asks for nothing) or         STATUS (a short status or unblock reply about something the user did, e.g. \"done, try again\", \"I turned the Mac on\"; asks for no new work).         A message with any request for a change, even alongside a question or remark, is TASK; when unsure, TASK." } else { "Line 1 = exactly the word TASK." };
    format!("This is a chat message to a software assistant. {what} If TASK, line 2 = a short imperative title for it as a GitHub issue         (at most 80 characters, plain words, no names or personal details; an @[name] marker is a screenshot, don't put markers in the title).         Output only those lines.

Message:
{ask}")
}

/// The filed issue: the model's short title only. The user's exact words and images stay in the local ask file until the
/// session that works the task writes a neutral description into the issue. No title (model down) = a placeholder, never raw words.
pub(crate) fn ask_issue(title: Option<&str>, raw: &str) -> String {
    let t = title.map(str::trim).filter(|t| !t.is_empty()).unwrap_or("New task from Clauracle (title pending)");
    let tags: String = store::hash_tags(raw).iter().map(|x| format!(" #{x}")).collect(); // written #Tags still become labels
    format!("{t}

The user's words and images are in the local ask file (not on GitHub). The session that works on this replaces this line with a neutral description.{tags}")
}
/// Real Haiku on a question, a remark, a task and a screenshot task: class word + title come back as `triage` reads them.
#[test]
#[ignore]
fn manager_live() {
    for (m, classify, want) in [("why did the build fail last night?", true, "question"), ("thanks, that looks great", true, "remark"),
        ("the save button in settings does nothing, make it save the form", true, "task"), ("make this @[a1b2] bigger and blue", false, "task")] {
        let mut c = store::command(&store::which("claude").unwrap());
        for (k, _) in std::env::vars() { if k.starts_with("CLAUDE") || k.starts_with("ANTHROPIC") { c.env_remove(k); } }
        let o = c.args(["-p", &manager_prompt(m, classify), "--model", "haiku", "--output-format", "text"]).output().unwrap();
        let s = String::from_utf8_lossy(&o.stdout);
        let (k, rest) = triage(s.trim_matches('`').trim());
        eprintln!("{m:?} → {k} | {:?}", rest.lines().next().unwrap_or(""));
        assert_eq!(k, want);
        if k == "task" { assert!(!rest.trim().is_empty() && !rest.contains("@[")); }
    }
}

#[test]
fn ask_issue_keeps_words_local() {
    let b = ask_issue(Some("Fix the save button"), "my secret words @[a1] about the save button #ui");
    assert!(b.starts_with("Fix the save button

") && !b.contains("secret") && b.ends_with(" #ui"));
    assert!(ask_issue(None, "private first line").starts_with("New task from Clauracle (title pending)
"));
}

#[tauri::command]
pub(crate) async fn task_do(app: AppHandle, st: St<'_>, pid: String, action: String, mut arg: String, images: Option<Vec<Value>>) -> R<String> {
    let p = st.project(&pid).ok_or("no such project")?;
    let mut err = String::new(); // "new": Sonnet failure → 3rd line of the reply → UI toast
    let chat = action == "ask"; // #336 "ask" = chat message → triaged first, then filed as "new"
    let action = if chat { "new".to_string() } else { action };
    let imgs = images.unwrap_or_default();
    let mut saved: Vec<String> = vec![];
    let mut ask_path: Option<String> = None;
    let id = store::new_id();
    if action == "new" {
        // The ask file is the durable record: exact text plus per-image notes, written before anything can fail.
        let opt = store::att_save_opt(&p.path, &id, &imgs);
        let shots: Vec<(String, String, String)> = opt.iter().zip(&imgs).filter_map(|(f, x)| Some((f.clone()?, match &x["notes"] {
            Value::String(s) => s.clone(),
            Value::Array(a) => a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join("\n"),
            _ => String::new(),
        }, store::img_ref(x)))).collect();
        // "@[ref] = path" so the session reading the issue can tell which file each @[ref] in the text means
        saved = shots.iter().map(|s| format!("@[{}] = {}", s.2, s.0)).collect();
        ask_path = store::ask_save(&p.path, &id, &arg, &shots);
    }
    if action == "new" { // #597 one model call: sort out non-tasks + a short title; the user's words stay in the local ask file
        let (st2, p2, raw) = (st.inner().clone(), p.clone(), arg.clone());
        let dir = store::home(); // = ~/.clauracle (joining again gives a missing dir, os error 267)
        let triaged = chat && imgs.is_empty() && may_triage(&arg); // a message with images, or a long one, is always a task
        let model = { // The Manager (Settings > Agents) picks the model; the old "Message check" choice still counts until it is set
            let cfg = store::proj_cfg(&st.cfg.lock().unwrap(), &p);
            match cfg.agent_models.get("manager") {
                Some(pick) => store::agent_model(pick),
                None => Some(cfg.triage_model.trim().to_string()).filter(|m| !m.is_empty()),
            }.unwrap_or_else(|| "haiku".into())
        };
        // #572 The Manager shows as working in the Agents tab while it checks (first words of the message, no secrets beyond what the user typed)
        let what: String = arg.split_whitespace().take(8).collect::<Vec<_>>().join(" ");
        claude::emit(&app, "manager", json!({"pid": pid, "doing": what}));
        let r = tauri::async_runtime::spawn_blocking(move || {
            claude_pt(&st2, &p2, &["--model", &model], &dir, vec![], &manager_prompt(&raw, triaged), Some(std::time::Duration::from_secs(30)))
        }).await.map_err(|e| e.to_string())?;
        claude::emit(&app, "manager", json!({"pid": pid, "doing": null}));
        // A failure or an unclear reply counts as a task (placeholder title): an ask is never dropped.
        let title = match r.as_ref().map(|s| triage(s.trim_matches('`').trim())) {
            Ok((c, _)) if c != "task" && triaged => {
                if let Some(a) = &ask_path { store::ask_note(&p.path, a, &format!("Not filed: {c}")); }
                return Ok(format!("triage:{c}"));
            }
            Ok((_, rest)) => rest.lines().next().map(|t| t.trim().trim_matches('"').chars().take(100).collect::<String>()),
            Err(e) => { err = format!("
Title failed ({}): placeholder title filed", e.lines().last().unwrap_or("no output")); None }
        };
        arg = ask_issue(title.as_deref(), &arg);
    } else { saved = store::att_save(&p.path, &imgs); }
    if !saved.is_empty() {
        arg += &format!("\n\nScreenshots (local to the project folder, not on GitHub; open them with the Read tool from the repo root; an @[name] in the text = the thing in that image, read it there; drawn markup/notes on them = instructions, keep these lines when editing or splitting):\n{}", saved.iter().map(|f| format!("- {f}")).collect::<Vec<_>>().join("\n"));
    }
    // "new" returns "URL, newline, filed title" (the UI row shows the neutral title, not the raw ask)
    tauri::async_runtime::spawn_blocking(move || match store::task_do(&p.path, &p.gh, &action, &arg) {
        Ok(u) if action == "new" => {
            if let Some(a) = &ask_path { store::ask_filed(&p.path, a, u.lines().next().unwrap_or("").trim()); }
            Ok(format!("{u}\n{}{err}", store::split_task(&arg).0))
        }
        // every ask file ends with its outcome: a gh failure used to leave none (ask-66000a2b148c5.md), so nobody could tell
        Err(e) => { if let Some(a) = &ask_path { store::ask_note(&p.path, a, &format!("Not filed: {}", e.lines().last().unwrap_or("gh failed"))); } Err(e) }
        r => r,
    }).await.map_err(|e| e.to_string())?
}

/// A queued ask edited after it was filed: the new words go into its ask file (local only), which wins over the issue
/// text, so the session that works it later sees the edit (it used to keep the first version: an edit was lost).
#[tauri::command]
pub(crate) fn ask_edit(st: St, pid: String, n: u64, text: String) -> R<()> {
    let p = st.project(&pid).ok_or("no such project")?;
    let f = store::ask_of(&p.path, n).ok_or("no ask file")?;
    store::ask_note(&p.path, &f, &format!("Edited by the user before it was worked on (this wins over the text above):\n{}", text.trim()));
    Ok(())
}

#[tauri::command]
pub(crate) async fn gh_repos(st: St<'_>, user: String) -> R<Value> {
    // user/repos ∵ `gh repo list` = owned only; this adds collaborator + org repos. --paginate → one array per page (a line each)
    let q = "[.[] | {nameWithOwner: .full_name, description, branch: .default_branch}]";
    // the account card's "Organization repos" switch (cfg.off "ghorg:<user>")
    let aff = if st.cfg.lock().unwrap().off.contains(&format!("ghorg:{user}")) { "owner,collaborator" } else { "owner,collaborator,organization_member" };
    let url = format!("user/repos?per_page=100&sort=pushed&affiliation={aff}");
    Ok(tauri::async_runtime::spawn_blocking(move || {
        let o = store::run("gh", &["api", "--paginate", &url, "--jq", q], None, &store::gh_env(&user));
        if !o.ok { return Value::Null; }
        Value::Array(o.stdout.lines().filter_map(|l| serde_json::from_str::<Vec<Value>>(l).ok()).flatten().collect())
    }).await.unwrap_or(Value::Null))
}

/// Account card details: name, owned repo count, last public activity, organizations.
#[tauri::command]
pub(crate) async fn gh_account(user: String) -> Value {
    tauri::async_runtime::spawn_blocking(move || {
        let env = store::gh_env(&user);
        let api = |path: &str, q: &str| { let o = store::run("gh", &["api", path, "--jq", q], None, &env); if o.ok { o.stdout.trim().to_string() } else { String::new() } };
        let me: Value = serde_json::from_str(&api("user", "{name, repos: (.public_repos + (.total_private_repos // 0))}")).unwrap_or(Value::Null);
        let orgs: Value = serde_json::from_str(&api("user/orgs", "[.[].login]")).unwrap_or(json!([]));
        json!({"name": me["name"], "repos": me["repos"], "orgs": orgs, "last": api(&format!("users/{user}/events?per_page=1"), ".[0].created_at // empty")})
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
