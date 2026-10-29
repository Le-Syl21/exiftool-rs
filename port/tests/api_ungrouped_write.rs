//! Ungrouped tags (`-Artist=...`), as ExifTool 13.59's SetNewValue writes
//! them: created in the preferred family (EXIF, then IPTC, then XMP) and
//! replaced where another family already has the tag; a deletion reaches
//! every family. Each expectation below is what Perl ExifTool does to the
//! same file (checked with `exiftool -o out.jpg -Artist=New in.jpg`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use exiftool_port::{ExifTool, Options};

fn dir() -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join("ungrouped_write");
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(src: &Path, name: &str, ops: &[(&str, Option<&str>)]) -> PathBuf {
    let mut et = ExifTool::new();
    for (tag, v) in ops {
        et.set_new_value(tag, *v);
    }
    let dst = dir().join(name);
    let n = et
        .write_info(src, &dst)
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(n as usize, ops.len(), "{name}: every request is applied");
    dst
}

/// `Group1:Tag` → values, for the tags named `names`.
fn read(path: &Path, names: &[&str]) -> BTreeMap<String, Vec<String>> {
    let et = ExifTool::with_options(Options {
        duplicates: true,
        ..Options::default()
    });
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for t in et.extract_info(path).unwrap() {
        if names.contains(&t.name.as_str()) && t.group.family0 != "Composite" {
            out.entry(format!("{}:{}", t.group.family1, t.name))
                .or_default()
                .push(t.print_value.clone());
        }
    }
    out
}

fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, Vec<String>> {
    let mut m: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (k, v) in pairs {
        m.entry(k.to_string()).or_default().push(v.to_string());
    }
    m
}

/// Canon.jpg (EXIF Make, no Artist) with XMP-tiff:Artist, XMP-tiff:Make,
/// XMP-dc:Title and IPTC:By-line added (one copy per test: they run in
/// parallel).
fn base(name: &str) -> PathBuf {
    write(
        Path::new("../tests/images/Canon.jpg"),
        &format!("base_{name}.jpg"),
        &[
            ("XMP-tiff:Artist", Some("OldX")),
            ("XMP-tiff:Make", Some("OldM")),
            ("XMP-dc:Title", Some("OldT")),
            ("IPTC:By-line", Some("OldI")),
        ],
    )
}

#[test]
fn a_tag_created_in_exif_replaces_the_xmp_copy() {
    let b = base("set");
    let out = write(&b, "artist.jpg", &[("Artist", Some("New"))]);
    assert_eq!(
        read(&out, &["Artist"]),
        map(&[("IFD0:Artist", "New"), ("XMP-tiff:Artist", "New")])
    );
    let out = write(&b, "make.jpg", &[("Make", Some("New"))]);
    assert_eq!(
        read(&out, &["Make"]),
        map(&[("IFD0:Make", "New"), ("XMP-tiff:Make", "New")])
    );
}

#[test]
fn an_ungrouped_delete_reaches_every_family() {
    let b = base("del");
    let out = write(&b, "del_artist.jpg", &[("Artist", None)]);
    assert!(read(&out, &["Artist"]).is_empty());
    let out = write(&b, "del_make.jpg", &[("Make", None)]);
    assert!(read(&out, &["Make"]).is_empty());
}

#[test]
fn iptc_comes_before_xmp_and_names_are_exiftools() {
    let b = base("order");
    // Keywords: IPTC is preferred over XMP; nothing is created in XMP.
    let out = write(&b, "keywords.jpg", &[("Keywords", Some("K"))]);
    assert_eq!(
        read(&out, &["Keywords", "Subject"]),
        map(&[("IPTC:Keywords", "K")])
    );
    // Title is XMP-dc's (IPTC's tag is ObjectName).
    let out = write(&b, "title.jpg", &[("Title", Some("T"))]);
    assert_eq!(
        read(&out, &["Title", "ObjectName"]),
        map(&[("XMP-dc:Title", "T")])
    );
}

#[test]
fn an_iptc_tag_also_replaces_its_xmp_copy_and_its_trailer_copies() {
    // ExifTool.jpg: IPTC:City "Kingston", XMP-photoshop:City, and the AFCP
    // trailer's City (IPTC3). Perl writes City everywhere it can, adds it
    // to the FotoStation IPTC record (IPTC2), and creates no IPTC directory
    // in the MIE trailer, whose own MIE-Geo:City it writes instead.
    let out = write(
        Path::new("../tests/images/ExifTool.jpg"),
        "city.jpg",
        &[("City", Some("New"))],
    );
    assert_eq!(
        read(&out, &["City"]),
        map(&[
            ("IPTC:City", "New"),
            ("IPTC2:City", "New"),
            ("IPTC3:City", "New"),
            ("MIE-Geo:City", "New"),
            ("XMP-photoshop:City", "New"),
        ])
    );
}
