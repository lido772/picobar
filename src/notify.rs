//! Native notifications through tools the OS already ships (no extra crates):
//! `osascript` on macOS, a PowerShell toast on Windows.

use crate::util;

pub fn send(title: &str, body: &str) {
    #[cfg(target_os = "macos")]
    let child = util::hidden("/usr/bin/osascript")
        .args([
            "-e",
            &format!(
                "display notification {} with title {}",
                apple_quote(body),
                apple_quote(title)
            ),
        ])
        .spawn();

    #[cfg(windows)]
    let child = util::hidden("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &toast_script(title, body),
        ])
        .spawn();

    #[cfg(not(any(target_os = "macos", windows)))]
    let child = util::hidden("notify-send").args([title, body]).spawn();

    if let Ok(mut c) = child {
        std::thread::spawn(move || c.wait());
    }
}

#[cfg(target_os = "macos")]
fn apple_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(windows)]
fn toast_script(title: &str, body: &str) -> String {
    let q = |s: &str| format!("'{}'", s.replace('\'', "''"));
    format!(
        "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null;\
         $t = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02);\
         $x = $t.GetElementsByTagName('text');\
         $x.Item(0).AppendChild($t.CreateTextNode({})) > $null;\
         $x.Item(1).AppendChild($t.CreateTextNode({})) > $null;\
         $id = '{{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}}\\WindowsPowerShell\\v1.0\\powershell.exe';\
         [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier($id).Show([Windows.UI.Notifications.ToastNotification]::new($t))",
        q(title),
        q(body)
    )
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    #[test]
    fn quotes_applescript() {
        assert_eq!(super::apple_quote(r#"a "b" \c"#), r#""a \"b\" \\c""#);
    }
}
