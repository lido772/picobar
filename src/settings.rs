//! User settings: a tiny `key=value` file in PicoBar's config folder.
//! Unknown keys and invalid values fall back to the default.

use crate::util;

const FILE: &str = "settings";
/// Old way to turn notifications off, before this file existed.
const NOTIFY_OFF_FILE: &str = "notifications-off";

pub const WARN_CHOICES: [u8; 3] = [70, 80, 90];
pub const REFRESH_CHOICES: [u8; 4] = [3, 5, 10, 15];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuBar {
    Percent,
    Reset,
    Icon,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub notify: bool,
    /// Warning threshold in percent.
    pub warn: u8,
    /// Minutes between limits fetches.
    pub refresh_min: u8,
    pub menubar: MenuBar,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            notify: true,
            warn: 80,
            refresh_min: 3,
            menubar: MenuBar::Percent,
        }
    }
}

impl Settings {
    pub fn to_text(self) -> String {
        let menubar = match self.menubar {
            MenuBar::Percent => "percent",
            MenuBar::Reset => "reset",
            MenuBar::Icon => "icon",
        };
        format!(
            "notify={}\nwarn={}\nrefresh={}\nmenubar={menubar}\n",
            if self.notify { "on" } else { "off" },
            self.warn,
            self.refresh_min
        )
    }
}

pub fn parse(text: &str) -> Settings {
    let mut s = Settings::default();
    for (k, v) in text.lines().filter_map(|l| l.split_once('=')) {
        let v = v.trim();
        match k.trim() {
            "notify" if v == "on" || v == "off" => s.notify = v == "on",
            "warn" => {
                if let Some(w) = v.parse().ok().filter(|w| WARN_CHOICES.contains(w)) {
                    s.warn = w
                }
            }
            "refresh" => {
                if let Some(m) = v.parse().ok().filter(|m| REFRESH_CHOICES.contains(m)) {
                    s.refresh_min = m
                }
            }
            "menubar" => {
                s.menubar = match v {
                    "reset" => MenuBar::Reset,
                    "icon" => MenuBar::Icon,
                    _ => MenuBar::Percent,
                }
            }
            _ => {}
        }
    }
    s
}

pub fn load() -> Settings {
    let Some(dir) = util::config_dir() else {
        return Settings::default();
    };
    match std::fs::read_to_string(dir.join(FILE)) {
        Ok(text) => parse(&text),
        Err(_) => Settings {
            notify: !dir.join(NOTIFY_OFF_FILE).exists(),
            ..Default::default()
        },
    }
}

/// Write atomically (temp + rename), then drop the old marker.
pub fn save(s: Settings) {
    let Some(dir) = util::config_dir() else {
        return;
    };
    let (tmp, path) = (dir.join("settings.tmp"), dir.join(FILE));
    if std::fs::write(&tmp, s.to_text()).is_ok() && std::fs::rename(&tmp, path).is_ok() {
        let _ = std::fs::remove_file(dir.join(NOTIFY_OFF_FILE));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let s = Settings {
            notify: false,
            warn: 90,
            refresh_min: 15,
            menubar: MenuBar::Reset,
        };
        assert_eq!(parse(&s.to_text()), s);
        assert_eq!(parse(""), Settings::default());
    }

    #[test]
    fn invalid_values_fall_back_to_default() {
        let s = parse("notify=maybe\nwarn=85\nrefresh=1\nmenubar=big\nwhat\nfoo=bar\n");
        assert_eq!(s, Settings::default());
        assert_eq!(parse("warn=70\r\n bogus ").warn, 70);
    }
}
