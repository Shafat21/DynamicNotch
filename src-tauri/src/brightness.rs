//! Display brightness (spec §23.4): built-in panels and DDC/CI monitors; displays that can't be
//! adjusted are left out. Hardware writes are slow (DDC ~50-200 ms), so callers run this off the UI
//! thread: one brightness worker owns it (COM for WMI on Windows), so reads and writes never overlap,
//! and writes are coalesced: only the latest value per display reaches the hardware.

use crate::worker::{self, Job};
use serde::Serialize;
use std::sync::{mpsc, Mutex, OnceLock};

#[cfg(windows)]
#[path = "brightness_windows.rs"]
mod os;
#[cfg(target_os = "linux")]
#[path = "brightness_linux.rs"]
mod os;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct BrightDisplay {
    pub id: String,
    pub name: String,
    pub percent: u8,
    /// "internal" | "external"
    pub kind: &'static str,
}

/// Raw value in `[min, max]` → percent.
pub fn to_percent(min: u32, cur: u32, max: u32) -> u8 {
    if max <= min {
        return 0;
    }
    ((cur.clamp(min, max) - min) as f64 * 100.0 / (max - min) as f64).round() as u8
}

/// Percent → raw value in `[min, max]`.
pub fn from_percent(min: u32, max: u32, percent: u8) -> u32 {
    if max <= min {
        return min;
    }
    min + ((max - min) as f64 * f64::from(percent.min(100)) / 100.0).round() as u32
}

/// The webview may only address a display from the last list it was given.
pub fn write_arg(id: &str, percent: u8, known: &[String]) -> Result<u8, String> {
    if known.iter().any(|k| k == id) {
        Ok(percent.min(100))
    } else {
        Err("Unknown display".into())
    }
}

/// Latest-wins write queue: a caller parks its value, waits for the hardware, then applies whatever is
/// latest for its display; callers whose value was superseded meanwhile return without touching it.
pub struct Coalescer {
    latest: Mutex<Vec<(String, u8)>>,
    apply: Mutex<()>,
}

impl Coalescer {
    pub const fn new() -> Self {
        Self { latest: Mutex::new(Vec::new()), apply: Mutex::new(()) }
    }

    pub fn run(&self, id: &str, percent: u8, write: impl FnOnce(u8) -> Result<(), String>) -> Result<(), String> {
        {
            let mut l = self.latest.lock().unwrap();
            l.retain(|(k, _)| k != id);
            l.push((id.to_string(), percent));
        }
        let _one = self.apply.lock().unwrap();
        let next = {
            let mut l = self.latest.lock().unwrap();
            l.iter().position(|(k, _)| k == id).map(|i| l.remove(i).1)
        };
        next.map_or(Ok(()), write)
    }
}

static KNOWN: Mutex<Vec<String>> = Mutex::new(Vec::new());
static WRITES: Coalescer = Coalescer::new();
static JOBS: OnceLock<mpsc::Sender<Job>> = OnceLock::new();

/// Run `f` on the brightness thread (started on first use) and wait for its result.
fn run<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    worker::call(Some(JOBS.get_or_init(|| worker::spawn(os::init_thread))), f)
}

/// Adjustable displays (slow with DDC monitors); also refreshes the ids `set` accepts.
pub fn displays() -> Vec<BrightDisplay> {
    let list = run(|| Ok(os::displays())).unwrap_or_default();
    *KNOWN.lock().unwrap() = list.iter().map(|d| d.id.clone()).collect();
    list
}

pub fn set(id: &str, percent: u8) -> Result<(), String> {
    let percent = write_arg(id, percent, &KNOWN.lock().unwrap())?;
    let owned = id.to_string();
    WRITES.run(id, percent, move |p| run(move || os::set(&owned, p)))
}

/// Output parsers for the Linux backend (`brightnessctl`, `ddcutil`); platform-neutral so they are tested everywhere.
#[cfg_attr(windows, allow(dead_code))]
pub mod parse {
    /// `brightnessctl -m -l -c backlight`: "device,class,current,percent,max" → (device, current, max).
    pub fn brightnessctl(out: &str) -> Vec<(String, u32, u32)> {
        out.lines()
            .filter_map(|l| {
                let f: Vec<&str> = l.trim().split(',').collect();
                let [dev, _, cur, _, max] = f[..] else { return None };
                Some((dev.to_string(), cur.parse().ok()?, max.parse().ok()?))
            })
            .collect()
    }

    /// `ddcutil detect --terse`: valid displays as (display number, model name).
    pub fn ddcutil_detect(out: &str) -> Vec<(String, String)> {
        let mut list: Vec<(String, String)> = Vec::new();
        let mut in_display = false;
        for line in out.lines() {
            if !line.starts_with(char::is_whitespace) {
                in_display = false;
                if let Some(n) = line.trim().strip_prefix("Display ") {
                    in_display = true;
                    list.push((n.trim().to_string(), format!("Display {}", n.trim())));
                }
            } else if let (true, Some(m)) = (in_display, line.trim().strip_prefix("Monitor:")) {
                // "MFG:Model:Serial"
                if let (Some(model), Some(last)) = (m.trim().split(':').nth(1).filter(|s| !s.trim().is_empty()), list.last_mut()) {
                    last.1 = model.trim().to_string();
                }
            }
        }
        list
    }

    /// `ddcutil getvcp 10 --brief`: "VCP 10 C <current> <max>".
    pub fn ddcutil_vcp(out: &str) -> Option<(u32, u32)> {
        let f: Vec<&str> = out.split_whitespace().collect();
        let ["VCP", _, "C", cur, max] = f[..] else { return None };
        Some((cur.parse().ok()?, max.parse().ok()?))
    }
}

#[cfg(test)]
mod tests {
    use super::parse::*;
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn maps_raw_values_to_percent_and_back() {
        assert_eq!(to_percent(0, 50, 100), 50);
        assert_eq!(to_percent(0, 19200, 48000), 40);
        assert_eq!(to_percent(10, 10, 110), 0);
        assert_eq!(to_percent(0, 150, 100), 100); // out of range: clamped
        assert_eq!(to_percent(5, 5, 5), 0); // broken range
        assert_eq!(from_percent(0, 100, 74), 74);
        assert_eq!(from_percent(0, 48000, 40), 19200);
        assert_eq!(from_percent(10, 110, 0), 10);
        assert_eq!(from_percent(0, 100, 200), 100); // percent clamped
        assert_eq!(from_percent(7, 7, 50), 7);
        for p in 0..=100u8 {
            assert_eq!(to_percent(0, from_percent(0, 255, p), 255), p);
        }
    }

    #[test]
    fn writes_need_a_listed_id_and_clamp_percent() {
        let known = vec!["wmi:A".to_string()];
        assert_eq!(write_arg("wmi:A", 74, &known), Ok(74));
        assert_eq!(write_arg("wmi:A", 255, &known), Ok(100));
        assert!(write_arg("wmi:B", 50, &known).is_err());
        assert!(write_arg("", 50, &[]).is_err());
    }

    #[test]
    fn coalesces_to_the_latest_value_per_display() {
        let c = Arc::new(Coalescer::new());
        let applied = Arc::new(std::sync::Mutex::new(Vec::new()));
        let gate = c.apply.lock().unwrap(); // a slow write is in progress
        let mut threads = Vec::new();
        for p in [10u8, 20, 30] {
            let (c, applied) = (c.clone(), applied.clone());
            threads.push(std::thread::spawn(move || c.run("ddc:1", p, |v| {
                applied.lock().unwrap().push(v);
                Ok(())
            })));
            std::thread::sleep(Duration::from_millis(40)); // queued in this order
        }
        drop(gate);
        for t in threads {
            assert_eq!(t.join().unwrap(), Ok(()));
        }
        assert_eq!(*applied.lock().unwrap(), vec![30]);
        // other displays are independent, errors come back to the caller
        assert_eq!(c.run("ddc:2", 5, |_| Err("DDC failed".into())), Err("DDC failed".to_string()));
    }

    #[test]
    fn parses_brightnessctl_machine_output() {
        let out = "intel_backlight,backlight,19200,40%,48000\nacpi_video0,backlight,5,50%,10\nbroken line\n";
        assert_eq!(brightnessctl(out), vec![("intel_backlight".to_string(), 19200, 48000), ("acpi_video0".to_string(), 5, 10)]);
        assert!(brightnessctl("").is_empty());
    }

    #[test]
    fn parses_ddcutil_output() {
        let detect = "Display 1
   I2C bus:  /dev/i2c-4
   Monitor:  DEL:DELL U2415:ABC123

Invalid display
   I2C bus:  /dev/i2c-5
   Monitor:  XXX:Broken:

Display 2
   I2C bus:  /dev/i2c-6
";
        assert_eq!(ddcutil_detect(detect), vec![("1".to_string(), "DELL U2415".to_string()), ("2".to_string(), "Display 2".to_string())]);
        assert_eq!(ddcutil_vcp("VCP 10 C 50 100\n"), Some((50, 100)));
        assert_eq!(ddcutil_vcp("VCP 10 ERR"), None);
    }
}
