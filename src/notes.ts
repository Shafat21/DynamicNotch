// Notes tab + Home quick input (spec §23.1). Plain text only: every note body goes through textContent / .value.
import { invoke } from "@tauri-apps/api/core";
import { icon } from "./icons-line";

type Note = { id: number; body: string; pinned: boolean; updatedAt: number };

const $ = (id: string) => document.getElementById(id)!;
const SAVE_MS = 500;
let query = "";
let notes: Note[] = []; // filtered by `query`
let selected: number | null = null; // null = a blank draft
let saved = ""; // stored body of the selected note (the list may be filtered, so it cannot be looked up there)
let timer = 0;
let confirmTimer = 0;
let loaded = 0; // guards against an older reply overwriting a newer one

const body = () => $("note-body") as HTMLTextAreaElement;
const firstLine = (s: string) => s.split("\n", 1)[0].slice(0, 80);

async function refresh(): Promise<void> {
  const n = ++loaded;
  const list = await invoke<Note[]>("list_notes", { query });
  if (n !== loaded) return;
  notes = list;
  draw();
}

function draw(): void {
  const list = $("notes-list");
  list.replaceChildren(
    ...notes.map((n) => {
      const b = document.createElement("button");
      b.className = "note-row" + (n.id === selected ? " on" : "");
      const t = document.createElement("span");
      t.className = "ell";
      t.textContent = firstLine(n.body);
      if (n.pinned) {
        const p = document.createElement("span");
        p.className = "note-pin";
        p.innerHTML = icon("pin", 12); // static SVG
        b.append(p);
      }
      b.append(t);
      b.onclick = () => select(n.id);
      return b;
    }),
  );
  if (!notes.length) {
    const e = document.createElement("div");
    e.className = "muted note-empty";
    e.textContent = query ? "No matching notes" : "No notes yet";
    list.append(e);
  }
  const cur = notes.find((n) => n.id === selected);
  $("note-pin").classList.toggle("on", !!cur?.pinned);
  ($("note-pin") as HTMLButtonElement).disabled = ($("note-del") as HTMLButtonElement).disabled = !cur;
}

async function select(id: number | null): Promise<void> {
  await flush();
  selected = id;
  saved = notes.find((n) => n.id === id)?.body.trim() ?? "";
  body().value = saved;
  resetDelete();
  draw();
  if (id === null) body().focus();
}

function resetDelete(): void {
  window.clearTimeout(confirmTimer);
  $("note-del").classList.remove("danger");
  $("note-del").title = "Delete";
}

let busy: Promise<void> | null = null;
let again = false;

/** Save the editor now (autosave timer fired, or focus/selection is leaving it). One save at a time; edits made meanwhile queue one more. */
export function flush(): Promise<void> {
  window.clearTimeout(timer);
  if (busy) {
    again = true;
    return busy;
  }
  busy = (async () => {
    do {
      again = false;
      await save();
    } while (again);
  })().finally(() => (busy = null));
  return busy;
}

async function save(): Promise<void> {
  const text = body().value.trim();
  if (!text || text === saved) return; // unchanged: don't bump updated_at
  try {
    if (selected === null) {
      const all = await invoke<Note[]>("add_note", { body: text });
      selected = Math.max(...all.map((n) => n.id)); // AUTOINCREMENT: the new note has the highest id
    } else {
      await invoke("update_note", { id: selected, body: text });
    }
    saved = text;
    await refresh();
  } catch (e) {
    console.error(e);
  }
}

export function showNotes(): void {
  void refresh();
}

export function initNotes(): void {
  const search = $("notes-search") as HTMLInputElement;
  search.oninput = () => {
    query = search.value;
    void refresh();
  };
  $("notes-new").onclick = () => void select(null);
  body().oninput = () => {
    window.clearTimeout(timer);
    timer = window.setTimeout(() => void flush(), SAVE_MS);
  };
  body().onblur = () => void flush();
  $("note-pin").onclick = async () => {
    const cur = notes.find((n) => n.id === selected);
    if (!cur) return;
    await invoke("pin_note", { id: cur.id, pinned: !cur.pinned }).catch(console.error);
    await refresh();
  };
  $("note-del").onclick = async () => {
    if (selected === null) return;
    if (!$("note-del").classList.contains("danger")) {
      $("note-del").classList.add("danger"); // two-step: a second click within 3 s deletes
      $("note-del").title = "Click again to delete";
      confirmTimer = window.setTimeout(resetDelete, 3000);
      return;
    }
    window.clearTimeout(timer);
    await invoke("delete_note", { id: selected }).catch(console.error);
    selected = null;
    saved = "";
    body().value = "";
    resetDelete();
    await refresh();
  };
  // Home card: Enter jots a note
  const quick = $("notes-quick") as HTMLInputElement;
  quick.onkeydown = async (e) => {
    if (e.key !== "Enter" || !quick.value.trim()) return;
    e.preventDefault();
    try {
      await invoke("add_note", { body: quick.value });
      quick.value = "";
      await refresh();
    } catch (err) {
      console.error(err);
    }
  };
  void refresh();
}
