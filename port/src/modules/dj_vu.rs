//! Image::ExifTool::DjVu (Image/ExifTool/DjVu.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read DjVu meta information: the annotation chunks (ANTa, and ANTz once
//! BZZ-decoded) and their metadata entries.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, perl_eval, perl_eval1};
use crate::perl::regex as pre;
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};
use crate::runtime;

/// `pos($$dataPt)`
fn get_pos(it: &mut Interp, data_pt: &Value) -> ER<Option<usize>> {
    let d = cell(data_pt.clone());
    let v = perl_eval1(it, "pos($$d)", "main", &[("d", &d)])?;
    Ok(if v.is_undef() { None } else { Some(v.to_i64() as usize) })
}

/// `pos($$dataPt) = POS` (undef resets it, as a failed `//g` match does).
fn set_pos(it: &mut Interp, data_pt: &Value, p: Option<usize>) -> ER<()> {
    let d = cell(data_pt.clone());
    let v = cell(match p {
        Some(p) => n(p as i64),
        None => Value::Undef,
    });
    perl_eval(it, "pos($$d) = $p", "main", &[("d", &d), ("p", &v)], Want::Void)?;
    Ok(())
}

/// A scalar `$str =~ /PAT/g` from pos() `p`: the captures on a match (and
/// `p` set to its end); on failure `p` becomes None, as perl resets pos().
fn match_g(pat: &str, flags: &str, subject: &[u8], p: &mut Option<usize>) -> Option<pl::Caps> {
    let rx = pre::compile(pat.as_bytes(), flags).unwrap_or_else(|e| panic!("bad pattern /{pat}/: {e}"));
    let start = p.unwrap_or(0);
    let r = if start > subject.len() { Ok(None) } else { rx.match_at(subject, start) };
    match r {
        Ok(Some(sp)) => {
            *p = Some(sp[0].unwrap().1);
            Some(sp.iter().map(|g| g.map(|(a, b)| Value::bytes(&subject[a..b]))).collect())
        }
        _ => {
            *p = None;
            None
        }
    }
}

//------------------------------------------------------------------------------
// Parse DjVu annotation "s-expression" syntax (recursively)
// Inputs: 0) data ref (with pos($$dataPt) set to start of annotation)
// Returns: reference to list of tokens/references, or undef if no tokens,
//          and the position in $$dataPt is set to end of last token
// Notes: The DjVu annotation syntax is not well documented, so I make
//        a number of assumptions here!
/// (`pos` is pos($$dataPt), in and out.)
fn parse_ant_core(data: &[u8], pos: &mut Option<usize>) -> Value {
    let mut toks: Vec<Value> = Vec::new();
    let mut tok: Value;
    let mut more = false;
    // (the DjVu annotation syntax really sucks, and requires that every
    // single token be parsed in order to properly scan through the items)
    'tok: loop {
        // find the next token
        let Some(c) = match_g(r"(\S)", "s", data, pos) else { break }; // get next non-space character
        let c1 = cap(&c, 1);
        if pl::str_eq(&c1, "(") {
            // start of list
            tok = parse_ant_core(data, pos);
        } else if pl::str_eq(&c1, ")") {
            // end of list
            more = true;
            break;
        } else if pl::str_eq(&c1, "\"") {
            // quoted string
            tok = pl::s("");
            loop {
                // get string up to the next quotation mark
                // this doesn't work in perl 5.6.2! grrrr
                // last Tok unless $$dataPt =~ /(.*?)"/sg;
                // $tok .= $1;
                let p0 = pos.unwrap_or(0);
                if match_g("\"", "s", data, pos).is_none() {
                    break 'tok;
                }
                let p1 = pos.unwrap_or(0);
                tok = pl::concat(&tok, &pl::substr(&Value::bytes(data), p0 as i64, Some(p1 as i64 - 1 - p0 as i64)));
                // we're good unless quote was escaped by odd number of backslashes
                let odd = match re(r"(\\+)$", "").m(&tok) {
                    Some(m) => cap(&m, 1).to_bytes().len() & 0x01 != 0,
                    None => false,
                };
                if !odd {
                    break;
                }
                tok = pl::concat(&tok, &pl::s("\"")); // quote is part of the string
            }
            // convert C escape sequences, allowed in quoted text
            // (note: this only converts a few of them!)
            let esc = |ch: &[u8]| -> Option<&'static [u8]> {
                Some(match ch {
                    b"a" => b"\x07",
                    b"b" => b"\x08",
                    b"f" => b"\x0c",
                    b"n" => b"\n",
                    b"r" => b"\r",
                    b"t" => b"\t",
                    b"\"" => b"\"",
                    b"\\" => b"\\",
                    _ => return None,
                })
            };
            re(r"\\(.)", "s").subst(&mut tok, true, |m| {
                let ch = cap(m, 1).to_bytes();
                match esc(&ch) {
                    Some(e) => e.to_vec(),
                    None => [&b"\\"[..], &ch[..]].concat(),
                }
            });
        } else {
            // key name
            *pos = Some(pos.unwrap_or(0) - 1);
            // allow anything in key but whitespace, braces and double quotes
            // (this is one of those assumptions I mentioned)
            tok = match match_g(r#"([^\s()"]+)"#, "s", data, pos) {
                Some(m) => cap(&m, 1),
                None => Value::Undef,
            };
        }
        if !tok.is_undef() {
            toks.push(tok.clone());
        }
    }
    // prevent further parsing unless more after this
    if !more {
        *pos = Some(data.len());
    }
    if !toks.is_empty() {
        Value::new_array(toks)
    } else {
        Value::Undef
    }
}

pub fn parse_ant(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let data_pt = arg(&a, 0);
    let mut pos = get_pos(it, &data_pt)?;
    let data = data_pt.deref().to_bytes();
    let r = parse_ant_core(&data, &mut pos);
    set_pos(it, &data_pt, pos)?;
    ret(r)
}

//------------------------------------------------------------------------------
// Process DjVu annotation chunk (ANTa or decoded ANTz)
// Inputs: 0) ExifTool object reference, 1) DirInfo reference, 2) tag table ref
// Returns: 1 on success
pub fn process_ant(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");

    // quick pre-scan to check for metadata or XMP
    if !re(r#"\(\s*(metadata|xmp)[\s("]"#, "s").is_match(&data_pt.deref()) {
        return ret(n(1));
    }

    // parse annotations into a tree structure
    let mut pos = Some(0);
    let data = data_pt.deref().to_bytes();
    let toks = parse_ant_core(&data, &mut pos);
    set_pos(it, &data_pt, pos)?;
    if !toks.truthy() {
        return ret(n(0));
    }

    // process annotations individually
    for ant in toks.items() {
        if !(ant.is_array() && ant.len() >= 2) {
            continue;
        }
        let tag = shift_array(&ant);
        if tag.is_ref() || tag_table_ptr.get(tag.to_bytes()).is_undef() {
            continue;
        }
        if pl::str_eq(&tag, "metadata") {
            // ProcessMeta() takes array reference
            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag, ant.clone()])?;
        } else {
            if ant.at(0).is_ref() {
                continue; // only process simple values
            }
            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag, ant.at(0)])?;
        }
    }
    ret(n(1))
}

/// `shift @$aref`
fn shift_array(a: &Value) -> Value {
    match a {
        Value::ArrayRef(v) => {
            let mut v = v.borrow_mut();
            if v.is_empty() {
                Value::Undef
            } else {
                v.remove(0)
            }
        }
        _ => Value::Undef,
    }
}

//------------------------------------------------------------------------------
// Process DjVu metadata
// Inputs: 0) ExifTool object reference, 1) DirInfo reference, 2) tag table ref
// Returns: 1 on success
// Notes: input dirInfo DataPt is a reference to a list of pre-parsed metadata entries
pub fn process_meta(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let list = data_pt.deref();
    if pl::ref_of(&list) != "ARRAY" {
        return ret(n(0));
    }
    pl::method(it, &et, "VerboseDir", vec![pl::s("Metadata"), n(list.len() as i64)])?;
    let mut err = false;
    for item in list.items() {
        // make sure item is a simple tag/value pair
        if !(pl::ref_of(&item) == "ARRAY" && item.len() >= 2 && !item.at(0).is_ref() && !item.at(1).is_ref()) {
            err = true;
            continue;
        }
        // add any new tags to the table
        if !tag_table_ptr.get(item.at(0).to_bytes()).truthy() {
            let mut name = item.at(0);
            pl::tr(&mut name, "-_a-zA-Z0-9", "", "dc"); // remove illegal characters
            if pl::length(&name).to_i64() == 0 {
                err = true;
                continue;
            }
            let info = Value::new_hash();
            info.set("Name", pl::ucfirst(&name));
            pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), item.at(0), info])?;
        }
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), item.at(0), item.at(1)])?;
    }
    if err {
        pl::method(it, &et, "Warn", vec![pl::s("Ignored invalid metadata entry(s)")])?;
    }
    ret(n(1))
}

//------------------------------------------------------------------------------
// Process BZZ-compressed data (in DjVu images)
// Inputs: 0) ExifTool object reference, 1) DirInfo reference, 2) tag table ref
// Returns: 1 on success
pub fn process_bzz(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    runtime::require(it, "Image::ExifTool::BZZ")?;
    let buff = pl::call(it, "Image::ExifTool::BZZ::Decode", vec![dir_info.get("DataPt")])?;
    if buff.is_undef() {
        let msg = pl::concat(&pl::s("Error decoding "), &dir_info.get("DirName"));
        pl::method(it, &et, "Warn", vec![msg])?;
        return ret(n(0));
    }
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let buff_ref = pl::sref(buff.clone());
    if pl::ge(&verbose, &n(3)) {
        // dump the decoded data in very verbose mode
        let msg = pl::concat(&pl::s("Decoded "), &dir_info.get("DirName"));
        pl::method(it, &et, "VerboseDir", vec![msg, n(0), pl::length(&buff)])?;
        pl::method(it, &et, "VerboseDump", vec![buff_ref.clone()])?;
    }
    dir_info.set("DataPt", buff_ref);
    let len = pl::length(&buff);
    dir_info.set("DirLen", len.clone());
    dir_info.set("DataLen", len);
    // process the data using the default process proc for this table
    let process_proc = tag_table_ptr.get("PROCESS_PROC");
    if !process_proc.truthy() {
        return ret(n(0));
    }
    pl::call_code(it, &process_proc, vec![et, dir_info, tag_table_ptr], want)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::DjVu::ParseAnt", parse_ant);
    it.define_native("Image::ExifTool::DjVu::ProcessAnt", process_ant);
    it.define_native("Image::ExifTool::DjVu::ProcessMeta", process_meta);
    it.define_native("Image::ExifTool::DjVu::ProcessBZZ", process_bzz);
}
