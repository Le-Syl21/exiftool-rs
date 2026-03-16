//! PDF metadata writer.
//!
//! Updates PDF Info dictionary values in-place.
//! For XMP, appends a new metadata stream.

use crate::error::{Error, Result};

/// Rewrite a PDF file with updated Info dictionary values.
pub fn write_pdf(
    source: &[u8],
    changes: &[(&str, &str)], // (key, value) pairs for Info dict
) -> Result<Vec<u8>> {
    if !source.starts_with(b"%PDF-") {
        return Err(Error::InvalidData("not a PDF file".into()));
    }

    let mut output = source.to_vec();

    for &(key, value) in changes {
        let pdf_key = match key.to_lowercase().as_str() {
            "title" => "/Title",
            "author" => "/Author",
            "subject" => "/Subject",
            "keywords" => "/Keywords",
            "creator" => "/Creator",
            "producer" => "/Producer",
            _ => continue,
        };

        // Find and replace the value in the Info dictionary
        if let Some(pos) = find_pdf_key(&output, pdf_key) {
            replace_pdf_value(&mut output, pos + pdf_key.len(), value);
        }
    }

    Ok(output)
}

/// Find a PDF key in the file (e.g., "/Title").
fn find_pdf_key(data: &[u8], key: &str) -> Option<usize> {
    let key_bytes = key.as_bytes();
    data.windows(key_bytes.len())
        .position(|w| w == key_bytes)
}

/// Replace a PDF string value after the key position.
fn replace_pdf_value(data: &mut Vec<u8>, after_key: usize, new_value: &str) {
    let rest = &data[after_key..];

    // Skip whitespace
    let mut pos = 0;
    while pos < rest.len() && (rest[pos] == b' ' || rest[pos] == b'\r' || rest[pos] == b'\n') {
        pos += 1;
    }

    if pos >= rest.len() {
        return;
    }

    let abs_start = after_key + pos;

    // Determine string type and find end
    if rest[pos] == b'(' {
        // Literal string: find matching ')'
        let mut depth = 1;
        let mut end = pos + 1;
        while end < rest.len() && depth > 0 {
            match rest[end] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                b'\\' => { end += 1; } // skip escaped char
                _ => {}
            }
            end += 1;
        }
        let abs_end = after_key + end;

        // Build replacement
        let escaped = new_value
            .replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)");
        let replacement = format!("({})", escaped);

        // Replace in-place
        let old_len = abs_end - abs_start;
        let new_bytes = replacement.as_bytes();

        if new_bytes.len() == old_len {
            data[abs_start..abs_end].copy_from_slice(new_bytes);
        } else {
            // Need to splice
            let mut new_data = Vec::with_capacity(data.len() + new_bytes.len());
            new_data.extend_from_slice(&data[..abs_start]);
            new_data.extend_from_slice(new_bytes);
            new_data.extend_from_slice(&data[abs_end..]);
            *data = new_data;
        }
    } else if rest[pos] == b'<' {
        // Hex string: find '>'
        let end = rest[pos..].iter().position(|&b| b == b'>').unwrap_or(rest.len()) + pos + 1;
        let abs_end = after_key + end;

        // Replace with literal string
        let escaped = new_value
            .replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)");
        let replacement = format!("({})", escaped);

        let mut new_data = Vec::with_capacity(data.len());
        new_data.extend_from_slice(&data[..abs_start]);
        new_data.extend_from_slice(replacement.as_bytes());
        new_data.extend_from_slice(&data[abs_end..]);
        *data = new_data;
    }
}
