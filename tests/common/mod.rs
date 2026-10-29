//! What the integration tests share: where Perl ExifTool's checkout is.
//!
//! The tests that run ExifTool's own scripts, parse its sources or read a
//! written file back with it need the source tree of the release the port
//! follows (ExifTool 13.60): `$EXIFTOOL_DIR`, or `../exiftool` next to this
//! repository. Without it they are skipped with a message, so `cargo test`
//! works on a machine that has no checkout; the CI provides one.
#![allow(dead_code)]

use std::path::PathBuf;

/// The ExifTool checkout: `$EXIFTOOL_DIR`, else `../exiftool` next to the
/// repository, when it holds the `exiftool` script and its `lib`. `None`
/// (and a line on stderr saying what is skipped) when it is absent.
pub fn exiftool_dir(what: &str) -> Option<PathBuf> {
    let dir = match std::env::var_os("EXIFTOOL_DIR") {
        Some(d) if !d.is_empty() => PathBuf::from(d),
        _ => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../exiftool"),
    };
    if dir.join("exiftool").is_file() && dir.join("lib/Image/ExifTool.pm").is_file() {
        Some(std::fs::canonicalize(&dir).unwrap_or(dir))
    } else {
        eprintln!("skipped {what}: no ExifTool checkout at {} (set EXIFTOOL_DIR)", dir.display());
        None
    }
}

/// Whether `perl` can be run; a line on stderr saying what is skipped when
/// it cannot.
pub fn have_perl(what: &str) -> bool {
    let ok = std::process::Command::new("perl").arg("-e").arg("1").output().map(|o| o.status.success()).unwrap_or(false);
    if !ok {
        eprintln!("skipped {what}: perl not found");
    }
    ok
}
