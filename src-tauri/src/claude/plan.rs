//! Local plan files (PLAN.md/HANDOVER.md): queue, waiting list, history.
use super::*;

/// Files in the project root that may hold tasks; PLAN.md = the single source once it exists.
pub(crate) const PLAN_FILES: [&str; 4] = ["PLAN.md", "HANDOVER.md", "TASKS.md", "TODO.md"];

/// Open tasks of a plan file, in order: its "## Queue" list if it has one; else (not HANDOVER.md: its bullets are
/// state, not tasks) every top-level list item that isn't checked off.
/// `wide` (plans() only): HANDOVER.md open-work headings count too; queue() stays Queue-only (Continue text).
pub(crate) fn items(dir: &str, file: &str, wide: bool) -> Vec<String> {
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
pub(crate) fn primary(dir: &str) -> &'static str {
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
pub(crate) fn sect(lines: &[String], head: fn(&str) -> bool) -> Option<(usize, usize, Vec<usize>)> {
    let s = lines.iter().position(|l| head(l))? + 1;
    let mut e = lines[s..].iter().position(|l| l.starts_with('#')).map_or(lines.len(), |i| s + i);
    while e > s && lines[e - 1].trim().is_empty() { e -= 1; }
    let item = regex::Regex::new(r"^(?:\d+[.)]|[-*])\s").unwrap();
    Some((s, e, (s..e).filter(|&i| item.is_match(&lines[i])).collect()))
}
pub(crate) fn wait_head(l: &str) -> bool { l.starts_with('#') && l.trim_start_matches('#').trim().to_ascii_lowercase().starts_with("waiting") }
pub(crate) fn queue_head(l: &str) -> bool { l.starts_with('#') && l.trim_start_matches('#').trim().eq_ignore_ascii_case("queue") }
pub(crate) const MARK: &str = r"^(?:\d+[.)]|[-*])\s+(?:\[ \]\s*)?";
pub(crate) fn plan_lines(dir: &str) -> (std::path::PathBuf, String, Vec<String>) {
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

