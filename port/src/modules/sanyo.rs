//! Image::ExifTool::Sanyo (Image/ExifTool/Sanyo.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, n, ret};

//------------------------------------------------------------------------------
/// Patch incorrect offsets in J1, J2, J4, S1, S3 and S4 maker notes
/// Inputs: 0) valuePtr, 1) end of previous value, 2) value size, 3) tag ID, 4) write flag
pub fn fix_offsets(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val_end, mut size, tag_id, w_flag) = (arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4));
    // ignore existing offsets and calculate reasonable values instead
    if pl::num_eq(&tag_id, &n(0x100)) {
        // just ignore the SanyoThumbnail when writing (pointer is garbage)
        if w_flag.truthy() {
            it.set_arg(0, Value::Undef)?;
            return ret(Value::Undef);
        }
        ret(w_flag)
    } else {
        it.set_arg(0, val_end.clone())?; // set value pointer to next logical location
        if ops::bit_and(&size, &n(0x01)).truthy() {
            size = pl::add(&size, &n(1));
        }
        let new_end = pl::add(&val_end, &size);
        it.set_arg(1, new_end.clone())?; // update end-of-value pointer
        ret(new_end)
    }
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Sanyo::FixOffsets", fix_offsets);
}
