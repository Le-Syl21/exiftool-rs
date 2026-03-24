//! MPEG-1/MPEG-2 video format reader.
//!
//! Parses MPEG program stream headers to extract video and audio metadata.
//! Mirrors ExifTool's MPEG.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

/// Create a tag in the MPEG/Video group.
fn mk_video(name: &str, value: Value, print: String) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "MPEG".into(),
            family1: "MPEG".into(),
            family2: "Video".into(),
        },
        raw_value: value,
        print_value: print,
        priority: 0,
    }
}

/// Create a tag in the MPEG/Audio group.
fn mk_audio(name: &str, value: Value, print: String) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "MPEG".into(),
            family1: "MPEG".into(),
            family2: "Audio".into(),
        },
        raw_value: value,
        print_value: print,
        priority: 0,
    }
}

/// Format a bitrate value for display (matches ExifTool's ConvertBitrate).
fn format_bitrate(bps: u32) -> String {
    if bps >= 1_000_000 {
        let mbps = bps as f64 / 1_000_000.0;
        if (mbps - mbps.round()).abs() < 0.0001 {
            format!("{} Mbps", mbps as u32)
        } else {
            format!("{:.3} Mbps", mbps)
        }
    } else if bps >= 1000 {
        let kbps = bps as f64 / 1000.0;
        if (kbps - kbps.round()).abs() < 0.0001 {
            format!("{} kbps", kbps as u32)
        } else {
            format!("{:.3} kbps", kbps)
        }
    } else {
        format!("{} bps", bps)
    }
}

/// Process an MPEG video sequence header (follows 0x000001B3).
/// The data should start right after the start code.
/// Returns tags extracted from the sequence header.
fn process_mpeg_video(data: &[u8]) -> Vec<Tag> {
    let mut tags = Vec::new();
    if data.len() < 8 {
        return tags;
    }

    let w1 = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    let w2 = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);

    // Bits 0-11: ImageWidth
    let width = (w1 >> 20) & 0xFFF;
    // Bits 12-23: ImageHeight
    let height = (w1 >> 8) & 0xFFF;
    // Bits 24-27: AspectRatio
    let aspect_code = (w1 >> 4) & 0xF;
    // Bits 28-31: FrameRate
    let frame_rate_code = w1 & 0xF;

    // Validate: forbidden aspect ratio 0 or 15, frame rate must be 1-8
    if aspect_code == 0 || aspect_code == 15 || frame_rate_code == 0 || frame_rate_code > 8 {
        return tags;
    }

    // Bits 32-49: VideoBitrate (18 bits from w2)
    let video_bitrate_raw = (w2 >> 14) & 0x3FFFF;

    tags.push(mk_video(
        "ImageWidth",
        Value::U32(width),
        width.to_string(),
    ));
    tags.push(mk_video(
        "ImageHeight",
        Value::U32(height),
        height.to_string(),
    ));

    let aspect_str = match aspect_code {
        1 => "1:1",
        2 => "0.6735",
        3 => "16:9, 625 line, PAL",
        4 => "0.7615",
        5 => "0.8055",
        6 => "16:9, 525 line, NTSC",
        7 => "0.8935",
        8 => "4:3, 625 line, PAL, CCIR601",
        9 => "0.9815",
        10 => "1.0255",
        11 => "1.0695",
        12 => "4:3, 525 line, NTSC, CCIR601",
        13 => "1.1575",
        14 => "1.2015",
        _ => "Unknown",
    };
    let aspect_val = match aspect_code {
        1 => 1.0,
        2 => 0.6735,
        3 => 0.7031,
        4 => 0.7615,
        5 => 0.8055,
        6 => 0.8437,
        7 => 0.8935,
        8 => 0.9157,
        9 => 0.9815,
        10 => 1.0255,
        11 => 1.0695,
        12 => 1.0950,
        13 => 1.1575,
        14 => 1.2015,
        _ => 0.0,
    };
    tags.push(mk_video(
        "AspectRatio",
        Value::F64(aspect_val),
        aspect_str.to_string(),
    ));

    let frame_rate = match frame_rate_code {
        1 => 23.976,
        2 => 24.0,
        3 => 25.0,
        4 => 29.97,
        5 => 30.0,
        6 => 50.0,
        7 => 59.94,
        8 => 60.0,
        _ => 0.0,
    };
    tags.push(mk_video(
        "FrameRate",
        Value::F64(frame_rate),
        format!("{} fps", frame_rate),
    ));

    if video_bitrate_raw == 0x3FFFF {
        tags.push(mk_video(
            "VideoBitrate",
            Value::String("Variable".into()),
            "Variable".into(),
        ));
    } else {
        let bitrate = video_bitrate_raw * 400;
        tags.push(mk_video(
            "VideoBitrate",
            Value::U32(bitrate),
            format_bitrate(bitrate),
        ));
    }

    tags
}

/// Parse an MPEG audio frame header (the 4-byte sync word).
/// Returns tags if a valid audio frame header is found.
fn parse_mpeg_audio_header(word: u32) -> Option<Vec<Tag>> {
    // Check frame sync (11 bits)
    if (word & 0xFFE00000) != 0xFFE00000 {
        return None;
    }

    // Validate header
    if (word & 0x180000) == 0x080000       // reserved version ID
        || (word & 0x060000) == 0x000000   // reserved layer
        || (word & 0x00F000) == 0x000000   // free bitrate
        || (word & 0x00F000) == 0x00F000   // bad bitrate
        || (word & 0x000C00) == 0x000C00   // reserved sample freq
        || (word & 0x000003) == 0x000002
    // reserved emphasis
    {
        return None;
    }

    let mut tags = Vec::new();

    // Bits 11-12: MPEG Audio Version
    let version_bits = (word >> 19) & 0x3;
    let version_str = match version_bits {
        0 => "2.5",
        2 => "2",
        3 => "1",
        _ => return None,
    };
    tags.push(mk_audio(
        "MPEGAudioVersion",
        Value::String(version_str.into()),
        version_str.into(),
    ));

    // Bits 13-14: Audio Layer
    let layer_bits = (word >> 17) & 0x3;
    let layer = match layer_bits {
        1 => 3u8,
        2 => 2,
        3 => 1,
        _ => return None,
    };
    tags.push(mk_audio(
        "AudioLayer",
        Value::U8(layer),
        layer.to_string(),
    ));

    // Bits 16-19: Audio Bitrate
    let bitrate_index = ((word >> 12) & 0xF) as usize;
    let bitrate = lookup_audio_bitrate(version_bits, layer_bits, bitrate_index);
    if let Some(br) = bitrate {
        if br > 0 {
            tags.push(mk_audio(
                "AudioBitrate",
                Value::U32(br),
                format_bitrate(br),
            ));
        }
    }

    // Bits 20-21: Sample Rate
    let sample_rate_index = (word >> 10) & 0x3;
    let sample_rate = match version_bits {
        3 => match sample_rate_index {
            // version 1
            0 => Some(44100u32),
            1 => Some(48000),
            2 => Some(32000),
            _ => None,
        },
        2 => match sample_rate_index {
            // version 2
            0 => Some(22050),
            1 => Some(24000),
            2 => Some(16000),
            _ => None,
        },
        0 => match sample_rate_index {
            // version 2.5
            0 => Some(11025),
            1 => Some(12000),
            2 => Some(8000),
            _ => None,
        },
        _ => None,
    };
    if let Some(sr) = sample_rate {
        tags.push(mk_audio(
            "SampleRate",
            Value::U32(sr),
            sr.to_string(),
        ));
    }

    // Bits 24-25: Channel Mode
    let channel_mode = (word >> 6) & 0x3;
    let channel_str = match channel_mode {
        0 => "Stereo",
        1 => "Joint Stereo",
        2 => "Dual Channel",
        3 => "Single Channel",
        _ => "Unknown",
    };
    tags.push(mk_audio(
        "ChannelMode",
        Value::String(channel_str.into()),
        channel_str.into(),
    ));

    // Bit 28: CopyrightFlag
    let copyright = (word >> 3) & 0x1;
    let copyright_str = if copyright == 1 { "True" } else { "False" };
    tags.push(mk_audio(
        "CopyrightFlag",
        Value::String(copyright_str.into()),
        copyright_str.into(),
    ));

    // Bit 29: OriginalMedia
    let original = (word >> 2) & 0x1;
    let original_str = if original == 1 { "True" } else { "False" };
    tags.push(mk_audio(
        "OriginalMedia",
        Value::String(original_str.into()),
        original_str.into(),
    ));

    // Bits 30-31: Emphasis
    let emphasis = word & 0x3;
    let emphasis_str = match emphasis {
        0 => "None",
        1 => "50/15 ms",
        2 => "reserved",
        3 => "CCIT J.17",
        _ => "Unknown",
    };
    tags.push(mk_audio(
        "Emphasis",
        Value::String(emphasis_str.into()),
        emphasis_str.into(),
    ));

    Some(tags)
}

/// Look up the audio bitrate from the MPEG version, layer, and bitrate index.
/// Returns the bitrate in bps, or None if invalid.
fn lookup_audio_bitrate(version_bits: u32, layer_bits: u32, index: usize) -> Option<u32> {
    if index == 0 || index >= 15 {
        return None;
    }

    // version_bits: 3=v1, 2=v2, 0=v2.5
    // layer_bits: 3=layer1, 2=layer2, 1=layer3
    let table: &[u32; 14] = match (version_bits, layer_bits) {
        // Version 1, Layer 1
        (3, 3) => &[
            32000, 64000, 96000, 128000, 160000, 192000, 224000, 256000, 288000, 320000, 352000,
            384000, 416000, 448000,
        ],
        // Version 1, Layer 2
        (3, 2) => &[
            32000, 48000, 56000, 64000, 80000, 96000, 112000, 128000, 160000, 192000, 224000,
            256000, 320000, 384000,
        ],
        // Version 1, Layer 3
        (3, 1) => &[
            32000, 40000, 48000, 56000, 64000, 80000, 96000, 112000, 128000, 160000, 192000,
            224000, 256000, 320000,
        ],
        // Version 2/2.5, Layer 1
        (0 | 2, 3) => &[
            32000, 48000, 56000, 64000, 80000, 96000, 112000, 128000, 144000, 160000, 176000,
            192000, 224000, 256000,
        ],
        // Version 2/2.5, Layer 2 or 3
        (0 | 2, 1 | 2) => &[
            8000, 16000, 24000, 32000, 40000, 48000, 56000, 64000, 80000, 96000, 112000, 128000,
            144000, 160000,
        ],
        _ => return None,
    };

    Some(table[index - 1])
}

/// Search for an MPEG audio frame sync in the data buffer.
/// Returns the parsed audio tags if found.
fn find_mpeg_audio(data: &[u8]) -> Option<Vec<Tag>> {
    let len = data.len();
    let mut pos = 0;
    while pos + 3 < len {
        if data[pos] == 0xFF && (data[pos + 1] & 0xE0) == 0xE0 {
            let word = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
            if let Some(tags) = parse_mpeg_audio_header(word) {
                return Some(tags);
            }
            // Not valid, try next byte
            pos += 1;
        } else {
            pos += 1;
        }
    }
    None
}

/// Read MPEG-1/MPEG-2 program stream.
///
/// Scans the first 256KB for video sequence headers (0x000001B3) and
/// audio stream headers (0x000001C0), extracting tags from each.
pub fn read_mpeg(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 4 {
        return Err(Error::InvalidData("not an MPEG file".into()));
    }

    // Validate MPEG start code
    if !(data[0] == 0x00
        && data[1] == 0x00
        && data[2] == 0x01
        && (data[3] == 0xBA || data[3] == 0xBB || (data[3] & 0xF0) == 0xE0 || data[3] == 0xB3))
    {
        return Err(Error::InvalidData("not an MPEG file".into()));
    }

    let mut tags = Vec::new();
    let mut found_video = false;
    let mut found_audio = false;

    // Scan up to 256KB (like Perl's 65536*4 = 262144)
    let scan_len = data.len().min(262144);

    // First, check for an audio frame sync before the first start code
    // (like Perl does to handle MP3-like data at start)
    let mut first_start = scan_len;
    let mut pos = 0;
    while pos + 3 < scan_len {
        if data[pos] == 0x00 && data[pos + 1] == 0x00 && data[pos + 2] == 0x01 {
            let code = data[pos + 3];
            if code == 0xB3 || code == 0xC0 {
                first_start = pos;
                break;
            }
        }
        pos += 1;
    }

    // Check for audio sync before the first video/audio start code
    if first_start > 3 {
        let pre_data = &data[..first_start.min(scan_len)];
        if let Some(audio_tags) = find_mpeg_audio(pre_data) {
            tags.extend(audio_tags);
            found_audio = true;
        }
    }

    // Scan for 0x000001B3 (sequence header) and 0x000001C0 (audio stream)
    pos = 0;
    while pos + 3 < scan_len {
        if data[pos] == 0x00 && data[pos + 1] == 0x00 && data[pos + 2] == 0x01 {
            let code = data[pos + 3];
            if code == 0xB3 && !found_video {
                // Video sequence header: data starts at pos+4
                let remaining = &data[pos + 4..scan_len.min(pos + 4 + 256)];
                let video_tags = process_mpeg_video(remaining);
                if !video_tags.is_empty() {
                    tags.extend(video_tags);
                    found_video = true;
                }
            } else if code == 0xC0 && !found_audio {
                // Audio stream: data starts at pos+4
                let end = scan_len.min(pos + 4 + 256);
                if pos + 4 < end {
                    let audio_data = &data[pos + 4..end];
                    if let Some(audio_tags) = find_mpeg_audio(audio_data) {
                        tags.extend(audio_tags);
                        found_audio = true;
                    }
                }
            }
            if found_video && found_audio {
                break;
            }
            pos += 4;
        } else {
            pos += 1;
        }
    }

    if tags.is_empty() {
        return Err(Error::InvalidData("no MPEG video or audio headers found".into()));
    }

    Ok(tags)
}
