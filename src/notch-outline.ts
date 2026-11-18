// The notch's visible edge as an SVG path: the island glow travels it (main.ts geometry()). Pure: scripts/notch.check.ts.

/** SVG path of the notch's visible edge in the island's box (body `w` x `h` plus an ear `s` wide on each side), the
 *  same curves the island's mask cuts: left ear arc from the screen edge, down the side, round the bottom corner,
 *  along the bottom, up the other side, out the right ear. On the idle pill the ear (s) reaches below where the
 *  bottom corner starts (h - r): there the ear runs straight into the corner arc, at the point where they meet. */
export function notchOutline(w: number, h: number, r: number, s: number): string {
  const W = w + 2 * s;
  const n = (v: number) => Math.round(v * 10) / 10;
  const d = Math.max(0, s - (h - r)); // how far the ear reaches into the corner's height
  const dx = r - Math.sqrt(Math.max(0, r * r - d * d)); // the corner arc's inset at the ear's bottom (0 when d = 0)
  const y = Math.max(s, h - r); // where the side meets the corner arc
  return [
    `M 0 0`,
    `A ${n(s)} ${n(s)} 0 0 1 ${n(s)} ${n(s)}`,
    `L ${n(s + dx)} ${n(y)}`,
    `A ${n(r)} ${n(r)} 0 0 0 ${n(s + r)} ${n(h)}`,
    `L ${n(W - s - r)} ${n(h)}`,
    `A ${n(r)} ${n(r)} 0 0 0 ${n(W - s - dx)} ${n(y)}`,
    `L ${n(W - s)} ${n(s)}`,
    `A ${n(s)} ${n(s)} 0 0 1 ${n(W)} 0`,
  ].join(" ");
}
