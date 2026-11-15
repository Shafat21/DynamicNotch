import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { button, h, page, report, row, section } from "../ui";

export function renderAbout(root: HTMLElement): void {
  const version = h("span", { className: "row-desc" });
  getVersion().then((v) => (version.textContent = `Version ${v}`), () => {});
  const link = (text: string, id: string) => button(text, () => invoke("open_link", { id }).catch((e) => report(String(e))));

  const p = page("About");
  p.append(
    section(
      "Dynamic Notch Island",
      h("p", { className: "page-hint" }, "A Dynamic Island for Windows: music, calendar, weather, a quick launcher and a Claude Code companion, at the top of your screen."),
      version,
    ),
    section(
      "Links",
      row("Source code", "Apache License 2.0.", link("GitHub", "repo")),
      row("Found a problem?", "Report it on GitHub.", link("Report an issue", "issues")),
    ),
  );
  root.append(p);
}
