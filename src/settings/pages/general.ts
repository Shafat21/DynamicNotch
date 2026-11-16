import { invoke } from "@tauri-apps/api/core";
import { patchAppearance, setGeneral, store } from "../store";
import { page, report, row, section, segmented, select, toggle, type Choice } from "../ui";

export const OPEN_ON: [string, string][] = [["home", "Home"], ["last", "Last used tab"]];

export function renderGeneral(root: HTMLElement): void {
  const s = store.settings;
  const redraw = () => {
    root.replaceChildren();
    renderGeneral(root);
  };
  const set = (key: string, value: string | boolean) => setGeneral(key, value).catch(redraw);

  const primary: Choice = ["primary", "Primary display", "whichever is main"];
  const display = select("Display", [primary], s.display, (v) => set("display", v));
  invoke<{ name: string; label: string; width: number; height: number; primary: boolean }[]>("list_displays")
    .then((list) =>
      display.setOptions([primary, ...list.map((m): Choice => [m.name, m.label, `${m.width} × ${m.height}`, m.primary ? "Primary" : undefined])], s.display),
    )
    .catch((e) => report(`Couldn't list displays: ${e}`));

  const p = page("General");
  p.append(
    section(
      "Startup",
      row("Start at login", "Open the island when you sign in to Windows.", toggle("Start at login", s.autostart, (on) => set("autostart", on))),
    ),
    section(
      "Island",
      row("Display", "The screen the island sits on. An unplugged screen falls back to the primary one.", display),
      row(
        "Island size",
        "Auto follows your screen resolution.",
        segmented("Island size", ["Auto", "Small", "Medium", "Large"].map((v) => [v, v]), s.islandSize, (v) => set("island_size", v)),
      ),
      row(
        "Hide in fullscreen apps",
        "Slide away while a game, video or presentation fills the screen.",
        toggle("Hide in fullscreen apps", s.hideFullscreen, (on) => set("hide_fullscreen", on)),
      ),
      row("Open on", "Which tab the island shows when you hover it.", segmented("Open on", OPEN_ON, store.look.appearance.openOn, (v) => patchAppearance({ openOn: v }))),
    ),
  );
  root.append(p);
}
