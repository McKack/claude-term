//! Processes and paths: PATH lookup, running tools, gh accounts and tokens, git remotes, project folders.
use super::*;

#[cfg(windows)]
pub(crate) const NO_WINDOW: u32 = 0x0800_0000;

pub fn bin_dir() -> PathBuf {
    home().join("bin") // where Clauracle puts a gh it downloaded
}

/// `claude`/`gh` on PATH, or where their installers put them (a .app doesn't get the login shell's PATH).
pub fn which(name: &str) -> Option<PathBuf> {
    let h = dirs::home_dir().unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    dirs.extend([bin_dir(), h.join(".local/bin"), h.join(".claude/local"), h.join(".npm-global/bin"),
                 h.join("google-cloud-sdk/bin"), dirs::data_local_dir().unwrap_or_default().join("Google/Cloud SDK/google-cloud-sdk/bin"),
                 "C:/Program Files (x86)/Google/Cloud SDK/google-cloud-sdk/bin".into(), "/opt/homebrew/bin".into(), "/usr/local/bin".into(), "/usr/bin".into(),
                 "C:/Program Files/Git/usr/bin".into()]); // last: Git for Windows' openssl (Mac signing)
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ".bat"] } else { &[""] };
    dirs.iter().flat_map(|d| exts.iter().map(move |e| d.join(format!("{name}{e}")))).find(|p| p.is_file())
}

/// #88: app launched from Start/Finder lacks shell PATH additions (rustup's ~/.cargo/bin…) → build steps/terminals
/// can't find cargo. Append existing toolchain dirs once at startup (before threads) → every child inherits.
pub fn fix_path() {
    let h = dirs::home_dir().unwrap_or_default();
    let mut p: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    for d in [h.join(".cargo/bin"), h.join(".local/bin"), h.join(".bun/bin"), h.join("go/bin"),
              "/opt/homebrew/bin".into(), "/usr/local/bin".into()] {
        if d.is_dir() && !p.contains(&d) { p.push(d); }
    }
    if let Ok(v) = std::env::join_paths(p) { std::env::set_var("PATH", v); }
}

/// Toolchains for local builds (#28): id → found. Only ones this OS can use; other OSes build in CI (#27).
/// ponytail: presence only, no version checks; Android paths = Android Studio defaults.
pub fn deps() -> serde_json::Value {
    let h = dirs::home_dir().unwrap_or_default();
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from).filter(|p| p.is_dir());
    let sdk = env("ANDROID_HOME").or_else(|| env("ANDROID_SDK_ROOT")).or_else(|| [dirs::data_local_dir().unwrap_or_default().join("Android/Sdk"),
        h.join("Library/Android/sdk"), h.join("Android/Sdk")].into_iter().find(|p| p.is_dir()));
    let ndk = env("NDK_HOME").is_some() || sdk.as_ref().is_some_and(|s| std::fs::read_dir(s.join("ndk")).is_ok_and(|mut r| r.next().is_some()));
    let mut m = serde_json::json!({"node": which("node").is_some(), "rust": which("cargo").is_some() || h.join(".cargo/bin").is_dir(),
        "jdk": which("java").is_some() || env("JAVA_HOME").is_some(), "android_sdk": sdk.is_some(), "android_ndk": ndk});
    if cfg!(target_os = "macos") { m["xcode"] = run("xcode-select", &["-p"], None, &[]).ok.into(); }
    if cfg!(target_os = "linux") { m["webkit"] = run("pkg-config", &["--exists", "webkit2gtk-4.1"], None, &[]).ok.into(); }
    if cfg!(windows) { m["wsl"] = wsl_ok().into(); } // #131 Linux builds via WSL
    m
}

/// #221 Hot files: project files (git ls-files, else a walk; ≤ 3000) + size + 30-day churn from `git log --numstat`.
/// → {files: [[path, bytes, churn, [commit idx ≤ 8, newest first]]], commits: [[hash, unix secs, subject]]}.
/// ponytail: --no-renames (rename = delete + add); churn = added + deleted lines, binary = 0.
pub fn file_heat(dir: &str) -> serde_json::Value {
    let root = Path::new(dir);
    let o = run("git", &["-c", "core.quotePath=false", "ls-files", "-co", "--exclude-standard"], Some(dir), &[]);
    let mut files: Vec<String> = if o.ok { o.stdout.lines().map(String::from).collect() } else {
        fn walk(root: &Path, p: &Path, out: &mut Vec<String>) {
            let Ok(rd) = std::fs::read_dir(p) else { return };
            for e in rd.flatten() {
                let (path, n) = (e.path(), e.file_name().to_string_lossy().to_string());
                if out.len() >= 3000 || n.starts_with('.') { continue; }
                if path.is_dir() { if !["node_modules", "target", "dist", "build", "__pycache__", "venv"].contains(&n.as_str()) { walk(root, &path, out); } }
                else if let Ok(r) = path.strip_prefix(root) { out.push(r.to_string_lossy().replace('\\', "/")); }
            }
        }
        let mut v = vec![]; walk(root, root, &mut v); v
    };
    files.truncate(3000);
    let idx: HashMap<&str, usize> = files.iter().enumerate().map(|(i, f)| (f.as_str(), i)).collect();
    let mut heat = vec![(0u64, Vec::<usize>::new()); files.len()];
    let mut commits = vec![];
    let log = run("git", &["-c", "core.quotePath=false", "log", "--since=30.days", "--numstat", "--no-renames", "--format=%x01%h%x09%ct%x09%s"], Some(dir), &[]);
    for l in log.stdout.lines() {
        if let Some(h) = l.strip_prefix('\u{1}') {
            let mut p = h.splitn(3, '\t');
            commits.push(serde_json::json!([p.next(), p.next().and_then(|t| t.parse::<i64>().ok()), p.next()])); continue;
        }
        let mut p = l.splitn(3, '\t');
        let (Some(a), Some(d), Some(f), Some(c)) = (p.next(), p.next(), p.next(), commits.len().checked_sub(1)) else { continue };
        let Some(&i) = idx.get(f) else { continue };
        heat[i].0 += a.parse::<u64>().unwrap_or(0) + d.parse::<u64>().unwrap_or(0);
        if heat[i].1.len() < 8 { heat[i].1.push(c); }
    }
    // #283 [4] = mtime unix secs (Start menu search rows)
    let out: Vec<_> = files.iter().zip(heat).map(|(f, (ch, cs))| { let m = std::fs::metadata(root.join(f)).ok();
        serde_json::json!([f, m.as_ref().map_or(0, |m| m.len()), ch, cs, m.and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs())]) }).collect();
    serde_json::json!({"files": out, "commits": commits})
}

/// #131: WSL has a real distro (docker-desktop* = Docker's own, can't build). `wsl -l -q` prints UTF-16 → strip NULs.
/// #136: also Node+cargo+WebKitGTK inside the default distro (login shell ∵ rustup PATH in ~/.profile); missing → deps row offers install.
/// ponytail: builds run in the default distro; a docker-desktop default would fail the inner check (= reads as missing)
pub fn wsl_ok() -> bool {
    let o = run("wsl", &["-l", "-q"], None, &[]);
    o.ok && o.stdout.replace('\0', "").lines().any(|l| { let l = l.trim(); !l.is_empty() && !l.starts_with("docker-desktop") })
        && run("wsl", &["-e", "bash", "-lc", "command -v node && command -v cargo && pkg-config --exists webkit2gtk-4.1"], None, &[]).ok
}

/// #131 bash for a Linux build in WSL: copy the project (no node_modules/target/.git) to ~/.clauracle-build/<pid>, build there
/// (Windows node_modules hold win32 binaries; sharing target/ would thrash), copy .AppImage/.deb/.rpm back to the same paths.
/// ponytail: tar copy never deletes files removed in the project; non-target/release/bundle outputs aren't copied back
pub fn wsl_script(pid: &str, cmd: &str) -> String {
    let pid: String = pid.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    format!("set -e; s=\"$PWD\"; d=\"$HOME/.clauracle-build/{pid}\"; mkdir -p \"$d\"; \
        tar -C \"$s\" --exclude=./node_modules --exclude=./target --exclude=./src-tauri/target --exclude=./.git -cf - . | tar -C \"$d\" -xf -; cd \"$d\"; \
        if [ -f package.json ]; then npm install --no-audit --no-fund; fi; {cmd}; \
        find . -path '*/target/release/bundle/*' -type f \\( -name '*.AppImage' -o -name '*.deb' -o -name '*.rpm' \\) -not -path './node_modules/*' -exec cp --parents {{}} \"$s/\" \\;")
}

pub fn command(program: &Path) -> Command {
    // .cmd shims (npm installs) must go through cmd
    let mut c = if program.extension().map_or(false, |e| e == "cmd" || e == "bat") {
        let mut c = Command::new("cmd");
        c.arg("/c").arg(program);
        c
    } else {
        Command::new(program)
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(NO_WINDOW);
    }
    c
}

pub struct Out {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

pub fn run(name: &str, args: &[&str], cwd: Option<&str>, env: &[(String, String)]) -> Out {
    let Some(p) = which(name) else {
        return Out { ok: false, stdout: String::new(), stderr: format!("{name} not found") };
    };
    let mut c = command(&p);
    c.args(args).envs(env.iter().cloned());
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    match c.output() {
        Ok(o) => Out { ok: o.status.success(), stdout: String::from_utf8_lossy(&o.stdout).into(),
                       stderr: String::from_utf8_lossy(&o.stderr).into() },
        Err(e) => Out { ok: false, stdout: String::new(), stderr: e.to_string() },
    }
}

// ---- GitHub: every account's token is handed to whoever needs it; nobody touches `gh auth switch` ----

pub(crate) static GH_USERS: Mutex<Option<Vec<String>>> = Mutex::new(None);

/// gh_accounts, remembered: `gh auth status` checks every account with GitHub (~1-2s). Forgotten on login/logout.
pub fn gh_users() -> Vec<String> {
    if let Some(u) = GH_USERS.lock().unwrap().clone() {
        return u;
    }
    let u = gh_accounts();
    *GH_USERS.lock().unwrap() = Some(u.clone());
    u
}

pub fn forget_gh_users() {
    *GH_USERS.lock().unwrap() = None;
}

pub fn gh_accounts() -> Vec<String> {
    let o = run("gh", &["auth", "status", "--json", "hosts"], None, &[]);
    serde_json::from_str::<serde_json::Value>(&o.stdout).ok()
        .and_then(|v| v["hosts"]["github.com"].as_array().cloned()).unwrap_or_default().iter()
        .filter(|a| a["state"] == "success").filter_map(|a| a["login"].as_str().map(String::from)).collect()
}

/// GH_TOKEN for gh, and git told to ask gh for github.com credentials (which then uses that same token).
pub fn gh_env(user: &str) -> Vec<(String, String)> {
    if user.is_empty() {
        return vec![];
    }
    let tok = run("gh", &["auth", "token", "-h", "github.com", "-u", user], None, &[]).stdout.trim().to_string();
    if tok.is_empty() {
        return vec![];
    }
    let mut env: Vec<(String, String)> = [("GH_TOKEN", tok.as_str()), ("GIT_CONFIG_COUNT", "2"),
     ("GIT_CONFIG_KEY_0", "credential.https://github.com.helper"), ("GIT_CONFIG_VALUE_0", ""),
     ("GIT_CONFIG_KEY_1", "credential.https://github.com.helper"), ("GIT_CONFIG_VALUE_1", "!gh auth git-credential")]
        .iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    // commit identity = this account (env beats .git/config user.email, which GitHub may map to another account)
    if let Some(email) = gh_noreply(user, &env) {
        for k in ["GIT_AUTHOR_NAME", "GIT_COMMITTER_NAME"] { env.push((k.into(), user.into())); }
        for k in ["GIT_AUTHOR_EMAIL", "GIT_COMMITTER_EMAIL"] { env.push((k.into(), email.clone())); }
    }
    env
}

pub(crate) static GH_NOREPLY: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// `{id}+{login}@users.noreply.github.com`: GitHub always attributes it to exactly this account. Cached per login.
pub(crate) fn gh_noreply(user: &str, env: &[(String, String)]) -> Option<String> {
    if let Some((_, e)) = GH_NOREPLY.lock().unwrap().iter().find(|(u, _)| u == user) {
        return Some(e.clone());
    }
    let id = run("gh", &["api", "user", "--jq", ".id"], None, env).stdout.trim().to_string();
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let e = format!("{id}+{user}@users.noreply.github.com");
    GH_NOREPLY.lock().unwrap().push((user.into(), e.clone()));
    Some(e)
}

pub fn gh_json(args: &[&str], user: &str, cwd: Option<&str>) -> serde_json::Value {
    let o = run("gh", args, cwd, &gh_env(user));
    if o.ok { serde_json::from_str(&o.stdout).unwrap_or(serde_json::Value::Null) } else { serde_json::Value::Null }
}

pub fn git_branch(cwd: &str) -> String {
    let o = run("git", &["status", "--porcelain", "-b"], Some(cwd), &[]);
    let lines: Vec<&str> = o.stdout.lines().collect();
    let Some(first) = lines.first() else { return String::new() };
    let branch = first.trim_start_matches("## ").split("...").next().unwrap_or("").to_string();
    if lines.len() > 1 { format!("{branch} · {} changed", lines.len() - 1) } else { branch }
}

/// owner, repo from a GitHub remote url (https or ssh)
pub fn parse_remote(url: &str) -> Option<(String, String)> {
    let re = Regex::new(r"github\.com[:/]([^/\s]+)/([^/\s]+?)(?:\.git)?/?$").unwrap();
    re.captures(url.trim()).map(|c| (c[1].to_string(), c[2].to_string()))
}

pub fn repo_of(cwd: &str) -> Option<(String, String)> {
    parse_remote(&run("git", &["remote", "get-url", "origin"], Some(cwd), &[]).stdout)
}

// ---- organize (#35): every project under root as <owner>/<repo> (GitHub origin) or local/<name> ----

/// Where `dir` belongs under `root`: SDK/tool → dependencies/<name>; GitHub clone named like its repo → <owner>/<repo>;
/// else local/<name> (#51: a renamed clone, e.g. AseShop from aseprite/aseprite = own project on someone's code).
pub fn place_of(root: &Path, dir: &str) -> PathBuf {
    let name = Path::new(dir).file_name().unwrap_or_default();
    if is_dep(Path::new(dir)) { return root.join("dependencies").join(name); }
    match repo_of(dir) {
        Some((o, r)) if r.eq_ignore_ascii_case(&name.to_string_lossy()) => root.join(o).join(r),
        _ => root.join("local").join(name),
    }
}

/// #51 SDK / tool install, not a project. ponytail: known names + "no project file and only binaries/one versioned
/// folder/bin"; misses → user's Move to….
pub fn is_dep(d: &Path) -> bool {
    const KNOWN: &[&str] = &["android-sdk", "flutter", "jdk", "ghidra", "nuget", "vcpkg", "emsdk", "depot_tools", "gradle", "maven",
        "llvm", "mingw64", "msys64", "cmake", "ue5", "unreal", "dotnet", ".gradle", ".pub-cache"];
    let n = d.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
    if KNOWN.contains(&n.as_str()) || n.starts_with("jdk-") || n.starts_with("android-sdk") { return true; }
    if d.join(".git").exists() { return false; }
    let kids: Vec<PathBuf> = std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path()).collect();
    const PROJ: &[&str] = &["package.json", "cargo.toml", "pyproject.toml", "go.mod", "cmakelists.txt", "makefile", "pubspec.yaml",
        "build.gradle", "build.gradle.kts", "src", "readme.md", "index.html"];
    let lower = |p: &PathBuf| p.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
    if kids.is_empty() || kids.iter().any(|k| { let l = lower(k); PROJ.contains(&l.as_str()) || [".sln", ".csproj", ".uproject"].iter().any(|e| l.ends_with(e)) }) { return false; }
    let ver = Regex::new(r"\d+\.\d+").unwrap();
    let bin = |k: &PathBuf| k.is_file() && [".exe", ".dll", ".zip", ".nupkg", ".msi", ".jar"].iter().any(|e| lower(k).ends_with(e));
    kids.iter().all(bin) || (kids.len() == 1 && kids[0].is_dir() && ver.is_match(&lower(&kids[0]))) || d.join("bin").is_dir()
}
pub fn same_path(a: &Path, b: &Path) -> bool {
    let n = |p: &Path| { let s = p.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_string(); if cfg!(windows) { s.to_lowercase() } else { s } };
    n(a) == n(b)
}

/// Folders under `root` that look like projects: git repos (depth ≤ 3, not descended into) + plain folders straight in
/// root or root/local. Owner folders (holding repos) are containers, not projects. ponytail: depth cap 3.
pub fn scan_projects(root: &Path) -> Vec<PathBuf> {
    fn walk(d: &Path, depth: u8, root: &Path, out: &mut Vec<PathBuf>) -> bool { // → found a repo below
        let kids: Vec<PathBuf> = std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path())
            .filter(|p| p.symlink_metadata().is_ok_and(|m| m.is_dir()) && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))).collect();
        let mut any = false;
        for k in kids {
            if k.join(".git").exists() { out.push(k); any = true; continue; }
            let below = depth < 3 && walk(&k, depth + 1, root, out);
            let loose = (depth == 1 && k.file_name().is_some_and(|n| n != "local" && n != "dependencies"))
                || (depth == 2 && (same_path(d, &root.join("local")) || same_path(d, &root.join("dependencies"))));
            if !below && loose { out.push(k); }
            any |= below;
        }
        any
    }
    let mut out = vec![];
    walk(root, 1, root, &mut out);
    out
}

/// Claude Code keeps a project's transcripts in ~/.claude/projects/<path, every non-alphanumeric char → '-'>.
pub fn claude_dir_of(path: &Path) -> PathBuf {
    let enc: String = path.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    dirs::home_dir().unwrap_or_default().join(".claude/projects").join(enc)
}

/// Move a folder: rename (same drive, atomic) else copy then delete (other drive; a failed copy is removed, source kept).
/// Claude's transcript folder follows so sessions still resume. Caches git ignores are dropped first (rebuilt anyway).
pub fn move_dir(from: &Path, to: &Path) -> Result<(), String> {
    if !from.is_dir() { return Err(format!("{} is not a folder", from.display())); }
    if to.exists() { return Err(format!("{} already exists", to.display())); }
    if to.starts_with(from) { return Err("can't move a folder into itself".into()); }
    std::fs::create_dir_all(to.parent().ok_or("bad target")?).map_err(|e| e.to_string())?;
    if std::fs::rename(from, to).is_err() {
        clean(&from.to_string_lossy(), &[]);
        let (f, t) = (from.to_string_lossy().to_string(), to.to_string_lossy().to_string());
        let ok = if cfg!(windows) {
            // robocopy exit < 8 = copied; /SL keeps links as links; /NFL /NDL /NJH /NJS quiet
            command(Path::new("robocopy")).args([f.as_str(), t.as_str(), "/E", "/SL", "/COPY:DAT", "/R:1", "/W:1", "/NFL", "/NDL", "/NJH", "/NJS"])
                .status().is_ok_and(|s| s.code().is_some_and(|c| c < 8))
        } else { run("cp", &["-a", &f, &t], None, &[]).ok };
        if !ok { let _ = std::fs::remove_dir_all(to); return Err("copy failed (a file in use?); nothing moved".into()); }
        let _ = std::fs::remove_dir_all(from); // ponytail: a locked leftover stays behind; the copy is complete
    }
    let (a, b) = (claude_dir_of(from), claude_dir_of(to));
    if a.is_dir() && !b.exists() { let _ = std::fs::rename(&a, &b); }
    Ok(())
}

/// Who should run a repo, by code: the gh account that owns it, else the first with push access;
/// then the AI account tied to that gh (else the default). shared = owner isn't one of ours.
#[derive(Serialize, Debug, PartialEq)]
pub struct Who { pub owner: String, pub shared: bool, pub gh: String, pub account: String }
pub fn suggest(cfg: &Config, owner: &str, users: &[String], can_push: impl Fn(&str) -> bool) -> Who {
    let mine = users.iter().find(|u| u.eq_ignore_ascii_case(owner));
    let gh = mine.or_else(|| users.iter().find(|u| can_push(u))).cloned().unwrap_or_default();
    let account = cfg.accounts.iter().find(|a| !gh.is_empty() && a.gh.eq_ignore_ascii_case(&gh))
        .map_or(cfg.default_account.clone(), |a| a.id.clone());
    Who { owner: owner.into(), shared: mine.is_none(), gh, account }
}

pub fn can_push(user: &str, owner: &str, repo: &str) -> bool {
    gh_json(&["api", &format!("repos/{owner}/{repo}")], user, None)["permissions"]["push"] == true
}

// ---- Connections & Skills ---------------------------------------------------------------------

pub fn server_id(label: &str) -> String {
    Regex::new(r"\W").unwrap().replace_all(label, "_").to_string() // "claude.ai Gmail" -> mcp__claude_ai_Gmail__*
}
