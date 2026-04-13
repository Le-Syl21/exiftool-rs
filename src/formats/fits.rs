//! FITS (Flexible Image Transport System) format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

pub fn read_fits(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 80 || !data.starts_with(b"SIMPLE  =") {
        return Err(Error::InvalidData("not a FITS file".into()));
    }

    let mut tags = Vec::new();
    // FITS header: 80-byte fixed-width keyword records
    let mut pos = 0;
    // For CONTINUE: track current tag to append value
    let mut continue_tag: Option<String> = None;
    let mut continue_val: String = String::new();

    while pos + 80 <= data.len() {
        let record = &data[pos..pos + 80];
        let keyword = crate::encoding::decode_utf8_or_latin1(&record[..8])
            .trim_end()
            .to_string();
        pos += 80;

        if keyword == "END" {
            break;
        }

        // Handle CONTINUE keyword
        if keyword == "CONTINUE" {
            if continue_tag.is_some() {
                // Continue value from previous quoted string
                let val_raw = crate::encoding::decode_utf8_or_latin1(&record[8..]).to_string();
                let (more, cont) = fits_parse_continued_value(&val_raw);
                continue_val.push_str(&more);
                if !cont {
                    let tag_name = continue_tag.take().unwrap();
                    let tag_desc = fits_tag_description(&tag_name);
                    tags.push(mktag(
                        "FITS",
                        &tag_name,
                        &tag_desc,
                        Value::String(continue_val.clone()),
                    ));
                    continue_val.clear();
                }
            }
            continue;
        }

        // Flush any pending continue
        if let Some(tag_name) = continue_tag.take() {
            let tag_desc = fits_tag_description(&tag_name);
            tags.push(mktag(
                "FITS",
                &tag_name,
                &tag_desc,
                Value::String(continue_val.clone()),
            ));
            continue_val.clear();
        }

        // COMMENT and HISTORY: special handling (no '= ' at position 8)
        if keyword == "COMMENT" || keyword == "HISTORY" {
            let val = crate::encoding::decode_utf8_or_latin1(&record[8..])
                .trim_end()
                .to_string();
            let name = if keyword == "COMMENT" {
                "Comment"
            } else {
                "History"
            };
            tags.push(mktag("FITS", name, name, Value::String(val)));
            continue;
        }

        // Standard keyword = value
        if keyword.is_empty() {
            continue;
        }
        if record.len() <= 10 || record[8] != b'=' {
            continue;
        }

        let val_raw = crate::encoding::decode_utf8_or_latin1(&record[10..]).to_string();
        // Parse value: may be quoted string, boolean T/F, or number
        let (value, is_continued) = fits_parse_value(&val_raw);
        if value.is_empty() {
            continue;
        }

        // Map known keywords, generate names for others
        let tag_name = fits_keyword_to_name(&keyword);
        let tag_desc = fits_tag_description(&tag_name);

        if is_continued {
            continue_tag = Some(tag_name);
            continue_val = value;
        } else {
            tags.push(mktag("FITS", &tag_name, &tag_desc, Value::String(value)));
        }
    }

    // Flush pending continue
    if let Some(tag_name) = continue_tag.take() {
        let tag_desc = fits_tag_description(&tag_name);
        tags.push(mktag(
            "FITS",
            &tag_name,
            &tag_desc,
            Value::String(continue_val.clone()),
        ));
    }

    Ok(tags)
}

/// Parse a FITS value field (columns 11-80 of an 80-char record).
/// Returns (value_string, is_continued) where is_continued means value ends with '&'.
fn fits_parse_value(s: &str) -> (String, bool) {
    let s = s.trim_start();
    if let Some(inner) = s.strip_prefix('\'') {
        // Quoted string: parse until closing quote (doubled quotes are escaped)
        let mut result = String::new();
        let mut chars = inner.chars().peekable();
        loop {
            match chars.next() {
                None => break,
                Some('\'') => {
                    if chars.peek() == Some(&'\'') {
                        // Escaped quote
                        chars.next();
                        result.push('\'');
                    } else {
                        break; // End of string
                    }
                }
                Some(c) => result.push(c),
            }
        }
        // Trim trailing spaces from quoted string
        let trimmed = result.trim_end().to_string();
        let is_cont = trimmed.ends_with('&');
        let val = if is_cont {
            trimmed[..trimmed.len() - 1].to_string()
        } else {
            trimmed
        };
        (val, is_cont)
    } else {
        // Non-quoted: take everything up to comment marker /
        // Remove trailing spaces and comment
        let val = s.split('/').next().unwrap_or("").trim().to_string();
        // Re-format float exponents: D/E -> e
        let val = val.replace(['D', 'E'], "e");
        if val.is_empty() {
            return (String::new(), false);
        }
        (val, false)
    }
}

/// Parse a FITS CONTINUE value (same format as normal value but starting at column 9).
fn fits_parse_continued_value(s: &str) -> (String, bool) {
    fits_parse_value(s)
}

/// Convert a FITS keyword to a tag name (ExifTool naming convention).
/// Known keywords get special names; others get generated from keyword.
fn fits_keyword_to_name(keyword: &str) -> String {
    match keyword {
        "SIMPLE" => String::new(), // Perl internal only
        "BITPIX" => "Bitpix".into(),
        "NAXIS" => "Naxis".into(),
        "NAXIS1" => "Naxis1".into(),
        "NAXIS2" => "Naxis2".into(),
        "EXTEND" => "Extend".into(),
        "ORIGIN" => "Origin".into(),
        "TELESCOP" => "Telescope".into(),
        "BACKGRND" => "Background".into(),
        "INSTRUME" => "Instrument".into(),
        "OBJECT" => "Object".into(),
        "OBSERVER" => "Observer".into(),
        "DATE" => "CreateDate".into(),
        "AUTHOR" => "Creator".into(),
        "REFERENC" => "Reference".into(),
        "DATE-OBS" => "ObservationDate".into(),
        "TIME-OBS" => "ObservationTime".into(),
        "DATE-END" => "ObservationDateEnd".into(),
        "TIME-END" => "ObservationTimeEnd".into(),
        "COMMENT" => "Comment".into(),
        "HISTORY" => "History".into(),
        _ => {
            // Generate name: ucfirst lc tag, remove underscores and capitalize next
            let lower = keyword.to_lowercase();
            let mut result = String::new();
            let mut capitalize_next = true;
            for ch in lower.chars() {
                if ch == '_' || ch == '-' {
                    capitalize_next = true;
                } else if capitalize_next {
                    for c in ch.to_uppercase() {
                        result.push(c);
                    }
                    capitalize_next = false;
                } else {
                    result.push(ch);
                }
            }
            result
        }
    }
}

fn fits_tag_description(name: &str) -> String {
    // Generate description by inserting spaces before capitals
    let mut desc = String::new();
    for ch in name.chars() {
        if ch.is_uppercase() && !desc.is_empty() {
            desc.push(' ');
        }
        desc.push(ch);
    }
    desc
}
