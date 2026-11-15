//! Reminders (spec §23.2): repeat math, quick-chip times, and the 15 s scheduler that emits `reminder-due`.

use crate::{fullscreen, AppState};
use chrono::{DateTime, Datelike, Days, Duration, Local, NaiveDateTime, NaiveTime, TimeZone, Timelike, Utc, Weekday};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

pub const SNOOZE_MS: i64 = 10 * 60 * 1000;
pub const MAX_TEXT: usize = 200;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    None,
    Daily,
    Weekdays,
    Weekly,
}

impl Repeat {
    pub fn as_str(self) -> &'static str {
        match self {
            Repeat::None => "none",
            Repeat::Daily => "daily",
            Repeat::Weekdays => "weekdays",
            Repeat::Weekly => "weekly",
        }
    }

    pub fn parse(s: &str) -> Repeat {
        match s {
            "daily" => Repeat::Daily,
            "weekdays" => Repeat::Weekdays,
            "weekly" => Repeat::Weekly,
            _ => Repeat::None,
        }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    pub id: i64,
    pub text: String,
    /// Scheduled time (epoch ms); repeats keep this wall-clock time.
    pub due_at: i64,
    pub repeat: Repeat,
    pub done: bool,
    /// Snoozed occurrence time; cleared on Done so a repeat keeps its own time.
    pub snooze_until: Option<i64>,
    /// When Done was last pressed (the tab's "done today").
    pub done_at: Option<i64>,
}

impl Reminder {
    /// When this occurrence is due (snooze wins).
    pub fn at(&self) -> i64 {
        self.snooze_until.unwrap_or(self.due_at)
    }
}

/// Trimmed text; empty or over `MAX_TEXT` chars is an error.
pub fn clean_text(text: &str) -> Result<&str, String> {
    let t = text.trim();
    if t.is_empty() || t.chars().count() > MAX_TEXT {
        return Err(format!("A reminder must be 1-{MAX_TEXT} characters"));
    }
    Ok(t)
}

pub fn snoozed(now: i64) -> i64 {
    now + SNOOZE_MS
}

/// Local wall-clock time → epoch ms; a time skipped by DST moves an hour later.
fn local_ms<Tz: TimeZone>(tz: &Tz, n: NaiveDateTime) -> Option<i64> {
    tz.from_local_datetime(&n)
        .earliest()
        .or_else(|| tz.from_local_datetime(&(n + Duration::hours(1))).earliest())
        .map(|d| d.timestamp_millis())
}

/// Next occurrence strictly after `now`, at the same local wall-clock time as `due_at`.
/// `None` for a one-off reminder.
pub fn next_due(due_at: i64, repeat: Repeat, now: i64, tz: &impl TimeZone) -> Option<i64> {
    let step = match repeat {
        Repeat::None => return None,
        Repeat::Weekly => 7,
        Repeat::Daily | Repeat::Weekdays => 1,
    };
    let local = tz.timestamp_millis_opt(due_at).single()?.naive_local();
    let mut day = local.date();
    loop {
        day = day.checked_add_days(Days::new(step))?;
        if repeat == Repeat::Weekdays && matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
            continue;
        }
        let t = local_ms(tz, day.and_time(local.time()))?;
        if t > now {
            return Some(t);
        }
    }
}

/// Due time for a New-reminder chip: `10m|30m|1h|evening|tomorrow` (evening 18:00, tomorrow 09:00).
pub fn quick_due<Tz: TimeZone>(chip: &str, now: DateTime<Tz>) -> Option<i64> {
    let ms = now.timestamp_millis();
    let tz = now.timezone();
    let at = |days: u64, hour: u32| local_ms(&tz, now.date_naive().checked_add_days(Days::new(days))?.and_hms_opt(hour, 0, 0)?);
    match chip {
        "10m" => Some(ms + 10 * 60_000),
        "30m" => Some(ms + 30 * 60_000),
        "1h" => Some(ms + 60 * 60_000),
        "evening" => at(if now.hour() < 18 { 0 } else { 1 }, 18),
        "tomorrow" => at(1, 9),
        _ => None,
    }
}

/// Reminders due at `now` that were not emitted yet for this occurrence (`(id, at)` in `fired`).
/// `fired` keeps only occurrences that are still due, so a snoozed or rolled one can fire again.
pub fn take_due(fired: &mut BTreeSet<(i64, i64)>, list: &[Reminder], now: i64) -> Vec<Reminder> {
    let due: Vec<&Reminder> = list.iter().filter(|r| !r.done && r.at() <= now).collect();
    fired.retain(|k| due.iter().any(|r| (r.id, r.at()) == *k));
    due.into_iter().filter(|r| fired.insert((r.id, r.at()))).cloned().collect()
}

static FIRED: Mutex<BTreeSet<(i64, i64)>> = Mutex::new(BTreeSet::new());

/// One check: emit `reminder-due` for each new due occurrence. While a fullscreen app hides the
/// island nothing is marked fired, so they all go out on the first check after it returns.
pub fn tick(app: &AppHandle) {
    if fullscreen::is_hidden(app) {
        return;
    }
    let Ok(list) = app.state::<AppState>().db.lock().unwrap().reminders(i64::MAX) else { return };
    for r in take_due(&mut FIRED.lock().unwrap(), &list, Utc::now().timestamp_millis()) {
        let _ = app.emit("reminder-due", serde_json::json!({ "id": r.id, "text": r.text, "dueAt": r.at() }));
    }
}

/// Checks every 15 s; the first check at startup fires reminders missed while the app was closed.
pub fn spawn_scheduler(app: AppHandle) {
    std::thread::spawn(move || loop {
        tick(&app);
        std::thread::sleep(std::time::Duration::from_secs(15));
    });
}

/// Start of today (local), epoch ms: the cut-off for the tab's "done today".
pub fn today_start() -> i64 {
    let d = Local::now().date_naive().and_time(NaiveTime::MIN);
    local_ms(&Local, d).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::Europe::London;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> i64 {
        London.with_ymd_and_hms(y, m, d, h, min, 0).unwrap().timestamp_millis()
    }

    fn r(id: i64, due_at: i64) -> Reminder {
        Reminder { id, text: format!("r{id}"), due_at, repeat: Repeat::None, done: false, snooze_until: None, done_at: None }
    }

    #[test]
    fn next_due_for_each_repeat() {
        let due = at(2026, 11, 10, 8, 25); // Tuesday
        assert_eq!(next_due(due, Repeat::None, due, &London), None);
        assert_eq!(next_due(due, Repeat::Daily, due, &London), Some(at(2026, 11, 11, 8, 25)));
        assert_eq!(next_due(due, Repeat::Weekdays, due, &London), Some(at(2026, 11, 11, 8, 25)));
        assert_eq!(next_due(due, Repeat::Weekly, due, &London), Some(at(2026, 11, 17, 8, 25)));
        // strictly after `now`, skipping every missed occurrence
        let now = at(2026, 11, 14, 12, 0);
        assert_eq!(next_due(due, Repeat::Daily, now, &London), Some(at(2026, 11, 15, 8, 25)));
        assert_eq!(next_due(due, Repeat::Weekly, now, &London), Some(at(2026, 11, 17, 8, 25)));
        assert_eq!(next_due(due, Repeat::Daily, at(2026, 11, 14, 8, 0), &London), Some(at(2026, 11, 14, 8, 25)));
    }

    #[test]
    fn weekdays_roll_friday_to_monday() {
        let fri = at(2026, 11, 13, 8, 25);
        assert_eq!(next_due(fri, Repeat::Weekdays, fri, &London), Some(at(2026, 11, 16, 8, 25)));
        let sat_noon = at(2026, 11, 14, 12, 0);
        assert_eq!(next_due(fri, Repeat::Weekdays, sat_noon, &London), Some(at(2026, 11, 16, 8, 25)));
    }

    #[test]
    fn next_due_keeps_wall_clock_across_dst() {
        // London springs forward on 2026-03-29 (01:00 GMT → 02:00 BST): that day is 23 h long.
        let sat = at(2026, 3, 28, 9, 0);
        let sun = next_due(sat, Repeat::Daily, sat, &London).unwrap();
        assert_eq!(sun, at(2026, 3, 29, 9, 0));
        assert_eq!(sun - sat, 23 * 3600 * 1000);
        let weekly = next_due(at(2026, 3, 22, 9, 0), Repeat::Weekly, at(2026, 3, 22, 9, 0), &London).unwrap();
        assert_eq!(weekly, at(2026, 3, 29, 9, 0));
        // 01:30 does not exist on the 29th: it moves an hour later
        let night = at(2026, 3, 28, 1, 30);
        assert_eq!(next_due(night, Repeat::Daily, night, &London), Some(at(2026, 3, 29, 2, 30)));
    }

    #[test]
    fn quick_due_chips() {
        let morning = London.with_ymd_and_hms(2026, 11, 15, 8, 25, 0).unwrap();
        let ms = morning.timestamp_millis();
        assert_eq!(quick_due("10m", morning), Some(ms + 10 * 60_000));
        assert_eq!(quick_due("30m", morning), Some(ms + 30 * 60_000));
        assert_eq!(quick_due("1h", morning), Some(ms + 60 * 60_000));
        assert_eq!(quick_due("evening", morning), Some(at(2026, 11, 15, 18, 0)));
        assert_eq!(quick_due("tomorrow", morning), Some(at(2026, 11, 16, 9, 0)));
        let late = London.with_ymd_and_hms(2026, 11, 15, 19, 0, 0).unwrap();
        assert_eq!(quick_due("evening", late), Some(at(2026, 11, 16, 18, 0)));
        let six = London.with_ymd_and_hms(2026, 11, 15, 18, 0, 0).unwrap();
        assert_eq!(quick_due("evening", six), Some(at(2026, 11, 16, 18, 0))); // 18:00 itself is past
        assert_eq!(quick_due("tomorrow", late), Some(at(2026, 11, 16, 9, 0)));
        assert_eq!(quick_due("pick", morning), None);
    }

    #[test]
    fn take_due_emits_each_occurrence_once() {
        let mut fired = BTreeSet::new();
        let now = at(2026, 11, 15, 8, 30);
        let mut list = vec![r(1, at(2026, 11, 15, 8, 25)), r(2, at(2026, 11, 15, 9, 0)), r(3, at(2026, 11, 14, 8, 0))];
        list[2].done = true;
        let ids = |v: Vec<Reminder>| v.into_iter().map(|r| r.id).collect::<Vec<_>>();
        assert_eq!(ids(take_due(&mut fired, &list, now)), [1]);
        assert!(take_due(&mut fired, &list, now + 15_000).is_empty()); // next tick: no duplicate
        assert_eq!(ids(take_due(&mut fired, &list, at(2026, 11, 15, 9, 0))), [2]);
        // snoozed: a new occurrence, so it fires again once due
        list[0].snooze_until = Some(snoozed(now));
        assert!(take_due(&mut fired, &list, now + 60_000).is_empty());
        assert_eq!(ids(take_due(&mut fired, &list, now + SNOOZE_MS)), [1]);
        assert!(take_due(&mut fired, &list, now + SNOOZE_MS + 15_000).is_empty());
        assert!(fired.len() <= 2); // answered occurrences are forgotten
    }

    #[test]
    fn snooze_and_text_rules() {
        assert_eq!(snoozed(1_000), 1_000 + 600_000);
        let mut x = r(1, 5);
        assert_eq!(x.at(), 5);
        x.snooze_until = Some(9);
        assert_eq!(x.at(), 9);
        assert_eq!(clean_text("  Stand-up \n"), Ok("Stand-up"));
        assert!(clean_text(" ").is_err());
        assert!(clean_text(&"é".repeat(MAX_TEXT)).is_ok());
        assert!(clean_text(&"x".repeat(MAX_TEXT + 1)).is_err());
        assert_eq!(serde_json::to_value(Repeat::Weekdays).unwrap(), "weekdays");
        assert_eq!(Repeat::parse(Repeat::Weekly.as_str()), Repeat::Weekly);
        assert!(serde_json::from_value::<Repeat>(serde_json::json!("hourly")).is_err());
    }
}
