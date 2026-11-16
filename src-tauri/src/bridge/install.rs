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

/// Points `statusLine` at our forwarder `command`. A user statusLine that is not ours is handed to
/// `save_chain` first; if saving fails, the user's statusLine is left in place. Non-object root → no change.
pub fn connect_statusline(settings: &mut Value, command: &str, save_chain: impl FnOnce(&Value) -> bool) {
    let Some(root) = settings.as_object_mut() else { return };
    if let Some(user) = root.get("statusLine").filter(|v| !crate::statusline::is_ours(v)) {
        if !save_chain(user) {
            return;
        }
    }
    // keep the user's other fields (padding, …) while connected; only type/command are ours
    let mut line = root.get("statusLine").filter(|v| v.is_object()).cloned().unwrap_or_else(|| json!({}));
    let obj = line.as_object_mut().expect("object");
    obj.insert("type".into(), "command".into());
    obj.insert("command".into(), command.into());
    obj.entry("refreshInterval").or_insert(30.into());
    // assigning an existing key keeps its position, so disconnect restores the file's key order
    root.insert("statusLine".into(), line);
}

/// The user's statusLine from `settings.json.dynamicnotch.bak` (if not ours): fallback when the DB lost the chain.
pub fn bak_statusline(path: &Path) -> Option<Value> {
    let mut bak = path.as_os_str().to_owned();
    bak.push(".dynamicnotch.bak");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(bak).ok()?).ok()?;
    v.get("statusLine").filter(|s| !crate::statusline::is_ours(s)).cloned()
}

/// Restores the saved original (`chain`) or removes `statusLine`, but only while it is still ours.
pub fn disconnect_statusline(settings: &mut Value, chain: Option<&Value>) {
    let Some(root) = settings.as_object_mut() else { return };
    if !root.get("statusLine").is_some_and(crate::statusline::is_ours) {
        return;
    }
    match chain {
        Some(original) => root.insert("statusLine".into(), original.clone()),
        None => root.shift_remove("statusLine"),
    };
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
            // keep the first real original: don't replace an existing backup with a file already carrying our line
            if !Path::new(&bak).exists() || !v.get("statusLine").is_some_and(crate::statusline::is_ours) {
                std::fs::copy(path, &bak).map_err(|e| format!("backup failed: {e}"))?;
            }
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

    /// Our forwarder command for the running (test) exe, as Connect writes it.
    fn ours() -> String {
        crate::statusline::statusline_command(r"C:\Program Files\Dynamic Notch Island\dynamic-notch.exe", true, true)
    }

    #[test]
    fn statusline_chain_stored_once_reconnect_idempotent_disconnect_restores_exactly() {
        let ours = ours();
        let user = json!({"type":"command","command":"powershell -NoProfile -File C:/caveman-statusline.ps1","padding":0});
        let original = json!({"model":"x","statusLine":user.clone(),"theme":"dark"});
        let mut s = original.clone();
        let mut saved: Vec<Value> = Vec::new();
        connect_statusline(&mut s, &ours, |v| { saved.push(v.clone()); true });
        connect_statusline(&mut s, &ours, |v| { saved.push(v.clone()); true }); // reconnect: already ours
        assert_eq!(saved, vec![user.clone()]);
        assert_eq!(s["statusLine"], json!({"type":"command","command":ours,"padding":0,"refreshInterval":30})); // padding kept
        disconnect_statusline(&mut s, saved.first());
        assert_eq!(serde_json::to_string(&s).unwrap(), serde_json::to_string(&original).unwrap()); // same key order too
    }

    #[test]
    fn statusline_keeps_users_refresh_interval_and_foreign_flag_command_is_chained() {
        let ours = ours();
        let user = json!({"type":"command","command":"my-tool --statusline","refreshInterval":5});
        let mut s = json!({"statusLine":user.clone()});
        let mut saved = None;
        connect_statusline(&mut s, &ours, |v| { saved = Some(v.clone()); true });
        assert_eq!(saved, Some(user.clone())); // same flag name, not our exe: saved, not overwritten
        assert_eq!(s["statusLine"], json!({"type":"command","command":ours,"refreshInterval":5}));
    }

    #[test]
    fn bak_statusline_returns_users_line_only() {
        let dir = std::env::temp_dir().join(format!("dn-test-bak-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let bak = dir.join("settings.json.dynamicnotch.bak");
        assert_eq!(bak_statusline(&path), None);
        std::fs::write(&bak, r#"{"statusLine":{"type":"command","command":"mine","padding":1}}"#).unwrap();
        assert_eq!(bak_statusline(&path), Some(json!({"type":"command","command":"mine","padding":1})));
        std::fs::write(&bak, json!({"statusLine":{"type":"command","command":ours()}}).to_string()).unwrap();
        assert_eq!(bak_statusline(&path), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bak_keeps_first_original_across_repeated_connects() {
        let dir = std::env::temp_dir().join(format!("dn-test-bak2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let user = json!({"type":"command","command":"mine","padding":2});
        std::fs::write(&path, json!({"statusLine":user.clone()}).to_string()).unwrap();
        let ours = ours();
        write_settings(&path, |s| connect_statusline(s, &ours, |_| true)).unwrap();
        write_settings(&path, |s| connect_statusline(s, &ours, |_| panic!("already ours"))).unwrap(); // reconnect
        assert_eq!(bak_statusline(&path), Some(user));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn moved_or_dev_build_forwarder_is_replaced_not_chained() {
        let old = crate::statusline::statusline_command(r"D:\old\dynamic-notch.exe", true, false);
        let dev = crate::statusline::statusline_command(r"E:\src\target\debug\dynamic-notch.exe", true, true);
        for prev in [old, dev] {
            let mut s = json!({"statusLine":{"type":"command","command":prev,"padding":1}});
            connect_statusline(&mut s, &ours(), |_| panic!("a forwarder must never become the chain"));
            assert_eq!(s["statusLine"], json!({"type":"command","command":ours(),"padding":1,"refreshInterval":30}));
            let mut d = s.clone();
            disconnect_statusline(&mut d, None); // still recognised as ours: removed
            assert!(d.get("statusLine").is_none());
        }
    }

    #[test]
    fn statusline_none_before_is_removed_after_and_failed_save_keeps_users() {
        let ours = ours();
        let mut s = json!({"model":"x"});
        connect_statusline(&mut s, &ours, |_| panic!("nothing to save"));
        assert_eq!(s["statusLine"]["refreshInterval"], 30);
        disconnect_statusline(&mut s, None);
        assert_eq!(s, json!({"model":"x"}));

        let mut s = json!({"statusLine":{"type":"command","command":"mine"}});
        connect_statusline(&mut s, &ours, |_| false);
        assert_eq!(s, json!({"statusLine":{"type":"command","command":"mine"}}));
        disconnect_statusline(&mut s, None); // not ours: user changed it, leave it
        assert_eq!(s, json!({"statusLine":{"type":"command","command":"mine"}}));

        let mut s = json!([1]);
        connect_statusline(&mut s, &ours, |_| true);
        disconnect_statusline(&mut s, None);
        assert_eq!(s, json!([1]));
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
