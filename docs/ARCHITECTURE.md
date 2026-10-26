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
  alerts.rs          when to notify: resets and the warning threshold
  settings.rs        user settings: key=value file in config_dir (pure parse, tested)
  autostart.rs       launch at login: LaunchAgent / HKCU Run / XDG autostart
  notify.rs          native notifications via OS tools
  icon.rs            procedurally drawn ring gauge (template image on macOS)
  util.rs            time parsing, countdowns, paths, portable mode, child processes
  update.rs          release check, verified download (SHA-256), self-replace
  demo.rs            fictional data for screenshots
  ui/                popover window: painter, text, layout
  sources/
    claude_limits.rs session / weekly limits (network, every 3 min by default)
    claude_logs.rs   tokens + active sessions from ~/.claude/projects
    opencode.rs      tokens, cost, active sessions from opencode.db
    claude_hook.rs   `--hook` entry, permission markers, hook install/uninstall
```

## Dependencies

| Crate | Why | Size note |
|---|---|---|
| `tao` | Native event loop on macOS + Windows | core |
| `tray-icon` | Menu bar item / tray icon + native menus | core |
| `serde` + `serde_json` | Parse Claude Code log lines and the limits response | small |
| `rusqlite` (bundled) | Read OpenCode's SQLite database | ~930 KB; macOS's system libsqlite3 crashed when linked directly, Windows has none |
| `softbuffer` | Put our pixels in the popover window (no GPU stack) | small |
| `ab_glyph` | Rasterize text with the system's fonts (memory-mapped, not embedded) | small |
| `libc` / `windows-sys` | Local time offset; HKCU Run key (`Win32_System_Registry`, from 2026-10-26, pending size-guardian) | already in the tree |

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

## Permission prompts (decided 2026-10-26)

Logs cannot show that Claude Code is waiting on a permission dialog, so an
opt-in Claude Code hook tells us. No new crate, no shell script.

- **One hook, one event.** `Notification` with matcher `permission_prompt`.
  It is read-only: unlike `PermissionRequest`, nothing it prints can approve
  or deny a tool call. Command string `"<absolute path of the running binary>" --hook`: the path
  is quoted so spaces are safe; same on macOS, Linux and Windows.
- **`picobar --hook`** runs before the event loop: reads stdin JSON with
  `serde_json`, takes `session_id`, accepts only `[A-Za-z0-9_-]{1,64}`
  (it becomes a filename), touches `config_dir()/waiting/<session_id>`,
  exits 0. Silent and quick, whatever happens. Portable mode works because
  `config_dir()` resolves from the same executable.
- **No clear hooks.** `<session_id>` is the stem of
  `~/.claude/projects/<project>/<session_id>.jsonl`. A marker counts only
  while it is newer than its log: approving, denying or interrupting writes
  a line to the log, which clears the state. Markers older than 1 day are
  deleted on refresh.
- **Known limit.** After approving a long-running tool, the state stays
  "Needs permission" until the tool finishes and its result is written to the
  log (approval itself writes nothing; unverified).
- **State.** `SessionState::NeedsInput` / `AgentState::NeedsInput`, sorted
  first and counted by `model::waiting`, so the existing coral dot lights up.
  One notification per entry into the state, behind the existing toggle.
- **Install** is a menu checkbox. It merges into `~/.claude/settings.json`
  with `serde_json::Value`, never touching other hooks, writes a one-time
  `settings.json.picobar-bak` and replaces the file atomically (temp + rename).
  Unparseable settings are left untouched and the reason is shown. Uninstall
  removes only entries whose command ends in `--hook` and mentions
  `picobar`. Keys get reordered, because `serde_json` sorts them unless
  `preserve_order` is enabled, and that feature is adopted only if
  size-guardian measures it free (that is, `indexmap` is already in the tree).

## Settings (decided 2026-10-26)

No settings window, no new crate. Four settings live in the native tray
menu under a **Settings** submenu; the popover only reflects them.

- **Storage.** `config_dir()/settings`, one `key=value` per line, parsed with
  `std` (`split_once('=')`). Unknown keys and invalid values fall back to the
  default, so a broken file never blocks startup. Written atomically
  (temp + rename). Keys: `notify=on|off`, `warn=70|80|90`,
  `refresh=3|5|10|15` (minutes), `menubar=percent|reset|icon`. Chosen over
  `serde_json`: same size or smaller, hand-editable, no derive code.
- **Migration.** If `notify` is absent, it is `off` when the old
  `notifications-off` marker exists. The first save writes the full file and
  deletes the marker.
- **Threshold** (default 80) is one value: passed to `Alerts::check`
  (hysteresis stays threshold − 5), `icon::gauge` / `is_template` and the
  popover `View`. Menu and popover labels read "at N%".
- **Refresh** sets only the limits fetch interval. The 60 s local tick
  stays (cheap, keeps countdowns current). Minimum stays 3 min: the endpoint
  is unofficial, so we never poll faster than before. Stale cutoff becomes
  5 × interval (900 s at the default, as today).
- **Menu bar display** (macOS and Linux only; Windows trays have no title):
  `percent` (today: "42%", tokens when no limits), `reset` (session
  countdown, e.g. "1h48", else as percent), `icon` (no title).
- **Launch at login** is not stored: the checkbox shows the state on disk,
  checked only when the entry names the running executable (like
  `claude_hook::installed`). Refused from a translocated app or a mounted DMG
  ("Move PicoBar to Applications first"), shared with the hook install.
  - macOS: `~/Library/LaunchAgents/io.github.lido772.picobar.plist`,
    `RunAtLoad`, `LimitLoadToSessionType=Aqua`, path XML-escaped. Rejected:
    `SMAppService` (macOS 13+, our minimum is 11, needs objc2 as a direct
    dependency); System Events login items (Automation prompt).
  - Windows: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value
    `PicoBar` = quoted exe path, via `windows-sys` `Win32_System_Registry`
    (a feature, not a crate). The installer's startup task writes the same
    value instead of a Startup-folder shortcut and deletes the old
    `{userstartup}\PicoBar.lnk`; the app also treats that legacy shortcut as
    "on" and removes it when toggled.
  - Linux: `$XDG_CONFIG_HOME` (or `~/.config`)`/autostart/picobar.desktop`,
    `Exec` quoted per the Desktop Entry spec.
- **Known limit.** Standalone mode (no tray) has no menu, so settings there
  are changed by editing the file.

## Design reference

Tokens for the upcoming popover: graphite `#16181B`, surface `#1C1F23`, text
`#ECEDEE`, muted `#8E959C`, mint accent `#5EE6A8`, amber warning `#F5B547`,
coral needs-you `#FF7A6B`. Numbers in a monospace face.

## Updates

`update.rs` asks `api.github.com/repos/lido772/picobar/releases/latest` once a
day through `curl`. Asset names are a contract with `release.yml` (Linux:
`linux-x86_64` and, from 2026-10-26, `linux-aarch64`, picked by `target_arch`). The
download is checked against `SHA256SUMS.txt` (SHA-256 implemented in the
crate, no dependency) before anything is replaced:

- macOS: the DMG is mounted, `PicoBar.app` copied out, swapped with the
  running bundle, and the new one started with `open -n`.
- Windows and Linux: the archive is unpacked with the OS `tar`; the running
  executable is renamed to `.old` (allowed on both), the new one copied in and
  started. The `.old` file is removed on the next launch.
- A location PicoBar cannot write (Linux `.deb` in `/usr/bin`) is detected
  before downloading, and the user is sent to the release page instead.

## Portable mode

A file named `portable` next to the executable moves PicoBar's settings
folder from `~/.picobar` to `PicoBar Data` beside the executable. The Windows
portable zip ships with it.
