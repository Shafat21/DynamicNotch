// Alarms, in the Timer tab next to the focus timer: a time, a label, which days, on / off. When one goes off the
// island opens on the Timer tab with a ringing card (Snooze 5 min / Stop) and a repeating tone until answered.
// shortcut: alarms live in this webview's localStorage and ring only while the app runs (a phone-style alarm that
// wakes a sleeping PC would need a Windows scheduled task); move to the DB if they ever need to sync.
// Labels are user text: textContent / .value only.
import { primary } from "./island-window";
import { icon } from "./icons-line";
import { daysText, dueNow, nextRing, untilText, type Alarm } from "./alarm-math";

const KEY = "dn.alarms";
const SNOOZE_MS = 5 * 60_000;
const RING_MAX_MS = 5 * 60_000; // an unanswered alarm stops by itself after this
const DAYS: [number, string][] = [[1, "M"], [2, "T"], [3, "W"], [4, "T"], [5, "F"], [6, "S"], [0, "S"]];

const el = <K extends keyof HTMLElementTagNameMap>(tag: K, cls: string, text?: string) =>
  Object.assign(document.createElement(tag), { className: cls, ...(text === undefined ? {} : { textContent: text }) });

let hooks = { ring: (_a: Alarm) => {}, stopped: () => {} };
let ringing: Alarm | null = null;
let ringTimer = 0;
let ringStarted = 0;
let editing: Alarm | null = null; // the alarm the form edits (null = a new one)

function load(): Alarm[] {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    return Array.isArray(v) ? v.filter((a) => a && typeof a.time === "string" && Array.isArray(a.days)) : [];
  } catch {
    return [];
  }
}

function save(list: Alarm[]): void {
  localStorage.setItem(KEY, JSON.stringify(list.slice(0, 30)));
  draw();
}

const update = (id: number, f: (a: Alarm) => void) => save(load().map((a) => (a.id === id ? (f(a), a) : a)));

/** "7:30 AM" for "07:30". */
function clock(time: string): string {
  const [h, m] = time.split(":").map(Number);
  return new Date(2000, 0, 1, h, m).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
}

// ---------- sound: a bright repeating three-note pattern (Web Audio, no files) ----------
let audio: AudioContext | null = null;
function tone(): void {
  audio ??= new AudioContext();
  void audio.resume();
  const t0 = audio.currentTime;
  for (const [freq, dt] of [[988, 0], [1319, 0.18], [988, 0.36]]) {
    const osc = audio.createOscillator();
    const gain = audio.createGain();
    osc.type = "triangle";
    osc.frequency.value = freq;
    gain.gain.setValueAtTime(0.0001, t0 + dt);
    gain.gain.exponentialRampToValueAtTime(0.28, t0 + dt + 0.02);
    gain.gain.exponentialRampToValueAtTime(0.0001, t0 + dt + 0.16);
    osc.connect(gain).connect(audio.destination);
    osc.start(t0 + dt);
    osc.stop(t0 + dt + 0.18);
  }
}

// ---------- ringing ----------
function startRing(a: Alarm): void {
  ringing = a;
  ringStarted = Date.now();
  clearInterval(ringTimer);
  tone();
  ringTimer = window.setInterval(() => (Date.now() - ringStarted > RING_MAX_MS ? stop() : tone()), 1400);
  draw();
  hooks.ring(a);
}

function stop(): void {
  clearInterval(ringTimer);
  ringing = null;
  draw();
  hooks.stopped();
}

function snooze(): void {
  if (!ringing) return;
  const id = ringing.id;
  stop();
  update(id, (a) => {
    a.on = true; // a once-alarm was switched off when it rang: snoozing keeps it alive
    a.snoozeUntil = Date.now() + SNOOZE_MS;
  });
}

function tick(): void {
  if (ringing || !primary) return; // the second island stays quiet: one alarm, not two
  const now = Date.now();
  for (const a of load()) {
    const t = dueNow(a, now);
    if (t === null) continue;
    update(a.id, (x) => {
      x.last = t;
      x.snoozeUntil = null;
      if (!x.days.length) x.on = false; // once: off after it rings (Snooze switches it back on)
    });
    startRing(a);
    return;
  }
  draw(); // keeps "In 3 h 20 min" current
}

// ---------- UI ----------
function draw(): void {
  const panel = document.getElementById("al-panel");
  if (!panel) return;
  const ring = panel.querySelector<HTMLElement>(".al-ring")!;
  ring.hidden = !ringing;
  panel.classList.toggle("ringing", !!ringing);
  if (ringing) {
    ring.querySelector(".al-ring-time")!.textContent = clock(ringing.time);
    ring.querySelector(".al-ring-label")!.textContent = ringing.label || "Alarm";
  }
  const now = Date.now();
  const list = load().sort((a, b) => a.time.localeCompare(b.time));
  panel.querySelector(".al-list")!.replaceChildren(
    ...(list.length
      ? list.map((a) => {
          const r = el("div", `al-row${a.on ? "" : " off"}`);
          const text = el("button", "al-text");
          text.type = "button";
          text.title = "Edit";
          text.onclick = () => openForm(a);
          const next = nextRing(a, now);
          text.append(
            el("span", "al-time", clock(a.time)),
            el("span", "al-sub ell", [a.label, daysText(a.days), a.on && next !== null ? untilText(next, now) : ""].filter(Boolean).join(" · ")),
          );
          const sw = el("button", "bt-switch");
          sw.type = "button";
          sw.setAttribute("role", "switch");
          sw.setAttribute("aria-checked", String(a.on));
          sw.setAttribute("aria-label", `${clock(a.time)} alarm`);
          sw.onclick = () => update(a.id, (x) => ((x.on = !x.on), (x.snoozeUntil = null)));
          r.append(text, sw);
          return r;
        })
      : [el("div", "muted small al-empty", "No alarms yet. Press + Alarm to add one.")]),
  );
}

// ---------- time picker: our own (the browser's time input pops up a clunky native list) ----------
type TimePicker = { el: HTMLElement; get(): string; set(t: string): void; focus(): void };
let picker: TimePicker | null = null;

/** 12-hour picker: hour and minute fields (type digits, ↑ ↓, the mouse wheel, or the small arrows) and AM / PM. */
function timePicker(): TimePicker {
  let h12 = 7, min = 0, pm = false;
  const root = el("div", "tp");

  const field = (label: string, lo: number, hi: number, read: () => number, write: (n: number) => void, next?: () => void) => {
    const box = el("div", "tp-field");
    const input = el("input", "tp-in");
    Object.assign(input, { type: "text", inputMode: "numeric", maxLength: 2, autocomplete: "off" });
    input.setAttribute("aria-label", label);
    const draw = () => (input.value = String(read()).padStart(2, "0"));
    const bump = (d: number) => {
      const span = hi - lo + 1;
      write((((read() - lo + d) % span) + span) % span + lo);
      draw();
    };
    const arrow = (cls: string, name: string, d: number) => {
      const b = el("button", `tp-step ${cls}`);
      b.type = "button";
      b.tabIndex = -1;
      b.setAttribute("aria-label", `${label} ${d > 0 ? "up" : "down"}`);
      b.innerHTML = icon(name, 9); // static SVG
      b.onclick = () => bump(d);
      return b;
    };
    const commit = () => {
      const n = Number(input.value);
      if (input.value !== "" && n >= lo && n <= hi) write(n);
      draw();
    };
    input.onfocus = () => input.select();
    input.oninput = () => {
      input.value = input.value.replace(/\D/g, "").slice(0, 2);
      if (input.value.length === 2) {
        commit();
        next?.();
      }
    };
    input.onblur = commit;
    input.onwheel = (e) => (e.preventDefault(), bump(e.deltaY < 0 ? 1 : -1));
    input.onkeydown = (e) => {
      if (e.key === "ArrowUp" || e.key === "ArrowDown") (e.preventDefault(), bump(e.key === "ArrowUp" ? 1 : -1));
    };
    const arrows = el("div", "tp-steps");
    arrows.append(arrow("up", "chevronUp", 1), arrow("down", "chevronDown", -1));
    box.append(input, arrows);
    return { box, input, draw };
  };

  const minute = field("Minute", 0, 59, () => min, (n) => (min = n));
  const hour = field("Hour", 1, 12, () => h12, (n) => (h12 = n), () => minute.input.focus());
  const ap = el("div", "tp-ap");
  const half = (text: string, on: boolean) => {
    const b = el("button", "tp-half", text);
    b.type = "button";
    b.onclick = () => {
      pm = on;
      drawAp();
    };
    return b;
  };
  const am = half("AM", false), pmBtn = half("PM", true);
  const drawAp = () => {
    am.classList.toggle("on", !pm);
    pmBtn.classList.toggle("on", pm);
    am.setAttribute("aria-pressed", String(!pm));
    pmBtn.setAttribute("aria-pressed", String(pm));
  };
  ap.append(am, pmBtn);
  root.append(hour.box, el("span", "tp-colon", ":"), minute.box, ap);
  root.setAttribute("role", "group");
  root.setAttribute("aria-label", "Alarm time");

  return {
    el: root,
    get: () => `${String((h12 % 12) + (pm ? 12 : 0)).padStart(2, "0")}:${String(min).padStart(2, "0")}`,
    set: (t) => {
      const [h, m] = t.split(":").map(Number);
      pm = h >= 12;
      h12 = h % 12 || 12;
      min = m;
      hour.draw();
      minute.draw();
      drawAp();
    },
    focus: () => hour.input.focus(),
  };
}

function openForm(a: Alarm | null): void {
  editing = a;
  const form = document.getElementById("al-form") as HTMLFormElement;
  form.hidden = false;
  picker!.set(a?.time ?? "07:00");
  form.querySelector<HTMLInputElement>(".al-in-label")!.value = a?.label ?? "";
  form.querySelectorAll<HTMLButtonElement>(".al-day").forEach((b) => b.classList.toggle("on", !!a?.days.includes(Number(b.dataset.day))));
  form.querySelector<HTMLButtonElement>(".al-del")!.hidden = !a;
  picker!.focus();
}

function build(host: HTMLElement): void {
  const panel = el("div", "al-panel");
  panel.id = "al-panel";

  const head = el("div", "al-head");
  const add = el("button", "lq-add", "+ Alarm");
  add.type = "button";
  add.onclick = () => openForm(null);
  head.append(el("span", "al-title", "Alarms"), add);

  const form = el("form", "al-form");
  form.id = "al-form";
  form.hidden = true;
  picker = timePicker();
  const label = el("input", "al-in-label");
  Object.assign(label, { type: "text", name: "label", maxLength: 40, placeholder: "Label (optional)", autocomplete: "off" });
  label.setAttribute("aria-label", "Alarm label");
  const days = el("div", "al-days");
  days.setAttribute("aria-label", "Repeat on");
  for (const [d, letter] of DAYS) {
    const b = el("button", "al-day", letter);
    b.type = "button";
    b.dataset.day = String(d);
    b.title = ["Sundays", "Mondays", "Tuesdays", "Wednesdays", "Thursdays", "Fridays", "Saturdays"][d];
    b.onclick = () => b.classList.toggle("on");
    days.append(b);
  }
  const del = el("button", "btn al-del", "Delete");
  del.type = "button";
  del.onclick = () => {
    if (editing) save(load().filter((a) => a.id !== editing!.id));
    form.hidden = true;
  };
  const cancel = el("button", "btn", "Cancel");
  cancel.type = "button";
  cancel.onclick = () => (form.hidden = true);
  const ok = el("button", "btn primary", "Save");
  ok.type = "submit";
  const actions = el("div", "al-actions");
  actions.append(del, el("span", "spacer"), cancel, ok);
  form.append(el("div", "al-form-row"), label, days, actions);
  form.firstElementChild!.append(picker.el);
  form.onsubmit = (e) => {
    e.preventDefault();
    const picked = [...days.querySelectorAll<HTMLElement>(".al-day.on")].map((b) => Number(b.dataset.day));
    const fields = { time: picker!.get(), label: label.value.trim(), days: picked, on: true, snoozeUntil: null, last: undefined };
    const list = load();
    save(editing ? list.map((a) => (a.id === editing!.id ? { ...a, ...fields } : a)) : [...list, { id: Date.now(), ...fields }]);
    form.hidden = true;
  };

  const ring = el("div", "al-ring");
  ring.hidden = true;
  ring.setAttribute("role", "alert");
  const bell = el("span", "al-ring-ic");
  bell.innerHTML = icon("bell", 22); // static SVG
  const snoozeBtn = el("button", "btn al-snooze", "Snooze 5 min");
  snoozeBtn.type = "button";
  snoozeBtn.onclick = snooze;
  const stopBtn = el("button", "btn primary al-stop", "Stop");
  stopBtn.type = "button";
  stopBtn.onclick = stop;
  const words = el("div", "al-ring-text");
  words.append(el("span", "al-ring-time"), el("span", "al-ring-label ell"));
  ring.append(bell, words, snoozeBtn, stopBtn);

  panel.append(head, ring, form, el("div", "al-list"));
  host.append(panel);
}

export const alarmRinging = () => ringing !== null;

/** `ring`: an alarm went off (open the island on the Timer tab); `stopped`: it was answered. */
export function initAlarms(h: typeof hooks): void {
  hooks = h;
  const host = document.getElementById("tab-timer");
  if (!host) return;
  build(host);
  draw();
  window.addEventListener("storage", (e) => e.key === KEY && draw()); // the other island changed them
  window.addEventListener("keydown", (e) => ringing && e.key === "Escape" && stop());
  window.setInterval(tick, 1000);
}
