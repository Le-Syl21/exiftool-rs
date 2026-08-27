//! A small evaluator for ExifTool's conversion expressions.
//!
//! `ValueConv` and `PrintConv` are Perl, and most of them are very little Perl:
//! of the 4706 in ExifTool 13.59, 3281 are arithmetic on `$val`, a comparison,
//! a ternary, an interpolated string or a `sprintf`. Those are ported here once
//! rather than by hand, one tag at a time, which is how a translation drifts
//! from its source.
//!
//! What is deliberately absent: regular expressions, statements, and calls into
//! ExifTool's own helpers. An expression using them returns `None`, and the
//! caller keeps the raw value rather than inventing a converted one.

use regex_lite::Regex;

/// A value flowing through a conversion: Perl does not distinguish, and neither
/// does ExifTool's output until it is printed.
#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    /// Perl's `undef`: what a hash member that was never set reads as.
    Undef,
    Num(f64),
    Str(String),
    /// Perl's list: what `split` and `unpack` produce and `join` consumes.
    List(Vec<Val>),
}

impl Val {
    #[must_use]
    pub fn as_num(&self) -> f64 {
        match self {
            Self::Num(n) => *n,
            // Perl reads a leading number out of a string and calls the rest zero.
            Self::Undef => 0.0,
            Self::Str(s) => leading_number(s),
            // A list in numeric context is its length.
            #[allow(clippy::cast_precision_loss)]
            Self::List(v) => v.len() as f64,
        }
    }

    #[must_use]
    pub fn as_string(&self) -> String {
        match self {
            Self::Undef => String::new(),
            Self::Num(n) => format_number(*n),
            Self::Str(s) => s.clone(),
            // `"@a"` interpolates a list separated by `$"`, which is a space.
            Self::List(v) => v.iter().map(Self::as_string).collect::<Vec<_>>().join(" "),
        }
    }

    fn truthy(&self) -> bool {
        match self {
            Self::Undef => false,
            Self::Num(n) => *n != 0.0,
            Self::Str(s) => !s.is_empty() && s != "0",
            Self::List(v) => !v.is_empty(),
        }
    }
}

/// Perl prints a float without a trailing `.0`, and integers as integers.
fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        let s = format!("{n}");
        s
    }
}

fn leading_number(s: &str) -> f64 {
    let t = s.trim_start();
    let end = t
        .char_indices()
        .take_while(|(i, c)| {
            c.is_ascii_digit()
                || *c == '.'
                || ((*c == '-' || *c == '+') && *i == 0)
                || *c == 'e'
                || *c == 'E'
        })
        .map(|(i, c)| i + c.len_utf8())
        .last()
        .unwrap_or(0);
    t[..end].parse().unwrap_or(0.0)
}

/// Evaluate an ExifTool conversion expression with `$val` bound to `val`.
///
/// Returns `None` when the expression uses anything outside the supported
/// grammar, which the caller must treat as "leave the value alone".
#[must_use]
/// What a conversion can ask about the file being read.
///
/// ExifTool keeps its parse state on the object itself, and conversions reach
/// into it: `$$self{TimecodeScale}`, `$$self{Model}`. `member` returns `None`
/// for a name the reader does not track at all, which declines the conversion
/// -- as opposed to `Some(Val::Undef)`, which is Perl's own answer for a
/// member that exists but was never set, and is a value like any other.
pub trait ParseState {
    fn member(&self, name: &str) -> Option<Val>;
}

/// A reader with no parse state at all: every member is unknown.
impl ParseState for () {
    fn member(&self, _: &str) -> Option<Val> {
        None
    }
}

impl ParseState for std::collections::HashMap<String, Val> {
    fn member(&self, name: &str) -> Option<Val> {
        self.get(name).cloned()
    }
}

pub fn eval(expr: &str, val: &Val) -> Option<Val> {
    eval_with(expr, val, &())
}

/// As [`eval`], with the file-level parse state the conversion may read.
pub fn eval_with(expr: &str, val: &Val, state: &dyn ParseState) -> Option<Val> {
    let mut p = Parser {
        s: expr.as_bytes(),
        i: 0,
        val: val.clone(),
        captures: Vec::new(),
        vars: std::collections::HashMap::new(),
        state,
        subject_after: None,
        quiet: 0,
    };
    // These conversions are sometimes two statements: `$val =~ s/ +$//; $val`
    // substitutes and then hands back the value it changed. The last one is the
    // result, as in Perl.
    p.skip_require();
    let mut last = p.statement()?;
    loop {
        p.skip_ws();
        if p.i < p.s.len() && p.s[p.i] == b';' {
            p.i += 1;
            p.skip_ws();
            p.skip_require();
            if p.i == p.s.len() {
                break;
            }
            last = p.statement()?;
        } else {
            break;
        }
    }
    p.skip_ws();
    if p.i == p.s.len() { Some(last) } else { None }
}

/// Somewhere a value can be stored.
enum LValue {
    /// `$val` itself, which several conversions rewrite before returning it.
    TheValue,
    Var(String),
    Elem(String, usize),
}

/// Perl's bitwise operators work on integers; a value that is not one is not
/// something to guess at.
fn to_u64(v: &Val) -> Option<u64> {
    let n = v.as_num();
    if !n.is_finite() {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(if n < 0.0 { n.trunc() as i64 as u64 } else { n.trunc() as u64 })
}

fn ident_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Perl's `%`, which truncates both sides to integers and takes the sign of
/// the right-hand one.
fn perl_modulo(a: f64, b: f64) -> Option<f64> {
    #[allow(clippy::cast_possible_truncation)]
    let (a, b) = (a.trunc() as i64, b.trunc() as i64);
    if b == 0 {
        return None;
    }
    #[allow(clippy::cast_precision_loss)]
    let m = a.rem_euclid(b.abs());
    #[allow(clippy::cast_precision_loss)]
    Some(if b < 0 && m != 0 { (m - b.abs()) as f64 } else { m as f64 })
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    /// Owned, because `s///` and `tr///` rewrite it in place as Perl does.
    val: Val,
    /// `$1`..`$9` from the most recent match.
    captures: Vec<String>,
    /// `my` variables. Scalars are stored under their bare name, arrays under
    /// the same name prefixed with `@`, and `$_` under `_`.
    vars: std::collections::HashMap<String, Val>,
    state: &'a dyn ParseState,
    /// What `s///` or `tr///` made of its subject, for the caller to store
    /// back into whatever the match was bound to.
    subject_after: Option<Val>,
    /// Non-zero while evaluating a branch Perl would never have run. The
    /// parser still has to walk it to find where it ends, but a division by
    /// zero in there is not a reason to refuse the conversion -- Perl guards
    /// exactly that way: `$val ? 1/$val : 0`.
    quiet: usize,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn eat(&mut self, tok: &str) -> bool {
        self.skip_ws();
        if self.s[self.i..].starts_with(tok.as_bytes()) {
            self.i += tok.len();
            true
        } else {
            false
        }
    }

    /// `require Image::ExifTool::XMP;` only tells Perl to load a file. Every
    /// module it can name is already part of this crate, so the statement
    /// carries no value and is skipped whole.
    fn skip_require(&mut self) {
        loop {
            self.skip_ws();
            if !self.s[self.i..].starts_with(b"require ") {
                return;
            }
            while self.i < self.s.len() && self.s[self.i] != b';' {
                self.i += 1;
            }
            if self.i < self.s.len() {
                self.i += 1; // the `;`
            }
        }
    }

    fn peek(&mut self, tok: &str) -> bool {
        self.skip_ws();
        self.s[self.i..].starts_with(tok.as_bytes())
    }

    /// One statement of a conversion. Perl lets a statement carry a trailing
    /// `foreach`, which runs it once per element with `$_` *aliased* to the
    /// element -- that aliasing is the whole point of
    /// `$_ /= 0x4000 foreach @a`, which scales the array in place.
    fn statement(&mut self) -> Option<Val> {
        if let Some((body_end, kw_end)) = self.find_foreach() {
            let body_start = self.i;
            self.i = kw_end;
            self.skip_ws();
            // Aliasing needs to know which array to write back into.
            let name = if self.eat("@") {
                Some(format!("@{}", self.ident()?))
            } else {
                None
            };
            let mut items = match &name {
                Some(n) => flatten(&[self.vars.get(n)?.clone()]),
                None => flatten(&[self.expr()?]),
            };
            for item in &mut *items {
                self.vars.insert("_".to_string(), item.clone());
                self.run_slice(body_start, body_end)?;
                *item = self.vars.get("_")?.clone();
            }
            let result = Val::List(items);
            if let Some(n) = name {
                self.vars.insert(n, result.clone());
            }
            return Some(result);
        }
        self.declaration()
    }

    /// Re-run a slice of the source with the same variables, which is what a
    /// statement modifier needs: the body is evaluated once per element.
    fn run_slice(&mut self, from: usize, to: usize) -> Option<Val> {
        let saved_s = self.s;
        let saved_i = self.i;
        self.s = &saved_s[from..to];
        self.i = 0;
        let r = self.declaration();
        self.s = saved_s;
        self.i = saved_i;
        r
    }

    /// Look ahead for a `foreach` (or `for`) modifier in this statement,
    /// outside any string or bracket. Returns where the body ends and where
    /// the list begins.
    fn find_foreach(&mut self) -> Option<(usize, usize)> {
        let mut depth = 0i32;
        let mut j = self.i;
        while j < self.s.len() {
            match self.s[j] {
                b'"' | b'\'' => {
                    let q = self.s[j];
                    j += 1;
                    while j < self.s.len() && self.s[j] != q {
                        j += if self.s[j] == b'\\' { 2 } else { 1 };
                    }
                }
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b';' if depth == 0 => return None,
                b'f' if depth == 0 && j > self.i && self.s[j - 1].is_ascii_whitespace() => {
                    for kw in ["foreach", "for"] {
                        if self.s[j..].starts_with(kw.as_bytes())
                            && self.s.get(j + kw.len()).is_some_and(u8::is_ascii_whitespace)
                        {
                            return Some((j, j + kw.len()));
                        }
                    }
                }
                _ => {}
            }
            j += 1;
        }
        None
    }

    /// `my $x = ...`, `my @a = ...`, `my ($a,$b,$c) = ...`, or a plain
    /// expression.
    fn declaration(&mut self) -> Option<Val> {
        self.skip_ws();
        if !(self.s[self.i..].starts_with(b"my")
            && self
                .s
                .get(self.i + 2)
                .is_some_and(|c| c.is_ascii_whitespace() || *c == b'(' || *c == b'$' || *c == b'@'))
        {
            return self.expr();
        }
        self.eat("my");
        self.skip_ws();
        if self.eat("(") {
            let mut names = Vec::new();
            loop {
                self.skip_ws();
                if !self.eat("$") {
                    return None;
                }
                names.push(self.ident()?);
                if !self.eat(",") {
                    break;
                }
            }
            if !self.eat(")") || !self.eat("=") {
                return None;
            }
            let items = flatten(&[self.expr()?]);
            for (k, n) in names.iter().enumerate() {
                // Perl leaves a name with no value undefined; an expression
                // that then reads it is one we cannot answer for.
                // Perl leaves a name past the end of the list undefined.
                let v = items.get(k).cloned().unwrap_or(Val::Undef);
                self.vars.insert(n.clone(), v);
            }
            return Some(Val::List(items));
        }
        if self.eat("@") {
            let name = format!("@{}", self.ident()?);
            if !self.eat("=") {
                return None;
            }
            let items = Val::List(flatten(&[self.expr()?]));
            self.vars.insert(name, items.clone());
            return Some(items);
        }
        if self.eat("$") {
            let name = self.ident()?;
            if !self.eat("=") {
                return None;
            }
            let v = self.expr()?;
            self.vars.insert(name, v.clone());
            return Some(v);
        }
        None
    }

    /// Assignment, which is the loosest-binding operator here and associates
    /// to the right.
    fn expr(&mut self) -> Option<Val> {
        if let Some(v) = self.try_assignment() {
            return Some(v);
        }
        self.low_or()
    }

    fn try_assignment(&mut self) -> Option<Val> {
        let save = self.i;
        self.skip_ws();
        let Some(target) = self.lvalue() else {
            self.i = save;
            return None;
        };
        self.skip_ws();
        let mut op = None;
        for candidate in ["**=", "+=", "-=", "*=", "/=", ".=", "%="] {
            if self.s[self.i..].starts_with(candidate.as_bytes()) {
                op = Some(candidate);
                break;
            }
        }
        if op.is_none()
            && self.s[self.i..].starts_with(b"=")
            && !self.s[self.i..].starts_with(b"==")
            && !self.s[self.i..].starts_with(b"=~")
            && !self.s[self.i..].starts_with(b"=>")
        {
            op = Some("=");
        }
        let Some(op) = op else {
            self.i = save;
            return None;
        };
        self.i += op.len();
        let Some(rhs) = self.expr() else {
            self.i = save;
            return None;
        };
        let cur = self.read_lvalue(&target);
        let value = match op {
            "=" => rhs,
            "+=" => Val::Num(cur?.as_num() + rhs.as_num()),
            "-=" => Val::Num(cur?.as_num() - rhs.as_num()),
            "*=" => Val::Num(cur?.as_num() * rhs.as_num()),
            "**=" => Val::Num(cur?.as_num().powf(rhs.as_num())),
            "%=" => Val::Num(perl_modulo(cur?.as_num(), rhs.as_num())?),
            ".=" => Val::Str(format!("{}{}", cur?.as_string(), rhs.as_string())),
            _ => {
                let d = rhs.as_num();
                if d == 0.0 {
                    return self.unreachable();
                }
                Val::Num(cur?.as_num() / d)
            }
        };
        self.write_lvalue(&target, value.clone())?;
        Some(value)
    }

    /// Something that can be assigned to. Returns `None` without moving if
    /// what is there is not one.
    fn lvalue(&mut self) -> Option<LValue> {
        let save = self.i;
        self.skip_ws();
        if self.s[self.i..].starts_with(b"$val")
            && !self.s.get(self.i + 4).is_some_and(|c| ident_char(*c))
        {
            self.i += 4;
            return Some(LValue::TheValue);
        }
        if self.s[self.i..].starts_with(b"@") {
            self.i += 1;
            let Some(name) = self.ident() else {
                self.i = save;
                return None;
            };
            return Some(LValue::Var(format!("@{name}")));
        }
        if self.s[self.i..].starts_with(b"$") && self.s.get(self.i + 1) != Some(&b'$') {
            self.i += 1;
            let Some(name) = self.ident() else {
                self.i = save;
                return None;
            };
            if self.peek("[") {
                self.eat("[");
                let Some(idx) = self.expr() else {
                    self.i = save;
                    return None;
                };
                if !self.eat("]") {
                    self.i = save;
                    return None;
                }
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                return Some(LValue::Elem(format!("@{name}"), idx.as_num() as usize));
            }
            return Some(LValue::Var(name));
        }
        self.i = save;
        None
    }

    fn read_lvalue(&self, target: &LValue) -> Option<Val> {
        match target {
            LValue::TheValue => Some(self.val.clone()),
            LValue::Var(n) => self.vars.get(n).cloned(),
            LValue::Elem(n, k) => match self.vars.get(n) {
                Some(Val::List(items)) => items.get(*k).cloned(),
                _ => None,
            },
        }
    }

    fn write_lvalue(&mut self, target: &LValue, v: Val) -> Option<()> {
        match target {
            LValue::TheValue => self.val = v,
            LValue::Var(n) => {
                self.vars.insert(n.clone(), v);
            }
            LValue::Elem(n, k) => {
                let Some(Val::List(items)) = self.vars.get_mut(n) else {
                    return None;
                };
                *items.get_mut(*k)? = v;
            }
        }
        Some(())
    }

    fn ident(&mut self) -> Option<String> {
        self.skip_ws();
        // `$_` is a name of its own.
        if self.s.get(self.i) == Some(&b'_') && !self.s.get(self.i + 1).is_some_and(|c| ident_char(*c))
        {
            self.i += 1;
            return Some("_".to_string());
        }
        let start = self.i;
        if !self.s.get(self.i).is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_') {
            return None;
        }
        while self.i < self.s.len() && ident_char(self.s[self.i]) {
            self.i += 1;
        }
        std::str::from_utf8(&self.s[start..self.i]).ok().map(str::to_string)
    }

    /// A variable reference in expression or string position: `$_`, a `my`
    /// scalar, an element of a `my` array, or the whole array.
    fn variable(&mut self) -> Option<Val> {
        let save = self.i;
        if self.eat("@") {
            let Some(name) = self.ident() else {
                self.i = save;
                return None;
            };
            return self.vars.get(&format!("@{name}")).cloned();
        }
        if !self.eat("$") {
            return None;
        }
        let Some(name) = self.ident() else {
            self.i = save;
            return None;
        };
        if self.peek("[") {
            self.eat("[");
            let idx = self.expr()?;
            if !self.eat("]") {
                return None;
            }
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let k = idx.as_num() as usize;
            // An element past the end of a known array is undef, as in Perl.
            // An array we never heard of is a gap, and declines.
            return match self.vars.get(&format!("@{name}")) {
                Some(Val::List(items)) => Some(items.get(k).cloned().unwrap_or(Val::Undef)),
                _ => None,
            };
        }
        self.vars.get(&name).cloned()
    }

    /// A value Perl would never have computed, in a branch it would never have
    /// run. Inside such a branch it stands in for the result; outside one, an
    /// arithmetic fault is a real one and refuses the conversion.
    fn unreachable(&self) -> Option<Val> {
        if self.quiet > 0 { Some(Val::Undef) } else { None }
    }

    /// Evaluate the branch that was not taken, only to find where it ends.
    fn skip_branch(&mut self, f: fn(&mut Self) -> Option<Val>) -> Option<Val> {
        self.quiet += 1;
        let r = f(self);
        self.quiet -= 1;
        r
    }

    /// `or` and `xor`, Perl's loosest operators -- looser even than assignment,
    /// which is why `$val and $val =~ s/^(\d)/+$1/` reads the way it does.
    fn low_or(&mut self) -> Option<Val> {
        let mut acc = self.low_and()?;
        loop {
            self.skip_ws();
            let word = ["or", "xor"]
                .into_iter()
                .find(|w| self.word_is(w));
            let Some(word) = word else { return Some(acc) };
            self.i += word.len();
            if word == "xor" {
                let r = self.low_and()?;
                acc = Val::Num(if acc.truthy() != r.truthy() { 1.0 } else { 0.0 });
            } else if acc.truthy() {
                self.skip_branch(Self::low_and)?;
            } else {
                acc = self.low_and()?;
            }
        }
    }

    fn low_and(&mut self) -> Option<Val> {
        let mut acc = self.low_not()?;
        loop {
            self.skip_ws();
            if !self.word_is("and") {
                return Some(acc);
            }
            self.i += 3;
            if acc.truthy() {
                acc = self.low_not()?;
            } else {
                self.skip_branch(Self::low_not)?;
            }
        }
    }

    fn low_not(&mut self) -> Option<Val> {
        self.skip_ws();
        if self.word_is("not") {
            self.i += 3;
            let v = self.low_not()?;
            return Some(Val::Num(if v.truthy() { 0.0 } else { 1.0 }));
        }
        self.ternary()
    }

    /// A bare word operator, which must not be read out of an identifier.
    fn word_is(&self, w: &str) -> bool {
        self.s[self.i..].starts_with(w.as_bytes())
            && !self.s.get(self.i + w.len()).is_some_and(|c| ident_char(*c))
    }

    fn ternary(&mut self) -> Option<Val> {
        let cond = self.or_or()?;
        if self.eat("?") {
            let taken = cond.truthy();
            let a = if taken { self.ternary()? } else { self.skip_branch(Self::ternary)? };
            if !self.eat(":") {
                return None;
            }
            let b = if taken { self.skip_branch(Self::ternary)? } else { self.ternary()? };
            return Some(if taken { a } else { b });
        }
        Some(cond)
    }

    /// `||` and `//`, which return the operand rather than a boolean: the
    /// `$val / ($$self{FocalUnits} || 1)` idiom depends on that.
    fn or_or(&mut self) -> Option<Val> {
        let mut acc = self.and_and()?;
        loop {
            self.skip_ws();
            // `//` is defined-or, but a `/` here could also start a regex, so
            // only the doubled form counts.
            let defined_or = self.s[self.i..].starts_with(b"//");
            if !defined_or && !self.s[self.i..].starts_with(b"||") {
                return Some(acc);
            }
            self.i += 2;
            let keep = if defined_or { acc != Val::Undef } else { acc.truthy() };
            if keep {
                self.skip_branch(Self::and_and)?;
            } else {
                acc = self.and_and()?;
            }
        }
    }

    fn and_and(&mut self) -> Option<Val> {
        let mut acc = self.bit_or()?;
        loop {
            self.skip_ws();
            if !self.s[self.i..].starts_with(b"&&") {
                return Some(acc);
            }
            self.i += 2;
            if acc.truthy() {
                acc = self.bit_or()?;
            } else {
                self.skip_branch(Self::bit_or)?;
            }
        }
    }

    /// The bitwise operators, which ExifTool reaches for constantly to pull a
    /// field out of a packed value: `sprintf("%x.%.2x",$val>>8,$val&0xff)`.
    fn bit_or(&mut self) -> Option<Val> {
        let mut acc = self.bit_and()?;
        loop {
            self.skip_ws();
            let c = self.s.get(self.i).copied();
            if (c != Some(b'|') && c != Some(b'^')) || self.s.get(self.i + 1).copied() == c {
                return Some(acc);
            }
            self.i += 1;
            let r = self.bit_and()?;
            let (a, b) = (to_u64(&acc)?, to_u64(&r)?);
            #[allow(clippy::cast_precision_loss)]
            let v = if c == Some(b'|') { a | b } else { a ^ b };
            acc = Val::Num(v as f64);
        }
    }

    fn bit_and(&mut self) -> Option<Val> {
        let mut acc = self.comparison()?;
        loop {
            self.skip_ws();
            if self.s.get(self.i) != Some(&b'&') || self.s.get(self.i + 1) == Some(&b'&') {
                return Some(acc);
            }
            self.i += 1;
            let r = self.comparison()?;
            #[allow(clippy::cast_precision_loss)]
            let v = (to_u64(&acc)? & to_u64(&r)?) as f64;
            acc = Val::Num(v);
        }
    }

    fn shift_expr(&mut self) -> Option<Val> {
        let mut acc = self.additive()?;
        loop {
            self.skip_ws();
            let left = self.s[self.i..].starts_with(b"<<");
            if !left && !self.s[self.i..].starts_with(b">>") {
                return Some(acc);
            }
            self.i += 2;
            let r = self.additive()?;
            let (a, b) = (to_u64(&acc)?, to_u64(&r)?);
            if b >= 64 {
                return self.unreachable();
            }
            #[allow(clippy::cast_precision_loss)]
            let v = if left { a << b } else { a >> b };
            acc = Val::Num(v as f64);
        }
    }

    fn comparison(&mut self) -> Option<Val> {
        // `=~` binds looser than arithmetic and tighter than a ternary. Its
        // left side is whatever the match reads and `s///` writes back to --
        // `$val` nearly always, but a `my` variable or a parse-state member
        // just as legitimately.
        let save = self.i;
        if let Some(target) = self.lvalue() {
            if self.peek("=~") || self.peek("!~") {
                let negated = self.peek("!~");
                self.eat(if negated { "!~" } else { "=~" });
                return self.bind_operation(&target, negated);
            }
        }
        self.i = save;
        if self.peek("$$self{") || self.peek("$self->{") {
            let subject = self.primary()?;
            if self.peek("=~") || self.peek("!~") {
                let negated = self.peek("!~");
                self.eat(if negated { "!~" } else { "=~" });
                // A member is read-only here: nothing writes a substitution
                // back into the parse state.
                return self.bind_to_value(&subject, negated);
            }
            self.i = save;
        }
        let left = self.shift_expr()?;
        // Perl keeps its string comparisons under separate names, and a word
        // operator must not be read out of the middle of an identifier.
        for (op, kind) in [("eq", 0), ("ne", 1), ("le", 2), ("ge", 3), ("lt", 4), ("gt", 5)] {
            self.skip_ws();
            if self.s[self.i..].starts_with(op.as_bytes())
                && !self.s.get(self.i + 2).is_some_and(|c| ident_char(*c))
            {
                self.i += 2;
                let right = self.shift_expr()?;
                let (a, b) = (left.as_string(), right.as_string());
                let r = match kind {
                    0 => a == b,
                    1 => a != b,
                    2 => a <= b,
                    3 => a >= b,
                    4 => a < b,
                    _ => a > b,
                };
                return Some(Val::Num(if r { 1.0 } else { 0.0 }));
            }
        }
        for op in ["<=>", "<=", ">=", "==", "!=", "<", ">"] {
            // `<` must not swallow the `<=` case, hence the order above.
            if self.peek(op) {
                self.eat(op);
                let right = self.shift_expr()?;
                let (a, b) = (left.as_num(), right.as_num());
                if op == "<=>" {
                    return Some(Val::Num(match a.partial_cmp(&b)? {
                        std::cmp::Ordering::Less => -1.0,
                        std::cmp::Ordering::Equal => 0.0,
                        std::cmp::Ordering::Greater => 1.0,
                    }));
                }
                let r = match op {
                    "<=" => a <= b,
                    ">=" => a >= b,
                    "==" => (a - b).abs() < f64::EPSILON,
                    "!=" => (a - b).abs() >= f64::EPSILON,
                    "<" => a < b,
                    _ => a > b,
                };
                return Some(Val::Num(if r { 1.0 } else { 0.0 }));
            }
        }
        Some(left)
    }

    fn additive(&mut self) -> Option<Val> {
        let mut acc = self.multiplicative()?;
        loop {
            self.skip_ws();
            // `**` is handled below; a `-` here is a binary minus.
            if self.peek("+") {
                self.eat("+");
                let r = self.multiplicative()?;
                acc = Val::Num(acc.as_num() + r.as_num());
            } else if self.peek("-") {
                self.eat("-");
                let r = self.multiplicative()?;
                acc = Val::Num(acc.as_num() - r.as_num());
            } else if self.peek(".")
                && self.s.get(self.i + 1).is_some_and(|c| *c != b'.' && !c.is_ascii_digit())
            {
                // String concatenation. A `.` before a digit is a decimal point
                // and a `..` is a range, so neither is one of ours.
                self.eat(".");
                let r = self.multiplicative()?;
                acc = Val::Str(format!("{}{}", acc.as_string(), r.as_string()));
            } else {
                return Some(acc);
            }
        }
    }

    fn multiplicative(&mut self) -> Option<Val> {
        let mut acc = self.power()?;
        loop {
            self.skip_ws();
            if self.peek("**") {
                return Some(acc); // handled by power()
            }
            // `x` repeats: `"H2" x 7` builds an unpack template, and it binds
            // as tightly as `*`. `xor` starts with the same letter, so the
            // operator is only an `x` that no identifier continues.
            if self.peek("x")
                && self
                    .s
                    .get(self.i + 1)
                    .is_none_or(|c| !(*c as char).is_alphabetic() && *c != b'_')
            {
                self.eat("x");
                let r = self.power()?;
                let n = r.as_num();
                if !(0.0..=4096.0).contains(&n) {
                    return None;
                }
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let n = n as usize;
                acc = match acc {
                    Val::List(items) => Val::List(std::iter::repeat_n(items, n).flatten().collect()),
                    other => Val::Str(other.as_string().repeat(n)),
                };
            } else if self.peek("*") {
                self.eat("*");
                let r = self.power()?;
                acc = Val::Num(acc.as_num() * r.as_num());
            } else if self.peek("%") {
                self.eat("%");
                let r = self.power()?;
                acc = match perl_modulo(acc.as_num(), r.as_num()) {
                    Some(m) => Val::Num(m),
                    None => self.unreachable()?,
                };
            } else if self.peek("/") {
                self.eat("/");
                let r = self.power()?;
                let d = r.as_num();
                // Perl dies on division by zero; ExifTool guards its expressions,
                // so reaching it means we misread something. Refuse rather than
                // return an infinity that would be printed as a value.
                if d == 0.0 {
                    return self.unreachable();
                }
                acc = Val::Num(acc.as_num() / d);
            } else {
                return Some(acc);
            }
        }
    }

    /// `**` binds tighter than `*` and associates to the right.
    fn power(&mut self) -> Option<Val> {
        let base = self.unary()?;
        if self.peek("**") {
            self.eat("**");
            let exp = self.power()?;
            return Some(Val::Num(base.as_num().powf(exp.as_num())));
        }
        Some(base)
    }

    fn unary(&mut self) -> Option<Val> {
        self.skip_ws();
        if self.peek("!") && !self.s[self.i..].starts_with(b"!~") {
            self.eat("!");
            let v = self.unary()?;
            return Some(Val::Num(if v.truthy() { 0.0 } else { 1.0 }));
        }
        if self.peek("~") {
            self.eat("~");
            let v = self.unary()?;
            #[allow(clippy::cast_precision_loss)]
            return Some(Val::Num(!to_u64(&v)? as f64));
        }
        if self.peek("+") {
            self.eat("+");
            return self.unary();
        }
        if self.peek("-") {
            self.eat("-");
            let v = self.unary()?;
            return Some(Val::Num(-v.as_num()));
        }
        self.primary()
    }

    fn primary(&mut self) -> Option<Val> {
        self.skip_ws();
        if self.i >= self.s.len() {
            return None;
        }
        if self.eat("(") {
            let v = self.expr()?;
            if !self.eat(")") {
                return None;
            }
            return Some(v);
        }
        if self.eat("$val") {
            return Some(self.val.clone());
        }
        if self.word_is("undef") {
            self.i += 5;
            return Some(Val::Undef);
        }
        // `$1`..`$9`, from the last successful match.
        if self.i + 1 < self.s.len() && self.s[self.i] == b'$' && self.s[self.i + 1].is_ascii_digit() {
            let idx = (self.s[self.i + 1] - b'0') as usize;
            self.i += 2;
            return Some(Val::Str(
                self.captures.get(idx.checked_sub(1)?).cloned().unwrap_or_default(),
            ));
        }
        // `$$self{Name}` and `$self->{Name}` read the file-level parse state.
        if self.peek("$$self{") || self.peek("$self->{") {
            if !self.eat("$$self{") {
                self.eat("$self->{");
            }
            let name = self.ident()?;
            if !self.eat("}") {
                return None; // a nested member is state we do not model
            }
            return self.state.member(&name);
        }
        // A `my` variable, an element of one, or `$_` inside a `foreach`.
        // An unknown name is state we do not have, and declines.
        if (self.s[self.i] == b'$' && self.s.get(self.i + 1) != Some(&b'$')
            && !self.s[self.i..].starts_with(b"$self"))
            || self.s[self.i] == b'@'
        {
            if let Some(v) = self.variable() {
                return Some(v);
            }
        }
        // ExifTool passes itself as the first argument to several helpers. It
        // carries options we do not model, and every helper ported here uses
        // only the defaults, so it stands in as an empty value.
        // …but `$self->Something(...)` is a call, not a value, and the helper
        // dispatch below must get to see it.
        if self.peek("$self") && !self.peek("$self->") {
            self.eat("$self");
            return Some(Val::Str(String::new()));
        }
        if self.peek("\"") {
            return self.interpolated_string();
        }
        // A single-quoted string interpolates nothing; only `\\` and `\'` are
        // escapes inside it.
        if self.peek("'") {
            self.eat("'");
            let mut out = String::new();
            while self.i < self.s.len() {
                let c = self.s[self.i];
                if c == b'\\' && self.i + 1 < self.s.len() {
                    out.push(self.s[self.i + 1] as char);
                    self.i += 2;
                    continue;
                }
                self.i += 1;
                if c == b'\'' {
                    return Some(Val::Str(out));
                }
                out.push(c as char);
            }
            return None;
        }
        for (name, f) in [
            ("int", f64::trunc as fn(f64) -> f64),
            ("abs", f64::abs),
            ("exp", f64::exp),
            ("log", f64::ln),
            ("sqrt", f64::sqrt),
        ] {
            if self.peek(name) {
                self.eat(name);
                if !self.eat("(") {
                    return None;
                }
                let v = self.ternary()?;
                if !self.eat(")") {
                    return None;
                }
                return Some(Val::Num(f(v.as_num())));
            }
        }
        if self.peek("sprintf") && self.s.get(self.i + 7) == Some(&b'(') {
            return self.sprintf_call();
        }
        if let Some(v) = self.list_op() {
            return Some(v);
        }
        if let Some(v) = self.helper_call() {
            return Some(v);
        }
        self.number()
    }

    fn number(&mut self) -> Option<Val> {
        self.skip_ws();
        // Hex literals are common in these expressions: offsets and bit masks
        // are written `0x80`, and reading only the leading `0` turned
        // `$val - 0x80` into `$val - 0`.
        if self.s[self.i..].starts_with(b"0x") || self.s[self.i..].starts_with(b"0X") {
            let start = self.i + 2;
            let mut j = start;
            while j < self.s.len() && (self.s[j] as char).is_ascii_hexdigit() {
                j += 1;
            }
            if j == start {
                return None;
            }
            self.i = j;
            let txt = std::str::from_utf8(&self.s[start..j]).ok()?;
            return i64::from_str_radix(txt, 16).ok().map(|n| Val::Num(n as f64));
        }
        let start = self.i;
        while self.i < self.s.len()
            && (self.s[self.i].is_ascii_digit() || self.s[self.i] == b'.')
        {
            self.i += 1;
        }
        if self.i == start {
            return None;
        }
        // Scientific notation: `$val / 1e6` is how several of these are written,
        // and stopping at the `e` left the divisor as 1.
        if self.i < self.s.len() && (self.s[self.i] | 0x20) == b'e' {
            let mark = self.i;
            let mut j = self.i + 1;
            if j < self.s.len() && (self.s[j] == b'+' || self.s[j] == b'-') {
                j += 1;
            }
            let digits = j;
            while j < self.s.len() && self.s[j].is_ascii_digit() {
                j += 1;
            }
            if j > digits {
                self.i = j;
            } else {
                self.i = mark;
            }
        }
        std::str::from_utf8(&self.s[start..self.i])
            .ok()?
            .parse()
            .ok()
            .map(Val::Num)
    }

    /// `"$val mm"` and friends: only `$val` interpolates.
    fn interpolated_string(&mut self) -> Option<Val> {
        if !self.eat("\"") {
            return None;
        }
        let mut out = String::new();
        while self.i < self.s.len() {
            if self.s[self.i] == b'"' {
                self.i += 1;
                return Some(Val::Str(out));
            }
            if self.s[self.i] == b'\\' {
                let (c, len) = string_escape(&self.s[self.i + 1..])?;
                out.push(c);
                self.i += 1 + len;
                continue;
            }
            if self.s[self.i..].starts_with(b"$val") {
                out.push_str(&self.val.as_string());
                self.i += 4;
                continue;
            }
            // `$1`..`$9` from the last match.
            if self.s[self.i] == b'$' && self.s.get(self.i + 1).is_some_and(u8::is_ascii_digit) {
                let idx = (self.s[self.i + 1] - b'0') as usize;
                self.i += 2;
                out.push_str(self.captures.get(idx.checked_sub(1)?)?);
                continue;
            }
            if self.s[self.i..].starts_with(b"$$self{") || self.s[self.i..].starts_with(b"$self->{") {
                if !self.eat("$$self{") {
                    self.eat("$self->{");
                }
                let name = self.ident()?;
                if !self.eat("}") {
                    return None;
                }
                out.push_str(&self.state.member(&name)?.as_string());
                continue;
            }
            // A `my` variable, or a whole array joined by `$"`. An `@` that no
            // name follows is literal text, as it is in Perl.
            if self.s[self.i] == b'$'
                || (self.s[self.i] == b'@'
                    && self.s.get(self.i + 1).is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_'))
            {
                out.push_str(&self.variable()?.as_string());
                continue;
            }
            let c = self.s[self.i];
            out.push(c as char);
            self.i += 1;
        }
        None
    }

    /// A few of ExifTool's own printers, where this crate already implements
    /// them. Written out by name so an unrecognised one still declines rather
    /// than being approximated: `PrintExposureTime` turning 0.0496 into `1/20`
    /// is not something a generic evaluator can guess.
    /// The right-hand side of `=~`: a substitution, a transliteration, or a
    /// match. Perl's `s///` and `tr///` change the variable and return a count;
    /// these conversions then hand `$val` back, so the change has to stick.
    fn bind_operation(&mut self, target: &LValue, negated: bool) -> Option<Val> {
        let subject = self.read_lvalue(target)?;
        let before = self.i;
        let r = self.bind_to_value(&subject, negated)?;
        // `s///` and `tr///` rewrote the subject; put it back where it came
        // from, which is what makes `$val =~ s/ +$//; $val` work.
        if self.i != before {
            let rewritten = std::mem::replace(&mut self.subject_after, None);
            if let Some(v) = rewritten {
                self.write_lvalue(target, v)?;
            }
        }
        Some(r)
    }

    fn bind_to_value(&mut self, subject: &Val, negated: bool) -> Option<Val> {
        self.subject_after = None;
        self.skip_ws();
        if self.peek("s/") {
            self.eat("s/");
            let pat = self.delimited('/')?;
            let rep = self.delimited('/')?;
            let flags = self.regex_flags();
            let re = build_regex(&pat, &flags)?;
            let subject = subject.as_string();
            let replaced = if flags.contains('g') {
                re.replace_all(&subject, perl_replacement(&rep).as_str()).into_owned()
            } else {
                re.replace(&subject, perl_replacement(&rep).as_str()).into_owned()
            };
            let changed = replaced != subject;
            self.subject_after = Some(Val::Str(replaced));
            return Some(Val::Num(if changed { 1.0 } else { 0.0 }));
        }
        if self.peek("tr/") || self.peek("y/") {
            let op = if self.peek("tr/") { "tr/" } else { "y/" };
            self.eat(op);
            let from = self.delimited('/')?;
            let to = self.delimited('/')?;
            self.regex_flags();
            let f: Vec<char> = from.chars().collect();
            let t: Vec<char> = to.chars().collect();
            let mut n = 0usize;
            let out: String = subject
                .as_string()
                .chars()
                .map(|c| match f.iter().position(|x| *x == c) {
                    Some(k) => {
                        n += 1;
                        // Perl repeats the last character when the lists differ.
                        *t.get(k).or_else(|| t.last()).unwrap_or(&c)
                    }
                    None => c,
                })
                .collect();
            self.subject_after = Some(Val::Str(out));
            return Some(Val::Num(n as f64));
        }
        // A bare match, with or without a leading `m`.
        if self.peek("m/") {
            self.eat("m");
        }
        if !self.peek("/") {
            return None;
        }
        self.eat("/");
        let pat = self.delimited('/')?;
        let flags = self.regex_flags();
        let re = build_regex(&pat, &flags)?;
        let subject = subject.as_string();
        let hit = match re.captures(&subject) {
            Some(c) => {
                self.captures = (1..c.len())
                    .map(|i| c.get(i).map(|m| m.as_str().to_string()).unwrap_or_default())
                    .collect();
                true
            }
            None => {
                self.captures.clear();
                false
            }
        };
        Some(Val::Num(if hit != negated { 1.0 } else { 0.0 }))
    }

    /// Read up to the next unescaped delimiter, consuming it.
    fn delimited(&mut self, delim: char) -> Option<String> {
        let mut out = String::new();
        while self.i < self.s.len() {
            let c = self.s[self.i] as char;
            if c == '\\' && self.i + 1 < self.s.len() {
                out.push(c);
                out.push(self.s[self.i + 1] as char);
                self.i += 2;
                continue;
            }
            self.i += 1;
            if c == delim {
                return Some(out);
            }
            out.push(c);
        }
        None
    }

    fn regex_flags(&mut self) -> String {
        let start = self.i;
        while self.i < self.s.len() && (self.s[self.i] as char).is_ascii_alphabetic() {
            self.i += 1;
        }
        String::from_utf8_lossy(&self.s[start..self.i]).into_owned()
    }

    fn helper_call(&mut self) -> Option<Val> {
        let save = self.i;
        // The same function is written three ways depending on the module's age:
        // `Image::ExifTool::Exif::PrintFNumber(...)`, `$self->ConvertDateTime(...)`
        // and a bare `ConvertBitrate(...)`. Strip whichever prefix is there and
        // match on the name alone.
        self.skip_ws();
        if self.peek("$self->") {
            self.eat("$self->");
        } else if self.peek("Image::ExifTool::") {
            self.eat("Image::ExifTool::");
            // Skip the module qualifier, e.g. `Exif::`.
            while self.i < self.s.len() {
                if self.s[self.i..].starts_with(b"::") {
                    self.i += 2;
                    break;
                }
                if !(self.s[self.i] as char).is_alphanumeric() && self.s[self.i] != b'_' {
                    self.i = save;
                    return None;
                }
                self.i += 1;
            }
        }
        let name_start = self.i;
        while self.i < self.s.len()
            && ((self.s[self.i] as char).is_alphanumeric() || self.s[self.i] == b'_')
        {
            self.i += 1;
        }
        let name = std::str::from_utf8(&self.s[name_start..self.i]).ok()?.to_string();
        if name.is_empty() || !self.eat("(") {
            self.i = save;
            return None;
        }
        let mut args = vec![self.expr()?];
        while self.eat(",") {
            args.push(self.expr()?);
        }
        if !self.eat(")") {
            self.i = save;
            return None;
        }
        match call_helper(&name, &args) {
            Some(v) => Some(v),
            None => {
                self.i = save;
                None
            }
        }
    }

    /// Perl's list and named-unary operators, which ExifTool calls as often
    /// without parentheses as with: `join " ", split "\0", substr($val, 8)`.
    ///
    /// The two families differ in how far right they reach. A list operator
    /// swallows every comma to its right, so `split` there takes both the
    /// pattern and the substring; a named unary takes one argument and lets
    /// the comma go to whoever asked for the list.
    fn list_op(&mut self) -> Option<Val> {
        const LIST: &[&str] = &["split", "join", "unpack", "pack", "reverse", "substr", "sprintf"];
        const UNARY: &[&str] = &[
            "length", "hex", "oct", "ord", "chr", "lc", "uc", "lcfirst", "ucfirst",
        ];

        let save = self.i;
        self.skip_ws();
        let start = self.i;
        while self.i < self.s.len()
            && ((self.s[self.i] as char).is_ascii_alphanumeric() || self.s[self.i] == b'_')
        {
            self.i += 1;
        }
        let name = std::str::from_utf8(&self.s[start..self.i]).ok()?.to_string();
        let is_list = LIST.contains(&name.as_str());
        if (!is_list && !UNARY.contains(&name.as_str())) || self.s[self.i..].starts_with(b"::") {
            self.i = save;
            return None;
        }
        let paren = self.eat("(");

        // `split`'s first argument is a pattern, and it is the one place these
        // conversions write a bare regex literal.
        let mut split_pat: Option<(String, bool)> = None;
        if name == "split" {
            self.skip_ws();
            if self.peek("/") {
                self.eat("/");
                let pat = self.delimited('/')?;
                self.regex_flags();
                split_pat = Some((pat, false));
            } else {
                let v = self.expr()?;
                let text = v.as_string();
                // A pattern of one literal space is Perl's awk mode: leading
                // whitespace goes, and any run of it separates.
                let awk = text == " ";
                split_pat = Some((text, awk));
            }
            if !self.eat(",") {
                self.i = save;
                return None;
            }
        }

        let mut args: Vec<Val> = Vec::new();
        if !(paren && self.peek(")")) {
            if is_list {
                args.push(self.expr()?);
                while self.eat(",") {
                    self.skip_ws();
                    if paren && self.peek(")") {
                        break; // a trailing comma
                    }
                    args.push(self.expr()?);
                }
            } else {
                args.push(self.additive()?);
            }
        }
        if paren && !self.eat(")") {
            self.i = save;
            return None;
        }
        match apply_list_op(&name, split_pat.as_ref(), &args) {
            Some(v) => Some(v),
            None => {
                self.i = save;
                None
            }
        }
    }

    fn sprintf_call(&mut self) -> Option<Val> {
        self.eat("sprintf");
        if !self.eat("(") {
            return None;
        }
        self.skip_ws();
        if !self.eat("\"") {
            return None;
        }
        let start = self.i;
        while self.i < self.s.len() && self.s[self.i] != b'"' {
            self.i += 1;
        }
        let fmt = std::str::from_utf8(&self.s[start..self.i]).ok()?.to_string();
        if !self.eat("\"") {
            return None;
        }
        let mut args = Vec::new();
        while self.eat(",") {
            args.push(self.expr()?);
        }
        if !self.eat(")") {
            return None;
        }
        format_sprintf(&fmt, &args).map(Val::Str)
    }
}

/// The subset of Perl's `sprintf` these conversions use: `%d`, `%f` with an
/// optional width and precision, `%s`, `%x`, and a leading `+`.
fn format_sprintf(fmt: &str, args: &[Val]) -> Option<String> {
    let mut out = String::new();
    let mut it = fmt.chars().peekable();
    // `sprintf("%d.%d%c", @a)` passes one array, and Perl sees three arguments.
    let args = flatten(args);
    let mut arg = args.iter();
    while let Some(c) = it.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        if it.peek() == Some(&'%') {
            it.next();
            out.push('%');
            continue;
        }
        let mut plus = false;
        let mut zero = false;
        let mut width = String::new();
        let mut prec: Option<usize> = None;
        while let Some(&c) = it.peek() {
            match c {
                '+' => {
                    plus = true;
                    it.next();
                }
                '0' if width.is_empty() => {
                    zero = true;
                    it.next();
                }
                '1'..='9' | '0' => {
                    width.push(c);
                    it.next();
                }
                '.' => {
                    it.next();
                    let mut p = String::new();
                    while let Some(&d) = it.peek() {
                        if d.is_ascii_digit() {
                            p.push(d);
                            it.next();
                        } else {
                            break;
                        }
                    }
                    prec = Some(p.parse().unwrap_or(0));
                }
                _ => break,
            }
        }
        let conv = it.next()?;
        let v = arg.next()?;
        let mut s = match conv {
            'd' | 'i' => {
                // Perl truncates towards zero here; it does not round.
                let n = v.as_num().trunc() as i64;
                if plus && n >= 0 { format!("+{n}") } else { format!("{n}") }
            }
            // `%c` is the character with that code.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            'c' => char::from_u32(v.as_num() as u32)?.to_string(),
            'f' => {
                let n = v.as_num();
                let p = prec.unwrap_or(6);
                if plus && n >= 0.0 {
                    format!("+{n:.p$}")
                } else {
                    format!("{n:.p$}")
                }
            }
            'g' | 'G' => {
                // Perl's %g: significant digits, trailing zeros trimmed.
                let n = v.as_num();
                let p = prec.unwrap_or(6).max(1);
                let mut t = format!("{n:.*e}", p - 1);
                if let Some((m, e)) = t.split_once('e') {
                    let exp: i32 = e.parse().unwrap_or(0);
                    if exp >= -4 && exp < p as i32 {
                        let dec = (p as i32 - 1 - exp).max(0) as usize;
                        t = format!("{n:.dec$}");
                        if t.contains('.') {
                            t = t.trim_end_matches('0').trim_end_matches('.').to_string();
                        }
                    } else {
                        let m = m.trim_end_matches('0').trim_end_matches('.');
                        t = format!("{m}e{exp:+03}");
                    }
                }
                t
            }
            'x' => format!("{:x}", v.as_num().round() as i64),
            'X' => format!("{:X}", v.as_num().round() as i64),
            's' => v.as_string(),
            _ => return None,
        };
        if let Ok(w) = width.parse::<usize>() {
            while s.len() < w {
                if zero {
                    let rest = s.clone();
                    s = format!("0{rest}");
                } else {
                    s.insert(0, ' ');
                }
            }
        }
        out.push_str(&s);
    }
    Some(out)
}

/// Compile a Perl pattern for regex-lite, declining what it cannot express.
fn build_regex(pat: &str, flags: &str) -> Option<Regex> {
    // Perl's \Z and \z both mean end-of-string here; regex-lite spells it $.
    // Perl writes a NUL in a pattern as \0; regex-lite wants \x00. Several
    // conversions strip trailing NULs with `s/[ \0]+$//`.
    let mut p = pat
        .replace("\\Z", "$")
        .replace("\\z", "$")
        .replace("\\0", "\\x00");
    if flags.contains('i') {
        p = format!("(?i){p}");
    }
    Regex::new(&p).ok()
}

/// Perl writes capture references as `$1`; regex-lite wants `${1}`.
fn perl_replacement(rep: &str) -> String {
    let mut out = String::new();
    let b: Vec<char> = rep.chars().collect();
    let mut i = 0;
    while i < b.len() {
        // A backreference, spelled `$1` or `\1`, becomes the form regex-lite
        // reads. Everything else a backslash protects is literal text, and
        // Perl drops the backslash: `\.` in a replacement is a full stop.
        if (b[i] == '$' || b[i] == '\\') && i + 1 < b.len() && b[i + 1].is_ascii_digit() {
            out.push_str(&format!("${{{}}}", b[i + 1]));
            i += 2;
            continue;
        }
        if b[i] == '\\' && i + 1 < b.len() {
            let rest: String = b[i + 1..].iter().collect();
            if let Some((c, len)) = string_escape(rest.as_bytes()) {
                out.push(c);
                i += 1 + len;
                continue;
            }
        }
        // A lone `$` is literal here, but `$` is how a replacement names a
        // group, so it has to be doubled on the way out.
        if b[i] == '$' {
            out.push_str("$$");
            i += 1;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// The ExifTool helpers this crate implements, by the name its conversions use.
///
/// A name that is not here declines, so the caller keeps the raw value: the
/// alternative is a plausible-looking number produced by the wrong formula.
/// Perl flattens nested lists into their elements when a list operator reads
/// them, and so must we before joining or packing.
fn flatten(args: &[Val]) -> Vec<Val> {
    let mut out = Vec::new();
    for a in args {
        match a {
            Val::List(items) => out.extend(flatten(items)),
            other => out.push(other.clone()),
        }
    }
    out
}

/// A Perl string used as binary data is a string of bytes. Ours arrives as a
/// Rust `String`, so a character above 0xFF means it was built from something
/// other than the raw bytes and we cannot say what those bytes were: refuse
/// rather than invent them.
fn perl_bytes(v: &Val) -> Option<Vec<u8>> {
    v.as_string()
        .chars()
        .map(|c| u8::try_from(c as u32).ok())
        .collect()
}

fn from_bytes(b: &[u8]) -> String {
    b.iter().map(|c| *c as char).collect()
}

fn apply_list_op(name: &str, split_pat: Option<&(String, bool)>, args: &[Val]) -> Option<Val> {
    Some(match name {
        "split" => {
            let (pat, awk) = split_pat?;
            Val::List(
                perl_split(pat, *awk, &args.first()?.as_string(), args.get(1).map(Val::as_num))?
                    .into_iter()
                    .map(Val::Str)
                    .collect(),
            )
        }
        "join" => {
            let sep = args.first()?.as_string();
            Val::Str(
                flatten(&args[1..])
                    .iter()
                    .map(Val::as_string)
                    .collect::<Vec<_>>()
                    .join(&sep),
            )
        }
        "reverse" => {
            let mut items = flatten(args);
            items.reverse();
            Val::List(items)
        }
        "unpack" => Val::List(unpack_template(
            &args.first()?.as_string(),
            &perl_bytes(args.get(1)?)?,
        )?),
        "pack" => Val::Str(from_bytes(&pack_template(
            &args.first()?.as_string(),
            &flatten(&args[1..]),
        )?)),
        "sprintf" => Val::Str(format_sprintf(&args.first()?.as_string(), &args[1..])?),
        "substr" => {
            let chars: Vec<char> = args.first()?.as_string().chars().collect();
            let len = i64::try_from(chars.len()).ok()?;
            #[allow(clippy::cast_possible_truncation)]
            let mut off = args.get(1)?.as_num() as i64;
            if off < 0 {
                off += len;
            }
            let off = off.clamp(0, len);
            let end = match args.get(2) {
                #[allow(clippy::cast_possible_truncation)]
                Some(n) => {
                    let n = n.as_num() as i64;
                    // A negative length means "stop that far from the end".
                    if n < 0 { (len + n).max(off) } else { (off + n).min(len) }
                }
                None => len,
            };
            Val::Str(chars[usize::try_from(off).ok()?..usize::try_from(end).ok()?].iter().collect())
        }
        #[allow(clippy::cast_precision_loss)]
        "length" => Val::Num(args.first()?.as_string().chars().count() as f64),
        "hex" => {
            let t = args.first()?.as_string();
            let t = t.trim();
            let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
            #[allow(clippy::cast_precision_loss)]
            Val::Num(u64::from_str_radix(t, 16).ok()? as f64)
        }
        "oct" => {
            let t = args.first()?.as_string();
            let t = t.trim();
            #[allow(clippy::cast_precision_loss)]
            let n = if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                u64::from_str_radix(h, 16).ok()?
            } else if let Some(b) = t.strip_prefix("0b").or_else(|| t.strip_prefix("0B")) {
                u64::from_str_radix(b, 2).ok()?
            } else {
                u64::from_str_radix(t.trim_start_matches('0'), 8).unwrap_or(0)
            };
            Val::Num(n as f64)
        }
        "ord" => {
            #[allow(clippy::cast_precision_loss)]
            Val::Num(args.first()?.as_string().chars().next().map_or(0.0, |c| c as u32 as f64))
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        "chr" => Val::Str(char::from_u32(args.first()?.as_num() as u32)?.to_string()),
        "lc" => Val::Str(args.first()?.as_string().to_lowercase()),
        "uc" => Val::Str(args.first()?.as_string().to_uppercase()),
        "lcfirst" | "ucfirst" => {
            let t = args.first()?.as_string();
            let mut it = t.chars();
            match it.next() {
                None => Val::Str(t),
                Some(c) => {
                    let head: String = if name == "lcfirst" {
                        c.to_lowercase().collect()
                    } else {
                        c.to_uppercase().collect()
                    };
                    Val::Str(head + it.as_str())
                }
            }
        }
        _ => return None,
    })
}

/// Perl's `split`. Without a limit the trailing empty fields are dropped, which
/// is what makes `split "\0", $val` on a NUL-terminated string give the strings
/// and not a run of blanks after them.
fn perl_split(pat: &str, awk: bool, subject: &str, limit: Option<f64>) -> Option<Vec<String>> {
    #[allow(clippy::cast_possible_truncation)]
    let limit = limit.map_or(0i64, |n| n as i64);
    let mut fields: Vec<String> = Vec::new();
    if awk {
        fields = subject.split_whitespace().map(str::to_string).collect();
    } else if pat.is_empty() {
        fields = subject.chars().map(|c| c.to_string()).collect();
    } else {
        let re = build_regex(pat, "")?;
        let mut last = 0usize;
        for m in re.find_iter(subject) {
            // A zero-width match would loop; Perl steps past it.
            if m.end() == m.start() && m.start() == last {
                continue;
            }
            if limit > 0 && i64::try_from(fields.len()).ok()? + 1 >= limit {
                break;
            }
            fields.push(subject[last..m.start()].to_string());
            last = m.end();
        }
        fields.push(subject[last..].to_string());
    }
    if limit == 0 {
        while fields.last().is_some_and(String::is_empty) {
            fields.pop();
        }
    }
    Some(fields)
}

/// One `letter[count|*]` item of a pack/unpack template.
fn template_items(tmpl: &str) -> Option<Vec<(char, Option<usize>)>> {
    let mut out = Vec::new();
    let mut it = tmpl.chars().peekable();
    while let Some(c) = it.next() {
        if c.is_whitespace() {
            continue;
        }
        if !c.is_ascii_alphabetic() && c != '@' {
            return None;
        }
        let mut count: Option<usize> = Some(1);
        if it.peek() == Some(&'*') {
            it.next();
            count = None; // "as many as there are"
        } else if it.peek().is_some_and(char::is_ascii_digit) {
            let mut n = String::new();
            while it.peek().is_some_and(char::is_ascii_digit) {
                n.push(it.next()?);
            }
            count = Some(n.parse().ok()?);
        }
        out.push((c, count));
    }
    Some(out)
}

/// Perl's `unpack`, for the templates ExifTool's conversions use.
///
/// `s S l L q Q f d` are Perl's *native* forms, so their byte order is the
/// machine's. ExifTool's own output — the baseline this crate is measured
/// against — comes from Perl on x86, so native means little-endian here.
/// An unknown letter refuses the whole conversion rather than guessing.
#[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
fn unpack_template(tmpl: &str, data: &[u8]) -> Option<Vec<Val>> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    for (letter, count) in template_items(tmpl)? {
        let left = data.len().saturating_sub(pos);
        match letter {
            'a' | 'A' | 'Z' => {
                let n = count.unwrap_or(left).min(left);
                let raw = &data[pos..pos + n];
                pos += n;
                let text = from_bytes(raw);
                out.push(Val::Str(match letter {
                    'A' => text.trim_end_matches([' ', '\0']).to_string(),
                    'Z' => text.split('\0').next().unwrap_or_default().to_string(),
                    _ => text,
                }));
            }
            'H' | 'h' => {
                let digits = count.unwrap_or(left * 2).min(left * 2);
                let mut text = String::new();
                for k in 0..digits {
                    let b = data[pos + k / 2];
                    let nyb = if (k % 2 == 0) == (letter == 'H') { b >> 4 } else { b & 0x0f };
                    text.push(char::from_digit(u32::from(nyb), 16)?);
                }
                pos += digits.div_ceil(2);
                out.push(Val::Str(text));
            }
            'x' => pos += count.unwrap_or(left).min(left),
            'X' => pos = pos.saturating_sub(count.unwrap_or(pos)),
            '@' => pos = count.unwrap_or(pos),
            _ => {
                let size = match letter {
                    'C' | 'c' => 1,
                    'n' | 'v' | 's' | 'S' => 2,
                    'N' | 'V' | 'l' | 'L' | 'f' => 4,
                    'q' | 'Q' | 'd' => 8,
                    _ => return None,
                };
                let n = count.unwrap_or(left / size);
                for _ in 0..n {
                    if pos + size > data.len() {
                        return Some(out); // Perl returns a short list, not an error
                    }
                    let b = &data[pos..pos + size];
                    pos += size;
                    out.push(Val::Num(match letter {
                        'C' => f64::from(b[0]),
                        'c' => f64::from(b[0] as i8),
                        'n' => f64::from(u16::from_be_bytes([b[0], b[1]])),
                        'v' | 'S' => f64::from(u16::from_le_bytes([b[0], b[1]])),
                        's' => f64::from(i16::from_le_bytes([b[0], b[1]])),
                        'N' => f64::from(u32::from_be_bytes([b[0], b[1], b[2], b[3]])),
                        'V' | 'L' => f64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
                        'l' => f64::from(i32::from_le_bytes([b[0], b[1], b[2], b[3]])),
                        'f' => f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
                        'd' => f64::from_le_bytes(b.try_into().ok()?),
                        'q' => i64::from_le_bytes(b.try_into().ok()?) as f64,
                        _ => u64::from_le_bytes(b.try_into().ok()?) as f64,
                    }));
                }
            }
        }
    }
    Some(out)
}

/// Perl's `pack`, the inverse of the templates above.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn pack_template(tmpl: &str, args: &[Val]) -> Option<Vec<u8>> {
    let mut out: Vec<u8> = Vec::new();
    let mut it = args.iter();
    for (letter, count) in template_items(tmpl)? {
        match letter {
            'a' | 'A' | 'Z' => {
                let text = perl_bytes(it.next()?)?;
                let n = count.unwrap_or(text.len() + usize::from(letter == 'Z'));
                let pad = if letter == 'A' { b' ' } else { 0 };
                for k in 0..n {
                    out.push(text.get(k).copied().unwrap_or(pad));
                }
            }
            'H' | 'h' => {
                let text = it.next()?.as_string();
                let digits: Vec<u32> = text.chars().map(|c| c.to_digit(16).unwrap_or(0)).collect();
                let n = count.unwrap_or(digits.len());
                let mut byte = 0u8;
                for k in 0..n {
                    let nyb = digits.get(k).copied().unwrap_or(0) as u8;
                    if (k % 2 == 0) == (letter == 'H') {
                        byte = nyb << 4;
                    } else {
                        byte |= nyb;
                    }
                    if k % 2 == 1 {
                        out.push(byte);
                        byte = 0;
                    }
                }
                if n % 2 == 1 {
                    out.push(byte);
                }
            }
            'x' => out.extend(std::iter::repeat_n(0u8, count.unwrap_or(1))),
            _ => {
                let n = count.unwrap_or_else(|| it.len());
                for _ in 0..n {
                    let v = it.next()?.as_num();
                    match letter {
                        'C' | 'c' => out.push(v as i64 as u8),
                        'n' => out.extend((v as i64 as u16).to_be_bytes()),
                        'v' | 'S' | 's' => out.extend((v as i64 as u16).to_le_bytes()),
                        'N' => out.extend((v as i64 as u32).to_be_bytes()),
                        'V' | 'L' | 'l' => out.extend((v as i64 as u32).to_le_bytes()),
                        'f' => out.extend((v as f32).to_le_bytes()),
                        'd' => out.extend(v.to_le_bytes()),
                        _ => return None,
                    }
                }
            }
        }
    }
    Some(out)
}

/// The escape sequences a double-quoted Perl string can carry. Returns the
/// character and how many bytes of the source it took after the backslash.
fn string_escape(rest: &[u8]) -> Option<(char, usize)> {
    let c = *rest.first()?;
    if c == b'x' {
        // `\x{263a}` names a code point; `\xNN` names a byte.
        if rest.get(1) == Some(&b'{') {
            let end = rest.iter().position(|b| *b == b'}')?;
            let hex = std::str::from_utf8(&rest[2..end]).ok()?;
            return Some((char::from_u32(u32::from_str_radix(hex, 16).ok()?)?, end + 1));
        }
        let mut n = 0usize;
        while n < 2 && rest.get(1 + n).is_some_and(u8::is_ascii_hexdigit) {
            n += 1;
        }
        let hex = std::str::from_utf8(&rest[1..1 + n]).ok()?;
        return Some((char::from_u32(u32::from_str_radix(hex, 16).ok()?)?, 1 + n));
    }
    Some((
        match c {
            b'n' => '\n',
            b't' => '\t',
            b'r' => '\r',
            b'0' => '\0',
            b'e' => '\u{1b}',
            b'a' => '\u{7}',
            b'f' => '\u{c}',
            // Perl drops the backslash from anything else.
            other => other as char,
        },
        1,
    ))
}

fn call_helper(name: &str, args: &[Val]) -> Option<Val> {
    let first = args.first()?;
    Some(match name {
        // Without a -d format ExifTool returns the date untouched, and that is
        // the default. This is the single most-used conversion it has.
        "ConvertDateTime" => first.clone(),
        "PrintExposureTime" => Val::Str(print_exposure_time(first.as_num())),
        "PrintFNumber" => Val::Str(print_f_number(first.as_num())),
        "PrintFraction" => Val::Str(crate::tags::exif::print_fraction(first.as_num())),
        "ConvertDuration" => Val::Str(convert_duration(first)?),
        "ConvertBitrate" => Val::Str(convert_bitrate(first)?),
        "ConvertUnixTime" => Val::Str(convert_unix_time(first.as_num())),
        // GPS::ToDMS takes the ExifTool object first, so the coordinate is the
        // second argument and the hemisphere the fourth when present.
        "ToDMS" => Val::Str(to_dms(
            args.get(1)?.as_num(),
            args.get(3).map(Val::as_string).as_deref(),
        )),
        "ToDegrees" => Val::Num(to_degrees(&first.as_string())?),
        "ExifDate" => Val::Str(exif_date(&first.as_string())),
        "ExifTime" => Val::Str(exif_time(&first.as_string())),
        // Nikon.pm PrintPC: four sentinel values, then a signed format.
        "PrintPC" => {
            let v = first.as_num();
            let norm = args.get(1).map(Val::as_string).unwrap_or_default();
            let fmt = args.get(2).map(Val::as_string).unwrap_or_default();
            let div = args.get(3).map_or(1.0, Val::as_num);
            if v == 0.0 {
                Val::Str(if norm.is_empty() { "Normal".into() } else { norm })
            } else if v == 127.0 {
                Val::Str("n/a".into())
            } else if v == -128.0 {
                Val::Str("Auto".into())
            } else if v == -127.0 {
                Val::Str("User".into())
            } else {
                let f = if fmt.is_empty() { "%+d".to_string() } else { fmt };
                let d = if div == 0.0 { 1.0 } else { div };
                Val::Str(format_sprintf(&f, &[Val::Num(v / d)])?)
            }
        }
        // GPS.pm PrintTimeStamp: trims the fractional seconds to microseconds.
        "PrintTimeStamp" => Val::Str(print_time_stamp(&first.as_string())),
        // XMP.pm ConvertXMPDate: an XMP date back into EXIF's layout.
        "ConvertXMPDate" => Val::Str(convert_xmp_date(&first.as_string())),
        _ => return None,
    })
}

/// ExifTool.pm ConvertUnixTime: seconds since the epoch, in EXIF's own layout.
fn convert_unix_time(t: f64) -> String {
    if t == 0.0 {
        return "0000:00:00 00:00:00".to_string();
    }
    // Days since 1970-01-01, then the civil date from them. No leap seconds,
    // which is what gmtime gives ExifTool too.
    let secs = t.trunc() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (h, mi, sec) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}:{m:02}:{d:02} {h:02}:{mi:02}:{sec:02}")
}

/// GPS.pm ToDMS with the default CoordFormat: `48 deg 51' 30.24" N`.
///
/// A negative coordinate flips the hemisphere rather than carrying a sign,
/// which is why the reference has to be known here and not applied afterwards.
fn to_dms(val: f64, reference: Option<&str>) -> String {
    let (mut v, suffix) = match reference {
        Some(r) if !r.is_empty() => {
            let flipped = if val < 0.0 {
                match r {
                    "N" => "S",
                    "E" => "W",
                    "S" => "N",
                    "W" => "E",
                    other => other,
                }
            } else {
                r
            };
            (val.abs(), format!(" {flipped}"))
        }
        _ => (val.abs(), String::new()),
    };
    let d = v.trunc();
    v = (v - d) * 60.0;
    let m = v.trunc();
    let sec = (v - m) * 60.0;
    // Rounding can carry the seconds to 60; ExifTool pushes that up a place.
    let (d, m, sec) = if format!("{sec:.2}").starts_with("60") {
        if m + 1.0 >= 60.0 {
            (d + 1.0, 0.0, 0.0)
        } else {
            (d, m + 1.0, 0.0)
        }
    } else {
        (d, m, sec)
    };
    format!("{d} deg {m}' {sec:.2}\"{suffix}")
}

/// GPS.pm ToDegrees: the first three numbers found, as degrees/minutes/seconds.
fn to_degrees(text: &str) -> Option<f64> {
    if text.contains("inf") || text.contains("undef") {
        return None;
    }
    let mut nums = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() && nums.len() < 3 {
        if b[i].is_ascii_digit() || (b[i] == b'.' && i + 1 < b.len() && b[i + 1].is_ascii_digit()) {
            let start = if i > 0 && (b[i - 1] == b'-' || b[i - 1] == b'+') { i - 1 } else { i };
            let mut j = i;
            while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'.') {
                j += 1;
            }
            if let Ok(n) = std::str::from_utf8(&b[start..j]).ok()?.parse::<f64>() {
                nums.push(n);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    if nums.is_empty() {
        return None;
    }
    let deg = nums.first().copied().unwrap_or(0.0)
        + nums.get(1).copied().unwrap_or(0.0) / 60.0
        + nums.get(2).copied().unwrap_or(0.0) / 3600.0;
    // A trailing S or W means the southern or western hemisphere.
    let neg = text
        .rsplit(|c: char| c.is_whitespace())
        .next()
        .is_some_and(|t| t.eq_ignore_ascii_case("S") || t.eq_ignore_ascii_case("W"));
    Some(if neg { -deg } else { deg })
}

/// GPS.pm PrintTimeStamp: seconds kept to microseconds, zero-padded below ten.
fn print_time_stamp(val: &str) -> String {
    let Some(pos) = val.rfind(':') else { return val.to_string() };
    let (head, tail) = val.split_at(pos);
    let secs = &tail[1..];
    if !secs.contains('.') || secs.parse::<f64>().is_err() {
        return val.to_string();
    }
    let Ok(f) = secs.parse::<f64>() else { return val.to_string() };
    let rounded = (f * 1_000_000.0 + 0.5).trunc() / 1_000_000.0;
    let mut s = format_number(rounded);
    if rounded < 10.0 {
        s = format!("0{s}");
    }
    format!("{head}:{s}")
}

/// XMP.pm ConvertXMPDate: `2026-08-27T11:22:02+02:00` becomes EXIF's
/// `2026:08:27 11:22:02+02:00`.
fn convert_xmp_date(val: &str) -> String {
    let b: Vec<char> = val.chars().collect();
    let is_full = b.len() >= 16
        && b[..4].iter().all(char::is_ascii_digit)
        && b[4] == '-'
        && b[5..7].iter().all(char::is_ascii_digit)
        && b[7] == '-'
        && b[8..10].iter().all(char::is_ascii_digit)
        && (b[10] == 'T' || b[10] == ' ');
    if is_full {
        let date: String = b[..10].iter().collect::<String>().replace('-', ":");
        let rest: String = b[11..].iter().collect();
        return format!("{date} {}", rest.trim_start());
    }
    // A bare date, or a year-month: only the separators change.
    if b.len() >= 4 && b[..4].iter().all(char::is_ascii_digit) {
        return val.replace('-', ":");
    }
    val.to_string()
}

/// Exif.pm ExifDate: eight digits, however they were separated, become
/// `YYYY:MM:DD`.
fn exif_date(date: &str) -> String {
    let d = date.trim_end_matches('\0');
    let digits: Vec<char> = d.chars().filter(char::is_ascii_digit).collect();
    if digits.len() == 8 {
        let s: String = digits.iter().collect();
        return format!("{}:{}:{}", &s[0..4], &s[4..6], &s[6..8]);
    }
    d.to_string()
}

/// Exif.pm ExifTime: spaces become colons, and six digits gain separators.
fn exif_time(time: &str) -> String {
    let t = time.trim_end_matches('\0').replace(' ', ":");
    let digits: Vec<char> = t.chars().filter(char::is_ascii_digit).collect();
    if digits.len() == 6 && !t.contains(':') {
        let s: String = digits.iter().collect();
        return format!("{}:{}:{}", &s[0..2], &s[2..4], &s[4..6]);
    }
    t
}

/// ExifTool.pm ConvertDuration: seconds to `1.23 s`, `1:02:03`, or with days.
fn convert_duration(v: &Val) -> Option<String> {
    let Val::Num(_) = v else {
        // Not a number: ExifTool returns it unchanged, and so must we.
        return Some(v.as_string());
    };
    let mut t = v.as_num();
    if t == 0.0 {
        return Some("0 s".to_string());
    }
    let sign = if t > 0.0 {
        String::new()
    } else {
        t = -t;
        "-".to_string()
    };
    if t < 30.0 {
        return Some(format!("{sign}{t:.2} s"));
    }
    t += 0.5; // round to the nearest second
    let mut h = (t / 3600.0).trunc();
    t -= h * 3600.0;
    let m = (t / 60.0).trunc();
    t -= m * 60.0;
    let mut prefix = sign;
    if h > 24.0 {
        let d = (h / 24.0).trunc();
        h -= d * 24.0;
        prefix = format!("{prefix}{d} days ");
    }
    Some(format!("{prefix}{h}:{m:02}:{:02}", t.trunc()))
}

/// ExifTool.pm ConvertBitrate: bps through Gbps, three significant digits below
/// 100 and none above.
fn convert_bitrate(v: &Val) -> Option<String> {
    let Val::Num(_) = v else {
        return Some(v.as_string());
    };
    let mut b = v.as_num();
    for (i, unit) in ["bps", "kbps", "Mbps", "Gbps"].into_iter().enumerate() {
        if b >= 1000.0 && i < 3 {
            b /= 1000.0;
            continue;
        }
        return Some(if b < 100.0 {
            format!("{} {unit}", format_sprintf("%.3g", &[Val::Num(b)])?)
        } else {
            format!("{b:.0} {unit}")
        });
    }
    None
}

/// Exif.pm PrintExposureTime: fractions below a second, plain seconds above.
fn print_exposure_time(val: f64) -> String {
    if val <= 0.0 {
        return "0".to_string();
    }
    if val >= 0.25001 {
        // Perl: sprintf("%.2g", $val) once the value is no longer a short fraction.
        let s = format!("{val:.2}");
        return s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    format!("1/{}", (1.0 / val).round() as u64)
}

/// Exif.pm PrintFNumber: one decimal, trimmed when whole.
fn print_f_number(val: f64) -> String {
    if val <= 0.0 {
        return format_number(val);
    }
    // Exif.pm: one decimal, or two below 1.0. It does not trim them.
    if val < 1.0 {
        format!("{val:.2}")
    } else {
        format!("{val:.1}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(v: f64) -> Val {
        Val::Num(v)
    }

    #[test]
    fn the_sony_exposure_time_and_f_number_formulas() {
        // Sony.pm, Tag9050b: the two that sent me here.
        // The raw 5205 is 1/20 s, which is what ExifTool prints for this frame.
        let e = eval("$val ? 2 ** (16 - $val/256) : 0", &n(5205.0)).unwrap();
        assert!((1.0 / e.as_num() - 20.14).abs() < 0.01, "got {}", e.as_num());
        assert_eq!(eval("$val ? 2 ** (16 - $val/256) : 0", &n(0.0)).unwrap(), Val::Num(0.0));
        let f = eval("2 ** (($val/256 - 16) / 2)", &n(5632.0)).unwrap();
        assert!((f.as_num() - 8.0).abs() < 1e-9, "got {}", f.as_num());
    }

    #[test]
    fn power_binds_tighter_than_multiplication_and_goes_right() {
        assert_eq!(eval("100 * 2 ** 2", &n(0.0)).unwrap().as_num(), 400.0);
        assert_eq!(eval("2 ** 3 ** 2", &n(0.0)).unwrap().as_num(), 512.0);
    }

    #[test]
    fn interpolation_and_sprintf() {
        assert_eq!(eval("\"$val mm\"", &n(35.0)).unwrap().as_string(), "35 mm");
        assert_eq!(
            eval("sprintf(\"%.1f mm\",$val)", &n(3.14159)).unwrap().as_string(),
            "3.1 mm"
        );
        assert_eq!(
            eval("sprintf(\"%+d\",$val)", &n(3.0)).unwrap().as_string(),
            "+3"
        );
    }

    #[test]
    fn ternary_and_comparison() {
        assert_eq!(eval("$val > 0 ? \"+$val\" : $val", &n(2.0)).unwrap().as_string(), "+2");
        assert_eq!(eval("$val > 0 ? \"+$val\" : $val", &n(-2.0)).unwrap().as_string(), "-2");
        assert_eq!(eval("$val ? 1 : 0", &n(0.0)).unwrap().as_num(), 0.0);
    }

    /// Every expected value here came out of Perl ExifTool, not out of my head.
    #[test]
    fn nikon_picture_control_and_the_date_reshapers() {
        let pc = |v: f64| {
            eval("Image::ExifTool::Nikon::PrintPC($val,\"None\",\"%.2f\",4)", &n(v))
                .unwrap()
                .as_string()
        };
        assert_eq!(pc(0.0), "None");
        assert_eq!(pc(127.0), "n/a");
        assert_eq!(pc(-128.0), "Auto");
        assert_eq!(pc(8.0), "2.00");
        assert_eq!(
            eval("Image::ExifTool::XMP::ConvertXMPDate($val)", &Val::Str("2026-08-27T11:22:02+02:00".into()))
                .unwrap().as_string(),
            "2026:08:27 11:22:02+02:00"
        );
        assert_eq!(
            eval("Image::ExifTool::GPS::PrintTimeStamp($val)", &Val::Str("11:22:02.500000".into()))
                .unwrap().as_string(),
            "11:22:02.5"
        );
    }

    #[test]
    fn nul_in_a_pattern_and_unpack_hex() {
        assert_eq!(
            eval("$val =~ s/[ \\0]+$//; $val", &Val::Str("abc \0\0".into()))
                .unwrap()
                .as_string(),
            "abc"
        );
        // A binary value is a string of bytes, so U+00FE is the byte 0xfe --
        // not the two bytes Rust would encode it as.
        assert_eq!(
            eval("unpack(\"H*\", $val)", &Val::Str("\u{fe}\u{fe}".into()))
                .unwrap()
                .as_string(),
            "fefe"
        );
    }

    /// The logical and bitwise operators, and the branch Perl never runs.
    /// Every expected value was read off Perl itself first.
    #[test]
    fn logic_bits_and_the_branch_not_taken() {
        assert_eq!(
            eval("sprintf(\"%x.%.2x\",$val>>8,$val&0xff)", &n(0x1234 as f64)).unwrap().as_string(),
            "12.34"
        );
        // The division is never run, so it is not a reason to refuse.
        assert_eq!(eval("$val ? 1/$val : \"zero\"", &n(0.0)).unwrap().as_string(), "zero");
        assert_eq!(eval("$val / (0 || 4)", &n(5.0)).unwrap().as_num(), 1.25);
        assert_eq!(eval("7 ^ 2", &n(0.0)).unwrap().as_num(), 5.0);
        assert_eq!(eval("~0 & 0xff", &n(0.0)).unwrap().as_num(), 255.0);
        assert_eq!(eval("1 << 10", &n(0.0)).unwrap().as_num(), 1024.0);
        assert_eq!(eval("3 <=> 5", &n(0.0)).unwrap().as_num(), -1.0);
        assert_eq!(eval("not 0", &n(0.0)).unwrap().as_num(), 1.0);
        assert_eq!(
            eval("$val eq \"x\" ? \"yes\" : \"no\"", &Val::Str("x".into())).unwrap().as_string(),
            "yes"
        );
        assert_eq!(eval("$val ? abs($val) : undef", &n(0.0)).unwrap(), Val::Undef);
    }

    /// The file-level parse state, which a conversion reads off the ExifTool
    /// object. A member the reader does not track declines; one it tracks but
    /// never set is `undef`, and false, exactly as in Perl.
    #[test]
    fn the_parse_state_dictionary() {
        let mut state: std::collections::HashMap<String, Val> = std::collections::HashMap::new();
        state.insert("TimecodeScale".to_string(), Val::Num(1_000_000.0));
        state.insert("Model".to_string(), Val::Str("ILCE-9".into()));
        state.insert("FacesDetected".to_string(), Val::Undef);
        let e = "$$self{TimecodeScale} ? $val * $$self{TimecodeScale} / 1e9 : $val";
        assert_eq!(eval_with(e, &n(500.0), &state).unwrap().as_num(), 0.5);
        assert_eq!(
            eval_with("$self->{Model} =~ /^ILCE/ ? 1 : 0", &n(0.0), &state).unwrap().as_num(),
            1.0
        );
        // Tracked but unset: Perl takes the false branch.
        assert_eq!(
            eval_with("$$self{FacesDetected} ? \"faces\" : \"none\"", &n(0.0), &state)
                .unwrap()
                .as_string(),
            "none"
        );
        // Not tracked at all: refuse rather than answer as if it were unset.
        assert!(eval_with("$$self{Make} ? 1 : 0", &n(0.0), &state).is_none());
    }

    /// `my` variables, the `foreach` modifier that aliases `$_` to each
    /// element, and assignment back into `$val`. Every expected value was read
    /// off Perl itself first.
    #[test]
    fn variables_and_the_foreach_modifier() {
        let s = |e: &str, v: &str| eval(e, &Val::Str(v.into())).unwrap().as_string();
        assert_eq!(s("my @a = split \" \",$val; $_ /= 2 foreach @a; \"@a\"", "1 2 3"), "0.5 1 1.5");
        assert_eq!(
            s("my @v=split \" \",$val; \"$v[0] (min $v[1], max $v[2])\"", "1 2 3"),
            "1 (min 2, max 3)"
        );
        assert_eq!(s("my @v=reverse split(\" \",$val);\"@v\"", "1 2 3"), "3 2 1");
        assert_eq!(
            eval("$val=sprintf(\"%x\",$val);$val=~s/(.{3})$/\\.$1/;$val", &n(4660.0))
                .unwrap()
                .as_string(),
            "1.234"
        );
        assert_eq!(s("my @a = split \" \",$val; sprintf(\"%d.%d%c\",@a)", "1 2 65"), "1.2A");
        assert_eq!(
            eval(
                "my ($a,$b,$c)=unpack(\"c3\",$val); $c ? $a*($b/$c) : 0",
                &Val::Str("\u{a}\u{2}\u{4}".into())
            )
            .unwrap()
            .as_num(),
            5.0
        );
        // `%` truncates its operands and takes the sign of the right-hand one.
        assert_eq!(eval("(-1) % 24", &n(0.0)).unwrap().as_num(), 23.0);
    }

    /// Every expected value here was read off Perl itself before it was
    /// written down.
    #[test]
    fn the_perl_list_operators() {
        let s = |e: &str, v: &str| eval(e, &Val::Str(v.into())).unwrap().as_string();
        assert_eq!(s("join \" \", split \"\\0\", substr($val, 8)", "headerXXab\0cd\0"), "ab cd");
        assert_eq!(s("join \" \", unpack \"H2H2\", $val", "\u{1f}\u{2e}"), "1f 2e");
        assert_eq!(s("join(\" \", unpack(\"H2\"x3, $val))", "\u{ab}\u{cd}\u{ef}"), "ab cd ef");
        assert_eq!(s("join \" \", reverse split(\" \",$val)", "1 2 3"), "3 2 1");
        assert_eq!(s("unpack \"H*\", pack \"C*\", split \" \", $val", "1 2 255"), "0102ff");
        assert_eq!(s("\"0x\" . unpack(\"H*\",$val)", "\u{a}"), "0x0a");
        assert_eq!(s("uc $val", "abc"), "ABC");
        assert_eq!(s("length($val) > 2 ? \"long\" : \"short\"", "abcd"), "long");
        assert_eq!(eval("sprintf \"%g\", $val", &Val::Num(0.5)).unwrap().as_string(), "0.5");
    }

    #[test]
    fn substitutions_transliterations_and_matches() {
        let s = |e: &str, v: &str| eval(e, &Val::Str(v.into())).unwrap().as_string();
        // The commonest shape: change the value, then hand it back.
        assert_eq!(s("$val =~ s/ +$//; $val", "35 mm   "), "35 mm");
        assert_eq!(s("$val=~s/^.*: //;$val", "Lens: 50mm"), "50mm");
        assert_eq!(s("$val =~ tr/ /./; $val", "1 2 3"), "1.2.3");
        assert_eq!(s("$val =~ tr/-/:/; $val", "2026-08-27"), "2026:08:27");
        // A match that captures, used as a condition.
        assert_eq!(
            eval("$val=~/(\\d+)/ ? $1/100 : 1", &Val::Str("abc 250 def".into()))
                .unwrap()
                .as_num(),
            2.5
        );
        assert_eq!(
            eval("$val=~/(\\d+)/ ? $1/100 : 1", &Val::Str("none".into()))
                .unwrap()
                .as_num(),
            1.0
        );
    }

    #[test]
    fn the_gps_and_date_helpers() {
        assert_eq!(
            eval("Image::ExifTool::GPS::ToDMS($self, $val, 1, \"N\")", &n(48.8584)).unwrap().as_string(),
            "48 deg 51' 30.24\" N"
        );
        // A negative latitude turns N into S rather than printing a minus.
        assert_eq!(
            eval("Image::ExifTool::GPS::ToDMS($self, $val, 1, \"E\")", &n(-2.2945)).unwrap().as_string(),
            "2 deg 17' 40.20\" W"
        );
        assert_eq!(
            eval("Image::ExifTool::GPS::ToDMS($self, $val, 1)", &n(48.8584)).unwrap().as_string(),
            "48 deg 51' 30.24\""
        );
        assert_eq!(
            eval("ConvertUnixTime($val)", &n(1_000_000_000.0)).unwrap().as_string(),
            "2001:09:09 01:46:40"
        );
        assert_eq!(eval("ConvertUnixTime($val)", &n(0.0)).unwrap().as_string(), "0000:00:00 00:00:00");
        let deg = eval("Image::ExifTool::GPS::ToDegrees($val, 1)", &Val::Str("48 51 30.24".into())).unwrap();
        assert!((deg.as_num() - 48.8584).abs() < 1e-6, "got {}", deg.as_num());
        assert_eq!(
            eval("Image::ExifTool::Exif::ExifDate($val)", &Val::Str("20260827".into())).unwrap().as_string(),
            "2026:08:27"
        );
        assert_eq!(
            eval("Image::ExifTool::Exif::ExifTime($val)", &Val::Str("11 22 02".into())).unwrap().as_string(),
            "11:22:02"
        );
    }

    #[test]
    fn the_helpers_by_any_of_their_spellings() {
        // ExifTool writes the same call three ways; all three must land.
        for e in [
            "Image::ExifTool::Exif::PrintFNumber($val)",
            "PrintFNumber($val)",
        ] {
            assert_eq!(eval(e, &n(8.0)).unwrap().as_string(), "8.0", "{e}");
        }
        assert_eq!(eval("$self->ConvertDateTime($val)", &Val::Str("2026:08:27 11:22:02".into()))
            .unwrap().as_string(), "2026:08:27 11:22:02");
        assert_eq!(eval("ConvertDuration($val)", &n(0.0)).unwrap().as_string(), "0 s");
        assert_eq!(eval("ConvertDuration($val)", &n(12.5)).unwrap().as_string(), "12.50 s");
        assert_eq!(eval("ConvertDuration($val)", &n(3723.0)).unwrap().as_string(), "1:02:03");
        // Checked against Perl: ConvertBitrate(1500000) is "1.5 Mbps", not "1.50".
        assert_eq!(eval("ConvertBitrate($val)", &n(1_500_000.0)).unwrap().as_string(), "1.5 Mbps");
        assert_eq!(eval("ConvertBitrate($val)", &n(999.0)).unwrap().as_string(), "999 bps");
        // A name we have not ported still declines.
        assert!(eval("ConvertRIFFDate($val)", &n(1.0)).is_none());
    }

    #[test]
    fn scientific_notation() {
        assert_eq!(eval("$val / 1e6", &n(2_500_000.0)).unwrap().as_num(), 2.5);
        assert_eq!(eval("$val * 1.5e-2", &n(200.0)).unwrap().as_num(), 3.0);
        // A bare `e` that is not an exponent must not swallow anything.
        assert!(eval("$val 1e", &n(1.0)).is_none());
    }

    #[test]
    fn hex_literals_and_percent_g() {
        assert_eq!(eval("$val - 0x80", &n(200.0)).unwrap().as_num(), 72.0);
        assert_eq!(
            eval("($val > 0x7 ? $val - 0x10 : $val) / 6", &n(9.0)).unwrap().as_num(),
            -7.0 / 6.0
        );
        assert_eq!(eval("sprintf(\"%.2g\",$val)", &n(0.0499)).unwrap().as_string(), "0.05");
    }

    #[test]
    fn the_helpers_exiftool_prints_with() {
        // The Sony frame that started this: 5205 -> 0.0496 s -> "1/20".
        let v = eval("$val ? 2 ** (16 - $val/256) : 0", &n(5205.0)).unwrap();
        let p = eval(
            "$val ? Image::ExifTool::Exif::PrintExposureTime($val) : \"Bulb\"",
            &v,
        )
        .unwrap();
        assert_eq!(p.as_string(), "1/20");
        // And the zero case still reaches the other branch.
        let bulb = eval(
            "$val ? Image::ExifTool::Exif::PrintExposureTime($val) : \"Bulb\"",
            &n(0.0),
        )
        .unwrap();
        assert_eq!(bulb.as_string(), "Bulb");
    }

    /// Anything outside the grammar must decline, not approximate: the caller
    /// keeps the raw value, which is honest, where a wrong conversion is not.
    #[test]
    fn unsupported_expressions_decline() {
        assert!(eval("Image::ExifTool::ASF::GetGUID($val)", &n(1.0)).is_none());
        assert!(eval("$self->Decode($val, \"UTF8\")", &n(1.0)).is_none());
        assert!(eval("$$self{Model}", &n(1.0)).is_none());
        assert!(eval("$val / 0", &n(1.0)).is_none());
    }
}
