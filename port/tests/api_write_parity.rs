//! Write-parity harness — the write-side counterpart to `regression.rs`.
//!
//! The read side compares our output to a Perl-derived baseline; the write
//! side had almost nothing (one self-round-trip on a single EXIF tag), which
//! is how the destructive IPTC-write bug (#7 — writing one IPTC tag dropped
//! all the others) slipped through: no test wrote to a metadata-rich file and
//! checked the rest survived, and no write was ever compared to ExifTool.
//!
//! For each case we apply an edit, read the result back with our own reader,
//! and snapshot it as `CurrentIPTCDigest` + the IPTC, Photoshop and XMP tags
//! (sorted) and any warning. The digest is an MD5 of the raw IPTC block, so a
//! matching digest proves the bytes we wrote are identical to what ExifTool
//! wrote (dataset order and mandatory version included); the tag listing
//! gives a readable diff and catches a dropped Photoshop resource or XMP
//! property, or one written to the wrong namespace.
//!
//! Baselines live in `tests/expected_write/` and are generated from Perl
//! ExifTool. Normal runs need no Perl — they compare against those snapshots.
//! Regenerate after an intentional change (needs `../exiftool`):
//!
//! ```sh
//! WRITE_PARITY_REGEN=1 cargo test --test write_parity
//! ```

use std::path::Path;
use std::process::Command;

use exiftool_port::ExifTool;

/// One edit applied to `image`, identified by `id` (the baseline file name).
/// `ops` is a list of `(group-qualified tag, value)`; `None` deletes the tag.
struct Case {
    id: &'static str,
    image: &'static str,
    ops: &'static [(&'static str, Option<&'static str>)],
}

const CASES: &[Case] = &[
    // Update an existing tag with an accented value (the #6 encoding path).
    Case {
        id: "set_byline_accent",
        image: "IPTC.jpg",
        ops: &[("IPTC:By-line", Some("Martín"))],
    },
    // Update a different existing tag (ASCII).
    Case {
        id: "set_headline",
        image: "IPTC.jpg",
        ops: &[("IPTC:Headline", Some("New headline"))],
    },
    // Delete an existing tag — the rest must survive.
    Case {
        id: "delete_byline",
        image: "IPTC.jpg",
        ops: &[("IPTC:By-line", None)],
    },
    // Add a tag the file doesn't have yet.
    Case {
        id: "add_city",
        image: "IPTC.jpg",
        ops: &[("IPTC:City", Some("Paris"))],
    },
    // Several ops at once: set + add + delete.
    Case {
        id: "multi_set_add_delete",
        image: "IPTC.jpg",
        ops: &[
            ("IPTC:By-line", Some("Ada Lovelace")),
            ("IPTC:City", Some("London")),
            ("IPTC:Headline", None),
        ],
    },
    // A file without IPTC: a new APP13 block, with the mandatory
    // ApplicationRecordVersion ExifTool adds after the tag.
    Case {
        id: "create_iptc_block",
        image: "Canon.jpg",
        ops: &[("IPTC:City", Some("Paris"))],
    },
    // IPTC:all goes, the other Photoshop resources (and IPTCDigest) stay.
    Case {
        id: "delete_all_iptc",
        image: "IPTC.jpg",
        ops: &[("IPTC:all", None)],
    },
    // A list tag: every old value is replaced.
    Case {
        id: "replace_keywords",
        image: "PhotoMechanic.jpg",
        ops: &[("IPTC:Keywords", Some("one"))],
    },
    // XMP:Description goes to XMP-dc (XMP-xmp:Description is `Avoid`), in a
    // UTF-16 packet whose other properties stay.
    Case {
        id: "xmp_description_utf16",
        image: "XMP.jpg",
        ops: &[("XMP:Description", Some("New description"))],
    },
    // The extended XMP is read with the standard packet; XMP:Title creates
    // dc:title and also replaces the pdf:Title the extended part holds.
    Case {
        id: "xmp_title_extended",
        image: "ExtendedXMP.jpg",
        ops: &[("XMP:Title", Some("New title"))],
    },
    // XMP:all removes the extended XMP too.
    Case {
        id: "xmp_delete_all_extended",
        image: "ExtendedXMP.jpg",
        ops: &[("XMP:all", None)],
    },
];

/// `CurrentIPTCDigest`, then the IPTC, Photoshop and XMP tags and the
/// warnings, sorted, as our reader sees the file.
fn snapshot(path: &Path) -> String {
    // Every instance (`-a`): XMP-pdf:Title is otherwise hidden by XMP-dc:Title.
    let mut et = ExifTool::new();
    et.options_mut().duplicates = true;
    let tags = et.extract_info(path).expect("read written file");
    let digest = tags
        .iter()
        .find(|t| t.name == "CurrentIPTCDigest")
        .map(|t| t.print_value.as_str())
        .unwrap_or("<none>");
    let mut lines: Vec<String> = tags
        .iter()
        .filter(|t| {
            matches!(t.group.family0.as_str(), "IPTC" | "Photoshop" | "XMP")
                && t.name != "XMPToolkit"
                || t.name == "Warning"
        })
        .map(|t| format!("{}:{}: {}\n", t.group.family1, t.name, t.print_value))
        .collect();
    lines.sort();
    format!("DIGEST {digest}\n{}", lines.concat())
}

/// Apply the edits with exiftool-rs (in place).
fn write_ours(src: &Path, dst: &Path, ops: &[(&str, Option<&str>)]) {
    std::fs::copy(src, dst).expect("copy source");
    let mut et = ExifTool::new();
    for (tag, val) in ops {
        et.set_new_value(tag, *val);
    }
    let p = dst.to_str().unwrap();
    et.write_info(p, p).expect("exiftool-rs write");
}

/// Apply the same edits with Perl ExifTool (baseline generation only).
fn write_perl(src: &Path, dst: &Path, ops: &[(&str, Option<&str>)]) {
    std::fs::copy(src, dst).expect("copy source");
    let mut cmd = Command::new("perl");
    cmd.arg("../exiftool/exiftool")
        .arg("-charset")
        .arg("UTF8")
        .arg("-overwrite_original");
    for (tag, val) in ops {
        match val {
            Some(v) => cmd.arg(format!("-{tag}={v}")),
            None => cmd.arg(format!("-{tag}=")),
        };
    }
    let out = cmd.arg(dst).output().expect("run perl exiftool");
    assert!(
        out.status.success(),
        "perl exiftool failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn iptc_write_parity() {
    let regen = std::env::var("WRITE_PARITY_REGEN").is_ok();
    let img_dir = Path::new("../tests/images");
    let exp_dir = Path::new("../tests/expected_write");

    // The corpus is committed; skip cleanly if a checkout lacks it.
    if !img_dir.join("IPTC.jpg").exists() {
        eprintln!("../tests/images/IPTC.jpg missing — skipping write-parity");
        return;
    }

    let mut mismatches = Vec::new();
    for case in CASES {
        let src = img_dir.join(case.image);
        let tmp = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("exiftool_port_wp_{}.jpg", case.id));
        let exp = exp_dir.join(format!("{}.snap", case.id));

        if regen {
            write_perl(&src, &tmp, case.ops);
            let snap = snapshot(&tmp);
            std::fs::create_dir_all(exp_dir).unwrap();
            std::fs::write(&exp, &snap).unwrap();
            eprintln!("regenerated {}", exp.display());
        } else {
            write_ours(&src, &tmp, case.ops);
            // Normalise line endings: git may check the baseline out with CRLF
            // on Windows while our snapshot uses LF, which would otherwise fail
            // the byte comparison for a purely cosmetic reason.
            let got = snapshot(&tmp).replace("\r\n", "\n");
            let want = std::fs::read_to_string(&exp)
                .unwrap_or_else(|_| {
                    panic!(
                        "missing baseline {} — run `WRITE_PARITY_REGEN=1 cargo test --test write_parity`",
                        exp.display()
                    )
                })
                .replace("\r\n", "\n");
            if got != want {
                mismatches.push(format!(
                    "case `{}`:\n--- expected (Perl ExifTool) ---\n{want}--- got (exiftool-rs) ---\n{got}",
                    case.id
                ));
            }
        }
        std::fs::remove_file(&tmp).ok();
    }

    assert!(
        mismatches.is_empty(),
        "write-parity mismatch vs Perl ExifTool:\n\n{}",
        mismatches.join("\n")
    );
}
