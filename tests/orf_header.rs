//! Olympus ORF headers (issue #24): ORF replaces the TIFF magic 42 with
//! 0x4F52 ("IIRO", "MMOR") or 0x5352 ("IIRS") and keeps a standard IFD.
//! Each fixture is a one-entry IFD0 holding Make = "OM", built in place; the
//! expected values are what ExifTool 13.59 prints for the same bytes, with
//! and without the .orf extension.

use exiftool_rs::ExifTool;
use std::path::Path;

/// A 26-byte ORF: header, IFD0 with one ASCII Make entry, its value, no IFD1.
fn orf(little_endian: bool, magic: &[u8; 2]) -> Vec<u8> {
    let mut data = Vec::new();
    if little_endian {
        data.extend_from_slice(b"II");
        data.extend_from_slice(magic);
        data.extend_from_slice(&8u32.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&0x010Fu16.to_le_bytes());
        data.extend_from_slice(&2u16.to_le_bytes());
        data.extend_from_slice(&4u32.to_le_bytes());
    } else {
        data.extend_from_slice(b"MM");
        data.extend_from_slice(magic);
        data.extend_from_slice(&8u32.to_be_bytes());
        data.extend_from_slice(&1u16.to_be_bytes());
        data.extend_from_slice(&0x010Fu16.to_be_bytes());
        data.extend_from_slice(&2u16.to_be_bytes());
        data.extend_from_slice(&4u32.to_be_bytes());
    }
    data.extend_from_slice(b"OM\0\0");
    data.extend_from_slice(&[0; 4]);
    data
}

fn value(tags: &[exiftool_rs::Tag], name: &str) -> Option<String> {
    tags.iter()
        .find(|t| t.name == name)
        .map(|t| t.print_value.clone())
}

#[test]
fn orf_magics_are_read_like_exiftool() {
    let cases = [
        ("IIRO", orf(true, b"RO"), "Little-endian (Intel, II)"),
        ("IIRS", orf(true, b"RS"), "Little-endian (Intel, II)"),
        ("MMOR", orf(false, b"OR"), "Big-endian (Motorola, MM)"),
    ];
    for (label, data, byte_order) in &cases {
        for file in ["sample.orf", "sample.bin"] {
            let tags = ExifTool::new()
                .extract_info_from_bytes(data, Path::new(file))
                .unwrap_or_else(|e| panic!("{label} as {file}: {e:?}"));
            let ctx = format!("{label} as {file}");
            assert_eq!(value(&tags, "FileType").as_deref(), Some("ORF"), "{ctx}");
            assert_eq!(
                value(&tags, "MIMEType").as_deref(),
                Some("image/x-olympus-orf"),
                "{ctx}"
            );
            assert_eq!(
                value(&tags, "ExifByteOrder").as_deref(),
                Some(*byte_order),
                "{ctx}"
            );
            assert_eq!(value(&tags, "Make").as_deref(), Some("OM"), "{ctx}");
        }
    }
}
