//! Image::ExifTool::WriteExif (Image/ExifTool/WriteExif.pl), ported sub by sub (see docs/port/PORTING.md).
//!
//! The first section holds the local helpers shared by the ports of the
//! Write*.pl files (WriteExif, WriteIPTC, WritePhotoshop, WriteCanonRaw,
//! WritePDF, WritePostScript): Perl operations the shared helpers do not
//! cover, built on the public Interp API only.

use std::rc::Rc;

use crate::interp::eval::{die, Cell, ER};
use crate::interp::{Interp, Scope, Want};
use crate::modules::exiftool::part2;
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, re, ret};

pub(crate) use crate::modules::random_access::{cell, substr};

// ---- local helpers (shared by the Write*.pl ports) ----

pub(crate) fn n(i: i64) -> Value {
    Value::Int(i)
}

/// The value of a `my` variable kept in a cell.
pub(crate) fn get(c: &Cell) -> Value {
    c.borrow().clone()
}

/// `$var = v` for a `my` variable kept in a cell.
pub(crate) fn put(c: &Cell, v: Value) {
    *c.borrow_mut() = v;
}

/// `\$var` of a `my` variable kept in a cell.
pub(crate) fn rf(c: &Cell) -> Value {
    Value::ScalarRef(c.clone())
}

/// `\$value` of a new scalar (the reference and its cell).
pub(crate) fn new_ref(v: Value) -> (Cell, Value) {
    let c = cell(v);
    let r = rf(&c);
    (c, r)
}

/// `Image::ExifTool::NAME(ARGS)` in scalar context.
pub(crate) fn etf(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `$et->Options(NAME)`
pub(crate) fn opt(it: &mut Interp, et: &Value, name: &str) -> ER<Value> {
    pl::method(it, et, "Options", vec![pl::s(name)])
}

/// `$raf->Seek(POS, WHENCE)`
pub(crate) fn seek(it: &mut Interp, raf: &Value, pos: Value, whence: i64) -> ER<bool> {
    Ok(pl::method(it, raf, "Seek", vec![pos, n(whence)])?.truthy())
}

/// `$raf->Read($buff, LEN)`: the number of bytes read.
pub(crate) fn read(it: &mut Interp, raf: &Value, buff: &Cell, len: Value) -> ER<Value> {
    crate::modules::random_access::raf_read_cell(it, raf, buff, len)
}

/// `$raf->Read($buff, LEN) == LEN`
pub(crate) fn read_eq(it: &mut Interp, raf: &Value, buff: &Cell, len: &Value) -> ER<bool> {
    let r = read(it, raf, buff, len.clone())?;
    Ok(pl::num_eq(&r, len))
}

/// `$raf->Tell()`
pub(crate) fn tell(it: &mut Interp, raf: &Value) -> ER<Value> {
    pl::method(it, raf, "Tell", vec![])
}

/// `Write($outfile, LIST)`: true on success.
pub(crate) fn write(it: &mut Interp, outfile: &Value, items: Vec<Value>) -> ER<bool> {
    let mut a = vec![outfile.clone()];
    a.extend(items);
    Ok(pl::call(it, "Image::ExifTool::Write", a)?.truthy())
}

fn sc(r: ER<Vec<Value>>) -> ER<Value> {
    Ok(r?.pop().unwrap_or_default())
}

/// `Get8u($dataPt, $pos)`
pub(crate) fn g8u(it: &mut Interp, pt: &Value, pos: Value) -> ER<Value> {
    sc(part2::get8u(it, vec![pt.clone(), pos], Want::Scalar))
}

/// `Get16u($dataPt, $pos)`
pub(crate) fn g16u(it: &mut Interp, pt: &Value, pos: Value) -> ER<Value> {
    sc(part2::get16u(it, vec![pt.clone(), pos], Want::Scalar))
}

/// `Get32u($dataPt, $pos)`
pub(crate) fn g32u(it: &mut Interp, pt: &Value, pos: Value) -> ER<Value> {
    sc(part2::get32u(it, vec![pt.clone(), pos], Want::Scalar))
}

/// `Set16u($val)`: the packed value.
pub(crate) fn s16u(it: &mut Interp, v: Value) -> ER<Value> {
    sc(part2::set16u(it, vec![v], Want::Scalar))
}

/// `Set32u($val)`: the packed value.
pub(crate) fn s32u(it: &mut Interp, v: Value) -> ER<Value> {
    sc(part2::set32u(it, vec![v], Want::Scalar))
}

/// `Set16u($val, $dataPt, $pos)`
pub(crate) fn s16u_at(it: &mut Interp, v: Value, pt: &Value, pos: Value) -> ER<()> {
    part2::set16u(it, vec![v, pt.clone(), pos], Want::Scalar)?;
    Ok(())
}

/// `Set32u($val, $dataPt, $pos)`
pub(crate) fn s32u_at(it: &mut Interp, v: Value, pt: &Value, pos: Value) -> ER<()> {
    part2::set32u(it, vec![v, pt.clone(), pos], Want::Scalar)?;
    Ok(())
}

/// `GetByteOrder()`
pub(crate) fn byte_order(it: &mut Interp) -> ER<Value> {
    sc(part2::get_byte_order(it, vec![], Want::Scalar))
}

/// `SetByteOrder($order)`
pub(crate) fn set_byte_order(it: &mut Interp, order: Value) -> ER<Value> {
    sc(part2::set_byte_order(it, vec![order], Want::Scalar))
}

/// `ReadValue(ARGS)` in scalar context.
pub(crate) fn read_value(it: &mut Interp, args: Vec<Value>) -> ER<Value> {
    etf(it, "ReadValue", args)
}

/// `WriteValue(ARGS)` in scalar context.
pub(crate) fn write_value(it: &mut Interp, args: Vec<Value>) -> ER<Value> {
    etf(it, "WriteValue", args)
}

/// `++$$h{key}`
pub(crate) fn inc(h: &Value, key: &str) {
    h.set(key, pl::add(&h.get(key), &n(1)));
}

/// `$a eq $b`
pub(crate) fn seq(a: &Value, b: &Value) -> bool {
    a.to_bytes() == b.to_bytes()
}

/// `length $x` as a number (undef is 0).
pub(crate) fn len(v: &Value) -> Value {
    n(v.to_bytes().len() as i64)
}

/// `$a . $b . ...`
pub(crate) fn cat(items: &[Value]) -> Value {
    let mut v = Vec::new();
    for i in items {
        v.extend_from_slice(&i.to_bytes());
    }
    Value::bytes(v)
}

/// `$s x $n`
pub(crate) fn rep(s: &[u8], count: &Value) -> Value {
    let c = count.to_i64().max(0) as usize;
    Value::bytes(s.repeat(c))
}

/// `substr($s, OFF, LEN) = REPL` (or 4-argument substr): the new string.
pub(crate) fn substr_repl(s: &Value, off: &Value, len: Option<&Value>, repl: &Value) -> ER<Value> {
    let b = s.to_bytes();
    let Some((x, y)) = crate::interp::eval::substr_range(b.len(), off.to_i64(), len.map(|l| l.to_i64())) else {
        return die("substr outside of string");
    };
    let mut out = b[..x].to_vec();
    out.extend_from_slice(&repl.to_bytes());
    out.extend_from_slice(&b[y..]);
    Ok(Value::bytes(out))
}

/// `chr($n)` as the interpreter's builtin.
pub(crate) fn chr(v: &Value) -> Value {
    let f = v.to_f64();
    let c = if f < 0.0 || f.is_nan() { 0xFFFD } else { f as u32 };
    if c < 256 {
        Value::bytes([c as u8])
    } else {
        Value::bytes(char::from_u32(c).unwrap_or('\u{FFFD}').to_string().into_bytes())
    }
}

/// `ord($s)`
pub(crate) fn ord(v: &Value) -> Value {
    n(v.to_bytes().first().copied().unwrap_or(0) as i64)
}

/// `sort { $a <=> $b } LIST` (stable, as perl's merge sort).
pub(crate) fn sort_num(v: &mut [Value]) {
    v.sort_by(|a, b| ops::num_cmp(a, b).unwrap_or(std::cmp::Ordering::Equal));
}

/// `sort LIST` (string order).
pub(crate) fn sort_str(v: &mut [Value]) {
    v.sort_by(ops::str_cmp);
}

/// `keys %$h` as values.
pub(crate) fn keys(h: &Value) -> Vec<Value> {
    h.keys().into_iter().map(Value::from).collect()
}

/// `$a & $b`
pub(crate) fn band(a: &Value, b: i64) -> Value {
    ops::bit_and(a, &n(b))
}

/// `print $out STRING`
pub(crate) fn print_out(it: &mut Interp, out: &Value, s: Value) -> ER<()> {
    pl::print_to(it, Some(out), &[s])?;
    Ok(())
}

/// A scope with the file lexicals of a Write*.pl file (`my` variables at
/// its top level), as an eval site in that file sees them.
pub(crate) fn wscope(it: &Interp, module: &str, file: &str) -> Rc<Scope> {
    let scope = Scope::new(None);
    let prefix = format!("{module}::<my:{file}>::");
    for (sigil, name, b) in it.globals_with_prefix(&prefix) {
        scope.declare(sigil, &name[prefix.len()..], b);
    }
    scope
}

/// A file lexical of a Write*.pl file (`my $x` at its top level).
pub(crate) fn wlex_scalar(it: &mut Interp, module: &str, file: &str, name: &str) -> Cell {
    match it.global(b'$', &format!("{module}::<my:{file}>::{name}")) {
        crate::interp::Binding::Scalar(c) => c,
        _ => unreachable!(),
    }
}

/// `%x` file lexical of a Write*.pl file.
pub(crate) fn wlex_hash(it: &mut Interp, module: &str, file: &str, name: &str) -> Value {
    match it.global(b'%', &format!("{module}::<my:{file}>::{name}")) {
        crate::interp::Binding::Hash(h) => Value::HashRef(h),
        _ => unreachable!(),
    }
}

/// `eval $code` at an eval site of a Write*.pl file, with the variables the
/// site has in scope (see `pl::eval_site_in`).
pub(crate) fn eval_at(it: &mut Interp, code: &Value, module: &str, file: &str, vars: &[(&str, Value)], want: Want) -> (Value, Vec<Value>) {
    let scope = wscope(it, module, file);
    pl::eval_site_in(it, code, module, Some(scope), vars, want)
}

/// `$$h{k}` as an array, autovivified (`@{$$h{k}}` in an lvalue context
/// such as grep or foreach).
pub(crate) fn viv_array(h: &Value, key: &str) -> Value {
    h.get_or_new_array(key)
}

/// `grep /RE/, LIST` in scalar context: the count.
pub(crate) fn grep_count(r: &pl::Re, list: &[Value]) -> usize {
    list.iter().filter(|v| r.is_match(v)).count()
}

const MODULE: &str = "Image::ExifTool::Exif";

pub fn register(_it: &mut Interp) {}
