// Settings → Roadmap: loaded live from roadmap.json in the GitHub repo (Rust fetches it; this build's copy when
// offline), so editing that file updates every install. Its text is shown with textContent only.
import { invoke } from "@tauri-apps/api/core";
import { button, h, page, report, section } from "../ui";

type Roadmap = { updated: string; sections: { title: string; items: { title: string; text: string }[] }[]; live: boolean };

export function renderRoadmap(root: HTMLElement): void {
  const p = page("Roadmap");
  const hint = h("p", { className: "page-hint" }, "Loading…");
  const body = h("div", {});
  const link = (text: string, id: string, cls = "btn") => button(text, () => invoke("open_link", { id }).catch((e) => report(String(e))), cls);
  p.append(hint, body, h("div", { className: "row-actions rm-actions" }, link("Request a feature", "request", "btn primary"), link("Report a bug", "issues")));
  root.append(p);

  invoke<Roadmap>("roadmap")
    .then((r) => {
      hint.textContent = `${r.live ? "Updated" : "Offline copy from"} ${r.updated}. What's coming, roughly in order: things move up when people ask for them.`;
      body.replaceChildren(
        ...r.sections.map((s) =>
          section(s.title, ...s.items.map((i) => h("div", { className: "rm-item" }, h("div", { className: "rm-title" }, i.title), h("div", { className: "rm-text" }, i.text)))),
        ),
      );
    })
    .catch((e) => (hint.textContent = `Couldn't load the roadmap: ${e}`));
}
