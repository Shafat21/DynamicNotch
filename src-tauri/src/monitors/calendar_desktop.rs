//! Events from the calendars the OS already syncs (Windows: the Calendar app's accounts: Outlook, Google,
//! Exchange, iCloud), read through the AppointmentStore. Read-only, nothing leaves the machine.
//! shortcut: Linux has no equivalent here (Evolution Data Server), use an iCal link there.

use super::calendar::Event;

#[cfg(windows)]
pub fn events(from_ms: i64, days: i64) -> Result<Vec<Event>, String> {
    use super::calendar::join_link;
    use windows::ApplicationModel::Appointments::{AppointmentManager, AppointmentStoreAccessType};
    use windows::Foundation::{DateTime, TimeSpan};
    const EPOCH_1601_MS: i64 = 11_644_473_600_000; // Windows ticks are 100 ns since 1601
    let e = |what: &str, err: windows::core::Error| format!("Desktop calendar: {what} ({})", err.message().trim());
    let store = AppointmentManager::RequestStoreAsync(AppointmentStoreAccessType::AllCalendarsReadOnly)
        .and_then(|op| op.get())
        .map_err(|x| e("no access to the Windows Calendar app", x))?;
    let start = DateTime { UniversalTime: (from_ms + EPOCH_1601_MS) * 10_000 };
    let length = TimeSpan { Duration: days * 86_400 * 10_000_000 };
    let list = store.FindAppointmentsAsync(start, length).and_then(|op| op.get()).map_err(|x| e("could not read appointments", x))?;
    let mut out = Vec::new();
    for a in list {
        let (Ok(s), Ok(d)) = (a.StartTime(), a.Duration()) else { continue };
        let start = s.UniversalTime / 10_000 - EPOCH_1601_MS;
        let text = |r: windows::core::Result<windows::core::HSTRING>| r.map(|t| t.to_string()).unwrap_or_default();
        let title = text(a.Subject());
        let (loc, details) = (text(a.Location()), text(a.Details()));
        out.push(Event {
            id: format!("dt:{}:{start}", text(a.LocalId())),
            title: if title.is_empty() { "(no title)".into() } else { title },
            start,
            end: start + d.Duration / 10_000,
            all_day: a.AllDay().unwrap_or(false),
            join: join_link(&[&loc, &details]),
        });
    }
    Ok(out)
}

#[cfg(not(windows))]
pub fn events(_from_ms: i64, _days: i64) -> Result<Vec<Event>, String> {
    Err("The desktop calendar is only available on Windows. Use an iCal link.".into())
}
