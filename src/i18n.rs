//! Internationalization support for tag descriptions.
//!
//! Uses YAML locale files in `locales/` directory.
//! Add a new language by creating `locales/xx.yml` with `TagName: "Translation"` entries,
//! then add it to AVAILABLE_LANGUAGES and LOCALES below.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Available languages — add new ones here and they appear in -h automatically
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

// Embed locale files at compile time
static LOCALES: &[(&str, &str)] = &[
    ("en_ca", include_str!("../locales/en_ca.yml")),
    ("en_gb", include_str!("../locales/en_gb.yml")),
    ("fr", include_str!("../locales/fr.yml")),
    ("es", include_str!("../locales/es.yml")),
    ("pt", include_str!("../locales/pt.yml")),
    ("it", include_str!("../locales/it.yml")),
    ("de", include_str!("../locales/de.yml")),
    ("nl", include_str!("../locales/nl.yml")),
    ("sv", include_str!("../locales/sv.yml")),
    ("fi", include_str!("../locales/fi.yml")),
    ("pl", include_str!("../locales/pl.yml")),
    ("cs", include_str!("../locales/cs.yml")),
    ("sk", include_str!("../locales/sk.yml")),
    ("tr", include_str!("../locales/tr.yml")),
    ("ru", include_str!("../locales/ru.yml")),
    ("ar", include_str!("../locales/ar.yml")),
    ("hi", include_str!("../locales/hi.yml")),
    ("bn", include_str!("../locales/bn.yml")),
    ("zh", include_str!("../locales/zh.yml")),
    ("zh_tw", include_str!("../locales/zh_tw.yml")),
    ("ja", include_str!("../locales/ja.yml")),
    ("ko", include_str!("../locales/ko.yml")),
];

static PARSED_LOCALES: OnceLock<HashMap<String, HashMap<String, String>>> = OnceLock::new();

fn parse_yaml_simple(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        if let Some((key, val)) = line.split_once(": ") {
            let key = key.trim().trim_matches('"');
            let val = val.trim().trim_matches('"');
            if !key.is_empty() && !val.is_empty() {
                map.insert(key.to_string(), val.to_string());
            }
        }
    }
    map
}

fn get_all_locales() -> &'static HashMap<String, HashMap<String, String>> {
    PARSED_LOCALES.get_or_init(|| {
        let mut all = HashMap::new();
        for (code, content) in LOCALES {
            all.insert(code.to_string(), parse_yaml_simple(content));
        }
        all
    })
}

/// Get translations for a language code. Returns None for "en" or unknown languages.
pub fn get_translations(lang: &str) -> Option<HashMap<&'static str, &'static str>> {
    // Normalize lang code
    let lang = match lang {
        "zh_cn" | "zh_CN" | "zhcn" | "zh-cn" | "zh-CN" => "zh",
        "zh_tw" | "zh_TW" | "zhtw" | "zh-tw" | "zh-TW" => "zh_tw",
        "pt_br" | "pt_BR" | "ptbr" | "pt-br" | "pt-BR" => "pt",
        "en_ca" | "en_CA" | "en-ca" | "en-CA" => "en_ca",
        "en_gb" | "en_GB" | "en-gb" | "en-GB" => "en_gb",
        other => other,
    };

    if lang == "en" { return None; }

    let locales = get_all_locales();
    let locale = locales.get(lang)?;

    let leaked: &'static HashMap<String, String> = Box::leak(Box::new(locale.clone()));
    let mut result = HashMap::new();
    for (k, v) in leaked {
        result.insert(k.as_str(), v.as_str());
    }
    Some(result)
}

/// Translate a tag description. Returns the original if no translation exists.
pub fn translate(lang: &str, tag_name: &str, default: &str) -> String {
    if lang == "en" { return default.to_string(); }
    let locales = get_all_locales();
    if let Some(locale) = locales.get(lang) {
        if let Some(translation) = locale.get(tag_name) {
            return translation.clone();
        }
    }
    default.to_string()
}

/// Detect system language for GUI autodetection.
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
    if let Ok(output) = std::process::Command::new("defaults")
        .args(["read", "-globalDomain", "AppleLanguages"])
        .output()
    {
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

/// List available language codes
pub fn available_languages() -> Vec<(&'static str, &'static str)> {
    AVAILABLE_LANGUAGES.to_vec()
}

/// GUI interface translations — reads from YAML locale files (key: _ui.xxx)
pub fn ui_text<'a>(lang: &str, key: &'a str) -> &'a str {
    // Look up _ui.{key} in locale YAML
    let ui_key = format!("_ui.{}", key);
    let locales = get_all_locales();

    // Try requested language first
    if lang != "en" {
        if let Some(locale) = locales.get(lang) {
            if let Some(val) = locale.get(&ui_key) {
                return Box::leak(val.clone().into_boxed_str());
            }
        }
    }

    // Fallback to English locale
    static EN_LOCALE: OnceLock<HashMap<String, String>> = OnceLock::new();
    let en = EN_LOCALE.get_or_init(|| {
        parse_yaml_simple(include_str!("../locales/en.yml"))
    });
    if let Some(val) = en.get(&ui_key) {
        return Box::leak(val.clone().into_boxed_str());
    }

    // Final fallback: return the key itself
    // Use hardcoded match for emoji-prefixed defaults
    key
}
