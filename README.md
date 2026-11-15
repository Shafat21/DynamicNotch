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

**Pal**
- An animated face with 8 moods: calm, happy, listening, thinking, needs-you, done, napping and broke.
- Lives in the idle pill next to the clock and on the assistant card; moods follow what Claude Code is doing.

**Claude Code companion**
- Live status of your Claude Code sessions (working, waiting for you, done, error).
- Answer permission prompts (allow / deny) and questions from the island instead of the terminal.

**Dashboard (hover the island)**
- **Music player:** album art, title/artist, seek by click or drag, play/pause/next/prev, synced lyrics.
- **Calendar:** today's agenda from your iCal link, with Join buttons for online meetings.
- **Focus timer:** 25 minutes by default, adjust in 5 minute steps.
- **System stats:** CPU, RAM, storage, network up/down.
- **Weather:** current conditions and the next hours.
- **Tasks:** a "Next Up" list with an inline editor, saved locally.
- **Month progress:** a dot grid of the current month.

**Quick app launcher**
- Pin your apps to the dashboard and start them with one click.

**Fits your desktop**
- **Liquid Glass** style with real background blur on Windows and KDE Plasma (CSS glass elsewhere).
- **Hides in fullscreen** games, videos and apps, and comes back when you leave fullscreen.
- **Choose the display** (primary or a specific screen) and **responsive scaling** that follows resolution,
  DPI and monitor changes (Island Size: Auto, Small, Medium, Large).
- **Caps Lock / Num Lock** indicators, **charging** animation and **notifications** in the island.
- **Click-through:** the desktop under the island stays usable; only the pill takes the mouse.

## Platform support

| Platform | Status |
|---|---|
| Windows 10 / 11 | ✅ Supported |
| Linux, GNOME (Wayland via XWayland) | ✅ Supported |
| Linux, KDE Plasma (layer-shell) | 🧪 Untested |
| macOS | ❌ Not supported |

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

On Linux, notifications are read by monitoring the D-Bus session bus (if your system refuses this,
everything else still works). The NVIDIA rendering workaround is applied automatically.

## Usage

| Action | What happens |
|---|---|
| Hover the island | Opens the dashboard |
| Click a tile | Opens its panel (music, calendar, timer, ...) |
| ⌂ | Back to the dashboard |
| Right-click | Menu: styles, Display, Island Size, Hide in fullscreen apps, Connect Claude Code, Start with Windows / Start at login, Quit |
| ＋ in the launcher | Add an app |
| Right-click an app twice | Remove it from the launcher |
| ＋ on the calendar | Paste your iCal link |

## Claude Code

Right-click the island and choose **Connect Claude Code**:

- Adds Dynamic Notch Island's hooks to `~/.claude/settings.json`. A backup of the file is kept next to it.
- **Disconnect** removes only the hooks it added; your other settings are untouched.
- The hooks talk to a local endpoint on `127.0.0.1:47863`, protected by a random token. Nothing leaves your machine.
- Permission prompts and questions wait **15 seconds** in the island; if you don't answer, Claude Code falls
  back to asking in the terminal.

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

- Settings, tasks and launcher apps are stored locally in SQLite:
  - Windows: `%APPDATA%\com.dynamicnotch.app\dynamic_notch.db`
  - Linux: `~/.local/share/com.dynamicnotch.app/dynamic_notch.db`
- Network calls: **Open-Meteo** (weather and city search), **LRCLIB** (lyrics; sends artist and title only)
  and **your calendar link**. The calendar link stays on your machine.
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
