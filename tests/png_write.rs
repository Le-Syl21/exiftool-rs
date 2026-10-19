//! PNG metadata writing (issue #22): `PNG:all`, the tIME/gAMA/sRGB chunks,
//! the `XML:com.adobe.xmp` keyword in each text chunk type, and the groups
//! only a PNG has (`PNG-pHYs`, `ICC_Profile`), compared chunk by chunk with
//! ExifTool 13.59.
//!
//! `tests/issue22/` holds five synthetic fixtures (`make_fixtures.py`), the
//! cases (`cases.txt`) and what ExifTool made of each case on each fixture
//! (`expected.txt`, from `make_expected.py`): whether it reported the file
//! changed, and the chunks of its output with their CRC, so a chunk ExifTool
//! keeps must come out byte for byte the same. The fixtures stay out of the
//! parity corpus: the matrix there picks its own edits.

use exiftool_rs::ExifTool;
use std::path::{Path, PathBuf};

/// Cases where we knowingly differ from ExifTool 13.59, with the reason.
const DIVERGENT: &[(&str, &str, &str)] = &[
    (
        "sink",
        "del-exif-all",
        "ExifTool drops the `Raw profile type exif` chunk but reports the file \
         unchanged (ProcessProfile does not count that deletion); the file did \
         change, so the count says so",
    ),
    (
        "sink",
        "set-xmp-title",
        "ExifTool also edits the XMP of a `Raw profile type xmp` chunk; raw \
         profiles are only deleted here, never edited",
    ),
];

/// A case of `cases.txt`: its name and its edits (tag, value or deletion).
type Case = (String, Vec<(String, Option<String>)>);

struct Expected {
    fixture: String,
    case: String,
    status: String,
    chunks: Vec<String>,
}

fn cases() -> Vec<Case> {
    let text = std::fs::read_to_string("tests/issue22/cases.txt").unwrap();
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            let mut parts = l.split('\t');
            let name = parts.next().unwrap().to_string();
            let ops = parts
                .map(|op| {
                    let (tag, value) = op.split_once('=').unwrap();
                    let value = (!value.is_empty()).then(|| value.to_string());
                    (tag.to_string(), value)
                })
                .collect();
            (name, ops)
        })
        .collect()
}

fn expected() -> Vec<Expected> {
    let text = std::fs::read_to_string("tests/issue22/expected.txt").unwrap();
    text.lines()
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            Expected {
                fixture: f[0].into(),
                case: f[1].into(),
                status: f[2].into(),
                chunks: f[3..].iter().map(|c| c.to_string()).collect(),
            }
        })
        .collect()
}

/// The chunks of a PNG as `make_expected.py` lists them.
fn chunks(data: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut p = 8;
    while p + 12 <= data.len() {
        let n = u32::from_be_bytes(data[p..p + 4].try_into().unwrap()) as usize;
        let typ = String::from_utf8_lossy(&data[p + 4..p + 8]).into_owned();
        let body = &data[p + 8..p + 8 + n];
        let mut s = typ.clone();
        if matches!(typ.as_str(), "tEXt" | "zTXt" | "iTXt") {
            let kw = &body[..body.iter().position(|&b| b == 0).unwrap_or(body.len())];
            s += &format!("[{}]", String::from_utf8_lossy(kw));
        }
        let crc = u32::from_be_bytes(data[p + 8 + n..p + 12 + n].try_into().unwrap());
        s += &format!(":{crc:08x}");
        out.push(s);
        p += 12 + n;
        if typ == "IEND" {
            break;
        }
    }
    if p < data.len() {
        out.push(format!("trailer:{}", data.len() - p));
    }
    out
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("exiftool_rs_png_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn fixture(name: &str) -> PathBuf {
    Path::new("tests/issue22").join(format!("issue22_{name}.png"))
}

#[test]
fn png_writes_match_exiftool() {
    let cases = cases();
    let mut failures = Vec::new();
    let mut checked = 0;
    for e in expected() {
        if DIVERGENT
            .iter()
            .any(|(f, c, _)| *f == e.fixture && *c == e.case)
        {
            continue;
        }
        let ops = &cases.iter().find(|(n, _)| *n == e.case).unwrap().1;
        let id = format!("{}/{}", e.fixture, e.case);
        let dst = tmp(&format!("{}.{}.png", e.fixture, e.case));
        let mut et = ExifTool::new();
        for (tag, value) in ops {
            et.set_new_value(tag, value.as_deref());
        }
        let status = match et.write_info(fixture(&e.fixture), &dst) {
            Ok(0) => "unchanged",
            Ok(_) => "changed",
            Err(_) => "error",
        };
        checked += 1;
        if status != e.status {
            failures.push(format!("{id}: {status}, ExifTool {}", e.status));
            continue;
        }
        if status == "error" {
            continue;
        }
        let ours = chunks(&std::fs::read(&dst).unwrap());
        // A rewritten XMP packet names its writer (x:xmptk): compare the
        // chunk, not its bytes, where the case edits the XMP.
        let edits_xmp = ops
            .iter()
            .any(|(t, v)| v.is_some() && t.starts_with("XMP:"));
        let key = |s: &String| -> String {
            if edits_xmp && s.contains("[XML:com.adobe.xmp]") {
                s.rsplit_once(':').unwrap().0.to_string()
            } else {
                s.clone()
            }
        };
        let ours: Vec<String> = ours.iter().map(key).collect();
        let theirs: Vec<String> = e.chunks.iter().map(key).collect();
        if ours != theirs {
            failures.push(format!(
                "{id}:\n    ours:     {:?}\n    ExifTool: {:?}",
                ours.iter()
                    .filter(|c| !theirs.contains(c))
                    .collect::<Vec<_>>(),
                theirs
                    .iter()
                    .filter(|c| !ours.contains(c))
                    .collect::<Vec<_>>()
            ));
        }
    }
    assert!(checked > 100, "only {checked} cases checked");
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// The case of the issue: `PNG:all` with the EXIF and XMP deletions removes
/// the tIME chunk and every text chunk, the `XML:com.adobe.xmp` one included,
/// and leaves the image data, the ICC profile and pHYs byte for byte.
#[test]
fn issue22_leaves_no_text_or_timestamp() {
    for name in ["text", "itxt", "ztxt", "lang"] {
        let dst = tmp(&format!("issue22-{name}.png"));
        let mut et = ExifTool::new();
        for tag in ["PNG:all", "EXIF:all", "XMP:all"] {
            et.set_new_value(tag, None);
        }
        let n = et.write_info(fixture(name), &dst).unwrap();
        // PNG:all, and whichever of EXIF and XMP the fixture has: the others an
        // XMP packet, `lang` an eXIf chunk and no XMP.
        assert_eq!(n, 2, "{name}");
        let tags = ExifTool::new().extract_info(&dst).unwrap();
        for gone in [
            "ModifyDate",
            "Author",
            "Gamma",
            "XMLcomadobexmp",
            "Description",
        ] {
            assert!(
                !tags.iter().any(|t| t.name == gone),
                "{name}: {gone} is still there"
            );
        }
        let src = chunks(&std::fs::read(fixture(name)).unwrap());
        let out = chunks(&std::fs::read(&dst).unwrap());
        for kept in ["IHDR", "iCCP", "pHYs", "IDAT", "IEND"] {
            let pick = |l: &[String]| l.iter().find(|c| c.starts_with(kept)).cloned();
            assert_eq!(pick(&out), pick(&src), "{name}: {kept}");
        }
    }
}

/// `PNG:XMLcomadobexmp` (the `XML:com.adobe.xmp` iTXt chunk with a language
/// code) is not a tag ExifTool can write: with `-Author= -PNG:XMLcomadobexmp=`
/// ExifTool 13.59 warns "Tag 'PNG:XMLcomadobexmp' is not defined", deletes
/// Author and keeps the chunk; alone, it copies the file unchanged.
#[test]
fn undefined_png_tag_is_skipped_with_a_warning() {
    let mut et = ExifTool::new();
    et.set_new_value("Author", None);
    et.set_new_value("PNG:XMLcomadobexmp", None);
    let dst = tmp("undefined-with-author.png");
    assert_eq!(et.write_info(fixture("lang"), &dst).unwrap(), 1);
    let warnings = et.write_warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("'PNG:XMLcomadobexmp'"), "{warnings:?}");
    let out = chunks(&std::fs::read(&dst).unwrap());
    assert!(
        !out.iter().any(|c| c.starts_with("tEXt[Author]")),
        "{out:?}"
    );
    let src = chunks(&std::fs::read(fixture("lang")).unwrap());
    let xmp = |l: &[String]| {
        l.iter()
            .find(|c| c.starts_with("iTXt[XML:com.adobe.xmp]"))
            .cloned()
    };
    assert_eq!(xmp(&out), xmp(&src));

    let mut et = ExifTool::new();
    et.set_new_value("PNG:XMLcomadobexmp", None);
    let dst = tmp("undefined-alone.png");
    assert_eq!(et.write_info(fixture("lang"), &dst).unwrap(), 0);
    assert_eq!(
        std::fs::read(&dst).unwrap(),
        std::fs::read(fixture("lang")).unwrap()
    );
    assert_eq!(et.write_warnings().len(), 1);
}

/// A date ExifTool refuses (no time) is not written, and is reported so: as
/// ExifTool 13.59 ("Warning: Invalid date/time ...", "1 image files
/// copied"), the output is an unchanged copy, with a warning.
#[test]
fn invalid_modify_date_is_refused() {
    let dst = tmp("bad-date.png");
    let mut et = ExifTool::new();
    et.set_new_value("PNG:ModifyDate", Some("2025:01:02"));
    assert_eq!(et.write_info(fixture("text"), &dst).unwrap(), 0);
    assert_eq!(
        std::fs::read(&dst).unwrap(),
        std::fs::read(fixture("text")).unwrap()
    );
    assert_eq!(et.write_warnings().len(), 1);
}
