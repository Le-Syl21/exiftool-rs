//! Canon focal lengths and ISO (issues #29, #30 and #35), printed and numeric
//! (`-n`), compared with ExifTool 13.59.
//!
//! `tests/issues/canon/` holds the three synthetic fixtures of the issues, each
//! with the script that made it: `focal.crw` (#29: a CIFF FocalLength block
//! divided by the CameraSettings FocalUnits, not by its own FocalType),
//! `iso.crw` (#30: the ISO composite reads the ShotInfo BaseISO, not the CIFF
//! one) and `lens.tif` (#35: Min/MaxFocalLength in FocalUnits, as the Lens and
//! LensID composites see them). `expected.txt` is every tag ExifTool reported
//! for them with `-a -G1`, with and without `-n` (`make_expected.py`).

use exiftool_port::{ExifTool, Options};
use std::collections::HashMap;
use std::path::Path;

/// Cases where we knowingly differ from ExifTool, with the reason:
/// (fixture, mode, tag, reason).
const DIVERGENT: &[(&str, &str, &str, &str)] = &[];

struct Expected {
    fixture: String,
    mode: String,
    tag: String,
    value: String,
}

fn expected() -> Vec<Expected> {
    let text = std::fs::read_to_string("../tests/issues/canon/expected.txt").unwrap();
    text.lines()
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            Expected {
                fixture: f[0].into(),
                mode: f[1].into(),
                tag: f[2].into(),
                value: f[3].into(),
            }
        })
        .collect()
}

/// `Group1:Name` to value, the first of any duplicates, as `-a -G1` lists them.
fn read(name: &str, numeric: bool) -> HashMap<String, String> {
    let et = ExifTool::with_options(Options {
        duplicates: true,
        print_conv: !numeric,
        ..Options::default()
    });
    let mut out = HashMap::new();
    for t in et
        .extract_info(Path::new("../tests/issues/canon").join(name))
        .unwrap()
    {
        out.entry(format!("{}:{}", t.group.family1, t.name))
            .or_insert_with(|| t.display_value(numeric));
    }
    out
}

#[test]
fn canon_focal_and_iso_values_match_exiftool() {
    let mut cache: HashMap<(String, String), HashMap<String, String>> = HashMap::new();
    let mut failures = Vec::new();
    let mut checked = 0;
    for e in expected() {
        if DIVERGENT
            .iter()
            .any(|(f, m, t, _)| *f == e.fixture && *m == e.mode && *t == e.tag)
        {
            continue;
        }
        let ours = cache
            .entry((e.fixture.clone(), e.mode.clone()))
            .or_insert_with(|| read(&e.fixture, e.mode == "numeric"));
        checked += 1;
        match ours.get(&e.tag) {
            Some(v) if *v == e.value => {}
            got => failures.push(format!(
                "{}/{} {}: {:?}, ExifTool {:?}",
                e.fixture, e.mode, e.tag, got, e.value
            )),
        }
    }
    assert!(checked > 100, "only {checked} values checked");
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// The FocalUnits a CRW's FocalLength is divided by is the file's, whichever
/// of the two blocks comes first: ExifTool only evaluates the ValueConv once
/// every block is read.
#[test]
fn crw_focal_units_after_focal_length_block() {
    let data = std::fs::read("../tests/issues/canon/focal.crw").unwrap();
    // The fixture's directory: CameraSettings (0x102d, 52 bytes at 0), then
    // CanonFocalLength (0x1029, 8 bytes at 52). Swap the two blocks.
    let (hlen, settings, focal) = (26, 52, 8);
    let mut swapped = data[..hlen].to_vec();
    swapped.extend_from_slice(&data[hlen + settings..hlen + settings + focal]);
    swapped.extend_from_slice(&data[hlen..hlen + settings]);
    let dir = &data[hlen + settings + focal..];
    let mut dir = dir.to_vec();
    // Entries: count (2 bytes), then (tag u16, size u32, offset u32) each.
    let entry = |tag: u16, size: u32, offset: u32| {
        let mut e = tag.to_le_bytes().to_vec();
        e.extend_from_slice(&size.to_le_bytes());
        e.extend_from_slice(&offset.to_le_bytes());
        e
    };
    dir.splice(2..22, [entry(0x1029, 8, 0), entry(0x102d, 52, 8)].concat());
    swapped.extend_from_slice(&dir);

    let tags = ExifTool::new()
        .extract_info_from_bytes(&swapped, Path::new("swapped.crw"))
        .unwrap();
    let focal = tags
        .iter()
        .find(|t| t.name == "FocalLength" && t.group.family1 == "Canon")
        .expect("Canon:FocalLength");
    assert_eq!(focal.print_value, "7 mm");
}
