//! Image::ExifTool::MISB (Image/ExifTool/MISB.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Motion Industry Standards Board metadata

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::cell;
use crate::perl::Value;
use crate::pl::{self, arg, n, re, ret};

const MODULE: &str = "Image::ExifTool::MISB";

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

fn m(it: &mut Interp, et: &Value, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::method(it, et, name, args)
}

/// Process KLV (Key-Length-Value) metadata
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 if anything was extracted
pub fn process_klv(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_start = dir_info.get("DirStart");
    let dir_end = pl::add(&dir_start, &dir_info.get("DirLen"));
    let mut rtn_val = n(0);

    m(it, &et, "VerboseDir", vec![dir_info.get("DirName"), Value::Undef, dir_info.get("DirLen")])?;

    // loop through KLV packets
    let mut pos = dir_start.clone();
    while pl::lt(&pos, &pl::sub(&dir_end, &n(1))) {
        let tag = call(it, "Get8u", vec![data_pt.clone(), pos.clone()])?;
        pos = pl::add(&pos, &n(1));
        let mut len = call(it, "Get8u", vec![data_pt.clone(), pos.clone()])?;
        pos = pl::add(&pos, &n(1));
        if len.to_i64() & 0x80 != 0 {
            let nn = len.to_i64() & 0x7f;
            if pl::gt(&pl::add(&pos, &n(nn)), &dir_end) {
                break;
            }
            len = n(0);
            for _ in 1..=nn {
                let b = call(it, "Get8u", vec![data_pt.clone(), pos.clone()])?;
                pos = pl::add(&pos, &n(1));
                len = pl::add(&pl::mul(&len, &n(256)), &b);
            }
        }
        if pl::gt(&pl::add(&pos, &len), &dir_end) {
            break;
        }
        // best guess at decoding the value
        let mut val;
        let mut tag_info = tag_table_ptr.get(tag.to_bytes());
        if !tag_info.truthy() {
            tag_info = Value::new_hash();
        }
        let mut format = tag_info.get("Format");
        if !format.truthy() {
            format = crate::runtime::lex_hash(it, MODULE, "defaultFormat").get(len.to_bytes());
        }
        if format.truthy() {
            val = call(it, "ReadValue", vec![data_pt.clone(), pos.clone(), format, Value::Undef, len.clone()])?;
        } else {
            // treat as string or binary data
            val = pl::substr(&data_pt.deref(), pos.to_i64(), Some(len.to_i64()));
            if !re(r"^[\t\n\r\x20-\x7e]*$", "").is_match(&val) {
                let _dat = val.clone();
                // ($val = \$val: the variable then holds a reference to itself)
                let c = cell(val);
                let r = Value::ScalarRef(c.clone());
                *c.borrow_mut() = r.clone();
                val = r;
            }
        }
        m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag, val, pl::s("DataPt"), data_pt.clone(), pl::s("Start"), pos.clone(), pl::s("Size"), len.clone()])?;
        rtn_val = n(1);
        pos = pl::add(&pos, &len);
    }
    ret(rtn_val)
}

/// Parse MISB metadata
/// Inputs: 0) ExifTool ref, 1) data ref, 2) tag table ref
/// Returns: 1 if something was extracted, 0 otherwise
pub fn parse_misb(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, data_pt, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let end = pl::length(&data_pt.deref());
    let mut rtn_val = n(0);
    let opts = et.get("OPTIONS");
    let unknown = opts.get("Unknown");
    let verbose = opts.get("Verbose");

    // increment document number in case we find any tags
    let dc = pl::add(&et.get("DOC_COUNT"), &n(1));
    et.set("DOC_COUNT", dc.clone());
    et.set("DOC_NUM", dc);
    et.set("INDENT", pl::concat(&et.get("INDENT"), &pl::s("| ")));

    // skip the 5-byte header (ref 1 pg. 68)
    //   0 - int8u: metadata service ID (0x00)
    //   1 - int8u: sequence number (increments each packet)
    //   2 - int8u: 0x0f (bits: cell fragmentation 00, decoder config 0, random access 0, reserved 1111)
    //   3-4 - int16u: data length (ie. packet length - 5)
    let mut pos = n(5);
    while pl::lt(&pl::add(&pos, &n(16)), &end) {
        let key = pl::unpack1("H*", &pl::substr(&data_pt.deref(), pos.to_i64(), Some(16)))?;
        pos = pl::add(&pos, &n(16));
        let mut len = call(it, "Get8u", vec![data_pt.clone(), pos.clone()])?;
        pos = pl::add(&pos, &n(1));
        if len.to_i64() & 0x80 != 0 {
            // is this a BER long form integer? (Basic Encoding Rules)
            let nn = len.to_i64() & 0x7f;
            len = n(0);
            if pl::gt(&pl::add(&pos, &n(nn)), &end) {
                return ret(rtn_val);
            }
            for _ in 1..=nn {
                let b = call(it, "Get8u", vec![data_pt.clone(), pos.clone()])?;
                pos = pl::add(&pos, &n(1));
                len = pl::add(&pl::mul(&len, &n(256)), &b);
            }
        }
        let mut tag_info = tag_table_ptr.get(key.to_bytes());
        if !tag_info.truthy() {
            if verbose.truthy() || unknown.truthy() {
                // (assume this is a data set, but it maybe be a simple tag)
                tag_info = Value::new_hash();
                tag_info.set("Name", pl::concat(&pl::s("MISB_"), &key));
                let sd = Value::new_hash();
                sd.set("TagTable", pl::s("Image::ExifTool::MISB::Unknown"));
                tag_info.set("SubDirectory", sd);
                let msg = pl::concat(&pl::concat(&pl::s("  [adding "), &tag_info.get("Name")), &pl::s("]\n"));
                m(it, &et, "VPrint", vec![n(0), msg])?;
                call(it, "AddTagToTable", vec![tag_table_ptr.clone(), key.clone(), tag_info.clone()])?;
            } else {
                // skip this record
                pos = pl::add(&pos, &len);
                continue;
            }
        }
        if pl::gt(&pl::add(&pos, &len), &end) {
            len = pl::sub(&end, &pos);
            let msg = pl::concat(&pl::concat(&et.get("INDENT"), &pl::s("(truncated record, only ")), &pl::concat(&len, &pl::s(" bytes available)\n")));
            m(it, &et, "VPrint", vec![n(0), msg])?;
        }
        let dir = tag_info.get("SubDirectory");
        if dir.get("ByteOrder").truthy() {
            call(it, "SetByteOrder", vec![dir.get("ByteOrder")])?;
        }
        let dir_info = Value::new_hash();
        dir_info.set("DataPt", data_pt.clone());
        dir_info.set("DirStart", pos.clone());
        dir_info.set("DirLen", len.clone());
        dir_info.set("DirName", tag_info.get("Name"));
        let table = call(it, "GetTagTable", vec![dir.get("TagTable")])?;
        if process_klv(it, vec![et.clone(), dir_info, table], Want::Scalar)?.pop().unwrap_or_default().truthy() {
            rtn_val = n(1);
        }
        call(it, "SetByteOrder", vec![pl::s("MM")])?;
        pos = pl::add(&pos, &len);
    }
    et.set("INDENT", pl::substr(&et.get("INDENT"), 0, Some(-2)));
    et.delete("DOC_NUM");
    if !rtn_val.truthy() {
        et.set("DOC_COUNT", pl::sub(&et.get("DOC_COUNT"), &n(1)));
    }
    ret(rtn_val)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::MISB::ProcessKLV", process_klv);
    it.define_native("Image::ExifTool::MISB::ParseMISB", parse_misb);
}
