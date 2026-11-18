// Panels whose DOM is rebuilt from data: tasks, month progress, launcher row.
// User/third-party text always goes through textContent, never innerHTML.
import { daysUntil, spans, untilLabel } from "./progress-math";
import { icon } from "./icons-line";

export type Task = { id: number; name: string; category: string; time: string; color: string };

const $ = (id: string) => document.getElementById(id)!;

function el(tag: string, cls: string, text?: string): HTMLElement {
  const e = document.createElement(tag);
  e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

// ---------- tasks ----------
/** Same order and colours as db.rs (CATEGORIES / category_color); the chip cycles through them. */
export const TASK_CATS: [name: string, color: string][] = [
  ["Work", "#00A0FF"], ["Health", "#00FF80"], ["Personal", "#FF5050"], ["Study", "#FFB000"], ["Other", "#A070FF"],
];
const UNDO_MS = 1100; // a crossed-off task can be un-crossed until it goes
let onDone: (id: number) => void = () => {};
const pending = new Map<number, number>(); // task id → removal timer

/** `check`: Tasks tab rows get the cross-off button; Home card rows are read-only. */
function taskRow(t: Task, check: boolean): HTMLElement {
  const row = el("div", "task-row");
  row.style.setProperty("--cat", t.color);
  const det = el("div", "task-det");
  // narrow Home card: the time shares the category line so the name keeps the width
  det.append(el("div", "task-name", t.name), el("div", "task-cat", !check && t.time ? `${t.category} · ${t.time}` : t.category));
  if (check) {
    const b = el("button", "task-check") as HTMLButtonElement;
    b.type = "button";
    const sync = () => {
      const on = pending.has(t.id);
      row.classList.toggle("done", on);
      b.ariaPressed = String(on);
      b.title = on ? "Undo" : "Done: cross it off";
      b.setAttribute("aria-label", on ? `Undo: keep ${t.name}` : `Done: ${t.name}`);
    };
    b.onclick = () => {
      if (pending.has(t.id)) {
        clearTimeout(pending.get(t.id));
        pending.delete(t.id);
      } else {
        pending.set(t.id, window.setTimeout(() => (pending.delete(t.id), onDone(t.id)), UNDO_MS));
      }
      sync();
    };
    b.innerHTML = icon("check", 12); // static SVG from our icon set
    row.append(b);
    sync();
  } else {
    row.append(el("div", "task-bar"));
  }
  row.append(det);
  if (check && t.time) row.append(el("div", "task-time", t.time));
  return row;
}

/** A Tasks-tab row (cross-off circle + undo) for elsewhere: the compact Todo card. */
export const taskRowFor = (t: Task): HTMLElement => taskRow(t, true);

/** `done(id)` removes a task once its cross-off can no longer be undone. */
export function initTasks(done: (id: number) => void): void {
  onDone = done;
  const chip = $("task-cat");
  const set = (i: number) => {
    const [name, color] = TASK_CATS[i];
    chip.dataset.cat = name;
    chip.style.setProperty("--cat", color);
    chip.querySelector("span")!.textContent = name;
    chip.title = `Category: ${name} (click to change)`;
    chip.setAttribute("aria-label", chip.title);
  };
  chip.onclick = () => set((TASK_CATS.findIndex(([n]) => n === chip.dataset.cat) + 1) % TASK_CATS.length);
  set(0);
}

export function renderTasks(tasks: Task[]): void {
  $("task-count").textContent = `${tasks.length} task${tasks.length === 1 ? "" : "s"}`;
  $("task-list").replaceChildren(
    ...(tasks.length ? tasks.map((t) => taskRow(t, true)) : [el("div", "task-empty muted", "Nothing next. Add a task above.")]),
  );
  // Home card: the next two
  $("card-task-list").replaceChildren(...(tasks.length ? tasks.slice(0, 2).map((t) => taskRow(t, false)) : [el("div", "muted", "No tasks")]));
}

// ---------- month progress (rebuilt on every show, so it never goes stale) ----------
// ---------- Progress tab (id "month"): time spans + countdowns ----------
// shortcut: countdowns live in this webview's localStorage (one PC, both islands share it); move to the DB if
// they ever need to sync or survive a WebView2 profile reset.
type Countdown = { id: number; name: string; date: string };
const CD_KEY = "dn.countdowns";
const loadCountdowns = (): Countdown[] => {
  try {
    const v = JSON.parse(localStorage.getItem(CD_KEY) ?? "[]");
    return Array.isArray(v) ? v.filter((c) => c && typeof c.name === "string" && typeof c.date === "string") : [];
  } catch {
    return [];
  }
};
const saveCountdowns = (list: Countdown[]) => localStorage.setItem(CD_KEY, JSON.stringify(list.slice(0, 20)));
let progressWired = false;

function wireProgress(): void {
  if (progressWired) return;
  progressWired = true;
  const form = $("pg-form") as HTMLFormElement;
  const name = $("pg-name") as HTMLInputElement, date = $("pg-date") as HTMLInputElement;
  $("pg-add").onclick = () => {
    form.hidden = !form.hidden;
    if (!form.hidden) name.focus();
  };
  form.onsubmit = (e) => {
    e.preventDefault();
    const n = name.value.trim();
    if (!n) return name.focus();
    if (Number.isNaN(daysUntil(date.value))) return date.focus();
    saveCountdowns([...loadCountdowns(), { id: Date.now(), name: n, date: date.value }]);
    name.value = date.value = "";
    form.hidden = true;
    renderMonth();
  };
}

export function renderMonth(now = new Date()): void {
  wireProgress();
  $("pg-spans").replaceChildren(
    ...spans(now).map((sp) => {
      const r = el("div", "pg-row");
      const top = el("div", "pg-top");
      top.append(el("span", "pg-label", sp.label), el("span", "pg-pct", `${Math.floor(sp.frac * 100)}%`), el("span", "pg-left", sp.left));
      const bar = el("div", "pg-bar");
      const fill = el("i", "");
      fill.style.width = `${(sp.frac * 100).toFixed(1)}%`;
      bar.append(fill);
      r.append(top, bar);
      return r;
    }),
  );
  const list = loadCountdowns()
    .map((c) => ({ ...c, n: daysUntil(c.date, now) }))
    .filter((c) => !Number.isNaN(c.n) && c.n >= -1) // a day after, it goes away by itself
    .sort((a, b) => a.n - b.n);
  $("pg-list").replaceChildren(
    ...(list.length
      ? list.map((c) => {
          const r = el("div", `pg-cd${c.n <= 0 ? " now" : c.n <= 7 ? " soon" : ""}`);
          r.title = new Date(`${c.date}T00:00`).toLocaleDateString("en-US", { weekday: "long", day: "numeric", month: "long", year: "numeric" });
          const days = el("span", "pg-days", c.n > 1 ? String(c.n) : c.n === 1 ? "1" : "★");
          const text = el("span", "pg-cd-text");
          text.append(el("span", "pg-cd-name ell", c.name), el("span", "pg-cd-when", untilLabel(c.n)));
          const rm = el("button", "pg-rm", "×") as HTMLButtonElement;
          rm.type = "button";
          rm.title = `Remove ${c.name}`;
          rm.onclick = () => (saveCountdowns(loadCountdowns().filter((x) => x.id !== c.id)), renderMonth());
          r.append(days, text, rm);
          return r;
        })
      : [el("div", "muted small pg-empty", "Add a date you're counting down to: a deadline, a trip, an exam.")]),
  );
}

// ---------- launcher (Home strip + Launcher tab) ----------
export type LauncherApp = { id: number; name: string; kind: "path" | "action"; target: string; icon: string | null };

// Built-in actions have no file icon; reuse the old Basics glyphs.
const ACTION_ICONS: Record<string, string> = {
  shutdown: "power", restart: "restart", sleep: "sleep", explorer: "folder", settings: "cog",
  taskmgr: "taskmgr", chrome: "chrome", youtube: "youtube", cmd: "console",
};
// Tint of each built-in action's badge (Launcher tab) and glyph (Home strip).
const ACTION_COLORS: Record<string, string> = {
  shutdown: "#ff6b6b", restart: "#ffa94d", sleep: "#9775fa", explorer: "#ffd43b", settings: "#74c0fc",
  taskmgr: "#69db7c", chrome: "#4dabf7", youtube: "#ff6b6b", cmd: "#ced4da",
};
const CONFIRM = new Set(["shutdown", "restart"]);
let armTimer = 0;

/** A steady colour per name for apps shown as a letter. */
const hue = (name: string) => [...name].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 7);

function disarm(): void {
  clearTimeout(armTimer);
  document.querySelectorAll<HTMLElement>(".launcher .armed").forEach((b) => {
    b.classList.remove("armed", "launch", "remove");
    b.title = b.dataset.name!;
    b.querySelector(".app-name")!.textContent = b.dataset.name!;
  });
}

/** Two-step confirm: the first call arms `b` for 3 s and returns false; a second call of the same kind returns true.
 *  `short` replaces the tile's label meanwhile (Launcher tab), `hint` is the tooltip. */
function confirmed(b: HTMLElement, kind: "launch" | "remove", hint: string, short: string): boolean {
  if (b.classList.contains(kind)) {
    disarm();
    return true;
  }
  disarm();
  b.classList.add("armed", kind);
  b.title = hint;
  b.querySelector(".app-name")!.textContent = short;
  armTimer = window.setTimeout(disarm, 3000);
  return false;
}

export function renderApps(apps: LauncherApp[], onLaunch: (a: LauncherApp) => void, onRemove: (a: LauncherApp) => void): void {
  clearTimeout(armTimer);
  $("launcher").replaceChildren(...apps.map((a) => appButton(a, onLaunch, onRemove)));
  $("launcher-grid").replaceChildren(...apps.map((a) => appButton(a, onLaunch, onRemove)), $("launcher-add")); // Add tile last
  fitDock();
}

/** Home dock: tiles that wrap past row 1 get .off and are counted on "+N" (opens the Launcher tab). */
export function fitDock(): void {
  const dock = $("launcher"), more = $("launcher-more");
  if (!dock.offsetParent) return; // Home/launcher hidden: every offsetTop is 0; the observer refits on show
  const apps = [...dock.children] as HTMLElement[];
  dock.style.width = ""; // measure at the flexible width; trimmed to the visible tiles below
  const wrapped = () => apps.filter((b) => b.offsetTop > apps[0].offsetTop);
  more.hidden = true; // measure with the most room first
  let off = wrapped();
  if (off.length) {
    more.hidden = false; // "+N" takes a slot: re-measure
    off = wrapped();
  }
  apps.forEach((b) => b.classList.toggle("off", off.includes(b)));
  // a wrapped flex row keeps its unwrapped width: trim it so the divider sits right after the last visible tile
  const last = apps.filter((b) => !off.includes(b)).pop();
  if (off.length && last) dock.style.width = `${last.offsetLeft + last.offsetWidth - dock.offsetLeft}px`;
  more.textContent = `+${off.length}`;
  more.title = `All apps (${off.length} more)`;
  more.setAttribute("aria-label", more.title);
}

function appButton(a: LauncherApp, onLaunch: (a: LauncherApp) => void, onRemove: (a: LauncherApp) => void): HTMLElement {
  // .app-ic: the icon (the whole tile on the Home strip, a tinted badge in the Launcher tab); .app-name: tab only
  const b = el("button", "app");
  const ic = el("span", "app-ic");
  b.title = b.dataset.name = a.name;
  b.setAttribute("aria-label", a.name);
  if (a.icon) {
    const img = document.createElement("img");
    img.src = a.icon;
    img.alt = "";
    ic.append(img);
    b.classList.add("has-img");
  } else if (a.kind === "action" && ACTION_ICONS[a.target]) {
    ic.innerHTML = icon(ACTION_ICONS[a.target], 20); // static SVG markup from our icon set
    b.style.setProperty("--app-c", ACTION_COLORS[a.target] ?? "var(--text)");
  } else {
    ic.textContent = [...a.name][0]?.toUpperCase() ?? "?";
    b.style.setProperty("--app-c", `hsl(${hue(a.name)} 70% 68%)`);
  }
  b.append(ic, el("span", "app-name", a.name));
  b.onclick = () => {
    if (a.kind === "action" && CONFIRM.has(a.target) && !confirmed(b, "launch", `Click again to ${a.name}`, "Click again")) return;
    disarm();
    onLaunch(a);
  };
  b.oncontextmenu = (e) => {
    e.preventDefault();
    e.stopPropagation(); // keep the island menu closed
    if (confirmed(b, "remove", "Right-click again to remove", "Remove?")) onRemove(a);
  };
  return b;
}
