//! Image::ExifTool::PhotoMechanic (Image/ExifTool/PhotoMechanic.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, n, ret};

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

/// Read/write PhotoMechanic information in a file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this file didn't contain PhotoMechanic information
/// - updates DataPos to point to start of PhotoMechanic information
/// - updates DirLen to trailer length
pub fn process_photo_mechanic(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let offset = {
        let o = dir_info.get("Offset");
        if o.truthy() {
            o
        } else {
            n(0)
        }
    };
    let outfile = dir_info.get("OutFile");
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let out = pl::method(it, &et, "Options", vec![pl::s("TextOut")])?;
    let mut rtn_val = n(0);
    let (buff, footer) = (cell(Value::Undef), cell(Value::Undef));

    #[allow(clippy::never_loop)]
    loop {
        // read and validate trailer footer (last 12 bytes)
        pl::at(163);
        if !(seek(it, &raf, pl::sub(&n(-12), &offset), 2)? && read_eq(it, &raf, &footer, &n(12))?) {
            break;
        }
        if !pl::re(r"cbipcbbl$", "").is_match(&val(&footer)) {
            break;
        }
        let size = pl::unpack1("N", &val(&footer))?;

        if ops::bit_and(&size, &Value::Int(0x80000000)).truthy() || !seek(it, &raf, pl::sub(&pl::neg(&size), &n(12)), 1)? {
            pl::method(it, &et, "Warn", vec![pl::s("Bad PhotoMechanic trailer")])?;
            break;
        }
        if !read_eq(it, &raf, &buff, &size)? {
            pl::method(it, &et, "Warn", vec![pl::s("Error reading PhotoMechanic trailer")])?;
            break;
        }
        rtn_val = n(1); // we read the trailer successfully

        // set variables returned in dirInfo hash
        let tell = pl::method(it, &raf, "Tell", vec![])?;
        dir_info.set("DataPos", pl::sub(&tell, &size));
        dir_info.set("DirLen", pl::add(&size, &n(12)));

        let dir_info2 = Value::new_hash();
        dir_info2.set("DataPt", rf(&buff));
        dir_info2.set("DataPos", dir_info.get("DataPos"));
        dir_info2.set("DirStart", n(0));
        dir_info2.set("DirLen", size.clone());
        dir_info2.set("Parent", pl::s("PhotoMechanic"));
        let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::PhotoMechanic::Main")])?;
        if !outfile.truthy() {
            // extract trailer information
            if verbose.truthy() || et.get("HTML_DUMP").truthy() {
                pl::method(it, &et, "DumpTrailer", vec![dir_info.clone()])?;
            }
            pl::method(it, &et, "ProcessDirectory", vec![dir_info2, tag_table_ptr])?;
        } else if et.get_or_new_hash("DEL_GROUP").get("PhotoMechanic").truthy() {
            // delete the trailer
            if verbose.truthy() {
                pl::print_to(it, Some(&out), &[pl::s("  Deleting PhotoMechanic trailer\n")])?;
            }
            et.set("CHANGED", pl::add(&et.get("CHANGED"), &n(1)));
        } else {
            // rewrite the trailer
            let new_pt;
            let new_buff = pl::method(it, &et, "WriteDirectory", vec![dir_info2, tag_table_ptr])?;
            if !new_buff.is_undef() {
                let nb = cell(new_buff);
                new_pt = rf(&nb); // write out the modified trailer
                let pad = pl::sub(&n(0x800), &pl::length(&val(&nb)));
                if pl::gt(&pad, &n(0)) && !et.get_or_new_hash("OPTIONS").get_or_new_hash("Compact").get("NoPadding").truthy() {
                    // pad out to 2kB like PhotoMechanic does
                    let p = Value::bytes(vec![0u8; pad.to_i64().max(0) as usize]);
                    let v = pl::concat(&val(&nb), &p);
                    *nb.borrow_mut() = v;
                }
                // generate new footer
                let f = pl::concat(&pl::pack("N", &[pl::length(&new_pt.deref())])?, &pl::s("cbipcbbl"));
                *footer.borrow_mut() = f;
            } else {
                new_pt = rf(&buff); // just copy existing trailer
            }
            // write out the trailer
            if !pl::call(it, "Image::ExifTool::Write", vec![outfile.clone(), new_pt.deref(), val(&footer)])?.truthy() {
                rtn_val = n(-1);
            }
        }
        break;
    }
    ret(rtn_val)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::PhotoMechanic::ProcessPhotoMechanic", process_photo_mechanic);
}
