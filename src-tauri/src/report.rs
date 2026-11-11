//! Commands: report HTML/PDF, logs, small Claude prompts.
use crate::*;

/// #413 one task's report → HTML from its data (no AI): summary counts, parts, files, failures, time table.
pub(crate) fn rep_html(r: &Value) -> String {
    let arr = |v: &Value| v.as_array().cloned().unwrap_or_default();
    let mins = |v: &Value| format!("{} min", (v.as_f64().unwrap_or(0.0) / 60000.0).round().max(1.0));
    let (segs, ops) = (arr(&r["segs"]), arr(&r["ops"]));
    let nums = |k: &str| segs.iter().flat_map(|s| arr(&s[k])).map(|n| format!("#{n}")).collect::<Vec<_>>().join(", ");
    let files: std::collections::BTreeSet<&str> = ops.iter().filter(|o| o["d"].is_array()).filter_map(|o| o["f"].as_str()).filter(|f| !f.is_empty()).collect();
    let bad: Vec<&Value> = ops.iter().filter(|o| o["ok"] == false).collect();
    let (closed, filed) = (nums("closed"), nums("filed"));
    let mut h = format!("<h2>Summary</h2><p>{} steps in {}, {} file{} changed{}{}.</p><h2>What was done</h2><ul>", ops.len(), mins(&r["ms"]), files.len(),
        if files.len() == 1 { "" } else { "s" }, if closed.is_empty() { String::new() } else { format!(", closed {closed}") }, if filed.is_empty() { String::new() } else { format!(", filed {filed}") });
    for s in &segs { h += &format!("<li>{} <small>({} steps, {} files)</small></li>", html_esc(s["title"].as_str().unwrap_or("")), s["n"], s["files"]); }
    h += "</ul>";
    if !files.is_empty() { h += &format!("<h3>Files changed</h3><ul>{}</ul>", files.iter().map(|f| format!("<li><code>{}</code></li>", html_esc(f))).collect::<String>()); }
    let first = |v: &Value| v.as_str().unwrap_or("").lines().next().unwrap_or("").to_string();
    if !bad.is_empty() { h += &format!("<h2>Problems spotted</h2><ul>{}</ul>", bad.iter().map(|o| format!("<li><code>{}</code>: {}</li>",
        html_esc(&first(if o["cmd"].as_str().is_some_and(|c| !c.is_empty()) { &o["cmd"] } else { &o["text"] })), html_esc(&first(&o["err"])))).collect::<String>()); }
    h += "<h2>Time</h2><table><tr><th>Part</th><th>Time</th></tr>";
    for s in &segs { h += &format!("<tr><td>{}</td><td>{}</td></tr>", html_esc(s["title"].as_str().unwrap_or("")), mins(&s["ms"])); }
    h + "</table>"
}
#[test]
pub(crate) fn rep_html_works() {
    let h = rep_html(&json!({"ms": 120000, "segs": [{"title": "Fix <x>", "n": 2, "files": 1, "ms": 90000, "closed": [5], "filed": []}],
        "ops": [{"ok": true, "f": "a.rs", "d": [["", "x"]]}, {"ok": false, "cmd": "cargo test", "err": "boom\nmore"}]}));
    assert!(h.contains("2 steps in 2 min, 1 file changed, closed #5.") && h.contains("Fix &lt;x&gt;") && h.contains("<code>cargo test</code>: boom</li>"), "{h}");
}

pub(crate) const REP_KEEP: u64 = 7 * 86400;
pub(crate) fn log_file(pid: &str) -> R<std::path::PathBuf> { rep_dir(pid)?; /* validates pid */ Ok(store::home().join("logs").join(format!("{pid}.json"))) }
/// #243 chat log of a project as the page shows it (capped by the page), read back by open_project.
#[tauri::command(async)]
pub(crate) fn log_save(pid: String, log: Value) -> R<()> {
    let f = log_file(&pid)?;
    std::fs::create_dir_all(f.parent().unwrap()).map_err(|e| e.to_string())?;
    store::write_atomic(&f, log.to_string().as_bytes()).map_err(|e| e.to_string())
}

pub(crate) fn rep_dir(pid: &str) -> R<std::path::PathBuf> {
    if pid.is_empty() || !pid.chars().all(|c| c.is_ascii_alphanumeric() || "-_".contains(c)) { return Err("bad project id".into()); }
    Ok(store::home().join("reports").join(pid))
}
/// Reports of the last 7 days, oldest first; older files deleted on the way.
pub(crate) fn rep_list(pid: &str) -> R<Vec<Value>> {
    let d = rep_dir(pid)?;
    let mut l: Vec<(std::path::PathBuf, Value)> = std::fs::read_dir(&d).into_iter().flatten().flatten().filter_map(|f| {
        let p = f.path();
        if p.extension()? != "json" { return None; }
        let age = f.metadata().ok()?.modified().ok()?.elapsed().map(|e| e.as_secs()).unwrap_or(0);
        if age > REP_KEEP { let _ = std::fs::remove_file(&p); return None; }
        Some((p.clone(), serde_json::from_str(&std::fs::read_to_string(&p).ok()?).ok()?))
    }).collect();
    l.sort_by(|a, b| a.0.cmp(&b.0)); // names = ms timestamps
    Ok(l.into_iter().map(|x| x.1).collect())
}

#[tauri::command(async)]
pub(crate) fn report_save(pid: String, rep: Value) -> R<()> {
    let d = rep_dir(&pid)?;
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let id = rep["id"].as_u64().unwrap_or((store::now() * 1000.0) as u64); // = UI rid (Date.now) → report_get
    std::fs::write(d.join(format!("{id}.json")), rep.to_string()).map_err(|e| e.to_string())?;
    rep_list(&pid).map(|_| ()) // prunes
}

#[tauri::command(async)]
pub(crate) fn report_get(pid: String, id: u64) -> R<Value> {
    let t = std::fs::read_to_string(rep_dir(&pid)?.join(format!("{id}.json"))).map_err(|e| e.to_string())?;
    serde_json::from_str(&t).map_err(|e| e.to_string())
}

/// One-shot `claude -p` on Sonnet with the project's AI account → trimmed stdout. cwd = a dir with no CLAUDE.md/hooks ∴ cheap; no tools.
pub(crate) fn sonnet(st: &State, p: &store::Project, effort: &str, cwd: &std::path::Path, prompt: &str) -> R<String> {
    claude_p(st, p, &["--model", "sonnet", "--effort", effort], cwd, vec![], prompt)
}

/// `claude -p` (text out, prompt on stdin) with the project's AI account + `args`/`env` extra.
pub(crate) fn claude_p(st: &State, p: &store::Project, args: &[&str], cwd: &std::path::Path, env: Vec<(String, String)>, prompt: &str) -> R<String> {
    claude_pt(st, p, args, cwd, env, prompt, None)
}

/// Same, but with `limit` the child is killed after that long and the call fails with "timed out".
pub(crate) fn claude_pt(st: &State, p: &store::Project, args: &[&str], cwd: &std::path::Path, env: Vec<(String, String)>, prompt: &str, limit: Option<std::time::Duration>) -> R<String> {
    use std::io::{Read, Write};
    let claude = store::which("claude").ok_or("Claude Code isn't installed")?;
    let base = st.cfg.lock().unwrap().accounts.iter().find(|a| a.id == p.account).map(|a| a.base_url.clone()).unwrap_or_default();
    let mut c = store::command(&claude);
    for k in ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL"] { c.env_remove(k); }
    for (k, _) in std::env::vars() { if k.starts_with("CLAUDE") || k == "NO_COLOR" { c.env_remove(k); } } // inherited session vars (Clauracle started from a CLI)
    c.arg("-p").args(args).args(["--output-format", "text"]).envs(store::account_env(&p.account, &base)).envs(env)
        .current_dir(cwd).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let mut ch = c.spawn().map_err(|e| e.to_string())?;
    track(ch.id());
    ch.stdin.take().unwrap().write_all(prompt.as_bytes()).map_err(|e| e.to_string())?;
    // Output is read on threads so the wait below can poll the deadline.
    let rd = |mut r: Box<dyn Read + Send>| std::thread::spawn(move || { let mut b = Vec::new(); let _ = r.read_to_end(&mut b); b });
    let (so, se) = (rd(Box::new(ch.stdout.take().unwrap())), rd(Box::new(ch.stderr.take().unwrap())));
    let t0 = std::time::Instant::now();
    let status = loop {
        if let Some(s) = ch.try_wait().map_err(|e| e.to_string())? { break s; }
        if limit.is_some_and(|l| t0.elapsed() > l) {
            // ponytail: kills the direct child only; on Windows a .cmd shim's node child lingers until app exit (tracked). taskkill /T here if that shows up.
            let _ = ch.kill();
            let _ = ch.wait();
            return Err("timed out".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    let (o, e) = (so.join().unwrap_or_default(), se.join().unwrap_or_default());
    let out = String::from_utf8_lossy(&o).trim().to_string();
    if !status.success() || out.is_empty() { return Err(String::from_utf8_lossy(&e).trim().to_string()); }
    Ok(out)
}

/// rep = one task's report, None = the 7-day report. Sonnet (medium effort) writes the prose as an HTML fragment, a
/// headless Chromium (Edge/Chrome) prints it → PDF in ~/.clauracle/reports/<pid>/pdf, opened. ponytail: no browser → Err.
#[tauri::command(async)]
pub(crate) fn report_pdf(st: St, pid: String, rep: Option<Value>) -> R<String> {
    let p = st.project(&pid).ok_or("project removed")?;
    let week = rep.is_none();
    let reps = match rep { Some(r) => vec![r], None => rep_list(&pid)? };
    if reps.is_empty() { return Err("No reports from the last 7 days yet: one is saved each time a task finishes.".into()); }
    let mut data = serde_json::to_string(&reps).unwrap_or_default();
    if data.len() > 80_000 { data = data.chars().take(80_000).collect(); } // ponytail: cut, Sonnet still sees most of the week
    let what = if week { "the last 7 days of work (one JSON object per finished task)" } else { "one finished task" };
    // #368 week: tasks grouped by area (sprint-meeting style), each group = <h3> + 1-2 sentence summary + its tasks
    let done = if week { "What was done: group the tasks by area/topic (e.g. UI, release, accounts), one <h3> per area, \
        each with a 1-2 sentence summary of what changed in that area this week, then a short <ul> of its tasks" } else { "What was done (<ul>)" };
    let prompt = format!("Write a short, sleek report for a team meeting about {what} on the software project \"{}\". \
        Output ONLY an HTML fragment (no <html>/<head>/<body>, no markdown, no code fences). Sections as <h2>: Summary (2-3 sentences), \
        {done}, Problems spotted (failed commands, missing tools; skip if none), Time (a small <table> of tasks and durations), \
        Suggestions (2-4 concrete next steps). Plain business tone, no filler. Data:\n{data}", p.name);
    let d = rep_dir(&pid)?.join("pdf");
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    // #413 task report = plain data → rep_html; only the 7-day one needs Sonnet (groups by area + summaries/suggestions)
    let body = if week { sonnet(&st, &p, "medium", &d, &prompt).map_err(|e| format!("Report writer failed: {e}"))? } else { rep_html(&reps[0]) };
    let body = body.trim_start_matches("```html").trim_matches('`').trim().to_string();
    let logo = include_str!("../../assets/icon.svg");
    let logo = &logo[logo.find("<svg").unwrap_or(0)..];
    let date = chrono_ish();
    let html = format!(r#"<!doctype html><meta charset="utf-8"><title>{n} report</title><style>
@page{{margin:18mm}}body{{font:11pt/1.5 "Segoe UI",system-ui,sans-serif;color:#222;max-width:720px;margin:auto}}
header{{display:flex;gap:14px;align-items:center;border-bottom:3px solid #D77655;padding-bottom:12px;margin-bottom:8px}}
header svg{{width:48px;height:48px}}h1{{margin:0;font-size:20pt}}header small{{color:#777}}
h2{{color:#D77655;font-size:13pt;margin:20px 0 6px}}h3{{font-size:11.5pt;margin:12px 0 2px}}table{{border-collapse:collapse;width:100%}}td,th{{border-bottom:1px solid #eee;padding:4px 6px;text-align:left}}
code{{background:#f4f1ef;padding:0 4px;border-radius:3px}}</style>
<header>{logo}<div><small>Clauracle · {kind}</small><h1>{n}</h1><small>{date}</small></div></header>{body}"#,
        n = html_esc(&p.name), kind = if week { "7-day report" } else { "Task report" });
    let stem = format!("{}-{}", if week { "7-day" } else { "task" }, (store::now()) as u64);
    let hf = d.join(format!("{stem}.html"));
    let pf = d.join(format!("{stem}.pdf"));
    std::fs::write(&hf, html).map_err(|e| e.to_string())?;
    let exe = store::browsers().iter().find_map(|b| b["exe"].as_str().map(String::from)).ok_or("No Chrome/Edge found to print the PDF")?;
    let r = std::process::Command::new(&exe).args(["--headless=new", "--disable-gpu", "--no-pdf-header-footer"])
        .arg(format!("--print-to-pdf={}", pf.display())).arg(format!("file:///{}", hf.display().to_string().replace('\\', "/"))).output();
    if !pf.exists() { return Err(format!("PDF print failed{}", r.err().map(|e| format!(": {e}")).unwrap_or_default())); }
    Ok(pf.to_string_lossy().into())
}
pub(crate) fn html_esc(s: &str) -> String { s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;") }
/// "2026-11-08 14:05 UTC" w/o a date crate: civil-from-days (Hinnant).
pub(crate) fn chrono_ish() -> String { civil(store::now() as i64) }
pub(crate) fn civil(s: i64) -> String {
    let (z, t) = (s.div_euclid(86400) + 719468, s.rem_euclid(86400));
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let dd = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{}-{m:02}-{dd:02} {:02}:{:02} UTC", yoe + era * 400 + (m <= 2) as i64, t / 3600, t % 3600 / 60)
}
