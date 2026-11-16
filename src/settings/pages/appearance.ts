import { patchAppearance, resetAppearance, store, type Appearance } from "../store";
import { h, page, report, row, section, segmented, slider } from "../ui";
import { OPEN_ON } from "./general";

const HEX = /^#[0-9a-fA-F]{6}$/;
const px = (v: number) => `${Math.round(v)} px`;
const pct = (v: number) => `${Math.round(v * 100)}%`;

export function renderAppearance(root: HTMLElement): void {
  const a = store.look.appearance;
  const redraw = () => {
    const y = root.scrollTop;
    root.replaceChildren();
    renderAppearance(root);
    root.scrollTop = y;
  };
  const p = page("Appearance", () => resetAppearance().then(redraw, (e) => report(`Couldn't reset: ${e}`)));

  // Theme swatches: gradient preview + accent dot, name below; the selected one is outlined.
  const themes = h("div", { className: "swatches" });
  for (const t of store.look.themes) {
    const preview = h("span", { className: "swatch-preview" }, h("span", { className: "swatch-dot" }));
    preview.style.background = `linear-gradient(135deg, ${t.bgFrom}, ${t.bgTo})`;
    (preview.firstChild as HTMLElement).style.background = t.accent;
    const b = h("button", { type: "button", className: "swatch", ariaPressed: String(t.name === a.theme) }, preview, h("span", {}, t.name));
    b.onclick = () => {
      patchAppearance({ theme: t.name, accent: "" }); // like apply_theme: a preset brings its own accent
      redraw();
    };
    themes.append(b);
  }

  // Accent: "Theme" (empty = the theme's own), the preset accents, or a custom #RRGGBB.
  const hex = h("input", { type: "text", className: "hex", value: a.accent, placeholder: "#RRGGBB", maxLength: 7, spellcheck: false, ariaLabel: "Custom accent colour (#RRGGBB)" });
  const accents = h("div", { className: "accents", role: "group", ariaLabel: "Accent colour" });
  const same = (x: string, y: string) => x.toLowerCase() === y.toLowerCase();
  // the theme's own accent is one swatch, "Theme accent": a custom value equal to it counts as that swatch, not a second one
  const markAccent = () => {
    const cur = store.look.appearance.accent;
    accents.querySelectorAll<HTMLButtonElement>("button").forEach((b) => {
      const v = b.dataset.accent!;
      b.ariaPressed = String(v === "" ? cur === "" || same(cur, store.look.theme.accent) : same(v, cur));
    });
  };
  const themeAccent = store.look.theme.accent;
  const options = [["", themeAccent, "Theme accent"], ...[...new Set(store.look.themes.map((t) => t.accent.toLowerCase()))].filter((c) => !same(c, themeAccent)).map((c) => [c, c, c])];
  for (const [value, colour, label] of options) {
    const b = h("button", { type: "button", className: value ? "accent" : "accent theme-accent", ariaLabel: label, title: label });
    b.style.background = colour;
    b.dataset.accent = value;
    b.onclick = () => {
      patchAppearance({ accent: value });
      hex.value = value;
      hex.ariaInvalid = "false";
      markAccent();
    };
    accents.append(b);
  }
  markAccent();
  hex.oninput = () => {
    const v = hex.value.trim();
    const ok = v === "" || HEX.test(v);
    hex.ariaInvalid = String(!ok);
    if (!ok) return;
    patchAppearance({ accent: v });
    markAccent();
  };

  const seg = (label: string, desc: string, key: keyof Appearance, opts: [string, string][]) =>
    row(label, desc, segmented(label, opts, String(a[key]), (v) => patchAppearance({ [key]: v })));
  const range = (label: string, desc: string, key: keyof Appearance, min: number, max: number, step: number, fmt: (v: number) => string) =>
    row(label, desc, slider(label, { min, max, step, value: Number(a[key]), fmt }, (v) => patchAppearance({ [key]: v })));

  p.append(
    section("Theme", themes),
    section("Accent", row("Accent colour", "Tints the active tab, buttons and highlights.", h("div", { className: "accent-pick" }, accents, hex))),
    section(
      "Style",
      seg("Icon style", "Thin line icons or coloured badges.", "iconStyle", [["line", "Line"], ["badges", "Colour badges"]]),
      seg("Shape", "A floating pill or a notch joined to the top edge.", "shape", [["pill", "Pill"], ["notch", "Notch"]]),
      seg("Density", "Spacing inside cards.", "density", [["compact", "Compact"], ["comfortable", "Comfortable"], ["airy", "Airy"]]),
      seg("Tab labels", "Text next to the tab icons.", "tabLabels", [["always", "Always"], ["active", "Active tab"], ["never", "Never"]]),
      seg("Open on", "Which tab the island shows when you hover it.", "openOn", OPEN_ON),
    ),
    section(
      "Size & shape",
      range("Corner radius", "Roundness of the island and its cards.", "radius", 8, 40, 1, px),
      range("Text size", "Scales all text in the island.", "textScale", 0.85, 1.25, 0.05, pct),
      range("Card opacity", "How solid the cards look on the background.", "cardOpacity", 0.3, 1, 0.05, pct),
      range("Collapsed width", "Width of the idle pill (at 1080p).", "collapsedW", 140, 320, 1, px),
      range("Collapsed height", "Height of the idle pill (at 1080p).", "collapsedH", 28, 48, 1, px),
      range("Open width", "Width of the open island (at 1080p).", "openW", 560, 1000, 10, px),
      range("Open height", "Height of the open island (at 1080p).", "openH", 200, 420, 10, px),
    ),
  );
  root.append(p);
}
