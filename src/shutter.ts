// Screenshot shutter sound: played by the app itself (src-tauri/src/capture.rs, Windows sound API) when a screenshot is
// copied or saved, so it doesn't depend on a webview being allowed to play audio. These only read / set the switch
// and play the Test sound from Settings → General.
import { invoke } from "@tauri-apps/api/core";

export const shutterOn = () => invoke<boolean>("capture_sound");
export const setShutter = (on: boolean) => invoke<void>("set_capture_sound", { on });
export const testShutter = () => invoke<void>("test_capture_sound");
