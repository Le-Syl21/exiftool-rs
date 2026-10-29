//! Every code string of ExifTool's tables must parse.

fn records() -> Vec<(String, Vec<u8>)> {
    let data = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/code_strings.txt")).unwrap();
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let nl = data[i..].iter().position(|&b| b == b'\n').unwrap() + i;
        let head = std::str::from_utf8(&data[i..nl]).unwrap();
        let (kind, len) = head.rsplit_once(' ').unwrap();
        let len: usize = len.parse().unwrap();
        out.push((kind.to_string(), data[nl + 1..nl + 1 + len].to_vec()));
        i = nl + 1 + len + 1;
    }
    out
}

#[test]
fn all_table_strings_parse() {
    let recs = records();
    let mut failures = Vec::new();
    for (kind, code) in &recs {
        if let Err(e) = exiftool_port::interp::parse::parse_program(code, "Image::ExifTool") {
            if perl_rejects(code) {
                continue; // ExifTool's own string does not compile either
            }
            failures.push(format!("{kind}: {e}\n    {}", String::from_utf8_lossy(code).replace('\n', "\n    ")));
        }
    }
    let shown: Vec<_> = failures.iter().take(40).cloned().collect();
    assert!(failures.is_empty(), "{} of {} strings fail to parse:\n{}", failures.len(), recs.len(), shown.join("\n"));
}

/// Whether perl itself fails to compile the string.
fn perl_rejects(code: &[u8]) -> bool {
    let mut path = std::env::temp_dir();
    path.push(format!("exiftool_port_code_{}.pl", std::process::id()));
    let mut src = b"no strict; no warnings; sub { ".to_vec();
    src.extend_from_slice(code);
    src.extend_from_slice(b"\n}\n");
    std::fs::write(&path, &src).unwrap();
    let out = std::process::Command::new("perl").arg("-c").arg(&path).output();
    let _ = std::fs::remove_file(&path);
    matches!(out, Ok(o) if !o.status.success())
}
