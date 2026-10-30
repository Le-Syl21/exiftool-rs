//! Write arbitrary files through the facade: queue a few edits (a string
//! tag, a date, a group deleted), `write_info` into a new file, read it
//! back. Any step may fail; none may panic, hang, overflow the stack or
//! allocate without bound.
//!
//! The first input byte picks the extension as in `extract` (its high bits
//! pick which group is deleted), the rest is the file.
//!
//! The writer and the reader are each one ExifTool kept from input to
//! input (the writer's queue cleared each time), so their interpreters are
//! reused as in a program editing many files.

#![no_main]

use exiftool_rs::ExifTool;
use libfuzzer_sys::fuzz_target;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

const EXTENSIONS: &str = include_str!("../extensions.txt");
const DELETED: [&str; 4] = ["IPTC:all", "XMP:all", "MakerNotes:all", "IFD1:all"];

static WRITER: Mutex<Option<ExifTool>> = Mutex::new(None);
static READER: OnceLock<ExifTool> = OnceLock::new();

fuzz_target!(|data: &[u8]| {
    let Some((&pick, body)) = data.split_first() else {
        return;
    };
    let extensions: Vec<&str> = EXTENSIONS.lines().collect();
    let ext = extensions[pick as usize % extensions.len()];
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tmp");
    let _ = std::fs::create_dir_all(&dir);
    let pid = std::process::id();
    let src = dir.join(format!("write-{pid}.{ext}"));
    let dst = dir.join(format!("write-{pid}-out.{ext}"));
    let _ = std::fs::remove_file(&dst);
    if std::fs::write(&src, body).is_err() {
        return;
    }
    let mut writer = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    let et = writer.get_or_insert_with(ExifTool::new);
    et.clear_new_values();
    et.set_new_value("Artist", Some("Fuzz \u{e9}t\u{e9}"));
    et.set_new_value("DateTimeOriginal", Some("2024:02:29 12:34:56+01:00"));
    et.set_new_value("XMP-dc:Title", Some("fuzz"));
    et.set_new_value(DELETED[(pick as usize / extensions.len()) % DELETED.len()], None);
    if et.write_info(&src, &dst).is_ok() {
        let _ = READER.get_or_init(ExifTool::new).extract_info(&dst);
    }
    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&dst);
});
