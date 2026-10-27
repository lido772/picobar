# Changelog

## [Unreleased]

### Fixed

- The popover header no longer says "Updating…" when Claude limits could not
  be read (it looked like a PicoBar update). It now shows "Loading…" before
  the first read and "Limits unavailable" when reading fails.

## [0.3.0] - 2026-10-27

### Added

- Claude permission prompts: an opt-in menu item, "Show Claude permission
  prompts", adds a Claude Code hook. A session waiting for you to approve a
  tool shows as "Needs permission" at the top of Agents, lights the coral
  badge and sends a notification. Your other hooks are kept and
  `settings.json` is backed up once before the first change.
- Settings submenu: warn threshold (70, 80 or 90%), limits refresh interval
  (3, 5, 10 or 15 min), what the menu bar shows (percent, time to reset or
  icon only; not on Windows) and Launch at login (a LaunchAgent on macOS,
  the Run key on Windows, an autostart entry on Linux).
- Light theme for the popover. Settings > Appearance: System (follows the OS
  on macOS and Windows, dark on Linux), Light or Dark.
- `picobar --render <dir> --demo --light` draws the light popover.
- Linux arm64 builds (`.deb` and `.tar.gz`); built-in updates pick the
  matching architecture.

### Changed

- Settings live in a `settings` file in the config folder; the old
  notifications switch is carried over.
- The Windows installer's start-at-sign-in option now uses the same Run key
  as the app, replaces the old Startup-folder shortcut and is removed on
  uninstall.
- PicoBar refuses to install the hook or the login item while it runs from
  the disk image or a translocated path on macOS; move it to Applications
  first.

## [0.2.0] - 2026-10-26

### Added

- Popover on left click, matching the design: Now, Agents and Trends tabs.
- Agents: Claude Code and OpenCode sessions active in the last hour; a coral
  badge in the menu bar and the popover when one is waiting for you.
- Trends: tokens per day over 7 or 30 days, change vs the previous week,
  burn-rate projection, usage heatmap and insights (35 days of history).
- `picobar --render <dir> [--demo]` draws the popover to image files.
- Linux build (x86_64, GTK 3 + AppIndicator) as a `.deb` and a portable
  `.tar.gz`; fonts come from fontconfig. Without a tray, or with
  `--window`, PicoBar opens as a small window.
- Windows installer (per user, no admin rights, optional start at sign-in)
  and a portable zip that keeps its settings next to `PicoBar.exe`.
- Built-in updates: a daily check of GitHub releases, an Update button in the
  popover and the menu, SHA-256 verification against `SHA256SUMS.txt`, then
  PicoBar replaces itself and restarts. `picobar --update` does it from a
  terminal.
- The popover header shows the running version.

### Changed

- Logs are read in bounded chunks and system fonts are memory-mapped,
  keeping idle memory around 16 MB.

## [0.1.0] - 2026-10-26

First release. A native menu bar (macOS) / tray (Windows) item, under 2 MB.

### Added

- Claude 5-hour session usage in the menu bar: ring gauge plus percentage,
  turning amber at 80%.
- Countdowns to the session and weekly resets, and weekly usage per model
  when your plan reports it.
- Token counter for the current session window (input, output, cache), read
  incrementally from Claude Code's local logs, subagents included.
- OpenCode tokens and estimated cost over the last 24 hours, from its local
  database.
- Active sessions: Claude Code sessions and subagents, OpenCode sessions.
- Notifications when the session or weekly limit resets, and when usage
  crosses 80%. Can be turned off from the menu.
- `picobar --once` prints the current numbers in a terminal.

### Notes

- Limits come from the same usage data Claude Code shows in `/usage`, using
  your existing Claude Code login. It is not a public API; if it changes or
  your login has expired, PicoBar keeps showing local token data.
- macOS: install from the `.dmg` (drag PicoBar to Applications). The app is
  not notarized yet: after the first launch attempt, go to System Settings ›
  Privacy & Security and click Open Anyway, or run
  `xattr -dr com.apple.quarantine /Applications/PicoBar.app`.
