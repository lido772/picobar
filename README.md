# PicoBar

**Ultra-light menu bar (macOS) and tray (Windows) monitor for Claude and OpenCode.**

See your Claude limits at a glance, get notified when they reset, and know
which of your coding agents is waiting for you. Native Rust, no webview, no
runtime: a ~2 MB app that uses about 16 MB of memory and 0% CPU when idle.

<p align="center">
  <img src="docs/screenshots/menubar.png" width="440" alt="PicoBar in a dark and a light menu bar: a ring gauge with the session percentage, amber with a coral dot when an agent is waiting">
</p>

<table>
  <tr>
    <th>Now</th>
    <th>Agents</th>
    <th>Trends</th>
  </tr>
  <tr>
    <td><img src="docs/screenshots/now.png" width="260" alt="Now tab: session usage with reset countdown, token counter, burn rate, weekly limits and OpenCode usage"></td>
    <td><img src="docs/screenshots/agents.png" width="260" alt="Agents tab: Claude Code and OpenCode sessions, with the ones waiting for you highlighted"></td>
    <td><img src="docs/screenshots/trends.png" width="260" alt="Trends tab: daily tokens over 7 days, burn-rate projection, usage heatmap and insights"></td>
  </tr>
</table>

<sub>Screenshots use demo data (`picobar --render <dir> --demo`).</sub>

## Features

**In the menu bar** — a ring gauge with your Claude 5-hour session usage. It
turns amber at 80%, and gets a coral dot when an agent is waiting for you.
Left click opens the popover; right click opens a compact menu.

**Now**
- Session usage with countdown and local time of the reset
- Token counter for the current session: total, input, output, cache
- Burn rate, and when you would hit the limit at this pace
- Weekly limits, per model when your plan has them
- OpenCode: tokens and estimated cost over 24 hours, last 7 days, providers

**Agents**
- Every Claude Code and OpenCode session active in the last hour
- Sessions that finished their turn and are waiting for you come first, with
  Claude's last message
- Running sessions with their subagents and token usage

**Trends**
- Tokens per day over 7 or 30 days (Claude and OpenCode), change vs the
  previous week
- Burn-rate projection for the current session
- Heatmap of when you use the most, over the last 4 weeks
- Insights: top model, cache share, busiest day

**Notifications** when your session or weekly limit resets, and when usage
crosses 80%. Can be turned off from the popover or the menu.

## Install

Download the latest build from [Releases](https://github.com/lido772/picobar/releases):

- **macOS 11+** (Intel and Apple Silicon): open the `.dmg` and drag PicoBar to
  Applications. The app is not notarized yet, so macOS says it "could not
  verify PicoBar is free of malware" the first time. Either:
  - open **System Settings › Privacy & Security**, scroll down and click
    **Open Anyway** next to PicoBar, then confirm; or
  - run once in Terminal: `xattr -dr com.apple.quarantine /Applications/PicoBar.app`
- **Windows 10+**: unzip and run `PicoBar.exe`.

To start PicoBar at login: macOS, System Settings › General › Login Items;
Windows, put a shortcut to `PicoBar.exe` in `shell:startup`.

## Build from source

```sh
cargo build --release
./target/release/picobar                 # menu bar / tray app
./target/release/picobar --once          # print the current numbers and exit
./target/release/picobar --render out    # draw the popover tabs to BMP files
```

## Where the numbers come from

| What | Source |
|---|---|
| Session and weekly limits | The usage data Claude Code shows in `/usage`, fetched with your existing Claude Code login (macOS Keychain, or `~/.claude/.credentials.json`) every 3 minutes |
| Claude tokens, history and sessions | Claude Code's local logs in `~/.claude/projects/` (last 35 days, read incrementally) |
| OpenCode tokens, cost and sessions | OpenCode's local database (`~/.local/share/opencode/opencode.db`), read-only |

The limits data is not a public API and may change. If it is unavailable, or
your login has expired, PicoBar says so and keeps showing local data. Your
login token never leaves your machine except to authenticate that request, and
is never written to disk or logged. Everything else stays local.

## How it stays light

- No webview, no GPU stack: the popover is drawn by a small software painter
  and shown with [`softbuffer`](https://crates.io/crates/softbuffer).
- Text uses the fonts your OS already has (San Francisco, Segoe UI),
  memory-mapped instead of embedded.
- HTTPS, Keychain and notifications go through tools the OS ships (`curl`,
  `security`, `osascript`, PowerShell) instead of extra libraries.
- The app sleeps between refreshes and reads only the bytes appended to logs.

| Metric | Budget | Current (macOS) |
|---|---|---|
| Binary | ≤ 3 MB | 2.0 MB |
| Idle memory | ≤ 20 MB | 16 MB |
| Idle CPU | ~0 % | 0 % |

Checked in CI by `scripts/size.sh`. History in [docs/SIZE.md](docs/SIZE.md).

## Roadmap

- Detect when an agent is blocked on a permission prompt (Claude Code hook)
- Settings: threshold, refresh interval, menu bar display mode, launch at login
- Light theme for the popover
- Signed and notarized macOS builds

## License

MIT
