//! RealMedia format reader.

use super::flv::{flv_convert_bitrate, flv_convert_duration};
use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

pub fn read_real_media(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 8 || !data.starts_with(b".RMF") {
        return Err(Error::InvalidData("not a RealMedia file".into()));
    }

    let mut tags = Vec::new();

    // Skip .RMF header (size at bytes 4..8)
    if data.len() < 8 {
        return Ok(tags);
    }
    let hdr_size = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let hdr_size = hdr_size.max(8);
    let mut pos = hdr_size;
    let mut first_mdpr = true;

    // Look for RJMD at specific position based on RMJE footer
    let rjmd_data_opt = real_find_rjmd(data);

    // Process chunks
    while pos + 10 <= data.len() {
        let chunk_id = &data[pos..pos + 4];
        if chunk_id == b"\x00\x00\x00\x00" {
            break;
        }
        let chunk_size =
            u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
                as usize;
        if chunk_size < 10 || pos + chunk_size > data.len() {
            break;
        }
        if chunk_id == b"DATA" {
            break;
        }

        let chunk_data = &data[pos + 10..pos + chunk_size];

        match chunk_id {
            b"PROP" => real_parse_prop(chunk_data, &mut tags),
            b"MDPR" => {
                real_parse_mdpr(chunk_data, &mut tags, first_mdpr);
                first_mdpr = false;
            }
            b"CONT" => real_parse_cont(chunk_data, &mut tags),
            _ => {}
        }

        pos += chunk_size;
    }

    // Process RJMD metadata
    if let Some(rjmd_data) = rjmd_data_opt {
        real_parse_rjmd(&rjmd_data, &mut tags);
    }

    // Check for ID3v1 at last 128 bytes
    if data.len() >= 128 && data[data.len() - 128..data.len() - 125] == *b"TAG" {
        let id3_data = &data[data.len() - 128..];
        real_parse_id3v1(id3_data, &mut tags);
    }

    Ok(tags)
}

fn real_find_rjmd(data: &[u8]) -> Option<Vec<u8>> {
    // Perl: seek(-140, 2) read 12 bytes, check for "RMJE"
    if data.len() < 140 {
        return None;
    }
    let rmje_pos = data.len() - 140;
    if &data[rmje_pos..rmje_pos + 4] != b"RMJE" {
        return None;
    }
    let meta_size = u32::from_be_bytes([
        data[rmje_pos + 8],
        data[rmje_pos + 9],
        data[rmje_pos + 10],
        data[rmje_pos + 11],
    ]) as usize;
    // RJMD starts at rmje_pos - meta_size
    if meta_size > rmje_pos {
        return None;
    }
    let rjmd_start = rmje_pos - meta_size;
    if rjmd_start + 4 > data.len() || &data[rjmd_start..rjmd_start + 4] != b"RJMD" {
        return None;
    }
    Some(data[rjmd_start..rjmd_start + meta_size].to_vec())
}

fn real_parse_prop(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 40 {
        return;
    }
    let mut off = 0usize;
    let max_bitrate = u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
    off += 4;
    let avg_bitrate = u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
    off += 4;
    let max_pkt = u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
    off += 4;
    let avg_pkt = u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
    off += 4;
    let num_pkts = u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
    off += 4;
    let duration_ms = u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
    off += 4;
    let preroll_ms = u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
    off += 4;
    off += 4; // index offset (unknown)
    off += 4; // data offset (unknown)
    if data.len() < off + 4 {
        return;
    }
    let num_streams = u16::from_be_bytes([data[off], data[off + 1]]);
    off += 2;
    if data.len() < off + 2 {
        return;
    }
    let flags = u16::from_be_bytes([data[off], data[off + 1]]);

    tags.push(mktag(
        "Real",
        "MaxBitrate",
        "Max Bitrate",
        Value::String(real_convert_bitrate(max_bitrate as f64)),
    ));
    tags.push(mktag(
        "Real",
        "AvgBitrate",
        "Avg Bitrate",
        Value::String(real_convert_bitrate(avg_bitrate as f64)),
    ));
    tags.push(mktag(
        "Real",
        "MaxPacketSize",
        "Max Packet Size",
        Value::U32(max_pkt),
    ));
    tags.push(mktag(
        "Real",
        "AvgPacketSize",
        "Avg Packet Size",
        Value::U32(avg_pkt),
    ));
    tags.push(mktag(
        "Real",
        "NumPackets",
        "Num Packets",
        Value::U32(num_pkts),
    ));
    // Duration: ms / 1000, then ConvertDuration
    let dur_secs = duration_ms as f64 / 1000.0;
    tags.push(mktag(
        "Real",
        "Duration",
        "Duration",
        Value::String(real_convert_duration(dur_secs)),
    ));
    let preroll_secs = preroll_ms as f64 / 1000.0;
    tags.push(mktag(
        "Real",
        "Preroll",
        "Preroll",
        Value::String(real_convert_duration(preroll_secs)),
    ));
    tags.push(mktag(
        "Real",
        "NumStreams",
        "Num Streams",
        Value::U16(num_streams),
    ));

    // Flags BITMASK
    let mut flag_strs = Vec::new();
    if flags & 0x01 != 0 {
        flag_strs.push("Allow Recording");
    }
    if flags & 0x02 != 0 {
        flag_strs.push("Perfect Play");
    }
    if flags & 0x04 != 0 {
        flag_strs.push("Live");
    }
    if flags & 0x08 != 0 {
        flag_strs.push("Allow Download");
    }
    if !flag_strs.is_empty() {
        tags.push(mktag(
            "Real",
            "Flags",
            "Flags",
            Value::String(flag_strs.join(", ")),
        ));
    }
}

fn real_parse_mdpr(data: &[u8], tags: &mut Vec<Tag>, is_first: bool) {
    if data.len() < 30 {
        return;
    }
    let mut off = 0usize;
    let stream_num = u16::from_be_bytes([data[off], data[off + 1]]);
    off += 2;
    let max_bitrate = u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
    off += 4;
    let avg_bitrate = u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
    off += 4;
    let max_pkt = u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
    off += 4;
    let avg_pkt = u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
    off += 4;
    let start_time = u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
    off += 4;
    let preroll_ms = u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
    off += 4;
    let duration_ms = u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
    off += 4;
    if off >= data.len() {
        return;
    }
    let name_len = data[off] as usize;
    off += 1;
    if off + name_len > data.len() {
        return;
    }
    let stream_name =
        crate::encoding::decode_utf8_or_latin1(&data[off..off + name_len]).to_string();
    off += name_len;
    if off >= data.len() {
        return;
    }
    let mime_len = data[off] as usize;
    off += 1;
    if off + mime_len > data.len() {
        return;
    }
    let mime_type = crate::encoding::decode_utf8_or_latin1(&data[off..off + mime_len]).to_string();
    off += mime_len;

    // Only emit stream info for first non-logical stream (Perl PRIORITY => 0 = first takes priority)
    if is_first {
        tags.push(mktag(
            "Real",
            "StreamNumber",
            "Stream Number",
            Value::U16(stream_num),
        ));
        tags.push(mktag(
            "Real",
            "StreamMaxBitrate",
            "Stream Max Bitrate",
            Value::String(real_convert_bitrate(max_bitrate as f64)),
        ));
        tags.push(mktag(
            "Real",
            "StreamAvgBitrate",
            "Stream Avg Bitrate",
            Value::String(real_convert_bitrate(avg_bitrate as f64)),
        ));
        tags.push(mktag(
            "Real",
            "StreamMaxPacketSize",
            "Stream Max Packet Size",
            Value::U32(max_pkt),
        ));
        tags.push(mktag(
            "Real",
            "StreamAvgPacketSize",
            "Stream Avg Packet Size",
            Value::U32(avg_pkt),
        ));
        tags.push(mktag(
            "Real",
            "StreamStartTime",
            "Stream Start Time",
            Value::U32(start_time),
        ));
        let preroll_secs = preroll_ms as f64 / 1000.0;
        tags.push(mktag(
            "Real",
            "StreamPreroll",
            "Stream Preroll",
            Value::String(real_convert_duration(preroll_secs)),
        ));
        let dur_secs = duration_ms as f64 / 1000.0;
        tags.push(mktag(
            "Real",
            "StreamDuration",
            "Stream Duration",
            Value::String(real_convert_duration(dur_secs)),
        ));
        tags.push(mktag(
            "Real",
            "StreamName",
            "Stream Name",
            Value::String(stream_name),
        ));
        tags.push(mktag(
            "Real",
            "StreamMimeType",
            "Stream Mime Type",
            Value::String(mime_type.clone()),
        ));
    }

    // Check for logical-fileinfo stream
    if mime_type == "logical-fileinfo" && off + 12 <= data.len() {
        real_parse_fileinfo(&data[off..], tags);
    }
}

fn real_parse_fileinfo(data: &[u8], tags: &mut Vec<Tag>) {
    // FileInfoLen (tag 12, 4 bytes) = type_spec_size,
    // FileInfoLen2 (tag 13, 4 bytes) = first 4 bytes of type_spec_data,
    // FileInfoVersion (tag 14, 2 bytes), PhysicalStreams (tag 15, 2 bytes),
    // [stream_nums(2)*N + data_offsets(4)*N], num_rules(2), [rule_nums(2)*N], num_props(2)
    if data.len() < 12 {
        return;
    }
    let mut off = 0usize;
    // FileInfoLen (tag 12): type_spec_size
    let _file_info_len =
        u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
    off += 4;
    // FileInfoLen2 (tag 13): first 4 bytes of type_spec_data (conditional on logical-fileinfo)
    let _file_info_len2 =
        u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
    off += 4;
    // FileInfoVersion (tag 14): int16u
    let fi_ver = u16::from_be_bytes([data[off], data[off + 1]]);
    off += 2;
    // PhysicalStreams (tag 15): int16u
    let phys_streams = u16::from_be_bytes([data[off], data[off + 1]]) as usize;
    off += 2;
    // Skip physical stream numbers (2 bytes each) and data offsets (4 bytes each)
    off += phys_streams * 2 + phys_streams * 4;
    if off + 2 > data.len() {
        return;
    }
    let num_rules = u16::from_be_bytes([data[off], data[off + 1]]) as usize;
    off += 2;
    // Skip rule map
    off += num_rules * 2;
    if off + 2 > data.len() {
        return;
    }
    let _num_props = u16::from_be_bytes([data[off], data[off + 1]]);
    off += 2;

    tags.push(mktag(
        "Real",
        "FileInfoVersion",
        "File Info Version",
        Value::U16(fi_ver),
    ));

    // Now parse FileInfoProperties
    real_parse_properties(&data[off..], tags);
}

fn real_parse_properties(data: &[u8], tags: &mut Vec<Tag>) {
    let mut pos = 0usize;
    while pos + 7 <= data.len() {
        let p_start = pos;
        let p_size =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let p_ver = u16::from_be_bytes([data[pos + 4], data[pos + 5]]);
        if p_size < 7 || p_start + p_size > data.len() {
            break;
        }
        if p_ver != 0 {
            pos = p_start + p_size;
            continue;
        }
        pos += 6;

        let tag_len = data[pos] as usize;
        pos += 1;
        if pos + tag_len > data.len() {
            break;
        }
        let tag_name =
            crate::encoding::decode_utf8_or_latin1(&data[pos..pos + tag_len]).to_string();
        pos += tag_len;

        if pos + 6 > data.len() {
            break;
        }
        let prop_type =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        pos += 4;
        let val_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;
        if pos + val_len > data.len() {
            break;
        }
        let val_data = &data[pos..pos + val_len];

        let (exif_name, val_str) = real_file_info_tag(&tag_name, prop_type, val_data, val_len);
        if let Some(val) = val_str {
            if !exif_name.is_empty() {
                tags.push(mktag("Real", &exif_name, &exif_name, Value::String(val)));
            }
        }

        pos = p_start + p_size;
    }
}

fn real_file_info_tag(
    tag: &str,
    prop_type: u32,
    val_data: &[u8],
    val_len: usize,
) -> (String, Option<String>) {
    let tag_name = match tag {
        "Content Rating" => "ContentRating",
        "Audiences" => "Audiences",
        "audioMode" => "AudioMode",
        "Creation Date" => "CreateDate",
        "Generated By" => "Software",
        "Modification Date" => "ModifyDate",
        "videoMode" => "VideoMode",
        "Description" => "Description",
        "Keywords" => "Keywords",
        "Indexable" => "Indexable",
        "File ID" => "FileID",
        "Target Audiences" => "TargetAudiences",
        "Audio Format" => "AudioFormat",
        "Video Quality" => "VideoQuality",
        _ => {
            // Remove spaces, ucfirst
            let s: String = tag.split_whitespace().collect::<Vec<_>>().join("");
            return (
                ucfirst_first_char(&s),
                real_parse_prop_value(tag, prop_type, val_data, val_len),
            );
        }
    };

    let val = match tag_name {
        "ContentRating" => {
            if prop_type == 0 && val_len >= 4 {
                let v = u32::from_be_bytes([val_data[0], val_data[1], val_data[2], val_data[3]]);
                Some(match v {
                    0 => "No Rating".to_string(),
                    1 => "All Ages".to_string(),
                    2 => "Older Children".to_string(),
                    3 => "Younger Teens".to_string(),
                    4 => "Older Teens".to_string(),
                    5 => "Adult Supervision Recommended".to_string(),
                    6 => "Adults Only".to_string(),
                    _ => format!("{}", v),
                })
            } else {
                None
            }
        }
        "CreateDate" | "ModifyDate" => {
            // Convert "D/M/YYYY H:MM:SS" to "YYYY:MM:DD HH:MM:SS"
            if prop_type == 2 {
                let s = crate::encoding::decode_utf8_or_latin1(val_data)
                    .trim_matches('\0')
                    .to_string();
                Some(real_parse_date(&s))
            } else {
                None
            }
        }
        _ => real_parse_prop_value(tag_name, prop_type, val_data, val_len),
    };

    (tag_name.to_string(), val)
}

fn real_parse_prop_value(
    tag: &str,
    prop_type: u32,
    val_data: &[u8],
    val_len: usize,
) -> Option<String> {
    let _ = tag;
    if val_len == 0 {
        return Some(String::new());
    }
    match prop_type {
        0 => {
            // int32u
            if val_len >= 4 {
                Some(format!(
                    "{}",
                    u32::from_be_bytes([val_data[0], val_data[1], val_data[2], val_data[3]])
                ))
            } else {
                None
            }
        }
        2 => {
            // string
            let s = crate::encoding::decode_utf8_or_latin1(val_data)
                .trim_matches('\0')
                .to_string();
            Some(s)
        }
        _ => None,
    }
}

fn real_parse_date(s: &str) -> String {
    // Parse "D/M/YYYY H:MM:SS" or "DD/MM/YYYY HH:MM:SS"
    let parts: Vec<&str> = s.split(['/', ' ', ':']).collect();
    if parts.len() >= 6 {
        let day: u32 = parts[0].parse().unwrap_or(0);
        let month: u32 = parts[1].parse().unwrap_or(0);
        let year: u32 = parts[2].parse().unwrap_or(0);
        let hour: u32 = parts[3].parse().unwrap_or(0);
        let min: u32 = parts[4].parse().unwrap_or(0);
        let sec: u32 = parts[5].parse().unwrap_or(0);
        format!(
            "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
            year, month, day, hour, min, sec
        )
    } else {
        s.to_string()
    }
}

fn real_parse_cont(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 8 {
        return;
    }
    let mut off = 0usize;

    let title_len = u16::from_be_bytes([data[off], data[off + 1]]) as usize;
    off += 2;
    if off + title_len > data.len() {
        return;
    }
    let title = crate::encoding::decode_utf8_or_latin1(&data[off..off + title_len]).to_string();
    off += title_len;

    if off + 2 > data.len() {
        return;
    }
    let author_len = u16::from_be_bytes([data[off], data[off + 1]]) as usize;
    off += 2;
    if off + author_len > data.len() {
        return;
    }
    let author = crate::encoding::decode_utf8_or_latin1(&data[off..off + author_len]).to_string();
    off += author_len;

    if off + 2 > data.len() {
        return;
    }
    let copyright_len = u16::from_be_bytes([data[off], data[off + 1]]) as usize;
    off += 2;
    if off + copyright_len > data.len() {
        return;
    }
    let copyright =
        crate::encoding::decode_utf8_or_latin1(&data[off..off + copyright_len]).to_string();
    off += copyright_len;

    if off + 2 > data.len() {
        return;
    }
    let comment_len = u16::from_be_bytes([data[off], data[off + 1]]) as usize;
    off += 2;
    if off + comment_len > data.len() {
        return;
    }
    let comment = crate::encoding::decode_utf8_or_latin1(&data[off..off + comment_len]).to_string();

    if !title.is_empty() {
        tags.push(mktag("Real", "Title", "Title", Value::String(title)));
    }
    if !author.is_empty() {
        tags.push(mktag("Real", "Author", "Author", Value::String(author)));
    }
    if !copyright.is_empty() {
        tags.push(mktag(
            "Real",
            "Copyright",
            "Copyright",
            Value::String(copyright),
        ));
    }
    if !comment.is_empty() {
        tags.push(mktag("Real", "Comment", "Comment", Value::String(comment)));
    }
}

fn real_parse_rjmd(data: &[u8], tags: &mut Vec<Tag>) {
    // data starts with RJMD header: RJMD(4) + version(4) + data_len(4) + metadata
    // ProcessRealMeta DirStart=8, DirLen=data.len()-8
    if data.len() < 8 {
        return;
    }
    let dir_start = 8usize;
    let dir_end = data.len();
    real_parse_rjmd_entries(data, dir_start, dir_end, "", tags);
}

fn real_parse_rjmd_entries(
    data: &[u8],
    pos_start: usize,
    dir_end: usize,
    prefix: &str,
    tags: &mut Vec<Tag>,
) {
    let mut pos = pos_start;
    while pos + 28 <= dir_end {
        if pos + 28 > data.len() {
            break;
        }
        let entry_size =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let entry_type =
            u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]);
        let _flags =
            u32::from_be_bytes([data[pos + 8], data[pos + 9], data[pos + 10], data[pos + 11]]);
        let value_pos_rel = u32::from_be_bytes([
            data[pos + 12],
            data[pos + 13],
            data[pos + 14],
            data[pos + 15],
        ]) as usize;
        let _sub_pos_rel = u32::from_be_bytes([
            data[pos + 16],
            data[pos + 17],
            data[pos + 18],
            data[pos + 19],
        ]) as usize;
        let num_sub = u32::from_be_bytes([
            data[pos + 20],
            data[pos + 21],
            data[pos + 22],
            data[pos + 23],
        ]) as usize;
        let name_len = u32::from_be_bytes([
            data[pos + 24],
            data[pos + 25],
            data[pos + 26],
            data[pos + 27],
        ]) as usize;

        if entry_size < 28 {
            break;
        }
        if pos + entry_size > dir_end {
            break;
        }
        if pos + 28 + name_len > dir_end {
            break;
        }

        let name_bytes = &data[pos + 28..pos + 28 + name_len];
        let name = crate::encoding::decode_utf8_or_latin1(name_bytes)
            .split('\0')
            .next()
            .unwrap_or("")
            .to_string();

        let full_name = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", prefix, name)
        };

        let value_pos = value_pos_rel + pos;
        if value_pos + 4 <= dir_end {
            let value_len = u32::from_be_bytes([
                data[value_pos],
                data[value_pos + 1],
                data[value_pos + 2],
                data[value_pos + 3],
            ]) as usize;
            let value_start = value_pos + 4;
            if value_start + value_len <= dir_end && entry_type != 9 && entry_type != 10 {
                // Emit value
                let val_data = &data[value_start..value_start + value_len];
                let val_str = match entry_type {
                    1 | 2 | 6 | 7 | 8 => {
                        // string/text/url/date/filename
                        let s = crate::encoding::decode_utf8_or_latin1(val_data)
                            .trim_matches('\0')
                            .to_string();
                        let s = s.trim_end().to_string();
                        if entry_type == 7 {
                            // date: YYYYMMDDHHMMSS format
                            real_parse_rjmd_date(&s)
                        } else {
                            s
                        }
                    }
                    4 => {
                        // int32u
                        if value_len >= 4 {
                            format!(
                                "{}",
                                u32::from_be_bytes([
                                    val_data[0],
                                    val_data[1],
                                    val_data[2],
                                    val_data[3]
                                ])
                            )
                        } else {
                            String::new()
                        }
                    }
                    3 => {
                        // flag
                        if value_len == 4 {
                            format!(
                                "{}",
                                u32::from_be_bytes([
                                    val_data[0],
                                    val_data[1],
                                    val_data[2],
                                    val_data[3]
                                ])
                            )
                        } else if value_len >= 1 {
                            format!("{}", val_data[0])
                        } else {
                            String::new()
                        }
                    }
                    _ => String::new(),
                };

                if !full_name.is_empty() {
                    let tag_name = real_rjmd_tag_name(&full_name);
                    if !tag_name.is_empty() {
                        tags.push(mktag("Real", &tag_name, &tag_name, Value::String(val_str)));
                    }
                }
            }

            // Process sub-properties
            if num_sub > 0 {
                let sub_dir_start = value_pos + 4 + value_len + num_sub * 8;
                let sub_dir_len = pos + entry_size - sub_dir_start;
                if sub_dir_start + sub_dir_len <= dir_end && sub_dir_len > 0 {
                    real_parse_rjmd_entries(
                        data,
                        sub_dir_start,
                        sub_dir_start + sub_dir_len,
                        &full_name,
                        tags,
                    );
                }
            }
        }

        pos += entry_size;
    }
}

fn real_parse_rjmd_date(s: &str) -> String {
    // Format: YYYYMMDDHHMMSS
    if s.len() >= 14 {
        format!(
            "{}:{}:{} {}:{}:{}",
            &s[..4],
            &s[4..6],
            &s[6..8],
            &s[8..10],
            &s[10..12],
            &s[12..14]
        )
    } else {
        s.to_string()
    }
}

fn real_rjmd_tag_name(full_name: &str) -> String {
    // Map RJMD path names to ExifTool tag names
    // Perl table entries:
    // 'Album/Name' => 'AlbumName'
    // 'Track/Category' => 'TrackCategory'
    // etc.
    match full_name {
        "Album/Name" => "AlbumName".to_string(),
        "Track/Category" => "TrackCategory".to_string(),
        "Track/Comments" => "TrackComments".to_string(),
        "Track/Lyrics" => "TrackLyrics".to_string(),
        _ => {
            // Auto-generate: strip /, remove spaces, ucfirst each component
            let clean: String = full_name
                .split('/')
                .filter(|s| !s.is_empty())
                .map(|s| {
                    // Remove spaces and capitalize
                    let cleaned: String = s
                        .chars()
                        .filter(|&c| c.is_alphanumeric() || c == '_')
                        .collect();
                    ucfirst_first_char(&cleaned)
                })
                .collect();
            clean
        }
    }
}

fn ucfirst_first_char(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => {
            let upper: String = c.to_uppercase().collect();
            upper + chars.as_str()
        }
    }
}

fn real_parse_id3v1(data: &[u8], tags: &mut Vec<Tag>) {
    // Simple ID3v1 parser (128 bytes starting with "TAG")
    // PRIORITY => 0 means these tags don't override already-set tags
    if data.len() < 128 || &data[..3] != b"TAG" {
        return;
    }

    // Title: bytes 3..33 (30 bytes)
    let title = read_null_terminated_str(&data[3..33]);
    // Artist: bytes 33..63
    let artist = read_null_terminated_str(&data[33..63]);
    // Album: bytes 63..93
    let album = read_null_terminated_str(&data[63..93]);
    // Year: bytes 93..97
    let year = read_null_terminated_str(&data[93..97]);
    // Comment: bytes 97..127 (but bytes 125-126 may be Track for ID3v1.1)
    let comment = if data[125] == 0 && data[126] != 0 {
        // ID3v1.1: track number in byte 126, comment is bytes 97..125
        read_null_terminated_str(&data[97..125])
    } else {
        read_null_terminated_str(&data[97..127])
    };
    // Genre: byte 127
    let genre_byte = data[127] as usize;

    // Collect new tags, skipping those already present (PRIORITY => 0 behavior)
    let mut new_tags: Vec<Tag> = Vec::new();
    if !title.is_empty() && !tags.iter().any(|t| t.name == "Title") {
        new_tags.push(mktag("ID3", "Title", "Title", Value::String(title)));
    }
    if !artist.is_empty() && !tags.iter().any(|t| t.name == "Artist") {
        new_tags.push(mktag("ID3", "Artist", "Artist", Value::String(artist)));
    }
    if !album.is_empty() && !tags.iter().any(|t| t.name == "Album") {
        new_tags.push(mktag("ID3", "Album", "Album", Value::String(album)));
    }
    if !year.is_empty() && !tags.iter().any(|t| t.name == "Year") {
        new_tags.push(mktag("ID3", "Year", "Year", Value::String(year)));
    }
    if !comment.is_empty() && !tags.iter().any(|t| t.name == "Comment") {
        new_tags.push(mktag("ID3", "Comment", "Comment", Value::String(comment)));
    }
    if let Some(genre_name) = id3v1_genre_name(genre_byte) {
        if !tags.iter().any(|t| t.name == "Genre") {
            new_tags.push(mktag(
                "ID3",
                "Genre",
                "Genre",
                Value::String(genre_name.to_string()),
            ));
        }
    }
    tags.extend(new_tags);
}

fn id3v1_genre_name(n: usize) -> Option<&'static str> {
    match n {
        0 => Some("Blues"),
        1 => Some("Classic Rock"),
        2 => Some("Country"),
        3 => Some("Dance"),
        4 => Some("Disco"),
        5 => Some("Funk"),
        6 => Some("Grunge"),
        7 => Some("Hip-Hop"),
        8 => Some("Jazz"),
        9 => Some("Metal"),
        10 => Some("New Age"),
        11 => Some("Oldies"),
        12 => Some("Other"),
        13 => Some("Pop"),
        14 => Some("R&B"),
        15 => Some("Rap"),
        16 => Some("Reggae"),
        17 => Some("Rock"),
        18 => Some("Techno"),
        19 => Some("Industrial"),
        20 => Some("Alternative"),
        21 => Some("Ska"),
        22 => Some("Death Metal"),
        23 => Some("Pranks"),
        24 => Some("Soundtrack"),
        25 => Some("Euro-Techno"),
        26 => Some("Ambient"),
        27 => Some("Trip-Hop"),
        28 => Some("Vocal"),
        29 => Some("Jazz+Funk"),
        30 => Some("Fusion"),
        31 => Some("Trance"),
        32 => Some("Classical"),
        33 => Some("Instrumental"),
        34 => Some("Acid"),
        35 => Some("House"),
        36 => Some("Game"),
        37 => Some("Sound Clip"),
        38 => Some("Gospel"),
        39 => Some("Noise"),
        40 => Some("Alt. Rock"),
        41 => Some("Bass"),
        42 => Some("Soul"),
        43 => Some("Punk"),
        44 => Some("Space"),
        45 => Some("Meditative"),
        46 => Some("Instrumental Pop"),
        47 => Some("Instrumental Rock"),
        48 => Some("Ethnic"),
        49 => Some("Gothic"),
        50 => Some("Darkwave"),
        51 => Some("Techno-Industrial"),
        52 => Some("Electronic"),
        53 => Some("Pop-Folk"),
        54 => Some("Eurodance"),
        55 => Some("Dream"),
        56 => Some("Southern Rock"),
        57 => Some("Comedy"),
        58 => Some("Cult"),
        59 => Some("Gangsta Rap"),
        60 => Some("Top 40"),
        61 => Some("Christian Rap"),
        62 => Some("Pop/Funk"),
        63 => Some("Jungle"),
        64 => Some("Native American"),
        65 => Some("Cabaret"),
        66 => Some("New Wave"),
        67 => Some("Psychedelic"),
        68 => Some("Rave"),
        69 => Some("Showtunes"),
        70 => Some("Trailer"),
        71 => Some("Lo-Fi"),
        72 => Some("Tribal"),
        73 => Some("Acid Punk"),
        74 => Some("Acid Jazz"),
        75 => Some("Polka"),
        76 => Some("Retro"),
        77 => Some("Musical"),
        78 => Some("Rock & Roll"),
        79 => Some("Hard Rock"),
        80 => Some("Folk"),
        81 => Some("Folk-Rock"),
        82 => Some("National Folk"),
        83 => Some("Swing"),
        84 => Some("Fast-Fusion"),
        85 => Some("Bebop"),
        86 => Some("Latin"),
        87 => Some("Revival"),
        88 => Some("Celtic"),
        89 => Some("Bluegrass"),
        90 => Some("Avantgarde"),
        91 => Some("Gothic Rock"),
        92 => Some("Progressive Rock"),
        93 => Some("Psychedelic Rock"),
        94 => Some("Symphonic Rock"),
        95 => Some("Slow Rock"),
        96 => Some("Big Band"),
        97 => Some("Chorus"),
        98 => Some("Easy Listening"),
        99 => Some("Acoustic"),
        100 => Some("Humour"),
        101 => Some("Speech"),
        102 => Some("Chanson"),
        103 => Some("Opera"),
        104 => Some("Chamber Music"),
        105 => Some("Sonata"),
        106 => Some("Symphony"),
        107 => Some("Booty Bass"),
        108 => Some("Primus"),
        109 => Some("Porn Groove"),
        110 => Some("Satire"),
        111 => Some("Slow Jam"),
        112 => Some("Club"),
        113 => Some("Tango"),
        114 => Some("Samba"),
        115 => Some("Folklore"),
        116 => Some("Ballad"),
        117 => Some("Power Ballad"),
        118 => Some("Rhythmic Soul"),
        119 => Some("Freestyle"),
        120 => Some("Duet"),
        121 => Some("Punk Rock"),
        122 => Some("Drum Solo"),
        123 => Some("A Cappella"),
        124 => Some("Euro-House"),
        125 => Some("Dance Hall"),
        126 => Some("Goa"),
        127 => Some("Drum & Bass"),
        128 => Some("Club-House"),
        129 => Some("Hardcore"),
        130 => Some("Terror"),
        131 => Some("Indie"),
        132 => Some("BritPop"),
        133 => Some("Afro-Punk"),
        134 => Some("Polsk Punk"),
        135 => Some("Beat"),
        136 => Some("Christian Gangsta Rap"),
        137 => Some("Heavy Metal"),
        138 => Some("Black Metal"),
        139 => Some("Crossover"),
        140 => Some("Contemporary Christian"),
        141 => Some("Christian Rock"),
        142 => Some("Merengue"),
        143 => Some("Salsa"),
        144 => Some("Thrash Metal"),
        145 => Some("Anime"),
        146 => Some("JPop"),
        147 => Some("Synthpop"),
        148 => Some("Abstract"),
        149 => Some("Art Rock"),
        150 => Some("Baroque"),
        151 => Some("Bhangra"),
        152 => Some("Big Beat"),
        153 => Some("Breakbeat"),
        154 => Some("Chillout"),
        155 => Some("Downtempo"),
        156 => Some("Dub"),
        157 => Some("EBM"),
        158 => Some("Eclectic"),
        159 => Some("Electro"),
        160 => Some("Electroclash"),
        161 => Some("Emo"),
        162 => Some("Experimental"),
        163 => Some("Garage"),
        164 => Some("Global"),
        165 => Some("IDM"),
        166 => Some("Illbient"),
        167 => Some("Industro-Goth"),
        168 => Some("Jam Band"),
        169 => Some("Krautrock"),
        170 => Some("Leftfield"),
        171 => Some("Lounge"),
        172 => Some("Math Rock"),
        173 => Some("New Romantic"),
        174 => Some("Nu-Breakz"),
        175 => Some("Post-Punk"),
        176 => Some("Post-Rock"),
        177 => Some("Psytrance"),
        178 => Some("Shoegaze"),
        179 => Some("Space Rock"),
        180 => Some("Trop Rock"),
        181 => Some("World Music"),
        182 => Some("Neoclassical"),
        183 => Some("Audiobook"),
        184 => Some("Audio Theatre"),
        185 => Some("Neue Deutsche Welle"),
        186 => Some("Podcast"),
        187 => Some("Indie Rock"),
        188 => Some("G-Funk"),
        189 => Some("Dubstep"),
        190 => Some("Garage Rock"),
        191 => Some("Psybient"),
        255 => Some("None"),
        _ => None,
    }
}

fn read_null_terminated_str(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    crate::encoding::decode_utf8_or_latin1(&bytes[..end])
        .trim()
        .to_string()
}

fn real_convert_bitrate(bps: f64) -> String {
    flv_convert_bitrate(bps)
}

fn real_convert_duration(secs: f64) -> String {
    flv_convert_duration(secs)
}
