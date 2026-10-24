//! GPS and capture tags of three synthetic files, printed and numeric (`-n`),
//! compared with ExifTool 13.59.
//!
//! `tests/issues/gps/` holds the fixtures of three issues:
//! - `gps.xmp` (#25): XMP-exif coordinates written as "12,30N" / "45,15W",
//!   which ToDegrees turns into signed decimal degrees.
//! - `gps.mov` (#26): an FFmpeg MOV with a Keys `location.ISO6709` position
//!   (`make_mov.sh`), from which the QuickTime composites derive latitude,
//!   longitude and altitude. Being an FFmpeg file it also has zero dates, a
//!   0x7fff language code, a `pasp` atom and a Keys `encoder`.
//! - `camera.exr` (#33): an OpenEXR header with the capture and location
//!   attributes of the OpenEXR::Main table (`gen33.py`).
//!
//! The expected values are ExifTool's `-j -G1` output, with and without
//! `-n`, for the tags these issues are about.

use exiftool_rs::{ExifTool, Options};
use std::collections::HashMap;
use std::path::Path;

/// (fixture, `Group1:Tag`, printed value, `-n` value).
const EXPECTED: &[(&str, &str, &str, &str)] = &[
    // #25
    (
        "gps.xmp",
        "XMP-exif:GPSLatitude",
        "12 deg 30' 0.00\" N",
        "12.5",
    ),
    (
        "gps.xmp",
        "XMP-exif:GPSLongitude",
        "45 deg 15' 0.00\" W",
        "-45.25",
    ),
    ("gps.xmp", "Composite:GPSLatitudeRef", "North", "N"),
    ("gps.xmp", "Composite:GPSLongitudeRef", "West", "W"),
    (
        "gps.xmp",
        "Composite:GPSPosition",
        "12 deg 30' 0.00\" N, 45 deg 15' 0.00\" W",
        "12.5 -45.25",
    ),
    // #26
    (
        "gps.mov",
        "Keys:GPSCoordinates",
        "12 deg 30' 0.00\" N, 45 deg 15' 0.00\" W, 10 m Above Sea Level",
        "12.5 -45.25 10",
    ),
    ("gps.mov", "Keys:Encoder", "Lavf63.1.100", "Lavf63.1.100"),
    (
        "gps.mov",
        "Composite:GPSLatitude",
        "12 deg 30' 0.00\" N",
        "12.5",
    ),
    (
        "gps.mov",
        "Composite:GPSLongitude",
        "45 deg 15' 0.00\" W",
        "-45.25",
    ),
    ("gps.mov", "Composite:GPSAltitude", "10 m", "10"),
    (
        "gps.mov",
        "Composite:GPSAltitudeRef",
        "Above Sea Level",
        "0",
    ),
    (
        "gps.mov",
        "Composite:GPSPosition",
        "12 deg 30' 0.00\" N, 45 deg 15' 0.00\" W",
        "12.5 -45.25",
    ),
    (
        "gps.mov",
        "QuickTime:CreateDate",
        "0000:00:00 00:00:00",
        "0000:00:00 00:00:00",
    ),
    (
        "gps.mov",
        "QuickTime:ModifyDate",
        "0000:00:00 00:00:00",
        "0000:00:00 00:00:00",
    ),
    (
        "gps.mov",
        "Track1:TrackCreateDate",
        "0000:00:00 00:00:00",
        "0000:00:00 00:00:00",
    ),
    (
        "gps.mov",
        "Track1:TrackModifyDate",
        "0000:00:00 00:00:00",
        "0000:00:00 00:00:00",
    ),
    (
        "gps.mov",
        "Track1:MediaCreateDate",
        "0000:00:00 00:00:00",
        "0000:00:00 00:00:00",
    ),
    (
        "gps.mov",
        "Track1:MediaModifyDate",
        "0000:00:00 00:00:00",
        "0000:00:00 00:00:00",
    ),
    (
        "gps.mov",
        "Track1:MediaLanguageCode",
        "Unknown (32767)",
        "32767",
    ),
    ("gps.mov", "Track1:PixelAspectRatio", "1:1", "1:1"),
    // #33
    (
        "camera.exr",
        "OpenEXR:DateTimeOriginal",
        "2026:01:02 03:04:05",
        "2026:01:02 03:04:05",
    ),
    ("camera.exr", "OpenEXR:ISO", "200", "200"),
    ("camera.exr", "OpenEXR:ExposureTime", "1/8", "0.125"),
    ("camera.exr", "OpenEXR:FocusDistance", "2.5 m", "2.5"),
    (
        "camera.exr",
        "OpenEXR:GPSLatitude",
        "12 deg 30' 0.00\" N",
        "12.5",
    ),
    (
        "camera.exr",
        "OpenEXR:GPSLongitude",
        "45 deg 15' 0.00\" W",
        "-45.25",
    ),
    (
        "camera.exr",
        "OpenEXR:GPSAltitude",
        "10 m Above Sea Level",
        "10",
    ),
    ("camera.exr", "Composite:ShutterSpeed", "1/8", "0.125"),
    (
        "camera.exr",
        "Composite:GPSPosition",
        "12 deg 30' 0.00\" N, 45 deg 15' 0.00\" W",
        "12.5 -45.25",
    ),
];

/// Tags ExifTool does not report for these files, which must stay absent:
/// the OpenEXR altitude has no reference, so GPS.pm's Composite GPSAltitude
/// (which requires an EXIF or XMP GPSAltitudeRef) is not built.
const ABSENT: &[(&str, &str)] = &[
    ("camera.exr", "Composite:GPSAltitude"),
    ("camera.exr", "Composite:GPSAltitudeRef"),
];

/// `Group1:Name` to value, the first of any duplicates, as `-a -G1` lists them.
fn read(name: &str, numeric: bool) -> HashMap<String, String> {
    let et = ExifTool::with_options(Options {
        duplicates: true,
        print_conv: !numeric,
        ..Options::default()
    });
    let mut out = HashMap::new();
    for t in et
        .extract_info(Path::new("tests/issues/gps").join(name))
        .unwrap()
    {
        out.entry(format!("{}:{}", t.group.family1, t.name))
            .or_insert_with(|| t.display_value(numeric));
    }
    out
}

#[test]
fn values_match_exiftool() {
    let mut cache: HashMap<(&str, bool), HashMap<String, String>> = HashMap::new();
    let mut failures = Vec::new();
    for (fixture, tag, print, numeric) in EXPECTED {
        for (mode, want) in [(false, print), (true, numeric)] {
            let ours = cache
                .entry((fixture, mode))
                .or_insert_with(|| read(fixture, mode));
            let got = ours.get(*tag).map(String::as_str);
            if got != Some(*want) {
                failures.push(format!(
                    "{fixture} {tag}{}: ExifTool {want:?}, ours {got:?}",
                    if mode { " (-n)" } else { "" }
                ));
            }
        }
    }
    for (fixture, tag) in ABSENT {
        for mode in [false, true] {
            let ours = cache
                .entry((fixture, mode))
                .or_insert_with(|| read(fixture, mode));
            if let Some(v) = ours.get(*tag) {
                failures.push(format!("{fixture} {tag}: ExifTool has none, ours {v:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The library route of the reports: `image_info` with `print_conv: false`
/// gives decimal degrees, west negative.
#[test]
fn image_info_numeric_coordinates() {
    let et = ExifTool::with_options(Options {
        print_conv: false,
        ..Options::default()
    });
    for (fixture, altitude) in [
        ("gps.xmp", None),
        ("gps.mov", Some("10")),
        ("camera.exr", Some("10")),
    ] {
        let info = et
            .image_info(Path::new("tests/issues/gps").join(fixture))
            .unwrap();
        let get = |name: &str| info.get(name).map(String::as_str);
        assert_eq!(get("GPSLatitude"), Some("12.5"), "{fixture}");
        assert_eq!(get("GPSLongitude"), Some("-45.25"), "{fixture}");
        assert_eq!(get("GPSPosition"), Some("12.5 -45.25"), "{fixture}");
        assert_eq!(get("GPSAltitude"), altitude, "{fixture}");
    }
}
