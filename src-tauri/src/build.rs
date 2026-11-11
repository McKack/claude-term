//! Commands: run steps, build guess, .env editing.
use crate::*;

#[tauri::command]
pub(crate) async fn run_step(app: AppHandle, st: St<'_>, pid: String, step: String) -> R<bool> {
    Ok(claude::run_step(&app, &st, &pid, &step).await.0)
}

#[tauri::command(async)]
pub(crate) fn build_guess(st: St, pid: String) -> R<store::Build> {
    Ok(store::guess_build(&st.project(&pid).ok_or("no such project")?.path))
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
