//! Live differential parity auditor: run a **pinned** Perl ExifTool release and
//! exiftool-rs over the same corpus and diff their output.
//!
//! This is the live counterpart to the frozen baselines in `tests/`: those are
//! the fast, no-Perl PR gate; this binary is the auditor that says *what our
//! real parity is right now against a specific ExifTool*, and regenerates the
//! baselines. Dev-only (needs `perl`, and `curl`+`tar` to fetch a release);
//! behind the `parity` feature so normal builds stay lean.
//!
//! ```sh
//! # default: pin the version this crate targets (exiftool_rs::EXIFTOOL_VERSION)
//! cargo run --features parity --bin parity
//! cargo run --features parity --bin parity -- --exiftool 13.59
//! cargo run --features parity --bin parity -- --exiftool-path ../exiftool
//! cargo run --features parity --bin parity -- --update   # refresh baseline
//! ```
//!
//! Exit code is non-zero if a **new** read delta (not in the baseline) or any
//! write mismatch is found.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `Group1:Tag` keys whose values are machine/tool state, not metadata parity.
const EXCLUDE: &[&str] = &[
    "ExifToolVersion",
    "FileModifyDate",
    "FileAccessDate",
    "FileInodeChangeDate",
    "FilePermissions",
    "FileName",
    "Directory",
    "FileSize",
];

const READ_BASELINE: &str = "tests/parity_live_baseline.txt";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut version = exiftool_rs::EXIFTOOL_VERSION.to_string();
    let mut path_override: Option<String> = None;
    let mut images = "tests/images".to_string();
    let mut update = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--exiftool" => {
                version = args.get(i + 1).cloned().unwrap_or(version);
                i += 1;
            }
            "--exiftool-path" => {
                path_override = args.get(i + 1).cloned();
                i += 1;
            }
            "--images" => {
                images = args.get(i + 1).cloned().unwrap_or(images);
                i += 1;
            }
            "--update" => update = true,
            "-h" | "--help" => {
                eprintln!(
                    "parity — diff exiftool-rs vs a pinned Perl ExifTool\n\
                     --exiftool <ver>     ExifTool release to fetch (default {})\n\
                     --exiftool-path <d>  use a local ExifTool checkout instead\n\
                     --images <dir>       corpus (default tests/images)\n\
                     --update             refresh the read baseline",
                    exiftool_rs::EXIFTOOL_VERSION
                );
                return;
            }
            other => eprintln!("ignoring unknown arg {other}"),
        }
        i += 1;
    }

    let script = match path_override {
        Some(p) => resolve_local(Path::new(&p)),
        None => ensure_exiftool(&version),
    };
    let reported = perl_version(&script);
    println!("Perl ExifTool: {reported}  ({})", script.display());
    println!(
        "exiftool-rs:   {} (targets {})",
        exiftool_rs::VERSION,
        exiftool_rs::EXIFTOOL_VERSION
    );
    if reported != exiftool_rs::EXIFTOOL_VERSION {
        eprintln!(
            "warning: running ExifTool {reported} but this crate targets {}",
            exiftool_rs::EXIFTOOL_VERSION
        );
    }
    println!();

    let read_new = read_parity(&script, Path::new(&images), update);
    let write_bad = write_parity(&script, Path::new(&images));

    println!();
    if read_new == 0 && write_bad == 0 {
        println!("PARITY OK — no new read deltas, no write mismatches.");
    } else {
        eprintln!("PARITY FAILED — {read_new} new read delta(s), {write_bad} write mismatch(es).");
        std::process::exit(1);
    }
}

// ── ExifTool resolution ─────────────────────────────────────────────────────

fn resolve_local(p: &Path) -> PathBuf {
    if p.is_file() {
        return p.to_path_buf();
    }
    let script = p.join("exiftool");
    assert!(script.exists(), "no `exiftool` script at {}", p.display());
    script
}

/// Fetch ExifTool `<version>` into `target/parity/` (cached) and return the
/// extracted `exiftool` script. Tries, in order: exiftool.org current,
/// exiftool.org `/older/`, then the GitHub release tag — so any pinned version
/// resolves in CI, not just whatever exiftool.org currently serves.
fn ensure_exiftool(version: &str) -> PathBuf {
    let cache = Path::new("target/parity");
    if let Some(s) = locate_script(cache, version) {
        return s;
    }
    std::fs::create_dir_all(cache).expect("create cache dir");
    let tarball = cache.join(format!("exiftool-{version}.tar.gz"));
    if !tarball.exists() {
        let urls = [
            format!("https://exiftool.org/Image-ExifTool-{version}.tar.gz"),
            format!("https://exiftool.org/older/Image-ExifTool-{version}.tar.gz"),
            format!("https://github.com/exiftool/exiftool/archive/refs/tags/{version}.tar.gz"),
        ];
        let ok = urls.iter().any(|url| {
            println!("trying {url}");
            Command::new("curl")
                .args(["-fSL", "-o", tarball.to_str().unwrap(), url])
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        });
        assert!(
            ok,
            "could not download ExifTool {version} — use --exiftool-path"
        );
    }
    run(
        "tar",
        &[
            "xzf",
            tarball.to_str().unwrap(),
            "-C",
            cache.to_str().unwrap(),
        ],
        "extract ExifTool",
    );
    locate_script(cache, version)
        .unwrap_or_else(|| panic!("extracted archive has no exiftool script for {version}"))
}

/// The `exiftool` script for `version` under `cache`, whatever the archive's
/// top-level dir is named (`Image-ExifTool-13.59/` on exiftool.org,
/// `exiftool-13.59/` on GitHub).
fn locate_script(cache: &Path, version: &str) -> Option<PathBuf> {
    for name in [
        format!("Image-ExifTool-{version}"),
        format!("exiftool-{version}"),
    ] {
        let s = cache.join(name).join("exiftool");
        if s.exists() {
            return Some(s);
        }
    }
    None
}

fn perl_version(script: &Path) -> String {
    let out = Command::new("perl")
        .arg(script)
        .arg("-ver")
        .output()
        .expect("run perl exiftool -ver");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

// ── read parity ─────────────────────────────────────────────────────────────

/// Diff every corpus file; returns the count of NEW deltas (not baselined).
fn read_parity(script: &Path, images: &Path, update: bool) -> usize {
    let mut files: Vec<PathBuf> = std::fs::read_dir(images)
        .expect("read images dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    files.sort();

    let baseline = load_baseline();
    let mut current: BTreeMap<String, String> = BTreeMap::new(); // delta-key → detail
    let mut files_clean = 0usize;

    for file in &files {
        let ours = read_ours(file);
        let theirs = read_perl(script, file);
        let mut file_deltas = 0;
        let keys: std::collections::BTreeSet<&String> = ours.keys().chain(theirs.keys()).collect();
        for k in keys {
            if EXCLUDE.iter().any(|e| k.ends_with(e)) {
                continue;
            }
            let a = ours.get(k);
            let b = theirs.get(k);
            if a != b {
                let dkey = format!("{}::{k}", file.file_name().unwrap().to_string_lossy());
                let detail = match (a, b) {
                    (None, Some(v)) => format!("MISSING (perl: {v})"),
                    (Some(v), None) => format!("EXTRA (ours: {v})"),
                    (Some(x), Some(y)) => format!("DIFF (ours: {x} | perl: {y})"),
                    (None, None) => unreachable!(),
                };
                current.insert(dkey, detail);
                file_deltas += 1;
            }
        }
        if file_deltas == 0 {
            files_clean += 1;
        }
    }

    if update {
        save_baseline(&current);
        println!(
            "read: baseline refreshed — {} known delta(s) across {} file(s)",
            current.len(),
            files.len()
        );
        return 0;
    }

    let new: Vec<_> = current.keys().filter(|k| !baseline.contains(*k)).collect();
    let fixed = baseline
        .iter()
        .filter(|k| !current.contains_key(*k))
        .count();
    println!(
        "read: {}/{} files identical, {} known delta(s), {} NEW, {} improved",
        files_clean,
        files.len(),
        current.len(),
        new.len(),
        fixed
    );
    for k in &new {
        println!("  NEW  {k}  {}", current[*k]);
    }
    if fixed > 0 {
        println!("  ({fixed} baselined delta(s) no longer occur — run --update to tighten)");
    }
    new.len()
}

fn read_ours(file: &Path) -> BTreeMap<String, String> {
    let et = exiftool_rs::ExifTool::new();
    let mut m = BTreeMap::new();
    if let Ok(tags) = et.extract_info(file) {
        for t in tags {
            m.insert(format!("{}:{}", t.group.family1, t.name), t.print_value);
        }
    }
    m
}

fn read_perl(script: &Path, file: &Path) -> BTreeMap<String, String> {
    let out = Command::new("perl")
        .arg(script)
        .args(["-s", "-G1"])
        .arg(file)
        .output()
        .expect("run perl exiftool");
    let text = String::from_utf8_lossy(&out.stdout);
    let mut m = BTreeMap::new();
    for line in text.lines() {
        // `[Group1]        TagName             : Value`
        let Some(rest) = line.strip_prefix('[') else {
            continue;
        };
        let Some((group, rest)) = rest.split_once(']') else {
            continue;
        };
        let Some((name, value)) = rest.split_once(':') else {
            continue;
        };
        m.insert(
            format!("{}:{}", group.trim(), name.trim()),
            value.trim().to_string(),
        );
    }
    m
}

fn load_baseline() -> std::collections::BTreeSet<String> {
    std::fs::read_to_string(READ_BASELINE)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(str::to_string)
        .collect()
}

fn save_baseline(current: &BTreeMap<String, String>) {
    let mut s = String::from(
        "# Live read-parity baseline: known exiftool-rs vs Perl ExifTool deltas.\n\
         # One delta key per line. Refresh with `--update`. New deltas fail the run.\n",
    );
    for k in current.keys() {
        s.push_str(k);
        s.push('\n');
    }
    std::fs::write(READ_BASELINE, s).expect("write baseline");
}

// ── write parity ────────────────────────────────────────────────────────────

/// Edits applied to `IPTC.jpg` — same matrix as `tests/write_parity.rs`, but
/// diffed live against Perl. Returns the count of mismatching cases.
fn write_parity(script: &Path, images: &Path) -> usize {
    let src = images.join("IPTC.jpg");
    if !src.exists() {
        println!("write: IPTC.jpg absent — skipped");
        return 0;
    }
    // id + list of (group-qualified tag, value) edits; None deletes.
    type WriteCase = (
        &'static str,
        &'static [(&'static str, Option<&'static str>)],
    );
    let cases: &[WriteCase] = &[
        ("set-accent", &[("IPTC:By-line", Some("Martín"))]),
        ("delete", &[("IPTC:By-line", None)]),
        ("add", &[("IPTC:City", Some("Paris"))]),
        (
            "multi",
            &[
                ("IPTC:By-line", Some("Ada")),
                ("IPTC:City", Some("London")),
                ("IPTC:Headline", None),
            ],
        ),
    ];
    let mut bad = 0;
    for (id, ops) in cases {
        let ours = write_and_snapshot(&src, ops, WriteWith::Ours);
        let theirs = write_and_snapshot(&src, ops, WriteWith::Perl(script));
        if ours == theirs {
            println!("write [{id}]: OK (digest {})", digest_of(&ours));
        } else {
            bad += 1;
            eprintln!("write [{id}]: MISMATCH\n--- ours ---\n{ours}--- perl ---\n{theirs}");
        }
    }
    bad
}

enum WriteWith<'a> {
    Ours,
    Perl(&'a Path),
}

fn write_and_snapshot(src: &Path, ops: &[(&str, Option<&str>)], with: WriteWith) -> String {
    let tmp = std::env::temp_dir().join("exiftool_rs_parity_wp.jpg");
    std::fs::copy(src, &tmp).expect("copy");
    match with {
        WriteWith::Ours => {
            let mut et = exiftool_rs::ExifTool::new();
            for (tag, val) in ops {
                et.set_new_value(tag, *val);
            }
            let p = tmp.to_str().unwrap();
            et.write_info(p, p).expect("ours write");
        }
        WriteWith::Perl(script) => {
            let mut cmd = Command::new("perl");
            cmd.arg(script)
                .args(["-charset", "UTF8", "-overwrite_original"]);
            for (tag, val) in ops {
                match val {
                    Some(v) => cmd.arg(format!("-{tag}={v}")),
                    None => cmd.arg(format!("-{tag}=")),
                };
            }
            let out = cmd.arg(&tmp).output().expect("perl write");
            assert!(out.status.success(), "perl write failed");
        }
    }
    let snap = snapshot(&tmp);
    std::fs::remove_file(&tmp).ok();
    snap
}

fn snapshot(path: &Path) -> String {
    let et = exiftool_rs::ExifTool::new();
    let tags = et.extract_info(path).expect("read");
    let digest = tags
        .iter()
        .find(|t| t.name == "CurrentIPTCDigest")
        .map(|t| t.print_value.as_str())
        .unwrap_or("<none>");
    let mut out = format!("DIGEST {digest}\n");
    for t in tags.iter().filter(|t| t.group.family0 == "IPTC") {
        out.push_str(&format!("{}: {}\n", t.name, t.print_value));
    }
    out
}

fn digest_of(snap: &str) -> &str {
    snap.lines()
        .next()
        .and_then(|l| l.strip_prefix("DIGEST "))
        .unwrap_or("?")
}

// ── util ────────────────────────────────────────────────────────────────────

fn run(cmd: &str, args: &[&str], what: &str) {
    let status = Command::new(cmd)
        .args(args)
        .status()
        .unwrap_or_else(|e| panic!("spawn {cmd}: {e}"));
    assert!(status.success(), "{what} failed ({cmd})");
}
