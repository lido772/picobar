//! What the menu shows, computed from the sources. Pure and unit-tested.

use crate::sources::claude_limits::{Bucket, Limits, LimitsError};
use crate::sources::claude_logs::{Active, Tokens};
use crate::sources::opencode::OpenCodeStats;
use crate::util;

pub const SESSION_SECS: i64 = 5 * 3600;

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub now: i64,
    pub limits: Option<Limits>,
    pub limits_error: Option<LimitsError>,
    /// Tokens in the current session window.
    pub tokens: Tokens,
    /// True when the window comes from Claude's reset time, false for a rolling 5h.
    pub window_known: bool,
    pub claude_active: Active,
    pub opencode: Option<OpenCodeStats>,
}

impl Snapshot {
    pub fn session(&self) -> Option<Bucket> {
        self.limits.as_ref()?.session
    }

    pub fn session_pct(&self) -> Option<u8> {
        self.session()
            .map(|b| b.pct.round().clamp(0.0, 100.0) as u8)
    }

    /// Text next to the menu bar icon (macOS).
    pub fn title(&self) -> String {
        match self.session_pct() {
            Some(p) => format!("{p}%"),
            None => human(self.tokens.total()),
        }
    }

    pub fn tooltip(&self) -> String {
        format!("PicoBar · {}", self.session_line())
    }

    pub fn session_line(&self) -> String {
        match (self.session(), self.limits_error) {
            (Some(b), _) => format!("Session  {:.0}%{}", b.pct, self.resets(b)),
            (None, Some(e)) => format!("Session  –  ({})", explain(e)),
            (None, None) => "Session  –".into(),
        }
    }

    pub fn weekly_line(&self) -> String {
        match self.limits.as_ref().and_then(|l| l.weekly) {
            Some(b) => format!("Weekly  {:.0}%{}", b.pct, self.resets(b)),
            None => "Weekly  –".into(),
        }
    }

    pub fn models_line(&self) -> String {
        let l = self.limits.as_ref();
        let part = |name: &str, b: Option<Bucket>| b.map(|b| format!("{name} {:.0}%", b.pct));
        let parts: Vec<String> = [
            part("Opus", l.and_then(|l| l.weekly_opus)),
            part("Sonnet", l.and_then(|l| l.weekly_sonnet)),
        ]
        .into_iter()
        .flatten()
        .collect();
        if parts.is_empty() {
            "Weekly by model  –".into()
        } else {
            format!("Weekly by model  {}", parts.join(" · "))
        }
    }

    pub fn tokens_line(&self) -> String {
        let t = &self.tokens;
        format!(
            "Tokens ({})  {} · in {} · out {} · cache {}",
            if self.window_known {
                "this session"
            } else {
                "last 5h"
            },
            human(t.total()),
            human(t.input),
            human(t.output),
            human(t.cache())
        )
    }

    pub fn opencode_line(&self) -> String {
        match self.opencode {
            Some(o) => format!(
                "OpenCode (24h)  {} tokens · ${:.2}",
                human(o.tokens),
                o.cost
            ),
            None => "OpenCode  not found".into(),
        }
    }

    pub fn active_line(&self) -> String {
        let a = self.claude_active;
        let subagents = match a.subagents {
            0 => String::new(),
            n => format!(" + {n} subagents"),
        };
        let opencode = self.opencode.map_or(0, |o| o.active);
        format!(
            "Active  Claude Code {}{subagents} · OpenCode {opencode}",
            a.sessions
        )
    }

    fn resets(&self, b: Bucket) -> String {
        b.resets_at
            .map(|r| format!(" · resets in {}", util::countdown(r - self.now)))
            .unwrap_or_default()
    }
}

fn explain(e: LimitsError) -> &'static str {
    match e {
        LimitsError::NoLogin => "sign in to Claude Code",
        LimitsError::Expired => "open Claude Code to refresh your login",
        LimitsError::Offline => "offline",
        LimitsError::Unexpected => "limits unavailable",
    }
}

/// 1_240_000 -> "1.24M", 412_000 -> "412K".
pub fn human(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{}K", n / 1_000),
        _ => format!("{:.2}M", n as f64 / 1_000_000.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> Snapshot {
        Snapshot {
            now: 1_000_000,
            limits: Some(Limits {
                session: Some(Bucket {
                    pct: 62.4,
                    resets_at: Some(1_000_000 + 6480),
                }),
                weekly: Some(Bucket {
                    pct: 41.0,
                    resets_at: Some(1_000_000 + 280_000),
                }),
                weekly_opus: Some(Bucket {
                    pct: 84.0,
                    resets_at: None,
                }),
                weekly_sonnet: None,
            }),
            tokens: Tokens {
                input: 412_000,
                output: 96_000,
                cache_write: 300_000,
                cache_read: 2_002_000,
            },
            window_known: true,
            ..Default::default()
        }
    }

    #[test]
    fn human_formats() {
        assert_eq!(human(999), "999");
        assert_eq!(human(412_000), "412K");
        assert_eq!(human(1_240_000), "1.24M");
    }

    #[test]
    fn lines_with_limits() {
        let s = snap();
        assert_eq!(s.title(), "62%");
        assert_eq!(s.session_line(), "Session  62% · resets in 1h48");
        assert_eq!(s.weekly_line(), "Weekly  41% · resets in 3d 5h");
        assert_eq!(s.models_line(), "Weekly by model  Opus 84%");
        assert_eq!(
            s.tokens_line(),
            "Tokens (this session)  2.81M · in 412K · out 96K · cache 2.30M"
        );
    }

    #[test]
    fn lines_without_limits() {
        let mut s = snap();
        s.limits = None;
        s.limits_error = Some(LimitsError::Expired);
        s.window_known = false;
        assert_eq!(s.title(), "2.81M");
        assert_eq!(
            s.session_line(),
            "Session  –  (open Claude Code to refresh your login)"
        );
        assert!(s.tokens_line().starts_with("Tokens (last 5h)"));
        assert_eq!(s.opencode_line(), "OpenCode  not found");
    }
}
