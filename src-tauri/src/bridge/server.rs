//! Local HTTP endpoint for Claude Code `type: "http"` hooks. Single route `POST /hook`.

use serde_json::Value;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

type Handler = dyn Fn(Value) -> Value + Send + Sync;

const MAX_BODY: u64 = 1024 * 1024;
const MAX_HEAD: usize = 16 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(5);
/// Whole head + body must arrive within this, so a trickling client cannot hold a thread.
const READ_DEADLINE: Duration = Duration::from_secs(10);

/// 403 if any `Origin` header (blocks browsers), 401 unless `Authorization: Bearer <token>` matches.
pub fn authorize(headers: &[(String, String)], token: &str) -> Result<(), u16> {
    let get = |name: &str| headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str());
    if get("origin").is_some() {
        return Err(403);
    }
    match get("authorization").and_then(|v| v.strip_prefix("Bearer ")) {
        Some(t) if ct_eq(t.as_bytes(), token.as_bytes()) => Ok(()),
        _ => Err(401),
    }
}

/// Constant-time for equal lengths (the token length is not secret).
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Binds `addr` (must be loopback), answers each connection on its own thread, returns the bound port.
/// Plain std HTTP/1.1, one request per connection: no chunked bodies, no keep-alive.
pub fn serve(addr: &str, token: String, handler: Arc<Handler>) -> std::io::Result<u16> {
    serve_with(addr, token, handler, READ_DEADLINE)
}

fn serve_with(addr: &str, token: String, handler: Arc<Handler>, read_deadline: Duration) -> std::io::Result<u16> {
    let listener = TcpListener::bind(addr)?;
    let local = listener.local_addr()?;
    if !local.ip().is_loopback() {
        return Err(std::io::Error::other("bridge must bind a loopback address"));
    }
    let token: Arc<str> = token.into();
    std::thread::Builder::new().name("bridge-accept".into()).spawn(move || {
        static WARNED: AtomicBool = AtomicBool::new(false);
        for stream in listener.incoming() {
            let Ok(stream) = stream else {
                std::thread::sleep(Duration::from_millis(50)); // a persistent accept error must not spin
                continue;
            };
            let (token, handler) = (token.clone(), handler.clone());
            let spawned = std::thread::Builder::new()
                .name("bridge-conn".into())
                .spawn(move || handle(stream, &token, &*handler, Instant::now() + read_deadline));
            // no thread (resources exhausted): the connection is dropped, Claude Code carries on
            if let Err(e) = spawned {
                if !WARNED.swap(true, Ordering::Relaxed) {
                    eprintln!("bridge: cannot start connection thread: {e}");
                }
            }
        }
    })?;
    Ok(local.port())
}

fn handle(mut stream: TcpStream, token: &str, handler: &Handler, deadline: Instant) {
    if stream.set_write_timeout(Some(IO_TIMEOUT)).is_err() {
        return;
    }
    let (code, body) = match respond(&mut stream, token, handler, deadline) {
        Some(r) => r,
        None => return, // client went away or timed out before a full head
    };
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        411 => "Length Required",
        413 => "Content Too Large",
        _ => "Request Header Fields Too Large",
    };
    let body = if code == 200 { body.to_string() } else { String::new() };
    let resp = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.shutdown(Shutdown::Write);
    drain(&mut stream);
}

/// Discards what the client is still sending (bounded), so closing does not reset the connection before it reads the reply.
fn drain(stream: &mut TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let (deadline, mut buf, mut total) = (Instant::now() + Duration::from_secs(2), [0u8; 8192], 0usize);
    while Instant::now() < deadline && total < 2 * MAX_BODY as usize {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => total += n,
        }
    }
}

/// `None` = no reply possible; otherwise (status, body). Checks run before any body byte is read.
/// One read with timeout = min(5 s, time left); `None` once the deadline passed, on EOF or on error.
fn read_by(stream: &mut TcpStream, buf: &mut [u8], deadline: Instant) -> Option<usize> {
    let left = deadline.checked_duration_since(Instant::now()).filter(|d| !d.is_zero())?;
    stream.set_read_timeout(Some(left.min(IO_TIMEOUT))).ok()?;
    stream.read(buf).ok().filter(|&n| n > 0)
}

fn respond(stream: &mut TcpStream, token: &str, handler: &Handler, deadline: Instant) -> Option<(u16, Value)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HEAD {
            return Some((431, Value::Null));
        }
        let n = read_by(stream, &mut chunk, deadline)?;
        buf.extend_from_slice(&chunk[..n]);
    };
    if head_end + 4 > MAX_HEAD {
        return Some((431, Value::Null));
    }
    let Ok(head) = std::str::from_utf8(&buf[..head_end]) else { return Some((400, Value::Null)) };
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next().unwrap_or("").split(' ');
    let (method, target) = (request_line.next().unwrap_or(""), request_line.next().unwrap_or(""));
    let mut headers = Vec::new();
    for line in lines {
        let Some((k, v)) = line.split_once(':') else { return Some((400, Value::Null)) };
        headers.push((k.trim().to_string(), v.trim().to_string()));
    }
    if let Err(code) = authorize(&headers, token) {
        return Some((code, Value::Null));
    }
    if method != "POST" || target != "/hook" {
        return Some((404, Value::Null));
    }
    let get = |name: &str| headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str());
    if get("transfer-encoding").is_some() {
        return Some((411, Value::Null));
    }
    let len = match get("content-length").map(str::parse::<u64>) {
        None => return Some((411, Value::Null)),
        Some(Err(_)) => return Some((400, Value::Null)),
        Some(Ok(n)) if n > MAX_BODY => return Some((413, Value::Null)),
        Some(Ok(n)) => n as usize,
    };
    let mut body = buf.split_off(head_end + 4);
    body.truncate(len);
    let mut filled = body.len();
    body.resize(len, 0);
    while filled < len {
        filled += read_by(stream, &mut body[filled..], deadline)?;
    }
    match serde_json::from_slice::<Value>(&body) {
        Ok(ev) => Some((200, handler(ev))),
        Err(_) => Some((400, Value::Null)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn auth_rules_over_http() {
        let port = serve("127.0.0.1:0", "tok".into(), Arc::new(|_| json!({"ok": true}))).unwrap();
        let url = format!("http://127.0.0.1:{port}/hook");
        let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).build().into();
        let post = |auth: Option<&str>, origin: Option<&str>| {
            let mut r = agent.post(&url);
            if let Some(a) = auth { r = r.header("Authorization", a); }
            if let Some(o) = origin { r = r.header("Origin", o); }
            r.send_json(json!({"hook_event_name":"Stop"})).unwrap().status().as_u16()
        };
        assert_eq!(post(None, None), 401);
        assert_eq!(post(Some("Bearer nope"), None), 401);
        assert_eq!(post(Some("Bearer tok"), Some("http://evil.example")), 403);
        assert_eq!(post(Some("Bearer tok"), None), 200);
    }

    /// Sends raw bytes, returns the response status code (0 if the server sent nothing).
    fn raw(port: u16, req: &[u8]) -> u16 {
        use std::io::{Read, Write};
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(std::time::Duration::from_secs(3))).unwrap();
        let _ = s.write_all(req);
        let mut out = Vec::new();
        let _ = s.read_to_end(&mut out);
        String::from_utf8_lossy(&out).split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0)
    }

    fn alive(port: u16) -> bool {
        raw(port, b"POST /hook HTTP/1.1\r\nAuthorization: Bearer tok\r\nContent-Length: 2\r\n\r\n{}") == 200
    }

    #[test]
    fn hostile_heads_are_rejected_without_hurting_the_server() {
        let port = serve("127.0.0.1:0", "tok".into(), Arc::new(|_| json!({}))).unwrap();
        let t = std::time::Instant::now();
        assert_eq!(raw(port, b"POST /hook HTTP/1.1\r\nContent-Length: 99999999999\r\n\r\n"), 401);
        assert!(t.elapsed() < std::time::Duration::from_secs(2));
        assert!(alive(port));
        assert_eq!(raw(port, b"POST /hook HTTP/1.1\r\nAuthorization: Bearer tok\r\nContent-Length: 18446744073709551615\r\n\r\n"), 413);
        assert!(alive(port));
        let big = format!("POST /hook HTTP/1.1\r\nX-Pad: {}\r\n\r\n", "a".repeat(17 * 1024));
        assert_eq!(raw(port, big.as_bytes()), 431);
        assert!(alive(port));
        let chunked = b"POST /hook HTTP/1.1\r\nAuthorization: Bearer tok\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n0\r\n\r\n";
        assert_eq!(raw(port, chunked), 411);
        assert!(alive(port));
    }

    #[test]
    fn trickling_client_is_cut_off_at_the_deadline() {
        use std::io::{Read, Write};
        let port = serve_with("127.0.0.1:0", "tok".into(), Arc::new(|_| json!({})), Duration::from_millis(300)).unwrap();
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut w = s.try_clone().unwrap();
        let t = Instant::now();
        std::thread::spawn(move || {
            // one header byte every 50 ms: each read is quick, only the overall deadline stops it
            let _ = w.write_all(b"POST /hook HTTP/1.1\r\nX-Slow: ");
            while t.elapsed() < Duration::from_secs(3) && w.write_all(b"a").is_ok() {
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        let _ = s.read_to_end(&mut Vec::new()); // returns when the server drops the connection
        assert!(t.elapsed() < Duration::from_millis(1500), "held for {:?}", t.elapsed());
    }

    #[test]
    fn routes_body_limit_and_bad_json() {
        let port = serve("127.0.0.1:0", "tok".into(), Arc::new(|v| v)).unwrap();
        let base = format!("http://127.0.0.1:{port}");
        let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).build().into();
        let send = |path: &str, body: &str| {
            let r = agent.post(format!("{base}{path}")).header("Authorization", "Bearer tok");
            r.send(body).unwrap()
        };
        assert_eq!(send("/other", "{}").status().as_u16(), 404);
        let get = agent.get(format!("{base}/hook")).header("Authorization", "Bearer tok").call().unwrap();
        assert_eq!(get.status().as_u16(), 404);
        assert_eq!(send("/hook", "{nope").status().as_u16(), 400);
        assert_eq!(send("/hook", &"x".repeat(1024 * 1024 + 1)).status().as_u16(), 413);
        let mut ok = send("/hook", r#"{"echo":1}"#);
        assert_eq!(ok.body_mut().read_json::<Value>().unwrap(), json!({"echo":1}));
    }

    #[test]
    fn authorize_header_rules() {
        let h = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<Vec<_>>();
        assert_eq!(authorize(&h(&[("authorization", "Bearer tok")]), "tok"), Ok(()));
        assert_eq!(authorize(&h(&[("Authorization", "Bearer toK")]), "tok"), Err(401));
        assert_eq!(authorize(&h(&[("Authorization", "Bearer tok2")]), "tok"), Err(401));
        assert_eq!(authorize(&h(&[("Authorization", "tok")]), "tok"), Err(401));
        assert_eq!(authorize(&h(&[("Authorization", "Bearer tok"), ("ORIGIN", "null")]), "tok"), Err(403));
    }
}
