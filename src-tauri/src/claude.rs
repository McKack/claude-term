//! One `claude` process per project, spoken to over its stream-json protocol (what claude-agent-sdk does):
//! user messages in, messages out, plus control requests both ways (permissions, our in-process MCP tools,
//! interrupt, context usage). On top: the continue modes, held questions, limits and pool switching.
use crate::store::{self, Config, Project, Question};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, Notify};

const LESSONS_MAX: usize = 30; // ponytail: ~30 one-liners ≈ 1k tokens per session; prune in Settings › Accounts
const PONYTAIL_MD: &str =include_str!("../../plugin/skills/ponytail/SKILL.md");

const APPEND: &str = "You are running inside Clauracle, a desktop app around Claude Code that keeps long work going \
across fresh sessions through a task list and HANDOVER.md. Its tools: mcp__clauracle__ready (the task is done and plan + \
handover are written: Clauracle clears the context and continues) and mcp__clauracle__ask (questions for the user, queued; never \
wait for them). Save tokens: hand searching, reading many files and running builds/tests to the helper agents \
(Task tool) when they fit, and keep your own replies short.";

const HANDOVER_RULES: &str = "
# Handover (Clauracle)
- PLAN.md at the project root = the ONE task list: an ordered \"## Queue\" of open tasks (first = next) holding only
  work you can start now. \"## Waiting on the user\" = YOUR OWN ideas needing the user's yes (shown as suggestions): rare,
  only clearly high-value ones, never already in the Queue, done (CHANGES.md) or asked by the user. A user's request
  stays in the Queue even while blocked on an answer (skip it until answered; never demote it to Waiting).
  HANDOVER.md lets a fresh session resume: goal, current state, how to build and test, gotchas; no task list. Each
  under ~50 lines. If HANDOVER.md (or TASKS.md/TODO.md) still has open tasks, move them into PLAN.md. In a git repo
  where these aren't tracked, add them to .git/info/exclude. Finished work goes to CHANGES.md (newest first, one line
  per change), not the plan or handover. Every Queue item must be finishable: never add standing items or rules
  (\"respond to the user\", \"always do X\"); rules go to HANDOVER.md. Empty Queue = done.
- Each new user request goes into the Queue at once (reworded, by priority), so nothing is lost across sessions.
- Finishing a task: verify it, update PLAN.md, HANDOVER.md and CHANGES.md, commit if the project's rules allow, then call
  mcp__clauracle__ready as the LAST action of the turn.
- Do NOT call ready, stop and say why instead, when something failed you can't fix or the queue is empty.
- Questions for the user go through mcp__clauracle__ask (numbered, options with your recommendation marked). They
  are queued and answered later: carry on with whatever doesn't depend on them.";

/// HANDOVER_RULES for a project on GitHub: tasks = issues (one list for Plan tab, Next pill, Continue, team; no PLAN.md drift).
const ISSUE_RULES: &str = "
# Handover (Clauracle)
- Tasks = open GitHub issues labelled `clauracle`: the ONE task list (no PLAN.md). Clauracle's continue message lists
  them in order (first = next; work on that one, not a later one); `gh issue view N` for details. Skip issues someone else is assigned to.
- Each new user request becomes an issue at once (`gh issue create --label clauracle --label <type> --title <short>
  --body <full request>`, type = task | feature | fix; `gh label create` it if missing), so nothing is lost across
  sessions. Your own ideas needing the user's yes: add `--label suggestion` (rare, only
  clearly high-value ones); the user accepts or dismisses them. Every issue must be finishable work, never a standing rule.
- Open work never lives in files: follow-ups, unverified checks, \"next: …\", things waiting on the user's input or
  restatement each become an issue (user's decision → `--label suggestion`). If PLAN.md, TASKS.md, TODO.md or
  HANDOVER.md still list open tasks, file them as issues (same order) and remove them there.
- HANDOVER.md lets a fresh session resume: goal, current state, how to build and test, gotchas, rules; no task list,
  no open items;
  under ~50 lines. Finished work goes to CHANGES.md (newest first, one line per change). In a git repo where these
  aren't tracked, add them to .git/info/exclude.
- Finishing a task: verify it, `gh issue close N -c <one-line summary>`, update HANDOVER.md and CHANGES.md, commit if
  the project's rules allow, then call mcp__clauracle__ready as the LAST action of the turn.
- Do NOT call ready, stop and say why instead, when something failed you can't fix or no task is left.
- Questions for the user go through mcp__clauracle__ask (numbered, options with your recommendation marked). They
  are queued and answered later: carry on with whatever doesn't depend on them.";

/// Tasks live in GitHub issues: the project is on GitHub and has a gh account to read them with.
pub fn issue_mode(p: &Project) -> bool { !p.owner.is_empty() && !p.gh.is_empty() }

const AUTO_RULES: &str = "
# Auto mode: the user is away
Work ONE task per session from the task queue, then hand over with mcp__clauracle__ready; Clauracle starts the
next session by itself.";

const AUTO_ANSWER_RULES: &str = "
# Auto mode with answers: the user is away
Work ONE task per session from the task queue, then hand over with mcp__clauracle__ready. When a question has a
sensible recommended answer, decide it yourself, go with that, and note the decision in CHANGES.md. Only questions
that truly need the user (taste, money, accounts, irreversible or risky choices) go to mcp__clauracle__ask with
blocking=true; they are held, and you continue with other tasks.";

const TERSE_RULES: &str = "
# Comments: dense, for AI readers
Write code comments as compact notes an AI reads fully: fragments, symbols (→ = ≠ ∵ ∴), no filler words, but keep
every fact (why, constraint, edge case, upgrade path). Example: `// cache: 5m TTL ∵ API rate limit; ↑ if stale data hurts`.";

const PREPARE: &str = "Pause here; don't start anything new{why}. Update HANDOVER.md so a fresh session can pick up \
exactly where you are (goal, what you were doing and how far it got, what is verified and what isn't, gotchas \
learned) and the task list (PLAN.md's Queue, or GitHub issues when tasks live there: next steps in order; there every open follow-up, unverified check or wait on the user = an issue, not a HANDOVER.md line), move finished work to CHANGES.md, then call mcp__clauracle__ready as the LAST action.";

const RESUMED: &str = "continue

Clauracle restarted while you were working (a restart you asked for, an update, or a crash), so your last turn was cut off. Check where you got to (files, git status) and carry on.";

pub const CONTINUE: &str = "continue\n\nThis is a fresh session (Clauracle cleared the context). Read HANDOVER.md and \
PLAN.md at the project root and carry on with the next step.";
// #87: nothing queued but user text rides along (answers) → no PLAN.md/task wording ∵ issue mode has no PLAN.md
pub const CONTINUE_BARE: &str = "continue\n\nThis is a fresh session (Clauracle cleared the context). Read HANDOVER.md \
for context; no tasks are open, so handle the message below.";
// same first 60 chars as CONTINUE (history() matches that prefix)
const CONTINUE_ISSUES: &str = "continue\n\nThis is a fresh session (Clauracle cleared the context). Read HANDOVER.md and \
carry on with the next open task (GitHub issues labelled `clauracle`, first = next). Work on the FIRST one only; \
the rest wait until it's closed:"; // #74: "first = next" alone → AI drifted to a later issue; UI highlights the first

/// The "continue" message for a fresh session, or None when nothing is queued. Issue mode: one `gh issue list`
/// (blocking, ~1s) → ordered "#N title" lines; gh failing → generic text (a network blip mustn't stop a run).
pub fn cont_text(p: &Project) -> Option<String> {
    if !issue_mode(p) {
        return (!queue(&p.path).is_empty()).then(|| CONTINUE.into());
    }
    let v = store::tasks(&p.path, &p.gh);
    if !v.is_array() {
        return Some(format!("{CONTINUE_ISSUES} run `gh issue list --label clauracle` to see them."));
    }
    let q = store::task_queue(&v, &p.gh, &p.order);
    (!q.is_empty()).then(|| format!("{CONTINUE_ISSUES}\n{}",
        q.iter().map(|x| format!("- #{} {}", x["number"], x["title"].as_str().unwrap_or(""))).collect::<Vec<_>>().join("\n")))
}

// ---------------------------------------------------------------------------------------------

pub enum Input {
    Msg(String),
    Rich(Value), // content blocks: text plus pasted images
    Reconnect,
}

/// The project's queue of things to send. `front` puts items ahead of what you queued meanwhile.
#[derive(Default)]
pub struct Inbox {
    q: Mutex<VecDeque<Input>>,
    n: Notify,
}
impl Inbox {
    pub fn push(&self, i: Input) {
        self.q.lock().unwrap().push_back(i);
        self.n.notify_one();
    }
    pub fn front(&self, items: Vec<Input>) {
        let mut q = self.q.lock().unwrap();
        for i in items.into_iter().rev() {
            q.push_front(i);
        }
        drop(q);
        self.n.notify_one();
    }
    async fn pop(&self) -> Input {
        loop {
            if let Some(i) = self.q.lock().unwrap().pop_front() {
                return i;
            }
            self.n.notified().await;
        }
    }
}

/// Writing to the running claude, and waiting for answers to our control requests.
pub struct Conn {
    stdin: tokio::sync::Mutex<tokio::process::ChildStdin>,
    pending: Mutex<HashMap<String, oneshot::Sender<Value>>>,
    counter: AtomicU64,
}
impl Conn {
    async fn write(&self, v: &Value) -> Result<(), String> {
        let mut s = self.stdin.lock().await;
        s.write_all(format!("{v}\n").as_bytes()).await.map_err(|e| e.to_string())?;
        s.flush().await.map_err(|e| e.to_string())
    }
    pub async fn request(&self, req: Value, secs: u64) -> Result<Value, String> {
        let id = format!("req_{}_{}", self.counter.fetch_add(1, SeqCst), store::new_id());
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id.clone(), tx);
        self.write(&json!({"type": "control_request", "request_id": id, "request": req})).await?;
        match tokio::time::timeout(std::time::Duration::from_secs(secs), rx).await {
            Ok(Ok(r)) if r["subtype"] == "error" => Err(r["error"].as_str().unwrap_or("error").into()),
            Ok(Ok(r)) => Ok(r["response"].clone()),
            _ => {
                self.pending.lock().unwrap().remove(&id);
                Err(format!("no answer to {}", req["subtype"]))
            }
        }
    }
}

/// Per-project live state.
#[derive(Default)]
pub struct Live {
    pub inbox: Inbox,
    pub busy: AtomicBool,
    pub conn: Mutex<Option<Arc<Conn>>>,
    armed: AtomicBool,        // ready was called this turn
    handing_over: AtomicBool, // Clauracle asked for a handover (context limit / Clear & continue)
    first_turn: AtomicBool,   // a fresh session already past the context limit must not hand over forever
    pin: AtomicU64,           // #83: issue this session was started on (0 = none); ready refuses once while it's open
    nagged: AtomicBool,       // pin refusal already sent → 2nd ready goes through (AI insists: blocked/waiting)
    built: Mutex<String>,     // #89 smart build: HEAD at session start / last good smart build = diff base
}

/// #83: the issue a continue message pins = its first "- #N" line (task_queue order, taken ones skipped).
fn pin_of(msg: &str) -> u64 {
    if !msg.starts_with(CONTINUE_ISSUES) { return 0; }
    msg.split("\n- #").nth(1).and_then(|s| s.split(' ').next()?.parse().ok()).unwrap_or(0)
}

#[test]
fn pin_of_works() {
    assert_eq!(pin_of(&format!("{CONTINUE_ISSUES}\n- #83 Pick tasks\n- #84 x")), 83);
    assert_eq!(pin_of(CONTINUE), 0);
    assert_eq!(pin_of(&format!("{CONTINUE_ISSUES} run `gh issue list --label clauracle` to see them.")), 0);
}

pub struct State {
    pub cfg: Mutex<Config>,
    pub live: Mutex<HashMap<String, Arc<Live>>>,
    pub asks: Mutex<HashMap<String, oneshot::Sender<Value>>>,
    pub plugin_dir: PathBuf,
}

impl State {
    pub fn project(&self, pid: &str) -> Option<Project> {
        self.cfg.lock().unwrap().projects.iter().find(|p| p.id == pid).cloned()
    }
    pub fn with_project<R>(&self, pid: &str, f: impl FnOnce(&mut Project) -> R) -> Option<R> {
        let mut cfg = self.cfg.lock().unwrap();
        let r = cfg.projects.iter_mut().find(|p| p.id == pid).map(f);
        store::save(&cfg);
        r
    }
}

pub fn emit(app: &AppHandle, event: &str, data: Value) {
    let _ = app.emit("clauracle", json!({"event": event, "data": data}));
}

pub fn line(app: &AppHandle, pid: &str, kind: &str, text: &str) {
    emit(app, "line", json!({"pid": pid, "kind": kind, "text": text}));
}

/// The project's session, started on first use.
pub fn session(app: &AppHandle, st: &Arc<State>, pid: &str) -> Arc<Live> {
    let mut map = st.live.lock().unwrap();
    if let Some(l) = map.get(pid) {
        return l.clone();
    }
    let live = Arc::new(Live::default());
    live.first_turn.store(true, SeqCst);
    map.insert(pid.into(), live.clone());
    let (app, st, pid, l) = (app.clone(), st.clone(), pid.to_string(), live.clone());
    tauri::async_runtime::spawn(async move { run(app, st, pid, l).await });
    live
}

async fn run(app: AppHandle, st: Arc<State>, pid: String, live: Arc<Live>) {
    if let Some(p) = st.project(&pid) {
        let head = tauri::async_runtime::spawn_blocking(move || store::head(&p.path)).await.unwrap_or_default();
        *live.built.lock().unwrap() = head;
    }
    loop {
        if st.project(&pid).is_none() || !st.live.lock().unwrap().get(&pid).is_some_and(|l| Arc::ptr_eq(l, &live)) {
            return; // removed, or its live entry dropped/replaced (move_folder): a new session starts on open
        }
        if let Err(e) = connect_and_serve(&app, &st, &pid, &live).await {
            *live.conn.lock().unwrap() = None;
            live.busy.store(false, SeqCst);
            emit(&app, "status", json!({"pid": pid, "busy": false}));
            if e != "reconnect" {
                line(&app, &pid, "err", &format!("claude: {e} (retrying in 5s)"));
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}

fn args(st: &State, p: &Project) -> Vec<String> {
    let cfg = st.cfg.lock().unwrap();
    let (deny, _) = store::restrictions(&cfg, p);
    let mut append = format!("{APPEND}\n{}", if issue_mode(p) { ISSUE_RULES } else { HANDOVER_RULES });
    append += match p.cont.as_str() { "auto" => AUTO_RULES, "auto_answer" => AUTO_ANSWER_RULES, _ => "" };
    if p.terse {
        append += TERSE_RULES;
    }
    if !p.ai_credit {
        append += "\n# Attribution\nNo AI credit in this repo: no Co-Authored-By trailers, no \"Generated with\" lines, never add \
            yourself as contributor/collaborator/author in commits, PRs, or files.\n";
    }
    if !p.build.build.is_empty() || !p.build.run.is_empty() {
        append += &format!("\n# Build (Clauracle runs these itself after a task; don't repeat them)\nbuild: {}\nrun: {}\n",
                           p.build.build, p.build.run);
    }
    if issue_mode(p) { // ponytail: no setting ∵ GitHub project ⇒ issues always; local-only ⇒ PLAN.md
        append += "\n# Team tasks\nAn issue with an assignee and the `in progress` label is taken. Before starting one, check \
            it's free and claim it (`gh issue edit N --add-assignee @me --add-label \"in progress\"`).\n";
    }
    if !p.gcp.is_empty() || !p.gaccount.is_empty() {
        append += &format!("\n# Google Cloud\ngcloud is already pointed at this project's account ({}) and Cloud project ({}) \
            through env vars: don't run `gcloud config set` or switch accounts.\n", p.gaccount, p.gcp);
    }
    if !cfg.lessons.is_empty() {
        append += "\n# Lessons (never repeat; add one with mcp__clauracle__lesson after a costly mistake)\n- ";
        append += &cfg.lessons.join("\n- ");
    }
    if p.ponytail {
        append += "\n\n";
        append += PONYTAIL_MD.splitn(3, "---").nth(2).unwrap_or(PONYTAIL_MD).trim();
    }
    let mcp = json!({"mcpServers": {"clauracle": {"type": "sdk", "name": "clauracle"}}});
    let mut a: Vec<String> = ["--output-format", "stream-json", "--verbose", "--input-format", "stream-json",
        "--include-partial-messages", "--permission-prompt-tool", "stdio"].iter().map(|s| s.to_string()).collect();
    a.extend(["--append-system-prompt".into(), append, "--permission-mode".into(), p.mode.clone(),
              "--plugin-dir".into(), st.plugin_dir.to_string_lossy().into(), "--mcp-config".into(), mcp.to_string()]);
    // per-project CLI settings: only picked connections/plugins load (store::cli_settings), no AI credit unless the
    // project turns it on. A file, not inline JSON (cmd /c mangles quotes)
    // base = the AI account's own settings (Accounts › Advanced AI settings), project restrictions on top
    let mut set = store::account_settings(&p.account).and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}));
    store::merge_settings(&mut set, &store::cli_settings(&cfg, p));
    if !p.ai_credit {
        set["includeCoAuthoredBy"] = json!(false);
        set["attribution"] = json!({"commit": "", "pr": ""});
    }
    let f = store::home().join(format!("settings-{}.json", p.id));
    if std::fs::write(&f, set.to_string()).is_ok() {
        a.extend(["--settings".into(), f.to_string_lossy().into()]);
    }
    if !p.effort.is_empty() {
        a.extend(["--effort".into(), p.effort.clone()]);
    }
    if !p.model.is_empty() {
        a.extend(["--model".into(), p.model.clone()]);
    }
    if !deny.is_empty() {
        a.extend(["--disallowedTools".into(), deny.join(",")]);
    }
    if let Some(s) = &p.session {
        a.push(format!("--resume={s}")); // = form: a dash-leading value can't smuggle in a flag
    }
    a
}

async fn connect_and_serve(app: &AppHandle, st: &Arc<State>, pid: &str, live: &Arc<Live>) -> Result<(), String> {
    let p = st.project(pid).ok_or("project removed")?;
    let claude = store::which("claude").ok_or("Claude Code isn't installed (Settings › Install)")?;
    let base = st.cfg.lock().unwrap().accounts.iter().find(|a| a.id == p.account).map(|a| a.base_url.clone()).unwrap_or_default();
    let env: Vec<(String, String)> = [store::account_env(&p.account, &base),store::gh_env(&p.gh), store::gcloud_env(&p.gaccount, &p.gcp),
        vec![("CLAUDE_CODE_ENTRYPOINT".into(), "sdk-clauracle".into())]].concat();
    let mut cmd = tokio::process::Command::from(store::command(&claude));
    if !base.is_empty() {
        cmd.env_remove("CLAUDE_CODE_OAUTH_TOKEN");
    }
    cmd.args(args(st, &p)).envs(env).env_remove("CLAUDECODE").current_dir(&p.path)
        .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null()).kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    child.id().map(crate::track);
    crate::adopt(&child);
    let conn = Arc::new(Conn { stdin: tokio::sync::Mutex::new(child.stdin.take().unwrap()),
                               pending: Mutex::new(HashMap::new()), counter: AtomicU64::new(0) });
    let (msgs_tx, mut msgs) = mpsc::unbounded_channel::<Value>();

    // reader: control traffic is handled here, everything else goes to the turn loop
    let stdout = child.stdout.take().unwrap();
    let (app2, st2, pid2, live2, conn2) = (app.clone(), st.clone(), pid.to_string(), live.clone(), conn.clone());
    tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            let Ok(v) = serde_json::from_str::<Value>(&l) else { continue };
            match v["type"].as_str() {
                Some("control_response") => {
                    let id = v["response"]["request_id"].as_str().unwrap_or("").to_string();
                    if let Some(tx) = conn2.pending.lock().unwrap().remove(&id) {
                        let _ = tx.send(v["response"].clone());
                    }
                }
                Some("control_request") => {
                    let (a, s, p, l, c) = (app2.clone(), st2.clone(), pid2.clone(), live2.clone(), conn2.clone());
                    tauri::async_runtime::spawn(async move { control(&a, &s, &p, &l, &c, v).await });
                }
                Some("control_cancel_request") | Some("keep_alive") => {}
                _ => {
                    let _ = msgs_tx.send(v);
                }
            }
        }
    });

    let p = st.project(pid).unwrap();
    let (_, skills) = store::restrictions(&st.cfg.lock().unwrap(), &p);
    let mut init = json!({"subtype": "initialize", "hooks": null});
    if let Some(s) = skills {
        init["skills"] = json!(s);
    }
    // Settings › Agents: cheaper helpers Claude can hand work to
    let agents = st.cfg.lock().unwrap().agents.clone().unwrap_or_else(store::default_agents);
    let agents: serde_json::Map<String, Value> = agents.iter().filter(|a| a.on && !a.name.trim().is_empty())
        .map(|a| (a.name.trim().to_string(), json!({"description": a.description, "prompt": a.prompt,
                                                     "model": if a.model.is_empty() { "inherit" } else { a.model.as_str() }})))
        .collect();
    if !agents.is_empty() && st.cfg.lock().unwrap().agents_on {
        init["agents"] = Value::Object(agents);
    }
    let r = conn.request(init, 90).await?;
    // /commands for the composer's autocomplete: [{name, description, argumentHint}], known before the first turn
    emit(app, "commands", json!({"pid": pid, "commands": r["commands"]}));
    *live.conn.lock().unwrap() = Some(conn.clone());
    emit(app, "status", json!({"pid": pid, "busy": false, "connected": true}));

    loop {
        // idle: our next input, OR output from a turn the CLI started itself (background task finished → it
        // wakes Claude). Without the 2nd arm that turn's `result` ended OUR next turn → every turn shown one late.
        let mut first = tokio::select! {
            i = live.inbox.pop() => {
                let content = match i {
                    Input::Msg(text) => json!(text),
                    Input::Rich(blocks) => blocks,
                    Input::Reconnect => {
                        let _ = child.kill().await;
                        return Err("reconnect".into());
                    }
                };
                live.busy.store(true, SeqCst);
                st.with_project(pid, |p| p.working = true);
                emit(app, "status", json!({"pid": pid, "busy": true, "handover": live.handing_over.load(SeqCst)}));
                conn.write(&json!({"type": "user", "message": {"role": "user", "content": content},
                                   "parent_tool_use_id": null, "session_id": "default"})).await?;
                None
            }
            m = msgs.recv() => {
                let m = m.ok_or("claude stopped")?;
                // between-turn noise (late result, system notices other than a turn's init) → show, stay idle
                if m["type"] == "result" || (m["type"] == "system" && m["subtype"] != "init") { show(app, st, pid, &p.account, &m); continue; }
                live.busy.store(true, SeqCst);
                st.with_project(pid, |p| p.working = true);
                emit(app, "status", json!({"pid": pid, "busy": true, "handover": false}));
                Some(m)
            }
        };
        loop {
            let m = match first.take() { Some(m) => m, None => msgs.recv().await.ok_or("claude stopped")? };
            show(app, st, pid, &p.account, &m);
            if m["type"] == "result" {
                break;
            }
        }
        live.busy.store(false, SeqCst);
        st.with_project(pid, |p| p.working = false);
        // armed (ready called): UI stays busy through build steps until `ready` is set ∵ busy:false flushes the UI
        // queue; before ready it saw no Continue waiting → user msgs went into the OLD session instead of toTasks
        let armed = live.armed.load(SeqCst);
        if !armed { emit(app, "status", json!({"pid": pid, "busy": false})); }
        after_turn(app, st, pid, live, &conn).await;
        if armed { emit(app, "status", json!({"pid": pid, "busy": false})); }
    }
}

fn show(app: &AppHandle, st: &State, pid: &str, acct: &str, m: &Value) { // acct = the account this CLI was spawned with
    // the id is known from the start: a first turn cut off by a crash can still be resumed
    if m["type"] == "system" && m["subtype"] == "init" {
        if let Some(sid) = m["session_id"].as_str() {
            st.with_project(pid, |p| p.session = Some(sid.into()));
        }
        // #155 unknown server (claude.ai connector) → catalog → every session reconnects w/ it denied unless picked
        let added = { let mut cfg = st.cfg.lock().unwrap(); let a = store::mcp_seen(&mut cfg, m); if a { store::save(&cfg); } a };
        if added { for l in st.live.lock().unwrap().values() { l.inbox.push(Input::Reconnect); } }
    }
    match m["type"].as_str() {
        Some("stream_event") => {
            let e = &m["event"];
            if e["type"] == "content_block_delta" && e["delta"]["type"] == "text_delta" && m["parent_tool_use_id"].is_null() {
                emit(app, "delta", json!({"pid": pid, "text": e["delta"]["text"]}));
            }
        }
        Some("assistant") => {
            for b in m["message"]["content"].as_array().into_iter().flatten() {
                match b["type"].as_str() {
                    Some("text") if m["parent_tool_use_id"].is_null() => line(app, pid, "claude", b["text"].as_str().unwrap_or("")),
                    Some("tool_use") if !b["name"].as_str().unwrap_or("").starts_with("mcp__clauracle__") => {
                        let sub = if m["parent_tool_use_id"].is_null() { "" } else { "↳ " };
                        // chat shows the one-liner; terminal view shows `detail` (the full input)
                        // id + parent: the Agents & Tasks panel pairs calls with results and subagent work with its agent
                        emit(app, "line", json!({"pid": pid, "kind": "tool", "detail": b["input"], "id": b["id"], "parent": m["parent_tool_use_id"],
                            "text": format!("{sub}{}  {}", b["name"].as_str().unwrap_or(""), summary(&b["input"]))}));
                    }
                    Some("thinking") => line(app, pid, "think", b["thinking"].as_str().unwrap_or("")),
                    _ => {}
                }
            }
        }
        Some("user") => {
            for b in m["message"]["content"].as_array().into_iter().flatten() {
                if b["type"] != "tool_result" {
                    continue;
                }
                let t = match &b["content"] {
                    Value::String(s) => s.clone(),
                    Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n"),
                    v => v.to_string(),
                };
                let kind = if b["is_error"] == true { "err" } else { "out" }; // out: terminal view only
                let mut t: String = t.chars().take(4000).collect();
                if kind == "err" {
                    t = t.chars().take(300).collect();
                }
                emit(app, "line", json!({"pid": pid, "kind": kind, "text": t, "result_of": b["tool_use_id"]}));
            }
        }
        Some("result") => {
            if let Some(sid) = m["session_id"].as_str() {
                st.with_project(pid, |p| p.session = Some(sid.into()));
            }
            if m["is_error"] == true {
                line(app, pid, "err", m["result"].as_str().unwrap_or("the turn ended with an error"));
            }
        }
        Some("rate_limit_event") => {
            // #111: spawn-time account, ≠ p.account ∵ a pool switch changes p.account before the reconnect
            let mut cfg = st.cfg.lock().unwrap();
            store::record_usage(&mut cfg, &acct, &m["rate_limit_info"]);
            store::save(&cfg);
            emit(app, "usage", json!({"account": acct, "usage": store::usage(&cfg, &acct)}));
        }
        Some("system") if m["subtype"] == "init" => {
            emit(app, "commands", json!({"pid": pid, "commands": m["slash_commands"]}));
        }
        _ => {}
    }
}

pub fn summary(input: &Value) -> String {
    for k in ["command", "file_path", "path", "pattern", "url", "query", "description", "prompt"] {
        if let Some(s) = input[k].as_str() {
            return s.lines().next().unwrap_or("").chars().take(160).collect();
        }
    }
    input.to_string().chars().take(160).collect()
}

// ---- after a turn: continue, hand over, switch account, or stop ----------------------------------

async fn after_turn(app: &AppHandle, st: &Arc<State>, pid: &str, live: &Arc<Live>, conn: &Conn) {
    let Some(p) = st.project(pid) else { return };
    if !p.gh.is_empty() {
        // commits Claude made → pushed in the background (user: no Push button chores); GitHub tab refetches on `pushed`
        let (app, pid, dir, gh) = (app.clone(), pid.to_string(), p.path.clone(), p.gh.clone());
        tauri::async_runtime::spawn_blocking(move || match crate::store::auto_push(&dir, &gh) {
            Ok(0) => {}
            Ok(n) => { line(&app, &pid, "note", &format!("Pushed {n} commit{}", if n > 1 { "s" } else { "" })); emit(&app, "pushed", json!({"pid": pid})); }
            Err(e) => line(&app, &pid, "note", &format!("Auto-push failed: {e}")),
        });
    }
    if live.armed.swap(false, SeqCst) {
        let was_handover = live.handing_over.swap(false, SeqCst);
        let mut p = p;
        let mut smart = false;
        if !was_handover && p.build.after == "smart" {
            // #89: build + run only when the diff since the last good smart build matters (code, big or a fix)
            let (dir, base) = (p.path.clone(), live.built.lock().unwrap().clone());
            let why = tauri::async_runtime::spawn_blocking(move || store::smart_why(&dir, &base)).await.unwrap_or_default();
            smart = !why.is_empty();
            if smart && p.build.build.is_empty() && p.build.run.is_empty() {
                let g = store::guess_build(&p.path); // unset steps → guessed once, saved so the Build tab shows them
                st.with_project(pid, |q| { q.build.build = g.build.clone(); q.build.run = g.run.clone(); if q.build.deploy.is_empty() { q.build.deploy = g.deploy.clone(); } });
                p = st.project(pid).unwrap_or(p);
                emit(app, "project", json!(p));
            }
            if smart && (p.build.build.is_empty() && p.build.run.is_empty()) {
                smart = false;
                line(app, pid, "note", "Smart build: no build/run step found; set one in the Build tab.");
            } else if smart {
                line(app, pid, "note", &format!("Smart build: {why} → build + run."));
            }
        }
        if !was_handover && (smart || !p.build.after.is_empty() && p.build.after != "smart") {
            // a task finished: build (and run/deploy) in plain code; a failure can go back to Claude
            let mut steps = vec!["build"];
            steps.extend(match p.build.after.as_str() { "build_run" | "smart" => vec!["run"], "build_deploy" => vec!["deploy"], _ => vec![] });
            let mut good = true;
            for step in steps {
                let (ok, tail) = run_step(app, st, pid, step).await;
                if !ok && p.build.fix {
                    line(app, pid, "note", &format!("{step} failed: sending the error to Claude."));
                    live.inbox.front(vec![Input::Msg(format!(
                        "The {step} step failed after your task (`{}`). Fix it, then call mcp__clauracle__ready again.\n```\n{tail}\n```",
                        step_cmd(&p, step)))]);
                    return;
                }
                if !ok {
                    good = false;
                    break;
                }
            }
            if smart && good {
                let dir = p.path.clone();
                let head = tauri::async_runtime::spawn_blocking(move || store::head(&dir)).await.unwrap_or_default();
                *live.built.lock().unwrap() = head;
            }
        }
        let auto = p.cont.starts_with("auto") || (p.cont == "off" && was_handover);
        st.with_project(pid, |p| p.ready = true);
        emit(app, "ready", json!({"pid": pid, "ready": true}));
        if !auto {
            if !p.questions.is_empty() {
                emit(app, "questions", json!({"pid": pid, "questions": p.questions}));
            }
            return; // manual: the Continue button takes it from here
        }
        return auto_continue(app, st, pid, live);
    }
    if live.handing_over.swap(false, SeqCst) {
        line(app, pid, "note", "No handover came back, so the context wasn't cleared.");
    }
    if p.cont == "off" && !p.questions.is_empty() {
        emit(app, "questions", json!({"pid": pid, "questions": p.questions}));
    }
    if let Ok(r) = conn.request(json!({"subtype": "get_context_usage"}), 15).await {
        let pct = r["percentage"].as_f64().unwrap_or(0.0);
        emit(app, "status", json!({"pid": pid, "context": pct.round(), "tokens": r["totalTokens"]}));
        if p.context_limit > 0.0 && pct >= p.context_limit && !live.first_turn.load(SeqCst) {
            prepare(app, live, pid, &format!(": the context window is at {}%", pct.round()));
        }
    }
    live.first_turn.store(false, SeqCst);
}

/// Auto modes: check the limits, switch to a pool account or stop, else start the next session.
fn auto_continue(app: &AppHandle, st: &Arc<State>, pid: &str, live: &Arc<Live>) {
    let p = st.project(pid).unwrap();
    let over = store::over_limit(&st.cfg.lock().unwrap(), &p.account);
    if let Some(window) = over {
        let next = store::pool_pick(&st.cfg.lock().unwrap(), &p.account);
        match next {
            Some(n) => {
                let nick = store::account_or_main(&st.cfg.lock().unwrap(), &n).nick.clone();
                // #111: keep = gh stays as set in the project, no "Keep current / Use these" suggestion
                st.with_project(pid, |p| { p.account = n.clone(); p.keep = true; });
                line(app, pid, "note", &format!("{} reached its {} limit: switched to {nick}.", p.account, label(&window)));
                emit(app, "project", json!(st.project(pid)));
            }
            None => {
                let msg = format!("Paused: {} is at its {} limit and no pool account has room.", p.account, label(&window));
                line(app, pid, "note", &msg);
                if !p.questions.is_empty() {
                    emit(app, "questions", json!({"pid": pid, "questions": p.questions}));
                }
                notify(app, st, &format!("{}: paused", p.name), &format!("{msg} {} question(s) waiting.", p.questions.len()));
                return; // ready stays: Continue resumes once there's room
            }
        }
    }
    // #9/#67: ANY open question pauses the run (auto only; auto_answer holds them by design) ∵ a clear while the card
    // is up = card gone + work goes on without the user → answering the last one resumes it
    if p.cont == "auto" && !p.questions.is_empty() {
        line(app, pid, "note", "Paused: a question needs you. Answering it carries on.");
        emit(app, "questions", json!({"pid": pid, "questions": p.questions}));
        return; // ready stays: answer_questions → continue_now
    }
    let msg = cont_text(&p); // ponytail: blocking gh call on a runtime worker (~1s, once per session)
    if msg.is_none() && p.answers.is_empty() {
        // empty Queue = done: no fresh "continue" turn ∵ it would only find nothing and stop (or invent work)
        line(app, pid, "note", "Queue is empty: all done. Send a new request to start again.");
        if !p.questions.is_empty() { // held auto_answer questions → show now, run is over
            emit(app, "questions", json!({"pid": pid, "questions": p.questions}));
        }
        notify(app, st, &format!("{}: done", p.name), &format!("The Queue is empty. {} question(s) waiting.", p.questions.len()));
        return; // ready stays: Continue still works if tasks get added
    }
    fresh(app, st, pid, live, Some(msg.unwrap_or_else(|| CONTINUE_BARE.into())));
}

fn label(window: &str) -> &str {
    match window { "five_hour" => "5-hour", "seven_day" => "weekly", w => w }
}

/// Clear the context: a new session, then (optionally) the continue message `cont` plus any answers.
pub fn fresh(app: &AppHandle, st: &State, pid: &str, live: &Live, cont: Option<String>) {
    live.armed.store(false, SeqCst);
    live.handing_over.store(false, SeqCst);
    live.first_turn.store(true, SeqCst);
    live.pin.store(cont.as_deref().map_or(0, pin_of), SeqCst);
    live.nagged.store(false, SeqCst);
    let answers = st.with_project(pid, |p| {
        if p.session.is_some() { p.prev_session = p.session.take(); }
        p.ready = false;
        std::mem::take(&mut p.answers)
    }).unwrap_or_default();
    emit(app, "cleared", json!({"pid": pid}));
    let mut items = vec![Input::Reconnect];
    if let Some(c) = cont {
        items.push(Input::Msg(if answers.is_empty() { c } else { format!("{c}\n\n{answers}") }));
    } else if !answers.is_empty() {
        items.push(Input::Msg(answers));
    }
    live.inbox.front(items);
}

pub fn prepare(app: &AppHandle, live: &Live, pid: &str, why: &str) {
    live.handing_over.store(true, SeqCst);
    line(app, pid, "note", "Writing the handover, then the context gets cleared.");
    live.inbox.front(vec![Input::Msg(PREPARE.replace("{why}", why))]);
}

pub fn notify(app: &AppHandle, st: &State, title: &str, body: &str) {
    let n = st.cfg.lock().unwrap().notify.clone();
    if n.desktop {
        use tauri_plugin_notification::NotificationExt;
        let _ = app.notification().builder().title(title).body(body).show();
    }
    let (t, b, topic) = (title.to_string(), body.to_string(), n.ntfy);
    std::thread::spawn(move || store::push_phone(&topic, &t, &b));
}

/// The user's answers, as text for Claude. answers: [{id, choice: [labels], note}]
pub fn answers_text(questions: &[Question], answers: &[Value]) -> String {
    let mut out = String::from("Answers to your questions:");
    for (i, q) in questions.iter().enumerate() {
        let Some(a) = answers.iter().find(|a| a["id"] == q.id.as_str()) else { continue };
        let choice: Vec<&str> = a["choice"].as_array().into_iter().flatten().filter_map(|c| c.as_str()).collect();
        let note = a["note"].as_str().unwrap_or("").trim();
        let mut s = format!("\n{}. {} → {}", i + 1, q.question, if choice.is_empty() { "(no choice)".into() } else { choice.join(", ") });
        if !note.is_empty() {
            s += &format!(" (note: {note})");
        }
        out += &s;
    }
    out
}

// ---- control requests from claude --------------------------------------------------------------

async fn control(app: &AppHandle, st: &Arc<State>, pid: &str, live: &Arc<Live>, conn: &Conn, v: Value) {
    let req = &v["request"];
    let id = v["request_id"].clone();
    let resp = match req["subtype"].as_str() {
        Some("can_use_tool") => Ok(permission(app, st, pid, req).await),
        // ui = async (waits on the page's reply) → handled here, not in sync mcp()
        Some("mcp_message") if req["message"]["params"]["name"] == "ui" && own_repo(st, pid) => {
            let m = &req["message"];
            let t = ui_tool(app, &m["params"]["arguments"]).await;
            Ok(json!({"mcp_response": {"jsonrpc": "2.0", "id": m["id"], "result": {"content": [{"type": "text", "text": t}]}}}))
        }
        Some("mcp_message") => Ok(json!({"mcp_response": mcp(app, st, pid, live, &req["message"])})),
        Some("hook_callback") => Ok(json!({})),
        s => Err(format!("unsupported control request {s:?}")),
    };
    let out = match resp {
        Ok(r) => json!({"type": "control_response", "response": {"subtype": "success", "request_id": id, "response": r}}),
        Err(e) => json!({"type": "control_response", "response": {"subtype": "error", "request_id": id, "error": e}}),
    };
    let _ = conn.write(&out).await;
}

async fn permission(app: &AppHandle, st: &Arc<State>, pid: &str, req: &Value) -> Value {
    let tool = req["tool_name"].as_str().unwrap_or("");
    let input = req["input"].clone();
    if tool.starts_with("mcp__clauracle__") {
        return json!({"behavior": "allow", "updatedInput": input});
    }
    let (cont, mode, allow) = st.project(pid).map(|p| (p.cont, p.mode, p.allow)).unwrap_or_default();
    if tool != "AskUserQuestion" && allowed(&allow, tool, &input, store::now() as u64) {
        return json!({"behavior": "allow", "updatedInput": input});
    }
    // the project's mode as set now: a switch mid-turn only reaches the CLI on reconnect, and CLI acceptEdits
    // still asks for files outside the project dir
    let edit = matches!(tool, "Edit" | "Write" | "MultiEdit" | "NotebookEdit");
    if mode == "bypassPermissions" && tool != "AskUserQuestion" || mode == "acceptEdits" && edit {
        return json!({"behavior": "allow", "updatedInput": input});
    }
    if tool == "AskUserQuestion" && cont != "off" {
        return json!({"behavior": "deny", "message": "Don't wait for the user: queue the questions with \
            mcp__clauracle__ask (they're answered later) and carry on with what doesn't depend on them."});
    }
    let ask_id = store::new_id();
    let (tx, rx) = oneshot::channel();
    st.asks.lock().unwrap().insert(ask_id.clone(), tx);
    emit(app, "ask", json!({"pid": pid, "id": ask_id, "tool": tool, "input": input}));
    let a = rx.await.unwrap_or(json!({"allow": false}));
    if a["allow"] != true {
        let note = a["note"].as_str().filter(|n| !n.is_empty()).unwrap_or("The user denied this.");
        return json!({"behavior": "deny", "message": note});
    }
    let mut input = input;
    if tool == "AskUserQuestion" {
        input["answers"] = a["answers"].clone();
    }
    if a["session"] == true { // "This session": CLI-side rule, gone when the CLI process ends (reconnect/clear)
        // rule(s) "Bash(grep:*)" → ruleContent "grep:*"; array = one per part of a compound command
        let list: Vec<&str> = a["rule"].as_array().map(|v| v.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_else(|| a["rule"].as_str().into_iter().collect());
        let rules: Vec<Value> = list.iter().map(|s| { let mut r = json!({"toolName": tool});
            if let Some(c) = s.strip_prefix(tool).and_then(|s| s.strip_prefix('(')?.strip_suffix(')')) { r["ruleContent"] = json!(c); } r }).collect();
        let rules = if rules.is_empty() { vec![json!({"toolName": tool})] } else { rules };
        return json!({"behavior": "allow", "updatedInput": input, "updatedPermissions": [{"type": "addRules",
            "rules": rules, "behavior": "allow", "destination": "session"}]});
    }
    json!({"behavior": "allow", "updatedInput": input})
}

/// Project allow rule: "Tool" or "Bash(prefix:*)", optional "@<unix secs>" expiry (For 24 hours).
/// Prefix rules never match shell operators ∵ "grep x && rm …" must still ask.
fn rule_hit(rule: &str, tool: &str, input: &Value, now: u64) -> bool {
    let (rule, until) = rule.rsplit_once('@').and_then(|(r, t)| Some((r, t.parse::<u64>().ok()?))).unwrap_or((rule, u64::MAX));
    if now >= until { return false; }
    match rule.strip_suffix(":*)").and_then(|r| r.split_once('(')) {
        None => rule == tool,
        Some((t, pre)) => t == tool && input["command"].as_str().is_some_and(|c| { let c = c.trim();
            (c == pre || c.starts_with(&format!("{pre} "))) && !c.contains(['&', '|', ';', '`', '$', '>', '<', '\n']) }),
    }
}

/// Any rule allows it; Bash compound (&& || ; | newline) → every part must hit some rule ("cd x; git y" = cd + git rules).
/// Parts with $ ` > < & (substitution/redirect/background) still never match a prefix rule (rule_hit).
fn allowed(allow: &[String], tool: &str, input: &Value, now: u64) -> bool {
    let hit = |inp: &Value| allow.iter().any(|r| rule_hit(r, tool, inp, now));
    if hit(input) { return true; }
    let Some(c) = input["command"].as_str().filter(|_| tool == "Bash") else { return false };
    let parts = split_cmd(c);
    parts.len() > 1 && parts.iter().all(|p| hit(&json!({"command": p})))
}

/// Parts at ; && || | newline outside quotes (= splitCmd in index.html): sed 's/a;b/' stays one part.
fn split_cmd(c: &str) -> Vec<String> {
    let (mut out, mut cur, mut q) = (vec![], String::new(), None::<char>);
    let mut it = c.chars().peekable();
    while let Some(ch) = it.next() {
        if let Some(qc) = q {
            cur.push(ch);
            if ch == qc { q = None } else if ch == '\\' && qc == '"' { cur.extend(it.next()) }
            continue;
        }
        match ch {
            '\\' => { cur.push(ch); cur.extend(it.next()); }
            '\'' | '"' => { q = Some(ch); cur.push(ch); }
            '&' if it.peek() == Some(&'&') => { it.next(); out.push(std::mem::take(&mut cur)); }
            '|' if it.peek() == Some(&'|') => { it.next(); out.push(std::mem::take(&mut cur)); }
            ';' | '|' | '\n' => out.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    out.push(cur);
    out.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

#[test]
fn rule_hit_works() {
    let a = |r: &[&str], c: &str| allowed(&r.iter().map(|s| s.to_string()).collect::<Vec<_>>(), "Bash", &json!({"command": c}), 0);
    assert!(a(&["Bash(cd:*)", "Bash(git:*)"], "cd D:/x; git status && git log | git y") && !a(&["Bash(cd:*)"], "cd x; rm -rf /"));
    assert!(!a(&["Bash(cd:*)", "Bash(git:*)"], "cd x; git $(rm y)") && !a(&["Bash(cd:*)"], "cd x & rm y"));
    assert_eq!(split_cmd("cd x; sed -i 's/a;b|c/' f && git log || echo \"x;y\""), ["cd x", "sed -i 's/a;b|c/' f", "git log", "echo \"x;y\""]);
    let i = json!({"command": "grep -c foo x.txt"});
    assert!(rule_hit("Bash", "Bash", &i, 0) && rule_hit("Bash(grep:*)", "Bash", &i, 0) && rule_hit("Bash(grep -c:*)", "Bash", &i, 0));
    assert!(!rule_hit("Bash(grep -v:*)", "Bash", &i, 0) && !rule_hit("Bash(gre:*)", "Bash", &i, 0) && !rule_hit("Edit", "Bash", &i, 0));
    assert!(!rule_hit("Bash(grep:*)", "Bash", &json!({"command": "grep x && rm -rf /"}), 0));
    assert!(rule_hit("Bash(grep:*)@100", "Bash", &i, 99) && !rule_hit("Bash(grep:*)@100", "Bash", &i, 100));
}

/// Project = Clauracle's own source → gets the `ui` self-test tool. ponytail: path sniff; ∵ any other project's
/// Claude clicking the app could approve its own permission cards.
fn own_repo(st: &Arc<State>, pid: &str) -> bool {
    st.project(pid).is_some_and(|p| std::fs::read_to_string(std::path::Path::new(&p.path).join("src-tauri/tauri.conf.json"))
        .is_ok_and(|s| s.contains("no.arctim.clauracle")))
}

pub static UI_WAIT: Mutex<Vec<(u64, tokio::sync::oneshot::Sender<String>)>> = Mutex::new(Vec::new());
static UI_N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Self-test: runs in the main webview (never the OS mouse): find by sel/text/x,y → click (or type), optional js,
/// then report the top open dialog/popover text (or `show` selector's). Page answers via `ui_reply`. Cards (permission/question) refused.
async fn ui_tool(app: &AppHandle, a: &Value) -> String {
    use tauri::Manager;
    let Some(w) = app.get_webview("main") else { return "No window.".into() }; // ≠ get_webview_window: None once the Web tab adds its child webview
    let id = UI_N.fetch_add(1, SeqCst);
    let (tx, rx) = tokio::sync::oneshot::channel();
    UI_WAIT.lock().unwrap().push((id, tx));
    let js = UI_JS.replace("ID", &id.to_string()).replace("ARGS", &a.to_string()); // ID first ∵ args may hold "ID"
    if let Err(e) = w.eval(&js) { return format!("eval failed: {e}"); }
    let r = tokio::time::timeout(std::time::Duration::from_secs(15), rx).await;
    UI_WAIT.lock().unwrap().retain(|(i, _)| *i != id);
    r.ok().and_then(|r| r.ok()).unwrap_or_else(|| "No reply from the page in 15s.".into())
}

const UI_JS: &str = r#"(async () => { const a = ARGS; let el = null, out = [];
try {
  if (a.sel) el = document.querySelector(a.sel);
  else if (a.text) { const t = a.text.toLowerCase(); el = [...document.querySelectorAll('button,a,summary,label,input,select,textarea,[onclick],[data-tip],[title],.ti,.proj,[tabindex]')]
    .filter(e => e.getClientRects().length).find(e => (e.innerText || e.value || e.dataset.tip || e.title || e.placeholder || '').trim().toLowerCase().includes(t)); }
  else if (a.x != null) el = document.elementFromPoint(a.x, a.y);
  if ((a.sel || a.text || a.x != null) && !el) out.push('Nothing matched.');
  else if (el && el.closest('.card.qs, .card:has([data-a])')) out.push('Refused: permission/question cards are the user\'s.');
  else if (el) { el.scrollIntoView({block: 'nearest'});
    if (a.type != null) { el.focus(); el.value = a.type; el.dispatchEvent(new Event('input', {bubbles: true})); el.dispatchEvent(new Event('change', {bubbles: true})); }
    else el[a.right ? 'dispatchEvent' : 'click'](new MouseEvent('contextmenu', {bubbles: true, clientX: el.getBoundingClientRect().x + 4, clientY: el.getBoundingClientRect().y + 4}));
    out.push((a.type != null ? 'Typed into: ' : 'Clicked: ') + el.outerHTML.slice(0, 200)); }
  if (a.js) { const v = await eval(a.js); out.push('js → ' + String(typeof v === 'string' ? v : JSON.stringify(v)).slice(0, 4000)); }
  await new Promise(r => setTimeout(r, a.wait ?? 400));
  const top = [...document.querySelectorAll('dialog[open], :popover-open')].filter(e => e.id !== 'tip').pop();
  const sh = a.show ? document.querySelector(a.show) : top; /* no #main dump by default ∵ chat log = token noise */
  if (sh) out.push(`${a.show ? 'Shown' : 'Open ' + (top.id || top.tagName)}: ` + sh.innerText.slice(0, 1500));
  else if (a.show) out.push('show: nothing matched');
} catch (e) { out.push('Error: ' + e); }
call('ui_reply', {id: ID, text: out.join('\n')}); })()"#;

fn tool_list(own: bool) -> Value {
    let q = json!({"type": "object", "required": ["questions"], "properties": {"questions": {"type": "array", "items": {
        "type": "object", "required": ["question", "options"], "properties": {
            "question": {"type": "string", "description": "the question only; Clauracle numbers them"},
            "options": {"type": "array", "items": {"type": "object", "required": ["label"], "properties": {
                "label": {"type": "string"}, "description": {"type": "string"},
                "recommended": {"type": "boolean", "description": "your suggestion; at most one per question"}}}},
            "multi": {"type": "boolean", "description": "more than one option may be picked"},
            "blocking": {"type": "boolean", "description": "true only if work truly can't go on without the user"}}}}}});
    let mut t = json!({"tools": [
        {"name": "ready", "description": "Call as the LAST action of a turn once the task is done (or paused) and \
          the task list and HANDOVER.md are updated. When the turn ends Clauracle clears the context and a fresh session continues \
          (or waits for the user's Continue).", "inputSchema": {"type": "object", "properties": {}}},
        {"name": "ask", "description": "Queue questions for the user (numbered in a card, options with your \
          recommendation, a note field each). Don't wait: the answers arrive later.", "inputSchema": q},
        {"name": "lesson", "description": "Record a never-again rule after a mistake that cost time (yours or a tool's \
          quirk), so every future session avoids it. One line, compressed: fragments, symbols (→ = ≠ ∵), ≤ 120 chars. \
          Not for task notes or project facts.", "inputSchema": {"type": "object", "required": ["rule"],
          "properties": {"rule": {"type": "string"}}}},
        {"name": "restart", "description": "Restart Clauracle itself, e.g. to try a new build of it. Nothing is lost: \
          every session resumes, this turn continues after the restart, unsent input stays. `exe`: a newly built \
          Clauracle executable: it replaces the running app's exe in place (same path), then starts.",
         "inputSchema": {"type": "object", "properties": {"exe": {"type": "string"}}}}]});
    if own {
        t["tools"].as_array_mut().unwrap().push(json!({"name": "ui", "description": "Self-test Clauracle's own window \
          (inside the page, not the OS mouse). Pick ONE target: `sel` (CSS selector), `text` (visible text/tooltip of a \
          clickable, case-insensitive substring) or `x`,`y` (CSS px). Default = click; `type` = set an input's value; \
          `right` = right-click. `js` = expression evaluated after (await-ed; page globals like S, cur, P() visible). \
          Returns what was hit + the open dialog/popover text after `wait` ms (400); `show` = CSS selector whose text \
          to return instead (e.g. \"#right\"). \
          Permission/question cards are refused.", "inputSchema": {"type": "object", "properties": {
            "sel": {"type": "string"}, "text": {"type": "string"}, "x": {"type": "number"}, "y": {"type": "number"},
            "type": {"type": "string"}, "right": {"type": "boolean"}, "js": {"type": "string"}, "wait": {"type": "number"}, "show": {"type": "string"}}}}));
    }
    t
}

/// What to start: this exe, or a new build as its own swap helper (`--swap-into` this exe's path), so the app's
/// path never changes (pinnable) ∵ Windows locks a running exe: the helper waits for us to exit, then replaces it.
fn restart_target(exe: &str) -> Result<(std::path::PathBuf, Vec<String>), String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    if exe.is_empty() {
        return Ok((me, vec![]));
    }
    let src = std::fs::canonicalize(exe).map_err(|_| format!("{exe} isn't a file"))?;
    if !src.is_file() {
        return Err(format!("{exe} isn't a file"));
    }
    if std::fs::canonicalize(&me).is_ok_and(|m| m == src) {
        return Ok((me, vec![])); // already running that build
    }
    Ok((src, vec!["--swap-into".into(), me.to_string_lossy().into_owned()]))
}

/// At startup: turns that were running when Clauracle went down (crash, restart) pick up where they were.
pub fn resume_interrupted(app: &AppHandle, st: &Arc<State>) {
    let cut: Vec<String> = st.cfg.lock().unwrap().projects.iter().filter(|p| p.working && p.session.is_some())
        .map(|p| p.id.clone()).collect();
    for pid in cut {
        session(app, st, &pid).inbox.push(Input::Msg(RESUMED.into()));
        line(app, &pid, "note", "Clauracle restarted mid-turn: resuming");
    }
}

fn mcp(app: &AppHandle, st: &Arc<State>, pid: &str, live: &Arc<Live>, m: &Value) -> Value {
    let id = m["id"].clone();
    let ok = |result: Value| json!({"jsonrpc": "2.0", "id": id, "result": result});
    let text = |t: &str| ok(json!({"content": [{"type": "text", "text": t}]}));
    match m["method"].as_str() {
        Some("initialize") => ok(json!({"protocolVersion": m["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
            "capabilities": {"tools": {}}, "serverInfo": {"name": "clauracle", "version": "1.0.0"}})),
        Some("tools/list") => ok(tool_list(own_repo(st, pid))),
        Some("tools/call") => match m["params"]["name"].as_str() {
            Some("ready") => {
                let p = st.project(pid).unwrap();
                if p.cont == "off" && !live.handing_over.load(SeqCst) {
                    return text("Clear-continue is off for this project: nothing will be cleared. Just end the turn.");
                }
                // #83: finished = pinned issue closed (code decides, not the AI) ∵ ready on an open issue → clear →
                // Continue re-picks it → #62/#75 loop. Handover pauses (context limit) pass; one refusal, then trust.
                let n = live.pin.load(SeqCst);
                if n > 0 && !live.handing_over.load(SeqCst) && !live.nagged.swap(true, SeqCst) {
                    let s = store::gh_json(&["issue", "view", &n.to_string(), "--json", "state"], &p.gh, Some(&p.path));
                    if s["state"] == "OPEN" { // ponytail: blocking gh (~1s) on the reader; gh failing → let ready through
                        return text(&format!("Not armed: issue #{n} (this session's task) is still open, so the next \
                          session would pick it again. If it's done: verify, `gh issue close {n} -c <summary>`. If it's too big: \
                          file the rest as new issues, then close #{n}. Then call ready again. (If it truly can't be closed \
                          yet, e.g. waiting on the user, call ready again and say why.)"));
                    }
                }
                live.armed.store(true, SeqCst);
                // watchdog: work is done once ready is called; only the closing summary is left. A stalled API reply
                // (seen: 3+ min, no output) would hold the Working pill forever → interrupt after 30s, the turn ends.
                let l = live.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    let conn = l.conn.lock().unwrap().clone();
                    if let (true, true, Some(c)) = (l.armed.load(SeqCst), l.busy.load(SeqCst), conn) {
                        let _ = c.request(json!({"subtype": "interrupt"}), 10).await;
                    }
                });
                text("Armed. When this turn ends the context is cleared and a fresh session continues from the task list and HANDOVER.md. End the turn now.")
            }
            Some("lesson") => {
                let rule: String = m["params"]["arguments"]["rule"].as_str().unwrap_or("").trim().chars().take(160).collect();
                if rule.is_empty() { return text("Empty rule: nothing saved."); }
                let mut cfg = st.cfg.lock().unwrap();
                if !cfg.lessons.contains(&rule) {
                    cfg.lessons.push(rule);
                    let n = cfg.lessons.len();
                    if n > LESSONS_MAX { cfg.lessons.drain(..n - LESSONS_MAX); } // oldest go first
                    store::save(&cfg);
                }
                text("Saved. Sessions started from now on get it.")
            }
            Some("restart") => {
                let exe = m["params"]["arguments"]["exe"].as_str().unwrap_or("").to_string();
                match restart_target(&exe) {
                    Ok((target, args)) => {
                        // page holds new prompts in its (saved) queue from now on ∵ one sent in the next 800ms reached
                        // the CLI, which was killed before writing it to the transcript → lost after the restart
                        emit(app, "restarting", json!({}));
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(800)).await; // let this reply land
                            crate::relaunch(&app, target, &args);
                        });
                        text("Restarting Clauracle now. This turn resumes after the restart; end it here.")
                    }
                    Err(e) => text(&format!("Can't restart: {e}")),
                }
            }
            Some("ask") => {
                let mut qs: Vec<Question> = serde_json::from_value(m["params"]["arguments"]["questions"].clone()).unwrap_or_default();
                for q in qs.iter_mut() {
                    q.id = store::new_id() + &q.question.len().to_string();
                }
                let cont = st.with_project(pid, |p| {
                    p.questions.extend(qs.clone());
                    p.cont.clone()
                }).unwrap_or_default();
                let name = st.project(pid).map(|p| p.name).unwrap_or_default();
                emit(app, "queued", json!({"pid": pid, "count": st.project(pid).map(|p| p.questions.len())}));
                match cont.as_str() {
                    "auto" => {
                        emit(app, "questions", json!({"pid": pid, "questions": st.project(pid).map(|p| p.questions)}));
                        notify(app, st, &format!("{name}: questions"), &format!("{} question(s) need your answer.", qs.len()));
                        text("Queued and the user was notified. Finish the current task where it doesn't depend on them; the run pauses after this turn (no clear) until they answer.")
                    }
                    "auto_answer" => text("Held until the user is back. Carry on with other tasks."),
                    _ => text("Queued: the user sees them after the next Continue (never blocks it). Finish what doesn't depend on them first."),
                }
            }
            _ => json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "unknown tool"}}),
        },
        _ if m.get("id").is_none() => json!({"jsonrpc": "2.0", "result": {}}), // a notification: just ack
        _ => json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "method not found"}}),
    }
}

// ---- build / run / deploy: plain commands, output streamed to the Build panel ------------------------

fn step_cmd(p: &Project, step: &str) -> String {
    match step { "build" => &p.build.build, "run" => &p.build.run, _ => &p.build.deploy }.clone()
}

fn shell(cmd: &str) -> tokio::process::Command {
    let mut c = if cfg!(windows) {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(cmd);
        c
    } else {
        let mut c = tokio::process::Command::new("sh");
        c.arg("-c").arg(cmd);
        c
    };
    #[cfg(windows)]
    c.creation_flags(0x0800_0000);
    c
}

/// Runs one step. `run` stays up (a dev server or the app): it's started, not awaited, and replaces the last one.
/// Returns (ok, last 40 lines of output).
pub async fn run_step(app: &AppHandle, st: &Arc<State>, pid: &str, step: &str) -> (bool, String) {
    let Some(p) = st.project(pid) else { return (false, String::new()) };
    let cmd = step_cmd(&p, step);
    if cmd.trim().is_empty() {
        return (true, String::new());
    }
    let say = |t: &str, done: Option<bool>| emit(app, "build", json!({"pid": pid, "step": step, "line": t, "done": done}));
    let sign = store::sign_env(&p.path);
    let mut cmds = vec![(cmd.clone(), shell(&cmd), None::<String>)];
    // #131: Windows + Linux target + WSL → 2nd pass in WSL (args set by Rust, no cmd.exe quoting); WSLENV forwards signing keys
    if cfg!(windows) && step == "build" && p.targets.iter().any(|t| t == "linux") && store::wsl_ok() {
        let mut w = tokio::process::Command::new("wsl");
        w.args(["-e", "bash", "-lc", &store::wsl_script(pid, &cmd)])
            .env("WSLENV", sign.iter().map(|(k, _)| k.as_str()).chain(std::env::var("WSLENV").ok().as_deref()).collect::<Vec<_>>().join(":"));
        #[cfg(windows)]
        w.creation_flags(0x0800_0000);
        cmds.push((format!("wsl: {cmd} (Linux)"), w, None));
    }
    // #149: non-Mac host + macos target + Network Mac → sync tree, build over ssh (script + #150 signing env on stdin), pull bundles
    let mac = st.cfg.lock().unwrap().mac.clone();
    let remote = !cfg!(target_os = "macos") && step == "build" && p.targets.iter().any(|t| t == "macos") && !mac.host.trim().is_empty();
    if remote {
        let mut c = tokio::process::Command::new("ssh");
        c.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=8", "-o", "StrictHostKeyChecking=accept-new",
            &if mac.user.trim().is_empty() { mac.host.trim().to_string() } else { format!("{}@{}", mac.user.trim(), mac.host.trim()) }, "zsh -l -s"]);
        #[cfg(windows)]
        c.creation_flags(0x0800_0000);
        cmds.push((format!("ssh {}: {cmd} (macOS)", mac.host.trim()), c, Some(store::mac_script(&mac, pid, &cmd, &sign))));
    }
    let mut tail: VecDeque<String> = VecDeque::new();
    let mut ok = true;
    for (shown, mut c, input) in cmds {
    say(&format!("$ {shown}"), None);
    if input.is_some() { // the Mac pass: copy the tree over first
        say(&format!("syncing source → {}:{}", mac.host.trim(), store::mac_dir(&mac, pid)), None);
        let (m2, pid2, dir2) = (mac.clone(), pid.to_string(), p.path.clone());
        if let Err(e) = tauri::async_runtime::spawn_blocking(move || store::mac_sync(&m2, &pid2, &dir2)).await.unwrap_or(Err("sync panicked".into())) {
            say(&format!("sync failed: {e}"), None);
            ok = false;
            tail.push_back(format!("Mac sync failed: {e}"));
            break;
        }
    }
    c.current_dir(&p.path).envs(sign.iter().cloned()).envs(store::gh_env(&p.gh)).envs(store::gcloud_env(&p.gaccount, &p.gcp))
        .stdin(if input.is_some() { std::process::Stdio::piped() } else { std::process::Stdio::null() })
        .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    let mut child = match c.spawn() {
        Ok(mut c) => {
            c.id().map(crate::track);
            crate::adopt(&c);
            if let (Some(t), Some(mut i)) = (&input, c.stdin.take()) {
                let t = t.clone();
                tauri::async_runtime::spawn(async move { let _ = i.write_all(t.as_bytes()).await; }); // drop → EOF → script runs
            }
            c
        }
        Err(e) => {
            say(&e.to_string(), Some(false));
            return (false, e.to_string());
        }
    };
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    for out in [child.stdout.take().map(|o| Box::new(o) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
                child.stderr.take().map(|o| Box::new(o) as Box<dyn tokio::io::AsyncRead + Unpin + Send>)].into_iter().flatten() {
        let tx = tx.clone();
        tauri::async_runtime::spawn(async move {
            let mut l = BufReader::new(out).lines();
            while let Ok(Some(s)) = l.next_line().await {
                let _ = tx.send(s);
            }
        });
    }
    drop(tx);
    if step == "run" {
        let (app2, pid2) = (app.clone(), pid.to_string());
        static GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let gen = GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let old = RUNNING.lock().unwrap().insert(pid.to_string(), (gen, child));
        if let Some((_, c)) = old { kill(c); } // replace = stop the previous run's whole tree
        tauri::async_runtime::spawn(async move {
            while let Some(s) = rx.recv().await {
                emit(&app2, "build", json!({"pid": pid2, "step": "run", "line": s, "done": null}));
            }
            // output closed = app exited (or stopped) → UI swaps Stop back to Run; skip if a newer run replaced this one
            let mut r = RUNNING.lock().unwrap();
            if r.get(&pid2).map_or(true, |(g, _)| *g == gen) {
                r.remove(&pid2);
                drop(r);
                emit(&app2, "build", json!({"pid": pid2, "step": "run", "line": null, "done": true}));
            }
        });
        return (true, String::new());
    }
    while let Some(s) = rx.recv().await {
        say(&s, None);
        tail.push_back(s);
        if tail.len() > 40 {
            tail.pop_front();
        }
    }
    ok = child.wait().await.map(|s| s.success()).unwrap_or(false);
    if ok && input.is_some() {
        say("pulling Mac bundles back", None);
        let (m2, pid2, dir2) = (mac.clone(), pid.to_string(), p.path.clone());
        if let Err(e) = tauri::async_runtime::spawn_blocking(move || store::mac_pull(&m2, &pid2, &dir2)).await.unwrap_or(Err("pull panicked".into())) {
            say(&format!("pull failed: {e}"), None);
            ok = false;
        }
    }
    if !ok { break; }
    }
    say(if ok { "✓ done" } else { "✗ failed" }, Some(ok));
    (ok, tail.into_iter().collect::<Vec<_>>().join("\n"))
}

pub fn stop_run(pid: &str) -> bool {
    let c = RUNNING.lock().unwrap().remove(pid);
    c.map(|(_, c)| kill(c)).is_some()
}

/// kill_on_drop only stops the `cmd /C` shell; the app it started (npm → node → cargo → app) kept running → tree kill.
fn kill(c: tokio::process::Child) {
    if let Some(id) = c.id() { crate::kill_tree(id); }
}

static RUNNING: std::sync::LazyLock<Mutex<HashMap<String, (u64, tokio::process::Child)>>> =
    std::sync::LazyLock::new(Default::default);

// ---- history of a resumed session ------------------------------------------------------------------

/// Files in the project root that may hold tasks; PLAN.md = the single source once it exists.
const PLAN_FILES: [&str; 4] = ["PLAN.md", "HANDOVER.md", "TASKS.md", "TODO.md"];

/// Open tasks of a plan file, in order: its "## Queue" list if it has one; else (not HANDOVER.md: its bullets are
/// state, not tasks) every top-level list item that isn't checked off.
/// `wide` (plans() only): HANDOVER.md open-work headings count too; queue() stays Queue-only (Continue text).
fn items(dir: &str, file: &str, wide: bool) -> Vec<String> {
    let text = std::fs::read_to_string(std::path::Path::new(dir).join(file)).unwrap_or_default();
    let item = regex::Regex::new(r"^(\s*)(?:\d+[.)]|[-*])\s+(?:\[ \]\s*)?(.+)").unwrap();
    // HANDOVER.md: its bullets = state, but open-work sections (#58: follow-ups that should be issues) count as tasks
    let ho = wide && file == "HANDOVER.md";
    let open = regex::Regex::new(r"(?i)^(queue|to-?do|tasks?|next( steps)?|open( items| questions)?|follow.?ups?|waiting on the user|not (yet )?verified( live)?)\b").unwrap();
    let is_q = |l: &str| { let h = l.trim_start_matches('#').trim(); if ho { open.is_match(h) } else { h.eq_ignore_ascii_case("queue") } };
    let has_q = text.lines().any(|l| l.starts_with('#') && is_q(l));
    if !has_q && file == "HANDOVER.md" {
        return vec![];
    }
    let mut on = !has_q;
    let mut out: Vec<String> = vec![];
    let mut last = false; // previous line was a kept item → indented lines continue it
    for l in text.lines() {
        if l.starts_with('#') {
            last = false;
            if has_q { on = is_q(l); } // non-HANDOVER exact: "## Waiting (not queue items)" ≠ queue
        } else if on {
            match item.captures(l) {
                Some(c) if (has_q || c[1].is_empty()) && !c[2].starts_with("[x]") && !c[2].starts_with("[X]") =>
                    { out.push(c[2].replace("**", "").trim().to_string()); last = true; }
                Some(_) => last = false,
                // indented wrap line of the item above → same task (was dropped → tasks cut mid-sentence)
                None if last && l.starts_with([' ', '\t']) && !l.trim().is_empty() => {
                    let t = out.last_mut().unwrap(); t.push(' '); t.push_str(l.trim().replace("**", "").as_str());
                }
                None => last = false,
            }
        }
    }
    out
}

/// The plan Clauracle works from: PLAN.md if it exists, else an old HANDOVER.md's Queue (migrated by Claude).
fn primary(dir: &str) -> &'static str {
    if std::path::Path::new(dir).join("PLAN.md").is_file() { "PLAN.md" } else { "HANDOVER.md" }
}

/// Next tasks (ready bar, Tasks tab): the primary plan's open items.
pub fn queue(dir: &str) -> Vec<String> {
    items(dir, primary(dir), false)
}

/// Plan tab drag: moves open Queue task `from` to slot `to` (indexes as `queue()` gives them) in the primary plan.
/// Block = top-level item + following lines (wraps, sub-items) → multi-line tasks move whole; numbered items renumbered
/// from the first number. ponytail: index → owning top-level block, so an indented sub-item drags its parent.
/// `to` = None → block removed (Plan tab tick = task done).
/// `want` = task text the UI showed → refuse if Claude changed PLAN.md meanwhile (else a stale index hits another task).
pub fn queue_move(dir: &str, from: usize, to: Option<usize>, want: Option<&str>) -> Result<(), String> {
    if want.is_some_and(|w| queue(dir).get(from).map(|s| s.as_str()) != Some(w)) {
        return Err("PLAN.md changed meanwhile: try again".into());
    }
    let path = std::path::Path::new(dir).join(primary(dir));
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let lines: Vec<&str> = text.lines().collect();
    let head = |l: &str| l.starts_with('#') && l.trim_start_matches('#').trim().eq_ignore_ascii_case("queue");
    let s = lines.iter().position(|l| head(l)).ok_or("no ## Queue section")? + 1;
    let e = lines[s..].iter().position(|l| l.starts_with('#')).map_or(lines.len(), |i| s + i);
    let item = regex::Regex::new(r"^(\s*)(?:\d+[.)]|[-*])\s+(?:\[ \]\s*)?(.+)").unwrap();
    let starts: Vec<usize> = (s..e).filter(|&i| item.captures(lines[i]).map_or(false, |c| c[1].is_empty())).collect();
    // open item k → its block (same filter as items(): [x] skipped)
    let opens: Vec<usize> = (s..e).filter(|&i| item.captures(lines[i]).is_some_and(|c| !c[2].starts_with("[x]") && !c[2].starts_with("[X]"))).collect();
    let owner: Vec<Option<usize>> = opens.iter().map(|&i| starts.iter().rposition(|&b| b <= i)).collect();
    let Some(Some(fb)) = owner.get(from).copied() else { return Err("task not found".into()) };
    let fi = opens[from];
    if to.is_none() && fi != starts[fb] {
        // ticked sub-item → only it (+ its deeper wrap lines) goes, not the parent block
        let ind = lines[fi].len() - lines[fi].trim_start().len();
        let mut j = fi + 1;
        while j < e && !lines[j].trim().is_empty() && !item.is_match(lines[j]) && lines[j].len() - lines[j].trim_start().len() > ind { j += 1; }
        let out: Vec<&str> = lines[..fi].iter().chain(&lines[j..]).copied().collect();
        return std::fs::write(&path, out.join(nl) + if text.ends_with('\n') { nl } else { "" }).map_err(|e| e.to_string());
    }
    let tb = match to { None => None, Some(t) => match owner.get(t) { Some(Some(b)) => Some(*b), _ => return Err("task not found".into()) } };
    if tb == Some(fb) { return Ok(()); }
    let mut tail_from = e; // trailing blank lines stay at the section's end, not with the last block
    while tail_from > starts[0] && lines[tail_from - 1].trim().is_empty() { tail_from -= 1; }
    let mut blocks: Vec<Vec<String>> = starts.iter().enumerate()
        .map(|(k, &b)| lines[b..starts.get(k + 1).copied().unwrap_or(tail_from).max(b + 1)].iter().map(|l| l.to_string()).collect()).collect();
    let b = blocks.remove(fb);
    if let Some(tb) = tb { blocks.insert(tb, b); }
    let num = regex::Regex::new(r"^(\d+)([.)]\s)").unwrap();
    let mut n = starts.iter().find_map(|&i| num.captures(lines[i]).and_then(|c| c[1].parse::<usize>().ok()));
    for bl in blocks.iter_mut() {
        if let (Some(k), true) = (n, num.is_match(&bl[0])) { bl[0] = num.replace(&bl[0], format!("{k}$2")).into_owned(); n = Some(k + 1); }
    }
    let out: Vec<String> = lines[..starts[0]].iter().map(|l| l.to_string()).chain(blocks.into_iter().flatten())
        .chain(lines[tail_from..].iter().map(|l| l.to_string())).collect();
    std::fs::write(&path, out.join(nl) + if text.ends_with('\n') { nl } else { "" }).map_err(|e| e.to_string())
}

/// Section after the heading `head`: (first line, end without trailing blanks, top-level item line indexes).
fn sect(lines: &[String], head: fn(&str) -> bool) -> Option<(usize, usize, Vec<usize>)> {
    let s = lines.iter().position(|l| head(l))? + 1;
    let mut e = lines[s..].iter().position(|l| l.starts_with('#')).map_or(lines.len(), |i| s + i);
    while e > s && lines[e - 1].trim().is_empty() { e -= 1; }
    let item = regex::Regex::new(r"^(?:\d+[.)]|[-*])\s").unwrap();
    Some((s, e, (s..e).filter(|&i| item.is_match(&lines[i])).collect()))
}
fn wait_head(l: &str) -> bool { l.starts_with('#') && l.trim_start_matches('#').trim().to_ascii_lowercase().starts_with("waiting") }
fn queue_head(l: &str) -> bool { l.starts_with('#') && l.trim_start_matches('#').trim().eq_ignore_ascii_case("queue") }
const MARK: &str = r"^(?:\d+[.)]|[-*])\s+(?:\[ \]\s*)?";
fn plan_lines(dir: &str) -> (std::path::PathBuf, String, Vec<String>) {
    let path = std::path::Path::new(dir).join(primary(dir));
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let lines = text.lines().map(String::from).collect();
    (path, text, lines)
}

/// "## Waiting on the user" items of the primary plan (Plan tab suggestions): one string per top-level block.
pub fn waiting(dir: &str) -> Vec<String> {
    let (_, _, lines) = plan_lines(dir);
    let Some((_, e, st)) = sect(&lines, wait_head) else { return vec![] };
    let mark = regex::Regex::new(MARK).unwrap();
    st.iter().enumerate().map(|(k, &b)| {
        let t = lines[b..st.get(k + 1).copied().unwrap_or(e)].iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ");
        mark.replace(&t, "").replace("**", "")
    }).collect()
}

/// Suggestion i out of "Waiting on the user": accept → appended to the Queue (numbered if the Queue is), else dropped.
pub fn wait_take(dir: &str, i: usize, accept: bool) -> Result<(), String> {
    let (path, text, mut lines) = plan_lines(dir);
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let (_, e, st) = sect(&lines, wait_head).ok_or("no Waiting section")?;
    let b = *st.get(i).ok_or("item not found")?;
    let mut blk: Vec<String> = lines.drain(b..st.get(i + 1).copied().unwrap_or(e)).collect();
    if accept {
        let (_, e, qs) = sect(&lines, queue_head).ok_or("no ## Queue section")?;
        let num = regex::Regex::new(r"^(\d+)[.)]\s").unwrap();
        let m = qs.last().and_then(|&l| num.captures(&lines[l]).and_then(|c| c[1].parse::<usize>().ok())).map_or("- ".into(), |n| format!("{}. ", n + 1));
        blk[0] = format!("{m}{}", regex::Regex::new(MARK).unwrap().replace(&blk[0], ""));
        lines.splice(e..e, blk);
    }
    std::fs::write(&path, lines.join(nl) + if text.ends_with('\n') { nl } else { "" }).map_err(|e| e.to_string())
}

/// Every plan file with open tasks, primary first: [{file, items}] (Tasks tab ⓘ, merge into PLAN.md).
pub fn plans(dir: &str) -> Vec<Value> {
    let p = primary(dir);
    std::iter::once(p).chain(PLAN_FILES.into_iter().filter(|f| *f != p))
        .map(|f| (f, items(dir, f, true))).filter(|(f, i)| *f == p || !i.is_empty())
        .map(|(f, i)| json!({"file": f, "items": i})).collect()
}

/// The visible part of a session's transcript (~/.claude/projects/*/<id>.jsonl), last 300 entries.

/// The newest `n` entries of CHANGES.md (newest first, one bullet per change): what was done lately.
pub fn done(dir: &str, n: usize) -> Vec<String> {
    let text = std::fs::read_to_string(std::path::Path::new(dir).join("CHANGES.md")).unwrap_or_default();
    text.lines().filter_map(|l| l.trim_start().strip_prefix("- ")).take(n)
        .map(|l| l.split_once(": ").filter(|(d, _)| d.len() <= 12).map_or(l, |(_, rest)| rest).to_string()).collect() // drop a "2026-11-04: " prefix
}

pub fn history(session: &str) -> Vec<Value> {
    let root = dirs::home_dir().unwrap_or_default().join(".claude/projects");
    let file = std::fs::read_dir(root).into_iter().flatten().flatten()
        .map(|d| d.path().join(format!("{session}.jsonl"))).find(|f| f.is_file());
    let Some(file) = file else { return vec![] };
    let mut out = vec![];
    let prep = PREPARE.split('{').next().unwrap_or(PREPARE);
    let mut hide = false; // inside a handover turn
    for l in std::fs::read_to_string(file).unwrap_or_default().lines() {
        let Ok(m) = serde_json::from_str::<Value>(l) else { continue };
        if m["isMeta"] == true || m["isSidechain"] == true {
            continue;
        }
        let content = &m["message"]["content"];
        match m["type"].as_str() {
            Some("user") => {
                if let Some(t) = content.as_str() {
                    hide = t.starts_with(prep);
                    if hide || t.starts_with('<') {
                        continue;
                    }
                    out.push(if t.starts_with(&CONTINUE[..60]) { // prefix: older CONTINUE wordings too
                        json!({"kind": "note", "text": "Fresh session, continued from the plan"})
                    } else if t.starts_with(RESUMED) {
                        json!({"kind": "note", "text": "Resumed after a restart"})
                    } else {
                        json!({"kind": "user", "text": t, "ts": m["timestamp"]})
                    });
                }
            }
            Some("assistant") if !hide => {
                for b in content.as_array().into_iter().flatten() {
                    match b["type"].as_str() {
                        Some("text") => out.push(json!({"kind": "claude", "text": b["text"], "ts": m["timestamp"]})),
                        Some("tool_use") if !b["name"].as_str().unwrap_or("").starts_with("mcp__clauracle__") => out.push(
                            json!({"kind": "tool", "detail": b["input"], "id": b["id"], "text": format!("{}  {}", b["name"].as_str().unwrap_or(""), summary(&b["input"]))})),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    let skip = out.len().saturating_sub(300); // ponytail: the last 300 entries only
    out.split_off(skip)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_move_keeps_blocks_and_renumbers() {
        let d = std::env::temp_dir().join("clauracle-qmove-test");
        let _ = std::fs::create_dir_all(&d);
        let p = d.join("PLAN.md");
        std::fs::write(&p, "# Plan\n\n## Queue\n0. A first\n   A wrap\n3. B\n- [x] gone\n4. C\n\n## Waiting\n- W\n").unwrap();
        let dir = d.to_str().unwrap();
        queue_move(dir, 0, Some(2), Some("A first A wrap")).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "# Plan\n\n## Queue\n0. B\n- [x] gone\n1. C\n2. A first\n   A wrap\n\n## Waiting\n- W\n");
        assert_eq!(queue(dir), ["B", "C", "A first A wrap"]);
        assert!(queue_move(dir, 2, Some(0), Some("B")).is_err()); // stale text → refused
        queue_move(dir, 2, Some(0), None).unwrap();
        assert_eq!(queue(dir), ["A first A wrap", "B", "C"]);
        queue_move(dir, 0, None, None).unwrap(); // tick = done → block gone, renumbered
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "# Plan\n\n## Queue\n0. B\n- [x] gone\n1. C\n\n## Waiting\n- W\n");
        std::fs::write(&p, "## Queue\n1. X\n   - a\n     a wrap\n   - b\n2. Y\n").unwrap();
        queue_move(dir, 1, None, None).unwrap(); // tick sub-item → only it goes
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "## Queue\n1. X\n   - b\n2. Y\n");
    }

    #[test]
    fn waiting_accept_and_dismiss() {
        let d = std::env::temp_dir().join("clauracle-wait-test");
        let _ = std::fs::create_dir_all(&d);
        let p = d.join("PLAN.md");
        std::fs::write(&p, "## Queue\n1. A\n\n## Waiting on the user (yes first)\n- **W1** first\n  wrap\n- W2\n").unwrap();
        let dir = d.to_str().unwrap();
        assert_eq!(waiting(dir), ["W1 first wrap", "W2"]);
        wait_take(dir, 0, true).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "## Queue\n1. A\n2. **W1** first\n  wrap\n\n## Waiting on the user (yes first)\n- W2\n");
        wait_take(dir, 0, false).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "## Queue\n1. A\n2. **W1** first\n  wrap\n\n## Waiting on the user (yes first)\n");
        assert!(waiting(dir).is_empty());
    }

    #[test]
    fn done_reads_newest_changes() {
        let d = std::env::temp_dir().join("clauracle-done-test");
        let _ = std::fs::create_dir_all(&d);
        std::fs::write(d.join("CHANGES.md"), "# Changes\n\n- 2026-11-04: Newest: thing\n- Older\n- Oldest\n- Gone\n").unwrap();
        assert_eq!(done(d.to_str().unwrap(), 3), ["Newest: thing", "Older", "Oldest"]);
    }

    #[test]
    fn handover_queue() {
        let d = std::env::temp_dir().join("clauracle-queue-test");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("HANDOVER.md"), "# Handover\n## Where\n- not this\n## Queue\n1. **Add** login\n2) Tests\n   detail line\n- Docs\n## Gotchas\n- no\n").unwrap();
        assert_eq!(queue(d.to_str().unwrap()), ["Add login", "Tests detail line", "Docs"]);
        assert!(queue("/nope").is_empty());
        std::fs::write(d.join("HANDOVER.md"), "## Queue\n- A\n## Waiting on the user (not queue items)\n- B\n").unwrap();
        assert_eq!(queue(d.to_str().unwrap()), ["A"]);
        // TODO.md without a Queue heading: top-level unchecked items; PLAN.md takes over as primary once it exists
        std::fs::write(d.join("TODO.md"), "# Todo\n- [ ] One\n  - sub\n- [x] Done\n* Two\n").unwrap();
        std::fs::write(d.join("PLAN.md"), "## Queue\n1. P\n").unwrap();
        let ds = d.to_str().unwrap();
        assert_eq!(queue(ds), ["P"]);
        assert_eq!(plans(ds), [json!({"file": "PLAN.md", "items": ["P"]}), json!({"file": "HANDOVER.md", "items": ["A", "B"]}),
            json!({"file": "TODO.md", "items": ["One", "Two"]})]);
        // #58: HANDOVER.md open-work sections (no Queue) → plans() sees them, queue() doesn't
        std::fs::write(d.join("HANDOVER.md"), "## State\n- x\n## Not verified live\n- #37 look\n## Gotchas\n- g\n").unwrap();
        assert_eq!(plans(ds)[1], json!({"file": "HANDOVER.md", "items": ["#37 look"]}));
        assert!(queue(ds).len() == 1); // PLAN.md's P
        for f in ["TODO.md", "PLAN.md"] { std::fs::remove_file(d.join(f)).unwrap(); }
    }

    #[test]
    fn inbox_front_goes_first_in_order() {
        let ib = Inbox::default();
        ib.push(Input::Msg("queued".into()));
        ib.front(vec![Input::Reconnect, Input::Msg("continue".into())]);
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let order: Vec<String> = (0..3).map(|_| match rt.block_on(ib.pop()) {
            Input::Msg(m) => m,
            Input::Rich(_) => "RICH".into(),
            Input::Reconnect => "RECONNECT".into(),
        }).collect();
        assert_eq!(order, ["RECONNECT", "continue", "queued"]);
    }

    #[test]
    fn answers_are_numbered_with_notes() {
        let qs = vec![Question { id: "a".into(), question: "Which db?".into(), ..Default::default() },
                      Question { id: "b".into(), question: "Name?".into(), ..Default::default() }];
        let t = answers_text(&qs, &[json!({"id": "a", "choice": ["SQLite"], "note": "keep it local"}),
                                   json!({"id": "b", "choice": ["Other"], "note": "Clauracle"})]);
        assert_eq!(t, "Answers to your questions:\n1. Which db? → SQLite (note: keep it local)\n2. Name? → Other (note: Clauracle)");
    }

    #[test]
    fn tool_summary() {
        assert_eq!(summary(&json!({"command": "ls\n-la"})), "ls");
        assert_eq!(summary(&json!({"file_path": "a.rs"})), "a.rs");
    }
}
