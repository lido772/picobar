//! Launch at login. The state lives on disk (not in settings) and counts as
//! on only when the entry names the running executable.

pub fn enabled() -> bool {
    os_enabled()
}

pub fn set(on: bool) -> Result<(), String> {
    os_set(on)
}

// ---------------------------------------------------------------- Linux

#[cfg(not(any(target_os = "macos", windows)))]
fn entry_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| Some(crate::util::home()?.join(".config")))?;
    Some(base.join("autostart").join("picobar.desktop"))
}

#[cfg(not(any(target_os = "macos", windows)))]
fn os_enabled() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let line = exec_line(&exe.to_string_lossy());
    entry_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .is_some_and(|t| t.lines().any(|l| l == line))
}

#[cfg(not(any(target_os = "macos", windows)))]
fn os_set(on: bool) -> Result<(), String> {
    let path = entry_path().ok_or("no config folder")?;
    if !on {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    }
    let exe = crate::util::stable_exe()?;
    let text = desktop_entry(&exe.to_string_lossy());
    std::fs::create_dir_all(path.parent().ok_or("no config folder")?)
        .and_then(|()| std::fs::write(path, text))
        .map_err(|e| e.to_string())
}

// -------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
fn entry_path() -> Option<std::path::PathBuf> {
    Some(
        crate::util::home()?
            .join("Library/LaunchAgents")
            .join("io.github.lido772.picobar.plist"),
    )
}

#[cfg(target_os = "macos")]
fn os_enabled() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let arg = format!("<string>{}</string>", xml_escape(&exe.to_string_lossy()));
    entry_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .is_some_and(|t| t.contains(&arg))
}

#[cfg(target_os = "macos")]
fn os_set(on: bool) -> Result<(), String> {
    let path = entry_path().ok_or("no home folder")?;
    if !on {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    }
    let exe = crate::util::stable_exe()?;
    let text = launch_agent(&exe.to_string_lossy());
    let tmp = path.with_extension("plist.tmp");
    std::fs::create_dir_all(path.parent().ok_or("no home folder")?)
        .and_then(|()| std::fs::write(&tmp, text))
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| e.to_string())
}

// ------------------------------------------------------------ Windows

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const RUN_VALUE: &str = "PicoBar";

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Pre-registry installers put a shortcut here; it still counts as "on".
#[cfg(windows)]
fn legacy_lnk() -> Option<std::path::PathBuf> {
    let appdata = std::path::PathBuf::from(std::env::var_os("APPDATA")?);
    Some(appdata.join(r"Microsoft\Windows\Start Menu\Programs\Startup\PicoBar.lnk"))
}

#[cfg(windows)]
fn remove_legacy_lnk() -> Result<(), String> {
    match legacy_lnk().map(std::fs::remove_file) {
        Some(Err(e)) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

#[cfg(windows)]
fn os_enabled() -> bool {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let mut buf = [0u16; 2048]; // longer values (ERROR_MORE_DATA) count as "off"
    let mut bytes = std::mem::size_of_val(&buf) as u32;
    // SAFETY: NUL-terminated wide strings and a buffer of `bytes` bytes, all alive for the call.
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide(RUN_KEY).as_ptr(),
            wide(RUN_VALUE).as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    let stored = (rc == 0).then(|| {
        let n = (bytes as usize / 2).min(buf.len());
        String::from_utf16_lossy(&buf[..n])
    });
    stored.is_some_and(|v| run_value_matches(v.trim_end_matches('\0'), &exe.to_string_lossy()))
        || legacy_lnk().is_some_and(|p| p.exists())
}

#[cfg(windows)]
fn os_set(on: bool) -> Result<(), String> {
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RegDeleteKeyValueW, RegSetKeyValueW,
    };
    let key = wide(RUN_KEY);
    let name = wide(RUN_VALUE);
    let rc = if on {
        let exe = crate::util::stable_exe()?;
        let data = wide(&run_value(&exe.to_string_lossy()));
        // SAFETY: NUL-terminated wide strings; `data` is `len * 2` bytes incl. the NUL.
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            )
        }
    } else {
        // SAFETY: NUL-terminated wide strings.
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) }
    };
    // 2 / 3 = value / key not found: already off.
    if rc != 0 && (on || (rc != 2 && rc != 3)) {
        return Err(format!("registry error {rc}"));
    }
    remove_legacy_lnk()
}

// -------------------------------------------------- Run value text

/// The Run value: the exe path as one quoted command.
#[cfg(any(test, windows))]
fn run_value(exe: &str) -> String {
    format!("\"{exe}\"")
}

/// Windows paths are case-insensitive.
#[cfg(any(test, windows))]
fn run_value_matches(stored: &str, exe: &str) -> bool {
    stored.to_lowercase() == run_value(exe).to_lowercase()
}

// ------------------------------------------------- Desktop Entry text

/// `Exec=` with the path as one quoted argument (Desktop Entry spec): inside
/// double quotes `"`, `` ` ``, `$` and `\` take a backslash, then the string
/// value doubles every backslash; `%` is a field code, so `%%`.
#[cfg(any(test, not(any(target_os = "macos", windows))))]
fn exec_line(exe: &str) -> String {
    let mut q = String::from("Exec=\"");
    for c in exe.chars() {
        match c {
            '"' | '`' | '$' => q.push_str(&format!("\\\\{c}")),
            '\\' => q.push_str("\\\\\\\\"),
            '%' => q.push_str("%%"),
            _ => q.push(c),
        }
    }
    q.push('"');
    q
}

#[cfg(any(test, not(any(target_os = "macos", windows))))]
fn desktop_entry(exe: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=PicoBar\n{}\nTerminal=false\n",
        exec_line(exe)
    )
}

// ------------------------------------------------------ LaunchAgent text

#[cfg(any(test, target_os = "macos"))]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(any(test, target_os = "macos"))]
fn launch_agent(exe: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n\
<dict>\n\
<key>Label</key>\n<string>io.github.lido772.picobar</string>\n\
<key>ProgramArguments</key>\n<array>\n<string>{}</string>\n</array>\n\
<key>RunAtLoad</key>\n<true/>\n\
<key>LimitLoadToSessionType</key>\n<string>Aqua</string>\n\
</dict>\n\
</plist>\n",
        xml_escape(exe)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_exec_per_desktop_entry_spec() {
        assert_eq!(
            exec_line("/opt/pico bar/picobar"),
            "Exec=\"/opt/pico bar/picobar\""
        );
        assert_eq!(exec_line("/a\"b`c$d"), "Exec=\"/a\\\\\"b\\\\`c\\\\$d\"");
        assert_eq!(exec_line("/a\\b%c"), "Exec=\"/a\\\\\\\\b%%c\"");
    }

    #[test]
    fn quotes_and_matches_run_value() {
        assert_eq!(
            run_value(r"C:\Pico Bar\PicoBar.exe"),
            r#""C:\Pico Bar\PicoBar.exe""#
        );
        assert!(run_value_matches(
            r#""c:\pico bar\picobar.EXE""#,
            r"C:\Pico Bar\PicoBar.exe"
        ));
        assert!(!run_value_matches(
            r"C:\Pico Bar\PicoBar.exe",
            r"C:\Pico Bar\PicoBar.exe"
        ));
        assert!(!run_value_matches(
            r#""C:\Other\PicoBar.exe""#,
            r"C:\Pico Bar\PicoBar.exe"
        ));
    }

    #[test]
    fn builds_a_desktop_entry() {
        let t = desktop_entry("/usr/bin/picobar");
        assert!(t.starts_with("[Desktop Entry]\nType=Application\n"));
        assert!(t.contains("\nExec=\"/usr/bin/picobar\"\n"));
    }

    #[test]
    fn builds_a_launch_agent() {
        let t = launch_agent("/Apps/A&B <x>/picobar");
        assert!(t.contains("<string>/Apps/A&amp;B &lt;x&gt;/picobar</string>"));
        assert!(t.contains("<key>RunAtLoad</key>\n<true/>"));
        assert!(t.contains("<string>Aqua</string>"));
    }
}
