//! Feed arbitrary bytes to `extract_info_from_bytes`: it may fail, it must
//! never panic, hang or overflow the stack.
//!
//! The file type comes from the content first and the extension second, so
//! the first input byte picks the extension (from `extensions.txt`, the ones
//! the test corpus covers) and the rest is the file. That lets the fuzzer
//! reach readers that are only chosen by name.

#![no_main]

use exiftool_rs::ExifTool;
use libfuzzer_sys::fuzz_target;
use std::path::Path;

const EXTENSIONS: &str = include_str!("../extensions.txt");

fuzz_target!(|data: &[u8]| {
    let Some((&pick, body)) = data.split_first() else {
        return;
    };
    let extensions: Vec<&str> = EXTENSIONS.lines().collect();
    let ext = extensions[pick as usize % extensions.len()];
    let _ = ExifTool::new().extract_info_from_bytes(body, Path::new(&format!("fuzz.{ext}")));
});
