// Coding agent companion (spec §5.3, §17, §22.1): sessions on the Home Pal card and the Pal tab,
// blocking prompts (permission / AskUserQuestion) answered in the island, Pal's mood from all of it.
// Agents and their labels come from the bridge (`bridge.agents`); nothing here names one.
// Session messages, tool summaries and questions come from the agents: textContent only.
import { renderReply } from "./pal-reply";
import { invoke } from "@tauri-apps/api/core";
import type { PalMood } from "./mood";
import { createPal, updateMood, type Pal } from "./pal";
import { costText, level, limitText, live, pct, resetText, staleText, type Level, type Usage, type UsageWindow } from "./usage";

export type Status = "idle" | "working" | "needs-you" | "done" | "error";
export type Session = { id: string; agent: string; project: string; cwd: string; status: Status; message: string; updatedAt: number }; // updatedAt: unix s
export type Question = { question: string; header?: string; multiSelect?: boolean; options: { label: string; description?: string }[] };
export type Prompt = { id: number; sessionId: string; agent: string; project: string; kind: "permission" | "question"; toolName?: string; summary?: string; questions?: Question[]; queue: number };
export type Agent = { id: string; label: string; connected: boolean };
export type Bridge = { online: boolean; connected: boolean; agents?: Agent[]; error?: string };
type Answer = { kind: "allow" | "deny" | "dismiss" } | { kind: "answers"; picks: Record<string, string[]>; other: Record<string, string> };

const PROMPT_MS = 15_000; // Rust answers {} after this (bridge PROMPT_WAIT)
const STATUS_TEXT: Record<Status, string> = { working: "Working…", "needs-you": "Needs you", done: "✔ Done", idle: "Idle", error: "Something broke" };
const POP_MOOD: Record<Status, PalMood> = { working: "thinking", "needs-you": "needs-you", done: "done", idle: "calm", error: "broke" };

const $ = (id: string) => document.getElementById(id)!;
const st = {
  sessions: [] as Session[],
  bridge: { online: true, connected: false } as Bridge,
  queue: [] as { p: Prompt; at: number }[], // FIFO; [0] is on screen
  cardId: null as string | null, // session picked on the card (wheel / ‹ ›); null = newest
  replyTo: null as string | null, // session the Pal tab's message box sends to; null = newest
  note: "", // set_agent error
  usage: null as Usage | null, // sticky `usage`; null until a statusLine ever arrived (hide the widget)
};
let seen: Map<string, Status> | null = null; // statuses from the previous `sessions`; null before the first
let typing = false; // an island input has focus
let dictating = false; // dictation is recording
let heard = false; // 2 s after a session started working (UserPromptSubmit)
let doneRecently = false;
let heardTimer = 0;
let doneTimer = 0;

function el(tag: string, cls: string, text?: string): HTMLElement {
  const e = document.createElement(tag);
  e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

/** A button inside a clickable card: doesn't also open the card's tab. */
function button(cls: string, text: string, onClick: () => void, title = ""): HTMLButtonElement {
  const b = el("button", cls, text) as HTMLButtonElement;
  b.title = title;
  b.onclick = (e) => {
    e.stopPropagation();
    onClick();
  };
  return b;
}

function dot(s: Status): HTMLElement {
  const d = el("span", "st-dot");
  d.dataset.status = s;
  d.title = STATUS_TEXT[s];
  return d;
}

function ago(sec: number): string {
  const m = Math.floor((Date.now() / 1000 - sec) / 60);
  return m < 1 ? "now" : m < 60 ? `${m} min ago` : `${Math.floor(m / 60)} h ago`;
}

// ---------- mood ----------
function mood(): void {
  const newest = Math.max(0, ...st.sessions.map((s) => s.updatedAt));
  updateMood({
    bridgeOnline: st.bridge.online,
    connected: st.bridge.connected,
    statuses: st.sessions.map((s) => s.status),
    promptPending: st.queue.length > 0,
    listening: typing || heard || dictating,
    doneRecently,
    quietForMs: newest ? Date.now() - newest * 1000 : 0,
  });
}

export function setTyping(on: boolean): void {
  typing = on;
  mood();
}

export function setDictating(on: boolean): void {
  dictating = on;
  mood();
}

// ---------- sessions + bridge ----------
export function setSessions(list: Session[]): void {
  if (seen)
    for (const s of list) {
      if (seen.get(s.id) === s.status) continue;
      if (s.status === "working") {
        heard = true;
        clearTimeout(heardTimer);
        heardTimer = window.setTimeout(() => ((heard = false), mood()), 2000);
      }
      if (s.status === "done") {
        doneRecently = true;
        clearTimeout(doneTimer);
        doneTimer = window.setTimeout(() => ((doneRecently = false), mood()), 60_000);
      }
    }
  seen = new Map(list.map((s) => [s.id, s.status]));
  st.sessions = list;
  render();
}

export function setBridge(b: Bridge): void {
  st.bridge = b;
  if (b.connected) st.note = "";
  render();
}

export function setUsage(u: Usage): void {
  st.usage = u;
  renderCard();
  renderTab();
}

export const usage = () => st.usage;

export const anyWorking = () => st.sessions.some((s) => s.status === "working");

/** Fixed mood for a bridge pop whose `app` is a session's project; null for other pops. */
export function popMood(app: string): PalMood | null {
  const s = st.sessions.find((x) => x.project === app);
  return s ? POP_MOOD[s.status] : null;
}

/** The agent's display name from the bridge; its id until the first `bridge` event. */
function agentLabel(id: string): string {
  return st.bridge.agents?.find((a) => a.id === id)?.label ?? id;
}

/** Connecting an agent edits its config file: done in Settings, one row per agent. */
function connect(): void {
  invoke("open_settings").catch((err) => {
    st.note = String(err);
    render();
  });
}

/** Not connected / offline / no sessions: the status text, or null when sessions can be shown. */
function bridgeText(): string | null {
  const b = st.bridge;
  if (!b.online) return `Bridge offline: ${b.error ?? "unknown error"}`;
  if (!b.connected) return st.note || "Not connected";
  return st.sessions.length ? null : "No sessions yet";
}

function render(): void {
  renderCard();
  renderTab();
  mood();
}

/** Home Pal card: face, status dot, project, status line, last message, i/N. */
function renderCard(): void {
  const card = $("pal-card");
  const face = $("pal-card-face");
  const row = el("div", "pal-row");
  const col = el("span", "col");
  row.append(face, col);
  const text = bridgeText();
  if (text !== null) {
    const sub = el("span", "muted ell", st.bridge.online ? (st.bridge.connected ? "Connected" : st.note || "Not connected") : "Offline");
    sub.title = sub.textContent!;
    col.append(el("b", "ell", "Agents"), sub);
    const foot = st.bridge.online && !st.bridge.connected ? button("btn primary pal-connect", "Connect", connect, "Connect a coding agent in Settings") : Object.assign(el("div", "pal-msg muted", text), { title: "Start a connected agent in a terminal to see its session here" });
    return card.replaceChildren(row, ...(st.bridge.connected ? usageCard() : []), foot);
  }
  const list = st.sessions;
  const i = Math.max(0, list.findIndex((s) => s.id === st.cardId));
  const s = list[i];
  const name = el("span", "pal-name");
  name.append(dot(s.status), el("b", "ell", s.project || agentLabel(s.agent)));
  const sub = [agentLabel(s.agent), STATUS_TEXT[s.status], ...(list.length > 1 ? [`${i + 1}/${list.length}`] : [])].join(" · ");
  col.append(name, el("span", "muted ell", sub));
  const foot = el("div", "pal-foot");
  const msg = el("div", "pal-msg ell", s.message || "—");
  msg.title = s.message;
  foot.append(msg);
  if (list.length > 1) foot.append(button("bar-btn", "‹", () => step(-1), "Previous session"), button("bar-btn", "›", () => step(1), "Next session"));
  card.replaceChildren(row, ...usageCard(), foot);
}

// ---------- plan usage (statusLine rate_limits) ----------
const nowS = () => Date.now() / 1000;
const NS = "http://www.w3.org/2000/svg";

/** Small progress ring: accent, amber from 80 %, red from 95 % / limit reached. */
function ring(p: number, lv: Level): SVGSVGElement {
  const svg = document.createElementNS(NS, "svg");
  svg.setAttribute("viewBox", "0 0 20 20");
  svg.setAttribute("class", "usage-ring");
  svg.setAttribute("aria-hidden", "true");
  svg.dataset.level = lv;
  for (const [cls, frac] of [["track", 1], ["arc", Math.min(1, p / 100)]] as const) {
    const c = document.createElementNS(NS, "circle");
    Object.entries({ cx: 10, cy: 10, r: 8, pathLength: 100, class: cls, "stroke-dasharray": `${frac * 100} 100` }).forEach(([k, v]) => c.setAttribute(k, String(v)));
    svg.append(c);
  }
  return svg;
}

function bar(w: UsageWindow, lv: Level): HTMLElement {
  const b = el("div", "bar usage-bar");
  const fill = el("i", "");
  fill.style.width = `${Math.min(100, w.usedPercentage)}%`;
  b.dataset.level = lv;
  b.append(fill);
  return b;
}

/** Usage rows for the Home Pal card (empty when nothing is known or every window has reset). */
function usageCard(): HTMLElement[] {
  const u = st.usage;
  const now = nowS();
  const five = u && live(u.fiveHour, now);
  const week = u && live(u.sevenDay, now);
  if (!u || (!five && !week && !u.limitReached)) return [];
  const box = el("div", "usage");
  box.title = [u.limitMessage, staleText(u.updatedAt, now)].filter(Boolean).join("\n");
  if (u.limitReached || five) {
    const line = el("div", "usage-5h");
    line.append(ring(u.limitReached ? 100 : five!.usedPercentage, level(five?.usedPercentage ?? 100, u.limitReached)));
    if (u.limitReached) line.append(el("span", "usage-limit", limitText(u, now)));
    else line.append(el("b", "", `5h ${pct(five!)}`), el("span", "muted ell", `resets ${resetText(five!.resetsAt, now)}`));
    box.append(line);
  }
  if (week && !u.limitReached) { // the red text needs the room; the Pal tab still shows the week
    const wk = el("div", "usage-week");
    wk.append(el("span", "muted ell", `Week ${pct(week)} · ${resetText(week.resetsAt, now)}`), bar(week, level(week.usedPercentage)));
    box.append(wk);
  }
  return [box];
}

/** Pal tab: both windows, the session cost estimate, staleness and the limit banner. */
function usageBlock(): HTMLElement[] {
  const u = st.usage;
  if (!u) return [];
  const now = nowS();
  const block = el("div", "sess usage-block");
  const head = el("div", "sess-head");
  head.append(el("b", "", "Plan usage"), el("span", "spacer"));
  const stale = staleText(u.updatedAt, now);
  if (stale) head.append(el("span", "muted small", stale));
  block.append(head);
  if (u.limitReached) {
    const lim = el("div", "usage-limit small", limitText(u, now));
    lim.title = u.limitMessage ?? "";
    block.append(lim);
  }
  for (const [name, w] of [["5 hours", live(u.fiveHour, now)], ["Week", live(u.sevenDay, now)]] as const) {
    if (!w) continue;
    const lv = level(w.usedPercentage, u.limitReached && w.usedPercentage >= 100);
    const r = el("div", "usage-row");
    r.append(el("span", "usage-name small", name), bar(w, lv), el("b", "small usage-pct", pct(w)), el("span", "muted small ell usage-reset", `resets ${resetText(w.resetsAt, now)}`));
    block.append(r);
  }
  if (u.sessionCostUsd !== undefined) block.append(el("div", "muted small", costText(u.sessionCostUsd)));
  return block.childElementCount > 1 ? [block] : [];
}

function step(by: number): void {
  const list = st.sessions;
  if (list.length < 2) return;
  const i = Math.max(0, list.findIndex((s) => s.id === st.cardId));
  st.cardId = list[(i + by + list.length) % list.length].id;
  renderCard();
}

/** Pal tab: big face + bridge state on the left, every session on the right. */
function renderTab(): void {
  const text = bridgeText();
  const state = $("pal-state");
  const n = st.sessions.length;
  state.replaceChildren(el("b", "", "Coding agents"), el("span", "muted small", text ?? `${n} session${n === 1 ? "" : "s"}`));
  if (st.bridge.online && !st.bridge.connected) state.append(button("btn primary", "Connect an agent", connect));
  const target = st.sessions.find((s) => s.id === st.replyTo) ?? st.sessions[0] ?? null; // sessions come newest first
  $("pal-sessions").replaceChildren(
    ...usageBlock(),
    ...st.sessions.map((s) => {
      const row = el("div", "sess");
      const head = el("div", "sess-head");
      const when = el("span", "muted small", ago(s.updatedAt));
      when.title = new Date(s.updatedAt * 1000).toLocaleString();
      head.append(dot(s.status), el("b", "ell", s.project || agentLabel(s.agent)), el("span", "muted small", agentLabel(s.agent)), el("span", "muted small sess-status", STATUS_TEXT[s.status]), el("span", "spacer"), when);
      row.append(head);
      if (s.message) row.append(Object.assign(el("div", "sess-msg muted", s.message), { title: s.message }));
      row.title = `${s.cwd}
Click to message this session`;
      row.classList.toggle("picked", s.id === target?.id && st.sessions.length > 1);
      row.onclick = () => {
        st.replyTo = s.id;
        renderTab();
      };
      return row;
    }),
  );
  renderReply(target);
}

// ---------- prompts ----------
export const currentPrompt = (): Prompt | null => st.queue[0]?.p ?? null;

/** Lines the permission summary wraps to (max 3; the rest scrolls inside it).
 *  shortcut: estimated from a ~7.3 px monospace char and the row's fixed parts (~380 px); measure the DOM if it proves off. */
function summaryLines(summary: string, openW: number, textScale: number): number {
  const perLine = Math.max(20, Math.floor((openW - 380) / (7.3 * textScale)));
  const lines = summary.split("\n").reduce((n, l) => n + Math.max(1, Math.ceil(l.length / perLine)), 0);
  return Math.min(3, lines);
}

/** Base px height of the prompt state (spec decision): permission 150 (+1 line per extra summary line, up to 3,
 *  capped by openH), questions 230 + 30 per extra question (max 420). */
export function promptHeight(openW: number, openH: number, textScale: number): number {
  const p = currentPrompt();
  if (p?.kind === "question") return Math.min(420, 230 + 30 * Math.max(0, (p.questions?.length ?? 1) - 1));
  const extra = (summaryLines(p?.summary || p?.toolName || "", openW, textScale) - 1) * Math.ceil(12 * 1.4 * textScale);
  return Math.min(openH, 150 + extra);
}

/** Queues `p`; true when it is the one on screen now. */
export function addPrompt(p: Prompt): boolean {
  st.queue.push({ p, at: Date.now() });
  mood();
  if (st.queue.length === 1) renderPrompt();
  else updateCount();
  return st.queue.length === 1;
}

/** Drops prompt `id`; true when it was the one on screen (the next one, if any, is rendered). */
export function closePrompt(id: number): boolean {
  const i = st.queue.findIndex((q) => q.p.id === id);
  if (i < 0) return false;
  st.queue.splice(i, 1);
  mood();
  if (i === 0 && st.queue.length) renderPrompt();
  else updateCount();
  if (!st.queue.length) {
    promptPal?.destroy();
    promptPal = null;
  }
  return i === 0;
}

/** Fullscreen app in front: let the terminal ask for everything pending right away. */
export function dismissAll(): void {
  for (const { p } of st.queue) invoke("answer_prompt", { id: p.id, answer: { kind: "dismiss" } }).catch(() => {});
}

let promptPal: Pal | null = null;
let submit: () => void = () => {};
let busy = false;

function updateCount(): void {
  const c = document.querySelector<HTMLElement>("#prompt .prompt-count");
  if (c) c.textContent = st.queue.length > 1 ? `1 of ${st.queue.length}` : "";
}

function showError(msg: string): void {
  const e = $("prompt").querySelector<HTMLElement>(".prompt-err")!;
  e.textContent = msg;
  e.hidden = !msg;
}

function setBusy(on: boolean): void {
  busy = on;
  $("prompt").querySelectorAll<HTMLButtonElement | HTMLInputElement>("button, input").forEach((b) => (b.disabled = on));
}

async function answer(a: Answer): Promise<void> {
  const cur = currentPrompt();
  if (!cur || busy) return;
  setBusy(true);
  try {
    await invoke("answer_prompt", { id: cur.id, answer: a }); // `prompt-closed` follows and clears it
  } catch (err) {
    if (currentPrompt()?.id !== cur.id) return;
    setBusy(false);
    showError(String(err));
  }
}

export const dismissPrompt = () => answer({ kind: "dismiss" });

/** Renders the prompt on screen (queue[0]) into #prompt: permission or questions, countdown, "1 of N". */
function renderPrompt(): void {
  const { p, at } = st.queue[0];
  busy = false;
  promptPal?.destroy();
  promptPal = createPal(22, "needs-you");
  const head = el("div", "prompt-head");
  const face = el("span", "prompt-face");
  face.append(promptPal.el);
  const body = el("div", "prompt-body");
  const err = el("div", "prompt-err");
  err.hidden = true;

  if (p.kind === "permission") {
    // one row, so it fits the 150 px state below the tab bar: who + tool | summary | Deny Allow
    const who = el("span", "col prompt-who");
    const sub = el("span", "muted small prompt-sub");
    sub.append(el("span", "ell", `${agentLabel(p.agent)} wants ${p.toolName || "a tool"}`), el("span", "prompt-count"));
    who.append(el("b", "ell", p.project || agentLabel(p.agent)), sub);
    const sum = el("code", "prompt-sum", p.summary || p.toolName || ""); // wraps to 3 lines, then scrolls
    sum.title = p.summary ?? "";
    head.append(face, who, sum, button("btn", "Deny", () => answer({ kind: "deny" })), button("btn primary", "Allow", () => answer({ kind: "allow" })));
    submit = () => answer({ kind: "allow" });
  } else {
    head.append(face, el("b", "ell prompt-project", p.project || agentLabel(p.agent)));
    const qs = p.questions ?? [];
    const picked = qs.map(() => new Set<string>());
    const others: HTMLInputElement[] = [];
    head.append(
      el("span", "muted ell", "asks"),
      el("span", "spacer"),
      el("span", "muted small prompt-count"),
      button("btn", "Dismiss", dismissPrompt),
      button("btn primary", "Send", () => submit()),
    );
    qs.forEach((q, i) => {
      const box = el("div", "q");
      const title = el("div", "q-text");
      if (q.header) title.append(el("span", "lbl", q.header));
      title.append(el("span", "", q.question));
      if (q.multiSelect) title.append(el("span", "muted small", "· pick any"));
      const opts = el("div", "opts");
      const optButtons = q.options.map((o) => {
        const b = button("opt", o.label, () => {
          const on = !picked[i].has(o.label);
          if (!q.multiSelect) picked[i].clear(); // single-select: one pick
          if (on) picked[i].add(o.label);
          else picked[i].delete(o.label);
          optButtons.forEach((x, k) => {
            x.classList.toggle("on", picked[i].has(q.options[k].label));
            x.setAttribute("aria-pressed", String(picked[i].has(q.options[k].label)));
          });
          showError("");
        }, o.description ?? "");
        b.setAttribute("aria-pressed", "false");
        return b;
      });
      const other = document.createElement("input");
      other.placeholder = "Other…";
      other.name = `other-${i}`;
      other.setAttribute("aria-label", `Other answer: ${q.question}`);
      other.maxLength = 500;
      other.autocomplete = "off";
      other.oninput = () => showError("");
      others.push(other);
      opts.append(...optButtons, other);
      box.append(title, opts);
      body.append(box);
    });
    // Client-side check of what Rust validates too (question_response); its errors still show inline.
    submit = () => {
      const picks: Record<string, string[]> = {};
      const other: Record<string, string> = {};
      for (const [i, q] of qs.entries()) {
        const o = others[i].value.trim();
        const name = q.header || q.question;
        if (!q.multiSelect && picked[i].size && o) return showError(`${name}: pick one option or type an answer, not both`);
        if (!picked[i].size && !o) return showError(`${name}: pick an option or type an answer (or Dismiss)`);
        if (picked[i].size) picks[q.question] = [...picked[i]];
        if (o) other[q.question] = o;
      }
      answer({ kind: "answers", picks, other });
    };
  }

  const bar = el("div", "countdown");
  const fill = el("i", "");
  fill.style.animationDuration = `${PROMPT_MS}ms`;
  fill.style.animationDelay = `${at - Date.now()}ms`; // counts from the event's arrival
  bar.append(fill);
  body.append(err);
  $("prompt").replaceChildren(head, body, bar);
  updateCount();
}

// ---------- wiring ----------
export function initAssistant(): void {
  // wheel on the card switches session, never the page under it
  let lastWheel = 0;
  $("card-pal").addEventListener("wheel", (e) => {
    e.stopPropagation();
    if (st.sessions.length < 2 || e.timeStamp - lastWheel < 250) return;
    lastWheel = e.timeStamp;
    step(e.deltaY > 0 ? 1 : -1);
  }, { passive: true });

  // Enter = Allow/Send, Esc = Dismiss while a prompt is on screen (keys only arrive when the island has focus)
  document.addEventListener("keydown", (e) => {
    if ($("prompt").hidden || !currentPrompt() || e.repeat) return;
    if (e.key === "Escape") {
      e.preventDefault();
      dismissPrompt();
    } else if (e.key === "Enter" && !(e.target instanceof HTMLButtonElement)) {
      e.preventDefault();
      submit();
    }
  });

  window.setInterval(() => {
    mood(); // quietForMs → napping after 30 min
    renderTab(); // "3 min ago"
    if (st.usage) renderCard(); // usage windows expire at their reset time
  }, 60_000);
  render();
}
