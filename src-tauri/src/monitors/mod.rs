//! Background threads that watch the system and push events to the frontend.
//! Each `spawn` starts one OS thread; threads live until the app exits.

pub mod media;
pub mod notify;
pub mod perf;
pub mod system;
pub mod weather;

use std::sync::OnceLock;
use std::time::Duration;
use tauri::AppHandle;

/// Shared HTTP client with a 10 s timeout (weather, geocoding, lyrics).
pub fn http() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(10))).build().into())
}

pub fn spawn_all(app: AppHandle) {
    system::spawn(app.clone());
    perf::spawn(app.clone());
    weather::spawn(app.clone());
    media::spawn(app.clone());
    notify::spawn(app);
}
