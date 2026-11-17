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

impl Server {
    /// Start `opencode serve` in `dir` (port 0 = a free one; it prints the URL and a password), create a session.
    /// `ask`: tool actions that must come back as permission requests (e.g. "*").
    pub fn start(dir: &str, ask: &[&str]) -> Result<Server, String> {
        let mut cmd = if cfg!(windows) { let mut c = Command::new("cmd"); c.args(["/c", "opencode"]); c } else { Command::new("opencode") };
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
        Ok(s)
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
            names.insert(d["id"].as_str().unwrap_or("").into(), d["name"].as_str().unwrap_or("tool").into());
            vec![]
        }
        "session.text.ended" => m(json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": d["text"]}]}})),
        "session.tool.called" => {
            let id = d["id"].as_str().unwrap_or("");
            let name = names.get(id).map(|s| s.as_str()).unwrap_or("tool");
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
        "session.execution.succeeded" => m(json!({"type": "result", "subtype": "success", "is_error": false, "session_id": sid})),
        "session.execution.failed" | "session.execution.interrupted" =>
            m(json!({"type": "result", "subtype": "error_during_execution", "is_error": true, "session_id": sid, "result": d["error"]})),
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
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
}
