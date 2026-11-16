//! Local SQLite storage: key/value settings + tasks.
//! Schema version lives in `PRAGMA user_version`; add a new `if version < N` block to migrate.

use crate::clipboard::Clip;
use crate::notes::Note;
use crate::reminders::{Reminder, Repeat};
use crate::shelf;
use rusqlite::{params, Connection, OptionalExtension, Result};
use serde::Serialize;
use std::path::Path;

pub const CATEGORIES: [&str; 5] = ["Work", "Health", "Personal", "Study", "Other"];
pub const ANIMATION_STYLES: [&str; 3] = ["Glow Sweep", "Fluid Blobs", "Neon Border"];
pub const ISLAND_SIZES: [&str; 4] = ["Auto", "Small", "Medium", "Large"];

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Task {
    pub id: i64,
    pub name: String,
    pub category: String,
    pub time: String,
    pub color: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub location: String,
    pub lat: f64,
    pub lon: f64,
    pub island_style: String,
    pub animation_style: String,
    pub autostart: bool,
    /// `"primary"` or a monitor name (`Monitor::name()`).
    pub display: String,
    pub island_size: String,
    pub hide_fullscreen: bool,
    pub reminder_sound: bool,
    pub clip_remember: bool,
    pub clip_paused: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct App {
    pub id: i64,
    pub name: String,
    pub kind: String, // "path" | "action"
    pub target: String,
    pub icon: Option<String>,
}

pub fn category_color(category: &str) -> &'static str {
    match category {
        "Work" => "#00A0FF",
        "Health" => "#00FF80",
        "Personal" => "#FF5050",
        "Study" => "#FFB000",
        _ => "#A070FF",
    }
}

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA secure_delete = ON;")?; // deleted notes / clips must not linger in free pages
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let version: i64 = self.conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < 1 {
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 CREATE TABLE tasks (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     name TEXT NOT NULL,
                     category TEXT NOT NULL,
                     time TEXT NOT NULL,
                     color TEXT NOT NULL
                 );
                 INSERT INTO settings VALUES
                     ('location', 'Varanasi, India'), ('lat', '25.3333'), ('lon', '83.0'),
                     ('island_style', 'Liquid Glass'), ('animation_style', 'Fluid Blobs');
                 INSERT INTO tasks (name, category, time, color) VALUES
                     ('Project Sync', 'Work', '2:00 PM', '#00A0FF'),
                     ('Gym Session', 'Health', '5:30 PM', '#00FF80'),
                     ('Dinner with Family', 'Personal', '8:00 PM', '#FF5050');
                 PRAGMA user_version = 1;
                 COMMIT;",
            )?;
        }
        if version < 2 {
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE apps (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     name TEXT NOT NULL,
                     kind TEXT NOT NULL CHECK (kind IN ('path','action')),
                     target TEXT NOT NULL,
                     icon TEXT,
                     position INTEGER NOT NULL
                 );
                 INSERT INTO apps (name, kind, target, position) VALUES
                     ('Shutdown', 'action', 'shutdown', 1), ('Restart', 'action', 'restart', 2),
                     ('Sleep', 'action', 'sleep', 3), ('File Explorer', 'action', 'explorer', 4),
                     ('Settings', 'action', 'settings', 5), ('Task Manager', 'action', 'taskmgr', 6),
                     ('Chrome', 'action', 'chrome', 7), ('YouTube', 'action', 'youtube', 8),
                     ('CMD', 'action', 'cmd', 9);
                 UPDATE settings SET value = 'Liquid Glass' WHERE key = 'island_style' AND value = 'Default';
                 PRAGMA user_version = 2;
                 COMMIT;",
            )?;
        }
        if version < 3 {
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE notes (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     body TEXT NOT NULL,
                     pinned INTEGER NOT NULL DEFAULT 0,
                     updated_at INTEGER NOT NULL
                 );
                 PRAGMA user_version = 3;
                 COMMIT;",
            )?;
        }
        if version < 4 {
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE reminders (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     text TEXT NOT NULL,
                     due_at INTEGER NOT NULL,
                     repeat TEXT NOT NULL DEFAULT 'none' CHECK (repeat IN ('none','daily','weekdays','weekly')),
                     done INTEGER NOT NULL DEFAULT 0,
                     snooze_until INTEGER,
                     done_at INTEGER
                 );
                 PRAGMA user_version = 4;
                 COMMIT;",
            )?;
        }
        if version < 5 {
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE clip_history (
                     id INTEGER PRIMARY KEY,
                     text TEXT NOT NULL,
                     pinned INTEGER NOT NULL DEFAULT 0,
                     at INTEGER NOT NULL
                 );
                 PRAGMA user_version = 5;
                 COMMIT;",
            )?;
        }
        if version < 6 {
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE shelf (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     path TEXT NOT NULL UNIQUE,
                     added_at INTEGER NOT NULL
                 );
                 PRAGMA user_version = 6;
                 COMMIT;",
            )?;
        }
        if version < 7 {
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS dictations (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     text TEXT NOT NULL,
                     app TEXT,
                     at INTEGER NOT NULL,
                     words INTEGER NOT NULL,
                     ms INTEGER NOT NULL
                 );
                 PRAGMA user_version = 7;
                 COMMIT;",
            )?;
        }
        if version < 8 {
            let has: i64 = self.conn.query_row("SELECT COUNT(*) FROM pragma_table_info('notes') WHERE name = 'is_call'", [], |r| r.get(0))?;
            let add = if has == 0 { "ALTER TABLE notes ADD COLUMN is_call INTEGER NOT NULL DEFAULT 0;" } else { "" };
            self.conn.execute_batch(&format!("BEGIN; {add} PRAGMA user_version = 8; COMMIT;"))?;
        }
        Ok(())
    }

    /// `at` = ms since 1970; `ms` = how long the user spoke.
    pub fn add_dictation(&self, text: &str, app: Option<&str>, at: i64, words: i64, ms: i64) -> Result<i64> {
        self.conn.execute("INSERT INTO dictations (text, app, at, words, ms) VALUES (?1, ?2, ?3, ?4, ?5)", params![text, app, at, words, ms])?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Newest first, at most `limit`. `query` is a literal, case-insensitive substring of the text or app.
    pub fn dictations(&self, query: &str, limit: i64) -> Result<Vec<crate::dictation::text::Entry>> {
        let like = format!("%{}%", query.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        let mut stmt = self.conn.prepare(
            "SELECT id, text, app, at, words, ms FROM dictations WHERE text LIKE ?1 ESCAPE '\\' OR app LIKE ?1 ESCAPE '\\'
             ORDER BY at DESC, id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![like, limit], |r| {
            Ok(crate::dictation::text::Entry { id: r.get(0)?, text: r.get(1)?, app: r.get(2)?, at: r.get(3)?, words: r.get(4)?, ms: r.get(5)? })
        })?;
        rows.collect()
    }

    pub fn delete_dictation(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM dictations WHERE id = ?1", [id])?;
        Ok(())
    }

    /// (words, ms, at, app) of every dictation, for the stats.
    pub fn dictation_rows(&self) -> Result<Vec<crate::dictation::text::Row>> {
        let mut stmt = self.conn.prepare("SELECT words, ms, at, app FROM dictations")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        rows.collect()
    }

    fn app_row(r: &rusqlite::Row) -> Result<App> {
        Ok(App { id: r.get(0)?, name: r.get(1)?, kind: r.get(2)?, target: r.get(3)?, icon: r.get(4)? })
    }

    pub fn apps(&self) -> Result<Vec<App>> {
        let mut stmt = self.conn.prepare("SELECT id, name, kind, target, icon FROM apps ORDER BY position, id")?;
        let rows = stmt.query_map([], Self::app_row)?;
        rows.collect()
    }

    pub fn app(&self, id: i64) -> Result<Option<App>> {
        self.conn
            .query_row("SELECT id, name, kind, target, icon FROM apps WHERE id = ?1", [id], Self::app_row)
            .optional()
    }

    pub fn add_app(&self, name: &str, kind: &str, target: &str, icon: Option<&str>) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO apps (name, kind, target, icon, position)
             VALUES (?1, ?2, ?3, ?4, (SELECT COALESCE(MAX(position), 0) + 1 FROM apps))",
            params![name, kind, target, icon],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn remove_app(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM apps WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Pinned first, then newest. `query` is a literal, case-insensitive substring.
    pub fn notes(&self, query: &str) -> Result<Vec<Note>> {
        let like = format!("%{}%", query.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        let mut stmt = self.conn.prepare(
            "SELECT id, body, pinned, updated_at, is_call FROM notes WHERE body LIKE ?1 ESCAPE '\\'
             ORDER BY pinned DESC, updated_at DESC, id DESC",
        )?;
        let rows = stmt.query_map([like], |r| {
            Ok(Note { id: r.get(0)?, body: r.get(1)?, pinned: r.get::<_, i64>(2)? != 0, updated_at: r.get(3)?, call: r.get::<_, i64>(4)? != 0 })
        })?;
        rows.collect()
    }

    pub fn add_note(&self, body: &str) -> Result<i64> {
        self.conn.execute("INSERT INTO notes (body, updated_at) VALUES (?1, strftime('%s','now'))", [body])?;
        Ok(self.conn.last_insert_rowid())
    }

    /// A note written by the notetaker (badged in the list).
    pub fn add_call_note(&self, body: &str) -> Result<i64> {
        self.conn.execute("INSERT INTO notes (body, updated_at, is_call) VALUES (?1, strftime('%s','now'), 1)", [body])?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_note(&self, id: i64, body: &str) -> Result<()> {
        self.conn.execute("UPDATE notes SET body = ?2, updated_at = strftime('%s','now') WHERE id = ?1", params![id, body])?;
        Ok(())
    }

    /// Pinning keeps `updated_at`, so unpinning returns the note to its old place.
    pub fn pin_note(&self, id: i64, pinned: bool) -> Result<()> {
        self.conn.execute("UPDATE notes SET pinned = ?2 WHERE id = ?1", params![id, pinned])?;
        Ok(())
    }

    pub fn delete_note(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM notes WHERE id = ?1", [id])?;
        Ok(())
    }

    const REMINDER_COLS: &'static str = "SELECT id, text, due_at, repeat, done, snooze_until, done_at FROM reminders";

    fn reminder_row(r: &rusqlite::Row) -> Result<Reminder> {
        Ok(Reminder {
            id: r.get(0)?,
            text: r.get(1)?,
            due_at: r.get(2)?,
            repeat: Repeat::parse(&r.get::<_, String>(3)?),
            done: r.get::<_, i64>(4)? != 0,
            snooze_until: r.get(5)?,
            done_at: r.get(6)?,
        })
    }

    /// Upcoming (not done) soonest first, then those done at or after `done_since` (epoch ms).
    pub fn reminders(&self, done_since: i64) -> Result<Vec<Reminder>> {
        let sql = format!(
            "{} WHERE done = 0 OR done_at >= ?1 ORDER BY done, COALESCE(snooze_until, due_at), id",
            Self::REMINDER_COLS
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([done_since], Self::reminder_row)?;
        rows.collect()
    }

    pub fn reminder(&self, id: i64) -> Result<Option<Reminder>> {
        self.conn.query_row(&format!("{} WHERE id = ?1", Self::REMINDER_COLS), [id], Self::reminder_row).optional()
    }

    pub fn add_reminder(&self, text: &str, due_at: i64, repeat: Repeat) -> Result<i64> {
        self.conn.execute("INSERT INTO reminders (text, due_at, repeat) VALUES (?1, ?2, ?3)", params![text, due_at, repeat.as_str()])?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn snooze_reminder(&self, id: i64, until: i64) -> Result<()> {
        self.conn.execute("UPDATE reminders SET snooze_until = ?2 WHERE id = ?1", params![id, until])?;
        Ok(())
    }

    /// Done: a repeat moves to `next`, a one-off (`next` = None) is marked done. Either way the snooze ends.
    pub fn finish_reminder(&self, id: i64, next: Option<i64>, now: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE reminders SET due_at = COALESCE(?2, due_at), done = (?2 IS NULL), snooze_until = NULL, done_at = ?3 WHERE id = ?1",
            params![id, next, now],
        )?;
        Ok(())
    }

    pub fn delete_reminder(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM reminders WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn clips(&self) -> Result<Vec<Clip>> {
        let mut stmt = self.conn.prepare("SELECT id, text, pinned, at FROM clip_history ORDER BY at DESC, id DESC")?;
        let rows = stmt.query_map([], |r| Ok(Clip { id: r.get(0)?, text: r.get(1)?, pinned: r.get::<_, i64>(2)? != 0, at: r.get(3)? }))?;
        rows.collect()
    }

    /// Replace the stored history (at most ~50 rows).
    pub fn save_clips(&self, clips: &[Clip]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM clip_history", [])?;
        for c in clips {
            tx.execute("INSERT INTO clip_history (id, text, pinned, at) VALUES (?1, ?2, ?3, ?4)", params![c.id, c.text, c.pinned, c.at])?;
        }
        tx.commit()
    }

    /// Newest first. Missing files stay listed (flagged), so a removable drive coming back just works.
    pub fn shelf_rows(&self) -> Result<Vec<(i64, String)>> {
        let mut stmt = self.conn.prepare("SELECT id, path FROM shelf ORDER BY id DESC")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        rows.collect()
    }

    #[cfg(test)]
    pub fn shelf(&self) -> Result<Vec<shelf::ShelfItem>> {
        Ok(self.shelf_rows()?.into_iter().map(|(id, path)| shelf::ShelfItem::new(id, path)).collect())
    }

    pub fn shelf_path(&self, id: i64) -> Result<Option<String>> {
        self.conn.query_row("SELECT path FROM shelf WHERE id = ?1", [id], |r| r.get(0)).optional()
    }

    /// Validates the path (existing, local); a path already on the shelf returns its id. Keeps the newest `shelf::CAP`.
    pub fn add_shelf(&self, path: &str) -> Result<i64> {
        shelf::validate(path).map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))?;
        if let Some((id, _)) = self.shelf_rows()?.into_iter().find(|(_, p)| shelf::same_path(p, path)) {
            return Ok(id);
        }
        self.conn.execute("INSERT OR IGNORE INTO shelf (path, added_at) VALUES (?1, ?2)", params![path, chrono::Utc::now().timestamp_millis()])?;
        self.conn.execute("DELETE FROM shelf WHERE id NOT IN (SELECT id FROM shelf ORDER BY id DESC LIMIT ?1)", [shelf::CAP])?;
        self.conn.query_row("SELECT id FROM shelf WHERE path = ?1", [path], |r| r.get(0))
    }

    pub fn remove_shelf(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM shelf WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn clear_shelf(&self) -> Result<()> {
        self.conn.execute("DELETE FROM shelf", [])?;
        Ok(())
    }

    pub fn bridge_port(&self) -> Result<u16> {
        Ok(self.get("bridge_port")?.and_then(|v| v.parse().ok()).unwrap_or(47863))
    }

    pub fn bridge_token(&self, generate: impl FnOnce() -> String) -> Result<String> {
        if let Some(t) = self.get("bridge_token")? {
            return Ok(t);
        }
        let t = generate();
        self.set("bridge_token", &t)?;
        Ok(t)
    }

    pub fn get(&self, key: &str) -> Result<Option<String>> {
        self.conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
            .optional()
    }

    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// `autostart` lives in the registry, not here; the caller fills it in.
    pub fn settings(&self) -> Result<Settings> {
        let text = |k: &str, d: &str| -> Result<String> { Ok(self.get(k)?.unwrap_or_else(|| d.into())) };
        let num = |k: &str, d: f64| -> Result<f64> { Ok(self.get(k)?.and_then(|v| v.parse().ok()).unwrap_or(d)) };
        Ok(Settings {
            location: text("location", "Varanasi, India")?,
            lat: num("lat", 25.3333)?,
            lon: num("lon", 83.0)?,
            island_style: text("island_style", "Liquid Glass")?,
            animation_style: text("animation_style", "Fluid Blobs")?,
            autostart: false,
            display: text("display", "primary")?,
            island_size: text("island_size", "Auto")?,
            hide_fullscreen: text("hide_fullscreen", "true")? != "false",
            reminder_sound: text("reminder_sound", "true")? != "false",
            clip_remember: text("clip_remember", "false")? == "true",
            clip_paused: text("clip_paused", "false")? == "true",
        })
    }

    pub fn set_location(&self, city: &str, lat: f64, lon: f64) -> Result<()> {
        self.set("location", city)?;
        self.set("lat", &lat.to_string())?;
        self.set("lon", &lon.to_string())
    }

    pub fn tasks(&self) -> Result<Vec<Task>> {
        let mut stmt = self.conn.prepare("SELECT id, name, category, time, color FROM tasks ORDER BY id")?;
        let rows = stmt.query_map([], |r| {
            Ok(Task { id: r.get(0)?, name: r.get(1)?, category: r.get(2)?, time: r.get(3)?, color: r.get(4)? })
        })?;
        rows.collect()
    }

    pub fn add_task(&self, name: &str, category: &str, time: &str) -> Result<()> {
        let category = if CATEGORIES.contains(&category) { category } else { "Other" };
        self.conn.execute(
            "INSERT INTO tasks (name, category, time, color) VALUES (?1, ?2, ?3, ?4)",
            params![name, category, time, category_color(category)],
        )?;
        Ok(())
    }

    pub fn delete_task(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM tasks WHERE id = ?1", [id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_db_is_seeded_and_migration_is_idempotent() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.tasks().unwrap().len(), 3);
        db.migrate().unwrap(); // second run must not re-seed
        assert_eq!(db.tasks().unwrap().len(), 3);
        let s = db.settings().unwrap();
        assert_eq!(s.island_style, "Liquid Glass");
        assert_eq!(s.animation_style, "Fluid Blobs");
        assert_eq!((s.display.as_str(), s.island_size.as_str(), s.hide_fullscreen, s.reminder_sound), ("primary", "Auto", true, true));
    }

    fn v1_conn(style: &str) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&format!(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE tasks (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
                 category TEXT NOT NULL, time TEXT NOT NULL, color TEXT NOT NULL);
             INSERT INTO settings VALUES ('island_style', '{style}');
             PRAGMA user_version = 1;"
        ))
        .unwrap();
        conn
    }

    #[test]
    fn v2_seeds_basics_as_action_apps() {
        let db = Db::open_in_memory().unwrap();
        let apps = db.apps().unwrap();
        let ids: Vec<_> = apps.iter().map(|a| a.target.as_str()).collect();
        assert_eq!(ids, ["shutdown", "restart", "sleep", "explorer", "settings", "taskmgr", "chrome", "youtube", "cmd"]);
        assert!(apps.iter().all(|a| a.kind == "action"));
        db.migrate().unwrap();
        assert_eq!(db.apps().unwrap().len(), 9); // idempotent
    }

    #[test]
    fn apps_add_get_remove_keep_order() {
        let db = Db::open_in_memory().unwrap();
        let id = db.add_app("My App", "path", r"C:\R&D Tools\app.exe", None).unwrap();
        let a = db.app(id).unwrap().unwrap();
        assert_eq!((a.name.as_str(), a.kind.as_str()), ("My App", "path"));
        assert_eq!(db.apps().unwrap().last().unwrap().id, id);
        db.remove_app(id).unwrap();
        assert!(db.app(id).unwrap().is_none());
        assert!(db.app(9999).unwrap().is_none());
    }

    #[test]
    fn glass_is_default_style_and_v1_default_upgrades() {
        assert_eq!(Db::open_in_memory().unwrap().settings().unwrap().island_style, "Liquid Glass");
        // a v1 DB still on the old default moves to glass; a deliberate choice is kept.
        assert_eq!(Db::init(v1_conn("Notch Nook")).unwrap().settings().unwrap().island_style, "Notch Nook");
        assert_eq!(Db::init(v1_conn("Default")).unwrap().settings().unwrap().island_style, "Liquid Glass");
    }

    #[test]
    fn bridge_token_generated_once_and_port_default() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.bridge_port().unwrap(), 47863);
        let t = db.bridge_token(|| "abc".into()).unwrap();
        assert_eq!(db.bridge_token(|| "other".into()).unwrap(), t);
    }

    #[test]
    fn settings_upsert_and_location() {
        let db = Db::open_in_memory().unwrap();
        db.set("island_style", "Notch Nook").unwrap();
        db.set_location("Dhaka, Bangladesh", 23.81, 90.41).unwrap();
        let s = db.settings().unwrap();
        assert_eq!(s.island_style, "Notch Nook");
        assert_eq!(s.location, "Dhaka, Bangladesh");
        assert!((s.lat - 23.81).abs() < 1e-9);
    }

    #[test]
    fn tasks_add_delete_and_category_color() {
        let db = Db::open_in_memory().unwrap();
        db.add_task("Read", "Study", "9:00 PM").unwrap();
        db.add_task("Weird", "NotACategory", "N/A").unwrap();
        let tasks = db.tasks().unwrap();
        assert_eq!(tasks.len(), 5);
        assert_eq!(tasks[3].color, "#FFB000");
        assert_eq!(tasks[4].category, "Other");
        db.delete_task(tasks[0].id).unwrap();
        assert_eq!(db.tasks().unwrap().len(), 4);
    }

    fn v2_conn() -> Connection {
        let conn = Db::open_in_memory().unwrap().conn;
        conn.execute_batch("DROP TABLE notes; DROP TABLE reminders; DROP TABLE clip_history; DROP TABLE shelf; PRAGMA user_version = 2;").unwrap();
        conn
    }

    #[test]
    fn notes_crud_and_sort_order() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.notes("").unwrap().is_empty());
        let (a, b) = (db.add_note("first").unwrap(), db.add_note("second").unwrap());
        db.conn.execute("UPDATE notes SET updated_at = id", []).unwrap(); // deterministic clock: b newer
        assert_eq!(db.notes("").unwrap().iter().map(|n| n.id).collect::<Vec<_>>(), [b, a]);
        db.pin_note(a, true).unwrap();
        assert_eq!(db.notes("").unwrap()[0].id, a); // pinned first
        db.update_note(b, "second!").unwrap();
        let n = db.notes("").unwrap().into_iter().find(|n| n.id == b).unwrap();
        assert_eq!((n.body.as_str(), n.pinned), ("second!", false));
        assert!(n.updated_at > 1_000_000);
        db.delete_note(a).unwrap();
        assert_eq!(db.notes("").unwrap().len(), 1);
    }

    #[test]
    fn notes_search_is_literal_and_case_insensitive() {
        let db = Db::open_in_memory().unwrap();
        for b in ["100% done", "a_b", "axb", "Hello World", r"back\slash"] {
            db.add_note(b).unwrap();
        }
        let hits = |q: &str| db.notes(q).unwrap().into_iter().map(|n| n.body).collect::<Vec<_>>();
        assert_eq!(hits("%"), ["100% done"]);
        assert_eq!(hits("a_b"), ["a_b"]); // _ is not a wildcard
        assert_eq!(hits("hello wor"), ["Hello World"]);
        assert_eq!(hits("\\"), [r"back\slash"]);
        assert_eq!(hits("").len(), 5);
    }

    fn v3_conn() -> Connection {
        let conn = Db::open_in_memory().unwrap().conn;
        conn.execute_batch("DROP TABLE reminders; DROP TABLE clip_history; DROP TABLE shelf; PRAGMA user_version = 3;").unwrap();
        conn
    }

    #[test]
    fn reminders_crud_snooze_and_finish() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.reminders(0).unwrap().is_empty());
        let a = db.add_reminder("Stand-up", 2_000, Repeat::Daily).unwrap();
        let b = db.add_reminder("Call", 1_000, Repeat::None).unwrap();
        let ids = |since| db.reminders(since).unwrap().into_iter().map(|r| r.id).collect::<Vec<_>>();
        assert_eq!(ids(0), [b, a]); // soonest first
        db.snooze_reminder(b, 5_000).unwrap();
        let r = db.reminder(b).unwrap().unwrap();
        assert_eq!((r.due_at, r.snooze_until, r.at()), (1_000, Some(5_000), 5_000));
        assert_eq!(ids(0), [a, b]);
        // repeating: rolls forward, snooze cleared, stays upcoming
        db.snooze_reminder(a, 3_000).unwrap();
        db.finish_reminder(a, Some(9_000), 4_000).unwrap();
        let r = db.reminder(a).unwrap().unwrap();
        assert_eq!((r.due_at, r.snooze_until, r.done, r.done_at, r.repeat), (9_000, None, false, Some(4_000), Repeat::Daily));
        // one-off: marked done; listed only when done since the cut-off
        db.finish_reminder(b, None, 6_000).unwrap();
        let r = db.reminder(b).unwrap().unwrap();
        assert!(r.done && r.done_at == Some(6_000) && r.snooze_until.is_none());
        assert_eq!(ids(6_000), [a, b]);
        assert_eq!(ids(6_001), [a]);
        assert_eq!(ids(i64::MAX), [a]); // the scheduler's view: not done only
        db.delete_reminder(a).unwrap();
        assert!(db.reminder(a).unwrap().is_none());
        assert!(db.reminder(9999).unwrap().is_none());
    }

    #[test]
    fn v4_upgrades_to_v5_keeping_reminders() {
        let conn = Db::open_in_memory().unwrap().conn;
        conn.execute_batch("DROP TABLE clip_history; DROP TABLE shelf; PRAGMA user_version = 4;").unwrap();
        let db = Db::init(conn).unwrap();
        db.add_reminder("x", 1, Repeat::None).unwrap();
        db.save_clips(&[Clip { id: 7, text: "t".into(), pinned: true, at: 1 }]).unwrap();
        db.migrate().unwrap();
        assert_eq!((db.reminders(0).unwrap().len(), db.clips().unwrap().len()), (1, 1));
        assert!(db.clips().unwrap()[0].pinned);
    }

    #[test]
    fn v5_upgrades_to_v6_adding_shelf() {
        let conn = Db::open_in_memory().unwrap().conn;
        conn.execute_batch("DROP TABLE shelf; PRAGMA user_version = 5;").unwrap();
        let db = Db::init(conn).unwrap();
        assert!(db.shelf().unwrap().is_empty());
        db.save_clips(&[Clip { id: 1, text: "kept".into(), pinned: false, at: 1 }]).unwrap();
        db.migrate().unwrap(); // re-running is a no-op
        assert_eq!(db.clips().unwrap().len(), 1);
    }

    #[test]
    fn v3_upgrades_to_v4_keeping_notes() {
        let conn = v3_conn();
        conn.execute("INSERT INTO notes (body, updated_at) VALUES ('kept', 1)", []).unwrap();
        let db = Db::init(conn).unwrap();
        assert_eq!(db.notes("").unwrap().len(), 1);
        db.add_reminder("x", 1, Repeat::Weekly).unwrap();
        db.migrate().unwrap();
        assert_eq!(db.reminders(0).unwrap().len(), 1);
        assert!(db.conn.execute("INSERT INTO reminders (text, due_at, repeat) VALUES ('y', 1, 'hourly')", []).is_err());
    }

    #[test]
    fn v2_upgrades_to_v3_keeping_apps() {
        let db = Db::init(v2_conn()).unwrap();
        assert_eq!(db.apps().unwrap().len(), 9);
        assert!(db.add_note("x").is_ok());
        db.migrate().unwrap();
        assert_eq!(db.notes("").unwrap().len(), 1);
    }

    #[test]
    fn v6_upgrades_to_v7_adding_dictations_with_search_and_delete() {
        let conn = Db::open_in_memory().unwrap().conn;
        conn.execute_batch("DROP TABLE dictations; PRAGMA user_version = 6;").unwrap();
        let db = Db::init(conn).unwrap();
        let a = db.add_dictation("hello world", Some("Notepad"), 1_000, 2, 1_500).unwrap();
        db.add_dictation("100% sure_thing", None, 2_000, 2, 900).unwrap();
        assert_eq!(db.dictations("", 50).unwrap().iter().map(|e| e.at).collect::<Vec<_>>(), [2_000, 1_000]);
        assert_eq!(db.dictations("WORLD", 50).unwrap().len(), 1);
        assert_eq!(db.dictations("notepad", 50).unwrap()[0].id, a);
        assert_eq!(db.dictations("0%", 50).unwrap().len(), 1); // % is literal
        assert_eq!(db.dictations("o_w", 50).unwrap().len(), 0); // _ is literal
        assert_eq!(db.dictations("", 1).unwrap().len(), 1);
        db.delete_dictation(a).unwrap();
        assert_eq!(db.dictation_rows().unwrap(), vec![(2, 900, 2_000, None)]);
        db.migrate().unwrap();
    }

    #[test]
    fn v7_upgrades_to_v8_marking_call_notes() {
        let conn = Db::open_in_memory().unwrap().conn;
        conn.execute_batch("ALTER TABLE notes DROP COLUMN is_call; PRAGMA user_version = 7;").unwrap();
        conn.execute("INSERT INTO notes (body, updated_at) VALUES ('old', 1)", []).unwrap();
        let db = Db::init(conn).unwrap();
        let call = db.add_call_note("Call notes").unwrap();
        let notes = db.notes("").unwrap();
        assert_eq!(notes.iter().map(|n| (n.body.as_str(), n.call)).collect::<Vec<_>>(), [("Call notes", true), ("old", false)]);
        db.update_note(call, "edited").unwrap();
        assert!(db.notes("edited").unwrap()[0].call); // editing keeps the badge
        db.migrate().unwrap();
    }
}
