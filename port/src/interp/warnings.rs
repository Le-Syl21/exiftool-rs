//! perl's run-time warnings: "Use of uninitialized value[ NAME] in OP" and
//! "Argument \"X\" isn't numeric in OP", when warnings are enabled
//! (`use warnings` in the running code, or `$^W`).
//!
//! The operators call `use_str` / `use_num` on each operand with the
//! operand's expression and perl's description of the operator; when the
//! value is undef (or a string that is not a number) and warnings are on,
//! the message is built as perl's `report_uninit` / `not_a_number` build
//! it. The name of the value follows perl's `find_uninit_var`: a variable
//! (`$x`, `$Pkg::x`, `$_`, `$1`), an element of a named array or hash with a
//! constant or simple-variable subscript (`$a[2]`, `$h{"key"}`), or
//! `within @a` / `within %h` when the subscript is an expression; nothing
//! for a dereference (`$$r{k}`), a sub's result or a temporary. When the
//! operator has more than one operand that could be undef (`many`), perl
//! names only a variable that holds the undefined value itself, so an
//! element that does not exist is not named.
//!
//! Everything here runs only on the slow path (an undef or non-numeric
//! operand), so that warnings cost nothing when values are defined.

use super::*;

/// Keeps alive the storage whose string was numified (so its address is
/// not reused while it is in `Interp::numified`).
#[allow(dead_code)]
pub(crate) enum Keep {
    Cell(Cell),
    Array(ArrayRc),
    Hash(HashRc),
    Const,
}

/// perl's description (OP_DESC) of a binary operator.
pub(crate) fn bin_desc(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "addition (+)",
        BinOp::Sub => "subtraction (-)",
        BinOp::Mul => "multiplication (*)",
        BinOp::Div => "division (/)",
        BinOp::Mod => "modulus (%)",
        BinOp::Pow => "exponentiation (**)",
        BinOp::Concat => "concatenation (.) or string",
        BinOp::Repeat => "repeat (x)",
        BinOp::NumEq => "numeric eq (==)",
        BinOp::NumNe => "numeric ne (!=)",
        BinOp::NumLt => "numeric lt (<)",
        BinOp::NumGt => "numeric gt (>)",
        BinOp::NumLe => "numeric le (<=)",
        BinOp::NumGe => "numeric ge (>=)",
        BinOp::NumCmp => "numeric comparison (<=>)",
        BinOp::StrEq => "string eq",
        BinOp::StrNe => "string ne",
        BinOp::StrLt => "string lt",
        BinOp::StrGt => "string gt",
        BinOp::StrLe => "string le",
        BinOp::StrGe => "string ge",
        BinOp::StrCmp => "string comparison (cmp)",
        BinOp::BitAnd => "bitwise and (&)",
        BinOp::BitOr => "bitwise or (|)",
        BinOp::BitXor => "bitwise xor (^)",
        BinOp::Shl => "left bitshift (<<)",
        BinOp::Shr => "right bitshift (>>)",
        BinOp::And | BinOp::Or | BinOp::DefOr | BinOp::Xor => "",
    }
}

/// Whether a string is a number for perl's "isn't numeric" check
/// (`looks_like_number`, and the "0 but true" exception).
pub(crate) fn numeric_string(s: &[u8]) -> bool {
    // the common case without grok_number: [-]digits[.digits]
    let b = s.strip_prefix(b"-").unwrap_or(s);
    let int = b.iter().take_while(|c| c.is_ascii_digit()).count();
    if int > 0 {
        let rest = &b[int..];
        if rest.is_empty() {
            return true;
        }
        if rest[0] == b'.' && rest[1..].iter().all(u8::is_ascii_digit) {
            return true;
        }
    }
    crate::perl::value::grok_number(s).1 == crate::perl::value::Grok::Whole || s == b"0 but true"
}

/// The string as perl's `not_a_number` shows it in its message: at most
/// about 56 characters (then "..."), control characters as `^X` or `\n`,
/// and non-printable high bytes as `M-` and their 7-bit equivalent.
pub(crate) fn display_arg(s: &[u8]) -> Vec<u8> {
    let mut d = Vec::new();
    let mut i = 0;
    while i < s.len() && d.len() < 56 {
        let mut ch = s[i];
        if ch >= 0x80 && ch < 0xa0 {
            d.extend_from_slice(b"M-");
            ch &= 127;
        }
        match ch {
            b'\n' => d.extend_from_slice(b"\\n"),
            b'\r' => d.extend_from_slice(b"\\r"),
            0x0c => d.extend_from_slice(b"\\f"),
            b'\\' => d.extend_from_slice(b"\\\\"),
            0 => d.extend_from_slice(b"\\0"),
            0x20..=0x7e | 0xa0..=0xff => d.push(ch),
            _ => {
                d.push(b'^');
                d.push(ch ^ 64);
            }
        }
        i += 1;
    }
    if i < s.len() {
        d.extend_from_slice(b"...");
    }
    d
}

/// A hash key as perl's `varname` quotes it (`pv_pretty` with 32
/// characters at most).
fn quote_key(k: &[u8]) -> String {
    let mut s = String::from("\"");
    let mut n = 0;
    let mut cut = false;
    for &c in k {
        let piece = match c {
            b'"' => "\\\"".to_string(),
            b'\\' => "\\\\".to_string(),
            b'\n' => "\\n".to_string(),
            b'\r' => "\\r".to_string(),
            b'\t' => "\\t".to_string(),
            0x0c => "\\f".to_string(),
            0x1b => "\\e".to_string(),
            0x07 => "\\a".to_string(),
            0x08 => "\\b".to_string(),
            0x20..=0x7e => (c as char).to_string(),
            _ => format!("\\{:o}", c),
        };
        if n + piece.len() > 32 {
            cut = true;
            break;
        }
        n += piece.len();
        s.push_str(&piece);
    }
    s.push('"');
    if cut {
        s.push_str("...");
    }
    s
}

/// An expression that perl skips when it looks for the culprit: a defined
/// constant.
fn defined_const(e: &Expr) -> bool {
    matches!(e, Expr::Const(v) if !v.is_undef())
}

/// A constant or a plain variable subscript, which perl names with its
/// value even when the element does not exist.
fn simple_subscript(e: &Expr) -> bool {
    match e {
        Expr::Const(_) | Expr::Scalar(_) => true,
        Expr::Un(UnOp::Neg, c) => matches!(**c, Expr::Const(_)),
        _ => false,
    }
}

/// Whether evaluating `e` again cannot change anything (to find which
/// element an expression subscript named).
fn pure(e: &Expr) -> bool {
    match e {
        Expr::Const(_) | Expr::Scalar(_) => true,
        Expr::Interp(parts) => parts.iter().all(|p| match p {
            Part::Scalar(e) | Part::Array(e) => pure(e),
            _ => true,
        }),
        Expr::Bin(op, l, r) => !matches!(op, BinOp::And | BinOp::Or | BinOp::DefOr) && pure(l) && pure(r),
        Expr::Un(op, e) => matches!(op, UnOp::Neg | UnOp::Plus | UnOp::Not) && pure(e),
        Expr::List(items) => items.len() == 1 && pure(&items[0]),
        Expr::Elem(Container::Named(_), i) => pure(i),
        Expr::HElem(Container::Named(_), k) => pure(k),
        Expr::Call { name, args, amp: false } if matches!(&**name, "lc" | "uc" | "lcfirst" | "ucfirst") => args.iter().all(pure),
        _ => false,
    }
}

impl Interp {
    /// Whether warnings are enabled for the running code.
    pub(crate) fn warn_on(&mut self) -> bool {
        if self.warn_quiet {
            return false;
        }
        // `use warnings` / `no warnings` in scope win over $^W
        if let Some(on) = WARNINGS_ON.with(|w| w.get()) {
            return on;
        }
        match self.globals.get(&self.w_key) {
            Some(Binding::Scalar(c)) => c.borrow().truthy(),
            _ => false,
        }
    }

    /// "Use of uninitialized value[ NAME] in OP at ..." through `warn`.
    pub(crate) fn report_uninit(&mut self, name: Option<String>, op: &str) -> ER<()> {
        let mut m = String::from("Use of uninitialized value");
        if let Some(n) = name {
            m.push(' ');
            m.push_str(&n);
        }
        if !op.is_empty() {
            m.push_str(" in ");
            m.push_str(op);
        }
        m.push_str(&location_suffix());
        self.warn(Value::from(m))
    }

    /// "Argument \"X\" isn't numeric in OP at ..." through `warn`.
    pub(crate) fn report_not_numeric(&mut self, s: &[u8], op: &str) -> ER<()> {
        let mut m = b"Argument \"".to_vec();
        m.extend_from_slice(&display_arg(s));
        m.extend_from_slice(b"\" isn't numeric in ");
        m.extend_from_slice(op.as_bytes());
        m.extend_from_slice(location_suffix().as_bytes());
        self.warn(Value::bytes(m))
    }

    /// The value `v` of operand `e` is used as a string by `op`.
    #[inline]
    pub(crate) fn use_str(&mut self, v: &Value, e: &Expr, scope: &Rc<Scope>, op: &str, many: bool) -> ER<()> {
        if v.is_undef() {
            self.uninit_slow(e, scope, op, many)?;
        }
        Ok(())
    }

    /// The value `v` of operand `e` is used as a number by `op`.
    #[inline]
    pub(crate) fn use_num(&mut self, v: &Value, e: &Expr, scope: &Rc<Scope>, op: &str, many: bool) -> ER<()> {
        match v {
            Value::Undef => self.uninit_slow(e, scope, op, many),
            Value::Str(s) | Value::UStr(s) => {
                if !numeric_string(s) {
                    self.not_numeric_slow(s, Some(e), scope, op)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    #[cold]
    pub(crate) fn uninit_slow(&mut self, e: &Expr, scope: &Rc<Scope>, op: &str, many: bool) -> ER<()> {
        if !self.warn_on() {
            return Ok(());
        }
        let n = self.uninit_name(e, scope, many);
        self.report_uninit(n, op)
    }

    #[cold]
    fn not_numeric_slow(&mut self, s: &Rc<[u8]>, e: Option<&Expr>, scope: &Rc<Scope>, op: &str) -> ER<()> {
        if !self.warn_on() {
            return Ok(());
        }
        // perl keeps the number it read in the variable: a variable that
        // was numified already does not warn again until it is assigned.
        if let Some(e) = e {
            if let Some((key, keep)) = self.storage_key(e, scope) {
                if let Some((_, old)) = self.numified.get(&key) {
                    if Rc::ptr_eq(old, s) {
                        return Ok(());
                    }
                }
                self.numified.insert(key, (keep, s.clone()));
            }
        }
        self.report_not_numeric(s, op)
    }

    /// Where the value of `e` is stored, for the numified-strings memory.
    fn storage_key(&mut self, e: &Expr, scope: &Rc<Scope>) -> Option<((usize, Vec<u8>), Keep)> {
        match e {
            // the result of || && // ?: is one of the operands themselves
            Expr::Bin(op @ (BinOp::Or | BinOp::And | BinOp::DefOr), l, r) if pure(l) => {
                let lv = self.quiet_scalar(l, scope)?;
                let left = match op {
                    BinOp::Or => lv.truthy(),
                    BinOp::And => !lv.truthy(),
                    _ => !lv.is_undef(),
                };
                self.storage_key(if left { l } else { r }, scope)
            }
            Expr::Cond(c, a, b) if pure(c) => {
                let t = self.quiet_scalar(c, scope)?.truthy();
                self.storage_key(if t { a } else { b }, scope)
            }
            Expr::List(items) if items.len() == 1 => self.storage_key(&items[0], scope),
            Expr::Scalar(n) => {
                if n.as_bytes()[0].is_ascii_digit() || matches!(&**n, "&" | "`" | "'" | "+") {
                    return None;
                }
                let c = self.scalar_cell(scope, n);
                Some(((Rc::as_ptr(&c) as usize, Vec::new()), Keep::Cell(c)))
            }
            Expr::Const(Value::Str(s) | Value::UStr(s)) => Some(((s.as_ptr() as usize, b"const".to_vec()), Keep::Const)),
            Expr::Elem(c, i) if pure(i) => {
                let i = self.quiet_scalar(i, scope)?.to_i64();
                let a = self.quiet_array(c, scope)?;
                let len = a.borrow().len();
                let i = index(len, i)?;
                Some(((Rc::as_ptr(&a) as usize, i.to_le_bytes().to_vec()), Keep::Array(a)))
            }
            Expr::HElem(c, k) if pure(k) => {
                let k = self.quiet_scalar(k, scope)?.to_bytes();
                let h = self.quiet_hash(c, scope)?;
                let mut key = k.to_vec();
                key.push(b'%');
                Some(((Rc::as_ptr(&h) as *const u8 as usize, key), Keep::Hash(h)))
            }
            _ => None,
        }
    }

    /// Evaluate a pure expression again, without warnings.
    fn quiet_scalar(&mut self, e: &Expr, scope: &Rc<Scope>) -> Option<Value> {
        let saved = std::mem::replace(&mut self.warn_quiet, true);
        let r = self.scalar(e, scope).ok();
        self.warn_quiet = saved;
        r
    }

    fn quiet_array(&mut self, c: &Container, scope: &Rc<Scope>) -> Option<ArrayRc> {
        match c {
            Container::Named(n) => Some(self.named_array(scope, n)),
            Container::Deref(e) if pure(e) => match self.quiet_scalar(e, scope)? {
                Value::ArrayRef(a) => Some(a),
                _ => None,
            },
            _ => None,
        }
    }

    fn quiet_hash(&mut self, c: &Container, scope: &Rc<Scope>) -> Option<HashRc> {
        match c {
            Container::Named(n) => Some(self.named_hash(scope, n)),
            Container::Deref(e) if pure(e) => match self.quiet_scalar(e, scope)? {
                Value::HashRef(h) => Some(h),
                _ => None,
            },
            _ => None,
        }
    }

    /// `$x`, `$Pkg::x`, `@a`, `%Pkg::h`: a variable as perl names it.
    fn var_name(&self, sigil: u8, n: &str, scope: &Scope) -> String {
        let mut s = String::new();
        s.push(sigil as char);
        if scope.lookup(sigil, n).is_some() {
            s.push_str(n);
            return s;
        }
        let key = self.global_key(n);
        s.push_str(key.strip_prefix("main::").unwrap_or(&key));
        s
    }

    /// The name perl gives the undefined value of `e` (see the module's
    /// comment), or None.
    pub(crate) fn uninit_name(&mut self, e: &Expr, scope: &Rc<Scope>, many: bool) -> Option<String> {
        match e {
            Expr::Scalar(n) => {
                if many && !self.quiet_scalar(e, scope).is_some_and(|v| v.is_undef()) {
                    return None;
                }
                Some(self.var_name(b'$', n, scope))
            }
            Expr::Elem(Container::Named(a), i) => {
                let simple = simple_subscript(i);
                let arr = self.named_array(scope, a);
                let found = if pure(i) {
                    self.quiet_scalar(i, scope).map(|v| v.to_i64()).map(|i| {
                        let a = arr.borrow();
                        (i, index(a.len(), i).and_then(|x| a.get(x)).map(|v| v.is_undef()))
                    })
                } else {
                    // perl finds the element holding the undef value: here
                    // the only undef one, if there is a single one
                    let a = arr.borrow();
                    let mut undefs = a.iter().enumerate().filter(|(_, v)| v.is_undef());
                    match (undefs.next(), undefs.next()) {
                        (Some((i, _)), None) => Some((i as i64, Some(true))),
                        _ => None,
                    }
                };
                let name = self.var_name(b'@', a, scope);
                match found {
                    // an element that exists and is undef
                    Some((i, Some(true))) => Some(format!("${}[{i}]", &name[1..])),
                    Some((i, _)) if simple && !many => Some(format!("${}[{i}]", &name[1..])),
                    _ if many => None,
                    _ => Some(format!("within {name}")),
                }
            }
            Expr::HElem(Container::Named(h), k) => {
                let simple = simple_subscript(k);
                let hash = self.named_hash(scope, h);
                let found = if pure(k) {
                    self.quiet_scalar(k, scope).map(|v| v.to_bytes()).map(|k| {
                        let st = hash.borrow().get(&k).map(|v| v.is_undef());
                        (k, st)
                    })
                } else {
                    None
                };
                let name = self.var_name(b'%', h, scope);
                match found {
                    Some((k, Some(true))) => Some(format!("${}{{{}}}", &name[1..], quote_key(&k))),
                    Some((k, _)) if simple && !many => Some(format!("${}{{{}}}", &name[1..], quote_key(&k))),
                    _ if many => None,
                    _ => Some(format!("within {name}")),
                }
            }
            Expr::Cond(c, a, b) => self.uninit_kids(&[c, a, b], scope, many),
            Expr::Bin(BinOp::And | BinOp::Or | BinOp::DefOr, l, r) => self.uninit_kids(&[l, r], scope, many),
            Expr::List(items) if items.len() == 1 => self.uninit_name(&items[0], scope, many),
            Expr::ListSlice(l, i) => match (&**l, &**i) {
                (Expr::List(items), Expr::Const(k)) => {
                    let k = k.to_i64();
                    let k = if k < 0 { items.len() as i64 + k } else { k };
                    let item = items.get(usize::try_from(k).ok()?)?;
                    self.uninit_name(item, scope, many)
                }
                _ => self.uninit_name(l, scope, many),
            },
            // length(undef) is undef
            Expr::Call { name, args, amp: false } if &**name == "length" && args.len() == 1 => self.uninit_name(&args[0], scope, many),
            _ => None,
        }
    }

    fn uninit_kids(&mut self, kids: &[&Expr], scope: &Rc<Scope>, many: bool) -> Option<String> {
        let cand: Vec<&Expr> = kids.iter().copied().filter(|k| !defined_const(k)).collect();
        if cand.len() == 1 {
            return self.uninit_name(cand[0], scope, many);
        }
        for k in cand {
            if let Some(n) = self.uninit_name(k, scope, true) {
                return Some(n);
            }
        }
        None
    }

    /// Whether more than one of the operands could be the undefined one.
    pub(crate) fn many(kids: &[&Expr]) -> bool {
        kids.iter().filter(|k| !defined_const(k)).count() > 1
    }

    /// The first argument of a builtin, or `$_`, used by `op` as a string
    /// or a number (`num`).
    pub(crate) fn arg_used(&mut self, args: &[Expr], scope: &Rc<Scope>, op: &str, num: bool) -> ER<Value> {
        let (v, e) = match args.first() {
            Some(a) => (self.scalar(a, scope)?, None),
            None => (self.read_scalar("_", scope), Some(Expr::Scalar(Rc::from("_")))),
        };
        if matches!(v, Value::Undef | Value::Str(_) | Value::UStr(_)) {
            let e = e.as_ref().unwrap_or_else(|| &args[0]);
            if num {
                self.use_num(&v, e, scope, op, false)?;
            } else {
                self.use_str(&v, e, scope, op, false)?;
            }
        }
        Ok(v)
    }

    /// The arguments of a list operator, flattened; with warnings on, also
    /// where each argument's values start (to name a value in a warning).
    pub(crate) fn values_traced(&mut self, args: &[Expr], scope: &Rc<Scope>) -> ER<(Vec<Value>, Vec<usize>)> {
        let trace = self.warn_on();
        let mut out = Vec::new();
        let mut starts = Vec::new();
        for a in args {
            if trace {
                starts.push(out.len());
            }
            out.extend(self.list(a, scope)?);
        }
        Ok((out, starts))
    }

    /// The name of value `j` of a list operator's arguments (see
    /// `values_traced`).
    pub(crate) fn list_name(&mut self, j: usize, args: &[Expr], origin: &[usize], scope: &Rc<Scope>) -> Option<String> {
        let i = origin.iter().rposition(|&s| s <= j)?;
        let kids: Vec<&Expr> = args.iter().collect();
        let many = Interp::many(&kids);
        self.elem_name(&args[i], j - origin[i], scope, many)
    }

    /// The name of the `k`th value of list expression `e`.
    pub(crate) fn elem_name(&mut self, e: &Expr, k: usize, scope: &Rc<Scope>, many: bool) -> Option<String> {
        match e {
            Expr::Array(Container::Named(a)) => {
                let n = self.var_name(b'@', a, scope);
                Some(format!("${}[{k}]", &n[1..]))
            }
            Expr::Array(_) | Expr::Hash(_) | Expr::Slice(..) | Expr::HSlice(..) | Expr::Call { .. } | Expr::Method { .. } | Expr::CallRef(..) | Expr::Map(..) | Expr::Grep(..) | Expr::Sort(..) => None,
            Expr::List(items) => {
                // scalars give one value each
                if items.iter().all(|i| matches!(i, Expr::Scalar(_) | Expr::Const(_) | Expr::Elem(..) | Expr::HElem(..))) {
                    let i = items.get(k)?;
                    return self.uninit_name(i, scope, many);
                }
                None
            }
            _ if k == 0 => self.uninit_name(e, scope, many),
            _ => None,
        }
    }

    /// Value `j` of a list operator's arguments used as a string or a number.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn list_use(&mut self, v: &Value, j: usize, args: &[Expr], origin: &[usize], scope: &Rc<Scope>, op: &str, num: bool) -> ER<()> {
        match v {
            Value::Undef => {
                if !origin.is_empty() && self.warn_on() {
                    let n = self.list_name(j, args, origin, scope);
                    self.report_uninit(n, op)?;
                }
            }
            Value::Str(s) | Value::UStr(s) if num && !numeric_string(s) => {
                if !origin.is_empty() {
                    // a variable keeps its number, as for use_num
                    let i = origin.iter().rposition(|&st| st <= j).unwrap_or(0);
                    let single = j - origin[i] == 0 && !matches!(args[i], Expr::Array(_) | Expr::List(_));
                    self.not_numeric_slow(s, if single { Some(&args[i]) } else { None }, scope, op)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// `sprintf` / `printf` on the values of their arguments (the format
    /// first), with perl's warnings for the arguments its conversions use.
    pub(crate) fn sprintf_warned(&mut self, vals: &[Value], args: &[Expr], origin: &[usize], scope: &Rc<Scope>, op: &str) -> ER<Value> {
        let fmt = vals.first().cloned().unwrap_or_else(|| Value::str(""));
        let rest = &vals[1.min(vals.len())..];
        if origin.is_empty() {
            let out = crate::perl::sprintf::sprintf_value(&fmt, rest, None);
            if let Some(e) = crate::perl::sprintf::take_error() {
                return die(e.message(op));
            }
            return Ok(out);
        }
        if let Some(f) = vals.first() {
            self.list_use(f, 0, args, origin, scope, op, false)?;
        }
        let mut uses = Vec::new();
        let out = crate::perl::sprintf::sprintf_value(&fmt, rest, Some(&mut uses));
        if let Some(e) = crate::perl::sprintf::take_error() {
            return die(e.message(op));
        }
        use crate::perl::sprintf::ArgUse;
        for u in uses {
            match u {
                ArgUse::Str(i) => self.list_use(&rest[i], i + 1, args, origin, scope, op, false)?,
                ArgUse::Num(i) => self.list_use(&rest[i], i + 1, args, origin, scope, op, true)?,
                ArgUse::Missing => {
                    let m = format!("Missing argument in {op}{}", location_suffix());
                    self.warn(Value::from(m))?;
                }
                ArgUse::Redundant => {
                    let m = format!("Redundant argument in {op}{}", location_suffix());
                    self.warn(Value::from(m))?;
                }
            }
        }
        Ok(out)
    }

    /// An array subscript, used as a number.
    pub(crate) fn elem_index(&mut self, i: &Expr, scope: &Rc<Scope>, op: &str) -> ER<i64> {
        let v = self.scalar(i, scope)?;
        match &v {
            Value::Undef => self.uninit_slow(i, scope, op, false)?,
            Value::Str(s) | Value::UStr(s) if !numeric_string(s) => {
                // a constant or variable subscript is part of perl's
                // multideref op
                let op = if simple_subscript(i) && op == "array element" { "array or hash lookup" } else { op };
                self.use_num(&v, i, scope, op, false)?
            }
            _ => {}
        }
        Ok(v.to_i64())
    }
}
