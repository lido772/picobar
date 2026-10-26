# Changelog

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
  not notarized yet: on first launch, right-click it and choose Open.
