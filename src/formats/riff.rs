//! RIFF container format reader (WebP, AVI, WAV).
//!
//! Parses RIFF chunks to extract metadata from WebP (EXIF, XMP, ICC),
//! AVI (video/audio info, INFO chunks), and WAV (audio format, INFO).
//! Mirrors ExifTool's RIFF.pm.

use crate::error::{Error, Result};
use crate::metadata::{ExifReader, XmpReader};
use crate::metadata::exif::ByteOrderMark;
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
                crate::encoding::decode_utf8_or_latin1(form_type)
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
                            // VP8Version: bits 1-3 of first byte
                            let version = (chunk_data[0] >> 1) & 0x07;
                            let version_str = match version {
                                0 => "0 (bicubic reconstruction, normal loop)",
                                1 => "1 (bilinear reconstruction, simple loop)",
                                2 => "2 (bilinear reconstruction, no loop)",
                                3 => "3 (no reconstruction, no loop)",
                                v => return {
                                    // fallback: just use number
                                    let width = u16::from_le_bytes([chunk_data[6], chunk_data[7]]) & 0x3FFF;
                                    let height = u16::from_le_bytes([chunk_data[8], chunk_data[9]]) & 0x3FFF;
                                    let hscale = (u16::from_le_bytes([chunk_data[6], chunk_data[7]]) >> 14) & 0x3;
                                    let vscale = (u16::from_le_bytes([chunk_data[8], chunk_data[9]]) >> 14) & 0x3;
                                    tags.push(mk_webp("VP8Version", "VP8 Version", Value::String(format!("{}", v))));
                                    tags.push(mk_webp("ImageWidth", "Image Width", Value::U16(width)));
                                    tags.push(mk_webp("HorizontalScale", "Horizontal Scale", Value::U16(hscale)));
                                    tags.push(mk_webp("ImageHeight", "Image Height", Value::U16(height)));
                                    tags.push(mk_webp("VerticalScale", "Vertical Scale", Value::U16(vscale)));
                                    Ok(())
                                },
                            };
                            let width = u16::from_le_bytes([chunk_data[6], chunk_data[7]]) & 0x3FFF;
                            let height = u16::from_le_bytes([chunk_data[8], chunk_data[9]]) & 0x3FFF;
                            let hscale = (u16::from_le_bytes([chunk_data[6], chunk_data[7]]) >> 14) & 0x3;
                            let vscale = (u16::from_le_bytes([chunk_data[8], chunk_data[9]]) >> 14) & 0x3;
                            tags.push(mk_webp("VP8Version", "VP8 Version", Value::String(version_str.into())));
                            tags.push(mk_webp("ImageWidth", "Image Width", Value::U16(width)));
                            tags.push(mk_webp("HorizontalScale", "Horizontal Scale", Value::U16(hscale)));
                            tags.push(mk_webp("ImageHeight", "Image Height", Value::U16(height)));
                            tags.push(mk_webp("VerticalScale", "Vertical Scale", Value::U16(vscale)));
                        }
                    }
                }
            }
            // VP8L lossless bitstream
            b"VP8L" => {
                if chunk_data.len() >= 5 && chunk_data[0] == 0x2F {
                    // Bits are packed: width is bits 1..14 (14 bits), height is bits 15..28
                    // The spec: read 32 bits starting at byte 1
                    let bits = u32::from_le_bytes([chunk_data[1], chunk_data[2], chunk_data[3], chunk_data[4]]);
                    let width = (bits & 0x3FFF) + 1;
                    let height = ((bits >> 14) & 0x3FFF) + 1;
                    let alpha = (bits >> 28) & 1;
                    tags.push(mk_webp("ImageWidth", "Image Width", Value::U32(width)));
                    tags.push(mk_webp("ImageHeight", "Image Height", Value::U32(height)));
                    tags.push(mk_webp("AlphaIsUsed", "Alpha Is Used", Value::String(if alpha != 0 { "Yes".into() } else { "No".into() })));
                }
            }
            // VP8X extended format
            b"VP8X" => {
                if chunk_data.len() >= 10 {
                    // Flags (32-bit little-endian)
                    let flags = u32::from_le_bytes([chunk_data[0], chunk_data[1], chunk_data[2], chunk_data[3]]);
                    // Width/Height are 24-bit LE at offsets 4 and 7
                    let width = (chunk_data[4] as u32)
                        | ((chunk_data[5] as u32) << 8)
                        | ((chunk_data[6] as u32) << 16);
                    let height = (chunk_data[7] as u32)
                        | ((chunk_data[8] as u32) << 8)
                        | ((chunk_data[9] as u32) << 16);

                    // Build WebP_Flags string (matching Perl ExifTool bitmask output)
                    // Perl bit positions: 1=Animation, 2=XMP, 3=EXIF, 4=Alpha, 5=ICC Profile
                    let mut flag_parts = Vec::new();
                    if flags & (1 << 1) != 0 { flag_parts.push("Animation"); }
                    if flags & (1 << 2) != 0 { flag_parts.push("XMP"); }
                    if flags & (1 << 3) != 0 { flag_parts.push("EXIF"); }
                    if flags & (1 << 4) != 0 { flag_parts.push("Alpha"); }
                    if flags & (1 << 5) != 0 { flag_parts.push("ICC Profile"); }
                    if !flag_parts.is_empty() {
                        tags.push(mk_webp("WebP_Flags", "WebP Flags", Value::String(flag_parts.join(", "))));
                    }

                    tags.push(mk_webp("ImageWidth", "Image Width", Value::U32(width + 1)));
                    tags.push(mk_webp("ImageHeight", "Image Height", Value::U32(height + 1)));
                }
            }
            // ALPH chunk (WebP alpha)
            b"ALPH" => {
                if !chunk_data.is_empty() {
                    let byte0 = chunk_data[0];
                    let preprocessing = byte0 & 0x03;
                    let filtering = (byte0 >> 2) & 0x03;
                    let compression = (byte0 >> 4) & 0x03;

                    let preprocessing_str = match preprocessing {
                        0 => "none",
                        1 => "Level Reduction",
                        _ => "Unknown",
                    };
                    let filtering_str = match filtering {
                        0 => "none",
                        1 => "Horizontal",
                        2 => "Vertical",
                        3 => "Gradient",
                        _ => "Unknown",
                    };
                    let compression_str = match compression {
                        0 => "none",
                        1 => "Lossless",
                        _ => "Unknown",
                    };

                    tags.push(mk_webp("AlphaPreprocessing", "Alpha Preprocessing", Value::String(preprocessing_str.into())));
                    tags.push(mk_webp("AlphaFiltering", "Alpha Filtering", Value::String(filtering_str.into())));
                    tags.push(mk_webp("AlphaCompression", "Alpha Compression", Value::String(compression_str.into())));
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
    let mut state = AviState::new();
    read_riff_chunks(data, start, data.len(), tags, "AVI", &mut state)
}

// ============================================================================
// WAV
// ============================================================================

fn read_wav_chunks(data: &[u8], start: usize, tags: &mut Vec<Tag>) -> Result<()> {
    let mut state = AviState::new();
    state.file_size = data.len() as u64;
    read_riff_chunks(data, start, data.len(), tags, "WAV", &mut state)
}

/// State for AVI stream tracking (stream type changes between strh/strf pairs)
struct AviState {
    /// Type of current stream: "vids", "auds", etc.
    current_stream_type: Option<String>,
    /// Accumulated data chunk size for duration calculation (WAV)
    data_len: u64,
    /// Total file size (for WAV duration fallback when data chunk is empty)
    file_size: u64,
    /// AvgBytesPerSec from fmt chunk (WAV duration)
    avg_bytes_per_sec: u32,
    /// Frame rate (microseconds per frame from avih)
    us_per_frame: u32,
    /// Total frames from avih
    total_frames: u32,
    /// Video frame count from strh
    video_frame_count: u32,
    /// Video frame rate (from strh, rational: scale/rate -> rate/scale fps)
    video_frame_rate: Option<f64>,
    /// Recursion depth — Duration is only emitted at depth 0
    depth: usize,
    /// Number of streams seen so far (StreamType/Quality/SampleSize only from first stream)
    stream_count_seen: u32,
}

impl AviState {
    fn new() -> Self {
        AviState {
            current_stream_type: None,
            data_len: 0,
            file_size: 0,
            avg_bytes_per_sec: 0,
            us_per_frame: 0,
            total_frames: 0,
            video_frame_count: 0,
            video_frame_rate: None,
            depth: 0,
            stream_count_seen: 0,
        }
    }
}

/// Generic RIFF chunk iterator used by both AVI and WAV.
fn read_riff_chunks(
    data: &[u8],
    start: usize,
    end: usize,
    tags: &mut Vec<Tag>,
    family: &str,
    state: &mut AviState,
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
                        b"INF0" => {
                            // Some files use '0' instead of 'O'
                            read_info_chunks(data, chunk_data_start + 4, chunk_data_end, tags, family)?;
                        }
                        b"hdrl" => {
                            // AVI header list - contains avih and strl
                            state.depth += 1;
                            read_riff_chunks(data, chunk_data_start + 4, chunk_data_end, tags, family, state)?;
                            state.depth -= 1;
                        }
                        b"strl" => {
                            state.depth += 1;
                            read_riff_chunks(data, chunk_data_start + 4, chunk_data_end, tags, family, state)?;
                            state.depth -= 1;
                        }
                        b"odml" => {
                            // OpenDML extended AVI header
                            state.depth += 1;
                            read_riff_chunks(data, chunk_data_start + 4, chunk_data_end, tags, family, state)?;
                            state.depth -= 1;
                        }
                        b"exif" => {
                            // EXIF data in AVI/WAV LIST exif chunk
                            read_exif_list_chunks(data, chunk_data_start + 4, chunk_data_end, tags, family)?;
                        }
                        b"adtl" => {
                            // Associated data list
                            state.depth += 1;
                            read_riff_chunks(data, chunk_data_start + 4, chunk_data_end, tags, family, state)?;
                            state.depth -= 1;
                        }
                        b"hydt" | b"pntx" => {
                            // Pentax metadata LIST (LIST hydt / LIST pntx)
                            read_pentax_avi_chunks(data, chunk_data_start + 4, chunk_data_end, tags)?;
                        }
                        _ => {}
                    }
                }
            }
            // AVI Main Header
            b"avih" => {
                if chunk_size >= 40 {
                    let cd = &data[chunk_data_start..chunk_data_end];
                    let us_per_frame = u32::from_le_bytes([cd[0], cd[1], cd[2], cd[3]]);
                    let max_data_rate = u32::from_le_bytes([cd[4], cd[5], cd[6], cd[7]]);
                    // cd[8..11] = PaddingGranularity, cd[12..15] = Flags
                    let total_frames = u32::from_le_bytes([cd[16], cd[17], cd[18], cd[19]]);
                    // cd[20..23] = InitialFrames
                    let stream_count = u32::from_le_bytes([cd[24], cd[25], cd[26], cd[27]]);
                    // cd[28..31] = SuggestedBufferSize
                    let width = u32::from_le_bytes([cd[32], cd[33], cd[34], cd[35]]);
                    let height = u32::from_le_bytes([cd[36], cd[37], cd[38], cd[39]]);

                    state.us_per_frame = us_per_frame;
                    state.total_frames = total_frames;

                    if us_per_frame > 0 {
                        let fps = 1_000_000.0_f64 / us_per_frame as f64;
                        // ExifTool prints as int($val * 1000 + 0.5) / 1000
                        let fps_rounded = (fps * 1000.0 + 0.5).floor() / 1000.0;
                        tags.push(mk_riff(family, "FrameRate", "Frame Rate", Value::String(format!("{}", fps_rounded))));
                    }

                    // MaxDataRate: ExifTool prints as "X kB/s" (sprintf("%.4g %s", $tmp, $unit))
                    let kbps = max_data_rate as f64 / 1000.0;
                    let max_data_rate_str = format_sig4(kbps, "kB/s");
                    tags.push(mk_riff(family, "MaxDataRate", "Max Data Rate", Value::String(max_data_rate_str)));

                    tags.push(mk_riff(family, "FrameCount", "Frame Count", Value::U32(total_frames)));
                    tags.push(mk_riff(family, "StreamCount", "Stream Count", Value::U32(stream_count)));
                    tags.push(mk_riff(family, "ImageWidth", "Image Width", Value::U32(width)));
                    tags.push(mk_riff(family, "ImageHeight", "Image Height", Value::U32(height)));
                }
            }
            // Stream Header (strh)
            b"strh" => {
                if chunk_size >= 4 {
                    let cd = &data[chunk_data_start..chunk_data_end];
                    let fcc_type = crate::encoding::decode_utf8_or_latin1(&cd[0..4]).to_string();
                    state.current_stream_type = Some(fcc_type.clone());
                    state.stream_count_seen += 1;
                    let is_first_stream = state.stream_count_seen == 1;

                    // StreamType — Perl uses PRIORITY=>0 so only first stream wins
                    if is_first_stream {
                        let stream_type_str = match fcc_type.as_str() {
                            "auds" => "Audio",
                            "mids" => "MIDI",
                            "txts" => "Text",
                            "vids" => "Video",
                            "iavs" => "Interleaved Audio+Video",
                            _ => &fcc_type,
                        };
                        tags.push(mk_riff(family, "StreamType", "Stream Type", Value::String(stream_type_str.to_string())));
                    }

                    if chunk_size >= 8 {
                        let fcc_handler = crate::encoding::decode_utf8_or_latin1(&cd[4..8]).trim_end_matches('\0').to_string();
                        if fcc_type == "vids" {
                            tags.push(mk_riff(family, "VideoCodec", "Video Codec", Value::String(fcc_handler)));
                        } else if fcc_type == "auds" {
                            tags.push(mk_riff(family, "AudioCodec", "Audio Codec", Value::String(fcc_handler)));
                        }
                    }

                    // Scale (offset 20) and Rate (offset 24) for frame rate (both int32u)
                    if chunk_size >= 28 {
                        let scale = u32::from_le_bytes([cd[20], cd[21], cd[22], cd[23]]);
                        let rate = u32::from_le_bytes([cd[24], cd[25], cd[26], cd[27]]);

                        if fcc_type == "auds" && scale > 0 {
                            // AudioSampleRate = rate/scale
                            let audio_rate = rate as f64 / scale as f64;
                            let audio_rate_rounded = (audio_rate * 100.0 + 0.5).floor() / 100.0;
                            tags.push(mk_riff(family, "AudioSampleRate", "Audio Sample Rate", Value::String(format!("{}", audio_rate_rounded))));
                        } else if fcc_type == "vids" && scale > 0 {
                            // VideoFrameRate = rate/scale
                            let vfr = rate as f64 / scale as f64;
                            let vfr_rounded = (vfr * 1000.0 + 0.5).floor() / 1000.0;
                            state.video_frame_rate = Some(vfr);
                            tags.push(mk_riff(family, "VideoFrameRate", "Video Frame Rate", Value::String(format!("{}", vfr_rounded))));
                        }
                    }

                    // Length (offset 32) = sample count / frame count
                    if chunk_size >= 36 {
                        let length = u32::from_le_bytes([cd[32], cd[33], cd[34], cd[35]]);
                        if fcc_type == "auds" {
                            tags.push(mk_riff(family, "AudioSampleCount", "Audio Sample Count", Value::U32(length)));
                        } else if fcc_type == "vids" {
                            state.video_frame_count = length;
                            tags.push(mk_riff(family, "VideoFrameCount", "Video Frame Count", Value::U32(length)));
                        }
                    }

                    // Quality (offset 40) and SampleSize (offset 44)
                    // Perl uses PRIORITY=>0 so only first stream's values are kept
                    if chunk_size >= 48 && is_first_stream {
                        let quality = u32::from_le_bytes([cd[40], cd[41], cd[42], cd[43]]);
                        let sample_size = u32::from_le_bytes([cd[44], cd[45], cd[46], cd[47]]);

                        let quality_str = if quality == 0xFFFFFFFF {
                            "Default".to_string()
                        } else {
                            format!("{}", quality)
                        };
                        tags.push(mk_riff(family, "Quality", "Quality", Value::String(quality_str)));

                        let sample_size_str = if sample_size == 0 {
                            "Variable".to_string()
                        } else if sample_size == 1 {
                            "1 byte".to_string()
                        } else {
                            format!("{} bytes", sample_size)
                        };
                        tags.push(mk_riff(family, "SampleSize", "Sample Size", Value::String(sample_size_str)));
                    }
                }
            }
            // Stream Format (strf)
            b"strf" => {
                match state.current_stream_type.as_deref() {
                    Some("auds") => {
                        // WAVEFORMATEX
                        parse_wave_format(data, chunk_data_start, chunk_data_end, tags, family);
                    }
                    Some("vids") => {
                        // BITMAPINFOHEADER
                        parse_bitmapinfoheader(data, chunk_data_start, chunk_data_end, tags, family);
                    }
                    _ => {}
                }
            }
            // Stream Data (strd) - may contain EXIF
            b"strd" => {
                // Try AVIF (EXIF in AVI stream data)
                if chunk_data_end >= chunk_data_start + 4 {
                    let tag = &data[chunk_data_start..chunk_data_start + 4];
                    if tag == b"AVIF" && chunk_data_end >= chunk_data_start + 8 {
                        // EXIF data starts at offset 8 in strd
                        if let Ok(exif_tags) = ExifReader::read(&data[chunk_data_start + 8..chunk_data_end]) {
                            tags.extend(exif_tags);
                        }
                    }
                }
            }
            // OpenDML extended AVI header (dmlh)
            b"dmlh" => {
                if chunk_size >= 4 {
                    let cd = &data[chunk_data_start..chunk_data_end];
                    let total_frame_count = u32::from_le_bytes([cd[0], cd[1], cd[2], cd[3]]);
                    tags.push(mk_riff(family, "TotalFrameCount", "Total Frame Count", Value::U32(total_frame_count)));
                }
            }
            // Audio Format (WAV fmt chunk at top level)
            b"fmt " => {
                parse_wave_format(data, chunk_data_start, chunk_data_end, tags, family);
                // Also capture AvgBytesPerSec for WAV duration calculation
                if chunk_size >= 12 {
                    let cd = &data[chunk_data_start..chunk_data_end];
                    state.avg_bytes_per_sec = u32::from_le_bytes([cd[8], cd[9], cd[10], cd[11]]);
                }
            }
            // WAV data chunk (for duration calculation)
            b"data" => {
                state.data_len += chunk_size as u64;
            }
            // IDIT - DateTimeOriginal (top-level or in hdrl)
            b"IDIT" => {
                let s = crate::encoding::decode_utf8_or_latin1(&data[chunk_data_start..chunk_data_end])
                    .trim_end_matches('\0')
                    .to_string();
                if !s.is_empty() {
                    let converted = convert_riff_date(&s);
                    tags.push(mk_riff(family, "DateTimeOriginal", "Date/Time Original", Value::String(converted)));
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
            // Broadcast extension (WAV bext)
            b"bext" => {
                parse_bext(data, chunk_data_start, chunk_data_end, chunk_size, tags, family);
            }
            _ => {}
        }

        pos = chunk_data_end + (chunk_size & 1);
    }

    // After processing all chunks, compute Duration — only at the top level (depth == 0)
    // to avoid emitting Duration once per recursive sub-list call.
    if state.depth > 0 {
        return Ok(());
    }

    // After processing all chunks, compute Duration for WAV
    // Use data chunk size, or fall back to file size (like Perl ExifTool)
    if family == "WAV" && state.avg_bytes_per_sec > 0 {
        let effective_len = if state.data_len > 0 { state.data_len } else { state.file_size };
        if effective_len > 0 {
            let duration = effective_len as f64 / state.avg_bytes_per_sec as f64;
            tags.push(mk_riff(family, "Duration", "Duration", Value::String(format_duration(duration))));
        }
    }

    // After processing all chunks, compute Duration for AVI
    if family == "AVI" && state.us_per_frame > 0 {
        let fps = 1_000_000.0_f64 / state.us_per_frame as f64;
        // Use video frame count/rate if available and ~2-3x difference suggests multi-track
        let dur = if let (Some(vfr), vc) = (state.video_frame_rate, state.video_frame_count) {
            if vc > 0 && vfr > 0.0 {
                let dur1 = state.total_frames as f64 / fps;
                let dur2 = vc as f64 / vfr;
                let rat = dur1 / dur2;
                if rat > 1.9 && rat < 3.1 { dur2 } else { dur1 }
            } else if state.total_frames > 0 {
                state.total_frames as f64 / fps
            } else {
                0.0
            }
        } else if state.total_frames > 0 {
            state.total_frames as f64 / fps
        } else {
            0.0
        };
        if dur > 0.0 {
            tags.push(mk_riff(family, "Duration", "Duration", Value::String(format_duration(dur))));
        }
    }

    Ok(())
}

/// Parse WAVEFORMATEX / AudioFormat chunk
fn parse_wave_format(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>, family: &str) {
    let chunk_size = end - start;
    if chunk_size < 14 {
        return;
    }
    let cd = &data[start..end];
    let format_tag = u16::from_le_bytes([cd[0], cd[1]]);
    let channels = u16::from_le_bytes([cd[2], cd[3]]);
    let sample_rate = u32::from_le_bytes([cd[4], cd[5], cd[6], cd[7]]);
    let avg_bytes = u32::from_le_bytes([cd[8], cd[9], cd[10], cd[11]]);

    let encoding = audio_encoding_name(format_tag);
    tags.push(mk_riff(family, "Encoding", "Encoding", Value::String(encoding.into())));
    tags.push(mk_riff(family, "NumChannels", "Num Channels", Value::U16(channels)));
    tags.push(mk_riff(family, "SampleRate", "Sample Rate", Value::U32(sample_rate)));
    tags.push(mk_riff(family, "AvgBytesPerSec", "Avg Bytes Per Sec", Value::U32(avg_bytes)));

    if chunk_size >= 16 {
        let bits_per_sample = u16::from_le_bytes([cd[14], cd[15]]);
        tags.push(mk_riff(family, "BitsPerSample", "Bits Per Sample", Value::U16(bits_per_sample)));
    }
}

/// Parse BITMAPINFOHEADER (strf for video streams)
fn parse_bitmapinfoheader(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>, family: &str) {
    let chunk_size = end - start;
    if chunk_size < 40 {
        return;
    }
    let cd = &data[start..end];

    // Size of structure = BMPVersion indicator
    let bmp_size = u32::from_le_bytes([cd[0], cd[1], cd[2], cd[3]]);
    let bmp_version = match bmp_size {
        40  => "Windows V3",
        68  => "AVI BMP structure?",
        108 => "Windows V4",
        124 => "Windows V5",
        _   => "Unknown",
    };
    tags.push(mk_riff(family, "BMPVersion", "BMP Version", Value::String(bmp_version.into())));

    // Width/Height are at offsets 4/8 but avih already emitted them; skip redundant ImageWidth/Height here
    // (ExifTool does emit them from strf too via BMP::Main, but they're the same values)
    // Planes at offset 12 (int16u)
    let planes = u16::from_le_bytes([cd[12], cd[13]]);
    tags.push(mk_riff(family, "Planes", "Planes", Value::U16(planes)));

    // BitDepth at offset 14 (int16u)
    let bit_depth = u16::from_le_bytes([cd[14], cd[15]]);
    tags.push(mk_riff(family, "BitDepth", "Bit Depth", Value::U16(bit_depth)));

    // Compression at offset 16 (int32u, but often a FourCC)
    let compression_raw = u32::from_le_bytes([cd[16], cd[17], cd[18], cd[19]]);
    let compression_str = if compression_raw > 256 {
        // FourCC: stored as little-endian, display as string
        let bytes = [cd[16], cd[17], cd[18], cd[19]];
        crate::encoding::decode_utf8_or_latin1(&bytes).to_uppercase()
    } else {
        match compression_raw {
            0 => "None".into(),
            1 => "8-Bit RLE".into(),
            2 => "4-Bit RLE".into(),
            3 => "Bitfields".into(),
            4 => "JPEG".into(),
            5 => "PNG".into(),
            _ => format!("{}", compression_raw).into(),
        }
    };
    tags.push(mk_riff(family, "Compression", "Compression", Value::String(compression_str.to_string())));

    // ImageLength at offset 20 (int32u)
    let image_length = u32::from_le_bytes([cd[20], cd[21], cd[22], cd[23]]);
    tags.push(mk_riff(family, "ImageLength", "Image Length", Value::U32(image_length)));

    // PixelsPerMeterX at offset 24
    let ppm_x = u32::from_le_bytes([cd[24], cd[25], cd[26], cd[27]]);
    tags.push(mk_riff(family, "PixelsPerMeterX", "Pixels Per Meter X", Value::U32(ppm_x)));

    // PixelsPerMeterY at offset 28
    let ppm_y = u32::from_le_bytes([cd[28], cd[29], cd[30], cd[31]]);
    tags.push(mk_riff(family, "PixelsPerMeterY", "Pixels Per Meter Y", Value::U32(ppm_y)));

    // NumColors at offset 32
    let num_colors = u32::from_le_bytes([cd[32], cd[33], cd[34], cd[35]]);
    let num_colors_str = if num_colors == 0 {
        "Use BitDepth".to_string()
    } else {
        format!("{}", num_colors)
    };
    tags.push(mk_riff(family, "NumColors", "Num Colors", Value::String(num_colors_str)));

    // NumImportantColors at offset 36
    let num_important = u32::from_le_bytes([cd[36], cd[37], cd[38], cd[39]]);
    let num_important_str = if num_important == 0 {
        "All".to_string()
    } else {
        format!("{}", num_important)
    };
    tags.push(mk_riff(family, "NumImportantColors", "Num Important Colors", Value::String(num_important_str)));
}

/// Parse broadcast audio extension chunk (bext)
fn parse_bext(data: &[u8], start: usize, end: usize, chunk_size: usize, tags: &mut Vec<Tag>, family: &str) {
    if chunk_size < 256 {
        return;
    }
    let cd = &data[start..end];

    // Description: 256 bytes at offset 0
    let description = crate::encoding::decode_utf8_or_latin1(&cd[..256.min(cd.len())])
        .trim_end_matches('\0')
        .to_string();
    if !description.is_empty() {
        tags.push(mk_riff(family, "Description", "Description", Value::String(description)));
    }

    if cd.len() >= 288 {
        // Originator: 32 bytes at offset 256
        let originator = crate::encoding::decode_utf8_or_latin1(&cd[256..288])
            .trim_end_matches('\0')
            .to_string();
        if !originator.is_empty() {
            tags.push(mk_riff(family, "Originator", "Originator", Value::String(originator)));
        }
    }

    if cd.len() >= 320 {
        // OriginatorReference: 32 bytes at offset 288
        let orig_ref = crate::encoding::decode_utf8_or_latin1(&cd[288..320])
            .trim_end_matches('\0')
            .to_string();
        if !orig_ref.is_empty() {
            tags.push(mk_riff(family, "OriginatorReference", "Originator Reference", Value::String(orig_ref)));
        }
    }

    if cd.len() >= 338 {
        // DateTimeOriginal: 18 bytes at offset 320 (format: "YYYY-MM-DD HH:MM:SS")
        let dt_str = crate::encoding::decode_utf8_or_latin1(&cd[320..338])
            .trim_end_matches('\0')
            .to_string();
        if !dt_str.is_empty() {
            // Convert YYYY-MM-DD to YYYY:MM:DD
            let converted = dt_str.replace('-', ":");
            let converted = if converted.len() >= 10 {
                format!("{} {}", &converted[..10], &converted[10..].trim())
            } else {
                converted
            };
            tags.push(mk_riff(family, "DateTimeOriginal", "Date/Time Original", Value::String(converted.trim().to_string())));
        }
    }
}

/// Read Pentax AVI sub-chunks from LIST hydt or LIST pntx.
/// Contains hymn or mknt chunks with Pentax MakerNotes (Pentax::Main IFD).
/// Mirrors Perl: LIST_hydt => PentaxData => TagTable Pentax::AVI => hymn => MakerNotes.
fn read_pentax_avi_chunks(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) -> Result<()> {
    let mut pos = start;

    while pos + 8 <= end {
        let chunk_id = &data[pos..pos + 4];
        let chunk_size = u32::from_le_bytes([
            data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7],
        ]) as usize;
        let data_start = pos + 8;
        let data_end = (data_start + chunk_size).min(end);

        if data_start > end {
            break;
        }

        match chunk_id {
            b"hymn" | b"mknt" => {
                // Pentax MakerNotes: data starts with "PENTAX \0" header (8 bytes),
                // then byte-order mark (MM or II, 2 bytes), IFD at offset 10.
                // Base = start of chunk data (all pointers relative to chunk start).
                let mn_data = &data[data_start..data_end];
                if mn_data.len() >= 12 && mn_data.starts_with(b"PENTAX \0") {
                    // Detect byte order from bytes 8-9 (MM = big-endian, II = little-endian)
                    let bo = if mn_data[8] == b'M' && mn_data[9] == b'M' {
                        ByteOrderMark::BigEndian
                    } else {
                        ByteOrderMark::LittleEndian
                    };
                    let mn_tags = crate::metadata::makernotes::parse_makernotes(
                        mn_data, 0, mn_data.len(), "PENTAX", "", bo,
                    );
                    tags.extend(mn_tags);
                }
            }
            _ => {}
        }

        pos = data_end + (chunk_size & 1);
    }

    Ok(())
}

/// Read LIST exif sub-chunks (EXIF 2.3 WAV metadata)
fn read_exif_list_chunks(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>, family: &str) -> Result<()> {
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

        let raw_bytes = &data[pos..pos + chunk_size];
        let value = crate::encoding::decode_utf8_or_latin1(raw_bytes)
            .trim_end_matches('\0')
            .to_string();

        if !value.is_empty() {
            match chunk_id {
                "ever" => tags.push(mk_riff(family, "ExifVersion", "Exif Version", Value::String(value))),
                "erel" => tags.push(mk_riff(family, "RelatedImageFile", "Related Image File", Value::String(value))),
                "etim" => tags.push(mk_riff(family, "TimeCreated", "Time Created", Value::String(value))),
                "ecor" => tags.push(mk_riff(family, "Make", "Make", Value::String(value))),
                "emdl" => tags.push(mk_riff(family, "Model", "Camera Model Name", Value::String(value))),
                "emnt" => tags.push(mk_riff(family, "MakerNotes", "Maker Notes", Value::Binary(raw_bytes.to_vec()))),
                "eucm" => tags.push(mk_riff(family, "UserComment", "User Comment", Value::String(value))),
                _ => {}
            }
        }

        pos += chunk_size + (chunk_size & 1);
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

        let value = crate::encoding::decode_utf8_or_latin1(&data[pos..pos + chunk_size])
            .trim_end_matches('\0')
            .to_string();

        if !value.is_empty() {
            let (name, description) = info_chunk_name(chunk_id);
            // ICRD: convert date separators from '-' to ':' (like Perl ExifTool ValueConv)
            let value = if chunk_id == "ICRD" {
                value.replace('-', ":")
            } else {
                value
            };
            tags.push(mk_riff(family, name, description, Value::String(value)));
        }

        pos += chunk_size + (chunk_size & 1);
    }

    Ok(())
}

/// Map RIFF INFO chunk IDs to tag names.
fn info_chunk_name(id: &str) -> (&str, &str) {
    match id {
        "IARL" => ("ArchivalLocation", "Archival Location"),
        "IART" => ("Artist", "Artist"),
        "ICMS" => ("Commissioned", "Commissioned"),
        "ICMT" => ("Comment", "Comment"),
        "ICOP" => ("Copyright", "Copyright"),
        "ICRD" => ("DateCreated", "Date Created"),
        "ICRP" => ("Cropped", "Cropped"),
        "IDIM" => ("Dimensions", "Dimensions"),
        "IDPI" => ("DotsPerInch", "Dots Per Inch"),
        "IENG" => ("Engineer", "Engineer"),
        "IGNR" => ("Genre", "Genre"),
        "IKEY" => ("Keywords", "Keywords"),
        "ILGT" => ("Lightness", "Lightness"),
        "IMED" => ("Medium", "Medium"),
        "INAM" => ("Title", "Title"),
        "ITRK" => ("TrackNumber", "Track Number"),
        "IPLT" => ("NumColors", "Num Colors"),
        "IPRD" => ("Product", "Product"),
        "ISBJ" => ("Subject", "Subject"),
        "ISFT" => ("Software", "Software"),
        "ISHP" => ("Sharpness", "Sharpness"),
        "ISRC" => ("Source", "Source"),
        "ISRF" => ("SourceForm", "Source Form"),
        "ITCH" => ("Technician", "Technician"),
        "ISGN" => ("SecondaryGenre", "Secondary Genre"),
        "IWRI" => ("WrittenBy", "Written By"),
        "IPRO" => ("ProducedBy", "Produced By"),
        "ICNM" => ("Cinematographer", "Cinematographer"),
        "IPDS" => ("ProductionDesigner", "Production Designer"),
        "IEDT" => ("EditedBy", "Edited By"),
        "ICDS" => ("CostumeDesigner", "Costume Designer"),
        "IMUS" => ("MusicBy", "Music By"),
        "ISTD" => ("ProductionStudio", "Production Studio"),
        "IDST" => ("DistributedBy", "Distributed By"),
        "ICNT" => ("Country", "Country"),
        "ILNG" => ("Language", "Language"),
        "IRTD" => ("Rating", "Rating"),
        "ISTR" => ("Starring", "Starring"),
        "TITL" => ("Title", "Title"),
        "DIRC" => ("Directory", "Directory"),
        "YEAR" => ("Year", "Year"),
        "GENR" => ("Genre", "Genre"),
        "COMM" => ("Comments", "Comments"),
        "LANG" => ("Language", "Language"),
        "AGES" => ("Rated", "Rated"),
        "STAR" => ("Starring", "Starring"),
        "CODE" => ("EncodedBy", "Encoded By"),
        "PRT1" => ("Part", "Part"),
        "PRT2" => ("NumberOfParts", "Number Of Parts"),
        "IDIT" => ("DateTimeOriginal", "Date/Time Original"),
        "ISMP" => ("TimeCode", "Time Code"),
        "DISP" => ("SoundSchemeTitle", "Sound Scheme Title"),
        "TLEN" => ("Length", "Length"),
        "TRCK" => ("TrackNumber", "Track Number"),
        "TURL" => ("URL", "URL"),
        "TVER" => ("Version", "Version"),
        "LOCA" => ("Location", "Location"),
        "TORG" => ("Organization", "Organization"),
        "TAPE" => ("TapeName", "Tape Name"),
        "CMNT" => ("Comment", "Comment"),
        "RATE" => ("Rate", "Rate"),
        "IENC" => ("EncodedBy", "Encoded By"),
        "IRIP" => ("RippedBy", "Ripped By"),
        _ => (id, id),
    }
}

/// Map audio format tag to encoding name (TwoCC)
fn audio_encoding_name(format_tag: u16) -> &'static str {
    match format_tag {
        0x0001 => "Microsoft PCM",
        0x0002 => "Microsoft ADPCM",
        0x0003 => "Microsoft IEEE float",
        0x0004 => "Compaq VSELP",
        0x0005 => "IBM CVSD",
        0x0006 => "Microsoft a-Law",
        0x0007 => "Microsoft u-Law",
        0x0008 => "Microsoft DTS",
        0x0009 => "DRM",
        0x000a => "WMA 9 Speech",
        0x000b => "Microsoft Windows Media RT Voice",
        0x0010 => "OKI-ADPCM",
        0x0011 => "Intel IMA/DVI-ADPCM",
        0x0050 => "Microsoft MPEG",
        0x0055 => "MP3",
        0x00ff => "AAC",
        0x0161 => "Windows Media Audio V2 V7 V8 V9 / DivX audio (WMA) / Alex AC3 Audio",
        0x0162 => "Windows Media Audio Professional V9",
        0x0163 => "Windows Media Audio Lossless V9",
        0xfffe => "Extensible",
        0xffff => "Development",
        _ => "Unknown",
    }
}

/// Convert RIFF date string to EXIF format (YYYY:MM:DD HH:MM:SS)
fn convert_riff_date(val: &str) -> String {
    let months = [
        "jan", "feb", "mar", "apr", "may", "jun",
        "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let parts: Vec<&str> = val.split_whitespace().collect();

    // Format: "Mon Mar 10 15:04:43 2003"
    if parts.len() >= 5 {
        let month_lower = parts[1].to_lowercase();
        if let Some(mon_idx) = months.iter().position(|&m| m == month_lower) {
            if let (Ok(day), Ok(year)) = (parts[2].parse::<u32>(), parts[4].parse::<u32>()) {
                return format!("{:04}:{:02}:{:02} {}", year, mon_idx + 1, day, parts[3]);
            }
        }
    }

    // Format: "YYYY/MM/DD HH:MM" or "YYYY/MM/DD/ HH:MM"
    if let Some(cap) = parse_casio_date(val) {
        return cap;
    }

    // Format: "YYYY-MM-DD HH:MM:SS" or "YYYY/MM/DD HH:MM:SS"
    let normalized = val.replace('/', ":").replace('-', ":");
    if let Some(colon_pos) = normalized.find(' ') {
        let date_part = &normalized[..colon_pos];
        let time_part = normalized[colon_pos + 1..].trim();
        if date_part.len() == 10 {
            return format!("{} {}", date_part, time_part);
        }
    }

    val.to_string()
}

fn parse_casio_date(val: &str) -> Option<String> {
    // Perl pattern: m{(\d{4})/\s*(\d+)/\s*(\d+)/?\s+(\d+):\s*(\d+)\s*(P?)}
    // Casio/Pentax AVI format: "YYYY/MM/DD HH:MM:SS" — only HH:MM is captured (seconds dropped)
    // e.g. "2009/10/27 12:14:34" → "2009:10:27 12:14:00"
    // e.g. "2001/ 1/27  1:42PM"  → "2001:01:27 13:42:00"
    // e.g. "2005/11/28/ 09:19"   → "2005:11:28 09:19:00"
    let bytes = val.as_bytes();
    let len = bytes.len();

    // Parse YYYY
    if len < 4 || !bytes[0..4].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let year: u32 = val[0..4].parse().ok()?;
    let mut pos = 4;
    if pos >= len || bytes[pos] != b'/' { return None; }
    pos += 1;
    // Skip optional spaces
    while pos < len && bytes[pos] == b' ' { pos += 1; }
    // Parse MM
    let month_start = pos;
    while pos < len && bytes[pos].is_ascii_digit() { pos += 1; }
    if pos == month_start { return None; }
    let month: u32 = val[month_start..pos].parse().ok()?;
    if pos >= len || bytes[pos] != b'/' { return None; }
    pos += 1;
    // Skip optional spaces
    while pos < len && bytes[pos] == b' ' { pos += 1; }
    // Parse DD
    let day_start = pos;
    while pos < len && bytes[pos].is_ascii_digit() { pos += 1; }
    if pos == day_start { return None; }
    let day: u32 = val[day_start..pos].parse().ok()?;
    // Skip optional trailing '/'
    if pos < len && bytes[pos] == b'/' { pos += 1; }
    // Skip whitespace
    while pos < len && bytes[pos] == b' ' { pos += 1; }
    if pos >= len { return None; }
    // Parse HH
    let hh_start = pos;
    while pos < len && bytes[pos].is_ascii_digit() { pos += 1; }
    if pos == hh_start { return None; }
    let hh: u32 = val[hh_start..pos].parse().ok()?;
    if pos >= len || bytes[pos] != b':' { return None; }
    pos += 1;
    // Skip optional spaces
    while pos < len && bytes[pos] == b' ' { pos += 1; }
    // Parse MM (minutes)
    let mm_start = pos;
    while pos < len && bytes[pos].is_ascii_digit() { pos += 1; }
    if pos == mm_start { return None; }
    let mm: u32 = val[mm_start..pos].parse().ok()?;
    // Skip optional spaces and check for PM
    while pos < len && bytes[pos] == b' ' { pos += 1; }
    let pm = pos < len && (bytes[pos] == b'P' || bytes[pos] == b'p');
    let hh_final = if pm { hh + 12 } else { hh };
    Some(format!("{:04}:{:02}:{:02} {:02}:{:02}:00", year, month, day, hh_final, mm))
}

/// Format duration in seconds like ExifTool's ConvertDuration
fn format_duration(seconds: f64) -> String {
    // ExifTool uses ConvertDuration which formats as "H:MM:SS" or fractional seconds
    // For small values, it may output "0.03 s" style.
    // Looking at the test output: "15.53 s" for AVI, "0.03 s" for WAV
    // ExifTool PrintConv for Duration composite calls ConvertDuration()
    // which outputs "H:MM:SS.ss" format for values >= 60s, else "S.ss s"
    if seconds < 60.0 {
        // round to 2 decimal places
        let rounded = (seconds * 100.0 + 0.5).floor() / 100.0;
        format!("{:.2} s", rounded)
    } else {
        let hours = (seconds / 3600.0).floor() as u64;
        let remaining = seconds - hours as f64 * 3600.0;
        let minutes = (remaining / 60.0).floor() as u64;
        let secs = remaining - minutes as f64 * 60.0;
        if hours > 0 {
            format!("{}:{:02}:{:05.2}", hours, minutes, secs)
        } else {
            format!("{}:{:05.2}", minutes, secs)
        }
    }
}

/// Format a float value with ~4 significant digits followed by a unit (like Perl's sprintf("%.4g %s"))
fn format_sig4(val: f64, unit: &str) -> String {
    // Implement %.4g: use at most 4 significant figures, no trailing zeros
    if val == 0.0 {
        return format!("0 {}", unit);
    }
    let magnitude = val.abs().log10().floor() as i32;
    let decimals = if magnitude >= 3 { 0 } else { (3 - magnitude).max(0) as usize };
    let s = format!("{:.prec$}", val, prec = decimals);
    // Remove trailing zeros after decimal point
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    };
    format!("{} {}", s, unit)
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
