import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { LauncherApp } from "../../panels";
import { setGeneral, store } from "../store";
import { button, h, page, report, row, section, segmented, select, toggle } from "../ui";

type Calendar = { configured: boolean; desktop?: boolean; error: string | null };
type Dictation = { engine: string; language: string; hotkey: string; mic: string; cloudProvider: string; cloudModel: string; output: string; cleanup: string; speech: string };
type Model = { installed: boolean; size: number; id: string };
type Models = Model & { multilingual: Model; claude: boolean };
type Words = { words: string[]; replacements: { from: string; to: string }[]; snippets: { trigger: string; text: string }[] };

const LANGS: [string, string][] = [
  ["auto", "Auto-detect"], ["en", "English"], ["de", "German"], ["fr", "French"], ["es", "Spanish"], ["it", "Italian"], ["pt", "Portuguese"], ["nl", "Dutch"],
  ["pl", "Polish"], ["ru", "Russian"], ["uk", "Ukrainian"], ["tr", "Turkish"], ["ar", "Arabic"], ["hi", "Hindi"], ["bn", "Bengali"], ["ja", "Japanese"], ["ko", "Korean"], ["zh", "Chinese"],
];

/** Pairs editor (replacements, snippets): a row per pair, saved whenever a complete pair changes. */
function pairs<K extends string>(keys: [K, K], labels: [string, string], items: Record<K, string>[], multiline: boolean, onSave: (v: Record<K, string>[]) => void): HTMLElement {
  const list = h("ul", { className: "pairs" });
  const collect = () =>
    onSave(
      [...list.children]
        .map((li) => {
          const [a, b] = li.querySelectorAll<HTMLInputElement | HTMLTextAreaElement>("input, textarea");
          return { [keys[0]]: a.value.trim(), [keys[1]]: b.value.trim() } as Record<K, string>;
        })
        .filter((p) => p[keys[0]] && p[keys[1]]),
    );
  const add = (item?: Record<K, string>) => {
    const a = h("input", { type: "text", value: item?.[keys[0]] ?? "", placeholder: labels[0], ariaLabel: labels[0], maxLength: 100, spellcheck: false });
    const b = multiline
      ? h("textarea", { value: item?.[keys[1]] ?? "", placeholder: labels[1], ariaLabel: labels[1], maxLength: 5000, rows: 2 })
      : h("input", { type: "text", value: item?.[keys[1]] ?? "", placeholder: labels[1], ariaLabel: labels[1], maxLength: 100, spellcheck: false });
    a.onchange = b.onchange = collect;
    const li = h("li", {}, a, b);
    const rm = button("Remove", () => (li.remove(), collect()), "btn small");
    rm.ariaLabel = `Remove ${labels[0].toLowerCase()}`;
    li.append(rm);
    list.append(li);
    return a;
  };
  items.forEach((i) => add(i));
  return h("div", { className: "pairs-box" }, list, button("Add", () => add().focus(), "btn small"));
}

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
  const mb = (n: number) => `${Math.round(n / 1e6)} MB`;
  const progress = new Map<string, HTMLElement>(); // model id → its state line, for `model-progress`

  const build = (s: Dictation, models: Models, mics: string[], words: Words) => {
    cur = s;
    const engine = segmented("Engine", [["auto", "Auto"], ["local", "Offline"], ["cloud", "Cloud"]], s.engine, (v) => saveOrReport({ engine: v }));

    // offline models: each downloaded on click, progress from `model-progress`
    const hint = h("span", { className: "row-desc" });
    const showHint = () =>
      (hint.textContent = cur.speech === "multilingual" && !models.multilingual.installed ? (models.installed ? "Download the multilingual model for Bangla and other languages. Until then English/European is used." : "Download the multilingual model for Bangla and other languages.") : "");
    const download = (m: Model, kind: string) => {
      const state = h("span", { className: "row-desc" }, m.installed ? `Installed · ${mb(m.size)}` : `Not downloaded · ${mb(m.size)}`);
      progress.set(m.id, state);
      const dl = button(m.installed ? "Installed" : "Download", () => {
        dl.disabled = true;
        state.textContent = "Starting download…";
        invoke("download_model", { model: kind }).then(
          () => {
            m.installed = true;
            state.textContent = `Installed · ${mb(m.size)}`;
            dl.textContent = "Installed";
            showHint();
            report("Offline model installed");
          },
          (e) => {
            state.textContent = String(e);
            dl.disabled = false;
          },
        );
      });
      dl.disabled = m.installed;
      return h("div", { className: "stack" }, dl, state);
    };
    const speech = segmented("Offline model", [["multilingual", "Auto (multilingual)"], ["european", "English/European (faster)"]], s.speech, async (v) => {
      await saveOrReport({ speech: v });
      showHint();
    });
    showHint();

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

    // dictionary, replacements, snippets: saved together
    let w = words;
    const saveWords = (patch: Partial<Words>) =>
      invoke<Words>("set_dictation_words", { words: { ...w, ...patch } }).then(
        (n) => (w = n),
        (e) => report(String(e)),
      );
    const dict = h("textarea", { value: words.words.join("\n"), ariaLabel: "Dictionary words", placeholder: "One per line, e.g. Supabase", rows: 4, spellcheck: false });
    dict.onchange = () => saveWords({ words: dict.value.split("\n").map((x) => x.trim()).filter(Boolean) });
    const block = (label: string, desc: string, control: Node) =>
      h("div", { className: "row col" }, h("div", { className: "row-text" }, h("div", { className: "row-label" }, label), h("div", { className: "row-desc" }, desc)), control);

    const langs = LANGS.some(([c]) => c === s.language) ? LANGS : [...LANGS, [s.language, s.language] as [string, string]];
    el.replaceChildren(
      el.firstElementChild!,
      row("Engine", "Auto uses the offline model once it's downloaded, else Cloud when an API key is saved.", engine),
      row("Offline model", "Auto understands Bangla, Hindi, English and 1600+ other languages (no punctuation). English/European is faster, with punctuation. Falls back to whichever is downloaded.", h("div", { className: "stack" }, speech, hint)),
      row("Multilingual model", "Meta Omnilingual ASR runs on this PC. Downloaded from Hugging Face and checked with SHA-256.", download(models.multilingual, "multilingual")),
      row("English/European model", "NVIDIA Parakeet (25 European languages) runs on this PC. Downloaded from Hugging Face and checked with SHA-256.", download(models, "european")),
      row("Cloud provider", "OpenAI-compatible transcription endpoint. Audio is sent to the provider.", provider),
      row("Cloud model", "Transcription model name at the provider.", model),
      row("API key", "Kept in the system credential store and never shown again.", h("div", { className: "stack" }, keyForm, keyState)),
      row("Hotkey", "Hold to record, release to paste. Double-tap for hands-free, press again to finish. Esc cancels.", h("div", { className: "stack" }, hotkey, hotErr)),
      row("Microphone", "Input device used while recording.", select("Microphone", [["", "System default"], ...mics.map((m): [string, string] => [m, m])], s.mic, (v) => saveOrReport({ mic: v }))),
      row("Language", "For Cloud: Auto-detect, or always this language (e.g. Bengali). The offline models find the language themselves.", select("Language", langs, s.language, (v) => saveOrReport({ language: v }))),
      row("Output", "Paste the text at the cursor, or only copy it to the clipboard.", segmented("Output", [["paste", "Paste at cursor"], ["copy", "Copy only"]], s.output, (v) => saveOrReport({ output: v }))),
      row(
        "Clean-up",
        models.claude
          ? "Natural removes filler words and fixes punctuation; Polished writes email-ready sentences. Uses your Claude Code login; only the text is sent."
          : "Needs Claude Code (claude) installed and logged in. Until then the text is pasted as said.",
        segmented("Clean-up style", [["verbatim", "As said"], ["natural", "Natural"], ["polished", "Polished"]], s.cleanup, (v) => saveOrReport({ cleanup: v })),
      ),
      block("Dictionary", "Words always spelled exactly like this.", dict),
      block("Replacements", "What was heard → what gets written.", pairs(["from", "to"], ["Heard", "Written"], words.replacements, false, (v) => saveWords({ replacements: v }))),
      block("Snippets", "Say the trigger phrase to paste the whole text.", pairs(["trigger", "text"], ["Trigger phrase", "Text to paste"], words.snippets, true, (v) => saveWords({ snippets: v }))),
      h("p", { className: "row-desc" }, "Dictation audio stays in memory and is never saved (call recordings from the Notetaker are temporary files, deleted after processing). History and stats stay on this PC (Settings → Dictation)."),
    );
  };

  Promise.all([
    invoke<Dictation>("dictation_settings"),
    invoke<Models>("model_status"),
    invoke<string[]>("list_mics").catch(() => [] as string[]),
    invoke<Words>("dictation_words"),
  ]).then(
    ([s, m, mics, w]) => build(s, m, mics, w),
    (e) => (loading.textContent = `Couldn't load dictation settings: ${e}`),
  );
  const unlisten = listen<{ name: string; received: number; total: number }>("model-progress", (e) => {
    const state = progress.get(e.payload.name);
    if (state) state.textContent = `Downloading… ${Math.floor((e.payload.received * 100) / Math.max(1, e.payload.total))}% of ${mb(e.payload.total)}`;
  });
  return { el, dispose: () => void unlisten.then((f) => f()) };
}

export function renderFeatures(root: HTMLElement): () => void {
  // Calendar (the stored link never comes back to the webview)
  const calState = h("span", { className: "row-desc" });
  const calInput = h("input", { type: "url", placeholder: "https://…/basic.ics", ariaLabel: "Calendar link (iCal)", spellcheck: false });
  const calRemove = button("Remove", () => saveCalendar(""));
  const desktopSwitch = toggle("Desktop calendar", false, async (on) => {
    try {
      onCalendar(await invoke<Calendar>("set_calendar_desktop", { on }));
    } catch (e) {
      desktopSwitch.checked = !on;
      report(String(e));
    }
  });
  const onCalendar = (c: Calendar) => {
    desktopSwitch.checked = !!c.desktop;
    calState.textContent = !c.configured ? "No calendar connected." : c.error ? `Last update: ${c.error}` : "Connected.";
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
  const p = page("Integrations");
  p.append(
    section(
      "Calendar",
      row("Desktop calendar", "Show events from the Windows Calendar app (Outlook, Google, iCloud and Exchange accounts added there). Read-only.", desktopSwitch),
      row("Calendar link", "Or paste a private iCal (.ics) link from Google or Outlook. Both can be on; events are merged.", h("div", { className: "stack" }, calForm, calState)),
    ),
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
  onCalendar({ configured: false, error: null });
  invoke<Record<string, unknown>>("get_snapshot").then((snap) => {
    if (snap.calendar) onCalendar(snap.calendar as Calendar);
  });
  invoke<LauncherApp[]>("list_apps").then(drawApps, (e) => report(String(e)));
  const unlisten: Promise<UnlistenFn>[] = [
    listen<Calendar>("calendar", (e) => onCalendar(e.payload)),
    listen<LauncherApp[]>("apps", (e) => drawApps(e.payload)),
  ];
  return () => {
    unlisten.forEach((u) => u.then((f) => f()));
    dictation.dispose();
  };
}
