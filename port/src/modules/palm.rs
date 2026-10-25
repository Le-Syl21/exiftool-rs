//! Image::ExifTool::Palm (Image/ExifTool/Palm.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Palm Database files

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::Value;
use crate::pl::{self, arg, n, ret};

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `$raf->Read($buff, $len)` into the `my $buff` cell.
fn read(it: &mut Interp, raf: &Value, buff: &Cell, len: &Value) -> ER<Value> {
    raf_read_cell(it, raf, buff, len.clone())
}

fn seek(it: &mut Interp, raf: &Value, pos: Value, whence: i64) -> ER<bool> {
    Ok(pl::method(it, raf, "Seek", vec![pos, n(whence)])?.truthy())
}

fn val(c: &Cell) -> Value {
    c.borrow().clone()
}

/// `\$buff`
fn rf(c: &Cell) -> Value {
    Value::ScalarRef(c.clone())
}

fn warn(it: &mut Interp, et: &Value, msg: &str) -> ER<()> {
    pl::method(it, et, "Warn", vec![pl::s(msg)])?;
    Ok(())
}

/// Process the MOBI extended header
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 (EXTH should have already been validated)
pub fn process_exth(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data_pos = dir_info.get("DataPos");
    let enc = {
        let e = dir_info.get("Encoding");
        if e.truthy() {
            e
        } else {
            pl::s("UTF8")
        }
    };
    let dir_len = pl::length(&data_pt.deref());

    pl::method(it, &et, "VerboseDir", vec![pl::s("EXTH"), dir_info.get("NumEntries"), dir_len.clone()])?;

    // process the EXTH entries
    let (mut index, mut pos) = (n(0), n(0));
    loop {
        if pl::gt(&pl::add(&pos, &n(8)), &dir_len) {
            break;
        }
        let tag = call(it, "Get32u", vec![data_pt.clone(), pos.clone()])?;
        let len = call(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(4))])?;
        if pl::lt(&len, &n(8)) || pl::gt(&pl::add(&pos, &len), &dir_len) {
            break;
        }
        let key = pl::method(
            it,
            &et,
            "HandleTag",
            vec![
                tag_table_ptr.clone(),
                tag,
                Value::Undef,
                pl::s("DataPt"),
                data_pt.clone(),
                pl::s("DataPos"),
                data_pos.clone(),
                pl::s("Start"),
                pl::add(&pos, &n(8)),
                pl::s("Size"),
                pl::sub(&len, &n(8)),
                pl::s("Index"),
                index.clone(),
            ],
        )?;
        // recode text if necessary
        if key.truthy() {
            let values = et.get("VALUE");
            let k = key.to_bytes();
            let v = pl::method(it, &et, "Decode", vec![values.get(&k), enc.clone()])?;
            values.set(&k, v);
        }
        pos = pl::add(&pos, &len);
        index = pl::add(&index, &n(1));
    }
    ret(n(1))
}

/// Extract information from a Palm DB file
/// Inputs: 0) ExifTool ref, 1) dirInfo reference
/// Returns: 1 if this was a recognized PDB file, 0 otherwise
pub fn process_pdb(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let (buff, buf2) = (cell(Value::Undef), cell(Value::Undef));
    let mut size = Value::Undef;
    let mut enc = Value::Undef;
    let _verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;

    // verify this is a valid Palm DB file
    if !pl::num_eq(&read(it, &raf, &buff, &n(86))?, &n(86)) {
        return ret(n(0));
    }
    let palm_types = crate::runtime::lex_hash(it, "Image::ExifTool::Palm", "palmTypes");
    let key = substr(it, &val(&buff), &n(60), Some(&n(8)))?;
    let type_ = palm_types.get(key.to_bytes());
    if !type_.truthy() {
        return ret(n(0));
    }
    //
    // Read and process the Palm DB file header
    //
    let ft = if type_.eq_str("Mobipocket") { "MOBI" } else { "PDB" };
    pl::method(it, &et, "SetFileType", vec![pl::s(ft)])?;
    call(it, "SetByteOrder", vec![pl::s("MM")])?;

    let mut tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Palm::Main")])?;
    let d = Value::new_hash();
    d.set("DataPt", rf(&buff));
    pl::method(it, &et, "ProcessDirectory", vec![d, tag_table_ptr.clone()])?;

    if !(type_.eq_str("Mobipocket") && call(it, "Get16u", vec![rf(&buff), n(76)])?.truthy()) {
        return ret(n(1));
    }
    //
    // Read and process MOBI header (should be the first record)
    //
    let offset = call(it, "Get32u", vec![rf(&buff), n(78)])?; // get offset to first record
    if !(seek(it, &raf, offset.clone(), 0)? && pl::num_eq(&read(it, &raf, &buff, &n(274))?, &n(274))) {
        warn(it, &et, "Truncated MOBI header")?;
        return ret(n(1));
    }
    if !substr(it, &val(&buff), &n(16), Some(&n(4)))?.eq_str("MOBI") {
        warn(it, &et, "Invalid MOBI header")?;
        return ret(n(1));
    }
    tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Palm::MOBI")])?;
    let d = Value::new_hash();
    d.set("DataPt", rf(&buff));
    pl::method(it, &et, "ProcessDirectory", vec![d, tag_table_ptr.clone()])?;

    // get text encoding
    if et.get("CodePage").truthy() {
        let charset_name = pl::pkg_hash(it, "Image::ExifTool::charsetName");
        enc = charset_name.get(pl::concat(&pl::s("cp"), &et.get("CodePage")).to_bytes());
    }
    if !enc.truthy() {
        enc = pl::s("UTF8");
    }

    // extract the BookName string
    let off = call(it, "Get32u", vec![rf(&buff), n(84)])?;
    let mut len = call(it, "Get32u", vec![rf(&buff), n(88)])?;

    if !(seek(it, &raf, pl::add(&offset, &off), 0)? && pl::num_eq(&read(it, &raf, &buf2, &len)?, &len)) {
        *buf2.borrow_mut() = pl::s("<err>");
    }
    let book_name = pl::method(it, &et, "Decode", vec![val(&buf2), enc.clone()])?;
    et.get("VALUE").set("BookName", book_name);
    //
    // Process the MOBI extended header if it exists
    //
    // first, check the flag bit to see if the EXTH record should exist
    let flag = call(it, "Get32u", vec![rf(&buff), n(128)])?;
    if (flag.to_i64() & 0x40) == 0 {
        return ret(n(1)); // check extended header flag
    }

    len = pl::add(&call(it, "Get32u", vec![rf(&buff), n(20)])?, &n(16)); // MOBI header length (including PalmDOC header)

    let ok = seek(it, &raf, pl::add(&offset, &len), 0)? && pl::num_eq(&read(it, &raf, &buf2, &n(12))?, &n(12)) && substr(it, &val(&buf2), &n(0), Some(&n(4)))?.eq_str("EXTH") && {
        size = call(it, "Get32u", vec![rf(&buf2), n(4)])?;
        pl::gt(&size, &n(12))
    };
    if !ok {
        warn(it, &et, "Invalid MOBI extended header")?;
        return ret(n(1));
    }

    // read and process the MOBI extended header
    size = pl::sub(&size, &n(12));
    if !pl::num_eq(&read(it, &raf, &buff, &size)?, &size) {
        warn(it, &et, "Truncated MOBI extended header")?;
        return ret(n(1));
    }
    let sub_dir_info = Value::new_hash();
    sub_dir_info.set("DataPt", rf(&buff));
    sub_dir_info.set("DataPos", pl::add(&pl::add(&offset, &len), &n(12)));
    sub_dir_info.set("NumEntries", call(it, "Get32u", vec![rf(&buf2), n(8)])?);
    sub_dir_info.set("Encoding", enc);
    tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Palm::EXTH")])?;
    pl::method(it, &et, "ProcessDirectory", vec![sub_dir_info, tag_table_ptr])?;

    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Palm::ProcessEXTH", process_exth);
    it.define_native("Image::ExifTool::Palm::ProcessPDB", process_pdb);
}
