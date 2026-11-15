//! Local HTTP endpoint for Claude Code `type: "http"` hooks. Single route `POST /hook`.

use serde_json::Value;
use std::sync::Arc;

pub fn authorize(_headers: &[(String, String)], _token: &str) -> Result<(), u16> {
    unimplemented!()
}

pub fn serve(_addr: &str, _token: String, _handler: Arc<dyn Fn(Value) -> Value + Send + Sync>) -> std::io::Result<u16> {
    unimplemented!()
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
