//! Image::ExifTool::Writer (Image/ExifTool/Writer.pl), ported sub by sub (see docs/port/PORTING.md).
//! Writer.pl is ported in two halves, writer_a.rs and writer_b.rs.

use crate::interp::Interp;

pub fn register(it: &mut Interp) {
    crate::modules::writer_a::register(it);
    crate::modules::writer_b::register(it);
}
