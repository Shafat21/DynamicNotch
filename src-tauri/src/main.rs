// Hide the console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Claude Code statusLine forwarder: no Tauri, no window, always exit 0.
    if std::env::args().any(|a| a == dynamic_notch_lib::statusline::FLAG) {
        let _ = std::panic::catch_unwind(dynamic_notch_lib::statusline::forward);
        std::process::exit(0);
    }
    dynamic_notch_lib::run()
}
