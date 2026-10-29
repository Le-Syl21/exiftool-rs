//! The GUI's settings, kept between runs: the theme and the language picked
//! in the toolbar, as `key=value` lines in `exiftool-rs/gui.conf` under the
//! platform's configuration folder (`$EXIFTOOL_RS_GUI_CONFIG` names another
//! file).

use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// "system", "light" or "dark"
    pub theme: Option<String>,
    /// A code of `i18n::AVAILABLE_LANGUAGES`
    pub lang: Option<String>,
}

impl Settings {
    pub fn path() -> Option<PathBuf> {
        if let Some(p) = std::env::var_os("EXIFTOOL_RS_GUI_CONFIG") {
            return Some(PathBuf::from(p));
        }
        let base = if cfg!(target_os = "windows") {
            std::env::var_os("APPDATA").map(PathBuf::from)
        } else if cfg!(target_os = "macos") {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
        } else {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        };
        base.map(|b| b.join("exiftool-rs").join("gui.conf"))
    }

    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim().to_string();
            match key.trim() {
                "theme" => s.theme = Some(value),
                "lang" => s.lang = Some(value),
                _ => {}
            }
        }
        s
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        if let Some(t) = &self.theme {
            out.push_str(&format!("theme={t}\n"));
        }
        if let Some(l) = &self.lang {
            out.push_str(&format!("lang={l}\n"));
        }
        out
    }

    pub fn load() -> Settings {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|t| Self::parse(&t))
            .unwrap_or_default()
    }

    /// Best effort: a settings file that cannot be written is no error.
    pub fn save(&self) {
        if let Some(p) = Self::path() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(p, self.to_text());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip() {
        let s = Settings { theme: Some("light".into()), lang: Some("fr".into()) };
        assert_eq!(Settings::parse(&s.to_text()), s);
        assert_eq!(Settings::parse("junk\ntheme = dark\n"), Settings { theme: Some("dark".into()), lang: None });
    }
}
