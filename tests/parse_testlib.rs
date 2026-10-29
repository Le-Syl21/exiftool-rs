//! ExifTool's test harness parses.
#[test]
fn testlib_and_bmp_t_parse() {
    for f in ["t/TestLib.pm", "t/BMP.t"] {
        let path = format!("{}/../../exiftool/{f}", env!("CARGO_MANIFEST_DIR"));
        let src = std::fs::read(&path).unwrap();
        if let Err(e) = exiftool_port::interp::parse::parse_program(&src, "main") {
            panic!("{f}: {e}");
        }
    }
}
