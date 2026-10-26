//! Fictional data for screenshots (`picobar --render <dir> --demo`), so the
//! README never shows anyone's real projects or messages.

use crate::model::{AgentRow, AgentState, DayBar, SESSION_SECS, Snapshot, Trends};
use crate::sources::claude_limits::{Bucket, Limits};
use crate::sources::claude_logs::{Active, Tokens};
use crate::sources::opencode::OpenCodeStats;
use crate::util;

pub fn data(range: usize) -> (Snapshot, Vec<AgentRow>, Trends) {
    let now = util::now();
    let reset = now + 6480;
    let today = util::local_day(now);
    let snap = Snapshot {
        now,
        limits: Some(Limits {
            session: Some(Bucket {
                pct: 62.0,
                resets_at: Some(reset),
            }),
            weekly: Some(Bucket {
                pct: 41.0,
                resets_at: Some(now + 3 * 86_400 + 5 * 3600),
            }),
            weekly_opus: Some(Bucket {
                pct: 84.0,
                resets_at: None,
            }),
            weekly_sonnet: None,
            plan: Some("max".into()),
        }),
        limits_error: None,
        tokens: Tokens {
            input: 412_000,
            output: 96_000,
            cache_write: 310_000,
            cache_read: 1_992_000,
        },
        window_start: Some(reset - SESSION_SECS),
        claude_active: Active {
            sessions: 2,
            subagents: 3,
        },
        opencode: Some(OpenCodeStats {
            tokens: 1_240_000,
            cost: 3.41,
            active: 1,
            daily: [
                310_000u64, 520_000, 240_000, 880_000, 610_000, 990_000, 1_240_000,
            ]
            .iter()
            .enumerate()
            .map(|(i, t)| (today - 6 + i as i64, *t))
            .collect(),
            providers: vec![
                ("anthropic".into(), 720_000),
                ("openai".into(), 335_000),
                ("opencode".into(), 185_000),
            ],
            sessions: Vec::new(),
        }),
        waiting: 3,
    };

    let row =
        |tool, name: &str, model: &str, state, since, tokens, subagents, preview: &str| AgentRow {
            id: String::new(),
            tool,
            name: name.into(),
            model: model.into(),
            state,
            since,
            tokens,
            subagents,
            preview: preview.into(),
        };
    let agents = vec![
        row(
            "Claude Code",
            "infra-scripts",
            "Sonnet 4.5",
            AgentState::NeedsInput,
            45,
            96_000,
            0,
            "",
        ),
        row(
            "Claude Code",
            "api-refactor",
            "Opus 4.1",
            AgentState::YourTurn,
            240,
            1_210_000,
            0,
            "All 214 tests pass. Want me to open the pull request?",
        ),
        row(
            "OpenCode",
            "web-app",
            "glm-4.6",
            AgentState::YourTurn,
            720,
            380_000,
            0,
            "",
        ),
        row(
            "Claude Code",
            "mobile-game",
            "Sonnet 4.5",
            AgentState::Running,
            20,
            412_000,
            3,
            "",
        ),
        row(
            "Claude Code",
            "etl-jobs",
            "Opus 4.1",
            AgentState::Running,
            60,
            96_000,
            0,
            "",
        ),
        row(
            "OpenCode",
            "landing",
            "gpt-5",
            AgentState::Idle,
            1500,
            31_000,
            0,
            "",
        ),
    ];

    let claude = [4.8, 2.0, 1.2, 5.8, 6.6, 5.4, 7.2];
    let opencode = [1.4, 0.8, 0.4, 2.2, 1.8, 2.6, 1.6];
    let days: Vec<DayBar> = (0..range)
        .map(|i| {
            let d = today - (range - 1 - i) as i64;
            let k = i % 7;
            DayBar {
                label: if range <= 7 {
                    util::WEEKDAYS[util::weekday(d)][..1].to_owned()
                } else {
                    String::new()
                },
                claude: (claude[k] * 1e6) as u64,
                opencode: (opencode[k] * 1e6) as u64,
                today: i == range - 1,
            }
        })
        .collect();
    let shape = [0u64, 0, 0, 1, 3, 4, 4, 3, 2, 2, 1, 0];
    let mut heat = [[0u64; 12]; 7];
    for (d, row) in heat.iter_mut().enumerate() {
        for (h, v) in row.iter_mut().enumerate() {
            let base = if d >= 5 {
                shape[h].saturating_sub(2)
            } else {
                shape[h]
            };
            *v = base * 1_000_000 + ((d * 7 + h * 3) % 5) as u64 * 150_000 * base.min(1);
        }
    }
    let trends = Trends {
        range,
        claude_total: days.iter().map(|d| d.claude).sum(),
        opencode_total: days.iter().map(|d| d.opencode).sum(),
        days,
        change: (range <= 7).then_some(23.0),
        heat,
        peak: Some("10:00–12:00".into()),
        insights: vec![
            "Opus 4.1 handled 61% of your Claude tokens".into(),
            "Cache reads were 82% of Claude tokens".into(),
        ],
    };
    let mut trends = trends;
    if let Some((i, b)) = trends
        .days
        .iter()
        .enumerate()
        .max_by_key(|(_, b)| b.claude + b.opencode)
    {
        let day = today - (range - 1 - i) as i64;
        trends.insights.push(format!(
            "Busiest day: {} {} · {} tokens",
            util::WEEKDAYS[util::weekday(day)],
            util::civil(day).2,
            crate::model::human(b.claude + b.opencode)
        ));
    }
    (snap, agents, trends)
}
