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

// one live Pal: morphs between FACES, blinks while calm; CSS (.pal[data-mood]) adds the idle motion
const NS = "http://www.w3.org/2000/svg";
const node = (tag, attrs, parent) => { const n = document.createElementNS(NS, tag); for (const k in attrs) n.setAttribute(k, attrs[k]); parent?.append(n); return n; };
let palId = 0;
function createPal(size, start = "calm") {
  const svg = node("svg", { class: "pal", viewBox: "0 0 100 100", width: size, height: size, "aria-hidden": "true" });
  const blurId = `pal-blur-${++palId}`;
  node("feGaussianBlur", { stdDeviation: 3 }, node("filter", { id: blurId, x: -1, y: -1, width: 3, height: 3 }, node("defs", {}, svg)));
  const blush = node("g", { class: "pal-blush", filter: `url(#${blurId})`, fill: "#ff6fa3" }, svg);
  node("circle", { cx: 21, cy: 61, r: 8 }, blush);
  node("circle", { cx: 79, cy: 61, r: 8 }, blush);
  const blink = node("g", {}, node("g", { class: "pal-eyes" }, svg));
  const eyeL = node("path", {}, blink), eyeR = node("path", {}, blink);
  const mouth = node("path", { class: "pal-mouth" }, svg);
  let face = { ...FACES[start] }, mood = start;
  const render = () => { eyeL.setAttribute("d", face.eyeL); eyeR.setAttribute("d", face.eyeR); mouth.setAttribute("d", face.mouth); blush.setAttribute("opacity", face.blush); };
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
  svg.dataset.mood = mood;
  render();
  setTimeout(doBlink, 1500 + Math.random() * 2000);
  return {
    svg,
    get mood() { return mood; },
    setMood(m) {
      if (m === mood) return;
      mood = m;
      svg.dataset.mood = m;
      const from = face, to = FACES[m], t0 = performance.now();
      const step = (now) => {
        const t = Math.min(1, (now - t0) / 300), e = 1 - (1 - t) ** 3;
        face = { eyeL: lerpPath(from.eyeL, to.eyeL, e), eyeR: lerpPath(from.eyeR, to.eyeR, e), mouth: lerpPath(from.mouth, to.mouth, e), blush: from.blush + (to.blush - from.blush) * e };
        render();
        if (t < 1) requestAnimationFrame(step);
      };
      requestAnimationFrame(step);
    },
  };
}
const MOTION = !matchMedia("(prefers-reduced-motion: reduce)").matches;

const notch = document.querySelector(".notch");
if (notch) {
  const pal = createPal(26);
  notch.querySelector(".n-pal").append(pal.svg);
  const sideEl = notch.querySelector(".n-side");

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
    pal.setMood(m);
    notch.dataset.mood = m;
  };

  sideEl.innerHTML = clock();
  if (MOTION) {
    let i = 0;
    const next = () => {
      sideEl.classList.add("swap");
      setTimeout(() => { i = (i + 1) % SCENES.length; show(SCENES[i]); sideEl.classList.remove("swap"); setTimeout(next, SCENES[i][2]); }, 200);
    };
    setTimeout(next, SCENES[0][2]);
  } else setInterval(() => (sideEl.innerHTML = clock()), 30000);
}

// ---------- live screenshots ----------
// Each .live wraps a real screenshot; these builders lay the moving parts over it at the screenshot's
// own pixel coordinates (CSS scales them). Patches use colours sampled from the screenshot underneath.
const lv = (root, [x, y, w, h], cls = "", css = "") => {
  const d = document.createElement("div");
  d.className = `lv ${cls}`;
  d.style.cssText = `--x:${x};--y:${y};--w:${w};--h:${h};${css}`;
  root.append(d);
  return d;
};
const bar = (root, box, track, p) => { const b = lv(root, box, "lv-bar", `background:${track}`); b.innerHTML = "<i></i>"; b.firstChild.style.setProperty("--p", `${p}%`); return (v) => b.firstChild.style.setProperty("--p", `${v}%`); };
const text = (root, box, bg, css = "") => lv(root, box, "", `background:${bg};${css}`);
const rand = (a, b) => a + Math.random() * (b - a);
const walk = (v, step, lo, hi) => Math.min(hi, Math.max(lo, v + rand(-step, step)));
const mmss = (s) => `${Math.floor(s / 60)}:${String(Math.floor(s) % 60).padStart(2, "0")}`;
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

const LIVE = {
  music(r) {
    const LEN = 243;
    let t = 101;
    const seek = bar(r, [384, 303, 481, 8], "#5b4e65", 0);
    const el = text(r, [382, 324, 70, 28], "#533556", "--fs:20;font-weight:400;color:rgba(236,239,247,.6)");
    const rem = text(r, [790, 324, 77, 28], "#352b44", "--fs:20;font-weight:400;color:rgba(236,239,247,.6);justify-content:flex-end");
    const dots = text(r, [472, 256, 52, 24], "#48304e", ""); dots.classList.add("lv-dots");
    dots.innerHTML = [0, 1, 2, 3].map((i) => `<i style="--i:${i}"></i>`).join("");
    // the lyric already on screen, sung through karaoke-style
    const ly = text(r, [905, 245, 610, 110], "linear-gradient(90deg,#2b314b,#373753)", "--fs:30");
    ly.classList.add("lv-lyric");
    ly.innerHTML = "<span>Waiting in a car, waiting for a ride in the dark</span>";
    let k = 0;
    return () => {
      t = t >= LEN ? 0 : t + 1;
      seek((t / LEN) * 100);
      el.textContent = mmss(t);
      rem.textContent = `-${mmss(LEN - t)}`;
      k = (k + 14) % 128;
      ly.firstChild.style.setProperty("--k", `${Math.min(100, k)}%`);
    };
  },
  calendar(r) { lv(r, [1420, 378, 88, 48], "lv-ring", "--r:14px"); },
  system(r) {
    let cpu = 23, ram = 61, dn = 2.3, up = 303;
    const cpuBar = bar(r, [72, 318, 698, 8], "#414657", cpu), ramBar = bar(r, [830, 318, 698, 8], "#414657", ram);
    const cpuT = text(r, [68, 278, 90, 34], "#1e2337", "--fs:24");
    const ramT = text(r, [826, 278, 90, 34], "#1d2337", "--fs:24");
    const net = text(r, [826, 394, 190, 64], "#1f2539", "--fs:22;flex-direction:column;align-items:flex-start;justify-content:center;gap:12%;font-weight:600");
    return () => {
      cpu = walk(cpu, 9, 6, 48); ram = walk(ram, 0.8, 58, 65); dn = walk(dn, 1.2, 0.2, 6.5); up = walk(up, 120, 40, 700);
      cpuBar(cpu); ramBar(ram);
      cpuT.textContent = `${Math.round(cpu)}%`; ramT.textContent = `${Math.round(ram)}%`;
      net.innerHTML = `<span>↓ ${dn.toFixed(1)} MB/s</span><span>↑ ${Math.round(up)} KB/s</span>`;
    };
  },
  "agents-pal"(r) {
    const face = lv(r, [140, 160, 164, 164], "lv-face", "background:#0f121c");
    const pal = createPal(60, "thinking");
    face.append(pal.svg);
    let h5 = 42, wk = 63;
    const b5 = bar(r, [544, 232, 698, 12], "#4a5066", h5), bw = bar(r, [544, 276, 652, 12], "#4a5066", wk);
    const t5 = text(r, [1262, 222, 72, 36], "#282f49", "--fs:22;justify-content:flex-end;font-weight:700");
    const tw = text(r, [1216, 266, 72, 36], "#282f49", "--fs:22;justify-content:flex-end;font-weight:700");
    const work = text(r, [722, 398, 130, 32], "#28304a", "--fs:22;font-weight:400;color:rgba(236,239,247,.6)");
    lv(r, [419, 404, 16, 16], "lv-ring", "--r:50%;--c:143,176,255");
    let n = 0;
    return () => {
      n++;
      if (h5 >= 49) { h5 = 42; wk = 63; } else { h5 += 0.35; wk += 0.12; }
      b5(h5); bw(wk);
      t5.textContent = `${Math.round(h5)}%`; tw.textContent = `${Math.round(wk)}%`;
      work.textContent = `Working${".".repeat(n % 4)}`;
      if (n % 9 === 0) { pal.setMood("happy"); setTimeout(() => pal.setMood("thinking"), 1400); }
    };
  },
  "prompt-permission"(r) {
    const count = lv(r, [74, 248, 1452, 6], "lv-count", "background:#2f324e");
    count.innerHTML = "<i></i>";
    const flash = lv(r, [1399, 180, 105, 47], "lv-flash");
    const done = lv(r, [74, 154, 1452, 101], "lv-done");
    done.innerHTML = "<b>✓</b> Allowed · <span class='lv-dim'>cargo test is running in your terminal</span>";
    const cur = lv(r, [900, 330, 30, 40], "lv-cursor");
    cur.innerHTML = '<svg viewBox="0 0 16 22"><path d="M1 1v17l4.5-4.2 3 7 3-1.3-3-6.8H15z" fill="#fff" stroke="#000" stroke-width="1.2" stroke-linejoin="round"/></svg>';
    const move = (x, y) => { cur.style.setProperty("--x", x); cur.style.setProperty("--y", y); };
    (async function loop() {
      for (;;) {
        done.classList.remove("on"); move(900, 330);
        count.firstChild.animate([{ transform: "scaleX(1)" }, { transform: "scaleX(0.55)" }], { duration: 3000, fill: "forwards" });
        await wait(1500); move(1440, 196);
        await wait(1400); cur.classList.add("down"); flash.classList.add("on");
        await wait(160); cur.classList.remove("down"); flash.classList.remove("on");
        await wait(200); done.classList.add("on"); move(1180, 330);
        await wait(2600);
      }
    })();
  },
  dictation(r) {
    const wave = lv(r, [300, 37, 195, 44], "lv-wave", "background:linear-gradient(90deg,#4a324f,#332e45)");
    const bars = Array.from({ length: 18 }, () => wave.appendChild(document.createElement("i")));
    const time = text(r, [500, 45, 72, 34], "linear-gradient(90deg,#342f46,#4b3b56)", "--fs:24;font-weight:700");
    let s = 0, ph = 0;
    setInterval(() => {
      ph += 0.5;
      const loud = 0.5 + 0.5 * Math.sin(ph * 0.7);
      bars.forEach((b, i) => b.style.setProperty("--s", (0.15 + Math.abs(Math.sin(ph + i * 0.9)) * loud * rand(0.6, 1)).toFixed(2)));
    }, 90);
    return () => { s = s >= 15 ? 0 : s + 1; time.textContent = mmss(s); };
  },
  reminder(r) {
    const bell = lv(r, [92, 52, 88, 88], "lv-bell", "background:#5c4045");
    bell.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="#f5a623" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9"/><path d="M10.3 21a1.94 1.94 0 0 0 3.4 0"/></svg>';
    lv(r, [1185, 64, 123, 64], "lv-ring", "--r:32px;--c:143,176,255");
  },
  "settings-appearance"(r) {
    lv(r, [685, 209, 206, 142], "lv-mask"); // hides the ring baked into the screenshot (Midnight)
    const pick = lv(r, [690, 213, 198, 134], "lv-pick");
    const chips = [...document.querySelectorAll(".swatches li")];
    const ORDER = [1, 2, 3, 4, 5, 0]; // tiles in the first row: Graphite..Rose
    let i = 0;
    return () => {
      i = (i + 1) % ORDER.length;
      const k = ORDER[i];
      pick.style.setProperty("--x", 481 + 208 * k);
      chips.forEach((c, j) => c.classList.toggle("on", j === k));
    };
  },
};

if (MOTION) {
  const ticks = new Map();
  const seen = new IntersectionObserver((es) => es.forEach((e) => e.target.classList.toggle("vis", e.isIntersecting)), { rootMargin: "80px" });
  document.querySelectorAll(".live").forEach((root) => {
    const tick = LIVE[root.dataset.live]?.(root);
    if (tick) ticks.set(root, tick);
    seen.observe(root);
  });
  setInterval(() => ticks.forEach((tick, root) => root.classList.contains("vis") && tick()), 1000);

  // preview: the pill opens into the dashboard, holds, closes; hovering holds it open
  const shot = document.querySelector(".shot-main");
  if (shot) {
    let hover = false, open = true;
    shot.addEventListener("pointerenter", () => { hover = true; shot.classList.remove("closed"); });
    shot.addEventListener("pointerleave", () => { hover = false; });
    setInterval(() => { if (hover) return; open = !open; shot.classList.toggle("closed", !open); }, 3600);
  }
}
document.querySelectorAll(".tm-win tbody tr").forEach((tr, i) => tr.style.setProperty("--r", i));

// sticky bar: solid, blurred background once the page has scrolled
const navBar = document.querySelector(".bar");
const onScroll = () => navBar?.classList.toggle("scrolled", scrollY > 8);
addEventListener("scroll", onScroll, { passive: true });
onScroll();

// reveal sections as they scroll into view
const io = "IntersectionObserver" in window && new IntersectionObserver((entries) => {
  for (const e of entries) if (e.isIntersecting) (e.target.classList.add("in"), io.unobserve(e.target));
}, { threshold: 0.12 });
document.querySelectorAll(".reveal").forEach((el) => (io ? io.observe(el) : el.classList.add("in")));
