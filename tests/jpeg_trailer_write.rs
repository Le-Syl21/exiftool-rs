//! Writes that reach the trailers of a JPEG, compared with ExifTool 13.59.
//!
//! `tests/images/ExifTool.jpg` chains seven trailers after its EOI (from the
//! end: Vivo, Samsung, MIE, PhotoMechanic, CanonVRD, FotoStation, AFCP).
//! ExifTool walks them from the end of the file backwards and edits the
//! IPTC of the FotoStation and AFCP trailers, and the EXIF, IPTC and XMP
//! directories of the MIE trailer, creating them there when a request
//! writes tags of their family.
//!
//! The baselines in `tests/expected_jpeg_trailers/` are `Group1:Tag<TAB>value`
//! lines, sorted and deduplicated, generated with the pinned ExifTool
//! (inside the parity Docker image):
//!
//! ```sh
//! perl exiftool -o out.jpg <edit> tests/images/ExifTool.jpg
//! perl exiftool -a -s -G1 -EXIF:all -IPTC:all -MIE:all -FotoStation:all \
//!     -PhotoMechanic:all -XMP:all -File:ExifByteOrder out.jpg
//! ```
//!
//! with the offsets that depend on the block layout left out, and
//! `XMPToolkit`, which names the writer.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use exiftool_rs::{ExifTool, Options};

const LEFT_OUT: &[&str] = &[
    "ThumbnailOffset",
    "PreviewImageStart",
    "OriginalDecisionDataOffset",
    "MPImageStart",
    "XMPToolkit",
];

fn write(case: &str, tag: &str, value: Option<&str>) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join("jpeg_trailer_write");
    std::fs::create_dir_all(&d).unwrap();
    let mut et = ExifTool::new();
    et.set_new_value(tag, value);
    let dst = d.join(format!("{case}.jpg"));
    let n = et
        .write_info("tests/images/ExifTool.jpg", &dst)
        .unwrap_or_else(|e| panic!("{case}: write failed: {e}"));
    assert_eq!(n, 1, "{case}: the request is applied");
    dst
}

fn snapshot(path: &Path) -> String {
    let et = ExifTool::with_options(Options {
        duplicates: true,
        ..Options::default()
    });
    let tags = et.extract_info(path).unwrap();
    let lines: BTreeSet<String> = tags
        .iter()
        .filter(|t| {
            matches!(
                t.group.family0.as_str(),
                "EXIF" | "IPTC" | "MIE" | "FotoStation" | "PhotoMechanic" | "XMP"
            ) || (t.group.family0 == "File" && t.name == "ExifByteOrder")
        })
        .filter(|t| !LEFT_OUT.contains(&t.name.as_str()))
        .map(|t| format!("{}:{}\t{}", t.group.family1, t.name, t.print_value))
        .collect();
    lines.into_iter().map(|l| l + "\n").collect()
}

fn check(case: &str, tag: &str, value: Option<&str>) {
    let out = write(case, tag, value);
    let expected: String =
        std::fs::read_to_string(format!("tests/expected_jpeg_trailers/{case}.txt"))
            .unwrap()
            .lines()
            .filter(|l| !LEFT_OUT.iter().any(|n| l.contains(&format!(":{n}\t"))))
            .map(|l| format!("{l}\n"))
            .collect();
    let got = snapshot(&out);
    if got != expected {
        let e: BTreeSet<&str> = expected.lines().collect();
        let g: BTreeSet<&str> = got.lines().collect();
        let missing: Vec<&&str> = e.difference(&g).collect();
        let extra: Vec<&&str> = g.difference(&e).collect();
        panic!("{case}: differs from ExifTool\n  missing: {missing:#?}\n  extra: {extra:#?}");
    }
}

#[test]
fn iptc_reaches_every_trailer_and_is_created_in_mie() {
    // APP13, FotoStation (not the last trailer) and AFCP get the tag; MIE
    // gets a new IPTC directory with it (IPTC2, as ExifTool numbers it).
    check("set_iptc", "IPTC:By-line", Some("Set by write parity"));
}

#[test]
fn iptc_all_empties_the_trailers_too() {
    // Refused before: the FotoStation IPTC was out of reach.
    check("del_iptc", "IPTC:all", None);
}

#[test]
fn exif_is_created_in_mie_with_the_jfif_resolution() {
    // A new big-endian IFD0 in MIE-Meta, with the JFIF's 144 dpi although
    // the JFIF segment comes after the APP1 EXIF.
    check("set_exif", "EXIF:Artist", Some("Set by write parity"));
}

#[test]
fn xmp_is_created_in_mie() {
    check("set_xmp", "XMP:Description", Some("Set by write parity"));
}

#[test]
fn the_samsung_audio_offsets_follow_the_trailer() {
    // Samsung.pm rewrites the absolute offsets of the embedded audio in the
    // QDIO block for where the trailer ends up.
    let out = std::fs::read(write("samsung", "IPTC:By-line", Some("Moved"))).unwrap();
    let q = out
        .windows(8)
        .position(|w| w == b"QDIO\x65\0\0\0")
        .expect("QDIO block");
    let at = |p: usize| u32::from_le_bytes(out[p..p + 4].try_into().unwrap()) as usize;
    let (start, end) = (at(q + 12), at(q + 16));
    assert_eq!(&out[start..end], b"<dummy wav file>");
}
