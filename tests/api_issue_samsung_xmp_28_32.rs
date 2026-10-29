//! Issues #28 and #32, compared tag by tag with ExifTool 13.59 in print mode
//! and with -n.
//!
//! #28: the Samsung Type2 maker note names 0xa001 FirmwareName, 0xa002
//! SerialNumber (only when its value starts with five word characters) and
//! 0xa003 LensType, whose `samsungLensTypes` name also makes the Composite
//! LensID; and the ExifIFD LensModel, stored after the maker note, keeps its
//! name. #32: ExifTool knows the MicrosoftPhoto namespace as
//! `http://ns.microsoft.com/photo/1.0`, and fixes the same URI with a
//! trailing slash, with a minor warning.
//!
//! `tests/issues/samsung_xmp/` holds the synthetic fixtures (`gen28.py`,
//! `gen32.py`) and what ExifTool reports for each (`expected.txt`, from
//! `make_expected.sh`), less the System group and ExifToolVersion. The order
//! of the tags is not compared: ExifTool sets the file type of an XMP file
//! once the XMP is read, so its Warning comes before the File tags.

use exiftool_rs::{ExifTool, Options};
use std::collections::BTreeMap;

const DIR: &str = "tests/issues/samsung_xmp";

/// `(fixture, mode)` -> the sorted `Group1:Tag <TAB> value` lines.
fn expected() -> BTreeMap<(String, String), Vec<String>> {
    let text = std::fs::read_to_string(format!("{DIR}/expected.txt")).unwrap();
    let mut out: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for line in text.lines().filter(|l| !l.is_empty()) {
        let f: Vec<&str> = line.splitn(4, '\t').collect();
        out.entry((f[0].into(), f[1].into()))
            .or_default()
            .push(format!("{}\t{}", f[2], f[3]));
    }
    for lines in out.values_mut() {
        lines.sort();
    }
    out
}

fn ours(fixture: &str, numeric: bool) -> Vec<String> {
    let et = ExifTool::with_options(Options {
        print_conv: !numeric,
        ..Default::default()
    });
    let tags = et.extract_info(format!("{DIR}/{fixture}")).unwrap();
    let mut lines: Vec<String> = tags
        .iter()
        .filter(|t| t.group.family1 != "System" && t.name != "ExifToolVersion")
        .map(|t| {
            let value = if numeric {
                t.raw_value.to_display_string()
            } else {
                t.print_value.clone()
            };
            format!("{}:{}\t{}", t.group.family1, t.name, value)
        })
        .collect();
    lines.sort();
    lines
}

#[test]
fn every_fixture_matches_exiftool() {
    let expected = expected();
    assert_eq!(expected.len(), 10, "five fixtures, two modes each");
    for ((fixture, mode), want) in &expected {
        let got = ours(fixture, mode == "numeric");
        assert_eq!(&got, want, "{fixture} ({mode})");
    }
}

#[test]
fn a_short_serial_number_is_not_reported() {
    // samsung.srw's 0xa002 is all zeros: the Condition fails, nothing is named.
    let got = ours("samsung.srw", false);
    assert!(!got.iter().any(|l| l.starts_with("Samsung:SerialNumber")));
    assert!(!got.iter().any(|l| l.starts_with("Samsung:LensModel")));
    assert!(got.contains(&"ExifIFD:LensModel\tNX 45mm F1.8".to_string()));
}

#[test]
fn the_uri_exiftool_knows_raises_no_warning() {
    for fixture in ["percent75.xmp", "percent3.xmp"] {
        let got = ours(fixture, false);
        assert!(
            !got.iter().any(|l| l.starts_with("ExifTool:Warning")),
            "{fixture}"
        );
    }
    let got = ours("trailing-slash.xmp", false);
    let warnings: Vec<&String> = got
        .iter()
        .filter(|l| l.starts_with("ExifTool:Warning"))
        .collect();
    assert_eq!(
        warnings,
        ["ExifTool:Warning\t[minor] Fixed incorrect URI for xmlns:MicrosoftPhoto"]
    );
}
