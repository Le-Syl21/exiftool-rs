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
//! cargo run --features parity --bin parity -- --update   # refresh baselines
//! cargo run --features parity --bin parity -- --dump     # TSV for the report
//! ```
//!
//! The write side (`write.rs`) edits every corpus file exiftool-rs writes, with
//! both tools, and verifies each output by re-reading it.
//!
//! Exit code is non-zero if a **new** read delta or a **new** failing write
//! case (neither in its baseline) is found.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

mod write;

/// Perl's output as text: UTF-8 when it is, else Latin-1 (one byte, one
/// character), so that no line is lost to an invalid sequence.
fn decode_utf8_or_latin1(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

/// `Group1:Tag` keys whose values are machine/tool state, not metadata parity.
const EXCLUDE: &[&str] = &[
    "ExifToolVersion",
    "FileModifyDate",
    "FileAccessDate",
    // the birth time, which Perl gives only with File::StatX (the port has it)
    "FileCreateDate",
    "FileInodeChangeDate",
    "FilePermissions",
    "FileName",
    "Directory",
    "FileSize",
];

const READ_BASELINE: &str = "tests/parity_live_baseline.txt";

/// Args passed to Perl ExifTool for the read comparison (shown in the report).
const PERL_READ_ARGS: &[&str] = &["-s", "-G1"];

/// Optional Perl modules ExifTool must have loaded for the corpus to decode
/// fully. The baseline is generated with these present; if CI (or a dev box)
/// lacks one, ExifTool silently emits fewer tags and the diff shows fake
/// deltas — so we fail loudly with an env mismatch instead.
const REQUIRED_MODULES: &[&str] = &[
    "Archive::Zip",         // .zip / DOCX / APK / EPUB
    "POSIX::strptime",      // date parsing
    "Unicode::LineBreak",   // charset / line handling (pulls MIME::Charset)
    "Compress::Raw::Lzma",  // xz / 7z / LZMA
    "IO::Compress::Brotli", // JXL / woff2
];

/// Debian/Ubuntu packages that provide [`REQUIRED_MODULES`].
const MODULE_APT_HINT: &str = "libarchive-zip-perl libio-compress-brotli-perl \
    libcompress-raw-lzma-perl libposix-strptime-perl libunicode-linebreak-perl \
    libmime-charset-perl";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut version = exiftool_rs::EXIFTOOL_VERSION.to_string();
    let mut path_override: Option<String> = None;
    let mut images = "tests/images".to_string();
    let mut update = false;
    let mut dump = false;
    let mut dump_write = false;
    let mut show: Option<String> = None;
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
            "--dump" => dump = true,
            "--dump-write" => dump_write = true,
            "--show" => {
                show = args.get(i + 1).cloned();
                i += 1;
            }
            "-h" | "--help" => {
                eprintln!(
                    "parity — diff exiftool-rs vs a pinned Perl ExifTool\n\
                     --exiftool <ver>     ExifTool release to fetch (default {})\n\
                     --exiftool-path <d>  use a local ExifTool checkout instead\n\
                     --images <dir>       corpus (default tests/images)\n\
                     --show <file>        tag-by-tag ExifTool vs exiftool-rs for one file\n\
                     --dump               TSV of the whole corpus (read section, then write section)\n\
                     --dump-write         TSV of the write matrix only\n\
                     --update             refresh the read and write baselines",
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
    check_modules(&script);

    if let Some(file) = show {
        show_file(&script, Path::new(&file));
        return;
    }
    if dump {
        dump_all(&script, Path::new(&images));
        write::dump(&script, Path::new(&images));
        return;
    }
    if dump_write {
        write::dump(&script, Path::new(&images));
        return;
    }
    println!(
        "\nREAD PARITY — input {}/*, output compared tag-by-tag (Group1:Tag)",
        Path::new(&images).display()
    );
    println!(
        "  exiftool-rs: extract_info(<FILE>) (in-process)\n  \
         ExifTool:    perl <exiftool> {} <FILE>",
        PERL_READ_ARGS.join(" ")
    );

    let read_new = read_parity(&script, Path::new(&images), update);
    let write_bad = write::check(&script, Path::new(&images), update);

    println!();
    if read_new == 0 && write_bad == 0 {
        println!("PARITY OK — no new read deltas, no new write failures.");
    } else {
        eprintln!(
            "PARITY FAILED — {read_new} new read delta(s), {write_bad} new write failure(s)."
        );
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

/// Assert ExifTool loaded every module in [`REQUIRED_MODULES`]; exit with a
/// clear env-mismatch message otherwise, so a missing module never masquerades
/// as a metadata delta. Reads the `-ver -v` "Optional libraries:" report.
fn check_modules(script: &Path) {
    let out = Command::new("perl")
        .arg(script)
        .args(["-ver", "-v"])
        .output()
        .expect("run perl exiftool -ver -v");
    let text = String::from_utf8_lossy(&out.stdout);
    let mut installed = std::collections::BTreeSet::new();
    let mut in_section = false;
    for line in text.lines() {
        if line.starts_with("Optional libraries:") {
            in_section = true;
            continue;
        }
        if !in_section {
            continue;
        }
        // Indented `  Module::Name   version-or-(not installed)`.
        if !line.starts_with(char::is_whitespace) {
            break;
        }
        let mut it = line.split_whitespace();
        let Some(name) = it.next() else { continue };
        let rest = it.collect::<Vec<_>>().join(" ");
        if !rest.contains("not installed") {
            installed.insert(name.to_string());
        }
    }
    let missing: Vec<&str> = REQUIRED_MODULES
        .iter()
        .copied()
        .filter(|m| !installed.contains(*m))
        .collect();
    if !missing.is_empty() {
        eprintln!(
            "ExifTool environment mismatch — missing optional module(s): {}",
            missing.join(", ")
        );
        eprintln!("The baseline needs these present. On Debian/Ubuntu:");
        eprintln!("  sudo apt-get install -y {MODULE_APT_HINT}");
        std::process::exit(2);
    }
    println!(
        "ExifTool optional modules OK ({})",
        REQUIRED_MODULES.join(", ")
    );
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
    // delta-key ("file::Group1:Tag") → (ours, perl); "∅" marks an absent side.
    let mut current: BTreeMap<String, (String, String)> = BTreeMap::new();
    let mut files_clean = 0usize;
    // Per-file rows for the overview table: (file, ExifTool tag count, ours, ISO).
    let mut per_file: Vec<(String, usize, usize, bool)> = Vec::new();

    for file in &files {
        let ours = read_ours(file);
        let theirs = read_perl(script, file);
        let mut file_deltas = 0;
        let cmp = |m: &BTreeMap<String, String>| {
            m.keys()
                .filter(|k| !EXCLUDE.iter().any(|e| k.ends_with(e)))
                .count()
        };
        let (ours_n, perl_n) = (cmp(&ours), cmp(&theirs));
        let keys: std::collections::BTreeSet<&String> = ours.keys().chain(theirs.keys()).collect();
        for k in keys {
            if EXCLUDE.iter().any(|e| k.ends_with(e)) {
                continue;
            }
            let a = ours.get(k);
            let b = theirs.get(k);
            if a != b {
                let dkey = format!("{}::{k}", file.file_name().unwrap().to_string_lossy());
                let ours_v = a.cloned().unwrap_or_else(|| "∅".into());
                let perl_v = b.cloned().unwrap_or_else(|| "∅".into());
                current.insert(dkey, (ours_v, perl_v));
                file_deltas += 1;
            }
        }
        if file_deltas == 0 {
            files_clean += 1;
        }
        per_file.push((
            file.file_name().unwrap().to_string_lossy().into_owned(),
            perl_n,
            ours_n,
            file_deltas == 0,
        ));
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

    let new: Vec<String> = current
        .keys()
        .filter(|k| !baseline.contains(*k))
        .cloned()
        .collect();
    let fixed = baseline
        .iter()
        .filter(|k| !current.contains_key(*k))
        .count();
    // Per-file table: one row per corpus file — same args on each, tag counts
    // on both sides, and the ISO verdict. `--show <file>` dumps a file's full
    // tag-by-tag comparison.
    let file_rows: Vec<Vec<String>> = per_file
        .iter()
        .map(|(f, p, o, iso)| {
            vec![
                f.clone(),
                p.to_string(),
                o.to_string(),
                if *iso { "✓".into() } else { "✗".into() },
            ]
        })
        .collect();
    print_table(
        &["File", "ExifTool tags", "exiftool-rs tags", "ISO"],
        &file_rows,
    );
    // Recap table — always printed, so every run ends on a tabular verdict.
    print_table(
        &["Files identical", "Read deltas", "New (fail)", "ISO"],
        &[vec![
            format!("{}/{}", files_clean, files.len()),
            current.len().to_string(),
            new.len().to_string(),
            if new.is_empty() {
                "✓".into()
            } else {
                "✗".into()
            },
        ]],
    );
    // Detail table of the deltas that fail this run (baselined ones are known
    // and omitted). Each row shows both sides and the ISO verdict.
    if !new.is_empty() {
        let rows: Vec<Vec<String>> = new
            .iter()
            .map(|k| {
                let (file, tag) = k.split_once("::").unwrap_or(("?", k.as_str()));
                let (o, p) = &current[k];
                vec![
                    file.to_string(),
                    tag.to_string(),
                    o.clone(),
                    p.clone(),
                    "✗".into(),
                ]
            })
            .collect();
        print_table(
            &["File", "Group1:Tag", "exiftool-rs", "ExifTool", "ISO"],
            &rows,
        );
    }
    if fixed > 0 {
        println!("  ({fixed} baselined delta(s) no longer occur — run --update to tighten)");
    }
    new.len()
}

/// Tag-by-tag ExifTool vs exiftool-rs dump for one file, so a human can eyeball
/// that the same args on the same file yield the same output.
fn show_file(script: &Path, file: &Path) {
    let ours = read_ours(file);
    let theirs = read_perl(script, file);
    println!(
        "\n{}\n  ExifTool:    perl <exiftool> {} {}\n  exiftool-rs: extract_info",
        file.display(),
        PERL_READ_ARGS.join(" "),
        file.display()
    );
    let keys: std::collections::BTreeSet<&String> = ours.keys().chain(theirs.keys()).collect();
    let mut rows = Vec::new();
    let mut diffs = 0usize;
    for k in keys {
        if EXCLUDE.iter().any(|e| k.ends_with(e)) {
            continue;
        }
        let o = ours.get(k).cloned().unwrap_or_else(|| "∅".into());
        let p = theirs.get(k).cloned().unwrap_or_else(|| "∅".into());
        let iso = o == p;
        if !iso {
            diffs += 1;
        }
        rows.push(vec![
            k.clone(),
            p,
            o,
            if iso { "✓".into() } else { "✗".into() },
        ]);
    }
    print_table(&["Group1:Tag", "ExifTool", "exiftool-rs", "ISO"], &rows);
    println!(
        "{} tag(s) compared, {} identical, {} differ",
        rows.len(),
        rows.len() - diffs,
        diffs
    );
}

/// TSV dump of the whole corpus for a report: one line per (file, tag) with
/// both values and the ISO verdict (1/0). A header line comes first.
fn dump_all(script: &Path, images: &Path) {
    let mut files: Vec<PathBuf> = std::fs::read_dir(images)
        .expect("read images dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
    println!("file\ttag\texiftool\texiftool_rs\tiso");
    for file in &files {
        let fname = file.file_name().unwrap().to_string_lossy();
        let ours = read_ours(file);
        let theirs = read_perl(script, file);
        let keys: std::collections::BTreeSet<&String> = ours.keys().chain(theirs.keys()).collect();
        for k in keys {
            if EXCLUDE.iter().any(|e| k.ends_with(e)) {
                continue;
            }
            let o = ours.get(k).map_or("∅", String::as_str);
            let p = theirs.get(k).map_or("∅", String::as_str);
            let iso = u8::from(o == p);
            println!("{fname}\t{k}\t{}\t{}\t{iso}", clean(p), clean(o));
        }
    }
}

fn read_ours(file: &Path) -> BTreeMap<String, String> {
    let et = exiftool_rs::ExifTool::new();
    let mut m = BTreeMap::new();
    if let Ok(tags) = et.extract_info(file) {
        for t in tags {
            m.insert(format!("{}:{}", t.group.family1, t.name), as_printed(&t.print_value));
        }
    }
    m
}

/// A value as the `exiftool` application prints it with `-s` (the API, as
/// Image::ExifTool, gives it unchanged): `tr/\x01-\x1f\x7f/./`, the NULs
/// removed and the trailing white space stripped.
fn as_printed(v: &str) -> String {
    let s: String = v
        .chars()
        .filter(|&c| c != '\0')
        .map(|c| if ('\x01'..='\x1f').contains(&c) || c == '\x7f' { '.' } else { c })
        .collect();
    s.trim_end_matches([' ', '\t', '\n', '\r', '\x0c', '\x0b']).to_string()
}

fn read_perl(script: &Path, file: &Path) -> BTreeMap<String, String> {
    let out = Command::new("perl")
        .arg(script)
        .args(PERL_READ_ARGS)
        .arg(file)
        .output()
        .expect("run perl exiftool");
    let mut m = BTreeMap::new();
    // ExifTool emits UTF-8 for text tags but passes raw bytes through for
    // undef/binary values (e.g. a Latin-1 `©` = 0xA9). Decoding the whole
    // stream as UTF-8 would mangle those to U+FFFD and fake a delta, so decode
    // each line UTF-8-or-Latin-1: a stray high byte falls back to Latin-1 (the
    // rest of the line is ASCII), while genuinely UTF-8 lines stay intact.
    for raw in out.stdout.split(|&b| b == b'\n') {
        let line = crate::decode_utf8_or_latin1(raw);
        // `[Group1]        TagName             : Value`. Split on the `": "`
        // separator (not just `:`) and keep the value verbatim — trimming it
        // would drop a leading space that ExifTool preserves (e.g. the ASF
        // AudioCodecDescription " 20 kbps"), silently masking a real delta.
        let Some(rest) = line.strip_prefix('[') else {
            continue;
        };
        let Some((group, rest)) = rest.split_once(']') else {
            continue;
        };
        let Some((name, value)) = rest.split_once(": ") else {
            continue;
        };
        m.insert(
            format!("{}:{}", group.trim(), name.trim()),
            value.to_string(),
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

fn save_baseline(current: &BTreeMap<String, (String, String)>) {
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

// ── util ────────────────────────────────────────────────────────────────────

/// Render an aligned box table. Cells wider than `MAX` chars are ellipsised.
fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    const MAX: usize = 46;
    let cols = headers.len();
    let clip = |s: &str| -> String {
        let cs: Vec<char> = s.chars().collect();
        if cs.len() > MAX {
            format!("{}…", cs[..MAX - 1].iter().collect::<String>())
        } else {
            s.to_string()
        }
    };
    let body: Vec<Vec<String>> = rows
        .iter()
        .map(|r| r.iter().map(|c| clip(c)).collect())
        .collect();
    let mut w = vec![0usize; cols];
    for (i, h) in headers.iter().enumerate() {
        w[i] = h.chars().count();
    }
    for r in &body {
        for (i, c) in r.iter().enumerate() {
            w[i] = w[i].max(c.chars().count());
        }
    }
    let rule = |l: char, m: char, r: char| -> String {
        let mut s = String::new();
        s.push(l);
        for (i, wi) in w.iter().enumerate() {
            s.extend(std::iter::repeat_n('─', wi + 2));
            s.push(if i + 1 == cols { r } else { m });
        }
        s
    };
    let line = |cells: &[String]| -> String {
        let mut s = String::from("│");
        for (i, c) in cells.iter().enumerate() {
            let pad = w[i] - c.chars().count();
            s.push(' ');
            s.push_str(c);
            s.extend(std::iter::repeat_n(' ', pad));
            s.push_str(" │");
        }
        s
    };
    println!("{}", rule('┌', '┬', '┐'));
    println!(
        "{}",
        line(&headers.iter().map(|h| (*h).to_string()).collect::<Vec<_>>())
    );
    println!("{}", rule('├', '┼', '┤'));
    for r in &body {
        println!("{}", line(r));
    }
    println!("{}", rule('└', '┴', '┘'));
}

fn run(cmd: &str, args: &[&str], what: &str) {
    let status = Command::new(cmd)
        .args(args)
        .status()
        .unwrap_or_else(|e| panic!("spawn {cmd}: {e}"));
    assert!(status.success(), "{what} failed ({cmd})");
}
