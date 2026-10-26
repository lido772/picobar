//! Token counts and active sessions from Claude Code's local logs
//! (`~/.claude/projects/<project>/<session>.jsonl`, subagents in
//! `<session>/subagents/*.jsonl`). Files are read incrementally: only bytes
//! appended since the last refresh are parsed.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Deserialize;

use crate::util;

/// Keep a bit more than one 5-hour window of entries.
const KEEP_SECS: i64 = 5 * 3600 + 600;
/// A log written to in the last 2 minutes counts as an active session.
const ACTIVE_SECS: u64 = 120;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
}

impl Tokens {
    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_write + self.cache_read
    }

    pub fn cache(&self) -> u64 {
        self.cache_write + self.cache_read
    }
}

#[derive(Deserialize)]
struct Line {
    timestamp: Option<String>,
    message: Option<Msg>,
}

#[derive(Deserialize)]
struct Msg {
    id: Option<String>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Active {
    pub sessions: usize,
    pub subagents: usize,
}

#[derive(Default)]
pub struct ClaudeLogs {
    offsets: HashMap<PathBuf, u64>,
    /// (unix seconds, message id, tokens); one entry per API message.
    entries: Vec<(i64, String, Tokens)>,
    seen: HashSet<String>,
    active: Active,
}

impl ClaudeLogs {
    pub fn refresh(&mut self) {
        let Some(root) = util::home().map(|h| h.join(".claude").join("projects")) else {
            return;
        };
        let now = util::now();
        let sys_now = SystemTime::now();
        let keep_cut = sys_now - Duration::from_secs(KEEP_SECS as u64);
        let active_cut = sys_now - Duration::from_secs(ACTIVE_SECS);

        let mut files = Vec::new();
        walk(&root, 0, &mut files);

        let mut live = HashSet::new();
        let mut active = Active::default();
        for (file, depth) in files {
            let Ok(mtime) = std::fs::metadata(&file).and_then(|m| m.modified()) else {
                continue;
            };
            if mtime < keep_cut {
                continue;
            }
            if mtime >= active_cut {
                if depth <= 1 {
                    active.sessions += 1;
                } else {
                    active.subagents += 1;
                }
            }
            self.read_new(&file, now);
            live.insert(file);
        }

        self.offsets.retain(|p, _| live.contains(p));
        self.entries.retain(|(ts, _, _)| *ts >= now - KEEP_SECS);
        self.seen = self.entries.iter().map(|(_, id, _)| id.clone()).collect();
        self.active = active;
    }

    /// Tokens of messages at or after `start` (unix seconds).
    pub fn since(&self, start: i64) -> Tokens {
        let mut t = Tokens::default();
        for (_, _, e) in self.entries.iter().filter(|(ts, _, _)| *ts >= start) {
            t.input += e.input;
            t.output += e.output;
            t.cache_write += e.cache_write;
            t.cache_read += e.cache_read;
        }
        t
    }

    pub fn active(&self) -> Active {
        self.active
    }

    fn read_new(&mut self, path: &Path, now: i64) {
        let Ok(mut f) = File::open(path) else { return };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        let mut off = self.offsets.get(path).copied().unwrap_or(0);
        if off > len {
            off = 0; // file was truncated or replaced
        }
        if off == len || f.seek(SeekFrom::Start(off)).is_err() {
            return;
        }
        let mut buf = Vec::new();
        if f.take(len - off).read_to_end(&mut buf).is_err() {
            return;
        }
        // Only consume complete lines; a half-written line is read next time.
        let Some(end) = buf.iter().rposition(|&b| b == b'\n') else {
            return;
        };
        self.offsets.insert(path.to_owned(), off + end as u64 + 1);
        for line in buf[..end].split(|&b| b == b'\n') {
            self.ingest(line, now);
        }
    }

    fn ingest(&mut self, line: &[u8], now: i64) {
        if !contains(line, b"\"usage\"") {
            return;
        }
        let Ok(l) = serde_json::from_slice::<Line>(line) else {
            return;
        };
        let Some(ts) = l.timestamp.as_deref().and_then(util::parse_iso) else {
            return;
        };
        let Some(Msg {
            id: Some(id),
            usage: Some(u),
        }) = l.message
        else {
            return;
        };
        // Claude Code writes one line per content block with the same usage.
        if ts < now - KEEP_SECS || !self.seen.insert(id.clone()) {
            return;
        }
        let t = Tokens {
            input: u.input_tokens,
            output: u.output_tokens,
            cache_write: u.cache_creation_input_tokens,
            cache_read: u.cache_read_input_tokens,
        };
        self.entries.push((ts, id, t));
    }
}

/// Collect `*.jsonl` files up to three levels below `dir`, with their depth.
fn walk(dir: &Path, depth: usize, out: &mut Vec<(PathBuf, usize)>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() && depth < 3 => walk(&path, depth + 1, out),
            Ok(t) if t.is_file() && path.extension().is_some_and(|e| e == "jsonl") => {
                out.push((path, depth))
            }
            _ => {}
        }
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(ts: &str, id: &str, input: u64, output: u64) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{ts}","message":{{"id":"{id}","model":"m","usage":{{"input_tokens":{input},"output_tokens":{output},"cache_creation_input_tokens":10,"cache_read_input_tokens":100,"iterations":[{{"input_tokens":999}}]}}}}}}"#
        )
    }

    #[test]
    fn dedupes_and_windows_entries() {
        let now = util::parse_iso("2026-10-26T12:00:00Z").unwrap();
        let mut logs = ClaudeLogs::default();
        for l in [
            line("2026-10-26T11:00:00Z", "msg_a", 5, 50),
            line("2026-10-26T11:00:01Z", "msg_a", 5, 50), // same message, second block
            line("2026-10-26T11:30:00Z", "msg_b", 1, 20),
            line("2026-10-25T01:00:00Z", "msg_old", 1, 1), // outside the window
            r#"{"type":"user","message":{"role":"user","content":"hi"}}"#.into(),
            "not json".into(),
        ] {
            logs.ingest(l.as_bytes(), now);
        }
        let all = logs.since(now - 5 * 3600);
        assert_eq!(all.input, 6);
        assert_eq!(all.output, 70);
        assert_eq!(all.cache(), 220);
        assert_eq!(all.total(), 296);
        let recent = logs.since(util::parse_iso("2026-10-26T11:15:00Z").unwrap());
        assert_eq!(recent.output, 20);
    }

    #[test]
    fn reads_only_complete_lines_incrementally() {
        let dir = std::env::temp_dir().join(format!("picobar-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("s.jsonl");
        let now = util::now();
        let ts = |s: i64| {
            // seconds ago -> RFC 3339 (UTC) via a tiny formatter for tests
            let t = now - s;
            let (days, rem) = (t.div_euclid(86_400), t.rem_euclid(86_400));
            let (y, m, d) = civil(days);
            format!(
                "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
                rem / 3600,
                rem % 3600 / 60,
                rem % 60
            )
        };
        let first = line(&ts(60), "msg_1", 1, 1);
        let second = line(&ts(30), "msg_2", 2, 2);
        std::fs::write(&file, format!("{first}\n{}", &second[..20])).unwrap();

        let mut logs = ClaudeLogs::default();
        logs.read_new(&file, now);
        assert_eq!(logs.since(0).input, 1);

        std::fs::write(&file, format!("{first}\n{second}\n")).unwrap();
        logs.read_new(&file, now);
        assert_eq!(logs.since(0).input, 3);
        std::fs::remove_dir_all(&dir).ok();
    }

    fn civil(z: i64) -> (i64, i64, i64) {
        let z = z + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        (yoe + era * 400 + i64::from(m <= 2), m, d)
    }
}
