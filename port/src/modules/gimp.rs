//! Image::ExifTool::GIMP (Image/ExifTool/GIMP.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::exiftool::part2::get32u;
use crate::modules::random_access::{cell, raf_read_cell};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

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

fn g32u(it: &mut Interp, data_pt: &Value, pos: Value) -> ER<Value> {
    Ok(get32u(it, vec![data_pt.clone(), pos], Want::Scalar)?.pop().unwrap_or_default())
}

/// Read information in a GIMP XCF parasite data (ref PH)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_parasites(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let unknown = {
        let u = pl::method(it, &et, "Options", vec![pl::s("Unknown")])?;
        if u.truthy() {
            u
        } else {
            pl::method(it, &et, "Options", vec![pl::s("Verbose")])?
        }
    };
    let data_pt = dir_info.get("DataPt");
    let mut pos = {
        let p = dir_info.get("DirStart");
        if p.truthy() {
            p
        } else {
            n(0)
        }
    };
    let end = pl::length(&data_pt.deref());
    pl::method(it, &et, "VerboseDir", vec![pl::s("Parasites"), Value::Undef, end.clone()])?;
    loop {
        if pl::gt(&pl::add(&pos, &n(4)), &end) {
            break;
        }
        let mut size = g32u(it, &data_pt, pos.clone())?; // length of tag string
        pos = pl::add(&pos, &n(4));
        if pl::gt(&pl::add(&pl::add(&pos, &size), &n(8)), &end) {
            break;
        }
        let mut tag = pl::substr(&data_pt.deref(), pos.to_i64(), Some(size.to_i64()));
        pos = pl::add(&pos, &size);
        re(r"\0.*", "s").subst(&mut tag, false, |_| Vec::new()); // trim at null terminator
        // my $flags = Get32u($dataPt, $pos);  (ignore flags)
        size = g32u(it, &data_pt, pl::add(&pos, &n(4)))?; // length of data
        pos = pl::add(&pos, &n(8));
        if pl::gt(&pl::add(&pos, &size), &end) {
            break;
        }
        if !tag_table_ptr.get(tag.to_bytes()).truthy() && unknown.truthy() {
            let mut name = tag.clone();
            pl::tr(&mut name, "-_A-Za-z0-9", "", "dc");
            re(r"^gimp-", "").subst(&mut name, false, |_| Vec::new());
            if pl::length(&name).to_i64() == 0 {
                continue;
            }
            name = pl::ucfirst(&name);
            re(r"([a-z])-([a-z])", "").subst(&mut name, true, |c| [&cap(c, 1).to_bytes()[..], &pl::ucfirst(&cap(c, 2)).to_bytes()[..]].concat());
            if pl::length(&name).to_i64() <= 1 {
                name = pl::concat(&pl::s("GIMP-"), &name);
            }
            let ti = Value::new_hash();
            ti.set("Name", name);
            ti.set("Unknown", n(1));
            pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), ti])?;
        }
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![tag_table_ptr.clone(), tag, Value::Undef, pl::s("DataPt"), data_pt.clone(), pl::s("Start"), pos.clone(), pl::s("Size"), size.clone()],
        )?;
        pos = pl::add(&pos, &size);
    }
    ret(n(1))
}

/// Read information in a GIMP XCF document
/// Inputs: 0) ExifTool ref, 1) dirInfo ref
/// Returns: 1 on success, 0 if this wasn't a valid XCF file
pub fn process_xcf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);

    if !read_eq(it, &raf, &buff, &n(26))? {
        return ret(n(0));
    }
    if !re(r"^gimp xcf ", "").is_match(&val(&buff)) {
        return ret(n(0));
    }

    let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::GIMP::Main")])?;
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    pl::method(it, &et, "SetFileType", vec![])?;
    pl::call(it, "Image::ExifTool::SetByteOrder", vec![pl::s("MM")])?;

    // process the XCF header
    pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("header"), val(&buff)])?;

    // skip over precision for XCV version 4 or later
    if let Some(m) = re(r"^v0*(\d+)", "").m(&et.get("XCFVersion")) {
        if pl::ge(&cap(&m, 1), &n(4)) {
            seek(it, &raf, n(4), 1)?;
        }
    }

    // loop through image properties
    loop {
        if !read_eq(it, &raf, &buff, &n(8))? {
            break;
        }
        let tag = g32u(it, &rf(&buff), n(0))?;
        if !tag.truthy() {
            break;
        }
        let size = g32u(it, &rf(&buff), n(4))?;
        if verbose.truthy() {
            let msg = pl::concat(&pl::concat(&pl::s("XCF property "), &tag), &pl::concat(&pl::concat(&pl::s(" ("), &size), &pl::s(" bytes):\n")));
            pl::method(it, &et, "VPrint", vec![n(0), msg])?;
        }
        if !tag_table_ptr.get(tag.to_bytes()).truthy() {
            seek(it, &raf, size.clone(), 1)?;
            continue;
        }
        if !read_eq(it, &raf, &buff, &size)? {
            break;
        }
        let tell = pl::method(it, &raf, "Tell", vec![])?;
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![tag_table_ptr.clone(), tag, Value::Undef, pl::s("DataPt"), rf(&buff), pl::s("DataPos"), pl::sub(&tell, &size), pl::s("Size"), size.clone()],
        )?;
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::GIMP::ProcessParasites", process_parasites);
    it.define_native("Image::ExifTool::GIMP::ProcessXCF", process_xcf);
}
