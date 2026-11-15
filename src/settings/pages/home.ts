import { renderLayoutList } from "./layout-list";

const LABELS: Record<string, string> = {
  clock: "Clock & weather", pal: "Pal (Claude Code)", music: "Now playing", calendar: "Calendar",
  launcher: "Launcher apps", focus: "Focus timer", system: "System", tasks: "Tasks", notes: "Notes", reminders: "Next reminder", sound: "Sound & brightness",
};

export function renderHome(root: HTMLElement): void {
  renderLayoutList(root, "Home", "home", LABELS, "Choose the cards on the Home tab, and their order. Drag the handle, or focus a row and press ↑ / ↓.");
}
