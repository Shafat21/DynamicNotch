import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { LauncherApp } from "../../panels";
import { store } from "../store";
import { button, h, page, report, row, section } from "../ui";

type Bridge = { online: boolean; connected?: boolean; error?: string };
type Calendar = { configured: boolean; error: string | null };

export function renderFeatures(root: HTMLElement): () => void {
  // Claude Code
  const claudeState = h("span", { className: "row-desc" });
  const claudeBtn = button("Connect", () => {});
  const onBridge = (b: Bridge) => {
    claudeState.textContent = !b.online ? `Bridge offline${b.error ? `: ${b.error}` : ""}` : b.connected ? "Connected" : "Not connected";
    claudeBtn.textContent = b.connected ? "Disconnect" : "Connect";
    claudeBtn.onclick = async () => {
      claudeBtn.disabled = true;
      try {
        await invoke(b.connected ? "disconnect_claude" : "connect_claude");
      } catch (e) {
        report(String(e));
      } finally {
        claudeBtn.disabled = false;
      }
    };
  };

  // Calendar (the stored link never comes back to the webview)
  const calState = h("span", { className: "row-desc" });
  const calInput = h("input", { type: "url", placeholder: "https://…/basic.ics", ariaLabel: "Calendar link (iCal)", spellcheck: false });
  const calRemove = button("Remove", () => saveCalendar(""));
  const onCalendar = (c: Calendar) => {
    calState.textContent = !c.configured ? "No calendar linked." : c.error ? `Linked, last update failed: ${c.error}` : "Linked.";
    calRemove.hidden = !c.configured;
  };
  const saveCalendar = async (url: string) => {
    try {
      onCalendar(await invoke<Calendar>("set_calendar_url", { url }));
      calInput.value = "";
      report(url ? "Calendar linked" : "Calendar removed");
    } catch (e) {
      report(String(e));
    }
  };
  const calForm = h("form", { className: "inline-form" }, calInput, h("button", { type: "submit", className: "btn" }, "Save"), calRemove);
  calForm.onsubmit = (e) => {
    e.preventDefault();
    if (calInput.value.trim()) saveCalendar(calInput.value);
  };

  // Weather location
  const locState = h("span", { className: "row-desc" }, `Now: ${store.settings.location}`);
  const locInput = h("input", { type: "text", placeholder: "City", ariaLabel: "Weather city", maxLength: 100 });
  const locForm = h("form", { className: "inline-form" }, locInput, h("button", { type: "submit", className: "btn" }, "Save"));
  locForm.onsubmit = async (e) => {
    e.preventDefault();
    const city = locInput.value.trim();
    if (!city) return locInput.focus();
    try {
      const name = await invoke<string>("set_location", { city });
      locState.textContent = `Now: ${name}`;
      locInput.value = "";
    } catch (err) {
      report(String(err));
    }
  };

  // Launcher apps
  const apps = h("ul", { className: "apps", ariaLabel: "Launcher apps" });
  const drawApps = (list: LauncherApp[]) =>
    apps.replaceChildren(
      ...list.map((a) => {
        const img = a.icon ? h("img", { src: a.icon, alt: "", width: 20, height: 20 }) : h("span", { className: "app-blank" });
        const remove = button("Remove", () => invoke<LauncherApp[]>("remove_app", { id: a.id }).then(drawApps, (e) => report(String(e))), "btn small");
        remove.ariaLabel = `Remove ${a.name}`;
        return h("li", {}, img, h("span", { className: "app-name" }, a.name), remove);
      }),
    );
  const addApp = button("Add app…", () => invoke<LauncherApp[]>("pick_app").then(drawApps, (e) => report(String(e))));

  const p = page("Features");
  p.append(
    section("Claude Code", row("Claude Code", "Show sessions and answer prompts from the island.", h("div", { className: "stack" }, claudeBtn, claudeState))),
    section("Calendar", row("Calendar link", "Paste a private iCal (.ics) link from Google or Outlook.", h("div", { className: "stack" }, calForm, calState))),
    section("Weather", row("Location", "City used for the weather card.", h("div", { className: "stack" }, locForm, locState))),
    section("Launcher apps", apps, h("div", { className: "section-actions" }, addApp)),
  );
  root.append(p);

  // Live status while the page is open; initial values from the sticky snapshot.
  onBridge({ online: false });
  onCalendar({ configured: false, error: null });
  invoke<Record<string, unknown>>("get_snapshot").then((snap) => {
    if (snap.bridge) onBridge(snap.bridge as Bridge);
    if (snap.calendar) onCalendar(snap.calendar as Calendar);
  });
  invoke<LauncherApp[]>("list_apps").then(drawApps, (e) => report(String(e)));
  const unlisten: Promise<UnlistenFn>[] = [
    listen<Bridge>("bridge", (e) => onBridge(e.payload)),
    listen<Calendar>("calendar", (e) => onCalendar(e.payload)),
    listen<LauncherApp[]>("apps", (e) => drawApps(e.payload)),
  ];
  return () => unlisten.forEach((u) => u.then((f) => f()));
}
