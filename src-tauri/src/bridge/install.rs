//! Merge / remove our hook entries in Claude Code's `~/.claude/settings.json`.

use serde_json::Value;
use std::path::Path;

pub fn connect(_settings: &mut Value, _url: &str, _token: &str) {
    unimplemented!()
}

pub fn disconnect(_settings: &mut Value, _url: &str) {
    unimplemented!()
}

pub fn is_connected(_settings: &Value, _url: &str, _token: &str) -> bool {
    unimplemented!()
}

pub fn write_settings(_path: &Path, _f: impl FnOnce(&mut Value)) -> Result<(), String> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const URL: &str = "http://127.0.0.1:47863/hook";

    #[test]
    fn connect_merges_idempotently_and_disconnect_removes_only_ours() {
        let mut s = json!({"model":"x","hooks":{"Stop":[{"hooks":[{"type":"command","command":"say done"}]}]}});
        connect(&mut s, URL, "t"); connect(&mut s, URL, "t");
        assert!(is_connected(&s, URL, "t")); assert!(!is_connected(&s, URL, "other"));
        let stop = s["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2); // user's group + ours, not duplicated
        assert_eq!(s["hooks"]["PreToolUse"][0]["matcher"], "AskUserQuestion");
        assert_eq!(s["hooks"]["PermissionRequest"][0]["hooks"][0]["timeout"], 25);
        disconnect(&mut s, URL);
        assert_eq!(s, json!({"model":"x","hooks":{"Stop":[{"hooks":[{"type":"command","command":"say done"}]}]}}));
    }

    #[test]
    fn invalid_settings_file_is_never_overwritten() {
        let dir = std::env::temp_dir().join(format!("dn-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(&path, "{ // comment\n \"a\": 1 }").unwrap();
        assert!(write_settings(&path, |s| connect(s, URL, "t")).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ // comment\n \"a\": 1 }");
    }

    #[test]
    fn eight_events_token_change_replaces_and_empty_settings_round_trip() {
        let mut s = json!({});
        connect(&mut s, URL, "old");
        connect(&mut s, URL, "new");
        let hooks = s["hooks"].as_object().unwrap();
        assert_eq!(hooks.len(), 8);
        assert!(hooks.values().all(|groups| groups.as_array().unwrap().len() == 1));
        assert_eq!(s["hooks"]["StopFailure"][0]["hooks"][0]["timeout"], 5);
        assert_eq!(s["hooks"]["SessionStart"][0]["hooks"][0]["headers"]["Authorization"], "Bearer new");
        assert!(s["hooks"]["Stop"][0]["hooks"][0].get("onFailure").is_none());
        assert!(is_connected(&s, URL, "new") && !is_connected(&s, URL, "old"));
        disconnect(&mut s, URL);
        assert_eq!(s, json!({}));
    }

    #[test]
    fn disconnect_keeps_user_hooks_sharing_a_group_and_unrelated_empty_groups() {
        let user = json!({"type":"command","command":"lint"});
        let mut s = json!({"hooks":{
            "Stop":[{"hooks":[user.clone(), {"type":"http","url":URL,"timeout":5}]}],
            "PostToolUse":[{"matcher":"Bash","hooks":[]}]}});
        disconnect(&mut s, URL);
        assert_eq!(s, json!({"hooks":{"Stop":[{"hooks":[user]}],"PostToolUse":[{"matcher":"Bash","hooks":[]}]}}));
    }

    #[test]
    fn unexpected_layout_is_left_alone() {
        let mut s = json!({"hooks":{"Stop":"oops"}});
        connect(&mut s, URL, "t");
        assert_eq!(s, json!({"hooks":{"Stop":"oops"}}));
        assert!(!is_connected(&s, URL, "t"));
    }

    #[test]
    fn write_settings_backs_up_creates_missing_and_preserves_key_order() {
        let dir = std::env::temp_dir().join(format!("dn-test-w-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join(".claude").join("settings.json");
        write_settings(&path, |s| connect(s, URL, "t")).unwrap(); // missing file and folder
        assert!(is_connected(&serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap(), URL, "t"));

        let original = "{\n  \"zeta\": 1,\n  \"alpha\": 2\n}";
        std::fs::write(&path, original).unwrap();
        write_settings(&path, |s| connect(s, URL, "t")).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join(".claude").join("settings.json.dynamicnotch.bak")).unwrap(), original);
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.find("zeta").unwrap() < written.find("alpha").unwrap());
        write_settings(&path, |s| disconnect(s, URL)).unwrap();
        let back: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back, serde_json::from_str::<Value>(original).unwrap());

        std::fs::write(&path, "[1]").unwrap();
        assert!(write_settings(&path, |s| connect(s, URL, "t")).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[1]");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
