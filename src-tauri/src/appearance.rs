//! Validated, persisted appearance model (spec §22.3): theme, sizes, density, tab / Home-card layout.

use crate::db::Db;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Appearance {
    pub theme: String,
    pub accent: String,
    pub bg_mode: String,
    pub icon_style: String,
    pub badge_colors: BTreeMap<String, String>,
    pub shape: String,
    pub density: String,
    pub radius: f64,
    pub text_scale: f64,
    pub card_opacity: f64,
    pub tab_labels: String,
    pub open_on: String,
    pub collapsed_w: f64,
    pub collapsed_h: f64,
    pub open_w: f64,
    pub open_h: f64,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: "Midnight".into(),
            accent: String::new(),
            bg_mode: "glass".into(),
            icon_style: "line".into(),
            badge_colors: BTreeMap::new(),
            shape: "pill".into(),
            density: "comfortable".into(),
            radius: 20.0,
            text_scale: 1.0,
            card_opacity: 0.75,
            tab_labels: "always".into(),
            open_on: "home".into(),
            collapsed_w: 180.0,
            collapsed_h: 40.0,
            open_w: 760.0,
            open_h: 260.0,
        }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Theme {
    pub name: &'static str,
    pub bg_from: &'static str,
    pub bg_to: &'static str,
    pub card: &'static str,
    pub accent: &'static str,
}

const fn th(name: &'static str, bg_from: &'static str, bg_to: &'static str, card: &'static str, accent: &'static str) -> Theme {
    Theme { name, bg_from, bg_to, card, accent }
}

pub const THEMES: [Theme; 7] = [
    th("Graphite", "#1c1c1e", "#2a2a2d", "#38383c", "#4f8cff"),
    th("Midnight", "#1b2033", "#20263b", "#2c3450", "#8fb0ff"),
    th("Aurora", "#1a1636", "#12303a", "#26335a", "#4ade9a"),
    th("Ember", "#2a1512", "#3a1c14", "#4d2a20", "#ff8a3d"),
    th("Lime", "#1c2310", "#252e14", "#34401e", "#a3e635"),
    th("Rose", "#2b1320", "#3a1829", "#4d2538", "#ff6fa5"),
    th("Pure black", "#000000", "#000000", "#1a1a1a", "#4f8cff"),
];

pub const TAB_IDS: &[&str] = &["home", "music", "calendar", "pal", "timer", "launcher", "weather", "system", "tasks", "month"];
pub const HOME_IDS: &[&str] = &["clock", "pal", "music", "calendar", "launcher", "focus", "system", "tasks"];
const HOME_DEFAULT_VISIBLE: &[&str] = &["clock", "pal", "music", "calendar", "launcher", "focus"];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LayoutItem {
    pub id: String,
    pub visible: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Layout {
    pub tabs: Vec<LayoutItem>,
    pub home: Vec<LayoutItem>,
}

fn is_hex(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

fn one_of(v: String, allowed: &[&str], default: &str) -> String {
    if allowed.contains(&v.as_str()) { v } else { default.into() }
}

fn clamp(v: f64, lo: f64, hi: f64, default: f64) -> f64 {
    if v.is_finite() { v.clamp(lo, hi) } else { default }
}

pub fn sanitize(a: Appearance) -> Appearance {
    let d = Appearance::default();
    let names: Vec<&str> = THEMES.iter().map(|t| t.name).collect();
    Appearance {
        theme: one_of(a.theme, &names, &d.theme),
        accent: if is_hex(&a.accent) { a.accent } else { String::new() },
        bg_mode: one_of(a.bg_mode, &["solid", "glass"], &d.bg_mode),
        icon_style: one_of(a.icon_style, &["line", "badges"], &d.icon_style),
        badge_colors: a
            .badge_colors
            .into_iter()
            .filter(|(k, v)| (HOME_IDS.contains(&k.as_str()) || TAB_IDS.contains(&k.as_str()) || k == "settings") && is_hex(v))
            .collect(),
        shape: one_of(a.shape, &["pill", "notch"], &d.shape),
        density: one_of(a.density, &["compact", "comfortable", "airy"], &d.density),
        radius: clamp(a.radius, 8.0, 40.0, d.radius),
        text_scale: clamp(a.text_scale, 0.85, 1.25, d.text_scale),
        card_opacity: clamp(a.card_opacity, 0.3, 1.0, d.card_opacity),
        tab_labels: one_of(a.tab_labels, &["always", "active", "never"], &d.tab_labels),
        open_on: one_of(a.open_on, &["home", "last"], &d.open_on),
        collapsed_w: clamp(a.collapsed_w, 140.0, 320.0, d.collapsed_w),
        collapsed_h: clamp(a.collapsed_h, 28.0, 48.0, d.collapsed_h),
        open_w: clamp(a.open_w, 560.0, 1000.0, d.open_w),
        open_h: clamp(a.open_h, 200.0, 420.0, d.open_h),
    }
}

fn normalize_list(items: Vec<LayoutItem>, known: &[&str], default_visible: &[&str]) -> Vec<LayoutItem> {
    if items.is_empty() {
        return known.iter().map(|id| LayoutItem { id: (*id).into(), visible: default_visible.contains(id) }).collect();
    }
    let mut out: Vec<LayoutItem> = Vec::new();
    for it in items {
        if known.contains(&it.id.as_str()) && !out.iter().any(|o| o.id == it.id) {
            out.push(it);
        }
    }
    for id in known {
        if !out.iter().any(|o| o.id == *id) {
            out.push(LayoutItem { id: (*id).into(), visible: false });
        }
    }
    out
}

pub fn normalize_layout(l: Layout) -> Layout {
    Layout { tabs: normalize_list(l.tabs, TAB_IDS, TAB_IDS), home: normalize_list(l.home, HOME_IDS, HOME_DEFAULT_VISIBLE) }
}

/// Shallow merge of the provided camelCase fields; a field of the wrong type or an unknown key is ignored.
pub fn merge_patch(current: &Appearance, patch: &Value) -> Appearance {
    let mut cur = current.clone();
    if let Some(obj) = patch.as_object() {
        for (k, v) in obj {
            let mut base = serde_json::to_value(&cur).unwrap_or_default();
            if let Some(m) = base.as_object_mut() {
                if m.contains_key(k) {
                    m.insert(k.clone(), v.clone());
                    if let Ok(next) = serde_json::from_value(base) {
                        cur = next;
                    }
                }
            }
        }
    }
    sanitize(cur)
}

pub fn theme_for(a: &Appearance) -> &'static Theme {
    THEMES.iter().find(|t| t.name == a.theme).unwrap_or(&THEMES[1])
}

pub fn payload(a: &Appearance, l: &Layout) -> Value {
    // `themes` feeds the Settings window's swatches, so the presets live only here
    json!({ "appearance": a, "layout": l, "theme": theme_for(a), "themes": THEMES })
}

/// Stored JSON missing or malformed → defaults. With no stored appearance, migrate from `island_style`.
pub fn load(db: &Db) -> (Appearance, Layout) {
    let stored = db.get("appearance").ok().flatten();
    // per-field merge: one wrong-typed stored field must not reset the others
    let mut a = stored
        .as_deref()
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .map_or_else(Appearance::default, |v| merge_patch(&Appearance::default(), &v));
    if stored.is_none() {
        match db.get("island_style").ok().flatten().as_deref() {
            Some("Default") => a.bg_mode = "solid".into(),
            Some("Liquid Glass") => a.bg_mode = "glass".into(),
            Some("Notch Nook") => a.shape = "notch".into(),
            _ => {}
        }
    }
    let l = db.get("layout").ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    (sanitize(a), normalize_layout(l))
}

pub fn save(db: &Db, a: &Appearance, l: &Layout) -> Result<(), String> {
    let e = |e: &dyn std::fmt::Display| e.to_string();
    db.set("appearance", &serde_json::to_string(a).map_err(|x| e(&x))?).map_err(|x| e(&x))?;
    db.set("layout", &serde_json::to_string(l).map_err(|x| e(&x))?).map_err(|x| e(&x))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_sanitize() {
        let d = Appearance::default();
        assert_eq!((d.theme.as_str(), d.bg_mode.as_str(), d.icon_style.as_str()), ("Midnight", "glass", "line"));
        let bad = Appearance { theme: "Nope".into(), radius: 99.0, text_scale: 0.1, accent: "red".into(), density: "huge".into(), open_w: 5000.0, ..d.clone() };
        let s = sanitize(bad);
        assert_eq!(s.theme, "Midnight"); assert_eq!(s.radius, 40.0); assert_eq!(s.text_scale, 0.85);
        assert_eq!(s.accent, ""); assert_eq!(s.density, "comfortable"); assert_eq!(s.open_w, 1000.0);
    }
    #[test]
    fn patch_merges_and_validates() {
        let cur = Appearance::default();
        let next = merge_patch(&cur, &serde_json::json!({"theme": "Ember", "radius": 12, "badgeColors": {"music": "#1DB954", "focus": "nope"}, "unknown": 1}));
        assert_eq!(next.theme, "Ember"); assert_eq!(next.radius, 12.0);
        assert_eq!(next.badge_colors.get("music").map(String::as_str), Some("#1DB954"));
        assert!(!next.badge_colors.contains_key("focus"));
        assert_eq!(next.open_w, cur.open_w);
    }
    #[test]
    fn layout_normalizes() {
        let l = normalize_layout(Layout { tabs: vec![LayoutItem { id: "music".into(), visible: true }, LayoutItem { id: "bogus".into(), visible: true }, LayoutItem { id: "music".into(), visible: false }], home: vec![] });
        assert_eq!(l.tabs[0].id, "music");
        assert_eq!(l.tabs.len(), TAB_IDS.len());
        assert!(l.tabs.iter().all(|t| t.id != "bogus"));
        assert!(!l.tabs.iter().find(|t| t.id == "home").unwrap().visible); // appended → hidden
        assert_eq!(l.home.len(), HOME_IDS.len());
    }
    #[test]
    fn every_theme_has_valid_colours() {
        for t in THEMES { for c in [t.bg_from, t.bg_to, t.card, t.accent] { assert!(c.len() == 7 && c.starts_with('#'), "{} {}", t.name, c); } }
        assert!(THEMES.iter().any(|t| t.name == "Midnight"));
    }
    #[test]
    fn load_defaults_migration_and_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        db.set("island_style", "Notch Nook").unwrap();
        let (a, l) = load(&db);
        assert_eq!(a.shape, "notch");
        assert!(l.tabs.iter().all(|t| t.visible));
        assert_eq!(l.home.iter().filter(|h| h.visible).count(), 6);
        db.set("appearance", "{not json").unwrap();
        assert_eq!(load(&db).0, Appearance::default());
        let a2 = merge_patch(&a, &json!({"theme": "Rose"}));
        save(&db, &a2, &l).unwrap();
        assert_eq!(load(&db), (a2, l));
    }
    #[test]
    fn load_is_per_field() {
        let db = Db::open_in_memory().unwrap();
        db.set("appearance", r#"{"theme":"Ember","radius":"x","openW":99999,"textScale":null}"#).unwrap();
        let a = load(&db).0;
        assert_eq!((a.theme.as_str(), a.radius, a.open_w, a.text_scale), ("Ember", 20.0, 1000.0, 1.0));
        db.set("appearance", "[1]").unwrap();
        assert_eq!(load(&db).0, Appearance::default());
    }
    #[test]
    fn patch_with_one_bad_field_applies_others() {
        let next = merge_patch(&Appearance::default(), &json!({"radius": "x", "theme": "Rose", "density": 5}));
        assert_eq!((next.theme.as_str(), next.radius, next.density.as_str()), ("Rose", 20.0, "comfortable"));
    }
}
