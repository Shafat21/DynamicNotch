import { invoke } from "@tauri-apps/api/core";
import { patchAppearance, setGeneral, store } from "../store";
import { button, h, page, report, row, section, segmented, select, toggle, type Choice } from "../ui";

export const OPEN_ON: [string, string][] = [["home", "Home"], ["last", "Last used tab"]];

/** "Keep showing over these apps": chips for the saved list, an input (with a picker of running apps) to add one. */
function exemptApps(): HTMLElement {
  const box = h("div", { className: "exempt" });
  const chips = h("div", { className: "chips" });
  const input = h("input", { type: "text", placeholder: "chrome.exe", className: "txt", ariaLabel: "App file name" });
  input.setAttribute("list", "running-apps");
  const running = h("datalist", { id: "running-apps" });
  const draw = (list: string[]) => {
    chips.replaceChildren(
      ...(list.length ? list : ["None yet"]).map((name) => {
        const chip = h("span", { className: list.length ? "chip" : "chip none", textContent: name });
        if (list.length) chip.append(h("button", { className: "chip-x", title: `Remove ${name}`, textContent: "×", onclick: () => save(name, false) }));
        return chip;
      }),
    );
  };
  const save = (name: string, on: boolean) =>
    invoke<string[]>("set_fullscreen_exempt", { name, on }).then((l) => (draw(l), (input.value = ""))).catch((e) => report(String(e)));
  input.onkeydown = (e) => e.key === "Enter" && save(input.value, true);
  invoke<string[]>("fullscreen_exempt").then(draw).catch((e) => report(`Couldn't load the list: ${e}`));
  invoke<string[]>("running_apps").then((l) => running.replaceChildren(...l.map((n) => h("option", { value: n })))).catch(() => {});
  box.append(chips, h("div", { className: "row-actions" }, input, button("Add", () => save(input.value, true), "btn small"), running));
  return box;
}

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
      row("Keep showing over these apps", "Stay visible even when one of these apps is fullscreen. Add its file name, or pick a running app.", exemptApps()),
      row(
        "Show on both displays",
        "An island on a second monitor too, same tabs and content. Windows only; needs two monitors.",
        toggle("Show on both displays", s.dualNotch, (on) => set("dual_notch", on)),
      ),
      row("Open on", "Which tab the island shows when you hover it.", segmented("Open on", OPEN_ON, store.look.appearance.openOn, (v) => patchAppearance({ openOn: v }))),
    ),
  );
  root.append(p);
}
