//! Read arbitrary bytes through the facade: it may fail, it must never
//! panic, hang, overflow the stack or allocate without bound.
//!
//! As the root crate's target: the first input byte picks the extension
//! (from `extensions.txt`) and the rest is the file. The facade's
//! `extract_info_from_bytes` identifies data by its content only, so the
//! file is written under `tmp/` with that extension and read by name,
//! which reaches the readers ExifTool chooses by extension. The first byte
//! also picks the options: `pick / extensions` even reads with the default
//! options (`extract_info`), odd with `print_conv = false` (`image_info`).

#![no_main]

use exiftool_port::{ExifTool, Options};
use libfuzzer_sys::fuzz_target;
use std::path::PathBuf;

const EXTENSIONS: &str = include_str!("../extensions.txt");

fn temp_path(ext: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tmp");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("extract-{}.{ext}", std::process::id()))
}

fuzz_target!(|data: &[u8]| {
    let Some((&pick, body)) = data.split_first() else {
        return;
    };
    let extensions: Vec<&str> = EXTENSIONS.lines().collect();
    let ext = extensions[pick as usize % extensions.len()];
    let path = temp_path(ext);
    if std::fs::write(&path, body).is_err() {
        return;
    }
    if (pick as usize / extensions.len()) % 2 == 0 {
        let _ = ExifTool::new().extract_info(&path);
    } else {
        let _ = ExifTool::with_options(Options { print_conv: false, ..Options::default() }).image_info(&path);
    }
    let _ = std::fs::remove_file(&path);
});
