# PicoBar architecture

## Lightness budget (hard limits, enforced in CI)

| Metric | Budget |
|---|---|
| Release binary (macOS x86_64 / arm64, Windows x64) | **≤ 3 MB** |
| Idle memory (physical footprint, as Activity Monitor shows; RSS counts shared system frameworks) | **≤ 20 MB** |
| Idle CPU | **~0 %** — the event loop sleeps between refreshes |
| Popover open → painted | ≤ 100 ms |
| Direct dependencies | ≤ 8, each justified below |

A change that breaks a budget is rejected, whatever the feature.

## Principles

1. **Native, no webview.** No Tauri, no Electron, no embedded browser.
2. **No async runtime.** No tokio. Polling every 60 s with plain `std` I/O is enough.
3. **Read local state first.** Files written by Claude Code / OpenCode, tailed incrementally (remember offsets, never re-read whole logs).
4. **One network call max per refresh** (Claude limits). TLS via the OS stack where possible.
5. **Fail soft.** A broken source shows "–", never crashes or spams.
6. **Never log secrets.** OAuth tokens stay in memory, never on disk or in logs.

## Layout

```
src/
  main.rs        event loop, tray item, menu wiring
  model.rs       plain data types + formatting (pure, unit-tested)
  icon.rs        procedurally drawn ring gauge (no image files)
  sources/
    claude.rs    limits (5h session, weekly, Opus) + session tokens
    opencode.rs  tokens + cost from local storage
    agents.rs    running / needs-input agents from ~/.picobar/events.jsonl
```

Planned: `popover/` (rich popover — Now / Agents / Trends tabs), `notify.rs`,
`platform/{macos,windows}.rs`, `history.rs` (compact daily aggregates for Trends).

## Dependencies

| Crate | Why | Alternative rejected |
|---|---|---|
| `tao` | Native event loop on macOS + Windows | `winit` (similar weight; tao pairs with tray-icon) |
| `tray-icon` | Menu bar item / tray icon + native menus | hand-written objc2 + Win32 (later, if it saves size) |

To evaluate (measure binary size before adopting any of them):

- Popover rendering: native menu first (zero cost) → custom popover with a tiny
  software renderer (`tiny-skia`) vs `egui` behind a cargo feature. Measure.
- Notifications: `notify-rust` vs direct `UNUserNotificationCenter` / WinRT toast.
- HTTP for Claude limits: `ureq` with `native-tls` vs OS APIs.
- JSON: `serde_json` vs a minimal hand-rolled line scanner for hot paths.

## Data sources (verify every one against the installed tool before coding)

- **Claude limits** — the usage data Claude Code's `/usage` shows, fetched with
  the user's existing Claude Code login. Not a public API: may change; PicoBar
  must degrade gracefully.
- **Claude tokens** — `usage` fields in `~/.claude/projects/**/*.jsonl`.
- **OpenCode** — local storage under OpenCode's data directory.
- **Agents / needs-you** — hooks installed by PicoBar (Claude Code
  `Notification`, `UserPromptSubmit`, `Stop`; an OpenCode plugin) append one
  JSON line per event to `~/.picobar/events.jsonl`.

## Design reference

The UI follows the PicoBar design canvas (menu bar item, popover Now / Agents /
Trends, Windows flyout, settings, notifications). Tokens: graphite `#16181B`,
surface `#1C1F23`, text `#ECEDEE`, muted `#8E959C`, mint accent `#5EE6A8`,
amber warning `#F5B547`, coral needs-you `#FF7A6B`. Numbers in a monospace face.
