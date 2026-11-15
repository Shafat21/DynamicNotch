import { renderLayoutList } from "./layout-list";

const LABELS: Record<string, string> = {
  home: "Home", music: "Music", calendar: "Calendar", pal: "Pal", timer: "Timer", launcher: "Launcher",
  weather: "Weather", system: "System", tasks: "Tasks", month: "Month", notes: "Notes", reminders: "Reminders",
};

export function renderTabs(root: HTMLElement): void {
  renderLayoutList(root, "Tabs", "tabs", LABELS, "Choose which tabs the island's tab bar shows, and their order. Drag the handle, or focus a row and press ↑ / ↓.");
}
