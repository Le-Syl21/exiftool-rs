//! Inputs found by the port's cargo-fuzz targets (`fuzz/`), each of which
//! once made the port panic, abort, hang or allocate without bound.
//!
//! `tests/fuzz/interp/` holds inputs of the `interp` target (a program
//! number, then its operands; see `fuzz/interp_programs.rs`): each must end
//! with the error perl dies with on it (`fuzz/interp_perl.pl` runs one in
//! perl), or with the port's own bound where perl runs out of memory.
//! `tests/fuzz/extract/` holds files (an artifact without its first byte,
//! which picks the extension) that the facade must read.

use std::path::Path;
use std::time::{Duration, Instant};

use exiftool_port::ExifTool;

include!("../fuzz/interp_programs.rs");

/// Run an `interp` input; returns its `$@` (without perl's " at ... line
/// N." suffix), checking it ends in a reasonable time.
fn interp(name: &str) -> String {
    let data = std::fs::read(Path::new("tests/fuzz/interp").join(name)).unwrap();
    let start = Instant::now();
    let err = String::from_utf8_lossy(&run_interp_input(&data)).into_owned();
    assert!(start.elapsed() < Duration::from_secs(20), "{name}: {:?}", start.elapsed());
    match err.find(" at (eval") {
        Some(k) => err[..k].to_string(),
        None => err.trim_end().to_string(),
    }
}

/// sprintf widths and precisions: perl's limit (a quarter of the address
/// space) gives its "Integer overflow" error, a float field past
/// INT_MAX its "Numeric format result too large"; below those perl builds
/// the field (and runs out of memory past a few GB), which the port bounds
/// at 256 MiB ("Out of memory in sprintf"). Precisions past 65535 used to
/// panic in Rust's formatting machinery.
#[test]
fn sprintf_limits() {
    assert_eq!(interp("sprintf-width-overflow"), "Integer overflow in format string for sprintf");
    assert_eq!(interp("sprintf-float-too-large"), "Numeric format result too large");
    assert_eq!(interp("sprintf-width-huge"), "Out of memory in sprintf");
    // (perl formats these: a precision past 65535 only adds zeros, and a
    // width of 1e164 is -1)
    assert_eq!(interp("sprintf-width-pad"), "");
    assert_eq!(interp("sprintf-width-pad-2"), "");
    assert_eq!(interp("sprintf-float-precision"), "");
    assert_eq!(interp("sprintf-float-precision-2"), "");
}

#[test]
fn sprintf_values() {
    use exiftool_port::perl::sprintf::sprintf;
    use exiftool_port::perl::Value;
    let f = |fmt: &str, v: Value| String::from_utf8(sprintf(fmt.as_bytes(), &[v])).unwrap();
    let long = f("%.70000f", Value::Num(0.1));
    assert_eq!(long.len(), 70002);
    assert!(long.starts_with("0.1000000000000000055511151231257827"));
    assert!(long.ends_with("000"));
    let e = f("%.70000e", Value::Num(1.5));
    assert!(e.starts_with("1.5000") && e.ends_with("0e+00") && e.len() == 70006);
    assert_eq!(f("%.70000g", Value::Num(1.5)), "1.5");
    assert_eq!(f("%.007f", Value::Num(1.0)), "1.0000000");
    assert_eq!(f("%.999999999s", Value::str("ab")), "ab");
}

/// pack/unpack: perl's template errors, its count overflow, and counts
/// that perl would honour until it runs out of memory (or forever).
#[test]
fn pack_limits() {
    assert_eq!(interp("pack-percent"), "'%' may not be used in pack");
    assert_eq!(interp("pack-count-overflow"), "pack/unpack repeat count overflow");
    assert_eq!(interp("unpack-u0-malformed"), "'<' allowed only after types sSiIlLqQjJfFdDpP( in unpack");
    for name in ["pack-count-huge", "pack-count-huge-2", "pack-count-huge-3", "pack-count-huge-4"] {
        assert_eq!(interp(name), "Out of memory in pack", "{name}");
    }
}

fn rss_kb() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    s.lines().find(|l| l.starts_with("VmRSS"))?.split_whitespace().nth(1)?.parse().ok()
}

/// Each facade call runs on an interpreter of its own; ExifTool's data is
/// full of reference cycles, which the interpreter's end must free (perl's
/// global destruction). They used to leak, about 2.5 MB a JPEG.
#[test]
fn interpreters_are_freed() {
    let et = ExifTool::new();
    let _ = et.extract_info("../tests/images/Canon.jpg");
    let Some(before) = rss_kb() else { return };
    for _ in 0..20 {
        let _ = et.extract_info("../tests/images/Canon.jpg");
    }
    let after = rss_kb().unwrap();
    assert!(after < before + 10_000, "{before} kB -> {after} kB");
}
