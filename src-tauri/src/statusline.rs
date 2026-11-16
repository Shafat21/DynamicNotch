//! `dynamic-notch --statusline`: Claude Code's statusLine command. Forwards the statusLine JSON to the
//! island's bridge (for plan usage) and runs the user's original statusLine command, printing its output.
//! Runs without Tauri, never writes the DB, never panics on bad input, always exits 0.

use serde_json::Value;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

pub const FLAG: &str = "--statusline";
const IDENT: &str = "com.dynamicnotch.app";
const MAX_STDIN: u64 = 1024 * 1024;
const POST_TIMEOUT: Duration = Duration::from_millis(300);
const CHAIN_TIMEOUT: Duration = Duration::from_secs(5); // cold PowerShell start can take seconds

/// The app's DB path, as Tauri's `app_data_dir` resolves it: Windows `%APPDATA%\<id>`,
/// Linux `$XDG_DATA_HOME/<id>` (absolute only) or `~/.local/share/<id>`.
pub fn db_path(windows: bool, appdata: Option<&str>, xdg_data_home: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    let dir = if windows {
        PathBuf::from(appdata.filter(|s| !s.is_empty())?)
    } else {
        match xdg_data_home.filter(|s| s.starts_with('/')) {
            Some(x) => PathBuf::from(x),
            None => Path::new(home.filter(|s| !s.is_empty())?).join(".local/share"),
        }
    };
    Some(dir.join(IDENT).join("dynamic_notch.db"))
}

/// Our statusLine `command` for the exe at `exe`. Windows paths get forward slashes (Git Bash eats backslashes).
/// Plain paths stay bare (works in every shell); others are quoted for the shell Claude Code will use.
pub fn statusline_command(exe: &str, windows: bool, git_bash: bool) -> String {
    let p = if windows { exe.replace('\\', "/") } else { exe.to_string() };
    let plain = p.chars().all(|c| c.is_ascii_alphanumeric() || "_./:-+".contains(c));
    let quoted = if plain {
        p
    } else if windows && !git_bash {
        format!("& '{}'", p.replace('\'', "''")) // PowerShell: call operator + literal string
    } else {
        format!("'{}'", p.replace('\'', r"'\''"))
    };
    format!("{quoted} {FLAG}")
}

/// Our exe's file names (installed / dev build); matched case-insensitively.
const EXE_NAMES: [&str; 2] = ["dynamic-notch.exe", "dynamic-notch"];

/// True if this statusLine value is a forwarder of ours: `<…/dynamic-notch[.exe]> --statusline` in any
/// quoting, from any install path (moved app, dev + installed builds sharing the DB). Never chain to ourselves.
pub fn is_ours(status_line: &Value) -> bool {
    let Some(prog) = status_line["command"].as_str().and_then(|c| c.trim().strip_suffix(FLAG)).filter(|p| p.ends_with(char::is_whitespace)) else {
        return false;
    };
    let prog = prog.trim().trim_start_matches("& ").trim_matches(|c| c == '\'' || c == '"');
    let base = prog.rsplit(['/', '\\']).next().unwrap_or("").to_ascii_lowercase();
    EXE_NAMES.contains(&base.as_str())
}

/// Any command ending in our flag (ours or a renamed copy): never stored over an existing saved original.
pub fn looks_like_forwarder(status_line: &Value) -> bool {
    status_line["command"].as_str().is_some_and(|c| c.trim_end().ends_with(FLAG))
}

/// The statusLine stdin as a bridge hook body, or None if it is not a JSON object.
pub fn hook_body(stdin: &[u8]) -> Option<Vec<u8>> {
    let mut v: Value = serde_json::from_slice(stdin).ok()?;
    v.as_object_mut()?.insert("hook_event_name".into(), "StatusLine".into());
    serde_json::to_vec(&v).ok()
}

/// The chained original command: a `{"type":"command","command":…}` value that is not ours.
pub fn chain_command(chain: &str) -> Option<String> {
    let v: Value = serde_json::from_str(chain).ok()?;
    let cmd = v["command"].as_str().filter(|c| !c.trim().is_empty())?;
    (v["type"] == "command" && !is_ours(&v)).then(|| cmd.to_string())
}

/// Git for Windows' bash, which Claude Code prefers for statusLine commands. None off Windows / not found.
pub fn git_bash() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    // shortcut: env override + standard install dirs + git on PATH; extend if a portable Git is missed
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let on_path = std::env::var_os("PATH")
        .and_then(|p| std::env::split_paths(&p).find(|d| d.join("git.exe").is_file()))
        .and_then(|d| d.parent().map(|p| p.join(r"bin\bash.exe")));
    [
        env("CLAUDE_CODE_GIT_BASH_PATH"),
        env("ProgramFiles").map(|p| p.join(r"Git\bin\bash.exe")),
        env("ProgramFiles(x86)").map(|p| p.join(r"Git\bin\bash.exe")),
        env("LOCALAPPDATA").map(|p| p.join(r"Programs\Git\bin\bash.exe")),
        on_path,
    ]
    .into_iter()
    .flatten()
    .find(|p| p.is_file())
}

/// (port, token, chain) from the app DB, opened read-only (never created or written).
pub(crate) fn read_db(path: &Path) -> Option<(u16, Option<String>, Option<String>)> {
    use rusqlite::{Connection, OpenFlags, OptionalExtension};
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).ok()?;
    let _ = conn.busy_timeout(Duration::from_millis(100));
    let get = |k: &str| -> Option<String> {
        conn.query_row("SELECT value FROM settings WHERE key = ?1", [k], |r| r.get(0)).optional().ok().flatten()
    };
    let port = get("bridge_port").and_then(|p| p.parse().ok()).unwrap_or(47863);
    Some((port, get("bridge_token"), get("statusline_chain")))
}

/// Fire-and-forget POST to the bridge. Raw TCP on purpose: no proxy env lookup, so the token stays local.
fn post(port: u16, token: &str, body: &[u8]) {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut s) = std::net::TcpStream::connect_timeout(&addr, POST_TIMEOUT) else { return };
    let _ = s.set_write_timeout(Some(POST_TIMEOUT));
    let head = format!(
        "POST /hook HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    // the server reads the whole body before handling, so no need to wait for its `{}` reply
    let _ = s.write_all(head.as_bytes()).and_then(|()| s.write_all(body));
}

fn shells(cmd: &str) -> Vec<Command> {
    let mut list = Vec::new();
    if cfg!(windows) {
        if let Some(bash) = git_bash() {
            list.push(Command::new(bash));
        }
        let mut ps = Command::new("powershell");
        ps.args(["-NoProfile", "-NonInteractive", "-Command", cmd]);
        list.push(ps);
    } else {
        list.push(Command::new("bash"));
        list.push(Command::new("sh"));
    }
    for c in &mut list {
        if c.get_args().next().is_none() {
            c.args(["-c", cmd]);
        }
    }
    list
}

/// Runs the user's original statusLine command with the same stdin and prints its stdout unchanged.
fn run_chain(cmd: &str, input: &[u8]) {
    if let Some(out) = chain_output(cmd, input) {
        let mut so = std::io::stdout().lock();
        let _ = so.write_all(&out).and_then(|()| so.flush());
    }
}

/// The chained command's stdout; None if no shell starts or it doesn't finish within CHAIN_TIMEOUT (then killed).
fn chain_output(cmd: &str, input: &[u8]) -> Option<Vec<u8>> {
    for mut c in shells(cmd) {
        c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: release exe has no console, avoid a flashing one
        }
        let Ok(mut child) = c.spawn() else { continue };
        if let Some(mut stdin) = child.stdin.take() {
            let data = input.to_vec();
            std::thread::spawn(move || {
                let _ = stdin.write_all(&data);
            });
        }
        let (tx, rx) = std::sync::mpsc::channel();
        if let Some(mut stdout) = child.stdout.take() {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = stdout.read_to_end(&mut buf);
                let _ = tx.send(buf);
            });
        }
        // stdout closes when the command exits; a reader stuck on an orphaned grandchild is simply abandoned
        let out = rx.recv_timeout(CHAIN_TIMEOUT).ok();
        if out.is_none() {
            let _ = child.kill();
            let _ = child.wait();
        }
        return out;
    }
    None
}

pub fn forward() {
    let mut input = Vec::new();
    let _ = std::io::stdin().take(MAX_STDIN).read_to_end(&mut input);
    let env = |k: &str| std::env::var(k).ok();
    let db = db_path(cfg!(windows), env("APPDATA").as_deref(), env("XDG_DATA_HOME").as_deref(), env("HOME").as_deref());
    let Some((port, token, chain)) = db.and_then(|p| read_db(&p)) else { return };
    let sender = match (hook_body(&input), token) {
        (Some(body), Some(token)) => Some(std::thread::spawn(move || post(port, &token, &body))),
        _ => None,
    };
    if let Some(cmd) = chain.as_deref().and_then(chain_command) {
        run_chain(&cmd, &input);
    }
    if let Some(h) = sender {
        let _ = h.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn db_path_matches_tauri_app_data_dir() {
        let p = db_path(true, Some(r"C:\Users\me\AppData\Roaming"), None, None).unwrap();
        assert_eq!(p, Path::new(r"C:\Users\me\AppData\Roaming").join(IDENT).join("dynamic_notch.db"));
        assert_eq!(db_path(true, None, None, Some("C:/x")), None);
        assert_eq!(db_path(false, None, Some("/xdg"), Some("/home/me")).unwrap(), Path::new("/xdg").join(IDENT).join("dynamic_notch.db"));
        let fallback = Path::new("/home/me").join(".local/share").join(IDENT).join("dynamic_notch.db");
        assert_eq!(db_path(false, None, Some("rel"), Some("/home/me")).unwrap(), fallback); // relative XDG ignored
        assert_eq!(db_path(false, None, Some(""), Some("/home/me")).unwrap(), fallback);
        assert_eq!(db_path(false, None, None, None), None);
    }

    #[test]
    fn command_is_quoted_per_shell() {
        assert_eq!(statusline_command(r"C:\Tools\dn.exe", true, true), "C:/Tools/dn.exe --statusline");
        assert_eq!(statusline_command(r"C:\Program Files\Dynamic Notch Island\dynamic-notch.exe", true, true),
            "'C:/Program Files/Dynamic Notch Island/dynamic-notch.exe' --statusline");
        assert_eq!(statusline_command(r"C:\Program Files\Dyn's\dn.exe", true, false), "& 'C:/Program Files/Dyn''s/dn.exe' --statusline");
        assert_eq!(statusline_command("/opt/dn/dn", false, false), "/opt/dn/dn --statusline");
        assert_eq!(statusline_command("/home/a b/it's", false, false), r"'/home/a b/it'\''s' --statusline");
    }

    #[test]
    fn ours_is_detected_by_exe_name_and_flag() {
        let cmd = |c: &str| json!({"type": "command", "command": c});
        let installed = r"C:\Program Files\Dynamic Notch Island\dynamic-notch.exe";
        for c in [statusline_command(installed, true, true), statusline_command(installed, true, false)] {
            assert!(is_ours(&cmd(&c)), "{c}");
        }
        // moved app / dev build sharing the DB: other paths, same exe name
        assert!(is_ours(&cmd(&statusline_command(r"D:\Apps\DN\Dynamic-Notch.EXE", true, false))));
        assert!(is_ours(&cmd(&statusline_command(r"E:\src\DynamicNotch\src-tauri\target\debug\dynamic-notch.exe", true, true))));
        assert!(is_ours(&cmd("/opt/dn/dynamic-notch --statusline  ")));
        assert!(!is_ours(&cmd("/opt/other --statusline"))); // a user tool with the same flag
        assert!(!is_ours(&cmd("my-status --statusline")));
        assert!(!is_ours(&cmd("/opt/dynamic-notch--statusline")));
        assert!(!is_ours(&cmd("powershell -File C:/caveman-statusline.ps1")));
        assert!(!is_ours(&json!("/opt/dynamic-notch --statusline")));
        assert!(!is_ours(&Value::Null));
        assert!(looks_like_forwarder(&cmd("my-status --statusline")) && !looks_like_forwarder(&cmd("x")));
    }

    #[test]
    fn chain_output_returns_stdout_and_kills_slow_commands() {
        assert_eq!(String::from_utf8_lossy(&chain_output("echo hi", b"{}").unwrap()).trim(), "hi");
        let t = std::time::Instant::now();
        assert_eq!(chain_output("sleep 10", b"{}"), None);
        assert!(t.elapsed() < Duration::from_secs(9));
    }

    #[test]
    fn hook_body_tags_the_event_and_rejects_non_objects() {
        let body = hook_body(br#"{"session_id":"s","rate_limits":{"five_hour":{"used_percentage":5,"resets_at":9}}}"#).unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["hook_event_name"], "StatusLine");
        assert_eq!(v["rate_limits"]["five_hour"]["used_percentage"], 5);
        assert!(hook_body(b"[1]").is_none());
        assert!(hook_body(b"not json").is_none());
    }

    #[test]
    fn chain_command_skips_ours_and_non_commands() {
        assert_eq!(chain_command(r#"{"type":"command","command":"powershell -File C:/s.ps1","padding":0}"#).as_deref(), Some("powershell -File C:/s.ps1"));
        let ours = json!({"type": "command", "command": statusline_command(r"C:\Old Place\dynamic-notch.exe", true, true)});
        assert_eq!(chain_command(&ours.to_string()), None);
        assert_eq!(chain_command(r#"{"type":"other","command":"x"}"#), None);
        assert_eq!(chain_command(""), None);
        assert_eq!(chain_command(r#"{"type":"command","command":"  "}"#), None);
    }
}
