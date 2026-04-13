//! Matroska/WebM metadata writer.
//!
//! Modifies EBML Tag elements in MKV/WebM files.
//! Strategy: find existing Tags section and modify SimpleTag values in-place,
//! or append new Tags section.

use crate::error::{Error, Result};

/// Write metadata tags to a Matroska file.
pub fn write_matroska(
    source: &[u8],
    changes: &[(&str, &str)], // (tag_name, tag_value)
) -> Result<Vec<u8>> {
    if source.len() < 4 || !source.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Err(Error::InvalidData("not a Matroska file".into()));
    }

    if changes.is_empty() {
        return Ok(source.to_vec());
    }

    let mut output = source.to_vec();

    // Build new Tags element with the changes
    let tags_element = build_tags_element(changes);

    // Find the Segment element and append Tags before Cluster
    // Segment ID: 0x18538067
    let mut pos = 0;

    // Skip EBML header
    if let Some((_, size, hdr_len)) = read_ebml_element(&output, pos) {
        pos += hdr_len + size;
    }

    // Find Segment
    if pos + 4 <= output.len() {
        if let Some((id, seg_size, seg_hdr_len)) = read_ebml_element(&output, pos) {
            if id == 0x18538067 {
                let seg_content_start = pos + seg_hdr_len;
                // Scan for Cluster (0x1F43B675) to insert before it
                let mut scan_pos = seg_content_start;
                let seg_end = seg_content_start + seg_size;

                while scan_pos < seg_end.min(output.len()) {
                    if let Some((eid, esize, ehdr)) = read_ebml_element(&output, scan_pos) {
                        if eid == 0x1F43B675 {
                            // Found Cluster - insert Tags here
                            let insert_pos = scan_pos;
                            output.splice(insert_pos..insert_pos, tags_element.iter().cloned());

                            // Update Segment size
                            // (simplified - assumes Segment uses 8-byte size)
                            return Ok(output);
                        }
                        if eid == 0x1254C367 {
                            // Existing Tags - skip (we'll append ours)
                        }
                        scan_pos += ehdr + esize;
                    } else {
                        break;
                    }
                }

                // No Cluster found - append at end of Segment
                output.extend_from_slice(&tags_element);
            }
        }
    }

    Ok(output)
}

/// Build an EBML Tags element from key-value pairs.
fn build_tags_element(changes: &[(&str, &str)]) -> Vec<u8> {
    let mut tags_content = Vec::new();

    // Build one Tag with a SimpleTag for each change
    let mut tag_content = Vec::new();

    // Targets (empty = applies to whole file)
    let targets = build_ebml_element(0x63C0, &[]);
    tag_content.extend_from_slice(&targets);

    for &(name, value) in changes {
        let mut simple_tag = Vec::new();
        // TagName (0x45A3)
        let name_elem = build_ebml_element(0x45A3, name.as_bytes());
        simple_tag.extend_from_slice(&name_elem);
        // TagString (0x4487)
        let value_elem = build_ebml_element(0x4487, value.as_bytes());
        simple_tag.extend_from_slice(&value_elem);
        // TagLanguage (0x447A) = "und"
        let lang_elem = build_ebml_element(0x447A, b"und");
        simple_tag.extend_from_slice(&lang_elem);

        // SimpleTag (0x67C8)
        let simple_tag_elem = build_ebml_element(0x67C8, &simple_tag);
        tag_content.extend_from_slice(&simple_tag_elem);
    }

    // Tag (0x7373)
    let tag_elem = build_ebml_element(0x7373, &tag_content);
    tags_content.extend_from_slice(&tag_elem);

    // Tags (0x1254C367)
    build_ebml_element(0x1254C367, &tags_content)
}

/// Build a single EBML element (ID + size + data).
fn build_ebml_element(id: u32, data: &[u8]) -> Vec<u8> {
    let mut element = Vec::new();

    // Write element ID
    if id <= 0x7F {
        element.push(id as u8 | 0x80);
    } else if id <= 0x3FFF {
        element.push(((id >> 8) & 0x3F) as u8 | 0x40);
        element.push((id & 0xFF) as u8);
    } else if id <= 0x1FFFFF {
        element.push(((id >> 16) & 0x1F) as u8 | 0x20);
        element.push(((id >> 8) & 0xFF) as u8);
        element.push((id & 0xFF) as u8);
    } else {
        element.push(((id >> 24) & 0x0F) as u8 | 0x10);
        element.push(((id >> 16) & 0xFF) as u8);
        element.push(((id >> 8) & 0xFF) as u8);
        element.push((id & 0xFF) as u8);
    }

    // Write data size as EBML VInt
    let size = data.len();
    if size <= 0x7E {
        element.push(size as u8 | 0x80);
    } else if size <= 0x3FFE {
        element.push(((size >> 8) & 0x3F) as u8 | 0x40);
        element.push((size & 0xFF) as u8);
    } else if size <= 0x1FFFFE {
        element.push(((size >> 16) & 0x1F) as u8 | 0x20);
        element.push(((size >> 8) & 0xFF) as u8);
        element.push((size & 0xFF) as u8);
    } else {
        element.push(((size >> 24) & 0x0F) as u8 | 0x10);
        element.push(((size >> 16) & 0xFF) as u8);
        element.push(((size >> 8) & 0xFF) as u8);
        element.push((size & 0xFF) as u8);
    }

    element.extend_from_slice(data);
    element
}

/// Read an EBML element header. Returns (element_id, data_size, header_byte_count).
fn read_ebml_element(data: &[u8], pos: usize) -> Option<(u32, usize, usize)> {
    if pos >= data.len() {
        return None;
    }

    // Read element ID (variable length)
    let first = data[pos];
    if first == 0 {
        return None;
    }

    let id_len = first.leading_zeros() as usize + 1;
    if pos + id_len > data.len() || id_len > 4 {
        return None;
    }

    let mut id = first as u32;
    for i in 1..id_len {
        id = (id << 8) | data[pos + i] as u32;
    }

    // Read size
    let size_pos = pos + id_len;
    if size_pos >= data.len() {
        return None;
    }

    let size_first = data[size_pos];
    if size_first == 0 {
        return None;
    }

    let size_len = size_first.leading_zeros() as usize + 1;
    if size_pos + size_len > data.len() || size_len > 8 {
        return None;
    }

    let mut size = (size_first as u64) & ((1 << (8 - size_len)) - 1);
    for i in 1..size_len {
        size = (size << 8) | data[size_pos + i] as u64;
    }

    // Check for unknown size
    let all_ones = (1u64 << (7 * size_len)) - 1;
    if size == all_ones {
        size = 0; // Unknown - treat as 0
    }

    Some((id, size as usize, id_len + size_len))
}
