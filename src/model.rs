//! Plain data types shared by sources and UI. No I/O here.

#[derive(Debug, Clone, Default)]
pub struct ClaudeUsage {
    /// 5-hour session window, 0–100.
    pub session_pct: u8,
    /// Minutes until the session window resets.
    pub session_reset_min: Option<u32>,
    pub weekly_pct: u8,
    pub weekly_opus_pct: u8,
    /// Tokens used in the current session window.
    pub session_tokens: u64,
}

#[derive(Debug, Clone, Default)]
pub struct OpenCodeUsage {
    pub tokens_today: u64,
    pub cost_today_cents: u64,
}

#[allow(dead_code)] // Idle/Done used once the Agents popover lands
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    Running,
    NeedsInput,
    Idle,
    Done,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // shown in the Agents popover (planned)
pub struct Agent {
    pub name: String,
    pub tool: &'static str,
    pub state: AgentState,
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub claude: ClaudeUsage,
    pub opencode: OpenCodeUsage,
    pub agents: Vec<Agent>,
}

impl Snapshot {
    pub fn waiting(&self) -> usize {
        self.agents
            .iter()
            .filter(|a| a.state == AgentState::NeedsInput)
            .count()
    }

    pub fn needs_attention(&self) -> bool {
        self.waiting() > 0
    }

    /// Text shown next to the menu bar icon (macOS only; ignored on Windows).
    pub fn title(&self) -> String {
        match self.waiting() {
            0 => format!("{}%", self.claude.session_pct),
            n => format!("{}% · {n}!", self.claude.session_pct),
        }
    }

    pub fn agents_line(&self) -> String {
        let running = self
            .agents
            .iter()
            .filter(|a| a.state == AgentState::Running)
            .count();
        format!("Agents: {} need you · {running} running", self.waiting())
    }
}

impl ClaudeUsage {
    pub fn session_line(&self) -> String {
        match self.session_reset_min {
            Some(m) => format!(
                "Claude session: {}% · resets in {}h{:02} · {} tok",
                self.session_pct,
                m / 60,
                m % 60,
                human(self.session_tokens)
            ),
            None => format!("Claude session: {}%", self.session_pct),
        }
    }

    pub fn weekly_line(&self) -> String {
        format!(
            "Claude weekly: {}% · Opus {}%",
            self.weekly_pct, self.weekly_opus_pct
        )
    }
}

impl OpenCodeUsage {
    pub fn line(&self) -> String {
        format!(
            "OpenCode today: {} tok · ${}.{:02}",
            human(self.tokens_today),
            self.cost_today_cents / 100,
            self.cost_today_cents % 100
        )
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

    #[test]
    fn human_formats() {
        assert_eq!(human(999), "999");
        assert_eq!(human(412_000), "412K");
        assert_eq!(human(1_240_000), "1.24M");
    }

    #[test]
    fn title_flags_waiting_agents() {
        let mut s = Snapshot::default();
        s.claude.session_pct = 62;
        assert_eq!(s.title(), "62%");
        s.agents.push(Agent {
            name: "x".into(),
            tool: "claude",
            state: AgentState::NeedsInput,
        });
        assert_eq!(s.title(), "62% · 1!");
    }
}
