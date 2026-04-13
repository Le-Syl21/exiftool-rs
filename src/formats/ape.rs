//! APE (Monkey's Audio) and MPC (Musepack) format readers.
//!
//! Reads MAC header (APE audio info) and APEv1/APEv2 tags.
//! Also reads MPC SV7 header and then APE tags.
//! Mirrors ExifTool's APE.pm and MPC.pm.

use crate::error::{Error, Result};
use crate::formats::id3;
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

fn mk(group0: &str, name: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: String::new(),
        group: TagGroup {
            family0: group0.into(),
            family1: "Main".into(),
            family2: "Audio".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}

/// Read Monkey's Audio (APE) file.
pub fn read_ape(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 4 {
        return Err(Error::InvalidData("not an APE file".into()));
    }

    let mut tags = Vec::new();

    // Check magic: "MAC " (Monkey's Audio Container)
    if !data.starts_with(b"MAC ") && !data.starts_with(b"APETAGEX") {
        return Err(Error::InvalidData("not an APE file".into()));
    }

    if data.starts_with(b"MAC ") {
        // Parse MAC header
        parse_mac_header(data, &mut tags);
    }

    // Look for APE tags (footer at end of file)
    parse_ape_tags(data, &mut tags, "APE");

    // Duration composite: (TotalFrames - 1) * BlocksPerFrame + FinalFrameBlocks) / SampleRate
    // Check if we can compute duration
    let sr = tags
        .iter()
        .find(|t| t.name == "SampleRate")
        .and_then(|t| t.raw_value.to_display_string().parse::<u64>().ok());
    let tf = tags
        .iter()
        .find(|t| t.name == "TotalFrames")
        .and_then(|t| t.raw_value.to_display_string().parse::<u64>().ok());
    let bpf = tags
        .iter()
        .find(|t| t.name == "BlocksPerFrame")
        .and_then(|t| t.raw_value.to_display_string().parse::<u64>().ok());
    let ffb = tags
        .iter()
        .find(|t| t.name == "FinalFrameBlocks")
        .and_then(|t| t.raw_value.to_display_string().parse::<u64>().ok());

    if let (Some(sr), Some(tf), Some(bpf), Some(ffb)) = (sr, tf, bpf, ffb) {
        if sr > 0 && tf > 0 {
            let total_blocks = (tf - 1) * bpf + ffb;
            let duration_secs = total_blocks as f64 / sr as f64;
            let pv = format_duration(duration_secs);
            let mut tag = mk(
                "Composite",
                "Duration",
                Value::String(format!("{:.6}", duration_secs)),
            );
            tag.print_value = pv;
            tag.group.family0 = "Composite".into();
            tag.group.family2 = "Audio".into();
            tags.push(tag);
        }
    }

    Ok(tags)
}

/// Read Musepack (MPC) file.
pub fn read_mpc(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 4 {
        return Err(Error::InvalidData("not an MPC file".into()));
    }

    let mut tags = Vec::new();

    // Check for leading ID3 tag
    let mpc_offset = if data.starts_with(b"ID3") {
        id3_skip_to_audio(data)
    } else {
        0
    };

    // Also read ID3 tags
    let id3_tags = id3::read_mp3(data).unwrap_or_default();
    tags.extend(id3_tags);

    // MPC data starts at mpc_offset
    let mpc_data = if mpc_offset < data.len() {
        &data[mpc_offset..]
    } else {
        data
    };

    // Check MPC signature "MP+" (SV7)
    if mpc_data.len() >= 4 && mpc_data.starts_with(b"MP+") {
        let version = mpc_data[3] & 0x0f;
        if version == 7 {
            parse_mpc_sv7(mpc_data, &mut tags);
        }
    }

    // Look for APE tags
    parse_ape_tags(data, &mut tags, "MPC");

    Ok(tags)
}

/// Find where the MPC audio data starts (after ID3v2 header).
fn id3_skip_to_audio(data: &[u8]) -> usize {
    if data.len() < 10 || !data.starts_with(b"ID3") {
        return 0;
    }
    // ID3v2 size is 4 bytes syncsafe integer at offset 6
    let s0 = data[6] as usize & 0x7f;
    let s1 = data[7] as usize & 0x7f;
    let s2 = data[8] as usize & 0x7f;
    let s3 = data[9] as usize & 0x7f;
    let size = (s0 << 21) | (s1 << 14) | (s2 << 7) | s3;
    10 + size
}

/// Parse MPC SV7 header bit fields.
/// The header is 32 bytes starting at "MP+".
/// Fields are bit-addressed using little-endian bit ordering (bit 0 = LSB of byte 0).
fn parse_mpc_sv7(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 32 {
        return;
    }

    // Read as 8 little-endian 32-bit words
    let mut words = [0u32; 8];
    for i in 0..8 {
        words[i] = u32::from_le_bytes([
            data[i * 4],
            data[i * 4 + 1],
            data[i * 4 + 2],
            data[i * 4 + 3],
        ]);
    }

    // Build flat bit array (bit 0 = LSB of word 0)
    let get_bits = |start: usize, end: usize| -> u32 {
        let mut val = 0u32;
        for bit in (start..=end).rev() {
            let word_idx = bit / 32;
            let bit_idx = bit % 32;
            val = (val << 1) | ((words[word_idx] >> bit_idx) & 1);
        }
        val
    };

    // TotalFrames: Bit032-063
    let total_frames = get_bits(32, 63);
    tags.push(mk(
        "MPC",
        "TotalFrames",
        Value::String(total_frames.to_string()),
    ));

    // SampleRate: Bit080-081
    let sr_idx = get_bits(80, 81);
    let sample_rate = match sr_idx {
        0 => 44100u32,
        1 => 48000,
        2 => 37800,
        3 => 32000,
        _ => 44100,
    };
    tags.push(mk(
        "MPC",
        "SampleRate",
        Value::String(sample_rate.to_string()),
    ));

    // Quality: Bit084-087
    let quality_val = get_bits(84, 87);
    let quality_str = match quality_val {
        1 => "Unstable/Experimental".to_string(),
        5 => "0".to_string(),
        6 => "1".to_string(),
        7 => "2 (Telephone)".to_string(),
        8 => "3 (Thumb)".to_string(),
        9 => "4 (Radio)".to_string(),
        10 => "5 (Standard)".to_string(),
        11 => "6 (Xtreme)".to_string(),
        12 => "7 (Insane)".to_string(),
        13 => "8 (BrainDead)".to_string(),
        14 => "9".to_string(),
        15 => "10".to_string(),
        _ => quality_val.to_string(),
    };
    let mut t = mk("MPC", "Quality", Value::String(quality_val.to_string()));
    t.print_value = quality_str;
    tags.push(t);

    // MaxBand: Bit088-093
    let max_band = get_bits(88, 93);
    tags.push(mk("MPC", "MaxBand", Value::String(max_band.to_string())));

    // ReplayGainTrackPeak: Bit096-111
    let rg_tp = get_bits(96, 111);
    tags.push(mk(
        "MPC",
        "ReplayGainTrackPeak",
        Value::String(rg_tp.to_string()),
    ));

    // ReplayGainTrackGain: Bit112-127
    let rg_tg = get_bits(112, 127);
    tags.push(mk(
        "MPC",
        "ReplayGainTrackGain",
        Value::String(rg_tg.to_string()),
    ));

    // ReplayGainAlbumPeak: Bit128-143
    let rg_ap = get_bits(128, 143);
    tags.push(mk(
        "MPC",
        "ReplayGainAlbumPeak",
        Value::String(rg_ap.to_string()),
    ));

    // ReplayGainAlbumGain: Bit144-159
    let rg_ag = get_bits(144, 159);
    tags.push(mk(
        "MPC",
        "ReplayGainAlbumGain",
        Value::String(rg_ag.to_string()),
    ));

    // FastSeek: Bit179
    let fast_seek = get_bits(179, 179);
    let mut t = mk("MPC", "FastSeek", Value::String(fast_seek.to_string()));
    t.print_value = if fast_seek == 0 {
        "No".to_string()
    } else {
        "Yes".to_string()
    };
    tags.push(t);

    // Gapless: Bit191
    let gapless = get_bits(191, 191);
    let mut t = mk("MPC", "Gapless", Value::String(gapless.to_string()));
    t.print_value = if gapless == 0 {
        "No".to_string()
    } else {
        "Yes".to_string()
    };
    tags.push(t);

    // EncoderVersion: Bit216-223
    let enc_ver = get_bits(216, 223);
    // PrintConv: $val =~ s/(\d)(\d)(\d)$/$1.$2.$3/; $val
    let enc_ver_str = if enc_ver >= 100 {
        let h = enc_ver / 100;
        let t2 = (enc_ver % 100) / 10;
        let u = enc_ver % 10;
        format!("{}.{}.{}", h, t2, u)
    } else {
        enc_ver.to_string()
    };
    let mut t = mk("MPC", "EncoderVersion", Value::String(enc_ver.to_string()));
    t.print_value = enc_ver_str;
    tags.push(t);
}

/// Parse APE MAC header to extract audio info.
fn parse_mac_header(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 16 {
        return;
    }
    let vers = u16::from_le_bytes([data[4], data[5]]);

    if vers <= 3970 {
        // Old header (OldHeader table, FORMAT = 'int16u' starting at offset 4)
        // data[4..] = version-specific header
        if data.len() < 30 {
            return;
        }
        let hdr = &data[4..]; // header data starts at offset 4
                              // 0 => APEVersion (int16u / 1000) -- skip, not in expected output
                              // 1 => CompressionLevel (int16u at offset 2)
        let compression = u16::from_le_bytes([hdr[2], hdr[3]]);
        tags.push(mk(
            "APE",
            "CompressionLevel",
            Value::String(compression.to_string()),
        ));
        // 3 => Channels (int16u at offset 6)
        let channels = u16::from_le_bytes([hdr[6], hdr[7]]);
        tags.push(mk("APE", "Channels", Value::String(channels.to_string())));
        // 4 => SampleRate (int32u at offset 8)
        if hdr.len() >= 12 {
            let sr = u32::from_le_bytes([hdr[8], hdr[9], hdr[10], hdr[11]]);
            tags.push(mk("APE", "SampleRate", Value::String(sr.to_string())));
        }
        // 10 => TotalFrames (int32u at offset 20)
        if hdr.len() >= 24 {
            let tf = u32::from_le_bytes([hdr[20], hdr[21], hdr[22], hdr[23]]);
            tags.push(mk("APE", "TotalFrames", Value::String(tf.to_string())));
        }
        // 12 => FinalFrameBlocks (int32u at offset 24)
        if hdr.len() >= 28 {
            let ffb = u32::from_le_bytes([hdr[24], hdr[25], hdr[26], hdr[27]]);
            tags.push(mk(
                "APE",
                "FinalFrameBlocks",
                Value::String(ffb.to_string()),
            ));
        }
    } else {
        // New header (NewHeader table, FORMAT = 'int16u' starting at dlen)
        // Read dlen and hlen
        if data.len() < 16 {
            return;
        }
        let dlen = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;
        let hlen = u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;
        if dlen >= data.len() || dlen + hlen > data.len() {
            return;
        }
        let hdr = &data[dlen..dlen + hlen];
        if hdr.len() < 12 {
            return;
        }

        // NewHeader (FORMAT = 'int16u', each field is int16u unless noted):
        // offset 0 (byte 0) => CompressionLevel
        let compression = u16::from_le_bytes([hdr[0], hdr[1]]);
        tags.push(mk(
            "APE",
            "CompressionLevel",
            Value::String(compression.to_string()),
        ));
        // offset 2 (byte 4) => BlocksPerFrame (int32u)
        if hdr.len() >= 8 {
            let bpf = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]);
            tags.push(mk("APE", "BlocksPerFrame", Value::String(bpf.to_string())));
        }
        // offset 4 (byte 8) => FinalFrameBlocks (int32u)
        if hdr.len() >= 12 {
            let ffb = u32::from_le_bytes([hdr[8], hdr[9], hdr[10], hdr[11]]);
            tags.push(mk(
                "APE",
                "FinalFrameBlocks",
                Value::String(ffb.to_string()),
            ));
        }
        // offset 6 (byte 12) => TotalFrames (int32u)
        if hdr.len() >= 16 {
            let tf = u32::from_le_bytes([hdr[12], hdr[13], hdr[14], hdr[15]]);
            tags.push(mk("APE", "TotalFrames", Value::String(tf.to_string())));
        }
        // offset 8 (byte 16) => BitsPerSample (int16u)
        if hdr.len() >= 18 {
            let bps = u16::from_le_bytes([hdr[16], hdr[17]]);
            tags.push(mk("APE", "BitsPerSample", Value::String(bps.to_string())));
        }
        // offset 9 (byte 18) => Channels (int16u)
        if hdr.len() >= 20 {
            let ch = u16::from_le_bytes([hdr[18], hdr[19]]);
            tags.push(mk("APE", "Channels", Value::String(ch.to_string())));
        }
        // offset 10 (byte 20) => SampleRate (int32u)
        if hdr.len() >= 24 {
            let sr = u32::from_le_bytes([hdr[20], hdr[21], hdr[22], hdr[23]]);
            tags.push(mk("APE", "SampleRate", Value::String(sr.to_string())));
        }
    }
}

/// Parse APEv1/APEv2 tags from data.
/// Looks for APETAGEX footer at the end of the file (before optional ID3v1 tag).
pub fn parse_ape_tags(data: &[u8], tags: &mut Vec<Tag>, group: &str) {
    // Check for trailing ID3v1 (128 bytes from end starting with "TAG")
    let search_end = if data.len() >= 128 && &data[data.len() - 128..data.len() - 125] == b"TAG" {
        data.len() - 128
    } else {
        data.len()
    };

    // Look for APETAGEX footer (32 bytes before end/ID3v1)
    if search_end < 32 {
        return;
    }
    let foot_pos = search_end - 32;
    if &data[foot_pos..foot_pos + 8] != b"APETAGEX" {
        return;
    }

    let footer = &data[foot_pos..foot_pos + 32];
    let _version = u32::from_le_bytes([footer[8], footer[9], footer[10], footer[11]]);
    let tag_size = u32::from_le_bytes([footer[12], footer[13], footer[14], footer[15]]) as usize;
    let tag_count = u32::from_le_bytes([footer[16], footer[17], footer[18], footer[19]]) as usize;
    let flags = u32::from_le_bytes([footer[20], footer[21], footer[22], footer[23]]);

    // Check if this is a footer (bit 29 = 0 means footer)
    let is_header = (flags >> 29) & 1 == 1;
    if is_header {
        return;
    } // skip if this is a header

    // tag_size includes the 32-byte footer
    if tag_size < 32 {
        return;
    }
    let data_size = tag_size - 32;
    if data_size > foot_pos {
        return;
    }
    let tag_data_start = foot_pos - data_size;
    let tag_data = &data[tag_data_start..foot_pos];

    // Parse APE items
    let mut pos = 0;
    for _ in 0..tag_count {
        if pos + 8 > tag_data.len() {
            break;
        }
        let item_size = u32::from_le_bytes([
            tag_data[pos],
            tag_data[pos + 1],
            tag_data[pos + 2],
            tag_data[pos + 3],
        ]) as usize;
        let item_flags = u32::from_le_bytes([
            tag_data[pos + 4],
            tag_data[pos + 5],
            tag_data[pos + 6],
            tag_data[pos + 7],
        ]);
        pos += 8;

        // Find null terminator for key
        let key_end = match tag_data[pos..].iter().position(|&b| b == 0) {
            Some(p) => pos + p,
            None => break,
        };
        let key = match std::str::from_utf8(&tag_data[pos..key_end]) {
            Ok(s) => s.to_string(),
            Err(_) => {
                pos = key_end + 1 + item_size;
                continue;
            }
        };
        pos = key_end + 1;

        if pos + item_size > tag_data.len() {
            break;
        }
        let val_bytes = &tag_data[pos..pos + item_size];
        pos += item_size;

        // Determine if binary (item type bits 1-2)
        let item_type = (item_flags >> 1) & 3;
        let is_binary = item_type == 1;

        // Generate tag name from key (ExifTool MakeTag logic)
        let tag_name = ape_key_to_tag_name(&key);

        if key.starts_with("Cover Art") && is_binary {
            // Split at first null: description + binary data
            if let Some(null_pos) = val_bytes.iter().position(|&b| b == 0) {
                let desc = crate::encoding::decode_utf8_or_latin1(&val_bytes[..null_pos]);
                let img_data = val_bytes[null_pos + 1..].to_vec();

                // Emit description tag
                let desc_key = format!("{} Desc", key);
                let desc_name = ape_key_to_tag_name(&desc_key);
                tags.push(mk(group, &desc_name, Value::String(desc)));

                // Emit binary cover art tag
                tags.push(mk(group, &tag_name, Value::Binary(img_data)));
            } else {
                tags.push(mk(group, &tag_name, Value::Binary(val_bytes.to_vec())));
            }
        } else if is_binary {
            tags.push(mk(group, &tag_name, Value::Binary(val_bytes.to_vec())));
        } else {
            let s = crate::encoding::decode_utf8_or_latin1(val_bytes);
            // Apply known tag transformations
            let (raw_val, print_val) = ape_value_conv(&tag_name, &s);
            let mut tag = mk(group, &tag_name, raw_val);
            tag.print_value = print_val;
            tags.push(tag);
        }
    }
}

/// Convert APE tag key to ExifTool tag name.
/// Mirrors Perl: ucfirst(lc($tag)), remove invalid chars, capitalize after non-word chars.
fn ape_key_to_tag_name(key: &str) -> String {
    // $name = ucfirst(lc($tag))
    // $name =~ s/[^\w-]+(.?)/\U$1/sg  -- replace non-word/hyphen chars + following char with uppercase
    // $name =~ s/([a-z0-9])_([a-z])/$1\U$2/g  -- capitalize after underscore following lowercase

    // Match known tags from PLIST.pm / APE.pm
    match key {
        "Tool Version" => return "ToolVersion".to_string(),
        "Tool Name" => return "ToolName".to_string(),
        "Media Jukebox: Date" => return "MediaJukeboxDate".to_string(),
        "Cover Art (front)" => return "CoverArtFront".to_string(),
        "Cover Art (front) Desc" => return "CoverArtFrontDesc".to_string(),
        _ => {}
    }

    let lower = key.to_lowercase();
    // ucfirst + capitalize after non-word chars
    let mut name = String::new();
    let mut capitalize_next = true;
    for c in lower.chars() {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            if capitalize_next {
                for uc in c.to_uppercase() {
                    name.push(uc);
                }
                capitalize_next = false;
            } else {
                name.push(c);
            }
        } else {
            capitalize_next = true;
        }
    }
    name
}

/// Apply value conversion for known APE tags.
fn ape_value_conv(_tag_name: &str, val: &str) -> (Value, String) {
    (Value::String(val.to_string()), val.to_string())
}

/// Format duration in ExifTool style.
fn format_duration(secs: f64) -> String {
    if secs < 60.0 {
        format!("{:.2} s", secs)
    } else if secs < 3600.0 {
        let m = (secs / 60.0) as u64;
        let s = secs - m as f64 * 60.0;
        format!("{}:{:05.2}", m, s)
    } else {
        let h = (secs / 3600.0) as u64;
        let rem = secs - h as f64 * 3600.0;
        let m = (rem / 60.0) as u64;
        let s = rem - m as f64 * 60.0;
        format!("{}:{:02}:{:05.2}", h, m, s)
    }
}
