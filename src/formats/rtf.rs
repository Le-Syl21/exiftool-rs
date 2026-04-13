//! RTF (Rich Text Format) reader - extract metadata from RTF info group.
//! Mirrors ExifTool's RTF.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_rtf(data: &[u8]) -> Result<Vec<Tag>> {
    if !data.starts_with(b"{\\rtf") {
        return Err(Error::InvalidData("not an RTF file".into()));
    }

    let mut tags = Vec::new();
    let text = crate::encoding::decode_utf8_or_latin1(data);

    // Extract the {\info ...} group contents
    if let Some(info_content) = find_rtf_group(&text, "info") {
        // Process info group commands: {\cmd value} or {\* \cmd value}
        let cmd_map = [
            ("title", "Title"),
            ("subject", "Subject"),
            ("author", "Author"),
            ("manager", "Manager"),
            ("company", "Company"),
            ("copyright", "Copyright"),
            ("operator", "LastModifiedBy"),
            ("category", "Category"),
            ("keywords", "Keywords"),
            ("comment", "Comment"),
            ("doccomm", "Comments"),
            ("hlinkbase", "HyperlinkBase"),
        ];
        let date_cmds = [
            ("creatim", "CreateDate"),
            ("revtim", "ModifyDate"),
            ("printim", "LastPrinted"),
            ("buptim", "BackupTime"),
        ];

        // Iterate over {group} blocks inside info
        let groups = extract_rtf_groups(&info_content);
        for (is_star, cmd, content) in groups {
            // date commands
            let mut found_date = false;
            for (dc, dn) in &date_cmds {
                if cmd == *dc {
                    if let Some(dt) = parse_rtf_date(&content) {
                        tags.push(mk(dn, dn, Value::String(dt)));
                    }
                    found_date = true;
                    break;
                }
            }
            if found_date {
                continue;
            }

            // text commands
            for (kw, name) in &cmd_map {
                if cmd == *kw {
                    let val = unescape_rtf(&content);
                    if !val.is_empty() {
                        tags.push(mk(name, name, Value::String(val)));
                    }
                    break;
                }
            }
            let _ = is_star; // used for {\*\company} style tags
        }
    }

    // Scan for user properties {\*\userprops ...}
    if let Some(props_content) = find_rtf_group_star(&text, "userprops") {
        // Parse {{\propname NAME}\proptype N{\staticval VALUE}} structures
        // Perl regex: \{[\n\r]*(\\\*[\n\r]*)?\\([a-zA-Z]+)([^a-zA-Z])
        let mut prop_name: Option<String> = None;
        let prop_str = &props_content;
        let mut search_pos = 0;
        let prop_chars: Vec<char> = prop_str.chars().collect();
        let prop_len = prop_chars.len();

        while search_pos < prop_len {
            // Look for '{'
            if prop_chars[search_pos] != '{' {
                search_pos += 1;
                continue;
            }
            let mut p = search_pos + 1;
            // skip whitespace
            while p < prop_len
                && (prop_chars[p] == '\n' || prop_chars[p] == '\r' || prop_chars[p] == ' ')
            {
                p += 1;
            }
            // skip optional \*
            if p + 1 < prop_len && prop_chars[p] == '\\' && prop_chars[p + 1] == '*' {
                p += 2;
                while p < prop_len
                    && (prop_chars[p] == '\n' || prop_chars[p] == '\r' || prop_chars[p] == ' ')
                {
                    p += 1;
                }
            }
            // must have '\'
            if p >= prop_len || prop_chars[p] != '\\' {
                search_pos += 1;
                continue;
            }
            p += 1;
            // read command name
            let cmd_start = p;
            while p < prop_len && prop_chars[p].is_ascii_alphabetic() {
                p += 1;
            }
            if p == cmd_start {
                search_pos += 1;
                continue;
            }
            let cmd: String = prop_chars[cmd_start..p].iter().collect();

            // skip optional terminator
            if p < prop_len
                && (prop_chars[p] == ' ' || prop_chars[p] == '\n' || prop_chars[p] == '\r')
            {
                p += 1;
            }

            // get content
            let content_chars: String = prop_chars[p..].iter().collect();
            let content = read_to_matching_brace(&content_chars).unwrap_or_default();

            match cmd.as_str() {
                "propname" => {
                    prop_name = Some(unescape_rtf(&content));
                }
                "staticval" => {
                    if let Some(ref name) = prop_name {
                        let tag_name = rtf_prop_name(name);
                        if !tag_name.is_empty() {
                            let val = unescape_rtf(&content);
                            tags.push(mk(&tag_name, &tag_name, Value::String(val)));
                        }
                    }
                    prop_name = None;
                }
                _ => {}
            }

            search_pos += 1;
        }
    }

    Ok(tags)
}

/// Find the content of a {\cmd ...} group (non-starred)
fn find_rtf_group(text: &str, cmd: &str) -> Option<String> {
    let pattern = format!("{{\\{}", cmd);
    let pos = text.find(&pattern)?;
    let rest = &text[pos + pattern.len()..];
    // skip optional non-alpha terminator
    let rest = rest.trim_start_matches(|c: char| c == ' ' || c == '\n' || c == '\r');
    read_to_matching_brace(rest)
}

/// Find {\*\cmd ...} or {\*\n\cmd ...} group (starred)
fn find_rtf_group_star(text: &str, cmd: &str) -> Option<String> {
    let cmd_escape = cmd;

    // Search for {\* followed by optional whitespace and \cmd
    let search = "{\\*";
    let mut pos = 0;
    while let Some(p) = text[pos..].find(search) {
        let start = pos + p;
        let after = &text[start + 3..]; // after "{\*"
        let trimmed =
            after.trim_start_matches(|c: char| c == ' ' || c == '\n' || c == '\r' || c == '\t');
        if trimmed.starts_with(&format!("\\{}", cmd_escape)) {
            // Found it - skip to after the command
            let skip = 3 + (after.len() - trimmed.len()) + 1 + cmd_escape.len();
            let rest_pos = start + skip;
            if rest_pos > text.len() {
                break;
            }
            let rest = &text[rest_pos..];
            let rest = rest.trim_start_matches(|c: char| c == ' ' || c == '\n' || c == '\r');
            return read_to_matching_brace(rest);
        }
        pos = start + 1;
    }
    None
}

/// Read from current position to matching brace (already past the opening brace)
fn read_to_matching_brace(s: &str) -> Option<String> {
    let mut level = 1i32;
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            // escaped char or control sequence
            if let Some(&nc) = chars.peek() {
                if nc == '{' || nc == '}' || nc == '\\' {
                    result.push(c);
                    result.push(nc);
                    chars.next();
                } else {
                    result.push(c);
                }
            }
        } else if c == '{' {
            level += 1;
            result.push(c);
        } else if c == '}' {
            level -= 1;
            if level <= 0 {
                return Some(result);
            }
            result.push(c);
        } else {
            result.push(c);
        }
    }
    None
}

/// Extract all {group} structures from text, returning (is_star, command, content)
fn extract_rtf_groups(text: &str) -> Vec<(bool, String, String)> {
    let mut result = Vec::new();
    let mut pos = 0;
    let bytes = text.as_bytes();

    while pos < bytes.len() {
        // Find '{'
        if bytes[pos] != b'{' {
            pos += 1;
            continue;
        }
        pos += 1; // skip '{'

        // Skip whitespace
        while pos < bytes.len()
            && (bytes[pos] == b' ' || bytes[pos] == b'\n' || bytes[pos] == b'\r')
        {
            pos += 1;
        }

        // Check for \*
        let is_star = if pos + 1 < bytes.len() && bytes[pos] == b'\\' && bytes[pos + 1] == b'*' {
            pos += 2;
            // skip whitespace
            while pos < bytes.len()
                && (bytes[pos] == b' ' || bytes[pos] == b'\n' || bytes[pos] == b'\r')
            {
                pos += 1;
            }
            true
        } else {
            false
        };

        // Must start with '\'
        if pos >= bytes.len() || bytes[pos] != b'\\' {
            // not a command group, skip to closing brace
            skip_to_closing_brace(bytes, &mut pos);
            continue;
        }
        pos += 1; // skip '\'

        // Read command name (alpha chars)
        let cmd_start = pos;
        while pos < bytes.len() && bytes[pos].is_ascii_alphabetic() {
            pos += 1;
        }
        if pos == cmd_start {
            skip_to_closing_brace(bytes, &mut pos);
            continue;
        }
        let cmd = crate::encoding::decode_utf8_or_latin1(&bytes[cmd_start..pos]).to_string();

        // skip optional terminator (space, newline, or digit sequence)
        if pos < bytes.len() && bytes[pos] == b' ' {
            pos += 1;
        } else if pos < bytes.len() && (bytes[pos] == b'\n' || bytes[pos] == b'\r') {
            pos += 1;
        }
        // Skip numeric argument if present
        if pos < bytes.len() && (bytes[pos].is_ascii_digit() || bytes[pos] == b'-') {
            while pos < bytes.len() && (bytes[pos].is_ascii_digit() || bytes[pos] == b'-') {
                pos += 1;
            }
            if pos < bytes.len() && bytes[pos] == b' ' {
                pos += 1;
            }
        }

        // Read content to matching brace
        let content_slice = &text[pos..];
        if let Some(content) = read_to_matching_brace(content_slice) {
            let content_len = content.len();
            result.push((is_star, cmd, content));
            pos += content_len + 1; // +1 for the closing brace
        } else {
            break;
        }
    }
    result
}

fn skip_to_closing_brace(bytes: &[u8], pos: &mut usize) {
    let mut level = 1i32;
    while *pos < bytes.len() {
        let c = bytes[*pos];
        *pos += 1;
        if c == b'\\' {
            // skip escaped char
            if *pos < bytes.len() {
                *pos += 1;
            }
        } else if c == b'{' {
            level += 1;
        } else if c == b'}' {
            level -= 1;
            if level <= 0 {
                return;
            }
        }
    }
}

fn parse_rtf_date(text: &str) -> Option<String> {
    let yr = extract_rtf_num(text, "\\yr").unwrap_or(0);
    let mo = extract_rtf_num(text, "\\mo").unwrap_or(1);
    let dy = extract_rtf_num(text, "\\dy").unwrap_or(1);
    let hr = extract_rtf_num(text, "\\hr").unwrap_or(0);
    let min = extract_rtf_num(text, "\\min").unwrap_or(0);
    let sec = extract_rtf_num(text, "\\sec").unwrap_or(0);
    if yr == 0 {
        return None;
    }
    Some(format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        yr, mo, dy, hr, min, sec
    ))
}

fn extract_rtf_num(text: &str, keyword: &str) -> Option<u32> {
    let pos = text.find(keyword)?;
    let rest = &text[pos + keyword.len()..];
    // skip if next char is alphabetic (avoid partial match like \min when looking for \mo)
    if let Some(c) = rest.chars().next() {
        if c.is_ascii_alphabetic() {
            return None;
        }
    }
    let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    num_str.parse().ok()
}

/// Simple RTF unescape: handles \', \\, \{, \}, named entities
fn unescape_rtf(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some(&nc) if nc == '{' || nc == '}' || nc == '\\' => {
                    result.push(nc);
                    chars.next();
                }
                Some(&'\'') => {
                    chars.next(); // consume '
                    let h1 = chars.next().unwrap_or('0');
                    let h2 = chars.next().unwrap_or('0');
                    let hex = format!("{}{}", h1, h2);
                    if let Ok(n) = u8::from_str_radix(&hex, 16) {
                        // Latin-1 character
                        let ch = char::from(n);
                        result.push(ch);
                    }
                }
                Some(&'n') => {
                    chars.next();
                    result.push('\n');
                }
                Some(&'t') => {
                    chars.next();
                    result.push('\t');
                }
                _ => {
                    // Skip control word
                    let mut word = String::new();
                    while let Some(&nc) = chars.peek() {
                        if nc.is_ascii_alphabetic() {
                            word.push(nc);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    // skip optional space after control word
                    if chars.peek() == Some(&' ') {
                        chars.next();
                    }
                    // skip optional digit sequence
                    let mut digits = String::new();
                    while let Some(&nc) = chars.peek() {
                        if nc.is_ascii_digit() || nc == '-' {
                            digits.push(nc);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    if !digits.is_empty() {
                        if chars.peek() == Some(&' ') {
                            chars.next();
                        }
                    }
                    // Handle unicode \uN
                    if word == "u" {
                        if let Ok(n) = digits.parse::<u32>() {
                            if let Some(ch) = char::from_u32(n) {
                                result.push(ch);
                            }
                        }
                    }
                    // For other control words (ldblquote, rdblquote, etc.), ignore them
                }
            }
        } else if c == '\n' || c == '\r' {
            // ignore bare line breaks
        } else if c != '{' && c != '}' {
            result.push(c);
        }
    }
    result.trim().to_string()
}

/// Convert RTF user property name to tag name (capitalize words, strip invalid chars)
fn rtf_prop_name(name: &str) -> String {
    // $tag =~ s/\s(.)/\U$1/g; $tag =~ tr/-_a-zA-Z0-9//dc;
    let mut result = String::new();
    let mut capitalize_next = false;
    for c in name.chars() {
        if c == ' ' {
            capitalize_next = true;
        } else if c == '-' || c == '_' || c.is_ascii_alphanumeric() {
            if capitalize_next {
                for uc in c.to_uppercase() {
                    result.push(uc);
                }
                capitalize_next = false;
            } else {
                result.push(c);
            }
        }
    }
    result
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "RTF".into(),
            family1: "RTF".into(),
            family2: "Document".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}
