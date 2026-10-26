//! OpenCode tokens, cost, history and sessions from its SQLite database
//! (`~/.local/share/opencode/opencode.db`), opened read-only.

use std::path::PathBuf;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use crate::util;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OcState {
    Running,
    /// Finished its turn successfully, waiting for you.
    YourTurn,
    /// Stopped on an error or was interrupted.
    Stopped,
    Idle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcSession {
    pub name: String,
    pub model: String,
    pub tokens: u64,
    pub state: OcState,
    /// Seconds since the last update.
    pub since: i64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct OpenCodeStats {
    /// Tokens over the last 24 hours (input + output + reasoning + cache).
    pub tokens: u64,
    /// Estimated cost over the last 24 hours, in USD.
    pub cost: f64,
    /// Sessions updated in the last 2 minutes and not idle.
    pub active: usize,
    /// (local day, tokens) for the last 35 days, days with usage only.
    pub daily: Vec<(i64, u64)>,
    /// (provider, tokens) over the last 24 hours, largest first.
    pub providers: Vec<(String, u64)>,
    /// Sessions with activity in the last hour.
    pub sessions: Vec<OcSession>,
}

const TOKENS: &str = "(IFNULL(json_extract(data, '$.tokens.input'), 0)
    + IFNULL(json_extract(data, '$.tokens.output'), 0)
    + IFNULL(json_extract(data, '$.tokens.reasoning'), 0)
    + IFNULL(json_extract(data, '$.tokens.cache.read'), 0)
    + IFNULL(json_extract(data, '$.tokens.cache.write'), 0))";

pub fn read() -> Option<OpenCodeStats> {
    let path = db_path()?;
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    conn.busy_timeout(Duration::from_millis(500)).ok()?;
    let now = util::now();
    query(&conn, now, util::utc_offset(now))
}

fn query(conn: &Connection, now: i64, offset: i64) -> Option<OpenCodeStats> {
    let day_ago = (now - 86_400) * 1000;
    let (tokens, cost): (i64, f64) = conn
        .query_row(
            &format!(
                "SELECT CAST(IFNULL(SUM({TOKENS}), 0) AS INTEGER),
                        CAST(IFNULL(SUM(IFNULL(json_extract(data, '$.cost'), 0)), 0) AS REAL)
                 FROM session_message WHERE type = 'assistant' AND time_created >= ?1"
            ),
            [day_ago],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok()?;

    let active: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM session_v2
             WHERE time_archived IS NULL AND time_updated >= ?1
               AND (time_idle IS NULL OR time_idle < time_updated)",
            [(now - 120) * 1000],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let daily = rows(
        conn,
        &format!(
            "SELECT (time_created / 1000 + ?2) / 86400 AS d, CAST(SUM({TOKENS}) AS INTEGER)
             FROM session_message WHERE type = 'assistant' AND time_created >= ?1
             GROUP BY d ORDER BY d"
        ),
        rusqlite::params![(now - 35 * 86_400) * 1000, offset],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?.max(0) as u64)),
    );

    let providers = rows(
        conn,
        &format!(
            "SELECT IFNULL(json_extract(data, '$.model.providerID'), 'other') AS p,
                    CAST(SUM({TOKENS}) AS INTEGER) AS t
             FROM session_message WHERE type = 'assistant' AND time_created >= ?1
             GROUP BY p ORDER BY t DESC LIMIT 4"
        ),
        rusqlite::params![day_ago],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?.max(0) as u64)),
    );

    let sessions = rows(
        conn,
        "SELECT IFNULL(NULLIF(title, ''), slug), directory,
                IFNULL(json_extract(model, '$.id'), ''),
                tokens_input + tokens_output + tokens_reasoning + tokens_cache_read + tokens_cache_write,
                time_updated, time_idle, idle_outcome
         FROM session_v2
         WHERE time_archived IS NULL AND parent_id IS NULL AND time_updated >= ?1
         ORDER BY time_updated DESC LIMIT 8",
        rusqlite::params![(now - 3600) * 1000],
        |r| {
            let title: String = r.get(0)?;
            let dir: String = r.get(1)?;
            let updated: i64 = r.get(4)?;
            let idle: Option<i64> = r.get(5)?;
            let outcome: Option<String> = r.get(6)?;
            let since = now - updated / 1000;
            let state = match (idle, outcome.as_deref()) {
                (Some(i), _) if i < updated && since <= 120 => OcState::Running,
                (None, _) if since <= 120 => OcState::Running,
                (Some(_), Some("succeeded")) => OcState::YourTurn,
                (Some(_), Some("failed" | "interrupted" | "error")) => OcState::Stopped,
                _ => OcState::Idle,
            };
            let project = dir.rsplit(['/', '\\']).find(|s| !s.is_empty()).unwrap_or("");
            Ok(OcSession {
                name: if project.is_empty() { title } else { project.to_owned() },
                model: r.get(2)?,
                tokens: r.get::<_, i64>(3)?.max(0) as u64,
                state,
                since,
            })
        },
    );

    Some(OpenCodeStats {
        tokens: tokens.max(0) as u64,
        cost,
        active: active.max(0) as usize,
        daily,
        providers,
        sessions,
    })
}

fn rows<T, P: rusqlite::Params>(
    conn: &Connection,
    sql: &str,
    params: P,
    f: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Vec<T> {
    let Ok(mut stmt) = conn.prepare(sql) else {
        return Vec::new();
    };
    stmt.query_map(params, f)
        .map(|it| it.flatten().collect())
        .unwrap_or_default()
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
    fn sums_history_providers_and_sessions() {
        let now = 1_792_920_000_i64;
        let ms = |s: i64| (now - s) * 1000;
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE session_message (type TEXT, time_created INTEGER, data TEXT);
             CREATE TABLE session_v2 (title TEXT, slug TEXT, directory TEXT, model TEXT,
               tokens_input INTEGER, tokens_output INTEGER, tokens_reasoning INTEGER,
               tokens_cache_read INTEGER, tokens_cache_write INTEGER,
               time_updated INTEGER, time_idle INTEGER, idle_outcome TEXT,
               time_archived INTEGER, parent_id TEXT);",
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
            r#"{"cost":0.5,"model":{"providerID":"opencode"},"tokens":{"input":10,"output":5,"reasoning":1,"cache":{"read":100,"write":4}}}"#,
        );
        msg(
            "assistant",
            ms(3600),
            r#"{"cost":0.25,"model":{"providerID":"anthropic"},"tokens":{"input":1,"output":1}}"#,
        );
        msg(
            "assistant",
            ms(90_000),
            r#"{"cost":9,"tokens":{"input":1000}}"#,
        );
        msg("user", ms(10), r#"{"tokens":{"input":7}}"#);
        let sess = |dir: &str, updated: i64, idle: Option<i64>, outcome: Option<&str>| {
            c.execute(
                "INSERT INTO session_v2 VALUES ('t', 's', ?1, '{\"id\":\"glm\"}', 1, 2, 0, 3, 0, ?2, ?3, ?4, NULL, NULL)",
                rusqlite::params![dir, updated, idle, outcome],
            )
            .unwrap();
        };
        sess("/u/me/web", ms(30), None, None);
        sess("/u/me/api", ms(600), Some(ms(600) + 3), Some("succeeded"));
        sess("/u/me/old", ms(7200), None, None);

        let s = query(&c, now, 0).unwrap();
        assert_eq!(s.tokens, 122);
        assert!((s.cost - 0.75).abs() < 1e-9);
        assert_eq!(s.active, 1);
        assert_eq!(s.providers[0], ("opencode".to_string(), 120));
        assert_eq!(s.daily.iter().map(|d| d.1).sum::<u64>(), 1122);
        assert_eq!(s.sessions.len(), 2);
        assert_eq!(s.sessions[0].name, "web");
        assert_eq!(s.sessions[0].state, OcState::Running);
        assert_eq!(s.sessions[1].state, OcState::YourTurn);
        assert_eq!(s.sessions[1].tokens, 6);
    }
}
