// Settings → Dictation: the stats strip (words dictated, words/min, days in a row, top apps) and the
// local history with search, copy and delete. All text via textContent.
// Layout ideas from Coucou's dictation / insights pages (https://github.com/Louis-CFM/coucou), © 2026 Louis Raillé, MIT License.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { button, h, page, report, section } from "../ui";

type Entry = { id: number; text: string; app: string | null; at: number; words: number; ms: number };
type Stats = { words: number; dictations: number; wordsPerMinute: number; streak: number; topApps: [string, number][] };

const tile = (value: string, label: string) => h("div", { className: "stat" }, h("div", { className: "stat-value" }, value), h("div", { className: "stat-label" }, label));

export function renderDictation(root: HTMLElement): () => void {
  const strip = h("div", { className: "stats" });
  const apps = h("p", { className: "row-desc" });
  const search = h("input", { type: "search", placeholder: "Search dictations", ariaLabel: "Search dictations", maxLength: 200, spellcheck: false });
  const list = h("ul", { className: "history", ariaLabel: "Dictation history" });

  const drawStats = (s: Stats) => {
    strip.replaceChildren(tile(s.words.toLocaleString(), "words dictated"), tile(String(s.wordsPerMinute), "words / min"), tile(String(s.streak), s.streak === 1 ? "day in a row" : "days in a row"));
    apps.textContent = s.topApps.length ? `Top apps: ${s.topApps.map(([a, w]) => `${a} (${w})`).join(", ")}` : "";
  };

  const drawList = (items: Entry[]) => {
    if (!items.length) {
      list.replaceChildren(h("li", { className: "row-desc" }, search.value ? "No dictation matches." : "No dictations yet. Hold the hotkey (Ctrl+Win by default) and speak."));
      return;
    }
    list.replaceChildren(
      ...items.map((e) => {
        const meta = [new Date(e.at).toLocaleString([], { dateStyle: "medium", timeStyle: "short" }), e.app, `${e.words} words`].filter(Boolean).join(" · ");
        const copy = button("Copy", () => invoke("dictation_copy", { text: e.text }).then(() => report("Copied"), (err) => report(String(err))), "btn small");
        const del = button("Delete", () => invoke("delete_dictation", { id: e.id }).then(refresh, (err) => report(String(err))), "btn small");
        copy.ariaLabel = `Copy dictation from ${meta}`;
        del.ariaLabel = `Delete dictation from ${meta}`;
        return h("li", {}, h("div", { className: "history-text" }, h("div", { className: "history-meta" }, meta), h("div", {}, e.text)), h("div", { className: "history-actions" }, copy, del));
      }),
    );
  };

  function refresh(): void {
    invoke<Stats>("dictation_stats").then(drawStats, (e) => report(String(e)));
    invoke<Entry[]>("dictation_history", { query: search.value }).then(drawList, (e) => report(String(e)));
  }
  let t = 0;
  search.oninput = () => {
    clearTimeout(t);
    t = window.setTimeout(refresh, 200);
  };

  const p = page("Dictation");
  p.append(h("p", { className: "page-hint" }, "Computed on this PC from your saved dictations."), strip, apps, section("History", search, list));
  root.append(p);
  refresh();
  const un = listen("dictation-history", refresh);
  return () => {
    clearTimeout(t);
    un.then((f) => f());
  };
}
