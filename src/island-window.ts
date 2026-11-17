// Which island window this page is. With "Both displays" a second window ("island2") runs the same page on the other
// monitor: it listens for its own hover / display / fullscreen events (suffixed, see window.rs `evt`) and stays
// quiet where two copies would double up (the reminder chime).
import { getCurrentWindow } from "@tauri-apps/api/window";

export const label = getCurrentWindow().label;
export const primary = label === "main";
/** Event-name suffix: "" for the first island, ":island2" for the second. */
export const sfx = primary ? "" : `:${label}`;
