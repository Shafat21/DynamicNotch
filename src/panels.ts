// Panels whose DOM is rebuilt from data: tasks, month progress, launcher row.
// User/third-party text always goes through textContent, never innerHTML.
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
let selectedTask: number | null = null;
export const getSelectedTask = () => selectedTask;

function taskRow(t: Task): HTMLElement {
  const row = el("div", "task-row");
  const bar = el("div", "task-bar");
  bar.style.background = t.color;
  const det = el("div", "task-det");
  det.append(el("div", "task-name", t.name), el("div", "task-cat", t.category));
  row.append(bar, det, el("div", "task-time", t.time));
  return row;
}

export function renderTasks(tasks: Task[]): void {
  $("task-count").textContent = `${tasks.length} task${tasks.length === 1 ? "" : "s"}`;
  $("task-list").replaceChildren(...tasks.slice(0, 3).map(taskRow));
  // Home card: the next two
  $("card-task-list").replaceChildren(...(tasks.length ? tasks.slice(0, 2).map(taskRow) : [el("div", "muted", "No tasks")]));

  if (!tasks.some((t) => t.id === selectedTask)) selectedTask = null;
  const edit = $("edit-list");
  edit.replaceChildren(
    ...tasks.map((t) => {
      const li = el("li", t.id === selectedTask ? "sel" : "", `${t.name} • ${t.time}`);
      li.append(el("small", "", t.category.toUpperCase()));
      li.onclick = () => {
        selectedTask = selectedTask === t.id ? null : t.id;
        renderTasks(tasks);
      };
      return li;
    }),
  );
}

// ---------- month progress (rebuilt on every show, so it never goes stale) ----------
export function renderMonth(now = new Date()): void {
  const days = new Date(now.getFullYear(), now.getMonth() + 1, 0).getDate();
  $("month-title").textContent = `${now.toLocaleDateString("en-US", { month: "long" })} Progress`;
  $("month-pct").textContent = `${Math.floor((now.getDate() / days) * 100)}%`;
  $("month-dots").replaceChildren(
    ...Array.from({ length: days }, (_, i) => el("div", i < now.getDate() ? "dot on" : "dot")),
  );
}

// ---------- launcher (Home strip + Launcher tab) ----------
export type LauncherApp = { id: number; name: string; kind: "path" | "action"; target: string; icon: string | null };

// Built-in actions have no file icon; reuse the old Basics glyphs.
const ACTION_ICONS: Record<string, string> = {
  shutdown: "power", restart: "restart", sleep: "sleep", explorer: "folder", settings: "cog",
  taskmgr: "taskmgr", chrome: "chrome", youtube: "youtube", cmd: "console",
};
const CONFIRM = new Set(["shutdown", "restart"]);
let armTimer = 0;

function disarm(): void {
  clearTimeout(armTimer);
  document.querySelectorAll<HTMLElement>(".launcher .armed").forEach((b) => {
    b.classList.remove("armed", "launch", "remove");
    b.title = b.dataset.name!;
  });
}

/** Two-step confirm: the first call arms `b` for 3 s and returns false; a second call of the same kind returns true. */
function confirmed(b: HTMLElement, kind: "launch" | "remove", hint: string): boolean {
  if (b.classList.contains(kind)) {
    disarm();
    return true;
  }
  disarm();
  b.classList.add("armed", kind);
  b.title = hint;
  armTimer = window.setTimeout(disarm, 3000);
  return false;
}

export function renderApps(apps: LauncherApp[], onLaunch: (a: LauncherApp) => void, onRemove: (a: LauncherApp) => void): void {
  clearTimeout(armTimer);
  for (const id of ["launcher", "launcher-grid"]) $(id).replaceChildren(...apps.map((a) => appButton(a, onLaunch, onRemove)));
}

function appButton(a: LauncherApp, onLaunch: (a: LauncherApp) => void, onRemove: (a: LauncherApp) => void): HTMLElement {
  const b = el("button", "app");
  b.title = b.dataset.name = a.name;
  b.setAttribute("aria-label", a.name);
  if (a.icon) {
    const img = document.createElement("img");
    img.src = a.icon;
    img.alt = "";
    b.append(img);
  } else if (a.kind === "action" && ACTION_ICONS[a.target]) {
    b.innerHTML = icon(ACTION_ICONS[a.target], 20);
  } else {
    b.textContent = [...a.name][0]?.toUpperCase() ?? "?";
  }
  b.onclick = () => {
    if (a.kind === "action" && CONFIRM.has(a.target) && !confirmed(b, "launch", `Click again to ${a.name}`)) return;
    disarm();
    onLaunch(a);
  };
  b.oncontextmenu = (e) => {
    e.preventDefault();
    e.stopPropagation(); // keep the island menu closed
    if (confirmed(b, "remove", "Right-click again to remove")) onRemove(a);
  };
  return b;
}
