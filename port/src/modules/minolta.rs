//! Image::ExifTool::Minolta (Image/ExifTool/Minolta.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, n, ret};

const MODULE: &str = "Image::ExifTool::Minolta";

//------------------------------------------------------------------------------
/// PrintConv for Minolta white balance
pub fn convert_white_balance(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    let minolta_white_balance = crate::runtime::lex_hash(it, MODULE, "minoltaWhiteBalance");
    let mut print_conv = minolta_white_balance.get(val.to_bytes());
    if print_conv.is_undef() {
        if ops::bit_and(&val, &n(0xffff0000)).truthy() {
            // the A2 values can be shifted by +- 3 settings, where
            // each setting adds or subtracts 0x0010000 (ref 2)
            let type_ = pl::add(&ops::bit_and(&val, &n(0xff000000)), &n(0x800000));
            if minolta_white_balance.get(type_.to_bytes()).truthy() {
                print_conv = pl::concat(&minolta_white_balance.get(type_.to_bytes()), &pl::sprintf("%+.8g", &[pl::div(&pl::sub(&val, &type_), &n(0x10000))?]));
            } else {
                print_conv = pl::sprintf("Unknown (0x%x)", &[val]);
            }
        } else {
            // sprintf("Unknown ($val)"): $val is part of the format
            print_conv = pl::sprintf_v(&pl::concat(&pl::concat(&pl::s("Unknown ("), &val), &pl::s(")")), &[]);
        }
    }
    ret(print_conv)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Minolta::ConvertWhiteBalance", convert_white_balance);
}
