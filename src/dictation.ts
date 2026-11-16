// Dictation (spec §23.7): the island's compact listening pill and the optional Home card of recent
// transcripts (memory only). main.ts decides when the pill is on screen; transcripts are textContent only.
import { invoke } from "@tauri-apps/api/core";
import { icon } from "./icons-line";
import { createPal } from "./pal";

export type DictEvent = { state: "recording" | "transcribing" | "done" | "error" | "idle"; level?: number; ms?: number; locked?: boolean; escCancels?: boolean; text?: string; pasted?: boolean; error?: string };

const BARS = 18;
const KEEP = 10;
const $ = (id: string) => document.getElementById(id)!;
const levels: number[] = Array(BARS).fill(0);
const recent: string[] = [];
const pal = createPal(22, "listening");
let phase: DictEvent["state"] = "idle";
let bars: HTMLElement[] = [];

/** Recording or transcribing: the pill should be up (main.ts priority: below prompt / reminder, above notify). */
export const dictating = () => phase === "recording" || phase === "transcribing";

const span = (cls: string) => Object.assign(document.createElement("span"), { className: cls });

export function initDictation(): void {
  const wave = span("dict-wave");
  bars = levels.map(() => wave.appendChild(document.createElement("i")));
  const face = span("icon");
  face.append(pal.el);
  $("dictation").append(face, wave, span("dict-text"), span("dict-time"), span("dict-hint"));
  // only reaches the webview when the island has focus; on Windows the keyboard hook catches Esc anyway
  document.addEventListener("keydown", (e) => {
    if (e.key !== "Escape" || !dictating()) return;
    e.preventDefault();
    invoke("dictation_cancel").catch(console.error);
  });
  renderCard();
}

const mmss = (ms: number) => `${Math.floor(ms / 60000)}:${String(Math.floor(ms / 1000) % 60).padStart(2, "0")}`;

/** Applies a `dictation` event to the pill. Returns how long to keep the pill: 0 = until the next event, -1 = leave now. */
export function onDictation(e: DictEvent): number {
  const was = phase;
  phase = e.state;
  const box = $("dictation");
  const [text, time, hint] = (["dict-text", "dict-time", "dict-hint"] as const).map((c) => box.querySelector<HTMLElement>(`.${c}`)!);
  box.dataset.phase = e.state;
  if (e.state === "recording") {
    if (was !== "recording") levels.fill(0);
    levels.push(Math.min(1, Math.sqrt(e.level ?? 0) * 1.3)); // peak 0..1; sqrt lifts quiet speech
    levels.shift();
    bars.forEach((b, i) => (b.style.transform = `scaleY(${Math.max(0.12, levels[i])})`));
    pal.setMood("listening");
    text.textContent = "";
    time.textContent = mmss(e.ms ?? 0);
    hint.textContent = e.locked ? (e.escCancels ? "Hands-free · Esc to cancel" : "Hands-free") : e.escCancels ? "Esc to cancel" : "";
    return 0;
  }
  time.textContent = hint.textContent = "";
  if (e.state === "transcribing") {
    pal.setMood("thinking");
    text.textContent = "Transcribing…";
    return 0;
  }
  if (e.state === "done") {
    pal.setMood("done");
    const words = (e.text ?? "").trim();
    text.replaceChildren(Object.assign(document.createElement("b"), { textContent: e.pasted ? "Pasted" : "Copied" }), ` ${words}`);
    text.title = words;
    if (words) {
      recent.unshift(words);
      recent.length = Math.min(recent.length, KEEP);
      renderCard();
    }
    return 1500;
  }
  if (e.state === "error") {
    pal.setMood("broke");
    text.textContent = e.error || "Dictation failed";
    text.title = text.textContent;
    return 3000;
  }
  return -1; // idle: cancelled or too short
}

/** Home "Dictation" card: the last 10 transcripts with copy buttons. */
function renderCard(): void {
  const list = $("dict-list");
  if (!recent.length) {
    list.replaceChildren(Object.assign(span("muted small"), { textContent: "Hold the dictation hotkey and speak (double-tap for hands-free). Recent text shows here." }));
    return;
  }
  list.replaceChildren(
    ...recent.map((t) => {
      const row = document.createElement("div");
      row.className = "dict-row";
      const txt = span("ell");
      txt.textContent = t;
      txt.title = t;
      const copy = document.createElement("button");
      copy.className = "action-btn";
      copy.title = copy.ariaLabel = "Copy";
      copy.innerHTML = icon("clipboard", 14); // static icon markup
      copy.onclick = (ev) => {
        ev.stopPropagation();
        navigator.clipboard.writeText(t).then(
          () => {
            copy.innerHTML = icon("check", 14);
            window.setTimeout(() => (copy.innerHTML = icon("clipboard", 14)), 1200);
          },
          console.error,
        );
      };
      row.append(txt, copy);
      return row;
    }),
  );
}
