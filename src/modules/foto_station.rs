//! Image::ExifTool::FotoStation (Image/ExifTool/FotoStation.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read/write FotoWare FotoStation trailer

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell};
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

/// `++$$et{CHANGED}`
fn inc_changed(et: &Value) {
    et.set("CHANGED", pl::add(&et.get("CHANGED"), &n(1)));
}

/// Read/write FotoStation information in a file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this file didn't contain FotoStation information
/// - updates DataPos to point to start of FotoStation information
/// - updates DirLen to trailer length
pub fn process_foto_station(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    if !et.truthy() {
        return ret(n(1)); // allow dummy access to autoload this package
    }
    let (buff, footer) = (cell(Value::Undef), cell(Value::Undef));
    let mut dir_buff = Value::Undef;
    let mut tag_table_ptr = Value::Undef;
    let raf = dir_info.get("RAF");
    let outfile = dir_info.get("OutFile");
    let offset = {
        let o = dir_info.get("Offset");
        if o.truthy() {
            o
        } else {
            n(0)
        }
    };
    let mut verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let out = pl::method(it, &et, "Options", vec![pl::s("TextOut")])?;
    let mut rtn_val = n(0);

    dir_info.set("DirLen", n(0)); // initialize returned trailer length
    pl::at(134);
    seek(it, &raf, pl::neg(&offset), 2)?; // seek to specified offset from end of file

    // loop through FotoStation records
    loop {
        // look for trailer signature
        if !(seek(it, &raf, n(-10), 1)? && pl::num_eq(&read(it, &raf, &footer, &n(10))?, &n(10))) {
            break;
        }
        let u = pl::unpack("nNN", &val(&footer))?;
        let (tag, mut size, sig) = (arg(&u, 0), arg(&u, 1), arg(&u, 2));
        if !(pl::num_eq(&sig, &n(0xa1b2c3d4)) && pl::ge(&size, &n(10)) && seek(it, &raf, pl::neg(&size), 1)?) {
            break;
        }
        size = pl::sub(&size, &n(10)); // size of data only
        if !pl::num_eq(&read(it, &raf, &buff, &size)?, &size) {
            break;
        }
        seek(it, &raf, pl::neg(&size), 1)?;
        // set variables returned in dirInfo hash
        let tell = pl::method(it, &raf, "Tell", vec![])?;
        dir_info.set("DataPos", tell);
        dir_info.set("DirLen", pl::add(&dir_info.get("DirLen"), &pl::add(&size, &n(10))));

        if !tag_table_ptr.truthy() {
            tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::FotoStation::Main")])?;
            call(it, "SetByteOrder", vec![pl::s("MM")])?; // necessary for the binary data
            rtn_val = n(1); // we found a valid FotoStation trailer
        }
        if !outfile.truthy() {
            // print verbose trailer information
            if verbose.truthy() || et.get("HTML_DUMP").truthy() {
                let h = Value::new_hash();
                h.set("RAF", raf.clone());
                h.set("DataPos", dir_info.get("DataPos"));
                h.set("DirLen", pl::add(&size, &n(10)));
                h.set("DirName", pl::concat(&pl::s("FotoStation_"), &tag));
                pl::method(it, &et, "DumpTrailer", vec![h])?;
            }
            // extract information for this tag
            pl::method(
                it,
                &et,
                "HandleTag",
                vec![
                    tag_table_ptr.clone(),
                    tag.clone(),
                    val(&buff),
                    pl::s("DataPt"),
                    Value::ScalarRef(buff.clone()),
                    pl::s("Start"),
                    n(0),
                    pl::s("Size"),
                    size.clone(),
                    pl::s("DataPos"),
                    dir_info.get("DataPos"),
                ],
            )?;
            continue;
        }
        if et.get("DEL_GROUP").get("FotoStation").truthy() {
            if verbose.truthy() {
                pl::print_to(it, Some(&out), &[pl::s("  Deleting FotoStation trailer\n")])?;
            }
            verbose = n(0); // no more verbose messages after this
            inc_changed(&et);
            continue;
        }
        // rewrite this information
        let tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
        if tag_info.truthy() {
            let mut new_val = Value::Undef;
            let tag_name = tag_info.get("Name");
            if tag_info.get("SubDirectory").truthy() {
                let subdir_info = Value::new_hash();
                subdir_info.set("DataPt", Value::ScalarRef(buff.clone()));
                subdir_info.set("DirStart", n(0));
                subdir_info.set("DirLen", size.clone());
                subdir_info.set("DataPos", dir_info.get("DataPos"));
                subdir_info.set("DirName", tag_name.clone());
                subdir_info.set("Parent", pl::s("FotoStation"));
                let sub_table = call(it, "GetTagTable", vec![tag_info.path(&["SubDirectory", "TagTable"])])?;
                new_val = pl::method(it, &et, "WriteDirectory", vec![subdir_info, sub_table])?;
            } else {
                let nv_hash = pl::method(it, &et, "GetNewValueHash", vec![tag_info.clone()])?;
                if pl::gt(&pl::method(it, &et, "IsOverwriting", vec![nv_hash.clone()])?, &n(0)) {
                    new_val = pl::method(it, &et, "GetNewValue", vec![nv_hash])?;
                    if new_val.is_undef() {
                        new_val = pl::s("");
                    }
                    if pl::gt(&verbose, &n(1)) {
                        let nn = pl::length(&new_val);
                        if size.truthy() {
                            let line = pl::concat(&pl::concat(&pl::concat(&pl::concat(&pl::s("    - FotoStation:"), &tag_name), &pl::s(" (")), &size), &pl::s(" bytes)\n"));
                            pl::print_to(it, Some(&out), &[line])?;
                        }
                        if nn.truthy() {
                            let line = pl::concat(&pl::concat(&pl::concat(&pl::concat(&pl::s("    + FotoStation:"), &tag_name), &pl::s(" (")), &nn), &pl::s(" bytes)\n"));
                            pl::print_to(it, Some(&out), &[line])?;
                        }
                    }
                    inc_changed(&et);
                }
            }
            if !new_val.is_undef() {
                // note: length may be 0 here, but we write the empty record anyway
                *buff.borrow_mut() = new_val.clone();
                size = pl::add(&pl::length(&new_val), &n(10));
                *footer.borrow_mut() = pl::pack("nNN", &[tag.clone(), size.clone(), sig.clone()])?;
            }
        }
        if !dir_buff.is_undef() {
            // maintain original record order
            dir_buff = pl::concat(&pl::concat(&val(&buff), &val(&footer)), &dir_buff);
        } else {
            dir_buff = pl::concat(&val(&buff), &val(&footer));
        }
    }
    // write the modified FotoStation trailer
    if dir_buff.truthy() && !call(it, "Write", vec![outfile.clone(), dir_buff])?.truthy() {
        rtn_val = n(-1);
    }
    ret(rtn_val)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::FotoStation::ProcessFotoStation", process_foto_station);
}
