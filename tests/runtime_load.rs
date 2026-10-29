//! Module data loads back as perl had it.
use exiftool_rs::interp::{Scope, Want};

fn eval(it: &mut exiftool_rs::interp::Interp, code: &str) -> String {
    let scope = Scope::new(None);
    match it.eval_string(code.as_bytes(), "main", &scope, Want::Scalar) {
        Ok(mut v) => v.pop().unwrap_or_default().to_text(),
        Err(e) => format!("ERROR: {}", e.to_text()),
    }
}

#[test]
fn bmp_tables_load() {
    let mut it = exiftool_rs::runtime::new_interp();
    assert_eq!(eval(&mut it, "require Image::ExifTool::BMP; 1"), "1");
    assert_eq!(eval(&mut it, "$Image::ExifTool::BMP::Main{0}{Name}"), "BMPVersion");
    assert_eq!(eval(&mut it, "$Image::ExifTool::BMP::Main{0}{PrintConv}{40}"), "Windows V3");
    assert_eq!(eval(&mut it, "ref $Image::ExifTool::BMP::Main{PROCESS_PROC}"), "CODE");
    assert_eq!(eval(&mut it, "$Image::ExifTool::fileTypeLookup{BMP}[0]"), "BMP");
    assert_eq!(eval(&mut it, "$Image::ExifTool::magicNumber{BMP}"), "BM");
    // A file lexical of ExifTool.pm.
    assert_eq!(eval(&mut it, "scalar keys %{'Image::ExifTool::<my>::moduleName'}") != "0", true);
}
