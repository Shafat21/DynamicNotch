import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import icon from "../../../docs/assets/icon.png";
import { button, h, page, report, row, section } from "../ui";

const FEATURES = ["Music & lyrics", "Calendar", "Weather", "Launcher", "Clipboard", "Notes & reminders", "Voice dictation", "Coding agents"];

export function renderAbout(root: HTMLElement): void {
  const version = h("span", { className: "about-ver" }, "v…");
  getVersion().then((v) => (version.textContent = `v${v}`), () => (version.hidden = true));
  const link = (text: string, id: string) => button(text, () => invoke("open_link", { id }).catch((e) => report(String(e))));

  const p = page("About");
  p.append(
    h(
      "div",
      { className: "about-hero" },
      h("img", { className: "about-icon", src: icon, alt: "", width: 72, height: 72 }),
      h(
        "div",
        { className: "about-text" },
        h("div", { className: "about-name" }, "Dynamic Notch Island", version),
        h("p", { className: "about-tag" }, "A Dynamic Island for Windows and Linux, with Pal, your companion for Claude Code, Codex, Gemini CLI, Cursor and Copilot CLI."),
      ),
    ),
    h("div", { className: "about-chips", ariaLabel: "What it does" }, ...FEATURES.map((f) => h("span", {}, f))),
    section(
      "Links",
      row("Source code", "Open source under the Apache License 2.0.", link("GitHub", "repo")),
      row("Releases", "Download the latest version and read what changed.", link("Releases", "releases")),
      row("Found a problem?", "Report it, or ask for a feature, on GitHub.", link("Report an issue", "issues")),
      row("Made by Shafat", "More of my projects.", link("Website", "site")),
    ),
    section("Privacy", h("p", { className: "page-hint" }, "No accounts and no telemetry. Your settings, notes and history stay on this PC; the app only contacts services you set up (weather, lyrics, your calendar link, your dictation provider).")),
  );
  root.append(p);
}
