//! Windows toast notifications via UserNotificationListener, polled every second.
//! Needs "Notification access" allowed for the app in Windows Settings > Privacy > Notifications.

use super::Notification;
use std::collections::HashSet;
use std::{thread, time::Duration};
use tauri::{AppHandle, Emitter};
use windows::core::Result;
use windows::UI::Notifications::Management::{UserNotificationListener, UserNotificationListenerAccessStatus};
use windows::UI::Notifications::{NotificationKinds, UserNotification};

pub fn spawn(app: AppHandle) {
    thread::spawn(move || {
        if let Err(e) = run(&app) {
            eprintln!("notification monitor stopped: {e}");
        }
    });
}

fn run(app: &AppHandle) -> Result<()> {
    let listener = UserNotificationListener::Current()?;
    let status = listener.RequestAccessAsync()?.get()?;
    if status != UserNotificationListenerAccessStatus::Allowed {
        eprintln!("notification access not granted ({status:?}); enable it in Windows Settings");
        return Ok(());
    }

    let ids = |list: &[UserNotification]| list.iter().filter_map(|n| n.Id().ok()).collect::<HashSet<u32>>();
    let mut known = ids(&current(&listener)?);
    loop {
        thread::sleep(Duration::from_secs(1));
        let Ok(list) = current(&listener) else { continue };
        for n in &list {
            if n.Id().is_ok_and(|id| !known.contains(&id)) {
                if let Ok(payload) = read(n) {
                    let _ = app.emit("notify", payload);
                }
            }
        }
        // Keep only ids still in the action centre, so the set never grows unbounded.
        known = ids(&list);
    }
}

fn current(listener: &UserNotificationListener) -> Result<Vec<UserNotification>> {
    Ok(listener.GetNotificationsAsync(NotificationKinds::Toast)?.get()?.into_iter().collect())
}

fn read(n: &UserNotification) -> Result<Notification> {
    let app = n.AppInfo()?.DisplayInfo()?.DisplayName()?.to_string();
    let (mut title, mut text) = (String::new(), String::new());
    for binding in n.Notification()?.Visual()?.Bindings()? {
        let parts = binding.GetTextElements()?;
        if parts.Size()? > 0 {
            title = parts.GetAt(0)?.Text()?.to_string();
        }
        if parts.Size()? > 1 {
            text = parts.GetAt(1)?.Text()?.to_string();
        }
    }
    Ok(Notification { app, title, text })
}
