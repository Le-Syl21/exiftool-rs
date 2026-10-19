//! HEIF/HEIC/AVIF metadata writing (issue #17), and the honesty of the count
//! `write_info` returns.
//!
//! Each case applies the same edit ExifTool 13.59 was given and compares the
//! EXIF and XMP tags of our output, as our reader sees them, with what
//! ExifTool read from *its* output (`-j -G1 -s`). The baselines live in
//! `tests/expected_heif/`; regenerate them with the scripts described in
//! that directory's cases (`perl exiftool -o out.heic <edits> in.heic`, then
//! `exiftool -j -G1 -s out.heic`, keeping the non-QuickTime, non-Composite
//! tags and dropping `XMPToolkit`, which names the writing tool).
//!
//! Fixtures: `issue17.heic` is the reporter's 4110-byte sample (synthetic
//! metadata); `tests/issue21/` holds the issue #21 pair (synthetic metadata),
//! identical but for the Exif item's extent length, 96 in `issue21.heic` and 0
//! in `issue21_zero_extent.heic`, kept out of the parity corpus because
//! ExifTool cannot read Exif stored in `idat`; `HEIF_meta.avif` and
//! `HEIF_plain.avif` were encoded with libheif/aom for these tests, with and
//! without the same metadata.

use exiftool_rs::ExifTool;
use std::path::{Path, PathBuf};

type Op = (&'static str, Option<&'static str>);

const ISSUE16: &[Op] = &[
    ("EXIF:*", None),
    ("XMP:*", None),
    ("GPS:*", None),
    ("IFD0:ImageDescription", None),
    ("IFD0:Make", None),
    ("IFD0:Model", None),
    ("IFD0:Artist", None),
    ("IFD0:Copyright", None),
    ("ExifIFD:DateTimeOriginal", None),
    ("GPS:GPSLatitude", None),
    ("GPS:GPSLatitudeRef", None),
    ("GPS:GPSLongitude", None),
    ("GPS:GPSLongitudeRef", None),
    ("XMP-dc:Description", None),
    ("XMP-exif:GPSLatitude", None),
    ("XMP-exif:GPSLongitude", None),
];

const EDIT: &[Op] = &[
    ("IFD0:Make", Some("Edited Make")),
    ("IFD0:Artist", Some("Someone Else")),
    ("GPS:GPSLatitude", Some("48.8584")),
    ("GPS:GPSLatitudeRef", Some("N")),
    ("GPS:GPSLongitude", Some("2.2945")),
    ("GPS:GPSLongitudeRef", Some("E")),
];

fn cases() -> Vec<(&'static str, &'static str, Vec<Op>)> {
    let mut v = Vec::new();
    for prefix in ["issue17", "avifmeta"] {
        v.push((prefix, "issue16", ISSUE16.to_vec()));
        v.push((prefix, "indiv13", ISSUE16[3..].to_vec()));
        v.push((
            prefix,
            "exifxmpall",
            vec![("EXIF:all", None), ("XMP:all", None)],
        ));
        v.push((prefix, "gpsall", vec![("GPS:all", None)]));
        v.push((prefix, "edit", EDIT.to_vec()));
        v.push((
            prefix,
            "editxmp",
            vec![("XMP-dc:Description", Some("New description"))],
        ));
        v.push((
            prefix,
            "addtitle",
            vec![("XMP-dc:Title", Some("Added title"))],
        ));
    }
    for prefix in ["QuickTime", "avifplain"] {
        v.push((
            prefix,
            "addxmp",
            vec![
                ("XMP-dc:Title", Some("Hello")),
                ("XMP-dc:Description", Some("A description")),
            ],
        ));
        v.push((
            prefix,
            "addexif",
            vec![
                ("IFD0:Artist", Some("Someone")),
                ("IFD0:Make", Some("Maker")),
            ],
        ));
        v.push((
            prefix,
            "addgps",
            vec![
                ("GPS:GPSLatitude", Some("48.8584")),
                ("GPS:GPSLatitudeRef", Some("N")),
            ],
        ));
        v.push((
            prefix,
            "deleteall",
            vec![("EXIF:all", None), ("XMP:all", None)],
        ));
    }
    v
}

fn image_for(prefix: &str) -> &'static str {
    match prefix {
        "issue17" => "issue17.heic",
        "avifmeta" => "HEIF_meta.avif",
        "avifplain" => "HEIF_plain.avif",
        _ => "QuickTime.heic",
    }
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("exiftool_rs_heif_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// Our EXIF/XMP tags as `Group1:Name=value` lines, sorted.
fn snapshot(path: &Path) -> Vec<String> {
    // All instances (`-a`): Composite:GPSLatitude would hide GPS:GPSLatitude.
    let mut et = ExifTool::new();
    et.options_mut().duplicates = true;
    let tags = et.extract_info(path).unwrap();
    let mut out: Vec<String> = tags
        .iter()
        .filter(|t| matches!(t.group.family0.as_str(), "EXIF" | "XMP" | "MakerNotes"))
        .filter(|t| t.name != "XMPToolkit" && t.name != "ExifByteOrder")
        .map(|t| format!("{}:{}={}", t.group.family1, t.name, t.print_value))
        .collect();
    out.sort();
    out.dedup();
    out
}

fn write(image: &str, ops: &[Op], out_name: &str) -> (exiftool_rs::Result<u32>, PathBuf) {
    let src = Path::new("tests/images").join(image);
    let dst = tmp(out_name);
    let mut et = ExifTool::new();
    for (tag, value) in ops {
        et.set_new_value(tag, *value);
    }
    (et.write_info(&src, &dst), dst)
}

/// The primary image's coded bytes (item 1 for these fixtures, through the
/// public reader of the item tables).
fn primary_item_bytes(path: &Path) -> Vec<u8> {
    let data = std::fs::read(path).unwrap();
    let (_, _, meta) = exiftool_rs::formats::heif_items::top_level_meta(&data).unwrap();
    let primary = meta.primary().unwrap();
    meta.item_stored_bytes(&data, primary).unwrap()
}

#[test]
fn heif_writes_match_exiftool() {
    let mut failures = Vec::new();
    for (prefix, case, ops) in cases() {
        let image = image_for(prefix);
        let id = format!("{prefix}-{case}");
        let ext = Path::new(image).extension().unwrap().to_str().unwrap();
        let (res, dst) = write(image, &ops, &format!("{id}.{ext}"));
        let n = res.unwrap_or_else(|e| panic!("{id}: {e}"));
        // Deleting from a file with no EXIF or XMP (QuickTime.heic,
        // HEIF_plain.avif) changes nothing, which ExifTool reports as
        // "1 image files unchanged".
        let empty = snapshot(&Path::new("tests/images").join(image)).is_empty();
        let changed = if empty && case == "deleteall" {
            0
        } else {
            ops.len()
        };
        assert_eq!(n as usize, changed, "{id}: every request should be applied");
        let expected: Vec<String> =
            std::fs::read_to_string(format!("tests/expected_heif/{id}.txt"))
                .unwrap()
                .lines()
                .map(str::to_string)
                .collect();
        let ours = snapshot(&dst);
        if ours != expected {
            failures.push(format!(
                "{id}\n  ours:     {ours:?}\n  exiftool: {expected:?}"
            ));
        }
        // The coded image is untouched.
        if image != "QuickTime.heic" || case != "deleteall" {
            assert_eq!(
                primary_item_bytes(&dst),
                primary_item_bytes(&Path::new("tests/images").join(image)),
                "{id}: image data changed"
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn issue17_deletions_leave_no_trace_of_the_values() {
    let (res, dst) = write("issue17.heic", ISSUE16, "issue17-trace.heic");
    assert_eq!(res.unwrap(), 16);
    let out = std::fs::read(&dst).unwrap();
    for needle in [
        &b"Fixture"[..],
        b"Synthetic Camera",
        b"synthetic",
        b"Exif\0\0",
        b"x:xmpmeta",
    ] {
        assert!(
            !out.windows(needle.len()).any(|w| w == needle),
            "{:?} still in the file",
            String::from_utf8_lossy(needle)
        );
    }
    // And writing it again is a no-op: nothing changed, as ExifTool's
    // "1 image files unchanged", and still no error.
    let mut et = ExifTool::new();
    et.set_new_value("EXIF:all", None);
    let again = tmp("issue17-again.heic");
    assert_eq!(et.write_info(&dst, &again).unwrap(), 0);
    assert_eq!(std::fs::read(&again).unwrap(), out);
}

#[test]
fn a_request_nothing_can_apply_is_skipped() {
    // HEIF has no IPTC: as ExifTool 13.59 ("1 image files copied"), the
    // output is an unchanged copy and nothing is counted.
    let (res, dst) = write("issue17.heic", &[("IPTC:City", Some("Paris"))], "nope.heic");
    assert_eq!(res.unwrap(), 0);
    assert_eq!(
        std::fs::read(&dst).unwrap(),
        std::fs::read("tests/images/issue17.heic").unwrap()
    );
    // Mixed with one that works, only the one that works is counted.
    let (res, _) = write(
        "issue17.heic",
        &[("IPTC:City", Some("Paris")), ("IFD0:Artist", Some("X"))],
        "mixed.heic",
    );
    assert_eq!(res.unwrap(), 1);
}

#[test]
fn deleting_an_absent_tag_changes_nothing() {
    // A zero extent length reads as an empty Exif item, as in ExifTool (which
    // writes that zero itself when it empties an item): the file has no Make.
    // ExifTool answers "1 image files unchanged", so nothing is counted, it
    // is not an error, and the output is still written, unchanged.
    let (res, dst) = write(
        "../issue21/issue21_zero_extent.heic",
        &[("IFD0:Make", None)],
        "z.heic",
    );
    assert_eq!(res.unwrap(), 0);
    assert_eq!(
        std::fs::read(&dst).unwrap(),
        std::fs::read("tests/issue21/issue21_zero_extent.heic").unwrap()
    );
    // With the extent length given, the same Make is there and is deleted.
    let (res, dst) = write("../issue21/issue21.heic", &[("IFD0:Make", None)], "e.heic");
    assert_eq!(res.unwrap(), 1);
    let left = snapshot(&dst);
    assert!(
        !left.iter().any(|t| t.starts_with("IFD0:Make=")),
        "{left:?}"
    );
    assert!(
        left.iter().any(|t| t.starts_with("IFD0:Artist=")),
        "{left:?}"
    );
    // Only the deletion that removes something counts.
    let (res, _) = write(
        "../issue21/issue21.heic",
        &[("IFD0:Make", None), ("IFD0:Copyright", None)],
        "e2.heic",
    );
    assert_eq!(res.unwrap(), 1);
}

#[test]
fn counts_are_honest_for_other_formats() {
    // JPEG: an XMP tag deletion its writer does not apply is not counted.
    let (res, dst) = write(
        "XMP.jpg",
        &[("XMP-dc:Creator", None), ("EXIF:Artist", Some("A"))],
        "xmp.jpg",
    );
    let n = res.unwrap();
    let still_there = ExifTool::new()
        .extract_info(&dst)
        .unwrap()
        .iter()
        .any(|t| t.group.family1 == "XMP-dc" && t.name == "Creator");
    assert_eq!(n, if still_there { 1 } else { 2 });

    // QuickTime/MP4: an XMP group deletion is applied, and so counted; an
    // IPTC request has nowhere to go and is not.
    let (res, _) = write("QuickTime.mov", &[("XMP:all", None)], "qt.mov");
    assert_eq!(res.unwrap(), 1);
    let (res, _) = write("QuickTime.mov", &[("IPTC:City", Some("x"))], "qt2.mov");
    assert_eq!(res.unwrap(), 0);

    // PNG: a text write lands and is counted.
    let (res, _) = write("PNG.png", &[("Comment", Some("hi"))], "c.png");
    assert_eq!(res.unwrap(), 1);
}

#[test]
fn group_all_spellings_are_equivalent() {
    for spelling in ["EXIF:all", "EXIF:*", "IFD0:all"] {
        let (res, dst) = write("issue17.heic", &[(spelling, None)], "spelling.heic");
        assert_eq!(res.unwrap(), 1, "{spelling}");
        assert!(
            !snapshot(&dst).iter().any(|l| l.starts_with("IFD0:")),
            "{spelling}"
        );
        assert!(snapshot(&dst).iter().any(|l| l.starts_with("XMP-dc:")));
    }
}
