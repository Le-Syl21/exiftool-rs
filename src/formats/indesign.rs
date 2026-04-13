//! Adobe InDesign format reader.

use crate::error::Result;
use crate::tag::Tag;

pub fn read_indesign(data: &[u8]) -> Result<Vec<Tag>> {
    // InDesign master page GUID: 06 06 ED F5 D8 1D 46 E5 BD 31 EF E7 FE 74 B7 1D
    let master_guid = &[
        0x06u8, 0x06, 0xED, 0xF5, 0xD8, 0x1D, 0x46, 0xE5, 0xBD, 0x31, 0xEF, 0xE7, 0xFE, 0x74, 0xB7,
        0x1D,
    ];
    let object_header_guid = &[
        0xDE, 0x39, 0x39, 0x79, 0x51, 0x88, 0x4B, 0x6C, 0x8E, 0x63, 0xEE, 0xF8, 0xAE, 0xE0, 0xDD,
        0x38,
    ];

    if data.len() < 4096 || !data.starts_with(master_guid) {
        return Err(crate::error::Error::InvalidData(
            "not an InDesign file".into(),
        ));
    }

    // Read two master pages (each 4096 bytes) and pick the most current one
    if data.len() < 8192 {
        return Ok(vec![]);
    }

    let page1 = &data[..4096];
    let page2 = &data[4096..8192];

    // Master pages always use LE byte order ('II')
    // Determine current master page (highest sequence number wins)
    let cur_page = {
        let seq1 = u64::from_le_bytes(page1[264..272].try_into().unwrap_or([0; 8]));
        let seq2 = if page2.starts_with(master_guid) {
            u64::from_le_bytes(page2[264..272].try_into().unwrap_or([0; 8]))
        } else {
            0
        };
        if seq2 > seq1 {
            page2
        } else {
            page1
        }
    };

    // Stream byte order is at offset 24 of current master page: 1 = LE, 2 = BE
    let _stream_is_le = cur_page[24] == 1;

    // Number of pages (determines start of stream objects) - master page is LE
    let pages = u32::from_le_bytes(cur_page[280..284].try_into().unwrap_or([0; 4]));
    let start_pos = (pages as usize) * 4096;
    if start_pos >= data.len() {
        return Ok(vec![]);
    }

    // Scan contiguous objects for XMP
    // Object header GUID (16 bytes) + additional header data (16 bytes) = 32 bytes total
    let mut pos = start_pos;
    while pos + 32 <= data.len() {
        if &data[pos..pos + 16] != object_header_guid {
            break;
        }
        // Object (stream) length at offset 24 in the 32-byte object header
        // The object header itself appears to always use LE byte order
        let obj_len =
            u32::from_le_bytes(data[pos + 24..pos + 28].try_into().unwrap_or([0; 4])) as usize;

        pos += 32;
        if obj_len == 0 || pos + obj_len > data.len() {
            break;
        }

        let obj_data = &data[pos..pos + obj_len];

        // XMP stream: 4-byte length prefix followed by XMP data
        // The actual XMP starts at offset 0 or 4 depending on encoding
        if obj_len > 56 {
            if let Some(xp_pos) = find_xpacket(obj_data) {
                let xmp_data = &obj_data[xp_pos..];
                if let Ok(xmp_tags) = crate::metadata::XmpReader::read(xmp_data) {
                    return Ok(xmp_tags);
                }
            }
        }

        pos += obj_len;
    }

    Ok(vec![])
}

fn find_xpacket(data: &[u8]) -> Option<usize> {
    // Look for "<?xpacket begin=" or "<x:xmpmeta"
    for i in 0..data.len().saturating_sub(10) {
        if data[i..].starts_with(b"<?xpacket") || data[i..].starts_with(b"<x:xmpmeta") {
            return Some(i);
        }
    }
    None
}
