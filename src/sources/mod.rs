//! Data sources. Each source is a pure "read local state → model" function,
//! polled from the main loop. No async runtime, no background threads yet.

mod agents;
mod claude;
mod opencode;

use crate::model::Snapshot;

pub fn snapshot() -> Snapshot {
    Snapshot {
        claude: claude::read(),
        opencode: opencode::read(),
        agents: agents::read(),
    }
}
