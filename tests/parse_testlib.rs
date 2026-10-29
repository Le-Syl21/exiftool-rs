//! ExifTool's test harness parses.
mod common;

#[test]
fn testlib_and_bmp_t_parse() {
    let Some(root) = common::exiftool_dir("parsing t/TestLib.pm") else { return };
    for f in ["t/TestLib.pm", "t/BMP.t"] {
        let path = root.join(f);
        let src = std::fs::read(&path).unwrap();
        if let Err(e) = exiftool_rs::interp::parse::parse_program(&src, "main") {
            panic!("{f}: {e}");
        }
    }
}
