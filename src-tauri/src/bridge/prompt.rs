use super::truncate;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::time::Duration;

#[derive(Deserialize, Debug, Clone)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Answer {
    Allow,
    Deny,
    Dismiss,
    Answers {
        picks: HashMap<String, Vec<String>>,
        #[serde(default)]
        other: HashMap<String, String>,
    },
}

pub fn permission_response(a: &Answer) -> Value {
    let decision = match a {
        Answer::Allow => json!({"behavior": "allow"}),
        Answer::Deny => json!({"behavior": "deny", "message": "Denied from DynamicNotch"}),
        _ => return json!({}), // dismiss or wrong kind: fall back to the terminal prompt
    };
    json!({"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": decision}})
}

pub fn question_response(tool_input: &Value, a: &Answer) -> Result<Value, String> {
    let (picks, other) = match a {
        Answer::Answers { picks, other } => (picks, other),
        Answer::Dismiss => return Ok(json!({})),
        _ => return Err("not an answer to a question".into()),
    };
    let mut answers = serde_json::Map::new();
    for q in tool_input["questions"].as_array().into_iter().flatten() {
        let Some(text) = q["question"].as_str() else { continue };
        let o = other.get(text).map_or("", |o| o.trim());
        if o.chars().count() > 500 {
            return Err("other text too long".into());
        }
        let p = picks.get(text).map_or(&[][..], |p| &p[..]);
        let multi = q["multiSelect"] == true;
        if !multi && p.len() > 1 {
            return Err(format!("multiple picks for single-select question: {text}"));
        }
        if !multi && !p.is_empty() && !o.is_empty() {
            return Err("pick one option or type an answer, not both".into());
        }
        let labels: Vec<&str> =
            q["options"].as_array().into_iter().flatten().filter_map(|o| o["label"].as_str()).collect();
        if let Some(bad) = p.iter().find(|l| !labels.contains(&l.as_str())) {
            return Err(format!("unknown option: {bad}"));
        }
        let mut parts: Vec<&str> = p.iter().map(String::as_str).collect();
        if !o.is_empty() {
            parts.push(o);
        }
        if !parts.is_empty() {
            answers.insert(text.into(), json!(parts.join(", ")));
        }
    }
    Ok(json!({"hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "allow",
        "updatedInput": {"questions": tool_input["questions"], "answers": answers}
    }}))
}

pub fn summary(tool_name: &str, tool_input: &Value) -> String {
    let key = match tool_name {
        "Bash" | "PowerShell" => "command",
        "Edit" | "Write" | "Read" | "NotebookEdit" => "file_path",
        _ => "",
    };
    let s = tool_input[key].as_str().map_or_else(|| tool_input.to_string(), str::to_string);
    truncate(&s, 160)
}

/// Timeout or dropped sender both mean dismiss.
pub fn wait_answer(rx: &Receiver<Answer>, timeout: Duration) -> Answer {
    rx.recv_timeout(timeout).unwrap_or(Answer::Dismiss)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_shapes() {
        assert_eq!(permission_response(&Answer::Allow),
            json!({"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"}}}));
        assert_eq!(permission_response(&Answer::Deny)["hookSpecificOutput"]["decision"],
            json!({"behavior":"deny","message":"Denied from DynamicNotch"}));
        assert_eq!(permission_response(&Answer::Dismiss), json!({}));
    }

    #[test]
    fn question_answer_echoes_and_validates() {
        let input = json!({"questions":[
            {"question":"Which DB?","header":"DB","multiSelect":false,"options":[{"label":"SQLite","description":""},{"label":"Postgres","description":""}]},
            {"question":"Extras?","header":"X","multiSelect":true,"options":[{"label":"Docs","description":""},{"label":"Tests","description":""}]}]});
        let a: Answer = serde_json::from_value(json!({"kind":"answers",
            "picks":{"Which DB?":["SQLite"],"Extras?":["Docs","Tests"]},"other":{}})).unwrap();
        let out = question_response(&input, &a).unwrap();
        let h = &out["hookSpecificOutput"];
        assert_eq!(h["permissionDecision"], "allow");
        assert_eq!(h["updatedInput"]["questions"], input["questions"]);
        assert_eq!(h["updatedInput"]["answers"], json!({"Which DB?":"SQLite","Extras?":"Docs, Tests"}));
        let bad: Answer = serde_json::from_value(json!({"kind":"answers","picks":{"Which DB?":["MySQL"]}})).unwrap();
        assert!(question_response(&input, &bad).is_err());
        let two: Answer = serde_json::from_value(json!({"kind":"answers","picks":{"Which DB?":["SQLite","Postgres"]}})).unwrap();
        assert!(question_response(&input, &two).is_err()); // single-select
        let other: Answer = serde_json::from_value(json!({"kind":"answers","picks":{},"other":{"Which DB?":"DuckDB"}})).unwrap();
        assert_eq!(question_response(&input, &other).unwrap()["hookSpecificOutput"]["updatedInput"]["answers"]["Which DB?"], "DuckDB");
        let huge: Answer = serde_json::from_value(json!({"kind":"answers","picks":{},"other":{"Which DB?":"x".repeat(501)}})).unwrap();
        assert!(question_response(&input, &huge).is_err());
        assert_eq!(question_response(&input, &Answer::Dismiss).unwrap(), json!({}));
    }

    #[test]
    fn other_text_combines_with_picks() {
        let input = json!({"questions":[
            {"question":"Which DB?","multiSelect":false,"options":[{"label":"SQLite"},{"label":"Postgres"}]},
            {"question":"Extras?","multiSelect":true,"options":[{"label":"Docs"},{"label":"Tests"}]}]});
        let run = |v: Value| question_response(&input, &serde_json::from_value(v).unwrap());
        let multi = run(json!({"kind":"answers","picks":{"Extras?":["Docs"]},"other":{"Extras?":"Lint"}})).unwrap();
        assert_eq!(multi["hookSpecificOutput"]["updatedInput"]["answers"]["Extras?"], "Docs, Lint");
        assert!(run(json!({"kind":"answers","picks":{"Which DB?":["SQLite"]},"other":{"Which DB?":"DuckDB"}})).is_err());
        let ws = run(json!({"kind":"answers","picks":{"Which DB?":["SQLite"]},"other":{"Which DB?":"   "}})).unwrap();
        assert_eq!(ws["hookSpecificOutput"]["updatedInput"]["answers"]["Which DB?"], "SQLite");
    }

    #[test]
    fn summary_picks_the_meaningful_field() {
        assert_eq!(summary("Bash", &json!({"command":"npm test"})), "npm test");
        assert_eq!(summary("Edit", &json!({"file_path":r"C:\a\b.rs"})), r"C:\a\b.rs");
        assert!(summary("Bash", &json!({"command":"é".repeat(500)})).chars().count() <= 161);
    }

    #[test]
    fn dropped_sender_dismisses_immediately() {
        let (tx, rx) = std::sync::mpsc::channel::<Answer>();
        drop(tx);
        let t = std::time::Instant::now();
        assert!(matches!(wait_answer(&rx, Duration::from_secs(15)), Answer::Dismiss));
        assert!(t.elapsed() < Duration::from_secs(1));
    }
}
