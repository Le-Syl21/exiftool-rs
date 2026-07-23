//! Internationalization support for tag descriptions and PrintConv values.
//!
//! Tag descriptions + UI strings live in YAML locale files (`locales/xx.yml`,
//! `TagName: "Translation"` and `_ui.*` entries). PrintConv value translations
//! (localized enum output, e.g. `Off` → `Arrêt`) live in tab-separated tables
//! (`locales/values/xx.tsv`, `tag<TAB>english<TAB>translation`), applied only
//! when `-lang` is set. Both are generated from ExifTool's `Lang/*.pm`.
//!
//! Add a new language by creating `locales/xx.yml` (and optionally
//! `locales/values/xx.tsv`), then add it to AVAILABLE_LANGUAGES, LOCALES, and
//! VALUE_LOCALES below.

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

// Embed PrintConv value-translation tables (tab-separated: tag\tenglish\ttranslation)
// at compile time. Only the languages ExifTool itself localizes have a file; the
// extra languages (ar/bn/hi/pt) and English carry no value translations.
static VALUE_LOCALES: &[(&str, &str)] = &[
    ("en_ca", include_str!("../locales/values/en_ca.tsv")),
    ("en_gb", include_str!("../locales/values/en_gb.tsv")),
    ("fr", include_str!("../locales/values/fr.tsv")),
    ("es", include_str!("../locales/values/es.tsv")),
    ("it", include_str!("../locales/values/it.tsv")),
    ("de", include_str!("../locales/values/de.tsv")),
    ("nl", include_str!("../locales/values/nl.tsv")),
    ("sv", include_str!("../locales/values/sv.tsv")),
    ("fi", include_str!("../locales/values/fi.tsv")),
    ("pl", include_str!("../locales/values/pl.tsv")),
    ("cs", include_str!("../locales/values/cs.tsv")),
    ("sk", include_str!("../locales/values/sk.tsv")),
    ("tr", include_str!("../locales/values/tr.tsv")),
    ("ru", include_str!("../locales/values/ru.tsv")),
    ("zh", include_str!("../locales/values/zh.tsv")),
    ("zh_tw", include_str!("../locales/values/zh_tw.tsv")),
    ("ja", include_str!("../locales/values/ja.tsv")),
    ("ko", include_str!("../locales/values/ko.tsv")),
];

// Group-scoped PrintConv value overrides (tab-separated: group1\ttag\tenglish\t
// output). These correct the flat tables where the family-1 group changes whether
// (or how) ExifTool localizes a value; an output equal to the English input means
// "ExifTool keeps English here" (suppress the flat translation). Generated from
// ExifTool's real corpus output by scripts/gen_value_overrides.py.
static VALUE_OVERRIDES: &[(&str, &str)] = &[
    ("en_ca", include_str!("../locales/values/en_ca.over.tsv")),
    ("en_gb", include_str!("../locales/values/en_gb.over.tsv")),
    ("fr", include_str!("../locales/values/fr.over.tsv")),
    ("es", include_str!("../locales/values/es.over.tsv")),
    ("it", include_str!("../locales/values/it.over.tsv")),
    ("de", include_str!("../locales/values/de.over.tsv")),
    ("nl", include_str!("../locales/values/nl.over.tsv")),
    ("sv", include_str!("../locales/values/sv.over.tsv")),
    ("fi", include_str!("../locales/values/fi.over.tsv")),
    ("pl", include_str!("../locales/values/pl.over.tsv")),
    ("cs", include_str!("../locales/values/cs.over.tsv")),
    ("sk", include_str!("../locales/values/sk.over.tsv")),
    ("tr", include_str!("../locales/values/tr.over.tsv")),
    ("ru", include_str!("../locales/values/ru.over.tsv")),
    ("zh", include_str!("../locales/values/zh.over.tsv")),
    ("zh_tw", include_str!("../locales/values/zh_tw.over.tsv")),
    ("ja", include_str!("../locales/values/ja.over.tsv")),
    ("ko", include_str!("../locales/values/ko.over.tsv")),
];

static PARSED_LOCALES: OnceLock<HashMap<String, HashMap<String, String>>> = OnceLock::new();
#[allow(clippy::type_complexity)]
static PARSED_VALUES: OnceLock<HashMap<String, HashMap<String, HashMap<String, String>>>> =
    OnceLock::new();
// lang -> (group1, tag, english) -> output
#[allow(clippy::type_complexity)]
static PARSED_OVERRIDES: OnceLock<HashMap<String, HashMap<(String, String, String), String>>> =
    OnceLock::new();

fn parse_yaml_simple(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
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

/// Normalize a user-supplied language code to the locale key used by the tables
/// (e.g. "zh_CN" → "zh", "pt-BR" → "pt", "en-GB" → "en_gb").
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

fn parse_values_tsv(content: &str) -> HashMap<String, HashMap<String, String>> {
    let mut map: HashMap<String, HashMap<String, String>> = HashMap::new();
    for line in content.lines() {
        let mut parts = line.splitn(3, '\t');
        if let (Some(tag), Some(eng), Some(tr)) = (parts.next(), parts.next(), parts.next()) {
            if !tag.is_empty() && !eng.is_empty() && !tr.is_empty() {
                map.entry(tag.to_string())
                    .or_default()
                    .insert(eng.to_string(), tr.to_string());
            }
        }
    }
    map
}

fn get_all_values() -> &'static HashMap<String, HashMap<String, HashMap<String, String>>> {
    PARSED_VALUES.get_or_init(|| {
        let mut all = HashMap::new();
        for (code, content) in VALUE_LOCALES {
            all.insert(code.to_string(), parse_values_tsv(content));
        }
        all
    })
}

#[allow(clippy::type_complexity)]
fn get_all_overrides() -> &'static HashMap<String, HashMap<(String, String, String), String>> {
    PARSED_OVERRIDES.get_or_init(|| {
        let mut all = HashMap::new();
        for (code, content) in VALUE_OVERRIDES {
            let mut map = HashMap::new();
            for line in content.lines() {
                let mut p = line.splitn(4, '\t');
                if let (Some(g), Some(tag), Some(eng), Some(out)) =
                    (p.next(), p.next(), p.next(), p.next())
                {
                    if !g.is_empty() && !tag.is_empty() {
                        map.insert(
                            (g.to_string(), tag.to_string(), eng.to_string()),
                            out.to_string(),
                        );
                    }
                }
            }
            all.insert(code.to_string(), map);
        }
        all
    })
}

/// Translate a PrintConv output value for a tag, when `-lang` is set. `group1` is
/// the tag's family-1 group, used to disambiguate same-named tags that ExifTool
/// localizes differently (or not at all) depending on their source table.
/// Returns the localized string, or `None` when the value stays in English.
/// Faithful to ExifTool: a group-scoped override (from real corpus output) wins
/// over the broad per-tag table, and an override equal to the input value means
/// ExifTool keeps English there.
pub fn translate_value(lang: &str, group1: &str, tag_name: &str, value: &str) -> Option<String> {
    let lang = normalize_lang(lang);
    if lang == "en" {
        return None;
    }
    // Group-scoped override is authoritative (it mirrors ExifTool's real output).
    if let Some(over) = get_all_overrides().get(lang) {
        if let Some(out) = over.get(&(group1.to_string(), tag_name.to_string(), value.to_string()))
        {
            return if out == value {
                None
            } else {
                Some(out.clone())
            };
        }
    }
    get_all_values()
        .get(lang)?
        .get(tag_name)?
        .get(value)
        .cloned()
}

/// Get translations for a language code. Returns None for "en" or unknown languages.
pub fn get_translations(lang: &str) -> Option<HashMap<&'static str, &'static str>> {
    let lang = normalize_lang(lang);

    if lang == "en" {
        return None;
    }

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
    if lang == "en" {
        return default.to_string();
    }
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
                let trimmed = line
                    .trim()
                    .trim_matches(|c| c == '"' || c == ',' || c == '(' || c == ')');
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
    let en = EN_LOCALE.get_or_init(|| parse_yaml_simple(include_str!("../locales/en.yml")));
    if let Some(val) = en.get(&ui_key) {
        return Box::leak(val.clone().into_boxed_str());
    }

    // Final fallback: return the key itself
    // Use hardcoded match for emoji-prefixed defaults
    key
}
