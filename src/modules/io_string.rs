//! IO::String, as ZIP.pm uses it: `IO::String->new($buff)`, an in-memory
//! filehandle on a ZIP archive read from a pipe or a scalar reference,
//! that Archive::Zip's readFromFileHandle then reads (the port's
//! Archive::Zip reads the handle's data through `Interp::handle_contents`).
//! The handle reads from a copy of the string, which ZIP.pm never changes
//! afterwards.

use std::cell::RefCell;
use std::io::Cursor;
use std::rc::Rc;

use crate::interp::eval::io::{FileHandle, Input};
use crate::interp::Interp;
use crate::perl::Value;
use crate::pl::arg;

pub fn register(it: &mut Interp) {
    it.set_scalar("IO::String::VERSION", Value::str("1.08"));
    // IO::String->new([$string]): a handle reading the string from its start
    it.define_native("IO::String::new", |_, a, _| {
        let data = arg(&a, 1);
        let data = if data.is_undef() { Vec::new() } else { data.to_bytes().to_vec() };
        let fh = FileHandle { input: Some(Input::Memory(Cursor::new(data))), class: Some(Rc::from("IO::String")), ..Default::default() };
        Ok(vec![Value::Handle(Rc::new(RefCell::new(fh)))])
    });
}
