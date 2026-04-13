//! M2TS (MPEG-2 Transport Stream) format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

// --- M2TS bit reader for SPS parsing ---
struct M2tsBitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_pos: u8,
    current: u8,
}

impl<'a> M2tsBitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        let (byte_pos, bit_pos, current) = if data.is_empty() {
            (0, 0, 0)
        } else {
            (1, 8, data[0])
        };
        M2tsBitReader {
            data,
            byte_pos,
            bit_pos,
            current,
        }
    }

    fn read_bit(&mut self) -> Option<u32> {
        if self.bit_pos == 0 {
            if self.byte_pos >= self.data.len() {
                return None;
            }
            self.current = self.data[self.byte_pos];
            self.byte_pos += 1;
            self.bit_pos = 8;
        }
        self.bit_pos -= 1;
        Some(((self.current >> self.bit_pos) & 1) as u32)
    }

    fn read_bits(&mut self, n: u32) -> Option<u32> {
        let mut val = 0u32;
        for _ in 0..n {
            val = (val << 1) | self.read_bit()?;
        }
        Some(val)
    }

    fn skip_bits(&mut self, n: u32) {
        for _ in 0..n {
            let _ = self.read_bit();
        }
    }

    fn read_ue(&mut self) -> Option<u32> {
        let mut leading = 0u32;
        while self.read_bit()? == 0 {
            leading += 1;
            if leading > 31 {
                return None;
            }
        }
        // After while loop, the '1' terminator bit was consumed.
        // Now read 'leading' INFO bits.
        let mut info = 0u32;
        for _ in 0..leading {
            info = (info << 1) | self.read_bit()?;
        }
        Some((1 << leading) + info - 1)
    }

    fn read_se(&mut self) -> Option<i32> {
        let ue = self.read_ue()?;
        let abs_val = ((ue + 1) >> 1) as i32;
        Some(if ue & 1 != 0 { abs_val } else { -abs_val })
    }
}

/// MDPM (Modified DV Pack Metadata) data extracted from H.264 SEI unregistered user data
#[derive(Clone)]
struct M2tsMdpmData {
    datetime_original: Option<String>,
    aperture_setting: Option<String>,
    gain: Option<String>,
    image_stabilization: Option<String>,
    exposure_time: Option<String>,
    shutter_speed: Option<String>,
    make: Option<String>,
    recording_mode: Option<String>,
}

/// Parse SEI NAL unit (type 6) from H.264 and extract MDPM camera metadata.
/// UUID: 17ee8c60-f84d-11d9-8cd6-0800200c9a66 + "MDPM"
fn m2ts_parse_sei(nal_data: &[u8]) -> Option<M2tsMdpmData> {
    // Remove emulation prevention bytes (0x000003 -> 0x0000)
    let mut rbsp = Vec::with_capacity(nal_data.len());
    let mut i = 0;
    while i < nal_data.len() {
        if i + 2 < nal_data.len()
            && nal_data[i] == 0
            && nal_data[i + 1] == 0
            && nal_data[i + 2] == 3
        {
            rbsp.push(0);
            rbsp.push(0);
            i += 3;
        } else {
            rbsp.push(nal_data[i]);
            i += 1;
        }
    }

    let data = &rbsp;
    let end = data.len();
    let mut pos = 1; // skip nal_unit_type byte (0x06)

    // Scan SEI payloads
    while pos < end {
        // Read payload type (extended via 0xFF bytes)
        let mut sei_type: u32 = 0;
        loop {
            if pos >= end {
                return None;
            }
            let t = data[pos];
            pos += 1;
            sei_type += t as u32;
            if t != 0xFF {
                break;
            }
        }
        if sei_type == 0x80 {
            return None;
        } // terminator

        // Read payload size
        let mut sei_size: usize = 0;
        loop {
            if pos >= end {
                return None;
            }
            let t = data[pos];
            pos += 1;
            sei_size += t as usize;
            if t != 0xFF {
                break;
            }
        }
        if pos + sei_size > end {
            return None;
        }

        if sei_type == 5 {
            // Unregistered user data: check for MDPM UUID
            // UUID bytes: 17 ee 8c 60 f8 4d 11 d9 8c d6 08 00 20 0c 9a 66
            // followed by "MDPM" (4 bytes)
            let payload = &data[pos..pos + sei_size];
            if sei_size > 20 {
                let uuid_mdpm =
                    b"\x17\xee\x8c\x60\xf8\x4d\x11\xd9\x8c\xd6\x08\x00\x20\x0c\x9a\x66MDPM";
                if payload.len() >= 20 && &payload[..20] == uuid_mdpm {
                    return m2ts_parse_mdpm(&payload[20..]);
                }
            }
        }

        pos += sei_size;
    }
    None
}

/// Parse MDPM entries and decode camera metadata tags.
fn m2ts_parse_mdpm(data: &[u8]) -> Option<M2tsMdpmData> {
    if data.is_empty() {
        return None;
    }

    let mut result = M2tsMdpmData {
        datetime_original: None,
        aperture_setting: None,
        gain: None,
        image_stabilization: None,
        exposure_time: None,
        shutter_speed: None,
        make: None,
        recording_mode: None,
    };

    let num = data[0] as usize;
    let mut pos = 1;
    let end = data.len();
    let mut last_tag: u8 = 0;
    let mut index = 0;

    while index < num && pos + 5 <= end {
        let tag = data[pos];
        if tag <= last_tag && index > 0 {
            break;
        } // out of sequence
        last_tag = tag;

        let val4 = [data[pos + 1], data[pos + 2], data[pos + 3], data[pos + 4]];
        pos += 5;
        index += 1;

        match tag {
            0x18 => {
                // DateTimeOriginal: combine with next tag (0x19)
                // Read 4 bytes from current tag, then peek at next tag
                let mut combined = val4.to_vec();
                if pos + 5 <= end && data[pos] == 0x19 {
                    combined.extend_from_slice(&data[pos + 1..pos + 5]);
                    pos += 5;
                    index += 1;
                    last_tag = 0x19;
                }
                // combined = [tz, yy_high, yy_low, mm, dd, HH, MM, SS] (BCD / raw)
                // ExifTool ValueConv: my ($tz, @a) = unpack('C*',$val);
                // sprintf('%.2x%.2x:%.2x:%.2x %.2x:%.2x:%.2x%s%.2d:%s%s', @a, ...)
                if combined.len() >= 8 {
                    let tz = combined[0];
                    let yh = combined[1]; // year high byte
                    let yl = combined[2]; // year low byte
                    let mo = combined[3];
                    let dy = combined[4];
                    let hh = combined[5];
                    let mm = combined[6];
                    let ss = combined[7];
                    let sign = if tz & 0x20 != 0 { '-' } else { '+' };
                    let tz_h = (tz >> 1) & 0x0f;
                    let tz_m = if tz & 0x01 != 0 { "30" } else { "00" };
                    let dst = if tz & 0x40 != 0 { " DST" } else { "" };
                    let s = format!(
                        "{:02x}{:02x}:{:02x}:{:02x} {:02x}:{:02x}:{:02x}{}{:02}:{}{}",
                        yh, yl, mo, dy, hh, mm, ss, sign, tz_h, tz_m, dst
                    );
                    result.datetime_original = Some(s);
                }
            }
            0x70 => {
                // Camera1: byte 0 = ApertureSetting, byte 1 = Gain (low nibble) + ExposureProgram (high nibble)
                let aperture_raw = val4[0];
                let aperture = match aperture_raw {
                    0xFF => "Auto".to_string(),
                    0xFE => "Closed".to_string(),
                    v => format!("{:.1}", 2f64.powf((v & 0x3f) as f64 / 8.0)),
                };
                result.aperture_setting = Some(aperture);

                let gain_raw = val4[1] & 0x0f;
                let gain_val = (gain_raw as i32 - 1) * 3;
                result.gain = if gain_val == 42 {
                    Some("Out of range".to_string())
                } else {
                    Some(format!("{} dB", gain_val))
                };
            }
            0x71 => {
                // Camera2: byte 1 = ImageStabilization
                let is_raw = val4[1];
                let is_str = match is_raw {
                    0x00 => "Off".to_string(),
                    0x3F => "On (0x3f)".to_string(),
                    0xBF => "Off (0xbf)".to_string(),
                    0xFF => "n/a".to_string(),
                    v => {
                        let state = if v & 0x10 != 0 { "On" } else { "Off" };
                        format!("{} (0x{:02x})", state, v)
                    }
                };
                result.image_stabilization = Some(is_str);
            }
            0x7F => {
                // Shutter: int16u little-endian, tag 1.1 mask 0x7fff = ExposureTime
                let val_le = u16::from_le_bytes([val4[0], val4[1]]);
                let val_le2 = u16::from_le_bytes([val4[2], val4[3]]);
                let shutter_raw = val_le2 & 0x7fff;
                let _ = val_le; // word 0 unused
                if shutter_raw != 0x7fff {
                    let exp_f = shutter_raw as f64 / 28125.0;
                    // Format as fraction using ExifTool::Exif::PrintExposureTime logic
                    let et_str = m2ts_format_exposure_time(exp_f);
                    result.exposure_time = Some(et_str.clone());
                    result.shutter_speed = Some(et_str);
                }
            }
            0xE0 => {
                // MakeModel: int16u[0] = Make code
                let make_code = u16::from_be_bytes([val4[0], val4[1]]);
                let make_str = match make_code {
                    0x0103 => "Panasonic",
                    0x0108 => "Sony",
                    0x1011 => "Canon",
                    0x1104 => "JVC",
                    _ => "Unknown",
                };
                result.make = Some(make_str.to_string());
            }
            0xE1 => {
                // RecInfo (Canon): int8u[0] = RecordingMode
                let rec_mode = val4[0];
                let mode_str = match rec_mode {
                    0x02 => "XP+",
                    0x04 => "SP",
                    0x05 => "LP",
                    0x06 => "FXP",
                    0x07 => "MXP",
                    _ => "Unknown",
                };
                result.recording_mode = Some(mode_str.to_string());
            }
            _ => {}
        }
    }

    if result.datetime_original.is_some()
        || result.aperture_setting.is_some()
        || result.gain.is_some()
        || result.make.is_some()
    {
        Some(result)
    } else {
        None
    }
}

/// Format exposure time like ExifTool's PrintExposureTime
fn m2ts_format_exposure_time(val: f64) -> String {
    if val <= 0.0 {
        return "0".to_string();
    }
    if val >= 1.0 {
        if (val - val.round()).abs() < 0.005 {
            return format!("{}", val.round() as i64);
        }
        return format!("{:.1}", val);
    }
    // Express as fraction 1/N
    let n = (1.0 / val).round() as i64;
    if n > 0 {
        format!("1/{}", n)
    } else {
        format!("{}", val)
    }
}

fn m2ts_find_packet_size(data: &[u8]) -> Option<(usize, usize)> {
    for &(pkt, tco) in &[(192usize, 4usize), (188, 0)] {
        if data.len() >= pkt * 3 && (0..3).all(|i| data[i * pkt + tco] == 0x47) {
            return Some((pkt, tco));
        }
    }
    None
}

fn m2ts_get_payload(pkt: &[u8], tco: usize) -> Option<(bool, u16, &[u8])> {
    if pkt.len() < tco + 4 {
        return None;
    }
    let hdr = &pkt[tco..];
    if hdr[0] != 0x47 {
        return None;
    }
    let pusi = (hdr[1] & 0x40) != 0;
    let pid = (((hdr[1] & 0x1F) as u16) << 8) | hdr[2] as u16;
    let afc = (hdr[3] >> 4) & 0x3;
    if afc == 0 || afc == 2 {
        return None;
    }
    let mut ps = 4;
    if afc == 3 {
        if hdr.len() <= ps {
            return None;
        }
        ps += 1 + hdr[ps] as usize;
    }
    if ps >= hdr.len() {
        return None;
    }
    Some((pusi, pid, &hdr[ps..]))
}

fn m2ts_parse_pat(section: &[u8]) -> Vec<u16> {
    let mut pmt_pids = Vec::new();
    if section.len() < 8 {
        return pmt_pids;
    }
    let section_length = (((section[1] & 0x0F) as usize) << 8) | section[2] as usize;
    let entries_end = (3 + section_length).saturating_sub(4).min(section.len());
    let mut i = 8;
    while i + 4 <= entries_end {
        let prog_num = ((section[i] as u16) << 8) | section[i + 1] as u16;
        let pmt_pid = (((section[i + 2] & 0x1F) as u16) << 8) | section[i + 3] as u16;
        if prog_num != 0 {
            pmt_pids.push(pmt_pid);
        }
        i += 4;
    }
    pmt_pids
}

struct M2tsStreamInfo {
    video_type: Option<String>,
    audio_type: Option<String>,
    audio_bitrate_idx: Option<u8>,
    audio_surround_mode: Option<u8>,
    audio_channels: Option<u8>,
    h264_pid: Option<u16>,
    audio_pid: Option<u16>,
}

fn m2ts_parse_pmt(section: &[u8]) -> Option<M2tsStreamInfo> {
    if section.len() < 12 || section[0] != 0x02 {
        return None;
    }
    let section_length = (((section[1] & 0x0F) as usize) << 8) | section[2] as usize;
    let section_end = (3 + section_length).saturating_sub(4).min(section.len());
    let prog_info_len = (((section[10] & 0x0F) as usize) << 8) | section[11] as usize;
    let mut es_pos = 12 + prog_info_len;
    if es_pos >= section_end {
        return None;
    }

    let mut info = M2tsStreamInfo {
        video_type: None,
        audio_type: None,
        audio_bitrate_idx: None,
        audio_surround_mode: None,
        audio_channels: None,
        h264_pid: None,
        audio_pid: None,
    };

    while es_pos + 5 <= section_end {
        let stream_type = section[es_pos];
        let es_pid = (((section[es_pos + 1] & 0x1F) as u16) << 8) | section[es_pos + 2] as u16;
        let es_info_len =
            (((section[es_pos + 3] & 0x0F) as usize) << 8) | section[es_pos + 4] as usize;
        let es_info_end = (es_pos + 5 + es_info_len).min(section_end);

        match stream_type {
            0x01 | 0x02 if info.video_type.is_none() => {
                info.video_type = Some(m2ts_stream_type_name(stream_type).to_string());
            }
            0x10 if info.video_type.is_none() => {
                info.video_type = Some(m2ts_stream_type_name(stream_type).to_string());
            }
            0x1b if info.video_type.is_none() => {
                info.video_type = Some("H.264 (AVC) Video".to_string());
                info.h264_pid = Some(es_pid);
            }
            0x24 if info.video_type.is_none() => {
                info.video_type = Some(m2ts_stream_type_name(stream_type).to_string());
            }
            0x03 | 0x04 if info.audio_type.is_none() => {
                info.audio_type = Some(m2ts_stream_type_name(stream_type).to_string());
                info.audio_pid = Some(es_pid);
            }
            0x0f if info.audio_type.is_none() => {
                info.audio_type = Some(m2ts_stream_type_name(stream_type).to_string());
                info.audio_pid = Some(es_pid);
            }
            0x81 if info.audio_type.is_none() => {
                info.audio_type = Some("A52/AC-3 Audio".to_string());
                info.audio_pid = Some(es_pid);
                // Parse AC3 audio descriptor from ES info
                let mut di = es_pos + 5;
                while di + 2 <= es_info_end {
                    let dtag = section[di];
                    let dlen = section[di + 1] as usize;
                    if di + 2 + dlen > es_info_end {
                        break;
                    }
                    if dtag == 0x81 && dlen >= 3 {
                        // AC3 audio descriptor per ATSC A/52
                        let d0 = section[di + 2];
                        let d1 = section[di + 3];
                        let d2 = section[di + 4];
                        info.audio_bitrate_idx = Some(d1 >> 2);
                        info.audio_surround_mode = Some(d1 & 0x03);
                        info.audio_channels = Some((d2 >> 1) & 0x0f);
                        let _ = d0; // sample_rate_idx from d0 >> 5 not used here
                    }
                    di += 2 + dlen;
                }
            }
            _ => {}
        }

        es_pos = es_info_end;
    }

    if info.video_type.is_some() || info.audio_type.is_some() {
        Some(info)
    } else {
        None
    }
}

fn m2ts_parse_sps(sps_nal: &[u8]) -> Option<(u32, u32)> {
    // Remove emulation prevention bytes
    let mut rbsp = Vec::with_capacity(sps_nal.len());
    let mut i = 0;
    while i < sps_nal.len() {
        if i + 2 < sps_nal.len() && sps_nal[i] == 0 && sps_nal[i + 1] == 0 && sps_nal[i + 2] == 3 {
            rbsp.push(0);
            rbsp.push(0);
            i += 3;
        } else {
            rbsp.push(sps_nal[i]);
            i += 1;
        }
    }

    let mut br = M2tsBitReader::new(&rbsp);
    br.skip_bits(8); // nal_unit_type byte
    let profile_idc = br.read_bits(8)?;
    br.skip_bits(16); // constraint_flags + level_idc
    br.read_ue()?; // seq_parameter_set_id

    if matches!(
        profile_idc,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128
    ) {
        let chroma = br.read_ue()?;
        if chroma == 3 {
            br.skip_bits(1);
        }
        br.read_ue()?;
        br.read_ue()?;
        br.skip_bits(1);
        let scaling = br.read_bit()?;
        if scaling != 0 {
            let count = if chroma != 3 { 8 } else { 12 };
            for ci in 0..count {
                if br.read_bit()? != 0 {
                    let sz = if ci < 6 { 16 } else { 64 };
                    let (mut last, mut next) = (8i32, 8i32);
                    for _ in 0..sz {
                        if next != 0 {
                            let d = br.read_se()?;
                            next = (last + d + 256) % 256;
                        }
                        last = if next == 0 { last } else { next };
                    }
                }
            }
        }
    }

    br.read_ue()?; // log2_max_frame_num_minus4
    let poc_type = br.read_ue()?;
    if poc_type == 0 {
        br.read_ue()?;
    } else if poc_type == 1 {
        br.skip_bits(1);
        br.read_se()?;
        br.read_se()?;
        let n = br.read_ue()?;
        for _ in 0..n {
            br.read_se()?;
        }
    }
    br.read_ue()?;
    br.skip_bits(1);

    let pic_w = br.read_ue()?;
    let pic_h = br.read_ue()?;
    let frame_mbs_only = br.read_bit()?;
    if frame_mbs_only == 0 {
        br.skip_bits(1);
    }
    br.skip_bits(1);

    let crop = br.read_bit()?;
    let (cl, cr, ct, cb) = if crop != 0 {
        (br.read_ue()?, br.read_ue()?, br.read_ue()?, br.read_ue()?)
    } else {
        (0, 0, 0, 0)
    };

    // Crop multiplier: 4 for width, (4 - frame_mbs_only*2) for height (Perl H264.pm)
    let m = 4 - frame_mbs_only * 2;
    let w = (pic_w + 1) * 16 - 4 * cl - 4 * cr;
    let h = ((pic_h + 1) * (2 - frame_mbs_only)) * 16 - m * ct - m * cb;
    // Validity check matching ExifTool H264.pm
    if (160..=4096).contains(&w) && (120..=3072).contains(&h) {
        Some((w, h))
    } else {
        None
    }
}

/// Returns (Option<(width,height)>, Option<MdpmData>) by scanning NAL units in payload.
fn m2ts_parse_h264_pes(payload: &[u8]) -> (Option<(u32, u32)>, Option<M2tsMdpmData>) {
    let mut dims = None;
    let mut mdpm = None;
    let mut i = 0;
    while i + 3 <= payload.len() {
        let nal_start = if payload[i] == 0
            && payload[i + 1] == 0
            && i + 3 < payload.len()
            && payload[i + 2] == 1
        {
            i + 3
        } else if i + 4 < payload.len()
            && payload[i] == 0
            && payload[i + 1] == 0
            && payload[i + 2] == 0
            && payload[i + 3] == 1
        {
            i + 4
        } else {
            i += 1;
            continue;
        };
        if nal_start >= payload.len() {
            break;
        }
        let nal_type = payload[nal_start] & 0x1F;
        match nal_type {
            7 if dims.is_none() => {
                dims = m2ts_parse_sps(&payload[nal_start..]);
            }
            6 if mdpm.is_none() => {
                mdpm = m2ts_parse_sei(&payload[nal_start..]);
            }
            _ => {}
        }
        i = nal_start + 1;
    }
    (dims, mdpm)
}

fn m2ts_parse_ac3_sample_rate(payload: &[u8]) -> Option<u32> {
    // Scan for 0x0B77 sync word and read fscod
    let pos = payload.windows(2).position(|w| w == [0x0B, 0x77])?;
    if pos + 5 > payload.len() {
        return None;
    }
    let fscod = payload[pos + 4] >> 6;
    let rates = [48000u32, 44100, 32000, 0];
    Some(rates.get(fscod as usize).copied().unwrap_or(0))
}

fn m2ts_stream_type_name(st: u8) -> &'static str {
    match st {
        0x01 => "MPEG1Video",
        0x02 => "MPEG2Video",
        0x03 => "MPEG1Audio",
        0x04 => "MPEG2Audio",
        0x0f => "ADTS AAC",
        0x10 => "MPEG4Video",
        0x1b => "H.264 (AVC) Video",
        0x24 => "HEVC Video",
        0x81 => "A52/AC-3 Audio",
        0x82 => "DTS Audio",
        _ => "Unknown",
    }
}

fn m2ts_format_bitrate(kbps: u32) -> String {
    format!("{} kbps", kbps)
}

fn m2ts_format_duration(first: u64, last: u64) -> String {
    if last <= first {
        return "0 s".to_string();
    }
    let ticks = last - first;
    let total_secs = ticks / 27_000_000;
    if total_secs == 0 {
        return "0 s".to_string();
    }
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    if h > 0 {
        format!("{}:{:02}:{:02}", h, m, s)
    } else {
        format!("{}:{:02}", m, s)
    }
}

pub fn read_m2ts(data: &[u8], extract_embedded: u8) -> Result<Vec<Tag>> {
    if data.is_empty() {
        return Err(Error::InvalidData("empty file".into()));
    }

    let (packet_size, tco) = m2ts_find_packet_size(data)
        .ok_or_else(|| Error::InvalidData("not an MPEG-2 TS file".into()))?;

    let mut tags = Vec::new();
    let num_packets = data.len() / packet_size;
    // With -ee, scan all packets; without, scan first 2000 only
    let scan_count = if extract_embedded > 0 {
        num_packets
    } else {
        num_packets.min(2000)
    };

    let mut pmt_pids: Vec<u16> = Vec::new();
    let mut pmt_buf: std::collections::HashMap<u16, Vec<u8>> = std::collections::HashMap::new();
    let mut pat_done = false;
    let mut stream_info: Option<M2tsStreamInfo> = None;
    let mut h264_dims: Option<(u32, u32)> = None;
    let mut mdpm_data: Option<M2tsMdpmData> = None;
    let mut all_mdpm: Vec<M2tsMdpmData> = Vec::new();
    let mut ac3_sample_rate: Option<u32> = None;
    let mut pcr_first: Option<u64> = None;
    let mut pcr_last: Option<u64> = None;

    for pkt_idx in 0..scan_count {
        let pkt = &data[pkt_idx * packet_size..(pkt_idx + 1) * packet_size];

        // Extract PCR from adaptation field (AFC=2 or AFC=3)
        let hdr = &pkt[tco..];
        if hdr.len() >= 12 && hdr[0] == 0x47 {
            let afc = (hdr[3] >> 4) & 0x3;
            if (afc == 2 || afc == 3) && hdr.len() > 5 {
                let af_len = hdr[4] as usize;
                if af_len >= 7 && hdr.len() >= 12 {
                    let af_flags = hdr[5];
                    if af_flags & 0x10 != 0 {
                        let pb = ((hdr[6] as u64) << 25)
                            | ((hdr[7] as u64) << 17)
                            | ((hdr[8] as u64) << 9)
                            | ((hdr[9] as u64) << 1)
                            | ((hdr[10] as u64) >> 7);
                        let pe = (((hdr[10] as u64) & 1) << 8) | hdr[11] as u64;
                        let pcr = pb * 300 + pe;
                        if pcr_first.is_none() {
                            pcr_first = Some(pcr);
                        }
                        pcr_last = Some(pcr);
                    }
                }
            }
        }

        if let Some((pusi, pid, payload)) = m2ts_get_payload(pkt, tco) {
            if pid == 0x0000 && !pat_done {
                let section = if pusi && !payload.is_empty() {
                    let ptr = payload[0] as usize;
                    &payload[(ptr + 1).min(payload.len())..]
                } else {
                    payload
                };
                let new_pmts = m2ts_parse_pat(section);
                if !new_pmts.is_empty() {
                    pmt_pids = new_pmts;
                    pat_done = true;
                }
            } else if stream_info.is_none() && pmt_pids.contains(&pid) {
                let buf = pmt_buf.entry(pid).or_default();
                if pusi {
                    buf.clear();
                    let ptr = if !payload.is_empty() {
                        payload[0] as usize
                    } else {
                        0
                    };
                    buf.extend_from_slice(&payload[(ptr + 1).min(payload.len())..]);
                } else {
                    buf.extend_from_slice(payload);
                }
                let buf_clone = buf.clone();
                if let Some(si) = m2ts_parse_pmt(&buf_clone) {
                    stream_info = Some(si);
                }
            } else if let Some(ref si) = stream_info {
                let need_first = h264_dims.is_none() || mdpm_data.is_none();
                if (need_first || extract_embedded > 0) && Some(pid) == si.h264_pid {
                    // Skip PES header to get to ES data
                    let es = m2ts_skip_pes_header(payload);
                    let (dims, mdpm) = m2ts_parse_h264_pes(es);
                    if dims.is_some() && h264_dims.is_none() {
                        h264_dims = dims;
                    }
                    if let Some(ref m) = mdpm {
                        if mdpm_data.is_none() {
                            mdpm_data = mdpm.clone();
                        }
                        if extract_embedded > 0 {
                            all_mdpm.push(m.clone());
                        }
                    }
                }
                if ac3_sample_rate.is_none() && Some(pid) == si.audio_pid {
                    let es = m2ts_skip_pes_header(payload);
                    if let Some(sr) = m2ts_parse_ac3_sample_rate(es) {
                        if sr > 0 {
                            ac3_sample_rate = Some(sr);
                        }
                    }
                }
            }
        }
    }

    // Also scan last packets for PCR (duration)
    if num_packets > scan_count {
        for pkt_idx in (num_packets - 500).max(scan_count)..num_packets {
            let pkt = &data[pkt_idx * packet_size..(pkt_idx + 1) * packet_size];
            let hdr = &pkt[tco..];
            if hdr.len() >= 12 && hdr[0] == 0x47 {
                let afc = (hdr[3] >> 4) & 0x3;
                if (afc == 2 || afc == 3) && hdr.len() > 5 {
                    let af_len = hdr[4] as usize;
                    if af_len >= 7 {
                        let af_flags = hdr[5];
                        if af_flags & 0x10 != 0 && hdr.len() >= 12 {
                            let pb = ((hdr[6] as u64) << 25)
                                | ((hdr[7] as u64) << 17)
                                | ((hdr[8] as u64) << 9)
                                | ((hdr[9] as u64) << 1)
                                | ((hdr[10] as u64) >> 7);
                            let pe = (((hdr[10] as u64) & 1) << 8) | hdr[11] as u64;
                            pcr_last = Some(pb * 300 + pe);
                        }
                    }
                }
            }
        }
    }

    // Emit tags
    if let Some(ref si) = stream_info {
        if let Some(ref vt) = si.video_type {
            tags.push(mktag(
                "M2TS",
                "VideoStreamType",
                "Video Stream Type",
                Value::String(vt.clone()),
            ));
        }
        if let Some(ref at) = si.audio_type {
            tags.push(mktag(
                "M2TS",
                "AudioStreamType",
                "Audio Stream Type",
                Value::String(at.clone()),
            ));
        }

        // AC3 audio descriptor info
        if si.audio_bitrate_idx.is_some()
            || si.audio_surround_mode.is_some()
            || si.audio_channels.is_some()
        {
            let bitrates = [
                32u32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 448, 512,
                576, 640,
            ];
            if let Some(bi) = si.audio_bitrate_idx {
                let idx = bi as usize;
                if idx < bitrates.len() {
                    tags.push(mktag(
                        "M2TS",
                        "AudioBitrate",
                        "Audio Bitrate",
                        Value::String(m2ts_format_bitrate(bitrates[idx])),
                    ));
                }
            }
            if let Some(sm) = si.audio_surround_mode {
                let s = match sm {
                    0 => "Not indicated",
                    1 => "Not Dolby surround",
                    2 => "Dolby surround",
                    _ => "Reserved",
                };
                tags.push(mktag(
                    "M2TS",
                    "SurroundMode",
                    "Surround Mode",
                    Value::String(s.into()),
                ));
            }
            if let Some(ch) = si.audio_channels {
                let cs = match ch {
                    0 => "1 + 1",
                    1 => "1",
                    2 => "2",
                    3 => "3",
                    4 => "2/1",
                    5 => "3/1",
                    6 => "2/2",
                    7 => "3/2",
                    _ => "Unknown",
                };
                tags.push(mktag(
                    "M2TS",
                    "AudioChannels",
                    "Audio Channels",
                    Value::String(cs.into()),
                ));
            }
        }
    }

    if let Some((w, h)) = h264_dims {
        tags.push(mktag("M2TS", "ImageWidth", "Image Width", Value::U32(w)));
        tags.push(mktag("M2TS", "ImageHeight", "Image Height", Value::U32(h)));
    }

    if let Some(sr) = ac3_sample_rate {
        tags.push(mktag(
            "M2TS",
            "AudioSampleRate",
            "Audio Sample Rate",
            Value::U32(sr),
        ));
    }

    // Duration
    if let (Some(first), Some(last)) = (pcr_first, pcr_last) {
        let dur = m2ts_format_duration(first, last);
        tags.push(mktag("M2TS", "Duration", "Duration", Value::String(dur)));
    }

    // MDPM camera metadata from H.264 SEI
    if let Some(ref mdpm) = mdpm_data {
        if let Some(ref v) = mdpm.make {
            tags.push(mktag("H264", "Make", "Make", Value::String(v.clone())));
        }
        if let Some(ref v) = mdpm.datetime_original {
            tags.push(mktag(
                "H264",
                "DateTimeOriginal",
                "Date/Time Original",
                Value::String(v.clone()),
            ));
        }
        if let Some(ref v) = mdpm.aperture_setting {
            tags.push(mktag(
                "H264",
                "ApertureSetting",
                "Aperture Setting",
                Value::String(v.clone()),
            ));
        }
        if let Some(ref v) = mdpm.gain {
            tags.push(mktag("H264", "Gain", "Gain", Value::String(v.clone())));
        }
        if let Some(ref v) = mdpm.image_stabilization {
            tags.push(mktag(
                "H264",
                "ImageStabilization",
                "Image Stabilization",
                Value::String(v.clone()),
            ));
        }
        if let Some(ref v) = mdpm.exposure_time {
            tags.push(mktag(
                "H264",
                "ExposureTime",
                "Exposure Time",
                Value::String(v.clone()),
            ));
        }
        if let Some(ref v) = mdpm.shutter_speed {
            tags.push(mktag(
                "H264",
                "ShutterSpeed",
                "Shutter Speed",
                Value::String(v.clone()),
            ));
        }
        if let Some(ref v) = mdpm.recording_mode {
            tags.push(mktag(
                "H264",
                "RecordingMode",
                "Recording Mode",
                Value::String(v.clone()),
            ));
        }
        // ExifTool emits Warning only when -ee is NOT used
        if extract_embedded == 0 {
            tags.push(mktag(
                "M2TS",
                "Warning",
                "Warning",
                Value::String(
                    "[minor] The ExtractEmbedded option may find more tags in the video data"
                        .to_string(),
                ),
            ));
        }
    }

    // With -ee: emit tags from ALL MDPM frames (per-frame metadata)
    // Skip first frame (already emitted above from mdpm_data)
    if extract_embedded > 0 && all_mdpm.len() > 1 {
        for mdpm in &all_mdpm[1..] {
            if let Some(ref v) = mdpm.datetime_original {
                tags.push(mktag(
                    "H264",
                    "DateTimeOriginal",
                    "Date/Time Original",
                    Value::String(v.clone()),
                ));
            }
            if let Some(ref v) = mdpm.make {
                tags.push(mktag("H264", "Make", "Make", Value::String(v.clone())));
            }
        }
    }

    Ok(tags)
}

fn m2ts_skip_pes_header(payload: &[u8]) -> &[u8] {
    // PES header: 00 00 01 stream_id [2 bytes length] [variable header]
    if payload.len() < 9 || payload[0] != 0x00 || payload[1] != 0x00 || payload[2] != 0x01 {
        return payload;
    }
    let stream_id = payload[3];
    // Private stream IDs don't have standard PES header extension
    if stream_id == 0xBC
        || stream_id == 0xBE
        || stream_id == 0xBF
        || stream_id == 0xF0
        || stream_id == 0xF1
        || stream_id == 0xFF
        || stream_id == 0xF2
        || stream_id == 0xF8
    {
        return &payload[6..];
    }
    if payload.len() < 9 {
        return payload;
    }
    let header_data_length = payload[8] as usize;
    let es_start = 9 + header_data_length;
    if es_start <= payload.len() {
        &payload[es_start..]
    } else {
        payload
    }
}
