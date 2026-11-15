// Settings-window state: the `appearance` payload and General settings, plus the calls that change them.
import { invoke } from "@tauri-apps/api/core";
import { report } from "./ui";

export type Theme = { name: string; bgFrom: string; bgTo: string; card: string; accent: string };
export type LayoutItem = { id: string; visible: boolean };
export type Layout = { tabs: LayoutItem[]; home: LayoutItem[] };
export type Appearance = {
  theme: string; accent: string; bgMode: string; iconStyle: string; shape: string; density: string;
  radius: number; textScale: number; cardOpacity: number; glassFrost: number; tabLabels: string; openOn: string;
  collapsedW: number; collapsedH: number; openW: number; openH: number;
};
export type Look = { appearance: Appearance; layout: Layout; theme: Theme; themes: Theme[] };
export type Settings = { location: string; autostart: boolean; display: string; islandSize: string; hideFullscreen: boolean; reminderSound: boolean; clipRemember: boolean; clipPaused: boolean };

export const store = { look: null as unknown as Look, settings: null as unknown as Settings };

let pending: Partial<Appearance> = {};
let timer = 0;

/** Window colours follow the island's current theme (+ custom accent). */
export function applyTheme(): void {
  const { theme, appearance } = store.look;
  const s = document.documentElement.style;
  s.setProperty("--bg-from", theme.bgFrom);
  s.setProperty("--bg-to", theme.bgTo);
  s.setProperty("--card", theme.card);
  s.setProperty("--accent", appearance.accent || theme.accent);
}

/** A payload from Rust (reply or `appearance` event); edits not yet sent stay on top. */
export function acceptLook(p: Look): void {
  store.look = p;
  Object.assign(p.appearance, pending);
  applyTheme();
}

/** Optimistic local change; everything changed within 200 ms goes out as one `set_appearance`. */
export function patchAppearance(patch: Partial<Appearance>): void {
  Object.assign(store.look.appearance, patch);
  Object.assign(pending, patch);
  if (patch.theme) store.look.theme = store.look.themes.find((t) => t.name === patch.theme) ?? store.look.theme;
  applyTheme();
  clearTimeout(timer);
  timer = window.setTimeout(async () => {
    const sent = pending;
    pending = {};
    try {
      acceptLook(await invoke<Look>("set_appearance", { patch: sent }));
    } catch (e) {
      report(`Couldn't save: ${e}`);
    }
  }, 200);
}

export async function resetAppearance(): Promise<void> {
  clearTimeout(timer);
  pending = {};
  acceptLook(await invoke<Look>("reset_appearance"));
}

/** `layout` omitted = save the current one; an empty list makes Rust restore that list's defaults. */
export async function saveLayout(layout: Layout = store.look.layout): Promise<void> {
  try {
    acceptLook(await invoke<Look>("set_layout", { layout }));
  } catch (e) {
    report(`Couldn't save: ${e}`);
  }
}

/** Throws (after reporting) so the caller can redraw the real state. */
export async function setGeneral(key: string, value: string | boolean): Promise<void> {
  try {
    store.settings = await invoke<Settings>("set_general", { key, value });
  } catch (e) {
    report(String(e));
    throw e;
  }
}
