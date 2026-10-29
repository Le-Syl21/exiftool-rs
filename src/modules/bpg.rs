//! Image::ExifTool::BPG (Image/ExifTool/BPG.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::exiftool::part2::get8u;
use crate::modules::random_access::{cell, raf_read_cell};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, n, re, ret};

fn val(c: &Cell) -> Value {
    c.borrow().clone()
}

/// `\$var` of a `my` variable kept in a cell.
fn rf(c: &Cell) -> Value {
    Value::ScalarRef(c.clone())
}

/// `$raf->Read($buff, $len) == $len`
fn read_eq(it: &mut Interp, raf: &Value, buff: &Cell, len: &Value) -> ER<bool> {
    let r = raf_read_cell(it, raf, buff, len.clone())?;
    Ok(pl::num_eq(&r, len))
}

fn seek(it: &mut Interp, raf: &Value, pos: Value, whence: i64) -> ER<bool> {
    Ok(pl::method(it, raf, "Seek", vec![pos, n(whence)])?.truthy())
}

fn g8u(it: &mut Interp, data_pt: &Value, pos: Value) -> ER<Value> {
    Ok(get8u(it, vec![data_pt.clone(), pos], Want::Scalar)?.pop().unwrap_or_default())
}

/// Get ue7 integer from binary data (max 32 bits)
/// Inputs: 0) data ref, 1) location in data (undef for 0)
/// Returns: 0) ue7 as integer or undef on error, 1) length of ue7 in bytes
pub fn get_ue7(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let data_pt = arg(&a, 0);
    let pos = {
        let p = arg(&a, 1);
        if p.truthy() {
            p
        } else {
            n(0)
        }
    };
    pl::at(126);
    let size = pl::length(&pl::sderef(&data_pt)?);
    let mut val = n(0);
    let mut i = n(0);
    loop {
        if pl::ge(&pl::add(&pos, &i), &size) || pl::ge(&i, &n(5)) {
            return pl::ret_nothing(want);
        }
        let byte = g8u(it, &data_pt, pl::add(&pos, &i))?;
        val = ops::bit_or(&ops::shift_left(&val, &n(7)), &ops::bit_and(&byte, &n(0x7f)));
        if !ops::bit_and(&byte, &n(0x80)).truthy() {
            if pl::num_eq(&i, &n(4)) && ops::bit_and(&byte, &n(0x70)).truthy() {
                return pl::ret_nothing(want); // error if bits 32-34 are set
            }
            break; // this was the last byte
        }
        if pl::num_eq(&i, &n(0)) && pl::num_eq(&byte, &n(0x80)) {
            return pl::ret_nothing(want); // error if first byte is 0x80
        }
        i = pl::add(&i, &n(1)); // step to the next byte
    }
    Ok(vec![val, pl::add(&i, &n(1))])
}

/// Extract information from a BPG image
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid BPG file
pub fn process_bpg(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| {
        let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
        let raf = dir_info.get("RAF");
        let buff = cell(Value::Undef);
        let (size, mut nn, mut len, mut pos);

        // verify this is a valid BPG file
        pl::at(155);
        if !read_eq(it, &raf, &buff, &n(21))? {
            return ret(n(0)); // (21 bytes is maximum header length)
        }
        if !re(r"^BPG\xfb", "").is_match(&val(&buff)) {
            return ret(n(0));
        }
        pl::method(it, &et, "SetFileType", vec![])?; // set the FileType tag

        pl::call(it, "Image::ExifTool::SetByteOrder", vec![pl::s("MM")])?;
        let dir_info2 = Value::new_hash();
        dir_info2.set("DataPt", rf(&buff));
        dir_info2.set("DirStart", n(0));
        dir_info2.set("DirLen", pl::length(&val(&buff)));
        dir_info2.set("VarFormatData", Value::new_array(vec![]));
        let table = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::BPG::Main")])?;
        pl::method(it, &et, "ProcessDirectory", vec![dir_info2.clone(), table])?;

        if !ops::bit_and(&et.get_or_new_hash("VALUE").get("Flags"), &n(0x0008)).truthy() {
            return ret(n(1)); // all done unless extension flag is set
        }

        // add varSize from last entry in VarFormatData to determine
        // the current read position in the file
        // (a negative subscript of an empty array cannot be autovivified)
        let var_format_data = dir_info2.get("VarFormatData");
        if var_format_data.is_empty() {
            pl::at(172);
            return crate::interp::eval::die("Modification of non-creatable array value attempted, subscript -1");
        }
        let mut data_pos = pl::add(&n(9), &var_format_data.at(-1).at(1));
        // read extension length
        if !(seek(it, &raf, data_pos.clone(), 0)? && read_eq(it, &raf, &buff, &n(5))?) {
            pl::method(it, &et, "Warn", vec![pl::s("Missing BPG extension data")])?;
            return ret(n(1));
        }
        let r = get_ue7(it, vec![rf(&buff)], Want::List)?;
        (size, nn) = (arg(&r, 0), arg(&r, 1));
        if size.is_undef() {
            pl::method(it, &et, "Warn", vec![pl::s("Corrupted BPG extension length")])?;
            return ret(n(1));
        }
        data_pos = pl::add(&data_pos, &nn);
        if pl::gt(&size, &n(10000000)) {
            pl::method(it, &et, "Warn", vec![pl::s("BPG extension is too large")])?;
            return ret(n(1));
        }
        if !(seek(it, &raf, data_pos.clone(), 0)? && read_eq(it, &raf, &buff, &size)?) {
            pl::method(it, &et, "Warn", vec![pl::s("Truncated BPG extension")])?;
            return ret(n(1));
        }
        let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::BPG::Extensions")])?;
        // loop through the individual extensions
        pos = n(0);
        'ext: while pl::lt(&pos, &size) {
            'body: {
                let ty = g8u(it, &rf(&buff), pos.clone())?;
                // get length of this extension
                pos = pl::add(&pos, &n(1));
                let r = get_ue7(it, vec![rf(&buff), pos.clone()], Want::List)?;
                (len, nn) = (arg(&r, 0), arg(&r, 1));
                if len.is_undef() {
                    pl::method(it, &et, "Warn", vec![pl::s("Corrupted BPG extension")])?;
                    break 'ext;
                }
                pos = pl::add(&pos, &nn); // point to start of data for this extension
                if pl::gt(&pl::add(&pos, &len), &size) {
                    pl::method(it, &et, "Warn", vec![pl::s("Invalid BPG extension size")])?;
                    break 'ext;
                }
                if !tag_table_ptr.get(ty.to_bytes()).truthy() {
                    let msg = pl::concat(&pl::concat(&pl::s("Unrecognized BPG extension "), &ty), &pl::concat(&pl::concat(&pl::s(" ("), &len), &pl::s(" bytes)")));
                    pl::method(it, &et, "Warn", vec![msg, n(1)])?;
                    break 'body;
                }
                // libbpg (in my opinion) incorrectly copies the padding byte after the
                // "EXIF\0" APP1 header to the start of the BPG EXIF extension, so issue a
                // minor warning and ignore the padding if we find it before the TIFF header
                if pl::num_eq(&ty, &n(1)) && pl::gt(&len, &n(3)) && re(r"^.(II|MM)", "s").is_match(&pl::substr(&val(&buff), pos.to_i64(), Some(3))) {
                    pl::method(it, &et, "Warn", vec![pl::s("Ignored extra byte at start of EXIF extension"), n(1)])?;
                    pos = pl::add(&pos, &n(1));
                    len = pl::sub(&len, &n(1));
                }
                pl::method(
                    it,
                    &et,
                    "HandleTag",
                    vec![
                        tag_table_ptr.clone(),
                        ty,
                        Value::Undef,
                        pl::s("DataPt"),
                        rf(&buff),
                        pl::s("DataPos"),
                        data_pos.clone(),
                        pl::s("Start"),
                        pos.clone(),
                        pl::s("Size"),
                        len.clone(),
                        pl::s("Parent"),
                        pl::s("BPG"),
                    ],
                )?;
            }
            pos = pl::add(&pos, &len);
        }
        ret(n(1))
    })
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::BPG::Get_ue7", get_ue7);
    it.define_native("Image::ExifTool::BPG::ProcessBPG", process_bpg);
}
