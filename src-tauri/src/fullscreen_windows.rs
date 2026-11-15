//! Windows: D3D-fullscreen/presentation/busy via the shell, else a foreground window covering our monitor.

use super::covers;
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

// QUNS_* states are system-wide: a fullscreen game on another monitor also hides the island.
pub fn is_fullscreen(app: &AppHandle) -> bool {
    if let Ok(state) = unsafe { SHQueryUserNotificationState() } {
        if matches!(state, QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE | QUNS_BUSY) {
            return true;
        }
    }
    let Some(ours) = app.get_webview_window("main").and_then(|w| w.hwnd().ok()).map(|h| HWND(h.0 as _)) else {
        return false;
    };
    let fg = unsafe { GetForegroundWindow() };
    if fg.0.is_null() || fg == ours {
        return false;
    }
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(fg, &mut buf) } as usize;
    if matches!(String::from_utf16_lossy(&buf[..n]).as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd") {
        return false;
    }
    let mut r = RECT::default();
    if unsafe { GetWindowRect(fg, &mut r) }.is_err() {
        return false;
    }
    // only the monitor the island is on counts
    match (monitor_rect(fg), monitor_rect(ours)) {
        (Some(fm), Some(om)) => fm == om && covers((r.left, r.top, r.right, r.bottom), fm),
        _ => false,
    }
}
