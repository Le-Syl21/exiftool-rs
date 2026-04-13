//! iTunes Cover Flow format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

pub fn read_itc(data: &[u8]) -> Result<Vec<Tag>> {
    // First block must be 'itch' with size >= 0x1c
    if data.len() < 8 {
        return Err(Error::InvalidData("not an ITC file".into()));
    }
    let first_size = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if &data[4..8] != b"itch" || !(0x1c..0x10000).contains(&first_size) {
        return Err(Error::InvalidData("not an ITC file".into()));
    }

    let mut tags = Vec::new();
    let mut pos = 0usize;

    while pos + 8 <= data.len() {
        let block_size =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let block_tag = &data[pos + 4..pos + 8];

        if !(8..0x80000000).contains(&block_size) {
            break;
        }
        if pos + block_size > data.len() {
            break;
        }

        if block_tag == b"itch" {
            // Header block: DataType is at offset 0x10 (16) within block body
            let body_start = pos + 8;
            let body_end = pos + block_size;
            let body = &data[body_start..body_end];
            if body.len() >= 20 {
                let _data_type = &body[0x10 - 8 + 8..]; // offset 0x10 from block start = 0x08 from body
                                                        // Actually offset 0x10 from the block start means byte 16 from pos
                                                        // body starts at pos+8, so offset 16 from pos = body[8..12]
                let dt_bytes = &data[pos + 16..pos + 20];
                let dt_str = match dt_bytes {
                    b"artw" => "Artwork",
                    _ => "Unknown",
                };
                tags.push(mktag(
                    "ITC",
                    "DataType",
                    "Data Type",
                    Value::String(dt_str.into()),
                ));
            }
        } else if block_tag == b"item" {
            // Read inner length (4 bytes after block header)
            if pos + 12 > data.len() {
                break;
            }
            let inner_len =
                u32::from_be_bytes([data[pos + 8], data[pos + 9], data[pos + 10], data[pos + 11]])
                    as usize;
            if inner_len < 0xd0 || inner_len > block_size {
                break;
            }

            // Remaining image data size
            let image_size = block_size - inner_len;

            // Skip past 4-byte blocks until null terminator
            // Starting after block_header(8) + inner_len_field(4) = pos+12
            let mut scan = pos + 12;
            let mut remaining = inner_len - 12;
            loop {
                if remaining < 4 || scan + 4 > data.len() {
                    break;
                }
                let word = &data[scan..scan + 4];
                remaining -= 4;
                scan += 4;
                if word == b"\0\0\0\0" {
                    break;
                }
            }
            if remaining < 4 {
                break;
            }

            // Read remaining header
            let hdr_start = scan;
            let hdr_len = remaining;
            if hdr_start + hdr_len > data.len() {
                break;
            }
            let hdr = &data[hdr_start..hdr_start + hdr_len];

            // Verify 'data' marker at offset 0xb0
            if hdr.len() < 0xb4 || &hdr[0xb0..0xb4] != b"data" {
                break;
            }

            // Parse ITC::Item fields (FORMAT = int32u, FIRST_ENTRY = 0)
            // Entry 0 (offset 0*4=0): LibraryID = undef[8] → hex string
            if hdr.len() >= 8 {
                let lib_id = &hdr[0..8];
                let hex: String = lib_id.iter().map(|b| format!("{:02X}", b)).collect();
                tags.push(mktag("ITC", "LibraryID", "Library ID", Value::String(hex)));
            }
            // Entry 2 (offset 2*4=8): TrackID = undef[8] → hex string
            if hdr.len() >= 16 {
                let track_id = &hdr[8..16];
                let hex: String = track_id.iter().map(|b| format!("{:02X}", b)).collect();
                tags.push(mktag("ITC", "TrackID", "Track ID", Value::String(hex)));
            }
            // Entry 4 (offset 4*4=16): DataLocation = undef[4]
            if hdr.len() >= 20 {
                let loc = &hdr[16..20];
                let loc_str = match loc {
                    b"down" => "Downloaded Separately",
                    b"locl" => "Local Music File",
                    _ => "Unknown",
                };
                tags.push(mktag(
                    "ITC",
                    "DataLocation",
                    "Data Location",
                    Value::String(loc_str.into()),
                ));
            }
            // Entry 5 (offset 5*4=20): ImageType = undef[4]
            if hdr.len() >= 24 {
                let img_type = &hdr[20..24];
                let type_str = match img_type {
                    b"PNGf" => "PNG",
                    b"\0\0\0\x0d" => "JPEG",
                    _ => "Unknown",
                };
                tags.push(mktag(
                    "ITC",
                    "ImageType",
                    "Image Type",
                    Value::String(type_str.into()),
                ));
            }
            // Entry 7 (offset 7*4=28): ImageWidth = int32u
            if hdr.len() >= 32 {
                let width = u32::from_be_bytes([hdr[28], hdr[29], hdr[30], hdr[31]]);
                tags.push(mktag("ITC", "ImageWidth", "Image Width", Value::U32(width)));
            }
            // Entry 8 (offset 8*4=32): ImageHeight = int32u
            if hdr.len() >= 36 {
                let height = u32::from_be_bytes([hdr[32], hdr[33], hdr[34], hdr[35]]);
                tags.push(mktag(
                    "ITC",
                    "ImageHeight",
                    "Image Height",
                    Value::U32(height),
                ));
            }

            // ImageData (binary data after item header)
            if image_size > 0 {
                let img_start = pos + block_size - image_size;
                if img_start + image_size <= data.len() {
                    let img_data = data[img_start..img_start + image_size].to_vec();
                    tags.push(mktag(
                        "ITC",
                        "ImageData",
                        "Image Data",
                        Value::Binary(img_data),
                    ));
                }
            }
        }

        pos += block_size;
    }

    Ok(tags)
}
