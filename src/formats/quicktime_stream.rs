//! QuickTime/MP4 timed metadata extraction (ExtractEmbedded / -ee).
//!
//! Ported from ExifTool's QuickTimeStream.pl.
//! When `-ee` is used on MP4/MOV files, scans sample data from timed
//! metadata tracks and dispatches to format-specific GPS/sensor processors.

use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

// ─── conversion factors ───
const KNOTS_TO_KPH: f64 = 1.852;
const MPS_TO_KPH: f64 = 3.6;
const MPH_TO_KPH: f64 = 1.60934;

// ─── per-track info collected during atom parsing ───

/// Information about one timed-metadata track.
#[derive(Debug, Clone, Default)]
pub struct TrackInfo {
    /// Handler type (e.g. b"vide", b"soun", b"meta", b"text", etc.)
    pub handler_type: [u8; 4],
    /// MetaFormat / OtherFormat from stsd (e.g. "gpmd", "camm", "mebx", "tx3g", etc.)
    pub meta_format: Option<String>,
    /// Media timescale from mdhd
    pub media_timescale: u32,
    /// Chunk offsets from stco/co64
    pub stco: Vec<u64>,
    /// Sample-to-chunk entries: (first_chunk, samples_per_chunk, desc_index)
    pub stsc: Vec<(u32, u32, u32)>,
    /// Sample sizes from stsz
    pub stsz: Vec<u32>,
    /// Time-to-sample entries: pairs of (count, delta)
    pub stts: Vec<(u32, u32)>,
}

/// Collected track infos gathered during first-pass atom parsing.
#[derive(Debug, Clone, Default)]
pub struct StreamState {
    pub tracks: Vec<TrackInfo>,
    /// Scratch: current track being built (pushed into tracks on next trak)
    pub current: TrackInfo,
    /// Whether we are inside an stbl for the current track
    pub in_stbl: bool,
}

// ─── public entry point ───

/// Extract timed metadata tags from MP4 sample data.
/// Called after normal atom parsing when `extract_embedded > 0`.
pub fn extract_stream_tags(data: &[u8], tracks: &[TrackInfo], _extract_embedded: u8) -> Vec<Tag> {
    let mut tags = Vec::new();
    let mut doc_count: u32 = 0;

    for track in tracks {
        let handler = &track.handler_type;
        // Skip audio/video tracks (we only want timed metadata)
        if handler == b"soun" || handler == b"vide" {
            continue;
        }

        // Compute per-sample (offset, size, time, duration)
        let samples = compute_samples(track);
        if samples.is_empty() {
            continue;
        }

        let meta_format = track.meta_format.as_deref().unwrap_or("");

        for s in &samples {
            if s.offset as usize + s.size as usize > data.len() || s.size == 0 {
                continue;
            }
            let sample_data = &data[s.offset as usize..(s.offset as usize + s.size as usize)];

            let mut sample_tags = Vec::new();

            // Dispatch based on handler_type and meta_format
            let dispatched = dispatch_sample(
                sample_data,
                handler,
                meta_format,
                s.time,
                s.duration,
                &mut sample_tags,
            );

            if dispatched && !sample_tags.is_empty() {
                doc_count += 1;
                // Prepend SampleTime / SampleDuration
                if let Some(t) = s.time {
                    sample_tags.insert(
                        0,
                        mk_stream(
                            "SampleTime",
                            "Sample Time",
                            Value::String(format!("{:.6}", t)),
                        ),
                    );
                }
                if let Some(d) = s.duration {
                    sample_tags.insert(
                        1,
                        mk_stream(
                            "SampleDuration",
                            "Sample Duration",
                            Value::String(format!("{:.6}", d)),
                        ),
                    );
                }
                // Tag each with document number
                for t in &mut sample_tags {
                    t.description = format!("{} (Doc{})", t.description, doc_count);
                }
                tags.extend(sample_tags);
            }
        }
    }

    // Also do a brute-force mdat scan for freeGPS if we found nothing
    if doc_count == 0 {
        scan_mdat_for_freegps(data, &mut tags, &mut doc_count);
    }

    tags
}

// ─── sample computation ───

struct SampleInfo {
    offset: u64,
    size: u32,
    time: Option<f64>,
    duration: Option<f64>,
}

fn compute_samples(track: &TrackInfo) -> Vec<SampleInfo> {
    let mut result = Vec::new();
    if track.stsz.is_empty() || track.stco.is_empty() || track.stsc.is_empty() {
        return result;
    }

    let ts = if track.media_timescale > 0 {
        track.media_timescale as f64
    } else {
        1.0
    };

    // Build flat stts list
    let mut stts_flat: Vec<(u32, u32)> = Vec::new();
    for &(count, delta) in &track.stts {
        stts_flat.push((count, delta));
    }
    let mut stts_idx = 0;
    let mut stts_remaining: u32 = if !stts_flat.is_empty() {
        stts_flat[0].0
    } else {
        0
    };
    let mut stts_delta: u32 = if !stts_flat.is_empty() {
        stts_flat[0].1
    } else {
        0
    };
    let mut time_acc: u64 = 0;
    let has_time = !stts_flat.is_empty();

    // Build sample list from stsc + stco
    let mut stsc_idx = 0;
    let mut samples_per_chunk = track.stsc[0].1;
    let mut next_first_chunk: Option<u32> = if track.stsc.len() > 1 {
        Some(track.stsc[1].0)
    } else {
        None
    };

    let mut sample_idx: usize = 0;

    for (chunk_idx_0, &chunk_offset) in track.stco.iter().enumerate() {
        let chunk_num = chunk_idx_0 as u32 + 1; // 1-based

        // Advance stsc if needed
        if let Some(nfc) = next_first_chunk {
            if chunk_num >= nfc {
                stsc_idx += 1;
                if stsc_idx < track.stsc.len() {
                    samples_per_chunk = track.stsc[stsc_idx].1;
                    next_first_chunk = if stsc_idx + 1 < track.stsc.len() {
                        Some(track.stsc[stsc_idx + 1].0)
                    } else {
                        None
                    };
                }
            }
        }

        let mut offset_in_chunk: u64 = 0;
        for _ in 0..samples_per_chunk {
            if sample_idx >= track.stsz.len() {
                break;
            }
            let sz = track.stsz[sample_idx];
            let sample_time = if has_time {
                Some(time_acc as f64 / ts)
            } else {
                None
            };
            let sample_dur = if has_time {
                Some(stts_delta as f64 / ts)
            } else {
                None
            };

            result.push(SampleInfo {
                offset: chunk_offset + offset_in_chunk,
                size: sz,
                time: sample_time,
                duration: sample_dur,
            });

            offset_in_chunk += sz as u64;
            sample_idx += 1;

            // Advance stts
            if has_time {
                time_acc += stts_delta as u64;
                stts_remaining = stts_remaining.saturating_sub(1);
                if stts_remaining == 0 {
                    stts_idx += 1;
                    if stts_idx < stts_flat.len() {
                        stts_remaining = stts_flat[stts_idx].0;
                        stts_delta = stts_flat[stts_idx].1;
                    }
                }
            }
        }
    }

    result
}

// ─── sample dispatch ───

fn dispatch_sample(
    sample: &[u8],
    handler: &[u8; 4],
    meta_format: &str,
    _time: Option<f64>,
    _dur: Option<f64>,
    tags: &mut Vec<Tag>,
) -> bool {
    // Try by meta_format first
    match meta_format {
        "camm" => return process_camm(sample, tags),
        "gpmd" => return process_gpmd(sample, tags),
        "mebx" => return process_mebx(sample, tags),
        "tx3g" => return process_tx3g(sample, tags),
        _ => {}
    }

    // Try by handler type
    match handler {
        b"text" | b"sbtl" => {
            // Text/subtitle track: try NMEA/text GPS
            if meta_format == "tx3g" {
                return process_tx3g(sample, tags);
            }
            return process_text(sample, tags);
        }
        b"gps " => {
            // GPS data list track: check for freeGPS
            if sample.len() >= 12 && &sample[4..12] == b"freeGPS " {
                return process_freegps(sample, tags);
            }
            // Try NMEA
            return process_nmea(sample, tags);
        }
        b"meta" | b"data" => {
            // Timed metadata: try by meta_format
            match meta_format {
                "RVMI" => return process_rvmi(sample, tags),
                _ => {
                    if sample.len() >= 12 && &sample[4..12] == b"freeGPS " {
                        return process_freegps(sample, tags);
                    }
                }
            }
        }
        _ => {
            // Try Kenwood udta format
            if sample.starts_with(b"VIDEO") && sample.windows(2).any(|w| w == b"\xfe\xfe") {
                return process_kenwood(sample, tags);
            }
        }
    }
    false
}

// ─── freeGPS processor (covers ~20 dashcam variants) ───

fn process_freegps(data: &[u8], tags: &mut Vec<Tag>) -> bool {
    if data.len() < 82 {
        return false;
    }

    // Type 1: encrypted Azdome/EEEkit (byte 18..26 = \xaa\xaa\xf2\xe1\xf0\xee\x54\x54)
    if data.len() > 26 && &data[18..26] == b"\xaa\xaa\xf2\xe1\xf0\xee\x54\x54" {
        return process_freegps_type1_encrypted(data, tags);
    }

    // Type 2: NMEA in freeGPS (Nextbase 512GW) - date at offset 52
    if data.len() > 64 {
        if let Some(dt) = try_ascii_digits(&data[52..], 14) {
            if dt.len() == 14 {
                return process_freegps_type2_nmea(data, tags);
            }
        }
    }

    // Type 3/17: Novatek binary at offset 72 (A[NS][EW]\0)
    if data.len() > 75 && data[72] == b'A' && is_ns(data[73]) && is_ew(data[74]) && data[75] == 0 {
        return process_freegps_novatek(data, tags);
    }

    // Type 3b: Viofo/Kenwood at offset 37/85 (\0\0\0A[NS][EW]\0)
    if data.len() > 44 && &data[37..41] == b"\0\0\0A" && is_ns(data[41]) && is_ew(data[42]) {
        return process_freegps_viofo(data, 0, tags);
    }
    if data.len() > 92 && &data[85..89] == b"\0\0\0A" && is_ns(data[89]) && is_ew(data[90]) {
        // Kenwood DRV-A510W: header 48 bytes longer
        return process_freegps_viofo(data, 48, tags);
    }

    // Type 6: Akaso (A\0\0\0 at offset 60, [NS]\0\0\0 at +8, [EW]\0\0\0 at +16)
    if data.len() > 96
        && data[60] == b'A'
        && data[61] == 0
        && data[62] == 0
        && data[63] == 0
        && is_ns(data[68])
        && is_ew(data[76])
    {
        return process_freegps_akaso(data, tags);
    }

    // Type 10: Vantrue S1 (A[NS][EW]\0 at offset 64)
    if data.len() > 100 && data[64] == b'A' && is_ns(data[65]) && is_ew(data[66]) && data[67] == 0 {
        return process_freegps_vantrue_s1(data, tags);
    }

    // Type 12: double lat/lon format (A\0 at 60, [NS]\0 at 72, [EW]\0 at 88)
    if data.len() >= 0x88
        && data[60] == b'A'
        && data[61] == 0
        && is_ns(data[72])
        && data[73] == 0
        && is_ew(data[88])
        && data[89] == 0
    {
        return process_freegps_type12(data, tags);
    }

    // Type 13: INNOVV (A[NS][EW]\0 at offset 16)
    if data.len() > 48 && data[16] == b'A' && is_ns(data[17]) && is_ew(data[18]) && data[19] == 0 {
        return process_freegps_innovv(data, tags);
    }

    // Type 15: Vantrue N4 (A at 28, [NS] at 40, [EW] at 56)
    if data.len() > 80 && data[28] == b'A' && is_ns(data[40]) && is_ew(data[56]) {
        return process_freegps_vantrue_n4(data, tags);
    }

    // Type 20: Nextbase 512G binary (32-byte records starting at 0x32)
    if data.len() > 0x50 {
        return process_freegps_nextbase_binary(data, tags);
    }

    false
}

// ──── freeGPS Type 1: encrypted Azdome ────

fn process_freegps_type1_encrypted(data: &[u8], tags: &mut Vec<Tag>) -> bool {
    let n = (data.len() - 18).min(0x101);
    let decrypted: Vec<u8> = data[18..18 + n].iter().map(|b| b ^ 0xaa).collect();

    if decrypted.len() < 66 {
        return false;
    }

    // date/time at decrypted[8..22]
    let dt_bytes = &decrypted[8..22];
    let dt_str = match std::str::from_utf8(dt_bytes) {
        Ok(s) if s.chars().all(|c| c.is_ascii_digit()) => s,
        _ => return false,
    };
    if dt_str.len() < 14 {
        return false;
    }
    let yr = &dt_str[0..4];
    let mo = &dt_str[4..6];
    let dy = &dt_str[6..8];
    let hr = &dt_str[8..10];
    let mi = &dt_str[10..12];
    let se = &dt_str[12..14];

    // lat/lon: [NS] at decrypted[37], lat 8 digits at [38..46], [EW] at [46], lon 9 digits at [47..56]
    if decrypted.len() < 57 {
        return false;
    }
    let lat_ref = decrypted[37];
    if lat_ref != b'N' && lat_ref != b'S' {
        return false;
    }
    let lon_ref = decrypted[46];
    if lon_ref != b'E' && lon_ref != b'W' {
        return false;
    }
    let lat_str = match std::str::from_utf8(&decrypted[38..46]) {
        Ok(s) if s.chars().all(|c| c.is_ascii_digit()) => s,
        _ => return false,
    };
    let lon_str = match std::str::from_utf8(&decrypted[47..56]) {
        Ok(s) if s.chars().all(|c| c.is_ascii_digit()) => s,
        _ => return false,
    };
    let lat: f64 = lat_str.parse::<f64>().unwrap_or(0.0) / 1e4;
    let lon: f64 = lon_str.parse::<f64>().unwrap_or(0.0) / 1e4;
    let (lat_dd, lon_dd) = convert_lat_lon(lat, lon);
    let lat_final = lat_dd * if lat_ref == b'S' { -1.0 } else { 1.0 };
    let lon_final = lon_dd * if lon_ref == b'W' { -1.0 } else { 1.0 };

    tags.push(mk_gps_dt(&format!(
        "{}:{}:{} {}:{}:{}Z",
        yr, mo, dy, hr, mi, se
    )));
    tags.push(mk_gps_lat(lat_final));
    tags.push(mk_gps_lon(lon_final));

    // speed: 8 digits at decrypted[56..64] (if present)
    if decrypted.len() >= 65 {
        if let Ok(s) = std::str::from_utf8(&decrypted[56..64]) {
            if let Ok(spd) = s.trim_start_matches('0').parse::<f64>() {
                tags.push(mk_gps_spd(spd));
            }
        }
    }

    true
}

// ──── freeGPS Type 2: NMEA in freeGPS ────

fn process_freegps_type2_nmea(data: &[u8], tags: &mut Vec<Tag>) -> bool {
    // Camera date/time at offset 52
    if let Some(dt) = try_ascii_digits(&data[52..], 14) {
        if dt.len() >= 14 {
            let cam_dt = format!(
                "{}:{}:{} {}:{}:{}",
                &dt[0..4],
                &dt[4..6],
                &dt[6..8],
                &dt[8..10],
                &dt[10..12],
                &dt[12..14]
            );
            tags.push(mk_stream(
                "CameraDateTime",
                "Camera Date/Time",
                Value::String(cam_dt),
            ));
        }
    }

    // Search for NMEA RMC sentence in the data
    let text = crate::encoding::decode_utf8_or_latin1(data);
    if parse_nmea_rmc(&text, tags) {
        return true;
    }
    if parse_nmea_gga(&text, tags) {
        return true;
    }
    false
}

// ──── freeGPS Novatek binary (Type 17) ────

fn process_freegps_novatek(data: &[u8], tags: &mut Vec<Tag>) -> bool {
    // Offsets (from data start, LE):
    // 0x30: hr, 0x34: min, 0x38: sec, 0x3c: yr-2000, 0x40: mon, 0x44: day
    // 0x48: stat(A/V), 0x49: latRef, 0x4a: lonRef
    // 0x4c: lat (float), 0x50: lon (float), 0x54: speed(knots,float), 0x58: heading(float)
    if data.len() < 0x5c {
        return false;
    }
    let hr = get_u32_le(data, 0x30);
    let min = get_u32_le(data, 0x34);
    let sec = get_u32_le(data, 0x38);
    let yr = get_u32_le(data, 0x3c);
    let mon = get_u32_le(data, 0x40);
    let day = get_u32_le(data, 0x44);
    let lat_ref = data[0x49];
    let lon_ref = data[0x4a];

    if !(1..=12).contains(&mon) || !(1..=31).contains(&day) {
        return false;
    }

    let full_yr = if yr < 2000 { yr + 2000 } else { yr };

    let lat = get_f32_le(data, 0x4c) as f64;
    let lon = get_f32_le(data, 0x50) as f64;
    let spd = get_f32_le(data, 0x54) as f64 * KNOTS_TO_KPH;
    let trk = get_f32_le(data, 0x58) as f64;

    let (lat_dd, lon_dd) = convert_lat_lon(lat, lon);
    let lat_final = lat_dd * if lat_ref == b'S' { -1.0 } else { 1.0 };
    let lon_final = lon_dd * if lon_ref == b'W' { -1.0 } else { 1.0 };

    tags.push(mk_gps_dt(&format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}Z",
        full_yr, mon, day, hr, min, sec
    )));
    tags.push(mk_gps_lat(lat_final));
    tags.push(mk_gps_lon(lon_final));
    tags.push(mk_gps_spd(spd));
    tags.push(mk_gps_trk(trk));

    true
}

// ──── freeGPS Viofo/Kenwood (Type 3) ────

fn process_freegps_viofo(data: &[u8], extra_offset: usize, tags: &mut Vec<Tag>) -> bool {
    let d = if extra_offset > 0 && data.len() > extra_offset {
        &data[extra_offset..]
    } else {
        data
    };
    if d.len() < 0x3c {
        return false;
    }

    let hr = get_u32_le(d, 0x10);
    let min = get_u32_le(d, 0x14);
    let sec = get_u32_le(d, 0x18);
    let yr = get_u32_le(d, 0x1c);
    let mon = get_u32_le(d, 0x20);
    let day = get_u32_le(d, 0x24);

    let lat_ref = d[0x29]; // N or S
    let lon_ref = d[0x2a]; // E or W

    if !(1..=12).contains(&mon) || !(1..=31).contains(&day) {
        return false;
    }

    let full_yr = if yr < 2000 { yr + 2000 } else { yr };

    let lat = get_f32_le(d, 0x2c) as f64;
    let lon = get_f32_le(d, 0x30) as f64;
    let spd = get_f32_le(d, 0x34) as f64 * KNOTS_TO_KPH;
    let trk = get_f32_le(d, 0x38) as f64;

    tags.push(mk_gps_dt(&format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}Z",
        full_yr, mon, day, hr, min, sec
    )));
    tags.push(mk_gps_lat(lat * if lat_ref == b'S' { -1.0 } else { 1.0 }));
    tags.push(mk_gps_lon(lon * if lon_ref == b'W' { -1.0 } else { 1.0 }));
    tags.push(mk_gps_spd(spd));
    tags.push(mk_gps_trk(trk));

    true
}

// ──── freeGPS Akaso (Type 6) ────

fn process_freegps_akaso(data: &[u8], tags: &mut Vec<Tag>) -> bool {
    if data.len() < 0x58 {
        return false;
    }
    let lat_ref = data[68];
    let lon_ref = data[76];
    let hr = get_u32_le(data, 48);
    let min = get_u32_le(data, 52);
    let sec = get_u32_le(data, 56);
    let yr = get_u32_le(data, 84);
    let mon = get_u32_le(data, 88);
    let day = get_u32_le(data, 92);

    if !(1..=12).contains(&mon) {
        return false;
    }

    let lat = get_f32_le(data, 0x40) as f64;
    let lon = get_f32_le(data, 0x48) as f64;
    let spd = get_f32_le(data, 0x50) as f64;
    let trk = get_f32_le(data, 0x54) as f64;

    tags.push(mk_gps_dt(&format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}Z",
        yr, mon, day, hr, min, sec
    )));
    tags.push(mk_gps_lat(lat * if lat_ref == b'S' { -1.0 } else { 1.0 }));
    tags.push(mk_gps_lon(lon * if lon_ref == b'W' { -1.0 } else { 1.0 }));
    tags.push(mk_gps_spd(spd));
    tags.push(mk_gps_trk(trk));

    true
}

// ──── freeGPS Vantrue S1 (Type 10) ────

fn process_freegps_vantrue_s1(data: &[u8], tags: &mut Vec<Tag>) -> bool {
    if data.len() < 0x70 {
        return false;
    }
    let lat_ref = data[65];
    let lon_ref = data[66];

    let yr = get_u32_le(data, 68);
    let mon = get_u32_le(data, 72);
    let day = get_u32_le(data, 76);
    let hr = get_u32_le(data, 80);
    let min = get_u32_le(data, 84);
    let sec = get_u32_le(data, 88);

    if !(1..=12).contains(&mon) || !(1..=31).contains(&day) {
        return false;
    }

    let lon = get_f32_le(data, 0x5c) as f64;
    let lat = get_f32_le(data, 0x60) as f64;
    let spd = get_f32_le(data, 0x64) as f64 * KNOTS_TO_KPH;
    let trk = get_f32_le(data, 0x68) as f64;
    let alt = get_f32_le(data, 0x6c) as f64;

    tags.push(mk_gps_dt(&format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}Z",
        yr, mon, day, hr, min, sec
    )));
    tags.push(mk_gps_lat(lat * if lat_ref == b'S' { -1.0 } else { 1.0 }));
    tags.push(mk_gps_lon(lon * if lon_ref == b'W' { -1.0 } else { 1.0 }));
    tags.push(mk_gps_spd(spd));
    tags.push(mk_gps_trk(trk));
    tags.push(mk_gps_alt(alt));

    true
}

// ──── freeGPS Type 12: double lat/lon ────

fn process_freegps_type12(data: &[u8], tags: &mut Vec<Tag>) -> bool {
    if data.len() < 0x88 {
        return false;
    }
    let lat_ref = data[72];
    let lon_ref = data[88];

    let hr = get_u32_le(data, 48);
    let min = get_u32_le(data, 52);
    let sec = get_u32_le(data, 56);
    let yr = get_u32_le(data, 0x70);
    let mon = get_u32_le(data, 0x74);
    let day = get_u32_le(data, 0x78);

    if !(1..=12).contains(&mon) {
        return false;
    }

    let full_yr = if yr < 2000 { yr + 2000 } else { yr };

    let lat = get_f64_le(data, 0x40);
    let lon = get_f64_le(data, 0x50);
    let spd = get_f64_le(data, 0x60) * KNOTS_TO_KPH;
    let trk = get_f64_le(data, 0x68);

    let (lat_dd, lon_dd) = convert_lat_lon(lat, lon);

    tags.push(mk_gps_dt(&format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}Z",
        full_yr, mon, day, hr, min, sec
    )));
    tags.push(mk_gps_lat(
        lat_dd * if lat_ref == b'S' { -1.0 } else { 1.0 },
    ));
    tags.push(mk_gps_lon(
        lon_dd * if lon_ref == b'W' { -1.0 } else { 1.0 },
    ));
    tags.push(mk_gps_spd(spd));
    tags.push(mk_gps_trk(trk));

    true
}

// ──── freeGPS INNOVV (Type 13) ────

fn process_freegps_innovv(data: &[u8], tags: &mut Vec<Tag>) -> bool {
    // Multiple 32-byte records starting at offset 16: A[NS][EW]\0 + 28 bytes
    let mut pos = 16;
    let mut found = false;
    while pos + 32 <= data.len() {
        if data[pos] != b'A' || !is_ns(data[pos + 1]) || !is_ew(data[pos + 2]) || data[pos + 3] != 0
        {
            break;
        }
        let lat_ref = data[pos + 1];
        let lon_ref = data[pos + 2];
        let lat = get_f32_le(data, pos + 4).abs() as f64;
        let lon = get_f32_le(data, pos + 8).abs() as f64;
        let spd = get_f32_le(data, pos + 12) as f64 * KNOTS_TO_KPH;
        let trk = get_f32_le(data, pos + 16) as f64;

        let (lat_dd, lon_dd) = convert_lat_lon(lat, lon);
        tags.push(mk_gps_lat(
            lat_dd * if lat_ref == b'S' { -1.0 } else { 1.0 },
        ));
        tags.push(mk_gps_lon(
            lon_dd * if lon_ref == b'W' { -1.0 } else { 1.0 },
        ));
        tags.push(mk_gps_spd(spd));
        tags.push(mk_gps_trk(trk));
        found = true;
        pos += 32;
    }
    found
}

// ──── freeGPS Vantrue N4 (Type 15) ────

fn process_freegps_vantrue_n4(data: &[u8], tags: &mut Vec<Tag>) -> bool {
    if data.len() < 80 {
        return false;
    }
    let lat_ref = data[40];
    let lon_ref = data[56];

    let hr = get_u32_le(data, 16);
    let min = get_u32_le(data, 20);
    let sec = get_u32_le(data, 24);

    // yr/mon/day at offset 80..92
    if data.len() < 92 {
        return false;
    }
    let yr = get_u32_le(data, 80);
    let mon = get_u32_le(data, 84);
    let day = get_u32_le(data, 88);

    if !(1..=12).contains(&mon) {
        return false;
    }

    let lat = get_f64_le(data, 32).abs();
    let lon = get_f64_le(data, 48).abs();
    let spd = get_f64_le(data, 64) * KNOTS_TO_KPH;
    let trk = get_f64_le(data, 72);

    tags.push(mk_gps_dt(&format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}Z",
        yr, mon, day, hr, min, sec
    )));
    tags.push(mk_gps_lat(lat * if lat_ref == b'S' { -1.0 } else { 1.0 }));
    tags.push(mk_gps_lon(lon * if lon_ref == b'W' { -1.0 } else { 1.0 }));
    tags.push(mk_gps_spd(spd));
    tags.push(mk_gps_trk(trk));

    true
}

// ──── freeGPS Nextbase 512G binary (Type 20) ────

fn process_freegps_nextbase_binary(data: &[u8], tags: &mut Vec<Tag>) -> bool {
    // 32-byte records at offset 0x32
    // Big endian!
    let mut pos = 0x32usize;
    let mut found = false;
    while pos + 0x1e <= data.len() {
        let spd_raw = get_u16_be(data, pos);
        let trk_raw = get_u16_be(data, pos + 2) as i16;
        let yr = get_u16_be(data, pos + 4);
        let mon = data[pos + 6];
        let day = data[pos + 7];
        let hr = data[pos + 8];
        let min = data[pos + 9];
        let sec10 = get_u16_be(data, pos + 10);

        if !(2000..=2200).contains(&yr)
            || !(1..=12).contains(&mon)
            || !(1..=31).contains(&day)
            || hr > 59
            || min > 59
            || sec10 > 600
        {
            break;
        }

        let lat_raw = get_u32_be(data, pos + 13);
        let lon_raw = get_u32_be(data, pos + 17);
        let lat = signed_u32(lat_raw) as f64 / 1e7;
        let lon = signed_u32(lon_raw) as f64 / 1e7;
        let mut trk = trk_raw as f64 / 100.0;
        if trk < 0.0 {
            trk += 360.0;
        }

        let time = format!(
            "{:04}:{:02}:{:02} {:02}:{:02}:{:04.1}Z",
            yr,
            mon,
            day,
            hr,
            min,
            sec10 as f64 / 10.0
        );
        tags.push(mk_gps_dt(&time));
        tags.push(mk_gps_lat(lat));
        tags.push(mk_gps_lon(lon));
        tags.push(mk_gps_spd(spd_raw as f64 / 100.0 * MPS_TO_KPH));
        tags.push(mk_gps_trk(trk));
        found = true;

        pos += 0x20;
    }
    found
}

// ─── CAMM processor (Google Street View Camera Motion Metadata) ───

fn process_camm(data: &[u8], tags: &mut Vec<Tag>) -> bool {
    if data.len() < 4 {
        return false;
    }
    let camm_type = get_u16_le(data, 2);

    match camm_type {
        0 => {
            // AngleAxis: 3 floats at offset 4
            if data.len() >= 16 {
                let x = get_f32_le(data, 4);
                let y = get_f32_le(data, 8);
                let z = get_f32_le(data, 12);
                tags.push(mk_stream(
                    "AngleAxis",
                    "Angle Axis",
                    Value::String(format!("{} {} {}", x, y, z)),
                ));
                return true;
            }
        }
        2 => {
            // AngularVelocity: 3 floats at offset 4
            if data.len() >= 16 {
                let x = get_f32_le(data, 4);
                let y = get_f32_le(data, 8);
                let z = get_f32_le(data, 12);
                tags.push(mk_stream(
                    "AngularVelocity",
                    "Angular Velocity",
                    Value::String(format!("{} {} {}", x, y, z)),
                ));
                return true;
            }
        }
        3 => {
            // Acceleration: 3 floats at offset 4
            if data.len() >= 16 {
                let x = get_f32_le(data, 4);
                let y = get_f32_le(data, 8);
                let z = get_f32_le(data, 12);
                tags.push(mk_stream(
                    "Accelerometer",
                    "Accelerometer",
                    Value::String(format!("{} {} {}", x, y, z)),
                ));
                return true;
            }
        }
        5 => {
            // GPS: lat(double), lon(double), alt(double) at offsets 4,12,20
            if data.len() >= 28 {
                let lat = get_f64_le(data, 4);
                let lon = get_f64_le(data, 12);
                let alt = get_f64_le(data, 20);
                tags.push(mk_gps_lat(lat));
                tags.push(mk_gps_lon(lon));
                tags.push(mk_gps_alt(alt));
                return true;
            }
        }
        6 => {
            // Full GPS: timestamp(double), mode(u32), lat(double), lon(double), alt(float), etc.
            if data.len() >= 60 {
                let _timestamp = get_f64_le(data, 4);
                let lat = get_f64_le(data, 0x10);
                let lon = get_f64_le(data, 0x18);
                let alt = get_f32_le(data, 0x20) as f64;

                tags.push(mk_gps_lat(lat));
                tags.push(mk_gps_lon(lon));
                tags.push(mk_gps_alt(alt));

                if data.len() >= 0x38 {
                    let vel_east = get_f32_le(data, 0x2c);
                    let vel_north = get_f32_le(data, 0x30);
                    let speed =
                        ((vel_east * vel_east + vel_north * vel_north) as f64).sqrt() * MPS_TO_KPH;
                    tags.push(mk_gps_spd(speed));
                }
                return true;
            }
        }
        7 => {
            // MagneticField: 3 floats at offset 4
            if data.len() >= 16 {
                let x = get_f32_le(data, 4);
                let y = get_f32_le(data, 8);
                let z = get_f32_le(data, 12);
                tags.push(mk_stream(
                    "MagneticField",
                    "Magnetic Field",
                    Value::String(format!("{} {} {}", x, y, z)),
                ));
                return true;
            }
        }
        _ => {}
    }
    false
}

// ─── GoPro GPMF / gpmd processor ───

fn process_gpmd(data: &[u8], tags: &mut Vec<Tag>) -> bool {
    // GoPro GPMF uses KLV (Key-Length-Value) structure.
    // We look for GPS5 (lat, lon, alt, speed2d, speed3d) entries.
    process_gpmf_klv(data, 0, data.len(), tags)
}

fn process_gpmf_klv(data: &[u8], start: usize, end: usize, tags: &mut Vec<Tag>) -> bool {
    let mut pos = start;
    let mut found = false;

    while pos + 8 <= end {
        let fourcc = &data[pos..pos + 4];
        let type_byte = data[pos + 4];
        let size_byte = data[pos + 5];
        let repeat = get_u16_be(data, pos + 6) as usize;

        let struct_size = size_byte as usize;
        let total_data = struct_size * repeat;
        // Align to 4 bytes
        let padded = (total_data + 3) & !3;
        let data_start = pos + 8;

        if data_start + padded > end {
            break;
        }

        if type_byte == 0 && struct_size == 4 {
            // Container: recurse
            if process_gpmf_klv(data, data_start, data_start + total_data, tags) {
                found = true;
            }
        } else if fourcc == b"GPS5" && struct_size >= 20 && type_byte == b'l' {
            // GPS5: int32s[5] per sample: lat*1e7, lon*1e7, alt(cm), speed2d(cm/s), speed3d(cm/s)
            for i in 0..repeat {
                let off = data_start + i * struct_size;
                if off + 20 > end {
                    break;
                }
                let lat = get_i32_be(data, off) as f64 / 1e7;
                let lon = get_i32_be(data, off + 4) as f64 / 1e7;
                let alt = get_i32_be(data, off + 8) as f64 / 100.0;
                let speed2d = get_i32_be(data, off + 12) as f64 / 100.0 * MPS_TO_KPH;

                tags.push(mk_gps_lat(lat));
                tags.push(mk_gps_lon(lon));
                tags.push(mk_gps_alt(alt));
                tags.push(mk_gps_spd(speed2d));
                found = true;
            }
        } else if fourcc == b"GPSU" && type_byte == b'U' && total_data >= 16 {
            // GPS UTC time: "yymmddhhmmss.sss"
            if let Ok(s) = std::str::from_utf8(&data[data_start..data_start + total_data.min(16)]) {
                let s = s.trim_end_matches('\0');
                if s.len() >= 12 {
                    let dt = format!(
                        "20{}:{}:{} {}:{}:{}Z",
                        &s[0..2],
                        &s[2..4],
                        &s[4..6],
                        &s[6..8],
                        &s[8..10],
                        &s[10..]
                    );
                    tags.push(mk_gps_dt(&dt));
                    found = true;
                }
            }
        } else if fourcc == b"ACCL" && type_byte == b's' && struct_size >= 6 {
            // Accelerometer: int16s[3] per sample
            for i in 0..repeat.min(1) {
                let off = data_start + i * struct_size;
                if off + 6 > end {
                    break;
                }
                let x = get_i16_be(data, off) as f64 / 100.0;
                let y = get_i16_be(data, off + 2) as f64 / 100.0;
                let z = get_i16_be(data, off + 4) as f64 / 100.0;
                tags.push(mk_stream(
                    "Accelerometer",
                    "Accelerometer",
                    Value::String(format!("{:.4} {:.4} {:.4}", x, y, z)),
                ));
                found = true;
            }
        } else if fourcc == b"GYRO" && type_byte == b's' && struct_size >= 6 {
            // Gyroscope: int16s[3]
            for i in 0..repeat.min(1) {
                let off = data_start + i * struct_size;
                if off + 6 > end {
                    break;
                }
                let x = get_i16_be(data, off) as f64 / 100.0;
                let y = get_i16_be(data, off + 2) as f64 / 100.0;
                let z = get_i16_be(data, off + 4) as f64 / 100.0;
                tags.push(mk_stream(
                    "AngularVelocity",
                    "Angular Velocity",
                    Value::String(format!("{:.4} {:.4} {:.4}", x, y, z)),
                ));
                found = true;
            }
        }

        pos = data_start + padded;
    }

    found
}

// ─── mebx (Apple metadata keys) processor ───

fn process_mebx(data: &[u8], tags: &mut Vec<Tag>) -> bool {
    // mebx: size(4BE) + key(4) + data...
    let mut pos = 0;
    let mut found = false;
    while pos + 8 < data.len() {
        let len = get_u32_be(data, pos) as usize;
        if len < 8 || pos + len > data.len() {
            break;
        }
        let key = &data[pos + 4..pos + 8];
        let val_data = &data[pos + 8..pos + len];

        // Try to decode as UTF-8 string
        if let Ok(s) = std::str::from_utf8(val_data) {
            let key_str = crate::encoding::decode_utf8_or_latin1(key).to_string();
            let name = key_str.trim().to_string();
            if !name.is_empty() {
                tags.push(mk_stream(&name, &name, Value::String(s.trim().to_string())));
                found = true;
            }
        }
        pos += len;
    }
    found
}

// ─── tx3g subtitle processor ───

fn process_tx3g(data: &[u8], tags: &mut Vec<Tag>) -> bool {
    if data.len() < 2 {
        return false;
    }
    let text = crate::encoding::decode_utf8_or_latin1(&data[2..]); // skip 2-byte length word
    let text = text.trim();
    if text.is_empty() {
        return false;
    }

    // Autel Evo II drone: HOME(W: lon, N: lat) datetime
    if text.starts_with("HOME(") {
        return process_tx3g_autel(text, tags);
    }

    // Try key:value pairs
    let mut found = false;
    // Check for drone-style lat/lon pairs
    for line in text.lines() {
        let line = line.trim();
        // Simple key:value
        for cap in line.split_whitespace() {
            if let Some((k, v)) = cap.split_once(':') {
                match k {
                    "Lat" => {
                        if let Ok(val) = v.parse::<f64>() {
                            tags.push(mk_gps_lat(val));
                            found = true;
                        }
                    }
                    "Lon" => {
                        if let Ok(val) = v.parse::<f64>() {
                            tags.push(mk_gps_lon(val));
                            found = true;
                        }
                    }
                    "Alt" => {
                        if let Ok(val) = v.trim_end_matches('m').trim().parse::<f64>() {
                            tags.push(mk_gps_alt(val));
                            found = true;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    if !found {
        // Just store as text
        tags.push(mk_stream("Text", "Text", Value::String(text.to_string())));
        // Try NMEA in text
        let _ = parse_nmea_rmc(text, tags) || parse_nmea_gga(text, tags);
        found = true;
    }
    found
}

fn process_tx3g_autel(text: &str, tags: &mut Vec<Tag>) -> bool {
    let mut found = false;
    for line in text.lines() {
        let line = line.trim();
        // HOME(W: 109.318642, N: 40.769371) 2023-09-12 10:28:07
        if line.starts_with("HOME(") {
            // Parse lon/lat from HOME line
            if let Some(rest) = line.strip_prefix("HOME(") {
                if let Some(paren_end) = rest.find(')') {
                    let coords = &rest[..paren_end];
                    let after = rest[paren_end + 1..].trim();
                    // Parse two coord pairs
                    let parts: Vec<&str> = coords.split(',').collect();
                    if parts.len() == 2 {
                        for part in &parts {
                            let part = part.trim();
                            if let Some((dir, val_s)) = part.split_once(':') {
                                let dir = dir.trim();
                                let val_s = val_s.trim();
                                if let Ok(val) = val_s.parse::<f64>() {
                                    match dir {
                                        "N" | "S" => {
                                            let v = if dir == "S" { -val } else { val };
                                            tags.push(mk_stream(
                                                "GPSHomeLatitude",
                                                "GPS Home Latitude",
                                                Value::String(format!("{:.6}", v)),
                                            ));
                                            found = true;
                                        }
                                        "E" | "W" => {
                                            let v = if dir == "W" { -val } else { val };
                                            tags.push(mk_stream(
                                                "GPSHomeLongitude",
                                                "GPS Home Longitude",
                                                Value::String(format!("{:.6}", v)),
                                            ));
                                            found = true;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                    // datetime after parenthesis
                    if !after.is_empty() {
                        let dt = after.replace('-', ":");
                        tags.push(mk_gps_dt(&dt));
                        found = true;
                    }
                }
            }
        } else if line.starts_with("GPS(") {
            // GPS(W: 109.339287, N: 40.768574, 2371.76m)
            if let Some(rest) = line.strip_prefix("GPS(") {
                if let Some(paren_end) = rest.find(')') {
                    let inner = &rest[..paren_end];
                    let parts: Vec<&str> = inner.split(',').collect();
                    for part in &parts {
                        let part = part.trim();
                        if let Some((dir, val_s)) = part.split_once(':') {
                            let dir = dir.trim();
                            let val_s = val_s.trim();
                            if let Ok(val) = val_s.parse::<f64>() {
                                match dir {
                                    "N" | "S" => {
                                        let v = if dir == "S" { -val } else { val };
                                        tags.push(mk_gps_lat(v));
                                        found = true;
                                    }
                                    "E" | "W" => {
                                        let v = if dir == "W" { -val } else { val };
                                        tags.push(mk_gps_lon(v));
                                        found = true;
                                    }
                                    _ => {}
                                }
                            }
                        } else if part.ends_with('m') {
                            if let Ok(alt) = part.trim_end_matches('m').trim().parse::<f64>() {
                                tags.push(mk_gps_alt(alt));
                                found = true;
                            }
                        }
                    }
                }
            }
        }
    }
    found
}

// ─── NMEA processor ───

fn process_nmea(data: &[u8], tags: &mut Vec<Tag>) -> bool {
    let text = crate::encoding::decode_utf8_or_latin1(data);
    parse_nmea_rmc(&text, tags) || parse_nmea_gga(&text, tags)
}

fn parse_nmea_rmc(text: &str, tags: &mut Vec<Tag>) -> bool {
    // $GPRMC,HHMMSS.sss,A,DDMM.MMMM,N,DDDMM.MMMM,E,speed,track,DDMMYY,,,*CC
    // Find any xxRMC sentence
    let rmc_patterns = ["$GPRMC,", "$GNRMC,", "$GBRMC,"];
    for pat in &rmc_patterns {
        if let Some(start) = text.find(pat) {
            let rest = &text[start + pat.len()..];
            return parse_rmc_fields(rest, tags);
        }
    }
    false
}

fn parse_rmc_fields(rest: &str, tags: &mut Vec<Tag>) -> bool {
    let fields: Vec<&str> = rest.split(',').collect();
    if fields.len() < 12 {
        return false;
    }

    // fields[0]=time, [1]=status, [2]=lat, [3]=N/S, [4]=lon, [5]=E/W,
    // [6]=speed(knots), [7]=track, [8]=date(DDMMYY)
    let time_str = fields[0];
    let status = fields[1];
    if status != "A" && !status.is_empty() {
        // Accept empty status too (some devices)
    }
    let lat_str = fields[2];
    let lat_ref = fields[3];
    let lon_str = fields[4];
    let lon_ref = fields[5];
    let spd_str = fields[6];
    let trk_str = fields[7];
    let date_str = fields[8];

    // Parse lat/lon from DDMM.MMMM format
    let lat = match parse_nmea_coord(lat_str) {
        Some(v) => v * if lat_ref == "S" { -1.0 } else { 1.0 },
        None => return false,
    };
    let lon = match parse_nmea_coord(lon_str) {
        Some(v) => v * if lon_ref == "W" { -1.0 } else { 1.0 },
        None => return false,
    };

    // Parse date/time
    if date_str.len() >= 6 && time_str.len() >= 6 {
        let dd = &date_str[0..2];
        let mm = &date_str[2..4];
        let yy = &date_str[4..6];
        let yr: u32 = yy.parse().unwrap_or(0);
        let full_yr = if yr >= 70 { 1900 + yr } else { 2000 + yr };
        let time_part = if time_str.len() > 6 {
            &time_str[..6]
        } else {
            time_str
        };
        let dt = format!(
            "{:04}:{:02}:{:02} {}:{}:{}Z",
            full_yr,
            mm,
            dd,
            &time_part[0..2],
            &time_part[2..4],
            &time_part[4..6]
        );
        tags.push(mk_gps_dt(&dt));
    }

    tags.push(mk_gps_lat(lat));
    tags.push(mk_gps_lon(lon));

    if let Ok(spd) = spd_str.parse::<f64>() {
        tags.push(mk_gps_spd(spd * KNOTS_TO_KPH));
    }
    if let Ok(trk) = trk_str.parse::<f64>() {
        tags.push(mk_gps_trk(trk));
    }

    true
}

fn parse_nmea_gga(text: &str, tags: &mut Vec<Tag>) -> bool {
    let patterns = ["$GPGGA,", "$GNGGA,"];
    for pat in &patterns {
        if let Some(start) = text.find(pat) {
            let rest = &text[start + pat.len()..];
            let fields: Vec<&str> = rest.split(',').collect();
            if fields.len() < 10 {
                continue;
            }

            let lat_str = fields[1];
            let lat_ref = fields[2];
            let lon_str = fields[3];
            let lon_ref = fields[4];

            let lat = match parse_nmea_coord(lat_str) {
                Some(v) => v * if lat_ref == "S" { -1.0 } else { 1.0 },
                None => continue,
            };
            let lon = match parse_nmea_coord(lon_str) {
                Some(v) => v * if lon_ref == "W" { -1.0 } else { 1.0 },
                None => continue,
            };

            tags.push(mk_gps_lat(lat));
            tags.push(mk_gps_lon(lon));

            // Altitude at field 8
            if fields.len() > 8 {
                if let Ok(alt) = fields[8].parse::<f64>() {
                    tags.push(mk_gps_alt(alt));
                }
            }
            // Satellites at field 6
            if let Ok(sats) = fields[6].parse::<u32>() {
                tags.push(mk_stream(
                    "GPSSatellites",
                    "GPS Satellites",
                    Value::String(sats.to_string()),
                ));
            }
            return true;
        }
    }
    false
}

fn parse_nmea_coord(s: &str) -> Option<f64> {
    // Format: DDMM.MMMM or DDDMM.MMMM
    if s.is_empty() {
        return None;
    }
    let val: f64 = s.parse().ok()?;
    let deg = (val / 100.0).floor();
    let min = val - deg * 100.0;
    Some(deg + min / 60.0)
}

// ─── text track processor ───

fn process_text(data: &[u8], tags: &mut Vec<Tag>) -> bool {
    let text = crate::encoding::decode_utf8_or_latin1(data);
    let text = text.trim();
    if text.is_empty() {
        return false;
    }

    // Try NMEA
    if parse_nmea_rmc(text, tags) || parse_nmea_gga(text, tags) {
        return true;
    }

    // DJI telemetry: "F/3.5, SS 1000, ISO 100, EV 0, GPS (8.6499, 53.1665, 18), ..."
    if text.contains("GPS (") || text.contains("GPS(") {
        return process_dji_text(text, tags);
    }

    // Garmin PNDM format
    if data.len() >= 20
        && (data.starts_with(b"PNDM") || (data.len() > 4 && &data[4..8.min(data.len())] == b"PNDM"))
    {
        return process_garmin_pndm(data, tags);
    }

    false
}

fn process_dji_text(text: &str, tags: &mut Vec<Tag>) -> bool {
    // GPS (lon, lat, alt)
    let gps_start = text.find("GPS (").or_else(|| text.find("GPS("));
    if let Some(idx) = gps_start {
        let rest = &text[idx..];
        if let Some(paren_start) = rest.find('(') {
            if let Some(paren_end) = rest.find(')') {
                let inner = &rest[paren_start + 1..paren_end];
                let parts: Vec<&str> = inner.split(',').collect();
                if parts.len() >= 2 {
                    if let (Ok(lon), Ok(lat)) = (
                        parts[0].trim().parse::<f64>(),
                        parts[1].trim().parse::<f64>(),
                    ) {
                        tags.push(mk_gps_lat(lat));
                        tags.push(mk_gps_lon(lon));
                        if parts.len() >= 3 {
                            if let Ok(alt) = parts[2].trim().parse::<f64>() {
                                tags.push(mk_gps_alt(alt));
                            }
                        }
                    }
                }
            }
        }
    }

    // H.S speed
    if let Some(idx) = text.find("H.S ") {
        let rest = &text[idx + 4..];
        if let Some(end) = rest.find("m/s") {
            if let Ok(spd) = rest[..end].trim().parse::<f64>() {
                tags.push(mk_gps_spd(spd * MPS_TO_KPH));
            }
        }
    }

    // ISO
    if let Some(idx) = text.find("ISO ") {
        let rest = &text[idx + 4..];
        let val: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !val.is_empty() {
            tags.push(mk_stream("ISO", "ISO", Value::String(val)));
        }
    }

    true
}

fn process_garmin_pndm(data: &[u8], tags: &mut Vec<Tag>) -> bool {
    let offset = if data.starts_with(b"PNDM") { 0 } else { 4 };
    if data.len() < offset + 20 {
        return false;
    }
    let lat = get_i32_be(data, offset + 12) as f64 * 180.0 / 0x80000000u32 as f64;
    let lon = get_i32_be(data, offset + 16) as f64 * 180.0 / 0x80000000u32 as f64;
    let spd = get_u16_be(data, offset + 8) as f64 * MPH_TO_KPH;

    tags.push(mk_gps_lat(lat));
    tags.push(mk_gps_lon(lon));
    tags.push(mk_gps_spd(spd));
    true
}

// ─── RVMI processor ───

fn process_rvmi(data: &[u8], tags: &mut Vec<Tag>) -> bool {
    if data.len() < 20 {
        return false;
    }
    if &data[0..4] == b"gReV" {
        // GPS data
        let lat = get_i32_le(data, 4) as f64 / 1e6;
        let lon = get_i32_le(data, 8) as f64 / 1e6;
        let spd = get_i16_le(data, 16) as f64 / 10.0;
        let trk = get_u16_le(data, 18) as f64 * 2.0;
        tags.push(mk_gps_lat(lat));
        tags.push(mk_gps_lon(lon));
        tags.push(mk_gps_spd(spd));
        tags.push(mk_gps_trk(trk));
        return true;
    }
    if &data[0..4] == b"sReV" {
        // G-sensor data
        if data.len() >= 10 {
            let x = get_i16_le(data, 4) as f64 / 1000.0;
            let y = get_i16_le(data, 6) as f64 / 1000.0;
            let z = get_i16_le(data, 8) as f64 / 1000.0;
            tags.push(mk_stream(
                "GSensor",
                "G Sensor",
                Value::String(format!("{} {} {}", x, y, z)),
            ));
            return true;
        }
    }
    false
}

// ─── Kenwood processor ───

fn process_kenwood(data: &[u8], tags: &mut Vec<Tag>) -> bool {
    // Look for \xfe\xfe markers followed by GPS data
    let mut found = false;
    let mut pos = 0;
    while pos + 2 < data.len() {
        // Find \xfe\xfe
        if let Some(idx) = data[pos..].windows(2).position(|w| w == b"\xfe\xfe") {
            let start = pos + idx + 2;
            if start + 40 > data.len() {
                break;
            }
            let dat = &data[start..];
            // YYYYMMDDHHMMSS (14 bytes) + . + YYYYMMDDHHMMSS (14 bytes) + . + [NS]digits[EW]digits...
            if let Some(dt) = try_ascii_digits(dat, 14) {
                if dt.len() == 14 {
                    let time = format!(
                        "{}:{}:{} {}:{}:{}",
                        &dt[0..4],
                        &dt[4..6],
                        &dt[6..8],
                        &dt[8..10],
                        &dt[10..12],
                        &dt[12..14]
                    );

                    // Skip past second datetime + separator
                    let after = &dat[15..]; // skip first 14 + separator
                    if after.len() < 20 {
                        pos = start + 14;
                        continue;
                    }
                    // Skip second date (14 digits + separator)
                    let after2 = if after.len() > 15 {
                        &after[15..]
                    } else {
                        after
                    };

                    // [NS]digits[EW]digits
                    if !after2.is_empty() && is_ns(after2[0]) {
                        let lat_ref = after2[0];
                        // Find E or W
                        let mut ew_pos = 1;
                        while ew_pos < after2.len() && !is_ew(after2[ew_pos]) {
                            ew_pos += 1;
                        }
                        if ew_pos < after2.len() {
                            let lon_ref = after2[ew_pos];
                            let lat_digits = &after2[1..ew_pos];
                            // Find end of lon digits
                            let lon_start = ew_pos + 1;
                            let mut lon_end = lon_start;
                            while lon_end < after2.len() && after2[lon_end].is_ascii_digit() {
                                lon_end += 1;
                            }
                            let lon_digits = &after2[lon_start..lon_end];

                            if let (Ok(lat_s), Ok(lon_s)) = (
                                std::str::from_utf8(lat_digits),
                                std::str::from_utf8(lon_digits),
                            ) {
                                if let (Ok(lat_raw), Ok(lon_raw)) =
                                    (lat_s.parse::<f64>(), lon_s.parse::<f64>())
                                {
                                    let lat = lat_raw / 1e4;
                                    let lon = lon_raw / 1e4;
                                    let (lat_dd, lon_dd) = convert_lat_lon(lat, lon);

                                    tags.push(mk_gps_dt(&time));
                                    tags.push(mk_gps_lat(
                                        lat_dd * if lat_ref == b'S' { -1.0 } else { 1.0 },
                                    ));
                                    tags.push(mk_gps_lon(
                                        lon_dd * if lon_ref == b'W' { -1.0 } else { 1.0 },
                                    ));
                                    found = true;

                                    // Try altitude and speed after lon
                                    if lon_end + 9 <= after2.len() {
                                        if let Ok(rest) =
                                            std::str::from_utf8(&after2[lon_end..lon_end + 9])
                                        {
                                            // +AAAA0SS (altitude+speed)
                                            if rest.starts_with('+') || rest.starts_with('-') {
                                                if let Ok(alt) = rest[0..5].parse::<f64>() {
                                                    tags.push(mk_gps_alt(alt));
                                                }
                                                if let Ok(spd) = rest[5..].parse::<f64>() {
                                                    tags.push(mk_gps_spd(spd));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            pos = start + 40;
        } else {
            break;
        }
    }
    found
}

// ─── mdat scan for freeGPS ───

fn scan_mdat_for_freegps(data: &[u8], tags: &mut Vec<Tag>, doc_count: &mut u32) {
    // Look for "\0..\0freeGPS " pattern in mdat region
    let pattern = b"freeGPS ";
    let mut pos = 0;
    let limit = data.len().min(20_000_000); // limit scan to first 20MB

    while pos + 12 < limit {
        if let Some(idx) = data[pos..limit].windows(8).position(|w| w == pattern) {
            let abs_pos = pos + idx;
            // freeGPS header: 4 bytes before "freeGPS " is the atom size
            if abs_pos >= 4 {
                let atom_start = abs_pos - 4;
                let atom_size = u32::from_be_bytes([
                    data[atom_start],
                    data[atom_start + 1],
                    data[atom_start + 2],
                    data[atom_start + 3],
                ]) as usize;
                let atom_size = if atom_size < 12 { 12 } else { atom_size };
                let end = (atom_start + atom_size).min(data.len());
                let block = &data[atom_start..end];

                let mut sample_tags = Vec::new();
                if process_freegps(block, &mut sample_tags) && !sample_tags.is_empty() {
                    *doc_count += 1;
                    for t in &mut sample_tags {
                        t.description = format!("{} (Doc{})", t.description, doc_count);
                    }
                    tags.extend(sample_tags);
                }
                pos = end;
            } else {
                pos = abs_pos + 8;
            }
        } else {
            break;
        }
    }
}

// ─── helpers ───

fn is_ns(b: u8) -> bool {
    b == b'N' || b == b'S'
}
fn is_ew(b: u8) -> bool {
    b == b'E' || b == b'W'
}

/// Convert DDDMM.MMMM to decimal degrees
fn convert_lat_lon(lat: f64, lon: f64) -> (f64, f64) {
    let lat_deg = (lat / 100.0).floor();
    let lat_dd = lat_deg + (lat - lat_deg * 100.0) / 60.0;
    let lon_deg = (lon / 100.0).floor();
    let lon_dd = lon_deg + (lon - lon_deg * 100.0) / 60.0;
    (lat_dd, lon_dd)
}

fn signed_u32(v: u32) -> i32 {
    v as i32 // wraps correctly in Rust for values >= 0x80000000
}

fn get_u16_be(data: &[u8], off: usize) -> u16 {
    u16::from_be_bytes([data[off], data[off + 1]])
}

fn get_u16_le(data: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([data[off], data[off + 1]])
}

fn get_u32_be(data: &[u8], off: usize) -> u32 {
    u32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

fn get_u32_le(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

fn get_i32_be(data: &[u8], off: usize) -> i32 {
    i32::from_be_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

fn get_i32_le(data: &[u8], off: usize) -> i32 {
    i32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

fn get_i16_be(data: &[u8], off: usize) -> i16 {
    i16::from_be_bytes([data[off], data[off + 1]])
}

fn get_i16_le(data: &[u8], off: usize) -> i16 {
    i16::from_le_bytes([data[off], data[off + 1]])
}

fn get_f32_le(data: &[u8], off: usize) -> f32 {
    f32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

fn get_f64_le(data: &[u8], off: usize) -> f64 {
    f64::from_le_bytes([
        data[off],
        data[off + 1],
        data[off + 2],
        data[off + 3],
        data[off + 4],
        data[off + 5],
        data[off + 6],
        data[off + 7],
    ])
}

fn try_ascii_digits(data: &[u8], max_len: usize) -> Option<String> {
    let end = data.len().min(max_len);
    let slice = &data[..end];
    if slice.iter().all(|b| b.is_ascii_digit()) {
        Some(crate::encoding::decode_utf8_or_latin1(slice).to_string())
    } else {
        None
    }
}

// ─── tag builders ───

fn mk_stream(name: &str, description: &str, value: Value) -> Tag {
    let print_value = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "QuickTime".into(),
            family1: "QuickTime".into(),
            family2: "Location".into(),
        },
        raw_value: value,
        print_value,
        priority: 0,
    }
}

fn mk_gps_dt(dt: &str) -> Tag {
    Tag {
        id: TagId::Text("GPSDateTime".into()),
        name: "GPSDateTime".into(),
        description: "GPS Date/Time".into(),
        group: TagGroup {
            family0: "QuickTime".into(),
            family1: "QuickTime".into(),
            family2: "Time".into(),
        },
        raw_value: Value::String(dt.to_string()),
        print_value: dt.to_string(),
        priority: 0,
    }
}

fn mk_gps_lat(val: f64) -> Tag {
    let abs_val = val.abs();
    let d = abs_val.floor() as u32;
    let m_total = (abs_val - d as f64) * 60.0;
    let m = m_total.floor() as u32;
    let s = (m_total - m as f64) * 60.0;
    let ref_c = if val >= 0.0 { "N" } else { "S" };
    let print = format!("{} deg {}' {:.2}\" {}", d, m, s, ref_c);
    Tag {
        id: TagId::Text("GPSLatitude".into()),
        name: "GPSLatitude".into(),
        description: "GPS Latitude".into(),
        group: TagGroup {
            family0: "QuickTime".into(),
            family1: "QuickTime".into(),
            family2: "Location".into(),
        },
        raw_value: Value::F64(val),
        print_value: print,
        priority: 0,
    }
}

fn mk_gps_lon(val: f64) -> Tag {
    let abs_val = val.abs();
    let d = abs_val.floor() as u32;
    let m_total = (abs_val - d as f64) * 60.0;
    let m = m_total.floor() as u32;
    let s = (m_total - m as f64) * 60.0;
    let ref_c = if val >= 0.0 { "E" } else { "W" };
    let print = format!("{} deg {}' {:.2}\" {}", d, m, s, ref_c);
    Tag {
        id: TagId::Text("GPSLongitude".into()),
        name: "GPSLongitude".into(),
        description: "GPS Longitude".into(),
        group: TagGroup {
            family0: "QuickTime".into(),
            family1: "QuickTime".into(),
            family2: "Location".into(),
        },
        raw_value: Value::F64(val),
        print_value: print,
        priority: 0,
    }
}

fn mk_gps_alt(val: f64) -> Tag {
    Tag {
        id: TagId::Text("GPSAltitude".into()),
        name: "GPSAltitude".into(),
        description: "GPS Altitude".into(),
        group: TagGroup {
            family0: "QuickTime".into(),
            family1: "QuickTime".into(),
            family2: "Location".into(),
        },
        raw_value: Value::F64(val),
        print_value: format!("{:.4} m", val),
        priority: 0,
    }
}

fn mk_gps_spd(val: f64) -> Tag {
    Tag {
        id: TagId::Text("GPSSpeed".into()),
        name: "GPSSpeed".into(),
        description: "GPS Speed".into(),
        group: TagGroup {
            family0: "QuickTime".into(),
            family1: "QuickTime".into(),
            family2: "Location".into(),
        },
        raw_value: Value::F64(val),
        print_value: format!("{:.4}", val),
        priority: 0,
    }
}

fn mk_gps_trk(val: f64) -> Tag {
    Tag {
        id: TagId::Text("GPSTrack".into()),
        name: "GPSTrack".into(),
        description: "GPS Track".into(),
        group: TagGroup {
            family0: "QuickTime".into(),
            family1: "QuickTime".into(),
            family2: "Location".into(),
        },
        raw_value: Value::F64(val),
        print_value: format!("{:.4}", val),
        priority: 0,
    }
}
