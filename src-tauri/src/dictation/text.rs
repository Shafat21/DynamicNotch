//! What happens to the words before they are pasted (dictionary, replacements, snippets, tidy-up) and
//! the numbers of the stats strip. Pure, nothing here touches the OS.
//!
//! Portions adapted from Coucou (https://github.com/Louis-CFM/coucou), © 2026 Louis Raillé, MIT License.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

/// A phrase written another way: "cloud code" → "Claude Code".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Replacement {
    pub from: String,
    pub to: String,
}

/// Say the trigger, get the whole text: "my signature" → "Have a nice day,\nAlex".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snippet {
    pub trigger: String,
    pub text: String,
}

/// The user's dictionary (always-spell terms), replacements and snippets.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Words {
    pub words: Vec<String>,
    pub replacements: Vec<Replacement>,
    pub snippets: Vec<Snippet>,
}

const MAX_ENTRIES: usize = 500;
const MAX_PHRASE: usize = 100;
const MAX_SNIPPET: usize = 5_000;

fn check(s: &str, max: usize, what: &str) -> Result<String, String> {
    let s = s.trim();
    if s.is_empty() || s.chars().count() > max {
        return Err(format!("{what} must be 1–{max} characters"));
    }
    Ok(s.to_string())
}

impl Words {
    /// Validate input from the webview: trimmed, bounded, no empty entries.
    pub fn validated(&self) -> Result<Words, String> {
        if self.words.len() + self.replacements.len() + self.snippets.len() > MAX_ENTRIES {
            return Err(format!("At most {MAX_ENTRIES} entries"));
        }
        Ok(Words {
            words: self.words.iter().map(|w| check(w, MAX_PHRASE, "A dictionary word")).collect::<Result<_, _>>()?,
            replacements: self
                .replacements
                .iter()
                .map(|r| Ok(Replacement { from: check(&r.from, MAX_PHRASE, "A replacement")?, to: check(&r.to, MAX_PHRASE, "A replacement")? }))
                .collect::<Result<_, String>>()?,
            snippets: self
                .snippets
                .iter()
                .map(|s| Ok(Snippet { trigger: check(&s.trigger, MAX_PHRASE, "A snippet trigger")?, text: check(&s.text, MAX_SNIPPET, "A snippet text")? }))
                .collect::<Result<_, String>>()?,
        })
    }
}

/// A dictation ready to paste, its snippets still set aside: each trigger heard became a mark (⟦1⟧) so
/// nothing else (replacements, dictionary, Claude's clean-up) touches the snippet's own text.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    pub text: String,
    pub snippets: BTreeMap<String, String>,
}

/// Snippet triggers set aside first, then the replacements, then the dictionary's spelling.
pub fn prepare(text: &str, w: &Words) -> Prepared {
    let mut out = text.to_string();
    let mut marks = BTreeMap::new();
    for (i, s) in w.snippets.iter().enumerate() {
        if s.text.is_empty() {
            continue;
        }
        let mark = format!("⟦{}⟧", i + 1);
        let replaced = replacing(&s.trigger, &mark, &out);
        if replaced != out {
            marks.insert(mark, s.text.clone());
            out = replaced;
        }
    }
    for r in &w.replacements {
        if !r.to.is_empty() {
            out = replacing(&r.from, &r.to, &out);
        }
    }
    for word in &w.words {
        if is_plain_word(word) {
            out = replacing(word, word, &out);
        }
    }
    Prepared { text: out, snippets: marks }
}

/// The marks replaced by their snippets' text.
pub fn finish(text: &str, snippets: &BTreeMap<String, String>) -> String {
    snippets.iter().fold(text.to_string(), |out, (mark, value)| out.replace(mark.as_str(), value))
}

/// Whether a rewritten text still has every mark (otherwise the rewrite is not used).
pub fn keeps_marks(text: &str, snippets: &BTreeMap<String, String>) -> bool {
    snippets.keys().all(|m| text.contains(m.as_str()))
}

/// Sentence tidy-up: single spaces, no space before , . ! ? ; :, first letter capitalised.
/// Run before `finish` so a snippet's own text (newlines, spacing) is left alone.
pub fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text.split_whitespace() {
        if !out.is_empty() && !word.starts_with([',', '.', '!', '?', ';', ':']) {
            out.push(' ');
        }
        out.push_str(word);
    }
    let mut c = out.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => out,
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
}

/// The letters-and-digits runs of a phrase: "Rendez-vous !" → ["Rendez", "vous"].
pub fn tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !is_word_char(c)).filter(|t| !t.is_empty()).map(String::from).collect()
}

/// Every occurrence of `phrase`, ignoring case and what separates its words ("rendez vous",
/// "Rendez-vous"), but only as whole words: "art" never matches in "partir".
pub fn replacing(phrase: &str, replacement: &str, text: &str) -> String {
    let wanted: Vec<String> = tokens(phrase).into_iter().map(|t| t.to_lowercase()).collect();
    if wanted.is_empty() {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    // runs of word characters: (start, end, lowercase)
    let mut runs: Vec<(usize, usize, String)> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if is_word_char(chars[i]) {
            let start = i;
            while i < chars.len() && is_word_char(chars[i]) {
                i += 1;
            }
            runs.push((start, i, chars[start..i].iter().collect::<String>().to_lowercase()));
        } else {
            i += 1;
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut i = 0;
    while i + wanted.len() <= runs.len() {
        if wanted.iter().enumerate().all(|(k, w)| runs[i + k].2 == *w) {
            out.extend(&chars[copied..runs[i].0]);
            out.push_str(replacement);
            copied = runs[i + wanted.len() - 1].1;
            i += wanted.len();
        } else {
            i += 1;
        }
    }
    out.extend(&chars[copied..]);
    out
}

/// Only words of letters, digits, spaces, hyphens and apostrophes get their spelling fixed: "C++" would
/// match every lone "c".
pub fn is_plain_word(word: &str) -> bool {
    let letters = word.chars().filter(|c| c.is_alphanumeric()).count();
    letters >= 2 && word.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '\'' | '’'))
}

/// Words as people count them: "c'est" is one word, a lone "?" none.
pub fn word_count(s: &str) -> usize {
    s.split_whitespace().filter(|w| w.chars().any(|c| c.is_alphanumeric())).count()
}

pub fn words_per_minute(words: u64, seconds: f64) -> u64 {
    if seconds < 1.0 || words == 0 {
        return 0;
    }
    (words as f64 / (seconds / 60.0)).round() as u64
}

/// Days in a row (days = days since 1970, local) with a dictation, ending today, or yesterday if
/// today has none yet.
pub fn streak(days: &HashSet<i64>, today: i64) -> u32 {
    let mut day = if days.contains(&today) { today } else { today - 1 };
    let mut n = 0;
    while days.contains(&day) {
        n += 1;
        day -= 1;
    }
    n
}

/// One stored dictation (SQLite `dictations`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: i64,
    pub text: String,
    pub app: Option<String>,
    /// ms since 1970
    pub at: i64,
    pub words: i64,
    /// how long the user spoke, ms
    pub ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub words: u64,
    pub dictations: u64,
    pub words_per_minute: u64,
    pub streak: u32,
    /// (app, words), most words first, at most 5.
    pub top_apps: Vec<(String, u64)>,
}

/// (words, ms, time or day, app) of one dictation.
pub type Row = (i64, i64, i64, Option<String>);

/// `rows` = (words, ms, local day number, app) for every dictation.
pub fn stats(rows: &[Row], today: i64) -> Stats {
    let words: u64 = rows.iter().map(|r| r.0.max(0) as u64).sum();
    let ms: i64 = rows.iter().map(|r| r.1.max(0)).sum();
    let days: HashSet<i64> = rows.iter().filter(|r| r.0 > 0).map(|r| r.2).collect();
    let mut apps: BTreeMap<&str, u64> = BTreeMap::new();
    for (w, _, _, app) in rows {
        if let Some(a) = app.as_deref().filter(|a| !a.is_empty()) {
            *apps.entry(a).or_default() += (*w).max(0) as u64;
        }
    }
    let mut top_apps: Vec<(String, u64)> = apps.into_iter().map(|(a, w)| (a.to_string(), w)).collect();
    top_apps.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    top_apps.truncate(5);
    Stats { words, dictations: rows.len() as u64, words_per_minute: words_per_minute(words, ms as f64 / 1000.0), streak: streak(&days, today), top_apps }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(w: &[&str], r: &[(&str, &str)], s: &[(&str, &str)]) -> Words {
        Words {
            words: w.iter().map(|x| x.to_string()).collect(),
            replacements: r.iter().map(|(f, t)| Replacement { from: f.to_string(), to: t.to_string() }).collect(),
            snippets: s.iter().map(|(t, x)| Snippet { trigger: t.to_string(), text: x.to_string() }).collect(),
        }
    }

    fn apply(text: &str, w: &Words) -> String {
        let p = prepare(text, w);
        finish(&p.text, &p.snippets)
    }

    #[test]
    fn replacements_ignore_case_and_separators_but_keep_whole_words() {
        assert_eq!(replacing("cloud code", "Claude Code", "I open Cloud-Code today"), "I open Claude Code today");
        assert_eq!(replacing("art", "ART", "partir à l'art"), "partir à l'ART");
        assert_eq!(replacing("", "x", "nothing"), "nothing");
        assert_eq!(replacing("open ai", "x", "I use OpenAI and open-ai"), "I use OpenAI and x");
    }

    #[test]
    fn the_dictionary_fixes_the_spelling_of_plain_words_only() {
        let w = words(&["Supabase", "C++", "Next.js"], &[], &[]);
        assert_eq!(apply("we move to supabase and c", &w), "we move to Supabase and c");
        assert!(is_plain_word("Claude Code") && is_plain_word("aujourd'hui"));
        assert!(!is_plain_word("C++") && !is_plain_word("x"));
    }

    #[test]
    fn snippets_are_set_aside_from_everything_else() {
        let w = words(&[], &[("code", "CODE")], &[("my signature", "Best,\nAlex Code"), ("empty", "")]);
        let p = prepare("thanks, my signature", &w);
        assert_eq!(p.text, "thanks, ⟦1⟧");
        assert!(keeps_marks(&p.text, &p.snippets) && !keeps_marks("thanks", &p.snippets));
        assert_eq!(finish(&p.text, &p.snippets), "thanks, Best,\nAlex Code");
        assert_eq!(apply("it is empty", &w), "it is empty"); // a snippet with no text is never a mark
    }

    #[test]
    fn tidy_fixes_spacing_and_the_first_capital() {
        assert_eq!(tidy("  hello   world , how are you ?  "), "Hello world, how are you?");
        assert_eq!(tidy("élan ⟦1⟧ ok"), "Élan ⟦1⟧ ok");
        assert_eq!(tidy(""), "");
    }

    #[test]
    fn words_are_counted_like_people_count_them() {
        assert_eq!(word_count("c'est  bon ? oui"), 3);
        assert_eq!(word_count(" ? ! "), 0);
        assert_eq!(tokens("Rendez-vous !"), vec!["Rendez", "vous"]);
    }

    #[test]
    fn validation_trims_and_bounds() {
        let w = words(&[" Supabase "], &[("a b", " c ")], &[("sig", "x")]).validated().unwrap();
        assert_eq!(w, words(&["Supabase"], &[("a b", "c")], &[("sig", "x")]));
        assert!(words(&[""], &[], &[]).validated().is_err());
        assert!(words(&[], &[("x", " ")], &[]).validated().is_err());
        assert!(words(&[&"x".repeat(101)], &[], &[]).validated().is_err());
        let many: Vec<&str> = vec!["ab"; 501];
        assert!(words(&many, &[], &[]).validated().is_err());
    }

    #[test]
    fn stats_count_words_speed_streak_and_apps() {
        let rows = vec![
            (30, 10_000, 100, Some("Slack".to_string())),
            (60, 20_000, 99, Some("Notepad".to_string())),
            (10, 30_000, 97, None),
            (0, 1_000, 98, Some("Slack".to_string())), // no words: not a streak day
        ];
        let s = stats(&rows, 100);
        assert_eq!((s.words, s.dictations, s.words_per_minute, s.streak), (100, 4, 98, 2));
        assert_eq!(s.top_apps, vec![("Notepad".to_string(), 60), ("Slack".to_string(), 30)]);
        assert_eq!(stats(&rows, 101).streak, 2); // nothing today yet: counts until yesterday
        assert_eq!(stats(&rows, 102).streak, 0);
        assert_eq!(stats(&[], 5), Stats { words: 0, dictations: 0, words_per_minute: 0, streak: 0, top_apps: vec![] });
        assert_eq!(words_per_minute(10, 0.5), 0);
    }
}
