// Launcher search box + quick links (Launcher tab). Typing filters the app tiles; "yt lofi", "standup" or an address
// opens a saved/typed link; Enter on anything else launches the first matching app, or searches the web.
// Link and app names come from the user's own data: textContent only.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { LauncherApp } from "./panels";

type Link = { id: number; name: string; keyword: string; url: string };
type Resolved = { kind: "link" | "url" | "search"; label: string; url: string };

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
let resolved: Resolved | null = null;
let seq = 0;

export function initQuickLinks(launch: (a: LauncherApp) => void, apps: () => LauncherApp[], onError: (e: unknown) => void): void {
  const input = $<HTMLInputElement>("launcher-search");
  const open = (url: string) =>
    invoke("open_quick", { url })
      .then(() => {
        input.value = "";
        update();
      })
      .catch(onError);
  const tiles = () => [...document.querySelectorAll<HTMLElement>("#launcher-grid .app:not(.app-add)")];

  /** Apps whose name contains the typed text. */
  const matching = () => {
    const q = input.value.trim().toLowerCase();
    return apps().filter((a) => q && a.name.toLowerCase().includes(q));
  };

  function update(): void {
    const q = input.value.trim().toLowerCase();
    tiles().forEach((t) => (t.hidden = !!q && !t.dataset.name!.toLowerCase().includes(q)));
    $("launcher-add").hidden = !!q;
    const mine = ++seq;
    if (!q) return show(null);
    invoke<Resolved | null>("quick_resolve", { input: input.value })
      .then((r) => mine === seq && show(r))
      .catch(onError);
  }

  /** The hint under the box says what Enter will do; a plain web search only when no app matches. */
  function show(r: Resolved | null): void {
    resolved = r;
    const found = matching();
    const text = !r ? "" : r.kind === "search" && found.length ? `Open ${found[0].name}` : r.label;
    $("launcher-hint").hidden = !text;
    $("launcher-hint-text").textContent = text ? `↵  ${text}` : "";
    $("launcher-hint-save").hidden = r?.kind !== "url";
  }

  input.oninput = update;
  input.onkeydown = (e) => {
    if (e.key === "Escape") {
      input.value = "";
      update();
    } else if (e.key === "Enter" && resolved) {
      e.preventDefault();
      const first = matching()[0];
      if (resolved.kind === "search" && first) {
        launch(first);
        input.value = "";
        update();
      } else open(resolved.url);
    }
  };

  // ----- saved links -----
  const form = $<HTMLFormElement>("ql-form");
  const fields = { name: $<HTMLInputElement>("ql-name"), key: $<HTMLInputElement>("ql-key"), url: $<HTMLInputElement>("ql-url") };
  const err = (m = "") => ($("ql-err").textContent = m);
  const edit = (name = "", url = "") => {
    form.hidden = false;
    fields.name.value = name;
    fields.key.value = "";
    fields.url.value = url;
    err();
    (name ? fields.key : fields.name).focus();
  };
  $("ql-add").onclick = () => (form.hidden ? edit() : (form.hidden = true));
  $("ql-cancel").onclick = () => (form.hidden = true);
  $("launcher-hint-save").onclick = () => resolved && edit(new URL(resolved.url).hostname.replace(/^www\./, ""), resolved.url);
  form.onsubmit = (e) => {
    e.preventDefault();
    invoke<Link[]>("add_quick_link", { name: fields.name.value, keyword: fields.key.value, url: fields.url.value })
      .then((l) => {
        form.hidden = true;
        draw(l);
      })
      .catch((m) => err(String(m)));
  };

  const draw = (list: Link[]) => {
    $("ql-chips").replaceChildren(
      ...list.map((l) => {
        const b = Object.assign(document.createElement("button"), { className: "lq-chip" });
        b.title = `${l.url}${l.keyword ? `\nType: ${l.keyword}` : ""}\nRight-click to remove`;
        b.append(Object.assign(document.createElement("span"), { textContent: l.name }));
        if (l.keyword) b.append(Object.assign(document.createElement("kbd"), { textContent: l.keyword }));
        // a link with a {query} slot opens its site from the chip; the search box fills the slot
        b.onclick = () => open(l.url.replace("{query}", ""));
        b.oncontextmenu = (e) => {
          e.preventDefault();
          invoke<Link[]>("remove_quick_link", { id: l.id }).then(draw).catch(onError);
        };
        return b;
      }),
    );
  };
  invoke<Link[]>("list_quick_links").then(draw).catch(onError);
  void listen<Link[]>("quick-links", (e) => draw(e.payload));
  new MutationObserver(update).observe($("launcher-grid"), { childList: true }); // apps changed: refilter
}
