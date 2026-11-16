// Island state machine + event wiring. Rust pushes events (perf, media, hover, ...);
// this file decides the island's size/state and which tab is visible.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { idlePillWidth } from "./idle-width";
import {
  addPrompt, anyWorking, closePrompt, currentPrompt, dismissAll, dismissPrompt, initAssistant, popMood, promptHeight, setBridge, setDictating, setSessions, setTyping, setUsage, usage,
  type Bridge, type Prompt, type Session,
} from "./assistant";
import { dictating, dictatingPage, initDictation, onDictation, type DictEvent } from "./dictation";
import { initNotetaker, recWidth, setCall, type CallEvent } from "./notetaker";
import { level, live, warnDot, type Usage } from "./usage";
import { renderAgenda, renderCard, type CalPayload } from "./calendar";
import { initFocus } from "./focus";
import { initClipboard, showClipboard } from "./clipboard";
import { initShelf, showShelf } from "./shelf";
import { initNotes, openNote, showNotes } from "./notes";
import { initBluetooth, showBluetooth } from "./bluetooth";
import { initSound, showSound } from "./sound";
import { addDue, chime, currentDue, initReminders, overdue, showReminders, type Due } from "./reminders";
import { hydrateIcons, icon } from "./icons-line";
import { hideTip, initTooltips } from "./tooltip";
import type { PalMood } from "./mood";
import { createPal, greet } from "./pal";
import { fitDock, initTasks, renderApps, renderMonth, renderTasks, type LauncherApp, type Task } from "./panels";
import { renderHome } from "./ui/home";
import { closeMore, fitTabs, initTabs, renderTabs, setActiveTab, visibleTabs } from "./ui/tabs";
import { applyTheme, DEFAULT_LOOK, type Look } from "./ui/theme";

type Media = { state: "Playing" | "Paused" | "Idle"; title: string; artist: string; accent: string; duration: number; seekable: boolean };
type Progress = { position: number; duration: number };
type Perf = { cpu: number; ram: number; disk: number; down: number; up: number };
type Weather = { city: string; temp: string; desc: string; icon: string; hourly: { time: string; temp: string; icon: string }[] };
type Settings = { location: string; lat: number; lon: number; animationStyle: string; autostart: boolean; reminderSound: boolean };
type Display = { uiScale: number; monitor: { index: number; name: string; width: number; height: number; scale: number } };
type State = "idle" | "hover" | "notify" | "prompt" | "reminder" | "dictation";

const ANIM_KEYS: Record<string, string> = { "Glow Sweep": "glow", "Fluid Blobs": "blobs", "Neon Border": "neon" };

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const shell = $("shell");
const island = $("island");
const view = $("view");

const ui = {
  state: "idle" as State,
  tab: "home", // a layout tab id (ui/tabs.ts); its page is #tab-<id>
  hovering: false,
  pinned: false, // pin button: stay open while the cursor is away
  hidden: false, // a fullscreen app is in front: island slid away, click-through
  charging: false,
  editingLocation: false,
  editingCalendar: false,
  calendar: null as CalPayload | null,
  media: { state: "Idle", title: "", artist: "", accent: "#000000", duration: 0, seekable: true } as Media,
  progress: { position: 0, duration: 0 } as Progress,
  seekPreview: null as number | null, // seconds while dragging the progress bar
  showLyric: true, // Music tab's lyrics panel (in memory)
  lyric: "",
  event: { title: "", text: "", mood: null as PalMood | null }, // mood: a coding agent pop shows Pal
  settings: { animationStyle: "Fluid Blobs" } as Settings,
  look: DEFAULT_LOOK as Look,
  tasks: [] as Task[],
  scale: 1, // display uiScale from Rust: sizes below are 1080p base px, multiplied in transition()
};

const look = () => ui.look.appearance;
const pillTop = () => (look().shape === "notch" ? 0 : 10);
const playingTitle = () => (ui.media.state === "Playing" ? ui.media.title : "");

let measureCtx: CanvasRenderingContext2D | null = null;
const popPal = createPal(22, "calm"); // coding agent pops: fixed mood per pop
const spinning = () => ui.state === "idle" && anyWorking();

/** Idle pill: clock width, or wide enough for the playing song title (never lyrics); + the working spinner. */
function idleWidth(): number {
  const title = playingTitle();
  if (!title) return idlePillWidth(null, look().collapsedW, anyWorking(), recWidth());
  measureCtx ??= document.createElement("canvas").getContext("2d")!;
  measureCtx.font = getComputedStyle($("status-text")).font;
  return idlePillWidth(measureCtx.measureText(title).width, look().collapsedW, anyWorking(), recWidth());
}

/** Base px at 1080p (appearance sizes); geometry() multiplies by the display scale. */
function targetSize(): [number, number] {
  const a = look();
  if (ui.charging) return [850, a.collapsedH];
  if (ui.state === "idle") return [idleWidth(), a.collapsedH];
  if (ui.state === "notify") return [380, a.collapsedH];
  if (ui.state === "prompt") return [a.openW, promptHeight(a.openW, a.openH, a.textScale)];
  if (ui.state === "reminder") return [640, 76];
  if (ui.state === "dictation") return [["done", "error"].includes($("dictation").dataset.phase ?? "") ? 400 : 300, a.collapsedH]; // room for the text
  return [a.openW, a.openH]; // every tab shares the open size; long pages scroll
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

/** Size, corner radius and shape of the current state on #shell, and the matching hit rect. */
function geometry(): void {
  const [bw, bh] = targetSize();
  const k = ui.scale;
  const [w, h] = [bw * k, bh * k];
  const r = Math.min(bh / 2, look().radius + 14) * k; // island corners follow the card radius
  shell.style.setProperty("--ui", String(k));
  shell.style.setProperty("--w", `${w}px`);
  shell.style.setProperty("--h", `${h}px`);
  shell.style.setProperty("--r", `${r}px`);
  shell.style.setProperty("--scoop", `${Math.min(r * 1.4, 25 * k)}px`);
  shell.style.setProperty("--ch", `${look().collapsedH}px`); // read inside the zoomed #view: base px
  shell.dataset.shape = look().shape;
  sendHitRect(w, h, k);
}

const sizeChanged = () => {
  const [w, h] = targetSize().map((v) => v * ui.scale);
  return shell.style.getPropertyValue("--w") !== `${w}px` || shell.style.getPropertyValue("--h") !== `${h}px`;
};

/** Morph to the size of the current state, swap content halfway through the fade. */
function transition(): void {
  geometry();
  // compositor hint only while the 850 ms morph runs
  shell.style.willChange = "width, height";
  clearTimeout(morphTimer);
  morphTimer = window.setTimeout(() => (shell.style.willChange = ""), 850);

  updateFx();
  if (!ui.charging) restartAnimation($("island").querySelector<HTMLElement>(".shine")!, "run");

  view.style.setProperty("--slide", `${targetSize()[0] / 4}px`); // #view is zoomed, so base px
  restartAnimation(view, "swap");
  clearTimeout(viewTimer);
  viewTimer = window.setTimeout(renderView, 250);
}

/** Re-morph only when the size actually changes; media/lyric events arrive often. */
function refresh(): void {
  if (sizeChanged()) transition();
  else renderHeader();
}

function updateFx(): void {
  const on = ui.state === "hover" ? ui.tab === "music" : playingTitle() !== "";
  island.dataset.anim = on ? ANIM_KEYS[ui.settings.animationStyle] ?? "blobs" : "";
  island.style.setProperty("--media", ui.media.accent);
}

/** Tell Rust where the island is so it can toggle click-through. */
function sendHitRect(w: number, h: number, k: number): void {
  const left = (window.innerWidth - w) / 2;
  const rect = ui.hidden ? { x: 0, y: 0, w: 0, h: 0 } : { x: left - 15 * k, y: 0, w: w + 30 * k, h: pillTop() + h + 30 * k };
  invoke("set_hit_rect", { rect }).catch(console.error);
}

function setState(s: State): void {
  if (s === "idle") {
    setPinned(false);
    if (look().openOn === "home") ui.tab = "home"; // otherwise the next hover reopens the last tab
  }
  if (s !== "hover") closeMore(); // clicks outside the island never reach the webview
  if (ui.state === s) return;
  ui.state = s;
  if (s === "hover" && !visibleTabs().includes(ui.tab)) ui.tab = visibleTabs()[0] ?? "home";
  if (s === "hover") greet();
  transition();
}

function showEvent(title: string, text: string, ms: number, mood: PalMood | null = null): void {
  if (ui.hidden || ui.state === "prompt" || ui.state === "reminder" || ui.state === "dictation") return; // pops are dropped, not queued; a prompt / reminder / dictation wins
  ui.event = { title, text, mood };
  clearTimeout(revertTimer);
  if (ui.state === "notify") transition();
  else setState("notify");
  revertTimer = window.setTimeout(() => setState(ui.hovering || ui.pinned ? "hover" : "idle"), ms);
}

// ---------- rendering ----------
/** Header icon + text: an icon name, "pal" (follows the mood, idle pill) or "pop" (Pal with the pop's mood). */
function setStatus(kind: string, text: string): void {
  $("status-pal").hidden = kind !== "pal";
  $("status-pop").hidden = kind !== "pop";
  $("status-icon").hidden = kind === "pal" || kind === "pop";
  if (!$("status-icon").hidden) $("status-icon").innerHTML = icon(kind, 18);
  $("status-spin").hidden = !spinning();
  $("status-text").textContent = text;
}

/** Idle pill: small dot on Pal when the 5-hour window is at 80 %+ or the limit was hit. */
function renderUsageDot(): void {
  const u = usage();
  const now = Date.now() / 1000;
  const dot = $("usage-dot");
  dot.hidden = !warnDot(u, now);
  if (u) dot.dataset.level = level(live(u.fiveHour, now)?.usedPercentage ?? 0, u.limitReached);
}

function onUsage(u: Usage): void {
  setUsage(u);
  renderUsageDot();
}

const clockTime = (d: Date) => d.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });

function renderHeader(): void {
  if (ui.state === "notify") {
    const { title, text, mood } = ui.event;
    if (mood) popPal.setMood(mood);
    setStatus(mood ? "pop" : title.includes("Lock") ? "bolt" : "email", `${title} - ${text}`);
    return;
  }
  if (ui.state === "idle" && playingTitle()) {
    setStatus("pal", playingTitle());
    return;
  }
  const now = new Date();
  setStatus("pal", `${now.toLocaleDateString("en-US", { weekday: "short" })}, ${clockTime(now)}`);
}

/** Date/time in the tab bar and on the Home clock card. */
function renderClock(): void {
  const now = new Date();
  const fmt = (o: Intl.DateTimeFormatOptions) => now.toLocaleDateString("en-US", o);
  const dayMonth = `${now.getDate()} ${fmt({ month: "short" })}`;
  $("top-clock").textContent = `${fmt({ weekday: "short" })}, ${dayMonth}  ${clockTime(now)}`;
  $("clock-time").textContent = clockTime(now);
  $("clock-date").textContent = `${fmt({ weekday: "long" })}, ${dayMonth}`;
}

const calHandlers = {
  join: (id: string) => invoke("open_join", { id }).catch((err) => showEvent("Calendar", String(err), 3500)),
  add: () => openCalendarEditor(),
};

function renderCalendar(): void {
  if (ui.state !== "hover") return;
  if (ui.tab === "home") renderCard(ui.calendar, calHandlers);
  if (ui.tab === "calendar") renderAgenda(ui.calendar, calHandlers);
}

function renderView(): void {
  const hover = ui.state === "hover";
  const prompt = ui.state === "prompt";
  const reminder = ui.state === "reminder";
  const dict = ui.state === "dictation";
  const t = ui.tab;
  $("header").hidden = hover || prompt || reminder || dict;
  $("reminder").hidden = !reminder;
  $("dictation").hidden = !dict;
  chime(reminder ? currentDue()?.id ?? null : null);
  $("open").hidden = !hover && !prompt;
  $("prompt").hidden = !prompt;
  $("pages").hidden = prompt; // the prompt state is sized for the prompt alone
  $("tabbar").setAttribute("aria-disabled", String(prompt)); // visible but inert until the prompt closes
  document.querySelectorAll<HTMLElement>(".page").forEach((p) => (p.hidden = p.id !== `tab-${t}`));
  setActiveTab(t);
  $("location-form").hidden = !ui.editingLocation;
  $("cal-form").hidden = !ui.editingCalendar;
  renderCalendar();
  if (hover && (t === "tasks" || t === "home")) renderTasks(ui.tasks);
  if (hover && t === "month") renderMonth();
  if (hover && t === "notes") showNotes();
  if (hover && t === "reminders") showReminders();
  if (hover && t === "clipboard") void showClipboard();
  if (hover && t === "shelf") void showShelf();
  if (hover && (t === "devices" || (t === "home" && !$("card-devices").hidden))) void showBluetooth();
  if (hover && (t === "sound" || (t === "home" && !$("card-sound").hidden))) void showSound();
  renderHeader();
  renderClock();
}

/** Switch the open island to tab `id` (Task 9 opens the Pal tab with it). Same size: no morph, just a fade. */
export function openTab(id: string): void {
  if (ui.state === "prompt") return; // never leave a pending prompt for a tab
  ui.tab = id;
  ui.editingLocation = ui.editingCalendar = false;
  if (ui.state !== "hover") return;
  updateFx();
  renderView();
  restartAnimation($("pages"), "fade");
}

// ---------- backend events ----------
function onPerf(p: Perf): void {
  const set = (val: string, bar: string, v: number) => {
    $(val).textContent = `${Math.round(v)}%`;
    $(bar).style.width = `${Math.min(100, v)}%`;
    $(bar).closest<HTMLElement>(".stat, #card-system")!.dataset.level = level(v, false); // amber from 80 %, red from 95 %
  };
  for (const [k, v] of [["cpu", p.cpu], ["ram", p.ram], ["disk", p.disk]] as const) set(`${k}-val`, `${k}-bar`, v);
  set("card-cpu", "card-cpu-bar", p.cpu);
  set("card-ram", "card-ram-bar", p.ram);
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
  document.querySelectorAll<HTMLElement>('[data-media="toggle"]').forEach((b) => {
    b.innerHTML = icon(m.state === "Playing" ? "pause" : "play", Number(b.dataset.size ?? 18));
  });
  $("player-title").textContent = idle ? "Nothing playing" : m.title;
  $("player-artist").textContent = idle ? "" : m.artist;
  $("player-state").textContent = idle ? "" : m.state;
  $("player-eq").classList.toggle("on", m.state === "Playing");
  $("tile-media-title").textContent = idle ? "Nothing playing" : m.title;
  $("tile-media-artist").textContent = idle ? "" : m.artist;
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
  $("music").classList.toggle("lyrics", ui.showLyric);
  const panel = $("player-lyric");
  panel.classList.toggle("empty", !ui.lyric);
  if (ui.lyric) panel.replaceChildren(Object.assign(document.createElement("div"), { className: "lyric-line", textContent: ui.lyric })); // new node = the line animates in
  else panel.replaceChildren(Object.assign(document.createElement("div"), { className: "lyric-note", textContent: "♪" }), Object.assign(document.createElement("div"), { className: "lyric-hint", textContent: ui.media.state === "Playing" ? "No synced lyrics for this song" : "Lyrics show here while music plays" }));
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
  $("clock-wtext").textContent = `${w.temp} ${w.desc}`;
  $("clock-city").textContent = w.city;
  $("clock-wicon").innerHTML = icon(w.icon, 18);
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

/** Appearance + layout from Settings (spec §22.3): tabs, Home cards, CSS variables. */
function applyLook(p: Look): void {
  ui.look = p;
  renderTabs(p.layout.tabs, ui.tab);
  renderHome(p.layout.home);
  applyTheme(p); // after the renders: it also paints the new badges
  fitTabs(); // text size may have changed the tab widths
}

function onAppearance(p: Look): void {
  applyLook(p);
  if (ui.state === "hover" && !visibleTabs().includes(ui.tab)) ui.tab = visibleTabs()[0] ?? "home";
  if (sizeChanged()) transition();
  else {
    geometry(); // radius/shape may still have changed
    renderView();
  }
}

function onSessions(list: Session[]): void {
  setSessions(list);
  refresh(); // idle pill width / spinner follow "any session working"
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
  if (hidden) dismissAll(); // the terminal asks instead
  shell.classList.toggle("away", hidden);
  clearTimeout(revertTimer);
  ui.hovering = false;
  setPinned(false);
  closeMore();
  ui.state = hidden ? "idle" : currentDue() ? "reminder" : dictating() ? "dictation" : "idle"; // reminders due meanwhile show once it's back
  if (look().openOn === "home") ui.tab = "home";
  transition(); // also sends the (empty or normal) hit rect
}

let backendHover = false; // last `hover` event: the pointer is really over the island

function onHover(inside: boolean): void {
  backendHover = inside;
  if (!inside) hideTip(); // click-through from here on: the webview gets no pointerout
  if (ui.hidden) return;
  ui.hovering = inside;
  if (ui.state === "notify" || ui.state === "prompt" || ui.state === "reminder" || (ui.state === "dictation" && !dictatingPage())) return;
  clearTimeout(revertTimer);
  if (inside) setState("hover");
  else if (ui.state === "hover" && !ui.pinned) revertTimer = window.setTimeout(() => setState("idle"), 1200);
}

function setPinned(on: boolean): void {
  ui.pinned = on;
  $("btn-pin").classList.toggle("on", on);
  $("btn-pin").setAttribute("aria-pressed", String(on));
}

// ---------- coding agent prompts ----------
function onPrompt(p: Prompt): void {
  if (ui.hidden) return; // the bridge lets the terminal ask while a fullscreen app is in front
  const onScreen = addPrompt(p);
  clearTimeout(revertTimer); // no auto-revert: answered, dismissed or timed out (prompt-closed)
  if (ui.state !== "prompt") setState("prompt");
  else if (onScreen) transition();
}

function onPromptClosed(id: number): void {
  if (!closePrompt(id) || ui.state !== "prompt") return;
  settle();
}

/** After a prompt, reminder or dictation ends: the next prompt, then the next reminder, a running dictation, then hover/idle. */
function settle(): void {
  const next: State = currentPrompt() ? "prompt" : currentDue() ? "reminder" : dictating() ? "dictation" : ui.hovering || ui.pinned ? "hover" : "idle";
  if (next === ui.state) transition(); // the next queued one
  else setState(next);
}

// ---------- dictation ----------
let dictTimer = 0;
/** Recording / transcribing pill; a flash of the pasted text or the error, then back. Below prompt / reminder. */
function onDictationEvent(e: DictEvent): void {
  const stay = onDictation(e);
  setDictating(e.state === "recording");
  clearTimeout(dictTimer);
  const showing = ui.state === "dictation";
  if (stay < 0) return void (showing && settle());
  if (e.page && ui.state === "hover") return; // a voice note in the open Notes tab: its mic button shows the state
  if (ui.hidden || ui.state === "prompt" || ui.state === "reminder") return; // fullscreen / answer first: settle() brings it back
  if (!showing) {
    clearTimeout(revertTimer); // a notify pop gives way
    setState("dictation");
  } else if (e.state !== "recording") refresh(); // done / error widen the pill
  if (stay > 0) dictTimer = window.setTimeout(() => ui.state === "dictation" && settle(), stay);
}

// ---------- reminders ----------
/** Banner stays until Snooze / Done; below an agent prompt, and held back while hidden. */
function onReminderDue(d: Due): void {
  if (!addDue(d) || ui.hidden || ui.state === "prompt") return; // shown when the prompt closes / fullscreen ends
  clearTimeout(revertTimer);
  setState("reminder");
}

function openLocationEditor(): void {
  ui.tab = "weather";
  ui.editingLocation = true;
  clearTimeout(revertTimer);
  if (ui.state === "hover") renderView();
  else setState("hover");
  // Opened from the menu while the cursor is elsewhere: fall back to idle if never reached.
  if (!ui.hovering && !ui.pinned) revertTimer = window.setTimeout(() => setState("idle"), 6000);
  window.setTimeout(() => $<HTMLInputElement>("location-input").focus(), 300);
}

function onCalendar(p: CalPayload): void {
  ui.calendar = p;
  renderCalendar();
}

function openCalendarEditor(): void {
  ui.tab = "calendar";
  ui.editingCalendar = true;
  if (ui.state === "hover") renderView();
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
  island.addEventListener("focusin", (e) => setTyping(e.target instanceof HTMLInputElement));
  island.addEventListener("focusout", () => setTyping(false));

  // navigation: tab bar, plus cards/links with data-goto (buttons nested in a card do their own thing)
  initTabs(openTab, () => ui.tab);
  document.querySelectorAll<HTMLElement>("[data-goto]").forEach((c) => {
    c.onclick = (e) => {
      const inner = (e.target as Element).closest("button, input, select");
      if (!inner || inner === c) openTab(c.dataset.goto!);
    };
    c.onkeydown = (e) => {
      if (e.target !== c || c instanceof HTMLButtonElement || (e.key !== "Enter" && e.key !== " ")) return;
      e.preventDefault();
      openTab(c.dataset.goto!);
    };
  });
  document.querySelectorAll<HTMLElement>("[data-settings]").forEach((b) => {
    b.onclick = () => invoke("open_settings").catch(console.error);
  });
  $("btn-pal-status").onclick = () => openTab("pal");
  $("btn-pin").onclick = () => setPinned(!ui.pinned);
  $("btn-collapse").onclick = () => (ui.state === "prompt" ? dismissPrompt() : setState("idle"));
  document.querySelectorAll<HTMLElement>("[data-add-app]").forEach((b) => {
    b.onclick = () => updateApps(invoke<LauncherApp[]>("pick_app"));
  });
  new ResizeObserver(() => fitDock()).observe($("card-launcher")); // morph, openW, textScale, density gaps, Home shown
  initFocus([$("card-focus"), $("timer-big")], () => showEvent("Focus", "Time's up", 3500));

  document.querySelectorAll<HTMLElement>("[data-action]").forEach((b) => {
    b.onclick = () => invoke("run_action", { id: b.dataset.action }).catch(console.error);
  });
  document.querySelectorAll<HTMLElement>("[data-media]").forEach((b) => {
    b.onclick = () => invoke("media_control", { action: b.dataset.media }).catch(console.error);
  });
  wirePlayer();

  initNotes();
  initNotetaker({
    changed: refresh, // the idle pill widens for the recording badge
    error: (msg) => showEvent("Notetaker", msg, 4000),
    // open, the Notes tab already shows the new note: pop only when the island is closed
    done: (id) => {
      void openNote(id);
      if (ui.state !== "hover") showEvent("Notetaker", "Call note saved in Notes", 3500);
    },
  });
  initClipboard();
  initShelf({
    dragging: (on) => {
      $("tab-shelf").firstElementChild?.classList.toggle("dropping", on);
      if (ui.hidden) return;
      clearTimeout(revertTimer);
      if (!on) {
        // drag left or dropped: back to the real pointer state (the backend hover loop keeps reporting it)
        ui.hovering = backendHover;
        if (ui.state === "hover" && !ui.hovering && !ui.pinned) revertTimer = window.setTimeout(() => setState("idle"), 1200);
        return;
      }
      if (!visibleTabs().includes("shelf")) return;
      ui.hovering = true; // stay open while the drag is over the island
      if (ui.state === "notify" || ui.state === "prompt" || ui.state === "reminder") return;
      ui.tab = "shelf";
      if (ui.state === "hover") openTab("shelf");
      else setState("hover");
    },
  });
  initBluetooth({ visible: () => ui.state === "hover" && (ui.tab === "devices" || (ui.tab === "home" && !$("card-devices").hidden)) });
  initSound({ switchFailed: () => openTab("sound") });
  initReminders({ changed: () => ui.state === "reminder" && settle(), sound: () => ui.settings.reminderSound !== false });

  // tasks
  // a crossed-off task goes once its undo window ends (panels.ts)
  initTasks(async (id) => {
    try {
      ui.tasks = await invoke<Task[]>("delete_task", { id });
      renderTasks(ui.tasks);
    } catch (err) {
      showEvent("Tasks", String(err), 3500);
    }
  });
  $("task-form").onsubmit = async (e) => {
    e.preventDefault();
    const name = $<HTMLInputElement>("task-name");
    const time = $<HTMLInputElement>("task-time");
    if (!name.value.trim()) return name.focus();
    try {
      ui.tasks = await invoke<Task[]>("add_task", { name: name.value, time: time.value, category: $("task-cat").dataset.cat ?? "Other" });
      name.value = time.value = "";
      renderTasks(ui.tasks);
    } catch (err) {
      showEvent("Tasks", String(err), 3500);
    }
  };


  // calendar
  $("btn-cal-link").onclick = (e) => {
    e.stopPropagation();
    openCalendarEditor();
  };
  $("btn-agenda-link").onclick = openCalendarEditor;
  $("cal-cancel").onclick = () => {
    ui.editingCalendar = false;
    renderView();
  };
  const saveCalendar = async (url: string) => {
    ui.editingCalendar = false;
    renderView();
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
    renderView();
  };
  $("location-form").onsubmit = async (e) => {
    e.preventDefault();
    const input = $<HTMLInputElement>("location-input");
    const city = input.value.trim();
    if (!city) return input.focus();
    ui.editingLocation = false;
    renderView();
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
  initTooltips();
  // Pals follow the shared mood (assistant.ts feeds it from coding agent sessions)
  $("status-pal").append(createPal(22).el);
  $("status-pop").append(popPal.el);
  $("pal-card-face").append(createPal(34).el);
  $("btn-pal-status").append(createPal(20).el);
  $("pal-tab-face").append(createPal(60).el);
  applyLook(DEFAULT_LOOK);
  initAssistant();
  initDictation();
  wireInput();
  renderLyric();
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
    listen<Settings>("settings", (e) => onSettings(e.payload)),
    listen<Display>("display", (e) => onDisplay(e.payload)),
    listen<Look>("appearance", (e) => onAppearance(e.payload)),
    listen<{ name: string; on: boolean }>("keylock", (e) => showEvent(e.payload.name, e.payload.on ? "ENABLED" : "DISABLED", 2500)),
    listen<{ app: string; title: string; text: string }>("notify", (e) => {
      const { app, title, text } = e.payload;
      showEvent(app, title ? `${title}: ${text}` : text, 3500, popMood(app));
    }),
    listen<Session[]>("sessions", (e) => onSessions(e.payload)),
    listen<Bridge>("bridge", (e) => setBridge(e.payload)),
    listen<Prompt>("prompt", (e) => onPrompt(e.payload)),
    listen<{ id: number }>("prompt-closed", (e) => onPromptClosed(e.payload.id)),
    listen<boolean>("fullscreen", (e) => onFullscreen(e.payload)),
    listen<Due>("reminder-due", (e) => onReminderDue(e.payload)),
    listen<DictEvent>("dictation", (e) => onDictationEvent(e.payload)),
    listen<Usage>("usage", (e) => onUsage(e.payload)),
    listen("charging", onCharging),
    listen<LauncherApp[]>("apps", (e) => updateApps(Promise.resolve(e.payload))), // changed in the Settings window
  ]);

  // Catch up on anything the monitors published before this page loaded.
  const snap = await invoke<Record<string, unknown>>("get_snapshot");
  if (snap.perf) onPerf(snap.perf as Perf);
  if (snap.weather) onWeather(snap.weather as Weather);
  if (snap.calendar) ui.calendar = snap.calendar as CalPayload;
  if (snap.media) onMedia(snap.media as Media);
  if (snap.lyric) onLyric(snap.lyric as string);
  if (snap["media-progress"]) onProgress(snap["media-progress"] as Progress);
  if (snap["media-art"]) onArt(snap["media-art"] as { art: string });
  if (snap.fullscreen) onFullscreen(true);
  if (snap.bridge) setBridge(snap.bridge as Bridge);
  if (snap.sessions) setSessions(snap.sessions as Session[]);
  if (snap.usage) onUsage(snap.usage as Usage);
  const call = snap.notetaker as CallEvent | undefined;
  if (call && (call.state === "recording" || call.state === "processing")) setCall(call);
  if (snap.display) ui.scale = (snap.display as Display).uiScale; // applied by onSettings' transition below
  if (snap.appearance) applyLook(snap.appearance as Look); // likewise

  ui.tasks = await invoke<Task[]>("list_tasks");
  onSettings(await invoke<Settings>("get_settings"));
  for (const d of await overdue()) onReminderDue(d); // missed while closed, or emitted before this page listened

  window.setInterval(() => {
    if (ui.state === "idle") {
      renderHeader();
      renderUsageDot(); // windows expire at their reset time
    }
    if (ui.state === "hover") renderClock();
    // Advance locally between backend updates (they come at most once per second).
    if (ui.media.state === "Playing" && ui.seekPreview === null && ui.progress.duration > 0) {
      ui.progress.position = Math.min(ui.progress.position + 1, ui.progress.duration);
      renderProgress();
    }
  }, 1000);
  window.setInterval(renderCalendar, 30_000); // relative labels ("In 20 min") while visible
}

boot().catch(console.error);
