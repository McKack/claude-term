//! Git publish/commit/push, build guessing, caches and the storage janitor, release and staging.
use super::*;

/// Is the file ignored by git (so secrets don't get committed)? None outside a repo.
/// A local-only folder becomes a new GitHub repo (`name` or `owner/name`) as `user`: git init if needed,
/// .env* git-ignored before anything is committed, a first commit if there is none, then create + push.
pub fn publish(dir: &str, user: &str, name: &str, private: bool) -> Result<String, String> {
    let env = gh_env(user);
    let git = |args: &[&str]| run("git", args, Some(dir), &env);
    let fail = |o: Out| Err(if o.stderr.trim().is_empty() { o.stdout.trim().to_string() } else { o.stderr.trim().to_string() });
    if !Path::new(dir).join(".git").exists() {
        let o = git(&["init", "-b", "main"]);
        if !o.ok { return fail(o); }
    }
    if git_ignored(dir, ".env") == Some(false) {
        let gi = Path::new(dir).join(".gitignore");
        let mut t = std::fs::read_to_string(&gi).unwrap_or_default();
        if !t.is_empty() && !t.ends_with('\n') { t.push('\n'); }
        std::fs::write(&gi, t + ".env*\n").map_err(|e| e.to_string())?;
    }
    if !git(&["rev-parse", "--verify", "HEAD"]).ok {
        git(&["add", "-A"]);
        let o = git(&["commit", "-m", "Initial commit"]);
        if !o.ok { return fail(o); }
    }
    let vis = if private { "--private" } else { "--public" };
    let o = run("gh", &["repo", "create", name, vis, "--source", ".", "--remote", "origin", "--push"], Some(dir), &env);
    if o.ok { Ok(o.stdout.trim().to_string()) } else { fail(o) }
}

/// Branch actions from the header pill, as the project's gh account: list | switch <b> | new <b> | push.
pub fn git_do(dir: &str, user: &str, action: &str, arg: &str) -> Result<serde_json::Value, String> {
    let env = gh_env(user);
    let git = |args: &[&str]| run("git", args, Some(dir), &env);
    // a branch name never starts with '-' (would read as a flag)
    if arg.starts_with('-') && action != "commit" { return Err("bad branch name".into()); }
    let o = match action {
        // uncommitted/unpushed state for the GitHub tab
        "status" => {
            let files: Vec<String> = git(&["status", "--porcelain"]).stdout.lines().map(|l| l.get(3..).unwrap_or("").to_string()).collect();
            let up = git(&["rev-list", "--count", "@{u}..HEAD"]);
            return Ok(serde_json::json!({"files": files, "ahead": if up.ok { up.stdout.trim().parse::<u32>().unwrap_or(0) } else { 0 },
                                         "upstream": up.ok}));
        }
        // everything, one commit, pushed (`-m` keeps a message starting with '-' a message)
        "commit" => {
            git(&["add", "-A"]);
            let c = git(&["commit", "-m", arg]);
            if !c.ok { return Err(if c.stdout.trim().is_empty() { c.stderr } else { c.stdout }.trim().to_string()); }
            git(&["push", "-u", "origin", "HEAD"])
        }
        "list" => {
            let b = git(&["branch", "--format=%(refname:short)"]).stdout;
            return Ok(serde_json::json!({"current": git(&["branch", "--show-current"]).stdout.trim(),
                                         "branches": b.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>()}));
        }
        "switch" => git(&["switch", arg]),
        "new" => git(&["switch", "-c", arg]),
        "push" => git(&["push", "-u", "origin", "HEAD"]),
        _ => return Err(format!("unknown action {action}")),
    };
    // git writes progress to stderr even on success
    if o.ok { Ok(serde_json::json!(o.stderr.trim())) } else { Err(if o.stderr.trim().is_empty() { o.stdout } else { o.stderr }.trim().to_string()) }
}

/// After a turn: push the current branch if it's ahead of its upstream → Some(n pushed). Never force.
/// ponytail: only branches with an upstream (a never-pushed local branch may be private); failure = Err for a note.
pub fn auto_push(dir: &str, user: &str) -> Result<u32, String> {
    let env = gh_env(user);
    let up = run("git", &["rev-list", "--count", "@{u}..HEAD"], Some(dir), &env);
    let n = if up.ok { up.stdout.trim().parse::<u32>().unwrap_or(0) } else { 0 };
    if n == 0 { return Ok(0); }
    let o = run("git", &["push"], Some(dir), &env);
    if o.ok { Ok(n) } else { Err(o.stderr.trim().lines().last().unwrap_or("").to_string()) }
}

/// Recent build outputs worth releasing (installers, packages, archives) in the usual output folders, newest first.
/// ponytail: fixed folder list + depth 5; add a per-project artifact glob in Build if a project hides them elsewhere
pub fn builds(dir: &str) -> Vec<serde_json::Value> {
    const DIRS: [&str; 9] = ["dist", "build", "out", "release", "publish", "bin", "target/release/bundle", "src-tauri/target/release/bundle", "app/build/outputs"];
    const EXT: [&str; 13] = ["exe", "msi", "msix", "dmg", "pkg", "appimage", "deb", "rpm", "apk", "aab", "ipa", "zip", "tar.gz"];
    fn walk(p: &Path, depth: u8, out: &mut Vec<(std::time::SystemTime, std::path::PathBuf, u64)>) {
        let Ok(rd) = std::fs::read_dir(p) else { return };
        for e in rd.flatten() {
            let path = e.path();
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() { if depth > 0 { walk(&path, depth - 1, out); } continue; }
            let n = path.to_string_lossy().to_lowercase();
            if EXT.iter().any(|x| n.ends_with(&format!(".{x}"))) {
                out.push((m.modified().unwrap_or(std::time::UNIX_EPOCH), path, m.len()));
            }
        }
    }
    let mut out = vec![];
    for d in DIRS { walk(&Path::new(dir).join(d), 5, &mut out); }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out.dedup_by(|a, b| a.1 == b.1);
    out.into_iter().take(12).map(|(t, p, size)| serde_json::json!({"path": p.to_string_lossy(), "name": p.file_name().map(|n| n.to_string_lossy().to_string()),
        "size": size, "mtime": t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)})).collect()
}

// ---- disk: build/cache folders a build regenerates. Safety: only fixed names AND git-ignored (no git → never touched) ----
pub(crate) const CACHE_DIRS: [&str; 17] = ["target", "src-tauri/target", "dist", "build", "out", ".next", ".nuxt", ".turbo", ".parcel-cache",
    ".gradle", "node_modules/.cache", ".pytest_cache", ".mypy_cache", ".ruff_cache", "coverage", "test-results", "playwright-report"];
pub const IDLE_CLEAN_DAYS: u64 = 14; // ponytail: fixed; a setting if someone wants caches kept longer

pub(crate) fn dir_size(p: &Path) -> u64 {
    // symlink_metadata: links/junctions count as themselves, never followed
    std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| match e.path().symlink_metadata() {
        Ok(m) if m.is_dir() => dir_size(&e.path()),
        Ok(m) => m.len(),
        _ => 0,
    }).sum()
}

/// Build tab auto-setup: guess build/run/deploy from project files, no tokens. Most specific first (tauri > npm > cargo…);
/// a field stays empty when nothing fits. ponytail: root files only; monorepos/subfolders → user edits or asks Claude.
pub fn guess_build(dir: &str) -> Build {
    let p = Path::new(dir);
    let read = |f: &str| std::fs::read_to_string(p.join(f)).unwrap_or_default();
    let has = |f: &str| p.join(f).exists();
    let mut b = Build::default();
    let set = |b: &mut Build, build: &str, run: &str, deploy: &str| {
        for (slot, v) in [(&mut b.build, build), (&mut b.run, run), (&mut b.deploy, deploy)] {
            if slot.is_empty() { *slot = v.to_string(); }
        }
    };
    if has("src-tauri/tauri.conf.json") { set(&mut b, "npx tauri build", "npx tauri dev", ""); }
    if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&read("package.json")) {
        let pm = if has("pnpm-lock.yaml") { "pnpm" } else if has("yarn.lock") { "yarn" }
            else if has("bun.lockb") || has("bun.lock") { "bun" } else { "npm" };
        let s = |k: &str| pkg["scripts"].get(k).map(|_| format!("{pm} run {k}")).unwrap_or_default();
        let run = ["dev", "start", "serve"].iter().map(|k| s(k)).find(|c| !c.is_empty()).unwrap_or_default();
        set(&mut b, &s("build"), &run, &s("deploy"));
    }
    if has("Cargo.toml") { set(&mut b, "cargo build --release", "cargo run", ""); }
    if has("go.mod") { set(&mut b, "go build ./...", "go run .", ""); }
    if has("project.godot") { set(&mut b, "", "godot --path .", ""); }
    if let Some(f) = ["main.py", "app.py"].into_iter().find(|f| has(f)) { set(&mut b, "", &format!("python {f}"), ""); }
    let mk = read("Makefile");
    let target = |t: &str| mk.lines().any(|l| l.starts_with(&format!("{t}:")));
    if !mk.is_empty() {
        set(&mut b, "make", if target("run") { "make run" } else { "" }, if target("deploy") { "make deploy" } else { "" });
    }
    b
}

/// HEAD sha ("" outside git / no commits).
pub fn head(dir: &str) -> String {
    run("git", &["rev-parse", "HEAD"], Some(dir), &[]).stdout.trim().to_string()
}

/// #89 smart build: why the changes since `base` (sha; "" = HEAD) deserve build + run, "" = skip.
pub fn smart_why(dir: &str, base: &str) -> String {
    let base = if base.is_empty() { "HEAD" } else { base };
    let stat = run("git", &["diff", "--numstat", base], Some(dir), &[]).stdout;
    let log = run("git", &["log", "--format=%s", &format!("{base}..HEAD")], Some(dir), &[]).stdout;
    smart_pick(&stat, &log)
}

/// Not code: docs and meta files (smart build skip, #92 stale-build check).
pub fn is_doc(f: &str) -> bool { f.starts_with("docs/") || [".md", ".txt", ".rst", ".adoc", "LICENSE", ".gitignore"].iter().any(|x| f.ends_with(x)) }

/// #92: unix time of the newest commit touching code (≠ docs, ≠ lock files: the post-build commit only bumps Cargo.lock…), 0 = none.
/// ponytail: last 200 commits; older code change → 0 → no stale warning.
pub fn code_time(dir: &str) -> u64 {
    let lock = |f: &str| f.ends_with(".lock") || f.ends_with("-lock.json") || f.ends_with("-lock.yaml");
    let out = run("git", &["log", "-n", "200", "--format=%x01%ct", "--name-only"], Some(dir), &[]).stdout;
    out.split('\u{1}').find_map(|c| { let mut l = c.lines(); let t = l.next()?.trim().parse::<u64>().ok()?;
        l.any(|f| !f.is_empty() && !is_doc(f) && !lock(f)).then_some(t) }).unwrap_or(0)
}

/// Doc-only → skip; a fix (commit subject words) → build; else ≥30 changed lines or ≥3 code files.
/// ponytail: heuristic on numstat + subjects; untracked files unseen (Claude commits). Tune thresholds if it nags.
pub fn smart_pick(numstat: &str, log: &str) -> String {
    let (mut files, mut lines) = (0u64, 0u64);
    for l in numstat.lines() {
        let c: Vec<&str> = l.split('\t').collect();
        if c.len() < 3 || is_doc(c[2]) { continue; }
        files += 1;
        lines += c[0].parse::<u64>().unwrap_or(1) + c[1].parse::<u64>().unwrap_or(0); // binary "-" = 1
    }
    if files == 0 { return String::new(); }
    let low = log.to_lowercase();
    if ["fix", "crash", "critical", "broken", "bug", "regression"].iter().any(|w| low.contains(w)) {
        return format!("a fix in {files} code file{}", if files > 1 { "s" } else { "" });
    }
    if lines >= 30 || files >= 3 { format!("{lines} changed lines in {files} code files") } else { String::new() }
}

#[test]
pub(crate) fn smart_pick_works() {
    assert_eq!(smart_pick("3\t1\tREADME.md\n200\t0\tdocs/a.html\n", "Fix typo"), "");
    assert_eq!(smart_pick("2\t1\tsrc/a.rs\n", "Tweak label"), "");
    assert!(!smart_pick("2\t1\tsrc/a.rs\n", "Fix crash on open").is_empty());
    assert!(!smart_pick("25\t10\tsrc/a.rs\n", "Add thing").is_empty());
    assert!(!smart_pick("1\t0\ta.rs\n1\t0\tb.rs\n-\t-\tc.png\n", "x").is_empty());
}

/// (folder relative to the project, bytes) for each cache folder present and git-ignored.
pub fn caches(dir: &str) -> Vec<(String, u64)> {
    CACHE_DIRS.iter().filter(|d| Path::new(dir).join(d).symlink_metadata().map_or(false, |m| m.is_dir()))
        .filter(|d| run("git", &["check-ignore", "-q", d], Some(dir), &[]).ok)
        .map(|d| (d.to_string(), dir_size(&Path::new(dir).join(d)))).collect()
}

/// Delete the cache folders named in `which` (empty = all); bytes freed. Re-checked here: the page isn't trusted with paths.
pub fn clean(dir: &str, which: &[String]) -> u64 {
    caches(dir).into_iter().filter(|(d, _)| which.is_empty() || which.contains(d))
        .filter(|(d, _)| std::fs::remove_dir_all(Path::new(dir).join(d)).is_ok()).map(|(_, n)| n).sum()
}

/// Days since the project's git index changed (add/commit/checkout); None without git.
pub fn idle_days(dir: &str) -> Option<u64> {
    let m = std::fs::metadata(Path::new(dir).join(".git/index")).ok()?.modified().ok()?;
    Some(m.elapsed().ok()?.as_secs() / 86400)
}

// ---- Storage Janitor (#26): startup + every JANITOR_EVERY. Rules: idle ≥ IDLE_CLEAN_DAYS → clean; caches > CACHE_CAP → clean;
// cap skips projects touched today (#118: wiped the live Tauri target/ mid-dogfood); drive low (free < min(10 GB, 10%)) → clean most-idle projects first till not low, then old Claude transcripts. ----
pub const JANITOR_EVERY: u64 = 6 * 3600;
pub(crate) const CACHE_CAP: u64 = 10 << 30; // ponytail: fixed caps; settings if someone needs other numbers
pub(crate) const GB: u64 = 1 << 30;

/// (free, total) bytes of the drive holding `dir`.
#[cfg(windows)]
pub fn disk_free(dir: &str) -> Option<(u64, u64)> {
    use std::os::windows::ffi::OsStrExt;
    let w: Vec<u16> = std::ffi::OsStr::new(dir).encode_wide().chain([0]).collect();
    let (mut free, mut total) = (0u64, 0u64);
    // SAFETY: w is NUL-terminated; out-params are valid u64s
    let ok = unsafe { windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(w.as_ptr(), &mut free, &mut total, std::ptr::null_mut()) };
    (ok != 0).then_some((free, total))
}
#[cfg(not(windows))]
pub fn disk_free(dir: &str) -> Option<(u64, u64)> {
    let o = run("df", &["-Pk", dir], None, &[]); // POSIX: 2nd line = fs total used avail …, in KiB
    let f: Vec<u64> = o.stdout.lines().nth(1)?.split_whitespace().skip(1).take(3).filter_map(|x| x.parse().ok()).collect();
    (f.len() == 3).then(|| (f[2] * 1024, f[0] * 1024))
}
pub fn disk_low(dir: &str) -> bool { disk_free(dir).is_some_and(|(f, t)| f < (10 * GB).min(t / 10)) } // #118: was max → 1 TB drive w/ 101 GB free = "low" → wiped builds

/// One janitor pass over (path, turn running → left alone); keep = session ids Clauracle resumes. Returns bytes freed.
pub fn janitor(projects: &[(String, bool)], keep_sessions: &[String]) -> u64 {
    let mut freed = 0;
    let mut idle: Vec<(u64, &String)> = projects.iter().filter(|(_, busy)| !busy).map(|(d, _)| (idle_days(d).unwrap_or(0), d)).collect();
    idle.sort_by(|a, b| b.0.cmp(&a.0)); // most idle first
    for (days, d) in &idle {
        let total: u64 = caches(d).iter().map(|c| c.1).sum();
        if total > 0 && (*days >= IDLE_CLEAN_DAYS || (*days > 0 && total > CACHE_CAP) || disk_low(d)) { freed += clean(d, &[]); }
    }
    let Some(home) = dirs::home_dir() else { return freed };
    let low = disk_low(&home.to_string_lossy());
    freed + claude_clean(&home, keep_sessions, low) + tmp_clean(&std::env::temp_dir().join("claude"), keep_sessions)
}

/// Newest file time anywhere under `p` (a folder's own time misses edits deeper down); an empty folder counts by its own time.
pub(crate) fn newest(p: &Path) -> std::time::SystemTime {
    let own = p.symlink_metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
    std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| match e.path().symlink_metadata() {
        Ok(m) if m.is_dir() => newest(&e.path()),
        Ok(m) => m.modified().unwrap_or(own),
        _ => own,
    }).max().unwrap_or(own)
}

/// Claude Code's temp folder (scratchpads, diffs, probes): entries untouched for TMP_DAYS go. Hex-named folders hold
/// one folder per session: those are judged one by one, and a session Clauracle resumes is kept whatever its age.
pub(crate) const TMP_DAYS: u64 = 3;
pub(crate) fn tmp_clean(root: &Path, keep: &[String]) -> u64 {
    let stale = |p: &Path| newest(p).elapsed().is_ok_and(|e| e.as_secs() > TMP_DAYS * 86400);
    let gone = |p: &Path| { let n = dir_size(p) + if p.is_file() { p.metadata().map_or(0, |m| m.len()) } else { 0 };
        let ok = if p.is_dir() { std::fs::remove_dir_all(p).is_ok() } else { std::fs::remove_file(p).is_ok() }; if ok { n } else { 0 } };
    let mut freed = 0;
    for e in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let (p, name) = (e.path(), e.file_name().to_string_lossy().to_string());
        if p.is_dir() && name.len() == 16 && name.bytes().all(|b| b.is_ascii_hexdigit()) {
            for s in std::fs::read_dir(&p).into_iter().flatten().flatten().map(|e| e.path()) {
                let id = s.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if !keep.contains(&id) && stale(&s) { freed += gone(&s); }
            }
        } else if stale(&p) { freed += gone(&p); }
    }
    freed
}

/// Claude Code leftovers: installer versions but the newest (it keeps several, ~300 MB each); when the home drive is low,
/// transcripts (projects/*/<uuid>.jsonl + <uuid>/ dir) untouched 14+ days that no Clauracle project resumes.
pub(crate) fn claude_clean(home: &Path, keep: &[String], low: bool) -> u64 {
    let mut freed = 0;
    let mut vs: Vec<_> = std::fs::read_dir(home.join(".local/share/claude/versions")).into_iter().flatten().flatten()
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path()))).collect();
    vs.sort();
    vs.pop(); // newest stays (a running older one fails to delete on Windows: fine)
    for (_, p) in vs { let n = p.metadata().map_or(0, |m| m.len()); if std::fs::remove_file(&p).is_ok() { freed += n; } }
    if !low { return freed; }
    let old = |p: &Path| p.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|e| e.as_secs() > 14 * 86400);
    for proj in std::fs::read_dir(home.join(".claude/projects")).into_iter().flatten().flatten() {
        for f in std::fs::read_dir(proj.path()).into_iter().flatten().flatten().map(|e| e.path()) {
            let Some(id) = f.file_stem().map(|s| s.to_string_lossy().to_string()) else { continue };
            if f.extension().is_none_or(|x| x != "jsonl") || keep.contains(&id) || !old(&f) { continue; }
            let n = f.metadata().map_or(0, |m| m.len());
            if std::fs::remove_file(&f).is_ok() { freed += n; }
            let sub = proj.path().join(&id); // subagent transcripts / tool results of that session
            if sub.is_dir() { freed += dir_size(&sub); let _ = std::fs::remove_dir_all(sub); }
        }
    }
    freed
}

/// GitHub release `tag` with `files` attached and notes generated from the commits since the last one.
pub fn release(dir: &str, user: &str, tag: &str, files: &[String], ci: bool, main: bool, sha: Option<&str>) -> Result<String, String> {
    // #266 sha = staged build's commit (an ancestor of HEAD, on the remote once HEAD is pushed); None = HEAD
    if tag.is_empty() || tag.starts_with('-') { return Err("bad tag".into()); }
    // #119: tag THIS commit (pushed first); w/o --target gh tags the default branch → v0.1.2 got built from 0.1.1 code
    // #154: never attach another version's bundle (v0.1.2 shipped 0.1.1 files; a slow/old build keeps its old name)
    let bad = wrong_version(tag, files);
    if !bad.is_empty() { return Err(format!("not built as {tag}: {} (rebuild first)", bad.join(", "))); }
    let env = gh_env(user);
    // #130: CI off → repo var gate (workflow `if: vars.CLAURACLE_CI != 'off'`); on → drop it. Errors ignored (no admin = no gate)
    if ci { run("gh", &["variable", "delete", "CLAURACLE_CI"], Some(dir), &env); }
    else { run("gh", &["variable", "set", "CLAURACLE_CI", "--body", "off"], Some(dir), &env); }
    let push = run("git", &["push", "-q", "origin", "HEAD"], Some(dir), &env);
    if !push.ok { return Err(format!("push failed: {}", push.stderr.trim())); }
    // #210 RC/Published: default branch fast-forwards to this commit (plain push = ff only, never forced) → main = what shipped
    if main {
        let def = run("git", &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"], Some(dir), &[]).stdout.trim().trim_start_matches("origin/").to_string();
        let def = if def.is_empty() { gh_json(&["repo", "view", "--json", "defaultBranchRef"], user, Some(dir))["defaultBranchRef"]["name"]
            .as_str().unwrap_or_default().to_string() } else { def };
        let cur = run("git", &["rev-parse", "--abbrev-ref", "HEAD"], Some(dir), &[]).stdout.trim().to_string();
        if !def.is_empty() && def != cur {
            let m = run("git", &["push", "-q", "origin", &format!("{}:refs/heads/{def}", sha.unwrap_or("HEAD"))], Some(dir), &env);
            if !m.ok { return Err(format!("couldn't move {def} to this release ({cur} is behind {def}: merge {def} into {cur} first): {}", m.stderr.trim())); }
        }
    }
    let sha = sha.map_or_else(|| run("git", &["rev-parse", "HEAD"], Some(dir), &[]).stdout.trim().to_string(), str::to_string);
    let mut a = vec!["release", "create", tag, "--title", tag, "--generate-notes", "--target", &sha];
    a.extend(files.iter().map(String::as_str));
    let o = run("gh", &a, Some(dir), &env);
    if o.ok && !ci {
        // fallback for workflows w/o the gate: cancel runs the tag push started (they appear after a few s)
        let (dir, tag) = (dir.to_string(), tag.to_string());
        std::thread::spawn(move || for _ in 0..6 {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let l = run("gh", &["run", "list", "--branch", &tag, "--event", "push", "--json", "databaseId,status", "--jq",
                ".[] | select(.status != \"completed\") | .databaseId"], Some(&dir), &env);
            for id in l.stdout.split_whitespace() { run("gh", &["run", "cancel", id], Some(&dir), &env); }
        });
    }
    if o.ok { Ok(o.stdout.trim().to_string()) } else { Err(o.stderr.trim().to_string()) }
}

/// #266 staged releases: .git/clauracle-stage/<v>/ = copies of a full build + stage.json {v, sha, t, files, src}.
/// Copies ∵ later builds/cargo clean/Janitor must not touch them. Actions: list | add (v, files; sha = HEAD) | done (drop copies) | skip (copies + originals).
/// ponytail: sha = HEAD at staging (UI stages only non-stale builds); uncommitted edits at build time aren't in that sha
pub fn stage(dir: &str, action: &str, v: &str, files: &[String]) -> Result<serde_json::Value, String> {
    let git = Path::new(dir).join(".git");
    if !git.is_dir() { return if action == "list" { Ok(serde_json::json!([])) } else { Err("not a git repo".into()) }; } // no git = no GitHub release
    let root = git.join("clauracle-stage");
    if !v.is_empty() && (v.contains(['/', '\\']) || v.contains("..")) { return Err("bad version".into()); }
    let d = root.join(v);
    let meta = |d: &Path| std::fs::read_to_string(d.join("stage.json")).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
    match action {
        "list" => Ok(std::fs::read_dir(&root).into_iter().flatten().flatten().filter_map(|e| meta(&e.path())).collect()),
        "add" => {
            if v.is_empty() || files.is_empty() { return Err("nothing to stage".into()); }
            std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
            let mut out = vec![];
            for f in files {
                let to = d.join(Path::new(f).file_name().ok_or("bad file")?);
                std::fs::copy(f, &to).map_err(|e| format!("{f}: {e}"))?;
                out.push(to.to_string_lossy().to_string());
            }
            let sha = run("git", &["rev-parse", "HEAD"], Some(dir), &[]).stdout.trim().to_string();
            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let m = serde_json::json!({"v": v, "sha": sha, "t": t, "files": out, "src": files});
            std::fs::write(d.join("stage.json"), m.to_string()).map_err(|e| e.to_string())?;
            Ok(m)
        }
        "done" | "skip" => {
            if v.is_empty() { return Err("bad version".into()); }
            if action == "skip" { for f in meta(&d).and_then(|m| m["src"].as_array().cloned()).unwrap_or_default() { if let Some(f) = f.as_str() { let _ = std::fs::remove_file(f); } } }
            std::fs::remove_dir_all(&d).map_err(|e| e.to_string())?;
            Ok(serde_json::Value::Null)
        }
        _ => Err("bad action".into()),
    }
}

#[test]
pub(crate) fn stage_add_list_skip() {
    let d = std::env::temp_dir().join(format!("clstage{}", new_id()));
    std::fs::create_dir_all(d.join(".git")).unwrap();
    let f = d.join("App_0.1.4_x64-setup.exe"); std::fs::write(&f, "x").unwrap();
    let ds = d.to_string_lossy().to_string();
    assert!(stage(&ds, "add", "../x", &[]).is_err());
    let m = stage(&ds, "add", "0.1.4", &[f.to_string_lossy().to_string()]).unwrap();
    assert!(Path::new(m["files"][0].as_str().unwrap()).exists());
    assert_eq!(stage(&ds, "list", "", &[]).unwrap().as_array().unwrap().len(), 1);
    stage(&ds, "skip", "0.1.4", &[]).unwrap();
    assert!(!f.exists() && stage(&ds, "list", "", &[]).unwrap().as_array().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

/// #154 files whose name carries a version (`Name_1.2.3_arch.ext`, Tauri's scheme) ≠ tag's. Unversioned names pass
/// (`Name_universal.app.tar.gz`). ponytail: name-based; a renamed file slips through
pub fn wrong_version(tag: &str, files: &[String]) -> Vec<String> {
    let want = tag.trim_start_matches('v');
    files.iter().filter_map(|f| {
        let n = Path::new(f).file_name()?.to_string_lossy().to_string();
        let v = n.split('_').find(|s| s.contains('.') && s.split('.').all(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit())))?;
        (v != want).then_some(n)
    }).collect()
}

#[test]
pub(crate) fn release_skips_other_versions() {
    let f = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert!(wrong_version("v0.1.4", &f(&["a/Clauracle_0.1.4_x64-setup.exe", "Clauracle_universal.app.tar.gz"])).is_empty());
    assert_eq!(wrong_version("v0.1.2", &f(&["b/Clauracle_0.1.1_amd64.deb", "Clauracle_0.1.2_amd64.deb"])), ["Clauracle_0.1.1_amd64.deb"]);
}

// ---- Clauracle's issues: label `clauracle` (= ours, ≠ hand-filed repo issues) + a type label (task/feature/fix/
// suggestion); taken = assignee + `in progress` (no commits, all live on GitHub) ----
