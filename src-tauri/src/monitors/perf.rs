//! CPU / RAM / system-drive usage and network speed, once per second. Event: `perf`.

use crate::publish;
use serde::Serialize;
use std::{path::Path, thread, time::Duration, time::Instant};
use sysinfo::{Disks, Networks, System};
use tauri::AppHandle;

#[derive(Serialize)]
struct Perf {
    cpu: f32,
    ram: f64,
    disk: f64,
    down: f64,
    up: f64,
}

pub fn spawn(app: AppHandle) {
    thread::spawn(move || {
        let mut sys = System::new();
        let mut nets = Networks::new_with_refreshed_list();
        let mut disks = Disks::new_with_refreshed_list();
        let drive = format!("{}\\", std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()));
        sys.refresh_cpu_usage();
        let mut last = Instant::now();
        loop {
            thread::sleep(Duration::from_secs(1));
            sys.refresh_cpu_usage();
            sys.refresh_memory();
            nets.refresh(true);
            disks.refresh(true);
            let dt = last.elapsed().as_secs_f64().max(0.001);
            last = Instant::now();

            let (down, up) = nets.iter().fold((0u64, 0u64), |(d, u), (_, n)| (d + n.received(), u + n.transmitted()));
            let disk = disks
                .iter()
                .find(|d| d.mount_point() == Path::new(&drive))
                .or_else(|| disks.iter().next())
                .map(|d| pct(d.total_space() - d.available_space(), d.total_space()))
                .unwrap_or(0.0);

            publish(
                &app,
                "perf",
                Perf {
                    cpu: sys.global_cpu_usage(),
                    ram: pct(sys.used_memory(), sys.total_memory()),
                    disk,
                    down: down as f64 / dt,
                    up: up as f64 / dt,
                },
            );
        }
    });
}

fn pct(part: u64, total: u64) -> f64 {
    if total == 0 { 0.0 } else { part as f64 * 100.0 / total as f64 }
}
