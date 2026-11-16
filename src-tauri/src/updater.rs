//! Updates from GitHub Releases: the release workflow uploads a signed `latest.json` (tauri.conf.json → plugins
//! → updater has the public key). Checks a minute after start and every 6 hours; a newer version is published
//! as `update` once (island toast) and installed only when the user clicks Install in Settings → About.
use serde::Serialize;
use std::sync::Mutex;
use std::time::Duration;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: Option<String>,
    /// false for .deb / .rpm installs (the package manager owns the files): show a download link instead
    pub can_install: bool,
}

/// The newest version found so far (None = up to date or not checked yet).
static FOUND: Mutex<Option<UpdateInfo>> = Mutex::new(None);

/// Windows (NSIS) and the AppImage replace themselves; Linux packages can't.
fn can_install() -> bool {
    cfg!(windows) || std::env::var_os("APPIMAGE").is_some()
}

pub fn found() -> Option<UpdateInfo> {
    FOUND.lock().unwrap().clone()
}

pub async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    let info = update.map(|u| UpdateInfo { version: u.version, notes: u.body, can_install: can_install() });
    *FOUND.lock().unwrap() = info.clone();
    Ok(info)
}

/// Downloads, verifies the signature, installs and restarts. On Windows the installer closes the app itself.
pub async fn install(app: AppHandle) -> Result<(), String> {
    if !can_install() {
        return Err("This install is managed by your package manager: download the new version from Releases.".into());
    }
    let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
    let Some(update) = update else { return Err("You're on the latest version.".into()) };
    update.download_and_install(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
    app.restart()
}

/// Background check: one `update` event per new version, not one every 6 hours.
pub fn watch(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60)); // let startup (models, monitors) settle first
        let mut told: Option<String> = None;
        loop {
            match tauri::async_runtime::block_on(check(&app)) {
                Ok(Some(info)) if told.as_deref() != Some(info.version.as_str()) => {
                    told = Some(info.version.clone());
                    crate::publish(&app, "update", info);
                }
                Ok(_) => {}
                Err(e) => eprintln!("update check failed: {e}"), // offline, or no release published yet
            }
            std::thread::sleep(Duration::from_secs(6 * 60 * 60));
        }
    });
}
