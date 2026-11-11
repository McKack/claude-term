//! Claude CLI side: MCP/skills catalog, CLI settings and restrictions, setup tokens, gcloud, installers.
use super::*;

/// #155 MCP server names from config files only: `claude mcp list` health-checks = launches every server (AppControl toast
/// at app start). ~/.claude.json user + every local scope, `.mcp.json` of our projects. claude.ai connectors live only on
/// the server → `old` ones kept; new ones arrive via session init (`mcp_seen`).
pub(crate) fn mcp_items(paths: &[String], old: &[Item]) -> Vec<Item> {
    let h = dirs::home_dir().unwrap_or_default();
    let read = |p: PathBuf| std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(t.trim_start_matches('\u{feff}')).ok()).unwrap_or_default();
    let keys = |v: &serde_json::Value| v.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default();
    let cj = read(h.join(".claude.json"));
    let mut names: Vec<(String, String)> = keys(&cj["mcpServers"]).into_iter().map(|n| (n, "user".into())).collect();
    for p in cj["projects"].as_object().into_iter().flat_map(|o| o.values()) { names.extend(keys(&p["mcpServers"]).into_iter().map(|n| (n, "local".into()))); }
    for p in paths { names.extend(keys(&read(Path::new(p).join(".mcp.json"))["mcpServers"]).into_iter().map(|n| (n, "project".into()))); }
    let mut items: Vec<Item> = old.iter().filter(|i| i.kind == "mcp" && i.label.starts_with("claude.ai ")).cloned().collect();
    for (label, note) in names {
        if label != "clauracle" && !items.iter().any(|i| i.label == label) { items.push(Item { id: format!("mcp:{}", server_id(&label)), label, kind: "mcp".into(), note }); }
    }
    items
}

/// #155 servers a session reported in its init message (claude.ai connectors aren't in any local file) → catalog.
/// True = new ones added (caller reconnects so deniedMcpServers covers them). Plugin servers follow enabledPlugins.
pub fn mcp_seen(cfg: &mut Config, init: &serde_json::Value) -> bool {
    let mut added = false;
    for s in init["mcp_servers"].as_array().into_iter().flatten() {
        let (Some(label), src) = (s["name"].as_str(), s["source"].as_str().unwrap_or("")) else { continue };
        if label == "clauracle" || src == "plugin" || src == "sdk" || cfg.catalog.iter().any(|i| i.kind == "mcp" && i.label == label) { continue; }
        cfg.catalog.push(Item { id: format!("mcp:{}", server_id(label)), label: label.into(), kind: "mcp".into(), note: if src == "claudeai" { "claude.ai".into() } else { src.into() } });
        added = true;
    }
    added
}

/// MCP servers (mcp_items) and skills (yours, installed plugins', Clauracle's own).
pub fn catalog(plugin_dir: &Path, paths: &[String], old: &[Item]) -> Vec<Item> {
    let mut items = mcp_items(paths, old);
    let h = dirs::home_dir().unwrap_or_default();
    let mut roots = vec![(String::new(), h.join(".claude/skills")), ("clauracle:".into(), plugin_dir.join("skills"))];
    if let Ok(s) = std::fs::read_to_string(h.join(".claude/plugins/installed_plugins.json")) {
        let v: serde_json::Value = serde_json::from_str(&s).unwrap_or_default();
        for (name, inst) in v["plugins"].as_object().cloned().unwrap_or_default() {
            if let Some(path) = inst[0]["installPath"].as_str() {
                roots.push((format!("{}:", name.split('@').next().unwrap_or(&name)), Path::new(path).join("skills")));
            }
        }
    }
    // plugins + inline mods as items of their own: one only loads (hooks, MCP, agents, skills) when it or one of its
    // skills is picked. ponytail@ponytail never: Clauracle bundles its own (else Ponytail is injected twice).
    let mut plugins: Vec<String> = std::fs::read_to_string(h.join(".claude/plugins/installed_plugins.json")).ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["plugins"].as_object().map(|o| o.keys().cloned().collect())).unwrap_or_default();
    plugins.extend(std::fs::read_dir(h.join(".claude/mods")).into_iter().flatten().flatten()
        .filter(|e| e.path().is_dir()).map(|e| format!("{}@inline", e.file_name().to_string_lossy())));
    plugins.sort();
    for k in plugins.into_iter().filter(|k| k != PONYTAIL_PLUGIN) {
        let name = k.split('@').next().unwrap_or(&k).to_string();
        let note = if k.ends_with("@inline") { "mod".into() } else { format!("plugin · {}", k.split('@').nth(1).unwrap_or("")) };
        items.push(Item { id: format!("plugin:{k}"), label: name, kind: "plugin".into(), note });
    }
    for (prefix, root) in roots {
        let mut names: Vec<String> = std::fs::read_dir(&root).into_iter().flatten().flatten()
            .filter(|e| e.path().join("SKILL.md").is_file())
            .map(|e| e.file_name().to_string_lossy().to_string()).collect();
        names.sort();
        for n in names {
            let note = if prefix.is_empty() { "your skills".into() } else { prefix.trim_end_matches(':').to_string() };
            items.push(Item { id: format!("skill:{prefix}{n}"), label: format!("{prefix}{n}"), kind: "skill".into(), note });
        }
    }
    items
}

/// Disabled MCP servers become deny rules; skills an allowlist for `initialize` (None = all of them).
/// A project with `on` gets only what it picked (Clauracle's own skills always); a legacy one (`on` None)
/// everything but its `off`. The global `off` switches things off for every project.
pub(crate) const PONYTAIL_PLUGIN: &str = "ponytail@ponytail";

/// Picked for this project? (global `off` wins; Clauracle's own skills always on; legacy `on` None = all but `off`)
pub(crate) fn is_on(cfg: &Config, p: &Project, id: &str) -> bool {
    let has = |l: &[String], id: &str| l.iter().any(|x| x == id);
    !has(&cfg.off, id) && (id.starts_with("skill:clauracle:") || match &p.on {
        Some(on) => has(on, id),
        None => !has(&p.off, id),
    })
}

/// The project's picks at the source, as CLI `--settings`: unpicked MCP servers (claude.ai connectors too) never start
/// (deniedMcpServers), unpicked plugins/mods don't load at all (enabledPlugins false: no hooks, MCP, agents, skills).
/// ponytail: deny-by-name from the catalog → a server added after the last scan runs until a rescan.
pub fn cli_settings(cfg: &Config, p: &Project) -> serde_json::Value {
    let denied: Vec<serde_json::Value> = cfg.catalog.iter().filter(|i| i.kind == "mcp" && !is_on(cfg, p, &i.id))
        .map(|i| serde_json::json!({"serverName": i.label})).collect();
    let mut plugins = serde_json::Map::new();
    plugins.insert(PONYTAIL_PLUGIN.into(), false.into());
    for i in cfg.catalog.iter().filter(|i| i.kind == "plugin") {
        let key = &i.id["plugin:".len()..];
        let skill_pre = format!("skill:{}:", key.split('@').next().unwrap_or(key));
        let on = is_on(cfg, p, &i.id) || cfg.catalog.iter().any(|s| s.id.starts_with(&skill_pre) && is_on(cfg, p, &s.id));
        if !on { plugins.insert(key.into(), false.into()); }
    }
    serde_json::json!({"deniedMcpServers": denied, "enabledPlugins": plugins})
}

pub fn restrictions(cfg: &Config, p: &Project) -> (Vec<String>, Option<Vec<String>>) {
    let enabled = |id: &str| is_on(cfg, p, id);
    let has = |l: &[String], id: &str| l.iter().any(|x| x == id);
    // ponytail: built from the catalog, so a server added after the last scan stays on until a rescan
    let ids: Vec<String> = cfg.catalog.iter().map(|i| i.id.clone()).chain(cfg.off.iter().cloned()).chain(p.off.iter().cloned()).collect();
    let mut deny: Vec<String> = ids.iter().filter(|i| !enabled(i)).filter_map(|i| i.strip_prefix("mcp:")).map(|s| format!("mcp__{s}")).collect();
    deny.sort();
    deny.dedup();
    if has(&cfg.off, "web") { // composer's globe: web search off everywhere
        deny.extend(["WebSearch", "WebFetch"].map(String::from));
    }
    // #407 Claude Code's own agents switched off (Settings › Helper agents): Agent = current tool name, Task = older CLIs
    for a in cfg.off.iter().filter_map(|i| i.strip_prefix("agent:")) {
        deny.extend([format!("Agent({a})"), format!("Task({a})")]);
    }
    if p.hide_env {
        deny.extend(["Read(**/.env*)", "Edit(**/.env*)", "Write(**/.env*)", "Bash(cat *.env*)"].map(String::from));
    }
    let skills = cfg.catalog.iter().filter(|i| i.kind == "skill");
    let allow = (p.on.is_some() || skills.clone().any(|i| !enabled(&i.id)))
        .then(|| skills.filter(|i| enabled(&i.id)).map(|i| i.id["skill:".len()..].to_string()).collect());
    (deny, allow)
}

// ---- `claude setup-token` in a pseudo-terminal (it draws with Ink, which prints nothing through pipes) ----

pub fn setup_token(on_url: impl Fn(String)) -> Result<String, String> {
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    let claude = which("claude").ok_or("Claude Code isn't installed yet")?;
    let pty = native_pty_system();
    let pair = pty.openpty(PtySize { rows: 50, cols: 1000, pixel_width: 0, pixel_height: 0 }) // wide: no wrapping
        .map_err(|e| e.to_string())?;
    let mut cmd = CommandBuilder::new(claude);
    cmd.arg("setup-token");
    let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let result = read_setup_token(&mut reader, on_url);
    let _ = child.kill();
    result.ok_or_else(|| "setup-token ended without a token".into())
}

pub fn read_setup_token(reader: &mut dyn std::io::Read, on_url: impl Fn(String)) -> Option<String> {
    // both need a character after the match, so a read that stops mid-token doesn't return half of it
    let url_re = Regex::new(r"https://\S+?/oauth/authorize\?[^\s\x1b\x07]+[\s\x1b\x07]").unwrap();
    let tok_re = Regex::new(r"sk-ant-oat\d+-[\w-]{20,}[^\w-]").unwrap();
    let (mut out, mut seen_url, mut buf) = (String::new(), false, [0u8; 4096]);
    loop {
        let n = reader.read(&mut buf).ok()?;
        if n == 0 {
            return None;
        }
        out.push_str(&String::from_utf8_lossy(&buf[..n]));
        if !seen_url {
            if let Some(m) = url_re.find(&out) {
                seen_url = true;
                on_url(m.as_str()[..m.as_str().len() - 1].to_string());
            }
        }
        if let Some(m) = tok_re.find(&out) {
            return Some(m.as_str()[..m.as_str().len() - 1].to_string());
        }
    }
}

pub fn token_ok(t: &str) -> bool {
    Regex::new(r"^sk-ant-oat\d+-[\w-]{20,}$").unwrap().is_match(t.trim())
}

// ---- nothing to install by hand: fetch the official Claude Code installer / the gh release ----

// ---- Google Cloud (gcloud) ----

/// A project's Google account + Cloud project, per process (like GH_TOKEN): never `gcloud config set`.
pub fn gcloud_env(account: &str, project: &str) -> Vec<(String, String)> {
    let mut v = vec![];
    if !account.is_empty() { v.push(("CLOUDSDK_CORE_ACCOUNT".to_string(), account.to_string())); }
    if !project.is_empty() { v.push(("CLOUDSDK_CORE_PROJECT".to_string(), project.to_string())); }
    v
}

pub fn gcloud(args: &[&str], account: &str, project: &str) -> Result<serde_json::Value, String> {
    let mut a = args.to_vec();
    a.extend(["--format=json", "--quiet"]);
    let o = run("gcloud", &a, None, &gcloud_env(account, project));
    if o.ok {
        Ok(serde_json::from_str(&o.stdout).unwrap_or(serde_json::Value::Null))
    } else {
        Err(o.stderr.lines().filter(|l| !l.trim().is_empty()).last().unwrap_or("gcloud failed").to_string())
    }
}

/// Google's own installer: silent NSIS on Windows (per user), install script elsewhere (~/google-cloud-sdk).
pub fn install_gcloud() -> Result<(), String> {
    let o = if cfg!(windows) {
        let exe = std::env::temp_dir().join("GoogleCloudSDKInstaller.exe");
        let mut body = vec![];
        ureq::get("https://dl.google.com/dl/cloudsdk/channels/rapid/GoogleCloudSDKInstaller.exe").call()
            .map_err(|e| e.to_string())?.into_reader().read_to_end(&mut body).map_err(|e| e.to_string())?;
        std::fs::write(&exe, body).map_err(|e| e.to_string())?;
        command(&exe).args(["/S", "/noreporting", "/nostartmenu", "/nodesktop"]).output()
    } else {
        Command::new("bash").args(["-c", "curl -fsSL https://sdk.cloud.google.com | bash -s -- --disable-prompts --install-dir=$HOME"]).output()
    }.map_err(|e| e.to_string())?;
    if which("gcloud").is_some() { Ok(()) } else { Err(String::from_utf8_lossy(&o.stderr).trim().to_string() + " (gcloud not found after install)") }
}

pub fn install_claude() -> Result<(), String> {
    let o = if cfg!(windows) {
        let mut c = command(Path::new("powershell"));
        c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", "irm https://claude.ai/install.ps1 | iex"]);
        c.output()
    } else {
        Command::new("bash").args(["-c", "curl -fsSL https://claude.ai/install.sh | bash"]).output()
    }.map_err(|e| e.to_string())?;
    if which("claude").is_some() { Ok(()) } else { Err(String::from_utf8_lossy(&o.stderr).into()) }
}

pub fn install_gh() -> Result<(), String> {
    let rel: serde_json::Value = ureq::get("https://api.github.com/repos/cli/cli/releases/latest")
        .set("User-Agent", "clauracle").call().map_err(|e| e.to_string())?.into_string().ok()
        .and_then(|s| serde_json::from_str(&s).ok()).ok_or("couldn't read the gh release list")?;
    let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "amd64" };
    let want = if cfg!(windows) { format!("_windows_{arch}.zip") }
               else if cfg!(target_os = "macos") { "_macOS_universal.zip".into() }
               else { format!("_linux_{arch}.tar.gz") };
    let asset = rel["assets"].as_array().into_iter().flatten()
        .find(|a| a["name"].as_str().map_or(false, |n| n.ends_with(&want))).ok_or("no gh build for this system")?;
    let tmp = home().join("gh-download");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let file = tmp.join(asset["name"].as_str().unwrap());
    let mut body = ureq::get(asset["browser_download_url"].as_str().unwrap()).call().map_err(|e| e.to_string())?.into_reader();
    std::io::copy(&mut body, &mut std::fs::File::create(&file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    // bsdtar unpacks zip and tar.gz alike and ships with Windows 10+, macOS and Linux
    let mut t = command(Path::new("tar"));
    t.arg("-xf").arg(&file).arg("-C").arg(&tmp).output().map_err(|e| e.to_string())?;
    let exe = if cfg!(windows) { "gh.exe" } else { "gh" };
    let found = find_file(&tmp, exe).ok_or("gh not found in the download")?;
    std::fs::create_dir_all(bin_dir()).map_err(|e| e.to_string())?;
    std::fs::copy(&found, bin_dir().join(exe)).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(bin_dir().join(exe), std::fs::Permissions::from_mode(0o755));
    }
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}
