//! OpenCode tokens, cost and active sessions from its SQLite database
//! (`~/.local/share/opencode/opencode.db`), opened read-only.

use std::path::PathBuf;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use crate::util;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct OpenCodeStats {
    /// Tokens over the last 24 hours (input + output + reasoning + cache).
    pub tokens: u64,
    /// Estimated cost over the last 24 hours, in USD.
    pub cost: f64,
    /// Sessions updated in the last 2 minutes and not idle.
    pub active: usize,
}

const USAGE_SQL: &str = "
SELECT
  CAST(IFNULL(SUM(
      IFNULL(json_extract(data, '$.tokens.input'), 0)
    + IFNULL(json_extract(data, '$.tokens.output'), 0)
    + IFNULL(json_extract(data, '$.tokens.reasoning'), 0)
    + IFNULL(json_extract(data, '$.tokens.cache.read'), 0)
    + IFNULL(json_extract(data, '$.tokens.cache.write'), 0)), 0) AS INTEGER),
  CAST(IFNULL(SUM(IFNULL(json_extract(data, '$.cost'), 0)), 0) AS REAL)
FROM session_message
WHERE type = 'assistant' AND time_created >= ?1";

const ACTIVE_SQL: &str = "
SELECT COUNT(*) FROM session_v2
WHERE time_archived IS NULL
  AND time_updated >= ?1
  AND (time_idle IS NULL OR time_idle < time_updated)";

pub fn read() -> Option<OpenCodeStats> {
    let path = db_path()?;
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    conn.busy_timeout(Duration::from_millis(500)).ok()?;
    query(&conn, util::now())
}

fn query(conn: &Connection, now: i64) -> Option<OpenCodeStats> {
    let (tokens, cost): (i64, f64) = conn
        .query_row(USAGE_SQL, [(now - 86_400) * 1000], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .ok()?;
    let active: i64 = conn
        .query_row(ACTIVE_SQL, [(now - 120) * 1000], |r| r.get(0))
        .unwrap_or(0);
    Some(OpenCodeStats {
        tokens: tokens.max(0) as u64,
        cost,
        active: active.max(0) as usize,
    })
}

fn db_path() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        candidates.push(PathBuf::from(xdg).join("opencode"));
    }
    if let Some(home) = util::home() {
        candidates.push(home.join(".local").join("share").join("opencode"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local).join("opencode"));
    }
    candidates
        .into_iter()
        .map(|d| d.join("opencode.db"))
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_last_24h_and_counts_active() {
        let now = 1_792_920_000_i64;
        let ms = |s: i64| (now - s) * 1000;
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE session_message (type TEXT, time_created INTEGER, data TEXT);
             CREATE TABLE session_v2 (time_archived INTEGER, time_updated INTEGER, time_idle INTEGER);",
        )
        .unwrap();
        let msg = |t: &str, at: i64, data: &str| {
            c.execute(
                "INSERT INTO session_message VALUES (?1, ?2, ?3)",
                rusqlite::params![t, at, data],
            )
            .unwrap();
        };
        msg(
            "assistant",
            ms(60),
            r#"{"cost":0.5,"tokens":{"input":10,"output":5,"reasoning":1,"cache":{"read":100,"write":4}}}"#,
        );
        msg(
            "assistant",
            ms(3600),
            r#"{"cost":0.25,"tokens":{"input":1,"output":1}}"#,
        );
        msg(
            "assistant",
            ms(90_000),
            r#"{"cost":9,"tokens":{"input":1000}}"#,
        ); // > 24h
        msg("user", ms(10), r#"{"tokens":{"input":7}}"#);
        c.execute_batch(&format!(
            "INSERT INTO session_v2 VALUES (NULL, {a}, NULL), (NULL, {a}, {idle}), (NULL, {old}, NULL);",
            a = ms(30),
            idle = ms(30) + 3,
            old = ms(3600)
        ))
        .unwrap();

        let s = query(&c, now).unwrap();
        assert_eq!(s.tokens, 122);
        assert!((s.cost - 0.75).abs() < 1e-9);
        assert_eq!(s.active, 1);
    }
}
