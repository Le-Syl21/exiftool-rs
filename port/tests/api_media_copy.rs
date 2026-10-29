//! Copies (`set_new_values_from_file`) between the formats ExifTool does not
//! write, written by the built-in extensions: as ExifTool's
//! `SetNewValuesFromFile` does, a tag copied without a destination group
//! goes to the destination's preferred group for its name, so a FLAC's
//! Vorbis Title becomes an MP3's ID3 Title; a group the tag list names is
//! kept. Each output is read back by the port and, when found at
//! `$EXIFTOOL_DIR`, by Perl ExifTool 13.59.

use exiftool_port::ExifTool;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn tmp(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("media_copy");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn image(name: &str) -> PathBuf {
    Path::new("../tests/images").join(name)
}

/// The six formats: test image, extension, family 0 group of its metadata.
const FORMATS: &[(&str, &str, &str)] = &[
    ("MP3.mp3", "mp3", "ID3"),
    ("AIFF.aif", "aif", "ID3"),
    ("FLAC.flac", "flac", "Vorbis"),
    ("Vorbis.ogg", "ogg", "Vorbis"),
    ("Opus.opus", "opus", "Vorbis"),
    ("Matroska.mkv", "mkv", "Matroska"),
];

/// The tags of the test that each format has, by ExifTool's names (ID3.pm,
/// the Vorbis.pm comments, Matroska.pm's segment Title and standardized
/// SimpleTags).
fn tags_of(g0: &str) -> &'static [&'static str] {
    match g0 {
        "ID3" => &["Title", "Artist", "Album", "Genre", "Comment", "Composer", "Copyright", "EncodedBy", "Lyrics", "Subtitle", "Conductor", "Publisher"],
        "Vorbis" => &["Title", "Artist", "Album", "Genre", "Comment", "Composer", "Copyright", "EncodedBy", "Description", "Director", "License"],
        _ => &["Title", "Artist", "Genre", "Comment", "Composer", "Copyright", "EncodedBy", "Lyrics", "Subtitle", "Conductor", "Publisher", "Description", "Director", "License"],
    }
}

fn value(tag: &str, ext: &str) -> String {
    format!("{tag} of {ext}")
}

/// (Group1, Tag, Value) of a file's `g0` tags as the port reads them.
fn read_port(path: &Path, g0: &str) -> BTreeSet<(String, String, String)> {
    let mut et = ExifTool::new();
    et.options_mut().duplicates = true;
    et.extract_info(path).unwrap().into_iter().filter(|t| t.group.family0 == g0).map(|t| (t.group.family1, t.name, t.print_value)).collect()
}

/// Perl ExifTool, to read a written file back: `$EXIFTOOL_DIR/exiftool`
/// (default `../../exiftool`, next to the exiftool-rs checkout).
fn perl_exiftool() -> Option<PathBuf> {
    let dir = std::env::var_os("EXIFTOOL_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("../../exiftool"));
    let script = dir.join("exiftool");
    let perl = std::process::Command::new("perl").arg("-e").arg("1").output().map(|o| o.status.success()).unwrap_or(false);
    (perl && script.is_file()).then_some(script)
}

/// Perl under the limits of the test machine (address space, time).
fn perl(script: &Path) -> std::process::Command {
    let mut cmd = if cfg!(target_os = "linux") {
        let mut c = std::process::Command::new("prlimit");
        c.args(["--as=4000000000", "--", "timeout", "60", "perl"]);
        c
    } else {
        std::process::Command::new("perl")
    };
    cmd.arg(script);
    cmd
}

/// (Group1, Tag, Value) of a file's `g0` tags as Perl ExifTool reads them
/// (`exiftool -a -G0:1 -s -charset UTF8`).
fn read_perl(script: &Path, path: &Path, g0: &str) -> BTreeSet<(String, String, String)> {
    let out = perl(script).args(["-a", "-G0:1", "-s", "-charset", "UTF8"]).arg(path).output().expect("run perl exiftool");
    assert!(out.status.success(), "perl exiftool {}: {}", path.display(), String::from_utf8_lossy(&out.stderr));
    let mut m = BTreeSet::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Some(rest) = line.strip_prefix('[') else { continue };
        let Some((groups, rest)) = rest.split_once(']') else { continue };
        let (f0, f1) = groups.split_once(':').unwrap_or((groups, groups));
        let Some((name, value)) = rest.split_once(" : ") else { continue };
        if f0 == g0 {
            m.insert((f1.to_string(), name.trim().to_string(), value.to_string()));
        }
    }
    m
}

/// A copy of a test image with every tag of [`tags_of`] its format has
/// set to "Tag of ext", written by the built-in extensions.
fn enriched(src: &str, ext: &str, g0: &str) -> PathBuf {
    let mut et = ExifTool::new();
    let group = g0;
    for tag in tags_of(g0) {
        et.set_new_value(&format!("{group}:{tag}"), Some(&value(tag, ext)));
    }
    let out = tmp(&format!("source.{ext}"));
    let n = et.write_info(image(src), &out).unwrap();
    assert_eq!(n as usize, tags_of(g0).len(), "{src}: enriched");
    out
}

/// The names of `tags` with the value `v`.
fn names_with(tags: &BTreeSet<(String, String, String)>, name: &str, v: &str) -> bool {
    tags.iter().any(|(_, n, val)| n == name && val == v)
}

#[test]
fn copies_between_the_extension_formats_go_to_the_destination_group() {
    let script = perl_exiftool();
    if script.is_none() {
        eprintln!("Perl ExifTool not found (EXIFTOOL_DIR): the copies are not read back by it");
    }
    let sources: Vec<PathBuf> = FORMATS.iter().map(|(src, ext, g0)| enriched(src, ext, g0)).collect();
    for ((_, sext, sg0), source) in FORMATS.iter().zip(&sources) {
        let source_tags = ExifTool::new().extract_info(source).unwrap();
        for (dst, dext, dg0) in FORMATS {
            let mut et = ExifTool::new();
            assert!(et.set_new_values_from_file(source, None).unwrap() > 0, "{sext}");
            let out = tmp(&format!("copy_{sext}_to_{dext}.{dext}"));
            let n = et.write_info(image(dst), &out).unwrap_or_else(|e| panic!("{sext} -> {dext}: {e}"));
            let common: Vec<&str> = tags_of(sg0).iter().copied().filter(|t| tags_of(dg0).contains(t)).collect();
            let before = read_port(&image(dst), dg0);
            let after = read_port(&out, dg0);
            for tag in &common {
                assert!(names_with(&after, tag, &value(tag, sext)), "{sext} -> {dext}: {tag} not copied (port): {after:?}");
            }
            // What else is new is a tag the source has under that name (Year
            // from MP3 to AIFF, Date from Ogg to FLAC, the TrackNumber of a
            // Matroska track to a Vorbis comment, as ExifTool copies it to
            // an M4A), its value as the format stores it (ID3v1 keeps the
            // track number of "1/5"): no tag of another format becomes a
            // Vorbis comment or a SimpleTag (Vendor, BlockSizeMin, DocType).
            let extra: BTreeSet<String> = after.iter().filter(|t| !before.contains(t) && !common.contains(&t.1.as_str())).filter(|(_, n, _)| !source_tags.iter().any(|t| &t.name == n)).map(|(_, n, v)| format!("{n}={v}")).collect();
            assert!(extra.is_empty(), "{sext} -> {dext}: also written {extra:?}");
            let carried: BTreeSet<&String> = after.iter().filter(|t| !before.contains(t) && !common.contains(&t.1.as_str())).map(|(_, n, _)| n).collect();
            eprintln!("{sext} -> {dext}: {} copied, also {carried:?}", common.len());
            assert!(n as usize >= common.len(), "{sext} -> {dext}: counted {n}, {} copied", common.len());
            if let Some(script) = &script {
                let after = read_perl(script, &out, dg0);
                for tag in &common {
                    assert!(names_with(&after, tag, &value(tag, sext)), "{sext} -> {dext}: {tag} not read by Perl ExifTool: {after:?}");
                }
            }
        }
    }
}

/// A copy from FLAC.flac (Vorbis Title "ExifTool test") to MP3.mp3 with a
/// tag list: the count, and the output.
fn copy_to_mp3(case: &str, tags: &[&str]) -> (u32, PathBuf) {
    let mut et = ExifTool::new();
    et.set_new_values_from_file(image("FLAC.flac"), Some(tags)).unwrap();
    let out = tmp(&format!("group_{case}.mp3"));
    (et.write_info(image("MP3.mp3"), &out).unwrap(), out)
}

#[test]
fn a_group_the_tag_list_names_is_kept() {
    let titles = |p: &Path| -> BTreeSet<(String, String)> { read_port(p, "ID3").into_iter().filter(|(_, n, _)| n == "Title").map(|(g, _, v)| (g, v)).collect() };
    let expect = |v1: &str, v2: &str| -> BTreeSet<(String, String)> { [("ID3v1".to_string(), v1.to_string()), ("ID3v2_2".to_string(), v2.to_string())].into() };
    // A source group alone is the destination group too (Writer.pl: "use
    // same group name for dest"), and an MP3 has no Vorbis comments: as
    // `exiftool -tagsFromFile FLAC.flac -Vorbis:Title` would leave an MP3,
    // nothing is written. Same for all:all, which keeps every group.
    for (case, tags) in [("vorbis_title", &["Vorbis:Title"][..]), ("all_all", &["all:all"][..])] {
        let (n, out) = copy_to_mp3(case, tags);
        assert_eq!(n, 0, "{case}");
        assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(image("MP3.mp3")).unwrap(), "{case}");
    }
    // Redirected to an ID3 group, it is written there, and only there.
    let (n, out) = copy_to_mp3("to_id3v1", &["Vorbis:Title>ID3v1:Title"]);
    assert_eq!(n, 1);
    assert_eq!(titles(&out), expect("ExifTool test", "ExifTool Test"));
    let (n, out) = copy_to_mp3("to_id3", &["ID3:Title<Vorbis:Title"]);
    assert_eq!(n, 1);
    assert_eq!(titles(&out), expect("ExifTool test", "ExifTool test"));
    // A tag named without a group goes to the preferred group, ID3 here.
    let (n, out) = copy_to_mp3("no_group", &["Title"]);
    assert_eq!(n, 1);
    assert_eq!(titles(&out), expect("ExifTool test", "ExifTool test"));
    if let Some(script) = perl_exiftool() {
        let perl_titles: BTreeSet<(String, String)> = read_perl(&script, &out, "ID3").into_iter().filter(|(_, n, _)| n == "Title").map(|(g, _, v)| (g, v)).collect();
        assert_eq!(perl_titles, expect("ExifTool test", "ExifTool test"));
    }
    // A request for an explicit group is honoured exactly too.
    let mut et = ExifTool::new();
    et.set_new_value("Vorbis:Title", Some("x"));
    let out = tmp("group_set_vorbis.mp3");
    assert_eq!(et.write_info(image("MP3.mp3"), &out).unwrap(), 0);
    assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(image("MP3.mp3")).unwrap());
}

/// An extension that counts its calls and writes the input unchanged.
struct Counting(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl exiftool_port::FormatExtension for Counting {
    fn name(&self) -> &str {
        "counting"
    }
    fn file_types(&self) -> &[&str] {
        &["MP4", "M4A", "JPEG"]
    }
    fn write(&self, input: &[u8], _file_type: &str, _edits: &[exiftool_port::NewValue]) -> exiftool_port::Result<Vec<u8>> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(input.to_vec())
    }
}

/// A copy to a format ExifTool writes is ExifTool's, as before: no extension
/// is called, and the output has what `exiftool -tagsFromFile SRC -o OUT
/// DST` writes (Perl ExifTool 13.59, read back by the port).
#[test]
fn copies_to_formats_exiftool_writes_are_exiftools() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let groups = |g0: &str| matches!(g0, "QuickTime" | "XMP" | "EXIF" | "IPTC");
    let keep = |p: &Path| -> BTreeSet<(String, String, String)> {
        let mut et = ExifTool::new();
        et.options_mut().duplicates = true;
        et.extract_info(p).unwrap().into_iter().filter(|t| groups(&t.group.family0) && t.name != "XMPToolkit").map(|t| (t.group.family1, t.name, t.print_value)).collect()
    };
    for (src, dst, ext) in [("MP3.mp3", "QuickTime.m4a", "m4a"), ("Vorbis.ogg", "QuickTime.m4a", "m4a"), ("Matroska.mkv", "Writer.jpg", "jpg")] {
        let mut et = ExifTool::new();
        et.register_extension(Counting(calls.clone()));
        et.set_new_values_from_file(image(src), None).unwrap();
        let out = tmp(&format!("native_{}.{ext}", src.replace('.', "_")));
        let n = et.write_info(image(dst), &out).unwrap();
        assert!(n > 0, "{src} -> {dst}");
        assert_eq!(calls.load(Ordering::SeqCst), 0, "{src} -> {dst}");
        let Some(script) = perl_exiftool() else { continue };
        let theirs = tmp(&format!("native_{}_perl.{ext}", src.replace('.', "_")));
        let _ = std::fs::remove_file(&theirs);
        let r = perl(&script).arg("-tagsFromFile").arg(image(src)).arg("-o").arg(&theirs).arg(image(dst)).output().unwrap();
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        assert_eq!(keep(&out), keep(&theirs), "{src} -> {dst}");
    }
}
