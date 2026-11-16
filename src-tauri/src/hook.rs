//! `dynamic-notch --hook --agent <id> [Event]`: hook command for agents that can only run commands
//! (Codex, Gemini CLI, Cursor, Copilot CLI; see `bridge::agents`). Maps the agent's payload onto Claude
//! Code's event and field names, tags it `agent`, POSTs it to the island's bridge and prints what the agent
//! expects back. Runs without Tauri, never panics on bad input, always exits 0.
//!
//! Never blocks the agent: no island → exits at once. Nothing that allows a tool is printed unless the
//! bridge answered with a decision the user clicked; otherwise the agent asks in its own terminal.

use crate::bridge::{agents, prompt};
use serde_json::{json, Map, Value};
use std::io::{Read, Write};
use std::time::Duration;

pub const FLAG: &str = "--hook";
const MAX_STDIN: u64 = 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
/// The bridge answers a prompt within 15 s (PROMPT_WAIT); the hook timeout we install is 25 s.
const DECISION_WAIT: Duration = Duration::from_secs(20);

#[derive(Debug, PartialEq)]
pub struct Args {
    pub agent: String,
    pub event: String,
}

/// `--agent <id>` and the first other non-flag word (the event, for agents that leave it out of the payload).
pub fn parse_args(args: impl IntoIterator<Item = String>) -> Args {
    let (mut agent, mut event) = (String::new(), String::new());
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        if a == "--agent" {
            agent = it.next().unwrap_or_default();
        } else if !a.starts_with("--") && event.is_empty() {
            event = a;
        }
    }
    Args { agent, event }
}

/// Claude Code's name for an agent's event. Claude-style names and unknown ones pass through.
pub fn event(name: &str) -> &str {
    match name {
        // Gemini CLI
        "BeforeAgent" => "UserPromptSubmit",
        "AfterAgent" => "Stop",
        "BeforeTool" => "PreToolUse",
        "AfterTool" => "PostToolUse",
        // Copilot CLI and Cursor (camelCase)
        "sessionStart" => "SessionStart",
        "sessionEnd" => "SessionEnd",
        "userPromptSubmitted" | "beforeSubmitPrompt" => "UserPromptSubmit",
        "agentStop" | "stop" => "Stop",
        "preToolUse" => "PreToolUse",
        "postToolUse" => "PostToolUse",
        "permissionRequest" => "PermissionRequest",
        "notification" => "Notification",
        other => other,
    }
}

fn non_empty<'a>(m: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    m.get(k).and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// Fills `tool_name`, `tool_input`, `session_id` and `cwd` from wherever the agent put them. Never overwrites.
fn fields(m: &mut Map<String, Value>, env: &dyn Fn(&str) -> Option<String>) {
    if !m.contains_key("tool_name") {
        // Copilot: toolName. Gemini: toolCall.name.
        let name = non_empty(m, "toolName").or_else(|| m.get("toolCall").and_then(|t| t.get("name")).and_then(Value::as_str));
        if let Some(n) = name.map(str::to_string) {
            m.insert("tool_name".into(), n.into());
        }
    }
    if !m.contains_key("tool_input") {
        // Copilot sends toolArgs as an object, or as the JSON text of one.
        let input = match m.get("toolArgs") {
            Some(Value::Object(o)) => Some(Value::Object(o.clone())),
            Some(Value::String(s)) => serde_json::from_str::<Value>(s).ok().filter(Value::is_object),
            _ => m.get("toolCall").and_then(|t| t.get("args")).filter(|a| a.is_object()).cloned(),
        };
        if let Some(i) = input {
            m.insert("tool_input".into(), i);
        }
    }
    if non_empty(m, "session_id").is_none() {
        // Copilot: sessionId. Cursor: conversation_id. Gemini: GEMINI_SESSION_ID as a last resort.
        let sid = ["sessionId", "conversation_id", "conversationId"].iter().find_map(|k| non_empty(m, k)).map(str::to_string);
        if let Some(s) = sid.or_else(|| env("GEMINI_SESSION_ID").filter(|s| !s.is_empty())) {
            m.insert("session_id".into(), s.into());
        }
    }
    if non_empty(m, "cwd").is_none() {
        // Copilot: workdir. Cursor: workspace_roots. Gemini: workspacePaths.
        let cwd = non_empty(m, "workdir").map(str::to_string).or_else(|| {
            ["workspace_roots", "workspacePaths"].iter().find_map(|k| m.get(*k)?.as_array()?.first()?.as_str().filter(|s| !s.is_empty()).map(str::to_string))
        });
        if let Some(c) = cwd {
            m.insert("cwd".into(), c.into());
        }
    }
}

/// The bridge body and the canonical event name, or None if stdin is not a JSON object.
pub fn prepare(raw: &[u8], args: &Args, env: &dyn Fn(&str) -> Option<String>, cwd: &str) -> Option<(Vec<u8>, String)> {
    let raw = raw.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(raw); // some shells add a UTF-8 BOM
    let mut v: Value = serde_json::from_slice(raw).ok()?;
    let m = v.as_object_mut()?;
    let raw_event = non_empty(m, "hook_event_name").unwrap_or(&args.event).to_string();
    fields(m, env);
    let mut name = event(&raw_event).to_string();
    if name == "Stop" && m.get("status").and_then(Value::as_str) == Some("error") {
        name = "StopFailure".into(); // Cursor reports a failed turn as a stop with an error status
    }
    m.insert("hook_event_name".into(), name.clone().into());
    m.insert("agent".into(), args.agent.clone().into());
    if non_empty(m, "cwd").is_none() && !cwd.is_empty() {
        m.insert("cwd".into(), cwd.into());
    }
    Some((serde_json::to_vec(&v).ok()?, name))
}

/// What to print for `event` given the bridge's reply (`None`: no island or no answer).
pub fn reply(agent: &str, event: &str, bridge: Option<&Value>) -> Option<String> {
    if event != "PermissionRequest" {
        return matches!(agent, "gemini" | "copilot").then(|| "{}".to_string()); // they read "{}" as no opinion
    }
    let behavior = bridge
        .filter(|_| agents::takes_decisions(agent))
        .and_then(|b| b["hookSpecificOutput"]["decision"]["behavior"].as_str())
        .filter(|b| matches!(*b, "allow" | "deny"));
    match agent {
        // Copilot is fail-closed: always valid JSON, "ask" when nobody clicked.
        "copilot" => Some(json!({"permissionDecision": behavior.unwrap_or("ask")}).to_string()),
        // Codex reads Claude Code's documented hookSpecificOutput; silence = ask in the terminal.
        _ => behavior.map(|b| {
            let a = if b == "allow" { prompt::Answer::Allow } else { prompt::Answer::Deny };
            prompt::permission_response(&a).to_string()
        }),
    }
}

/// POSTs `body` to the bridge; with `wait`, returns the JSON reply. Raw TCP: no proxy lookup, the token stays local.
fn send(port: u16, token: &str, body: &[u8], wait: Option<Duration>) -> Option<Value> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = std::net::TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).ok()?;
    s.set_write_timeout(Some(CONNECT_TIMEOUT)).ok()?;
    let head = format!(
        "POST /hook HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    s.write_all(head.as_bytes()).and_then(|()| s.write_all(body)).ok()?;
    s.set_read_timeout(Some(wait?)).ok()?;
    let mut resp = Vec::new();
    let _ = s.take(64 * 1024).read_to_end(&mut resp);
    parse_response(&resp)
}

/// The JSON body of a `200` response.
fn parse_response(resp: &[u8]) -> Option<Value> {
    let end = resp.windows(4).position(|w| w == b"\r\n\r\n")?;
    let status = std::str::from_utf8(&resp[..end]).ok()?.split(' ').nth(1)?;
    (status == "200").then(|| serde_json::from_slice(&resp[end + 4..]).ok()).flatten()
}

fn print(line: Option<String>) {
    if let Some(l) = line {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{l}").and_then(|()| out.flush());
    }
}

pub fn forward() {
    let args = parse_args(std::env::args().skip(1));
    let mut raw = Vec::new();
    let _ = std::io::stdin().take(MAX_STDIN).read_to_end(&mut raw);
    let env = |k: &str| std::env::var(k).ok();
    let cwd = std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let Some((body, name)) = prepare(&raw, &args, &env, &cwd) else {
        return print(reply(&args.agent, event(&args.event), None));
    };
    let wait = (name == "PermissionRequest" && agents::takes_decisions(&args.agent)).then_some(DECISION_WAIT);
    let db = crate::statusline::db_path(cfg!(windows), env("APPDATA").as_deref(), env("XDG_DATA_HOME").as_deref(), env("HOME").as_deref());
    let answer = db
        .and_then(|p| crate::statusline::read_db(&p))
        .and_then(|(port, token, _)| send(port, &token?, &body, wait));
    print(reply(&args.agent, &name, answer.as_ref()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn run(raw: &str, agent: &str, ev: &str) -> (Value, String) {
        let args = Args { agent: agent.into(), event: ev.into() };
        let (body, name) = prepare(raw.as_bytes(), &args, &no_env, "/fallback").unwrap();
        (serde_json::from_slice(&body).unwrap(), name)
    }

    #[test]
    fn args_take_agent_and_event_in_any_order() {
        let a = |v: &[&str]| parse_args(v.iter().map(|s| s.to_string()));
        assert_eq!(a(&["--hook", "--agent", "copilot", "agentStop"]), Args { agent: "copilot".into(), event: "agentStop".into() });
        assert_eq!(a(&["Stop", "--hook", "--agent", "codex"]), Args { agent: "codex".into(), event: "Stop".into() });
        assert_eq!(a(&["--hook"]), Args { agent: String::new(), event: String::new() });
    }

    #[test]
    fn events_map_onto_claude_codes() {
        for (raw, want) in [
            ("BeforeAgent", "UserPromptSubmit"), ("AfterAgent", "Stop"), ("sessionStart", "SessionStart"),
            ("userPromptSubmitted", "UserPromptSubmit"), ("beforeSubmitPrompt", "UserPromptSubmit"), ("agentStop", "Stop"),
            ("stop", "Stop"), ("permissionRequest", "PermissionRequest"), ("sessionEnd", "SessionEnd"),
            ("PermissionRequest", "PermissionRequest"), ("somethingNew", "somethingNew"),
        ] {
            assert_eq!(event(raw), want, "{raw}");
        }
    }

    #[test]
    fn payloads_are_tagged_renamed_and_filled() {
        // Copilot: event only on the command line, camelCase fields
        let (v, name) = run(r#"{"sessionId":"s-9","workdir":"/w/app","toolName":"bash","toolArgs":"{\"command\":\"npm test\"}"}"#, "copilot", "permissionRequest");
        assert_eq!(name, "PermissionRequest");
        assert_eq!((v["agent"].as_str(), v["session_id"].as_str(), v["cwd"].as_str()), (Some("copilot"), Some("s-9"), Some("/w/app")));
        assert_eq!((v["tool_name"].as_str(), v["tool_input"]["command"].as_str()), (Some("bash"), Some("npm test")));
        // Cursor: conversation + workspace, a failed stop
        let (v, name) = run(r#"{"hook_event_name":"stop","status":"error","conversation_id":"c-1","workspace_roots":["/w/p"]}"#, "cursor", "stop");
        assert_eq!((name.as_str(), v["session_id"].as_str(), v["cwd"].as_str()), ("StopFailure", Some("c-1"), Some("/w/p")));
        // Codex: Claude-style already; payload's event wins over the command line; cwd fallback
        let (v, name) = run("\u{feff}{\"hook_event_name\":\"Stop\",\"session_id\":\"x\"}", "codex", "SessionStart");
        assert_eq!((name.as_str(), v["cwd"].as_str()), ("Stop", Some("/fallback")));
        // Gemini session from the environment
        let args = Args { agent: "gemini".into(), event: "BeforeAgent".into() };
        let (body, _) = prepare(b"{}", &args, &|k| (k == "GEMINI_SESSION_ID").then(|| "g-7".into()), "").unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&body).unwrap()["session_id"], "g-7");
        // what the payload already says is kept
        let (v, _) = run(r#"{"session_id":"keep","cwd":"/keep","sessionId":"no","workdir":"/no"}"#, "copilot", "x");
        assert_eq!((v["session_id"].as_str(), v["cwd"].as_str()), (Some("keep"), Some("/keep")));
        assert!(prepare(b"[1]", &Args { agent: "codex".into(), event: String::new() }, &no_env, "").is_none());
    }

    #[test]
    fn nothing_is_allowed_without_a_click() {
        let allow = prompt::permission_response(&prompt::Answer::Allow);
        let deny = prompt::permission_response(&prompt::Answer::Deny);
        let forged = json!({"permissionDecision": "allow", "hookSpecificOutput": {"decision": {"behavior": "maybe"}}});
        for agent in ["codex", "copilot", "gemini", "cursor", "", "my-tool"] {
            for ev in ["SessionStart", "UserPromptSubmit", "Stop", "PreToolUse", "PermissionRequest"] {
                for bridge in [None, Some(json!({})), Some(forged.clone())] {
                    if let Some(out) = reply(agent, ev, bridge.as_ref()) {
                        assert!(!out.contains("allow"), "{agent} {ev}: {out}");
                    }
                }
                if ev != "PermissionRequest" {
                    assert!(reply(agent, ev, Some(&allow)).is_none_or(|o| o == "{}"), "{agent} {ev}");
                }
            }
        }
        assert_eq!(reply("codex", "PermissionRequest", Some(&allow)), Some(allow.to_string()));
        assert_eq!(reply("codex", "PermissionRequest", Some(&deny)), Some(deny.to_string()));
        assert_eq!(reply("codex", "PermissionRequest", None), None);
        assert_eq!(reply("copilot", "PermissionRequest", Some(&allow)).unwrap(), r#"{"permissionDecision":"allow"}"#);
        assert_eq!(reply("copilot", "PermissionRequest", Some(&deny)).unwrap(), r#"{"permissionDecision":"deny"}"#);
        assert_eq!(reply("copilot", "PermissionRequest", None).unwrap(), r#"{"permissionDecision":"ask"}"#);
        // agents without approvals never pass a decision on
        assert_eq!(reply("gemini", "PermissionRequest", Some(&allow)), None);
        assert_eq!(reply("gemini", "Stop", None).as_deref(), Some("{}"));
        assert_eq!(reply("cursor", "Stop", None), None);
    }

    #[test]
    fn only_a_200_json_body_counts() {
        assert_eq!(parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}"), Some(json!({})));
        assert_eq!(parse_response(b"HTTP/1.1 401 Unauthorized\r\n\r\n"), None);
        assert_eq!(parse_response(b"HTTP/1.1 200 OK\r\n\r\nnot json"), None);
        assert_eq!(parse_response(b"garbage"), None);
    }
}
