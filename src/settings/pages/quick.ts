import { renderLayoutList } from "./layout-list";

const LABELS: Record<string, string> = { call: "Record a call", pal: "Pal", shot: "Screenshot", pin: "Keep open" };

const DESCS: Record<string, string> = {
  call: "Record a call and get a transcript (and a summary) in Notes.",
  pal: "Your coding agents at a glance; click to open the Pal tab.",
  shot: "Drag an area, mark it up, copy or save it (the hotkey is in General).",
  pin: "Keep the island open after the pointer leaves it.",
};

export function renderQuick(root: HTMLElement): void {
  renderLayoutList(root, "Top buttons", "quick", LABELS, "The buttons at the island's top right, and their order. Settings and the collapse button always stay. Drag the handle, or focus a row and press ↑ / ↓.", DESCS);
}
