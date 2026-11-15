//! Windows media sessions (GSMTC): polls every 100 ms and publishes `media` / `lyric`.

use super::{art_data_uri, current_line, dominant_color, fetch_lyrics, key, Lyrics, Media, LYRIC_LEAD_SECS};
use crate::publish;
use serde_json::json;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
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
    let mut line = String::new();
    let (tx, rx) = mpsc::channel::<(String, Lyrics)>();
    publish(app, "media", &last);

    let mut tick: u64 = 0;
    loop {
        tick = tick.wrapping_add(1);
        thread::sleep(Duration::from_millis(100));
        let session = mgr.GetCurrentSession().ok();

        if tick.is_multiple_of(5) {
            let mut cur = session.as_ref().and_then(|s| read_state(s).ok()).unwrap_or_else(Media::idle);
            let track_changed = cur.title != last.title || cur.artist != last.artist;
            (cur.accent, cur.art) = match (&session, track_changed) {
                (Some(s), true) if cur.state != "Idle" => {
                    let bytes = thumbnail(s).unwrap_or_default();
                    (dominant_color(&bytes).unwrap_or_else(|| "#000000".into()), art_data_uri(&bytes).unwrap_or_default())
                }
                (_, true) => (last.accent.clone(), String::new()),
                _ => (last.accent.clone(), last.art.clone()),
            };
            if cur != last {
                last = cur;
                publish(app, "media", &last);

                if track_changed {
                    lyrics.clear();
                    line.clear();
                    publish(app, "lyric", "");
                    if !last.artist.is_empty() && !last.title.is_empty() {
                        let (tx, artist, title) = (tx.clone(), last.artist.clone(), last.title.clone());
                        thread::spawn(move || {
                            let _ = tx.send((key(&artist, &title), fetch_lyrics(&artist, &title)));
                        });
                    }
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
            }
        }

        if last.state == "Playing" && !lyrics.is_empty() {
            if let Some(pos) = session.as_ref().and_then(|s| position_secs(s, true)) {
                let now = current_line(&lyrics, pos + LYRIC_LEAD_SECS);
                if now != line {
                    line = now.to_string();
                    publish(app, "lyric", &line);
                }
            }
        }
    }
}

/// State, title, artist, duration and seekability; the caller fills in accent/art.
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
    let tl = s.GetTimelineProperties()?;
    let duration = ((tl.EndTime()?.Duration - tl.StartTime()?.Duration) as f64 / 1e7).max(0.0);
    let seekable = info.Controls()?.IsPlaybackPositionEnabled()?;
    Ok(Media { state, title, artist: props.Artist()?.to_string(), accent: String::new(), art: String::new(), duration, seekable })
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

/// Play/pause, next, previous on the current media session.
pub fn control(action: &str) -> std::result::Result<(), String> {
    let run = || -> Result<()> {
        let s = Manager::RequestAsync()?.get()?.GetCurrentSession()?;
        match action {
            "toggle" => s.TryTogglePlayPauseAsync()?.get()?,
            "next" => s.TrySkipNextAsync()?.get()?,
            "prev" => s.TrySkipPreviousAsync()?.get()?,
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
