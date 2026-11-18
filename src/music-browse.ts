// Music tab, right side: Search · Playlists · Queue from Spotify (spotify.rs), and the ♥ for the song playing.
// Not connected: the search box opens Spotify's own search (app, else web). Titles and names are Spotify data:
// textContent only; cover art is an https://*.scdn.co / *.spotifycdn.com URL (allowed by the CSP for images only).
import { invoke } from "@tauri-apps/api/core";
import { icon } from "./icons-line";

type Track = { uri: string; id: string; name: string; artists: string; album: string; art: string | null; ms: number };
type Playlist = { uri: string; id: string; name: string; owner: string; art: string | null; count: number };
type Status = { clientId: boolean; connected: boolean; user: string | null; premium: boolean | null };
type Tab = "search" | "playlists" | "queue";

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const el = (tag: string, cls: string, text?: string) => Object.assign(document.createElement(tag), { className: cls, ...(text === undefined ? {} : { textContent: text }) });
let status: Status | null = null;
let tab: Tab = "search";
let seq = 0; // drops replies that arrive after the user moved on
let timer = 0;
let current: { id: string; name: string; liked: boolean } | null = null;

function cover(url: string | null): HTMLElement {
  const box = el("span", "mb-art");
  if (url) box.append(Object.assign(document.createElement("img"), { src: url, alt: "", loading: "lazy", referrerPolicy: "no-referrer" }));
  return box;
}

function note(text: string, action?: [string, () => void]): HTMLElement {
  const n = el("div", "mb-note", text);
  if (action) {
    const b = el("button", "link-btn", action[0]) as HTMLButtonElement;
    b.type = "button";
    b.onclick = action[1];
    n.append(b);
  }
  return n;
}

const fail = (e: unknown) => show([note(String(e))]);
const show = (rows: HTMLElement[]) => $("mb-list").replaceChildren(...rows);

/** A song row: click plays it (inside `context` when given), + adds it to the queue. */
function trackRow(t: Track, context?: string): HTMLElement {
  const r = el("div", "mb-row");
  r.title = `${t.name} · ${t.artists}`;
  const text = el("span", "mb-text");
  text.append(el("span", "mb-name ell", t.name), el("span", "mb-sub ell", t.artists));
  const add = el("button", "media-btn mb-add") as HTMLButtonElement;
  add.type = "button";
  add.title = "Add to queue";
  add.innerHTML = icon("plus", 14); // static SVG
  add.onclick = (e) => {
    e.stopPropagation();
    invoke("spotify_queue_add", { uri: t.uri }).then(() => flash(add), fail);
  };
  r.append(cover(t.art), text, add);
  r.onclick = () => invoke("spotify_play", context ? { context, offset: t.uri } : { uri: t.uri }).catch(fail);
  return r;
}

function flash(b: HTMLElement): void {
  b.classList.add("done");
  window.setTimeout(() => b.classList.remove("done"), 900);
}

function playlistRow(p: Playlist): HTMLElement {
  const r = el("div", "mb-row");
  const text = el("span", "mb-text");
  text.append(el("span", "mb-name ell", p.name), el("span", "mb-sub ell", [p.owner, p.count ? `${p.count} songs` : ""].filter(Boolean).join(" · ")));
  r.append(cover(p.art), text);
  r.onclick = () => openPlaylist(p);
  return r;
}

async function openPlaylist(p: Playlist): Promise<void> {
  const mine = ++seq;
  const head = el("div", "mb-row mb-back");
  const back = el("button", "media-btn", "‹") as HTMLButtonElement;
  back.type = "button";
  back.title = "Back";
  back.onclick = () => render();
  const play = el("button", "btn primary small", "Play") as HTMLButtonElement;
  play.type = "button";
  play.onclick = () => invoke("spotify_play", { context: p.uri }).catch(fail);
  head.append(back, el("span", "mb-name ell", p.name), play);
  show([head, note("Loading…")]);
  try {
    const tracks = await invoke<Track[]>("spotify_playlist_tracks", { id: p.id });
    if (mine === seq) show([head, ...(tracks.length ? tracks.map((t) => trackRow(t, p.uri)) : [note("No songs in this playlist")])]);
  } catch (e) {
    if (mine === seq) show([head, note(String(e))]);
  }
}

async function render(): Promise<void> {
  const mine = ++seq;
  document.querySelectorAll<HTMLElement>("[data-mb]").forEach((b) => b.classList.toggle("on", b.dataset.mb === tab));
  $("mb-q").hidden = tab !== "search";
  if (!status?.connected) {
    if (tab !== "search") return show([note("Connect Spotify to see your playlists and queue here.", ["Set up", () => invoke("open_settings").catch(fail)])]);
    return show([note("Press Enter to search in Spotify. Connect Spotify in Settings → Integrations to search and play right here.", ["Set up", () => invoke("open_settings").catch(fail)])]);
  }
  try {
    if (tab === "search") {
      const q = $<HTMLInputElement>("mb-q").value.trim();
      if (!q) return show([note("Search songs and playlists")]);
      show([note("Searching…")]);
      const r = await invoke<{ tracks: Track[]; playlists: Playlist[] }>("spotify_search", { q });
      if (mine !== seq) return;
      const rows = [...r.tracks.map((t) => trackRow(t))];
      if (r.playlists.length) rows.push(el("div", "mb-label", "Playlists"), ...r.playlists.map(playlistRow));
      show(rows.length ? rows : [note(`Nothing found for “${q}”`)]);
    } else if (tab === "playlists") {
      show([note("Loading…")]);
      const list = await invoke<Playlist[]>("spotify_playlists");
      if (mine === seq) show(list.length ? list.map(playlistRow) : [note("You have no playlists yet")]);
    } else {
      show([note("Loading…")]);
      const q = await invoke<{ current: Track | null; queue: Track[] }>("spotify_queue");
      if (mine !== seq) return;
      const rows: HTMLElement[] = [];
      if (q.current) rows.push(el("div", "mb-label", "Now playing"), trackRow(q.current));
      if (q.queue.length) rows.push(el("div", "mb-label", "Next up"), ...q.queue.map((t) => trackRow(t)));
      show(rows.length ? rows : [note("Nothing queued. Open Spotify and play something.")]);
    }
  } catch (e) {
    if (mine === seq) fail(e);
  }
}

async function refreshStatus(): Promise<void> {
  status = await invoke<Status>("spotify_status").catch(() => null);
}

/** The ♥ next to the controls: shown when Spotify is connected and is what's playing. Called on every song change. */
export async function refreshLike(title: string): Promise<void> {
  const btn = $<HTMLButtonElement>("btn-like");
  if (!status) await refreshStatus();
  if (!status?.connected || !title) return void (btn.hidden = true);
  current = await invoke<{ id: string; name: string; liked: boolean } | null>("spotify_current").catch(() => null);
  const ours = !!current && current.name === title; // the playing app is Spotify, not a browser tab with the same song
  btn.hidden = !ours;
  if (ours) drawLike();
}

function drawLike(): void {
  const btn = $<HTMLButtonElement>("btn-like");
  btn.classList.toggle("on", !!current?.liked);
  btn.setAttribute("aria-pressed", String(!!current?.liked));
  btn.title = current?.liked ? "Remove from Liked Songs" : "Save to Liked Songs";
  btn.innerHTML = icon("heart", 15); // static SVG
}

export function initMusicBrowse(): void {
  const music = $("music");
  const toggle = $<HTMLButtonElement>("btn-browse");
  toggle.onclick = async () => {
    const on = !music.classList.contains("browse");
    music.classList.toggle("browse", on);
    toggle.classList.toggle("on", on);
    toggle.setAttribute("aria-pressed", String(on));
    if (on) {
      await refreshStatus();
      render();
      if (tab === "search") $("mb-q").focus();
    }
  };
  document.querySelectorAll<HTMLElement>("[data-mb]").forEach((b) => {
    b.onclick = () => {
      tab = b.dataset.mb as Tab;
      render();
    };
  });
  const q = $<HTMLInputElement>("mb-q");
  q.oninput = () => {
    if (!status?.connected) return;
    clearTimeout(timer);
    timer = window.setTimeout(render, 350);
  };
  q.onkeydown = (e) => {
    if (e.key === "Escape") (q.value = ""), render();
    if (e.key !== "Enter") return;
    e.preventDefault();
    if (status?.connected) render();
    else invoke("spotify_open_search", { q: q.value }).catch(fail);
  };
  const like = $<HTMLButtonElement>("btn-like");
  like.onclick = () => {
    if (!current) return;
    const on = !current.liked;
    invoke("spotify_like", { id: current.id, on }).then(() => {
      if (current) current.liked = on;
      drawLike();
    }, fail);
  };
}
