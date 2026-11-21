// Screenshot overlay (capture.html, window "capture"): the frozen desktop from capture.rs; drag a region, mark it
// up, then Copy (Enter / Ctrl+C) or Save (Ctrl+S). Esc cancels. Nothing leaves this PC.
import "./capture.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { icon } from "../icons-line";
import { barPos, drawMark, monitorOf, rectOf, type Mark, type Monitor, type Pt, type Rect, type Tool } from "./marks";

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const img = $<HTMLCanvasElement>("shot"); // the frozen desktop, 1 canvas px = 1 screen px
const ink = $<HTMLCanvasElement>("ink");
const ctx = ink.getContext("2d")!;
const COLORS = ["#ff3b30", "#ff9f0a", "#ffd60a", "#30d158", "#0a84ff", "#ffffff"];
const TOOLS: [Tool, string, string][] = [
  ["arrow", "arrow", "Arrow (A)"], ["rect", "rect", "Box (R)"], ["pen", "pencil", "Pen (P)"],
  ["marker", "marker", "Highlighter (H)"], ["text", "text", "Text (T)"], ["blur", "blur", "Blur (B)"],
];

let scale = 1; // image px per CSS px
let sel: Rect | null = null; // the chosen region
let dragFrom: Pt | null = null; // selecting, or drawing a mark
let draft: Mark | null = null;
let tool: Tool = "arrow";
let color = COLORS[0];
const marks: Mark[] = [];
const undone: Mark[] = [];
let busy = false;
let wins: Rect[] = []; // windows on the frozen screen, front to back (CSS px)
let mons: Monitor[] = []; // the monitors (CSS px): the overlay's box can be bigger than what they show
/** How much to shrink / grow our UI on `m` so it is as big as everywhere else on that monitor: the window has
 *  its top-left monitor's scaling, which is wrong for a 100 % monitor next to a 200 % one. */
const uiScale = (m: Monitor) => (m.scale ?? window.devicePixelRatio) / (window.devicePixelRatio || 1);
const whole = (): Rect => ({ x: 0, y: 0, w: innerWidth, h: innerHeight });
/** "Whole screen" = the monitor under the cursor. */
const screenAt = (p: Pt): Rect => ({ ...monitorOf(p, mons, whole()) });
let hoverWin: Rect | null = null; // the one under the cursor before any drag: a click takes it
let queued = false;
/** Redraw at most once per display frame (pointer events come faster than the screen refreshes). */
function schedule(): void {
  if (queued) return;
  queued = true;
  requestAnimationFrame(() => {
    queued = false;
    draw();
  });
}

function button(cls: string, ic: string, title: string, onClick: () => void): HTMLButtonElement {
  const b = Object.assign(document.createElement("button"), { type: "button", className: cls });
  b.setAttribute("aria-label", title);
  b.dataset.tip = title;
  b.innerHTML = icon(ic, 15); // static SVG from our icon set
  b.onclick = (e) => (e.stopPropagation(), onClick());
  return b;
}

function buildBar(): void {
  $("tools").replaceChildren(
    ...TOOLS.map(([t, ic, title]) => {
      const b = button("tb", ic, title, () => setTool(t));
      b.dataset.tool = t;
      return b;
    }),
  );
  $("colors").replaceChildren(
    ...COLORS.map((c) => {
      const b = Object.assign(document.createElement("button"), { type: "button", className: "sw" });
      b.setAttribute("aria-label", `Colour ${c}`);
      b.style.setProperty("--c", c);
      b.dataset.color = c;
      b.onclick = (e) => (e.stopPropagation(), setColor(c));
      return b;
    }),
  );
  $("edits").replaceChildren(button("tb", "undo", "Undo (Ctrl+Z)", undo), button("tb", "redo", "Redo (Ctrl+Y)", redo), button("tb", "crop", "Select again", reselect));
  const copy = button("tb primary", "copy", "Copy (Enter)", () => finish(true, false));
  const save = button("tb", "download", "Save to your screenshots folder (Ctrl+S)", () => finish(true, true));
  $("outs").replaceChildren(copy, save, button("tb", "close", "Cancel (Esc)", cancel));
  setTool(tool);
  setColor(color);
}

// ----- tooltips under the tools (the overlay window gets no native ones) -----
function wireTips(): void {
  const tip = $("tip");
  const bar = $("bar");
  bar.addEventListener("pointerover", (e) => {
    const b = (e.target as HTMLElement).closest<HTMLElement>("[data-tip]");
    if (!b) return;
    tip.textContent = b.dataset.tip!;
    tip.hidden = false;
    tip.style.left = `${b.offsetLeft + b.offsetWidth / 2}px`;
  });
  bar.addEventListener("pointerout", (e) => {
    if (!(e.relatedTarget as HTMLElement | null)?.closest?.("[data-tip]")) tip.hidden = true;
  });
}

function setTool(t: Tool): void {
  tool = t;
  document.querySelectorAll<HTMLElement>("[data-tool]").forEach((b) => b.classList.toggle("on", b.dataset.tool === t));
  ink.style.cursor = t === "text" ? "text" : "crosshair";
}

function setColor(c: string): void {
  color = c;
  document.querySelectorAll<HTMLElement>("[data-color]").forEach((b) => b.classList.toggle("on", b.dataset.color === c));
}

function undo(): void {
  const m = marks.pop();
  if (m) undone.push(m), draw();
}

function redo(): void {
  const m = undone.pop();
  if (m) marks.push(m), draw();
}

function reselect(): void {
  sel = null;
  marks.length = undone.length = 0;
  $("bar").hidden = true;
  $("hint").hidden = false;
  draw();
}

const cancel = () => {
  $("mag").hidden = true;
  void invoke("capture_cancel");
};

/** Dim everything but the selection, outline it, draw the marks clipped to it. */
function draw(): void {
  const dpr = window.devicePixelRatio || 1;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, innerWidth, innerHeight);
  const r = sel ?? (dragFrom && draft === null ? rectOf(dragFrom, last) : hoverWin);
  drawMag();
  ctx.fillStyle = "rgba(0, 0, 0, 0.45)";
  ctx.beginPath();
  ctx.rect(0, 0, innerWidth, innerHeight);
  if (r) ctx.rect(r.x + r.w, r.y, -r.w, r.h); // opposite winding: a hole
  ctx.fill("evenodd");
  if (!r) return;
  ctx.save();
  ctx.beginPath();
  ctx.rect(r.x, r.y, r.w, r.h);
  ctx.clip();
  for (const m of draft ? [...marks, draft] : marks) drawMark(ctx, m, img, scale);
  ctx.restore();
  ctx.strokeStyle = "#0a84ff";
  ctx.lineWidth = 1.5;
  ctx.strokeRect(r.x + 0.75, r.y + 0.75, r.w - 1.5, r.h - 1.5);
  const size = $("size");
  size.hidden = false;
  size.textContent = `${Math.round(r.w * scale)} × ${Math.round(r.h * scale)}`;
  size.style.left = `${r.x}px`;
  size.style.top = `${r.y >= 26 ? r.y - 24 : r.y + 4}px`;
}

let last: Pt = { x: 0, y: 0 };
const at = (e: PointerEvent): Pt => ({ x: e.clientX, y: e.clientY });
const inside = (p: Pt) => !!sel && p.x >= sel.x && p.x <= sel.x + sel.w && p.y >= sel.y && p.y <= sel.y + sel.h;

function placeBar(): void {
  if (!sel) return;
  const bar = $("bar");
  bar.hidden = false;
  $("hint").hidden = true;
  const mon = monitorOf(sel, mons, whole());
  const k = uiScale(mon);
  bar.style.transform = `scale(${k})`; // origin top-left: left/top below stay in window px
  const p = barPos(sel, { w: bar.offsetWidth * k, h: bar.offsetHeight * k }, mon);
  bar.style.left = `${p.x}px`;
  bar.style.top = `${p.y}px`;
  // tooltips go under the bar, or above it when the bar sits at the bottom of the screen
  bar.classList.toggle("tips-up", p.y + (bar.offsetHeight + 34) * k > mon.y + mon.h);
  $("size").style.transform = $("mag").style.transform = $("hint").style.transform = `scale(${k})`;
}

ink.onpointerdown = (e) => {
  if (e.button !== 0 || busy) return;
  const p = at(e);
  if (!$("text-in").hidden) return commitText();
  if (!sel) {
    dragFrom = last = p;
    ink.setPointerCapture(e.pointerId);
    return;
  }
  if (!inside(p)) return;
  if (tool === "text") return startText(p);
  dragFrom = last = p;
  undone.length = 0;
  draft = tool === "pen" || tool === "marker" ? { kind: tool, color, pts: [p] } : { kind: tool, color, a: p, b: p };
  ink.setPointerCapture(e.pointerId);
};

ink.onpointermove = (e) => {
  last = at(e);
  if (!dragFrom) {
    if (!sel) {
      hoverWin = wins.find((w) => last.x >= w.x && last.x < w.x + w.w && last.y >= w.y && last.y < w.y + w.h) ?? null;
      schedule();
    }
    return;
  }
  if (draft && "pts" in draft) draft.pts.push(last);
  else if (draft && "b" in draft) draft.b = last;
  schedule();
};
ink.onpointerleave = () => ($("mag").hidden = true);

ink.onpointerup = () => {
  if (!dragFrom) return;
  if (draft) {
    marks.push(draft);
    draft = null;
  } else {
    const r = rectOf(dragFrom, last);
    // a click (no drag) takes the window under it, or the whole screen
    sel = r.w >= 4 && r.h >= 4 ? r : (hoverWin ?? screenAt(last));
    hoverWin = null;
    placeBar();
  }
  dragFrom = null;
  draw();
};

ink.oncontextmenu = (e) => (e.preventDefault(), sel ? reselect() : cancel());

// ----- magnifier: the pixels around the cursor, 9x, with a crosshair and the position (precise edges) -----
const ZOOM = 9;
function drawMag(): void {
  const mag = $<HTMLCanvasElement>("mag");
  const choosing = !sel && !busy;
  mag.hidden = !choosing;
  if (!choosing) return;
  const m = mag.getContext("2d")!;
  const size = mag.width, cells = Math.ceil(size / ZOOM) | 1; // odd: the cursor pixel sits in the middle
  const cx = Math.floor(last.x * scale), cy = Math.floor(last.y * scale);
  m.imageSmoothingEnabled = false;
  m.fillStyle = "#000";
  m.fillRect(0, 0, size, size);
  const off = (size - cells * ZOOM) / 2;
  m.drawImage(img, cx - (cells >> 1), cy - (cells >> 1), cells, cells, off, off, cells * ZOOM, cells * ZOOM);
  m.strokeStyle = "rgba(10, 132, 255, 0.9)";
  m.lineWidth = 1;
  const mid = off + (cells >> 1) * ZOOM;
  m.strokeRect(mid + 0.5, mid + 0.5, ZOOM - 1, ZOOM - 1);
  m.fillStyle = "rgba(10, 132, 255, 0.35)";
  m.fillRect(0, mid, mid, ZOOM);
  m.fillRect(mid + ZOOM, mid, size - mid - ZOOM, ZOOM);
  m.fillRect(mid, 0, ZOOM, mid);
  m.fillRect(mid, mid + ZOOM, ZOOM, size - mid - ZOOM);
  m.fillStyle = "rgba(0, 0, 0, 0.7)";
  m.fillRect(0, size - 18, size, 18);
  m.fillStyle = "#fff";
  m.font = "600 11px 'Segoe UI', system-ui, sans-serif";
  m.textAlign = "center";
  m.fillText(`${cx}, ${cy}`, size / 2, size - 5);
  // beside the cursor, flipped near the right / bottom edges
  const css = mag.offsetWidth || 130;
  mag.style.left = `${last.x + 24 + css > innerWidth ? last.x - 24 - css : last.x + 24}px`;
  mag.style.top = `${last.y + 24 + css > innerHeight ? last.y - 24 - css : last.y + 24}px`;
}

// ----- text -----
function startText(p: Pt): void {
  const t = $<HTMLInputElement>("text-in");
  t.hidden = false;
  t.value = "";
  t.style.left = `${p.x}px`;
  t.style.top = `${p.y - 4}px`;
  t.style.color = color;
  t.dataset.x = String(p.x);
  t.dataset.y = String(p.y);
  t.focus();
}

function commitText(): void {
  const t = $<HTMLInputElement>("text-in");
  if (t.hidden) return;
  const text = t.value.trim();
  t.hidden = true;
  if (text) marks.push({ kind: "text", color: t.style.color || color, at: { x: Number(t.dataset.x), y: Number(t.dataset.y) }, text });
  draw();
}

$<HTMLInputElement>("text-in").onkeydown = (e) => {
  e.stopPropagation();
  if (e.key === "Enter") commitText();
  if (e.key === "Escape") ($("text-in").hidden = true);
};

// ----- output -----
async function render(): Promise<Uint8Array> {
  const r = sel!;
  const out = new OffscreenCanvas(Math.max(1, Math.round(r.w * scale)), Math.max(1, Math.round(r.h * scale)));
  const o = out.getContext("2d")!;
  o.drawImage(img, r.x * scale, r.y * scale, r.w * scale, r.h * scale, 0, 0, out.width, out.height);
  o.scale(scale, scale);
  o.translate(-r.x, -r.y);
  for (const m of marks) drawMark(o, m, img, scale);
  return new Uint8Array(await (await out.convertToBlob({ type: "image/png" })).arrayBuffer());
}

async function finish(copy: boolean, save: boolean): Promise<void> {
  if (!sel || busy) return;
  commitText();
  busy = true;
  document.body.classList.add("busy");
  try {
    await invoke("capture_finish", await render(), { headers: { "x-copy": copy ? "1" : "0", "x-save": save ? "1" : "0" } });
  } catch (e) {
    busy = false;
    document.body.classList.remove("busy");
    $("hint").hidden = false;
    $("hint").textContent = String(e);
  }
}

window.onkeydown = (e) => {
  const k = e.key.toLowerCase();
  if (e.key === "Escape") return cancel();
  if (e.key === "Enter") return sel ? void finish(true, false) : ((sel = screenAt(last)), placeBar(), draw());
  if (!sel) return;
  if (e.ctrlKey && k === "z") return e.shiftKey ? redo() : undo();
  if (e.ctrlKey && k === "y") return redo();
  if (e.ctrlKey && k === "c") return void finish(true, false);
  if (e.ctrlKey && k === "s") return e.preventDefault(), void finish(true, true);
  const t = TOOLS.find(([, , title]) => title.endsWith(`(${e.key.toUpperCase()})`));
  if (t && !e.ctrlKey) setTool(t[0]);
};

/** Windows and monitors as Rust sent them (image pixels); `fit` turns them into CSS px with the current scale. */
let rawWins: [number, number, number, number][] = [];
let rawMons: [number, number, number, number, number][] = [];

function fit(): void {
  const dpr = window.devicePixelRatio || 1;
  // a window that was resized while hidden can still report a 0 or stale size until it is shown: fit again then
  if (innerWidth < 2 || innerHeight < 2) return;
  ink.width = Math.round(innerWidth * dpr);
  ink.height = Math.round(innerHeight * dpr);
  scale = img.width / innerWidth || dpr;
  const css = ([x, y, w, h]: [number, number, number, number]): Rect => ({ x: x / scale, y: y / scale, w: w / scale, h: h / scale });
  wins = rawWins.map(css);
  mons = rawMons.map((m) => ({ ...css([m[0], m[1], m[2], m[3]]), scale: m[4] / 100 }));
  const k0 = uiScale(monitorOf(last, mons, whole())); // until a selection exists: the monitor under the cursor
  $("size").style.transform = $("mag").style.transform = $("hint").style.transform = `scale(${k0})`;
  draw();
  if (sel) placeBar();
}
window.onresize = fit;

/** A new capture: the raw pixels straight onto the canvas (no image decoding), then a clean slate. */
async function load(): Promise<void> {
  const buf = await invoke<ArrayBuffer>("capture_image");
  const head = new DataView(buf, 0, 8);
  const w = head.getUint32(0, true), h = head.getUint32(4, true);
  img.width = w;
  img.height = h;
  img.getContext("2d")!.putImageData(new ImageData(new Uint8ClampedArray(buf, 8, w * h * 4), w, h), 0, 0);
  sel = null;
  dragFrom = draft = hoverWin = null;
  marks.length = undone.length = 0;
  busy = false;
  document.body.classList.remove("busy");
  $("bar").hidden = true;
  $("hint").hidden = false;
  $("hint").innerHTML = "Drag to capture an area &nbsp;·&nbsp; <b>Enter</b> whole screen &nbsp;·&nbsp; <b>Esc</b> cancel"; // static text
  $("text-in").hidden = true;
  setTool("arrow");
  rawWins = await invoke<[number, number, number, number][]>("capture_windows").catch(() => []);
  rawMons = await invoke<[number, number, number, number, number][]>("capture_monitors").catch(() => []);
  fit();
  await invoke("capture_ready"); // Rust shows the window only now: no white flash
  // the layout size is only reliable once the window is visible: measure again then (and once more after it settles)
  requestAnimationFrame(fit);
  window.setTimeout(fit, 150);
}

async function boot(): Promise<void> {
  buildBar();
  wireTips();
  await listen("capture-shot", () => load().catch((e) => (console.error(e), cancel())));
  await load().catch(() => {}); // opened for a capture already in progress (first use): show it; else wait
}

void boot();
