// Bluetooth (spec §23.5): Home card + Devices tab. State is polled via `bt_state` (on show, then every 10 s
// while visible). Device names are third-party text: textContent only. Where the OS can't connect devices
// (Windows) each row opens the system Bluetooth settings instead.
import { invoke } from "@tauri-apps/api/core";
import { icon } from "./icons-line";
import { showPeripherals } from "./peripherals";

type BtDevice = { id: string; name: string; kind: "audio" | "input" | "phone" | "other"; connected: boolean };
type BtState = { radio: boolean | null; devices: BtDevice[]; canConnect: boolean };

const KIND_ICON = { audio: "headphones", input: "keyboard", phone: "phone", other: "bluetooth" } as const;
const POLL_MS = 10_000;
const VIEWS = [
  { list: "bt-card-list", radio: "bt-card-radio", err: "bt-card-err", compact: true },
  { list: "bt-tab-list", radio: "bt-tab-radio", err: "bt-tab-err", compact: false },
];

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
let st: BtState | null = null;
let error = "";
let fetching = false;
const pending = new Set<string>(); // device ids (or "radio") with a request in flight

function message(text: string): HTMLElement {
  const e = document.createElement("div");
  e.className = "muted small bt-msg";
  e.textContent = text;
  return e;
}

async function act(key: string, call: () => Promise<unknown>): Promise<void> {
  pending.add(key);
  error = "";
  draw();
  try {
    await call();
  } catch (e) {
    error = String(e);
  } finally {
    pending.delete(key);
  }
  await showBluetooth();
}

function row(d: BtDevice, canConnect: boolean, compact: boolean): HTMLElement {
  const r = document.createElement("div");
  r.className = `bt-row${d.connected ? "" : " off"}`;
  const ic = document.createElement("span");
  ic.className = "bt-ic";
  ic.innerHTML = icon(KIND_ICON[d.kind] ?? "bluetooth", 16); // static SVG
  const main = document.createElement("span");
  main.className = "bt-main";
  const name = document.createElement("span");
  name.className = "ell";
  name.textContent = d.name;
  name.title = d.name;
  const status = document.createElement("span");
  status.className = "bt-status ell";
  status.textContent = d.connected ? "Connected" : "Not connected";
  main.append(name, status);
  r.append(ic, main);
  const b = document.createElement("button");
  b.className = "bt-act";
  const label = canConnect ? (d.connected ? "Disconnect" : "Connect") : "Open Bluetooth settings";
  b.title = label;
  b.ariaLabel = `${label}: ${d.name}`;
  if (compact) {
    b.innerHTML = icon(canConnect ? (d.connected ? "mute" : "bolt") : "cog", 14); // static SVG
  } else {
    b.textContent = label;
  }
  b.disabled = pending.has(d.id);
  b.onclick = () => {
    if (!canConnect) return void invoke("run_action", { id: "bluetooth" }).catch(console.error);
    void act(d.id, () => invoke(d.connected ? "bt_disconnect" : "bt_connect", { id: d.id }));
  };
  r.append(b);
  return r;
}

function draw(): void {
  for (const v of VIEWS) {
    const sw = $<HTMLButtonElement>(v.radio);
    sw.disabled = st?.radio == null || pending.has("radio");
    sw.setAttribute("aria-checked", String(st?.radio === true));
    const list = $(v.list);
    if (!st) list.replaceChildren(message(error ? "" : "Looking for devices…"));
    else if (st.radio === null) list.replaceChildren(message("No Bluetooth adapter found"));
    else if (!st.radio) list.replaceChildren(message("Bluetooth is off"));
    else if (!st.devices.length) list.replaceChildren(message("No paired devices"));
    else list.replaceChildren(...st.devices.map((d) => row(d, st!.canConnect, v.compact)));
    const e = $(v.err);
    e.hidden = !error;
    e.textContent = error;
  }
}

/** Called when the Home card or the Devices tab is visible (and by the 10 s poll). */
export async function showBluetooth(): Promise<void> {
  if (!document.getElementById("tab-devices")!.hidden) void showPeripherals(); // the USB / dongle list lives in the tab only
  if (fetching) return;
  fetching = true;
  try {
    st = await invoke<BtState>("bt_state");
  } catch (e) {
    error = String(e);
    console.error(e);
  } finally {
    fetching = false;
  }
  draw();
}

export function initBluetooth(opts: { visible: () => boolean }): void {
  for (const v of VIEWS) {
    $(v.radio).onclick = () => st?.radio != null && void act("radio", () => invoke("bt_set_radio", { on: !st!.radio }));
  }
  for (const id of ["bt-card-refresh", "bt-tab-refresh"]) $(id).onclick = () => void showBluetooth();
  draw();
  window.setInterval(() => opts.visible() && void showBluetooth(), POLL_MS);
}
