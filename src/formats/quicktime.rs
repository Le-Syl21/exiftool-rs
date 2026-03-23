//! QuickTime/MP4/M4A/MOV file format reader.
//!
//! Parses ISO Base Media File Format (ISOBMFF) atom/box tree to extract
//! metadata from moov/udta/meta/ilst and embedded EXIF/XMP.
//! Mirrors ExifTool's QuickTime.pm.

use crate::error::{Error, Result};
use crate::metadata::makernotes::parse_canon_cr3_makernotes;
use crate::metadata::{ExifReader, XmpReader};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

/// Parser state carried through recursive atom parsing.
#[derive(Default, Clone)]
struct QtState {
    /// Timescale from mvhd (for duration conversions at movie level)
    movie_timescale: u32,
    /// Timescale from mdhd (for the current media track)
    media_timescale: u32,
    /// Current handler type ('vide', 'soun', etc.)
    handler_type: [u8; 4],
    /// MovieHeaderVersion (0 or 1)
    movie_header_version: u8,
    /// TrackHeaderVersion (0 or 1)
    track_header_version: u8,
    /// MediaHeaderVersion (0 or 1)
    media_header_version: u8,
    /// Whether we already emitted HEVC config (to avoid duplicates from multiple hvcC boxes)
    hevc_config_done: bool,
    /// Whether we already emitted image spatial extent (to avoid duplicates)
    ispe_done: bool,
    /// CTMD sample offset in file (from co64/stco in meta handler track)
    ctmd_offset: Option<u64>,
    /// CTMD sample size in bytes
    ctmd_size: Option<u32>,
    /// Whether the current track's stsd format is CTMD
    current_track_is_ctmd: bool,
    /// Whether the current track's stsd format is JPEG (for JpgFromRaw)
    current_track_is_jpeg: bool,
    /// JPEG track sample offset and size (for JpgFromRaw extraction)
    jpeg_offset: Option<u64>,
    jpeg_size: Option<u32>,
    /// ExtractEmbedded level (0=off, 1=-ee, 2=-ee2, 3=-ee3)
    extract_embedded: u8,
    /// Completed tracks for timed metadata extraction
    stream_tracks: Vec<super::quicktime_stream::TrackInfo>,
    /// Current track being built (for stream extraction)
    stream_current: super::quicktime_stream::TrackInfo,
    /// stsd format for the current track (meta_format)
    current_stsd_format: Option<String>,
}

pub fn read_quicktime(data: &[u8]) -> Result<Vec<Tag>> {
    read_quicktime_with_ee(data, 0)
}

/// Read QuickTime metadata, optionally extracting embedded timed metadata.
pub fn read_quicktime_with_ee(data: &[u8], extract_embedded: u8) -> Result<Vec<Tag>> {
    if data.len() < 8 {
        return Err(Error::InvalidData("file too small for QuickTime".into()));
    }

    let mut tags = Vec::new();

    // Check for ftyp
    if data.len() >= 12 && &data[4..8] == b"ftyp" {
        let size = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let brand_raw = String::from_utf8_lossy(&data[8..12]).to_string();
        let brand_display = ftyp_brand_name(&brand_raw)
            .unwrap_or(brand_raw.as_str())
            .to_string();
        tags.push(mk("MajorBrand", "Major Brand", Value::String(brand_display)));
        if size >= 16 && data.len() >= 16 {
            let minor_raw = u32::from_be_bytes([data[12], data[13], data[14], data[15]]);
            // Format minor version as X.X.X (each byte)
            let mv = format!(
                "{}.{}.{}",
                (minor_raw >> 16) & 0xFF,
                (minor_raw >> 8) & 0xFF,
                minor_raw & 0xFF
            );
            tags.push(mk("MinorVersion", "Minor Version", Value::String(mv)));
        }
        // Compatible brands
        if size > 16 {
            let mut brands = Vec::new();
            let mut pos = 16;
            while pos + 4 <= size.min(data.len()) {
                let b = String::from_utf8_lossy(&data[pos..pos + 4]).trim().to_string();
                if !b.is_empty() {
                    brands.push(b);
                }
                pos += 4;
            }
            if !brands.is_empty() {
                tags.push(mk(
                    "CompatibleBrands",
                    "Compatible Brands",
                    Value::String(brands.join(", ")),
                ));
            }
        }
    }

    // Parse atom tree
    let mut state = QtState::default();
    state.extract_embedded = extract_embedded;
    parse_atoms(data, 0, data.len(), &mut tags, &mut state, 0);

    // Compute composite AvgBitrate from MediaDataSize and Duration
    // Mirrors Perl QuickTime composite AvgBitrate:
    //   sum all MediaDataSize values, divide by Duration in seconds, multiply by 8
    {
        // Find Duration in seconds (from movie-level "Duration" tag which is already in s)
        let duration_secs: Option<f64> = tags.iter().find_map(|t| {
            if t.name == "Duration" {
                if let Value::String(ref s) = t.raw_value {
                    // Format is e.g. "29.05 s" or "0:01:23"
                    if let Some(stripped) = s.strip_suffix(" s") {
                        return stripped.parse::<f64>().ok();
                    }
                    // hh:mm:ss format
                    let parts: Vec<&str> = s.split(':').collect();
                    if parts.len() == 3 {
                        if let (Ok(h), Ok(m), Ok(s)) = (
                            parts[0].parse::<f64>(),
                            parts[1].parse::<f64>(),
                            parts[2].parse::<f64>(),
                        ) {
                            return Some(h * 3600.0 + m * 60.0 + s);
                        }
                    }
                }
            }
            None
        });

        if let Some(dur) = duration_secs {
            if dur > 0.0 {
                // Sum all MediaDataSize values
                let total_size: u64 = tags.iter().filter_map(|t| {
                    if t.name == "MediaDataSize" {
                        if let Value::U32(v) = t.raw_value { Some(v as u64) } else { None }
                    } else {
                        None
                    }
                }).sum();

                let bitrate = (total_size as f64 * 8.0 / dur + 0.5) as u64;
                tags.push(mk(
                    "AvgBitrate",
                    "Avg Bitrate",
                    Value::String(convert_bitrate(bitrate)),
                ));
            } else {
                // Duration is 0 or effectively zero
                tags.push(mk(
                    "AvgBitrate",
                    "Avg Bitrate",
                    Value::String("0 bps".to_string()),
                ));
            }
        }
    }

    // Parse Canon CTMD (Canon Timed MetaData) if found
    if let (Some(ctmd_off), Some(ctmd_sz)) = (state.ctmd_offset, state.ctmd_size) {
        let ctmd_off = ctmd_off as usize;
        let ctmd_sz = ctmd_sz as usize;
        if ctmd_off + ctmd_sz <= data.len() {
            parse_canon_ctmd(data, ctmd_off, ctmd_sz, &mut tags);
        }
    }

    // Extract JpgFromRaw from JPEG track sample data
    if let (Some(jpg_off), Some(jpg_sz)) = (state.jpeg_offset, state.jpeg_size) {
        let jpg_off = jpg_off as usize;
        let jpg_sz = jpg_sz as usize;
        if jpg_sz > 0 && jpg_off + jpg_sz <= data.len() {
            let jpg_data = &data[jpg_off..jpg_off + jpg_sz];
            tags.push(Tag {
                id: TagId::Text("JpgFromRaw".into()),
                name: "JpgFromRaw".into(),
                description: "Jpg From Raw".into(),
                group: TagGroup { family0: "QuickTime".into(), family1: "QuickTime".into(), family2: "Preview".into() },
                raw_value: Value::Binary(jpg_data.to_vec()),
                print_value: format!("(Binary data {} bytes, use -b option to extract)", jpg_sz),
                priority: 0,
            });
        }
    }

    // Extract timed metadata from stream tracks when -ee is used
    if extract_embedded > 0 {
        // Finalize the last track being built (if any)
        if state.stream_current.handler_type != [0; 4]
            || state.stream_current.meta_format.is_some()
        {
            state.stream_tracks.push(state.stream_current.clone());
        }
        if !state.stream_tracks.is_empty() {
            let stream_tags = super::quicktime_stream::extract_stream_tags(
                data,
                &state.stream_tracks,
                extract_embedded,
            );
            if !stream_tags.is_empty() {
                tags.extend(stream_tags);
            } else {
                tags.push(mk(
                    "Warning",
                    "Warning",
                    Value::String(
                        "[minor] The ExtractEmbedded option may find more tags in the video data"
                            .to_string(),
                    ),
                ));
            }
        }
    }

    Ok(tags)
}

/// Parse Canon CTMD (Canon Timed MetaData) records.
/// Format: records of size(4LE) + type(2LE) + header(6) + data.
/// Types 7/8/9 contain ExifInfo with embedded MakerNotes.
fn parse_canon_ctmd(data: &[u8], start: usize, size: usize, tags: &mut Vec<Tag>) {
    let end = start + size;
    let mut pos = start;

    while pos + 12 < end {
        let rec_size = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
        let rec_type = u16::from_le_bytes([data[pos+4], data[pos+5]]);

        if rec_size < 12 || pos + rec_size > end { break; }

        let rec_data = &data[pos + 12..pos + rec_size];

        match rec_type {
            1 => {
                // TimeStamp: 2 bytes skip + year(2LE) + month + day + hour + min + sec + centisec
                if rec_data.len() >= 9 {
                    let year = u16::from_le_bytes([rec_data[2], rec_data[3]]);
                    let ts = format!("{:04}:{:02}:{:02} {:02}:{:02}:{:02}.{:02}",
                        year, rec_data[4], rec_data[5], rec_data[6], rec_data[7], rec_data[8],
                        if rec_data.len() > 9 { rec_data[9] } else { 0 });
                    tags.push(mk("TimeStamp", "Time Stamp", Value::String(ts)));
                }
            }
            7 | 8 | 9 => {
                // ExifInfo: records of size(4LE)+tag(4LE)+TIFF_data
                // Tags: 0x8769=ExifIFD, 0x927C=MakerNote
                let mut epos = 0;
                while epos + 8 < rec_data.len() {
                    let elen = u32::from_le_bytes([rec_data[epos], rec_data[epos+1], rec_data[epos+2], rec_data[epos+3]]) as usize;
                    let etag = u32::from_le_bytes([rec_data[epos+4], rec_data[epos+5], rec_data[epos+6], rec_data[epos+7]]);
                    if elen < 8 || epos + elen > rec_data.len() { break; }
                    let edata = &rec_data[epos+8..epos+elen];
                    match etag {
                        0x927C => {
                            // MakerNoteCanon: TIFF containing Canon MakerNote IFD
                            // CTMD has the full MakerNote — replace any CMT3 versions
                            let model = tags.iter().find(|t| t.name == "Model")
                                .map(|t| t.print_value.clone()).unwrap_or_default();
                            let mn_tags = parse_canon_cr3_makernotes(edata, &model);
                            for t in mn_tags {
                                // Replace existing tag with CTMD version (CTMD has priority)
                                tags.retain(|e| e.name != t.name);
                                tags.push(t);
                            }
                        }
                        0x8769 => {
                            // ExifIFD: parse as TIFF
                            if let Ok(exif_tags) = crate::metadata::ExifReader::read(edata) {
                                for t in exif_tags {
                                    if !tags.iter().any(|e| e.name == t.name) {
                                        tags.push(t);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                    epos += elen;
                }
            }
            _ => {}
        }

        pos += rec_size;
    }
}

/// Convert a bitrate in bps to a human-readable string.
/// Mirrors ExifTool's ConvertBitrate(): uses %.3g for <100, %.0f for >=100.
fn convert_bitrate(bps: u64) -> String {
    let mut val = bps as f64;
    let units = ["bps", "kbps", "Mbps", "Gbps"];
    let mut idx = 0;
    while val >= 1000.0 && idx + 1 < units.len() {
        val /= 1000.0;
        idx += 1;
    }
    let num_str = if val < 100.0 {
        // %.3g: 3 significant figures
        format_3g(val)
    } else {
        format!("{:.0}", val)
    };
    format!("{} {}", num_str, units[idx])
}

/// Format a float with up to 3 significant figures (like Perl's %.3g).
fn format_3g(val: f64) -> String {
    if val == 0.0 {
        return "0".to_string();
    }
    // Use 3 significant digits
    let mag = val.abs().log10().floor() as i32;
    let decimals = (2 - mag).max(0) as usize;
    let s = format!("{:.prec$}", val, prec = decimals);
    // Strip trailing zeros after decimal point
    if s.contains('.') {
        let s = s.trim_end_matches('0').trim_end_matches('.');
        s.to_string()
    } else {
        s
    }
}

/// Recursively parse QuickTime atoms.
fn parse_atoms(
    data: &[u8],
    start: usize,
    end: usize,
    tags: &mut Vec<Tag>,
    state: &mut QtState,
    depth: u32,
) {
    if depth > 20 {
        return; // Prevent infinite recursion
    }

    let mut pos = start;

    while pos + 8 <= end {
        let mut size = u32::from_be_bytes([
            data[pos],
            data[pos + 1],
            data[pos + 2],
            data[pos + 3],
        ]) as u64;
        let atom_type = &data[pos + 4..pos + 8];
        let header_size;

        if size == 1 && pos + 16 <= end {
            // Extended size (64-bit)
            size = u64::from_be_bytes([
                data[pos + 8],
                data[pos + 9],
                data[pos + 10],
                data[pos + 11],
                data[pos + 12],
                data[pos + 13],
                data[pos + 14],
                data[pos + 15],
            ]);
            header_size = 16;
        } else if size == 0 {
            // Atom extends to end of file
            size = (end - pos) as u64;
            header_size = 8;
        } else {
            header_size = 8;
        }

        let atom_end = (pos as u64 + size) as usize;
        if atom_end > end || size < header_size as u64 {
            break;
        }

        let content_start = pos + header_size;
        let content_end = atom_end;

        match atom_type {
            // Container atoms - recurse into (these just contain sub-atoms)
            b"moov" | b"edts" => {
                parse_atoms(data, content_start, content_end, tags, state, depth + 1);
            }
            b"trak" => {
                // Finalize previous stream track (if any) before starting a new one
                if state.extract_embedded > 0
                    && (state.stream_current.handler_type != [0; 4]
                        || state.stream_current.meta_format.is_some())
                {
                    state.stream_tracks.push(state.stream_current.clone());
                }
                state.stream_current = super::quicktime_stream::TrackInfo::default();
                state.current_stsd_format = None;
                // Reset per-track CTMD flag when entering a new track
                state.current_track_is_ctmd = false;
                state.current_track_is_jpeg = false;
                parse_atoms(data, content_start, content_end, tags, state, depth + 1);
            }
            // mdia: recurse but reset media-level state
            b"mdia" => {
                parse_atoms(data, content_start, content_end, tags, state, depth + 1);
            }
            // minf, stbl, dinf: container
            b"minf" | b"stbl" | b"dinf" => {
                parse_atoms(data, content_start, content_end, tags, state, depth + 1);
            }
            // tapt (track aperture mode dimensions)
            b"tapt" => {
                parse_atoms(data, content_start, content_end, tags, state, depth + 1);
            }
            // User data
            b"udta" => {
                parse_atoms(data, content_start, content_end, tags, state, depth + 1);
            }
            // Metadata container: meta has a 4-byte version/flags before sub-atoms
            b"meta" => {
                if content_start + 4 <= content_end {
                    parse_atoms(
                        data,
                        content_start + 4,
                        content_end,
                        tags,
                        state,
                        depth + 1,
                    );
                }
            }
            // iTunes item list
            b"ilst" => {
                parse_ilst(data, content_start, content_end, tags);
            }
            // Movie header
            b"mvhd" => {
                parse_mvhd(data, content_start, content_end, tags, state);
            }
            // Track header
            b"tkhd" => {
                parse_tkhd(data, content_start, content_end, tags, state);
            }
            // Media header
            b"mdhd" => {
                parse_mdhd(data, content_start, content_end, tags, state);
            }
            // Handler reference
            b"hdlr" => {
                parse_hdlr(data, content_start, content_end, tags, state);
            }
            // Video media header
            b"vmhd" => {
                parse_vmhd(data, content_start, content_end, tags);
            }
            // Audio media header
            b"smhd" => {
                parse_smhd(data, content_start, content_end, tags);
            }
            // Sample description
            b"stsd" => {
                parse_stsd(data, content_start, content_end, tags, state);
            }
            // Time-to-sample (stts) -- used for VideoFrameRate
            b"stts" => {
                parse_stts(data, content_start, content_end, tags, state);
            }
            // Chunk offset table (32-bit) - used to locate CTMD/JPEG sample data
            b"stco" => {
                let d = &data[content_start..content_end];
                if d.len() >= 12 {
                    let entry_count = u32::from_be_bytes([d[4], d[5], d[6], d[7]]) as usize;
                    if entry_count > 0 && d.len() >= 8 + entry_count * 4 {
                        let offset = u32::from_be_bytes([d[8], d[9], d[10], d[11]]) as u64;
                        if state.current_track_is_ctmd && state.ctmd_offset.is_none() {
                            state.ctmd_offset = Some(offset);
                        }
                        if state.current_track_is_jpeg && state.jpeg_offset.is_none() {
                            state.jpeg_offset = Some(offset);
                        }
                        // Collect all chunk offsets for stream extraction
                        if state.extract_embedded > 0 {
                            let max_entries = entry_count.min((d.len() - 8) / 4);
                            for i in 0..max_entries {
                                let off = u32::from_be_bytes([
                                    d[8 + i * 4], d[9 + i * 4], d[10 + i * 4], d[11 + i * 4],
                                ]) as u64;
                                state.stream_current.stco.push(off);
                            }
                        }
                    }
                }
            }
            // Chunk offset table (64-bit) - used to locate CTMD sample data
            b"co64" => {
                let d = &data[content_start..content_end];
                if d.len() >= 16 {
                    let entry_count = u32::from_be_bytes([d[4], d[5], d[6], d[7]]) as usize;
                    if entry_count > 0 && d.len() >= 8 + entry_count * 8 {
                        let offset = u64::from_be_bytes([d[8], d[9], d[10], d[11], d[12], d[13], d[14], d[15]]);
                        if state.current_track_is_ctmd && state.ctmd_offset.is_none() {
                            state.ctmd_offset = Some(offset);
                        }
                        if state.current_track_is_jpeg && state.jpeg_offset.is_none() {
                            state.jpeg_offset = Some(offset);
                        }
                        // Collect all chunk offsets for stream extraction
                        if state.extract_embedded > 0 {
                            let max_entries = entry_count.min((d.len() - 8) / 8);
                            for i in 0..max_entries {
                                let off = u64::from_be_bytes([
                                    d[8 + i * 8], d[9 + i * 8], d[10 + i * 8], d[11 + i * 8],
                                    d[12 + i * 8], d[13 + i * 8], d[14 + i * 8], d[15 + i * 8],
                                ]);
                                state.stream_current.stco.push(off);
                            }
                        }
                    }
                }
            }
            // Sample sizes - used to get CTMD/JPEG sample size
            b"stsz" => {
                let d = &data[content_start..content_end];
                if d.len() >= 12 {
                    let sample_size = u32::from_be_bytes([d[4], d[5], d[6], d[7]]);
                    let sample_count = u32::from_be_bytes([d[8], d[9], d[10], d[11]]) as usize;
                    let first_size = if sample_size > 0 {
                        sample_size
                    } else if d.len() >= 16 {
                        u32::from_be_bytes([d[12], d[13], d[14], d[15]])
                    } else { 0 };
                    if state.current_track_is_ctmd && state.ctmd_size.is_none() && first_size > 0 {
                        state.ctmd_size = Some(first_size);
                    }
                    if state.current_track_is_jpeg && state.jpeg_size.is_none() && first_size > 0 {
                        state.jpeg_size = Some(first_size);
                    }
                    // Collect all sample sizes for stream extraction
                    if state.extract_embedded > 0 {
                        if sample_size > 0 {
                            // All samples have the same size
                            for _ in 0..sample_count {
                                state.stream_current.stsz.push(sample_size);
                            }
                        } else {
                            // Individual sizes at offset 12
                            let max_samples = sample_count.min((d.len() - 12) / 4);
                            for i in 0..max_samples {
                                let sz = u32::from_be_bytes([
                                    d[12 + i * 4], d[13 + i * 4], d[14 + i * 4], d[15 + i * 4],
                                ]);
                                state.stream_current.stsz.push(sz);
                            }
                        }
                    }
                }
            }
            // Sample-to-chunk table - used for stream extraction
            b"stsc" => {
                let d = &data[content_start..content_end];
                if d.len() >= 8 {
                    let entry_count = u32::from_be_bytes([d[4], d[5], d[6], d[7]]) as usize;
                    if state.extract_embedded > 0 && d.len() >= 8 + entry_count * 12 {
                        for i in 0..entry_count {
                            let off = 8 + i * 12;
                            let first_chunk = u32::from_be_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]]);
                            let spc = u32::from_be_bytes([d[off + 4], d[off + 5], d[off + 6], d[off + 7]]);
                            let desc_idx = u32::from_be_bytes([d[off + 8], d[off + 9], d[off + 10], d[off + 11]]);
                            state.stream_current.stsc.push((first_chunk, spc, desc_idx));
                        }
                    }
                }
            }
            // Track aperture atoms
            b"clef" => {
                parse_aperture_dim(data, content_start, content_end, tags, "CleanApertureDimensions", "Clean Aperture Dimensions");
            }
            b"prof" => {
                parse_aperture_dim(data, content_start, content_end, tags, "ProductionApertureDimensions", "Production Aperture Dimensions");
            }
            b"enof" => {
                parse_aperture_dim(data, content_start, content_end, tags, "EncodedPixelsDimensions", "Encoded Pixels Dimensions");
            }
            // HEIF/HEIC item properties container
            b"iprp" => {
                parse_atoms(data, content_start, content_end, tags, state, depth + 1);
            }
            // Item property container (inside iprp)
            b"ipco" => {
                parse_atoms(data, content_start, content_end, tags, state, depth + 1);
            }
            // HEVC configuration box (only process first one - for primary item)
            b"hvcC" => {
                if !state.hevc_config_done {
                    state.hevc_config_done = true;
                    parse_hvcc(data, content_start, content_end, tags);
                }
            }
            // Image spatial extent (width/height for HEIF) - only process first one
            b"ispe" => {
                if !state.ispe_done {
                    state.ispe_done = true;
                    parse_ispe(data, content_start, content_end, tags);
                }
            }
            // Primary item reference (HEIF)
            b"pitm" => {
                parse_pitm(data, content_start, content_end, tags);
            }
            // mdat: record offset and size
            b"mdat" => {
                tags.push(mk(
                    "MediaDataSize",
                    "Media Data Size",
                    Value::U32((content_end - content_start) as u32),
                ));
                tags.push(mk(
                    "MediaDataOffset",
                    "Media Data Offset",
                    Value::U32(content_start as u32),
                ));
            }
            // XMP metadata (uuid box)
            b"uuid" => {
                if content_end - content_start > 16 {
                    let uuid = &data[content_start..content_start + 16];
                    // Canon UUID: 85C0B687820F11E08111F4CE462B6A48
                    if uuid == b"\x85\xc0\xb6\x87\x82\x0f\x11\xe0\x81\x11\xf4\xce\x46\x2b\x6a\x48" {
                        parse_canon_uuid(data, content_start + 16, content_end, tags);
                    }
                    // Canon DPP4 UUID: EAF42B5E1C984B88B9FBB7DC406E4D16 (contains PRVW)
                    else if uuid == b"\xea\xf4\x2b\x5e\x1c\x98\x4b\x88\xb9\xfb\xb7\xdc\x40\x6e\x4d\x16" {
                        // Find PRVW signature in the uuid content
                        let inner = &data[content_start + 16..content_end];
                        if let Some(prvw_pos) = inner.windows(4).position(|w| w == b"PRVW") {
                            // PRVW: skip 16 bytes (4 tag + 12 header) after "PRVW"
                            let data_start = prvw_pos + 16;
                            if data_start < inner.len() {
                                let prvw_data = &inner[data_start..];
                                let size = prvw_data.len();
                                tags.push(Tag {
                                    id: TagId::Text("PreviewImage".into()),
                                    name: "PreviewImage".into(),
                                    description: "Preview Image".into(),
                                    group: TagGroup { family0: "QuickTime".into(), family1: "QuickTime".into(), family2: "Preview".into() },
                                    raw_value: Value::Binary(prvw_data.to_vec()),
                                    print_value: format!("(Binary data {} bytes, use -b option to extract)", size),
                                    priority: 0,
                                });
                            }
                        }
                    }
                    // XMP UUID: BE7ACFCB97A942E89C71999491E3AFAC
                    else if uuid[0] == 0xBE
                        && uuid[1] == 0x7A
                        && uuid[2] == 0xCF
                        && uuid[3] == 0xCB
                    {
                        let xmp_data = &data[content_start + 16..content_end];
                        if let Ok(xmp_tags) = XmpReader::read(xmp_data) {
                            tags.extend(xmp_tags);
                        }
                    }
                }
            }
            // XMP in udta XMP_ atom
            b"XMP_" => {
                let xmp_data = &data[content_start..content_end];
                if let Ok(xmp_tags) = XmpReader::read(xmp_data) {
                    tags.extend(xmp_tags);
                }
            }
            // QuickTime text atoms (©xxx)
            _ if atom_type[0] == 0xA9 => {
                parse_qt_text_atom(atom_type, data, content_start, content_end, tags);
            }
            // Pentax/Samsung/Sanyo manufacturer tags in udta TAGS atom
            b"TAGS" => {
                let cd = &data[content_start..content_end];
                if cd.starts_with(b"PENTAX DIGITAL CAMERA\0") {
                    parse_pentax_mov(cd, tags);
                }
            }
            _ => {}
        }

        pos = atom_end;
    }
}

/// Parse movie header (mvhd) atom.
/// FORMAT=int32u, field N = byte N*4.
/// Fields: 0=version(int8u), 1=CreateDate, 2=ModifyDate, 3=TimeScale,
///         4=Duration, 5=PreferredRate(fixed32s), 6=PreferredVolume(int16u),
///         9=MatrixStructure(fixed32s[9]), 18=PreviewTime, 19=PreviewDuration,
///         20=PosterTime, 21=SelectionTime, 22=SelectionDuration, 23=CurrentTime,
///         24=NextTrackID
fn parse_mvhd(
    data: &[u8],
    start: usize,
    end: usize,
    tags: &mut Vec<Tag>,
    state: &mut QtState,
) {
    if start + 4 > end {
        return;
    }

    let version = data[start];
    state.movie_header_version = version;
    tags.push(mk(
        "MovieHeaderVersion",
        "Movie Header Version",
        Value::U32(version as u32),
    ));

    // After version+flags (4 bytes), parse fields
    let d = &data[start + 4..end]; // d[0] = field 1 start (byte 0 of data after ver+flags)

    let (creation, modification, timescale, duration, data_after);

    if version == 0 {
        // All fields are int32u (4 bytes each)
        if d.len() < 96 {
            return;
        }
        creation = u32::from_be_bytes([d[0], d[1], d[2], d[3]]) as u64;
        modification = u32::from_be_bytes([d[4], d[5], d[6], d[7]]) as u64;
        timescale = u32::from_be_bytes([d[8], d[9], d[10], d[11]]);
        duration = u32::from_be_bytes([d[12], d[13], d[14], d[15]]) as u64;
        data_after = &d[16..]; // starts at what would be field 5 (byte 20 from version)
    } else if version == 1 {
        // int64u for dates and duration
        if d.len() < 108 {
            return;
        }
        creation =
            u64::from_be_bytes([d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]]);
        modification =
            u64::from_be_bytes([d[8], d[9], d[10], d[11], d[12], d[13], d[14], d[15]]);
        timescale = u32::from_be_bytes([d[16], d[17], d[18], d[19]]);
        duration = u64::from_be_bytes([
            d[20], d[21], d[22], d[23], d[24], d[25], d[26], d[27],
        ]);
        data_after = &d[28..]; // field 5 comes after 28 bytes
    } else {
        return;
    }

    state.movie_timescale = timescale;

    if let Some(dt) = mac_epoch_to_string(creation) {
        tags.push(mk("CreateDate", "Create Date", Value::String(dt)));
    }
    if let Some(dt) = mac_epoch_to_string(modification) {
        tags.push(mk("ModifyDate", "Modify Date", Value::String(dt)));
    }
    tags.push(mk("TimeScale", "Time Scale", Value::U32(timescale)));

    if timescale > 0 {
        let dur_secs = duration as f64 / timescale as f64;
        tags.push(mk(
            "Duration",
            "Duration",
            Value::String(convert_duration(dur_secs)),
        ));
    }

    // data_after[0..] = PreferredRate (field 5, fixed32s = int32u/0x10000)
    if data_after.len() >= 4 {
        let rate_raw = u32::from_be_bytes([
            data_after[0],
            data_after[1],
            data_after[2],
            data_after[3],
        ]);
        let rate = rate_raw as f64 / 0x10000 as f64;
        let rate_str = if rate == rate.floor() {
            format!("{}", rate as i32)
        } else {
            format!("{:.4}", rate).trim_end_matches('0').to_string()
        };
        tags.push(mk("PreferredRate", "Preferred Rate", Value::String(rate_str)));
    }

    // PreferredVolume (field 6): int16u at byte 4 of data_after (6*4=24 - 5*4=20 = 4)
    // Actually: field 6 is at byte offset 6*4=24 from start of version byte.
    // data_after starts at byte 20 from version, so field 6 is at data_after[4..6]
    if data_after.len() >= 6 {
        let vol_raw = u16::from_be_bytes([data_after[4], data_after[5]]);
        let vol_pct = vol_raw as f64 / 256.0 * 100.0;
        tags.push(mk(
            "PreferredVolume",
            "Preferred Volume",
            Value::String(format!("{:.2}%", vol_pct)),
        ));
    }

    // MatrixStructure (field 9): fixed32s[9] at byte 9*4=36 from version byte
    // = byte 36 - 4 (version_flags) = 32 from d[0], and data_after = d[16..]
    // => data_after[16..52] (byte 32 from d start = byte 16 in data_after)
    if data_after.len() >= 52 {
        let matrix_str = parse_matrix_structure(&data_after[16..52]);
        tags.push(mk(
            "MatrixStructure",
            "Matrix Structure",
            Value::String(matrix_str),
        ));
    }

    // Fields 18-24 are int32u at byte N*4 from version byte
    // Field 18 = byte 72 from version = d[68] = data_after[52]
    if data_after.len() >= 80 {
        let preview_time =
            u32::from_be_bytes([
                data_after[52],
                data_after[53],
                data_after[54],
                data_after[55],
            ]) as u64;
        let preview_dur =
            u32::from_be_bytes([
                data_after[56],
                data_after[57],
                data_after[58],
                data_after[59],
            ]) as u64;
        let poster_time =
            u32::from_be_bytes([
                data_after[60],
                data_after[61],
                data_after[62],
                data_after[63],
            ]) as u64;
        let sel_time =
            u32::from_be_bytes([
                data_after[64],
                data_after[65],
                data_after[66],
                data_after[67],
            ]) as u64;
        let sel_dur =
            u32::from_be_bytes([
                data_after[68],
                data_after[69],
                data_after[70],
                data_after[71],
            ]) as u64;
        let cur_time =
            u32::from_be_bytes([
                data_after[72],
                data_after[73],
                data_after[74],
                data_after[75],
            ]) as u64;
        let next_track = u32::from_be_bytes([
            data_after[76],
            data_after[77],
            data_after[78],
            data_after[79],
        ]);

        let ts = timescale;
        tags.push(mk(
            "PreviewTime",
            "Preview Time",
            Value::String(duration_as_time(preview_time, ts)),
        ));
        tags.push(mk(
            "PreviewDuration",
            "Preview Duration",
            Value::String(duration_as_time(preview_dur, ts)),
        ));
        tags.push(mk(
            "PosterTime",
            "Poster Time",
            Value::String(duration_as_time(poster_time, ts)),
        ));
        tags.push(mk(
            "SelectionTime",
            "Selection Time",
            Value::String(duration_as_time(sel_time, ts)),
        ));
        tags.push(mk(
            "SelectionDuration",
            "Selection Duration",
            Value::String(duration_as_time(sel_dur, ts)),
        ));
        tags.push(mk(
            "CurrentTime",
            "Current Time",
            Value::String(duration_as_time(cur_time, ts)),
        ));
        tags.push(mk("NextTrackID", "Next Track ID", Value::U32(next_track)));
    }
}

/// Convert a raw duration value to a time string using the given timescale.
/// Mimics ExifTool's ConvertDuration.
fn duration_as_time(raw: u64, timescale: u32) -> String {
    if timescale == 0 {
        return raw.to_string();
    }
    let secs = raw as f64 / timescale as f64;
    convert_duration(secs)
}

/// Parse matrix structure (9 signed int32 values, fixed-point).
/// Indices 2, 5, 8 are fixed 2.30; others are fixed 16.16.
fn parse_matrix_structure(bytes: &[u8]) -> String {
    if bytes.len() < 36 {
        return String::new();
    }
    let mut parts = Vec::with_capacity(9);
    for i in 0..9 {
        let raw = i32::from_be_bytes([
            bytes[i * 4],
            bytes[i * 4 + 1],
            bytes[i * 4 + 2],
            bytes[i * 4 + 3],
        ]);
        let fval = if i == 2 || i == 5 || i == 8 {
            raw as f64 / (1i64 << 30) as f64 // fixed 2.30
        } else {
            raw as f64 / (1i64 << 16) as f64 // fixed 16.16
        };
        // Format: integer if whole, otherwise decimal
        if fval == fval.floor() && fval.abs() < 1e9 {
            parts.push(format!("{}", fval as i64));
        } else {
            parts.push(format!("{}", fval));
        }
    }
    parts.join(" ")
}

/// Parse track header (tkhd).
/// FORMAT=int32u, field N = byte N*4 (from start of atom content including version byte).
/// Fields: 0=TrackHeaderVersion(int8u), 1=TrackCreateDate, 2=TrackModifyDate,
///         3=TrackID, 5=TrackDuration, 8=TrackLayer(int16u@32), 9=TrackVolume(int16u@36),
///         10=MatrixStructure(fixed32s[9]@40), 19=ImageWidth(fixed32u@76), 20=ImageHeight(fixed32u@80)
fn parse_tkhd(
    data: &[u8],
    start: usize,
    end: usize,
    tags: &mut Vec<Tag>,
    state: &mut QtState,
) {
    if start + 4 > end {
        return;
    }
    let version = data[start];
    state.track_header_version = version;
    tags.push(mk(
        "TrackHeaderVersion",
        "Track Header Version",
        Value::U32(version as u32),
    ));

    let d = &data[start + 4..end]; // d starts at version+flags offset 4

    let (create, modify, track_id, track_dur, data_rest);

    if version == 0 {
        if d.len() < 80 {
            return;
        }
        create = u32::from_be_bytes([d[0], d[1], d[2], d[3]]) as u64;
        modify = u32::from_be_bytes([d[4], d[5], d[6], d[7]]) as u64;
        track_id = u32::from_be_bytes([d[8], d[9], d[10], d[11]]);
        // d[12..16] = reserved
        track_dur = u32::from_be_bytes([d[16], d[17], d[18], d[19]]) as u64;
        data_rest = &d[20..]; // starts at byte 20 (field 5+1, byte 24 from version)
    } else if version == 1 {
        if d.len() < 88 {
            return;
        }
        create =
            u64::from_be_bytes([d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]]);
        modify =
            u64::from_be_bytes([d[8], d[9], d[10], d[11], d[12], d[13], d[14], d[15]]);
        track_id = u32::from_be_bytes([d[16], d[17], d[18], d[19]]);
        // d[20..24] = reserved
        track_dur = u64::from_be_bytes([
            d[24], d[25], d[26], d[27], d[28], d[29], d[30], d[31],
        ]);
        data_rest = &d[32..];
    } else {
        return;
    }

    if let Some(dt) = mac_epoch_to_string(create) {
        tags.push(mk(
            "TrackCreateDate",
            "Track Create Date",
            Value::String(dt),
        ));
    }
    if let Some(dt) = mac_epoch_to_string(modify) {
        tags.push(mk(
            "TrackModifyDate",
            "Track Modify Date",
            Value::String(dt),
        ));
    }
    tags.push(mk("TrackID", "Track ID", Value::U32(track_id)));

    let ts = state.movie_timescale;
    if ts > 0 {
        let dur_secs = track_dur as f64 / ts as f64;
        tags.push(mk(
            "TrackDuration",
            "Track Duration",
            Value::String(convert_duration(dur_secs)),
        ));
    }

    // data_rest[0..]: follows after duration (and reserved 8 bytes after duration in v0)
    // For v0: field layout from d[0]:
    //   [0..4]=create, [4..8]=modify, [8..12]=trackid, [12..16]=reserved
    //   [16..20]=duration, [20..28]=reserved(2xint32), [28..30]=TrackLayer(int16u),
    //   [30..32]=TrackVolume(int16u), [32..68]=Matrix(9xint32), [68..72]=ImageWidth, [72..76]=ImageHeight
    // But our data_rest starts at d[20..] (after create/modify/trackid/reserved/dur)
    // So data_rest[0..8]=reserved, [8..10]=layer, [10..12]=vol, [12..48]=matrix, [48..52]=width, [52..56]=height

    if data_rest.len() >= 10 {
        let layer = u16::from_be_bytes([data_rest[8], data_rest[9]]);
        tags.push(mk("TrackLayer", "Track Layer", Value::U32(layer as u32)));
    }

    if data_rest.len() >= 12 {
        let vol_raw = u16::from_be_bytes([data_rest[10], data_rest[11]]);
        let vol_pct = vol_raw as f64 / 256.0 * 100.0;
        tags.push(mk(
            "TrackVolume",
            "Track Volume",
            Value::String(format!("{:.2}%", vol_pct)),
        ));
    }

    // ImageWidth/Height at data_rest[48..56] (fixed32u = fixed 16.16)
    let mut has_video = false;
    if data_rest.len() >= 56 {
        let w_raw = u32::from_be_bytes([
            data_rest[48],
            data_rest[49],
            data_rest[50],
            data_rest[51],
        ]);
        let h_raw = u32::from_be_bytes([
            data_rest[52],
            data_rest[53],
            data_rest[54],
            data_rest[55],
        ]);
        // FixWrongFormat: if high bits set, the value is actually in wrong format
        let w = fix_wrong_format(w_raw);
        let h = fix_wrong_format(h_raw);
        if w > 0 && h > 0 {
            has_video = true;
            tags.push(mk("ImageWidth", "Image Width", Value::U32(w)));
            tags.push(mk("ImageHeight", "Image Height", Value::U32(h)));
        }
    }

    // Matrix at data_rest[12..48] (9 int32s)
    // Only emit Rotation for video tracks (those with valid image dimensions)
    if data_rest.len() >= 48 && has_video {
        let rotation = calc_rotation_from_matrix(&data_rest[12..48]);
        tags.push(mk(
            "Rotation",
            "Rotation",
            Value::String(format!("{}", rotation)),
        ));
    }
}

/// FixWrongFormat: if val & 0xfff00000 (high bits set), use upper 16 bits.
/// Otherwise treat as fixed 16.16 (divide by 65536).
fn fix_wrong_format(val: u32) -> u32 {
    if val == 0 {
        return 0;
    }
    if val & 0xfff00000 != 0 {
        // It's stored as fixed 16.16 already but with wrong format flag
        // Use the upper 16 bits (integer part of fixed 16.16)
        (val >> 16) & 0xFFFF
    } else {
        val
    }
}

/// Calculate rotation angle (degrees) from a 3x3 matrix in fixed-point bytes.
fn calc_rotation_from_matrix(bytes: &[u8]) -> i32 {
    if bytes.len() < 36 {
        return 0;
    }
    // Elements [0][0], [0][1], [1][0], [1][1] determine rotation
    // In fixed 16.16 format:
    let a = i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]); // [0][0]
    let b = i32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]); // [0][1]
    let _c = i32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]); // [0][2] (2.30)
    let d = i32::from_be_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]); // [1][0]
    let e = i32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]); // [1][1]

    // Convert to floats (fixed 16.16)
    let af = a as f64 / 65536.0;
    let bf = b as f64 / 65536.0;
    let df = d as f64 / 65536.0;
    let ef = e as f64 / 65536.0;

    // Determine rotation angle
    // Typical rotation matrices:
    // 0°:   [[1,0],[0,1]]
    // 90°:  [[0,1],[-1,0]]
    // 180°: [[-1,0],[0,-1]]
    // 270°: [[0,-1],[1,0]]
    let angle_rad = af.atan2(bf);
    let angle_deg = (angle_rad * 180.0 / std::f64::consts::PI).round() as i32;

    // Normalize
    if (af - 1.0).abs() < 0.01 && ef.abs() < 0.01 && df.abs() < 0.01 {
        return 0;
    }
    if af.abs() < 0.01 && (bf - 1.0).abs() < 0.01 && (df + 1.0).abs() < 0.01 && ef.abs() < 0.01 {
        return 90;
    }
    if (af + 1.0).abs() < 0.01 && ef.abs() < 0.01 && df.abs() < 0.01 {
        return 180;
    }
    if af.abs() < 0.01 && (bf + 1.0).abs() < 0.01 && (df - 1.0).abs() < 0.01 && ef.abs() < 0.01 {
        return 270;
    }

    // Generic: compute from atan2
    let angle = ((angle_deg % 360) + 360) % 360;
    angle
}

/// Parse media header (mdhd).
fn parse_mdhd(
    data: &[u8],
    start: usize,
    end: usize,
    tags: &mut Vec<Tag>,
    state: &mut QtState,
) {
    if start + 4 > end {
        return;
    }
    let version = data[start];
    state.media_header_version = version;
    tags.push(mk(
        "MediaHeaderVersion",
        "Media Header Version",
        Value::U32(version as u32),
    ));

    let d = &data[start + 4..end];

    let (create, modify, timescale, duration, lang_offset);

    if version == 0 {
        if d.len() < 20 {
            return;
        }
        create = u32::from_be_bytes([d[0], d[1], d[2], d[3]]) as u64;
        modify = u32::from_be_bytes([d[4], d[5], d[6], d[7]]) as u64;
        timescale = u32::from_be_bytes([d[8], d[9], d[10], d[11]]);
        duration = u32::from_be_bytes([d[12], d[13], d[14], d[15]]) as u64;
        lang_offset = 16;
    } else if version == 1 {
        if d.len() < 32 {
            return;
        }
        create =
            u64::from_be_bytes([d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]]);
        modify =
            u64::from_be_bytes([d[8], d[9], d[10], d[11], d[12], d[13], d[14], d[15]]);
        timescale = u32::from_be_bytes([d[16], d[17], d[18], d[19]]);
        duration = u64::from_be_bytes([
            d[20], d[21], d[22], d[23], d[24], d[25], d[26], d[27],
        ]);
        lang_offset = 28;
    } else {
        return;
    }

    state.media_timescale = timescale;
    if state.extract_embedded > 0 {
        state.stream_current.media_timescale = timescale;
    }

    if let Some(dt) = mac_epoch_to_string(create) {
        tags.push(mk(
            "MediaCreateDate",
            "Media Create Date",
            Value::String(dt),
        ));
    }
    if let Some(dt) = mac_epoch_to_string(modify) {
        tags.push(mk(
            "MediaModifyDate",
            "Media Modify Date",
            Value::String(dt),
        ));
    }
    tags.push(mk(
        "MediaTimeScale",
        "Media Time Scale",
        Value::U32(timescale),
    ));

    if timescale > 0 {
        let dur_secs = duration as f64 / timescale as f64;
        tags.push(mk(
            "MediaDuration",
            "Media Duration",
            Value::String(convert_duration(dur_secs)),
        ));
    }

    // Language code (ISO 639-2 packed)
    if d.len() >= lang_offset + 2 {
        let lang_code = u16::from_be_bytes([d[lang_offset], d[lang_offset + 1]]);
        if lang_code != 0 && lang_code != 0x7FFF {
            if lang_code >= 0x400 {
                // ISO 639-2 packed format: 3 x 5-bit codes offset by 0x60
                let c1 = ((lang_code >> 10) & 0x1F) as u8 + 0x60;
                let c2 = ((lang_code >> 5) & 0x1F) as u8 + 0x60;
                let c3 = (lang_code & 0x1F) as u8 + 0x60;
                if c1.is_ascii_lowercase()
                    && c2.is_ascii_lowercase()
                    && c3.is_ascii_lowercase()
                {
                    let lang = format!("{}{}{}", c1 as char, c2 as char, c3 as char);
                    tags.push(mk(
                        "MediaLanguageCode",
                        "Media Language Code",
                        Value::String(lang),
                    ));
                }
            } else {
                // Macintosh language code - just emit numeric
                tags.push(mk(
                    "MediaLanguageCode",
                    "Media Language Code",
                    Value::U32(lang_code as u32),
                ));
            }
        }
    }
}

/// Parse handler reference (hdlr).
/// Byte layout: version+flags(4), HandlerClass(4), HandlerType(4), HandlerVendorID(4),
///              reserved(12), HandlerDescription(string)
fn parse_hdlr(
    data: &[u8],
    start: usize,
    end: usize,
    tags: &mut Vec<Tag>,
    state: &mut QtState,
) {
    if start + 12 > end {
        return;
    }
    let d = &data[start..end]; // includes version+flags

    // HandlerClass at byte 4 (relative to start)
    if d.len() >= 8 {
        let hclass = &d[4..8];
        if hclass != b"\0\0\0\0" {
            let class_str = String::from_utf8_lossy(hclass).to_string();
            let class_name = match hclass {
                b"mhlr" => "Media Handler",
                b"dhlr" => "Data Handler",
                _ => &class_str,
            };
            tags.push(mk(
                "HandlerClass",
                "Handler Class",
                Value::String(class_name.to_string()),
            ));
        }
    }

    // HandlerType at byte 8
    if d.len() >= 12 {
        let htype_bytes = &d[8..12];
        let htype_raw = String::from_utf8_lossy(htype_bytes).trim().to_string();
        // Skip 'alis' and 'url ' types (they don't set the main handler type)
        if htype_bytes != b"alis" && htype_bytes != b"url " {
            state.handler_type = [htype_bytes[0], htype_bytes[1], htype_bytes[2], htype_bytes[3]];
            // Copy to stream current track
            if state.extract_embedded > 0 {
                state.stream_current.handler_type = state.handler_type;
            }
        }
        let handler_name = match htype_bytes {
            b"alis" => "Alias Data",
            b"crsm" => "Clock Reference",
            b"hint" => "Hint Track",
            b"ipsm" => "IPMP",
            b"m7sm" => "MPEG-7 Stream",
            b"meta" => "NRT Metadata",
            b"mdir" => "Metadata",
            b"mdta" => "Metadata Tags",
            b"mjsm" => "MPEG-J",
            b"ocsm" => "Object Content",
            b"odsm" => "Object Descriptor",
            b"priv" => "Private",
            b"sdsm" => "Scene Description",
            b"soun" => "Audio Track",
            b"text" => "Text",
            b"tmcd" => "Time Code",
            b"url " => "URL",
            b"vide" => "Video Track",
            b"subp" => "Subpicture",
            b"nrtm" => "Non-Real Time Metadata",
            b"pict" => "Picture",
            b"camm" => "Camera Metadata",
            b"psmd" => "Panasonic Static Metadata",
            b"data" => "Data",
            b"sbtl" => "Subtitle",
            _ => &htype_raw,
        };
        tags.push(mk(
            "HandlerType",
            "Handler Type",
            Value::String(handler_name.to_string()),
        ));
    }

    // HandlerVendorID at byte 12
    if d.len() >= 16 {
        let vendor = &d[12..16];
        if vendor != b"\0\0\0\0" {
            let vendor_str = String::from_utf8_lossy(vendor).to_string();
            let vendor_name = vendor_id_name(vendor);
            tags.push(mk(
                "HandlerVendorID",
                "Handler Vendor ID",
                Value::String(vendor_name.map(|s| s.to_string()).unwrap_or(vendor_str)),
            ));
        }
    }

    // HandlerDescription at byte 24 (string, possibly Pascal-style)
    if d.len() > 24 {
        let desc_bytes = &d[24..];
        let desc = decode_pascal_or_c_string(desc_bytes);
        if !desc.is_empty() {
            tags.push(mk(
                "HandlerDescription",
                "Handler Description",
                Value::String(desc),
            ));
        }
    }
}

/// Decode a string that might be a Pascal string (first byte = length) or C string (null-terminated).
fn decode_pascal_or_c_string(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let first = bytes[0];
    // If first byte is a control char (0x00-0x1F) and < len, it's Pascal
    if first < 0x20 && (first as usize) < bytes.len() {
        let s = &bytes[1..1 + first as usize];
        return String::from_utf8_lossy(s).trim_end_matches('\0').to_string();
    }
    // Otherwise C string
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

/// Look up a vendor ID.
fn vendor_id_name(vendor: &[u8]) -> Option<&'static str> {
    match vendor {
        b"appl" => Some("Apple"),
        b"fe20" => Some("Olympus (fe20)"),
        b"FFMP" => Some("FFmpeg"),
        b"GIC " => Some("General Imaging Co."),
        b"kdak" => Some("Kodak"),
        b"KMPI" => Some("Konica-Minolta"),
        b"leic" => Some("Leica"),
        b"mino" => Some("Minolta"),
        b"niko" => Some("Nikon"),
        b"NIKO" => Some("Nikon"),
        b"olym" => Some("Olympus"),
        b"pana" => Some("Panasonic"),
        b"pent" => Some("Pentax"),
        b"pr01" => Some("Olympus (pr01)"),
        b"sany" => Some("Sanyo"),
        b"SMI " => Some("Sorenson Media Inc."),
        b"ZORA" => Some("Zoran Corporation"),
        b"AR.D" => Some("Parrot AR.Drone"),
        b" KD " => Some("Kodak"),
        _ => None,
    }
}

/// Parse video media header (vmhd).
/// FORMAT=int16u, field N = byte N*2.
/// version+flags(4), field 2=GraphicsMode(int16u@4), field 3=OpColor(int16u[3]@6)
fn parse_vmhd(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    let d = &data[start..end];
    // version+flags at bytes 0..4
    if d.len() >= 6 {
        let gmode = u16::from_be_bytes([d[4], d[5]]);
        let gmode_name = graphics_mode_name(gmode);
        tags.push(mk(
            "GraphicsMode",
            "Graphics Mode",
            Value::String(gmode_name.to_string()),
        ));
    }
    if d.len() >= 12 {
        let r = u16::from_be_bytes([d[6], d[7]]);
        let g = u16::from_be_bytes([d[8], d[9]]);
        let b = u16::from_be_bytes([d[10], d[11]]);
        tags.push(mk(
            "OpColor",
            "Op Color",
            Value::String(format!("{} {} {}", r, g, b)),
        ));
    }
}

fn graphics_mode_name(mode: u16) -> &'static str {
    match mode {
        0x00 => "srcCopy",
        0x01 => "srcOr",
        0x02 => "srcXor",
        0x03 => "srcBic",
        0x04 => "notSrcCopy",
        0x05 => "notSrcOr",
        0x06 => "notSrcXor",
        0x07 => "notSrcBic",
        0x08 => "patCopy",
        0x09 => "patOr",
        0x0a => "patXor",
        0x0b => "patBic",
        0x0c => "notPatCopy",
        0x0d => "notPatOr",
        0x0e => "notPatXor",
        0x0f => "notPatBic",
        0x20 => "blend",
        0x21 => "addPin",
        0x22 => "addOver",
        0x23 => "subPin",
        0x24 => "transparent",
        0x25 => "addMax",
        0x26 => "subOver",
        0x27 => "addMin",
        0x31 => "grayishTextOr",
        0x32 => "hilite",
        0x40 => "ditherCopy",
        0x100 => "Alpha",
        0x101 => "White Alpha",
        0x102 => "Pre-multiplied Black Alpha",
        0x110 => "Component Alpha",
        _ => "Unknown",
    }
}

/// Parse audio media header (smhd).
/// FORMAT=int16u, field 2=Balance(fixed16s@4)
fn parse_smhd(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    let d = &data[start..end];
    if d.len() >= 6 {
        let balance_raw = i16::from_be_bytes([d[4], d[5]]);
        let balance = balance_raw as f64 / 256.0;
        let balance_str = if balance == balance.floor() {
            format!("{}", balance as i32)
        } else {
            format!("{:.4}", balance)
                .trim_end_matches('0')
                .to_string()
        };
        tags.push(mk("Balance", "Balance", Value::String(balance_str)));
    }
}

/// Parse HEVC configuration box (hvcC).
fn parse_hvcc(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    let d = &data[start..end];
    if d.len() < 22 {
        return;
    }

    // Byte 0: HEVCConfigurationVersion
    tags.push(mk(
        "HEVCConfigurationVersion",
        "HEVC Configuration Version",
        Value::U32(d[0] as u32),
    ));

    // Byte 1: GeneralProfileSpace (bits 7-6), GeneralTierFlag (bit 5), GeneralProfileIDC (bits 4-0)
    let profile_space = (d[1] >> 6) & 0x3;
    let tier_flag = (d[1] >> 5) & 0x1;
    let profile_idc = d[1] & 0x1f;

    let profile_space_str = match profile_space {
        0 => "Conforming",
        1 => "Reserved 1",
        2 => "Reserved 2",
        3 => "Reserved 3",
        _ => "Unknown",
    };
    tags.push(mk(
        "GeneralProfileSpace",
        "General Profile Space",
        Value::String(profile_space_str.to_string()),
    ));

    let tier_str = if tier_flag == 0 { "Main Tier" } else { "High Tier" };
    tags.push(mk(
        "GeneralTierFlag",
        "General Tier Flag",
        Value::String(tier_str.to_string()),
    ));

    let profile_name = match profile_idc {
        0 => "No Profile",
        1 => "Main",
        2 => "Main 10",
        3 => "Main Still Picture",
        4 => "Format Range Extensions",
        5 => "High Throughput",
        6 => "Multiview Main",
        7 => "Scalable Main",
        8 => "3D Main",
        9 => "Screen Content Coding Extensions",
        10 => "Scalable Format Range Extensions",
        11 => "High Throughput Screen Content Coding Extensions",
        _ => "Unknown",
    };
    tags.push(mk(
        "GeneralProfileIDC",
        "General Profile IDC",
        Value::String(profile_name.to_string()),
    ));

    // Bytes 2-5: GenProfileCompatibilityFlags (int32u, BITMASK)
    if d.len() >= 6 {
        let flags = u32::from_be_bytes([d[2], d[3], d[4], d[5]]);
        let compat_str = hevc_compat_flags_to_string(flags);
        tags.push(mk(
            "GenProfileCompatibilityFlags",
            "Gen Profile Compatibility Flags",
            Value::String(compat_str),
        ));
    }

    // Bytes 6-11: ConstraintIndicatorFlags (6 bytes as space-separated decimals)
    if d.len() >= 12 {
        let constraint = format!(
            "{} {} {} {} {} {}",
            d[6], d[7], d[8], d[9], d[10], d[11]
        );
        tags.push(mk(
            "ConstraintIndicatorFlags",
            "Constraint Indicator Flags",
            Value::String(constraint),
        ));
    }

    // Byte 12: GeneralLevelIDC
    if d.len() >= 13 {
        let level = d[12];
        let level_str = format!("{} (level {:.1})", level, level as f64 / 30.0);
        tags.push(mk(
            "GeneralLevelIDC",
            "General Level IDC",
            Value::String(level_str),
        ));
    }

    // Bytes 13-14: MinSpatialSegmentationIDC (int16u, mask 0x0FFF)
    if d.len() >= 15 {
        let min_seg = u16::from_be_bytes([d[13], d[14]]) & 0x0FFF;
        tags.push(mk(
            "MinSpatialSegmentationIDC",
            "Min Spatial Segmentation IDC",
            Value::U32(min_seg as u32),
        ));
    }

    // Byte 15: ParallelismType (bits 1-0)
    if d.len() >= 16 {
        let parallelism = d[15] & 0x3;
        tags.push(mk(
            "ParallelismType",
            "Parallelism Type",
            Value::U32(parallelism as u32),
        ));
    }

    // Byte 16: ChromaFormat (bits 1-0)
    if d.len() >= 17 {
        let chroma = d[16] & 0x3;
        let chroma_str = match chroma {
            0 => "Monochrome",
            1 => "4:2:0",
            2 => "4:2:2",
            3 => "4:4:4",
            _ => "Unknown",
        };
        tags.push(mk(
            "ChromaFormat",
            "Chroma Format",
            Value::String(chroma_str.to_string()),
        ));
    }

    // Byte 17: BitDepthLuma (bits 2-0, add 8)
    if d.len() >= 18 {
        let luma = (d[17] & 0x7) + 8;
        tags.push(mk("BitDepthLuma", "Bit Depth Luma", Value::U32(luma as u32)));
    }

    // Byte 18: BitDepthChroma (bits 2-0, add 8)
    if d.len() >= 19 {
        let chroma = (d[18] & 0x7) + 8;
        tags.push(mk(
            "BitDepthChroma",
            "Bit Depth Chroma",
            Value::U32(chroma as u32),
        ));
    }

    // Bytes 19-20: AverageFrameRate (int16u, /256)
    if d.len() >= 21 {
        let avg_fr = u16::from_be_bytes([d[19], d[20]]);
        let avg_fr_val = avg_fr as f64 / 256.0;
        let avg_str = if avg_fr_val == avg_fr_val.floor() {
            format!("{}", avg_fr_val as u32)
        } else {
            format!("{:.4}", avg_fr_val)
                .trim_end_matches('0')
                .to_string()
        };
        tags.push(mk(
            "AverageFrameRate",
            "Average Frame Rate",
            Value::String(avg_str),
        ));
    }

    // Byte 21: ConstantFrameRate (bits 7-6), NumTemporalLayers (bits 5-3), TemporalIDNested (bit 2)
    if d.len() >= 22 {
        let b21 = d[21];
        let const_fr = (b21 >> 6) & 0x3;
        let const_str = match const_fr {
            0 => "Unknown",
            1 => "Constant Frame Rate",
            2 => "Each Temporal Layer is Constant Frame Rate",
            _ => "Unknown",
        };
        tags.push(mk(
            "ConstantFrameRate",
            "Constant Frame Rate",
            Value::String(const_str.to_string()),
        ));

        let num_layers = (b21 >> 3) & 0x7;
        tags.push(mk(
            "NumTemporalLayers",
            "Num Temporal Layers",
            Value::U32(num_layers as u32),
        ));

        let nested = (b21 >> 2) & 0x1;
        let nested_str = if nested == 0 { "No" } else { "Yes" };
        tags.push(mk(
            "TemporalIDNested",
            "Temporal ID Nested",
            Value::String(nested_str.to_string()),
        ));
    }
}

/// Convert HEVC GenProfileCompatibilityFlags bitmask to descriptive string.
fn hevc_compat_flags_to_string(flags: u32) -> String {
    // ExifTool BITMASK iterates in ascending key order (bit 20 first, bit 31 last).
    // Bit N = 1u32 << N.
    let bit_names: [(u32, &str); 12] = [
        (20, "High Throughput Screen Content Coding Extensions"),
        (21, "Scalable Format Range Extensions"),
        (22, "Screen Content Coding Extensions"),
        (23, "3D Main"),
        (24, "Scalable Main"),
        (25, "Multiview Main"),
        (26, "High Throughput"),
        (27, "Format Range Extensions"),
        (28, "Main Still Picture"),
        (29, "Main 10"),
        (30, "Main"),
        (31, "No Profile"),
    ];
    let mut parts = Vec::new();
    for (bit, name) in &bit_names {
        if flags & (1u32 << bit) != 0 {
            parts.push(*name);
        }
    }
    if parts.is_empty() {
        "(none)".to_string()
    } else {
        parts.join(", ")
    }
}

/// Parse image spatial extent (ispe) for HEIF/HEIC.
/// version+flags(4) + width(4) + height(4)
fn parse_ispe(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    let d = &data[start..end];
    if d.len() < 12 {
        return;
    }
    // Check version/flags == 0
    let ver_flags = u32::from_be_bytes([d[0], d[1], d[2], d[3]]);
    if ver_flags != 0 {
        return;
    }
    let width = u32::from_be_bytes([d[4], d[5], d[6], d[7]]);
    let height = u32::from_be_bytes([d[8], d[9], d[10], d[11]]);
    if width > 0 && height > 0 {
        let extent_str = format!("{}x{}", width, height);
        tags.push(mk(
            "ImageSpatialExtent",
            "Image Spatial Extent",
            Value::String(extent_str),
        ));
        // Also emit ImageWidth/Height (only for the primary item, no DOC_NUM)
        tags.push(mk("ImageWidth", "Image Width", Value::U32(width)));
        tags.push(mk("ImageHeight", "Image Height", Value::U32(height)));
    }
}

/// Parse primary item reference (pitm) for HEIF/HEIC.
fn parse_pitm(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    let d = &data[start..end];
    if d.len() < 6 {
        return;
    }
    // version(1) + flags(3) + item_id(2 or 4 depending on version)
    let version = d[0];
    let item_id = if version == 0 && d.len() >= 6 {
        u16::from_be_bytes([d[4], d[5]]) as u32
    } else if version == 1 && d.len() >= 8 {
        u32::from_be_bytes([d[4], d[5], d[6], d[7]])
    } else {
        return;
    };
    tags.push(mk(
        "PrimaryItemReference",
        "Primary Item Reference",
        Value::U32(item_id),
    ));
}

/// Parse sample description (stsd) for codec info.
/// The stsd contains: version+flags(4), entry_count(4), then entries.
/// Each entry: size(4), format(4), reserved(6), data_ref_index(2), then format-specific data.
fn parse_stsd(
    data: &[u8],
    start: usize,
    end: usize,
    tags: &mut Vec<Tag>,
    state: &mut QtState,
) {
    let d = &data[start..end];
    if d.len() < 16 {
        return;
    }
    let entry_count = u32::from_be_bytes([d[4], d[5], d[6], d[7]]);
    if entry_count == 0 {
        return;
    }

    // First sample entry at offset 8
    let entry = &d[8..];
    if entry.len() < 16 {
        return;
    }
    let entry_size = u32::from_be_bytes([entry[0], entry[1], entry[2], entry[3]]) as usize;
    let format = &entry[4..8];
    let format_str = String::from_utf8_lossy(format).trim().to_string();

    // Check for CTMD (Canon Timed MetaData) format
    if format == b"CTMD" {
        state.current_track_is_ctmd = true;
        tags.push(mk("MetaFormat", "Meta Format", Value::String("CTMD".into())));
    }
    // Check for JPEG or CRAW format (Canon CR3 JpgFromRaw track)
    // First CRAW track in CR3 contains JpgFromRaw data
    if format == b"JPEG" || (format == b"CRAW" && state.jpeg_offset.is_none()) {
        state.current_track_is_jpeg = true;
    }

    // Record meta format for stream extraction
    if state.extract_embedded > 0 && !format_str.is_empty() {
        state.stream_current.meta_format = Some(format_str.clone());
    }

    // Determine if audio or video based on handler type
    let handler = &state.handler_type;

    if handler == b"soun" {
        // AudioSampleDesc: FORMAT=undef, offsets are byte-based
        // Field 4: AudioFormat (undef[4]) at byte 4 of entry
        // Field 20: AudioVendorID (undef[4]) at byte 20
        // Field 24: AudioChannels (int16u) at byte 24
        // Field 26: AudioBitsPerSample (int16u) at byte 26
        // Field 32: AudioSampleRate (fixed32u) at byte 32
        let fmt = String::from_utf8_lossy(format).to_string();
        if fmt.chars().all(|c| c.is_ascii_graphic() || c == ' ') && !fmt.trim().is_empty() {
            tags.push(mk(
                "AudioFormat",
                "Audio Format",
                Value::String(fmt.trim().to_string()),
            ));
        }

        if entry.len() >= 24 {
            let channels = u16::from_be_bytes([entry[24], entry[25]]);
            tags.push(mk(
                "AudioChannels",
                "Audio Channels",
                Value::U32(channels as u32),
            ));
        }

        if entry.len() >= 28 {
            let bits = u16::from_be_bytes([entry[26], entry[27]]);
            tags.push(mk(
                "AudioBitsPerSample",
                "Audio Bits Per Sample",
                Value::U32(bits as u32),
            ));
        }

        if entry.len() >= 36 {
            let sr_raw = u32::from_be_bytes([entry[32], entry[33], entry[34], entry[35]]);
            let sr = sr_raw as f64 / 65536.0;
            let sr_str = if sr == sr.floor() {
                format!("{}", sr as u32)
            } else {
                format!("{:.4}", sr).trim_end_matches('0').to_string()
            };
            tags.push(mk(
                "AudioSampleRate",
                "Audio Sample Rate",
                Value::String(sr_str),
            ));
        }
    } else if handler == b"vide" {
        // VisualSampleDesc: FORMAT=int16u, field N = byte N*2
        // Field 2: CompressorID (string[4]) at byte 4
        // Field 10: VendorID (string[4]) at byte 20
        // Field 16: SourceImageWidth (int16u) at byte 32
        // Field 17: SourceImageHeight (int16u) at byte 34
        // Field 18: XResolution (fixed32u) at byte 36
        // Field 20: YResolution (fixed32u) at byte 40
        // Field 25: CompressorName (string[32]) at byte 50
        // Field 41: BitDepth (int16u) at byte 82

        // CompressorID is the format code (jpeg, avc1, etc.)
        if !format_str.trim().is_empty() {
            tags.push(mk(
                "CompressorID",
                "Compressor ID",
                Value::String(format_str.trim().to_string()),
            ));
        }

        // VendorID at byte 20
        if entry.len() >= 24 {
            let vendor = &entry[20..24];
            if vendor != b"\0\0\0\0" {
                let vendor_str = String::from_utf8_lossy(vendor).to_string();
                let vname = vendor_id_name(vendor)
                    .map(|s| s.to_string())
                    .unwrap_or(vendor_str);
                if !vname.trim().is_empty() {
                    tags.push(mk("VendorID", "Vendor ID", Value::String(vname)));
                }
            }
        }

        // SourceImageWidth at byte 32
        if entry.len() >= 34 {
            let w = u16::from_be_bytes([entry[32], entry[33]]);
            tags.push(mk(
                "SourceImageWidth",
                "Source Image Width",
                Value::U32(w as u32),
            ));
        }

        // SourceImageHeight at byte 34
        if entry.len() >= 36 {
            let h = u16::from_be_bytes([entry[34], entry[35]]);
            tags.push(mk(
                "SourceImageHeight",
                "Source Image Height",
                Value::U32(h as u32),
            ));
        }

        // XResolution at byte 36 (fixed32u = 16.16)
        if entry.len() >= 40 {
            let xres_raw = u32::from_be_bytes([entry[36], entry[37], entry[38], entry[39]]);
            let xres = xres_raw as f64 / 65536.0;
            let xres_str = if xres == xres.floor() {
                format!("{}", xres as u32)
            } else {
                format!("{:.4}", xres).trim_end_matches('0').to_string()
            };
            tags.push(mk("XResolution", "X Resolution", Value::String(xres_str)));
        }

        // YResolution at byte 40 (fixed32u = 16.16)
        if entry.len() >= 44 {
            let yres_raw = u32::from_be_bytes([entry[40], entry[41], entry[42], entry[43]]);
            let yres = yres_raw as f64 / 65536.0;
            let yres_str = if yres == yres.floor() {
                format!("{}", yres as u32)
            } else {
                format!("{:.4}", yres).trim_end_matches('0').to_string()
            };
            tags.push(mk("YResolution", "Y Resolution", Value::String(yres_str)));
        }

        // CompressorName at byte 50 (32 bytes, Pascal string)
        if entry.len() >= 82 {
            let comp_bytes = &entry[50..82];
            let comp_name = decode_pascal_or_c_string(comp_bytes);
            if !comp_name.is_empty() {
                tags.push(mk(
                    "CompressorName",
                    "Compressor Name",
                    Value::String(comp_name),
                ));
            }
        }

        // BitDepth at byte 82
        if entry.len() >= 84 {
            let bitdepth = u16::from_be_bytes([entry[82], entry[83]]);
            tags.push(mk("BitDepth", "Bit Depth", Value::U32(bitdepth as u32)));
        }
    }

    let _ = entry_size;
}

/// Parse time-to-sample table (stts) to compute VideoFrameRate.
/// Only for video tracks (handler_type == 'vide').
fn parse_stts(
    data: &[u8],
    start: usize,
    end: usize,
    tags: &mut Vec<Tag>,
    state: &mut QtState,
) {
    let d = &data[start..end];
    if d.len() < 8 {
        return;
    }
    let entry_count = u32::from_be_bytes([d[4], d[5], d[6], d[7]]) as usize;
    if entry_count == 0 || d.len() < 8 + entry_count * 8 {
        return;
    }

    // Collect stts entries for stream extraction
    if state.extract_embedded > 0 {
        for i in 0..entry_count {
            let off = 8 + i * 8;
            if off + 8 > d.len() {
                break;
            }
            let count = u32::from_be_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]]);
            let delta = u32::from_be_bytes([d[off + 4], d[off + 5], d[off + 6], d[off + 7]]);
            state.stream_current.stts.push((count, delta));
        }
    }

    // For metadata tracks (handler "meta"), emit SampleTime and SampleDuration
    if &state.handler_type == b"meta" && state.current_track_is_ctmd {
        if entry_count > 0 {
            let off = 8;
            let _count = u32::from_be_bytes([d[off], d[off+1], d[off+2], d[off+3]]);
            let delta = u32::from_be_bytes([d[off+4], d[off+5], d[off+6], d[off+7]]);
            // SampleTime=0, SampleDuration=count/delta (simplified)
            let sample_time_s = 0u32;
            let sample_dur_s = delta as f64; // In Perl, uses movie timescale; simplified here
            tags.push(mk("SampleTime", "Sample Time", Value::String(format!("{} s", sample_time_s as u32))));
            tags.push(mk("SampleDuration", "Sample Duration", Value::String(format!("{:.2} s", sample_dur_s))));
        }
        return;
    }

    if &state.handler_type != b"vide" {
        return;
    }

    let mut total_samples: u64 = 0;
    let mut total_duration: u64 = 0;

    for i in 0..entry_count {
        let off = 8 + i * 8;
        if off + 8 > d.len() {
            break;
        }
        let count = u32::from_be_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]]) as u64;
        let delta = u32::from_be_bytes([
            d[off + 4],
            d[off + 5],
            d[off + 6],
            d[off + 7],
        ]) as u64;
        total_samples += count;
        total_duration += count * delta;
    }

    let ts = state.media_timescale as u64;
    if total_samples > 0 && total_duration > 0 && ts > 0 {
        let rate = total_samples as f64 * ts as f64 / total_duration as f64;
        // Round to 3 decimal places
        let rate_rounded = (rate * 1000.0 + 0.5).floor() / 1000.0;
        let rate_str = if rate_rounded == rate_rounded.floor() {
            format!("{}", rate_rounded as u32)
        } else {
            format!("{:.3}", rate_rounded)
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string()
        };
        tags.push(mk(
            "VideoFrameRate",
            "Video Frame Rate",
            Value::String(rate_str),
        ));
    }
}

/// Parse track aperture dimension atoms (clef, prof, enof).
/// Format: version+flags(4), width_fixed32u(4), height_fixed32u(4)
fn parse_aperture_dim(
    data: &[u8],
    start: usize,
    end: usize,
    tags: &mut Vec<Tag>,
    name: &str,
    desc: &str,
) {
    let d = &data[start..end];
    if d.len() < 12 {
        return;
    }
    // Skip version+flags (4 bytes), then width and height as fixed32u
    let w_raw = u32::from_be_bytes([d[4], d[5], d[6], d[7]]);
    let h_raw = u32::from_be_bytes([d[8], d[9], d[10], d[11]]);
    let w = w_raw as f64 / 65536.0;
    let h = h_raw as f64 / 65536.0;
    if w > 0.0 && h > 0.0 {
        let w_int = w as u32;
        let h_int = h as u32;
        tags.push(mk(name, desc, Value::String(format!("{}x{}", w_int, h_int))));
    }
}

/// Parse iTunes 'mean/name/data' triplet (the '----' atom in ilst).
/// Produces tags like VolumeNormalization from iTunNORM.
fn parse_ilst_triplet(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    let mut pos = start;
    let mut mean_val = String::new();
    let mut name_val = String::new();
    let mut data_val = String::new();

    while pos + 8 <= end {
        let size = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        if size < 8 || pos + size > end {
            break;
        }
        let atype = &data[pos + 4..pos + 8];
        let content = &data[pos + 8..pos + size];

        match atype {
            b"mean" => {
                // version+flags(4) + string
                if content.len() > 4 {
                    mean_val = String::from_utf8_lossy(&content[4..])
                        .trim_end_matches('\0')
                        .to_string();
                }
            }
            b"name" => {
                // version+flags(4) + string
                if content.len() > 4 {
                    name_val = String::from_utf8_lossy(&content[4..])
                        .trim_end_matches('\0')
                        .to_string();
                }
            }
            b"data" => {
                // data_type(4) + locale(4) + value
                if content.len() > 8 {
                    data_val = String::from_utf8_lossy(&content[8..])
                        .trim_end_matches('\0')
                        .to_string();
                }
            }
            _ => {}
        }

        pos += size;
    }

    if name_val.is_empty() {
        return;
    }

    // Build tag ID: strip 'com.apple.iTunes/' prefix from mean
    let tag_id = if mean_val == "com.apple.iTunes" {
        name_val.clone()
    } else if !mean_val.is_empty() {
        format!("{}/{}", mean_val, name_val)
    } else {
        name_val.clone()
    };

    // Map known tag IDs to tag names and apply PrintConv
    let (tag_name, tag_desc, display_value) = match tag_id.as_str() {
        "iTunNORM" => {
            // VolumeNormalization: remove leading zeros from hex words
            let cleaned = itun_norm_print_conv(&data_val);
            ("VolumeNormalization", "Volume Normalization", cleaned)
        }
        "iTunSMPB" => {
            let cleaned = itun_norm_print_conv(&data_val);
            ("iTunSMPB", "iTunSMPB", cleaned)
        }
        "iTunEXTC" => ("ContentRating", "Content Rating", data_val.clone()),
        _ => return, // Unknown triplet tag, skip
    };

    if !display_value.is_empty() {
        tags.push(mk(tag_name, tag_desc, Value::String(display_value)));
    }
}

/// PrintConv for iTunNORM / iTunSMPB: remove leading zeros from hex words.
fn itun_norm_print_conv(val: &str) -> String {
    // Replace " 0+X" with " X" (remove leading zeros in each hex word)
    let mut result = String::new();
    for word in val.split_whitespace() {
        if !result.is_empty() {
            result.push(' ');
        }
        // Trim leading zeros but keep at least one char
        let trimmed = word.trim_start_matches('0');
        result.push_str(if trimmed.is_empty() { "0" } else { trimmed });
    }
    result
}

/// Apply PrintConv to ilst tag values for specific tags.
fn apply_ilst_print_conv(item_type: &[u8], value: &str) -> String {
    match item_type {
        b"pgap" => {
            // PlayGap: 0='Insert Gap', 1='No Gap'
            match value {
                "0" => "Insert Gap".to_string(),
                "1" => "No Gap".to_string(),
                _ => value.to_string(),
            }
        }
        _ => value.to_string(),
    }
}

/// Parse iTunes metadata item list (ilst).
fn parse_ilst(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) {
    let mut pos = start;

    while pos + 8 <= end {
        let item_size =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                as usize;
        let item_type = &data[pos + 4..pos + 8];
        let item_end = pos + item_size;

        if item_size < 8 || item_end > end {
            break;
        }

        if item_type == b"----" {
            // mean/name/data triplet (iTunes reverse-DNS tags)
            parse_ilst_triplet(data, pos + 8, item_end, tags);
        } else {
            // Find the 'data' atom inside this item
            if let Some(value) = find_data_atom(data, pos + 8, item_end) {
                let (name, description) = ilst_tag_name(item_type);
                if !name.is_empty() {
                    // Apply PrintConv for specific tags
                    let display_value = apply_ilst_print_conv(item_type, &value);
                    tags.push(mk(name, description, Value::String(display_value)));
                }
            }
        }

        pos = item_end;
    }
}

/// Find and decode the 'data' atom inside an ilst item.
fn find_data_atom(data: &[u8], start: usize, end: usize) -> Option<String> {
    let mut pos = start;

    while pos + 16 <= end {
        let size =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                as usize;
        let atom_type = &data[pos + 4..pos + 8];

        if size < 16 || pos + size > end {
            break;
        }

        if atom_type == b"data" {
            let data_type =
                u32::from_be_bytes([data[pos + 8], data[pos + 9], data[pos + 10], data[pos + 11]]);
            let value_data = &data[pos + 16..pos + size];

            return Some(match data_type & 0xFF {
                1 => {
                    // UTF-8
                    String::from_utf8_lossy(value_data).to_string()
                }
                2 => {
                    // UTF-16
                    let units: Vec<u16> = value_data
                        .chunks_exact(2)
                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                        .collect();
                    String::from_utf16_lossy(&units)
                }
                13 | 14 => {
                    // JPEG / PNG cover art
                    format!("(Binary data {} bytes, use -b option to extract)", value_data.len())
                }
                21 => {
                    // Signed integer
                    match value_data.len() {
                        1 => (value_data[0] as i8).to_string(),
                        2 => i16::from_be_bytes([value_data[0], value_data[1]]).to_string(),
                        4 => i32::from_be_bytes([
                            value_data[0],
                            value_data[1],
                            value_data[2],
                            value_data[3],
                        ])
                        .to_string(),
                        8 => i64::from_be_bytes([
                            value_data[0],
                            value_data[1],
                            value_data[2],
                            value_data[3],
                            value_data[4],
                            value_data[5],
                            value_data[6],
                            value_data[7],
                        ])
                        .to_string(),
                        _ => format!("(Signed {} bytes)", value_data.len()),
                    }
                }
                22 => {
                    // Unsigned integer
                    match value_data.len() {
                        1 => value_data[0].to_string(),
                        2 => u16::from_be_bytes([value_data[0], value_data[1]]).to_string(),
                        4 => u32::from_be_bytes([
                            value_data[0],
                            value_data[1],
                            value_data[2],
                            value_data[3],
                        ])
                        .to_string(),
                        _ => format!("(Unsigned {} bytes)", value_data.len()),
                    }
                }
                0 => {
                    // Implicit (binary) - try to decode as track/disc number etc.
                    if value_data.len() >= 4 {
                        // Track number format: 0x0000 + track(2) + total(2)
                        let track = u16::from_be_bytes([value_data[2], value_data[3]]);
                        if value_data.len() >= 6 {
                            let total = u16::from_be_bytes([value_data[4], value_data[5]]);
                            if total > 0 {
                                format!("{} of {}", track, total)
                            } else {
                                track.to_string()
                            }
                        } else {
                            track.to_string()
                        }
                    } else {
                        format!("(Binary {} bytes)", value_data.len())
                    }
                }
                _ => String::from_utf8_lossy(value_data).to_string(),
            });
        }

        pos += size;
    }

    None
}

/// Parse QuickTime text atom (©xxx at container level).
fn parse_qt_text_atom(
    atom_type: &[u8],
    data: &[u8],
    start: usize,
    end: usize,
    tags: &mut Vec<Tag>,
) {
    if start + 4 > end {
        return;
    }

    // QuickTime text: 2-byte text length + 2-byte language + text
    let text_len = u16::from_be_bytes([data[start], data[start + 1]]) as usize;
    let text_start = start + 4;

    if text_start + text_len <= end {
        let text = String::from_utf8_lossy(&data[text_start..text_start + text_len])
            .trim_end_matches('\0')
            .to_string();
        if !text.is_empty() {
            let key = String::from_utf8_lossy(&atom_type[1..4]).to_string();
            let (static_name, static_desc) = qt_text_name(&key);
            if !static_name.is_empty() {
                tags.push(mk(static_name, static_desc, Value::String(text)));
            }
            // Unknown © tags are skipped (they may be handled elsewhere)
        }
    }
}

/// Map ilst item types to tag names.
fn ilst_tag_name(item_type: &[u8]) -> (&'static str, &'static str) {
    match item_type {
        b"\xa9nam" => ("Title", "Title"),
        b"\xa9ART" => ("Artist", "Artist"),
        b"\xa9alb" => ("Album", "Album"),
        b"\xa9day" => ("ContentCreateDate", "Content Create Date"),
        b"\xa9cmt" => ("Comment", "Comment"),
        b"\xa9gen" => ("Genre", "Genre"),
        b"\xa9wrt" => ("Composer", "Composer"),
        b"\xa9too" => ("Encoder", "Encoder"),
        b"\xa9grp" => ("Grouping", "Grouping"),
        b"\xa9lyr" => ("Lyrics", "Lyrics"),
        b"\xa9des" => ("Description", "Description"),
        b"trkn" => ("TrackNumber", "Track Number"),
        b"disk" => ("DiskNumber", "Disk Number"),
        b"tmpo" => ("BeatsPerMinute", "Beats Per Minute"),
        b"cpil" => ("Compilation", "Compilation"),
        b"pgap" => ("PlayGap", "Play Gap"),
        b"covr" => ("CoverArt", "Cover Art"),
        b"aART" => ("AlbumArtist", "Album Artist"),
        b"cprt" => ("Copyright", "Copyright"),
        b"desc" => ("Description", "Description"),
        b"ldes" => ("LongDescription", "Long Description"),
        b"tvsh" => ("TVShow", "TV Show"),
        b"tven" => ("TVEpisodeID", "TV Episode ID"),
        b"tvsn" => ("TVSeason", "TV Season"),
        b"tves" => ("TVEpisode", "TV Episode"),
        b"purd" => ("PurchaseDate", "Purchase Date"),
        b"stik" => ("MediaType", "Media Type"),
        b"rtng" => ("Rating", "Rating"),
        _ => {
            if item_type[0] == 0xA9 {
                // Unknown © tag - skip
                return ("", "");
            }
            ("", "")
        }
    }
}

/// Map QuickTime text atom keys to tag names.
fn qt_text_name(key: &str) -> (&'static str, &'static str) {
    match key {
        "nam" => ("Title", "Title"),
        "ART" => ("Artist", "Artist"),
        "alb" => ("Album", "Album"),
        "day" => ("ContentCreateDate", "Content Create Date"),
        "cmt" => ("Comment", "Comment"),
        "gen" => ("Genre", "Genre"),
        "wrt" => ("Composer", "Composer"),
        "too" => ("Encoder", "Encoder"),
        "inf" => ("Information", "Information"),
        "req" => ("Requirements", "Requirements"),
        "fmt" => ("Format", "Format"),
        "dir" => ("Director", "Director"),
        "prd" => ("Producer", "Producer"),
        "prf" => ("Performers", "Performers"),
        "src" => ("SourceCredits", "Source Credits"),
        "swr" => ("SoftwareVersion", "Software Version"),
        "mak" => ("Make", "Make"),
        "mod" => ("Model", "Model"),
        "cpy" => ("Copyright", "Copyright"),
        "com" => ("Composer", "Composer"),
        "lyr" => ("Lyrics", "Lyrics"),
        "grp" => ("Grouping", "Grouping"),
        _ => ("", ""),
    }
}

/// Convert Mac epoch (seconds since 1904-01-01) to date string.
fn mac_epoch_to_string(secs: u64) -> Option<String> {
    if secs == 0 {
        return None;
    }
    // Mac epoch: Jan 1, 1904. Unix epoch: Jan 1, 1970.
    // Difference: 66 years + 17 leap days = (66*365+17)*24*3600
    let offset: i64 = (66 * 365 + 17) * 24 * 3600;
    let unix_secs = secs as i64 - offset;
    if unix_secs < 0 {
        // Likely wrong epoch - some software uses Unix epoch
        // Try treating as Unix epoch (add back offset to check validity)
        // For now just skip invalid dates
        return None;
    }

    // Simple date conversion
    let days = unix_secs / 86400;
    let time_of_day = unix_secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let mut y = 1970i32;
    let mut remaining_days = days;

    loop {
        let days_in_year = if is_leap_year(y) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        y += 1;
    }

    let months = [
        31i64,
        if is_leap_year(y) { 29 } else { 28 },
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
    let mut m = 1;
    for &days_in_month in &months {
        if remaining_days < days_in_month {
            break;
        }
        remaining_days -= days_in_month;
        m += 1;
    }
    let d = remaining_days + 1;

    Some(format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        y, m, d, hours, minutes, seconds
    ))
}

fn is_leap_year(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// Convert duration (seconds) to display string.
/// Mirrors ExifTool's ConvertDuration.
fn convert_duration(secs: f64) -> String {
    if secs == 0.0 {
        return "0 s".to_string();
    }
    let sign = if secs < 0.0 { "-" } else { "" };
    let secs = secs.abs();
    if secs < 30.0 {
        return format!("{}{:.2} s", sign, secs);
    }
    let secs_rounded = secs + 0.5;
    let h = (secs_rounded / 3600.0) as u64;
    let m = ((secs_rounded % 3600.0) / 60.0) as u64;
    let s = (secs_rounded % 60.0) as u64;
    if h > 24 {
        let d = h / 24;
        let h = h % 24;
        format!("{}{} days {}:{:02}:{:02}", sign, d, h, m, s)
    } else {
        format!("{}{}:{:02}:{:02}", sign, h, m, s)
    }
}

/// Look up an ftyp brand code to a human-readable description.
fn ftyp_brand_name(brand: &str) -> Option<&'static str> {
    match brand {
        "3g2a" => Some("3GPP2 Media (.3G2) compliant with 3GPP2 C.S0050-0 V1.0"),
        "3g2b" => Some("3GPP2 Media (.3G2) compliant with 3GPP2 C.S0050-A V1.0.0"),
        "3g2c" => Some("3GPP2 Media (.3G2) compliant with 3GPP2 C.S0050-B v1.0"),
        "3gp4" => Some("3GPP Media (.3GP) Release 4"),
        "3gp5" => Some("3GPP Media (.3GP) Release 5"),
        "3gp6" => Some("3GPP Media (.3GP) Release 6 Basic Profile"),
        "aax " => Some("Audible Enhanced Audiobook (.AAX)"),
        "avc1" => Some("MP4 Base w/ AVC ext [ISO 14496-12:2005]"),
        "avif" => Some("AV1 Image File Format (.AVIF)"),
        "CAEP" => Some("Canon Digital Camera"),
        "crx " => Some("Canon Raw (.CRX)"),
        "F4A " => Some("Audio for Adobe Flash Player 9+ (.F4A)"),
        "F4B " => Some("Audio Book for Adobe Flash Player 9+ (.F4B)"),
        "F4P " => Some("Protected Video for Adobe Flash Player 9+ (.F4P)"),
        "F4V " => Some("Video for Adobe Flash Player 9+ (.F4V)"),
        "heic" => Some("High Efficiency Image Format HEVC still image (.HEIC)"),
        "hevc" => Some("High Efficiency Image Format HEVC sequence (.HEICS)"),
        "heix" => Some("High Efficiency Image Format still image (.HEIF)"),
        "isom" => Some("MP4 Base Media v1 [IS0 14496-12:2003]"),
        "iso2" => Some("MP4 Base Media v2 [ISO 14496-12:2005]"),
        "iso3" => Some("MP4 Base Media v3"),
        "iso4" => Some("MP4 Base Media v4"),
        "iso5" => Some("MP4 Base Media v5"),
        "iso6" => Some("MP4 Base Media v6"),
        "iso7" => Some("MP4 Base Media v7"),
        "iso8" => Some("MP4 Base Media v8"),
        "iso9" => Some("MP4 Base Media v9"),
        "JP2 " => Some("JPEG 2000 Image (.JP2) [ISO 15444-1 ?]"),
        "jpm " => Some("JPEG 2000 Compound Image (.JPM) [ISO 15444-6]"),
        "jpx " => Some("JPEG 2000 with extensions (.JPX) [ISO 15444-2]"),
        "M4A " => Some("Apple iTunes AAC-LC (.M4A) Audio"),
        "M4B " => Some("Apple iTunes AAC-LC (.M4B) Audio Book"),
        "M4P " => Some("Apple iTunes AAC-LC (.M4P) AES Protected Audio"),
        "M4V " => Some("Apple iTunes Video (.M4V) Video"),
        "M4VH" => Some("Apple TV (.M4V)"),
        "M4VP" => Some("Apple iPhone (.M4V)"),
        "mif1" => Some("High Efficiency Image Format still image (.HEIF)"),
        "mjp2" => Some("Motion JPEG 2000 [ISO 15444-3] General Profile"),
        "mmp4" => Some("MPEG-4/3GPP Mobile Profile (.MP4/3GP) (for NTT)"),
        "mp41" => Some("MP4 v1 [ISO 14496-1:ch13]"),
        "mp42" => Some("MP4 v2 [ISO 14496-14]"),
        "MSNV" => Some("MPEG-4 (.MP4) for SonyPSP"),
        "msf1" => Some("High Efficiency Image Format sequence (.HEIFS)"),
        "NDAS" => Some("MP4 v2 [ISO 14496-14] Nero Digital AAC Audio"),
        "pana" => Some("Panasonic Digital Camera"),
        "qt  " => Some("Apple QuickTime (.MOV/QT)"),
        "sdv " => Some("SD Memory Card Video"),
        "XAVC" => Some("Sony XAVC"),
        _ => None,
    }
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let print_value = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "QuickTime".into(),
            family1: "QuickTime".into(),
            family2: "Video".into(),
        },
        raw_value: value,
        print_value,
        priority: 0,
    }
}

fn mk_makernote(name: &str, description: &str, value: Value) -> Tag {
    let print_value = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "MakerNotes".into(),
            family1: "MakerNotes".into(),
            family2: "Camera".into(),
        },
        raw_value: value,
        print_value,
        priority: 0,
    }
}

/// Parse Pentax::MOV binary data (GROUPS { 0 => MakerNotes, 2 => Camera }, ByteOrder => LE).
/// data starts with "PENTAX DIGITAL CAMERA\0..."
fn parse_pentax_mov(data: &[u8], tags: &mut Vec<Tag>) {
    // Make (0x00, string[24])
    if data.len() >= 24 {
        let make_bytes = &data[0..24];
        let end = make_bytes.iter().position(|&b| b == 0).unwrap_or(24);
        let make = String::from_utf8_lossy(&make_bytes[..end]).to_string();
        if !make.is_empty() {
            tags.push(mk_makernote("Make", "Make", Value::String(make)));
        }
    }

    // ExposureTime (0x26, int32u LE), ValueConv = '10/$val'
    if data.len() >= 0x2a {
        let val = u32::from_le_bytes([data[0x26], data[0x27], data[0x28], data[0x29]]);
        if val > 0 {
            let et = 10.0 / val as f64;
            // Format like ExifTool: "1/N" for exposures < 1
            let et_str = if et < 1.0 {
                let denom = (1.0 / et).round() as u32;
                format!("1/{}", denom)
            } else {
                format!("{}", et)
            };
            tags.push(mk_makernote("ExposureTime", "Exposure Time", Value::String(et_str)));
        }
    }

    // FNumber (0x2a, rational64u LE)
    if data.len() >= 0x32 {
        let n = u32::from_le_bytes([data[0x2a], data[0x2b], data[0x2c], data[0x2d]]);
        let d = u32::from_le_bytes([data[0x2e], data[0x2f], data[0x30], data[0x31]]);
        if d > 0 {
            let fn_val = n as f64 / d as f64;
            let fn_str = format!("{:.1}", fn_val);
            tags.push(mk_makernote("FNumber", "F Number", Value::String(fn_str)));
        }
    }

    // ExposureCompensation (0x32, rational64s LE)
    if data.len() >= 0x3a {
        let n = i32::from_le_bytes([data[0x32], data[0x33], data[0x34], data[0x35]]);
        let d = i32::from_le_bytes([data[0x36], data[0x37], data[0x38], data[0x39]]);
        if d != 0 {
            let ec_val = n as f64 / d as f64;
            let ec_str = if ec_val == 0.0 {
                "0".to_string()
            } else {
                format!("{:+.1}", ec_val)
            };
            tags.push(mk_makernote("ExposureCompensation", "Exposure Compensation", Value::String(ec_str)));
        }
    }

    // WhiteBalance (0x44, int16u LE)
    if data.len() >= 0x46 {
        let wb = u16::from_le_bytes([data[0x44], data[0x45]]);
        let wb_str = match wb {
            0 => "Auto",
            1 => "Daylight",
            2 => "Shade",
            3 => "Fluorescent",
            4 => "Tungsten",
            5 => "Manual",
            _ => "Unknown",
        };
        tags.push(mk_makernote("WhiteBalance", "White Balance", Value::String(wb_str.into())));
    }

    // FocalLength (0x48, rational64u LE)
    if data.len() >= 0x50 {
        let n = u32::from_le_bytes([data[0x48], data[0x49], data[0x4a], data[0x4b]]);
        let d = u32::from_le_bytes([data[0x4c], data[0x4d], data[0x4e], data[0x4f]]);
        if d > 0 {
            let fl_val = n as f64 / d as f64;
            let fl_str = format!("{:.1} mm", fl_val);
            tags.push(mk_makernote("FocalLength", "Focal Length", Value::String(fl_str)));
        }
    }

    // ISO (0xaf, int16u LE)
    if data.len() >= 0xb1 {
        let iso = u16::from_le_bytes([data[0xaf], data[0xb0]]);
        if iso > 0 {
            tags.push(mk_makernote("ISO", "ISO", Value::U16(iso)));
        }
    }
}

/// Parse Canon UUID box content (CR3 files).
///
/// The Canon UUID (85C0B687...) contains a series of sub-boxes:
/// - CNCV: compressor version string
/// - CMT1: TIFF with IFD0 (Make, Model, ImageWidth/Height, etc.)
/// - CMT2: TIFF with ExifIFD (ExposureTime, FNumber, ISO, etc.)
/// - CMT3: TIFF with Canon MakerNotes IFD (standalone)
/// - CMT4: TIFF with GPS IFD as IFD0
/// - THMB: thumbnail image
///
/// Mirrors Canon.pm %Image::ExifTool::Canon::uuid processing.
fn parse_canon_uuid(
    data: &[u8],
    start: usize,
    end: usize,
    tags: &mut Vec<Tag>,
) {
    let mut pos = start;
    let mut model = String::new();

    // First pass: find Make/Model from CMT1 if available (for MakerNotes dispatch)
    // We'll extract model after processing CMT1.

    while pos + 8 <= end {
        let size = u32::from_be_bytes([
            data[pos], data[pos + 1], data[pos + 2], data[pos + 3],
        ]) as usize;
        if size < 8 || pos + size > end {
            break;
        }
        let box_type = &data[pos + 4..pos + 8];
        let content_start = pos + 8;
        let content_end = pos + size;

        match box_type {
            b"CNCV" => {
                // Canon Compressor Version - string tag
                if content_end > content_start {
                    let s = String::from_utf8_lossy(&data[content_start..content_end])
                        .trim_end_matches('\0')
                        .to_string();
                    if !s.is_empty() {
                        tags.push(mk("CompressorVersion", "Compressor Version", Value::String(s)));
                    }
                }
            }
            b"CMT1" => {
                // IFD0: TIFF containing Make, Model, ImageWidth/Height, etc.
                // Don't parse MakerNotes from CMT1 — they're in CMT3 (full version)
                if content_end > content_start {
                    let tiff_data = &data[content_start..content_end];
                    if let Ok(exif_tags) = ExifReader::read(tiff_data) {
                        // Extract model for CMT3 MakerNotes dispatch
                        if let Some(m) = exif_tags.iter().find(|t| t.name == "Model") {
                            model = m.print_value.clone();
                        }
                        // Filter out MakerNote tags from CMT1 (CMT3 has the full version)
                        for t in exif_tags {
                            if t.group.family0 == "MakerNotes" { continue; }
                            if t.name == "MakerNoteByteOrder" { continue; }
                            tags.push(t);
                        }
                    }
                }
            }
            b"CMT2" => {
                // ExifIFD: TIFF whose IFD0 IS the ExifIFD (ExposureTime, FNumber, etc.)
                if content_end > content_start {
                    let tiff_data = &data[content_start..content_end];
                    // Parse IFD0 as ExifIFD (the CMT2 TIFF stores ExifIFD tags directly in IFD0)
                    let exif_tags = ExifReader::read_as_named_ifd(tiff_data, "ExifIFD");
                    tags.extend(exif_tags);
                }
            }
            b"CMT3" => {
                // MakerNoteCanon: TIFF whose IFD0 IS the Canon MakerNotes IFD
                // Note: CMT3 has incomplete sub-tables (truncated ColorData).
                // CTMD type 8 has the full MakerNote with correct ColorData.
                // Only add CMT3 tags that CTMD doesn't provide (CTMD parsed later).
                if content_end > content_start {
                    let tiff_data = &data[content_start..content_end];
                    let mn_tags = parse_canon_cr3_makernotes(tiff_data, &model);
                    // Store CMT3 tags — they may be overwritten by CTMD later
                    tags.extend(mn_tags);
                }
            }
            b"CMT4" => {
                // GPS: TIFF whose IFD0 IS the GPS IFD
                if content_end > content_start {
                    let tiff_data = &data[content_start..content_end];
                    let gps_tags = ExifReader::read_as_named_ifd(tiff_data, "GPS");
                    tags.extend(gps_tags);
                }
            }
            b"THMB" => {
                // ThumbnailImage: skip 16-byte header
                if content_end > content_start + 16 {
                    let thumb_data = &data[content_start + 16..content_end];
                    let size = thumb_data.len();
                    tags.push(Tag {
                        id: TagId::Text("ThumbnailImage".into()),
                        name: "ThumbnailImage".into(),
                        description: "Thumbnail Image".into(),
                        group: TagGroup { family0: "MakerNotes".into(), family1: "Canon".into(), family2: "Preview".into() },
                        raw_value: Value::Binary(thumb_data.to_vec()),
                        print_value: format!("(Binary data {} bytes, use -b option to extract)", size),
                        priority: 0,
                    });
                }
            }
            _ => {
                // CCTP, CTBO, CNCV, free, etc. - ignore
            }
        }

        pos += size;
    }
}
