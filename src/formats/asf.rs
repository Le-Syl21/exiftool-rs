//! ASF/WMV/WMA file format reader.
//!
//! Parses GUID-based ASF objects for metadata.
//! Mirrors ExifTool's ASF.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

// ASF Header Object GUID
const ASF_HEADER_GUID: [u8; 16] = [
    0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C,
];

// GUIDs for sub-objects
const GUID_FILE_PROPERTIES: [u8; 16] = [
    0xA1, 0xDC, 0xAB, 0x8C, 0x47, 0xA9, 0xCF, 0x11, 0x8E, 0xE4, 0x00, 0xC0, 0x0C, 0x20, 0x53, 0x65,
];
const GUID_CONTENT_DESCRIPTION: [u8; 16] = [
    0x33, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C,
];
const GUID_EXTENDED_CONTENT_DESCR: [u8; 16] = [
    0x40, 0xA4, 0xD0, 0xD2, 0x07, 0xE3, 0xD2, 0x11, 0x97, 0xF0, 0x00, 0xA0, 0xC9, 0x5E, 0xA8, 0x50,
];
const GUID_STREAM_PROPERTIES: [u8; 16] = [
    0x91, 0x07, 0xDC, 0xB7, 0xB7, 0xA9, 0xCF, 0x11, 0x8E, 0xE6, 0x00, 0xC0, 0x0C, 0x20, 0x53, 0x65,
];
const GUID_CODEC_LIST: [u8; 16] = [
    0x40, 0x52, 0xD1, 0x86, 0x1D, 0x31, 0xD0, 0x11, 0xA3, 0xA4, 0x00, 0xA0, 0xC9, 0x03, 0x48, 0xF6,
];
const GUID_HEADER_EXTENSION: [u8; 16] = [
    0xB5, 0x03, 0xBF, 0x5F, 0x2E, 0xA9, 0xCF, 0x11, 0x8E, 0xE3, 0x00, 0xC0, 0x0C, 0x20, 0x53, 0x65,
];
const GUID_METADATA: [u8; 16] = [
    0xEA, 0xCB, 0xF8, 0xC5, 0xAF, 0x5B, 0x77, 0x48, 0x84, 0x67, 0xAA, 0x8C, 0x44, 0xFA, 0x4C, 0xCA,
];
const GUID_METADATA_LIBRARY: [u8; 16] = [
    0x94, 0x1C, 0x23, 0x44, 0x98, 0x94, 0xD1, 0x49, 0xA1, 0x41, 0x1D, 0x13, 0x4E, 0x45, 0x70, 0x54,
];

// Stream type GUIDs
const GUID_AUDIO_STREAM: [u8; 16] = [
    0x40, 0x9E, 0x69, 0xF8, 0x4D, 0x5B, 0xCF, 0x11, 0xA8, 0xFD, 0x00, 0x80, 0x5F, 0x5C, 0x44, 0x2B,
];
const GUID_VIDEO_STREAM: [u8; 16] = [
    0xC0, 0xEF, 0x19, 0xBC, 0x4D, 0x5B, 0xCF, 0x11, 0xA8, 0xFD, 0x00, 0x80, 0x5F, 0x5C, 0x44, 0x2B,
];

pub fn read_asf(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 30 || &data[..16] != ASF_HEADER_GUID {
        return Err(Error::InvalidData("not an ASF file".into()));
    }

    let mut tags = Vec::new();
    let header_size = u64::from_le_bytes([
        data[16], data[17], data[18], data[19], data[20], data[21], data[22], data[23],
    ]) as usize;

    let end = header_size.min(data.len());
    parse_asf_objects(data, 30, end, &mut tags);

    Ok(tags)
}

fn parse_asf_objects(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    let mut pos = start;
    while pos + 24 <= end {
        let guid = &data[pos..pos + 16];
        let obj_size = u64::from_le_bytes([
            data[pos + 16],
            data[pos + 17],
            data[pos + 18],
            data[pos + 19],
            data[pos + 20],
            data[pos + 21],
            data[pos + 22],
            data[pos + 23],
        ]) as usize;

        if obj_size < 24 || pos + obj_size > end {
            break;
        }

        let obj_data = &data[pos + 24..pos + obj_size];

        if guid_matches(guid, &GUID_FILE_PROPERTIES) {
            parse_file_properties(obj_data, tags);
        } else if guid_matches(guid, &GUID_CONTENT_DESCRIPTION) {
            parse_content_description(obj_data, tags);
        } else if guid_matches(guid, &GUID_EXTENDED_CONTENT_DESCR) {
            parse_extended_content(obj_data, tags);
        } else if guid_matches(guid, &GUID_STREAM_PROPERTIES) {
            parse_stream_properties(obj_data, tags);
        } else if guid_matches(guid, &GUID_CODEC_LIST) {
            parse_codec_list(obj_data, tags);
        } else if guid_matches(guid, &GUID_HEADER_EXTENSION) {
            // HeaderExtension has 22 reserved bytes before sub-objects
            if obj_data.len() > 22 {
                let ext_size =
                    u32::from_le_bytes([obj_data[18], obj_data[19], obj_data[20], obj_data[21]])
                        as usize;
                let sub_end = (22 + ext_size).min(obj_data.len());
                parse_asf_objects(obj_data, 22, sub_end, tags);
            }
        } else if guid_matches(guid, &GUID_METADATA) {
            parse_metadata_object(obj_data, tags);
        } else if guid_matches(guid, &GUID_METADATA_LIBRARY) {
            parse_metadata_library(obj_data, tags);
        }

        pos += obj_size;
    }
}

fn parse_file_properties(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 80 {
        return;
    }

    // FileID at offset 0 (GUID 16 bytes)
    let file_id = format_guid(&data[0..16]);
    tags.push(mk("FileID", "File ID", Value::String(file_id)));

    // FileLength at offset 16 (uint64)
    let file_length = u64::from_le_bytes([
        data[16], data[17], data[18], data[19], data[20], data[21], data[22], data[23],
    ]);
    tags.push(mk(
        "FileLength",
        "File Length",
        Value::U32(file_length as u32),
    ));

    // CreationDate at offset 24 (FILETIME)
    let create_time = u64::from_le_bytes([
        data[24], data[25], data[26], data[27], data[28], data[29], data[30], data[31],
    ]);
    if create_time > 0 {
        if let Some(dt) = filetime_to_string(create_time) {
            tags.push(mk("CreationDate", "Creation Date", Value::String(dt + "Z")));
        }
    }

    // DataPackets at offset 32
    let data_packets = u64::from_le_bytes([
        data[32], data[33], data[34], data[35], data[36], data[37], data[38], data[39],
    ]);
    tags.push(mk(
        "DataPackets",
        "Data Packets",
        Value::U32(data_packets as u32),
    ));

    // Duration (PlayDuration) at offset 40 (100ns units)
    let duration_100ns = u64::from_le_bytes([
        data[40], data[41], data[42], data[43], data[44], data[45], data[46], data[47],
    ]);

    // SendDuration at offset 48
    let send_dur_100ns = u64::from_le_bytes([
        data[48], data[49], data[50], data[51], data[52], data[53], data[54], data[55],
    ]);

    // Preroll at offset 56 (milliseconds)
    let preroll_ms = u64::from_le_bytes([
        data[56], data[57], data[58], data[59], data[60], data[61], data[62], data[63],
    ]);

    // Flags at offset 64
    let flags = u32::from_le_bytes([data[64], data[65], data[66], data[67]]);

    // MinPacketSize at offset 68
    let min_pkt = u32::from_le_bytes([data[68], data[69], data[70], data[71]]);

    // MaxPacketSize at offset 72
    let max_pkt = u32::from_le_bytes([data[72], data[73], data[74], data[75]]);

    // MaxBitrate at offset 76
    let max_bitrate = u32::from_le_bytes([data[76], data[77], data[78], data[79]]);

    // Compute duration
    let dur_secs = duration_100ns as f64 / 1e7;
    if dur_secs > 0.0 {
        tags.push(mk(
            "Duration",
            "Duration",
            Value::String(format_duration(dur_secs)),
        ));
    }

    // SendDuration
    let send_secs = send_dur_100ns as f64 / 1e7;
    if send_secs > 0.0 {
        tags.push(mk(
            "SendDuration",
            "Send Duration",
            Value::String(format_duration(send_secs)),
        ));
    }

    tags.push(mk("Preroll", "Preroll", Value::U32(preroll_ms as u32)));
    tags.push(mk("Flags", "Flags", Value::U32(flags)));
    tags.push(mk("MinPacketSize", "Min Packet Size", Value::U32(min_pkt)));
    tags.push(mk("MaxPacketSize", "Max Packet Size", Value::U32(max_pkt)));

    if max_bitrate > 0 {
        // ExifTool prints as "X kbps" or "X.X kbps" via ConvertBitrate
        let kbps = max_bitrate as f64 / 1000.0;
        let bps_str = if kbps < 10.0 {
            format!("{:.2} kbps", kbps)
        } else if kbps < 100.0 {
            format!("{:.1} kbps", kbps)
        } else {
            format!("{} kbps", kbps as u32)
        };
        tags.push(mk("MaxBitrate", "Max Bitrate", Value::String(bps_str)));
    }
}

fn format_duration(secs: f64) -> String {
    let mins = (secs / 60.0) as u32;
    let s = secs % 60.0;
    format!("{}:{:05.2}", mins, s)
}

fn parse_content_description(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 10 {
        return;
    }

    let title_len = u16::from_le_bytes([data[0], data[1]]) as usize;
    let author_len = u16::from_le_bytes([data[2], data[3]]) as usize;
    let copyright_len = u16::from_le_bytes([data[4], data[5]]) as usize;
    let desc_len = u16::from_le_bytes([data[6], data[7]]) as usize;
    let _rating_len = u16::from_le_bytes([data[8], data[9]]) as usize;

    let mut pos = 10;
    let fields = [
        (title_len, "Title", "Title"),
        (author_len, "Author", "Author"),
        (copyright_len, "Copyright", "Copyright"),
        (desc_len, "Description", "Description"),
    ];

    for (len, name, desc) in &fields {
        if pos + len > data.len() {
            break;
        }
        let text = decode_utf16le(&data[pos..pos + len]);
        if !text.is_empty() {
            tags.push(mk(name, desc, Value::String(text)));
        }
        pos += len;
    }
}

fn parse_extended_content(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 2 {
        return;
    }
    let count = u16::from_le_bytes([data[0], data[1]]) as usize;
    let mut pos = 2;

    for _ in 0..count {
        if pos + 6 > data.len() {
            break;
        }
        let name_len = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;
        if pos + name_len > data.len() {
            break;
        }
        let name = decode_utf16le(&data[pos..pos + name_len]);
        pos += name_len;

        if pos + 4 > data.len() {
            break;
        }
        let val_type = u16::from_le_bytes([data[pos], data[pos + 1]]);
        let val_len = u16::from_le_bytes([data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;

        if pos + val_len > data.len() {
            break;
        }

        let val_bytes = &data[pos..pos + val_len];
        pos += val_len;

        // Strip WM/ prefix from name
        let clean_name = name.trim_start_matches("WM/");

        // Handle WM/Picture specially
        if clean_name == "Picture" && val_type == 1 {
            parse_wm_picture(val_bytes, tags);
            continue;
        }

        // Handle MediaClassPrimaryID / MediaClassSecondaryID (binary GUID)
        if (clean_name == "MediaClassPrimaryID" || clean_name == "MediaClassSecondaryID")
            && val_type == 1
            && val_len == 16
        {
            let guid = format_guid(val_bytes);
            tags.push(mk(clean_name, clean_name, Value::String(guid)));
            continue;
        }

        let value = match val_type {
            0 => decode_utf16le(val_bytes), // Unicode string
            1 => format!("(Binary data {} bytes, use -b option to extract)", val_len), // Binary
            2 => {
                // Bool
                if val_len >= 4 {
                    let v = u32::from_le_bytes([
                        val_bytes[0],
                        val_bytes[1],
                        val_bytes[2],
                        val_bytes[3],
                    ]);
                    if v != 0 {
                        "True".into()
                    } else {
                        "False".into()
                    }
                } else if val_len >= 2 {
                    let v = u16::from_le_bytes([val_bytes[0], val_bytes[1]]);
                    if v != 0 {
                        "True".into()
                    } else {
                        "False".into()
                    }
                } else {
                    String::new()
                }
            }
            3 => {
                // DWORD
                if val_len >= 4 {
                    u32::from_le_bytes([val_bytes[0], val_bytes[1], val_bytes[2], val_bytes[3]])
                        .to_string()
                } else {
                    String::new()
                }
            }
            4 => {
                // QWORD
                if val_len >= 8 {
                    u64::from_le_bytes([
                        val_bytes[0],
                        val_bytes[1],
                        val_bytes[2],
                        val_bytes[3],
                        val_bytes[4],
                        val_bytes[5],
                        val_bytes[6],
                        val_bytes[7],
                    ])
                    .to_string()
                } else {
                    String::new()
                }
            }
            5 => {
                // WORD
                if val_len >= 2 {
                    u16::from_le_bytes([val_bytes[0], val_bytes[1]]).to_string()
                } else {
                    String::new()
                }
            }
            6 => {
                // GUID
                if val_len >= 16 {
                    format_guid(val_bytes)
                } else {
                    String::new()
                }
            }
            _ => String::new(),
        };

        if !value.is_empty() && !clean_name.is_empty() && is_known_asf_tag(clean_name) {
            tags.push(mk(clean_name, clean_name, Value::String(value)));
        }
    }
}

/// Check if a tag name is in the known ASF tag table
fn is_known_asf_tag(name: &str) -> bool {
    matches!(
        name,
        "HasArbitraryDataStream" | "HasAttachedImages" | "HasAudio" | "HasFileTransferStream" |
        "HasImage" | "HasScript" | "HasVideo" | "Is_Protected" | "Is_Trusted" | "IsVBR" |
        "NSC_Address" | "NSC_Description" | "NSC_Email" | "NSC_Name" | "NSC_Phone" |
        "NumberOfFrames" | "OptimalBitrate" | "PeakValue" | "Rating" | "Seekable" |
        "Signature_Name" | "Stridable" | "Title" | "VBRPeak" |
        "AlbumArtist" | "AlbumCoverURL" | "AlbumTitle" | "ASFPacketCount" |
        "ASFSecurityObjectsSize" | "AudioFileURL" | "AudioSourceURL" | "AuthorURL" |
        "BeatsPerMinute" | "Category" | "Codec" | "Composer" | "Conductor" |
        "ContainerFormat" | "ContentDistributor" | "ContentGroupDescription" |
        "Director" | "DRM" | "DVDID" | "EncodedBy" | "EncodingSettings" | "EncodingTime" |
        "Genre" | "GenreID" | "InitialKey" | "ISRC" | "Language" | "Lyrics" |
        "Lyrics_Synchronised" | "MCDI" | "MediaClassPrimaryID" | "MediaClassSecondaryID" |
        "MediaCredits" | "MediaIsDelay" | "MediaIsFinale" | "MediaIsLive" |
        "MediaIsPremiere" | "MediaIsRepeat" | "MediaIsSAP" | "MediaIsStereo" |
        "MediaIsSubtitled" | "MediaIsTape" | "MediaNetworkAffiliation" |
        "MediaOriginalBroadcastDateTime" | "MediaOriginalChannel" |
        "MediaStationCallSign" | "MediaStationName" | "ModifiedBy" | "Mood" |
        "OriginalAlbumTitle" | "OriginalArtist" | "OriginalFileName" | "OriginalLyricist" |
        "OriginalReleaseTime" | "OriginalReleaseYear" | "ParentalRating" |
        "ParentalRatingReason" | "PartOfSet" | "PeakBitrate" | "Period" | "Picture" |
        "PlaylistDelay" | "Producer" | "PromotionURL" | "ProtectionType" |
        "Provider" | "ProviderCopyright" | "ProviderRating" | "ProviderStyle" |
        "Publisher" | "RadioStationName" | "RadioStationOwner" | "SharedUserRating" |
        "StreamTypeInfo" | "SubscriptionContentID" | "Subtitle" | "SubtitleDescription" |
        "Text" | "ToolName" | "ToolVersion" | "Track" | "TrackNumber" |
        "UniqueFileIdentifier" | "UserWebURL" | "VideoClosedCaptioning" |
        "VideoFrameRate" | "VideoHeight" | "VideoWidth" |
        "WMADRCAverageReference" | "WMADRCAverageTarget" | "WMADRCPeakReference" |
        "WMADRCPeakTarget" | "WMCollectionGroupID" | "WMCollectionID" | "WMContentID" |
        "Writer" | "Year" |
        // Binary tags
        "ASFLeakyBucketPairs" |
        // Picture sub-tags are handled separately
        "PictureType" | "PictureMIMEType" | "PictureDescription"
    )
}

/// Parse WM/Picture binary data
fn parse_wm_picture(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 6 {
        return;
    }

    let pic_type = data[0];
    let pic_data_len = u32::from_le_bytes([data[1], data[2], data[3], data[4]]) as usize;

    let pic_type_str = match pic_type {
        0 => "Other",
        1 => "32x32 PNG Icon",
        2 => "Other Icon",
        3 => "Front Cover",
        4 => "Back Cover",
        5 => "Leaflet",
        6 => "Media",
        7 => "Lead Artist",
        8 => "Artist",
        9 => "Conductor",
        10 => "Band",
        11 => "Composer",
        12 => "Lyricist",
        13 => "Recording Studio or Location",
        14 => "Recording Session",
        15 => "Performance",
        16 => "Capture from Movie or Video",
        17 => "Bright(ly) Colored Fish",
        18 => "Illustration",
        19 => "Band Logo",
        20 => "Publisher Logo",
        _ => "Unknown",
    };
    tags.push(mk(
        "PictureType",
        "Picture Type",
        Value::String(pic_type_str.into()),
    ));

    // Read MIME (null-terminated UTF-16)
    let mut pos = 5;
    let mut mime_end = pos;
    while mime_end + 2 <= data.len() {
        let ch = u16::from_le_bytes([data[mime_end], data[mime_end + 1]]);
        mime_end += 2;
        if ch == 0 {
            break;
        }
    }
    let mime = decode_utf16le(&data[pos..mime_end.saturating_sub(2)]);
    pos = mime_end;

    // Read description (null-terminated UTF-16)
    let mut desc_end = pos;
    while desc_end + 2 <= data.len() {
        let ch = u16::from_le_bytes([data[desc_end], data[desc_end + 1]]);
        desc_end += 2;
        if ch == 0 {
            break;
        }
    }
    pos = desc_end;

    if !mime.is_empty() {
        tags.push(mk(
            "PictureMIMEType",
            "Picture MIME Type",
            Value::String(mime),
        ));
    }

    // Picture binary data
    let pic_end = (pos + pic_data_len).min(data.len());
    if pos < data.len() {
        let pic_data = data[pos..pic_end].to_vec();
        tags.push(mk("Picture", "Picture", Value::Binary(pic_data)));
    }
}

fn parse_stream_properties(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 54 {
        return;
    }

    // Remove previous stream metadata tags (ExifTool overwrites with last stream)
    for tag_name in &[
        "StreamType",
        "ErrorCorrectionType",
        "TimeOffset",
        "StreamNumber",
    ] {
        tags.retain(|t| &t.name != tag_name);
    }

    // StreamType GUID at offset 0
    let stream_guid = &data[0..16];
    let stream_type_name = if guid_matches(stream_guid, &GUID_AUDIO_STREAM) {
        "Audio"
    } else if guid_matches(stream_guid, &GUID_VIDEO_STREAM) {
        "Video"
    } else {
        ""
    };
    if !stream_type_name.is_empty() {
        tags.push(mk(
            "StreamType",
            "Stream Type",
            Value::String(stream_type_name.into()),
        ));
    }

    // ErrorCorrectionType GUID at offset 16
    let err_guid = &data[16..32];
    let err_str = if guid_matches(
        err_guid,
        &[
            0x00, 0x57, 0xFB, 0x20, 0x55, 0x5B, 0xCF, 0x11, 0xA8, 0xFD, 0x00, 0x80, 0x5F, 0x5C,
            0x44, 0x2B,
        ],
    ) {
        "No Error Correction"
    } else if guid_matches(
        err_guid,
        &[
            0x50, 0xCD, 0xC3, 0xBF, 0x8F, 0x61, 0xCF, 0x11, 0x8B, 0xB2, 0x00, 0xAA, 0x00, 0xB4,
            0xE2, 0x20,
        ],
    ) {
        "Audio Spread"
    } else {
        ""
    };
    if !err_str.is_empty() {
        tags.push(mk(
            "ErrorCorrectionType",
            "Error Correction Type",
            Value::String(err_str.into()),
        ));
    }

    // TimeOffset at offset 32 (int64u, 100ns units)
    let time_offset_100ns = u64::from_le_bytes([
        data[32], data[33], data[34], data[35], data[36], data[37], data[38], data[39],
    ]);
    let time_offset_secs = time_offset_100ns as f64 / 1e7;
    tags.push(mk(
        "TimeOffset",
        "Time Offset",
        Value::String(format!("{} s", time_offset_secs)),
    ));

    // TypeSpecificLen at offset 40
    let type_spec_len = u32::from_le_bytes([data[40], data[41], data[42], data[43]]) as usize;

    // StreamNumber at offset 48 (int16u, low 7 bits)
    let stream_flags = u16::from_le_bytes([data[48], data[49]]);
    let stream_num = stream_flags & 0x7f;
    let encrypted_str = if stream_flags & 0x8000 != 0 {
        " (encrypted)"
    } else {
        ""
    };
    tags.push(mk(
        "StreamNumber",
        "Stream Number",
        Value::String(format!("{}{}", stream_num, encrypted_str)),
    ));

    // Type-specific data starts at offset 54
    if data.len() < 54 {
        return;
    }
    let ts = &data[54..];

    if guid_matches(stream_guid, &GUID_AUDIO_STREAM) && ts.len() >= 16 {
        // WAVEFORMATEX
        let codec_id = u16::from_le_bytes([ts[0], ts[1]]);
        let codec_name = waveformat_codec_name(codec_id);
        if !codec_name.is_empty() {
            tags.push(mk(
                "AudioCodecID",
                "Audio Codec ID",
                Value::String(codec_name.into()),
            ));
        } else {
            tags.push(mk(
                "AudioCodecID",
                "Audio Codec ID",
                Value::String(format!("0x{:04X}", codec_id)),
            ));
        }
        let channels = u16::from_le_bytes([ts[2], ts[3]]);
        let sample_rate = u32::from_le_bytes([ts[4], ts[5], ts[6], ts[7]]);
        let _bits_per_sample = if ts.len() >= 16 {
            u16::from_le_bytes([ts[14], ts[15]])
        } else {
            0
        };

        tags.push(mk("AudioChannels", "Audio Channels", Value::U16(channels)));
        tags.push(mk(
            "AudioSampleRate",
            "Audio Sample Rate",
            Value::U32(sample_rate),
        ));
    } else if guid_matches(stream_guid, &GUID_VIDEO_STREAM) && ts.len() >= 11 {
        // VideoMediaType: 4 bytes width, 4 bytes height at offset 4 and 8
        let width = u32::from_le_bytes([ts[4], ts[5], ts[6], ts[7]]);
        let height = u32::from_le_bytes([ts[8], ts[9], ts[10], ts[11]]);
        tags.push(mk("ImageWidth", "Image Width", Value::U32(width)));
        tags.push(mk("ImageHeight", "Image Height", Value::U32(height)));
    }

    let _ = type_spec_len;
}

fn waveformat_codec_name(codec_id: u16) -> &'static str {
    match codec_id {
        0x0001 => "PCM",
        0x0002 => "Microsoft ADPCM",
        0x0003 => "IEEE Float",
        0x0006 => "a-Law",
        0x0007 => "u-Law",
        0x0055 => "MPEG",
        0x0160 => "Windows Media Audio V1 / DivX audio (WMA) / Alex AC3 Audio",
        0x0161 => "Windows Media Audio V2 V7 V8 V9 / DivX audio (WMA) / Alex AC3 Audio",
        0x0162 => "Windows Media Audio 9 Professional",
        0x0163 => "Windows Media Audio 9 Lossless",
        0x0200 => "Creative ADPCM",
        0x0270 => "ATRAC3",
        _ => "",
    }
}

fn parse_codec_list(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 20 {
        return;
    }
    // Skip 16-byte reserved GUID + 4-byte codec count
    let codec_count = u32::from_le_bytes([data[16], data[17], data[18], data[19]]) as usize;
    let mut pos = 20;

    for _ in 0..codec_count.min(20) {
        if pos + 6 > data.len() {
            break;
        }
        let codec_type = u16::from_le_bytes([data[pos], data[pos + 1]]);
        let name_len = u16::from_le_bytes([data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;

        if pos + name_len * 2 > data.len() {
            break;
        }
        let name = decode_utf16le(&data[pos..pos + name_len * 2]);
        pos += name_len * 2;

        if pos + 2 > data.len() {
            break;
        }
        let desc_len = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;

        if pos + desc_len * 2 > data.len() {
            break;
        }
        let desc = decode_utf16le(&data[pos..pos + desc_len * 2]);
        pos += desc_len * 2;

        if pos + 2 > data.len() {
            break;
        }
        let info_len = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2 + info_len;

        match codec_type {
            1 => {
                // Video codec
                tags.push(mk(
                    "VideoCodecName",
                    "Video Codec Name",
                    Value::String(name),
                ));
                tags.push(mk(
                    "VideoCodecDescription",
                    "Video Codec Description",
                    Value::String(desc),
                ));
            }
            2 => {
                // Audio codec
                tags.push(mk(
                    "AudioCodecName",
                    "Audio Codec Name",
                    Value::String(name),
                ));
                tags.push(mk(
                    "AudioCodecDescription",
                    "Audio Codec Description",
                    Value::String(desc),
                ));
            }
            _ => {}
        }
    }
}

fn parse_metadata_object(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 2 {
        return;
    }
    let count = u16::from_le_bytes([data[0], data[1]]) as usize;
    let mut pos = 2;

    for _ in 0..count {
        if pos + 12 > data.len() {
            break;
        }
        let _reserved = u16::from_le_bytes([data[pos], data[pos + 1]]);
        let _stream_num = u16::from_le_bytes([data[pos + 2], data[pos + 3]]);
        let name_len = u16::from_le_bytes([data[pos + 4], data[pos + 5]]) as usize;
        let data_type = u16::from_le_bytes([data[pos + 6], data[pos + 7]]);
        let data_len =
            u32::from_le_bytes([data[pos + 8], data[pos + 9], data[pos + 10], data[pos + 11]])
                as usize;
        pos += 12;

        if pos + name_len > data.len() {
            break;
        }
        let name = decode_utf16le(&data[pos..pos + name_len]);
        pos += name_len;

        if pos + data_len > data.len() {
            break;
        }
        let val_bytes = &data[pos..pos + data_len];
        pos += data_len;

        let clean_name = name.trim_start_matches("WM/");
        let val = parse_typed_value(val_bytes, data_type);
        // Skip tags already set by ExtendedDescr (IsVBR etc.) to avoid duplicates
        if !val.is_empty() && !clean_name.is_empty() && is_known_asf_tag(clean_name) {
            // Only add if not already present (first-wins for Metadata vs ExtendedDescr)
            if !tags.iter().any(|t| t.name == clean_name) {
                tags.push(mk(clean_name, clean_name, Value::String(val)));
            }
        }
    }
}

fn parse_metadata_library(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 2 {
        return;
    }
    let count = u16::from_le_bytes([data[0], data[1]]) as usize;
    let mut pos = 2;

    for _ in 0..count {
        if pos + 12 > data.len() {
            break;
        }
        let _reserved = u16::from_le_bytes([data[pos], data[pos + 1]]);
        let _stream_num = u16::from_le_bytes([data[pos + 2], data[pos + 3]]);
        let name_len = u16::from_le_bytes([data[pos + 4], data[pos + 5]]) as usize;
        let data_type = u16::from_le_bytes([data[pos + 6], data[pos + 7]]);
        let data_len =
            u32::from_le_bytes([data[pos + 8], data[pos + 9], data[pos + 10], data[pos + 11]])
                as usize;
        pos += 12;

        if pos + name_len > data.len() {
            break;
        }
        let name = decode_utf16le(&data[pos..pos + name_len]);
        pos += name_len;

        if pos + data_len > data.len() {
            break;
        }
        let val_bytes = &data[pos..pos + data_len];
        pos += data_len;

        let clean_name = name.trim_start_matches("WM/");
        let val = parse_typed_value(val_bytes, data_type);
        if !val.is_empty() && !clean_name.is_empty() && is_known_asf_tag(clean_name) {
            tags.push(mk(clean_name, clean_name, Value::String(val)));
        }
    }
}

fn parse_typed_value(data: &[u8], data_type: u16) -> String {
    match data_type {
        0 => decode_utf16le(data), // Unicode string
        1 => format!(
            "(Binary data {} bytes, use -b option to extract)",
            data.len()
        ), // Binary
        2 => {
            // Bool
            if data.len() >= 2 {
                let v = u16::from_le_bytes([data[0], data[1]]);
                if v != 0 {
                    "True".into()
                } else {
                    "False".into()
                }
            } else {
                String::new()
            }
        }
        3 => {
            // DWORD
            if data.len() >= 4 {
                u32::from_le_bytes([data[0], data[1], data[2], data[3]]).to_string()
            } else {
                String::new()
            }
        }
        4 => {
            // QWORD
            if data.len() >= 8 {
                u64::from_le_bytes([
                    data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
                ])
                .to_string()
            } else {
                String::new()
            }
        }
        5 => {
            // WORD
            if data.len() >= 2 {
                u16::from_le_bytes([data[0], data[1]]).to_string()
            } else {
                String::new()
            }
        }
        6 => {
            // GUID
            if data.len() >= 16 {
                format_guid(data)
            } else {
                String::new()
            }
        }
        _ => String::new(),
    }
}

fn guid_matches(a: &[u8], b: &[u8; 16]) -> bool {
    a.len() >= 16 && &a[..16] == b
}

/// Format a GUID in standard Windows format: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
fn format_guid(data: &[u8]) -> String {
    if data.len() < 16 {
        return String::new();
    }
    let p1 = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let p2 = u16::from_le_bytes([data[4], data[5]]);
    let p3 = u16::from_le_bytes([data[6], data[7]]);
    let p4 = &data[8..16];
    format!(
        "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        p1, p2, p3, p4[0], p4[1], p4[2], p4[3], p4[4], p4[5], p4[6], p4[7]
    )
}

fn decode_utf16le(data: &[u8]) -> String {
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
        .trim_end_matches('\0')
        .to_string()
}

fn filetime_to_string(ft: u64) -> Option<String> {
    if ft == 0 {
        return None;
    }
    // FILETIME: 100ns intervals since 1601-01-01
    // Unix epoch starts 11644473600 seconds later
    let unix_secs = (ft / 10_000_000) as i64 - 11644473600;
    if unix_secs < 0 {
        return None;
    }

    let days = unix_secs / 86400;
    let time = unix_secs % 86400;
    let h = time / 3600;
    let m = (time % 3600) / 60;
    let s = time % 60;

    let mut y = 1970i32;
    let mut rem = days;
    loop {
        let dy = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
            366
        } else {
            365
        };
        if rem < dy {
            break;
        }
        rem -= dy;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let months = [
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
    let mut mo = 1;
    for &dm in &months {
        if rem < dm {
            break;
        }
        rem -= dm;
        mo += 1;
    }

    Some(format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        y,
        mo,
        rem + 1,
        h,
        m,
        s
    ))
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "ASF".into(),
            family1: "ASF".into(),
            family2: "Video".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}
