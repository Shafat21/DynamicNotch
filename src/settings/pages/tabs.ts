import { renderLayoutList } from "./layout-list";

const LABELS: Record<string, string> = {
  home: "Home", music: "Music", calendar: "Calendar", pal: "Pal", timer: "Timer", launcher: "Launcher",
  weather: "Weather", system: "System", tasks: "Tasks", month: "Progress", notes: "Notes", reminders: "Reminders", clipboard: "Clipboard", sound: "Sound", devices: "Devices", shelf: "Shelf",
};

const DESCS: Record<string, string> = {
  home: "Your dashboard: the cards you pick on the Home page, plus the launcher dock, quick note and focus timer.",
  music: "The song playing in any app: controls, volume, synced lyrics, and Spotify search, playlists and queue.",
  calendar: "A month grid with your events; pick a day to see what's on. Needs your calendar (Integrations).",
  pal: "Your coding agents (Claude Code, Codex, Gemini…): what each session is doing, and a box to message it.",
  timer: "A focus timer with 15 / 25 / 45 / 60 min presets. It also shows in the notch while it runs.",
  launcher: "Your apps, a search box and quick links: type “yt lofi” or paste an address and press Enter.",
  weather: "Today's weather and the next hours for your city (set in Integrations).",
  system: "Live CPU, memory, GPU and battery use.",
  tasks: "A short to-do list for today, with a time and a category per task.",
  month: "How far through today, this week, this month and this year; plus your own countdowns.",
  notes: "Quick notes and voice notes, searchable, with pinning. Call notes land here too.",
  reminders: "Reminders that pop up on the island at the time you set, once or repeating.",
  clipboard: "Text you copied lately; click to copy it again, pin the ones you keep.",
  sound: "Volume, mute, output device and screen brightness.",
  devices: "Bluetooth devices, and USB / wireless ones with their battery (Logitech).",
  shelf: "Drop files on the island to keep them handy, then drag them out wherever you need them.",
};

export function renderTabs(root: HTMLElement): void {
  renderLayoutList(root, "Tabs", "tabs", LABELS, "Choose which tabs the island's tab bar shows, and their order. Drag the handle, or focus a row and press ↑ / ↓.", DESCS);
}
