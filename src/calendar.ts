// Calendar card (dashboard tile) + agenda view, from the `calendar` event (spec §15.3).
// Event titles come from a third-party feed: textContent only.

export type CalEvent = { id: string; title: string; start: number; end: number; allDay: boolean; join: string | null };
/** `fatal`: nothing could be loaded (show `error` instead of events); otherwise `error` is just a note. */
export type CalPayload = { configured: boolean; error: string | null; fatal: boolean; today: CalEvent[]; tomorrowFirst: CalEvent | null };
type Handlers = { join: (id: string) => void; add: () => void };

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
  if (e.join) row.append(button("cal-join glass-btn primary green", "Join", () => h.join(e.id)));
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
  count.textContent = `${left.length} event${left.length === 1 ? "" : "s"} left today`;
  count.title = count.textContent;
  const note = p.error && !p.fatal ? [el("div", "cal-note", p.error)] : [];
  if (left[0]) return body.replaceChildren(eventRow(left[0], h, now, "cal-row"), ...note);

  const rest = el("div", "cal-det");
  rest.append(el("div", "cal-title", p.fatal ? "Calendar unavailable" : "No more events today"));
  const t = p.tomorrowFirst;
  const sub = p.fatal ? p.error : t ? `Tomorrow ${t.allDay ? "" : time(t.start) + " · "}${t.title}` : "";
  if (sub) rest.append(el("div", "cal-when", sub));
  body.replaceChildren(rest, ...note);
}

export function renderAgenda(p: CalPayload | null, h: Handlers): void {
  const now = Date.now();
  const list = $("agenda-list");
  if (!p?.configured) return list.replaceChildren(button("link-btn cal-prompt", "Add your calendar link", h.add));
  const events = [...p.today].sort((a, b) => Number(b.allDay) - Number(a.allDay)); // stable: all-day first, then by start
  const rows = events.map((e) => eventRow(e, h, now, "agenda-row"));
  if (!rows.length) rows.push(el("div", "muted agenda-empty", "No events today"));
  if (p.error) rows.push(el("div", p.fatal ? "agenda-empty" : "muted agenda-empty cal-note", p.error));
  list.replaceChildren(...rows);
}
