// Live toasts (both layouts): a short two-line card for events like a Bluetooth device connecting or the charger
// being plugged in. main.ts owns the queue and the "toast" state; this renders one. Names: textContent only.
import { icon } from "./icons-line";

export type Toast = { icon: string; title: string; text: string; value?: string; ms?: number };

export const TOAST_MS = 3500;

export function renderToast(el: HTMLElement, t: Toast): void {
  const ic = Object.assign(document.createElement("span"), { className: "toast-ic" });
  ic.innerHTML = icon(t.icon, 18); // static SVG from our icon set
  const col = Object.assign(document.createElement("span"), { className: "toast-col" });
  col.append(Object.assign(document.createElement("b"), { className: "ell", textContent: t.title }), Object.assign(document.createElement("span"), { className: "toast-text ell", textContent: t.text }));
  el.replaceChildren(ic, col, ...(t.value ? [Object.assign(document.createElement("span"), { className: "toast-value", textContent: t.value })] : []));
}
