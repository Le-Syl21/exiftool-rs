//! Image::ExifTool::PrintIM (Image/ExifTool/PrintIM.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::substr;
use crate::perl::Value;
use crate::pl::{self, arg, ret};

fn n(i: i64) -> Value {
    Value::Int(i)
}

/// Process PrintIM IFD
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_print_im(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let offset = dir_info.get("DirStart");
    let size = dir_info.get("DirLen");
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;

    if !size.truthy() {
        pl::method(it, &et, "Warn", vec![pl::s("Empty PrintIM data"), n(1)])?;
        return ret(n(0));
    }
    if !pl::gt(&size, &n(15)) {
        pl::method(it, &et, "Warn", vec![pl::s("Bad PrintIM data")])?;
        return ret(n(0));
    }
    pl::at(59);
    if !substr(it, &pl::sderef(&data_pt)?, &offset, Some(&n(7)))?.eq_str("PrintIM") {
        pl::method(it, &et, "Warn", vec![pl::s("Invalid PrintIM header")])?;
        return ret(n(0));
    }
    // check size of PrintIM block
    let mut num = pl::call(it, "Image::ExifTool::Get16u", vec![data_pt.clone(), pl::add(&offset, &n(14))])?;
    if pl::lt(&size, &pl::add(&n(16), &pl::mul(&num, &n(6)))) {
        // size is too big, maybe byte ordering is wrong
        pl::call(it, "Image::ExifTool::ToggleByteOrder", vec![])?;
        num = pl::call(it, "Image::ExifTool::Get16u", vec![data_pt.clone(), pl::add(&offset, &n(14))])?;
        if pl::lt(&size, &pl::add(&n(16), &pl::mul(&num, &n(6)))) {
            pl::method(it, &et, "Warn", vec![pl::s("Bad PrintIM size")])?;
            return ret(n(0));
        }
    }
    if verbose.truthy() {
        pl::method(it, &et, "VerboseDir", vec![pl::s("PrintIM"), num.clone()])?;
    }
    pl::at(75);
    let version = substr(it, &pl::sderef(&data_pt)?, &pl::add(&offset, &n(8)), Some(&n(4)))?;
    pl::method(
        it,
        &et,
        "HandleTag",
        vec![tag_table_ptr.clone(), pl::s("PrintIMVersion"), version, pl::s("DataPt"), data_pt.clone(), pl::s("Start"), pl::add(&offset, &n(8)), pl::s("Size"), n(4)],
    )?;
    let mut i = n(0);
    while pl::lt(&i, &num) {
        let pos = pl::add(&pl::add(&offset, &n(16)), &pl::mul(&i, &n(6)));
        let tag = pl::call(it, "Image::ExifTool::Get16u", vec![data_pt.clone(), pos.clone()])?;
        let val = pl::call(it, "Image::ExifTool::Get32u", vec![data_pt.clone(), pl::add(&pos, &n(2))])?;
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![tag_table_ptr.clone(), tag, val, pl::s("Index"), i.clone(), pl::s("DataPt"), data_pt.clone(), pl::s("Start"), pl::add(&pos, &n(2)), pl::s("Size"), n(4)],
        )?;
        i = pl::add(&i, &n(1));
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::PrintIM::ProcessPrintIM", process_print_im);
}
