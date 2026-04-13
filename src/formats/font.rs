//! Font file reader (TrueType, OpenType, WOFF, WOFF2).
//!
//! Extracts font name table entries.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

pub fn read_font(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 12 {
        return Err(Error::InvalidData("file too small for font".into()));
    }

    let mut tags = Vec::new();

    // Detect font type
    if data.starts_with(b"wOFF") || data.starts_with(b"wOF2") {
        return read_woff(data);
    }
    if data.starts_with(b"ttcf") {
        // TTC: process each font in the collection
        if data.len() < 12 {
            return Err(Error::InvalidData("TTC too small".into()));
        }
        let num_fonts = u32::from_be_bytes([data[8], data[9], data[10], data[11]]) as usize;
        tags.push(mk("NumFonts", "Num Fonts", Value::U32(num_fonts as u32)));
        // Offsets of each font start at byte 12
        for i in 0..num_fonts.min(64) {
            let off_pos = 12 + i * 4;
            if off_pos + 4 > data.len() {
                break;
            }
            let font_off = u32::from_be_bytes([
                data[off_pos],
                data[off_pos + 1],
                data[off_pos + 2],
                data[off_pos + 3],
            ]) as usize;
            if font_off + 12 <= data.len() {
                parse_otf_font(&data[font_off..], 0, &mut tags);
            }
        }
        return Ok(tags);
    }

    // Check for DFONT (Mac resource fork with sfnt resources)
    if data.len() >= 16 {
        let dat_off = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let map_off = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
        let dat_len = u32::from_be_bytes([data[8], data[9], data[10], data[11]]) as usize;
        let map_len = u32::from_be_bytes([data[12], data[13], data[14], data[15]]) as usize;
        // Validate it looks like a RSRC (dfont) file
        if dat_off >= 0x10
            && map_off >= 0x10
            && dat_off + dat_len <= data.len()
            && map_off + map_len <= data.len()
            && map_len >= 30
            && (dat_off == 0x100
                || (dat_off as u64 + dat_len as u64 == map_off as u64)
                || (map_off as u64 + map_len as u64 <= data.len() as u64))
        {
            if read_dfont(data, &mut tags).is_ok() && !tags.is_empty() {
                return Ok(tags);
            }
        }
    }

    // OTF/TTF
    parse_otf_font(data, 0, &mut tags);
    Ok(tags)
}

/// Parse a single OTF/TTF font (not a TTC, dfont, or WOFF).
fn parse_otf_font(data: &[u8], _base: usize, tags: &mut Vec<Tag>) {
    if data.len() < 12 {
        return;
    }
    // Verify it starts with a known sfnt signature
    if !data.starts_with(b"OTTO")
        && !data.starts_with(&[0x00, 0x01, 0x00, 0x00])
        && !data.starts_with(b"true")
        && !data.starts_with(b"typ1")
        && !data.starts_with(&[0xa5, b'k', b'b', b'd'])
        && !data.starts_with(&[0xa5, b'l', b's', b't'])
    {
        return;
    }
    let num_tables = u16::from_be_bytes([data[4], data[5]]) as usize;
    if num_tables > 256 {
        return;
    }
    let mut pos = 12;
    for _ in 0..num_tables {
        if pos + 16 > data.len() {
            break;
        }
        let tbl_tag = &data[pos..pos + 4];
        let offset =
            u32::from_be_bytes([data[pos + 8], data[pos + 9], data[pos + 10], data[pos + 11]])
                as usize;
        let length = u32::from_be_bytes([
            data[pos + 12],
            data[pos + 13],
            data[pos + 14],
            data[pos + 15],
        ]) as usize;
        pos += 16;
        if tbl_tag == b"name" && offset + length <= data.len() {
            parse_name_table(&data[offset..offset + length], tags);
        }
    }
}

/// Read a DFONT (Mac resource fork) file and extract font name tags.
/// The dfont file is a Mac OS resource fork file containing sfnt resources.
pub fn read_dfont(data: &[u8], tags: &mut Vec<Tag>) -> Result<()> {
    if data.len() < 30 {
        return Err(Error::InvalidData("dfont too small".into()));
    }
    let dat_off = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let map_off = u32::from_be_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let _dat_len = u32::from_be_bytes([data[8], data[9], data[10], data[11]]) as usize;
    let map_len = u32::from_be_bytes([data[12], data[13], data[14], data[15]]) as usize;

    if map_off + map_len > data.len() || map_len < 30 {
        return Err(Error::InvalidData("invalid dfont map".into()));
    }
    let map = &data[map_off..map_off + map_len];
    // type_off: offset from start of map to type list
    let type_off = u16::from_be_bytes([map[24], map[25]]) as usize;
    let name_off = u16::from_be_bytes([map[26], map[27]]) as usize;
    let num_types = ((u16::from_be_bytes([map[28], map[29]]) as usize) + 1) & 0xffff;

    if type_off < 28 || name_off < 30 {
        return Err(Error::InvalidData("bad offsets".into()));
    }

    // Parse type list
    for i in 0..num_types {
        let off = type_off + 2 + 8 * i;
        if off + 8 > map_len {
            break;
        }
        let res_type = &map[off..off + 4];
        let res_num = (u16::from_be_bytes([map[off + 4], map[off + 5]]) as usize) + 1;
        let ref_off = (u16::from_be_bytes([map[off + 6], map[off + 7]]) as usize) + type_off;

        // Only process 'sfnt' and 'vers' resources
        if res_type == b"sfnt" {
            for j in 0..res_num {
                let roff = ref_off + 12 * j;
                if roff + 12 > map_len {
                    break;
                }
                // bytes 5-7 of reference entry are the 3-byte data offset (byte 4 is attributes)
                let res_data_off =
                    (u32::from_be_bytes([0, map[roff + 5], map[roff + 6], map[roff + 7]]) as usize)
                        + dat_off;
                if res_data_off + 4 > data.len() {
                    continue;
                }
                let res_data_len = u32::from_be_bytes([
                    data[res_data_off],
                    data[res_data_off + 1],
                    data[res_data_off + 2],
                    data[res_data_off + 3],
                ]) as usize;
                let font_start = res_data_off + 4;
                if font_start + res_data_len <= data.len() {
                    parse_otf_font(&data[font_start..font_start + res_data_len], 0, tags);
                }
            }
        } else if res_type == b"vers" {
            for j in 0..res_num {
                let roff = ref_off + 12 * j;
                if roff + 12 > map_len {
                    break;
                }
                // bytes 5-7 of reference entry are the 3-byte data offset (byte 4 is attributes)
                let res_data_off =
                    (u32::from_be_bytes([0, map[roff + 5], map[roff + 6], map[roff + 7]]) as usize)
                        + dat_off;
                if res_data_off + 4 > data.len() {
                    continue;
                }
                let res_data_len = u32::from_be_bytes([
                    data[res_data_off],
                    data[res_data_off + 1],
                    data[res_data_off + 2],
                    data[res_data_off + 3],
                ]) as usize;
                let payload_start = res_data_off + 4;
                if payload_start + res_data_len > data.len() || res_data_len < 8 {
                    continue;
                }
                let vers_data = &data[payload_start..payload_start + res_data_len];
                // 'vers' resource: short version (4 bytes), country code (2 bytes), short string (1 byte + N), long string (1 byte + N)
                let short_len = vers_data[6] as usize;
                let p = 7 + short_len;
                if p + 1 > vers_data.len() {
                    continue;
                }
                let long_len = vers_data[p] as usize;
                let p2 = p + 1;
                if p2 + long_len <= vers_data.len() && long_len > 0 {
                    let ver_str =
                        crate::encoding::decode_utf8_or_latin1(&vers_data[p2..p2 + long_len])
                            .to_string();
                    tags.push(mk(
                        "ApplicationVersion",
                        "Application Version",
                        Value::String(ver_str),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn read_woff(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 44 {
        return Err(Error::InvalidData("file too small for WOFF".into()));
    }

    let mut tags = Vec::new();
    tags.push(mk(
        "FontFormat",
        "Font Format",
        Value::String("WOFF".into()),
    ));

    let flavor = &data[4..8];
    if flavor == b"OTTO" {
        tags.push(mk(
            "FontFlavor",
            "Font Flavor",
            Value::String("OpenType/CFF".into()),
        ));
    } else {
        tags.push(mk(
            "FontFlavor",
            "Font Flavor",
            Value::String("TrueType".into()),
        ));
    }

    let _num_tables = u16::from_be_bytes([data[12], data[13]]) as usize;
    let _total_size = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let major = u16::from_be_bytes([data[20], data[21]]);
    let minor = u16::from_be_bytes([data[22], data[23]]);

    tags.push(mk(
        "Version",
        "Version",
        Value::String(format!("{}.{}", major, minor)),
    ));

    Ok(tags)
}

fn parse_name_table(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 6 {
        return;
    }

    let format = u16::from_be_bytes([data[0], data[1]]);
    let count = u16::from_be_bytes([data[2], data[3]]) as usize;
    let string_offset = u16::from_be_bytes([data[4], data[5]]) as usize;
    let mut pos = 6;

    // For format 1: parse language-tag records after the name records
    let mut lang_tag_map: std::collections::HashMap<u16, String> = std::collections::HashMap::new();
    if format == 1 && 6 + count * 12 + 2 <= data.len() {
        let lang_tag_count_pos = 6 + count * 12;
        let lang_tag_count =
            u16::from_be_bytes([data[lang_tag_count_pos], data[lang_tag_count_pos + 1]]) as usize;
        let mut lt_pos = lang_tag_count_pos + 2;
        for i in 0..lang_tag_count {
            if lt_pos + 4 > data.len() {
                break;
            }
            let lang_len = u16::from_be_bytes([data[lt_pos], data[lt_pos + 1]]) as usize;
            let lang_str_off = u16::from_be_bytes([data[lt_pos + 2], data[lt_pos + 3]]) as usize;
            lt_pos += 4;
            let abs = string_offset + lang_str_off;
            if abs + lang_len <= data.len() && lang_len % 2 == 0 {
                let units: Vec<u16> = data[abs..abs + lang_len]
                    .chunks_exact(2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect();
                let lang_str = String::from_utf16_lossy(&units);
                // lang_tag IDs start at 0x8000
                lang_tag_map.insert(0x8000 + i as u16, lang_str.to_string());
            }
        }
    }

    // Track which base tag names already have their default ('en') entry
    let mut seen_tags: std::collections::HashMap<String, bool> = std::collections::HashMap::new();

    for _ in 0..count {
        if pos + 12 > data.len() {
            break;
        }

        let platform_id = u16::from_be_bytes([data[pos], data[pos + 1]]);
        let encoding_id = u16::from_be_bytes([data[pos + 2], data[pos + 3]]);
        let language_id = u16::from_be_bytes([data[pos + 4], data[pos + 5]]);
        let name_id = u16::from_be_bytes([data[pos + 6], data[pos + 7]]);
        let length = u16::from_be_bytes([data[pos + 8], data[pos + 9]]) as usize;
        let offset = u16::from_be_bytes([data[pos + 10], data[pos + 11]]) as usize;
        pos += 12;

        let abs_offset = string_offset + offset;
        if abs_offset + length > data.len() {
            continue;
        }

        // Decode text based on platform/encoding
        let text = decode_font_name_string(data, abs_offset, length, platform_id, encoding_id);
        if text.is_empty() {
            continue;
        }

        // Get tag name and description
        let (base_name, desc) = match name_id {
            0 => ("Copyright", "Copyright"),
            1 => ("FontFamily", "Font Family"),
            2 => ("FontSubfamily", "Font Subfamily"),
            3 => ("FontSubfamilyID", "Unique ID"),
            4 => ("FontName", "Font Name"),
            5 => ("NameTableVersion", "Name Table Version"),
            6 => ("PostScriptFontName", "PostScript Font Name"),
            7 => ("Trademark", "Trademark"),
            8 => ("Manufacturer", "Manufacturer"),
            9 => ("Designer", "Designer"),
            10 => ("Description", "Description"),
            11 => ("VendorURL", "Vendor URL"),
            12 => ("DesignerURL", "Designer URL"),
            13 => ("License", "License"),
            14 => ("LicenseInfoURL", "License Info URL"),
            16 => ("PreferredFamily", "Preferred Family"),
            17 => ("PreferredSubfamily", "Preferred Subfamily"),
            18 => ("CompatibleFontName", "Compatible Font Name"),
            19 => ("SampleText", "Sample Text"),
            20 => ("PostScriptFontName", "PostScript Font Name"),
            21 => ("WWSFamilyName", "WWS Family Name"),
            22 => ("WWSSubfamilyName", "WWS Subfamily Name"),
            _ => continue,
        };

        // Get language code
        let lang_code = get_font_language_code(platform_id, language_id, &lang_tag_map);

        // Construct tag name with language suffix if not default
        let tag_name = if lang_code.is_empty() || lang_code == "en" {
            base_name.to_string()
        } else {
            format!("{}-{}", base_name, lang_code)
        };

        // Avoid duplicates
        if !seen_tags.contains_key(&tag_name) {
            seen_tags.insert(tag_name.clone(), true);
            let desc_full = if lang_code.is_empty() || lang_code == "en" {
                desc.to_string()
            } else {
                format!("{} ({})", desc, lang_code)
            };
            tags.push(mk(&tag_name, &desc_full, Value::String(text)));
        }
    }
}

/// Decode a font name string based on platform and encoding.
fn decode_font_name_string(
    data: &[u8],
    offset: usize,
    length: usize,
    platform: u16,
    _encoding: u16,
) -> String {
    if offset + length > data.len() {
        return String::new();
    }
    let raw = &data[offset..offset + length];
    match platform {
        3 => {
            // Windows: UTF-16 BE
            let units: Vec<u16> = raw
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&units).trim().to_string()
        }
        0 => {
            // Unicode: UTF-16 BE
            let units: Vec<u16> = raw
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&units).trim().to_string()
        }
        1 => {
            // Macintosh: encoding depends on encoding_id
            // For encoding 0 (MacRoman), treat as Latin-1
            crate::encoding::decode_utf8_or_latin1(raw)
                .trim()
                .to_string()
        }
        _ => crate::encoding::decode_utf8_or_latin1(raw)
            .trim()
            .to_string(),
    }
}

/// Get language code for a font name table entry.
fn get_font_language_code(
    platform: u16,
    language_id: u16,
    lang_tag_map: &std::collections::HashMap<u16, String>,
) -> String {
    // Check custom language tag map first
    if let Some(lang) = lang_tag_map.get(&language_id) {
        return lang.clone();
    }

    match platform {
        1 => {
            // Macintosh language codes
            match language_id {
                0 => "en".into(),
                1 => "fr".into(),
                2 => "de".into(),
                3 => "it".into(),
                4 => "nl-NL".into(),
                5 => "sv".into(),
                6 => "es".into(),
                7 => "da".into(),
                8 => "pt".into(),
                9 => "no".into(),
                10 => "he".into(),
                11 => "ja".into(),
                12 => "ar".into(),
                13 => "fi".into(),
                14 => "el".into(),
                15 => "is".into(),
                16 => "mt".into(),
                17 => "tr".into(),
                18 => "hr".into(),
                19 => "zh-TW".into(),
                20 => "ur".into(),
                21 => "hi".into(),
                22 => "th".into(),
                23 => "ko".into(),
                24 => "lt".into(),
                25 => "pl".into(),
                26 => "hu".into(),
                27 => "et".into(),
                28 => "lv".into(),
                33 => "zh-CN".into(),
                _ => format!("{}", language_id),
            }
        }
        3 => {
            // Windows language codes (matching Perl's Font.pm %ttLang table)
            match language_id {
                0x0401 => "ar-SA".into(),
                0x0402 => "bg".into(),
                0x0403 => "ca".into(),
                0x0404 => "zh-TW".into(),
                0x0405 => "cs".into(),
                0x0406 => "da".into(),
                0x0407 => "de-DE".into(),
                0x0408 => "el".into(),
                0x0409 => "en-US".into(),
                0x040a => "es-ES".into(),
                0x040b => "fi".into(),
                0x040c => "fr-FR".into(),
                0x040d => "he".into(),
                0x040e => "hu".into(),
                0x040f => "is".into(),
                0x0410 => "it-IT".into(),
                0x0411 => "ja".into(),
                0x0412 => "ko".into(),
                0x0413 => "nl-NL".into(),
                0x0414 => "no-NO".into(),
                0x0415 => "pl".into(),
                0x0416 => "pt-BR".into(),
                0x0417 => "rm".into(),
                0x0418 => "ro".into(),
                0x0419 => "ru".into(),
                0x041a => "hr".into(),
                0x041b => "sk".into(),
                0x041c => "sq".into(),
                0x041d => "sv-SE".into(),
                0x041e => "th".into(),
                0x041f => "tr".into(),
                0x0420 => "ur".into(),
                0x0421 => "id".into(),
                0x0422 => "uk".into(),
                0x0423 => "be".into(),
                0x0424 => "sl".into(),
                0x0425 => "et".into(),
                0x0426 => "lv".into(),
                0x0427 => "lt".into(),
                0x0429 => "fa".into(),
                0x042a => "vi".into(),
                0x042d => "eu".into(),
                0x042f => "mk".into(),
                0x0436 => "af".into(),
                0x0438 => "fo".into(),
                0x0439 => "hi".into(),
                0x043e => "ms-MY".into(),
                0x0441 => "sw".into(),
                0x0445 => "bn-IN".into(),
                0x0447 => "gu".into(),
                0x0449 => "ta".into(),
                0x044a => "te".into(),
                0x044b => "kn".into(),
                0x044c => "ml".into(),
                0x044e => "mr".into(),
                0x044f => "sa".into(),
                0x0450 => "mn-MN".into(),
                0x0456 => "gl".into(),
                0x045a => "syr".into(),
                0x0804 => "zh-CN".into(),
                0x0807 => "de-CH".into(),
                0x0809 => "en-GB".into(),
                0x080a => "es-MX".into(),
                0x080c => "fr-BE".into(),
                0x0810 => "it-CH".into(),
                0x0813 => "nl-BE".into(),
                0x0814 => "nn".into(),
                0x0816 => "pt-PT".into(),
                0x0c01 => "ar-EG".into(),
                0x0c04 => "zh-HK".into(),
                0x0c07 => "de-AT".into(),
                0x0c09 => "en-AU".into(),
                0x0c0a => "es-ES".into(),
                0x0c0c => "fr-CA".into(),
                0x1001 => "ar-LY".into(),
                0x1009 => "en-CA".into(),
                0x100a => "es-GT".into(),
                0x100c => "fr-CH".into(),
                0x1401 => "ar-DZ".into(),
                0x1409 => "en-NZ".into(),
                0x140a => "es-CR".into(),
                0x140c => "fr-LU".into(),
                0x1801 => "ar-MA".into(),
                0x1809 => "en-IE".into(),
                0x180a => "es-PA".into(),
                0x180c => "fr-MC".into(),
                0x1c01 => "ar-TN".into(),
                0x1c09 => "en-ZA".into(),
                0x1c0a => "es-DO".into(),
                0x2001 => "ar-OM".into(),
                0x2009 => "en-JM".into(),
                0x200a => "es-VE".into(),
                0x2401 => "ar-YE".into(),
                0x2409 => "en-CB".into(),
                0x240a => "es-CO".into(),
                0x2801 => "ar-SY".into(),
                0x2809 => "en-BZ".into(),
                0x280a => "es-PE".into(),
                0x2c01 => "ar-JO".into(),
                0x2c09 => "en-TT".into(),
                0x2c0a => "es-AR".into(),
                0x3001 => "ar-LB".into(),
                0x3009 => "en-ZW".into(),
                0x300a => "es-EC".into(),
                0x3401 => "ar-KW".into(),
                0x3409 => "en-PH".into(),
                0x340a => "es-CL".into(),
                0x3801 => "ar-AE".into(),
                0x380a => "es-UY".into(),
                0x3c01 => "ar-BH".into(),
                0x3c0a => "es-PY".into(),
                0x4001 => "ar-QA".into(),
                0x400a => "es-BO".into(),
                0x440a => "es-SV".into(),
                0x480a => "es-HN".into(),
                0x4c0a => "es-NI".into(),
                0x500a => "es-PR".into(),
                _ => String::new(),
            }
        }
        0 => "en".into(), // Unicode platform - typically English
        _ => String::new(),
    }
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "Font".into(),
            family1: "Font".into(),
            family2: "Other".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}

/// Read PostScript Type 1 ASCII font (.pfa) file.
/// Mirrors ExifTool's PostScript.pm + Font.pm PSInfo handling.
pub fn read_pfa(data: &[u8]) -> Result<Vec<Tag>> {
    // Must start with %!PS-AdobeFont or similar (check bytes directly)
    if !data.starts_with(b"%!PS-AdobeFont") && !data.starts_with(b"%!FontType1") {
        return Err(Error::InvalidData("not a PFA file".into()));
    }

    // Take only the text portion (until we hit binary data)
    // Find the first non-text byte or use the full file if all text
    let text_end = data.iter().position(|&b| b == 0x80).unwrap_or(data.len());
    let text_data = &data[..text_end];
    let text = crate::encoding::decode_utf8_or_latin1(text_data);

    let mut tags = Vec::new();
    let mut comment_parts: Vec<String> = Vec::new();
    let mut in_font_info = false;
    let mut comment_done = false;

    for line in text.lines() {
        // DSC comments: %% prefix
        if line.starts_with("%%") {
            if let Some(rest) = line.strip_prefix("%%Title: ") {
                tags.push(mk("Title", "Title", Value::String(rest.trim().to_string())));
            } else if let Some(rest) = line.strip_prefix("%%CreationDate: ") {
                tags.push(mk(
                    "CreateDate",
                    "Create Date",
                    Value::String(rest.trim().to_string()),
                ));
            } else if let Some(rest) = line.strip_prefix("%%Creator: ") {
                tags.push(mk(
                    "Creator",
                    "Creator",
                    Value::String(rest.trim().to_string()),
                ));
            } else if line.starts_with("%%EndComments") {
            }
            continue;
        }

        // Single % comment (only before EndComments / first non-comment)
        if line.starts_with('%') && !comment_done {
            let rest = &line[1..].trim_start();
            if !rest.is_empty() {
                comment_parts.push(rest.to_string());
            }
            continue;
        }

        // Non-comment line: stop accumulating comments if we haven't already
        if !line.starts_with('%') && !comment_done && !comment_parts.is_empty() {
            comment_done = true;
        }

        // Detect FontInfo begin/end
        if line.contains("FontInfo") && (line.contains("begin") || line.contains("dict begin")) {
            in_font_info = true;
        }
        if line.contains("currentdict end") || line.contains("end\n") || line.trim() == "end" {
            if in_font_info {
                in_font_info = false;
            }
        }

        // Parse /key value lines (both inside and outside FontInfo for top-level attrs)
        if line.contains('/') {
            let line_trimmed = line.trim();
            if let Some(rest) = line_trimmed.strip_prefix('/') {
                // Parse /Key value
                if let Some((key, val_part)) = rest.split_once(|c: char| c == ' ' || c == '\t') {
                    let val = val_part.trim();
                    let val_str = if val.starts_with('(') && val.contains(')') {
                        // PostScript string literal (value)
                        let inner = val.trim_start_matches('(');
                        if let Some(end) = inner.rfind(')') {
                            unescape_postscript(&inner[..end]).to_string()
                        } else {
                            inner.to_string()
                        }
                    } else if val.starts_with('/') {
                        // /Key /Value
                        val[1..].split_whitespace().next().unwrap_or("").to_string()
                    } else {
                        // /Key value (number, boolean)
                        val.split_whitespace().next().unwrap_or("").to_string()
                    };

                    // Map key to tag (PSInfo table)
                    match key {
                        "FontName" => {
                            tags.push(mk("FontName", "Font Name", Value::String(val_str)))
                        }
                        "FontType" => {
                            tags.push(mk("FontType", "Font Type", Value::String(val_str)))
                        }
                        "FullName" => {
                            tags.push(mk("FullName", "Full Name", Value::String(val_str)))
                        }
                        "FamilyName" => {
                            tags.push(mk("FontFamily", "Font Family", Value::String(val_str)))
                        }
                        "Weight" => tags.push(mk("Weight", "Weight", Value::String(val_str))),
                        "Notice" => tags.push(mk("Notice", "Notice", Value::String(val_str))),
                        "version" => tags.push(mk("Version", "Version", Value::String(val_str))),
                        "FSType" => tags.push(mk("FSType", "FS Type", Value::String(val_str))),
                        "ItalicAngle" => {
                            tags.push(mk("ItalicAngle", "Italic Angle", Value::String(val_str)))
                        }
                        "isFixedPitch" => {
                            tags.push(mk("IsFixedPitch", "Is Fixed Pitch", Value::String(val_str)))
                        }
                        "UnderlinePosition" => tags.push(mk(
                            "UnderlinePosition",
                            "Underline Position",
                            Value::String(val_str),
                        )),
                        "UnderlineThickness" => tags.push(mk(
                            "UnderlineThickness",
                            "Underline Thickness",
                            Value::String(val_str),
                        )),
                        _ => {}
                    }
                }
            }
        }
    }

    // Add accumulated comment
    if !comment_parts.is_empty() {
        let combined = comment_parts.join(".."); // ExifTool joins with ".." separator
        tags.push(mk("Comment", "Comment", Value::String(combined)));
    }

    Ok(tags)
}

fn unescape_postscript(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some('t') => result.push('\t'),
                Some('b') => result.push('\x08'),
                Some('f') => result.push('\x0c'),
                Some('\\') => result.push('\\'),
                Some('(') => result.push('('),
                Some(')') => result.push(')'),
                Some(c) => {
                    result.push('\\');
                    result.push(c);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Read Adobe Font Metrics (.afm) text file.
/// Mirrors ExifTool's Font.pm AFM handling.
pub fn read_afm(data: &[u8]) -> Result<Vec<Tag>> {
    let text = std::str::from_utf8(data).map_err(|_| Error::InvalidData("AFM not UTF-8".into()))?;

    // Must start with StartFontMetrics
    if !text.starts_with("StartFontMetrics") {
        return Err(Error::InvalidData("not an AFM file".into()));
    }

    let mut tags = Vec::new();
    let mut create_date: Option<String> = None;

    for line in text.lines() {
        // Comment lines: "Comment key: value" or "Comment text"
        if line.starts_with("Comment ") {
            let rest = &line[8..];
            // Check for "Comment Creation Date: ..."
            if let Some(stripped) = rest.strip_prefix("Creation Date: ") {
                create_date = Some(stripped.trim().to_string());
            } else if create_date.is_none() && !rest.is_empty() {
                // First non-date comment becomes Comment tag
                tags.push(mk(
                    "Comment",
                    "Comment",
                    Value::String(rest.trim().to_string()),
                ));
            }
            continue;
        }

        // Key value pairs separated by first whitespace
        if let Some((key, value)) = line.split_once(|c: char| c == ' ' || c == '\t') {
            let key = key.trim();
            let value = value.trim();

            // Map AFM keys to ExifTool tag names
            // ExifTool uses the Perl key directly (mostly same as AFM key)
            let tag_name = match key {
                "FontName" => Some(("FontName", "Font Name")),
                "FullName" => Some(("FullName", "Full Name")),
                "FamilyName" => Some(("FontFamily", "Font Family")),
                "Weight" => Some(("Weight", "Weight")),
                "Notice" => {
                    // Strip parentheses
                    let v = value.trim_start_matches('(').trim_end_matches(')');
                    tags.push(mk("Notice", "Notice", Value::String(v.to_string())));
                    None
                }
                "Version" => Some(("Version", "Version")),
                "EncodingScheme" => Some(("EncodingScheme", "Encoding Scheme")),
                "CapHeight" => {
                    if let Ok(n) = value.parse::<f64>() {
                        tags.push(mk(
                            "CapHeight",
                            "Cap Height",
                            Value::String(format!("{}", n)),
                        ));
                    }
                    None
                }
                "XHeight" => {
                    if let Ok(n) = value.parse::<f64>() {
                        tags.push(mk("XHeight", "X Height", Value::String(format!("{}", n))));
                    }
                    None
                }
                "Ascender" => {
                    if let Ok(n) = value.parse::<f64>() {
                        tags.push(mk("Ascender", "Ascender", Value::String(format!("{}", n))));
                    }
                    None
                }
                "Descender" => {
                    if let Ok(n) = value.parse::<f64>() {
                        tags.push(mk(
                            "Descender",
                            "Descender",
                            Value::String(format!("{}", n)),
                        ));
                    }
                    None
                }
                _ => None,
            };
            if let Some((name, desc)) = tag_name {
                tags.push(mk(name, desc, Value::String(value.to_string())));
            }
        }
    }

    // Add CreateDate from comments
    if let Some(date) = create_date {
        tags.push(mk("CreateDate", "Create Date", Value::String(date)));
    }

    Ok(tags)
}

/// Read PostScript Type 1 Binary font (.pfb) file.
/// PFB wraps PFA-style text in binary segments:
///   \x80\x01 + 4-byte LE length = ASCII segment
///   \x80\x02 + 4-byte LE length = binary/encrypted segment
///   \x80\x03                     = EOF
/// Extracts all ASCII segments, concatenates them, then parses as PFA.
pub fn read_pfb(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 6 || data[0] != 0x80 {
        return Err(Error::InvalidData("not a PFB file".into()));
    }

    let mut text = Vec::new();
    let mut pos = 0;
    while pos + 2 <= data.len() {
        if data[pos] != 0x80 {
            break;
        }
        let seg_type = data[pos + 1];
        if seg_type == 3 {
            break; // EOF segment
        }
        if pos + 6 > data.len() {
            break;
        }
        let length =
            u32::from_le_bytes([data[pos + 2], data[pos + 3], data[pos + 4], data[pos + 5]])
                as usize;
        pos += 6;
        if pos + length > data.len() {
            break;
        }
        if seg_type == 1 {
            // ASCII segment — collect for parsing
            text.extend_from_slice(&data[pos..pos + length]);
        }
        // type 2 = binary/encrypted — skip
        pos += length;
    }

    if text.is_empty() {
        return Err(Error::InvalidData("no ASCII segments in PFB file".into()));
    }

    // Parse the assembled text just like a PFA file
    read_pfa(&text)
}

/// Read Printer Font Metrics (.pfm) file.
/// Little-endian binary format with fixed-offset fields.
pub fn read_pfm(data: &[u8]) -> Result<Vec<Tag>> {
    // PFM: starts with \x00\x01 or \x00\x02, total size at offset 2 (int32u LE)
    if data.len() < 117 || data[0] != 0x00 || (data[1] != 0x01 && data[1] != 0x02) {
        return Err(Error::InvalidData("not a PFM file".into()));
    }
    let stored_size = u32::from_le_bytes([data[2], data[3], data[4], data[5]]) as usize;
    if stored_size != data.len() {
        return Err(Error::InvalidData("PFM file size mismatch".into()));
    }

    let mut tags = Vec::new();
    let group = "Font";

    // PFMVersion at offset 0: int16u LE, PrintConv: sprintf("%x.%.2x",$val>>8,$val&0xff)
    let pfm_ver = u16::from_le_bytes([data[0], data[1]]);
    let ver_str = format!("{:x}.{:02x}", pfm_ver >> 8, pfm_ver & 0xff);
    tags.push(mk_font(
        group,
        "PFMVersion",
        "PFM Version",
        Value::String(ver_str),
    ));

    // Copyright at offset 6: string[60]
    let copyright = pfm_read_string(data, 6, 60);
    if !copyright.is_empty() {
        tags.push(mk_font(
            group,
            "Copyright",
            "Copyright",
            Value::String(copyright),
        ));
    }

    // FontType at offset 66: int16u LE
    let font_type = u16::from_le_bytes([data[66], data[67]]);
    tags.push(mk_font(
        group,
        "FontType",
        "Font Type",
        Value::String(format!("{}", font_type)),
    ));

    // PointSize at offset 68: int16u LE
    let point_size = u16::from_le_bytes([data[68], data[69]]);
    tags.push(mk_font(
        group,
        "PointSize",
        "Point Size",
        Value::String(format!("{}", point_size)),
    ));

    // YResolution at offset 70: int16u LE
    let y_res = u16::from_le_bytes([data[70], data[71]]);
    tags.push(mk_font(
        group,
        "YResolution",
        "Y Resolution",
        Value::String(format!("{}", y_res)),
    ));

    // XResolution at offset 72: int16u LE
    let x_res = u16::from_le_bytes([data[72], data[73]]);
    tags.push(mk_font(
        group,
        "XResolution",
        "X Resolution",
        Value::String(format!("{}", x_res)),
    ));

    // Ascent at offset 74: int16u LE
    let ascent = u16::from_le_bytes([data[74], data[75]]);
    tags.push(mk_font(
        group,
        "Ascent",
        "Ascent",
        Value::String(format!("{}", ascent)),
    ));

    // InternalLeading at offset 76: int16u LE
    let int_lead = u16::from_le_bytes([data[76], data[77]]);
    tags.push(mk_font(
        group,
        "InternalLeading",
        "Internal Leading",
        Value::String(format!("{}", int_lead)),
    ));

    // ExternalLeading at offset 78: int16u LE
    let ext_lead = u16::from_le_bytes([data[78], data[79]]);
    tags.push(mk_font(
        group,
        "ExternalLeading",
        "External Leading",
        Value::String(format!("{}", ext_lead)),
    ));

    // Italic at offset 80: int8u
    tags.push(mk_font(
        group,
        "Italic",
        "Italic",
        Value::String(format!("{}", data[80])),
    ));

    // Underline at offset 81: int8u
    tags.push(mk_font(
        group,
        "Underline",
        "Underline",
        Value::String(format!("{}", data[81])),
    ));

    // Strikeout at offset 82: int8u
    tags.push(mk_font(
        group,
        "Strikeout",
        "Strikeout",
        Value::String(format!("{}", data[82])),
    ));

    // Weight at offset 83: int16u LE
    let weight = u16::from_le_bytes([data[83], data[84]]);
    tags.push(mk_font(
        group,
        "Weight",
        "Weight",
        Value::String(format!("{}", weight)),
    ));

    // CharacterSet at offset 85: int8u
    tags.push(mk_font(
        group,
        "CharacterSet",
        "Character Set",
        Value::String(format!("{}", data[85])),
    ));

    // PixWidth at offset 86: int16u LE
    let pix_w = u16::from_le_bytes([data[86], data[87]]);
    tags.push(mk_font(
        group,
        "PixWidth",
        "Pix Width",
        Value::String(format!("{}", pix_w)),
    ));

    // PixHeight at offset 88: int16u LE
    let pix_h = u16::from_le_bytes([data[88], data[89]]);
    tags.push(mk_font(
        group,
        "PixHeight",
        "Pix Height",
        Value::String(format!("{}", pix_h)),
    ));

    // PitchAndFamily at offset 90: int8u
    tags.push(mk_font(
        group,
        "PitchAndFamily",
        "Pitch And Family",
        Value::String(format!("{}", data[90])),
    ));

    // AvgWidth at offset 91: int16u LE
    let avg_w = u16::from_le_bytes([data[91], data[92]]);
    tags.push(mk_font(
        group,
        "AvgWidth",
        "Avg Width",
        Value::String(format!("{}", avg_w)),
    ));

    // MaxWidth at offset 93: int16u LE
    let max_w = u16::from_le_bytes([data[93], data[94]]);
    tags.push(mk_font(
        group,
        "MaxWidth",
        "Max Width",
        Value::String(format!("{}", max_w)),
    ));

    // FirstChar at offset 95: int8u
    tags.push(mk_font(
        group,
        "FirstChar",
        "First Char",
        Value::String(format!("{}", data[95])),
    ));

    // LastChar at offset 96: int8u
    tags.push(mk_font(
        group,
        "LastChar",
        "Last Char",
        Value::String(format!("{}", data[96])),
    ));

    // DefaultChar at offset 97: int8u
    tags.push(mk_font(
        group,
        "DefaultChar",
        "Default Char",
        Value::String(format!("{}", data[97])),
    ));

    // BreakChar at offset 98: int8u
    tags.push(mk_font(
        group,
        "BreakChar",
        "Break Char",
        Value::String(format!("{}", data[98])),
    ));

    // WidthBytes at offset 99: int16u LE
    let width_bytes = u16::from_le_bytes([data[99], data[100]]);
    tags.push(mk_font(
        group,
        "WidthBytes",
        "Width Bytes",
        Value::String(format!("{}", width_bytes)),
    ));

    // FontName and PostScriptFontName: at offset stored at position 105 (int32u LE)
    if data.len() >= 109 {
        let name_off = u32::from_le_bytes([data[105], data[106], data[107], data[108]]) as usize;
        if name_off < data.len() {
            let rest = &data[name_off..];
            if let Some(null_pos) = rest.iter().position(|&b| b == 0) {
                let font_name: String = rest[..null_pos]
                    .iter()
                    .filter(|&&b| b >= 0x20)
                    .map(|&b| b as char)
                    .collect();
                if !font_name.is_empty() {
                    tags.push(mk_font(
                        group,
                        "FontName",
                        "Font Name",
                        Value::String(font_name),
                    ));
                }
                let rest2 = &rest[null_pos + 1..];
                if let Some(null_pos2) = rest2.iter().position(|&b| b == 0) {
                    let ps_name: String = rest2[..null_pos2]
                        .iter()
                        .filter(|&&b| b >= 0x20)
                        .map(|&b| b as char)
                        .collect();
                    if !ps_name.is_empty() {
                        tags.push(mk_font(
                            group,
                            "PostScriptFontName",
                            "PostScript Font Name",
                            Value::String(ps_name),
                        ));
                    }
                }
            }
        }
    }

    Ok(tags)
}

fn mk_font(group: &str, name: &str, desc: &str, val: Value) -> Tag {
    let pv = val.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: desc.to_string(),
        raw_value: val,
        print_value: pv,
        priority: 0,
        group: TagGroup {
            family0: "File".to_string(),
            family1: group.to_string(),
            family2: "Document".to_string(),
        },
    }
}

fn pfm_read_string(data: &[u8], offset: usize, max_len: usize) -> String {
    let end = (offset + max_len).min(data.len());
    let slice = &data[offset..end];
    let slice = if let Some(null_pos) = slice.iter().position(|&b| b == 0) {
        &slice[..null_pos]
    } else {
        slice
    };
    slice
        .iter()
        .filter(|&&b| b >= 0x20 || b == b'\t')
        .map(|&b| b as char)
        .collect::<String>()
        .trim()
        .to_string()
}
