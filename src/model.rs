//! What PicoBar shows, computed from the sources. Pure and unit-tested.

use std::collections::HashMap;

use crate::sources::claude_limits::{Bucket, Limits, LimitsError};
use crate::sources::claude_logs::{self, Active, ClaudeLogs, SessionState, Tokens};
use crate::sources::opencode::{OcState, OpenCodeStats};
use crate::util;

pub const SESSION_SECS: i64 = 5 * 3600;

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub now: i64,
    pub limits: Option<Limits>,
    pub limits_error: Option<LimitsError>,
    /// Tokens in the current session window.
    pub tokens: Tokens,
    /// Start of the session window when known from Claude's reset time.
    pub window_start: Option<i64>,
    pub claude_active: Active,
    pub opencode: Option<OpenCodeStats>,
    /// Agents waiting for you (Claude Code + OpenCode).
    pub waiting: usize,
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
        let parts = self.model_buckets();
        if parts.is_empty() {
            "Weekly by model  –".into()
        } else {
            let p: Vec<String> = parts
                .iter()
                .map(|(n, b)| format!("{n} {:.0}%", b.pct))
                .collect();
            format!("Weekly by model  {}", p.join(" · "))
        }
    }

    pub fn model_buckets(&self) -> Vec<(&'static str, Bucket)> {
        let l = self.limits.as_ref();
        [
            ("Opus", l.and_then(|l| l.weekly_opus)),
            ("Sonnet", l.and_then(|l| l.weekly_sonnet)),
        ]
        .into_iter()
        .filter_map(|(n, b)| Some((n, b?)))
        .collect()
    }

    pub fn tokens_line(&self) -> String {
        let t = &self.tokens;
        format!(
            "Tokens ({})  {} · in {} · out {} · cache {}",
            if self.window_start.is_some() {
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
        match &self.opencode {
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
        let opencode = self.opencode.as_ref().map_or(0, |o| o.active);
        format!(
            "Active  Claude Code {}{subagents} · OpenCode {opencode}",
            a.sessions
        )
    }

    pub fn status(&self) -> &'static str {
        match (self.limits.is_some(), self.limits_error) {
            (true, _) => "Live",
            (false, Some(e)) => match e {
                LimitsError::Expired => "Login expired",
                LimitsError::NoLogin => "Not signed in",
                LimitsError::Offline => "Offline",
                LimitsError::Unexpected => "Unavailable",
            },
            (false, None) => "Loading",
        }
    }

    /// Burn rate and projection for the current session window.
    pub fn burn(&self) -> Option<Burn> {
        let start = self.window_start?;
        let b = self.session()?;
        let reset = b.resets_at?;
        let elapsed = (self.now - start).max(1);
        let per_hour = self.tokens.total() * 3600 / elapsed.max(900) as u64;
        let rate = b.pct / elapsed as f32; // % per second
        let at_reset = b.pct + rate * (reset - self.now).max(0) as f32;
        let limit_at = (elapsed >= 600 && at_reset >= 100.0 && rate > 0.0)
            .then(|| self.now + ((100.0 - b.pct) / rate) as i64);
        Some(Burn {
            per_hour,
            pct_now: b.pct,
            pct_at_reset: at_reset,
            limit_at,
            reset_at: reset,
        })
    }

    fn resets(&self, b: Bucket) -> String {
        b.resets_at
            .map(|r| format!(" · resets in {}", util::countdown(r - self.now)))
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Burn {
    pub per_hour: u64,
    pub pct_now: f32,
    pub pct_at_reset: f32,
    /// When the limit would be reached at this pace, if before the reset.
    pub limit_at: Option<i64>,
    pub reset_at: i64,
}

fn explain(e: LimitsError) -> &'static str {
    match e {
        LimitsError::NoLogin => "sign in to Claude Code",
        LimitsError::Expired => "open Claude Code to refresh your login",
        LimitsError::Offline => "offline",
        LimitsError::Unexpected => "limits unavailable",
    }
}

// ---------------------------------------------------------------- agents

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AgentState {
    NeedsInput,
    YourTurn,
    Stopped,
    Running,
    Idle,
}

#[derive(Debug, Clone)]
pub struct AgentRow {
    /// Claude session id; empty for OpenCode.
    pub id: String,
    pub tool: &'static str,
    pub name: String,
    pub model: String,
    pub state: AgentState,
    pub since: i64,
    pub tokens: u64,
    pub subagents: usize,
    pub preview: String,
}

pub fn agents(logs: &ClaudeLogs, oc: Option<&OpenCodeStats>, now: i64) -> Vec<AgentRow> {
    let mut rows: Vec<AgentRow> = logs
        .sessions(now)
        .into_iter()
        .map(|s| AgentRow {
            id: s.id,
            tool: "Claude Code",
            name: s.name,
            model: s.model,
            state: match s.state {
                SessionState::NeedsInput => AgentState::NeedsInput,
                SessionState::YourTurn => AgentState::YourTurn,
                SessionState::Running => AgentState::Running,
                SessionState::Idle => AgentState::Idle,
            },
            since: s.since,
            tokens: s.tokens_5h,
            subagents: s.subagents_running,
            preview: s.preview,
        })
        .collect();
    for s in oc.map(|o| o.sessions.as_slice()).unwrap_or_default() {
        rows.push(AgentRow {
            id: String::new(),
            tool: "OpenCode",
            name: s.name.clone(),
            model: s.model.clone(),
            state: match s.state {
                OcState::YourTurn => AgentState::YourTurn,
                OcState::Stopped => AgentState::Stopped,
                OcState::Running => AgentState::Running,
                OcState::Idle => AgentState::Idle,
            },
            since: s.since,
            tokens: s.tokens,
            subagents: 0,
            preview: String::new(),
        });
    }
    rows.sort_by_key(|r| (r.state, r.since));
    rows
}

pub fn waiting(rows: &[AgentRow]) -> usize {
    rows.iter()
        .filter(|r| {
            matches!(
                r.state,
                AgentState::NeedsInput | AgentState::YourTurn | AgentState::Stopped
            )
        })
        .count()
}

// ---------------------------------------------------------------- trends

#[derive(Debug, Clone, Default)]
pub struct DayBar {
    pub label: String,
    pub claude: u64,
    pub opencode: u64,
    pub today: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Trends {
    pub range: usize,
    pub days: Vec<DayBar>,
    pub claude_total: u64,
    pub opencode_total: u64,
    /// Change vs the previous period of the same length (7-day range only).
    pub change: Option<f32>,
    /// [weekday Mon..Sun][2-hour block], Claude tokens over the last 28 days.
    pub heat: [[u64; 12]; 7],
    pub peak: Option<String>,
    pub insights: Vec<String>,
}

impl Trends {
    pub fn total(&self) -> u64 {
        self.claude_total + self.opencode_total
    }
}

pub fn trends(logs: &ClaudeLogs, oc: Option<&OpenCodeStats>, now: i64, range: usize) -> Trends {
    let off = util::utc_offset(now);
    let day_of = |ts: i64| (ts + off).div_euclid(86_400);
    let today = day_of(now);
    let first = today - range as i64 + 1;

    let mut claude: HashMap<i64, u64> = HashMap::new();
    let mut heat = [[0u64; 12]; 7];
    let mut by_model: HashMap<String, u64> = HashMap::new();
    let mut range_tokens = Tokens::default();
    for e in logs.entries() {
        let d = day_of(e.ts);
        let total = e.tokens.total();
        *claude.entry(d).or_default() += total;
        if d > today - 28 {
            let block = ((e.ts + off).rem_euclid(86_400) / 7200) as usize;
            heat[util::weekday(d)][block] += total;
        }
        if d >= first {
            *by_model
                .entry(claude_logs::short_model(logs.model_name(e.model)))
                .or_default() += total;
            range_tokens.input += e.tokens.input;
            range_tokens.output += e.tokens.output;
            range_tokens.cache_write += e.tokens.cache_write;
            range_tokens.cache_read += e.tokens.cache_read;
        }
    }
    let opencode: HashMap<i64, u64> = oc
        .map(|o| o.daily.iter().copied().collect())
        .unwrap_or_default();

    let days: Vec<DayBar> = (first..=today)
        .map(|d| DayBar {
            label: if range <= 7 {
                util::WEEKDAYS[util::weekday(d)][..1].to_owned()
            } else {
                String::new()
            },
            claude: claude.get(&d).copied().unwrap_or(0),
            opencode: opencode.get(&d).copied().unwrap_or(0),
            today: d == today,
        })
        .collect();
    let claude_total: u64 = days.iter().map(|d| d.claude).sum();
    let opencode_total: u64 = days.iter().map(|d| d.opencode).sum();

    let change = (range <= 7)
        .then(|| {
            let prev: u64 = (first - range as i64..first)
                .map(|d| {
                    claude.get(&d).copied().unwrap_or(0) + opencode.get(&d).copied().unwrap_or(0)
                })
                .sum();
            (prev > 0).then(|| ((claude_total + opencode_total) as f32 / prev as f32 - 1.0) * 100.0)
        })
        .flatten();

    let peak = {
        let mut blocks = [0u64; 12];
        for row in &heat {
            for (i, v) in row.iter().enumerate() {
                blocks[i] += v;
            }
        }
        let (i, max) = blocks
            .iter()
            .enumerate()
            .max_by_key(|(_, v)| **v)
            .unwrap_or((0, &0));
        (*max > 0).then(|| format!("{:02}:00–{:02}:00", i * 2, (i * 2 + 2) % 24))
    };

    let mut insights = Vec::new();
    let model_total: u64 = by_model.values().sum();
    if let Some((name, v)) = by_model
        .iter()
        .filter(|(n, _)| !n.starts_with('<'))
        .max_by_key(|(_, v)| **v)
        && model_total > 0
    {
        insights.push(format!(
            "{name} handled {:.0}% of your Claude tokens",
            *v as f32 * 100.0 / model_total as f32
        ));
    }
    if range_tokens.total() > 0 {
        insights.push(format!(
            "Cache reads were {:.0}% of Claude tokens",
            range_tokens.cache_read as f32 * 100.0 / range_tokens.total() as f32
        ));
    }
    if let Some((i, d)) = days
        .iter()
        .enumerate()
        .max_by_key(|(_, d)| d.claude + d.opencode)
        && d.claude + d.opencode > 0
    {
        let day = first + i as i64;
        let (_, _, dom) = util::civil(day);
        insights.push(format!(
            "Busiest day: {} {} · {} tokens",
            util::WEEKDAYS[util::weekday(day)],
            dom,
            human(d.claude + d.opencode)
        ));
    }

    Trends {
        range,
        days,
        claude_total,
        opencode_total,
        change,
        heat,
        peak,
        insights,
    }
}

/// 1_240_000 -> "1.24M", 412_000 -> "412K".
pub fn human(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{}K", n / 1_000),
        1_000_000..=999_999_999 => format!("{:.2}M", n as f64 / 1e6),
        _ => format!("{:.2}B", n as f64 / 1e9),
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
                plan: None,
            }),
            tokens: Tokens {
                input: 412_000,
                output: 96_000,
                cache_write: 300_000,
                cache_read: 2_002_000,
            },
            window_start: Some(1_000_000 + 6480 - SESSION_SECS),
            ..Default::default()
        }
    }

    #[test]
    fn human_formats() {
        assert_eq!(human(999), "999");
        assert_eq!(human(412_000), "412K");
        assert_eq!(human(1_240_000), "1.24M");
        assert_eq!(human(2_500_000_000), "2.50B");
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
        assert_eq!(s.status(), "Live");
    }

    #[test]
    fn burn_projects_limit_before_reset() {
        let b = snap().burn().unwrap();
        // 62.4% in 3h12 -> 100% about 1h56 later, before the reset in 1h48? No:
        // rate = 62.4 / 11520 s; at reset (+6480 s) = 97.5% -> stays under.
        assert!(b.pct_at_reset < 100.0);
        assert_eq!(b.limit_at, None);
        assert_eq!(b.per_hour, 2_810_000 * 3600 / 11_520);
    }

    #[test]
    fn lines_without_limits() {
        let mut s = snap();
        s.limits = None;
        s.limits_error = Some(LimitsError::Expired);
        s.window_start = None;
        assert_eq!(s.title(), "2.81M");
        assert_eq!(
            s.session_line(),
            "Session  –  (open Claude Code to refresh your login)"
        );
        assert!(s.tokens_line().starts_with("Tokens (last 5h)"));
        assert_eq!(s.opencode_line(), "OpenCode  not found");
        assert_eq!(s.status(), "Login expired");
        assert!(s.burn().is_none());
    }
}
