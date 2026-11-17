//! Launcher quick links: named web links, optionally with a `{query}` slot ("yt lofi beats" → YouTube results).
//! `resolve` decides what the launcher's search box does with what was typed; the rest is storage and validation.

use serde::Serialize;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub id: i64,
    pub name: String,
    /// What to type first ("yt"); lower-case, no spaces. May be empty: the name is matched instead.
    pub keyword: String,
    /// http(s) address; `{query}` is replaced by the rest of what was typed.
    pub url: String,
}

/// What Enter does for the typed text. `kind`: "link" (a saved link), "url" (an address typed in) or "search" (Google).
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Resolved {
    pub kind: String,
    pub label: String,
    pub url: String,
}

/// Percent-encode for a query string: unreserved characters stay, a space becomes `+`, every other byte `%XX`.
pub fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Only web addresses are opened from here: anything else (file:, javascript:, a program path) is refused.
pub fn check_url(url: &str) -> Result<(), String> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")).ok_or("The link must start with http:// or https://")?;
    if rest.is_empty() || rest.starts_with('/') || url.chars().any(char::is_whitespace) {
        return Err("That isn't a valid web address".into());
    }
    Ok(())
}

/// "zoom.us/j/1" or "https://x.y/z" typed in as an address: the address to open, else None.
fn as_url(s: &str) -> Option<String> {
    if s.chars().any(char::is_whitespace) {
        return None;
    }
    if check_url(s).is_ok() {
        return Some(s.to_string());
    }
    let host = s.split(['/', '?', '#']).next()?;
    let tld = host.rsplit('.').next()?;
    let ok = host.contains('.') && !host.starts_with('.') && !host.ends_with('.') && tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic());
    (ok && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')).then(|| format!("https://{s}"))
}

fn host_of(url: &str) -> String {
    url.split("://").nth(1).unwrap_or(url).split(['/', '?', '#']).next().unwrap_or("").trim_start_matches("www.").to_string()
}

pub fn resolve(input: &str, links: &[Link]) -> Option<Resolved> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    let (first, rest) = input.split_once(char::is_whitespace).map_or((input, ""), |(a, b)| (a, b.trim()));
    let first = first.to_lowercase();
    let hit = links.iter().find(|l| if l.keyword.is_empty() { l.name.to_lowercase().replace(' ', "") == first } else { l.keyword == first });
    if let Some(l) = hit {
        let slot = l.url.contains("{query}");
        let (label, url) = match (slot, rest.is_empty()) {
            (true, false) => (format!("Search {} for “{rest}”", l.name), l.url.replace("{query}", &encode(rest))),
            (true, true) => (format!("Open {}", l.name), l.url.replace("{query}", "")),
            (false, _) => (format!("Open {}", l.name), l.url.clone()),
        };
        return Some(Resolved { kind: "link".into(), label, url });
    }
    if let Some(url) = as_url(input) {
        return Some(Resolved { kind: "url".into(), label: format!("Open {}", host_of(&url)), url });
    }
    Some(Resolved { kind: "search".into(), label: format!("Search Google for “{input}”"), url: format!("https://www.google.com/search?q={}", encode(input)) })
}

/// The links a new database starts with.
pub const DEFAULTS: [(&str, &str, &str); 4] = [
    ("Google", "g", "https://www.google.com/search?q={query}"),
    ("YouTube", "yt", "https://www.youtube.com/results?search_query={query}"),
    ("GitHub", "gh", "https://github.com/search?q={query}"),
    ("Maps", "maps", "https://www.google.com/maps/search/{query}"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn links() -> Vec<Link> {
        let mk = |id, name: &str, keyword: &str, url: &str| Link { id, name: name.into(), keyword: keyword.into(), url: url.into() };
        vec![
            mk(1, "YouTube", "yt", "https://www.youtube.com/results?search_query={query}"),
            mk(2, "Standup", "standup", "https://us06web.zoom.us/j/123?pwd=abc#success"),
            mk(3, "Team Chat", "", "https://discord.com/channels/1/2"),
        ]
    }

    #[test]
    fn keyword_with_a_query_fills_the_slot() {
        let r = resolve("YT lofi beats & rain", &links()).unwrap();
        assert_eq!(r.kind, "link");
        assert_eq!(r.url, "https://www.youtube.com/results?search_query=lofi+beats+%26+rain");
        assert_eq!(r.label, "Search YouTube for “lofi beats & rain”");
    }

    #[test]
    fn saved_links_without_a_slot_open_as_they_are() {
        // a meeting link with its passcode and a Discord channel: opened untouched, extra words ignored
        assert_eq!(resolve("standup", &links()).unwrap().url, "https://us06web.zoom.us/j/123?pwd=abc#success");
        assert_eq!(resolve("standup now", &links()).unwrap().url, "https://us06web.zoom.us/j/123?pwd=abc#success");
        // no keyword: the name (spaces dropped) is what to type
        assert_eq!(resolve("teamchat", &links()).unwrap().url, "https://discord.com/channels/1/2");
    }

    #[test]
    fn typed_addresses_open_and_anything_else_searches() {
        assert_eq!(resolve("https://discord.com/channels/9/8", &[]).unwrap().url, "https://discord.com/channels/9/8");
        let r = resolve("zoom.us/j/669?pwd=x", &[]).unwrap();
        assert_eq!((r.kind.as_str(), r.url.as_str(), r.label.as_str()), ("url", "https://zoom.us/j/669?pwd=x", "Open zoom.us"));
        assert_eq!(resolve("hello world", &[]).unwrap().kind, "search");
        assert_eq!(resolve("v1.2", &[]).unwrap().kind, "search"); // a version number is not a site
        assert_eq!(resolve("   ", &links()), None);
    }

    #[test]
    fn only_web_addresses_can_be_saved() {
        assert!(check_url("https://example.com/a?b=1#c").is_ok());
        for bad in ["file:///C:/x.exe", "javascript:alert(1)", r"C:\Windows\calc.exe", "https://", "https:///x", "https://a b.com", "example.com"] {
            assert!(check_url(bad).is_err(), "{bad}");
        }
    }
}
