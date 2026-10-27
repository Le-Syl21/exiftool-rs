//! Image::ExifTool::WritePhotoshop (Image/ExifTool/WritePhotoshop.pl), ported sub by sub (see docs/port/PORTING.md).
//!
//! Write Photoshop IRB meta information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::write_exif::{band, cat, chr, etf, g16u, g32u, g8u, inc, keys, len, s16u, s32u, seq, set_byte_order, sort_num, substr, viv_array};
use crate::perl::Value;
use crate::pl::{self, arg, cap, re, ret};

fn n(i: i64) -> Value {
    Value::Int(i)
}

/// SetResourceName with the name as a variable the caller sees changed.
/// Returns whether the name was set (`$_[1] = $name`).
fn set_resource_name_mut(it: &mut Interp, tag_info: &Value, name: &mut Value, val_pt: &Value) -> ER<bool> {
    let set_name = tag_info.get("SetResourceName");
    if !set_name.is_undef() {
        let mut nm;
        // extract resource name from value
        if let Some(c) = re(r".*/#(.{0,255})#/$", "s").m(&val_pt.deref()) {
            nm = cap(&c, 1);
            // strip name from value
            let v = substr(it, &val_pt.deref(), &n(0), Some(&pl::sub(&n(-4), &len(&nm))))?;
            pl::set_deref(val_pt, v)?;
        } else if pl::str_eq(&set_name, "1") {
            return Ok(false); // use old name
        } else {
            nm = set_name;
        }
        // convert to padded pascal string
        nm = cat(&[chr(&len(&nm)), nm.clone()]);
        if band(&len(&nm), 0x01).truthy() {
            nm = cat(&[nm, pl::s("\0")]);
        }
        *name = nm; // return new name
        return Ok(true);
    }
    Ok(false)
}

/// Strip resource name from value prepare resource name for writing into IRB
/// Inputs: 0) tagInfo ref, 1) resource name (padded pascal string), 2) new value ref
/// Returns: none (updates name and value if necessary)
pub fn set_resource_name(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (tag_info, mut name, val_pt) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if set_resource_name_mut(it, &tag_info, &mut name, &val_pt)? {
        it.set_arg(1, name)?;
    }
    pl::ret_nothing(want)
}

/// Write Photoshop IRB resource
/// Inputs: 0) ExifTool object reference, 1) source dirInfo reference,
///         2) tag table reference
/// Returns: IRB resource data (may be empty if no Photoshop data)
/// Notes: Increments ExifTool CHANGED flag for each tag changed
pub fn write_photoshop(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et.truthy() {
        return ret(n(1)); // allow dummy access to autoload this package
    }
    let mut data_pt = dir_info.get("DataPt");
    if !data_pt.truthy() {
        data_pt = pl::sref(pl::s(""));
    }
    let start = dir_info.get("DirStart");
    let start = if start.truthy() { start } else { n(0) };
    let dir_len = dir_info.get("DirLen");
    let dir_len = if dir_len.truthy() { dir_len } else { pl::sub(&len(&data_pt.deref()), &start) };
    let dir_end = pl::add(&start, &dir_len);
    let mut new_data = pl::s("");

    // make a hash of new tag info, keyed on tagID
    let new_tags = pl::method(it, &et, "GetNewTagInfoHash", vec![tag_table_ptr.clone()])?;

    let ad = pl::method_list(it, &et, "GetAddDirHash", vec![tag_table_ptr.clone()])?;
    let (add_dirs, edit_dirs) = (arg(&ad, 0), arg(&ad, 1));

    set_byte_order(it, pl::s("MM"))?; // Photoshop is always big-endian
    let iptc_digest_info = it.get_scalar("Image::ExifTool::Photoshop::iptcDigestInfo");
    //
    // rewrite existing tags in the old directory, deleting ones as necessary
    // (the Photoshop directory entries aren't in any particular order)
    //
    // Format: 0) Type, 4 bytes - '8BIM' (or the rare 'PHUT', 'DCSR', 'AgHg' or 'MeSa')
    //         1) TagID,2 bytes
    //         2) Name, pascal string padded to even no. bytes
    //         3) Size, 4 bytes - N
    //         4) Data, N bytes
    let (mut value, mut size, mut tag_info);
    let mut pos = start.clone();
    size = Value::Undef;
    tag_info = Value::Undef;
    while pl::lt(&pl::add(&pos, &n(8)), &dir_end) {
        // each entry must be on same even byte boundary as directory start
        if band(&crate::perl::ops::bit_xor(&pos, &start), 0x01).truthy() {
            pos = pl::add(&pos, &n(1));
        }
        let typ = substr(it, &data_pt.deref(), &pos, Some(&n(4)))?;
        if !re(r"^(8BIM|PHUT|DCSR|AgHg|MeSa)$", "").is_match(&typ) {
            pl::method(it, &et, "Error", vec![pl::s("Bad Photoshop IRB resource")])?;
            new_data = Value::Undef;
            break;
        }
        let tag_id = g16u(it, &data_pt, pl::add(&pos, &n(4)))?;
        // get resource block name (pascal string padded to an even # of bytes)
        let mut namelen = pl::add(&n(1), &g8u(it, &data_pt, pl::add(&pos, &n(6)))?);
        if band(&namelen, 0x01).truthy() {
            namelen = pl::add(&namelen, &n(1));
        }
        if pl::gt(&pl::add(&pl::add(&pos, &namelen), &n(10)), &dir_end) {
            pl::method(it, &et, "Error", vec![pl::s("Bad APP13 resource block")])?;
            new_data = Value::Undef;
            break;
        }
        let mut name = substr(it, &data_pt.deref(), &pl::add(&pos, &n(6)), Some(&namelen))?;
        size = g32u(it, &data_pt, pl::add(&pl::add(&pos, &n(6)), &namelen))?;
        pos = pl::add(&pos, &pl::add(&namelen, &n(10)));
        if pl::gt(&pl::add(&size, &pos), &dir_end) {
            pl::method(it, &et, "Error", vec![pl::concat(&pl::s("Bad APP13 resource data size "), &size)])?;
            new_data = Value::Undef;
            break;
        }
        let tid = tag_id.to_bytes();
        if new_tags.get(&tid).truthy() && typ.eq_str("8BIM") {
            tag_info = new_tags.get(&tid);
            new_tags.delete(&tid);
            let nv_hash = pl::method(it, &et, "GetNewValueHash", vec![tag_info.clone()])?;
            // check to see if we are overwriting this tag
            value = substr(it, &data_pt.deref(), &pos, Some(&size))?;
            let mut is_overwriting = pl::method(it, &et, "IsOverwriting", vec![nv_hash.clone(), value.clone()])?;
            // handle special 'new' and 'old' values for IPTCDigest
            if !is_overwriting.truthy() && seq(&tag_info, &iptc_digest_info) {
                let del = viv_array(&nv_hash, "DelValue").items();
                if del.iter().any(|v| re(r"^new$", "").is_match(v)) {
                    let nd = et.get("NewIPTCDigest");
                    if nd.truthy() && seq(&nd, &value) {
                        is_overwriting = n(1);
                    }
                }
                let del = viv_array(&nv_hash, "DelValue").items();
                if del.iter().any(|v| re(r"^old$", "").is_match(v)) {
                    let od = et.get("OldIPTCDigest");
                    if od.truthy() && seq(&od, &value) {
                        is_overwriting = n(1);
                    }
                }
            }
            if is_overwriting.truthy() {
                let nm = pl::concat(&pl::s("- Photoshop:"), &tag_info.get("Name"));
                pl::method(it, &et, "VerboseValue", vec![nm, value.clone()])?;
                // handle IPTCDigest specially because we want to write it last
                // so the new IPTC digest will be known
                if seq(&tag_info, &iptc_digest_info) {
                    new_tags.set(&tid, tag_info.clone()); // add later
                    value = Value::Undef;
                } else {
                    value = pl::method(it, &et, "GetNewValue", vec![nv_hash.clone()])?;
                }
                inc(&et, "CHANGED");
                if value.is_undef() {
                    pos = pl::add(&pos, &size);
                    continue; // next if tag is being deleted
                }
                // set resource name if necessary
                let vr = pl::sref(value.clone());
                set_resource_name_mut(it, &tag_info, &mut name, &vr)?;
                value = vr.deref();
                let nm = pl::concat(&pl::s("+ Photoshop:"), &tag_info.get("Name"));
                pl::method(it, &et, "VerboseValue", vec![nm, value.clone()])?;
            }
        } else {
            if typ.eq_str("8BIM") {
                tag_info = edit_dirs.get(&tid);
                if !tag_info.truthy() {
                    // process subdirectory anyway if writable (except EXIF to avoid recursion)
                    // --> this allows IPTC to be processed if found here in TIFF images
                    // (but allow EXIF to be written in PSD files if XMP or IPTC tags are
                    //  being written because I have seen cases of XMP in PSD-EXIFInfo-IFD0
                    //  and IPTC in PSD-EXIFInfo-IFD0-IPTC, see forum10768 and forum13198)
                    let tmp_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag_id.clone()])?;
                    if tmp_info.truthy()
                        && tmp_info.get("SubDirectory").truthy()
                        && (!tmp_info.get("SubDirectory").get("TagTable").eq_str("Image::ExifTool::Exif::Main")
                            || (et.get("FILE_TYPE").eq_str("PSD") && (edit_dirs.get("1028").truthy() || edit_dirs.get("1060").truthy())))
                    {
                        let table = etf(it, "GetTagTable", vec![tmp_info.get("SubDirectory").get("TagTable")])?;
                        if table.get("WRITE_PROC").truthy() {
                            tag_info = tmp_info;
                        }
                    }
                }
            }
            if tag_info.truthy() {
                if add_dirs.get(&tid).truthy() {
                    add_dirs.delete(&tid);
                }
                let subdir_info = Value::new_hash();
                subdir_info.set("DataPt", data_pt.clone());
                subdir_info.set("DirStart", pos.clone());
                subdir_info.set("DataLen", dir_len.clone());
                subdir_info.set("DirLen", size.clone());
                subdir_info.set("Parent", dir_info.get("DirName"));
                let sub_table = etf(it, "GetTagTable", vec![tag_info.get("SubDirectory").get("TagTable")])?;
                let write_proc = tag_info.get("SubDirectory").get("WriteProc");
                let new_value = pl::method(it, &et, "WriteDirectory", vec![subdir_info, sub_table, write_proc])?;
                if !new_value.is_undef() {
                    if len(&new_value).to_i64() == 0 {
                        pos = pl::add(&pos, &size);
                        continue; // remove subdirectory entry
                    }
                    let vr = pl::sref(new_value);
                    set_resource_name_mut(it, &tag_info, &mut name, &vr)?;
                    value = vr.deref();
                } else {
                    value = substr(it, &data_pt.deref(), &pos, Some(&size))?; // rewrite old directory
                }
            } else {
                value = substr(it, &data_pt.deref(), &pos, Some(&size))?;
            }
        }
        let new_size = len(&value);
        // write this directory entry
        let s16 = s16u(it, tag_id.clone())?;
        let s32 = s32u(it, new_size.clone())?;
        new_data = cat(&[new_data, typ, s16, name, s32, value]);
        if band(&new_size, 0x01).truthy() {
            new_data = pl::concat(&new_data, &pl::s("\0")); // must null pad to even byte
        }
        pos = pl::add(&pos, &size);
    }
    //
    // write any remaining entries we didn't find in the old directory
    // (might as well write them in numerical tag order)
    //
    let mut tags_left = keys(&new_tags);
    tags_left.extend(keys(&add_dirs));
    sort_num(&mut tags_left);
    for tag_id in tags_left {
        let tid = tag_id.to_bytes();
        let mut name = pl::s("\0\0");
        if new_tags.get(&tid).truthy() {
            tag_info = new_tags.get(&tid);
            let nv_hash = pl::method(it, &et, "GetNewValueHash", vec![tag_info.clone()])?;
            value = pl::method(it, &et, "GetNewValue", vec![nv_hash.clone()])?;
            // handle new IPTCDigest value specially
            if seq(&tag_info, &iptc_digest_info) && !value.is_undef() {
                if value.eq_str("new") {
                    value = et.get("NewIPTCDigest");
                } else if value.eq_str("old") {
                    value = et.get("OldIPTCDigest");
                }
                // (we already know we want to create this tag)
            } else {
                // don't add this tag unless specified
                if !nv_hash.get("IsCreating").truthy() {
                    continue;
                }
            }
            if value.is_undef() {
                continue; // next if tag is being deleted
            }
            let nm = pl::concat(&pl::s("+ Photoshop:"), &tag_info.get("Name"));
            pl::method(it, &et, "VerboseValue", vec![nm, value.clone()])?;
            inc(&et, "CHANGED");
        } else {
            tag_info = add_dirs.get(&tid);
            // create new directory
            let subdir_info = Value::new_hash();
            subdir_info.set("Parent", dir_info.get("DirName"));
            let sub_table = etf(it, "GetTagTable", vec![tag_info.get("SubDirectory").get("TagTable")])?;
            let write_proc = tag_info.get("SubDirectory").get("WriteProc");
            value = pl::method(it, &et, "WriteDirectory", vec![subdir_info, sub_table, write_proc])?;
            if !value.truthy() {
                continue;
            }
        }
        // set resource name if necessary
        let vr = pl::sref(value.clone());
        set_resource_name_mut(it, &tag_info, &mut name, &vr)?;
        value = vr.deref();
        size = len(&value);
        // write the new directory entry
        let s16 = s16u(it, tag_id.clone())?;
        let s32 = s32u(it, size.clone())?;
        new_data = cat(&[new_data, pl::s("8BIM"), s16, name, s32, value.clone()]);
        if band(&size, 0x01).truthy() {
            new_data = pl::concat(&new_data, &pl::s("\0")); // must null pad to even numbered byte
        }
        inc(&et, "CHANGED");
    }
    let _ = size;
    ret(new_data)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Photoshop::SetResourceName", set_resource_name);
    it.define_native("Image::ExifTool::Photoshop::WritePhotoshop", write_photoshop);
}
