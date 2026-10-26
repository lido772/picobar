//! Tokens, history and per-session state from Claude Code's local logs
//! (`~/.claude/projects/<project>/<session>.jsonl`, subagents in
//! `<session>/subagents/*.jsonl`). Files are read incrementally: only bytes
//! appended since the last refresh are parsed. The first scan (35 days of
//! history) runs on a background thread.

use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::sources::claude_hook;
use crate::util;

/// History kept for Trends.
pub const KEEP_SECS: i64 = 35 * 86_400;
/// A session written to in the last 2 minutes is running.
const RUNNING_SECS: i64 = 120;
/// Sessions with activity in the last hour are listed in Agents.
const LIST_SECS: i64 = 3600;

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

    fn add(&mut self, o: &Tokens) {
        self.input += o.input;
        self.output += o.output;
        self.cache_write += o.cache_write;
        self.cache_read += o.cache_read;
    }
}

#[derive(Deserialize)]
struct Line {
    timestamp: Option<String>,
    cwd: Option<String>,
    message: Option<Msg>,
}

#[derive(Deserialize)]
struct Msg {
    id: Option<String>,
    model: Option<String>,
    stop_reason: Option<String>,
    usage: Option<Usage>,
    content: Option<Vec<Block>>,
}

#[derive(Deserialize)]
struct Block {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
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

#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub ts: i64,
    pub file: u32,
    pub model: u16,
    pub tokens: Tokens,
}

#[derive(Default)]
struct FileState {
    offset: u64,
    recent_ids: VecDeque<String>,
    subagent: bool,
    /// Index of the parent session file (for subagents).
    parent: Option<PathBuf>,
    cwd: Option<String>,
    model: Option<String>,
    last_stop: Option<String>,
    /// A user line (new prompt or tool result) arrived after the last assistant message.
    user_after: bool,
    last_ts: i64,
    mtime: i64,
    preview: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Blocked on a permission prompt (see `claude_hook`).
    NeedsInput,
    /// Claude finished its turn and is waiting for you.
    YourTurn,
    Running,
    Idle,
}

#[derive(Debug, Clone)]
pub struct Session {
    /// Log file stem, which is Claude Code's session id.
    pub id: String,
    pub name: String,
    pub model: String,
    pub state: SessionState,
    /// Seconds since the last activity.
    pub since: i64,
    pub tokens_5h: u64,
    pub subagents_running: usize,
    pub preview: String,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Active {
    pub sessions: usize,
    pub subagents: usize,
}

#[derive(Default)]
pub struct ClaudeLogs {
    files: HashMap<PathBuf, FileState>,
    file_ids: HashMap<PathBuf, u32>,
    models: Vec<String>,
    entries: Vec<Entry>,
    /// Permission-prompt markers: session id -> mtime.
    markers: HashMap<String, i64>,
}

/// A marker counts only while it is at least as new as the session log.
fn needs_input(marker: Option<i64>, log_mtime: i64) -> bool {
    marker.is_some_and(|m| m >= log_mtime)
}

impl ClaudeLogs {
    /// Read everything new under `~/.claude/projects`.
    pub fn refresh(&mut self) {
        let Some(root) = util::home().map(|h| h.join(".claude").join("projects")) else {
            return;
        };
        let now = util::now();
        self.markers = claude_hook::markers(now);
        let mut found = Vec::new();
        walk(&root, 0, &mut found);
        let mut live = Vec::with_capacity(found.len());
        for (path, depth) in found {
            let Some(mtime) = mtime(&path) else { continue };
            if mtime < now - KEEP_SECS {
                continue;
            }
            let state = self.files.entry(path.clone()).or_default();
            state.mtime = mtime;
            if depth >= 2 && !state.subagent {
                state.subagent = true;
                // <project>/<session>/subagents/x.jsonl -> <project>/<session>.jsonl
                state.parent = path
                    .parent()
                    .and_then(Path::parent)
                    .map(|s| s.with_extension("jsonl"));
            }
            self.read_new(&path, now);
            live.push(path);
        }
        self.files.retain(|p, _| live.contains(p));
        self.entries.retain(|e| e.ts >= now - KEEP_SECS);
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn model_name(&self, id: u16) -> &str {
        self.models.get(id as usize).map_or("", String::as_str)
    }

    /// Tokens of messages at or after `start` (unix seconds).
    pub fn since(&self, start: i64) -> Tokens {
        let mut t = Tokens::default();
        for e in self.entries.iter().filter(|e| e.ts >= start) {
            t.add(&e.tokens);
        }
        t
    }

    pub fn active(&self, now: i64) -> Active {
        let mut a = Active::default();
        for f in self
            .files
            .values()
            .filter(|f| now - f.mtime <= RUNNING_SECS)
        {
            if f.subagent {
                a.subagents += 1;
            } else {
                a.sessions += 1;
            }
        }
        a
    }

    /// Top-level sessions with activity in the last hour, most relevant first.
    pub fn sessions(&self, now: i64) -> Vec<Session> {
        let mut tokens: HashMap<u32, u64> = HashMap::new();
        for e in self.entries.iter().filter(|e| e.ts >= now - 5 * 3600) {
            *tokens.entry(e.file).or_default() += e.tokens.total();
        }
        let mut out: Vec<Session> = self
            .files
            .iter()
            .filter(|(_, f)| !f.subagent && now - f.mtime <= LIST_SECS && f.model.is_some())
            .map(|(path, f)| {
                let mut own = self
                    .file_ids
                    .get(path)
                    .and_then(|id| tokens.get(id))
                    .copied()
                    .unwrap_or(0);
                let mut subagents_running = 0;
                for (sp, sf) in self
                    .files
                    .iter()
                    .filter(|(_, s)| s.parent.as_deref() == Some(path))
                {
                    own += self
                        .file_ids
                        .get(sp)
                        .and_then(|id| tokens.get(id))
                        .copied()
                        .unwrap_or(0);
                    if now - sf.mtime <= RUNNING_SECS {
                        subagents_running += 1;
                    }
                }
                let your_turn = f.last_stop.as_deref() == Some("end_turn")
                    && !f.user_after
                    && subagents_running == 0;
                let id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_owned();
                let state = if needs_input(self.markers.get(&id).copied(), f.mtime) {
                    SessionState::NeedsInput
                } else if your_turn {
                    SessionState::YourTurn
                } else if now - f.mtime <= RUNNING_SECS || subagents_running > 0 {
                    SessionState::Running
                } else {
                    SessionState::Idle
                };
                Session {
                    id,
                    name: f
                        .cwd
                        .as_deref()
                        .and_then(|c| c.rsplit(['/', '\\']).find(|s| !s.is_empty()))
                        .unwrap_or("session")
                        .to_owned(),
                    model: short_model(f.model.as_deref().unwrap_or("")),
                    state,
                    since: now - f.last_ts.max(f.mtime.min(now)),
                    tokens_5h: own,
                    subagents_running,
                    preview: f.preview.clone(),
                }
            })
            .collect();
        out.sort_by_key(|s| (s.state as u8, s.since));
        out
    }

    fn file_id(&mut self, path: &Path) -> u32 {
        let next = self.file_ids.len() as u32;
        *self.file_ids.entry(path.to_owned()).or_insert(next)
    }

    fn model_id(&mut self, model: &str) -> u16 {
        if let Some(i) = self.models.iter().position(|m| m == model) {
            return i as u16;
        }
        self.models.push(model.to_owned());
        (self.models.len() - 1) as u16
    }

    fn read_new(&mut self, path: &Path, now: i64) {
        /// Read in bounded chunks so a large log never needs a large buffer.
        const CHUNK: usize = 256 * 1024;
        let Ok(mut f) = File::open(path) else { return };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        let state = self.files.entry(path.to_owned()).or_default();
        if state.offset > len {
            state.offset = 0; // truncated or replaced
        }
        let mut off = state.offset;
        if off == len || f.seek(SeekFrom::Start(off)).is_err() {
            return;
        }
        let file = self.file_id(path);
        let mut buf: Vec<u8> = Vec::with_capacity(CHUNK);
        let mut chunk = vec![0u8; CHUNK];
        while off + (buf.len() as u64) < len {
            let want = CHUNK.min((len - off) as usize - buf.len());
            let Ok(n) = f.read(&mut chunk[..want]) else {
                break;
            };
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            // Consume complete lines; keep a partial line for the next chunk.
            let Some(end) = buf.iter().rposition(|&b| b == b'\n') else {
                continue;
            };
            for line in buf[..end].split(|&b| b == b'\n') {
                self.ingest(path, file, line, now);
            }
            off += end as u64 + 1;
            buf.drain(..=end);
        }
        if let Some(s) = self.files.get_mut(path) {
            s.offset = off;
        }
    }

    fn ingest(&mut self, path: &Path, file: u32, line: &[u8], now: i64) {
        let is_user = contains(line, b"\"type\":\"user\"");
        let is_assistant = contains(line, b"\"type\":\"assistant\"");
        if is_user {
            if let Some(s) = self.files.get_mut(path) {
                s.user_after = true;
            }
            return;
        }
        if !is_assistant || !contains(line, b"\"usage\"") {
            return;
        }
        let Ok(l) = serde_json::from_slice::<Line>(line) else {
            return;
        };
        let Some(ts) = l.timestamp.as_deref().and_then(util::parse_iso) else {
            return;
        };
        let Some(msg) = l.message else { return };
        let model_id = msg.model.as_deref().map(|m| self.model_id(m));
        let s = self.files.entry(path.to_owned()).or_default();
        if l.cwd.is_some() {
            s.cwd = l.cwd;
        }
        if ts >= s.last_ts {
            s.last_ts = ts;
            s.user_after = false;
            if msg.stop_reason.is_some() {
                s.last_stop = msg.stop_reason.clone();
            }
            if let Some(m) = &msg.model
                && !m.starts_with('<')
            {
                s.model = Some(m.clone());
            }
            if let Some(text) = msg
                .content
                .iter()
                .flatten()
                .filter(|b| b.kind.as_deref() == Some("text"))
                .filter_map(|b| b.text.as_deref())
                .next_back()
            {
                s.preview = preview(text);
            }
        }
        let (Some(id), Some(u)) = (msg.id, msg.usage) else {
            return;
        };
        // One API message is written as several lines sharing its id.
        if ts < now - KEEP_SECS || s.recent_ids.contains(&id) {
            return;
        }
        s.recent_ids.push_back(id);
        if s.recent_ids.len() > 16 {
            s.recent_ids.pop_front();
        }
        self.entries.push(Entry {
            ts,
            file,
            model: model_id.unwrap_or(u16::MAX),
            tokens: Tokens {
                input: u.input_tokens,
                output: u.output_tokens,
                cache_write: u.cache_creation_input_tokens,
                cache_read: u.cache_read_input_tokens,
            },
        });
    }
}

/// "claude-sonnet-5-5" -> "Sonnet 5.5", "claude-opus-4-1-20250805" -> "Opus 4.1".
pub fn short_model(m: &str) -> String {
    let parts: Vec<&str> = m.trim_start_matches("claude-").split('-').collect();
    let Some(family) = parts.first().filter(|f| !f.is_empty()) else {
        return "Claude".into();
    };
    let mut name = String::new();
    let mut chars = family.chars();
    if let Some(c) = chars.next() {
        name.extend(c.to_uppercase());
        name.push_str(chars.as_str());
    }
    let version: Vec<&str> = parts[1..]
        .iter()
        .take_while(|p| p.len() <= 2 && p.chars().all(|c| c.is_ascii_digit()))
        .copied()
        .collect();
    if !version.is_empty() {
        name.push(' ');
        name.push_str(&version.join("."));
    }
    name
}

fn preview(text: &str) -> String {
    let one_line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = one_line.chars().take(90).collect();
    if one_line.chars().count() > 90 {
        out.push('…');
    }
    out
}

fn mtime(path: &Path) -> Option<i64> {
    let m = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(m.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64)
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

    fn line(ts: &str, id: &str, input: u64, output: u64, stop: &str) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{ts}","cwd":"/Users/me/dev/api","message":{{"id":"{id}","model":"claude-sonnet-5-5","stop_reason":"{stop}","content":[{{"type":"text","text":"All   tests pass."}}],"usage":{{"input_tokens":{input},"output_tokens":{output},"cache_creation_input_tokens":10,"cache_read_input_tokens":100,"iterations":[{{"input_tokens":999}}]}}}}}}"#
        )
    }

    fn ingest_all(logs: &mut ClaudeLogs, lines: &[String], now: i64) {
        let p = PathBuf::from("/tmp/x/s.jsonl");
        let id = logs.file_id(&p);
        for l in lines {
            logs.ingest(&p, id, l.as_bytes(), now);
        }
    }

    #[test]
    fn marker_must_not_be_older_than_log() {
        assert!(needs_input(Some(100), 100));
        assert!(needs_input(Some(101), 100));
        assert!(!needs_input(Some(99), 100));
        assert!(!needs_input(None, 100));
    }

    #[test]
    fn dedupes_and_windows_entries() {
        let now = util::parse_iso("2026-10-26T12:00:00Z").unwrap();
        let mut logs = ClaudeLogs::default();
        ingest_all(
            &mut logs,
            &[
                line("2026-10-26T11:00:00Z", "msg_a", 5, 50, "tool_use"),
                line("2026-10-26T11:00:01Z", "msg_a", 5, 50, "tool_use"),
                line("2026-10-26T11:30:00Z", "msg_b", 1, 20, "end_turn"),
                line("2026-08-01T01:00:00Z", "msg_old", 1, 1, "end_turn"),
                r#"{"type":"system","subtype":"x"}"#.into(),
                "not json".into(),
            ],
            now,
        );
        let all = logs.since(now - 5 * 3600);
        assert_eq!(all.input, 6);
        assert_eq!(all.output, 70);
        assert_eq!(all.cache(), 220);
        assert_eq!(all.total(), 296);
        assert_eq!(logs.entries().len(), 2);
        assert_eq!(
            logs.model_name(logs.entries()[0].model),
            "claude-sonnet-5-5"
        );
    }

    #[test]
    fn tracks_whose_turn_it_is() {
        let now = util::parse_iso("2026-10-26T12:00:00Z").unwrap();
        let mut logs = ClaudeLogs::default();
        ingest_all(
            &mut logs,
            &[line("2026-10-26T11:59:00Z", "m1", 1, 1, "end_turn")],
            now,
        );
        let s = logs.files.values_mut().next().unwrap();
        s.mtime = now - 60;
        let sessions = logs.sessions(now);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].state, SessionState::YourTurn);
        assert_eq!(sessions[0].name, "api");
        assert_eq!(sessions[0].model, "Sonnet 5.5");
        assert_eq!(sessions[0].preview, "All tests pass.");

        ingest_all(
            &mut logs,
            &[r#"{"type":"user","message":{"role":"user"}}"#.into()],
            now,
        );
        assert_eq!(logs.sessions(now)[0].state, SessionState::Running);
    }

    #[test]
    fn shortens_model_names() {
        assert_eq!(short_model("claude-sonnet-5-5"), "Sonnet 5.5");
        assert_eq!(short_model("claude-opus-4-1-20250805"), "Opus 4.1");
        assert_eq!(short_model("claude-haiku-4-5-20251001"), "Haiku 4.5");
        assert_eq!(short_model(""), "Claude");
    }
}
