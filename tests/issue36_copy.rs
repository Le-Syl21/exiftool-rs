//! Issue #36: copying EXIF from one image to another
//! (`set_new_values_from_file`) lost ExposureTime, FNumber, ISO, FocalLength
//! and the GPS coordinates, because the EXIF writer only knew 29 tags. It now
//! writes every tag of Exif.pm and GPS.pm whose inverse conversion is ported
//! (`scripts/gen_exif_write.pl`).
//!
//! `tests/issues/copy/` holds the two synthetic JPEGs of the issue. The
//! expected values are what ExifTool 13.59 prints (`-G1 -s -a`) for the
//! source.

use exiftool_rs::{ExifTool, Options};
use std::path::Path;

const DIR: &str = "tests/issues/copy";

/// `Group1:Name` = printed value of every EXIF tag of a file.
fn exif(path: &Path) -> Vec<(String, String)> {
    let et = ExifTool::with_options(Options {
        duplicates: true,
        ..Options::default()
    });
    let mut tags: Vec<(String, String)> = et
        .extract_info(path)
        .unwrap()
        .into_iter()
        .filter(|t| t.group.family0 == "EXIF")
        .map(|t| (format!("{}:{}", t.group.family1, t.name), t.print_value))
        .collect();
    tags.sort();
    tags
}

fn get<'a>(tags: &'a [(String, String)], key: &str) -> Option<&'a str> {
    tags.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

#[test]
fn copying_keeps_capture_settings_and_gps() {
    let out = std::env::temp_dir().join("exiftool-rs-issue36-copy.jpg");
    let _ = std::fs::remove_file(&out);
    let mut et = ExifTool::new();
    let queued = et
        .set_new_values_from_file(Path::new(DIR).join("source.jpg"), None)
        .unwrap();
    assert!(queued > 0);
    et.write_info(Path::new(DIR).join("target.jpg"), &out)
        .unwrap();
    assert_eq!(et.write_warnings(), Vec::<String>::new());

    let tags = exif(&out);
    for (key, value) in [
        ("IFD0:Make", "Fixture Camera"),
        ("IFD0:Model", "Fixture Model"),
        ("ExifIFD:LensModel", "Fixture 35mm"),
        ("ExifIFD:DateTimeOriginal", "2026:01:02 03:04:05"),
        ("ExifIFD:ExposureTime", "1/125"),
        ("ExifIFD:FNumber", "2.8"),
        ("ExifIFD:ISO", "400"),
        ("ExifIFD:FocalLength", "35.0 mm"),
        ("GPS:GPSLatitude", "12 deg 30' 0.00\""),
        ("GPS:GPSLatitudeRef", "North"),
        ("GPS:GPSLongitude", "45 deg 15' 0.00\""),
        ("GPS:GPSLongitudeRef", "West"),
        ("GPS:GPSAltitude", "10 m"),
        ("GPS:GPSAltitudeRef", "Above Sea Level"),
    ] {
        assert_eq!(get(&tags, key), Some(value), "{key}");
    }
    // Everything ExifTool reads from the source is in the copy.
    assert_eq!(exif(&Path::new(DIR).join("source.jpg")), tags);
    let _ = std::fs::remove_file(&out);
}

#[test]
fn capture_settings_can_be_written_by_name() {
    let out = std::env::temp_dir().join("exiftool-rs-issue36-write.jpg");
    let _ = std::fs::remove_file(&out);
    let mut et = ExifTool::new();
    for (tag, value) in [
        ("EXIF:ExposureTime", "1/125"),
        ("EXIF:FNumber", "2.8"),
        ("EXIF:ISO", "400"),
        ("EXIF:FocalLength", "35 mm"),
        ("EXIF:ComponentsConfiguration", "Y, Cb, Cr, -"),
        ("EXIF:XPKeywords", "旅行;Paris"),
    ] {
        et.set_new_value(tag, Some(value));
    }
    assert_eq!(
        et.write_info(Path::new(DIR).join("target.jpg"), &out)
            .unwrap(),
        6
    );
    assert_eq!(et.write_warnings(), Vec::<String>::new());
    let tags = exif(&out);
    assert_eq!(get(&tags, "ExifIFD:ExposureTime"), Some("1/125"));
    assert_eq!(get(&tags, "ExifIFD:FNumber"), Some("2.8"));
    assert_eq!(get(&tags, "ExifIFD:ISO"), Some("400"));
    assert_eq!(get(&tags, "ExifIFD:FocalLength"), Some("35.0 mm"));
    assert_eq!(
        get(&tags, "ExifIFD:ComponentsConfiguration"),
        Some("Y, Cb, Cr, -")
    );
    assert_eq!(get(&tags, "IFD0:XPKeywords"), Some("旅行;Paris"));
    let _ = std::fs::remove_file(&out);
}
