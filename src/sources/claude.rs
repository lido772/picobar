//! Claude limits + tokens.
//!
//! TODO:
//! - Limits: read the Claude Code OAuth token (macOS Keychain item
//!   "Claude Code-credentials", Windows/Linux `~/.claude/.credentials.json`)
//!   and query the usage endpoint Claude Code's `/usage` relies on. This is
//!   NOT a public API — verify, fail soft, never log the token.
//! - Tokens: sum `usage` fields from `~/.claude/projects/**/*.jsonl` inside
//!   the current 5-hour window (incremental tail, remember file offsets).

use crate::model::ClaudeUsage;

pub fn read() -> ClaudeUsage {
    // Stub values until the real reader lands.
    ClaudeUsage {
        session_pct: 62,
        session_reset_min: Some(108),
        weekly_pct: 41,
        weekly_opus_pct: 84,
        session_tokens: 2_810_000,
    }
}
