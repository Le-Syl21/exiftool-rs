//! The languages exiftool-rs adds to ExifTool's (ar, bn, hi, pt: Lang
//! modules made from the root crate's locales by scripts/gen_extra_langs.pl),
//! through the exiftool application (`-lang`) and the facade
//! (`Options::lang`). ExifTool's own languages are judged against Perl by
//! scripts/compare_cli.py.

use std::path::Path;

use exiftool_rs::interp::{Binding, Flow, Want};
use exiftool_rs::perl::Value;
use exiftool_rs::{ExifTool, Options};

const CANON: &str = "tests/images/Canon.jpg";

/// Run the exiftool application on `args`; returns its output and its
/// exit code.
fn exiftool_rs(args: &[&str]) -> (String, i32) {
    let mut it = exiftool_rs::runtime::new_interp();
    it.strict = false;
    it.set_scalar("main::0", Value::str("exiftool-rs"));
    let argv: Vec<Value> = args.iter().map(|a| Value::str(a)).collect();
    it.set_global(b'@', "main::ARGV", Binding::Array(std::rc::Rc::new(std::cell::RefCell::new(argv))));
    exiftool_rs::cli::register(&mut it);
    match it.call_named("main::<program>", vec![], Want::Void) {
        Ok(_) => {}
        Err(Flow::Exit(code)) => it.exit_code = Some(code),
        Err(Flow::Die(e)) => panic!("died: {}", e.to_text()),
        Err(_) => {}
    }
    let _ = it.run_end_blocks();
    it.flush_handles();
    let mut out = String::from_utf8_lossy(&it.stdout).into_owned();
    out.push_str(&String::from_utf8_lossy(&it.stderr));
    (out, it.exit_code.unwrap_or(0))
}

#[test]
fn lang_prints_descriptions_in_the_added_languages() {
    let cases = [
        ("pt", "Fabricante da Câmera", "Modelo da Câmera"),
        ("ar", "صانع الكاميرا", "موديل الكاميرا"),
        ("bn", "ক্যামেরা নির্মাতা", "ক্যামেরা মডেল"),
        ("hi", "कैमरा निर्माता", "कैमरा मॉडल"),
    ];
    for (lang, make, model) in cases {
        let (out, code) = exiftool_rs(&["-lang", lang, "-Make", "-Model", CANON]);
        assert_eq!(code, 0, "{lang}: {out}");
        let lines: Vec<(&str, &str)> = out.lines().filter_map(|l| l.split_once(": ")).map(|(d, v)| (d.trim_end(), v)).collect();
        assert_eq!(lines, [(make, "Canon"), (model, "Canon EOS DIGITAL REBEL")], "{lang}");
    }
    // the language code as -lang takes it (upper case, dash)
    let (out, _) = exiftool_rs(&["-lang", "PT", "-Make", CANON]);
    assert!(out.starts_with("Fabricante da Câmera"), "{out}");
    // -X: the descriptions in the RDF too
    let (out, _) = exiftool_rs(&["-lang", "pt", "-X", "-Make", CANON]);
    assert!(out.contains("<IFD0:Make>Canon</IFD0:Make>") && out.contains("<rdf:Description"), "{out}");
}

#[test]
fn zh_is_simplified_chinese() {
    let (zh, _) = exiftool_rs(&["-lang", "zh", "-Make", "-Flash", CANON]);
    let (zh_cn, _) = exiftool_rs(&["-lang", "zh_cn", "-Make", "-Flash", CANON]);
    assert_eq!(zh, zh_cn);
    assert!(zh.starts_with("厂商"), "{zh}");
}

#[test]
fn the_language_list_stays_exiftools() {
    // (as Perl's -lang prints it, and -listx: the added languages are not in
    // @Image::ExifTool::langs)
    let (out, _) = exiftool_rs(&["-lang"]);
    assert!(out.starts_with("Available languages:\n  cs - Czech"), "{out}");
    assert!(!out.contains("pt - ") && !out.contains("ar - "), "{out}");
    assert_eq!(out.lines().count(), 20, "{out}");
    // an unknown language is still refused
    let (out, code) = exiftool_rs(&["-lang", "xx", "-Make", CANON]);
    assert!(out.starts_with("Invalid or unsupported language 'xx'."), "{out}");
    assert_ne!(code, 0);
}

fn description(et: &ExifTool, name: &str) -> (String, String) {
    let tags = et.extract_info(CANON).unwrap();
    let tag = tags.iter().find(|t| t.name == name).unwrap_or_else(|| panic!("no {name}"));
    (tag.description.clone(), tag.print_value.clone())
}

fn with_lang(lang: Option<&str>) -> ExifTool {
    ExifTool::with_options(Options {
        lang: lang.map(str::to_string),
        ..Options::default()
    })
}

#[test]
fn options_lang_is_exiftools_lang() {
    assert_eq!(description(&with_lang(None), "Flash"), ("Flash".into(), "No Flash".into()));
    assert_eq!(description(&with_lang(Some("en")), "Orientation"), ("Orientation".into(), "Horizontal (normal)".into()));
    // ExifTool's French: descriptions and values (`exiftool -lang fr`)
    let fr = with_lang(Some("fr"));
    assert_eq!(description(&fr, "Make"), ("Fabricant".into(), "Canon".into()));
    // (fr.pm's description has a trailing space)
    assert_eq!(description(&fr, "Flash"), ("Flash ".into(), "Flash non déclenché".into()));
    assert_eq!(description(&fr, "Orientation"), ("Orientation de l'image".into(), "Horizontale (normale)".into()));
    // zh, zh-CN and zh_cn are the same
    for code in ["zh", "zh-CN", "zh_cn"] {
        assert_eq!(description(&with_lang(Some(code)), "Make").0, "厂商", "{code}");
    }
    // an added language
    assert_eq!(description(&with_lang(Some("pt")), "Model").0, "Modelo da Câmera");
    assert_eq!(description(&with_lang(Some("ar")), "Make").0, "صانع الكاميرا");
    // a language ExifTool does not have is ignored, as by ExifTool
    assert_eq!(description(&with_lang(Some("xx")), "Make").0, "Make");
}

#[test]
fn a_write_takes_values_in_the_language() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("extra_langs");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("fr.jpg");
    let mut et = with_lang(Some("fr"));
    et.set_new_value("Orientation", Some("Rotation antihoraire de 90°"));
    assert_eq!(et.write_info(CANON, &out).unwrap(), 1);
    let tags = ExifTool::new().extract_info(&out).unwrap();
    assert_eq!(tags.iter().find(|t| t.name == "Orientation").map(|t| t.print_value.as_str()), Some("Rotate 90 CW"));
    // and in place, read back in the language
    et.clear_new_values();
    et.set_new_value("Orientation", Some("Horizontale (normale)"));
    assert_eq!(et.write_info(&out, &out).unwrap(), 1);
    let tags = ExifTool::new().extract_info(&out).unwrap();
    assert_eq!(tags.iter().find(|t| t.name == "Orientation").map(|t| t.print_value.as_str()), Some("Horizontal (normal)"));
}
