// Small DOM builders for the Settings pages. Text always goes in as text nodes, never HTML.
import { icon } from "./icons";
import "./select.css";

type Kid = Node | string;

export function h<K extends keyof HTMLElementTagNameMap>(tag: K, props: Partial<HTMLElementTagNameMap[K]> = {}, ...kids: Kid[]): HTMLElementTagNameMap[K] {
  const el = Object.assign(document.createElement(tag), props);
  el.append(...kids);
  return el;
}

let statusTimer = 0;
export function report(msg: string): void {
  const s = document.getElementById("status")!;
  s.textContent = msg;
  s.classList.add("show");
  clearTimeout(statusTimer);
  statusTimer = window.setTimeout(() => s.classList.remove("show"), 3500);
}

/** Page container with its title and an optional "Reset" button. */
export function page(title: string, onReset?: () => void): HTMLElement {
  const head = h("header", { className: "page-head" }, h("h1", {}, title));
  if (onReset) head.append(h("button", { className: "btn", type: "button", onclick: onReset }, "Reset"));
  return h("div", { className: "page" }, head);
}

export function section(title: string, ...kids: Kid[]): HTMLElement {
  return h("section", { className: "section" }, h("h2", {}, title), ...kids);
}

/** Label + description on the left, control on the right. */
export function row(label: string, desc: string, control: Node): HTMLElement {
  return h("div", { className: "row" }, h("div", { className: "row-text" }, h("div", { className: "row-label" }, label), h("div", { className: "row-desc" }, desc)), control);
}

export function segmented(label: string, options: [value: string, text: string][], value: string, onChange: (v: string) => void): HTMLElement {
  const group = h("div", { className: "seg", role: "group", ariaLabel: label });
  for (const [v, text] of options) {
    const b = h("button", { type: "button", ariaPressed: String(v === value) }, text);
    b.onclick = () => {
      group.querySelectorAll("button").forEach((x) => (x.ariaPressed = String(x === b)));
      onChange(v);
    };
    group.append(b);
  }
  return group;
}

export function slider(label: string, o: { min: number; max: number; step: number; value: number; fmt: (v: number) => string }, onInput: (v: number) => void): HTMLElement {
  const out = h("output", {}, o.fmt(o.value));
  const input = h("input", { type: "range", min: String(o.min), max: String(o.max), step: String(o.step), value: String(o.value), ariaLabel: label });
  input.oninput = () => {
    out.textContent = o.fmt(input.valueAsNumber);
    onInput(input.valueAsNumber);
  };
  return h("div", { className: "slider" }, input, out);
}

export function toggle(label: string, checked: boolean, onChange: (on: boolean) => void): HTMLInputElement {
  const t = h("input", { type: "checkbox", className: "switch", role: "switch", checked, ariaLabel: label });
  t.onchange = () => onChange(t.checked);
  return t;
}

/** `[value, text, detail?, tag?]`: detail is muted text after the label (e.g. a resolution), tag a small pill. */
export type Choice = [value: string, text: string, detail?: string, tag?: string];
export type Select = HTMLElement & { value: string; setOptions: (options: Choice[], value?: string) => void };

let openSelect: (() => void) | null = null; // closes the dropdown that is open, if any

/** Themed dropdown (the native <select> popup ignores the theme). Button + listbox, keyboard as a native select:
 *  Enter / Space / ↑ ↓ open, ↑ ↓ Home End move, Enter picks, Esc / Tab / click outside close, letters jump. */
export function select(label: string, options: Choice[], value: string, onChange: (v: string) => void): Select {
  const id = `sel-${Math.random().toString(36).slice(2, 8)}`;
  const text = h("span", { className: "sel-text" });
  const btn = h("button", { type: "button", className: "sel-btn", role: "combobox", ariaLabel: label, ariaHasPopup: "listbox", ariaExpanded: "false" }, text);
  btn.setAttribute("aria-controls", id);
  const chev = icon("chevron", 16);
  chev.classList.add("sel-chev");
  btn.append(chev);
  const list = h("ul", { className: "sel-list", id, role: "listbox", tabIndex: -1, hidden: true });
  list.setAttribute("aria-label", label);
  const root = h("div", { className: "sel" }, btn, list) as unknown as Select;
  let opts: Choice[] = [];
  let active = 0; // highlighted row while open

  const items = () => [...list.children] as HTMLElement[];
  const paint = () => {
    const cur = opts.find((o) => o[0] === root.value) ?? opts[0];
    text.replaceChildren(cur?.[1] ?? "", ...(cur?.[2] ? [h("span", { className: "sel-detail" }, cur[2])] : []));
    items().forEach((li, i) => {
      li.ariaSelected = String(opts[i][0] === root.value);
      li.classList.toggle("active", i === active);
    });
    const a = items()[active];
    if (a) {
      list.setAttribute("aria-activedescendant", a.id);
      if (!list.hidden) a.scrollIntoView({ block: "nearest" });
    }
  };
  const close = (focus = true) => {
    if (list.hidden) return;
    list.hidden = true;
    btn.ariaExpanded = "false";
    root.classList.remove("open", "up");
    if (openSelect === close) openSelect = null;
    if (focus) btn.focus();
  };
  const open = () => {
    if (!opts.length) return;
    openSelect?.();
    openSelect = close;
    active = Math.max(0, opts.findIndex((o) => o[0] === root.value));
    list.hidden = false;
    btn.ariaExpanded = "true";
    root.classList.add("open");
    // open upwards when there's no room below (inside the window)
    const r = btn.getBoundingClientRect();
    root.classList.toggle("up", r.bottom + list.offsetHeight + 8 > window.innerHeight && r.top > list.offsetHeight + 8);
    paint();
    list.focus();
  };
  const pick = (i: number) => {
    const v = opts[i]?.[0];
    close();
    if (v === undefined || v === root.value) return;
    root.value = v;
    paint();
    onChange(v);
  };

  root.setOptions = (next, v = root.value) => {
    opts = next;
    root.value = v;
    list.replaceChildren(
      ...opts.map(([, t, detail, tag], i) => {
        const li = h("li", { id: `${id}-${i}`, role: "option", className: "sel-opt" }, h("span", { className: "sel-label" }, t));
        if (detail) li.append(h("span", { className: "sel-detail" }, detail));
        if (tag) li.append(h("span", { className: "sel-tag" }, tag));
        const check = icon("check", 14);
        check.classList.add("sel-check");
        li.prepend(check);
        li.onmousemove = () => active !== i && ((active = i), paint());
        li.onclick = () => pick(i);
        return li;
      }),
    );
    paint();
  };

  btn.onclick = () => (list.hidden ? open() : close());
  btn.onkeydown = (e) => {
    if (["ArrowDown", "ArrowUp", "Enter", " "].includes(e.key)) {
      e.preventDefault();
      open();
    }
  };
  list.onkeydown = (e) => {
    const n = opts.length;
    const move: Record<string, number> = { ArrowDown: active + 1, ArrowUp: active - 1, Home: 0, End: n - 1, PageDown: active + 5, PageUp: active - 5 };
    if (e.key in move) active = Math.min(n - 1, Math.max(0, move[e.key]));
    else if (e.key === "Enter" || e.key === " ") return e.preventDefault(), pick(active);
    else if (e.key === "Escape") return e.preventDefault(), close();
    else if (e.key === "Tab") return close(false);
    else if (e.key.length === 1) {
      // type-ahead: next option starting with the letter
      const k = e.key.toLowerCase();
      const j = [...opts.keys()].map((x) => (active + 1 + x) % n).find((x) => opts[x][1].toLowerCase().startsWith(k));
      if (j === undefined) return;
      active = j;
    } else return;
    e.preventDefault();
    paint();
  };
  list.onblur = (e) => !root.contains(e.relatedTarget as Node) && close(false);

  root.setOptions(options, value);
  return root;
}

// a click anywhere outside the open dropdown closes it
document.addEventListener("pointerdown", (e) => {
  if (openSelect && !(e.target as Element).closest?.(".sel.open")) openSelect();
});

export function button(text: string, onclick: () => void, className = "btn"): HTMLButtonElement {
  return h("button", { type: "button", className, onclick }, text);
}
