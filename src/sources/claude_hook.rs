//! Permission-prompt detection. Logs cannot show a pending permission dialog,
//! so an opt-in Claude Code `Notification` hook (matcher `permission_prompt`)
//! runs `picobar --hook`, which touches `config_dir()/waiting/<session_id>`.
//! A marker counts only while it is newer than the session log.
//! Also installs/removes that hook in `~/.claude/settings.json`.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::util;

const MARKER_MAX_AGE_SECS: i64 = 86_400;

/// A session id becomes a file name: only `[A-Za-z0-9_-]{1,64}`.
pub fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn marker_dir() -> Option<PathBuf> {
    Some(util::config_dir()?.join("waiting"))
}

/// `picobar --hook`: silent, never fails.
pub fn run_hook() {
    let _ = touch();
}

fn touch() -> Option<()> {
    let mut s = String::new();
    std::io::stdin().take(1 << 20).read_to_string(&mut s).ok()?;
    let v: Value = serde_json::from_str(&s).ok()?;
    let id = v.get("session_id")?.as_str().filter(|i| valid_id(i))?;
    let dir = marker_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    let f = std::fs::File::create(dir.join(id)).ok()?;
    f.set_modified(std::time::SystemTime::now()).ok()
}

/// Session id -> marker mtime. Markers older than a day are deleted.
pub fn markers(now: i64) -> HashMap<String, i64> {
    let mut out = HashMap::new();
    let Some(rd) = marker_dir().and_then(|d| std::fs::read_dir(d).ok()) else {
        return out;
    };
    for e in rd.flatten() {
        let Some(id) = e.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(t) = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        else {
            continue;
        };
        let t = t.as_secs() as i64;
        if now - t > MARKER_MAX_AGE_SECS {
            let _ = std::fs::remove_file(e.path());
        } else if valid_id(&id) {
            out.insert(id, t);
        }
    }
    out
}

// ------------------------------------------------------------ settings.json

fn settings_path() -> Option<PathBuf> {
    Some(util::home()?.join(".claude").join("settings.json"))
}

/// The hook command for this executable; quoted so spaces work everywhere.
fn command() -> Option<String> {
    Some(quote(&std::env::current_exe().ok()?.display().to_string()) + " --hook")
}

#[cfg(unix)]
fn quote(p: &str) -> String {
    format!("'{}'", p.replace('\'', "'\\''"))
}

#[cfg(not(unix))]
fn quote(p: &str) -> String {
    format!("\"{p}\"")
}

/// `<path to picobar[.exe]>` (quoted or not) followed by `--hook`.
fn is_ours(cmd: &str) -> bool {
    let Some(path) = cmd.trim_end().strip_suffix("--hook") else {
        return false;
    };
    let path = path.trim_end().trim_end_matches(['"', '\'']).to_lowercase();
    let name = path.rsplit(['/', '\\']).next().unwrap_or("");
    name == "picobar" || name == "picobar.exe"
}

fn our_hook(h: &Value) -> bool {
    h.get("command")
        .and_then(Value::as_str)
        .is_some_and(is_ours)
}

fn notification_entries(root: &mut Value) -> Option<&mut Vec<Value>> {
    root.get_mut("hooks")?
        .get_mut("Notification")?
        .as_array_mut()
}

/// Remove our hooks (and entries left empty by that); true if any removed.
fn unmerge(root: &mut Value) -> bool {
    let Some(entries) = notification_entries(root) else {
        return false;
    };
    let mut removed = false;
    // Only entries this call emptied go; pre-existing empty ones stay.
    entries.retain_mut(|e| {
        let Some(hs) = e.get_mut("hooks").and_then(Value::as_array_mut) else {
            return true;
        };
        let n = hs.len();
        hs.retain(|h| !our_hook(h));
        removed |= hs.len() != n;
        hs.len() == n || !hs.is_empty()
    });
    removed
}

/// Idempotent: replaces any previous PicoBar entry, keeps everything else.
fn merge(root: &mut Value, cmd: &str) -> Result<(), String> {
    const BAD: &str = "settings.json has an unexpected shape; left untouched";
    unmerge(root);
    let hooks = root
        .as_object_mut()
        .ok_or(BAD)?
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or(BAD)?;
    hooks
        .entry("Notification")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or(BAD)?
        .push(json!({
            "matcher": "permission_prompt",
            "hooks": [{ "type": "command", "command": cmd }],
        }));
    Ok(())
}

fn installed_command(root: &Value) -> Option<&str> {
    root.get("hooks")?
        .get("Notification")?
        .as_array()?
        .iter()
        .filter_map(|e| e.get("hooks")?.as_array())
        .flatten()
        .filter_map(|h| h.get("command")?.as_str())
        .find(|c| is_ours(c))
}

fn load(path: &PathBuf) -> Result<Value, String> {
    match std::fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s)
            .map_err(|_| "settings.json is not valid JSON; left untouched".to_owned()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(_) => Err("could not read settings.json".into()),
    }
}

fn save(path: &PathBuf, root: &Value) -> Result<(), String> {
    const FAIL: &str = "could not write settings.json";
    let fail = |_| FAIL.to_owned();
    // Write next to the real file so a symlinked settings.json stays a symlink.
    let path = &std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
    let sibling = |ext: &str| {
        let mut s = path.as_os_str().to_owned();
        s.push(ext);
        PathBuf::from(s)
    };
    let (bak, tmp) = (sibling(".picobar-bak"), sibling(".picobar-tmp"));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(fail)?;
    }
    if path.exists() && !bak.exists() {
        std::fs::copy(path, &bak).map_err(fail)?;
    }
    let text = serde_json::to_string_pretty(root).map_err(|_| FAIL.to_owned())?;
    let write = || -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        if let Ok(m) = std::fs::metadata(path) {
            std::fs::set_permissions(&tmp, m.permissions())?;
        }
        std::fs::rename(&tmp, path)
    };
    write().map_err(|_| {
        let _ = std::fs::remove_file(&tmp);
        FAIL.to_owned()
    })
}

pub fn install() -> Result<(), String> {
    let (path, cmd) = settings_path().zip(command()).ok_or("no home folder")?;
    if std::env::current_exe().is_ok_and(|e| e.to_string_lossy().contains("/AppTranslocation/")) {
        return Err("Move PicoBar to Applications first".into());
    }
    let mut root = load(&path)?;
    merge(&mut root, &cmd)?;
    save(&path, &root)
}

pub fn uninstall() -> Result<(), String> {
    let path = settings_path().ok_or("no home folder")?;
    let mut root = load(&path)?;
    if unmerge(&mut root) {
        save(&path, &root)?;
    }
    Ok(())
}

/// True when the installed hook points at the running executable.
pub fn installed() -> bool {
    let Some((path, cmd)) = settings_path().zip(command()) else {
        return false;
    };
    load(&path).is_ok_and(|r| installed_command(&r) == Some(cmd.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../tests/fixtures/claude_settings.json");
    const CMD: &str = "'/Apps/PicoBar app/picobar' --hook";

    #[test]
    fn id_validation() {
        assert!(valid_id("3f2a-B_9"));
        assert!(!valid_id(""));
        assert!(!valid_id("../x"));
        assert!(!valid_id("a/b"));
        assert!(!valid_id(&"a".repeat(65)));
        assert!(valid_id(&"a".repeat(64)));
    }

    #[test]
    fn install_is_idempotent_and_keeps_user_hooks() {
        let orig: Value = serde_json::from_str(FIXTURE).unwrap();
        let mut v = orig.clone();
        merge(&mut v, CMD).unwrap();
        let once = v.clone();
        merge(&mut v, CMD).unwrap();
        assert_eq!(v, once);
        assert_eq!(installed_command(&v), Some(CMD));
        assert_eq!(v["hooks"]["Notification"].as_array().unwrap().len(), 2);
        assert_eq!(v["hooks"]["PreToolUse"], orig["hooks"]["PreToolUse"]);
        assert_eq!(v["model"], "opus");
    }

    #[test]
    fn uninstall_restores_user_settings() {
        let orig: Value = serde_json::from_str(FIXTURE).unwrap();
        let mut v = orig.clone();
        merge(&mut v, CMD).unwrap();
        assert!(unmerge(&mut v));
        assert_eq!(v, orig);
        assert!(!unmerge(&mut v));
        assert_eq!(installed_command(&v), None);
    }

    #[cfg(unix)]
    #[test]
    fn quotes_single_quotes() {
        assert_eq!(quote("/a b/it's/picobar"), r"'/a b/it'\''s/picobar'");
    }

    #[test]
    fn recognises_only_our_command() {
        for ok in [
            "'/x/PicoBar' --hook",
            "\"C:\\Apps\\PicoBar.exe\" --hook  ",
            "/usr/bin/picobar --hook",
        ] {
            assert!(is_ours(ok), "{ok}");
        }
        for no in [
            "/x/picobar-helper --hook",
            "/x/notpicobar --hook",
            "/x/picobar --other",
            "/x/picobar --hook --more",
            "echo picobar --hook",
        ] {
            assert!(!is_ours(no), "{no}");
        }
    }

    #[test]
    fn unmerge_keeps_preexisting_empty_entries() {
        let mut v = json!({"hooks": {"Notification": [
            {"matcher": "x", "hooks": []},
            {"matcher": "permission_prompt", "hooks": [{"type": "command", "command": CMD}]},
        ]}});
        assert!(unmerge(&mut v));
        assert_eq!(
            v["hooks"]["Notification"],
            json!([{"matcher": "x", "hooks": []}])
        );
    }

    #[cfg(unix)]
    #[test]
    fn save_keeps_symlink_and_permissions() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let d = std::env::temp_dir().join(format!("picobar-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("real")).unwrap();
        let real = d.join("real/settings.json");
        std::fs::write(&real, "{}").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o640)).unwrap();
        let link = d.join("settings.json");
        symlink(&real, &link).unwrap();
        save(&link, &json!({"a": 1})).unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(
            std::fs::metadata(&real).unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert!(std::fs::read_to_string(&real).unwrap().contains("\"a\""));
        assert_eq!(
            std::fs::read_to_string(d.join("real/settings.json.picobar-bak")).unwrap(),
            "{}"
        );
        assert!(!d.join("real/settings.json.picobar-tmp").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn merge_rejects_odd_shapes() {
        let mut v = json!({"hooks": {"Notification": "x"}});
        assert!(merge(&mut v, CMD).is_err());
    }
}
