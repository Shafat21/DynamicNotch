//! Reply to a coding agent from the island: the agent's window (Zed, VS Code, a terminal) is brought forward and the
//! text pasted into it with Enter, as if typed there. The agent is found by its session's working folder (Claude Code
//! reports over HTTP, so the hook carries no process id), then its parent processes up to the one owning a window.
//! An image is saved to a temp file and its path added to the text: the agents read image files by path.
//! Windows only. The paste goes to whatever input has focus in that window: the UI says to keep the agent's box focused.

/// Process names an agent's CLI runs as, or a path segment of its script when it runs under node / bun.
fn names(agent: &str) -> &'static [&'static str] {
    match agent {
        "claude" => &["claude", "claude-code"],
        "codex" => &["codex"],
        "gemini" => &["gemini", "gemini-cli"],
        "cursor" => &["cursor-agent"],
        "copilot" => &["copilot"],
        "opencode" => &["opencode"],
        "commandcode" => &["command-code"], // its `cmd` command would also match cmd.exe
        "antigravity" => &["antigravity"],
        _ => &[],
    }
}

/// Is this process the agent's CLI? `exe`: file name ("claude.exe", "node.exe"); `cmd`: its command line.
pub fn is_agent(agent: &str, exe: &str, cmd: &[String]) -> bool {
    let stem = exe.to_lowercase();
    let stem = stem.trim_end_matches(".exe");
    let want = names(agent);
    want.contains(&stem)
        || (matches!(stem, "node" | "bun" | "deno")
            && cmd.iter().skip(1).any(|a| a.to_lowercase().replace('\\', "/").split('/').any(|seg| want.contains(&seg.trim_end_matches(".js")))))
}

/// One line for the agent's prompt: no control characters, newlines as spaces (Enter would send early), capped.
pub fn clean(text: &str) -> String {
    let t: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    t.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(4000).collect()
}

/// The text to send: the message, then the image path (if any).
pub fn message(text: &str, image: Option<&str>) -> String {
    let text = clean(text);
    match image {
        Some(p) if text.is_empty() => format!("Look at this image: {p}"),
        Some(p) => format!("{text} (image: {p})"),
        None => text,
    }
}

/// Standard base64 (the webview sends a pasted image as a data URL) to bytes; None on bad input.
pub fn decode_base64(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let s = s.trim_end_matches('=').as_bytes();
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for &c in s {
        acc = (acc << 6) | val(c)? as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// Of an app's windows (titles, front to back), the one for `project`: its title names the project (Zed, VS Code
/// and terminals put the folder name in it), else the frontmost.
pub fn pick_window(titles: &[String], project: &str) -> Option<usize> {
    let p = project.to_lowercase();
    let named = if p.is_empty() { None } else { titles.iter().position(|t| t.to_lowercase().contains(&p)) };
    named.or((!titles.is_empty()).then_some(0))
}

#[cfg(windows)]
pub use os::{save_image, send};

#[cfg(windows)]
mod os {
    use std::path::Path;
    use std::time::Duration;
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_MENU, VK_RETURN,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetForegroundWindow, GetWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
        SetForegroundWindow, ShowWindow, GW_OWNER, SW_RESTORE,
    };

    fn norm(p: &Path) -> String {
        p.to_string_lossy().trim_end_matches(['\\', '/']).to_lowercase()
    }

    /// Visible top-level windows with a title, front to back: (window, owning pid, title).
    fn windows() -> Vec<(HWND, u32, String)> {
        unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> BOOL {
            let out = unsafe { &mut *(lp.0 as *mut Vec<(HWND, u32, String)>) };
            unsafe {
                let n = GetWindowTextLengthW(hwnd);
                if n > 0 && IsWindowVisible(hwnd).as_bool() && GetWindow(hwnd, GW_OWNER).is_err() {
                    let mut buf = vec![0u16; n as usize + 1];
                    let len = GetWindowTextW(hwnd, &mut buf) as usize;
                    let mut pid = 0;
                    GetWindowThreadProcessId(hwnd, Some(&mut pid));
                    out.push((hwnd, pid, String::from_utf16_lossy(&buf[..len])));
                }
            }
            true.into()
        }
        let mut out = Vec::new();
        let _ = unsafe { EnumWindows(Some(each), LPARAM(&mut out as *mut _ as isize)) };
        out
    }

    /// The window hosting a session's agent: the agent CLI running in `cwd`, then up its parents (claude.exe → Zed,
    /// or → shell → Windows Terminal) to the first process that owns a window.
    fn host_window(agent: &str, cwd: &str, project: &str) -> Result<HWND, String> {
        let mut sys = System::new();
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always).with_cwd(UpdateKind::Always));
        let want = norm(Path::new(cwd));
        let agent_proc = sys
            .processes()
            .values()
            .filter(|p| p.cwd().is_some_and(|c| norm(c) == want))
            .filter(|p| super::is_agent(agent, &p.name().to_string_lossy(), &p.cmd().iter().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>()))
            .max_by_key(|p| p.start_time())
            .ok_or("Couldn't find this agent running. Is the session still open?")?;
        let wins = windows();
        let mut pid: Option<Pid> = Some(agent_proc.pid());
        for _ in 0..8 {
            let Some(p) = pid else { break };
            let mine: Vec<&(HWND, u32, String)> = wins.iter().filter(|w| w.1 == p.as_u32()).collect();
            let titles: Vec<String> = mine.iter().map(|w| w.2.clone()).collect();
            if let Some(i) = super::pick_window(&titles, project) {
                return Ok(mine[i].0);
            }
            pid = sys.process(p).and_then(|x| x.parent());
        }
        Err("Couldn't find the window this agent runs in".into())
    }

    fn keys(seq: &[(VIRTUAL_KEY, bool)]) {
        let inputs: Vec<INPUT> = seq
            .iter()
            .map(|&(vk, down)| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, dwFlags: if down { KEYBD_EVENT_FLAGS(0) } else { KEYEVENTF_KEYUP }, ..Default::default() } },
            })
            .collect();
        unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    }

    /// Bring the agent's window forward and paste `text` + Enter into it (into its focused input box).
    pub fn send(agent: &str, cwd: &str, project: &str, text: &str) -> Result<(), String> {
        let hwnd = host_window(agent, cwd, project)?;
        crate::clipboard::write(text)?;
        unsafe {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            keys(&[(VK_MENU, true), (VK_MENU, false)]); // a key press lifts Windows' "background apps can't take focus" lock
            let _ = SetForegroundWindow(hwnd);
        }
        for _ in 0..20 {
            if unsafe { GetForegroundWindow() } == hwnd {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        if unsafe { GetForegroundWindow() } != hwnd {
            return Err("Couldn't bring the agent's window forward. Your message is on the clipboard: paste it there.".into());
        }
        std::thread::sleep(Duration::from_millis(150)); // let the window give focus back to its input box
        keys(&[(VK_CONTROL, true), (VIRTUAL_KEY(b'V' as u16), true), (VIRTUAL_KEY(b'V' as u16), false), (VK_CONTROL, false)]);
        std::thread::sleep(Duration::from_millis(300)); // a long paste takes a moment to land
        keys(&[(VK_RETURN, true), (VK_RETURN, false)]);
        Ok(())
    }

    /// Live, read-only: which window a reply to this project's Claude session would go to.
    /// `cargo test --lib live_host_window -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_host_window() {
        let cwd = std::env::current_dir().unwrap().parent().unwrap().to_string_lossy().into_owned();
        let hwnd = host_window("claude", &cwd, "DynamicNotch").unwrap();
        let w = windows().into_iter().find(|w| w.0 == hwnd).unwrap();
        println!("reply would go to: {:?} (pid {})", w.2, w.1);
    }

    /// Saves a pasted image (PNG bytes) under %TEMP%/dynamic-notch/replies and returns its path.
    pub fn save_image(png: &[u8]) -> Result<String, String> {
        let dir = std::env::temp_dir().join("dynamic-notch").join("replies");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let path = dir.join(format!("image-{ms}.png"));
        std::fs::write(&path, png).map_err(|e| e.to_string())?;
        Ok(path.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_agent_processes_by_name_or_script() {
        assert!(is_agent("claude", "claude.exe", &[]));
        assert!(is_agent("claude", "node.exe", &["node".into(), r"C:\Users\x\AppData\Roaming\npm\node_modules\@anthropic-ai\claude-code\cli.js".into()]));
        assert!(is_agent("codex", "codex.exe", &[]));
        assert!(!is_agent("claude", "node.exe", &["node".into(), "server.js".into()]));
        assert!(!is_agent("claude", "chrome.exe", &[]));
        assert!(!is_agent("unknown", "claude.exe", &[]));
    }

    #[test]
    fn message_is_one_clean_line() {
        assert_eq!(message("fix the\nbug\x1b[31m  now", None), "fix the bug [31m now");
        assert_eq!(message("", Some(r"C:\t\a.png")), r"Look at this image: C:\t\a.png");
        assert_eq!(message("why?", Some(r"C:\t\a.png")), r"why? (image: C:\t\a.png)");
    }

    #[test]
    fn base64_round_trips_with_the_encoder() {
        for bytes in [&b""[..], b"f", b"fo", b"foo", b"\x89PNG\r\n\x1a\n\x00\xff"] {
            assert_eq!(decode_base64(&crate::launcher::base64(bytes)).as_deref(), Some(bytes));
        }
        assert_eq!(decode_base64("ab$c"), None);
    }

    #[test]
    fn picks_the_window_named_for_the_project() {
        let t = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(pick_window(&t(&["webshop — Zed", "DynamicNotch — Zed"]), "DynamicNotch"), Some(1));
        assert_eq!(pick_window(&t(&["Windows PowerShell", "Zed"]), "webshop"), Some(0)); // no match: the frontmost
        assert_eq!(pick_window(&t(&[]), "x"), None);
    }
}
