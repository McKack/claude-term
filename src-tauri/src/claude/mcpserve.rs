//! Clauracle's MCP tools (`control::mcp`) over HTTP for backends that can't take the CLI's in-process server (OpenCode,
//! #608): one local listener per session, JSON-RPC in a POST body, JSON back (MCP streamable HTTP without streaming).
use super::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

pub struct McpHttp { pub url: String, stop: Arc<AtomicBool>, port: u16 }

impl Drop for McpHttp {
    fn drop(&mut self) {
        self.stop.store(true, SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port)); // wakes the blocking accept
    }
}

pub fn start(app: &AppHandle, st: &Arc<State>, pid: &str, live: &Arc<Live>) -> Result<McpHttp, String> {
    let l = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = l.local_addr().map_err(|e| e.to_string())?.port();
    let stop = Arc::new(AtomicBool::new(false));
    let (app, st, pid, live, s2) = (app.clone(), st.clone(), pid.to_string(), live.clone(), stop.clone());
    std::thread::spawn(move || {
        for c in l.incoming() {
            if s2.load(SeqCst) { break }
            let Ok(c) = c else { continue };
            let (app, st, pid, live) = (app.clone(), st.clone(), pid.clone(), live.clone());
            std::thread::spawn(move || { let _ = serve(c, &app, &st, &pid, &live); });
        }
    });
    Ok(McpHttp { url: format!("http://127.0.0.1:{port}/mcp"), stop, port })
}

/// (headers, body) of one HTTP request; None when the peer sent nothing.
pub(crate) fn read_request(c: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let mut buf = vec![];
    let mut tmp = [0u8; 4096];
    let end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") { break i + 4 }
        let n = c.read(&mut tmp).ok().filter(|n| *n > 0)?;
        buf.extend_from_slice(&tmp[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..end]).to_string();
    let len = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").and_then(|v| v.trim().parse::<usize>().ok())).unwrap_or(0);
    let mut body = buf[end..].to_vec();
    while body.len() < len {
        let n = c.read(&mut tmp).ok().filter(|n| *n > 0)?;
        body.extend_from_slice(&tmp[..n]);
    }
    Some((head, body))
}

fn serve(mut c: TcpStream, app: &AppHandle, st: &Arc<State>, pid: &str, live: &Arc<Live>) -> Option<()> {
    let (head, body) = read_request(&mut c)?;
    let reply = |c: &mut TcpStream, status: &str, ct: &str, b: &str| {
        let _ = write!(c, "HTTP/1.1 {status}\r\ncontent-type: {ct}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{b}", b.len());
    };
    if !head.starts_with("POST ") { reply(&mut c, "405 Method Not Allowed", "text/plain", ""); return None }
    let Ok(m) = serde_json::from_slice::<Value>(&body) else { reply(&mut c, "400 Bad Request", "text/plain", ""); return None };
    if m.get("id").is_none() { reply(&mut c, "202 Accepted", "text/plain", ""); return None } // a notification
    let r = tauri::async_runtime::block_on(control::mcp_any(app, st, pid, live, &m));
    reply(&mut c, "200 OK", "application/json", &r.to_string());
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Real OpenCode against a stand-in MCP endpoint speaking the same framing: it lists the tools and calls `clauracle_ready`.
    /// `cargo test mcp_opencode_live -- --ignored --nocapture` (needs `opencode` and its free model).
    #[test] #[ignore] fn mcp_opencode_live() {
        use crate::claude::backend::Backend;
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}/mcp", l.local_addr().unwrap().port());
        let called = Arc::new(AtomicBool::new(false));
        let c2 = called.clone();
        std::thread::spawn(move || for c in l.incoming() {
            let Ok(mut c) = c else { continue };
            let Some((_, b)) = read_request(&mut c) else { continue };
            let m: Value = serde_json::from_slice(&b).unwrap_or(Value::Null);
            let id = m["id"].clone();
            let r = match m["method"].as_str() {
                Some("initialize") => json!({"protocolVersion": m["params"]["protocolVersion"], "capabilities": {"tools": {}}, "serverInfo": {"name": "clauracle", "version": "1"}}),
                Some("tools/list") => json!({"tools": [{"name": "ready", "description": "Call when the task is done.", "inputSchema": {"type": "object", "properties": {}}}]}),
                Some("tools/call") => { c2.store(true, SeqCst); json!({"content": [{"type": "text", "text": "Armed."}]}) }
                _ => Value::Null,
            };
            let body = if id.is_null() { String::new() } else { json!({"jsonrpc": "2.0", "id": id, "result": r}).to_string() };
            let st = if id.is_null() { "202 Accepted" } else { "200 OK" };
            let _ = write!(c, "HTTP/1.1 {st}
content-type: application/json
content-length: {}
connection: close

{body}", body.len());
        });
        let dir = std::env::temp_dir().to_string_lossy().to_string();
        let s = crate::claude::opencode::Server::start_with(&dir, &["*"], "", "", "", &crate::claude::opencode::config("", "", &url)).unwrap();
        let evs = s.events().unwrap();
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
            .block_on(s.send(json!("Call the clauracle_ready tool now, then say done."))).unwrap();
        let mut names = Default::default();
        'o: for ev in evs { for o in crate::claude::opencode::translate(&ev, &s.session, &mut names) {
            match o {
                crate::claude::opencode::Out::Ask { id, .. } => { s.reply(&id, "once").unwrap() }
                crate::claude::opencode::Out::Msg(m) => { if m["type"] == "result" { break 'o } }
            } } }
        s.stop();
        assert!(called.load(SeqCst), "OpenCode called the MCP tool");
    }
    /// The framing alone: a POST body is read whole, however it is split across packets.
    #[test] fn reads_split_body() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let t = std::thread::spawn(move || { let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
            c.write_all(b"POST /mcp HTTP/1.1\r\nContent-Length: 9\r\n\r\n{\"id\"").unwrap(); std::thread::sleep(std::time::Duration::from_millis(50)); c.write_all(b":1,}").unwrap(); });
        let (mut s, _) = l.accept().unwrap();
        let (h, b) = read_request(&mut s).unwrap();
        t.join().unwrap();
        assert!(h.starts_with("POST ") && b == b"{\"id\":1,}");
    }
}
