//! Windows: a message-only window on its own thread receives WM_CLIPBOARDUPDATE (AddClipboardFormatListener).
//! Content that password managers mark as private is skipped.

use std::cell::RefCell;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Duration;
use tauri::AppHandle;
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{GlobalFree, HANDLE, HGLOBAL, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        System::{
            DataExchange::{
                AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
                RegisterClipboardFormatW, SetClipboardData,
            },
            LibraryLoader::GetModuleHandleW,
            Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE},
        },
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW, TranslateMessage, HWND_MESSAGE, MSG, WINDOW_EX_STYLE,
            WINDOW_STYLE, WM_CLIPBOARDUPDATE, WNDCLASSW,
        },
    },
};

const CF_UNICODETEXT: u32 = 13;

/// Our listener window (clipboard owner for writes); 0 until the thread has created it.
static OWNER: AtomicIsize = AtomicIsize::new(0);

type Callback = Box<dyn Fn(String)>;

thread_local! {
    static ON_TEXT: RefCell<Option<Callback>> = const { RefCell::new(None) };
}

/// OpenClipboard fails while another app holds it, so retry briefly.
fn open(owner: Option<HWND>) -> bool {
    for _ in 0..10 {
        if unsafe { OpenClipboard(owner) }.is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// With the clipboard open: true when the source asked not to be recorded.
fn private() -> bool {
    unsafe {
        if IsClipboardFormatAvailable(RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"))).is_ok() {
            return true;
        }
        let fmt = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
        if IsClipboardFormatAvailable(fmt).is_err() {
            return false;
        }
        // present but unreadable: fail closed (treat as private)
        let Ok(h) = GetClipboardData(fmt) else { return true };
        let g = HGLOBAL(h.0);
        let p = GlobalLock(g) as *const u32;
        let denied = p.is_null() || GlobalSize(g) < 4 || p.read_unaligned() == 0;
        if !p.is_null() {
            let _ = GlobalUnlock(g);
        }
        denied
    }
}

fn read(hwnd: HWND) -> Option<String> {
    if !open(Some(hwnd)) {
        return None;
    }
    let text = (|| unsafe {
        if private() || IsClipboardFormatAvailable(CF_UNICODETEXT).is_err() {
            return None;
        }
        let g = HGLOBAL(GetClipboardData(CF_UNICODETEXT).ok()?.0);
        let p = GlobalLock(g) as *const u16;
        if p.is_null() {
            return None;
        }
        let max = GlobalSize(g) / 2;
        let mut n = 0;
        while n < max && *p.add(n) != 0 {
            n += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
        let _ = GlobalUnlock(g);
        Some(s)
    })();
    unsafe {
        let _ = CloseClipboard();
    }
    text
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_CLIPBOARDUPDATE {
        if let Some(t) = read(hwnd) {
            ON_TEXT.with(|f| f.borrow().as_ref().map(|f| f(t)));
        }
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

pub fn spawn(_app: AppHandle, on_text: impl Fn(String) + Send + 'static) {
    std::thread::spawn(move || unsafe {
        ON_TEXT.with(|f| *f.borrow_mut() = Some(Box::new(on_text)));
        let Ok(module) = GetModuleHandleW(None) else { return };
        let class = w!("DynamicNotchClipboard");
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: HINSTANCE(module.0), lpszClassName: class, ..Default::default() });
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            PCWSTR::null(),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(HINSTANCE(module.0)),
            None,
        ) else {
            eprintln!("clipboard: window creation failed");
            return;
        };
        if let Err(e) = AddClipboardFormatListener(hwnd) {
            eprintln!("clipboard: listener failed: {e}");
            return;
        }
        OWNER.store(hwnd.0 as isize, Ordering::Relaxed);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}

pub fn write(text: &str) -> Result<(), String> {
    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    let owner = OWNER.load(Ordering::Relaxed);
    if !open((owner != 0).then_some(HWND(owner as _))) {
        return Err("The clipboard is busy".into());
    }
    let r = (|| unsafe {
        let g = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2).map_err(|e| e.to_string())?;
        let p = GlobalLock(g) as *mut u16;
        if p.is_null() {
            let _ = GlobalFree(Some(g));
            return Err("Out of memory".to_string());
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
        let _ = GlobalUnlock(g);
        if let Err(e) = EmptyClipboard() {
            let _ = GlobalFree(Some(g));
            return Err(e.to_string());
        }
        // on success the system owns `g`
        SetClipboardData(CF_UNICODETEXT, Some(HANDLE(g.0))).map(|_| ()).map_err(|e| {
            let _ = GlobalFree(Some(g));
            e.to_string()
        })
    })();
    unsafe {
        let _ = CloseClipboard();
    }
    r
}
