use super::truncate;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

const STALE_SECS: u64 = 6 * 3600;

#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Idle,
    Working,
    NeedsYou,
    Done,
    Error,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub project: String,
    pub cwd: String,
    pub status: Status,
    pub message: String,
    pub updated_at: u64,
}

pub struct Pop {
    pub project: String,
    pub text: String,
}

#[derive(Default)]
pub struct Sessions {
    map: HashMap<String, Session>,
}

fn project_of(cwd: &str) -> String {
    cwd.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or("").to_string()
}

fn text<'a>(ev: &'a Value, key: &str) -> Option<&'a str> {
    ev.get(key).and_then(Value::as_str)
}

impl Sessions {
    pub fn apply(&mut self, ev: &Value, now: u64) -> Option<Pop> {
        let name = text(ev, "hook_event_name")?;
        let id = text(ev, "session_id")?;
        if name == "SessionEnd" {
            self.map.remove(id);
            return None;
        }
        let cwd = text(ev, "cwd").unwrap_or("");
        let s = self.map.entry(id.to_string()).or_insert_with(|| Session {
            id: id.to_string(),
            project: project_of(cwd),
            cwd: cwd.to_string(),
            status: Status::Idle,
            message: String::new(),
            updated_at: now,
        });
        s.updated_at = now;
        let (status, message, pop_text) = match name {
            "UserPromptSubmit" => (Status::Working, None, None),
            "Stop" => {
                let m = truncate(text(ev, "last_assistant_message").unwrap_or(""), 200);
                (Status::Done, Some(m), Some("Done".to_string()))
            }
            "StopFailure" => {
                let m = truncate(text(ev, "error").or(text(ev, "message")).unwrap_or(""), 200);
                (Status::Error, Some(m), Some("Something broke".to_string()))
            }
            "Notification"
                if matches!(
                    text(ev, "notification_type"),
                    Some("idle_prompt" | "agent_needs_input" | "elicitation_dialog")
                ) =>
            {
                let m = truncate(text(ev, "message").unwrap_or(""), 200);
                (Status::NeedsYou, Some(m.clone()), Some(m))
            }
            "SessionStart" => (Status::Idle, None, None),
            _ => return None,
        };
        s.status = status;
        if let Some(m) = message {
            s.message = m;
        }
        pop_text.map(|text| Pop { project: s.project.clone(), text })
    }

    pub fn prune(&mut self, now: u64) {
        self.map.retain(|_, s| now.saturating_sub(s.updated_at) <= STALE_SECS);
    }

    pub fn list(&self) -> Vec<Session> {
        let mut v: Vec<Session> = self.map.values().cloned().collect();
        v.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(name: &str, extra: Value) -> Value {
        let mut v = json!({"hook_event_name": name, "session_id": "s1", "cwd": r"E:\code\webshop"});
        v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        v
    }

    #[test]
    fn lifecycle_transitions_and_pops() {
        let mut s = Sessions::default();
        assert!(s.apply(&ev("SessionStart", json!({})), 1).is_none());
        assert_eq!(s.list()[0].project, "webshop");
        assert_eq!(s.list()[0].status, Status::Idle);
        s.apply(&ev("UserPromptSubmit", json!({})), 2);
        assert_eq!(s.list()[0].status, Status::Working);
        let pop = s.apply(&ev("Stop", json!({"last_assistant_message": "Stripe checkout is live, 40 tests pass"})), 3).unwrap();
        assert_eq!((pop.project.as_str(), s.list()[0].status), ("webshop", Status::Done));
        assert!(s.apply(&ev("Notification", json!({"notification_type": "idle_prompt", "message": "Claude is waiting"})), 4).is_some());
        assert_eq!(s.list()[0].status, Status::NeedsYou);
        assert!(s.apply(&ev("Notification", json!({"notification_type": "permission_prompt", "message": "x"})), 5).is_none());
        let pop = s.apply(&ev("StopFailure", json!({"error": "API overloaded"})), 7).unwrap();
        assert_eq!(pop.text, "Something broke");
        assert_eq!(s.list()[0].status, Status::Error);
        assert_eq!(s.list()[0].message, "API overloaded");
        s.apply(&ev("SessionEnd", json!({})), 8);
        assert!(s.list().is_empty());
    }

    #[test]
    fn unknown_session_event_creates_it_and_prune_drops_stale() {
        let mut s = Sessions::default();
        s.apply(&ev("UserPromptSubmit", json!({})), 0); // no SessionStart seen (island started later)
        assert_eq!(s.list().len(), 1);
        s.prune(6 * 3600 - 1);
        assert_eq!(s.list().len(), 1);
        s.prune(6 * 3600 + 1);
        assert!(s.list().is_empty());
    }
}
