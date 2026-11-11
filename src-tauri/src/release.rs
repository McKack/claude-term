//! Commands: updates, releases, staging, disk, Apple signing, website, release checks, secrets.
use crate::*;

// ---- updates: Clauracle's own GitHub releases; via gh when signed in (the repo may be private), else plain HTTPS ----
pub(crate) const REPO: &str = "McKack/claude-term";
pub(crate) fn latest_release() -> Value {
    match store::gh_users().first() {
        Some(u) => store::gh_json(&["api", &format!("repos/{REPO}/releases/latest")], u, None),
        None => ureq::get(&format!("https://api.github.com/repos/{REPO}/releases/latest")).set("User-Agent", "Clauracle")
            .call().ok().and_then(|r| r.into_string().ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default(),
    }
}
/// "0.2.0" > "0.1.9"; non-numbers count as 0
pub(crate) fn newer(a: &str, b: &str) -> bool {
    let v = |s: &str| s.trim_start_matches('v').split('.').map(|x| x.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>();
    v(a) > v(b)
}
#[tauri::command]
pub(crate) async fn update_check(st: St<'_>) -> R<Value> {
    { let mut cfg = st.cfg.lock().unwrap(); cfg.update_at = store::now() as u64; store::save(&cfg); }
    tauri::async_runtime::spawn_blocking(|| {
        let r = latest_release();
        let tag = r["tag_name"].as_str().unwrap_or("");
        json!({"current": env!("CARGO_PKG_VERSION"), "latest": tag, "newer": !tag.is_empty() && newer(tag, env!("CARGO_PKG_VERSION")),
               "url": r["html_url"], "notes": r["body"]})
    }).await.map_err(|e| e.to_string())
}
/// Windows: fetch the release's *-setup.exe, start it, quit (the installer replaces this exe). Elsewhere the page opens the release.
#[tauri::command]
pub(crate) async fn update_run(app: AppHandle) -> R<()> {
    let exe = tauri::async_runtime::spawn_blocking(|| -> R<std::path::PathBuf> {
        let r = latest_release();
        let tag = r["tag_name"].as_str().ok_or("no release")?;
        let dir = std::env::temp_dir().join("clauracle-update");
        let _ = std::fs::create_dir_all(&dir);
        let asset = r["assets"].as_array().into_iter().flatten().find(|a| a["name"].as_str().map_or(false, |n| n.ends_with("-setup.exe"))).ok_or("no installer in the release")?;
        let path = dir.join(asset["name"].as_str().unwrap_or("setup.exe"));
        if let Some(u) = store::gh_users().first() {
            let d = dir.to_string_lossy();
            let o = store::run("gh", &["release", "download", tag, "-R", REPO, "-p", "*-setup.exe", "-D", &d, "--clobber"], None, &store::gh_env(u));
            if !o.ok { return Err(o.stderr.trim().to_string()); }
        } else {
            let resp = ureq::get(asset["browser_download_url"].as_str().unwrap_or("")).call().map_err(|e| e.to_string())?;
            let mut f = std::fs::File::create(&path).map_err(|e| e.to_string())?;
            std::io::copy(&mut resp.into_reader(), &mut f).map_err(|e| e.to_string())?;
        }
        Ok(path)
    }).await.map_err(|e| e.to_string())??;
    std::process::Command::new(exe).spawn().map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub(crate) async fn release(st: St<'_>, pid: String, tag: String, files: Vec<String>, main: Option<bool>, sha: Option<String>) -> R<String> {
    let p = st.project(&pid).ok_or("no such project")?;
    let ci = st.cfg.lock().unwrap().ci_builds;
    tauri::async_runtime::spawn_blocking(move || store::release(&p.path, &p.gh, &tag, &files, ci, main.unwrap_or(false), sha.as_deref())).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn stage(st: St<'_>, pid: String, action: String, v: Option<String>, files: Option<Vec<String>>) -> R<Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    tauri::async_runtime::spawn_blocking(move || store::stage(&p.path, &action, v.as_deref().unwrap_or(""), &files.unwrap_or_default())).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn disk(st: St<'_>, pid: String) -> R<Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    tauri::async_runtime::spawn_blocking(move || json!(store::caches(&p.path).into_iter()
        .map(|(d, n)| json!({"dir": d, "size": n})).collect::<Vec<_>>())).await.map_err(|e| e.to_string())
}
#[tauri::command]
pub(crate) async fn disk_clean(st: St<'_>, pid: String, dirs: Vec<String>) -> R<u64> {
    let p = st.project(&pid).ok_or("no such project")?;
    tauri::async_runtime::spawn_blocking(move || store::clean(&p.path, &dirs)).await.map_err(|e| e.to_string())
}

/// One Storage Janitor pass with this config's projects (busy ones left alone) and the sessions Clauracle resumes.
pub(crate) fn janitor_pass(st: &State) -> u64 {
    let (ps, keep): (Vec<(String, bool)>, Vec<String>) = {
        let c = st.cfg.lock().unwrap();
        (c.projects.iter().map(|p| (p.path.clone(), p.working)).collect(),
         c.projects.iter().flat_map(|p| [p.session.clone(), p.prev_session.clone()]).flatten().collect())
    };
    store::janitor(&ps, &keep)
}
/// The `/janitor` command: the same pass as the timer, now. Bytes freed.
#[tauri::command]
pub(crate) async fn janitor(st: St<'_>) -> R<u64> {
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || janitor_pass(&st)).await.map_err(|e| e.to_string())
}

pub(crate) fn apple_dir(pid: &str) -> R<std::path::PathBuf> {
    let d = dirs::home_dir().ok_or("no home")?.join(".clauracle/apple").join(pid);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    Ok(d)
}

pub(crate) fn ssl(args: &[&str]) -> R<String> {
    let o = store::run("openssl", args, None, &[]);
    if o.ok { Ok(o.stdout) } else { Err(format!("openssl: {}", o.stderr.trim())) }
}

/// Signing request; reuses an existing key (a new key would orphan a cert already made from the old one).
/// Email = project's git email (Apple rewrites the subject anyway → no prompt). Copied to Downloads so Apple's upload
/// dialog finds it at once; returns that copy's path.
#[tauri::command(async)]
pub(crate) fn apple_csr(st: St, pid: String) -> R<String> {
    let p = st.project(&pid).ok_or("no such project")?;
    let d = apple_dir(&pid)?;
    let (key, csr) = (d.join("devid.key"), d.join("devid.csr"));
    if !key.is_file() || !csr.is_file() {
        let email = store::run("git", &["config", "user.email"], Some(&p.path), &[]).stdout.trim().replace(['/', '='], "");
        let email = if email.is_empty() { "dev@example.com".into() } else { email };
        ssl(&["req", "-new", "-newkey", "rsa:2048", "-nodes", "-keyout", &key.to_string_lossy(), "-out", &csr.to_string_lossy(),
              "-subj", &format!("/emailAddress={email}/CN=Developer ID")])?;
    }
    let Some(dl) = dirs::download_dir() else { return Ok(csr.to_string_lossy().into()) };
    let out = dl.join("devid.csr");
    std::fs::copy(&csr, &out).map_err(|e| e.to_string())?;
    Ok(out.to_string_lossy().into())
}

/// Newest .cer in Downloads made after our key (= the one Apple just gave for our CSR), if any. UI polls this.
#[tauri::command(async)]
pub(crate) fn apple_cer_find(pid: String) -> Option<String> {
    let since = std::fs::metadata(apple_dir(&pid).ok()?.join("devid.key")).ok()?.modified().ok()?;
    std::fs::read_dir(dirs::download_dir()?).ok()?.flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("cer")))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .filter(|(t, _)| *t > since).max().map(|(_, p)| p.to_string_lossy().into())
}

/// Apple's .cer + our key → .p12 → .env (APPLE_CERTIFICATE base64, password, signing identity, team id). Returns the identity.
#[tauri::command(async)]
pub(crate) fn apple_p12(st: St, pid: String, cer: String) -> R<String> {
    let p = st.project(&pid).ok_or("no such project")?;
    let d = apple_dir(&pid)?;
    let s = |x: &str| d.join(x).to_string_lossy().into_owned();
    if !d.join("devid.key").is_file() { return Err("make the signing request first".into()); }
    ssl(&["x509", "-inform", "DER", "-in", &cer, "-out", &s("devid.pem")])
        .or_else(|_| ssl(&["x509", "-in", &cer, "-out", &s("devid.pem")]))?; // .cer is DER; PEM accepted too
    let subj = ssl(&["x509", "-in", &s("devid.pem"), "-noout", "-subject", "-nameopt", "sep_multiline,utf8"])?;
    let field = |k: &str| subj.lines().find_map(|l| l.trim().strip_prefix(k)).unwrap_or("").to_string();
    let (cn, team) = (field("CN="), field("OU="));
    if !cn.starts_with("Developer ID Application") {
        return Err(format!("that's a \"{cn}\" certificate: make a Developer ID Application one"));
    }
    let pw = ssl(&["rand", "-hex", "16"])?.trim().to_string();
    let args = |legacy| { let mut a = vec!["pkcs12", "-export", "-inkey"]; if legacy { a.insert(2, "-legacy"); } a };
    let (k, pem, p12, pass) = (s("devid.key"), s("devid.pem"), s("devid.p12"), format!("pass:{pw}"));
    let rest = ["-in", pem.as_str(), "-out", p12.as_str(), "-passout", pass.as_str()];
    // -legacy (OpenSSL 3) ∵ macOS `security import` can't read its default AES p12; OpenSSL 1.x has no flag → retry without
    ssl(&[args(true).as_slice(), &[k.as_str()], &rest].concat()).or_else(|_| ssl(&[args(false).as_slice(), &[k.as_str()], &rest].concat()))?;
    let b64 = ssl(&["base64", "-A", "-in", &p12])?.trim().to_string();
    let env = std::path::Path::new(&p.path).join(".env");
    store::env_put(&env, &[("APPLE_CERTIFICATE", Some(b64.as_str())), ("APPLE_CERTIFICATE_PASSWORD", Some(pw.as_str())), ("APPLE_SIGNING_IDENTITY", Some(cn.as_str())), ("APPLE_TEAM_ID", Some(team.as_str()))])?;
    if store::git_ignored(&p.path, ".env") == Some(false) { env_ignore(st, pid, ".env".into())?; }
    Ok(cn)
}

/// Does this project build for macOS (a CI workflow mentions macos, or it's a Tauri app)? → Build tab offers Mac signing.
#[tauri::command(async)]
pub(crate) fn ships_mac(st: St, pid: String) -> R<bool> {
    let root = std::path::PathBuf::from(st.project(&pid).ok_or("no such project")?.path);
    Ok(root.join("src-tauri/tauri.conf.json").is_file() || std::fs::read_dir(root.join(".github/workflows")).into_iter().flatten().flatten()
        .any(|e| std::fs::read_to_string(e.path()).is_ok_and(|t| t.to_lowercase().contains("macos"))))
}

/// Bundle ID (= Apple App ID) from the project's config: Tauri `identifier`, Capacitor `appId`, Expo ios.bundleIdentifier.
/// {id, file, suggest}: suggest = com.<repo owner>.<repo name> (never from the email domain), for the user to accept/edit.
#[tauri::command(async)]
pub(crate) fn bundle_id(st: St, pid: String) -> R<serde_json::Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    let root = std::path::PathBuf::from(&p.path);
    let j = |f: &str| std::fs::read_to_string(root.join(f)).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).unwrap_or_default();
    // ponytail: JSON configs only; Xcode pbxproj / Info.plist / Gradle when a project needs them
    let found = [("src-tauri/tauri.conf.json", j("src-tauri/tauri.conf.json")["identifier"].clone()), ("capacitor.config.json", j("capacitor.config.json")["appId"].clone()),
        ("app.json", j("app.json")["expo"]["ios"]["bundleIdentifier"].clone())].into_iter().find_map(|(f, v)| v.as_str().map(|s| (f, s.to_string())));
    let url = store::run("git", &["remote", "get-url", "origin"], Some(&p.path), &[]).stdout;
    let seg = |s: &str| s.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect::<String>();
    let mut parts = url.trim().trim_end_matches(".git").rsplit(['/', ':']);
    let (name, owner) = (parts.next().map(seg).unwrap_or_default(), parts.next().map(seg).unwrap_or_default());
    // app name: last part of the current ID (keeps "clauracle" over repo "claude-term") → repo name → folder
    let name = found.as_ref().and_then(|f| f.1.rsplit('.').next().map(seg)).filter(|s| !s.is_empty())
        .unwrap_or(if name.is_empty() { seg(&root.file_name().unwrap_or_default().to_string_lossy()) } else { name });
    // #40: website domain (clauracle.arctim.no) wins → reversed (no.arctim.clauracle); a bare domain gets the app name appended
    let dom: Vec<String> = p.site["domain"].as_str().unwrap_or("").split('.').map(seg).filter(|s| !s.is_empty()).rev().collect();
    let suggest = if dom.len() >= 3 { dom.join(".") } else if dom.len() == 2 { format!("{}.{name}", dom.join(".")) }
        else if owner.is_empty() { format!("com.example.{name}") } else { format!("com.{owner}.{name}") };
    Ok(serde_json::json!({"id": found.as_ref().map(|f| &f.1), "file": found.as_ref().map(|f| f.0), "suggest": suggest}))
}

/// #161 website host setup done by code instead of the user: ghpages = Pages on w/ Actions source (gh api);
/// cfpages = CLOUDFLARE_ACCOUNT_ID from the token; netlify = new site → NETLIFY_SITE_ID; #164 services supabase/firebase too. → value shown/filled.
#[tauri::command(async)]
pub(crate) fn site_auto(st: St, pid: String, host: String) -> R<String> {
    let p = st.project(&pid).ok_or("no such project")?;
    let envp = std::path::Path::new(&p.path).join(".env");
    let text = std::fs::read_to_string(&envp).unwrap_or_default();
    let get = |k: &str| store::env_parse(&text).into_iter().find(|(x, v)| x == k && !v.is_empty()).map(|(_, v)| v).ok_or(format!("fill {k} first"));
    let fill = |k: &str, v: &str| store::env_put(&envp, &[(k, Some(v))]).map(|_| v.to_string()); // fresh read ∵ text is from before the API call
    let err = |e: ureq::Error| match e { ureq::Error::Status(c, r) => format!("{c}: {}", r.into_string().unwrap_or_default()), e => e.to_string() };
    match host.as_str() {
        "ghpages" => { // POST = create; 409 = exists → PUT switches its source to Actions
            let env = store::gh_env(&p.gh);
            let a = ["-f", "build_type=workflow"];
            let o = store::run("gh", &[&["api", "-X", "POST", "repos/{owner}/{repo}/pages"], &a[..]].concat(), Some(&p.path), &env);
            if o.ok { return Ok("on".into()); }
            let o2 = store::run("gh", &[&["api", "-X", "PUT", "repos/{owner}/{repo}/pages"], &a[..]].concat(), Some(&p.path), &env);
            if o2.ok { Ok("on".into()) } else { Err(format!("{} {}", o.stderr.trim(), o2.stderr.trim())) }
        }
        "cfpages" => {
            let v: Value = ureq::get("https://api.cloudflare.com/client/v4/accounts").set("Authorization", &format!("Bearer {}", get("CLOUDFLARE_API_TOKEN")?))
                .call().map_err(err)?.into_string().ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
            // ponytail: first account; several → user edits the key
            fill("CLOUDFLARE_ACCOUNT_ID", v["result"][0]["id"].as_str().ok_or("token sees no account: give it Account › Cloudflare Pages: Edit")?)
        }
        "netlify" => {
            let v: Value = ureq::post("https://api.netlify.com/api/v1/sites").set("Authorization", &format!("Bearer {}", get("NETLIFY_AUTH_TOKEN")?))
                .set("Content-Type", "application/json").send_string("{}").map_err(err)?.into_string().ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
            fill("NETLIFY_SITE_ID", v["id"].as_str().ok_or("Netlify returned no site id")?)
        }
        "stripe" => { // #163 webhook endpoint on the website address → STRIPE_WEBHOOK_SECRET (secret only in the create response)
            let d = p.site["domain"].as_str().filter(|d| !d.is_empty()).ok_or("set the website address under Website first")?;
            let url = format!("https://{d}/api/stripe/webhook"); // ponytail: fixed path; the AI wires its handler there
            let ev = ["checkout.session.completed", "customer.subscription.created", "customer.subscription.updated", "customer.subscription.deleted", "invoice.payment_failed"];
            let mut form = vec![("url", url.as_str())];
            form.extend(ev.iter().map(|e| ("enabled_events[]", *e)));
            let v: Value = ureq::post("https://api.stripe.com/v1/webhook_endpoints").set("Authorization", &format!("Bearer {}", get("STRIPE_SECRET_KEY")?))
                .send_form(&form).map_err(err)?.into_string().ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
            fill("STRIPE_WEBHOOK_SECRET", v["secret"].as_str().ok_or("Stripe returned no signing secret")?).map(|_| format!("STRIPE_WEBHOOK_SECRET ({url})"))
        }
        "sshkey" => { // #166 deploy key → .env; → pubkey (UI copies it for the panel)
            let name = std::path::Path::new(&p.path).file_name().unwrap_or_default().to_string_lossy().to_string();
            store::deploy_key(&envp, &format!("clauracle-deploy-{name}"))
        }
        "supabase" => { // #164 Management API w/ a personal token → URL + legacy anon/service_role keys
            let tok = format!("Bearer {}", get("SUPABASE_ACCESS_TOKEN")?);
            let api = |path: &str| -> R<Value> { ureq::get(&format!("https://api.supabase.com/v1/{path}")).set("Authorization", &tok)
                .call().map_err(err)?.into_string().map_err(|e| e.to_string()).and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string())) };
            let ps = api("projects")?;
            let ps = ps.as_array().cloned().unwrap_or_default();
            // pick: ref in a filled SUPABASE_URL → only project → name = folder name; ponytail: no create (needs org, region, db password)
            let want = get("SUPABASE_URL").ok().and_then(|u| u.split("//").nth(1)?.split('.').next().map(String::from));
            let dir = std::path::Path::new(&p.path).file_name().unwrap_or_default().to_string_lossy().to_lowercase();
            let pr = ps.iter().find(|x| want.as_deref().is_some_and(|w| x["id"] == w))
                .or(if ps.len() == 1 { ps.first() } else { None })
                .or_else(|| ps.iter().find(|x| x["name"].as_str().is_some_and(|n| n.to_lowercase() == dir)))
                .ok_or_else(|| if ps.is_empty() { "make a Supabase project first".to_string() } else {
                    format!("several projects ({}): fill SUPABASE_URL with the one to use", ps.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(", ")) })?;
            let r = pr["id"].as_str().ok_or("project without id")?;
            let keys = api(&format!("projects/{r}/api-keys?reveal=true"))?;
            let key = |n: &str| keys.as_array().and_then(|a| a.iter().find(|k| k["name"] == n)).and_then(|k| k["api_key"].as_str()).map(String::from).ok_or(format!("no {n} key"));
            let (anon, svc) = (key("anon")?, key("service_role")?);
            let url = format!("https://{r}.supabase.co");
            store::env_put(&envp, &[("SUPABASE_URL", Some(url.as_str())), ("SUPABASE_ANON_KEY", Some(anon.as_str())), ("SUPABASE_SERVICE_ROLE_KEY", Some(svc.as_str()))])?;
            Ok("SUPABASE_URL + keys".into())
        }
        "firebase" => { // #164 Firebase Management API w/ the project's gcloud account: addFirebase if needed → web app → its config
            if p.gcp.is_empty() { return Err("pick the Google Cloud project under the Google service first".into()); }
            let g = p.gcp.clone();
            let _ = store::gcloud(&["services", "enable", "firebase.googleapis.com"], &p.gaccount, &g); // already on → no-op
            let o = store::run("gcloud", &["auth", "print-access-token"], None, &store::gcloud_env(&p.gaccount, ""));
            if !o.ok { return Err(format!("gcloud: {}", o.stderr.trim())); }
            let tok = format!("Bearer {}", o.stdout.trim());
            let fb = |m: &str, path: &str, body: Option<&str>| -> R<Value> {
                let rq = ureq::request(m, &format!("https://firebase.googleapis.com/v1beta1/{path}")).set("Authorization", &tok).set("x-goog-user-project", &g);
                let rs = match body { Some(b) => rq.set("Content-Type", "application/json").send_string(b), None => rq.call() };
                rs.map_err(err)?.into_string().map_err(|e| e.to_string()).and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
            };
            // long-running op → poll ≤60s
            let wait = |mut v: Value| -> R<Value> {
                for _ in 0..30 {
                    if v["done"] == true { return if v["error"].is_null() { Ok(v["response"].clone()) } else { Err(v["error"]["message"].as_str().unwrap_or("failed").into()) }; }
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    v = fb("GET", v["name"].as_str().ok_or("no operation name")?, None)?;
                }
                Err("Firebase took too long: try again in a minute".into())
            };
            let apps = match fb("GET", &format!("projects/{g}/webApps"), None) {
                Ok(v) => v,
                Err(e) if e.starts_with("404") || e.starts_with("403") => { wait(fb("POST", &format!("projects/{g}:addFirebase"), Some("{}"))?)?; Value::Null }
                Err(e) => return Err(e),
            };
            let id = match apps["apps"][0]["appId"].as_str() {
                Some(a) => a.to_string(),
                None => wait(fb("POST", &format!("projects/{g}/webApps"), Some(&serde_json::json!({"displayName": g}).to_string()))?)?["appId"].as_str().ok_or("no app id")?.to_string(),
            };
            let c = fb("GET", &format!("projects/{g}/webApps/{id}/config"), None)?;
            let s = |k: &str| c[k].as_str().unwrap_or("").to_string();
            let kv = [("FIREBASE_API_KEY", s("apiKey")), ("FIREBASE_AUTH_DOMAIN", s("authDomain")), ("FIREBASE_PROJECT_ID", s("projectId")), ("FIREBASE_APP_ID", s("appId"))];
            store::env_put(&envp, &kv.iter().map(|(k, v)| (*k, Some(v.as_str()))).collect::<Vec<_>>())?;
            Ok("Firebase web config".into())
        }
        "play" => { // #165 service account in the project's Cloud project → JSON key (compact, one .env line) → email (UI copies it for the Play Console invite; no API for that)
            if p.gcp.is_empty() { return Err("pick the Google Cloud project under the Google service first".into()); }
            let g = p.gcp.clone();
            let _ = store::gcloud(&["services", "enable", "androidpublisher.googleapis.com"], &p.gaccount, &g);
            if let Err(e) = store::gcloud(&["iam", "service-accounts", "create", "clauracle-play", "--display-name=Google Play releases"], &p.gaccount, &g) {
                if !e.contains("already exists") { return Err(format!("gcloud: {e}")); }
            }
            let email = format!("clauracle-play@{g}.iam.gserviceaccount.com");
            let tmp = std::env::temp_dir().join(format!("clauracle-play-{}.json", std::process::id()));
            let r = store::gcloud(&["iam", "service-accounts", "keys", "create", &tmp.to_string_lossy(), &format!("--iam-account={email}")], &p.gaccount, &g)
                .and_then(|_| std::fs::read_to_string(&tmp).map_err(|e| e.to_string()));
            let _ = std::fs::remove_file(&tmp); // private key: never leave it in temp
            let key: Value = serde_json::from_str(&r.map_err(|e| format!("gcloud: {e}"))?).map_err(|e| e.to_string())?;
            fill("PLAY_SERVICE_ACCOUNT_JSON", &key.to_string())?;
            Ok(email)
        }
        _ => Err("nothing to automate for this host".into()),
    }
}

/// #166 plain SSH/VPS: deploy pubkey → SITE_SSH_USER@SITE_SSH_HOST's authorized_keys w/ one password login (never stored).
#[tauri::command(async)]
pub(crate) fn site_key_install(st: St, pid: String, pw: String) -> R<()> {
    let p = st.project(&pid).ok_or("no such project")?;
    let env = store::env_parse(&std::fs::read_to_string(std::path::Path::new(&p.path).join(".env")).unwrap_or_default());
    let get = |k: &str| env.iter().find(|(x, v)| x == k && !v.is_empty()).map(|(_, v)| v.clone()).ok_or(format!("fill {k} first"));
    let (host, user, k) = (get("SITE_SSH_HOST")?, get("SITE_SSH_USER")?, get("SITE_SSH_PUBLIC_KEY")?);
    store::ssh_key_install(&format!("{user}@{host}"), &k, &pw).map_err(|e| e.replace("{who}", "SSH"))
}

/// #40: HTTP status per URL (0 = unreachable); the release check uses it for /privacy + /terms being live.
#[tauri::command(async)]
pub(crate) fn site_check(urls: Vec<String>) -> Vec<u16> {
    let a = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(8)).build();
    urls.iter().map(|u| match a.get(u).set("User-Agent", "Clauracle").call() {
        Ok(r) => r.status(), Err(ureq::Error::Status(c, _)) => c, Err(_) => 0 }).collect()
}

pub(crate) fn version_of(root: &std::path::Path) -> Option<(String, &'static str)> {
    ["src-tauri/tauri.conf.json", "package.json", "Cargo.toml", "src-tauri/Cargo.toml", "pyproject.toml"].into_iter().find_map(|f| {
        let t = std::fs::read_to_string(root.join(f)).ok()?;
        let v = if f.ends_with(".json") { serde_json::from_str::<Value>(&t).ok()?["version"].as_str()?.to_string() } else { toml_version(&t)? };
        // tauri.conf "version": "../package.json" = path ref → not a version, try the next file
        v.starts_with(|c: char| c.is_ascii_digit()).then_some((v, f))
    })
}
/// PNG size from the IHDR header (bytes 16..24: width, height BE) → shorter side; 0 = unreadable
pub(crate) fn png_px(f: &std::path::Path) -> u32 {
    use std::io::Read;
    let mut b = [0u8; 24];
    std::fs::File::open(f).and_then(|mut h| h.read_exact(&mut b)).map_or(0, |_| if &b[1..4] == b"PNG" { u32::from_be_bytes(b[16..20].try_into().unwrap()).min(u32::from_be_bytes(b[20..24].try_into().unwrap())) } else { 0 })
}
pub(crate) fn toml_version(t: &str) -> Option<String> {
    t.lines().find_map(|l| l.trim().strip_prefix("version")?.trim_start().strip_prefix('=')?.trim().strip_prefix('"')?.split('"').next().map(String::from))
}

/// #360 checklist Fix/Run = headless `claude -p` in the project (no chat turn) → its final text. Edits auto-accepted;
/// shell limited to git/gh/npm/cargo (CI prompts push). ponytail: no live progress, UI spins till it returns.
#[tauri::command(async)]
pub(crate) fn rel_fix(st: St, pid: String, prompt: String) -> R<String> {
    let p = st.project(&pid).ok_or("project removed")?;
    let env: Vec<(String, String)> = store::gh_env(&p.gh).into_iter().chain(store::gcloud_env(&p.gaccount, &p.gcp)).collect();
    claude_p(&st, &p, &["--permission-mode", "acceptEdits", "--allowedTools", "Read,Write,Edit,Glob,Grep,Bash(git:*),Bash(gh:*),Bash(npm:*),Bash(npx:*),Bash(cargo:*)"],
        std::path::Path::new(&p.path), env, &prompt)
}

/// Release wizard facts, plain checks, no AI: files present, version vs last tag, git clean, last CI run, Mac secrets.
#[tauri::command]
pub(crate) async fn release_check(st: St<'_>, pid: String) -> R<Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    tauri::async_runtime::spawn_blocking(move || {
        let root = std::path::PathBuf::from(&p.path);
        let files: Vec<(String, std::path::PathBuf)> = std::fs::read_dir(&root).into_iter().flatten().flatten().map(|e| (e.file_name().to_string_lossy().to_uppercase(), e.path())).collect();
        let has = |pre: &[&str]| files.iter().any(|(n, _)| pre.iter().any(|x| n.starts_with(x)));
        // #264 licence id guessed from the file text → preselect on Confirm. ponytail: first-match keywords, not SPDX matching
        let lic_f = files.iter().find(|(n, _)| ["LICENSE", "LICENCE", "COPYING"].iter().any(|x| n.starts_with(x))).map(|(_, f)| f);
        let lic_id = lic_f.and_then(|f| std::fs::read_to_string(f).ok()).map_or("", |t| lic_guess(&t));
        let lic_file = lic_f.and_then(|f| f.file_name()).map(|n| n.to_string_lossy().to_string()); // #363 row label = real name
        let readme = files.iter().find(|(n, _)| n.starts_with("README")).and_then(|(_, f)| std::fs::read_to_string(f).ok()).map_or(0, |t| t.lines().count());
        let ver = version_of(&root);
        let git = |a: &[&str]| store::run("git", a, Some(&p.path), &[]);
        // #119: highest version tag incl. remote ones (gh release create tags on GitHub only → describe saw v0.1.1 after v0.1.2)
        let _ = store::run("git", &["fetch", "-q", "--tags"], Some(&p.path), &store::gh_env(&p.gh));
        let tag = git(&["tag", "--list", "v[0-9]*", "--sort=-v:refname"]);
        let tag = store::Out { stdout: tag.stdout.lines().next().unwrap_or("").to_string(), ok: tag.ok && !tag.stdout.trim().is_empty(), ..tag };
        // #237 release icon source: SVG wins, else biggest PNG (px = shorter side); px None = vector / can't tell (ico, Android res dir)
        let icon = ["assets/icon.svg", "src-tauri/icons/icon.svg", "icon.svg", "public/favicon.svg", "src-tauri/icons/icon.png", "assets/icon.png", "icon.png", "public/favicon.ico", "app/src/main/res"]
            .iter().filter(|f| root.join(f).exists()).map(|f| (*f, if f.ends_with(".png") { Some(png_px(&root.join(f))) } else if f.ends_with(".svg") { None } else { Some(0) }))
            .max_by_key(|(_, px)| px.unwrap_or(u32::MAX));
        let remote = store::repo_of(&p.path).is_some();
        // target platforms the CI workflows build (#27): plain text match on runner/target names. ponytail: no YAML parse
        let wf: String = std::fs::read_dir(root.join(".github/workflows")).into_iter().flatten().flatten()
            .filter_map(|e| std::fs::read_to_string(e.path()).ok()).collect::<Vec<_>>().join("\n").to_lowercase();
        let ci_os: Vec<&str> = [("windows", "windows-"), ("macos", "macos-"), ("linux", "ubuntu-"), ("ios", "ios"), ("android", "android")]
            .into_iter().filter(|(_, k)| wf.contains(k)).map(|(t, _)| t).collect();
        // #123: last run; failed → first job annotation as `why` (billing/spending-limit refusals show only there)
        let mut ci = if remote { store::gh_json(&["run", "list", "--limit", "1", "--json", "databaseId,conclusion,status,workflowName,url,headBranch"], &p.gh, Some(&p.path)).get(0).cloned() } else { None };
        if let Some(r) = ci.as_mut().filter(|r| r["conclusion"] == "failure") {
            let id = r["databaseId"].to_string();
            let job = store::run("gh", &["run", "view", &id, "--json", "jobs", "--jq", ".jobs[0].databaseId"], Some(&p.path), &store::gh_env(&p.gh)).stdout.trim().to_string();
            if !job.is_empty() {
                let why = store::run("gh", &["api", &format!("repos/{{owner}}/{{repo}}/check-runs/{job}/annotations"), "--jq", ".[0].message // \"\""], Some(&p.path), &store::gh_env(&p.gh)).stdout.trim().to_string();
                // #133 billing block → exact budgets page (user vs org). ponytail: no user budget API ∴ link, not fix-in-app
                let w = why.to_lowercase();
                if ["spending limit", "billing", "payment", "budget"].iter().any(|k| w.contains(k)) {
                    if let Some((owner, _)) = store::repo_of(&p.path) {
                        r["bill"] = json!(if owner.eq_ignore_ascii_case(&p.gh) { "https://github.com/settings/billing/budgets".to_string() }
                                          else { format!("https://github.com/organizations/{owner}/settings/billing/budgets") });
                    }
                }
                r["why"] = json!(why);
            }
        }
        // #350 README stale = untouched since the last tag while code changed after it. ponytail: commit times, no content check
        let ct = |a: &[&str]| git(a).stdout.trim().parse::<u64>().unwrap_or(0);
        let tag_t = if tag.ok { ct(&["log", "-1", "--format=%ct", tag.stdout.trim()]) } else { 0 };
        let code_t = store::code_time(&p.path);
        let readme_old = tag_t > 0 && code_t > tag_t && ct(&["log", "-1", "--format=%ct", "--", "README*"]) < tag_t;
        let deps = lic_deps(&root, &p.license); // #412
        let cmiss = credits_miss(&root, &deps);
        json!({"deps": deps, "credits_miss": cmiss, "readme": readme, "readme_old": readme_old, "credits": has(&["CREDITS"]), "ci_os": ci_os, "license": has(&["LICENSE", "LICENCE", "COPYING"]), "lic_id": lic_id, "lic_file": lic_file,"changelog": has(&["CHANGELOG", "HISTORY", "RELEASES"]),
               "version": ver.as_ref().map(|v| &v.0), "vfile": ver.as_ref().map(|v| v.1), "tag": if tag.ok { tag.stdout.trim() } else { "" },
               "clean": git(&["status", "--porcelain"]).stdout.trim().is_empty(), "icon": icon.map(|i| i.0), "icon_px": icon.and_then(|i| i.1), "remote": remote, "code_t": code_t,
               "ci": ci, "sign": store::sign_env(&p.path).into_iter().map(|(k, _)| k).collect::<Vec<_>>()})
    }).await.map_err(|e| e.to_string())
}

/// #128 Actions usage this month for the repo owner (user or org) via the billing usage report. Needs the token's `user`
/// scope (org: admin) → `{scope}` when missing. ponytail: no budget/spending-limit API for users → UI links the billing page.
#[tauri::command(async)]
pub(crate) fn actions_usage(st: St, pid: String, year: u32, month: u32) -> R<Value> {
    let p = st.project(&pid).ok_or("no such project")?;
    let (owner, _) = store::repo_of(&p.path).ok_or("not on GitHub")?;
    let me = owner.eq_ignore_ascii_case(&p.gh);
    let base = if me { format!("users/{owner}") } else { format!("organizations/{owner}") };
    let env = store::gh_env(&p.gh);
    let o = store::run("gh", &["api", &format!("{base}/settings/billing/usage?year={year}&month={month}")], None, &env);
    if !o.ok {
        let e = format!("{}{}", o.stdout, o.stderr);
        if e.contains("scope") || e.contains("404") || e.contains("403") { return Ok(json!({"owner": owner, "org": !me, "scope": true, "why": e.lines().last().unwrap_or("").trim()})); }
        return Err(e.trim().to_string());
    }
    let v: Value = serde_json::from_str(&o.stdout).unwrap_or_default();
    Ok(json!({"owner": owner, "org": !me, "plan": if me { store::run("gh", &["api", "user", "--jq", ".plan.name"], None, &env).stdout.trim().to_string() } else { String::new() },
              "u": actions_sum(&v)}))
}

/// usageItems → {min, sku: {name: minutes}, gross, net} for product "actions" (minutes rows only for min/sku).
pub(crate) fn actions_sum(v: &Value) -> Value {
    let (mut min, mut gross, mut net, mut sku) = (0.0, 0.0, 0.0, serde_json::Map::new());
    for i in v["usageItems"].as_array().into_iter().flatten().filter(|i| i["product"].as_str().is_some_and(|s| s.eq_ignore_ascii_case("actions"))) {
        gross += i["grossAmount"].as_f64().unwrap_or(0.0);
        net += i["netAmount"].as_f64().unwrap_or(0.0);
        if i["unitType"].as_str().is_some_and(|s| s.eq_ignore_ascii_case("minutes")) {
            let q = i["quantity"].as_f64().unwrap_or(0.0);
            min += q;
            let k = i["sku"].as_str().unwrap_or("?").trim_start_matches("Actions ").to_string();
            let e = sku.entry(k).or_insert(json!(0.0));
            *e = json!(e.as_f64().unwrap_or(0.0) + q);
        }
    }
    json!({"min": min, "sku": sku, "gross": gross, "net": net})
}

#[test]
pub(crate) fn actions_sum_adds_minutes_per_sku() {
    let v = json!({"usageItems": [
        {"product": "actions", "sku": "Actions Linux", "unitType": "Minutes", "quantity": 100.0, "grossAmount": 0.8, "netAmount": 0.0},
        {"product": "actions", "sku": "Actions Linux", "unitType": "Minutes", "quantity": 20.0, "grossAmount": 0.16, "netAmount": 0.0},
        {"product": "actions", "sku": "Actions macOS 3-core", "unitType": "Minutes", "quantity": 5.0, "grossAmount": 0.4, "netAmount": 0.4},
        {"product": "actions", "sku": "Actions storage", "unitType": "GigabyteHours", "quantity": 9.0, "grossAmount": 0.1, "netAmount": 0.1},
        {"product": "git_lfs", "sku": "LFS", "unitType": "Minutes", "quantity": 7.0, "grossAmount": 1.0, "netAmount": 1.0}]});
    let s = actions_sum(&v);
    assert_eq!(s["min"], 125.0);
    assert_eq!(s["sku"]["Linux"], 120.0);
    assert!((s["net"].as_f64().unwrap() - 0.5).abs() < 1e-9);
}

/// Page → the `ui` self-test tool's waiting call (claude::ui_tool).
#[tauri::command]
pub(crate) fn ui_reply(id: u64, text: String) {
    let mut w = claude::UI_WAIT.lock().unwrap();
    if let Some(i) = w.iter().position(|(n, _)| *n == id) { let _ = w.remove(i).1.send(text); }
}

/// Release panel "Set version": swaps the version in every version file that exists (first `version` key/line only,
/// formatting kept). Cargo.lock follows on the next cargo build.
#[tauri::command(async)]
pub(crate) fn version_set(st: St, pid: String, v: String) -> R<Vec<String>> {
    if v.is_empty() || !v.chars().all(|c| c.is_ascii_alphanumeric() || ".-+".contains(c)) { return Err("bad version".into()); }
    let root = std::path::PathBuf::from(&st.project(&pid).ok_or("no such project")?.path);
    let mut done = vec![];
    for f in ["src-tauri/tauri.conf.json", "package.json", "Cargo.toml", "src-tauri/Cargo.toml", "pyproject.toml"] {
        let Ok(t) = std::fs::read_to_string(root.join(f)) else { continue };
        if let Some(n) = swap_version(&t, &v) { std::fs::write(root.join(f), n).map_err(|e| e.to_string())?; done.push(f.to_string()); }
    }
    if done.is_empty() { Err("no version found (tauri.conf.json / package.json / Cargo.toml / pyproject.toml)".into()) } else { Ok(done) }
}
/// json: first `"version": "x"`; toml: first line `version = "x"` (= [package] in practice). None = no version there.
pub(crate) fn swap_version(t: &str, v: &str) -> Option<String> {
    let at = t.find("\"version\"").or_else(|| t.split_inclusive('\n').scan(0, |o, l| { let s = *o; *o += l.len(); Some((s, l)) }) /* ≠ lines(): +1 was off by one per CRLF */
        .find(|(_, l)| l.starts_with("version") && l[7..].trim_start().starts_with('=')).map(|(s, _)| s))?;
    let q1 = at + t[at..].find(['=', ':'])? + 1;
    let q1 = q1 + t[q1..].find('"')? + 1;
    let q2 = q1 + t[q1..].find('"')?;
    Some(format!("{}{v}{}", &t[..q1], &t[q2..]))
}
#[cfg(test)]
#[test]
pub(crate) fn swap_version_works() {
    assert_eq!(swap_version("{\n  \"name\": \"a\",\n  \"version\": \"0.1.0\"\n}", "0.1.1").unwrap(), "{\n  \"name\": \"a\",\n  \"version\": \"0.1.1\"\n}");
    assert_eq!(swap_version("[package]\nname = \"x\"\nversion = \"0.1.0\"\n[dependencies]\nfoo = { version = \"1\" }", "0.2.0").unwrap(),
               "[package]\nname = \"x\"\nversion = \"0.2.0\"\n[dependencies]\nfoo = { version = \"1\" }");
    assert!(swap_version("[workspace]\nmembers = []", "1.0.0").is_none());
}
#[cfg(test)]
#[test]
pub(crate) fn png_px_reads_header() {
    let d = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    assert_eq!(png_px(&d.join("icons/32x32.png")), 32);
    assert_eq!(png_px(&d.join("Cargo.toml")), 0);
}

/// Writes a new bundle ID into the config file it was found in (string swap keeps formatting).
#[tauri::command(async)]
pub(crate) fn bundle_id_set(st: St, pid: String, id: String) -> R<()> {
    let ok = id.split('.').count() >= 2 && id.split('.').all(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    if !ok { return Err("use reverse-domain form: letters, digits, - and dots, like com.you.app".into()); }
    let cur = bundle_id(st.clone(), pid.clone())?;
    let (Some(old), Some(file)) = (cur["id"].as_str(), cur["file"].as_str()) else { return Err("no config with a bundle ID (tauri.conf.json identifier / capacitor appId / app.json)".into()) };
    let path = std::path::Path::new(&st.project(&pid).ok_or("no such project")?.path).join(file);
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    std::fs::write(&path, text.replacen(&format!("\"{old}\""), &format!("\"{id}\""), 1)).map_err(|e| e.to_string())
}

/// Names of the repo's GitHub Actions secrets.
#[tauri::command(async)]
pub(crate) fn gh_secrets(st: St, pid: String) -> R<Vec<String>> {
    let p = st.project(&pid).ok_or("no such project")?;
    let v = store::gh_json(&["secret", "list", "--json", "name"], &p.gh, Some(&p.path));
    Ok(v.as_array().ok_or("gh couldn't list secrets (no repo / no access?)")?.iter().filter_map(|x| x["name"].as_str().map(String::from)).collect())
}

/// .env values of `keys` → repo secrets, via a temp dotenv file (no values on command lines), deleted right after.
#[tauri::command(async)]
pub(crate) fn gh_secrets_set(st: St, pid: String, keys: Vec<String>) -> R<usize> {
    let p = st.project(&pid).ok_or("no such project")?;
    let text = std::fs::read_to_string(std::path::Path::new(&p.path).join(".env")).unwrap_or_default();
    let vals: Vec<(String, String)> = store::env_parse(&text).into_iter().filter(|(k, v)| keys.contains(k) && !v.is_empty()).collect();
    if vals.is_empty() { return Err("none of those keys have a value in .env".into()); }
    let tmp = apple_dir(&pid)?.join("secrets.env");
    std::fs::write(&tmp, vals.iter().map(|(k, v)| format!("{k}={v}\n")).collect::<String>()).map_err(|e| e.to_string())?;
    let o = store::run("gh", &["secret", "set", "-f", &tmp.to_string_lossy()], Some(&p.path), &store::gh_env(&p.gh));
    let _ = std::fs::remove_file(&tmp);
    if o.ok { Ok(vals.len()) } else { Err(o.stderr.trim().to_string()) }
}

// ---- code view ----
