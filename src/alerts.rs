//! Decides when to notify: a limit resets, or usage crosses the threshold.

use crate::model::{AgentRow, AgentState};
use crate::sources::claude_limits::{Bucket, Limits};
use crate::util;

#[derive(Default)]
pub struct Alerts {
    session: Watch,
    weekly: Watch,
    /// Claude sessions currently blocked on a permission prompt.
    blocked: Vec<String>,
    /// Skip sessions already blocked at launch.
    agents_primed: bool,
}

#[derive(Default)]
struct Watch {
    /// Next known reset time.
    reset_at: Option<i64>,
    notified_reset: Option<i64>,
    /// Threshold the last warning was sent for.
    warned_at: Option<f32>,
    /// Skip the threshold warning on the very first reading after launch.
    primed: bool,
}

pub type Message = (String, String);

impl Alerts {
    /// Call on every tick with the latest known limits (possibly stale or none).
    pub fn check(&mut self, limits: Option<&Limits>, now: i64, warn: f32) -> Vec<Message> {
        let mut out = Vec::new();
        self.session.update(
            limits.and_then(|l| l.session),
            now,
            warn,
            (
                "Claude session reset",
                "You're back to 100% for a new 5-hour window.",
            ),
            "Claude session",
            &mut out,
        );
        self.weekly.update(
            limits.and_then(|l| l.weekly),
            now,
            warn,
            (
                "Claude weekly limit reset",
                "Your weekly usage is back to 0%.",
            ),
            "Claude weekly usage",
            &mut out,
        );
        out
    }
}

impl Alerts {
    /// One message per session entering the permission-prompt state.
    pub fn check_agents(&mut self, rows: &[AgentRow]) -> Vec<Message> {
        let now: Vec<&AgentRow> = rows
            .iter()
            .filter(|r| r.state == AgentState::NeedsInput)
            .collect();
        let out = now
            .iter()
            .filter(|_| self.agents_primed)
            .filter(|r| !self.blocked.contains(&r.id))
            .map(|r| {
                (
                    format!("{} needs permission", r.name),
                    "Claude Code is waiting for you to approve a tool call.".to_owned(),
                )
            })
            .collect();
        self.blocked = now.iter().map(|r| r.id.clone()).collect();
        self.agents_primed = true;
        out
    }
}

impl Watch {
    fn update(
        &mut self,
        bucket: Option<Bucket>,
        now: i64,
        warn: f32,
        reset_msg: (&str, &str),
        name: &str,
        out: &mut Vec<Message>,
    ) {
        if let Some(r) = self.reset_at
            && now >= r
            && self.notified_reset != Some(r)
        {
            self.notified_reset = Some(r);
            self.warned_at = None;
            out.push((reset_msg.0.into(), reset_msg.1.into()));
        }
        let Some(b) = bucket else { return };
        if let Some(r) = b.resets_at
            && r > now
        {
            self.reset_at = Some(r);
        }
        if b.pct >= warn {
            if self.warned_at != Some(warn) && self.primed {
                let when = b
                    .resets_at
                    .map(|r| format!("Resets in {}.", util::countdown(r - now)))
                    .unwrap_or_default();
                out.push((format!("{name} at {:.0}%", b.pct), when));
            }
            self.warned_at = Some(warn);
        } else if self.notified_reset.is_none() || b.pct < warn - 5.0 {
            self.warned_at = None;
        }
        self.primed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(pct: f32, resets_at: i64) -> Limits {
        Limits {
            session: Some(Bucket {
                pct,
                resets_at: Some(resets_at),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn notifies_once_per_permission_prompt() {
        let row = |state| AgentRow {
            id: "s1".into(),
            tool: "Claude Code",
            name: "api".into(),
            model: String::new(),
            state,
            since: 0,
            tokens: 0,
            subagents: 0,
            preview: String::new(),
        };
        let mut a = Alerts::default();
        // Already blocked at launch: recorded, not notified.
        assert!(a.check_agents(&[row(AgentState::NeedsInput)]).is_empty());
        assert!(a.check_agents(&[row(AgentState::Running)]).is_empty());
        assert_eq!(a.check_agents(&[row(AgentState::NeedsInput)]).len(), 1);
        assert!(a.check_agents(&[row(AgentState::NeedsInput)]).is_empty());
        assert!(a.check_agents(&[row(AgentState::Running)]).is_empty());
        assert_eq!(a.check_agents(&[row(AgentState::NeedsInput)]).len(), 1);
    }

    #[test]
    fn warns_once_when_crossing_threshold() {
        let mut a = Alerts::default();
        assert!(a.check(Some(&limits(50.0, 1000)), 0, 80.0).is_empty());
        let m = a.check(Some(&limits(81.0, 1000)), 100, 80.0);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].0, "Claude session at 81%");
        assert!(a.check(Some(&limits(85.0, 1000)), 200, 80.0).is_empty());
    }

    #[test]
    fn warns_again_for_a_higher_threshold() {
        let mut a = Alerts::default();
        a.check(Some(&limits(50.0, 1000)), 0, 70.0);
        assert_eq!(a.check(Some(&limits(72.0, 1000)), 1, 70.0).len(), 1);
        assert!(a.check(Some(&limits(77.0, 1000)), 2, 80.0).is_empty());
        assert_eq!(a.check(Some(&limits(81.0, 1000)), 3, 80.0).len(), 1);
    }

    #[test]
    fn no_warning_on_first_reading() {
        let mut a = Alerts::default();
        assert!(a.check(Some(&limits(90.0, 1000)), 0, 80.0).is_empty());
    }

    #[test]
    fn notifies_reset_once_even_offline() {
        let mut a = Alerts::default();
        a.check(Some(&limits(40.0, 1000)), 0, 80.0);
        let m = a.check(None, 1000, 80.0);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].0, "Claude session reset");
        assert!(a.check(None, 1060, 80.0).is_empty());
    }
}
