//! Image::ExifTool::Leaf (Image/ExifTool/Leaf.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Creo Leaf EXIF meta information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::substr;
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `{ KEY => VAL, ... }`
fn hash(pairs: Vec<(&str, Value)>) -> Value {
    let h = Value::new_hash();
    for (k, v) in pairs {
        h.set(k, v);
    }
    h
}

/// `$a || $b`
fn or(a: Value, b: impl FnOnce() -> Value) -> Value {
    if a.truthy() {
        a
    } else {
        b()
    }
}

//------------------------------------------------------------------------------
/// Process Leaf information
/// Inputs: 0) ExifTool object reference
///         1) Reference to directory information hash
///         2) Pointer to tag table for this directory
/// Returns: 1 on success, otherwise returns 0 and sets a Warning
pub fn process_leaf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_start = or(dir_info.get("DirStart"), || n(0));
    let dir_len = or(dir_info.get("DirLen"), || pl::sub(&dir_info.get("DataLen"), &dir_start));
    let dir_end = pl::add(&dir_start, &dir_len);
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let mut pos = dir_start.clone();
    let hdr_len = n(52); // header length for PKTS information
    let mut success = Value::Undef;

    if verbose.truthy() {
        pl::method(it, &et, "VerboseDir", vec![pl::s("Leaf")])?;
    }
    loop {
        if pl::gt(&pl::add(&pos, &hdr_len), &dir_end) {
            break;
        }
        let header = substr(it, &data_pt.deref(), &pos, Some(&hdr_len))?;
        if !pl::str_eq(&substr(it, &header, &n(0), Some(&n(4)))?, "PKTS") {
            break;
        }
        success = n(1);
        let size = call(it, "Get32u", vec![pl::sref(header.clone()), n(48)])?;
        pos = pl::add(&pos, &hdr_len);
        if pl::gt(&pl::add(&pos, &size), &dir_end) {
            pl::method(it, &et, "Warn", vec![pl::s("Truncated Leaf data")])?;
            break;
        }
        let mut tag = substr(it, &header, &n(8), Some(&n(40)))?;
        re(r"\0.*", "s").subst(&mut tag, false, |_| Vec::new());
        if !tag.truthy() {
            continue;
        }
        let mut tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
        // generate tag info for unknown tags
        let mut val = if tag_info.truthy() && tag_info.get("Format").truthy() {
            call(it, "ReadValue", vec![data_pt.clone(), pos.clone(), tag_info.get("Format"), Value::Undef, size.clone()])?
        } else {
            substr(it, &data_pt.deref(), &pos, Some(&size))?
        };
        if !tag_info.truthy() {
            let mut name = pl::ucfirst(&tag);
            re(r"_(.)", "").subst(&mut name, true, |c| cap(c, 1).to_bytes().to_ascii_uppercase());
            let main = pl::pkg_hash(it, "Image::ExifTool::Leaf::Main");
            if re(r"^PKTS\0\0\0\x01", "").is_match(&val) {
                // also unpack unknown directories
                tag_info = hash(vec![("Name", name), ("SubDirectory", hash(vec![("TagTable", pl::s("Image::ExifTool::Leaf::Unknown"))]))]);
            } else if !tag_table_ptr.same_ref(&main) || pl::method(it, &et, "Options", vec![pl::s("Unknown")])?.truthy() {
                let lim = Value::Code(it.sub_ref("Image::ExifTool::LimitLongValues"));
                tag_info = hash(vec![("Name", name), ("Writable", n(0)), ("PrintConv", lim)]);
                // make tags in main table unknown because they tend to be binary
                if tag_table_ptr.same_ref(&main) {
                    tag_info.set("Unknown", n(1));
                }
            }
            if tag_info.truthy() {
                call(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info.clone()])?;
            }
        }
        if verbose.truthy() {
            pl::method(
                it,
                &et,
                "VerboseInfo",
                vec![
                    tag.clone(),
                    tag_info.clone(),
                    pl::s("Table"),
                    tag_table_ptr.clone(),
                    pl::s("Value"),
                    val.clone(),
                    pl::s("DataPt"),
                    data_pt.clone(),
                    pl::s("DataPos"),
                    dir_info.get("DataPos"),
                    pl::s("Size"),
                    size.clone(),
                    pl::s("Start"),
                    pos.clone(),
                ],
            )?;
        }
        if tag_info.truthy() {
            if tag_info.get("SubDirectory").truthy() {
                let subdir_info = hash(vec![
                    ("DataPt", data_pt.clone()),
                    ("DirLen", size.clone()),
                    ("DirStart", pos.clone()),
                    ("DataPos", dir_info.get("DataPos")),
                    ("DirName", pl::s("Leaf PKTS")),
                ]);
                let sub_table = call(it, "GetTagTable", vec![tag_info.get("SubDirectory").get("TagTable")])?;
                pl::method(it, &et, "ProcessDirectory", vec![subdir_info, sub_table])?;
            } else {
                pl::tr(&mut val, "\n", " ", ""); // translate newlines to spaces
                re(r"\0+$", "").subst(&mut val, false, |_| Vec::new()); // remove null terminators
                pl::method(it, &et, "FoundTag", vec![tag_info.clone(), val])?;
            }
        }
        pos = pl::add(&pos, &size);
    }
    if !success.truthy() {
        pl::method(it, &et, "Warn", vec![pl::s("Bad format Leaf data")])?;
    }
    ret(success)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Leaf::ProcessLeaf", process_leaf);
}
