//! EXIF text tags and unknown enum values, printed and numeric (`-n`),
//! compared with ExifTool 13.59.
//!
//! - #27: the Windows XP* tags (0x9c9b-0x9c9f) are UCS-2 text read as
//!   `undef`, whatever type the entry states; they printed as byte lists.
//! - #31: a UserComment without a character code ExifTool recognizes keeps
//!   its first 8 bytes, with the warning "Invalid EXIF text encoding"; they
//!   were cut off.
//! - #34: a value a PrintConv hash has no entry for prints as `Unknown (..)`;
//!   Orientation 0 printed another module's "Horizontal (normal)", 9 its number.
//!
//! `tests/issues/exif/` holds the fixtures of the three reports, two more
//! (`make_fixtures.py`) for the other XP* tags, a UNICODE UserComment and the
//! other tags of Exif::Main and GPS::Main converted the same way, and what
//! ExifTool reported for each (`expected.txt`, from `make_expected.py`). Every
//! tag but the file system's is compared, and a tag ExifTool did not report
//! is a failure too.

use exiftool_port::{ExifTool, Options};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Cases where we knowingly differ from ExifTool, with the reason:
/// (fixture, mode, tag, reason).
const DIVERGENT: &[(&str, &str, &str, &str)] = &[];

/// fixture, mode -> `Group1:Name` -> value.
type Expected = BTreeMap<(String, String), BTreeMap<String, String>>;

fn expected() -> Expected {
    let text = std::fs::read_to_string("../tests/issues/exif/expected.txt").unwrap();
    let mut out = Expected::new();
    for l in text.lines() {
        let f: Vec<&str> = l.split('\t').collect();
        out.entry((f[0].into(), f[1].into()))
            .or_default()
            .insert(f[2].into(), f[3].into());
    }
    out
}

/// `Group1:Name` to value, the first of any duplicates, as `-a -G1` lists
/// them, leaving out the file system tags as `make_expected.py` does.
fn read(name: &str, numeric: bool) -> BTreeMap<String, String> {
    let et = ExifTool::with_options(Options {
        duplicates: true,
        print_conv: !numeric,
        ..Options::default()
    });
    let mut out = BTreeMap::new();
    for t in et
        .extract_info(Path::new("../tests/issues/exif").join(name))
        .unwrap()
    {
        let group = t.group.family1.as_str();
        if matches!(group, "System" | "File") || t.name == "ExifToolVersion" {
            continue;
        }
        out.entry(format!("{group}:{}", t.name))
            .or_insert_with(|| t.display_value(numeric));
    }
    out
}

#[test]
fn exif_text_and_unknown_values_match_exiftool() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for ((fixture, mode), tags) in expected() {
        let ours = read(&fixture, mode == "numeric");
        let divergent = |tag: &str| {
            DIVERGENT
                .iter()
                .any(|(f, m, t, _)| *f == fixture && *m == mode && *t == tag)
        };
        for (tag, value) in &tags {
            if divergent(tag) {
                continue;
            }
            checked += 1;
            match ours.get(tag) {
                Some(v) if v == value => {}
                got => failures.push(format!(
                    "{fixture}/{mode} {tag}: {got:?}, ExifTool {value:?}"
                )),
            }
        }
        let extra: BTreeSet<_> = ours
            .keys()
            .filter(|t| !tags.contains_key(*t) && !divergent(t))
            .collect();
        if !extra.is_empty() {
            failures.push(format!(
                "{fixture}/{mode}: not reported by ExifTool: {extra:?}"
            ));
        }
    }
    assert!(checked >= 50, "only {checked} values checked");
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
