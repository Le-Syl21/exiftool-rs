//! What the facade (`exiftool_rs::ExifTool`, the root crate's API over
//! Image::ExifTool) guarantees, one test per requirement drawn from the
//! issues of the root crate. Expected values are Perl ExifTool 13.59's.

use std::path::{Path, PathBuf};

use exiftool_rs::{ExifTool, FileType, Options, Value};

fn dir(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join("api_requirements").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn image(name: &str) -> PathBuf {
    Path::new("tests/images").join(name)
}

/// `Group1:Name` = printed value, every instance.
fn tags(path: &Path) -> Vec<(String, String)> {
    let et = ExifTool::with_options(Options { duplicates: true, ..Options::default() });
    et.extract_info(path).unwrap().into_iter().map(|t| (format!("{}:{}", t.group.family1, t.name), t.print_value)).collect()
}

fn get<'a>(tags: &'a [(String, String)], key: &str) -> Option<&'a str> {
    tags.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

fn leftovers(d: &Path) -> Vec<String> {
    std::fs::read_dir(d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with(".exiftool-")).collect()
}

#[test]
fn a_copy_reads_the_source_with_duplicates() {
    // GPS.jpg: without duplicates, Composite:GPSLatitude would hide
    // GPS:GPSLatitude, and composites are not copied.
    let d = dir("copy");
    let mut et = ExifTool::new();
    assert!(et.set_new_values_from_file(image("GPS.jpg"), None).unwrap() > 0);
    let out = d.join("out.jpg");
    et.write_info(image("Writer.jpg"), &out).unwrap();
    let src = tags(&image("GPS.jpg"));
    let dst = tags(&out);
    for key in ["GPS:GPSLatitude", "GPS:GPSLongitude", "GPS:GPSLatitudeRef", "GPS:GPSLongitudeRef"] {
        assert!(get(&src, key).is_some(), "{key}");
        assert_eq!(get(&dst, key), get(&src, key), "{key}");
    }
}

#[test]
fn a_deletion_queued_after_a_copy_wins() {
    let d = dir("copy_then_delete");
    let mut et = ExifTool::new();
    et.set_new_values_from_file(image("Canon.jpg"), None).unwrap();
    et.set_new_value("Model", None);
    et.set_new_value("EXIF:Make", Some("After the copy"));
    let out = d.join("out.jpg");
    et.write_info(image("Writer.jpg"), &out).unwrap();
    let dst = tags(&out);
    assert_eq!(get(&dst, "IFD0:Model"), None);
    assert_eq!(get(&dst, "IFD0:Make"), Some("After the copy"));
    // the rest of the copy is there
    assert_eq!(get(&dst, "ExifIFD:ExposureTime"), get(&tags(&image("Canon.jpg")), "ExifIFD:ExposureTime"));
}

#[test]
fn write_info_counts_the_writes_that_took_effect() {
    let d = dir("count");
    let src = image("Writer.jpg");
    let mut et = ExifTool::new();
    et.set_new_value("Artist", Some("Me"));
    et.set_new_value("XMP-dc:Title", Some("T"));
    assert_eq!(et.write_info(&src, d.join("two.jpg")).unwrap(), 2);
    // deleting a tag the file does not have changes nothing: Ok(0), and
    // the output is still written
    let mut et = ExifTool::new();
    et.set_new_value("LensSerialNumber", None);
    let out = d.join("absent.jpg");
    assert_eq!(et.write_info(&src, &out).unwrap(), 0);
    assert!(out.exists());
    // only the deletion that removes something counts
    let mut et = ExifTool::new();
    et.set_new_value("LensSerialNumber", None);
    et.set_new_value("EXIF:Make", None);
    assert_eq!(et.write_info(image("Canon.jpg"), d.join("one.jpg")).unwrap(), 1);
    assert!(leftovers(&d).is_empty(), "{:?}", leftovers(&d));
}

#[cfg(unix)]
#[test]
fn write_info_replaces_the_file_through_a_temporary_one_keeping_its_permissions() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let d = dir("in_place");
    let file = d.join("photo.jpg");
    std::fs::copy(image("Writer.jpg"), &file).unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
    let ino = std::fs::metadata(&file).unwrap().ino();
    let mut et = ExifTool::new();
    et.set_new_value("Artist", Some("In place"));
    assert_eq!(et.write_info(&file, &file).unwrap(), 1);
    let meta = std::fs::metadata(&file).unwrap();
    assert_eq!(meta.permissions().mode() & 0o777, 0o640);
    // a new file renamed over the old one, not the old one rewritten
    assert_ne!(meta.ino(), ino);
    assert_eq!(get(&tags(&file), "IFD0:Artist"), Some("In place"));
    assert!(leftovers(&d).is_empty(), "{:?}", leftovers(&d));
}

#[test]
fn a_failed_write_leaves_no_temporary_file() {
    let d = dir("failure");
    // the destination is a directory that is not empty: the rename fails
    let dst = d.join("out.jpg");
    std::fs::create_dir_all(dst.join("inside")).unwrap();
    let mut et = ExifTool::new();
    et.set_new_value("Artist", Some("x"));
    assert!(et.write_info(image("Writer.jpg"), &dst).is_err());
    assert!(leftovers(&d).is_empty(), "{:?}", leftovers(&d));
    // nothing to do in place: an error, the file untouched
    let file = d.join("photo.jpg");
    std::fs::copy(image("Writer.jpg"), &file).unwrap();
    let mut et = ExifTool::new();
    et.set_new_value("NoSuchTag", Some("x"));
    assert!(et.write_info(&file, &file).is_err());
    assert_eq!(std::fs::read(&file).unwrap(), std::fs::read(image("Writer.jpg")).unwrap());
    assert!(leftovers(&d).is_empty(), "{:?}", leftovers(&d));
}

#[test]
fn write_warnings_are_exiftools() {
    let d = dir("warnings");
    let mut et = ExifTool::new();
    et.set_new_value("NoSuchTag", Some("x"));
    et.set_new_value("Artist", Some("y"));
    assert_eq!(et.write_info(image("Writer.jpg"), d.join("out.jpg")).unwrap(), 1);
    // SetNewValue's own words
    assert_eq!(et.write_warnings(), ["Tag 'NoSuchTag' is not defined"]);
    let mut et = ExifTool::new();
    et.set_new_value("EXIF:Orientation", Some("Sideways"));
    // nothing else to do: an unchanged copy
    assert_eq!(et.write_info(image("Writer.jpg"), d.join("bad.jpg")).unwrap(), 0);
    assert_eq!(et.write_warnings(), ["Can't convert IFD0:Orientation (not in PrintConv)"]);
}

#[test]
fn raw_values_are_typed_numbers() {
    let et = ExifTool::with_options(Options { print_conv: false, ..Options::default() });
    let tags = et.extract_info(image("NikonD70.jpg")).unwrap();
    let raw = |name: &str| tags.iter().find(|t| t.name == name).map(|t| t.raw_value.clone()).unwrap();
    assert_eq!(raw("FNumber"), Value::F64(5.0));
    assert_eq!(raw("ExposureTime").as_f64().map(|v| (v * 60.0).round()), Some(1.0));
    assert_eq!(raw("Orientation"), Value::U16(1));
    // a count of several numbers is a list of them (the Nikon ISO, the one
    // ExifTool gives priority to here)
    assert_eq!(raw("ISO"), Value::List(vec![Value::U16(0), Value::U16(200)]));
    // and image_info gives their text, as `exiftool -n`
    let info = et.image_info(image("NikonD70.jpg")).unwrap();
    assert_eq!(info.get("ISO").map(String::as_str), Some("0 200"));
    assert_eq!(info.get("Orientation").map(String::as_str), Some("1"));
    assert_eq!(info.get("FNumber").map(String::as_str), Some("5"));
    // printed, as `exiftool`
    let info = ExifTool::new().image_info(image("NikonD70.jpg")).unwrap();
    assert_eq!(info.get("ISO").map(String::as_str), Some("200"));
    assert_eq!(info.get("FNumber").map(String::as_str), Some("5.0"));
}

#[test]
fn binary_data_is_extracted_with_the_binary_option() {
    // As ImageInfo with and without `Binary => 1` (Perl ExifTool 13.59):
    // ExifTool.jpg's PreviewImage is "Binary data 1039273 bytes" without it,
    // 5777 bytes of JPEG with it; its ThumbnailImage is extracted either way.
    let raw = |binary: bool, file: &str, name: &str, requested: &[&str]| -> Value {
        let et = ExifTool::with_options(Options { binary, requested_tags: requested.iter().map(|t| t.to_string()).collect(), ..Options::default() });
        let tags = et.extract_info(image(file)).unwrap();
        let t = tags.iter().find(|t| t.name == name).unwrap_or_else(|| panic!("{file}: no {name}"));
        assert!(t.print_value.starts_with("(Binary data "), "{file} {name}: {}", t.print_value);
        t.raw_value.clone()
    };
    let bytes = |v: Value| match v {
        Value::Binary(b) => b,
        other => panic!("not binary: {other:?}"),
    };
    assert_eq!(raw(false, "ExifTool.jpg", "PreviewImage", &[]), Value::String("(Binary data 1039273 bytes, use -b option to extract)".into()));
    let preview = bytes(raw(true, "ExifTool.jpg", "PreviewImage", &[]));
    assert_eq!(preview.len(), 5777);
    assert!(preview.starts_with(b"\xff\xd8\xff\xdb"));
    assert_eq!(bytes(raw(false, "ExifTool.jpg", "ThumbnailImage", &[])).len(), 1558);
    assert_eq!(raw(false, "Nikon.nef", "JpgFromRaw", &[]), Value::String("(Binary data 29 bytes, use -b option to extract)".into()));
    assert_eq!(bytes(raw(true, "Nikon.nef", "JpgFromRaw", &[])).len(), 29);
    // A tag requested by name has its bytes without the option, as with
    // ExifTool.
    assert_eq!(bytes(raw(false, "ExifTool.jpg", "PreviewImage", &["PreviewImage"])), preview);
    // (a composite one needs the tags it is made from: ImageInfo gives
    // nothing for Nikon.nef's PreviewImage requested alone, in Perl too)
    let et = ExifTool::with_options(Options { requested_tags: vec!["PreviewImage".into()], ..Options::default() });
    assert!(et.extract_info(image("Nikon.nef")).unwrap().is_empty());
}

#[test]
fn values_are_trimmed_on_request() {
    let tag = |trim: bool, file: &str, key: &str| -> (String, Value) {
        let et = ExifTool::with_options(Options { trim, duplicates: true, ..Options::default() });
        let tags = et.extract_info(image(file)).unwrap();
        let t = tags.iter().find(|t| format!("{}:{}", t.group.family1, t.name) == key).unwrap_or_else(|| panic!("{file}: no {key}"));
        (t.print_value.clone(), t.raw_value.clone())
    };
    // ExifTool's values by default, as `exiftool -j` gives them
    let padded = "OLYMPUS DIGITAL CAMERA         ";
    assert_eq!(tag(false, "Olympus2.jpg", "IFD0:ImageDescription"), (padded.to_string(), Value::String(padded.into())));
    assert_eq!(tag(false, "GPS.jpg", "IPTC:City").0, " ");
    // trimmed: the printed and the raw value
    assert_eq!(tag(true, "Olympus2.jpg", "IFD0:ImageDescription"), ("OLYMPUS DIGITAL CAMERA".to_string(), Value::String("OLYMPUS DIGITAL CAMERA".into())));
    assert_eq!(tag(true, "GPS.jpg", "IPTC:City"), (String::new(), Value::String(String::new())));
    // numbers and binary data are left as they are
    let (_, orientation) = tag(true, "NikonD70.jpg", "IFD0:Orientation");
    assert_eq!(orientation, Value::U16(1));
    let (_, thumbnail) = tag(true, "ExifTool.jpg", "JFXX:ThumbnailImage");
    assert!(matches!(&thumbnail, Value::Binary(b) if b.len() == 1558), "{thumbnail:?}");
}

#[test]
fn lens_id_does_not_depend_on_duplicates() {
    let lens = |duplicates: bool| -> Vec<String> {
        ExifTool::with_options(Options { duplicates, ..Options::default() })
            .extract_info(image("NikonD70.jpg"))
            .unwrap()
            .into_iter()
            .filter(|t| t.name == "LensID")
            .map(|t| format!("{}:{}", t.group.family0, t.print_value))
            .collect()
    };
    let expected = ["Composite:AF-S DX Zoom-Nikkor 18-70mm f/3.5-4.5G IF-ED"];
    assert_eq!(lens(false), expected);
    assert_eq!(lens(true), expected);
}

#[test]
fn structures_are_values() {
    let et = ExifTool::with_options(Options { struct_output: true, ..Options::default() });
    let tags = et.extract_info(image("XMP.jpg")).unwrap();
    let structs: Vec<_> = tags.iter().filter(|t| t.raw_value.has_struct()).collect();
    assert!(!structs.is_empty());
    for t in structs {
        // printed as `exiftool -struct` prints it
        assert!(t.print_value.starts_with('{') || t.print_value.starts_with('['), "{}: {}", t.name, t.print_value);
    }
    let flat = ExifTool::new().extract_info(image("XMP.jpg")).unwrap();
    assert!(flat.iter().all(|t| !t.raw_value.has_struct()));
}

#[test]
fn png_all_is_deleted() {
    let d = dir("png_all");
    let mut et = ExifTool::new();
    et.set_new_value("PNG:all", None);
    let out = d.join("out.png");
    assert_eq!(et.write_info(image("PNG.png"), &out).unwrap(), 1);
    let after = tags(&out);
    // the text chunks are gone, the image header stays
    assert!(after.iter().all(|(k, _)| k != "PNG:Title" && k != "PNG:Comment" && k != "PNG:ModifyDate"), "{after:?}");
    assert!(get(&after, "PNG:ImageWidth").is_some());
    // deleting it again changes nothing
    let mut et = ExifTool::new();
    et.set_new_value("PNG:all", None);
    assert_eq!(et.write_info(&out, d.join("again.png")).unwrap(), 0);
}

#[test]
fn file_types_are_exiftools() {
    assert_eq!(exiftool_rs::get_file_type(image("Canon.jpg")).unwrap(), FileType::Jpeg);
    assert_eq!(exiftool_rs::get_file_type(image("PNG.png")).unwrap(), FileType::Png);
    assert_eq!(exiftool_rs::file_type::detect_from_extension("cr2"), Some(FileType::Cr2));
    assert_eq!(exiftool_rs::file_type::detect_from_magic(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"), Some(FileType::Png));
    assert_eq!(FileType::Jpeg.support(), exiftool_rs::file_type::Support::ReadWrite);
    assert_eq!(FileType::Xmp.support(), exiftool_rs::file_type::Support::ReadWriteCreate);
    assert_eq!(FileType::Mp3.support(), exiftool_rs::file_type::Support::Read);
    assert!(ExifTool::writable_tags(FileType::Mp3).unwrap().is_empty());
    assert!(ExifTool::writable_tags(FileType::Jpeg).unwrap().contains("artist"));
}

#[test]
fn mwg_composites_are_loaded_on_request() {
    let mwg = |use_mwg: bool| -> bool {
        ExifTool::with_options(Options { use_mwg, ..Options::default() })
            .extract_info(image("Canon.jpg"))
            .unwrap()
            .iter()
            .any(|t| t.group.family0 == "Composite" && t.name == "DateTimeOriginal")
    };
    assert!(mwg(true));
    // and only then: each object has an ExifTool of its own
    assert!(!mwg(false));
    // nor once turned off on the same object (MWG cannot be unloaded: the
    // object's interpreter is replaced)
    let mut et = ExifTool::with_options(Options { use_mwg: true, ..Options::default() });
    let has = |et: &ExifTool| et.extract_info(image("Canon.jpg")).unwrap().iter().any(|t| t.group.family0 == "Composite" && t.name == "DateTimeOriginal");
    assert!(has(&et));
    et.options_mut().use_mwg = false;
    assert!(!has(&et));
}

#[test]
fn one_interpreter_per_object() {
    // An ExifTool reuses its interpreter and Image::ExifTool object, so
    // ExifTool's tables carry from one file to the next as in Perl: with one
    // Image::ExifTool object, `ImageInfo` gives XMP.xml no AutoFocus, then
    // reads Nikon.jpg (Nikon.pm defines the AutoFocus composite), then gives
    // XMP.xml AutoFocus "On".
    let auto_focus = |et: &ExifTool| et.image_info(image("XMP.xml")).unwrap().get("AutoFocus").cloned();
    let et = ExifTool::new();
    assert_eq!(auto_focus(&et), None);
    assert_eq!(et.image_info(image("Nikon.jpg")).unwrap().get("AutoFocus").map(String::as_str), Some("On"));
    assert_eq!(auto_focus(&et).as_deref(), Some("On"));
    // A new object starts from a new interpreter.
    assert_eq!(auto_focus(&ExifTool::new()), None);
    // With fresh_interpreter, every call does.
    let et = ExifTool::with_options(Options { fresh_interpreter: true, ..Options::default() });
    et.image_info(image("Nikon.jpg")).unwrap();
    assert_eq!(auto_focus(&et), None);
    // The options of one call do not stay for the next (ClearOptions).
    let mut et = ExifTool::with_options(Options { requested_tags: vec!["Model".into()], ..Options::default() });
    assert_eq!(et.extract_info(image("Canon.jpg")).unwrap().len(), 1);
    et.options_mut().requested_tags.clear();
    assert!(et.extract_info(image("Canon.jpg")).unwrap().len() > 50);
}

#[test]
fn engines_are_per_thread() {
    // ExifTool is Send + Sync: threads sharing one take turns on its
    // interpreter.
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ExifTool>();
    let et = std::sync::Arc::new(ExifTool::new());
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let et = et.clone();
            std::thread::spawn(move || et.image_info(image("Canon.jpg")).unwrap().get("Model").cloned())
        })
        .collect();
    for h in handles {
        assert_eq!(h.join().unwrap().as_deref(), Some("Canon EOS DIGITAL REBEL"));
    }
}

#[test]
fn embedded_binary_is_binary() {
    // PDF's EmbeddedImage ValueConv is a SCALAR reference to an element of
    // the stream's dictionary: its raw value is the data (as many bytes as
    // Perl ExifTool 13.60 reports), never the reference's "SCALAR(0x...)"
    // text, and it prints as any binary value.
    for (file, size) in [("PDF.pdf", 501), ("PDF2.pdf", 251)] {
        let et = ExifTool::with_options(Options { extract_embedded: 1, ..Options::default() });
        let tags = et.extract_info(image(file)).unwrap();
        let t = tags.iter().find(|t| t.name == "EmbeddedImage").unwrap_or_else(|| panic!("{file}: no EmbeddedImage"));
        match &t.raw_value {
            Value::Binary(b) => assert_eq!(b.len(), size, "{file}"),
            other => panic!("{file}: EmbeddedImage raw value {other:?}"),
        }
        assert_eq!(t.print_value, format!("(Binary data {size} bytes, use -b option to extract)"), "{file}");
    }
}
