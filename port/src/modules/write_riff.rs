//! Image::ExifTool::WriteRIFF (Image/ExifTool/WriteRIFF.pl), ported sub by sub (see docs/port/PORTING.md).
//!
//! Write RIFF-format files (currently WebP only). The file is autoloaded in
//! package Image::ExifTool::RIFF; its `my %webpMap` and `my %deletableGroup`
//! live under `Image::ExifTool::RIFF::<my:WriteRIFF.pl>::NAME`.

use crate::interp::eval::ER;
use crate::interp::{Binding, Interp, Want};
use crate::modules::exiftool::part2;
use crate::modules::xmp::{cat, hh};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, n, re, ret};

const MODULE: &str = "Image::ExifTool::RIFF";

/// A file lexical of WriteRIFF.pl (`my %webpMap`).
fn lex(it: &mut Interp, name: &str) -> Value {
    match it.global(b'%', &format!("{MODULE}::<my:WriteRIFF.pl>::{name}")) {
        Binding::Hash(h) => Value::HashRef(h),
        _ => Value::new_hash(),
    }
}

type Native = fn(&mut Interp, Vec<Value>, Want) -> ER<Vec<Value>>;

/// A call to one of ExifTool's data-access functions (Get32u, Set32u...).
fn da(it: &mut Interp, f: Native, args: Vec<Value>) -> ER<Value> {
    Ok(f(it, args, Want::Scalar)?.pop().unwrap_or_default())
}

/// `$raf->Read($buff, $len)`: the count, with `buff` updated.
fn read(it: &mut Interp, raf: &Value, buff: &mut Value, len: Value) -> ER<Value> {
    let mut args = [buff.clone(), len];
    let r = pl::method_mut(it, raf, "Read", &mut args, Want::Scalar)?;
    *buff = args[0].clone();
    Ok(r.into_iter().next().unwrap_or_default())
}

/// `Write($outfile, LIST)`: true on success.
fn write(it: &mut Interp, outfile: &Value, items: &[Value]) -> ER<bool> {
    let mut args = vec![outfile.clone()];
    args.extend_from_slice(items);
    Ok(pl::call(it, "Image::ExifTool::Write", args)?.truthy())
}

fn seek(it: &mut Interp, raf: &Value, pos: Value, whence: i64) -> ER<bool> {
    Ok(pl::method(it, raf, "Seek", vec![pos, n(whence)])?.truthy())
}

fn error(it: &mut Interp, et: &Value, msg: Value) -> ER<()> {
    pl::method(it, et, "Error", vec![msg])?;
    Ok(())
}

fn inc_changed(et: &Value) {
    et.set("CHANGED", pl::add(&et.get("CHANGED"), &n(1)));
}

/// `$x |= FLAG if COND`
fn or_flag(flags: &mut Value, cond: bool, flag: i64) {
    if cond {
        *flags = ops::bit_or(flags, &n(flag));
    }
}

/// Write RIFF file (currently WebP-type only)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref
/// Returns: 1 on success, 0 if this wasn't a valid RIFF file, or -1 if
///          an output file was specified and a write error occurred
pub fn write_riff(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    if !et.truthy() {
        return ret(n(1)); // allow dummy access to autoload this package
    }
    let outfile = dir_info.get("OutFile");
    let mut outsize = n(0);
    let raf = dir_info.get("RAF");
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let mut buff = Value::Undef;
    let mut err = false;
    let has = Value::new_hash();
    let dir_dat = Value::new_hash();
    let (mut image_width, mut image_height) = (Value::Undef, Value::Undef);
    let webp_map = lex(it, "webpMap");
    let deletable_group = lex(it, "deletableGroup");

    // do this in 2 passes so we can set the size of the containing RIFF chunk
    // without having to buffer the output (also to set the WebP_Flags)
    let mut pass: i64 = 0;
    'outer: loop {
        let done_dir = Value::new_hash();
        // verify this is a valid RIFF file
        if !pl::num_eq(&read(it, &raf, &mut buff, n(12))?, &n(12)) {
            return ret(n(0));
        }
        let Some(c) = re(r"^(RIFF|RF64)....(.{4})", "s").m(&buff) else {
            return ret(n(0));
        };
        let (c1, c2) = (pl::cap(&c, 1), pl::cap(&c, 2));
        if !(pl::str_eq(&c1, "RIFF") && pl::str_eq(&c2, "WEBP")) {
            let mut type_ = c2.clone();
            pl::tr(&mut type_, "-_a-zA-Z", "", "dc");
            error(it, &et, cat!("Can't currently write ", c1, " ", type_, " files"))?;
            return ret(n(1));
        }
        da(it, part2::set_byte_order, vec![pl::s("II")])?;

        // determine which directories we must write for this file type
        if pass != 0 {
            pl::method(it, &et, "Options", vec![pl::s("Verbose"), n(0)])?; // (avoid duplicate Verbose options here)
        }
        pl::method(it, &et, "InitWriteDirs", vec![webp_map.clone()])?;
        let add_dirs = et.get("ADD_DIRS");
        let edit_dirs = et.get("EDIT_DIRS");
        if add_dirs.get("EXIF").truthy() {
            add_dirs.set("IFD0", pl::s("EXIF")); // set flag to add IFD0 if adding EXIF (don't ask)
        }
        let (mut create_vp8x, mut delete_vp8x) = (false, false);

        // write header
        if pass != 0 {
            pl::method(it, &et, "Options", vec![pl::s("Verbose"), verbose.clone()])?;
            let needs_vp8x = has.get("ANIM").truthy() || has.get("XMP ").truthy() || has.get("EXIF").truthy() || has.get("ALPH").truthy() || has.get("ICCP").truthy();
            if has.get("VP8X").truthy() && !needs_vp8x && et.get("CHANGED").truthy() {
                delete_vp8x = true; // delete the VP8X chunk
                outsize = pl::sub(&outsize, &n(18)); // account for missing VP8X
            } else if needs_vp8x && !has.get("VP8X").truthy() {
                if !image_width.is_undef() {
                    inc_changed(&et);
                    create_vp8x = true; // add VP8X chunk
                    outsize = pl::add(&outsize, &n(18)); // account for VP8X size
                } else {
                    pl::method(it, &et, "Warn", vec![pl::s("Error getting image size for required VP8X chunk")])?;
                }
            }
            // finally we can set the overall RIFF chunk size:
            let r = pl::sref(buff.clone());
            da(it, part2::set32u, vec![pl::sub(&outsize, &n(8)), r.clone(), n(4)])?;
            buff = r.deref();
            if !write(it, &outfile, &[buff.clone()])? {
                err = true;
            }
            // create VP8X chunk if necessary
            if create_vp8x {
                pl::method(it, &et, "VPrint", vec![n(0), pl::s("  Adding required VP8X chunk (Extended WEBP)\n")])?;
                let mut flags = n(0);
                or_flag(&mut flags, has.get("ANIM").truthy(), 0x02);
                or_flag(&mut flags, has.get("XMP ").truthy(), 0x04);
                or_flag(&mut flags, has.get("EXIF").truthy(), 0x08);
                or_flag(&mut flags, has.get("ALPH").truthy(), 0x10);
                or_flag(&mut flags, has.get("ICCP").truthy(), 0x20);
                let w1 = pl::sub(&image_width, &n(1));
                let h1 = pl::sub(&image_height, &n(1));
                let v2 = ops::bit_or(&w1, &ops::shift_left(&ops::bit_and(&h1, &n(0xff)), &n(24)));
                let v3 = ops::shift_right(&h1, &n(8));
                let p = pl::pack("V3v", &[n(10), flags, v2, v3])?;
                write(it, &outfile, &[pl::s("VP8X"), p])?;
                // write ICCP after VP8X
                if dir_dat.get("ICCP").truthy() && !write(it, &outfile, &[dir_dat.get("ICCP")])? {
                    err = true;
                }
            }
        } else {
            outsize = pl::add(&outsize, &pl::length(&buff));
        }
        let mut pos = n(12);
        //
        // Read chunks in RIFF image
        //
        'chunks: loop {
            let (tag, mut len);
            let num = read(it, &raf, &mut buff, n(8))?;
            if pl::lt(&num, &n(8)) {
                if num.truthy() {
                    error(it, &et, pl::s("RIFF format error"))?;
                    return ret(n(1));
                }
                // all done if we hit end of file unless we need to add EXIF or XMP
                if !(add_dirs.get("EXIF").truthy() || add_dirs.get("XMP ").truthy() || add_dirs.get("ICCP").truthy()) {
                    break 'chunks;
                }
                // continue to add required EXIF or XMP chunks
                // ($num = $len = 0: $num is not read again)
                len = n(0);
                buff = pl::s("");
                tag = pl::s("");
            } else {
                pos = pl::add(&pos, &n(8));
                let u = pl::unpack("a4V", &buff)?;
                tag = arg(&u, 0);
                len = arg(&u, 1);
                if pl::le(&len, &n(0)) {
                    if pl::lt(&len, &n(0)) {
                        error(it, &et, pl::s("Invalid chunk length"))?;
                        return ret(n(1));
                    } else if pl::str_eq(&tag, "\0\0\0\0") {
                        // avoid reading through corrupted files filled with nulls because it takes forever
                        error(it, &et, pl::s("Encountered empty null chunk. Processing aborted"))?;
                        return ret(n(1));
                    } else {
                        // (just in case a tag may have no data)
                        if pass != 0 {
                            if !write(it, &outfile, &[buff.clone()])? {
                                err = true;
                            }
                        } else {
                            outsize = pl::add(&outsize, &pl::length(&buff));
                        }
                        continue 'chunks;
                    }
                }
            }
            // RIFF chunks are padded to an even number of bytes
            let mut len2 = pl::add(&len, &ops::bit_and(&len, &n(0x01)));
            // handle incorrect "XMP\0" chunk ID written by Google software
            let dg = deletable_group.get(tag.to_bytes());
            if dg.truthy() {
                if hh(&et, "DEL_GROUP").get(dg.to_bytes()).truthy() {
                    // just ignore this chunk if deleting the associated group
                    if !seek(it, &raf, len2.clone(), 1)? {
                        error(it, &et, pl::s("Seek error"))?;
                        break 'chunks;
                    }
                    if pass != 0 {
                        pl::method(it, &et, "VPrint", vec![n(0), cat!("  Deleting ", dg, "\n")])?;
                    }
                    inc_changed(&et);
                    continue 'chunks;
                } else if pl::str_eq(&tag, "XMP\0") && pass != 0 {
                    pl::method(it, &et, "Warn", vec![pl::s("Incorrect XMP tag ID"), n(1)])?;
                }
            }
            // edit/add/delete necessary metadata chunks (EXIF must come before XMP)
            if edit_dirs.get(tag.to_bytes()).truthy() || pl::str_eq(&tag, "") || (pl::str_eq(&tag, "XMP ") && add_dirs.get("EXIF").truthy()) {
                let mut handled_tag = false;
                if len2.truthy() {
                    if done_dir.get(tag.to_bytes()).truthy() && pass == 0 {
                        pl::method(it, &et, "Warn", vec![cat!("Duplicate '", tag, "' chunk")])?;
                    }
                    done_dir.set(tag.to_bytes(), n(1));
                    if !pl::num_eq(&read(it, &raf, &mut buff, len2.clone())?, &len2) {
                        error(it, &et, cat!("Truncated '", tag, "' chunk"))?;
                        break 'chunks;
                    }
                    pos = pl::add(&pos, &len2); // update current position
                } else {
                    buff = pl::s("");
                }
                //
                // add/edit/delete EXIF/XMP/ICCP (note: EXIF must come before XMP, and ICCP is written elsewhere)
                //
                let dir_name = |d: &str| match d {
                    "EXIF" => "IFD0",
                    "XMP " => "XMP",
                    "ICCP" => "ICC_Profile",
                    _ => "JUMBF",
                };
                let tbl_name = |d: &str| match d {
                    "EXIF" => "Exif",
                    "XMP " => "XMP",
                    "ICCP" => "ICC_Profile",
                    _ => "Jpeg2000",
                };
                for dir in ["EXIF", "XMP ", "ICCP", "C2PA"] {
                    if !(pl::str_eq(&tag, dir) || (add_dirs.get(dir).truthy() && (pl::str_eq(&tag, "") || (pl::str_eq(&tag, "XMP ") && dir == "EXIF")))) {
                        continue;
                    }
                    let mut start = Value::Undef;
                    if pass == 0 {
                        // write the EXIF and save the result for the next pass
                        let mut data_pt = pl::sref(buff.clone());
                        let mut buff_is_data = true;
                        if pl::str_eq(&tag, "EXIF") {
                            // (only need to set directory $start for EXIF)
                            if re(r"^Exif\0\0", "").is_match(&buff) {
                                if hh(&et, "DEL_GROUP").get("EXIF").truthy() {
                                    // remove incorrect header if rewriting anyway
                                    buff = pl::substr(&buff, 6, None);
                                    data_pt = pl::sref(buff.clone());
                                    len = pl::sub(&len, &n(6));
                                    len2 = pl::sub(&len2, &n(6));
                                } else {
                                    pl::method(it, &et, "Warn", vec![pl::s("Improper EXIF header"), n(1)])?;
                                    start = n(6);
                                }
                            } else {
                                start = n(0);
                            }
                        } else if !pl::str_eq(&tag, dir) {
                            // create from scratch
                            data_pt = pl::sref(pl::s(""));
                            buff_is_data = false;
                        }
                        // write the new directory to memory
                        let d_info = Value::new_hash();
                        d_info.set("DataPt", data_pt.clone());
                        d_info.set("DataPos", n(0)); // (relative to Base)
                        d_info.set("DirStart", start.clone());
                        d_info.set("Base", pl::sub(&pos, &len2));
                        d_info.set("Parent", pl::s(dir));
                        d_info.set("DirName", pl::s(dir_name(dir)));
                        // must pass the TagInfo to enable deletion of C2PA information
                        let main_info = pl::pkg_hash(it, "Image::ExifTool::RIFF::Main").get(dir);
                        if main_info.ref_type() == "HASH" {
                            d_info.set("TagInfo", main_info);
                        }
                        let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s(&format!("Image::ExifTool::{}::Main", tbl_name(dir)))])?;
                        // (override writeProc for EXIF because it has the TIFF header)
                        let write_proc = if dir == "EXIF" { Value::Code(it.sub_ref("Image::ExifTool::WriteTIFF")) } else { Value::Undef };
                        let r = pl::method(it, &et, "WriteDirectory", vec![d_info, tag_table_ptr, write_proc])?;
                        dir_dat.set(dir, r);
                        if buff_is_data {
                            buff = data_pt.deref(); // ($dataPt is \$buff)
                        }
                    }
                    add_dirs.delete(dir); // (don't try to add again)
                    if !dir_dat.get(dir).is_undef() {
                        if pl::str_eq(&tag, dir) {
                            handled_tag = true; // set flag indicating we edited this tag
                            // increment CHANGED count if we are deleting the directory
                            if !pl::length(&dir_dat.get(dir)).truthy() {
                                inc_changed(&et);
                            }
                        }
                        if pl::length(&dir_dat.get(dir)).truthy() {
                            if pass != 0 {
                                // write metadata chunk now (but not ICCP because it was added earlier)
                                if dir != "ICCP" && !write(it, &outfile, &[dir_dat.get(dir)])? {
                                    err = true;
                                }
                            } else {
                                // preserve (incorrect EXIF) header if it existed
                                let hdr = if start.truthy() { pl::substr(&buff, 0, Some(start.to_i64())) } else { pl::s("") };
                                // (don't overwrite $len here because it may be XMP length)
                                let dir_len = pl::add(&pl::length(&dir_dat.get(dir)), &pl::length(&hdr));
                                // add chunk header and padding
                                let pad = if ops::bit_and(&dir_len, &n(0x01)).truthy() { pl::s("\0") } else { pl::s("") };
                                let s32 = da(it, part2::set32u, vec![dir_len])?;
                                dir_dat.set(dir, cat!(dir, s32, hdr, dir_dat.get(dir), pad));
                                outsize = pl::add(&outsize, &pl::length(&dir_dat.get(dir)));
                                has.set(dir, n(1));
                            }
                        }
                    }
                }
                //
                // just copy XMP, EXIF or ICC if nothing changed
                //
                if !handled_tag && pl::length(&buff).truthy() {
                    // write the chunk without changes
                    if pass != 0 {
                        let s32 = da(it, part2::set32u, vec![len.clone()])?;
                        if !write(it, &outfile, &[tag.clone(), s32, buff.clone()])? {
                            err = true;
                        }
                    } else {
                        outsize = pl::add(&outsize, &pl::add(&n(8), &pl::length(&buff)));
                        has.set(tag.to_bytes(), n(1));
                    }
                }
                continue 'chunks;
            }
            pos = pl::add(&pos, &len2); // set read position at end of chunk data
            //
            // update necessary flags in VP8X chunk
            //
            if pl::str_eq(&tag, "VP8X") {
                let mut buf2 = Value::Undef;
                if pl::lt(&len2, &n(10)) || !pl::num_eq(&read(it, &raf, &mut buf2, len2.clone())?, &len2) {
                    error(it, &et, pl::s("Truncated VP8X chunk"))?;
                    return ret(n(1));
                }
                if pass != 0 {
                    if delete_vp8x {
                        pl::method(it, &et, "VPrint", vec![n(0), pl::s("  Deleting unnecessary VP8X chunk (Standard WEBP)\n")])?;
                        continue 'chunks;
                    }
                    // ...but first set the VP8X flags
                    let r2 = pl::sref(buf2.clone());
                    let mut flags = da(it, part2::get32u, vec![r2.clone(), n(0)])?;
                    flags = ops::bit_and(&flags, &ops::bit_not(&n(0x2c))); // (reset flags for everything we can write)
                    or_flag(&mut flags, has.get("XMP ").truthy(), 0x04);
                    or_flag(&mut flags, has.get("EXIF").truthy(), 0x08);
                    or_flag(&mut flags, has.get("ICCP").truthy(), 0x20);
                    da(it, part2::set32u, vec![flags, r2.clone(), n(0)])?;
                    buf2 = r2.deref();
                    if !write(it, &outfile, &[buff.clone(), buf2.clone()])? {
                        err = true;
                    }
                } else {
                    // get the image size
                    let r2 = pl::sref(buf2.clone());
                    image_width = pl::add(&ops::bit_and(&da(it, part2::get32u, vec![r2.clone(), n(4)])?, &n(0xffffff)), &n(1));
                    image_height = pl::add(&ops::shift_right(&da(it, part2::get32u, vec![r2, n(6)])?, &n(8)), &n(1));
                    outsize = pl::add(&outsize, &pl::add(&n(8), &len2));
                    has.set(tag.to_bytes(), n(1));
                }
                // write ICCP after VP8X
                if dir_dat.get("ICCP").truthy() && !write(it, &outfile, &[dir_dat.get("ICCP")])? {
                    err = true;
                }
                continue 'chunks;
            }
            //
            // just copy all other chunks
            //
            if pass != 0 {
                // write chunk header (still in $buff)
                if !write(it, &outfile, &[buff.clone()])? {
                    err = true;
                }
            } else {
                outsize = pl::add(&outsize, &pl::length(&buff));
                has.set(tag.to_bytes(), n(1));
            }
            if !(pass != 0 || !image_width.is_undef()) {
                // get WebP image size from VP8 or VP8L header
                if pl::str_eq(&tag, "VP8 ") && pl::ge(&len2, &n(16)) {
                    if !pl::num_eq(&read(it, &raf, &mut buff, n(16))?, &n(16)) {
                        error(it, &et, pl::s("Truncated VP8 chunk"))?;
                        return ret(n(1));
                    }
                    outsize = pl::add(&outsize, &n(16));
                    if re(r"^...\x9d\x01\x2a", "s").is_match(&buff) {
                        let r = pl::sref(buff.clone());
                        image_width = ops::bit_and(&da(it, part2::get16u, vec![r.clone(), n(6)])?, &n(0x3fff));
                        image_height = ops::bit_and(&da(it, part2::get16u, vec![r, n(8)])?, &n(0x3fff));
                    }
                    len2 = pl::sub(&len2, &n(16));
                } else if pl::str_eq(&tag, "VP8L") && pl::ge(&len2, &n(6)) {
                    if !pl::num_eq(&read(it, &raf, &mut buff, n(6))?, &n(6)) {
                        error(it, &et, pl::s("Truncated VP8L chunk"))?;
                        return ret(n(1));
                    }
                    outsize = pl::add(&outsize, &n(6));
                    if re(r"^\x2f", "s").is_match(&buff) {
                        let r = pl::sref(buff.clone());
                        let word = da(it, part2::get32u, vec![r.clone(), n(2)])?;
                        image_width = pl::add(&ops::bit_and(&da(it, part2::get16u, vec![r, n(1)])?, &n(0x3fff)), &n(1));
                        image_height = pl::add(&ops::bit_and(&ops::shift_right(&word, &n(6)), &n(0x3fff)), &n(1));
                        if ops::bit_and(&word, &n(0x100000)).truthy() {
                            has.set("ALPH", n(1)); // set alpha flag if necessary
                        }
                    }
                    len2 = pl::sub(&len2, &n(6));
                }
            }
            if pass != 0 {
                // copy the chunk data in 64k blocks
                while len2.truthy() {
                    let mut num = len2.clone();
                    if pl::gt(&num, &n(65536)) {
                        num = n(65536);
                    }
                    if !pl::num_eq(&read(it, &raf, &mut buff, num.clone())?, &num) {
                        error(it, &et, pl::s("Truncated RIFF chunk"))?;
                        break;
                    }
                    if !write(it, &outfile, &[buff.clone()])? {
                        err = true;
                        break;
                    }
                    len2 = pl::sub(&len2, &num);
                }
            } else {
                if !seek(it, &raf, len2.clone(), 1)? {
                    error(it, &et, pl::s("Seek error"))?;
                    break 'chunks;
                }
                outsize = pl::add(&outsize, &len2);
            }
        }
        if pass != 0 {
            break 'outer;
        }
        if !seek(it, &raf, n(0), 0)? {
            error(it, &et, pl::s("Seek error"))?;
            break 'outer;
        }
        pass += 1;
    }
    ret(n(if err { -1 } else { 1 }))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::RIFF::WriteRIFF", write_riff);
}
