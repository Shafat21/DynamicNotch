// Island state machine + event wiring. Rust pushes events (perf, media, hover, ...);
// this file decides the island's size/state and which view is visible.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { initFocus } from "./focus";
import { hydrateIcons, icon } from "./icons";
import { createPal, greet, updateMood } from "./pal";
import { getSelectedTask, renderApps, renderMonth, renderTasks, type LauncherApp, type Task } from "./panels";

type Media = { state: "Playing" | "Paused" | "Idle"; title: string; artist: string; accent: string };
type Perf = { cpu: number; ram: number; disk: number; down: number; up: number };
type Weather = { city: string; temp: string; desc: string; icon: string; hourly: { time: string; temp: string; icon: string }[] };
type Settings = { location: string; lat: number; lon: number; islandStyle: string; animationStyle: string; autostart: boolean };
type State = "idle" | "hover" | "notify";
type View = "dashboard" | "perf" | "media" | "weather" | "calendar" | "month";

const PANELS = ["dashboard", "perf", "weather", "calendar", "month"] as const; // views with their own <section>; media uses the header
const ANIM_KEYS: Record<string, string> = { "Glow Sweep": "glow", "Fluid Blobs": "blobs", "Neon Border": "neon" };

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const shell = $("shell");
const island = $("island");
const view = $("view");

const ui = {
  state: "idle" as State,
  view: "dashboard" as View,
  hovering: false,
  charging: false,
  editingTasks: false,
  editingLocation: false,
  media: { state: "Idle", title: "", artist: "", accent: "#000000" } as Media,
  lyric: "",
  event: { title: "", text: "" },
  settings: { islandStyle: "Default", animationStyle: "Fluid Blobs" } as Settings,
  tasks: [] as Task[],
};

const isNotch = () => ui.settings.islandStyle === "Notch Nook";
const pillTop = () => (isNotch() ? 0 : 10);
const showingLyrics = () => ui.lyric !== "" && ui.media.state !== "Idle";
const playingTitle = () => (ui.media.state === "Playing" ? ui.media.title : "");
const calendarHeight = () => {
  const n = Math.min(ui.tasks.length, 3);
  return n ? 80 + n * 35 : 105;
};

let measureCtx: CanvasRenderingContext2D | null = null;
/** Idle pill: clock width, or wide enough for the playing song title (never lyrics). */
function idleWidth(): number {
  const title = playingTitle();
  if (!title) return 180;
  measureCtx ??= document.createElement("canvas").getContext("2d")!;
  measureCtx.font = getComputedStyle($("status-text")).font;
  // 2x25px padding + 22px icon + 10px gap
  return Math.min(420, Math.max(180, Math.ceil(measureCtx.measureText(title).width) + 82));
}

function targetSize(): [number, number] {
  if (ui.charging) return [850, 40];
  if (ui.state === "idle") return [idleWidth(), 40];
  if (ui.state === "notify") return [380, 40];
  switch (ui.view) {
    case "dashboard": return [720, 230];
    case "media": return [showingLyrics() ? 640 : 420, 40];
    case "perf": return [360, 180];
    case "weather": return [360, ui.editingLocation ? 205 : 160];
    case "calendar": return ui.editingTasks ? [540, 520] : [420, calendarHeight()];
    case "month": return [360, 150];
  }
}

// ---------- transitions ----------
let viewTimer = 0;
let revertTimer = 0;

function restartAnimation(node: HTMLElement, cls: string): void {
  node.classList.remove(cls);
  void node.offsetWidth; // force reflow so the animation starts again
  node.classList.add(cls);
}

/** Morph to the size of the current state, swap content halfway through the fade. */
function transition(): void {
  const [w, h] = targetSize();
  const r = Math.min(h / 2, isNotch() ? 18 : 30);
  shell.style.setProperty("--w", `${w}px`);
  shell.style.setProperty("--h", `${h}px`);
  shell.style.setProperty("--r", `${r}px`);
  shell.style.setProperty("--scoop", `${Math.min(r * 1.4, 25)}px`);
  shell.dataset.style = ui.settings.islandStyle;

  island.dataset.bg = ui.state === "hover" ? ui.view : "";
  updateFx();
  if (!ui.charging) restartAnimation($("island").querySelector<HTMLElement>(".shine")!, "run");

  view.style.setProperty("--slide", `${w / 4}px`);
  restartAnimation(view, "swap");
  clearTimeout(viewTimer);
  viewTimer = window.setTimeout(renderView, 250);

  sendHitRect(w, h);
}

/** Re-morph only when the size actually changes; media/lyric events arrive often. */
function refresh(): void {
  const [w, h] = targetSize();
  if (shell.style.getPropertyValue("--w") !== `${w}px` || shell.style.getPropertyValue("--h") !== `${h}px`) transition();
  else renderHeader();
}

function updateFx(): void {
  const on = ui.state === "hover" ? ui.view === "media" : playingTitle() !== "";
  island.dataset.anim = on ? ANIM_KEYS[ui.settings.animationStyle] ?? "blobs" : "";
  island.style.setProperty("--accent", ui.media.accent);
}

/** Tell Rust where the island is so it can toggle click-through. */
function sendHitRect(w: number, h: number): void {
  const left = (window.innerWidth - w) / 2;
  const rect = { x: left - 15, y: 0, w: w + 30, h: pillTop() + h + 30 };
  invoke("set_hit_rect", { rect }).catch(console.error);
}

function setState(s: State): void {
  if (s === "idle") ui.view = "dashboard"; // the next hover opens home
  if (ui.state === s) return;
  ui.state = s;
  if (s === "hover" && ui.view === "dashboard") greet();
  transition();
}

function showEvent(title: string, text: string, ms: number): void {
  ui.event = { title, text };
  clearTimeout(revertTimer);
  if (ui.state === "notify") transition();
  else setState("notify");
  revertTimer = window.setTimeout(() => setState(ui.hovering ? "hover" : "idle"), ms);
}

// ---------- rendering ----------
/** Header icon + text; no icon name = Pal (idle pill). */
function setStatus(iconName: string | null, text: string): void {
  $("status-pal").hidden = iconName !== null;
  $("status-icon").hidden = iconName === null;
  if (iconName) $("status-icon").innerHTML = icon(iconName, 18);
  $("status-text").textContent = text;
}

function renderHeader(): void {
  if (ui.state === "notify") {
    setStatus(ui.event.title.includes("Lock") ? "bolt" : "email", `${ui.event.title} - ${ui.event.text}`);
    return;
  }
  const m = ui.media;
  if (ui.state === "hover" && ui.view === "media") {
    let text = "Music Player";
    if (showingLyrics()) text = ui.lyric;
    else if (m.state !== "Idle") text = m.artist ? `${m.title} - ${m.artist}` : m.title;
    setStatus("music", text);
    return;
  }
  if (ui.state === "idle" && playingTitle()) {
    setStatus(null, playingTitle());
    return;
  }
  const now = new Date();
  const day = now.toLocaleDateString("en-US", { weekday: "short" });
  const time = now.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
  setStatus(null, `${day}, ${time}`);
}

function renderClock(now = new Date()): void {
  const days = new Date(now.getFullYear(), now.getMonth() + 1, 0).getDate();
  $("clock-time").textContent = now.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
  $("clock-date").textContent = now.toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "short" });
  $("clock-bar").style.width = `${(now.getDate() / days) * 100}%`;
}

function renderView(): void {
  const hover = ui.state === "hover";
  const v = ui.view;
  $("header").hidden = hover && v !== "media";
  $("media-controls").hidden = !(hover && v === "media");
  for (const p of PANELS) $(`panel-${p}`).hidden = !(hover && v === p);
  $("task-view").hidden = ui.editingTasks;
  $("task-edit").hidden = !ui.editingTasks;
  $("location-form").hidden = !ui.editingLocation;
  if (hover && v === "dashboard") renderClock();
  if (hover && v === "calendar") renderTasks(ui.tasks);
  if (hover && v === "month") renderMonth();
  renderHeader();
}

function openView(v: View): void {
  ui.view = v;
  ui.editingTasks = ui.editingLocation = false;
  if (ui.state === "hover") transition();
}

// ---------- backend events ----------
function onPerf(p: Perf): void {
  const set = (k: string, v: number) => {
    $(`${k}-val`).textContent = `${Math.round(v)}%`;
    $(`${k}-bar`).style.width = `${Math.min(100, v)}%`;
  };
  set("cpu", p.cpu);
  set("ram", p.ram);
  set("disk", p.disk);
  $("chip-cpu-val").textContent = `CPU ${Math.round(p.cpu)}%`;
  const fmt = (b: number) => (b < 1024 ? `${Math.round(b)} B/s` : b < 1048576 ? `${Math.round(b / 1024)} KB/s` : `${(b / 1048576).toFixed(1)} MB/s`);
  $("net-down").textContent = `↓ ${fmt(p.down)}`;
  $("net-up").textContent = `↑ ${fmt(p.up)}`;
}

function onMedia(m: Media): void {
  ui.media = m;
  if (m.state === "Idle") ui.lyric = "";
  $("btn-play").innerHTML = icon(m.state === "Playing" ? "pause" : "play");
  $("tile-media-title").textContent = m.state === "Idle" ? "Nothing playing" : m.title;
  $("tile-media-artist").textContent = m.state === "Idle" ? "" : m.artist;
  $("tile-media").style.setProperty("--accent", m.accent);
  updateFx();
  refresh(); // idle pill width follows the song title
}

function onLyric(text: string): void {
  ui.lyric = text;
  refresh(); // the Music view widens to 640 while lyrics show
}

function onWeather(w: Weather): void {
  $("weather-temp").textContent = w.temp;
  $("weather-city").textContent = w.city;
  $("weather-cond").textContent = w.desc;
  $("chip-weather-val").textContent = w.temp;
  $("chip-weather-icon").innerHTML = icon(w.icon, 14);
  $("hourly").replaceChildren(
    ...w.hourly.map((h) => {
      const slot = document.createElement("div");
      slot.className = "slot";
      const t = document.createElement("span");
      t.className = "t";
      t.textContent = h.time;
      const i = document.createElement("span");
      i.innerHTML = icon(h.icon, 18);
      const v = document.createElement("span");
      v.className = "v";
      v.textContent = h.temp;
      slot.append(t, i, v);
      return slot;
    }),
  );
}

function onSettings(s: Settings): void {
  ui.settings = s;
  transition();
}

function onCharging(): void {
  ui.charging = true;
  restartAnimation(shell, "charging");
  transition();
  window.setTimeout(() => {
    ui.charging = false;
    shell.classList.remove("charging");
    transition();
  }, 3100);
}

function onHover(inside: boolean): void {
  ui.hovering = inside;
  if (ui.state === "notify") return;
  clearTimeout(revertTimer);
  if (inside) setState("hover");
  else if (ui.state === "hover") revertTimer = window.setTimeout(() => setState("idle"), 1200);
}

function openLocationEditor(): void {
  ui.view = "weather";
  ui.editingLocation = true;
  clearTimeout(revertTimer);
  if (ui.state === "hover") transition();
  else setState("hover");
  // Opened from the menu while the cursor is elsewhere: fall back to idle if never reached.
  if (!ui.hovering) revertTimer = window.setTimeout(() => setState("idle"), 6000);
  window.setTimeout(() => $<HTMLInputElement>("location-input").focus(), 300);
}

// ---------- launcher ----------
const launcherError = (err: unknown) => showEvent("Launcher", String(err), 3500);

/** Render the app list a launcher command returns. */
function updateApps(apps: Promise<LauncherApp[]>): void {
  apps
    .then((list) =>
      renderApps(
        list,
        (a) => invoke("launch_app", { id: a.id }).catch(launcherError),
        (a) => updateApps(invoke<LauncherApp[]>("remove_app", { id: a.id })),
      ),
    )
    .catch(launcherError);
}

// ---------- user input ----------
function wireInput(): void {
  window.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    invoke("show_context_menu").catch(console.error);
  });

  // Pal listens while the user types in any island input.
  island.addEventListener("focusin", (e) => updateMood({ listening: e.target instanceof HTMLInputElement }));
  island.addEventListener("focusout", () => updateMood({ listening: false }));

  // dashboard navigation
  document.querySelectorAll<HTMLElement>("[data-view]").forEach((b) => {
    b.onclick = () => openView(b.dataset.view as View);
  });
  document.querySelectorAll<HTMLElement>("[data-menu]").forEach((b) => {
    b.onclick = () => invoke("show_context_menu").catch(console.error);
  });
  $("btn-collapse").onclick = () => setState("idle");
  $("btn-add-app").onclick = () => updateApps(invoke<LauncherApp[]>("pick_app"));
  initFocus($("tile-focus"), () => showEvent("Focus", "Time's up", 3500));

  document.querySelectorAll<HTMLElement>("[data-action]").forEach((b) => {
    b.onclick = () => invoke("run_action", { id: b.dataset.action }).catch(console.error);
  });
  document.querySelectorAll<HTMLElement>("[data-media]").forEach((b) => {
    b.onclick = () => invoke("media_control", { action: b.dataset.media }).catch(console.error);
  });

  // tasks
  const toggleEditor = () => {
    ui.editingTasks = !ui.editingTasks;
    transition();
  };
  $("btn-edit-tasks").onclick = toggleEditor;
  $("task-done").onclick = toggleEditor;
  $("task-form").onsubmit = async (e) => {
    e.preventDefault();
    const name = $<HTMLInputElement>("task-name");
    const time = $<HTMLInputElement>("task-time");
    if (!name.value.trim()) return name.focus();
    try {
      ui.tasks = await invoke<Task[]>("add_task", { name: name.value, time: time.value, category: $<HTMLSelectElement>("task-cat").value });
      name.value = time.value = "";
      renderTasks(ui.tasks);
    } catch (err) {
      showEvent("Tasks", String(err), 3500);
    }
  };
  $("task-delete").onclick = async () => {
    const id = getSelectedTask();
    if (id === null) return;
    ui.tasks = await invoke<Task[]>("delete_task", { id });
    renderTasks(ui.tasks);
  };

  // location
  $("btn-edit-location").onclick = openLocationEditor;
  $("location-cancel").onclick = () => {
    ui.editingLocation = false;
    transition();
  };
  $("location-form").onsubmit = async (e) => {
    e.preventDefault();
    const input = $<HTMLInputElement>("location-input");
    const city = input.value.trim();
    if (!city) return input.focus();
    ui.editingLocation = false;
    try {
      const name = await invoke<string>("set_location", { city });
      input.value = "";
      showEvent("Weather", `Location Updated: Now showing weather for ${name}`, 3500);
    } catch (err) {
      showEvent("Weather", `Error: ${err}`, 3500);
    }
  };
}

// ---------- boot ----------
async function boot(): Promise<void> {
  hydrateIcons();
  $("status-pal").append(createPal(22).el);
  $("assistant-avatar").append(createPal(56).el); // napping until Task 9 connects Claude Code
  wireInput();
  updateApps(invoke<LauncherApp[]>("list_apps"));

  await Promise.all([
    listen<boolean>("hover", (e) => onHover(e.payload)),
    listen<Perf>("perf", (e) => onPerf(e.payload)),
    listen<Media>("media", (e) => onMedia(e.payload)),
    listen<string>("lyric", (e) => onLyric(e.payload)),
    listen<Weather>("weather", (e) => onWeather(e.payload)),
    listen<string>("accent", (e) => document.documentElement.style.setProperty("--sys-accent", e.payload)),
    listen<Settings>("settings", (e) => onSettings(e.payload)),
    listen<{ name: string; on: boolean }>("keylock", (e) => showEvent(e.payload.name, e.payload.on ? "ENABLED" : "DISABLED", 2500)),
    listen<{ app: string; title: string; text: string }>("notify", (e) => {
      const { app, title, text } = e.payload;
      showEvent(app, title ? `${title}: ${text}` : text, 3500);
    }),
    listen("charging", onCharging),
    listen("edit-location", openLocationEditor),
  ]);

  // Catch up on anything the monitors published before this page loaded.
  const snap = await invoke<Record<string, unknown>>("get_snapshot");
  if (snap.accent) document.documentElement.style.setProperty("--sys-accent", snap.accent as string);
  if (snap.perf) onPerf(snap.perf as Perf);
  if (snap.weather) onWeather(snap.weather as Weather);
  if (snap.media) onMedia(snap.media as Media);
  if (snap.lyric) ui.lyric = snap.lyric as string;

  ui.tasks = await invoke<Task[]>("list_tasks");
  onSettings(await invoke<Settings>("get_settings"));

  window.setInterval(() => {
    if (ui.state === "idle") renderHeader();
    else if (ui.state === "hover" && ui.view === "dashboard") renderClock();
  }, 1000);
}

boot().catch(console.error);
