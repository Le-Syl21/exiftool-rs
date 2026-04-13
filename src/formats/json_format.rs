//! JSON metadata format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

pub fn read_json(data: &[u8]) -> Result<Vec<Tag>> {
    let text = crate::encoding::decode_utf8_or_latin1(data);
    let trimmed = text.trim();
    if !trimmed.starts_with('{') && !trimmed.starts_with('[') {
        return Err(Error::InvalidData("not a JSON file".into()));
    }

    let mut tags = Vec::new();

    // Parse top-level JSON object fields
    if trimmed.starts_with('{') {
        let mut collected: Vec<(String, String)> = Vec::new();
        parse_json_object(trimmed, "", &mut collected);
        for (key, value) in collected {
            let tag_name = json_key_to_tag_name(&key);
            if tag_name.is_empty() {
                continue;
            }
            tags.push(mktag("JSON", &tag_name, &tag_name, Value::String(value)));
        }
    }

    Ok(tags)
}

/// Recursively parse a JSON object, collecting (flat_tag_name, value) pairs.
/// For nested objects, the key is prepended to nested keys.
fn parse_json_object(json: &str, prefix: &str, out: &mut Vec<(String, String)>) {
    let mut pos = 0;
    let chars: Vec<char> = json.chars().collect();

    // skip opening {
    if pos < chars.len() && chars[pos] == '{' {
        pos += 1;
    }

    loop {
        // skip whitespace and commas
        while pos < chars.len() && (chars[pos].is_whitespace() || chars[pos] == ',') {
            pos += 1;
        }
        if pos >= chars.len() || chars[pos] == '}' {
            break;
        }

        // read key
        if chars[pos] != '"' {
            break;
        }
        let key = read_json_string(&chars, &mut pos);

        // skip whitespace and colon
        while pos < chars.len() && (chars[pos].is_whitespace() || chars[pos] == ':') {
            pos += 1;
        }

        // read value
        if pos >= chars.len() {
            break;
        }

        let full_key = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{}{}", prefix, ucfirst_str(&key))
        };

        match chars[pos] {
            '"' => {
                let val = read_json_string(&chars, &mut pos);
                out.push((full_key, val));
            }
            '{' => {
                let obj_start = pos;
                let obj_end = find_matching_bracket(&chars, pos, '{', '}');
                let obj_str: String = chars[obj_start..obj_end + 1].iter().collect();
                // For objects, flatten with parent key as prefix
                parse_json_object(&obj_str, &full_key, out);
                pos = obj_end + 1;
            }
            '[' => {
                let arr_start = pos;
                let arr_end = find_matching_bracket(&chars, pos, '[', ']');
                let arr_str: String = chars[arr_start..arr_end + 1].iter().collect();
                // Check if array contains objects (array-of-objects flattening)
                if array_contains_objects(&arr_str) {
                    // Flatten: parse each object with parent key as prefix, accumulate per sub-key
                    let mut sub_map: Vec<(String, Vec<String>)> = Vec::new();
                    parse_json_array_of_objects(&arr_str, &full_key, &mut sub_map);
                    for (sub_key, vals) in sub_map {
                        if !vals.is_empty() {
                            out.push((sub_key, vals.join(", ")));
                        }
                    }
                } else {
                    let values = parse_json_array(&arr_str);
                    if !values.is_empty() {
                        out.push((full_key, values.join(", ")));
                    }
                }
                pos = arr_end + 1;
            }
            'n' => {
                // null
                pos += 4;
                out.push((full_key, "null".into()));
            }
            't' => {
                // true
                pos += 4;
                out.push((full_key, "1".into()));
            }
            'f' => {
                // false
                pos += 5;
                out.push((full_key, "0".into()));
            }
            _ => {
                // number
                let num_start = pos;
                while pos < chars.len()
                    && !chars[pos].is_whitespace()
                    && chars[pos] != ','
                    && chars[pos] != '}'
                {
                    pos += 1;
                }
                let num: String = chars[num_start..pos].iter().collect();
                out.push((full_key, num));
            }
        }
    }
}

fn parse_json_array(json: &str) -> Vec<String> {
    let mut results = Vec::new();
    let chars: Vec<char> = json.chars().collect();
    let mut pos = 0;

    if pos < chars.len() && chars[pos] == '[' {
        pos += 1;
    }

    loop {
        while pos < chars.len() && (chars[pos].is_whitespace() || chars[pos] == ',') {
            pos += 1;
        }
        if pos >= chars.len() || chars[pos] == ']' {
            break;
        }

        match chars[pos] {
            '"' => {
                let val = read_json_string(&chars, &mut pos);
                results.push(val);
            }
            '[' => {
                let end = find_matching_bracket(&chars, pos, '[', ']');
                let sub: String = chars[pos..end + 1].iter().collect();
                let sub_vals = parse_json_array(&sub);
                results.extend(sub_vals);
                pos = end + 1;
            }
            '{' => {
                let end = find_matching_bracket(&chars, pos, '{', '}');
                pos = end + 1;
            }
            'n' => {
                pos += 4;
                results.push("null".into());
            }
            't' => {
                pos += 4;
                results.push("1".into());
            }
            'f' => {
                pos += 5;
                results.push("0".into());
            }
            _ => {
                let start = pos;
                while pos < chars.len()
                    && !chars[pos].is_whitespace()
                    && chars[pos] != ','
                    && chars[pos] != ']'
                {
                    pos += 1;
                }
                results.push(chars[start..pos].iter().collect());
            }
        }
    }
    results
}

/// Returns true if the JSON array contains at least one object element.
fn array_contains_objects(json: &str) -> bool {
    let chars: Vec<char> = json.chars().collect();
    let mut pos = 0;
    if pos < chars.len() && chars[pos] == '[' {
        pos += 1;
    }
    while pos < chars.len() {
        if chars[pos].is_whitespace() || chars[pos] == ',' {
            pos += 1;
            continue;
        }
        if chars[pos] == ']' {
            break;
        }
        if chars[pos] == '{' {
            return true;
        }
        break;
    }
    false
}

/// Parse an array of objects, accumulating sub-fields per key.
/// sub_map: Vec<(sub_key, Vec<value>)> — ordered by first occurrence.
fn parse_json_array_of_objects(json: &str, prefix: &str, sub_map: &mut Vec<(String, Vec<String>)>) {
    let chars: Vec<char> = json.chars().collect();
    let mut pos = 0;
    if pos < chars.len() && chars[pos] == '[' {
        pos += 1;
    }

    loop {
        while pos < chars.len() && (chars[pos].is_whitespace() || chars[pos] == ',') {
            pos += 1;
        }
        if pos >= chars.len() || chars[pos] == ']' {
            break;
        }
        if chars[pos] == '{' {
            let end = find_matching_bracket(&chars, pos, '{', '}');
            let obj_str: String = chars[pos..end + 1].iter().collect();
            let mut obj_fields: Vec<(String, String)> = Vec::new();
            parse_json_object(&obj_str, prefix, &mut obj_fields);
            for (k, v) in obj_fields {
                if let Some(entry) = sub_map.iter_mut().find(|(sk, _)| sk == &k) {
                    // Append multiple values from nested arrays too
                    for part in v.split(", ") {
                        entry.1.push(part.to_string());
                    }
                } else {
                    let vals: Vec<String> = v.split(", ").map(|s| s.to_string()).collect();
                    sub_map.push((k, vals));
                }
            }
            pos = end + 1;
        } else {
            // Non-object element, skip
            while pos < chars.len() && chars[pos] != ',' && chars[pos] != ']' {
                pos += 1;
            }
        }
    }
}

fn read_json_string(chars: &[char], pos: &mut usize) -> String {
    if *pos >= chars.len() || chars[*pos] != '"' {
        return String::new();
    }
    *pos += 1; // skip opening "
    let mut result = String::new();
    while *pos < chars.len() && chars[*pos] != '"' {
        if chars[*pos] == '\\' && *pos + 1 < chars.len() {
            *pos += 1;
            match chars[*pos] {
                '"' => result.push('"'),
                '\\' => result.push('\\'),
                '/' => result.push('/'),
                'n' => result.push('\n'),
                'r' => result.push('\r'),
                't' => result.push('\t'),
                _ => result.push(chars[*pos]),
            }
        } else {
            result.push(chars[*pos]);
        }
        *pos += 1;
    }
    if *pos < chars.len() {
        *pos += 1;
    } // skip closing "
    result
}

fn find_matching_bracket(chars: &[char], start: usize, open: char, close: char) -> usize {
    let mut level = 0;
    let mut pos = start;
    let mut in_string = false;
    while pos < chars.len() {
        if chars[pos] == '"' && (pos == 0 || chars[pos - 1] != '\\') {
            in_string = !in_string;
        }
        if !in_string {
            if chars[pos] == open {
                level += 1;
            } else if chars[pos] == close {
                level -= 1;
                if level == 0 {
                    return pos;
                }
            }
        }
        pos += 1;
    }
    pos.saturating_sub(1)
}

fn ucfirst_str(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

/// Convert a JSON key (possibly nested like "testThis") to ExifTool tag name.
/// Mirrors Perl: ucfirst, then capitalize letters after non-alphabetic chars.
fn json_key_to_tag_name(key: &str) -> String {
    // ucfirst
    let key = ucfirst_str(key);
    // Capitalize after non-alpha: s/([^a-zA-Z])([a-z])/$1\U$2/g
    let mut result = String::new();
    let chars: Vec<char> = key.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        result.push(c);
        if !c.is_ascii_alphabetic() && i + 1 < chars.len() && chars[i + 1].is_ascii_lowercase() {
            let uc = chars[i + 1].to_ascii_uppercase();
            result.push(uc);
            i += 2;
            continue;
        }
        i += 1;
    }
    result
}
