//! FLAC metadata writer — update Vorbis comments block.

use crate::error::{Error, Result};

pub fn write_flac(source: &[u8], changes: &[(&str, &str)]) -> Result<Vec<u8>> {
    if source.len() < 8 || !source.starts_with(b"fLaC") {
        return Err(Error::InvalidData("not a FLAC file".into()));
    }
    let mut output = Vec::with_capacity(source.len());
    output.extend_from_slice(b"fLaC");
    let mut pos = 4;
    let mut _wrote_comments = false;

    loop {
        if pos + 4 > source.len() { break; }
        let header = source[pos];
        let is_last = (header & 0x80) != 0;
        let block_type = header & 0x7F;
        let block_size = ((source[pos+1] as usize) << 16) | ((source[pos+2] as usize) << 8) | source[pos+3] as usize;
        pos += 4;
        if pos + block_size > source.len() { break; }

        if block_type == 4 && !changes.is_empty() {
            // Replace Vorbis comment block
            let new_block = build_vorbis_comments(&source[pos..pos+block_size], changes);
            let new_header = if is_last { 0x84 } else { 0x04 };
            output.push(new_header);
            output.push(((new_block.len() >> 16) & 0xFF) as u8);
            output.push(((new_block.len() >> 8) & 0xFF) as u8);
            output.push((new_block.len() & 0xFF) as u8);
            output.extend_from_slice(&new_block);
            _wrote_comments = true;
        } else {
            output.push(header);
            output.push(((block_size >> 16) & 0xFF) as u8);
            output.push(((block_size >> 8) & 0xFF) as u8);
            output.push((block_size & 0xFF) as u8);
            output.extend_from_slice(&source[pos..pos+block_size]);
        }
        pos += block_size;
        if is_last { break; }
    }

    // Append remaining data (audio frames)
    if pos < source.len() {
        output.extend_from_slice(&source[pos..]);
    }

    Ok(output)
}

fn build_vorbis_comments(existing: &[u8], changes: &[(&str, &str)]) -> Vec<u8> {
    let mut comments: Vec<(String, String)> = Vec::new();

    // Parse existing comments
    if existing.len() >= 8 {
        let vendor_len = u32::from_le_bytes([existing[0], existing[1], existing[2], existing[3]]) as usize;
        let mut p = 4 + vendor_len;
        if p + 4 <= existing.len() {
            let num = u32::from_le_bytes([existing[p], existing[p+1], existing[p+2], existing[p+3]]);
            p += 4;
            for _ in 0..num {
                if p + 4 > existing.len() { break; }
                let clen = u32::from_le_bytes([existing[p], existing[p+1], existing[p+2], existing[p+3]]) as usize;
                p += 4;
                if p + clen > existing.len() { break; }
                let comment = String::from_utf8_lossy(&existing[p..p+clen]).to_string();
                if let Some(eq) = comment.find('=') {
                    comments.push((comment[..eq].to_string(), comment[eq+1..].to_string()));
                }
                p += clen;
            }
        }
    }

    // Apply changes
    for &(key, value) in changes {
        let upper = key.to_uppercase();
        if let Some(existing) = comments.iter_mut().find(|(k, _)| k.to_uppercase() == upper) {
            existing.1 = value.to_string();
        } else {
            comments.push((key.to_uppercase(), value.to_string()));
        }
    }

    // Build output
    let mut out = Vec::new();
    let vendor = b"exiftool-rs";
    out.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    out.extend_from_slice(vendor);
    out.extend_from_slice(&(comments.len() as u32).to_le_bytes());
    for (k, v) in &comments {
        let comment = format!("{}={}", k, v);
        let bytes = comment.as_bytes();
        out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(bytes);
    }
    out
}
