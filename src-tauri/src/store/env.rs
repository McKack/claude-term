//! .env files: find, parse, write.
use super::*;

pub(crate) fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(f) = find_file(&p, name) {
                return Some(f);
            }
        } else if p.file_name().map_or(false, |n| n == name) && p.parent().map_or(false, |d| d.ends_with("bin")) {
            return Some(p);
        }
    }
    None
}

// ---- .env files: keys are shown, values stay masked in the UI until revealed ----

pub fn env_files(dir: &str) -> Vec<String> {
    let mut f: Vec<String> = std::fs::read_dir(dir).into_iter().flatten().flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n == ".env" || n.starts_with(".env.")).collect();
    f.sort();
    f
}

/// KEY=value pairs in file order; comments and blank lines are kept on write.
pub fn env_parse(text: &str) -> Vec<(String, String)> {
    text.lines().filter_map(|l| {
        let l = l.trim().trim_start_matches("export ");
        if l.starts_with('#') { return None; }
        let (k, v) = l.split_once('=')?;
        let v = v.trim();
        let v = if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
            unescape(&v[1..v.len() - 1])
        } else if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') {
            v[1..v.len() - 1].to_string()
        } else {
            v.to_string()
        };
        Some((k.trim().to_string(), v))
    }).collect()
}

/// #129 local signing: .env keys the bundler reads → build/deploy step env. tauri build on macOS imports APPLE_CERTIFICATE
/// into a temp keychain (or uses APPLE_SIGNING_IDENTITY from the login keychain) + notarizes w/ APPLE_ID/PASSWORD/TEAM_ID
/// or APPLE_API_*; Windows signs via tauri.conf signCommand/certificateThumbprint, AZURE_* feed trusted-signing-cli.
/// Empty values dropped (empty APPLE_CERTIFICATE would make tauri fail instead of skip).
pub fn sign_env(dir: &str) -> Vec<(String, String)> {
    let text = std::fs::read_to_string(std::path::Path::new(dir).join(".env")).unwrap_or_default();
    env_parse(&text).into_iter().filter(|(k, v)| !v.is_empty()
        && (k.starts_with("APPLE_") || k.starts_with("AZURE_") || k.starts_with("TAURI_SIGNING_") || k.starts_with("WINDOWS_CERTIFICATE"))).collect()
}

pub(crate) fn unescape(s: &str) -> String {
    let (mut out, mut it) = (String::new(), s.chars());
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some(o) => out.push(o),
            None => out.push('\\'),
        }
    }
    out
}

/// Set (Some) or delete (None) one key, keeping everything else in the file as it was.
/// temp + rename in the same dir: a crash mid-write never leaves a truncated file (settings.json, .env)
pub fn write_atomic(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("{}clauracle-tmp", path.extension().map(|e| e.to_string_lossy().to_string() + ".").unwrap_or_default()));
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path).inspect_err(|_| { let _ = std::fs::remove_file(&tmp); })
}

/// .env keys set from a FRESH read right before writing (callers used text read before a slow network call → lost edits)
pub fn env_put(path: &std::path::Path, kv: &[(&str, Option<&str>)]) -> Result<(), String> {
    let mut t = std::fs::read_to_string(path).unwrap_or_default();
    for (k, v) in kv { t = env_set(&t, k, *v); }
    write_atomic(path, t.as_bytes()).map_err(|e| e.to_string())
}

pub fn env_set(text: &str, key: &str, value: Option<&str>) -> String {
    let quote = |v: &str| if v.chars().any(|c| c.is_whitespace() || "#\"'\\$".contains(c)) {
        format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n"))
    } else { v.to_string() };
    let mut found = false;
    let mut out: Vec<String> = text.lines().filter_map(|l| {
        let t = l.trim().trim_start_matches("export ");
        if t.split_once('=').map_or(false, |(k, _)| k.trim() == key) && !t.starts_with('#') {
            found = true;
            return value.map(|v| format!("{key}={}", quote(v)));
        }
        Some(l.to_string())
    }).collect();
    if !found {
        if let Some(v) = value { out.push(format!("{key}={}", quote(v))); }
    }
    out.join("\n") + "\n"
}
