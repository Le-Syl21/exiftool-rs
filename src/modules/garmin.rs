//! Image::ExifTool::Garmin (Image/ExifTool/Garmin.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Garmin FIT files. The tag tables are data; these are the module's
//! two subs.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::quick_time_stream::{cat, get16u, get8u, get_byte_order, get_tag_table, hash_of, m, n, new_doc, raf_read, raf_seek, raf_tell, read_value, s, scalar, set_byte_order, warn};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, re, ret};
use crate::runtime;

const PKG: &str = "Image::ExifTool::Garmin";

/// Calculate CRC for each byte
/// Inputs: 0) running CRC, 1) byte value
/// Returns: updated CRC
/// - unused here, but included in case it may be helpful later
pub fn crc(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (mut crc, byte) = (arg(&a, 0), arg(&a, 1));
    let crc_table = runtime::lex_array(it, PKG, "crc_table");
    let idx = |v: &Value| ops::bit_and(v, &n(0xf)).to_i64();
    // compute checksum of lower four bits of byte
    let mut tmp = crc_table.at(idx(&crc));
    crc = ops::bit_and(&ops::shift_right(&crc, &n(4)), &n(0x0fff));
    crc = ops::bit_xor(&ops::bit_xor(&crc, &tmp), &crc_table.at(idx(&byte)));
    // now compute checksum of upper four bits of byte
    tmp = crc_table.at(idx(&crc));
    crc = ops::bit_and(&ops::shift_right(&crc, &n(4)), &n(0x0fff));
    crc = ops::bit_xor(&ops::bit_xor(&crc, &tmp), &crc_table.at(idx(&ops::shift_right(&byte, &n(4)))));
    ret(crc)
}

/// Read Garmin FIT file
/// Inputs: 0) ExifTool ref, 1) dirInfo ref
/// Return: true on success
pub fn process_fit(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| process_fit_impl(it, &a))
}

fn process_fit_impl(it: &mut Interp, a: &[Value]) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(a, 0), arg(a, 1));
    let mut buff = Value::Undef;
    let mut err = Value::Undef;
    let msg = Value::new_hash();
    let done = Value::new_hash();
    let dev_data = Value::new_hash();
    let mut i: Value;
    let mut pos: Value;
    let raf = dir_info.get("RAF");
    let mut unknown = et.get("OPTIONS").get("Unknown");
    let verbose = et.get("OPTIONS").get("Verbose");
    let ee = et.get("OPTIONS").get("ExtractEmbedded");
    if verbose.truthy() && !unknown.truthy() {
        unknown = n(1);
    }
    let base_type = runtime::lex_hash(it, PKG, "baseType");

    // verify this is a valid Garmin FIT file
    pl::at(6330);
    if !(pl::num_eq(&raf_read(it, &raf, &mut buff, n(12))?, &n(12)) && re(r"^.{8}\.FIT", "s").is_match(&buff)) {
        return ret(n(0));
    }
    m(it, &et, "SetFileType", vec![])?;
    let tag_tbl = get_tag_table(it, "Image::ExifTool::Garmin::FIT")?;
    let common = get_tag_table(it, "Image::ExifTool::Garmin::Common")?;
    let dev_tbl = get_tag_table(it, "Image::ExifTool::Garmin::Dev")?;
    // clear out old developer tags
    for k in pl::call_list(it, "Image::ExifTool::TagTableKeys", vec![dev_tbl.clone()])? {
        dev_tbl.delete(k.to_bytes());
    }
    let v = get8u(it, &pl::sref(buff.clone()), n(1))?;
    m(it, &et, "HandleTag", vec![tag_tbl.clone(), s("vers"), v])?;
    if !ee.truthy() {
        m(it, &et, "Warn", vec![s("Use ExtractEmbedded option to extract all timed metadata"), n(3)])?;
    }
    let u = pl::unpack("Cx3V", &buff)?;
    let (hdr_len, data_len) = (arg(&u, 0), arg(&u, 1));
    if pl::gt(&hdr_len, &n(12)) {
        raf_read(it, &raf, &mut buff, pl::sub(&hdr_len, &n(12)))?;
    }
    let mut timestamp = n(0);
    loop {
        let msg_start = raf_tell(it, &raf)?;
        if pl::ge(&msg_start, &pl::add(&hdr_len, &data_len)) {
            break;
        }
        if !pl::num_eq(&raf_read(it, &raf, &mut buff, n(1))?, &n(1)) {
            break;
        }
        let flags = pl::unpack1("C", &buff)?.to_i64();
        let local_num;
        if flags & 0x80 != 0 {
            // compressed header
            local_num = n((flags >> 5) & 0x03);
            // update current timestamp
            let time_offset = n(flags & 0x1f);
            if time_offset.truthy() {
                // the FIT specification is stupid here. The offset should have
                // been defined as a simple difference from the previous timestamp
                // instead of the lower 5 bits with a wrap-around.  Dumb.  All the
                // resulting bit gymnastics are totally unnecessary:
                let low_bits = ops::bit_and(&timestamp, &n(0x1f));
                let mut ts = pl::add(&ops::bit_and(&timestamp, &n(0xffffffe0)), &time_offset);
                if pl::lt(&time_offset, &low_bits) {
                    ts = pl::add(&ts, &n(0x20));
                }
                msg.get_or_new_hash(local_num.to_bytes()).set("TS", Value::new_array(vec![n(253), ts])); // [num, new timestamp]
            }
        } else {
            // normal header
            local_num = n(flags & 0x0f);
            if flags & 0x40 != 0 {
                //
                // Process the definition message
                //
                if !pl::num_eq(&raf_read(it, &raf, &mut buff, n(5))?, &n(5)) {
                    if !pl::num_eq(&raf_tell(it, &raf)?, &pl::add(&pl::add(&hdr_len, &data_len), &n(2))) {
                        // (+2 for checksum)
                        err = s("Unexpected end of file");
                    }
                    break;
                }
                let bp = pl::sref(buff.clone());
                let bo = if get8u(it, &bp, n(1))?.truthy() { "MM" } else { "II" };
                set_byte_order(it, s(bo))?;
                let msg_num = get16u(it, &bp, n(2))?;
                let n_fields = get8u(it, &bp, n(4))?;
                let mut len = pl::mul(&n_fields, &n(3));
                let the_msg = Value::new_hash(); // start new message definition
                msg.set(local_num.to_bytes(), the_msg.clone());
                the_msg.set("ByteOrder", get_byte_order(it)?);
                the_msg.set("MessageNum", msg_num.clone());
                let mut tag_info = tag_tbl.get(msg_num.to_bytes());
                let msg_name;
                let mut field_info = Value::Undef;
                if tag_info.truthy() {
                    msg_name = tag_info.get("Name");
                    the_msg.set("Name", msg_name.clone());
                } else {
                    msg_name = cat(&[&s("Unknown"), &msg_num]);
                    the_msg.set("Name", msg_name.clone());
                    tag_info = hash_of(vec![("Name", msg_name.clone()), ("Unknown", n(1))]);
                    pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_tbl.clone(), msg_num.clone(), tag_info.clone()])?;
                }
                if !tag_info.get("SubDirectory").truthy() {
                    // construct table for this subdirectory
                    let table_name = format!("Image::ExifTool::Garmin::{}", msg_name.to_text());
                    let tbl = pl::pkg_hash(it, &table_name);
                    if let Value::HashRef(h) = &tbl {
                        h.borrow_mut().clear();
                    }
                    tbl.set("GROUPS", hash_of(vec![("0", s("Garmin")), ("1", msg_name.clone()), ("2", s("Unknown"))]));
                    tbl.set("VARS", hash_of(vec![("ID_FMT", s("dec")), ("NO_LOOKUP", n(1))]));
                    tag_info.get_or_new_hash("SubDirectory").set("TagTable", cat(&[&s("Image::ExifTool::Garmin::"), &msg_name]));
                }
                let sub_tbl = pl::call(it, "Image::ExifTool::GetTagTable", vec![tag_info.get("SubDirectory").get("TagTable")])?;
                m(it, &et, "VPrint", vec![n(1), cat(&[&s("  + ["), &msg_name, &s(" definition message (local "), &local_num, &s(")]")])])?;
                if !pl::num_eq(&raf_read(it, &raf, &mut buff, len.clone())?, &len) {
                    err = s("Truncated definition message");
                    break;
                }
                let mut tot_size = n(0);
                if !(tag_info.get("Unknown").truthy() && !unknown.truthy()) {
                    field_info = Value::new_array(vec![]);
                    the_msg.set("FieldInfo", field_info.clone());
                }
                // loop through the message field definitions
                pos = n(0);
                while pl::lt(&pos, &len) {
                    let c = pl::unpack(&format!("x{} C3", pos.to_i64()), &buff)?;
                    let (num, size, typ) = (arg(&c, 0), arg(&c, 1), arg(&c, 2));
                    if base_type.get(typ.to_bytes()).truthy() {
                        // remember offset of timestamp if this message has one
                        let st = sub_tbl.get(num.to_bytes());
                        if pl::num_eq(&num, &n(253))
                            || (st.truthy() && !the_msg.get("TS").truthy() &&
                            // IsTimeStamp flag is set for timestamps to consider for
                            // incrementing the document number (eg. new GPS fix)
                            st.get("IsTimeStamp").truthy())
                        {
                            the_msg.set("TS", Value::new_array(vec![num.clone(), size.clone(), typ.clone(), tot_size.clone()]));
                        }
                        // save field definitions for fields we care about
                        if field_info.truthy() {
                            field_info.push(Value::new_array(vec![num.clone(), size.clone(), typ.clone()]));
                        }
                    } else {
                        warn(it, &et, cat(&[&s("Unknown field type "), &typ]))?;
                    }
                    tot_size = pl::add(&tot_size, &size);
                    pos = pl::add(&pos, &n(3));
                }
                let mut n_dev = n(0);
                if flags & 0x20 != 0 {
                    // developer data flag
                    if !pl::num_eq(&raf_read(it, &raf, &mut buff, n(1))?, &n(1)) {
                        err = s("Missing developer definition");
                        break;
                    }
                    n_dev = get8u(it, &pl::sref(buff.clone()), n(0))?;
                    len = pl::mul(&n_dev, &n(3));
                    if !pl::num_eq(&raf_read(it, &raf, &mut buff, len.clone())?, &len) {
                        err = s("Truncated developer definition");
                        break;
                    }
                    // loop through the developer fields
                    i = n(0);
                    pos = n(0);
                    while pl::lt(&i, &n_dev) {
                        let c = pl::unpack(&format!("x{} C3", pos.to_i64()), &buff)?;
                        let (num, size, idx) = (arg(&c, 0), arg(&c, 1), arg(&c, 2));
                        // fieldInfo[3] is flag for developer field
                        if field_info.truthy() {
                            field_info.push(Value::new_array(vec![num, size.clone(), idx, n(1)]));
                        }
                        tot_size = pl::add(&tot_size, &size);
                        i = pl::add(&i, &n(1));
                        pos = pl::add(&pos, &n(3));
                    }
                }
                let _ = n_dev;
                the_msg.set("Size", tot_size);
                if pl::gt(&verbose, &n(2)) {
                    let nn = pl::sub(&raf_tell(it, &raf)?, &msg_start);
                    let ok = raf_seek(it, &raf, pl::neg(&nn), 1)? && raf_read(it, &raf, &mut buff, nn.clone())?.truthy();
                    if !ok {
                        err = s("Seek error");
                        break;
                    }
                    let mut n_dev = pl::sub(&pl::sub(&nn, &n(6)), &pl::mul(&n(3), &n_fields));
                    if n_dev.truthy() {
                        n_dev = pl::div(&pl::sub(&n_dev, &n(1)), &n(3))?;
                    }
                    m(it, &et, "VPrint", vec![n(1), cat(&[&s("    ("), &n_fields, &s(" data + "), &n_dev, &s(" developer fields)")])])?;
                    let dp = pl::sub(&raf_tell(it, &raf)?, &nn);
                    m(it, &et, "VerboseDump", vec![pl::sref(buff.clone()), s("DataPos"), dp])?;
                }
                continue;
            }
        }
        //
        // Process the data message
        //
        let the_msg = msg.get(local_num.to_bytes());
        if !the_msg.truthy() {
            err = cat(&[&s("Missing definition for local message "), &local_num]);
            break;
        }
        let data_pos = raf_tell(it, &raf)?;
        let msg_size = the_msg.get("Size");
        let msg_num = the_msg.get("MessageNum");
        set_byte_order(it, the_msg.get("ByteOrder"))?;
        if !ee.truthy() {
            if done.get(msg_num.to_bytes()).truthy() {
                if !raf_seek(it, &raf, msg_size.clone(), 1)? {
                    err = s("Seek error");
                    break;
                }
                continue;
            } else {
                done.set(msg_num.to_bytes(), n(1));
            }
        }
        if !pl::num_eq(&raf_read(it, &raf, &mut buff, msg_size.clone())?, &msg_size) {
            err = s("Truncated data message");
            break;
        }
        let field_info = the_msg.get("FieldInfo");
        let msg_name = the_msg.get("Name");
        let tag_info = tag_tbl.get(msg_num.to_bytes());
        let sub_tbl = pl::call(it, "Image::ExifTool::GetTagTable", vec![tag_info.get("SubDirectory").get("TagTable")])?;
        let old_indent = et.get("INDENT");
        et.set("INDENT", cat(&[&old_indent, &s("| ")]));
        let comp = if flags & 0x80 != 0 { "compressed header, " } else { "" };
        m(it, &et, "VerboseDir", vec![tag_info.get("Name"), Value::Undef, cat(&[&s("(local "), &local_num, &s(") "), &s(comp), &msg_size])])?;
        if pl::gt(&verbose, &n(2)) {
            let tmp = cat(&[&Value::bytes([flags as u8]), &buff]);
            let dp = pl::sub(&pl::sub(&raf_tell(it, &raf)?, &msg_size), &n(1));
            m(it, &et, "VerboseDump", vec![pl::sref(tmp), s("DataPos"), dp])?;
        }
        let buff_ref = pl::sref(buff.clone());
        let ts = the_msg.get("TS");
        if ts.truthy() && pl::num_eq(&ts.at(0), &n(253)) {
            let val;
            if !ts.at(2).is_undef() {
                val = read_value(it, &buff_ref, ts.at(3), base_type.get(ts.at(2).to_bytes()).at(0), n(1), ts.at(1))?;
            } else {
                val = ts.at(1);
            }
            // save TimeStamp tag now for unknown or compressed message
            if !pl::num_eq(&timestamp, &val) {
                timestamp = val.clone();
                // increment document number for GPS data or any message with a new timestamp
                new_doc(&et);
                if !(field_info.truthy() && !ts.at(2).is_undef()) {
                    let mut parms: Vec<Value> = Vec::new();
                    if !ts.at(2).is_undef() {
                        let bt = base_type.get(ts.at(2).to_bytes());
                        parms = vec![s("DataPt"), buff_ref.clone(), s("DataPos"), data_pos.clone(), s("Start"), ts.at(3), s("Size"), ts.at(1), s("Format"), cat(&[&bt.at(0), &s(" \""), &bt.at(1), &s("\"")])];
                    }
                    // set group 1 to message name
                    et.set("SET_GROUP1", sub_tbl.get("GROUPS").get("1"));
                    let mut args = vec![common.clone(), ts.at(0), val];
                    args.extend(parms);
                    m(it, &et, "HandleTag", args)?;
                    et.delete("SET_GROUP1");
                }
            }
        }
        // ignore this message unless it contains something else we care about
        if !field_info.truthy() {
            et.set("INDENT", old_indent);
            continue;
        }
        i = n(0);
        pos = n(0);
        let field_value = Value::new_hash();
        let mut tbl;
        for field in field_info.items() {
            let (mut num, size, mut typ) = (field.at(0), field.at(1), field.at(2));
            if !field.at(3).truthy() {
                // standard field
                tbl = sub_tbl.clone();
                if !tbl.get(num.to_bytes()).truthy() {
                    if common.get(num.to_bytes()).truthy() {
                        tbl = common.clone();
                        et.set("SET_GROUP1", sub_tbl.get("GROUPS").get("1"));
                    } else {
                        let info = hash_of(vec![("Name", cat(&[&msg_name, &s("_"), &num])), ("Unknown", n(1))]);
                        pl::call(it, "Image::ExifTool::AddTagToTable", vec![tbl.clone(), num.clone(), info])?;
                    }
                }
            } else {
                // developer field
                //
                // Example layout of developer messages:
                //
                // DevData: app=A idx=0
                // DevData: app=B idx=1
                // FieldDescr: idx=0 (app A) num=0 type=uint8  name="speed" units="km/h"
                // FieldDescr: idx=1 (app B) num=0 type=uint16 name="elev" units="m"
                // FieldDescr: idx=1 (app B) num=1 type=string name="model" units=""
                // Message XX definition:
                //   Fields (2)
                //     num=0 size=2 type=uint16 (for example)
                //     num=1 size=4 type=uint32 (for example)
                //   DevFields (2)
                //     num=0 size=1 idx=0 (app A speed)
                //     num=0 size=2 idx=1 (app B elev)
                //     num=1 size=5 idx=1 (app B model)
                // Message XX data:
                //     uint16 uint32 unit8(speed) uint16(elev) "A380\0"(model)
                //
                tbl = dev_tbl.clone();
                let idx = typ.clone();
                let dev = dev_data.get(idx.to_bytes());
                let dn = dev.get(num.to_bytes());
                if dev.truthy() && dn.truthy() && !dn.get("2").is_undef() && !dn.get("3").is_undef() && !dev.get("ID").get("1").is_undef() {
                    let hex = pl::unpack1("H*", &dev.get("ID").get("1"))?;
                    let id = cat(&[&hex, &s("_"), &dn.get("1")]);
                    if !tbl.get(id.to_bytes()).truthy() {
                        let mut name = dn.get("3");
                        re(r"($|_)([a-z])", "").subst(&mut name, true, |c| cap(c, 2).to_bytes().to_ascii_uppercase());
                        name = pl::call(it, "Image::ExifTool::MakeTagName", vec![name])?;
                        // make sure name is unique
                        for k in pl::call_list(it, "Image::ExifTool::TagTableKeys", vec![tbl.clone()])? {
                            let ti = tbl.get(k.to_bytes());
                            if pl::ref_of(&ti) != "HASH" {
                                continue;
                            }
                            let existing = ti.get("Name");
                            if !(existing.truthy() && existing.to_bytes()[..] == name.to_bytes()[..]) {
                                continue;
                            }
                            name = cat(&[&name, &s("-"), &idx]); // add developer data index
                            break;
                        }
                        let tag_info = hash_of(vec![("Name", name)]);
                        let mut units = dn.get("8");
                        if units.truthy() {
                            pl::tr(&mut units, "-_a-zA-Z0-9/+*", "", "dc");
                            tag_info.set("PrintConv", cat(&[&s("\"$val "), &units, &s("\"")]));
                        }
                        pl::call(it, "Image::ExifTool::AddTagToTable", vec![tbl.clone(), id.clone(), tag_info])?;
                    }
                    typ = dn.get("2");
                    num = id;
                } else {
                    if unknown.truthy() {
                        warn(it, &et, s("Incomplete developer field definition"))?;
                    }
                    pos = pl::add(&pos, &size);
                    continue;
                }
            }
            let bt = base_type.get(typ.to_bytes());
            if bt.truthy() {
                let fmt = bt.at(0);
                let fs = scalar(part2_format_size(it, fmt.clone()))?;
                pl::at(6585);
                let count = pl::div(&size, &fs)?;
                if pl::num_eq(&pl::int(&count), &count) {
                    let val = read_value(it, &buff_ref, pos.clone(), fmt.clone(), count, size.clone())?;
                    // ignore invalid values
                    // (the string representation of NaN maybe 'nan', '-NaN' or according
                    // to ChatGPT other junk such as '1.#QNAN' on some systems, so use a
                    // numerical test for this since NaN is not equal to itself numerically)
                    let invalid = bt.at(2);
                    let is_invalid = if pl::str_eq(&invalid, "NaN") { !pl::num_eq(&val, &val) } else { val.to_bytes()[..] == invalid.to_bytes()[..] };
                    if !is_invalid {
                        if !field.at(3).truthy() {
                            field_value.set(num.to_bytes(), val.clone());
                        }
                        let mut st = cat(&[&fmt, &s(" \""), &bt.at(1), &s("\"")]);
                        if field.at(3).truthy() {
                            st = cat(&[&st, &s(", devIdx="), &field.at(2)]);
                        }
                        let v = if fmt.eq_str("undef") { pl::sref(val) } else { val };
                        let idx = i.clone();
                        i = pl::add(&i, &n(1));
                        m(it, &et, "HandleTag", vec![tbl.clone(), num.clone(), v, s("DataPt"), buff_ref.clone(), s("DataPos"), data_pos.clone(), s("Start"), pos.clone(), s("Size"), size.clone(), s("Index"), idx, s("Format"), st])?;
                    }
                } else {
                    warn(it, &et, cat(&[&s("Bad count for "), &fmt, &s(" "), &msg_name, &s(" field "), &num]))?;
                }
            } else {
                warn(it, &et, cat(&[&s("Unknown field type "), &typ]))?;
            }
            et.delete("SET_GROUP1");
            pos = pl::add(&pos, &size);
        }
        et.set("INDENT", old_indent);
        // save messages required to decode developer fields
        if pl::num_eq(&msg_num, &n(207)) {
            // DeveloperDataID
            let idx = field_value.get("3"); // DeveloperDataIndex
            if !idx.is_undef() {
                dev_data.get_or_new_hash(idx.to_bytes()).set("ID", field_value.hash_copy());
            }
        } else if pl::num_eq(&msg_num, &n(206)) {
            // FieldDescription
            let idx = field_value.get("0"); // DeveloperDataIndex
            let num = field_value.get("1");
            if !idx.is_undef() && !num.is_undef() {
                dev_data.get_or_new_hash(idx.to_bytes()).set(num.to_bytes(), field_value.hash_copy());
            }
        }
    }
    if err.truthy() {
        warn(it, &et, err)?;
    }
    et.delete("DOC_NUM");
    ret(n(1))
}

fn part2_format_size(it: &mut Interp, fmt: Value) -> ER<Vec<Value>> {
    crate::modules::exiftool::part2::format_size(it, vec![fmt], Want::Scalar)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Garmin::CRC", crc);
    it.define_native("Image::ExifTool::Garmin::ProcessFIT", process_fit);
}
