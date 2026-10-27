//! Image::ExifTool::Scalado (Image/ExifTool/Scalado.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read the JPEG APP4 SCALADO segment.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::Value;
use crate::pl::{self, arg, n, ret};

fn s(x: &str) -> Value {
    pl::s(x)
}

fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

//------------------------------------------------------------------------------
// Extract information from the JPEG APP4 SCALADO segment
// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
// Returns: 1 on success
pub fn process_scalado(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let mut pos = n(0);
    pl::at(73);
    let mut end = pl::length(&pl::sderef(&data_pt)?);
    let unknown = pl::method(it, &et, "Options", vec![s("Unknown")])?;

    pl::method(it, &et, "VerboseDir", vec![s("APP4 SCALADO"), Value::Undef, end.clone()])?;
    call(it, "SetByteOrder", vec![s("MM")])?;

    loop {
        if pl::gt(&pl::add(&pos, &n(12)), &end) {
            break;
        }
        let tag = pl::substr(&data_pt.deref(), pos.to_i64(), Some(4));
        let ver = call(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(4))])?; // (looks like a version for some tags)
        if !tag_table_ptr.get(tag.to_bytes()).truthy() && unknown.truthy() {
            let mut name = tag.clone();
            pl::tr(&mut name, "-A-Za-z0-9_", "", "dc");
            if !pl::length(&name).truthy() {
                break; // stop if tag is garbage
            }
            let ti = Value::new_hash();
            ti.set("Name", pl::concat(&s("Scalado_"), &name));
            ti.set("Description", pl::concat(&s("Scalado "), &name));
            ti.set("Unknown", n(1));
            call(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), ti])?;
        }
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![
                tag_table_ptr.clone(),
                tag.clone(),
                Value::Undef,
                s("DataPt"),
                data_pt.clone(),
                s("Start"),
                pl::add(&pos, &n(8)),
                s("Size"),
                n(4),
                s("Extra"),
                pl::concat(&s(", ver "), &ver),
            ],
        )?;
        if pl::str_eq(&tag, "SPMO") {
            let val = call(it, "Get32u", vec![data_pt.clone(), pl::add(&pos, &n(8))])?;
            if pl::lt(&ver, &n(5)) {
                // (I don't have samples for version 3 or 4, so I'm not sure about these)
                end = pl::sub(&end, &val); // SPMO gives trailer data length
            } else {
                end = pl::add(&val, &n(12)); // SPMO gives length of Scalado directory (excepting this entry)
            }
        }
        pos = pl::add(&pos, &n(12));
    }
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Scalado::ProcessScalado", process_scalado);
}
