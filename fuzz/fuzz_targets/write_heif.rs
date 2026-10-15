//! Rewrite arbitrary bytes as a HEIF file: the item-table parser, the
//! relocation of every `iloc` entry, and the EXIF and XMP editors must fail
//! cleanly, never panic, hang or overflow. A rewrite that succeeds must
//! produce a file whose item tables parse again.
//!
//! The first byte picks the edit (bits 0-1: EXIF keep/remove/replace/edit,
//! bits 2-3: the same for XMP); the rest is the file.

#![no_main]

use exiftool_rs::formats::heif_items;
use exiftool_rs::writer::exif_edit::{self, ExifChanges, ExifOutcome, Ifd};
use exiftool_rs::writer::heif_writer::{self, ItemChange};
use exiftool_rs::writer::xmp_edit::{self, XmpChanges, XmpOutcome};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&pick, body)) = data.split_first() else {
        return;
    };
    let current = heif_writer::read_metadata(body).unwrap_or_default();
    let exif = match pick & 3 {
        0 => ItemChange::Keep,
        1 => ItemChange::Remove,
        2 => ItemChange::Replace(b"\0\0\0\x06Exif\0\0MM\0\x2a\0\0\0\x08\0\0\0\0\0\0".to_vec()),
        _ => {
            // Delete GPS and a tag from whatever EXIF is there.
            let changes = ExifChanges {
                delete_dirs: vec![Ifd::Gps],
                delete_tags: vec![(None, "make".into())],
                ..Default::default()
            };
            let tiff = current.exif.as_deref().and_then(|p| {
                let n = u32::from_be_bytes(p.get(..4)?.try_into().ok()?) as usize;
                p.get(n.checked_add(4)?..)
            });
            match exif_edit::edit_exif(tiff, &changes) {
                Ok(ExifOutcome::Rewritten(t)) => {
                    ItemChange::Replace([&[0, 0, 0, 0][..], &t].concat())
                }
                Ok(ExifOutcome::Removed) => ItemChange::Remove,
                _ => ItemChange::Keep,
            }
        }
    };
    let xmp = match (pick >> 2) & 3 {
        0 => ItemChange::Keep,
        1 => ItemChange::Remove,
        2 => ItemChange::Replace(vec![b'x'; (pick as usize) * 37]),
        _ => {
            let changes = XmpChanges {
                delete_tags: vec![(Some("dc".into()), "description".into())],
                toolkit: "fuzz".into(),
                ..Default::default()
            };
            match xmp_edit::edit_xmp(current.xmp.as_deref(), &changes) {
                Ok(XmpOutcome::Rewritten(x)) => ItemChange::Replace(x),
                Ok(XmpOutcome::Removed) => ItemChange::Remove,
                _ => ItemChange::Keep,
            }
        }
    };
    if let Ok(out) = heif_writer::rewrite(body, exif, xmp) {
        assert!(heif_items::top_level_meta(&out).is_ok());
    }
});
