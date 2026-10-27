//! Image::ExifTool::Real (Image/ExifTool/Real.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Real audio/video meta information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell, raf_read_line, substr};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

const MODULE: &str = "Image::ExifTool::Real";

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

fn warn(it: &mut Interp, et: &Value, msg: Value) -> ER<()> {
    pl::method(it, et, "Warn", vec![msg])?;
    Ok(())
}

/// `unpack("x${pos}TEMPLATE", $data)`
fn unpack_at(pos: &Value, t: &str, data: &Value) -> ER<Vec<Value>> {
    pl::unpack(&format!("x{}{}", pos.to_text(), t), data)
}

/// `{ Name => $name }`
fn name_hash(name: Value) -> Value {
    let h = Value::new_hash();
    h.set("Name", name);
    h
}

/// `chomp $buff` (removes the trailing `$/`; all trailing newlines in
/// paragraph mode; nothing when `$/` is undefined).
fn chomp(it: &mut Interp, v: &mut Value) {
    if v.is_undef() {
        return;
    }
    let sep = it.get_scalar("main::/");
    if sep.is_undef() {
        return;
    }
    let s = v.to_bytes();
    let sp = sep.to_bytes();
    if sp.is_empty() {
        let k = s.iter().rev().take_while(|&&c| c == b'\n').count();
        if k > 0 {
            *v = Value::bytes(&s[..s.len() - k]);
        }
    } else if s.ends_with(&sp) {
        *v = Value::bytes(&s[..s.len() - sp.len()]);
    }
}

/// Process Real NameValueProperties
/// Inputs: 0) ExifTool object reference, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_real_properties(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let dir_len = dir_info.get("DirLen");
    let mut pos = dir_info.get("DirStart");
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let property_type = crate::runtime::lex_hash(it, MODULE, "propertyType");

    if verbose.truthy() {
        pl::method(it, &et, "VerboseDir", vec![pl::s("RealProperties"), Value::Undef, dir_len.clone()])?;
    }

    while pl::le(&pl::add(&pos, &n(6)), &dir_len) {
        let data = data_pt.deref();

        // get property size and version
        let u = unpack_at(&pos, "Nn", &data)?;
        let (size, vers) = (arg(&u, 0), arg(&u, 1));
        if pl::lt(&size, &n(6)) {
            break;
        }
        if !pl::num_eq(&vers, &n(0)) {
            pos = pl::add(&pos, &size);
            continue;
        }
        pos = pl::add(&pos, &n(6));

        let tag_len = pl::unpack1(&format!("x{}C", pos.to_text()), &data)?;
        pos = pl::add(&pos, &n(1));

        if pl::gt(&pl::add(&pos, &tag_len), &dir_len) {
            break;
        }
        let tag = substr(it, &data, &pos, Some(&tag_len))?;
        pos = pl::add(&pos, &tag_len);

        if pl::gt(&pl::add(&pos, &n(6)), &dir_len) {
            break;
        }
        let u = unpack_at(&pos, "Nn", &data)?;
        let (type_, val_len) = (arg(&u, 0), arg(&u, 1));
        pos = pl::add(&pos, &n(6));

        if pl::gt(&pl::add(&pos, &val_len), &dir_len) {
            break;
        }
        let pt = property_type.get(type_.to_bytes());
        let format = if pt.truthy() { pt } else { pl::s("undef") };
        let fsize = call(it, "FormatSize", vec![format.clone()])?;
        let count = pl::int(&pl::div(&val_len, &fsize)?);
        let val = call(it, "ReadValue", vec![data_pt.clone(), pos.clone(), format.clone(), count.clone(), pl::sub(&dir_len, &pos)])?;

        let mut tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
        if !tag_info.truthy() {
            let mut tag_name = tag.clone();
            re(r"\s+", "").subst(&mut tag_name, true, |_| Vec::new());
            if !re(r"^\w+$", "").is_match(&tag_name) {
                continue; // ignore crazy names
            }
            tag_info = name_hash(pl::ucfirst(&tag_name));
            call(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info.clone()])?;
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
                    pl::s("Size"),
                    val_len.clone(),
                    pl::s("Start"),
                    pos.clone(),
                    pl::s("Addr"),
                    pl::add(&pos, &dir_info.get("DataPos")),
                    pl::s("Format"),
                    format.clone(),
                    pl::s("Count"),
                    count.clone(),
                ],
            )?;
        }
        pl::method(it, &et, "FoundTag", vec![tag_info, val])?;
        pos = pl::add(&pos, &val_len);
    }
    ret(n(1))
}

/// Process Real metadata properties
/// Inputs: 0) ExifTool object reference, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_real_meta(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let data_pos = dir_info.get("DataPos");
    let mut pos = dir_info.get("DirStart");
    let dir_end = pl::add(&pos, &dir_info.get("DirLen"));
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let p = dir_info.get("Prefix");
    let mut prefix = if p.truthy() { p } else { pl::s("") };
    if prefix.truthy() {
        prefix = pl::concat(&prefix, &pl::s("/"));
    }
    let metadata_format = crate::runtime::lex_hash(it, MODULE, "metadataFormat");

    if verbose.truthy() {
        pl::method(it, &et, "VerboseDir", vec![pl::s("RealMetadata"), Value::Undef, dir_info.get("DirLen")])?;
    }

    loop {
        if pl::gt(&pl::add(&pos, &n(28)), &dir_end) {
            break;
        }
        let data = data_pt.deref();
        // extract fixed-position metadata structure members
        let u = unpack_at(&pos, "N7", &data)?;
        let (size, type_, mut flags, mut value_pos, mut sub_prop_pos, num_sub_props, name_len) = (arg(&u, 0), arg(&u, 1), arg(&u, 2), arg(&u, 3), arg(&u, 4), arg(&u, 5), arg(&u, 6));
        // make pointers relative to data start
        value_pos = pl::add(&value_pos, &pos);
        sub_prop_pos = pl::add(&sub_prop_pos, &pos);
        let _ = &sub_prop_pos;
        // validate what we have read so far
        if pl::gt(&pl::add(&pos, &size), &dir_end) {
            break;
        }
        if pl::gt(&pl::add(&pl::add(&pos, &n(28)), &name_len), &dir_end) {
            break;
        }
        if pl::lt(&value_pos, &pl::add(&pl::add(&pos, &n(28)), &name_len)) {
            break;
        }
        if pl::gt(&pl::add(&value_pos, &n(4)), &dir_end) {
            break;
        }
        let mut tag = substr(it, &data, &pl::add(&pos, &n(28)), Some(&name_len))?;
        re(r"\0.*", "s").subst(&mut tag, false, |_| Vec::new()); // truncate at null
        tag = pl::concat(&prefix, &tag);
        let value_len = pl::unpack1(&format!("x{}N", value_pos.to_text()), &data)?;
        value_pos = pl::add(&value_pos, &n(4)); // point at value itself
        if pl::gt(&pl::add(&value_pos, &value_len), &dir_end) {
            break;
        }

        let mut format = metadata_format.get(type_.to_bytes());
        let mut tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
        if !tag_info.truthy() {
            let mut tag_name = tag.clone();
            pl::tr(&mut tag_name, "A-Za-z0-9", "", "dc");
            tag_info = name_hash(pl::ucfirst(&tag_name));
            call(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), tag_info.clone()])?;
        }
        if verbose.truthy() {
            if format.is_undef() {
                format = pl::s("undef");
            }
            let metadata_flag = crate::runtime::lex_hash(it, MODULE, "metadataFlag");
            flags = pl::call(it, "Image::ExifTool::DecodeBits", vec![flags, metadata_flag])?;
        }
        if value_len.truthy() && format.truthy() {
            // (a flag can be 1 or 4 bytes)
            if format.eq_str("flag") {
                format = pl::s(if pl::num_eq(&value_len, &n(4)) { "int32u" } else { "int8u" });
            } else if pl::num_eq(&type_, &n(7)) && tag_info.truthy() {
                // add PrintConv and ValueConv for "date" type
                if !tag_info.get("ValueConv").truthy() {
                    tag_info.set(
                        "ValueConv",
                        pl::s("\n                    $val =~ /^(\\d{4})(\\d{2})(\\d{2})(\\d{2})(\\d{2})(\\d{2})/ ?\n                    sprintf(\"%.4d:%.2d:%.2d %.2d:%.2d:%.2d\",$1,$2,$3,$4,$5,$6) :\n                    $val;\n                "),
                    );
                }
                if !tag_info.get("PrintConv").truthy() {
                    tag_info.set("PrintConv", pl::s("$self->ConvertDateTime($val)"));
                }
            }
            let fsize = call(it, "FormatSize", vec![format.clone()])?;
            let count = pl::int(&pl::div(&value_len, &fsize)?);
            let val = call(it, "ReadValue", vec![data_pt.clone(), value_pos.clone(), format.clone(), count, pl::sub(&dir_end, &value_pos)])?;
            let fmt = pl::concat(&pl::concat(&pl::concat(&pl::s("type="), &type_), &pl::s(", flags=")), &flags);
            pl::method(
                it,
                &et,
                "HandleTag",
                vec![tag_table_ptr.clone(), tag.clone(), val, pl::s("DataPt"), data_pt.clone(), pl::s("DataPos"), data_pos.clone(), pl::s("Start"), value_pos.clone(), pl::s("Size"), value_len.clone(), pl::s("Format"), fmt],
            )?;
        }
        // extract sub-properties
        if num_sub_props.truthy() {
            let dir_start = pl::add(&pl::add(&value_pos, &value_len), &pl::mul(&num_sub_props, &n(8)));
            let d = Value::new_hash();
            d.set("DataPt", data_pt.clone());
            d.set("DataPos", data_pos.clone());
            d.set("DirStart", dir_start.clone());
            d.set("DirLen", pl::sub(&pl::add(&pos, &size), &dir_start));
            d.set("Prefix", tag.clone());
            pl::method(it, &et, "ProcessDirectory", vec![d, tag_table_ptr.clone()])?;
        }
        pos = pl::add(&pos, &size); // step to next Metadata structure
    }
    if !pl::num_eq(&pos, &dir_end) {
        warn(it, &et, pl::s("Format error in Real Metadata"))?;
        return ret(n(0));
    }
    ret(n(1))
}

/// Read information frame a Real file
/// Inputs: 0) ExifTool object reference, 1) Directory information reference
/// Returns: 1 on success, 0 if this wasn't a valid Real file
pub fn process_real(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);
    let buff_ref = Value::ScalarRef(buff.clone());
    let b = || buff.borrow().clone();
    let mut mime_types: Vec<Value> = Vec::new();
    let dir_count = Value::new_hash();

    if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(8))?, &n(8)) {
        return ret(n(0));
    }
    let Some(m) = re(r"^(\.RMF|\.ra\xfd|pnm://|rtsp://|http://)", "").m(&b()) else {
        return ret(n(0));
    };

    let fs = et.get("OPTIONS").get("FastScan");
    let fast3 = fs.truthy() && pl::num_eq(&fs, &n(3));
    let (type_, tag_table_ptr);
    let c1 = cap(&m, 1);
    if c1.eq_str(".RMF") {
        tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Real::Media")])?;
        type_ = pl::s("RM");
    } else if &c1.to_bytes()[..] == b".ra\xfd" {
        tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Real::Audio")])?;
        type_ = pl::s("RA");
    } else {
        let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Real::Metafile")])?;
        let ext = et.get("FILE_EXT");
        let type_ = pl::s(if ext.truthy() && ext.eq_str("RPM") { "RPM" } else { "RAM" });
        crate::runtime::require(it, "Image::ExifTool::PostScript")?;
        let sep = pl::call(it, "Image::ExifTool::PostScript::GetInputRecordSeparator", vec![raf.clone()])?;
        let sep = if sep.truthy() { sep } else { pl::s("\n") };
        return pl::local_global(it, b'$', "main::/", |it| {
            it.set_scalar("main::/", sep);
            let mut type_ = type_;
            pl::method(it, &raf, "Seek", vec![n(0), n(0)])?;
            let mut buff = Value::Undef;
            while raf_read_line(it, &raf, &mut buff)?.truthy() {
                if pl::gt(&pl::length(&buff), &n(256)) {
                    break;
                }
                if !buff.truthy() {
                    continue;
                }
                chomp(it, &mut buff);
                if type_.truthy() {
                    // must be a Real file type if protocol is http
                    if re("^http", "").is_match(&buff) && !re(r"\.(ra|rm|rv|rmvb|smil)$", "i").is_match(&buff) {
                        return ret(n(0));
                    }
                    pl::method(it, &et, "SetFileType", vec![type_.clone()])?;
                    if fast3 {
                        return ret(n(1));
                    }
                    type_ = Value::Undef;
                }
                // save URL or Text from RAM file
                let tag = pl::s(if re("^[a-z]{3,4}://", "").is_match(&buff) { "url" } else { "txt" });
                pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), tag, buff.clone()])?;
            }
            ret(n(1))
        });
    }

    pl::method(it, &et, "SetFileType", vec![type_.clone()])?;
    if fast3 {
        return ret(n(1));
    }
    call(it, "SetByteOrder", vec![pl::s("MM")])?;
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    //
    // Process RealAudio file
    //
    if type_.eq_str("RA") {
        let u = pl::unpack("x4nn", &b())?;
        let (vers, _extra) = (arg(&u, 0), arg(&u, 1));
        let tag = pl::concat(&pl::s(".ra"), &vers);
        let fpos = pl::method(it, &raf, "Tell", vec![])?;
        if !raf_read_cell(it, &raf, &buff, n(512))?.truthy() {
            warn(it, &et, pl::s("Error reading audio header"))?;
            return ret(n(1));
        }
        let tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
        if pl::gt(&verbose, &n(2)) {
            pl::method(it, &et, "VerboseInfo", vec![tag.clone(), tag_info.clone(), pl::s("DataPt"), buff_ref.clone(), pl::s("DataPos"), fpos.clone()])?;
        }
        if tag_info.truthy() {
            let sub_table_ptr = call(it, "GetTagTable", vec![tag_info.get("SubDirectory").get("TagTable")])?;
            let d = Value::new_hash();
            d.set("DataPt", buff_ref.clone());
            d.set("DataPos", fpos);
            d.set("DirLen", pl::length(&b()));
            d.set("DirStart", n(0));
            pl::method(it, &et, "ProcessDirectory", vec![d, sub_table_ptr])?;
        } else {
            warn(it, &et, pl::s("Unsupported RealAudio version"))?;
        }
        return ret(n(1));
    }
    //
    // Process RealMedia file
    //
    // skip the rest of the RM header
    let mut size = pl::unpack1("x4N", &b())?;
    if !pl::method(it, &raf, "Seek", vec![pl::sub(&size, &n(8)), n(1)])?.truthy() {
        warn(it, &et, pl::s("Error seeking in file"))?;
        return ret(n(0));
    }

    // Process RealMedia chunks
    loop {
        if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(10))?, &n(10)) {
            break;
        }
        let u = pl::unpack("a4Nn", &b())?;
        let tag = arg(&u, 0);
        size = arg(&u, 1);
        let _vers = arg(&u, 2);
        if tag.eq_str("\0\0\0\0") {
            break;
        }
        if verbose.truthy() {
            let msg = pl::concat(&pl::concat(&pl::concat(&tag, &pl::s(" chunk (")), &size), &pl::s(" bytes):\n"));
            pl::method(it, &et, "VPrint", vec![n(0), msg])?;
        } else if tag.eq_str("DATA") {
            break; // stop normal parsing at DATA tag
        }
        if size.to_i64() & 0x80000000 != 0 || pl::lt(&size, &n(10)) {
            warn(it, &et, pl::s("Bad chunk header"))?;
            break;
        }
        let tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])?;
        if tag_info.truthy() && tag_info.get("SubDirectory").truthy() {
            let fpos = pl::method(it, &raf, "Tell", vec![])?;
            let len = pl::sub(&size, &n(10));
            if !pl::num_eq(&raf_read_cell(it, &raf, &buff, len.clone())?, &len) {
                warn(it, &et, pl::concat(&pl::concat(&pl::s("Error reading "), &tag), &pl::s(" chunk")))?;
                break;
            }
            if pl::gt(&verbose, &n(2)) {
                pl::method(it, &et, "VerboseInfo", vec![tag.clone(), tag_info.clone(), pl::s("DataPt"), buff_ref.clone(), pl::s("DataPos"), fpos.clone()])?;
            }
            let sub_table_ptr = call(it, "GetTagTable", vec![tag_info.get("SubDirectory").get("TagTable")])?;
            let d = Value::new_hash();
            d.set("DataPt", buff_ref.clone());
            d.set("DataPos", fpos);
            d.set("DirLen", pl::length(&b()));
            d.set("DirStart", n(0));
            let cnt = dir_count.get(tag.to_bytes());
            if cnt.truthy() {
                let c = pl::add(&cnt, &n(1));
                dir_count.set(tag.to_bytes(), c.clone());
                et.set("SET_GROUP1", pl::concat(&pl::s("+"), &c));
            } else {
                dir_count.set(tag.to_bytes(), n(1));
            }
            pl::method(it, &et, "ProcessDirectory", vec![d, sub_table_ptr])?;
            et.delete("SET_GROUP1");
            // keep track of stream MIME types
            let mut mime = et.get("RealStreamMime");
            if mime.truthy() {
                et.delete("RealStreamMime");
                re(r"\0.*", "s").subst(&mut mime, false, |_| Vec::new());
                if !re("^logical-", "").is_match(&mime) {
                    mime_types.push(mime);
                }
            }
        } else if !pl::method(it, &raf, "Seek", vec![pl::sub(&size, &n(10)), n(1)])?.truthy() {
            warn(it, &et, pl::s("Error seeking in file"))?;
            break;
        }
    }
    // override MIMEType with stream MIME type if we only have one stream
    if mime_types.len() == 1 && pl::length(&mime_types[0]).truthy() {
        et.get("VALUE").set("MIMEType", mime_types[0].clone());
        let msg = pl::concat(&pl::concat(&pl::s("  MIMEType = "), &mime_types[0]), &pl::s("\n"));
        pl::method(it, &et, "VPrint", vec![n(0), msg])?;
    }
    //
    // Process footer containing Real metadata and ID3 information
    //
    if pl::method(it, &raf, "Seek", vec![n(-140), n(2)])?.truthy() && pl::num_eq(&raf_read_cell(it, &raf, &buff, n(12))?, &n(12)) && re("^RMJE", "").is_match(&b()) {
        let meta_size = pl::unpack1("x8N", &b())?;
        if pl::method(it, &raf, "Seek", vec![pl::sub(&pl::neg(&meta_size), &n(12)), n(1)])?.truthy()
            && pl::num_eq(&raf_read_cell(it, &raf, &buff, meta_size.clone())?, &meta_size)
            && re("^RJMD", "").is_match(&b())
        {
            let d = Value::new_hash();
            d.set("DataPt", buff_ref.clone());
            d.set("DataPos", pl::sub(&pl::method(it, &raf, "Tell", vec![])?, &meta_size));
            d.set("DirStart", n(8));
            d.set("DirLen", pl::sub(&pl::length(&b()), &n(8)));
            let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::Real::Metadata")])?;
            pl::method(it, &et, "ProcessDirectory", vec![d, tag_table_ptr])?;
        } else {
            warn(it, &et, pl::s("Bad metadata footer"))?;
        }
        if pl::method(it, &raf, "Seek", vec![n(-128), n(2)])?.truthy() && pl::num_eq(&raf_read_cell(it, &raf, &buff, n(128))?, &n(128)) && re("^TAG", "").is_match(&b()) {
            pl::method(it, &et, "VPrint", vec![n(0), pl::s("ID3v1:\n")])?;
            let d = Value::new_hash();
            d.set("DataPt", buff_ref.clone());
            d.set("DirStart", n(0));
            d.set("DirLen", pl::length(&b()));
            let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::ID3::v1")])?;
            pl::method(it, &et, "ProcessDirectory", vec![d, tag_table_ptr])?;
        }
    }
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Real::ProcessRealProperties", process_real_properties);
    it.define_native("Image::ExifTool::Real::ProcessRealMeta", process_real_meta);
    it.define_native("Image::ExifTool::Real::ProcessReal", process_real);
}
