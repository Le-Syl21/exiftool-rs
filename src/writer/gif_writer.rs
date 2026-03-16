//! GIF metadata writer — update/add comment extension blocks.

use crate::error::{Error, Result};

pub fn write_gif(source: &[u8], new_comment: Option<&str>) -> Result<Vec<u8>> {
    if source.len() < 13 || !source.starts_with(b"GIF8") {
        return Err(Error::InvalidData("not a GIF file".into()));
    }
    let mut output = Vec::with_capacity(source.len());
    let mut pos = 0;

    // Copy header (6 bytes) + screen descriptor (7 bytes)
    output.extend_from_slice(&source[..13]);
    pos = 13;

    // Skip global color table if present
    let packed = source[10];
    let has_gct = (packed & 0x80) != 0;
    if has_gct {
        let gct_size = 3 * (1 << ((packed & 0x07) + 1));
        output.extend_from_slice(&source[pos..pos + gct_size]);
        pos += gct_size;
    }

    // Insert new comment before first image/extension
    if let Some(comment) = new_comment {
        output.push(0x21); // Extension introducer
        output.push(0xFE); // Comment label
        let bytes = comment.as_bytes();
        for chunk in bytes.chunks(255) {
            output.push(chunk.len() as u8);
            output.extend_from_slice(chunk);
        }
        output.push(0); // Block terminator
    }

    // Copy rest of file, removing existing comments if replacing
    while pos < source.len() {
        match source[pos] {
            0x21 if pos + 1 < source.len() && source[pos + 1] == 0xFE && new_comment.is_some() => {
                // Skip existing comment block
                pos += 2;
                while pos < source.len() {
                    let block_size = source[pos] as usize;
                    pos += 1;
                    if block_size == 0 { break; }
                    pos += block_size;
                }
            }
            _ => {
                output.push(source[pos]);
                pos += 1;
            }
        }
    }
    Ok(output)
}
