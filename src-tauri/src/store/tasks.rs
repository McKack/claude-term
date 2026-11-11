//! Task queue on GitHub issues: labels, ask files and attachments, triage, file overlap between tasks.
use super::*;

pub const OUR_LABEL: &str = "clauracle";
pub const TASK_LABEL: &str = "task";
pub const DOING_LABEL: &str = "in progress";
pub const IDEA_LABEL: &str = "suggestion"; // Claude's own idea awaiting the user's yes (Issues list Accept/Dismiss)
pub const WAIT_LABEL: &str = "needs-user"; // task blocked on the user (login, password, hardware, billing): out of the AI queue until unblocked

pub fn tasks(dir: &str, user: &str) -> serde_json::Value {
    // 300 ∵ gh lists newest first: past the limit the OLDEST tasks (first in line) would drop out
    let v = gh_json(&["issue", "list", "--label", OUR_LABEL, "--state", "open", "--limit", "300",
              "--json", "number,title,url,assignees,labels,updatedAt,author"], user, Some(dir));
    // (2026-11-05 `task`→`clauracle` relabel migration removed: it bypassed trusted() and wrote to GitHub on every empty poll)
    trusted(v, user, || {
        // ∵ issue templates can auto-label outsiders' issues → only push-access authors queue work for the AI
        gh_json(&["api", "repos/{owner}/{repo}/collaborators", "--jq", "[.[] | select(.permissions.push) | .login]"], user, Some(dir))
            .as_array().into_iter().flatten().filter_map(|l| l.as_str().map(str::to_lowercase)).collect()
    })
}

/// Keep issues by `me` or a push collaborator. Collaborators fetched only if a stranger's issue shows up (solo repo = 0
/// extra calls); fetch fails → only mine. ponytail: per-poll fetch then; cache if foreign issues get common.
pub(crate) fn trusted(v: serde_json::Value, me: &str, collab: impl FnOnce() -> Vec<String>) -> serde_json::Value {
    let me = me.to_lowercase();
    let who = |x: &serde_json::Value| x["author"]["login"].as_str().unwrap_or("").to_lowercase();
    let Some(a) = v.as_array() else { return v };
    if a.iter().all(|x| who(x) == me) { return v; }
    let ok = collab();
    serde_json::Value::Array(a.iter().filter(|x| { let w = who(x); w == me || ok.contains(&w) }).cloned().collect())
}

#[test]
pub(crate) fn trusted_works() {
    let v = serde_json::json!([{"number": 1, "author": {"login": "Me"}}, {"number": 2, "author": {"login": "bob"}}, {"number": 3, "author": {"login": "eve"}}]);
    let n = |v: serde_json::Value| v.as_array().unwrap().iter().map(|x| x["number"].as_u64().unwrap()).collect::<Vec<_>>();
    assert_eq!(n(trusted(v.clone(), "me", || vec!["bob".into()])), [1, 2]);
    assert_eq!(n(trusted(v.clone(), "me", Vec::new)), [1]);
    assert_eq!(n(trusted(serde_json::json!([{"number": 1, "author": {"login": "me"}}]), "me", || panic!("no fetch"))), [1]);
}

/// #220: ask images → files a fresh session can Read (gh can't upload to issues) → their paths, for the issue body.
/// `<repo>/.git/clauracle-att/` ∵ inside the project = no permission prompt, never committed; no .git dir → ~/.clauracle/att.
/// #234: returns paths relative to the project (`.git/clauracle-att/…`) or `~/.clauracle/att/…` ∵ they go into public
/// issue bodies → no user/folder names; the session's cwd = project root ∴ still readable.
/// ponytail: kept forever (screenshots, small); prune when someone notices the folder.
pub fn att_save(dir: &str, images: &[serde_json::Value]) -> Vec<String> {
    att_save_opt(dir, images).into_iter().flatten().collect()
}

/// Folder for ask files and screenshots, and the path shown for it.
pub(crate) fn att_dir(dir: &str) -> (PathBuf, &'static str) {
    let git = std::path::Path::new(dir).join(".git");
    let r = if git.is_dir() { (git.join("clauracle-att"), ".git/clauracle-att") } else { (home().join("att"), "~/.clauracle/att") };
    let _ = std::fs::create_dir_all(&r.0);
    r
}

/// Like att_save, but one entry per input image (None = that image failed), so callers can pair them with notes.
pub fn att_save_opt(dir: &str, images: &[serde_json::Value]) -> Vec<Option<String>> {
    if images.is_empty() { return vec![]; }
    let (d, shown) = att_dir(dir);
    images.iter().enumerate().map(|(i, x)| {
        let ext = x["media_type"].as_str().unwrap_or("image/png").rsplit('/').next().unwrap_or("png").replace("jpeg", "jpg");
        // #239 attachment stem kept (sanitized) ∴ viewer/issue show the name the composer used; none → index
        let stem: String = x["name"].as_str().map(|n| n.rsplit_once('.').map_or(n, |s| s.0)).unwrap_or("").chars()
            .filter(|c| c.is_ascii_alphanumeric() || "-_".contains(*c)).take(60).collect();
        let name = format!("{}{i}-{}.{ext}", new_id(), if /* {i}: same id + stem twice can't collide */ stem.is_empty() { i.to_string() } else { stem });
        std::fs::write(d.join(&name), b64(x["data"].as_str()?)?).ok()?;
        Some(format!("{shown}/{name}"))
    }).collect()
}

/// The durable record of a chat ask: exact text, then the saved images with their notes (shots = path, notes).
/// Written next to the screenshots; returns the shown path. Each image's notes also go in a same-name `.md` beside it.
/// Nothing about these files goes on GitHub: the app finds them by name (ask_filed, ask_of).
pub fn ask_save(dir: &str, raw: &str, shots: &[(String, String)]) -> Option<String> {
    let (d, shown) = att_dir(dir);
    let name = format!("ask-{}.md", new_id());
    let mut t = raw.to_string();
    if !shots.is_empty() {
        t += "\n\n## Screenshots\n";
        for (p, n) in shots {
            t += &format!("- {p}\n");
            for l in n.lines().filter(|l| !l.trim().is_empty()) { t += &format!("  {l}\n"); }
            if !n.trim().is_empty() { let _ = std::fs::write(ask_file(dir, p).with_extension("md"), n); }
        }
    }
    std::fs::write(d.join(&name), t).ok()?;
    Some(format!("{shown}/{name}"))
}

/// Appends a line to an ask file given by its shown path (best effort).
pub fn ask_note(dir: &str, shown: &str, line: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(ask_file(dir, shown)) { let _ = writeln!(f, "\n{line}"); }
}

/// Shown path (`.git/clauracle-att/x` or `~/.clauracle/att/x`) → real path.
pub(crate) fn ask_file(dir: &str, shown: &str) -> PathBuf {
    match shown.strip_prefix("~/.clauracle/att/") {
        Some(n) => home().join("att").join(n),
        None => std::path::Path::new(dir).join(shown),
    }
}

/// Filed: note the URL, then rename to `ask-<issue number>.md` so the app finds it by number (ask_of).
/// ponytail: only inside a repo's .git; the shared ~/.clauracle/att fallback keeps its id name (numbers would collide).
pub fn ask_filed(dir: &str, shown: &str, url: &str) {
    ask_note(dir, shown, &format!("Filed: {url}"));
    let n = url.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    if shown.starts_with(".git/") && !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()) {
        let f = ask_file(dir, shown);
        let _ = std::fs::rename(&f, f.with_file_name(format!("ask-{n}.md")));
    }
}

/// The ask file of issue `n`, as a path relative to the project, when there is one.
pub fn ask_of(dir: &str, n: u64) -> Option<String> {
    let s = format!(".git/clauracle-att/ask-{n}.md");
    std::path::Path::new(dir).join(&s).is_file().then_some(s)
}

#[test]
pub(crate) fn ask_save_works() {
    let d = std::env::temp_dir().join(format!("clauracle-ask-test-{}", new_id()));
    std::fs::create_dir_all(d.join(".git")).unwrap();
    let ds = d.to_string_lossy().to_string();
    let shots = vec![(".git/clauracle-att/a.png".to_string(), "first\nsecond".to_string()), (".git/clauracle-att/b.png".to_string(), String::new())];
    let shown = ask_save(&ds, "Do  the\nthing", &shots).unwrap();
    assert!(shown.starts_with(".git/clauracle-att/ask-"));
    assert_eq!(std::fs::read_to_string(d.join(".git/clauracle-att/a.md")).unwrap(), "first\nsecond"); // notes beside the image
    assert!(!d.join(".git/clauracle-att/b.md").exists());
    assert_eq!(ask_of(&ds, 7), None);
    ask_filed(&ds, &shown, "http://x/7");
    let t = std::fs::read_to_string(d.join(ask_of(&ds, 7).unwrap())).unwrap();
    assert_eq!(t, "Do  the\nthing\n\n## Screenshots\n- .git/clauracle-att/a.png\n  first\n  second\n- .git/clauracle-att/b.png\n\nFiled: http://x/7\n");
    let _ = std::fs::remove_dir_all(&d);
}

/// ponytail: minimal standard base64 decoder (no crate for one use); None on a bad char.
pub(crate) fn b64(s: &str) -> Option<Vec<u8>> {
    let (mut out, mut acc, mut bits) = (Vec::with_capacity(s.len() * 3 / 4), 0u32, 0);
    for c in s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let v = match c { b'A'..=b'Z' => c - b'A', b'a'..=b'z' => c - b'a' + 26, b'0'..=b'9' => c - b'0' + 52, b'+' | b'-' => 62, b'/' | b'_' => 63, _ => return None };
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 { bits -= 8; out.push((acc >> bits) as u8); }
    }
    Some(out)
}

#[test]
pub(crate) fn b64_decodes() {
    assert_eq!(b64("aGVsbG8=").unwrap(), b"hello");
    assert_eq!(b64("aGk").unwrap(), b"hi");
    assert!(b64("a$").is_none());
}

/// #309: body minus every `<details><summary>Original ask</summary>…</details>` block; None = nothing to strip.
/// ponytail: unclosed block → cut to end (the quote always sits last).
pub fn scrub_ask(b: &str) -> Option<String> {
    const OPEN: &str = "<details><summary>Original ask</summary>";
    if !b.contains(OPEN) { return None; }
    let mut out = String::new();
    let mut rest = b;
    while let Some(i) = rest.find(OPEN) {
        out += &rest[..i];
        rest = rest[i..].find("</details>").map_or("", |j| &rest[i + j + "</details>".len()..]);
    }
    out += rest;
    Some(out.trim_end().to_string())
}

#[test]
pub(crate) fn scrub_ask_drops_quote() {
    assert_eq!(scrub_ask("Do X.\n\n<details><summary>Original ask</summary>\n\nmy raw words\n</details>\n").unwrap(), "Do X.");
    assert_eq!(scrub_ask("A <details><summary>Original ask</summary>q</details> B").unwrap(), "A  B");
    assert!(scrub_ask("plain").is_none());
}

/// gh issue actions: new (title) | claim | release | done | scrub (number).
pub fn task_do(dir: &str, user: &str, action: &str, arg: &str) -> Result<String, String> { // → gh stdout ("new": issue URL)
    let env = gh_env(user);
    let gh = |a: &[&str]| { let o = run("gh", a, Some(dir), &env); if o.ok { Ok(o.stdout.trim().to_string()) } else { Err(o.stderr.trim().to_string()) } };
    match action {
        "new" => {
            let _ = gh(&["label", "create", OUR_LABEL, "--color", "D77655", "--description", "Tracked in Clauracle"]);
            let (title, body) = split_task(arg);
            let mut a = vec!["issue", "create", "--title", &title, "--body", &body, "--label", OUR_LABEL, "--label", TASK_LABEL];
            let tags = hash_tags(arg); // #71: written #Tags → labels (made if missing; color = gh default)
            for t in &tags { let _ = gh(&["label", "create", t]); a.extend(["--label", t.as_str()]); }
            gh(&a)
        }
        "claim" => {
            let _ = gh(&["label", "create", DOING_LABEL, "--color", "4F5BD5", "--description", "Someone is on it"]);
            gh(&["issue", "edit", arg, "--add-assignee", "@me", "--add-label", DOING_LABEL])
        }
        "release" => gh(&["issue", "edit", arg, "--remove-assignee", "@me", "--remove-label", DOING_LABEL]),
        "done" => { let _ = task_do(dir, user, "scrub", arg); gh(&["issue", "close", arg]) }
        // #309 privacy: closed issue keeps no raw prompt (quoted "Original ask" block dropped); no block → no edit
        "scrub" => {
            let b = gh(&["issue", "view", arg, "--json", "body", "--jq", ".body"])?;
            match scrub_ask(&b) { Some(nb) => gh(&["issue", "edit", arg, "--body", &nb]), None => Ok(String::new()) }
        }
        "accept" => gh(&["issue", "edit", arg, "--remove-label", IDEA_LABEL]), // suggestion → queued task
        "unblock" => gh(&["issue", "edit", arg, "--remove-label", WAIT_LABEL]), // user did their part → back in the queue (old number ∴ near the front)
        "dismiss" => { let _ = task_do(dir, user, "scrub", arg); gh(&["issue", "close", arg, "--reason", "not planned"]) }
        // #112 edit modal: view = issue fields; meta = repo labels + assignable people (page caches); edit = JSON diff
        "view" => gh(&["issue", "view", arg, "--json", "number,title,body,labels,assignees"]),
        "meta" => Ok(serde_json::json!({
            "labels": gh_json(&["label", "list", "--limit", "200", "--json", "name,color"], user, Some(dir)),
            "people": gh_json(&["api", "repos/{owner}/{repo}/assignees?per_page=100", "--jq", "[.[].login]"], user, Some(dir)),
        }).to_string()),
        // #113 collaborators: list (+ role) and pending invites; invite = PUT (gh login chars only ∵ goes into a URL path)
        "collab" => Ok(serde_json::json!({
            "people": gh_json(&["api", "repos/{owner}/{repo}/collaborators?per_page=100", "--jq", "[.[] | {login, role: .role_name}]"], user, Some(dir)),
            "invites": gh_json(&["api", "repos/{owner}/{repo}/invitations?per_page=100", "--jq", "[.[].invitee.login]"], user, Some(dir)),
        }).to_string()),
        "invite" => {
            let u = arg.trim().trim_start_matches('@');
            if u.is_empty() || !u.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') { return Err("not a GitHub username".into()); }
            gh(&["api", "-X", "PUT", &format!("repos/{{owner}}/{{repo}}/collaborators/{u}"), "--silent"])
        }
        "edit" => {
            let v: serde_json::Value = serde_json::from_str(arg).map_err(|e| e.to_string())?;
            let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
            let n = v["n"].as_u64().ok_or("no issue number")?.to_string();
            let (title, body) = (s("title"), s("body"));
            let mut a: Vec<String> = ["issue", "edit", &n, "--title", &title, "--body", &body].map(String::from).to_vec();
            for (k, flag) in [("addL", "--add-label"), ("rmL", "--remove-label"), ("addA", "--add-assignee"), ("rmA", "--remove-assignee")] {
                for x in v[k].as_array().into_iter().flatten().filter_map(|x| x.as_str()) { a.extend([flag.to_string(), x.to_string()]); }
            }
            gh(&a.iter().map(String::as_str).collect::<Vec<_>>())
        }
        _ =>Err("unknown task action".into()),
    }
}

/// Task text → (issue title, body). Short → title only; long → title = lead up to the first ": " / ". " (≤100 chars,
/// else cut at a word + "…"), body = full text ∵ issues came truncated with an empty body.
pub fn split_task(t: &str) -> (String, String) {
    let t = t.trim();
    if let Some((h, _)) = t.split_once('\n') { // multi-line: line 1 = title (#93 card "Title: summary\n\nNote: …")
        if !h.trim().is_empty() && h.chars().count() <= 100 { return (h.trim().to_string(), t.to_string()); }
    }
    if t.chars().count() <= 100 { return (t.to_string(), String::new()); }
    let lead = [": ", ". ", " (", " — "].iter().filter_map(|s| t.find(s)).filter(|&i| i >= 8 && t[..i].chars().count() <= 100).min();
    let title = match lead {
        Some(i) => t[..i].to_string(),
        None => { let c: String = t.chars().take(97).collect(); format!("{}…", c.rsplit_once(' ').map_or(c.as_str(), |(a, _)| a)) }
    };
    (title, t.to_string())
}

/// Issue-mode queue: open `task` issues (from `tasks()`) minus suggestions, ones blocked on the user and ones someone
/// else took. Order: `work` (in progress) first, then the project's `order` (urgent asks are put at its front), then
/// oldest first ∴ outstanding work beats new asks. Same rule as the UI's issueQ().
pub fn task_queue(list: &serde_json::Value, me: &str, order: &[u64], work: u64) -> Vec<serde_json::Value> {
    let me = me.to_lowercase();
    let mut q: Vec<serde_json::Value> = list.as_array().into_iter().flatten().filter(|x| {
        let names = |k: &str, f: &str| x[k].as_array().into_iter().flatten().filter_map(|l| l[f].as_str().map(str::to_lowercase)).collect::<Vec<_>>();
        let who = names("assignees", "login");
        !names("labels", "name").iter().any(|l| l == IDEA_LABEL || l == WAIT_LABEL) && (who.is_empty() || who.contains(&me))
    }).cloned().collect();
    let n = |x: &serde_json::Value| x["number"].as_u64().unwrap_or(0);
    q.sort_by_key(|x| (n(x) != work, order.iter().position(|&o| o == n(x)).unwrap_or(usize::MAX), n(x)));
    q
}

/// `#Critical` → "critical": # after start/space, then a letter (∴ #62 refs, C#, `# heading` skipped); deduped.
pub fn hash_tags(s: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for w in s.split_whitespace().filter_map(|w| w.strip_prefix('#')) {
        let t: String = w.chars().take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect::<String>().to_lowercase();
        if t.starts_with(|c: char| c.is_alphabetic()) && !v.contains(&t) { v.push(t); }
    }
    v
}

#[test]
pub(crate) fn hash_tags_works() {
    assert_eq!(hash_tags("#Critical fix #62, C# and #bug-fix. # head #critical"), ["critical", "bug-fix"]);
}

#[test]
pub(crate) fn task_queue_works() {
    let v = serde_json::json!([
        {"number": 3, "title": "c", "labels": [], "assignees": []},
        {"number": 1, "title": "a", "labels": [{"name": "suggestion"}], "assignees": []},
        {"number": 2, "title": "b", "labels": [], "assignees": [{"login": "Bob"}]},
        {"number": 4, "title": "d", "labels": [], "assignees": [{"login": "Me"}]},
        {"number": 5, "title": "e", "labels": [], "assignees": []},
        {"number": 6, "title": "f", "labels": [{"name": "needs-user"}], "assignees": []}]);
    let nums = |q: Vec<serde_json::Value>| q.iter().map(|x| x["number"].as_u64().unwrap()).collect::<Vec<_>>();
    assert_eq!(nums(task_queue(&v, "me", &[5], 0)), [5, 3, 4]);
    assert_eq!(nums(task_queue(&v, "me", &[], 0)), [3, 4, 5]);
    assert_eq!(nums(task_queue(&v, "me", &[5], 4)), [4, 5, 3]); // in-progress beats order
    assert_eq!(nums(task_queue(&v, "me", &[], 6)), [3, 4, 5]); // blocked on the user → out even if it was the work
    assert!(task_queue(&serde_json::Value::Null, "me", &[], 0).is_empty());
}

#[test]
pub(crate) fn env_put_rereads_and_replaces() {
    let d = std::env::temp_dir().join("clauracle-envput-test");
    let _ = std::fs::create_dir_all(&d);
    let f = d.join(".env");
    std::fs::write(&f, "A=1\nB=2\n").unwrap();
    env_put(&f, &[("B", Some("3")), ("C", Some("4"))]).unwrap();
    let t = std::fs::read_to_string(&f).unwrap();
    assert!(t.contains("A=1") && t.contains("B=3") && t.contains("C=4") && !t.contains("B=2"), "{t}");
    assert!(!d.join(".env.clauracle-tmp").exists());
}

#[test]
pub(crate) fn split_task_works() {
    assert_eq!(split_task("Fix the bug"), ("Fix the bug".into(), String::new()));
    assert_eq!(split_task("Fix it\n\nNote: soon"), ("Fix it".into(), "Fix it\n\nNote: soon".into()));
    let long = format!("Tasks from GitHub Issues by default: {}", "x ".repeat(60));
    assert_eq!(split_task(&long), ("Tasks from GitHub Issues by default".into(), long.trim().into()));
    let (t, b) = split_task(&"word ".repeat(40));
    assert!(t.ends_with('…') && t.chars().count() <= 100 && b.len() > 100);
}

/// Files my work touches: uncommitted changes + commits on this branch since it left the default branch.
pub fn my_files(dir: &str) -> Vec<String> {
    let mut files: Vec<String> = run("git", &["status", "--porcelain"], Some(dir), &[]).stdout.lines()
        .filter_map(|l| l.get(3..)).map(|f| f.rsplit(" -> ").next().unwrap_or(f).trim_matches('"').to_string()).collect();
    for base in ["origin/HEAD", "origin/main", "origin/master"] {
        let o = run("git", &["diff", "--name-only", &format!("{base}...HEAD")], Some(dir), &[]);
        if o.ok {
            files.extend(o.stdout.lines().map(String::from));
            break;
        }
    }
    files.sort();
    files.dedup();
    files
}

/// Someone else's open PRs that change the same files as my work: [{number, title, url, author, files}].
pub fn overlap(mine: &[String], prs: &serde_json::Value, me: &str, branch: &str) -> Vec<serde_json::Value> {
    prs.as_array().into_iter().flatten().filter(|pr| {
        !pr["author"]["login"].as_str().unwrap_or("").eq_ignore_ascii_case(me) && pr["headRefName"] != branch
    }).filter_map(|pr| {
        let both: Vec<&str> = pr["files"].as_array().into_iter().flatten().filter_map(|f| f["path"].as_str())
            .filter(|f| mine.iter().any(|m| m == f)).collect();
        (!both.is_empty()).then(|| serde_json::json!({"number": pr["number"], "title": pr["title"], "url": pr["url"],
                                                       "author": pr["author"]["login"], "files": both}))
    }).collect()
}

pub fn conflicts(dir: &str, user: &str) -> Vec<serde_json::Value> {
    let mine = my_files(dir);
    if mine.is_empty() {
        return vec![];
    }
    let branch = run("git", &["branch", "--show-current"], Some(dir), &[]).stdout.trim().to_string();
    let prs = gh_json(&["pr", "list", "--state", "open", "--limit", "50", "--json", "number,title,url,author,files,headRefName"], user, Some(dir));
    overlap(&mine, &prs, user, &branch)
}

pub fn git_ignored(dir: &str, file: &str) -> Option<bool> {
    if !Path::new(dir).join(".git").exists() { return None; }
    Some(run("git", &["check-ignore", "-q", file], Some(dir), &[]).ok)
}
