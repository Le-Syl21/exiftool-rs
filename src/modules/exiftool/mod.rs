//! Image::ExifTool (ExifTool.pm). The file is ported in three parts, by
//! line range, each registering its subs under Image::ExifTool.

pub mod part1;
pub mod part2;
pub mod part3;

use crate::interp::Interp;

pub fn register(it: &mut Interp) {
    part1::register(it);
    part2::register(it);
    part3::register(it);
}
