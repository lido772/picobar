//! OpenCode tokens + cost from its local storage.
//!
//! TODO: locate OpenCode's data dir (`~/.local/share/opencode`
//! on macOS/Linux, `%USERPROFILE%\.local\share\opencode` on Windows — verify),
//! read assistant messages for today and sum tokens / cost. Verify the
//! on-disk format against the installed OpenCode version before coding.

use crate::model::OpenCodeUsage;

pub fn read() -> OpenCodeUsage {
    OpenCodeUsage {
        tokens_today: 1_240_000,
        cost_today_cents: 341,
    }
}
