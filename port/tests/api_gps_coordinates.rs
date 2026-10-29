//! GPS coordinates, printed and numeric (`-n`), compared with ExifTool 13.59.
//!
//! `tests/gps/` holds four synthetic fixtures (`make_fixtures.py`): a GPS IFD
//! for two places, each in a JPEG (behind the IFD0 pointer) and in a CR3 (as
//! IFD0 of the TIFF in the CMT4 box, a different route through the EXIF
//! reader). `expected.txt` is what ExifTool reported for them and for the
//! corpus file `GPS.jpg`, for the GPS group and the GPS composites, with and
//! without `-n` (`make_expected.py`). The fixtures stay out of the parity
//! corpus: the name and value baselines there are print-mode only, and have
//! no CR3 with coordinates. The coordinates are in degrees, minutes and
//! seconds; the time stamp (`GPSTimeStamp`) covers the fraction PrintConv
//! rounds to the microsecond.

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
    let text = std::fs::read_to_string("../tests/gps/expected.txt").unwrap();
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

fn fixture(name: &str) -> std::path::PathBuf {
    if name == "GPS.jpg" {
        Path::new("../tests/images").join(name)
    } else {
        Path::new("../tests/gps").join(name)
    }
}

/// `Group1:Name` to value, the first of any duplicates, as `-a -G1` lists them.
fn read(name: &str, numeric: bool) -> HashMap<String, String> {
    let et = ExifTool::with_options(Options {
        duplicates: true,
        print_conv: !numeric,
        ..Options::default()
    });
    let mut out = HashMap::new();
    for t in et.extract_info(fixture(name)).unwrap() {
        out.entry(format!("{}:{}", t.group.family1, t.name))
            .or_insert_with(|| t.display_value(numeric));
    }
    out
}

#[test]
fn gps_values_match_exiftool() {
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
