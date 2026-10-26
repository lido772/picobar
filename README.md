# PicoBar

**Ultra-light menu bar (macOS) and tray (Windows) monitor for Claude and OpenCode.**

Native Rust, no webview, no runtime. Under 1 MB today, hard budget of 3 MB.

- Claude 5-hour session and weekly limits, with countdowns to reset
- Notifications when limits reset or cross your threshold
- Live token counter and burn-rate projection
- Trends: daily tokens, week-over-week change, when you burn the most
- Agent monitoring for Claude Code and OpenCode, with a badge when an agent
  needs your input

> Status: early skeleton. The menu bar item and menu work with stub data; data
> sources are being implemented. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Build

```sh
cargo build --release
./target/release/picobar
```

Requires stable Rust. macOS 12+ (Intel or Apple Silicon) and Windows 10+.

## Lightness budget

| Metric | Budget |
|---|---|
| Binary | ≤ 3 MB |
| Idle memory | ≤ 20 MB |
| Idle CPU | ~0 % |

Checked in CI by `scripts/size.sh`. History in [docs/SIZE.md](docs/SIZE.md).

## How it gets data

- **Claude** — your existing Claude Code login and local session logs. The
  limits data is not a public API and may change; PicoBar degrades gracefully.
- **OpenCode** — its local storage.
- **Agents** — small opt-in hooks that write events to `~/.picobar/events.jsonl`.

Credentials never leave your machine and are never logged.


## License

MIT
