//! Image::ExifTool::PNG (Image/ExifTool/PNG.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read and write PNG meta information. The writer routines (WritePNG.pl:
//! CalculateCRC, AddChunks, Add_iCCP, DoneDir, BuildTextChunk, HexEncode...)
//! are autoloaded and called by name.

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::cell;
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

const MODULE: &str = "Image::ExifTool::PNG";

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `NAME(ARGS)` of this package (ported here, or autoloaded from WritePNG.pl).
fn png_call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::PNG::{name}"), args)
}

fn m(it: &mut Interp, et: &Value, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::method(it, et, name, args)
}

fn opt(it: &mut Interp, et: &Value, name: &str) -> ER<Value> {
    pl::method(it, et, "Options", vec![pl::s(name)])
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
    Ok(call(it, "Write", args)?.truthy())
}

/// `print $out MSG`
fn print(it: &mut Interp, out: &Value, msg: Value) -> ER<()> {
    pl::print_to(it, Some(out), &[msg])?;
    Ok(())
}

/// String concatenation of several values.
fn cat(parts: &[&Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

fn inc_changed(et: &Value) {
    et.set("CHANGED", pl::add(&et.get("CHANGED"), &n(1)));
}

/// `split /\0/, $str, $limit`
fn split0(s: &Value, limit: i64) -> Vec<Value> {
    pl::split(&re(r"\0", ""), s, limit)
}

/// `$$et{INDENT} =~ s/..$//`
fn unindent(et: &Value) {
    let mut ind = et.get("INDENT");
    if re(r"..$", "").subst(&mut ind, false, |_| Vec::new()) > 0 {
        et.set("INDENT", ind);
    }
}

/// `\&Image::ExifTool::NAME`
fn code_ref(it: &mut Interp, name: &str) -> Value {
    Value::Code(it.sub_ref(name))
}

/// AutoLoad our writer routines when necessary
pub fn autoload(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let name = it.get_scalar("Image::ExifTool::PNG::AUTOLOAD");
    let mut args = vec![name];
    args.extend(a);
    it.call_named("Image::ExifTool::DoAutoLoad", args, want)
}

/// Get standard case for language code (this routine copied from XMP.pm)
/// Inputs: 0) Language code
/// Returns: Language code in standard case
pub fn standard_lang_case(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let lang = arg(&a, 0);
    // make 2nd subtag uppercase only if it is 2 letters
    if let Some(c) = re(r"^([a-z]{2,3}|[xi])(-[a-z]{2})\b(.*)", "i").m(&lang) {
        return ret(cat(&[&pl::lc(&cap(&c, 1)), &pl::uc(&cap(&c, 2)), &pl::lc(&cap(&c, 3))]));
    }
    ret(pl::lc(&lang))
}

fn standard_lang_case1(it: &mut Interp, lang: Value) -> ER<Value> {
    Ok(standard_lang_case(it, vec![lang], Want::Scalar)?.pop().unwrap_or_default())
}

/// Convert date from PNG to EXIF format
/// Inputs: 0) Date/time in PNG format, 1) ExifTool ref
/// Returns: EXIF formatted date/time string
pub fn convert_png_date(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val, et) = (arg(&a, 0), arg(&a, 1));
    let month_num = crate::runtime::lex_hash(it, MODULE, "monthNum");
    let tz_conv = crate::runtime::lex_hash(it, MODULE, "tzConv");
    // standard format is like "Mon, 1 Jan 2018 12:10:22 EST" (RFC-1123 section 5.2.14)
    if let Some(c) = re(r"(\d+)\s*(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\s*(\d+)\s+(\d+):(\d{2})(:\d{2})?\s*(\S*)", "i").m(&val) {
        let (day, mut mon, mut yr, hr, min, sec, mut tz) = (cap(&c, 1), cap(&c, 2), cap(&c, 3), cap(&c, 4), cap(&c, 5), cap(&c, 6), cap(&c, 7));
        if pl::lt(&yr, &n(100)) {
            yr = pl::add(&yr, &n(if pl::gt(&yr, &n(70)) { 1900 } else { 2000 })); // boost year to 4 digits if necessary
        }
        mon = month_num.get(pl::ucfirst(&pl::lc(&mon)).to_bytes());
        if !mon.truthy() {
            return ret(val);
        }
        let mut standard = true;
        if !tz.truthy() {
            tz = pl::s("");
        } else if tz_conv.get(pl::uc(&tz).to_bytes()).truthy() {
            tz = tz_conv.get(pl::uc(&tz).to_bytes());
        } else if let Some(t) = re(r"^([-+]\d+):?(\d{2})", "").m(&tz) {
            tz = cat(&[&cap(&t, 1), &pl::s(":"), &cap(&t, 2)]);
        } else {
            standard = false; // (non-standard date)
        }
        if standard {
            let sec = if sec.truthy() { sec } else { pl::s(":00") };
            return ret(pl::sprintf("%.4d:%.2d:%.2d %.2d:%.2d%s%s", &[yr, mon, day, hr, min, sec, tz]));
        }
    }
    if (opt(it, &et, "StrictDate")?.truthy() && !et.get("TAGS_FROM_FILE").truthy()) || opt(it, &et, "Validate")?.truthy() {
        m(it, &et, "Warn", vec![pl::s("Non standard PNG date/time format"), n(1)])?;
    }
    ret(val)
}

/// Convert EXIF date/time to PNG format
/// Inputs: 0) Date/time in EXIF format, 1) ExifTool ref
/// Returns: PNG formatted date/time string
pub fn inverse_png_date(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (mut val, et) = (arg(&a, 0), arg(&a, 1));
    if opt(it, &et, "StrictDate")?.truthy() {
        let mut err = false;
        if let Some(c) = re(r"^(\d{4}):(\d{2}):(\d{2}) (\d{2})(:\d{2})(:\d{2})?(?:\.\d*)?\s*(\S*)", "").m(&val) {
            let (yr, mut mon, day, hr, min, mut sec, mut tz) = (cap(&c, 1), cap(&c, 2), cap(&c, 3), cap(&c, 4), cap(&c, 5), cap(&c, 6), cap(&c, 7));
            if !sec.truthy() {
                sec = pl::s("");
            }
            let month_num = crate::runtime::lex_hash(it, MODULE, "monthNum");
            let mon_name = Value::new_hash();
            for k in month_num.keys() {
                mon_name.set(month_num.get(&k).to_bytes(), Value::bytes(&k[..]));
            }
            mon = mon_name.get(pl::add(&mon, &n(0)).to_bytes());
            if !mon.truthy() {
                err = true;
            }
            if pl::length(&tz).truthy() {
                if !re(r"^(Z|[-+]\d{2}:?\d{2})", "").is_match(&tz) {
                    err = true;
                }
                pl::tr(&mut tz, ":", "", "d");
                tz = pl::concat(&pl::s(" "), &tz);
            }
            if !err {
                val = cat(&[&day, &pl::s(" "), &mon, &pl::s(" "), &yr, &pl::s(" "), &hr, &min, &sec, &tz]);
            }
        }
        if err {
            pl::warn(it, "Invalid date/time (use YYYY:mm:dd HH:MM:SS[.ss][+/-HH:MM|Z])\n")?;
            val = Value::Undef;
        }
    }
    ret(val)
}

/// Get localized version of tagInfo hash
/// Inputs: 0) tagInfo hash ref, 1) language code (eg. "x-default")
/// Returns: new tagInfo hash ref, or undef if invalid
pub fn get_lang_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (tag_info, mut lang) = (arg(&a, 0), arg(&a, 1));
    pl::tr(&mut lang, "_", "-", ""); // RFC 3066 specifies '-' as a separator
    // no alternate languages for XMP or raw profile directories
    if tag_info.get("SubDirectory").truthy() {
        return pl::ret_undef();
    }
    // language code must normalized for use in tag ID
    let l = standard_lang_case1(it, lang)?;
    ret(call(it, "GetLangInfo", vec![tag_info, l])?)
}

/// Found a PNG tag -- extract info from subdirectory or decompress data if necessary
/// Inputs: 0) ExifTool object reference, 1) Pointer to tag table,
///         2) Tag ID, 3) Tag value, 4) [optional] compressed data flag:
///            0=not compressed, 1=unknown compression, 2-N=compression with type N-2
///         5) optional output buffer ref, 6) character encoding (tEXt/zTXt/iTXt only)
///         6) optional language code
/// Returns: 1 on success
pub fn found_png(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, tag_table_ptr, tag, mut val, mut compressed, out_buff, enc, mut lang) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4), arg(&a, 5), arg(&a, 6), arg(&a, 7));
    if val.is_undef() {
        return ret(n(0));
    }
    let verbose = opt(it, &et, "Verbose")?;
    let mut id = tag.clone(); // generate tag ID which includes language code
    if lang.truthy() {
        // case of language code must be normalized since they are case insensitive
        lang = standard_lang_case1(it, lang)?;
        id = cat(&[&id, &pl::s("-"), &lang]);
    }
    let mut tag_info = m(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), id.clone()])?;
    if !tag_info.truthy() {
        // (some software forgets to capitalize first letter)
        tag_info = m(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), pl::ucfirst(&id)])?;
    }
    // create alternate language tag if necessary
    if !tag_info.truthy() && lang.truthy() {
        tag_info = m(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
        if !tag_info.truthy() {
            tag_info = m(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), pl::ucfirst(&tag)])?;
        }
        if tag_info.truthy() {
            tag_info = get_lang_info(it, vec![tag_info, lang.clone()], Want::Scalar)?.pop().unwrap_or_default();
        }
    }
    //
    // uncompress data if necessary
    //
    let (mut was_compressed, mut deflate_err) = (Value::Undef, Value::Undef);
    let no_compress_lib = crate::runtime::lex_scalar(it, MODULE, "noCompressLib");
    if compressed.truthy() && pl::gt(&compressed, &n(1)) {
        if pl::num_eq(&compressed, &n(2)) {
            // Inflate/Deflate compression
            if pl::eval_block(it, |it| crate::runtime::require(it, "Compress::Zlib"))?.is_some() {
                let (mut v2, mut stat) = (Value::Undef, Value::Undef);
                let inflate = pl::call(it, "Compress::Zlib::inflateInit", vec![])?;
                if inflate.truthy() {
                    let r = pl::method_list(it, &inflate, "inflate", vec![val.clone()])?;
                    v2 = arg(&r, 0);
                    stat = arg(&r, 1);
                }
                if inflate.truthy() && pl::num_eq(&stat, &pl::call(it, "Compress::Zlib::Z_STREAM_END", vec![])?) {
                    val = v2;
                    compressed = n(0);
                    was_compressed = n(1);
                } else {
                    deflate_err = cat(&[&pl::s("Error inflating "), &tag]);
                }
            } else if !no_compress_lib.borrow().truthy() {
                deflate_err = pl::s("Install Compress::Zlib to read compressed information");
            } else {
                deflate_err = pl::s(""); // flag deflate error but no warning
            }
        } else {
            compressed = pl::sub(&compressed, &n(2));
            deflate_err = cat(&[&pl::s("Unknown compression method "), &compressed, &pl::s(" for "), &tag]);
        }
        if compressed.truthy() && verbose.truthy() && tag_info.truthy() && tag_info.get("SubDirectory").truthy() {
            let name = cat(&[&pl::s("Unable to decompress "), &tag_info.get("Name")]);
            m(it, &et, "VerboseDir", vec![name, n(0), pl::length(&val)])?;
        }
        // issue warning if relevant
        if deflate_err.truthy() && !out_buff.truthy() {
            m(it, &et, "Warn", vec![deflate_err.clone()])?;
            if re(r"^Install", "").is_match(&deflate_err) {
                *no_compress_lib.borrow_mut() = n(1);
            }
        }
    }
    // translate character encoding if necessary (tEXt/zTXt/iTXt string values only)
    if enc.truthy() && !compressed.truthy() && !(tag_info.truthy() && tag_info.get("SubDirectory").truthy()) {
        val = m(it, &et, "Decode", vec![val, enc.clone()])?;
    }
    //
    // extract information from subdirectory if available
    //
    if tag_info.truthy() {
        let tag_name = tag_info.get("Name");
        let mut processed = Value::Undef;
        if tag_info.get("SubDirectory").truthy() {
            if et.path(&["OPTIONS", "Validate"]).truthy() && tag_info.get("NonStandard").truthy() {
                let msg = cat(&[&pl::s("Non-standard "), &tag_info.get("NonStandard"), &pl::s(" in PNG "), &tag, &pl::s(" chunk")]);
                m(it, &et, "Warn", vec![msg, n(1)])?;
            }
            let subdir = tag_info.get("SubDirectory");
            let dn = subdir.get("DirName");
            let dir_name = if dn.truthy() { dn } else { tag_name.clone() };
            if !compressed.truthy() {
                let len = pl::length(&val);
                if verbose.truthy() && !et.get("INDENT").eq_str("  ") {
                    if was_compressed.truthy() && pl::gt(&verbose, &n(2)) {
                        let mut name = tag_name.clone();
                        if was_compressed.truthy() {
                            name = cat(&[&pl::s("Decompressed "), &name]);
                        }
                        m(it, &et, "VerboseDir", vec![name, n(0), len.clone()])?;
                        m(it, &et, "VerboseDump", vec![pl::sref(val.clone())])?;
                    }
                    // don't indent next directory (since it is really the same data)
                    unindent(&et);
                }
                let mut process_proc = subdir.get("ProcessProc");
                // nothing more to do if writing and subdirectory is not writable
                let sub_table = call(it, "GetTagTable", vec![subdir.get("TagTable")])?;
                if out_buff.truthy() && !sub_table.get("WRITE_PROC").truthy() {
                    if et.get("DEL_GROUP").get(dir_name.to_bytes()).truthy() {
                        // non-writable directories may be deleted as a group (eg. SEAL)
                        m(it, &et, "VPrint", vec![n(0), cat(&[&pl::s("  Deleting "), &dir_name, &pl::s("\n")])])?;
                        pl::set_deref(&out_buff, pl::s(""))?;
                        inc_changed(&et);
                    }
                    return ret(n(1));
                }
                let val_ref = pl::sref(val.clone());
                let subdir_info = Value::new_hash();
                subdir_info.set("DataPt", val_ref.clone());
                subdir_info.set("DirStart", n(0));
                subdir_info.set("DataLen", len.clone());
                subdir_info.set("DirLen", len.clone());
                subdir_info.set("DirName", dir_name.clone());
                subdir_info.set("TagInfo", tag_info.clone());
                subdir_info.set("ReadOnly", n(1)); // (used only by WriteXMP)
                subdir_info.set("OutBuff", out_buff.clone());
                subdir_info.set("IgnoreProp", subdir.get("IgnoreProp")); // (XML hack for meTa chunk)
                // no need to re-decompress if already done
                let compressed_proc = code_ref(it, "Image::ExifTool::PNG::ProcessPNG_Compressed");
                if was_compressed.truthy() && process_proc.truthy() && process_proc.same_ref(&compressed_proc) {
                    process_proc = Value::Undef;
                }
                // rewrite this directory if necessary (but always process TextualData normally)
                let textual = pl::pkg_hash(it, "Image::ExifTool::PNG::TextualData");
                if out_buff.truthy() && !process_proc.truthy() && !sub_table.same_ref(&textual) {
                    if !et.get("EDIT_DIRS").get(dir_name.to_bytes()).truthy() {
                        return ret(n(1));
                    }
                    let w = m(it, &et, "WriteDirectory", vec![subdir_info.clone(), sub_table.clone()])?;
                    pl::set_deref(&out_buff, w)?;
                    if tag_name.eq_str("XMP") && out_buff.deref().truthy() {
                        // make sure the XMP is marked as read-only
                        pl::call(it, "Image::ExifTool::XMP::ValidateXMP", vec![out_buff.clone(), pl::s("r")])?;
                    }
                    png_call(it, "DoneDir", vec![et.clone(), dir_name.clone(), out_buff.clone(), tag_info.get("NonStandard")])?;
                } else {
                    processed = m(it, &et, "ProcessDirectory", vec![subdir_info.clone(), sub_table.clone(), process_proc])?;
                }
                val = val_ref.deref();
                compressed = n(1); // pretend this is compressed since it is binary data
            } else if out_buff.truthy() {
                let del_group = et.get("DEL_GROUP");
                if del_group.get(dir_name.to_bytes()).truthy() || (dir_name.eq_str("EXIF") && del_group.get("IFD0").truthy()) {
                    pl::set_deref(&out_buff, pl::s(""))?;
                    inc_changed(&et);
                    m(it, &et, "VPrint", vec![n(0), cat(&[&pl::s("  Deleting "), &tag, &pl::s(" chunk")])])?;
                } else {
                    let edit_dirs = et.get("EDIT_DIRS");
                    if edit_dirs.get(dir_name.to_bytes()).truthy() || (dir_name.eq_str("EXIF") && edit_dirs.get("IFD0").truthy()) {
                        m(it, &et, "Warn", vec![cat(&[&pl::s("Can't write "), &dir_name, &pl::s(". Requires Compress::Zlib")])])?;
                    }
                    // pretend we did this directory so we don't try to recreate it
                    png_call(it, "DoneDir", vec![et.clone(), dir_name.clone(), out_buff.clone(), tag_info.get("NonStandard")])?;
                }
            }
        }
        if out_buff.truthy() {
            let writable = tag_info.get("Writable");
            let mut is_overwriting;
            if writable.truthy() || (tag_table_ptr.get("WRITABLE").truthy() && writable.is_undef() && !tag_info.get("SubDirectory").truthy()) {
                // write new value for this tag if necessary
                let mut new_val = Value::Undef;
                if et.get("DEL_GROUP").get("PNG").truthy() {
                    // remove this tag now, but keep in ADD_PNG list to add back later
                    is_overwriting = n(1);
                } else {
                    // remove this from the list of PNG tags to add
                    let add_png = et.get_or_new_hash("ADD_PNG");
                    add_png.delete(id.to_bytes());
                    // (also handle case of tEXt tags written with lowercase first letter)
                    add_png.delete(pl::ucfirst(&id).to_bytes());
                    let nv_hash = m(it, &et, "GetNewValueHash", vec![tag_info.clone()])?;
                    is_overwriting = m(it, &et, "IsOverwriting", vec![nv_hash.clone()])?;
                    if !deflate_err.is_undef() {
                        new_val = m(it, &et, "GetNewValue", vec![nv_hash.clone()])?;
                        // can only write tag now if always overwriting
                        if pl::gt(&is_overwriting, &n(0)) {
                            val = pl::s("<deflate error>");
                        } else if is_overwriting.truthy() {
                            is_overwriting = n(0); // can't overwrite
                            if deflate_err.truthy() {
                                m(it, &et, "Warn", vec![deflate_err.clone()])?;
                            }
                        }
                    } else {
                        if pl::lt(&is_overwriting, &n(0)) {
                            is_overwriting = m(it, &et, "IsOverwriting", vec![nv_hash.clone(), val.clone()])?;
                        }
                        // (must get new value after IsOverwriting() in case it was shifted)
                        new_val = m(it, &et, "GetNewValue", vec![nv_hash.clone()])?;
                    }
                }
                if is_overwriting.truthy() {
                    pl::set_deref(&out_buff, if !new_val.is_undef() { new_val.clone() } else { pl::s("") })?;
                    inc_changed(&et);
                    m(it, &et, "VerboseValue", vec![cat(&[&pl::s("- PNG:"), &tag_name]), val.clone()])?;
                    if !new_val.is_undef() {
                        m(it, &et, "VerboseValue", vec![cat(&[&pl::s("+ PNG:"), &tag_name]), new_val.clone()])?;
                    }
                }
            }
            let ob = out_buff.deref();
            if !ob.is_undef() && pl::length(&ob).truthy() {
                if enc.truthy() {
                    // must be tEXt/zTXt/iTXt if $enc is set
                    let r = png_call(it, "BuildTextChunk", vec![et.clone(), tag.clone(), tag_info.clone(), ob, lang.clone()])?;
                    pl::set_deref(&out_buff, r)?;
                } else if was_compressed.truthy() {
                    // re-compress the output data
                    let len = pl::length(&ob);
                    let deflate = pl::call(it, "Compress::Zlib::deflateInit", vec![])?;
                    if deflate.truthy() {
                        let d = m(it, &deflate, "deflate", vec![ob.clone()])?;
                        pl::set_deref(&out_buff, d.clone())?;
                        if !d.is_undef() {
                            let f = m(it, &deflate, "flush", vec![])?;
                            pl::set_deref(&out_buff, pl::concat(&d, &f))?;
                        }
                    } else {
                        pl::set_deref(&out_buff, Value::Undef)?;
                    }
                    if !out_buff.deref().truthy() {
                        m(it, &et, "Warn", vec![cat(&[&pl::s("PNG:"), &tag_name, &pl::s(" not written (compress error)")])])?;
                    } else if pl::lc(&tag).eq_str("zxif") {
                        let hdr = pl::pack("N", &[len])?;
                        pl::set_deref(&out_buff, cat(&[&Value::bytes(b"\0"), &hdr, &out_buff.deref()]))?; // add zXIf header
                    }
                }
            }
            return ret(n(1));
        }
        if processed.truthy() {
            return ret(n(1));
        }
    } else if out_buff.truthy() {
        let textual = pl::pkg_hash(it, "Image::ExifTool::PNG::TextualData");
        if et.get("DEL_GROUP").get("PNG").truthy() && tag_table_ptr.same_ref(&textual) {
            // delete all TextualData tags if deleting the PNG group
            pl::set_deref(&out_buff, pl::s(""))?;
            inc_changed(&et);
            m(it, &et, "VerboseValue", vec![cat(&[&pl::s("- PNG:"), &tag]), val.clone()])?;
        }
        return ret(n(1));
    } else {
        let mut name = tag.clone();
        re(r"\s+(.)", "").subst(&mut name, true, |c| pl::ucfirst(&cap(c, 1)).to_bytes().to_vec()); // remove white space from tag name
        tag_info = Value::new_hash();
        tag_info.set("Name", name);
        if lang.truthy() {
            tag_info.set("LangCode", lang.clone());
        }
        // make unknown profiles binary data type
        if re(r"^Raw profile type ", "").is_match(&tag) {
            tag_info.set("Binary", n(1));
        }
        if verbose.truthy() {
            m(it, &et, "VPrint", vec![n(0), cat(&[&pl::s("  [adding "), &tag, &pl::s("]\n")])])?;
        }
        call(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info.clone()])?;
    }
    //
    // store this tag information
    //
    if verbose.truthy() {
        // temporarily remove subdirectory so it isn't printed in verbose information
        // since we aren't decoding it anyway;
        let subdir = tag_info.get("SubDirectory");
        tag_info.delete("SubDirectory");
        m(it, &et, "VerboseInfo", vec![tag.clone(), tag_info.clone(), pl::s("Table"), tag_table_ptr.clone(), pl::s("DataPt"), pl::sref(val.clone())])?;
        if subdir.truthy() {
            tag_info.set("SubDirectory", subdir);
        }
    }
    // set the RawConv dynamically depending on whether this is binary or not
    let mut del_raw_conv = false;
    if compressed.truthy() && tag_info.get("ValueConv").is_undef() {
        tag_info.set("RawConv", pl::s("\\$val"));
        del_raw_conv = true;
    }
    m(it, &et, "FoundTag", vec![tag_info.clone(), val])?;
    if del_raw_conv {
        tag_info.delete("RawConv");
    }
    ret(n(1))
}

fn found_png1(it: &mut Interp, args: Vec<Value>) -> ER<Value> {
    Ok(found_png(it, args, Want::Scalar)?.pop().unwrap_or_default())
}

/// Process encoded PNG profile information
/// Inputs: 0) ExifTool object reference, 1) DirInfo reference, 2) Pointer to tag table
/// Returns: 1 on success
pub fn process_profile(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, mut tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let tag_info = dir_info.get("TagInfo");
    let out_buff = dir_info.get("OutBuff");
    let tag_name = tag_info.get("Name");

    // ImageMagick 5.3.6 writes profiles with the following headers:
    // "\nICC Profile\n", "\nIPTC profile\n", "\n\xaa\x01{generic prof\n"
    // and "\ngeneric profile\n"
    let Some(c) = re(r"^\n(.*?)\n\s*(\d+)\n(.*)", "s").m(&data_pt.deref()) else {
        return ret(n(0));
    };
    let (profile_type, mut len) = (cap(&c, 1), cap(&c, 2));
    // data is encoded in hex, so change back to binary
    let hex = pl::join("", &pl::split_ws(&cap(&c, 3)));
    let buff = pl::pack("H*", &[hex])?;
    let actual_len = pl::length(&buff);
    if len.to_bytes() != actual_len.to_bytes() {
        let msg = cat(&[&tag_name, &pl::s(" is wrong size (should be "), &len, &pl::s(" bytes but is "), &actual_len, &pl::s(")")]);
        m(it, &et, "Warn", vec![msg])?;
        len = actual_len;
    }
    let verbose = opt(it, &et, "Verbose")?;
    let buff_ref = pl::sref(buff.clone());
    if verbose.truthy() {
        if pl::gt(&verbose, &n(2)) {
            m(it, &et, "VerboseDir", vec![cat(&[&pl::s("Decoded "), &tag_name]), n(0), len.clone()])?;
            m(it, &et, "VerboseDump", vec![buff_ref.clone()])?;
        }
        // don't indent next directory (since it is really the same data)
        unindent(&et);
    }
    let dir_info2 = Value::new_hash();
    dir_info2.set("Parent", pl::s("PNG"));
    dir_info2.set("DataPt", buff_ref.clone());
    dir_info2.set("DataLen", len.clone());
    dir_info2.set("DirStart", n(0));
    dir_info2.set("DirLen", len.clone());
    dir_info2.set("Base", n(0));
    dir_info2.set("OutFile", out_buff.clone());
    et.set("PROCESSED", Value::new_hash()); // reset processed directory offsets
    let mut processed = n(0);
    let old_changed = et.get("CHANGED");
    let exif_table = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Exif::Main")])?;
    let edit_dirs = et.get("EDIT_DIRS");
    let exif_hdr = it.get_scalar("Image::ExifTool::exifAPP1hdr");
    let xmp_hdr = it.get_scalar("Image::ExifTool::xmpAPP1hdr");
    let starts = |hdr: &Value, b: &Value| -> ER<bool> {
        let mut p = b"^".to_vec();
        p.extend_from_slice(&hdr.to_bytes());
        Ok(pl::re_dyn(&p, "")?.is_match(b))
    };

    if !tag_table_ptr.same_ref(&exif_table) {
        // this is unfortunate, but the "IPTC" profile may be stored as either
        // IPTC IIM or a Photoshop IRB resource, so we must test for this
        if tag_name.eq_str("IPTC_Profile") && re(r"^\x1c", "").is_match(&buff_ref.deref()) {
            tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::IPTC::Main")])?;
        }
        // process non-EXIF and non-APP1 profile as-is
        if out_buff.truthy() {
            // no need to rewrite this if not editing tags in this directory
            let mut dir = tag_name.clone();
            if !re(r"^ICC", "").is_match(&dir) {
                re(r"_Profile", "").subst(&mut dir, false, |_| Vec::new());
            }
            if !edit_dirs.get(dir.to_bytes()).truthy() {
                return ret(n(1));
            }
            let w = m(it, &et, "WriteDirectory", vec![dir_info2.clone(), tag_table_ptr.clone()])?;
            pl::set_deref(&out_buff, w)?;
            png_call(it, "DoneDir", vec![et.clone(), dir, out_buff.clone(), tag_info.get("NonStandard")])?;
        } else {
            processed = m(it, &et, "ProcessDirectory", vec![dir_info2.clone(), tag_table_ptr.clone()])?;
        }
    } else if starts(&exif_hdr, &buff_ref.deref())? {
        // APP1 EXIF information
        if out_buff.truthy() && !edit_dirs.get("IFD0").truthy() {
            return ret(n(1));
        }
        let hdr_len = pl::length(&exif_hdr);
        dir_info2.set("DirStart", pl::add(&dir_info2.get("DirStart"), &hdr_len));
        dir_info2.set("DirLen", pl::sub(&dir_info2.get("DirLen"), &hdr_len));
        if out_buff.truthy() {
            // delete non-standard EXIF if recreating from scratch
            if et.get("DEL_GROUP").get("EXIF").truthy() || et.get("DEL_GROUP").get("IFD0").truthy() {
                pl::set_deref(&out_buff, pl::s(""))?;
                m(it, &et, "VPrint", vec![n(0), pl::s("  Deleting non-standard APP1 EXIF information")])?;
                return ret(n(1));
            }
            let wt = code_ref(it, "Image::ExifTool::WriteTIFF");
            let w = m(it, &et, "WriteDirectory", vec![dir_info2.clone(), tag_table_ptr.clone(), wt])?;
            pl::set_deref(&out_buff, w)?;
            if out_buff.deref().truthy() {
                pl::set_deref(&out_buff, pl::concat(&exif_hdr, &out_buff.deref()))?;
            }
            png_call(it, "DoneDir", vec![et.clone(), pl::s("IFD0"), out_buff.clone(), tag_info.get("NonStandard")])?;
        } else {
            processed = m(it, &et, "ProcessTIFF", vec![dir_info2.clone()])?;
        }
    } else if starts(&xmp_hdr, &buff_ref.deref())? {
        // APP1 XMP information
        let hdr_len = pl::length(&xmp_hdr);
        let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::XMP::Main")])?;
        dir_info2.set("DirStart", pl::add(&dir_info2.get("DirStart"), &hdr_len));
        dir_info2.set("DirLen", pl::sub(&dir_info2.get("DirLen"), &hdr_len));
        if out_buff.truthy() {
            if !edit_dirs.get("XMP").truthy() {
                return ret(n(1));
            }
            let w = m(it, &et, "WriteDirectory", vec![dir_info2.clone(), tag_table_ptr.clone()])?;
            pl::set_deref(&out_buff, w)?;
            if out_buff.deref().truthy() {
                pl::set_deref(&out_buff, pl::concat(&xmp_hdr, &out_buff.deref()))?;
            }
            png_call(it, "DoneDir", vec![et.clone(), pl::s("XMP"), out_buff.clone(), tag_info.get("NonStandard")])?;
        } else {
            processed = m(it, &et, "ProcessDirectory", vec![dir_info2.clone(), tag_table_ptr.clone()])?;
        }
    } else if re(r"^(MM\0\x2a|II\x2a\0)", "").is_match(&buff_ref.deref()) {
        // TIFF information
        if out_buff.truthy() && !edit_dirs.get("IFD0").truthy() {
            return ret(n(1));
        }
        if out_buff.truthy() {
            // delete non-standard EXIF if recreating from scratch
            if et.get("DEL_GROUP").get("EXIF").truthy() || et.get("DEL_GROUP").get("IFD0").truthy() {
                pl::set_deref(&out_buff, pl::s(""))?;
                m(it, &et, "VPrint", vec![n(0), pl::s("  Deleting non-standard EXIF/TIFF information")])?;
                return ret(n(1));
            }
            let wt = code_ref(it, "Image::ExifTool::WriteTIFF");
            let w = m(it, &et, "WriteDirectory", vec![dir_info2.clone(), tag_table_ptr.clone(), wt])?;
            pl::set_deref(&out_buff, w)?;
            png_call(it, "DoneDir", vec![et.clone(), pl::s("IFD0"), out_buff.clone(), tag_info.get("NonStandard")])?;
        } else {
            processed = m(it, &et, "ProcessTIFF", vec![dir_info2.clone()])?;
        }
    } else {
        let mut prof_name = profile_type.clone();
        pl::tr(&mut prof_name, r"\x00-\x1f\x7f-\xff", ".", "");
        m(it, &et, "Warn", vec![cat(&[&pl::s("Unknown raw profile '"), &prof_name, &pl::s("'")])])?;
    }
    if out_buff.truthy() && !out_buff.deref().is_undef() && pl::length(&out_buff.deref()).truthy() {
        if !pl::num_eq(&et.get("CHANGED"), &old_changed) {
            let hdr = pl::sprintf("\n%s\n%8d\n", &[profile_type.clone(), pl::length(&out_buff.deref())]);
            // hex encode the data
            let h = png_call(it, "HexEncode", vec![out_buff.clone()])?;
            pl::set_deref(&out_buff, pl::concat(&hdr, &h))?;
        } else {
            pl::set_deref(&out_buff, Value::Undef)?;
        }
    }
    ret(processed)
}

/// Process PNG compressed zTXt or iCCP chunk
/// Inputs: 0) ExifTool object reference, 1) DirInfo reference, 2) Pointer to tag table
/// Returns: 1 on success
/// Notes: writes new chunk data to ${$$dirInfo{OutBuff}} if writing tag
pub fn process_png_compressed(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, mut tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let parts = split0(&dir_info.get("DataPt").deref(), 2);
    let (tag, mut val) = (arg(&parts, 0), arg(&parts, 1));
    if val.is_undef() {
        return ret(n(0));
    }
    // set compressed to 2 + compression method to decompress the data
    let compressed = pl::add(&n(2), &pl::unpack1("C", &val)?);
    let mut hdr = cat(&[&tag, &Value::bytes(b"\0"), &pl::substr(&val, 0, Some(1))]);
    val = pl::substr(&val, 1, None); // remove compression method byte
    let success;
    let out_buff = dir_info.get("OutBuff");
    let tag_info = dir_info.get("TagInfo");
    // use the PNG chunk tag instead of the embedded tag name for iCCP chunks
    if tag_info.truthy() && tag_info.get("Name").eq_str("ICC_Profile") {
        m(it, &et, "VerboseDir", vec![pl::s("iCCP")])?;
        tag_table_ptr = pl::pkg_hash(it, "Image::ExifTool::PNG::Main");
        if pl::length(&tag).truthy() && !out_buff.truthy() {
            found_png1(it, vec![et.clone(), tag_table_ptr.clone(), pl::s("iCCP-name"), tag.clone()])?;
        }
        success = found_png1(it, vec![et.clone(), tag_table_ptr.clone(), pl::s("iCCP"), val.clone(), compressed, out_buff.clone()])?;
        if out_buff.truthy() && out_buff.deref().truthy() {
            let main = pl::pkg_hash(it, "Image::ExifTool::PNG::Main");
            let profile_name = m(it, &et, "GetNewValue", vec![main.get("iCCP-name")])?;
            if !profile_name.is_undef() {
                hdr = pl::concat(&profile_name, &pl::substr(&hdr, pl::length(&tag).to_i64(), None));
                m(it, &et, "VerboseValue", vec![pl::s("+ PNG:ProfileName"), profile_name.clone()])?;
            }
            pl::set_deref(&out_buff, pl::concat(&hdr, &out_buff.deref()))?;
        }
    } else {
        success = found_png1(it, vec![et.clone(), tag_table_ptr.clone(), tag.clone(), val.clone(), compressed, out_buff.clone(), pl::s("Latin")])?;
    }
    ret(success)
}

/// Process PNG tEXt chunk
/// Inputs: 0) ExifTool object reference, 1) DirInfo reference, 2) Pointer to tag table
/// Returns: 1 on success
/// Notes: writes new chunk data to ${$$dirInfo{OutBuff}} if writing tag
pub fn process_png_text(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let parts = split0(&dir_info.get("DataPt").deref(), 2);
    let (tag, val) = (arg(&parts, 0), arg(&parts, 1));
    let out_buff = dir_info.get("OutBuff");
    if et.path(&["OPTIONS", "Verbose"]).truthy() {
        et.set("INDENT", pl::substr(&et.get("INDENT"), 0, Some(-2)));
    }
    ret(found_png1(it, vec![et, tag_table_ptr, tag, val, Value::Undef, out_buff, pl::s("Latin")])?)
}

/// Process PNG iTXt chunk
/// Inputs: 0) ExifTool object reference, 1) DirInfo reference, 2) Pointer to tag table
/// Returns: 1 on success
/// Notes: writes new chunk data to ${$$dirInfo{OutBuff}} if writing tag
pub fn process_png_itxt(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let parts = split0(&dir_info.get("DataPt").deref(), 2);
    let (tag, dat) = (arg(&parts, 0), arg(&parts, 1));
    if !(!dat.is_undef() && pl::ge(&pl::length(&dat), &n(4))) {
        return ret(n(0));
    }
    let cm = pl::unpack("CC", &dat)?;
    let (mut compressed, meth) = (arg(&cm, 0), arg(&cm, 1));
    let p = split0(&pl::substr(&dat, 2, None), 3);
    let (lang, _trans, val) = (arg(&p, 0), arg(&p, 1), arg(&p, 2));
    // set compressed flag so we will decompress it in FoundPNG()
    if compressed.truthy() {
        compressed = pl::add(&n(2), &meth);
    }
    let out_buff = dir_info.get("OutBuff");
    if et.path(&["OPTIONS", "Verbose"]).truthy() {
        et.set("INDENT", pl::substr(&et.get("INDENT"), 0, Some(-2)));
    }
    ret(found_png1(it, vec![et, tag_table_ptr, tag, val, compressed, out_buff, pl::s("UTF8"), lang])?)
}

/// Process PNG eXIf/zXIf chunk
/// Inputs: 0) ExifTool object reference, 1) DirInfo reference, 2) Pointer to tag table
/// Returns: 1 on success
/// Notes: writes new chunk data to ${$$dirInfo{OutBuff}} if writing tag
pub fn process_png_exif(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, mut tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let out_buff = dir_info.get("OutBuff");
    let data_pt = dir_info.get("DataPt");
    let tag_info = dir_info.get("TagInfo");
    let tag = tag_info.get("TagID");
    let del = if out_buff.truthy() { Value::from(et.get("DEL_GROUP").get("EXIF").truthy() || et.get("DEL_GROUP").get("IFD0").truthy()) } else { out_buff.clone() };

    if re(r"^Exif\0\0", "").is_match(&data_pt.deref()) {
        m(it, &et, "Warn", vec![pl::s("Improper \"Exif00\" header in EXIF chunk")])?;
        pl::set_deref(&data_pt, pl::substr(&data_pt.deref(), 6, None))?;
        dir_info.set("DataLen", pl::length(&data_pt.deref()));
        if dir_info.get("DirLen").truthy() {
            dir_info.set("DirLen", pl::sub(&dir_info.get("DirLen"), &n(6)));
        }
    }
    let type_;
    if let Some(c) = re(r"^(\0|II|MM)", "").m(&data_pt.deref()) {
        type_ = cap(&c, 1);
    } else if del.truthy() {
        m(it, &et, "VPrint", vec![n(0), cat(&[&pl::s("  Deleting invalid "), &tag, &pl::s(" chunk")])])?;
        pl::set_deref(&out_buff, pl::s(""))?;
        inc_changed(&et);
        return ret(n(1));
    } else {
        m(it, &et, "Warn", vec![cat(&[&pl::s("Invalid "), &tag, &pl::s(" chunk")])])?;
        return ret(n(0));
    }
    if type_.eq_str("\0") {
        // is this compressed EXIF?
        let buf = pl::substr(&data_pt.deref(), 5, None);
        // go around again to uncompress the data
        tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::PNG::Main")])?;
        return ret(found_png1(it, vec![et, tag_table_ptr, tag_info.get("TagID"), pl::sref(buf), n(2), out_buff])?);
    } else if !out_buff.truthy() {
        return ret(m(it, &et, "ProcessTIFF", vec![dir_info])?);
    // (zxIf was not adopted)
    //} elsif ($del and ($et->Options('Compress') xor lc($tag) eq 'zxif')) {
    } else if del.truthy() && pl::lc(&tag).eq_str("zxif") {
        m(it, &et, "VPrint", vec![n(0), cat(&[&pl::s("  Deleting "), &tag, &pl::s(" chunk")])])?;
        pl::set_deref(&out_buff, pl::s(""))?;
        inc_changed(&et);
    } else if et.get("EDIT_DIRS").get("IFD0").truthy() {
        let wt = code_ref(it, "Image::ExifTool::WriteTIFF");
        let w = m(it, &et, "WriteDirectory", vec![dir_info.clone(), tag_table_ptr.clone(), wt])?;
        pl::set_deref(&out_buff, w)?;
        png_call(it, "DoneDir", vec![et.clone(), pl::s("IFD0"), out_buff.clone(), tag_info.get("NonStandard")])?;
    }
    ret(n(1))
}

/// `CalculateCRC(\$buff, $crc, $start)` (from WritePNG.pl)
fn crc(it: &mut Interp, buff: &Value, crc: Value, start: Value) -> ER<Value> {
    let mut args = vec![pl::sref(buff.clone()), crc];
    if !start.is_undef() {
        args.push(start);
    }
    png_call(it, "CalculateCRC", args)
}

/// Extract meta information from a PNG image
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid PNG image, or -1 on write error
pub fn process_png(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let outfile = dir_info.get("OutFile");
    let raf = dir_info.get("RAF");
    let mut dat_chunk = pl::s("");
    let mut dat_count = n(0);
    let mut dat_bytes = n(0);
    let fast_scan = opt(it, &et, "FastScan")?;
    let hash = et.get("ImageDataHash");
    let mut nn;
    let (mut sig, mut err, mut hbuf, mut dbuf, mut cbuf) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    let (mut was_hdr, mut was_end, mut was_dat, mut do_txt, mut was_trailer) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    let mut txt_offset: Vec<Value> = Vec::new();
    let png_lookup = crate::runtime::lex_hash(it, MODULE, "pngLookup");
    let png_map = crate::runtime::lex_hash(it, MODULE, "pngMap");
    let is_dat_chunk = crate::runtime::lex_hash(it, MODULE, "isDatChunk");
    let is_txt_chunk = crate::runtime::lex_hash(it, MODULE, "isTxtChunk");
    let no_leap_frog = crate::runtime::lex_hash(it, MODULE, "noLeapFrog");
    let no_compress_lib: Cell = crate::runtime::lex_scalar(it, MODULE, "noCompressLib");
    let std_case = pl::pkg_hash(it, "Image::ExifTool::PNG::stdCase");

    // check to be sure this is a valid PNG/MNG/JNG image
    if !(pl::num_eq(&read(it, &raf, &mut sig, n(8))?, &n(8)) && png_lookup.get(sig.to_bytes()).truthy()) {
        return ret(n(0));
    }

    if outfile.truthy() {
        et.delete("TextChunkType");
        if outfile.truthy() && !write(it, &outfile, &[sig.clone()])? {
            err = n(1);
        }
        // can only add tags in Main and TextualData tables
        let main = pl::pkg_hash(it, "Image::ExifTool::PNG::Main");
        let textual = pl::pkg_hash(it, "Image::ExifTool::PNG::TextualData");
        let add = m(it, &et, "GetNewTagInfoHash", vec![main, textual])?;
        et.set("ADD_PNG", add);
        // initialize with same directories, with PNG tags taking priority
        m(it, &et, "InitWriteDirs", vec![png_map.clone(), pl::s("PNG")])?;
    } else {
        // disable buffering in FastScan mode
        if fast_scan.truthy() {
            raf.set("NoBuffer", n(1));
        }
    }
    let l = png_lookup.get(sig.to_bytes());
    let (file_type, hdr_chunk, end_chunk) = (l.at(0), l.at(1), l.at(2));
    m(it, &et, "SetFileType", vec![file_type.clone()])?; // set the FileType tag
    call(it, "SetByteOrder", vec![pl::s("MM")])?; // PNG files are big-endian
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::PNG::Main")])?;
    let mut mng_table_ptr = Value::Undef;
    if !file_type.eq_str("PNG") {
        mng_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::MNG::Main")])?;
    }
    let verbose = opt(it, &et, "Verbose")?;
    let validate = opt(it, &et, "Validate")?;
    let out = opt(it, &et, "TextOut")?;

    // scan ahead to find offsets of all text chunks after IDAT
    if outfile.truthy() {
        while pl::num_eq(&read(it, &raf, &mut hbuf, n(8))?, &n(8)) {
            let u = pl::unpack("Na4", &hbuf)?;
            let (len, chunk) = (arg(&u, 0), arg(&u, 1));
            if pl::gt(&len, &n(0x7fffffff)) {
                break;
            }
            if was_dat.truthy() {
                if no_leap_frog.get(chunk.to_bytes()).truthy() {
                    break; // (don't move text across these chunks)
                }
                if is_txt_chunk.get(chunk.to_bytes()).truthy() {
                    let t = m(it, &raf, "Tell", vec![])?;
                    txt_offset.push(pl::sub(&t, &n(8)));
                }
            } else if is_dat_chunk.get(chunk.to_bytes()).truthy() {
                was_dat = chunk.clone();
            }
            if !m(it, &raf, "Seek", vec![pl::add(&len, &n(4)), n(1)])?.truthy() {
                break; // skip chunk data
            }
        }
        if !m(it, &raf, "Seek", vec![n(8), n(0)])?.truthy() {
            m(it, &et, "Error", vec![pl::s("Error seeking in file")])?;
            return ret(n(-1));
        }
        was_dat = Value::Undef;
    }

    // process the PNG/MNG/JNG chunks
    *no_compress_lib.borrow_mut() = Value::Undef;
    loop {
        if do_txt.truthy() {
            // read text chunks that were found after IDAT so we can write them before
            let off = if txt_offset.is_empty() { Value::Undef } else { txt_offset.remove(0) };
            if !m(it, &raf, "Seek", vec![off, n(0)])?.truthy() {
                m(it, &et, "Error", vec![pl::s("Seek error")])?;
                break;
            }
            // (this is the IDAT offset if @txtOffset is now empty)
            if txt_offset.is_empty() {
                do_txt = Value::Undef;
            }
        }
        nn = read(it, &raf, &mut hbuf, n(8))?; // read chunk header

        if was_end.truthy() {
            if !nn.truthy() {
                break; // stop now if normal end of PNG
            }
            m(it, &et, "Warn", vec![cat(&[&pl::s("Trailer data after "), &file_type, &pl::s(" "), &end_chunk, &pl::s(" chunk")]), n(1)])?;
            was_trailer = n(1);
            if pl::lt(&nn, &n(8)) {
                break;
            }
            et.set("SET_GROUP1", pl::s("Trailer"));
        } else if !pl::num_eq(&nn, &n(8)) {
            if !was_end.truthy() {
                m(it, &et, "Warn", vec![cat(&[&pl::s("Truncated "), &file_type, &pl::s(" image")])])?;
            }
            break;
        }
        let u = pl::unpack("Na4", &hbuf)?;
        let (len, mut chunk) = (arg(&u, 0), arg(&u, 1));
        if pl::gt(&len, &n(0x7fffffff)) {
            if !was_end.truthy() {
                m(it, &et, "Warn", vec![cat(&[&pl::s("Invalid "), &file_type, &pl::s(" chunk size")])])?;
            }
            break;
        }
        if verbose.truthy() {
            if do_txt.truthy() {
                print(it, &out, cat(&[&pl::s("  Moving "), &chunk, &pl::s(" from after IDAT ("), &len, &pl::s(" bytes)\n")]))?;
            }
            // don't dump image data chunks in verbose mode (only give count instead)
            if dat_count.truthy() && chunk.to_bytes() != dat_chunk.to_bytes() {
                let s = pl::s(if pl::gt(&dat_count, &n(1)) { "s" } else { "" });
                print(it, &out, cat(&[&file_type, &pl::s(" "), &dat_chunk, &pl::s(" ("), &dat_count, &pl::s(" chunk"), &s, &pl::s(", total "), &dat_bytes, &pl::s(" bytes)\n")]))?;
                if hash.truthy() {
                    print(it, &out, cat(&[&et.get("INDENT"), &pl::s("(ImageDataHash: "), &dat_bytes, &pl::s(" bytes of "), &dat_chunk, &pl::s(" data)\n")]))?;
                }
                dat_count = n(0);
                dat_bytes = n(0);
            }
        }
        if !was_hdr.truthy() {
            if chunk.to_bytes() == hdr_chunk.to_bytes() {
                was_hdr = n(1);
            } else if hdr_chunk.eq_str("IHDR") && chunk.eq_str("CgBI") {
                m(it, &et, "Warn", vec![pl::s("Non-standard PNG image (Apple iPhone format)")])?;
            } else {
                m(it, &et, "Warn", vec![cat(&[&file_type, &pl::s(" image did not start with "), &hdr_chunk])])?;
            }
        }
        if outfile.truthy() && (is_dat_chunk.get(chunk.to_bytes()).truthy() || chunk.to_bytes() == end_chunk.to_bytes()) && !txt_offset.is_empty() {
            // continue processing here after we move the text chunks from after IDAT
            let t = m(it, &raf, "Tell", vec![])?;
            txt_offset.push(pl::sub(&t, &n(8)));
            do_txt = n(1); // process text chunks now
            continue;
        }
        if is_dat_chunk.get(chunk.to_bytes()).truthy() {
            if fast_scan.truthy() && pl::ge(&fast_scan, &n(2)) {
                m(it, &et, "VPrint", vec![n(0), cat(&[&pl::s("End processing at "), &chunk, &pl::s(" chunk due to FastScan="), &fast_scan, &pl::s(" setting")])])?;
                break;
            }
            dat_chunk = chunk.clone();
            dat_count = pl::add(&dat_count, &n(1));
            dat_bytes = pl::add(&dat_bytes, &len);
            was_dat = chunk.clone();
        } else {
            dat_chunk = pl::s("");
        }
        if outfile.truthy() {
            // add text chunks (including XMP) before any data chunk end chunk
            if dat_chunk.truthy() || chunk.to_bytes() == end_chunk.to_bytes() {
                // write iCCP chunk now if requested because AddChunks will try
                // to add it as a text profile chunk if this isn't successful
                // (ie. if Compress::Zlib wasn't available)
                png_call(it, "Add_iCCP", vec![et.clone(), outfile.clone()])?;
                if !png_call(it, "AddChunks", vec![et.clone(), outfile.clone()])?.truthy() {
                    err = n(1); // add all text chunks
                }
                if !png_call(it, "AddChunks", vec![et.clone(), outfile.clone(), pl::s("IFD0")])?.truthy() {
                    err = n(1); // and eXIf chunk
                }
            } else if chunk.eq_str("PLTE") {
                // iCCP chunk must come before PLTE (and IDAT, handled above)
                // (ignore errors -- will add later as text profile if this fails)
                png_call(it, "Add_iCCP", vec![et.clone(), outfile.clone()])?;
            }
        }
        if chunk.to_bytes() == end_chunk.to_bytes() {
            // read CRC
            if !pl::num_eq(&read(it, &raf, &mut cbuf, n(4))?, &n(4)) {
                if !was_end.truthy() {
                    m(it, &et, "Warn", vec![cat(&[&pl::s("Truncated "), &file_type, &pl::s(" "), &end_chunk, &pl::s(" chunk")])])?;
                }
                break;
            }
            if verbose.truthy() {
                print(it, &out, cat(&[&file_type, &pl::s(" "), &chunk, &pl::s(" (end of image)\n")]))?;
            }
            was_end = n(1);
            if outfile.truthy() {
                // write the IEND/MEND chunk with CRC
                if !write(it, &outfile, &[hbuf.clone(), cbuf.clone()])? {
                    err = n(1);
                }
                if et.get("DEL_GROUP").get("Trailer").truthy() {
                    if read(it, &raf, &mut hbuf, n(1))?.truthy() {
                        if verbose.truthy() {
                            print(it, &out, pl::s("  Deleting PNG trailer\n"))?;
                        }
                        inc_changed(&et);
                    }
                } else {
                    // copy over any existing trailer data
                    let mut tot = n(0);
                    loop {
                        nn = read(it, &raf, &mut hbuf, n(65536))?;
                        if !nn.truthy() {
                            break;
                        }
                        tot = pl::add(&tot, &nn);
                        if !write(it, &outfile, &[hbuf.clone()])? {
                            err = n(1);
                        }
                    }
                    if tot.truthy() && verbose.truthy() {
                        print(it, &out, cat(&[&pl::s("  Copying PNG trailer ("), &tot, &pl::s(" bytes)\n")]))?;
                    }
                }
                break;
            }
            continue;
        }
        if dat_chunk.truthy() {
            let chunk_size_limit = n(10000000); // largest chunk to read into memory
            if outfile.truthy() {
                // avoid loading very large data chunks into memory
                if pl::gt(&len, &chunk_size_limit) {
                    if !write(it, &outfile, &[hbuf.clone()])? {
                        err = n(1);
                    }
                    if !call(it, "CopyBlock", vec![raf.clone(), outfile.clone(), pl::add(&len, &n(4))])?.truthy() {
                        m(it, &et, "Error", vec![cat(&[&pl::s("Error copying "), &dat_chunk])])?;
                    }
                    continue;
                }
            // skip over data chunks if possible/necessary
            } else if !validate.truthy() || pl::gt(&len, &chunk_size_limit) {
                if hash.truthy() {
                    m(it, &et, "ImageDataHash", vec![raf.clone(), len.clone()])?;
                    if !pl::num_eq(&read(it, &raf, &mut cbuf, n(4))?, &n(4)) {
                        m(it, &et, "Warn", vec![pl::s("Truncated data")])?;
                        break;
                    }
                } else if !m(it, &raf, "Seek", vec![pl::add(&len, &n(4)), n(1)])?.truthy() {
                    m(it, &et, "Warn", vec![pl::s("Seek error")])?;
                    break;
                }
                continue;
            }
        } else if was_dat.truthy() && is_txt_chunk.get(chunk.to_bytes()).truthy() {
            let msg = if !outfile.truthy() {
                pl::s("may be ignored by some readers")
            } else if !do_txt.is_undef() {
                // $doTxt == 0 if we crossed a noLeapFrog chunk
                pl::s("can't be moved") // (but could be deleted then added back again)
            } else {
                pl::s("fixed")
            };
            m(it, &et, "Warn", vec![cat(&[&pl::s("Text/EXIF chunk(s) found after "), &et.get("FileType"), &pl::s(" "), &was_dat, &pl::s(" ("), &msg, &pl::s(")")]), n(1)])?;
        }
        // read chunk data and CRC
        if !(pl::num_eq(&read(it, &raf, &mut dbuf, len.clone())?, &len) && pl::num_eq(&read(it, &raf, &mut cbuf, n(4))?, &n(4))) {
            if !was_end.truthy() {
                m(it, &et, "Warn", vec![cat(&[&pl::s("Corrupted "), &file_type, &pl::s(" image")])])?;
            }
            break;
        }
        if hash.truthy() && dat_chunk.truthy() {
            m(it, &hash, "add", vec![dbuf.clone()])?; // add to hash if necessary
        }
        if verbose.truthy() || validate.truthy() || (outfile.truthy() && !fast_scan.truthy()) {
            // check CRC when in verbose mode (since we don't care about speed)
            let mut c = crc(it, &hbuf, Value::Undef, n(4))?;
            c = crc(it, &dbuf, c, Value::Undef)?;
            if !pl::num_eq(&c, &pl::unpack1("N", &cbuf)?) {
                let msg = cat(&[&pl::s("Bad CRC for "), &chunk, &pl::s(" chunk")]);
                if outfile.truthy() {
                    m(it, &et, "Error", vec![msg, n(1)])?;
                } else {
                    m(it, &et, "Warn", vec![msg])?;
                }
            }
            if dat_chunk.truthy() {
                if outfile.truthy() && !write(it, &outfile, &[hbuf.clone(), dbuf.clone(), cbuf.clone()])? {
                    err = n(1);
                }
                continue;
            }
            // just skip over any text chunk found after IDAT
            if outfile.truthy() && was_dat.truthy() {
                if is_txt_chunk.get(chunk.to_bytes()).truthy() && do_txt.is_undef() {
                    if et.get("FORCE_WRITE").get("PNG").truthy() {
                        inc_changed(&et);
                    }
                    if verbose.truthy() {
                        print(it, &out, cat(&[&pl::s("  Deleting "), &chunk, &pl::s(" that was moved ("), &len, &pl::s(" bytes)\n")]))?;
                    }
                    continue;
                }
                // done moving text if we hit one of these chunks
                if no_leap_frog.get(chunk.to_bytes()).truthy() {
                    do_txt = n(0);
                }
            }
            if verbose.truthy() {
                print(it, &out, cat(&[&file_type, &pl::s(" "), &chunk, &pl::s(" ("), &len, &pl::s(" bytes):\n")]))?;
                if pl::gt(&verbose, &n(2)) {
                    let t = m(it, &raf, "Tell", vec![])?;
                    let addr = pl::sub(&pl::sub(&t, &len), &n(4));
                    m(it, &et, "VerboseDump", vec![pl::sref(dbuf.clone()), pl::s("Addr"), addr])?;
                }
            }
        }
        // translate case of chunk names that have changed since the first implementation
        if !tag_table_ptr.get(chunk.to_bytes()).truthy() && std_case.get(pl::lc(&chunk).to_bytes()).truthy() {
            let std_chunk = std_case.get(pl::lc(&chunk).to_bytes());
            if outfile.truthy() && (et.get("EDIT_DIRS").get("IFD0").truthy() || !re(r"^[ez]xif$", "i").is_match(&std_chunk)) {
                m(it, &et, "Warn", vec![cat(&[&pl::s("Changed "), &chunk, &pl::s(" chunk to "), &std_chunk]), n(1)])?;
                inc_changed(&et);
            } else {
                m(it, &et, "Warn", vec![cat(&[&chunk, &pl::s(" chunk should be "), &std_chunk]), n(1)])?;
            }
            chunk = std_case.get(pl::lc(&chunk).to_bytes());
        }
        // only extract information from chunks in our tables
        let the_buff = cell(Value::Undef);
        let out_buff = if outfile.truthy() { Value::ScalarRef(the_buff.clone()) } else { Value::Undef };
        if tag_table_ptr.get(chunk.to_bytes()).truthy() {
            found_png1(it, vec![et.clone(), tag_table_ptr.clone(), chunk.clone(), dbuf.clone(), Value::Undef, out_buff.clone()])?;
        } else if mng_table_ptr.truthy() && mng_table_ptr.get(chunk.to_bytes()).truthy() {
            found_png1(it, vec![et.clone(), mng_table_ptr.clone(), chunk.clone(), dbuf.clone(), Value::Undef, out_buff.clone()])?;
        }
        if outfile.truthy() {
            let tb = the_buff.borrow().clone();
            if !tb.is_undef() {
                if !pl::length(&tb).truthy() {
                    continue; // empty if we deleted the information
                }
                // change chunk type if necessary
                if et.get("TextChunkType").truthy() {
                    chunk = et.get("TextChunkType");
                    et.delete("TextChunkType");
                }
                hbuf = pl::pack("Na4", &[pl::length(&tb), chunk.clone()])?;
                dbuf = tb;
                let mut c = crc(it, &hbuf, Value::Undef, n(4))?;
                c = crc(it, &dbuf, c, Value::Undef)?;
                cbuf = pl::pack("N", &[c])?;
            }
            if !write(it, &outfile, &[hbuf.clone(), dbuf.clone(), cbuf.clone()])? {
                err = n(1);
            }
        }
    }
    et.delete("SET_GROUP1");
    // read Samsung trailer if it exists
    if was_trailer.truthy() && !outfile.truthy() && m(it, &raf, "Seek", vec![n(-8), n(2)])?.truthy() && read(it, &raf, &mut dbuf, n(8))?.truthy() && re(r"\0\0(QDIOBS|SEFT)$", "").is_match(&dbuf) {
        // (have only seen SEFT type)
        crate::runtime::require(it, "Image::ExifTool::Samsung")?;
        let di = Value::new_hash();
        di.set("DirName", pl::s("Samsung"));
        di.set("RAF", raf.clone());
        pl::call(it, "Image::ExifTool::Samsung::ProcessSamsung", vec![et.clone(), di])?;
    }
    if outfile.truthy() && (err.truthy() || !was_end.truthy()) {
        return ret(n(-1));
    }
    ret(n(1)) // this was a valid PNG/MNG/JNG image
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::PNG::AUTOLOAD", autoload);
    it.define_native("Image::ExifTool::PNG::StandardLangCase", standard_lang_case);
    it.define_native("Image::ExifTool::PNG::ConvertPNGDate", convert_png_date);
    it.define_native("Image::ExifTool::PNG::InversePNGDate", inverse_png_date);
    it.define_native("Image::ExifTool::PNG::GetLangInfo", get_lang_info);
    it.define_native("Image::ExifTool::PNG::FoundPNG", found_png);
    it.define_native("Image::ExifTool::PNG::ProcessProfile", process_profile);
    it.define_native("Image::ExifTool::PNG::ProcessPNG_Compressed", process_png_compressed);
    it.define_native("Image::ExifTool::PNG::ProcessPNG_tEXt", process_png_text);
    it.define_native("Image::ExifTool::PNG::ProcessPNG_iTXt", process_png_itxt);
    it.define_native("Image::ExifTool::PNG::ProcessPNG_eXIf", process_png_exif);
    it.define_native("Image::ExifTool::PNG::ProcessPNG", process_png);
}
