// wired up by the bridge server (Task 8)
#![allow(dead_code)]

pub mod prompt;
pub mod sessions;

use serde_json::Value;

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
    use serde_json::json;

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
