//! Claude Code only speaks Anthropic's Messages API. Accounts whose endpoint takes OpenAI chat completions instead
//! (OpenAI, xAI, Gemini, Ollama, LM Studio, vLLM…) point Claude Code at this local listener, which rewrites each
//! request for the endpoint and streams the reply back in Anthropic's event format. Paths open with route::secret().
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::OnceLock;

static PORT: OnceLock<u16> = OnceLock::new();

/// Base URL for Claude Code that forwards to the OpenAI-format `upstream`; the upstream rides in the path (hex),
/// so one listener serves every account and the key stays in the request header.
pub fn base(upstream: &str) -> String {
    let port = *PORT.get_or_init(|| {
        let Ok(l) = TcpListener::bind("127.0.0.1:0") else { return 1 }; // unreachable port: the CLI reports the error
        let p = l.local_addr().map(|a| a.port()).unwrap_or(1);
        std::thread::spawn(move || for s in l.incoming().flatten() { std::thread::spawn(move || { let _ = serve(s); }); });
        p
    });
    let hex: String = upstream.trim().trim_end_matches('/').bytes().map(|b| format!("{b:02x}")).collect();
    format!("http://127.0.0.1:{port}/{}/{hex}", crate::route::secret())
}

fn unhex(h: &str) -> Option<String> {
    let b: Option<Vec<u8>> = (0..h.len()).step_by(2).map(|i| h.get(i..i + 2).and_then(|x| u8::from_str_radix(x, 16).ok())).collect();
    b.and_then(|b| String::from_utf8(b).ok()).filter(|u| u.starts_with("http"))
}

fn reply(s: &mut TcpStream, code: u16, body: &str) -> std::io::Result<()> {
    write!(s, "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
}

fn err(code: u16, msg: &str) -> String {
    let t = match code { 401 | 403 => "authentication_error", 429 => "rate_limit_error", 400 | 404 | 422 => "invalid_request_error", _ => "api_error" };
    json!({"type": "error", "error": {"type": t, "message": msg}}).to_string()
}

fn serve(mut s: TcpStream) -> std::io::Result<()> {
    let mut r = BufReader::new(s.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let path = line.split_whitespace().nth(1).unwrap_or("").split('?').next().unwrap_or("").to_string();
    let (mut len, mut key) = (0usize, String::new());
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 || h.trim().is_empty() { break; }
        let Some((k, v)) = h.split_once(':') else { continue };
        let v = v.trim();
        match k.trim().to_ascii_lowercase().as_str() {
            "content-length" => len = v.parse().unwrap_or(0),
            "authorization" if !v.is_empty() => key = v.strip_prefix("Bearer ").unwrap_or(v).into(),
            "x-api-key" if key.is_empty() => key = v.into(),
            _ => {}
        }
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body)?;
    let Some(inner) = crate::route::unlock(&path) else { return reply(&mut s, 404, &err(404, "not found")) };
    let (hex, rest) = inner.split_once('/').unwrap_or(("", ""));
    // shortcut: rough token estimate (4 bytes per token); good enough for Claude Code's context gauge
    if rest == "v1/messages/count_tokens" { return reply(&mut s, 200, &json!({"input_tokens": len / 4}).to_string()); }
    let (Some(up), "v1/messages") = (unhex(hex), rest) else { return reply(&mut s, 404, &err(404, "not found")) };
    let req: Value = serde_json::from_slice(&body).unwrap_or_default();
    let res = ureq::post(&format!("{up}/chat/completions")).set("Authorization", &format!("Bearer {key}"))
        .set("Content-Type", "application/json").send_string(&to_openai(&req).to_string());
    let res = match res {
        Ok(r) => r,
        Err(ureq::Error::Status(c, r)) => return reply(&mut s, c, &err(c, &r.into_string().unwrap_or_default())),
        Err(e) => return reply(&mut s, 502, &err(502, &e.to_string())),
    };
    let model = req["model"].clone();
    if req["stream"] != true {
        let v: Value = serde_json::from_str(&res.into_string()?).unwrap_or_default();
        return reply(&mut s, 200, &from_openai(&v, &model).to_string());
    }
    s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n")?;
    stream(&mut s, BufReader::new(res.into_reader()), &model)
}

fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// Anthropic request → OpenAI chat completions request.
fn to_openai(r: &Value) -> Value {
    let mut msgs = vec![];
    let sys = text_of(&r["system"]);
    if !sys.is_empty() { msgs.push(json!({"role": "system", "content": sys})); }
    for m in r["messages"].as_array().into_iter().flatten() {
        let blocks = match &m["content"] { Value::String(s) => vec![json!({"type": "text", "text": s})], Value::Array(a) => a.clone(), _ => vec![] };
        let of = |t: &'static str| blocks.iter().filter(move |b| b["type"] == t);
        if m["role"] == "assistant" {
            let text = of("text").filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n");
            let calls: Vec<Value> = of("tool_use").map(|b| json!({"id": b["id"], "type": "function",
                "function": {"name": b["name"], "arguments": b["input"].to_string()}})).collect();
            let mut a = json!({"role": "assistant", "content": if text.is_empty() && !calls.is_empty() { Value::Null } else { text.into() }});
            if !calls.is_empty() { a["tool_calls"] = calls.into(); }
            msgs.push(a);
            continue;
        }
        // tool results must follow the assistant's calls directly, so they go before the user's own text
        // shortcut: images inside a tool result are not passed on (OpenAI tool messages take text only); the model is told
        for b in of("tool_result") {
            let mut t = text_of(&b["content"]);
            let imgs = b["content"].as_array().map_or(0, |a| a.iter().filter(|x| x["type"] == "image").count());
            if imgs > 0 { t += &format!("\n[{imgs} image(s) left out: this model endpoint takes text only in tool results]"); }
            msgs.push(json!({"role": "tool", "tool_call_id": b["tool_use_id"], "content": if b["is_error"] == true { format!("Error: {t}") } else { t }}));
        }
        let parts: Vec<Value> = blocks.iter().filter_map(|b| match b["type"].as_str() {
            Some("text") => Some(json!({"type": "text", "text": b["text"]})),
            Some("image") if b["source"]["type"] == "base64" => Some(json!({"type": "image_url",
                "image_url": {"url": format!("data:{};base64,{}", b["source"]["media_type"].as_str().unwrap_or("image/png"), b["source"]["data"].as_str().unwrap_or(""))}})),
            _ => None,
        }).collect();
        if !parts.is_empty() { msgs.push(json!({"role": "user", "content": parts})); }
    }
    // shortcut: max_tokens, temperature and thinking are left to the endpoint's defaults (newer OpenAI models reject max_tokens and custom temperatures)
    let mut o = json!({"model": r["model"], "messages": msgs, "stream": r["stream"] == true});
    let tools: Vec<Value> = r["tools"].as_array().into_iter().flatten().filter(|t| t.get("input_schema").is_some())
        .map(|t| json!({"type": "function", "function": {"name": t["name"], "description": t["description"].as_str().unwrap_or(""), "parameters": t["input_schema"]}})).collect();
    if !tools.is_empty() {
        o["tools"] = tools.into();
        o["tool_choice"] = match r["tool_choice"]["type"].as_str() {
            Some("any") => "required".into(),
            Some("none") => "none".into(),
            Some("tool") => json!({"type": "function", "function": {"name": r["tool_choice"]["name"]}}),
            _ => "auto".into(),
        };
    }
    if r["stream"] == true { o["stream_options"] = json!({"include_usage": true}); }
    if r["stop_sequences"].as_array().is_some_and(|a| !a.is_empty()) { o["stop"] = r["stop_sequences"].clone(); }
    o
}

fn stop_reason(f: &str, tools: bool) -> &'static str {
    match f { "length" => "max_tokens", "tool_calls" | "function_call" => "tool_use", _ if tools => "tool_use", _ => "end_turn" }
}

/// OpenAI non-streamed reply → Anthropic message.
fn from_openai(v: &Value, model: &Value) -> Value {
    let m = &v["choices"][0]["message"];
    let mut content = vec![];
    if let Some(t) = m["content"].as_str().filter(|t| !t.is_empty()) { content.push(json!({"type": "text", "text": t})); }
    for c in m["tool_calls"].as_array().into_iter().flatten() {
        let input: Value = serde_json::from_str(c["function"]["arguments"].as_str().unwrap_or("{}")).unwrap_or(json!({}));
        content.push(json!({"type": "tool_use", "id": c["id"], "name": c["function"]["name"], "input": input}));
    }
    let tools = content.iter().any(|b| b["type"] == "tool_use");
    json!({"id": v["id"].as_str().map(|i| format!("msg_{i}")).unwrap_or("msg_oai".into()), "type": "message", "role": "assistant", "model": model, "content": content,
        "stop_reason": stop_reason(v["choices"][0]["finish_reason"].as_str().unwrap_or(""), tools), "stop_sequence": null,
        "usage": {"input_tokens": v["usage"]["prompt_tokens"].as_u64().unwrap_or(0), "output_tokens": v["usage"]["completion_tokens"].as_u64().unwrap_or(0)}})
}

fn ev(w: &mut impl Write, t: &str, d: Value) -> std::io::Result<()> {
    write!(w, "event: {t}\ndata: {d}\n\n")?;
    w.flush()
}

/// OpenAI chunk stream → Anthropic server-sent events, written to `w` as they arrive.
fn stream(w: &mut impl Write, r: impl BufRead, model: &Value) -> std::io::Result<()> {
    ev(w, "message_start", json!({"type": "message_start", "message": {"id": "msg_oai", "type": "message", "role": "assistant", "model": model,
        "content": [], "stop_reason": null, "stop_sequence": null, "usage": {"input_tokens": 0, "output_tokens": 0}}}))?;
    // open block: its Anthropic index and, for a tool call, the OpenAI call index (text = -1)
    let (mut idx, mut open, mut tools, mut fin, mut usage) = (-1i64, None::<i64>, false, String::new(), (0u64, 0u64));
    for line in r.lines() {
        let line = match line { Ok(l) => l, Err(e) => { ev(w, "error", json!({"type": "error", "error": {"type": "api_error", "message": e.to_string()}}))?; return Ok(()) } };
        let Some(d) = line.strip_prefix("data:").map(str::trim) else { continue };
        if d == "[DONE]" { break; }
        let Ok(c) = serde_json::from_str::<Value>(d) else { continue };
        if c["usage"].is_object() { usage = (c["usage"]["prompt_tokens"].as_u64().unwrap_or(0), c["usage"]["completion_tokens"].as_u64().unwrap_or(0)); }
        if c["error"].is_object() { ev(w, "error", json!({"type": "error", "error": {"type": "api_error", "message": c["error"]["message"]}}))?; return Ok(()); }
        let ch = &c["choices"][0];
        if let Some(t) = ch["delta"]["content"].as_str().filter(|t| !t.is_empty()) {
            if open != Some(-1) {
                if open.is_some() { ev(w, "content_block_stop", json!({"type": "content_block_stop", "index": idx}))?; }
                idx += 1; open = Some(-1);
                ev(w, "content_block_start", json!({"type": "content_block_start", "index": idx, "content_block": {"type": "text", "text": ""}}))?;
            }
            ev(w, "content_block_delta", json!({"type": "content_block_delta", "index": idx, "delta": {"type": "text_delta", "text": t}}))?;
        }
        for tc in ch["delta"]["tool_calls"].as_array().into_iter().flatten() {
            let i = tc["index"].as_i64().unwrap_or(0);
            if open != Some(i) {
                if open.is_some() { ev(w, "content_block_stop", json!({"type": "content_block_stop", "index": idx}))?; }
                idx += 1; open = Some(i); tools = true;
                let id = tc["id"].as_str().filter(|s| !s.is_empty()).map(String::from).unwrap_or(format!("toolu_{idx}"));
                ev(w, "content_block_start", json!({"type": "content_block_start", "index": idx, "content_block": {"type": "tool_use", "id": id, "name": tc["function"]["name"], "input": {}}}))?;
            }
            if let Some(a) = tc["function"]["arguments"].as_str().filter(|a| !a.is_empty()) {
                ev(w, "content_block_delta", json!({"type": "content_block_delta", "index": idx, "delta": {"type": "input_json_delta", "partial_json": a}}))?;
            }
        }
        if let Some(f) = ch["finish_reason"].as_str() { fin = f.into(); }
    }
    if open.is_some() { ev(w, "content_block_stop", json!({"type": "content_block_stop", "index": idx}))?; }
    ev(w, "message_delta", json!({"type": "message_delta", "delta": {"stop_reason": stop_reason(&fin, tools), "stop_sequence": null},
        "usage": {"input_tokens": usage.0, "output_tokens": usage.1}}))?;
    ev(w, "message_stop", json!({"type": "message_stop"}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_and_stream() {
        let r = json!({"model": "gpt-x", "stream": true, "system": [{"type": "text", "text": "be brief"}], "tools": [{"name": "Read", "input_schema": {"type": "object"}}],
            "messages": [{"role": "user", "content": "hi"},
                {"role": "assistant", "content": [{"type": "text", "text": "ok"}, {"type": "tool_use", "id": "t1", "name": "Read", "input": {"p": 1}}]},
                {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": [{"type": "text", "text": "data"}]}, {"type": "text", "text": "go"}]}]});
        let o = to_openai(&r);
        let m = o["messages"].as_array().unwrap();
        assert_eq!(m.iter().map(|m| m["role"].as_str().unwrap()).collect::<Vec<_>>(), ["system", "user", "assistant", "tool", "user"]);
        assert_eq!(m[2]["tool_calls"][0]["function"]["arguments"], "{\"p\":1}");
        assert_eq!((m[3]["tool_call_id"].as_str(), m[3]["content"].as_str()), (Some("t1"), Some("data")));
        assert_eq!((o["tools"][0]["function"]["name"].as_str(), o["tool_choice"].as_str()), (Some("Read"), Some("auto")));

        let up = ["{\"choices\":[{\"delta\":{\"content\":\"He\"}}]}", "{\"choices\":[{\"delta\":{\"content\":\"y\"}}]}",
            "{\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"Read\",\"arguments\":\"{\\\"p\\\"\"}}]}}]}",
            "{\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\":1}\"}}]},\"finish_reason\":\"stop\"}]}",
            "{\"choices\":[],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":3}}", "[DONE]"]
            .map(|d| format!("data: {d}\n\n")).concat();
        let mut out = vec![];
        stream(&mut out, up.as_bytes(), &json!("gpt-x")).unwrap();
        let ev: Vec<Value> = String::from_utf8(out).unwrap().lines().filter_map(|l| l.strip_prefix("data: ")).map(|d| serde_json::from_str(d).unwrap()).collect();
        let types: Vec<&str> = ev.iter().map(|e| e["type"].as_str().unwrap()).collect();
        assert_eq!(types, ["message_start", "content_block_start", "content_block_delta", "content_block_delta", "content_block_stop",
            "content_block_start", "content_block_delta", "content_block_delta", "content_block_stop", "message_delta", "message_stop"]);
        assert_eq!(ev[5]["content_block"]["id"], "c1");
        assert_eq!(ev[9]["delta"]["stop_reason"], "tool_use"); // Gemini says "stop" after a tool call
        assert_eq!(ev[9]["usage"]["output_tokens"], 3);
        assert_eq!(unhex(base("https://api.x.ai/v1/").rsplit('/').next().unwrap()).as_deref(), Some("https://api.x.ai/v1"));
    }

    #[test]
    fn plain_reply_images_and_choices() {
        let v = json!({"id": "c9", "choices": [{"finish_reason": "tool_calls", "message": {"content": "", "tool_calls": [{"id": "k1", "function": {"name": "Grep", "arguments": "{\"q\":\"x\"}"}}]}}],
            "usage": {"prompt_tokens": 5, "completion_tokens": 2}});
        let m = from_openai(&v, &json!("gpt-x"));
        assert_eq!((m["id"].as_str(), m["stop_reason"].as_str(), m["content"].as_array().unwrap().len()), (Some("msg_c9"), Some("tool_use"), 1));
        assert_eq!((m["content"][0]["input"]["q"].as_str(), m["usage"]["input_tokens"].as_u64()), (Some("x"), Some(5)));
        let t = from_openai(&json!({"choices": [{"finish_reason": "length", "message": {"content": "cut"}}]}), &json!("m"));
        assert_eq!((t["content"][0]["text"].as_str(), t["stop_reason"].as_str()), (Some("cut"), Some("max_tokens")));

        let r = json!({"model": "m", "system": "sys", "tool_choice": {"type": "tool", "name": "Read"}, "stop_sequences": ["END"],
            "tools": [{"name": "Read", "input_schema": {"type": "object"}}, {"type": "web_search_20250305", "name": "web_search"}],
            "messages": [{"role": "user", "content": [{"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": "QUJD"}}, {"type": "text", "text": "what?"}]},
                {"role": "assistant", "content": [{"type": "text", "text": "a"}, {"type": "text", "text": "b"}]},
                {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t", "is_error": true, "content": [{"type": "text", "text": "no"}, {"type": "image", "source": {}}]}]}]});
        let o = to_openai(&r);
        assert_eq!(o["messages"][0]["content"], "sys");
        assert_eq!(o["messages"][1]["content"][0]["image_url"]["url"], "data:image/jpeg;base64,QUJD");
        assert_eq!(o["messages"][2]["content"], "a\nb");
        let tr = o["messages"][3]["content"].as_str().unwrap();
        assert!(tr.starts_with("Error: no") && tr.contains("1 image(s) left out"), "{tr}");
        assert_eq!(o["tools"].as_array().unwrap().len(), 1, "server tools without a schema are not passed on");
        assert_eq!((o["tool_choice"]["function"]["name"].as_str(), o["stop"][0].as_str(), o.get("stream_options").is_none()), (Some("Read"), Some("END"), true));
        assert_eq!(to_openai(&json!({"tools": [{"name": "A", "input_schema": {}}], "tool_choice": {"type": "any"}}))["tool_choice"], "required");
    }

    #[test]
    fn upstream_errors_keep_their_kind() {
        let e: Value = serde_json::from_str(&err(429, "slow down")).unwrap();
        assert_eq!((e["error"]["type"].as_str(), e["error"]["message"].as_str()), (Some("rate_limit_error"), Some("slow down")));
        assert_eq!(serde_json::from_str::<Value>(&err(401, "")).unwrap()["error"]["type"], "authentication_error");
        // a mid-stream error from the endpoint ends the stream with an Anthropic error event
        let mut out = vec![];
        stream(&mut out, "data: {\"error\":{\"message\":\"boom\"}}\n\n".as_bytes(), &json!("m")).unwrap();
        assert!(String::from_utf8(out).unwrap().contains("\"message\":\"boom\""));
    }

    /// The listener end to end: a fake OpenAI endpoint, a streamed request through base(), Anthropic events back; the
    /// key reaches the endpoint as a Bearer token, a wrong secret gets 404.
    #[test]
    fn listener_end_to_end() {
        let up = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = up.local_addr().unwrap().port();
        let seen = std::thread::spawn(move || {
            let (mut c, _) = up.accept().unwrap();
            let mut r = BufReader::new(c.try_clone().unwrap());
            let (mut head, mut len) = (String::new(), 0usize);
            loop { let mut l = String::new(); r.read_line(&mut l).unwrap(); if l.trim().is_empty() { break; }
                if let Some(v) = l.to_ascii_lowercase().strip_prefix("content-length:") { len = v.trim().parse().unwrap(); } head += &l; }
            let mut b = vec![0; len]; r.read_exact(&mut b).unwrap();
            let body = "data: {\"choices\":[{\"delta\":{\"content\":\"PONG\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
            write!(c, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            (head, String::from_utf8(b).unwrap())
        });
        let b = base(&format!("http://127.0.0.1:{port}/v1"));
        let res = ureq::post(&format!("{b}/v1/messages")).set("x-api-key", "k-123")
            .send_string(&json!({"model": "m", "stream": true, "messages": [{"role": "user", "content": "ping"}]}).to_string()).unwrap();
        let text = res.into_string().unwrap();
        assert!(text.contains("\"text\":\"PONG\"") && text.contains("event: message_stop"), "{text}");
        let (head, body) = seen.join().unwrap();
        assert!(head.starts_with("POST /v1/chat/completions") && head.contains("Bearer k-123"), "{head}");
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["messages"][0]["content"][0]["text"], "ping");
        let wrong = b.replacen(crate::route::secret(), "0000", 1);
        assert!(matches!(ureq::post(&format!("{wrong}/v1/messages")).send_string("{}"), Err(ureq::Error::Status(404, _))));
        let n: Value = serde_json::from_str(&ureq::post(&format!("{b}/v1/messages/count_tokens")).send_string("12345678").unwrap().into_string().unwrap()).unwrap();
        assert_eq!(n["input_tokens"], 2);
    }
}
