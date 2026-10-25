//! Image::ExifTool::AIFF (Image/ExifTool/AIFF.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read AIFF meta information

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

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

fn warn(it: &mut Interp, et: &Value, msg: Value) -> ER<()> {
    pl::method(it, et, "Warn", vec![msg])?;
    Ok(())
}

/// Process AIFF Comment chunk
/// Inputs: 0) ExifTool object reference, 1) DirInfo reference, 2) tag table ref
/// Returns: 1 on success
pub fn process_comment(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_len = dir_info.get("DirLen");
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    if !pl::gt(&dir_len, &n(2)) {
        return ret(n(0));
    }
    let num_comments = pl::unpack1("n", &data_pt.deref())?;
    let mut pos = n(2);
    if verbose.truthy() {
        pl::method(it, &et, "VerboseDir", vec![pl::s("Comment"), num_comments.clone()])?;
    }
    let mut i = n(0);
    // (the sub has no return: it gives the value of its last statement, the
    // false loop condition, or an empty list after "last")
    while pl::lt(&i, &num_comments) {
        if pl::gt(&pl::add(&pos, &n(8)), &dir_len) {
            return pl::ret_nothing(want);
        }
        let t = format!("x{}Nnn", pos.to_text());
        let u = pl::unpack(&t, &data_pt.deref())?;
        let (time, marker_id, mut size) = (arg(&u, 0), arg(&u, 1), arg(&u, 2));
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), n(0), time])?;
        if marker_id.truthy() {
            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), n(1), marker_id])?;
        }
        pos = pl::add(&pos, &n(8));
        if pl::gt(&pl::add(&pos, &size), &dir_len) {
            return pl::ret_nothing(want);
        }
        let v = substr(it, &data_pt.deref(), &pos, Some(&size))?;
        pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), n(2), v])?;
        if size.to_i64() & 0x01 != 0 {
            size = pl::add(&size, &n(1)); // account for padding byte if necessary
        }
        pos = pl::add(&pos, &size);
        i = pl::add(&i, &n(1));
    }
    ret(pl::s(""))
}

/// Extract information from a AIFF file
/// Inputs: 0) ExifTool object reference, 1) DirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid AIFF file
pub fn process_aiff(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);
    let (mut err, tag_table_ptr, mut page, type_);
    err = false;
    page = Value::Undef;

    // verify this is a valid AIFF file
    if !pl::num_eq(&read(it, &raf, &buff, &n(12))?, &n(12)) {
        return ret(n(0));
    }
    let fs = et.get("OPTIONS").get("FastScan");
    let fast3 = fs.truthy() && pl::num_eq(&fs, &n(3));
    let mut pos = n(12);
    // check for DjVu image
    if re(r"^AT&TFORM", "").is_match(&val(&buff)) {
        // http://www.djvu.org/
        // http://djvu.sourceforge.net/specs/djvu3changes.txt
        let buf2 = cell(Value::Undef);
        if !(pl::num_eq(&read(it, &raf, &buf2, &n(4))?, &n(4)) && re(r"^(DJVU|DJVM)", "").is_match(&val(&buf2))) {
            return ret(n(0));
        }
        pos = pl::add(&pos, &n(4));
        let b = pl::concat(&substr(it, &val(&buff), &n(4), None)?, &val(&buf2));
        *buff.borrow_mut() = b;
        pl::method(it, &et, "SetFileType", vec![pl::s("DJVU")])?;
        if fast3 {
            return ret(n(1));
        }
        tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::DjVu::Main")])?;
        // modify FileType to indicate a multi-page document
        let values = et.get("VALUE");
        if val(&buf2).eq_str("DJVM") && values.get("FileType").truthy() {
            values.set("FileType", pl::concat(&values.get("FileType"), &pl::s(" (multi-page)")));
        }
        type_ = pl::s("DjVu");
    } else {
        let Some(m) = re(r"^FORM....(AIF(F|C))", "s").m(&val(&buff)) else {
            return ret(n(0));
        };
        pl::method(it, &et, "SetFileType", vec![cap(&m, 1)])?;
        if fast3 {
            return ret(n(1));
        }
        tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::AIFF::Main")])?;
        type_ = pl::s("AIFF");
    }
    call(it, "SetByteOrder", vec![pl::s("MM")])?;
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    //
    // Read through the IFF chunks
    //
    let mut nn = n(0);
    loop {
        'body: {
            if !pl::num_eq(&read(it, &raf, &buff, &n(8))?, &n(8)) {
                return finish(it, &et, err, &type_);
            }
            pos = pl::add(&pos, &n(8));
            let u = pl::unpack("a4N", &val(&buff))?;
            let (tag, mut len) = (arg(&u, 0), arg(&u, 1));
            let mut tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
            let tell = pl::method(it, &raf, "Tell", vec![])?;
            let msg = pl::concat(&pl::concat(&pl::concat(&pl::concat(&pl::s("AIFF '"), &tag), &pl::s("' chunk (")), &len), &pl::s(" bytes of data): "));
            pl::method(it, &et, "VPrint", vec![n(0), msg, tell, pl::s("\n")])?;
            // AIFF chunks are padded to an even number of bytes
            let mut len2 = pl::add(&len, &n(len.to_i64() & 0x01));
            if pl::gt(&len2, &n(100000000)) {
                if pl::ge(&len2, &n(0x80000000)) {
                    if !pl::method(it, &et, "Options", vec![pl::s("LargeFileSupport")])?.truthy() {
                        warn(it, &et, pl::s("End of processing at large chunk (LargeFileSupport not enabled)"))?;
                        return finish(it, &et, err, &type_);
                    } else if pl::method(it, &et, "Options", vec![pl::s("LargeFileSupport")])?.eq_str("2") {
                        warn(it, &et, pl::s("Skipping large chunk (LargeFileSupport is 2)"))?;
                    }
                }
                if tag_info.truthy() {
                    let m = pl::concat(&pl::concat(&pl::s("Skipping large "), &tag_info.get("Name")), &pl::s(" chunk (> 100 MB)"));
                    warn(it, &et, m)?;
                    tag_info = Value::Undef;
                }
            }
            if tag_info.truthy() {
                if tag_info.get("TypeOnly").truthy() {
                    len = n(4);
                    len2 = n(4);
                    page = pl::add(&if page.truthy() { page.clone() } else { n(0) }, &n(1));
                    let m = pl::concat(&et.get("INDENT"), &pl::concat(&pl::concat(&pl::s("Page "), &page), &pl::s(":\n")));
                    pl::method(it, &et, "VPrint", vec![n(0), m])?;
                }
                if !pl::ge(&read(it, &raf, &buff, &len2)?, &len) {
                    err = true;
                    return finish(it, &et, err, &type_);
                }
                if !(tag_info.get("SubDirectory").truthy() || tag_info.get("Binary").truthy()) {
                    let mut b = val(&buff);
                    if re(r"\0+$", "").subst(&mut b, false, |_| Vec::new()) > 0 {
                        *buff.borrow_mut() = b; // remove trailing nulls
                    }
                }
                pl::method(
                    it,
                    &et,
                    "HandleTag",
                    vec![tag_table_ptr.clone(), tag.clone(), val(&buff), pl::s("DataPt"), Value::ScalarRef(buff.clone()), pl::s("DataPos"), pos.clone(), pl::s("Start"), n(0), pl::s("Size"), len.clone()],
                )?;
            } else if !len.truthy() {
                nn = pl::add(&nn, &n(1));
                if pl::lt(&nn, &n(100)) {
                    break 'body; // next
                }
                warn(it, &et, pl::s("Aborting scan.  Too many empty chunks"))?;
                return finish(it, &et, err, &type_);
            } else if pl::gt(&verbose, &n(2)) && pl::lt(&len2, &n(1024000)) {
                if !pl::num_eq(&read(it, &raf, &buff, &len2)?, &len2) {
                    err = true;
                    return finish(it, &et, err, &type_);
                }
                pl::method(it, &et, "VerboseDump", vec![Value::ScalarRef(buff.clone())])?;
            } else if !pl::method(it, &raf, "Seek", vec![len2.clone(), n(1)])?.truthy() {
                err = true;
                return finish(it, &et, err, &type_);
            }
            pos = pl::add(&pos, &len2);
            nn = n(0);
        }
        nn = pl::add(&nn, &n(1));
    }
}

/// The end of ProcessAIFF, after its chunk loop.
fn finish(it: &mut Interp, et: &Value, err: bool, type_: &Value) -> ER<Vec<Value>> {
    if err {
        let m = pl::concat(&pl::concat(&pl::s("Error reading "), type_), &pl::s(" file (corrupted?)"));
        warn(it, et, m)?;
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::AIFF::ProcessComment", process_comment);
    it.define_native("Image::ExifTool::AIFF::ProcessAIFF", process_aiff);
}
