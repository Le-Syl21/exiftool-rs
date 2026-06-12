//! OGG Vorbis/Opus container reader.
//!
//! Parses OGG pages to find Vorbis/Opus identification and comment headers.
//! Mirrors ExifTool's Ogg.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_ogg(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 27 || !data.starts_with(b"OggS") {
        return Err(Error::InvalidData("not an OGG file".into()));
    }

    let mut tags = Vec::new();
    let mut pos = 0;
    let mut packets: Vec<Vec<u8>> = Vec::new();
    let mut current_packet = Vec::new();
    let mut packet_count = 0;

    // Read OGG pages and assemble packets (first stream only)
    while pos + 27 <= data.len() && packet_count < 3 {
        if &data[pos..pos + 4] != b"OggS" {
            break;
        }

        let _version = data[pos + 4];
        let _header_type = data[pos + 5];
        let num_segments = data[pos + 26] as usize;

        if pos + 27 + num_segments > data.len() {
            break;
        }

        let segment_table = &data[pos + 27..pos + 27 + num_segments];
        let mut data_pos = pos + 27 + num_segments;

        for &seg_size in segment_table {
            let seg_size = seg_size as usize;
            if data_pos + seg_size > data.len() {
                break;
            }
            current_packet.extend_from_slice(&data[data_pos..data_pos + seg_size]);
            data_pos += seg_size;

            // Segment size < 255 means end of packet
            if seg_size < 255 {
                packets.push(std::mem::take(&mut current_packet));
                packet_count += 1;
            }
        }

        pos = data_pos;
    }

    // Detect if this is a FLAC-in-OGG stream
    let is_flac_in_ogg = packets
        .first()
        .map(|p| p.len() >= 5 && p[0] == 0x7F && &p[1..5] == b"FLAC")
        .unwrap_or(false);

    // Process packets
    let mut nominal_bitrate: Option<i32> = None;
    for packet in &packets {
        if packet.len() < 4 {
            continue;
        }

        if is_flac_in_ogg {
            // FLAC-in-OGG: first packet has \x7FFLAC header, subsequent packets are FLAC metadata blocks
            if packet[0] == 0x7F && packet.len() >= 5 && &packet[1..5] == b"FLAC" {
                parse_flac_in_ogg_packet(packet, &mut tags);
            } else {
                // Subsequent FLAC-in-OGG packets are raw FLAC metadata blocks
                // Type 4 = VORBIS_COMMENT (parse as Vorbis comments)
                let block_type = packet[0] & 0x7F;
                if block_type == 4 && packet.len() >= 4 {
                    // Skip 4-byte FLAC metadata block header (type + 24-bit size)
                    crate::formats::flac::parse_vorbis_comments(&packet[4..], &mut tags);
                }
            }
            continue;
        }

        if packet.len() < 7 {
            continue;
        }

        // Vorbis identification header
        if packet[0] == 1 && &packet[1..7] == b"vorbis" {
            nominal_bitrate = parse_vorbis_identification(packet, &mut tags);
        }
        // Vorbis comment header
        else if packet[0] == 3 && &packet[1..7] == b"vorbis" {
            crate::formats::flac::parse_vorbis_comments(&packet[7..], &mut tags);
        }
        // Opus identification header
        else if packet.len() >= 8 && &packet[..8] == b"OpusHead" {
            parse_opus_identification(packet, &mut tags);
        }
        // Opus tags (comment) header
        else if packet.len() >= 8 && &packet[..8] == b"OpusTags" {
            crate::formats::flac::parse_vorbis_comments(&packet[8..], &mut tags);
        }
        // Theora identification header
        else if packet.len() >= 7 && packet[0] == 0x80 && &packet[1..7] == b"theora" {
            parse_theora_identification(packet, &mut tags);
        }
    }

    // Duration is the Vorbis composite (Vorbis.pm): FileSize * 8 / NominalBitrate,
    // formatted with ConvertDuration + " (approx)". This is only approximate.
    if let Some(nominal) = nominal_bitrate {
        if nominal > 0 {
            let duration = data.len() as f64 * 8.0 / nominal as f64;
            tags.push(mk(
                "Duration",
                "Duration",
                Value::String(format!("{} (approx)", convert_duration(duration))),
            ));
        }
    }

    Ok(tags)
}

/// Parse a FLAC-in-OGG identification packet.
/// Format: \x7F FLAC version(2) num_headers(2) "fLaC" flac_metadata_blocks...
fn parse_flac_in_ogg_packet(packet: &[u8], tags: &mut Vec<Tag>) {
    // After \x7FFLAC (5 bytes) + version(2) + num_headers(2) = 9 bytes header
    // Then the native FLAC stream starting with "fLaC" magic
    if packet.len() < 9 {
        return;
    }
    let flac_data = &packet[9..];
    // Pass to FLAC reader (it expects "fLaC" magic at start)
    if flac_data.starts_with(b"fLaC") {
        if let Ok(flac_tags) = crate::formats::flac::read_flac(flac_data) {
            tags.extend(flac_tags);
        }
    } else {
        // Try parsing as raw FLAC metadata block (without "fLaC" header)
        // Some encoders skip the "fLaC" sync marker in OGG embedding
        // Build a fake FLAC stream
        let mut fake_flac = b"fLaC".to_vec();
        fake_flac.extend_from_slice(flac_data);
        if let Ok(flac_tags) = crate::formats::flac::read_flac(&fake_flac) {
            tags.extend(flac_tags);
        }
    }
}

fn parse_vorbis_identification(packet: &[u8], tags: &mut Vec<Tag>) -> Option<i32> {
    if packet.len() < 30 {
        return None;
    }
    // Skip type byte (1) + "vorbis" (6) = 7 bytes
    let d = &packet[7..];

    let _version = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
    let channels = d[4];
    let sample_rate = u32::from_le_bytes([d[5], d[6], d[7], d[8]]);
    let _max_bitrate = i32::from_le_bytes([d[9], d[10], d[11], d[12]]);
    let nominal_bitrate = i32::from_le_bytes([d[13], d[14], d[15], d[16]]);
    let _min_bitrate = i32::from_le_bytes([d[17], d[18], d[19], d[20]]);

    // Perl: VorbisVersion, AudioChannels, SampleRate (no AudioFormat tag)
    let version = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
    tags.push(mk("VorbisVersion", "Vorbis Version", Value::U32(version)));
    tags.push(mk("AudioChannels", "Audio Channels", Value::U8(channels)));
    tags.push(mk("SampleRate", "Sample Rate", Value::U32(sample_rate)));

    if nominal_bitrate > 0 {
        tags.push(mk(
            "NominalBitrate",
            "Nominal Bitrate",
            Value::String(format!("{} kbps", nominal_bitrate / 1000)),
        ));
    }
    Some(nominal_bitrate)
}

/// Port of ExifTool ConvertDuration: "%.2f s" under 30 s, else "[D days ]H:MM:SS".
fn convert_duration(secs: f64) -> String {
    if secs == 0.0 {
        return "0 s".to_string();
    }
    let sign = if secs < 0.0 { "-" } else { "" };
    let mut t = secs.abs();
    if t < 30.0 {
        return format!("{}{:.2} s", sign, t);
    }
    t += 0.5; // round to nearest second
    let mut h = (t / 3600.0) as i64;
    t -= (h as f64) * 3600.0;
    let m = (t / 60.0) as i64;
    t -= (m as f64) * 60.0;
    if h > 24 {
        let d = h / 24;
        h -= d * 24;
        format!("{}{} days {}:{:02}:{:02}", sign, d, h, m, t as i64)
    } else {
        format!("{}{}:{:02}:{:02}", sign, h, m, t as i64)
    }
}

fn parse_opus_identification(packet: &[u8], tags: &mut Vec<Tag>) {
    if packet.len() < 19 {
        return;
    }
    // "OpusHead" (8) then: version(1) channels(1) pre_skip(2) sample_rate(4) output_gain(2) map_family(1)
    let d = &packet[8..];
    let version = d[0];
    let channels = d[1];
    let _pre_skip = u16::from_le_bytes([d[2], d[3]]);
    let sample_rate = u32::from_le_bytes([d[4], d[5], d[6], d[7]]);
    let output_gain = u16::from_le_bytes([d[8], d[9]]);

    // Perl tag names: OpusVersion, AudioChannels, SampleRate, OutputGain.
    // OpusVersion (Opus::Header index 0) is the raw int8u value, no formatting.
    tags.push(mk("OpusVersion", "Opus Version", Value::U8(version)));
    tags.push(mk("AudioChannels", "Audio Channels", Value::U8(channels)));
    tags.push(mk("SampleRate", "Sample Rate", Value::U32(sample_rate)));
    // OutputGain (Opus::Header index 8, int16u): ValueConv 10 ** ($val/5120).
    let gain = 10f64.powf(output_gain as f64 / 5120.0);
    tags.push(mk(
        "OutputGain",
        "Output Gain",
        Value::String(crate::value::format_g15(gain)),
    ));
}

fn parse_theora_identification(packet: &[u8], tags: &mut Vec<Tag>) {
    if packet.len() < 42 {
        return;
    }
    let d = &packet[7..];
    let major_ver = d[0];
    let minor_ver = d[1];
    let rev_ver = d[2];
    let width = ((d[3] as u32) << 8 | d[4] as u32) << 4;
    let height = ((d[5] as u32) << 8 | d[6] as u32) << 4;

    tags.push(mk(
        "VideoFormat",
        "Video Format",
        Value::String(format!("Theora {}.{}.{}", major_ver, minor_ver, rev_ver)),
    ));
    tags.push(mk("ImageWidth", "Image Width", Value::U32(width)));
    tags.push(mk("ImageHeight", "Image Height", Value::U32(height)));
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let print_value = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "Vorbis".into(),
            family1: "Ogg".into(),
            family2: "Audio".into(),
        },
        raw_value: value,
        print_value,
        priority: 0,
    }
}
