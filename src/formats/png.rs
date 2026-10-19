//! PNG file format reader.
//!
//! Parses PNG chunks to extract metadata: tEXt, iTXt, zTXt, eXIf, iCCP.
//! Mirrors ExifTool's PNG.pm.

use crate::error::{Error, Result};
use crate::metadata::ExifReader;
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

/// PNG magic signature.
const PNG_SIGNATURE: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

/// Extract all metadata tags from a PNG file.
pub fn read_png(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 8 || !data.starts_with(PNG_SIGNATURE) {
        return Err(Error::InvalidData("not a PNG file".into()));
    }

    let mut tags = Vec::new();
    let mut pos = 8; // skip signature
    let mut found_idat = false;
    let mut text_after_idat_count = 0usize;

    while pos + 12 <= data.len() {
        let chunk_len =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let chunk_type = &data[pos + 4..pos + 8];
        let chunk_data_start = pos + 8;
        let chunk_data_end = chunk_data_start + chunk_len;

        if chunk_data_end + 4 > data.len() {
            break;
        }

        let chunk_data = &data[chunk_data_start..chunk_data_end];

        match chunk_type {
            // IDAT - image data, track when we've seen it
            b"IDAT" => {
                found_idat = true;
            }
            // IHDR - Image header (always first chunk)
            b"IHDR" if chunk_len >= 13 => {
                let width = u32::from_be_bytes([
                    chunk_data[0],
                    chunk_data[1],
                    chunk_data[2],
                    chunk_data[3],
                ]);
                let height = u32::from_be_bytes([
                    chunk_data[4],
                    chunk_data[5],
                    chunk_data[6],
                    chunk_data[7],
                ]);
                let bit_depth = chunk_data[8];
                let color_type = chunk_data[9];

                tags.push(make_png_tag("ImageWidth", "Image Width", Value::U32(width)));
                tags.push(make_png_tag(
                    "ImageHeight",
                    "Image Height",
                    Value::U32(height),
                ));
                tags.push(make_png_tag("BitDepth", "Bit Depth", Value::U8(bit_depth)));
                tags.push(make_png_tag(
                    "ColorType",
                    "Color Type",
                    Value::String(
                        match color_type {
                            0 => "Grayscale",
                            2 => "RGB",
                            3 => "Palette",
                            4 => "Grayscale with Alpha",
                            6 => "RGB with Alpha",
                            _ => "Unknown",
                        }
                        .to_string(),
                    ),
                ));
                // Compression (byte 10), Filter (byte 11), Interlace (byte 12) — from Perl PNG.pm
                let compression = match chunk_data[10] {
                    0 => "Deflate/Inflate",
                    _ => "Unknown",
                };
                tags.push(make_png_tag(
                    "Compression",
                    "Compression",
                    Value::String(compression.into()),
                ));
                let filter = match chunk_data[11] {
                    0 => "Adaptive",
                    _ => "Unknown",
                };
                tags.push(make_png_tag(
                    "Filter",
                    "Filter",
                    Value::String(filter.into()),
                ));
                let interlace = match chunk_data[12] {
                    0 => "Noninterlaced",
                    1 => "Adam7 Interlace",
                    _ => "Unknown",
                };
                tags.push(make_png_tag(
                    "Interlace",
                    "Interlace",
                    Value::String(interlace.into()),
                ));
            }

            // bKGD - Background color
            b"bKGD" if !chunk_data.is_empty() => {
                let bg = if chunk_data.len() >= 6 {
                    format!(
                        "{} {} {}",
                        u16::from_be_bytes([chunk_data[0], chunk_data[1]]),
                        u16::from_be_bytes([chunk_data[2], chunk_data[3]]),
                        u16::from_be_bytes([chunk_data[4], chunk_data[5]])
                    )
                } else if chunk_data.len() >= 2 {
                    u16::from_be_bytes([chunk_data[0], chunk_data[1]]).to_string()
                } else {
                    chunk_data[0].to_string()
                };
                tags.push(make_png_tag(
                    "BackgroundColor",
                    "Background Color",
                    Value::String(bg),
                ));
            }

            // tEXt - Uncompressed text
            b"tEXt" => {
                if found_idat {
                    text_after_idat_count += 1;
                }
                if let Some(null_pos) = chunk_data.iter().position(|&b| b == 0) {
                    push_text(
                        &chunk_data[..null_pos],
                        &chunk_data[null_pos + 1..],
                        "",
                        false,
                        &mut tags,
                    );
                }
            }

            // zTXt - Compressed text: keyword, NUL, compression method, data
            b"zTXt" => {
                if found_idat {
                    text_after_idat_count += 1;
                }
                if let Some(null_pos) = chunk_data.iter().position(|&b| b == 0) {
                    let packed = chunk_data.get(null_pos + 2..).unwrap_or_default();
                    if let Some(text) = inflate(packed) {
                        push_text(&chunk_data[..null_pos], &text, "", false, &mut tags);
                    }
                }
            }

            // iTXt - International text (UTF-8)
            b"iTXt" => {
                if found_idat {
                    text_after_idat_count += 1;
                }
                parse_itxt_into(chunk_data, &mut tags);
            }

            // gAMA - Gamma: ExifTool reports the decoding gamma, the inverse
            // of the stored one (PNG.pm `int(1e9/$a+0.5)/1e4`).
            b"gAMA" if chunk_len >= 4 => {
                let a = u32::from_be_bytes([
                    chunk_data[0],
                    chunk_data[1],
                    chunk_data[2],
                    chunk_data[3],
                ]);
                let value = if a == 0 {
                    Value::U32(0)
                } else {
                    Value::F64((1e9 / f64::from(a) + 0.5).floor() / 1e4)
                };
                tags.push(make_png_tag("Gamma", "Gamma", value));
            }

            // iCCP - ICC profile: name, NUL, compression method, deflated profile
            b"iCCP" => {
                if let Some(null_pos) = chunk_data.iter().position(|&b| b == 0) {
                    let name = crate::encoding::decode_latin1(&chunk_data[..null_pos]);
                    tags.push(make_png_tag(
                        "ProfileName",
                        "Profile Name",
                        Value::String(name),
                    ));
                    let packed = chunk_data.get(null_pos + 2..).unwrap_or_default();
                    if let Some(profile) = inflate(packed) {
                        match crate::formats::icc::read_icc(&profile) {
                            Ok(icc_tags) => tags.extend(icc_tags),
                            Err(_) => tags.push(make_png_tag(
                                "ICC_Profile",
                                "ICC Profile",
                                Value::Binary(profile),
                            )),
                        }
                    }
                }
            }

            // eXIf - EXIF data (PNG 1.5+)
            b"eXIf" => {
                if let Ok(exif_tags) = ExifReader::read(chunk_data) {
                    tags.extend(exif_tags);
                }
            }

            // pHYs - Physical pixel dimensions
            b"pHYs" if chunk_len >= 9 => {
                let ppux = u32::from_be_bytes([
                    chunk_data[0],
                    chunk_data[1],
                    chunk_data[2],
                    chunk_data[3],
                ]);
                let ppuy = u32::from_be_bytes([
                    chunk_data[4],
                    chunk_data[5],
                    chunk_data[6],
                    chunk_data[7],
                ]);
                let unit = chunk_data[8];

                let unit_str = match unit {
                    1 => "meters",
                    _ => "unknown",
                };
                tags.push(make_png_phys_tag(
                    "PixelsPerUnitX",
                    "Pixels Per Unit X",
                    Value::U32(ppux),
                ));
                tags.push(make_png_phys_tag(
                    "PixelsPerUnitY",
                    "Pixels Per Unit Y",
                    Value::U32(ppuy),
                ));
                tags.push(make_png_phys_tag(
                    "PixelUnits",
                    "Pixel Units",
                    Value::String(unit_str.to_string()),
                ));
            }

            // tIME - Last modification time
            b"tIME" if chunk_len >= 7 => {
                let year = u16::from_be_bytes([chunk_data[0], chunk_data[1]]);
                let month = chunk_data[2];
                let day = chunk_data[3];
                let hour = chunk_data[4];
                let minute = chunk_data[5];
                let second = chunk_data[6];
                let date_str = format!(
                    "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
                    year, month, day, hour, minute, second
                );
                tags.push(make_png_tag(
                    "ModifyDate",
                    "Modify Date",
                    Value::String(date_str),
                ));
            }

            // sRGB chunk - sRGB rendering intent (1 byte)
            b"sRGB" if chunk_len >= 1 => {
                let intent = chunk_data[0];
                let intent_name = match intent {
                    0 => "Perceptual",
                    1 => "Relative Colorimetric",
                    2 => "Saturation",
                    3 => "Absolute Colorimetric",
                    _ => "Unknown",
                };
                tags.push(make_png_tag(
                    "SRGBRendering",
                    "sRGB Rendering",
                    Value::String(intent_name.to_string()),
                ));
            }

            // IEND - End of image
            b"IEND" => break,

            _ => {}
        }

        // Move past chunk data + 4-byte CRC
        pos = chunk_data_end + 4;
    }

    // Generate warning if text/EXIF chunks appeared after IDAT
    if text_after_idat_count > 0 {
        let warn_msg = if text_after_idat_count > 1 {
            format!("[minor] Text/EXIF chunk(s) found after PNG IDAT (may be ignored by some readers) [x{}]", text_after_idat_count)
        } else {
            "[minor] Text/EXIF chunk(s) found after PNG IDAT (may be ignored by some readers)"
                .to_string()
        };
        tags.push(crate::tag::warning_tag(warn_msg));
    }

    Ok(tags)
}

fn make_png_tag(name: &str, description: &str, value: Value) -> Tag {
    let print_value = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "PNG".to_string(),
            family1: "PNG".to_string(),
            family2: "Image".to_string(),
            family3: "Main".into(),
        },
        raw_value: value,
        print_value,
        priority: 0,
    }
}

/// A tag from the PNG `pHYs` chunk. ExifTool reads it with the `PNG::PhysicalPixel`
/// table (`GROUPS => { 1 => 'PNG-pHYs' }`, PNG.pm line 446), so it sits in
/// family 1 `PNG-pHYs`, not the container `PNG` group.
fn make_png_phys_tag(name: &str, description: &str, value: Value) -> Tag {
    let mut t = make_png_tag(name, description, value);
    t.group.family1 = "PNG-pHYs".to_string();
    t
}

/// The text tags of PNG.pm `TextualData` (the directories, XMP and raw
/// profiles, aside): chunk keyword and tag name.
pub(crate) const TEXT_TAGS: &[(&str, &str)] = &[
    ("Artist", "Artist"),
    ("Author", "Author"),
    ("Collection", "Collection"),
    ("Comment", "Comment"),
    ("Copyright", "Copyright"),
    ("Creation Time", "CreationTime"),
    ("Description", "Description"),
    ("Disclaimer", "Disclaimer"),
    ("Document", "Document"),
    ("Label", "Label"),
    ("Make", "Make"),
    ("Model", "Model"),
    ("Software", "Software"),
    ("Source", "Source"),
    ("TimeStamp", "TimeStamp"),
    ("Title", "Title"),
    ("Warning", "PNGWarning"),
    ("aesthetic_score", "AestheticScore"),
    ("create-date", "CreateDate"),
    ("modify-date", "ModDate"),
    ("parameters", "parameters"),
];

/// The name of the known text tag a keyword holds: ExifTool looks the
/// keyword up as is, then with its first letter capitalized (`FoundPNG`).
pub(crate) fn text_tag_name(key: &str) -> Option<&'static str> {
    let find = |k: &str| TEXT_TAGS.iter().find(|(kw, _)| *kw == k).map(|(_, n)| *n);
    find(key).or_else(|| {
        let mut c = key.chars();
        let first = c.next()?;
        find(&(first.to_uppercase().collect::<String>() + c.as_str()))
    })
}

fn make_png_text_tag(key: &str, value: &str) -> Tag {
    let unknown;
    let mapped_name = match text_tag_name(key) {
        Some(n) => n,
        None => {
            unknown = unknown_tag_name(key);
            unknown.as_str()
        }
    };
    Tag {
        id: TagId::Text(mapped_name.to_string()),
        name: mapped_name.to_string(),
        description: mapped_name.to_string(),
        group: TagGroup {
            family0: "PNG".to_string(),
            // `%Image::ExifTool::PNG::TextualData` (PNG.pm line 592) states
            // only `GROUPS => { 2 => 'Image' }`, so families 0 and 1 both fall
            // back to the module name, `PNG` — the tEXt/zTXt/iTXt chunk a tag
            // came out of is not a group of its own (unlike pHYs and cICP,
            // which do declare one).
            family1: "PNG".to_string(),
            family2: "Image".to_string(),
            family3: "Main".into(),
        },
        raw_value: Value::String(value.to_string()),
        print_value: value.to_string(),
        priority: 0,
    }
}

/// The name ExifTool gives an unknown keyword: white space removed with the
/// next letter capitalized (`s/\s+(.)/\u$1/g`), then `MakeTagName`.
fn unknown_tag_name(key: &str) -> String {
    let mut name = String::new();
    let mut up = false;
    for c in key.chars() {
        if c.is_whitespace() {
            up = true;
            continue;
        }
        if up {
            name.extend(c.to_uppercase());
            up = false;
        } else {
            name.push(c);
        }
    }
    let mut name: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if let Some(f) = name.get(..1) {
        name = f.to_ascii_uppercase() + &name[1..];
    }
    if name.len() < 2 || name.starts_with(|c: char| c == '-' || c.is_ascii_digit()) {
        name = format!("Tag{name}");
    }
    name
}

fn parse_itxt_into(data: &[u8], tags: &mut Vec<Tag>) {
    // iTXt: keyword\0 compression_flag\0 compression_method\0 language\0 translated_keyword\0 text
    let null_pos = match data.iter().position(|&b| b == 0) {
        Some(p) => p,
        None => return,
    };
    let key = crate::encoding::decode_utf8_or_latin1(&data[..null_pos]).to_string();

    let rest = &data[null_pos + 1..];
    if rest.len() < 2 {
        return;
    }

    let compressed = rest[0] != 0;
    let rest = &rest[2..];

    // Language tag
    let null_pos = match rest.iter().position(|&b| b == 0) {
        Some(p) => p,
        None => return,
    };
    let lang = crate::encoding::decode_latin1(&rest[..null_pos]);
    let rest = &rest[null_pos + 1..];

    // Skip translated keyword
    let null_pos = match rest.iter().position(|&b| b == 0) {
        Some(p) => p,
        None => return,
    };
    let text_slice = &rest[null_pos + 1..];
    let inflated;
    let text_slice = if compressed {
        match inflate(text_slice) {
            Some(t) => {
                inflated = t;
                &inflated[..]
            }
            None => return,
        }
    } else {
        text_slice
    };
    push_text(key.as_bytes(), text_slice, &lang, true, tags);
}

/// A tEXt, zTXt or iTXt value, as ExifTool's `FoundPNG` files it: the
/// `XML:com.adobe.xmp` keyword holds an XMP packet, other keywords a text
/// tag. A language code makes the XMP keyword an unknown tag (`GetLangInfo`
/// has no alternate-language form of a directory), named like any unknown
/// keyword. tEXt and zTXt are Latin-1, iTXt is UTF-8 (`utf8`).
fn push_text(key: &[u8], value: &[u8], lang: &str, utf8: bool, tags: &mut Vec<Tag>) {
    if key == b"XML:com.adobe.xmp" && lang.is_empty() {
        if let Ok(xmp_tags) = crate::metadata::XmpReader::read(value) {
            tags.extend(xmp_tags);
        }
        return;
    }
    if let Some(kind) = key.strip_prefix(b"Raw profile type ") {
        if lang.is_empty() && RAW_PROFILES.contains(&kind) {
            read_raw_profile(kind, value, tags);
            return;
        }
    }
    let decode = |b: &[u8]| {
        if utf8 {
            crate::encoding::decode_utf8_or_latin1(b).to_string()
        } else {
            crate::encoding::decode_latin1(b)
        }
    };
    let key = decode(key);
    let mut tag = make_png_text_tag(&key, &decode(value));
    if key.starts_with("Raw profile type ") {
        // An unknown profile is binary data (`$$tagInfo{Binary}`).
        tag.raw_value = Value::Binary(value.to_vec());
        tag.print_value = tag.raw_value.to_display_string();
    } else if !lang.is_empty() && text_tag_name(&key).is_some() {
        // A known tag in another language: `Title-fr` (PNG.pm
        // `StandardLangCase`: the second subtag upper-case when it has two
        // letters).
        let lang = standard_lang_case(lang);
        tag.name = format!("{}-{lang}", tag.name);
        tag.id = TagId::Text(tag.name.clone());
    }
    tags.push(tag);
}

/// PNG.pm `StandardLangCase`: lower case, but for a two-letter second
/// subtag after a primary one of 2-3 letters (or `x`/`i`), in upper case.
fn standard_lang_case(lang: &str) -> String {
    let lc = lang.to_ascii_lowercase();
    if let Some((first, rest)) = lc.split_once('-') {
        let primary = (2..=3).contains(&first.len())
            && first.bytes().all(|c| c.is_ascii_alphabetic())
            || first == "x"
            || first == "i";
        let b = rest.as_bytes();
        let region = b.len() >= 2
            && b[..2].iter().all(u8::is_ascii_alphabetic)
            && b.get(2)
                .is_none_or(|c| !(c.is_ascii_alphanumeric() || *c == b'_'));
        if primary && region {
            return format!("{first}-{}{}", rest[..2].to_ascii_uppercase(), &rest[2..]);
        }
    }
    lc
}

/// The raw profiles ExifTool decodes (PNG.pm `TextualData`).
const RAW_PROFILES: &[&[u8]] = &[b"APP1", b"exif", b"icc", b"icm", b"iptc", b"xmp", b"8bim"];

/// PNG.pm `ProcessProfile`: a hex-encoded profile (`\n<type>\n<length>\n<hex>`),
/// read as the directory it holds. IPTC and Photoshop profiles are not read.
fn read_raw_profile(kind: &[u8], text: &[u8], tags: &mut Vec<Tag>) {
    let Some(rest) = text.strip_prefix(b"\n") else {
        return;
    };
    let Some(nl) = rest.iter().position(|&b| b == b'\n') else {
        return;
    };
    let rest = &rest[nl + 1..];
    let start = rest
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(rest.len());
    let rest = &rest[start..];
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 || rest.get(digits) != Some(&b'\n') {
        return;
    }
    let hex: Vec<u8> = rest[digits + 1..]
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    let nibble = |c: u8| (c as char).to_digit(16).unwrap_or(0) as u8;
    let data: Vec<u8> = hex
        .chunks(2)
        .map(|p| nibble(p[0]) << 4 | p.get(1).map_or(0, |&c| nibble(c)))
        .collect();
    match kind {
        b"exif" | b"APP1" => {
            if let Some(xmp) = data.strip_prefix(b"http://ns.adobe.com/xap/1.0/\0") {
                if let Ok(t) = crate::metadata::XmpReader::read(xmp) {
                    tags.extend(t);
                }
                return;
            }
            let tiff = data.strip_prefix(b"Exif\0\0").unwrap_or(&data);
            if tiff.starts_with(b"MM\0\x2a") || tiff.starts_with(b"II\x2a\0") {
                if let Ok(t) = ExifReader::read(tiff) {
                    tags.extend(t);
                }
            }
        }
        b"xmp" => {
            if let Ok(t) = crate::metadata::XmpReader::read(&data) {
                tags.extend(t);
            }
        }
        b"icc" | b"icm" => {
            if let Ok(t) = crate::formats::icc::read_icc(&data) {
                tags.extend(t);
            }
        }
        _ => {}
    }
}

/// A zlib stream, inflated.
fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(data)
        .read_to_end(&mut out)
        .ok()?;
    Some(out)
}

/// Is `name` a PNG tag that `PNG:all` leaves alone? `PNG:all` deletes the
/// textual tags and the writable chunks (tIME, gAMA, sRGB); the image header,
/// the background colour and the iCCP profile name stay.
pub(crate) fn kept_by_png_all(name: &str) -> bool {
    matches!(
        name,
        "ImageWidth"
            | "ImageHeight"
            | "BitDepth"
            | "ColorType"
            | "Compression"
            | "Filter"
            | "Interlace"
            | "BackgroundColor"
            | "ProfileName"
            | "ICC_Profile"
    )
}
