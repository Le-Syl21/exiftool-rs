//! Image::ExifTool::PSP (Image/ExifTool/PSP.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Paint Shop Pro meta information

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::Value;
use crate::pl::{self, arg, n, re, ret};

/// `$raf->Read(...)` runs as a perl statement (random_access::perl_eval),
/// whose errors would name "(eval N)": an invocant that is not an object
/// gives perl's error here instead, at the native's location.
fn need_obj(v: &Value, method: &str) -> ER<()> {
    match v {
        Value::Undef => crate::interp::eval::die(format!("Can't call method \"{method}\" on an undefined value")),
        Value::HashRef(h) if h.borrow().blessed.is_none() => crate::interp::eval::die(format!("Can't call method \"{method}\" on unblessed reference")),
        Value::ArrayRef(_) | Value::ScalarRef(_) | Value::Code(_) => crate::interp::eval::die(format!("Can't call method \"{method}\" on unblessed reference")),
        _ => Ok(()),
    }
}

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `$raf->Read($buff, $len)` into the `my $buff` cell.
fn read(it: &mut Interp, raf: &Value, buff: &Cell, len: &Value) -> ER<Value> {
    raf_read_cell(it, raf, buff, len.clone())
}

fn val(c: &Cell) -> Value {
    c.borrow().clone()
}

/// Extract information from the extended data block
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_ext_data(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_len = dir_info.get("DirLen");
    let mut pos = n(0);
    // loop through sub-blocks
    while pl::lt(&pl::add(&pos, &n(10)), &dir_len) {
        pl::at(172);
        if !substr(it, &pl::sderef(&data_pt)?, &pos, Some(&n(4)))?.eq_str("~FL\0") {
            pl::method(it, &et, "Warn", vec![pl::s("Lost synchronization while reading sub blocks")])?;
            break;
        }
        let tag = call(it, "Get16u", vec![data_pt.clone(), pl::add(&pos, &n(4))])?;
        let len = call(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(6))])?;
        pos = pl::add(&pos, &pl::add(&n(10), &len));
        if pl::gt(&pos, &dir_len) {
            let msg = pl::concat(&pl::concat(&pl::concat(&pl::s("Truncated sub block ID="), &tag), &pl::s(" len=")), &len);
            pl::method(it, &et, "Warn", vec![msg])?;
            break;
        }
        if !tag_table_ptr.get(tag.to_bytes()).truthy() {
            continue;
        }
        let tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
        if !tag_info.truthy() {
            continue;
        }
        let mut start = pl::sub(&pos, &len);
        if !tag_info.get("Name").eq_str("EXIFInfo") {
            pl::at(187);
            let data_len = pl::length(&pl::sderef(&data_pt)?);
            pl::method(
                it,
                &et,
                "HandleTag",
                vec![
                    tag_table_ptr.clone(),
                    tag.clone(),
                    Value::Undef,
                    pl::s("TagInfo"),
                    tag_info.clone(),
                    pl::s("DataPt"),
                    data_pt.clone(),
                    pl::s("DataPos"),
                    dir_info.get("DataPos"),
                    pl::s("DataLen"),
                    data_len,
                    pl::s("Start"),
                    start.clone(),
                    pl::s("Size"),
                    len.clone(),
                ],
            )?;
            continue;
        }
        // validate EXIF block header and set byte order
        pl::at(198);
        if !(pl::gt(&len, &n(14)) && substr(it, &pl::sderef(&data_pt)?, &pl::sub(&pos, &len), Some(&n(6)))?.eq_str("Exif\0\0")) {
            continue;
        }
        // (a sub argument: substr($$dataPt...) is an lvalue there, so an
        // undefined $dataPt is autovivified, not an error)
        let order = substr(it, &data_pt.deref(), &pl::add(&start, &n(6)), Some(&n(2)))?;
        if !call(it, "SetByteOrder", vec![order])?.truthy() {
            continue;
        }
        // This is REALLY annoying...  They use a standard TIFF offset to point to
        // the first IFD, but after that the offsets are relative to the start of
        // the IFD instead of the TIFF base, which means that I must handle it as a
        // special case.  Dumb, dumb...
        start = pl::add(&start, &n(14));
        let sub_dir_info = Value::new_hash();
        sub_dir_info.set("DirName", pl::s("EXIF"));
        sub_dir_info.set("Parent", pl::s("PSP"));
        sub_dir_info.set("DataPt", data_pt.clone());
        sub_dir_info.set("DataPos", pl::neg(&start)); // data position relative to Base
        pl::at(205);
        sub_dir_info.set("DataLen", pl::length(&pl::sderef(&data_pt)?));
        sub_dir_info.set("DirStart", start.clone());
        sub_dir_info.set("Base", pl::add(&start, &dir_info.get("DataPos"))); // absolute base offset
        sub_dir_info.set("Multi", n(0));
        let exif_table = call(it, "GetTagTable", vec![tag_info.path(&["SubDirectory", "TagTable"])])?;
        pl::call(it, "Image::ExifTool::Exif::ProcessExif", vec![et.clone(), sub_dir_info, exif_table])?;
        call(it, "SetByteOrder", vec![pl::s("II")])?;
    }
    ret(n(1))
}

/// Extract information from a PSP file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid PSP file
pub fn process_psp(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);
    let (mut tag, mut len, mut err) = (Value::Undef, Value::Undef, false);
    pl::at(231);
    need_obj(&raf, "Read")?;
    if !(pl::num_eq(&read(it, &raf, &buff, &n(32))?, &n(32))
        && val(&buff).eq_str("Paint Shop Pro Image File\x0a\x1a\0\0\0\0\0")
        && pl::num_eq(&read(it, &raf, &buff, &n(4))?, &n(4)))
    {
        return ret(n(0));
    }
    pl::method(it, &et, "SetFileType", vec![])?;
    call(it, "SetByteOrder", vec![pl::s("II")])?;
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::PSP::Main")])?;
    let av = pl::unpack("v*", &val(&buff))?;
    // figure out block header length for this format PSP file
    let hlen = if pl::gt(&arg(&av, 0), &n(3)) { n(10) } else { n(14) };
    et.set("PSPFileVersion", arg(&av, 0)); // save for use in Condition
    pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("FileVersion"), pl::join(" ", &av)])?;
    // loop through blocks in file
    let mut pos = n(36);
    loop {
        if !pl::num_eq(&read(it, &raf, &buff, &hlen)?, &hlen) {
            break;
        }
        if !re(r"^~BK\0", "").is_match(&val(&buff)) {
            pl::method(it, &et, "Warn", vec![pl::s("Lost synchronization while reading main PSP blocks")])?;
            break;
        }
        let bp = Value::ScalarRef(buff.clone());
        tag = call(it, "Get16u", vec![bp.clone(), n(4)])?;
        len = call(it, "Get32u", vec![bp.clone(), pl::sub(&hlen, &n(4))])?;
        pos = pl::add(&pos, &pl::add(&hlen, &len));
        if !tag_table_ptr.get(tag.to_bytes()).truthy() {
            if !pl::method(it, &raf, "Seek", vec![len.clone(), n(1)])?.truthy() {
                err = true;
                break;
            }
            continue;
        }
        if !pl::num_eq(&read(it, &raf, &buff, &len)?, &len) {
            err = true;
            break;
        }
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![tag_table_ptr.clone(), tag.clone(), val(&buff), pl::s("DataPt"), bp, pl::s("DataPos"), pl::sub(&pos, &len), pl::s("Size"), len.clone()],
        )?;
    }
    if err {
        let msg = pl::concat(&pl::concat(&pl::concat(&pl::s("Truncated main block ID="), &tag), &pl::s(" len=")), &len);
        pl::method(it, &et, "Warn", vec![msg])?;
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::PSP::ProcessExtData", process_ext_data);
    it.define_native("Image::ExifTool::PSP::ProcessPSP", process_psp);
}
