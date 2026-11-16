//! Windows: put the text on the clipboard (kept out of history), send Ctrl+V, then put the previous
//! clipboard text back after ~800 ms if the clipboard still holds ours.
//! An elevated foreground window (while we are not) blocks injected keys (UIPI) without an error:
//! then we only copy and leave our text on the clipboard.
//! Before Ctrl+V we wait (up to 1 s) for the dictation keys to come up, so the paste is a plain Ctrl+V
//! and not Ctrl+Win+V; keys still held after that get key-ups.
//!
//! Portions adapted from Coucou (https://github.com/Louis-CFM/coucou), © 2026 Louis Raillé, MIT License.

use std::time::{Duration, Instant};
use windows::{
    core::PWSTR,
    Win32::{
        Foundation::{CloseHandle, HANDLE, HGLOBAL},
        Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
        System::{
            DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard},
            Memory::{GlobalLock, GlobalSize, GlobalUnlock},
            Threading::{GetCurrentProcess, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION},
        },
        UI::{
            Input::KeyboardAndMouse::GetAsyncKeyState,
            WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
        },
    },
};

const CF_UNICODETEXT: u32 = 13;
const VK_CONTROL: u16 = 0x11;
const VK_V: u16 = 0x56;
/// LShift, RShift, LAlt, RAlt, LWin, RWin: released before Ctrl+V so it doesn't arrive as Ctrl+Shift+V etc.
const HELD: [u16; 6] = [0xA0, 0xA1, 0xA4, 0xA5, 0x5B, 0x5C];
/// LCtrl, RCtrl: waited for too (part of the default Ctrl+Win), but not force-released.
const CTRLS: [u16; 2] = [0xA2, 0xA3];

fn down(vk: u16) -> bool {
    unsafe { GetAsyncKeyState(vk as i32) as u16 & 0x8000 != 0 }
}

/// Wait up to a second for the modifiers to come up (hands-free ends with a key press, so the
/// dictation keys can still be held when the text is ready).
fn wait_keys_released() {
    let until = Instant::now() + Duration::from_secs(1);
    while HELD.iter().chain(&CTRLS).any(|&vk| down(vk)) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(15));
    }
}

/// Parts of the desktop itself, never a place to dictate into.
fn is_shell(stem: &str) -> bool {
    matches!(stem, "explorer" | "applicationframehost" | "textinputhost" | "shellexperiencehost" | "searchhost" | "startmenuexperiencehost" | "lockapp")
}

/// The program in front, e.g. "Notepad"; None for us, the desktop or when it can't be read.
pub fn foreground_app() -> Option<String> {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
        if pid == 0 || pid == std::process::id() {
            return None;
        }
        let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(p);
        if !ok {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        let stem = std::path::Path::new(&path).file_stem()?.to_string_lossy().into_owned();
        if is_shell(&stem.to_lowercase()) {
            return None;
        }
        let mut c = stem.chars();
        c.next().map(|f| f.to_uppercase().chain(c).collect())
    }
}
/// Unassigned key tapped before releasing Alt/Win, so the lone key-up doesn't open a menu or Start.
const MASK_KEY: u16 = 0xE8;

/// Current clipboard text; None when empty, not text, busy, or marked private (never re-put a password).
fn read() -> Option<String> {
    let mut opened = false;
    for _ in 0..10 {
        if unsafe { OpenClipboard(None) }.is_ok() {
            opened = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if !opened {
        return None;
    }
    let text = (|| unsafe {
        if crate::clipboard::os::private() {
            return None;
        }
        let g = HGLOBAL(GetClipboardData(CF_UNICODETEXT).ok()?.0);
        let p = GlobalLock(g) as *const u16;
        if p.is_null() {
            return None;
        }
        let max = GlobalSize(g) / 2;
        let n = (0..max).find(|&i| *p.add(i) == 0).unwrap_or(max);
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
        let _ = GlobalUnlock(g);
        Some(s)
    })();
    unsafe {
        let _ = CloseClipboard();
    }
    text
}

/// Token elevation of a process; None when the token can't be opened or read.
fn elevated(process: HANDLE) -> Option<bool> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
        let mut e = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let r = GetTokenInformation(token, TokenElevation, Some(&mut e as *mut _ as *mut _), std::mem::size_of::<TOKEN_ELEVATION>() as u32, &mut len);
        let _ = CloseHandle(token);
        r.ok().map(|_| e.TokenIsElevated != 0)
    }
}

/// True when the foreground window would drop our keystrokes: it runs elevated (or we can't tell) and we don't.
fn foreground_blocks_input() -> bool {
    if elevated(unsafe { GetCurrentProcess() }) == Some(true) {
        return false;
    }
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
        let Ok(p) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return true };
        let r = elevated(p);
        let _ = CloseHandle(p);
        r != Some(false)
    }
}

/// Ok(true) = pasted; Ok(false) = only copied (the text stays on the clipboard).
pub fn paste(text: &str) -> Result<bool, String> {
    if foreground_blocks_input() {
        super::copy(text)?;
        return Ok(false);
    }
    wait_keys_released();
    let prev = read();
    crate::clipboard::write(text)?;
    std::thread::sleep(Duration::from_millis(40)); // let the clipboard change be announced before the app reads it
    let held: Vec<u16> = HELD.into_iter().filter(|&vk| down(vk)).collect();
    let mut keys = Vec::new();
    if !held.is_empty() {
        keys.extend([(MASK_KEY, false), (MASK_KEY, true)]);
        keys.extend(held.iter().map(|&vk| (vk, true)));
    }
    keys.extend([(VK_CONTROL, false), (VK_V, false), (VK_V, true), (VK_CONTROL, true)]);
    super::hotkey_os::send_keys(&keys)?;
    let ours = text.to_string();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(800)); // slow targets read the clipboard well after Ctrl+V
        if let Some(p) = prev.filter(|_| read().as_deref() == Some(ours.as_str())) {
            let _ = crate::clipboard::write(&p);
        }
    });
    Ok(true)
}

#[cfg(test)]
mod tests {
    /// Real paste into a real text box: a window of ours with an EDIT control is brought to the front,
    /// `paste` runs from another thread, and the box's text is read back. Touches the clipboard and the
    /// keyboard, so it is ignored by default: `cargo test --lib paste_into_an_edit_box -- --ignored`
    #[test]
    #[ignore]
    fn paste_into_an_edit_box() {
        use windows::core::w;
        use windows::Win32::UI::WindowsAndMessaging::*;
        let before = super::read();
        unsafe {
            let hwnd = CreateWindowExW(Default::default(), w!("EDIT"), w!(""), WS_OVERLAPPEDWINDOW | WS_VISIBLE, 200, 200, 400, 120, None, None, None, None).unwrap();
            let _ = SetForegroundWindow(hwnd);
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(hwnd));
            let front = super::GetForegroundWindow() == hwnd;
            let worker = std::thread::spawn(|| super::paste("Pasted by dictation ✓ 123"));
            let until = std::time::Instant::now() + std::time::Duration::from_millis(1500);
            let mut msg = MSG::default();
            while std::time::Instant::now() < until {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let pasted = worker.join().unwrap();
            let mut buf = [0u16; 256];
            let n = GetWindowTextW(hwnd, &mut buf);
            let text = String::from_utf16_lossy(&buf[..n as usize]);
            let _ = DestroyWindow(hwnd);
            println!("foreground={front} paste={pasted:?} box={text:?} clipboard before={:?} after={:?}", before.as_deref().map(|s| s.chars().take(20).collect::<String>()), super::read().map(|s| s.chars().take(20).collect::<String>()));
            assert_eq!(pasted, Ok(true));
            assert_eq!(text, "Pasted by dictation ✓ 123");
            if before.is_some() {
                assert_eq!(super::read(), before, "the previous clipboard text is put back");
            }
        }
    }
}
