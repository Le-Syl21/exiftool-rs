//! Image::ExifTool::PGF (Image/ExifTool/PGF.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

/// `\$buff` of a `my` variable kept in a cell.
fn rf(c: &Cell) -> Value {
    Value::ScalarRef(c.clone())
}

fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `$raf->METHOD(...)` dies as perl does on an undefined `$raf` (the read
/// itself runs as a Perl snippet, whose messages name the snippet).
fn raf_defined(raf: &Value, method: &str) -> ER<()> {
    if raf.is_undef() {
        return crate::interp::eval::die(format!("Can't call method \"{method}\" on an undefined value"));
    }
    Ok(())
}

/// Extract information from a PGF image
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid PGF file
pub fn process_pgf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);

    // read header and check magic number
    pl::at(76);
    raf_defined(&raf, "Read")?;
    if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(24))?, &n(24)) {
        return ret(n(0));
    }
    let b = buff.borrow().clone();
    let Some(m) = re(r"^PGF(.)", "s").m(&b) else { return ret(n(0)) };
    let ver = cap(&m, 1).to_bytes().first().copied().unwrap_or(0) as i64; // ord $1
    pl::method(it, &et, "SetFileType", vec![])?;
    call(it, "SetByteOrder", vec![pl::s("II")])?;

    // currently support only version 0x36
    if ver != 0x36 {
        pl::method(it, &et, "Error", vec![pl::sprintf("Unsupported PGF version 0x%.2x", &[n(ver)])])?;
        return ret(n(1));
    }
    // extract information from the PGF header
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::PGF::Main")])?;
    let di = Value::new_hash();
    di.set("DataPt", rf(&buff));
    di.set("DataPos", n(0));
    pl::method(it, &et, "ProcessDirectory", vec![di, tag_table_ptr])?;

    let mut len = pl::sub(&call(it, "Get32u", vec![rf(&buff), n(4)])?, &n(16)); // length of post-header data

    // skip colour table if necessary
    if pl::num_eq(&et.get("PGFColorMode"), &n(2)) {
        let d = if pl::method(it, &raf, "Seek", vec![n(1024), n(1)])?.truthy() { n(1024) } else { len.clone() };
        len = pl::sub(&len, &d);
    }

    // extract information from the embedded metadata image (PNG format)
    if pl::gt(&len, &n(0)) && pl::lt(&len, &n(0x1000000)) && pl::num_eq(&raf_read_cell(it, &raf, &buff, len.clone())?, &len) {
        let opts = Value::new_hash();
        opts.set("ReEntry", n(1));
        pl::method(it, &et, "ExtractInfo", vec![rf(&buff), opts])?;
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::PGF::ProcessPGF", process_pgf);
}
