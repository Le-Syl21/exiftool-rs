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

/// Held by the tests that allocate much (some inputs build hundreds of MB)
/// and by the one that measures the memory, so that they never overlap.
static MEMORY: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Run an `interp` input; returns its `$@` (without perl's " at ... line
/// N." suffix), checking it ends in a reasonable time.
fn interp(name: &str) -> String {
    let data = std::fs::read(Path::new("tests/fuzz/interp").join(name)).unwrap();
    let _serial = MEMORY.lock().unwrap_or_else(|e| e.into_inner());
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
    assert_eq!(interp("unpack-count-after-slash"), "Count after length/code in unpack");
    // 100 million characters unpacked: perl runs out of memory (4 GB);
    // a 1.8 GB string of bits: perl makes it
    assert_eq!(interp("unpack-values-huge"), "Out of memory in unpack");
    assert_eq!(interp("unpack-bits-huge"), "Out of memory in unpack");
    // `%32C*` over 44 MB: summed as read (it made a value of each byte)
    assert_eq!(interp("unpack-checksum-large"), "");
    // a precision of 7 million: zeros put in front one by one took minutes
    assert_eq!(interp("sprintf-precision-zeros"), "");
}

#[test]
fn pack_values() {
    use exiftool_port::perl::pack::{pack, unpack};
    use exiftool_port::perl::Value;
    let p = |t: &str, v: &[Value]| pack(t.as_bytes(), v).unwrap();
    assert_eq!(p("H5", &[Value::str("abc")]), [0xab, 0xc0, 0x00]);
    assert_eq!(p("h3", &[Value::str("abcd")]), [0xba, 0x0c]);
    assert_eq!(p("B12", &[Value::str("101")]), [0xa0, 0x00]);
    assert_eq!(p("b9", &[Value::str("1")]), [0x01, 0x00]);
    assert_eq!(p("n3", &[Value::Int(1)]), [0, 1, 0, 0, 0, 0]);
    assert_eq!(p("f>2", &[Value::Num(1.0)]), [0x3f, 0x80, 0, 0, 0, 0, 0, 0]);
    assert_eq!(p("a4 Z3 A3", &[Value::str("ab"), Value::str("abc"), Value::str("x")]), b"ab\0\0ab\0x  ");
    assert_eq!(p("U2", &[Value::Int(0x263a)]), "\u{263a}\0".as_bytes());
    assert_eq!(p("(n)3", &[Value::Int(1), Value::Int(2)]), [0, 1, 0, 2, 0, 0]);
    let u = |t: &str, d: &[u8]| unpack(t.as_bytes(), d).unwrap().iter().map(Value::to_text).collect::<Vec<_>>();
    assert_eq!(u("%32n*", &[0, 1, 0xff, 0xff]), ["65536"]);
    assert_eq!(u("%8C*", &[200, 100]), ["44"]);
    assert_eq!(u("%32n2 C", &[0, 1, 0, 2, 0, 3, 9]), ["3", "0"]);
    assert_eq!(u("%64Q", &[0xff; 8]), ["18446744073709551615"]);
}

/// The value of a Perl expression, run in the port's interpreter.
fn perl(code: &str) -> String {
    use exiftool_port::interp::{Scope, Want};
    let mut it = exiftool_port::runtime::new_interp();
    let r = it.eval_string(code.as_bytes(), "main", &Scope::new(None), Want::Scalar);
    r.map(|v| v[0].to_text()).unwrap_or_else(|e| e.to_text())
}

/// `%` with an operand past the IV range (here -Inf, from a fuzzed "-inf")
/// overflowed negating it. The values are perl 5.38's.
#[test]
fn modulus_of_large_operands() {
    assert_eq!(
        perl("join ' ', 7 % -9**9**9, -7 % -9**9**9, 7 % 9**9**9, -7 % 9**9**9, 7 % -9223372036854775808, 7 % -9.3e18, -7 % 3, 7 % -3, -7 % -3, 0 % -9**9**9"),
        "-Inf -7 7 Inf -9223372036854775801 -9.3e+18 2 -2 -1 0"
    );
    assert_eq!(
        perl("join ' ', 1e30 % 2.7, 1e30 % 2.2, 1e30 % 7, -1e30 % 7, 1e30 % -7, 7.9 % 2.9, -7.9 % 2.9, 18446744073709551615 % 10, -1 % 18446744073709551615, 'nan' % 3, 3 % 'nan', 9.3e18 % 7, 1e19 % 3.5"),
        "1 0 5 2 -2 1 1 5 18446744073709551614 NaN NaN 3 1"
    );
    assert_eq!(interp("modulus-minus-inf"), "");
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
    let _serial = MEMORY.lock().unwrap_or_else(|e| e.into_inner());
    let et = ExifTool::new();
    let _ = et.extract_info("../tests/images/Canon.jpg");
    let Some(before) = rss_kb() else { return };
    for _ in 0..20 {
        let _ = et.extract_info("../tests/images/Canon.jpg");
    }
    let after = rss_kb().unwrap();
    assert!(after < before + 10_000, "{before} kB -> {after} kB");
}

/// The compiled patterns are cached per thread, beyond the interpreters
/// (one per facade call), and ExifTool builds patterns from what it reads:
/// the caches are bounded. 100 000 patterns used to keep 280 MB.
#[test]
fn regex_caches_are_bounded() {
    let _serial = MEMORY.lock().unwrap_or_else(|e| e.into_inner());
    perl("'x' =~ /^warm-up$/");
    let Some(before) = rss_kb() else { return };
    for k in 0..20 {
        perl(&format!("my $n = 0; for my $i (1..5000) {{ $n++ if 'x' =~ /^a{k}_$i(?:b|c)+$/ }} $n"));
    }
    let after = rss_kb().unwrap();
    assert!(after < before + 120_000, "{before} kB -> {after} kB");
}

/// Files the facade must read in a reasonable time. A RealMedia metadata
/// structure of size 0 was read again and again: perl hangs on it too
/// (Real.pm, ProcessRealMeta); the port reads it once.
#[test]
fn extract_inputs() {
    let et = ExifTool::new();
    for (name, tags) in [("real-meta-size-zero.rm", 10)] {
        let start = Instant::now();
        let read = et.extract_info(Path::new("tests/fuzz/extract").join(name)).unwrap();
        assert!(start.elapsed() < Duration::from_secs(20), "{name}: {:?}", start.elapsed());
        assert!(read.len() >= tags, "{name}: {} tags", read.len());
    }
}

/// Replay the `interp` inputs of a directory (`FUZZ_INPUTS`), printing how
/// long each takes and how it ends, to triage a fuzzing round in release:
/// `FUZZ_INPUTS=fuzz/artifacts/interp-r2 cargo test --release --test
/// fuzz_regressions -- --ignored --nocapture replay_interp`
#[test]
#[ignore]
fn replay_interp() {
    let Ok(dir) = std::env::var("FUZZ_INPUTS") else { return };
    let mut paths: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
    paths.sort();
    for path in paths {
        let data = std::fs::read(&path).unwrap();
        let start = Instant::now();
        let err = String::from_utf8_lossy(&run_interp_input(&data)).into_owned();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        eprintln!("{name}: {:.2} s, rss {} MB: {}", start.elapsed().as_secs_f64(), rss_kb().unwrap_or(0) / 1024, err.lines().next().unwrap_or(""));
    }
}
