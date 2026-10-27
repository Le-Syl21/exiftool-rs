//! Image::ExifTool::Text (Image/ExifTool/Text.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Deduce some characteristics of TXT and CSV files.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{raf_read, raf_read_line};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

fn s(x: &str) -> Value {
    pl::s(x)
}

fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

fn handle_tag(it: &mut Interp, et: &Value, table: &Value, tag: &str, val: Value) -> ER<()> {
    pl::method(it, et, "HandleTag", vec![table.clone(), s(tag), val])?;
    Ok(())
}

/// The pattern `/(^|$delim)(["'])(.*?)\2(?=$delim|$)/` with `$delim`
/// interpolated as is (it is one of ',', ';' or "\t").
fn quote_re(delim: &Value) -> ER<crate::pl::Re> {
    let d = delim.to_bytes();
    let mut p = b"(^|".to_vec();
    p.extend_from_slice(&d);
    p.extend_from_slice(br#")(["'])(.*?)\2(?="#);
    p.extend_from_slice(&d);
    p.extend_from_slice(b"|$)");
    pl::re_dyn(&p, "s")
}

//------------------------------------------------------------------------------
// Extract some stats from a text file
// Inputs: 0) ExifTool ref, 1) dirInfo ref
// Returns: 1 on success, 0 if this wasn't a text file
pub fn process_txt(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let mut data_pt = dir_info.get("TestBuff");
    let raf = dir_info.get("RAF");
    let f = pl::method(it, &et, "Options", vec![s("FastScan")])?;
    let fast = if f.truthy() { f } else { n(0) };
    let mut buff = Value::Undef;
    let mut enc: Value;
    let (mut is_bom, mut is_utf8) = (Value::Undef, Value::Undef);
    let mut nl = s("");

    if !pl::length(&data_pt.deref()).truthy() {
        return ret(n(0)); // can't call it a text file if it has no text
    }

    // read more from the file if necessary
    let test_len = it.get_scalar("Image::ExifTool::testLen");
    if pl::lt(&fast, &n(3)) && pl::num_eq(&pl::length(&data_pt.deref()), &test_len) {
        if !raf_read(it, &raf, &mut buff, n(65536))?.truthy() {
            return ret(n(0));
        }
        data_pt = pl::sref(buff.clone());
    }
    let data = data_pt.deref();
    //
    // make our best guess at the character encoding (EBCDIC is not supported)
    //
    if re(r"([\0-\x06\x0e-\x1a\x1c-\x1f\x7f])", "").is_match(&data) {
        // file contains weird control characters, could be multi-byte Unicode
        if let Some(m) = re(r"^(\xff\xfe\0\0|\0\0\xfe\xff)", "").m(&data) {
            if cap(&m, 1).to_bytes()[..] == b"\xff\xfe\0\0"[..] {
                enc = s("utf-32le");
                if let Some(m) = re(r"(\r\0\0\0\n|\r|\n)\0\0\0", "").m(&data) {
                    nl = cap(&m, 1);
                }
            } else {
                enc = s("utf-32be");
                if let Some(m) = re(r"\0\0\0(\r\0\0\0\n|\r|\n)", "").m(&data) {
                    nl = cap(&m, 1);
                }
            }
        } else if let Some(m) = re(r"^(\xff\xfe|\xfe\xff)", "").m(&data) {
            if cap(&m, 1).to_bytes()[..] == b"\xff\xfe"[..] {
                enc = s("utf-16le");
                if let Some(m) = re(r"(\r\0\n|\r|\n)\0", "").m(&data) {
                    nl = cap(&m, 1);
                }
            } else {
                enc = s("utf-16be");
                if let Some(m) = re(r"\0(\r\0\n|\r|\n)", "").m(&data) {
                    nl = cap(&m, 1);
                }
            }
        } else {
            return ret(n(0)); // probably not a text file
        }
        pl::tr(&mut nl, "\0", "", "d"); // remove nulls from newline sequence
        is_bom = n(1); // (we don't recognize UTF-16/UTF-32 without one)
    } else {
        is_utf8 = call(it, "IsUTF8", vec![data_pt.clone(), n(1)])?;
        if pl::num_eq(&is_utf8, &n(0)) {
            enc = s("us-ascii");
        } else if pl::gt(&is_utf8, &n(0)) {
            enc = s("utf-8");
            is_bom = n(if re(r"^\xef\xbb\xbf", "").is_match(&data) { 1 } else { 0 });
        } else if !re(r"[\x80-\x9f]", "").is_match(&data) {
            enc = s("iso-8859-1");
        } else {
            enc = s("unknown-8bit");
        }
        if let Some(m) = re(r"(\r\n|\r|\n)", "").m(&data) {
            nl = cap(&m, 1);
        }
    }

    let tag_table_ptr = call(it, "GetTagTable", vec![s("Image::ExifTool::Text::Main")])?;

    pl::method(it, &et, "SetFileType", vec![])?;
    handle_tag(it, &et, &tag_table_ptr, "MIMEEncoding", enc.clone())?;

    if pl::num_eq(&fast, &n(3)) || !pl::method(it, &raf, "Seek", vec![n(0), n(0)])?.truthy() {
        return ret(n(1));
    }

    if !is_bom.is_undef() {
        handle_tag(it, &et, &tag_table_ptr, "ByteOrderMark", is_bom.clone())?;
    }
    handle_tag(it, &et, &tag_table_ptr, "Newlines", nl.clone())?;

    if fast.truthy() || is_utf8.is_undef() {
        return ret(n(1));
    }
    //
    // generate stats for CSV files
    //
    if pl::str_eq(&et.get("FileType"), "CSV") {
        let (mut delim, mut quot, mut ncols) = (Value::Undef, Value::Undef, Value::Undef);
        let mut nrows = n(0);
        while raf_read_line(it, &raf, &mut buff)?.truthy() {
            if delim.is_undef() {
                // (%count: ',', ';' and "\t")
                let mut count = [n(0), n(0), n(0)];
                let ci = |d: &[u8]| -> usize {
                    match d {
                        b"," => 0,
                        b";" => 1,
                        _ => 2,
                    }
                };
                for c in re(r"[,;\t]", "").all(&buff) {
                    let i = ci(&c.to_bytes());
                    count[i] = pl::add(&count[i], &n(1));
                }
                if pl::gt(&count[0], &count[1]) && pl::gt(&count[0], &count[2]) {
                    delim = s(",");
                } else if pl::gt(&count[1], &count[2]) {
                    delim = s(";");
                } else if count[2].truthy() {
                    delim = s("\t");
                } else {
                    delim = s("");
                    ncols = n(1);
                }
                if !ncols.truthy() {
                    // account for delimiters in quotes (simplistically)
                    let qre = quote_re(&delim)?;
                    let dre = pl::re_dyn(&delim.to_bytes(), "")?;
                    let i = ci(&delim.to_bytes());
                    for c in qre.each(&buff) {
                        quot = cap(&c, 2);
                        let field = cap(&c, 3);
                        let k = dre.all(&field).len() as i64;
                        count[i] = pl::sub(&count[i], &n(k));
                    }
                    ncols = pl::add(&count[i], &n(1));
                }
            } else if !quot.truthy() {
                let qre = quote_re(&delim)?;
                if let Some(m) = qre.m(&buff) {
                    quot = cap(&m, 2);
                }
            }
            nrows = pl::add(&nrows, &n(1));
            if pl::num_eq(&nrows, &n(1000)) && pl::method(it, &et, "Warn", vec![s("Not counting rows past 1000"), n(2)])?.truthy() {
                nrows = Value::Undef;
                break;
            }
        }
        handle_tag(it, &et, &tag_table_ptr, "Delimiter", if delim.truthy() { delim } else { s("") })?;
        handle_tag(it, &et, &tag_table_ptr, "Quoting", if quot.truthy() { quot } else { s("") })?;
        handle_tag(it, &et, &tag_table_ptr, "ColumnCount", ncols)?;
        if nrows.truthy() {
            handle_tag(it, &et, &tag_table_ptr, "RowCount", nrows)?;
        }
        return ret(n(1));
    }
    let fs = et.path(&["VALUE", "FileSize"]);
    if fs.truthy() && pl::gt(&fs, &n(20000000)) && pl::method(it, &et, "Warn", vec![s("Not counting lines/words in text file larger than 20 MB"), n(2)])?.truthy() {
        return ret(n(1));
    }
    //
    // count lines/words and check encoding of the rest of the file
    //
    let (mut lines, mut words) = (n(0), n(0));
    let old_nl = it.get_scalar("main::/");
    if nl.truthy() {
        it.set_scalar("main::/", nl.clone());
    }
    while raf_read_line(it, &raf, &mut buff)?.truthy() {
        lines = pl::add(&lines, &n(1));
        let k = re(r"\S+", "").all(&buff).len() as i64;
        words = pl::add(&words, &n(k));
        if !nl.truthy() {
            if let Some(m) = re(r"(\r\n|\r|\n)$", "").m(&buff) {
                // (the first line must have been longer than 64 kB)
                nl = cap(&m, 1);
                et.get_or_new_hash("VALUE").set("Newlines", nl.clone());
            }
        }
        if pl::lt(&pl::method(it, &raf, "Tell", vec![])?, &n(65536)) {
            continue;
        }
        // continue to check encoding after the first 64 kB
        if pl::ge(&is_utf8, &n(0)) {
            // (if ascii or utf8)
            is_utf8 = call(it, "IsUTF8", vec![pl::sref(buff.clone())])?;
            if pl::gt(&is_utf8, &n(0)) {
                enc = s("utf-8");
            } else if pl::lt(&is_utf8, &n(0)) {
                enc = s(if re(r"[\x80-\x9f]", "").is_match(&buff) { "unknown-8bit" } else { "iso-8859-1" });
            }
        } else if pl::str_eq(&enc, "iso-8859-1") && re(r"[\x80-\x9f]", "").is_match(&buff) {
            enc = s("unknown-8bit");
        }
    }
    // ($$et{VALUE}{MIMEEncoding} may be undef if it was ignored)
    let me = et.path(&["VALUE", "MIMEEncoding"]);
    if !me.is_undef() && me.to_bytes() != enc.to_bytes() {
        et.get_or_new_hash("VALUE").set("MIMEEncoding", enc.clone());
        let msg = pl::concat(&pl::concat(&s("  MIMEEncoding [override] = "), &enc), &s("\n"));
        pl::method(it, &et, "VPrint", vec![n(0), msg])?;
    }
    it.set_scalar("main::/", old_nl);
    handle_tag(it, &et, &tag_table_ptr, "LineCount", lines)?;
    handle_tag(it, &et, &tag_table_ptr, "WordCount", words)?;
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Text::ProcessTXT", process_txt);
}
