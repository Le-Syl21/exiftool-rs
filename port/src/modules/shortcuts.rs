//! Image::ExifTool::Shortcuts (Image/ExifTool/Shortcuts.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! ExifTool shortcut tags: the table is data, this is its one sub.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::Value;
use crate::pl::{self, arg};

//------------------------------------------------------------------------------
// load user-defined shortcuts if available
// Inputs: reference to user-defined shortcut hash
pub fn load_shortcuts(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let shortcuts = arg(&a, 0);
    let main = pl::pkg_hash(it, "Image::ExifTool::Shortcuts::Main");
    for shortcut in shortcuts.keys() {
        let mut val = shortcuts.get(&shortcut);
        // also allow simple aliases
        if pl::ref_of(&val) != "ARRAY" {
            val = Value::new_array(vec![val]);
        }
        // save the user-defined shortcut or alias
        main.set(&shortcut, val);
    }
    // (the value of the foreach loop: an empty list)
    pl::ret_nothing(want)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Shortcuts::LoadShortcuts", load_shortcuts);
}
