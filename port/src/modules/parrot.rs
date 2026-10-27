//! Image::ExifTool::Parrot (Image/ExifTool/Parrot.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read timed metadata from Parrot drone videos. The tag tables are data;
//! this is the module's one sub.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::quick_time_stream::{cat, len_of, m, n, s, warn_lvl};
use crate::perl::Value;
use crate::pl::{self, arg, re, ret};

/// Parse Parrot 'mett' timed metadata
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
/// (this metadata design is really stupid -- you need to know the size of the base structures)
pub fn process_mett(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data = data_pt.deref();
    let data_pos = dir_info.get("DataPos");
    let dir_end = len_of(&data);
    let ds = dir_info.get("DirStart");
    let mut pos = if ds.truthy() { ds.to_i64() } else { 0 };
    let mt = et.get("MetaType");
    let meta_type = if mt.truthy() { mt } else { s("") };

    m(it, &et, "VerboseDir", vec![s("Parrot mett"), Value::Undef, n(dir_end)])?;

    if tag_tbl.get(meta_type.to_bytes()).truthy() {
        // loop through ARCore records
        while pos < dir_end - 2 {
            if !pl::substr(&data, pos, Some(1)).eq_str("\x0a") {
                break;
            }
            let mut len = pl::substr(&data, pos + 1, Some(1)).to_bytes().first().copied().unwrap_or(0) as i64;
            if pos + len + 2 > dir_end {
                warn_lvl(it, &et, cat(&[&s("Unexpected length for "), &meta_type, &s(" record")]), 1)?;
                break;
            }
            if len == 0 {
                len = dir_end - pos - 2; // use entire record if length is 0
            }
            m(it, &et, "HandleTag", vec![tag_tbl.clone(), meta_type.clone(), Value::Undef, s("DataPt"), data_pt.clone(), s("DataPos"), data_pos.clone(), s("DirStart"), n(pos), s("DirLen"), n(len), s("Base"), dir_info.get("Base")])?;
            pos += len + 2;
        }
        return ret(n(1));
    }
    while pos + 4 < dir_end {
        let u = pl::unpack(&format!("x{pos}a2n"), &data)?;
        let (mut id, nwords) = (arg(&u, 0), arg(&u, 1));
        let size;
        if !re(r"^[EP]\d", "").is_match(&id) {
            // no ID so this should be a 60-byte V1 recording record, otherwise give up
            if dir_end != 60 {
                break;
            }
            id = s("P1"); // generate a fake ID
            // ignore the first 4 of the record so the fields will align with
            // the other V1 records (unfortunately, this means that we won't
            // decode the V1 recording frame timestamp, but oh well)
            pos += 4;
            size = n(dir_end - pos);
        // must override size for P3 and P3 records since it includes the extensions (augh!)
        } else if id.eq_str("P2") {
            size = n(56);
        } else if id.eq_str("P3") {
            size = n(72);
        } else {
            size = pl::add(&pl::mul(&nwords, &n(4)), &n(4));
        }
        if pl::gt(&pl::add(&n(pos), &size), &n(dir_end)) {
            break;
        }
        m(it, &et, "HandleTag", vec![tag_tbl.clone(), id, Value::Undef, s("DataPt"), data_pt.clone(), s("DataPos"), data_pos.clone(), s("Base"), dir_info.get("Base"), s("Start"), n(pos), s("Size"), size.clone()])?;
        pos += size.to_i64();
    }
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Parrot::Process_mett", process_mett);
}
