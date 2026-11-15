// Shelf tab (spec §23.6): files dropped on the island. File names are user data: textContent only.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { icon } from "./icons-line";

type Item = { id: number; path: string; name: string; isDir: boolean; exists: boolean; icon: string | null };

const $ = (id: string) => document.getElementById(id)!;
const DRAG_PX = 6;
let items: Item[] = [];
let dragOut = true; // flips off when the OS can't start a native drag; the menu keeps Copy path / Show in folder
let flash = "";
let flashTimer = 0;

function say(msg: string): void {
  flash = msg;
  window.clearTimeout(flashTimer);
  flashTimer = window.setTimeout(() => {
    flash = "";
    draw();
  }, 1500);
  draw();
}

async function run(cmd: string, id: number, done?: string): Promise<void> {
  try {
    await invoke(cmd, { id });
    if (done) say(done);
  } catch (e) {
    say(String(e));
  }
}

async function update(cmd: string, id?: number): Promise<void> {
  try {
    items = await invoke<Item[]>(cmd, { id });
    draw();
  } catch (e) {
    console.error(e);
  }
}

function closeMenu(): void {
  $("shelf-menu").hidden = true;
}

function openMenu(it: Item, x: number, y: number): void {
  const menu = $("shelf-menu");
  const entries: [string, () => void, boolean][] = [
    ["Open", () => void run("open_shelf", it.id), it.exists],
    ["Show in folder", () => void run("reveal_shelf", it.id), it.exists],
    ["Copy path", () => void run("copy_shelf_path", it.id, "Path copied"), true],
    ["Remove", () => void update("remove_shelf", it.id), true],
  ];
  menu.replaceChildren(
    ...entries.map(([label, fn, on]) => {
      const b = document.createElement("button");
      b.type = "button";
      b.textContent = label;
      b.disabled = !on;
      b.onclick = () => {
        closeMenu();
        fn();
      };
      return b;
    }),
  );
  const host = $("tab-shelf").getBoundingClientRect();
  menu.hidden = false;
  menu.style.left = `${Math.max(0, Math.min(x - host.left, host.width - menu.offsetWidth))}px`;
  menu.style.top = `${Math.max(0, Math.min(y - host.top, host.height - menu.offsetHeight))}px`;
  (menu.querySelector("button:not(:disabled)") as HTMLElement | null)?.focus();
}

function tile(it: Item): HTMLElement {
  const b = document.createElement("button");
  b.type = "button";
  b.className = "shelf-tile" + (it.exists ? "" : " missing");
  b.title = it.path;
  const ic = document.createElement("span");
  ic.className = "shelf-ic";
  if (it.icon) {
    const img = document.createElement("img");
    img.src = it.icon;
    img.alt = "";
    img.draggable = false;
    ic.append(img);
  } else {
    ic.innerHTML = icon(it.isDir ? "folder" : "notes", 26); // static SVG
  }
  const name = document.createElement("span");
  name.className = "shelf-name";
  name.textContent = it.name;
  b.append(ic, name);
  if (!it.exists) {
    const m = document.createElement("span");
    m.className = "shelf-badge";
    m.textContent = "Missing";
    b.append(m);
  }
  b.onclick = () => it.exists && void run("open_shelf", it.id);
  b.oncontextmenu = (e) => {
    e.preventDefault();
    openMenu(it, e.clientX, e.clientY);
  };
  b.onkeydown = (e) => {
    if (e.key === "Delete") void update("remove_shelf", it.id);
    if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
      e.preventDefault();
      const r = b.getBoundingClientRect();
      openMenu(it, r.left + 8, r.bottom - 8);
    }
  };
  // pointer-drag out: past a few px with the button held starts a native file drag
  b.onpointerdown = (e) => {
    if (!dragOut || !it.exists || e.button !== 0) return;
    const [sx, sy] = [e.clientX, e.clientY];
    const stop = () => b.removeEventListener("pointermove", move);
    const move = (m: PointerEvent) => {
      if (Math.hypot(m.clientX - sx, m.clientY - sy) < DRAG_PX) return;
      stop();
      invoke("drag_out", { id: it.id }).catch((err) => {
        if (!String(err).includes("not supported")) return; // transient (file vanished, ...): try again next time
        dragOut = false; // unsupported here: keep Copy path / Show in folder
        draw();
      });
    };
    b.addEventListener("pointermove", move);
    b.addEventListener("pointerup", stop, { once: true });
  };
  return b;
}

function draw(): void {
  const grid = $("shelf-grid");
  grid.replaceChildren(...items.map(tile));
  $("shelf-empty").hidden = items.length > 0;
  $("shelf-hint").textContent = flash || (dragOut ? "Drag out, click to open, right-click for more" : "Click to open, right-click for more");
  ($("shelf-clear") as HTMLButtonElement).disabled = items.length === 0;
}

export async function showShelf(): Promise<void> {
  items = await invoke<Item[]>("list_shelf");
  draw();
}

export function initShelf(h: { dragging: (on: boolean) => void }): void {
  $("shelf-clear").onclick = () => void update("clear_shelf");
  $("tab-shelf").addEventListener("pointerdown", (e) => {
    if (!(e.target as HTMLElement).closest("#shelf-menu")) closeMenu();
  });
  void listen<Item[]>("shelf", (e) => {
    items = e.payload;
    draw();
  });
  void listen<string>("shelf-drag", (e) => h.dragging(e.payload === "enter"));
}
