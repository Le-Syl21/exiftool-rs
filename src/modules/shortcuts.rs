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

/// Shortcuts.pm's module-level code: load the user-defined shortcuts
/// (`%Image::ExifTool::UserDefined::Shortcuts`, or the old name
/// `%Image::ExifTool::Shortcuts::UserDefined`). Called once the module's
/// dump is loaded.
fn init_shortcuts(it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    // (for backward compatibility, renamed in ExifTool 7.75)
    for name in ["Image::ExifTool::Shortcuts::UserDefined", "Image::ExifTool::UserDefined::Shortcuts"] {
        if let crate::interp::Binding::Hash(h) = it.global(b'%', name) {
            if !h.borrow().is_empty() {
                pl::call(it, "Image::ExifTool::Shortcuts::LoadShortcuts", vec![Value::HashRef(h)])?;
            }
        }
    }
    Ok(vec![])
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Shortcuts::InitShortcuts", init_shortcuts);
    it.define_native("Image::ExifTool::Shortcuts::LoadShortcuts", load_shortcuts);
}
