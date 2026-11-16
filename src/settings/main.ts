// Settings window: sidebar routing, theme from the `appearance` event, initial state.
import "./settings.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { icon } from "./icons";
import { acceptLook, store, type Look, type Settings } from "./store";
import { h } from "./ui";
import { renderGeneral } from "./pages/general";
import { renderAppearance } from "./pages/appearance";
import { renderTabs } from "./pages/tabs";
import { renderHome } from "./pages/home";
import { renderAgents } from "./pages/agents";
import { renderFeatures } from "./pages/features";
import { renderDictation } from "./pages/dictation";
import { renderAbout } from "./pages/about";

/** A page draws into `root`; it may return a cleanup (e.g. event listeners) run when leaving it. */
type Render = (root: HTMLElement) => (() => void) | void;

const PAGES: [id: string, label: string, render: Render][] = [
  ["general", "General", renderGeneral],
  ["appearance", "Appearance", renderAppearance],
  ["tabs", "Tabs", renderTabs],
  ["home", "Home", renderHome],
  ["agents", "Agents", renderAgents],
  ["features", "Integrations", renderFeatures],
  ["dictation", "Dictation", renderDictation],
  ["about", "About", renderAbout],
];

const nav = document.getElementById("nav")!;
const content = document.getElementById("content")!;
let cleanup: (() => void) | void;

function show(id: string): void {
  const [, , render] = PAGES.find((p) => p[0] === id) ?? PAGES[0];
  cleanup?.();
  content.replaceChildren();
  content.scrollTop = 0;
  cleanup = render(content);
  nav.querySelectorAll("button").forEach((b) => (b.ariaCurrent = b.dataset.page === id ? "page" : null));
}

async function boot(): Promise<void> {
  const [look, settings] = await Promise.all([invoke<Look>("get_appearance"), invoke<Settings>("get_settings")]);
  acceptLook(look);
  store.settings = settings;
  await Promise.all([
    listen<Look>("appearance", (e) => acceptLook(e.payload)),
    listen<Settings>("settings", (e) => (store.settings = e.payload)),
  ]);

  nav.append(
    h("div", { className: "brand" }, "Settings"),
    ...PAGES.map(([id, label]) => {
      const b = h("button", { type: "button", onclick: () => show(id) }, icon(id), h("span", {}, label));
      b.dataset.page = id;
      return b;
    }),
  );
  show("general");
}

boot().catch((e) => {
  content.textContent = `Settings failed to load: ${e}`;
});
