// Compact notch: Pomodoro card, driving the one shared focus timer (focus.ts). Rounds / long breaks: later.
import { icon } from "../icons-line";

export type PomodoroState = { minutes: number; label: string; remaining: string; endsAt: string; running: boolean; fraction: number; onToggle(): void; onReset(): void };

const node = (tag: string, cls: string, text = "") => Object.assign(document.createElement(tag), { className: cls, textContent: text });

export function renderPomodoro(el: HTMLElement, s: PomodoroState): void {
  if (el.dataset.built !== "pomodoro") {
    const left = node("div", "cp-left");
    left.append(node("div", "cp-label muted small"), node("div", "cp-time"), node("div", "cp-ends small"));
    const bar = node("div", "cp-bar");
    bar.append(node("i", "cp-fill"));
    left.append(bar);
    const right = node("div", "cp-right");
    const toggle = node("button", "btn sm cp-toggle") as HTMLButtonElement;
    toggle.type = "button";
    const reset = node("button", "cm-btn cp-reset") as HTMLButtonElement;
    reset.type = "button";
    reset.title = "Reset";
    reset.innerHTML = icon("restart", 16); // static SVG
    right.append(toggle, reset);
    const wrap = node("div", "cp");
    wrap.append(left, right);
    el.replaceChildren(wrap);
    el.dataset.built = "pomodoro";
  }
  const q = <T extends Element>(sel: string) => el.querySelector<T>(sel)!;
  q(".cp-label").textContent = s.label;
  q(".cp-time").textContent = s.remaining;
  q(".cp-ends").textContent = s.running ? `ends ${s.endsAt}` : `${s.minutes} min session`;
  q<HTMLElement>(".cp-fill").style.width = `${Math.round(s.fraction * 100)}%`;
  const toggle = q<HTMLButtonElement>(".cp-toggle");
  toggle.textContent = s.running ? "Pause" : s.fraction > 0 ? "Resume" : "Start";
  toggle.onclick = (e) => (e.stopPropagation(), s.onToggle());
  q<HTMLButtonElement>(".cp-reset").onclick = (e) => (e.stopPropagation(), s.onReset());
}
