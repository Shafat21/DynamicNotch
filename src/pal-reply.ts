// Pal tab: message a coding agent from the island. Text (and an image pasted into the box) goes to the picked
// session's agent: Rust brings its window (Zed, VS Code, a terminal) forward and pastes it there with Enter.
import { invoke } from "@tauri-apps/api/core";
import type { Session } from "./assistant";

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const AGENT: Record<string, string> = { claude: "Claude Code", codex: "Codex", gemini: "Gemini CLI", cursor: "Cursor", copilot: "Copilot CLI", antigravity: "Antigravity", commandcode: "Command Code", opencode: "opencode" };
let target: Session | null = null;
let image: string | null = null; // base64 PNG, no data: prefix
let sending = false;

function setImage(dataUrl: string | null): void {
  image = dataUrl ? dataUrl.slice(dataUrl.indexOf(",") + 1) : null;
  const img = $<HTMLImageElement>("pr-img");
  img.hidden = !dataUrl;
  if (dataUrl) img.src = dataUrl; // data: URL of the user's own pasted image
  $("pr-clear-img").hidden = !dataUrl;
}

/** Called by the Pal tab whenever the sessions or the pick change. */
export function renderReply(s: Session | null): void {
  target = s;
  $("pal-reply").hidden = !s;
  if (s) $("pr-hint").textContent = `To ${s.project || "this session"} · ${AGENT[s.agent] ?? s.agent}. Pasted into its window: keep the agent's message box focused there.`;
}

export function initPalReply(onError: (e: unknown) => void): void {
  const input = $<HTMLInputElement>("pr-text");
  input.onpaste = (e) => {
    const file = [...(e.clipboardData?.items ?? [])].find((i) => i.type.startsWith("image/"))?.getAsFile();
    if (!file) return; // plain text: the input handles it
    e.preventDefault();
    // re-encode as PNG whatever the source format, so Rust gets one known type (data: URLs: the CSP has no blob:)
    const reader = new FileReader();
    reader.onload = () => {
      const img = new Image();
      img.onload = () => {
        const c = Object.assign(document.createElement("canvas"), { width: img.naturalWidth, height: img.naturalHeight });
        c.getContext("2d")!.drawImage(img, 0, 0);
        setImage(c.toDataURL("image/png"));
      };
      img.src = String(reader.result);
    };
    reader.readAsDataURL(file);
  };
  $("pr-clear-img").onclick = () => setImage(null);
  $<HTMLFormElement>("pal-reply").onsubmit = async (e) => {
    e.preventDefault();
    if (!target || sending || (!input.value.trim() && !image)) return;
    sending = true;
    $<HTMLButtonElement>("pr-send").disabled = true;
    try {
      await invoke("reply_to_agent", { id: target.id, text: input.value, image });
      input.value = "";
      setImage(null);
    } catch (err) {
      onError(err);
    } finally {
      sending = false;
      $<HTMLButtonElement>("pr-send").disabled = false;
    }
  };
}
