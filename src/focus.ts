// Focus timer: click start/pause, wheel ±5 min while stopped, in memory only.
// One timer shown in several places (Home strip chip, Timer tab); every element drives the same state.
import { icon } from "./icons-line";

const pad = (n: number) => String(n).padStart(2, "0");
const PRESETS = [15, 25, 45, 60];
const RING_R = 62;
const RING_C = 2 * Math.PI * RING_R;

export type FocusState = { left: number; total: number; running: boolean; end: number };
export type Focus = { state(): FocusState; toggle(): void; reset(): void };

const endClock = (ms: number) =>
  new Date(ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });

/** `onChange` runs on every tick and change (the compact Pomodoro card follows the same timer). */
export function initFocus(els: HTMLElement[], onDone: () => void, onChange: () => void = () => {}): Focus {
  let minutes = 25;
  let left = minutes * 60; // seconds remaining
  let end = 0; // deadline in ms while running (timers drift, the clock doesn't)
  let timer = 0;
  let lastWheel = 0;

  const stop = () => {
    clearInterval(timer);
    timer = 0;
  };

  const setMinutes = (m: number) => {
    if (timer) return;
    minutes = Math.min(95, Math.max(5, m));
    left = minutes * 60;
    render();
  };

  const reset = () => {
    stop();
    left = minutes * 60;
    render();
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
    setMinutes(minutes + (e.deltaY < 0 ? 5 : -5));
  };

  // Home strip chips ( everything except #timer-big )
  const chips = els.filter((el) => el.id !== "timer-big").map((el) => {
    const ic = document.createElement("span");
    ic.className = "focus-ic";
    ic.dataset.badge = "focus";
    ic.innerHTML = icon("timer", 18); // static SVG, no user text
    const label = document.createElement("span");
    label.className = "focus-label";
    const time = document.createElement("span");
    time.className = "focus-time";
    el.append(ic, label, time);
    el.addEventListener("click", toggle);
    el.addEventListener("wheel", wheel, { passive: true });
    return { el, label, time };
  });

  // Rich Timer tab UI (#tab-timer)
  const big = els.find((el) => el.id === "timer-big");
  let tabRefs: {
    root: HTMLElement;
    arc: SVGCircleElement;
    time: HTMLElement;
    status: HTMLElement;
    sub: HTMLElement;
    playBtn: HTMLButtonElement;
    playIc: HTMLElement;
    playTxt: HTMLElement;
    resetBtn: HTMLButtonElement;
    minusBtn: HTMLButtonElement;
    plusBtn: HTMLButtonElement;
    presetBtns: { m: number; btn: HTMLButtonElement }[];
  } | null = null;

  if (big) {
    const page = big.closest("#tab-timer") as HTMLElement | null;
    const host = page ?? big.parentElement!;
    host.innerHTML = `
      <div class="tm-view">
        <div class="tm-dial" id="timer-big" role="button" tabindex="0" title="Click to start or pause · scroll to change by 5 min">
          <svg class="tm-svg" viewBox="0 0 140 140" aria-hidden="true">
            <circle class="tm-track" cx="70" cy="70" r="${RING_R}" />
            <circle class="tm-arc" cx="70" cy="70" r="${RING_R}" stroke-dasharray="${RING_C.toFixed(2)}" stroke-dashoffset="0" />
          </svg>
          <div class="tm-center">
            <span class="tm-badge"><span class="focus-ic" data-badge="focus">${icon("timer", 13)}</span><span class="tm-status">Focus</span></span>
            <div class="focus-time tm-time">25:00</div>
          </div>
        </div>
        <div class="tm-side">
          <div class="tm-head">
            <div class="tm-title">Focus Session</div>
            <div class="tm-sub muted small">25 min session</div>
          </div>
          <div class="tm-presets" role="group" aria-label="Session length"></div>
          <div class="tm-actions">
            <button type="button" class="btn primary tm-play"><span class="tm-play-ic"></span><span class="tm-play-txt">Start</span></button>
            <div class="tm-step">
              <button type="button" class="action-btn tm-minus" title="−5 minutes" aria-label="Minus 5 minutes">−5</button>
              <button type="button" class="action-btn tm-plus" title="+5 minutes" aria-label="Plus 5 minutes">+5</button>
            </div>
            <button type="button" class="action-btn tm-reset" title="Reset timer" aria-label="Reset timer">${icon("restart", 15)}</button>
          </div>
        </div>
      </div>
    `;
    const root = host.querySelector<HTMLElement>(".tm-view")!;
    const dial = host.querySelector<HTMLElement>("#timer-big")!;
    const presetsEl = host.querySelector<HTMLElement>(".tm-presets")!;
    const presetBtns = PRESETS.map((m) => {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = "opt tm-preset";
      btn.textContent = `${m}m`;
      btn.onclick = () => setMinutes(m);
      presetsEl.append(btn);
      return { m, btn };
    });

    const playBtn = host.querySelector<HTMLButtonElement>(".tm-play")!;
    const resetBtn = host.querySelector<HTMLButtonElement>(".tm-reset")!;
    const minusBtn = host.querySelector<HTMLButtonElement>(".tm-minus")!;
    const plusBtn = host.querySelector<HTMLButtonElement>(".tm-plus")!;

    dial.addEventListener("click", toggle);
    dial.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        toggle();
      }
    });
    root.addEventListener("wheel", wheel, { passive: true });
    playBtn.onclick = toggle;
    resetBtn.onclick = reset;
    minusBtn.onclick = () => setMinutes(minutes - 5);
    plusBtn.onclick = () => setMinutes(minutes + 5);

    tabRefs = {
      root,
      arc: host.querySelector<SVGCircleElement>(".tm-arc")!,
      time: host.querySelector<HTMLElement>(".tm-time")!,
      status: host.querySelector<HTMLElement>(".tm-status")!,
      sub: host.querySelector<HTMLElement>(".tm-sub")!,
      playBtn,
      playIc: host.querySelector<HTMLElement>(".tm-play-ic")!,
      playTxt: host.querySelector<HTMLElement>(".tm-play-txt")!,
      resetBtn,
      minusBtn,
      plusBtn,
      presetBtns,
    };
  }

  function render(): void {
    const total = minutes * 60;
    const running = timer !== 0;
    const paused = !running && left < total;
    const clock = `${pad(Math.floor(left / 60))}:${pad(left % 60)}`;
    const statusText = running ? "Focusing" : paused ? "Paused" : "Focus";

    for (const p of chips) {
      p.time.textContent = clock;
      p.label.textContent = statusText;
      p.el.classList.toggle("running", running);
    }

    if (tabRefs) {
      tabRefs.root.classList.toggle("running", running);
      tabRefs.root.classList.toggle("paused", paused);
      tabRefs.time.textContent = clock;
      tabRefs.status.textContent = statusText;
      const frac = total > 0 ? (total - left) / total : 0;
      // When idle at 0% elapsed, show a full subtle ring; while running/paused, show remaining arc
      const remFrac = total > 0 ? left / total : 1;
      tabRefs.arc.style.strokeDashoffset = `${(RING_C * (1 - remFrac)).toFixed(2)}`;
      tabRefs.sub.textContent = running
        ? `Ends at ${endClock(end)} · ${minutes}m session`
        : paused
          ? `${Math.ceil(left / 60)} min left of ${minutes}m`
          : `${minutes} min session · scroll to adjust`;
      tabRefs.playIc.innerHTML = icon(running ? "pause" : "play", 14);
      tabRefs.playTxt.textContent = running ? "Pause" : paused ? "Resume" : "Start";
      tabRefs.resetBtn.disabled = !running && left === total;
      tabRefs.minusBtn.disabled = running || minutes <= 5;
      tabRefs.plusBtn.disabled = running || minutes >= 95;
      for (const { m, btn } of tabRefs.presetBtns) {
        btn.classList.toggle("on", minutes === m);
        btn.disabled = running;
      }
      void frac;
    }
    onChange();
  }

  render();
  return {
    state: () => ({ left, total: minutes * 60, running: timer !== 0, end }),
    toggle,
    reset,
  };
}
