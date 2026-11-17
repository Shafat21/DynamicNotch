// Devices tab, "Wired & wireless": USB and 2.4 GHz dongle devices from `peripherals` (Rust caches the scan for
// 20 s). Battery where the device reports it. Names come from the devices themselves: textContent only.
import { invoke } from "@tauri-apps/api/core";
import { icon } from "./icons-line";

type Peripheral = { id: string; name: string; kind: string; via: "wireless" | "usb"; battery: number | null; charging: boolean };

const KIND_ICON: Record<string, string> = { mouse: "mouse", keyboard: "keyboard", headset: "headphones", audio: "headphones", gamepad: "gamepad" };
let busy = false;

function row(p: Peripheral): HTMLElement {
  const r = Object.assign(document.createElement("div"), { className: "bt-row per-row" });
  const ic = Object.assign(document.createElement("span"), { className: "bt-ic" });
  ic.innerHTML = icon(KIND_ICON[p.kind] ?? "usb", 16); // static SVG
  const main = Object.assign(document.createElement("span"), { className: "bt-main" });
  main.append(
    Object.assign(document.createElement("span"), { className: "ell", textContent: p.name, title: p.name }),
    Object.assign(document.createElement("span"), { className: "bt-status ell", textContent: p.via === "wireless" ? "Wireless" : "USB" }),
  );
  r.append(ic, main);
  if (p.battery !== null) {
    const b = Object.assign(document.createElement("span"), { className: `per-batt${p.battery <= 20 && !p.charging ? " low" : ""}` });
    b.style.setProperty("--pct", `${p.battery}%`);
    b.title = p.charging ? `Charging, ${p.battery}%` : `${p.battery}% battery`;
    if (p.charging) b.innerHTML = icon("bolt", 11); // static SVG
    b.append(`${p.battery}%`);
    r.append(b);
  }
  return r;
}

/** Called with the Bluetooth refresh (tab shown, every 10 s while visible, and the refresh button). */
export async function showPeripherals(): Promise<void> {
  if (busy) return;
  busy = true;
  try {
    const list = await invoke<Peripheral[]>("peripherals");
    const el = document.getElementById("per-list")!;
    el.replaceChildren(...(list.length ? list.map(row) : [Object.assign(document.createElement("div"), { className: "muted small bt-msg", textContent: "No USB or wireless devices found" })]));
  } catch (e) {
    console.error(e);
  } finally {
    busy = false;
  }
}
