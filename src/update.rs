//! Built-in update: find the latest GitHub release, download the build for
//! this platform, verify its SHA-256 against the release's SHA256SUMS.txt,
//! swap it in place and relaunch.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::util;

const REPO: &str = "lido772/picobar";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
pub const RELEASES_PAGE: &str = "https://github.com/lido772/picobar/releases/latest";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub page: String,
    asset: Option<(String, String)>,
    sums: Option<String>,
}

impl Release {
    /// Whether this release ships a build PicoBar can install by itself.
    pub fn installable(&self) -> bool {
        self.asset.is_some() && self.sums.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UpdateState {
    #[default]
    Unknown,
    Checking,
    UpToDate,
    Available(Release),
    Installing(String),
    Failed(String),
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

/// File name of this platform's build for `version`.
/// None when no build is published for this platform.
pub fn asset_name(version: &str) -> Option<String> {
    if cfg!(target_os = "macos") {
        Some(format!("PicoBar-{version}-macos-universal.dmg"))
    } else if cfg!(windows) {
        Some(format!("PicoBar-{version}-windows-x64-portable.zip"))
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some(format!("PicoBar-{version}-linux-aarch64.tar.gz"))
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some(format!("PicoBar-{version}-linux-x86_64.tar.gz"))
    } else {
        None
    }
}

/// Latest published release if it is newer than this build.
pub fn check() -> Result<Option<Release>, String> {
    // PICOBAR_RELEASES_URL points the check at another server (tests).
    let url = std::env::var("PICOBAR_RELEASES_URL")
        .unwrap_or_else(|_| format!("https://api.github.com/repos/{REPO}/releases/latest"));
    let out = util::hidden("curl")
        .args([
            "-sSL",
            "-m",
            "15",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            &format!("User-Agent: picobar/{CURRENT}"),
            &url,
        ])
        .output()
        .map_err(|_| "curl not found".to_string())?;
    if !out.status.success() {
        return Err("offline".into());
    }
    parse_latest(&String::from_utf8_lossy(&out.stdout))
}

fn parse_latest(body: &str) -> Result<Option<Release>, String> {
    let gh: GhRelease =
        serde_json::from_str(body).map_err(|_| "no published release".to_string())?;
    let version = gh.tag_name.trim_start_matches('v').to_owned();
    if !is_newer(&version, CURRENT) {
        return Ok(None);
    }
    let want = asset_name(&version);
    let find = |name: &str| {
        gh.assets
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.browser_download_url.clone())
    };
    Ok(Some(Release {
        asset: want.and_then(|w| find(&w).map(|u| (w, u))),
        sums: find("SHA256SUMS.txt"),
        version,
        page: gh.html_url,
    }))
}

/// Numeric comparison of dotted versions ("0.10.0" > "0.9.3").
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.split(['-', '+'])
            .next()
            .unwrap_or("")
            .split('.')
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parse(candidate), parse(current));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    false
}

/// Download, verify and install `rel`, then start the new version.
/// On success the caller should exit right away.
pub fn install(rel: &Release) -> Result<(), String> {
    let (name, url) = rel.asset.clone().ok_or("no build for this platform")?;
    let sums_url = rel.sums.clone().ok_or("release has no checksums")?;
    let target = install_target()?;
    let work = target
        .parent()
        .ok_or("bad install path")?
        .join(".picobar-update");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|_| {
        "PicoBar is installed system-wide; get the new version from the release page".to_string()
    })?;

    let result = (|| {
        let file = work.join(&name);
        download(&url, &file)?;
        let sums = work.join("SHA256SUMS.txt");
        download(&sums_url, &sums)?;
        let expected = std::fs::read_to_string(&sums)
            .ok()
            .and_then(|s| {
                s.lines()
                    .find(|l| {
                        l.split_whitespace()
                            .nth(1)
                            .map(|n| n.trim_start_matches('*'))
                            == Some(name.as_str())
                    })
                    .and_then(|l| l.split_whitespace().next().map(str::to_lowercase))
            })
            .ok_or("checksum missing for this build")?;
        let data = std::fs::read(&file).map_err(|_| "download failed")?;
        if hex(&sha256(&data)) != expected {
            return Err("checksum mismatch, update cancelled".to_string());
        }
        drop(data);
        swap_in(&file, &work, &target)
    })();
    let _ = std::fs::remove_dir_all(&work);
    result?;
    relaunch(&target)
}

fn download(url: &str, to: &Path) -> Result<(), String> {
    let ok = util::hidden("curl")
        .args(["-sSLf", "-m", "300", "-o"])
        .arg(to)
        .arg(url)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err("download failed".into())
    }
}

fn run(cmd: &mut Command, what: &str) -> Result<(), String> {
    match cmd.status() {
        Ok(s) if s.success() => Ok(()),
        _ => Err(format!("{what} failed")),
    }
}

/// What gets replaced: the .app bundle on macOS, the executable elsewhere.
fn install_target() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|_| "cannot locate PicoBar")?;
    let exe = exe.canonicalize().unwrap_or(exe);
    if exe.to_string_lossy().contains("/Cellar/") {
        return Err("installed with Homebrew: run `brew upgrade picobar`".into());
    }
    if cfg!(target_os = "macos") {
        exe.ancestors()
            .find(|p| p.extension().is_some_and(|e| e == "app"))
            .map(Path::to_path_buf)
            .ok_or_else(|| "not running from PicoBar.app".to_string())
    } else {
        Ok(exe)
    }
}

#[cfg(target_os = "macos")]
fn swap_in(dmg: &Path, work: &Path, app: &Path) -> Result<(), String> {
    let mnt = work.join("mnt");
    run(
        util::hidden("/usr/bin/hdiutil")
            .args(["attach", "-nobrowse", "-readonly", "-quiet", "-mountpoint"])
            .arg(&mnt)
            .arg(dmg),
        "opening the disk image",
    )?;
    let staged = work.join("PicoBar.app");
    let copied = run(
        util::hidden("/usr/bin/ditto")
            .arg(mnt.join("PicoBar.app"))
            .arg(&staged),
        "copying the new version",
    );
    let _ = util::hidden("/usr/bin/hdiutil")
        .args(["detach", "-quiet"])
        .arg(&mnt)
        .status();
    copied?;
    let old = work.join("PicoBar-old.app");
    std::fs::rename(app, &old).map_err(|e| format!("cannot replace the app ({e})"))?;
    if let Err(e) = std::fs::rename(&staged, app) {
        let _ = std::fs::rename(&old, app);
        return Err(format!("cannot replace the app ({e})"));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn swap_in(archive: &Path, work: &Path, exe: &Path) -> Result<(), String> {
    let out = work.join("new");
    std::fs::create_dir_all(&out).map_err(|_| "cannot unpack")?;
    // Windows 10+ and Linux both ship bsdtar/GNU tar able to unpack these.
    let flags = if cfg!(windows) { "-xf" } else { "-xzf" };
    run(
        util::hidden("tar")
            .arg(flags)
            .arg(archive)
            .arg("-C")
            .arg(&out),
        "unpacking",
    )?;
    let name = if cfg!(windows) {
        "PicoBar.exe"
    } else {
        "picobar"
    };
    let new = out.join(name);
    if !new.is_file() {
        return Err("unexpected archive layout".into());
    }
    // A running executable can be renamed (Windows) or replaced (Linux).
    let old = exe.with_extension("old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old).map_err(|e| format!("cannot replace PicoBar ({e})"))?;
    if let Err(e) = std::fs::copy(&new, exe) {
        let _ = std::fs::rename(&old, exe);
        return Err(format!("cannot replace PicoBar ({e})"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(exe, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

fn relaunch(target: &Path) -> Result<(), String> {
    let spawned = if cfg!(target_os = "macos") {
        util::hidden("/usr/bin/open").arg("-n").arg(target).spawn()
    } else {
        Command::new(target).spawn()
    };
    spawned
        .map(|_| ())
        .map_err(|_| "installed; please start PicoBar again".into())
}

/// Remove what a previous update left behind (Windows keeps the old .exe).
pub fn cleanup() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("old"));
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 (FIPS 180-4).
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [
                t1.wrapping_add(t2),
                v[0],
                v[1],
                v[2],
                v[3].wrapping_add(t1),
                v[4],
                v[5],
                v[6],
            ];
        }
        for (a, b) in h.iter_mut().zip(v) {
            *a = a.wrapping_add(b);
        }
    }
    let mut out = [0u8; 32];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vectors() {
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let long = vec![b'a'; 1000];
        assert_eq!(
            hex(&sha256(&long)),
            "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3"
        );
    }

    #[test]
    fn compares_versions() {
        assert!(is_newer("0.3.0", "0.2.0"));
        assert!(is_newer("0.10.0", "0.9.3"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.1.9", "0.2.0"));
        assert!(!is_newer("0.2.0-beta", "0.2.0"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_asset_names() {
        let want = if cfg!(target_arch = "aarch64") {
            "PicoBar-1.2.3-linux-aarch64.tar.gz"
        } else {
            "PicoBar-1.2.3-linux-x86_64.tar.gz"
        };
        assert_eq!(asset_name("1.2.3").as_deref(), Some(want));
    }

    #[test]
    fn parses_latest_release() {
        let name = asset_name("99.0.0").unwrap();
        let body = format!(
            r#"{{"tag_name":"v99.0.0","html_url":"https://x/r","assets":[
                {{"name":"{name}","browser_download_url":"https://x/a"}},
                {{"name":"SHA256SUMS.txt","browser_download_url":"https://x/s"}}]}}"#
        );
        let r = parse_latest(&body).unwrap().unwrap();
        assert_eq!(r.version, "99.0.0");
        assert!(r.installable());
        let old = r#"{"tag_name":"v0.0.1","html_url":"x","assets":[]}"#;
        assert_eq!(parse_latest(old).unwrap(), None);
        assert!(parse_latest(r#"{"message":"Not Found"}"#).is_err());
    }
}
