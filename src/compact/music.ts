// Compact notch: Music card, and its Lyrics side (the current line large; the backend sends no other lines).
// Song text comes from the player: textContent only.
import { invoke } from "@tauri-apps/api/core";
import { icon } from "../icons-line";

type Media = { state: string; title: string; artist: string; duration: number; seekable: boolean };
type Progress = { position: number; duration: number };
export type MusicState = { media: Media; progress: Progress; lyric: string; art: string | null; showLyrics: boolean; onFlip(): void };

const mmss = (s: number) => `${Math.floor(Math.max(0, s) / 60)}:${String(Math.floor(Math.max(0, s) % 60)).padStart(2, "0")}`;
const node = (tag: string, cls: string, text = "") => Object.assign(document.createElement(tag), { className: cls, textContent: text });

function btn(cls: string, ic: string, label: string, onClick: () => void, size = 18): HTMLButtonElement {
  const b = node("button", `cm-btn ${cls}`) as HTMLButtonElement;
  b.type = "button";
  b.title = label;
  b.innerHTML = icon(ic, size); // static SVG from our icon set
  b.onclick = (e) => (e.stopPropagation(), onClick());
  return b;
}
const control = (action: string) => void invoke("media_control", { action }).catch(console.error);

export function renderMusic(el: HTMLElement, s: MusicState): void {
  const idle = s.media.state === "Idle" || !s.media.title;
  const view = s.showLyrics && !idle ? "lyrics" : "player";
  if (el.dataset.built !== `music-${view}`) build(el, view, s);
  el.dataset.built = `music-${view}`;
  const q = <T extends Element>(sel: string) => el.querySelector<T>(sel)!;
  const art = q<HTMLImageElement>(".cm-art img");
  art.hidden = !s.art;
  if (s.art) art.src = s.art; // data URI from the backend
  q(".cm-title").textContent = idle ? "Nothing playing" : s.media.title;
  q(".cm-artist").textContent = idle ? "Play something in any app" : s.media.artist;
  q(".cm-play").innerHTML = icon(s.media.state === "Playing" ? "pause" : "play", 20); // static SVG
  const dur = s.progress.duration || s.media.duration;
  const bar = q<HTMLElement>(".cm-bar");
  // set on every render: the card is built once, the track (duration, seekable) changes under it
  bar.onclick = (e) => {
    e.stopPropagation();
    if (!s.media.seekable || !dur) return;
    const r = bar.getBoundingClientRect();
    void invoke("media_seek", { seconds: ((e.clientX - r.left) / r.width) * dur }).catch(console.error);
  };
  q<HTMLElement>(".cm-fill").style.width = `${dur ? Math.min(100, (s.progress.position / dur) * 100) : 0}%`;
  if (view === "player") {
    q(".cm-elapsed").textContent = mmss(s.progress.position);
    q(".cm-left").textContent = `-${mmss(dur - s.progress.position)}`;
  } else {
    const line = q<HTMLElement>(".cm-line");
    if (line.textContent !== (s.lyric || "♪")) {
      line.textContent = s.lyric || "♪";
      line.classList.remove("in");
      void line.offsetWidth;
      line.classList.add("in");
    }
  }
}

function build(el: HTMLElement, view: "player" | "lyrics", s: MusicState): void {
  const art = node("div", "cm-art");
  art.append(Object.assign(document.createElement("img"), { alt: "" }));
  const meta = node("div", "cm-meta");
  meta.append(node("div", "cm-title ell"), node("div", "cm-artist ell"));
  const bar = node("div", "cm-bar");
  bar.append(node("i", "cm-fill"));
  const ctrl = node("div", "cm-ctrl");
  const lyr = btn("cm-lyr", "lyrics", view === "lyrics" ? "Back to the player" : "Lyrics", s.onFlip, 16);
  lyr.classList.toggle("on", view === "lyrics");
  ctrl.append(lyr, btn("", "prev", "Previous", () => control("prev")), btn("cm-play", "play", "Play / pause", () => control("toggle")), btn("", "next", "Next", () => control("next")));
  const wrap = node("div", `cm-music ${view}`);
  if (view === "player") {
    // cover | title, artist · elapsed ━━━ -left · controls (fits the ~95 px card)
    const times = node("div", "cm-times");
    times.append(node("span", "cm-elapsed"), bar, node("span", "cm-left"));
    const col = node("div", "cm-col");
    col.append(meta, times, ctrl);
    wrap.append(art, col);
  } else {
    const left = node("div", "cm-side");
    const head = node("div", "cm-head");
    head.append(art, meta);
    left.append(head, bar, ctrl);
    const lyrics = node("div", "cm-lyrics");
    lyrics.append(node("div", "cm-line"));
    wrap.append(left, lyrics);
  }
  el.replaceChildren(wrap);
}
