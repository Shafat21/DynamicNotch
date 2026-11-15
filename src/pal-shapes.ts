// Pal's face parts on a 100x100 viewBox. Each part is the filled outline of a thick, round-capped
// stroke along a straight or circular centerline, always emitted as M + 12 C + Z, so any two shapes
// of a part morph by interpolating numbers (CSS `d` transitions are Chromium-only).
import type { PalMood } from "./mood";

export type Part = "eyeL" | "eyeR" | "mouth";
export type Shape = string;

/** Centerline midpoint, tangent angle there (deg, y down), length, curvature (1/radius, 0 = straight), width. */
type Stroke = [x: number, y: number, angle: number, len: number, curve: number, w: number];

const SIDE = 4; // cubics per side: <= 90 degrees each, even for the full ring
const f = (v: number) => String(+v.toFixed(2));

function stroke([x, y, angle, len, curve, w]: Stroke): Shape {
  const a0 = (angle * Math.PI) / 180;
  const h = w / 2;
  const L = len / 2;
  const th = (s: number) => a0 + curve * s;
  // Point at arc length s from the midpoint, pushed `off` along the normal.
  const at = (s: number, off: number) => {
    const t = th(s);
    const px = curve ? x + (Math.sin(t) - Math.sin(a0)) / curve : x + s * Math.cos(a0);
    const py = curve ? y - (Math.cos(t) - Math.cos(a0)) / curve : y + s * Math.sin(a0);
    return [px - off * Math.sin(t), py + off * Math.cos(t)];
  };
  // One cubic along the offset curve from s0 to s1 (either direction); exact-circle arm length.
  const side = (s0: number, s1: number, off: number) => {
    const ds = s1 - s0;
    const dt = curve * ds;
    const k = (dt ? ((4 / 3) * Math.tan(dt / 4)) / dt : 1 / 3) * ds * (1 - off * curve);
    const [ax, ay] = at(s0, off);
    const [bx, by] = at(s1, off);
    return [ax + k * Math.cos(th(s0)), ay + k * Math.sin(th(s0)), bx - k * Math.cos(th(s1)), by - k * Math.sin(th(s1)), bx, by];
  };
  // Half circle of radius h around the centerline point at s, from angle `from`, clockwise in math terms.
  const cap = (s: number, from: number) => {
    const [cx, cy] = at(s, 0);
    const k = -(4 / 3) * Math.tan(Math.PI / 8) * h;
    return [0, 1].map((i) => {
      const a = from - (i * Math.PI) / 2;
      const b = a - Math.PI / 2;
      return [cx + h * Math.cos(a) - k * Math.sin(a), cy + h * Math.sin(a) + k * Math.cos(a), cx + h * Math.cos(b) + k * Math.sin(b), cy + h * Math.sin(b) - k * Math.cos(b), cx + h * Math.cos(b), cy + h * Math.sin(b)];
    });
  };
  const s = (i: number) => -L + (len * i) / SIDE;
  const cmds: number[][] = [];
  for (let i = 0; i < SIDE; i++) cmds.push(side(s(i), s(i + 1), h));
  cmds.push(...cap(L, th(L) + Math.PI / 2));
  for (let i = SIDE; i > 0; i--) cmds.push(side(s(i), s(i - 1), -h));
  cmds.push(...cap(-L, th(-L) - Math.PI / 2));
  const [mx, my] = at(-L, h);
  return `M ${f(mx)} ${f(my)} ${cmds.map((c) => `C ${c.map(f).join(" ")}`).join(" ")} Z`;
}

const EYE_GAP = 34; // eyeR = eyeL shifted right
const eyes = ([x, y, ...rest]: Stroke, dy = 0) => ({ eyeL: stroke([x, y, ...rest]), eyeR: stroke([x + EYE_GAP, y + dy, ...rest]) });

const PILL = eyes([33, 42, -90, 14, 0, 13]);
const TILTED = eyes([34, 40, -75, 14, 0, 13], 4); // head tilt: tops lean right, right eye sits lower
const ROUND = eyes([33, 43, -90, 5, 0, 14]);
const ARC_UP = eyes([33, 38, 0, 26, 1 / 9, 7.5]); // ^
const ARC_DOWN = eyes([33, 45, 0, 22, -1 / 9, 7]); // closed, like a v / ‿
const DOT = eyes([33, 43, -90, 0, 0, 12]);

const SMALL_SMILE = stroke([50, 71, 0, 16, -1 / 7, 5]);
const WIDE_SMILE = stroke([50, 74, 0, 28, -1 / 11, 6]);
const TILTED_SMILE = stroke([53, 70, -20, 15, -1 / 8, 5]); // check-mark-like
const FLAT = stroke([50, 69, 0, 13, 0, 5]);
const RING = stroke([50, 73.5, 0, 2 * Math.PI * 5.5, -1 / 5.5, 4.5]); // O: full loop, caps overlap, hole stays (nonzero)
const FROWN = stroke([50, 67, 0, 15, 1 / 7, 5]);

export const FACES: Record<PalMood, { eyeL: Shape; eyeR: Shape; mouth: Shape; blush: number }> = {
  calm: { ...PILL, mouth: SMALL_SMILE, blush: 0.6 },
  happy: { ...ARC_UP, mouth: WIDE_SMILE, blush: 1 },
  listening: { ...TILTED, mouth: TILTED_SMILE, blush: 0.35 },
  thinking: { ...ROUND, mouth: FLAT, blush: 0 },
  "needs-you": { ...PILL, mouth: RING, blush: 0 },
  done: { ...ARC_UP, mouth: WIDE_SMILE, blush: 1 },
  napping: { ...ARC_DOWN, mouth: SMALL_SMILE, blush: 0 },
  broke: { ...DOT, mouth: FROWN, blush: 0 },
};

const TOKEN = /[A-Za-z]|-?(?:\d+\.?\d*|\.\d+)/g;
const isCmd = (t: string) => /[A-Za-z]/.test(t);

/** Interpolate every number of two paths; throws if their command letters differ. */
export function lerpPath(a: Shape, b: Shape, t: number): Shape {
  const ta = a.match(TOKEN) ?? [];
  const tb = b.match(TOKEN) ?? [];
  if (ta.length !== tb.length) throw new Error("lerpPath: path structures differ");
  return ta
    .map((x, i) => {
      const y = tb[i];
      if (isCmd(x) || isCmd(y)) {
        if (x !== y) throw new Error("lerpPath: path structures differ");
        return x;
      }
      return f(+x * (1 - t) + +y * t);
    })
    .join(" ");
}
