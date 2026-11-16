//! Windows: put the text on the clipboard (kept out of history), send Ctrl+V, then put the previous
//! clipboard text back after ~800 ms if the clipboard still holds ours.
//! An elevated foreground window (while we are not) blocks injected keys (UIPI) without an error:
//! then we only copy and leave our text on the clipboard.

use std::time::Duration;
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, HGLOBAL},
        Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
        System::{
            DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard},
            Memory::{GlobalLock, GlobalSize, GlobalUnlock},
            Threading::{GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION},
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
    let prev = read();
    crate::clipboard::write(text)?;
    let held: Vec<u16> = HELD.into_iter().filter(|&vk| unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 != 0).collect();
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
