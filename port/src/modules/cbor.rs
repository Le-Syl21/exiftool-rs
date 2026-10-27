//! Image::ExifTool::CBOR (Image/ExifTool/CBOR.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read CBOR format metadata

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, n};

const MODULE: &str = "Image::ExifTool::CBOR";

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `$et->VPrint(LEVEL, MSG)`
fn vprint(it: &mut Interp, et: &Value, level: i64, msg: Value) -> ER<()> {
    pl::method(it, et, "VPrint", vec![n(level), msg])?;
    Ok(())
}

/// `"a" . "b" . ...`
fn cat(parts: &[Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

/// `' ' x $n`
fn spaces(count: &Value) -> Value {
    Value::bytes(vec![b' '; count.to_i64().max(0) as usize])
}

/// The list `(undef, $err, $pos)` returned on error.
fn err_ret(err: &str, pos: &Value) -> ER<Vec<Value>> {
    Ok(vec![Value::Undef, pl::s(err), pos.clone()])
}

/// `Image::ExifTool::HexDump($dataPt, $len, Start => .., DataPos => .., Prefix => .., [MaxLen => ..,] Out => ..)`
fn hex_dump(
    it: &mut Interp,
    et: &Value,
    data_pt: &Value,
    len: Value,
    start: &Value,
    prefix: Value,
    max_len: Option<Value>,
) -> ER<()> {
    let mut args = vec![
        data_pt.clone(),
        len,
        pl::s("Start"),
        start.clone(),
        pl::s("DataPos"),
        et.get("cbor_datapos"),
        pl::s("Prefix"),
        prefix,
    ];
    if let Some(m) = max_len {
        args.push(pl::s("MaxLen"));
        args.push(m);
    }
    args.push(pl::s("Out"));
    args.push(pl::method(it, et, "Options", vec![pl::s("TextOut")])?);
    call(it, "HexDump", args)?;
    Ok(())
}

/// Read CBOR value
/// Inputs: 0) ExifTool ref, 1) data ref, 2) position in data, 3) data end
/// Returns: 0) value, 1) error string, 2) new data position
pub fn read_cbor_value(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, data_pt, mut pos, end) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    if pl::ge(&pos, &end) {
        return err_ret("Truncated CBOR data", &pos);
    }
    let verbose = et.get("OPTIONS").get("Verbose");
    let indent = et.get("INDENT");
    let mut dump_start = pos.clone();
    let mut fmt = call(it, "Get8u", vec![data_pt.clone(), pos.clone()])?;
    pos = pl::add(&pos, &n(1));
    let dat = ops::bit_and(&fmt, &n(0x1f));
    let mut num;
    let mut val;
    let mut err = Value::Undef;
    let mut size = Value::Undef;
    fmt = ops::shift_right(&fmt, &n(5));
    if pl::lt(&dat, &n(24)) {
        num = dat.clone();
    } else if pl::num_eq(&dat, &n(31)) {
        // indefinite count (not used in C2PA)
        num = n(-1); // (flag for indefinite count)
        vprint(
            it,
            &et,
            1,
            cat(&[et.get("INDENT"), pl::s(" (indefinite count):\n")]),
        )?;
    } else {
        let format = match dat.to_i64() {
            24 => pl::s("int8u"),
            25 => pl::s("int16u"),
            26 => pl::s("int32u"),
            27 => pl::s("int64u"),
            _ => Value::Undef,
        };
        if !format.truthy() {
            return Ok(vec![
                Value::Undef,
                cat(&[pl::s("Invalid CBOR integer type "), dat.clone()]),
                pos,
            ]);
        }
        size = call(it, "FormatSize", vec![format.clone()])?;
        if pl::gt(&pl::add(&pos, &size), &end) {
            return err_ret("Truncated CBOR integer value", &pos);
        }
        num = call(
            it,
            "ReadValue",
            vec![data_pt.clone(), pos.clone(), format, n(1), size.clone()],
        )?;
        pos = pl::add(&pos, &size);
    }
    let mut pre = pl::s("");
    if !et.get("cbor_pre").is_undef() && !pl::num_eq(&fmt, &n(6)) {
        pre = et.get("cbor_pre");
        et.delete("cbor_pre");
    }
    if pl::num_eq(&fmt, &n(0)) {
        // positive integer
        val = num.clone();
        vprint(
            it,
            &et,
            1,
            cat(&[
                et.get("INDENT"),
                pl::s(" "),
                pre.clone(),
                pl::s("int+: "),
                val.clone(),
                pl::s("\n"),
            ]),
        )?;
    } else if pl::num_eq(&fmt, &n(1)) {
        // negative integer
        val = pl::mul(&n(-1), &num);
        vprint(
            it,
            &et,
            1,
            cat(&[
                et.get("INDENT"),
                pl::s(" "),
                pre.clone(),
                pl::s("int-: "),
                val.clone(),
                pl::s("\n"),
            ]),
        )?;
    } else if pl::num_eq(&fmt, &n(2)) || pl::num_eq(&fmt, &n(3)) {
        // byte/UTF8 string
        if pl::gt(&pl::add(&pos, &num), &end) {
            return err_ret("Truncated CBOR string value", &pos);
        }
        if pl::lt(&num, &n(0)) {
            // (should not happen in C2PA)
            let mut string = pl::s("");
            et.set("INDENT", pl::concat(&et.get("INDENT"), &pl::s("  ")));
            loop {
                let r = read_cbor_value(
                    it,
                    vec![et.clone(), data_pt.clone(), pos.clone(), end.clone()],
                    Want::List,
                )?;
                val = arg(&r, 0);
                err = arg(&r, 1);
                pos = arg(&r, 2);
                if err.truthy() {
                    return Ok(vec![Value::Undef, err, pos]);
                }
                if val.is_undef() {
                    break; // hit the break?
                }
                // (note: strictly we should be checking that this was a string we read)
                string = pl::concat(&string, &val);
            }
            et.set("INDENT", indent);
            return Ok(vec![string, Value::Undef, pos]); // return concatenated byte/text string
        } else {
            val = pl::substr(&data_pt.deref(), pos.to_i64(), Some(num.to_i64()));
        }
        pos = pl::add(&pos, &num);
        if pl::num_eq(&fmt, &n(2)) {
            // (byte string)
            vprint(
                it,
                &et,
                1,
                cat(&[
                    et.get("INDENT"),
                    pl::s(" "),
                    pre.clone(),
                    pl::s("byte: <binary data "),
                    pl::length(&val),
                    pl::s(" bytes>\n"),
                ]),
            )?;
            let dat = val.clone();
            val = pl::sref(dat); // use scalar reference for binary data
        } else {
            // (text string)
            val = pl::method(it, &et, "Decode", vec![val, pl::s("UTF8")])?;
            vprint(
                it,
                &et,
                1,
                cat(&[
                    et.get("INDENT"),
                    pl::s(" "),
                    pre.clone(),
                    pl::s("text: '"),
                    val.clone(),
                    pl::s("'\n"),
                ]),
            )?;
        }
    } else if pl::num_eq(&fmt, &n(4)) || pl::num_eq(&fmt, &n(5)) {
        // list/hash
        if pl::num_eq(&fmt, &n(4)) {
            vprint(
                it,
                &et,
                1,
                cat(&[
                    et.get("INDENT"),
                    pl::s(" "),
                    pre.clone(),
                    pl::s("list: <"),
                    num.clone(),
                    pl::s(" elements>\n"),
                ]),
            )?;
        } else {
            vprint(
                it,
                &et,
                1,
                cat(&[
                    et.get("INDENT"),
                    pl::s(" "),
                    pre.clone(),
                    pl::s("hash: <"),
                    num.clone(),
                    pl::s(" pairs>\n"),
                ]),
            )?;
            num = pl::mul(&num, &n(2));
        }
        et.set("INDENT", pl::concat(&et.get("INDENT"), &pl::s("  ")));
        let mut i = n(0);
        let mut list: Vec<Value> = Vec::new();
        if pl::gt(&verbose, &n(2)) {
            hex_dump(
                it,
                &et,
                &data_pt,
                pl::sub(&pos, &dump_start),
                &dump_start,
                et.get("INDENT"),
                None,
            )?;
        }
        while num.truthy() {
            et.set("cbor_pre", cat(&[i.clone(), pl::s(") ")]));
            if pl::num_eq(&fmt, &n(4)) {
                i = pl::add(&i, &n(1));
            } else if ops::bit_and(&num, &n(0x01)).truthy() {
                et.set("cbor_pre", spaces(&pl::length(&et.get("cbor_pre"))));
                i = pl::add(&i, &n(1));
            }
            let r = read_cbor_value(
                it,
                vec![et.clone(), data_pt.clone(), pos.clone(), end.clone()],
                Want::List,
            )?;
            val = arg(&r, 0);
            err = arg(&r, 1);
            pos = arg(&r, 2);
            if err.truthy() {
                return Ok(vec![Value::Undef, err, pos]);
            }
            if val.is_undef() {
                if !pl::lt(&num, &n(0)) {
                    return err_ret("Unexpected list terminator", &pos);
                }
                break;
            }
            list.push(val.clone());
            num = pl::sub(&num, &n(1));
        }
        dump_start = pos.clone();
        et.set("INDENT", indent.clone());
        if pl::num_eq(&fmt, &n(5)) {
            let keys = Value::new_array(Vec::new());
            let hash = Value::new_hash();
            hash.set("_ordered_keys_", keys.clone());
            let mut i = 0usize;
            while (i as i64) < list.len() as i64 - 1 {
                hash.set(list[i].to_bytes(), list[i + 1].clone());
                keys.push(list[i].clone()); // save ordered list of keys
                i += 2;
            }
            val = hash;
        } else {
            val = Value::new_array(list);
        }
    } else if pl::num_eq(&fmt, &n(6)) {
        // optional tag
        if verbose.truthy() {
            let cbor_type6 = crate::runtime::lex_hash(it, MODULE, "cborType6");
            let t = cbor_type6.get(num.to_bytes());
            let str_ = cat(&[
                num.clone(),
                pl::s(" ("),
                if t.truthy() { t } else { pl::s("unknown") },
                pl::s(")"),
            ]);
            let spc = if et.get("cbor_pre").truthy() {
                spaces(&pl::length(&et.get("cbor_pre")))
            } else {
                pl::s("")
            };
            vprint(
                it,
                &et,
                1,
                cat(&[
                    et.get("INDENT"),
                    pl::s(" "),
                    spc,
                    pl::s("<CBOR optional type "),
                    str_,
                    pl::s(">\n"),
                ]),
            )?;
            if pl::gt(&verbose, &n(2)) {
                hex_dump(
                    it,
                    &et,
                    &data_pt,
                    pl::sub(&pos, &dump_start),
                    &dump_start,
                    pl::concat(&et.get("INDENT"), &pl::s("  ")),
                    None,
                )?;
            }
        }
        // read next value (note: in the case of multiple tags,
        // this nesting will apply the tags in the correct order)
        let r = read_cbor_value(
            it,
            vec![et.clone(), data_pt.clone(), pos.clone(), end.clone()],
            Want::List,
        )?;
        val = arg(&r, 0);
        err = arg(&r, 1);
        pos = arg(&r, 2);
        if err.truthy() {
            return Ok(vec![Value::Undef, err, pos]);
        }
        dump_start = pos.clone();
        // convert some values according to the optional tag number (untested)
        if pl::num_eq(&num, &n(0)) && !val.is_ref() {
            // date/time string
            crate::runtime::require(it, "Image::ExifTool::XMP")?;
            val = pl::call(it, "Image::ExifTool::XMP::ConvertXMPDate", vec![val])?;
        } else if pl::num_eq(&num, &n(1)) && !val.is_ref() {
            // epoch-based date/time
            if call(it, "IsFloat", vec![val.clone()])?.truthy() {
                let dec = if pl::num_eq(&val, &pl::int(&val)) {
                    Value::Undef
                } else {
                    n(6)
                };
                val = call(it, "ConvertUnixTime", vec![val, n(1), dec])?;
            }
        } else if (pl::num_eq(&num, &n(2)) || pl::num_eq(&num, &n(3)))
            && pl::ref_of(&val) == "SCALAR"
        {
            // pos/neg bignum
            let mut big = n(0);
            let last = pl::length(&val.deref()).to_i64() - 1;
            for k in 0..=last {
                big = pl::add(
                    &pl::mul(&n(256), &big),
                    &call(it, "Get8u", vec![val.clone(), n(k)])?,
                );
            }
            val = if pl::num_eq(&num, &n(2)) {
                big
            } else {
                pl::neg(&big)
            };
        } else if (pl::num_eq(&num, &n(4)) || pl::num_eq(&num, &n(5)))
            && pl::ref_of(&val) == "ARRAY"
            && val.len() == 2
            && call(it, "IsInt", vec![val.at(0)])?.truthy()
            && call(it, "IsInt", vec![val.at(1)])?.truthy()
        {
            // decimal fraction or bigfloat
            let base = if pl::num_eq(&num, &n(4)) { n(10) } else { n(2) };
            val = pl::mul(&val.at(1), &ops::pow(&base, &val.at(0)));
        }
    } else if pl::num_eq(&fmt, &n(7)) {
        if pl::num_eq(&dat, &n(31)) {
            val = Value::Undef; // "break" = end of indefinite array/hash (not used in C2PA)
        } else if pl::lt(&dat, &n(24)) {
            let cbor_type7 = crate::runtime::lex_hash(it, MODULE, "cborType7");
            val = cbor_type7.get(num.to_bytes());
            if val.is_undef() {
                val = cat(&[pl::s("Unknown ("), val.clone(), pl::s(")")]);
            }
        } else if pl::num_eq(&dat, &n(25)) {
            // half-precision float
            let exp = ops::bit_and(&ops::shift_right(&num, &n(10)), &n(0x1f));
            let mant = ops::bit_and(&num, &n(0x3ff));
            // (the Perl tests "exp != 31", which is exp($_) != 31)
            let topic = it.get_scalar("main::_");
            if pl::num_eq(&exp, &n(0)) {
                val = ops::pow(&mant, &n(-24));
                if ops::bit_and(&num, &n(0x8000)).truthy() {
                    val = pl::mul(&val, &n(-1));
                }
            } else if topic.to_f64().exp() != 31.0 {
                val = ops::pow(&pl::add(&mant, &n(1024)), &pl::sub(&exp, &n(25)));
                if ops::bit_and(&num, &n(0x8000)).truthy() {
                    val = pl::mul(&val, &n(-1));
                }
            } else {
                val = if pl::num_eq(&mant, &n(0)) {
                    pl::s("<inf>")
                } else {
                    pl::s("<nan>")
                };
            }
        } else if pl::num_eq(&dat, &n(26)) {
            // float
            val = call(it, "GetFloat", vec![data_pt.clone(), pl::sub(&pos, &size)])?;
        } else if pl::num_eq(&dat, &n(27)) {
            // double
            val = call(it, "GetDouble", vec![data_pt.clone(), pl::sub(&pos, &size)])?;
        } else {
            return Ok(vec![
                Value::Undef,
                cat(&[pl::s("Invalid CBOR type 7 variant "), num.clone()]),
                pos,
            ]);
        }
        let shown = if val.is_undef() {
            pl::s("<break>")
        } else {
            val.clone()
        };
        vprint(
            it,
            &et,
            1,
            cat(&[
                et.get("INDENT"),
                pl::s(" "),
                pre.clone(),
                pl::s("typ7: "),
                shown,
                pl::s("\n"),
            ]),
        )?;
    } else {
        return Ok(vec![
            Value::Undef,
            cat(&[pl::s("Unknown CBOR format "), fmt.clone()]),
            pos,
        ]);
    }
    if pl::gt(&verbose, &n(2)) {
        let max_len = if pl::lt(&verbose, &n(5)) {
            if pl::num_eq(&verbose, &n(3)) {
                n(96)
            } else {
                n(2048)
            }
        } else {
            Value::Undef
        };
        hex_dump(
            it,
            &et,
            &data_pt,
            pl::sub(&pos, &dump_start),
            &dump_start,
            pl::concat(&et.get("INDENT"), &pl::s("  ")),
            Some(max_len),
        )?;
    }
    Ok(vec![val, err, pos])
}

/// Read CBOR box
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_cbor(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let data_pt = dir_info.get("DataPt");
    let mut pos = dir_info.get("DirStart");
    let end = pl::add(&pos, &dir_info.get("DirLen"));

    pl::method(
        it,
        &et,
        "VerboseDir",
        vec![pl::s("CBOR"), Value::Undef, dir_info.get("DirLen")],
    )?;
    call(it, "SetByteOrder", vec![pl::s("MM")])?;

    et.set(
        "cbor_datapos",
        pl::add(&dir_info.get("DataPos"), &dir_info.get("Base")),
    );

    while pl::lt(&pos, &end) {
        let r = read_cbor_value(
            it,
            vec![et.clone(), data_pt.clone(), pos.clone(), end.clone()],
            Want::List,
        )?;
        let (val, err) = (arg(&r, 0), arg(&r, 1));
        pos = arg(&r, 2);
        if err.truthy() {
            pl::method(it, &et, "Warn", vec![err])?;
            break;
        }
        if pl::ref_of(&val) == "HASH" {
            for tag in val.get("_ordered_keys_").items() {
                let v = val.get(tag.to_bytes());
                pl::call(
                    it,
                    "Image::ExifTool::JSON::ProcessTag",
                    vec![et.clone(), tag_table_ptr.clone(), tag, v],
                )?;
            }
        } else if pl::ref_of(&val) == "ARRAY" {
            let mut i = 0;
            while i < val.len() {
                pl::call(
                    it,
                    "Image::ExifTool::JSON::ProcessTag",
                    vec![
                        et.clone(),
                        tag_table_ptr.clone(),
                        pl::s(&format!("Item{i}")),
                        val.at(i as i64),
                    ],
                )?;
                i += 1;
            }
        } else if pl::str_eq(&val, "0") {
            vprint(it, &et, 1, cat(&[et.get("INDENT"), pl::s(" <CBOR end>\n")]))?;
            break; // (treat as padding)
        } else {
            vprint(
                it,
                &et,
                1,
                cat(&[
                    et.get("INDENT"),
                    pl::s(" Unknown value: "),
                    val,
                    pl::s("\n"),
                ]),
            )?;
        }
    }
    pl::ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::CBOR::ReadCBORValue", read_cbor_value);
    it.define_native("Image::ExifTool::CBOR::ProcessCBOR", process_cbor);
}
