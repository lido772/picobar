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

/// PicoBar's own folder, created on demand: `~/.picobar`, or a
/// `PicoBar Data` folder next to the executable in portable mode (when a
/// file named `portable` sits beside it).
pub fn config_dir() -> Option<PathBuf> {
    let dir = match portable_dir() {
        Some(d) => d.join("PicoBar Data"),
        None => home()?.join(".picobar"),
    };
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// The executable's folder when running in portable mode.
pub fn portable_dir() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    dir.join("portable").is_file().then_some(dir)
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

/// Local UTC offset in seconds at unix time `ts` (DST-aware on macOS/Linux).
#[cfg(unix)]
pub fn utc_offset(ts: i64) -> i64 {
    let t = ts as libc::time_t;
    // SAFETY: localtime_r only writes into the zeroed `tm` we own.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        0
    } else {
        tm.tm_gmtoff as i64
    }
}

/// Local UTC offset in seconds (current rules).
#[cfg(windows)]
pub fn utc_offset(_ts: i64) -> i64 {
    use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    // SAFETY: the API fills the zeroed struct we own.
    let mut tz: TIME_ZONE_INFORMATION = unsafe { std::mem::zeroed() };
    let id = unsafe { GetTimeZoneInformation(&mut tz) };
    let bias = tz.Bias
        + if id == 2 {
            tz.DaylightBias
        } else {
            tz.StandardBias
        };
    -(bias as i64) * 60
}

/// Local day number (days since 1970-01-01 in local time).
pub fn local_day(ts: i64) -> i64 {
    (ts + utc_offset(ts)).div_euclid(86_400)
}

/// Local "HH:MM".
pub fn local_hm(ts: i64) -> String {
    let s = (ts + utc_offset(ts)).rem_euclid(86_400);
    format!("{:02}:{:02}", s / 3600, s % 3600 / 60)
}

/// Monday = 0 … Sunday = 6, for a local day number.
pub fn weekday(day: i64) -> usize {
    (day + 3).rem_euclid(7) as usize
}

/// (year, month, day) for a day number (days since 1970-01-01).
pub fn civil(day: i64) -> (i64, u32, u32) {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

pub const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// "Thu 09:00" for a future time within a week, "Thu 29" style otherwise.
pub fn local_day_hm(ts: i64) -> String {
    format!("{} {}", WEEKDAYS[weekday(local_day(ts))], local_hm(ts))
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
