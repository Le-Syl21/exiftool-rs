//! Image::ExifTool::Protobuf (Image/ExifTool/Protobuf.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Decode protocol buffer data

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, n, re};

const MODULE: &str = "Image::ExifTool::Protobuf";

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

/// The file's `my $NAME` variable.
fn lex(it: &mut Interp, name: &str) -> Value {
    crate::runtime::lex_scalar(it, MODULE, name)
        .borrow()
        .clone()
}

/// Read bytes from dirInfo object
/// Inputs: 0) dirInfo ref (with DataPt and Pos set), 1) number of bytes
/// Returns: binary data or undef on error
pub fn get_bytes(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (dir_info, nb) = (arg(&a, 0), arg(&a, 1));
    let data_pt = dir_info.get("DataPt");
    let pos = dir_info.get("Pos");
    if pl::gt(&pl::add(&pos, &nb), &pl::length(&data_pt.deref())) {
        return pl::ret_undef();
    }
    dir_info.set("Pos", pl::add(&dir_info.get("Pos"), &nb));
    pl::ret(pl::substr(
        &data_pt.deref(),
        pos.to_i64(),
        Some(nb.to_i64()),
    ))
}

fn get_bytes1(it: &mut Interp, dir_info: &Value, nb: i64) -> ER<Value> {
    Ok(get_bytes(it, vec![dir_info.clone(), n(nb)], Want::Scalar)?
        .pop()
        .unwrap_or_default())
}

/// Read variable-length integer
/// Inputs: 0) dirInfo ref
/// Returns: integer value
/// - sets $$dirInfo{Bit0} according to bit 0 of returned value
///   (necessary for cases where a signed integer exceeds $intMax)
pub fn var_int(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let dir_info = arg(&a, 0);
    let mut buff = get_bytes1(it, &dir_info, 1)?;
    if buff.is_undef() {
        return pl::ret_undef();
    }
    let mut val = ops::bit_and(&ord(&buff), &n(0x7f));
    dir_info.set("Bit0", ops::bit_and(&val, &n(0x01)));
    let mut mult = n(128);
    let mut i = 0;
    loop {
        if !ops::bit_and(&ord(&buff), &n(0x80)).truthy() {
            break;
        }
        buff = get_bytes1(it, &dir_info, 1)?;
        if buff.is_undef() {
            return pl::ret_undef();
        }
        val = pl::add(&val, &pl::mul(&ops::bit_and(&ord(&buff), &n(0x7f)), &mult));
        if !ops::bit_and(&ord(&buff), &n(0x80)).truthy() {
            break;
        }
        i += 1;
        if i > 32 {
            return pl::ret_undef(); // set a reasonable limit
        }
        mult = pl::mul(&mult, &n(128)); // (Note: don't use integer bit shift to avoid integer overflow)
    }
    pl::ret(val)
}

fn var_int1(it: &mut Interp, dir_info: &Value) -> ER<Value> {
    Ok(var_int(it, vec![dir_info.clone()], Want::Scalar)?
        .pop()
        .unwrap_or_default())
}

/// Read protobuf record
/// Inputs: 0) dirInfo ref
/// Returns: 0) record payload (plus tag id and format type in list context) or undef on error
/// Notes: Updates $$dirInfo{Pos} to start of next record, and sets $$dirInfo{Bit0}
///        according to the least significant bit of type 0 (varInt) records
pub fn read_record(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let dir_info = arg(&a, 0);
    let val = var_int1(it, &dir_info)?;
    if val.is_undef() {
        return pl::ret_undef();
    }
    let id = ops::shift_right(&val, &n(3));
    let typ = ops::bit_and(&val, &n(0x07));
    let mut buff = Value::Undef;

    match typ.to_i64() {
        // varInt
        0 => buff = var_int1(it, &dir_info)?,
        // 64-bit number
        1 => buff = get_bytes1(it, &dir_info, 8)?,
        // string, bytes or protobuf
        2 => {
            let len = var_int1(it, &dir_info)?;
            if len.truthy() {
                buff = get_bytes(it, vec![dir_info.clone(), len], Want::Scalar)?
                    .pop()
                    .unwrap_or_default();
            } else {
                buff = pl::s("");
            }
        }
        // (deprecated start group)
        3 => buff = pl::s(""),
        // (deprecated end group)
        4 => buff = pl::s(""),
        // 32-bit number
        5 => buff = get_bytes1(it, &dir_info, 4)?,
        _ => {}
    }
    if want == Want::List {
        Ok(vec![buff, id, typ])
    } else {
        pl::ret(buff)
    }
}

/// Check to see if this could be a protobuf object
/// Inputs: 0) data reference
/// Retursn: true if this looks like a protobuf
pub fn is_protobuf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let pt = arg(&a, 0);
    let dir_info = Value::new_hash();
    dir_info.set("DataPt", pt.clone());
    dir_info.set("Pos", n(0));
    loop {
        let r = read_record(it, vec![dir_info.clone()], Want::Scalar)?
            .pop()
            .unwrap_or_default();
        if r.is_undef() {
            return pl::ret(n(0));
        }
        if pl::num_eq(&dir_info.get("Pos"), &pl::length(&pt.deref())) {
            return pl::ret(n(1));
        }
    }
}

/// The signed interpretation of a zigzag-encoded varInt.
fn zigzag(dir_info: &Value, val: &Value, int_max: &Value) -> ER<Value> {
    Ok(if pl::gt(val, int_max) {
        // use double math (15 decimal digits precision)
        if dir_info.get("Bit0").truthy() {
            pl::sub(&pl::neg(&pl::int(&pl::div(val, &n(2))?)), &n(1))
        } else {
            pl::div(val, &n(2))?
        }
    } else if ops::bit_and(val, &n(1)).truthy() {
        // use integer math
        pl::sub(&pl::neg(&ops::shift_right(val, &n(1))), &n(1))
    } else {
        ops::shift_right(val, &n(1))
    })
}

/// Process protobuf data (eg. DJI djmd timed data from Action4 videos) (ref 1)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref with DataPt, DataPos, DirName and Base,
///         2) tag table ptr, 3) prefix of parent protobuf ID's
/// Returns: true on success
pub fn process_protobuf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl, mut prefix) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    let int_max = lex(it, "intMax");
    let int64s_min = lex(it, "int64sMin");
    let data_pt = dir_info.get("DataPt");
    let dir_name = dir_info.get("DirName");
    let dir_start = {
        let v = dir_info.get("DirStart");
        if v.truthy() {
            v
        } else {
            n(0)
        }
    };
    let dir_len = {
        let v = dir_info.get("DirLen");
        if v.truthy() {
            v
        } else {
            pl::sub(&pl::length(&data_pt.deref()), &dir_start)
        }
    };
    let dir_end = pl::add(&dir_start, &dir_len);
    let or0 = |v: Value| if v.truthy() { v } else { n(0) };
    let data_pos = pl::add(&or0(dir_info.get("Base")), &or0(dir_info.get("DataPos")));
    let mut unknown = pl::method(it, &et, "Options", vec![pl::s("Unknown")])?;
    if !unknown.truthy() {
        unknown = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    }

    dir_info.set("Pos", or0(dir_info.get("DirStart"))); // initialize buffer Pos
    pl::method(
        it,
        &et,
        "VerboseDir",
        vec![pl::s("Protobuf"), Value::Undef, dir_len.clone()],
    )?;
    if !prefix.truthy() {
        prefix = pl::s("");
        let pp = et.get_or_new_hash("ProtoPrefix");
        if pp.get(dir_name.to_bytes()).is_undef() {
            pp.set(dir_name.to_bytes(), pl::s(""));
        }
        call(it, "SetByteOrder", vec![pl::s("II")])?;
    }
    // prefix for unknown tags
    let unk_pre = if tag_tbl.get("TAG_PREFIX").truthy() {
        pl::concat(&tag_tbl.get("TAG_PREFIX"), &pl::s("_"))
    } else {
        pl::s("Protobuf ")
    };

    // loop through protobuf records
    loop {
        let pos = dir_info.get("Pos");
        if pl::ge(&pos, &dir_end) {
            break;
        }
        let r = read_record(it, vec![dir_info.clone()], Want::List)?;
        let (buff, id, typ) = (arg(&r, 0), arg(&r, 1), arg(&r, 2));
        if buff.is_undef() {
            pl::method(it, &et, "Warn", vec![pl::s("Protobuf format error")])?;
            break;
        }
        if pl::num_eq(&typ, &n(2)) && re(r"\.proto$", "").is_match(&buff) {
            // save protocol name separately for directory type
            et.get_or_new_hash("ProtoPrefix").set(
                dir_name.to_bytes(),
                pl::concat(&pl::substr(&buff, 0, Some(-6)), &pl::s("_")),
            );
            pl::method(
                it,
                &et,
                "HandleTag",
                vec![tag_tbl.clone(), pl::s("Protocol"), buff.clone()],
            )?;
        }
        let tag = cat(&[
            et.get_or_new_hash("ProtoPrefix").get(dir_name.to_bytes()),
            prefix.clone(),
            id.clone(),
        ]);
        let mut tag_info = tag_tbl.get(tag.to_bytes());
        if tag_info.truthy() {
            if !pl::num_eq(&typ, &n(2)) && tag_info.get("Unknown").truthy() && !unknown.truthy() {
                continue;
            }
        } else {
            if !(pl::num_eq(&typ, &n(2)) || unknown.truthy()) {
                continue;
            }
            let h = Value::new_hash();
            h.set("Unknown", n(1));
            tag_info = call(it, "AddTagToTable", vec![tag_tbl.clone(), tag.clone(), h])?;
        }
        // set IsProtobuf flag (only for Unknown tags) if necessary
        if pl::num_eq(&typ, &n(2)) && tag_info.get("Unknown").truthy() {
            if tag_info.get("IsProtobuf").truthy() {
                if !is_protobuf(it, vec![pl::sref(buff.clone())], Want::Scalar)?
                    .pop()
                    .unwrap_or_default()
                    .truthy()
                {
                    tag_info.set("IsProtobuf", n(0));
                }
            } else if tag_info.get("IsProtobuf").is_undef()
                && re(r"[^\x20-\x7e]", "").is_match(&buff)
                && is_protobuf(it, vec![pl::sref(buff.clone())], Want::Scalar)?
                    .pop()
                    .unwrap_or_default()
                    .truthy()
            {
                tag_info.set("IsProtobuf", n(1));
            }
            if !(tag_info.get("IsProtobuf").truthy() || unknown.truthy()) {
                continue;
            }
        }
        // format binary payload into a useful value
        let mut val;
        let format = tag_info.get("Format");
        if format.truthy() {
            if pl::num_eq(&typ, &n(0)) {
                val = buff.clone();
                if pl::str_eq(&format, "signed") {
                    val = zigzag(&dir_info, &val, &int_max)?;
                } else if pl::str_eq(&format, "int64s") && pl::ge(&val, &int64s_min) {
                    // hack for DJI drones which store 64-bit signed integers improperly
                    // (just toss upper 32 bits which should be all 1's anyway)
                    // Note: do the two subtractions because $int64sMin + 4294967296
                    // is too large for a 64-bit integer
                    val = pl::sub(&pl::sub(&val, &int64s_min), &n(4294967296));
                }
            } else if pl::num_eq(&typ, &n(2)) && pl::str_eq(&format, "rational") {
                let dir = Value::new_hash();
                dir.set("DataPt", pl::sref(buff.clone()));
                dir.set("Pos", n(0));
                let num = var_int1(it, &dir)?;
                let den = var_int1(it, &dir)?;
                val = if !num.is_undef() && den.truthy() {
                    pl::div(&num, &den)?
                } else {
                    pl::s("err")
                };
            } else {
                val = call(
                    it,
                    "ReadValue",
                    vec![
                        pl::sref(buff.clone()),
                        n(0),
                        format.clone(),
                        Value::Undef,
                        pl::length(&buff),
                    ],
                )?;
            }
        } else if pl::num_eq(&typ, &n(0)) {
            // varInt
            val = buff.clone();
            let hex = pl::sprintf("%x", &[val.clone()]);
            if pl::ge(&val, &int64s_min) {
                let s64 = pl::sub(&pl::sub(&val, &int64s_min), &n(4294967296));
                val = cat(&[
                    val.clone(),
                    pl::s(" (0x"),
                    hex,
                    pl::s(", int64s "),
                    s64,
                    pl::s(")"),
                ]);
            } else {
                let signed = zigzag(&dir_info, &val, &int_max)?;
                val = cat(&[
                    val.clone(),
                    pl::s(" (0x"),
                    hex,
                    pl::s(", signed "),
                    signed,
                    pl::s(")"),
                ]);
            }
        } else if pl::num_eq(&typ, &n(1)) {
            // 64-bit number
            let d = call(it, "GetDouble", vec![pl::sref(buff.clone()), n(0)])?;
            val = cat(&[
                pl::s("0x"),
                pl::unpack1("H*", &buff)?,
                pl::s(" (double "),
                d,
                pl::s(")"),
            ]);
        } else if pl::num_eq(&typ, &n(2)) {
            // string, bytes or protobuf
            if tag_info.get("SubDirectory").truthy() {
                // (fall through to process known SubDirectory)
                val = Value::Undef;
            } else if tag_info.get("IsProtobuf").truthy() {
                // process Unknown protobuf directories
                pl::method(
                    it,
                    &et,
                    "VPrint",
                    vec![
                        n(1),
                        cat(&[
                            et.get("INDENT"),
                            unk_pre.clone(),
                            tag.clone(),
                            pl::s(" ("),
                            pl::length(&buff),
                            pl::s(" bytes) -->\n"),
                        ]),
                    ],
                )?;
                let addr = pl::sub(
                    &pl::add(&data_pos, &dir_info.get("Pos")),
                    &pl::length(&buff),
                );
                let buff_ref = pl::sref(buff.clone());
                pl::method(
                    it,
                    &et,
                    "VerboseDump",
                    vec![
                        buff_ref.clone(),
                        pl::s("Addr"),
                        addr.clone(),
                        pl::s("Prefix"),
                        et.get("INDENT"),
                    ],
                )?;
                let subdir = Value::new_hash();
                subdir.set("DataPt", buff_ref);
                subdir.set("DataPos", addr);
                subdir.set("DirName", dir_name.clone());
                et.set("INDENT", pl::concat(&et.get("INDENT"), &pl::s("| ")));
                process_protobuf(
                    it,
                    vec![
                        et.clone(),
                        subdir,
                        tag_tbl.clone(),
                        cat(&[prefix.clone(), id.clone(), pl::s("-")]),
                    ],
                    Want::Scalar,
                )?;
                et.set("INDENT", pl::substr(&et.get("INDENT"), 0, Some(-2)));
                continue;
            } else {
                // check for rational value (2 varInt values)
                let mut rat = Value::Undef;
                let dir = Value::new_hash();
                dir.set("DataPt", pl::sref(buff.clone()));
                dir.set("Pos", n(0));
                let num = var_int1(it, &dir)?;
                if !num.is_undef() {
                    let denom = var_int1(it, &dir)?;
                    if denom.truthy() && pl::num_eq(&dir.get("Pos"), &pl::length(&buff)) {
                        rat = cat(&[pl::s(" (rational "), num, pl::s("/"), denom, pl::s(")")]);
                    }
                }
                if !re(r"[^\r\n\t\x20-\x7e]", "").is_match(&buff) {
                    val = buff.clone(); // assume this is an ASCII string
                } else if pl::modulo(&pl::length(&buff), &n(4))?.truthy() {
                    val = pl::concat(&pl::s("0x"), &pl::unpack1("H*", &buff)?);
                } else {
                    let nn = pl::div(&pl::length(&buff), &n(4))?;
                    // (do this instead of '(H8)*' because older Perl version didn't support this)
                    let t = format!("(H8){}", nn.to_text());
                    val = pl::concat(&pl::s("0x"), &pl::join(" ", &pl::unpack(&t, &buff)?));
                    // (group in 4-byte blocks)
                }
                if rat.truthy() {
                    val = pl::concat(&val, &rat);
                }
            }
        } else if pl::num_eq(&typ, &n(5)) {
            // 32-bit number
            let bref = pl::sref(buff.clone());
            val = cat(&[
                pl::s("0x"),
                pl::unpack1("H*", &buff)?,
                pl::s(" (int32u "),
                call(it, "Get32u", vec![bref.clone(), n(0)])?,
            ]);
            if ops::bit_and(&ord(&pl::substr(&buff, 3, Some(1))), &n(0x80)).truthy() {
                val = cat(&[
                    val.clone(),
                    pl::s(", int32s "),
                    call(it, "Get32s", vec![bref.clone(), n(0)])?,
                ]);
            }
            val = cat(&[
                val.clone(),
                pl::s(", float "),
                call(it, "GetFloat", vec![bref, n(0)])?,
                pl::s(")"),
            ]);
        } else {
            val = buff.clone();
        }
        // get length of data in the record
        let start = if pl::num_eq(&typ, &n(0)) {
            pl::add(&pos, &n(1))
        } else {
            pl::sub(&dir_info.get("Pos"), &pl::length(&buff))
        };
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![
                tag_tbl.clone(),
                tag.clone(),
                val,
                pl::s("DataPt"),
                data_pt.clone(),
                pl::s("DataPos"),
                data_pos.clone(),
                pl::s("Start"),
                start.clone(),
                pl::s("Size"),
                pl::sub(&dir_info.get("Pos"), &start),
                pl::s("Extra"),
                pl::concat(&pl::s(", type="), &typ),
                pl::s("Format"),
                tag_info.get("Format"),
            ],
        )?;
    }
    // warn if we didn't finish exactly at the end of the buffer
    if !(prefix.truthy() || pl::num_eq(&dir_info.get("Pos"), &dir_end)) {
        pl::method(it, &et, "Warn", vec![pl::s("Truncated protobuf data")])?;
    }
    pl::ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Protobuf::GetBytes", get_bytes);
    it.define_native("Image::ExifTool::Protobuf::VarInt", var_int);
    it.define_native("Image::ExifTool::Protobuf::ReadRecord", read_record);
    it.define_native("Image::ExifTool::Protobuf::IsProtobuf", is_protobuf);
    it.define_native(
        "Image::ExifTool::Protobuf::ProcessProtobuf",
        process_protobuf,
    );
}
