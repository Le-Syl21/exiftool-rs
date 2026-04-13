//! FLV (Flash Video) format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

pub fn read_flv(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 9 || !data.starts_with(b"FLV\x01") {
        return Err(Error::InvalidData("not an FLV file".into()));
    }

    let mut tags = Vec::new();

    // FLV header: FLV(3) version(1) flags(1) offset(4 BE)
    let flags = data[4];
    let has_audio = flags & 0x04 != 0;
    let has_video = flags & 0x01 != 0;
    let header_offset = u32::from_be_bytes([data[5], data[6], data[7], data[8]]) as usize;

    // Parse FLV tags starting at header_offset
    // Each tag: prev_tag_size(4) type(1) data_size(3 BE) timestamp(3 BE) ts_ext(1) stream_id(3) data(N)
    // First prev_tag_size is 0 at position header_offset
    let mut pos = header_offset;

    // Skip initial prev_tag_size (4 bytes)
    if pos + 4 <= data.len() {
        pos += 4;
    }

    let mut found_meta = false;
    let mut audio_info_found = false;
    let mut video_info_found = false;

    while pos + 11 <= data.len()
        && (!found_meta || (!audio_info_found && has_audio) || (!video_info_found && has_video))
    {
        let tag_type = data[pos];
        let data_size = ((data[pos + 1] as usize) << 16)
            | ((data[pos + 2] as usize) << 8)
            | (data[pos + 3] as usize);
        // skip timestamp (3) + ts_ext (1) + stream_id (3) = 7 bytes
        let tag_start = pos + 11;
        let tag_end = tag_start + data_size;

        if tag_end > data.len() {
            break;
        }

        match tag_type {
            0x12 => {
                // Script (AMF metadata) tag
                if !found_meta {
                    let tag_data = &data[tag_start..tag_end];
                    flv_parse_amf_metadata(tag_data, &mut tags);
                    found_meta = true;
                }
            }
            0x08 if !audio_info_found => {
                // Audio tag: first byte = codec info
                if data_size >= 1 {
                    let info_byte = data[tag_start];
                    let codec_id = (info_byte >> 4) & 0x0f;
                    let sample_rate_idx = (info_byte >> 2) & 0x03;
                    let sample_size = (info_byte >> 1) & 0x01;
                    let stereo = info_byte & 0x01;

                    let codec_name = match codec_id {
                        0 => "Uncompressed",
                        1 => "ADPCM",
                        2 => "MP3",
                        3 => "Uncompressed LE",
                        4 => "Nellymoser 16kHz",
                        5 => "Nellymoser 8kHz",
                        6 => "Nellymoser",
                        7 => "G711 A-law",
                        8 => "G711 mu-law",
                        10 => "AAC",
                        11 => "Speex",
                        14 => "MP3 8kHz",
                        15 => "Device-specific",
                        _ => "Unknown",
                    };
                    let sample_rate = match sample_rate_idx {
                        0 => "5512",
                        1 => "11025",
                        2 => "22050",
                        3 => "44100",
                        _ => "Unknown",
                    };
                    let channels = if stereo == 1 {
                        "2 (stereo)"
                    } else {
                        "1 (mono)"
                    };
                    let bits = if sample_size == 1 { "16" } else { "8" };

                    tags.push(mktag(
                        "FLV",
                        "AudioCodecID",
                        "Audio Codec ID",
                        Value::String(format!("{}", codec_id)),
                    ));
                    tags.push(mktag(
                        "FLV",
                        "AudioSampleRate",
                        "Audio Sample Rate",
                        Value::String(sample_rate.to_string()),
                    ));
                    tags.push(mktag(
                        "FLV",
                        "AudioBitsPerSample",
                        "Audio Bits Per Sample",
                        Value::String(bits.to_string()),
                    ));
                    tags.push(mktag(
                        "FLV",
                        "AudioChannels",
                        "Audio Channels",
                        Value::String(channels.to_string()),
                    ));
                    tags.push(mktag(
                        "FLV",
                        "AudioEncoding",
                        "Audio Encoding",
                        Value::String(codec_name.to_string()),
                    ));
                    audio_info_found = true;
                }
            }
            0x09 if !video_info_found => {
                // Video tag: first byte = codec info
                if data_size >= 1 {
                    let info_byte = data[tag_start];
                    let codec_id = info_byte & 0x0f;
                    let codec_name = match codec_id {
                        2 => "Sorenson H.263",
                        3 => "Screen video",
                        4 => "On2 VP6",
                        5 => "On2 VP6 with alpha",
                        6 => "Screen video v2",
                        7 => "H.264",
                        _ => "Unknown",
                    };
                    tags.push(mktag(
                        "FLV",
                        "VideoCodecID",
                        "Video Codec ID",
                        Value::String(format!("{}", codec_id)),
                    ));
                    tags.push(mktag(
                        "FLV",
                        "VideoEncoding",
                        "Video Encoding",
                        Value::String(codec_name.to_string()),
                    ));
                    video_info_found = true;
                }
            }
            _ => {}
        }

        // Move to next tag: skip data + prev_tag_size(4)
        pos = tag_end + 4;
    }

    // Add HasAudio/HasVideo from header flags
    if has_audio && !tags.iter().any(|t| t.name == "HasAudio") {
        tags.push(mktag(
            "FLV",
            "HasAudio",
            "Has Audio",
            Value::String("Yes".into()),
        ));
    }
    if has_video && !tags.iter().any(|t| t.name == "HasVideo") {
        tags.push(mktag(
            "FLV",
            "HasVideo",
            "Has Video",
            Value::String("Yes".into()),
        ));
    }

    // Deduplicate: keep the last occurrence of each tag name (mirrors Perl's hash-based storage)
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut deduped: Vec<Tag> = Vec::with_capacity(tags.len());
    for tag in tags.into_iter().rev() {
        if seen.insert(tag.name.clone()) {
            deduped.push(tag);
        }
    }
    deduped.reverse();

    Ok(deduped)
}

/// Parse AMF metadata from FLV script tag.
/// AMF format: type_byte + value...
/// type 0x02 = string (2-byte BE len + bytes)
/// type 0x08 = ECMAArray (4-byte count + key-value pairs until 0x00 0x00 0x09)
fn flv_parse_amf_metadata(data: &[u8], tags: &mut Vec<Tag>) {
    let mut pos = 0;

    // First value should be string "onMetaData"
    if pos + 3 > data.len() || data[pos] != 0x02 {
        return;
    }
    pos += 1;
    let str_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
    pos += 2;
    if pos + str_len > data.len() {
        return;
    }
    let name = crate::encoding::decode_utf8_or_latin1(&data[pos..pos + str_len]).to_string();
    pos += str_len;

    if name != "onMetaData" {
        return;
    }

    // Second value should be ECMAArray (0x08) or Object (0x03)
    if pos >= data.len() {
        return;
    }
    let container_type = data[pos];
    pos += 1;

    if container_type == 0x08 {
        // ECMAArray: 4-byte count, then key-value pairs
        if pos + 4 > data.len() {
            return;
        }
        pos += 4; // skip array count
    } else if container_type == 0x03 {
        // Object: just key-value pairs
    } else {
        return;
    }

    // Parse key-value pairs until we hit 0x00 0x00 0x09 (end marker)
    flv_parse_amf_object(data, &mut pos, tags, "");
}

/// Parse an AMF value at *pos, advancing pos.
/// For struct types (0x03, 0x08), emits sub-tags directly.
/// compound_key: the raw key built for this value (for tag name lookup).
/// struct_name: prefix for nested object keys (e.g. "CuePoint0", "keyframes").
fn flv_parse_amf_value(
    data: &[u8],
    pos: &mut usize,
    tags: &mut Vec<Tag>,
    compound_key: &str,
    struct_name: &str,
) {
    if *pos >= data.len() {
        return;
    }
    let val_type = data[*pos];
    *pos += 1;

    match val_type {
        0x00 => {
            if *pos + 8 > data.len() {
                return;
            }
            let bytes: [u8; 8] = [
                data[*pos],
                data[*pos + 1],
                data[*pos + 2],
                data[*pos + 3],
                data[*pos + 4],
                data[*pos + 5],
                data[*pos + 6],
                data[*pos + 7],
            ];
            let val = f64::from_be_bytes(bytes);
            *pos += 8;
            let tag_name = flv_lookup_tag(compound_key);
            let val_str = flv_apply_conv(&tag_name, val);
            tags.push(mktag("FLV", &tag_name, &tag_name, Value::String(val_str)));
        }
        0x01 => {
            if *pos >= data.len() {
                return;
            }
            let b = data[*pos] != 0;
            *pos += 1;
            let tag_name = flv_lookup_tag(compound_key);
            tags.push(mktag(
                "FLV",
                &tag_name,
                &tag_name,
                Value::String(if b { "Yes" } else { "No" }.to_string()),
            ));
        }
        0x02 => {
            if *pos + 2 > data.len() {
                return;
            }
            let slen = u16::from_be_bytes([data[*pos], data[*pos + 1]]) as usize;
            *pos += 2;
            if *pos + slen > data.len() {
                return;
            }
            let s = crate::encoding::decode_utf8_or_latin1(&data[*pos..*pos + slen]).to_string();
            *pos += slen;
            let tag_name = flv_lookup_tag(compound_key);
            let s = s.trim_end().to_string();
            tags.push(mktag("FLV", &tag_name, &tag_name, Value::String(s)));
        }
        0x03 | 0x08 => {
            if val_type == 0x08 {
                if *pos + 4 > data.len() {
                    return;
                }
                *pos += 4;
            }
            flv_parse_amf_object(data, pos, tags, struct_name);
        }
        0x09 => { /* end marker, ignore */ }
        0x0a => {
            if *pos + 4 > data.len() {
                return;
            }
            let count =
                u32::from_be_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]])
                    as usize;
            *pos += 4;
            let mut items: Vec<String> = Vec::new();
            for i in 0..count {
                if *pos >= data.len() {
                    break;
                }
                let item_type = data[*pos];
                if item_type == 0x03 || item_type == 0x08 {
                    let indexed_name = format!("{}{}", struct_name, i);
                    *pos += 1;
                    if item_type == 0x08 {
                        if *pos + 4 > data.len() {
                            break;
                        }
                        *pos += 4;
                    }
                    flv_parse_amf_object(data, pos, tags, &indexed_name);
                } else {
                    *pos += 1;
                    match item_type {
                        0x00 => {
                            if *pos + 8 > data.len() {
                                break;
                            }
                            let bytes: [u8; 8] = [
                                data[*pos],
                                data[*pos + 1],
                                data[*pos + 2],
                                data[*pos + 3],
                                data[*pos + 4],
                                data[*pos + 5],
                                data[*pos + 6],
                                data[*pos + 7],
                            ];
                            let v = f64::from_be_bytes(bytes);
                            *pos += 8;
                            items.push(flv_format_number(v));
                        }
                        0x01 => {
                            if *pos >= data.len() {
                                break;
                            }
                            let b = data[*pos] != 0;
                            *pos += 1;
                            items.push(if b { "Yes" } else { "No" }.to_string());
                        }
                        0x02 => {
                            if *pos + 2 > data.len() {
                                break;
                            }
                            let slen = u16::from_be_bytes([data[*pos], data[*pos + 1]]) as usize;
                            *pos += 2;
                            if *pos + slen > data.len() {
                                break;
                            }
                            let s =
                                crate::encoding::decode_utf8_or_latin1(&data[*pos..*pos + slen])
                                    .to_string();
                            *pos += slen;
                            items.push(s);
                        }
                        _ => {
                            *pos = data.len();
                            break;
                        }
                    }
                }
            }
            if !items.is_empty() {
                let tag_name = flv_lookup_tag(compound_key);
                tags.push(mktag(
                    "FLV",
                    &tag_name,
                    &tag_name,
                    Value::String(items.join(", ")),
                ));
            }
        }
        0x0b => {
            if *pos + 10 > data.len() {
                return;
            }
            let ms = f64::from_be_bytes([
                data[*pos],
                data[*pos + 1],
                data[*pos + 2],
                data[*pos + 3],
                data[*pos + 4],
                data[*pos + 5],
                data[*pos + 6],
                data[*pos + 7],
            ]);
            let tz_offset = i16::from_be_bytes([data[*pos + 8], data[*pos + 9]]) as i32;
            *pos += 10;
            let s = flv_format_date(ms, tz_offset);
            let tag_name = flv_lookup_tag(compound_key);
            tags.push(mktag("FLV", &tag_name, &tag_name, Value::String(s)));
        }
        0x0c | 0x0f => {
            if *pos + 4 > data.len() {
                return;
            }
            let slen =
                u32::from_be_bytes([data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]])
                    as usize;
            *pos += 4;
            if *pos + slen > data.len() {
                return;
            }
            let s = crate::encoding::decode_utf8_or_latin1(&data[*pos..*pos + slen]).to_string();
            *pos += slen;
            let tag_name = flv_lookup_tag(compound_key);
            tags.push(mktag("FLV", &tag_name, &tag_name, Value::String(s)));
        }
        0x05 | 0x06 => { /* null/undefined, no value bytes */ }
        _ => {
            *pos = data.len();
        }
    }
}

fn flv_parse_amf_object(data: &[u8], pos: &mut usize, tags: &mut Vec<Tag>, struct_name: &str) {
    while *pos + 3 <= data.len() {
        if data[*pos] == 0x00
            && data[*pos + 1] == 0x00
            && *pos + 2 < data.len()
            && data[*pos + 2] == 0x09
        {
            *pos += 3;
            break;
        }
        if *pos + 2 > data.len() {
            break;
        }
        let key_len = u16::from_be_bytes([data[*pos], data[*pos + 1]]) as usize;
        *pos += 2;
        if *pos + key_len > data.len() {
            break;
        }
        let key = crate::encoding::decode_utf8_or_latin1(&data[*pos..*pos + key_len]).to_string();
        *pos += key_len;
        if *pos >= data.len() {
            break;
        }

        // Build compound key: structName + ucfirst(mapped_key), mirrors Perl's:
        //   $tag = $$tagInfo{Name} if SubDirectory
        //   $tag = $structName . ucfirst($tag) if defined $structName
        //   StructName = $tag
        let (compound_key, nested_struct) = flv_build_compound_key(struct_name, &key);

        flv_parse_amf_value(data, pos, tags, &compound_key, &nested_struct);
    }
}

/// Build (compound_key, nested_struct_name) for a given struct_name + raw key.
/// Mirrors Perl AMF ProcessMeta logic:
///   - At top level (struct_name empty): compound_key = raw_key
///   - Nested: compound_key = struct_name + ucfirst(mapped_sub_key)
///   - nested_struct = compound_key for most cases (used as StructName in Perl)
///   - Exception: SubDirectory entries (cuePoints) → nested_struct = Name = "CuePoint"
fn flv_build_compound_key(struct_name: &str, raw_key: &str) -> (String, String) {
    if struct_name.is_empty() {
        // Top-level key
        let compound_key = raw_key.to_string();
        // SubDirectory entries use their Name as the nested struct_name
        let nested_struct = match raw_key {
            "cuePoints" => "CuePoint".to_string(),
            _ => raw_key.to_string(),
        };
        (compound_key, nested_struct)
    } else {
        // Nested key: map through sub-key table first
        let mapped_key = flv_map_sub_key(struct_name, raw_key);
        let uckey = flv_ucfirst(&mapped_key);
        let compound_key = format!("{}{}", struct_name, uckey);
        // nested_struct is the compound_key (raw, used as StructName)
        let nested_struct = compound_key.clone();
        (compound_key, nested_struct)
    }
}

/// Map a raw sub-key through the appropriate subtable based on the parent struct_name.
/// e.g. inside a CuePoint struct: "parameters" → "Parameter"
fn flv_map_sub_key(struct_name: &str, key: &str) -> String {
    // CuePointN struct (but not CuePointNParameter*)
    if let Some(rest) = struct_name.strip_prefix("CuePoint") {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() && !rest[digits.len()..].starts_with("Parameter") {
            // Inside CuePoint subtable
            return match key {
                "name" => "Name".to_string(),
                "type" => "Type".to_string(),
                "time" => "Time".to_string(),
                "parameters" => "Parameter".to_string(),
                _ => key.to_string(),
            };
        }
    }
    key.to_string()
}

fn flv_lookup_tag(key: &str) -> String {
    match key {
        "audiocodecid" => return "AudioCodecID".to_string(),
        "audiodatarate" => return "AudioBitrate".to_string(),
        "audiodelay" => return "AudioDelay".to_string(),
        "audiosamplerate" => return "AudioSampleRate".to_string(),
        "audiosamplesize" => return "AudioSampleSize".to_string(),
        "audiosize" => return "AudioSize".to_string(),
        "bytelength" => return "ByteLength".to_string(),
        "canseekontime" => return "CanSeekOnTime".to_string(),
        "canSeekToEnd" => return "CanSeekToEnd".to_string(),
        "creationdate" => return "CreateDate".to_string(),
        "createdby" => return "CreatedBy".to_string(),
        "cuePoints" => return "CuePoint".to_string(),
        "datasize" => return "DataSize".to_string(),
        "duration" => return "Duration".to_string(),
        "filesize" => return "FileSizeBytes".to_string(),
        "framerate" => return "FrameRate".to_string(),
        "hasAudio" => return "HasAudio".to_string(),
        "hasCuePoints" => return "HasCuePoints".to_string(),
        "hasKeyframes" => return "HasKeyFrames".to_string(),
        "hasMetadata" => return "HasMetadata".to_string(),
        "hasVideo" => return "HasVideo".to_string(),
        "height" => return "ImageHeight".to_string(),
        "httphostheader" => return "HTTPHostHeader".to_string(),
        "keyframesTimes" => return "KeyFramesTimes".to_string(),
        "keyframesFilepositions" => return "KeyFramePositions".to_string(),
        "lasttimestamp" => return "LastTimeStamp".to_string(),
        "lastkeyframetimestamp" => return "LastKeyFrameTime".to_string(),
        "metadatacreator" => return "MetadataCreator".to_string(),
        "metadatadate" => return "MetadataDate".to_string(),
        "purl" => return "URL".to_string(),
        "pmsg" => return "Message".to_string(),
        "sourcedata" => return "SourceData".to_string(),
        "starttime" => return "StartTime".to_string(),
        "stereo" => return "Stereo".to_string(),
        "totaldatarate" => return "TotalDataRate".to_string(),
        "totalduration" => return "TotalDuration".to_string(),
        "videocodecid" => return "VideoCodecID".to_string(),
        "videodatarate" => return "VideoBitrate".to_string(),
        "videosize" => return "VideoSize".to_string(),
        "width" => return "ImageWidth".to_string(),
        _ => {}
    }
    flv_ucfirst(key)
}

fn flv_apply_conv(tag_name: &str, val: f64) -> String {
    match tag_name {
        "AudioBitrate" => flv_convert_bitrate(val * 1000.0),
        "VideoBitrate" => flv_convert_bitrate(val * 1000.0),
        "Duration" | "StartTime" | "TotalDuration" => flv_convert_duration(val),
        "FrameRate" => {
            let rounded = (val * 1000.0 + 0.5).floor() / 1000.0;
            flv_format_number(rounded)
        }
        _ => flv_format_number(val),
    }
}

pub(crate) fn flv_convert_bitrate(bps: f64) -> String {
    // Mirrors Perl's ConvertBitrate: divide by 1000 until < 1000,
    // then %.0f if >= 100, else %.3g (3 significant digits)
    let mut val = bps;
    let mut units = "bps";
    for u in &["bps", "kbps", "Mbps", "Gbps"] {
        units = u;
        if val < 1000.0 {
            break;
        }
        val /= 1000.0;
    }
    if val >= 100.0 {
        format!("{:.0} {}", val, units)
    } else {
        // 3 significant digits
        let s = format_3g(val);
        format!("{} {}", s, units)
    }
}

fn format_3g(val: f64) -> String {
    if val == 0.0 {
        return "0".to_string();
    }
    // 3 significant digits
    let magnitude = val.abs().log10().floor() as i32;
    let factor = 10f64.powi(2 - magnitude);
    let rounded = (val * factor).round() / factor;
    if rounded.fract() == 0.0 {
        format!("{:.0}", rounded)
    } else {
        let s = format!("{:.6}", rounded);
        let s = s.trim_end_matches('0').trim_end_matches('.');
        s.to_string()
    }
}

pub(crate) fn flv_convert_duration(secs: f64) -> String {
    if secs == 0.0 {
        return "0 s".to_string();
    }
    let sign = if secs < 0.0 { "-" } else { "" };
    let t = secs.abs();
    if t < 30.0 {
        return format!("{}{:.2} s", sign, t);
    }
    let t = t + 0.5; // round to nearest second
    let h = (t / 3600.0) as u64;
    let t = t - (h as f64) * 3600.0;
    let m = (t / 60.0) as u64;
    let t = t - (m as f64) * 60.0;
    if h > 24 {
        let d = h / 24;
        let h = h - d * 24;
        format!("{}{}d {}:{:02}:{:02}", sign, d, h, m, t as u64)
    } else {
        format!("{}{}:{:02}:{:02}", sign, h, m, t as u64)
    }
}

fn flv_format_number(val: f64) -> String {
    if val.fract() == 0.0 && val.abs() < 1e15 {
        format!("{}", val as i64)
    } else {
        let s = format!("{:.10}", val);
        let s = s.trim_end_matches('0');
        let s = s.trim_end_matches('.');
        s.to_string()
    }
}

fn flv_ucfirst(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => {
            let upper: String = c.to_uppercase().collect();
            upper + chars.as_str()
        }
    }
}

fn flv_format_date(ms: f64, tz_offset_minutes: i32) -> String {
    // Perl: ConvertUnixTime($val/1000, 0, 6) - convert to datetime with 6 decimal places
    let unix_secs = ms / 1000.0;
    let whole_secs = unix_secs.floor() as i64;
    // Microseconds from fractional part (6 decimal places)
    let usec = ((unix_secs - unix_secs.floor()) * 1_000_000.0).round() as u64;

    let epoch_to_ymdhms = |ts: i64| -> (i32, u32, u32, u32, u32, u32) {
        let days = ts / 86400;
        let rem_secs = ts % 86400;
        let hours = rem_secs / 3600;
        let mins = (rem_secs % 3600) / 60;
        let secs = rem_secs % 60;
        let mut year = 1970i32;
        let mut remaining_days = days;
        loop {
            let days_in_year = if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                366
            } else {
                365
            };
            if remaining_days < days_in_year {
                break;
            }
            remaining_days -= days_in_year;
            year += 1;
        }
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let month_days: [i64; 12] = [
            31,
            if leap { 29 } else { 28 },
            31,
            30,
            31,
            30,
            31,
            31,
            30,
            31,
            30,
            31,
        ];
        let mut month = 1u32;
        let mut day = remaining_days + 1;
        for &md in &month_days {
            if day > md {
                day -= md;
                month += 1;
            } else {
                break;
            }
        }
        (
            year,
            month,
            day as u32,
            hours as u32,
            mins as u32,
            secs as u32,
        )
    };

    let (year, month, day, hours, mins, secs) = epoch_to_ymdhms(whole_secs);
    let tz_hours = tz_offset_minutes.abs() / 60;
    let tz_mins = tz_offset_minutes.abs() % 60;
    let tz_sign = if tz_offset_minutes >= 0 { "+" } else { "-" };

    if usec != 0 {
        format!(
            "{:04}:{:02}:{:02} {:02}:{:02}:{:02}.{:06}{}{:02}:{:02}",
            year, month, day, hours, mins, secs, usec, tz_sign, tz_hours, tz_mins
        )
    } else {
        format!(
            "{:04}:{:02}:{:02} {:02}:{:02}:{:02}{}{:02}:{:02}",
            year, month, day, hours, mins, secs, tz_sign, tz_hours, tz_mins
        )
    }
}
