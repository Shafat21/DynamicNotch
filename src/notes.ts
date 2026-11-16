// Notes tab + Home quick input (spec §23.1). Plain text only: every note body goes through textContent / .value.
// Voice notes (Task 38): the mic buttons record through the dictation engine and insert the text at the caret.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { DictEvent } from "./dictation";
import { icon } from "./icons-line";

type Note = { id: number; body: string; pinned: boolean; updatedAt: number; call: boolean };

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
      if (n.call) {
        const c = document.createElement("span");
        c.className = "note-call";
        c.title = "Call notes";
        c.innerHTML = icon("phoneCall", 12); // static SVG
        b.append(c);
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

/** Show note `id` in the editor (a finished call note). */
export async function openNote(id: number): Promise<void> {
  query = "";
  ($("notes-search") as HTMLInputElement).value = "";
  await refresh();
  await select(id);
}

// ---------- voice notes ----------
type Field = HTMLTextAreaElement | HTMLInputElement;
const HOLD_MS = 300; // held longer: releasing finishes; a click keeps listening until the next click
let mic: { btn: HTMLElement; field: Field; sel: [number, number] } | null = null;
let micState: "idle" | "recording" | "transcribing" = "idle";
let downAt = 0;
let fresh = false; // this press started the recording

function setMic(state: typeof micState, title?: string): void {
  micState = state;
  for (const id of ["note-mic", "notes-quick-mic"]) {
    const b = $(id);
    const mine = mic?.btn === b;
    b.classList.toggle("mic-on", mine && state === "recording");
    b.classList.toggle("mic-busy", mine && state === "transcribing");
    b.title = (mine && title) || "Voice note: click to start / stop, or hold";
  }
}

/** The text at the caret (or the selection the field had when recording started), spaced from the word before. */
function insert(f: Field, text: string, saved: [number, number]): void {
  const [a, b] = document.activeElement === f ? [f.selectionStart ?? f.value.length, f.selectionEnd ?? f.value.length] : saved;
  const pad = a > 0 && !/\s$/.test(f.value.slice(0, a)) ? " " : "";
  f.setRangeText(pad + text, a, b, "end");
  f.dispatchEvent(new Event("input", { bubbles: true })); // the editor's autosave
}

function wireMic(btn: HTMLElement, field: Field): void {
  btn.onmousedown = (e) => e.preventDefault(); // keep the caret in the field
  btn.onpointerdown = (e) => {
    if (e.button !== 0 || micState !== "idle") return;
    mic = { btn, field, sel: [field.selectionStart ?? field.value.length, field.selectionEnd ?? field.value.length] };
    downAt = Date.now();
    fresh = true;
    setMic("recording", "Listening… click to stop");
    invoke("voice_note_start").catch(console.error);
  };
  btn.onpointerup = () => {
    if (mic?.btn !== btn) return;
    if (fresh) {
      fresh = false;
      if (Date.now() - downAt >= HOLD_MS) invoke("voice_note_stop").catch(console.error);
    } else if (micState === "recording") invoke("voice_note_stop").catch(console.error);
  };
}

function onDictation(e: DictEvent): void {
  if (!mic || (!e.page && e.state !== "idle")) return;
  if (e.state === "recording") return setMic("recording", "Listening… click to stop");
  if (e.state === "transcribing") return setMic("transcribing", "Transcribing…");
  if (e.state === "done" && e.text) insert(mic.field, e.text, mic.sel);
  const err = e.state === "error" ? e.error : undefined;
  setMic("idle");
  if (err) {
    mic.btn.title = err; // shortcut: the pill shows the error too when the island is closed
    mic.btn.classList.add("mic-err");
    const b = mic.btn;
    window.setTimeout(() => b.classList.remove("mic-err"), 3000);
  }
  mic = null;
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
  wireMic($("note-mic"), body());
  wireMic($("notes-quick-mic"), quick);
  void listen<DictEvent>("dictation", (e) => onDictation(e.payload));
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
