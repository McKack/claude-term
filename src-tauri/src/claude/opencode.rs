//! OpenCode backend spike (#599): `opencode serve` (HTTP + SSE, basic auth) behind the `Backend` trait.
//! Not wired into turn.rs yet (see HANDOVER / issue #599 for the mapping). `translate` turns OpenCode events into
//! the stream-json shaped messages `turn::show` already consumes; `Server::events` is the blocking SSE reader.
use super::Backend;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

pub struct Server { pub base: String, auth: String, pub session: String, child: std::sync::Mutex<Child> }

fn b64(s: &str) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut o = String::new();
    for c in s.as_bytes().chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 { if i <= c.len() { o.push(T[(n >> (18 - 6 * i) & 63) as usize] as char) } else { o.push('=') } }
    }
    o
}

/// OpenCode config: `local` = llama-server URL becomes the custom provider `local` (OpenAI-compatible, model "local/<name>");
/// `mcp` = Clauracle's tools (`mcpserve`), which OpenCode names `clauracle_<tool>`.
pub fn config(local: &str, model: &str, mcp: &str) -> Value {
    let mut c = json!({});
    if !local.is_empty() {
        let m = model.strip_prefix("local/").unwrap_or(model);
        c["provider"] = json!({"local": {"npm": "@ai-sdk/openai-compatible", "name": "Local", "options": {"baseURL": format!("{local}/v1")}, "models": {m: {"name": m}}}});
    }
    if !mcp.is_empty() { c["mcp"] = json!({"clauracle": {"type": "remote", "url": mcp, "enabled": true}}); }
    c
}

/// OpenCode's name for one of Clauracle's MCP tools → the CLI's (`clauracle_ready` → `mcp__clauracle__ready`), which turn.rs knows.
pub fn tool_name(n: &str) -> String { n.strip_prefix("clauracle_").map_or(n.to_string(), |t| format!("mcp__clauracle__{t}")) }

impl Server {
    /// Start `opencode serve` in `dir` (port 0 = a free one; it prints the URL and a password), create a session.
    /// `ask`: tool actions that must come back as permission requests (e.g. "*").
    pub fn start(dir: &str, ask: &[&str]) -> Result<Server, String> { Self::start_with(dir, ask, "", "", "", &Value::Null) }
    /// `model` = "provider/model" ("" = OpenCode's default free one); `key` (with `label`) is stored as that provider's credential.
    pub fn start_with(dir: &str, ask: &[&str], model: &str, key: &str, label: &str, cfg: &Value) -> Result<Server, String> {
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
        let mut s = Server { base, auth: format!("Basic {}", b64(&format!("opencode:{pw}"))), session: String::new(), child: std::sync::Mutex::new(child) };
        let rules: Vec<Value> = ask.iter().map(|a| json!({"action": a, "resource": "*", "effect": "ask"})).collect();
        let r = s.call("POST", "/api/session", Some(json!({"permissions": rules})))?;
        s.session = r["data"]["id"].as_str().ok_or("no session id")?.to_string();
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
    pub fn reply(&self, req: &str, decision: &str) -> Result<(), String> {
        self.call("POST", &format!("/api/session/{}/permission/{req}/reply", self.session), Some(json!({"decision": decision}))).map(|_| ())
    }
    pub fn stop(&self) {
        let mut c = self.child.lock().unwrap();
        #[cfg(windows)] { let _ = Command::new("taskkill").args(["/F", "/T", "/PID", &c.id().to_string()]).output(); }
        let _ = c.kill();
    }
}

impl Backend for Server {
    async fn send(&self, content: Value) -> Result<(), String> {
        // ponytail: text only; pasted images go through `files` once this is wired in
        let text = match &content {
            Value::String(s) => s.clone(),
            v => v.as_array().into_iter().flatten().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"),
        };
        self.call("POST", &format!("/api/session/{}/prompt", self.session), Some(json!({"text": text}))).map(|_| ())
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
        assert_eq!(v["message"]["content"][0]["name"], "shell");
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
    #[test] fn maps_usage() {
        let o = translate(&json!({"type": "session.usage.updated", "data": {"sessionID": "s", "cost": 0, "tokens": {"input": 10, "output": 5, "reasoning": 1, "cache": {"read": 99}}}}), "s", &mut Default::default());
        assert_eq!(o, vec![Out::Msg(json!({"type": "oc_usage", "tokens": 16.0, "cost": 0}))]);
    }
    #[test] fn local_provider_config() {
        let c = config("http://127.0.0.1:8080", "local/qwen", "http://127.0.0.1:9/mcp");
        assert_eq!(c["mcp"]["clauracle"]["url"], "http://127.0.0.1:9/mcp");
        assert_eq!(tool_name("clauracle_ready"), "mcp__clauracle__ready");
        assert_eq!(c["provider"]["local"]["options"]["baseURL"], "http://127.0.0.1:8080/v1");
        assert!(c["provider"]["local"]["models"]["qwen"].is_object());
    }
    /// A picked model + key reach the server: `cargo test opencode_model_live -- --ignored`.
    #[test] #[ignore] fn opencode_model_live() {
        let dir = std::env::temp_dir().to_string_lossy().to_string();
        let s = Server::start_with(&dir, &["*"], "opencode/big-pickle", "k-test", "clauracle-test", &Value::Null).unwrap();
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
        let s = Server::start_with(&dir, &["*"], &format!("local/{id}"), "", "", &config(&url, &format!("local/{id}"), "")).unwrap();
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
