//! Running agents and "needs you" detection.
//!
//! TODO: PicoBar ships tiny hooks that append one JSON line per
//! event to `~/.picobar/events.jsonl`; this module tails that file.
//! - Claude Code: hooks on `Notification` (permission / waiting for input),
//!   `UserPromptSubmit` (running again), `Stop` (done), `SessionEnd`.
//! - OpenCode: a small plugin forwarding session / permission events.
//!   Verify event names against the installed versions.

use crate::model::{Agent, AgentState};

pub fn read() -> Vec<Agent> {
    vec![
        Agent {
            name: "api-refactor".into(),
            tool: "claude",
            state: AgentState::NeedsInput,
        },
        Agent {
            name: "web-app".into(),
            tool: "opencode",
            state: AgentState::Running,
        },
    ]
}
