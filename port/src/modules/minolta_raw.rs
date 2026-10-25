//! Image::ExifTool::MinoltaRaw (Image/ExifTool/MinoltaRaw.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::exiftool::part2;
use crate::modules::random_access::{cell, raf_read, raf_read_cell};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, n, re, ret};

fn val(c: &Cell) -> Value {
    c.borrow().clone()
}

/// `\$var` of a `my` variable kept in a cell.
fn rf(c: &Cell) -> Value {
    Value::ScalarRef(c.clone())
}

/// Perl's boolean results: 1 or "".
fn pbool(b: bool) -> Value {
    if b {
        n(1)
    } else {
        pl::s("")
    }
}

fn g32u(it: &mut Interp, data_pt: &Value, pos: Value) -> ER<Value> {
    Ok(part2::get32u(it, vec![data_pt.clone(), pos], Want::Scalar)?.pop().unwrap_or_default())
}

fn s32u(it: &mut Interp, v: Value) -> ER<Value> {
    Ok(part2::set32u(it, vec![v], Want::Scalar)?.pop().unwrap_or_default())
}

//------------------------------------------------------------------------------
/// PrintConv for WBMode
pub fn convert_wb_mode(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    let mrw_wb = |k: i64| -> Option<&'static str> {
        Some(match k {
            0 => "Auto",
            1 => "Daylight",
            2 => "Cloudy",
            3 => "Tungsten",
            4 => "Flash/Fluorescent",
            5 => "Fluorescent",
            6 => "Shade",
            7 => "User 1",
            8 => "User 2",
            9 => "User 3",
            10 => "Temperature",
            _ => return None,
        })
    };
    let lo = ops::bit_and(&val, &n(0x0f));
    let mut wbstr = match mrw_wb(lo.to_i64()) {
        Some(s) => pl::s(s),
        None => pl::concat(&pl::concat(&pl::s("Unknown ("), &lo), &pl::s(")")),
    };
    let hi = ops::shift_right(&val, &n(4));
    if pl::ge(&hi, &n(6)) && pl::le(&hi, &n(12)) {
        wbstr = pl::concat(&wbstr, &pl::concat(&pl::concat(&pl::s(" ("), &pl::sub(&hi, &n(8))), &pl::s(")")));
    }
    ret(wbstr)
}

//------------------------------------------------------------------------------
/// Write MRW directory (eg. in ARW images)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) optional tag table ref
/// Returns: new MRW data or undef on error
pub fn write_mrw(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et.truthy() {
        return ret(n(1)); // allow dummy access
    }
    let buff = cell(pl::s(""));
    dir_info.set("OutFile", rf(&buff));
    let r = process_mrw(it, vec![et, dir_info, tag_table_ptr], Want::Scalar)?.pop().unwrap_or_default();
    if !pl::gt(&r, &n(0)) {
        *buff.borrow_mut() = Value::Undef;
    }
    ret(val(&buff))
}

//------------------------------------------------------------------------------
/// Read or write Minolta MRW file
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) optional tag table ref
/// Returns: 1 on success, 0 if this wasn't a valid MRW file, or -1 on write error
/// Notes: File pointer must be set to start of MRW in RAF upon entry
pub fn process_mrw(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let mut raf = dir_info.get("RAF");
    let outfile = dir_info.get("OutFile");
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let out = pl::method(it, &et, "Options", vec![pl::s("TextOut")])?;
    let (mut data, mut err, mut out_buff) = (Value::Undef, Value::Undef, Value::Undef);

    if dir_info.get("DataPt").truthy() {
        // make a RAF object for MRW information extracted from other file types
        raf = pl::method(it, &pl::s("File::RandomAccess"), "new", vec![dir_info.get("DataPt")])?;
        // MRW information in DNG images may not start at beginning of data block
        if dir_info.get("DirStart").truthy() {
            pl::method(it, &raf, "Seek", vec![dir_info.get("DirStart"), n(0)])?;
        }
    }
    if !pl::num_eq(&raf_read(it, &raf, &mut data, n(8))?, &n(8)) {
        return ret(n(0));
    }
    // "\0MRM" for big-endian (MRW images), and
    // "\0MRI" for little-endian (MRWInfo in ARW images)
    let Some(m) = re(r"^\0MR([MI])", "").m(&data) else { return ret(n(0)) };
    let c1 = cap(&m, 1);
    let hdr = pl::concat(&pl::s("\0MR"), &c1);
    pl::call(it, "Image::ExifTool::SetByteOrder", vec![pl::concat(&c1, &c1)])?;
    pl::method(it, &et, "SetFileType", vec![])?;
    let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::MinoltaRaw::Main")])?;
    if outfile.truthy() {
        pl::method(it, &et, "InitWriteDirs", vec![pl::s("TIFF")])?; // use same write dirs as TIFF
        out_buff = pl::s("");
    }
    let mut pos = pl::method(it, &raf, "Tell", vec![])?;
    let offset = pl::add(&g32u(it, &pl::sref(data.clone()), n(4))?, &pos);
    let mut rtn_val = n(1);
    if verbose.truthy() {
        pl::print_to(it, Some(&out), &[pl::sprintf("  [MRW Data Offset: 0x%x]\n", &[offset.clone()])])?;
    }
    // loop through MRW segments (ref 1)
    while pl::lt(&pos, &offset) {
        if !pl::num_eq(&raf_read(it, &raf, &mut data, n(8))?, &n(8)) {
            err = n(1);
            break;
        }
        pos = pl::add(&pos, &n(8));
        let tag = pl::substr(&data, 0, Some(4));
        let len = g32u(it, &pl::sref(data.clone()), n(4))?;
        if verbose.truthy() {
            let p = pl::method(it, &et, "Printable", vec![tag.clone()])?;
            pl::print_to(it, Some(&out), &[pl::s("MRW "), p, pl::concat(&pl::concat(&pl::s(" segment ("), &len), &pl::s(" bytes):\n"))])?;
            if pl::gt(&verbose, &n(2)) {
                if !(pl::num_eq(&raf_read(it, &raf, &mut data, len.clone())?, &len) && pl::method(it, &raf, "Seek", vec![pos.clone(), n(0)])?.truthy()) {
                    err = n(1);
                    break;
                }
                pl::method(it, &et, "VerboseDump", vec![pl::sref(data.clone())])?;
            }
        }
        let tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
        if tag_info.truthy() && tag_info.get("SubDirectory").truthy() {
            let sub_table = pl::call(it, "Image::ExifTool::GetTagTable", vec![tag_info.get("SubDirectory").get("TagTable")])?;
            let buff = cell(Value::Undef);
            // save shift for values stored with wrong base offset
            let tell = pl::method(it, &raf, "Tell", vec![])?;
            et.set("MRW_WrongBase", pl::neg(&tell));
            if !pl::num_eq(&raf_read_cell(it, &raf, &buff, len.clone())?, &len) {
                err = n(1);
                break;
            }
            let subdir_info = Value::new_hash();
            subdir_info.set("DataPt", rf(&buff));
            subdir_info.set("DataLen", len.clone());
            subdir_info.set("DataPos", pos.clone());
            subdir_info.set("DirStart", n(0));
            subdir_info.set("DirLen", len.clone());
            subdir_info.set("DirName", tag_info.get("Name"));
            subdir_info.set("Parent", pl::s("MRW"));
            subdir_info.set("NoTiffEnd", n(1)); // no end-of-TIFF check
            if outfile.truthy() {
                let write_proc = tag_info.get("SubDirectory").get("WriteProc");
                let mut v = pl::method(it, &et, "WriteDirectory", vec![subdir_info, sub_table, write_proc])?;
                if !v.is_undef() && pl::length(&v).truthy() {
                    // pad to an even 4 bytes (can't hurt, and it seems to be the standard)
                    let l = pl::length(&v);
                    if ops::bit_and(&l, &n(0x03)).truthy() {
                        let cnt = pl::sub(&n(4), &ops::bit_and(&l, &n(0x03))).to_i64().max(0) as usize;
                        v = pl::concat(&v, &Value::bytes(vec![0u8; cnt]));
                    }
                    let l = s32u(it, pl::length(&v))?;
                    out_buff = pl::concat(&out_buff, &pl::concat(&pl::concat(&tag, &l), &v));
                } else if v.is_undef() {
                    out_buff = pl::concat(&out_buff, &pl::concat(&data, &val(&buff))); // copy over original information
                }
            } else {
                let process_proc = tag_info.get("SubDirectory").get("ProcessProc");
                pl::method(it, &et, "ProcessDirectory", vec![subdir_info, sub_table, process_proc])?;
            }
        } else if outfile.truthy() {
            // add this segment to the output buffer
            let mut buff = Value::Undef;
            if !pl::num_eq(&raf_read(it, &raf, &mut buff, len.clone())?, &len) {
                err = n(1);
                break;
            }
            out_buff = pl::concat(&out_buff, &pl::concat(&data, &buff));
        } else {
            // skip this segment
            if !pl::method(it, &raf, "Seek", vec![pl::add(&pos, &len), n(0)])?.truthy() {
                err = n(1);
                break;
            }
        }
        pos = pl::add(&pos, &len);
    }
    if !pl::num_eq(&pos, &offset) {
        err = n(1); // meta information length check
    }

    if outfile.truthy() {
        // write the file header then the buffered meta information
        let l = s32u(it, pl::length(&out_buff))?;
        if !pl::call(it, "Image::ExifTool::Write", vec![outfile.clone(), hdr, l, out_buff.clone()])?.truthy() {
            rtn_val = n(-1);
        }
        // copy over image data
        while raf_read(it, &raf, &mut out_buff, n(65536))?.truthy() {
            if !pl::call(it, "Image::ExifTool::Write", vec![outfile.clone(), out_buff.clone()])?.truthy() {
                rtn_val = n(-1);
            }
        }
        // Sony IDC utility corrupts MRWInfo when writing ARW images,
        // so make this a minor error for these images
        if err.truthy() {
            let minor = pbool(pl::str_eq(&et.get("TIFF_TYPE"), "ARW"));
            pl::method(it, &et, "Error", vec![pl::s("MRW format error"), minor])?;
        }
    } else {
        if err.truthy() {
            pl::method(it, &et, "Warn", vec![pl::s("MRW format error")])?;
        }
        if !et.get("A100DataOffset").truthy() {
            pl::method(it, &et, "ImageDataHash", vec![raf, Value::Undef, pl::s("raw")])?;
        }
    }
    ret(rtn_val)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::MinoltaRaw::ConvertWBMode", convert_wb_mode);
    it.define_native("Image::ExifTool::MinoltaRaw::WriteMRW", write_mrw);
    it.define_native("Image::ExifTool::MinoltaRaw::ProcessMRW", process_mrw);
}
