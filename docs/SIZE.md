# Size log

Updated on each release. Budget: binary ≤ 3 MB, idle memory ≤ 20 MB, idle CPU ~0 %.
Memory is the physical footprint (what Activity Monitor shows), measured after 20 s.

| Date | Version | Target | Binary | Idle memory | Idle CPU | Note |
|---|---|---|---|---|---|---|
| 2026-10-26 | skeleton | x86_64-apple-darwin | 715 KB | 6.3 MB | 0.0 % | tray + native menu, stub data |
| 2026-10-26 | 0.1.0 | x86_64-apple-darwin | 1786 KB | 11 MB | 0.0 % | real sources; bundled SQLite adds ~930 KB |
| 2026-10-26 | 0.2.0-dev | x86_64-apple-darwin | 2062 KB | 16 MB | 0.0 % | popover (softbuffer + ab_glyph), 35-day history |
| 2026-10-26 | 0.2.0 | x86_64-unknown-linux-gnu | 2434 KB | 18.5 MB private | 0.0 % | GTK 3 + AppIndicator |
