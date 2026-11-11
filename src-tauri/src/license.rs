//! Commands: licence guessing and the credits file.
use crate::*;

/// App version + the file it's in: package.json / tauri.conf.json "version", Cargo.toml / pyproject.toml `version = "…"`.
// ponytail: first `version =` line in a toml (≈ [package]/[project]); workspace-inherited versions → not found
/// #264 licence text → SPDX id ("" = unknown). Order matters: LGPL/AGPL before GPL.
pub(crate) fn lic_guess(t: &str) -> &'static str {
    let t = t.to_lowercase();
    [("mit license", "MIT"), ("apache license", "Apache-2.0"), ("lesser general public", "LGPL-3.0"), ("affero general public", "AGPL-3.0"),
     ("general public license", "GPL-3.0"), ("mozilla public license", "MPL-2.0"), ("bsd 3-clause", "BSD-3-Clause"), ("redistributions of source", "BSD-3-Clause"),
     ("unlicense", "Unlicense"), ("isc license", "ISC"), ("permission is hereby granted, free of charge", "MIT")]
        .iter().filter_map(|(k, id)| t.find(k).map(|i| (i, *id))).min().map_or("", |x| x.1) // #362 earliest match = the title ∵ GPL-3.0 §13 names the Affero licence
}
#[test]
pub(crate) fn lic_guess_ids() { assert_eq!(lic_guess("MIT License\n\nCopyright"), "MIT"); assert_eq!(lic_guess("GNU LESSER GENERAL PUBLIC LICENSE"), "LGPL-3.0"); assert_eq!(lic_guess("hi"), "");
    assert_eq!(lic_guess("GNU GENERAL PUBLIC LICENSE\n... 13. Use with the GNU Affero General Public License."), "GPL-3.0"); }

/// #412 licence family: 0 permissive, 1 weak copyleft (file/library-level), 2 GPL, 3 AGPL, 8 proprietary/no-reuse, 9 unknown.
pub(crate) fn lic_cat(id: &str) -> u8 {
    let u = id.trim().trim_matches(|c| c == '(' || c == ')').to_uppercase();
    let has = |l: &[&str]| l.iter().any(|x| u.starts_with(x));
    if has(&["AGPL"]) { 3 } else if has(&["LGPL"]) { 1 } else if has(&["GPL"]) { 2 }
    else if has(&["MPL", "EPL", "CDDL", "OFL"]) { 1 }
    else if has(&["MIT", "APACHE", "BSD", "CDLA-PERMISSIVE", "ISC", "ZLIB", "UNLICENSE", "0BSD", "CC0", "UNICODE", "BSL-1.0", "BOOST", "WTFPL", "PYTHON", "PSF", "CC-BY-4.0", "CC-BY-3.0", "BLUEOAK", "X11", "NCSA", "ARTISTIC", "POSTGRESQL", "W3C"]) { 0 }
    else if has(&["PROPRIETARY", "UNLICENSED", "SSPL", "BUSL", "CC-BY-NC", "COMMONS-CLAUSE"]) { 8 } else { 9 }
}
/// #412 can a project under `proj` ship a dep under SPDX expr `expr`? → "ok" | "note" (weak copyleft: keep it a separate,
/// unmodified library) | "bad" | "unk". OR/"/" = best alternative, AND = worst. ponytail: parens ignored (A AND (B OR C) ≈ flat)
pub(crate) fn lic_fit(proj: &str, expr: &str) -> &'static str {
    let p = lic_cat(proj);
    let one = |id: &str| match lic_cat(id) { 0 => 0, 1 => 1, 2 if p == 2 || p == 3 => 0, 3 if p == 3 => 0, 9 => 3, _ => 4 }; // 0 ok 1 note 3 unk 4 bad
    let e = expr.replace('/', " OR ");
    let r = e.split(" OR ").map(|alt| alt.split(" AND ").map(one).max().unwrap_or(3)).min().unwrap_or(3);
    if expr.trim().is_empty() { "unk" } else { ["ok", "note", "", "unk", "bad"][r as usize] }
}
#[test]
pub(crate) fn lic_fit_rules() {
    assert_eq!(lic_fit("MIT", "MIT OR Apache-2.0"), "ok"); assert_eq!(lic_fit("MIT", "GPL-3.0-only"), "bad");
    assert_eq!(lic_fit("GPL-3.0", "GPL-2.0-or-later"), "ok"); assert_eq!(lic_fit("GPL-3.0", "AGPL-3.0"), "bad");
    assert_eq!(lic_fit("Proprietary", "MPL-2.0"), "note"); assert_eq!(lic_fit("MIT", "MIT/Apache-2.0"), "ok");
    assert_eq!(lic_fit("MIT", "(MIT OR Apache-2.0) AND Unicode-3.0"), "ok"); assert_eq!(lic_fit("MIT", ""), "unk"); assert_eq!(lic_fit("MIT", "LGPL-2.1"), "note");
}
/// #412 shipped deps + licences from data only (never AI): cargo tree (normal deps, all targets, offline) + package-lock
/// non-dev packages. → [{n, v, l, e, d(direct), s(lic_fit)}] deduped. ponytail: Python/Go/etc. not scanned; no lock → npm skipped
pub(crate) fn lic_deps(root: &std::path::Path, proj: &str) -> Vec<Value> {
    let mut seen = std::collections::HashSet::new();
    let mut out = vec![];
    let mut add = |n: &str, v: &str, l: &str, e: &str, d: bool| if seen.insert(format!("{e}:{n}@{v}")) {
        out.push(json!({"n": n, "v": v, "l": l, "e": e, "d": d, "s": lic_fit(proj, l)}));
    };
    if let Some(m) = ["Cargo.toml", "src-tauri/Cargo.toml"].iter().map(|f| root.join(f)).find(|f| f.exists()) {
        let o = store::run("cargo", &["tree", "--manifest-path", &m.to_string_lossy(), "-e", "normal", "--target", "all", "--prefix", "depth", "--format", "{p}|{l}", "--offline"], None, &[]);
        for ln in o.stdout.lines() {
            let dep = ln.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            let depth: usize = ln[..ln.len() - dep].parse().unwrap_or(0);
            let (pk, l) = ln[ln.len() - dep..].split_once('|').unwrap_or((ln, ""));
            let mut w = pk.split_whitespace();
            // "(D:\…" path crate = own code, "(proc-macro)" = build-time only → skipped; "(https…" git dep kept; " (*)" = cargo's dedupe mark
            let own = pk.contains('(') && !pk.contains("(http") && !pk.contains("(git");
            if let (Some(n), Some(v), true) = (w.next(), w.next(), depth > 0 && !own) { add(n, v.trim_start_matches('v'), l.trim().trim_end_matches("(*)").trim(), "cargo", depth == 1); }
        }
    }
    let pj: Value = std::fs::read_to_string(root.join("package.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let lock: Value = std::fs::read_to_string(root.join("package-lock.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    for (k, x) in lock["packages"].as_object().into_iter().flatten() {
        let Some(n) = k.rsplit_once("node_modules/").map(|x| x.1) else { continue };
        if x["dev"] == true || x["link"] == true { continue }
        let l = x["license"].as_str().map(String::from).or_else(|| x["license"]["type"].as_str().map(String::from)).unwrap_or_default();
        add(n, x["version"].as_str().unwrap_or(""), &l, "npm", k.matches("node_modules/").count() == 1 && pj["dependencies"].get(n).is_some());
    }
    out
}
/// #412 direct deps CREDITS.md doesn't name. Match = full name, or first+last name part (tauri-plugin-dialog ↔ "Tauri … dialog").
pub(crate) fn credits_miss(root: &std::path::Path, deps: &[Value]) -> Vec<String> {
    let t = std::fs::read_dir(root).into_iter().flatten().flatten().find(|e| e.file_name().to_string_lossy().to_uppercase().starts_with("CREDITS"))
        .and_then(|e| std::fs::read_to_string(e.path()).ok()).unwrap_or_default().to_lowercase().replace('_', "-");
    deps.iter().filter(|d| d["d"] == true).filter_map(|d| d["n"].as_str()).filter(|n| {
        let n = n.to_lowercase().replace('_', "-");
        let parts: Vec<&str> = n.trim_start_matches('@').split(['-', '/']).filter(|s| !s.is_empty()).collect();
        !(t.contains(&n) || parts.len() > 1 && t.contains(parts[0]) && t.contains(parts[parts.len() - 1]))
    }).map(String::from).collect()
}
#[test]
pub(crate) fn credits_match() {
    let d = std::env::temp_dir().join("clauracle-credits-test"); let _ = std::fs::create_dir_all(&d);
    std::fs::write(d.join("CREDITS.md"), "- Tauri 2 and its dialog plugin\n- serde_json").unwrap();
    let deps: Vec<Value> = ["tauri-plugin-dialog", "serde-json", "ureq", "libc"].iter().enumerate().map(|(i, n)| json!({"n": n, "d": i < 3})).collect();
    assert_eq!(credits_miss(&d, &deps), vec!["ureq"]); let _ = std::fs::remove_dir_all(&d);
}
/// #412 appends the uncredited direct deps to CREDITS.md (creates it), lines built from scan data: "- name version: licence".
#[tauri::command(async)]
pub(crate) fn lic_credits(st: St, pid: String) -> R<usize> {
    let p = st.project(&pid).ok_or("no such project")?;
    let root = std::path::PathBuf::from(&p.path);
    let deps = lic_deps(&root, &p.license);
    let miss = credits_miss(&root, &deps);
    let f = std::fs::read_dir(&root).into_iter().flatten().flatten().map(|e| e.path()).find(|f| f.file_name().is_some_and(|n| n.to_string_lossy().to_uppercase().starts_with("CREDITS"))).unwrap_or(root.join("CREDITS.md"));
    if miss.is_empty() && f.exists() { return Ok(0) }
    // #413 no file yet → made-by line from git/GitHub (no AI)
    let mut t = std::fs::read_to_string(&f).unwrap_or_else(|_| format!("# Credits\n\nMade by {}.\n", lic_holder(&p)));
    if !t.ends_with('\n') { t.push('\n') }
    if !miss.is_empty() { t += "\n## Third-party libraries\n\n"; }
    for n in &miss { if let Some(d) = deps.iter().find(|d| d["n"] == n.as_str() && d["d"] == true) {
        t += &format!("- {n} {}: {}\n", d["v"].as_str().unwrap_or(""), d["l"].as_str().filter(|l| !l.is_empty()).unwrap_or("licence not stated"));
    } }
    store::write_atomic(&f, t.as_bytes()).map_err(|e| e.to_string())?;
    Ok(miss.len())
}

/// copyright holder: project git user.name → repo owner → GitHub login
pub(crate) fn lic_holder(p: &store::Project) -> String {
    let n = store::run("git", &["config", "user.name"], Some(&p.path), &[]).stdout.trim().to_string();
    if !n.is_empty() { return n }
    store::repo_of(&p.path).map(|r| r.0).unwrap_or_else(|| p.gh.clone())
}
/// #413 LICENSE.md from GitHub's licence API text (no AI; [year]/[fullname] filled), other licence files removed, existing
/// licence fields in package.json / tauri.conf.json / Cargo.toml / pyproject set. → changed files. ponytail: absent fields not added
#[tauri::command(async)]
pub(crate) fn lic_write(st: St, pid: String) -> R<Vec<String>> {
    let p = st.project(&pid).ok_or("no such project")?;
    let (root, id) = (std::path::PathBuf::from(&p.path), p.license.as_str());
    if id.is_empty() { return Err("Pick a license first".into()) }
    let (holder, year) = (lic_holder(&p), chrono_ish()[..4].to_string());
    let text = if id == "Proprietary" {
        format!("Copyright (c) {year} {holder}. All rights reserved.\n\nNo permission is granted to use, copy, modify or distribute this software without the copyright holder's written consent.\n")
    } else {
        let o = store::run("gh", &["api", &format!("licenses/{}", id.to_lowercase()), "--jq", ".body"], None, &store::gh_env(&p.gh));
        if !o.ok || o.stdout.trim().is_empty() { return Err(format!("GitHub has no text for {id}: {}", o.stderr.trim())) }
        o.stdout.replace("[year]", &year).replace("[fullname]", &holder)
    };
    let mut done = vec!["LICENSE.md".to_string()];
    for e in std::fs::read_dir(&root).into_iter().flatten().flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        if n != "LICENSE.md" && ["LICENSE", "LICENCE", "COPYING"].iter().any(|x| n.to_uppercase().starts_with(x)) && std::fs::remove_file(e.path()).is_ok() { done.push(format!("{n} (removed)")); }
    }
    store::write_atomic(&root.join("LICENSE.md"), text.as_bytes()).map_err(|e| e.to_string())?;
    if id != "Proprietary" {
        let spdx = if id.contains("GPL") { format!("{id}-only") } else { id.to_string() };
        let json_re = regex::Regex::new(r#""license"\s*:\s*"[^"]*""#).unwrap();
        let toml_re = regex::Regex::new(r#"(?m)^license\s*=\s*"[^"]*""#).unwrap();
        for f in ["package.json", "src-tauri/tauri.conf.json", "Cargo.toml", "src-tauri/Cargo.toml", "pyproject.toml"] {
            let Ok(t) = std::fs::read_to_string(root.join(f)) else { continue };
            let n = if f.ends_with(".json") { json_re.replacen(&t, 1, format!("\"license\": \"{spdx}\"")) } else { toml_re.replacen(&t, 1, format!("license = \"{spdx}\"")) };
            if n != t && store::write_atomic(&root.join(f), n.as_bytes()).is_ok() { done.push(f.to_string()); }
        }
    }
    Ok(done)
}
