//! Scalability guard for the memory-mapped reader (issue #5).
//!
//! Builds — at run time, in the OS temp dir — a multi-gigabyte MP4 whose `mdat`
//! payload is a *sparse hole* (instant to create, ~0 bytes on disk) with the real
//! `moov` from `Android.mp4` placed at the very end, exactly like Android camera
//! output. We then parse it and assert the device metadata is still extracted.
//!
//! Why this is a robust regression test and not a flaky benchmark: with the mmap
//! reader we skip the `mdat` by offset and only fault in the header + trailing
//! `moov` pages, so it completes instantly. If the reader ever regresses to
//! reading the whole file into memory (the original #5 bug), it would try to
//! allocate several GB and blow up — a hard failure, not a timing guess.
//!
//! Unix-only: sparse-file and large-file semantics are reliable on ext4/APFS but
//! finicky on Windows. The Linux + macOS CI legs still cover it.

#![cfg(unix)]

use exiftool_rs::ExifTool;
use std::io::{Seek, SeekFrom, Write};
use std::time::{Duration, Instant};

/// Find a top-level atom by 4CC, returning `(offset, size)`.
fn find_atom(data: &[u8], want: &[u8; 4]) -> Option<(usize, u64)> {
    let mut pos = 0usize;
    while pos + 8 <= data.len() {
        let size32 = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap()) as u64;
        let typ = &data[pos + 4..pos + 8];
        let size = match size32 {
            1 => u64::from_be_bytes(data[pos + 8..pos + 16].try_into().unwrap()),
            0 => (data.len() - pos) as u64,
            n => n,
        };
        if typ == want {
            return Some((pos, size));
        }
        if size == 0 {
            break;
        }
        pos += size as usize;
    }
    None
}

#[test]
fn parses_metadata_from_a_multi_gb_file_without_reading_it_whole() {
    let src = std::path::Path::new("tests/images/Android.mp4");
    if !src.exists() {
        return;
    }
    let data = std::fs::read(src).unwrap();
    let (mdat_off, _) = find_atom(&data, b"mdat").expect("mdat atom");
    let (moov_off, _) = find_atom(&data, b"moov").expect("moov atom");

    // 4 GiB sparse hole: far past any 32-bit boundary and large enough that a
    // regression to a full read would OOM rather than merely slow down.
    const HOLE: u64 = 4 * 1024 * 1024 * 1024;

    let path = std::env::temp_dir().join("exiftool_rs_large_sparse.mp4");
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&data[..mdat_off]).unwrap(); // ftyp + free + free
        f.write_all(&1u32.to_be_bytes()).unwrap(); // mdat: 64-bit extended size
        f.write_all(b"mdat").unwrap();
        f.write_all(&(16u64 + HOLE).to_be_bytes()).unwrap();
        f.seek(SeekFrom::Current(HOLE as i64)).unwrap(); // sparse hole
        f.write_all(&data[moov_off..]).unwrap(); // real moov at the very end
        f.flush().unwrap();
    }
    assert!(
        std::fs::metadata(&path).unwrap().len() > HOLE,
        "file should be multi-GB"
    );

    let start = Instant::now();
    let tags = ExifTool::new()
        .extract_info(&path)
        .expect("parse large file");
    let elapsed = start.elapsed();

    let make = tags
        .iter()
        .find(|t| t.name == "AndroidMake")
        .map(|t| t.print_value.as_str());
    assert_eq!(
        make,
        Some("Xiaomi"),
        "device metadata must survive at 4 GB+"
    );

    // Generous secondary guard: skipping the hole is sub-second; a full read of
    // 4 GB never is.
    assert!(
        elapsed < Duration::from_secs(10),
        "parsing a sparse 4 GB file took {elapsed:?} — reader is not skipping mdat by offset"
    );

    std::fs::remove_file(&path).ok();
}
