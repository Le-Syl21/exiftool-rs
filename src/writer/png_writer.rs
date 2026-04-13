//! PNG metadata writer.
//!
//! Rewrites PNG files with updated/added/removed text chunks and eXIf.

use crate::error::{Error, Result};

/// Rewrite a PNG file with updated metadata.
///
/// `new_text` - list of (keyword, value) pairs to add as tEXt chunks.
/// `new_exif` - TIFF/EXIF data to write as eXIf chunk, or None.
/// `remove_text` - keywords to remove from existing tEXt/iTXt chunks.
pub fn write_png(
    source: &[u8],
    new_text: &[(&str, &str)],
    new_exif: Option<&[u8]>,
    remove_text: &[&str],
) -> Result<Vec<u8>> {
    let sig = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    if source.len() < 8 || !source.starts_with(sig) {
        return Err(Error::InvalidData("not a PNG file".into()));
    }

    let mut output = Vec::with_capacity(source.len());
    output.extend_from_slice(sig);

    let mut pos = 8;
    let mut wrote_new_chunks = false;

    while pos + 12 <= source.len() {
        let chunk_len = u32::from_be_bytes([
            source[pos],
            source[pos + 1],
            source[pos + 2],
            source[pos + 3],
        ]) as usize;
        let chunk_type = &source[pos + 4..pos + 8];
        let chunk_end = pos + 8 + chunk_len + 4; // +4 for CRC

        if chunk_end > source.len() {
            // Copy remainder as-is
            output.extend_from_slice(&source[pos..]);
            break;
        }

        let chunk_data = &source[pos + 8..pos + 8 + chunk_len];

        // Before IDAT or IEND, insert new metadata chunks
        if !wrote_new_chunks && (chunk_type == b"IDAT" || chunk_type == b"IEND") {
            wrote_new_chunks = true;

            // Write new tEXt chunks
            for (key, value) in new_text {
                write_text_chunk(&mut output, key, value);
            }

            // Write eXIf chunk
            if let Some(exif_data) = new_exif {
                write_chunk(&mut output, b"eXIf", exif_data);
            }
        }

        // Filter existing chunks
        match chunk_type {
            b"tEXt" | b"iTXt" | b"zTXt" => {
                // Check if this text chunk should be removed
                let null_pos = chunk_data
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(chunk_data.len());
                let keyword = crate::encoding::decode_latin1(&chunk_data[..null_pos]);

                if remove_text.contains(&keyword.as_str()) {
                    pos = chunk_end;
                    continue; // Skip this chunk
                }

                // Check if we're replacing this keyword
                if new_text.iter().any(|(k, _)| *k == keyword.as_str()) {
                    pos = chunk_end;
                    continue; // Skip (will be replaced by new chunk above)
                }

                // Keep existing chunk
                output.extend_from_slice(&source[pos..chunk_end]);
            }
            b"eXIf" => {
                // Replace with new EXIF if provided
                if new_exif.is_some() {
                    pos = chunk_end;
                    continue; // Skip (replaced above)
                }
                output.extend_from_slice(&source[pos..chunk_end]);
            }
            _ => {
                // Keep all other chunks as-is
                output.extend_from_slice(&source[pos..chunk_end]);
            }
        }

        pos = chunk_end;
    }

    Ok(output)
}

/// Write a tEXt chunk.
fn write_text_chunk(output: &mut Vec<u8>, keyword: &str, value: &str) {
    let mut data = Vec::new();
    data.extend_from_slice(keyword.as_bytes());
    data.push(0); // null separator
    data.extend_from_slice(value.as_bytes());

    write_chunk(output, b"tEXt", &data);
}

/// Write a PNG chunk with correct length and CRC.
fn write_chunk(output: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    let len = data.len() as u32;
    output.extend_from_slice(&len.to_be_bytes());
    output.extend_from_slice(chunk_type);
    output.extend_from_slice(data);

    // CRC-32 over chunk_type + data
    let crc = crc32(chunk_type, data);
    output.extend_from_slice(&crc.to_be_bytes());
}

/// Calculate PNG CRC-32 (ISO 3309).
fn crc32(chunk_type: &[u8], data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;

    for &byte in chunk_type.iter().chain(data.iter()) {
        let index = ((crc ^ byte as u32) & 0xFF) as usize;
        crc = CRC_TABLE[index] ^ (crc >> 8);
    }

    crc ^ 0xFFFFFFFF
}

/// Pre-computed CRC-32 lookup table.
static CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            if c & 1 != 0 {
                c = 0xEDB88320 ^ (c >> 1);
            } else {
                c >>= 1;
            }
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
};
