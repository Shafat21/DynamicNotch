// Notetaker (Task 38): the "Record a call" buttons (island quick bar + Notes tab), the idle pill's red dot
// + timer, and the Notes tab's call strip (consent reminder, Stop / Cancel, progress). Rust does the work.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { icon } from "./icons-line";

export type CallEvent = {
  state: "idle" | "recording" | "processing" | "done" | "error";
  ms?: number; system?: boolean; warn?: string | null; step?: "transcribing" | "summarizing"; progress?: number; noteId?: number; error?: string;
};
type Hooks = { changed: () => void; error: (msg: string) => void; done: (noteId: number) => void };

const CONSENT = "Tell the people in the call you're recording.";
const $ = (id: string) => document.getElementById(id)!;
let cur: CallEvent = { state: "idle" };
let hooks: Hooks;

const clock = (ms: number) => {
  const t = Math.floor(ms / 1000);
  const [h, m, s] = [Math.floor(t / 3600), Math.floor(t / 60) % 60, String(t % 60).padStart(2, "0")];
  return h ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
};

/** Extra idle pill width for the badge, base px. */
export const recWidth = () => (cur.state === "recording" ? 72 : cur.state === "processing" ? 128 : 0);

const progressText = () => (cur.step === "summarizing" ? "Summarizing…" : `Transcribing ${Math.round((cur.progress ?? 0) * 100)}%`);

const el = (tag: string, cls: string, text = "") => Object.assign(document.createElement(tag), { className: cls, textContent: text });

function button(label: string, cls: string, onClick: () => void): HTMLButtonElement {
  const b = el("button", cls, label) as HTMLButtonElement;
  b.onclick = onClick;
  return b;
}

function render(): void {
  const rec = cur.state === "recording";
  const busy = cur.state === "processing";
  const badge = $("rec-badge");
  badge.hidden = !rec && !busy;
  badge.dataset.state = cur.state;
  badge.textContent = rec ? clock(cur.ms ?? 0) : busy ? progressText() : "";
  for (const [id, size] of [["btn-call", 18], ["notes-call", 16]] as const) {
    const b = $(id) as HTMLButtonElement;
    b.classList.toggle("rec", rec);
    b.disabled = busy;
    b.title = b.ariaLabel = rec ? "Stop recording the call" : busy ? "Processing the call…" : `Record a call. ${CONSENT}`;
    b.innerHTML = icon(rec ? "stop" : "phoneCall", size); // static icon markup
  }
  const strip = $("call-strip");
  strip.dataset.state = cur.state;
  strip.hidden = !rec && !busy && cur.state !== "error";
  if (rec) {
    const head = el("div", "row");
    const extra = [cur.system === false ? "mic only" : "", cur.warn ?? ""].filter(Boolean).join(" · ");
    head.append(el("span", "rec-dot"), el("b", "", `Recording ${clock(cur.ms ?? 0)}`), el("span", "muted grow ell", extra));
    head.append(button("Stop", "btn compact", stop), button("Cancel", "btn compact ghost", () => void invoke("notetaker_cancel").catch(console.error)));
    strip.replaceChildren(head, el("div", "muted", CONSENT));
  } else if (busy) {
    const bar = el("div", "call-bar");
    const fill = document.createElement("i");
    fill.style.width = `${Math.round((cur.progress ?? 0) * 100)}%`;
    bar.append(fill);
    strip.replaceChildren(el("div", "", cur.step === "summarizing" ? "Writing the summary with Claude Code…" : progressText()), bar);
  } else if (cur.state === "error") {
    const row = el("div", "row");
    row.append(el("span", "grow", cur.error || "The call couldn't be processed"), button("OK", "btn compact ghost", () => setCall({ state: "idle" })));
    strip.replaceChildren(row);
  }
}

function stop(): void {
  invoke("notetaker_stop").catch(console.error);
}

function toggle(): void {
  if (cur.state === "recording") return stop();
  if (cur.state === "processing") return;
  invoke("notetaker_start").catch((e) => setCall({ state: "error", error: String(e) })); // pop + the Notes strip
}

/** Applies a `notetaker` event (or the snapshot's last one). */
export function setCall(e: CallEvent): void {
  const was = cur.state;
  cur = e;
  render();
  hooks.changed();
  if (e.state === "done" && was === "processing" && e.noteId) hooks.done(e.noteId);
  if (e.state === "error" && was !== "error") hooks.error(e.error || "The call couldn't be processed");
}

export function initNotetaker(h: Hooks): void {
  hooks = h;
  $("btn-call").onclick = toggle;
  $("notes-call").onclick = toggle;
  render();
  void listen<CallEvent>("notetaker", (e) => setCall(e.payload));
}
