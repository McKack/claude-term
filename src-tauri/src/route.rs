//! Agents on another account than their session: a session with such an agent talks to this local listener, which sends
//! each request whose model names a routed agent (`clauracle-<agent>`) to that agent's account, and passes everything
//! else unchanged to the session's own endpoint.
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Mutex, OnceLock};

/// Where one agent's calls go: base URL, the auth headers for it, and the real model name.
#[derive(Clone, Debug, PartialEq)]
pub struct Target { pub url: String, pub headers: Vec<(String, String)>, pub model: String }

impl Target {
    /// From an account's CLI environment (store::account_env): the same headers Claude Code would send with it.
    pub fn from_env(env: &[(String, String)], model: String) -> Target {
        let get = |k: &str| env.iter().find(|e| e.0 == k).map(|e| e.1.clone()).filter(|v| !v.is_empty());
        let mut headers = vec![];
        if let Some(t) = get("CLAUDE_CODE_OAUTH_TOKEN") {
            headers.push(("authorization".into(), format!("Bearer {t}")));
            headers.push(("anthropic-beta".into(), "oauth-2025-04-20".into()));
        }
        if let Some(k) = get("ANTHROPIC_API_KEY") { headers.push(("x-api-key".into(), k)); }
        if let Some(t) = get("ANTHROPIC_AUTH_TOKEN") { headers.push(("authorization".into(), format!("Bearer {t}"))); }
        Target { url: get("ANTHROPIC_BASE_URL").unwrap_or_else(|| ANTHROPIC.into()), headers, model }
    }
}

pub const ANTHROPIC: &str = "https://api.anthropic.com";

/// Random per run, the first path segment of every URL this listener and oai.rs hand out: another program on this
/// machine can't use them (and the keys they attach) without it. Std only: RandomState is seeded by the OS.
pub fn secret() -> &'static str {
    static S: OnceLock<String> = OnceLock::new();
    S.get_or_init(|| {
        use std::hash::{BuildHasher, Hasher};
        let half = || { let mut h = std::collections::hash_map::RandomState::new().build_hasher(); h.write_u64(0); h.finish() };
        format!("{:016x}{:016x}", half(), half())
    })
}

/// The rest of `path` after the secret, None when it doesn't open with it.
pub fn unlock(path: &str) -> Option<&str> {
    path.trim_start_matches('/').strip_prefix(secret())?.strip_prefix('/')
}
struct Route { up: String, agents: HashMap<String, Target> }
static PORT: OnceLock<u16> = OnceLock::new();
static ROUTES: Mutex<Option<HashMap<String, Route>>> = Mutex::new(None);
#[cfg(test)]
pub static HITS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0); // routed calls, for the live test

/// Base URL for a session (`key`) whose own endpoint is `up`; `agents` = routed model name → its target.
pub fn base(key: &str, up: &str, agents: HashMap<String, Target>) -> String {
    let port = *PORT.get_or_init(|| {
        let Ok(l) = TcpListener::bind("127.0.0.1:0") else { return 1 }; // unreachable port: the CLI reports the error
        let p = l.local_addr().map(|a| a.port()).unwrap_or(1);
        std::thread::spawn(move || for s in l.incoming().flatten() { std::thread::spawn(move || { let _ = serve(s); }); });
        p
    });
    let key: String = key.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    ROUTES.lock().unwrap().get_or_insert_with(HashMap::new).insert(key.clone(), Route { up: up.trim_end_matches('/').into(), agents });
    format!("http://127.0.0.1:{port}/{}/{key}", secret())
}

fn serve(mut s: TcpStream) -> std::io::Result<()> {
    let mut r = BufReader::new(s.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or("GET").to_string(), parts.next().unwrap_or("/").to_string());
    let (mut headers, mut len, mut chunked) = (Vec::<(String, String)>::new(), 0usize, false);
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 || h.trim().is_empty() { break; }
        let Some((k, v)) = h.split_once(':') else { continue };
        let k = k.trim().to_ascii_lowercase();
        if k == "content-length" { len = v.trim().parse().unwrap_or(0); }
        if k == "transfer-encoding" { chunked = v.to_ascii_lowercase().contains("chunked"); }
        // the listener sets these itself; compressed replies would arrive decoded but still marked
        if !matches!(k.as_str(), "host" | "content-length" | "connection" | "accept-encoding" | "transfer-encoding") { headers.push((k, v.trim().into())); }
    }
    let body = if chunked { read_chunked(&mut r)? } else { let mut b = vec![0; len]; r.read_exact(&mut b)?; b };
    let inner = unlock(&path).unwrap_or("");
    let (key, rest) = inner.split_once('/').unwrap_or((inner, ""));
    let (url, body) = {
        let g = ROUTES.lock().unwrap();
        let Some(route) = g.as_ref().and_then(|m| m.get(key)).filter(|_| !key.is_empty()) else {
            return write!(s, "HTTP/1.1 404 X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        };
        let mut req: Value = if rest.starts_with("v1/messages") { serde_json::from_slice(&body).unwrap_or_default() } else { Value::Null };
        match req["model"].as_str().and_then(|m| route.agents.get(m)) {
            Some(t) => {
                #[cfg(test)]
                HITS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                req["model"] = t.model.clone().into();
                headers.retain(|(k, _)| !matches!(k.as_str(), "authorization" | "x-api-key"));
                // the session's OAuth beta flag goes with its OAuth token: a key-based target would refuse it
                if !t.headers.iter().any(|(k, _)| k == "anthropic-beta") { strip_oauth_beta(&mut headers); }
                for (k, v) in &t.headers {
                    // keep the CLI's own beta flags next to the target's
                    let v = if k == "anthropic-beta" { merge_beta(&headers, v) } else { v.clone() };
                    headers.retain(|h| h.0 != *k);
                    headers.push((k.clone(), v));
                }
                (format!("{}/{rest}", t.url.trim_end_matches('/')), serde_json::to_vec(&req).unwrap_or(body))
            }
            None => (format!("{}/{rest}", route.up), body),
        }
    };
    let mut q = ureq::request(&method, &url);
    for (k, v) in &headers { q = q.set(k, v); }
    let res = match q.send_bytes(&body) {
        Ok(r) | Err(ureq::Error::Status(_, r)) => r,
        Err(e) => {
            let b = serde_json::json!({"type": "error", "error": {"type": "api_error", "message": e.to_string()}}).to_string();
            return write!(s, "HTTP/1.1 502 X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}", b.len());
        }
    };
    let mut head = format!("HTTP/1.1 {} {}\r\n", res.status(), res.status_text());
    for n in res.headers_names() {
        if matches!(n.as_str(), "content-length" | "transfer-encoding" | "connection" | "content-encoding") { continue; }
        for v in res.all(&n) { head += &format!("{n}: {v}\r\n"); }
    }
    s.write_all((head + "Connection: close\r\n\r\n").as_bytes())?;
    // the body ends when the connection closes, so a stream passes through as it arrives
    std::io::copy(&mut res.into_reader(), &mut s).map(|_| ())
}

/// A chunked request body (hex size line, data, CRLF … 0), trailers skipped.
fn read_chunked(r: &mut impl BufRead) -> std::io::Result<Vec<u8>> {
    let mut out = vec![];
    loop {
        let mut l = String::new();
        if r.read_line(&mut l)? == 0 { return Ok(out); }
        let n = usize::from_str_radix(l.trim().split(';').next().unwrap_or(""), 16)
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "bad chunk size"))?;
        if n == 0 { loop { let mut t = String::new(); if r.read_line(&mut t)? == 0 || t.trim().is_empty() { return Ok(out); } } }
        let at = out.len();
        out.resize(at + n, 0);
        r.read_exact(&mut out[at..])?;
        let mut crlf = String::new();
        r.read_line(&mut crlf)?;
    }
}

fn merge_beta(headers: &[(String, String)], add: &str) -> String {
    let mut v: Vec<&str> = headers.iter().filter(|h| h.0 == "anthropic-beta").flat_map(|h| h.1.split(',')).map(str::trim).collect();
    for a in add.split(',').map(str::trim) { if !v.contains(&a) { v.push(a); } }
    v.join(",")
}

fn strip_oauth_beta(headers: &mut Vec<(String, String)>) {
    for h in headers.iter_mut().filter(|h| h.0 == "anthropic-beta") {
        h.1 = h.1.split(',').map(str::trim).filter(|b| !b.starts_with("oauth-")).collect::<Vec<_>>().join(",");
    }
    headers.retain(|h| h.0 != "anthropic-beta" || !h.1.is_empty());
}

#[test]
fn locked_by_the_secret() {
    let s = secret();
    assert_eq!(s.len(), 32);
    assert_eq!(unlock(&format!("/{s}/p1/v1/messages")), Some("p1/v1/messages"));
    assert_eq!(unlock("/p1/v1/messages"), None);
    assert_eq!(unlock(&format!("/{}/p1", &s[..31])), None);
    let mut h = vec![("anthropic-beta".to_string(), "a, oauth-2025-04-20".to_string()), ("x".into(), "y".into())];
    strip_oauth_beta(&mut h);
    assert_eq!(h[0].1, "a");
    let mut h = vec![("anthropic-beta".to_string(), "oauth-2025-04-20".to_string())];
    strip_oauth_beta(&mut h);
    assert!(h.is_empty());
}

#[test]
fn beta_flags_merge() {
    let h = vec![("anthropic-beta".to_string(), "a, b".to_string())];
    assert_eq!(merge_beta(&h, "b,oauth"), "a,b,oauth");
    let t = Target::from_env(&[("CLAUDE_CODE_OAUTH_TOKEN".into(), "t".into())], "m".into());
    assert_eq!((t.url.as_str(), t.headers[0].1.as_str()), (ANTHROPIC, "Bearer t"));
}

#[test]
fn chunked_body() {
    let mut r = std::io::Cursor::new(b"4\r\nWiki\r\n5;x=1\r\npedia\r\n0\r\nT: v\r\n\r\n".to_vec());
    assert_eq!(read_chunked(&mut r).unwrap(), b"Wikipedia");
}

/// The listener against a fake upstream: a routed model goes to its target with the target's key and real model name,
/// the session's key and OAuth beta stay behind; other models pass to the session's endpoint untouched; locked paths 404.
#[test]
fn routes_end_to_end() {
    let up = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = up.local_addr().unwrap().port();
    let (tx, rx) = std::sync::mpsc::channel::<(String, Vec<(String, String)>, Value)>();
    std::thread::spawn(move || for c in up.incoming().flatten() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut c = c;
            let mut r = BufReader::new(c.try_clone().unwrap());
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            let (mut hs, mut len) = (vec![], 0);
            loop {
                let mut h = String::new();
                if r.read_line(&mut h).unwrap() == 0 || h.trim().is_empty() { break; }
                let (k, v) = h.split_once(':').unwrap();
                let (k, v) = (k.trim().to_ascii_lowercase(), v.trim().to_string());
                if k == "content-length" { len = v.parse().unwrap(); }
                hs.push((k, v));
            }
            let mut b = vec![0; len];
            r.read_exact(&mut b).unwrap();
            tx.send((line.split_whitespace().nth(1).unwrap().to_string(), hs, serde_json::from_slice(&b).unwrap_or_default())).unwrap();
            write!(c, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 4\r\nConnection: close\r\n\r\nPONG").unwrap();
        });
    });
    let t = Target { url: format!("http://127.0.0.1:{port}/tg"), headers: vec![("x-api-key".into(), "K2".into())], model: "real-model".into() };
    let b = base("e2e", &format!("http://127.0.0.1:{port}/up"), [("clauracle-helper".to_string(), t)].into());
    let send = |model: &str| ureq::post(&format!("{b}/v1/messages")).set("authorization", "Bearer SESSION").set("anthropic-beta", "x-1,oauth-2025-04-20")
        .send_string(&serde_json::json!({"model": model, "messages": []}).to_string()).unwrap().into_string().unwrap();
    let get = |hs: &[(String, String)], k: &str| hs.iter().find(|h| h.0 == k).map(|h| h.1.clone());

    assert_eq!(send("clauracle-helper"), "PONG");
    let (path, hs, body) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(path, "/tg/v1/messages");
    assert_eq!(body["model"], "real-model");
    assert_eq!((get(&hs, "x-api-key").as_deref(), get(&hs, "authorization")), (Some("K2"), None));
    assert_eq!(get(&hs, "anthropic-beta").as_deref(), Some("x-1"));

    assert_eq!(send("claude-sonnet-5-5"), "PONG");
    let (path, hs, body) = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    assert_eq!(path, "/up/v1/messages");
    assert_eq!(body["model"], "claude-sonnet-5-5");
    assert_eq!((get(&hs, "authorization").as_deref(), get(&hs, "x-api-key")), (Some("Bearer SESSION"), None));
    assert_eq!(get(&hs, "anthropic-beta").as_deref(), Some("x-1,oauth-2025-04-20"));

    let wrong = b.replacen(secret(), "0000", 1);
    assert!(matches!(ureq::post(&format!("{wrong}/v1/messages")).send_string("{}"), Err(ureq::Error::Status(404, _))));
    assert!(matches!(ureq::post(&format!("{}/nokey/v1/messages", b.rsplit_once('/').unwrap().0)).send_string("{}"), Err(ureq::Error::Status(404, _))));
}
