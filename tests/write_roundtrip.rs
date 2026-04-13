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
