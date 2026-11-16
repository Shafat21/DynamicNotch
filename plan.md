# DynamicNotch — Plan & Project Guide

A **Dynamic Island for Windows**, written in **Rust + Tauri 2**, with **SQLite** for local storage.

A small black pill sits at the top-centre of the screen. It shows the clock, and morphs into
rich panels when you hover it, scroll on it, play music, get a notification, toggle Caps Lock,
or plug in the charger.

---

## 1. Why this stack

| Piece | Choice | Why |
|---|---|---|
| App shell | **Tauri 2** | Rust backend + WebView2 frontend. Small exe (~5–10 MB), native window control. |
| Backend | **Rust** | System access (WinRT media/notifications, Win32 keys/power), threads, SQLite. |
| Frontend | **Vanilla TypeScript + CSS** (Vite) | The island is mostly animation: gradients, glows, morphing. CSS does it in a few lines; no framework needed. |
| Storage | **SQLite** via `rusqlite` (`bundled`) | No external DB install; the SQLite library is compiled in. |
| Icons | `@mdi/js` | Material Design Icons. Tree-shaken SVG paths. |

---

## 2. How it works (big picture)

```
 ┌──────────────────────── Rust (src-tauri) ────────────────────────┐
 │  monitors/  one OS thread each: system, perf, weather, calendar, │
 │             media, notify                                        │
 │  window*    placement + click-through/hover (cursor poll)        │
 │  watchers   display, fullscreen, reminders scheduler, clipboard, │
 │             sound                                                │
 │  bridge/    Claude Code hooks server (127.0.0.1), sessions,      │
 │             prompts, usage;  statusline.rs = `--statusline` CLI  │
 │  dictation/ hotkey -> mic -> cloud|local Whisper -> paste        │
 │  db.rs (SQLite)   commands.rs (invoke API)   menu.rs             │
 └───────────────▲──────────────────────────────┬───────────────────┘
          invoke │                        emit  │  events
 ┌───────────────┴──────── WebView (src) ───────▼───────────────────┐
 │  main.ts   state machine, sizing, tabs, panels, event wiring     │
 │  one module per page (see file map), styles.css                  │
 └──────────────────────────────────────────────────────────────────┘
```

**The window trick.** The real window is a transparent, frameless, always-on-top, no-taskbar window
(1200x700 nominal; `display::fit_window` scales it for the chosen monitor's work area). Only the pill is
visible. On Windows the cursor is polled and outside the pill the window ignores the mouse so clicks reach the
desktop; the frontend reports the pill with `set_hit_rect`. On Linux this is **not implemented** (`window_linux.rs`).

**State machine** (`src/main.ts`): `idle` (small pill: clock/Pal, song title or lyric), `hover` (dashboard with tab bar),
`notify` (notification or Caps/Num Lock change, then back), `charging` (short beam animation). `transition()` sets the
target size, runs the CSS morph and swaps the visible page while the content is faded out.

---

## 3. File map

```
DynamicNotch/
├─ plan.md, README.md
├─ index.html              <- static markup for the island and pages
├─ src/                    <- vanilla TS + CSS (Vite)
│  ├─ main.ts              <- state machine, event listeners, input wiring, boot
│  ├─ panels.ts            <- tasks, month progress
│  ├─ assistant.ts         <- Claude Code card: sessions, prompts
│  ├─ usage.ts             <- usage-limit ring + reset text (5 h / weekly)
│  ├─ dictation.ts         <- dictation HUD (recording / transcribing / pasted / error)
│  ├─ pal.ts, pal-shapes.ts, mood.ts   <- Pal face and its 8 moods
│  ├─ calendar.ts, focus.ts, notes.ts, reminders.ts, clipboard.ts, sound.ts, bluetooth.ts, shelf.ts
│  ├─ idle-width.ts        <- idle pill width
│  ├─ icons-line.ts        <- line icon set
│  ├─ ui/                  <- home.ts (Home cards), tabs.ts (tab bar + More overflow), theme.ts (apply theme)
│  ├─ settings/            <- separate Settings window: main.ts, store.ts, ui.ts, icons.ts, settings.css,
│  │                          pages/ general, appearance, features, home, tabs, layout-list, about
│  └─ styles.css
├─ src-tauri/
│  ├─ Cargo.toml           <- sherpa-onnx (static, prebuilt libs fetched at build time)
│  ├─ tauri.conf.json, capabilities/, icons/
│  └─ src/
│     ├─ main.rs           <- entry; `--statusline` runs statusline.rs
│     ├─ lib.rs            <- setup, AppState, publish(), generate_handler!
│     ├─ db.rs             <- SQLite schema, migrations v1..v6, settings, tasks, apps, notes, reminders, clips, shelf
│     ├─ commands.rs       <- #[tauri::command] API (validates webview input)
│     ├─ menu.rs           <- right-click menu (Settings, Animation Style, Connect Claude, Quit)
│     ├─ actions.rs        <- allow-list of shell actions
│     ├─ appearance.rs     <- 7 themes, icon style, density, sizes, tab/home layout
│     ├─ window.rs         <- placement, click-through, hit rect (+ window_windows / window_linux)
│     ├─ display.rs        <- display choice, responsive scaling, display watcher
│     ├─ fullscreen.rs     <- hide-in-fullscreen watcher (Linux impl always returns false)
│     ├─ launcher.rs       <- app list, pick/launch, icons
│     ├─ notes.rs, reminders.rs (scheduler thread, snooze/done, repeat)
│     ├─ clipboard.rs      <- text history, memory-only by default, skips private copies (Windows)
│     ├─ sound.rs, brightness.rs, bluetooth.rs, shelf.rs   <- each with _windows / _linux backends
│     ├─ worker.rs         <- background job helper
│     ├─ statusline.rs     <- `dynamic-notch.exe --statusline`: forwards Claude Code statusLine JSON to the bridge,
│     │                       then runs the chained original statusLine
│     ├─ bridge/           <- mod.rs (status, 15 s prompt wait), server.rs (loopback HTTP + token),
│     │                       install.rs (settings.json hooks + statusLine, .dynamicnotch.bak),
│     │                       sessions.rs, prompt.rs, usage.rs
│     ├─ dictation/        <- mod.rs, settings.rs (hotkey parse), audio.rs (cpal), hotkey_*/paste_* (windows, linux),
│     │                       cloud.rs (OpenAI/Groq + keyring), local.rs (sherpa-onnx Parakeet), text.rs, cleanup.rs,
│     │                       models.rs (Hugging Face download + SHA-256), commands.rs
│     └─ monitors/         <- mod.rs (spawn_all, HTTP client), perf.rs, weather.rs, calendar.rs (iCal),
│                             media.rs, notify.rs, system.rs (media/notify/system have _windows/_linux)
└─ scripts/usage.check.ts  <- usage helper check
```

---

## 4. Backend <-> frontend contract

### Events (Rust -> TS)
Sticky events (via `publish()`) are cached in `AppState.last`; the page calls `get_snapshot` on load to catch up.

| Event | Notes | Sticky |
|---|---|---|
| `perf` | cpu, ram, disk, network speed | yes |
| `weather`, `calendar` | forecast; agenda from the iCal link | yes |
| `media`, `media-art`, `media-progress`, `lyric` | playback state, art, position, current lyric line | yes |
| `accent` | OS accent colour | yes |
| `sound` | volume / mute / device | yes |
| `appearance` | theme, icons, density, layout | yes |
| `fullscreen` | a fullscreen app is active | yes |
| `bridge` | `{online, connected, error?}` | yes |
| `sessions`, `usage` | Claude Code sessions; plan usage limits | yes |
| `settings`, `apps`, `clips`, `shelf` | data changed | no |
| `hover`, `keylock`, `notify`, `charging` | island input / notification events | no |
| `prompt`, `prompt-closed` | Claude Code permission prompt / question shown, resolved | no |
| `reminder-due` | `{id, text, ...}` | no |
| `dictation`, `model-progress` | HUD state; local model download progress | no |
| `shelf-drag` | drag phase (drop, ...) | no |

### Commands (TS -> Rust, `invoke`), from `generate_handler!` in lib.rs
- **Settings/appearance:** `get_settings`, `open_settings`, `set_general`, `list_displays`, `open_link`, `get_appearance`, `set_appearance`, `apply_theme`, `reset_appearance`, `set_layout`, `get_snapshot`
- **Tasks/notes:** `list_tasks`, `add_task`, `delete_task`, `list_notes`, `add_note`, `update_note`, `pin_note`, `delete_note`
- **Reminders:** `list_reminders`, `quick_due`, `add_reminder`, `snooze_reminder`, `done_reminder`, `delete_reminder`
- **Clipboard:** `list_clips`, `copy_clip`, `pin_clip`, `remove_clip`, `clear_clips`, `set_clip_paused`
- **Shelf:** `list_shelf`, `open_shelf`, `reveal_shelf`, `copy_shelf_path`, `remove_shelf`, `clear_shelf`, `drag_out`
- **Weather/calendar/media:** `set_location`, `set_calendar_url`, `open_join`, `media_control`, `media_seek`, `media_open_player`, `run_action`
- **Devices:** `bt_state`, `bt_set_radio`, `bt_connect`, `bt_disconnect`, `sound_state`, `set_volume`, `set_mute`, `audio_devices`, `set_audio_device`, `brightness_displays`, `set_brightness`
- **Launcher:** `list_apps`, `pick_app`, `remove_app`, `launch_app`
- **Window/menu:** `set_hit_rect`, `show_context_menu`
- **Claude Code:** `answer_prompt`, `connect_claude`, `disconnect_claude`
- **Dictation:** `dictation_settings`, `set_dictation_settings`, `download_model`, `model_status`, `set_cloud_key`, `clear_cloud_key`, `has_cloud_key`, `list_mics`, `dictation_cancel`

---

## 5. Database (SQLite)

File: `%APPDATA%\com.dynamicnotch.app\dynamic_notch.db` (Linux `~/.local/share/com.dynamicnotch.app/`). Schema version is in
`PRAGMA user_version`; current version **6**. Dictation settings (`dictation_*`) and `bridge_port` are rows in `settings`.

```sql
-- v1
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE tasks (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, category TEXT NOT NULL,
                    time TEXT NOT NULL, color TEXT NOT NULL);
-- v2
CREATE TABLE apps (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
                   kind TEXT NOT NULL CHECK (kind IN ('path','action')), target TEXT NOT NULL,
                   icon TEXT, position INTEGER NOT NULL);
-- v3
CREATE TABLE notes (id INTEGER PRIMARY KEY AUTOINCREMENT, body TEXT NOT NULL,
                    pinned INTEGER NOT NULL DEFAULT 0, updated_at INTEGER NOT NULL);
-- v4
CREATE TABLE reminders (id INTEGER PRIMARY KEY AUTOINCREMENT, text TEXT NOT NULL, due_at INTEGER NOT NULL,
                        repeat TEXT NOT NULL DEFAULT 'none' CHECK (repeat IN ('none','daily','weekdays','weekly')),
                        done INTEGER NOT NULL DEFAULT 0, snooze_until INTEGER, done_at INTEGER);
-- v5
CREATE TABLE clip_history (id INTEGER PRIMARY KEY, text TEXT NOT NULL, pinned INTEGER NOT NULL DEFAULT 0, at INTEGER NOT NULL);
-- v6
CREATE TABLE shelf (id INTEGER PRIMARY KEY AUTOINCREMENT, path TEXT NOT NULL UNIQUE, added_at INTEGER NOT NULL);
```

`clip_history` is only written when "remember across restarts" is on. Autostart is not in the DB (the OS is the source of truth).
**To migrate:** add `if version < 7 { ...; PRAGMA user_version = 7; }` in `Db::migrate`.

---

## 6. Run, test, build

Prereqs: Rust (MSVC toolchain), Node 20+, WebView2 (built into Windows 11).

```bash
npm install
npm run tauri dev          # dev mode with hot reload for TS/CSS
cd src-tauri && cargo test # Rust unit tests
npm run build              # typecheck + bundle the frontend only
npm run tauri build        # release exe + NSIS installer in src-tauri/target/release/bundle/
```

**Notifications (Windows):** Windows has to allow notification access for the app
(Settings → Privacy & security → Notifications). If it is denied, the monitor logs this and stops;
everything else keeps working.

---

## 7. Status

**Done (Windows)**
- [x] Scaffold, SQLite + migrations (v1-v6), monitors, commands with input validation
- [x] Dashboard with tabs/More, Home cards, music + lyrics, calendar, timer, launcher, notes, reminders, clipboard, sound/brightness, Bluetooth, shelf
- [x] Pal and 8 moods; Settings window; 7 themes; display choice + responsive scaling; hide in fullscreen
- [x] Claude Code bridge: sessions, prompts/questions (15 s timeout), hooks + chained statusLine install/restore, usage limits
- [x] Dictation: hotkey, mic capture, cloud OpenAI/Groq (keyring), paste; offline Parakeet (sherpa-onnx), text pipeline, hands-free, history + stats (Task 37)

**Not verified on hardware yet**
- [ ] Notification listener permission on an unpackaged build
- [x] Offline Parakeet build and model download end to end (Task 37; real WAV transcribed)
- [ ] Multi-monitor / mixed-DPI behaviour, fullscreen detection edge cases
- [ ] Release installer size

**Linux (all untested; pending)**
- [ ] Click-through / hover (`window_linux.rs`; layer-shell on Wayland)
- [ ] Media (MPRIS), notifications (D-Bus), accent, Caps/Num Lock, charging: `*_linux.rs` are stubs
- [ ] Launcher (open + icons), fullscreen detection, shelf drag-out
- [ ] Verify best-effort code: sound (`wpctl`/`pactl`), brightness, Bluetooth (`bluetoothctl`), clipboard (arboard)
- [ ] Dictation: key-combo hotkey (modifier-only is Windows only); paste is copy-only

---

## 8. Roadmap / ideas

1. Retry the album-art colour if the thumbnail isn't ready when the track changes.
2. Tray icon with Quit / Settings, for when the island is hidden.
3. Notification filtering (mute apps) -> new `muted_apps` table.
4. Auto-update via `tauri-plugin-updater` + GitHub releases.
5. Finish the Linux backends listed above.

---

## 9. Learning notes (Rust/Tauri concepts used here)

- **`tauri::command`**: a Rust function the frontend can call with `invoke("name", {args})`.
  Arguments are deserialized from JSON (camelCase in TS ↔ snake_case in Rust).
- **Events**: `app.emit("name", payload)` in Rust → `listen("name", cb)` in TS. Used for push updates.
- **Managed state**: `app.manage(AppState{..})`, then `State<AppState>` in commands or
  `app.state::<AppState>()` anywhere. `Mutex` gives safe shared access from many threads.
- **Threads**: every monitor is a plain `std::thread` loop. Simple and enough for this app; no async runtime needed.
- **WinRT from Rust**: the `windows` crate wraps Windows APIs. `IAsyncOperation::get()` blocks until the
  async call finishes (fine on a background thread, never on the UI thread).
- **`?` operator**: returns the error early. Most functions return `Result<T, E>`; errors become strings at the command boundary.
- **Capabilities** (`src-tauri/capabilities/`): what the webview may call. Kept to `core:default`, so
  the frontend can only reach the app through our own validated commands.
- **CSS `@property`**: registers typed custom properties (`--p`, `--angle`) so they can be animated.
  This is what drives the shine sweep and the neon border.
