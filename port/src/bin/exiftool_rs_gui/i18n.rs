//! The GUI's own strings (the `_ui.*` entries of the root crate's
//! `locales/*.yml`, gathered in `data/gui_text.tsv` by
//! scripts/gen_extra_langs.pl), its language list and the system language.
//!
//! Tag descriptions and values are not here: they are ExifTool's, in the
//! language of the `Lang` option (`Options::lang`), from ExifTool's Lang
//! modules and those exiftool-rs adds (ar, bn, hi, pt).

use std::collections::HashMap;
use std::sync::OnceLock;

/// The languages of the language selector: code, then name. The codes are
/// the root crate's (`zh` is Simplified Chinese, ExifTool's `zh_cn`).
pub const AVAILABLE_LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"),
    ("en_ca", "English (CA)"),
    ("en_gb", "English (UK)"),
    ("fr", "Français"),
    ("es", "Español"),
    ("pt", "Português"),
    ("it", "Italiano"),
    ("de", "Deutsch"),
    ("nl", "Nederlands"),
    ("sv", "Svenska"),
    ("fi", "Suomi"),
    ("pl", "Polski"),
    ("cs", "Čeština"),
    ("sk", "Slovenčina"),
    ("tr", "Türkçe"),
    ("ru", "Русский"),
    ("ar", "العربية"),
    ("hi", "हिन्दी"),
    ("bn", "বাংলা"),
    ("zh", "中文"),
    ("zh_tw", "繁體中文"),
    ("ja", "日本語"),
    ("ko", "한국어"),
];

static GUI_TEXT: &str = include_str!("../../../data/gui_text.tsv");

/// The strings of the port's GUI that the root crate's has not (theme and
/// language selectors, the names of the icon buttons, the states a screen
/// reader says), in the same `lang<TAB>key<TAB>text` form.
static GUI_EXTRA_TEXT: &str = include_str!("gui_extra_text.tsv");

/// language -> key -> text
fn all_text() -> &'static HashMap<&'static str, HashMap<&'static str, &'static str>> {
    static TEXT: OnceLock<HashMap<&'static str, HashMap<&'static str, &'static str>>> = OnceLock::new();
    TEXT.get_or_init(|| {
        let mut all: HashMap<&str, HashMap<&str, &str>> = HashMap::new();
        for line in GUI_TEXT.lines().chain(GUI_EXTRA_TEXT.lines()) {
            let mut p = line.splitn(3, '\t');
            if let (Some(lang), Some(key), Some(text)) = (p.next(), p.next(), p.next()) {
                all.entry(lang).or_default().insert(key, text);
            }
        }
        all
    })
}

/// The language code of the locale files for a code the user may give
/// (`zh_CN` is `zh`, `pt-BR` is `pt`, `en-GB` is `en_gb`).
fn normalize_lang(lang: &str) -> &str {
    match lang {
        "zh_cn" | "zh_CN" | "zhcn" | "zh-cn" | "zh-CN" => "zh",
        "zh_tw" | "zh_TW" | "zhtw" | "zh-tw" | "zh-TW" => "zh_tw",
        "pt_br" | "pt_BR" | "ptbr" | "pt-br" | "pt-BR" => "pt",
        "en_ca" | "en_CA" | "en-ca" | "en-CA" => "en_ca",
        "en_gb" | "en_GB" | "en-gb" | "en-GB" => "en_gb",
        other => other,
    }
}

/// The `Lang` option for a GUI language: `zh` is ExifTool's `zh_cn`
/// (`Options::lang` takes it as such), and English is no option.
pub fn exiftool_lang(lang: &str) -> Option<String> {
    match normalize_lang(lang) {
        "en" => None,
        other => Some(other.to_string()),
    }
}

/// The BCP 47 tag of a language of the selector, which tells a screen
/// reader the voice (and braille table) to read a text with.
pub fn bcp47(lang: &str) -> &str {
    match lang {
        "en_ca" => "en-CA",
        "en_gb" => "en-GB",
        "zh" => "zh-CN",
        "zh_tw" => "zh-TW",
        other => other,
    }
}

/// GUI interface text: `_ui.{key}` of the language, else of English, else
/// the key itself.
pub fn ui_text<'a>(lang: &str, key: &'a str) -> &'a str {
    let all = all_text();
    let lang = normalize_lang(lang);
    if lang != "en" {
        if let Some(text) = all.get(lang).and_then(|m| m.get(key)) {
            return text;
        }
    }
    if let Some(text) = all.get("en").and_then(|m| m.get(key)) {
        return text;
    }
    key
}

/// Detect the system language for the GUI.
/// Returns the language code (e.g., "fr", "de", "ja") or "en" as fallback.
pub fn detect_system_language() -> String {
    // 1. Check POSIX environment variables (Linux, macOS terminal)
    for var in &["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(val) = std::env::var(var) {
            if let Some(lang) = match_locale(&val) {
                return lang;
            }
        }
    }

    // 2. Platform-specific detection
    if let Some(lang) = detect_platform_language() {
        return lang;
    }

    "en".to_string()
}

/// Try to match a locale string (e.g. "fr_FR.UTF-8", "fr-FR", "fr") to a supported language.
fn match_locale(val: &str) -> Option<String> {
    let val = val.to_lowercase();
    // Parse "fr_FR.UTF-8" → "fr"
    let code = val.split('.').next().unwrap_or(&val);
    let short = code.split('_').next().unwrap_or(code);
    // Check short code first (e.g. "fr")
    if AVAILABLE_LANGUAGES.iter().any(|(c, _)| *c == short) {
        return Some(short.to_string());
    }
    // Try full code (e.g. "zh_tw", "en_ca")
    let full = code.replace('-', "_");
    if AVAILABLE_LANGUAGES.iter().any(|(c, _)| *c == full) {
        return Some(full);
    }
    None
}

/// Platform-specific language detection.
#[cfg(target_os = "windows")]
fn detect_platform_language() -> Option<String> {
    // Use Windows GetUserDefaultLocaleName API
    #[link(name = "kernel32")]
    extern "system" {
        fn GetUserDefaultLocaleName(locale: *mut u16, len: i32) -> i32;
    }
    let mut buf = [0u16; 85];
    let len = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
    if len > 0 {
        let locale = String::from_utf16_lossy(&buf[..len as usize - 1]);
        return match_locale(&locale);
    }
    None
}

#[cfg(target_os = "macos")]
fn detect_platform_language() -> Option<String> {
    // Use defaults read .GlobalPreferences AppleLanguages
    if let Ok(output) = std::process::Command::new("defaults").args(["read", "-globalDomain", "AppleLanguages"]).output() {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            // Output is a plist array like: ( "fr-FR", "en-US", ... )
            // Extract the first language
            for line in text.lines() {
                let trimmed = line.trim().trim_matches(|c| c == '"' || c == ',' || c == '(' || c == ')');
                if !trimmed.is_empty() {
                    if let Some(lang) = match_locale(trimmed) {
                        return Some(lang);
                    }
                }
            }
        }
    }
    None
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn detect_platform_language() -> Option<String> {
    None // Linux relies on environment variables above
}
