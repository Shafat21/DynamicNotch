//! Sound (spec §23.4): output volume, mute and default output device. Every OS call runs on one worker
//! thread (on Windows it owns COM, MTA); it polls once a second and publishes the sticky `sound` event
//! `{ state, devices }` when something changed, so changes made elsewhere show up in the island.

use serde::Serialize;
use crate::worker::{self, Job};
use std::sync::{mpsc, OnceLock};
use std::time::{Duration, Instant};
use tauri::AppHandle;

#[cfg(windows)]
#[path = "sound_windows.rs"]
mod os;
#[cfg(target_os = "linux")]
#[path = "sound_linux.rs"]
mod os;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SoundState {
    /// 0..=1
    pub volume: f32,
    pub muted: bool,
    pub device_id: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub default: bool,
}

/// Payload of the `sound` event; `state` is null when there is no output device.
#[derive(Serialize, Clone, Debug, PartialEq)]
struct Snapshot {
    state: Option<SoundState>,
    devices: Vec<AudioDevice>,
}

pub fn volume_arg(v: f64) -> Result<f32, String> {
    if v.is_finite() {
        Ok(v.clamp(0.0, 1.0) as f32)
    } else {
        Err("Invalid volume".into())
    }
}

/// The webview may only pick a device from the current list.
pub fn check_id(id: &str, devices: &[AudioDevice]) -> Result<(), String> {
    if devices.iter().any(|d| d.id == id) {
        Ok(())
    } else {
        Err("Unknown audio device".into())
    }
}

static JOBS: OnceLock<mpsc::Sender<Job>> = OnceLock::new();
const POLL: Duration = Duration::from_secs(1);
/// While a slider is dragged jobs arrive back to back: re-read at most this often.
const MIN_POLL: Duration = Duration::from_millis(250);

/// The default device's id comes from the device list (Linux would otherwise run `wpctl status` twice per poll).
fn with_default(s: SoundState, devices: &[AudioDevice]) -> SoundState {
    match devices.iter().find(|d| d.default) {
        Some(d) => SoundState { device_id: d.id.clone(), ..s },
        None => s,
    }
}

fn snapshot() -> Snapshot {
    let devices = os::devices().unwrap_or_default();
    Snapshot { state: os::state().ok().map(|s| with_default(s, &devices)), devices }
}

pub fn spawn(app: AppHandle) {
    let (tx, rx) = mpsc::channel::<Job>();
    if JOBS.set(tx).is_err() {
        return;
    }
    std::thread::spawn(move || {
        os::init_thread();
        let mut last: Option<Snapshot> = None;
        let mut polled = Instant::now() - POLL;
        loop {
            match rx.recv_timeout(POLL.saturating_sub(polled.elapsed())) {
                Ok(job) => worker::run_job(job),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
            if polled.elapsed() < MIN_POLL {
                continue;
            }
            polled = Instant::now();
            let snap = snapshot();
            if last.as_ref() != Some(&snap) {
                crate::publish(&app, "sound", &snap);
                last = Some(snap);
            }
        }
    });
}

/// Run `f` on the sound thread and wait for its result.
fn run<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    worker::call(JOBS.get(), f)
}

pub fn state() -> Result<SoundState, String> {
    run(|| Ok(with_default(os::state()?, &os::devices().unwrap_or_default())))
}

pub fn set_volume(value: f64) -> Result<(), String> {
    let v = volume_arg(value)?;
    run(move || os::set_volume(v))
}

pub fn set_mute(muted: bool) -> Result<(), String> {
    run(move || os::set_mute(muted))
}

pub fn devices() -> Result<Vec<AudioDevice>, String> {
    run(os::devices)
}

pub fn set_default(id: String) -> Result<(), String> {
    run(move || {
        check_id(&id, &os::devices()?)?;
        os::set_default(&id)
    })
}

/// Run a helper program (Linux backends) with C locale output; stdout on success.
#[cfg(target_os = "linux")]
pub fn sh(prog: &str, args: &[&str]) -> Result<String, String> {
    let o = std::process::Command::new(prog).args(args).env("LC_ALL", "C").output().map_err(|e| format!("{prog}: {e}"))?;
    if o.status.success() {
        Ok(String::from_utf8_lossy(&o.stdout).into_owned())
    } else {
        Err(format!("{prog} failed: {}", String::from_utf8_lossy(&o.stderr).trim()))
    }
}

/// Output parsers for the Linux backend (`wpctl`, `pactl`); platform-neutral so they are tested everywhere.
#[cfg_attr(windows, allow(dead_code))]
pub mod parse {
    use super::AudioDevice;

    /// `wpctl get-volume`: "Volume: 0.40" or "Volume: 0.40 [MUTED]".
    pub fn wpctl_volume(out: &str) -> Option<(f32, bool)> {
        let rest = out.trim().strip_prefix("Volume:")?.trim();
        let v: f32 = rest.split_whitespace().next()?.parse().ok()?;
        Some((v.clamp(0.0, 1.0), rest.contains("[MUTED]")))
    }

    /// The "Sinks:" list of the "Audio" section of `wpctl status`; `*` marks the default.
    pub fn wpctl_sinks(out: &str) -> Vec<AudioDevice> {
        let (mut audio, mut sinks) = (false, false);
        let mut list = Vec::new();
        for line in out.lines() {
            if line.starts_with(|c: char| c.is_alphanumeric()) {
                audio = line.trim() == "Audio";
                sinks = false;
                continue;
            }
            let body = line.trim_start_matches(|c: char| c.is_whitespace() || "│├└─".contains(c)).trim_end();
            if body.ends_with(':') {
                sinks = audio && body == "Sinks:";
                continue;
            }
            if !sinks {
                continue;
            }
            let (default, body) = match body.strip_prefix('*') {
                Some(b) => (true, b.trim_start()),
                None => (false, body),
            };
            let Some((id, rest)) = body.split_once(". ") else { continue };
            if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            let name = rest.rsplit_once(" [").map_or(rest, |(n, _)| n).trim();
            list.push(AudioDevice { id: id.into(), name: name.into(), default });
        }
        list
    }

    /// `pactl get-sink-volume`: the first channel's percentage.
    pub fn pactl_volume(out: &str) -> Option<f32> {
        let p = out.split_whitespace().find(|t| t.ends_with('%'))?;
        Some((p.trim_end_matches('%').parse::<f32>().ok()? / 100.0).clamp(0.0, 1.0))
    }

    /// `pactl get-sink-mute`: "Mute: yes|no".
    pub fn pactl_mute(out: &str) -> Option<bool> {
        match out.trim().strip_prefix("Mute:")?.trim() {
            "yes" => Some(true),
            "no" => Some(false),
            _ => None,
        }
    }

    /// `pactl list sinks`: id = sink name, label = its description.
    pub fn pactl_sinks(out: &str, default: &str) -> Vec<AudioDevice> {
        let mut list = Vec::new();
        let mut name: Option<String> = None;
        for line in out.lines().map(str::trim) {
            if let Some(n) = line.strip_prefix("Name: ") {
                name = Some(n.into());
            } else if let (Some(d), Some(id)) = (line.strip_prefix("Description: "), name.as_ref()) {
                list.push(AudioDevice { default: id == default.trim(), id: id.clone(), name: d.into() });
                name = None;
            }
        }
        list
    }
}

/// Is `exe` ("spotify.exe") the process behind media session `app`? The session names the app by its
/// AppUserModelID ("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify", "Chrome", "MSEdge") or its exe path.
/// shortcut: name matching; players whose id is opaque (Firefox's is a hash) fall back to the system volume.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn matches_app(app: &str, exe: &str) -> bool {
    let stem = exe.trim_end_matches(".exe").to_lowercase();
    let app = app.to_lowercase();
    let app_stem = app.rsplit(['\\', '/']).next().unwrap_or(&app).trim_end_matches(".exe");
    !stem.is_empty() && (app_stem == stem || (stem.len() >= 4 && app.contains(&stem)))
}

/// Volume 0..1 of the app playing media (`app`: its media session id), or None when no audio session matches.
pub fn app_volume(app: String) -> Result<Option<f32>, String> {
    run(move || os::app_volume(&app))
}

/// Set the playing app's volume; false when no audio session of that app was found.
pub fn set_app_volume(app: String, value: f64) -> Result<bool, String> {
    let v = volume_arg(value)?;
    run(move || os::set_app_volume(&app, v))
}

#[cfg(test)]
mod tests {
    use super::parse::*;
    use super::*;

    #[test]
    fn media_apps_match_their_process() {
        assert!(matches_app("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify", "spotify.exe"));
        assert!(matches_app("Spotify.exe", "spotify.exe"));
        assert!(matches_app("Chrome", "chrome.exe"));
        assert!(matches_app("MSEdge", "msedge.exe"));
        assert!(matches_app(r"C:\Program Files\VideoLAN\vlc.exe", "vlc.exe"));
        assert!(!matches_app("Chrome", "spotify.exe"));
        assert!(!matches_app("308046B0AF4A39CB", "firefox.exe")); // opaque id: no match, system volume instead
        assert!(!matches_app("Spotify", "")); // unknown process
    }

    fn dev(id: &str, name: &str, default: bool) -> AudioDevice {
        AudioDevice { id: id.into(), name: name.into(), default }
    }

    #[test]
    fn volume_is_validated_and_clamped() {
        assert_eq!(volume_arg(0.35), Ok(0.35));
        assert_eq!(volume_arg(1.2), Ok(1.0));
        assert_eq!(volume_arg(-0.1), Ok(0.0));
        assert!(volume_arg(f64::NAN).is_err());
        assert!(volume_arg(f64::INFINITY).is_err());
    }

    #[test]
    fn device_must_be_in_the_current_list() {
        let list = [dev("{0.0.0.00000000}.{abc}", "Speakers", true), dev("{0.0.0.00000000}.{def}", "JBL", false)];
        assert!(check_id("{0.0.0.00000000}.{def}", &list).is_ok());
        assert!(check_id("{0.0.0.00000000}.{xyz}", &list).is_err());
        assert!(check_id("", &list).is_err());
        assert!(check_id("a", &[]).is_err());
    }

    #[test]
    fn device_id_comes_from_the_listed_default() {
        let s = SoundState { volume: 0.5, muted: false, device_id: String::new() };
        assert_eq!(with_default(s.clone(), &[dev("46", "A", false), dev("51", "B", true)]).device_id, "51");
        assert_eq!(with_default(SoundState { device_id: "x".into(), ..s }, &[dev("46", "A", false)]).device_id, "x"); // no default listed: kept
    }

    #[test]
    fn parses_wpctl_volume() {
        assert_eq!(wpctl_volume("Volume: 0.40\n"), Some((0.40, false)));
        assert_eq!(wpctl_volume("Volume: 0.55 [MUTED]"), Some((0.55, true)));
        assert_eq!(wpctl_volume("Volume: 1.30"), Some((1.0, false))); // over-amplified: shown as 100 %
        assert_eq!(wpctl_volume("Error: no such node"), None);
    }

    #[test]
    fn parses_wpctl_status_sinks() {
        let out = "PipeWire 'pipewire-0' [1.0.5, me@box, cookie:1]
 └─ Clients:
        31. WirePlumber                         [1.0.5, me@box, pid:1234]

Audio
 ├─ Devices:
 │      42. Built-in Audio                      [alsa]
 │
 ├─ Sinks:
 │  *   46. Built-in Audio Analog Stereo        [vol: 0.40]
 │      51. JBL Flip 5                          [vol: 0.55 MUTED]
 │
 ├─ Sink endpoints:
 │      60. Not a sink                          [vol: 1.00]
 ├─ Sources:
 │  *   47. Built-in Audio Analog Stereo        [vol: 1.00]
 │
 └─ Streams:

Video
 ├─ Devices:
 │      70. Webcam                              [v4l2]
 ├─ Sinks:
 │      71. Video sink                          [vol: 1.00]
";
        assert_eq!(wpctl_sinks(out), vec![dev("46", "Built-in Audio Analog Stereo", true), dev("51", "JBL Flip 5", false)]);
        assert!(wpctl_sinks("").is_empty());
    }

    #[test]
    fn parses_pactl_output() {
        let vol = "Volume: front-left: 26214 /  40% / -23.88 dB,   front-right: 26214 /  40% / -23.88 dB\n        balance 0.00\n";
        assert_eq!(pactl_volume(vol), Some(0.40));
        assert_eq!(pactl_volume("Volume: mono: 98304 / 150% / 10.57 dB"), Some(1.0));
        assert_eq!(pactl_volume("nothing"), None);
        assert_eq!(pactl_mute("Mute: yes\n"), Some(true));
        assert_eq!(pactl_mute("Mute: no"), Some(false));
        assert_eq!(pactl_mute("Stummschaltung: nein"), None);
        let sinks = "Sink #46
\tState: RUNNING
\tName: alsa_output.pci-0000_00_1f.3.analog-stereo
\tDescription: Built-in Audio Analog Stereo
\tDriver: PipeWire

Sink #51
\tState: SUSPENDED
\tName: bluez_output.AA_BB.1
\tDescription: JBL Flip 5
";
        assert_eq!(
            pactl_sinks(sinks, "bluez_output.AA_BB.1\n"),
            vec![dev("alsa_output.pci-0000_00_1f.3.analog-stereo", "Built-in Audio Analog Stereo", false), dev("bluez_output.AA_BB.1", "JBL Flip 5", true)]
        );
    }
}
