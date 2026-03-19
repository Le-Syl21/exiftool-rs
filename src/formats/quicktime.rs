//! QuickTime/MP4/M4A/MOV file format reader.
//!
//! Parses ISO Base Media File Format (ISOBMFF) atom/box tree to extract
//! metadata from moov/udta/meta/ilst and embedded EXIF/XMP.
//! Mirrors ExifTool's QuickTime.pm.

use crate::error::{Error, Result};
use crate::metadata::XmpReader;
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
}

pub fn read_quicktime(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 8 {
        return Err(Error::InvalidData("file too small for QuickTime".into()));
    }

    let mut tags = Vec::new();

    // Check for ftyp
    if data.len() >= 12 && &data[4..8] == b"ftyp" {
        let size = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let brand = String::from_utf8_lossy(&data[8..12]).to_string();
        tags.push(mk("MajorBrand", "Major Brand", Value::String(brand)));
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
    parse_atoms(data, 0, data.len(), &mut tags, &mut state, 0);

    Ok(tags)
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
            b"moov" | b"trak" | b"edts" => {
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
                // XMP UUID: BE7ACFCB97A942E89C71999491E3AFAC
                if content_end - content_start > 16 {
                    let uuid = &data[content_start..content_start + 16];
                    if uuid[0] == 0xBE
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
    state: &QtState,
) {
    if &state.handler_type != b"vide" {
        return;
    }
    let d = &data[start..end];
    if d.len() < 8 {
        return;
    }
    let entry_count = u32::from_be_bytes([d[4], d[5], d[6], d[7]]) as usize;
    if entry_count == 0 || d.len() < 8 + entry_count * 8 {
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
