//! The Perl-subset interpreter: ExifTool's table strings, its eval sites
//! and `.ExifTool_config` files run here, parsed once and cached.

pub mod ast;
pub mod eval;
pub mod parse;
pub mod time;

pub use eval::{Binding, Code, Flow, Interp, Scope, Want};
