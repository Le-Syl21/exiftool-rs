//! OpenEXR image format reader.
//!
//! Reads header attributes from EXR files.
//! Mirrors ExifTool's OpenEXR.pm ProcessEXR().

use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

fn mk(name: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "OpenEXR".into(),
            family1: "OpenEXR".into(),
            family2: "Image".into(),
            family3: "Main".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}

fn mk_with_print(name: &str, raw: Value, print: String) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            family0: "OpenEXR".into(),
            family1: "OpenEXR".into(),
            family2: "Image".into(),
            family3: "Main".into(),
        },
        raw_value: raw,
        print_value: print,
        priority: 0,
    }
}

fn read_le_u32(data: &[u8], off: usize) -> Option<u32> {
    if off + 4 > data.len() {
        return None;
    }
    Some(u32::from_le_bytes([
        data[off],
        data[off + 1],
        data[off + 2],
        data[off + 3],
    ]))
}

fn read_le_u8(data: &[u8], off: usize) -> Option<u8> {
    data.get(off).copied()
}

/// Read null-terminated string from data starting at pos, max_len bytes
fn read_cstr(data: &[u8], pos: usize, max_len: usize) -> Option<(&str, usize)> {
    let end = data[pos..].iter().take(max_len + 1).position(|&b| b == 0)?;
    let s = std::str::from_utf8(&data[pos..pos + end]).ok()?;
    Some((s, pos + end + 1))
}

pub fn read_openexr(data: &[u8]) -> crate::error::Result<Vec<Tag>> {
    if data.len() < 8 {
        return Ok(Vec::new());
    }
    // Magic: 0x762f3101 (little-endian)
    if data[0..4] != [0x76, 0x2f, 0x31, 0x01] {
        return Ok(Vec::new());
    }

    let flags = read_le_u32(data, 4).unwrap_or(0);
    let version = flags & 0xff;
    let flags_bits = flags & 0xffffff00;
    let max_name_len = if flags & 0x400 != 0 { 255 } else { 31 };

    let mut tags = Vec::new();

    // EXRVersion
    tags.push(mk("EXRVersion", Value::String(version.to_string())));

    // Flags
    {
        let mut flag_parts = Vec::new();
        if flags_bits & (1 << 9) != 0 {
            flag_parts.push("Tiled");
        }
        if flags_bits & (1 << 10) != 0 {
            flag_parts.push("Long names");
        }
        if flags_bits & (1 << 11) != 0 {
            flag_parts.push("Deep data");
        }
        if flags_bits & (1 << 12) != 0 {
            flag_parts.push("Multipart");
        }
        let flag_str = if flag_parts.is_empty() {
            "(none)".to_string()
        } else {
            flag_parts.join(", ")
        };
        tags.push(mk_with_print("Flags", Value::U32(flags_bits), flag_str));
    }

    let mut pos = 8;
    let mut data_window: Option<[i32; 4]> = None;
    let mut display_window: Option<[i32; 4]> = None;

    // Parse attributes
    loop {
        if pos >= data.len() {
            break;
        }
        if data[pos] == 0 {
            break;
        } // end of header

        // Read attribute name
        let (attr_name, next) = match read_cstr(data, pos, max_name_len) {
            Some(v) => v,
            None => break,
        };
        let attr_name = attr_name.to_string();
        pos = next;

        if pos >= data.len() {
            break;
        }

        // Read attribute type
        let (attr_type, next) = match read_cstr(data, pos, max_name_len) {
            Some(v) => v,
            None => break,
        };
        pos = next;

        // Read size
        let size = match read_le_u32(data, pos) {
            Some(v) => v as usize,
            None => break,
        };
        pos += 4;

        if pos + size > data.len() {
            break;
        }
        let val_data = &data[pos..pos + size];
        pos += size;

        // Process the attribute
        match attr_name.as_str() {
            "channels" => {
                // chlist: null-terminated channel entries
                let mut channels = Vec::new();
                let mut cp = 0;
                while cp < val_data.len() {
                    if val_data[cp] == 0 {
                        break;
                    }
                    // Read channel name (null-terminated, max 31)
                    let end = val_data[cp..].iter().take(32).position(|&b| b == 0);
                    let end = match end {
                        Some(e) => e,
                        None => break,
                    };
                    let ch_name = std::str::from_utf8(&val_data[cp..cp + end]).unwrap_or("?");
                    cp += end + 1;
                    // Read pixel type (4), linear (1), x sampling (4), y sampling (4) = 16 bytes total from name end, but ptype is first
                    if cp + 16 > val_data.len() {
                        break;
                    }
                    let pix_type = u32::from_le_bytes([
                        val_data[cp],
                        val_data[cp + 1],
                        val_data[cp + 2],
                        val_data[cp + 3],
                    ]);
                    let linear = val_data[cp + 4];
                    // skip 3 reserved bytes
                    let x_samp = u32::from_le_bytes([
                        val_data[cp + 8],
                        val_data[cp + 9],
                        val_data[cp + 10],
                        val_data[cp + 11],
                    ]);
                    let y_samp = u32::from_le_bytes([
                        val_data[cp + 12],
                        val_data[cp + 13],
                        val_data[cp + 14],
                        val_data[cp + 15],
                    ]);
                    cp += 16;
                    let pix_str = match pix_type {
                        0 => "int8u",
                        1 => "half",
                        2 => "float",
                        _ => "unknown",
                    };
                    let lin_str = if linear != 0 { " linear" } else { "" };
                    channels.push(format!(
                        "{} {}{} {} {}",
                        ch_name, pix_str, lin_str, x_samp, y_samp
                    ));
                }
                // Channels is a List=>1 tag: keep elements so JSON emits an array.
                tags.push(mk(
                    "Channels",
                    Value::List(channels.into_iter().map(Value::String).collect()),
                ));
            }
            "compression" => {
                if let Some(v) = read_le_u8(val_data, 0) {
                    let comp_str = match v {
                        0 => "None",
                        1 => "RLE",
                        2 => "ZIPS",
                        3 => "ZIP",
                        4 => "PIZ",
                        5 => "PXR24",
                        6 => "B44",
                        7 => "B44A",
                        8 => "DWAA",
                        9 => "DWAB",
                        _ => "Unknown",
                    };
                    tags.push(mk_with_print(
                        "Compression",
                        Value::U8(v),
                        comp_str.to_string(),
                    ));
                }
            }
            "dataWindow" => {
                if val_data.len() >= 16 {
                    let x1 =
                        i32::from_le_bytes([val_data[0], val_data[1], val_data[2], val_data[3]]);
                    let y1 =
                        i32::from_le_bytes([val_data[4], val_data[5], val_data[6], val_data[7]]);
                    let x2 =
                        i32::from_le_bytes([val_data[8], val_data[9], val_data[10], val_data[11]]);
                    let y2 = i32::from_le_bytes([
                        val_data[12],
                        val_data[13],
                        val_data[14],
                        val_data[15],
                    ]);
                    data_window = Some([x1, y1, x2, y2]);
                    tags.push(mk(
                        "DataWindow",
                        Value::String(format!("{} {} {} {}", x1, y1, x2, y2)),
                    ));
                }
            }
            "displayWindow" => {
                if val_data.len() >= 16 {
                    let x1 =
                        i32::from_le_bytes([val_data[0], val_data[1], val_data[2], val_data[3]]);
                    let y1 =
                        i32::from_le_bytes([val_data[4], val_data[5], val_data[6], val_data[7]]);
                    let x2 =
                        i32::from_le_bytes([val_data[8], val_data[9], val_data[10], val_data[11]]);
                    let y2 = i32::from_le_bytes([
                        val_data[12],
                        val_data[13],
                        val_data[14],
                        val_data[15],
                    ]);
                    if display_window.is_none() {
                        display_window = Some([x1, y1, x2, y2]);
                    }
                    tags.push(mk(
                        "DisplayWindow",
                        Value::String(format!("{} {} {} {}", x1, y1, x2, y2)),
                    ));
                }
            }
            "lineOrder" => {
                if let Some(v) = read_le_u8(val_data, 0) {
                    let lo_str = match v {
                        0 => "Increasing Y",
                        1 => "Decreasing Y",
                        2 => "Random Y",
                        _ => "Unknown",
                    };
                    tags.push(mk_with_print("LineOrder", Value::U8(v), lo_str.to_string()));
                }
            }
            "pixelAspectRatio" => {
                if val_data.len() >= 4 {
                    let v = f32::from_bits(u32::from_le_bytes([
                        val_data[0],
                        val_data[1],
                        val_data[2],
                        val_data[3],
                    ]));
                    // Format like Perl: integer if whole, else float
                    let s = if v.fract() == 0.0 && (0.0..1e9).contains(&v) {
                        format!("{}", v as i64)
                    } else {
                        format!("{}", v)
                    };
                    tags.push(mk("PixelAspectRatio", Value::String(s)));
                }
            }
            "screenWindowCenter" => {
                if val_data.len() >= 8 {
                    let x = f32::from_bits(u32::from_le_bytes([
                        val_data[0],
                        val_data[1],
                        val_data[2],
                        val_data[3],
                    ]));
                    let y = f32::from_bits(u32::from_le_bytes([
                        val_data[4],
                        val_data[5],
                        val_data[6],
                        val_data[7],
                    ]));
                    let xs = format_float(x);
                    let ys = format_float(y);
                    tags.push(mk(
                        "ScreenWindowCenter",
                        Value::String(format!("{} {}", xs, ys)),
                    ));
                }
            }
            "screenWindowWidth" if val_data.len() >= 4 => {
                let v = f32::from_bits(u32::from_le_bytes([
                    val_data[0],
                    val_data[1],
                    val_data[2],
                    val_data[3],
                ]));
                tags.push(mk("ScreenWindowWidth", Value::String(format_float(v))));
            }
            // SubDirectory attributes (EXIF and XMP blocks written by
            // PanoramaStudio) are not decoded here.
            "exif" | "xmp" => {}
            _ => {
                if let Some(tag) = generic_attribute(&attr_name, attr_type, val_data) {
                    tags.push(tag);
                }
            }
        }
    }

    // Calculate image dimensions from dataWindow (fallback to displayWindow)
    let dim = data_window.or(display_window);
    if let Some([x1, y1, x2, y2]) = dim {
        let w = (x2 - x1 + 1) as u32;
        let h = (y2 - y1 + 1) as u32;
        // `$et->FoundTag('ImageWidth', ...)` / `('ImageHeight', ...)`
        // (OpenEXR.pm lines 319-320): a bare NAME, so both resolve through
        // `%Image::ExifTool::Extra` — File/File/Image (ExifTool.pm lines
        // 1285 and 1670-1671) — not the OpenEXR table.
        let mk_file = |name: &str, v: u32| {
            let mut t = mk(name, Value::U32(v));
            t.group.family0 = "File".into();
            t.group.family1 = "File".into();
            t
        };
        tags.push(mk_file("ImageWidth", w));
        tags.push(mk_file("ImageHeight", h));
    }

    Ok(tags)
}

/// Name, family 2 group and PrintConv of the OpenEXR::Main entries that have
/// one (OpenEXR.pm lines 47-171). Every other attribute keeps its own name,
/// capitalized, in the table's default `Image` group.
fn table_entry(attr: &str) -> Option<(&'static str, &'static str, Option<&'static str>)> {
    Some(match attr {
        "altitude" => ("GPSAltitude", "Location", None),
        "aperture" => ("Aperture", "Image", Some("sprintf(\"%.1f\",$val)")),
        "capDate" => (
            "DateTimeOriginal",
            "Time",
            Some("$self->ConvertDateTime($val)"),
        ),
        "envmap" => ("EnvironmentMap", "Image", None),
        "expTime" => (
            "ExposureTime",
            "Image",
            Some("Image::ExifTool::Exif::PrintExposureTime($val)"),
        ),
        "focus" => ("FocusDistance", "Image", Some("\"$val m\"")),
        "isoSpeed" => ("ISO", "Image", None),
        "latitude" => (
            "GPSLatitude",
            "Location",
            Some("Image::ExifTool::GPS::ToDMS($self, $val, 1, \"N\")"),
        ),
        "longitude" => (
            "GPSLongitude",
            "Location",
            Some("Image::ExifTool::GPS::ToDMS($self, $val, 1, \"E\")"),
        ),
        "owner" => ("Owner", "Author", None),
        "preview" => ("Preview", "Preview", None),
        "utcOffset" => ("TimeZone", "Time", Some("TimeZoneString($val / 60)")),
        "wrapmodes" => ("WrapModes", "Image", None),
        "xDensity" => ("XResolution", "Image", None),
        _ => return None,
    })
}

/// The name ProcessEXR gives an attribute missing from its table: first
/// letter of each word capitalized, then only `-_a-zA-Z0-9` kept
/// (OpenEXR.pm lines 216-226).
fn generated_name(attr: &str) -> String {
    let mut name = String::with_capacity(attr.len());
    let mut prev: Option<char> = None;
    for c in attr.chars() {
        let up = match prev {
            None => true,
            Some(p) => !p.is_ascii_alphabetic() && c.is_ascii_lowercase(),
        };
        name.push(if up { c.to_ascii_uppercase() } else { c });
        prev = Some(c);
    }
    name.retain(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    match name.len() {
        0 => "Invalid".to_string(),
        1 => format!("Tag{name}"),
        _ => name,
    }
}

/// Decode an attribute through ProcessEXR's `%formatType` (OpenEXR.pm lines
/// 21-44): the types that map onto ExifTool formats are read as such, and any
/// other type is kept as binary data.
fn decode_value(attr_type: &str, d: &[u8]) -> Option<Value> {
    let word = |i: usize| d.get(i * 4..i * 4 + 4).map(|b| [b[0], b[1], b[2], b[3]]);
    let f32_at = |i: usize| word(i).map(|b| f32::from_le_bytes(b) as f64);
    let i32_at = |i: usize| word(i).map(i32::from_le_bytes);
    // ReadValue joins a multi-value read with spaces.
    let floats = |n: usize| -> Option<Value> {
        let v: Option<Vec<String>> = (0..n)
            .map(|i| f32_at(i).map(crate::value::format_g15))
            .collect();
        Some(Value::String(v?.join(" ")))
    };
    let ints = |n: usize, unsigned: bool| -> Option<Value> {
        let v: Option<Vec<String>> = (0..n)
            .map(|i| {
                word(i).map(|b| {
                    if unsigned {
                        u32::from_le_bytes(b).to_string()
                    } else {
                        i32::from_le_bytes(b).to_string()
                    }
                })
            })
            .collect();
        Some(Value::String(v?.join(" ")))
    };
    Some(match attr_type {
        "float" => Value::F64(f32_at(0)?),
        "double" => Value::F64(f64::from_le_bytes(d.get(..8)?.try_into().ok()?)),
        "int" => Value::I32(i32_at(0)?),
        "compression" | "envmap" | "lineOrder" => Value::U8(*d.first()?),
        "rational" => Value::IRational(i32_at(0)?, i32_at(1)?),
        "box2f" => floats(4)?,
        "chromaticities" => floats(8)?,
        "m33f" => floats(9)?,
        "m44f" => floats(16)?,
        "v2f" => floats(2)?,
        "v3f" => floats(3)?,
        "box2i" => ints(4, false)?,
        "keycode" => ints(7, false)?,
        "v2i" => ints(2, false)?,
        "v3i" => ints(3, false)?,
        "timecode" => ints(2, true)?,
        // ReadValue's `string` stops at the first null.
        "string" => {
            let end = d.iter().position(|&b| b == 0).unwrap_or(d.len());
            Value::String(String::from_utf8_lossy(&d[..end]).into_owned())
        }
        "stringvector" => {
            let mut items = Vec::new();
            let mut p = 0;
            while p + 4 <= d.len() {
                let len = u32::from_le_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]]) as usize;
                if len > d.len() - p - 4 {
                    break;
                }
                items.push(Value::String(
                    String::from_utf8_lossy(&d[p + 4..p + 4 + len]).into_owned(),
                ));
                p += 4 + len;
            }
            Value::List(items)
        }
        "tiledesc" => {
            if d.len() < 9 {
                return None;
            }
            let x = u32::from_le_bytes([d[0], d[1], d[2], d[3]]);
            let y = u32::from_le_bytes([d[4], d[5], d[6], d[7]]);
            let mode = d[8];
            let lvl = match mode & 0x0f {
                0 => "One Level".to_string(),
                1 => "MIMAP Levels".to_string(),
                2 => "RIPMAP Levels".to_string(),
                n => format!("Unknown Levels ({n})"),
            };
            let rnd = match mode >> 4 {
                0 => "Round Down".to_string(),
                1 => "Round Up".to_string(),
                n => format!("Unknown Rounding ({n})"),
            };
            Value::String(format!("{x}x{y}; {lvl}; {rnd}"))
        }
        _ => Value::Binary(d.to_vec()),
    })
}

/// An attribute without a dedicated decoder above: decoded by its type, named
/// and converted as the OpenEXR::Main table says.
fn generic_attribute(attr: &str, attr_type: &str, data: &[u8]) -> Option<Tag> {
    use crate::tags::conv_expr::{eval, Val};
    // An undecodable value is stored as '<bad>' (OpenEXR.pm line 293).
    let raw = decode_value(attr_type, data).unwrap_or_else(|| Value::String("<bad>".into()));
    let (name, group2, conv) = match table_entry(attr) {
        Some((n, g, c)) => (n.to_string(), g, c),
        None => (generated_name(attr), "Image", None),
    };
    let num = match &raw {
        Value::F64(_) | Value::I32(_) | Value::U8(_) | Value::IRational(..) => raw.as_f64(),
        _ => None,
    };
    let print = match (attr, conv, num) {
        // `$val = int($val * 10) / 10`, then the sign becomes Above/Below.
        ("altitude", _, Some(v)) => {
            let s = crate::value::format_g15((v * 10.0).trunc() / 10.0);
            match s.strip_prefix('-') {
                Some(abs) => format!("{abs} m Below Sea Level"),
                None => format!("{s} m Above Sea Level"),
            }
        }
        ("envmap", _, Some(v)) => match v as i64 {
            0 => "Latitude/Longitude".to_string(),
            1 => "Cube".to_string(),
            n => format!("Unknown ({n})"),
        },
        (_, Some(expr), _) => {
            let val = match num {
                Some(n) => Val::Num(n),
                None => Val::Str(raw.to_display_string()),
            };
            eval(expr, &val)
                .map(|v| v.as_string())
                .unwrap_or_else(|| raw.to_display_string())
        }
        _ => raw.to_display_string(),
    };
    let mut tag = mk_with_print(&name, raw, print);
    tag.group.family2 = group2.into();
    Some(tag)
}

fn format_float(v: f32) -> String {
    if v.fract() == 0.0 && v.abs() < 1e9 {
        format!("{}", v as i64)
    } else {
        format!("{}", v)
    }
}
