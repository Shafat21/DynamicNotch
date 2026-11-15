//! Calendar card from a private ICS link (spec §15). Fetch on start, every 5 min, when the local
//! date changes, and on `set_calendar_url`. Event: `calendar`.
//! The link is a secret: it is never logged, put in an error string, or sent to the webview.

use super::http;
use crate::{publish, AppState};
use chrono::{DateTime, Days, Local, NaiveDate, NaiveDateTime, TimeDelta, TimeZone, Utc};
use icalendar::parser::{read_calendar, unfold, Component, Property};
use rrule::{RRule, Tz as RTz, Unvalidated};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeSet, HashSet};
use std::sync::Mutex;
use std::{thread, time::Duration, time::Instant};
use tauri::{AppHandle, Manager};

/// Settings key of the ICS link ("" = not configured).
pub const KEY: &str = "calendar_url";
const INTERVAL: Duration = Duration::from_secs(5 * 60);
/// Last good events of the stored link (yesterday..tomorrow) + a generation bumped by every save/remove.
/// One lock, so "still current? → store → publish" is atomic between `set_calendar_url` and the monitor.
struct Cache {
    generation: u64,
    events: Vec<Event>,
}
static CACHE: Mutex<Cache> = Mutex::new(Cache { generation: 0, events: Vec::new() });

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    pub title: String,
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
    pub join: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Payload<'a> {
    configured: bool,
    error: Option<String>,
    /// Nothing could be loaded at all (fetch/parse failed, no cached events); `error` alone is just a note.
    fatal: bool,
    today: Vec<&'a Event>,
    tomorrow_first: Option<&'a Event>,
}

pub fn spawn(app: AppHandle) {
    thread::spawn(move || {
        let mut last: Option<(Instant, NaiveDate)> = None;
        loop {
            let today = Local::now().date_naive();
            if last.is_none_or(|(t, d)| t.elapsed() >= INTERVAL || d != today) {
                last = Some((Instant::now(), today));
                refresh(&app);
            }
            thread::sleep(Duration::from_secs(60));
        }
    });
}

fn stored_url(app: &AppHandle) -> Option<String> {
    app.state::<AppState>().db.lock().unwrap().get(KEY).ok().flatten().filter(|u| !u.is_empty())
}

/// A result is dropped if a save/remove started a newer generation while it was loading.
fn refresh(app: &AppHandle) {
    let started = CACHE.lock().unwrap().generation;
    let _ = match stored_url(app) {
        None => commit(app, started, false, Some(Vec::new()), None, None),
        Some(url) => match load(&url) {
            Ok((events, note)) => commit(app, started, true, Some(events), note, None),
            Err(e) => commit(app, started, true, None, Some(e), None), // keep the last good events
        },
    };
}

/// Start a save/remove: anything still loading for an older generation will be discarded.
pub fn begin() -> u64 {
    let mut c = CACHE.lock().unwrap();
    c.generation += 1;
    c.generation
}

/// The result of work started at generation `started`, if no save/remove began or committed since.
/// A committed save/remove (`store`) bumps the generation, so a monitor fetch that captured the
/// generation between `begin()` and that commit (and so may hold the old link's events) is rejected.
fn accept<T>(generation: &mut u64, started: u64, store: bool, result: T) -> Option<T> {
    if started != *generation {
        return None;
    }
    if store {
        *generation += 1;
    }
    Some(result)
}

/// If still current: store the link (when given), replace the cached events (None = load failed, keep
/// the last good ones), publish the `calendar` payload and return it.
pub fn commit(app: &AppHandle, started: u64, configured: bool, events: Option<Vec<Event>>, error: Option<String>, store: Option<&str>) -> Result<Value, String> {
    let mut c = CACHE.lock().unwrap();
    let (events, store) = accept(&mut c.generation, started, store.is_some(), (events, store)).ok_or("The calendar link was changed meanwhile")?;
    if let Some(url) = store {
        app.state::<AppState>().db.lock().unwrap().set(KEY, url).map_err(|e| e.to_string())?;
    }
    let failed = events.is_none();
    if let Some(events) = events {
        c.events = events;
    }
    let p = payload(configured, error, failed, &c.events);
    publish(app, "calendar", p.clone());
    Ok(p)
}

/// `today` = events overlapping the local day; `tomorrowFirst` = first event starting tomorrow (timed before all-day).
fn payload(configured: bool, error: Option<String>, failed: bool, events: &[Event]) -> Value {
    let day = Local::now().date_naive();
    let ms = |d: NaiveDate| midnight(&Local, d).map(|t| t.timestamp_millis()).unwrap_or_default();
    let (t0, t1, t2) = (ms(day), ms(day + Days::new(1)), ms(day + Days::new(2)));
    let p = Payload {
        configured,
        error,
        fatal: failed && events.is_empty(),
        today: events.iter().filter(|e| overlaps(e.start, e.end, t0, t1)).collect(),
        tomorrow_first: events.iter().filter(|e| e.start >= t1 && e.start < t2).min_by_key(|e| (e.all_day, e.start)),
    };
    serde_json::to_value(p).unwrap_or_default()
}

/// Fetch and parse the yesterday..tomorrow window (spec §15.5). Second value: a note about unknown time zones.
pub fn load(url: &str) -> Result<(Vec<Event>, Option<String>), String> {
    let ics = fetch(url)?;
    let start = midnight(&Local, Local::now().date_naive() - Days::new(1)).ok_or("invalid local time")?;
    let (events, unknown) = parse(&ics, start, 3)?;
    let note = (!unknown.is_empty())
        .then(|| format!("Unknown time zone ({}), shown as local time", unknown.into_iter().collect::<Vec<_>>().join(", ")));
    Ok((events, note))
}

/// Errors never include the link.
fn fetch(url: &str) -> Result<String, String> {
    let fail = |e: ureq::Error| match e {
        ureq::Error::StatusCode(code) => format!("Couldn't fetch calendar (HTTP {code})"),
        _ => "Couldn't fetch calendar".to_string(),
    };
    http().get(url).call().map_err(fail)?.body_mut().with_config().limit(32 << 20).read_to_string().map_err(fail)
}

/// Trim; empty → None; `webcal://` → `https://`; must be https and at most 2048 chars.
pub fn normalize_url(input: &str) -> Result<Option<String>, String> {
    let s = input.trim();
    if s.is_empty() {
        return Ok(None);
    }
    let s = match s.get(..9) {
        Some(p) if p.eq_ignore_ascii_case("webcal://") => format!("https://{}", &s[9..]),
        _ => s.to_string(),
    };
    if s.len() <= 8 || !s.get(..8).is_some_and(|p| p.eq_ignore_ascii_case("https://")) {
        return Err("The calendar link must start with https:// or webcal://".into());
    }
    if s.chars().count() > 2048 {
        return Err("The calendar link is too long".into());
    }
    Ok(Some(s))
}

/// First `https://` URL in `texts` whose host is a known meeting service (spec §15.2).
pub fn join_link(texts: &[&str]) -> Option<String> {
    texts
        .iter()
        .flat_map(|t| t.match_indices("https://").map(move |(i, _)| &t[i..]))
        .map(|s| s[..s.find(|c: char| c.is_whitespace() || "\"'<>".contains(c)).unwrap_or(s.len())].trim_end_matches(['.', ',', ';', ')']))
        .find(|u| {
            let host = u[8..].split(['/', '?', '#']).next().unwrap_or("").to_ascii_lowercase();
            host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') // no userinfo/port tricks
                && (matches!(host.as_str(), "teams.microsoft.com" | "teams.live.com" | "meet.google.com" | "zoom.us")
                    || host.ends_with(".zoom.us")
                    || host.ends_with(".webex.com"))
        })
        .map(String::from)
}

/// Events overlapping `[day_start, day_start + days)`. Floating times and dates are wall-clock times
/// in `day_start`'s zone. Sorted by start; cancelled events skipped; recurrences expanded.
#[allow(dead_code)] // the monitor calls `parse` directly for the unknown-time-zone note
pub fn parse_events<T: TimeZone>(ics: &str, day_start: DateTime<T>, days: u32) -> Result<Vec<Event>, String> {
    parse(ics, day_start, days).map(|(events, _)| events)
}

/// Where a DATE / DATE-TIME's wall clock lives.
#[derive(Clone, Copy)]
enum Zone {
    Utc,
    Named(chrono_tz::Tz),
    Floating,
}

fn parse<T: TimeZone>(ics: &str, day_start: DateTime<T>, days: u32) -> Result<(Vec<Event>, BTreeSet<String>), String> {
    let unfolded = unfold(ics);
    let cal = read_calendar(&unfolded).map_err(|_| "Not a valid calendar (ICS) file".to_string())?;
    let local = day_start.timezone();
    let win_start = day_start.timestamp_millis();
    let win_end = midnight(&local, day_start.naive_local().date() + Days::new(days.into()))
        .ok_or("invalid window")?
        .timestamp_millis();
    let mut unknown = BTreeSet::new();
    let vevents: Vec<&Component> = cal.components.iter().filter(|c| c.name.as_str().eq_ignore_ascii_case("VEVENT")).collect();

    // Instances replaced by a RECURRENCE-ID override (cancelled ones too) are dropped from the master's expansion.
    let overridden: HashSet<(String, i64)> = vevents
        .iter()
        .filter_map(|c| {
            let (n, z, _) = time_prop(c.find_prop("RECURRENCE-ID")?, &mut unknown)?;
            Some((text(c, "UID"), instant(&local, n, z)?))
        })
        .collect();

    let mut out: Vec<Event> = Vec::new();
    for c in vevents {
        if text(c, "STATUS").eq_ignore_ascii_case("CANCELLED") {
            continue;
        }
        let Some((start, zone, all_day)) = c.find_prop("DTSTART").and_then(|p| time_prop(p, &mut unknown)) else { continue };
        let Some(start_ms) = instant(&local, start, zone) else { continue };
        let end = c.find_prop("DTEND").and_then(|p| time_prop(p, &mut unknown));
        // shortcut: DURATION isn't read (Google/Outlook send DTEND); no DTEND = 1 day (all-day) or zero length.
        let wall_len = end.filter(|_| all_day).map(|(e, ..)| e - start).unwrap_or(TimeDelta::days(1));
        let ms_len = end.and_then(|(e, z, _)| instant(&local, e, z)).map(|e| e - start_ms).unwrap_or(0).max(0);
        let uid = text(c, "UID");
        let master = c.find_prop("RECURRENCE-ID").is_none();

        // Occurrence starts as wall-clock times in the event's zone (rrule runs in a UTC "wall" frame).
        let mut walls = vec![start];
        if let (Some(rule), true) = (c.find_prop("RRULE"), master) {
            let margin = TimeDelta::days(2) + TimeDelta::milliseconds(ms_len.max(wall_len.num_milliseconds()));
            let from = DateTime::from_timestamp_millis(win_start).unwrap_or_default().naive_utc() - margin;
            let to = DateTime::from_timestamp_millis(win_end).unwrap_or_default().naive_utc() + TimeDelta::days(2);
            if let Some(more) = expand(rule.val.as_str(), start, zone, &local, from, to) {
                walls = more;
            }
        }
        let mut exdates = HashSet::new();
        for p in &c.properties {
            let name = p.name.as_str().to_ascii_uppercase();
            if name == "RDATE" {
                walls.extend(values(p, &mut unknown).filter_map(|(n, z, _)| wall_in(instant(&local, n, z)?, zone, &local)));
            } else if name == "EXDATE" {
                exdates.extend(values(p, &mut unknown).filter_map(|(n, z, _)| instant(&local, n, z)));
            }
        }

        let title = text(c, "SUMMARY");
        let title = if title.trim().is_empty() { "(No title)".to_string() } else { title };
        let texts = ["LOCATION", "URL", "X-GOOGLE-CONFERENCE", "DESCRIPTION"].map(|k| text(c, k));
        let join = join_link(&texts.iter().map(String::as_str).collect::<Vec<_>>());
        for w in walls {
            let Some(s) = instant(&local, w, zone) else { continue };
            if master && (exdates.contains(&s) || overridden.contains(&(uid.clone(), s))) {
                continue;
            }
            let e = if all_day { instant(&local, w + wall_len, zone).unwrap_or(s) } else { s + ms_len };
            let id = format!("{uid}@{s}");
            if overlaps(s, e, win_start, win_end) && !out.iter().any(|x| x.id == id) {
                out.push(Event { id, title: title.clone(), start: s, end: e, all_day, join: join.clone() });
            }
        }
    }
    out.sort_by(|a, b| (a.start, &a.title).cmp(&(b.start, &b.title)));
    Ok((out, unknown))
}

/// RRULE occurrences starting in `[from, to]` (UTC wall frame); None if the rule is invalid.
fn expand<T: TimeZone>(rule: &str, start: NaiveDateTime, zone: Zone, local: &T, from: NaiveDateTime, to: NaiveDateTime) -> Option<Vec<NaiveDateTime>> {
    let mut rule: RRule<Unvalidated> = rule.parse().ok()?;
    // UNTIL is UTC ("Z") or a wall clock (which rrule parses as machine-local); bring it into the wall frame.
    if let Some(until) = rule.get_until().copied() {
        let wall = if until.timezone() == RTz::UTC { wall_in(until.timestamp_millis(), zone, local)? } else { until.naive_local() };
        rule = rule.until(RTz::UTC.from_utc_datetime(&wall));
    }
    let set = rule.build(RTz::UTC.from_utc_datetime(&start)).ok()?;
    let r = set.after(RTz::UTC.from_utc_datetime(&from)).before(RTz::UTC.from_utc_datetime(&to)).all(1000);
    Some(r.dates.iter().map(|d| d.naive_utc()).collect())
}

fn overlaps(s: i64, e: i64, from: i64, to: i64) -> bool {
    s < to && (e > from || s >= from)
}

fn midnight<T: TimeZone>(tz: &T, d: NaiveDate) -> Option<DateTime<T>> {
    resolve(tz, d.and_hms_opt(0, 0, 0)?)
}

/// Wall clock → instant; a time in a DST gap moves forward an hour.
fn resolve<T: TimeZone>(tz: &T, n: NaiveDateTime) -> Option<DateTime<T>> {
    tz.from_local_datetime(&n).earliest().or_else(|| tz.from_local_datetime(&(n + TimeDelta::hours(1))).earliest())
}

fn instant<T: TimeZone>(local: &T, n: NaiveDateTime, zone: Zone) -> Option<i64> {
    Some(match zone {
        Zone::Utc => n.and_utc().timestamp_millis(),
        Zone::Named(tz) => resolve(&tz, n)?.timestamp_millis(),
        Zone::Floating => resolve(local, n)?.timestamp_millis(),
    })
}

/// Instant → wall clock in `zone`.
fn wall_in<T: TimeZone>(ms: i64, zone: Zone, local: &T) -> Option<NaiveDateTime> {
    let utc = DateTime::<Utc>::from_timestamp_millis(ms)?;
    Some(match zone {
        Zone::Utc => utc.naive_utc(),
        Zone::Named(tz) => utc.with_timezone(&tz).naive_local(),
        Zone::Floating => utc.with_timezone(local).naive_local(),
    })
}

/// Unescaped text value of the first property named `name`, or "".
fn text(c: &Component, name: &str) -> String {
    c.find_prop(name).map(|p| p.val.clone().unescape_text().as_str().to_string()).unwrap_or_default()
}

fn time_prop(p: &Property, unknown: &mut BTreeSet<String>) -> Option<(NaiveDateTime, Zone, bool)> {
    values(p, unknown).next()
}

/// The (comma-separated) DATE / DATE-TIME values of a property: (wall clock, zone, is a DATE).
fn values<'a>(p: &'a Property, unknown: &mut BTreeSet<String>) -> impl Iterator<Item = (NaiveDateTime, Zone, bool)> + 'a {
    let tzid = p.params.iter().find(|x| x.key.as_str().eq_ignore_ascii_case("TZID")).and_then(|x| x.val.as_ref()).map(|v| v.as_str().trim_matches('"'));
    let named = match tzid {
        Some(t) => match t.trim_start_matches('/').parse::<chrono_tz::Tz>().ok().or_else(|| windows_tz(t)) {
            Some(tz) => Zone::Named(tz),
            None => {
                unknown.insert(t.to_string());
                Zone::Floating
            }
        },
        None => Zone::Floating,
    };
    p.val.as_str().split(',').filter_map(move |v| {
        let v = v.trim();
        if v.len() == 8 {
            return Some((NaiveDate::parse_from_str(v, "%Y%m%d").ok()?.and_hms_opt(0, 0, 0)?, Zone::Floating, true));
        }
        let (body, utc) = v.strip_suffix('Z').map_or((v, false), |b| (b, true));
        let n = NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M%S").ok()?;
        Some((n, if utc { Zone::Utc } else { named }, false))
    })
}

/// Common Windows time zone names (used as TZIDs by Outlook) → IANA zones.
pub fn windows_tz(name: &str) -> Option<chrono_tz::Tz> {
    let iana = match name.trim() {
        "Dateline Standard Time" => "Etc/GMT+12",
        "UTC" | "Coordinated Universal Time" => "UTC",
        "Hawaiian Standard Time" => "Pacific/Honolulu",
        "Alaskan Standard Time" => "America/Anchorage",
        "Pacific Standard Time" => "America/Los_Angeles",
        "US Mountain Standard Time" => "America/Phoenix",
        "Mountain Standard Time" => "America/Denver",
        "Central Standard Time" => "America/Chicago",
        "Central America Standard Time" => "America/Guatemala",
        "Canada Central Standard Time" => "America/Regina",
        "Central Standard Time (Mexico)" => "America/Mexico_City",
        "Eastern Standard Time" => "America/New_York",
        "US Eastern Standard Time" => "America/Indiana/Indianapolis",
        "SA Pacific Standard Time" => "America/Bogota",
        "Atlantic Standard Time" => "America/Halifax",
        "Newfoundland Standard Time" => "America/St_Johns",
        "E. South America Standard Time" => "America/Sao_Paulo",
        "Argentina Standard Time" => "America/Argentina/Buenos_Aires",
        "GMT Standard Time" => "Europe/London",
        "Greenwich Standard Time" => "Atlantic/Reykjavik",
        "W. Europe Standard Time" => "Europe/Berlin",
        "Romance Standard Time" => "Europe/Paris",
        "Central Europe Standard Time" => "Europe/Budapest",
        "Central European Standard Time" => "Europe/Warsaw",
        "E. Europe Standard Time" => "Europe/Chisinau",
        "FLE Standard Time" => "Europe/Kiev",
        "GTB Standard Time" => "Europe/Bucharest",
        "Turkey Standard Time" => "Europe/Istanbul",
        "Israel Standard Time" => "Asia/Jerusalem",
        "Egypt Standard Time" => "Africa/Cairo",
        "South Africa Standard Time" => "Africa/Johannesburg",
        "W. Central Africa Standard Time" => "Africa/Lagos",
        "Russian Standard Time" => "Europe/Moscow",
        "Arab Standard Time" => "Asia/Riyadh",
        "Arabian Standard Time" => "Asia/Dubai",
        "Iran Standard Time" => "Asia/Tehran",
        "Pakistan Standard Time" => "Asia/Karachi",
        "India Standard Time" => "Asia/Kolkata",
        "Sri Lanka Standard Time" => "Asia/Colombo",
        "Nepal Standard Time" => "Asia/Kathmandu",
        "Bangladesh Standard Time" => "Asia/Dhaka",
        "Myanmar Standard Time" => "Asia/Yangon",
        "SE Asia Standard Time" => "Asia/Bangkok",
        "China Standard Time" => "Asia/Shanghai",
        "Singapore Standard Time" => "Asia/Singapore",
        "Taipei Standard Time" => "Asia/Taipei",
        "W. Australia Standard Time" => "Australia/Perth",
        "Tokyo Standard Time" => "Asia/Tokyo",
        "Korea Standard Time" => "Asia/Seoul",
        "Cen. Australia Standard Time" => "Australia/Adelaide",
        "E. Australia Standard Time" => "Australia/Brisbane",
        "AUS Eastern Standard Time" => "Australia/Sydney",
        "New Zealand Standard Time" => "Pacific/Auckland",
        _ => return None,
    };
    iana.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UTC midnight of that date, so the test doesn't depend on the machine's zone.
    fn utc_day(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 0, 0, 0).unwrap()
    }

    const ICS: &str = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nDTSTART:20261104T090000Z\r\nDTEND:20261104T093000Z\r\nSUMMARY:Standup\r\nRRULE:FREQ=DAILY;COUNT=3\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:b\r\nDTSTART;TZID=Pacific Standard Time:20261104T080000\r\nDTEND;TZID=Pacific Standard Time:20261104T090000\r\nSUMMARY:Design review\r\nLOCATION:https://teams.microsoft.com/l/meetup-join/abc\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:c\r\nDTSTART;VALUE=DATE:20261104\r\nSUMMARY:Holiday\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:d\r\nDTSTART:20261104T120000Z\r\nDTEND:20261104T130000Z\r\nSTATUS:CANCELLED\r\nSUMMARY:Gone\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    #[test]
    fn parses_recurrence_tz_allday_cancelled_and_join() {
        let day = utc_day(2026, 11, 4);
        let ev = parse_events(ICS, day, 1).unwrap();
        let titles: Vec<_> = ev.iter().map(|e| e.title.as_str()).collect();
        assert!(titles.contains(&"Standup") && titles.contains(&"Design review") && titles.contains(&"Holiday"));
        assert!(!titles.contains(&"Gone"));
        let dr = ev.iter().find(|e| e.title == "Design review").unwrap();
        assert_eq!(dr.start, 1_793_808_000_000); // 2026-11-04 08:00 PST = 16:00Z
        assert_eq!(dr.join.as_deref(), Some("https://teams.microsoft.com/l/meetup-join/abc"));
        assert!(ev.iter().find(|e| e.title == "Holiday").unwrap().all_day);
        assert_eq!(parse_events(ICS, utc_day(2026, 11, 6), 1).unwrap().iter().filter(|e| e.title == "Standup").count(), 1);
        assert_eq!(parse_events(ICS, utc_day(2026, 11, 7), 1).unwrap().iter().filter(|e| e.title == "Standup").count(), 0); // COUNT=3
    }
    #[test]
    fn exdate_override_and_date_until() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:w\r\nDTSTART;TZID=Europe/Berlin:20261102T100000\r\nDTEND;TZID=Europe/Berlin:20261102T110000\r\nRRULE:FREQ=DAILY;UNTIL=20261105T225959Z\r\nEXDATE;TZID=Europe/Berlin:20261103T100000\r\nSUMMARY:Sync\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:w\r\nRECURRENCE-ID;TZID=Europe/Berlin:20261104T100000\r\nDTSTART;TZID=Europe/Berlin:20261104T150000\r\nDTEND;TZID=Europe/Berlin:20261104T160000\r\nSUMMARY:Sync (moved)\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:h\r\nDTSTART;VALUE=DATE:20261102\r\nDTEND;VALUE=DATE:20261103\r\nRRULE:FREQ=DAILY;UNTIL=20261103\r\nSUMMARY:Trip\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let titles = |d| parse_events(ics, utc_day(2026, 11, d), 1).unwrap().into_iter().map(|e| e.title).collect::<Vec<_>>();
        assert_eq!(titles(2), ["Trip", "Sync"]);
        assert_eq!(titles(3), ["Trip"]); // EXDATE
        assert_eq!(titles(4), ["Sync (moved)"]); // override replaces the 10:00 instance; Trip's UNTIL passed
        assert_eq!(titles(5), ["Sync"]);
        assert!(titles(6).is_empty()); // UNTIL
    }
    #[test]
    fn stale_generation_result_is_discarded() {
        let mut generation = 4;
        assert_eq!(accept(&mut generation, 3, false, "old link's events"), None); // a newer save/remove started meanwhile
        assert_eq!(accept(&mut generation, 4, false, "events"), Some("events"));
        assert_eq!(generation, 4); // monitor refreshes don't bump
    }
    #[test]
    fn monitor_result_rejected_after_save_or_remove_commits() {
        let mut generation = 0;
        generation += 1; // begin(): set_calendar_url starts
        let save = generation;
        let monitor = generation; // monitor captures the generation after begin(), reads the old link, fetches
        assert_eq!(accept(&mut generation, save, true, "new link"), Some("new link")); // save/remove commits
        assert_eq!(accept(&mut generation, monitor, false, "old link's events"), None); // must not republish
    }
    #[test]
    fn fatal_only_when_nothing_could_be_loaded() {
        let ev = Event { id: "a@0".into(), title: "x".into(), start: 0, end: 1, all_day: false, join: None };
        assert_eq!(payload(true, Some("Couldn't fetch calendar".into()), true, &[])["fatal"], true);
        assert_eq!(payload(true, Some("Couldn't fetch calendar".into()), true, &[ev])["fatal"], false); // cached events remain
        assert_eq!(payload(true, Some("Unknown time zone (X)".into()), false, &[])["fatal"], false); // note only
    }
    #[test]
    fn join_links_allow_list() {
        assert_eq!(join_link(&["see https://meet.google.com/abc-defg-hij"]).as_deref(), Some("https://meet.google.com/abc-defg-hij"));
        assert_eq!(join_link(&["https://us02web.zoom.us/j/123"]).as_deref(), Some("https://us02web.zoom.us/j/123"));
        assert!(join_link(&["http://meet.google.com/x"]).is_none());
        assert!(join_link(&["https://evil.example/meet.google.com"]).is_none());
        assert!(join_link(&["https://zoom.us.evil.example/j/1"]).is_none());
    }
    #[test]
    fn url_normalization() {
        assert_eq!(normalize_url("  webcal://p.example/c.ics ").unwrap().as_deref(), Some("https://p.example/c.ics"));
        assert_eq!(normalize_url("").unwrap(), None);
        assert!(normalize_url("http://p.example/c.ics").is_err());
        assert!(normalize_url(&format!("https://x/{}", "a".repeat(2050))).is_err());
    }
}
