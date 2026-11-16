// The compact notch (appearance.view = "compact"): header (ring, date, time), one card at a time, ‹ › arrows,
// page dots, the wheel and ←/→ to flip. Cards render themselves from main.ts's data (render functions).
import "./compact.css";
import { CARD_NAMES, CARDS, step, type CardId } from "./pager";

const KEY = "compact-card";
const NS = "http://www.w3.org/2000/svg";

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls: string, text?: string): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

/** A small progress ring (0..1); hidden when `fraction` is null. */
export function ring(size: number): { el: SVGSVGElement; set(fraction: number | null): void } {
  const svg = document.createElementNS(NS, "svg");
  svg.setAttribute("viewBox", "0 0 20 20");
  svg.setAttribute("width", String(size));
  svg.setAttribute("height", String(size));
  svg.setAttribute("aria-hidden", "true");
  svg.classList.add("cmp-ring");
  const arc = document.createElementNS(NS, "circle");
  for (const [cls, c] of [["track", document.createElementNS(NS, "circle")], ["arc", arc]] as const) {
    for (const [k, v] of Object.entries({ cx: 10, cy: 10, r: 8, pathLength: 100, class: cls })) c.setAttribute(k, String(v));
    svg.append(c);
  }
  return {
    el: svg,
    set(f) {
      svg.style.visibility = f === null ? "hidden" : "";
      arc.setAttribute("stroke-dasharray", `${Math.max(0, Math.min(1, f ?? 0)) * 100} 100`);
    },
  };
}

export function initCompact(root: HTMLElement, cards: Record<CardId, (el: HTMLElement) => void>, opts: { ring: () => number | null }): { show(): void; refresh(): void } {
  let i = Math.max(0, CARDS.indexOf(localStorage.getItem(KEY) as CardId));
  const r = ring(18);
  const date = el("span", "cmp-date");
  const time = el("b", "cmp-time");
  const head = el("div", "cmp-head");
  head.append(r.el, date, time);
  const card = el("div", "cmp-card");
  card.setAttribute("role", "tabpanel");
  const arrow = (dir: 1 | -1) => {
    const b = el("button", `cmp-arrow ${dir < 0 ? "prev" : "next"}`, dir < 0 ? "‹" : "›");
    b.type = "button";
    b.setAttribute("aria-label", dir < 0 ? "Previous card" : "Next card");
    b.onclick = (e) => (e.stopPropagation(), go(dir));
    return b;
  };
  const body = el("div", "cmp-body");
  body.append(arrow(-1), card, arrow(1));
  const dots = el("div", "cmp-dots");
  dots.setAttribute("role", "tablist");
  const dotButtons = CARDS.map((id, n) => {
    const d = el("button", "cmp-dot");
    d.type = "button";
    d.setAttribute("role", "tab");
    d.setAttribute("aria-label", CARD_NAMES[id]);
    d.onclick = (e) => (e.stopPropagation(), (i = n), render());
    return d;
  });
  dots.append(...dotButtons);
  root.replaceChildren(head, body, dots);

  const clock = () => {
    const now = new Date();
    date.textContent = now.toLocaleDateString(undefined, { weekday: "short", day: "numeric", month: "short" });
    time.textContent = now.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
    r.set(opts.ring());
  };
  const render = () => {
    localStorage.setItem(KEY, CARDS[i]);
    card.dataset.card = CARDS[i];
    card.setAttribute("aria-label", CARD_NAMES[CARDS[i]]);
    dotButtons.forEach((d, n) => d.setAttribute("aria-selected", String(n === i)));
    cards[CARDS[i]](card);
    clock();
  };
  const go = (dir: 1 | -1) => {
    i = step(i, CARDS.length, dir);
    card.classList.remove("in");
    void card.offsetWidth; // restart the slide-in
    card.classList.add("in");
    card.dataset.dir = String(dir);
    render();
  };

  let lastWheel = 0;
  root.addEventListener("wheel", (e) => {
    if (e.timeStamp - lastWheel < 300 || Math.abs(e.deltaY) + Math.abs(e.deltaX) < 4) return;
    lastWheel = e.timeStamp;
    go((e.deltaY || e.deltaX) > 0 ? 1 : -1);
  }, { passive: true });
  document.addEventListener("keydown", (e) => {
    if (root.hidden || e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
    if (e.key === "ArrowRight") go(1);
    else if (e.key === "ArrowLeft") go(-1);
  });
  window.setInterval(() => !root.hidden && clock(), 15_000);

  return { show: render, refresh: () => !root.hidden && render() };
}
