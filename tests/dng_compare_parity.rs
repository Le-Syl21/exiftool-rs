//! Parity under dng-compare's extraction flags (`-j -a -G0:1:4 -n -struct -b -u -U`),
//! checked against Perl ExifTool 13.59 output captured on 2026-10-16.
//! Fixtures are synthetic: a hand-built TIFF/DNG and Lightroom-style XMP.
use exiftool_rs::{ExifTool, Options, Value};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("exiftool-rs-dng-compare-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Little-endian TIFF, 1x1 8-bit gray, DNGVersion 1.4.0.0, plus the XMP packet in tag 700.
fn dng_with_xmp(xmp: &[u8]) -> Vec<u8> {
    let entries: Vec<(u16, u16, u32, u32)> = vec![
        (256, 4, 1, 1),
        (257, 4, 1, 1),
        (258, 3, 1, 8),
        (259, 3, 1, 1),
        (262, 3, 1, 1),
        (273, 4, 1, 0),
        (277, 3, 1, 1),
        (279, 4, 1, 1),
        (700, 1, xmp.len() as u32, 0),
        (50706, 1, 4, 0x0000_0401),
    ];
    let ifd_len = 2 + entries.len() * 12 + 4;
    let strip_offset = 8 + ifd_len;
    let xmp_offset = strip_offset + 1;
    let mut data = vec![0x49, 0x49, 42, 0, 8, 0, 0, 0];
    data.extend((entries.len() as u16).to_le_bytes());
    for (tag, kind, count, value) in entries {
        let value = match tag {
            273 => strip_offset as u32,
            700 => xmp_offset as u32,
            _ => value,
        };
        data.extend(tag.to_le_bytes());
        data.extend(kind.to_le_bytes());
        data.extend(count.to_le_bytes());
        data.extend(value.to_le_bytes());
    }
    data.extend(0u32.to_le_bytes());
    data.push(0x7f);
    data.extend_from_slice(xmp);
    data
}

const LIGHTROOM_XMP: &str = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/" xmlns:stEvt="http://ns.adobe.com/xap/1.0/sType/ResourceEvent#" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:exif="http://ns.adobe.com/exif/1.0/" crs:Version="15.0" crs:Exposure2012="+0.50"><xmpMM:History><rdf:Seq><rdf:li stEvt:action="derived" stEvt:parameters="converted from image/x-canon-cr2 to image/dng"/><rdf:li rdf:parseType="Resource"><stEvt:action>saved</stEvt:action><stEvt:instanceID>xmp.iid:abc</stEvt:instanceID><stEvt:when>2024-05-06T10:11:12+02:00</stEvt:when><stEvt:softwareAgent>Adobe Lightroom 7.3</stEvt:softwareAgent><stEvt:changed>/metadata</stEvt:changed></rdf:li></rdf:Seq></xmpMM:History><crs:Look rdf:parseType="Resource"><crs:Name>Adobe Color</crs:Name><crs:Amount>1.000000</crs:Amount><crs:Group><rdf:Alt><rdf:li xml:lang="x-default">Profiles</rdf:li><rdf:li xml:lang="de">Profile</rdf:li></rdf:Alt></crs:Group><crs:Parameters rdf:parseType="Resource"><crs:Version>15.0</crs:Version><crs:ToneCurvePV2012><rdf:Seq><rdf:li>0, 0</rdf:li><rdf:li>255, 255</rdf:li></rdf:Seq></crs:ToneCurvePV2012><crs:ConvertToGrayscale>False</crs:ConvertToGrayscale></crs:Parameters></crs:Look><crs:MaskGroupBasedCorrections><rdf:Seq><rdf:li rdf:parseType="Resource"><crs:What>Correction</crs:What><crs:LocalExposure2012>0.25</crs:LocalExposure2012><crs:CorrectionMasks><rdf:Seq><rdf:li rdf:parseType="Resource"><crs:What>Mask/Image</crs:What><crs:MaskValue>1</crs:MaskValue></rdf:li><rdf:li rdf:parseType="Resource"><crs:What>Mask/Gradient</crs:What><crs:MaskValue>0.5</crs:MaskValue></rdf:li></rdf:Seq></crs:CorrectionMasks></rdf:li><rdf:li rdf:parseType="Resource"><crs:What>Correction</crs:What><crs:LocalExposure2012>-0.10</crs:LocalExposure2012><crs:CorrectionMasks><rdf:Seq><rdf:li rdf:parseType="Resource"><crs:What>Mask/Brush</crs:What><crs:MaskValue>0.7</crs:MaskValue></rdf:li></rdf:Seq></crs:CorrectionMasks></rdf:li></rdf:Seq></crs:MaskGroupBasedCorrections><dc:subject><rdf:Bag><rdf:li>alpha</rdf:li><rdf:li>beta, gamma</rdf:li></rdf:Bag></dc:subject><dc:creator><rdf:Seq><rdf:li>one</rdf:li></rdf:Seq></dc:creator><dc:title><rdf:Alt><rdf:li xml:lang="x-default">T</rdf:li></rdf:Alt></dc:title><exif:ISOSpeedRatings><rdf:Seq><rdf:li>400</rdf:li></rdf:Seq></exif:ISOSpeedRatings></rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#;

fn write_fixture(name: &str, bytes: &[u8]) -> PathBuf {
    let path = temp_dir().join(name);
    std::fs::File::create(&path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
    path
}

/// The CLI's JSON output for one file, as text lines keyed by `"key": value`.
fn cli_json(args: &[&str], path: &PathBuf) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_exiftool-rs"))
        .args(args)
        .arg(path)
        .output()
        .expect("run exiftool-rs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

const DNG_COMPARE_ARGS: &[&str] = &["-j", "-a", "-G0:1:4", "-n", "-struct", "-b", "-u", "-U"];

fn member<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\": ");
    json.lines().find_map(|line| {
        let line = line.trim_start();
        line.strip_prefix(&needle)
            .map(|rest| rest.trim_end_matches(','))
    })
}

#[test]
fn pseudo_tags_and_composites_match_exiftool_numeric_output() {
    let path = write_fixture("plain.DNG", &dng_with_xmp(b""));
    let json = cli_json(DNG_COMPARE_ARGS, &path);
    // Perl ExifTool 13.59 -n values for the same file.
    assert_eq!(member(&json, "File:FileType"), Some("\"DNG\""));
    assert_eq!(member(&json, "File:FileTypeExtension"), Some("\"DNG\""));
    assert_eq!(member(&json, "File:ExifByteOrder"), Some("\"II\""));
    assert_eq!(member(&json, "Composite:ImageSize"), Some("\"1 1\""));
    assert_eq!(member(&json, "EXIF:IFD0:DNGVersion"), Some("\"1 4 0 0\""));
    assert_eq!(member(&json, "EXIF:IFD0:Compression"), Some("1"));
    assert!(member(&json, "File:System:FileSize")
        .unwrap()
        .bytes()
        .all(|b| b.is_ascii_digit()));
    // Print mode keeps the PrintConv forms.
    let printed = cli_json(&["-j", "-G0:1:4"], &path);
    assert_eq!(member(&printed, "File:FileTypeExtension"), Some("\"dng\""));
    assert_eq!(
        member(&printed, "File:ExifByteOrder"),
        Some("\"Little-endian (Intel, II)\"")
    );
    assert_eq!(member(&printed, "Composite:ImageSize"), Some("\"1x1\""));
    assert_eq!(
        member(&printed, "EXIF:IFD0:DNGVersion"),
        Some("\"1.4.0.0\"")
    );
}

#[test]
fn bare_file_name_has_directory_dot() {
    let path = write_fixture("bare.DNG", &dng_with_xmp(b""));
    let output = Command::new(env!("CARGO_BIN_EXE_exiftool-rs"))
        .current_dir(path.parent().unwrap())
        .args(["-j", "-G0:1", "-Directory", "bare.DNG"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains("\"File:System:Directory\": \".\""));
}

#[test]
fn struct_output_matches_exiftool_shape() {
    let path = write_fixture("lightroom.DNG", &dng_with_xmp(LIGHTROOM_XMP.as_bytes()));
    let json = cli_json(DNG_COMPARE_ARGS, &path);
    assert_eq!(
        member(&json, "XMP:XMP-xmpMM:History"),
        Some(
            r#"[{"Action": "derived", "Parameters": "converted from image/x-canon-cr2 to image/dng"},{"Action": "saved", "Changed": "/metadata", "InstanceID": "xmp.iid:abc", "SoftwareAgent": "Adobe Lightroom 7.3", "When": "2024:05:06 10:11:12+02:00"}]"#
        )
    );
    assert_eq!(
        member(&json, "XMP:XMP-crs:Look"),
        Some(
            r#"{"Amount": 1.000000, "Group": "Profiles", "Group-de": "Profile", "Name": "Adobe Color", "Parameters": {"ConvertToGrayscale": false, "ToneCurvePV2012": ["0, 0","255, 255"], "Version": 15.0}}"#
        )
    );
    assert_eq!(
        member(&json, "XMP:XMP-crs:MaskGroupBasedCorrections"),
        Some(
            r#"[{"CorrectionMasks": [{"MaskValue": 1, "What": "Mask/Image"},{"MaskValue": 0.5, "What": "Mask/Gradient"}], "LocalExposure2012": 0.25, "What": "Correction"},{"CorrectionMasks": [{"MaskValue": 0.7, "What": "Mask/Brush"}], "LocalExposure2012": -0.10, "What": "Correction"}]"#
        )
    );
    // Flattened fields of the structures are gone; simple properties stay.
    for gone in [
        "XMP:XMP-crs:LookName",
        "XMP:XMP-xmpMM:HistoryAction",
        "XMP:XMP-crs:MaskGroupBasedCorrectionsWhat",
    ] {
        assert!(
            member(&json, gone).is_none(),
            "{gone} should be replaced by its structure"
        );
    }
    assert_eq!(member(&json, "XMP:XMP-crs:Exposure2012"), Some("\"+0.50\""));
    // Lists are arrays even with one item; language alternatives are not lists.
    assert_eq!(member(&json, "XMP:XMP-dc:Creator"), Some(r#"["one"]"#));
    assert_eq!(member(&json, "XMP:XMP-exif:ISO"), Some("[400]"));
    assert_eq!(
        member(&json, "XMP:XMP-dc:Subject"),
        Some(r#"["alpha","beta, gamma"]"#)
    );
    assert_eq!(member(&json, "XMP:XMP-dc:Title"), Some("\"T\""));
}

#[test]
fn flat_output_keeps_equal_repeated_list_items() {
    let path = write_fixture("flat.DNG", &dng_with_xmp(LIGHTROOM_XMP.as_bytes()));
    let json = cli_json(&["-j", "-a", "-G0:1:4", "-n"], &path);
    assert_eq!(
        member(&json, "XMP:XMP-crs:MaskGroupBasedCorrectionsWhat"),
        Some(r#"["Correction","Correction"]"#)
    );
    assert_eq!(
        member(
            &json,
            "XMP:XMP-crs:MaskGroupBasedCorrectionsCorrectionMasksMaskValue"
        ),
        Some("[1,0.5,0.7]")
    );
    assert_eq!(member(&json, "XMP:XMP-dc:Creator"), Some("\"one\""));
    assert!(member(&json, "XMP:XMP-crs:Look").is_none());
}

#[test]
fn library_exposes_struct_values() {
    let path = write_fixture("lib.DNG", &dng_with_xmp(LIGHTROOM_XMP.as_bytes()));
    let et = ExifTool::with_options(Options {
        struct_output: true,
        duplicates: true,
        print_conv: false,
        ..Options::default()
    });
    let tags = et.extract_info(&path).unwrap();
    let look = tags
        .iter()
        .find(|t| t.name == "Look")
        .expect("Look struct tag");
    assert_eq!(look.group.family1, "XMP-crs");
    let Value::Struct(fields) = &look.raw_value else {
        panic!("not a struct: {:?}", look.raw_value)
    };
    let keys: Vec<&str> = fields.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["Amount", "Group", "Group-de", "Name", "Parameters"]);
    assert!(
        look.print_value
            .starts_with("{Amount=1.000000,Group=Profiles,"),
        "{}",
        look.print_value
    );
    assert!(tags.iter().all(|t| t.name != "LookName"));
    let masks = tags
        .iter()
        .find(|t| t.name == "MaskGroupBasedCorrections")
        .unwrap();
    assert!(masks.raw_value.has_struct());
    // Without the option nothing changes.
    let flat = ExifTool::new().extract_info(&path).unwrap();
    assert!(flat.iter().all(|t| !t.raw_value.has_struct()));
    assert!(flat.iter().any(|t| t.name == "LookName"));
}

#[test]
fn copy_numbers_and_binary_base64_follow_exiftool() {
    let path = write_fixture("copies.DNG", &dng_with_xmp(LIGHTROOM_XMP.as_bytes()));
    let json = cli_json(&["-j", "-a", "-G0:1:4", "-n", "-b"], &path);
    // Every key is unique and the second same-named tag carries Copy1.
    let keys: Vec<&str> = json
        .lines()
        .filter_map(|l| l.trim_start().strip_prefix('"'))
        .filter_map(|l| l.split("\": ").next())
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), keys.len(), "duplicate keys in {json}");
    let versions: Vec<&&str> = keys.iter().filter(|k| k.ends_with(":Version")).collect();
    assert!(
        versions.iter().any(|k| k.contains(":Copy1:")) || versions.len() < 2,
        "{versions:?}"
    );
    let plain = cli_json(&["-j", "-G0:1"], &path);
    assert!(plain.lines().all(|l| !l.contains("base64:")));
}
