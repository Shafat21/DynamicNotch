// Latest release info (version and download links) from GitHub's public API; the page works without it
// (the links fall back to the Releases page). No cookies, no tracking.
document.documentElement.classList.add("js");

const REPO = "Shafat21/DynamicNotch";
const pick = (assets, re) => assets.find((a) => re.test(a.name))?.browser_download_url;

fetch(`https://api.github.com/repos/${REPO}/releases/latest`)
  .then((r) => (r.ok ? r.json() : Promise.reject(r.status)))
  .then((rel) => {
    const ver = String(rel.tag_name || "").replace(/^v/, "");
    if (ver) document.querySelectorAll(".js-ver").forEach((e) => (e.textContent = ver));
    const set = (sel, url) => url && document.querySelectorAll(sel).forEach((a) => (a.href = url));
    set(".js-win", pick(rel.assets, /setup\.exe$/i));
    set(".js-deb", pick(rel.assets, /\.deb$/i));
    set(".js-rpm", pick(rel.assets, /\.rpm$/i));
    set(".js-appimage", pick(rel.assets, /\.AppImage$/i));
  })
  .catch(() => {});

// ---------- Pal in the hero notch ----------
// Faces and morphing ported from the app (src/pal-shapes.ts, src/pal.ts); keep the numbers in sync.
const SIDE = 4;
const f = (v) => String(+v.toFixed(2));
function stroke([x, y, angle, len, curve, w]) {
  const a0 = (angle * Math.PI) / 180, h = w / 2, L = len / 2;
  const th = (s) => a0 + curve * s;
  const at = (s, off) => {
    const t = th(s);
    const px = curve ? x + (Math.sin(t) - Math.sin(a0)) / curve : x + s * Math.cos(a0);
    const py = curve ? y - (Math.cos(t) - Math.cos(a0)) / curve : y + s * Math.sin(a0);
    return [px - off * Math.sin(t), py + off * Math.cos(t)];
  };
  const side = (s0, s1, off) => {
    const ds = s1 - s0, dt = curve * ds;
    const k = (dt ? ((4 / 3) * Math.tan(dt / 4)) / dt : 1 / 3) * ds * (1 - off * curve);
    const [ax, ay] = at(s0, off), [bx, by] = at(s1, off);
    return [ax + k * Math.cos(th(s0)), ay + k * Math.sin(th(s0)), bx - k * Math.cos(th(s1)), by - k * Math.sin(th(s1)), bx, by];
  };
  const cap = (s, from) => {
    const [cx, cy] = at(s, 0);
    const k = -(4 / 3) * Math.tan(Math.PI / 8) * h;
    return [0, 1].map((i) => {
      const a = from - (i * Math.PI) / 2, b = a - Math.PI / 2;
      return [cx + h * Math.cos(a) - k * Math.sin(a), cy + h * Math.sin(a) + k * Math.cos(a), cx + h * Math.cos(b) + k * Math.sin(b), cy + h * Math.sin(b) - k * Math.cos(b), cx + h * Math.cos(b), cy + h * Math.sin(b)];
    });
  };
  const s = (i) => -L + (len * i) / SIDE;
  const cmds = [];
  for (let i = 0; i < SIDE; i++) cmds.push(side(s(i), s(i + 1), h));
  cmds.push(...cap(L, th(L) + Math.PI / 2));
  for (let i = SIDE; i > 0; i--) cmds.push(side(s(i), s(i - 1), -h));
  cmds.push(...cap(-L, th(-L) - Math.PI / 2));
  const [mx, my] = at(-L, h);
  return `M ${f(mx)} ${f(my)} ${cmds.map((c) => `C ${c.map(f).join(" ")}`).join(" ")} Z`;
}
const eyes = ([x, y, ...rest], dy = 0) => ({ eyeL: stroke([x, y, ...rest]), eyeR: stroke([x + 34, y + dy, ...rest]) });
const PILL = eyes([33, 42, -90, 14, 0, 13]), TILTED = eyes([34, 40, -75, 14, 0, 13], 4), ROUND = eyes([33, 43, -90, 5, 0, 14]);
const ARC_UP = eyes([33, 38, 0, 26, 1 / 9, 7.5]), ARC_DOWN = eyes([33, 45, 0, 22, -1 / 9, 7]);
const SMALL_SMILE = stroke([50, 71, 0, 16, -1 / 7, 5]), WIDE_SMILE = stroke([50, 74, 0, 28, -1 / 11, 6]);
const TILTED_SMILE = stroke([53, 70, -20, 15, -1 / 8, 5]), FLAT = stroke([50, 69, 0, 13, 0, 5]);
const RING = stroke([50, 73.5, 0, 2 * Math.PI * 5.5, -1 / 5.5, 4.5]);
const FACES = {
  calm: { ...PILL, mouth: SMALL_SMILE, blush: 0.6 },
  happy: { ...ARC_UP, mouth: WIDE_SMILE, blush: 1 },
  listening: { ...TILTED, mouth: TILTED_SMILE, blush: 0.35 },
  thinking: { ...ROUND, mouth: FLAT, blush: 0 },
  "needs-you": { ...PILL, mouth: RING, blush: 0 },
  done: { ...ARC_UP, mouth: WIDE_SMILE, blush: 1 },
  napping: { ...ARC_DOWN, mouth: SMALL_SMILE, blush: 0 },
};
const TOKEN = /[A-Za-z]|-?(?:\d+\.?\d*|\.\d+)/g;
const lerpPath = (a, b, t) => { const tb = b.match(TOKEN); return a.match(TOKEN).map((x, i) => (/[A-Za-z]/.test(x) ? x : f(+x * (1 - t) + +tb[i] * t))).join(" "); };

const notch = document.querySelector(".notch");
if (notch) {
  const NS = "http://www.w3.org/2000/svg";
  const node = (tag, attrs, parent) => { const n = document.createElementNS(NS, tag); for (const k in attrs) n.setAttribute(k, attrs[k]); parent?.append(n); return n; };
  const svg = node("svg", { class: "pal", viewBox: "0 0 100 100", width: 26, height: 26 });
  node("feGaussianBlur", { stdDeviation: 3 }, node("filter", { id: "pal-blur", x: -1, y: -1, width: 3, height: 3 }, node("defs", {}, svg)));
  const blush = node("g", { class: "pal-blush", filter: "url(#pal-blur)", fill: "#ff6fa3" }, svg);
  node("circle", { cx: 21, cy: 61, r: 8 }, blush);
  node("circle", { cx: 79, cy: 61, r: 8 }, blush);
  const blink = node("g", {}, node("g", { class: "pal-eyes" }, svg));
  const eyeL = node("path", {}, blink), eyeR = node("path", {}, blink);
  const mouth = node("path", { class: "pal-mouth" }, svg);
  notch.querySelector(".n-pal").append(svg);
  const sideEl = notch.querySelector(".n-side");

  let face = { ...FACES.calm }, mood = "calm";
  const render = () => { eyeL.setAttribute("d", face.eyeL); eyeR.setAttribute("d", face.eyeR); mouth.setAttribute("d", face.mouth); blush.setAttribute("opacity", face.blush); };
  const morphTo = (m) => {
    const from = face, to = FACES[m], t0 = performance.now();
    const step = (now) => {
      const t = Math.min(1, (now - t0) / 300), e = 1 - (1 - t) ** 3;
      face = { eyeL: lerpPath(from.eyeL, to.eyeL, e), eyeR: lerpPath(from.eyeR, to.eyeR, e), mouth: lerpPath(from.mouth, to.mouth, e), blush: from.blush + (to.blush - from.blush) * e };
      render();
      if (t < 1) requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  };
  const doBlink = () => {
    if (mood === "calm") {
      const t0 = performance.now();
      const step = (now) => {
        const t = (now - t0) / 140, sc = t >= 1 ? 1 : 1 - 0.9 * Math.sin(Math.PI * Math.max(0, t));
        blink.setAttribute("transform", `matrix(1 0 0 ${sc.toFixed(3)} 0 ${(42 * (1 - sc)).toFixed(2)})`);
        if (t < 1) requestAnimationFrame(step);
      };
      requestAnimationFrame(step);
    }
    setTimeout(doBlink, 3000 + Math.random() * 3000);
  };

  const clock = () => new Date().toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  const wave = `<span class="n-wave">${[0, 1, 2, 3, 4].map((i) => `<i style="--i:${i}"></i>`).join("")}</span>`;
  // what the real pill shows in each mood
  const SCENES = [
    ["calm", () => clock(), 3200],
    ["thinking", () => `<span class="n-spin"></span>Claude Code <small>working</small>`, 3000],
    ["needs-you", () => `<span class="n-dot"></span>Codex <small>wants to run a command</small>`, 3000],
    ["done", () => `<span class="n-ok">✓</span>Claude Code <small>done</small>`, 2400],
    ["listening", () => `${wave}<small>Listening…</small>`, 2800],
    ["napping", () => `<small>zz</small> ${clock()}`, 2600],
  ];

  // animate width: measure the new content at auto width, then transition from the old width to it
  const show = ([m, html]) => {
    const w0 = notch.offsetWidth;
    sideEl.innerHTML = html();
    notch.style.width = "auto";
    const w1 = notch.offsetWidth;
    notch.style.width = `${w0}px`;
    void notch.offsetWidth;
    notch.style.width = `${w1}px`;
    if (m !== mood) { mood = m; svg.dataset.mood = m; notch.dataset.mood = m; morphTo(m); }
  };

  svg.dataset.mood = mood;
  render();
  sideEl.innerHTML = clock();
  setTimeout(doBlink, 2000);
  if (!matchMedia("(prefers-reduced-motion: reduce)").matches) {
    let i = 0;
    const next = () => {
      sideEl.classList.add("swap");
      setTimeout(() => { i = (i + 1) % SCENES.length; show(SCENES[i]); sideEl.classList.remove("swap"); setTimeout(next, SCENES[i][2]); }, 200);
    };
    setTimeout(next, SCENES[0][2]);
  } else setInterval(() => (sideEl.innerHTML = clock()), 30000);
}

// reveal sections as they scroll into view
const io = "IntersectionObserver" in window && new IntersectionObserver((entries) => {
  for (const e of entries) if (e.isIntersecting) (e.target.classList.add("in"), io.unobserve(e.target));
}, { threshold: 0.12 });
document.querySelectorAll(".reveal").forEach((el) => (io ? io.observe(el) : el.classList.add("in")));
