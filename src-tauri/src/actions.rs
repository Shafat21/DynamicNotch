//! Basics panel + "open app" buttons. The frontend sends an action id; only ids listed here can run,
//! so the webview can never pass an arbitrary shell command.

use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn command_for(id: &str) -> Option<(&'static str, &'static [&'static str])> {
    // `cmd /C start "" <target>` lets Windows resolve URIs, app aliases and elevation prompts.
    Some(match id {
        "shutdown" => ("shutdown", &["/s", "/t", "0"]),
        "restart" => ("shutdown", &["/r", "/t", "0"]),
        "sleep" => ("rundll32.exe", &["powrprof.dll,SetSuspendState", "0,1,0"]),
        "explorer" => ("explorer.exe", &[]),
        "settings" => ("cmd", &["/C", "start", "", "ms-settings:"]),
        "taskmgr" => ("cmd", &["/C", "start", "", "taskmgr"]),
        "chrome" => ("cmd", &["/C", "start", "", "chrome"]),
        "youtube" => ("cmd", &["/C", "start", "", "https://www.youtube.com"]),
        "cmd" => ("cmd", &["/C", "start", "", "cmd"]),
        "weather" => ("cmd", &["/C", "start", "", "bingweather:"]),
        "datetime" => ("cmd", &["/C", "start", "", "ms-settings:dateandtime"]),
        _ => return None,
    })
}

pub fn run(id: &str) -> Result<(), String> {
    let (prog, args) = command_for(id).ok_or_else(|| format!("unknown action: {id}"))?;
    let mut cmd = Command::new(prog);
    cmd.args(args);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::command_for;

    #[test]
    fn only_known_ids_map_to_commands() {
        assert!(command_for("shutdown").is_some());
        assert!(command_for("youtube").is_some());
        assert!(command_for("calc & del *").is_none());
        assert!(command_for("").is_none());
    }
}
