//! Windows: modifier-only hotkeys (default Ctrl+Win) through a WH_KEYBOARD_LL hook on its own thread.
//! Combos with a regular key go through tauri-plugin-global-shortcut (see `dictation::bind`).

use super::settings::Mods;
use super::Msg;
use std::cell::RefCell;
use std::sync::Mutex;
use windows::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY},
    UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, WH_KEYBOARD_LL, WM_KEYDOWN,
        WM_SYSKEYDOWN,
    },
};

/// Bit i of `Hook::down` = MODS[i] held: LCtrl, RCtrl, LShift, RShift, LAlt, RAlt, LWin, RWin.
const MODS: [u32; 8] = [0xA2, 0xA3, 0xA0, 0xA1, 0xA4, 0xA5, 0x5B, 0x5C];
/// Unassigned virtual key: tapping it before Win goes up stops the Start menu from opening.
const MASK_KEY: u16 = 0xE8;

/// The modifier-only combo to watch; None while the hotkey has a regular key.
static WANT: Mutex<Option<Mods>> = Mutex::new(None);

#[derive(Default)]
pub struct Hook {
    down: u8,
    active: bool,
    /// Another key was pressed during the combo (e.g. Ctrl+Win+→): ignore it until a modifier is released.
    spoiled: bool,
}

impl Hook {
    fn mods(&self) -> Mods {
        let d = self.down;
        Mods { ctrl: d & 0b11 != 0, shift: d & 0b1100 != 0, alt: d & 0b11_0000 != 0, win: d & 0b1100_0000 != 0 }
    }

    /// One physical key event → (dictation event, inject the mask key now).
    /// The mask goes out when the combo fires while Win is held, so it lands between Win-down and Win-up
    /// (injecting it while handling Win-up would queue it behind that key-up, too late for the shell).
    pub fn on_key(&mut self, vk: u32, down: bool, want: Option<Mods>) -> (Option<Msg>, bool) {
        match MODS.iter().position(|m| *m == vk) {
            Some(i) if down => self.down |= 1 << i,
            Some(i) => self.down &= !(1 << i),
            None if down && self.active => {
                self.active = false;
                self.spoiled = true;
                return (Some(Msg::Cancel), false);
            }
            None => {}
        }
        let m = self.mods();
        let matches = want == Some(m);
        if !matches {
            self.spoiled = false;
        }
        // arm only on a key press, so releasing Shift out of Ctrl+Shift+Win doesn't start a recording
        if matches && down && !self.active && !self.spoiled {
            self.active = true;
            return (Some(Msg::Press), m.win);
        }
        if self.active && !matches {
            self.active = false;
            return (Some(Msg::Release), false);
        }
        (None, false)
    }
}

thread_local! {
    static HOOK: RefCell<Hook> = RefCell::new(Hook::default());
}

/// Press (`up = false`) / release virtual keys, in order.
pub fn send_keys(keys: &[(u16, bool)]) -> Result<(), String> {
    let inputs: Vec<INPUT> = keys
        .iter()
        .map(|&(vk, up)| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk), dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, ..Default::default() } },
        })
        .collect();
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err("Couldn't send keystrokes (blocked by another app?)".into())
    }
}

unsafe extern "system" fn hook_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kb = &*(lp.0 as *const KBDLLHOOKSTRUCT);
        // injected events (our own mask key and Ctrl+V) must not change the tracked state
        if kb.flags.0 & LLKHF_INJECTED.0 == 0 {
            let down = matches!(wp.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
            let want = *WANT.lock().unwrap_or_else(|e| e.into_inner()); // a panic must never unwind out of the hook
            let (msg, inject) = HOOK.with(|h| h.borrow_mut().on_key(kb.vkCode, down, want));
            if inject {
                let _ = send_keys(&[(MASK_KEY, false), (MASK_KEY, true)]);
            }
            if let Some(m) = msg {
                super::send(m);
            }
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

pub fn spawn() {
    std::thread::spawn(|| unsafe {
        let module = GetModuleHandleW(None).ok().map(|m| m.into());
        if let Err(e) = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), module, 0) {
            eprintln!("dictation: keyboard hook failed: {e}");
            return;
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}

pub fn set(mods: Option<Mods>) -> Result<(), String> {
    *WANT.lock().unwrap_or_else(|e| e.into_inner()) = mods;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LCTRL: u32 = 0xA2;
    const LWIN: u32 = 0x5B;
    const SHIFT: u32 = 0xA0;
    const D: u32 = 0x44;

    fn run(h: &mut Hook, want: Mods, keys: &[(u32, bool)]) -> Vec<(Option<Msg>, bool)> {
        keys.iter().map(|&(vk, down)| h.on_key(vk, down, Some(want))).collect()
    }

    #[test]
    fn ctrl_win_press_release_masks_start_menu() {
        let want = Mods { ctrl: true, win: true, ..Default::default() };
        let r = run(&mut Hook::default(), want, &[(LCTRL, true), (LWIN, true), (LWIN, true), (LWIN, false), (LCTRL, false)]);
        // mask injected when the combo fires (Win still down), never on the key-ups
        assert_eq!(r, [(None, false), (Some(Msg::Press), true), (None, false), (Some(Msg::Release), false), (None, false)]);
        let r = run(&mut Hook::default(), want, &[(LWIN, true), (0xA3, true), (0xA3, false), (LWIN, false)]);
        assert_eq!(r, [(None, false), (Some(Msg::Press), true), (Some(Msg::Release), false), (None, false)]);
        // a combo without Win needs no mask
        let r = run(&mut Hook::default(), Mods { ctrl: true, alt: true, ..Default::default() }, &[(LCTRL, true), (0xA4, true)]);
        assert_eq!(r, [(None, false), (Some(Msg::Press), false)]);
    }

    #[test]
    fn other_key_cancels_until_modifier_released() {
        let want = Mods { ctrl: true, win: true, ..Default::default() };
        let mut h = Hook::default();
        let r = run(&mut h, want, &[(LCTRL, true), (LWIN, true), (D, true), (LWIN, true), (D, false), (LWIN, false), (LWIN, true)]);
        assert_eq!(r, [(None, false), (Some(Msg::Press), true), (Some(Msg::Cancel), false), (None, false), (None, false), (None, false), (Some(Msg::Press), true)]);
    }

    #[test]
    fn extra_modifier_or_no_combo_never_fires() {
        let want = Mods { ctrl: true, win: true, ..Default::default() };
        let r = run(&mut Hook::default(), want, &[(LCTRL, true), (SHIFT, true), (LWIN, true), (SHIFT, false), (LWIN, false)]);
        assert!(r.iter().all(|e| e.0.is_none()));
        let mut h = Hook::default();
        assert_eq!(h.on_key(LCTRL, true, None), (None, false));
        assert_eq!(h.on_key(LWIN, true, None), (None, false));
    }
}
