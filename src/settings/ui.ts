// Small DOM builders for the Settings pages. Text always goes in as text nodes, never HTML.

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

export function select(label: string, options: [value: string, text: string][], value: string, onChange: (v: string) => void): HTMLSelectElement {
  const s = h("select", { ariaLabel: label }, ...options.map(([v, text]) => h("option", { value: v, selected: v === value }, text)));
  s.onchange = () => onChange(s.value);
  return s;
}

export function button(text: string, onclick: () => void, className = "btn"): HTMLButtonElement {
  return h("button", { type: "button", className, onclick }, text);
}
