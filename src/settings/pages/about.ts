import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import icon from "../../../docs/assets/icon.png";
import { button, h, page, report, row, section } from "../ui";

type UpdateInfo = { version: string; notes: string | null; can_install: boolean };

const FEATURES = ["Screenshots & markup", "Music, lyrics & Spotify", "Coding agents", "Calendar", "Focus timer", "Launcher & quick links", "Notes & reminders", "Clipboard", "Devices & battery", "Progress & countdowns", "Voice dictation", "Weather", "Both displays"];

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
        h("p", { className: "about-tag" }, "A Dynamic Island for Windows and Linux: music, screenshots, your calendar and more at the top of the screen, with Pal, your companion for Claude Code, Codex, Gemini CLI, Cursor, Copilot CLI, Antigravity, Command Code and opencode."),
      ),
    ),
    h("div", { className: "about-chips", ariaLabel: "What it does" }, ...FEATURES.map((f) => h("span", {}, f))),
    section("Updates", updatesRow(link)),
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

/** Check now / Install & restart. The island checks by itself a minute after start and every 6 hours
 *  (updater.rs) and shows a toast; nothing installs until this button is clicked. */
function updatesRow(link: (text: string, id: string) => HTMLButtonElement): HTMLElement {
  const desc = h("span", {}, "Checks GitHub a minute after start and every 6 hours.");
  const controls = h("div", { className: "row-actions" });
  const r = row("Software update", "", controls);
  r.querySelector(".row-desc")!.replaceChildren(desc);
  const check = button("Check for updates", () => {
    check.disabled = true;
    check.textContent = "Checking…";
    invoke<UpdateInfo | null>("check_update").then(show, (e) => {
      desc.textContent = `Couldn't check: ${e}`;
      check.disabled = false;
      check.textContent = "Try again";
    });
  });
  const show = (u: UpdateInfo | null) => {
    check.disabled = false;
    check.textContent = "Check for updates";
    if (!u) {
      desc.textContent = "You're on the latest version.";
      controls.replaceChildren(check);
      return;
    }
    desc.textContent = u.can_install ? `Version ${u.version} is available.` : `Version ${u.version} is available. Your package manager owns this install: download it from Releases.`;
    const install = u.can_install
      ? button("Install & restart", () => {
          install.disabled = true;
          install.textContent = "Installing…";
          desc.textContent = `Downloading ${u.version}. The app restarts by itself.`;
          invoke("install_update").catch((e) => {
            desc.textContent = `Update failed: ${e}`;
            install.disabled = false;
            install.textContent = "Try again";
          });
        }, "btn primary")
      : link("Download", "releases");
    controls.replaceChildren(install);
  };
  controls.append(check);
  invoke<UpdateInfo | null>("update_status").then((u) => u && show(u), () => {});
  return r;
}
