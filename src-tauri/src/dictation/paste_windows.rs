//! Windows: put the text on the clipboard (kept out of history), send Ctrl+V, then put the previous
//! clipboard text back after ~400 ms if the clipboard still holds ours.

use std::time::Duration;
use windows::{
    core::w,
    Win32::{
        Foundation::HGLOBAL,
        System::{
            DataExchange::{CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW},
            Memory::{GlobalLock, GlobalSize, GlobalUnlock},
        },
    },
};

const CF_UNICODETEXT: u32 = 13;
const VK_CONTROL: u16 = 0x11;
const VK_V: u16 = 0x56;

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
        if IsClipboardFormatAvailable(RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"))).is_ok() {
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

/// Ok(true) = pasted.
pub fn paste(text: &str) -> Result<bool, String> {
    let prev = read();
    crate::clipboard::write(text)?;
    super::hotkey_os::send_keys(&[(VK_CONTROL, false), (VK_V, false), (VK_V, true), (VK_CONTROL, true)])?;
    let ours = text.to_string();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        if let Some(p) = prev.filter(|_| read().as_deref() == Some(ours.as_str())) {
            let _ = crate::clipboard::write(&p);
        }
    });
    Ok(true)
}
