// Panels whose DOM is rebuilt from data: tasks, month progress, basics control balls.
// User/third-party text always goes through textContent, never innerHTML.
import { invoke } from "@tauri-apps/api/core";
import { icon } from "./icons";

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

export function renderTasks(tasks: Task[]): void {
  $("task-count").textContent = `${tasks.length} task${tasks.length === 1 ? "" : "s"}`;

  const list = $("task-list");
  list.replaceChildren(
    ...tasks.slice(0, 3).map((t) => {
      const row = el("div", "task-row");
      const bar = el("div", "task-bar");
      bar.style.background = t.color;
      const det = el("div", "task-det");
      det.append(el("div", "task-name", t.name), el("div", "task-cat", t.category));
      row.append(bar, det, el("div", "task-time", t.time));
      return row;
    }),
  );

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

// ---------- basics: control balls orbiting the small pill ----------
type Basic = { id: string; name: string; icon: string; confirm?: boolean };
const BASICS: Basic[] = [
  { id: "shutdown", name: "Shutdown", icon: "power", confirm: true },
  { id: "restart", name: "Restart", icon: "restart", confirm: true },
  { id: "sleep", name: "Sleep", icon: "sleep" },
  { id: "explorer", name: "File Explorer", icon: "folder" },
  { id: "settings", name: "Settings", icon: "cog" },
  { id: "taskmgr", name: "Task Manager", icon: "taskmgr" },
  { id: "chrome", name: "Chrome", icon: "chrome" },
  { id: "youtube", name: "YouTube", icon: "youtube" },
  { id: "cmd", name: "CMD", icon: "console" },
];

let basicsIndex = 0;
let armTimer = 0;
const balls: HTMLButtonElement[] = [];

export function initBalls(): void {
  for (let i = 0; i < 4; i++) {
    const b = document.createElement("button");
    b.className = "ball";
    b.onclick = () => onBallClick(b);
    $("balls").append(b);
    balls.push(b);
  }
}

function onBallClick(b: HTMLButtonElement): void {
  const item = BASICS.find((x) => x.id === b.dataset.id);
  if (!item) return;
  // Shutdown/restart need a second click within 3 s.
  if (item.confirm && !b.classList.contains("armed")) {
    disarm();
    b.classList.add("armed");
    b.title = `Click again to ${item.name}`;
    armTimer = window.setTimeout(disarm, 3000);
    return;
  }
  disarm();
  invoke("run_action", { id: item.id }).catch(console.error);
}

function disarm(): void {
  clearTimeout(armTimer);
  balls.forEach((b) => b.classList.remove("armed"));
}

export function scrollBalls(delta: number, pillTop: number): void {
  basicsIndex = (basicsIndex + delta + BASICS.length) % BASICS.length;
  layoutBalls(true, pillTop);
}

/** Show: fan the balls out around the right side of the 180x40 pill. Hide: tuck them behind it. */
export function layoutBalls(show: boolean, pillTop: number): void {
  const left = (window.innerWidth - 180) / 2;
  const right = left + 180;
  const cx = right - 22;
  const cy = pillTop + 20;
  const targets = [[cx + 42, cy - 24], [cx + 25, cy + 22], [cx - 18, cy + 44], [cx - 58, cy + 44]];
  const home = [right - 20, pillTop + 20];
  if (!show) disarm();

  balls.forEach((b, i) => {
    const item = BASICS[(basicsIndex + i - 1 + BASICS.length) % BASICS.length];
    b.innerHTML = icon(item.icon, 20);
    b.title = item.name;
    b.dataset.id = item.id;
    const visible = show && i <= 2;
    const [x, y] = show ? targets[i] : home;
    const scale = show ? (i === 1 ? 1.15 : 1) : 0.7;
    b.style.transitionDelay = show ? `${i * 45}ms` : "0ms";
    b.style.transform = `translate(${x}px, ${y}px) scale(${scale})`;
    b.style.opacity = visible ? "1" : "0";
    b.style.pointerEvents = visible ? "auto" : "none";
  });
}
