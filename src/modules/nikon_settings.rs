//! Image::ExifTool::NikonSettings (Image/ExifTool/NikonSettings.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! The tag table is data; this is the module's directory processor.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::Value;
use crate::pl::{self, arg, ret};

fn n(i: i64) -> Value {
    Value::Int(i)
}

fn s(x: &str) -> Value {
    pl::s(x)
}

//------------------------------------------------------------------------------
/// Process NikonSettings directory (ref PH)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_nikon_settings(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));

    // NikonSettings directory has a 24-byte header:
    //  0x00 - undef[4]: '0100'
    //  0x04 - int32u:   1 (D-models), 2 (Z-models)
    //  0x08 - undef[4]: '0100'
    //  0x0c - int32u:   1 (Z7), 2 (D850), 3 (D780,Z50), 4 (D6,Z5), 5(Z7m2)
    //  0x10 - undef[4]: firmware version (eg. '0110' for firmware 1.10)
    //  0x14 - int32u:   number of entries in directory

    if pl::lt(&dir_info.get("DirLen"), &n(24)) {
        return ret(n(0)); // sanity check
    }

    let data_pt = dir_info.get("DataPt");
    let start = dir_info.get("DirStart");
    let mut num = pl::call(it, "Image::ExifTool::Get32u", vec![data_pt.clone(), pl::add(&start, &n(0x14))])?;

    pl::method(it, &et, "VerboseDir", vec![s("NikonSettings"), num.clone()])?;

    let nn = pl::int(&pl::div(&pl::sub(&dir_info.get("DirLen"), &n(0x18)), &n(8))?);
    if pl::lt(&nn, &num) {
        let msg = pl::concat(&pl::concat(&s("Missing "), &pl::sub(&num, &nn)), &s(" NikonSettings entries"));
        pl::method(it, &et, "Warn", vec![msg, n(1)])?;
        num = nn;
    } else if pl::gt(&nn, &num) {
        pl::method(it, &et, "Warn", vec![s("Unused space in NikonSettings directory"), n(1)])?;
    }
    let mut i = n(0);
    while pl::lt(&i, &num) {
        let entry = pl::add(&pl::add(&start, &n(0x18)), &pl::mul(&i, &n(8)));
        let tag = pl::call(it, "Image::ExifTool::Get16u", vec![data_pt.clone(), entry.clone()])?;
        // this is odd, but either the format is 16-bit and always big-endian,
        // or it is 8-bit and we have an unknown byte in the entry...
        let fmt = pl::call(it, "Image::ExifTool::Get8u", vec![data_pt.clone(), pl::add(&entry, &n(3))])?;
        let val = pl::call(it, "Image::ExifTool::Get32u", vec![data_pt.clone(), pl::add(&entry, &n(4))])?;
        // abort if the tag has a format that we haven't yet seen
        // (assuming this is a size/format code.  So far we have only seen a code of 4)
        if !pl::num_eq(&fmt, &n(4)) {
            let msg = pl::sprintf("Unknown format $fmt for NikonSettings tag 0x%.4x", &[tag]);
            pl::method(it, &et, "Warn", vec![msg])?;
            break;
        }
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![
                tag_table_ptr.clone(),
                tag,
                val,
                s("DataPt"),
                data_pt.clone(),
                s("DataPos"),
                dir_info.get("DataPos"),
                s("Base"),
                dir_info.get("Base"),
                s("Start"),
                pl::add(&entry, &n(4)),
                s("Size"),
                n(4),
                s("Format"),
                s("int32u"),
                s("Index"),
                i.clone(),
            ],
        )?;
        i = pl::add(&i, &n(1));
    }
    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::NikonSettings::ProcessNikonSettings", process_nikon_settings);
}
