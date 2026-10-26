# PicoBar architecture

## Lightness budget (hard limits, enforced in CI)

| Metric | Budget |
|---|---|
| Release binary (macOS universal slice / Windows x64) | **≤ 3 MB** |
| Idle memory (physical footprint, as Activity Monitor shows) | **≤ 20 MB** |
| Idle CPU | **~0 %** — the event loop sleeps between refreshes |
| Direct dependencies | ≤ 8, each justified below |

A change that breaks a budget is rejected, whatever the feature.

## Principles

1. **Native, no webview.** No Tauri, no Electron, no embedded browser.
2. **No async runtime.** A 60 s tick with plain `std` I/O; the one network call
   runs on a short-lived thread and wakes the loop when done.
3. **Read local state first**, incrementally: Claude Code logs are tailed from
   the last byte offset; OpenCode's database is opened read-only.
4. **Use what the OS ships** instead of crates: `curl` for HTTPS (macOS and
   Windows 10+ include it), `security` for the macOS Keychain, `osascript` /
   PowerShell for notifications.
5. **Fail soft.** A broken source shows "–" and a short reason, never crashes.
6. **Never log secrets.** The OAuth token is passed to curl over stdin (not the
   command line), never written to disk or logged.

## Layout

```
src/
  main.rs            event loop, tray item, menu, refresh scheduling
  model.rs           Snapshot + menu text (pure, unit-tested)
  alerts.rs          when to notify: resets and the 80% threshold
  notify.rs          native notifications via OS tools
  icon.rs            procedurally drawn ring gauge (template image on macOS)
  util.rs            time parsing, countdowns, paths, hidden child processes
  sources/
    claude_limits.rs session / weekly limits (network, every 3 min)
    claude_logs.rs   tokens + active sessions from ~/.claude/projects
    opencode.rs      tokens, cost, active sessions from opencode.db
```

## Dependencies

| Crate | Why | Size note |
|---|---|---|
| `tao` | Native event loop on macOS + Windows | core |
| `tray-icon` | Menu bar item / tray icon + native menus | core |
| `serde` + `serde_json` | Parse Claude Code log lines and the limits response | small |
| `rusqlite` (bundled) | Read OpenCode's SQLite database | ~930 KB; macOS's system libsqlite3 crashed when linked directly, Windows has none |

Candidates for later, measured before adoption: a popover renderer
(`tiny-skia` vs `egui` behind a feature), a smaller SQLite build.

## Data sources (verified 2026-10-26)

- **Claude limits** — `GET https://api.anthropic.com/api/oauth/usage` with the
  Claude Code OAuth token and `anthropic-beta: oauth-2025-04-20`. Buckets
  `five_hour`, `seven_day`, `seven_day_opus`, `seven_day_sonnet`, each
  `{utilization: 0–100, resets_at: RFC 3339 | null}`. Not a public API. An
  expired token returns 401; PicoBar never refreshes tokens itself.
- **Claude tokens** — assistant lines in `~/.claude/projects/<project>/*.jsonl`
  and `<session>/subagents/*.jsonl`; `message.usage` holds input, output,
  cache creation and cache read tokens. One API message can span several lines
  with the same `message.id`: deduplicated by id.
- **OpenCode** — `opencode.db`, table `session_message` (type `assistant`,
  `data.tokens.{input,output,reasoning,cache.read,cache.write}`, `data.cost`,
  `time_created` in ms) and `session_v2` (`time_updated`, `time_idle`).

## Design reference

Tokens for the upcoming popover: graphite `#16181B`, surface `#1C1F23`, text
`#ECEDEE`, muted `#8E959C`, mint accent `#5EE6A8`, amber warning `#F5B547`,
coral needs-you `#FF7A6B`. Numbers in a monospace face.
