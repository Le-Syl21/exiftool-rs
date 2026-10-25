//! Perl idioms for ported code.
//!
//! A ported sub keeps the Perl it comes from line for line; these helpers
//! give each Perl operation its exact semantics on [`Value`]s, so the port
//! never approximates them with Rust's own (string/number duality, `%.15g`
//! stringification, numeric strings, regexes on bytes, `split`'s rules).
//!
//! | Perl | here |
//! |---|---|
//! | `$a + $b`, `-`, `*` | `add`, `sub`, `mul` |
//! | `$a / $b` | `div` (dies on zero, as perl) |
//! | `-$x` | `neg` |
//! | `$a < $b`, `==`... | `lt`, `le`, `gt`, `ge`, `num_eq` |
//! | `$a eq $b`, `$a . $b` | `str_eq`, `concat` |
//! | `int($x)`, `abs($x)` | `int`, `abs` |
//! | `sprintf(FMT, LIST)` | `sprintf` |
//! | `$s =~ /re/` | `re(pat, flags)` then `Re::m` / `Re::all` / `Re::subst` |
//! | `split ' ', $s` / `split /re/, $s` | `split_ws` / `split` |
//! | `$obj->Method(...)` | `method` (scalar) / `method_list` |
//! | `Pkg::Func(...)` | `call` / `call_list` |
//! | `$$h{k}`, `$$a[i]` | `Value::get`, `Value::at` (see `perl::hv`) |

use std::rc::Rc;

use crate::interp::eval::{die, ER};
use crate::interp::{Interp, Want};
use crate::perl::regex::{self as pre, PerlRegex};
use crate::perl::{ops, Value};

pub use crate::perl::Value as V;

/// A string value.
pub fn s(x: &str) -> Value {
    Value::str(x)
}

pub fn add(a: &Value, b: &Value) -> Value {
    ops::add(a, b)
}

pub fn sub(a: &Value, b: &Value) -> Value {
    ops::sub(a, b)
}

pub fn mul(a: &Value, b: &Value) -> Value {
    ops::mul(a, b)
}

/// `$a / $b`: fatal "Illegal division by zero", as in perl.
pub fn div(a: &Value, b: &Value) -> ER<Value> {
    match ops::div(a, b) {
        Ok(v) => Ok(v),
        Err(e) => die(e),
    }
}

/// `-$x`, including perl's string negation.
pub fn neg(a: &Value) -> Value {
    ops::negate(a)
}

pub fn int(a: &Value) -> Value {
    ops::int(a)
}

pub fn abs(a: &Value) -> Value {
    ops::abs(a)
}

pub fn lt(a: &Value, b: &Value) -> bool {
    ops::num_cmp(a, b) == Some(std::cmp::Ordering::Less)
}

pub fn le(a: &Value, b: &Value) -> bool {
    matches!(ops::num_cmp(a, b), Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal))
}

pub fn gt(a: &Value, b: &Value) -> bool {
    ops::num_cmp(a, b) == Some(std::cmp::Ordering::Greater)
}

pub fn ge(a: &Value, b: &Value) -> bool {
    matches!(ops::num_cmp(a, b), Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal))
}

pub fn num_eq(a: &Value, b: &Value) -> bool {
    ops::num_cmp(a, b) == Some(std::cmp::Ordering::Equal)
}

pub fn str_eq(a: &Value, b: &str) -> bool {
    &a.to_bytes()[..] == b.as_bytes()
}

/// `$a . $b`
pub fn concat(a: &Value, b: &Value) -> Value {
    let mut v = a.to_bytes().to_vec();
    v.extend_from_slice(&b.to_bytes());
    Value::bytes(v)
}

/// `length $x` (undef stays undef).
pub fn length(a: &Value) -> Value {
    if a.is_undef() {
        Value::Undef
    } else {
        Value::Int(a.to_bytes().len() as i64)
    }
}

/// `sprintf(FMT, LIST)`
pub fn sprintf(fmt: &str, args: &[Value]) -> Value {
    Value::bytes(crate::perl::sprintf::sprintf(fmt.as_bytes(), args))
}

/// `sprintf` with a format held in a value.
pub fn sprintf_v(fmt: &Value, args: &[Value]) -> Value {
    Value::bytes(crate::perl::sprintf::sprintf(&fmt.to_bytes(), args))
}

/// A compiled pattern (cached), for `=~`.
pub struct Re(Rc<PerlRegex>);

/// `qr/PAT/FLAGS`: dies like perl on a bad pattern.
pub fn re(pat: &str, flags: &str) -> Re {
    Re(pre::compile(pat.as_bytes(), flags).unwrap_or_else(|e| panic!("bad pattern /{pat}/: {e}")))
}

/// A pattern built at run time (interpolated); errors are perl's die.
pub fn re_dyn(pat: &[u8], flags: &str) -> ER<Re> {
    match pre::compile(pat, flags) {
        Ok(r) => Ok(Re(r)),
        Err(e) => die(e),
    }
}

/// The captures of a match: `[0]` is `$&`, then `$1`...; `None` for a group
/// that did not take part (undef).
pub type Caps = Vec<Option<Value>>;

fn caps(subject: &[u8], spans: &[Option<(usize, usize)>]) -> Caps {
    spans.iter().map(|g| g.map(|(a, b)| Value::bytes(&subject[a..b]))).collect()
}

/// A capture as a value (undef when the group did not match).
pub fn cap(c: &Caps, i: usize) -> Value {
    c.get(i).cloned().flatten().unwrap_or_default()
}

impl Re {
    /// `$s =~ /re/`: the captures of the first match.
    pub fn m(&self, subject: &Value) -> Option<Caps> {
        let b = subject.to_bytes();
        match self.0.match_at(&b, 0) {
            Ok(Some(sp)) => Some(caps(&b, &sp)),
            _ => None,
        }
    }

    pub fn is_match(&self, subject: &Value) -> bool {
        self.m(subject).is_some()
    }

    /// `($s =~ /re/g)` in list context: every match's captures (or the whole
    /// match when there are no groups), flattened, as perl returns them.
    pub fn all(&self, subject: &Value) -> Vec<Value> {
        let b = subject.to_bytes();
        let mut out = Vec::new();
        if let Ok(all) = self.0.match_all(&b, 0) {
            for sp in all {
                if sp.len() == 1 {
                    let (s, e) = sp[0].unwrap();
                    out.push(Value::bytes(&b[s..e]));
                } else {
                    for g in &sp[1..] {
                        out.push(match g {
                            Some((s, e)) => Value::bytes(&b[*s..*e]),
                            None => Value::Undef,
                        });
                    }
                }
            }
        }
        out
    }

    /// Every match with its captures (`while ($s =~ /re/g) { ... }`).
    pub fn each(&self, subject: &Value) -> Vec<Caps> {
        let b = subject.to_bytes();
        match self.0.match_all(&b, 0) {
            Ok(all) => all.iter().map(|sp| caps(&b, sp)).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// `$s =~ s/re/REPL/` (or `/g`): the replacement is computed from each
    /// match's captures. Returns the number of substitutions; `$s` is
    /// updated only when there was one.
    pub fn subst(&self, target: &mut Value, global: bool, mut repl: impl FnMut(&Caps) -> Vec<u8>) -> usize {
        let b = target.to_bytes();
        let mut out = Vec::with_capacity(b.len());
        let (mut last, mut start, mut n) = (0, 0, 0);
        while start <= b.len() {
            let Ok(Some(sp)) = self.0.match_at(&b, start) else { break };
            let (s, e) = sp[0].unwrap();
            out.extend_from_slice(&b[last..s]);
            out.extend_from_slice(&repl(&caps(&b, &sp)));
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
        n
    }
}

/// `split ' ', $s`
pub fn split_ws(subject: &Value) -> Vec<Value> {
    let re = pre::compile(b"\\s+", "").unwrap();
    pre::split(&re, true, &subject.to_bytes(), 0).unwrap_or_default()
}

/// `split /re/, $s, $limit` (limit 0 for none).
pub fn split(re: &Re, subject: &Value, limit: i64) -> Vec<Value> {
    pre::split(&re.0, false, &subject.to_bytes(), limit).unwrap_or_default()
}

/// `$obj->Method(ARGS)` in scalar context (the sub may be ported or still
/// interpreted).
pub fn method(it: &mut Interp, obj: &Value, name: &str, args: Vec<Value>) -> ER<Value> {
    let mut a = vec![obj.clone()];
    a.extend(args);
    Ok(it.call_method(obj, name, a, Want::Scalar)?.pop().unwrap_or_default())
}

/// `$obj->Method(ARGS)` in list context.
pub fn method_list(it: &mut Interp, obj: &Value, name: &str, args: Vec<Value>) -> ER<Vec<Value>> {
    let mut a = vec![obj.clone()];
    a.extend(args);
    it.call_method(obj, name, a, Want::List)
}

/// `Pkg::Func(ARGS)` in scalar context, by its full name.
pub fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    Ok(it.call_named(name, args, Want::Scalar)?.pop().unwrap_or_default())
}

/// `Pkg::Func(ARGS)` in list context.
pub fn call_list(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Vec<Value>> {
    it.call_named(name, args, Want::List)
}

/// `warn LIST`: through `$SIG{__WARN__}` as perl does (ExifTool catches
/// its warnings that way). Add the "\n" yourself, as in the Perl.
pub fn warn(it: &mut Interp, msg: &str) -> ER<()> {
    let mut m = msg.to_string();
    if !m.ends_with('\n') {
        m.push_str(" at (eval) line 1.\n");
    }
    it.warn(Value::from(m))
}

/// Cells holding the arguments, and their aliases, for a call whose callee
/// writes `$_[i]`.
fn alias_cells(args: &[Value]) -> (Vec<crate::interp::eval::Cell>, Vec<Option<crate::interp::eval::Lv>>) {
    let cells: Vec<_> = args.iter().map(|v| std::rc::Rc::new(std::cell::RefCell::new(v.clone()))).collect();
    let al = cells.iter().map(|c| Some(crate::interp::eval::Lv::Scalar(c.clone()))).collect();
    (cells, al)
}

/// `Pkg::Func($a, $b)` where the sub writes its arguments (`$_[0] = ...`):
/// the slice gets the new values back, as perl's aliasing gives them.
pub fn call_mut(it: &mut Interp, name: &str, args: &mut [Value], want: Want) -> ER<Vec<Value>> {
    let (cells, al) = alias_cells(args);
    let code = it.sub_ref(name);
    let r = it.call_with_aliases(&code, args.to_vec(), al, want);
    for (a, c) in args.iter_mut().zip(cells) {
        *a = c.borrow().clone();
    }
    r
}

/// `$obj->Method($a, $b)` where the method writes its arguments
/// (`$raf->Read($buff, $len)`): the slice gets the new values back. The
/// object itself is not aliased.
pub fn method_mut(it: &mut Interp, obj: &Value, name: &str, args: &mut [Value], want: Want) -> ER<Vec<Value>> {
    let (cells, mut al) = alias_cells(args);
    let mut a = vec![obj.clone()];
    a.extend(args.iter().cloned());
    al.insert(0, None);
    it.set_pending_aliases(al);
    let r = it.call_method(obj, name, a, want);
    it.set_pending_aliases_none();
    for (x, c) in args.iter_mut().zip(cells) {
        *x = c.borrow().clone();
    }
    r
}

/// `substr(STR, OFF[, LEN])` (undef outside the string, as perl, without
/// the warning: use the builtin through Perl when the warning matters).
pub fn substr(s: &Value, off: i64, len: Option<i64>) -> Value {
    let b = s.to_bytes();
    match crate::interp::eval::substr_range(b.len(), off, len) {
        Some((a, e)) => Value::bytes(&b[a..e]),
        None => Value::Undef,
    }
}

/// `$a % $b` (dies on zero, as perl).
pub fn modulo(a: &Value, b: &Value) -> ER<Value> {
    match ops::modulo(a, b) {
        Ok(v) => Ok(v),
        Err(e) => die(e),
    }
}

/// The argument `i` of a ported sub (`$_[i]`), undef when missing.
pub fn arg(a: &[Value], i: usize) -> Value {
    a.get(i).cloned().unwrap_or_default()
}

/// Return a scalar from a ported sub.
pub fn ret(v: impl Into<Value>) -> ER<Vec<Value>> {
    Ok(vec![v.into()])
}

/// `return undef`
pub fn ret_undef() -> ER<Vec<Value>> {
    Ok(vec![Value::Undef])
}

/// `return;`: an empty list, or undef in scalar context.
pub fn ret_nothing(want: Want) -> ER<Vec<Value>> {
    Ok(if want == Want::List { vec![] } else { vec![Value::Undef] })
}
