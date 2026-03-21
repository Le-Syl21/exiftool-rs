//! Matroska/WebM (MKV) file format reader.
//!
//! Parses EBML elements to extract metadata from MKV/WebM containers.
//! Mirrors ExifTool's Matroska.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_matroska(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 4 || !data.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Err(Error::InvalidData("not a Matroska/EBML file".into()));
    }

    let mut tags = Vec::new();
    let mut pos = 0;

    // Parse EBML header
    let (_header_id, header_size, header_hdr_len) = read_element_header(data, pos)?;
    pos += header_hdr_len;
    let header_end = pos + header_size;

    // Parse elements inside EBML header
    while pos < header_end && pos < data.len() {
        let (id, size, hdr_len) = match read_element_header(data, pos) {
            Ok(v) => v,
            Err(_) => break,
        };
        pos += hdr_len;

        if pos + size > data.len() {
            break;
        }

        match id {
            0x4286 => {
                // EBMLVersion
                let v = read_uint(data, pos, size);
                tags.push(mk("EBMLVersion", "EBML Version", Value::U32(v as u32)));
            }
            0x4282 => {
                // DocType
                let s = read_string(data, pos, size);
                tags.push(mk("DocType", "Document Type", Value::String(s)));
            }
            0x4287 => {
                // DocTypeVersion
                let v = read_uint(data, pos, size);
                tags.push(mk("DocTypeVersion", "Document Type Version", Value::U32(v as u32)));
            }
            0x4285 => {
                // DocTypeReadVersion
                let v = read_uint(data, pos, size);
                tags.push(mk("DocTypeReadVersion", "Doc Type Read Version", Value::U32(v as u32)));
            }
            _ => {}
        }
        pos += size;
    }

    // Find Segment element
    pos = header_end;
    if pos + 4 > data.len() {
        return Ok(tags);
    }

    let (seg_id, seg_size, seg_hdr_len) = match read_element_header(data, pos) {
        Ok(v) => v,
        Err(_) => return Ok(tags),
    };

    if seg_id != 0x18538067 {
        return Ok(tags);
    }

    pos += seg_hdr_len;
    let seg_end = (pos + seg_size).min(data.len());

    // Parse top-level Segment children
    while pos < seg_end {
        let (id, size, hdr_len) = match read_element_header(data, pos) {
            Ok(v) => v,
            Err(_) => break,
        };
        pos += hdr_len;

        if pos + size > seg_end {
            break;
        }

        match id {
            0x1549A966 => parse_info(data, pos, pos + size, &mut tags),    // Info
            0x1654AE6B => parse_tracks(data, pos, pos + size, &mut tags),  // Tracks
            0x1254C367 => parse_tags(data, pos, pos + size, &mut tags),    // Tags
            0x1043A770 => parse_chapters(data, pos, pos + size, &mut tags), // Chapters
            0x1F43B675 => break, // Cluster - stop here (actual media data)
            _ => {}
        }

        pos += size;
    }

    Ok(tags)
}

fn parse_info(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    let mut pos = start;
    let mut timecode_scale: u64 = 1_000_000; // default 1ms

    while pos < end {
        let (id, size, hdr_len) = match read_element_header(data, pos) {
            Ok(v) => v,
            Err(_) => break,
        };
        pos += hdr_len;
        if pos + size > end { break; }

        match id {
            0x2AD7B1 => {
                // TimecodeScale
                timecode_scale = read_uint(data, pos, size);
                tags.push(mk("TimecodeScale", "Timecode Scale", Value::String(format!("{} ns", timecode_scale))));
            }
            0x4489 => {
                // Duration (float)
                let dur = read_float(data, pos, size);
                let dur_secs = dur * timecode_scale as f64 / 1e9;
                tags.push(mk("Duration", "Duration", Value::String(format_duration(dur_secs))));
            }
            0x4461 => {
                // DateUTC (signed int, nanoseconds since 2001-01-01)
                let ns = read_int(data, pos, size);
                let unix_secs = ns / 1_000_000_000 + 978307200; // 2001-01-01 epoch to Unix epoch
                tags.push(mk("DateTimeOriginal", "Date/Time Original", Value::String(format!("{}", unix_secs))));
            }
            0x7BA9 => {
                // Title
                let s = read_string(data, pos, size);
                tags.push(mk("Title", "Title", Value::String(s)));
            }
            0x4D80 => {
                // MuxingApp
                let s = read_string(data, pos, size);
                tags.push(mk("MuxingApp", "Muxing Application", Value::String(s)));
            }
            0x5741 => {
                // WritingApp
                let s = read_string(data, pos, size);
                tags.push(mk("WritingApp", "Writing Application", Value::String(s)));
            }
            _ => {}
        }
        pos += size;
    }
}

fn parse_tracks(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    let mut pos = start;

    while pos < end {
        let (id, size, hdr_len) = match read_element_header(data, pos) {
            Ok(v) => v,
            Err(_) => break,
        };
        pos += hdr_len;
        if pos + size > end { break; }

        if id == 0xAE {
            // TrackEntry
            parse_track_entry(data, pos, pos + size, tags);
        }

        pos += size;
    }
}

fn parse_track_entry(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    let mut pos = start;
    let mut track_type: u64 = 0;

    // First pass: find TrackType so we can prefix CodecID correctly
    {
        let mut scan = start;
        while scan < end {
            let (id, size, hdr_len) = match read_element_header(data, scan) { Ok(v) => v, Err(_) => break };
            scan += hdr_len;
            if scan + size > end { break; }
            if id == 0x83 { track_type = read_uint(data, scan, size); }
            scan += size;
        }
    }

    while pos < end {
        let (id, size, hdr_len) = match read_element_header(data, pos) {
            Ok(v) => v,
            Err(_) => break,
        };
        pos += hdr_len;
        if pos + size > end { break; }

        match id {
            0xD7 => { // TrackNumber (Perl 0x57 → raw 0xD7)
                let v = read_uint(data, pos, size);
                tags.push(mk("TrackNumber", "Track Number", Value::U32(v as u32)));
            }
            0x73C5 => { // TrackUID (Perl 0x33C5 → raw 0x73C5)
                let v = read_uint(data, pos, size);
                tags.push(mk("TrackUID", "Track UID", Value::String(format!("{:08x}", v))));
            }
            0x83 => {
                // TrackType (Perl 0x03 → raw 0x83)
                let type_str = match track_type {
                    1 => "Video", 2 => "Audio", 3 => "Complex",
                    0x10 => "Logo", 0x11 => "Subtitle", 0x12 => "Buttons", 0x20 => "Control",
                    _ => "Unknown",
                };
                tags.push(mk("TrackType", "Track Type", Value::String(type_str.into())));
            }
            0xB9 => { // TrackUsed/FlagEnabled (Perl 0x39 → raw 0xB9)
                let v = read_uint(data, pos, size);
                tags.push(mk("TrackUsed", "Track Used", Value::String(if v != 0 { "Yes" } else { "No" }.into())));
            }
            0x88 => { // TrackDefault/FlagDefault (Perl 0x08 → raw 0x88)
                let v = read_uint(data, pos, size);
                tags.push(mk("TrackDefault", "Track Default", Value::String(if v != 0 { "Yes" } else { "No" }.into())));
            }
            0x55AA => { // TrackForced/FlagForced (Perl 0x15AA → raw 0x55AA)
                let v = read_uint(data, pos, size);
                tags.push(mk("TrackForced", "Track Forced", Value::String(if v != 0 { "Yes" } else { "No" }.into())));
            }
            0x23314F => { // TrackTimecodeScale (Perl 0x3314F → raw 0x23314F)
                let v = read_float(data, pos, size);
                tags.push(mk("TrackTimecodeScale", "Track Timecode Scale", Value::String(format!("{}", v))));
            }
            0xAA => { // CodecDecodeAll (Perl 0x2A → raw 0xAA)
                let v = read_uint(data, pos, size);
                tags.push(mk("CodecDecodeAll", "Codec Decode All", Value::String(if v != 0 { "Yes" } else { "No" }.into())));
            }
            0x23E383 => { // DefaultDuration (ns)
                let v = read_uint(data, pos, size);
                let ms = v / 1_000_000;
                tags.push(mk("DefaultDuration", "Default Duration", Value::String(format!("{} ms", ms))));
                // VideoFrameRate = 1e9 / DefaultDuration
                if v > 0 && track_type == 1 {
                    let fps = 1_000_000_000.0 / v as f64;
                    tags.push(mk("VideoFrameRate", "Video Frame Rate", Value::String(format!("{:.0}", fps))));
                }
            }
            0x86 => {
                // CodecID — prefixed with Video/Audio based on TrackType
                let s = read_string(data, pos, size);
                let name = match track_type {
                    1 => "VideoCodecID",
                    2 => "AudioCodecID",
                    _ => "CodecID",
                };
                tags.push(mk(name, name, Value::String(s)));
            }
            0x258688 => {
                let s = read_string(data, pos, size);
                tags.push(mk("CodecName", "Codec Name", Value::String(s)));
            }
            0x536E => {
                let s = read_string(data, pos, size);
                tags.push(mk("TrackName", "Track Name", Value::String(s)));
            }
            0x22B59C => {
                let s = read_string(data, pos, size);
                tags.push(mk("TrackLanguage", "Track Language", Value::String(s)));
            }
            0xE0 => {
                parse_video_settings(data, pos, pos + size, tags);
            }
            0xE1 => {
                parse_audio_settings(data, pos, pos + size, tags);
            }
            _ => {}
        }
        pos += size;
    }
}

fn parse_video_settings(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    let mut pos = start;

    while pos < end {
        let (id, size, hdr_len) = match read_element_header(data, pos) {
            Ok(v) => v,
            Err(_) => break,
        };
        pos += hdr_len;
        if pos + size > end { break; }

        match id {
            0xB0 => {
                let v = read_uint(data, pos, size);
                tags.push(mk("ImageWidth", "Image Width", Value::U32(v as u32)));
            }
            0xBA => {
                let v = read_uint(data, pos, size);
                tags.push(mk("ImageHeight", "Image Height", Value::U32(v as u32)));
            }
            0x54B0 => {
                let v = read_uint(data, pos, size);
                tags.push(mk("DisplayWidth", "Display Width", Value::U32(v as u32)));
            }
            0x54BA => {
                let v = read_uint(data, pos, size);
                tags.push(mk("DisplayHeight", "Display Height", Value::U32(v as u32)));
            }
            0x9A => {
                // VideoScanType / FlagInterlaced (Perl 0x1A → raw 0x9A)
                let v = read_uint(data, pos, size);
                let s = match v {
                    0 => "Undetermined",
                    1 => "Interlaced",
                    2 => "Progressive",
                    _ => "Unknown",
                };
                tags.push(mk("VideoScanType", "Video Scan Type", Value::String(s.into())));
            }
            _ => {}
        }
        pos += size;
    }
}

fn parse_audio_settings(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    let mut pos = start;

    while pos < end {
        let (id, size, hdr_len) = match read_element_header(data, pos) {
            Ok(v) => v,
            Err(_) => break,
        };
        pos += hdr_len;
        if pos + size > end { break; }

        match id {
            0xB5 => {
                let v = read_float(data, pos, size);
                tags.push(mk("AudioSampleRate", "Audio Sample Rate", Value::U32(v as u32)));
            }
            0x9F => {
                let v = read_uint(data, pos, size);
                tags.push(mk("AudioChannels", "Audio Channels", Value::U32(v as u32)));
            }
            0x6264 => {
                let v = read_uint(data, pos, size);
                tags.push(mk("AudioBitsPerSample", "Audio Bits Per Sample", Value::U32(v as u32)));
            }
            _ => {}
        }
        pos += size;
    }
}

fn parse_tags(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    let mut pos = start;

    while pos < end {
        let (id, size, hdr_len) = match read_element_header(data, pos) {
            Ok(v) => v,
            Err(_) => break,
        };
        pos += hdr_len;
        if pos + size > end { break; }

        if id == 0x7373 {
            // Tag element → contains SimpleTag children
            parse_tag_element(data, pos, pos + size, tags);
        }

        pos += size;
    }
}

fn parse_tag_element(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    let mut pos = start;

    while pos < end {
        let (id, size, hdr_len) = match read_element_header(data, pos) {
            Ok(v) => v,
            Err(_) => break,
        };
        pos += hdr_len;
        if pos + size > end { break; }

        if id == 0x67C8 {
            // SimpleTag
            parse_simple_tag(data, pos, pos + size, tags);
        }

        pos += size;
    }
}

fn parse_simple_tag(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    let mut pos = start;
    let mut tag_name = String::new();
    let mut tag_string = String::new();

    while pos < end {
        let (id, size, hdr_len) = match read_element_header(data, pos) {
            Ok(v) => v,
            Err(_) => break,
        };
        pos += hdr_len;
        if pos + size > end { break; }

        match id {
            0x45A3 => tag_name = read_string(data, pos, size),    // TagName
            0x4487 => tag_string = read_string(data, pos, size),  // TagString
            _ => {}
        }
        pos += size;
    }

    if !tag_name.is_empty() && !tag_string.is_empty() {
        tags.push(mk(&tag_name, &tag_name, Value::String(tag_string)));
    }
}

fn parse_chapters(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    // Just count chapters
    let mut pos = start;
    let mut chapter_count = 0u32;

    while pos < end {
        let (id, size, hdr_len) = match read_element_header(data, pos) {
            Ok(v) => v,
            Err(_) => break,
        };
        pos += hdr_len;
        if pos + size > end { break; }

        if id == 0xB6 {
            // ChapterAtom
            chapter_count += 1;
        }

        pos += size;
    }

    if chapter_count > 0 {
        tags.push(mk("ChapterCount", "Chapter Count", Value::U32(chapter_count)));
    }
}

// ============================================================================
// EBML VInt and value readers
// ============================================================================

/// Read EBML element header: returns (element_id, data_size, header_byte_count).
fn read_element_header(data: &[u8], pos: usize) -> Result<(u32, usize, usize)> {
    if pos >= data.len() {
        return Err(Error::InvalidData("unexpected end of EBML data".into()));
    }

    // Read element ID (variable length)
    let (id, id_len) = read_vint_raw(data, pos)?;

    // Read element size (variable length)
    let (size, size_len) = read_vint(data, pos + id_len)?;

    Ok((id as u32, size as usize, id_len + size_len))
}

/// Read a variable-length integer (EBML VInt) - returns value with leading bits masked.
fn read_vint(data: &[u8], pos: usize) -> Result<(u64, usize)> {
    if pos >= data.len() {
        return Err(Error::InvalidData("unexpected end of EBML data".into()));
    }

    let first = data[pos];
    if first == 0 {
        return Err(Error::InvalidData("invalid EBML VInt".into()));
    }

    let len = first.leading_zeros() as usize + 1;
    if pos + len > data.len() || len > 8 {
        return Err(Error::InvalidData("EBML VInt exceeds data".into()));
    }

    let mut value = (first as u64) & ((1 << (8 - len)) - 1);
    for i in 1..len {
        value = (value << 8) | data[pos + i] as u64;
    }

    // Check for "unknown" marker (all data bits set)
    let all_ones = (1u64 << (7 * len)) - 1;
    if value == all_ones {
        value = u64::MAX; // Unknown size
    }

    Ok((value, len))
}

/// Read VInt preserving the leading marker bit (for element IDs).
fn read_vint_raw(data: &[u8], pos: usize) -> Result<(u64, usize)> {
    if pos >= data.len() {
        return Err(Error::InvalidData("unexpected end of EBML data".into()));
    }

    let first = data[pos];
    if first == 0 {
        return Err(Error::InvalidData("invalid EBML VInt".into()));
    }

    let len = first.leading_zeros() as usize + 1;
    if pos + len > data.len() || len > 4 {
        return Err(Error::InvalidData("EBML element ID too long".into()));
    }

    let mut value = first as u64;
    for i in 1..len {
        value = (value << 8) | data[pos + i] as u64;
    }

    Ok((value, len))
}

fn read_uint(data: &[u8], pos: usize, size: usize) -> u64 {
    let end = (pos + size).min(data.len());
    let mut value = 0u64;
    for i in pos..end {
        value = (value << 8) | data[i] as u64;
    }
    value
}

fn read_int(data: &[u8], pos: usize, size: usize) -> i64 {
    let v = read_uint(data, pos, size);
    // Sign extend
    if size > 0 && size < 8 {
        let sign_bit = 1u64 << (size * 8 - 1);
        if v & sign_bit != 0 {
            return (v | !((1u64 << (size * 8)) - 1)) as i64;
        }
    }
    v as i64
}

fn read_float(data: &[u8], pos: usize, size: usize) -> f64 {
    if size == 4 && pos + 4 <= data.len() {
        let bits = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        f32::from_bits(bits) as f64
    } else if size == 8 && pos + 8 <= data.len() {
        let bits = u64::from_be_bytes([
            data[pos], data[pos + 1], data[pos + 2], data[pos + 3],
            data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7],
        ]);
        f64::from_bits(bits)
    } else {
        0.0
    }
}

fn read_string(data: &[u8], pos: usize, size: usize) -> String {
    let end = (pos + size).min(data.len());
    String::from_utf8_lossy(&data[pos..end])
        .trim_end_matches('\0')
        .to_string()
}

fn format_duration(seconds: f64) -> String {
    let hours = (seconds / 3600.0) as u32;
    let minutes = ((seconds % 3600.0) / 60.0) as u32;
    let secs = seconds % 60.0;
    if hours > 0 {
        format!("{}:{:02}:{:05.2}", hours, minutes, secs)
    } else {
        format!("{}:{:05.2}", minutes, secs)
    }
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let print_value = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "Matroska".into(),
            family1: "Matroska".into(),
            family2: "Video".into(),
        },
        raw_value: value,
        print_value,
        priority: 0,
    }
}
