# DynamicNotch

A sleek, animated **Dynamic Island for Windows**, built with **Rust + Tauri 2** and **SQLite**.

A small pill at the top of your screen shows the time and expands into live panels when you
hover it: system stats, music with synced lyrics, weather, tasks, month progress and quick controls.

> Architecture, internals and roadmap are in [plan.md](plan.md).

---

## Features

- **Fluid morphing.** Smooth size transitions with a shine sweep between states.
- **Hover panels.** Scroll on the island to switch between them:
  - **System Status**: CPU, RAM, storage, network up/down
  - **Music**: title/artist, play/pause/next/prev, **synced lyrics** (lrclib.net), animations tinted by the album art
  - **Weather**: current conditions + next 5 hours (Open-Meteo, no API key)
  - **Tasks**: "Next Up" list with an inline editor (add/delete), saved in SQLite
  - **Month Progress**: dot grid of the current month
  - **Basics**: control balls for Shutdown, Restart, Sleep, Explorer, Settings, Task Manager, Chrome, YouTube, CMD
- **Notifications.** Windows toast notifications appear in the island.
- **Caps Lock / Num Lock** indicators.
- **Charging animation** when you plug in the charger.
- **Styles.** Island: Default, Liquid Glass, Notch Nook. Music animation: Glow Sweep, Fluid Blobs, Neon Border.
- **Click-through.** The desktop under the island stays usable; only the pill takes the mouse.
- **Safe controls.** Shutdown/Restart need a second click to confirm.
- **Start with Windows.** Optional, off by default.

## Usage

| Action | What happens |
|---|---|
| Hover the island | Expands into the current panel |
| Scroll on the island | Next/previous panel |
| Right-click | Menu: animation style, island style, Start with Windows, Change Location, Quit |
| Pencil in Tasks | Open the task editor |
| Pencil in Weather | Change city |
| `<` `>` in Basics | Rotate the control balls |

## Install & run (from source)

**Requirements:** Windows 10/11, [Rust](https://rustup.rs) (MSVC toolchain), [Node.js](https://nodejs.org) 20+,
WebView2 (built into Windows 11).

```bash
git clone https://github.com/Shafat21/DynamicNotch.git
cd DynamicNotch
npm install
npm run tauri dev
```

### Build a release

```bash
npm run tauri build
```

The exe and NSIS installer are written to `src-tauri/target/release/bundle/`.

### Tests

```bash
cd src-tauri
cargo test
```

## Data & privacy

- Settings and tasks are stored locally in SQLite:
  `%APPDATA%\com.dynamicnotch.app\dynamic_notch.db`
- Network calls: **Open-Meteo** (weather + city search) and **lrclib.net** (lyrics, sends artist + title only).
  No accounts, no telemetry.

## Notification access

To show Windows notifications in the island, allow access under
**Settings → Privacy & security → Notifications**. If access is denied, every other feature still works.

## Tech stack

| Layer | Tech |
|---|---|
| App shell | Tauri 2 (WebView2) |
| Backend | Rust: `windows` crate (media sessions, notifications, Win32), `sysinfo`, `rusqlite`, `ureq`, `image` |
| Frontend | TypeScript + CSS (Vite), Material Design Icons |
| Storage | SQLite (bundled) |

## Project structure

```
src/            frontend (state machine, panels, styles)
src-tauri/src/  Rust backend
  monitors/     perf, media, weather, notify, system (keys, power, accent)
  db.rs         SQLite settings + tasks
  commands.rs   API called from the frontend
```

See [plan.md](plan.md) for the full file map, event/command contract and roadmap.

## Credits

Weather by [Open-Meteo](https://open-meteo.com), lyrics by [LRCLIB](https://lrclib.net).
