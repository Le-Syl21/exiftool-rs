//! Image::ExifTool::Jpeg2000 (Image/ExifTool/Jpeg2000.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read JPEG 2000 meta information (and JPEG XL, JUMBF/C2PA and the
//! Brotli-compressed 'brob' boxes, through the IO::Compress::Brotli and
//! IO::Uncompress::Brotli natives).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::cell;
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

const MODULE: &str = "Image::ExifTool::Jpeg2000";

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

fn m(it: &mut Interp, et: &Value, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::method(it, et, name, args)
}

/// `$raf->Read($buff, $len)`: the count, with `buff` updated.
fn read(it: &mut Interp, raf: &Value, buff: &mut Value, len: Value) -> ER<Value> {
    let mut args = [buff.clone(), len];
    let r = pl::method_mut(it, raf, "Read", &mut args, Want::Scalar)?;
    *buff = args[0].clone();
    Ok(r.into_iter().next().unwrap_or_default())
}

/// `$raf->Read($buff, $len) == $len`
fn read_eq(it: &mut Interp, raf: &Value, buff: &mut Value, len: &Value) -> ER<bool> {
    let r = read(it, raf, buff, len.clone())?;
    Ok(pl::num_eq(&r, len))
}

/// `Write($outfile, LIST)`: true on success.
fn write(it: &mut Interp, outfile: &Value, items: &[Value]) -> ER<bool> {
    let mut args = vec![outfile.clone()];
    args.extend_from_slice(items);
    Ok(call(it, "Write", args)?.truthy())
}

fn seek(it: &mut Interp, raf: &Value, pos: Value, whence: i64) -> ER<bool> {
    Ok(m(it, raf, "Seek", vec![pos, n(whence)])?.truthy())
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

/// `eval { require MODULE }`: true if loaded.
fn try_require(it: &mut Interp, module: &str) -> ER<bool> {
    Ok(pl::eval_block(it, |it| crate::runtime::require(it, module))?.is_some())
}

/// `eval { $res = FUNC(ARGS) }`: the result, or None when it died (with `$@` set).
fn eval_call(it: &mut Interp, func: &str, args: Vec<Value>) -> ER<Option<Value>> {
    pl::eval_block(it, |it| pl::call(it, func, args))
}

/// The keys of a hash, sorted (`sort keys %$h`).
fn sorted_keys(h: &Value) -> Vec<Value> {
    let mut k: Vec<Value> = h.keys().into_iter().map(Value::from).collect();
    k.sort_by(|x, y| x.to_bytes().cmp(&y.to_bytes()));
    k
}

/// Read JUMBF box to keep track of sub-document numbers
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_jumb(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if et.get("jumd_level").truthy() {
        let lv = et.get("jumd_level");
        if lv.is_empty() {
            return crate::interp::eval::die("Modification of non-creatable array value attempted, subscript -1");
        }
        let last = lv.len() - 1;
        lv.set_at(last, pl::add(&lv.at(last as i64), &n(1))); // increment current sub-document number
    } else {
        let dc = pl::add(&et.get("DOC_COUNT"), &n(1));
        et.set("DOC_COUNT", dc.clone());
        et.set("jumd_level", Value::new_array(vec![dc])); // new top-level sub-document
        et.set("SET_GROUP0", pl::s("JUMBF"));
    }
    et.set("DOC_NUM", pl::join("-", &et.get("jumd_level").items()));
    et.get("jumd_level").push(n(0));
    process_jpeg2000_box(it, vec![et.clone(), dir_info, tag_table_ptr], Want::Scalar)?;
    et.delete("DOC_NUM");
    et.delete("JUMBFLabel");
    let lv = et.get_or_new_array("jumd_level");
    if let Value::ArrayRef(r) = &lv {
        r.borrow_mut().pop();
    }
    if lv.len() < 2 {
        et.delete("jumd_level");
        et.delete("SET_GROUP0");
    }
    ret(n(1))
}

/// Read JUMBF description box (ref https://github.com/thorfdbg/codestream-parser)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_jumd(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let mut pos = dir_info.get("DirStart");
    let end = pl::add(&pos, &dir_info.get("DirLen"));
    m(it, &et, "VerboseDir", vec![pl::s("JUMD"), n(0), pl::sub(&end, &pos)])?;
    et.delete("JUMBFLabel");
    if pl::lt(&dir_info.get("DirLen"), &n(17)) {
        m(it, &et, "Warn", vec![pl::s("Truncated JUMD directory")])?;
        return ret(n(0));
    }
    pl::at(812);
    let data = pl::sderef(&data_pt)?;
    let _type = pl::substr(&data, pos.to_i64(), Some(4));
    m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("type"), pl::substr(&data, pos.to_i64(), Some(16))])?;
    pos = pl::add(&pos, &n(16));
    let flags = call(it, "Get8u", vec![data_pt.clone(), pos.clone()])?;
    pos = pl::add(&pos, &n(1));
    m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("toggles"), flags.clone()])?;
    if flags.to_i64() & 0x02 != 0 {
        // label exists?
        // pos($$dataPt) = $pos; $$dataPt =~ /\0/g
        let b = pl::sderef(&data_pt)?.to_bytes();
        let p = pos.to_i64().max(0) as usize;
        let found = if p <= b.len() { b[p..].iter().position(|&c| c == 0).map(|i| p + i + 1) } else { None };
        let Some(end_pos) = found else {
            m(it, &et, "Warn", vec![pl::s("Missing JUMD label terminator")])?;
            return ret(n(0));
        };
        let len = n(end_pos as i64 - pos.to_i64());
        let mut name = pl::substr(&pl::sderef(&data_pt)?, pos.to_i64(), Some(len.to_i64()));
        m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("label"), name.clone()])?;
        pos = pl::add(&pos, &len);
        if len.truthy() {
            re(r"[^-_a-zA-Z0-9]([a-z])", "").subst(&mut name, true, |c| pl::uc(&cap(c, 1)).to_bytes().to_vec()); // capitalize characters after illegal characters
            pl::tr(&mut name, "-_a-zA-Z0-9", "", "dc"); // remove other illegal characters
            re(r"__", "").subst(&mut name, false, |_| b"_".to_vec()); // collapse double underlines
            name = pl::ucfirst(&name); // capitalize first letter
            re(r"C2pa", "").subst(&mut name, false, |_| b"C2PA".to_vec()); // capitalize C2PA
            if pl::lt(&pl::length(&name), &n(2)) {
                name = pl::concat(&pl::s("Tag"), &name); // must at least 2 characters long
            }
            et.set("JUMBFLabel", name);
        }
    }
    if flags.to_i64() & 0x04 != 0 {
        // ID exists?
        if pl::gt(&pl::add(&pos, &n(4)), &end) {
            m(it, &et, "Warn", vec![pl::s("Missing JUMD ID")])?;
            return ret(n(0));
        }
        let id = call(it, "Get32u", vec![data_pt.clone(), pos.clone()])?;
        m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("id"), id])?;
        pos = pl::add(&pos, &n(4));
    }
    if flags.to_i64() & 0x08 != 0 {
        // signature exists?
        if pl::gt(&pl::add(&pos, &n(32)), &end) {
            m(it, &et, "Warn", vec![pl::s("Missing JUMD signature")])?;
            return ret(n(0));
        }
        let sig = pl::substr(&pl::sderef(&data_pt)?, pos.to_i64(), Some(32));
        m(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("sig"), sig])?;
        pos = pl::add(&pos, &n(32));
    }
    let more = pl::sub(&end, &pos);
    if more.truthy() {
        // (may find c2sh box hiding after JUMD record)
        if pl::ge(&more, &n(8)) {
            let di = Value::new_hash();
            di.set("DataPt", data_pt.clone());
            di.set("DataLen", dir_info.get("DataLen"));
            di.set("DirStart", pos.clone());
            di.set("DirLen", more.clone());
            di.set("DirName", pl::s("JUMDPrivate"));
            let t = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Jpeg2000::Main")])?;
            m(it, &et, "ProcessDirectory", vec![di, t])?;
        } else {
            m(it, &et, "Warn", vec![cat(&[&pl::s("Extra data in JUMD box "), &more, &pl::s(" bytes)")]), n(1)])?;
        }
    }
    ret(n(1))
}

/// Warn about error in Brotli compression/decompression
/// Inputs: 0) ExifTool ref, 1) box type, 2) true for decoding (Uncompress)
pub fn brotli_warn(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, type_, uncompress) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let (enc, mod_) = if uncompress.truthy() { ("decoding", "Uncompress") } else { ("encoding", "Compress") };
    m(it, &et, "Warn", vec![cat(&[&pl::s("Error "), &pl::s(enc), &pl::s(" '"), &type_, &pl::s("' brob box")])])?;
    ret(m(it, &et, "Warn", vec![pl::s(&format!("Try updating to IO::{mod_}::Brotli 0.004 or later"))])?)
}

fn brotli_warn1(it: &mut Interp, et: &Value, type_: &Value, uncompress: bool) -> ER<()> {
    let mut a = vec![et.clone(), type_.clone()];
    if uncompress {
        a.push(n(1));
    }
    brotli_warn(it, a, Want::Void)?;
    Ok(())
}

/// Create new JPEG 2000 boxes when writing
/// (Currently only supports adding top-level Writable JPEG2000 tags and certain UUID boxes)
/// Inputs: 0) ExifTool object ref, 1) Output file or scalar ref
/// Returns: 1 on success
pub fn create_new_boxes(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, outfile) = (arg(&a, 0), arg(&a, 1));
    let add_tags = et.get("AddJp2Tags");
    let add_dirs = et.get("AddJp2Dirs");
    et.delete("AddJp2Tags");
    et.delete("AddJp2Dirs");
    let uuid = crate::runtime::lex_hash(it, MODULE, "uuid");
    // add JPEG2000 tags
    for tag in sorted_keys(&add_tags) {
        let tag_info = add_tags.get(tag.to_bytes());
        let nv_hash = m(it, &et, "GetNewValueHash", vec![tag_info.clone()])?;
        // (native JPEG2000 information is always preferred, so don't check IsCreating)
        if !(tag_info.get("List").truthy() || pl::gt(&m(it, &et, "IsOverwriting", vec![nv_hash.clone()])?, &n(0))) {
            continue;
        }
        if nv_hash.get("EditOnly").truthy() {
            continue;
        }
        let vals = pl::method_list(it, &et, "GetNewValue", vec![nv_hash.clone()])?;
        for val in vals {
            let boxhdr = pl::concat(&pl::pack("N", &[pl::add(&pl::length(&val), &n(8))])?, &tag_info.get("TagID"));
            if !write(it, &outfile, &[boxhdr, val.clone()])? {
                return ret(n(0));
            }
            inc_changed(&et);
            m(it, &et, "VerboseValue", vec![cat(&[&pl::s("+ Jpeg2000:"), &tag_info.get("Name")]), val])?;
        }
    }
    // add UUID boxes (and/or JXL Exif/XML boxes)
    let main = pl::pkg_hash(it, "Image::ExifTool::Jpeg2000::Main");
    for dir_name in sorted_keys(&add_dirs) {
        // handle JPEG XL XMP and EXIF
        if dir_name.eq_str("xml ") || dir_name.eq_str("Exif") {
            let (mut tag, dir) = if dir_name.eq_str("xml ") { (pl::s("xml "), "XMP") } else { (pl::s("Exif"), "EXIF") };
            let mut tag_info = main.get(tag.to_bytes());
            if tag_info.is_array() {
                tag_info = tag_info.at(1); // (hack for stupid JXL XMP)
            }
            let subdir = tag_info.get("SubDirectory");
            let mut tag_table = call(it, "GetTagTable", vec![subdir.get("TagTable")])?;
            if dir == "XMP" {
                tag_table = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::XMP::Main")])?;
            }
            let di = Value::new_hash();
            di.set("DirName", pl::s(dir));
            di.set("Parent", tag.clone());
            let compress = m(it, &et, "Options", vec![pl::s("Compress")])?;
            if et.get("IsJXL").truthy() && compress.truthy() {
                di.set("Compact", n(1));
            }
            let mut newdir = m(it, &et, "WriteDirectory", vec![di, tag_table, subdir.get("WriteProc")])?;
            if !newdir.is_undef() && pl::length(&newdir).truthy() {
                // not sure why, but EXIF box is padded with leading 0's in my sample
                let mut pad = if dir_name.eq_str("Exif") { Value::bytes(b"\0\0\0\0") } else { pl::s("") };
                if et.get("IsJXL").truthy() && compress.truthy() {
                    // create as Brotli-compressed metadata
                    if try_require(it, "IO::Compress::Brotli")? {
                        let compressed = eval_call(it, "IO::Compress::Brotli::bro", vec![pl::concat(&pad, &newdir)])?.unwrap_or_default();
                        if it.get_scalar("main::@").truthy() || !compressed.truthy() {
                            brotli_warn1(it, &et, &dir_name, false)?;
                        } else {
                            m(it, &et, "VPrint", vec![n(0), pl::s(&format!("  Writing Brotli-compressed {dir}\n"))])?;
                            newdir = compressed;
                            pad = tag.clone();
                            tag = pl::s("brob");
                        }
                    } else {
                        m(it, &et, "Warn", vec![pl::s("Install IO::Compress::Brotli to create Brotli-compressed metadata")])?;
                    }
                }
                let boxhdr = pl::concat(&pl::pack("N", &[pl::add(&pl::add(&pl::length(&newdir), &pl::length(&pad)), &n(8))])?, &tag);
                if !write(it, &outfile, &[boxhdr, pad, newdir])? {
                    return ret(n(0));
                }
                continue;
            }
        }
        if !uuid.get(dir_name.to_bytes()).truthy() {
            continue;
        }
        for tag_info in main.get("uuid").items() {
            if tag_info.get("Name").to_bytes() != dir_name.to_bytes() {
                continue;
            }
            let subdir = tag_info.get("SubDirectory");
            let tag_table = call(it, "GetTagTable", vec![subdir.get("TagTable")])?;
            let di = Value::new_hash();
            let sdn = subdir.get("DirName");
            di.set("DirName", if sdn.truthy() { sdn } else { dir_name.clone() });
            di.set("Parent", pl::s("JP2"));
            // remove "UUID-" from start of directory name to allow appropriate
            // directories to be written as a block
            let mut dn = di.get("DirName");
            if re(r"^UUID-", "").subst(&mut dn, false, |_| Vec::new()) > 0 {
                di.set("DirName", dn);
            }
            let newdir = m(it, &et, "WriteDirectory", vec![di, tag_table, subdir.get("WriteProc")])?;
            if !newdir.is_undef() && pl::length(&newdir).truthy() {
                let boxhdr = cat(&[&pl::pack("N", &[pl::add(&pl::length(&newdir), &n(24))])?, &pl::s("uuid"), &uuid.get(dir_name.to_bytes())]);
                if !write(it, &outfile, &[boxhdr, newdir])? {
                    return ret(n(0));
                }
                break;
            }
        }
    }
    ret(n(1))
}

/// Create Color Specification Box
/// Inputs: 0) ExifTool object ref, 1) Output file or scalar ref
/// Returns: 1 on success
pub fn create_color_spec(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, outfile) = (arg(&a, 0), arg(&a, 1));
    let mut meth = m(it, &et, "GetNewValue", vec![pl::s("Jpeg2000:ColorSpecMethod")])?;
    let p = m(it, &et, "GetNewValue", vec![pl::s("Jpeg2000:ColorSpecPrecedence")])?;
    let prec = if p.truthy() { p } else { n(0) };
    let ap = m(it, &et, "GetNewValue", vec![pl::s("Jpeg2000:ColorSpecApproximation")])?;
    let approx = if ap.truthy() { ap } else { n(0) };
    let icc = m(it, &et, "GetNewValue", vec![pl::s("ICC_Profile")])?;
    let space = m(it, &et, "GetNewValue", vec![pl::s("Jpeg2000:ColorSpace")])?;
    let mut cdata = m(it, &et, "GetNewValue", vec![pl::s("Jpeg2000:ColorSpecData")])?;
    if !meth.truthy() {
        if icc.truthy() {
            meth = n(2);
        } else if !space.is_undef() {
            meth = n(1);
        } else if !cdata.is_undef() {
            meth = n(4);
        } else {
            m(it, &et, "Warn", vec![pl::s("Color space not defined")])?;
            return ret(n(0));
        }
    }
    if meth.eq_str("1") {
        if space.is_undef() {
            m(it, &et, "Warn", vec![pl::s("Must specify ColorSpace")])?;
            return ret(n(0));
        }
        cdata = pl::pack("N", &[space])?;
    } else if meth.eq_str("2") || meth.eq_str("3") {
        if icc.is_undef() {
            m(it, &et, "Warn", vec![pl::s("Must specify ICC_Profile")])?;
            return ret(n(0));
        }
        cdata = icc;
    } else if meth.eq_str("4") {
        if cdata.is_undef() {
            m(it, &et, "Warn", vec![pl::s("Must specify ColorSpecData")])?;
            return ret(n(0));
        }
    } else {
        m(it, &et, "Warn", vec![pl::s("Unknown ColorSpecMethod")])?;
        return ret(n(0));
    }
    let boxhdr = pl::concat(&pl::pack("N", &[pl::add(&pl::length(&cdata), &n(11))])?, &pl::s("colr"));
    let ccc = pl::pack("CCC", &[meth, prec, approx])?;
    if !write(it, &outfile, &[boxhdr, ccc, cdata])? {
        return ret(n(0));
    }
    inc_changed(&et);
    m(it, &et, "VPrint", vec![n(1), pl::s("    + Jpeg2000:ColorSpec\n")])?;
    ret(n(1))
}

/// Process JPEG 2000 box
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference, 2) Pointer to tag table
/// Returns: 1 on success when reading, or -1 on write error
///          (or JP2 box or undef when writing from buffer)
pub fn process_jpeg2000_box(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let or0 = |v: Value| if v.truthy() { v } else { n(0) };
    let mut data_pt = dir_info.get("DataPt");
    let mut data_len = dir_info.get("DataLen");
    let mut data_pos = or0(dir_info.get("DataPos"));
    let mut _dir_len = or0(dir_info.get("DirLen"));
    let dir_start = or0(dir_info.get("DirStart"));
    let base = or0(dir_info.get("Base"));
    let mut outfile = dir_info.get("OutFile");
    let dn = dir_info.get("DirName");
    let dir_name = if dn.truthy() { dn } else { pl::s("") };
    let mut dir_end = pl::add(&dir_start, &_dir_len);
    let (mut err, mut verbose, mut do_colour, mut hash, mut raf) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    let out_buff = cell(Value::Undef);
    let uuid_h = crate::runtime::lex_hash(it, MODULE, "uuid");
    let is_image_data = crate::runtime::lex_hash(it, MODULE, "isImageData");

    if data_pt.truthy() {
        // save C2PA JUMBF as a block if requested
        if dir_name.eq_str("JUMBF") && et.get("REQ_TAG_LOOKUP").get("jumbf").truthy() && !dir_info.get("NoBlockSave").truthy() {
            pl::at(1033);
            if dir_start.truthy() || _dir_len.to_bytes() != pl::length(&pl::sderef(&data_pt)?).to_bytes() {
                let dat = pl::substr(&pl::sderef(&data_pt)?, dir_start.to_i64(), Some(_dir_len.to_i64()));
                m(it, &et, "FoundTag", vec![pl::s("JUMBF"), pl::sref(dat)])?;
            } else {
                m(it, &et, "FoundTag", vec![pl::s("JUMBF"), data_pt.clone()])?;
            }
        }
    } else {
        raf = dir_info.get("RAF"); // read from RAF
    }

    if outfile.truthy() {
        if !raf.truthy() {
            // buffer output to be used for return value
            *out_buff.borrow_mut() = pl::s("");
            outfile = Value::ScalarRef(out_buff.clone());
        }
        // determine if we will be writing colr box
        if dir_name.eq_str("JP2Header") {
            let g = |it: &mut Interp, t: &str| m(it, &et, "GetNewValue", vec![pl::s(t)]);
            if !g(it, "ColorSpecMethod")?.is_undef() || g(it, "ICC_Profile")?.truthy() || !g(it, "ColorSpecPrecedence")?.is_undef() || !g(it, "ColorSpace")?.is_undef() || !g(it, "ColorSpecApproximation")?.is_undef() || !g(it, "ColorSpecData")?.is_undef() {
                do_colour = n(2);
            }
        }
    } else {
        // (must not set verbose flag when writing!)
        verbose = et.path(&["OPTIONS", "Verbose"]);
        if verbose.truthy() {
            m(it, &et, "VerboseDir", vec![dir_name.clone()])?;
        }
        // do hash if requested, but only for top-level image data
        if raf.truthy() {
            hash = et.get("ImageDataHash");
        }
    }
    // loop through all contained boxes
    let mut pos = dir_start.clone();
    let mut box_len = Value::Undef;
    let mut _last_box;
    let mut first = true;
    loop {
        if !first {
            pos = pl::add(&pos, &box_len);
        }
        first = false;
        let mut box_id;
        let buff = cell(Value::Undef);
        let value_ptr;
        let mut hdr_len = n(8); // the box header length
        if raf.truthy() {
            data_pos = pl::sub(&m(it, &raf, "Tell", vec![])?, &base);
            let mut b = Value::Undef;
            let nn = read(it, &raf, &mut b, hdr_len.clone())?;
            *buff.borrow_mut() = b;
            if !pl::num_eq(&nn, &hdr_len) {
                if nn.truthy() {
                    err = pl::s("");
                    break;
                }
                if outfile.truthy() && !create_new_boxes(it, vec![et.clone(), outfile.clone()], Want::Scalar)?.pop().unwrap_or_default().truthy() {
                    err = n(1);
                }
                break;
            }
            data_pt = Value::ScalarRef(buff.clone());
            _dir_len = hdr_len.clone();
            dir_end = hdr_len.clone();
            pos = n(0);
        } else if pl::ge(&pos, &pl::sub(&dir_end, &hdr_len)) {
            if !pl::num_eq(&pos, &dir_end) {
                err = pl::s("");
            }
            break;
        }
        pl::at(1083);
        box_len = pl::unpack1(&format!("x{} N", pos.to_i64()), &pl::sderef(&data_pt)?)?; // (length includes header and data)
        box_id = pl::substr(&pl::sderef(&data_pt)?, pl::add(&pos, &n(4)).to_i64(), Some(4));
        // (ftbl box contains flst boxes with absolute file offsets, not currently handled)
        if outfile.truthy() && box_id.eq_str("ftbl") {
            m(it, &et, "Error", vec![pl::s("Can't yet handle fragmented JPX files")])?;
            return ret(n(-1));
        }
        // remove old colr boxes if necessary
        if do_colour.truthy() && box_id.eq_str("colr") {
            if pl::num_eq(&do_colour, &n(1)) {
                // did we successfully write the new colr box?
                m(it, &et, "VPrint", vec![n(1), pl::s("    - Jpeg2000:ColorSpec\n")])?;
                inc_changed(&et);
                continue;
            }
            m(it, &et, "Warn", vec![pl::s("Out-of-order colr box encountered")])?;
            do_colour = Value::Undef;
        }
        _last_box = box_id.clone();
        pos = pl::add(&pos, &hdr_len); // move to end of box header
        if pl::num_eq(&box_len, &n(1)) {
            // box header contains an additional 8-byte integer for length
            hdr_len = pl::add(&hdr_len, &n(8));
            if raf.truthy() {
                let mut buf2 = Value::Undef;
                if read_eq(it, &raf, &mut buf2, &n(8))? {
                    let b = pl::concat(&buff.borrow(), &buf2);
                    *buff.borrow_mut() = b;
                    _dir_len = hdr_len.clone();
                    dir_end = hdr_len.clone();
                }
            }
            if pl::gt(&pos, &pl::sub(&dir_end, &n(8))) {
                err = pl::s("");
                break;
            }
            pl::at(1113);
            let hl = pl::unpack(&format!("x{} N2", pos.to_i64()), &pl::sderef(&data_pt)?)?;
            let (hi, lo) = (arg(&hl, 0), arg(&hl, 1));
            if hi.truthy() {
                err = pl::s("Can't currently handle JPEG 2000 boxes > 4 GB");
                break;
            }
            pos = pl::add(&pos, &n(8)); // move to end of extended-length box header
            box_len = pl::sub(&lo, &hdr_len); // length of remaining box data
        } else if pl::num_eq(&box_len, &n(0)) {
            if raf.truthy() {
                if outfile.truthy() {
                    if !create_new_boxes(it, vec![et.clone(), outfile.clone()], Want::Scalar)?.pop().unwrap_or_default().truthy() {
                        err = n(1);
                    }
                    // copy over the rest of the file
                    if !write(it, &outfile, &[pl::sderef(&data_pt)?])? {
                        err = n(1);
                    }
                    let mut b = buff.borrow().clone();
                    while read(it, &raf, &mut b, n(65536))?.truthy() {
                        if !write(it, &outfile, &[b.clone()])? {
                            err = n(1);
                        }
                    }
                    *buff.borrow_mut() = b;
                } else {
                    if verbose.truthy() {
                        let msg = pl::sprintf("offset 0x%.4x to end of file", &[pl::add(&pl::add(&data_pos, &base), &pos)]);
                        m(it, &et, "VPrint", vec![n(0), cat(&[&et.get("INDENT"), &pl::s("- Tag '"), &box_id, &pl::s("' ("), &msg, &pl::s(")\n")])])?;
                    }
                    if hash.truthy() && is_image_data.get(box_id.to_bytes()).truthy() {
                        m(it, &et, "ImageDataHash", vec![raf.clone(), Value::Undef, box_id.clone()])?;
                    }
                }
                break; // (ignore the rest of the file when reading)
            }
            box_len = pl::sub(&dir_end, &pos); // data runs to end of file
        } else {
            box_len = pl::sub(&box_len, &hdr_len); // length of remaining box data
        }
        if pl::lt(&box_len, &n(0)) {
            err = pl::s("Invalid JPEG 2000 box length");
            break;
        }
        let mut tag_info = m(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), box_id.clone()])?;
        if !(!tag_info.is_undef() || verbose.truthy()) {
            // no need to process this box
            if raf.truthy() {
                if outfile.truthy() {
                    if !write(it, &outfile, &[pl::sderef(&data_pt)?])? {
                        err = n(1);
                    }
                    let mut b = buff.borrow().clone();
                    let ok = read_eq(it, &raf, &mut b, &box_len)?;
                    *buff.borrow_mut() = b.clone();
                    if !ok {
                        err = pl::s("");
                        break;
                    }
                    if !write(it, &outfile, &[b])? {
                        err = n(1);
                    }
                } else if hash.truthy() && is_image_data.get(box_id.to_bytes()).truthy() {
                    m(it, &et, "ImageDataHash", vec![raf.clone(), box_len.clone(), box_id.clone()])?;
                } else if !seek(it, &raf, box_len.clone(), 1)? {
                    err = pl::s("Seek error");
                    break;
                }
            } else if outfile.truthy() {
                let s = pl::substr(&pl::sderef(&data_pt)?, pl::sub(&pos, &hdr_len).to_i64(), Some(pl::add(&box_len, &hdr_len).to_i64()));
                if !write(it, &outfile, &[s])? {
                    err = pl::s("");
                    break;
                }
            }
            continue;
        }
        if raf.truthy() {
            // read the box data
            data_pos = pl::sub(&m(it, &raf, "Tell", vec![])?, &base);
            let mut b = buff.borrow().clone();
            let ok = read_eq(it, &raf, &mut b, &box_len)?;
            *buff.borrow_mut() = b.clone();
            if !ok {
                err = pl::s("");
                break;
            }
            if hash.truthy() && is_image_data.get(box_id.to_bytes()).truthy() {
                m(it, &hash, "add", vec![b])?;
                m(it, &et, "VPrint", vec![n(0), cat(&[&et.get("INDENT"), &pl::s("(ImageDataHash: "), &box_len, &pl::s(" bytes of "), &box_id, &pl::s(" data)\n")])])?;
            }
            value_ptr = n(0);
            data_len = box_len.clone();
        } else if pl::gt(&pl::add(&pos, &box_len), &dir_end) {
            err = pl::s("");
            break;
        } else {
            value_ptr = pos.clone();
        }
        let mut value_ptr = value_ptr;
        if !tag_info.is_undef() && !tag_info.truthy() {
            // GetTagInfo() required the value for a Condition
            let l = if pl::lt(&box_len, &n(128)) { box_len.clone() } else { n(128) };
            let tmp_val = pl::substr(&pl::sderef(&data_pt)?, value_ptr.to_i64(), Some(l.to_i64()));
            tag_info = m(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), box_id.clone(), pl::sref(tmp_val)])?;
        }
        // delete all UUID boxes and any writable box if deleting all information
        if outfile.truthy() && tag_info.truthy() {
            if box_id.eq_str("uuid") && et.get("DEL_GROUP").get("*").truthy() {
                m(it, &et, "VPrint", vec![n(0), cat(&[&pl::s("  Deleting "), &tag_info.get("Name"), &pl::s("\n")])])?;
                inc_changed(&et);
                continue;
            } else if tag_info.get("Writable").truthy() {
                let is_overwriting;
                if et.get("DEL_GROUP").get("Jpeg2000").truthy() {
                    is_overwriting = n(1);
                } else {
                    let nv_hash = m(it, &et, "GetNewValueHash", vec![tag_info.clone()])?;
                    is_overwriting = m(it, &et, "IsOverwriting", vec![nv_hash])?;
                }
                if is_overwriting.truthy() {
                    let val = pl::substr(&pl::sderef(&data_pt)?, value_ptr.to_i64(), Some(box_len.to_i64()));
                    m(it, &et, "VerboseValue", vec![cat(&[&pl::s("- Jpeg2000:"), &tag_info.get("Name")]), val])?;
                    inc_changed(&et);
                    continue;
                } else if !tag_info.get("List").truthy() {
                    et.get_or_new_hash("AddJp2Tags").delete(box_id.to_bytes());
                }
            }
        }
        // create new tag for JUMBF data values with name corresponding to JUMBFLabel
        if tag_info.truthy() && et.get("JUMBFLabel").truthy() && (!tag_info.get("SubDirectory").truthy() || tag_info.get("BlockExtract").truthy()) {
            let sfx = tag_info.get("JUMBF_Suffix");
            let new_info = tag_info.hash_copy();
            new_info.set("Name", pl::concat(&et.get("JUMBFLabel"), &if sfx.truthy() { sfx } else { pl::s("") }));
            tag_info = new_info;
            let mut desc = call(it, "MakeDescription", vec![tag_info.get("Name")])?;
            re(r"C2 PA", "").subst(&mut desc, false, |_| b"C2PA".to_vec());
            tag_info.set("Description", desc);
            call(it, "AddTagToTable", vec![tag_table_ptr.clone(), pl::concat(&pl::s("_JUMBF_"), &et.get("JUMBFLabel")), tag_info.clone()])?;
            tag_info.delete("Protected"); // (must do this so -j -b returns JUMBF binary data)
            tag_info.set("TagID", box_id.clone());
        }
        if verbose.truthy() {
            m(
                it,
                &et,
                "VerboseInfo",
                vec![
                    box_id.clone(),
                    tag_info.clone(),
                    pl::s("Table"),
                    tag_table_ptr.clone(),
                    pl::s("DataPt"),
                    data_pt.clone(),
                    pl::s("Size"),
                    box_len.clone(),
                    pl::s("Start"),
                    value_ptr.clone(),
                    pl::s("Addr"),
                    pl::add(&pl::add(&value_ptr, &data_pos), &base),
                ],
            )?;
            if !tag_info.truthy() {
                continue;
            }
        }
        if tag_info.get("SubDirectory").truthy() {
            let subdir = tag_info.get("SubDirectory");
            let mut subdir_start = value_ptr.clone();
            let mut subdir_len = box_len.clone();
            if !subdir.get("Start").is_undef() {
                //### eval Start ($valuePtr, $dataPt)
                let scope = crate::runtime::file_scope(it, MODULE);
                let (v, _) = pl::eval_site_in(it, &subdir.get("Start"), MODULE, Some(scope), &[("valuePtr", value_ptr.clone()), ("dataPt", data_pt.clone())], Want::Scalar);
                subdir_start = v;
                subdir_len = pl::sub(&subdir_len, &pl::sub(&subdir_start, &value_ptr));
                if pl::lt(&subdir_len, &n(0)) {
                    subdir_start = value_ptr.clone();
                    subdir_len = n(0);
                }
            }
            let subdir_info = Value::new_hash();
            subdir_info.set("Parent", pl::s("JP2"));
            subdir_info.set("DataPt", data_pt.clone());
            subdir_info.set("DataPos", pl::neg(&subdir_start)); // (relative to Base)
            subdir_info.set("DataLen", data_len.clone());
            subdir_info.set("DirStart", subdir_start.clone());
            subdir_info.set("DirLen", subdir_len.clone());
            let sdn = subdir.get("DirName");
            subdir_info.set("DirName", if sdn.truthy() { sdn } else { tag_info.get("Name") });
            subdir_info.set("OutFile", outfile.clone());
            subdir_info.set("Base", pl::add(&pl::add(&base, &data_pos), &subdir_start));
            let uuid = uuid_h.get(tag_info.get("Name").to_bytes());
            // remove "UUID-" prefix to allow appropriate directories to be written as a block
            let mut sdn = subdir_info.get("DirName");
            if re(r"^UUID-", "").subst(&mut sdn, false, |_| Vec::new()) > 0 {
                subdir_info.set("DirName", sdn);
            }
            let st = call(it, "GetTagTable", vec![subdir.get("TagTable")])?;
            let sub_table = if st.truthy() { st } else { tag_table_ptr.clone() };
            if outfile.truthy() {
                // (special case for brob box, which may be EXIF or XMP)
                let mut fake_id = box_id.clone();
                if box_id.eq_str("brob") {
                    // I have seen 'brob' ID's with funny cases, so standardize these
                    if re(r"^xml ", "i").is_match(&pl::sderef(&data_pt)?) {
                        fake_id = pl::s("xml ");
                    }
                    if re(r"^Exif", "i").is_match(&pl::sderef(&data_pt)?) {
                        fake_id = pl::s("Exif");
                    }
                }
                let mut newdir = Value::Undef;
                // only edit writable UUID, Exif and jp2h boxes
                if uuid.truthy() || fake_id.eq_str("Exif") || (fake_id.eq_str("xml ") && et.get("IsJXL").truthy()) || (box_id.eq_str("jp2h") && et.get("EDIT_DIRS").get("jp2h").truthy()) {
                    let compress = m(it, &et, "Options", vec![pl::s("Compress")])?;
                    subdir_info.set("Parent", fake_id.clone());
                    if compress.truthy() && et.get("IsJXL").truthy() {
                        subdir_info.set("Compact", n(1));
                    }
                    newdir = m(it, &et, "WriteDirectory", vec![subdir_info.clone(), sub_table.clone(), subdir.get("WriteProc")])?;
                    if !newdir.is_undef() && !pl::length(&newdir).truthy() {
                        continue; // next if deleting the box
                    }
                    // compress JXL EXIF or XMP metadata if requested
                    if !newdir.is_undef() && et.get("IsJXL").truthy() && !compress.is_undef() && (fake_id.eq_str("Exif") || fake_id.eq_str("xml ")) {
                        if compress.truthy() && !box_id.eq_str("brob") {
                            // rewrite as a Brotli-compressed 'brob' box
                            if try_require(it, "IO::Compress::Brotli")? {
                                let pad = if box_id.eq_str("Exif") { Value::bytes(b"\0\0\0\0") } else { pl::s("") };
                                let compressed = eval_call(it, "IO::Compress::Brotli::bro", vec![pl::concat(&pad, &newdir)])?.unwrap_or_default();
                                if it.get_scalar("main::@").truthy() || !compressed.truthy() {
                                    brotli_warn1(it, &et, &box_id, false)?;
                                } else {
                                    m(it, &et, "VPrint", vec![n(0), cat(&[&pl::s("  Writing Brotli-compressed "), &box_id, &pl::s("\n")])])?;
                                    newdir = pl::concat(&box_id, &compressed);
                                    box_id = pl::s("brob");
                                    subdir_start = n(0);
                                    value_ptr = n(0);
                                    inc_changed(&et);
                                }
                            } else {
                                m(it, &et, "Warn", vec![pl::s("Install IO::Compress::Brotli to write Brotli-compressed metadata")])?;
                            }
                        } else if !compress.truthy() && box_id.eq_str("brob") {
                            // (in this case, ProcessBrotli has returned uncompressed data,
                            //  so change to the uncompressed 'xml ' or 'Exif' box type)
                            m(it, &et, "VPrint", vec![n(0), cat(&[&pl::s("  Writing uncompressed "), &fake_id, &pl::s("\n")])])?;
                            box_id = fake_id.clone();
                            subdir_start = n(0);
                            value_ptr = n(0);
                            inc_changed(&et);
                        }
                    }
                } else if !uuid.is_undef() {
                    m(it, &et, "Warn", vec![cat(&[&pl::s("Not editing "), &tag_info.get("Name"), &pl::s(" box")]), n(1)])?;
                }
                // remove this directory from our create list
                let add_dirs = et.get_or_new_hash("AddJp2Dirs");
                add_dirs.delete(fake_id.to_bytes()); // (eg. 'Exif' or 'xml ')
                if box_id.eq_str("brob") {
                    // (can't make tag Name 'XMP' or 'Exif' for Brotli-compressed tags because it
                    //  would break the logic in WriteDirectory(), so we do a lookup here instead)
                    let k = if fake_id.eq_str("xml ") {
                        "XMP"
                    } else if fake_id.eq_str("Exif") {
                        "EXIF"
                    } else {
                        ""
                    };
                    add_dirs.delete(k);
                } else {
                    add_dirs.delete(tag_info.get("Name").to_bytes()); // (eg. 'EXIF' or 'XMP')
                }
                // use old box data if not changed
                if newdir.is_undef() {
                    newdir = pl::substr(&pl::sderef(&data_pt)?, subdir_start.to_i64(), Some(subdir_len.to_i64()));
                }
                let prefix_len = pl::sub(&subdir_start, &value_ptr);
                let mut boxhdr = pl::concat(&pl::pack("N", &[pl::add(&pl::add(&pl::length(&newdir), &n(8)), &prefix_len)])?, &box_id);
                if prefix_len.truthy() {
                    boxhdr = pl::concat(&boxhdr, &pl::substr(&pl::sderef(&data_pt)?, value_ptr.to_i64(), Some(prefix_len.to_i64())));
                }
                if !write(it, &outfile, &[boxhdr, newdir])? {
                    err = n(1);
                }
                // write new colr box immediately after ihdr
                if do_colour.truthy() && box_id.eq_str("ihdr") {
                    // (shouldn't be multiple ihdr boxes, but just in case, write only 1)
                    do_colour = if pl::num_eq(&do_colour, &n(2)) { create_color_spec(it, vec![et.clone(), outfile.clone()], Want::Scalar)?.pop().unwrap_or_default() } else { n(0) };
                }
            } else {
                // extract as a block if specified
                if tag_info.get("BlockExtract").truthy() {
                    subdir_info.set("BlockInfo", tag_info.clone());
                }
                if !uuid.is_undef() && uuid.eq_str("0") {
                    m(it, &et, "Warn", vec![cat(&[&pl::s("Reading non-standard "), &tag_info.get("Name"), &pl::s(" box")])])?;
                }
                if !m(it, &et, "ProcessDirectory", vec![subdir_info.clone(), sub_table.clone(), subdir.get("ProcessProc")])?.truthy() {
                    if sub_table.same_ref(&tag_table_ptr) {
                        err = pl::s("JPEG 2000 format error");
                        break;
                    }
                    m(it, &et, "Warn", vec![cat(&[&pl::s("Unrecognized "), &tag_info.get("Name"), &pl::s(" box")])])?;
                }
            }
        } else if tag_info.get("Format").truthy() && !outfile.truthy() {
            // only save tag values if Format was specified
            let rational = pl::sref(Value::Undef);
            let val = call(it, "ReadValue", vec![data_pt.clone(), value_ptr.clone(), tag_info.get("Format"), Value::Undef, box_len.clone(), rational.clone()])?;
            if !val.is_undef() {
                let key = m(it, &et, "FoundTag", vec![tag_info.clone(), val])?;
                // save Rational value
                let r = rational.deref();
                if !r.is_undef() && !key.is_undef() {
                    et.get_or_new_hash("TAG_EXTRA").get_or_new_hash(key.to_bytes()).set("Rational", r);
                }
            }
        } else if outfile.truthy() {
            let boxhdr = pl::concat(&pl::pack("N", &[pl::add(&box_len, &n(8))])?, &box_id);
            let s = pl::substr(&pl::sderef(&data_pt)?, value_ptr.to_i64(), Some(box_len.to_i64()));
            if !write(it, &outfile, &[boxhdr, s])? {
                err = n(1);
            }
        }
    }
    if !err.is_undef() {
        if !err.truthy() {
            err = pl::s("Truncated JPEG 2000 box");
        }
        if outfile.truthy() {
            if !err.eq_str("1") {
                m(it, &et, "Error", vec![err.clone()])?;
            }
            return if raf.truthy() { ret(n(-1)) } else { pl::ret_undef() };
        }
        m(it, &et, "Warn", vec![err])?;
    }
    if outfile.truthy() && !raf.truthy() {
        return ret(out_buff.borrow().clone());
    }
    ret(n(1))
}

/// Return bits from a bitstream object
/// Inputs: 0) array ref, 1) number of bits
/// Returns: specified number of bits as an integer, and shifts input bitstream
pub fn get_bits(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (arr, mut nb) = (arg(&a, 0), arg(&a, 1).to_i64());
    let mut v: i64 = 0;
    let mut bit: i64 = 1;
    while nb != 0 {
        nb -= 1;
        let mut i = 0;
        pl::at(1372);
        pl::aderef(&arr)?; // ($i<@$a dies unless $a is an ARRAY ref)
        while i < arr.len() {
            // consume bits LSB first
            let x = arr.at(i as i64).to_i64();
            let set = x & 1;
            arr.set_at(i, n(((x as u64) >> 1) as i64));
            if i != 0 {
                if set != 0 {
                    let p = arr.at(i as i64 - 1).to_i64();
                    arr.set_at(i - 1, n(p | 0x80));
                }
            } else {
                if set != 0 {
                    v |= bit;
                }
                bit <<= 1;
            }
            i += 1;
        }
    }
    ret(n(v))
}

fn get_bits1(it: &mut Interp, arr: &Value, nb: Value) -> ER<Value> {
    Ok(get_bits(it, vec![arr.clone(), nb], Want::Scalar)?.pop().unwrap_or_default())
}

/// Read/write Brotli-encoded metadata
/// Inputs: 0) ExifTool ref, 1) dirInfoRef, 2) tag table ref
/// Returns: 1 on success when reading, or new data when writing (undef if unchanged)
/// (ref https://libjxl.readthedocs.io/en/latest/api_decoder.html)
pub fn process_brotli(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");

    pl::at(1397);
    if !pl::gt(&pl::length(&pl::sderef(&data_pt)?), &n(4)) {
        return ret(n(0));
    }

    let is_writing = dir_info.get("IsWriting");
    let mut type_ = pl::substr(&pl::sderef(&data_pt)?, 0, Some(4));
    if !is_writing.truthy() {
        m(it, &et, "VerboseDir", vec![cat(&[&pl::s("Decrypted Brotli '"), &type_, &pl::s("'")])])?;
    }
    let lt = pl::lc(&type_);
    let std_type = match &*lt.to_bytes() {
        b"exif" => pl::s("Exif"),
        b"xml " => pl::s("xml "),
        b"jumb" => pl::s("jumb"),
        _ => Value::Undef,
    };
    if !std_type.truthy() {
        m(it, &et, "Warn", vec![pl::s("Unknown Brotli box type"), n(1)])?;
        return ret(n(1));
    }
    if type_.to_bytes() != std_type.to_bytes() {
        m(it, &et, "Warn", vec![cat(&[&pl::s("Incorrect case for Brotli '"), &type_, &pl::s("' data (should be '"), &std_type, &pl::s("')")])])?;
        type_ = std_type;
    }
    if try_require(it, "IO::Uncompress::Brotli")? {
        if is_writing.truthy() && !try_require(it, "IO::Compress::Brotli")? {
            m(it, &et, "Warn", vec![pl::s("Install IO::Compress::Brotli to write Brotli-compressed metadata")])?;
            return pl::ret_undef();
        }
        let compress = m(it, &et, "Options", vec![pl::s("Compress")])?;
        let verbose = if is_writing.truthy() { n(0) } else { m(it, &et, "Options", vec![pl::s("Verbose")])? };
        let dat = cell(pl::substr(&pl::sderef(&data_pt)?, 4, None));
        let d0 = dat.borrow().clone();
        if let Some(d) = eval_call(it, "IO::Uncompress::Brotli::unbro", vec![d0, n(100000000)])? {
            *dat.borrow_mut() = d;
        }
        if it.get_scalar("main::@").truthy() {
            brotli_warn1(it, &et, &type_, true)?;
            return ret(n(1));
        }
        if pl::gt(&verbose, &n(2)) {
            m(it, &et, "VerboseDump", vec![Value::ScalarRef(dat.clone()), pl::s("Prefix"), pl::concat(&et.get("INDENT"), &pl::s("  "))])?;
        }
        let di = Value::new_hash();
        di.set("DataPt", Value::ScalarRef(dat.clone()));
        if type_.eq_str("xml ") {
            di.set("DirName", pl::s("XMP")); // (necessary for block read/write)
            crate::runtime::require(it, "Image::ExifTool::XMP")?;
            if is_writing.truthy() {
                if compress.truthy() {
                    di.set("Compact", n(1)); // (no need to add padding if writing compressed)
                }
                let w = m(it, &et, "WriteDirectory", vec![di.clone(), tag_table_ptr.clone()])?;
                *dat.borrow_mut() = w;
            } else {
                pl::call(it, "Image::ExifTool::XMP::ProcessXMP", vec![et.clone(), di.clone(), tag_table_ptr.clone()])?;
            }
        } else if type_.eq_str("Exif") {
            di.set("DirName", pl::s("EXIF")); // (necessary for block read/write)
            let d = dat.borrow().clone();
            let off = if pl::gt(&pl::length(&d), &n(4)) { pl::unpack1("N", &d)? } else { n(0) };
            di.set("DirStart", pl::add(&n(4), &off));
            if pl::gt(&di.get("DirStart"), &pl::length(&dat.borrow())) {
                m(it, &et, "Warn", vec![cat(&[&pl::s("Corrupted Brotli '"), &type_, &pl::s("' data")])])?;
            } else if is_writing.truthy() {
                let wt = Value::Code(it.sub_ref("Image::ExifTool::WriteTIFF"));
                let mut w = m(it, &et, "WriteDirectory", vec![di.clone(), tag_table_ptr.clone(), wt])?;
                // add back header word
                if !w.is_undef() && pl::length(&w).truthy() {
                    w = pl::concat(&Value::bytes(b"\0\0\0\0"), &w);
                }
                *dat.borrow_mut() = w;
            } else {
                m(it, &et, "ProcessTIFF", vec![di.clone(), tag_table_ptr.clone()])?;
            }
        } else if type_.eq_str("jumb") {
            if is_writing.truthy() {
                return pl::ret_undef(); // (can't yet write JUMBF)
            }
            process_jumb(it, vec![et.clone(), di.clone(), tag_table_ptr.clone()], Want::Scalar)?; // (untested)
        }
        if is_writing.truthy() {
            let d = dat.borrow().clone();
            if d.is_undef() {
                return pl::ret_undef();
            }
            // rewrite as uncompressed if Compress option is set to 0 (or '')
            if !compress.is_undef() && !compress.truthy() {
                return ret(d);
            }
            let c = eval_call(it, "IO::Compress::Brotli::bro", vec![d.clone()])?;
            let d = c.unwrap_or(d);
            if it.get_scalar("main::@").truthy() {
                brotli_warn1(it, &et, &type_, false)?;
                return pl::ret_undef();
            }
            m(it, &et, "VPrint", vec![n(0), cat(&[&pl::s("  Writing Brotli-compressed "), &type_, &pl::s("\n")])])?;
            return ret(pl::concat(&type_, &d));
        }
    } else {
        m(it, &et, "Warn", vec![pl::s("Install IO::Uncompress::Brotli to decode Brotli-compressed metadata")])?;
        if is_writing.truthy() {
            return pl::ret_undef();
        }
    }
    ret(n(1))
}

/// Extract parameters from JPEG XL codestream [unverified!]
/// Inputs: 0) ExifTool ref, 1) codestream ref
/// Returns: 1 on success
pub fn process_jxl_codestream(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, data_pt) = (arg(&a, 0), arg(&a, 1));
    pl::at(1473);
    let data = pl::sderef(&data_pt)?;

    if !re(r"^(\0\0\0\0)?\xff\x0a", "").is_match(&data) {
        return ret(n(0)); // validate codestream
    }
    // ignore if already extracted (ie. subsequent jxlp boxes)
    if et.get("ProcessedJXLCodestream").truthy() {
        return ret(n(0));
    }
    et.set("ProcessedJXLCodestream", n(1));
    // work with first 64 bytes of codestream data
    // (and add padding if necessary to avoid unpacking past end of data)
    let mut dat = if pl::gt(&pl::length(&data), &n(64)) {
        pl::substr(&data, 0, Some(64))
    } else if pl::lt(&pl::length(&data), &n(18)) {
        pl::concat(&data, &Value::bytes([0u8; 18])) // (so we'll have a minimum 14 bytes to work with)
    } else {
        data.clone()
    };
    re(r"^\0\0\0\0", "").subst(&mut dat, false, |_| Vec::new()); // remove jxlp header word
    let arr = Value::new_array(pl::unpack("x2C12", &dat)?);
    let (x, y);
    let small = get_bits1(it, &arr, n(1))?;
    if small.truthy() {
        y = pl::mul(&pl::add(&get_bits1(it, &arr, n(5))?, &n(1)), &n(8));
    } else {
        let sel = get_bits1(it, &arr, n(2))?;
        let nb = [9, 13, 18, 30].get(sel.to_i64() as usize).copied().map_or(Value::Undef, n);
        y = pl::add(&get_bits1(it, &arr, nb)?, &n(1));
    }
    let ratio = get_bits1(it, &arr, n(3))?;
    if pl::num_eq(&ratio, &n(0)) {
        if small.truthy() {
            x = pl::mul(&pl::add(&get_bits1(it, &arr, n(5))?, &n(1)), &n(8));
        } else {
            let sel = get_bits1(it, &arr, n(2))?;
            let nb = [9, 13, 18, 30].get(sel.to_i64() as usize).copied().map_or(Value::Undef, n);
            x = pl::add(&get_bits1(it, &arr, nb)?, &n(1));
        }
    } else {
        let r = [[1, 1], [12, 10], [4, 3], [3, 2], [16, 9], [5, 4], [2, 1]][(ratio.to_i64() - 1) as usize];
        x = pl::int(&pl::div(&pl::mul(&y, &n(r[0])), &n(r[1]))?);
    }
    m(it, &et, "FoundTag", vec![pl::s("ImageWidth"), x])?;
    m(it, &et, "FoundTag", vec![pl::s("ImageHeight"), y])?;
    ret(n(1))
}

/// Read/write meta information from a C2PA/JUMBF file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid JUMBF file
pub fn process_jumbf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let mut hdr = Value::Undef;

    // check to be sure this is a valid JPG2000 file
    pl::at(1523);
    if !(read_eq(it, &raf, &mut hdr, &n(20))? && seek(it, &raf, n(0), 0)?) {
        return ret(n(0));
    }
    let Some(c) = re(r"^.{4}jumb\0.{3}jumd(.{4})", "").m(&hdr) else {
        return ret(n(0));
    };
    m(it, &et, "SetFileType", vec![pl::s(if cap(&c, 1).eq_str("c2pa") { "C2PA" } else { "JUMBF" })])?;
    let di = Value::new_hash();
    di.set("RAF", raf.clone());
    di.set("DirName", pl::s("JUMBF"));
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Jpeg2000::Main")])?;
    ret(m(it, &et, "ProcessDirectory", vec![di, tag_table_ptr])?)
}

/// Read/write meta information from a JPEG 2000 image
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid JPEG 2000 file, or -1 on write error
pub fn process_jp2(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| {
        let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
        let raf = dir_info.get("RAF");
        let outfile = dir_info.get("OutFile");
        let mut hdr = Value::Undef;

        // check to be sure this is a valid JPG2000 file
        pl::at(1547);
        if !read_eq(it, &raf, &mut hdr, &n(12))? {
            return ret(n(0));
        }
        let h = hdr.to_bytes();
        if !(&h[..] == b"\0\0\0\x0cjP  \x0d\x0a\x87\x0a" // (ref 1)
            || &h[..] == b"\0\0\0\x0cjP\x1a\x1a\x0d\x0a\x87\x0a" // (ref 2)
            || et.get("IsJXL").truthy())
        {
            if !re(r"^\xff\x4f\xff\x51\0", "").is_match(&hdr) {
                return ret(n(0)); // check for JP2 codestream format
            }
            if outfile.truthy() {
                m(it, &et, "Error", vec![pl::s("Writing of J2C files is not yet supported")])?;
                return ret(n(0));
            }
            // add J2C markers if not done already
            let jpeg_marker = pl::pkg_hash(it, "Image::ExifTool::jpegMarker");
            if !jpeg_marker.get("79").truthy() {
                let j2c_marker = crate::runtime::lex_hash(it, MODULE, "j2cMarker");
                for k in j2c_marker.keys() {
                    it.set_scalar("main::_", Value::bytes(&k[..]));
                    jpeg_marker.set(&k[..], j2c_marker.get(&k));
                }
            }
            m(it, &et, "SetFileType", vec![pl::s("J2C")])?;
            seek(it, &raf, n(0), 0)?;
            return ret(m(it, &et, "ProcessJPEG", vec![dir_info.clone()])?); // decode with JPEG processor
        }
        if outfile.truthy() {
            if !write(it, &outfile, &[hdr.clone()])? {
                return ret(n(-1));
            }
            if et.get("IsJXL").truthy() {
                let jxl_map = crate::runtime::lex_hash(it, MODULE, "jxlMap");
                m(it, &et, "InitWriteDirs", vec![jxl_map])?;
                et.set("AddJp2Tags", Value::new_hash()); // (don't add JP2 tags in JXL files)
            } else {
                let jp2_map = crate::runtime::lex_hash(it, MODULE, "jp2Map");
                m(it, &et, "InitWriteDirs", vec![jp2_map])?;
                let main = pl::pkg_hash(it, "Image::ExifTool::Jpeg2000::Main");
                let h = m(it, &et, "GetNewTagInfoHash", vec![main])?;
                et.set("AddJp2Tags", h);
            }
            // save list of directories to create
            let add_dirs = et.get("ADD_DIRS").hash_copy(); // (make a copy)
            et.set("AddJp2Dirs", add_dirs);
        } else {
            let mut buff = Value::Undef;
            let mut file_type = Value::Undef;
            // recognize JPX and JPM as unique types of JP2
            if read_eq(it, &raf, &mut buff, &n(12))? {
                if let Some(c) = re(r"^.{4}ftyp(.{4})", "s").m(&buff) {
                    let t = cap(&c, 1);
                    if t.eq_str("jpx ") {
                        file_type = pl::s("JPX");
                    }
                    if t.eq_str("jpm ") {
                        file_type = pl::s("JPM");
                    }
                    if t.eq_str("jxl ") {
                        file_type = pl::s("JXL");
                    }
                    if t.eq_str("jph ") {
                        file_type = pl::s("JPH");
                    }
                }
            }
            if !buff.is_undef() {
                seek(it, &raf, pl::neg(&pl::length(&buff)), 1)?;
            }
            m(it, &et, "SetFileType", vec![file_type])?;
        }
        call(it, "SetByteOrder", vec![pl::s("MM")])?; // JPEG 2000 files are big-endian
        let di = Value::new_hash();
        di.set("RAF", raf.clone());
        di.set("DirName", pl::s("JP2"));
        di.set("OutFile", dir_info.get("OutFile"));
        let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Jpeg2000::Main")])?;
        ret(m(it, &et, "ProcessDirectory", vec![di, tag_table_ptr])?)
    })
}

/// Read/write meta information in a JPEG XL image
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid JPEG XL file, -1 on write error
pub fn process_jxl(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let outfile = dir_info.get("OutFile");
    let (mut hdr, mut buff) = (Value::Undef, Value::Undef);

    pl::at(1610);
    if !read_eq(it, &raf, &mut hdr, &n(12))? {
        return ret(n(0));
    }
    if &hdr.to_bytes()[..] == b"\0\0\0\x0cJXL \x0d\x0a\x87\x0a" {
        // JPEG XL in ISO BMFF container
        et.set("IsJXL", n(1));
    } else if re(r"^\xff\x0a", "").is_match(&hdr) {
        // JPEG XL codestream
        if outfile.truthy() {
            if et.path(&["OPTIONS", "IgnoreMinorErrors"]).truthy() {
                m(it, &et, "Warn", vec![pl::s("Wrapped JXL codestream in ISO BMFF container")])?;
            } else {
                m(it, &et, "Error", vec![pl::s("Will wrap JXL codestream in ISO BMFF container for writing"), n(1)])?;
                return ret(n(0));
            }
            et.set("IsJXL", n(2));
            let buff = pl::sref(Value::bytes(b"\0\0\0\x0cJXL \x0d\x0a\x87\x0a\0\0\0\x14ftypjxl \0\0\0\0jxl "));
            // add metadata to empty ISO BMFF container
            let r = m(it, &pl::s("File::RandomAccess"), "new", vec![buff])?;
            dir_info.set("RAF", r);
        } else {
            m(it, &et, "SetFileType", vec![pl::s("JXL Codestream"), pl::s("image/jxl"), pl::s("jxl")])?;
            if et.get("ImageDataHash").truthy() && seek(it, &raf, n(0), 0)? {
                m(it, &et, "ImageDataHash", vec![raf.clone(), Value::Undef, pl::s("JXL")])?;
            }
            return process_jxl_codestream(it, vec![et.clone(), pl::sref(hdr.clone())], Want::Scalar);
        }
    } else {
        return ret(n(0));
    }
    if !seek(it, &raf, n(0), 0)? {
        m(it, &et, "Error", vec![pl::s("Seek error")])?;
        return ret(n(0));
    }

    let success = process_jp2(it, vec![et.clone(), dir_info.clone()], Want::Scalar)?.pop().unwrap_or_default();

    if outfile.truthy() && pl::gt(&success, &n(0)) && pl::num_eq(&et.get("IsJXL"), &n(2)) {
        // attach the JXL codestream box to the ISO BMFF file
        if !seek(it, &raf, n(0), 2)? {
            return ret(n(-1));
        }
        let size = m(it, &raf, "Tell", vec![])?;
        if !seek(it, &raf, n(0), 0)? {
            return ret(n(-1));
        }
        call(it, "SetByteOrder", vec![pl::s("MM")])?;
        let s32 = call(it, "Set32u", vec![pl::add(&size, &n(8))])?;
        if !write(it, &outfile, &[s32, pl::s("jxlc")])? {
            return ret(n(-1));
        }
        while read(it, &raf, &mut buff, n(65536))?.truthy() {
            if !write(it, &outfile, &[buff.clone()])? {
                return ret(n(-1));
            }
        }
    }
    ret(success)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Jpeg2000::ProcessJUMB", process_jumb);
    it.define_native("Image::ExifTool::Jpeg2000::ProcessJUMD", process_jumd);
    it.define_native("Image::ExifTool::Jpeg2000::BrotliWarn", brotli_warn);
    it.define_native("Image::ExifTool::Jpeg2000::CreateNewBoxes", create_new_boxes);
    it.define_native("Image::ExifTool::Jpeg2000::CreateColorSpec", create_color_spec);
    it.define_native("Image::ExifTool::Jpeg2000::ProcessJpeg2000Box", process_jpeg2000_box);
    it.define_native("Image::ExifTool::Jpeg2000::GetBits", get_bits);
    it.define_native("Image::ExifTool::Jpeg2000::ProcessBrotli", process_brotli);
    it.define_native("Image::ExifTool::Jpeg2000::ProcessJXLCodestream", process_jxl_codestream);
    it.define_native("Image::ExifTool::Jpeg2000::ProcessJUMBF", process_jumbf);
    it.define_native("Image::ExifTool::Jpeg2000::ProcessJP2", process_jp2);
    it.define_native("Image::ExifTool::Jpeg2000::ProcessJXL", process_jxl);
}
