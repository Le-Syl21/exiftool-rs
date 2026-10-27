//! Image::ExifTool::RSRC (Image/ExifTool/RSRC.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Mac OS resource information.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::raf_read;
use crate::perl::ops;
use crate::perl::Value;
use crate::pl::{self, arg, n, re, ret};

fn s(x: &str) -> Value {
    pl::s(x)
}

fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `ord $str`
fn ord(v: &Value) -> Value {
    n(v.to_bytes().first().copied().unwrap_or(0) as i64)
}

//------------------------------------------------------------------------------
// Read information from a Mac resource file (ref 1)
// Inputs: 0) ExifTool ref, 1) dirInfo ref
// Returns: 1 on success, 0 if this wasn't a valid resource file
pub fn process_rsrc(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let mut raf = dir_info.get("RAF");
    let (mut hdr, mut map, mut buff) = (Value::Undef, Value::Undef, Value::Undef);

    // allow access with data reference
    if !raf.truthy() {
        raf = pl::method(it, &s("File::RandomAccess"), "new", vec![dir_info.get("DataPt")])?;
    }

    // attempt to validate the format as thoroughly as practical
    if !pl::num_eq(&raf_read(it, &raf, &mut hdr, n(30))?, &n(30)) {
        return ret(n(0));
    }
    let u = pl::unpack("N*", &hdr)?;
    let (dat_off, map_off, dat_len, map_len) = (arg(&u, 0), arg(&u, 1), arg(&u, 2), arg(&u, 3));
    if !pl::method(it, &raf, "Seek", vec![n(0), n(2)])?.truthy() {
        return ret(n(0));
    }
    let f_len = pl::method(it, &raf, "Tell", vec![])?;
    if pl::lt(&dat_off, &n(0x10)) || pl::gt(&pl::add(&dat_off, &dat_len), &f_len) {
        return ret(n(0));
    }
    if pl::lt(&map_off, &n(0x10)) || pl::gt(&pl::add(&map_off, &map_len), &f_len) || pl::lt(&map_len, &n(30)) {
        return ret(n(0));
    }
    if pl::lt(&dat_off, &map_off) && pl::gt(&pl::add(&dat_off, &dat_len), &map_off) {
        return ret(n(0));
    }
    if pl::lt(&map_off, &dat_off) && pl::gt(&pl::add(&map_off, &map_len), &dat_off) {
        return ret(n(0));
    }

    // read the resource map
    if !(pl::method(it, &raf, "Seek", vec![map_off.clone(), n(0)])?.truthy() && pl::num_eq(&raf_read(it, &raf, &mut map, map_len.clone())?, &map_len)) {
        return ret(n(0));
    }
    call(it, "SetByteOrder", vec![s("MM")])?;
    let mp = pl::sref(map.clone());
    let type_off = call(it, "Get16u", vec![mp.clone(), n(24)])?;
    let name_off = call(it, "Get16u", vec![mp.clone(), n(26)])?;
    let num_types = ops::bit_and(&pl::add(&call(it, "Get16u", vec![mp.clone(), n(28)])?, &n(1)), &n(0xffff));

    // validate offsets in the resource map
    if pl::lt(&type_off, &n(28)) || pl::lt(&name_off, &n(30)) {
        return ret(n(0));
    }

    if !et.get("IN_RESOURCE").truthy() {
        pl::method(it, &et, "SetFileType", vec![s("RSRC")])?;
    }
    let verbose = pl::method(it, &et, "Options", vec![s("Verbose")])?;
    let tag_table_ptr = call(it, "GetTagTable", vec![s("Image::ExifTool::RSRC::Main")])?;
    pl::method(it, &et, "VerboseDir", vec![s("RSRC"), num_types.clone()])?;

    // parse resource type list
    let mut i: i64 = 0;
    while pl::lt(&n(i), &num_types) {
        let off = pl::add(&pl::add(&type_off, &n(2)), &n(8 * i)); // offset of entry in type list
        if pl::gt(&pl::add(&off, &n(8)), &map_len) {
            break;
        }
        let res_type = pl::substr(&map, off.to_i64(), Some(4)); // resource type
        let res_num = call(it, "Get16u", vec![mp.clone(), pl::add(&off, &n(4))])?; // number of resources - 1
        let ref_off = pl::add(&call(it, "Get16u", vec![mp.clone(), pl::add(&off, &n(6))])?, &type_off); // offset to first resource reference
        // loop through all resources
        let mut j: i64 = 0;
        while pl::le(&n(j), &res_num) {
            let jj = j;
            j += 1;
            let roff = pl::add(&ref_off, &n(12 * jj));
            if pl::gt(&pl::add(&roff, &n(12)), &map_len) {
                break;
            }
            // read only the 24-bit resource data offset
            let id = call(it, "Get16u", vec![mp.clone(), roff.clone()])?;
            let res_off = pl::add(&ops::bit_and(&call(it, "Get32u", vec![mp.clone(), pl::add(&roff, &n(4))])?, &n(0x00ffffff)), &dat_off);
            let res_name_off = pl::add(&pl::add(&call(it, "Get16u", vec![mp.clone(), pl::add(&roff, &n(2))])?, &name_off), &map_off);
            let tag;
            let (mut val, mut val_len) = (Value::Undef, Value::Undef);
            let mut tag_info = tag_table_ptr.get(res_type.to_bytes());
            if tag_info.truthy() {
                tag = res_type.clone();
            } else {
                tag = pl::sprintf("%s_0x%.4x", &[res_type.clone(), id.clone()]);
                tag_info = tag_table_ptr.get(tag.to_bytes());
            }
            // read the resource data if necessary
            if tag_info.truthy() || verbose.truthy() {
                let mut ok = pl::method(it, &raf, "Seek", vec![res_off.clone(), n(0)])?.truthy() && pl::num_eq(&raf_read(it, &raf, &mut buff, n(4))?, &n(4));
                if ok {
                    val_len = pl::unpack1("N", &buff)?;
                    ok = pl::lt(&val_len, &n(100000000)) // arbitrary size limit (100MB)
                        && pl::num_eq(&raf_read(it, &raf, &mut val, val_len.clone())?, &val_len);
                }
                if !ok {
                    let msg = pl::concat(&pl::concat(&s("Error reading "), &res_type), &s(" resource"));
                    pl::method(it, &et, "Warn", vec![msg])?;
                    continue;
                }
            }
            if verbose.truthy() {
                let mut res_name = Value::Undef;
                let mut ok = pl::method(it, &raf, "Seek", vec![res_name_off, n(0)])?.truthy() && raf_read(it, &raf, &mut buff, n(1))?.truthy();
                if ok {
                    let name_len = ord(&buff);
                    ok = !pl::num_eq(&name_len, &n(0)) && pl::num_eq(&raf_read(it, &raf, &mut res_name, name_len.clone())?, &name_len);
                }
                if !ok {
                    res_name = s("");
                }
                let mut fmt = b"%s resource ID 0x%.4x (offset 0x%.4x, ".to_vec();
                fmt.extend_from_slice(&val_len.to_bytes());
                fmt.extend_from_slice(b" bytes, name='%s'):\n");
                let line = pl::sprintf_v(&Value::bytes(fmt), &[res_type.clone(), id.clone(), res_off.clone(), res_name]);
                pl::method(it, &et, "VPrint", vec![n(0), line])?;
                pl::method(it, &et, "VerboseDump", vec![pl::sref(val.clone())])?;
            }
            if !tag_info.truthy() {
                continue;
            }
            if pl::str_eq(&res_type, "vers") {
                // parse the 'vers' resource to get the long version string
                if !pl::gt(&val_len, &n(8)) {
                    continue;
                }
                // long version string is after short version
                let mut p = pl::add(&n(7), &call(it, "Get8u", vec![pl::sref(val.clone()), n(6)])?);
                if pl::ge(&p, &val_len) {
                    continue;
                }
                let vlen = call(it, "Get8u", vec![pl::sref(val.clone()), p.clone()])?;
                p = pl::add(&p, &n(1));
                if pl::gt(&pl::add(&p, &vlen), &val_len) {
                    continue;
                }
                let _tag_table_ptr = call(it, "GetTagTable", vec![s("Image::ExifTool::RSRC::Main")])?;
                val = pl::method(it, &et, "Decode", vec![pl::substr(&val, p.to_i64(), Some(vlen.to_i64())), s("MacRoman")])?;
            } else if pl::str_eq(&res_type, "sfnt") {
                // parse the OTF font block
                if !pl::method(it, &raf, "Seek", vec![pl::add(&res_off, &n(4)), n(0)])?.truthy() {
                    continue;
                }
                dir_info.set("Base", pl::add(&res_off, &n(4)));
                crate::runtime::require(it, "Image::ExifTool::Font")?;
                if !pl::call(it, "Image::ExifTool::Font::ProcessOTF", vec![et.clone(), dir_info.clone()])?.truthy() {
                    pl::method(it, &et, "Warn", vec![s("Unrecognized sfnt resource format")])?;
                }
                // assume this is a DFONT file unless processing the rsrc fork
                if !et.get("DOC_NUM").truthy() {
                    pl::method(it, &et, "OverrideFileType", vec![s("DFONT")])?;
                }
                continue;
            } else if pl::str_eq(&res_type, "8BIM") {
                let tt_ptr = call(it, "GetTagTable", vec![s("Image::ExifTool::Photoshop::Main")])?;
                pl::method(
                    it,
                    &et,
                    "HandleTag",
                    vec![
                        tt_ptr,
                        id.clone(),
                        val.clone(),
                        s("DataPt"),
                        pl::sref(val.clone()),
                        s("DataPos"),
                        pl::add(&res_off, &n(4)),
                        s("Size"),
                        val_len.clone(),
                        s("Start"),
                        n(0),
                        s("Parent"),
                        s("RSRC"),
                    ],
                )?;
                continue;
            } else if pl::str_eq(&res_type, "STR ") && pl::gt(&val_len, &n(1)) {
                // extract Pascal string
                let len = ord(&val);
                if !pl::ge(&val_len, &pl::add(&len, &n(1))) {
                    continue;
                }
                val = pl::substr(&val, 1, Some(len.to_i64()));
            } else if pl::str_eq(&res_type, "usro") && pl::gt(&val_len, &n(4)) {
                let len = pl::unpack1("N", &val)?;
                if !pl::ge(&val_len, &pl::add(&len, &n(4))) {
                    continue;
                }
                val = pl::substr(&val, 4, Some(len.to_i64()));
                re(r"\0.*", "").subst(&mut val, true, |_| Vec::new()); // truncate at null
            } else if pl::str_eq(&res_type, "STR#") && pl::gt(&val_len, &n(2)) {
                // extract list of strings (ref http://simtech.sourceforge.net/tech/strings.html)
                let num = pl::unpack1("n", &val)?;
                if ops::bit_and(&num, &n(0xf000)).truthy() {
                    continue; // (ignore special-format STR# resources)
                }
                let mut vals: Vec<Value> = Vec::new();
                let mut pos = n(2);
                let mut k: i64 = 0;
                while pl::lt(&n(k), &num) {
                    if pl::ge(&pos, &val_len) {
                        break;
                    }
                    let len = ord(&pl::substr(&val, pos.to_i64(), Some(1)));
                    pos = pl::add(&pos, &n(1));
                    if pl::gt(&pl::add(&pos, &len), &val_len) {
                        break;
                    }
                    vals.push(pl::substr(&val, pos.to_i64(), Some(len.to_i64())));
                    pos = pl::add(&pos, &len);
                    k += 1;
                }
                val = Value::new_array(vals);
            } else if pl::str_eq(&res_type, "POST") {
                // assume this is a DFONT file unless processing the rsrc fork
                if !et.get("DOC_NUM").truthy() {
                    pl::method(it, &et, "OverrideFileType", vec![s("DFONT")])?;
                }
                val = pl::substr(&val, 2, None);
            } else if !pl::str_eq(&res_type, "TEXT") {
                continue;
            }
            pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag, val])?;
        }
        i += 1;
    }
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::RSRC::ProcessRSRC", process_rsrc);
}
