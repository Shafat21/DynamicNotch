//! Dictation settings (rows in the `settings` table) and hotkey parsing. Pure, unit-tested.

use crate::db::Db;
use serde::Serialize;
use serde_json::Value;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub win: bool,
}

impl Mods {
    fn count(&self) -> usize {
        [self.ctrl, self.shift, self.alt, self.win].iter().filter(|b| **b).count()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hotkey {
    pub mods: Mods,
    /// None = modifier-only combo (Windows low-level hook); Some = key for the global-shortcut plugin.
    pub key: Option<String>,
}

impl Hotkey {
    fn join(&self, win: &str) -> String {
        let m = self.mods;
        let parts = [(m.ctrl, "Ctrl"), (m.shift, "Shift"), (m.alt, "Alt"), (m.win, win)];
        let mut out: Vec<&str> = parts.iter().filter(|p| p.0).map(|p| p.1).collect();
        out.extend(self.key.as_deref());
        out.join("+")
    }

    /// Stored / displayed form, e.g. "Ctrl+Win".
    pub fn canonical(&self) -> String {
        self.join("Win")
    }

    /// Accelerator string for tauri-plugin-global-shortcut.
    pub fn shortcut(&self) -> String {
        self.join("Super")
    }
}

/// "Ctrl+Win", "Ctrl+Shift+Space", "Alt+Q". Modifier-only combos need ≥ 2 modifiers, key combos ≥ 1.
pub fn parse_hotkey(s: &str) -> Result<Hotkey, String> {
    let bad = || format!("Invalid hotkey \"{s}\"");
    let mut mods = Mods::default();
    let mut key: Option<String> = None;
    for tok in s.split('+').map(str::trim) {
        let flag = match tok.to_ascii_lowercase().as_str() {
            "" => return Err(bad()),
            "ctrl" | "control" => &mut mods.ctrl,
            "shift" => &mut mods.shift,
            "alt" | "option" => &mut mods.alt,
            "win" | "super" | "meta" | "cmd" => &mut mods.win,
            _ => {
                if key.is_some() || tauri_plugin_global_shortcut::Shortcut::from_str(tok).is_err() {
                    return Err(bad());
                }
                let mut c = tok.chars();
                key = c.next().map(|f| f.to_uppercase().chain(c).collect()); // "space" → "Space", "q" → "Q"
                continue;
            }
        };
        if *flag {
            return Err(bad()); // duplicate modifier
        }
        *flag = true;
    }
    let need = if key.is_some() { 1 } else { 2 };
    if mods.count() < need {
        return Err(format!("{}: add a modifier (Ctrl, Shift, Alt or Win)", bad()));
    }
    Ok(Hotkey { mods, key })
}

pub const ENGINES: [&str; 2] = ["local", "cloud"];
pub const PROVIDERS: [&str; 2] = ["openai", "groq"];
pub const OUTPUTS: [&str; 2] = ["paste", "copy"];

pub fn default_cloud_model(provider: &str) -> &'static str {
    if provider == "groq" {
        "whisper-large-v3-turbo"
    } else {
        "whisper-1"
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub engine: String,
    pub model: String,
    pub language: String,
    pub hotkey: String,
    /// "" = system default microphone.
    pub mic: String,
    pub cloud_provider: String,
    pub cloud_model: String,
    pub output: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            engine: "local".into(),
            model: "base".into(),
            language: "auto".into(),
            // Linux has no modifier-only hook (spec §23.7: best effort)
            hotkey: if cfg!(windows) { "Ctrl+Win" } else { "Ctrl+Shift+Space" }.into(),
            mic: String::new(),
            cloud_provider: "openai".into(),
            cloud_model: default_cloud_model("openai").into(),
            output: "paste".into(),
        }
    }
}

/// (patch / JSON key, DB key)
const KEYS: [(&str, &str); 8] = [
    ("engine", "dictation_engine"),
    ("model", "dictation_model"),
    ("language", "dictation_language"),
    ("hotkey", "dictation_hotkey"),
    ("mic", "dictation_mic"),
    ("cloudProvider", "dictation_cloud_provider"),
    ("cloudModel", "dictation_cloud_model"),
    ("output", "dictation_output"),
];

impl Settings {
    fn field(&mut self, key: &str) -> &mut String {
        match key {
            "engine" => &mut self.engine,
            "model" => &mut self.model,
            "language" => &mut self.language,
            "hotkey" => &mut self.hotkey,
            "mic" => &mut self.mic,
            "cloudProvider" => &mut self.cloud_provider,
            "cloudModel" => &mut self.cloud_model,
            _ => &mut self.output,
        }
    }

    pub fn load(db: &Db) -> Self {
        let mut s = Self::default();
        for (k, dbk) in KEYS {
            if let Ok(Some(v)) = db.get(dbk) {
                *s.field(k) = v;
            }
        }
        s
    }

    pub fn save(&mut self, db: &Db) -> Result<(), String> {
        for (k, dbk) in KEYS {
            db.set(dbk, self.field(k)).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

fn one_of(v: &str, list: &[&str], what: &str) -> Result<(), String> {
    if list.contains(&v) {
        Ok(())
    } else {
        Err(format!("Unknown {what} \"{v}\""))
    }
}

/// Validate a partial update from the webview. `mics` is only called when a specific mic is chosen.
pub fn apply_patch(cur: &Settings, patch: &Value, mics: impl FnOnce() -> Vec<String>) -> Result<Settings, String> {
    let obj = patch.as_object().ok_or("Expected an object")?;
    let mut s = cur.clone();
    for (k, v) in obj {
        if !KEYS.iter().any(|(pk, _)| pk == k) {
            return Err(format!("Unknown setting \"{k}\""));
        }
        let v = v.as_str().ok_or_else(|| format!("\"{k}\" must be a string"))?.trim().to_string();
        *s.field(k) = v;
    }
    one_of(&s.engine, &ENGINES, "engine")?;
    if super::models::info(&s.model).is_none() {
        return Err(format!("Unknown model \"{}\"", s.model));
    }
    let l = &s.language;
    if l != "auto" && !(l.len() == 2 && l.bytes().all(|b| b.is_ascii_lowercase())) {
        return Err(format!("Unknown language \"{l}\""));
    }
    s.hotkey = parse_hotkey(&s.hotkey)?.canonical();
    one_of(&s.cloud_provider, &PROVIDERS, "cloud provider")?;
    if s.cloud_provider != cur.cloud_provider && !obj.contains_key("cloudModel") {
        s.cloud_model = default_cloud_model(&s.cloud_provider).into();
    }
    let n = s.cloud_model.chars().count();
    if n == 0 || n > 64 || s.cloud_model.chars().any(char::is_control) {
        return Err("Cloud model must be 1–64 characters".into());
    }
    one_of(&s.output, &OUTPUTS, "output")?;
    if obj.contains_key("mic") && !s.mic.is_empty() && !mics().contains(&s.mic) {
        return Err(format!("Microphone \"{}\" not found", s.mic));
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hotkey_parsing() {
        let h = parse_hotkey("Ctrl+Win").unwrap();
        assert_eq!(h.mods, Mods { ctrl: true, win: true, ..Default::default() });
        assert_eq!(h.key, None);
        assert_eq!(h.canonical(), "Ctrl+Win");
        let h = parse_hotkey(" ctrl + shift + space ").unwrap();
        assert_eq!(h.key.as_deref(), Some("Space"));
        assert_eq!(h.shortcut(), "Ctrl+Shift+Space");
        let h = parse_hotkey("Alt+q").unwrap();
        assert_eq!((h.canonical(), h.shortcut()), ("Alt+Q".to_string(), "Alt+Q".to_string()));
        assert_eq!(parse_hotkey("Win+Alt+F5").unwrap().shortcut(), "Alt+Super+F5");
        for bad in ["", "Ctrl", "Q", "Ctrl+", "Ctrl+Foo", "Ctrl+A+B", "Ctrl+Ctrl+A", "Space"] {
            assert!(parse_hotkey(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn validation() {
        let cur = Settings::default();
        let none = || -> Vec<String> { panic!("mic list not needed") };
        assert_eq!(apply_patch(&cur, &json!({}), none).unwrap(), cur);
        let s = apply_patch(&cur, &json!({"engine": "cloud", "model": "small", "language": "de", "hotkey": "ctrl+alt"}), none).unwrap();
        assert_eq!((s.engine.as_str(), s.model.as_str(), s.language.as_str(), s.hotkey.as_str()), ("cloud", "small", "de", "Ctrl+Alt"));
        for bad in [
            json!({"engine": "gpu"}),
            json!({"model": "large"}),
            json!({"language": "english"}),
            json!({"language": "EN"}),
            json!({"hotkey": "Q"}),
            json!({"cloudProvider": "azure"}),
            json!({"cloudModel": ""}),
            json!({"cloudModel": "x".repeat(65)}),
            json!({"output": "type"}),
            json!({"engine": 1}),
            json!({"apiKey": "sk-..."}),
            json!("cloud"),
        ] {
            assert!(apply_patch(&cur, &bad, none).is_err(), "{bad} should be rejected");
        }
        // switching provider resets the model to that provider's default unless one is given
        let s = apply_patch(&cur, &json!({"cloudProvider": "groq"}), none).unwrap();
        assert_eq!(s.cloud_model, "whisper-large-v3-turbo");
        let s = apply_patch(&cur, &json!({"cloudProvider": "groq", "cloudModel": "distil-whisper"}), none).unwrap();
        assert_eq!(s.cloud_model, "distil-whisper");
        // mic must exist; "" = default
        let mics = || vec!["USB Mic".to_string()];
        assert_eq!(apply_patch(&cur, &json!({"mic": "USB Mic"}), mics).unwrap().mic, "USB Mic");
        assert!(apply_patch(&cur, &json!({"mic": "Gone"}), mics).is_err());
        assert_eq!(apply_patch(&cur, &json!({"mic": ""}), none).unwrap().mic, "");
    }

    #[test]
    fn load_save_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(Settings::load(&db), Settings::default());
        let mut s = Settings { language: "fr".into(), output: "copy".into(), ..Default::default() };
        s.save(&db).unwrap();
        assert_eq!(Settings::load(&db), s);
        assert_eq!(db.get("dictation_output").unwrap().as_deref(), Some("copy"));
    }
}
