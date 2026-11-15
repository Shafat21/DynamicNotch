// Sound & brightness (spec §23.4): Home card + Sound tab. Volume/devices come from the sticky `sound`
// event (Rust polls every second); brightness is fetched when the card or tab is shown (DDC is slow).
// Device names are third-party text: textContent / option.text only.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { icon } from "./icons-line";

type SoundState = { volume: number; muted: boolean; deviceId: string };
type AudioDevice = { id: string; name: string; default: boolean };
type SoundSnap = { state: SoundState | null; devices: AudioDevice[] };
type BrightDisplay = { id: string; name: string; percent: number; kind: "internal" | "external" };

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
let snap: SoundSnap = { state: null, devices: [] };
let displays: BrightDisplay[] = [];
let fetching = false;
let dragging: HTMLInputElement | null = null; // incoming values don't move a slider the user is holding
let onSwitchFailed = () => {};

/** Accent fill left of the thumb; `hold` keeps the user's value (their write is still on its way). */
function paint(slider: HTMLInputElement, value: number, label: HTMLElement, hold = false): void {
  if (dragging !== slider && !hold) slider.value = String(value);
  slider.style.setProperty("--fill", `${slider.value}%`);
  label.textContent = slider.value;
}

function holdWhileDragging(slider: HTMLInputElement): void {
  slider.addEventListener("pointerdown", () => (dragging = slider));
  slider.addEventListener("pointerup", () => (dragging = null));
  slider.addEventListener("lostpointercapture", () => (dragging = null));
}

/** Sends the newest value only: one request in flight, later inputs replace the queued one.
 *  `idle(ok, v)` runs once the queue is empty, with the outcome of the last send. `busy()`: still sending. */
function latest(send: (v: number) => Promise<unknown>, idle: (ok: boolean, v: number) => void): ((v: number) => void) & { busy: () => boolean } {
  let busy = false;
  let next: number | null = null;
  const go = async (v: number) => {
    busy = true;
    const ok = await send(v).then(
      () => true,
      (e) => (console.error(e), false),
    );
    busy = false;
    if (next === null) return idle(ok, v);
    const n = next;
    next = null;
    void go(n);
  };
  const push = (v: number) => (busy ? (next = v) : void go(v));
  return Object.assign(push, { busy: () => busy });
}

let volSource: HTMLInputElement | null = null; // the volume slider whose value is on its way to the backend
const setVolume = latest(
  (v) => invoke("set_volume", { value: v / 100 }),
  (ok, v) => {
    volSource = null;
    if (ok) {
      if (snap.state) snap.state.volume = v / 100; // the next `sound` event confirms it
      drawSound();
    } else {
      invoke<SoundState>("sound_state").then(
        (s) => ((snap.state = s), drawSound()),
        () => drawSound(),
      );
    }
  },
);
const brightTimers = new Map<string, number>();
/** 150 ms debounce per display: DDC writes are slow (the backend also coalesces). */
function setBrightness(id: string, percent: number): void {
  window.clearTimeout(brightTimers.get(id));
  brightTimers.set(id, window.setTimeout(() => invoke("set_brightness", { id, percent }).catch(console.error), 150));
}

// ---------- volume + output device (card and tab share these) ----------
const VOLUMES = [
  ["snd-mute", "snd-vol", "snd-vol-val"],
  ["snd-tab-mute", "snd-tab-vol", "snd-tab-vol-val"],
] as const;
const DEVICES = ["snd-device", "snd-tab-device"];

function drawSound(): void {
  const s = snap.state;
  for (const [m, v, l] of VOLUMES) {
    const mute = $<HTMLButtonElement>(m);
    const slider = $<HTMLInputElement>(v);
    mute.disabled = slider.disabled = !s;
    mute.innerHTML = icon(s?.muted ? "mute" : "volume", 16); // static SVG
    mute.title = s?.muted ? "Unmute" : "Mute";
    mute.setAttribute("aria-pressed", String(!!s?.muted));
    slider.classList.toggle("muted", !!s?.muted);
    paint(slider, Math.round((s?.volume ?? 0) * 100), $(l), slider === volSource && setVolume.busy()); // an event older than our write must not snap it back
    if (!s) $(l).textContent = "--";
  }
  for (const id of DEVICES) {
    const sel = $<HTMLSelectElement>(id);
    sel.replaceChildren(
      ...snap.devices.map((d) => {
        const o = document.createElement("option");
        o.value = d.id;
        o.text = d.name;
        return o;
      }),
    );
    const current = snap.devices.find((d) => d.default)?.id ?? s?.deviceId ?? "";
    sel.value = current;
    sel.disabled = snap.devices.length < 2;
    sel.title = snap.devices.find((d) => d.id === current)?.name ?? "No output device";
  }
}

async function switchDevice(sel: HTMLSelectElement): Promise<void> {
  const err = $("snd-err");
  try {
    await invoke("set_audio_device", { id: sel.value });
    err.hidden = true;
  } catch (e) {
    console.error(e);
    drawSound(); // back to the real default
    err.hidden = false;
    onSwitchFailed();
  }
}

// ---------- brightness ----------
function brightRow(d: BrightDisplay): HTMLElement {
  const row = document.createElement("div");
  row.className = "snd-row";
  const ic = document.createElement("span");
  ic.className = "snd-ic";
  ic.innerHTML = icon("sunny", 16); // static SVG
  const name = document.createElement("span");
  name.className = "snd-name ell";
  name.textContent = d.name;
  name.title = d.name;
  const slider = sliderFor(d);
  const val = document.createElement("span");
  val.className = "snd-val";
  holdWhileDragging(slider);
  wireBright(slider, val, d);
  row.append(ic, name, slider, val);
  return row;
}

function sliderFor(d: BrightDisplay): HTMLInputElement {
  const s = document.createElement("input");
  s.type = "range";
  s.className = "slider";
  s.min = "0";
  s.max = "100";
  s.ariaLabel = `Brightness: ${d.name}`;
  return s;
}

function wireBright(slider: HTMLInputElement, val: HTMLElement, d: BrightDisplay): void {
  slider.oninput = () => {
    paint(slider, Number(slider.value), val);
    d.percent = Number(slider.value);
    setBrightness(d.id, d.percent);
  };
  paint(slider, d.percent, val);
}

function drawBrightness(): void {
  if (dragging) return; // never rebuild under the pointer; the next show refreshes
  // card: the built-in panel first, else the first monitor
  const first = displays.find((d) => d.kind === "internal") ?? displays[0];
  $("snd-bright-row").hidden = !first;
  if (first) {
    const slider = $<HTMLInputElement>("snd-bright");
    slider.ariaLabel = `Brightness: ${first.name}`;
    wireBright(slider, $("snd-bright-val"), first);
  }
  const list = $("snd-displays");
  list.replaceChildren(...displays.map(brightRow));
  if (!displays.length) {
    const e = document.createElement("div");
    e.className = "muted small";
    e.textContent = fetching ? "Looking for displays…" : "No display here supports brightness control";
    list.append(e);
  }
}

/** Called when the Home card or the Sound tab becomes visible. */
export async function showSound(): Promise<void> {
  if (fetching || dragging) return;
  fetching = true;
  if (!displays.length) drawBrightness();
  try {
    displays = await invoke<BrightDisplay[]>("brightness_displays");
  } catch (e) {
    console.error(e);
  } finally {
    fetching = false;
  }
  drawBrightness();
}

export function initSound(opts: { switchFailed: () => void }): void {
  onSwitchFailed = opts.switchFailed;
  for (const [m, v, l] of VOLUMES) {
    const slider = $<HTMLInputElement>(v);
    holdWhileDragging(slider);
    slider.oninput = () => {
      paint(slider, Number(slider.value), $(l));
      volSource = slider;
      setVolume(Number(slider.value));
    };
    $(m).onclick = () => {
      if (snap.state) invoke("set_mute", { muted: !snap.state.muted }).catch(console.error);
    };
  }
  for (const id of DEVICES) {
    const sel = $<HTMLSelectElement>(id);
    sel.onchange = () => void switchDevice(sel);
  }
  holdWhileDragging($<HTMLInputElement>("snd-bright"));
  drawSound();
  void listen<SoundSnap>("sound", (e) => {
    snap = e.payload;
    drawSound();
  });
  void invoke<Record<string, unknown>>("get_snapshot").then((s) => {
    if (s.sound) {
      snap = s.sound as SoundSnap;
      drawSound();
    }
  });
}
