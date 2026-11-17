//! Commands: app state, projects, sending messages, agents, answers.
use crate::*;

// async + locks dropped before the file reads (PLAN/HANDOVER/CHANGES per project): ran on the window thread holding cfg
#[tauri::command(async)]
pub(crate) fn state(st: St) -> Value {
    let (mut out, paths) = {
        let cfg = st.cfg.lock().unwrap();
        let usage: serde_json::Map<String, Value> =
            cfg.accounts.iter().map(|a| (a.id.clone(), json!(store::usage(&cfg, &a.id)))).collect();
        // when each usage window resets (unix secs), per account: shown in the usage cards, toast when one passes
        let resets: serde_json::Map<String, Value> = cfg.usage.iter().map(|(id, w)|
            (id.clone(), json!(w.iter().filter_map(|(k, v)| v.resets_at.map(|r| (k.clone(), r))).collect::<std::collections::HashMap<_, _>>()))).collect();
        let busy: Vec<String> = st.live.lock().unwrap().iter().filter(|(_, l)| l.busy.load(SeqCst)).map(|(k, _)| k.clone()).collect();
        let paths: Vec<(String, String)> = cfg.projects.iter().map(|p| (p.id.clone(), p.path.clone())).collect();
        (json!({"projects": cfg.projects, "accounts": cfg.accounts, "usage": usage, "resets": resets, "active": cfg.active, "busy": busy,
           "default_account": cfg.default_account, "agents_on": cfg.agents_on, "agents_auto": cfg.agents_auto, "ci_builds": cfg.ci_builds, "mac": cfg.mac, "web": !cfg.off.iter().any(|x| x == "web"), "lessons": cfg.lessons, "devs": cfg.devs, "you": cfg.you, "habits": cfg.habits, "notify": cfg.notify,
           "update_every": cfg.update_every.unwrap_or(24), "update_at": cfg.update_at,
           "projects_dir": store::projects_root(&cfg).to_string_lossy(), "local_dir": local::dir(&cfg), "agent_models": cfg.agent_models, "agent_effort": cfg.agent_effort}), paths)
    };
    let per = |f: &dyn Fn(&str) -> Value| paths.iter().map(|(id, p)| (id.clone(), f(p))).collect::<serde_json::Map<String, Value>>();
    // next steps from PLAN.md (ready bar, Tasks tab), every plan file with tasks (Plan tab ⓘ / merge), last few done (CHANGES.md),
    // PLAN.md "## Waiting on the user" → Plan tab suggestions (Accept/Dismiss), parse only
    out["queues"] = json!(per(&|p| json!(claude::queue(p))));
    out["plans"] = json!(per(&|p| json!(claude::plans(p))));
    out["done"] = json!(per(&|p| json!(claude::done(p, 3))));
    out["waits"] = json!(per(&|p| json!(claude::waiting(p))));
    out["drafts"] = json!(store::drafts());
    out["claude"] = json!(store::which("claude").is_some());
    out["gh"] = json!(store::which("gh").is_some());
    out
}

#[tauri::command(async)]
pub(crate) fn open_project(app: AppHandle, st: St, pid: String) -> R<Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    {
        let mut cfg = st.cfg.lock().unwrap();
        cfg.active = Some(pid.clone());
        if let Some(p) = cfg.projects.iter_mut().find(|p| p.id == pid) {
            p.used = store::now();
        }
        store::save(&cfg);
    }
    let live = session(&app, &st, &pid);
    // previous session (before the last clear), faded, then the current one
    let mut history: Vec<Value> = p.prev_session.as_deref().map(claude::history).unwrap_or_default().into_iter()
        .map(|mut e| { e["old"] = json!(true); e }).collect();
    history.extend(p.session.as_deref().map(claude::history).unwrap_or_default());
    // #243 saved = the page's own log (UI-only notes like task reports too), survives clear + restart; null = never saved
    let saved: Value = log_file(&pid).ok().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    Ok(json!({"branch": store::git_branch(&p.path), "busy": live.busy.load(SeqCst), "history": history, "saved": saved}))
}

/// Who should run the repo at `path` (store::suggest); None if it isn't on GitHub. gh calls, so off the main thread.
pub(crate) async fn detect(st: &St<'_>, path: String) -> Option<store::Who> {
    let cfg = st.cfg.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (owner, repo) = store::repo_of(&path)?;
        Some(store::suggest(&cfg, &owner, &store::gh_users(), |u| store::can_push(u, &owner, &repo)))
    }).await.ok().flatten()
}

/// A new project starts with the suggested GitHub + AI account (the repo owner's, if it's one of ours).
#[tauri::command]
pub(crate) async fn add_project(st: St<'_>, path: String) -> R<Value> {
    let who = detect(&st, path.clone()).await;
    let mut cfg = st.cfg.lock().unwrap();
    let p = match &who {
        Some(w) => store::new_project(&mut cfg, &path, Some(&w.account), Some(w.gh.as_str()).filter(|g| !g.is_empty())),
        None => store::new_project(&mut cfg, &path, None, None),
    };
    let p = mark(&mut cfg, &p.id, who.as_ref()).ok_or("no such project")?;
    store::save(&cfg);
    Ok(json!(p))
}

/// None = project removed meanwhile (was unwrap under the cfg lock → poisoned Mutex → every command dead)
pub(crate) fn mark(cfg: &mut store::Config, pid: &str, who: Option<&store::Who>) -> Option<store::Project> {
    let p = cfg.projects.iter_mut().find(|p| p.id == pid)?;
    if let Some(w) = who {
        (p.owner, p.shared) = (w.owner.clone(), w.shared);
    }
    Some(p.clone())
}

/// After opening: refresh owner/shared, and say whether the suggestion differs from what the project uses.
#[tauri::command]
pub(crate) async fn who(st: St<'_>, pid: String) -> R<Value> {
    let path = st.project(&pid).ok_or("no such project")?.path;
    let Some(w) = detect(&st, path).await else { return Ok(Value::Null) };
    let mut cfg = st.cfg.lock().unwrap();
    let p = mark(&mut cfg, &pid, Some(&w)).ok_or("no such project")?;
    store::save(&cfg);
    let differs = !p.keep && !w.gh.is_empty() && (!w.gh.eq_ignore_ascii_case(&p.gh) || w.account != p.account);
    Ok(json!({"who": w, "differs": differs}))
}

#[tauri::command]
pub(crate) fn remove_project(st: St, pid: String) {
    let mut cfg = st.cfg.lock().unwrap();
    cfg.projects.retain(|p| p.id != pid);
    store::save(&cfg);
    claude::stop_run(&pid); // its dev server/app kept running until exit
    if let Some(l) = st.live.lock().unwrap().remove(&pid) {
        l.inbox.front(vec![Input::Reconnect]); // its loop sees the project gone and ends
    }
}

/// Merge changed fields into the project; anything that shapes the claude process reconnects it (after the
/// current reply, same session).
#[tauri::command]
pub(crate) fn update_project(app: AppHandle, st: St, pid: String, changes: Value) -> R<Value> {
    let quiet = ["name", "context_limit", "build", "keep", "services", "stage", "rel_auto", "order", "work", "targets", "site", "wired", "license", "devs"]; // read live per call: no reconnect
    let p = st.with_project(&pid, |p| {
        let mut v = json!(p);
        for (k, val) in changes.as_object().cloned().unwrap_or_default() {
            if !["id", "path", "session", "questions"].contains(&k.as_str()) {
                v[k] = val;
            }
        }
        // bad value (e.g. NaN → null) used to be dropped silently while the UI thought it saved
        serde_json::from_value(v).map(|n| { *p = n; p.clone() }).map_err(|e| format!("not saved: {e}"))
    }).ok_or("no such project")??;
    if changes.as_object().map_or(false, |o| o.keys().any(|k| !quiet.contains(&k.as_str()))) {
        if let Some(l) = st.live.lock().unwrap().get(&pid) {
            l.inbox.front(vec![Input::Reconnect]);
        }
    }
    emit(&app, "project", json!(p));
    Ok(json!(p))
}

/// images: pasted ones as [{media_type, data(base64)}]; files: paths, mentioned so Claude reads only what it needs.
/// work: the issue this message starts (urgent ask / ask into an empty queue) → persisted ∴ a handover or crash resumes it.
#[tauri::command]
pub(crate) fn send(app: AppHandle, st: St, pid: String, text: String, images: Option<Vec<Value>>, files: Option<Vec<String>>, work: Option<u64>) {
    if let Some(n) = work.filter(|&n| n > 0) { st.with_project(&pid, |p| p.work = n); }
    let live = session(&app, &st, &pid);
    let (images, files) = (images.unwrap_or_default(), files.unwrap_or_default());
    if text.trim() == "/clear" && images.is_empty() && files.is_empty() {
        return claude::fresh(&app, &st, &pid, &live, None);
    }
    let text = if files.is_empty() { text } else {
        format!("{text}\n\nAttached files (read them as needed):\n{}", files.iter().map(|f| format!("- {f}")).collect::<Vec<_>>().join("\n"))
    };
    if images.is_empty() {
        return live.inbox.push(Input::Msg(text));
    }
    live.inbox.push(Input::Rich(json!(rich_blocks(&text, &images))));
}

/// Text and pasted images as content blocks, each image placed right after its "@[ref]" marker so the sentence
/// around it keeps pointing at it. Images the text never mentions go first, each labelled with its marker.
pub(crate) fn rich_blocks(text: &str, images: &[Value]) -> Vec<Value> {
    let img = |i: &Value| json!({"type": "image", "source": {"type": "base64", "media_type": i["media_type"], "data": i["data"]}});
    let txt = |t: &str| json!({"type": "text", "text": t});
    // marker end in the text (term view writes "@[ ref]") → image index, in text order
    let mut at: Vec<(usize, usize)> = images.iter().enumerate().filter_map(|(k, i)| {
        let r = store::img_ref(i);
        [format!("@[{r}]"), format!("@[ {r}]")].iter().find_map(|m| text.find(m.as_str()).map(|p| (p + m.len(), k)))
    }).collect();
    at.sort();
    let mut out: Vec<Value> = images.iter().enumerate().filter(|(k, _)| !at.iter().any(|a| a.1 == *k))
        .flat_map(|(_, i)| [txt(&format!("@[{}]", store::img_ref(i))), img(i)]).collect();
    let mut from = 0;
    // two images with one name share a marker: no empty text block between them (the API refuses those)
    for (end, k) in at { if end > from { out.push(txt(&text[from..end])); } out.push(img(&images[k])); from = end; }
    let rest = &text[from..];
    if !rest.trim().is_empty() { out.push(txt(rest)); } else if text.trim().is_empty() { out.push(txt("(see the image)")); }
    out
}

#[test]
fn rich_blocks_keep_images_in_place() {
    let im = |n: &str| json!({"name": n, "media_type": "image/png", "data": n});
    let b = rich_blocks("Make @[a] blue, not @[ b] or x.", &[im("b.png"), im("c.png"), im("a.png")]);
    let kinds: Vec<String> = b.iter().map(|x| x["text"].as_str().map_or_else(|| format!("img {}", x["source"]["data"].as_str().unwrap()), String::from)).collect();
    assert_eq!(kinds, ["@[c]", "img c.png", "Make @[a]", "img a.png", " blue, not @[ b]", "img b.png", " or x."]);
    assert_eq!(rich_blocks("", &[im("a.png")]).last().unwrap()["text"], "(see the image)");
}

#[tauri::command]
pub(crate) fn agents(st: St) -> Value {
    json!(st.cfg.lock().unwrap().agents.clone().unwrap_or_else(store::default_agents))
}

/// Takes effect when a session next connects (new session / continue / settings change).
#[tauri::command]
pub(crate) fn set_agents(st: St, agents: Vec<store::Agent>, on: Option<bool>, auto: Option<bool>) {
    let mut cfg = st.cfg.lock().unwrap();
    cfg.agents = Some(agents);
    if let Some(on) = on { cfg.agents_on = on; }
    if let Some(a) = auto { cfg.agents_auto = a; }
    store::save(&cfg);
    for l in st.live.lock().unwrap().values() {
        l.inbox.front(vec![Input::Reconnect]);
    }
}

/// Settings › Agents model pick for a built-in agent ("" = its default); sessions reconnect to load it.
#[tauri::command]
pub(crate) fn set_agent_model(st: St, name: String, model: String, effort: Option<String>) {
    let mut cfg = st.cfg.lock().unwrap();
    match effort.as_deref() { None => {}, Some("") => { cfg.agent_effort.remove(&name); }, Some(e) => { cfg.agent_effort.insert(name.clone(), e.into()); } }
    if model.is_empty() { cfg.agent_models.remove(&name); } else { cfg.agent_models.insert(name, model); }
    store::save(&cfg);
    for l in st.live.lock().unwrap().values() {
        l.inbox.front(vec![Input::Reconnect]);
    }
}

/// Message typed while a turn runs → straight into it (true), or false = page keeps it queued for the turn end.
#[tauri::command]
pub(crate) async fn send_mid(st: St<'_>, pid: String, text: String, work: Option<u64>) -> R<bool> {
    let Some(live) = st.live.lock().unwrap().get(&pid).cloned() else { return Ok(false) };
    let ok = claude::send_mid(&live, &text).await;
    if ok { if let Some(n) = work.filter(|&n| n > 0) { st.with_project(&pid, |p| p.work = n); } }
    Ok(ok)
}

#[tauri::command]
pub(crate) async fn interrupt(st: St<'_>, pid: String) -> R<()> {
    let (conn, oc) = st.live.lock().unwrap().get(&pid).map(|l| (l.conn.lock().unwrap().clone(), l.oc.lock().unwrap().clone())).unwrap_or_default();
    if let Some(o) = oc { claude::Backend::interrupt(&*o).await?; }
    if let Some(c) = conn {
        claude::Backend::interrupt(&*c).await?;
    }
    Ok(())
}

/// The Continue button: clear the context and carry on from the handover (with any answers).
/// QR code as an SVG string (Browser tab, phone preview).
#[tauri::command]
pub(crate) fn qr(text: String) -> R<String> {
    let c = qrcode::QrCode::new(text.as_bytes()).map_err(|e| e.to_string())?;
    // 2px per module, no quiet zone (the page pads it): sharp at 1:1, scaled up on click
    Ok(c.render::<qrcode::render::svg::Color>().module_dimensions(2, 2).quiet_zone(false).build())
}

/// GitHub avatar bytes, disk-cached 1 day in ~/.clauracle/avatars ∵ webview re-fetched github.com/x.png on every
/// card open for some users (redirect not cached). UI turns bytes → blob URL once per run.
#[tauri::command]
pub(crate) async fn avatar(login: String) -> R<tauri::ipc::Response> {
    if login.is_empty() || !login.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '[' || c == ']') { return Err("bad login".into()); }
    let f = store::home().join("avatars").join(format!("{login}.png"));
    let fresh = std::fs::metadata(&f).and_then(|m| m.modified()).ok()
        .and_then(|t| t.elapsed().ok()).is_some_and(|age| age.as_secs() < 86400);
    if !fresh {
        let got = ureq::get(&format!("https://github.com/{login}.png?size=88")).set("User-Agent", "Clauracle").timeout(std::time::Duration::from_secs(10)).call()
            .map_err(|e| e.to_string()).and_then(|r| { let mut b = vec![]; std::io::Read::read_to_end(&mut r.into_reader(), &mut b).map_err(|e| e.to_string())?; Ok(b) });
        match got {
            Ok(b) => { let _ = std::fs::create_dir_all(f.parent().unwrap()); let _ = std::fs::write(&f, &b); return Ok(tauri::ipc::Response::new(b)); }
            Err(e) if !f.exists() => return Err(e),
            Err(_) => {} // offline → stale copy
        }
    }
    std::fs::read(&f).map(tauri::ipc::Response::new).map_err(|e| e.to_string())
}

/// This machine's LAN address (the route a phone on the same Wi-Fi would use); UDP connect sends nothing.
#[tauri::command(async)]
pub(crate) fn lan_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0").and_then(|s| { s.connect("8.8.8.8:80")?; s.local_addr() })
        .map(|a| a.ip().to_string()).unwrap_or_default()
}

#[tauri::command]
pub(crate) fn set_lessons(st: St, lessons: Vec<String>) {
    let mut cfg = st.cfg.lock().unwrap();
    cfg.lessons = lessons.into_iter().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
    store::save(&cfg);
}

/// #603 the onboarding answers (what the user makes, where websites live, who handles domains, release platforms).
#[tauri::command]
pub(crate) fn set_habits(st: St, habits: serde_json::Value) {
    let mut cfg = st.cfg.lock().unwrap();
    cfg.habits = habits;
    store::save(&cfg);
}

#[tauri::command]
pub(crate) fn set_you(st: St, you: store::You) {
    let mut cfg = st.cfg.lock().unwrap();
    cfg.you = store::You {nick: you.nick.trim().into(), prefs: you.prefs.trim().into(),
        tags: you.tags.into_iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect()};
    let md = format!("# About the user
Interests: {}

{}
", cfg.you.tags.join(", "), cfg.you.prefs);
    let _ = store::write_atomic(&store::home().join("you.md"), md.as_bytes());
    store::save(&cfg);
}

#[tauri::command(async)] // async ∵ cont_text runs gh in issue mode
pub(crate) fn continue_now(app: AppHandle, st: St, pid: String, extra: Option<String>) {
    if st.project(&pid).is_none() { return; } // removed: no stray session, no unwrap panic below
    let live = session(&app, &st, &pid);
    if let Some(x) = extra.filter(|x| !x.trim().is_empty()) { // user messages sent while it waited: ride along with "continue"
        st.with_project(&pid, |p| p.answers = if p.answers.is_empty() { x } else { format!("{}

{x}", p.answers) });
    }
    live.pause_gen.fetch_add(1, SeqCst); // a manual Continue replaces a pending auto-resume
    if !claude::limit_route(&app, &st, &pid, &live) { return; } // nothing left anywhere: paused until a reset, said so
    let p = st.project(&pid).unwrap(); // after a relay switch: same path/gh anyway
    // #220: open question = runner paused in every mode (auto_answer holds them by design) ∵ next task went on unanswered
    if !p.questions.is_empty() && p.cont != "auto_answer" {
        claude::line(&app, &pid, "note", "Paused: a question needs you. Answering it carries on.");
        emit(&app, "status", json!({"pid": pid, "busy": false})); // #302 ends the UI's "Reading handover…" pill (no turn starts)
        emit(&app, "questions", json!({"pid": pid, "questions": p.questions}));
        return;
    }
    // #87: no open task + no user text → no fresh turn (was: old PLAN.md "continue" → AI found nothing, stopped)
    let Some(msg) = claude::cont_text(&p, st.cfg.lock().unwrap().agents_auto).or_else(|| (!p.answers.is_empty()).then(|| claude::CONTINUE_BARE.into())) else {
        claude::line(&app, &pid, "note", "No open tasks: nothing to continue. Send a new request to start again.");
        emit(&app, "status", json!({"pid": pid, "busy": false})); // #302 same
        return;
    };
    if live.continuing.swap(true, SeqCst) { return; } // double click / Continue racing an answer: one fresh session, not two
    claude::fresh(&app, &st, &pid, &live, Some(msg));
}

/// Clear & continue on demand: Claude writes the handover first, then it's ready.
#[tauri::command]
pub(crate) fn prepare_handover(app: AppHandle, st: St, pid: String) {
    let live = session(&app, &st, &pid);
    claude::prepare(&app, &live, &pid, "");
}

#[tauri::command]
pub(crate) fn answer_ask(st: St, id: String, answer: Value) {
    if let Some(tx) = st.asks.lock().unwrap().remove(&id) {
        let _ = tx.send(answer);
    }
}

/// The question card's Confirm. Answers go into the next Continue, or straight to Claude when it's idle.
#[tauri::command(async)] // async ∵ resume → continue_now → cont_text (gh)
pub(crate) fn answer_questions(app: AppHandle, st: St, pid: String, answers: Vec<Value>) {
    let ids: Vec<&str> = answers.iter().filter_map(|a| a["id"].as_str()).collect();
    let (text, send_now, resume) = st.with_project(&pid, |p| {
        let text = claude::answers_text(&p.questions, &answers);
        p.questions.retain(|q| !ids.contains(&q.id.as_str()));
        // #144: answers wake an idle session in any mode: mid-task (!ready) → sent as a turn; ready → Continue.
        // #9/#67 exception: auto run paused on open questions waits for the last one
        (text, !p.ready, p.ready && (p.questions.is_empty() || p.cont == "auto_answer")) // #220: last answer resumes
    }).unwrap_or_default();
    let live = session(&app, &st, &pid);
    if send_now && !live.busy.load(SeqCst) {
        live.inbox.push(Input::Msg(text));
    } else {
        st.with_project(&pid, |p| p.answers = [p.answers.as_str(), text.as_str()].join("\n").trim().to_string());
        // turn ended between the busy check and parking → its pickup took nothing: send now (else answers sat unread)
        if send_now && !live.busy.load(SeqCst) {
            let t = st.with_project(&pid, |p| std::mem::take(&mut p.answers)).unwrap_or_default();
            if !t.is_empty() { live.inbox.push(Input::Msg(t)); }
        }
    }
    emit(&app, "project", json!(st.project(&pid)));
    let acct = st.project(&pid).map(|p| p.account).unwrap_or_default();
    let room = store::over_limit(&st.cfg.lock().unwrap(), &acct).is_none(); // limit pause stays a limit pause
    if resume && room && !live.busy.load(SeqCst) {
        continue_now(app, st, pid, None);
    }
}

// ---- settings ----
