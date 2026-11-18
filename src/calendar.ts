// Calendar card (dashboard tile) + the Calendar tab (month grid and the picked day), from the `calendar` event (spec §15.3).
// Event titles come from a third-party feed: textContent only.

export type CalEvent = { id: string; title: string; start: number; end: number; allDay: boolean; join: string | null };
/** `fatal`: nothing could be loaded (show `error` instead of events); otherwise `error` is just a note. */
export type CalPayload = { configured: boolean; desktop?: boolean; error: string | null; fatal: boolean; today: CalEvent[]; tomorrowFirst: CalEvent | null; events?: CalEvent[] };
type Handlers = { join: (id: string) => void; add: () => void; desktop: () => void };

import { dayBounds, eventsOn, monthCells, shiftMonth } from "./calendar-grid";

const $ = (id: string) => document.getElementById(id)!;
const time = (ms: number) => new Date(ms).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });

function el(tag: string, cls: string, text?: string): HTMLElement {
  const e = document.createElement(tag);
  e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

/** "All day", "Now · until 3:30 PM", "In 20 min · 2:00 PM" (under an hour away) or "2:00 PM". */
export function relLabel(e: CalEvent, now = Date.now()): string {
  if (e.allDay) return "All day";
  if (e.start <= now && now < e.end) return `Now · until ${time(e.end)}`;
  const min = Math.ceil((e.start - now) / 60000);
  return min > 0 && min < 60 ? `In ${min} min · ${time(e.start)}` : time(e.start);
}

/** A button that doesn't also trigger the card's click (open agenda). */
function button(cls: string, text: string, onClick: () => void): HTMLElement {
  const b = el("button", cls, text);
  b.onclick = (ev) => {
    ev.stopPropagation();
    onClick();
  };
  return b;
}

function eventRow(e: CalEvent, h: Handlers, now: number, cls: string): HTMLElement {
  const row = el("div", cls);
  const det = el("div", "cal-det");
  det.append(el("div", "cal-title", e.title), el("div", "cal-when", relLabel(e, now)));
  row.append(el("span", "cal-bar"), det);
  if (e.join) row.append(button("cal-join", "Join", () => h.join(e.id)));
  return row;
}

export function renderCard(p: CalPayload | null, h: Handlers): void {
  const d = new Date();
  const now = d.getTime();
  $("cal-date").textContent = `${d.toLocaleDateString("en-US", { weekday: "short" })}, ${d.getDate()} ${d.toLocaleDateString("en-US", { month: "short" })}`;
  const body = $("cal-body");
  const count = $("cal-count");
  count.textContent = "";
  if (!p?.configured) return body.replaceChildren(button("link-btn cal-prompt", "Add your calendar", h.add));

  const left = p.today.filter((e) => !e.allDay && e.end > now); // sorted by start: ongoing first, then upcoming
  count.textContent = left.length ? `${left.length} event${left.length === 1 ? "" : "s"} left today` : ""; // the body says it when there are none
  count.title = count.textContent;
  const note = p.error && !p.fatal ? [el("div", "cal-note", p.error)] : [];
  if (left[0]) return body.replaceChildren(eventRow(left[0], h, now, "cal-row"), ...note);

  const rest = el("div", "cal-det");
  rest.append(el("div", "cal-title", p.fatal ? "Calendar unavailable" : "You're free today"));
  const t = p.tomorrowFirst;
  const sub = p.fatal ? p.error : t ? `Tomorrow ${t.allDay ? "" : time(t.start) + " · "}${t.title}` : "";
  if (sub) rest.append(el("div", "cal-when", sub));
  body.replaceChildren(rest, ...note);
}

// ---------- Calendar tab: a month grid on the left, the picked day's events on the right ----------
const today0 = () => {
  const d = new Date();
  return { y: d.getFullYear(), m: d.getMonth(), d: d.getDate() };
};
let view = { y: today0().y, m: today0().m }; // month on show
let sel = today0(); // day whose events are listed
let last: { p: CalPayload | null; h: Handlers } | null = null;
let wired = false;
const WEEKDAYS = ["M", "T", "W", "T", "F", "S", "S"];

function showMonth(by: number): void {
  view = shiftMonth(view.y, view.m, by);
  if (last) renderAgenda(last.p, last.h);
}

function wireOnce(): void {
  if (wired) return;
  wired = true;
  $("cal-prev").onclick = () => showMonth(-1);
  $("cal-next").onclick = () => showMonth(1);
  $("cal-today").onclick = () => {
    sel = today0();
    view = { y: sel.y, m: sel.m };
    if (last) renderAgenda(last.p, last.h);
  };
}

export function renderAgenda(p: CalPayload | null, h: Handlers): void {
  last = { p, h };
  wireOnce();
  const now = Date.now();
  const events = p?.configured ? (p.events ?? p.today) : [];
  const t = today0();

  $("cal-month-title").textContent = new Date(view.y, view.m, 1).toLocaleDateString("en-US", { month: "long", year: "numeric" });
  const cells = monthCells(view.y, view.m, 1);
  if (!cells[35].inMonth) cells.length = 35; // a month that fits five weeks gets taller rows
  $("cal-grid").style.setProperty("--weeks", String(cells.length / 7));
  $("cal-grid").replaceChildren(
    ...WEEKDAYS.map((w) => el("span", "cal-wd", w)),
    ...cells.map((c) => {
      const n = eventsOn(events, c.y, c.m, c.d).length;
      const b = el("button", "cal-day") as HTMLButtonElement;
      b.type = "button";
      b.append(el("span", "cal-n", String(c.d)));
      if (n) b.append(el("span", `cal-dots d${Math.min(n, 3)}`));
      b.classList.toggle("out", !c.inMonth);
      b.classList.toggle("today", c.y === t.y && c.m === t.m && c.d === t.d);
      b.classList.toggle("picked", c.y === sel.y && c.m === sel.m && c.d === sel.d);
      const long = new Date(c.y, c.m, c.d).toLocaleDateString("en-US", { weekday: "long", day: "numeric", month: "long" });
      b.title = n ? `${long} · ${n} event${n === 1 ? "" : "s"}` : long;
      b.onclick = () => {
        sel = { y: c.y, m: c.m, d: c.d };
        if (!c.inMonth) view = { y: c.y, m: c.m };
        renderAgenda(p, h);
      };
      return b;
    }),
  );

  const isToday = sel.y === t.y && sel.m === t.m && sel.d === t.d;
  const when = new Date(sel.y, sel.m, sel.d);
  $("cal-day-title").textContent = isToday ? "Today" : when.toLocaleDateString("en-US", { weekday: "long" });
  $("cal-day-num").textContent = String(sel.d);
  $("cal-day-sub").textContent = when.toLocaleDateString("en-US", { month: "long", year: sel.y === t.y ? undefined : "numeric" });
  $("cal-today").hidden = isToday && view.y === t.y && view.m === t.m;

  const list = $("agenda-list");
  if (!p?.configured) {
    const empty = el("div", "agenda-setup");
    empty.append(
      el("div", "agenda-setup-title", "Show your events here"),
      el("div", "muted small", "Read the calendar app on this PC, or paste a private iCal link from Google, Outlook or iCloud."),
    );
    const actions = el("div", "agenda-setup-actions");
    actions.append(button("btn primary", "Use desktop calendar", h.desktop), button("btn", "Add iCal link", h.add));
    return list.replaceChildren(empty, actions);
  }
  const [d0, d1] = dayBounds(sel.y, sel.m, sel.d);
  const day = eventsOn(events, sel.y, sel.m, sel.d);
  $("cal-day-sub").textContent += day.length ? ` · ${day.length} event${day.length === 1 ? "" : "s"}` : "";
  const rows = day.map((e) => {
    const row = el("div", "cal-ev");
    const live = isToday && !e.allDay && e.start <= now && now < e.end;
    if (isToday && !e.allDay && e.end <= now) row.classList.add("past");
    if (live) row.classList.add("now");
    const at = e.allDay ? "All day" : live ? "Now" : time(Math.max(e.start, d0)).replace(":00", "");
    row.title = `${e.title}
${e.allDay ? "All day" : `${time(Math.max(e.start, d0))} - ${time(Math.min(e.end, d1))}`}`;
    row.append(el("span", "cal-ev-at", at), el("span", "cal-ev-title", e.title));
    if (isToday && !e.allDay && !live && e.start > now && e.start - now < 3600000) row.append(el("span", "cal-ev-soon", relLabel(e, now).split(" · ")[0]));
    if (e.join) row.append(button("cal-join", "Join", () => h.join(e.id)));
    return row;
  });
  if (!rows.length) rows.push(el("div", "muted agenda-empty", isToday ? "No events today" : "Nothing planned"));
  if (p.error) rows.push(el("div", p.fatal ? "agenda-empty" : "muted agenda-empty cal-note", p.error));
  list.replaceChildren(...rows);
  fit(list, rows, h);
}

// no scrollbar: rows that don't fit go behind a "+N more" pager that steps through the day
let pageAt = 0;
let pageKey = "";
function fit(list: HTMLElement, rows: HTMLElement[], h: Handlers): void {
  const key = `${sel.y}-${sel.m}-${sel.d}`;
  if (key !== pageKey) (pageKey = key), (pageAt = 0);
  if (pageAt >= rows.length) pageAt = 0;
  requestAnimationFrame(() => {
    let shown = rows.slice(pageAt);
    list.replaceChildren(...shown);
    if (list.scrollHeight <= list.clientHeight && !pageAt) return;
    const more = button("cal-more", "", () => {
      pageAt += shown.length;
      if (last) renderAgenda(last.p, h);
    });
    const label = () => (more.textContent = pageAt + shown.length < rows.length ? `+${rows.length - pageAt - shown.length} more` : "Back to start");
    list.append(more);
    label();
    while (shown.length > 1 && list.scrollHeight > list.clientHeight) {
      shown = shown.slice(0, -1);
      list.replaceChildren(...shown, more);
      label();
    }
  });
}
