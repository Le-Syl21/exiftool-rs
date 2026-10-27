//! Image::ExifTool::WritePDF (Image/ExifTool/WritePDF.pl), ported sub by sub (see docs/port/PORTING.md).
//!
//! Write PDF meta information. The file's lexicals ($keyExt, $pdfVer,
//! %myDictTags, %pdfMap, $beginComment, $endComment) stay the Perl ones,
//! under Image::ExifTool::PDF::<my:WritePDF.pl>.

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::write_exif::{cat, cell, etf, get, keys, len, put, read, seek, seq, sort_num, sort_str, substr, substr_repl, tell, wlex_hash, wlex_scalar, write};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, re, ret};

const MODULE: &str = "Image::ExifTool::PDF";
const FILE: &str = "WritePDF.pl";

fn n(i: i64) -> Value {
    Value::Int(i)
}

fn key_ext(it: &mut Interp) -> Cell {
    wlex_scalar(it, MODULE, FILE, "keyExt")
}

fn pdf_ver(it: &mut Interp) -> Cell {
    wlex_scalar(it, MODULE, FILE, "pdfVer")
}

/// `$/`
fn irs(it: &mut Interp) -> Value {
    it.get_scalar("main::/")
}

/// `hex($s)`
fn hex(v: &Value) -> Value {
    let mut r: u64 = 0;
    for &c in v.to_bytes().iter() {
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            b'_' => continue,
            _ => break,
        };
        r = r.wrapping_mul(16).wrapping_add(d as u64);
    }
    ops::from_uv(r)
}

/// `%$h = ()`
fn clear_hash(h: &Value) {
    if let Value::HashRef(r) = h {
        r.borrow_mut().clear();
    }
}

/// Validate raw PDF values for writing (string date integer real boolean name)
/// Inputs: 0) ExifTool object ref, 1) tagInfo hash ref, 2) raw value ref
/// Returns: error string or undef (and possibly changes value) on success
pub fn check_pdf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (_et, tag_info, val_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let w = tag_info.get("Writable");
    let format = if w.truthy() { w } else { tag_info.get("Table").get("WRITABLE") };
    if !format.truthy() {
        return ret(pl::s("No writable format"));
    } else if format.eq_str("string") {
        // (encode later because list-type string tags need to be encoded as a unit)
    } else if format.eq_str("date") {
        // be flexible about this for now
        if !re(r"^\d{4}", "").is_match(&val_ptr.deref()) {
            return ret(pl::s("Bad date format"));
        }
    } else if format.eq_str("integer") {
        if !pl::call(it, "Image::ExifTool::IsInt", vec![val_ptr.deref()])?.truthy() {
            return ret(pl::s("Not an integer"));
        }
    } else if format.eq_str("real") {
        if !re(r"^[+-]?(?=\d|\.\d)\d*(\.\d*)?$", "").is_match(&val_ptr.deref()) {
            return ret(pl::s("Not a real number"));
        }
    } else if format.eq_str("boolean") {
        let v = val_ptr.deref();
        let t = v.truthy() && !re(r"^f", "i").is_match(&v);
        pl::set_deref(&val_ptr, pl::s(if t { "true" } else { "false" }))?;
    } else if format.eq_str("name") {
        if re(r"\0", "").is_match(&val_ptr.deref()) {
            return ret(pl::s("Invalid PDF name"));
        }
    } else {
        return ret(pl::s(&format!("Invalid PDF format '{}'", format.to_text())));
    }
    pl::ret_undef() // value is OK
}

/// Format value for writing to PDF file
/// Inputs: 0) ExifTool ref, 1) value, 2) format string (string,date,integer,real,boolean,name)
/// Returns: formatted value or undef on error
/// Notes: Called at write time, so $pdfVer may be checked
pub fn write_pdf_value(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, val, format) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    ret(write_pdf_value_v(it, &et, val, &format)?)
}

fn write_pdf_value_v(it: &mut Interp, et: &Value, mut val: Value, format: &Value) -> ER<Value> {
    if !format.truthy() {
        return Ok(Value::Undef);
    } else if format.eq_str("string") {
        // encode as UCS2 if it contains any special characters
        if re(r"[\x80-\xff]", "").is_match(&val) {
            let e = pl::method(it, et, "Encode", vec![val.clone(), pl::s("UCS2"), pl::s("MM")])?;
            val = pl::concat(&Value::bytes(b"\xfe\xff"), &e);
        }
        let r = pl::sref(val);
        encode_string_ref(it, &r)?;
        val = r.deref();
    } else if format.eq_str("date") {
        // convert date to "D:YYYYmmddHHMMSS+-HH'MM'" format
        re(r"(:\d{2})\.\d*", "").subst(&mut val, false, |c| cap(c, 1).to_bytes().to_vec()); // remove sub-seconds
        re(r"([-+]\d{2}):(\d{2})", "").subst(&mut val, false, |c| [&cap(c, 1).to_bytes()[..], b"'", &cap(c, 2).to_bytes()[..], b"'"].concat()); // change timezone delimiters if necessary
        pl::tr(&mut val, " :", "", "d"); // remove spaces and colons
        val = pl::concat(&pl::s("D:"), &val); // add leading "D:"
        let r = pl::sref(val);
        encode_string_ref(it, &r)?;
        val = r.deref();
    } else if re(r"^(integer|real|boolean)$", "").is_match(format) {
        // no reformatting necessary
    } else if format.eq_str("name") {
        if re(r"\0", "").is_match(&val) {
            return Ok(Value::Undef);
        }
        let ver = get(&pdf_ver(it));
        if pl::ge(&ver, &Value::Num(1.2)) {
            re(r"([\t\n\f\r ()<>\[\]{}/%#])", "s").subst(&mut val, true, |c| pl::sprintf("#%.2x", &[crate::modules::write_exif::ord(&cap(c, 1))]).to_bytes().to_vec());
        } else if re(r"[\t\n\f\r ()<>\[\]{}/%]", "").is_match(&val) {
            return Ok(Value::Undef);
        }
        val = pl::concat(&pl::s("/"), &val); // add leading '/'
    } else {
        return Ok(Value::Undef);
    }
    Ok(val)
}

/// Encode PDF string (the string held by a reference, updated in place).
fn encode_string_ref(it: &mut Interp, str_pt: &Value) -> ER<()> {
    let s = str_pt.deref();
    if s.is_array() {
        let items = s.items();
        for (i, item) in items.into_iter().enumerate() {
            let r = pl::sref(item);
            encode_string_ref(it, &r)?;
            s.set_at(i, r.deref());
        }
        return Ok(());
    }
    let ke = get(&key_ext(it));
    pl::call(it, "Image::ExifTool::PDF::Crypt", vec![str_pt.clone(), ke, n(1)])?; // encrypt if necessary
    // encode as hex if we have any control characters (except tab)
    if re(r"[\x00-\x08\x0a-\x1f\x7f\xff]", "").is_match(&str_pt.deref()) {
        // encode as hex
        let mut st = pl::s("");
        let l = len(&str_pt.deref());
        let mut i = n(0);
        loop {
            let mut nn = pl::sub(&l, &i);
            if !nn.truthy() {
                break;
            }
            if pl::gt(&nn, &n(40)) {
                nn = n(40); // break into reasonable-length lines
            }
            if i.truthy() {
                st = pl::concat(&st, &irs(it));
            }
            let part = substr(it, &str_pt.deref(), &i, Some(&nn))?;
            st = pl::concat(&st, &pl::unpack1("H*", &part)?);
            i = pl::add(&i, &nn);
        }
        pl::set_deref(str_pt, cat(&[pl::s("<"), st, pl::s(">")]))?;
    } else {
        let mut v = str_pt.deref();
        re(r"([()\\])", "").subst(&mut v, true, |c| [b"\\", &cap(c, 1).to_bytes()[..]].concat()); // must escape round brackets and backslashes
        pl::set_deref(str_pt, cat(&[pl::s("("), v, pl::s(")")]))?;
    }
    Ok(())
}

/// Encode PDF string
/// Inputs: 0) reference to PDF string
/// Returns: (updates string with encoded data)
pub fn encode_string(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    encode_string_ref(it, &arg(&a, 0))?;
    pl::ret_nothing(want)
}

/// CryptObject on a variable: returns whether `$_[0]` was assigned.
fn crypt_object_mut(it: &mut Interp, obj: &mut Value) -> ER<bool> {
    let o = obj.clone();
    if !o.is_ref() {
        // only literal strings and hex strings are encrypted
        if re(r"^[(<]", "").is_match(&o) {
            it.set_scalar("Image::ExifTool::PDF::lastFetched", Value::Undef); // (reset this just in case)
            let v = pl::call(it, "Image::ExifTool::PDF::ReadPDFValue", vec![o])?;
            let r = pl::sref(v);
            encode_string_ref(it, &r)?;
            *obj = r.deref();
            return Ok(true);
        }
    } else if pl::ref_of(&o) == "HASH" {
        let need_crypt = o.get("_needCrypt");
        let my_dict_tags = wlex_hash(it, MODULE, FILE, "myDictTags");
        for tag in o.keys() {
            if my_dict_tags.get(&tag).truthy() {
                continue;
            }
            // re-encrypt necessary objects only (others are still encrypted)
            // (this is really annoying, but is necessary because objects stored
            // in encrypted streams are decrypted when extracting, but strings stored
            // as direct objects are decrypted later since they must be decoded
            // before being decrypted)
            if need_crypt.truthy() {
                let nc = need_crypt.get(&tag);
                let flag = if !nc.is_undef() { nc } else { need_crypt.get("*") };
                if !flag.truthy() {
                    continue;
                }
            }
            let mut v = o.get(&tag);
            if crypt_object_mut(it, &mut v)? {
                o.set(&tag, v);
            }
        }
        o.delete("_needCrypt"); // avoid re-re-crypting
    } else if pl::ref_of(&o) == "ARRAY" {
        for (i, item) in o.items().into_iter().enumerate() {
            let mut v = item;
            if crypt_object_mut(it, &mut v)? {
                o.set_at(i, v);
            }
        }
    }
    Ok(false)
}

/// Encrypt an object
/// Inputs: 0) PDF object (encrypts in place)
/// Notes: Encrypts according to "_needCrypt" dictionary entry,
///        then deletes "_needCrypt" when done
pub fn crypt_object(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let mut obj = arg(&a, 0);
    if crypt_object_mut(it, &mut obj)? {
        it.set_arg(0, obj)?;
    }
    pl::ret_nothing(want)
}

fn get_free_entries_v(it: &mut Interp, dict: &Value) -> ER<Value> {
    let xref_free = Value::new_hash();
    // we write xref stream entries in 'CNn' or 'CNNn' format (with 8-byte 'NN' offset),
    let w = dict.get("W");
    if pl::ref_of(&w) == "ARRAY" {
        let sep = it.get_scalar("main::\"");
        let parts: Vec<Vec<u8>> = w.items().iter().map(|v| v.to_bytes().to_vec()).collect();
        let bytes = Value::bytes(parts.join(&sep.to_bytes()[..]));
        let fmt;
        if bytes.eq_str("1 4 2") {
            fmt = "CNn";
        } else if bytes.eq_str("1 8 2") {
            fmt = "CNNn";
        } else {
            return Ok(xref_free);
        }
        let size = dict.get("_entry_size"); // this will be 7 for 'CNn' or 11 for 'CNNn'
        let index = dict.get("Index");
        let l = len(&dict.get("_stream"));
        // scan the table for free objects
        let num = pl::div(&n(index.len() as i64), &n(2))?;
        let mut pos = n(0);
        let mut i = n(0);
        while pl::lt(&i, &num) {
            let start = index.at(pl::mul(&i, &n(2)).to_i64());
            let count = index.at(pl::add(&pl::mul(&i, &n(2)), &n(1)).to_i64());
            let mut j = n(0);
            while pl::lt(&j, &count) {
                if pl::gt(&pl::add(&pos, &size), &l) {
                    break;
                }
                let tmpl = format!("x{} {}", pos.to_text(), fmt);
                let mut t = pl::unpack(&tmpl, &dict.get("_stream"))?;
                if t.len() == 4 {
                    t[1] = pl::add(&pl::mul(&t[1], &Value::Num(4294967296.0)), &t[2]);
                    t[2] = t[3].clone();
                    t = vec![n(3)];
                }
                // add entry if object was free
                if pl::num_eq(&pl::arg(&t, 0), &n(0)) {
                    xref_free.set(pl::add(&start, &j).to_bytes(), Value::new_array(vec![pl::arg(&t, 1), pl::arg(&t, 2), pl::s("f")]));
                }
                pos = pl::add(&pos, &size); // step to next entry
                j = pl::add(&j, &n(1));
            }
            i = pl::add(&i, &n(1));
        }
    }
    Ok(xref_free)
}

/// Get free entries from xref stream dictionary that we wrote previously
/// Inputs: 0) xref dictionary reference
/// Returns: free entry hash (keys are object numbers, values are xref entry list refs)
pub fn get_free_entries(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(get_free_entries_v(it, &arg(&a, 0))?)
}

fn write_object_v(it: &mut Interp, outfile: &Value, obj: &Value) -> ER<bool> {
    let nl = irs(it);
    let r = pl::ref_of(obj);
    if r == "SCALAR" {
        if !write(it, outfile, vec![pl::s(" "), obj.deref()])? {
            return Ok(false);
        }
    } else if r == "ARRAY" {
        // write array
        let lead = if obj.len() > 10 { nl.clone() } else { pl::s(" ") };
        if !write(it, outfile, vec![lead, pl::s("[")])? {
            return Ok(false);
        }
        for item in obj.items() {
            if !write_object_v(it, outfile, &item)? {
                return Ok(false);
            }
        }
        if !write(it, outfile, vec![pl::s(" ]")])? {
            return Ok(false);
        }
    } else if r == "HASH" {
        // write dictionary
        if !write(it, outfile, vec![nl.clone(), pl::s("<<")])? {
            return Ok(false);
        }
        // prepare object as required if it has a stream
        if obj.get("_stream").truthy() {
            // encrypt stream if necessary (must be done before determining Length)
            if obj.get("_decrypted").truthy() {
                let ke = get(&key_ext(it));
                pl::call(it, "Image::ExifTool::PDF::CryptStream", vec![obj.clone(), ke])?;
            }
            // write "Length" entry in dictionary
            obj.set("Length", len(&obj.get("_stream")));
            obj.get_or_new_array("_tags").push(pl::s("Length"));
            // delete Filter-related entries since we don't yet write filtered streams
            obj.delete("Filter");
            obj.delete("DecodeParms");
            obj.delete("DL");
        }
        // don't write my internal entries
        let wrote = wlex_hash(it, MODULE, FILE, "myDictTags").hash_copy();
        // write tags in original order, adding new ones later alphabetically
        let mut tags = obj.get_or_new_array("_tags").items();
        let mut k = keys(obj);
        sort_str(&mut k);
        tags.extend(k);
        for tag in tags {
            // ignore already-written or missing entries
            let tb = tag.to_bytes();
            if wrote.get(&tb).truthy() || obj.get(&tb).is_undef() {
                continue;
            }
            if !write(it, outfile, vec![nl.clone(), pl::concat(&pl::s("/"), &tag)])? {
                return Ok(false);
            }
            if !write_object_v(it, outfile, &obj.get(&tb))? {
                return Ok(false);
            }
            wrote.set(&tb, n(1));
        }
        if !write(it, outfile, vec![nl.clone(), pl::s(">>")])? {
            return Ok(false);
        }
        if obj.get("_stream").truthy() {
            // write object stream
            // (a single 0x0d may not follow 'stream', so use 0x0d+0x0a here to be sure)
            if !write(it, outfile, vec![nl.clone(), pl::s("stream\x0d\x0a")])? {
                return Ok(false);
            }
            if !write(it, outfile, vec![obj.get("_stream"), nl.clone(), pl::s("endstream")])? {
                return Ok(false);
            }
        }
    } else {
        // write string, number, name or object reference
        write(it, outfile, vec![pl::s(" "), obj.clone()])?;
    }
    Ok(true)
}

/// Write PDF object
/// Inputs: 0) output file or scalar ref, 1) PDF object
/// Returns: true on success
/// Notes: inserts white space before object, but none afterward
pub fn write_object(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let ok = write_object_v(it, &arg(&a, 0), &arg(&a, 1))?;
    ret(n(ok as i64))
}

/// `$$h{k}->[i]`
fn at(h: &Value, k: &Value, i: i64) -> Value {
    h.get(k.to_bytes()).at(i)
}

/// Write PDF File
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if not valid PDF file, -1 on write error
/// Notes: dictionary structure:  Main --+--> Info
///                                      +--> Root --> Metadata
pub fn write_pdf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let outfile = dir_info.get("OutFile");
    let buff = cell(Value::Undef);
    let capture = Value::new_hash();
    let new_xref = Value::new_hash();
    let new_obj = Value::new_hash();
    let (mut out, mut id, mut gen) = (Value::Undef, Value::Undef, Value::Undef);

    // make sure this is a PDF file
    let pos = tell(it, &raf)?;
    if !pl::ge(&read(it, &raf, &buff, n(1024))?, &n(8)) {
        return ret(n(0));
    }
    let Some(c) = re(r"^(\s*)%PDF-(\d+\.\d+)", "").m(&get(&buff)) else {
        return ret(n(0));
    };
    et.set("PDFBase", len(&cap(&c, 1)));
    seek(it, &raf, pos, 0)?;

    // create a new ExifTool object and use it to read PDF and XMP information
    let new_tool = pl::method(it, &pl::s("Image::ExifTool"), "new", vec![])?;
    pl::method(it, &new_tool, "Options", vec![pl::s("List"), n(1)])?;
    let pw = pl::method(it, &et, "Options", vec![pl::s("Password")])?;
    pl::method(it, &new_tool, "Options", vec![pl::s("Password"), pw])?;
    let np = pl::method(it, &et, "Options", vec![pl::s("NoPDFList")])?;
    pl::method(it, &new_tool, "Options", vec![pl::s("NoPDFList"), np])?;
    new_tool.set("PDF_CAPTURE", capture.clone());
    let mut info = pl::method(it, &new_tool, "ImageInfo", vec![raf.clone(), pl::s("XMP"), pl::s("PDF:*"), pl::s("Error"), pl::s("Warning")])?;
    // not a valid PDF file unless we got a version number
    let pv = pdf_ver(it);
    put(&pv, new_tool.get("PDFVersion"));
    if !get(&pv).truthy() {
        pl::method(it, &et, "Error", vec![pl::s("Missing PDF:PDFVersion")])?;
        return ret(n(0));
    }
    // check version number
    if pl::gt(&get(&pv), &Value::Num(2.0)) && pl::method(it, &et, "Error", vec![cat(&[pl::s("Writing PDF "), get(&pv), pl::s(" is untested")]), n(1)])?.truthy() {
        return ret(n(0));
    }
    // fail if we had any serious errors while extracting information
    if capture.get("Error").truthy() || info.get("Error").truthy() {
        let e = if capture.get("Error").truthy() { capture.get("Error") } else { info.get("Error") };
        pl::method(it, &et, "Error", vec![e])?;
        return ret(n(1));
    }
    // make sure we have everything we need to rewrite this file
    for obj in ["Main", "Root", "xref"] {
        if capture.get(obj).truthy() {
            continue;
        }
        // any warning we received may give a clue about why this object is missing
        if info.get("Warning").truthy() {
            pl::method(it, &et, "Error", vec![info.get("Warning")])?;
        }
        pl::method(it, &et, "Error", vec![pl::s(&format!("Can't find {obj} object"))])?;
        return ret(n(1));
    }
    let pdf_map = wlex_hash(it, MODULE, FILE, "pdfMap");
    pl::method(it, &et, "InitWriteDirs", vec![pdf_map, pl::s("XMP")])?;

    // copy file up to start of previous exiftool update or end of file
    // (comment, startxref & EOF with 11-digit offsets and 2-byte newlines is 63 bytes)
    if !(seek(it, &raf, n(-64), 2)? && read(it, &raf, &buff, n(64))?.truthy() && seek(it, &raf, n(0), 0)?) {
        return ret(n(-1));
    }
    let mut rtn = n(1);
    let mut prev_update = Value::Undef;
    let begin_comment = get(&wlex_scalar(it, MODULE, FILE, "beginComment"));
    let end_comment = get(&wlex_scalar(it, MODULE, FILE, "endComment"));
    // (now $endComment is before "startxref", but pre-7.41 we wrote it after the EOF)
    let mut p = end_comment.to_bytes().to_vec();
    p.extend_from_slice(br"(\d+)\s+(startxref\s+\d+\s+%%EOF\s+)?$");
    if let Some(c) = pl::re_dyn(&p, "s")?.m(&get(&buff)) {
        prev_update = cap(&c, 1);
        // rewrite the file up to the original EOF
        if !etf(it, "CopyBlock", vec![raf.clone(), outfile.clone(), pl::add(&prev_update, &et.get("PDFBase"))])?.truthy() {
            rtn = n(-1);
        }
        // verify that we are now at the start of an ExifTool update
        if !(read(it, &raf, &buff, len(&begin_comment))?.truthy() && seq(&get(&buff), &begin_comment)) {
            pl::method(it, &et, "Error", vec![pl::s("Previous ExifTool update is corrupted")])?;
            return ret(rtn);
        }
        if !seek(it, &raf, pl::add(&prev_update, &et.get("PDFBase")), 0)? {
            rtn = n(-1);
        }
        if et.get("DEL_GROUP").get("PDF-update").truthy() {
            pl::method(it, &et, "VPrint", vec![n(0), pl::s("  Reverted previous ExifTool updates\n")])?;
            crate::modules::write_exif::inc(&et, "CHANGED");
            return ret(rtn);
        }
    } else if et.get("DEL_GROUP").get("PDF-update").truthy() {
        pl::method(it, &et, "Error", vec![pl::s("File contains no previous ExifTool update")])?;
        return ret(rtn);
    } else {
        // rewrite the whole file
        while read(it, &raf, &buff, n(65536))?.truthy() {
            if !write(it, &outfile, vec![get(&buff)])? {
                rtn = n(-1);
            }
        }
    }
    if pl::method(it, &et, "Options", vec![pl::s("Verbose")])?.truthy() {
        out = pl::method(it, &et, "Options", vec![pl::s("TextOut")])?;
    }
    //
    // create our new PDF objects to write
    //
    let xref = capture.get("xref");
    let main_dict = capture.get("Main");
    let mut meta_ref = capture.get("Root").get("Metadata");
    let mut next_object;

    // start by finding reference for info object in case it was deleted
    // in a previous edit so we can re-use it here if adding PDF Info
    let prev_info_ref;
    if prev_update.truthy() {
        if !capture.get("Prev").truthy() {
            pl::method(it, &et, "Error", vec![pl::s("Can't locate trailer dictionary prior to last edit")])?;
            return ret(rtn);
        }
        prev_info_ref = capture.get("Prev").get("Info");
        // start from previous size so the xref table doesn't continue
        // to grow if we repeatedly add and delete the Metadata object
        next_object = capture.get("Prev").get("Size");
        // don't re-use Meta reference if object was added in a previous update
        if meta_ref.truthy() {
            if let Some(c) = re(r"^(\d+)", "").m(&meta_ref.deref()) {
                if pl::ge(&cap(&c, 1), &next_object) {
                    meta_ref = Value::Undef;
                }
            }
        }
    } else {
        prev_info_ref = main_dict.get("Info");
        next_object = main_dict.get("Size");
    }

    // delete entire PDF group if specified
    let mut info_changed = n(0);
    if et.get("DEL_GROUP").get("PDF").truthy() && capture.get("Info").truthy() {
        capture.delete("Info");
        let ni = Value::new_hash();
        ni.set("XMP", info.get("XMP"));
        info = ni; // remove extracted PDF tags
        if out.truthy() {
            crate::modules::write_exif::print_out(it, &out, pl::s("  Deleting PDF Info dictionary\n"))?;
        }
        info_changed = pl::add(&info_changed, &n(1));
    }

    // create new Info dictionary if necessary
    if !capture.get("Info").truthy() {
        let d = Value::new_hash();
        d.set("_tags", Value::new_array(vec![]));
        capture.set("Info", d);
    }
    let info_dict = capture.get("Info");

    // must pre-determine Info reference to be used in encryption
    let info_ref = if prev_info_ref.truthy() { prev_info_ref.clone() } else { pl::sref(pl::concat(&next_object, &pl::s(" 0 R"))) };
    if pl::ref_of(&info_ref) != "SCALAR" {
        pl::method(it, &et, "Error", vec![pl::s("Info dictionary is not an indirect object")])?;
        return ret(rtn);
    }
    let ke = key_ext(it);
    put(&ke, info_ref.deref());

    // must encrypt all values in dictionary if they came from an encrypted stream
    if info_dict.get("_needCrypt").truthy() {
        let mut d = info_dict.clone();
        crypt_object_mut(it, &mut d)?;
    }

    // must set line separator before calling WritePDFValue()
    let newline = capture.get("newline");
    let r = pl::local_global(it, b'$', "main::/", |it| -> ER<Value> {
        it.set_scalar("main::/", newline);
        let nl = irs(it);

        // rewrite PDF Info tags
        let info_table = pl::pkg_hash(it, "Image::ExifTool::PDF::Info");
        let new_tags = pl::method(it, &et, "GetNewTagInfoHash", vec![info_table.clone()])?;
        let mut tag_ids = keys(&new_tags);
        sort_str(&mut tag_ids);
        for tag_id in tag_ids {
            let tid = tag_id.to_bytes();
            let tag_info = new_tags.get(&tid);
            if pl::ge(&get(&pdf_ver(it)), &Value::Num(2.0)) && !tag_info.get("PDF2").truthy() {
                let msg = cat(&[pl::s("Writing PDF:"), tag_info.get("Name"), pl::s(" is deprecated for PDF 2.0 documents")]);
                if pl::method(it, &et, "Warn", vec![msg, n(2)])?.truthy() {
                    continue;
                }
            }
            let nv_hash = pl::method(it, &et, "GetNewValueHash", vec![tag_info.clone()])?;
            let mut vals: Vec<Value> = Vec::new();
            let mut deleted = false;
            let tag = tag_info.get("Name");
            let mut val = info.get(tag.to_bytes());
            if val.is_undef() {
                // must check for tag key with copy number
                let mut p = b"^".to_vec();
                p.extend_from_slice(&tag.to_bytes());
                let r = pl::re_dyn(&p, "")?;
                let tag_key = keys(&info).into_iter().find(|k| r.is_match(k)).unwrap_or_default();
                if tag_key.truthy() {
                    val = info.get(tag_key.to_bytes());
                }
            }
            if !val.is_undef() {
                let mut old_vals: Vec<Value> = Vec::new();
                if pl::ref_of(&val) == "ARRAY" {
                    old_vals = val.items();
                    val = if old_vals.is_empty() { Value::Undef } else { old_vals.remove(0) };
                }
                loop {
                    if pl::gt(&pl::method(it, &et, "IsOverwriting", vec![nv_hash.clone(), val.clone()])?, &n(0)) {
                        deleted = true;
                        pl::method(it, &et, "VerboseValue", vec![pl::concat(&pl::s("- PDF:"), &tag), val.clone()])?;
                        info_changed = pl::add(&info_changed, &n(1));
                    } else {
                        vals.push(val.clone());
                    }
                    if old_vals.is_empty() {
                        break;
                    }
                    val = old_vals.remove(0);
                }
                // don't write this out if we deleted all values
                if vals.is_empty() {
                    info_dict.delete(&tid);
                }
            } else if nv_hash.get("EditOnly").truthy() {
                continue;
            }
            // decide whether we want to write this tag
            // (native PDF information is always preferred, so don't check IsCreating)
            if !(deleted || tag_info.get("List").truthy() || !info_dict.exists(&tid)) {
                continue;
            }

            // add new values to existing ones
            let new_vals = pl::method_list(it, &et, "GetNewValue", vec![nv_hash.clone()])?;
            if !new_vals.is_empty() {
                vals.extend(new_vals.iter().cloned());
                info_changed = pl::add(&info_changed, &n(1));
                if out.truthy() {
                    for v in &new_vals {
                        pl::method(it, &et, "VerboseValue", vec![pl::concat(&pl::s("+ PDF:"), &tag), v.clone()])?;
                    }
                }
            }
            if vals.is_empty() {
                // remove this entry from the Info dictionary if no values remain
                info_dict.delete(&tid);
                continue;
            }
            // format value(s) for writing to PDF file
            let w = tag_info.get("Writable");
            let writable = if w.truthy() { w } else { info_table.get("WRITABLE") };
            if !tag_info.get("List").truthy() {
                let first = vals.remove(0);
                val = write_pdf_value_v(it, &et, first, &writable)?;
            } else if tag_info.get("List").eq_str("array") {
                let mut i = 0;
                while i < vals.len() {
                    let v = write_pdf_value_v(it, &et, vals[i].clone(), &writable)?;
                    vals[i] = v.clone();
                    if v.is_undef() {
                        vals.clear();
                        break;
                    }
                    i += 1;
                }
                val = if !vals.is_empty() { Value::new_array(vals.clone()) } else { Value::Undef };
            } else {
                let sep = pl::method(it, &et, "Options", vec![pl::s("ListSep")])?;
                let parts: Vec<Vec<u8>> = vals.iter().map(|v| v.to_bytes().to_vec()).collect();
                let joined = Value::bytes(parts.join(&sep.to_bytes()[..]));
                val = write_pdf_value_v(it, &et, joined, &writable)?;
            }
            if !val.is_undef() {
                info_dict.set(&tid, val);
                info_changed = pl::add(&info_changed, &n(1));
            } else {
                pl::method(it, &et, "Warn", vec![cat(&[pl::s("Error converting "), tag_info.get("Name"), pl::s(" value")])])?;
            }
        }
        if info_changed.truthy() {
            et.set("CHANGED", pl::add(&et.get("CHANGED"), &info_changed));
        } else if prev_update.truthy() {
            // must still write Info dictionary if it was previously updated
            let old_pos = pl::call(it, "Image::ExifTool::PDF::LocateObject", vec![xref.clone(), info_ref.deref()])?;
            if old_pos.truthy() && pl::gt(&old_pos, &prev_update) {
                info_changed = n(1);
            }
        }

        // create new Info dictionary if necessary
        if info_changed.truthy() {
            // increment object count if we used a new object here
            if capture.get("Info").len() > 1 {
                new_obj.set(info_ref.deref().to_bytes(), capture.get("Info")); // save to write later
                main_dict.set("Info", info_ref.clone()); // add reference to trailer dictionary
                if !prev_info_ref.truthy() {
                    next_object = pl::add(&next_object, &n(1));
                }
            } else {
                // remove Info from Main (trailer) dictionary
                main_dict.delete("Info");
                // write free entry in xref table if Info existed prior to all edits
                if prev_info_ref.truthy() {
                    new_obj.set(info_ref.deref().to_bytes(), pl::s(""));
                }
            }
        }

        // rewrite XMP
        let xmp_info = Value::new_hash();
        xmp_info.set("DataPt", info.get("XMP"));
        xmp_info.set("Parent", pl::s("PDF"));
        let xmp_table = etf(it, "GetTagTable", vec![pl::s("Image::ExifTool::XMP::Main")])?;
        let old_changed = et.get("CHANGED");
        let mut new_xmp = pl::method(it, &et, "WriteDirectory", vec![xmp_info, xmp_table])?;
        if new_xmp.is_undef() {
            new_xmp = if info.get("XMP").truthy() { info.get("XMP").deref() } else { pl::s("") };
        }

        // WriteDirectory() will increment CHANGED erroneously if non-existent
        // XMP is deleted as a block -- so check for this
        if !(new_xmp.truthy() || info.get("XMP").truthy()) {
            et.set("CHANGED", old_changed.clone());
            pl::method(it, &et, "VPrint", vec![n(0), pl::s("  (XMP not changed -- still empty)\n")])?;
        }
        let (mut meta_changed, mut root_changed) = (false, false);

        if !pl::num_eq(&et.get("CHANGED"), &old_changed) && !new_xmp.is_undef() {
            meta_changed = true;
        } else if prev_update.truthy() && capture.get("Root").get("Metadata").truthy() {
            // must still write Metadata dictionary if it was previously updated
            let old_pos = pl::call(it, "Image::ExifTool::PDF::LocateObject", vec![xref.clone(), capture.get("Root").get("Metadata").deref()])?;
            if old_pos.truthy() && pl::gt(&old_pos, &prev_update) {
                meta_changed = true;
            }
        }
        if meta_changed {
            if new_xmp.truthy() {
                if !meta_ref.is_ref() {
                    // allocate new PDF object
                    meta_ref = pl::sref(pl::concat(&next_object, &pl::s(" 0 R")));
                    next_object = pl::add(&next_object, &n(1));
                    capture.get("Root").set("Metadata", meta_ref.clone());
                    root_changed = true; // set flag to replace Root dictionary
                }
                // create the new metadata dictionary to write later
                let d = Value::new_hash();
                d.set("Type", pl::s("/Metadata"));
                d.set("Subtype", pl::s("/XML"));
                // Length     => length $newXMP, (set by WriteObject)
                d.set("_tags", Value::new_array(vec![pl::s("Type"), pl::s("Subtype")]));
                d.set("_stream", new_xmp.clone());
                d.set("_decrypted", n(1)); // (this will be ignored if EncryptMetadata is false)
                new_obj.set(meta_ref.deref().to_bytes(), d);
            } else if capture.get("Root").get("Metadata").truthy() {
                // free existing metadata object
                new_obj.set(capture.get("Root").get("Metadata").deref().to_bytes(), pl::s(""));
                capture.get("Root").delete("Metadata");
                root_changed = true; // set flag to replace Root dictionary
            }
        }
        // add new Root dictionary if necessary
        let root_ref = main_dict.get("Root");
        if !root_ref.truthy() {
            pl::method(it, &et, "Error", vec![pl::s("Can't find Root dictionary")])?;
            return Ok(rtn.clone());
        }
        if !root_changed && prev_update.truthy() {
            // must still write Root dictionary if it was previously updated
            let old_pos = pl::call(it, "Image::ExifTool::PDF::LocateObject", vec![xref.clone(), root_ref.deref()])?;
            if old_pos.truthy() && pl::gt(&old_pos, &prev_update) {
                root_changed = true;
            }
        }
        if root_changed {
            new_obj.set(root_ref.deref().to_bytes(), capture.get("Root"));
        }
        //
        // write incremental update if anything was changed
        //
        if et.get("CHANGED").truthy() {
            // remember position of original EOF
            let old_eof = pl::sub(&etf(it, "Tell", vec![outfile.clone()])?, &et.get("PDFBase"));
            if !write(it, &outfile, vec![begin_comment.clone()])? {
                rtn = n(-1);
            }

            // write new objects
            let mut obj_refs = keys(&new_obj);
            sort_str(&mut obj_refs);
            for obj_ref in obj_refs {
                let Some(c) = re(r"^(\d+) (\d+)", "").m(&obj_ref) else {
                    rtn = n(-1);
                    break;
                };
                id = cap(&c, 1);
                gen = cap(&c, 2);
                if !new_obj.get(obj_ref.to_bytes()).truthy() {
                    if pl::lt(&gen, &n(65535)) {
                        gen = pl::add(&gen, &n(1));
                    }
                    // write free entry in xref table
                    new_xref.set(id.to_bytes(), Value::new_array(vec![n(0), gen.clone(), pl::s("f")]));
                    continue;
                }
                // create new entry for xref table
                let p = pl::add(&pl::sub(&etf(it, "Tell", vec![outfile.clone()])?, &et.get("PDFBase")), &len(&nl));
                new_xref.set(id.to_bytes(), Value::new_array(vec![p, gen.clone(), pl::s("n")]));
                put(&key_ext(it), cat(&[id.clone(), pl::s(" "), gen.clone(), pl::s(" obj")])); // (must set for stream encryption)
                let kx = get(&key_ext(it));
                if !write(it, &outfile, vec![nl.clone(), kx])? {
                    rtn = n(-1);
                }
                if !write_object_v(it, &outfile, &new_obj.get(obj_ref.to_bytes()))? {
                    rtn = n(-1);
                }
                if !write(it, &outfile, vec![nl.clone(), pl::s("endobj")])? {
                    rtn = n(-1);
                }
            }

            // Prev points to old xref table
            if !prev_update.truthy() {
                main_dict.set("Prev", capture.get("startxref"));
            }

            // add xref entry for head of free-object list
            new_xref.set("0", Value::new_array(vec![n(0), n(65535), pl::s("f")]));

            // must insert free xref entries from previous exiftool update if applicable
            if prev_update.truthy() {
                let main_free;
                // extract free entries from our previous Main xref stream
                if main_dict.get("Type").truthy() && main_dict.get("Type").eq_str("/XRef") {
                    main_free = get_free_entries_v(it, &xref.get("dicts").at(0))?;
                } else {
                    // free entries from Main xref table already captured for us
                    main_free = capture.get("mainFree");
                }
                let mut ids = keys(&main_free);
                sort_num(&mut ids);
                for i in ids {
                    if !new_xref.get(i.to_bytes()).truthy() {
                        new_xref.set(i.to_bytes(), main_free.get(i.to_bytes()));
                    }
                }
            }

            // connect linked list of free object in our xref table
            let mut prev_free = n(0);
            let mut ids = keys(&new_xref);
            sort_num(&mut ids);
            ids.reverse(); // (reverse sort)
            for i in ids {
                id = i.clone();
                if !at(&new_xref, &id, 2).eq_str("f") {
                    continue; // skip if not free
                }
                // no need to add free entry for objects added by us
                // in previous edits then freed again
                if pl::ge(&id, &next_object) {
                    new_xref.delete(id.to_bytes()); // Note: deleting newXRef entry!
                    continue;
                }
                new_xref.get(id.to_bytes()).set_at(0, prev_free.clone());
                prev_free = id.clone();
            }

            // prepare our main dictionary for writing
            main_dict.set("Size", next_object.clone()); // update number of objects
            // must change the ID if it exists
            if pl::ref_of(&main_dict.get("ID")) == "ARRAY" && main_dict.get("ID").len() > 1 {
                // increment first byte since this is an easy change to make
                id = main_dict.get("ID").at(1);
                if let Some(c) = re(r"^<([0-9a-f]{2})", "i").m(&id) {
                    let v = ops::bit_and(&pl::add(&hex(&cap(&c, 1)), &n(1)), &n(0xff));
                    let byte = pl::unpack1("H2", &crate::modules::write_exif::chr(&v))?;
                    id = substr_repl(&id, &n(1), Some(&n(2)), &byte)?;
                } else if let Some(c) = re(r"^\((.)", "s").m(&id) {
                    let c1 = cap(&c, 1);
                    if !c1.eq_str("\\") && !c1.eq_str(")") && !c1.eq_str("(") {
                        let v = ops::bit_and(&pl::add(&crate::modules::write_exif::ord(&c1), &n(1)), &n(0xff));
                        let mut ch = crate::modules::write_exif::chr(&v);
                        // avoid generating characters that could cause problems
                        if re(r"[()\\\x00-\x08\x0a-\x1f\x7f\xff]", "").is_match(&ch) {
                            ch = pl::s("a");
                        }
                        id = substr_repl(&id, &n(1), Some(&n(1)), &ch)?;
                    }
                }
                main_dict.get("ID").set_at(1, id.clone());
            }

            // remember position of xref table in file (we will write this next)
            let startxref = pl::add(&pl::sub(&etf(it, "Tell", vec![outfile.clone()])?, &et.get("PDFBase")), &len(&nl));

            // must write xref as a stream in xref-stream-only files
            if main_dict.get("Type").truthy() && main_dict.get("Type").eq_str("/XRef") {
                // create entry for the xref stream object itself
                let p = pl::add(&pl::sub(&etf(it, "Tell", vec![outfile.clone()])?, &et.get("PDFBase")), &len(&nl));
                new_xref.set(next_object.to_bytes(), Value::new_array(vec![p, n(0), pl::s("n")]));
                next_object = pl::add(&next_object, &n(1));
                main_dict.set("Size", next_object.clone());
                // create xref stream and Index entry
                let mut bits = n(4);
                'restart: loop {
                    main_dict.set("W", Value::new_array(vec![n(1), bits.clone(), n(2)])); // int8u, int32u/int64u, int16u ('CNn' or 'CNNn')
                    main_dict.set("Index", Value::new_array(vec![]));
                    main_dict.set("_stream", pl::s(""));
                    let mut ids = keys(&new_xref);
                    sort_num(&mut ids);
                    while !ids.is_empty() {
                        let start_id = ids[0].clone();
                        loop {
                            id = ids.remove(0);
                            let e = new_xref.get(id.to_bytes());
                            let (pos, gen, typ) = (e.at(0), e.at(1), e.at(2));
                            if pl::gt(&pos, &Value::Num(4294967295.0)) && pl::num_eq(&bits, &n(4)) {
                                // switch to 64-bit integer offsets
                                bits = n(8);
                                continue 'restart;
                            }
                            let t = if typ.eq_str("f") { n(0) } else { n(1) };
                            let packed = if pl::num_eq(&bits, &n(4)) {
                                pl::pack("CNn", &[t, pos, gen])?
                            } else {
                                let hi = pl::int(&pl::div(&pos, &Value::Num(4294967296.0))?);
                                let lo = pl::sub(&pos, &pl::mul(&hi, &Value::Num(4294967296.0)));
                                pl::pack("CNNn", &[t, hi, lo, gen])?
                            };
                            main_dict.set("_stream", pl::concat(&main_dict.get("_stream"), &packed));
                            if ids.is_empty() || !pl::num_eq(&ids[0], &pl::add(&id, &n(1))) {
                                break;
                            }
                        }
                        // add Index entries for this section of the xref stream
                        let idx = main_dict.get("Index");
                        idx.push(start_id.clone());
                        idx.push(pl::add(&pl::sub(&id, &start_id), &n(1)));
                    }
                    break;
                }
                // write the xref stream object
                put(&key_ext(it), cat(&[id.clone(), pl::s(" 0 obj")])); // (set anyway, but xref stream should NOT be encrypted)
                let kx = get(&key_ext(it));
                if !write(it, &outfile, vec![nl.clone(), kx])? {
                    rtn = n(-1);
                }
                if !write_object_v(it, &outfile, &main_dict)? {
                    rtn = n(-1);
                }
                if !write(it, &outfile, vec![nl.clone(), pl::s("endobj")])? {
                    rtn = n(-1);
                }
            } else {
                // write new xref table
                if !write(it, &outfile, vec![nl.clone(), pl::s("xref"), nl.clone()])? {
                    rtn = n(-1);
                }
                // lines must be exactly 20 bytes, so pad newline if necessary
                let endl = pl::concat(&pl::s(if len(&nl).to_i64() == 1 { " " } else { "" }), &nl);
                let mut ids = keys(&new_xref);
                sort_num(&mut ids);
                while !ids.is_empty() {
                    let start_id = ids[0].clone();
                    let mut b = pl::s("");
                    loop {
                        id = ids.remove(0);
                        let mut args = new_xref.get(id.to_bytes()).items();
                        args.push(endl.clone());
                        b = pl::concat(&b, &pl::sprintf("%.10d %.5d %s%s", &args));
                        if ids.is_empty() || !pl::num_eq(&ids[0], &pl::add(&id, &n(1))) {
                            break;
                        }
                    }
                    put(&buff, b.clone());
                    // write this (contiguous-numbered object) section of the xref table
                    let cnt = pl::add(&pl::sub(&id, &start_id), &n(1));
                    if !write(it, &outfile, vec![start_id, pl::s(" "), cnt, nl.clone(), b])? {
                        rtn = n(-1);
                    }
                }

                // write main (trailer) dictionary
                if !write(it, &outfile, vec![pl::s("trailer")])? {
                    rtn = n(-1);
                }
                if !write_object_v(it, &outfile, &main_dict)? {
                    rtn = n(-1);
                }
            }
            // write trailing comment (marker to allow edits to be reverted)
            if !write(it, &outfile, vec![nl.clone(), end_comment.clone(), old_eof, nl.clone()])? {
                rtn = n(-1);
            }

            // write pointer to main xref table and EOF marker
            if !write(it, &outfile, vec![pl::s("startxref"), nl.clone(), startxref, nl.clone(), pl::s("%%EOF"), nl.clone()])? {
                rtn = n(-1);
            }
        } else if prev_update.truthy() {
            // nothing new changed, so copy over previous incremental update
            if !seek(it, &raf, pl::add(&prev_update, &et.get("PDFBase")), 0)? {
                rtn = n(-1);
            }
            while read(it, &raf, &buff, n(65536))?.truthy() {
                if !write(it, &outfile, vec![get(&buff)])? {
                    rtn = n(-1);
                }
            }
        }
        if pl::gt(&rtn, &n(0)) && et.get("CHANGED").truthy() && (et.get("DEL_GROUP").get("PDF").truthy() || et.get("DEL_GROUP").get("XMP").truthy()) {
            pl::method(it, &et, "Warn", vec![pl::s("ExifTool PDF edits are reversible. Deleted tags may be recovered!"), n(1)])?;
        }
        // undef $newTool; undef %capture;
        clear_hash(&capture);
        Ok(rtn.clone())
    })?;
    ret(r)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::PDF::CheckPDF", check_pdf);
    it.define_native("Image::ExifTool::PDF::WritePDFValue", write_pdf_value);
    it.define_native("Image::ExifTool::PDF::EncodeString", encode_string);
    it.define_native("Image::ExifTool::PDF::CryptObject", crypt_object);
    it.define_native("Image::ExifTool::PDF::GetFreeEntries", get_free_entries);
    it.define_native("Image::ExifTool::PDF::WriteObject", write_object);
    it.define_native("Image::ExifTool::PDF::WritePDF", write_pdf);
}
