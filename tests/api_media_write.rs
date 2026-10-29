//! QuickTime (MOV/M4A) metadata writing, checked against ExifTool 13.59,
//! and the formats ExifTool does not write (ID3, Vorbis comments in
//! FLAC/Ogg/Opus, Matroska), written by the built-in extensions and read
//! back by the port and by Perl ExifTool.
//!
//! The QuickTime cases apply the same edit ExifTool was given and compare
//! the ItemList, UserData, Keys and XMP tags of our output, as our reader
//! sees them, with what ExifTool read from *its* output. The baselines in
//! `tests/expected_media/` were made with, for each case:
//!
//! ```sh
//! perl exiftool -charset UTF8 -o out.mov <edits> tests/images/QuickTime.mov
//! perl exiftool -a -s -G0:1 -charset UTF8 -QuickTime:all -XMP:all out.mov
//! ```
//!
//! keeping the `QuickTime:ItemList|UserData|Keys|iTunes` and `XMP:XMP-*`
//! lines (not `XMPToolkit`, which names the writing tool) as
//! `Group1:Tag=Value`.

use exiftool_rs::ExifTool;

mod common;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

type Op = (&'static str, Option<&'static str>);

fn tmp(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("media_write");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn write(src: &str, out: &str, ops: &[Op]) -> (PathBuf, exiftool_rs::Result<u32>) {
    let mut et = ExifTool::new();
    for (tag, v) in ops {
        et.set_new_value(tag, *v);
    }
    let out = tmp(out);
    let r = et.write_info(Path::new("tests/images").join(src), &out);
    (out, r)
}

/// A printed value as `exiftool -s` shows it (the baselines' form): control
/// characters as dots, NULs dropped, trailing white space trimmed.
fn as_printed(v: &str) -> String {
    let mapped: String = v
        .chars()
        .filter(|&c| c != '\0')
        .map(|c| if ('\u{01}'..='\u{1f}').contains(&c) || c == '\u{7f}' { '.' } else { c })
        .collect();
    mapped.trim_end().to_string()
}

/// `Group1:Tag=Value` lines of a file, as a multiset, for the groups
/// `keep` accepts.
fn read(path: &Path, keep: impl Fn(&str, &str) -> bool) -> BTreeMap<String, usize> {
    let mut et = ExifTool::new();
    et.options_mut().duplicates = true;
    let mut m = BTreeMap::new();
    for t in et.extract_info(path).unwrap() {
        if keep(&t.group.family0, &t.group.family1) && t.name != "XMPToolkit" {
            *m.entry(format!("{}:{}={}", t.group.family1, t.name, as_printed(&t.print_value)))
                .or_default() += 1;
        }
    }
    m
}

fn quicktime_groups(g0: &str, g1: &str) -> bool {
    (g0 == "QuickTime" && matches!(g1, "ItemList" | "UserData" | "Keys" | "iTunes"))
        || (g0 == "XMP" && g1.starts_with("XMP-"))
}

fn expected(case: &str) -> BTreeMap<String, usize> {
    let text = std::fs::read_to_string(format!("tests/expected_media/{case}.txt")).unwrap();
    let mut m = BTreeMap::new();
    for l in text.lines().filter(|l| !l.is_empty()) {
        *m.entry(l.to_string()).or_default() += 1;
    }
    m
}

fn check_quicktime(case: &str, src: &str, ops: &[Op]) {
    let ext = src.rsplit('.').next().unwrap();
    let (out, r) = write(src, &format!("{case}.{ext}"), ops);
    let n = r.unwrap_or_else(|e| panic!("{case}: {e}"));
    assert_eq!(n as usize, ops.len(), "{case}: applied count");
    let ours = read(&out, quicktime_groups);
    let theirs = expected(case);
    let only = |a: &BTreeMap<String, usize>, b: &BTreeMap<String, usize>| -> Vec<String> {
        a.iter()
            .filter(|(k, n)| b.get(*k) != Some(n))
            .map(|(k, _)| k.clone())
            .collect()
    };
    assert!(
        ours == theirs,
        "{case}: ours only {:?}\nExifTool only {:?}",
        only(&ours, &theirs),
        only(&theirs, &ours)
    );
    // The movie data that follows the metadata is still where the file
    // says it is: the mdat box is intact and reads the same.
    let data = std::fs::read(&out).unwrap();
    let srcd = std::fs::read(Path::new("tests/images").join(src)).unwrap();
    let mdat = |d: &[u8]| {
        d.windows(4)
            .position(|w| w == b"mdat")
            .map(|i| d[i - 4..].to_vec())
    };
    assert_eq!(mdat(&data), mdat(&srcd), "{case}: mdat changed");
}

#[test]
fn quicktime_mov_matches_exiftool() {
    let cases: &[(&str, &[Op])] = &[
        (
            "mov_set_artist",
            &[("QuickTime:Artist", Some("Set by test"))],
        ),
        ("mov_add_title", &[("QuickTime:Title", Some("Added title"))]),
        ("mov_del_genre", &[("QuickTime:Genre", None)]),
        ("mov_del_keys_artist", &[("Keys:Artist", None)]),
        ("mov_itemlist_all", &[("ItemList:all", None)]),
        ("mov_userdata_all", &[("UserData:all", None)]),
        (
            "mov_userdata_accent",
            &[
                ("UserData:Artist", Some("Ärtïst")),
                ("UserData:Title", Some("Tîtle")),
            ],
        ),
        (
            "mov_xmp_edit",
            &[
                ("XMP:Creator", Some("New creator")),
                ("XMP:Description", Some("Added description")),
            ],
        ),
        ("mov_xmp_all", &[("XMP:all", None)]),
    ];
    for (case, ops) in cases {
        check_quicktime(case, "QuickTime.mov", ops);
    }
}

#[test]
fn quicktime_m4a_matches_exiftool() {
    let cases: &[(&str, &[Op])] = &[
        (
            "m4a_title_xmp",
            &[
                ("QuickTime:Title", Some("New title")),
                ("XMP:Description", Some("A description")),
            ],
        ),
        ("m4a_quicktime_all", &[("QuickTime:all", None)]),
        (
            "m4a_mixed",
            &[
                ("QuickTime:Album", None),
                ("ItemList:Comment", Some("New comment")),
                ("QuickTime:Copyright", Some("(c) test")),
            ],
        ),
    ];
    for (case, ops) in cases {
        check_quicktime(case, "QuickTime.m4a", ops);
    }
}

/// Perl ExifTool, to read a written file back: `$EXIFTOOL_DIR/exiftool`
/// (default `../exiftool`, next to the repository: tests/common/mod.rs). `None`
/// when it is not there or perl is missing.
fn perl_exiftool() -> Option<PathBuf> {
    let script = common::exiftool_dir("reading the written files back with Perl ExifTool")?.join("exiftool");
    let perl = std::process::Command::new("perl").arg("-e").arg("1").output().map(|o| o.status.success()).unwrap_or(false);
    (perl && script.is_file()).then_some(script)
}

/// `Group1:Tag=Value` lines of a file as Perl ExifTool 13.59 reads it
/// (`exiftool -a -G0:1 -s -charset UTF8`), for the family 0 group `g0`.
fn read_perl(script: &Path, path: &Path, g0: &str) -> BTreeMap<String, usize> {
    let mut cmd = if cfg!(target_os = "linux") {
        let mut c = std::process::Command::new("prlimit");
        c.args(["--as=4000000000", "--", "timeout", "60", "perl"]);
        c
    } else {
        std::process::Command::new("perl")
    };
    let out = cmd.arg(script).args(["-a", "-G0:1", "-s", "-charset", "UTF8"]).arg(path).output().expect("run perl exiftool");
    assert!(out.status.success(), "perl exiftool {}: {}", path.display(), String::from_utf8_lossy(&out.stderr));
    let mut m = BTreeMap::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        // "[ID3:ID3v2_2]   Title    : value"
        let Some(rest) = line.strip_prefix('[') else { continue };
        let Some((groups, rest)) = rest.split_once(']') else { continue };
        // (one name when both families have it: "[Vorbis]")
        let (f0, f1) = groups.split_once(':').unwrap_or((groups, groups));
        let Some((name, value)) = rest.split_once(" : ") else { continue };
        if f0 == g0 {
            *m.entry(format!("{f1}:{}={}", name.trim(), value)).or_default() += 1;
        }
    }
    m
}

/// `before` with the `changed` tags (`Group1:Tag`) dropped, and given their
/// new value when there is one.
fn edited(mut before: BTreeMap<String, usize>, changed: &[(&str, Option<&str>)]) -> BTreeMap<String, usize> {
    for (key, value) in changed {
        before.retain(|k, _| !k.starts_with(&format!("{key}=")));
        if let Some(v) = value {
            *before.entry(format!("{key}={v}")).or_default() += 1;
        }
    }
    before
}

/// The formats Perl ExifTool reads but does not write ("Can't write MP3
/// files"), written by the built-in extensions (feature `extra-writers`):
/// the edit must be there and nothing else of `g0` may have changed, as the
/// port's reader reads the result, and as Perl ExifTool 13.59 reads it back.
fn check_extension(src: &str, g0: &str, ops: &[Op], changed: &[(&str, Option<&str>)]) {
    let ext = src.rsplit('.').next().unwrap();
    let name = format!("ext_{}_{}.{ext}", src.replace('.', "_"), ops.len());
    let (out, r) = write(src, &name, ops);
    let n = r.unwrap_or_else(|e| panic!("{src} {ops:?}: {e}"));
    assert_eq!(n as usize, ops.len(), "{src} {ops:?}: applied count");
    let input = Path::new("tests/images").join(src);
    let keep = |a: &str, _: &str| a == g0;
    assert_eq!(read(&out, keep), edited(read(&input, keep), changed), "{src} {ops:?}: read by the port");
    match perl_exiftool() {
        Some(script) => assert_eq!(read_perl(&script, &out, g0), edited(read_perl(&script, &input, g0), changed), "{src} {ops:?}: read by Perl ExifTool"),
        None => eprintln!("Perl ExifTool not found (EXIFTOOL_DIR): {src} not read back by it"),
    }
}

#[test]
fn id3_edits_keep_the_other_frames() {
    check_extension(
        "MP3.mp3",
        "ID3",
        &[("ID3:Title", Some("New title")), ("ID3:Composer", None)],
        &[("ID3v2_2:Title", Some("New title")), ("ID3v1:Title", Some("New title")), ("ID3v2_2:Composer", None)],
    );
    check_extension("AIFF.aif", "ID3", &[("ID3:Album", Some("Älbum")), ("ID3:Lyrics", None)], &[("ID3v2_2:Album", Some("Älbum")), ("ID3v2_2:Lyrics", None)]);
}

#[test]
fn vorbis_comment_edits_keep_the_others() {
    check_extension("FLAC.flac", "Vorbis", &[("Vorbis:Title", Some("New")), ("Vorbis:Copyright", None)], &[("Vorbis:Title", Some("New")), ("Vorbis:Copyright", None)]);
    check_extension("Vorbis.ogg", "Vorbis", &[("Vorbis:Genre", Some("Jazz")), ("Vorbis:Comment", None)], &[("Vorbis:Genre", Some("Jazz")), ("Vorbis:Comment", None)]);
    check_extension("Opus.opus", "Vorbis", &[("Vorbis:Artist", Some("Someone"))], &[("Vorbis:Artist", Some("Someone"))]);
    check_extension("FLAC.ogg", "Vorbis", &[("Vorbis:Album", Some("An album"))], &[("Vorbis:Album", Some("An album"))]);
}

#[test]
fn matroska_title_and_tags() {
    check_extension("Matroska.mkv", "Matroska", &[("Matroska:Title", Some("A title"))], &[("Info:Title", Some("A title"))]);
    check_extension(
        "Matroska.mkv",
        "Matroska",
        &[("Matroska:Artist", Some("An artist")), ("Matroska:Title", Some("Another title"))],
        &[("Info:Title", Some("Another title")), ("Matroska:Artist", Some("An artist"))],
    );
}

#[test]
fn unwritable_requests_are_skipped() {
    // Nothing of this can be written: as ExifTool 13.59 does with a request
    // it cannot apply (`-IPTC:City=x` on a MOV: "1 image files copied"), the
    // output is an unchanged copy and nothing is counted. ExifTool does not
    // warn about it (IPTC:City is a tag it can write, only not here); nor do
    // the extensions, for a group they do not write (EXIF in an MP3, XMP in
    // a FLAC).
    for (src, op) in [("QuickTime.mov", ("IPTC:City", Some("x"))), ("MP3.mp3", ("EXIF:Artist", Some("x"))), ("FLAC.flac", ("XMP:Title", Some("x")))] {
        let mut et = ExifTool::new();
        et.set_new_value(op.0, op.1);
        let out = tmp(&format!("skip_{src}"));
        let input = Path::new("tests/images").join(src);
        assert_eq!(et.write_info(&input, &out).unwrap(), 0, "{src}");
        assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(&input).unwrap(), "{src}");
        assert_eq!(et.write_warnings(), Vec::<String>::new(), "{src}");
    }
}

/// Without the extensions, ExifTool 13.59 does not write these formats: as
/// the exiftool application (`exiftool -o out.mp3 -Title=x MP3.mp3`: "Can't
/// write MP3 files"), `write_info` refuses them and writes nothing.
#[test]
fn without_extensions_these_formats_are_refused() {
    for (src, op, kind) in [
        ("MP3.mp3", ("ID3:Title", Some("New title")), "MP3"),
        ("AIFF.aif", ("ID3:Album", Some("x")), "AIFF"),
        ("FLAC.flac", ("Vorbis:Title", Some("New")), "FLAC"),
        ("Vorbis.ogg", ("Vorbis:Genre", Some("Jazz")), "OGG"),
        ("Opus.opus", ("Vorbis:Artist", Some("Someone")), "OGG"),
        ("Matroska.mkv", ("Matroska:Title", Some("A title")), "MKV"),
    ] {
        let mut et = ExifTool::new();
        et.clear_extensions();
        et.set_new_value(op.0, op.1);
        let ext = src.rsplit('.').next().unwrap();
        let out = tmp(&format!("refused_{}.{ext}", src.replace('.', "_")));
        let _ = std::fs::remove_file(&out);
        match et.write_info(Path::new("tests/images").join(src), &out) {
            Err(exiftool_rs::Error::UnsupportedFileType(msg)) => assert_eq!(msg, format!("Can't write {kind} files"), "{src}"),
            other => panic!("{src}: {other:?}"),
        }
        assert!(!out.exists(), "{src}: output written");
    }
}

/// An extension that counts its calls and writes the input unchanged.
struct Counting(std::sync::Arc<std::sync::atomic::AtomicUsize>, &'static [&'static str]);

impl exiftool_rs::FormatExtension for Counting {
    fn name(&self) -> &str {
        "counting"
    }
    fn file_types(&self) -> &[&str] {
        self.1
    }
    fn write(&self, input: &[u8], _file_type: &str, _edits: &[exiftool_rs::NewValue]) -> exiftool_rs::Result<Vec<u8>> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(input.to_vec())
    }
}

#[test]
fn extensions_are_called_only_for_what_exiftool_cannot_write() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    // A type ExifTool writes is ExifTool's, whatever is registered.
    let mut et = ExifTool::new();
    et.register_extension(Counting(calls.clone(), &["JPEG"]));
    et.set_new_value("EXIF:Artist", Some("Someone"));
    assert_eq!(et.write_info(Path::new("tests/images/Canon.jpg"), tmp("ext_canon.jpg")).unwrap(), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    // The last registered for a type writes it (here, in place of the
    // built-in ID3 writer: the copy is unchanged, so nothing is counted).
    et.clear_new_values();
    et.register_extension(Counting(calls.clone(), &["MP3"]));
    et.set_new_value("ID3:Title", Some("x"));
    let out = tmp("ext_counting.mp3");
    let input = Path::new("tests/images/MP3.mp3");
    assert_eq!(et.write_info(input, &out).unwrap(), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(input).unwrap());
    // A destination ExifTool names another type is refused, as ExifTool
    // refuses it.
    let r = et.write_info(input, tmp("ext_counting.flac"));
    assert!(matches!(&r, Err(exiftool_rs::Error::UnsupportedFileType(m)) if m == "Can't write FLAC files"), "{r:?}");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
