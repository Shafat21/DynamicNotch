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
 │  monitors/ (one OS thread each)          hit.rs (25 ms loop)     │
 │   perf    → "perf"   every 1 s            cursor inside island?  │
 │   media   → "media", "lyric"  100 ms      → toggle click-through │
 │   weather → "weather" every 30 min        → emit "hover"         │
 │   notify  → "notify"  every 1 s                                  │
 │   system  → "keylock", "charging", "accent"                      │
 │                                                                  │
 │  db.rs (SQLite)   commands.rs (invoke API)   menu.rs (right-click)│
 └───────────────▲──────────────────────────────┬───────────────────┘
          invoke │                        emit  │  events
 ┌───────────────┴──────── WebView (src) ───────▼───────────────────┐
 │  main.ts   state machine: idle | hover | notify                  │
 │            picks size per state/feature, animates, swaps panels  │
 │  panels.ts tasks list/editor, month dots, basics control balls   │
 │  styles.css all visuals (shape, backgrounds, media fx, shine)    │
 └──────────────────────────────────────────────────────────────────┘
```

**The window trick.** The real window is a fixed 1200×700 transparent, frameless, always-on-top,
no-taskbar window. Only the pill is visible. Rust polls the cursor every 25 ms; outside the pill
the window ignores the mouse (`set_ignore_cursor_events(true)`) so clicks reach the desktop.
The frontend tells Rust where the pill is with `set_hit_rect`.

**State machine** (`src/main.ts`):
- `idle` → small pill: clock, or song title / lyric if the media feature is selected and music plays.
- `hover` → the cursor is on the island. Scrolling cycles the features
  `perf → media → weather → calendar → month → basics`.
- `notify` → a notification or Caps/Num Lock change. Shown for 3.5 s / 2.5 s, then goes back.
- `charging` overrides the size (850×40 + glowing beams) for about 3 s.

`transition()` sets the target size, starts the CSS morph (850 ms), a shine sweep, and the
content cross-fade, then swaps the visible panel at 250 ms (when the content is invisible).

---

## 3. File map

```
DynamicNotch/
├─ plan.md                ← this file
├─ index.html             ← all panel markup (static), filled in by TS
├─ src/
│  ├─ main.ts             ← state machine, event listeners, input wiring, boot
│  ├─ panels.ts           ← tasks, month progress, basics control balls
│  ├─ icons.ts            ← MDI icon table + SVG helper
│  └─ styles.css          ← every visual: shape, notch ears, backgrounds, media fx, shine, charging
├─ src-tauri/
│  ├─ Cargo.toml          ← Rust deps
│  ├─ tauri.conf.json     ← window config (transparent, always-on-top, 1200×700), bundle
│  ├─ capabilities/       ← frontend permissions (core only; app features go through our commands)
│  ├─ icons/              ← generated from ../app-icon.svg (`npx tauri icon app-icon.svg`)
│  └─ src/
│     ├─ main.rs          ← entry point
│     ├─ lib.rs           ← app setup, AppState, publish() helper, command registration
│     ├─ db.rs            ← SQLite schema, migrations, settings + tasks (unit tested)
│     ├─ commands.rs      ← #[tauri::command] API, validates all webview input
│     ├─ hit.rs           ← click-through / hover detection loop
│     ├─ menu.rs          ← native right-click menu
│     ├─ actions.rs       ← allow-list of shell actions (shutdown, open apps…)
│     └─ monitors/
│        ├─ mod.rs        ← spawn_all(), shared HTTP client (10 s timeout)
│        ├─ perf.rs       ← sysinfo: CPU, RAM, system drive, network speed
│        ├─ media.rs      ← Windows media session, album-art colour, LRC lyrics
│        ├─ weather.rs    ← Open-Meteo forecast + geocoding
│        ├─ notify.rs     ← Windows toast notification listener
│        └─ system.rs     ← accent colour, Caps/Num Lock, charger plug-in
└─ app-icon.svg           ← source icon
```

---

## 4. Backend ⇄ frontend contract

### Events (Rust → TS)
| Event | Payload | Sticky¹ |
|---|---|---|
| `perf` | `{cpu, ram, disk, down, up}` (percent, bytes/s) | yes |
| `media` | `{state: "Playing"\|"Paused"\|"Idle", title, artist, accent}` | yes |
| `lyric` | `string` (current line, `""` = none) | yes |
| `weather` | `{city, temp, desc, icon, hourly: [{time, temp, icon}×5]}` | yes |
| `accent` | `"#rrggbb"` Windows accent colour | yes |
| `settings` | `{location, lat, lon, islandStyle, animationStyle, autostart}` | no |
| `hover` | `bool` | no |
| `keylock` | `{name: "Caps Lock"\|"Num Lock", on}` | no |
| `notify` | `{app, title, text}` | no |
| `charging` | `()` | no |
| `edit-location` | `()` (menu → open the location input) | no |

¹ Sticky events are cached in `AppState.last`; the page calls `get_snapshot` on load to catch up.

### Commands (TS → Rust, `invoke`)
`get_settings`, `get_snapshot`, `list_tasks`, `add_task{name,category,time}`, `delete_task{id}`,
`set_location{city}`, `media_control{action: toggle|next|prev}`, `run_action{id}`,
`set_hit_rect{rect:{x,y,w,h}}`, `show_context_menu`.

---

## 5. Database (SQLite)

File: `%APPDATA%\com.dynamicnotch.app\dynamic_notch.db`. Schema version is in `PRAGMA user_version`.

```sql
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
-- keys: location, lat, lon, island_style, animation_style
CREATE TABLE tasks (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL, category TEXT NOT NULL, time TEXT NOT NULL, color TEXT NOT NULL
);
```

Seeded with defaults on first run. "Start with Windows" is **not** in the
DB; the registry (via `tauri-plugin-autostart`) is the source of truth.

**To migrate:** add `if version < 2 { ...; PRAGMA user_version = 2; }` in `Db::migrate`.

---

## 6. Run, test, build

Prereqs: Rust (MSVC toolchain), Node 20+, WebView2 (built into Windows 11).

```bash
npm install
npm run tauri dev          # dev mode with hot reload for TS/CSS
cd src-tauri && cargo test # Rust unit tests (db, LRC parser, weather parser, colour, actions, hit-rect)
npm run build              # typecheck + bundle the frontend only
npm run tauri build        # release exe + NSIS installer in src-tauri/target/release/bundle/
```

**Notifications:** Windows has to allow notification access for the app
(Settings → Privacy & security → Notifications). If it is denied, the monitor logs this and stops;
everything else keeps working.

---

## 7. Status

**Done and verified**
- [x] Project scaffold (Tauri 2, Vite, TS, icons)
- [x] SQLite layer + migrations + seed — 3 unit tests
- [x] All monitors written: perf, media + lyrics, weather, notifications, keys, power, accent
- [x] Commands, input validation, action allow-list, right-click menu, autostart toggle
- [x] Frontend: state machine, all 6 panels, notify/keylock, charging, 3 media animations,
      3 island styles, task editor, location editor, basics balls with confirm
- [x] `cargo test`: 9/9 pass · `tsc` + `vite build`: clean · `cargo clippy`: 2 warnings fixed (clippy not re-run after the fix)

**Not yet verified (next session)**
- [ ] Launch with `npm run tauri dev` and check the result on screen
- [ ] Transparency, click-through and hover feel right on a real desktop, with DPI scaling
- [ ] Media monitor with Spotify / browser / Windows media player (title, colour, lyrics sync)
- [ ] Notification listener permission on an unpackaged (non-MSIX) build
- [ ] Caps/Num Lock detection from a background thread (`GetKeyState`)
- [ ] Release build + installer size

---

## 8. Roadmap / ideas

1. **Polish pass after first run:** tune timings, blob motion and the shine size.
2. Make the basics items configurable (store them in a new `basics` table).
3. Retry the album-art colour if the thumbnail isn't ready when the track changes (currently falls back to black).
4. Multi-monitor: choose which monitor; re-centre when the display changes.
5. Tray icon with Quit / Settings, for when the island is hidden.
6. Settings panel inside the island (style, animation, location, autostart), so the menu is optional.
7. Notification filtering (mute apps) → new `muted_apps` table.
8. Auto-update via `tauri-plugin-updater` + GitHub releases.

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
