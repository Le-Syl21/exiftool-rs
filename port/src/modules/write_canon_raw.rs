//! Image::ExifTool::WriteCanonRaw (Image/ExifTool/WriteCanonRaw.pl), ported sub by sub (see docs/port/PORTING.md).
//!
//! Write Canon RAW (CRW and CR2) meta information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::write_exif::{band, byte_order, cat, cell, etf, eval_at, g16u, g32u, get, inc, keys, len, opt, print_out, put, read_eq, read_value, rep, rf, s16u, s32u, s32u_at, seek, set_byte_order, sort_num, substr, tell, wlex_hash, write, write_value};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, re, ret};

const MODULE: &str = "Image::ExifTool::CanonRaw";
const FILE: &str = "WriteCanonRaw.pl";

fn n(i: i64) -> Value {
    Value::Int(i)
}

/// Initialize buffers for building MakerNotes from RAW data
/// Inputs: 0) ExifTool object reference
pub fn init_maker_notes(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let et = arg(&a, 0);
    let h = Value::new_hash();
    h.set("Entries", Value::new_hash()); // directory entries keyed by tagID
    h.set("ValBuff", pl::s("\0\0\0\0")); // value data buffer (start with zero nextIFD pointer)
    h.set("FixupTags", Value::new_hash()); // flags for tags with data in value buffer
    et.set("MAKER_NOTE_INFO", h.clone());
    ret(h)
}

/// Build maker notes from CanonRaw information
/// Inputs: 0) ExifTool object reference, 1) raw tag ID, 2) reference to tagInfo
///         3) reference to value, 4) format name, 5) count
/// Notes: This will build the directory in the order the tags are found in the CRW
/// file, which isn't sequential (but Canon's version isn't sequential either...)
pub fn build_maker_notes(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, raw_tag, tag_info, value_pt, form_name, mut count) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4), arg(&a, 5));

    let map_raw_tag = wlex_hash(it, MODULE, FILE, "mapRawTag");
    let tag_id = map_raw_tag.get(raw_tag.to_bytes());
    if !tag_id.truthy() {
        return pl::ret_nothing(want);
    }
    if !form_name.truthy() {
        pl::warn(it, &pl::sprintf("No format for tag 0x%x!\n", &[raw_tag]).to_text())?;
        return pl::ret_nothing(want);
    }
    // special case: ignore user comment because it gets saved in EXIF
    // (and has the same raw tagID as CanonFileDescription)
    if tag_info.truthy() && tag_info.get("Name").eq_str("UserComment") {
        return pl::ret_nothing(want);
    }
    let format = pl::pkg_hash(it, "Image::ExifTool::Exif::formatNumber").get(form_name.to_bytes());
    let fsiz = pl::pkg_array(it, "Image::ExifTool::Exif::formatSize").at(format.to_i64());
    let mut size = len(&value_pt.deref());
    let value;
    if count.truthy() && !pl::num_eq(&size, &pl::mul(&count, &fsiz)) {
        if pl::lt(&size, &pl::mul(&count, &fsiz)) {
            pl::warn(it, &pl::sprintf("Value too short for raw tag 0x%x\n", &[raw_tag]).to_text())?;
            return pl::ret_nothing(want);
        }
        // shorten value appropriately
        size = pl::mul(&count, &fsiz);
        value = substr(it, &value_pt.deref(), &n(0), Some(&size))?;
    } else {
        count = pl::div(&size, &fsiz)?;
        value = value_pt.deref();
    }
    let mut offset_val;
    let maker_info = et.get("MAKER_NOTE_INFO");
    if pl::gt(&size, &n(4)) {
        let l = len(&maker_info.get("ValBuff"));
        offset_val = s32u(it, l)?;
        maker_info.set("ValBuff", pl::concat(&maker_info.get("ValBuff"), &value));
        // pad to an even number of bytes
        if band(&size, 0x01).truthy() {
            maker_info.set("ValBuff", pl::concat(&maker_info.get("ValBuff"), &pl::s("\0")));
        }
        // set flag indicating that this tag needs a fixup
        maker_info.get_or_new_hash("FixupTags").set(tag_id.to_bytes(), n(1));
    } else {
        offset_val = value;
        if pl::lt(&size, &n(4)) {
            offset_val = pl::concat(&offset_val, &rep(b"\0", &pl::sub(&n(4), &size)));
        }
    }
    let e = cat(&[s16u(it, tag_id.clone())?, s16u(it, format)?, s32u(it, count)?, offset_val]);
    maker_info.get_or_new_hash("Entries").set(tag_id.to_bytes(), e.clone());
    ret(e)
}

/// Finish building and save MakerNotes
/// Inputs: 0) ExifTool object reference
pub fn save_maker_notes(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let et = arg(&a, 0);
    // save maker notes
    let maker_info = et.get("MAKER_NOTE_INFO");
    et.delete("MAKER_NOTE_INFO");
    let dir_entries = maker_info.get("Entries");
    let num_entries = dir_entries.len();
    let fixup = pl::method(it, &pl::s("Image::ExifTool::Fixup"), "new", vec![])?;
    if num_entries == 0 {
        return pl::ret_nothing(want);
    }
    // build the MakerNotes directory
    let mut maker_notes = crate::modules::write_exif::s16u(it, n(num_entries as i64))?;
    // write the entries in proper tag order (even though Canon doesn't do this...)
    let mut ids = keys(&dir_entries);
    sort_num(&mut ids);
    for tag_id in ids {
        maker_notes = pl::concat(&maker_notes, &dir_entries.get(tag_id.to_bytes()));
        if !maker_info.get("FixupTags").get(tag_id.to_bytes()).truthy() {
            continue;
        }
        // add fixup for this pointer
        pl::method(it, &fixup, "AddFixup", vec![pl::sub(&len(&maker_notes), &n(4))])?;
    }
    // save position of maker notes for pointer fixups
    fixup.set("Shift", pl::add(&fixup.get("Shift"), &len(&maker_notes)));
    let bo = byte_order(it)?;
    et.set("MAKER_NOTE_BYTE_ORDER", bo);
    // add value data
    maker_notes = pl::concat(&maker_notes, &maker_info.get("ValBuff"));
    // get MakerNotes tag info
    let tag_table_ptr = etf(it, "GetTagTable", vec![pl::s("Image::ExifTool::Exif::Main")])?;
    let mn = cell(maker_notes.clone());
    let mut tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), n(0x927c), rf(&mn)])?;
    // save the MakerNotes
    let key = pl::method(it, &et, "FoundTag", vec![tag_info.clone(), get(&mn)])?;
    et.get_or_new_hash("TAG_EXTRA").get_or_new_hash(key.to_bytes()).set("Fixup", fixup);
    // save the garbage collection some work later
    maker_info.delete("Entries");
    maker_info.delete("ValBuff");
    maker_info.delete("FixupTags");
    // also generate Orientation tag since Rotation isn't transferred from RAW info
    let rotation = pl::method(it, &et, "GetValue", vec![pl::s("Rotation"), pl::s("ValueConv")])?;
    let map_rotation = wlex_hash(it, MODULE, FILE, "mapRotation");
    if !rotation.is_undef() && !map_rotation.get(rotation.to_bytes()).is_undef() {
        tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr, n(0x112)])?;
        let r = pl::method(it, &et, "FoundTag", vec![tag_info, map_rotation.get(rotation.to_bytes())])?;
        return ret(r);
    }
    pl::ret_nothing(want)
}

/// Check CanonRaw information
/// Inputs: 0) ExifTool object reference, 1) tagInfo hash reference,
///         2) raw value reference
/// Returns: error string or undef (and may change value) on success
pub fn check_canon_raw(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, tag_info, val_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let tag_name = tag_info.get("Name");
    if tag_name.eq_str("JpgFromRaw") || tag_name.eq_str("ThumbnailImage") {
        if !(re(r"^\xff\xd8", "").is_match(&val_ptr.deref()) || opt(it, &et, "IgnoreMinorErrors")?.truthy()) {
            return ret(pl::s("[Minor] Not a valid image"));
        }
    } else {
        let mut format = tag_info.get("Format");
        let count = tag_info.get("Count");
        if !format.truthy() {
            let tag_type = ops::bit_and(&ops::shift_right(&tag_info.get("TagID"), &n(8)), &n(0x38));
            format = pl::pkg_hash(it, "Image::ExifTool::CanonRaw::crwTagFormat").get(tag_type.to_bytes());
        }
        if format.truthy() {
            return ret(etf(it, "CheckValue", vec![val_ptr, format, count])?);
        }
    }
    pl::ret_undef()
}

/// Write CR2 file
/// Inputs: 0) ExifTool ref, 1) dirInfo reference (must have read first 16 bytes)
///         2) tag table reference
/// Returns: true on success
pub fn write_cr2(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    if !data_pt.truthy() {
        return ret(n(0));
    }
    let outfile = dir_info.get("OutFile");
    if !outfile.truthy() {
        return ret(n(0));
    }
    if !dir_info.get("RAF").truthy() {
        return ret(n(0));
    }

    // check CR2 signature
    if !re(r"^.{8}CR\x02\0", "s").is_match(&data_pt.deref()) {
        let (msg, mut minor) = (pl::s(""), Value::Undef);
        let msg = if re(r"^.{8}CR", "s").is_match(&data_pt.deref()) {
            minor = n(1);
            pl::s("Unsupported Canon RAW file. May cause problems if rewritten")
        } else if re(r"^.{8}\xba\xb0\xac\xbb", "s").is_match(&data_pt.deref()) {
            pl::s("Can not currently write Canon 1D RAW images")
        } else {
            let _ = msg;
            pl::s("Unrecognized Canon RAW file")
        };
        if pl::method(it, &et, "Error", vec![msg, minor])?.truthy() {
            return ret(n(0));
        }
    }

    // CR2 has a 16-byte header
    dir_info.set("NewDataPos", n(16));
    let new_data = pl::method(it, &et, "WriteDirectory", vec![dir_info.clone(), tag_table_ptr])?;
    if new_data.is_undef() {
        return ret(n(0));
    }
    if !dir_info.get("LastIFD").truthy() {
        pl::method(it, &et, "Error", vec![pl::s("CR2 image IFD may not be deleted")])?;
        return ret(n(0));
    }

    if len(&new_data).truthy() {
        // build 16 byte header for Canon RAW file
        let header = cell(substr(it, &data_pt.deref(), &n(0), Some(&n(16)))?);
        // set IFD0 pointer (may not be 16 if edited by PhotoMechanic)
        s32u_at(it, n(16), &rf(&header), n(4))?;
        // last 4 bytes of header is pointer to last IFD
        s32u_at(it, dir_info.get("LastIFD"), &rf(&header), n(12))?;
        if !write(it, &outfile, vec![get(&header), new_data])? {
            return ret(n(0));
        }
        // (undef $newData: free memory)

        // copy over image data now if necessary
        if dir_info.get("ImageData").is_ref() {
            if !pl::method(it, &et, "CopyImageData", vec![dir_info.get("ImageData"), outfile.clone()])?.truthy() {
                return ret(n(0));
            }
            dir_info.delete("ImageData");
        }
    }
    ret(n(1))
}

/// Write CanonRaw (CRW) information
/// Inputs: 0) ExifTool object reference, 1) source dirInfo reference,
///         2) tag table reference
/// Returns: true on success
/// Notes: Increments ExifTool CHANGED flag for each tag changed This routine is
/// different from all of the other write routines because Canon RAW files are
/// designed well!  So it isn't necessary to buffer the data in memory before
/// writing it out.  Therefore this routine doesn't return the directory data as
/// the rest of the Write routines do.  Instead, it writes to the dirInfo
/// OutFile on the fly --> much faster, efficient, and less demanding on memory!
pub fn write_canon_raw(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !et.truthy() {
        return ret(n(1)); // allow dummy access to autoload this package
    }
    let block_start = dir_info.get("DirStart");
    let block_size = dir_info.get("DirLen");
    let raf = dir_info.get("RAF");
    if !raf.truthy() {
        return ret(n(0));
    }
    let outfile = dir_info.get("OutFile");
    if !outfile.truthy() {
        return ret(n(0));
    }
    let mut out_pos = dir_info.get("OutPos");
    if !out_pos.truthy() {
        return ret(n(0));
    }
    let out_base = out_pos.clone();
    let verbose = opt(it, &et, "Verbose")?;
    let out = opt(it, &et, "TextOut")?;
    let buff = cell(Value::Undef);
    let mut tag_info;

    // 4 bytes at end of block give directory position within block
    if !seek(it, &raf, pl::sub(&pl::add(&block_start, &block_size), &n(4)), 0)? {
        return ret(n(0));
    }
    if !read_eq(it, &raf, &buff, &n(4))? {
        return ret(n(0));
    }
    let dir_offset = pl::add(&g32u(it, &rf(&buff), n(0))?, &block_start);
    // avoid infinite recursion
    if !et.get("ProcessedCanonRaw").truthy() {
        et.set("ProcessedCanonRaw", Value::new_hash());
    }
    if et.get("ProcessedCanonRaw").get(dir_offset.to_bytes()).truthy() {
        pl::method(it, &et, "Error", vec![pl::concat(&pl::s("Double-referenced "), &pl::concat(&dir_info.get("DirName"), &pl::s(" directory")))])?;
        return ret(n(0));
    }
    et.get("ProcessedCanonRaw").set(dir_offset.to_bytes(), n(1));
    if !seek(it, &raf, dir_offset, 0)? {
        return ret(n(0));
    }
    if !read_eq(it, &raf, &buff, &n(2))? {
        return ret(n(0));
    }
    let mut entries = g16u(it, &rf(&buff), n(0))?; // get number of entries in directory
    // read the directory (10 bytes per entry)
    if !read_eq(it, &raf, &buff, &pl::mul(&n(10), &entries))? {
        return ret(n(0));
    }
    let mut new_dir = pl::s("");

    // get hash of new information keyed by tagID
    let new_tags = pl::method(it, &et, "GetNewTagInfoHash", vec![tag_table_ptr.clone()])?;

    // generate list of tags to add or delete (currently, we only allow JpgFromRaw
    // and ThumbnailImage, to be added or deleted from the root CanonRaw directory)
    let mut add_tags: Vec<Value> = Vec::new();
    let del_tag = Value::new_hash();
    if pl::num_eq(&dir_info.get("Nesting"), &n(0)) {
        for tag_id in keys(&new_tags) {
            let permanent = new_tags.get(tag_id.to_bytes()).get("Permanent");
            if !permanent.is_undef() && !permanent.truthy() {
                add_tags.push(tag_id);
            }
        }
    }

    tag_info = Value::Undef;
    let mut index = n(0);
    loop {
        let (mut pt, mut tag, mut size, mut value_ptr, mut ptr) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef);
        let value = cell(Value::Undef);
        if pl::lt(&index, &entries) {
            pt = pl::mul(&n(10), &index);
            tag = g16u(it, &rf(&buff), pt.clone())?;
            size = g32u(it, &rf(&buff), pl::add(&pt, &n(2)))?;
            value_ptr = g32u(it, &rf(&buff), pl::add(&pt, &n(6)))?;
            ptr = pl::add(&value_ptr, &block_start); // all pointers relative to block start
        }
        // add any required new tags
        // NOTE: can't currently add tags where value is stored in directory
        if !add_tags.is_empty() && (tag.is_undef() || pl::ge(&tag, &add_tags[0])) {
            let add_tag = add_tags.remove(0);
            tag_info = new_tags.get(add_tag.to_bytes());
            let mut new_val = pl::method(it, &et, "GetNewValue", vec![tag_info.clone()])?;
            if !new_val.is_undef() {
                // pad value to an even length (Canon ImageBrowser and ZoomBrowser
                // version 6.1.1 have problems with odd-sized embedded JPEG images
                // even if the value is padded to maintain alignment, so do this
                // before calculating the size for the directory entry)
                if band(&len(&new_val), 0x01).truthy() {
                    new_val = pl::concat(&new_val, &pl::s("\0"));
                }
                // add new directory entry
                let e = cat(&[s16u(it, add_tag.clone())?, s32u(it, len(&new_val))?, s32u(it, pl::sub(&out_pos, &out_base))?]);
                new_dir = pl::concat(&new_dir, &e);
                // write new value data
                if !write(it, &outfile, vec![new_val.clone()])? {
                    return ret(n(0));
                }
                out_pos = pl::add(&out_pos, &len(&new_val)); // update current position
                if pl::gt(&verbose, &n(1)) {
                    print_out(it, &out, cat(&[pl::s("    + CanonRaw:"), tag_info.get("Name"), pl::s("\n")]))?;
                }
                inc(&et, "CHANGED");
            }
            // set flag to delete this tag if found later
            del_tag.set(add_tag.to_bytes(), n(1));
        }
        if tag.is_undef() {
            break; // all done if no more directory entries
        }
        if band(&tag, 0x8000).truthy() {
            return ret(n(0)); // top bit should not be set
        }
        let tag_id = band(&tag, 0x3fff); // get tag ID
        let tag_type = band(&ops::shift_right(&tag, &n(8)), 0x38); // get tag type
        let value_in_dir = band(&tag, 0x4000); // flag for value in directory

        let tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag_id.clone()])?;
        let mut format = pl::pkg_hash(it, "Image::ExifTool::CanonRaw::crwTagFormat").get(tag_type.to_bytes());
        let (mut count, mut subdir) = (Value::Undef, Value::Undef);
        if tag_info.truthy() {
            subdir = tag_info.get("SubDirectory");
            if tag_info.get("Format").truthy() {
                format = tag_info.get("Format");
            }
            count = tag_info.get("Count");
        }
        if value_in_dir.truthy() {
            size = n(8);
            put(&value, substr(it, &get(&buff), &pl::add(&pt, &n(2)), Some(&size))?);
            // set count to 1 by default for normal values in directory
            if count.is_undef() && format.truthy() && !format.eq_str("string") && !subdir.truthy() {
                count = n(1);
            }
        } else if pl::num_eq(&tag_type, &n(0x28)) || pl::num_eq(&tag_type, &n(0x30)) {
            // this type of tag specifies a raw subdirectory
            let mut name = Value::Undef;
            if tag_info.truthy() {
                name = tag_info.get("Name");
            }
            if !name.truthy() {
                name = pl::sprintf("CanonRaw_0x%.4x", &[tag_id.clone()]);
            }
            let subdir_info = Value::new_hash();
            subdir_info.set("DirName", name);
            subdir_info.set("DataLen", n(0));
            subdir_info.set("DirStart", ptr.clone());
            subdir_info.set("DirLen", size.clone());
            subdir_info.set("Nesting", pl::add(&dir_info.get("Nesting"), &n(1)));
            subdir_info.set("RAF", raf.clone());
            subdir_info.set("Parent", dir_info.get("DirName"));
            subdir_info.set("OutFile", outfile.clone());
            subdir_info.set("OutPos", out_pos.clone());
            let result = pl::method(it, &et, "WriteDirectory", vec![subdir_info.clone(), tag_table_ptr.clone()])?;
            if !result.truthy() {
                return ret(n(0));
            }
            // set size and pointer for this new directory
            size = pl::sub(&subdir_info.get("OutPos"), &out_pos);
            value_ptr = pl::sub(&out_pos, &out_base);
            out_pos = subdir_info.get("OutPos");
        } else {
            // verify that the value data is within this block
            if !pl::le(&pl::add(&value_ptr, &size), &block_size) {
                return ret(n(0));
            }
            // read value from file
            if !seek(it, &raf, ptr.clone(), 0)? {
                return ret(n(0));
            }
            if !read_eq(it, &raf, &value, &size)? {
                return ret(n(0));
            }
        }
        // set count from tagInfo count if necessary
        if format.truthy() && !count.truthy() {
            // set count according to format and size
            let fnum = pl::pkg_hash(it, "Image::ExifTool::Exif::formatNumber").get(format.to_bytes());
            let fsiz = pl::pkg_array(it, "Image::ExifTool::Exif::formatSize").at(fnum.to_i64());
            count = pl::int(&pl::div(&size, &fsiz)?);
        }
        // edit subdirectory if necessary
        if tag_info.truthy() {
            if subdir.truthy() && subdir.get("TagTable").truthy() {
                let name = tag_info.get("Name");
                let new_tag_table = etf(it, "GetTagTable", vec![subdir.get("TagTable")])?;
                if !new_tag_table.truthy() {
                    return ret(n(0));
                }
                let mut subdir_start = n(0);
                //### eval Start ()
                if subdir.get("Start").truthy() {
                    subdir_start = eval_at(it, &subdir.get("Start"), MODULE, FILE, &[], Want::Scalar).0;
                }
                let dir_data = rf(&value);
                let subdir_info = Value::new_hash();
                subdir_info.set("Name", name.clone());
                subdir_info.set("DataPt", dir_data.clone());
                subdir_info.set("DataLen", size.clone());
                subdir_info.set("DirStart", subdir_start.clone());
                subdir_info.set("DirLen", pl::sub(&size, &subdir_start));
                subdir_info.set("Nesting", pl::add(&dir_info.get("Nesting"), &n(1)));
                subdir_info.set("RAF", raf.clone());
                subdir_info.set("Parent", dir_info.get("DirName"));
                //### eval Validate ($dirData, $subdirStart, $size)
                let invalid = !subdir.get("Validate").is_undef() && {
                    let vars = [("dirData", dir_data), ("subdirStart", subdir_start.clone()), ("size", size.clone())];
                    !eval_at(it, &subdir.get("Validate"), MODULE, FILE, &vars, Want::Scalar).0.truthy()
                };
                if invalid {
                    pl::method(it, &et, "Warn", vec![cat(&[pl::s("Invalid "), name, pl::s(" data")])])?;
                } else {
                    subdir = pl::method(it, &et, "WriteDirectory", vec![subdir_info, new_tag_table])?;
                    if !subdir.is_undef() && len(&subdir).truthy() {
                        if subdir_start.truthy() {
                            // add header before data directory
                            let h = substr(it, &get(&value), &n(0), Some(&subdir_start))?;
                            put(&value, pl::concat(&h, &subdir));
                        } else {
                            put(&value, subdir.clone());
                        }
                    }
                }
            } else if new_tags.get(tag_id.to_bytes()).truthy() {
                if del_tag.get(tag_id.to_bytes()).truthy() {
                    if pl::gt(&verbose, &n(1)) {
                        print_out(it, &out, cat(&[pl::s("    - CanonRaw:"), tag_info.get("Name"), pl::s("\n")]))?;
                    }
                    inc(&et, "CHANGED");
                    index = pl::add(&index, &n(1));
                    continue; // next since we already added this tag
                }
                let old_val = if format.truthy() { read_value(it, vec![rf(&value), n(0), format.clone(), count.clone(), size.clone()])? } else { get(&value) };
                let nv_hash = pl::method(it, &et, "GetNewValueHash", vec![tag_info.clone()])?;
                if pl::method(it, &et, "IsOverwriting", vec![nv_hash.clone(), old_val.clone()])?.truthy() {
                    let mut new_val = pl::method(it, &et, "GetNewValue", vec![nv_hash])?;
                    let mut verbose_val = Value::Undef;
                    if pl::gt(&verbose, &n(1)) {
                        verbose_val = new_val.clone();
                    }
                    // convert to specified format if necessary
                    if !new_val.is_undef() && format.truthy() {
                        new_val = write_value(it, vec![new_val, format.clone(), count.clone()])?;
                    }
                    if !new_val.is_undef() {
                        put(&value, new_val);
                        inc(&et, "CHANGED");
                        pl::method(it, &et, "VerboseValue", vec![pl::concat(&pl::s("- CanonRaw:"), &tag_info.get("Name")), old_val])?;
                        pl::method(it, &et, "VerboseValue", vec![pl::concat(&pl::s("+ CanonRaw:"), &tag_info.get("Name")), verbose_val])?;
                    }
                }
            }
        }
        if value_in_dir.truthy() {
            let l = len(&get(&value));
            if pl::lt(&l, &n(8)) {
                // pad with original garbage in case it contained something useful
                let g = substr(it, &get(&buff), &pl::sub(&pl::add(&pt, &n(10)), &l), Some(&pl::sub(&n(8), &l)))?;
                put(&value, pl::concat(&get(&value), &g));
            } else if pl::gt(&l, &n(8)) {
                // this shouldn't happen
                pl::warn(it, "Value too long! -- truncated\n")?;
                let v = substr(it, &get(&value), &n(0), Some(&n(8)))?;
                put(&value, v);
            }
            // create new directory entry
            new_dir = cat(&[new_dir, s16u(it, tag.clone())?, get(&value)]);
            index = pl::add(&index, &n(1));
            continue; // all done this entry
        }
        if !get(&value).is_undef() {
            // don't allow value to change length unless Writable is 'resize'
            let writable = tag_info.get("Writable");
            let diff = pl::sub(&len(&get(&value)), &size);
            if diff.truthy() {
                if writable.truthy() && writable.eq_str("resize") {
                    size = pl::add(&size, &diff); // allow size to change
                } else if pl::gt(&diff, &n(0)) {
                    put(&value, pl::concat(&get(&value), &rep(b"\0", &diff)));
                } else {
                    let v = substr(it, &get(&value), &n(0), Some(&size))?;
                    put(&value, v);
                }
            }
            // pad value if necessary to align on even-byte boundary (as per CIFF spec)
            if band(&size, 0x01).truthy() {
                put(&value, pl::concat(&get(&value), &pl::s("\0")));
            }
            value_ptr = pl::sub(&out_pos, &out_base);
            // write out value data
            if !write(it, &outfile, vec![get(&value)])? {
                return ret(n(0));
            }
            out_pos = pl::add(&out_pos, &len(&get(&value))); // update current position in outfile
        }
        // create new directory entry
        new_dir = cat(&[new_dir, s16u(it, tag.clone())?, s32u(it, size.clone())?, s32u(it, value_ptr.clone())?]);
        index = pl::add(&index, &n(1));
    }
    let _ = tag_info;
    // add the directory counts and offset to the directory start,
    entries = pl::div(&len(&new_dir), &n(10))?;
    new_dir = cat(&[s16u(it, entries)?, new_dir, s32u(it, pl::sub(&out_pos, &out_base))?]);
    // write directory data
    if !write(it, &outfile, vec![new_dir.clone()])? {
        return ret(n(0));
    }

    // update current output file position in dirInfo
    dir_info.set("OutPos", pl::add(&out_pos, &len(&new_dir)));
    // save outfile directory start (needed for rewriting VRD trailer)
    dir_info.set("OutDirStart", pl::sub(&out_pos, &out_base));

    ret(n(1))
}

/// write Canon RAW (CRW) file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid CRW file,
///          or -1 if a write error occurred
pub fn write_crw(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let outfile = dir_info.get("OutFile");
    let raf = dir_info.get("RAF");
    let mut rtn_val = n(0);
    let (buff, sig) = (cell(Value::Undef), cell(Value::Undef));
    let mut err = Value::Undef;

    if !read_eq(it, &raf, &buff, &n(2))? {
        return ret(n(0));
    }
    if !set_byte_order(it, get(&buff))?.truthy() {
        return ret(n(0));
    }
    if !read_eq(it, &raf, &buff, &n(4))? {
        return ret(n(0));
    }
    if !read_eq(it, &raf, &sig, &n(8))? {
        return ret(n(0)); // get file signature
    }
    let Some(c) = re(r"^HEAP(CCDR|JPGM)", "").m(&get(&sig)) else {
        return ret(n(0)); // validate signature
    };
    let typ = pl::cap(&c, 1);
    let hlen = g32u(it, &rf(&buff), n(0))?; // get header length

    if et.get("DEL_GROUP").get("MakerNotes").truthy() {
        if typ.eq_str("CCDR") {
            pl::method(it, &et, "Error", vec![pl::s("Can't delete MakerNotes from CRW")])?;
            return ret(n(0));
        } else {
            inc(&et, "CHANGED");
            return ret(n(1));
        }
    }
    // make XMP the preferred group for CRW files
    if et.get("FILE_TYPE").eq_str("CRW") {
        let crw_map = wlex_hash(it, MODULE, FILE, "crwMap");
        pl::method(it, &et, "InitWriteDirs", vec![crw_map, pl::s("XMP")])?;
    }

    // write header
    if !seek(it, &raf, n(0), 0)? {
        return ret(n(0));
    }
    if !read_eq(it, &raf, &buff, &hlen)? {
        return ret(n(0));
    }
    if !write(it, &outfile, vec![get(&buff)])? {
        err = n(1);
    }

    if !seek(it, &raf, n(0), 2)? {
        return ret(n(0)); // seek to end of file
    }
    let filesize = tell(it, &raf)?;
    if !filesize.truthy() {
        return ret(n(0));
    }

    // build directory information for main raw directory
    let dinfo = Value::new_hash();
    dinfo.set("DataLen", n(0));
    dinfo.set("DirStart", hlen.clone());
    dinfo.set("DirLen", pl::sub(&filesize, &hlen));
    dinfo.set("Nesting", n(0));
    dinfo.set("RAF", raf.clone());
    dinfo.set("Parent", pl::s("CRW"));
    dinfo.set("OutFile", outfile.clone());
    dinfo.set("OutPos", hlen.clone());
    // process the raw directory
    let tag_table_ptr = etf(it, "GetTagTable", vec![pl::s("Image::ExifTool::CanonRaw::Main")])?;
    let mut success = pl::method(it, &et, "WriteDirectory", vec![dinfo.clone(), tag_table_ptr])?;

    let mut trail_pt = Value::Undef;
    if success.truthy() {
        // check to see if trailer(s) exist(s)
        let trail_info = pl::method(it, &et, "IdentifyTrailer", vec![raf.clone()])?;
        if trail_info.truthy() {
            // rewrite the trailer(s)
            put(&buff, pl::s(""));
            trail_info.set("OutFile", rf(&buff));
            success = pl::method(it, &et, "ProcessTrailers", vec![trail_info.clone()])?;
            if success.truthy() {
                trail_pt = trail_info.get("OutFile");
                // nothing to write if trailers were deleted
                if pl::lt(&len(&trail_pt.deref()), &n(4)) {
                    trail_pt = Value::Undef;
                }
            }
        }
    }
    if success.truthy() {
        // add CanonVRD trailer if writing as a block
        trail_pt = pl::method(it, &et, "AddNewTrailers", vec![trail_pt, pl::s("CanonVRD")])?;
        if !trail_pt.truthy() && et.get("ADD_DIRS").get("CanonVRD").truthy() {
            // create CanonVRD from scratch if necessary
            let outbuff = cell(pl::s(""));
            let save_order = byte_order(it)?;
            crate::runtime::require(it, "Image::ExifTool::CanonVRD")?;
            let h = Value::new_hash();
            h.set("OutFile", rf(&outbuff));
            if pl::gt(&pl::call(it, "Image::ExifTool::CanonVRD::ProcessCanonVRD", vec![et.clone(), h])?, &n(0)) {
                trail_pt = rf(&outbuff);
            }
            set_byte_order(it, save_order)?;
        }
        // write trailer
        if trail_pt.truthy() {
            // must append DirStart pointer to end of trailer
            let new_dir_start = s32u(it, dinfo.get("OutDirStart"))?;
            let l = len(&trail_pt.deref());
            let pad = pl::s(if band(&l, 0x01).truthy() { " " } else { "" }); // add pad byte if necessary
            let body = substr(it, &trail_pt.deref(), &n(0), Some(&pl::sub(&l, &n(4))))?;
            if !write(it, &outfile, vec![pad, body, new_dir_start])? {
                err = n(1);
            }
        }
        rtn_val = if err.truthy() { n(-1) } else { n(1) };
    } else {
        pl::method(it, &et, "Error", vec![pl::s("Error rewriting CRW file")])?;
    }
    ret(rtn_val)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::CanonRaw::InitMakerNotes", init_maker_notes);
    it.define_native("Image::ExifTool::CanonRaw::BuildMakerNotes", build_maker_notes);
    it.define_native("Image::ExifTool::CanonRaw::SaveMakerNotes", save_maker_notes);
    it.define_native("Image::ExifTool::CanonRaw::CheckCanonRaw", check_canon_raw);
    it.define_native("Image::ExifTool::CanonRaw::WriteCR2", write_cr2);
    it.define_native("Image::ExifTool::CanonRaw::WriteCanonRaw", write_canon_raw);
    it.define_native("Image::ExifTool::CanonRaw::WriteCRW", write_crw);
}
