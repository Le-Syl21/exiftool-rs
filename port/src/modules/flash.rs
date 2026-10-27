//! Image::ExifTool::Flash (Image/ExifTool/Flash.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Shockwave Flash (SWF) and Flash Video (FLV) meta information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, n, re};

const MODULE: &str = "Image::ExifTool::Flash";

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `"a" . "b" . ...`
fn cat(parts: &[Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

/// `ord($s)`
fn ord(v: &Value) -> Value {
    n(v.to_bytes().first().map_or(0, |b| *b as i64))
}

/// `$raf->Read($buff, $len)`: the count, with `buff` updated.
fn read(it: &mut Interp, raf: &Value, buff: &mut Value, len: Value) -> ER<Value> {
    let mut args = [buff.clone(), len];
    let r = pl::method_mut(it, raf, "Read", &mut args, Want::Scalar)?;
    *buff = args[0].clone();
    Ok(r.into_iter().next().unwrap_or_default())
}

/// `$et->VPrint(LEVEL, MSG)`
fn vprint(it: &mut Interp, et: &Value, level: i64, msg: Value) -> ER<()> {
    pl::method(it, et, "VPrint", vec![n(level), msg])?;
    Ok(())
}

/// `$amfType[$t] || sprintf(FMT, $t)`
fn amf_type_name(it: &mut Interp, t: &Value, fmt: &str) -> Value {
    let amf_type = crate::runtime::lex_array(it, MODULE, "amfType");
    let name = amf_type.at(t.to_i64());
    if name.truthy() {
        name
    } else {
        pl::sprintf(fmt, &[t.clone()])
    }
}

/// `$isStruct{$t}`
fn is_struct(it: &mut Interp, t: &Value) -> bool {
    crate::runtime::lex_hash(it, MODULE, "isStruct")
        .get(t.to_bytes())
        .truthy()
}

/// Process Flash Video AMF Meta packet (ref 3)
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
///         3) Set to extract single type/value only
/// Returns: 1 on success, (or type/value if extracting single value)
/// Notes: Updates DataPos in dirInfo if extracting single value
pub fn process_meta(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr, single) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    let data_pt = dir_info.get("DataPt");
    let data_pos = dir_info.get("DataPos");
    let dir_len = {
        let v = dir_info.get("DirLen");
        if v.truthy() {
            v
        } else {
            pl::at(295);
            pl::length(&pl::sderef(&data_pt)?)
        }
    };
    let mut pos = {
        let v = dir_info.get("Pos");
        if v.truthy() {
            v
        } else {
            n(0)
        }
    };
    let mut typ = Value::Undef;
    let mut val = Value::Undef;
    let process_meta_packet = crate::runtime::lex_hash(it, MODULE, "processMetaPacket");

    if !single.truthy() {
        pl::method(it, &et, "VerboseDir", vec![pl::s("Meta")])?;
    }

    let mut rec = 0i64;
    'record: loop {
        if pl::ge(&pos, &dir_len) {
            break;
        }
        pl::at(303);
        typ = ord(&pl::substr(&pl::sderef(&data_pt)?, pos.to_i64(), None));
        pos = pl::add(&pos, &n(1));
        let t = typ.to_i64();
        if t == 0x00 || t == 0x0b {
            // double or date
            if pl::gt(&pl::add(&pos, &n(8)), &dir_len) {
                break;
            }
            val = call(it, "GetDouble", vec![data_pt.clone(), pos.clone()])?;
            pos = pl::add(&pos, &n(8));
            if t == 0x0b {
                // date
                val = pl::div(&val, &n(1000))?; // convert to seconds
                                                // get time zone
                if pl::gt(&pl::add(&pos, &n(2)), &dir_len) {
                    break;
                }
                let mut tz = call(it, "Get16s", vec![data_pt.clone(), pos.clone()])?;
                pos = pl::add(&pos, &n(2));
                // construct date/time string
                val = call(it, "ConvertUnixTime", vec![val, n(0), n(6)])?;
                // add timezone
                if pl::lt(&tz, &n(0)) {
                    val = pl::concat(&val, &pl::s("-"));
                    tz = pl::mul(&tz, &n(-1));
                } else {
                    val = pl::concat(&val, &pl::s("+"));
                }
                val = pl::concat(
                    &val,
                    &pl::sprintf(
                        "%.2d:%.2d",
                        &[pl::int(&pl::div(&tz, &n(60))?), pl::modulo(&tz, &n(60))?],
                    ),
                );
            }
        } else if t == 0x01 {
            // boolean
            if pl::gt(&pl::add(&pos, &n(1)), &dir_len) {
                break;
            }
            val = call(it, "Get8u", vec![data_pt.clone(), pos.clone()])?;
            if pl::lt(&val, &n(2)) {
                val = match val.to_i64() {
                    0 => pl::s("No"),
                    1 => pl::s("Yes"),
                    _ => Value::Undef,
                };
            }
            pos = pl::add(&pos, &n(1));
        } else if t == 0x02 {
            // string
            if pl::gt(&pl::add(&pos, &n(2)), &dir_len) {
                break;
            }
            let len = call(it, "Get16u", vec![data_pt.clone(), pos.clone()])?;
            if pl::gt(&pl::add(&pl::add(&pos, &n(2)), &len), &dir_len) {
                break;
            }
            val = pl::substr(&data_pt.deref(), pos.to_i64() + 2, Some(len.to_i64()));
            pos = pl::add(&pos, &pl::add(&n(2), &len));
        } else if is_struct(it, &typ) {
            // object, mixed array or typed object
            let tn = amf_type_name_plain(it, &typ);
            vprint(it, &et, 1, cat(&[pl::s("  + ["), tn, pl::s("]\n")]))?;
            let mut get_name = false;
            val = pl::s(""); // dummy value
            if t == 0x08 {
                // mixed array
                // skip last array index for mixed array
                if pl::gt(&pl::add(&pos, &n(4)), &dir_len) {
                    break;
                }
                pos = pl::add(&pos, &n(4));
            } else if t == 0x10 {
                // typed object
                get_name = true;
            }
            loop {
                // get tag ID (or typed object name)
                if pl::gt(&pl::add(&pos, &n(2)), &dir_len) {
                    break 'record;
                }
                let len = call(it, "Get16u", vec![data_pt.clone(), pos.clone()])?;
                if pl::gt(&pl::add(&pl::add(&pos, &n(2)), &len), &dir_len) {
                    let tn = amf_type_name_plain(it, &typ);
                    pl::method(
                        it,
                        &et,
                        "Warn",
                        vec![cat(&[pl::s("Truncated "), tn, pl::s(" record")])],
                    )?;
                    break 'record;
                }
                let mut tag = pl::substr(&data_pt.deref(), pos.to_i64() + 2, Some(len.to_i64()));
                pos = pl::add(&pos, &pl::add(&n(2), &len));
                // first string of a typed object is the object name
                if get_name {
                    vprint(
                        it,
                        &et,
                        1,
                        cat(&[pl::s("  | (object name '"), tag.clone(), pl::s("')\n")]),
                    )?;
                    get_name = false;
                    continue; // (ignore name for now)
                }
                let mut sub_table_ptr = tag_table_ptr.clone();
                let tag_info = sub_table_ptr.get(tag.to_bytes());
                // switch to subdirectory table if necessary
                if tag_info.truthy() && tag_info.get("SubDirectory").truthy() {
                    let sub_table = tag_info.get("SubDirectory").get("TagTable");
                    // descend into Flash SubDirectory
                    if re(r"^Image::ExifTool::Flash::", "").is_match(&sub_table) {
                        tag = tag_info.get("Name"); // use our name for the tag
                        sub_table_ptr = call(it, "GetTagTable", vec![sub_table])?;
                    }
                }
                // get object value
                let val_pos = pl::add(&pos, &n(1));
                dir_info.set("Pos", pos.clone());
                let struct_name = dir_info.get("StructName");
                // add structure name to start of tag name
                if !struct_name.is_undef() {
                    tag = pl::concat(&struct_name, &pl::ucfirst(&tag));
                }
                dir_info.set("StructName", tag.clone()); // set new structure name
                let r = process_meta(
                    it,
                    vec![et.clone(), dir_info.clone(), sub_table_ptr.clone(), n(1)],
                    Want::List,
                )?;
                let (tt, v) = (arg(&r, 0), arg(&r, 1));
                dir_info.set("StructName", struct_name); // restore original structure name
                pos = dir_info.get("Pos"); // update to new position in packet
                                           // all done if this value contained tags
                if tt.is_undef() || v.is_undef() {
                    break 'record;
                }
                if is_struct(it, &tt) {
                    continue; // already handled tags in sub-structures
                }
                if pl::ref_of(&v) == "ARRAY" && v.is_empty() {
                    continue; // ignore empty arrays
                }
                if pl::num_eq(&tt, &n(0x09)) {
                    break; // (end of object)
                }
                if !sub_table_ptr.get(tag.to_bytes()).truthy() && re(r"^\w+$", "").is_match(&tag) {
                    let h = Value::new_hash();
                    h.set("Name", pl::ucfirst(&tag));
                    call(
                        it,
                        "AddTagToTable",
                        vec![sub_table_ptr.clone(), tag.clone(), h],
                    )?;
                    vprint(
                        it,
                        &et,
                        1,
                        cat(&[pl::s("  | (adding "), tag.clone(), pl::s(")\n")]),
                    )?;
                }
                let format = amf_type_name(it, &tt, "0x%x");
                pl::method(
                    it,
                    &et,
                    "HandleTag",
                    vec![
                        sub_table_ptr.clone(),
                        tag.clone(),
                        v,
                        pl::s("DataPt"),
                        data_pt.clone(),
                        pl::s("DataPos"),
                        data_pos.clone(),
                        pl::s("Start"),
                        val_pos.clone(),
                        pl::s("Size"),
                        pl::sub(&pos, &val_pos),
                        pl::s("Format"),
                        format,
                    ],
                )?;
            }
        // } elsif ($type == 0x04) {   # movie clip (not supported)
        } else if t == 0x05 || t == 0x06 || t == 0x09 || t == 0x0d {
            // null, undefined, dirLen of object, or unsupported
            val = pl::s("");
        } else if t == 0x07 {
            // reference
            if pl::gt(&pl::add(&pos, &n(2)), &dir_len) {
                break;
            }
            val = call(it, "Get16u", vec![data_pt.clone(), pos.clone()])?;
            pos = pl::add(&pos, &n(2));
        } else if t == 0x0a {
            // array
            if pl::gt(&pl::add(&pos, &n(4)), &dir_len) {
                break;
            }
            let num = call(it, "Get32u", vec![data_pt.clone(), pos.clone()])?;
            dir_info.set("Pos", pl::add(&pos, &n(4)));
            let mut vals: Vec<Value> = Vec::new();
            // add array index to compound tag name
            let struct_name = dir_info.get("StructName");
            let mut i = 0i64;
            while pl::lt(&n(i), &num) {
                if !struct_name.is_undef() {
                    dir_info.set("StructName", pl::concat(&struct_name, &n(i)));
                }
                let r = process_meta(
                    it,
                    vec![et.clone(), dir_info.clone(), tag_table_ptr.clone(), n(1)],
                    Want::List,
                )?;
                let (tt, v) = (arg(&r, 0), arg(&r, 1));
                if v.is_undef() {
                    break 'record;
                }
                // save value unless contained in a sub-structure
                if !is_struct(it, &tt) {
                    vals.push(v);
                }
                i += 1;
            }
            dir_info.set("StructName", struct_name);
            pos = dir_info.get("Pos");
            val = Value::new_array(vals);
        } else if t == 0x0c || t == 0x0f {
            // long string or XML
            if pl::gt(&pl::add(&pos, &n(4)), &dir_len) {
                break;
            }
            let len = call(it, "Get32u", vec![data_pt.clone(), pos.clone()])?;
            if pl::gt(&pl::add(&pl::add(&pos, &n(4)), &len), &dir_len) {
                break;
            }
            val = pl::substr(&data_pt.deref(), pos.to_i64() + 4, Some(len.to_i64()));
            pos = pl::add(&pos, &pl::add(&n(4), &len));
        // } elsif ($type == 0x0e) {   # record set (not supported)
        // } elsif ($type == 0x11) {   # AMF3 data (can't add support for this without a test sample)
        } else {
            let tn = amf_type_name(it, &typ, "type 0x%x");
            pl::method(
                it,
                &et,
                "Warn",
                vec![cat(&[
                    pl::s("AMF "),
                    tn,
                    pl::s(" record not yet supported"),
                ])],
            )?;
            typ = Value::Undef; // (so we don't print another warning)
            break; // can't continue
        }
        if single.truthy() {
            break; // all done if extracting single value
        }
        if !is_struct(it, &typ) {
            // only process certain Meta packets
            if pl::num_eq(&typ, &n(0x02)) && rec == 0 {
                let process = process_meta_packet.get(val.to_bytes()).truthy();
                let verb = if process { "processing" } else { "ignoring" };
                vprint(
                    it,
                    &et,
                    0,
                    cat(&[
                        pl::s("  | ("),
                        pl::s(verb),
                        pl::s(" "),
                        val.clone(),
                        pl::s(" information)\n"),
                    ]),
                )?;
                if !process {
                    break;
                }
            } else {
                // give verbose indication if we ignore a lone value
                let tn = amf_type_name(it, &typ, "type 0x%x");
                vprint(
                    it,
                    &et,
                    1,
                    cat(&[
                        pl::s("  | (ignored lone "),
                        tn,
                        pl::s(" value '"),
                        val.clone(),
                        pl::s("')\n"),
                    ]),
                )?;
            }
        }
        rec += 1;
    }
    if val.is_undef() && !typ.is_undef() {
        pl::method(
            it,
            &et,
            "Warn",
            vec![pl::sprintf("Truncated AMF record 0x%x", &[typ.clone()])],
        )?;
    }
    if !single.truthy() {
        return pl::ret(n(1)); // all done
    }
    dir_info.set("Pos", pos); // update position
    Ok(vec![typ, val]) // return single type/value pair
}

/// `$amfType[$type]` (interpolated as is).
fn amf_type_name_plain(it: &mut Interp, t: &Value) -> Value {
    crate::runtime::lex_array(it, MODULE, "amfType").at(t.to_i64())
}

/// Read information frame a Flash Video file
/// Inputs: 0) ExifTool object reference, 1) Directory information reference
/// Returns: 1 on success, 0 if this wasn't a valid Flash Video file
pub fn process_flv(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let raf = dir_info.get("RAF");
    let mut buff = Value::Undef;

    pl::at(474);
    if !pl::num_eq(&read(it, &raf, &mut buff, n(9))?, &n(9)) {
        return pl::ret(n(0));
    }
    if !re(r"^FLV\x01", "").is_match(&buff) {
        return pl::ret(n(0));
    }
    call(it, "SetByteOrder", vec![pl::s("MM")])?;
    pl::method(it, &et, "SetFileType", vec![])?;
    let u = pl::unpack("x4CN", &buff)?;
    let (mut flags, offset) = (arg(&u, 0), arg(&u, 1));
    if pl::gt(&offset, &n(9))
        && !pl::method(it, &raf, "Seek", vec![pl::sub(&offset, &n(9)), n(1)])?.truthy()
    {
        return pl::ret(n(1));
    }
    flags = ops::bit_and(&flags, &n(0x05)); // only look for audio/video
    let mut found = n(0);
    let tag_table_ptr = call(
        it,
        "GetTagTable",
        vec![pl::s("Image::ExifTool::Flash::FLV")],
    )?;
    loop {
        if !pl::num_eq(&read(it, &raf, &mut buff, n(15))?, &n(15)) {
            break;
        }
        let mut len = pl::unpack1("x4N", &buff)?;
        let typ = ops::shift_right(&len, &n(24));
        len = ops::bit_and(&len, &n(0x00ffffff));
        let tag_info = pl::method(
            it,
            &et,
            "GetTagInfo",
            vec![tag_table_ptr.clone(), typ.clone()],
        )?;
        if pl::gt(&verbose, &n(1)) {
            let name = if tag_info.truthy() {
                tag_info.get("Name")
            } else {
                pl::concat(&pl::s("type "), &typ)
            };
            vprint(
                it,
                &et,
                1,
                cat(&[
                    pl::s("FLV "),
                    name,
                    pl::s(" packet, len "),
                    len.clone(),
                    pl::s("\n"),
                ]),
            )?;
        }
        buff = Value::Undef;
        if tag_info.truthy() && tag_info.get("SubDirectory").truthy() {
            let mask = tag_info.get("BitMask");
            if mask.truthy() {
                // handle audio or video packet
                if !ops::bit_and(&found, &mask).truthy() {
                    found = ops::bit_or(&found, &mask);
                    flags = ops::bit_and(&flags, &ops::bit_not(&mask));
                    if pl::ge(&len, &n(1)) && pl::num_eq(&read(it, &raf, &mut buff, n(1))?, &n(1)) {
                        len = pl::sub(&len, &n(1));
                    } else {
                        pl::method(
                            it,
                            &et,
                            "Warn",
                            vec![cat(&[
                                pl::s("Bad "),
                                tag_info.get("Name"),
                                pl::s(" packet"),
                            ])],
                        )?;
                        break;
                    }
                }
            } else if pl::num_eq(&read(it, &raf, &mut buff, len.clone())?, &len) {
                len = n(0);
            } else {
                pl::method(it, &et, "Warn", vec![pl::s("Truncated Meta packet")])?;
                break;
            }
        }
        if !buff.is_undef() {
            let data_pos = pl::sub(&pl::method(it, &raf, "Tell", vec![])?, &pl::length(&buff));
            pl::method(
                it,
                &et,
                "HandleTag",
                vec![
                    tag_table_ptr.clone(),
                    typ.clone(),
                    Value::Undef,
                    pl::s("DataPt"),
                    pl::sref(buff.clone()),
                    pl::s("DataPos"),
                    data_pos,
                ],
            )?;
        }
        if !flags.truthy() {
            break;
        }
        if len.truthy() && !pl::method(it, &raf, "Seek", vec![len.clone(), n(1)])?.truthy() {
            break;
        }
    }
    pl::ret(n(1))
}

/// Found a Flash tag
/// Inputs: 0) ExifTool object ref, 1) tag name, 2) tag value
pub fn found_flash_tag(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, tag, val) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let table = pl::pkg_hash(it, "Image::ExifTool::Flash::Main");
    it.call_method(&et, "HandleTag", vec![et.clone(), table, tag, val], want)
}

fn found(it: &mut Interp, et: &Value, tag: Value, val: Value) -> ER<()> {
    found_flash_tag(it, vec![et.clone(), tag, val], Want::Void)?;
    Ok(())
}

/// ReadCompressed with its written arguments (`$_[1]`, `$_[3]`) as `buff`
/// and `compressed`.
fn read_compressed_core(
    it: &mut Interp,
    raf: &Value,
    buf: &mut Value,
    len: &Value,
    compressed: &mut Value,
) -> ER<Value> {
    read_compressed_io(it, raf, buf, len, compressed, &mut false, &mut false)
}

/// The body of ReadCompressed; `set_buf` / `set_comp` tell whether `$_[1]` /
/// `$_[3]` were assigned.
fn read_compressed_io(
    it: &mut Interp,
    raf: &Value,
    buf: &mut Value,
    len: &Value,
    compressed: &mut Value,
    set_buf: &mut bool,
    set_comp: &mut bool,
) -> ER<Value> {
    let mut inflate = compressed.clone();
    let mut buff = Value::Undef;
    pl::at(547);
    if !read(it, raf, &mut buff, len.clone())?.truthy() {
        *set_comp = true;
        *compressed = pl::s("Error reading file");
        return Ok(n(0));
    }
    // uncompress if necessary
    if inflate.truthy() {
        if !inflate.is_ref() {
            if pl::eval_block(it, |it| crate::runtime::require(it, "Compress::Zlib"))?.is_none() {
                *set_comp = true;
                *compressed = pl::s("Install Compress::Zlib to extract compressed information");
                return Ok(n(0));
            }
            inflate = pl::call(it, "Compress::Zlib::inflateInit", vec![])?;
            if !inflate.truthy() {
                *set_comp = true;
                *compressed = pl::s("Error initializing inflate for Flash data");
                return Ok(n(0));
            }
            *set_comp = true;
            *compressed = inflate.clone(); // pass inflate object back to caller
        }
        let mut tmp = buff.clone();
        buff = pl::s("");
        // read 64 more bytes at a time and inflate until we get enough uncompressed data
        loop {
            let mut args = [tmp.clone()];
            let r = pl::method_mut(it, &inflate, "inflate", &mut args, Want::List)?;
            tmp = args[0].clone();
            let (dat, stat) = (arg(&r, 0), arg(&r, 1));
            let z_stream_end = pl::call(it, "Compress::Zlib::Z_STREAM_END", vec![])?;
            let z_ok = pl::call(it, "Compress::Zlib::Z_OK", vec![])?;
            if pl::num_eq(&stat, &z_stream_end) || pl::num_eq(&stat, &z_ok) {
                buff = pl::concat(&buff, &dat); // add inflated data to buffer
                if pl::ge(&pl::length(&buff), len) || pl::num_eq(&stat, &z_stream_end) {
                    break;
                }
                if !read(it, raf, &mut tmp, n(64))?.truthy() {
                    break; // must read a bit more data
                }
            } else {
                buff = pl::s("");
                break;
            }
        }
        if !pl::length(&buff).truthy() {
            *set_comp = true;
            *compressed = pl::s("Error inflating compressed Flash data");
        }
    }
    *set_buf = true;
    *buf = if !buf.is_undef() {
        pl::concat(buf, &buff)
    } else {
        buff.clone()
    };
    Ok(pl::length(&buff))
}

/// Read data from possibly compressed file
/// Inputs: 0) RAF reference, 1) data buffer, 2) bytes to read, 2) compressed flag
/// Returns: number of bytes read (may be greater than requested bytes if compressed)
/// - concatenates data to current buffer
/// - updates compressed flag with reference to inflate object for future calls
///   (or sets to error message and returns zero on error)
pub fn read_compressed(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (raf, mut buf, len, mut compressed) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    let (mut set_buf, mut set_comp) = (false, false);
    let r = read_compressed_io(
        it,
        &raf,
        &mut buf,
        &len,
        &mut compressed,
        &mut set_buf,
        &mut set_comp,
    );
    if set_comp {
        it.set_arg(3, compressed)?;
    }
    if set_buf {
        it.set_arg(1, buf)?;
    }
    pl::ret(r?)
}

/// Read information frame a Flash file
/// Inputs: 0) ExifTool object reference, 1) Directory information reference
/// Returns: 1 on success, 0 if this wasn't a valid Flash file
pub fn process_swf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let mut buff = Value::Undef;
    let mut has_meta = false;

    pl::at(597);
    if !pl::num_eq(&read(it, &raf, &mut buff, n(8))?, &n(8)) {
        return pl::ret(n(0));
    }
    let Some(m) = re(r"^(F|C)WS([^\0])", "").m(&buff) else {
        return pl::ret(n(0));
    };
    let (mut compressed, vers) = (
        if pl::str_eq(&cap(&m, 1), "C") {
            n(1)
        } else {
            n(0)
        },
        ord(&cap(&m, 2)),
    );

    call(it, "SetByteOrder", vec![pl::s("II")])?;
    pl::method(it, &et, "SetFileType", vec![])?;
    call(
        it,
        "GetTagTable",
        vec![pl::s("Image::ExifTool::Flash::Main")],
    )?; // make sure table is initialized

    found(it, &et, pl::s("FlashVersion"), vers)?;
    found(it, &et, pl::s("Compressed"), compressed.clone())?;

    // read the next 64 bytes of the file (and inflate if necessary)
    buff = pl::s("");
    if !read_compressed_core(it, &raf, &mut buff, &n(64), &mut compressed)?.truthy() {
        if compressed.truthy() {
            pl::method(it, &et, "Warn", vec![compressed])?;
        }
        return pl::ret(n(1));
    }

    // unpack elements of bit-packed Flash Rect structure
    let n_bits = ops::shift_right(&pl::unpack1("C", &buff)?, &n(3)); // bits in x1,x2,y1,y2 elements
    let tot_bits = pl::add(&n(5), &pl::mul(&n_bits, &n(4))); // total bits in Rect structure
    let n_bytes = pl::int(&pl::div(&pl::add(&tot_bits, &n(7)), &n(8))?); // byte length of Rect structure
    if pl::lt(&pl::length(&buff), &pl::add(&n_bytes, &n(4))) {
        pl::method(it, &et, "Warn", vec![pl::s("Truncated Flash file")])?;
        return pl::ret(n(1));
    }
    let bits = pl::unpack1(&format!("B{}", tot_bits.to_text()), &buff)?;
    // isolate Rect elements and convert from ASCII bit strings to integers
    let nb = n_bits.to_text();
    let mut vals = pl::unpack(&format!("x5a{nb}a{nb}a{nb}a{nb}"), &bits)?;
    // (do conversion the hard way because oct("0b$val") requires Perl 5.6)
    for v in vals.iter_mut() {
        let pad = (32 - pl::length(v).to_i64()).max(0) as usize;
        let s = pl::concat(&Value::bytes(vec![b'0'; pad]), v);
        *v = pl::unpack1("N", &pl::pack("B32", &[s])?)?;
    }

    // calculate and store ImageWidth/Height
    let w = pl::div(&pl::sub(&arg(&vals, 1), &arg(&vals, 0)), &n(20))?;
    found(it, &et, pl::s("ImageWidth"), w)?;
    let h = pl::div(&pl::sub(&arg(&vals, 3), &arg(&vals, 2)), &n(20))?;
    found(it, &et, pl::s("ImageHeight"), h)?;

    // get frame rate and count
    vals = pl::unpack(&format!("x{}v2", n_bytes.to_text()), &buff)?;
    found(
        it,
        &et,
        pl::s("FrameRate"),
        pl::div(&arg(&vals, 0), &n(256))?,
    )?;
    found(it, &et, pl::s("FrameCount"), arg(&vals, 1))?;
    if arg(&vals, 0).truthy() {
        let d = pl::div(&pl::mul(&arg(&vals, 1), &n(256)), &arg(&vals, 0))?;
        found(it, &et, pl::s("Duration"), d)?;
    }

    // scan through the tags to find FlashAttributes and XMP
    buff = pl::substr(&buff, pl::add(&n_bytes, &n(4)).to_i64(), None);
    loop {
        let mut buff_len = pl::length(&buff);
        if pl::lt(&buff_len, &n(2)) {
            break;
        }
        let code = call(it, "Get16u", vec![pl::sref(buff.clone()), n(0)])?;
        let mut pos = n(2);
        let tag = ops::shift_right(&code, &n(6));
        let mut size = ops::bit_and(&code, &n(0x3f));
        vprint(
            it,
            &et,
            1,
            cat(&[
                pl::s("SWF tag "),
                tag.clone(),
                pl::s(" ("),
                size.clone(),
                pl::s(" bytes):\n"),
            ]),
        )?;
        if !(pl::num_eq(&tag, &n(69)) || pl::num_eq(&tag, &n(77)) || has_meta) {
            break;
        }
        // read enough to get a complete short record
        if pl::gt(&pl::add(&pos, &size), &buff_len) {
            // (read 2 extra bytes if available to get next tag word)
            if !read_compressed_core(it, &raf, &mut buff, &pl::add(&size, &n(2)), &mut compressed)?
                .truthy()
            {
                if compressed.truthy() {
                    pl::method(it, &et, "Warn", vec![compressed])?;
                }
                return pl::ret(n(1));
            }
            buff_len = pl::length(&buff);
            if pl::gt(&pl::add(&pos, &size), &buff_len) {
                break;
            }
        }
        // read extended record if necessary
        if pl::num_eq(&size, &n(0x3f)) {
            if pl::gt(&pl::add(&pos, &n(4)), &buff_len) {
                break;
            }
            size = call(it, "Get32u", vec![pl::sref(buff.clone()), pos.clone()])?;
            pos = pl::add(&pos, &n(4));
            if pl::gt(&size, &n(1000000)) {
                break; // don't read anything huge
            }
            if pl::gt(&pl::add(&pos, &size), &buff_len) {
                if !read_compressed_core(
                    it,
                    &raf,
                    &mut buff,
                    &pl::add(&size, &n(2)),
                    &mut compressed,
                )?
                .truthy()
                {
                    if compressed.truthy() {
                        pl::method(it, &et, "Warn", vec![compressed])?;
                    }
                    return pl::ret(n(1));
                }
                buff_len = pl::length(&buff);
                if pl::gt(&pl::add(&pos, &size), &buff_len) {
                    break;
                }
            }
            vprint(
                it,
                &et,
                1,
                cat(&[pl::s("  [extended size "), size.clone(), pl::s(" bytes]\n")]),
            )?;
        }
        if pl::num_eq(&tag, &n(69)) {
            // FlashAttributes
            if !size.truthy() {
                break;
            }
            let flags = call(it, "Get8u", vec![pl::sref(buff.clone()), pos.clone()])?;
            found(it, &et, tag.clone(), flags.clone())?;
            if !ops::bit_and(&flags, &n(0x10)).truthy() {
                break; // only continue if we have metadata (XMP)
            }
            has_meta = true;
        } else if pl::num_eq(&tag, &n(77)) {
            // Metadata
            let val = pl::substr(&buff, pos.to_i64(), Some(size.to_i64()));
            found(it, &et, tag.clone(), val)?;
            break;
        }
        if pl::gt(&pl::add(&pos, &n(2)), &buff_len) {
            break;
        }
        buff = pl::substr(&buff, pos.to_i64(), None); // remove everything before the next tag
    }
    pl::ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Flash::ProcessMeta", process_meta);
    it.define_native("Image::ExifTool::Flash::ProcessFLV", process_flv);
    it.define_native("Image::ExifTool::Flash::FoundFlashTag", found_flash_tag);
    it.define_native("Image::ExifTool::Flash::ReadCompressed", read_compressed);
    it.define_native("Image::ExifTool::Flash::ProcessSWF", process_swf);
}
