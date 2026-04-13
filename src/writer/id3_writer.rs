//! ID3v2 tag writer for MP3 files.

use crate::error::Result;

pub fn write_id3(source: &[u8], changes: &[(&str, &str)]) -> Result<Vec<u8>> {
    if changes.is_empty() {
        return Ok(source.to_vec());
    }

    // Build new ID3v2.3 tag
    let mut frames = Vec::new();
    for &(tag, value) in changes {
        if let Some(frame_id) = tag_to_frame_id(tag) {
            let mut frame_data = Vec::new();
            frame_data.push(3); // UTF-8 encoding
            frame_data.extend_from_slice(value.as_bytes());

            // Frame header: ID(4) + size(4) + flags(2) + data
            frames.extend_from_slice(frame_id);
            frames.extend_from_slice(&(frame_data.len() as u32).to_be_bytes());
            frames.extend_from_slice(&[0, 0]); // flags
            frames.extend_from_slice(&frame_data);
        }
    }

    if frames.is_empty() {
        return Ok(source.to_vec());
    }

    let mut output = Vec::with_capacity(source.len() + frames.len() + 10);

    // ID3v2 header
    output.extend_from_slice(b"ID3");
    output.push(3); // version 2.3
    output.push(0); // revision
    output.push(0); // flags
                    // Sync-safe size
    let size = frames.len() as u32;
    output.push(((size >> 21) & 0x7F) as u8);
    output.push(((size >> 14) & 0x7F) as u8);
    output.push(((size >> 7) & 0x7F) as u8);
    output.push((size & 0x7F) as u8);
    output.extend_from_slice(&frames);

    // Skip existing ID3v2 tag if present
    let audio_start = if source.starts_with(b"ID3") && source.len() >= 10 {
        let old_size = (((source[6] as usize) << 21)
            | ((source[7] as usize) << 14)
            | ((source[8] as usize) << 7)
            | source[9] as usize)
            + 10;
        old_size.min(source.len())
    } else {
        0
    };

    output.extend_from_slice(&source[audio_start..]);
    Ok(output)
}

fn tag_to_frame_id(name: &str) -> Option<&'static [u8; 4]> {
    Some(match name.to_lowercase().as_str() {
        "title" => b"TIT2",
        "artist" => b"TPE1",
        "album" => b"TALB",
        "year" | "date" => b"TDRC",
        "track" => b"TRCK",
        "genre" => b"TCON",
        "comment" => b"COMM",
        "composer" => b"TCOM",
        "albumartist" => b"TPE2",
        "encoder" | "encodedby" => b"TENC",
        "publisher" => b"TPUB",
        "copyright" => b"TCOP",
        "bpm" => b"TBPM",
        "lyrics" => b"USLT",
        _ => return None,
    })
}
