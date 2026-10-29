//! Maker-note decoding once Make, Model or Software has been edited.
//!
//! ExifTool chooses how to read a maker note from `$$self{Make}`,
//! `$$self{Model}` and `$$self{Software}` -- the IFD0 values, read before the
//! ExifIFD that holds the note -- and its sub-tables test the same members.
//! Changing or deleting one of them therefore changes what ExifTool reports.
//! Each case writes the edit with our writer, reads the result back, and
//! checks the tags Perl ExifTool 13.59 reports on that same output (the
//! write-parity matrix, `parity --dump-write`, is where the values come from).

use std::path::{Path, PathBuf};

use exiftool_rs::{ExifTool, Options, Tag};

fn edited(case: &str, src: &str, ops: &[(&str, Option<&str>)]) -> Vec<Tag> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("makernote_conditions");
    std::fs::create_dir_all(&dir).unwrap();
    let ext = Path::new(src).extension().unwrap().to_str().unwrap();
    let dst: PathBuf = dir.join(format!("{case}.{ext}"));
    let mut et = ExifTool::new();
    for (tag, value) in ops {
        et.set_new_value(tag, *value);
    }
    et.write_info(Path::new("tests/images").join(src), &dst)
        .unwrap_or_else(|e| panic!("{case}: write failed: {e}"));
    ExifTool::with_options(Options {
        duplicates: true,
        ..Options::default()
    })
    .extract_info(&dst)
    .unwrap()
}

/// The printed value of `group1:name`, if there is one.
fn get<'a>(tags: &'a [Tag], group1: &str, name: &str) -> Option<&'a str> {
    tags.iter()
        .find(|t| t.group.family1 == group1 && t.name == name)
        .map(|t| t.print_value.as_str())
}

fn warnings(tags: &[Tag]) -> Vec<&str> {
    tags.iter()
        .filter(|t| t.name == "Warning")
        .map(|t| t.print_value.as_str())
        .collect()
}

#[test]
fn a_canon_note_under_another_make_is_an_unknown_one() {
    // No arm of MakerNotes.pm claims it: MakerNoteUnknown names nothing.
    let tags = edited(
        "canon_make",
        "Canon.jpg",
        &[("EXIF:Make", Some("Set by write parity"))],
    );
    assert!(
        !tags.iter().any(|t| t.group.family0 == "MakerNotes"),
        "no maker-note tag"
    );
    // CalcSensorDiag is for `Make eq 'Canon'`; the scale factor then comes
    // from ExifImageWidth/Height, not RelatedImageWidth/Height.
    assert_eq!(get(&tags, "Composite", "ScaleFactor35efl"), Some("29.3"));
}

#[test]
fn a_kodak_note_under_another_make_is_unrecognized() {
    let tags = edited(
        "kodak_make",
        "Kodak.jpg",
        &[("EXIF:Make", Some("Set by write parity"))],
    );
    assert!(warnings(&tags).contains(&"[minor] Unrecognized MakerNotes"));
    assert!(!tags.iter().any(|t| t.group.family0 == "MakerNotes"));
}

#[test]
fn a_missing_model_matches_no_model_pattern() {
    // Canon FocalLength and AFInfo: valid for any model that is not an EOS.
    let tags = edited("canon_no_model", "Canon.jpg", &[("EXIF:Model", None)]);
    assert_eq!(get(&tags, "Canon", "FocalPlaneXSize"), Some("23.22 mm"));
    assert_eq!(get(&tags, "Canon", "FocalPlaneYSize"), Some("15.49 mm"));
    assert_eq!(get(&tags, "Canon", "PrimaryAFPoint"), Some("65535"));

    // CanonCustom::Functions2: the 1D variants no longer apply.
    let tags = edited(
        "canon1d3_no_model",
        "Canon1DmkIII.jpg",
        &[("EXIF:Model", None)],
    );
    assert_eq!(get(&tags, "Canon", "AFPointsSelected"), None);
    assert_eq!(get(&tags, "Canon", "PrimaryAFPoint"), Some("26284"));
    assert_eq!(get(&tags, "CanonCustom", "AEBShotCount"), Some("7 shots"));
    assert_eq!(
        get(&tags, "CanonCustom", "ExposureLevelIncrements"),
        Some("1/3 Stop")
    );
    assert_eq!(
        get(&tags, "CanonCustom", "AFMicroadjustment"),
        Some("Disable; 0; 0; 0; 0")
    );

    // Nikon AFInfo is little-endian unless the model is a NIKON D.
    let tags = edited("nikond70_no_model", "NikonD70.jpg", &[("EXIF:Model", None)]);
    assert_eq!(get(&tags, "Nikon", "AFPointsInFocus"), Some("Lower-right"));

    // Pentax CameraSettings and BatteryInfo: the K10D entries go.
    let tags = edited("pentax_no_model", "Pentax.jpg", &[("EXIF:Model", None)]);
    assert_eq!(get(&tags, "Pentax", "AvApertureSetting"), None);
    assert_eq!(get(&tags, "Pentax", "RawAndJpgRecording"), None);
    assert_eq!(get(&tags, "Pentax", "GripBatteryState"), None);
    assert_eq!(
        get(&tags, "Pentax", "BodyBatteryState"),
        Some("Close to Full")
    );

    // Ricoh: only the Caplio RR1 counts the sub-directory offsets from its
    // own start; any other model counts them from the TIFF header.
    // (the value as ExifTool has it, trailing spaces included: only the
    // exiftool application's text output trims them)
    let tags = edited("ricoh_no_model", "Ricoh.jpg", &[("EXIF:Model", None)]);
    assert_eq!(get(&tags, "Ricoh", "ManufactureDate1"), Some("RICOH      "));
}

#[test]
fn sony_arw_is_not_written() {
    // ExifTool 13.59 refuses these edits of SonyILCE9.arw (a fixture whose
    // preview image is cut off): "Error reading PreviewImage data in IFD1",
    // "0 image files updated". The output is not created.
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("makernote_conditions");
    std::fs::create_dir_all(&dir).unwrap();
    for (case, tag, value) in [
        ("sony_software", "EXIF:Software", Some("Set by write parity")),
        ("sony_no_software", "EXIF:Software", None),
        ("sony_no_model", "IFD0:Model", None),
    ] {
        let dst = dir.join(format!("{case}.arw"));
        let _ = std::fs::remove_file(&dst);
        let mut et = ExifTool::new();
        et.set_new_value(tag, value);
        let err = et
            .write_info(Path::new("tests/images/SonyILCE9.arw"), &dst)
            .expect_err(case);
        assert!(err.to_string().contains("Error reading PreviewImage data in IFD1"), "{case}: {err}");
        assert!(!dst.exists(), "{case}");
    }
}
