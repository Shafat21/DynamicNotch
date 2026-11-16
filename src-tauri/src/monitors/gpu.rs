//! GPU load for the System Status card. Windows: the same "GPU Engine" performance counters Task
//! Manager reads (any vendor); per adapter+engine type the process values add up, the busiest wins.
//! Elsewhere: `None` (the card shows "n/a").

#[cfg(windows)]
pub struct Gpu(windows::Win32::System::Performance::PDH_HQUERY, windows::Win32::System::Performance::PDH_HCOUNTER);

#[cfg(windows)]
impl Gpu {
    pub fn new() -> Option<Gpu> {
        use windows::{core::w, Win32::System::Performance::*};
        unsafe {
            let mut q = PDH_HQUERY::default();
            if PdhOpenQueryW(None, 0, &mut q) != 0 {
                return None;
            }
            let mut c = PDH_HCOUNTER::default();
            if PdhAddEnglishCounterW(q, w!("\\GPU Engine(*)\\Utilization Percentage"), 0, &mut c) != 0 {
                let _ = PdhCloseQuery(q);
                return None;
            }
            let _ = PdhCollectQueryData(q);
            Some(Gpu(q, c))
        }
    }

    /// Percent 0–100, or `None` when the counters are unavailable (no GPU / old driver).
    pub fn read(&self) -> Option<f64> {
        use std::collections::HashMap;
        use windows::Win32::System::Performance::*;
        unsafe {
            if PdhCollectQueryData(self.0) != 0 {
                return None;
            }
            let (mut size, mut count) = (0u32, 0u32);
            PdhGetFormattedCounterArrayW(self.1, PDH_FMT_DOUBLE, &mut size, &mut count, None); // PDH_MORE_DATA: sizes it
            if size == 0 {
                return None;
            }
            let mut buf = vec![0u64; (size as usize).div_ceil(8)];
            let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
            if PdhGetFormattedCounterArrayW(self.1, PDH_FMT_DOUBLE, &mut size, &mut count, Some(items)) != 0 {
                return None;
            }
            // instance: "pid_123_luid_0x0_0xABCD_phys_0_eng_0_engtype_3D" → key "luid…_engtype_3D"
            let mut sums: HashMap<String, f64> = HashMap::new();
            for i in 0..count as usize {
                let it = &*items.add(i);
                let name = it.szName.to_string().unwrap_or_default();
                let key = match (name.find("luid_"), name.find("engtype_")) {
                    (Some(a), Some(b)) if a < b => format!("{}{}", &name[a..a + name[a..].find("_phys").unwrap_or(0)], &name[b..]),
                    _ => name,
                };
                *sums.entry(key).or_default() += it.FmtValue.Anonymous.doubleValue;
            }
            sums.values().cloned().reduce(f64::max).map(|v| v.clamp(0.0, 100.0))
        }
    }
}

#[cfg(windows)]
impl Drop for Gpu {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::System::Performance::PdhCloseQuery(self.0);
        }
    }
}

#[cfg(not(windows))]
pub struct Gpu;

#[cfg(not(windows))]
impl Gpu {
    pub fn new() -> Option<Gpu> {
        None
    }
    pub fn read(&self) -> Option<f64> {
        None
    }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn reads_a_percentage_when_counters_exist() {
        let Some(g) = super::Gpu::new() else { return };
        std::thread::sleep(std::time::Duration::from_secs(1));
        if let Some(v) = g.read() {
            println!("gpu = {v:.1}%");
            assert!((0.0..=100.0).contains(&v));
        }
    }
}
