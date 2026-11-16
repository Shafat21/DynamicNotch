// Focus timer: click start/pause, wheel ±5 min while stopped, in memory only.
// One timer shown in several places (Home strip chip, Timer tab); every element drives the same state.
import { icon } from "./icons-line";

const pad = (n: number) => String(n).padStart(2, "0");

export type FocusState = { left: number; total: number; running: boolean; end: number };
export type Focus = { state(): FocusState; toggle(): void; reset(): void };

/** `onChange` runs on every tick and change (the compact Pomodoro card follows the same timer). */
export function initFocus(els: HTMLElement[], onDone: () => void, onChange: () => void = () => {}): Focus {
  let minutes = 25;
  let left = minutes * 60; // seconds remaining
  let end = 0; // deadline in ms while running (timers drift, the clock doesn't)
  let timer = 0;
  let lastWheel = 0;

  const parts = els.map((el) => {
    const ic = document.createElement("span");
    ic.className = "focus-ic";
    ic.dataset.badge = "focus";
    ic.innerHTML = icon("timer", 18); // static SVG, no user text
    const label = document.createElement("span");
    label.className = "focus-label";
    const time = document.createElement("span");
    time.className = "focus-time";
    el.append(ic, label, time);
    return { el, label, time };
  });

  const render = () => {
    for (const p of parts) {
      p.time.textContent = `${pad(Math.floor(left / 60))}:${pad(left % 60)}`;
      p.label.textContent = timer ? "Focusing" : left < minutes * 60 ? "Paused" : "Focus";
      p.el.classList.toggle("running", timer !== 0);
    }
    onChange();
  };
  const stop = () => {
    clearInterval(timer);
    timer = 0;
  };

  const toggle = () => {
    if (timer) stop();
    else {
      end = Date.now() + left * 1000;
      timer = window.setInterval(() => {
        left = Math.max(0, Math.ceil((end - Date.now()) / 1000));
        if (left === 0) {
          stop();
          left = minutes * 60;
          onDone();
        }
        render();
      }, 250);
    }
    render();
  };

  const wheel = (e: WheelEvent) => {
    if (timer || e.timeStamp - lastWheel < 250) return; // trackpads fire many events per gesture
    lastWheel = e.timeStamp;
    minutes = Math.min(95, Math.max(5, minutes + (e.deltaY < 0 ? 5 : -5)));
    left = minutes * 60;
    render();
  };

  for (const el of els) {
    el.addEventListener("click", toggle);
    el.addEventListener("wheel", wheel, { passive: true });
  }
  render();
  return {
    state: () => ({ left, total: minutes * 60, running: timer !== 0, end }),
    toggle,
    reset: () => {
      stop();
      left = minutes * 60;
      render();
    },
  };
}
