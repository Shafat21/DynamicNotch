//! Screenshots: the notch's camera button (or `capture_start`) freezes the whole desktop (every monitor) into one
//! image, then opens a borderless window over it (capture.html) where you drag a region and mark it up (arrows,
//! boxes, pen, highlighter, text, blur). The page sends back the finished PNG to copy and / or save.
//! The island windows are kept out of the shot (display affinity) without hiding them. Windows only for now.
//! Speed: the overlay window is made once (hidden, at app start) and reused, and the page gets the raw pixels
//! (no PNG round trip): pressing the hotkey shows the frozen screen in a frame or two.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

pub const LABEL: &str = "capture";

/// The frozen desktop while the overlay is open (PNG), and the windows that were on it, front to back, as
/// [x, y, w, h] in the image's pixels: hovering one in the overlay offers it, a click takes it.
struct Shot {
    /// Top-down RGBA, `w` x `h`.
    rgba: Vec<u8>,
    w: u32,
    h: u32,
    windows: Vec<[i32; 4]>,
    /// Each monitor in the image's pixels: the overlay spans their bounding box, which can include areas no
    /// monitor shows (monitors of different sizes), so the toolbar and "whole screen" use these instead.
    /// [x, y, w, h, scale %]: the overlay window gets one scale factor (its top-left monitor's), so the page
    /// sizes its toolbar by the scale of the monitor it is on.
    monitors: Vec<[i32; 5]>,
}

/// `r` (left, top, right, bottom on the virtual screen) moved into the image (origin `ox, oy`, size `w x h`) and cut
/// to it; None when nothing of it is on screen or it's a sliver.
pub fn to_image(r: (i32, i32, i32, i32), ox: i32, oy: i32, w: i32, h: i32) -> Option<[i32; 4]> {
    let (l, t) = ((r.0 - ox).max(0), (r.1 - oy).max(0));
    let (rr, b) = ((r.2 - ox).min(w), (r.3 - oy).min(h));
    (rr - l >= 24 && b - t >= 24).then_some([l, t, rr - l, b - t])
}

pub fn windows() -> Vec<[i32; 4]> {
    SHOT.lock().unwrap().as_ref().map(|s| s.windows.clone()).unwrap_or_default()
}

pub fn monitors() -> Vec<[i32; 5]> {
    SHOT.lock().unwrap().as_ref().map(|s| s.monitors.clone()).unwrap_or_default()
}
static SHOT: Mutex<Option<Shot>> = Mutex::new(None);
/// A capture is on screen (the overlay window itself stays alive, hidden, between captures).
static OPEN: AtomicBool = AtomicBool::new(false);

/// "Screenshot 2026-11-18 at 19.02.33.png": sorts by time, no characters Windows refuses.
pub fn file_name(t: chrono::DateTime<chrono::Local>) -> String {
    t.format("Screenshot %Y-%m-%d at %H.%M.%S.png").to_string()
}

/// RGBA rows (top-down) → a CF_DIB block: BITMAPINFOHEADER + bottom-up BGRA rows, what every Windows app pastes.
pub fn dib(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(40 + rgba.len());
    for v in [40u32, w, h] {
        out.extend_from_slice(&v.to_le_bytes()); // biSize, biWidth, biHeight (positive: bottom-up)
    }
    out.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    out.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    out.extend_from_slice(&0u32.to_le_bytes()); // biCompression = BI_RGB
    out.extend_from_slice(&(w * h * 4).to_le_bytes()); // biSizeImage
    out.extend_from_slice(&[0u8; 16]); // resolution and palette: unused
    let row = w as usize * 4;
    for y in (0..h as usize).rev() {
        for px in rgba[y * row..(y + 1) * row].chunks_exact(4) {
            out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
    }
    out
}

/// The frozen desktop for the page: width and height (u32 little-endian) then the RGBA pixels.
pub fn image() -> Result<Vec<u8>, String> {
    let g = SHOT.lock().unwrap();
    let s = g.as_ref().ok_or("No screenshot in progress")?;
    let mut out = Vec::with_capacity(8 + s.rgba.len());
    out.extend_from_slice(&s.w.to_le_bytes());
    out.extend_from_slice(&s.h.to_le_bytes());
    out.extend_from_slice(&s.rgba);
    Ok(out)
}

/// The overlay page drew the frozen desktop: show it (it was created hidden so nothing flashes).
pub fn ready(app: &AppHandle) -> Result<(), String> {
    let w = app.get_webview_window(LABEL).ok_or("No screenshot in progress")?;
    w.show().map_err(|e| e.to_string())?;
    w.set_focus().map_err(|e| e.to_string())
}

/// Hide the overlay (kept for the next capture) and drop the frozen image.
pub fn close(app: &AppHandle) {
    *SHOT.lock().unwrap() = None;
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.hide();
    }
    OPEN.store(false, Ordering::SeqCst);
}

/// `png`: the finished image from the page. `copy` puts it on the clipboard, `save` writes it to
/// Pictures\Screenshots and onto the Shelf. Returns the saved path (if saved). The overlay closes either way.
pub fn finish(app: &AppHandle, png: &[u8], copy: bool, save: bool) -> Result<Option<String>, String> {
    let result = (|| {
        if png.len() < 8 || &png[..8] != b"\x89PNG\r\n\x1a\n" || png.len() > 200 * 1024 * 1024 {
            return Err("The screenshot came back damaged".to_string());
        }
        if copy {
            os::copy(png)?;
        }
        if !save {
            return Ok(None);
        }
        let dir = app.path().picture_dir().map_err(|e| e.to_string())?.join("Screenshots");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(file_name(chrono::Local::now()));
        std::fs::write(&path, png).map_err(|e| format!("Saving failed: {e}"))?;
        let text = path.to_string_lossy().into_owned();
        {
            let state = app.state::<crate::AppState>();
            let _ = state.db.lock().unwrap().add_shelf(&text);
        }
        if let Ok(list) = crate::shelf::list(app) {
            use tauri::Emitter;
            let _ = app.emit("shelf", list);
        }
        Ok(Some(text))
    })();
    close(app);
    result
}

// ---------- hotkey ----------

const HOTKEY_KEY: &str = "capture_hotkey";
static HOTKEY: Mutex<Option<String>> = Mutex::new(None);

/// "Ctrl+Shift+S", "PrintScreen", "Alt+F1" → the canonical form, or an error to show. "" = no hotkey.
/// A key alone only for PrintScreen and F13–F24 (keys nothing else types); others need a modifier.
pub fn parse_hotkey(s: &str) -> Result<String, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(String::new());
    }
    let solo = s.eq_ignore_ascii_case("PrintScreen") || s.strip_prefix(['F', 'f']).and_then(|n| n.parse::<u8>().ok()).is_some_and(|n| (13..=24).contains(&n));
    if solo {
        let canon = if s.eq_ignore_ascii_case("PrintScreen") { "PrintScreen".to_string() } else { s.to_uppercase() };
        use std::str::FromStr;
        return tauri_plugin_global_shortcut::Shortcut::from_str(&canon).map(|_| canon.clone()).map_err(|_| format!("Invalid hotkey \"{s}\""));
    }
    let hk = crate::dictation::settings::parse_hotkey(s)?;
    if hk.key.is_none() {
        return Err("Add a key to the modifiers, like Ctrl+Shift+S".into());
    }
    Ok(hk.canonical())
}

/// The accelerator for the plugin ("Win" is "Super" there).
fn accelerator(canon: &str) -> String {
    canon.split('+').map(|t| if t == "Win" { "Super" } else { t }).collect::<Vec<_>>().join("+")
}

pub fn hotkey(app: &AppHandle) -> String {
    app.state::<crate::AppState>().db.lock().unwrap().get(HOTKEY_KEY).ok().flatten().unwrap_or_default()
}

/// Register `canon` ("" = none) in place of the previous screenshot hotkey; pressing it starts a capture.
fn bind(app: &AppHandle, canon: &str) -> Result<(), String> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
    let gs = app.global_shortcut();
    if let Some(old) = HOTKEY.lock().unwrap().take() {
        let _ = gs.unregister(old.as_str());
    }
    if canon.is_empty() {
        return Ok(());
    }
    let acc = accelerator(canon);
    gs.on_shortcut(acc.as_str(), |app, _, e| {
        if e.state() == ShortcutState::Pressed {
            let app = app.clone();
            // off the event loop: start() builds a window
            std::thread::spawn(move || {
                if let Err(e) = start(&app) {
                    eprintln!("screenshot: {e}");
                }
            });
        }
    })
    .map_err(|e| format!("Couldn't use {canon}: another app may have it. Pick another. ({e})"))?;
    *HOTKEY.lock().unwrap() = Some(acc);
    Ok(())
}

/// Settings → General: save and apply a new hotkey. Returns the canonical form saved.
pub fn set_hotkey(app: &AppHandle, s: &str) -> Result<String, String> {
    let canon = parse_hotkey(s)?;
    if !canon.is_empty() && canon == crate::dictation::settings::Settings::load(&app.state::<crate::AppState>().db.lock().unwrap()).hotkey {
        return Err("That's the dictation hotkey. Pick another.".into());
    }
    bind(app, &canon)?;
    app.state::<crate::AppState>().db.lock().unwrap().set(HOTKEY_KEY, &canon).map_err(|e| e.to_string())?;
    Ok(canon)
}

/// App start (after the global-shortcut plugin is registered by dictation): the saved hotkey.
pub fn bind_saved(app: &AppHandle) {
    let saved = hotkey(app);
    if let Err(e) = bind(app, &saved) {
        eprintln!("screenshot hotkey {saved}: {e}");
    }
}

#[cfg(windows)]
pub use os::{prewarm, start};

#[cfg(not(windows))]
pub fn prewarm(_app: &AppHandle) {}

#[cfg(not(windows))]
pub fn start(_app: &AppHandle) -> Result<(), String> {
    Err("Screenshots work on Windows for now".into())
}

#[cfg(not(windows))]
mod os {
    pub fn copy(_png: &[u8]) -> Result<(), String> {
        Err("Copying screenshots works on Windows for now".into())
    }
}

#[cfg(windows)]
mod os {
    use super::{Shot, LABEL, OPEN, SHOT};
    use std::sync::atomic::Ordering;
    use tauri::Emitter;
    use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};
    use windows::core::w;
    use windows::Win32::Foundation::{HANDLE, HWND};
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, SRCCOPY,
    };
    use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData};
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SetWindowDisplayAffinity, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
    };

    fn islands(app: &AppHandle) -> Vec<HWND> {
        crate::window::ISLANDS.iter().filter_map(|l| app.get_webview_window(l)).filter_map(|w| w.hwnd().ok()).map(|h| HWND(h.0 as _)).collect()
    }

    /// The whole virtual desktop as top-down BGRA.
    unsafe fn grab(x: i32, y: i32, w: i32, h: i32) -> Result<Vec<u8>, String> {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp.into());
        let blit = BitBlt(mem, 0, 0, w, h, Some(screen), x, y, SRCCOPY | CAPTUREBLT);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; w as usize * h as usize * 4];
        let rows = GetDIBits(mem, bmp, 0, h as u32, Some(buf.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        blit.map_err(|e| format!("Couldn't capture the screen: {e}"))?;
        if rows == 0 {
            return Err("Couldn't read the captured screen".into());
        }
        Ok(buf)
    }

    /// The overlay window, made once and kept hidden; building a webview is most of a cold start's delay.
    fn overlay(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
        if let Some(w) = app.get_webview_window(LABEL) {
            return Ok(w);
        }
        WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("capture.html".into()))
            .title("Screenshot")
            .decorations(false)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .shadow(false)
            .visible(false)
            .focused(false)
            .build()
            .map_err(|e| e.to_string())
    }

    /// App start: make the overlay now, so the first screenshot is as quick as the rest.
    pub fn prewarm(app: &AppHandle) {
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(3)); // after the islands are up
            if let Err(e) = overlay(&app) {
                eprintln!("screenshot overlay: {e}");
            }
        });
    }

    /// Freeze the desktop and show the overlay over it (hidden until the page has drawn it, see `ready`).
    pub fn start(app: &AppHandle) -> Result<(), String> {
        if OPEN.swap(true, Ordering::SeqCst) {
            return Ok(()); // already selecting
        }
        let r = run(app);
        if r.is_err() {
            OPEN.store(false, Ordering::SeqCst);
        }
        r
    }

    fn run(app: &AppHandle) -> Result<(), String> {
        let (x, y, w, h) = unsafe {
            (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN), GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN))
        };
        if w <= 0 || h <= 0 {
            return Err("No screen to capture".into());
        }
        let hidden = islands(app);
        for hwnd in &hidden {
            let _ = unsafe { SetWindowDisplayAffinity(*hwnd, WDA_EXCLUDEFROMCAPTURE) }; // the notch stays on screen, not in the shot
        }
        std::thread::sleep(std::time::Duration::from_millis(20)); // one compositor frame for DWM to apply it
        let shot = unsafe { grab(x, y, w, h) };
        for hwnd in &hidden {
            let _ = unsafe { SetWindowDisplayAffinity(*hwnd, WDA_NONE) };
        }
        let mut rgba = shot?;
        for px in rgba.chunks_exact_mut(4) {
            px.swap(0, 2); // BGRA → RGBA
            px[3] = 255;
        }
        let windows = window_list(app, x, y, w, h);
        let monitors = app
            .available_monitors()
            .unwrap_or_default()
            .iter()
            .filter_map(|m| {
                let (p, s) = (m.position(), m.size());
                let [a, b, c, d] = super::to_image((p.x, p.y, p.x + s.width as i32, p.y + s.height as i32), x, y, w, h)?;
                Some([a, b, c, d, (m.scale_factor() * 100.0).round() as i32])
            })
            .collect();
        *SHOT.lock().unwrap() = Some(Shot { rgba, w: w as u32, h: h as u32, windows, monitors });

        let win = overlay(app)?;
        win.set_position(PhysicalPosition::new(x, y)).map_err(|e| e.to_string())?;
        win.set_size(PhysicalSize::new(w as u32, h as u32)).map_err(|e| e.to_string())?;
        let _ = app.emit("capture-shot", ()); // the page loads it and calls `ready` (a fresh page checks on its own)
        Ok(())
    }

    /// Visible top-level windows, front to back, by their visible frame (DWM's extended frame bounds leave out
    /// the invisible resize border and shadow), skipping cloaked ones (other virtual desktops, suspended UWP apps)
    /// and our own islands.
    fn window_list(app: &AppHandle, ox: i32, oy: i32, w: i32, h: i32) -> Vec<[i32; 4]> {
        use windows::core::BOOL;
        use windows::Win32::Foundation::{LPARAM, RECT};
        use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
        use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowRect, IsIconic, IsWindowVisible};
        unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> BOOL {
            let out = unsafe { &mut *(lp.0 as *mut Vec<(HWND, (i32, i32, i32, i32))>) };
            unsafe {
                if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                    return true.into();
                }
                let mut cloaked = 0u32;
                if DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, (&mut cloaked as *mut u32).cast(), 4).is_ok() && cloaked != 0 {
                    return true.into();
                }
                let mut r = RECT::default();
                let framed = DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, (&mut r as *mut RECT).cast(), size_of::<RECT>() as u32).is_ok();
                if !framed && GetWindowRect(hwnd, &mut r).is_err() {
                    return true.into();
                }
                out.push((hwnd, (r.left, r.top, r.right, r.bottom)));
            }
            true.into()
        }
        let mut found: Vec<(HWND, (i32, i32, i32, i32))> = Vec::new();
        let _ = unsafe { EnumWindows(Some(each), LPARAM(&mut found as *mut _ as isize)) };
        let ours = islands(app);
        found.into_iter().filter(|(h, _)| !ours.contains(h)).filter_map(|(_, r)| super::to_image(r, ox, oy, w, h)).collect()
    }

    /// The image as CF_DIB (every app) plus the registered "PNG" format (Discord, Slack, browsers keep it sharp).
    pub fn copy(png: &[u8]) -> Result<(), String> {
        let img = image::load_from_memory_with_format(png, image::ImageFormat::Png).map_err(|e| e.to_string())?.to_rgba8();
        let dib = super::dib(img.as_raw(), img.width(), img.height());
        unsafe {
            let mut open = false;
            for _ in 0..10 {
                if OpenClipboard(None).is_ok() {
                    open = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(30)); // another app has it for a moment
            }
            if !open {
                return Err("The clipboard is busy, try again".into());
            }
            let put = |format: u32, bytes: &[u8]| -> Result<(), String> {
                let g = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).map_err(|e| e.to_string())?;
                let p = GlobalLock(g) as *mut u8;
                if p.is_null() {
                    return Err("Out of memory".into());
                }
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
                let _ = GlobalUnlock(g);
                SetClipboardData(format, Some(HANDLE(g.0))).map(drop).map_err(|e| e.to_string()) // the clipboard owns `g` now
            };
            let r = EmptyClipboard().map_err(|e| e.to_string()).and_then(|()| put(8 /* CF_DIB */, &dib)).and_then(|()| {
                let png_format = RegisterClipboardFormatW(w!("PNG"));
                if png_format != 0 { put(png_format, png) } else { Ok(()) }
            });
            let _ = CloseClipboard();
            r
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dib_is_bottom_up_bgra_with_a_40_byte_header() {
        // 2x2: top row red, green; bottom row blue, white
        let rgba = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255];
        let d = dib(&rgba, 2, 2);
        assert_eq!(d.len(), 40 + 16);
        assert_eq!(u32::from_le_bytes(d[0..4].try_into().unwrap()), 40);
        assert_eq!(i32::from_le_bytes(d[8..12].try_into().unwrap()), 2); // positive height: bottom-up
        assert_eq!(u16::from_le_bytes(d[14..16].try_into().unwrap()), 32);
        assert_eq!(&d[40..44], &[255, 0, 0, 255]); // first stored pixel = bottom-left blue, as BGRA
        assert_eq!(&d[48..52], &[0, 0, 255, 255]); // then the top row: red as BGRA
    }

    #[test]
    fn window_rects_land_in_image_pixels_cut_to_the_screen() {
        // virtual screen starts at (-1920, 0): a second monitor on the left
        assert_eq!(to_image((-1900, 10, -1000, 600), -1920, 0, 3840, 1080), Some([20, 10, 900, 590]));
        assert_eq!(to_image((-2000, -8, 100, 1088), -1920, 0, 3840, 1080), Some([0, 0, 2020, 1080])); // maximised: cut
        assert_eq!(to_image((5000, 0, 6000, 500), -1920, 0, 3840, 1080), None); // off screen
        assert_eq!(to_image((0, 0, 10, 500), 0, 0, 1920, 1080), None); // a sliver
    }

    #[test]
    fn hotkeys_need_a_modifier_unless_nothing_else_types_that_key() {
        assert_eq!(parse_hotkey("ctrl+shift+s"), Ok("Ctrl+Shift+S".into()));
        assert_eq!(parse_hotkey("printscreen"), Ok("PrintScreen".into()));
        assert_eq!(parse_hotkey("f13"), Ok("F13".into()));
        assert_eq!(parse_hotkey(""), Ok(String::new())); // off
        assert!(parse_hotkey("S").is_err()); // would fire while typing
        assert!(parse_hotkey("Ctrl+Shift").is_err()); // modifiers only: dictation's style, not this one
        assert_eq!(accelerator("Ctrl+Win+S"), "Ctrl+Super+S");
    }

    #[test]
    fn file_names_sort_by_time_and_are_windows_safe() {
        use chrono::TimeZone;
        let t = chrono::Local.with_ymd_and_hms(2026, 11, 18, 19, 2, 33).unwrap();
        let n = file_name(t);
        assert_eq!(n, "Screenshot 2026-11-18 at 19.02.33.png");
        assert!(!n.contains([':', '/', '\\', '*', '?', '"', '<', '>', '|']));
    }
}
