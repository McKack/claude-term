//! OpenCode backend spike (#599): `opencode serve` (HTTP + SSE, basic auth) behind the `Backend` trait.
//! Not wired into turn.rs yet (see HANDOVER / issue #599 for the mapping). `translate` turns OpenCode events into
//! the stream-json shaped messages `turn::show` already consumes; `Server::events` is the blocking SSE reader.
use super::Backend;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

pub struct Server { pub base: String, auth: String, pub session: String, pub resumed: bool, child: std::sync::Mutex<Child> }

fn b64(s: &str) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut o = String::new();
    for c in s.as_bytes().chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 { if i <= c.len() { o.push(T[(n >> (18 - 6 * i) & 63) as usize] as char) } else { o.push('=') } }
    }
    o
}

/// OpenCode config: `entry` = (id, provider JSON) of a custom provider (an account that isn't Claude, or the local llama-server),
/// `mcp` = Clauracle's tools (`mcpserve`), which OpenCode names `clauracle_<tool>`. Goes in OPENCODE_CONFIG_CONTENT (the key never hits a disk).
pub fn config(entry: Option<(&str, Value)>, mcp: &str) -> Value {
    let mut c = json!({});
    if let Some((id, p)) = entry { c["provider"] = json!({id: p}); }
    if !mcp.is_empty() { c["mcp"] = json!({"clauracle": {"type": "remote", "url": mcp, "enabled": true}}); }
    c
}

/// Model id + custom provider for an account (see `account_entry`); `local` = (url, window) of the running llama-server for a local account.
pub fn provider_for(a: &crate::store::Account, key: &str, local: Option<(&str, u64)>) -> Result<(String, Option<(&'static str, Value)>), String> {
    if a.provider == "local" {
        let (url, ctx) = local.ok_or("the local model isn't running")?;
        Ok((format!("local/{}", a.model), Some(("local", local_entry(url, &a.model, ctx)))))
    } else if a.provider != "opencode" {
        let (m, e) = account_entry(a, key)?;
        Ok((format!("clauracle/{m}"), Some(("clauracle", e))))
    } else { Ok((a.model.clone(), None)) }
}

/// The built-in llama-server as an OpenAI-compatible provider; `ctx` = the window it really has (OpenCode compacts by it).
pub fn local_entry(url: &str, model: &str, ctx: u64) -> Value {
    json!({"npm": "@ai-sdk/openai-compatible", "name": "Local", "options": {"baseURL": format!("{url}/v1")},
        "models": {model: {"name": model, "limit": {"context": ctx, "output": 8192}}}})
}

/// Model an account without its own pick uses (the endpoint names Claude's roles itself there; OpenCode needs one id).
fn default_model(provider: &str) -> &'static str {
    match provider { "deepseek" => "deepseek-chat", "zai" => "glm-5.3", "kimi" => "kimi-k3", "minimax" => "MiniMax-M3.1-Flash", "qwen" => "qwen3.7-max", _ => "" }
}

/// A non-Claude account as an OpenCode custom provider (#601): Anthropic-format endpoints through @ai-sdk/anthropic, OpenAI-format
/// ones (api "openai", OpenRouter) through @ai-sdk/openai-compatible. Returns (model id, provider JSON).
pub fn account_entry(a: &crate::store::Account, key: &str) -> Result<(String, Value), String> {
    let model = if a.model.is_empty() { default_model(&a.provider).to_string() } else { a.model.clone() };
    if model.is_empty() { return Err(format!("Pick a model for {} (Accounts > Model).", a.nick)) }
    let url = a.base_url.trim_end_matches('/');
    let openai = a.api == "openai" || a.provider == "openrouter";
    let key = if key.is_empty() { "none" } else { key };
    let entry = if openai {
        // OpenRouter's account URL is its Anthropic skin (.../api): the OpenAI one is .../api/v1
        let base = if a.provider == "openrouter" { format!("{url}/v1") } else { url.to_string() };
        json!({"npm": "@ai-sdk/openai-compatible", "name": a.nick, "options": {"baseURL": base, "apiKey": key}, "models": {&model: {"name": &model}}})
    } else {
        // gateways differ (x-api-key vs Bearer): send both; @ai-sdk/anthropic adds /messages to the base
        json!({"npm": "@ai-sdk/anthropic", "name": a.nick, "options": {"baseURL": format!("{url}/v1"), "apiKey": key, "headers": {"authorization": format!("Bearer {key}")}}, "models": {&model: {"name": &model}}})
    };
    Ok((model, entry))
}

/// OpenCode's name for one of Clauracle's MCP tools → the CLI's (`clauracle_ready` → `mcp__clauracle__ready`), which turn.rs knows.
pub fn tool_name(n: &str) -> String {
    match n {
        "shell" | "bash" => "Bash".into(), "edit" | "patch" => "Edit".into(), "write" => "Write".into(), "read" => "Read".into(),
        "grep" => "Grep".into(), "glob" => "Glob".into(), "webfetch" => "WebFetch".into(),
        _ => n.strip_prefix("clauracle_").map_or(n.to_string(), |t| format!("mcp__clauracle__{t}")),
    }
}

impl Server {
    /// Start `opencode serve` in `dir` (port 0 = a free one; it prints the URL and a password), create a session.
    /// `ask`: tool actions that must come back as permission requests (e.g. "*").
    #[cfg(test)]
    pub fn start(dir: &str, ask: &[&str]) -> Result<Server, String> { Self::start_with(dir, ask, "", "", "", &Value::Null, "") }
    /// `model` = "provider/model" ("" = OpenCode's default free one); `key` (with `label`) is stored as that provider's credential.
    pub fn start_with(dir: &str, ask: &[&str], model: &str, key: &str, label: &str, cfg: &Value, resume: &str) -> Result<Server, String> {
        let mut cmd = if cfg!(windows) { let mut c = Command::new("cmd"); c.args(["/c", "opencode"]); c } else { Command::new("opencode") };
        if !cfg.is_null() { cmd.env("OPENCODE_CONFIG_CONTENT", cfg.to_string()); }
        let mut child = cmd.args(["serve", "--hostname", "127.0.0.1", "--port", "0"]).current_dir(dir)
            .stdout(Stdio::piped()).stderr(Stdio::null()).stdin(Stdio::null()).spawn().map_err(|e| e.to_string())?;
        let out = child.stdout.take().ok_or("no stdout")?;
        let (mut base, mut pw) = (String::new(), String::new());
        for l in BufReader::new(out).lines().map_while(Result::ok).take(8) {
            if let Some(u) = l.strip_prefix("server listening on ") { base = u.trim().into() }
            if let Some(p) = l.strip_prefix("server password ") { pw = p.trim().into() }
            if !base.is_empty() && !pw.is_empty() { break }
        }
        if base.is_empty() { let _ = child.kill(); return Err("opencode serve did not report its address".into()) }
        let mut s = Server { base, auth: format!("Basic {}", b64(&format!("opencode:{pw}"))), session: String::new(), resumed: false, child: std::sync::Mutex::new(child) };
        // "!edit" = deny that action (a read-only helper), plain = ask
        let rules: Vec<Value> = ask.iter().map(|a| match a.strip_prefix('!') { Some(d) => json!({"action": d, "resource": "*", "effect": "deny"}), None => json!({"action": a, "resource": "*", "effect": "ask"}) }).collect();
        // a session of an earlier server (they persist on disk, with their permission rules) continues; unknown id → a new one
        if !resume.is_empty() && s.call("GET", &format!("/api/session/{resume}"), None).is_ok_and(|r| r["data"]["id"] == resume) {
            s.session = resume.to_string();
            s.resumed = true;
        } else {
            let r = s.call("POST", "/api/session", Some(json!({"permissions": rules})))?;
            s.session = r["data"]["id"].as_str().ok_or("no session id")?.to_string();
        }
        if let Some((prov, id)) = model.split_once('/') {
            if !key.is_empty() { s.set_key(prov, key, label)?; }
            s.call("POST", &format!("/api/session/{}/model", s.session), Some(json!({"model": {"id": id, "providerID": prov}})))?;
        }
        Ok(s)
    }
    /// The provider's API key as a credential, replacing the one this account made before.
    fn set_key(&self, prov: &str, key: &str, label: &str) -> Result<(), String> {
        let old = self.call("GET", "/api/credential", None)?;
        for c in old["data"].as_array().into_iter().flatten().filter(|c| c["label"] == label) {
            if let Some(id) = c["id"].as_str() { let _ = self.call("DELETE", &format!("/api/credential/{id}"), None); }
        }
        self.call("POST", "/api/credential", Some(json!({"integrationID": prov, "label": label, "value": {"type": "key", "key": key}, "activate": true}))).map(|_| ())
    }
    fn call(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
        let req = ureq::request(method, &format!("{}{path}", self.base)).set("authorization", &self.auth);
        let r = match body { Some(b) => req.set("content-type", "application/json").send_string(&b.to_string()), None => req.call() }.map_err(|e| e.to_string())?;
        Ok(serde_json::from_str(&r.into_string().unwrap_or_default()).unwrap_or(Value::Null))
    }
    /// Blocking SSE stream of `/api/event` (parsed `data:` JSON objects).
    pub fn events(&self) -> Result<impl Iterator<Item = Value>, String> {
        let r = ureq::get(&format!("{}/api/event", self.base)).set("authorization", &self.auth).call().map_err(|e| e.to_string())?;
        Ok(BufReader::new(r.into_reader()).lines().map_while(Result::ok)
            .filter_map(|l| l.strip_prefix("data: ").and_then(|d| serde_json::from_str::<Value>(d).ok())))
    }
    /// Answer a permission request: "once" | "always" | "reject".
    /// One task on a fresh session, blocking: the last assistant text once the turn ends. Asks are refused (the helper has nobody to ask).
    pub fn run_once(&self, task: &str, secs: u64) -> Result<String, String> {
        let evs = self.events()?;
        self.call("POST", &format!("/api/session/{}/prompt", self.session), Some(json!({"text": task})))?;
        let (t0, mut out, mut names) = (std::time::Instant::now(), String::new(), std::collections::HashMap::new());
        for ev in evs {
            for o in translate(&ev, &self.session, &mut names) {
                match o {
                    Out::Ask { id, .. } => { let _ = self.reply(&id, "reject"); }
                    Out::Msg(m) => {
                        if m["type"] == "assistant" { if let Some(t) = m["message"]["content"][0]["text"].as_str() { out = t.into() } }
                        if m["type"] == "result" { return if m["is_error"] == true { Err(format!("the helper failed: {}", m["result"])) } else { Ok(out) } }
                    }
                }
            }
            if t0.elapsed().as_secs() > secs { break }
        }
        Err("the helper took too long or stopped".into())
    }
    pub fn reply(&self, req: &str, decision: &str) -> Result<(), String> {
        self.call("POST", &format!("/api/session/{}/permission/{req}/reply", self.session), Some(json!({"decision": decision}))).map(|_| ())
    }
    pub fn stop(&self) {
        let mut c = self.child.lock().unwrap();
        #[cfg(windows)] { let _ = Command::new("taskkill").args(["/F", "/T", "/PID", &c.id().to_string()]).output(); }
        let _ = c.kill();
    }
}

/// Clauracle's content (a string, or Claude blocks: text and base64 images) → OpenCode's `text` + `files` (data URIs).
fn prompt_parts(content: &Value) -> (String, Vec<Value>) {
    let Some(blocks) = content.as_array() else { return (content.as_str().unwrap_or("").to_string(), vec![]) };
    let text = blocks.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("
");
    let files = blocks.iter().filter(|b| b["type"] == "image").enumerate().filter_map(|(i, b)| {
        let (mime, data) = (b["source"]["media_type"].as_str()?, b["source"]["data"].as_str()?);
        Some(json!({"uri": format!("data:{mime};base64,{data}"), "name": format!("image-{}.{}", i + 1, mime.rsplit('/').next().unwrap_or("png"))}))
    }).collect();
    (text, files)
}

impl Backend for Server {
    async fn send(&self, content: Value) -> Result<(), String> {
        let (text, files) = prompt_parts(&content);
        let mut body = json!({"text": text});
        if !files.is_empty() { body["files"] = json!(files); }
        self.call("POST", &format!("/api/session/{}/prompt", self.session), Some(body)).map(|_| ())
    }
    async fn interrupt(&self) -> Result<(), String> {
        self.call("POST", &format!("/api/session/{}/interrupt", self.session), None).map(|_| ())
    }
}

/// What `translate` produces: a stream-json message for `turn::show`, or a permission ask for `control::permission`.
#[derive(Debug, PartialEq)]
pub enum Out { Msg(Value), Ask { id: String, action: String, resources: Vec<String> } }

/// One OpenCode event → zero or more `Out`. `names` remembers tool call id → tool name (only `input.started` has it).
pub fn translate(ev: &Value, sid: &str, names: &mut std::collections::HashMap<String, String>) -> Vec<Out> {
    let d = &ev["data"];
    if d["sessionID"].as_str().is_some_and(|s| s != sid) { return vec![] }
    let m = |v: Value| vec![Out::Msg(v)];
    let t = ev["type"].as_str().unwrap_or("");
    match t {
        "session.execution.started" => m(json!({"type": "system", "subtype": "init", "session_id": sid})),
        "session.tool.input.started" => {
            names.insert(d["id"].as_str().unwrap_or("").into(), tool_name(d["name"].as_str().unwrap_or("tool")));
            vec![]
        }
        "session.text.ended" => m(json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": d["text"]}]}})),
        "session.tool.called" => {
            let id = d["id"].as_str().unwrap_or("");
            // OpenCode runs MCP tools inside its `execute` tool as `tools.clauracle.<tool>(…)`: name the card after the tool
            let wrapped = d["input"]["code"].as_str().and_then(|c| c.split_once("tools.clauracle.")).map(|(_, r)| r.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect::<String>());
            let name = &match wrapped { Some(t) if !t.is_empty() => format!("mcp__clauracle__{t}"), _ => names.get(id).cloned().unwrap_or("tool".into()) };
            m(json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "tool_use", "id": id, "name": name, "input": d["input"]}]}}))
        }
        "session.tool.success" | "session.tool.failed" | "session.tool.error" => {
            let txt: String = d["content"].as_array().into_iter().flatten().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n");
            m(json!({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": d["id"], "content": txt, "is_error": t != "session.tool.success"}]}}))
        }
        "permission.asked" => vec![Out::Ask {
            id: d["id"].as_str().unwrap_or("").into(),
            action: d["action"].as_str().unwrap_or("").into(),
            resources: d["resources"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect(),
        }],
        "session.usage.updated" => { let t = &d["tokens"]; let n: f64 = ["input", "output", "reasoning"].iter().filter_map(|k| t[*k].as_f64()).sum(); m(json!({"type": "oc_usage", "tokens": n, "cost": d["cost"]})) }
        "session.execution.succeeded" => m(json!({"type": "result", "subtype": "success", "is_error": false, "session_id": sid})),
        "session.execution.failed" | "session.execution.interrupted" =>
            m(json!({"type": "result", "subtype": "error_during_execution", "is_error": true, "session_id": sid, "result": d["error"]})),
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use crate::claude::backend::Backend;
    use super::*;
    #[test] fn b64_basic() { assert_eq!(b64("opencode:x"), "b3BlbmNvZGU6eA=="); assert_eq!(b64("ab"), "YWI="); }
    #[test] fn maps_tool_turn() {
        let mut n = Default::default();
        let e = |t: &str, d: Value| json!({"type": t, "data": d});
        assert!(translate(&e("session.tool.input.started", json!({"sessionID": "s", "id": "t1", "name": "shell"})), "s", &mut n).is_empty());
        let Out::Msg(v) = &translate(&e("session.tool.called", json!({"sessionID": "s", "id": "t1", "input": {"command": "ls"}})), "s", &mut n)[0] else { panic!() };
        assert_eq!(v["message"]["content"][0]["name"], "Bash");
        let o = translate(&e("permission.asked", json!({"sessionID": "s", "id": "p1", "action": "shell", "resources": ["ls"]})), "s", &mut n);
        assert_eq!(o, vec![Out::Ask { id: "p1".into(), action: "shell".into(), resources: vec!["ls".into()] }]);
        assert!(translate(&e("session.text.ended", json!({"sessionID": "other", "text": "x"})), "s", &mut n).is_empty());
    }
    /// Real turn: `cargo test opencode_live -- --ignored --nocapture` (needs `opencode` on PATH and its free model).
    #[test] #[ignore] fn opencode_live() {
        let dir = std::env::temp_dir().to_string_lossy().to_string();
        let s = Server::start(&dir, &["*"]).unwrap();
        let evs = s.events().unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(s.send(json!("Run the shell command `echo hello-spike` and tell me its output."))).unwrap();
        let (mut names, mut asked, mut text, mut result) = (Default::default(), false, String::new(), false);
        for ev in evs {
            for o in translate(&ev, &s.session, &mut names) {
                match o {
                    Out::Ask { id, .. } => { asked = true; s.reply(&id, "once").unwrap() }
                    Out::Msg(m) => {
                        if m["type"] == "assistant" { if let Some(t) = m["message"]["content"][0]["text"].as_str() { text = t.into() } }
                        if m["type"] == "result" { result = true }
                    }
                }
            }
            if result { break }
        }
        s.stop();
        println!("asked={asked} text={text}");
        assert!(asked && result && text.contains("hello-spike"));
    }
    #[test] fn unwraps_mcp_execute() {
        let mut n = std::collections::HashMap::new();
        translate(&json!({"type": "session.tool.input.started", "data": {"sessionID": "s", "id": "t", "name": "execute"}}), "s", &mut n);
        let o = translate(&json!({"type": "session.tool.called", "data": {"sessionID": "s", "id": "t", "input": {"code": "await tools.clauracle.ready();
return 1"}}}), "s", &mut n);
        assert!(matches!(&o[0], Out::Msg(m) if m["message"]["content"][0]["name"] == "mcp__clauracle__ready"));
    }
    #[test] fn images_become_files() {
        let (t, f) = prompt_parts(&json!([{"type": "text", "text": "look"}, {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "AAAA"}}]));
        assert_eq!(t, "look");
        assert_eq!(f, vec![json!({"uri": "data:image/png;base64,AAAA", "name": "image-1.png"})]);
        assert_eq!(prompt_parts(&json!("hi")), ("hi".to_string(), vec![]));
    }
    /// Pasted image + resume: `cargo test opencode_image_resume_live -- --ignored --nocapture` (needs `opencode`).
    #[test] #[ignore] fn opencode_image_resume_live() {
        let dir = std::env::temp_dir().to_string_lossy().to_string();
        let s = Server::start(&dir, &["*"]).unwrap();
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAIAAABLbSncAAAAEklEQVR4nGP4z8CAFWEXHbQSACj/P8Fu7N9hAAAAAElFTkSuQmCC";
        let evs = s.events().unwrap();
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
            .block_on(s.send(json!([{"type": "text", "text": "Reply with the word OK."}, {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": png}}]))).unwrap();
        let (mut names, mut res) = (Default::default(), Value::Null);
        let t0 = std::time::Instant::now();
        'o: for ev in evs { if t0.elapsed().as_secs() > 60 { break } for o in translate(&ev, &s.session, &mut names) { if let Out::Msg(m) = o { if m["type"] == "result" { res = m; break 'o } } } }
        let sid = s.session.clone();
        let got = s.call("GET", &format!("/api/session/{sid}"), None);
        println!("result={res} get={got:?}");
        s.stop();
        // a new server finds the old session
        let s2 = Server::start(&dir, &["*"]).unwrap();
        let again = s2.call("GET", &format!("/api/session/{sid}"), None);
        println!("resumed get={again:?}");
        s2.stop();
        assert_eq!(res["is_error"], false);
        assert!(again.is_ok());
    }
    /// OpenCode reaches a custom account's endpoint the way `account_entry` says (path + auth), for both formats; a fake endpoint
    /// records the first request. `cargo test account_wire_live -- --ignored --nocapture` (needs `opencode`).
    #[test] #[ignore] fn account_wire_live() {
        use std::io::{Read, Write};
        for (provider, api, want_path) in [("deepseek", "", "/anthropic/v1/messages"), ("ollama", "openai", "/v1/chat/completions")] {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = l.local_addr().unwrap().port();
            let (tx, rx) = std::sync::mpsc::channel::<String>();
            std::thread::spawn(move || for c in l.incoming() {
                let Ok(mut c) = c else { continue };
                let mut buf = [0u8; 8192];
                let n = c.read(&mut buf).unwrap_or(0);
                let _ = tx.send(String::from_utf8_lossy(&buf[..n]).to_string());
                let body = "{\"error\":{\"message\":\"fake\"}}";
                let _ = write!(c, "HTTP/1.1 401 Unauthorized\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
            });
            let base = if api == "openai" { format!("http://127.0.0.1:{port}/v1") } else { format!("http://127.0.0.1:{port}/anthropic") };
            let a = crate::store::Account { provider: provider.into(), base_url: base, api: api.into(), model: "m1".into(), nick: "T".into(), ..Default::default() };
            let (m, e) = account_entry(&a, "sekrit").unwrap();
            let dir = std::env::temp_dir().to_string_lossy().to_string();
            let s = Server::start_with(&dir, &["*"], &format!("clauracle/{m}"), "", "", &config(Some(("clauracle", e)), ""), "").unwrap();
            tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(s.send(json!("hi"))).unwrap();
            let req = rx.recv_timeout(std::time::Duration::from_secs(30)).unwrap_or_default();
            s.stop();
            println!("{provider}: {}", req.lines().take(12).collect::<Vec<_>>().join(" | "));
            let low = req.to_lowercase();
            assert!(req.starts_with(&format!("POST {want_path}")), "{provider} path: {req}");
            assert!(low.contains("sekrit"), "{provider} sent the key");
        }
    }
    #[test] fn maps_usage() {
        let o = translate(&json!({"type": "session.usage.updated", "data": {"sessionID": "s", "cost": 0, "tokens": {"input": 10, "output": 5, "reasoning": 1, "cache": {"read": 99}}}}), "s", &mut Default::default());
        assert_eq!(o, vec![Out::Msg(json!({"type": "oc_usage", "tokens": 16.0, "cost": 0}))]);
    }
    #[test] fn provider_config() {
        let c = config(Some(("local", local_entry("http://127.0.0.1:8080", "qwen", 32768))), "http://127.0.0.1:9/mcp");
        assert_eq!(c["provider"]["local"]["options"]["baseURL"], "http://127.0.0.1:8080/v1");
        assert_eq!(c["provider"]["local"]["models"]["qwen"]["limit"]["context"], 32768);
        assert_eq!(c["mcp"]["clauracle"]["url"], "http://127.0.0.1:9/mcp");
        assert_eq!(tool_name("clauracle_ready"), "mcp__clauracle__ready");
        assert_eq!((tool_name("shell"), tool_name("edit")), ("Bash".to_string(), "Edit".to_string()));
    }
    #[test] fn account_entries() {
        let a = |p: &str, u: &str, api: &str, m: &str| crate::store::Account { provider: p.into(), base_url: u.into(), api: api.into(), model: m.into(), nick: "N".into(), ..Default::default() };
        let (m, e) = account_entry(&a("deepseek", "https://api.deepseek.com/anthropic", "", ""), "k").unwrap();
        assert_eq!((m.as_str(), e["npm"].as_str(), e["options"]["baseURL"].as_str()), ("deepseek-chat", Some("@ai-sdk/anthropic"), Some("https://api.deepseek.com/anthropic/v1")));
        assert_eq!(e["options"]["headers"]["authorization"], "Bearer k");
        let (_, e) = account_entry(&a("openrouter", "https://openrouter.ai/api", "", "x/y:free"), "k").unwrap();
        assert_eq!((e["npm"].as_str(), e["options"]["baseURL"].as_str()), (Some("@ai-sdk/openai-compatible"), Some("https://openrouter.ai/api/v1")));
        let (_, e) = account_entry(&a("ollama", "http://localhost:11434/v1", "openai", "llama3"), "").unwrap();
        assert_eq!((e["options"]["baseURL"].as_str(), e["options"]["apiKey"].as_str()), (Some("http://localhost:11434/v1"), Some("none")));
        assert!(account_entry(&a("openrouter", "https://openrouter.ai/api", "", ""), "k").is_err(), "no model, no default");
    }
    /// A picked model + key reach the server: `cargo test opencode_model_live -- --ignored`.
    #[test] #[ignore] fn opencode_model_live() {
        let dir = std::env::temp_dir().to_string_lossy().to_string();
        let s = Server::start_with(&dir, &["*"], "opencode/big-pickle", "k-test", "clauracle-test", &Value::Null, "").unwrap();
        let cred = s.call("GET", "/api/credential", None).unwrap();
        s.stop();
        assert!(cred["data"].as_array().unwrap().iter().any(|c| c["label"] == "clauracle-test" || c["integrationID"] == "claude-agent"));
    }

    /// Real run (#606): one OpenCode turn on the local model (same env as route_live, needs `opencode` on PATH).
    #[test] #[ignore]
    fn opencode_local_live() {
        let d = std::path::PathBuf::from(std::env::var("CLAURACLE_LOCAL_TEST").expect("set CLAURACLE_LOCAL_TEST"));
        let (id, _) = crate::local::test_model();
        let url = crate::local::ensure_in(&d, id, &|_, _| {}).unwrap();
        let dir = std::env::temp_dir().to_string_lossy().to_string();
        let s = Server::start_with(&dir, &["*"], &format!("local/{id}"), "", "", &config(Some(("local", local_entry(&url, id, 32768))), ""), "").unwrap();
        let evs = s.events().unwrap();
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
            .block_on(s.send(json!("Reply with exactly: LOCAL OK"))).unwrap();
        let (mut names, mut text) = (Default::default(), String::new());
        'o: for ev in evs { for o in translate(&ev, &s.session, &mut names) {
            if let Out::Msg(m) = o {
                if m["type"] == "assistant" { if let Some(t) = m["message"]["content"][0]["text"].as_str() { text = t.into() } }
                if m["type"] == "result" { break 'o }
            } } }
        s.stop();
        println!("text={text}");
        assert!(text.to_uppercase().contains("LOCAL OK"));
    }
}
