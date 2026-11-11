//! Network Mac over SSH: key setup, Xcode check, sync and remote build scripts.
use super::*;

/// #147 `ssh -o BatchMode=yes [user@]host` probe: login shell (zsh -l ∵ Homebrew/rustup PATH lives in .zprofile) prints
/// key=value lines → {ok, os, xcode, rust, node, err}. rust also probes keg-only Homebrew rustup / ~/.cargo (not on PATH; #148 adds them). ConnectTimeout 8s ∵ an asleep/moved Mac must not hang the UI.
pub(crate) fn mac_to(m: &NetMac) -> String {
    if m.user.trim().is_empty() { m.host.trim().to_string() } else { format!("{}@{}", m.user.trim(), m.host.trim()) }
}

pub fn mac_test(m: &NetMac) -> serde_json::Value {
    let to = mac_to(m);
    if m.host.trim().is_empty() { return serde_json::json!({"ok": false, "err": "No host set"}); }
    let sh = "zsh -lc 'echo os=$(uname -sm); echo xcode=$(xcode-select -p 2>/dev/null); echo rust=$(command -v cargo || ls ~/.cargo/bin/cargo /opt/homebrew/opt/rustup/bin/cargo 2>/dev/null | head -1); echo node=$(command -v node)'";
    let o = run("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=8", "-o", "StrictHostKeyChecking=accept-new", &to, sh], None, &[]);
    let kv = |k: &str| o.stdout.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).unwrap_or("").trim().to_string();
    serde_json::json!({"ok": o.ok && !kv("os").is_empty(), "os": kv("os"), "xcode": kv("xcode"), "rust": kv("rust"), "node": kv("node"),
        "err": o.stderr.lines().filter(|l| !l.trim().is_empty() && !l.starts_with("Warning: Permanently added")).last().unwrap_or("").trim()})
}

/// #160 one-click key login: ssh-keygen ed25519 if missing → pubkey appended (dedup) to the Mac's authorized_keys over a
/// password login. Password → ssh via SSH_ASKPASS = this exe in askpass mode (main.rs: env CLAURACLE_ASKPASS → stdout);
/// lives only in that child's env, never stored. SSH_ASKPASS_REQUIRE=force needs OpenSSH ≥8.4 (Win11/macOS ok).
pub fn mac_key_setup(m: &NetMac, pw: &str) -> Result<(), String> {
    if m.host.trim().is_empty() { return Err("No host set".into()); }
    let sshd = dirs::home_dir().ok_or("no home")?.join(".ssh");
    let key = sshd.join("id_ed25519");
    let pubk = sshd.join("id_ed25519.pub");
    if !pubk.exists() {
        let _ = std::fs::create_dir_all(&sshd);
        let o = run("ssh-keygen", &["-q", "-t", "ed25519", "-N", "", "-f", &key.to_string_lossy()], None, &[]);
        if !o.ok { return Err(format!("ssh-keygen: {}", o.stderr.trim())); }
    }
    let k = std::fs::read_to_string(&pubk).map_err(|e| e.to_string())?;
    ssh_key_install(&mac_to(m), &k, pw).map_err(|e| e.replace("{who}", "Mac").replace("SSH is off on that server", "Remote Login is off on the Mac"))
}

/// pubkey → `to`'s authorized_keys (dedup) over one password login; shared by #160 Mac + #166 website hosts. Errors carry {who}.
pub fn ssh_key_install(to: &str, k: &str, pw: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut c = command(&which("ssh").ok_or("ssh not found")?);
    c.args(["-o", "ConnectTimeout=8", "-o", "StrictHostKeyChecking=accept-new", "-o", "PubkeyAuthentication=no",
            // ponytail: password only ∵ wrong pw → fallback to keyboard-interactive hung (macOS, #171); kbd-only sshd → "Wrong…" msg, add kbd back w/ a timeout if seen
            "-o", "PreferredAuthentications=password", "-o", "NumberOfPasswordPrompts=1", to,
            "umask 077; mkdir -p ~/.ssh; k=$(cat); grep -qxF \"$k\" ~/.ssh/authorized_keys 2>/dev/null || echo \"$k\" >> ~/.ssh/authorized_keys"])
        .env("SSH_ASKPASS", exe).env("SSH_ASKPASS_REQUIRE", "force").env("DISPLAY", ":0").env("CLAURACLE_ASKPASS", pw)
        .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());
    let mut ch = c.spawn().map_err(|e| e.to_string())?;
    { use std::io::Write; let _ = ch.stdin.take().unwrap().write_all(k.trim().as_bytes()); }
    let o = ch.wait_with_output().map_err(|e| e.to_string())?;
    if o.status.success() { return Ok(()); }
    let e = String::from_utf8_lossy(&o.stderr);
    let l = e.lines().filter(|l| !l.trim().is_empty() && !l.starts_with("Warning: Permanently added")).last().unwrap_or("ssh failed").trim();
    Err(if l.contains("Permission denied") { "Wrong {who} username or password".into() }
        else if l.contains("onnection refused") { "SSH is off on that server".into() } else { l.to_string() })
}

/// #166 website deploy key: ed25519 pair in a temp dir → SITE_SSH_PRIVATE_KEY + SITE_SSH_PUBLIC_KEY in `env`; rerun = same pubkey.
/// Own private key pasted w/o pubkey → refuse (no overwrite). → pubkey (for the panel / ssh_key_install).
pub fn deploy_key(env: &std::path::Path, comment: &str) -> Result<String, String> {
    let have = env_parse(&std::fs::read_to_string(env).unwrap_or_default());
    let get = |k: &str| have.iter().find(|(x, v)| x == k && !v.is_empty()).map(|(_, v)| v.clone());
    if let Some(k) = get("SITE_SSH_PUBLIC_KEY") { return Ok(k); }
    if get("SITE_SSH_PRIVATE_KEY").is_some() { return Err("SITE_SSH_PRIVATE_KEY is already set (your own key): clear it to make a new one".into()); }
    let dir = std::env::temp_dir().join(format!("clauracle-key-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let f = dir.join("k");
    let o = run("ssh-keygen", &["-q", "-t", "ed25519", "-N", "", "-C", comment, "-f", &f.to_string_lossy()], None, &[]);
    let r = (|| {
        if !o.ok { return Err(format!("ssh-keygen: {}", o.stderr.trim())); }
        let pk = std::fs::read_to_string(&f).map_err(|e| e.to_string())?;
        let pubk = std::fs::read_to_string(dir.join("k.pub")).map_err(|e| e.to_string())?.trim().to_string();
        env_put(env, &[("SITE_SSH_PRIVATE_KEY", Some(pk.trim_end())), ("SITE_SSH_PUBLIC_KEY", Some(pubk.as_str()))])?;
        Ok(pubk)
    })();
    let _ = std::fs::remove_dir_all(&dir); // private key lives only in .env
    r
}

/// #160 Xcode CLT: pops Apple's install dialog on the Mac (user clicks Install there).
pub fn mac_xcode(m: &NetMac) -> Result<(), String> {
    let o = mac_ssh(m, "xcode-select --install").output().map_err(|e| e.to_string())?;
    let e = String::from_utf8_lossy(&o.stderr);
    if o.status.success() || e.contains("already installed") { Ok(()) } else { Err(e.lines().last().unwrap_or("failed").trim().into()) }
}

/// #148 idempotent dep install on the Network Mac (rerunnable): node + Homebrew rustup (keg-only → PATH line in ~/.zprofile),
/// stable toolchain, both mac targets (universal builds). Homebrew/Xcode not auto-installed ∵ need the Mac password/GUI → ERR/WARN lines.
/// Script via stdin to `zsh -l -s` (no quoting hell); each output line → on_line (progress). Ok = exit 0 + DONE.
pub(crate) const MAC_SETUP: &str = r#"exec 2>&1
B=/opt/homebrew/bin/brew; [ -x $B ] || B=/usr/local/bin/brew
[ -x $B ] || { echo "ERR Homebrew missing: install it from https://brew.sh on the Mac (asks the Mac password), then run Set up again"; exit 2; }
xcode-select -p >/dev/null 2>&1 || echo "WARN Xcode tools missing: run xcode-select --install on the Mac"
command -v node >/dev/null || { echo "STEP Installing Node.js"; $B install node || exit 3; }
R=$($B --prefix)/opt/rustup/bin
command -v rustup >/dev/null || [ -x $R/rustup ] || { echo "STEP Installing rustup"; $B install rustup || exit 4; }
if [ -x $R/rustup ] && ! grep -q opt/rustup/bin ~/.zprofile 2>/dev/null; then echo "STEP Adding rustup to PATH (~/.zprofile)"; echo "export PATH=\"$R:\$PATH\"" >> ~/.zprofile; fi
export PATH="$R:$PATH"
rustup default >/dev/null 2>&1 || { echo "STEP Installing Rust stable"; rustup default stable || exit 5; }
echo "STEP Adding Mac targets (Apple silicon + Intel)"; rustup target add aarch64-apple-darwin x86_64-apple-darwin || exit 6
echo DONE
"#;

pub fn mac_setup(m: &NetMac, on_line: impl Fn(&str)) -> bool {
    use std::io::{BufRead, Read, Write};
    if m.host.trim().is_empty() { on_line("ERR No host set"); return false; }
    let Some(ssh) = which("ssh") else { on_line("ERR ssh not found"); return false; };
    let mut c = command(&ssh);
    c.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=8", "-o", "StrictHostKeyChecking=accept-new", &mac_to(m), "zsh -l -s"])
        .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let Ok(mut ch) = c.spawn() else { on_line("ERR ssh failed to start"); return false; };
    if let Some(mut i) = ch.stdin.take() { let _ = i.write_all(MAC_SETUP.as_bytes()); } // dropped → EOF → script runs
    let mut done = false;
    for l in std::io::BufReader::new(ch.stdout.take().unwrap()).lines().map_while(Result::ok) {
        done |= l.trim() == "DONE";
        if !l.trim().is_empty() { on_line(l.trim_end()); }
    }
    let ok = ch.wait().map_or(false, |s| s.success()) && done;
    if !ok { // ssh's own error (unreachable, key refused) only lands on stderr
        let mut e = String::new();
        let _ = ch.stderr.take().unwrap().read_to_string(&mut e);
        if let Some(l) = e.lines().filter(|l| !l.trim().is_empty() && !l.starts_with("Warning: Permanently added")).last() { on_line(&format!("ERR {}", l.trim())); }
    }
    ok
}

/// #149 remote build dir on the Mac: `<dir or clauracle-build>/<pid>` (relative = Mac home). Only safe path chars ∵ goes unquoted
/// into the remote shell (keeps `~` expansion).
pub fn mac_dir(m: &NetMac, pid: &str) -> String {
    let clean = |s: &str, extra: &str| s.chars().filter(|c| c.is_ascii_alphanumeric() || "-_.".contains(*c) || extra.contains(*c)).collect::<String>();
    let root = clean(m.dir.trim().trim_end_matches('/'), "/~");
    format!("{}/{}", if root.is_empty() { "clauracle-build".into() } else { root }, clean(pid, ""))
}

pub(crate) fn mac_ssh(m: &NetMac, remote: &str) -> Command {
    let mut c = command(&which("ssh").unwrap_or_else(|| "ssh".into()));
    c.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=8", "-o", "StrictHostKeyChecking=accept-new", &mac_to(m), remote]);
    c
}

/// a | b (both std Commands): a.stdout → b.stdin. Err = last stderr line of whichever failed.
pub(crate) fn pipe(mut a: Command, mut b: Command) -> Result<(), String> {
    use std::process::Stdio;
    let mut ca = a.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    let out = ca.stdout.take().unwrap();
    let mut ea = ca.stderr.take().unwrap(); // drained in parallel ∵ a full stderr pipe blocked `a` → b never saw EOF → hang
    let ta = std::thread::spawn(move || { let mut v = vec![]; let _ = std::io::Read::read_to_end(&mut ea, &mut v); v });
    let ob = b.stdin(out).stdout(Stdio::null()).stderr(Stdio::piped()).output().map_err(|e| e.to_string())?;
    let st = ca.wait().map_err(|e| e.to_string())?;
    let oa = std::process::Output { status: st, stdout: vec![], stderr: ta.join().unwrap_or_default() };
    let last = |e: &[u8]| String::from_utf8_lossy(e).lines().filter(|l| !l.trim().is_empty() && !l.starts_with("Warning: Permanently added")).last().unwrap_or("failed").trim().to_string();
    if !oa.status.success() { return Err(last(&oa.stderr)); }
    if !ob.status.success() { return Err(last(&ob.stderr)); }
    Ok(())
}

/// #149 working tree → Mac (like the WSL copy: uncommitted edits included). Never .env/.git/node_modules/target
/// (.env = secrets: they travel only as env in mac_script, #150). ponytail: never deletes files removed locally.
pub fn mac_sync(m: &NetMac, pid: &str, dir: &str) -> Result<(), String> {
    let mut t = Command::new("tar");
    t.current_dir(dir).args(["--exclude=node_modules", "--exclude=target", "--exclude=.git", "--exclude=.env", "--exclude=.env.*", "-cf", "-", "."]);
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; t.creation_flags(NO_WINDOW); }
    let d = mac_dir(m, pid);
    pipe(t, mac_ssh(m, &format!("mkdir -p {d} && tar -C {d} -xf -")))
}

/// #149 Mac bundles (dmg, updater .app.tar.gz + .sig) → same paths in the local project ∴ builds()/Release see them.
pub fn mac_pull(m: &NetMac, pid: &str, dir: &str) -> Result<(), String> {
    let d = mac_dir(m, pid);
    let find = format!("cd {d} && find . -path '*/release/bundle/*' -type f \\( -name '*.dmg' -not -name 'rw.*' -o -name '*.app.tar.gz' -o -name '*.app.tar.gz.sig' \\) -not -path './node_modules/*' | COPYFILE_DISABLE=1 tar -cf - -T -"); // no ._ AppleDouble files
    let mut t = Command::new("tar");
    t.current_dir(dir).args(["-xf", "-"]);
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; t.creation_flags(NO_WINDOW); }
    pipe(mac_ssh(m, &find), t)
}

/// #149/#150 zsh fed via stdin to `ssh host zsh -l -s`: signing keys exported in-process only (never argv, never a file on
/// the Mac, gone when the shell exits), then npm install + the build cmd in the synced copy. Only APPLE_* / TAURI_SIGNING_*
/// travel (Windows/Azure keys are useless there). tauri imports APPLE_CERTIFICATE into its own temp keychain + notarizes.
pub fn mac_script(m: &NetMac, pid: &str, cmd: &str, sign: &[(String, String)]) -> String {
    let q = |v: &str| format!("'{}'", v.replace('\'', "'\\''"));
    let env: String = sign.iter().filter(|(k, _)| k.starts_with("APPLE_") || k.starts_with("TAURI_SIGNING_"))
        .filter(|(k, _)| k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .map(|(k, v)| format!("export {k}={}\n", q(v))).collect();
    // CI=true: tauri's bundle_dmg.sh skips the Finder AppleScript layout (times out over ssh: no GUI session, -1712)
    // #156: fresh Mac lacks Apple's Developer ID G2 intermediate → codesign "unable to build chain" / errSecInternalComponent.
    // Public cert → login keychain import works over ssh w/o unlock; skip if present, never fatal
    let ca = "security find-certificate -c 'Developer ID Certification Authority' >/dev/null 2>&1 || { curl -fsSL https://www.apple.com/certificateauthority/DeveloperIDG2CA.cer -o /tmp/devidg2.cer && security import /tmp/devidg2.cer -k ~/Library/Keychains/login.keychain-db; } || true\n";
    // #156: tauri notarizes+staples only the .app; the dmg stays "Unnotarized Developer ID" → notarize + staple each fresh dmg.
    // ponytail: Apple ID creds only (APPLE_API_* path not handled); -mmin 120 = this build's dmgs, not old versions'
    let dmg = "if [ -n \"$APPLE_ID\" ] && [ -n \"$APPLE_PASSWORD\" ] && [ -n \"$APPLE_TEAM_ID\" ]; then find src-tauri/target -path '*/bundle/dmg/*.dmg' -not -name 'rw.*' -mmin -120 | while read -r d; do echo \"Notarizing $d\"; xcrun notarytool submit \"$d\" --apple-id \"$APPLE_ID\" --password \"$APPLE_PASSWORD\" --team-id \"$APPLE_TEAM_ID\" --wait && xcrun stapler staple \"$d\"; done; fi\n";
    format!("exec 2>&1\nset -e\nexport CI=true\n{ca}{env}cd {}\nif [ -f package.json ]; then npm install --no-audit --no-fund; fi\n{cmd}\n{dmg}", mac_dir(m, pid))
}
