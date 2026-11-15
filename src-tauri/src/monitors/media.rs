//! Now-playing via Windows media sessions (GSMTC): state/title/artist + album-art accent colour
//! (`media`), and synced lyrics from lrclib.net (`lyric`). Polls every 100 ms.

use super::http;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

#[cfg(windows)]
#[path = "media_windows.rs"]
mod os;
#[cfg(target_os = "linux")]
#[path = "media_linux.rs"]
mod os;
pub use os::{control, spawn};


/// Lyrics are shown slightly early so the line appears as it is sung.
pub(super) const LYRIC_LEAD_SECS: f64 = 1.0;

#[derive(Serialize, Clone, PartialEq, Debug)]
pub(super) struct Media {
    state: &'static str, // "Playing" | "Paused" | "Idle"
    title: String,
    artist: String,
    accent: String,
}

impl Media {
    pub(super) fn idle() -> Self {
        Self { state: "Idle", title: String::new(), artist: String::new(), accent: "#000000".into() }
    }
}

pub(super) type Lyrics = Vec<(f64, String)>;

pub(super) fn key(artist: &str, title: &str) -> String {
    format!("{artist}\u{1f}{title}")
}

/// Most common colour bucket of the album art, skipping near-black and near-white.
pub(super) fn dominant_color(bytes: &[u8]) -> Option<String> {
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

pub(super) fn fetch_lyrics(artist: &str, title: &str) -> Lyrics {
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

pub(super) fn current_line(lyrics: &Lyrics, pos: f64) -> &str {
    lyrics.iter().take_while(|(t, _)| *t <= pos).last().map(|(_, l)| l.as_str()).unwrap_or("")
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
