// wired up by the bridge server (Task 8)
#![allow(dead_code)]

pub mod install;
pub mod prompt;
pub mod server;
pub mod sessions;

use prompt::Answer;
use serde::Serialize;
use serde_json::Value;
use std::sync::mpsc::Receiver;

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

#[derive(Default)]
pub struct Prompts {}

impl Prompts {
    pub fn open(&self, _p: PromptInfo) -> (u64, Receiver<Answer>) {
        unimplemented!()
    }
    pub fn answer(&self, _id: u64, _a: Answer) -> Result<(), String> {
        unimplemented!()
    }
    pub fn close(&self, _id: u64) {}
    pub fn info(&self, _id: u64) -> Option<PromptInfo> {
        unimplemented!()
    }
}

pub fn random_token() -> String {
    unimplemented!()
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
