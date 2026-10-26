//! Small helpers: time, paths, hidden child processes.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

/// Current time as unix seconds.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// PicoBar's own folder (`~/.picobar`), created on demand.
pub fn config_dir() -> Option<PathBuf> {
    let dir = home()?.join(".picobar");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// A child process with no console window (Windows) and no inherited stdio.
pub fn hidden(program: &str) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    cmd
}

/// Parse RFC 3339 timestamps such as `2026-10-26T09:28:52.511Z` or
/// `2026-10-26T14:00:00.123456+00:00` into unix seconds.
pub fn parse_iso(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let num = |from: usize, to: usize| s.get(from..to)?.parse::<i64>().ok();
    if b.len() < 19 || b[4] != b'-' || b[10] != b'T' && b[10] != b' ' {
        return None;
    }
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, se) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    let mut i = 19;
    if b.get(i) == Some(&b'.') {
        i += 1;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
    }
    let offset = match b.get(i) {
        None | Some(b'Z') | Some(b'z') => 0,
        Some(&sign @ (b'+' | b'-')) => {
            let oh = num(i + 1, i + 3)?;
            let om = if b.get(i + 3) == Some(&b':') {
                num(i + 4, i + 6)?
            } else {
                num(i + 3, i + 5).unwrap_or(0)
            };
            let v = oh * 3600 + om * 60;
            if sign == b'-' { -v } else { v }
        }
        _ => return None,
    };
    Some(days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se - offset)
}

/// Days since 1970-01-01 (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 6480 -> "1h48", 280000 -> "3d 5h", 300 -> "5m".
pub fn countdown(secs: i64) -> String {
    if secs <= 0 {
        return "now".into();
    }
    let (d, h, m) = (secs / 86_400, secs % 86_400 / 3600, secs % 3600 / 60);
    match (d, h) {
        (0, 0) => format!("{}m", m.max(1)),
        (0, _) => format!("{h}h{m:02}"),
        _ => format!("{d}d {h}h"),
    }
}

/// Open a URL in the default browser.
pub fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let child = hidden("/usr/bin/open").arg(url).spawn();
    #[cfg(windows)]
    let child = hidden("cmd").args(["/C", "start", "", url]).spawn();
    #[cfg(not(any(target_os = "macos", windows)))]
    let child = hidden("xdg-open").arg(url).spawn();
    if let Ok(mut c) = child {
        std::thread::spawn(move || c.wait());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_timestamps() {
        assert_eq!(parse_iso("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso("2026-10-26T09:28:52.511Z"), Some(1_793_006_932));
        assert_eq!(
            parse_iso("2026-10-26T11:28:52.123456+02:00"),
            Some(1_793_006_932)
        );
        assert_eq!(parse_iso("2026-10-26T09:28:52+0000"), Some(1_793_006_932));
        assert_eq!(parse_iso("garbage"), None);
    }

    #[test]
    fn formats_countdowns() {
        assert_eq!(countdown(6480), "1h48");
        assert_eq!(countdown(280_000), "3d 5h");
        assert_eq!(countdown(300), "5m");
        assert_eq!(countdown(-1), "now");
    }
}
