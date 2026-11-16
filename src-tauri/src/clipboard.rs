//! Clipboard history (spec §23.3): text only, in memory unless "Remember across restarts" is on.

use crate::db::Db;
use crate::AppState;
use serde::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

#[cfg(windows)]
#[path = "clipboard_windows.rs"]
pub(crate) mod os;
#[cfg(target_os = "linux")]
#[path = "clipboard_linux.rs"]
mod os;

pub const CAP: usize = 50;
pub const MAX_CHARS: usize = 10_000;
const MARK: &str = "…[truncated]";

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    pub id: i64,
    pub text: String,
    pub pinned: bool,
    pub at: i64,
}

/// Newest first; `list()` puts pinned entries on top.
#[derive(Default)]
pub struct ClipStore {
    clips: Vec<Clip>,
    next_id: i64,
}

impl ClipStore {
    pub fn from_saved(mut clips: Vec<Clip>) -> Self {
        clips.sort_by_key(|c| std::cmp::Reverse((c.at, c.id)));
        let next_id = clips.iter().map(|c| c.id).max().unwrap_or(0) + 1;
        Self { clips, next_id }
    }

    /// False when ignored (blank). A repeat of an existing entry moves it to the top.
    pub fn push(&mut self, text: String, now: i64) -> bool {
        if text.trim().is_empty() {
            return false;
        }
        let text = if text.chars().count() > MAX_CHARS { text.chars().take(MAX_CHARS).chain(MARK.chars()).collect() } else { text };
        let (id, pinned) = match self.clips.iter().position(|c| c.text == text) {
            Some(i) => {
                let c = self.clips.remove(i);
                (c.id, c.pinned)
            }
            None => {
                self.next_id = self.next_id.max(1);
                self.next_id += 1;
                (self.next_id - 1, false)
            }
        };
        self.clips.insert(0, Clip { id, text, pinned, at: now });
        let mut unpinned = 0;
        self.clips.retain(|c| {
            unpinned += usize::from(!c.pinned);
            c.pinned || unpinned <= CAP
        });
        true
    }

    pub fn pin(&mut self, id: i64, pinned: bool) {
        if let Some(c) = self.clips.iter_mut().find(|c| c.id == id) {
            c.pinned = pinned;
        }
    }

    pub fn remove(&mut self, id: i64) {
        self.clips.retain(|c| c.id != id);
    }

    pub fn clear(&mut self) {
        self.clips.retain(|c| c.pinned);
    }

    pub fn list(&self) -> Vec<Clip> {
        let (mut pinned, rest): (Vec<Clip>, Vec<Clip>) = self.clips.iter().cloned().partition(|c| c.pinned);
        pinned.extend(rest);
        pinned
    }

    pub fn text_of(&self, id: i64) -> Option<String> {
        self.clips.iter().find(|c| c.id == id).map(|c| c.text.clone())
    }
}

/// Mirror the history to disk only while "Remember across restarts" is on; otherwise make sure nothing is left there.
pub fn persist(db: &Db, store: &ClipStore, remember: bool) {
    let list = if remember { store.list() } else { Vec::new() };
    if let Err(e) = db.save_clips(&list) {
        eprintln!("clipboard: saving history failed: {e}");
    }
}

/// Own writes still waiting to come back through the listener: (text hash, expiry).
static SUPPRESSED: Mutex<Vec<(u64, Instant)>> = Mutex::new(Vec::new());
const SUPPRESS_TTL: Duration = Duration::from_secs(2);
const SUPPRESS_MAX: usize = 8;

pub fn hash(text: &str) -> u64 {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

fn suppress_at(list: &mut Vec<(u64, Instant)>, text_hash: u64, now: Instant) {
    list.retain(|e| e.1 > now);
    if list.len() >= SUPPRESS_MAX {
        list.remove(0);
    }
    list.push((text_hash, now + SUPPRESS_TTL));
}

fn take_at(list: &mut Vec<(u64, Instant)>, text: &str, now: Instant) -> bool {
    list.retain(|e| e.1 > now);
    let h = hash(text);
    match list.iter().position(|e| e.0 == h) {
        Some(i) => {
            list.remove(i);
            true
        }
        None => false,
    }
}

/// The next clipboard change carrying this text (hash) within 2 s is our own write (copy-back, dictation): keep it out of history.
/// Several can be pending at once.
pub fn suppress_next(text_hash: u64) {
    suppress_at(&mut SUPPRESSED.lock().unwrap(), text_hash, Instant::now());
}

pub fn take_suppressed(text: &str) -> bool {
    take_at(&mut SUPPRESSED.lock().unwrap(), text, Instant::now())
}

pub fn remember(db: &Db) -> bool {
    db.get("clip_remember").ok().flatten().as_deref() == Some("true")
}

pub fn paused(db: &Db) -> bool {
    db.get("clip_paused").ok().flatten().as_deref() == Some("true")
}

/// Save + tell the UI.
pub fn changed(app: &AppHandle, db: &Db, store: &ClipStore) {
    if remember(db) {
        persist(db, store, true); // memory-only mode must not touch the DB on every copy
    }
    let _ = app.emit("clips", store.list());
}

fn on_text(app: &AppHandle, text: String) {
    let state = app.state::<AppState>();
    if take_suppressed(&text) {
        return;
    }
    let db = state.db.lock().unwrap();
    if paused(&db) {
        return;
    }
    let mut store = state.clips.lock().unwrap();
    if store.push(text, chrono::Utc::now().timestamp_millis()) {
        changed(app, &db, &store);
    }
}

pub fn spawn(app: AppHandle) {
    os::spawn(app.clone(), move |t| on_text(&app, t));
}

/// Put `text` on the clipboard without it re-entering history.
pub fn write(text: &str) -> Result<(), String> {
    suppress_next(hash(text));
    let r = os::write(text);
    if r.is_err() {
        take_suppressed(text); // the write failed: nothing will come back
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn texts(s: &ClipStore) -> Vec<String> {
        s.list().into_iter().map(|c| c.text).collect()
    }

    #[test]
    fn dedupe_moves_to_top_and_keeps_pin() {
        let mut s = ClipStore::default();
        assert!(s.push("a".into(), 1) && s.push("b".into(), 2));
        let a = s.list()[1].id;
        s.pin(a, true);
        assert!(s.push("a".into(), 3));
        assert_eq!(texts(&s), ["a", "b"]);
        let top = &s.list()[0];
        assert!(top.pinned && top.at == 3 && top.id == a);
        s.pin(a, false);
        s.push("a".into(), 4);
        assert_eq!(texts(&s), ["a", "b"]); // moved to top, no duplicate
    }

    #[test]
    fn cap_counts_unpinned_only() {
        let mut s = ClipStore::default();
        s.push("keep".into(), 0);
        let keep = s.list()[0].id;
        s.pin(keep, true);
        for i in 0..60 {
            s.push(format!("n{i}"), i + 1);
        }
        let l = s.list();
        assert_eq!(l.len(), 51);
        assert_eq!(l[0].text, "keep"); // pinned first
        assert_eq!(l[1].text, "n59");
        assert_eq!(l[50].text, "n10");
    }

    #[test]
    fn truncates_by_chars() {
        let mut s = ClipStore::default();
        s.push("é".repeat(10_000), 1);
        assert_eq!(s.list()[0].text.chars().count(), 10_000); // exactly the limit: untouched
        s.push("é".repeat(10_001), 2);
        let t = &s.list()[0].text;
        assert_eq!(t.chars().count(), 10_000 + "…[truncated]".chars().count());
        assert!(t.ends_with("…[truncated]"));
    }

    #[test]
    fn ignores_blank() {
        let mut s = ClipStore::default();
        assert!(!s.push("".into(), 1) && !s.push(" \n\t ".into(), 1));
        assert!(s.list().is_empty());
        assert!(s.push("  x ".into(), 2));
        assert_eq!(s.list()[0].text, "  x "); // stored as copied
    }

    #[test]
    fn remove_and_clear_keep_pinned() {
        let mut s = ClipStore::default();
        for (i, t) in ["a", "b", "c"].iter().enumerate() {
            s.push((*t).into(), i as i64);
        }
        let ids: Vec<i64> = s.list().iter().map(|c| c.id).collect(); // c, b, a
        s.pin(ids[2], true);
        s.remove(ids[0]);
        assert_eq!(texts(&s), ["a", "b"]);
        s.clear();
        assert_eq!(texts(&s), ["a"]);
        assert!(s.text_of(ids[2]).is_some() && s.text_of(ids[1]).is_none());
    }

    #[test]
    fn suppression_coexists_and_expires() {
        let t0 = Instant::now();
        let mut l = Vec::new();
        suppress_at(&mut l, hash("temp"), t0);
        suppress_at(&mut l, hash("restore"), t0);
        assert!(!take_at(&mut l, "other", t0));
        assert!(take_at(&mut l, "restore", t0) && take_at(&mut l, "temp", t0));
        assert!(!take_at(&mut l, "temp", t0)); // one-shot
        suppress_at(&mut l, hash("old"), t0);
        assert!(!take_at(&mut l, "old", t0 + SUPPRESS_TTL + Duration::from_millis(1))); // expired: a genuine copy is kept
        assert!(l.is_empty());
        for i in 0..20 {
            suppress_at(&mut l, i, t0);
        }
        assert_eq!(l.len(), SUPPRESS_MAX);
    }

    #[test]
    fn persistence_on_writes_db_off_wipes_it() {
        let db = Db::open_in_memory().unwrap();
        let mut s = ClipStore::default();
        s.push("one".into(), 1);
        s.push("two".into(), 2);
        let id = s.list()[0].id;
        s.pin(id, true);
        persist(&db, &s, false);
        assert!(db.clips().unwrap().is_empty()); // memory only by default
        persist(&db, &s, true);
        let mut back = ClipStore::from_saved(db.clips().unwrap());
        assert_eq!(texts(&back), ["two", "one"]);
        assert!(back.list()[0].pinned);
        back.push("three".into(), 3);
        assert!(back.list().iter().map(|c| c.id).collect::<std::collections::BTreeSet<_>>().len() == 3); // fresh ids
        persist(&db, &s, false); // turning it off wipes the table
        assert!(db.clips().unwrap().is_empty());
    }
}
