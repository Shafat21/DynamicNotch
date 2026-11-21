import { invoke } from "@tauri-apps/api/core";
import { patchAppearance, setGeneral, store } from "../store";
import { setShutter, shutterOn, testShutter } from "../../shutter";
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

/** Screenshot sound: a Test button and the on / off switch (read from the app, which plays the sound itself). */
function shutterControl(): HTMLElement {
  const fail = (e: unknown) => report(String(e));
  const sw = toggle("Screenshot sound", true, (on) => void setShutter(on).then(() => on && testShutter(), fail));
  void shutterOn().then((on) => (sw.checked = on), () => {});
  return h("div", { className: "row-actions" }, button("Test", () => void testShutter().catch(fail), "btn small"), sw);
}

/** Screenshot folder: shown in full, with Change… (native folder picker), Open and Reset (back to Pictures › Screenshots). */
function captureFolder(): HTMLElement {
  const path = h("code", { className: "sp-redirect cap-dir" }, "…");
  const show = (p: string) => ((path.textContent = p), (path.title = p));
  const fail = (e: unknown) => report(String(e));
  invoke<string>("capture_dir").then(show, fail);
  return h(
    "div",
    { className: "stack" },
    path,
    h(
      "div",
      { className: "row-actions" },
      button("Change…", () => void invoke<string>("choose_capture_dir").then(show, fail), "btn small"),
      button("Open", () => void invoke("open_capture_dir").catch(fail), "btn small"),
      button("Reset", () => void invoke<string>("set_capture_dir", { dir: "" }).then((p) => (show(p), report("Back to Pictures › Screenshots")), fail), "btn small"),
    ),
  );
}

/** Screenshot hotkey: press the combination in the field (or type it, then Enter). Saved and applied at once. */
function captureHotkey(): HTMLElement {
  const input = h("input", { type: "text", className: "txt hk-in", placeholder: "Click, then press keys", ariaLabel: "Screenshot hotkey", maxLength: 40, spellcheck: false });
  const err = h("span", { className: "row-desc hk-err" });
  const save = async (v: string) => {
    try {
      input.value = await invoke<string>("set_capture_hotkey", { hotkey: v });
      err.textContent = input.value ? "" : "No hotkey: use the camera button in the notch.";
      input.ariaInvalid = "false";
      report(input.value ? `Screenshot hotkey: ${input.value}` : "Screenshot hotkey removed");
    } catch (e) {
      err.textContent = String(e);
      input.ariaInvalid = "true";
    }
    input.blur();
  };
  // keyboard combination → "Ctrl+Shift+S" (the backend validates and may refuse, e.g. a key with no modifier)
  const keyName = (e: KeyboardEvent): string | null => {
    const c = e.code;
    if (/^Key[A-Z]$/.test(c)) return c.slice(3);
    if (/^Digit\d$/.test(c)) return c.slice(5);
    if (/^F\d{1,2}$/.test(c)) return c;
    return ({ PrintScreen: "PrintScreen", Space: "Space", Insert: "Insert", Home: "Home", End: "End", PageUp: "PageUp", PageDown: "PageDown", Delete: "Delete" } as Record<string, string>)[c] ?? null;
  };
  const combo = (e: KeyboardEvent) => {
    const k = keyName(e);
    if (!k) return null;
    return [e.ctrlKey && "Ctrl", e.shiftKey && "Shift", e.altKey && "Alt", e.metaKey && "Win", k].filter(Boolean).join("+");
  };
  input.onkeydown = (e) => {
    if (e.key === "Escape") return input.blur();
    if (e.key === "Enter") return e.preventDefault(), void save(input.value);
    const withMods = e.ctrlKey || e.altKey || e.metaKey || /^F\d{1,2}$/.test(e.code);
    if (!withMods) return; // plain typing: let them type "PrintScreen" and press Enter
    const c = combo(e);
    if (!c) return;
    e.preventDefault();
    input.value = c;
    void save(c);
  };
  input.onkeyup = (e) => e.code === "PrintScreen" && void save("PrintScreen"); // Windows sends no keydown for it
  invoke<string>("capture_hotkey").then((v) => {
    input.value = v;
    err.textContent = v ? "" : "No hotkey yet: the camera button in the notch always works.";
  }, () => {});
  const clear = button("Clear", () => void save(""), "btn small");
  return h("div", { className: "stack" }, h("div", { className: "row-actions" }, input, clear), err);
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
      row("Screenshot hotkey", "Press it anywhere to drag an area, mark it up and copy or save it. E.g. Ctrl+Shift+S, or PrintScreen (turn off Windows' own PrintScreen snipping in Settings › Accessibility › Keyboard first).", captureHotkey()),
      row(
        "Screenshot sound",
        "A camera shutter when a screenshot is copied or saved.",
        shutterControl(),
      ),
      row("Screenshot folder", "Where “Save” puts screenshots. Default: Pictures › Screenshots in your user folder (Windows and Linux).", captureFolder()),
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
