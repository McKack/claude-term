//! Built-in local models: llama.cpp's `llama-server` (MIT), downloaded on first use for this machine's GPU, run as a
//! hidden child that serves the Anthropic Messages API, so Claude Code talks to it like any custom endpoint.
use crate::*;
use std::path::{Path, PathBuf};

/// (id, name, Hugging Face repo/file, size GB, memory needed GB incl. a 32k context, coding/agent score). Ordered by need.
/// ponytail: score = rough 0-100 estimate from public leaderboards and user reports (2026-11, data was thin); refresh with the list.
/// llama-server fits the context to free memory (--fit, its default) but never below 32k: Claude Code alone needs ~20k.
/// Checked against huggingface.co 2026-11-13: official ggml-org builds where one fits (Qwen 3.5 9B has none: unsloth; Devstral Small 2
/// only as Q8_0 there, 25 GB: LM Studio's Q4_K_M). Newer models come out often, update the list from time to time.
const MODELS: [(&str, &str, &str, f64, u32, u32); 9] = [
    ("qwen3.5-4b", "Qwen 3.5 4B", "ggml-org/Qwen3.5-4B-GGUF/Qwen3.5-4B-Q4_K_M.gguf", 3.0, 6, 40),
    ("gemma-4-e4b", "Gemma 4 E4B", "ggml-org/gemma-4-E4B-it-GGUF/gemma-4-E4B-it-Q4_0.gguf", 4.6, 8, 42),
    ("qwen3.5-9b", "Qwen 3.5 9B", "unsloth/Qwen3.5-9B-GGUF/Qwen3.5-9B-Q4_K_M.gguf", 5.7, 10, 52),
    ("gemma-4-12b", "Gemma 4 12B", "ggml-org/gemma-4-12B-it-GGUF/gemma-4-12B-it-Q4_0.gguf", 7.2, 12, 55),
    ("gpt-oss-20b", "gpt-oss 20B", "ggml-org/gpt-oss-20b-GGUF/gpt-oss-20b-MXFP4.gguf", 12.1, 16, 64),
    ("gemma-4-26b", "Gemma 4 26B A4B", "ggml-org/gemma-4-26B-A4B-it-GGUF/gemma-4-26B-A4B-it-Q4_0.gguf", 14.6, 20, 68),
    ("devstral-24b", "Devstral Small 2 24B", "lmstudio-community/Devstral-Small-2-24B-Instruct-2512-GGUF/Devstral-Small-2-24B-Instruct-2512-Q4_K_M.gguf", 14.3, 20, 62),
    ("qwen3.8-27b", "Qwen 3.8 27B", "ggml-org/Qwen3.8-27B-GGUF/Qwen3.8-27B-Q4_K_M.gguf", 19.4, 26, 74),
    ("qwen3.6-35b", "Qwen 3.6 35B A3B", "ggml-org/Qwen3.6-35B-A3B-GGUF/Qwen3.6-35B-A3B-Q4_K_M.gguf", 20.5, 28, 72),
];

pub fn dir(cfg: &store::Config) -> PathBuf {
    if cfg.local_dir.trim().is_empty() { store::home().join("local") } else { PathBuf::from(cfg.local_dir.trim()) }
}

fn model_file(dir: &Path, id: &str) -> Option<PathBuf> {
    MODELS.iter().find(|m| m.0 == id).map(|m| dir.join("models").join(m.2.rsplit('/').next().unwrap()))
}

/// (RAM GB, NVIDIA VRAM GB, backend name used to pick the llama.cpp build), measured once per run.
fn hw() -> (u32, u32, &'static str) {
    static HW: std::sync::OnceLock<(u32, u32, &str)> = std::sync::OnceLock::new();
    *HW.get_or_init(|| {
        let gb = |b: f64| (b / 1e9).round() as u32;
        let ram = if cfg!(windows) {
            store::run("powershell", &["-NoProfile", "-Command", "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory"], None, &[]).stdout.trim().parse().map_or(0, gb)
        } else if cfg!(target_os = "macos") {
            store::run("sysctl", &["-n", "hw.memsize"], None, &[]).stdout.trim().parse().map_or(0, gb)
        } else {
            std::fs::read_to_string("/proc/meminfo").ok().and_then(|t| t.lines().next()?.split_whitespace().nth(1)?.parse::<f64>().ok()).map_or(0, |k| gb(k * 1024.0))
        };
        let smi = store::run("nvidia-smi", &["--query-gpu=memory.total", "--format=csv,noheader,nounits"], None, &[]);
        let vram = smi.stdout.lines().next().and_then(|l| l.trim().parse::<f64>().ok()).map_or(0, |m| gb(m * 1.048576e6));
        let vulkan = if cfg!(windows) { Path::new("C:/Windows/System32/vulkan-1.dll").exists() } else { ["/usr/lib/x86_64-linux-gnu/libvulkan.so.1", "/usr/lib/libvulkan.so.1", "/usr/lib64/libvulkan.so.1"].iter().any(|p| Path::new(p).exists()) };
        let be = if cfg!(target_os = "macos") { "metal" } else if smi.ok && vram > 0 { "cuda" } else if vulkan { "vulkan" } else { "cpu" };
        (ram, vram, be)
    })
}

/// Models pulled into a running Ollama, each marked cloud (runs on Ollama's servers, name ends in -cloud) or local,
/// then Ollama's cloud catalog (tool-capable ones not pulled yet) under the names a local Ollama serves them by.
/// Fetched here, not from the page, because Ollama's CORS allow-list may not include the app's origin.
#[tauri::command(async)]
pub(crate) fn ollama_models() -> Vec<(String, bool)> {
    let get = |u: &str, s| -> Value { ureq::get(u).timeout(std::time::Duration::from_secs(s)).call().ok()
        .and_then(|r| r.into_string().ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default() };
    let mut v: Vec<(String, bool)> = get("http://localhost:11434/api/tags", 2)["models"].as_array().into_iter().flatten()
        .filter_map(|m| m["name"].as_str().map(|n| (n.to_string(), is_cloud(n, m)))).collect();
    for m in get("https://ollama.com/api/tags", 5)["models"].as_array().into_iter().flatten() {
        let (Some(n), true) = (m["name"].as_str(), m["capabilities"].as_array().is_some_and(|c| c.iter().any(|c| c == "tools"))) else { continue };
        let n = cloud_name(n);
        if !v.iter().any(|x| x.0 == n) { v.push((n, true)); }
    }
    v
}
/// ollama.com lists "glm-5.3" / "qwen3-coder:480b"; a local Ollama runs them as "glm-5.3:cloud" / "qwen3-coder:480b-cloud"
fn cloud_name(n: &str) -> String { if n.contains(':') { format!("{n}-cloud") } else { format!("{n}:cloud") } }

/// A cloud model has to be pulled (a small stub) before a local Ollama serves it.
#[tauri::command(async)]
pub(crate) fn ollama_pull(model: String) -> R<()> {
    ureq::post("http://localhost:11434/api/pull").timeout(std::time::Duration::from_secs(120))
        .set("Content-Type", "application/json").send_string(&json!({"model": model, "stream": false}).to_string()).map(|_| ())
        .map_err(|e| format!("Ollama could not get {model}: {e}. Is Ollama running and signed in (ollama signin)?"))
}
fn is_cloud(name: &str, m: &Value) -> bool { m.get("remote_host").is_some() || name.ends_with("-cloud") || name.ends_with(":cloud") }

/// What the Add-account dialog lists: every model, ordered by need, with whether it fits this machine.
#[tauri::command(async)]
pub(crate) fn local_models(st: St) -> Value {
    let d = dir(&st.cfg.lock().unwrap());
    let (ram, vram, be) = hw();
    let free = store::disk_free(&d.ancestors().find(|p| p.exists()).unwrap_or(&d).to_string_lossy()).map_or(0, |f| f.0 / 1_000_000_000);
    let gb = |b: u64| b / 1_000_000_000;
    // Windows: every drive letter (A/B skipped: probing a floppy stalls); elsewhere the root and the models folder's disk
    let roots: Vec<String> = if cfg!(windows) { (b'C'..=b'Z').map(|c| format!("{}:\\", c as char)).filter(|p| Path::new(p).exists()).collect() }
        else { vec!["/".into(), d.ancestors().find(|p| p.exists()).unwrap_or(&d).to_string_lossy().into()] };
    let mut drives: Vec<Value> = vec![];
    for r in roots { if let Some((f, t)) = store::disk_free(&r) { if !drives.iter().any(|x| x["total"] == gb(t) && x["free"] == gb(f)) { drives.push(json!({"name": r, "free": gb(f), "total": gb(t)})); } } }
    let cpu = std::thread::available_parallelism().map_or(0, |n| n.get());
    json!({"ram": ram, "vram": vram, "backend": be, "dir": d, "free": free, "drives": drives, "cpu": cpu,
        "models": MODELS.iter().map(|m| json!({"id": m.0, "name": m.1, "gb": m.3, "need": m.4, "score": m.5,
            "fits": m.4 <= ram.max(vram), "have": model_file(&d, m.0).is_some_and(|f| f.exists())})).collect::<Vec<_>>()})
}

#[tauri::command]
pub(crate) fn local_dir_set(st: St, dir: String) {
    let mut cfg = st.cfg.lock().unwrap();
    cfg.local_dir = dir;
    store::save(&cfg);
}

/// Downloads the engine and the model if missing (progress as `local` events), then adds the account.
#[tauri::command(async)]
pub(crate) fn local_add(app: AppHandle, st: St, nick: String, model: String) -> R<Value> {
    let d = dir(&st.cfg.lock().unwrap());
    let file = model_file(&d, &model).ok_or("unknown model")?;
    let m = MODELS.iter().find(|m| m.0 == model).unwrap();
    let need = if file.exists() { 0.0 } else { m.3 } + if server_exe(&d).is_none() { 1.0 } else { 0.0 };
    std::fs::create_dir_all(d.join("models")).map_err(|e| format!("{}: {e}", d.display()))?;
    if let Some((free, _)) = store::disk_free(&d.to_string_lossy()) {
        if (free as f64 / 1e9) < need + 1.0 {
            return Err(format!("Needs about {need:.0} GB free in {}; {:.1} GB left. Pick another folder.", d.display(), free as f64 / 1e9));
        }
    }
    let say = |what: &str, pct: u32| emit(&app, "local", json!({"what": what, "pct": pct}));
    if server_exe(&d).is_none() {
        engine(&d, &say)?;
    }
    if !file.exists() {
        let part = file.with_extension("part");
        fetch(&format!("https://huggingface.co/{}/resolve/main/{}", m.2.rsplitn(2, '/').nth(1).unwrap(), m.2.rsplit('/').next().unwrap()), &part, &|p| say(m.1, p))?;
        std::fs::rename(&part, &file).map_err(|e| e.to_string())?;
    }
    let mut cfg = st.cfg.lock().unwrap();
    let a = store::add_account(&mut cfg, &nick, "local", "local", "local", &model, ""); // no key: llama-server takes any
    store::save(&cfg);
    Ok(json!(a))
}

/// Streams `url` to `to`, reporting whole percents.
fn fetch(url: &str, to: &Path, pct: &dyn Fn(u32)) -> R<()> {
    use std::io::{Read, Write};
    // a stalled connection fails after a minute instead of hanging forever
    let r = ureq::AgentBuilder::new().timeout_connect(std::time::Duration::from_secs(30)).timeout_read(std::time::Duration::from_secs(60)).build()
        .get(url).set("User-Agent", "clauracle").call().map_err(|e| format!("download failed: {e}"))?;
    let len: f64 = r.header("Content-Length").and_then(|l| l.parse().ok()).unwrap_or(0.0);
    let (mut body, mut out) = (r.into_reader(), std::io::BufWriter::new(std::fs::File::create(to).map_err(|e| e.to_string())?));
    let (mut buf, mut got, mut last) = (vec![0u8; 1 << 20], 0f64, u32::MAX);
    loop {
        let n = body.read(&mut buf).map_err(|e| format!("download broke off: {e}"))?;
        if n == 0 { break; }
        out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        got += n as f64;
        let p = if len > 0.0 { (got / len * 100.0) as u32 } else { 0 };
        if p != last { pct(p); last = p; }
    }
    out.flush().map_err(|e| e.to_string())
}

/// Picks this machine's build from the newest llama.cpp release (GPU build first, CPU last) and unpacks it in <dir>/engine.
fn engine(d: &Path, say: &dyn Fn(&str, u32)) -> R<()> {
    let rels: Value = ureq::get("https://api.github.com/repos/ggml-org/llama.cpp/releases?per_page=5").set("User-Agent", "clauracle")
        .call().map_err(|e| e.to_string())?.into_string().ok().and_then(|s| serde_json::from_str(&s).ok()).ok_or("couldn't read the llama.cpp release list")?;
    // "latest" can point at a non-build tag: take the newest release that carries binaries
    let assets: Vec<String> = rels.as_array().into_iter().flatten().map(|r| r["assets"].as_array().into_iter().flatten()
        .filter_map(|a| a["browser_download_url"].as_str().map(String::from)).collect::<Vec<_>>()).find(|a| a.len() > 5).ok_or("no llama.cpp release found")?;
    let picks = pick_assets(&assets, hw().2).ok_or("no llama.cpp build for this system")?;
    let tmp = d.join("engine.tmp");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    for u in &picks {
        let f = tmp.join(u.rsplit('/').next().unwrap());
        fetch(u, &f, &|p| say("llama.cpp engine", p))?;
        // bsdtar unpacks zip and tar.gz alike (Windows 10+, macOS); GNU tar on Linux gets tar.gz only, which is all Linux ships
        let o = store::command(Path::new("tar")).arg("-xf").arg(&f).arg("-C").arg(&tmp).output().map_err(|e| e.to_string())?;
        if !o.status.success() { return Err(format!("unpacking failed: {}", String::from_utf8_lossy(&o.stderr).trim())); }
        let _ = std::fs::remove_file(&f);
    }
    let _ = std::fs::remove_dir_all(d.join("engine"));
    std::fs::rename(&tmp, d.join("engine")).map_err(|e| e.to_string())?;
    server_exe(d).map(|_| ()).ok_or("llama-server not found in the download".into())
}

/// Download URLs for one backend: the first of the machine's preference list that the release has (+ its CUDA runtime).
fn pick_assets(assets: &[String], be: &str) -> Option<Vec<String>> {
    let os = if cfg!(windows) { "win" } else if cfg!(target_os = "macos") { "macos" } else { "ubuntu" };
    let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "x64" };
    let order: &[&str] = match be { "cuda" => &["cuda-12", "vulkan", "cpu", ""], "vulkan" => &["vulkan", "cpu", ""], _ => &["cpu", ""] };
    let name = |u: &String| u.rsplit('/').next().unwrap_or("").to_string();
    order.iter().find_map(|b| {
        let tag = if b.is_empty() { format!("-bin-{os}-{arch}.") } else { format!("-bin-{os}-{b}") };
        let main = assets.iter().find(|u| { let n = name(u); n.starts_with("llama-") && n.contains(&tag) && n.contains(&format!("-{arch}.")) })?;
        let mut v = vec![main.clone()];
        if b.starts_with("cuda") { // the CUDA runtime ships as a separate archive
            let cuda = name(main).split(&format!("-bin-{os}-")).nth(1)?.split('-').take(2).collect::<Vec<_>>().join("-"); // "cuda-12.4"
            v.extend(assets.iter().find(|u| { let n = name(u); n.starts_with("cudart-") && n.contains(&format!("{os}-{cuda}-{arch}")) }).cloned());
        }
        Some(v)
    })
}

fn server_exe(d: &Path) -> Option<PathBuf> {
    fn find(d: &Path, name: &str) -> Option<PathBuf> {
        std::fs::read_dir(d).ok()?.flatten().find_map(|e| { let p = e.path(); if p.is_dir() { find(&p, name) } else { (p.file_name()? == name).then_some(p) } })
    }
    find(&d.join("engine"), if cfg!(windows) { "llama-server.exe" } else { "llama-server" })
}

/// One model loaded at a time (they take most of the memory): (model id, url, child).
static SERVER: Mutex<Option<(String, String, std::process::Child)>> = Mutex::new(None);

/// Base URL of a running llama-server for `model`, starting it (and stopping another model's) when needed.
/// Blocks until the model is loaded, up to 5 minutes; tells the chat log how the load goes.
pub fn ensure_noting(app: &AppHandle, pid: &str, model: &str) -> R<String> {
    let note = |text: &str, done: bool| emit(app, "localLoad", json!({"pid": pid, "text": text, "done": done}));
    let r = ensure_in(&dir(&store::load()), model, &note);
    if let Err(e) = &r { note(&format!("Loading {} failed: {e}", model_name(model)), true); }
    r
}

/// Context window the running llama-server gave the model (--fit-ctx picks it from free memory, never below 32k).
pub fn ctx(url: &str) -> u64 {
    ureq::get(&format!("{url}/props")).timeout(std::time::Duration::from_secs(3)).call().ok().and_then(|r| r.into_string().ok())
        .and_then(|b| serde_json::from_str::<Value>(&b).ok()).and_then(|v| v["default_generation_settings"]["n_ctx"].as_u64()).unwrap_or(32768)
}

fn model_name(id: &str) -> &str { MODELS.iter().find(|m| m.0 == id).map_or(id, |m| m.1) }

/// Working set of a process in GB (0 when unknown). Std only: asks PowerShell on Windows, ps elsewhere.
fn mem_gb(pid: u32) -> f64 {
    let p = pid.to_string();
    let o = if cfg!(windows) { store::run("powershell", &["-NoProfile", "-Command", &format!("(Get-Process -Id {p}).WorkingSet64")], None, &[]).stdout.trim().parse::<f64>().unwrap_or(0.0) }
        else { store::run("ps", &["-o", "rss=", "-p", &p], None, &[]).stdout.trim().parse::<f64>().unwrap_or(0.0) * 1024.0 };
    o / 1e9
}

pub(crate) fn ensure_in(d: &Path, model: &str, note: &dyn Fn(&str, bool)) -> R<String> {
    let mut s = SERVER.lock().unwrap();
    if let Some((m, url, ch)) = s.as_mut() {
        if m == model && ch.try_wait().ok().flatten().is_none() { return Ok(url.clone()); }
        let _ = (ch.kill(), ch.wait()); // reaped, and its memory free before the next model loads
    }
    *s = None;
    let file = model_file(d, model).filter(|f| f.exists()).ok_or("The local model isn't downloaded: add the account again to download it.")?;
    // Claude Code puts system messages mid-conversation, which most model templates reject: the first start reads the
    // model's template and saves it with a fold in front (local_fix.jinja), later starts use that file directly.
    let tpl = file.with_extension("jinja");
    // saved with an older fix: rebuilt from the model's own template below
    if tpl.exists() && !std::fs::read_to_string(&tpl).is_ok_and(|t| t.starts_with(include_str!("local_fix.jinja"))) { let _ = std::fs::remove_file(&tpl); }
    if tpl.exists() { return start(d, &file, model, Some(&tpl), note).map(|(url, ch)| { *s = Some((model.into(), url.clone(), ch)); url }); }
    let (url, mut ch) = start(d, &file, model, None, note)?;
    let t = ureq::get(&format!("{url}/props")).call().ok().and_then(|r| r.into_string().ok())
        .and_then(|b| serde_json::from_str::<serde_json::Value>(&b).ok()).and_then(|v| v["chat_template"].as_str().map(String::from)).unwrap_or_default();
    if t.is_empty() { *s = Some((model.into(), url.clone(), ch)); return Ok(url); }
    let _ = (ch.kill(), ch.wait());
    std::fs::write(&tpl, format!("{}{t}", include_str!("local_fix.jinja"))).map_err(|e| e.to_string())?;
    let (url, ch) = start(d, &file, model, Some(&tpl), note)?;
    *s = Some((model.into(), url.clone(), ch));
    Ok(url)
}

/// Starts llama-server for `file` on a free port and waits until it answers /health.
fn start(d: &Path, file: &Path, model: &str, tpl: Option<&Path>, note: &dyn Fn(&str, bool)) -> R<(String, std::process::Child)> {
    let exe = server_exe(d).ok_or("The local engine is missing: add the account again to download it.")?;
    let port = std::net::TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).map_err(|e| e.to_string())?.port();
    let log = d.join("server.log");
    let mut c = store::command(&exe);
    c.arg("-m").arg(file).args(["--host", "127.0.0.1", "--port", &port.to_string(), "--alias", model, "--fit-ctx", "32768"]);
    if let Some(t) = tpl { c.arg("--chat-template-file").arg(t); }
    c.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(&log).map_err(|e| e.to_string())?);
    let mut ch = c.spawn().map_err(|e| format!("couldn't start llama-server: {e}"))?;
    track(ch.id());
    #[cfg(windows)]
    { use std::os::windows::io::AsRawHandle; adopt_raw(ch.as_raw_handle() as usize); }
    let url = format!("http://127.0.0.1:{port}");
    let (name, total) = (model_name(model), hw().0);
    let (mut used, mut last) = (0.0, 0.0);
    for i in 0..600 {
        if let Ok(Some(_)) = ch.try_wait() {
            let t = std::fs::read_to_string(&log).unwrap_or_default();
            return Err(format!("llama-server stopped: {}", t.lines().filter(|l| !l.trim().is_empty()).last().unwrap_or("no output")));
        }
        if i % 4 == 0 { // memory is read every 2 s: it costs a process start on Windows
            used = mem_gb(ch.id());
            if (used - last).abs() >= 0.1 || i == 0 { last = used; note(&format!("Loading {name}… {used:.1} / {total} GB"), false); }
        }
        if ureq::get(&format!("{url}/health")).timeout(std::time::Duration::from_secs(2)).call().is_ok() {
            note(&format!("Loaded {name} ({:.1} GB)", mem_gb(ch.id()).max(used)), true);
            return Ok((url, ch));
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    let _ = ch.kill();
    Err("The local model took over 5 minutes to load.".into())
}

#[test]
fn picks_a_build_per_backend() {
    let a: Vec<String> = ["llama-b1-bin-win-cpu-x64.zip", "llama-b1-bin-win-cuda-12.4-x64.zip", "llama-b1-bin-win-cuda-13.5-x64.zip", "cudart-llama-bin-win-cuda-12.4-x64.zip",
        "llama-b1-bin-win-vulkan-x64.zip", "llama-b1-bin-macos-arm64.tar.gz", "llama-b1-bin-ubuntu-x64.tar.gz", "llama-b1-bin-ubuntu-vulkan-x64.tar.gz"]
        .iter().map(|n| format!("https://x/{n}")).collect();
    let names = |be| pick_assets(&a, be).unwrap().iter().map(|u| u.rsplit('/').next().unwrap().to_string()).collect::<Vec<_>>().join(" ");
    if cfg!(windows) && cfg!(target_arch = "x86_64") {
        assert_eq!(names("cuda"), "llama-b1-bin-win-cuda-12.4-x64.zip cudart-llama-bin-win-cuda-12.4-x64.zip");
        assert_eq!(names("vulkan"), "llama-b1-bin-win-vulkan-x64.zip");
        assert_eq!(names("cpu"), "llama-b1-bin-win-cpu-x64.zip");
    }
    assert!(MODELS.windows(2).all(|w| w[0].4 <= w[1].4), "list stays ordered by need");
}

#[test]
fn ollama_cloud_names() {
    assert_eq!(cloud_name("glm-5.3"), "glm-5.3:cloud");
    assert_eq!(cloud_name("qwen3-coder:480b"), "qwen3-coder:480b-cloud");
}

/// Model the live tests run: $CLAURACLE_LOCAL_MODEL (an id from MODELS, e.g. one already downloaded), else the smallest.
#[cfg(test)]
pub(crate) fn test_model() -> (&'static str, &'static str) {
    let want = std::env::var("CLAURACLE_LOCAL_MODEL").unwrap_or_default();
    MODELS.iter().find(|m| m.0 == want).or(MODELS.first()).map(|m| (m.0, m.2)).unwrap()
}

/// Real run: engine + model (see test_model) into $CLAURACLE_LOCAL_TEST (GBs), and the chat-template fix (a turn on it: opencode_local_live).
#[test]
#[ignore]
fn local_live() {
    let d = PathBuf::from(std::env::var("CLAURACLE_LOCAL_TEST").expect("set CLAURACLE_LOCAL_TEST to a scratch folder"));
    std::fs::create_dir_all(d.join("models")).unwrap();
    if server_exe(&d).is_none() { engine(&d, &|w, p| if p % 25 == 0 { eprintln!("{w} {p}%") }).unwrap(); }
    let (id, m) = test_model();
    let f = model_file(&d, id).unwrap();
    if !f.exists() { fetch(&format!("https://huggingface.co/{}/resolve/main/{}", m.rsplitn(2, '/').nth(1).unwrap(), m.rsplit('/').next().unwrap()), &f, &|p| if p % 25 == 0 { eprintln!("model {p}%") }).unwrap(); }
    let url = ensure_in(&d, id, &|_, _| {}).unwrap();
    assert!(ctx(&url) >= 32768, "window read from the server");
    // the template fix: a system message after a plain-text user turn is folded into it, not a template error
    let r = ureq::post(&format!("{url}/v1/chat/completions")).send_string(&json!({"messages": [{"role": "system", "content": "Be brief."},
        {"role": "user", "content": "Reply with exactly: FOLD OK"}, {"role": "system", "content": "Use capitals."}], "max_tokens": 400}).to_string())
        .map_err(|e| match e { ureq::Error::Status(c, r) => format!("{c}: {}", r.into_string().unwrap_or_default()), e => e.to_string() }).unwrap().into_string().unwrap();
    eprintln!("fold: {}", &r[..r.len().min(300)]);
}
