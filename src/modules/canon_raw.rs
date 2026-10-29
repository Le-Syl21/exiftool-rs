//! Image::ExifTool::CanonRaw (Image/ExifTool/CanonRaw.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! The writer routines (WriteCRW, WriteCanonRaw, CheckCanonRaw, the maker
//! note builders) live in WriteCanonRaw.pl and are autoloaded.

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::exiftool::part2;
use crate::modules::random_access::{cell, raf_read_cell};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, n, ret};

const MODULE: &str = "Image::ExifTool::CanonRaw";

/// `Image::ExifTool::NAME(ARGS)` (the functions CanonRaw.pm imports).
fn et(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

fn val(c: &Cell) -> Value {
    c.borrow().clone()
}

fn put(c: &Cell, v: Value) {
    *c.borrow_mut() = v;
}

/// `$raf->Seek(POS, WHENCE)`
fn seek(it: &mut Interp, raf: &Value, pos: Value, whence: i64) -> ER<bool> {
    Ok(pl::method(it, raf, "Seek", vec![pos, n(whence)])?.truthy())
}

/// `$raf->Read($buff, $len) == $len`
fn read_eq(it: &mut Interp, raf: &Value, buff: &Cell, len: &Value) -> ER<bool> {
    let r = raf_read_cell(it, raf, buff, len.clone())?;
    Ok(pl::num_eq(&r, len))
}

/// AutoLoad our writer routines when necessary
pub fn autoload(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let name = it.get_scalar("Image::ExifTool::CanonRaw::AUTOLOAD");
    let mut args = vec![name];
    args.extend(a);
    pl::at(608);
    it.call_named("Image::ExifTool::DoAutoLoad", args, want)
}

/// Process Raw file directory
/// Inputs: 0) ExifTool object reference
///         1) directory information reference, 2) tag table reference
/// Returns: 1 on success
pub fn process_canon_raw(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et_, dir_info, raw_tag_table) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let block_start = dir_info.get("DirStart");
    let block_size = dir_info.get("DirLen");
    let raf = dir_info.get("RAF");
    if !raf.truthy() {
        return ret(n(0));
    }
    let buff = cell(Value::Undef);
    let verbose = pl::method(it, &et_, "Options", vec![pl::s("Verbose")])?;
    let build_maker_notes = pl::method(it, &et_, "Options", vec![pl::s("MakerNotes")])?;

    // 4 bytes at end of block give directory position within block
    if !seek(it, &raf, pl::sub(&pl::add(&block_start, &block_size), &n(4)), 0)? {
        return ret(n(0));
    }
    if !read_eq(it, &raf, &buff, &n(4))? {
        return ret(n(0));
    }
    let buff_ref = Value::ScalarRef(buff.clone());
    let dir_offset = pl::add(&et(it, "Get32u", vec![buff_ref.clone(), n(0)])?, &block_start);
    // avoid infinite recursion
    if !et_.get("ProcessedCanonRaw").truthy() {
        et_.set("ProcessedCanonRaw", Value::new_hash());
    }
    if et_.get("ProcessedCanonRaw").get(dir_offset.to_bytes()).truthy() {
        let msg = pl::concat(&pl::concat(&pl::s("Not processing double-referenced "), &dir_info.get("DirName")), &pl::s(" directory"));
        pl::method(it, &et_, "Warn", vec![msg])?;
        return ret(n(0));
    }
    et_.get("ProcessedCanonRaw").set(dir_offset.to_bytes(), n(1));
    if !seek(it, &raf, dir_offset.clone(), 0)? {
        return ret(n(0));
    }
    if !read_eq(it, &raf, &buff, &n(2))? {
        return ret(n(0));
    }
    let entries = et(it, "Get16u", vec![buff_ref.clone(), n(0)])?; // get number of entries in directory
    // read the directory (10 bytes per entry)
    if !read_eq(it, &raf, &buff, &pl::mul(&n(10), &entries))? {
        return ret(n(0));
    }

    if verbose.truthy() {
        pl::method(it, &et_, "VerboseDir", vec![pl::s("CIFF"), entries.clone()])?;
    }
    let crw_tag_format = pl::pkg_hash(it, "Image::ExifTool::CanonRaw::crwTagFormat");
    let mut index = n(0);
    while pl::lt(&index, &entries) {
        if let Some(r) = process_entry(it, &et_, &dir_info, &raw_tag_table, &raf, &buff, &crw_tag_format, &verbose, &build_maker_notes, &block_start, &dir_offset, &index)? {
            return ret(r);
        }
        index = pl::add(&index, &n(1));
    }
    ret(n(1))
}

/// One pass of ProcessCanonRaw's directory loop (`next` is a return of
/// None, `return 1` a return of Some(1)).
#[allow(clippy::too_many_arguments)]
fn process_entry(it: &mut Interp, et_: &Value, dir_info: &Value, raw_tag_table: &Value, raf: &Value, buff: &Cell, crw_tag_format: &Value, verbose: &Value, build_maker_notes: &Value, block_start: &Value, dir_offset: &Value, index: &Value) -> ER<Option<Value>> {
    let buff_ref = Value::ScalarRef(buff.clone());
    let pt = pl::mul(&n(10), index);
    let tag = et(it, "Get16u", vec![buff_ref.clone(), pt.clone()])?;
    let mut size = et(it, "Get32u", vec![buff_ref.clone(), pl::add(&pt, &n(2))])?;
    let value_ptr = et(it, "Get32u", vec![buff_ref.clone(), pl::add(&pt, &n(6))])?;
    let ptr = pl::add(&value_ptr, block_start); // all pointers relative to block start
    if ops::bit_and(&tag, &n(0x8000)).truthy() {
        pl::method(it, et_, "Warn", vec![pl::s("Bad CRW directory entry")])?;
        return Ok(Some(n(1)));
    }
    let tag_id = ops::bit_and(&tag, &n(0x3fff)); // get tag ID
    let tag_type = ops::bit_and(&ops::shift_right(&tag, &n(8)), &n(0x38)); // get tag type
    let value_in_dir = ops::bit_and(&tag, &n(0x4000)); // flag for value in directory
    let tag_info = pl::method(it, et_, "GetTagInfo", vec![raw_tag_table.clone(), tag_id.clone()])?;
    if (pl::num_eq(&tag_type, &n(0x28)) || pl::num_eq(&tag_type, &n(0x30))) && !value_in_dir.truthy() {
        // this type of tag specifies a raw subdirectory
        let mut name = Value::Undef;
        if tag_info.truthy() {
            name = tag_info.get("Name");
        }
        if !name.truthy() {
            name = pl::sprintf("CanonRaw_0x%.4x", &[tag.clone()]);
        }
        let subdir_info = Value::new_hash();
        subdir_info.set("DirName", name.clone());
        subdir_info.set("DataLen", n(0));
        subdir_info.set("DirStart", ptr.clone());
        subdir_info.set("DirLen", size.clone());
        subdir_info.set("Nesting", pl::add(&dir_info.get("Nesting"), &n(1)));
        subdir_info.set("RAF", raf.clone());
        subdir_info.set("Parent", dir_info.get("DirName"));
        if verbose.truthy() {
            let fake_info = Value::new_hash();
            fake_info.set("Name", name);
            fake_info.set("SubDirectory", Value::new_hash());
            pl::method(it, et_, "VerboseInfo", vec![tag_id.clone(), fake_info, pl::s("Index"), index.clone(), pl::s("Size"), size.clone(), pl::s("Start"), ptr.clone()])?;
        }
        pl::method(it, et_, "ProcessDirectory", vec![subdir_info, raw_tag_table.clone()])?;
        return Ok(None);
    }
    let value_data_pos;
    let (mut count, mut subdir) = (Value::Undef, Value::Undef);
    let mut format = crw_tag_format.get(tag_type.to_bytes());
    if tag_info.truthy() {
        subdir = tag_info.get("SubDirectory");
        if tag_info.get("Format").truthy() {
            format = tag_info.get("Format");
        }
        count = tag_info.get("Count");
    }
    // get value data
    let value = cell(Value::Undef);
    let mut del_raw_conv = false;
    if value_in_dir.truthy() {
        // is the value data in the directory?
        // this type of tag stores the value in the 'size' and 'ptr' fields
        value_data_pos = pl::add(&pl::add(dir_offset, &pt), &n(4)); // (remember, +2 for the entry count)
        size = n(8);
        put(&value, crate::modules::random_access::substr(it, &val(buff), &pl::add(&pt, &n(2)), Some(&size))?);
        // set count to 1 by default for normal values in directory
        if count.is_undef() && format.truthy() && !pl::str_eq(&format, "string") && !subdir.truthy() {
            count = n(1);
        }
    } else {
        value_data_pos = ptr.clone();
        // do hash of image data if requested
        if et_.get("ImageDataHash").truthy() && pl::num_eq(&tag_id, &n(0x2005)) && seek(it, raf, ptr.clone(), 0)? {
            pl::method(it, et_, "ImageDataHash", vec![raf.clone(), size.clone(), pl::s("raw")])?;
        }
        let mut read_it = pl::le(&size, &n(512)) || (pl::gt(verbose, &n(2)) && pl::le(&size, &n(65536)));
        if !read_it && tag_info.truthy() {
            if tag_info.get("SubDirectory").truthy() {
                read_it = true;
            } else {
                let mut pat = b"^".to_vec();
                pat.extend_from_slice(&tag_info.get("Name").to_bytes());
                pat.push(b'$');
                let re = pl::re_dyn(&pat, "i")?;
                let req = pl::method_list(it, et_, "GetRequestedTags", vec![])?;
                read_it = req.iter().any(|t| re.is_match(t));
            }
        }
        if read_it {
            // read value if size is small or specifically requested
            // or if this is a SubDirectory
            if !(seek(it, raf, ptr.clone(), 0)? && read_eq(it, raf, &value, &size)?) {
                pl::method(it, et_, "Warn", vec![pl::sprintf("Error reading %d bytes from 0x%x", &[size, ptr])])?;
                return Ok(None);
            }
        } else {
            put(&value, pl::concat(&pl::concat(&pl::s("Binary data "), &size), &pl::s(" bytes")));
            if tag_info.truthy() {
                if pl::method(it, et_, "Options", vec![pl::s("Binary")])?.truthy() || verbose.truthy() {
                    // read the value anyway
                    if !(seek(it, raf, ptr.clone(), 0)? && read_eq(it, raf, &value, &size)?) {
                        pl::method(it, et_, "Warn", vec![pl::sprintf("Error reading %d bytes from 0x%x", &[size, ptr])])?;
                        return Ok(None);
                    }
                }
                // force this to be a binary (scalar reference)
                tag_info.set("RawConv", pl::s("\\$val"));
                del_raw_conv = true;
            }
            size = pl::length(&val(&value));
            format = Value::Undef;
        }
    }
    // set count from tagInfo count if necessary
    if format.truthy() && !count.truthy() {
        // set count according to format and size
        let fnum = pl::pkg_hash(it, "Image::ExifTool::Exif::formatNumber").get(format.to_bytes());
        let fsiz = pl::pkg_array(it, "Image::ExifTool::Exif::formatSize").at(fnum.to_i64());
        pl::at(740);
        count = pl::int(&pl::div(&size, &fsiz)?);
    }
    if verbose.truthy() {
        let mut v = val(&value);
        if format.truthy() {
            v = et(it, "ReadValue", vec![pl::sref(v), n(0), format.clone(), count.clone(), size.clone()])?;
        }
        pl::method(
            it,
            et_,
            "VerboseInfo",
            vec![
                tag_id.clone(),
                tag_info.clone(),
                pl::s("Table"),
                raw_tag_table.clone(),
                pl::s("Index"),
                index.clone(),
                pl::s("Value"),
                v,
                pl::s("DataPt"),
                Value::ScalarRef(value.clone()),
                pl::s("DataPos"),
                value_data_pos.clone(),
                pl::s("Size"),
                size.clone(),
                pl::s("Format"),
                format.clone(),
                pl::s("Count"),
                count.clone(),
            ],
        )?;
    }
    if build_maker_notes.truthy() {
        // build maker notes information if requested
        pl::call(it, "Image::ExifTool::CanonRaw::BuildMakerNotes", vec![et_.clone(), tag_id.clone(), tag_info.clone(), Value::ScalarRef(value.clone()), format.clone(), count.clone()])?;
    }
    if tag_info.is_undef() {
        return Ok(None);
    }

    if subdir.truthy() {
        let name = tag_info.get("Name");
        let new_tag_table;
        if subdir.get("TagTable").truthy() {
            new_tag_table = et(it, "GetTagTable", vec![subdir.get("TagTable")])?;
            if !new_tag_table.truthy() {
                pl::warn(it, &format!("Unknown tag table {}\n", subdir.get("TagTable").to_text()))?;
                return Ok(None);
            }
        } else {
            pl::warn(it, &format!("Must specify TagTable for SubDirectory {}\n", name.to_text()))?;
            return Ok(None);
        }
        let mut subdir_start = n(0);
        //### eval Start ()
        if subdir.get("Start").truthy() {
            let scope = crate::runtime::file_scope(it, MODULE);
            subdir_start = pl::eval_site_in(it, &subdir.get("Start"), MODULE, Some(scope), &[], Want::Scalar).0;
        }
        let dir_data = Value::ScalarRef(value.clone());
        let subdir_info = Value::new_hash();
        subdir_info.set("Name", name.clone());
        subdir_info.set("DataPt", dir_data.clone());
        subdir_info.set("DataLen", size.clone());
        subdir_info.set("DataPos", value_data_pos);
        subdir_info.set("DirStart", subdir_start.clone());
        subdir_info.set("DirLen", pl::sub(&size, &subdir_start));
        subdir_info.set("Nesting", pl::add(&dir_info.get("Nesting"), &n(1)));
        subdir_info.set("RAF", raf.clone());
        subdir_info.set("DirName", name.clone());
        subdir_info.set("Parent", dir_info.get("DirName"));
        //### eval Validate ($dirData, $subdirStart, $size)
        let invalid = !subdir.get("Validate").is_undef() && {
            let scope = crate::runtime::file_scope(it, MODULE);
            let vars = [("dirData", dir_data), ("subdirStart", subdir_start), ("size", size)];
            !pl::eval_site_in(it, &subdir.get("Validate"), MODULE, Some(scope), &vars, Want::Scalar).0.truthy()
        };
        if invalid {
            pl::method(it, et_, "Warn", vec![pl::concat(&pl::concat(&pl::s("Invalid "), &name), &pl::s(" data"))])?;
        } else {
            pl::method(it, et_, "ProcessDirectory", vec![subdir_info, new_tag_table, subdir.get("ProcessProc")])?;
        }
    } else {
        // convert to specified format if necessary
        if format.truthy() {
            let v = et(it, "ReadValue", vec![Value::ScalarRef(value.clone()), n(0), format, count, size])?;
            put(&value, v);
        }
        // save the information
        pl::method(it, et_, "FoundTag", vec![tag_info.clone(), val(&value)])?;
        if del_raw_conv {
            tag_info.delete("RawConv");
        }
    }
    Ok(None)
}

/// get information from raw file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 if this was a valid Canon RAW file
pub fn process_crw(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et_, dir_info) = (arg(&a, 0), arg(&a, 1));
    let (buff, sig) = (cell(Value::Undef), cell(Value::Undef));
    let raf = dir_info.get("RAF");
    let build_maker_notes = pl::method(it, &et_, "Options", vec![pl::s("MakerNotes")])?;

    pl::at(819);
    part2::check_invocant(it, &raf, "Read")?;
    if !read_eq(it, &raf, &buff, &n(2))? {
        return ret(n(0));
    }
    if !et(it, "SetByteOrder", vec![val(&buff)])?.truthy() {
        return ret(n(0));
    }
    if !read_eq(it, &raf, &buff, &n(4))? {
        return ret(n(0));
    }
    if !read_eq(it, &raf, &sig, &n(8))? {
        return ret(n(0)); // get file signature
    }
    if !pl::re(r"^HEAP(CCDR|JPGM)", "").is_match(&val(&sig)) {
        return ret(n(0)); // validate signature
    }
    let hlen = et(it, "Get32u", vec![Value::ScalarRef(buff.clone()), n(0)])?;

    if !seek(it, &raf, n(0), 2)? {
        return ret(n(0)); // seek to end of file
    }
    let filesize = pl::method(it, &raf, "Tell", vec![])?;
    if !filesize.truthy() {
        return ret(n(0));
    }

    // initialize maker note data if building maker notes
    if build_maker_notes.truthy() {
        pl::call(it, "Image::ExifTool::CanonRaw::InitMakerNotes", vec![et_.clone()])?;
    }

    // set the FileType tag unless already done (eg. APP0 CIFF record in JPEG image)
    pl::method(it, &et_, "SetFileType", vec![])?;

    // build directory information for main raw directory
    let d = Value::new_hash();
    d.set("DataLen", n(0));
    d.set("DirStart", hlen.clone());
    d.set("DirLen", pl::sub(&filesize, &hlen));
    d.set("Nesting", n(0));
    d.set("RAF", raf.clone());
    d.set("Parent", pl::s("CRW"));

    // process the raw directory
    let raw_tag_table = et(it, "GetTagTable", vec![pl::s("Image::ExifTool::CanonRaw::Main")])?;
    let old_indent = et_.get("INDENT");
    et_.set("INDENT", pl::concat(&et_.get("INDENT"), &pl::s("| ")));
    let r = process_canon_raw(it, vec![et_.clone(), d, raw_tag_table], Want::Scalar)?;
    if !arg(&r, 0).truthy() {
        pl::method(it, &et_, "Warn", vec![pl::s("CRW file format error")])?;
    }
    et_.set("INDENT", old_indent);

    // finish building maker notes if necessary
    if build_maker_notes.truthy() {
        pl::call(it, "Image::ExifTool::CanonRaw::SaveMakerNotes", vec![et_.clone()])?;
    }

    // process trailers if they exist in CRW file (not in CIFF information!)
    if et_.get("FILE_TYPE").eq_str("CRW") {
        let trail_info = pl::method(it, &et_, "IdentifyTrailer", vec![raf])?;
        if trail_info.truthy() {
            pl::method(it, &et_, "ProcessTrailers", vec![trail_info])?;
        }
    }

    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::CanonRaw::AUTOLOAD", autoload);
    it.define_native("Image::ExifTool::CanonRaw::ProcessCanonRaw", process_canon_raw);
    it.define_native("Image::ExifTool::CanonRaw::ProcessCRW", process_crw);
}
