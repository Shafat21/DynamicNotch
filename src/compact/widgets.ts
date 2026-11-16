// Compact notch: Widgets card. "This week" (Mon–Sun, today highlighted, a dot where the calendar has events)
// and a power ring (power_status: AC + battery percent where the PC has a battery). Event titles: textContent.
import { invoke } from "@tauri-apps/api/core";
import type { CalPayload } from "../calendar";
import { ring } from "./compact";
import { weekOf } from "./week";

type Power = { plugged: boolean | null; percent: number | null };
const node = (tag: string, cls: string, text = "") => Object.assign(document.createElement(tag), { className: cls, textContent: text });

export function renderWidgets(el: HTMLElement, s: { calendar: CalPayload | null; taskCount: number }): void {
  const now = new Date();
  const sameDay = (a: Date, b: Date) => a.toDateString() === b.toDateString();
  const events = [...(s.calendar?.today ?? []), ...(s.calendar?.tomorrowFirst ? [s.calendar.tomorrowFirst] : [])];

  const week = node("div", "cw-week");
  week.append(node("div", "cw-label", "THIS WEEK"));
  const grid = node("div", "cw-days");
  for (const day of weekOf(now)) {
    const cell = node("div", `cw-day${sameDay(day, now) ? " today" : ""}${day < now && !sameDay(day, now) ? " past" : ""}`);
    cell.append(node("span", "cw-dow", day.toLocaleDateString(undefined, { weekday: "narrow" })), node("b", "cw-num", String(day.getDate())));
    if (events.some((e) => sameDay(new Date(e.start), day))) cell.append(node("i", "cw-dot"));
    grid.append(cell);
  }
  week.append(grid);
  const next = (s.calendar?.today ?? []).find((e) => !e.allDay && e.end > now.getTime());
  const line = node("div", "cw-line small");
  line.append(node("i", "cw-pip"), node("b", "", `${s.taskCount} task${s.taskCount === 1 ? "" : "s"}`));
  if (next) line.append(node("span", "muted ell", ` · Next: ${next.title}, ${new Date(next.start).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })}`));
  week.append(line);

  const power = node("div", "cw-power");
  power.append(node("div", "cw-label", "POWER"));
  const r = ring(54);
  const body = node("div", "cw-pbody");
  const pct = node("b", "cw-pct", "…");
  const state = node("span", "cw-state small", "");
  const col = node("div", "");
  col.append(pct, state);
  body.append(r.el, col);
  power.append(body);
  const wrap = node("div", "cw");
  wrap.append(week, power);
  el.replaceChildren(wrap);

  invoke<Power>("power_status").then(
    (p) => {
      r.set(p.percent === null ? null : p.percent / 100);
      pct.textContent = p.percent === null ? (p.plugged ? "AC" : "—") : `${p.percent}%`;
      state.textContent = p.plugged === null ? "Unknown" : p.plugged ? (p.percent !== null && p.percent < 100 ? "Charging" : "Plugged in") : "On battery";
      power.classList.toggle("charging", p.plugged === true);
    },
    () => (state.textContent = "Unknown"),
  );
}
