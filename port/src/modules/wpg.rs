//! Image::ExifTool::WPG (Image/ExifTool/WPG.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read WordPerfect Graphics meta information

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell};
use crate::perl::Value;
use crate::pl::{self, arg, n, re, ret};

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

/// `ord($s)`
fn ord(s: &Value) -> Value {
    n(s.to_bytes().first().copied().unwrap_or(0) as i64)
}

/// Print record type
/// Inputs: 0) record type and count, 1) inverse flag, 2) PrintConv hash ref
/// Returns: converted record name
pub fn print_record(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val, _inv, print_conv) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let parts = pl::split(&re("x", ""), &val, 0);
    let (type_, count) = (arg(&parts, 0), arg(&parts, 1));
    let mut prt = print_conv.get(type_.to_bytes());
    if !prt.truthy() {
        prt = pl::sprintf("Unknown (0x%.2x)", &[type_]);
    }
    if count.truthy() {
        prt = pl::concat(&prt, &pl::concat(&pl::s(" x "), &count));
    }
    ret(prt)
}

/// Read variable-length integer
/// Inputs: 0) RAF ref
/// Returns: integer value
pub fn read_var_int(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let raf = arg(&a, 0);
    let buff = cell(Value::Undef);
    if !read(it, &raf, &buff, &n(1))?.truthy() {
        return ret(n(0));
    }
    let mut val = ord(&val(&buff));
    if pl::num_eq(&val, &n(0xff)) {
        if !pl::num_eq(&read(it, &raf, &buff, &n(2))?, &n(2)) {
            return ret(n(0));
        }
        val = pl::unpack1("v", &self::val(&buff))?;
        if val.to_i64() & 0x8000 != 0 {
            if !pl::num_eq(&read(it, &raf, &buff, &n(2))?, &n(2)) {
                return ret(n(0));
            }
            val = n(((val.to_i64() & 0x7fff) << 16) | pl::unpack1("v", &self::val(&buff))?.to_i64());
        }
    }
    ret(val)
}

/// `ReadVarInt($raf)` in scalar context.
fn read_var_int1(it: &mut Interp, raf: &Value) -> ER<Value> {
    Ok(read_var_int(it, vec![raf.clone()], Want::Scalar)?.pop().unwrap_or_default())
}

/// Read WPG version 1 or 2 image
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid WPG file
pub fn process_wpg(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);
    let (mut last_type, mut count) = (Value::Undef, Value::Undef);

    // verify this is a valid WPG file
    if !pl::num_eq(&read(it, &raf, &buff, &n(16))?, &n(16)) {
        return ret(n(0));
    }
    if !re(r"^\xff\x57\x50\x43", "").is_match(&val(&buff)) {
        return ret(n(0));
    }
    pl::method(it, &et, "SetFileType", vec![])?;
    call(it, "SetByteOrder", vec![pl::s("II")])?;
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::WPG::Main")])?;
    let offset = call(it, "Get32u", vec![Value::ScalarRef(buff.clone()), n(4)])?;
    let vr = pl::unpack("x10CC", &val(&buff))?;
    let (ver, rev) = (arg(&vr, 0), arg(&vr, 1));
    let vs = pl::concat(&pl::concat(&ver, &pl::s(".")), &rev);
    pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("WPGVersion"), vs])?;
    if pl::lt(&ver, &n(1)) || pl::gt(&ver, &n(2)) {
        // currently support only version 1 and 2 formats
        pl::method(it, &et, "Warn", vec![pl::s("Unsupported WPG version")])?;
        return ret(n(1));
    }
    let tag = if pl::num_eq(&ver, &n(1)) { pl::s("Records") } else { pl::s("RecordsV2") };
    if pl::gt(&offset, &n(16)) && !pl::method(it, &raf, "Seek", vec![pl::sub(&offset, &n(16)), n(1)])?.truthy() {
        return ret(n(1));
    }
    // loop through records
    loop {
        let (mut type_, len, mut get_size);
        type_ = Value::Undef;
        get_size = false;
        if pl::num_eq(&read(it, &raf, &buff, &ver)?, &ver) {
            // read 1 or 2 bytes, based on version
            if pl::num_eq(&ver, &n(1)) {
                // read version 1 record header
                type_ = ord(&val(&buff));
                len = read_var_int1(it, &raf)?;
                if pl::num_eq(&type_, &n(0x0f)) {
                    get_size = true; // Start WPG (Type 1)
                }
            } else {
                // read version 2 record header
                type_ = pl::unpack1("xC", &val(&buff))?;
                read_var_int1(it, &raf)?; // skip extensions
                len = read_var_int1(it, &raf)?;
                if pl::num_eq(&type_, &n(0x01)) {
                    get_size = true; // Start WPG
                }
                if pl::gt(&type_, &n(0x3f)) {
                    type_ = Value::Undef;
                }
            }
            if get_size {
                // read Start record to obtain image size
                if !pl::num_eq(&read(it, &raf, &buff, &len)?, &len) {
                    pl::method(it, &et, "Warn", vec![pl::s("File format error")])?;
                    break;
                }
                let (mut w, mut h, mut xres, mut yres, mut err) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef);
                if pl::num_eq(&ver, &n(1)) {
                    if pl::lt(&len, &n(6)) {
                        err = pl::s("");
                    } else {
                        let wh = pl::unpack("x2vv", &val(&buff))?;
                        w = arg(&wh, 0);
                        h = arg(&wh, 1);
                    }
                } else {
                    let mut format = Value::Undef;
                    let xyp = pl::unpack("vvC", &val(&buff))?;
                    xres = arg(&xyp, 0);
                    yres = arg(&xyp, 1);
                    let precision = arg(&xyp, 2);
                    if pl::num_eq(&precision, &n(0)) && pl::ge(&len, &n(21)) {
                        if !pl::ge(&len, &n(13 + 2 * 4)) {
                            err = pl::s("");
                        }
                        format = pl::s("int16s");
                    } else if pl::num_eq(&precision, &n(1)) && pl::ge(&len, &n(29)) {
                        if !pl::ge(&len, &n(13 + 4 * 4)) {
                            err = pl::s("");
                        }
                        format = pl::s("int32s");
                    } else {
                        err = pl::s("Invalid integer precision");
                    }
                    if err.is_undef() {
                        let c = pl::call_list(it, "Image::ExifTool::ReadValue", vec![Value::ScalarRef(buff.clone()), n(13), format, n(4), pl::sub(&len, &n(13))])?;
                        let (x1, y1, x2, y2) = (arg(&c, 0), arg(&c, 1), arg(&c, 2), arg(&c, 3));
                        w = pl::abs(&pl::sub(&x2, &x1));
                        h = pl::abs(&pl::sub(&y2, &y1));
                    }
                }
                if !err.is_undef() {
                    let msg = if err.truthy() { err } else { pl::s("Truncated start record") };
                    pl::method(it, &et, "Warn", vec![msg])?;
                } else {
                    let xr = if xres.truthy() { xres } else { n(1200) };
                    let wi = pl::div(&w, &xr)?;
                    pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("ImageWidthInches"), wi])?;
                    let yr = if yres.truthy() { yres } else { n(1200) };
                    let hi = pl::div(&h, &yr)?;
                    pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("ImageHeightInches"), hi])?;
                }
            } else {
                // skip to the next record
                if !pl::method(it, &raf, "Seek", vec![len.clone(), n(1)])?.truthy() {
                    pl::method(it, &et, "Warn", vec![pl::s("Truncated record")])?;
                    break;
                }
            }
        }
        // go to some trouble to collapse identical sequential entries in record list
        // (trying to keep the length of the list managable for complex images)
        if last_type.truthy() && type_.truthy() && pl::num_eq(&type_, &last_type) {
            count = pl::add(&count, &n(1));
            continue;
        }
        if last_type.truthy() {
            let v = if pl::gt(&count, &n(1)) { pl::concat(&pl::concat(&last_type, &pl::s("x")), &count) } else { last_type.clone() };
            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag.clone(), v])?;
        }
        if !type_.truthy() {
            break;
        }
        last_type = type_;
        count = n(1);
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::WPG::PrintRecord", print_record);
    it.define_native("Image::ExifTool::WPG::ReadVarInt", read_var_int);
    it.define_native("Image::ExifTool::WPG::ProcessWPG", process_wpg);
}
