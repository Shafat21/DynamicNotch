// Settings → Agents: one row per coding agent the bridge knows (Claude Code, Codex, …), from the `bridge` event.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { button, h, page, report, row, section } from "../ui";

type Bridge = { online: boolean; agents?: { id: string; label: string; connected: boolean }[]; error?: string };

export function renderAgents(root: HTMLElement): () => void {
  const list = section("Coding agents", h("p", { className: "row-desc" }, "Loading…"));
  const onBridge = (b: Bridge) => {
    const offline = b.online ? null : h("p", { className: "row-desc" }, `Bridge offline${b.error ? `: ${b.error}` : ""}`);
    const rows = (b.agents ?? []).map((a) => {
      const btn = button(a.connected ? "Disconnect" : "Connect", async () => {
        btn.disabled = true;
        try {
          await invoke("set_agent", { id: a.id, on: !a.connected });
        } catch (e) {
          report(String(e));
        } finally {
          btn.disabled = false;
        }
      });
      btn.ariaLabel = `${a.connected ? "Disconnect" : "Connect"} ${a.label}`;
      const state = h("span", { className: "row-desc" }, a.connected ? "Connected" : "Not connected");
      return row(a.label, "Show sessions and answer prompts from the island.", h("div", { className: "stack" }, btn, state));
    });
    list.replaceChildren(list.firstElementChild!, ...(offline ? [offline] : []), ...rows);
  };

  const p = page("Agents");
  p.append(
    h("p", { className: "page-hint" }, "Connecting adds the island's hooks to the agent's config file (backed up first). Sessions then show on the Pal card, and permission prompts can be answered from the island."),
    list,
  );
  root.append(p);

  // "Loading…" until the sticky snapshot arrives, then live while the page is open.
  invoke<Record<string, unknown>>("get_snapshot").then((snap) => snap.bridge && onBridge(snap.bridge as Bridge));
  const unlisten = listen<Bridge>("bridge", (e) => onBridge(e.payload));
  return () => void unlisten.then((f) => f());
}
