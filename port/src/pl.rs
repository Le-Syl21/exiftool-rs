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
        m.push_str(&crate::interp::eval::location_suffix());
    }
    it.warn(Value::from(m))
}

/// perl's "Use of uninitialized value NAME in OP" warning (with -w or
/// `use warnings`) where a native uses `v` as a string or a number and it
/// may be undef: `name` is how perl names the value (`"$lensModel"`, or ""
/// when perl does not name it), `op` perl's description of the operator
/// (`"pattern match (m//)"`, `"addition (+)"`...).
pub fn warn_uninit(it: &mut Interp, v: &Value, name: &str, op: &str) -> ER<()> {
    if v.is_undef() && it.warn_on() {
        it.report_uninit(if name.is_empty() { None } else { Some(name.to_string()) }, op)?;
    }
    Ok(())
}

/// perl's warnings where a native uses `v` as a number: "Argument \"X\"
/// isn't numeric in OP" for a string that is not a number, or the
/// uninitialized-value warning (see `warn_uninit`) for undef.
pub fn warn_num(it: &mut Interp, v: &Value, name: &str, op: &str) -> ER<()> {
    match v {
        Value::Undef => warn_uninit(it, v, name, op),
        Value::Str(s) if !crate::interp::eval::warnings::numeric_string(s) && it.warn_on() => it.report_not_numeric(s, op),
        _ => Ok(()),
    }
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
    // Straight to the named sub: the late-bound reference of sub_ref is a
    // trampoline whose own frame would take the aliases.
    it.set_pending_aliases(al);
    let r = it.call_named(name, args.to_vec(), want);
    it.set_pending_aliases_none();
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

// ---- shared by the pilot ports (each had written its own) ----

/// An integer value: `5`.
pub fn n(i: i64) -> Value {
    Value::Int(i)
}

/// `join(SEP, LIST)`
pub fn join(sep: &str, items: &[Value]) -> Value {
    let parts: Vec<Vec<u8>> = items.iter().map(|v| v.to_bytes().to_vec()).collect();
    Value::bytes(parts.join(sep.as_bytes()))
}

/// `lc`, `uc`, `ucfirst`, `lcfirst` (ASCII, as perl on byte strings).
pub fn lc(v: &Value) -> Value {
    Value::bytes(v.to_bytes().to_ascii_lowercase())
}

pub fn uc(v: &Value) -> Value {
    Value::bytes(v.to_bytes().to_ascii_uppercase())
}

pub fn ucfirst(v: &Value) -> Value {
    let mut b = v.to_bytes().to_vec();
    if let Some(f) = b.first_mut() {
        *f = f.to_ascii_uppercase();
    }
    Value::bytes(b)
}

pub fn lcfirst(v: &Value) -> Value {
    let mut b = v.to_bytes().to_vec();
    if let Some(f) = b.first_mut() {
        *f = f.to_ascii_lowercase();
    }
    Value::bytes(b)
}

/// `$s =~ tr/SEARCH/REPL/FLAGS`: updates `s` (unless /r) and returns the
/// count, or the new string with /r.
pub fn tr(s: &mut Value, search: &str, repl: &str, flags: &str) -> Value {
    let (out, count) = crate::perl::tr::tr(&s.to_bytes(), search.as_bytes(), repl.as_bytes(), flags);
    if flags.contains('r') {
        return Value::bytes(out);
    }
    *s = Value::bytes(out);
    Value::Int(count as i64)
}

/// `unpack(TEMPLATE, DATA)` in list context (perl's error is a die).
pub fn unpack(t: &str, data: &Value) -> ER<Vec<Value>> {
    match crate::perl::pack::unpack(t.as_bytes(), &data.to_bytes()) {
        Ok(v) => Ok(v),
        Err(e) => die(e),
    }
}

/// `unpack(TEMPLATE, DATA)` with its warnings (malformed UTF-8 in C0 mode)
/// issued through $SIG{__WARN__}, as perl does.
pub fn unpack_warn(it: &mut Interp, t: &str, data: &Value) -> ER<Vec<Value>> {
    let v = unpack(t, data)?;
    for w in crate::perl::pack::take_warnings() {
        warn(it, &w)?;
    }
    Ok(v)
}

/// `unpack` in scalar context: the first value.
pub fn unpack1(t: &str, data: &Value) -> ER<Value> {
    Ok(unpack(t, data)?.into_iter().next().unwrap_or_default())
}

/// `pack(TEMPLATE, LIST)`
pub fn pack(t: &str, args: &[Value]) -> ER<Value> {
    match crate::perl::pack::pack(t.as_bytes(), args) {
        Ok(b) => Ok(Value::bytes(b)),
        Err(e) => die(e),
    }
}

/// `ref $v` as perl gives it: the class of a blessed reference.
pub fn ref_of(v: &Value) -> String {
    match v {
        Value::HashRef(h) => match &h.borrow().blessed {
            Some(c) => c.to_string(),
            None => "HASH".into(),
        },
        Value::Handle(h) => match &h.borrow().class {
            Some(c) => c.to_string(),
            None => "GLOB".into(),
        },
        v => v.ref_type().to_string(),
    }
}

/// `%Pkg::name`, `@Pkg::name`, `$Pkg::name` (package variables).
pub fn pkg_hash(it: &mut Interp, name: &str) -> Value {
    match it.global(b'%', name) {
        crate::interp::Binding::Hash(h) => Value::HashRef(h),
        _ => unreachable!(),
    }
}

pub fn pkg_array(it: &mut Interp, name: &str) -> Value {
    match it.global(b'@', name) {
        crate::interp::Binding::Array(a) => Value::ArrayRef(a),
        _ => unreachable!(),
    }
}

/// `\$v`: a reference to a new scalar holding v.
pub fn sref(v: Value) -> Value {
    Value::ScalarRef(Rc::new(std::cell::RefCell::new(v)))
}

/// `\$$h{key}`: a reference to the element itself.
pub fn elem_ref(h: &Value, key: &[u8]) -> Value {
    match h {
        Value::HashRef(rc) => Value::LvRef(Rc::new(crate::interp::eval::Lv::HElem(rc.clone(), Rc::from(key)))),
        _ => sref(Value::Undef),
    }
}

/// `$$ref = v` for a SCALAR reference (or an element reference).
pub fn set_deref(r: &Value, v: Value) -> ER<()> {
    match r {
        Value::ScalarRef(c) => *c.borrow_mut() = v,
        Value::LvRef(l) => l.set(v)?,
        _ => return die("Not a SCALAR reference"),
    }
    Ok(())
}

/// Call a code value (or a sub named by a string, as `&$name`).
pub fn call_code(it: &mut Interp, code: &Value, args: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    match code {
        Value::Code(c) => it.call(c, args, want),
        _ => {
            let name = code.to_text();
            it.call_named(&name, args, want)
        }
    }
}

/// `local $_;` around `f`: a new `$_` (restored on every exit, errors too).
pub fn local_topic<T>(it: &mut Interp, f: impl FnOnce(&mut Interp) -> ER<T>) -> ER<T> {
    local_global(it, b'$', "main::_", f)
}

/// `local $Pkg::x` / `local @x` / `local %x` around `f`.
pub fn local_global<T>(it: &mut Interp, sigil: u8, name: &str, f: impl FnOnce(&mut Interp) -> ER<T>) -> ER<T> {
    use crate::interp::Binding;
    let old = it.global(sigil, name);
    let fresh = match sigil {
        b'$' => Binding::Scalar(Rc::new(std::cell::RefCell::new(Value::Undef))),
        b'@' => Binding::Array(Rc::new(std::cell::RefCell::new(Vec::new()))),
        _ => Binding::Hash(Rc::new(std::cell::RefCell::new(Default::default()))),
    };
    it.set_global(sigil, name, fresh);
    let r = f(it);
    it.set_global(sigil, name, old);
    r
}

/// `local $SIG{__WARN__} = HANDLER` around `f` (`\&SetWarning` is
/// `it.sub_ref("Image::ExifTool::SetWarning")` as a Value::Code).
pub fn local_sig_warn<T>(it: &mut Interp, handler: Value, f: impl FnOnce(&mut Interp) -> ER<T>) -> ER<T> {
    let sig = pkg_hash(it, "main::SIG");
    let old = sig.exists("__WARN__").then(|| sig.get("__WARN__"));
    sig.set("__WARN__", handler);
    let r = f(it);
    match old {
        Some(v) => sig.set("__WARN__", v),
        None => {
            sig.delete("__WARN__");
        }
    }
    r
}

/// `eval { ... }`: runs `f`; a die becomes `$@` and `None`, success clears
/// `$@`. Other flow (exit, return) passes through.
pub fn eval_block<T>(it: &mut Interp, f: impl FnOnce(&mut Interp) -> ER<T>) -> ER<Option<T>> {
    match f(it) {
        Ok(v) => {
            it.set_scalar("main::@", s(""));
            Ok(Some(v))
        }
        Err(crate::interp::Flow::Die(e)) => {
            it.set_scalar("main::@", e);
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// An `eval $string` site: the code runs in `package` with `vars` as the
/// lexicals in scope (`$val`, `$self`... named as at the Perl eval site;
/// a leading `@`/`%` makes an array or hash). Returns the result (undef on
/// error, with `$@` set as perl's eval does) and the variables afterwards,
/// for the ones the code may change.
pub fn eval_site(it: &mut Interp, code: &Value, package: &str, vars: &[(&str, Value)], want: Want) -> (Value, Vec<Value>) {
    eval_site_in(it, code, package, None, vars, want)
}

/// `eval_site` with an outer scope, usually `runtime::file_scope(it,
/// MODULE)` so the code also sees the file's `my` variables.
pub fn eval_site_in(it: &mut Interp, code: &Value, package: &str, outer: Option<Rc<crate::interp::Scope>>, vars: &[(&str, Value)], want: Want) -> (Value, Vec<Value>) {
    let scope = crate::interp::Scope::new(outer);
    enum Slot {
        S(crate::interp::eval::Cell),
        A(crate::interp::eval::ArrayRc),
        H(crate::interp::eval::HashRc),
    }
    let slots: Vec<Slot> = vars
        .iter()
        .map(|(name, v)| {
            if let Some(n) = name.strip_prefix('@') {
                Slot::A(scope.array(n, v.items()))
            } else if let Some(n) = name.strip_prefix('%') {
                let h = match v {
                    Value::HashRef(h) => h.borrow().clone(),
                    _ => Default::default(),
                };
                Slot::H(scope.hash(n, h))
            } else {
                Slot::S(scope.scalar(name.trim_start_matches('$'), v.clone()))
            }
        })
        .collect();
    let r = it.eval_string(&code.to_bytes(), package, &scope, want);
    let v = match r {
        Ok(v) => v.last().cloned().unwrap_or_default(),
        Err(_) => Value::Undef,
    };
    let after = slots
        .iter()
        .map(|s| match s {
            Slot::S(c) => c.borrow().clone(),
            Slot::A(a) => Value::ArrayRef(a.clone()),
            Slot::H(h) => Value::HashRef(h.clone()),
        })
        .collect();
    (v, after)
}

/// `print $fh LIST` (or STDOUT when `fh` is None), with `$,` and `$\`.
pub fn print_to(it: &mut Interp, fh: Option<&Value>, items: &[Value]) -> ER<bool> {
    let scope = crate::interp::Scope::new(None);
    scope.array("args", items.to_vec());
    let code: &[u8] = match fh {
        Some(o) => {
            scope.scalar("fh", o.clone());
            b"print $fh @args"
        }
        None => b"print @args",
    };
    let saved = it.get_scalar("main::@");
    let r = it.eval_string(code, "main", &scope, Want::Scalar);
    it.set_scalar("main::@", saved);
    match r {
        Ok(v) => Ok(v.last().is_some_and(|v| v.truthy())),
        Err(e) => Err(crate::interp::Flow::Die(e)),
    }
}

/// `pl::at(N)`: the native is now at line N of its Perl sub (for the
/// location of die/warn messages, "at FILE line N.", as perl's).
pub fn at(line: u32) {
    crate::interp::eval::set_line(line);
}

/// The text perl puts in its messages for a string used as a reference
/// under `strict refs` (ExifTool's modules use strict).
fn symref_die<T>(s: &Value, what: &str) -> ER<T> {
    let b = s.to_bytes();
    let mut m = String::from("Can't use string (\"");
    m.push_str(&String::from_utf8_lossy(&b[..b.len().min(32)]));
    m.push('"');
    if b.len() > 32 {
        m.push_str("...");
    }
    m.push_str(&format!(") as {what} ref while \"strict refs\" in use"));
    crate::interp::eval::die(m)
}

/// `$$r` read as perl does under strict refs: undef dies "Can't use an
/// undefined value as a SCALAR reference", a string dies with the strict
/// refs message, another kind of reference "Not a SCALAR reference".
/// (Not for `defined $$r`, nor for an element `$$r{k}` / `$$r[i]`, which
/// autovivify instead: see docs/port/PORTING.md.)
pub fn sderef(r: &Value) -> ER<Value> {
    match r {
        Value::ScalarRef(_) | Value::LvRef(_) => Ok(r.deref()),
        Value::Undef => crate::interp::eval::die("Can't use an undefined value as a SCALAR reference"),
        v if !v.is_ref() => symref_die(v, "a SCALAR"),
        _ => crate::interp::eval::die("Not a SCALAR reference"),
    }
}

/// `@$r` read as perl does under strict refs (see sderef): the elements.
pub fn aderef(r: &Value) -> ER<Vec<Value>> {
    match r {
        Value::ArrayRef(a) => Ok(a.borrow().clone()),
        Value::Undef => crate::interp::eval::die("Can't use an undefined value as an ARRAY reference"),
        v if !v.is_ref() => symref_die(v, "an ARRAY"),
        _ => crate::interp::eval::die("Not an ARRAY reference"),
    }
}

/// `%$r` read as perl does under strict refs (see sderef): the hash
/// reference itself, checked.
pub fn hderef(r: &Value) -> ER<Value> {
    match r {
        Value::HashRef(_) => Ok(r.clone()),
        Value::Undef => crate::interp::eval::die("Can't use an undefined value as a HASH reference"),
        v if !v.is_ref() => symref_die(v, "a HASH"),
        _ => crate::interp::eval::die("Not a HASH reference"),
    }
}
