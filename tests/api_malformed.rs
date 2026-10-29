//! Corrupt and truncated inputs must never bring the reader down.
//!
//! Each file in `tests/malformed/` once made `extract_info` panic, or overflow
//! the stack, which aborts the whole process and cannot be caught. They came
//! from mutating the regular test corpus: truncating a file, or flipping a few
//! bytes in its header. Extraction may fail on them or return partial tags;
//! the only requirement is that it returns.

use exiftool_rs::ExifTool;
use std::path::Path;

fn survives(name: &str) {
    let path = Path::new("tests/malformed").join(name);
    // Ok or Err are both fine: only a panic (or an abort) fails the test.
    let _ = ExifTool::new().extract_info(&path);
}

/// A binary plist whose object references loop back on themselves.
#[test]
fn plist_reference_cycle() {
    survives("plist-cycle.plist");
}

/// DSS time strings holding non-ASCII bytes, sliced inside a character.
#[test]
fn dss_non_ascii_time() {
    survives("dss-bad-time-1.dss");
    survives("dss-bad-time-2.dss");
}

/// A DSC comment line holding non-ASCII bytes where "%%Begin" is matched.
#[test]
fn postscript_non_ascii_comment() {
    survives("postscript-non-ascii.eps");
}

/// RIFF chunks declaring more bytes than the file holds.
#[test]
fn riff_truncated_chunks() {
    survives("riff-truncated.avi");
    survives("riff-truncated.wav");
}

/// One input per format and failure kind from the first cargo-fuzz campaign
/// (`fuzz/`): stack overflows through looping EXIF IFDs, slices cut inside a
/// character, arithmetic overflows, a declared 3 GB extended-XMP buffer, and
/// parsers that spun without advancing (Lytro JSON, TNEF, cyclic OLE2 FATs).
#[test]
fn fuzz_campaign_1() {
    let mut names: Vec<_> = std::fs::read_dir("tests/malformed")
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with("fuzz-"))
        .collect();
    names.sort();
    assert!(!names.is_empty());
    for name in &names {
        survives(name);
    }
}
