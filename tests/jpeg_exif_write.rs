//! JPEG EXIF and GPS writing, compared with ExifTool 13.59.
//!
//! Each case applies to a corpus JPEG the edit ExifTool was given, reads our
//! output back with our own reader and compares its EXIF, maker-note and
//! IPTC tags (plus `ExifByteOrder`) with what ExifTool read from *its*
//! output. The EXIF block is edited in place, as ExifTool's WriteExif does,
//! so the byte order, IFD1 and its thumbnail, the InteropIFD and the maker
//! notes all survive an edit; these baselines prove it tag by tag.
//!
//! The baselines in `tests/expected_jpeg_exif/` are `Group1:Tag<TAB>value`
//! lines, sorted, generated with the pinned ExifTool (inside the parity
//! Docker image, which has its optional Perl modules):
//!
//! ```sh
//! perl exiftool -o out.jpg <edits> tests/images/<file>
//! perl exiftool -a -s -G0:1 -EXIF:all -MakerNotes:all -IPTC:all \
//!     -File:ExifByteOrder out.jpg
//! ```
//!
//! with the family-1 group kept, and the offsets that depend on the block
//! layout (`ThumbnailOffset`, `PreviewImageStart`,
//! `OriginalDecisionDataOffset`, `MPImageStart`) left out.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use exiftool_rs::{ExifTool, Options};

type Op = (&'static str, Option<&'static str>);

const LAYOUT_OFFSETS: &[&str] = &[
    "ThumbnailOffset",
    "PreviewImageStart",
    "OriginalDecisionDataOffset",
    "MPImageStart",
];

fn out_dir() -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join("jpeg_exif_write");
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(case: &str, src: &str, ops: &[Op]) -> PathBuf {
    let mut et = ExifTool::new();
    for (tag, value) in ops {
        et.set_new_value(tag, *value);
    }
    let dst = out_dir().join(format!("{case}.jpg"));
    let n = et
        .write_info(Path::new("tests/images").join(src), &dst)
        .unwrap_or_else(|e| panic!("{case}: write failed: {e}"));
    assert_eq!(n as usize, ops.len(), "{case}: every request is applied");
    dst
}

fn snapshot(path: &Path) -> String {
    let et = ExifTool::with_options(Options {
        duplicates: true,
        ..Options::default()
    });
    let tags = et.extract_info(path).unwrap();
    let lines: BTreeSet<String> = tags
        .iter()
        .filter(|t| {
            matches!(t.group.family0.as_str(), "EXIF" | "MakerNotes" | "IPTC")
                || (t.group.family0 == "File" && t.name == "ExifByteOrder")
        })
        .filter(|t| !LAYOUT_OFFSETS.contains(&t.name.as_str()))
        .map(|t| format!("{}:{}\t{}", t.group.family1, t.name, t.print_value))
        .collect();
    lines.into_iter().map(|l| l + "\n").collect()
}

fn check(case: &str, src: &str, ops: &[Op]) {
    let out = write(case, src, ops);
    let expected = std::fs::read_to_string(format!("tests/expected_jpeg_exif/{case}.txt")).unwrap();
    let got = snapshot(&out);
    if got != expected {
        let e: BTreeSet<&str> = expected.lines().collect();
        let g: BTreeSet<&str> = got.lines().collect();
        let missing: Vec<&&str> = e.difference(&g).collect();
        let extra: Vec<&&str> = g.difference(&e).collect();
        panic!("{case}: differs from ExifTool\n  missing: {missing:#?}\n  extra: {extra:#?}");
    }
}

#[test]
fn adding_a_tag_keeps_byte_order_ifd1_interop_and_maker_notes() {
    // Canon.jpg is little-endian, with IFD1 (thumbnail), InteropIFD and a
    // Canon maker note.
    check(
        "canon_add_artist",
        "Canon.jpg",
        &[("EXIF:Artist", Some("Added by write parity"))],
    );
}

#[test]
fn gps_coordinates_create_the_gps_directory() {
    check(
        "canon_add_gps",
        "Canon.jpg",
        &[
            ("GPS:GPSLatitude", Some("48.8566")),
            ("GPS:GPSLatitudeRef", Some("N")),
            ("GPS:GPSLongitude", Some("2.3522")),
            ("GPS:GPSLongitudeRef", Some("E")),
        ],
    );
}

#[test]
fn exif_all_removes_the_block() {
    check("canon_del_exif", "Canon.jpg", &[("EXIF:all", None)]);
}

#[test]
fn deleting_a_tag_only_reaches_its_write_group() {
    // Sony.jpg has Model in IFD0 and in IFD1: ExifTool deletes IFD0:Model
    // only (Model's WriteGroup), and so do we.
    check("sony_del_model", "Sony.jpg", &[("EXIF:Model", None)]);
}

#[test]
fn gps_all_removes_the_gps_directory_only() {
    check("gps_del_group", "GPS.jpg", &[("GPS:all", None)]);
}

#[test]
fn a_new_exif_block_takes_its_resolution_from_jfif() {
    // CasioQVCI.jpg has no EXIF, and a JFIF segment with a 1:1 aspect ratio
    // (units 0): ExifTool's new IFD0 gets XResolution 1, ResolutionUnit None.
    check(
        "casio_new_exif",
        "CasioQVCI.jpg",
        &[("EXIF:Artist", Some("Added by write parity"))],
    );
    // The new APP1 comes right after the APP0 segments.
    let out = std::fs::read(out_dir().join("casio_new_exif.jpg")).unwrap();
    let mut pos = 2;
    let mut markers = Vec::new();
    while pos + 4 <= out.len() && out[pos] == 0xFF && out[pos + 1] != 0xDA {
        markers.push(out[pos + 1]);
        pos += 2 + u16::from_be_bytes([out[pos + 2], out[pos + 3]]) as usize;
    }
    let first_non_app0 = markers.iter().position(|&m| m != 0xE0).unwrap();
    assert_eq!(markers[first_non_app0], 0xE1, "markers: {markers:x?}");
}

#[test]
fn a_size_change_keeps_the_afcp_trailer_offsets_valid() {
    // The AFCP trailer holds absolute file offsets; ExifTool warns
    // "Adjusted AFCP offsets" on a file where they are stale, and our reader
    // loses the trailer's IPTC.
    check(
        "afcp_add_artist",
        "AFCP.jpg",
        &[("EXIF:Artist", Some("Added by write parity"))],
    );
    let out = std::fs::read(out_dir().join("afcp_add_artist.jpg")).unwrap();
    let end = out.len() - 12;
    assert_eq!(&out[end..end + 4], b"AXS!");
    let start = u32::from_be_bytes(out[end + 4..end + 8].try_into().unwrap()) as usize;
    assert_eq!(
        &out[start..start + 4],
        b"AXS!",
        "end record points at the header"
    );
}
