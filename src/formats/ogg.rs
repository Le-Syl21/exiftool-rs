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

    // Process packets
    for (_i, packet) in packets.iter().enumerate() {
        if packet.len() < 7 {
            continue;
        }

        // Vorbis identification header
        if packet[0] == 1 && &packet[1..7] == b"vorbis" {
            parse_vorbis_identification(packet, &mut tags);
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

    Ok(tags)
}

fn parse_vorbis_identification(packet: &[u8], tags: &mut Vec<Tag>) {
    if packet.len() < 30 {
        return;
    }
    // Skip type byte (1) + "vorbis" (6) = 7 bytes
    let d = &packet[7..];

    let _version = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
    let channels = d[4];
    let sample_rate = u32::from_le_bytes([d[5], d[6], d[7], d[8]]);
    let _max_bitrate = i32::from_le_bytes([d[9], d[10], d[11], d[12]]);
    let nominal_bitrate = i32::from_le_bytes([d[13], d[14], d[15], d[16]]);
    let _min_bitrate = i32::from_le_bytes([d[17], d[18], d[19], d[20]]);

    tags.push(mk("AudioFormat", "Audio Format", Value::String("Vorbis".into())));
    tags.push(mk("NumChannels", "Number of Channels", Value::U8(channels)));
    tags.push(mk("SampleRate", "Sample Rate", Value::U32(sample_rate)));

    if nominal_bitrate > 0 {
        tags.push(mk(
            "NominalBitrate",
            "Nominal Bitrate",
            Value::String(format!("{} kbps", nominal_bitrate / 1000)),
        ));
    }
}

fn parse_opus_identification(packet: &[u8], tags: &mut Vec<Tag>) {
    if packet.len() < 19 {
        return;
    }
    // "OpusHead" (8) then:
    let d = &packet[8..];
    let _version = d[0];
    let channels = d[1];
    let _pre_skip = u16::from_le_bytes([d[2], d[3]]);
    let sample_rate = u32::from_le_bytes([d[4], d[5], d[6], d[7]]);

    tags.push(mk("AudioFormat", "Audio Format", Value::String("Opus".into())));
    tags.push(mk("NumChannels", "Number of Channels", Value::U8(channels)));
    tags.push(mk("SampleRate", "Sample Rate", Value::U32(sample_rate)));
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

    tags.push(mk("VideoFormat", "Video Format", Value::String(format!("Theora {}.{}.{}", major_ver, minor_ver, rev_ver))));
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
