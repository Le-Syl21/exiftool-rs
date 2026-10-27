//! Image::ExifTool::Import (Image/ExifTool/Import.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Import CSV and JSON database files

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, perl_eval, perl_eval1};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret, s};
use crate::runtime;

const MODULE: &str = "Image::ExifTool::Import";

/// `quotemeta`
fn quotemeta(v: &Value) -> Vec<u8> {
    crate::perl::regex::quotemeta(&v.to_bytes())
}

/// `$raf->Read($buff, $len)`: the count read; `buff` gets the data.
fn raf_read(it: &mut Interp, raf: &Value, buff: &mut Value, len: i64) -> ER<Value> {
    let mut args = [buff.clone(), n(len)];
    let r = pl::method_mut(it, raf, "Read", &mut args, Want::Scalar)?;
    *buff = args[0].clone();
    Ok(r.last().cloned().unwrap_or_default())
}

/// `$raf->ReadLine($buff)`
fn raf_read_line(it: &mut Interp, raf: &Value, buff: &mut Value) -> ER<Value> {
    let mut args = [buff.clone()];
    let r = pl::method_mut(it, raf, "ReadLine", &mut args, Want::Scalar)?;
    *buff = args[0].clone();
    Ok(r.last().cloned().unwrap_or_default())
}

/// `open NAME, $file or ...; binmode NAME; File::RandomAccess->new(\*NAME)`:
/// the RAF, or None when the open failed.
fn open_raf(it: &mut Interp, handle: &str, file: &Value) -> ER<Option<Value>> {
    let f = cell(file.clone());
    let code = format!("open({handle}, $f) ? do {{ binmode {handle}; File::RandomAccess->new(\\*{handle}) }} : undef");
    let raf = perl_eval1(it, &code, MODULE, &[("f", &f)])?;
    Ok(if raf.is_undef() { None } else { Some(raf) })
}

/// `close NAME`
fn close_handle(it: &mut Interp, handle: &str) -> ER<()> {
    perl_eval(it, &format!("close {handle}"), MODULE, &[], Want::Void)?;
    Ok(())
}

/// `ref $v` as a value.
fn ref_is(v: &Value, t: &str) -> bool {
    pl::ref_of(v) == t
}

//------------------------------------------------------------------------------
/// Read CSV file
/// Inputs: 0) CSV file name, file ref or RAF ref, 1) database hash ref,
///         2) missing tag value, 3) delimiter if other than ','
/// Returns: undef on success, or error string
/// Notes: There are various flavours of CSV, but here we assume that only
///        double quotes are escaped, and they are escaped by doubling them
pub fn read_csv(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    // local ($_, $/);
    pl::local_topic(it, |it| pl::local_global(it, b'$', "main::/", |it| read_csv_body(it, a)))
}

fn read_csv_body(it: &mut Interp, a: Vec<Value>) -> ER<Vec<Value>> {
    let (mut file, database, missing_value, mut delim) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    let mut buff = Value::Undef;
    let mut tags: Vec<Value> = Vec::new();
    let mut found = false;
    let mut err = Value::Undef;
    let raf;
    let mut opened_file = false;

    if pl::call(it, "UNIVERSAL::isa", vec![file.clone(), s("File::RandomAccess")])?.truthy() {
        raf = file.clone();
        file = s("CSV file");
    } else if ref_is(&file, "GLOB") {
        raf = pl::call(it, "File::RandomAccess::new", vec![s("File::RandomAccess"), file.clone()])?;
        file = s("CSV file");
    } else {
        match open_raf(it, "CSVFILE", &file)? {
            Some(r) => raf = r,
            None => return ret(s(&format!("Error opening CSV file '{}'", file.to_text()))),
        }
        opened_file = true;
    }
    if delim.is_undef() {
        delim = s(",");
    }
    // set input record separator by first newline found in the file
    // (safe because first line should contain only tag names)
    while raf_read(it, &raf, &mut buff, 65536)?.truthy() {
        if let Some(c) = re(r"(\x0d\x0a|\x0d|\x0a)", "").m(&buff) {
            it.set_scalar("main::/", cap(&c, 1));
            break;
        }
    }
    pl::method(it, &raf, "Seek", vec![n(0), n(0)])?;
    let delim_re = pl::re_dyn(&quotemeta(&delim), "")?;
    while raf_read_line(it, &raf, &mut buff)?.truthy() {
        let mut vals: Vec<Value> = Vec::new();
        let file_info = Value::new_hash();
        let mut toks = pl::split(&delim_re, &buff, 0);
        while !toks.is_empty() {
            let mut v = toks.remove(0);
            re(r"^ +", "").subst(&mut v, false, |_| Vec::new()); // remove leading spaces
            if re(r#"^""#, "").subst(&mut v, false, |_| Vec::new()) > 0 {
                // quoted value must end in an odd number of quotes
                loop {
                    let odd = match re(r#"("+)\s*$"#, "").m(&v) {
                        Some(c) => pl::length(&cap(&c, 1)).to_i64() & 1 != 0,
                        None => false,
                    };
                    if odd {
                        break;
                    }
                    if !toks.is_empty() {
                        v = pl::concat(&pl::concat(&v, &delim), &toks.remove(0));
                    } else {
                        // read another line from the file
                        if !raf_read_line(it, &raf, &mut buff)?.truthy() {
                            break;
                        }
                        toks = pl::split(&delim_re, &buff, 0);
                        if toks.is_empty() {
                            break;
                        }
                        v = pl::concat(&v, &toks.remove(0));
                    }
                }
                re(r#""\s*$"#, "").subst(&mut v, false, |_| Vec::new()); // remove trailing quote and whitespace
                re(r#""""#, "").subst(&mut v, true, |_| b"\"".to_vec()); // un-escape quotes
            } else {
                re(r"[ \n\r]+$", "").subst(&mut v, false, |_| Vec::new()); // remove trailing spaces/newlines
            }
            vals.push(v);
        }
        if !tags.is_empty() {
            // save values for each tag
            let ordered = Value::new_array(vec![]);
            file_info.set("_ordered_keys_", ordered.clone());
            let mut i = 0;
            while i < vals.len() && i < tags.len() {
                // ignore empty entries unless missingValue is empty too
                if !(pl::length(&vals[i]).truthy() || (!missing_value.is_undef() && pl::str_eq(&missing_value, ""))) {
                    i += 1;
                    continue;
                }
                // delete tag (set value to undef) if value is same as missing tag
                let v = if !missing_value.is_undef() && vals[i].to_bytes()[..] == missing_value.to_bytes()[..] { Value::Undef } else { vals[i].clone() };
                file_info.set(tags[i].to_bytes(), v);
                ordered.push(tags[i].clone());
                i += 1;
            }
            // figure out the file name to use
            if file_info.get("SourceFile").truthy() {
                database.set(file_info.get("SourceFile").to_bytes(), file_info.clone());
                found = true;
            }
        } else {
            // the first row should be the tag names
            for v in vals.iter_mut() {
                // terminate at first blank tag name (eg. extra comma at end of line)
                if !pl::length(v).truthy() {
                    break;
                }
                if tags.is_empty() {
                    re(r"^\xef\xbb\xbf", "").subst(v, false, |_| Vec::new()); // remove UTF-8 BOM if it exists
                }
                if !re(r"^([-_0-9A-Z]+:)*[-_0-9A-Z]+#?$", "i").is_match(v) {
                    if re(r"\0", "").is_match(v) {
                        err = s(&format!("Format error in {} (doesn't look like UTF-8)", file.to_text()));
                        break;
                    }
                    err = pl::concat(&pl::concat(&pl::concat(&s("Invalid tag name '"), v), &s("' in ")), &file);
                    break;
                }
                tags.push(v.clone());
            }
            if err.truthy() {
                break;
            }
            if tags.is_empty() {
                err = s(&format!("No tags found in {}", file.to_text()));
                break;
            }
            // fix "SourceFile" case if necessary
            if pl::str_eq(&pl::lc(&tags[0]), "sourcefile") {
                tags[0] = s("SourceFile");
            }
        }
    }
    if opened_file {
        close_handle(it, "CSVFILE")?;
    }
    drop(raf);
    if !(found || err.truthy()) {
        err = pl::concat(&s("No SourceFile column in "), &file);
    }
    ret(if err.truthy() { err } else { Value::Undef })
}

//------------------------------------------------------------------------------
/// Convert unicode code point to UTF-8
/// Inputs: 0) integer Unicode character
/// Returns: UTF-8 bytes
pub fn to_utf8(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    runtime::require(it, "Image::ExifTool::Charset")?;
    let charset = runtime::lex_scalar(it, MODULE, "charset").borrow().clone();
    ret(pl::call(it, "Image::ExifTool::Charset::Recompose", vec![Value::Undef, Value::new_array(vec![arg(&a, 0)]), charset])?)
}

/// A snippet on `$$b` (the JSON buffer) for the operations that keep
/// pos() on the caller's scalar.
fn on_buff(it: &mut Interp, code: &str, b: &Cell) -> ER<Vec<Value>> {
    perl_eval(it, code, MODULE, &[("b", b)], Want::List)
}

/// `pos $$buffPt`
fn pos_of(it: &mut Interp, b: &Cell) -> ER<Value> {
    Ok(on_buff(it, "pos $$b", b)?.pop().unwrap_or_default())
}

/// `$$buffPt =~ /(RE)/g`: `$1`, or None when it fails (pos is then reset).
fn match_g(it: &mut Interp, code: &str, b: &Cell) -> ER<Option<Value>> {
    let r = on_buff(it, code, b)?;
    Ok(r.into_iter().next())
}

/// `$$buffPt` as a value.
fn buff_of(b: &Cell) -> Value {
    b.borrow().deref()
}

//------------------------------------------------------------------------------
/// Read JSON object from file
/// Inputs: 0) RAF reference or undef, 1) optional scalar reference for data
///            to read before reading from file (ie. the file read buffer)
/// Returns: JSON object (scalar, hash ref, or array ref), or undef on EOF or
///          empty object or array (and sets $$buffPt to empty string on EOF)
/// Notes: position in buffer is significant
pub fn read_json_object(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (raf, buff_pt) = (arg(&a, 0), arg(&a, 1));
    ret(read_json_object_rs(it, &raf, buff_pt)?)
}

fn read_json_object_rs(it: &mut Interp, raf: &Value, mut buff_pt: Value) -> ER<Value> {
    // initialize buffer if necessary
    let mut pos;
    let mut read_more = false;
    let mut rtn_val = Value::Undef;
    let mut tok = Value::Undef;
    let mut key = Value::Undef;
    let mut did_bom = false;
    let b: Cell;
    if buff_pt.truthy() {
        b = cell(buff_pt.clone());
        pos = pos_of(it, &b)?;
        if pos.is_undef() {
            on_buff(it, "pos($$b) = 0", &b)?;
            pos = n(0);
        }
    } else {
        buff_pt = pl::sref(s(""));
        b = cell(buff_pt.clone());
        pos = n(0);
    }
    'tok: loop {
        // (didn't spend the time to understand how $pos could be undef, but
        //  put a test here to be safe because one user reported this problem)
        if pos.is_undef() {
            break;
        }
        if pl::ge(&pos, &pl::length(&buff_of(&b))) || read_more {
            if raf.is_undef() {
                break;
            }
            // read another 64kB and add to unparsed data
            let offset = pl::sub(&pl::length(&buff_of(&b)), &pos);
            if offset.truthy() {
                let mut buff = Value::Undef;
                if !raf_read(it, raf, &mut buff, 65536)?.truthy() {
                    pl::set_deref(&buff_pt, s(""))?;
                    break;
                }
                let rest = pl::substr(&buff_of(&b), pos.to_i64(), None);
                pl::set_deref(&buff_pt, pl::concat(&rest, &buff))?;
            } else {
                let mut buff = buff_of(&b);
                let r = raf_read(it, raf, &mut buff, 65536)?;
                pl::set_deref(&buff_pt, buff)?;
                if !r.truthy() {
                    pl::set_deref(&buff_pt, s(""))?;
                    break;
                }
            }
            if !did_bom {
                let mut v = buff_of(&b);
                if re(r"^\xef\xbb\xbf", "").subst(&mut v, false, |_| Vec::new()) > 0 {
                    pl::set_deref(&buff_pt, v)?; // remove UTF-8 BOM if it exists
                }
                did_bom = true;
            }
            on_buff(it, "pos($$b) = 0", &b)?;
            pos = n(0);
            read_more = false;
        }
        if !tok.truthy() {
            // skip white space and find next character
            match match_g(it, r"$$b =~ /(\S)/g ? ($1) : ()", &b)? {
                Some(t) => tok = t,
                None => {
                    pos = pl::length(&buff_of(&b));
                    continue;
                }
            }
            pos = pos_of(it, &b)?;
        }
        // see what type of object this is
        if pl::str_eq(&tok, "{") {
            // object (hash)
            if rtn_val.is_undef() {
                rtn_val = Value::new_hash();
                rtn_val.set("_ordered_keys_", Value::new_array(vec![]));
            }
            loop {
                // read "KEY":"VALUE" pairs
                if key.is_undef() {
                    key = read_json_object_rs(it, raf, buff_pt.clone())?;
                    pos = pos_of(it, &b)?;
                }
                // ($key may be undef for empty JSON object)
                if !key.is_undef() {
                    // scan to delimiting ':'
                    let Some(c) = match_g(it, r"$$b =~ /(\S)/g ? ($1) : ()", &b)? else {
                        read_more = true;
                        continue 'tok;
                    };
                    if !pl::str_eq(&c, ":") {
                        return Ok(Value::Undef); // error if not a colon
                    }
                    let val = read_json_object_rs(it, raf, buff_pt.clone())?;
                    pos = pos_of(it, &b)?;
                    if val.is_undef() {
                        return Ok(Value::Undef);
                    }
                    rtn_val.set(key.to_bytes(), val);
                    rtn_val.get("_ordered_keys_").push(key.clone());
                    key = Value::Undef;
                }
                // scan to delimiting ',' or bounding '}'
                let Some(c) = match_g(it, r"$$b =~ /(\S)/g ? ($1) : ()", &b)? else {
                    read_more = true;
                    continue 'tok;
                };
                if pl::str_eq(&c, "}") {
                    break; // check for end of object
                }
                if !pl::str_eq(&c, ",") {
                    return Ok(Value::Undef); // error if not a comma
                }
            }
        } else if pl::str_eq(&tok, "[") {
            // array
            if rtn_val.is_undef() {
                rtn_val = Value::new_array(vec![]);
            }
            loop {
                let item = read_json_object_rs(it, raf, buff_pt.clone())?;
                pos = pos_of(it, &b)?;
                // ($item may be undef for empty array)
                if !item.is_undef() {
                    rtn_val.push(item);
                }
                // scan to delimiting ',' or bounding ']'
                let Some(c) = match_g(it, r"$$b =~ /(\S)/g ? ($1) : ()", &b)? else {
                    read_more = true;
                    continue 'tok;
                };
                if pl::str_eq(&c, "]") {
                    break; // check for end of array
                }
                if !pl::str_eq(&c, ",") {
                    return Ok(Value::Undef); // error if not a comma
                }
            }
        } else if pl::str_eq(&tok, "\"") {
            // quoted string
            loop {
                let Some(c) = match_g(it, r#"$$b =~ /(\\*)"/g ? ($1) : ()"#, &b)? else {
                    read_more = true;
                    continue 'tok;
                };
                if pl::length(&c).to_i64() & 1 == 0 {
                    break; // check for escaped quote
                }
            }
            let p = pos_of(it, &b)?;
            let len = pl::sub(&pl::sub(&p, &pos), &n(1));
            rtn_val = pl::substr(&buff_of(&b), pos.to_i64(), Some(len.to_i64()));
            // unescape characters
            let mut failed = None;
            re(r"\\u([0-9a-f]{4})", "i").subst(&mut rtn_val, true, |c| {
                let h = u64::from_str_radix(&cap(c, 1).to_text(), 16).unwrap_or(0);
                match to_utf8(it, vec![n(h as i64)], Want::Scalar) {
                    Ok(v) => v.last().map(|v| v.to_bytes().to_vec()).unwrap_or_default(),
                    Err(e) => {
                        failed = Some(e);
                        Vec::new()
                    }
                }
            });
            if let Some(e) = failed {
                return Err(e);
            }
            let unescape_json = runtime::lex_hash(it, MODULE, "unescapeJSON");
            re(r"\\(.)", "s").subst(&mut rtn_val, true, |c| {
                let v = unescape_json.get(cap(c, 1).to_bytes());
                if v.truthy() {
                    v.to_bytes().to_vec()
                } else {
                    cap(c, 1).to_bytes().to_vec()
                }
            });
            // decode base64 (binary data) values
            if re(r"^base64:[A-Za-z0-9+\/]*={0,2}$", "").is_match(&rtn_val) && pl::length(&rtn_val).to_i64() % 4 == 3 {
                runtime::require(it, "Image::ExifTool::XMP")?;
                let r = pl::call(it, "Image::ExifTool::XMP::DecodeBase64", vec![pl::substr(&rtn_val, 7, None)])?;
                rtn_val = r.deref();
            }
        } else if pl::str_eq(&tok, "]") || pl::str_eq(&tok, "}") || pl::str_eq(&tok, ",") {
            // return undef for empty object, array, or list item
            // (empty list item actually not valid JSON)
            on_buff(it, "pos($$b) = pos($$b) - 1", &b)?;
        } else {
            // number, 'true', 'false', 'null'
            if match_g(it, r"$$b =~ /([\s:,\}\]])/g ? ($1) : ()", &b)?.is_none() {
                read_more = true;
                continue;
            }
            on_buff(it, "pos($$b) = pos($$b) - 1", &b)?;
            let p = pos_of(it, &b)?;
            rtn_val = pl::concat(&tok, &pl::substr(&buff_of(&b), pos.to_i64(), Some(pl::sub(&p, &pos).to_i64())));
        }
        break;
    }
    Ok(rtn_val)
}

//------------------------------------------------------------------------------
/// Read JSON file
/// Inputs: 0) JSON file name, file ref, RAF ref or SCALAR ref, 1) database hash ref,
///         2) flag to delete "-" tags, 3) character set
/// Returns: undef on success, or error string
pub fn read_json(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| read_json_body(it, a))
}

fn read_json_body(it: &mut Interp, a: Vec<Value>) -> ER<Vec<Value>> {
    let (mut file, database, missing_value, chset) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    let raf;
    let mut opened_file = false;
    let mut buff = Value::Undef;

    // initialize character set for converting "\uHHHH" chars
    *runtime::lex_scalar(it, MODULE, "charset").borrow_mut() = if chset.truthy() { chset.clone() } else { s("UTF8") };
    if pl::call(it, "UNIVERSAL::isa", vec![file.clone(), s("File::RandomAccess")])?.truthy() {
        raf = file.clone();
        file = s("JSON file");
    } else if ref_is(&file, "GLOB") {
        raf = pl::call(it, "File::RandomAccess::new", vec![s("File::RandomAccess"), file.clone()])?;
        file = s("JSON file");
    } else if ref_is(&file, "SCALAR") {
        raf = pl::call(it, "File::RandomAccess::new", vec![s("File::RandomAccess"), file.clone()])?;
        file = s("JSON in memory");
    } else {
        match open_raf(it, "JSONFILE", &file)? {
            Some(r) => raf = r,
            None => return ret(s(&format!("Error opening JSON file '{}'", file.to_text()))),
        }
        opened_file = true;
    }
    let mut obj = read_json_object_rs(it, &raf, Value::Undef)?;
    if opened_file {
        close_handle(it, "JSONFILE")?;
    }
    if !ref_is(&obj, "ARRAY") {
        if !ref_is(&obj, "HASH") {
            let mut err = pl::concat(&s("Format error in "), &file);
            if pl::method(it, &raf, "Seek", vec![n(0), n(0)])?.truthy() && raf_read(it, &raf, &mut buff, 64)?.truthy() && re(r"\0", "").is_match(&buff) {
                err = pl::concat(&err, &s(" (doesn't look like UTF-8)"));
            }
            return ret(err);
        }
        obj = Value::new_array(vec![obj]);
    }
    let mut found = false;
    for info in obj.items() {
        if !ref_is(&info, "HASH") {
            continue;
        }
        // fix "SourceFile" case, or assume '*' if SourceFile not specified
        if info.get("SourceFile").is_undef() {
            let r = re(r"^SourceFile$", "i");
            let key = info.keys().into_iter().map(Value::Str).find(|k| r.is_match(k));
            match key {
                Some(key) if key.truthy() => {
                    info.set("SourceFile", info.get(key.to_bytes()));
                    info.delete(key.to_bytes());
                }
                _ => info.set("SourceFile", s("*")),
            }
        }
        if !missing_value.is_undef() {
            for k in info.keys() {
                if info.get(&k).to_bytes()[..] == missing_value.to_bytes()[..] {
                    info.set(&k, Value::Undef);
                }
            }
        }
        database.set(info.get("SourceFile").to_bytes(), info.clone());
        found = true;
    }
    ret(if found { Value::Undef } else { pl::concat(&s("No valid JSON objects in "), &file) })
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Import::ReadCSV", read_csv);
    it.define_native("Image::ExifTool::Import::ToUTF8", to_utf8);
    it.define_native("Image::ExifTool::Import::ReadJSONObject", read_json_object);
    it.define_native("Image::ExifTool::Import::ReadJSON", read_json);
}
