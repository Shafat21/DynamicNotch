// Island state machine + event wiring. Rust pushes events (perf, media, hover, ...);
// this file decides the island's size/state and which panel is visible.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { hydrateIcons, icon } from "./icons";
import { getSelectedTask, initBalls, layoutBalls, renderMonth, renderTasks, scrollBalls, type Task } from "./panels";

type Media = { state: "Playing" | "Paused" | "Idle"; title: string; artist: string; accent: string };
type Perf = { cpu: number; ram: number; disk: number; down: number; up: number };
type Weather = { city: string; temp: string; desc: string; icon: string; hourly: { time: string; temp: string; icon: string }[] };
type Settings = { location: string; lat: number; lon: number; islandStyle: string; animationStyle: string; autostart: boolean };
type State = "idle" | "hover" | "notify";

const FEATURES = ["perf", "media", "weather", "calendar", "month", "basics"] as const;
type Feature = (typeof FEATURES)[number];
const ANIM_KEYS: Record<string, string> = { "Glow Sweep": "glow", "Fluid Blobs": "blobs", "Neon Border": "neon" };

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const shell = $("shell");
const island = $("island");
const view = $("view");

const ui = {
  state: "idle" as State,
  feat: 0,
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

const feature = (): Feature => FEATURES[ui.feat];
const isNotch = () => ui.settings.islandStyle === "Notch Nook";
const pillTop = () => (isNotch() ? 0 : 10);
const showingLyrics = () => ui.lyric !== "" && ui.media.state !== "Idle";
const calendarHeight = () => {
  const n = Math.min(ui.tasks.length, 3);
  return n ? 80 + n * 35 : 105;
};

function targetSize(): [number, number] {
  if (ui.charging) return [850, 40];
  if (ui.state === "idle") return feature() === "media" && showingLyrics() ? [640, 40] : [180, 40];
  if (ui.state === "notify") return [380, 40];
  switch (feature()) {
    case "media": return [showingLyrics() ? 640 : 420, 40];
    case "perf": return [360, 180];
    case "weather": return [360, ui.editingLocation ? 205 : 160];
    case "calendar": return ui.editingTasks ? [540, 520] : [420, calendarHeight()];
    case "month": return [360, 150];
    case "basics": return [180, 40];
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

  island.dataset.bg = ui.state === "hover" ? feature() : "";
  updateFx();
  if (!ui.charging) restartAnimation($("island").querySelector<HTMLElement>(".shine")!, "run");

  view.style.setProperty("--slide", `${w / 4}px`);
  restartAnimation(view, "swap");
  clearTimeout(viewTimer);
  viewTimer = window.setTimeout(renderView, 250);

  const basics = ui.state === "hover" && feature() === "basics";
  layoutBalls(basics, pillTop());
  sendHitRect(w, h, basics);
}

function updateFx(): void {
  const on = feature() === "media" && (ui.state !== "idle" || ui.media.state !== "Idle");
  island.dataset.anim = on ? ANIM_KEYS[ui.settings.animationStyle] ?? "blobs" : "";
  island.style.setProperty("--accent", ui.media.accent);
}

/** Tell Rust where the island is so it can toggle click-through. Extra room on the right/bottom for the balls. */
function sendHitRect(w: number, h: number, basics: boolean): void {
  const left = (window.innerWidth - w) / 2;
  const rect = { x: left - 15, y: 0, w: w + 15 + (basics ? 80 : 15), h: pillTop() + h + (basics ? 90 : 30) };
  invoke("set_hit_rect", { rect }).catch(console.error);
}

function setState(s: State): void {
  if (ui.state === s) return;
  ui.state = s;
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
function setStatus(iconName: string, text: string, color = "#fff"): void {
  $("status-icon").innerHTML = icon(iconName, 18, color);
  $("status-text").textContent = text;
}

function renderHeader(): void {
  if (ui.state === "notify") {
    setStatus(ui.event.title.includes("Lock") ? "bolt" : "email", `${ui.event.title} - ${ui.event.text}`);
    return;
  }
  const m = ui.media;
  if (feature() === "media" && (ui.state === "hover" || m.state !== "Idle")) {
    let text = "Music Player";
    if (showingLyrics()) text = ui.lyric;
    else if (m.state !== "Idle") text = ui.state === "hover" && m.artist ? `${m.title} - ${m.artist}` : m.title;
    setStatus("music", text);
    return;
  }
  const now = new Date();
  const day = now.toLocaleDateString("en-US", { weekday: "short" });
  const time = now.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
  setStatus("circle", `${day}, ${time}`, getComputedStyle(document.documentElement).getPropertyValue("--sys-accent").trim() || "#0078d7");
}

function renderView(): void {
  const hover = ui.state === "hover";
  const f = feature();
  $("header").hidden = hover && f !== "media";
  $("media-controls").hidden = !(hover && f === "media");
  for (const p of FEATURES) $(`panel-${p}`).hidden = !(hover && f === p);
  $("task-view").hidden = ui.editingTasks;
  $("task-edit").hidden = !ui.editingTasks;
  $("location-form").hidden = !ui.editingLocation;
  if (hover && f === "calendar") renderTasks(ui.tasks);
  if (hover && f === "month") renderMonth();
  renderHeader();
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
  const fmt = (b: number) => (b < 1024 ? `${Math.round(b)} B/s` : b < 1048576 ? `${Math.round(b / 1024)} KB/s` : `${(b / 1048576).toFixed(1)} MB/s`);
  $("net-down").textContent = `↓ ${fmt(p.down)}`;
  $("net-up").textContent = `↑ ${fmt(p.up)}`;
}

function onMedia(m: Media): void {
  ui.media = m;
  if (m.state === "Idle") ui.lyric = "";
  $("btn-play").innerHTML = icon(m.state === "Playing" ? "pause" : "play");
  updateFx();
  renderHeader();
}

function onLyric(text: string): void {
  const was = showingLyrics();
  ui.lyric = text;
  if (was !== showingLyrics()) transition(); // width changes between 640 and normal
  else renderHeader();
}

function onWeather(w: Weather): void {
  $("weather-temp").textContent = w.temp;
  $("weather-city").textContent = w.city;
  $("weather-cond").textContent = w.desc;
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
  ui.feat = FEATURES.indexOf("weather");
  ui.editingLocation = true;
  clearTimeout(revertTimer);
  if (ui.state === "hover") transition();
  else setState("hover");
  // Opened from the menu while the cursor is elsewhere: fall back to idle if never reached.
  if (!ui.hovering) revertTimer = window.setTimeout(() => setState("idle"), 6000);
  window.setTimeout(() => $<HTMLInputElement>("location-input").focus(), 300);
}

// ---------- user input ----------
function wireInput(): void {
  window.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    invoke("show_context_menu").catch(console.error);
  });

  let lastWheel = 0;
  window.addEventListener(
    "wheel",
    (e) => {
      if (ui.state !== "hover" || ui.editingTasks || ui.editingLocation) return;
      if (e.timeStamp - lastWheel < 250) return; // trackpads fire many events per gesture
      lastWheel = e.timeStamp;
      ui.feat = (ui.feat + (e.deltaY > 0 ? 1 : -1) + FEATURES.length) % FEATURES.length;
      transition();
    },
    { passive: true },
  );

  document.querySelectorAll<HTMLElement>("[data-action]").forEach((b) => {
    b.onclick = () => invoke("run_action", { id: b.dataset.action }).catch(console.error);
  });
  document.querySelectorAll<HTMLElement>("[data-media]").forEach((b) => {
    b.onclick = () => invoke("media_control", { action: b.dataset.media }).catch(console.error);
  });
  document.querySelectorAll<HTMLElement>("[data-scroll]").forEach((b) => {
    b.onclick = () => scrollBalls(Number(b.dataset.scroll), pillTop());
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
  initBalls();
  wireInput();

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
  ui.settings = await invoke<Settings>("get_settings");
  transition();

  window.setInterval(() => ui.state === "idle" && renderHeader(), 1000);
}

boot().catch(console.error);
