# PicoBar

**Ultra-light menu bar (macOS) and tray (Windows) monitor for Claude and OpenCode.**

Native Rust, no webview, no runtime: a ~1.8 MB binary that sleeps between
refreshes (about 11 MB of memory, 0% CPU when idle).

- Claude 5-hour session usage at a glance, with a ring gauge that turns amber at 80%
- Countdowns to the session and weekly resets
- Notifications when a limit resets or crosses 80%
- Live token counter for the current session (input, output, cache)
- OpenCode tokens and cost over the last 24 hours
- Active Claude Code sessions and subagents, and OpenCode sessions

## Install

Download the latest build from [Releases](https://github.com/lido772/picobar/releases):

- **macOS 11+** (Intel and Apple Silicon): unzip and move `PicoBar.app` to
  Applications. The app is not notarized yet, so the first time, right-click
  it and choose **Open**.
- **Windows 10+**: unzip and run `PicoBar.exe`.

To start PicoBar at login: macOS, System Settings › General › Login Items;
Windows, put a shortcut to `PicoBar.exe` in `shell:startup`.

## Build from source

```sh
cargo build --release
./target/release/picobar          # menu bar / tray app
./target/release/picobar --once   # print the current numbers and exit
```

## Where the numbers come from

| What | Source |
|---|---|
| Session and weekly limits | The usage data Claude Code shows in `/usage`, fetched with your existing Claude Code login (macOS Keychain, or `~/.claude/.credentials.json`) every 3 minutes |
| Claude tokens and active sessions | Claude Code's local logs in `~/.claude/projects/` |
| OpenCode tokens, cost and sessions | OpenCode's local database (`~/.local/share/opencode/opencode.db`), read-only |

The limits data is not a public API and may change. If it is unavailable, or
your login has expired, PicoBar says so and keeps showing local data. Your
login token never leaves your machine except to authenticate that request, and
is never written to disk or logged.

## Lightness budget

| Metric | Budget | v0.1.0 (macOS) |
|---|---|---|
| Binary | ≤ 3 MB | 1.8 MB |
| Idle memory | ≤ 20 MB | 11 MB |
| Idle CPU | ~0 % | 0 % |

Checked in CI by `scripts/size.sh`. History in [docs/SIZE.md](docs/SIZE.md).

## Roadmap

- Popover with Now / Agents / Trends views
- Badge when a Claude Code or OpenCode agent is waiting for your input
- Trends: daily tokens, week-over-week change, burn-rate projection
- Settings: threshold, refresh interval, menu bar display mode, launch at login

## License

MIT
