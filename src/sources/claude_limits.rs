//! Claude plan limits (5-hour session, weekly) from the usage endpoint that
//! Claude Code's `/usage` relies on, using the existing Claude Code login.
//!
//! This is not a public API: every failure degrades to an error the menu can
//! explain, never a crash. The token is never logged, stored or put on a
//! command line (curl reads the headers from stdin).

use std::io::Write;
use std::process::Stdio;

use serde_json::Value;

use crate::util;

const URL: &str = "https://api.anthropic.com/api/oauth/usage";

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Bucket {
    /// 0–100.
    pub pct: f32,
    /// Unix seconds.
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Limits {
    pub session: Option<Bucket>,
    pub weekly: Option<Bucket>,
    pub weekly_opus: Option<Bucket>,
    pub weekly_sonnet: Option<Bucket>,
    /// Subscription from the login, e.g. "pro" or "max".
    pub plan: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitsError {
    /// No Claude Code login found on this machine.
    NoLogin,
    /// The login token expired; using Claude Code refreshes it.
    Expired,
    Offline,
    /// The endpoint answered something PicoBar does not understand.
    Unexpected,
}

pub fn fetch() -> Result<Limits, LimitsError> {
    let (token, plan) = token()?;
    let body = get(&token)?;
    let mut limits = parse(&body).ok_or(LimitsError::Unexpected)?;
    limits.plan = plan;
    Ok(limits)
}

fn token() -> Result<(String, Option<String>), LimitsError> {
    let raw = credentials().ok_or(LimitsError::NoLogin)?;
    let v: Value = serde_json::from_str(&raw).map_err(|_| LimitsError::NoLogin)?;
    let oauth = &v["claudeAiOauth"];
    let token = oauth["accessToken"].as_str().ok_or(LimitsError::NoLogin)?;
    if oauth["expiresAt"]
        .as_i64()
        .is_some_and(|ms| ms / 1000 <= util::now())
    {
        return Err(LimitsError::Expired);
    }
    let plan = oauth["subscriptionType"].as_str().map(str::to_owned);
    Ok((token.to_owned(), plan))
}

#[cfg(target_os = "macos")]
fn credentials() -> Option<String> {
    let out = util::hidden("/usr/bin/security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout)
        .ok()
        .map(|s| s.trim().to_owned())
}

#[cfg(not(target_os = "macos"))]
fn credentials() -> Option<String> {
    std::fs::read_to_string(util::home()?.join(".claude").join(".credentials.json")).ok()
}

fn get(token: &str) -> Result<String, LimitsError> {
    let mut child = util::hidden("curl")
        .args(["-sS", "-m", "8", "-K", "-", "-w", "\n%{http_code}", URL])
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|_| LimitsError::Offline)?;
    {
        let mut stdin = child.stdin.take().ok_or(LimitsError::Offline)?;
        let config = format!(
            "header = \"Authorization: Bearer {token}\"\n\
             header = \"anthropic-beta: oauth-2025-04-20\"\n\
             header = \"User-Agent: picobar/{}\"\n",
            env!("CARGO_PKG_VERSION")
        );
        stdin
            .write_all(config.as_bytes())
            .map_err(|_| LimitsError::Offline)?;
    }
    let out = child.wait_with_output().map_err(|_| LimitsError::Offline)?;
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, code) = text.rsplit_once('\n').ok_or(LimitsError::Offline)?;
    match code.trim() {
        "200" => Ok(body.to_owned()),
        "401" => Err(LimitsError::Expired),
        "000" | "" => Err(LimitsError::Offline),
        _ => Err(LimitsError::Unexpected),
    }
}

/// Parse the endpoint's JSON. Buckets that are absent or null become `None`.
pub fn parse(body: &str) -> Option<Limits> {
    let v: Value = serde_json::from_str(body).ok()?;
    v.as_object()?;
    let bucket = |key: &str| -> Option<Bucket> {
        let o = v.get(key)?;
        Some(Bucket {
            pct: o.get("utilization")?.as_f64()? as f32,
            resets_at: o
                .get("resets_at")
                .and_then(Value::as_str)
                .and_then(util::parse_iso),
        })
    };
    Some(Limits {
        session: bucket("five_hour"),
        weekly: bucket("seven_day"),
        weekly_opus: bucket("seven_day_opus"),
        weekly_sonnet: bucket("seven_day_sonnet"),
        plan: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_usage_response() {
        let body = r#"{
            "five_hour": {"utilization": 62.0, "resets_at": "2026-10-26T14:30:00.000000+00:00"},
            "seven_day": {"utilization": 41, "resets_at": "2026-10-29T08:00:00Z"},
            "seven_day_opus": null,
            "seven_day_sonnet": {"utilization": 12.5, "resets_at": null},
            "extra_usage": {"is_enabled": false}
        }"#;
        let l = parse(body).unwrap();
        assert_eq!(l.session.unwrap().pct, 62.0);
        assert_eq!(
            l.session.unwrap().resets_at,
            util::parse_iso("2026-10-26T14:30:00Z")
        );
        assert_eq!(l.weekly.unwrap().pct, 41.0);
        assert!(l.weekly_opus.is_none());
        assert_eq!(l.weekly_sonnet.unwrap().resets_at, None);
    }

    #[test]
    fn rejects_non_objects() {
        assert!(parse("[]").is_none());
        assert!(parse("not json").is_none());
    }
}
