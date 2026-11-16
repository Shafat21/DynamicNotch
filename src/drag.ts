// Hold-and-drag the island to any screen edge (Rust moves the real window: commands island_drag_*), and the
// island's hit rect for whichever edge it sits on.
import { invoke } from "@tauri-apps/api/core";

export type Edge = "top" | "bottom" | "left" | "right";

const HOLD_MS = 450;
const CANCEL_PX = 6;

/** The part of the window that takes the pointer: the island (w x h, already scaled by k), `gap` px off its
 *  edge, plus a 15·k margin, in window CSS px. Top is the original centred formula, the others mirror it. */
export function hitRectFor(edge: Edge, w: number, h: number, viewW: number, viewH: number, gap: number, k: number): { x: number; y: number; w: number; h: number } {
  const m = 15 * k;
  switch (edge) {
    case "top":
      return { x: (viewW - w) / 2 - m, y: 0, w: w + 2 * m, h: gap + h + 2 * m };
    case "bottom":
      return { x: (viewW - w) / 2 - m, y: viewH - gap - h - m, w: w + 2 * m, h: gap + h + 2 * m };
    case "left":
      return { x: 0, y: (viewH - h) / 2 - m, w: gap + w + 2 * m, h: h + 2 * m };
    case "right":
      return { x: viewW - gap - w - 2 * m, y: (viewH - h) / 2 - m, w: gap + w + 2 * m, h: h + 2 * m };
  }
}

/** Press and hold `el` (not on a control) for HOLD_MS, then drag; moving more than CANCEL_PX first is a normal
 *  click. `anchor()` = the point (window CSS px) to keep under the pointer, i.e. the pill's centre. */
export function initDrag(el: HTMLElement, opts: { canStart: (target: Element) => boolean; anchor: () => { x: number; y: number }; onStart: () => void; onEnd: (placement: string | null) => void }): void {
  let timer = 0;
  let start: { x: number; y: number; id: number } | null = null;
  let dragging = false;
  let frame = 0;

  const cancel = () => {
    clearTimeout(timer);
    start = null;
  };
  const finish = () => {
    cancelAnimationFrame(frame);
    dragging = false;
    start = null;
    invoke<string>("island_drag_end").then(
      (p) => opts.onEnd(p),
      () => opts.onEnd(null), // Linux / failure: the window is put back where it was
    );
  };

  el.addEventListener("pointerdown", (e) => {
    if (e.button !== 0 || !(e.target instanceof Element) || !opts.canStart(e.target)) return;
    start = { x: e.clientX, y: e.clientY, id: e.pointerId };
    timer = window.setTimeout(() => {
      if (!start) return;
      dragging = true;
      el.setPointerCapture(start.id);
      opts.onStart();
      const a = opts.anchor();
      invoke("island_drag_start", { x: a.x, y: a.y }).catch(() => finish());
    }, HOLD_MS);
  });
  el.addEventListener("pointermove", (e) => {
    if (!dragging) {
      if (start && Math.hypot(e.clientX - start.x, e.clientY - start.y) > CANCEL_PX) cancel();
      return;
    }
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => void invoke("island_drag_move").catch(() => {}));
  });
  const up = () => (dragging ? finish() : cancel());
  el.addEventListener("pointerup", up);
  el.addEventListener("pointercancel", up);
  // the click that ends a drag must not also press whatever is under the pointer
  el.addEventListener("click", (e) => (el.dataset.dragged ? (e.stopPropagation(), e.preventDefault(), delete el.dataset.dragged) : 0), true);
  el.addEventListener("lostpointercapture", () => dragging && finish());
  el.addEventListener("pointerdown", () => delete el.dataset.dragged, true);
  const markDragged = () => (el.dataset.dragged = "1");
  el.addEventListener("pointerup", () => dragging && markDragged(), true);
}
