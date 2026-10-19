use exiftool_rs::ExifTool;
use std::path::Path;

#[test]
fn test_write_read_roundtrip() {
    let src = Path::new("tests/images/Canon.jpg");
    if !src.exists() {
        return;
    }

    let tmp = std::env::temp_dir().join("exiftool_rs_test_roundtrip.jpg");
    std::fs::copy(src, &tmp).unwrap();

    let mut et = ExifTool::new();
    et.set_new_value("Artist", Some("Test Author"));
    et.write_info(tmp.to_str().unwrap(), tmp.to_str().unwrap())
        .unwrap();

    let et2 = ExifTool::new();
    let info = et2.image_info(&tmp).unwrap();
    assert_eq!(info.get("Artist").map(|s| s.as_str()), Some("Test Author"));

    std::fs::remove_file(&tmp).ok();
}

/// A tag ExifTool does not define is skipped with a warning and the rest is
/// written, as ExifTool 13.59 does with `-Foo=bar -Artist=me` on a JPEG
/// ("Warning: Tag 'Foo' is not defined", "1 image files created"). With
/// nothing else to do, a new destination is an unchanged copy ("1 image
/// files copied"), and in place the file is left alone ("Nothing to do.").
#[test]
fn undefined_tag_is_skipped_with_a_warning() {
    let src = Path::new("tests/images/Writer.jpg");
    let dir = std::env::temp_dir().join(format!("exiftool_rs_undefined_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let mut et = ExifTool::new();
    et.set_new_value("Foo", Some("bar"));
    et.set_new_value("Artist", Some("me"));
    let dst = dir.join("both.jpg");
    assert_eq!(et.write_info(src, &dst).unwrap(), 1);
    let warnings = et.write_warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("'Foo'"), "{warnings:?}");
    let info = ExifTool::new().image_info(&dst).unwrap();
    assert_eq!(info.get("Artist").map(String::as_str), Some("me"));

    // The next call starts with no warnings of its own.
    let mut ok = ExifTool::new();
    ok.set_new_value("Artist", Some("me"));
    assert_eq!(ok.write_info(src, dir.join("ok.jpg")).unwrap(), 1);
    assert!(ok.write_warnings().is_empty());

    let mut et = ExifTool::new();
    et.set_new_value("Foo", Some("bar"));
    let dst = dir.join("copy.jpg");
    assert_eq!(et.write_info(src, &dst).unwrap(), 0);
    assert_eq!(std::fs::read(&dst).unwrap(), std::fs::read(src).unwrap());
    assert_eq!(et.write_warnings().len(), 1);

    let in_place = dir.join("in_place.jpg");
    std::fs::copy(src, &in_place).unwrap();
    assert!(et.write_info(&in_place, &in_place).is_err());
    assert_eq!(
        std::fs::read(&in_place).unwrap(),
        std::fs::read(src).unwrap()
    );

    std::fs::remove_dir_all(&dir).ok();
}
