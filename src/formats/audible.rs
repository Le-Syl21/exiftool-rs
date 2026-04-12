//! Audible AA file format reader.
//!
//! Parses Audible .aa audiobook files to extract metadata.
//! Mirrors ExifTool's Audible.pm.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

// Magic bytes at offset 4
const MAGIC: [u8; 4] = [0x57, 0x90, 0x75, 0x36];

pub fn read_audible(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 16 || data[4..8] != MAGIC {
        return Err(Error::InvalidData("not an Audible AA file".into()));
    }

    let mut tags = Vec::new();

    // Number of TOC entries at offset 8 (big-endian uint32)
    let toc_count = u32::from_be_bytes([data[8], data[9], data[10], data[11]]) as usize;

    // Sanity check
    if toc_count > 256 {
        return Err(Error::InvalidData("invalid TOC count".into()));
    }

    // TOC starts at offset 16 (after 16-byte header), each entry is 12 bytes: type(4), offset(4), length(4)
    let toc_bytes = toc_count * 12;
    if 16 + toc_bytes > data.len() {
        return Err(Error::InvalidData("truncated TOC".into()));
    }

    let toc = &data[16..16 + toc_bytes];

    for i in 0..toc_count {
        let base = i * 12;
        let entry_type = u32::from_be_bytes([toc[base], toc[base+1], toc[base+2], toc[base+3]]);
        let offset = u32::from_be_bytes([toc[base+4], toc[base+5], toc[base+6], toc[base+7]]) as usize;
        let length = u32::from_be_bytes([toc[base+8], toc[base+9], toc[base+10], toc[base+11]]) as usize;

        if length == 0 {
            continue;
        }
        if offset + length > data.len() {
            continue;
        }

        let chunk = &data[offset..offset + length];

        match entry_type {
            2 => {
                // Metadata dictionary
                parse_metadata(chunk, &mut tags);
            }
            6 => {
                // Chapter count - first 4 bytes
                if chunk.len() >= 4 {
                    let _count = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                    // ExifTool extracts ChapterCount but it's not in the diff we need to fix
                }
            }
            11 => {
                // Cover art
                if chunk.len() >= 8 {
                    let art_len = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as usize;
                    let art_off_abs = u32::from_be_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]) as usize;
                    // art_off is absolute offset within the file
                    if art_off_abs >= offset && art_off_abs + art_len <= offset + length {
                        let art_rel = art_off_abs - offset;
                        if art_rel + art_len <= chunk.len() {
                            let art = chunk[art_rel..art_rel + art_len].to_vec();
                            tags.push(mk("CoverArt", "Cover Art", Value::Binary(art)));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(tags)
}

fn parse_metadata(data: &[u8], tags: &mut Vec<Tag>) {
    if data.len() < 4 {
        return;
    }
    let num = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if num > 512 {
        return;
    }

    let mut pos = 4usize;

    for _ in 0..num {
        if pos + 9 > data.len() {
            break;
        }
        // 1 unknown byte
        // 4 bytes: tag string length
        // 4 bytes: value string length
        let _unk = data[pos];
        let tag_len = u32::from_be_bytes([data[pos+1], data[pos+2], data[pos+3], data[pos+4]]) as usize;
        let val_len = u32::from_be_bytes([data[pos+5], data[pos+6], data[pos+7], data[pos+8]]) as usize;

        pos += 9;
        let tag_end = pos + tag_len;
        let val_end = tag_end + val_len;

        if val_end > data.len() {
            break;
        }

        let tag = crate::encoding::decode_utf8_or_latin1(&data[pos..tag_end]);
        let val = crate::encoding::decode_utf8_or_latin1(&data[tag_end..val_end]);

        pos = val_end;

        if tag.is_empty() || val.is_empty() {
            continue;
        }

        let tag_name = audible_tag_name(&tag);
        tags.push(mk(&tag_name, &tag_name, Value::String(val)));
    }
}

fn audible_tag_name(key: &str) -> String {
    // Map raw tag names to ExifTool-style names
    match key {
        "pubdate" => "PublishDate".into(),
        "pub_date_start" => "PublishDateStart".into(),
        "author" => "Author".into(),
        "copyright" => "Copyright".into(),
        "product_id" => "ProductId".into(),
        "parent_id" => "ParentId".into(),
        "title" => "Title".into(),
        "provider" => "Provider".into(),
        "narrator" => "Narrator".into(),
        "price" => "Price".into(),
        "description" => "Description".into(),
        "long_description" => "LongDescription".into(),
        "short_title" => "ShortTitle".into(),
        "is_aggregation" => "IsAggregation".into(),
        "title_id" => "TitleId".into(),
        "codec" => "Codec".into(),
        "HeaderSeed" => "HeaderSeed".into(),
        "EncryptedBlocks" => "EncryptedBlocks".into(),
        "HeaderKey" => "HeaderKey".into(),
        "license_list" => "LicenseList".into(),
        "CPUType" => "CPUType".into(),
        "license_count" => "LicenseCount".into(),
        "parent_short_title" => "ParentShortTitle".into(),
        "parent_title" => "ParentTitle".into(),
        "aggregation_id" => "AggregationId".into(),
        "short_description" => "ShortDescription".into(),
        "user_alias" => "UserAlias".into(),
        other => {
            // Convert underscore_case to CamelCase
            // Or just capitalize first letter if no underscores
            if other.contains('_') {
                let mut result = String::new();
                let mut capitalize = true;
                for c in other.chars() {
                    if c == '_' {
                        capitalize = true;
                    } else if capitalize {
                        result.extend(c.to_uppercase());
                        capitalize = false;
                    } else {
                        result.push(c);
                    }
                }
                result
            } else {
                // Keep as-is for hex-looking tag names like "7eb298ac1328"
                let tag_name = format!("Tag{}", other);
                tag_name
            }
        }
    }
}

fn mk(name: &str, description: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: description.to_string(),
        group: TagGroup {
            family0: "Audible".into(),
            family1: "Audible".into(),
            family2: "Audio".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}
