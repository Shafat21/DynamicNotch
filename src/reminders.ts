// Reminders (spec §23.2): the due banner + chime, the Reminders tab (upcoming, done today, new-reminder form)
// and the Home strip chip. Reminder text is user text: textContent / .value only.
import { invoke } from "@tauri-apps/api/core";
import { icon } from "./icons-line";
import { updateMood } from "./pal";

type Repeat = "none" | "daily" | "weekdays" | "weekly";
type Reminder = { id: number; text: string; dueAt: number; repeat: Repeat; done: boolean; snoozeUntil: number | null; doneAt: number | null };
export type Due = { id: number; text: string; dueAt: number }; // `reminder-due` payload

const CHIPS: [string, string][] = [
  ["10m", "In 10 min"], ["30m", "In 30 min"], ["1h", "In 1 hour"], ["evening", "This evening"], ["tomorrow", "Tomorrow morning"], ["pick", "Pick a time"],
];
const REPEATS: Record<Repeat, string> = { none: "Once", daily: "Every day", weekdays: "Weekdays", weekly: "Every week" };
const CHIME_EVERY = 30_000;
const CHIME_TIMES = 3;

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const at = (r: Reminder) => r.snoozeUntil ?? r.dueAt;
let list: Reminder[] = []; // upcoming (soonest first) + done today, as Rust returns it
let queue: Due[] = []; // due and unanswered, oldest first; [0] is on the banner
let hooks = { changed: () => {}, sound: () => true };
const form = { chip: "30m", repeat: "none" as Repeat, dueAt: 0 };

function el(tag: string, cls: string, text?: string): HTMLElement {
  const e = document.createElement(tag);
  e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

function button(cls: string, label: string, onClick: () => void, iconName?: string): HTMLButtonElement {
  const b = el("button", cls, iconName ? undefined : label) as HTMLButtonElement;
  b.type = "button";
  if (iconName) {
    b.innerHTML = icon(iconName, 15); // static SVG
    b.title = label;
  }
  b.onclick = onClick;
  return b;
}

// ---------- time labels ----------
const time = (ms: number) => new Date(ms).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });

function day(ms: number): string {
  const days = Math.round((new Date(ms).setHours(0, 0, 0, 0) - new Date().setHours(0, 0, 0, 0)) / 86_400_000); // round: DST days
  if (days === 0) return "Today";
  if (days === 1) return "Tomorrow";
  return new Date(ms).toLocaleDateString("en-US", { weekday: "short", day: "numeric", month: "short" });
}

const when = (ms: number) => `${day(ms)} at ${time(ms)}`;

function relative(ms: number): string {
  const min = Math.round((ms - Date.now()) / 60_000);
  if (min <= 0) return "Now";
  if (min < 60) return `In ${min} min`;
  const h = Math.floor(min / 60);
  if (h < 24) return min % 60 ? `In ${h} h ${min % 60} min` : `In ${h} h`;
  const d = Math.round(h / 24);
  return `In ${d} day${d === 1 ? "" : "s"}`;
}

// ---------- due queue + banner ----------
export const currentDue = (): Due | null => queue[0] ?? null;

const mood = () => updateMood({ reminderDue: queue.length > 0 });

/** Queues a due reminder (once per id); true when it is new. */
export function addDue(d: Due): boolean {
  if (queue.some((q) => q.id === d.id)) return false;
  queue.push(d);
  mood();
  renderBanner();
  return true;
}

/** Drops answered / deleted / no longer due ids; tells main.ts when the banner's reminder went. */
function dropStale(): void {
  const now = Date.now();
  const head = queue[0]?.id;
  queue = queue.filter((q) => list.some((r) => r.id === q.id && !r.done && at(r) <= now));
  mood();
  renderBanner();
  if (head !== undefined && queue[0]?.id !== head) hooks.changed();
}

export function renderBanner(): void {
  const d = queue[0];
  if (!d) return;
  const ic = el("span", "rem-ic");
  ic.innerHTML = icon("bell", 22); // static SVG
  const text = el("div", "col rem-text");
  const title = el("b", "rem-title ell", d.text);
  title.title = d.text;
  text.append(title, el("span", "muted ell", `Reminder · ${time(d.dueAt)}${queue.length > 1 ? ` · 1 of ${queue.length}` : ""}`));
  $("reminder").replaceChildren(
    ic,
    text,
    button("pill", "Snooze 10 min", () => answer("snooze_reminder", d.id)),
    button("pill light", "Done", () => answer("done_reminder", d.id)),
  );
}

async function answer(cmd: "snooze_reminder" | "done_reminder" | "delete_reminder", id: number): Promise<void> {
  $("reminder").querySelectorAll("button").forEach((b) => (b.disabled = true));
  try {
    setList(await invoke<Reminder[]>(cmd, { id }));
  } catch (e) {
    console.error(e);
    await refresh(); // still due → stays on the banner (buttons re-enabled by the re-render)
    renderBanner();
  }
}

// ---------- chime ----------
let audio: AudioContext | null = null;
let chimeId: number | null = null;
let chimeTimer = 0;

/** Soft two-tone (Web Audio, no files). */
function beep(): void {
  audio ??= new AudioContext();
  void audio.resume();
  const t0 = audio.currentTime;
  for (const [freq, dt] of [[880, 0], [660, 0.22]]) {
    const osc = audio.createOscillator();
    const gain = audio.createGain();
    osc.frequency.value = freq;
    gain.gain.setValueAtTime(0.0001, t0 + dt);
    gain.gain.exponentialRampToValueAtTime(0.12, t0 + dt + 0.03);
    gain.gain.exponentialRampToValueAtTime(0.0001, t0 + dt + 0.6);
    osc.connect(gain).connect(audio.destination);
    osc.start(t0 + dt);
    osc.stop(t0 + dt + 0.65);
  }
}

/** Chime for the reminder on the banner (null = none on screen): now, then every 30 s, 3 times in all. */
export function chime(id: number | null): void {
  if (id === chimeId) return;
  chimeId = id;
  clearInterval(chimeTimer);
  if (id === null || !hooks.sound()) return;
  let n = 1;
  beep();
  chimeTimer = window.setInterval(() => {
    if (++n >= CHIME_TIMES) clearInterval(chimeTimer);
    if (hooks.sound()) beep();
  }, CHIME_EVERY);
}

// ---------- tab + Home chip ----------
async function refresh(): Promise<void> {
  try {
    setList(await invoke<Reminder[]>("list_reminders"));
  } catch (e) {
    console.error(e);
  }
}

function setList(l: Reminder[]): void {
  list = l;
  dropStale();
  draw();
}

function row(r: Reminder, done: boolean): HTMLElement {
  const box = el("div", "rem-row" + (done ? " done" : ""));
  const col = el("div", "col");
  const name = el("span", "ell rem-name", r.text);
  name.title = r.text;
  const sub = done ? `Done at ${time(r.doneAt ?? 0)}` : `${when(at(r))}${r.repeat !== "none" ? ` · ${REPEATS[r.repeat]}` : ""}`;
  col.append(name, el("span", "muted small ell", sub));
  box.append(col);
  if (!done) box.append(button("action-btn", "Done", () => answer("done_reminder", r.id), "check"));
  box.append(button("action-btn", "Delete", () => answer("delete_reminder", r.id), "trash"));
  return box;
}

function draw(): void {
  const startOfDay = new Date().setHours(0, 0, 0, 0);
  const upcoming = list.filter((r) => !r.done);
  const doneToday = list.filter((r) => r.doneAt !== null && r.doneAt >= startOfDay);
  $("rem-count").textContent = upcoming.length ? `${upcoming.length} upcoming` : "";
  $("rem-upcoming").replaceChildren(...(upcoming.length ? upcoming.map((r) => row(r, false)) : [el("div", "muted rem-empty", "No reminders. Press + to add one.")]));
  $("rem-done").replaceChildren(...(doneToday.length ? doneToday.map((r) => row(r, true)) : [el("div", "muted rem-empty", "Nothing yet")]));
  const next = upcoming[0];
  $("rem-next").textContent = next ? `${next.text} · ${time(at(next))}` : "No reminders";
  $("rem-next").classList.toggle("muted", !next);
}

/** Reminders tab opened: fresh list, list view. */
export function showReminders(): void {
  showForm(false);
  void refresh();
}

// ---------- new-reminder form ----------
/** `<input type="datetime-local">` value for a local time. */
const localInput = (ms: number) => new Date(ms - new Date(ms).getTimezoneOffset() * 60_000).toISOString().slice(0, 16);

function showForm(on: boolean): void {
  $("rem-view").hidden = on;
  $("rem-form").hidden = !on;
  if (!on) return;
  $<HTMLInputElement>("rem-text").value = "";
  form.repeat = "none";
  void pickChip("30m");
  window.setTimeout(() => $("rem-text").focus(), 50);
}

/** Chips compute their time now for the summary, and again on submit ("In 10 min" means from then). */
async function chipTime(): Promise<number> {
  if (form.chip === "pick") return new Date($<HTMLInputElement>("rem-pick").value).getTime();
  return invoke<number>("quick_due", { chip: form.chip });
}

async function pickChip(chip: string): Promise<void> {
  form.chip = chip;
  const pick = $<HTMLInputElement>("rem-pick");
  pick.hidden = chip !== "pick";
  if (chip === "pick" && !pick.value) pick.value = localInput(Date.now() + 3_600_000);
  form.dueAt = await chipTime().catch(() => NaN);
  drawForm();
}

function drawForm(err = ""): void {
  const chips = (box: string, items: [string, string][], on: string, pick: (v: string) => void) =>
    $(box).replaceChildren(
      ...items.map(([v, label]) => {
        const b = button("opt" + (v === on ? " on" : ""), label, () => pick(v));
        b.setAttribute("aria-pressed", String(v === on));
        return b;
      }),
    );
  chips("rem-when", CHIPS, form.chip, (v) => void pickChip(v));
  chips("rem-repeat", Object.entries(REPEATS), form.repeat, (v) => {
    form.repeat = v as Repeat;
    drawForm();
  });
  const s = $("rem-summary");
  const ok = Number.isFinite(form.dueAt);
  s.textContent = err || (ok ? `${when(form.dueAt)} · ${relative(form.dueAt)}${form.repeat !== "none" ? ` · ${REPEATS[form.repeat]}` : ""}` : "Pick a time");
  s.classList.toggle("rem-err", !!err);
}

async function submit(): Promise<void> {
  const text = $<HTMLInputElement>("rem-text");
  if (!text.value.trim()) return text.focus();
  try {
    const dueAt = await chipTime();
    if (!Number.isFinite(dueAt)) throw "Pick a time";
    setList(await invoke<Reminder[]>("add_reminder", { text: text.value, dueAt, repeat: form.repeat }));
    showForm(false);
  } catch (e) {
    drawForm(String(e));
  }
}

/** `changed`: the banner's reminder was answered (show the next one or close). `sound`: Settings → Reminder sound. */
export function initReminders(h: typeof hooks): void {
  hooks = h;
  $("rem-new").onclick = () => showForm(true);
  $("rem-cancel").onclick = () => showForm(false);
  $("rem-pick").onchange = () => void pickChip("pick");
  $("rem-form").onsubmit = (e) => {
    e.preventDefault();
    void submit();
  };
}

/** Not done and already due: missed while the app was closed, or before this page loaded. */
export async function overdue(): Promise<Due[]> {
  await refresh();
  const now = Date.now();
  return list.filter((r) => !r.done && at(r) <= now).map((r) => ({ id: r.id, text: r.text, dueAt: at(r) }));
}
