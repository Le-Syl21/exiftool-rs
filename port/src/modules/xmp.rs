//! Image::ExifTool::XMP (Image/ExifTool/XMP.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! The XMP parser works on the data buffer with `m//g` and `pos()`: here
//! each such loop keeps the position in a local variable and matches at
//! that offset (`\G` anchors there, as with perl's `pos`).

use std::cell::RefCell;
use std::rc::Rc;

use crate::interp::eval::{die, Binding, Lv, ER};
use crate::interp::{Interp, Want};
use crate::perl::regex::{self as pre, PerlRegex, Spans};
use crate::perl::Value;
use crate::pl::{self, arg, cap, ret, Caps};

/// `$raf->METHOD(...)` on an undefined `$raf` dies as perl does, at the
/// statement's location (the Read helpers run the call as an eval string,
/// which has a location of its own).
fn check_invocant(raf: &Value, method: &str) -> ER<()> {
    if raf.is_undef() {
        return crate::interp::eval::die(format!("Can't call method \"{method}\" on an undefined value"));
    }
    Ok(())
}

// ---- local helpers (Perl idioms the shared helpers don't cover yet) ----

/// A piece of a string concatenation (`"...$x..."`).
pub(crate) trait Piece {
    fn put(&self, out: &mut Vec<u8>);
}

impl Piece for str {
    fn put(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self.as_bytes());
    }
}

impl Piece for [u8] {
    fn put(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self);
    }
}

impl Piece for Vec<u8> {
    fn put(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self);
    }
}

impl Piece for String {
    fn put(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self.as_bytes());
    }
}

impl Piece for Value {
    fn put(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.to_bytes());
    }
}

impl<T: Piece + ?Sized> Piece for &T {
    fn put(&self, out: &mut Vec<u8>) {
        (**self).put(out);
    }
}

/// `"$a$b..."`: the concatenation of the pieces, as a value.
macro_rules! cat {
    ($($e:expr),* $(,)?) => {{
        let mut o: Vec<u8> = Vec::new();
        $( $crate::modules::xmp::Piece::put(&$e, &mut o); )*
        $crate::perl::Value::bytes(o)
    }};
}
#[allow(unused_imports)]
pub(crate) use cat;

/// `foreach (@$r)` at perl line `line`: an undefined `$r` is autovivified
/// (an empty list here), anything but an ARRAY reference dies.
fn foreach_items(r: &Value, line: u32) -> ER<Vec<Value>> {
    if r.is_undef() {
        return Ok(Vec::new());
    }
    pl::at(line);
    pl::aderef(r)
}

/// A constant pattern (cached).
pub(crate) fn rx(pat: &str, flags: &str) -> Rc<PerlRegex> {
    pre::compile(pat.as_bytes(), flags).unwrap_or_else(|e| panic!("bad pattern /{pat}/: {e}"))
}

/// An interpolated pattern: a bad one dies as in perl.
pub(crate) fn rx_dyn(pat: &[u8], flags: &str) -> ER<Rc<PerlRegex>> {
    match pre::compile(pat, flags) {
        Ok(r) => Ok(r),
        Err(e) => die(e),
    }
}

/// Match from byte offset `pos` (`pos($s) = $pos; $s =~ /.../g`).
pub(crate) fn m_at(re: &PerlRegex, subject: &[u8], pos: usize) -> Option<Spans> {
    re.match_at(subject, pos).ok().flatten()
}

pub(crate) fn caps_of(subject: &[u8], spans: &Spans) -> Caps {
    spans.iter().map(|g| g.map(|(a, b)| Value::bytes(&subject[a..b]))).collect()
}

/// `$s =~ /re/`: the captures of the first match.
pub(crate) fn m(re: &PerlRegex, subject: &Value) -> Option<Caps> {
    let b = subject.to_bytes();
    m_at(re, &b, 0).map(|sp| caps_of(&b, &sp))
}

/// The end of a match (perl's `pos` after a `/g` match).
pub(crate) fn end_of(sp: &Spans) -> usize {
    sp[0].unwrap().1
}

/// `$s =~ s/re/EXPR/e` (or `/g`), with a replacement that may call Perl
/// code (and so fail). Returns the number of substitutions; `$s` is
/// updated only when there was one.
pub(crate) fn subst(re: &PerlRegex, target: &mut Value, global: bool, mut repl: impl FnMut(&Caps) -> ER<Vec<u8>>) -> ER<usize> {
    let b = target.to_bytes();
    let mut out = Vec::with_capacity(b.len());
    let (mut last, mut start, mut n) = (0, 0, 0);
    while start <= b.len() {
        let Some(sp) = m_at(re, &b, start) else { break };
        let (s, e) = sp[0].unwrap();
        out.extend_from_slice(&b[last..s]);
        out.extend_from_slice(&repl(&caps_of(&b, &sp))?);
        n += 1;
        last = e;
        if !global {
            break;
        }
        if e == s {
            if e >= b.len() {
                break;
            }
            out.push(b[e]);
            last = e + 1;
            start = e + 1;
        } else {
            start = e;
        }
    }
    if n > 0 {
        out.extend_from_slice(&b[last.min(b.len())..]);
        *target = Value::bytes(out);
    }
    Ok(n)
}

/// A package hash (`%Image::ExifTool::XMP::stdXlatNS`) as a reference.
pub(crate) fn pkg_hash(it: &mut Interp, name: &str) -> Value {
    match it.global(b'%', name) {
        Binding::Hash(h) => Value::HashRef(h),
        _ => unreachable!(),
    }
}

/// A file lexical of XMP.pm (`my %uri2ns`), as a reference.
pub(crate) fn lex(it: &mut Interp, name: &str) -> Value {
    crate::runtime::lex_hash(it, "Image::ExifTool::XMP", name)
}

/// `$$ref = $val` for a SCALAR reference.
pub(crate) fn set_deref(r: &Value, v: Value) -> ER<()> {
    match r {
        Value::ScalarRef(c) => *c.borrow_mut() = v,
        Value::LvRef(l) => l.set(v)?,
        _ => {}
    }
    Ok(())
}

/// `\$x`: a new scalar reference.
pub(crate) fn sref(v: Value) -> Value {
    Value::ScalarRef(Rc::new(RefCell::new(v)))
}

/// `\$$h{key}`: a reference to a hash element.
pub(crate) fn elem_ref(h: &Value, key: &[u8]) -> Value {
    match h {
        Value::HashRef(rc) => Value::LvRef(Rc::new(Lv::HElem(rc.clone(), Rc::from(key)))),
        _ => sref(Value::Undef),
    }
}

/// `$$h{a}{b}` in rvalue context: perl autovivifies `$$h{a}`.
pub(crate) fn hh(h: &Value, key: impl AsRef<[u8]>) -> Value {
    if h.is_hash() {
        h.get_or_new_hash(key)
    } else {
        Value::Undef
    }
}

/// `&$code(ARGS)` on a code reference.
pub(crate) fn call_code(it: &mut Interp, code: &Value, args: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    match code {
        Value::Code(c) => it.call(c, args, want),
        _ => {
            let name = code.to_text();
            it.call_named(&name, args, want)
        }
    }
}

pub(crate) fn call_code_scalar(it: &mut Interp, code: &Value, args: Vec<Value>) -> ER<Value> {
    Ok(call_code(it, code, args, Want::Scalar)?.pop().unwrap_or_default())
}

/// `\&Pkg::name`
pub(crate) fn code_ref(it: &mut Interp, name: &str) -> Value {
    Value::Code(it.sub_ref(name))
}

/// `require EXPR`
pub(crate) fn require(it: &mut Interp, what: &str) -> ER<()> {
    match it.require_hook.clone() {
        Some(h) => h(it, what),
        None => Ok(()),
    }
}

/// `local $_`: run `f` with `$_` undefined, restoring it on every exit.
pub(crate) fn local_topic<T>(it: &mut Interp, f: impl FnOnce(&mut Interp) -> ER<T>) -> ER<T> {
    let saved = it.get_scalar("main::_");
    it.set_scalar("main::_", Value::Undef);
    let r = f(it);
    it.set_scalar("main::_", saved);
    r
}

/// `lc($x)` (ASCII, as perl on byte strings).
pub(crate) fn lc(v: &Value) -> Value {
    Value::bytes(v.to_bytes().to_ascii_lowercase())
}

pub(crate) fn uc(v: &Value) -> Value {
    Value::bytes(v.to_bytes().to_ascii_uppercase())
}

pub(crate) fn ucfirst(v: &Value) -> Value {
    let mut o = v.to_bytes().to_vec();
    if let Some(f) = o.first_mut() {
        *f = f.to_ascii_uppercase();
    }
    Value::bytes(o)
}

/// `chr($n)`, as the interpreter's.
pub(crate) fn chr(v: &Value) -> Value {
    let n = v.to_f64();
    let n = if n < 0.0 || n.is_nan() { 0xFFFD } else { n as u32 };
    let out = if n < 256 { vec![n as u8] } else { char::from_u32(n).unwrap_or('\u{FFFD}').to_string().into_bytes() };
    Value::bytes(out)
}

/// `hex($s)` of a string of hex digits.
pub(crate) fn hex(v: &Value) -> Value {
    let mut u: u64 = 0;
    let mut f: f64 = 0.0;
    let mut overflow = false;
    for &c in v.to_bytes().iter() {
        let d = match (c as char).to_digit(16) {
            Some(d) => d as u64,
            None => break,
        };
        if !overflow {
            match u.checked_mul(16).and_then(|x| x.checked_add(d)) {
                Some(x) => u = x,
                None => {
                    overflow = true;
                    f = u as f64 * 16.0 + d as f64;
                }
            }
        } else {
            f = f * 16.0 + d as f64;
        }
    }
    if overflow {
        Value::Num(f)
    } else {
        crate::perl::ops::from_uv(u)
    }
}

/// `ref $x eq 'HASH'`
pub(crate) fn is_hash(v: &Value) -> bool {
    v.ref_type() == "HASH"
}

pub(crate) fn same_str(a: &Value, b: &Value) -> bool {
    a.to_bytes() == b.to_bytes()
}

/// `pack(TEMPLATE, LIST)`: a bad template dies.
pub(crate) fn pack(t: &str, args: &[Value]) -> ER<Value> {
    match crate::perl::pack::pack(t.as_bytes(), args) {
        Ok(b) => Ok(Value::bytes(b)),
        Err(e) => die(e),
    }
}

pub(crate) fn unpack(t: &[u8], data: &[u8]) -> ER<Vec<Value>> {
    match crate::perl::pack::unpack(t, data) {
        Ok(v) => Ok(v),
        Err(e) => die(e),
    }
}

/// `$s =~ tr/SEARCH/REPLACE/flags` (the string is updated, the count returned).
pub(crate) fn tr(s: &mut Value, search: &[u8], replace: &[u8], flags: &str) -> usize {
    let (out, n) = crate::perl::tr::tr(&s.to_bytes(), search, replace, flags);
    if !s.is_undef() || n > 0 {
        *s = Value::bytes(out);
    }
    n
}

pub(crate) fn n(i: i64) -> Value {
    Value::Int(i)
}

/// `substr($s, OFF, LEN)` (undef outside of the string).
pub(crate) fn substr(s: &[u8], off: i64, len: Option<i64>) -> Value {
    match crate::interp::eval::substr_range(s.len(), off, len) {
        Some((a, b)) => Value::bytes(&s[a..b]),
        None => Value::Undef,
    }
}

/// `pop @$a`
pub(crate) fn apop(a: &Value) -> Value {
    match a {
        Value::ArrayRef(r) => r.borrow_mut().pop().unwrap_or_default(),
        _ => Value::Undef,
    }
}

/// `%$h = ()`
pub(crate) fn hclear(h: &Value) {
    if let Value::HashRef(r) = h {
        r.borrow_mut().clear();
    }
}

/// `join($sep, @$a)`
pub(crate) fn join(sep: &[u8], items: &[Value]) -> Value {
    let v: Vec<Vec<u8>> = items.iter().map(|x| x.to_bytes().to_vec()).collect();
    Value::bytes(v.join(sep))
}

/// A new hash from key/value pairs: `{ K => V, ... }`.
pub(crate) fn hash_of(pairs: Vec<(&str, Value)>) -> Value {
    let h = Value::new_hash();
    for (k, v) in pairs {
        h.set(k, v);
    }
    h
}

/// `$x || $y`
pub(crate) fn or(a: Value, b: Value) -> Value {
    if a.truthy() {
        a
    } else {
        b
    }
}

/// `$obj->METHOD(ARGS)` where the method writes some of its arguments
/// (`$raf->Read($buff, $len)`): each argument is a variable of the call,
/// and the variables are copied back. (The shared helpers have no way to
/// pass aliases to a call yet, so the call goes through a small eval.)
pub(crate) fn method_aliased(it: &mut Interp, obj: &Value, name: &str, args: &mut [Value]) -> ER<Value> {
    let scope = crate::interp::Scope::new(None);
    scope.scalar("o", obj.clone());
    let mut cells = Vec::new();
    let mut code = format!("$o->{name}(");
    for (i, a) in args.iter().enumerate() {
        cells.push(scope.scalar(&format!("a{i}"), a.clone()));
        if i > 0 {
            code.push(',');
        }
        code.push_str(&format!("$a{i}"));
    }
    code.push(')');
    let saved_err = it.get_scalar("main::@");
    let r = it.eval_string(code.as_bytes(), XMP, &scope, Want::Scalar);
    it.set_scalar("main::@", saved_err);
    for (a, c) in args.iter_mut().zip(cells.iter()) {
        *a = c.borrow().clone();
    }
    match r {
        Ok(mut v) => Ok(v.pop().unwrap_or_default()),
        Err(e) => Err(crate::interp::Flow::Die(e)),
    }
}

const XMP: &str = "Image::ExifTool::XMP";

// ---- the subs of XMP.pm ----

/// AutoLoad our writer routines when necessary
pub fn autoload(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let name = it.get_scalar("Image::ExifTool::XMP::AUTOLOAD");
    let mut args = vec![name];
    args.extend(a);
    pl::at(2853);
    it.call_named("Image::ExifTool::DoAutoLoad", args, want)
}

/// Escape necessary XML characters in UTF-8 string
/// Inputs: 0) string to be escaped
/// Returns: escaped string
pub fn escape_xml(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(escape_xml_v(it, arg(&a, 0))?)
}

pub(crate) fn escape_xml_v(it: &mut Interp, mut str: Value) -> ER<Value> {
    let char_name = lex(it, "charName");
    // escape necessary XML characters
    subst(&rx(r#"([&><'"])"#, "s"), &mut str, true, |c| Ok(cat!("&", char_name.get(cap(c, 1).to_bytes()), ";").to_bytes().to_vec()))?;
    Ok(str)
}

/// Unescape XML character references (entities and numerical)
/// Inputs: 0) string to be unescaped
///         1) optional hash reference to convert entity names to numbers
///         2) optional character encoding
/// Returns: unescaped string
pub fn unescape_xml(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(unescape_xml_v(it, arg(&a, 0), arg(&a, 1), arg(&a, 2))?)
}

pub(crate) fn unescape_xml_v(it: &mut Interp, mut str: Value, mut conv: Value, enc: Value) -> ER<Value> {
    if !conv.truthy() {
        conv = lex(it, "charNum");
    }
    subst(&rx(r"&(#?\w+);", "s"), &mut str, true, |c| Ok(unescape_char_v(it, &cap(c, 1), &conv, &enc)?.to_bytes().to_vec()))?;
    Ok(str)
}

/// Escape string for XML, ensuring valid XML and UTF-8
/// Inputs: 0) string
/// Returns: escaped string
pub fn full_escape_xml(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut str = arg(&a, 0);
    let char_name = lex(it, "charName");
    subst(&rx(r#"([&><'"])"#, "s"), &mut str, true, |c| Ok(cat!("&", char_name.get(cap(c, 1).to_bytes()), ";").to_bytes().to_vec()))?; // escape necessary XML characters
    subst(&rx(r"\\", "s"), &mut str, true, |_| Ok(b"&#92;".to_vec()))?; // escape backslashes too
    // then use C-escape sequences for invalid characters
    let bad = m(&rx(r"[\0-\x1f]", ""), &str).is_some() || {
        let r = sref(str.clone());
        let v = pl::call(it, "Image::ExifTool::IsUTF8", vec![r])?;
        pl::lt(&v, &n(0))
    };
    if bad {
        subst(&rx(r"([\0-\x1f\x7f-\xff])", "s"), &mut str, true, |c| {
            let ord = cap(c, 1).to_bytes().first().copied().unwrap_or(0);
            Ok(pl::sprintf("\\x%.2x", &[n(ord as i64)]).to_bytes().to_vec())
        })?;
    }
    ret(str)
}

/// Unescape XML/C escaped string
/// Inputs: 0) string
/// Returns: unescaped string
pub fn full_unescape_xml(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut str = arg(&a, 0);
    // unescape C escape sequences first
    subst(&rx(r"\\x([\da-f]{2})", "s"), &mut str, true, |c| Ok(chr(&hex(&cap(c, 1))).to_bytes().to_vec()))?;
    let conv = lex(it, "charNum");
    subst(&rx(r"&(#?\w+);", "s"), &mut str, true, |c| Ok(unescape_char_v(it, &cap(c, 1), &conv, &Value::Undef)?.to_bytes().to_vec()))?;
    ret(str)
}

/// Convert XML character reference to UTF-8
/// Inputs: 0) XML character reference stripped of the '&' and ';' (eg. 'quot', '#34', '#x22')
///         1) hash reference for looking up character numbers by name
///         2) optional character encoding (default 'UTF8')
/// Returns: UTF-8 equivalent (or original character on conversion error)
pub fn unescape_char(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(unescape_char_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?)
}

pub(crate) fn unescape_char_v(it: &mut Interp, ch: &Value, conv: &Value, enc: &Value) -> ER<Value> {
    let mut val = conv.get(ch.to_bytes());
    if val.is_undef() {
        if let Some(c) = m(&rx(r"^#x([0-9a-fA-F]+)$", ""), ch) {
            val = hex(&cap(&c, 1));
        } else if let Some(c) = m(&rx(r"^#(\d+)$", ""), ch) {
            val = cap(&c, 1);
        } else {
            return Ok(cat!("&", ch, ";")); // should issue a warning here? [no]
        }
    }
    if pl::lt(&val, &n(0x80)) {
        return Ok(chr(&val)); // simple ASCII
    }
    val = pack("C0U", &[val])?;
    if enc.truthy() && !pl::str_eq(enc, "UTF8") {
        val = pl::call(it, "Image::ExifTool::Decode", vec![Value::Undef, val, pl::s("UTF8"), Value::Undef, enc.clone()])?;
    }
    Ok(val)
}

/// Fix malformed UTF8 (by replacing bad bytes with specified character)
/// Inputs: 0) string reference, 1) string to replace each bad byte,
///         may be '' to delete bad bytes, or undef to use '?'
/// Returns: true if string was fixed, and updates string
pub fn fix_utf8(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (str_pt, mut bad) = (arg(&a, 0), arg(&a, 1));
    let mut fixed = Value::Undef;
    // (pos($$strPt) = 0 autovivifies an undefined $strPt, but dies on
    // anything else that is not a SCALAR reference)
    if !str_pt.is_undef() {
        pl::at(2947);
        pl::sderef(&str_pt)?;
    }
    let mut s: Vec<u8> = str_pt.deref().to_bytes().to_vec();
    let mut p = 0usize; // start at beginning of string
    let hi = rx(r"([\x80-\xff])", "");
    loop {
        let Some(sp) = m_at(&hi, &s, p) else { break };
        let ch = s[sp[1].unwrap().0];
        let pos = end_of(&sp);
        p = pos;
        // (see comments in Image::ExifTool::IsUTF8())
        if (0xc2..0xf8).contains(&ch) {
            let nb = if ch < 0xe0 { 1 } else if ch < 0xf0 { 2 } else { 3 };
            let cont = rx(&format!(r"\G([\x80-\xbf]{{{nb}}})"), "");
            if let Some(sp2) = m_at(&cont, &s, p) {
                p = end_of(&sp2);
                let (c0, c1) = sp2[1].unwrap();
                let one = &s[c0..c1];
                if nb == 1 {
                    continue;
                }
                if nb == 2 {
                    if !((ch == 0xe0 && (one[0] & 0xe0) == 0x80) || (ch == 0xed && (one[0] & 0xe0) == 0xa0) || (ch == 0xef && one[0] == 0xbf && (one[1] & 0xfe) == 0xbe)) {
                        continue;
                    }
                } else if !((ch == 0xf0 && (one[0] & 0xf0) == 0x80) || (ch == 0xf4 && one[0] > 0x8f) || ch > 0xf4) {
                    continue;
                }
            }
        }
        // replace bad character
        if bad.is_undef() {
            bad = pl::s("?");
        }
        let bb = bad.to_bytes();
        s.splice(pos - 1..pos, bb.iter().copied());
        p = pos - 1 + bb.len();
        fixed = n(1);
    }
    if fixed.truthy() {
        set_deref(&str_pt, Value::bytes(s))?;
    }
    ret(fixed)
}

/// Utility routine to decode a base64 string
/// Inputs: 0) base64 string
/// Returns: reference to decoded data
pub fn decode_base64(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(decode_base64_v(arg(&a, 0))?)
}

pub(crate) fn decode_base64_v(mut str: Value) -> ER<Value> {
    // truncate at first unrecognized character (base 64 data
    // may only contain A-Z, a-z, 0-9, +, /, =, or white space)
    subst(&rx(r"[^A-Za-z0-9+\/= \t\n\r\f].*", "s"), &mut str, false, |_| Ok(Vec::new()))?;
    // translate to uucoded and remove padding and white space
    tr(&mut str, br"A-Za-z0-9+\/= \t\n\r\f", b" -_", "d");

    // convert the data to binary in chunks
    let chunk_size: i64 = 60;
    let mut uu_len = pack("c", &[n(32 + chunk_size * 3 / 4)])?; // calculate length byte
    let mut dat: Vec<u8> = Vec::new();
    let sb = str.to_bytes();
    let substr = |i: i64, l: i64| -> Vec<u8> {
        let a = (i.max(0) as usize).min(sb.len());
        let b = ((i + l).max(0) as usize).min(sb.len());
        sb[a..b.max(a)].to_vec()
    };
    // loop through the whole chunks
    let mut len = sb.len() as i64 - chunk_size;
    let mut i: i64 = 0;
    while i <= len {
        let sub = substr(i, chunk_size); // get a chunk of the data
        let mut d = uu_len.to_bytes().to_vec();
        d.extend_from_slice(&sub);
        dat.extend_from_slice(&unpack(b"u", &d)?.into_iter().next().unwrap_or_default().to_bytes()); // decode it
        i += chunk_size;
    }
    len += chunk_size;
    // handle last partial chunk if necessary
    if i < len {
        let l = pl::add(&n(32), &pl::div(&n((len - i) * 3), &n(4))?);
        uu_len = pack("c", &[l])?; // recalculate length
        let sub = substr(i, len - i); // get the last partial chunk
        let mut d = uu_len.to_bytes().to_vec();
        d.extend_from_slice(&sub);
        dat.extend_from_slice(&unpack(b"u", &d)?.into_iter().next().unwrap_or_default().to_bytes()); // decode it
    }
    Ok(sref(Value::bytes(dat)))
}

/// Generate a tag ID for this XMP tag
/// Inputs: 0) tag property name list ref, 1) array ref for receiving structure property list
///         2) array for receiving namespace list
/// Returns: tagID and outtermost interesting namespace (or '' if no namespace)
pub fn get_xmp_tag_id(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (tag, namespace) = get_xmp_tag_id_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?;
    if want == Want::List {
        Ok(vec![tag, if namespace.truthy() { namespace } else { pl::s("") }])
    } else {
        ret(tag)
    }
}

/// GetXMPTagID: the tag and the raw namespace.
pub(crate) fn get_xmp_tag_id_v(it: &mut Interp, props: &Value, struct_props: &Value, ns_list: &Value) -> ER<(Value, Value)> {
    let (mut tag, mut namespace) = (Value::Undef, Value::Undef);
    let ignore_namespace = lex(it, "ignoreNamespace");
    let ignore_prop = lex(it, "ignoreProp");
    let ignore_et_prop = lex(it, "ignoreEtProp");
    let std_xlat_ns = pkg_hash(it, "Image::ExifTool::XMP::stdXlatNS");
    let main = pkg_hash(it, "Image::ExifTool::XMP::Main");
    for prop in foreach_items(props, 3022)? {
        // split name into namespace and property name
        // (Note: namespace can be '' for property qualifiers)
        let (ns, mut nm) = match m(&rx(r"(.*?):(.*)", ""), &prop) {
            Some(c) => (cap(&c, 1), cap(&c, 2)),
            None => (pl::s(""), prop.clone()),
        };
        let pb = prop.to_bytes();
        if ignore_namespace.get(ns.to_bytes()).truthy() || ignore_prop.get(&pb).truthy() || ignore_et_prop.get(&pb).truthy() {
            // special case: don't ignore rdf numbered items
            // (not technically allowed in XMP, but used in RDF/XML)
            match m(&rx(r"^rdf:(_\d+)$", ""), &prop) {
                None => {
                    // save list index if necessary for structures
                    if struct_props.truthy() && !struct_props.is_empty() {
                        if let Some(c) = m(&rx(r"^rdf:li (\d+)$", ""), &prop) {
                            struct_props.at(-1).push(cap(&c, 1));
                        }
                    }
                    continue;
                }
                Some(c) => {
                    if !tag.is_undef() {
                        tag = pl::concat(&tag, &cap(&c, 1));
                    }
                }
            }
        } else {
            subst(&rx(r" .*", ""), &mut nm, false, |_| Ok(Vec::new()))?; // remove nodeID if it exists
            // all uppercase is ugly, so convert it
            if m(&rx(r"[a-z]", ""), &nm).is_none() {
                let x = std_xlat_ns.get(ns.to_bytes());
                let xlat = if x.truthy() { x } else { ns.clone() };
                let info = main.get(xlat.to_bytes());
                let mut table = Value::Undef;
                if is_hash(&info) && info.get("SubDirectory").truthy() {
                    let tt = info.get("SubDirectory").get("TagTable");
                    table = pl::call(it, "Image::ExifTool::GetTagTable", vec![tt])?;
                }
                if !(table.truthy() && table.get(nm.to_bytes()).truthy()) {
                    nm = lc(&nm);
                    subst(&rx(r"_([a-z])", ""), &mut nm, true, |c| Ok(ucfirst(&cap(c, 1)).to_bytes().to_vec()))?;
                }
            }
            if !tag.is_undef() {
                tag = pl::concat(&tag, &ucfirst(&nm)); // add to tag name
            } else {
                tag = nm.clone();
            }
            // save structure information if necessary
            if struct_props.truthy() {
                struct_props.push(Value::new_array(vec![nm.clone()]));
                if ns_list.truthy() {
                    ns_list.push(ns.clone());
                }
            }
        }
        // save namespace of first property to contribute to tag name
        if !namespace.truthy() {
            namespace = ns;
        }
    }
    Ok((tag, namespace))
}

/// Register namespace for specified user-defined table
/// Inputs: 0) tag/structure table ref
/// Returns: namespace prefix
pub fn register_namespace(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(register_namespace_v(it, &arg(&a, 0))?)
}

pub(crate) fn register_namespace_v(it: &mut Interp, table: &Value) -> ER<Value> {
    if !table.get("NAMESPACE").is_ref() {
        return Ok(table.get("NAMESPACE"));
    }
    let ns_ref = table.get("NAMESPACE");
    let ns_uri = pkg_hash(it, "Image::ExifTool::XMP::nsURI");
    let uri2ns = lex(it, "uri2ns");
    // recognize as either a list or hash
    let mut ns = Value::Undef;
    if ns_ref.ref_type() == "ARRAY" {
        ns = ns_ref.at(0);
        ns_uri.set(ns.to_bytes(), ns_ref.at(1));
        uri2ns.set(ns_ref.at(1).to_bytes(), ns.clone());
    } else {
        // must be a hash
        pl::at(3089);
        let mut nss = pl::hderef(&ns_ref)?.keys(); // allow multiple namespace definitions
        nss.sort();
        while let Some(k) = nss.pop() {
            ns = Value::from(k.clone());
            let cur = ns_uri.get(&k);
            if cur.truthy() && !same_str(&cur, &ns_ref.get(&k)) {
                it.warn(cat!("User-defined namespace prefix '", ns, "' conflicts with existing namespace\n"))?;
            }
            ns_uri.set(&k, ns_ref.get(&k));
            uri2ns.set(ns_ref.get(&k).to_bytes(), ns.clone());
        }
    }
    table.set("NAMESPACE", ns.clone());
    Ok(ns)
}

/// Get localized version of tagInfo hash
/// Inputs: 0) tagInfo hash ref, 1) language code (eg. "x-default")
/// Returns: new tagInfo hash ref, or undef if invalid
pub fn get_lang_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(get_lang_info_v(it, &arg(&a, 0), arg(&a, 1))?)
}

pub(crate) fn get_lang_info_v(it: &mut Interp, tag_info: &Value, mut lang_code: Value) -> ER<Value> {
    // only allow alternate language tags in lang-alt lists
    let w = tag_info.get("Writable");
    if !(w.truthy() && pl::str_eq(&w, "lang-alt")) {
        return Ok(Value::Undef);
    }
    tr(&mut lang_code, b"_", b"-", ""); // RFC 3066 specifies '-' as a separator
    let lang_info = pl::call(it, "Image::ExifTool::GetLangInfo", vec![tag_info.clone(), lang_code])?;
    Ok(lang_info)
}

/// Get standard case for language code
/// Inputs: 0) Language code
/// Returns: Language code in standard case
pub fn standard_lang_case(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(standard_lang_case_v(&arg(&a, 0)))
}

pub(crate) fn standard_lang_case_v(lang: &Value) -> Value {
    // make 2nd subtag uppercase only if it is 2 letters
    if let Some(c) = m(&rx(r"^([a-z]{2,3}|[xi])(-[a-z]{2})\b(.*)", "i"), lang) {
        return cat!(lc(&cap(&c, 1)), uc(&cap(&c, 2)), lc(&cap(&c, 3)));
    }
    lc(lang)
}

/// Convert XMP date/time to EXIF format
/// Inputs: 0) XMP date/time string, 1) set if we aren't sure this is a date
/// Returns: EXIF date/time, and flag in list context if this was a standard date/time value
pub fn convert_xmp_date(_it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (val, std) = convert_xmp_date_v(arg(&a, 0), &arg(&a, 1));
    if std && want == Want::List {
        return Ok(vec![val, n(1)]);
    }
    ret(val)
}

/// ConvertXMPDate: the value, and whether it was a standard date/time.
pub(crate) fn convert_xmp_date_v(mut val: Value, unsure: &Value) -> (Value, bool) {
    if let Some(c) = m(&rx(r"^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}:\d{2})(:\d{2})?\s*(\S*)$", ""), &val) {
        let s5 = cap(&c, 5);
        let s = if s5.truthy() { s5 } else { pl::s("") }; // seconds may be missing
        val = cat!(cap(&c, 1), ":", cap(&c, 2), ":", cap(&c, 3), " ", cap(&c, 4), s, cap(&c, 6)); // convert back to EXIF time format
        return (val, true);
    } else if !unsure.truthy() && m(&rx(r"^(\d{4})(-\d{2}){0,2}", ""), &val).is_some() {
        tr(&mut val, b"-", b":", "");
    }
    (val, false)
}

/// Convert rational string value
/// Inputs: 0) string (converted to number, 'inf' or 'undef' on return if rational)
/// Returns: true if value was converted
pub fn convert_rational(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    if !convert_rational_v(&mut val)? {
        return pl::ret_undef();
    }
    it.set_arg(0, val)?;
    ret(n(1))
}

/// ConvertRational on a variable (`$_[0]` is the caller's).
pub(crate) fn convert_rational_v(v: &mut Value) -> ER<bool> {
    let val = v.clone();
    let Some(c) = m(&rx(r"^(-?\d+)/(-?\d+)$", ""), &val) else { return Ok(false) };
    let (c1, c2) = (cap(&c, 1), cap(&c, 2));
    if !pl::num_eq(&c2, &n(0)) {
        *v = pl::div(&c1, &c2)?; // calculate quotient
    } else if c1.truthy() {
        *v = pl::s("inf");
    } else {
        *v = pl::s("undef");
    }
    Ok(true)
}

/// Convert a string of floating point values to rationals
/// Inputs: 0) string of floating point numbers separated by spaces
/// Returns: string of rational numbers separated by spaces
pub fn convert_rational_list(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    let mut vals = pl::split_ws(&val);
    if vals.len() != 4 {
        return ret(val);
    }
    for v in vals.iter_mut() {
        if !convert_rational_v(v)? {
            return ret(val);
        }
    }
    let joined: Vec<Vec<u8>> = vals.iter().map(|x| x.to_bytes().to_vec()).collect();
    ret(Value::bytes(joined.join(&b' ')))
}

/// Generate flattened tags and add to table
/// Inputs: 0) tag table ref, 1) tag ID for Struct tag (if not defined, whole table is done),
///         2) flag to not expand sub-structures, 3) Hidden flag
/// Returns: number of tags added (not counting those just initialized)
/// Notes: Must have verified that $$tagTablePtr{$tagID}{Struct} exists before calling this routine
/// - makes sure that the tagInfo Struct is a HASH reference
pub fn add_flattened_tags(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(n(add_flattened_tags_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3))?))
}

pub(crate) fn add_flattened_tags_v(it: &mut Interp, tag_table_ptr: &Value, tag_id: &Value, no_sub_struct: &Value, hidden: &Value) -> ER<i64> {
    local_topic(it, |it| {
        let mut count: i64 = 0;
        let mut tag_ids: Vec<Value> = Vec::new();

        if !tag_id.is_undef() {
            tag_ids.push(tag_id.clone());
        } else {
            for tid in pl::call_list(it, "Image::ExifTool::TagTableKeys", vec![tag_table_ptr.clone()])? {
                let tag_info = tag_table_ptr.get(tid.to_bytes());
                if !(is_hash(&tag_info) && tag_info.get("Struct").truthy()) {
                    continue;
                }
                tag_ids.push(tid);
            }
        }
        let special_struct = pkg_hash(it, "Image::ExifTool::XMP::specialStruct");
        let user_struct = pkg_hash(it, "Image::ExifTool::UserDefined::xmpStruct");

        // loop through specified tags
        for tag_id in &tag_ids {
            let tag_info = tag_table_ptr.get(tag_id.to_bytes());

            if tag_info.get("Flattened").truthy() {
                continue; // only generate flattened tags once
            }
            tag_info.set("Flattened", n(1));

            let mut str_table = tag_info.get("Struct");
            if !str_table.is_ref() {
                // (allow a structure name for backward compatibility only)
                let str_name = str_table.clone();
                str_table = user_struct.get(str_table.to_bytes());
                if !str_table.truthy() {
                    continue;
                }
                if !str_table.get("STRUCT_NAME").truthy() {
                    str_table.set("STRUCT_NAME", cat!("XMP ", str_name));
                }
                tag_info.set("Struct", str_table.clone()); // replace old-style name with HASH ref
                tag_info.delete("SubDirectory"); // deprecated use of SubDirectory in Struct tags
            }

            // get prefix for flattened tag names
            let flat = if !tag_info.get("FlatName").is_undef() { tag_info.get("FlatName") } else { tag_info.get("Name") };

            // get family 2 group name for this structure tag
            let mut tag_g2 = Value::Undef;
            if tag_info.get("Groups").truthy() {
                tag_g2 = tag_info.get("Groups").get("2");
            }
            if !tag_g2.truthy() {
                tag_g2 = hh(tag_table_ptr, "GROUPS").get("2");
            }

            for field in str_table.keys() {
                if special_struct.get(&field).truthy() {
                    continue;
                }
                let field = Value::from(field);
                let field_info = str_table.get(field.to_bytes());
                if field_info.get("LangCode").truthy() {
                    continue; // don't flatten lang-alt tags
                }
                if field_info.get("Struct").truthy() && no_sub_struct.truthy() {
                    continue; // don't expand sub-structures if specified
                }
                // build a tag ID for the corresponding flattened tag
                let field_name = ucfirst(&field);
                let flat_field = or(field_info.get("FlatName"), field_name.clone());
                let flat_id = pl::concat(tag_id, &field_name);
                let mut flat_info = tag_table_ptr.get(flat_id.to_bytes());
                if flat_info.truthy() {
                    if !is_hash(&flat_info) {
                        it.warn(cat!(flat_info, " is not a HASH!\n"))?;
                        continue; // (to be safe)
                    }
                    // pre-defined flattened tags should have Flat flag set
                    if flat_info.get("Flat").is_undef() {
                        if flat_info.get("NotFlat").truthy() {
                            continue;
                        }
                        if it.get_scalar("Image::ExifTool::debug").truthy() {
                            it.warn(cat!("Missing Flat flag for ", flat_info.get("Name"), "\n"))?;
                        }
                    }
                    flat_info.set("Flat", n(0));
                    // copy all missing entries from field information
                    for k in field_info.keys() {
                        // must not copy PropertyPath (but can't delete it afterwards
                        // because the flat tag may already have this set)
                        if &k[..] == b"PropertyPath" || !flat_info.get(&k).is_undef() {
                            continue;
                        }
                        // copy the property (making a copy of the Groups hash)
                        let v = if &k[..] == b"Groups" { field_info.get(&k).hash_copy() } else { field_info.get(&k) };
                        flat_info.set(&k, v);
                    }
                    // (NOTE: Can NOT delete Groups because we need them if GotGroups was done)
                    // re-generate List flag unless it is set to 0
                    if flat_info.get("List").truthy() {
                        flat_info.delete("List");
                    }
                } else {
                    // generate new flattened tag information based on structure field
                    let flat_name = pl::concat(&flat, &flat_field);
                    flat_info = field_info.hash_copy();
                    flat_info.set("Name", flat_name.clone());
                    flat_info.set("Flat", n(0));
                    if !hidden.truthy() {
                        flat_info.set("Hidden", n(0));
                    }
                    if field_info.get("FlatName").truthy() {
                        flat_info.set("FlatName", flat_name);
                    }
                    // make a copy of the Groups hash if necessary
                    if field_info.get("Groups").truthy() {
                        flat_info.set("Groups", field_info.get("Groups").hash_copy());
                    }
                    // add new flattened tag to table
                    pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), flat_id.clone(), flat_info.clone()])?;
                    count += 1;
                }
                // propagate List flag (unless set to 0 in pre-defined flattened tag)
                if flat_info.get("List").is_undef() && (field_info.get("List").truthy() || tag_info.get("List").truthy()) {
                    flat_info.set("List", or(field_info.get("List"), n(1)));
                }
                // set group 2 name from the first existing family 2 group in the:
                // 1) structure field Groups, 2) structure table GROUPS, 3) structure tag Groups
                if field_info.get("Groups").truthy() && field_info.get("Groups").get("2").truthy() {
                    flat_info.get_or_new_hash("Groups").set("2", field_info.get("Groups").get("2"));
                } else if str_table.get("GROUPS").truthy() && str_table.get("GROUPS").get("2").truthy() {
                    flat_info.get_or_new_hash("Groups").set("2", str_table.get("GROUPS").get("2"));
                } else {
                    flat_info.get_or_new_hash("Groups").set("2", tag_g2.clone());
                }
                // save reference to top-level and parent structures
                flat_info.set("RootTagInfo", or(tag_info.get("RootTagInfo"), tag_info.clone()));
                flat_info.set("ParentTagInfo", tag_info.clone());
                // recursively generate flattened tags for sub-structures
                if !flat_info.get("Struct").truthy() {
                    continue;
                }
                if flat_id.to_bytes().len() > 250 {
                    it.warn(cat!("Possible deep recursion for tag ", flat_id, "\n"))?;
                    break;
                }
                // reset flattened tag just in case we flattened hierarchy in the wrong order
                // because we must start from the outtermost structure to get the List flags right
                // (this should only happen when building tag tables)
                flat_info.delete("Flattened");
                count += add_flattened_tags_v(it, tag_table_ptr, &flat_id, &flat_info.get("NoSubStruct"), &Value::Undef)?;
            }
        }
        Ok(count)
    })
}

/// Scan for XMP in a file
/// Inputs: 0) ExifTool object ref, 1) RAF reference
/// Returns: 1 if xmp was found, 0 otherwise
/// Notes: Currently only recognizes UTF8-encoded XMP
pub fn scan_for_xmp(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, raf) = (arg(&a, 0), arg(&a, 1));
    let (mut buff, mut xmp) = (Value::Undef, Value::Undef);
    let mut last_buff = pl::s("");

    pl::method(it, &et, "VPrint", vec![n(0), pl::s("Scanning for XMP\n")])?;
    loop {
        if buff.is_undef() {
            pl::at(3260);
            check_invocant(&raf, "Read")?;
            let mut args = [buff.clone(), n(65536)];
            let r = method_aliased(it, &raf, "Read", &mut args)?;
            buff = args[0].clone();
            if !r.truthy() {
                return ret(n(0));
            }
        }
        if xmp.is_undef() {
            last_buff = pl::concat(&last_buff, &buff);
            let lb = last_buff.to_bytes();
            let Some(sp) = m_at(&rx(r"(<\?xpacket begin=)", ""), &lb, 0) else {
                // must keep last 15 bytes to match 16-byte "xpacket begin" string
                let bb = buff.to_bytes();
                last_buff = if bb.len() <= 15 { buff.clone() } else { substr(&bb, -15, None) };
                buff = Value::Undef;
                continue;
            };
            xmp = caps_of(&lb, &sp)[1].clone().unwrap_or_default();
            buff = substr(&lb, end_of(&sp) as i64, None);
        }
        let mut pos = xmp.to_bytes().len() as i64 - 18; // (18 = length("<?xpacket end...") - 1)
        xmp = pl::concat(&xmp, &buff); // add new data to our XMP
        let xb = xmp.to_bytes();
        let start = if pos > 0 { pos as usize } else { 0 }; // set start for "xpacket end" scan
        if let Some(sp) = m_at(&rx(r#"<\?xpacket end=['"][wr]['"]\?>"#, ""), &xb, start) {
            let e = end_of(&sp);
            buff = substr(&xb, e as i64, None); // save data after end of XMP
            xmp = substr(&xb, 0, Some(e as i64)); // isolate XMP
            // check XMP for validity (not valid if it contains null bytes)
            let x2 = xmp.to_bytes();
            pos = x2.iter().rposition(|&c| c == 0).map_or(-1, |p| p as i64) + 1;
            if pos == 0 {
                break;
            }
            last_buff = substr(&x2, pos, None); // re-parse beginning after last null byte
            xmp = Value::Undef;
        } else {
            buff = Value::Undef;
        }
    }
    if !et.get("FileType").truthy() {
        et.set("FILE_TYPE", et.get("FILE_EXT"));
        pl::method(it, &et, "SetFileType", vec![pl::s("<unknown file containing XMP>"), Value::Undef, pl::s("")])?;
    }
    let len = pl::length(&xmp);
    let dir_info = hash_of(vec![("DataPt", sref(xmp)), ("DirLen", len.clone()), ("DataLen", len)]);
    process_xmp_v(it, &et, &dir_info, Value::Undef)?;
    ret(n(1))
}

/// Print conversion for XMP-aux:LensID
/// Inputs: 0) ExifTool ref, 1) LensID, 2) Make, 3) LensInfo, 4) FocalLength,
///         5) LensModel, 6) MaxApertureValue
/// (yes, this is ugly -- blame Adobe)
pub fn print_lens_id(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    local_topic(it, |it| print_lens_id_v(it, a, want))
}

fn print_lens_id_v(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (et, mut id, make, info, focal_length, lens_model, max_av) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4), arg(&a, 5), arg(&a, 6));
    // Pentax changed its name to Ricoh
    let alt = |mk: &str| if mk == "Pentax" { Some("Ricoh") } else { None };
    // missing: Olympus (no XMP:LensID written by Adobe)
    for mk in ["Canon", "Nikon", "Pentax", "Sony", "Sigma", "Samsung", "Leica"] {
        let hit = m(&rx(mk, "i"), &make).is_some() || alt(mk).is_some_and(|al| m(&rx(al, "i"), &make).is_some());
        if !hit {
            continue;
        }
        // get name of module containing the lens lookup (default "Make.pm")
        let md = match mk {
            "Sigma" => "SigmaRaw",
            "Leica" => "Panasonic",
            _ => mk,
        };
        require(it, &format!("Image/ExifTool/{md}.pm"))?;
        // get the name of the lens name lookup (default "makeLensTypes")
        // (canonLensTypes, pentaxLensTypes, nikonLensIDs, etc)
        let conv_name = format!("Image::ExifTool::{md}::{}", if mk == "Nikon" { "nikonLensIDs".to_string() } else { format!("{}LensTypes", mk.to_ascii_lowercase()) });
        let mut print_conv = pkg_hash(it, &conv_name);
        if print_conv.is_empty() {
            break;
        }
        // sf = short focal
        // lf = long focal
        // sa = max aperture at short focal
        // la = max aperture at long focal
        let (mut sf, mut lf, mut sa, mut la) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef);
        if info.truthy() {
            let mut av = pl::split_ws(&info);
            for x in av.iter_mut() {
                if pl::str_eq(x, "undef") {
                    *x = Value::Undef;
                }
            }
            (sf, lf, sa, la) = (arg(&av, 0), arg(&av, 1), arg(&av, 2), arg(&av, 3));
            // for Sony and ambiguous LensID, $info data may be incorrect:
            // use only if it agrees with $focalLength and $maxAv (ref JR)
            let half = Value::Num(0.5);
            let q = Value::Num(0.15);
            if mk == "Sony"
                && ((focal_length.truthy() && ((sf.truthy() && pl::lt(&focal_length, &pl::sub(&sf, &half))) || (lf.truthy() && pl::gt(&focal_length, &pl::add(&lf, &half)))))
                    || (max_av.truthy() && ((sa.truthy() && pl::lt(&max_av, &pl::sub(&sa, &q))) || (la.truthy() && pl::gt(&max_av, &pl::add(&la, &q))))))
            {
                sf = Value::Undef;
                lf = Value::Undef;
                sa = Value::Undef;
                la = Value::Undef;
            } else if max_av.truthy() {
                // (using the short-focal-length max aperture in place of MaxAperture
                // is a bad approximation, so don't do this if MaxApertureValue exists)
                sa = Value::Undef;
            }
        }
        let _ = la;
        if mk == "Pentax" && m(&rx(r"^\d+$", ""), &id).is_some() {
            // for Pentax, CS4 stores an int16u, but we use 2 x int8u
            let packed = pack("n", &[id.clone()])?;
            id = join(b" ", &unpack(b"C*", &packed.to_bytes())?);
        }
        // Nikon is a special case because Adobe doesn't store the full LensID
        // (Apple Photos does, but we have to convert back to hex)
        if mk == "Nikon" {
            id = pl::sprintf("%X", &[id.clone()]);
            if id.to_bytes().len() & 0x01 != 0 {
                id = cat!("0", id); // pad with leading 0 if necessary
            }
            // put spaces between bytes
            if subst(&rx(r"(..)", ""), &mut id, true, |c| Ok(cat!(cap(c, 1), " ").to_bytes().to_vec()))? > 0 {
                subst(&rx(r" $", ""), &mut id, false, |_| Ok(Vec::new()))?;
            }
            let new_conv = Value::new_hash();
            let used = Value::new_hash();
            let mut i = 0i64;
            let re = rx_dyn(&cat!("^", id).to_bytes(), "")?;
            for k in print_conv.keys() {
                if m_at(&re, &k, 0).is_none() {
                    continue;
                }
                let lens = print_conv.get(&k);
                if used.get(lens.to_bytes()).truthy() {
                    continue; // avoid duplicates
                }
                used.set(lens.to_bytes(), n(1));
                let key = if i != 0 { cat!(id, ".", n(i)) } else { id.clone() };
                new_conv.set(key.to_bytes(), lens);
                i += 1;
            }
            print_conv = new_conv;
        }
        let str = or(print_conv.get(id.to_bytes()), cat!("Unknown (", id, ")"));
        return it.call_named("Image::ExifTool::Exif::PrintLensID", vec![et, str, print_conv, Value::Undef, id, focal_length, sa, max_av, sf, lf, lens_model], want);
    }
    ret(cat!("Unknown (", id, ")"))
}

/// We found an XMP property name/value
/// Inputs: 0) ExifTool object ref, 1) Pointer to tag table
///         2) reference to array of XMP property names (last is current property)
///         3) property value, 4) attribute hash ref (for 'xml:lang' or 'rdf:datatype')
/// Returns: 1 if valid tag was found
pub fn found_xmp(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let r = local_topic(it, |it| found_xmp_v(it, a))?;
    ret(r)
}

fn found_xmp_v(it: &mut Interp, a: Vec<Value>) -> ER<Value> {
    let (et, mut tag_table_ptr, props, mut val, mut attrs) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4));
    let mut lang = Value::Undef;
    let struct_props = Value::new_array(Vec::new());
    let (mut raw_val, mut rational) = (Value::Undef, Value::Undef);
    let sp_arg = if hh(&et, "OPTIONS").get("Struct").truthy() { struct_props.clone() } else { Value::Undef };
    let (mut tag, ns0) = get_xmp_tag_id_v(it, &props, &sp_arg, &Value::Undef)?;
    let mut ns = or(ns0, pl::s(""));
    if !tag.truthy() {
        return Ok(n(0)); // ignore things that aren't valid tags
    }

    let std_xlat_ns = pkg_hash(it, "Image::ExifTool::XMP::stdXlatNS");
    let xmp_ns = lex(it, "xmpNS");
    // translate namespace if necessary
    if std_xlat_ns.get(ns.to_bytes()).truthy() {
        ns = std_xlat_ns.get(ns.to_bytes());
    }
    let info = tag_table_ptr.get(ns.to_bytes());
    let (mut table, mut added, mut xns) = (Value::Undef, Value::Undef, Value::Undef);
    let mut tag_id;
    if info.truthy() {
        table = hh(&info, "SubDirectory").get("TagTable");
        if !table.truthy() {
            it.warn(cat!("Missing TagTable for ", tag, "!\n"))?;
        }
    } else if pl::str_eq(&props.at(0), "svg:svg") {
        if !ns.truthy() {
            // disambiguate MetadataID by adding back the 'metadata' we ignored
            if pl::str_eq(&tag, "id") && pl::str_eq(&props.at(1), "svg:metadata") {
                tag = pl::s("metadataId");
            }
            // use SVG namespace in SVG files if nothing better to use
            table = pl::s("Image::ExifTool::XMP::SVG");
        } else if !props.items().iter().any(|p| m(&rx("^rdf:", ""), p).is_some()) {
            // only other SVG information if not inside RDF (call it XMP if in RDF)
            table = pl::s("Image::ExifTool::XMP::otherSVG");
        }
    }

    let mut xml_groups = false;
    let grp0 = hh(&tag_table_ptr, "GROUPS").get("0");
    if !ns.truthy() && !pl::str_eq(&grp0, "XMP") {
        tag_id = tag.clone();
    } else if pl::str_eq(&grp0, "XML") && !table.truthy() {
        // this is an XML table (no namespace lookup)
        tag_id = cat!(ns, ":", tag);
    } else {
        if pl::str_eq(&grp0, "XML") {
            xml_groups = true;
        }
        // look up this tag in the appropriate table
        if !table.truthy() {
            table = pl::s("Image::ExifTool::XMP::other");
        }
        tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![table.clone()])?;
        if tag_table_ptr.get("NAMESPACE").truthy() {
            tag_id = tag.clone();
        } else {
            xns = xmp_ns.get(ns.to_bytes());
            if xns.is_undef() {
                xns = ns.clone();
                // validate namespace prefix
                if !(m(&rx(r"^[A-Z_a-z\x80-\xff][-.0-9A-Z_a-z\x80-\xff]*$", ""), &ns).is_some() || pl::str_eq(&ns, "")) {
                    pl::method(it, &et, "Warn", vec![cat!("Invalid XMP namespace prefix '", ns, "'")])?;
                    // clean up prefix for use as an ExifTool group name
                    tr(&mut ns, br"-.0-9A-Z_a-z\x80-\xff", b"", "dc");
                    if m(&rx(r"^[A-Z_a-z\x80-\xff]", ""), &ns).is_none() {
                        ns = cat!("ns_", ns);
                    }
                    std_xlat_ns.set(xns.to_bytes(), ns.clone());
                    xmp_ns.set(ns.to_bytes(), xns.clone());
                }
            }
            // add XMP namespace prefix to avoid collisions in variable-namespace tables
            tag_id = cat!(xns, ":", tag);
            // add namespace to top-level structure property
            if !struct_props.is_empty() {
                let e = struct_props.at(0);
                e.set_at(0, cat!(xns, ":", e.at(0)));
            }
        }
    }
    let mut tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag_id.clone()])?;

    if attrs.truthy() {
        lang = attrs.get("xml:lang");
    }

    // must add a new tag table entry if this tag isn't pre-defined
    // (or initialize from structure field if this is a pre-defined flattened tag)
    'no_loop: {
        if !(!tag_info.truthy() || tag_info.get("Flat").truthy()) {
            break 'no_loop;
        }
        let tag_list = Value::new_array(Vec::new());
        let ns_list = Value::new_array(Vec::new());
        get_xmp_tag_id_v(it, &props, &tag_list, &ns_list)?;
        let (mut t, mut added_flat) = (Value::Undef, Value::Undef);
        // build tag ID strings for each level in the property path
        for ta in tag_list.items() {
            // insert tag ID in index 1 of tagList list
            t = if t.truthy() { pl::concat(&t, &ucfirst(&ta.at(0))) } else { ta.at(0) };
            ta.set_at(1, t.clone());
            // generate flattened tags for top-level structure if necessary
            if !added_flat.is_undef() {
                continue;
            }
            let ti = tag_table_ptr.get(t.to_bytes());
            if !ti.truthy() {
                continue;
            }
            if !(is_hash(&ti) && ti.get("Struct").truthy()) {
                continue;
            }
            added_flat = n(add_flattened_tags_v(it, &tag_table_ptr, &t, &Value::Undef, &Value::Undef)?);
            // all done if we generated the tag we are looking for
            if added_flat.truthy() {
                tag_info = tag_table_ptr.get(tag_id.to_bytes());
                if tag_info.truthy() {
                    break 'no_loop;
                }
            }
        }
        let mut name = ucfirst(&tag);

        // search for the innermost containing structure
        // (in case tag is an unknown field in a known structure)
        // (only necessary if we found a structure above)
        if !added_flat.is_undef() {
            let mut t2 = pl::s("");
            let len = tag_list.len() as i64;
            let main = pkg_hash(it, "Image::ExifTool::XMP::Main");
            let mut i = len - 2;
            while i >= 0 {
                t = tag_list.at(i).at(1);
                t2 = pl::concat(&tag_list.at(i + 1).at(0), &ucfirst(&t2)); // build relative tag id
                let ti = tag_table_ptr.get(t.to_bytes());
                if !ti.truthy() || !is_hash(&ti) {
                    i -= 1;
                    continue;
                }
                let str_table = ti.get("Struct");
                if !str_table.truthy() {
                    i -= 1;
                    continue;
                }
                let flat = if !ti.get("FlatName").is_undef() { ti.get("FlatName") } else { ti.get("Name") };
                name = pl::concat(&flat, &ucfirst(&t2));
                // don't continue if structure is known but field is not
                if str_table.get("NAMESPACE").truthy() || !str_table.exists("NAMESPACE") {
                    break;
                }
                // this is a variable-namespace structure, so we must:
                // 1) get tagInfo from corresponding top-level XMP tag if it exists
                // 2) add new entry in this tag table, but with namespace prefix on tag ID
                let mut nn = ns_list.at(i + 1); // namespace of structure field
                // translate to standard ExifTool namespace
                if std_xlat_ns.get(nn.to_bytes()).truthy() {
                    nn = std_xlat_ns.get(nn.to_bytes());
                }
                let xn = or(xmp_ns.get(nn.to_bytes()), nn.clone()); // standard XMP namespace
                // no need to continue with variable-namespace logic if
                // we are in our own namespace (right?)
                if same_str(&xn, &or(tag_table_ptr.get("NAMESPACE"), pl::s(""))) {
                    break;
                }
                tag_id = cat!(xn, ":", tag); // add namespace to avoid collisions
                // change structure properties to add the standard XMP namespace
                // prefix for this field (needed for variable-namespace fields)
                if !struct_props.is_empty() {
                    let mut e = struct_props.at(i + 1);
                    if e.is_undef() {
                        e = Value::new_array(Vec::new());
                        struct_props.set_at((i + 1) as usize, e.clone());
                    }
                    e.set_at(0, cat!(xn, ":", e.at(0)));
                }
                // copy tagInfo entries from the existing top-level XMP tag
                let tg = main.get(nn.to_bytes());
                if !(is_hash(&tg) && tg.get("SubDirectory").truthy()) {
                    break;
                }
                let tbl = pl::call(it, "Image::ExifTool::GetTagTable", vec![tg.get("SubDirectory").get("TagTable")])?;
                if !tbl.truthy() {
                    break;
                }
                let mut sti = pl::method(it, &et, "GetTagInfo", vec![tbl.clone(), t2.clone()])?;
                if !sti.truthy() || sti.get("Flat").truthy() {
                    // again, we must initialize flattened tags if necessary
                    // (but don't bother to recursively apply full logic to
                    //  allow nested variable-namespace strucures until someone
                    //  actually wants to do such a silly thing)
                    let mut t3 = pl::s("");
                    let mut j = i + 1;
                    while j < len {
                        t3 = pl::concat(&tag_list.at(j).at(0), &ucfirst(&t3));
                        let ti3 = tbl.get(t3.to_bytes());
                        if !ti3.truthy() || !(is_hash(&ti3) && ti3.get("Struct").truthy()) {
                            j += 1;
                            continue;
                        }
                        if add_flattened_tags_v(it, &tbl, &t3, &Value::Undef, &Value::Undef)? == 0 {
                            break;
                        }
                        sti = tbl.get(t2.to_bytes());
                        break;
                    }
                    if !sti.truthy() {
                        break;
                    }
                }
                // use existing definition if we already added this tag
                if tag_table_ptr.get(tag_id.to_bytes()).truthy() {
                    tag_info = tag_table_ptr.get(tag_id.to_bytes());
                } else {
                    // generate new tagInfo hash based on existing top-level tag
                    tag_info = sti.hash_copy();
                    tag_info.set("Name", pl::concat(&flat, &sti.get("Name")));
                    // be careful not to copy elements we shouldn't...
                    tag_info.delete("Description"); // Description will be different
                    // can't copy group hash because group 1 will be different and
                    // we need to check this when writing tag to a specific group
                    tag_info.delete("Groups");
                    if sti.get("Groups").truthy() {
                        tag_info.get_or_new_hash("Groups").set("2", sti.get("Groups").get("2"));
                    }
                }
                break;
            }
        }
        // generate a default tagInfo hash if necessary
        if !tag_info.truthy() {
            // shorten tag name if necessary
            if et.get("ShortenXmpTags").truthy() {
                let shorten = et.get("ShortenXmpTags");
                name = call_code_scalar(it, &shorten, vec![name])?;
            }
            tag_info = hash_of(vec![("Name", name), ("IsDefault", n(1)), ("Priority", n(0))]);
        }
        // add tag Namespace entry for tags in variable-namespace tables
        if xns.truthy() {
            tag_info.set("Namespace", xns.clone());
        }
        let cu = hh(&et, "curURI").get(ns.to_bytes());
        if cu.truthy() {
            if let Some(c) = m(&rx(r"^http://ns.exiftool.(?:ca|org)/(.*?)/(.*?)/", ""), &cu) {
                let (mut g0, mut g1) = (cap(&c, 1), cap(&c, 2));
                // apply a little magic to recover original group names
                // from this exiftool-written RDF/XML file
                if pl::str_eq(&g1, "System") {
                    g1 = pl::s("XML-System");
                    g0 = pl::s("XML");
                } else if m(&rx(r"^\d", ""), &g1).is_some() {
                    // URI's with only family 0 are internal tags from the source file,
                    // so change the group name to avoid confusion with tags from this file
                    g1 = cat!("XML-", g0);
                    g0 = pl::s("XML");
                }
                tag_info.set("Groups", hash_of(vec![("0", g0), ("1", g1)]));
                // flag to avoid setting group 1 later
                tag_info.set("StaticGroup1", n(1));
            }
        }
        // construct tag information for this unknown tag
        // -> make this a List or lang-alt tag if necessary
        if props.len() > 2 && m(&rx(r"^rdf:li \d+$", ""), &props.at(-1)).is_some() {
            if let Some(c) = m(&rx(r"^rdf:(Bag|Seq|Alt)$", ""), &props.at(-2)) {
                if lang.truthy() && pl::str_eq(&cap(&c, 1), "Alt") {
                    tag_info.set("Writable", pl::s("lang-alt"));
                } else {
                    tag_info.set("List", cap(&c, 1));
                }
            }
            // tried this, but maybe not a good idea for complex structures:
            //} elsif (grep / /, @$props) {
            //    $$tagInfo{List} = 1;
        }
        let cur = tag_table_ptr.get(tag_id.to_bytes());
        if !(cur.truthy() && same_str(&cur, &tag_info)) {
            // save property list for verbose "adding" message unless this tag already exists
            if !cur.truthy() {
                added = tag_list.clone();
            }
            // if this is an empty structure, we must add a Struct field
            if !pl::length(&val).truthy() {
                if attrs.is_undef() {
                    attrs = Value::new_hash();
                }
                let pt = attrs.get("rdf:parseType");
                if pt.truthy() && pl::str_eq(&pt, "Resource") && !tag_info.get("Struct").truthy() {
                    tag_info.set("Struct", hash_of(vec![("STRUCT_NAME", pl::s("XMP Unknown"))]));
                }
            }
            tag_info.set("Hidden", n(2)); // (don't show in -list outputs)
            pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_table_ptr.clone(), tag_id.clone(), tag_info.clone()])?;
        }
    }
    // decode value if necessary (et:encoding was used before exiftool 7.71)
    if attrs.truthy() {
        let enc = or(attrs.get("rdf:datatype"), attrs.get("et:encoding"));
        if enc.truthy() && m(&rx("base64", ""), &enc).is_some() {
            val = decode_base64_v(val)?; // (now a value ref)
            let d = val.deref();
            if !(pl::gt(&pl::length(&d), &n(100)) || m(&rx(r"[\0-\x08\x0b\0x0c\x0e-\x1f]", ""), &d).is_some()) {
                val = d;
            }
        }
    }
    if !lang.is_undef() && !pl::str_eq(&lc(&lang), "x-default") {
        lang = standard_lang_case_v(&lang);
        let lang_info = get_lang_info_v(it, &tag_info, lang.clone())?;
        if lang_info.truthy() {
            tag_info = lang_info;
        }
    }
    // un-escape XML character entities (handling CDATA)
    let vb = val.to_bytes();
    let cdata = rx(r"<!\[CDATA\[(.*?)\]\]>", "s");
    if let Some(sp) = m_at(&cdata, &vb, 0) {
        let mut p = end_of(&sp) as i64;
        let l1 = sp[1].map_or(0, |(a, b)| b - a) as i64;
        // unescape everything up to the start of the CDATA section
        // (the length of "<[[CDATA[]]>" is 12 characters)
        let mut v = pl::concat(&unescape_xml_v(it, substr(&vb, 0, Some(p - l1 - 12)), Value::Undef, Value::Undef)?, &caps_of(&vb, &sp)[1].clone().unwrap_or_default());
        while let Some(sp) = m_at(&cdata, &vb, p as usize) {
            let p1 = end_of(&sp) as i64;
            let l1 = sp[1].map_or(0, |(a, b)| b - a) as i64;
            let u = unescape_xml_v(it, substr(&vb, p, Some(p1 - p - l1 - 12)), Value::Undef, Value::Undef)?;
            v = cat!(v, u, caps_of(&vb, &sp)[1].clone().unwrap_or_default());
            p = p1;
        }
        let u = unescape_xml_v(it, substr(&vb, p, None), Value::Undef, Value::Undef)?;
        val = pl::concat(&v, &u);
    } else {
        val = unescape_xml_v(it, val, Value::Undef, Value::Undef)?;
    }
    // decode from UTF8
    val = pl::method(it, &et, "Decode", vec![val, pl::s("UTF8")])?;
    // convert rational and date values to a more sensible format
    let fmt = tag_info.get("Writable");
    let is_default = tag_info.get("IsDefault");
    let new = if is_default.truthy() { hh(&et, "OPTIONS").get("XMPAutoConv") } else { is_default };
    if fmt.truthy() || new.truthy() {
        raw_val = val.clone(); // save raw value for verbose output
        if (new.truthy() || pl::str_eq(&fmt, "rational")) && convert_rational_v(&mut val)? {
            rational = raw_val.clone();
        } else {
            let mut std_date = false;
            if new.truthy() || pl::str_eq(&fmt, "date") {
                let (v, s) = convert_xmp_date_v(val, &new);
                val = v;
                std_date = s;
            }
            if std_date && added.truthy() {
                tag_info.get_or_new_hash("Groups").set("2", pl::s("Time"));
                tag_info.set("PrintConv", pl::s("$self->ConvertDateTime($val)"));
            }
        }
        if et.get("XmpValidate").truthy() && fmt.truthy() && pl::str_eq(&fmt, "boolean") && m(&rx(r"^True|False$", ""), &val).is_none() {
            if m(&rx(r"^true|false$", ""), &val).is_some() {
                pl::method(it, &et, "Warn", vec![cat!("Boolean value for XMP-", ns, ":", tag_info.get("Name"), " should be capitalized"), n(1)])?;
            } else {
                pl::method(it, &et, "Warn", vec![cat!("Boolean value for XMP-", ns, ":", tag_info.get("Name"), " should be \"True\" or \"False\""), n(1)])?;
            }
        }
        // protect against large binary data in unknown tags
        if new.truthy() && val.to_bytes().len() > 65536 {
            tag_info.set("Binary", n(1));
        }
    }
    if hh(&et, "OPTIONS").get("Verbose").truthy() {
        let tid = join(b"/", &props.items());
        pl::method(it, &et, "VerboseInfo", vec![tid, tag_info.clone(), pl::s("Value"), or(raw_val.clone(), val.clone())])?;
    }
    // store the value for this tag
    let key = pl::method(it, &et, "FoundTag", vec![tag_info.clone(), val])?;
    if !key.truthy() {
        return Ok(n(0));
    }
    // save original components of rational numbers (used when copying)
    if !rational.is_undef() {
        et.get_or_new_hash("TAG_EXTRA").get_or_new_hash(key.to_bytes()).set("Rational", rational);
    }
    // save structure/list information if necessary
    if !struct_props.is_empty() && (struct_props.len() > 1 || !struct_props.at(0).at(1).is_undef()) && !et.get("NO_STRUCT").truthy() {
        et.get_or_new_hash("TAG_EXTRA").get_or_new_hash(key.to_bytes()).set("Struct", struct_props.clone());
        et.set("IsStruct", n(1));
    }
    if xml_groups {
        pl::method(it, &et, "SetGroup", vec![key.clone(), pl::s("XML"), n(0)])?;
        pl::method(it, &et, "SetGroup", vec![key.clone(), cat!("XML-", ns), n(1)])?;
    } else if ns.truthy() && !tag_info.get("StaticGroup1").truthy() {
        // set group1 dynamically according to the namespace
        let g = cat!(hh(&tag_table_ptr, "GROUPS").get("0"), "-", ns);
        pl::method(it, &et, "SetGroup", vec![key.clone(), g])?;
    }
    if added.truthy() && hh(&et, "OPTIONS").get("Verbose").truthy() {
        let props_s = if added.len() > 1 {
            tag_info.set("Flat", n(0)); // this is a flattened tag
            let ps: Vec<Value> = added.items().iter().map(|x| x.at(0)).collect();
            cat!(" (", join(b"/", &ps), ")")
        } else {
            pl::s("")
        };
        let g1 = pl::method(it, &et, "GetGroup", vec![key.clone(), n(1)])?;
        pl::method(it, &et, "VPrint", vec![n(0), et.get("INDENT"), cat!("[adding ", g1, ":", tag, "]", props_s, "\n")])?;
    }
    // allow read-only subdirectories (eg. embedded base64 XMP/IPTC in NKSC files)
    if tag_info.get("SubDirectory").truthy() && !et.get("IsWriting").truthy() {
        let subdir = tag_info.get("SubDirectory");
        let vh = hh(&et, "VALUE");
        let v = vh.get(key.to_bytes());
        let mut data_pt = if v.is_ref() { v } else { elem_ref(&vh, &key.to_bytes()) };
        // decode if necessary (eg. Nikon XMP-ast:XMLPackets)
        let enc = tag_info.get("Encoding");
        if enc.truthy() && pl::str_eq(&enc, "Base64") {
            data_pt = decode_base64_v(data_pt.deref())?;
        }
        // process subdirectory information
        let dir_info = hash_of(vec![
            ("DirName", or(subdir.get("DirName"), tag_info.get("Name"))),
            ("DataPt", data_pt.clone()),
            ("DirLen", pl::length(&data_pt.deref())),
            ("TagInfo", tag_info.clone()),
            ("IgnoreProp", subdir.get("IgnoreProp")), // (allow XML to ignore specified properties)
            ("IsExtended", n(1)),                     // (hack to avoid Duplicate warning for embedded XMP)
            ("NoStruct", n(1)),                       // (don't try to build structures since this isn't true XMP)
            ("NoBlockSave", n(1)),                    // (don't save as a block because we already did this)
        ]);
        let old_order = pl::call(it, "Image::ExifTool::GetByteOrder", vec![])?;
        if subdir.get("ByteOrder").truthy() {
            pl::call(it, "Image::ExifTool::SetByteOrder", vec![subdir.get("ByteOrder")])?;
        }
        let old_ns = et.get("definedNS");
        et.delete("definedNS");
        let sub_table_ptr = or(pl::call(it, "Image::ExifTool::GetTagTable", vec![subdir.get("TagTable")])?, tag_table_ptr.clone());
        pl::method(it, &et, "ProcessDirectory", vec![dir_info, sub_table_ptr, subdir.get("ProcessProc")])?;
        pl::call(it, "Image::ExifTool::SetByteOrder", vec![old_order])?;
        et.set("definedNS", old_ns);
    }
    Ok(n(1))
}

/// `grep /RE/, @list` is not empty.
fn any_match(list: &Value, re: &PerlRegex) -> bool {
    list.items().iter().any(|p| m(re, p).is_some())
}

/// Recursively parse nested XMP data element
/// Inputs: 0) ExifTool ref, 1) tag table ref, 2) XMP data ref
///         3) offset to start of XMP element, 4) offset to end of XMP element
///         5) reference to array of enclosing XMP property names (undef if none)
///         6) reference to blank node information hash
/// Returns: Number of contained XMP elements
pub fn parse_xmp_element(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let r = parse_xmp_element_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), arg(&a, 3), arg(&a, 4), arg(&a, 5), arg(&a, 6))?;
    ret(n(r))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn parse_xmp_element_v(it: &mut Interp, et: &Value, tag_table_ptr: &Value, data_pt: &Value, start: Value, end: Value, prop_list: Value, blank_info: Value) -> ER<i64> {
    local_topic(it, |it| parse_xmp_element_body(it, et, tag_table_ptr, data_pt, start, end, prop_list, blank_info))
}

#[allow(clippy::too_many_arguments)]
fn parse_xmp_element_body(it: &mut Interp, et: &Value, tag_table_ptr: &Value, data_pt: &Value, start: Value, end: Value, mut prop_list: Value, mut blank_info: Value) -> ER<i64> {
    let (mut count, mut n_items): (i64, i64) = (0, 0);
    let is_writing = et.get("XMP_CAPTURE");
    let is_svg = et.get("XMP_IS_SVG");
    let mut save_ns = Value::Undef; // save xlatNS lookup if changed for the scope of this element
    // namespaces defined and used in this scope
    let (defined_ns, used_ns) = (Value::new_hash(), Value::new_hash());
    let mut err = Value::Undef;

    // get our parse procs
    let (mut attr_proc, found_proc);
    attr_proc = Value::Undef;
    if et.get("XMPParseOpts").truthy() {
        attr_proc = et.get("XMPParseOpts").get("AttrProc");
        let fp = et.get("XMPParseOpts").get("FoundProc");
        found_proc = if fp.truthy() { fp } else { code_ref(it, "Image::ExifTool::XMP::FoundXMP") };
    } else {
        found_proc = code_ref(it, "Image::ExifTool::XMP::FoundXMP");
    }
    // ($end or $end = length $$dataPt; then pos($$dataPt) = $start, which
    // autovivifies an undefined $dataPt but dies on any other non-reference)
    if !end.truthy() {
        pl::at(3788);
        pl::sderef(data_pt)?;
    } else if !data_pt.is_undef() {
        pl::at(3796);
        pl::sderef(data_pt)?;
    }
    let data = data_pt.deref().to_bytes();
    let mut start: i64 = if start.truthy() { start.to_i64() } else { 0 };
    let end: i64 = if end.truthy() { end.to_i64() } else { data.len() as i64 };
    if !prop_list.truthy() {
        prop_list = Value::new_array(Vec::new());
    }

    let mut process_blank_info = Value::Undef;
    // create empty blank node information hash if necessary
    if !blank_info.truthy() {
        process_blank_info = hash_of(vec![("Prop", Value::new_hash())]);
        blank_info = process_blank_info.clone();
    }
    // keep track of current nodeID at this nesting level
    let old_node_id = blank_info.get("NodeID");
    let mut pos: usize = start.max(0) as usize;

    // lookup for translating namespace prefixes
    let mut xlat_ns = et.get("xlatNS");

    let ignore_namespace = lex(it, "ignoreNamespace");
    let ignore_prop = lex(it, "ignoreProp");
    let ignore_et_prop = lex(it, "ignoreEtProp");
    let recognized_attrs = lex(it, "recognizedAttrs");
    let uri2ns = lex(it, "uri2ns");
    let ns_uri = pkg_hash(it, "Image::ExifTool::XMP::nsURI");
    let std_xlat_ns = pkg_hash(it, "Image::ExifTool::XMP::stdXlatNS");
    let elem_re = rx(r"<([?/]?)([-\w:.\x80-\xff]+|!--)([^>]*)>|(<!\[CDATA\[)", "s");
    let cdata_end = rx(r"\]\]>", "s");
    let comment_end = rx(r"-->", "s");
    let colon = rx(r"(.*?):", "");

    'element: loop {
        // all done if there isn't enough data for another element
        // (the smallest possible element is 4 bytes, eg. "<a/>")
        if pos as i64 > end - 4 {
            break;
        }
        // reset nodeID before processing each element
        let mut node_id = old_node_id.clone();
        blank_info.set("NodeID", old_node_id.clone());
        // find start of next element, skipping over "<![CDATA[xxx]]>" sections
        let Some(sp) = m_at(&elem_re, &data, pos) else { break };
        pos = end_of(&sp);
        if pos as i64 > end {
            break;
        }
        let c = caps_of(&data, &sp);
        // (the only reason we match '<[?/]' is to keep from scanning past the
        //  "<?xpacket end..." terminator or other closing token, so
        if cap(&c, 1).truthy() {
            continue;
        }
        if cap(&c, 4).truthy() {
            // skip CDATA
            if let Some(s2) = m_at(&cdata_end, &data, pos) {
                pos = end_of(&s2);
                if pos as i64 <= end {
                    continue;
                }
            }
            err = pl::s("Missing CDATA terminator");
            break 'element;
        }
        let (mut prop, mut attrs) = (cap(&c, 2), cap(&c, 3));
        // skip comments
        if pl::str_eq(&prop, "!--") {
            if m(&rx("--$", ""), &attrs).is_some() {
                continue;
            }
            if let Some(s2) = m_at(&comment_end, &data, pos) {
                pos = end_of(&s2);
                continue;
            }
            break;
        }
        let mut val_start = pos as i64;
        let val_end: i64;
        let mut was_comment = false;
        // only look for closing token if this is not an empty element
        // (empty elements end with '/', eg. <a:b/>)
        if subst(&rx(r"\/$", ""), &mut attrs, false, |_| Ok(Vec::new()))? == 0 {
            let mut nesting = 1;
            let mut pat = b"<(?:(/?)".to_vec();
            pat.extend_from_slice(&pre::quotemeta(&prop.to_bytes()));
            pat.extend_from_slice(br"([-\w:.\x80-\xff]*)(.*?(/?))>|(!\[CDATA\[|!--))");
            // (one pattern per property name, matched a few times: no JIT)
            let close_re = pre::compile_few(&pat, "s").or_else(die)?;
            loop {
                // this match fails with perl 5.6.2 (perl bug!), but it works without
                // the '(.*?)', so we must do it differently...
                //                $$dataPt =~ m/(.*?)<\/$prop>/sg or last Element;
                //                my $val2 = $1;
                // find next matching closing token, or the next opening token
                // of a nested same-named element
                let found = match m_at(&close_re, &data, pos) {
                    Some(sp) => {
                        pos = end_of(&sp);
                        if pos as i64 > end {
                            None
                        } else {
                            Some(caps_of(&data, &sp))
                        }
                    }
                    None => None,
                };
                let Some(c) = found else {
                    err = cat!("XMP format error (no closing tag for ", prop, ")");
                    break 'element;
                };
                if cap(&c, 5).truthy() {
                    // find end of CDATA or comment section
                    if pl::str_eq(&cap(&c, 5), "![CDATA[") {
                        if let Some(s2) = m_at(&cdata_end, &data, pos) {
                            pos = end_of(&s2);
                            if pos as i64 <= end {
                                continue;
                            }
                        }
                        err = pl::s("Missing CDATA terminator");
                        break 'element;
                    } else {
                        if let Some(s2) = m_at(&comment_end, &data, pos) {
                            pos = end_of(&s2);
                            if pos as i64 <= end {
                                was_comment = true;
                                continue;
                            }
                        }
                        err = pl::s("Missing comment terminator");
                        break 'element;
                    }
                }
                if cap(&c, 2).truthy() {
                    continue; // ignore opening properties with different names
                }
                if cap(&c, 1).truthy() {
                    nesting -= 1;
                    if nesting != 0 {
                        continue;
                    }
                    val_end = pos as i64 - prop.to_bytes().len() as i64 - cap(&c, 3).to_bytes().len() as i64 - 3;
                    break; // this element is complete
                }
                // this is a nested opening token (or empty element)
                if !cap(&c, 4).truthy() {
                    nesting += 1;
                }
            }
        } else {
            val_end = val_start;
        }
        start = pos as i64; // start from here the next time around

        // ignore specified XMP namespaces/properties
        if et.get("EXCL_XMP_LOOKUP").truthy() && !is_writing.truthy() {
            if let Some(c) = m(&rx(r"^(.+):(.*)", ""), &prop) {
                let (ns, nm) = (lc(&or(std_xlat_ns.get(cap(&c, 1).to_bytes()), cap(&c, 1))), lc(&cap(&c, 2)));
                let ex = et.get("EXCL_XMP_LOOKUP");
                if ex.get(cat!("xmp-", ns, ":all").to_bytes()).truthy() || ex.get(cat!("xmp-", ns, ":", nm).to_bytes()).truthy() || ex.get(cat!("xmp-all:", nm).to_bytes()).truthy() {
                    count += 1; // (pretend we found something so we don't store as a tag value)
                    continue;
                }
            }
        }

        // extract property attributes
        let mut parse_resource = false;
        let attrs_h = Value::new_hash();
        let attrs_a = Value::new_array(Vec::new());
        // this hangs Perl (v5.18.4) for a specific capture string [patched in ExifTool 12.98]
        //        while ($attrs =~ m/(\S+?)\s*=\s*(['"])(.*?)\2/sg) {
        // this may hang Perl v5.26.3 (but not v5.18.4) if there is lots of garbage in the XMP [patched in 13.23]
        //        while ($attrs =~ /(\S+?)\s*=\s*(['"])/g) {
        let ab = attrs.to_bytes();
        let mut apos = 0usize;
        loop {
            let (mut attr, quote);
            if ab.len() < 2000 {
                // (do it the easy way if attributes aren't stupid long)
                let Some(sp) = m_at(&rx(r#"(\S+?)\s*=\s*(['"])"#, ""), &ab, apos) else { break };
                apos = end_of(&sp);
                let c = caps_of(&ab, &sp);
                (attr, quote) = (cap(&c, 1), cap(&c, 2));
            } else {
                // 13.23 patch to avoid capturing tons of garbage if XMP is corrupted
                let Some(sp) = m_at(&rx(r#"=\s*(['"])"#, ""), &ab, apos) else { break };
                apos = end_of(&sp);
                quote = caps_of(&ab, &sp)[1].clone().unwrap_or_default();
                let p = if apos > 1000 { apos - 1000 } else { 0 };
                let tmp = Value::bytes(&ab[p..apos]);
                let mut pat = br"(\S+)\s*=\s*".to_vec();
                pat.extend_from_slice(&quote.to_bytes());
                pat.push(b'$');
                let Some(c) = m(&*rx_dyn(&pat, "")?, &tmp) else { break };
                attr = cap(&c, 1);
            }
            let p0 = apos;
            let Some(sp) = m_at(&*rx_dyn(&quote.to_bytes(), "")?, &ab, apos) else { break };
            apos = end_of(&sp);
            let mut val = Value::bytes(&ab[p0..apos - 1]);
            // handle namespace prefixes (defined by xmlns:PREFIX, or used with PREFIX:tag)
            if let Some(c) = m(&colon, &attr) {
                let c1 = cap(&c, 1);
                if pl::str_eq(&c1, "xmlns") {
                    let ns = substr(&attr.to_bytes(), 6, None);
                    let mut std_ns = uri2ns.get(val.to_bytes());
                    // keep track of namespace prefixes defined in this scope (for Validate)
                    let edn = hh(et, "definedNS");
                    if !edn.get(ns.to_bytes()).truthy() {
                        defined_ns.set(ns.to_bytes(), n(1));
                        edn.set(ns.to_bytes(), n(1));
                    }
                    if !std_ns.truthy() {
                        let mut try_ = val.clone();
                        // patch for Nikon NX2 URI bug for Microsoft PhotoInfo namespace
                        if subst(&rx("/$", ""), &mut try_, false, |_| Ok(Vec::new()))? == 0 {
                            try_ = cat!(try_, "/");
                        }
                        std_ns = uri2ns.get(try_.to_bytes());
                        if std_ns.truthy() {
                            val = try_;
                            pl::method(it, et, "Warn", vec![cat!("Fixed incorrect URI for xmlns:", ns), n(1)])?;
                        } else if m(&rx(r"^http://ns.nikon.com/BASIC_PARAM", ""), &val).is_some() {
                            pl::method(it, et, "OverrideFileType", vec![pl::s("NXD"), pl::s("application/x-nikon-nxd")])?;
                        } else {
                            // look for same namespace with different version number
                            try_ = Value::bytes(pre::quotemeta(&val.to_bytes())); // (note: escapes slashes too)
                            subst(&rx(r"\\/\d+\\\.\d+(\\/|$)", ""), &mut try_, false, |c| Ok(cat!(r"\/\d+\.\d+", cap(c, 1)).to_bytes().to_vec()))?;
                            let re = rx_dyn(&cat!("^", try_, "$").to_bytes(), "")?;
                            let good = uri2ns.keys().into_iter().find(|k| m_at(&re, k, 0).is_some());
                            if let Some(good) = good {
                                std_ns = uri2ns.get(&good);
                                pl::method(it, et, "VPrint", vec![n(0), et.get("INDENT"), cat!("[different ", std_ns, " version: ", val, "]\n")])?;
                            }
                        }
                    }
                    // tame wild namespace prefixes (patches Microsoft stupidity)
                    let mut new_ns: Option<Value> = None;
                    if std_ns.truthy() {
                        // use standard namespace prefix if pre-defined
                        if !same_str(&std_ns, &ns) {
                            new_ns = Some(std_ns.clone());
                        } else if xlat_ns.get(ns.to_bytes()).truthy() {
                            // this prefix is re-defined to the standard prefix in this scope
                            new_ns = Some(pl::s(""));
                        }
                    } else if hh(et, "curNS").get(val.to_bytes()).truthy() {
                        // use a consistent prefix over the entire XMP for a given namespace URI
                        let cn = et.get("curNS").get(val.to_bytes());
                        if !same_str(&cn, &ns) {
                            new_ns = Some(cn);
                        }
                    } else {
                        let cur_uri = et.get("curURI");
                        let cur_ns = et.get("curNS");
                        let mut used = ns.clone();
                        // use unique prefixes for all namespaces across the entire XMP
                        if cur_uri.get(ns.to_bytes()).truthy() || ns_uri.get(ns.to_bytes()).truthy() {
                            // generate a temporary namespace prefix to resolve any conflict
                            let mut i = 0;
                            while cur_uri.get(format!("tmp{i}")).truthy() {
                                i += 1;
                            }
                            used = Value::from(format!("tmp{i}"));
                            new_ns = Some(used.clone());
                        }
                        // keep track of the namespace prefixes and URI's used in this XMP
                        cur_ns.set(val.to_bytes(), used.clone());
                        cur_uri.set(used.to_bytes(), val.clone());
                    }
                    if let Some(new_ns) = new_ns {
                        // save translation used in containing scope if necessary
                        // create new namespace translation for the scope of this element
                        if !save_ns.truthy() {
                            save_ns = xlat_ns.clone();
                            xlat_ns = xlat_ns.hash_copy();
                            et.set("xlatNS", xlat_ns.clone());
                        }
                        if !new_ns.to_bytes().is_empty() {
                            // use the new namespace prefix
                            xlat_ns.set(ns.to_bytes(), new_ns.clone());
                            attr = cat!("xmlns:", new_ns);
                            // must go through previous attributes and change prefixes if necessary
                            for (i, x) in attrs_a.items().into_iter().enumerate() {
                                let Some(c) = m(&colon, &x) else { continue };
                                if !(same_str(&cap(&c, 1), &ns) && !same_str(&cap(&c, 1), &new_ns)) {
                                    continue;
                                }
                                let new_attr = pl::concat(&new_ns, &substr(&x.to_bytes(), ns.to_bytes().len() as i64, None));
                                attrs_h.set(new_attr.to_bytes(), attrs_h.get(x.to_bytes()));
                                attrs_h.delete(x.to_bytes());
                                attrs_a.set_at(i, new_attr);
                            }
                        } else {
                            xlat_ns.delete(ns.to_bytes());
                        }
                    }
                } else {
                    let x = xlat_ns.get(c1.to_bytes());
                    if x.truthy() {
                        attr = pl::concat(&x, &substr(&attr.to_bytes(), c1.to_bytes().len() as i64, None));
                    }
                    used_ns.set(c1.to_bytes(), n(1));
                }
            }
            attrs_a.push(attr.clone()); // preserve order
            attrs_h.set(attr.to_bytes(), val);
        }
        if let Some(c) = m(&colon, &prop) {
            let c1 = cap(&c, 1);
            used_ns.set(c1.to_bytes(), n(1));
            // tame wild namespace prefixes (patch for Microsoft stupidity)
            let x = xlat_ns.get(c1.to_bytes());
            if x.truthy() {
                prop = pl::concat(&x, &substr(&prop.to_bytes(), c1.to_bytes().len() as i64, None));
            }
        }

        if pl::str_eq(&prop, "rdf:li") {
            // impose a reasonable maximum on the number of items in a list
            if n_items == 1000 {
                let (tg, ns) = get_xmp_tag_id_v(it, &prop_list, &Value::Undef, &Value::Undef)?;
                let ns = or(ns, pl::s(""));
                if is_writing.truthy() {
                    pl::method(it, et, "Warn", vec![cat!("Excessive number of items for ", ns, ":", tg, ". Processing may be slow"), n(1)])?;
                } else if !hh(et, "OPTIONS").get("IgnoreMinorErrors").truthy() {
                    pl::method(it, et, "Warn", vec![cat!("Extracted only 1000 ", ns, ":", tg, " items. Ignore minor errors to extract all"), n(2)])?;
                    break;
                }
            }
            // add index to list items so we can keep them in order
            // (this also enables us to keep structure elements grouped properly
            // for lists of structures, like JobRef)
            // Note: the list index is prefixed by the number of digits so sorting
            // alphabetically gives the correct order while still allowing a flexible
            // number of digits -- this scheme allows up to 9 digits in the index,
            // with index numbers ranging from 0 to 999999999.  The sequence is:
            // 10,11,12-19,210,211-299,3100,3101-3999,41000...9999999999.
            let ni = n_items.to_string();
            prop = cat!(prop, " ", ni.len().to_string(), ni);
            // reset LIST_TAGS at the start of the outtermost list
            // (avoids accumulating incorrectly-written elements in a correctly-written list)
            if n_items == 0 && !any_match(&prop_list, &rx("^rdf:li ", "")) {
                et.set("LIST_TAGS", Value::new_hash());
            }
            n_items += 1;
        } else if pl::str_eq(&prop, "rdf:Description") {
            // remove unnecessary rdf:Description elements since parseType='Resource'
            // is more efficient (also necessary to make property path consistent)
            if any_match(&prop_list, &rx("^rdf:Description$", "")) {
                parse_resource = true;
                // set parseType so we know this is a structure
                attrs_h.set("rdf:parseType", pl::s("Resource"));
            }
        } else if pl::str_eq(&prop, "xmp:xmpmeta") {
            // patch MicrosoftPhoto unconformity
            prop = pl::s("x:xmpmeta");
            if et.get("XmpValidate").truthy() {
                pl::method(it, et, "Warn", vec![pl::s("Wrong namespace for xmpmeta")])?;
            }
        }

        // hook for special parsing of attributes
        let mut val = Value::Undef;
        if attr_proc.truthy() {
            val = substr(&data, val_start, Some(val_end - val_start));
            let (pr, vr) = (sref(prop.clone()), sref(val.clone()));
            let changed = call_code_scalar(it, &attr_proc, vec![attrs_a.clone(), attrs_h.clone(), pr.clone(), vr.clone()])?;
            prop = pr.deref();
            val = vr.deref();
            if changed.truthy() {
                // the value was changed, so reset $valStart/$valEnd to use $val instead
                val_start = val_end;
            }
        }

        // add nodeID to property path (with leading ' #') if it exists
        if !attrs_h.get("rdf:nodeID").is_undef() {
            node_id = attrs_h.get("rdf:nodeID");
            blank_info.set("NodeID", node_id.clone());
            attrs_h.delete("rdf:nodeID");
            prop = cat!(prop, " #", node_id);
            parse_resource = false; // can't ignore if this is a node
        }

        // push this property name onto our hierarchy list
        if !parse_resource {
            prop_list.push(prop.clone());
        }

        if is_svg.truthy() {
            // ignore everything but top level SVG tags and metadata unless Unknown set
            let opts = hh(et, "OPTIONS");
            if !(pl::gt(&opts.get("Unknown"), &n(1)) || opts.get("Verbose").truthy()) && prop_list.len() > 1 && m(&rx(r"\b(metadata|desc|title)$", ""), &prop_list.at(1)).is_none() {
                apop(&prop_list);
                continue;
            }
            if pl::str_eq(&prop, "svg") || pl::str_eq(&prop, "metadata") {
                // add svg namespace prefix if missing to ignore these entries in the tag name
                let last = prop_list.len() - 1;
                prop_list.set_at(last, cat!("svg:", prop));
            }
        } else if et.get("XmpIgnoreProps").truthy() {
            // ignore specified properties for tag name
            for x in et.get("XmpIgnoreProps").items() {
                if prop_list.is_empty() {
                    break;
                }
                if same_str(&x, &prop_list.at(0)) {
                    apop(&prop_list);
                }
            }
        }

        // handle properties inside element attributes (RDF shorthand format):
        // (attributes take the form a:b='c' or a:b="c")
        let (mut shorthand, mut ignored) = (false, Value::Undef);
        for short_name in attrs_a.items() {
            if attrs_h.get(short_name.to_bytes()).is_undef() {
                continue;
            }
            let mut prop_name = short_name.clone();
            let (ns, name);
            if let Some(c) = m(&rx(r"(.*?):(.*)", ""), &prop_name) {
                ns = cap(&c, 1); // specified namespace
                name = cap(&c, 2);
            } else if let Some(c) = m(&rx(r"(\S*?):", ""), &prop) {
                ns = cap(&c, 1); // assume same namespace as parent
                name = prop_name.clone();
                prop_name = cat!(ns, ":", name); // generate full property name
            } else {
                // a property qualifier is the only property name that may not
                // have a namespace, and a qualifier shouldn't have attributes,
                // but what the heck, let's allow this anyway
                ns = pl::s("");
                name = prop_name.clone();
            }
            if pl::str_eq(&prop_name, "rdf:about") {
                let sv = attrs_h.get(short_name.to_bytes());
                if !et.get("XmpAbout").truthy() {
                    et.set("XmpAbout", sv);
                } else if !same_str(&et.get("XmpAbout"), &sv) {
                    if is_writing.truthy() {
                        let str = pl::s("Different 'rdf:about' attributes not handled");
                        if !hh(et, "WAS_WARNED").get(str.to_bytes()).truthy() {
                            pl::method(it, et, "Error", vec![str.clone(), n(1)])?;
                            et.get_or_new_hash("WAS_WARNED").set(str.to_bytes(), n(1));
                        }
                    } else if et.get("XmpValidate").truthy() {
                        pl::method(it, et, "Warn", vec![pl::s("Different 'rdf:about' attributes")])?;
                    }
                }
            }
            if is_writing.truthy() {
                // keep track of our namespaces when writing
                if pl::str_eq(&ns, "xmlns") {
                    let std_ns = uri2ns.get(attrs_h.get(short_name.to_bytes()).to_bytes());
                    if !(std_ns.truthy() && (pl::str_eq(&std_ns, "x") || pl::str_eq(&std_ns, "iX"))) {
                        let ns_used = et.get("XMP_NS");
                        if ns_used.get(name.to_bytes()).is_undef() {
                            ns_used.set(name.to_bytes(), attrs_h.get(short_name.to_bytes()));
                        }
                    }
                    attrs_h.delete(short_name.to_bytes()); // (handled by namespace logic)
                    continue;
                } else if recognized_attrs.get(prop_name.to_bytes()).truthy() {
                    continue;
                }
            }
            let short_val = attrs_h.get(short_name.to_bytes());
            // Note: $prop is the containing property in this loop (not the shorthand property)
            // so $ignoreProp ignores all attributes of the ignored property
            if ignore_namespace.get(ns.to_bytes()).truthy() || ignore_prop.get(prop.to_bytes()).truthy() || ignore_et_prop.get(prop_name.to_bytes()).truthy() {
                ignored = prop_name.clone();
                // handle special attributes (extract as tags only once if not empty)
                let ra = recognized_attrs.get(prop_name.to_bytes());
                if ra.is_ref() && short_val.truthy() {
                    let (tbl, id, name) = (ra.at(0), ra.at(1), ra.at(2));
                    let tval = unescape_xml_v(it, short_val.clone(), Value::Undef, Value::Undef)?;
                    let cur = hh(et, "VALUE").get(name.to_bytes());
                    if !(!cur.is_undef() && same_str(&cur, &tval)) {
                        let t = pl::call(it, "Image::ExifTool::GetTagTable", vec![tbl])?;
                        pl::method(it, et, "HandleTag", vec![t, id, tval])?;
                    }
                }
                continue;
            }
            attrs_h.delete(short_name.to_bytes()); // don't re-use this attribute
            prop_list.push(prop_name.clone());
            // save this shorthand XMP property
            if !node_id.is_undef() {
                it.call_named("Image::ExifTool::XMP::SaveBlankInfo", vec![blank_info.clone(), prop_list.clone(), short_val], Want::Void)?;
            } else if is_writing.truthy() {
                it.call_named("Image::ExifTool::XMP::CaptureXMP", vec![et.clone(), prop_list.clone(), short_val], Want::Void)?;
            } else {
                if et.get("XmpValidate").truthy() {
                    it.call_named("Image::ExifTool::XMP::ValidateProperty", vec![et.clone(), prop_list.clone()], Want::Void)?;
                }
                call_code(it, &found_proc, vec![et.clone(), tag_table_ptr.clone(), prop_list.clone(), short_val], Want::Void)?;
            }
            apop(&prop_list);
            shorthand = true;
        }
        if is_writing.truthy() {
            if parse_xmp_element_v(it, et, tag_table_ptr, data_pt, n(val_start), n(val_end), prop_list.clone(), blank_info.clone())? != 0 {
                // (no value since we found more properties within this one)
                // set an error on any ignored attributes here, because they will be lost
                if ignored.truthy() {
                    et.set("XMP_ERROR", cat!("Can't handle XMP attribute '", ignored, "'"));
                }
            } else if !shorthand || val_end != val_start {
                val = substr(&data, val_start, Some(val_end - val_start));
                // remove comments and whitespace from rdf:Description only
                if pl::str_eq(&prop, "rdf:Description") {
                    subst(&rx("<!--.*?-->", ""), &mut val, true, |_| Ok(Vec::new()))?;
                    subst(&rx(r"^\s+", ""), &mut val, false, |_| Ok(Vec::new()))?;
                    subst(&rx(r"\s+$", ""), &mut val, false, |_| Ok(Vec::new()))?;
                }
                if !node_id.is_undef() {
                    it.call_named("Image::ExifTool::XMP::SaveBlankInfo", vec![blank_info.clone(), prop_list.clone(), val.clone(), attrs_h.clone()], Want::Void)?;
                } else {
                    it.call_named("Image::ExifTool::XMP::CaptureXMP", vec![et.clone(), prop_list.clone(), val.clone(), attrs_h.clone()], Want::Void)?;
                }
            }
        } else {
            // look for additional elements contained within this one
            if val_start == val_end || parse_xmp_element_v(it, et, tag_table_ptr, data_pt, n(val_start), n(val_end), prop_list.clone(), blank_info.clone())? == 0 {
                let mut was_empty = false;
                if val.is_undef() {
                    val = substr(&data, val_start, Some(val_end - val_start));
                    // remove comments and whitespace from rdf:Description only
                    if pl::str_eq(&prop, "rdf:Description") && val.truthy() {
                        subst(&rx("<!--.*?-->", ""), &mut val, true, |_| Ok(Vec::new()))?;
                        subst(&rx(r"^\s+", ""), &mut val, false, |_| Ok(Vec::new()))?;
                        subst(&rx(r"\s+$", ""), &mut val, false, |_| Ok(Vec::new()))?;
                    } else if was_comment && val.truthy() {
                        subst(&rx("<!--.*?-->", ""), &mut val, true, |_| Ok(Vec::new()))?;
                    }
                    // if element value is empty, take value from RDF 'value' or 'resource' attribute
                    // (preferentially) or 'about' attribute (if no 'value' or 'resource')
                    if pl::str_eq(&val, "") {
                        let c = m(&rx(r#"\brdf:(?:value|resource)=(['"])(.*?)\1"#, ""), &attrs).or_else(|| m(&rx(r#"\brdf:about=(['"])(.*?)\1"#, ""), &attrs));
                        if let Some(c) = c {
                            val = cap(&c, 2);
                            was_empty = true;
                        }
                    }
                }
                // there are no contained elements, so this must be a simple property value
                // (unless we already extracted shorthand values from this element)
                if pl::length(&val).truthy() || !shorthand {
                    let last_prop = prop_list.at(-1);
                    let last_prop = if last_prop.is_undef() { pl::s("") } else { last_prop };
                    let et_m = m(&rx(r"^et:(desc|prt|val)$", ""), &last_prop);
                    if !node_id.is_undef() {
                        it.call_named("Image::ExifTool::XMP::SaveBlankInfo", vec![blank_info.clone(), prop_list.clone(), val.clone()], Want::Void)?;
                    } else if pl::str_eq(&last_prop, "rdf:type") && was_empty {
                        // do not extract empty structure types (for now)
                    } else if et_m.as_ref().is_some_and(|c| count != 0 || pl::str_eq(&cap(c, 1), "desc")) {
                        // ignore et:desc, and et:val if preceded by et:prt
                        count -= 1;
                    } else {
                        if et.get("XmpValidate").truthy() {
                            it.call_named("Image::ExifTool::XMP::ValidateProperty", vec![et.clone(), prop_list.clone(), attrs_h.clone()], Want::Void)?;
                        }
                        call_code(it, &found_proc, vec![et.clone(), tag_table_ptr.clone(), prop_list.clone(), val.clone(), attrs_h.clone()], Want::Void)?;
                    }
                }
            }
        }
        if !parse_resource {
            apop(&prop_list);
        }
        count += 1;

        // validate namespace prefixes used at this level if necessary
        if et.get("XmpValidate").truthy() {
            let mut keys = used_ns.keys();
            keys.sort();
            for k in keys {
                let edn = hh(et, "definedNS");
                if edn.get(&k).truthy() || &k[..] == b"xml" {
                    continue;
                }
                let kv = Value::from(k.clone());
                if !edn.get(&k).is_undef() {
                    pl::method(it, et, "Warn", vec![cat!("XMP namespace ", kv, " is used out of scope")])?;
                } else {
                    pl::method(it, et, "Warn", vec![cat!("Undefined XMP namespace: ", kv)])?;
                }
                edn.set(&k, n(-1)); // (don't warn again for this namespace)
            }
            // reset namespaces that went out of scope
            let edn = hh(et, "definedNS");
            for k in defined_ns.keys() {
                edn.set(&k, n(0));
            }
            hclear(&used_ns);
            hclear(&defined_ns);
        }

        if start >= end {
            break;
        }
        pos = start as usize;
        // skip white space after closing token
        if let Some(sp) = m_at(&rx(r"\G\s+", ""), &data, pos) {
            pos = end_of(&sp);
        }
    }
    // handle any error which aborted processing
    if err.truthy() {
        pl::method(it, et, if is_writing.truthy() { "Error" } else { "Warn" }, vec![err])?;
    }
    //
    // process resources referenced by blank nodeID's
    //
    if process_blank_info.truthy() && !blank_info.get("Prop").is_empty() {
        it.call_named("Image::ExifTool::XMP::ProcessBlankInfo", vec![et.clone(), tag_table_ptr.clone(), blank_info.clone(), is_writing.clone()], Want::Void)?;
        hclear(&blank_info); // free some memory
    }
    // restore namespace lookup from the containing scope
    if save_ns.truthy() {
        et.set("xlatNS", save_ns);
    }

    Ok(count) // return the number of elements found at this level
}

/// `local $SIG{__WARN__} = $handler`: the previous handler, to restore.
fn local_sig_warn(it: &mut Interp, handler: Value) -> Option<Value> {
    let sig = pkg_hash(it, "main::SIG");
    let old = if sig.exists("__WARN__") { Some(sig.get("__WARN__")) } else { None };
    sig.set("__WARN__", handler);
    old
}

fn restore_sig_warn(it: &mut Interp, old: Option<Value>) {
    let sig = pkg_hash(it, "main::SIG");
    match old {
        Some(v) => sig.set("__WARN__", v),
        None => {
            sig.delete("__WARN__");
        }
    }
}

/// Process XMP data
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
/// Notes: The following flavours of XMP files are currently recognized:
/// - standard XMP with xpacket, x:xmpmeta and rdf:RDF elements
/// - XMP that is missing the xpacket and/or x:xmpmeta elements
/// - mutant Microsoft XMP with xmp:xmpmeta element
/// - XML files beginning with "<xml"
/// - SVG files that begin with "<svg" or "<!DOCTYPE svg"
/// - XMP and XML files beginning with a UTF-8 byte order mark
/// - UTF-8, UTF-16 and UTF-32 encoded XMP
/// - erroneously double-UTF8 encoded XMP
/// - otherwise valid files with leading XML comment
pub fn process_xmp(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(process_xmp_v(it, &arg(&a, 0), &arg(&a, 1), arg(&a, 2))?)
}

pub(crate) fn process_xmp_v(it: &mut Interp, et: &Value, dir_info: &Value, mut tag_table_ptr: Value) -> ER<Value> {
    let mut data_pt = dir_info.get("DataPt");
    let (mut dir_start, mut dir_len): (i64, i64);
    let mut double = Value::Undef;
    // $buff is a variable that $dataPt may point to
    let buff = Rc::new(RefCell::new(Value::Undef));
    let (mut fmt, mut has_xmp, mut is_xml, mut is_rdf, mut is_svg) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    let mut rtn_val = 0;
    let mut bom = 0;
    let path = pl::method(it, et, "MetadataPath", vec![])?;
    let std_path = lex(it, "stdPath");

    // namespaces and prefixes currently in effect while parsing the file,
    // and lookup to translate brain-dead-Microsoft-Photo-software prefixes
    et.set("curURI", Value::new_hash());
    et.set("curNS", Value::new_hash());
    et.set("xlatNS", Value::new_hash());
    et.set("definedNS", Value::new_hash());
    et.delete("XmpAbout");
    et.delete("XmpValidate"); // don't validate by default
    et.delete("XmpValidateLangAlt");

    // ignore non-standard XMP while in strict MWG compatibility mode
    let mwg_strict = it.get_scalar("Image::ExifTool::MWG::strict");
    let opts = hh(et, "OPTIONS");
    if (mwg_strict.truthy() || opts.get("Validate").truthy()) && !(et.get("XMP_CAPTURE").truthy() || et.get("DOC_NUM").truthy()) && (pl::str_eq(&or(dir_info.get("DirName"), pl::s("")), "XMP") || pl::str_eq(&et.get("FILE_TYPE"), "XMP")) {
        if opts.get("Validate").truthy() {
            et.set("XmpValidate", Value::new_hash());
        }
        let sp = std_path.get(et.get("FILE_TYPE").to_bytes());
        let non_std = sp.truthy() && !same_str(&path, &sp);
        if non_std && mwg_strict.truthy() {
            pl::method(it, et, "Warn", vec![cat!("Ignored non-standard XMP at ", path)])?;
            return Ok(n(1));
        }
        if non_std {
            pl::method(it, et, "Warn", vec![cat!("Non-standard XMP at ", path), n(1)])?;
        } else if !dir_info.get("IsExtended").truthy() {
            let dc = et.get_or_new_hash("DIR_COUNT");
            if dc.get("XMP").truthy() {
                pl::method(it, et, "Warn", vec![cat!("Duplicate XMP at ", path)])?;
            }
            dc.set("XMP", pl::add(&or(dc.get("XMP"), n(0)), &n(1))); // count standard XMP
        }
    }
    if data_pt.truthy() {
        let d = data_pt.deref().to_bytes();
        dir_start = or(dir_info.get("DirStart"), n(0)).to_i64();
        dir_len = or(dir_info.get("DirLen"), n(d.len() as i64 - dir_start)).to_i64();
        // check leading BOM (may indicate double-encoded UTF)
        let ds = dir_start.max(0) as usize;
        if let Some(sp) = m_at(&rx(r"\G((\0\0)?\xfe\xff|\xff\xfe(\0\0)?|\xef\xbb\xbf)\0*<\0*\?\0*x\0*p\0*a\0*c\0*k\0*e\0*t", ""), &d, ds) {
            double = caps_of(&d, &sp)[1].clone().unwrap_or_default();
        } else {
            // handle UTF-16/32 XML
            if let Some(sp) = m_at(&rx(r"\G((\0\0)?\xfe\xff|\xff\xfe(\0\0)?|\xef\xbb\xbf)\0*<\0*\?\0*x\0*m\0*l\0* ", ""), &d, ds) {
                let tmp = caps_of(&d, &sp)[1].clone().unwrap_or_default();
                fmt = pl::s(if m(&rx(r"\xfe\xff", ""), &tmp).is_some() { "n" } else { "v" });
                if m(&rx(r"\0\0", ""), &tmp).is_some() {
                    fmt = uc(&fmt);
                }
                is_xml = n(1);
            }
        }
    } else {
        let (mut type_, mut mime) = (Value::Undef, Value::Undef);
        // read information from XMP file
        let raf = dir_info.get("RAF");
        if !raf.truthy() {
            return Ok(n(0));
        }
        let read = |it: &mut Interp, buf: &mut Value, len: Value| -> ER<Value> {
            let mut args = [buf.clone(), len];
            let r = method_aliased(it, &raf, "Read", &mut args)?;
            *buf = args[0].clone();
            Ok(r)
        };
        let mut b = Value::Undef;
        if !read(it, &mut b, n(256))?.truthy() {
            *buff.borrow_mut() = b;
            return Ok(n(0));
        }
        let mut buf2 = b.clone();
        tr(&mut buf2, br"\0", b"", "d"); // cheap conversion to UTF-8
        // remove leading comments if they exist (eg. ImageIngester)
        while m(&rx(r"^\s*<!--", ""), &buf2).is_some() {
            // remove the comment if it is complete
            if subst(&rx(r"^\s*<!--.*?-->\s+", "s"), &mut buf2, false, |_| Ok(Vec::new()))? > 0 {
                // continue with parsing if we have more than 128 bytes remaining
                if buf2.to_bytes().len() > 128 {
                    continue;
                }
            } else {
                // don't read more than 10k when looking for the end of comment
                if buf2.to_bytes().len() > 10000 {
                    return Ok(n(0));
                }
            }
            let mut buf3 = Value::Undef;
            if !read(it, &mut buf3, n(256))?.truthy() {
                break; // read more data if available
            }
            b = pl::concat(&b, &buf3);
            tr(&mut buf3, br"\0", b"", "d");
            buf2 = pl::concat(&buf2, &buf3);
        }
        // check to see if this is XMP format
        // (CS2 writes .XMP files without the "xpacket begin")
        if m(&rx(r"^\s*(<\?xpacket begin=|<x(mp)?:x[ma]pmeta)", ""), &buf2).is_some() {
            has_xmp = n(1);
        } else {
            // also recognize XML files and .XMP files with BOM and without x:xmpmeta
            let c;
            if let Some(x) = m(&rx(r"^(\xfe\xff)(<\?xml|<rdf:RDF|<x(mp)?:x[ma]pmeta)", ""), &buf2) {
                fmt = pl::s("n"); // UTF-16 or 32 MM with BOM
                c = x;
            } else if let Some(x) = m(&rx(r"^(\xff\xfe)(<\?xml|<rdf:RDF|<x(mp)?:x[ma]pmeta)", ""), &buf2) {
                fmt = pl::s("v"); // UTF-16 or 32 II with BOM
                c = x;
            } else if let Some(x) = m(&rx(r"^(\xef\xbb\xbf)?(<\?xml|<rdf:RDF|<x(mp)?:x[ma]pmeta|<svg\b)", ""), &buf2) {
                fmt = n(0); // UTF-8 with BOM or unknown encoding without BOM
                c = x;
            } else if let Some(x) = m(&rx(r"^(\xfe\xff|\xff\xfe|\xef\xbb\xbf)(<\?xpacket begin=)", ""), &buf2) {
                double = cap(&x, 1); // double-encoded UTF
                c = x;
            } else {
                *buff.borrow_mut() = b;
                return Ok(n(0)); // not recognized XMP or XML
            }
            if cap(&c, 1).truthy() {
                bom = 1;
            }
            let c2 = cap(&c, 2);
            if pl::str_eq(&c2, "<?xml") {
                if !fmt.is_undef() && !fmt.truthy() && m(&rx(r"^[^\n\r]*[\n\r]+<\?aid ", "s"), &buf2).is_some() {
                    et.set("XmpValidate", Value::Undef); // don't validate INX
                    if et.get("XMP_CAPTURE").truthy() {
                        pl::method(it, et, "Error", vec![pl::s("ExifTool does not yet support writing of INX files")])?;
                        return Ok(n(0));
                    }
                    type_ = pl::s("INX");
                } else if m(&rx(r"<x(mp)?:x[ma]pmeta", ""), &buf2).is_some() {
                    has_xmp = n(1);
                } else {
                    et.set("XmpValidate", Value::Undef); // don't validate XML
                    // identify SVG images and PLIST files by DOCTYPE if available
                    if let Some(d) = m(&rx(r"<!DOCTYPE\s+(\w+)", ""), &buf2) {
                        let d1 = cap(&d, 1);
                        if pl::str_eq(&d1, "svg") {
                            is_svg = n(1);
                        } else if pl::str_eq(&d1, "plist") {
                            type_ = pl::s("PLIST");
                        } else if pl::str_eq(&d1, "REDXIF") {
                            type_ = pl::s("RMD");
                            mime = pl::s("application/xml");
                        } else if !pl::str_eq(&d1, "fcpxml") {
                            // Final Cut Pro XML
                            *buff.borrow_mut() = b;
                            return Ok(n(0));
                        }
                    } else if m(&rx(r"<svg[\s>]", ""), &buf2).is_some() {
                        is_svg = n(1);
                    } else if m(&rx(r"<rdf:RDF", ""), &buf2).is_some() {
                        is_rdf = n(1);
                    } else if m(&rx(r"<plist[\s>]", ""), &buf2).is_some() {
                        type_ = pl::s("PLIST");
                    }
                }
                is_xml = n(1);
            } else if pl::str_eq(&c2, "<rdf:RDF") {
                is_rdf = n(1); // recognize XMP without x:xmpmeta element
            } else if pl::str_eq(&c2, "<svg") {
                is_svg = n(1);
                is_xml = n(1);
            }
            if is_svg.truthy() && et.get("XMP_CAPTURE").truthy() {
                pl::method(it, et, "Error", vec![pl::s("ExifTool does not yet support writing of SVG images")])?;
                return Ok(n(0));
            }
            if m(&rx(r"^\0\0", ""), &b).is_some() {
                fmt = pl::s("N"); // UTF-32 MM with or without BOM
            } else if m(&rx(r"^..\0\0", "s"), &b).is_some() {
                fmt = pl::s("V"); // UTF-32 II with or without BOM
            } else if !fmt.truthy() {
                if m(&rx(r"^\0", ""), &b).is_some() {
                    fmt = pl::s("n"); // UTF-16 MM without BOM
                } else if m(&rx(r"^.\0", "s"), &b).is_some() {
                    fmt = pl::s("v"); // UTF-16 II without BOM
                }
            }
        }
        let size: i64;
        if type_.truthy() {
            if pl::str_eq(&type_, "PLIST") {
                let ext = et.get("FILE_EXT");
                if ext.truthy() && pl::str_eq(&ext, "MODD") {
                    type_ = ext;
                }
                tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::PLIST::Main")])?;
                let fp = code_ref(it, "Image::ExifTool::PLIST::FoundTag");
                dir_info.get_or_new_hash("XMPParseOpts").set("FoundProc", fp);
            }
        } else if is_svg.truthy() {
            type_ = pl::s("SVG");
        } else if is_xml.truthy() && !has_xmp.truthy() && !is_rdf.truthy() {
            type_ = pl::s("XML");
            let ext = et.get("FILE_EXT");
            if ext.truthy() && pl::str_eq(&ext, "COS") {
                type_ = ext; // recognize COS by extension
            }
        }
        pl::method(it, et, "SetFileType", vec![type_.clone(), mime])?;

        let fast = pl::method(it, et, "Options", vec![pl::s("FastScan")])?;
        if fast.truthy() && pl::num_eq(&fast, &n(3)) {
            *buff.borrow_mut() = b;
            return Ok(n(1));
        }

        if type_.truthy() && pl::str_eq(&type_, "INX") {
            // brute force search for first XMP packet in INX file
            // start: '<![CDATA[<?xpacket begin' (24 bytes)
            // end:   '<?xpacket end="r"?>]]>'   (22 bytes)
            if !pl::method(it, &raf, "Seek", vec![n(0), n(0)])?.truthy() {
                return Ok(n(0));
            }
            if !read(it, &mut b, n(65536))?.truthy() {
                *buff.borrow_mut() = b;
                return Ok(n(1));
            }
            let p;
            loop {
                let bb = b.to_bytes();
                if let Some(sp) = m_at(&rx(r"<!\[CDATA\[<\?xpacket begin", ""), &bb, 0) {
                    p = end_of(&sp) as i64;
                    break;
                }
                let mut buf2 = Value::Undef;
                if !read(it, &mut buf2, n(65536))?.truthy() {
                    *buff.borrow_mut() = b;
                    return Ok(n(1));
                }
                b = pl::concat(&substr(&bb, -24, None), &buf2);
            }
            b = substr(&b.to_bytes(), p - 15, None); // (discard '<![CDATA[' and before)
            let mut sp_from: usize = 0;
            let e;
            loop {
                let bb = b.to_bytes();
                if let Some(sp) = m_at(&rx(r#"<\?xpacket end="[rw]"\?>\]\]>"#, ""), &bb, sp_from) {
                    e = end_of(&sp) as i64;
                    break;
                }
                let nl = bb.len() as i64;
                let mut buf2 = Value::Undef;
                if !read(it, &mut buf2, n(65536))?.truthy() {
                    pl::method(it, et, "Warn", vec![pl::s("Missing xpacket end")])?;
                    *buff.borrow_mut() = b;
                    return Ok(n(1));
                }
                b = pl::concat(&b, &buf2);
                // don't miss end pattern if it was split
                let np = nl - 22;
                let total = b.to_bytes().len() as i64;
                sp_from = if np < 0 { (total + np).max(0) as usize } else { np as usize };
            }
            size = e - 3; // (discard ']]>' and after)
            b = substr(&b.to_bytes(), 0, Some(size));
        } else {
            // read the entire file
            if !pl::method(it, &raf, "Seek", vec![n(0), n(2)])?.truthy() {
                return Ok(n(0));
            }
            let sz = pl::method(it, &raf, "Tell", vec![])?;
            if !sz.truthy() {
                return Ok(n(0));
            }
            size = sz.to_i64();
            if !pl::method(it, &raf, "Seek", vec![n(0), n(0)])?.truthy() {
                return Ok(n(0));
            }
            let r = read(it, &mut b, n(size))?;
            if !pl::num_eq(&r, &n(size)) {
                *buff.borrow_mut() = b;
                return Ok(n(0));
            }
        }
        *buff.borrow_mut() = b;
        data_pt = Value::ScalarRef(buff.clone());
        dir_start = 0;
        dir_len = size;
    }

    // decode the first layer of double-encoded UTF text (if necessary)
    if double.truthy() {
        let d = data_pt.deref().to_bytes();
        *buff.borrow_mut() = substr(&d, dir_start + double.to_bytes().len() as i64, None); // remove leading BOM
        pl::call(it, "Image::ExifTool::SetWarning", vec![Value::Undef])?; // clear old warning
        let handler = code_ref(it, "Image::ExifTool::SetWarning");
        let old = local_sig_warn(it, handler);
        let r = (|| -> ER<()> {
            // assume that character data has been re-encoded in UTF, so re-pack
            // as characters and look for warnings indicating a false assumption
            let bv = buff.borrow().clone();
            let buf2;
            if double.to_bytes()[..] == b"\xef\xbb\xbf"[..] {
                require(it, "Image::ExifTool::Charset")?;
                let uni = pl::call(it, "Image::ExifTool::Charset::Decompose", vec![Value::Undef, bv, pl::s("UTF8")])?;
                buf2 = pack("C*", &uni.items())?;
            } else {
                let db = double.to_bytes();
                let f = if db.len() == 2 {
                    if &db[..] == b"\xfe\xff" {
                        "n"
                    } else {
                        "v"
                    }
                } else if &db[..] == b"\0\0\xfe\xff" {
                    "N"
                } else {
                    "V"
                };
                let mut t = f.as_bytes().to_vec();
                t.push(b'*');
                buf2 = pack("C*", &unpack(&t, &bv.to_bytes())?)?;
            }
            if pl::call(it, "Image::ExifTool::GetWarning", vec![])?.truthy() {
                if !dir_info.get("RAF").truthy() {
                    pl::method(it, et, "Warn", vec![pl::s("Superfluous BOM at start of XMP")])?;
                }
                data_pt = Value::ScalarRef(buff.clone()); // use XMP with the BOM removed
            } else {
                pl::method(it, et, "Warn", vec![pl::s("XMP is double UTF-encoded")])?;
                data_pt = sref(buf2); // use the decoded XMP
            }
            Ok(())
        })();
        restore_sig_warn(it, old);
        r?;
        dir_start = 0;
        dir_len = data_pt.deref().to_bytes().len() as i64;
    }

    // extract XMP/XML as a block if specified
    let block_info = dir_info.get("BlockInfo");
    let block_name = if block_info.truthy() { block_info.get("Name") } else { pl::s("XMP") };
    let block_extract = pl::method(it, et, "Options", vec![pl::s("BlockExtract")])?;
    let lbn = lc(&block_name);
    if (hh(et, "REQ_TAG_LOOKUP").get(lbn.to_bytes()).truthy() || (et.get("TAGS_FROM_FILE").truthy() && !hh(et, "EXCL_TAG_LOOKUP").get(lbn.to_bytes()).truthy()) || block_extract.truthy())
        && ((pl::str_eq(&et.get("FileType"), "XMP") && pl::str_eq(&block_name, "XMP")) || (dir_info.get("DirName").truthy() && same_str(&dir_info.get("DirName"), &block_name)))
    {
        let d = data_pt.deref().to_bytes();
        pl::method(it, et, "FoundTag", vec![or(block_info.clone(), pl::s("XMP")), substr(&d, dir_start, Some(dir_len))])?;
        if block_extract.truthy() && pl::gt(&block_extract, &n(1)) {
            return Ok(n(1));
        }
    }

    if !tag_table_ptr.truthy() {
        tag_table_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::XMP::Main")])?;
    }
    if pl::method(it, et, "Options", vec![pl::s("Verbose")])?.truthy() && !et.get("XMP_CAPTURE").truthy() {
        let dir_type = if is_svg.truthy() { pl::s("SVG") } else { hh(&tag_table_ptr, "GROUPS").get("1") };
        pl::method(it, et, "VerboseDir", vec![dir_type, n(0), n(dir_len)])?;
    }
    //
    // convert UTF-16 or UTF-32 encoded XMP to UTF-8 if necessary
    //
    let mut begin: Vec<u8> = b"<?xpacket begin=".to_vec();
    let mut dir_end = dir_start + dir_len;
    let d = data_pt.deref().to_bytes();
    let ds = dir_start.max(0) as usize;
    et.delete("XMP_IS_XML");
    et.delete("XMP_IS_SVG");
    let gq = |b: &[u8], tail: &[u8]| -> Vec<u8> {
        let mut p = br"\G".to_vec();
        p.extend_from_slice(b);
        p.extend_from_slice(tail);
        p
    };
    if is_xml.truthy() || is_rdf.truthy() {
        et.set("XMP_IS_XML", is_xml.clone());
        et.set("XMP_IS_SVG", is_svg.clone());
        et.set("XMP_NO_XPACKET", n(1 + bom));
    } else if m_at(&*rx_dyn(&gq(&pre::quotemeta(&begin), b""), "")?, &d, ds).is_some() {
        et.delete("XMP_NO_XPACKET");
    } else if let Some(p) = m_at(&rx(r"<x(mp)?:x[ma]pmeta", ""), &d, ds).map(|sp| end_of(&sp) as i64).filter(|&p| p > dir_start && p < dir_end) {
        let _ = p;
        et.set("XMP_NO_XPACKET", n(1 + bom));
    } else {
        et.delete("XMP_NO_XPACKET");
        // check for UTF-16 encoding (insert one \0 between characters)
        begin = begin.iter().map(|&c| vec![c]).collect::<Vec<_>>().join(&0u8);
        // must reset pos because it was killed by previous unsuccessful //g match
        let mut bad_enc = false;
        let mut p16 = br"(\0)?".to_vec();
        p16.extend_from_slice(&pre::quotemeta(&begin));
        if let Some(sp) = m_at(&*rx_dyn(&gq(&p16, br"\0."), "s")?, &d, ds) {
            let p = end_of(&sp);
            // validate byte ordering by checking for U+FEFF character
            if caps_of(&d, &sp)[1].clone().unwrap_or_default().truthy() {
                // should be big-endian since we had a leading \0
                fmt = pl::s("n");
                bad_enc = m_at(&rx(r"\G\xfe\xff", ""), &d, p).is_none();
            } else {
                fmt = pl::s("v");
                bad_enc = m_at(&rx(r"\G\0\xff\xfe", ""), &d, p).is_none();
            }
        } else {
            // check for UTF-32 encoding (with three \0's between characters)
            begin = begin.iter().flat_map(|&c| if c == 0 { vec![0, 0, 0] } else { vec![c] }).collect();
            let mut p32 = br"(\0\0\0)?".to_vec();
            p32.extend_from_slice(&pre::quotemeta(&begin));
            match m_at(&*rx_dyn(&gq(&p32, br"\0\0\0."), "s")?, &d, ds) {
                None => fmt = n(0), // set format to zero as indication we didn't find encoded XMP
                Some(sp) => {
                    let p = end_of(&sp);
                    if caps_of(&d, &sp)[1].clone().unwrap_or_default().truthy() {
                        // should be big-endian
                        fmt = pl::s("N");
                        bad_enc = m_at(&rx(r"\G\0\0\xfe\xff", ""), &d, p).is_none();
                    } else {
                        fmt = pl::s("V");
                        bad_enc = m_at(&rx(r"\G\0\0\0\xff\xfe\0\0", ""), &d, p).is_none();
                    }
                }
            }
        }
        if bad_enc {
            pl::method(it, et, "Warn", vec![pl::s("Invalid XMP encoding marker")])?;
        }
    }
    // warn if standard XMP is missing xpacket wrapper
    let sp = std_path.get(et.get("FILE_TYPE").to_bytes());
    if et.get("XMP_NO_XPACKET").truthy() && opts.get("Validate").truthy() && sp.truthy() && same_str(&path, &sp) && !dir_info.get("IsExtended").truthy() && !et.get("DOC_NUM").truthy() {
        pl::method(it, et, "Warn", vec![pl::s("XMP is missing xpacket wrapper"), n(1)])?;
    }
    if fmt.truthy() {
        // trim if necessary to avoid converting non-UTF data
        let d = data_pt.deref().to_bytes();
        if dir_start != 0 || dir_end != d.len() as i64 {
            *buff.borrow_mut() = substr(&d, dir_start, Some(dir_len));
            data_pt = Value::ScalarRef(buff.clone());
        }
        // convert into UTF-8
        let mut t = fmt.to_bytes().to_vec();
        t.push(b'*');
        let src = data_pt.deref().to_bytes();
        *buff.borrow_mut() = pack("C0U*", &unpack(&t, &src)?)?;
        data_pt = Value::ScalarRef(buff.clone());
        dir_start = 0;
        dir_len = data_pt.deref().to_bytes().len() as i64;
        dir_end = dir_start + dir_len;
    }
    // avoid scanning for XMP later in case ScanForXMP is set
    if tag_table_ptr.same_ref(&pkg_hash(it, "Image::ExifTool::XMP::Main")) {
        et.set("FoundXMP", n(1));
    }

    // set XMP parsing options
    et.set("XMPParseOpts", dir_info.get("XMPParseOpts"));

    // ignore any specified properties (XML hack)
    let ignore_prop = lex(it, "ignoreProp");
    if dir_info.get("IgnoreProp").truthy() {
        let src = dir_info.get("IgnoreProp");
        let pairs: Vec<(Rc<[u8]>, Value)> = src.keys().into_iter().map(|k| (k.clone(), src.get(&k))).collect();
        hclear(&ignore_prop);
        for (k, v) in pairs {
            ignore_prop.set(&k, v);
        }
    } else {
        hclear(&ignore_prop);
    }

    // need to preserve list indices to be able to handle multi-dimensional lists
    let mut keep_flat = Value::Undef;
    if opts.get("Struct").truthy() {
        if pl::str_eq(&opts.get("Struct"), "2") {
            keep_flat = n(1); // preserve flattened tags
            // setting NO_LIST to 0 combines list items in a TAG_EXTRA "NoList" element
            // to allow them to be re-listed later if necessary.  A "NoListDel" element
            // is also created for tags that wouldn't have existed.
            et.set("NO_LIST", n(0));
        } else {
            et.set("NO_LIST", n(1));
        }
    }

    // don't generate structures if this isn't real XMP
    if dir_info.get("BlockInfo").truthy() || dir_info.get("NoStruct").truthy() {
        et.set("NO_STRUCT", n(1));
    }

    // parse the XMP
    if parse_xmp_element_v(it, et, &tag_table_ptr, &data_pt, n(dir_start), n(dir_end), Value::Undef, Value::Undef)? != 0 {
        rtn_val = 1;
    } else if dir_info.get("DirName").truthy() && pl::str_eq(&dir_info.get("DirName"), "XMP") {
        // if DirName was 'XMP' we expect well-formed XMP, so set Warning since it wasn't
        // (but allow empty XMP as written by some PhaseOne cameras)
        let xmp = substr(&data_pt.deref().to_bytes(), dir_start, Some(dir_len));
        if m(&rx(r"^ *\0*$", ""), &xmp).is_some() {
            pl::method(it, et, "Warn", vec![pl::s("Invalid XMP")])?;
        } else {
            pl::method(it, et, "Warn", vec![pl::s("Empty XMP"), n(1)])?;
            rtn_val = 1;
        }
    }
    et.delete("NO_STRUCT");

    // return DataPt if successful in case we want it for writing
    if rtn_val != 0 && dir_info.get("RAF").truthy() {
        dir_info.set("DataPt", data_pt.clone());
    }

    // restore structures if necessary
    if et.get("IsStruct").truthy() {
        if !dir_info.get("NoStruct").truthy() {
            require(it, "Image/ExifTool/XMPStruct.pl")?;
            it.call_named("Image::ExifTool::XMP::RestoreStruct", vec![et.clone(), keep_flat], Want::Void)?;
        }
        et.delete("IsStruct");
    }
    // reset NO_LIST flag (must do this _after_ RestoreStruct() above)
    et.delete("NO_LIST");
    et.delete("XMPParseOpts");
    et.delete("curURI");
    et.delete("curNS");
    et.delete("xlatNS");
    et.delete("definedNS");

    Ok(n(rtn_val))
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::XMP::AUTOLOAD", autoload);
    it.define_native("Image::ExifTool::XMP::EscapeXML", escape_xml);
    it.define_native("Image::ExifTool::XMP::UnescapeXML", unescape_xml);
    it.define_native("Image::ExifTool::XMP::FullEscapeXML", full_escape_xml);
    it.define_native("Image::ExifTool::XMP::FullUnescapeXML", full_unescape_xml);
    it.define_native("Image::ExifTool::XMP::UnescapeChar", unescape_char);
    it.define_native("Image::ExifTool::XMP::FixUTF8", fix_utf8);
    it.define_native("Image::ExifTool::XMP::DecodeBase64", decode_base64);
    it.define_native("Image::ExifTool::XMP::GetXMPTagID", get_xmp_tag_id);
    it.define_native("Image::ExifTool::XMP::RegisterNamespace", register_namespace);
    it.define_native("Image::ExifTool::XMP::GetLangInfo", get_lang_info);
    it.define_native("Image::ExifTool::XMP::StandardLangCase", standard_lang_case);
    it.define_native("Image::ExifTool::XMP::ConvertXMPDate", convert_xmp_date);
    it.define_native("Image::ExifTool::XMP::ConvertRational", convert_rational);
    it.define_native("Image::ExifTool::XMP::ConvertRationalList", convert_rational_list);
    it.define_native("Image::ExifTool::XMP::AddFlattenedTags", add_flattened_tags);
    it.define_native("Image::ExifTool::XMP::ScanForXMP", scan_for_xmp);
    it.define_native("Image::ExifTool::XMP::PrintLensID", print_lens_id);
    it.define_native("Image::ExifTool::XMP::FoundXMP", found_xmp);
    it.define_native("Image::ExifTool::XMP::ParseXMPElement", parse_xmp_element);
    it.define_native("Image::ExifTool::XMP::ProcessXMP", process_xmp);
}
