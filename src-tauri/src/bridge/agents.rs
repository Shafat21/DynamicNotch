//! Coding agents other than Claude Code. They can only run hook *commands*, so their config points at
//! our exe in relay mode (`dynamic-notch --hook --agent <id> <Event>`, see `crate::hook`), which forwards
//! to the same bridge Claude Code's http hooks reach. Claude Code stays in `install.rs` (http + statusLine).
//! Hook shapes per agent follow coucou-main's `windows/src-tauri/src/agents.rs`.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const CLAUDE: &str = "claude";

/// The shell an agent runs hook commands through on Windows (Linux: always `sh`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Shell {
    Sh,
    PowerShell,
    /// `cmd /C` or a process started directly.
    Cmd,
}

pub struct Agent {
    pub id: &'static str,
    pub label: &'static str,
    /// Config file under the home folder.
    path: &'static [&'static str],
    /// (config event name, timeout in the unit the agent reads). The event also goes on the command line.
    events: &'static [(&'static str, u64)],
    /// The island answers this agent's permission requests (relay prints its reply format).
    pub approvals: bool,
    /// The file is ours alone (removed once our last entry goes).
    own_file: bool,
}

pub const AGENTS: &[Agent] = &[
    // Claude-style groups, timeouts in s. PermissionRequest waits for the island (bridge answers within 15 s).
    Agent {
        id: "codex",
        label: "Codex",
        path: &[".codex", "hooks.json"],
        events: &[("SessionStart", 10), ("UserPromptSubmit", 10), ("PermissionRequest", 25), ("Stop", 10), ("SessionEnd", 3)],
        approvals: true,
        own_file: false,
    },
    // Claude-style groups with a `*` matcher, timeouts in ms; runs hooks with `powershell -Command` on Windows.
    // AfterModel is left out: it fires on every response chunk.
    Agent {
        id: "gemini",
        label: "Gemini CLI",
        path: &[".gemini", "settings.json"],
        events: &[("SessionStart", 10_000), ("BeforeAgent", 5_000), ("AfterAgent", 5_000), ("SessionEnd", 10_000)],
        approvals: false,
        own_file: false,
    },
    // `{"version": 1, "hooks": {"<event>": [{"command"}]}}`, camelCase events, no timeouts.
    Agent {
        id: "cursor",
        label: "Cursor",
        path: &[".cursor", "hooks.json"],
        events: &[("sessionStart", 0), ("beforeSubmitPrompt", 0), ("stop", 0), ("sessionEnd", 0)],
        approvals: false,
        own_file: false,
    },
    // A file of our own: entries `{"type":"command","bash","powershell"?,"timeoutSec"}`. The event is not in the payload.
    Agent {
        id: "copilot",
        label: "Copilot CLI",
        path: &[".copilot", "hooks", "dynamic-notch.json"],
        events: &[("sessionStart", 10), ("userPromptSubmitted", 10), ("permissionRequest", 25), ("agentStop", 10), ("sessionEnd", 3)],
        approvals: true,
        own_file: true,
    },
];

pub fn find(id: &str) -> Option<&'static Agent> {
    AGENTS.iter().find(|a| a.id == id)
}

/// Agents whose permission requests get an island card: Claude Code and the relayed ones that take a decision.
pub fn takes_decisions(id: &str) -> bool {
    id == CLAUDE || find(id).is_some_and(|a| a.approvals)
}

/// The agent a hook event is from: its `agent` tag if valid (`^[a-z0-9-]{1,24}$`), else Claude Code.
pub fn agent_of(ev: &Value) -> &str {
    ev["agent"]
        .as_str()
        .filter(|a| (1..=24).contains(&a.len()) && a.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
        .unwrap_or(CLAUDE)
}

/// `exe` followed by `args` as one command line for `shell`. Windows paths get forward slashes for Git Bash.
pub fn command(exe: &str, windows: bool, shell: Shell, args: &str) -> String {
    let plain = |p: &str| p.chars().all(|c| c.is_ascii_alphanumeric() || "\\/:._-".contains(c));
    let sh_quote = |p: &str| format!("'{}'", p.replace('\'', r"'\''"));
    let exe = match (windows, shell) {
        (false, _) if plain(exe) => exe.to_string(),
        (false, _) => sh_quote(exe),
        (true, Shell::Sh) => sh_quote(&exe.replace('\\', "/")),
        (true, Shell::PowerShell) => format!("& '{}'", exe.replace('\'', "''")), // a quoted string alone is not run
        (true, Shell::Cmd) if plain(exe) => exe.to_string(),
        (true, Shell::Cmd) => format!("\"{exe}\""), // a Windows path cannot contain `"`
    };
    format!("{exe} {args}")
}

/// A command line we wrote for `agent`, from any install path (moved app, dev build).
fn is_our_command(c: Option<&Value>, agent: &str) -> bool {
    c.and_then(Value::as_str)
        .is_some_and(|c| c.to_ascii_lowercase().contains("dynamic-notch") && c.contains(&format!("{} --agent {agent} ", crate::hook::FLAG)))
}

/// A hook entry of ours: `command` (Codex, Gemini, Cursor) or `bash` / `powershell` (Copilot).
fn is_our_entry(e: &Value, agent: &str) -> bool {
    ["command", "bash", "powershell"].iter().any(|k| is_our_command(e.get(*k), agent))
}

impl Agent {
    pub fn path(&self, home: &Path) -> PathBuf {
        self.path.iter().fold(home.to_path_buf(), |p, s| p.join(s))
    }

    /// The list entry for `event` pointing at `exe`.
    fn entry(&self, event: &str, timeout: u64, exe: &str, windows: bool) -> Value {
        let args = format!("{} --agent {} {event}", crate::hook::FLAG, self.id);
        let cmd = |shell| command(exe, windows, shell, &args);
        match self.id {
            "codex" => {
                let mut h = json!({"type": "command", "command": cmd(Shell::Cmd), "timeout": timeout});
                if event == "PermissionRequest" {
                    h["statusMessage"] = json!("Waiting for your answer in Dynamic Notch Island");
                }
                json!({"hooks": [h]})
            }
            "gemini" => json!({"matcher": "*", "hooks": [{"type": "command", "command": cmd(Shell::PowerShell), "timeout": timeout}]}),
            "cursor" => json!({"command": cmd(Shell::Cmd)}),
            _ => {
                let mut e = json!({"type": "command", "bash": cmd(Shell::Sh), "timeoutSec": timeout});
                if windows {
                    e["powershell"] = json!(cmd(Shell::PowerShell));
                }
                e
            }
        }
    }

    /// Our entries for every event, replacing earlier ones of ours; nobody else's are touched.
    /// Unexpected layout → error, nothing changed.
    pub fn install(&self, root: &mut Value, exe: &str, windows: bool) -> Result<(), String> {
        self.uninstall(root)?;
        let obj = root.as_object_mut().ok_or("not a JSON object")?;
        let hooks = obj.entry("hooks").or_insert_with(|| json!({})).as_object_mut().expect("checked by uninstall");
        for (event, timeout) in self.events {
            let list = hooks.entry(*event).or_insert_with(|| json!([])).as_array_mut().expect("checked by uninstall");
            list.push(self.entry(event, *timeout, exe, windows));
        }
        if matches!(self.id, "cursor" | "copilot") {
            obj.entry("version").or_insert(json!(1));
        }
        Ok(())
    }

    /// Removes our entries (flat, or inside a group's `hooks`); a group / event / `hooks` we emptied goes.
    pub fn uninstall(&self, root: &mut Value) -> Result<(), String> {
        let obj = root.as_object_mut().ok_or("not a JSON object")?;
        let Some(hooks) = obj.get_mut("hooks") else { return Ok(()) };
        let hooks = hooks.as_object_mut().ok_or("\"hooks\" is not an object")?;
        if let Some((e, _)) = hooks.iter().find(|(_, v)| !v.is_array()) {
            return Err(format!("\"hooks\".\"{e}\" is not a list"));
        }
        let mut emptied = Vec::new();
        for (event, list) in hooks.iter_mut() {
            let list = list.as_array_mut().expect("checked");
            let n = list.len();
            list.retain_mut(|g| {
                if is_our_entry(g, self.id) {
                    return false;
                }
                let Some(inner) = g.get_mut("hooks").and_then(Value::as_array_mut) else { return true };
                let had = inner.len();
                inner.retain(|h| !is_our_entry(h, self.id));
                !(inner.is_empty() && had > 0)
            });
            if list.is_empty() && n > 0 {
                emptied.push(event.clone());
            }
        }
        for e in &emptied {
            hooks.shift_remove(e);
        }
        if hooks.is_empty() && !emptied.is_empty() {
            obj.shift_remove("hooks");
        }
        Ok(())
    }

    pub fn installed(&self, root: &Value) -> bool {
        let ours = |g: &Value| is_our_entry(g, self.id) || g["hooks"].as_array().is_some_and(|i| i.iter().any(|h| is_our_entry(h, self.id)));
        root["hooks"].as_object().is_some_and(|h| h.values().filter_map(Value::as_array).flatten().any(ours))
    }

    pub fn installed_at(&self, home: &Path) -> bool {
        let text = std::fs::read_to_string(self.path(home)).ok();
        text.and_then(|t| serde_json::from_str::<Value>(&t).ok()).is_some_and(|v| self.installed(&v))
    }

    /// Our own file with nothing of anyone else's left in it.
    pub fn file_is_empty(&self, root: &Value) -> bool {
        self.own_file && root.as_object().is_some_and(|o| o.keys().all(|k| k == "version"))
    }

    pub fn event_names(&self) -> String {
        self.events.iter().map(|(e, _)| *e).collect::<Vec<_>>().join(", ")
    }
}

/// `[{id, label, connected}]` for every agent, Claude Code first.
pub fn status(home: Option<&Path>, claude: bool) -> Value {
    let mut list = vec![json!({"id": CLAUDE, "label": "Claude Code", "connected": claude})];
    list.extend(AGENTS.iter().map(|a| json!({"id": a.id, "label": a.label, "connected": home.is_some_and(|h| a.installed_at(h))})));
    Value::Array(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN: &str = r"C:\Program Files\Dynamic Notch Island\dynamic-notch.exe";

    #[test]
    fn commands_are_quoted_per_shell() {
        assert_eq!(command(WIN, true, Shell::Cmd, "--hook --agent codex Stop"), format!("\"{WIN}\" --hook --agent codex Stop"));
        assert_eq!(command(WIN, true, Shell::PowerShell, "x"), format!("& '{WIN}' x"));
        assert_eq!(command(WIN, true, Shell::Sh, "x"), "'C:/Program Files/Dynamic Notch Island/dynamic-notch.exe' x");
        assert_eq!(command(r"C:\dn\dynamic-notch.exe", true, Shell::Cmd, "x"), r"C:\dn\dynamic-notch.exe x");
        assert_eq!(command("/opt/dn/dynamic-notch", false, Shell::PowerShell, "x"), "/opt/dn/dynamic-notch x");
        assert_eq!(command("/home/a b/it's", false, Shell::Sh, "x"), r"'/home/a b/it'\''s' x");
    }

    #[test]
    fn every_agent_installs_idempotently_and_uninstalls_to_the_original() {
        for a in AGENTS {
            let user = json!({"command": "notify-send hi", "type": "command"});
            let original = json!({"theme": "dark", "hooks": {a.events[0].0: [{"hooks": [user.clone()]}], "Other": [user.clone()]}});
            let mut v = original.clone();
            a.install(&mut v, WIN, true).unwrap();
            a.install(&mut v, WIN, true).unwrap();
            assert!(a.installed(&v), "{}", a.id);
            for (e, _) in a.events {
                let ours = v["hooks"][*e].as_array().unwrap().iter().filter(|g| !g["hooks"][0]["command"].as_str().is_some_and(|c| c.contains("notify"))).count();
                assert_eq!(ours, 1, "{} {e}: one entry of ours", a.id);
            }
            a.uninstall(&mut v).unwrap();
            assert!(!a.installed(&v));
            v.as_object_mut().unwrap().remove("version");
            assert_eq!(v, original, "{}", a.id);
        }
    }

    #[test]
    fn shapes_match_what_each_agent_reads() {
        let mut v = json!({});
        find("codex").unwrap().install(&mut v, WIN, true).unwrap();
        let perm = &v["hooks"]["PermissionRequest"][0]["hooks"][0];
        assert_eq!(perm["timeout"], 25);
        assert!(perm["statusMessage"].is_string());
        assert!(perm["command"].as_str().unwrap().ends_with("--hook --agent codex PermissionRequest"));

        let mut v = json!({});
        find("gemini").unwrap().install(&mut v, WIN, true).unwrap();
        assert_eq!(v["hooks"]["BeforeAgent"][0]["matcher"], "*");
        assert!(v["hooks"]["BeforeAgent"][0]["hooks"][0]["command"].as_str().unwrap().starts_with("& '"));

        let mut v = json!({});
        find("cursor").unwrap().install(&mut v, WIN, true).unwrap();
        assert_eq!(v["version"], 1);
        assert!(v["hooks"]["stop"][0]["command"].as_str().unwrap().ends_with("--agent cursor stop"));

        let a = find("copilot").unwrap();
        let mut v = json!({});
        a.install(&mut v, WIN, true).unwrap();
        let e = &v["hooks"]["permissionRequest"][0];
        assert_eq!(e["timeoutSec"], 25);
        assert!(e["bash"].as_str().unwrap().starts_with("'C:/") && e["powershell"].as_str().unwrap().starts_with("& '"));
        a.uninstall(&mut v).unwrap();
        assert!(a.file_is_empty(&v));
        assert!(!find("codex").unwrap().file_is_empty(&json!({})));
    }

    #[test]
    fn moved_app_entries_are_replaced_and_other_agents_left_alone() {
        let codex = find("codex").unwrap();
        let mut v = json!({});
        codex.install(&mut v, r"D:\old\dynamic-notch.exe", true).unwrap();
        find("gemini").unwrap().install(&mut v, WIN, true).unwrap(); // same file layout, other agent
        codex.install(&mut v, WIN, true).unwrap();
        assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 1);
        assert!(v["hooks"]["Stop"][0]["hooks"][0]["command"].as_str().unwrap().contains("Program Files"));
        codex.uninstall(&mut v).unwrap();
        assert!(find("gemini").unwrap().installed(&v));
        // a user tool with the same flags but another exe is not ours
        assert!(!is_our_command(Some(&json!("other-tool --hook --agent codex Stop")), "codex"));
    }

    #[test]
    fn unexpected_layout_is_refused_untouched() {
        let a = find("codex").unwrap();
        for bad in [json!({"hooks": {"Stop": "oops"}}), json!({"hooks": []}), json!([1])] {
            let mut v = bad.clone();
            assert!(a.install(&mut v, WIN, true).is_err());
            assert_eq!(v, bad);
        }
    }

    #[test]
    fn agent_tag_is_validated() {
        assert_eq!(agent_of(&json!({"agent": "codex"})), "codex");
        assert_eq!(agent_of(&json!({})), CLAUDE);
        assert_eq!(agent_of(&json!({"agent": "Bad Name"})), CLAUDE);
        assert_eq!(agent_of(&json!({"agent": "x".repeat(25)})), CLAUDE);
        assert!(takes_decisions("claude") && takes_decisions("codex") && takes_decisions("copilot"));
        assert!(!takes_decisions("gemini") && !takes_decisions("cursor") && !takes_decisions("my-tool"));
    }
}
