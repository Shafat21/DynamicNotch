//! Local SQLite storage: key/value settings + tasks.
//! Schema version lives in `PRAGMA user_version`; add a new `if version < N` block to migrate.

use rusqlite::{params, Connection, OptionalExtension, Result};
use serde::Serialize;
use std::path::Path;

pub const CATEGORIES: [&str; 5] = ["Work", "Health", "Personal", "Study", "Other"];
pub const ISLAND_STYLES: [&str; 3] = ["Liquid Glass", "Default", "Notch Nook"];
pub const ANIMATION_STYLES: [&str; 3] = ["Glow Sweep", "Fluid Blobs", "Neon Border"];

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
        Ok(())
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
}
