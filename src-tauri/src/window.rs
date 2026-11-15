//! Window placement + click-through: the window is 1200x700 and transparent, but only the island
//! should take the mouse. OS-specific code lives in `window_{windows,linux}.rs`.

use serde::Deserialize;

#[cfg(windows)]
#[path = "window_windows.rs"]
mod os;
#[cfg(target_os = "linux")]
#[path = "window_linux.rs"]
mod os;
pub use os::{set_hit_rect, setup};

#[derive(Deserialize, Clone, Copy, Default, Debug)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

#[cfg(test)]
mod tests {
    use super::Rect;

    #[test]
    fn contains_edges() {
        let r = Rect { x: 10.0, y: 0.0, w: 100.0, h: 40.0 };
        assert!(r.contains(10.0, 0.0) && r.contains(110.0, 40.0) && r.contains(50.0, 20.0));
        assert!(!r.contains(9.9, 20.0) && !r.contains(50.0, 40.1));
    }
}
