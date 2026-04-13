//! ExifTool configuration file parser (.ExifTool_config).
//!
//! Supports user-defined tags and shortcuts in a simplified format.
//! The Perl ExifTool uses Perl code in the config; we support a subset:
//!
//! ```text
//! # Comment
//! %Image::ExifTool::UserDefined = (
//!     'Image::ExifTool::Exif::Main' => {
//!         0xd000 => { Name => 'MyCustomTag', Writable => 'string' },
//!     },
//! );
//!
//! %Image::ExifTool::UserDefined::Shortcuts = (
//!     MyShortcut => ['Artist', 'Copyright', 'Title'],
//! );
//! ```

use std::path::Path;

/// A user-defined tag from the config file.
#[derive(Debug, Clone)]
pub struct UserTag {
    pub tag_id: u16,
    pub name: String,
    pub writable: Option<String>,
    pub group: String,
}

/// A tag shortcut (group of tag names).
#[derive(Debug, Clone)]
pub struct Shortcut {
    pub name: String,
    pub tags: Vec<String>,
}

/// Parsed configuration.
#[derive(Debug, Default)]
pub struct Config {
    pub user_tags: Vec<UserTag>,
    pub shortcuts: Vec<Shortcut>,
}

impl Config {
    /// Load configuration from the default location.
    pub fn load_default() -> Self {
        // Try ~/.ExifTool_config, then ./.ExifTool_config
        let candidates = [
            dirs_home().map(|h| h.join(".ExifTool_config")),
            Some(std::path::PathBuf::from(".ExifTool_config")),
        ];

        for candidate in candidates.iter().flatten() {
            if candidate.exists() {
                if let Some(config) = Self::load(candidate) {
                    return config;
                }
            }
        }

        Self::default()
    }

    /// Load and parse a config file.
    pub fn load<P: AsRef<Path>>(path: P) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        Some(Self::parse(&content))
    }

    /// Parse config file content.
    fn parse(content: &str) -> Self {
        let mut config = Config::default();

        // Remove comments
        let lines: Vec<&str> = content
            .lines()
            .map(|l| l.split('#').next().unwrap_or("").trim())
            .collect();
        let text = lines.join("\n");

        // Parse UserDefined tags
        if let Some(start) = text.find("%Image::ExifTool::UserDefined") {
            if let Some(paren_start) = text[start..].find('(') {
                let block_start = start + paren_start + 1;
                if let Some(block_end) = find_matching_paren(&text, block_start) {
                    let block = &text[block_start..block_end];
                    parse_user_tags(block, &mut config.user_tags);
                }
            }
        }

        // Parse Shortcuts
        if let Some(start) = text.find("Shortcuts") {
            if let Some(paren_start) = text[start..].find('(') {
                let block_start = start + paren_start + 1;
                if let Some(block_end) = find_matching_paren(&text, block_start) {
                    let block = &text[block_start..block_end];
                    parse_shortcuts(block, &mut config.shortcuts);
                }
            }
        }

        config
    }
}

fn parse_user_tags(block: &str, tags: &mut Vec<UserTag>) {
    // Look for: 0xNNNN => { Name => 'XXX' }
    let mut pos = 0;
    while let Some(hex_pos) = block[pos..].find("0x") {
        let abs_pos = pos + hex_pos;
        let rest = &block[abs_pos + 2..];

        // Read hex number
        let hex_end = rest
            .find(|c: char| !c.is_ascii_hexdigit())
            .unwrap_or(rest.len());
        if let Ok(tag_id) = u16::from_str_radix(&rest[..hex_end], 16) {
            // Find Name
            if let Some(name_pos) = rest.find("Name") {
                let after_name = &rest[name_pos..];
                if let Some(name) = extract_perl_string(after_name) {
                    tags.push(UserTag {
                        tag_id,
                        name: name.clone(),
                        writable: extract_after_key(after_name, "Writable"),
                        group: "UserDefined".to_string(),
                    });
                }
            }
        }

        pos = abs_pos + hex_end + 2;
    }
}

fn parse_shortcuts(block: &str, shortcuts: &mut Vec<Shortcut>) {
    // Look for: ShortcutName => ['Tag1', 'Tag2', ...]
    for line in block.lines() {
        let line = line.trim();
        if let Some(arrow) = line.find("=>") {
            let name = line[..arrow]
                .trim()
                .trim_matches('\'')
                .trim_matches('"')
                .to_string();
            let rest = &line[arrow + 2..];

            // Parse array ['Tag1', 'Tag2']
            if let Some(bracket_start) = rest.find('[') {
                if let Some(bracket_end) = rest.find(']') {
                    let array_content = &rest[bracket_start + 1..bracket_end];
                    let tags: Vec<String> = array_content
                        .split(',')
                        .map(|s| s.trim().trim_matches('\'').trim_matches('"').to_string())
                        .filter(|s| !s.is_empty())
                        .collect();

                    if !name.is_empty() && !tags.is_empty() {
                        shortcuts.push(Shortcut { name, tags });
                    }
                }
            }
        }
    }
}

fn extract_perl_string(text: &str) -> Option<String> {
    // Find first quoted string after =>
    let arrow = text.find("=>")?;
    let rest = &text[arrow + 2..];
    let rest = rest.trim();

    if rest.starts_with('\'') {
        let end = rest[1..].find('\'')?;
        Some(rest[1..1 + end].to_string())
    } else if rest.starts_with('"') {
        let end = rest[1..].find('"')?;
        Some(rest[1..1 + end].to_string())
    } else {
        None
    }
}

fn extract_after_key(text: &str, key: &str) -> Option<String> {
    let pos = text.find(key)?;
    extract_perl_string(&text[pos..])
}

fn find_matching_paren(text: &str, start: usize) -> Option<usize> {
    let mut depth = 1;
    let bytes = text.as_bytes();
    let mut i = start;
    while i < bytes.len() && depth > 0 {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var("HOME").ok().map(std::path::PathBuf::from)
}
