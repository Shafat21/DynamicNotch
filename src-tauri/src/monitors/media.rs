//! Now-playing via Windows media sessions (GSMTC): state/title/artist + album-art accent colour
//! (`media`), and synced lyrics from lrclib.net (`lyric`). Polls every 100 ms.

use super::http;
use crate::publish;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::thread;
use tauri::AppHandle;
use windows::core::Result;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::DataReader;

/// Lyrics are shown slightly early so the line appears as it is sung.
const LYRIC_LEAD_SECS: f64 = 1.0;

#[derive(Serialize, Clone, PartialEq, Debug)]
struct Media {
    state: &'static str, // "Playing" | "Paused" | "Idle"
    title: String,
    artist: String,
    accent: String,
}

impl Media {
    fn idle() -> Self {
        Self { state: "Idle", title: String::new(), artist: String::new(), accent: "#000000".into() }
    }
}

type Lyrics = Vec<(f64, String)>;

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

        if tick % 5 == 0 {
            let (state, title, artist) = session.as_ref().and_then(|s| read_state(s).ok()).unwrap_or(("Idle", String::new(), String::new()));
            let track_changed = title != last.title || artist != last.artist;
            if state != last.state || track_changed {
                let accent = match (&session, track_changed) {
                    (Some(s), true) if state != "Idle" => thumbnail_color(s).unwrap_or_else(|| "#000000".into()),
                    _ => last.accent.clone(),
                };
                last = Media { state, title, artist, accent };
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

        // Drop results for tracks that are no longer playing.
        while let Ok((k, l)) = rx.try_recv() {
            if k == key(&last.artist, &last.title) {
                lyrics = l;
            }
        }

        if last.state == "Playing" && !lyrics.is_empty() {
            if let Some(pos) = session.as_ref().and_then(position_secs) {
                let now = current_line(&lyrics, pos + LYRIC_LEAD_SECS);
                if now != line {
                    line = now.to_string();
                    publish(app, "lyric", &line);
                }
            }
        }
    }
}

fn key(artist: &str, title: &str) -> String {
    format!("{artist}\u{1f}{title}")
}

fn read_state(s: &Session) -> Result<(&'static str, String, String)> {
    let state = match s.GetPlaybackInfo()?.PlaybackStatus()? {
        Status::Playing => "Playing",
        Status::Paused => "Paused",
        _ => return Ok(("Idle", String::new(), String::new())),
    };
    let props = s.TryGetMediaPropertiesAsync()?.get()?;
    let title = props.Title()?.to_string();
    let title = if title.is_empty() { "Unknown Title".to_string() } else { title };
    Ok((state, title, props.Artist()?.to_string()))
}

/// Current playback position, extrapolated from the last timeline update.
fn position_secs(s: &Session) -> Option<f64> {
    let tl = s.GetTimelineProperties().ok()?;
    let pos = tl.Position().ok()?.Duration as f64 / 1e7;
    let updated = tl.LastUpdatedTime().ok()?.UniversalTime; // 100 ns ticks since 1601-01-01
    let now_unix = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    let now = (now_unix.as_secs_f64() + 11_644_473_600.0) * 1e7;
    Some(pos + ((now - updated as f64) / 1e7).max(0.0))
}

fn thumbnail_color(s: &Session) -> Option<String> {
    let read = || -> Result<Vec<u8>> {
        let stream = s.TryGetMediaPropertiesAsync()?.get()?.Thumbnail()?.OpenReadAsync()?.get()?;
        let size = stream.Size()? as u32;
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
        reader.LoadAsync(size)?.get()?;
        let mut buf = vec![0u8; size as usize];
        reader.ReadBytes(&mut buf)?;
        Ok(buf)
    };
    dominant_color(&read().ok()?)
}

/// Most common colour bucket of the album art, skipping near-black and near-white.
fn dominant_color(bytes: &[u8]) -> Option<String> {
    let img = image::load_from_memory(bytes).ok()?.resize_exact(32, 32, image::imageops::FilterType::Triangle).to_rgb8();
    let mut buckets: HashMap<(u8, u8, u8), (u32, [u32; 3])> = HashMap::new();
    let mut all = [0u32; 3];
    for p in img.pixels() {
        let [r, g, b] = p.0;
        all = [all[0] + r as u32, all[1] + g as u32, all[2] + b as u32];
        let sum = r as u32 + g as u32 + b as u32;
        if !(50..700).contains(&sum) {
            continue;
        }
        let e = buckets.entry((r >> 4, g >> 4, b >> 4)).or_default();
        e.0 += 1;
        e.1 = [e.1[0] + r as u32, e.1[1] + g as u32, e.1[2] + b as u32];
    }
    let (n, sum) = buckets.into_values().max_by_key(|(n, _)| *n).unwrap_or((1024, all));
    Some(format!("#{:02x}{:02x}{:02x}", sum[0] / n, sum[1] / n, sum[2] / n))
}

fn fetch_lyrics(artist: &str, title: &str) -> Lyrics {
    let res: std::result::Result<Value, ureq::Error> = http()
        .get("https://lrclib.net/api/get")
        .query("artist_name", artist)
        .query("track_name", title)
        .call()
        .and_then(|mut r| r.body_mut().read_json());
    match res {
        Ok(data) => {
            if let Some(lrc) = data["syncedLyrics"].as_str() {
                parse_lrc(lrc)
            } else if let Some(first) = data["plainLyrics"].as_str().and_then(|p| p.lines().next()) {
                vec![(0.0, first.to_string())]
            } else {
                Vec::new()
            }
        }
        Err(_) => Vec::new(), // 404 = no lyrics for this track; nothing to show
    }
}

/// Parse LRC text: "[mm:ss.xx]line", including several timestamps on one line.
fn parse_lrc(text: &str) -> Lyrics {
    let mut out = Vec::new();
    for raw in text.lines() {
        let mut rest = raw.trim();
        let mut stamps = Vec::new();
        while let Some(end) = rest.strip_prefix('[').and_then(|r| r.find(']')) {
            let tag = &rest[1..end + 1];
            if let Some((m, s)) = tag.split_once(':') {
                if let (Ok(m), Ok(s)) = (m.parse::<f64>(), s.parse::<f64>()) {
                    stamps.push(m * 60.0 + s);
                }
            }
            rest = &rest[end + 2..];
        }
        for t in stamps {
            out.push((t, rest.trim().to_string()));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

fn current_line(lyrics: &Lyrics, pos: f64) -> &str {
    lyrics.iter().take_while(|(t, _)| *t <= pos).last().map(|(_, l)| l.as_str()).unwrap_or("")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc_parsing_and_lookup() {
        let l = parse_lrc("[ar:Someone]\n[00:12.50]Second\n[00:01.00][00:20.00]Chorus\nno tag line\n[01:02]Last");
        assert_eq!(l.len(), 4);
        assert_eq!(l[0], (1.0, "Chorus".to_string()));
        assert_eq!(current_line(&l, 0.5), "");
        assert_eq!(current_line(&l, 13.0), "Second");
        assert_eq!(current_line(&l, 21.0), "Chorus");
        assert_eq!(current_line(&l, 62.0), "Last");
    }

    #[test]
    fn dominant_color_picks_main_bucket() {
        let mut img = image::RgbImage::from_pixel(10, 10, image::Rgb([200, 30, 30]));
        img.put_pixel(0, 0, image::Rgb([0, 0, 0])); // ignored: too dark
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let c = dominant_color(&png).unwrap();
        assert!(c.starts_with("#c") || c.starts_with("#b"), "got {c}");
    }
}
