//! Commands: run steps, AI build detection, .env editing.
use crate::*;

#[tauri::command]
pub(crate) async fn run_step(app: AppHandle, st: St<'_>, pid: String, step: String) -> R<bool> {
    Ok(claude::run_step(&app, &st, &pid, &step).await.0)
}

/// #586 build steps read by the AI from the project's own files (replaces the file-pattern guess in the setup wizard and
/// the Build fold's Detect). Empty folder: nothing to read, empty steps without a model call. Blocking: claude -p, up to 2 min.
#[tauri::command(async)]
pub(crate) fn build_ai(st: St, pid: String) -> R<store::Build> {
    let p = st.project(&pid).ok_or("no such project")?;
    let dir = std::path::PathBuf::from(&p.path);
    let files = std::fs::read_dir(&dir).map(|r| r.flatten().filter(|e| !e.file_name().to_string_lossy().starts_with('.')).count()).unwrap_or(0);
    if files == 0 { return Ok(store::Build { after: p.build.after.clone(), ..Default::default() }); }
    let out = report::claude_pt(&st, &p, &["--model", "sonnet", "--effort", "low", "--allowedTools", "Read,Glob,Grep"], &dir, vec![], BUILD_AI,
        Some(std::time::Duration::from_secs(120)))?;
    parse_build(&out).map(|b| store::Build { after: p.build.after.clone(), fix: p.build.fix, release: p.build.release, ..b }).ok_or_else(|| format!("no build steps in the reply: {}", out.trim().chars().take(200).collect::<String>()))
}

const BUILD_AI: &str = "Find how this project is built, run and deployed on this computer (OS shell commands run from the project root).     Look at its files (package.json scripts, Cargo.toml, tauri.conf.json, Makefile, pyproject.toml, *.csproj, project.godot, Dockerfile, README, CI workflows…).     build = full release build; run = start it locally for a quick look (dev server, app or game); deploy = publish it, only if the project defines how;     local = a faster debug build, only if clearly different from build. Use the project's own scripts where they exist.     Each value is only the command line, never an explanation or comment. Unknown → empty string. Reply with only one line of JSON: {\"build\":\"…\",\"run\":\"…\",\"deploy\":\"…\",\"local\":\"…\"}";

fn parse_build(s: &str) -> Option<store::Build> {
    let j: serde_json::Value = serde_json::from_str(s.get(s.find('{')?..=s.rfind('}')?)?).ok()?;
    let g = |k: &str| j[k].as_str().unwrap_or("").trim().to_string();
    Some(store::Build { build: g("build"), run: g("run"), deploy: g("deploy"), local: g("local"), ..Default::default() })
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_build_reply() {
        let b = super::parse_build("Here:
```json
{\"build\":\"npm run build\",\"run\":\"npm run dev\",\"deploy\":\"\",\"local\":null}
```").unwrap();
        assert_eq!((b.build.as_str(), b.run.as_str(), b.deploy.as_str(), b.local.as_str()), ("npm run build", "npm run dev", "", ""));
        assert!(super::parse_build("no idea").is_none());
    }
}

#[tauri::command(async)] // taskkill blocks
pub(crate) fn stop_run(pid: String) -> bool {
    claude::stop_run(&pid)
}

/// "Built!" note / Builds card: swap into Clauracle's freshly built exe (own repo only); turns resume after it.
#[tauri::command(async)]
pub(crate) fn restart_into(app: AppHandle, st: St, pid: String) -> R<()> {
    if !claude::own_repo(&st, &pid) { return Err("not Clauracle's own repo".into()); }
    let p = st.project(&pid).ok_or("no such project")?;
    let (t, a) = claude::restart_target(&claude::own_exe(&p.path))?;
    emit(&app, "restarting", json!({}));
    relaunch(&app, t, &a);
    Ok(())
}

#[tauri::command(async)] // taskkill blocks
pub(crate) fn stop_build(pid: String) -> bool {
    claude::stop_build(&pid)
}

// ---- .env ----

#[tauri::command(async)] // git check-ignore per file
pub(crate) fn env_list(st: St, pid: String) -> R<Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    Ok(json!(store::env_files(&p.path).iter().map(|f| {
        let text = std::fs::read_to_string(std::path::Path::new(&p.path).join(f)).unwrap_or_default();
        json!({"file": f, "ignored": store::git_ignored(&p.path, f), "keys": store::env_parse(&text)})
    }).collect::<Vec<_>>()))
}

#[tauri::command]
pub(crate) fn env_set(st: St, pid: String, file: String, key: String, value: Option<String>) -> R<()> {
    let p = st.project(&pid).ok_or("no such project")?;
    if !(file == ".env" || file.starts_with(".env.")) || file.contains(['/', '\\']) {
        return Err("only .env files in the project root".into());
    }
    if !regex::Regex::new(r"^[A-Za-z_][A-Za-z0-9_.]*$").unwrap().is_match(&key) {
        return Err("keys are letters, digits and _".into());
    }
    let path = std::path::Path::new(&p.path).join(&file);
    store::env_put(&path, &[(&key, value.as_deref())])
}

/// Add the file to .gitignore.
#[tauri::command(async)]
pub(crate) fn env_ignore(st: St, pid: String, file: String) -> R<()> {
    let p = st.project(&pid).ok_or("no such project")?;
    let gi = std::path::Path::new(&p.path).join(".gitignore");
    let mut text = std::fs::read_to_string(&gi).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text += &format!("{file}\n");
    std::fs::write(gi, text).map_err(|e| e.to_string())
}

// ---- Mac signing (Developer ID) from any OS, via openssl ----
// key + CSR (~/.clauracle/apple/<pid>, never in the repo) → user uploads CSR at Apple, downloads .cer → .p12 →
// base64 + identity/team into .env → gh secrets (tauri-action signs + notarizes when APPLE_* secrets exist).
