//! PostScript/EPS metadata writer.
//! Modifies DSC comments in-place.

use crate::error::{Error, Result};

pub fn write_postscript(source: &[u8], changes: &[(&str, &str)]) -> Result<Vec<u8>> {
    let mut offset = 0;

    // DOS EPS binary header
    if source.starts_with(&[0xC5, 0xD0, 0xD3, 0xC6]) && source.len() >= 12 {
        offset = u32::from_le_bytes([source[4], source[5], source[6], source[7]]) as usize;
    }

    if offset + 4 > source.len()
        || (!source[offset..].starts_with(b"%!PS") && !source[offset..].starts_with(b"%!Ad"))
    {
        return Err(Error::InvalidData("not a PostScript file".into()));
    }

    let text = crate::encoding::decode_utf8_or_latin1(source);
    let mut result = text.to_string();

    for &(key, value) in changes {
        let dsc_key = match key.to_lowercase().as_str() {
            "title" => "%%Title",
            "creator" => "%%Creator",
            "author" | "for" => "%%For",
            "creationdate" | "createdate" => "%%CreationDate",
            _ => continue,
        };

        // Find and replace existing DSC comment
        if let Some(pos) = result.find(dsc_key) {
            let line_end = result[pos..].find('\n').unwrap_or(result.len() - pos);
            let old_line = &result[pos..pos + line_end];
            let new_line = format!("{}: ({})", dsc_key, value);
            result = result.replacen(old_line, &new_line, 1);
        }
    }

    Ok(result.into_bytes())
}
