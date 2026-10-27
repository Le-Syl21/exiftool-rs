//! Image::ExifTool::Font (Image/ExifTool/Font.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read meta information from font files

// (the Perl assigns values it does not read again; the port keeps them)
#![allow(unused_assignments)]

use crate::interp::eval::{die, Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};
use crate::runtime;

const MODULE: &str = "Image::ExifTool::Font";

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `\$buff` of a `my` variable kept in a cell.
fn rf(c: &Cell) -> Value {
    Value::ScalarRef(c.clone())
}

fn val(c: &Cell) -> Value {
    c.borrow().clone()
}

fn set(c: &Cell, v: Value) {
    *c.borrow_mut() = v;
}

/// `$raf->Read($buff, $len)` into the `my $buff` cell.
fn read(it: &mut Interp, raf: &Value, buff: &Cell, len: Value) -> ER<Value> {
    raf_read_cell(it, raf, buff, len)
}

/// `$raf->ReadLine($buff)` into the `my $buff` cell.
fn read_line(it: &mut Interp, raf: &Value, buff: &Cell) -> ER<Value> {
    let mut a = [val(buff)];
    let r = pl::method_mut(it, raf, "ReadLine", &mut a, Want::Scalar)?;
    set(buff, a[0].clone());
    Ok(r.into_iter().next().unwrap_or_default())
}

fn seek(it: &mut Interp, raf: &Value, pos: Value, whence: i64) -> ER<bool> {
    Ok(pl::method(it, raf, "Seek", vec![pos, n(whence)])?.truthy())
}

fn warn(it: &mut Interp, et: &Value, msg: Value) -> ER<()> {
    pl::method(it, et, "Warn", vec![msg])?;
    Ok(())
}

fn cat(parts: &[&Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

fn handle_tag(it: &mut Interp, et: &Value, table: &Value, tag: Value, v: Value, opts: &[(&str, Value)]) -> ER<Value> {
    let mut args = vec![table.clone(), tag, v];
    for (k, x) in opts {
        args.push(pl::s(k));
        args.push(x.clone());
    }
    pl::method(it, et, "HandleTag", args)
}

/// `$$et{OPTIONS}{FastScan} and $$et{OPTIONS}{FastScan} == 3`
fn fast3(et: &Value) -> bool {
    let fs = et.get("OPTIONS").get("FastScan");
    fs.truthy() && pl::num_eq(&fs, &n(3))
}

fn process_ttc_impl(it: &mut Interp, et: &Value, dir_info: &Value) -> ER<Value> {
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);

    if !pl::num_eq(&read(it, &raf, &buff, n(12))?, &n(12)) {
        return Ok(n(0));
    }
    if !re(r"^ttcf\0[\x01\x02]\0\0", "").is_match(&val(&buff)) {
        return Ok(n(0));
    }
    call(it, "SetByteOrder", vec![pl::s("MM")])?;
    let num = call(it, "Get32u", vec![rf(&buff), n(8)])?;
    // might as well put a limit on the number of fonts we will parse (< 256)
    if !pl::lt(&num, &n(0x100)) {
        return Ok(n(0));
    }
    let want = pl::mul(&num, &n(4));
    if !pl::num_eq(&read(it, &raf, &buff, want.clone())?, &want) {
        return Ok(n(0));
    }
    pl::method(it, et, "SetFileType", vec![pl::s("TTC")])?;
    if fast3(et) {
        return Ok(n(1));
    }
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Font::Main")])?;
    handle_tag(it, et, &tag_table_ptr, pl::s("numfonts"), num.clone(), &[])?;
    // loop through all fonts in the collection
    let mut i = n(0);
    while pl::lt(&i, &num) {
        let nn = pl::add(&i, &n(1));
        pl::method(it, et, "VPrint", vec![n(0), cat(&[&pl::s("Font "), &nn, &pl::s(":\n")])])?;
        et.set("SET_GROUP1", cat(&[&pl::s("+"), &nn]));
        let offset = call(it, "Get32u", vec![rf(&buff), pl::mul(&i, &n(4))])?;
        if !seek(it, &raf, offset, 0)? {
            break;
        }
        if !process_otf_impl(it, et, dir_info)?.truthy() {
            break;
        }
        i = pl::add(&i, &n(1));
    }
    et.delete("SET_GROUP1");
    Ok(n(1))
}

/// Read information from a TrueType font collection (TTC) (refs 2,3)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref
/// Returns: 1 on success, 0 if this wasn't a valid TrueType font collection
pub fn process_ttc(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(process_ttc_impl(it, &arg(&a, 0), &arg(&a, 1))?)
}

fn process_table_entry_impl(it: &mut Interp, et: &Value, idx: &Value, tag: &Value, data_pt: &Value, offset: &Value, transformed: &Value) -> ER<Value> {
    let mut tag = tag.clone();
    let verbose = pl::method(it, et, "Options", vec![pl::s("Verbose")])?;
    let size = pl::length(&data_pt.deref());
    let process_tag = runtime::lex_hash(it, MODULE, "processTag");

    let offset = if offset.truthy() { offset.clone() } else { n(0) };
    if verbose.truthy() {
        re(r"([\0-\x1f\x7f-\xff])", "").subst(&mut tag, true, |c| pl::sprintf("\\x%.2x", &[n(cap(c, 1).to_bytes()[0] as i64)]).to_bytes().to_vec());
        let fmt = "%s%d) Tag '%s' (offset 0x%.4x, %d bytes)\n";
        let s = pl::sprintf(fmt, &[et.get("INDENT"), idx.clone(), tag.clone(), offset.clone(), size.clone()]);
        pl::method(it, et, "VPrint", vec![n(0), s])?;
        if pl::gt(&verbose, &n(2)) {
            pl::method(it, et, "VerboseDump", vec![data_pt.clone(), pl::s("Addr"), offset.clone()])?;
        }
        if !process_tag.get(tag.to_bytes()).truthy() {
            return Ok(Value::Undef);
        }
    }
    if transformed.truthy() || pl::lt(&size, &n(8)) {
        return Ok(Value::Undef);
    }
    if !pl::str_eq(&tag, "name") {
        let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Font::Main")])?;
        handle_tag(it, et, &tag_table_ptr, tag.clone(), Value::Undef, &[("DataPt", data_pt.clone()), ("Size", pl::length(&data_pt.deref()))])?;
        return Ok(Value::Undef);
    }
    // process the 'name' tag
    let entries = call(it, "Get16u", vec![data_pt.clone(), n(2)])?;
    let rec_end = pl::add(&n(6), &pl::mul(&entries, &n(12)));
    if pl::gt(&rec_end, &size) {
        warn(it, et, pl::s("Truncated name record"))?;
        return Ok(n(0));
    }
    let str_start = call(it, "Get16u", vec![data_pt.clone(), n(4)])?;
    if pl::lt(&str_start, &rec_end) || pl::gt(&str_start, &size) {
        warn(it, et, pl::s("Invalid string offset"))?;
        return Ok(n(0));
    }
    // parse language-tag record (in format 1 Naming table only) (ref 2)
    let lang_tag = Value::new_hash();
    if pl::num_eq(&call(it, "Get16u", vec![data_pt.clone(), n(0)])?, &n(1)) && pl::le(&pl::add(&rec_end, &n(2)), &size) {
        let lang_tags = call(it, "Get16u", vec![data_pt.clone(), rec_end.clone()])?;
        if lang_tags.truthy() && pl::lt(&pl::add(&pl::add(&rec_end, &n(2)), &pl::mul(&lang_tags, &n(4))), &size) {
            let mut i = n(0);
            while pl::lt(&i, &lang_tags) {
                let pt = pl::add(&pl::add(&rec_end, &n(2)), &pl::mul(&i, &n(4)));
                let lang_len = call(it, "Get16u", vec![data_pt.clone(), pt.clone()])?;
                // make sure the language string length is reasonable (UTF-16BE)
                if pl::num_eq(&lang_len, &n(0)) || (lang_len.to_i64() & 0x01) != 0 || pl::gt(&lang_len, &n(40)) {
                    break;
                }
                let lang_pt = pl::add(&call(it, "Get16u", vec![data_pt.clone(), pl::add(&pt, &n(2))])?, &str_start);
                if pl::gt(&pl::add(&lang_pt, &lang_len), &size) {
                    break;
                }
                let lang = substr(it, &data_pt.deref(), &lang_pt, Some(&lang_len))?;
                let mut lang = pl::method(it, et, "Decode", vec![lang, pl::s("UCS2"), pl::s("MM"), pl::s("UTF8")])?;
                pl::tr(&mut lang, "-_a-zA-Z0-9", "", "dc"); // remove naughty characters
                lang_tag.set(pl::add(&i, &n(0x8000)).to_bytes(), lang);
                i = pl::add(&i, &n(1));
            }
        }
    }
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Font::Name")])?;
    let old_indent = et.get("INDENT");
    et.set("INDENT", cat(&[&old_indent, &pl::s("| ")]));
    if verbose.truthy() {
        pl::method(it, et, "VerboseDir", vec![pl::s("Name"), entries.clone()])?;
    }
    let tt_platform = runtime::lex_hash(it, MODULE, "ttPlatform");
    let tt_charset = runtime::lex_hash(it, MODULE, "ttCharset");
    let tt_lang = pl::pkg_hash(it, "Image::ExifTool::Font::ttLang");
    let mut i = n(0);
    while pl::lt(&i, &entries) {
        let pt = pl::add(&n(6), &pl::mul(&i, &n(12)));
        let platform = call(it, "Get16u", vec![data_pt.clone(), pt.clone()])?;
        let encoding = call(it, "Get16u", vec![data_pt.clone(), pl::add(&pt, &n(2))])?;
        let mut lang_id = call(it, "Get16u", vec![data_pt.clone(), pl::add(&pt, &n(4))])?;
        let name_id = call(it, "Get16u", vec![data_pt.clone(), pl::add(&pt, &n(6))])?;
        let str_len = call(it, "Get16u", vec![data_pt.clone(), pl::add(&pt, &n(8))])?;
        let str_pt = pl::add(&call(it, "Get16u", vec![data_pt.clone(), pl::add(&pt, &n(10))])?, &str_start);
        if pl::le(&pl::add(&str_pt, &str_len), &size) {
            let mut v = substr(it, &data_pt.deref(), &str_pt, Some(&str_len))?;
            let (mut lang, mut charset, mut extra) = (Value::Undef, Value::Undef, Value::Undef);
            let sys = tt_platform.get(platform.to_bytes());
            // translate from specified encoding
            if sys.truthy() {
                let l = tt_lang.get(sys.to_bytes()).get(lang_id.to_bytes());
                lang = if l.truthy() { l } else { lang_tag.get(lang_id.to_bytes()) };
                charset = tt_charset.get(sys.to_bytes()).get(encoding.to_bytes());
                if !charset.truthy() {
                    if charset.is_undef() && !et.get("FontWarn").truthy() {
                        warn(it, et, cat(&[&pl::s("Unknown "), &sys, &pl::s(" character set ("), &encoding, &pl::s(")")]))?;
                        et.set("FontWarn", n(1));
                    }
                } else {
                    // translate to ExifTool character set
                    v = pl::method(it, et, "Decode", vec![v, charset.clone()])?;
                }
            } else {
                warn(it, et, cat(&[&pl::s("Unknown platform ("), &platform, &pl::s(") for name "), &name_id]))?;
            }
            // get the tagInfo for our specific language (use 'en' for default)
            let mut tag_info = pl::method(it, et, "GetTagInfo", vec![tag_table_ptr.clone(), name_id.clone()])?;
            if tag_info.truthy() && lang.truthy() && !pl::str_eq(&lang, "en") {
                let lang_info = call(it, "GetLangInfo", vec![tag_info.clone(), lang.clone()])?;
                if lang_info.truthy() {
                    tag_info = lang_info;
                }
            }
            if verbose.truthy() {
                if pl::gt(&lang_id, &n(0x400)) {
                    lang_id = pl::sprintf("0x%x", &[lang_id]);
                }
                let or_unknown = |x: &Value| if x.truthy() { x.clone() } else { pl::s("Unknown") };
                extra = cat(&[
                    &pl::s(", Plat="),
                    &platform,
                    &pl::s("/"),
                    &or_unknown(&sys),
                    &pl::s(", "),
                    &pl::s("Enc="),
                    &encoding,
                    &pl::s("/"),
                    &or_unknown(&charset),
                    &pl::s(", "),
                    &pl::s("Lang="),
                    &lang_id,
                    &pl::s("/"),
                    &or_unknown(&lang),
                ]);
            }
            handle_tag(
                it,
                et,
                &tag_table_ptr,
                name_id,
                v,
                &[("TagInfo", tag_info), ("DataPt", data_pt.clone()), ("DataPos", offset.clone()), ("Start", str_pt), ("Size", str_len), ("Index", i.clone()), ("Extra", extra)],
            )?;
        }
        i = pl::add(&i, &n(1));
    }
    et.set("INDENT", old_indent);
    Ok(if verbose.truthy() { Value::Undef } else { n(0) })
}

/// Process an OTF tag table entry (refs 1,2)
/// Inputs: 0) ExifTool ref, 1) entry index, 2) tag, 3) data ref,
///         4) offset if uncompressed, 5) true to skip because data is transformed
/// Returns: undef on success, 0 to stop processing table, or error string otherwise
pub fn process_table_entry(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(process_table_entry_impl(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3), &arg(&a, 4), &arg(&a, 5))?)
}

fn process_otf_impl(it: &mut Interp, et: &Value, dir_info: &Value) -> ER<Value> {
    let raf = dir_info.get("RAF");
    let (tbl, buff) = (cell(Value::Undef), cell(Value::Undef));
    let b = dir_info.get("Base");
    let base = if b.truthy() { b } else { n(0) };

    if !pl::num_eq(&read(it, &raf, &buff, n(12))?, &n(12)) {
        return Ok(n(0));
    }
    let Some(m) = re(r"^(\0\x01\0\0|OTTO|true|typ1|\xa5(kbd|lst))[\0\x01]", "").m(&val(&buff)) else {
        return Ok(n(0));
    };

    pl::method(it, et, "SetFileType", vec![pl::s(if pl::str_eq(&cap(&m, 1), "OTTO") { "OTF" } else { "TTF" })])?;
    if fast3(et) {
        return Ok(n(1));
    }
    call(it, "SetByteOrder", vec![pl::s("MM")])?;
    let num_tables = call(it, "Get16u", vec![rf(&buff), n(4)])?;
    if !(pl::gt(&num_tables, &n(0)) && pl::lt(&num_tables, &n(0x200))) {
        return Ok(n(0));
    }
    let len = pl::mul(&num_tables, &n(16));
    if !pl::num_eq(&read(it, &raf, &tbl, len.clone())?, &len) {
        return Ok(n(0));
    }

    let verbose = pl::method(it, et, "Options", vec![pl::s("Verbose")])?;
    let process_tag = runtime::lex_hash(it, MODULE, "processTag");

    let (mut pos, mut i) = (n(0), n(0));
    while pl::lt(&pos, &len) {
        'body: {
            // look for tags to process
            let tag = pl::substr(&val(&tbl), pos.to_i64(), Some(4));
            if !(process_tag.get(tag.to_bytes()).truthy() || verbose.truthy()) {
                break 'body;
            }
            let offset = call(it, "Get32u", vec![rf(&tbl), pl::add(&pos, &n(8))])?;
            let size = call(it, "Get32u", vec![rf(&tbl), pl::add(&pos, &n(12))])?;
            if !(seek(it, &raf, pl::add(&offset, &base), 0)? && pl::num_eq(&read(it, &raf, &buff, size.clone())?, &size)) {
                warn(it, et, cat(&[&pl::s("Error reading '"), &tag, &pl::s("' data")]))?;
                break 'body;
            }
            let err = process_table_entry_impl(it, et, &i, &tag, &rf(&buff), &offset, &Value::Undef)?;
            if err.truthy() {
                warn(it, et, err)?;
                return Ok(n(1));
            }
            if !err.is_undef() {
                return Ok(n(1));
            }
        }
        pos = pl::add(&pos, &n(16));
        i = pl::add(&i, &n(1));
    }
    Ok(n(1))
}

/// Read information from a TrueType font file (OTF or TTF) (refs 1,2)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref
/// Returns: 1 on success, 0 if this wasn't a valid TrueType font file
pub fn process_otf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(process_otf_impl(it, &arg(&a, 0), &arg(&a, 1))?)
}

fn process_afm_impl(it: &mut Interp, et: &Value, dir_info: &Value) -> ER<Value> {
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);
    let mut comment = Value::Undef;

    runtime::require(it, "Image::ExifTool::PostScript")?;
    let sep = pl::call(it, "Image::ExifTool::PostScript::GetInputRecordSeparator", vec![raf.clone()])?;
    pl::local_global(it, b'$', "main::/", |it| {
        it.set_scalar("main::/", sep);
        read_line(it, &raf, &buff)?;
        let Some(m) = re(r"^Start(Comp|Master)?FontMetrics\s+\d+", "").m(&val(&buff)) else {
            return Ok(n(0));
        };
        let one = cap(&m, 1);
        let ftyp = if one.truthy() {
            if pl::str_eq(&one, "Comp") {
                "ACFM"
            } else {
                "AMFM"
            }
        } else {
            "AFM"
        };
        pl::method(it, et, "SetFileType", vec![pl::s(ftyp), pl::s("application/x-font-afm")])?;
        if fast3(et) {
            return Ok(n(1));
        }
        let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Font::AFM")])?;

        loop {
            if !read_line(it, &raf, &buff)?.truthy() {
                break;
            }
            if !comment.is_undef() && !re(r"^Comment\s", "").is_match(&val(&buff)) {
                pl::method(it, et, "FoundTag", vec![pl::s("Comment"), comment.clone()])?;
                comment = Value::Undef;
            }
            let Some(m) = re(r"^(\w+)\s+(.*?)[\x0d\x0a]", "").m(&val(&buff)) else {
                continue;
            };
            let (mut tag, mut v) = (cap(&m, 1), cap(&m, 2));
            if pl::str_eq(&tag, "Comment") {
                if let Some(m) = re(r"^(Creation Date):\s+(.*)", "").m(&v) {
                    (tag, v) = (cap(&m, 1), cap(&m, 2));
                }
            }
            re(r"^\((.*)\)$", "").subst(&mut v, false, |c| cap(c, 1).to_bytes().to_vec()); // (some values may be in brackets)
            if pl::str_eq(&tag, "Comment") {
                // concatinate all comments into a single value
                comment = if !comment.is_undef() { cat(&[&comment, &pl::s("\n"), &v]) } else { v };
                continue;
            }
            if !handle_tag(it, et, &tag_table_ptr, tag.clone(), v, &[])?.truthy() {
                // end parsing if we start any subsection
                if re(r"^Start", "").is_match(&tag) && !pl::str_eq(&tag, "StartDirection") {
                    break;
                }
            }
        }
        Ok(n(1))
    })
}

/// Read information from an Adobe Font Metrics file (AFM, ACFM, AMFM) (ref 6)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref
/// Returns: 1 on success, 0 if this wasn't a recognized AFM-type file
pub fn process_afm(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(process_afm_impl(it, &arg(&a, 0), &arg(&a, 1))?)
}

fn read255_uint16_impl(it: &mut Interp, raf: &Value) -> ER<Value> {
    let buff = cell(Value::Undef);
    if !read(it, raf, &buff, n(1))?.truthy() {
        return Ok(Value::Undef);
    }
    let mut v = pl::unpack1("C", &val(&buff))?;
    if pl::num_eq(&v, &n(253)) {
        if !pl::num_eq(&read(it, raf, &buff, n(2))?, &n(2)) {
            return Ok(Value::Undef);
        }
        v = pl::unpack1("n", &val(&buff))?;
    } else if pl::num_eq(&v, &n(254)) {
        if !read(it, raf, &buff, n(1))?.truthy() {
            return Ok(Value::Undef);
        }
        v = pl::add(&pl::unpack1("C", &val(&buff))?, &n(253 * 2));
    } else if pl::num_eq(&v, &n(255)) {
        if !read(it, raf, &buff, n(1))?.truthy() {
            return Ok(Value::Undef);
        }
        v = pl::add(&pl::unpack1("C", &val(&buff))?, &n(253));
    }
    Ok(v)
}

/// Read WOFF2 255UInt16 integer (ref 8)
/// Inputs: 0) raf ref
/// Returns: value, or undef on error
pub fn read255_uint16(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(read255_uint16_impl(it, &arg(&a, 0))?)
}

fn read_uint_base128_impl(it: &mut Interp, raf: &Value) -> ER<Value> {
    let buff = cell(Value::Undef);
    let mut v: u64 = 0;
    for k in 0..=4 {
        if !read(it, raf, &buff, n(1))?.truthy() {
            return Ok(Value::Undef);
        }
        let byte = pl::unpack1("C", &val(&buff))?.to_i64() as u64;
        if k == 0 && byte == 0x80 {
            return Ok(Value::Undef);
        }
        if v & 0xfe000000 != 0 {
            return Ok(Value::Undef);
        }
        v = (v << 7) | (byte & 0x7f);
        if byte & 0x80 == 0 {
            return Ok(n(v as i64));
        }
    }
    Ok(Value::Undef) // can't be longer than 5 bytes
}

/// Read WOFF2 UIntBase128 integer (ref 8)
/// Inputs: 0) raf ref
/// Returns: value, or undef on error
pub fn read_uint_base128(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(read_uint_base128_impl(it, &arg(&a, 0))?)
}

fn uncompress_impl(it: &mut Interp, et: &Value, data_pt: &Value) -> ER<Value> {
    let mut stat = Value::Undef;
    if pl::eval_block(it, |it| runtime::require(it, "Compress::Zlib"))?.is_none() {
        warn(it, et, pl::s("Install Compress::Zlib to read compressed metadata"))?;
        return Ok(n(0));
    }
    let inflate = pl::call(it, "Compress::Zlib::inflateInit", vec![])?;
    if inflate.truthy() {
        // (inflate() may consume its input, so $$dataPt is passed as an alias)
        let mut args = [data_pt.deref()];
        let r = pl::method_mut(it, &inflate, "inflate", &mut args, Want::List)?;
        pl::set_deref(data_pt, args[0].clone())?;
        pl::set_deref(data_pt, arg(&r, 0))?;
        stat = arg(&r, 1);
    }
    if !(inflate.truthy() && pl::num_eq(&stat, &pl::call(it, "Compress::Zlib::Z_STREAM_END", vec![])?)) {
        warn(it, et, pl::s("Error uncompressing metadata"))?;
        return Ok(n(0));
    }
    Ok(n(1))
}

/// Uncompress data
/// Inputs: 0) ExifTool ref, 1) data ref
/// Returns: true on success
pub fn uncompress(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(uncompress_impl(it, &arg(&a, 0), &arg(&a, 1))?)
}

fn unbrotli_impl(it: &mut Interp, et: &Value, data_pt: &Value) -> ER<Value> {
    if pl::eval_block(it, |it| runtime::require(it, "IO::Uncompress::Brotli"))?.is_none() {
        warn(it, et, pl::s("Install IO::Compress::Brotli to decode Brotli-compressed metadata"))?;
        return Ok(n(0));
    }
    pl::eval_block(it, |it| {
        let v = pl::call(it, "IO::Uncompress::Brotli::unbro", vec![data_pt.deref(), n(100000000)])?;
        pl::set_deref(data_pt, v)
    })?;
    if it.get_scalar("main::@").truthy() {
        warn(it, et, pl::s("Error decoding metadata"))?;
        warn(it, et, pl::s("Try updating to IO::Uncompress::Brotli 0.004 or later"))?;
        return Ok(n(0));
    }
    Ok(n(1))
}

/// Brotli uncompress data
/// Inputs: 0) ExifTool ref, 1) data ref
/// Returns: true on success
pub fn unbrotli(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(unbrotli_impl(it, &arg(&a, 0), &arg(&a, 1))?)
}

fn process_woff_impl(it: &mut Interp, et: &Value, dir_info: &Value) -> ER<Value> {
    let raf = dir_info.get("RAF");
    let (buff, tbl) = (cell(Value::Undef), cell(Value::Undef));
    if !(seek(it, &raf, n(0), 0)? && pl::num_eq(&read(it, &raf, &buff, n(48))?, &n(48))) {
        return Ok(n(0));
    }
    let Some(m) = re(r"^(wOF[F2])", "").m(&val(&buff)) else {
        return Ok(n(0));
    };
    let (type_, off) = if pl::str_eq(&cap(&m, 1), "wOFF") { ("woff", 20) } else { ("woff2", 24) };
    pl::method(it, et, "SetFileType", vec![pl::uc(&pl::s(type_)), pl::s(&format!("font/{type_}"))])?;
    call(it, "SetByteOrder", vec![pl::s("MM")])?;
    let flavor = pl::substr(&val(&buff), 4, Some(4));
    let num_tables = call(it, "Get16u", vec![rf(&buff), n(12)])?;
    let u = pl::unpack(&format!("x{off}nnNN"), &val(&buff))?;
    let (vh, vl, meta_pos, meta_len) = (arg(&u, 0), arg(&u, 1), arg(&u, 2), arg(&u, 3));
    let verbose = pl::method(it, et, "Options", vec![pl::s("Verbose")])?;
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Font::Main")])?;
    handle_tag(it, et, &tag_table_ptr, pl::s("WOFFVersion"), cat(&[&vh, &pl::s("."), &vl]), &[])?;
    let process_tag = runtime::lex_hash(it, MODULE, "processTag");
    //
    // read font table
    //
    if type_ == "woff" {
        let want = pl::mul(&num_tables, &n(20));
        if !(seek(it, &raf, n(off + 24), 0)? && pl::num_eq(&read(it, &raf, &tbl, want.clone())?, &want)) {
            warn(it, et, pl::s("Error reading font table"))?;
            return Ok(n(1));
        }
        let mut i = n(0);
        'tables: while pl::lt(&i, &num_tables) {
            'body: {
                let pt = pl::mul(&i, &n(20));
                let u = pl::unpack(&format!("x{}a4N3", pt.to_text()), &val(&tbl))?;
                let (tag, pos, comp_len, len) = (arg(&u, 0), arg(&u, 1), arg(&u, 2), arg(&u, 3));
                if !(process_tag.get(tag.to_bytes()).truthy() || verbose.truthy()) {
                    break 'body;
                }
                if !(seek(it, &raf, pos.clone(), 0)? && pl::num_eq(&read(it, &raf, &buff, comp_len.clone())?, &comp_len)) {
                    warn(it, et, pl::s("Bad font table entry"))?;
                    return Ok(n(1));
                }
                let mut data_pos = Value::Undef;
                if comp_len.to_bytes() == len.to_bytes() {
                    data_pos = pos;
                } else if !uncompress_impl(it, et, &rf(&buff))?.truthy() {
                    break 'body;
                }
                let err = process_table_entry_impl(it, et, &i, &tag, &rf(&buff), &data_pos, &Value::Undef)?;
                if err.truthy() {
                    warn(it, et, err)?;
                    return Ok(n(1));
                }
                if !err.is_undef() {
                    break 'tables;
                }
            }
            i = pl::add(&i, &n(1));
        }
    } else {
        // WOFF2
        let comp_size = call(it, "Get32u", vec![rf(&buff), n(20)])?;
        let mut err = false;
        let mut entry: Vec<(Value, Value, Value, Value)> = Vec::new();
        if !seek(it, &raf, n(off + 24), 0)? {
            warn(it, et, pl::s("Error seeking to font table"))?;
            return Ok(n(1));
        }
        let known_tags = runtime::lex_array(it, MODULE, "knownTags");
        let mut i = n(0);
        while pl::lt(&i, &num_tables) {
            if !read(it, &raf, &buff, n(1))?.truthy() {
                err = true;
                break;
            }
            let flags = pl::unpack1("C", &val(&buff))?;
            let tag = cell(known_tags.at(flags.to_i64() & 0x3f));
            if !val(&tag).truthy() && !pl::num_eq(&read(it, &raf, &tag, n(4))?, &n(4)) {
                err = true;
                break;
            }
            let mut len = read_uint_base128_impl(it, &raf)?;
            if len.is_undef() {
                err = true;
                break;
            }
            let mut transformed = Value::Undef;
            let gl = pl::str_eq(&val(&tag), "glyf") || pl::str_eq(&val(&tag), "loca");
            if gl ^ (flags.to_i64() & 0xc0 != 0) {
                // a non-null transform was used
                len = read_uint_base128_impl(it, &raf)?;
                transformed = n(1);
            }
            // save information about this entry for later
            entry.push((i.clone(), val(&tag), len, transformed));
            i = pl::add(&i, &n(1));
        }
        if err {
            warn(it, et, pl::s("Error reading font table"))?;
            return Ok(n(1));
        }
        // skip the collection table if necessary
        if pl::str_eq(&flavor, "ttcf") {
            if !seek(it, &raf, n(4), 1)? {
                warn(it, et, pl::s("Seek error"))?;
                return Ok(n(1));
            }
            let nn = read255_uint16_impl(it, &raf)?;
            if !(!nn.is_undef() && seek(it, &raf, n(4), 1)?) {
                warn(it, et, pl::s("Error reading collection table"))?;
                return Ok(n(1));
            }
            if !seek(it, &raf, n(4), 1)? {
                // ("$err = 1, last" outside of any loop in the Perl)
                return die("Can't \"last\" outside a loop block");
            }
            let mut i = n(0);
            while pl::lt(&i, &nn) {
                if read255_uint16_impl(it, &raf)?.is_undef() {
                    err = true;
                    break;
                }
                i = pl::add(&i, &n(1));
            }
            if err {
                warn(it, et, pl::s("Error reading collection directory"))?;
                return Ok(n(1));
            }
        }
        if !pl::num_eq(&read(it, &raf, &buff, comp_size.clone())?, &comp_size) {
            warn(it, et, pl::s("Error reading font data"))?;
            return Ok(n(1));
        }
        if !unbrotli_impl(it, et, &rf(&buff))?.truthy() {
            return Ok(n(1));
        }
        // after all that exhausting and frankly unnecessary work (poor file design),
        // we finally have the uncompressed font data so we can process the table entries
        let mut pos = n(0);
        for (i, tag, len, transformed) in entry {
            if process_tag.get(tag.to_bytes()).truthy() || verbose.truthy() {
                let dat = substr(it, &val(&buff), &pos, Some(&len))?;
                let err = process_table_entry_impl(it, et, &i, &tag, &pl::sref(dat), &Value::Undef, &transformed)?;
                if err.truthy() {
                    warn(it, et, err)?;
                    return Ok(n(1));
                }
                if !err.is_undef() {
                    break;
                }
            }
            pos = pl::add(&pos, &len);
        }
    }
    //
    // read compressed XML-format metadata (NC)
    //
    if meta_len.truthy() {
        if !(seek(it, &raf, meta_pos, 0)? && pl::num_eq(&read(it, &raf, &buff, meta_len.clone())?, &meta_len)) {
            warn(it, et, pl::s("Error reading metadata"))?;
            return Ok(n(1));
        }
        if type_ == "woff" {
            if !uncompress_impl(it, et, &rf(&buff))?.truthy() {
                return Ok(n(1));
            }
        } else if !unbrotli_impl(it, et, &rf(&buff))?.truthy() {
            // WOFF2
            return Ok(n(1));
        }
        // (we don't properly support XML structures)
        let old_struct = pl::method(it, et, "Options", vec![pl::s("Struct")])?;
        pl::method(it, et, "Options", vec![pl::s("Struct"), n(0)])?;
        handle_tag(it, et, &tag_table_ptr, pl::s("XML"), val(&buff), &[])?;
        pl::method(it, et, "Options", vec![pl::s("Struct"), old_struct])?;
    }
    Ok(n(1))
}

/// Read information from WOFF/WOFF2 font files
/// Inputs: 0) ExifTool ref, 1) dirInfo ref
/// Returns: 1 on success, 0 if this wasn't a recognized WOFF file
pub fn process_woff(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(process_woff_impl(it, &arg(&a, 0), &arg(&a, 1))?)
}

/// The PFM test of ProcessFont, evaluated in the Perl's order.
fn is_pfm(it: &mut Interp, raf: &Value, buff: &Cell, buf2: &Cell) -> ER<bool> {
    if !re(r"^\0[\x01\x02]", "").is_match(&val(buff)) || !seek(it, raf, n(0), 2)? {
        return Ok(false);
    }
    // validate file size
    if !pl::gt(&pl::method(it, raf, "Tell", vec![])?, &n(117)) {
        return Ok(false);
    }
    let size = pl::unpack1("x2V", &val(buff))?;
    if !pl::num_eq(&pl::method(it, raf, "Tell", vec![])?, &size) {
        return Ok(false);
    }
    // read PFM header
    if !(seek(it, raf, n(0), 0)? && pl::num_eq(&read(it, raf, buff, n(117))?, &n(117))) {
        return Ok(false);
    }
    // validate "DeviceType" string (must be "PostScript\0")
    if !call(it, "SetByteOrder", vec![pl::s("II")])?.truthy() {
        return Ok(false);
    }
    let off = call(it, "Get32u", vec![rf(buff), n(101)])?;
    if !seek(it, raf, off, 0)? {
        return Ok(false);
    }
    // the DeviceType should be "PostScript\0", but FontForge
    // incorrectly writes "Postscript\0", so ignore case
    Ok(pl::num_eq(&read(it, raf, buf2, n(11))?, &n(11)) && pl::str_eq(&pl::lc(&val(buf2)), "postscript\0"))
}

/// Read information from various format font files
/// Inputs: 0) ExifTool ref, 1) dirInfo ref
/// Returns: 1 on success, 0 if this wasn't a recognized Font file
pub fn process_font(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let (buff, buf2) = (cell(Value::Undef), cell(Value::Undef));
    let rtn_val;
    if !(read(it, &raf, &buff, n(24))?.truthy() && seek(it, &raf, n(0), 0)?) {
        return ret(n(0));
    }
    let pfa = re(r"^(.{6})?%!(PS-(AdobeFont-|Bitstream )|FontType1-)", "s");
    if re(r"^(\0\x01\0\0|OTTO|true|typ1)[\0\x01]", "").is_match(&val(&buff)) {
        // OTF, TTF
        rtn_val = process_otf_impl(it, &et, &dir_info)?;
    } else if re(r"^ttcf\0[\x01\x02]\0\0", "").is_match(&val(&buff)) {
        // TTC
        rtn_val = process_ttc_impl(it, &et, &dir_info)?;
    } else if re(r"^Start(Comp|Master)?FontMetrics\s+\d+", "s").is_match(&val(&buff)) {
        // AFM
        rtn_val = process_afm_impl(it, &et, &dir_info)?;
    } else if let Some(m) = pfa.m(&val(&buff)) {
        // PFA, PFB
        if cap(&m, 1).truthy() && seek(it, &raf, n(6), 0)? {
            pl::method(it, &et, "SetFileType", vec![pl::s("PFB")])?;
        }
        runtime::require(it, "Image::ExifTool::PostScript")?;
        rtn_val = pl::call(it, "Image::ExifTool::PostScript::ProcessPS", vec![et.clone(), dir_info.clone()])?;
    } else if is_pfm(it, &raf, &buff, &buf2)? {
        // PFM
        pl::method(it, &et, "SetFileType", vec![pl::s("PFM")])?;
        if fast3(&et) {
            return ret(n(1));
        }
        call(it, "SetByteOrder", vec![pl::s("II")])?;
        let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Font::Main")])?;
        // process the PFM header
        handle_tag(it, &et, &tag_table_ptr, pl::s("PFM"), val(&buff), &[])?;
        // extract the font names
        let name_off = call(it, "Get32u", vec![rf(&buff), n(105)])?;
        if seek(it, &raf, name_off, 0)? && read(it, &raf, &buff, n(256))?.truthy() {
            if let Some(m) = re(r"^([\x20-\xff]+)\0([\x20-\xff]+)\0", "").m(&val(&buff)) {
                handle_tag(it, &et, &tag_table_ptr, pl::s("fontname"), cap(&m, 1), &[])?;
                handle_tag(it, &et, &tag_table_ptr, pl::s("postfont"), cap(&m, 2), &[])?;
            }
        }
        rtn_val = n(1);
    } else if re(r"^(wOF[F2])", "").is_match(&val(&buff)) {
        rtn_val = process_woff_impl(it, &et, &dir_info)?;
    } else {
        rtn_val = n(0);
    }
    ret(rtn_val)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Font::ProcessTTC", process_ttc);
    it.define_native("Image::ExifTool::Font::ProcessTableEntry", process_table_entry);
    it.define_native("Image::ExifTool::Font::ProcessOTF", process_otf);
    it.define_native("Image::ExifTool::Font::ProcessAFM", process_afm);
    it.define_native("Image::ExifTool::Font::Read255UInt16", read255_uint16);
    it.define_native("Image::ExifTool::Font::ReadUIntBase128", read_uint_base128);
    it.define_native("Image::ExifTool::Font::Uncompress", uncompress);
    it.define_native("Image::ExifTool::Font::Unbrotli", unbrotli);
    it.define_native("Image::ExifTool::Font::ProcessWOFF", process_woff);
    it.define_native("Image::ExifTool::Font::ProcessFont", process_font);
}
