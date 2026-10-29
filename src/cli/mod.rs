//! The `exiftool` application (the script `exiftool` of ExifTool 13.60),
//! ported sub by sub as the library is (see docs/port/CLI.md).
//!
//! While the port is under way, `run_t exiftool ARGS` runs the script with
//! these natives in place of its subs (they win over the script's own
//! definitions), and the script's file lexicals are `main::<my>::NAME`
//! variables both sides share (`lex_scalar`, `lex_array`, `lex_hash`).

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::perl::Value;

pub mod get_image_info;
pub mod options;
pub mod program;
pub mod set_image_info;
pub mod subs_a;
pub mod subs_b;

/// A script sub ported to Rust.
pub type Native = fn(&mut Interp, Vec<Value>, Want) -> ER<Vec<Value>>;

/// The script's file lexical `$NAME`.
pub fn lex_scalar(it: &mut Interp, name: &str) -> Cell {
    crate::runtime::lex_scalar(it, "main", name)
}

/// The script's file lexical `@NAME`, as an array reference.
pub fn lex_array(it: &mut Interp, name: &str) -> Value {
    crate::runtime::lex_array(it, "main", name)
}

/// The script's file lexical `%NAME`, as a hash reference.
pub fn lex_hash(it: &mut Interp, name: &str) -> Value {
    crate::runtime::lex_hash(it, "main", name)
}

/// Define a ported sub (`main::Name`, or `Image::ExifTool::EndDir`...),
/// kept over the script's own definition.
pub fn define(it: &mut Interp, name: &str, f: Native) {
    it.define_native(name, f);
    it.sticky_natives.insert(std::rc::Rc::from(name));
}

/// Register the script's ported subs.
pub fn register(it: &mut Interp) {
    subs_a::register(it);
    subs_b::register(it);
    get_image_info::register(it);
    set_image_info::register(it);
    options::register(it);
    program::register(it);
}
