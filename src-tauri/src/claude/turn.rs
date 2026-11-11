//! The session loop: spawn the CLI, read its stream, show messages, end-of-turn work and continue.
use super::*;

pub(crate) async fn run(app: AppHandle, st: Arc<State>, pid: String, live: Arc<Live>) {
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

pub(crate) fn args(st: &State, p: &Project) -> Vec<String> {
    let cfg = st.cfg.lock().unwrap();
    let (deny, _) = store::restrictions(&cfg, p);
    let mut append = format!("{APPEND}\n{}", if issue_mode(p) { ISSUE_RULES } else { HANDOVER_RULES });
    append += match p.cont.as_str() { "auto" => AUTO_RULES, "auto_answer" => AUTO_ANSWER_RULES, _ => "" };
    append += WORK_RULES;
    // voice: never address the user by name (read as sarcastic)
    append += "\n# Voice\nThe user sees you as Clauracle. Reply plainly and directly, like a colleague; never address the user by name. \
        \"You\" = Clauracle: answer as Clauracle (the app and the assistant in it), never as Claude/the underlying model. \
        Asked to do something in the app (open a tab, change a setting, add a service…) → do it with the clauracle `ui` tool.\n";
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
    // Ponytail is not shipped: the page installs it from its source on first run (APPSKILLS). Until it is there, the switch adds nothing.
    if let Some(md) = Some(()).filter(|_| p.ponytail).and_then(|_| dirs::home_dir())
        .and_then(|h| std::fs::read_to_string(h.join(".claude/skills/ponytail/SKILL.md")).ok()) {
        append += "\n\n";
        append += md.splitn(3, "---").nth(2).unwrap_or(&md).trim();
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

pub(crate) async fn connect_and_serve(app: &AppHandle, st: &Arc<State>, pid: &str, live: &Arc<Live>) -> Result<(), String> {
    let p = st.project(pid).ok_or("project removed")?;
    let claude = store::which("claude").ok_or("Claude Code isn't installed (Settings › Install)")?;
    let base = st.cfg.lock().unwrap().accounts.iter().find(|a| a.id == p.account).map(|a| a.base_url.clone()).unwrap_or_default();
    let env: Vec<(String, String)> = [store::account_env(&p.account, &base),store::gh_env(&p.gh), store::gcloud_env(&p.gaccount, &p.gcp),
        vec![("CLAUDE_CODE_ENTRYPOINT".into(), "sdk-clauracle".into())]].concat();
    let mut cmd = tokio::process::Command::from(store::command(&claude));
    // inherited creds (Clauracle started from a session of another account) would make Main run — and record
    // usage — as that account: each session gets only its own account's vars (Main: the ~/.claude login)
    for k in ["CLAUDE_CODE_OAUTH_TOKEN", "ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL"] {
        cmd.env_remove(k);
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
    // /commands for the composer's autocomplete: [{name, description, argumentHint}], known before the first turn + #280 models (the CLI's own /model list)
    emit(app, "commands", json!({"pid": pid, "commands": r["commands"], "models": r["models"], "agents": r["agents"]}));
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
                        if let Some(id) = child.id() { crate::kill_tree(id); } // its MCP servers/shells too, not just claude.exe
                        let _ = child.kill().await;
                        return Err("reconnect".into());
                    }
                };
                live.busy.store(true, SeqCst);
                live.continuing.store(false, SeqCst);
                st.with_project(pid, |p| p.working = true);
                emit(app, "status", json!({"pid": pid, "busy": true, "handover": live.handing_over.load(SeqCst)}));
                if let Err(e) = conn.write(&json!({"type": "user", "message": {"role": "user", "content": content.clone()},
                                   "parent_tool_use_id": null, "session_id": "default"})).await {
                    // claude died under us: the message goes back first in line for the reconnected session (was lost)
                    live.inbox.front(vec![match content { Value::String(t) => Input::Msg(t), c => Input::Rich(c) }]);
                    return Err(e);
                }
                if let Some(t) = content.as_str() { // answers riding in this message are delivered now → drop them from the config
                    st.with_project(pid, |p| if !p.answers.is_empty() && t.contains(p.answers.as_str()) { p.answers.clear(); });
                }
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

pub(crate) fn show(app: &AppHandle, st: &State, pid: &str, acct: &str, m: &Value) { // acct = the account this CLI was spawned with
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
                    // model "<synthetic>" = text the CLI wrote itself: the reply of a /command (or an API error). cmd → the page offers the options it names
                    Some("text") if m["parent_tool_use_id"].is_null() && m["message"]["model"] == "<synthetic>" =>
                        emit(app, "line", json!({"pid": pid, "kind": "claude", "text": b["text"], "cmd": true})),
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
        // a background agent finished: the CLI injects its notice as plain user text; the page pairs it with the agent by its id
        Some("user") if m["message"]["content"].as_str().is_some_and(|s| s.contains("<task-notification>")) => {
            let s = m["message"]["content"].as_str().unwrap_or("");
            let tag = |n: &str| s.split(&format!("<{n}>")).nth(1).and_then(|r| r.split(&format!("</{n}>")).next()).unwrap_or("").trim().to_string();
            agent_done(app, pid, &Value::Null, &tag("task-id"), &tag("status"), &tag("summary"));
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
            // a /command the CLI ran itself and that printed nothing: say so, or the chat shows no reply at all
            if let Some(c) = m["local_command"].as_str().filter(|_| m["is_error"] != true && m["result"].as_str().unwrap_or("").is_empty()) {
                emit(app, "line", json!({"pid": pid, "kind": "claude", "text": format!("`/{c}` done."), "cmd": true}));
            }
            if m["is_error"] == true {
                let r = m["result"].as_str().unwrap_or("the turn ended with an error");
                line(app, pid, "err", r);
                // the wall itself: mark that window full now, or the next continue walks into it again (the loop)
                if is_limit_msg(r) {
                    let mut cfg = st.cfg.lock().unwrap();
                    store::mark_limit_hit(&mut cfg, acct, r);
                    store::save(&cfg);
                    emit(app, "usage", json!({"account": acct, "usage": store::usage(&cfg, acct)}));
                    drop(cfg);
                    if let Some(l) = st.live.lock().unwrap().get(pid) { l.limit_hit.store(true, SeqCst); }
                }
            }
        }
        Some("rate_limit_event") => {
            // #111: spawn-time account, ≠ p.account ∵ a relay switch changes p.account before the reconnect
            let mut cfg = st.cfg.lock().unwrap();
            store::record_usage(&mut cfg, &acct, &m["rate_limit_info"]);
            store::save(&cfg);
            emit(app, "usage", json!({"account": acct, "usage": store::usage(&cfg, &acct)}));
        }
        Some("system") if m["subtype"] == "init" => {
            emit(app, "commands", json!({"pid": pid, "commands": m["slash_commands"]}));
        }
        // a skill or plugin was added or removed mid-session: the composer's /list follows
        Some("system") if m["subtype"] == "commands_changed" => {
            emit(app, "commands", json!({"pid": pid, "commands": m["commands"]}));
        }
        Some("system") if m["subtype"] == "task_notification" =>
            agent_done(app, pid, &m["tool_use_id"], m["task_id"].as_str().unwrap_or(""), m["status"].as_str().unwrap_or(""), m["summary"].as_str().unwrap_or("")),
        _ => {}
    }
}

// a second result for the agent's tool call: the first one was only "Async agent launched"
fn agent_done(app: &AppHandle, pid: &str, of: &Value, task: &str, status: &str, summary: &str) {
    let kind = if status.is_empty() || status == "completed" { "out" } else { "err" };
    emit(app, "line", json!({"pid": pid, "kind": kind, "text": if summary.is_empty() { format!("Agent {status}") } else { summary.to_string() }, "result_of": of, "task": task}));
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

pub(crate) async fn after_turn(app: &AppHandle, st: &Arc<State>, pid: &str, live: &Arc<Live>, conn: &Conn) {
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
    // stopped mid-task by a limit (no ready): an auto run moves to a relay account and carries on, or pauses until a reset
    if live.limit_hit.swap(false, SeqCst) && !live.armed.load(SeqCst) && p.cont.starts_with("auto") {
        if limit_route(app, st, pid, live) {
            line(app, pid, "note", "Limit hit mid-task: carrying on.");
            live.inbox.front(vec![Input::Reconnect, Input::Msg(RESUMED.into())]);
        }
        return;
    }
    if live.armed.swap(false, SeqCst) {
        let was_handover = live.handing_over.swap(false, SeqCst);
        let mut p = p;
        let mut smart = false;
        let mut restart_self = false; // #170: Clauracle built itself after a task → swap into the new build
        let a = if p.build.after == "smart" { "smart_run" } else { p.build.after.as_str() }.to_string(); // #182 legacy smart = smart+run
        if !was_handover && a.starts_with("smart") {
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
                line(app, pid, "note", &format!("Smart build: {why}…"));
            }
        }
        if !was_handover && (smart || a.starts_with("build")) {
            // a task finished: build (and run/deploy) in plain code; a failure can go back to Claude
            let mut steps = vec![if a.contains("deploy") { "build" } else { "local" }]; // after a task: this machine only; deploy needs every target
            let own = own_repo(st, pid);
            if a.contains("run") && !own { steps.push("run"); } // own: restart replaces run
            if a.contains("deploy") { steps.push("deploy"); }
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
            restart_self = good && own;
            if smart && good {
                let dir = p.path.clone();
                let head = tauri::async_runtime::spawn_blocking(move || store::head(&dir)).await.unwrap_or_default();
                *live.built.lock().unwrap() = head;
            }
        }
        let auto = p.cont.starts_with("auto") || (p.cont == "off" && was_handover);
        st.with_project(pid, |p| p.ready = true);
        emit(app, "ready", json!({"pid": pid, "ready": true}));
        if restart_self {
            if let Ok((t, a)) = restart_target(&own_exe(&p.path)) {
                line(app, pid, "note", "Built: restarting into the new build (the run carries on after it).");
                emit(app, "restarting", json!({}));
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                    crate::relaunch(&app, t, &a);
                });
                return; // ready stays: resume_interrupted continues an auto run at the next start
            }
        }
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
    // #144: answers given mid-turn (lib answer_questions parks them in p.answers) → next turn now, not the next Continue
    let parked = st.with_project(pid, |p| std::mem::take(&mut p.answers)).unwrap_or_default();
    if !parked.is_empty() {
        live.inbox.push(Input::Msg(parked));
    }
    if let Ok(r) = conn.request(json!({"subtype": "get_context_usage"}), 15).await {
        let pct = r["percentage"].as_f64().unwrap_or(0.0);
        let max = r["totalTokens"].as_f64().filter(|_| pct > 0.0).map(|t| (t * 100.0 / pct).round()); // window size, for the saved-tokens cap
        emit(app, "status", json!({"pid": pid, "context": pct.round(), "tokens": r["totalTokens"], "max": max}));
        if p.context_limit > 0.0 && pct >= p.context_limit && !live.first_turn.load(SeqCst) {
            prepare(app, live, pid, &format!(": the context window is at {}%", pct.round()));
        }
    }
    live.first_turn.store(false, SeqCst);
}

/// Auto modes: check the limits, switch to a relay account or stop, else start the next session.
pub(crate) fn auto_continue(app: &AppHandle, st: &Arc<State>, pid: &str, live: &Arc<Live>) {
    if !limit_route(app, st, pid, live) { return; } // ready stays: resumes at the reset (or Continue)
    let p = st.project(pid).unwrap();
    // #9/#67: ANY open question pauses the run (auto only; auto_answer holds them by design) ∵ a clear while the card
    // is up = card gone + work goes on without the user → answering the last one resumes it
    if p.cont != "auto_answer" && !p.questions.is_empty() { // #220: off+handover too
        line(app, pid, "note", "Paused: a question needs you. Answering it carries on.");
        emit(app, "questions", json!({"pid": pid, "questions": p.questions}));
        return; // ready stays: answer_questions → continue_now
    }
    let msg = cont_text(&p, st.cfg.lock().unwrap().agents_on); // ponytail: blocking gh call on a runtime worker (~1s, once per session)
    // gh failing (logged out, token revoked) → generic continue; twice in a row = endless empty sessions burning tokens → pause
    let blind = msg.as_deref().is_some_and(|m| m.starts_with(CONTINUE_ISSUES) && pin_of(m) == 0);
    if live.blind.swap(blind, SeqCst) && blind {
        line(app, pid, "note", "Paused: Clauracle can't read the GitHub issues (check the gh account in Settings). Continue retries.");
        return;
    }
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

pub(crate) fn label(window: &str) -> &str {
    match window { "five_hour" => "5-hour", "seven_day" => "weekly", w => w }
}

/// Clear the context: a new session, then (optionally) the continue message `cont` plus any answers.
pub fn fresh(app: &AppHandle, st: &State, pid: &str, live: &Live, cont: Option<String>) {
    live.armed.store(false, SeqCst);
    live.handing_over.store(false, SeqCst);
    live.first_turn.store(true, SeqCst);
    live.nagged.store(false, SeqCst);
    let pin = cont.as_deref().map_or(0, pin_of);
    let answers = st.with_project(pid, |p| {
        if pin > 0 { p.work = pin; } // #83 + persisted: a crash/restart/handover resumes this issue first
        if p.session.is_some() { p.prev_session = p.session.take(); }
        p.ready = false;
        p.answers.clone() // cleared once written to the new session (connect loop) ∵ a crash before that lost them
    }).unwrap_or_default();
    emit(app, "cleared", json!({"pid": pid}));
    if pin > 0 { emit(app, "project", json!(st.project(pid))); } // UI's P.work → "Working on #N" + queue order
    // #217 claim it ourselves (assignee + `in progress`) ∵ the prompt's "claim it" rule was skipped by the AI → others saw it free
    if let Some(p) = st.project(pid).filter(|p| pin > 0 && issue_mode(p)) {
        std::thread::spawn(move || { let _ = store::task_do(&p.path, &p.gh, "claim", &pin.to_string()); });
    }
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

/// #398 auto relay switch (never manual picks) → log note + in-app toast (bell list) + desktop/phone notify.
pub fn relay_switched(app: &AppHandle, st: &State, pid: &str, from: &str, to: &str, window: &str) {
    let (f, t, relay) = {
        let c = st.cfg.lock().unwrap();
        let (a, b) = (store::account_or_main(&c, from), store::account_or_main(&c, to));
        (a.nick.clone(), b.nick.clone(), b.relay.clone())
    };
    let name = st.project(pid).map(|p| p.name).unwrap_or_default();
    let msg = format!("{f} reached its {} limit: switched to {t} (relay {relay}).", label(window));
    line(app, pid, "note", &msg);
    emit(app, "project", json!(st.project(pid)));
    emit(app, "switched", json!({"pid": pid, "text": format!("{name}: {msg}")}));
    notify(app, st, &format!("{name}: account switched"), &msg);
}

pub fn notify(app: &AppHandle, st: &State, title: &str, body: &str) {
    let n = st.cfg.lock().unwrap().notify.clone();
    if (store::now() as u64) < n.snooze {
        return;
    }
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

