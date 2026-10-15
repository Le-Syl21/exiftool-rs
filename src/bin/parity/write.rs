//! Write-parity matrix: edit every corpus file exiftool-rs claims to write,
//! the same way with exiftool-rs and with Perl ExifTool, then re-read both
//! outputs and compare.
//!
//! The read side of the auditor only proves we *decode* like ExifTool. A writer
//! can return `Ok` and leave the file untouched (issue #17: HEIC deletions
//! returned `Ok(16)` with a byte-identical output) and no read check would ever
//! notice. Here every edit is verified by re-reading:
//!
//! 1. **expected state** — Perl ExifTool (the independent referee) re-reads our
//!    output: a set/added tag carries the requested value, a deleted tag or
//!    group is gone. An edit whose tag is in the same state as in the input is
//!    a *no-op*.
//! 2. **Perl-identical** — Perl's re-read of our output equals its re-read of
//!    its own output, tag by tag (`Group1:Tag` and value), minus [`IGNORED`].
//! 3. **re-read consistent** — our reader reads our output the way Perl does
//!    (deltas already present on the input file are not counted again).
//! 4. **valid** — Perl reads our output without an `Error`/`Warning` that
//!    neither the input nor Perl's own output has.
//! 5. **silent no-op** — `write_info` returned `Ok` but the output is
//!    byte-identical, or a requested change is absent (while Perl made it):
//!    the most severe verdict, because the caller is told it worked.
//!
//! Which files are covered is derived from the code, not listed: a file is in
//! the matrix when `write_info` with no edits does not answer "writing not yet
//! supported". Which edits are applied depends on the metadata families the
//! writer for that file type handles ([`families_for`]) and on what the input
//! file already contains, so the matrix stays small and deterministic.
//!
//! Perl does all writes of one file in a single process (`-@ argfile` with
//! `-execute`) and all re-reads in another, so the matrix costs two Perl runs
//! per corpus file.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use super::{print_table, EXCLUDE};

/// Known write failures (`file::case::verdict`). A failing case not listed
/// here fails the run; `--update` refreshes it.
const WRITE_BASELINE: &str = "tests/parity_write_baseline.txt";

/// Scratch directory for the edited copies, removed when the matrix ends.
const WORK_DIR: &str = "target/parity/write-work";

/// Tags left out of the "Perl-identical" and "re-read consistent" comparisons
/// on top of [`EXCLUDE`] (file-system state and tool version), with the reason.
/// They legitimately differ between two correct writers.
pub const IGNORED: &[(&str, &str)] = &[
    (
        "XMPToolkit",
        "names the program that wrote the XMP packet (Image::ExifTool 13.59 vs exiftool-rs)",
    ),
    (
        "ThumbnailOffset",
        "file offset of the EXIF thumbnail; depends on the writer's block layout",
    ),
    (
        "PreviewImageStart",
        "file offset of the preview image; depends on the writer's block layout",
    ),
    (
        "JpgFromRawStart",
        "file offset of the embedded JPEG; depends on the writer's block layout",
    ),
    (
        "OtherImageStart",
        "file offset of the embedded image; depends on the writer's block layout",
    ),
    (
        "StripOffsets",
        "file offsets of the image data; depend on the writer's block layout",
    ),
    (
        "TileOffsets",
        "file offsets of the image data; depend on the writer's block layout",
    ),
];

/// Values written by the matrix.
const SET_VALUE: &str = "Set by write parity";
const ADD_VALUE: &str = "Added by write parity";
const MIXED_VALUE: &str = "Mixed write parity";

/// Header of the write section of `--dump`.
pub const DUMP_HEADER: &str =
    "WRITE\tfile\tformat\tcase\tedits\tours\tperl\texpected\tperl_iso\treread\tvalid\tverdict";

// ── tag maps ────────────────────────────────────────────────────────────────

/// One re-read: `(Group0, Group1, Tag)` → printed value.
type TagMap = BTreeMap<(String, String, String), String>;

fn g1_key(k: &(String, String, String)) -> String {
    format!("{}:{}", k.1, k.2)
}

fn ignored(g1key: &str) -> bool {
    let name = g1key.rsplit(':').next().unwrap_or(g1key);
    EXCLUDE.iter().any(|e| g1key.ends_with(e)) || IGNORED.iter().any(|(t, _)| *t == name)
}

/// `Group1:Tag` → value, without the ignored tags.
fn by_g1(m: &TagMap) -> BTreeMap<String, String> {
    m.iter()
        .map(|(k, v)| (g1_key(k), v.clone()))
        .filter(|(k, _)| !ignored(k))
        .collect()
}

fn read_ours(file: &Path) -> TagMap {
    let et = exiftool_rs::ExifTool::new();
    let mut m = TagMap::new();
    if let Ok(tags) = et.extract_info(file) {
        for t in tags {
            m.insert(
                (t.group.family0.clone(), t.group.family1.clone(), t.name),
                t.print_value,
            );
        }
    }
    m
}

/// Args of the Perl re-reads. `-a` because without it ExifTool hides a tag
/// whose name also exists in a higher-priority group (`IPTC:City` behind
/// `XMP-photoshop:City`, `GPS:GPSLatitude` behind `Composite:GPSLatitude`),
/// which would make a correctly written tag look absent.
pub const PERL_REREAD_ARGS: &[&str] = &["-a", "-s", "-G0:1"];

/// Re-read several files in one Perl process. Files Perl could not open at all
/// are missing from the result.
fn read_perl_many(script: &Path, files: &[PathBuf]) -> BTreeMap<PathBuf, TagMap> {
    let out = Command::new("perl")
        .arg(script)
        .args(PERL_REREAD_ARGS)
        .args(files)
        .output()
        .expect("run perl exiftool");
    let mut res: BTreeMap<PathBuf, TagMap> = BTreeMap::new();
    let mut current: Option<PathBuf> = if files.len() == 1 {
        Some(files[0].clone())
    } else {
        None
    };
    for raw in out.stdout.split(|&b| b == b'\n') {
        // Same per-line UTF-8-or-Latin-1 decoding as the read comparison.
        let line = exiftool_rs::encoding::decode_utf8_or_latin1(raw);
        if let Some(name) = line.strip_prefix("======== ") {
            let p = PathBuf::from(name.trim_end());
            res.entry(p.clone()).or_default();
            current = Some(p);
            continue;
        }
        let Some(cur) = current.as_ref() else {
            continue;
        };
        // `[Group0:Group1]  Tag : Value` (or `[Group]` when both are equal).
        let Some(rest) = line.strip_prefix('[') else {
            continue;
        };
        let Some((groups, rest)) = rest.split_once(']') else {
            continue;
        };
        let Some((name, value)) = rest.split_once(": ") else {
            continue;
        };
        let (g0, g1) = match groups.trim().split_once(':') {
            Some((a, b)) => (a.to_string(), b.to_string()),
            None => (groups.trim().to_string(), groups.trim().to_string()),
        };
        res.entry(cur.clone())
            .or_default()
            .insert((g0, g1, name.trim().to_string()), value.to_string());
    }
    res
}

// ── edits and cases ─────────────────────────────────────────────────────────

/// A metadata family a writer handles: the group prefix used in the edit
/// (`EXIF:Artist`), which is also the family-0 group the tag reads back in,
/// and the tag names the matrix picks from, in order of preference.
#[derive(Clone, Copy, PartialEq)]
enum Fam {
    Exif,
    Xmp,
    Iptc,
    QuickTime,
    Png,
    Pdf,
    PostScript,
    Id3,
    Vorbis,
    Matroska,
    Comment,
}

impl Fam {
    fn group(self) -> &'static str {
        match self {
            Fam::Exif => "EXIF",
            Fam::Xmp => "XMP",
            Fam::Iptc => "IPTC",
            Fam::QuickTime => "QuickTime",
            Fam::Png => "PNG",
            Fam::Pdf => "PDF",
            Fam::PostScript => "PostScript",
            Fam::Id3 => "ID3",
            Fam::Vorbis => "Vorbis",
            Fam::Matroska => "Matroska",
            // GIF/JPEG comments read back as File:Comment.
            Fam::Comment => "File",
        }
    }

    fn candidates(self) -> &'static [&'static str] {
        match self {
            Fam::Exif => &[
                "Artist",
                "Copyright",
                "Software",
                "ImageDescription",
                "Make",
                "Model",
            ],
            Fam::Xmp => &["Description", "Title", "Creator", "Rights"],
            Fam::Iptc => &[
                "By-line",
                "City",
                "Headline",
                "Caption-Abstract",
                "ObjectName",
                "Credit",
            ],
            Fam::QuickTime => &["Title", "Artist", "Comment", "Album", "Genre"],
            Fam::Png => &["Title", "Author", "Description", "Comment", "Software"],
            Fam::Pdf => &["Title", "Author", "Subject", "Creator", "Producer"],
            Fam::PostScript => &["Title", "Creator", "For"],
            Fam::Id3 => &["Title", "Artist", "Album"],
            Fam::Vorbis => &["Title", "Artist", "Album", "Comment"],
            Fam::Matroska => &["Title"],
            Fam::Comment => &["Comment"],
        }
    }
}

/// The families the exiftool-rs writer for a file type (Perl `FileType`)
/// handles, following the `write_info` dispatch and `writable_tags`.
fn families_for(file_type: &str) -> &'static [Fam] {
    match file_type {
        "JPEG" => &[Fam::Exif, Fam::Xmp, Fam::Iptc],
        "TIFF" | "DNG" | "CR2" | "NEF" | "ARW" | "ORF" | "PEF" => &[Fam::Exif],
        "WEBP" | "Extended WEBP" => &[Fam::Exif, Fam::Xmp],
        "MP4" | "MOV" | "M4A" | "3GP" | "F4V" => &[Fam::QuickTime, Fam::Xmp],
        // HEIF goes through the MP4 writer (ilst + XMP); its EXIF is what
        // issue #17 is about, so it is exercised too.
        "HEIC" | "HEIF" | "AVIF" => &[Fam::Exif, Fam::Xmp, Fam::QuickTime],
        "PSD" => &[Fam::Iptc, Fam::Xmp],
        "PDF" => &[Fam::Pdf],
        "PS" | "EPS" => &[Fam::PostScript],
        "MP3" | "AIFF" => &[Fam::Id3],
        "GIF" => &[Fam::Comment],
        // A bare JXL codestream has no box to hold XMP; it is still in the
        // matrix because `write_info` accepts it.
        "JP2" | "JXL" | "JXL Codestream" => &[Fam::Xmp],
        "PNG" => &[Fam::Png],
        "FLAC" | "OGG" | "OPUS" => &[Fam::Vorbis],
        "MKV" | "WEBM" => &[Fam::Matroska],
        "XMP" => &[Fam::Xmp],
        _ => &[],
    }
}

#[derive(Clone)]
struct Edit {
    /// Group prefix: a family-0 group, or `GPS` (family 1).
    group: String,
    /// Tag name, or `all` for a whole-group deletion.
    name: String,
    value: Option<String>,
    /// Only check that the tag exists (its printed form differs from the
    /// written value, e.g. GPS coordinates).
    presence_only: bool,
}

impl Edit {
    fn set(group: &str, name: &str, value: &str) -> Self {
        Edit {
            group: group.into(),
            name: name.into(),
            value: Some(value.into()),
            presence_only: false,
        }
    }
    fn del(group: &str, name: &str) -> Self {
        Edit {
            group: group.into(),
            name: name.into(),
            value: None,
            presence_only: false,
        }
    }
    fn spec(&self) -> String {
        format!("{}:{}", self.group, self.name)
    }
    fn display(&self) -> String {
        match &self.value {
            Some(v) => format!("{}={v}", self.spec()),
            None => format!("{}=", self.spec()),
        }
    }
    fn is_group_delete(&self) -> bool {
        self.value.is_none() && self.name == "all"
    }
    /// Does a re-read key belong to this edit's group?
    fn in_group(&self, k: &(String, String, String)) -> bool {
        if self.group == "GPS" {
            k.1 == "GPS"
        } else {
            k.0.eq_ignore_ascii_case(&self.group)
        }
    }
    /// The edit's observable state in a re-read: the tag's value, or for a
    /// group deletion the sorted list of the group's tags.
    fn state(&self, m: &TagMap) -> Option<String> {
        if self.is_group_delete() {
            let names: Vec<String> = m
                .iter()
                .filter(|(k, _)| self.in_group(k))
                .map(|(k, _)| g1_key(k))
                .collect();
            return (!names.is_empty()).then(|| names.join(", "));
        }
        m.iter()
            .find(|(k, _)| self.in_group(k) && k.2 == self.name)
            .map(|(_, v)| v.clone())
    }
    fn satisfied(&self, m: &TagMap) -> bool {
        match (&self.value, self.state(m)) {
            (None, s) => s.is_none(),
            (Some(_), Some(_)) if self.presence_only => true,
            (Some(want), Some(got)) => *want == got,
            (Some(_), None) => false,
        }
    }
}

struct Case {
    id: String,
    edits: Vec<Edit>,
}

fn has(input: &TagMap, f: Fam, name: &str) -> bool {
    input
        .keys()
        .any(|k| k.0.eq_ignore_ascii_case(f.group()) && k.2 == name)
}

/// Build the deterministic case list for one file from what it contains.
fn build_cases(file: &str, file_type: &str, input: &TagMap) -> Vec<Case> {
    let fams = families_for(file_type);
    let mut cases = Vec::new();
    let split = |f: Fam| -> (Vec<&'static str>, Vec<&'static str>) {
        f.candidates().iter().partition(|n| has(input, f, n))
    };

    for &f in fams {
        let (present, absent) = split(f);
        if let Some(p) = present.first() {
            cases.push(Case {
                id: format!("set-{}", f.group()),
                edits: vec![Edit::set(f.group(), p, SET_VALUE)],
            });
        }
        if let Some(a) = absent.first() {
            cases.push(Case {
                id: format!("add-{}", f.group()),
                edits: vec![Edit::set(f.group(), a, ADD_VALUE)],
            });
        }
        if let Some(d) = present.last() {
            cases.push(Case {
                id: format!("del-{}", f.group()),
                edits: vec![Edit::del(f.group(), d)],
            });
        }
    }

    // Whole-group deletions, for the groups the writer handles and the file has.
    for (group, fam) in [
        ("EXIF", Fam::Exif),
        ("GPS", Fam::Exif),
        ("XMP", Fam::Xmp),
        ("IPTC", Fam::Iptc),
    ] {
        if !fams.contains(&fam) {
            continue;
        }
        let e = Edit::del(group, "all");
        if e.state(input).is_some() {
            cases.push(Case {
                id: format!("delgrp-{group}"),
                edits: vec![e],
            });
        }
    }

    // Mixed: alternate set and delete across the families, so one write
    // touches several blocks at once; GPS coordinates when EXIF is handled.
    let mut mixed = Vec::new();
    for (i, &f) in fams.iter().enumerate() {
        let (present, absent) = split(f);
        if fams.len() == 1 {
            if let Some(a) = absent.first() {
                mixed.push(Edit::set(f.group(), a, MIXED_VALUE));
            }
            if let Some(p) = present.first() {
                mixed.push(Edit::del(f.group(), p));
            }
        } else if i % 2 == 0 || present.is_empty() {
            let name = absent.first().or(present.first());
            if let Some(n) = name {
                mixed.push(Edit::set(f.group(), n, MIXED_VALUE));
            }
        } else {
            mixed.push(Edit::del(f.group(), present[0]));
        }
    }
    if fams.contains(&Fam::Exif) {
        for (name, value) in [("GPSLatitude", "48.8566"), ("GPSLatitudeRef", "N")] {
            let mut e = Edit::set("GPS", name, value);
            e.presence_only = true;
            mixed.push(e);
        }
    }
    if mixed.len() >= 2 {
        cases.push(Case {
            id: "mixed".into(),
            edits: mixed,
        });
    }

    // The original IPTC write cases (accented value, delete, add, multi).
    if file == "IPTC.jpg" {
        let legacy: [(&str, Vec<Edit>); 4] = [
            (
                "iptc-set-accent",
                vec![Edit::set("IPTC", "By-line", "Martín")],
            ),
            ("iptc-delete", vec![Edit::del("IPTC", "By-line")]),
            ("iptc-add", vec![Edit::set("IPTC", "City", "Paris")]),
            (
                "iptc-multi",
                vec![
                    Edit::set("IPTC", "By-line", "Ada"),
                    Edit::set("IPTC", "City", "London"),
                    Edit::del("IPTC", "Headline"),
                ],
            ),
        ];
        for (id, edits) in legacy {
            cases.push(Case {
                id: id.into(),
                edits,
            });
        }
    }
    cases
}

// ── results ─────────────────────────────────────────────────────────────────

/// Verdicts, most severe first. `Iso`, `NoRef` and `BothRefuse` are passes.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Verdict {
    /// `write_info` returned `Ok` but changed nothing that was asked.
    Silent,
    /// `write_info` returned an error where Perl wrote the file.
    Refused,
    /// The output gained an ExifTool `Error`/`Warning`.
    Broken,
    /// Wrong value, or the output differs from Perl's, or our reader disagrees.
    Mismatch,
    /// Perl ExifTool cannot write this; our edits verified on their own.
    NoRef,
    /// Neither tool writes this edit.
    BothRefuse,
    Iso,
}

impl Verdict {
    fn as_str(self) -> &'static str {
        match self {
            Verdict::Silent => "silent",
            Verdict::Refused => "refused",
            Verdict::Broken => "broken",
            Verdict::Mismatch => "mismatch",
            Verdict::NoRef => "noref",
            Verdict::BothRefuse => "both-refuse",
            Verdict::Iso => "iso",
        }
    }
    fn failing(self) -> bool {
        self < Verdict::NoRef
    }
}

/// One differing item, for the report: `(check, key, ours, reference)`.
type Detail = (&'static str, String, String, String);

struct CaseResult {
    file: String,
    format: String,
    id: String,
    edits: String,
    ours: String,
    perl: String,
    /// `ok`, `noop`, `wrong`, or `-` when our write failed.
    expected: &'static str,
    /// `None` when there is no Perl reference.
    perl_iso: Option<bool>,
    reread_ok: bool,
    valid_ok: bool,
    verdict: Verdict,
    details: Vec<Detail>,
}

/// Outcome of one Perl write command, parsed from its stdout/stderr.
struct PerlWrite {
    updated: bool,
    summary: String,
}

fn parse_perl_write(stdout: &str, stderr: &str) -> PerlWrite {
    let updated = stdout
        .lines()
        .any(|l| l.trim_start().starts_with("1 image files updated"));
    let mut msgs: Vec<String> = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    if msgs.is_empty() {
        msgs = stdout
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect();
    }
    let summary = if updated && !stderr.contains("Error") {
        "updated".to_string()
    } else {
        msgs.join(" / ")
    };
    PerlWrite { updated, summary }
}

const END: &str = "@@WRITE-PARITY-END@@";

/// Run every case's Perl write in one process; one result per case.
fn perl_write_all(script: &Path, work: &Path, jobs: &[(Vec<Edit>, PathBuf)]) -> Vec<PerlWrite> {
    let mut args = String::new();
    for (i, (edits, path)) in jobs.iter().enumerate() {
        if i > 0 {
            args.push_str("-execute\n");
        }
        args.push_str("-charset\nUTF8\n-overwrite_original\n");
        for e in edits {
            args.push('-');
            args.push_str(&e.display());
            args.push('\n');
        }
        args.push_str(&path.to_string_lossy());
        args.push('\n');
        args.push_str(&format!("-echo3\n{END}\n-echo4\n{END}\n"));
    }
    let argfile = work.join("perl-args.txt");
    std::fs::write(&argfile, args).expect("write Perl argfile");
    let out = Command::new("perl")
        .arg(script)
        .arg("-@")
        .arg(&argfile)
        .output()
        .expect("run perl exiftool write");
    let so = String::from_utf8_lossy(&out.stdout);
    let se = String::from_utf8_lossy(&out.stderr);
    let so: Vec<&str> = so.split(END).collect();
    let se: Vec<&str> = se.split(END).collect();
    (0..jobs.len())
        .map(|i| {
            let mut r = parse_perl_write(so.get(i).unwrap_or(&""), se.get(i).unwrap_or(&""));
            // Messages name the scratch copy; keep the report free of it.
            r.summary = r.summary.replace(&*jobs[i].1.to_string_lossy(), "<copy>");
            r
        })
        .collect()
}

/// Is a file writable by exiftool-rs? Derived from the code: `write_info`
/// with no edits answers "writing not yet supported" for every type its
/// dispatch does not handle.
fn writable(file: &Path, work: &Path) -> bool {
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("bin");
    let probe = work.join(format!("probe.{ext}"));
    let r = exiftool_rs::ExifTool::new().write_info(file, &probe);
    std::fs::remove_file(&probe).ok();
    !matches!(r, Err(exiftool_rs::Error::UnsupportedFileType(_)))
}

/// Run the whole matrix over `images`. Returns the results and the elapsed
/// seconds. Progress goes to stderr.
fn run_matrix(script: &Path, images: &Path) -> (Vec<CaseResult>, f64) {
    let t0 = Instant::now();
    let work = PathBuf::from(WORK_DIR);
    std::fs::remove_dir_all(&work).ok();
    std::fs::create_dir_all(&work).expect("create write work dir");
    let _cleanup = RemoveOnDrop(work.clone());

    let mut files: Vec<PathBuf> = std::fs::read_dir(images)
        .expect("read images dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    files.sort();

    let files: Vec<PathBuf> = files.into_iter().filter(|f| writable(f, &work)).collect();
    // All inputs in one Perl run: they decide which cases each file gets.
    let mut inputs = read_perl_many(script, &files);

    // Files are independent: spread them over a few threads (each Perl run
    // is a separate process), every file in its own scratch subdirectory.
    let jobs: Vec<(usize, PathBuf, TagMap)> = files
        .iter()
        .enumerate()
        .map(|(i, f)| (i, f.clone(), inputs.remove(f).unwrap_or_default()))
        .collect();
    let queue = std::sync::Mutex::new(jobs.into_iter());
    let threads = std::thread::available_parallelism()
        .map_or(2, |n| n.get())
        .clamp(1, 4);
    let mut per_file: Vec<(usize, Vec<CaseResult>)> = std::thread::scope(|sc| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                sc.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let next = queue.lock().unwrap().next();
                        let Some((i, src, input)) = next else { break };
                        let dir = work.join(i.to_string());
                        std::fs::create_dir_all(&dir).expect("create file work dir");
                        done.push((i, run_file(script, &src, input, &dir)));
                    }
                    done
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("write matrix thread"))
            .collect()
    });
    per_file.sort_by_key(|(i, _)| *i);
    let results = per_file.into_iter().flat_map(|(_, r)| r).collect();
    (results, t0.elapsed().as_secs_f64())
}

/// All cases of one corpus file: our writes, Perl's writes (one process),
/// one Perl re-read of every output, then the verdicts.
fn run_file(script: &Path, src: &Path, input_perl: TagMap, work: &Path) -> Vec<CaseResult> {
    let mut results = Vec::new();
    let fname = src.file_name().unwrap().to_string_lossy().into_owned();
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("bin");
    let format = input_perl
        .iter()
        .find(|(k, _)| k.2 == "FileType")
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| "?".into());
    let cases = build_cases(&fname, &format, &input_perl);
    eprintln!("write: {fname} ({format}) — {} case(s)", cases.len());
    if families_for(&format).is_empty() {
        eprintln!("warning: {fname}: no edit family mapped for FileType {format:?}");
    }
    if cases.is_empty() {
        return Vec::new();
    }
    let src_bytes = std::fs::read(src).expect("read source");
    let input_ours = read_ours(src);

    // Our writes (plus the `GROUP:*` spelling of group deletions, which
    // some writers recognise instead of `GROUP:all`).
    let mut ours_res = Vec::new();
    let mut star_paths = Vec::new();
    let mut jobs = Vec::new();
    for (k, case) in cases.iter().enumerate() {
        let out = work.join(format!("{k}-ours.{ext}"));
        let mut et = exiftool_rs::ExifTool::new();
        for e in &case.edits {
            et.set_new_value(&e.spec(), e.value.as_deref());
        }
        ours_res.push(et.write_info(src, &out));
        let star = if case.edits.iter().any(Edit::is_group_delete) {
            let p = work.join(format!("{k}-star.{ext}"));
            let mut et = exiftool_rs::ExifTool::new();
            for e in &case.edits {
                let spec = if e.is_group_delete() {
                    format!("{}:*", e.group)
                } else {
                    e.spec()
                };
                et.set_new_value(&spec, e.value.as_deref());
            }
            et.write_info(src, &p).ok().map(|_| p)
        } else {
            None
        };
        star_paths.push(star);
        let perl_out = work.join(format!("{k}-perl.{ext}"));
        std::fs::copy(src, &perl_out).expect("copy for Perl");
        jobs.push((case.edits.clone(), perl_out));
    }
    let perl_res = perl_write_all(script, work, &jobs);

    // One Perl re-read of every output.
    let mut to_read: Vec<PathBuf> = Vec::new();
    for (k, r) in ours_res.iter().enumerate() {
        if r.is_ok() {
            to_read.push(work.join(format!("{k}-ours.{ext}")));
        }
        to_read.push(jobs[k].1.clone());
        if let Some(p) = &star_paths[k] {
            to_read.push(p.clone());
        }
    }
    let reread = read_perl_many(script, &to_read);

    let input_deltas = reader_deltas(&input_ours, &input_perl);
    for (k, case) in cases.iter().enumerate() {
        let ours_out = work.join(format!("{k}-ours.{ext}"));
        let ctx = CaseCtx {
            src_bytes: &src_bytes,
            input: &input_perl,
            input_deltas: &input_deltas,
            ours_out: &ours_out,
            ours_res: &ours_res[k],
            perl: &perl_res[k],
            perl_map: reread.get(&jobs[k].1),
            star_map: star_paths[k].as_ref().and_then(|p| reread.get(p)),
            reread: &reread,
        };
        let mut r = evaluate(case, &ctx);
        r.file = fname.clone();
        r.format = format.clone();
        results.push(r);
    }
    std::fs::remove_dir_all(work).ok();
    results
}

struct RemoveOnDrop(PathBuf);
impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// Keys where our reader and Perl's disagree on the same file.
fn reader_deltas(ours: &TagMap, perl: &TagMap) -> BTreeMap<String, (String, String)> {
    let (a, b) = (by_g1(ours), by_g1(perl));
    let keys: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    keys.into_iter()
        .filter(|k| a.get(*k) != b.get(*k))
        .map(|k| {
            (
                k.clone(),
                (
                    a.get(k).cloned().unwrap_or_else(|| "∅".into()),
                    b.get(k).cloned().unwrap_or_else(|| "∅".into()),
                ),
            )
        })
        .collect()
}

struct CaseCtx<'a> {
    src_bytes: &'a [u8],
    input: &'a TagMap,
    input_deltas: &'a BTreeMap<String, (String, String)>,
    ours_out: &'a Path,
    ours_res: &'a exiftool_rs::Result<u32>,
    perl: &'a PerlWrite,
    perl_map: Option<&'a TagMap>,
    star_map: Option<&'a TagMap>,
    reread: &'a BTreeMap<PathBuf, TagMap>,
}

fn diag(m: &TagMap) -> Vec<String> {
    m.iter()
        .filter(|(k, _)| k.0 == "ExifTool" && (k.2 == "Error" || k.2 == "Warning"))
        .map(|(k, v)| format!("{}: {v}", k.2))
        .collect()
}

fn evaluate(case: &Case, c: &CaseCtx) -> CaseResult {
    let edits = case
        .edits
        .iter()
        .map(Edit::display)
        .collect::<Vec<_>>()
        .join(" ");
    let perl_ref = if c.perl.updated { c.perl_map } else { None };
    let mut r = CaseResult {
        file: String::new(),
        format: String::new(),
        id: case.id.clone(),
        edits,
        ours: String::new(),
        perl: c.perl.summary.clone(),
        expected: "-",
        perl_iso: None,
        reread_ok: true,
        valid_ok: true,
        verdict: Verdict::Iso,
        details: Vec::new(),
    };

    let n = match c.ours_res {
        Ok(n) => *n,
        Err(e) => {
            r.ours = format!("Err: {e}");
            r.verdict = if c.perl.updated {
                Verdict::Refused
            } else {
                Verdict::BothRefuse
            };
            return r;
        }
    };
    r.ours = format!("Ok({n})");
    let out_bytes = std::fs::read(c.ours_out).unwrap_or_default();
    let identical = out_bytes == c.src_bytes;
    let Some(out_perl) = c.reread.get(c.ours_out) else {
        r.valid_ok = false;
        r.verdict = Verdict::Broken;
        r.details.push((
            "valid",
            "file".into(),
            "Perl could not read the output".into(),
            String::new(),
        ));
        return r;
    };

    // (a) expected state, judged by Perl's re-read of our output.
    let mut noop = false;
    let mut wrong = false;
    for e in &case.edits {
        if e.satisfied(out_perl) {
            continue;
        }
        let perl_did = perl_ref.is_none_or(|m| e.satisfied(m));
        let got = e.state(out_perl);
        let unchanged = got == e.state(c.input);
        let want = match &e.value {
            Some(v) if e.presence_only => format!("present ({v})"),
            Some(v) => v.clone(),
            None => "∅".into(),
        };
        let mut note = got.unwrap_or_else(|| "∅".into());
        if unchanged {
            note = format!("unchanged: {note}");
        }
        if e.is_group_delete() {
            if let Some(m) = c.star_map {
                let star_ok = e.satisfied(m);
                note.push_str(if star_ok {
                    " (GROUP:* spelling works)"
                } else {
                    " (GROUP:* spelling fails too)"
                });
            }
        }
        if !perl_did {
            note.push_str(" (Perl's output does not satisfy it either)");
        }
        r.details.push(("expected", e.display(), note, want));
        if !perl_did {
            continue;
        }
        if unchanged {
            noop = true;
        } else {
            wrong = true;
        }
    }
    r.expected = if noop {
        "noop"
    } else if wrong {
        "wrong"
    } else {
        "ok"
    };

    // (b) Perl-identical.
    if let Some(pm) = perl_ref {
        let (a, b) = (by_g1(out_perl), by_g1(pm));
        let keys: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
        let mut iso = true;
        for k in keys {
            let (x, y) = (a.get(k), b.get(k));
            if x != y {
                iso = false;
                r.details.push((
                    "perl-identical",
                    k.clone(),
                    x.cloned().unwrap_or_else(|| "∅".into()),
                    y.cloned().unwrap_or_else(|| "∅".into()),
                ));
            }
        }
        r.perl_iso = Some(iso);
    }

    // (c) our reader vs Perl's on our output, beyond the input's deltas.
    let ours_reread = read_ours(c.ours_out);
    for (k, (o, p)) in reader_deltas(&ours_reread, out_perl) {
        if c.input_deltas.contains_key(&k) {
            continue;
        }
        r.reread_ok = false;
        r.details.push(("reread", k, o, p));
    }

    // (d) validity: diagnostics Perl reports on our output only.
    let before: BTreeSet<String> = diag(c.input).into_iter().collect();
    let theirs: BTreeSet<String> = c
        .perl_map
        .map(diag)
        .unwrap_or_default()
        .into_iter()
        .collect();
    for d in diag(out_perl) {
        if !before.contains(&d) && !theirs.contains(&d) {
            r.valid_ok = false;
            r.details
                .push(("valid", "ExifTool".into(), d, String::new()));
        }
    }

    // (e) verdict, most severe first.
    let any_change_expected = !case.edits.is_empty();
    r.verdict = if n > 0 && identical && any_change_expected {
        r.details.push((
            "silent",
            "file".into(),
            format!("Ok({n}) but the output is byte-identical to the input"),
            String::new(),
        ));
        Verdict::Silent
    } else if noop {
        Verdict::Silent
    } else if !r.valid_ok {
        Verdict::Broken
    } else if wrong || r.perl_iso == Some(false) || !r.reread_ok {
        Verdict::Mismatch
    } else if perl_ref.is_none() {
        Verdict::NoRef
    } else {
        Verdict::Iso
    };
    r
}

// ── entry points ────────────────────────────────────────────────────────────

fn clean(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

/// `--dump`: the write section (header, ignored tags, cases, details, timing).
pub fn dump(script: &Path, images: &Path) {
    let (results, secs) = run_matrix(script, images);
    println!("{DUMP_HEADER}");
    for (t, why) in IGNORED {
        println!("I\t{t}\t{why}");
    }
    for e in EXCLUDE {
        println!("I\t{e}\tfile-system or tool state, not metadata");
    }
    for r in &results {
        println!(
            "C\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            r.file,
            clean(&r.format),
            r.id,
            clean(&r.edits),
            clean(&r.ours),
            clean(&r.perl),
            r.expected,
            match r.perl_iso {
                Some(true) => "1",
                Some(false) => "0",
                None => "-",
            },
            u8::from(r.reread_ok),
            u8::from(r.valid_ok),
            r.verdict.as_str()
        );
        for (check, key, ours, other) in &r.details {
            println!(
                "D\t{}\t{}\t{check}\t{}\t{}\t{}",
                r.file,
                r.id,
                clean(key),
                clean(ours),
                clean(other)
            );
        }
    }
    let files: BTreeSet<&str> = results.iter().map(|r| r.file.as_str()).collect();
    println!("T\t{secs:.1}\t{}\t{}", files.len(), results.len());
}

/// Default mode: per-format table, then the failing cases not in the
/// baseline. Returns the number of new failures (0 after `--update`).
pub fn check(script: &Path, images: &Path, update: bool) -> usize {
    let (results, secs) = run_matrix(script, images);
    println!(
        "\nWRITE PARITY — every corpus file exiftool-rs writes, edited by both tools, \
         re-read by Perl ExifTool and by exiftool-rs"
    );
    println!(
        "  exiftool-rs: set_new_value + write_info (in-process)\n  \
         ExifTool:    perl <exiftool> -charset UTF8 -overwrite_original <edits> <copy>\n  \
         re-read:     perl <exiftool> -a -s -G0:1 <output>   and   extract_info(<output>)"
    );
    let mut by_fmt: BTreeMap<&str, Vec<&CaseResult>> = BTreeMap::new();
    for r in &results {
        by_fmt.entry(r.format.as_str()).or_default().push(r);
    }
    let count = |v: &[&CaseResult], d: Verdict| v.iter().filter(|r| r.verdict == d).count();
    let rows: Vec<Vec<String>> = by_fmt
        .iter()
        .map(|(f, v)| {
            let files: BTreeSet<&str> = v.iter().map(|r| r.file.as_str()).collect();
            vec![
                (*f).to_string(),
                files.len().to_string(),
                v.len().to_string(),
                count(v, Verdict::Iso).to_string(),
                count(v, Verdict::Mismatch).to_string(),
                count(v, Verdict::Silent).to_string(),
                count(v, Verdict::Refused).to_string(),
                count(v, Verdict::Broken).to_string(),
                (count(v, Verdict::NoRef) + count(v, Verdict::BothRefuse)).to_string(),
            ]
        })
        .collect();
    print_table(
        &[
            "Format", "Files", "Cases", "ISO", "Mismatch", "Silent", "Refused", "Broken", "No ref",
        ],
        &rows,
    );

    let current: BTreeSet<String> = results
        .iter()
        .filter(|r| r.verdict.failing())
        .map(|r| format!("{}::{}::{}", r.file, r.id, r.verdict.as_str()))
        .collect();
    if update {
        let mut s = String::from(
            "# Live write-parity baseline: known failing write cases (file::case::verdict).\n\
             # Refresh with `--update`. A failing case not listed here fails the run.\n",
        );
        for k in &current {
            s.push_str(k);
            s.push('\n');
        }
        std::fs::write(WRITE_BASELINE, s).expect("write write baseline");
        println!(
            "write: baseline refreshed — {} known failing case(s) of {} ({secs:.1} s)",
            current.len(),
            results.len()
        );
        return 0;
    }
    let baseline: BTreeSet<String> = std::fs::read_to_string(WRITE_BASELINE)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(str::to_string)
        .collect();
    let new: Vec<&String> = current.difference(&baseline).collect();
    let fixed = baseline.difference(&current).count();
    print_table(
        &["Cases", "Failing", "New (fail)", "Seconds", "ISO"],
        &[vec![
            results.len().to_string(),
            current.len().to_string(),
            new.len().to_string(),
            format!("{secs:.1}"),
            if new.is_empty() { "✓" } else { "✗" }.into(),
        ]],
    );
    if !new.is_empty() {
        let rows: Vec<Vec<String>> = new
            .iter()
            .map(|k| {
                let mut it = k.splitn(3, "::");
                let (f, c, v) = (
                    it.next().unwrap_or(""),
                    it.next().unwrap_or(""),
                    it.next().unwrap_or(""),
                );
                let edits = results
                    .iter()
                    .find(|r| r.file == f && r.id == c)
                    .map(|r| r.edits.clone())
                    .unwrap_or_default();
                vec![f.into(), c.into(), edits, v.into()]
            })
            .collect();
        print_table(&["File", "Case", "Edits", "Verdict"], &rows);
    }
    if fixed > 0 {
        println!(
            "  ({fixed} baselined write failure(s) no longer occur — run --update to tighten)"
        );
    }
    new.len()
}
