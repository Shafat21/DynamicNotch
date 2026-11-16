import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { LauncherApp } from "../../panels";
import { setGeneral, store } from "../store";
import { button, h, page, report, row, section, segmented, select, toggle } from "../ui";

type Bridge = { online: boolean; connected?: boolean; error?: string };
type Calendar = { configured: boolean; error: string | null };
type Dictation = { engine: string; model: string; language: string; hotkey: string; mic: string; cloudProvider: string; cloudModel: string; output: string };
type Models = Record<string, { installed: boolean; size: number }> & { localAvailable?: boolean };

const LANGS: [string, string][] = [
  ["auto", "Auto-detect"], ["en", "English"], ["de", "German"], ["fr", "French"], ["es", "Spanish"], ["it", "Italian"], ["pt", "Portuguese"], ["nl", "Dutch"],
  ["pl", "Polish"], ["ru", "Russian"], ["uk", "Ukrainian"], ["tr", "Turkish"], ["ar", "Arabic"], ["hi", "Hindi"], ["bn", "Bengali"], ["ja", "Japanese"], ["ko", "Korean"], ["zh", "Chinese"],
];

/** Settings → Features → Dictation (spec §23.7). Every change saves at once; the API key is write-only. */
function dictationSection(): { el: HTMLElement; dispose: () => void } {
  const loading = h("p", { className: "row-desc" }, "Loading…");
  const el = section("Dictation", loading);
  let cur: Dictation;
  const save = async (patch: Partial<Dictation>): Promise<string | null> => {
    try {
      cur = await invoke<Dictation>("set_dictation_settings", { patch });
      return null;
    } catch (e) {
      return String(e);
    }
  };
  const saveOrReport = (patch: Partial<Dictation>) => save(patch).then((err) => err && report(err));
  const dlState = h("span", { className: "row-desc" });

  const build = (s: Dictation, models: Models, mics: string[]) => {
    cur = s;
    const localOk = models.localAvailable === true;
    const engine = segmented("Engine", [["local", "Local"], ["cloud", "Cloud"]], s.engine, (v) => saveOrReport({ engine: v }));
    engine.querySelector("button")!.disabled = !localOk;

    // cloud: provider, model, write-only key
    const keyState = h("span", { className: "row-desc" });
    const refreshKey = () =>
      invoke<boolean>("has_cloud_key", { provider: cur.cloudProvider }).then(
        (on) => (keyState.textContent = on ? "Saved" : "Not set"),
        () => (keyState.textContent = "Not set"),
      );
    const model = h("input", { type: "text", value: s.cloudModel, ariaLabel: "Cloud model", maxLength: 64, spellcheck: false });
    model.onchange = () => saveOrReport({ cloudModel: model.value }).then(() => (model.value = cur.cloudModel));
    const provider = select("Cloud provider", [["openai", "OpenAI"], ["groq", "Groq"]], s.cloudProvider, async (v) => {
      await saveOrReport({ cloudProvider: v });
      model.value = cur.cloudModel; // the backend switches to the provider's default model
      refreshKey();
    });
    const key = h("input", { type: "password", placeholder: "Paste API key", ariaLabel: "API key", autocomplete: "off", spellcheck: false });
    const clear = button("Clear", () => invoke("clear_cloud_key", { provider: cur.cloudProvider }).then(refreshKey, (e) => report(String(e))));
    const keyForm = h("form", { className: "inline-form" }, key, h("button", { type: "submit", className: "btn" }, "Save"), clear);
    keyForm.onsubmit = (e) => {
      e.preventDefault();
      if (!key.value.trim()) return key.focus();
      invoke("set_cloud_key", { provider: cur.cloudProvider, key: key.value.trim() }).then(refreshKey, (err) => report(String(err)));
      key.value = ""; // never kept or shown again
    };
    refreshKey();

    // hotkey: the backend's validation error shows inline
    const hotErr = h("span", { className: "field-err", hidden: true });
    const hotkey = h("input", { type: "text", value: s.hotkey, ariaLabel: "Dictation hotkey", maxLength: 40, spellcheck: false });
    hotkey.onchange = async () => {
      const err = await save({ hotkey: hotkey.value });
      hotErr.hidden = !err;
      hotErr.textContent = err ?? "";
      hotkey.ariaInvalid = String(!!err);
      if (!err) hotkey.value = cur.hotkey;
    };

    // local models: listed; downloadable only when this build has local Whisper
    const mb = (n: number) => `${Math.round(n / 1e6)} MB`;
    const names = Object.keys(models).filter((k) => k !== "localAvailable");
    const modelSel = select("Local model", names.map((n): [string, string] => [n, `${n} (${mb(models[n].size)})${models[n].installed ? " · downloaded" : ""}`]), s.model, (v) => saveOrReport({ model: v }));
    const dl = button("Download", () => {
      dl.disabled = true;
      invoke("download_model", { name: modelSel.value })
        .then(() => (dlState.textContent = "Downloaded"), (e) => (dlState.textContent = String(e)))
        .finally(() => (dl.disabled = false));
    });
    dlState.textContent = localOk ? "" : "Not available in this build";
    modelSel.disabled = dl.disabled = !localOk;

    const langs = LANGS.some(([c]) => c === s.language) ? LANGS : [...LANGS, [s.language, s.language] as [string, string]];
    el.replaceChildren(
      el.firstElementChild!,
      row("Engine", localOk ? "Local runs Whisper on this PC. Cloud sends audio to the provider below." : "Local: Not available in this build. Cloud sends audio to the provider below.", engine),
      row("Local model", "Whisper model from Hugging Face, checked with SHA-256 after download.", h("div", { className: "stack" }, h("div", { className: "inline-form" }, modelSel, dl), dlState)),
      row("Cloud provider", "OpenAI-compatible transcription endpoint.", provider),
      row("Cloud model", "Transcription model name at the provider.", model),
      row("API key", "Kept in the system credential store and never shown again.", h("div", { className: "stack" }, keyForm, keyState)),
      row("Hotkey", "Hold to record, release to transcribe. Modifier-only combos need two modifiers, like Ctrl+Win.", h("div", { className: "stack" }, hotkey, hotErr)),
      row("Microphone", "Input device used while recording.", select("Microphone", [["", "System default"], ...mics.map((m): [string, string] => [m, m])], s.mic, (v) => saveOrReport({ mic: v }))),
      row("Language", "Auto-detect, or a fixed spoken language.", select("Language", langs, s.language, (v) => saveOrReport({ language: v }))),
      row("Output", "Paste the text at the cursor, or only copy it to the clipboard.", segmented("Output", [["paste", "Paste at cursor"], ["copy", "Copy only"]], s.output, (v) => saveOrReport({ output: v }))),
      h("p", { className: "row-desc" }, "Audio stays in memory and is never saved. With Cloud, each recording is sent to the chosen provider."),
    );
  };

  Promise.all([invoke<Dictation>("dictation_settings"), invoke<Models>("model_status"), invoke<string[]>("list_mics").catch(() => [] as string[])]).then(
    ([s, m, mics]) => build(s, m, mics),
    (e) => (loading.textContent = `Couldn't load dictation settings: ${e}`),
  );
  const unlisten = listen<{ name: string; received: number; total: number }>("model-progress", (e) => {
    dlState.textContent = `Downloading ${e.payload.name}… ${Math.floor((e.payload.received * 100) / Math.max(1, e.payload.total))}%`;
  });
  return { el, dispose: () => void unlisten.then((f) => f()) };
}

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

  const dictation = dictationSection();
  const p = page("Features");
  p.append(
    section("Claude Code", row("Claude Code", "Show sessions and answer prompts from the island.", h("div", { className: "stack" }, claudeBtn, claudeState))),
    section("Calendar", row("Calendar link", "Paste a private iCal (.ics) link from Google or Outlook.", h("div", { className: "stack" }, calForm, calState))),
    section(
      "Reminders",
      row(
        "Reminder sound",
        "Play a soft chime when a reminder is due (up to 3 times, 30 s apart).",
        toggle("Reminder sound", store.settings.reminderSound, (on) => setGeneral("reminder_sound", on).catch(() => {})),
      ),
    ),
    section(
      "Clipboard history",
      row(
        "Remember across restarts",
        "Off by default: history stays in memory. When on, copied text is stored on disk, unencrypted, in this app's database.",
        toggle("Remember across restarts", store.settings.clipRemember, (on) => setGeneral("clip_remember", on).catch(() => {})),
      ),
      row(
        "Pause",
        "Stop recording new copies.",
        toggle("Pause clipboard history", store.settings.clipPaused, (on) => invoke("set_clip_paused", { paused: on }).catch((e) => report(String(e)))),
      ),
      row("Clear history", "Removes every entry except pinned ones.", button("Clear history", () => invoke("clear_clips").then(() => report("Clipboard history cleared"), (e) => report(String(e))))),
    ),
    section(
      "Shelf",
      row("Clear shelf", "Removes every item from the shelf. The files themselves are not touched.", button("Clear shelf", () => invoke("clear_shelf").then(() => report("Shelf cleared"), (e) => report(String(e))))),
    ),
    section(
      "Sound & brightness",
      row(
        "Sound settings",
        "Output devices that can't be switched from the island can be changed in the system's sound settings.",
        button("Open sound settings", () => invoke("run_action", { id: "sound-settings" }).catch((e) => report(String(e)))),
      ),
    ),
    section(
      "Bluetooth",
      row(
        "Bluetooth settings",
        "Windows can't connect or disconnect paired devices from the island; pair and connect them in the system's Bluetooth settings.",
        button("Open Bluetooth settings", () => invoke("run_action", { id: "bluetooth" }).catch((e) => report(String(e)))),
      ),
    ),
    dictation.el,
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
  return () => {
    unlisten.forEach((u) => u.then((f) => f()));
    dictation.dispose();
  };
}
