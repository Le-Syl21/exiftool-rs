//! QuickTime (MOV/M4A) metadata writing, checked against ExifTool 13.59,
//! and the media writers ExifTool has no counterpart for (ID3, Vorbis
//! comments in FLAC/Ogg/Opus, Matroska), checked by reading back.
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

/// `Group1:Tag=Value` lines of a file, as a multiset, for the groups
/// `keep` accepts.
fn read(path: &Path, keep: impl Fn(&str, &str) -> bool) -> BTreeMap<String, usize> {
    let mut et = ExifTool::new();
    et.options_mut().duplicates = true;
    let mut m = BTreeMap::new();
    for t in et.extract_info(path).unwrap() {
        if keep(&t.group.family0, &t.group.family1) && t.name != "XMPToolkit" {
            *m.entry(format!("{}:{}={}", t.group.family1, t.name, t.print_value))
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

/// Tags of `g0` before and after, for the no-reference formats: the edit
/// must be there and nothing else may have changed.
fn check_noref(src: &str, g0: &str, ops: &[Op], changed: &[(&str, Option<&str>)]) {
    let ext = src.rsplit('.').next().unwrap();
    let name = format!("noref_{}_{}.{ext}", src.replace('.', "_"), ops.len());
    let (out, r) = write(src, &name, ops);
    assert!(r.is_ok(), "{src} {ops:?}: {r:?}");
    let keep = |a: &str, _: &str| a == g0;
    let mut before = read(&Path::new("tests/images").join(src), keep);
    let after = read(&out, keep);
    for (key, value) in changed {
        // `key` is `Group1:Tag`; drop every old instance, add the new one.
        before.retain(|k, _| !k.starts_with(&format!("{key}=")));
        if let Some(v) = value {
            *before.entry(format!("{key}={v}")).or_default() += 1;
        }
    }
    assert_eq!(after, before, "{src} {ops:?}");
}

#[test]
fn id3_edits_keep_the_other_frames() {
    check_noref(
        "MP3.mp3",
        "ID3",
        &[("ID3:Title", Some("New title")), ("ID3:Composer", None)],
        &[
            ("ID3v2_2:Title", Some("New title")),
            ("ID3v1:Title", Some("New title")),
            ("ID3v2_2:Composer", None),
        ],
    );
    check_noref(
        "AIFF.aif",
        "ID3",
        &[("ID3:Album", Some("Älbum")), ("ID3:Lyrics", None)],
        &[("ID3v2_2:Album", Some("Älbum")), ("ID3v2_2:Lyrics", None)],
    );
}

#[test]
fn vorbis_comment_edits_keep_the_others() {
    check_noref(
        "FLAC.flac",
        "Vorbis",
        &[("Vorbis:Title", Some("New")), ("Vorbis:Copyright", None)],
        &[("Vorbis:Title", Some("New")), ("Vorbis:Copyright", None)],
    );
    check_noref(
        "Vorbis.ogg",
        "Vorbis",
        &[("Vorbis:Genre", Some("Jazz")), ("Vorbis:Comment", None)],
        &[("Vorbis:Genre", Some("Jazz")), ("Vorbis:Comment", None)],
    );
    check_noref(
        "Opus.opus",
        "Vorbis",
        &[("Vorbis:Artist", Some("Someone"))],
        &[("Vorbis:Artist", Some("Someone"))],
    );
    check_noref(
        "FLAC.ogg",
        "Vorbis",
        &[("Vorbis:Album", Some("An album"))],
        &[("Vorbis:Album", Some("An album"))],
    );
}

#[test]
fn matroska_title() {
    check_noref(
        "Matroska.mkv",
        "Matroska",
        &[("Matroska:Title", Some("A title"))],
        &[("Info:Title", Some("A title"))],
    );
}

#[test]
fn unwritable_requests_are_errors() {
    // Nothing of these can be written: an error, and no output file.
    for (src, op) in [
        ("MP3.mp3", ("EXIF:Artist", Some("x"))),
        ("FLAC.flac", ("XMP:Title", Some("x"))),
        ("QuickTime.mov", ("IPTC:City", Some("x"))),
    ] {
        std::fs::remove_file(tmp(&format!("err_{src}"))).ok();
        let (out, r) = write(src, &format!("err_{src}"), &[op]);
        assert!(r.is_err(), "{src} {op:?}");
        assert!(!out.exists(), "{src}: an output was written");
    }
}
