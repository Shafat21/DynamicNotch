import { renderLayoutList } from "./layout-list";

const LABELS: Record<string, string> = {
  clock: "Clock & weather", pal: "Pal (coding agents)", music: "Now playing", calendar: "Calendar",
  launcher: "Launcher apps", focus: "Focus timer", system: "System", tasks: "Tasks", notes: "Notes", reminders: "Next reminder", sound: "Sound & brightness", devices: "Bluetooth", dictation: "Dictation",
};

const DESCS: Record<string, string> = {
  clock: "The time and date with the current weather.",
  pal: "Your newest coding agent session and what it's doing.",
  music: "The song playing, with play / pause and skip.",
  calendar: "Your next event today, or the first one tomorrow.",
  launcher: "A dock of your apps along the bottom of Home.",
  focus: "Start the focus timer from Home (scroll to change the length).",
  system: "CPU and memory use.",
  tasks: "Your next tasks for today.",
  notes: "A one-line quick note: type and press Enter.",
  reminders: "Your next reminder; click to open Reminders.",
  sound: "Volume, output device and brightness sliders.",
  devices: "Bluetooth on / off and your paired devices.",
  dictation: "Your latest dictated texts, to copy again.",
};

export function renderHome(root: HTMLElement): void {
  renderLayoutList(root, "Home", "home", LABELS, "Choose the cards on the Home tab, and their order. Drag the handle, or focus a row and press ↑ / ↓.", DESCS);
}
