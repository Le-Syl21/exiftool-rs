//! Image::ExifTool::AAC (Image/ExifTool/AAC.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::ops::{bit_and as and, bit_or as or, shift_left as shl, shift_right as shr};
use crate::perl::Value;
use crate::pl::{self, arg, n, re, ret};

/// `\$buff` of a `my` variable kept in a cell.
fn rf(c: &Cell) -> Value {
    Value::ScalarRef(c.clone())
}

/// Read information from an AAC file
/// Inputs: 0) ExifTool object reference, 1) Directory information reference
/// Returns: 1 on success, 0 if this wasn't a valid AAC file
pub fn process_aac(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);

    // format of frame header (7 bytes):
    // SSSS SSSS SSSS ILLC PPRR RRpC CCoh csff ffff ffff fffb bbbb bbbb bbNN
    // 1111 1111 1111 0001 0110 0000 0100 0000 0000 0101 0101 1111 1111 1100 (eg.)
    //  S = sync word                o = original/copy
    //  I = ID                       h = home
    //  L = layer (00)               c = copyright ID
    //  C = CRC absent               s = copyright start
    //  P = profile object type      f = frame length
    //  R = sampling rate index      b = buffer fullness
    //  p = private                  N = number of raw data blocks in frame
    //  C = channel configuration

    if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(7))?, &n(7)) {
        return ret(n(0));
    }
    if !re(r"^\xff[\xf0\xf1]", "").is_match(&buff.borrow()) {
        return ret(n(0));
    }
    let t = pl::unpack("NnC", &buff.borrow())?;
    let (t0, t1, t2) = (arg(&t, 0), arg(&t, 1), arg(&t, 2));
    if pl::num_eq(&and(&shr(&t0, &n(16)), &n(0x03)), &n(3)) {
        return ret(n(0)); // (reserved profile type)
    }
    if pl::gt(&and(&shr(&t0, &n(12)), &n(0x0f)), &n(12)) {
        return ret(n(0)); // validate sampling frequency index
    }
    let len = or(&and(&shl(&t0, &n(11)), &n(0x1800)), &and(&shr(&t1, &n(5)), &n(0x07ff)));
    if pl::lt(&len, &n(7)) {
        return ret(n(0));
    }

    pl::method(it, &et, "SetFileType", vec![])?;

    let tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::AAC::Main")])?;
    let di = Value::new_hash();
    di.set("DataPt", rf(&buff));
    pl::method(it, &et, "ProcessDirectory", vec![di, tag_table_ptr.clone()])?;

    // read the first frame data to check for a filler with the encoder name
    while pl::gt(&len, &n(8)) && {
        let l7 = pl::sub(&len, &n(7));
        pl::num_eq(&raf_read_cell(it, &raf, &buff, l7.clone())?, &l7)
    } {
        let b = buff.borrow().clone();
        let no_crc = and(&t0, &n(0x00010000));
        let blocks = and(&t2, &n(0x03));
        let mut pos = n(0);
        if !no_crc.truthy() {
            pos = pl::add(&pos, &pl::add(&n(2), &pl::mul(&n(2), &blocks)));
        }
        if pl::gt(&pl::add(&pos, &n(2)), &pl::length(&b)) {
            break;
        }
        let tmp = pl::unpack1(&format!("x{}n", pos.to_text()), &b)?;
        let id = shr(&tmp, &n(13));
        // read filler payload
        if pl::num_eq(&id, &n(6)) {
            let mut cnt = and(&shr(&tmp, &n(9)), &n(0x0f));
            pos = pl::add(&pos, &n(1));
            if pl::num_eq(&cnt, &n(15)) {
                cnt = pl::add(&cnt, &pl::sub(&and(&shr(&tmp, &n(1)), &n(0xff)), &n(1)));
                pos = pl::add(&pos, &n(1));
            }
            if pl::le(&pl::add(&pos, &cnt), &pl::length(&b)) {
                let mut dat = substr(it, &b, &pos, Some(&cnt))?;
                re(r"^\0+", "").subst(&mut dat, false, |_| Vec::new());
                re(r"\0+$", "").subst(&mut dat, false, |_| Vec::new());
                if re(r"^[\x20-\x7e]+$", "").is_match(&dat) {
                    pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("Encoder"), dat])?;
                }
            }
        }
        break;
    }

    ret(n(1))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::AAC::ProcessAAC", process_aac);
}
