//! Windows: D3D-fullscreen/presentation/busy via the shell, else a foreground window covering our monitor.

use super::{covers, Fs};
use tauri::{AppHandle, Manager};
use windows::Win32::{
    Foundation::{HWND, RECT},
    Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST},
    UI::{
        Shell::{SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN},
        WindowsAndMessaging::{GetClassNameW, GetForegroundWindow, GetWindowRect},
    },
};

fn monitor_rect(hwnd: HWND) -> Option<(i32, i32, i32, i32)> {
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe { GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut mi).as_bool().then_some(())? };
    let r = mi.rcMonitor;
    Some((r.left, r.top, r.right, r.bottom))
}

fn hwnd_of(app: &AppHandle, label: &str) -> Option<HWND> {
    app.get_webview_window(label).and_then(|w| w.hwnd().ok()).map(|h| HWND(h.0 as _))
}

/// The monitor an island window is on.
pub fn window_monitor(app: &AppHandle, label: &str) -> Option<(i32, i32, i32, i32)> {
    monitor_rect(hwnd_of(app, label)?)
}

/// QUNS_* states are system-wide: with a single island a fullscreen game on another monitor also hides it. With an
/// island per monitor (`per_monitor`) the game's own monitor (where the foreground window is) is the one hidden.
pub fn fullscreen_state(app: &AppHandle, per_monitor: bool) -> Fs {
    let fg = unsafe { GetForegroundWindow() };
    if let Ok(state) = unsafe { SHQueryUserNotificationState() } {
        if matches!(state, QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE | QUNS_BUSY) {
            return match monitor_rect(fg).filter(|_| per_monitor && !fg.0.is_null()) {
                Some(m) => Fs::Monitor(m),
                None => Fs::All,
            };
        }
    }
    if fg.0.is_null() || crate::window::ISLANDS.iter().any(|l| hwnd_of(app, l) == Some(fg)) {
        return Fs::No;
    }
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(fg, &mut buf) } as usize;
    if matches!(String::from_utf16_lossy(&buf[..n]).as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd") {
        return Fs::No;
    }
    let mut r = RECT::default();
    if unsafe { GetWindowRect(fg, &mut r) }.is_err() {
        return Fs::No;
    }
    // a window that covers its monitor: only the island(s) on that monitor are hidden
    match monitor_rect(fg) {
        Some(fm) if covers((r.left, r.top, r.right, r.bottom), fm) => Fs::Monitor(fm),
        _ => Fs::No,
    }
}

/// File name of the process that owns `hwnd`, lower-case ("chrome.exe").
fn exe_of(hwnd: HWND) -> Option<String> {
    use windows::{
        core::PWSTR,
        Win32::{
            Foundation::CloseHandle,
            System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION},
            UI::WindowsAndMessaging::GetWindowThreadProcessId,
        },
    };
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buf = [0u16; 520];
    let mut len = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }.is_ok();
    let _ = unsafe { CloseHandle(h) };
    ok.then(|| super::exe_name(&String::from_utf16_lossy(&buf[..len as usize])))
}

/// The app in front, by file name (None when there is no foreground window).
pub fn foreground_exe() -> Option<String> {
    let fg = unsafe { GetForegroundWindow() };
    (!fg.0.is_null()).then(|| exe_of(fg)).flatten()
}

/// Apps that have a visible, titled window right now, for the picker in Settings.
pub fn running_apps() -> Vec<String> {
    use windows::{
        core::BOOL,
        Win32::{
            Foundation::LPARAM,
            UI::WindowsAndMessaging::{EnumWindows, GetWindowTextLengthW, IsWindowVisible},
        },
    };
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let out = unsafe { &mut *(lparam.0 as *mut Vec<String>) };
        if unsafe { IsWindowVisible(hwnd) }.as_bool() && unsafe { GetWindowTextLengthW(hwnd) } > 0 {
            if let Some(name) = exe_of(hwnd) {
                out.push(name);
            }
        }
        true.into()
    }
    let mut out: Vec<String> = Vec::new();
    let _ = unsafe { EnumWindows(Some(each), LPARAM(&mut out as *mut _ as isize)) };
    out.sort();
    out.dedup();
    out
}
