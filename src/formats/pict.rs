//! Apple PICT format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

pub fn read_pict(data: &[u8]) -> Result<Vec<Tag>> {
    // PICT files have a 512-byte header (usually zeros) then the picture data
    let offset = if data.len() > 522 && data[..512].iter().all(|&b| b == 0) {
        512
    } else {
        0
    };

    if offset + 10 > data.len() {
        return Err(Error::InvalidData("not a PICT file".into()));
    }

    let mut tags = Vec::new();
    let d = &data[offset..];

    // Size (2 bytes) + bounding rect (8 bytes: top, left, bottom, right)
    let top = i16::from_be_bytes([d[2], d[3]]);
    let left = i16::from_be_bytes([d[4], d[5]]);
    let bottom = i16::from_be_bytes([d[6], d[7]]);
    let right = i16::from_be_bytes([d[8], d[9]]);

    // Check PICT version at byte 10
    // Version 2 opcode: 0x0011 at bytes 10-11
    let mut h_res: Option<f64> = None;
    let mut v_res: Option<f64> = None;
    let mut w = (right - left) as i32;
    let mut h = (bottom - top) as i32;

    if d.len() >= 40 && d[10] == 0x00 && d[11] == 0x11 {
        // Version 2: next 2 bytes are 0x02ff, then check for extended
        // d[12..14] = 0x02ff, d[14..16] = 0x0c00
        // d[16..18]: 0xffff = normal, 0xfffe = extended
        if d.len() >= 18
            && d[12] == 0x02
            && d[13] == 0xff
            && d[16] == 0xff
            && d[17] == 0xfe
            && d.len() >= 36
        {
            // Extended version 2: resolution at offsets 24..28 and 28..32 (x8 skip from byte 16)
            // From Perl: unpack('x8N2', $buff) where buff starts at byte after 0x0011 opcode
            // $buff was read starting at position 12 (after 12-byte first read)
            // x8 skips bytes 12..20, N2 reads bytes 20..24 and 24..28 in original data
            // Actually the 28 bytes buff starts after the 12-byte header
            // In d: after opcode 0x0011 at d[10..12], read 28 bytes: d[12..40]
            // x8 skip => skip d[12..20], N2 => d[20..24] and d[24..28]
            let h_fixed = i32::from_be_bytes([d[20], d[21], d[22], d[23]]);
            let v_fixed = i32::from_be_bytes([d[24], d[25], d[26], d[27]]);
            if h_fixed != 0 && v_fixed != 0 {
                h_res = Some(h_fixed as f64 / 65536.0);
                v_res = Some(v_fixed as f64 / 65536.0);
                // Scale dimensions from 72-dpi equivalent
                w = (w as f64 * h_res.unwrap() / 72.0 + 0.5) as i32;
                h = (h as f64 * v_res.unwrap() / 72.0 + 0.5) as i32;
            }
        }
    }

    tags.push(mktag("PICT", "ImageWidth", "Image Width", Value::I32(w)));
    tags.push(mktag("PICT", "ImageHeight", "Image Height", Value::I32(h)));
    if let Some(hr) = h_res {
        tags.push(mktag(
            "PICT",
            "XResolution",
            "X Resolution",
            Value::String(format!("{}", hr as i64)),
        ));
    }
    if let Some(vr) = v_res {
        tags.push(mktag(
            "PICT",
            "YResolution",
            "Y Resolution",
            Value::String(format!("{}", vr as i64)),
        ));
    }

    Ok(tags)
}
