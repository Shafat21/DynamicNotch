// Compact notch: Shelf card with a Files / Clipboard toggle. Lists come straight from Rust; file names and
// clip text are user data: textContent only.
import { invoke } from "@tauri-apps/api/core";

type ShelfItem = { id: number; name: string; exists: boolean; icon: string | null };
type Clip = { id: number; text: string; at: number };
export type ShelfMode = "files" | "clips";

const node = (tag: string, cls: string, text = "") => Object.assign(document.createElement(tag), { className: cls, textContent: text });
const ago = (ms: number) => {
  const m = Math.floor((Date.now() - ms) / 60000);
  return m < 1 ? "Just now" : m < 60 ? `${m} min ago` : m < 1440 ? `${Math.floor(m / 60)} h ago` : `${Math.floor(m / 1440)} d ago`;
};

export function renderShelf(el: HTMLElement, s: { mode: ShelfMode; onMode(m: ShelfMode): void }): void {
  const seg = node("div", "cs-seg");
  seg.setAttribute("role", "group");
  seg.setAttribute("aria-label", "Shelf shows");
  for (const [m, label] of [["files", "Files"], ["clips", "Clipboard"]] as const) {
    const b = node("button", "cs-mode", label) as HTMLButtonElement;
    b.type = "button";
    b.setAttribute("aria-pressed", String(s.mode === m));
    b.onclick = (e) => (e.stopPropagation(), s.onMode(m));
    seg.append(b);
  }
  const list = node("div", "cs-list");
  const note = node("div", "cs-note muted small");
  const foot = node("div", "cs-foot");
  foot.append(note, seg); // one row under the list: the list keeps the height
  el.replaceChildren(list, foot);
  const fail = (e: unknown) => (note.textContent = String(e));

  if (s.mode === "files") {
    invoke<ShelfItem[]>("list_shelf").then((items) => {
      if (!items.length) {
        list.append(node("div", "cs-drop"));
        list.firstElementChild!.append(node("b", "", "Drop files here"), node("span", "muted small", "Drag them back out anytime · click to open"));
        return;
      }
      for (const it of items) {
        const t = node("button", `cs-tile${it.exists ? "" : " gone"}`) as HTMLButtonElement;
        t.type = "button";
        t.title = it.name;
        if (it.icon) t.append(Object.assign(document.createElement("img"), { src: it.icon, alt: "" }));
        t.append(node("span", "cs-name ell", it.name));
        t.onclick = (e) => (e.stopPropagation(), it.exists && void invoke("open_shelf", { id: it.id }).catch(fail));
        t.onpointerdown = (e) => e.button === 0 && it.exists && void invoke("drag_out", { id: it.id }).catch(() => {}); // native drag-out (Windows)
        list.append(t);
      }
      note.textContent = `${items.length} file${items.length === 1 ? "" : "s"} · click to open`;
    }, fail);
  } else {
    invoke<Clip[]>("list_clips").then((clips) => {
      if (!clips.length) return void list.append(node("div", "cs-drop muted", "Copy some text and it shows up here"));
      for (const c of clips.slice(0, 12)) {
        const t = node("button", "cs-clip") as HTMLButtonElement;
        t.type = "button";
        t.title = c.text;
        t.append(node("span", "cs-text", c.text), node("span", "cs-when muted small", ago(c.at)));
        t.onclick = (e) => {
          e.stopPropagation();
          invoke("copy_clip", { id: c.id }).then(() => (note.textContent = "Copied"), fail);
        };
        list.append(t);
      }
      note.textContent = `${clips.length} item${clips.length === 1 ? "" : "s"} · click to copy`;
    }, fail);
  }
}
