//! Control requests: permissions and allow rules, the ui tool and Clauracle's in-process MCP server.
use super::*;

pub(crate) async fn control(app: &AppHandle, st: &Arc<State>, pid: &str, live: &Arc<Live>, conn: &Conn, v: Value) {
    let req = &v["request"];
    let id = v["request_id"].clone();
    let resp = match req["subtype"].as_str() {
        Some("can_use_tool") => Ok(permission(app, st, pid, req).await),
        // ui = async (waits on the page's reply) → handled here, not in sync mcp()
        Some("mcp_message") if req["message"]["params"]["name"] == "ui" => {
            let m = &req["message"];
            let a = &m["params"]["arguments"];
            // js runs with the page's full powers (every app command): only Clauracle's own repo gets it, for self-tests
            let t = if !a["js"].is_null() && !own_repo(st, pid) {
                "Refused: `js` only works in Clauracle's own project. Use sel/text/x,y to click and `show` to read.".to_string()
            } else { ui_tool(app, a).await };
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

pub(crate) async fn permission(app: &AppHandle, st: &Arc<State>, pid: &str, req: &Value) -> Value {
    let tool = req["tool_name"].as_str().unwrap_or("");
    let input = req["input"].clone();
    // ui acts in the app as the user: it asks like any other tool (allow rules and bypass mode still apply)
    if tool.starts_with("mcp__clauracle__") && tool != "mcp__clauracle__ui" {
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
pub(crate) fn rule_hit(rule: &str, tool: &str, input: &Value, now: u64) -> bool {
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
pub(crate) fn allowed(allow: &[String], tool: &str, input: &Value, now: u64) -> bool {
    let hit = |inp: &Value| allow.iter().any(|r| rule_hit(r, tool, inp, now));
    if hit(input) { return true; }
    let Some(c) = input["command"].as_str().filter(|_| tool == "Bash") else { return false };
    let parts = split_cmd(c);
    parts.len() > 1 && parts.iter().all(|p| hit(&json!({"command": p})))
}

/// Parts at ; && || | newline outside quotes (= splitCmd in index.html): sed 's/a;b/' stays one part.
pub(crate) fn split_cmd(c: &str) -> Vec<String> {
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
pub(crate) fn rule_hit_works() {
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
pub fn own_repo(st: &Arc<State>, pid: &str) -> bool {
    st.project(pid).is_some_and(|p| std::fs::read_to_string(std::path::Path::new(&p.path).join("src-tauri/tauri.conf.json"))
        .is_ok_and(|s| s.contains("no.arctim.clauracle")))
}

pub static UI_WAIT: Mutex<Vec<(u64, tokio::sync::oneshot::Sender<String>)>> = Mutex::new(Vec::new());
pub(crate) static UI_N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Self-test: runs in the main webview (never the OS mouse): find by sel/text/x,y → click (or type), optional js,
/// then report the top open dialog/popover text (or `show` selector's). Page answers via `ui_reply`. Cards (permission/question) refused.
pub(crate) async fn ui_tool(app: &AppHandle, a: &Value) -> String {
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

pub(crate) const UI_JS: &str = r#"(async () => { const a = ARGS; let el = null, out = [];
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

pub(crate) fn tool_list() -> Value {
    let q = json!({"type": "object", "required": ["questions"], "properties": {"questions": {"type": "array", "items": {
        "type": "object", "required": ["question", "options"], "properties": {
            "question": {"type": "string", "description": "the question only; Clauracle numbers them"},
            "options": {"type": "array", "items": {"type": "object", "required": ["label"], "properties": {
                "label": {"type": "string"}, "description": {"type": "string"},
                "recommended": {"type": "boolean", "description": "your suggestion; at most one per question"}}}},
            "multi": {"type": "boolean", "description": "more than one option may be picked"},
            "blocking": {"type": "boolean", "description": "true only if work truly can't go on without the user"}}}}}});
    json!({"tools": [
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
         "inputSchema": {"type": "object", "properties": {"exe": {"type": "string"}}}},
        // #414 every project ∵ "do X in the app" = anything the user can click (tabs, settings, services, notifications,
        // start menu, permission level, links…); question/permission cards stay the user's
        {"name": "ui", "description": "Act in Clauracle's own window like the user would, when they ask you to do \
          something in the app (open a tab/page/menu/the start menu, change a setting or the permission level, add a \
          service, check notifications, open a link…), or to self-test it. Inside the page, not the OS mouse. Look first \
          (`show`, or `js` reading the DOM) when unsure what to click. Pick ONE target: `sel` (CSS selector), `text` (visible text/tooltip of a \
          clickable, case-insensitive substring) or `x`,`y` (CSS px). Default = click; `type` = set an input's value; \
          `right` = right-click. `js` (Clauracle's own project only) = expression evaluated after (await-ed; page globals like S, cur, P() visible). \
          Returns what was hit + the open dialog/popover text after `wait` ms (400); `show` = CSS selector whose text \
          to return instead (e.g. \"#right\"). \
          Permission/question cards are refused.", "inputSchema": {"type": "object", "properties": {
            "sel": {"type": "string"}, "text": {"type": "string"}, "x": {"type": "number"}, "y": {"type": "number"},
            "type": {"type": "string"}, "right": {"type": "boolean"}, "js": {"type": "string"}, "wait": {"type": "number"}, "show": {"type": "string"}}}}]})
}

/// What to start: this exe, or a new build as its own swap helper (`--swap-into` this exe's path), so the app's
/// path never changes (pinnable) ∵ Windows locks a running exe: the helper waits for us to exit, then replaces it.
pub fn restart_target(exe: &str) -> Result<(std::path::PathBuf, Vec<String>), String> {
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
        let live = session(app, st, &pid);
        if !limit_route(app, st, &pid, &live) { continue; } // all at a limit: resumes at the reset, not into the wall
        live.inbox.push(Input::Msg(RESUMED.into()));
        line(app, &pid, "note", "Clauracle restarted mid-turn: resuming");
    }
    // auto run that was between tasks (ready) when the app went down (self-restart #170, update, crash) → carry on;
    // auto_continue re-checks limits, questions and an empty queue. Delay: page listening first.
    let waiting: Vec<String> = st.cfg.lock().unwrap().projects.iter().filter(|p| p.ready && !p.working && p.cont.starts_with("auto"))
        .map(|p| p.id.clone()).collect();
    for pid in waiting {
        let (app, st) = (app.clone(), st.clone());
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let live = session(&app, &st, &pid);
            if !live.busy.load(SeqCst) && st.project(&pid).is_some_and(|p| p.ready) {
                tauri::async_runtime::spawn_blocking(move || auto_continue(&app, &st, &pid, &live)); // cont_text runs gh
            }
        });
    }
}

/// The exe a build of Clauracle's own repo makes (tauri build → target/release).
/// #303 newest of release/debug exe ∵ after-task build = fast debug twin (#288) → release went stale, restart swapped into old code
pub fn own_exe(dir: &str) -> String {
    let n = if cfg!(windows) { "clauracle.exe" } else { "clauracle" };
    let t = std::path::Path::new(dir).join("src-tauri/target");
    let age = |p: &std::path::Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let (r, d) = (t.join("release").join(n), t.join("debug").join(n));
    if age(&d) > age(&r) { d } else { r }.to_string_lossy().into()
}

pub(crate) fn mcp(app: &AppHandle, st: &Arc<State>, pid: &str, live: &Arc<Live>, m: &Value) -> Value {
    let id = m["id"].clone();
    let ok = |result: Value| json!({"jsonrpc": "2.0", "id": id, "result": result});
    let text = |t: &str| ok(json!({"content": [{"type": "text", "text": t}]}));
    match m["method"].as_str() {
        Some("initialize") => ok(json!({"protocolVersion": m["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
            "capabilities": {"tools": {}}, "serverInfo": {"name": "clauracle", "version": "1.0.0"}})),
        Some("tools/list") => ok(tool_list()),
        Some("tools/call") => match m["params"]["name"].as_str() {
            Some("ready") => {
                let p = st.project(pid).unwrap();
                if p.cont == "off" && !live.handing_over.load(SeqCst) {
                    return text("Clear-continue is off for this project: nothing will be cleared. Just end the turn.");
                }
                // #83: finished = pinned issue closed (code decides, not the AI) ∵ ready on an open issue → clear →
                // Continue re-picks it → #62/#75 loop. Handover pauses (context limit) pass; one refusal, then trust.
                let n = p.work;
                if n > 0 && !live.handing_over.load(SeqCst) && !live.nagged.swap(true, SeqCst) {
                    let s = store::gh_json(&["issue", "view", &n.to_string(), "--json", "state,labels"], &p.gh, Some(&p.path));
                    let waits = s["labels"].as_array().into_iter().flatten().any(|l| l["name"] == store::WAIT_LABEL);
                    if s["state"] == "OPEN" && !waits { // ponytail: blocking gh (~1s) on the reader; gh failing → let ready through
                        return text(&format!("Not armed: issue #{n} (this session's task) is still open. Done → verify, \
                          `gh issue close {n} -c <summary>`, call ready again. Not done → comment the progress on #{n} and \
                          call ready again: the next session resumes it first (don't split it into new issues). Blocked on \
                          the user → add the `needs-user` label + a comment saying what they must do, then ready."));
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
                // swapping in another exe = running any binary as the app: only Clauracle's own repo may do it
                if !exe.is_empty() && !own_repo(st, pid) {
                    return text("Refused: only Clauracle's own project can restart into another build.");
                }
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
                    _ => text("Queued: the user sees them when this turn ends; the next task waits until they answer. Finish what doesn't depend on them first."),
                }
            }
            _ => json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "unknown tool"}}),
        },
        _ if m.get("id").is_none() => json!({"jsonrpc": "2.0", "result": {}}), // a notification: just ack
        _ => json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "method not found"}}),
    }
}

// ---- build / run / deploy: plain commands, output streamed to the Build panel ------------------------

