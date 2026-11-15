// Focus timer tile: click start/pause, wheel ±5 min while stopped, in memory only.
import { icon } from "./icons";

const pad = (n: number) => String(n).padStart(2, "0");

export function initFocus(el: HTMLElement, onDone: () => void): void {
  let minutes = 25;
  let left = minutes * 60; // seconds remaining
  let end = 0; // deadline in ms while running (timers drift, the clock doesn't)
  let timer = 0;
  let lastWheel = 0;

  const head = document.createElement("span");
  head.className = "tile-title focus-time";
  head.innerHTML = icon("timer", 18); // static SVG, no user text
  const time = document.createElement("span");
  head.append(time);
  const label = document.createElement("span");
  label.className = "tile-sub";
  el.append(head, label);

  const render = () => {
    time.textContent = `${pad(Math.floor(left / 60))}:${pad(left % 60)}`;
    label.textContent = timer ? "Focusing" : left < minutes * 60 ? "Paused" : "Focus";
    el.classList.toggle("running", timer !== 0);
  };
  const stop = () => {
    clearInterval(timer);
    timer = 0;
  };

  el.addEventListener("click", () => {
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
  });

  el.addEventListener(
    "wheel",
    (e) => {
      if (timer || e.timeStamp - lastWheel < 250) return; // trackpads fire many events per gesture
      lastWheel = e.timeStamp;
      minutes = Math.min(95, Math.max(5, minutes + (e.deltaY < 0 ? 5 : -5)));
      left = minutes * 60;
      render();
    },
    { passive: true },
  );

  render();
}
