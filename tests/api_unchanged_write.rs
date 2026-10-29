//! A write that changes nothing counts nothing, and in place leaves the
//! file as it was: same bytes, same inode, same modification time. That is
//! Perl ExifTool's "1 image files unchanged" (`WriteInfo` returns 2, for a
//! deletion of what is not there), and also the value already there, which
//! Perl reports as "1 image files updated" while its new file has the same
//! bytes. The extensions (MP3, FLAC, MKV) follow the same rule.

use exiftool_rs::ExifTool;

mod common;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

fn dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("unchanged_write").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A copy of a test image, dated in 2001.
fn copy(d: &Path, name: &str) -> PathBuf {
    let file = d.join(name);
    std::fs::copy(Path::new("tests/images").join(name), &file).unwrap();
    let f = std::fs::File::options().write(true).open(&file).unwrap();
    f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000)).unwrap();
    file
}

/// What says a file was not touched: its bytes, inode and modification time.
fn state(p: &Path) -> (Vec<u8>, u64, SystemTime) {
    let m = std::fs::metadata(p).unwrap();
    (std::fs::read(p).unwrap(), m.ino(), m.modified().unwrap())
}

fn write(file: &Path, ops: &[(&str, Option<&str>)]) -> u32 {
    let mut et = ExifTool::new();
    for (tag, v) in ops {
        et.set_new_value(tag, *v);
    }
    et.write_info(file, file).unwrap_or_else(|e| panic!("{} {ops:?}: {e}", file.display()))
}

/// Perl ExifTool's summary for the same write in place, on a copy ("1
/// image files updated", "1 image files unchanged"), and the bytes of the
/// copy after it. `None` when Perl ExifTool is not at `$EXIFTOOL_DIR`
/// (default `../exiftool`, next to the repository: tests/common/mod.rs).
fn perl_summary(file: &Path, args: &[&str]) -> Option<(String, Vec<u8>)> {
    let script = common::exiftool_dir("the Perl side of the comparison")?.join("exiftool");
    let copy = file.with_extension(format!("perl.{}", file.extension().unwrap().to_str().unwrap()));
    std::fs::copy(file, &copy).unwrap();
    let out = std::process::Command::new("prlimit")
        .args(["--as=4000000000", "--", "timeout", "60", "perl"])
        .arg(&script)
        .arg("-overwrite_original")
        .args(args)
        .arg(&copy)
        .output()
        .ok()?;
    let bytes = std::fs::read(&copy).unwrap();
    let _ = std::fs::remove_file(&copy);
    Some((String::from_utf8_lossy(&out.stdout).trim().to_string(), bytes))
}

#[test]
fn writing_the_value_already_there_counts_nothing() {
    let d = dir("jpeg");
    let file = copy(&d, "Writer.jpg");
    assert_eq!(write(&file, &[("Artist", Some("y"))]), 1);
    let f = std::fs::File::options().write(true).open(&file).unwrap();
    f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000)).unwrap();
    let before = state(&file);
    // the same value again: Perl says "updated", and renames a new file
    // with the same bytes over the old one
    if let Some((s, bytes)) = perl_summary(&file, &["-Artist=y"]) {
        assert_eq!(s, "1 image files updated");
        assert!(bytes == before.0, "Perl changed the bytes");
    }
    assert_eq!(write(&file, &[("Artist", Some("y"))]), 0);
    assert!(state(&file) == before, "the file was touched");
    // the same with its group, and twice in one call
    assert_eq!(write(&file, &[("EXIF:Artist", Some("y")), ("Artist", Some("y"))]), 0);
    assert!(state(&file) == before, "the file was touched");
    // one that changes and one that does not: one counted
    if let Some((s, bytes)) = perl_summary(&file, &["-Artist=y", "-Copyright=z"]) {
        assert_eq!(s, "1 image files updated");
        assert!(bytes != before.0);
    }
    assert_eq!(write(&file, &[("Artist", Some("y")), ("Copyright", Some("z"))]), 1);
    assert_ne!(state(&file).0, before.0);
    // to a new destination: the unchanged copy, nothing counted
    let mut et = ExifTool::new();
    et.set_new_value("Artist", Some("y"));
    let out = d.join("out.jpg");
    assert_eq!(et.write_info(&file, &out).unwrap(), 0);
    assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(&file).unwrap());
}

#[test]
fn deleting_what_is_not_there_counts_nothing() {
    let d = dir("delete");
    let file = copy(&d, "Writer.jpg");
    let before = state(&file);
    // Perl's WriteInfo returns 2 ("0 image files updated", "1 image files
    // unchanged")
    if let Some((s, bytes)) = perl_summary(&file, &["-Lens="]) {
        assert!(s.ends_with("1 image files unchanged"), "{s}");
        assert!(bytes == before.0);
    }
    assert_eq!(write(&file, &[("Lens", None)]), 0);
    assert!(state(&file) == before, "the file was touched");
    // nothing that can be written at all, in place: "Nothing to do."
    let mut et = ExifTool::new();
    et.set_new_value("NoSuchTag", Some("x"));
    assert!(et.write_info(&file, &file).is_err());
    assert!(state(&file) == before, "the file was touched");
}

#[test]
fn extensions_count_only_what_changed() {
    let d = dir("extensions");
    for (name, group, other) in [("MP3.mp3", "ID3", "Album"), ("FLAC.flac", "Vorbis", "Album"), ("Matroska.mkv", "Matroska", "Artist")] {
        let file = copy(&d, name);
        let title = |v: &str| (format!("{group}:Title"), Some(v.to_string()));
        let ops = |list: &[(String, Option<String>)]| -> u32 {
            let mut et = ExifTool::new();
            for (t, v) in list {
                et.set_new_value(t, v.as_deref());
            }
            et.write_info(&file, &file).unwrap_or_else(|e| panic!("{name}: {e}"))
        };
        assert_eq!(ops(&[title("Same")]), 1, "{name}");
        let f = std::fs::File::options().write(true).open(&file).unwrap();
        f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000)).unwrap();
        let before = state(&file);
        assert_eq!(ops(&[title("Same")]), 0, "{name}");
        assert!(state(&file) == before, "{name}: the file was touched");
        assert_eq!(ops(&[(format!("{group}:Conductor"), None), (format!("{group}:Publisher"), None)]), 0, "{name}");
        assert!(state(&file) == before, "{name}: the file was touched");
        assert_eq!(ops(&[title("Same"), (format!("{group}:{other}"), Some("New".to_string()))]), 1, "{name}");
        assert_ne!(state(&file).0, before.0, "{name}");
    }
}
