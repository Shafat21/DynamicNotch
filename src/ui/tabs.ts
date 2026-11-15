// Tab bar of the open island (spec §22.1): visible tabs from the saved layout; whatever doesn't fit
// next to the quick capsule goes into "More ▾".
import { icon } from "../icons-line";
import type { LayoutItem } from "./theme";

const TABS: Record<string, [label: string, icon: string]> = {
  home: ["Home", "home"], music: ["Music", "music"], calendar: ["Calendar", "calendar"], pal: ["Pal", "pal"], timer: ["Timer", "timer"],
  launcher: ["Launcher", "launcher"], weather: ["Weather", "partly"], system: ["System", "cpu"], tasks: ["Tasks", "tasks"], month: ["Month", "month"],
};
// Shown in More ▾ by default when everything would fit (spec §22.1); the rest only when space runs out.
const OVERFLOW_FIRST = new Set(["weather", "system", "tasks", "month"]);
const MIN_CLOCK = 70; // below this the date/time is hidden rather than reduced to "…"

const $ = (id: string) => document.getElementById(id)!;
let order: string[] = []; // visible tab ids in layout order
let buttons: HTMLButtonElement[] = [];
let more: HTMLButtonElement | null = null;
let pick: (id: string) => void = () => {};
let active = "home";

export const visibleTabs = () => order;

function tabButton(id: string, label: string, iconName: string, badge?: string): HTMLButtonElement {
  const b = document.createElement("button");
  b.title = label;
  const ic = document.createElement("span");
  ic.className = "tab-ic";
  if (badge) ic.dataset.badge = badge;
  ic.innerHTML = icon(iconName, 18); // static SVG
  const text = document.createElement("span");
  text.className = "tab-label";
  text.textContent = label;
  b.append(ic, text);
  b.dataset.tab = id;
  return b;
}

export const closeMore = () => ($("more-menu").hidden = true);

export function renderTabs(items: LayoutItem[], current: string): void {
  order = items.filter((t) => t.visible && TABS[t.id]).map((t) => t.id);
  buttons = order.map((id) => {
    const b = tabButton(id, TABS[id][0], TABS[id][1], id === "timer" ? "focus" : id);
    b.onclick = () => pick(id);
    return b;
  });
  more = tabButton("more", "More ▾", "dots");
  more.className = "tab more";
  more.onclick = (e) => {
    e.stopPropagation();
    const menu = $("more-menu");
    menu.style.left = `${more!.offsetLeft}px`;
    menu.hidden = !menu.hidden;
  };
  closeMore();
  active = current;
  fitTabs();
}

/** Bar gets as many tabs as fit beside the quick capsule; the rest (default overflow ones first) go to More ▾. */
export function fitTabs(): void {
  if (!more) return;
  const row = $("tabs");
  const clock = $("top-clock");
  const avail = row.closest<HTMLElement>(".topbar")!.clientWidth - document.querySelector<HTMLElement>(".topbar .quick")!.offsetWidth - 8;
  // the default overflow tabs stay in More while the full set would not fit anyway (old 7-slot rule)
  const bar = order.length > 7 ? order.filter((id) => !OVERFLOW_FIRST.has(id)) : [...order];
  const place = () => {
    const inBar = new Set(bar);
    row.replaceChildren(...buttons.filter((b) => inBar.has(b.dataset.tab!)), ...(bar.length < order.length ? [more!] : []));
    $("more-menu").replaceChildren(...buttons.filter((b) => !inBar.has(b.dataset.tab!)));
    for (const b of buttons) b.className = inBar.has(b.dataset.tab!) ? "tab" : "more-item";
  };
  place();
  if (avail > 0) {
    while (bar.length > 1 && row.scrollWidth > avail) {
      bar.pop();
      place();
    }
    clock.hidden = false;
    clock.hidden = clock.clientWidth < MIN_CLOCK;
  }
  setActiveTab(active);
}

export function setActiveTab(current: string): void {
  active = current;
  for (const b of buttons) b.classList.toggle("on", b.dataset.tab === current);
  more?.classList.toggle("on", $("more-menu").querySelector(".on") !== null);
}

/** Clicks pick a tab; the wheel on the bar steps through all visible tabs. */
export function initTabs(onPick: (id: string) => void, current: () => string): void {
  pick = (id) => {
    closeMore();
    onPick(id);
  };
  let lastWheel = 0;
  $("tabbar").addEventListener(
    "wheel",
    (e) => {
      if (!order.length || e.timeStamp - lastWheel < 250) return; // trackpads fire many events per gesture
      lastWheel = e.timeStamp;
      const i = order.indexOf(current());
      pick(order[(i + (e.deltaY > 0 ? 1 : order.length - 1)) % order.length]);
    },
    { passive: true },
  );
  document.addEventListener("click", (e) => {
    if (!$("more-menu").contains(e.target as Node)) closeMore();
  });
  // re-fit when the open island appears or its width changes (open size, display scale)
  new ResizeObserver(() => fitTabs()).observe(document.querySelector<HTMLElement>(".topbar")!);
}
