//! Every Perl file of ExifTool parses (the interpreter runs the modules not
//! ported yet).
#[test]
fn exiftool_sources_parse() {
    let root = format!("{}/../../exiftool", env!("CARGO_MANIFEST_DIR"));
    let mut files = vec![format!("{root}/lib/Image/ExifTool.pm"), format!("{root}/lib/File/RandomAccess.pm"), format!("{root}/exiftool")];
    for dir in ["lib/Image/ExifTool", "lib/Image/ExifTool/Lang", "lib/Image/ExifTool/Charset"] {
        let mut v: Vec<_> = std::fs::read_dir(format!("{root}/{dir}")).unwrap().filter_map(|e| e.ok()).map(|e| e.path().to_string_lossy().into_owned()).filter(|p| p.ends_with(".pm") || p.ends_with(".pl")).collect();
        v.sort();
        files.extend(v);
    }
    for t in std::fs::read_dir(format!("{root}/t")).unwrap().filter_map(|e| e.ok()) {
        let p = t.path().to_string_lossy().into_owned();
        if p.ends_with(".t") || p.ends_with(".pm") {
            files.push(p);
        }
    }
    let mut failures = Vec::new();
    for f in &files {
        let src = std::fs::read(f).unwrap();
        if let Err(e) = exiftool_port::interp::parse::parse_program(&src, "main") {
            failures.push(format!("{}: {e}", f.trim_start_matches(&root)));
        }
    }
    assert!(failures.is_empty(), "{} of {} files fail to parse:\n{}", failures.len(), files.len(), failures.join("\n"));
}
