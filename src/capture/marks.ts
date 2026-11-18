// Screenshot marks: the shapes, how they draw, and the selection maths (pure parts: scripts/capture.check.ts).
// Coordinates are CSS px of the overlay window; the export scales them to the image's pixels.

export type Pt = { x: number; y: number };
/** The window canvas or the offscreen one used for the export. */
export type Ctx2D = CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D;
export type Rect = { x: number; y: number; w: number; h: number };
/** A monitor: its area plus its display scaling (1.25 = 125 %). */
export type Monitor = Rect & { scale?: number };
export type Tool = "arrow" | "rect" | "pen" | "marker" | "text" | "blur";
export type Mark =
  | { kind: "arrow" | "rect" | "blur"; color: string; a: Pt; b: Pt }
  | { kind: "pen" | "marker"; color: string; pts: Pt[] }
  | { kind: "text"; color: string; at: Pt; text: string };

/** The rectangle between two drag points, any direction. */
export function rectOf(a: Pt, b: Pt): Rect {
  return { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), w: Math.abs(a.x - b.x), h: Math.abs(a.y - b.y) };
}

/** Where the toolbar goes: under the selection, else above it, else inside its bottom; always inside `area`
 *  (the monitor the selection is on: the overlay's box can reach past the screens). */
export function barPos(sel: Rect, bar: { w: number; h: number }, area: Rect, gap = 8): Pt {
  const lo = (a: number) => a + 8;
  const hiX = area.x + area.w - bar.w - 8, hiY = area.y + area.h - bar.h - 8;
  const x = Math.min(Math.max(lo(area.x), sel.x + sel.w - bar.w), hiX);
  const below = sel.y + sel.h + gap;
  const above = sel.y - gap - bar.h;
  const y = below <= hiY ? below : above >= lo(area.y) ? above : Math.min(sel.y + sel.h, area.y + area.h) - bar.h - gap;
  return { x: Math.max(lo(area.x), x), y: Math.min(Math.max(lo(area.y), y), hiY) };
}

/** The monitor holding most of `r` (by its centre), else the first; `mons` empty → the whole window. */
export function monitorOf(r: Rect | Pt, mons: Rect[], win: Rect): Rect {
  const c = "w" in r ? { x: r.x + r.w / 2, y: r.y + r.h / 2 } : r;
  return mons.find((m) => c.x >= m.x && c.x < m.x + m.w && c.y >= m.y && c.y < m.y + m.h) ?? mons[0] ?? win;
}

/** A region of the screen image pixelated into ~`cell` px blocks (the Blur tool: unreadable, not just soft). */
export function pixelate(ctx: Ctx2D, src: CanvasImageSource, scale: number, r: Rect, cell = 10): void {
  if (r.w < 1 || r.h < 1) return;
  const cw = Math.max(1, Math.round(r.w / cell)), ch = Math.max(1, Math.round(r.h / cell));
  const tiny = new OffscreenCanvas(cw, ch);
  const t = tiny.getContext("2d")!;
  t.imageSmoothingEnabled = true;
  t.drawImage(src, r.x * scale, r.y * scale, r.w * scale, r.h * scale, 0, 0, cw, ch);
  ctx.save();
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(tiny, 0, 0, cw, ch, r.x, r.y, r.w, r.h);
  ctx.restore();
}

export function drawMark(ctx: Ctx2D, m: Mark, src: CanvasImageSource, scale: number): void {
  ctx.save();
  ctx.strokeStyle = ctx.fillStyle = m.color;
  ctx.lineCap = ctx.lineJoin = "round";
  if (m.kind === "rect") {
    const r = rectOf(m.a, m.b);
    ctx.lineWidth = 3;
    ctx.strokeRect(r.x, r.y, r.w, r.h);
  } else if (m.kind === "blur") {
    pixelate(ctx, src, scale, rectOf(m.a, m.b));
  } else if (m.kind === "arrow") {
    const len = Math.hypot(m.b.x - m.a.x, m.b.y - m.a.y);
    if (len >= 2) {
      const ang = Math.atan2(m.b.y - m.a.y, m.b.x - m.a.x);
      const head = Math.min(18, Math.max(10, len * 0.3));
      ctx.lineWidth = 3.5;
      ctx.beginPath();
      ctx.moveTo(m.a.x, m.a.y);
      ctx.lineTo(m.b.x - Math.cos(ang) * head * 0.6, m.b.y - Math.sin(ang) * head * 0.6); // the shaft stops under the head
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(m.b.x, m.b.y);
      ctx.lineTo(m.b.x - Math.cos(ang - 0.45) * head, m.b.y - Math.sin(ang - 0.45) * head);
      ctx.lineTo(m.b.x - Math.cos(ang + 0.45) * head, m.b.y - Math.sin(ang + 0.45) * head);
      ctx.closePath();
      ctx.fill();
    }
  } else if (m.kind === "pen" || m.kind === "marker") {
    if (m.kind === "marker") {
      ctx.globalAlpha = 0.38;
      ctx.lineWidth = 16;
      ctx.lineCap = "butt";
    } else ctx.lineWidth = 3;
    ctx.beginPath();
    m.pts.forEach((p, i) => (i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y)));
    if (m.pts.length === 1) ctx.lineTo(m.pts[0].x + 0.1, m.pts[0].y);
    ctx.stroke();
  } else if (m.kind === "text") {
    ctx.font = "600 18px 'Segoe UI', system-ui, sans-serif";
    ctx.textBaseline = "top";
    ctx.lineWidth = 4;
    ctx.strokeStyle = "rgba(0,0,0,0.55)"; // a dark halo keeps text readable on any background
    ctx.strokeText(m.text, m.at.x, m.at.y);
    ctx.fillText(m.text, m.at.x, m.at.y);
  }
  ctx.restore();
}
