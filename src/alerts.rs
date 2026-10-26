//! Decides when to notify: a limit resets, or usage crosses the threshold.

use crate::sources::claude_limits::{Bucket, Limits};
use crate::util;

pub const WARN_PCT: f32 = 80.0;

#[derive(Default)]
pub struct Alerts {
    session: Watch,
    weekly: Watch,
}

#[derive(Default)]
struct Watch {
    /// Next known reset time.
    reset_at: Option<i64>,
    notified_reset: Option<i64>,
    warned: bool,
    /// Skip the threshold warning on the very first reading after launch.
    primed: bool,
}

pub type Message = (String, String);

impl Alerts {
    /// Call on every tick with the latest known limits (possibly stale or none).
    pub fn check(&mut self, limits: Option<&Limits>, now: i64) -> Vec<Message> {
        let mut out = Vec::new();
        self.session.update(
            limits.and_then(|l| l.session),
            now,
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

impl Watch {
    fn update(
        &mut self,
        bucket: Option<Bucket>,
        now: i64,
        reset_msg: (&str, &str),
        name: &str,
        out: &mut Vec<Message>,
    ) {
        if let Some(r) = self.reset_at
            && now >= r
            && self.notified_reset != Some(r)
        {
            self.notified_reset = Some(r);
            self.warned = false;
            out.push((reset_msg.0.into(), reset_msg.1.into()));
        }
        let Some(b) = bucket else { return };
        if let Some(r) = b.resets_at
            && r > now
        {
            self.reset_at = Some(r);
        }
        if b.pct >= WARN_PCT {
            if !self.warned && self.primed {
                let when = b
                    .resets_at
                    .map(|r| format!("Resets in {}.", util::countdown(r - now)))
                    .unwrap_or_default();
                out.push((format!("{name} at {:.0}%", b.pct), when));
            }
            self.warned = true;
        } else if self.notified_reset.is_none() || b.pct < WARN_PCT - 5.0 {
            self.warned = false;
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
    fn warns_once_when_crossing_threshold() {
        let mut a = Alerts::default();
        assert!(a.check(Some(&limits(50.0, 1000)), 0).is_empty());
        let m = a.check(Some(&limits(81.0, 1000)), 100);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].0, "Claude session at 81%");
        assert!(a.check(Some(&limits(85.0, 1000)), 200).is_empty());
    }

    #[test]
    fn no_warning_on_first_reading() {
        let mut a = Alerts::default();
        assert!(a.check(Some(&limits(90.0, 1000)), 0).is_empty());
    }

    #[test]
    fn notifies_reset_once_even_offline() {
        let mut a = Alerts::default();
        a.check(Some(&limits(40.0, 1000)), 0);
        let m = a.check(None, 1000);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].0, "Claude session reset");
        assert!(a.check(None, 1060).is_empty());
    }
}
