//! Google HDRP MakerNote decoder.
//!
//! Decodes the Google HDR+ protobuf maker note stored in XMP GCamera:HdrPlusMakernote.
//! The encoding is: base64 → HDRP header stripped → XOR decrypt → gunzip → protobuf.
//!
//! References:
//! - ExifTool Google.pm ProcessHDRP (Perl source)
//! - <https://github.com/jakiki6/ruminant/blob/master/ruminant/modules/images.py>

use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

// ============================================================================
// Public entry point
// ============================================================================

/// Decode base64-encoded HDRP maker note data and return extracted tags.
///
/// `b64_data` is the raw base64 string from XMP GCamera:HdrPlusMakernote.
/// Returns a list of MakerNotes tags extracted from the protobuf payload.
pub fn decode_hdrp_makernote(b64_data: &str) -> Vec<Tag> {
    // Step 1: base64 decode
    let raw = match base64_decode(b64_data.trim()) {
        Some(v) => v,
        None => return Vec::new(),
    };

    // Step 2: decrypt + gunzip
    let decompressed = match hdrp_decrypt_gunzip(&raw) {
        Some(v) => v,
        None => return Vec::new(),
    };

    // Step 3: parse protobuf and map to tags
    parse_hdrp_protobuf(&decompressed)
}

// ============================================================================
// Base64 decoder
// ============================================================================

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const TABLE: [i8; 256] = {
        let mut t = [-1i8; 256];
        let mut i = 0u8;
        // A-Z = 0-25
        while i < 26 {
            t[(b'A' + i) as usize] = i as i8;
            i += 1;
        }
        // a-z = 26-51
        i = 0;
        while i < 26 {
            t[(b'a' + i) as usize] = (i + 26) as i8;
            i += 1;
        }
        // 0-9 = 52-61
        i = 0;
        while i < 10 {
            t[(b'0' + i) as usize] = (i + 52) as i8;
            i += 1;
        }
        t[b'+' as usize] = 62;
        t[b'/' as usize] = 63;
        t[b'=' as usize] = 0; // padding
        t
    };

    let bytes: Vec<u8> = s.bytes().filter(|&b| !b.is_ascii_whitespace()).collect();
    if bytes.is_empty() {
        return None;
    }

    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut i = 0;
    while i + 3 < bytes.len() {
        let a = TABLE[bytes[i] as usize];
        let b = TABLE[bytes[i + 1] as usize];
        let c = TABLE[bytes[i + 2] as usize];
        let d = TABLE[bytes[i + 3] as usize];
        if a < 0 || b < 0 {
            return None;
        }
        out.push(((a as u8) << 2) | ((b as u8) >> 4));
        if bytes[i + 2] != b'=' {
            out.push(((b as u8) << 4) | ((c as u8) >> 2));
        }
        if bytes[i + 3] != b'=' {
            out.push(((c as u8) << 6) | (d as u8));
        }
        i += 4;
    }
    Some(out)
}

// ============================================================================
// HDRP decryption (port of Perl ProcessHDRP XOR key schedule)
// ============================================================================

/// Decrypt the HDRP payload (after stripping "HDRP\x02" or "HDRP\x03" header).
/// Returns decrypted bytes, or None if not an HDRP stream.
fn hdrp_decrypt(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 5 || &data[0..4] != b"HDRP" {
        return None;
    }
    let _ver = data[4];

    // Data after the 5-byte HDRP header
    let payload = &data[5..];
    let pad = (8usize.wrapping_sub(payload.len() % 8)) & 7;
    let mut words: Vec<u32> = {
        let mut padded = payload.to_vec();
        padded.extend_from_slice(&[0u8; 8][..pad]);
        padded
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    };

    // Initial key: 0x2515606b4a7791cd (hi=0x2515606b, lo=0x4a7791cd)
    let mut hi: u64 = 0x2515606b;
    let mut lo: u64 = 0x4a7791cd;

    let mut i = 0;
    while i < words.len() {
        // Rotate the 64-bit key using the xorshift-then-multiply from Perl
        // All arithmetic kept as 64-bit to avoid wrapping issues

        // Perl: $lo ^= $lo >> 12 | ($hi & 0xfff) << 20;
        // Perl: $hi ^= $hi >> 12;
        // Note: Perl's `|` has lower precedence than `^=`, so:
        //   $lo ^= (($lo >> 12) | (($hi & 0xfff) << 20))
        let new_lo = lo ^ ((lo >> 12) | ((hi & 0xfff) << 20));
        let new_hi = hi ^ (hi >> 12);
        lo = new_lo & 0xffffffff;
        hi = new_hi & 0xffffffff;

        // Perl: $hi ^= ($hi & 0x7f) << 25 | $lo >> 7;
        // Perl: $lo ^= ($lo & 0x7f) << 25;
        let new_hi = hi ^ (((hi & 0x7f) << 25) | (lo >> 7));
        let new_lo = lo ^ ((lo & 0x7f) << 25);
        lo = new_lo & 0xffffffff;
        hi = new_hi & 0xffffffff;

        // Perl: $lo ^= $lo >> 27 | ($hi & 0x7ffffff) << 5;
        // Perl: $hi ^= $hi >> 27;
        let new_lo = lo ^ ((lo >> 27) | ((hi & 0x7ffffff) << 5));
        let new_hi = hi ^ (hi >> 27);
        lo = new_lo & 0xffffffff;
        hi = new_hi & 0xffffffff;

        // Multiply key by 0x2545f4914f6cdd1d (64-bit × 64-bit, keep low 64 bits)
        // Perl uses 16-bit chunks to avoid overflow; we use u128 in Rust
        let key64: u64 = (hi << 32) | lo;
        let product = (key64 as u128).wrapping_mul(0x2545f4914f6cdd1d_u128);
        let result64 = product as u64;
        lo = result64 & 0xffffffff;
        hi = (result64 >> 32) & 0xffffffff;

        // XOR the key lo/hi with the current 64-bit word
        words[i] ^= lo as u32;
        i += 1;
        if i < words.len() {
            words[i] ^= hi as u32;
            i += 1;
        }
    }

    // Reassemble into bytes, remove padding
    let mut result: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    if pad > 0 {
        result.truncate(result.len().saturating_sub(pad));
    }
    Some(result)
}

// ============================================================================
// gzip wrapper (RFC 1952) over flate2
// ============================================================================

fn deflate_decompress(data: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::DeflateDecoder::new(data)
        .read_to_end(&mut out)
        .ok()?;
    Some(out)
}

/// Decompress gzip-compressed data (RFC 1952 wrapper + DEFLATE payload).
fn gunzip(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 18 {
        return None;
    }
    if data[0] != 0x1f || data[1] != 0x8b {
        return None;
    }
    let method = data[2];
    if method != 8 {
        return None;
    }
    let flags = data[3];

    let mut pos = 10usize;
    // Skip FEXTRA
    if flags & 0x04 != 0 {
        if pos + 2 > data.len() {
            return None;
        }
        let xlen = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2 + xlen;
    }
    // Skip FNAME
    if flags & 0x08 != 0 {
        while pos < data.len() && data[pos] != 0 {
            pos += 1;
        }
        pos += 1;
    }
    // Skip FCOMMENT
    if flags & 0x10 != 0 {
        while pos < data.len() && data[pos] != 0 {
            pos += 1;
        }
        pos += 1;
    }
    // Skip CRC16
    if flags & 0x02 != 0 {
        pos += 2;
    }

    if pos + 8 > data.len() {
        return None;
    }
    let compressed = &data[pos..data.len() - 8];
    deflate_decompress(compressed)
}

fn hdrp_decrypt_gunzip(data: &[u8]) -> Option<Vec<u8>> {
    let decrypted = hdrp_decrypt(data)?;
    gunzip(&decrypted)
}

// ============================================================================
// Protobuf parser → tag extraction
// ============================================================================

/// Read a varint from data starting at `pos`. Returns (value, new_pos) or None.
fn read_varint(data: &[u8], pos: usize) -> Option<(u64, usize)> {
    let mut val = 0u64;
    let mut shift = 0u32;
    let mut p = pos;
    loop {
        if p >= data.len() {
            return None;
        }
        let b = data[p] as u64;
        p += 1;
        val |= (b & 0x7f) << shift;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 70 {
            return None;
        } // malformed
    }
    Some((val, p))
}

/// Read length-delimited bytes from data at `pos`. Returns (bytes, new_pos) or None.
fn read_len_delimited(data: &[u8], pos: usize) -> Option<(&[u8], usize)> {
    let (len, p) = read_varint(data, pos)?;
    let end = p + len as usize;
    if end > data.len() {
        return None;
    }
    Some((&data[p..end], end))
}

/// Tag info for a known protobuf field path.
struct FieldDef {
    path: &'static str, // e.g. "1-1", "12-8"
    name: &'static str,
    fmt: FieldFmt,
    group2: &'static str,
}

#[derive(Clone, Copy, Debug)]
enum FieldFmt {
    String,
    Binary,
    Unsigned,     // varint as unsigned integer
    Float,        // 4-byte IEEE float
    FloatDiv1000, // float / 1000 (ExposureTime conversion)
    UnixTimeMs,   // varint / 1000 → Unix timestamp with milliseconds
    UnixTime,     // varint seconds → local Unix timestamp (ConvertUnixTime $val, 1)
}

static HDRP_FIELDS: &[FieldDef] = &[
    FieldDef {
        path: "1-1",
        name: "ImageName",
        fmt: FieldFmt::String,
        group2: "Image",
    },
    FieldDef {
        path: "1-2",
        name: "ImageData",
        fmt: FieldFmt::Binary,
        group2: "Image",
    },
    FieldDef {
        path: "2",
        name: "TimeLogText",
        fmt: FieldFmt::Binary,
        group2: "Image",
    },
    FieldDef {
        path: "3",
        name: "SummaryText",
        fmt: FieldFmt::Binary,
        group2: "Image",
    },
    FieldDef {
        path: "9-3",
        name: "FrameCount",
        fmt: FieldFmt::Unsigned,
        group2: "Image",
    },
    // Google.pm:539 '9-36-1' CreateDate: Format 'unsigned', ConvertUnixTime($val,1).
    FieldDef {
        path: "9-36-1",
        name: "CreateDate",
        fmt: FieldFmt::UnixTime,
        group2: "Time",
    },
    FieldDef {
        path: "12-1",
        name: "DeviceMake",
        fmt: FieldFmt::String,
        group2: "Device",
    },
    FieldDef {
        path: "12-2",
        name: "DeviceModel",
        fmt: FieldFmt::String,
        group2: "Device",
    },
    FieldDef {
        path: "12-3",
        name: "DeviceCodename",
        fmt: FieldFmt::String,
        group2: "Device",
    },
    FieldDef {
        path: "12-4",
        name: "DeviceHardwareRevision",
        fmt: FieldFmt::String,
        group2: "Device",
    },
    FieldDef {
        path: "12-6",
        name: "HDRPSoftware",
        fmt: FieldFmt::String,
        group2: "Device",
    },
    FieldDef {
        path: "12-7",
        name: "AndroidRelease",
        fmt: FieldFmt::String,
        group2: "Device",
    },
    FieldDef {
        path: "12-8",
        name: "SoftwareDate",
        fmt: FieldFmt::UnixTimeMs,
        group2: "Time",
    },
    FieldDef {
        path: "12-9",
        name: "Application",
        fmt: FieldFmt::String,
        group2: "Device",
    },
    FieldDef {
        path: "12-10",
        name: "AppVersion",
        fmt: FieldFmt::String,
        group2: "Device",
    },
    FieldDef {
        path: "12-12-1",
        name: "ExposureTimeMin",
        fmt: FieldFmt::FloatDiv1000,
        group2: "Camera",
    },
    FieldDef {
        path: "12-12-2",
        name: "ExposureTimeMax",
        fmt: FieldFmt::FloatDiv1000,
        group2: "Camera",
    },
    FieldDef {
        path: "12-13-1",
        name: "ISOMin",
        fmt: FieldFmt::Float,
        group2: "Camera",
    },
    FieldDef {
        path: "12-13-2",
        name: "ISOMax",
        fmt: FieldFmt::Float,
        group2: "Camera",
    },
    FieldDef {
        path: "12-14",
        name: "MaxAnalogISO",
        fmt: FieldFmt::Float,
        group2: "Camera",
    },
];

fn make_tag(name: &str, group2: &str, raw: Value, print: String) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            // Image::ExifTool::Google::HDRPlusMakerNote: family 1 is the module
            // name, Google.
            family0: "MakerNotes".into(),
            family1: "Google".into(),
            family2: group2.into(),
            family3: "Main".into(),
        },
        raw_value: raw,
        print_value: print,
        priority: 0,
    }
}

/// True if some known field lives below `path` (so recursing into it can yield tags).
fn has_known_descendant(path: &str) -> bool {
    HDRP_FIELDS.iter().any(|d| {
        d.path.len() > path.len()
            && d.path.as_bytes()[path.len()] == b'-'
            && d.path.starts_with(path)
    })
}

/// Parse protobuf data recursively, collecting tags.
/// `prefix` is the dot-separated path built so far (e.g. "" at top, "12-" inside field 12).
fn parse_protobuf(data: &[u8], prefix: &str, tags: &mut Vec<Tag>) {
    let mut pos = 0;
    while pos < data.len() {
        let (tag_word, p) = match read_varint(data, pos) {
            Some(v) => v,
            None => break,
        };
        let field_id = tag_word >> 3;
        let wire_type = tag_word & 7;
        pos = p;

        let path = if prefix.is_empty() {
            format!("{}", field_id)
        } else {
            format!("{}{}", prefix, field_id)
        };

        match wire_type {
            0 => {
                // varint
                let (val, p2) = match read_varint(data, pos) {
                    Some(v) => v,
                    None => break,
                };
                pos = p2;
                // Check if this path is a known field
                if let Some(def) = HDRP_FIELDS.iter().find(|d| d.path == path) {
                    let (raw, print) = match def.fmt {
                        FieldFmt::Unsigned => {
                            let s = format!("{}", val);
                            (Value::U32(val as u32), s)
                        }
                        FieldFmt::UnixTimeMs => {
                            // ExifTool: ConvertUnixTime($val / 1000, 1, 3)
                            // Convert ms timestamp to ExifTool datetime format
                            let secs = (val / 1000) as i64;
                            let ms = val % 1000;
                            let s = format_unix_time_ms(secs, ms as u32);
                            (Value::String(s.clone()), s)
                        }
                        FieldFmt::UnixTime => {
                            // ExifTool: ConvertUnixTime($val, 1) — local time, no
                            // sub-seconds.
                            let s = crate::formats::gzip::gzip_unix_to_datetime(val as i64);
                            (Value::String(s.clone()), s)
                        }
                        _ => {
                            let s = format!("{}", val);
                            (Value::U32(val as u32), s)
                        }
                    };
                    let mut t = make_tag(def.name, def.group2, raw, print);
                    // Google.pm:542 marks '9-36-1' CreateDate `Priority => 0` ("to
                    // give EXIF priority"), so the EXIF CreateDate wins when
                    // duplicates are collapsed; both show under -ee.
                    if matches!(def.fmt, FieldFmt::UnixTime) {
                        t.priority = crate::tag::PRIORITY_EXPLICIT_ZERO;
                    }
                    tags.push(t);
                }
            }
            1 => {
                // 64-bit
                if pos + 8 > data.len() {
                    break;
                }
                pos += 8;
            }
            2 => {
                // length-delimited
                let (bytes, p2) = match read_len_delimited(data, pos) {
                    Some(v) => v,
                    None => break,
                };
                pos = p2;

                // Check known fields first
                if let Some(def) = HDRP_FIELDS.iter().find(|d| d.path == path) {
                    match def.fmt {
                        FieldFmt::String => {
                            let s = crate::encoding::decode_utf8_or_latin1(bytes).to_string();
                            tags.push(make_tag(def.name, def.group2, Value::String(s.clone()), s));
                        }
                        FieldFmt::Binary => {
                            // Store as binary; ExifTool shows "(Binary data N bytes)"
                            let print = format!(
                                "(Binary data {} bytes, use -b option to extract)",
                                bytes.len()
                            );
                            tags.push(make_tag(
                                def.name,
                                def.group2,
                                Value::Binary(bytes.to_vec()),
                                print,
                            ));
                        }
                        FieldFmt::Float | FieldFmt::FloatDiv1000 => {
                            // 4-byte float in a len-delimited wrapper
                            if bytes.len() >= 4 {
                                let f =
                                    f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                                let v = if matches!(def.fmt, FieldFmt::FloatDiv1000) {
                                    f as f64 / 1000.0
                                } else {
                                    f as f64
                                };
                                let s = format_float(v);
                                tags.push(make_tag(def.name, def.group2, Value::F64(v), s));
                            }
                        }
                        _ => {
                            // Try to recurse as sub-message
                            if has_known_descendant(&path) {
                                parse_protobuf(bytes, &format!("{}-", path), tags);
                            }
                        }
                    }
                } else {
                    // Unknown field - try to recurse as sub-message to find known sub-fields
                    if has_known_descendant(&path) {
                        parse_protobuf(bytes, &format!("{}-", path), tags);
                    }
                }
            }
            5 => {
                // 32-bit
                if pos + 4 > data.len() {
                    break;
                }
                let bytes = &data[pos..pos + 4];
                pos += 4;

                if let Some(def) = HDRP_FIELDS.iter().find(|d| d.path == path) {
                    if matches!(def.fmt, FieldFmt::Float | FieldFmt::FloatDiv1000) {
                        let f = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                        let v = if matches!(def.fmt, FieldFmt::FloatDiv1000) {
                            f as f64 / 1000.0
                        } else {
                            f as f64
                        };
                        let s = format_float(v);
                        tags.push(make_tag(def.name, def.group2, Value::F64(v), s));
                    }
                }
            }
            3 | 4 => {
                // Deprecated group start/end - skip
            }
            _ => break, // Unknown wire type, stop parsing
        }
    }
}

/// Parse the decompressed HDRP protobuf payload.
fn parse_hdrp_protobuf(data: &[u8]) -> Vec<Tag> {
    let mut tags = Vec::new();
    parse_protobuf(data, "", &mut tags);
    tags
}

// ============================================================================
// Utilities
// ============================================================================

/// Format a float value like ExifTool/Perl's default stringification.
/// Perl uses up to ~15 significant digits, removes trailing zeros.
fn format_float(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    // Use Perl-like %.15g formatting: up to 15 significant digits, no trailing zeros
    format_g(v, 15)
}

/// Format like printf("%.{prec}g", v) - significant digits, strip trailing zeros
fn format_g(v: f64, prec: usize) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let abs = v.abs();
    // Use scientific notation if exponent >= prec or exponent < -4
    let exp = abs.log10().floor() as i32;
    if exp >= prec as i32 || exp < -4 {
        // Scientific notation
        let _mantissa = v / 10f64.powi(exp);
        let decimals = prec.saturating_sub(1);
        let s = format!("{:.prec$e}", v, prec = decimals);
        // Perl uses e+XX format
        normalize_sci(s)
    } else {
        // Fixed notation with enough precision
        let decimals = if exp >= 0 {
            prec.saturating_sub(exp as usize + 1)
        } else {
            prec + (-exp - 1) as usize
        };
        let s = format!("{:.prec$}", v, prec = decimals);
        // Strip trailing zeros after decimal point
        strip_trailing_zeros(s)
    }
}

fn normalize_sci(s: String) -> String {
    // Convert Rust "1.234e5" to Perl "1.234e+05" or "1.234e-05"
    if let Some(e_pos) = s.find('e') {
        let mantissa = &s[..e_pos];
        let exp_str = &s[e_pos + 1..];
        let mantissa = strip_trailing_zeros(mantissa.to_string());
        let exp: i32 = exp_str.parse().unwrap_or(0);
        if exp >= 0 {
            format!("{}e+{:02}", mantissa, exp)
        } else {
            format!("{}e-{:02}", mantissa, -exp)
        }
    } else {
        s
    }
}

fn strip_trailing_zeros(s: String) -> String {
    if s.contains('.') {
        let s = s.trim_end_matches('0').to_string();
        let s = s.trim_end_matches('.').to_string();
        s
    } else {
        s
    }
}

/// Format a Unix timestamp (seconds + milliseconds) in ExifTool's datetime format.
/// ExifTool uses ConvertUnixTime($val/1000, 1, 3) which gives "YYYY:MM:DD HH:MM:SS.mmm+TZ"
/// We use UTC since we don't have timezone info.
fn format_unix_time_ms(secs: i64, ms: u32) -> String {
    // ExifTool prints in local time (ConvertUnixTime $val,1,3); splice the milliseconds
    // in before the timezone offset of the local datetime string.
    let local = crate::formats::gzip::gzip_unix_to_datetime(secs);
    // local looks like "YYYY:MM:DD HH:MM:SS+HH:MM" — insert ".mmm" before the tz sign.
    if let Some(tz_pos) = local.rfind(['+', '-']) {
        if tz_pos > 10 {
            return format!("{}.{:03}{}", &local[..tz_pos], ms, &local[tz_pos..]);
        }
    }
    format!("{}.{:03}", local, ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_decode() {
        // "HDRP" base64 encoded
        let encoded = "SERS"; // first 3 bytes of "HDRP..."
        let decoded = base64_decode(encoded);
        assert!(decoded.is_some());
    }

    #[test]
    fn test_deflate_stored() {
        // A stored block: BFINAL=1 BTYPE=00, then LEN/NLEN then data
        let data = b"Hello";
        let len = data.len() as u16;
        let nlen = !len;
        let mut block = vec![0x01u8]; // BFINAL=1, BTYPE=00 (stored)
        block.extend_from_slice(&len.to_le_bytes());
        block.extend_from_slice(&nlen.to_le_bytes());
        block.extend_from_slice(data);
        let result = deflate_decompress(&block);
        assert_eq!(result, Some(b"Hello".to_vec()));
    }
}

#[cfg(test)]
mod actual_data_tests {
    use super::*;

    #[test]
    fn test_actual_hdrp_decode() {
        // Read b64 from file if available
        let b64_path = "/tmp/hdrp_b64.txt";
        if let Ok(b64) = std::fs::read_to_string(b64_path) {
            let b64 = b64.trim();

            // Step 1: base64 decode
            let raw = base64_decode(b64).expect("b64 decode failed");
            assert!(raw.len() > 5, "too short: {}", raw.len());
            assert_eq!(&raw[..4], b"HDRP", "not HDRP: {:?}", &raw[..4]);
            assert_eq!(raw[4], 3, "wrong version: {}", raw[4]);

            // Step 2: decrypt
            let decrypted = hdrp_decrypt(&raw).expect("decrypt failed");
            assert!(decrypted.len() > 2, "decrypt too short");
            assert_eq!(
                decrypted[0], 0x1f,
                "not gzip magic[0]: {:02x}",
                decrypted[0]
            );
            assert_eq!(
                decrypted[1], 0x8b,
                "not gzip magic[1]: {:02x}",
                decrypted[1]
            );

            // Step 3: gunzip
            let decompressed = gunzip(&decrypted).expect("gunzip failed");
            assert!(!decompressed.is_empty(), "decompressed is empty");
            eprintln!(
                "decompressed {} bytes, first 20: {}",
                decompressed.len(),
                decompressed
                    .iter()
                    .take(20)
                    .map(|b| format!("{:02x}", b))
                    .collect::<Vec<_>>()
                    .join(" ")
            );

            // Step 4: parse protobuf
            let tags = parse_hdrp_protobuf(&decompressed);
            eprintln!("Found {} tags", tags.len());
            for tag in &tags {
                eprintln!("  {} = {}", tag.name, tag.print_value);
            }
            assert!(!tags.is_empty(), "no tags found");
        } else {
            eprintln!("Test file not found, skipping");
        }
    }
}
