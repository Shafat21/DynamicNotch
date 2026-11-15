//! Merge / remove our hook entries in Claude Code's `~/.claude/settings.json`.

use serde_json::{json, Value};
use std::path::Path;

/// Events we install, with the hook `timeout` (s). Blocking ones get 25 s so the island (15 s) answers first.
pub const EVENTS: [(&str, u64); 8] = [
    ("SessionStart", 5),
    ("UserPromptSubmit", 5),
    ("Notification", 5),
    ("Stop", 5),
    ("StopFailure", 5),
    ("SessionEnd", 5),
    ("PermissionRequest", 25),
    ("PreToolUse", 25),
];

/// Root is an object, `hooks` absent or an object, and each of our events absent or an array.
fn layout_ok(settings: &Value) -> bool {
    match settings.get("hooks") {
        None => settings.is_object(),
        Some(h) => h.as_object().is_some_and(|h| EVENTS.iter().all(|(e, _)| h.get(*e).is_none_or(Value::is_array))),
    }
}

/// Adds one group per event pointing at `url`; replaces any previous entry of ours. Unexpected layout → no change.
pub fn connect(settings: &mut Value, url: &str, token: &str) {
    if !layout_ok(settings) {
        return;
    }
    disconnect(settings, url);
    let root = settings.as_object_mut().expect("layout checked");
    let hooks = root.entry("hooks").or_insert_with(|| json!({})).as_object_mut().expect("layout checked");
    for (event, timeout) in EVENTS {
        let entry = json!({"type": "http", "url": url, "headers": {"Authorization": format!("Bearer {token}")}, "timeout": timeout});
        let group = if event == "PreToolUse" {
            json!({"matcher": "AskUserQuestion", "hooks": [entry]})
        } else {
            json!({"hooks": [entry]})
        };
        hooks.entry(event).or_insert_with(|| json!([])).as_array_mut().expect("layout checked").push(group);
    }
}

/// Removes hook entries whose `url` is ours, plus only the groups / event arrays / `hooks` key that this emptied.
pub fn disconnect(settings: &mut Value, url: &str) {
    let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) else { return };
    let mut emptied = Vec::new();
    for (event, groups) in hooks.iter_mut() {
        let Some(groups) = groups.as_array_mut() else { continue };
        let n = groups.len();
        groups.retain_mut(|g| {
            let Some(list) = g.get_mut("hooks").and_then(Value::as_array_mut) else { return true };
            let had = list.len();
            list.retain(|h| h["url"] != url);
            !(list.is_empty() && had > 0)
        });
        if groups.is_empty() && n > 0 {
            emptied.push(event.clone());
        }
    }
    for e in &emptied {
        hooks.shift_remove(e); // shift, not swap: keep the user's key order
    }
    if hooks.is_empty() && !emptied.is_empty() {
        settings.as_object_mut().expect("has hooks").shift_remove("hooks");
    }
}

/// All 8 events carry an entry with our URL and the current token.
pub fn is_connected(settings: &Value, url: &str, token: &str) -> bool {
    let auth = format!("Bearer {token}");
    EVENTS.iter().all(|(e, _)| {
        settings["hooks"][*e].as_array().is_some_and(|groups| {
            groups.iter().any(|g| {
                g["hooks"].as_array().is_some_and(|hs| {
                    hs.iter().any(|h| h["url"] == url && h["headers"]["Authorization"] == auth.as_str())
                })
            })
        })
    })
}

/// Read-modify-write `path`: missing → `{}`; invalid JSON or non-object → error, file untouched.
/// Existing file is copied to `settings.json.dynamicnotch.bak` first; the write is temp file + rename.
pub fn write_settings(path: &Path, f: impl FnOnce(&mut Value)) -> Result<(), String> {
    let shown = path.display();
    let mut settings = match std::fs::read_to_string(path) {
        Ok(text) => {
            let v: Value = serde_json::from_str(&text).map_err(|e| format!("{shown} is not valid JSON ({e}); left unchanged"))?;
            if !v.is_object() {
                return Err(format!("{shown} is not a JSON object; left unchanged"));
            }
            let mut bak = path.as_os_str().to_owned();
            bak.push(".dynamicnotch.bak");
            std::fs::copy(path, &bak).map_err(|e| format!("backup failed: {e}"))?;
            v
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(format!("cannot read {shown}: {e}")),
    };
    f(&mut settings);
    let dir = path.parent().ok_or("settings path has no folder")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".dynamicnotch.tmp");
    let text = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())? + "\n";
    std::fs::write(&tmp, text)
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("cannot write {shown}: {e}")
        })
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
