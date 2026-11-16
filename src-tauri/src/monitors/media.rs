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
pub use os::{control, open_player, seek, spawn};

/// Lyrics are shown slightly early so the line appears as it is sung (the card animates in over ~0.35 s).
pub(super) const LYRIC_LEAD_SECS: f64 = 0.5;

#[derive(Serialize, Clone, PartialEq, Debug)]
pub(super) struct Media {
    state: &'static str, // "Playing" | "Paused" | "Idle"
    title: String,
    artist: String,
    accent: String, // album art itself goes out separately as `media-art`, only on track change
    duration: f64,  // seconds, 0 if unknown
    seekable: bool,
}

impl Media {
    pub(super) fn idle() -> Self {
        Self { state: "Idle", title: String::new(), artist: String::new(), accent: "#000000".into(), duration: 0.0, seekable: true }
    }
}

/// Album art as a data URI, mime sniffed from magic bytes; `None` if unknown or over 512 KB.
pub(super) fn art_data_uri(bytes: &[u8]) -> Option<String> {
    if bytes.len() > 512 * 1024 {
        return None;
    }
    let mime = match bytes {
        [0xFF, 0xD8, 0xFF, ..] => "jpeg",
        [0x89, b'P', b'N', b'G', ..] => "png",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "webp",
        [b'B', b'M', ..] => "bmp",
        _ => return None,
    };
    Some(format!("data:image/{mime};base64,{}", crate::launcher::base64(bytes)))
}

/// A track's thumbnail often lands a moment after its title: keep re-reading it for up to 5 s.
pub(super) fn should_retry_art(elapsed_ms: u64, have_art: bool) -> bool {
    !have_art && elapsed_ms <= 5_000
}

/// Seek target must be finite and within a known (non-zero) duration.
pub fn validate_seek(seconds: f64, duration: f64) -> Result<f64, String> {
    if duration.is_finite() && duration > 0.0 && seconds.is_finite() && (0.0..=duration).contains(&seconds) {
        Ok(seconds)
    } else {
        Err("Can't seek there".into())
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

/// One lrclib search result.
struct Cand {
    title: String,
    duration: f64,
    synced: Option<String>,
}

/// lrclib often has several edits of a song (293 s and 299 s of the same title); lyrics timed for another edit
/// drift away from the music, so a version more than this far off the playing track's length is not used.
const MAX_LENGTH_DIFF: f64 = 10.0;

/// The candidate with synced lyrics whose length is closest to the playing track's (`duration` 0 = unknown: the
/// first one). None when nothing fits.
fn pick_best(c: &[Cand], duration: f64) -> Option<usize> {
    c.iter()
        .enumerate()
        .filter(|(_, c)| c.synced.is_some())
        .map(|(i, c)| (i, if duration > 0.0 { (c.duration - duration).abs() } else { 0.0 }))
        .filter(|&(_, d)| duration <= 0.0 || d <= MAX_LENGTH_DIFF)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// "Song (feat. X) [Remastered]" -> "Song": drops every (...) and [...] group.
fn clean_title(t: &str) -> String {
    let (mut out, mut depth) = (String::new(), 0u32);
    for ch in t.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// "A, B" / "A & B" / "A feat. B" -> "A".
fn first_artist(a: &str) -> String {
    let lower = a.to_lowercase();
    let cut = [",", "&", ";", " feat", " ft.", " ft ", " x ", " with "].iter().filter_map(|s| lower.find(s)).min().unwrap_or(a.len());
    a[..cut].trim().to_string()
}

/// What to ask lrclib, most specific first: the names as the player gives them, then without features and
/// decorations, and for YouTube ("Artist - Song (Official Video)" by a channel) the artist and song from the title.
fn lyric_queries(artist: &str, title: &str) -> Vec<(String, String)> {
    fn push(out: &mut Vec<(String, String)>, a: String, t: String) {
        if !a.is_empty() && !t.is_empty() && !out.iter().any(|(x, y)| *x == a && *y == t) {
            out.push((a, t));
        }
    }
    let mut out = Vec::new();
    push(&mut out, artist.to_string(), title.to_string());
    match title.split_once(" - ") {
        Some((l, r)) => push(&mut out, first_artist(l.trim()), clean_title(r)),
        None => push(&mut out, first_artist(artist), clean_title(title)),
    }
    out
}

fn same_title(a: &str, b: &str) -> bool {
    clean_title(a).to_lowercase() == clean_title(b).to_lowercase()
}

fn search(params: &[(&str, &str)], title: &str, duration: f64) -> Option<Lyrics> {
    let mut req = http().get("https://lrclib.net/api/search");
    for (k, v) in params {
        req = req.query(k, v);
    }
    let res: Value = req.call().ok()?.body_mut().read_json().ok()?;
    let mut cands: Vec<Cand> = res
        .as_array()?
        .iter()
        .map(|r| Cand {
            title: r["trackName"].as_str().unwrap_or("").into(),
            duration: r["duration"].as_f64().unwrap_or(0.0),
            synced: r["syncedLyrics"].as_str().map(String::from),
        })
        .collect();
    cands.retain(|c| same_title(&c.title, title)); // a search can return other songs: never show those lyrics
    let lyrics = parse_lrc(cands[pick_best(&cands, duration)?].synced.as_deref()?);
    (!lyrics.is_empty()).then_some(lyrics)
}

/// Synced lyrics for the playing track (`duration` seconds, 0 = unknown); empty when lrclib has none that fit.
pub(super) fn fetch_lyrics(artist: &str, title: &str, duration: f64) -> Lyrics {
    for (a, t) in lyric_queries(artist, title) {
        if let Some(l) = search(&[("track_name", &t), ("artist_name", &a)], &t, duration) {
            return l;
        }
    }
    // last resort: free text, for names the field search misses
    let t = clean_title(title);
    search(&[("q", &format!("{} {t}", first_artist(artist)))], &t, duration).unwrap_or_default()
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

/// Index of the line being sung at `pos`; None before the first line.
pub(super) fn current_index(lyrics: &Lyrics, pos: f64) -> Option<usize> {
    lyrics.iter().rposition(|(t, _)| *t <= pos)
}

#[cfg(test)]
pub(super) fn current_line(lyrics: &Lyrics, pos: f64) -> &str {
    current_index(lyrics, pos).map_or("", |i| lyrics[i].1.as_str())
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

    fn cand(_artist: &str, title: &str, duration: f64, synced: bool) -> Cand {
        Cand { title: title.into(), duration, synced: synced.then(|| "[00:01.00]x".to_string()) }
    }

    #[test]
    fn picks_the_version_whose_length_matches_the_track() {
        let c = [cand("A", "T", 293.0, true), cand("A", "T", 299.0, true), cand("A", "T", 254.0, false)];
        assert_eq!(pick_best(&c, 298.0), Some(1));
        assert_eq!(pick_best(&c, 293.4), Some(0));
        assert_eq!(pick_best(&c, 0.0), Some(0)); // length unknown: the first with synced lyrics
        assert_eq!(pick_best(&c, 200.0), None); // a different edit: wrong timing is worse than none
        assert_eq!(pick_best(&[cand("A", "T", 254.0, false)], 254.0), None); // no synced lyrics
    }

    #[test]
    fn lookup_names_drop_features_and_youtube_decoration() {
        let q = lyric_queries("Bassbaba Sumon, Anila", "Gaibona (feat. Fuad)");
        assert_eq!(q[0], ("Bassbaba Sumon, Anila".to_string(), "Gaibona (feat. Fuad)".to_string()));
        assert!(q.contains(&("Bassbaba Sumon".to_string(), "Gaibona".to_string())));
        // YouTube: the channel is the artist and the title carries "Artist - Song (Official Video)"
        let q = lyric_queries("G Series Music", "Bassbaba Sumon - Gaibona (Official Video)");
        assert!(q.contains(&("Bassbaba Sumon".to_string(), "Gaibona".to_string())));
        // nothing to clean: one query
        assert_eq!(lyric_queries("Queen", "Bohemian Rhapsody").len(), 1);
    }

    #[test]
    fn current_index_is_minus_one_before_the_first_line() {
        let l = parse_lrc("[00:05.00]a
[00:10.00]b
[00:10.00]c");
        assert_eq!(current_index(&l, 1.0), None);
        assert_eq!(current_index(&l, 5.0), Some(0));
        assert_eq!(current_index(&l, 10.5), Some(2)); // same stamp twice: the later line
        assert_eq!(current_line(&l, 1.0), "");
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

    #[test]
    fn art_data_uri_sniffs_and_caps() {
        assert!(art_data_uri(&[0xFF, 0xD8, 0xFF, 0xE0, 1, 2]).unwrap().starts_with("data:image/jpeg;base64,"));
        assert!(art_data_uri(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]).unwrap().starts_with("data:image/png;base64,"));
        assert!(art_data_uri(b"RIFF\0\0\0\0WEBPVP8 ").unwrap().starts_with("data:image/webp;base64,"));
        assert!(art_data_uri(b"hello").is_none());
        let mut big = vec![0xFF, 0xD8, 0xFF]; big.resize(512 * 1024 + 1, 0);
        assert!(art_data_uri(&big).is_none());
    }
    #[test]
    fn seek_bounds() {
        assert_eq!(validate_seek(30.0, 200.0), Ok(30.0));
        assert!(validate_seek(-1.0, 200.0).is_err());
        assert!(validate_seek(201.0, 200.0).is_err());
        assert!(validate_seek(f64::NAN, 200.0).is_err());
        assert!(validate_seek(10.0, 0.0).is_err()); // unknown duration → no seeking
    }
    #[test]
    fn art_retry_window() {
        assert!(should_retry_art(0, false));
        assert!(should_retry_art(5_000, false));
        assert!(!should_retry_art(5_001, false));
        assert!(!should_retry_art(1_000, true));
    }
}
