// Pal, the assistant's face: inline SVG that morphs between FACES with one shared rAF loop.
// Continuous per-mood idle motion is CSS keyframes keyed on data-mood (styles.css), so a calm Pal
// costs only a blink timer; rAF runs just during morphs (300 ms) and blinks (140 ms).
import { moodFor, type MoodInput, type PalMood } from "./mood";
import { FACES, lerpPath } from "./pal-shapes";

export type Pal = { el: SVGSVGElement; setMood(m: PalMood): void; destroy(): void };

const NS = "http://www.w3.org/2000/svg";
const MORPH_MS = 300;
const BLINK_MS = 140;
const EYE_Y = 42; // blink squashes around the eyes' vertical centre

// ---------- shared animation loop ----------
const frames = new Set<(now: number) => boolean>(); // returns false when finished
let raf = 0;
function tick(now: number): void {
  for (const fr of frames) if (!fr(now)) frames.delete(fr);
  raf = frames.size ? requestAnimationFrame(tick) : 0;
}
function animate(fr: (now: number) => boolean): void {
  frames.add(fr);
  raf ||= requestAnimationFrame(tick);
}

// ---------- mood owner: pals created without a mood follow it ----------
const input: MoodInput = { bridgeOnline: true, connected: false, statuses: [], promptPending: false, reminderDue: false, listening: false, greeting: false, doneRecently: false, quietForMs: 0 };
let current = moodFor(input);
const followers = new Set<Pal>();

/** Merge new facts (Task 9: sessions/bridge) and push the resulting mood to every following Pal. */
export function updateMood(patch: Partial<MoodInput>): void {
  Object.assign(input, patch);
  const m = moodFor(input);
  if (m === current) return;
  current = m;
  for (const p of followers) p.setMood(m);
}

let greetTimer = 0;
/** Dashboard opened: happy for 1.5 s, then whatever applies. */
export function greet(): void {
  updateMood({ greeting: true });
  clearTimeout(greetTimer);
  greetTimer = window.setTimeout(() => updateMood({ greeting: false }), 1500);
}

// ---------- one face ----------
let uid = 0;
const el = <K extends keyof SVGElementTagNameMap>(tag: K, attrs: Record<string, string | number>, parent?: Element) => {
  const node = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, String(v));
  parent?.append(node);
  return node;
};

/** `mood` given: a fixed Pal you drive with setMood. Omitted: it follows updateMood/greet. */
export function createPal(size: number, mood?: PalMood): Pal {
  let shown: PalMood = mood ?? current;
  let face = { ...FACES[shown] };
  let morph: { from: typeof face; to: typeof face; t0: number } | null = null;
  let blinkT0 = -1;
  let blinkTimer = 0;

  const svg = el("svg", { class: "pal", viewBox: "0 0 100 100", width: size, height: size, "aria-hidden": "true" });
  const blurId = `pal-blur-${++uid}`;
  el("feGaussianBlur", { stdDeviation: 3 }, el("filter", { id: blurId, x: -1, y: -1, width: 3, height: 3 }, el("defs", {}, svg)));
  const blush = el("g", { class: "pal-blush", filter: `url(#${blurId})`, fill: "#ff6fa3" }, svg);
  el("circle", { cx: 21, cy: 61, r: 8 }, blush);
  el("circle", { cx: 79, cy: 61, r: 8 }, blush);
  const blink = el("g", {}, el("g", { class: "pal-eyes" }, svg)); // CSS animates .pal-eyes, JS the inner blink group
  const eyeL = el("path", {}, blink);
  const eyeR = el("path", {}, blink);
  const mouth = el("path", { class: "pal-mouth" }, svg);

  const render = () => {
    eyeL.setAttribute("d", face.eyeL);
    eyeR.setAttribute("d", face.eyeR);
    mouth.setAttribute("d", face.mouth);
    blush.setAttribute("opacity", String(face.blush));
  };

  const frame = (now: number): boolean => {
    if (morph) {
      const t = Math.min(1, Math.max(0, (now - morph.t0) / MORPH_MS));
      const e = 1 - (1 - t) ** 3; // ease-out
      const { from, to } = morph;
      face = { eyeL: lerpPath(from.eyeL, to.eyeL, e), eyeR: lerpPath(from.eyeR, to.eyeR, e), mouth: lerpPath(from.mouth, to.mouth, e), blush: from.blush + (to.blush - from.blush) * e };
      render();
      if (t === 1) morph = null;
    }
    if (blinkT0 >= 0) {
      const t = (now - blinkT0) / BLINK_MS;
      const s = t >= 1 ? 1 : 1 - 0.9 * Math.sin(Math.PI * Math.max(0, t));
      blink.setAttribute("transform", `matrix(1 0 0 ${s.toFixed(3)} 0 ${(EYE_Y * (1 - s)).toFixed(2)})`);
      if (t >= 1) blinkT0 = -1;
    }
    return morph !== null || blinkT0 >= 0;
  };

  // Blinks are allowed under reduced motion (spec §17.2); only calm blinks.
  const scheduleBlink = () => {
    clearTimeout(blinkTimer);
    if (shown !== "calm") return;
    blinkTimer = window.setTimeout(() => {
      blinkT0 = performance.now();
      animate(frame);
      scheduleBlink();
    }, 3000 + Math.random() * 3000);
  };

  const pal: Pal = {
    el: svg,
    setMood(m) {
      if (m === shown) return;
      shown = m;
      svg.dataset.mood = m;
      morph = { from: face, to: FACES[m], t0: performance.now() };
      animate(frame);
      scheduleBlink();
    },
    destroy() {
      followers.delete(pal);
      frames.delete(frame);
      clearTimeout(blinkTimer);
      svg.remove();
    },
  };

  svg.dataset.mood = shown;
  render();
  scheduleBlink();
  if (mood === undefined) followers.add(pal);
  return pal;
}
