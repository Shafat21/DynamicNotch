pub mod install;
pub mod prompt;
pub mod server;
pub mod sessions;

use crate::AppState;
use prompt::Answer;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

// shortcut: spike (Task 1) assumed passing; flip ANSWERING off if an empty response does not fall back to the
// terminal prompt (drops all answering), or ANSWER_QUESTIONS off if updatedInput.answers does not answer AskUserQuestion.
pub const ANSWERING: bool = true;
pub const ANSWER_QUESTIONS: bool = true;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PromptInfo {
    pub id: u64,
    pub session_id: String,
    pub project: String,
    pub kind: &'static str,
    pub tool_name: String,
    pub summary: String,
    pub questions: Value,
    pub queue: usize,
}

/// Pending blocking prompts in FIFO order, each with the sender its hook thread waits on.
#[derive(Default)]
pub struct Prompts {
    inner: Mutex<(u64, Pending)>,
}

type Pending = Vec<(PromptInfo, Sender<Answer>)>;

impl Prompts {
    pub fn open(&self, mut p: PromptInfo) -> (u64, Receiver<Answer>) {
        let (tx, rx) = channel();
        let mut g = self.inner.lock().unwrap();
        g.0 += 1;
        p.id = g.0;
        g.1.push((p, tx));
        (g.0, rx)
    }

    /// Validates `a` for the prompt's kind, then hands it to the waiting hook. A prompt takes one answer.
    pub fn answer(&self, id: u64, a: Answer) -> Result<(), String> {
        let mut g = self.inner.lock().unwrap();
        let pos = g.1.iter().position(|(p, _)| p.id == id).ok_or("This prompt is no longer waiting")?;
        let p = &g.1[pos].0;
        match (p.kind, &a) {
            ("question", _) => prompt::question_response(&json!({"questions": p.questions}), &a).map(|_| ())?,
            (_, Answer::Answers { .. }) => return Err("not an answer to a permission request".into()),
            _ => {}
        }
        let (_, tx) = g.1.remove(pos);
        tx.send(a).map_err(|_| "This prompt is no longer waiting".to_string())
    }

    /// Waits for the answer, then closes the prompt. An answer accepted between the timeout and the
    /// close is still used; after the close, `answer` fails for this id.
    pub fn wait(&self, id: u64, rx: &Receiver<Answer>, timeout: Duration) -> Answer {
        let a = prompt::wait_answer(rx, timeout);
        self.close(id);
        rx.try_recv().unwrap_or(a)
    }

    pub fn close(&self, id: u64) {
        self.inner.lock().unwrap().1.retain(|(p, _)| p.id != id);
    }

    /// The prompt as shown in the island; `queue` = prompts pending right now.
    pub fn info(&self, id: u64) -> Option<PromptInfo> {
        let g = self.inner.lock().unwrap();
        let p = g.1.iter().find(|(p, _)| p.id == id)?;
        Some(PromptInfo { queue: g.1.len(), ..p.0.clone() })
    }
}

const PROMPT_WAIT: Duration = Duration::from_secs(15);

pub fn url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/hook")
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().home_dir().map_err(|e| e.to_string())?.join(".claude").join("settings.json"))
}

/// Hook URL and token (the token is generated and stored on first use).
fn endpoint(app: &AppHandle) -> Result<(String, String), String> {
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();
    Ok((url(db.bridge_port().map_err(|e| e.to_string())?), db.bridge_token(random_token).map_err(|e| e.to_string())?))
}

fn is_connected_now(app: &AppHandle) -> bool {
    let (Ok((url, token)), Ok(path)) = (endpoint(app), settings_path(app)) else { return false };
    let settings = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    settings.is_some_and(|s| install::is_connected(&s, &url, &token))
}

fn last_bridge(app: &AppHandle) -> Value {
    app.state::<AppState>().last.lock().unwrap().get("bridge").cloned().unwrap_or_else(|| json!({"online": false}))
}

/// Re-publishes sticky `bridge {online, connected, error?}` with `connected` re-read from settings.json.
pub fn publish_status(app: &AppHandle) {
    let mut v = last_bridge(app);
    v["connected"] = json!(is_connected_now(app));
    crate::publish(app, "bridge", v);
}

pub fn is_connected_shown(app: &AppHandle) -> bool {
    last_bridge(app)["connected"] == true
}

fn publish_sessions(app: &AppHandle) {
    let list = app.state::<AppState>().sessions.lock().unwrap().list();
    crate::publish(app, "sessions", list);
}

/// Starts the hook endpoint and the 60 s session prune timer. A taken port leaves the bridge offline only.
pub fn spawn(app: AppHandle) {
    let started = endpoint(&app).and_then(|(url, token)| {
        let addr = url.trim_start_matches("http://").trim_end_matches("/hook").to_string();
        let handle = app.clone();
        server::serve(&addr, token, Arc::new(move |ev| on_hook(&handle, ev))).map_err(|e| format!("{addr}: {e}"))
    });
    let base = match started {
        Ok(_) => json!({"online": true}),
        Err(e) => {
            eprintln!("bridge offline: {e}");
            json!({"online": false, "error": e})
        }
    };
    app.state::<AppState>().last.lock().unwrap().insert("bridge", base);
    publish_status(&app);
    publish_sessions(&app);
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(60));
        app.state::<AppState>().sessions.lock().unwrap().prune(now());
        publish_sessions(&app);
    });
}

fn on_hook(app: &AppHandle, ev: Value) -> Value {
    if !is_interactive(&ev) {
        return json!({});
    }
    let state = app.state::<AppState>();
    let pop = state.sessions.lock().unwrap().apply(&ev, now());
    publish_sessions(app);
    if let Some(p) = pop {
        let _ = app.emit("notify", json!({"app": p.project, "title": "", "text": p.text}));
    }
    let tool = ev["tool_name"].as_str().unwrap_or("");
    let kind = match ev["hook_event_name"].as_str() {
        Some("PermissionRequest") if ANSWERING => "permission",
        Some("PreToolUse") if ANSWERING && ANSWER_QUESTIONS && tool == "AskUserQuestion" => "question",
        _ => return json!({}),
    };
    // Task 22: while the island is hidden for a fullscreen app, return {} here (terminal asks) and drop the pop above.
    let input = &ev["tool_input"];
    let session_id = ev["session_id"].as_str().unwrap_or("").to_string();
    let project = state.sessions.lock().unwrap().list().into_iter().find(|s| s.id == session_id).map(|s| s.project);
    let question = kind == "question";
    let (id, rx) = state.prompts.open(PromptInfo {
        id: 0,
        session_id,
        project: project.unwrap_or_default(),
        kind,
        tool_name: tool.to_string(),
        summary: if question { String::new() } else { prompt::summary(tool, input) },
        questions: if question { input["questions"].clone() } else { json!([]) },
        queue: 0,
    });
    if let Some(info) = state.prompts.info(id) {
        let _ = app.emit("prompt", info);
    }
    let answer = state.prompts.wait(id, &rx, PROMPT_WAIT);
    let _ = app.emit("prompt-closed", json!({"id": id}));
    if question {
        // answer_prompt already validated the answer against these questions
        prompt::question_response(input, &answer).unwrap_or_else(|_| json!({}))
    } else {
        prompt::permission_response(&answer)
    }
}

/// Connect (after a native confirm) or disconnect our hooks in ~/.claude/settings.json. Blocks: not on the main thread.
pub fn set_connected(app: &AppHandle, on: bool) -> Result<(), String> {
    let path = settings_path(app)?;
    let (url, token) = endpoint(app)?;
    if on {
        if last_bridge(app)["online"] != true {
            // the port may belong to another program, which would then receive our token
            return Err("The island's hook endpoint is offline, so Claude Code cannot be connected.".into());
        }
        let events = install::EVENTS.map(|(e, _)| e).join(", ");
        let ok = app
            .dialog()
            .message(format!(
                "Add Dynamic Notch Island hooks to\n{}\n\nEvents: {events}\n\nThe current file is backed up to settings.json.dynamicnotch.bak.",
                path.display()
            ))
            .title("Connect Claude Code")
            .buttons(MessageDialogButtons::OkCancel)
            .blocking_show();
        if !ok {
            return Ok(());
        }
        install::write_settings(&path, |s| install::connect(s, &url, &token))?;
    } else if path.exists() {
        install::write_settings(&path, |s| install::disconnect(s, &url))?;
    }
    publish_status(app);
    if on && !is_connected_shown(app) {
        return Err(format!("{} has an unexpected \"hooks\" layout; no hooks were added.", path.display()));
    }
    Ok(())
}

/// 32 random bytes from the OS CSPRNG as 64 lowercase hex chars.
pub fn random_token() -> String {
    let mut buf = [0u8; 32];
    fill_random(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(windows)]
fn fill_random(buf: &mut [u8]) {
    use windows::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};
    // SAFETY: valid mutable buffer; no algorithm handle needed with the system-preferred RNG flag.
    unsafe { BCryptGenRandom(None, buf, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }.ok().expect("BCryptGenRandom failed");
}

#[cfg(target_os = "linux")]
fn fill_random(buf: &mut [u8]) {
    use std::io::Read;
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(buf)).expect("/dev/urandom unreadable");
}

/// Cuts to `max_chars` chars (never bytes), appending `…` when cut.
pub fn truncate(s: &str, max_chars: usize) -> String {
    match s.char_indices().nth(max_chars) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

pub fn is_interactive(_ev: &Value) -> bool {
    // shortcut: spike result pending; until then every session counts as interactive
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use prompt::wait_answer;
    use serde_json::json;
    use std::time::Duration;

    fn info(kind: &'static str) -> PromptInfo {
        PromptInfo { id: 0, session_id: "s1".into(), project: "p".into(), kind, tool_name: "Bash".into(),
            summary: "ls".into(), questions: json!([{"question":"Q?","options":[{"label":"A"}]}]), queue: 0 }
    }

    #[test]
    #[allow(unused_variables)] // brief's verbatim test names `a` for readability
    fn two_prompts_are_independent() {
        let p = Prompts::default();
        let (a, rxa) = p.open(info("permission")); let (b, rxb) = p.open(info("permission"));
        p.answer(b, Answer::Allow).unwrap();
        assert!(matches!(wait_answer(&rxb, Duration::from_millis(50)), Answer::Allow));
        assert!(matches!(wait_answer(&rxa, Duration::from_millis(50)), Answer::Dismiss)); // a untouched → times out
        assert!(p.answer(999, Answer::Allow).is_err());
    }

    #[test]
    fn answers_are_validated_per_kind_and_closed_prompts_reject() {
        let p = Prompts::default();
        let (q, rxq) = p.open(info("question"));
        assert_eq!(p.info(q).unwrap().queue, 1);
        let (perm, _rxp) = p.open(info("permission"));
        assert_eq!(p.info(perm).map(|i| (i.id, i.queue)), Some((perm, 2)));
        assert!(p.answer(q, Answer::Allow).is_err());
        let bad: Answer = serde_json::from_value(json!({"kind":"answers","picks":{"Q?":["Z"]}})).unwrap();
        assert!(p.answer(q, bad).is_err());
        let picks: Answer = serde_json::from_value(json!({"kind":"answers","picks":{"Q?":["A"]}})).unwrap();
        assert!(p.answer(perm, picks.clone()).is_err());
        p.answer(q, picks).unwrap();
        assert!(matches!(wait_answer(&rxq, Duration::from_millis(50)), Answer::Answers { .. }));
        assert!(p.answer(q, Answer::Dismiss).is_err()); // already answered
        p.close(perm);
        assert!(p.answer(perm, Answer::Allow).is_err());
        assert!(p.info(perm).is_none());
    }

    #[test]
    fn wait_closes_the_prompt_and_keeps_an_accepted_answer() {
        let p = Prompts::default();
        let (id, rx) = p.open(info("permission"));
        p.answer(id, Answer::Deny).unwrap();
        assert!(matches!(p.wait(id, &rx, Duration::ZERO), Answer::Deny));
        let (id, rx) = p.open(info("permission"));
        assert!(matches!(p.wait(id, &rx, Duration::from_millis(10)), Answer::Dismiss));
        assert!(p.answer(id, Answer::Allow).is_err()); // closed by wait
        assert!(p.info(id).is_none());
    }

    #[test]
    fn random_tokens_are_64_hex_and_differ() {
        let (a, b) = (random_token(), random_token());
        assert_ne!(a, b);
        for t in [a, b] {
            assert_eq!(t.len(), 64);
            assert!(t.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));
        }
    }

    #[test]
    fn truncate_is_char_safe() {
        let long = "🙂".repeat(300);
        assert_eq!(truncate(&long, 200).chars().count(), 201); // 200 + "…"
        assert_eq!(truncate("short", 200), "short");
    }

    #[test]
    fn session_start_is_interactive() {
        assert!(is_interactive(&json!({"hook_event_name": "SessionStart", "session_id": "s1"})));
    }
}
