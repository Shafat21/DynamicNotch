//! Windows media sessions (GSMTC): polls every 100 ms and publishes `media` / `media-art` / `media-progress` / `lyric`.

use super::{art_data_uri, current_index, dominant_color, fetch_lyrics, key, should_retry_art, Lyrics, Media, LYRIC_LEAD_SECS};
use crate::publish;
use serde_json::json;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::thread;
use tauri::AppHandle;
use windows::core::{Result, BOOL, PWSTR};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::DataReader;

pub fn spawn(app: AppHandle) {
    thread::spawn(move || {
        if let Err(e) = run(&app) {
            eprintln!("media monitor stopped: {e}");
        }
    });
}

fn run(app: &AppHandle) -> Result<()> {
    let mgr = Manager::RequestAsync()?.get()?;
    let mut last = Media::idle();
    let mut lyrics: Lyrics = Vec::new();
    let mut at: Option<usize> = None; // the line being sung
    let (tx, rx) = mpsc::channel::<(String, Lyrics)>();
    let (mut art_since, mut have_art) = (Instant::now(), false); // art retry window, reset per track
    publish(app, "media", &last);

    let mut tick: u64 = 0;
    loop {
        tick = tick.wrapping_add(1);
        thread::sleep(Duration::from_millis(100));
        let session = mgr.GetCurrentSession().ok();

        if tick.is_multiple_of(5) {
            let mut cur = session.as_ref().and_then(|s| read_state(s).ok()).unwrap_or_else(Media::idle);
            let track_changed = cur.title != last.title || cur.artist != last.artist;
            cur.accent = last.accent.clone();
            if track_changed {
                let mut art = String::new();
                if let (Some(s), false) = (&session, cur.state == "Idle") {
                    let bytes = thumbnail(s).unwrap_or_default();
                    cur.accent = dominant_color(&bytes).unwrap_or_else(|| "#000000".into());
                    art = art_data_uri(&bytes).unwrap_or_default();
                }
                (art_since, have_art) = (Instant::now(), !art.is_empty());
                publish(app, "media-art", json!({ "art": art }));
            }
            if cur != last {
                last = cur;
                publish(app, "media", &last);

                if track_changed {
                    lyrics.clear();
                    at = None;
                    publish(app, "lyric", "");
                    publish(app, "lyric-at", -1);
                    let looking = !last.artist.is_empty() && !last.title.is_empty();
                    publish(app, "lyric-lines", json!({ "state": if looking { "loading" } else { "none" }, "lines": [] }));
                    if looking {
                        let (tx, artist, title, length) = (tx.clone(), last.artist.clone(), last.title.clone(), last.duration);
                        thread::spawn(move || {
                            let _ = tx.send((key(&artist, &title), fetch_lyrics(&artist, &title, length)));
                        });
                    }
                }
            }
        }

        if tick.is_multiple_of(10) && last.state != "Idle" && should_retry_art(art_since.elapsed().as_millis() as u64, have_art) {
            let bytes = session.as_ref().and_then(thumbnail).unwrap_or_default();
            if let Some(art) = art_data_uri(&bytes) {
                have_art = true;
                publish(app, "media-art", json!({ "art": art }));
                if let Some(accent) = dominant_color(&bytes) {
                    last.accent = accent;
                    publish(app, "media", &last);
                }
            }
        }

        if tick.is_multiple_of(10) && last.state != "Idle" {
            if let Some(pos) = session.as_ref().and_then(|s| position_secs(s, last.state == "Playing")) {
                let pos = if last.duration > 0.0 { pos.min(last.duration) } else { pos };
                publish(app, "media-progress", json!({ "position": pos, "duration": last.duration }));
            }
        }

        // Drop results for tracks that are no longer playing.
        while let Ok((k, l)) = rx.try_recv() {
            if k == key(&last.artist, &last.title) {
                lyrics = l;
                at = None;
                let lines: Vec<_> = lyrics.iter().map(|(t, text)| json!({ "t": t, "text": text })).collect();
                publish(app, "lyric-lines", json!({ "state": if lines.is_empty() { "none" } else { "ready" }, "lines": lines }));
            }
        }

        // also while paused: a seek moves the line, and the position is exact then
        if last.state != "Idle" && !lyrics.is_empty() {
            if let Some(pos) = session.as_ref().and_then(|s| position_secs(s, last.state == "Playing")) {
                let now = current_index(&lyrics, pos + LYRIC_LEAD_SECS);
                if now != at {
                    at = now;
                    publish(app, "lyric-at", now.map_or(-1, |i| i as i64));
                    publish(app, "lyric", now.map_or("", |i| lyrics[i].1.as_str()));
                }
            }
        }
    }
}

/// State, title, artist, duration and seekability; the caller fills in the accent.
fn read_state(s: &Session) -> Result<Media> {
    let info = s.GetPlaybackInfo()?;
    let state = match info.PlaybackStatus()? {
        Status::Playing => "Playing",
        Status::Paused => "Paused",
        _ => return Ok(Media::idle()),
    };
    let props = s.TryGetMediaPropertiesAsync()?.get()?;
    let title = props.Title()?.to_string();
    let title = if title.is_empty() { "Unknown Title".to_string() } else { title };
    // Timeline/controls are optional extras: a player without them still shows as playing.
    let duration = s
        .GetTimelineProperties()
        .and_then(|tl| Ok((tl.EndTime()?.Duration - tl.StartTime()?.Duration) as f64 / 1e7))
        .map_or(0.0, |d| d.max(0.0));
    let seekable = info.Controls().and_then(|c| c.IsPlaybackPositionEnabled()).unwrap_or(true);
    Ok(Media { state, title, artist: props.Artist()?.to_string(), accent: String::new(), duration, seekable })
}

/// Current playback position; while playing, extrapolated from the last timeline update.
fn position_secs(s: &Session, extrapolate: bool) -> Option<f64> {
    let tl = s.GetTimelineProperties().ok()?;
    let pos = tl.Position().ok()?.Duration as f64 / 1e7;
    if !extrapolate {
        return Some(pos);
    }
    let updated = tl.LastUpdatedTime().ok()?.UniversalTime; // 100 ns ticks since 1601-01-01
    let now_unix = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    let now = (now_unix.as_secs_f64() + 11_644_473_600.0) * 1e7;
    Some(pos + ((now - updated as f64) / 1e7).max(0.0))
}

fn thumbnail(s: &Session) -> Option<Vec<u8>> {
    let read = || -> Result<Vec<u8>> {
        let stream = s.TryGetMediaPropertiesAsync()?.get()?.Thumbnail()?.OpenReadAsync()?.get()?;
        let size = stream.Size()? as u32;
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
        reader.LoadAsync(size)?.get()?;
        let mut buf = vec![0u8; size as usize];
        reader.ReadBytes(&mut buf)?;
        Ok(buf)
    };
    read().ok()
}

/// A system media key, as a keyboard's own next / previous button sends it.
fn media_key(vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP};
    let key = |flags: KEYBD_EVENT_FLAGS| INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, dwFlags: flags, ..Default::default() } } };
    unsafe { SendInput(&[key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)], std::mem::size_of::<INPUT>() as i32) };
}

/// Play/pause, next, previous on the current media session. Browsers (YouTube in Chrome) often answer a skip
/// request with "no" when the page has no next / previous handler of its own; then the media key goes to
/// whatever owns media keys, which is the same browser tab.
pub fn control(action: &str) -> std::result::Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_MEDIA_NEXT_TRACK, VK_MEDIA_PREV_TRACK};
    let run = || -> Result<()> {
        let s = Manager::RequestAsync()?.get()?.GetCurrentSession()?;
        match action {
            "toggle" => s.TryTogglePlayPauseAsync()?.get()?,
            "next" => s.TrySkipNextAsync()?.get()? || (media_key(VK_MEDIA_NEXT_TRACK), true).1,
            "prev" => s.TrySkipPreviousAsync()?.get()? || (media_key(VK_MEDIA_PREV_TRACK), true).1,
            _ => false,
        };
        Ok(())
    };
    run().map_err(|e| e.to_string())
}

pub fn seek(seconds: f64) -> std::result::Result<(), String> {
    let s = Manager::RequestAsync().and_then(|m| m.get()).and_then(|m| m.GetCurrentSession()).map_err(|e| e.to_string())?;
    match s.TryChangePlaybackPositionAsync((seconds * 1e7) as i64).and_then(|op| op.get()) {
        Ok(true) => Ok(()),
        Ok(false) => Err("The player refused to seek".into()),
        Err(e) => Err(e.to_string()),
    }
}

/// Bring the current session's app forward: packaged apps via their AUMID, others via their exe's window.
pub fn open_player() -> std::result::Result<(), String> {
    let aumid = Manager::RequestAsync()
        .and_then(|m| m.get())
        .and_then(|m| m.GetCurrentSession())
        .and_then(|s| s.SourceAppUserModelId())
        .map(|h| h.to_string())
        .unwrap_or_default();
    let ok = if aumid.contains('!') {
        // explorer.exe's exit code is meaningless, so a successful spawn counts as success.
        Command::new("explorer.exe").arg(format!(r"shell:AppsFolder\{aumid}")).spawn().is_ok()
    } else {
        Path::new(&aumid).file_name().is_some_and(|exe| focus_exe_window(&exe.to_string_lossy()))
    };
    if ok { Ok(()) } else { Err("Can't open this player".into()) }
}

/// Restore and foreground a top-level window of the process named `exe` (visible windows first).
fn focus_exe_window(exe: &str) -> bool {
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows::Win32::UI::WindowsAndMessaging::*;

    unsafe extern "system" fn collect(hwnd: HWND, lp: LPARAM) -> BOOL {
        let found = &mut *(lp.0 as *mut Vec<HWND>);
        if GetWindow(hwnd, GW_OWNER).is_err() && GetWindowTextLengthW(hwnd) > 0 {
            found.push(hwnd);
        }
        true.into()
    }
    let image_name = |hwnd: HWND| -> Option<String> {
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        let proc = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = unsafe { QueryFullProcessImageNameW(proc, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) };
        let _ = unsafe { CloseHandle(proc) };
        ok.ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned())
    };

    let mut found: Vec<HWND> = Vec::new();
    let _ = unsafe { EnumWindows(Some(collect), LPARAM(&mut found as *mut _ as isize)) };
    let mut mine: Vec<HWND> = found.into_iter().filter(|&h| image_name(h).is_some_and(|n| n.eq_ignore_ascii_case(exe))).collect();
    mine.sort_by_key(|&h| !unsafe { IsWindowVisible(h) }.as_bool());
    let Some(&hwnd) = mine.first() else { return false };
    unsafe {
        // SW_RESTORE would un-maximize a maximized window, so only use it when minimized or hidden.
        if IsIconic(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd).as_bool()
    }
}
