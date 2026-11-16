// Themed tooltips for every `title` in the window (island and Settings): the native tooltip ignores the theme.
// On hover / keyboard focus the `title` moves to `data-tip` (so the native one never shows) and a bubble appears
// below the element (above when there's no room). Code keeps writing `el.title = …` as before.
import "./tooltip.css";

const SHOW_MS = 450; // first tooltip
const WARM_MS = 300; // moving to another element within this shows the next one at once

let tip: HTMLDivElement | null = null;
let target: HTMLElement | null = null;
let timer = 0;
let lastHide = 0;

/** The element's tooltip text; takes over a `title` first. Icon-only controls keep it as their accessible name. */
function textOf(el: HTMLElement): string {
  const t = el.getAttribute("title");
  if (t !== null) {
    el.removeAttribute("title");
    el.dataset.tip = t;
    if (t && !el.hasAttribute("aria-label") && !el.textContent?.trim()) el.setAttribute("aria-label", t);
  }
  return el.dataset.tip ?? "";
}

function place(el: HTMLElement): void {
  const t = tip!;
  t.style.zoom = String((el as HTMLElement & { currentCSSZoom?: number }).currentCSSZoom ?? 1); // island content is zoomed by display scale
  const z = parseFloat(t.style.zoom) || 1;
  const r = el.getBoundingClientRect();
  const w = t.offsetWidth * z;
  const h = t.offsetHeight * z;
  const gap = 8 * z;
  const below = r.bottom + gap + h <= innerHeight - 4 || r.top - gap - h < 4;
  const x = Math.min(Math.max(6, r.left + r.width / 2 - w / 2), innerWidth - w - 6);
  const y = below ? r.bottom + gap : r.top - gap - h;
  t.dataset.side = below ? "below" : "above";
  // zoom scales left/top too, so divide them back
  t.style.left = `${x / z}px`;
  t.style.top = `${y / z}px`;
  t.style.setProperty("--arrow-x", `${(r.left + r.width / 2 - x) / z}px`);
}

function show(el: HTMLElement): void {
  const text = textOf(el);
  if (!text || !el.isConnected) return;
  tip ??= Object.assign(document.createElement("div"), { className: "tip", role: "tooltip", id: "tip" });
  tip.textContent = text;
  if (!tip.isConnected) document.body.append(tip);
  tip.classList.remove("on");
  place(el);
  void tip.offsetWidth; // restart the fade
  tip.classList.add("on");
  el.setAttribute("aria-describedby", "tip");
  // re-renders replace elements (session cards, lists): don't leave a bubble pointing at nothing
  clearInterval(watch);
  watch = window.setInterval(() => (target !== el || !el.isConnected) && hideTip(), 400);
}
let watch = 0;

/** Hides the tooltip now (also called by the island when the pointer leaves it: click-through stops mouse events). */
export function hideTip(): void {
  clearTimeout(timer);
  clearInterval(watch);
  if (target?.getAttribute("aria-describedby") === "tip") target.removeAttribute("aria-describedby");
  if (tip?.classList.contains("on")) lastHide = performance.now();
  tip?.classList.remove("on");
  target = null;
}

function arm(el: HTMLElement): void {
  if (el === target) return;
  hideTip();
  const text = textOf(el);
  // a tooltip that only repeats the visible label (More-menu items, labelled tabs) just covers its neighbours;
  // innerText skips hidden labels, so icon-only tabs still get theirs
  if (!text || el.innerText.replace(/\s+/g, " ").trim() === text.trim()) return;
  target = el;
  const warm = performance.now() - lastHide < WARM_MS;
  timer = window.setTimeout(() => target === el && show(el), warm ? 0 : SHOW_MS);
}

const tipped = (n: EventTarget | null) => (n instanceof Element ? n.closest<HTMLElement>("[title], [data-tip]") : null);

export function initTooltips(): void {
  document.addEventListener("pointerover", (e) => {
    const el = tipped(e.target);
    if (el) arm(el);
    else if (target) hideTip();
  });
  document.addEventListener("pointerout", (e) => {
    if (target && !target.contains(e.relatedTarget as Node)) hideTip();
  });
  document.addEventListener("focusin", (e) => {
    const el = tipped(e.target);
    if (el && el === e.target && el.matches(":focus-visible")) arm(el);
  });
  document.addEventListener("focusout", hideTip);
  document.addEventListener("pointerdown", hideTip, true);
  document.addEventListener("wheel", hideTip, { capture: true, passive: true });
  document.addEventListener("scroll", hideTip, true);
  document.addEventListener("keydown", (e) => e.key === "Escape" && hideTip());
  window.addEventListener("blur", hideTip);
}
