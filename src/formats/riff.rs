//! RIFF container format reader (WebP, AVI, WAV).
//!
//! Parses RIFF chunks to extract metadata from WebP (EXIF, XMP, ICC),
//! AVI (video/audio info, INFO chunks), and WAV (audio format, INFO).
//! Mirrors ExifTool's RIFF.pm.

use crate::error::{Error, Result};
use crate::metadata::{ExifReader, XmpReader};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

/// Read metadata from a RIFF-based file (WebP, AVI, WAV).
pub fn read_riff(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 12 || !data.starts_with(b"RIFF") {
        return Err(Error::InvalidData("not a RIFF file".into()));
    }

    let _file_size = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let form_type = &data[8..12];
    let mut tags = Vec::new();

    match form_type {
        b"WEBP" => read_webp_chunks(data, 12, &mut tags)?,
        b"AVI " => read_avi_chunks(data, 12, &mut tags)?,
        b"WAVE" => read_wav_chunks(data, 12, &mut tags)?,
        _ => {
            return Err(Error::InvalidData(format!(
                "unknown RIFF type: {}",
                String::from_utf8_lossy(form_type)
            )));
        }
    }

    Ok(tags)
}

// ============================================================================
// WebP
// ============================================================================

fn read_webp_chunks(data: &[u8], start: usize, tags: &mut Vec<Tag>) -> Result<()> {
    let mut pos = start;

    while pos + 8 <= data.len() {
        let chunk_id = &data[pos..pos + 4];
        let chunk_size = u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]) as usize;
        let chunk_data_start = pos + 8;
        let chunk_data_end = chunk_data_start + chunk_size;

        if chunk_data_end > data.len() {
            break;
        }
        let chunk_data = &data[chunk_data_start..chunk_data_end];

        match chunk_id {
            // VP8 lossy bitstream
            b"VP8 " => {
                if chunk_data.len() >= 10 {
                    // VP8 frame header: 3 bytes frame tag, then dimensions
                    let frame_tag = u32::from_le_bytes([
                        chunk_data[0], chunk_data[1], chunk_data[2], 0,
                    ]);
                    let is_keyframe = (frame_tag & 1) == 0;
                    if is_keyframe && chunk_data.len() >= 10 {
                        // Check for VP8 signature 0x9D012A
                        if chunk_data[3] == 0x9D && chunk_data[4] == 0x01 && chunk_data[5] == 0x2A {
                            let width = u16::from_le_bytes([chunk_data[6], chunk_data[7]]) & 0x3FFF;
                            let height = u16::from_le_bytes([chunk_data[8], chunk_data[9]]) & 0x3FFF;
                            tags.push(mk_webp("ImageWidth", "Image Width", Value::U16(width)));
                            tags.push(mk_webp("ImageHeight", "Image Height", Value::U16(height)));
                        }
                    }
                }
            }
            // VP8L lossless bitstream
            b"VP8L" => {
                if chunk_data.len() >= 5 && chunk_data[0] == 0x2F {
                    let bits = u32::from_le_bytes([chunk_data[1], chunk_data[2], chunk_data[3], chunk_data[4]]);
                    let width = (bits & 0x3FFF) + 1;
                    let height = ((bits >> 14) & 0x3FFF) + 1;
                    let alpha = (bits >> 28) & 1;
                    tags.push(mk_webp("ImageWidth", "Image Width", Value::U32(width)));
                    tags.push(mk_webp("ImageHeight", "Image Height", Value::U32(height)));
                    if alpha != 0 {
                        tags.push(mk_webp("HasAlpha", "Has Alpha", Value::String("Yes".into())));
                    }
                }
            }
            // VP8X extended format
            b"VP8X" => {
                if chunk_data.len() >= 10 {
                    let flags = chunk_data[0];
                    let width = (chunk_data[4] as u32)
                        | ((chunk_data[5] as u32) << 8)
                        | ((chunk_data[6] as u32) << 16);
                    let height = (chunk_data[7] as u32)
                        | ((chunk_data[8] as u32) << 8)
                        | ((chunk_data[9] as u32) << 16);
                    tags.push(mk_webp("ImageWidth", "Image Width", Value::U32(width + 1)));
                    tags.push(mk_webp("ImageHeight", "Image Height", Value::U32(height + 1)));
                    if flags & 0x10 != 0 {
                        tags.push(mk_webp("HasAlpha", "Has Alpha", Value::String("Yes".into())));
                    }
                    if flags & 0x02 != 0 {
                        tags.push(mk_webp("Animation", "Animation", Value::String("Yes".into())));
                    }
                }
            }
            // EXIF data
            b"EXIF" => {
                // May start with "Exif\0\0" header or directly with TIFF header
                let exif_data = if chunk_data.len() > 6 && chunk_data.starts_with(b"Exif\0\0") {
                    &chunk_data[6..]
                } else {
                    chunk_data
                };
                if let Ok(exif_tags) = ExifReader::read(exif_data) {
                    tags.extend(exif_tags);
                }
            }
            // XMP data
            b"XMP " => {
                if let Ok(xmp_tags) = XmpReader::read(chunk_data) {
                    tags.extend(xmp_tags);
                }
            }
            // ICC Profile
            b"ICCP" => {
                tags.push(mk_webp(
                    "ICC_Profile",
                    "ICC Profile",
                    Value::Binary(chunk_data.to_vec()),
                ));
            }
            // Animation
            b"ANIM" => {
                if chunk_data.len() >= 6 {
                    let bg_color = u32::from_le_bytes([
                        chunk_data[0], chunk_data[1], chunk_data[2], chunk_data[3],
                    ]);
                    let loop_count = u16::from_le_bytes([chunk_data[4], chunk_data[5]]);
                    tags.push(mk_webp(
                        "BackgroundColor",
                        "Background Color",
                        Value::U32(bg_color),
                    ));
                    tags.push(mk_webp(
                        "AnimationLoopCount",
                        "Animation Loop Count",
                        Value::U16(loop_count),
                    ));
                }
            }
            _ => {}
        }

        // Advance (pad to even boundary)
        pos = chunk_data_end + (chunk_size & 1);
    }

    Ok(())
}

// ============================================================================
// AVI
// ============================================================================

fn read_avi_chunks(data: &[u8], start: usize, tags: &mut Vec<Tag>) -> Result<()> {
    read_riff_chunks(data, start, data.len(), tags, "AVI")
}

// ============================================================================
// WAV
// ============================================================================

fn read_wav_chunks(data: &[u8], start: usize, tags: &mut Vec<Tag>) -> Result<()> {
    read_riff_chunks(data, start, data.len(), tags, "WAV")
}

/// Generic RIFF chunk iterator used by both AVI and WAV.
fn read_riff_chunks(
    data: &[u8],
    start: usize,
    end: usize,
    tags: &mut Vec<Tag>,
    family: &str,
) -> Result<()> {
    let mut pos = start;

    while pos + 8 <= end {
        let chunk_id = &data[pos..pos + 4];
        let chunk_size = u32::from_le_bytes([
            data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7],
        ]) as usize;
        let chunk_data_start = pos + 8;
        let chunk_data_end = (chunk_data_start + chunk_size).min(end);

        if chunk_data_start > end {
            break;
        }

        match chunk_id {
            // LIST chunk: contains a type + sub-chunks
            b"LIST" => {
                if chunk_data_end >= chunk_data_start + 4 {
                    let list_type = &data[chunk_data_start..chunk_data_start + 4];
                    match list_type {
                        b"INFO" => {
                            read_info_chunks(data, chunk_data_start + 4, chunk_data_end, tags, family)?;
                        }
                        b"hdrl" => {
                            // AVI header list - contains avih and strl
                            read_riff_chunks(data, chunk_data_start + 4, chunk_data_end, tags, family)?;
                        }
                        b"strl" => {
                            read_riff_chunks(data, chunk_data_start + 4, chunk_data_end, tags, family)?;
                        }
                        b"exif" => {
                            // EXIF data in AVI
                            if let Ok(exif_tags) = ExifReader::read(&data[chunk_data_start + 4..chunk_data_end]) {
                                tags.extend(exif_tags);
                            }
                        }
                        _ => {}
                    }
                }
            }
            // AVI Main Header
            b"avih" => {
                if chunk_size >= 56 {
                    let cd = &data[chunk_data_start..chunk_data_end];
                    let us_per_frame = u32::from_le_bytes([cd[0], cd[1], cd[2], cd[3]]);
                    let total_frames = u32::from_le_bytes([cd[16], cd[17], cd[18], cd[19]]);
                    let width = u32::from_le_bytes([cd[32], cd[33], cd[34], cd[35]]);
                    let height = u32::from_le_bytes([cd[36], cd[37], cd[38], cd[39]]);

                    tags.push(mk_riff(family, "ImageWidth", "Image Width", Value::U32(width)));
                    tags.push(mk_riff(family, "ImageHeight", "Image Height", Value::U32(height)));
                    tags.push(mk_riff(family, "TotalFrames", "Total Frames", Value::U32(total_frames)));

                    if us_per_frame > 0 {
                        let fps = 1_000_000.0 / us_per_frame as f64;
                        tags.push(mk_riff(
                            family,
                            "FrameRate",
                            "Frame Rate",
                            Value::String(format!("{:.2} fps", fps)),
                        ));
                        if total_frames > 0 {
                            let duration = total_frames as f64 * us_per_frame as f64 / 1_000_000.0;
                            tags.push(mk_riff(
                                family,
                                "Duration",
                                "Duration",
                                Value::String(format!("{:.2} s", duration)),
                            ));
                        }
                    }
                }
            }
            // Stream Header
            b"strh" => {
                if chunk_size >= 56 {
                    let cd = &data[chunk_data_start..chunk_data_end];
                    let fcc_type = String::from_utf8_lossy(&cd[0..4]).trim().to_string();
                    let fcc_handler = String::from_utf8_lossy(&cd[4..8]).trim().to_string();

                    if fcc_type == "vids" {
                        tags.push(mk_riff(family, "VideoCodec", "Video Codec", Value::String(fcc_handler)));
                    } else if fcc_type == "auds" {
                        tags.push(mk_riff(family, "AudioCodec", "Audio Codec", Value::String(fcc_handler)));
                    }
                }
            }
            // Audio Format
            b"fmt " => {
                if chunk_size >= 16 {
                    let cd = &data[chunk_data_start..chunk_data_end];
                    let format_tag = u16::from_le_bytes([cd[0], cd[1]]);
                    let channels = u16::from_le_bytes([cd[2], cd[3]]);
                    let sample_rate = u32::from_le_bytes([cd[4], cd[5], cd[6], cd[7]]);
                    let avg_bytes = u32::from_le_bytes([cd[8], cd[9], cd[10], cd[11]]);
                    let bits_per_sample = u16::from_le_bytes([cd[14], cd[15]]);

                    let encoding = match format_tag {
                        0x0001 => "PCM",
                        0x0002 => "Microsoft ADPCM",
                        0x0003 => "IEEE Float",
                        0x0006 => "A-Law",
                        0x0007 => "mu-Law",
                        0x0011 => "IMA ADPCM",
                        0x0050 => "MPEG",
                        0x0055 => "MP3",
                        0x00FF => "AAC",
                        0x0161 => "WMA V2",
                        0x0162 => "WMA Pro",
                        0xFFFE => "Extensible",
                        _ => "Unknown",
                    };

                    tags.push(mk_riff(family, "AudioEncoding", "Audio Encoding", Value::String(encoding.into())));
                    tags.push(mk_riff(family, "NumChannels", "Number of Channels", Value::U16(channels)));
                    tags.push(mk_riff(family, "SampleRate", "Sample Rate", Value::U32(sample_rate)));
                    tags.push(mk_riff(family, "BitsPerSample", "Bits Per Sample", Value::U16(bits_per_sample)));
                    tags.push(mk_riff(family, "AvgBytesPerSec", "Average Bytes/Sec", Value::U32(avg_bytes)));
                }
            }
            // EXIF chunk
            b"EXIF" => {
                let exif_data = &data[chunk_data_start..chunk_data_end];
                let exif_data = if exif_data.starts_with(b"Exif\0\0") {
                    &exif_data[6..]
                } else {
                    exif_data
                };
                if let Ok(exif_tags) = ExifReader::read(exif_data) {
                    tags.extend(exif_tags);
                }
            }
            // XMP
            b"_PMX" | b"XMP " => {
                if let Ok(xmp_tags) = XmpReader::read(&data[chunk_data_start..chunk_data_end]) {
                    tags.extend(xmp_tags);
                }
            }
            // Broadcast extension (WAV)
            b"bext" => {
                if chunk_size >= 256 {
                    let cd = &data[chunk_data_start..chunk_data_end];
                    let description = String::from_utf8_lossy(&cd[..256])
                        .trim_end_matches('\0')
                        .to_string();
                    if !description.is_empty() {
                        tags.push(mk_riff(family, "Description", "Description", Value::String(description)));
                    }
                    let originator = String::from_utf8_lossy(&cd[256..288.min(chunk_size)])
                        .trim_end_matches('\0')
                        .to_string();
                    if !originator.is_empty() {
                        tags.push(mk_riff(family, "Originator", "Originator", Value::String(originator)));
                    }
                }
            }
            _ => {}
        }

        pos = chunk_data_end + (chunk_size & 1);
    }

    Ok(())
}

/// Read INFO list chunks (RIFF metadata: IART, INAM, ICRD, etc.)
fn read_info_chunks(
    data: &[u8],
    start: usize,
    end: usize,
    tags: &mut Vec<Tag>,
    family: &str,
) -> Result<()> {
    let mut pos = start;

    while pos + 8 <= end {
        let chunk_id = std::str::from_utf8(&data[pos..pos + 4]).unwrap_or("????");
        let chunk_size = u32::from_le_bytes([
            data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7],
        ]) as usize;
        pos += 8;

        if pos + chunk_size > end {
            break;
        }

        let value = String::from_utf8_lossy(&data[pos..pos + chunk_size])
            .trim_end_matches('\0')
            .to_string();

        if !value.is_empty() {
            let (name, description) = info_chunk_name(chunk_id);
            tags.push(mk_riff(family, name, description, Value::String(value)));
        }

        pos += chunk_size + (chunk_size & 1);
    }

    Ok(())
}

/// Map RIFF INFO chunk IDs to tag names.
fn info_chunk_name(id: &str) -> (&str, &str) {
    match id {
        "IART" => ("Artist", "Artist"),
        "ICMT" => ("Comment", "Comment"),
        "ICOP" => ("Copyright", "Copyright"),
        "ICRD" => ("DateCreated", "Date Created"),
        "IGNR" => ("Genre", "Genre"),
        "INAM" => ("Title", "Title"),
        "IPRD" => ("Product", "Product"),
        "ISFT" => ("Software", "Software"),
        "ISBJ" => ("Subject", "Subject"),
        "ISRC" => ("Source", "Source"),
        "ITCH" => ("Technician", "Technician"),
        "IENG" => ("Engineer", "Engineer"),
        "IKEY" => ("Keywords", "Keywords"),
        "IMED" => ("Medium", "Medium"),
        "ILNG" => ("Language", "Language"),
        "ITRK" => ("TrackNumber", "Track Number"),
        "IDST" => ("DistributedBy", "Distributed By"),
        "IDIT" => ("DateTimeOriginal", "Date/Time Original"),
        "IDPI" => ("DotsPerInch", "Dots Per Inch"),
        _ => (id, id),
    }
}

fn mk_webp(name: &str, description: &str, value: Value) -> Tag {
    let print_value = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "RIFF".into(),
            family1: "WebP".into(),
            family2: "Image".into(),
        },
        raw_value: value,
        print_value,
        priority: 0,
    }
}

fn mk_riff(family: &str, name: &str, description: &str, value: Value) -> Tag {
    let print_value = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "RIFF".into(),
            family1: family.into(),
            family2: if family == "AVI" { "Video".into() } else { "Audio".into() },
        },
        raw_value: value,
        print_value,
        priority: 0,
    }
}
