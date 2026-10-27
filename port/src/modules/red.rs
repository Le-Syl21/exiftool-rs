//! Image::ExifTool::Red (Image/ExifTool/Red.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::exiftool::part2::{get16u, get8u};
use crate::modules::random_access::{cell, raf_read_cell};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, n, re, ret};

/// `$raf->Read(...)`: die before the call, as perl does on an undefined or
/// unblessed invocant (the read helper runs the call as interpreted code,
/// which would name its own location in the message).
fn check_invocant(it: &mut Interp, raf: &Value, name: &str) -> ER<()> {
    match raf {
        Value::Str(_) => Ok(()),
        Value::HashRef(h) if h.borrow().blessed.is_some() => Ok(()),
        _ => pl::method(it, raf, name, vec![]).map(|_| ()),
    }
}

const MODULE: &str = "Image::ExifTool::Red";

fn val(c: &Cell) -> Value {
    c.borrow().clone()
}

/// `\$var` of a `my` variable kept in a cell.
fn rf(c: &Cell) -> Value {
    Value::ScalarRef(c.clone())
}

fn g16u(it: &mut Interp, data_pt: &Value, pos: Value) -> ER<Value> {
    Ok(get16u(it, vec![data_pt.clone(), pos], Want::Scalar)?.pop().unwrap_or_default())
}

fn g8u(it: &mut Interp, data_pt: &Value, pos: Value) -> ER<Value> {
    Ok(get8u(it, vec![data_pt.clone(), pos], Want::Scalar)?.pop().unwrap_or_default())
}

/// `return $et->Warn(MSG)`, in the caller's context.
fn ret_warn(it: &mut Interp, et: &Value, msg: Value, want: Want) -> ER<Vec<Value>> {
    it.call_method(et, "Warn", vec![et.clone(), msg], want)
}

/// Process metadata from a Redcode R3D video (ref PH)
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid R3D file
pub fn process_r3d(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let (buff, buf2) = (cell(Value::Undef), cell(Value::Undef));
    let (mut pos, mut dir_len, dir_end);
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let err_trunc = crate::runtime::lex_scalar(it, MODULE, "errTrunc").borrow().clone();

    // R3D file structure:
    // - each block starts with int32u block size followed by 4-byte block type
    // - first block type is either "RED1" (version 1) or "RED2" (version 2)
    // - blocks begin on even 0x1000 byte boundaries for version 2 files

    // validate the file header
    pl::at(225);
    check_invocant(it, &raf, "Read")?;
    if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(8))?, &n(8)) {
        return ret(n(0));
    }
    let Some(m) = re(r"^\0\0..RED(1|2)", "s").m(&val(&buff)) else { return ret(n(0)) };
    let ver = cap(&m, 1);
    let size = pl::unpack1("N", &val(&buff))?;
    if pl::lt(&size, &n(8)) {
        return ret(n(0));
    }

    pl::method(it, &et, "SetFileType", vec![])?;
    pl::call(it, "Image::ExifTool::SetByteOrder", vec![pl::s("MM")])?;
    let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::Red::Main")])?;
    let mut data_pos = n(0);

    // read the first block of the file
    let len = pl::sub(&size, &n(8));
    if !pl::num_eq(&raf_read_cell(it, &raf, &buf2, len.clone())?, &len) {
        return ret_warn(it, &et, err_trunc, want);
    }
    let joined = pl::concat(&val(&buff), &val(&buf2));
    *buff.borrow_mut() = joined;

    // extract tags from the header
    pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::concat(&pl::s("RED"), &ver), Value::Undef, pl::s("DataPt"), rf(&buff)])?;

    // read the second block from a version 1 file because
    // the first block doesn't contain a Red directory
    if pl::str_eq(&ver, "1") {
        // (read more than we need)
        if !raf_read_cell(it, &raf, &buff, n(0x10000))?.truthy() {
            return ret_warn(it, &et, err_trunc, want);
        }
        data_pos = pl::add(&data_pos, &size);
        pos = n(0x22); // directory starts at offset 0x22
    } else {
        // calculate position of Red directory start
        pos = n(0x44);
        if pl::lt(&pl::length(&val(&buff)), &pos) {
            return ret_warn(it, &et, err_trunc, want);
        }
        pos = pl::add(&pos, &pl::mul(&g8u(it, &rf(&buff), n(0x40))?, &n(0x18))); // skip "rdi" records
        pos = pl::add(&pos, &pl::mul(&g8u(it, &rf(&buff), n(0x41))?, &n(0x14))); // skip "rda" records
        pos = pl::add(&pos, &pl::mul(&g8u(it, &rf(&buff), n(0x42))?, &n(0x10))); // skip "rdx" records
    }
    if pl::gt(&pl::add(&pos, &n(8)), &pl::length(&val(&buff))) {
        dir_len = n(0); // find directory the hard way
    } else {
        dir_len = g16u(it, &rf(&buff), pos.clone())?; // get length of Red directory
        pos = pl::add(&pos, &n(2)); // skip length word
    }
    // do sanity check on the directory size (in case our assumptions were wrong)
    if pl::lt(&dir_len, &n(300)) || pl::ge(&dir_len, &n(2048)) || pl::gt(&pl::add(&pos, &dir_len), &pl::length(&val(&buff))) {
        // tag 0x1000 with length 0x000f should be near the directory start
        // ($buff =~ /\0\x0f\x10[\0\x06]/g: pos($buff) is undef here, so the search starts at 0)
        let b = val(&buff).to_bytes();
        let rx = crate::perl::regex::compile(br"\0\x0f\x10[\0\x06]", "").expect("pattern");
        let found = match rx.match_at(&b, 0) {
            Ok(Some(sp)) => sp[0].map(|(_, e)| e),
            _ => None,
        };
        let Some(end) = found else {
            return ret_warn(it, &et, pl::s("Can't find Red directory. Please submit sample for testing"), want);
        };
        pos = n(end as i64 - 4);
        dir_end = pl::length(&val(&buff));
        dir_len = Value::Undef;
        pl::method(it, &et, "Warn", vec![pl::s("This R3D file is different. Please submit a sample for testing")])?;
    } else {
        dir_end = pl::add(&pos, &dir_len);
    }
    if verbose.truthy() {
        et.set("INDENT", pl::concat(&et.get("INDENT"), &pl::s("| ")));
        pl::method(it, &et, "VerboseDir", vec![pl::s("Red"), Value::Undef, dir_len.clone()])?;
    }

    // process the first Red directory
    let red_format = crate::runtime::lex_hash(it, MODULE, "redFormat");
    while pl::le(&pl::add(&pos, &n(4)), &dir_end) {
        let len = g16u(it, &rf(&buff), pos.clone())?;
        if pl::lt(&len, &n(4)) || pl::gt(&pl::add(&pos, &len), &dir_end) {
            break;
        }
        let tag = g16u(it, &rf(&buff), pl::add(&pos, &n(2)))?;
        let fmt = red_format.get(ops::shift_right(&tag, &n(12)).to_bytes()); // format is top 4 bits of tag ID (ref PH)
        if !fmt.truthy() {
            if dir_len.truthy() {
                pl::method(it, &et, "Warn", vec![pl::s("Unknown format code")])?;
            }
            break;
        }
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![
                tag_table_ptr.clone(),
                tag,
                Value::Undef,
                pl::s("DataPt"),
                rf(&buff),
                pl::s("DataPos"),
                data_pos.clone(),
                pl::s("Start"),
                pl::add(&pos, &n(4)),
                pl::s("Size"),
                pl::sub(&len, &n(4)),
                pl::s("Format"),
                fmt,
            ],
        )?;
        pos = pl::add(&pos, &len);
    }
    if verbose.truthy() {
        et.set("INDENT", pl::substr(&et.get("INDENT"), 0, Some(-2)));
    }

    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Red::ProcessR3D", process_r3d);
}
