# Size log

Updated on each release. Budget: binary ≤ 3 MB, idle memory ≤ 20 MB, idle CPU ~0 %.
Memory is the physical footprint (what Activity Monitor shows), measured after 20 s.

| Date | Version | Target | Binary | Idle memory | Idle CPU | Note |
|---|---|---|---|---|---|---|
| 2026-10-26 | skeleton | x86_64-apple-darwin | 715 KB | 6.3 MB | 0.0 % | tray + native menu, stub data |
| 2026-10-26 | 0.1.0 | x86_64-apple-darwin | 1786 KB | 11 MB | 0.0 % | real sources; bundled SQLite adds ~930 KB |
| 2026-10-26 | 0.2.0-dev | x86_64-apple-darwin | 2062 KB | 16 MB | 0.0 % | popover (softbuffer + ab_glyph), 35-day history |
| 2026-10-26 | 0.2.0 | x86_64-unknown-linux-gnu | 2434 KB | 18.5 MB private | 0.0 % | GTK 3 + AppIndicator |
| 2026-10-26 | 0.2.0+hook (uncommitted) | x86_64-apple-darwin | 2119 KB | 14 MB | 0.0 % | claude permission-prompt hook; Cargo.toml/Cargo.lock unchanged, 8 direct deps |
| 2026-10-26 | 0.2.0+settings (uncommitted) | x86_64-apple-darwin | 2131 KB | 14 MB | 0.0 % | settings + autostart; Cargo.lock unchanged, no new crates, 8 direct deps; windows-sys gets Win32_System_Registry (bindings only, Windows not built here) |
| 2026-10-27 | 0.2.0+light-theme (uncommitted) | x86_64-apple-darwin | 2135 KB | 13 MB | 0.0 % | popover light theme; Cargo.toml/Cargo.lock unchanged, 8 direct deps, no new crates; footprint sampled at 30 s |
