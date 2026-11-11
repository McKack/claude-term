//! One `claude` process per project, spoken to over its stream-json protocol (what claude-agent-sdk does):
//! user messages in, messages out, plus control requests both ways (permissions, our in-process MCP tools,
//! interrupt, context usage). On top: the continue modes, held questions, limits and relay switching.
use crate::store::{self, Config, Project, Question};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot, Notify};
mod turn;
pub use turn::*;
mod control;
pub use control::*;
mod steps;
pub use steps::*;
mod plan;
pub use plan::*;

const LESSONS_MAX: usize = 30; // ponytail: ~30 one-liners ≈ 1k tokens per session; prune in Settings › Accounts

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
- Gotchas don't grow HANDOVER.md: code-area ones go to .claude/rules/<area>.md with \"paths:\" frontmatter (e.g.
  ui/**), loaded only when a matching file is touched; topic recipes (release, live tests) go to .claude/notes/<topic>.md,
  listed in HANDOVER.md with one \"read when …\" line each. One line per gotcha, only what a later change would get
  wrong, not a log of what was done; under ~40 lines a file, prune stale ones when you add one.
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
  them in order (first = next; work on that one only, the rest wait); `gh issue view N` for details. Outstanding work
  comes before new asks: a new ask goes to the END of the list unless the user says it's urgent. Skip issues someone
  else is assigned to.
- Clauracle files the user's asks as issues itself (\"Filed as issue #N\" note): don't file them again. Never lose one:
  an ask that reached you without that note and is new work → `gh issue create --label clauracle --label <task|feature|fix>`.
- Do NOT create issues for your own follow-ups, ideas, live checks, rebuilds or splits: that piles up work nobody asked
  for (only exception: one user ask holding several separate asks, split as its note says). Verify inside the task (tests, build, the `ui` tool) before closing it. A worthwhile idea → at most one issue with
  `--label suggestion` (the user accepts or dismisses it; rare). A task too big for one session stays ONE issue: comment
  the progress on it and hand over; the next session resumes it first.
- \"Build and run\" / \"restart into a new copy\" = an action, not a task: do it now (this machine's build only, never every
  release target unless asked) and start it (Clauracle itself: mcp__clauracle__restart with the new exe). Close any issue filed for it.
- Blocked on the user (their login, password, hardware, billing, a decision, a check only they can do): `gh issue edit N
  --add-label needs-user` + one comment saying exactly what they must do, then move on (ready). Clauracle shows it under
  \"Waiting on you\" and puts it back in the queue when they unblock it. Never file a separate issue for that.
- HANDOVER.md lets a fresh session resume: goal, current state, how to build and test, gotchas, rules; no task list,
  no open items; under ~50 lines (prune stale notes when you add one). Finished work goes to CHANGES.md (newest first,
  one line per change). In a git repo where these aren't tracked, add them to .git/info/exclude.
- Gotchas don't grow HANDOVER.md: code-area ones go to .claude/rules/<area>.md with \"paths:\" frontmatter (e.g.
  ui/**), loaded only when a matching file is touched; topic recipes (release, live tests) go to .claude/notes/<topic>.md,
  listed in HANDOVER.md with one \"read when …\" line each. One line per gotcha, only what a later change would get
  wrong, not a log of what was done; under ~40 lines a file, prune stale ones when you add one.
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

/// Every project. Sessions used to report fixes as done from reading the code, and wrote symbol shorthand nobody could read.
const WORK_RULES: &str = "
# Done means checked
Before you say something is fixed, done or working, run the check in this turn (the tests, the build, or the app
itself) and quote the result line. Nothing run → say \"not verified\" and what is left. Never claim it works from
reading the code, and never repeat an earlier session's claim without checking it again.
# Code
Write it the way a careful person would: reuse what the file already has before adding a new helper or class, and
keep comments short, plain sentences that say why (no symbol shorthand, no issue-number logs).
# Unclear asks
When an ask is unclear, work out what is meant from its screenshots and the user's text next to each image
reference. Don't mention the confusion in your reply; just do what they meant.";

const PREPARE: &str = "Pause here; don't start anything new{why}. Update HANDOVER.md so a fresh session can pick up exactly where you are (goal, what you were doing and how far it got, what is verified and what isn't, gotchas learned). Task list: PLAN.md's Queue (local), or with GitHub issues a progress comment on the issue you're on (it stays open and is resumed first; file no new issues). Move finished work to CHANGES.md, then call mcp__clauracle__ready as the LAST action.";

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

/// Agents on: later tasks that touch other files may run beside the first one, each claimed so no one else takes it.
const PARALLEL: &str = "\n\nAgents are on, so the first task need not run alone: you may hand up to two later tasks from this \
list to helper agents (Task tool) at the same time, only ones that clearly touch different files than the first task and each \
other (check with `gh issue view N`; when unsure, leave it for a later session). Claim each one before its agent starts \
(`gh issue edit N --add-assignee @me --add-label \"in progress\"`) and tell the agent the issue, its ask file if any, and the \
only files it may change. When an agent is back, check its work like your own, then close the issue and add it to CHANGES.md; \
if it is not done, comment its progress on the issue and remove the `in progress` label.";

/// The "continue" message for a fresh session, or None when nothing is queued. Issue mode: one `gh issue list`
/// (blocking, ~1s) → ordered "#N title" lines; gh failing → generic text (a network blip mustn't stop a run).
pub fn cont_text(p: &Project, agents: bool) -> Option<String> {
    if !issue_mode(p) {
        return (!queue(&p.path).is_empty()).then(|| CONTINUE.into());
    }
    let v = store::tasks(&p.path, &p.gh);
    if !v.is_array() {
        return Some(format!("{CONTINUE_ISSUES} run `gh issue list --label clauracle` to see them."));
    }
    let q = store::task_queue(&v, &p.gh, &p.order, p.work);
    // the first task's local ask file (exact words, image notes) is found by issue number; nothing about it is on GitHub
    let ask = q.first().and_then(|x| { let n = x["number"].as_u64()?; Some((n, store::ask_of(&p.path, n)?)) })
        .map(|(n, f)| format!("\n\nThe user's exact words for #{n} are in the local file {f}: Read it first, it wins over the issue summary. \
            An image's notes are in a .md of the same name beside it.")).unwrap_or_default();
    let par = if agents && q.len() > 1 { PARALLEL } else { "" };
    (!q.is_empty()).then(|| format!("{CONTINUE_ISSUES}\n{}{ask}{par}",
        q.iter().map(|x| format!("- #{} {}", x["number"], x["title"].as_str().unwrap_or(""))).collect::<Vec<_>>().join("\n")))
}

// ---------------------------------------------------------------------------------------------

/// Mid-turn delivery (CLI feel): a user message written into the RUNNING turn; the CLI hands it to the model at the next
/// step instead of after the whole turn. Refused (→ caller queues it for the turn end) when idle, connecting, or the turn
/// is wrapping up (ready armed / handover: the context is about to be cleared and the message would be lost with it).
pub async fn send_mid(live: &Live, text: &str) -> bool {
    if !live.busy.load(SeqCst) || live.armed.load(SeqCst) || live.handing_over.load(SeqCst) { return false; }
    let Some(c) = live.conn.lock().unwrap().clone() else { return false };
    c.write(&json!({"type": "user", "message": {"role": "user", "content": text}, "parent_tool_use_id": null, "session_id": "default"})).await.is_ok()
}

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
    pub armed: AtomicBool,        // ready was called this turn
    pub handing_over: AtomicBool, // Clauracle asked for a handover (context limit / Clear & continue)
    first_turn: AtomicBool,   // a fresh session already past the context limit must not hand over forever
    nagged: AtomicBool,       // pin refusal already sent → 2nd ready goes through (AI insists: blocked/waiting)
    blind: AtomicBool,
    pub continuing: AtomicBool, // a Continue's fresh session is on its way (cleared when its message goes out) → 2nd click ignored        // last auto continue couldn't read the issues (gh failing) → a 2nd in a row pauses
    built: Mutex<String>,     // #89 smart build: HEAD at session start / last good smart build = diff base
    hits: Mutex<Vec<f64>>,    // times a limit made this run switch or stretch: 3 within 15 min = a loop → pause
    stretched: Mutex<String>, // "account+extra" already announced, so the stretch note shows once
    limit_hit: AtomicBool,    // this turn ended on the CLI's limit message
    pub pause_gen: std::sync::atomic::AtomicU64, // a newer pause (or a manual Continue) cancels an older pause's auto-resume
}

/// Limits before a turn starts: switch to a relay account with room, stretch one past its set limit, or pause until
/// the earliest reset (then an auto run resumes by itself). false = paused: start nothing.
pub fn limit_route(app: &AppHandle, st: &Arc<State>, pid: &str, live: &Arc<Live>) -> bool {
    let Some(p) = st.project(pid) else { return false };
    let route = store::relay_route(&st.cfg.lock().unwrap(), &p.account);
    let to = match &route {
        store::Route::Stay => return true,
        store::Route::Switch(n) | store::Route::Stretch(n, _) => n.clone(),
        store::Route::Pause(until) => { pause(app, st, pid, live, *until); return false; }
    };
    // loop guard: the limit info can lag behind the real limit (a turn hits the wall, the next account too…)
    let t = store::now();
    let looping = { let mut h = live.hits.lock().unwrap(); h.retain(|x| t - x < 900.0); h.push(t); h.len() >= 3 };
    if looping {
        live.hits.lock().unwrap().clear();
        let until = store::relay_free_at(&st.cfg.lock().unwrap(), &p.account);
        pause(app, st, pid, live, until);
        return false;
    }
    let window = store::over_limit(&st.cfg.lock().unwrap(), &p.account).unwrap_or_default();
    if to != p.account {
        // #111: keep = gh stays as set in the project, no "Keep current / Use these" suggestion
        st.with_project(pid, |q| { q.account = to.clone(); q.keep = true; });
        if let store::Route::Switch(_) = route { relay_switched(app, st, pid, &p.account, &to, &window); }
        else { emit(app, "project", json!(st.project(pid))); }
    }
    if let store::Route::Stretch(n, extra) = route {
        let key = format!("{n}+{extra}");
        if *live.stretched.lock().unwrap() != key {
            let (nick, relay) = { let c = st.cfg.lock().unwrap(); let a = store::account_or_main(&c, &n); (a.nick.clone(), a.relay.clone()) };
            let who = if relay.is_empty() { format!("{nick} is") } else { format!("Every account of the {relay} relay is") };
            line(app, pid, "note", &format!("{who} at its set limit: {nick} goes on up to {extra}% past it until a limit resets."));
            *live.stretched.lock().unwrap() = key;
        }
    }
    true
}

/// Nothing left anywhere: say until when (the page writes the local time), notify, and resume an auto run at the reset.
fn pause(app: &AppHandle, st: &Arc<State>, pid: &str, live: &Arc<Live>, until: f64) {
    let Some(p) = st.project(pid) else { return };
    let relay = store::account_or_main(&st.cfg.lock().unwrap(), &p.account).relay.clone();
    let gen = live.pause_gen.fetch_add(1, SeqCst) + 1;
    emit(app, "paused", json!({"pid": pid, "until": until, "relay": relay}));
    emit(app, "status", json!({"pid": pid, "busy": false}));
    let mins = ((until - store::now()) / 60.0).max(1.0).round() as u64;
    let what = if relay.is_empty() { "The usage limits are hit".to_string() } else { format!("All usage limits of the {relay} relay are hit") };
    notify(app, st, &format!("{}: paused", p.name), &format!("{what}: work is paused for {}h {:02}m, until a limit resets.", mins / 60, mins % 60));
    if !p.questions.is_empty() {
        emit(app, "questions", json!({"pid": pid, "questions": p.questions}));
    }
    if !p.cont.starts_with("auto") { return; } // manual: Continue after the reset
    let (app, st, pid, live) = (app.clone(), st.clone(), pid.to_string(), live.clone());
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs_f64((until - store::now()).max(0.0) + 60.0)).await;
        if live.pause_gen.load(SeqCst) != gen || live.busy.load(SeqCst) { return; }
        live.hits.lock().unwrap().clear();
        if st.project(&pid).is_some_and(|p| p.ready) {
            tauri::async_runtime::spawn_blocking(move || auto_continue(&app, &st, &pid, &live));
        } else if st.project(&pid).is_some() && limit_route(&app, &st, &pid, &live) {
            // stopped mid-task by the limit: the same session carries on (a new account needs a new process)
            line(&app, &pid, "note", "A limit reset: resuming.");
            live.inbox.front(vec![Input::Reconnect, Input::Msg(RESUMED.into())]);
        }
    });
}

/// The CLI's own limit message (turn result): "You've hit your 5-hour limit · resets …", "usage limit reached".
fn is_limit_msg(t: &str) -> bool {
    let t = t.to_lowercase();
    t.contains("limit") && (t.contains("hit your") || t.contains("usage limit") || t.contains("rate limit") || t.contains("limit reached"))
}

#[test]
fn limit_messages() {
    assert!(is_limit_msg("You've hit your 5-hour limit · resets 10:30pm (Europe/Oslo)"));
    assert!(is_limit_msg("Claude usage limit reached. Your limit will reset at 5pm"));
    assert!(!is_limit_msg("the turn ended with an error"));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobile_cmd_rewrites_tauri_and_flutter() {
        assert_eq!(mobile_cmd("npx tauri build", "android").as_deref(), Some("npx tauri android build"));
        assert_eq!(mobile_cmd("flutter build windows", "ios").as_deref(), Some("flutter build ipa"));
        assert_eq!(mobile_cmd("make", "ios"), None);
    }

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

    #[cfg(windows)]
    #[test]
    fn running_exe_is_moved_aside_before_a_build() {
        let root = std::env::temp_dir().join(format!("clauracle-lock-{}", std::process::id()));
        let dir = root.join("target/debug");
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("app.exe");
        std::fs::copy(r"C:\Windows\System32\ping.exe", &exe).unwrap();
        let mut child = std::process::Command::new(&exe).args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null()).spawn().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(std::fs::remove_file(&exe).is_err(), "a running exe should be locked");
        free_locked_exes(root.to_str().unwrap());
        let moved = std::fs::read_dir(&dir).unwrap().flatten().any(|e| e.file_name().to_string_lossy().ends_with(".exe.old"));
        let _ = child.kill(); let _ = child.wait();
        assert!(!exe.exists() && moved);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn answers_are_numbered_with_notes() {
        let qs = vec![Question { id: "a".into(), question: "Which db?".into(), ..Default::default() },
                      Question { id: "b".into(), question: "Name?".into(), ..Default::default() }];
        assert_eq!(fast_cmd("npx tauri build"), "npx tauri build --debug --no-bundle");
        assert_eq!(fast_cmd("cargo build --release"), "cargo build");
        assert_eq!(fast_cmd("npm run build"), "npm run build");
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
