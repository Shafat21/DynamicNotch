// Clipboard history tab (spec §23.3). Clip text is user data: textContent only. Click a row to copy it back.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { icon } from "./icons-line";

type Clip = { id: number; text: string; pinned: boolean; at: number };

const $ = (id: string) => document.getElementById(id)!;
let clips: Clip[] = [];
let query = "";
let paused = false;
let flash: number | null = null; // id of the row showing "Copied"
let flashTimer = 0;

const oneLine = (s: string) => s.replace(/\s+/g, " ").trim().slice(0, 140);

function iconButton(name: string, label: string, onClick: () => void, on = false): HTMLButtonElement {
  const b = document.createElement("button");
  b.type = "button";
  b.className = "action-btn" + (on ? " on" : "");
  b.title = label;
  b.innerHTML = icon(name, 14); // static SVG
  b.onclick = (e) => {
    e.stopPropagation();
    onClick();
  };
  return b;
}

function draw(): void {
  const q = query.toLowerCase();
  const shown = clips.filter((c) => c.text.toLowerCase().includes(q));
  const rows = shown.map((c) => {
    const row = document.createElement("div");
    row.className = "clip-row" + (c.id === flash ? " flash" : "");
    const main = document.createElement("button");
    main.type = "button";
    main.className = "clip-main";
    main.title = "Copy";
    const t = document.createElement("span");
    t.className = "ell";
    t.textContent = c.id === flash ? "Copied" : oneLine(c.text);
    main.append(t);
    main.onclick = () => void copy(c.id);
    row.append(
      main,
      iconButton("pin", c.pinned ? "Unpin" : "Pin", () => void act("pin_clip", { id: c.id, pinned: !c.pinned }), c.pinned),
      iconButton("trash", "Remove", () => void act("remove_clip", { id: c.id })),
    );
    return row;
  });
  const list = $("clip-list");
  list.replaceChildren(...rows);
  if (!shown.length) {
    const e = document.createElement("div");
    e.className = "muted clip-empty";
    e.textContent = query ? "No matching clips" : paused ? "Paused: copies are not recorded" : "Copy some text and it shows up here";
    list.append(e);
  }
  $("clip-state").hidden = !paused;
  $("clip-pause").innerHTML = icon(paused ? "play" : "pause", 16); // static SVG
  $("clip-pause").title = paused ? "Resume recording" : "Pause recording";
  ($("clip-clear") as HTMLButtonElement).disabled = !clips.some((c) => !c.pinned);
}

async function copy(id: number): Promise<void> {
  try {
    await invoke("copy_clip", { id });
  } catch (e) {
    console.error(e);
    return;
  }
  flash = id;
  window.clearTimeout(flashTimer);
  flashTimer = window.setTimeout(() => {
    flash = null;
    draw();
  }, 900);
  draw();
}

async function act(cmd: string, args?: Record<string, unknown>): Promise<void> {
  try {
    clips = await invoke<Clip[]>(cmd, args);
    draw();
  } catch (e) {
    console.error(e);
  }
}

export async function showClipboard(): Promise<void> {
  const [list, s] = await Promise.all([invoke<Clip[]>("list_clips"), invoke<{ clipPaused: boolean }>("get_settings")]);
  clips = list;
  paused = s.clipPaused;
  draw();
}

export function initClipboard(): void {
  const search = $("clip-search") as HTMLInputElement;
  search.oninput = () => {
    query = search.value;
    draw();
  };
  $("clip-clear").onclick = () => void act("clear_clips");
  $("clip-pause").onclick = async () => {
    paused = !paused;
    draw();
    await invoke("set_clip_paused", { paused }).catch(console.error);
  };
  void listen<Clip[]>("clips", (e) => {
    clips = e.payload;
    draw();
  });
  void listen<{ clipPaused: boolean }>("settings", (e) => {
    paused = e.payload.clipPaused;
    draw();
  });
}
