<div align="center">

<img src="docs/assets/icon.png" width="128" alt="Dynamic Notch Island icon">

# Dynamic Notch Island

A Dynamic Island for Windows and Linux, with Pal, your Claude Code companion.

[![Latest release](https://img.shields.io/github/v/release/Shafat21/DynamicNotch?include_prereleases)](https://github.com/Shafat21/DynamicNotch/releases)
[![Downloads](https://img.shields.io/github/downloads/Shafat21/DynamicNotch/total)](https://github.com/Shafat21/DynamicNotch/releases)
[![Stars](https://img.shields.io/github/stars/Shafat21/DynamicNotch)](https://github.com/Shafat21/DynamicNotch/stargazers)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)
![Platforms](https://img.shields.io/badge/platform-Windows%20%7C%20Linux-lightgrey)
![Built with Tauri 2](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-black?logo=rust)

<br>

[![Download for Windows](https://img.shields.io/badge/Download_for-Windows-0078D4?style=for-the-badge&logo=data:image/svg%2Bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAyNCAyNCI%2BPHBhdGggZmlsbD0iI2ZmZiIgZD0iTTAgMy40IDkuOCAydjkuNUgwek0xMSAxLjggMjQgMHYxMS41SDExek0wIDEyLjVoOS44VjIyTDAgMjAuNnpNMTEgMTIuNWgxM1YyNGwtMTMtMS44eiIvPjwvc3ZnPg%3D%3D)](https://github.com/Shafat21/DynamicNotch/releases/latest)
[![Download for Linux](https://img.shields.io/badge/Download_for-Linux-FCC624?style=for-the-badge&logo=linux&logoColor=black)](https://github.com/Shafat21/DynamicNotch/releases/latest)

**Windows:** `.exe` installer &nbsp;·&nbsp; **Linux:** `.deb`, `.rpm`, `.AppImage`

</div>

---

A small pill sits at the top of your screen and shows the time. Hover it and it morphs into a dashboard;
when Claude Code needs you, Pal tells you and you can answer right from the island.

> Architecture, internals and roadmap are in [plan.md](plan.md).

## Features

**Dashboard (hover the island)** - a tab bar over these pages (tabs you can reorder or hide in Settings, extra ones go under **More**):
- **Home:** configurable cards (clock, Pal, music, calendar, launcher, focus, notes, reminders, and optional system, tasks, sound, devices, dictation).
- **Music player:** album art, title/artist, seek by click or drag, play/pause/next/prev, synced lyrics.
- **Calendar:** today's agenda from your iCal link, with Join buttons for online meetings.
- **Pal**, **Timer** (focus timer, 5 minute steps), **Weather**, **System stats** (CPU, RAM, storage, network), **Tasks**, **Month progress**.
- **Launcher dock:** pin apps and start them with one click; extra apps collapse into a +N overflow.
- **Notes** (pinnable) and **Reminders** with snooze / done and a chime.
- **Clipboard history:** text only, kept in memory by default (optional "remember across restarts"); copies that password managers mark as private are skipped (Windows).
- **Sound & brightness:** volume, mute, output device, per-display brightness.
- **Bluetooth devices:** radio on/off, connect / disconnect paired devices.
- **Shelf:** drop files on the island to park them; open, reveal, copy path, drag back out (Windows).

**Pal**
- An animated face with 8 moods: calm, happy, listening, thinking, needs-you, done, napping and broke.
- Lives in the idle pill next to the clock and on the assistant card; moods follow what Claude Code is doing.

**Claude Code companion**
- Live status of your Claude Code sessions (working, waiting for you, done, error).
- Answer permission prompts (allow / deny) and questions from the island instead of the terminal.
- **Usage limits ring:** your 5-hour and weekly plan usage with reset times, read from Claude Code's official `statusLine` `rate_limits`.

**Voice dictation**
- Hold **Ctrl+Win**, speak, release: the text is typed into the focused app.
- Cloud transcription via OpenAI or Groq (API key kept in the OS credential store). Local Whisper is an optional build feature, see [Dictation](#dictation).

**Settings window**
- Themes: Graphite, Midnight, Aurora, Ember, Lime, Rose, Pure black; line or colour-badge icons; density, sizes, tabs and Home layout.

**Fits your desktop**
- **Hides in fullscreen** games, videos and apps (Windows), and comes back when you leave fullscreen.
- **Choose the display** (primary or a specific screen) and **responsive scaling** that follows resolution,
  DPI and monitor changes (Island Size: Auto, Small, Medium, Large).
- **Caps Lock / Num Lock** indicators, **charging** animation and **notifications** in the island (Windows).
- **Click-through:** the desktop under the island stays usable; only the pill takes the mouse (Windows).

## Platform support

| Platform | Status |
|---|---|
| Windows 10 / 11 | ✅ Supported |
| Linux, GNOME | 🧪 Code present but untested; much is still stubbed (see below) |
| Linux, KDE Plasma | 🧪 Untested |
| macOS | ❌ Not supported |

Linux today: the window is placed but click-through/hover detection, media (MPRIS), notifications,
launcher, fullscreen detection, shelf drag-out and modifier-only dictation hotkeys are **not implemented**.
Sound (`wpctl`/`pactl`), brightness, Bluetooth (`bluetoothctl`) and clipboard history have best-effort code that has not
been compiled or run on real hardware. Dictation on Linux copies the text and you press Ctrl+V.

## Install

**Windows:** download the `.exe` installer from the [latest release](https://github.com/Shafat21/DynamicNotch/releases/latest)
and run it.

**Linux:** download the package for your distro from the [latest release](https://github.com/Shafat21/DynamicNotch/releases/latest):

```bash
# Debian / Ubuntu
sudo apt install ./*.deb

# Fedora / openSUSE
sudo dnf install ./*.rpm

# Any distro
chmod +x *.AppImage
./*.AppImage
```

Linux builds are experimental, see [Platform support](#platform-support).

## Usage

| Action | What happens |
|---|---|
| Hover the island | Opens the dashboard |
| Click a tab | Opens its page (music, calendar, timer, ...) |
| Right-click | Menu: Settings…, Animation Style, Connect / Disconnect Claude Code, Quit (display, island size, autostart and fullscreen options live in Settings) |
| ＋ in the launcher | Add an app |
| Right-click an app twice | Remove it from the launcher |
| ＋ on the calendar | Paste your iCal link |

## Claude Code

Right-click the island and choose **Connect Claude Code**:

- Adds Dynamic Notch Island's hooks to `~/.claude/settings.json` and sets `statusLine` to
  `dynamic-notch.exe --statusline`, which feeds the usage ring and then runs your existing status line (chained), so yours keeps working.
- The file is backed up first as `settings.json.dynamicnotch.bak`. **Disconnect** removes only what it added and restores your original `statusLine`.
- The hooks talk to a local endpoint on `127.0.0.1`, default port **47863** (configurable), protected by a random token. Nothing leaves your machine.
- Permission prompts and questions wait **15 seconds** in the island; if you don't answer, Claude Code falls
  back to asking in the terminal.
- Usage limits need a Claude Pro/Max plan and an open Claude Code session (Claude Code only reports `rate_limits` then).

## Dictation

- **Setup:** Settings → Features → Dictation, choose OpenAI or Groq and add your API key. Then hold **Ctrl+Win** (changeable) and speak.
- **Privacy:** audio stays in memory and is never written to disk. The API key is stored in Windows Credential Manager
  (Secret Service on Linux), not in the database. With cloud dictation the audio is sent to the provider you chose.
- **Local Whisper (optional):** not in the default build. Build with `cd src-tauri && cargo build --features local-whisper`
  (needs [CMake](https://cmake.org) and LLVM/libclang). Models (tiny/base/small) are downloaded from Hugging Face on demand and SHA-256 verified.

## Build from source

**Windows:** [Rust](https://rustup.rs) (MSVC toolchain), [Node.js](https://nodejs.org) 20+, WebView2
(built into Windows 11).

**Linux (Debian/Ubuntu):** Rust, Node.js 20+, and:

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-layer-shell-dev build-essential curl wget file \
  libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

Then:

```bash
git clone https://github.com/Shafat21/DynamicNotch.git
cd DynamicNotch
npm install
npm run tauri dev      # run in development
npm run tauri build    # installers in src-tauri/target/release/bundle/
cd src-tauri && cargo test
```

Dual-booting to test on Linux? See [plan.md](plan.md).

## Releasing

Push a tag `vX.Y.Z` and GitHub Actions builds Windows and Linux packages into a **draft release**.
Review it and publish.

```bash
git tag v0.2.0
git push origin v0.2.0
```

## Data & privacy

- Settings, tasks, notes, reminders, launcher apps, shelf entries and (only if you enable it) clipboard history are stored locally in SQLite:
  - Windows: `%APPDATA%\com.dynamicnotch.app\dynamic_notch.db`
  - Linux: `~/.local/share/com.dynamicnotch.app/dynamic_notch.db`
- Network calls: **Open-Meteo** (weather and city search), **LRCLIB** (lyrics; sends artist and title only),
  **your calendar link**, **your chosen dictation provider** (OpenAI or Groq, only when cloud dictation is used) and
  **Hugging Face** (only when you download a local Whisper model). The calendar link stays on your machine.
- No accounts, no telemetry.

## Star History

<a href="https://www.star-history.com/?type=date&repos=Shafat21%2FDynamicNotch">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/chart?repos=Shafat21/DynamicNotch&type=date&theme=dark&legend=top-left" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/chart?repos=Shafat21/DynamicNotch&type=date&legend=top-left" />
   <img alt="Star History Chart" src="https://api.star-history.com/chart?repos=Shafat21/DynamicNotch&type=date&legend=top-left" />
 </picture>
</a>

## Credits

Weather by [Open-Meteo](https://open-meteo.com), lyrics by [LRCLIB](https://lrclib.net).

## License

[Apache-2.0](LICENSE)
