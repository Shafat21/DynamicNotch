// Home tab: the static cards in index.html (`#card-<id>`) are shown, hidden and ordered from the saved layout.
// Launcher and focus live in the bottom strip, everything else in the 4-column grid.
import type { LayoutItem } from "./theme";

const STRIP = new Set(["launcher", "focus", "notes", "reminders"]);

export function renderHome(items: LayoutItem[]): void {
  const grid = document.getElementById("home-grid")!;
  const strip = document.getElementById("home-strip")!;
  for (const it of items) {
    const card = document.getElementById(`card-${it.id}`);
    if (!card) continue;
    card.hidden = !it.visible;
    (STRIP.has(it.id) ? strip : grid).append(card); // append moves it: array order = screen order
  }
  strip.hidden = !items.some((it) => it.visible && STRIP.has(it.id));
}
