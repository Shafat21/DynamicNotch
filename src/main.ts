// Island state machine + event wiring. Rust pushes events (perf, media, hover, ...);
// this file decides the island's size/state and which view is visible.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { agendaRows, renderAgenda, renderCard, type CalPayload } from "./calendar";
import { initFocus } from "./focus";
import { hydrateIcons, icon } from "./icons";
import { createPal, greet, updateMood } from "./pal";
import { getSelectedTask, renderApps, renderMonth, renderTasks, type LauncherApp, type Task } from "./panels";

type Media = { state: "Playing" | "Paused" | "Idle"; title: string; artist: string; accent: string; duration: number; seekable: boolean };
type Progress = { position: number; duration: number };
type Perf = { cpu: number; ram: number; disk: number; down: number; up: number };
type Weather = { city: string; temp: string; desc: string; icon: string; hourly: { time: string; temp: string; icon: string }[] };
type Settings = { location: string; lat: number; lon: number; islandStyle: string; animationStyle: string; autostart: boolean };
type Display = { uiScale: number; monitor: { index: number; name: string; width: number; height: number; scale: number } };
type State = "idle" | "hover" | "notify";
type View = "dashboard" | "perf" | "media" | "weather" | "calendar" | "month" | "agenda"; // "calendar" = Tasks

const PANELS = ["dashboard", "perf", "media", "weather", "calendar", "month", "agenda"] as const; // views with their own <section>
const ANIM_KEYS: Record<string, string> = { "Glow Sweep": "glow", "Fluid Blobs": "blobs", "Neon Border": "neon" };

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const shell = $("shell");
const island = $("island");
const view = $("view");

const ui = {
  state: "idle" as State,
  view: "dashboard" as View,
  hovering: false,
  hidden: false, // a fullscreen app is in front: island slid away, click-through
  charging: false,
  editingTasks: false,
  editingLocation: false,
  editingCalendar: false,
  calendar: null as CalPayload | null,
  media: { state: "Idle", title: "", artist: "", accent: "#000000", duration: 0, seekable: true } as Media,
  progress: { position: 0, duration: 0 } as Progress,
  seekPreview: null as number | null, // seconds while dragging the progress bar
  showLyric: false,
  lyric: "",
  event: { title: "", text: "" },
  settings: { islandStyle: "Default", animationStyle: "Fluid Blobs" } as Settings,
  tasks: [] as Task[],
  scale: 1, // display uiScale from Rust: sizes below are 1080p base px, multiplied in transition()
};

const isNotch = () => ui.settings.islandStyle === "Notch Nook";
const pillTop = () => (isNotch() ? 0 : 10);
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
    case "media": return [460, ui.showLyric ? 175 : 150];
    case "perf": return [360, 180];
    case "weather": return [360, ui.editingLocation ? 205 : 160];
    case "calendar": return ui.editingTasks ? [540, 520] : [420, calendarHeight()];
    case "month": return [360, 150];
    case "agenda": return [420, Math.min(360, 80 + 36 * agendaRows(ui.calendar)) + (ui.editingCalendar ? 50 : 0)];
  }
}

// ---------- transitions ----------
let viewTimer = 0;
let revertTimer = 0;
let morphTimer = 0;

function restartAnimation(node: HTMLElement, cls: string): void {
  node.classList.remove(cls);
  void node.offsetWidth; // force reflow so the animation starts again
  node.classList.add(cls);
}

/** Morph to the size of the current state, swap content halfway through the fade. */
function transition(): void {
  const [bw, bh] = targetSize();
  const k = ui.scale;
  const [w, h] = [bw * k, bh * k];
  const r = Math.min(bh / 2, isNotch() ? 18 : 30) * k;
  shell.style.setProperty("--ui", String(k));
  shell.style.setProperty("--w", `${w}px`);
  shell.style.setProperty("--h", `${h}px`);
  shell.style.setProperty("--r", `${r}px`);
  shell.style.setProperty("--scoop", `${Math.min(r * 1.4, 25 * k)}px`);
  shell.dataset.style = ui.settings.islandStyle;
  // compositor hint only while the 850 ms morph runs
  shell.style.willChange = "width, height";
  clearTimeout(morphTimer);
  morphTimer = window.setTimeout(() => (shell.style.willChange = ""), 850);

  island.dataset.bg = ui.state === "hover" ? ui.view : "";
  updateFx();
  if (!ui.charging) restartAnimation($("island").querySelector<HTMLElement>(".shine")!, "run");

  view.style.setProperty("--slide", `${bw / 4}px`); // #view is zoomed, so base px
  restartAnimation(view, "swap");
  clearTimeout(viewTimer);
  viewTimer = window.setTimeout(renderView, 250);

  sendHitRect(w, h, k);
}

/** Re-morph only when the size actually changes; media/lyric events arrive often. */
function refresh(): void {
  const [w, h] = targetSize().map((v) => v * ui.scale);
  if (shell.style.getPropertyValue("--w") !== `${w}px` || shell.style.getPropertyValue("--h") !== `${h}px`) transition();
  else renderHeader();
}

function updateFx(): void {
  const on = ui.state === "hover" ? ui.view === "media" : playingTitle() !== "";
  island.dataset.anim = on ? ANIM_KEYS[ui.settings.animationStyle] ?? "blobs" : "";
  island.style.setProperty("--accent", ui.media.accent);
}

/** Tell Rust where the island is so it can toggle click-through. */
function sendHitRect(w: number, h: number, k: number): void {
  const left = (window.innerWidth - w) / 2;
  const rect = ui.hidden ? { x: 0, y: 0, w: 0, h: 0 } : { x: left - 15 * k, y: 0, w: w + 30 * k, h: pillTop() + h + 30 * k };
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
  if (ui.hidden) return; // pops are dropped, not queued
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
  if (ui.state === "idle" && playingTitle()) {
    setStatus(null, playingTitle());
    return;
  }
  const now = new Date();
  const day = now.toLocaleDateString("en-US", { weekday: "short" });
  const time = now.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
  setStatus(null, `${day}, ${time}`);
}

const calHandlers = {
  join: (id: string) => invoke("open_join", { id }).catch((err) => showEvent("Calendar", String(err), 3500)),
  add: () => openCalendarEditor(),
};

function renderCalendar(): void {
  if (ui.state !== "hover") return;
  if (ui.view === "dashboard") renderCard(ui.calendar, calHandlers);
  if (ui.view === "agenda") renderAgenda(ui.calendar, calHandlers);
}

function renderView(): void {
  const hover = ui.state === "hover";
  const v = ui.view;
  $("header").hidden = hover;
  for (const p of PANELS) $(`panel-${p}`).hidden = !(hover && v === p);
  $("task-view").hidden = ui.editingTasks;
  $("task-edit").hidden = !ui.editingTasks;
  $("location-form").hidden = !ui.editingLocation;
  $("cal-form").hidden = !ui.editingCalendar;
  renderCalendar();
  if (hover && v === "calendar") renderTasks(ui.tasks);
  if (hover && v === "month") renderMonth();
  renderHeader();
}

function openView(v: View): void {
  ui.view = v;
  ui.editingTasks = ui.editingLocation = ui.editingCalendar = false;
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

/** `m:ss`, or `h:mm:ss` from one hour. */
function formatTime(secs: number): string {
  const t = Math.max(0, Math.floor(secs));
  const h = Math.floor(t / 3600);
  const m = Math.floor((t % 3600) / 60);
  const s = String(t % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

/** Album art arrives only on track change (or a late retry), separately from `media`. */
function onArt({ art }: { art: string }): void {
  for (const id of ["player-art", "tile-media-art"]) {
    const img = $<HTMLImageElement>(id);
    img.hidden = !art;
    if (art) img.src = art; // data URI built by the backend
    else img.removeAttribute("src");
  }
}

function onMedia(m: Media): void {
  const idle = m.state === "Idle";
  if (m.title !== ui.media.title || m.artist !== ui.media.artist) ui.progress.position = 0;
  ui.media = m;
  ui.progress.duration = m.duration;
  if (idle) ui.lyric = "";
  $("btn-play").innerHTML = icon(m.state === "Playing" ? "pause" : "play", 32);
  $("player-title").textContent = idle ? "Nothing playing" : m.title;
  $("player-artist").textContent = idle ? "" : m.artist;
  $("player-eq").classList.toggle("on", m.state === "Playing");
  $("tile-media-title").textContent = idle ? "Nothing playing" : m.title;
  $("tile-media-artist").textContent = idle ? "" : m.artist;
  $("tile-media").style.setProperty("--accent", m.accent);
  renderProgress();
  renderLyric();
  updateFx();
  refresh(); // idle pill width follows the song title
}

function onProgress(p: Progress): void {
  if (ui.seekPreview !== null) return; // the user is dragging; their preview wins
  ui.progress = p;
  renderProgress();
}

function renderProgress(): void {
  const { duration } = ui.progress;
  const pos = Math.min(ui.seekPreview ?? ui.progress.position, duration);
  $("player-progress").style.visibility = duration > 0 ? "" : "hidden";
  $("player-track").classList.toggle("readonly", !ui.media.seekable);
  $("player-elapsed").textContent = formatTime(pos);
  $("player-remaining").textContent = `-${formatTime(duration - pos)}`;
  $("player-fill").style.width = duration > 0 ? `${(pos / duration) * 100}%` : "0";
}

function renderLyric(): void {
  $("player-lyric").hidden = !ui.showLyric;
  $("player-lyric").textContent = ui.lyric || "♪";
  $("btn-lyrics").classList.toggle("on", ui.showLyric);
  $("btn-lyrics").setAttribute("aria-pressed", String(ui.showLyric));
}

function onLyric(text: string): void {
  ui.lyric = text;
  renderLyric();
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

function onDisplay(d: Display): void {
  ui.scale = d.uiScale;
  transition(); // the window may also have been resized: re-centre the hit rect
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

/** Fullscreen app in front: slide away and go click-through; come back to the idle pill afterwards. */
function onFullscreen(hidden: boolean): void {
  ui.hidden = hidden;
  shell.classList.toggle("away", hidden);
  clearTimeout(revertTimer);
  ui.hovering = false;
  ui.state = "idle";
  ui.view = "dashboard";
  transition(); // also sends the (empty or normal) hit rect
}

function onHover(inside: boolean): void {
  if (ui.hidden) return;
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

function onCalendar(p: CalPayload): void {
  ui.calendar = p;
  refresh(); // the agenda's height follows the event count
  renderCalendar();
}

function openCalendarEditor(): void {
  ui.view = "agenda";
  ui.editingCalendar = true;
  if (ui.state === "hover") transition();
  else setState("hover");
  window.setTimeout(() => $<HTMLInputElement>("cal-input").focus(), 300);
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
function wirePlayer(): void {
  $("btn-lyrics").onclick = () => {
    ui.showLyric = !ui.showLyric;
    renderLyric();
    refresh(); // card height 150 <-> 175
  };
  $("btn-open-player").onclick = () => invoke("media_open_player").catch((err) => showEvent("Music", String(err), 3500));

  // Seek: preview while dragging, seek once on release.
  const track = $("player-track");
  const secondsAt = (e: PointerEvent) => {
    const r = track.getBoundingClientRect();
    return Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)) * ui.progress.duration;
  };
  track.onpointerdown = (e) => {
    if (e.button !== 0 || !e.isPrimary || !ui.media.seekable || ui.progress.duration <= 0) return;
    track.setPointerCapture(e.pointerId);
    ui.seekPreview = secondsAt(e);
    renderProgress();
  };
  track.onpointermove = (e) => {
    if (ui.seekPreview === null) return;
    ui.seekPreview = secondsAt(e);
    renderProgress();
  };
  track.onpointerup = (e) => {
    if (ui.seekPreview === null) return;
    const seconds = secondsAt(e);
    ui.seekPreview = null;
    ui.progress.position = seconds;
    renderProgress();
    invoke("media_seek", { seconds }).catch((err) => showEvent("Music", String(err), 3500));
  };
  track.onpointercancel = () => {
    ui.seekPreview = null;
    renderProgress();
  };
}

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
  wirePlayer();

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

  // calendar
  $("tile-clock").onclick = () => openView("agenda");
  $("tile-clock").onkeydown = (e) => {
    // only the card itself: Enter/Space on the nested +/Join buttons must not also open the agenda
    if (e.target !== e.currentTarget || (e.key !== "Enter" && e.key !== " ")) return;
    e.preventDefault();
    openView("agenda");
  };
  $("btn-cal-link").onclick = (e) => {
    e.stopPropagation();
    openCalendarEditor();
  };
  $("btn-agenda-link").onclick = openCalendarEditor;
  $("cal-cancel").onclick = () => {
    ui.editingCalendar = false;
    transition();
  };
  const saveCalendar = async (url: string) => {
    ui.editingCalendar = false;
    try {
      onCalendar(await invoke<CalPayload>("set_calendar_url", { url }));
      $<HTMLInputElement>("cal-input").value = "";
      showEvent("Calendar", url ? "Calendar connected" : "Calendar removed", 2500);
    } catch (err) {
      showEvent("Calendar", `Error: ${err}`, 3500);
    }
  };
  $("cal-remove").onclick = () => saveCalendar("");
  $("cal-form").onsubmit = (e) => {
    e.preventDefault();
    const input = $<HTMLInputElement>("cal-input");
    if (!input.value.trim()) return input.focus();
    saveCalendar(input.value);
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
    listen<Progress>("media-progress", (e) => onProgress(e.payload)),
    listen<{ art: string }>("media-art", (e) => onArt(e.payload)),
    listen<Weather>("weather", (e) => onWeather(e.payload)),
    listen<CalPayload>("calendar", (e) => onCalendar(e.payload)),
    listen<string>("accent", (e) => document.documentElement.style.setProperty("--sys-accent", e.payload)),
    listen<Settings>("settings", (e) => onSettings(e.payload)),
    listen<Display>("display", (e) => onDisplay(e.payload)),
    listen<{ name: string; on: boolean }>("keylock", (e) => showEvent(e.payload.name, e.payload.on ? "ENABLED" : "DISABLED", 2500)),
    listen<{ app: string; title: string; text: string }>("notify", (e) => {
      const { app, title, text } = e.payload;
      showEvent(app, title ? `${title}: ${text}` : text, 3500);
    }),
    listen<boolean>("fullscreen", (e) => onFullscreen(e.payload)),
    listen("charging", onCharging),
    listen("edit-location", openLocationEditor),
  ]);

  // Catch up on anything the monitors published before this page loaded.
  const snap = await invoke<Record<string, unknown>>("get_snapshot");
  if (snap.accent) document.documentElement.style.setProperty("--sys-accent", snap.accent as string);
  if (snap.perf) onPerf(snap.perf as Perf);
  if (snap.weather) onWeather(snap.weather as Weather);
  if (snap.calendar) ui.calendar = snap.calendar as CalPayload;
  if (snap.media) onMedia(snap.media as Media);
  if (snap.lyric) onLyric(snap.lyric as string);
  if (snap["media-progress"]) onProgress(snap["media-progress"] as Progress);
  if (snap["media-art"]) onArt(snap["media-art"] as { art: string });
  if (snap.fullscreen) onFullscreen(true);
  if (snap.display) ui.scale = (snap.display as Display).uiScale; // applied by onSettings' transition below

  ui.tasks = await invoke<Task[]>("list_tasks");
  onSettings(await invoke<Settings>("get_settings"));

  window.setInterval(() => {
    if (ui.state === "idle") renderHeader();
    // Advance locally between backend updates (they come at most once per second).
    if (ui.media.state === "Playing" && ui.seekPreview === null && ui.progress.duration > 0) {
      ui.progress.position = Math.min(ui.progress.position + 1, ui.progress.duration);
      renderProgress();
    }
  }, 1000);
  window.setInterval(renderCalendar, 30_000); // relative labels ("In 20 min") while visible
}

boot().catch(console.error);
