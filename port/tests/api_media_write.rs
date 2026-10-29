//! QuickTime (MOV/M4A) metadata writing, checked against ExifTool 13.59,
//! and the formats ExifTool does not write (ID3, Vorbis comments in
//! FLAC/Ogg/Opus, Matroska), which it refuses.
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

use exiftool_port::ExifTool;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

type Op = (&'static str, Option<&'static str>);

fn tmp(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("media_write");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn write(src: &str, out: &str, ops: &[Op]) -> (PathBuf, exiftool_port::Result<u32>) {
    let mut et = ExifTool::new();
    for (tag, v) in ops {
        et.set_new_value(tag, *v);
    }
    let out = tmp(out);
    let r = et.write_info(Path::new("../tests/images").join(src), &out);
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
    let text = std::fs::read_to_string(format!("../tests/expected_media/{case}.txt")).unwrap();
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
    let srcd = std::fs::read(Path::new("../tests/images").join(src)).unwrap();
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

/// ExifTool 13.59 does not write these formats: as the exiftool application
/// (`exiftool -o out.mp3 -Title=x MP3.mp3`: "Can't write MP3 files"),
/// `write_info` refuses them and writes nothing.
fn check_refused(src: &str, ops: &[Op], kind: &str) {
    let ext = src.rsplit('.').next().unwrap();
    let name = format!("refused_{}_{}.{ext}", src.replace('.', "_"), ops.len());
    let _ = std::fs::remove_file(tmp(&name));
    let (out, r) = write(src, &name, ops);
    match r {
        Err(exiftool_port::Error::UnsupportedFileType(msg)) => {
            assert_eq!(msg, format!("Can't write {kind} files"), "{src} {ops:?}")
        }
        other => panic!("{src} {ops:?}: {other:?}"),
    }
    assert!(!out.exists(), "{src}: output written");
}

#[test]
fn id3_is_not_written() {
    check_refused("MP3.mp3", &[("ID3:Title", Some("New title")), ("ID3:Composer", None)], "MP3");
    check_refused("AIFF.aif", &[("ID3:Album", Some("Älbum")), ("ID3:Lyrics", None)], "AIFF");
}

#[test]
fn vorbis_comments_are_not_written() {
    check_refused("FLAC.flac", &[("Vorbis:Title", Some("New")), ("Vorbis:Copyright", None)], "FLAC");
    check_refused("Vorbis.ogg", &[("Vorbis:Genre", Some("Jazz")), ("Vorbis:Comment", None)], "OGG");
    check_refused("Opus.opus", &[("Vorbis:Artist", Some("Someone"))], "OGG");
    check_refused("FLAC.ogg", &[("Vorbis:Album", Some("An album"))], "OGG");
}

#[test]
fn matroska_is_not_written() {
    check_refused("Matroska.mkv", &[("Matroska:Title", Some("A title"))], "MKV");
}

#[test]
fn unwritable_requests_are_skipped() {
    // Nothing of this can be written: as ExifTool 13.59 does with a request
    // it cannot apply (`-IPTC:City=x` on a MOV: "1 image files copied"), the
    // output is an unchanged copy and nothing is counted. ExifTool does not
    // warn about it (IPTC:City is a tag it can write, only not here).
    let src = "QuickTime.mov";
    let mut et = ExifTool::new();
    et.set_new_value("IPTC:City", Some("x"));
    let out = tmp(&format!("skip_{src}"));
    let input = Path::new("../tests/images").join(src);
    assert_eq!(et.write_info(&input, &out).unwrap(), 0, "{src}");
    assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(&input).unwrap(), "{src}");
    assert_eq!(et.write_warnings(), Vec::<String>::new(), "{src}");
    // ExifTool writes neither MP3 nor FLAC at all.
    check_refused("MP3.mp3", &[("EXIF:Artist", Some("x"))], "MP3");
    check_refused("FLAC.flac", &[("XMP:Title", Some("x"))], "FLAC");
}
