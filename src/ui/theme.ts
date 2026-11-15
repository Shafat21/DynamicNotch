// Applies the saved appearance (sticky `appearance` event, spec §22.3) to CSS variables + data attributes.
export type Theme = { name: string; bgFrom: string; bgTo: string; card: string; accent: string };
export type LayoutItem = { id: string; visible: boolean };
export type Layout = { tabs: LayoutItem[]; home: LayoutItem[] };
export type Appearance = {
  theme: string; accent: string; bgMode: string; iconStyle: string; badgeColors: Record<string, string>; shape: string;
  density: string; radius: number; textScale: number; cardOpacity: number; tabLabels: string; openOn: string;
  collapsedW: number; collapsedH: number; openW: number; openH: number;
};
export type Look = { appearance: Appearance; layout: Layout; theme: Theme };

const items = (ids: string[], hidden: string[] = []) => ids.map((id) => ({ id, visible: !hidden.includes(id) }));
/** Rust defaults (appearance.rs), used only until the snapshot/event arrives. */
export const DEFAULT_LOOK: Look = {
  appearance: {
    theme: "Midnight", accent: "", bgMode: "glass", iconStyle: "line", badgeColors: {}, shape: "pill", density: "comfortable",
    radius: 20, textScale: 1, cardOpacity: 0.75, tabLabels: "always", openOn: "home", collapsedW: 180, collapsedH: 40, openW: 760, openH: 260,
  },
  layout: {
    tabs: items(["home", "music", "calendar", "pal", "timer", "launcher", "weather", "system", "tasks", "month", "notes", "reminders", "clipboard", "sound"]),
    home: items(["clock", "pal", "music", "calendar", "launcher", "focus", "system", "tasks", "notes", "reminders", "sound"], ["system", "tasks", "sound"]),
  },
  theme: { name: "Midnight", bgFrom: "#1b2033", bgTo: "#20263b", card: "#2c3450", accent: "#8fb0ff" },
};

// Spec defaults for the eight named badges; the rest borrow a related colour.
const BADGES: Record<string, string> = {
  music: "#1DB954", focus: "#FF9F0A", calendar: "#FF453A", settings: "#8E7CFF", pal: "#30D158", launcher: "#0A84FF", weather: "#64D2FF", system: "#5E5CE6",
  timer: "#FF9F0A", clock: "#64D2FF", tasks: "#FFD60A", month: "#BF5AF2", notes: "#FFCC00", reminders: "#FF9F0A", clipboard: "#64D2FF", sound: "#FF375F", home: "#8E8E93",
};
const DENSITY: Record<string, [gap: number, pad: string]> = { compact: [8, "10px 12px"], comfortable: [12, "14px 16px"], airy: [16, "18px 20px"] };

/** Dark glyph on light badges, white on dark ones (relative luminance of #RRGGBB). */
function badgeFg(hex: string): string {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b > 0.55 ? "#000" : "#fff";
}

export function applyTheme({ appearance: a, theme: t }: Look): void {
  const root = document.documentElement;
  const [gap, pad] = DENSITY[a.density] ?? DENSITY.comfortable;
  const vars: Record<string, string> = {
    "--bg-from": t.bgFrom, "--bg-to": t.bgTo, "--card": t.card, "--card-opacity": String(a.cardOpacity), "--accent": a.accent || t.accent,
    "--radius": `${a.radius}px`, "--gap": `${gap}px`, "--pad": pad, "--text-scale": String(a.textScale),
  };
  for (const [k, v] of Object.entries(vars)) root.style.setProperty(k, v);
  Object.assign(root.dataset, { icons: a.iconStyle, bg: a.bgMode, labels: a.tabLabels });
  paintBadges(a);
}

/** Every `[data-badge]` gets its colour as `--badge` (+ readable `--badge-fg`); CSS uses them only in badges mode. */
export function paintBadges(a: Appearance, root: ParentNode = document): void {
  root.querySelectorAll<HTMLElement>("[data-badge]").forEach((el) => {
    const c = a.badgeColors[el.dataset.badge!] ?? BADGES[el.dataset.badge!] ?? "#8E8E93";
    el.style.setProperty("--badge", c);
    el.style.setProperty("--badge-fg", badgeFg(c));
  });
}
