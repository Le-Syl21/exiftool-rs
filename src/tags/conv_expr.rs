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
    /// A reference to a list, which is what `\@val` passes: one argument
    /// rather than its elements.
    Reference(Box<Val>),
    /// What `\$val` makes: a reference, which is how a conversion says "this
    /// is binary data, not something to print". ExifTool renders it as
    /// `(Binary data N bytes, use -b option to extract)`; rendering it is the
    /// caller's job, so the bytes are kept here as they are.
    Binary(Vec<u8>),
}

impl Val {
    #[must_use]
    pub fn as_num(&self) -> f64 {
        match self {
            Self::Num(n) => *n,
            // Perl reads a leading number out of a string and calls the rest zero.
            Self::Undef => 0.0,
            #[allow(clippy::cast_precision_loss)]
            Self::Binary(b) => b.len() as f64,
            Self::Reference(v) => v.as_num(),
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
            Self::Binary(b) => from_bytes(b),
            Self::Reference(v) => v.as_string(),
            Self::Num(n) => format_number(*n),
            Self::Str(s) => s.clone(),
            // `"@a"` interpolates a list separated by `$"`, which is a space.
            Self::List(v) => v.iter().map(Self::as_string).collect::<Vec<_>>().join(" "),
        }
    }

    fn truthy(&self) -> bool {
        match self {
            Self::Undef => false,
            Self::Binary(b) => !b.is_empty(),
            Self::Reference(v) => v.truthy(),
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

    /// A reader option, as `$self->Options("DateFormat")` asks for. `None`
    /// declines; `Some(Val::Undef)` is an option that is simply not set, which
    /// is what most of them are and what these conversions branch on.
    fn option(&self, _name: &str) -> Option<Val> {
        None
    }

    /// The byte order the file is being read in, `"II"` or `"MM"`. ExifTool
    /// keeps it in a package global and `Decode` falls back to it when the
    /// conversion does not name one. `None` declines.
    fn byte_order(&self) -> Option<&str> {
        None
    }

    /// A tag the reader has already extracted, by the name ExifTool would use
    /// (`"HandlerType"`, `"MatrixStructure (1)"`).
    fn tag_value(&self, _name: &str) -> Option<Val> {
        None
    }

    /// The family-1 group of an extracted tag, which is how QuickTime tells
    /// one track's tags from another's.
    fn tag_group1(&self, _name: &str) -> Option<String> {
        None
    }

    /// A note the reader kept beside a tag while parsing -- GoPro records the
    /// units of each field that way.
    fn tag_extra(&self, _tag: &str, _key: &str) -> Option<Val> {
        None
    }

    /// The tag being converted, which a few conversions pass on as `$tag`.
    fn current_tag(&self) -> Option<String> {
        None
    }
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

/// A Composite tag's conversion, which is handed both the values it is built
/// from (`@val`, `$val[0]`) and their printed forms (`@prt`, `$prt[0]`).
pub fn eval_composite(
    expr: &str,
    vals: &[Val],
    prts: &[Val],
    raws: &[Val],
    state: &dyn ParseState,
) -> Option<Val> {
    let mut p = new_parser(expr, &Val::List(vals.to_vec()), state);
    p.prt = Val::List(prts.to_vec());
    p.raw = Val::List(raws.to_vec());
    run(p)
}

/// As [`eval`], with the file-level parse state the conversion may read.
pub fn eval_with(expr: &str, val: &Val, state: &dyn ParseState) -> Option<Val> {
    run(new_parser(expr, val, state))
}

fn new_parser<'a>(expr: &'a str, val: &Val, state: &'a dyn ParseState) -> Parser<'a> {
    Parser {
        s: expr.as_bytes(),
        i: 0,
        val: val.clone(),
        captures: Vec::new(),
        vars: std::collections::HashMap::new(),
        state,
        subject_after: None,
        quiet: 0,
        prt: Val::Undef,
        raw: Val::Undef,
    }
}

/// Run a prepared parser to the end of its source.
///
/// These conversions are sometimes two statements: `$val =~ s/ +$//; $val`
/// substitutes and then hands back the value it changed. The last one is the
/// result, as in Perl.
fn run(mut p: Parser) -> Option<Val> {
    p.skip_require();
    let mut last = p.statement()?;
    loop {
        p.skip_ws();
        // Perl's comma is also a statement separator in scalar context, and
        // `$_=$val,s/(\d+)(\d{4})/$1-$2/,$_` uses it as one.
        if p.i < p.s.len() && (p.s[p.i] == b';' || p.s[p.i] == b',') {
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
    /// One of the values a Composite tag is built from.
    ValElem(usize),
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

/// What Perl accepts as a quoting delimiter after `s`, `m`, `tr` or `y`.
fn is_delimiter(c: u8) -> bool {
    !c.is_ascii_alphanumeric() && !c.is_ascii_whitespace() && c != b'_' && c != b',' && c != b';'
}

/// The closing half of a bracketing delimiter, if it has one.
fn closing_delimiter(c: u8) -> Option<u8> {
    Some(match c {
        b'{' => b'}',
        b'(' => b')',
        b'[' => b']',
        b'<' => b'>',
        _ => return None,
    })
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
    /// The printed values of a Composite tag's parts, read as `@prt`, and
    /// their unconverted forms, read as `@raw`. Undef for every other
    /// conversion, which has neither.
    prt: Val,
    raw: Val,
    /// Non-zero while evaluating a branch Perl would never have run. The
    /// parser still has to walk it to find where it ends, but a division by
    /// zero in there is not a reason to refuse the conversion -- Perl guards
    /// exactly that way: `$val ? 1/$val : 0`.
    quiet: usize,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        loop {
            while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            // A `#` comment runs to the end of the line. One conversion in
            // Olympus.pm is written across six lines with a comment naming
            // each field between them.
            if self.s.get(self.i) != Some(&b'#') {
                return;
            }
            while self.i < self.s.len() && self.s[self.i] != b'\n' {
                self.i += 1;
            }
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
        // `$val += 4294967296 if $val < 0` runs the statement only when the
        // condition holds, and is worth nothing when it does not.
        if let Some((body_end, kw_end, negated)) = self.find_conditional() {
            let body_start = self.i;
            self.i = kw_end;
            let cond = self.expr()?;
            return if cond.truthy() != negated {
                self.run_slice(body_start, body_end)
            } else {
                Some(Val::Undef)
            };
        }
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

    /// The `}` that closes a block opened just before `from`.
    fn scan_to_close(&self, from: usize) -> Option<usize> {
        let mut depth = 1i32;
        let mut j = from;
        while j < self.s.len() {
            match self.s[j] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(j);
                    }
                }
                _ => {}
            }
            j += 1;
        }
        None
    }

    /// The comma that ends `grep`'s first argument: the first one outside any
    /// bracket.
    fn scan_to_comma(&self, from: usize) -> Option<usize> {
        let mut depth = 0i32;
        let mut j = from;
        while j < self.s.len() {
            match self.s[j] {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => {
                    if depth == 0 {
                        return None;
                    }
                    depth -= 1;
                }
                b',' if depth == 0 => return Some(j),
                _ => {}
            }
            j += 1;
        }
        None
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

    /// Look ahead for a trailing `if` or `unless`, and say which it was.
    fn find_conditional(&mut self) -> Option<(usize, usize, bool)> {
        let (at, kw) = self.find_word_at_depth(&["if", "unless"])?;
        Some((at, at + kw.len(), kw == "unless"))
    }

    /// Look ahead for a `foreach` (or `for`) modifier in this statement,
    /// outside any string or bracket. Returns where the body ends and where
    /// the list begins.
    fn find_foreach(&mut self) -> Option<(usize, usize)> {
        let (at, kw) = self.find_word_at_depth(&["foreach", "for"])?;
        Some((at, at + kw.len()))
    }

    /// The first of `words` appearing in this statement outside any string or
    /// bracket -- which is where a statement modifier sits, and nowhere else.
    fn find_word_at_depth(&mut self, words: &[&'static str]) -> Option<(usize, &'static str)> {
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
                _ if depth == 0 && j > self.i && self.s[j - 1].is_ascii_whitespace() => {
                    for kw in words {
                        if self.s[j..].starts_with(kw.as_bytes())
                            && self.s.get(j + kw.len()).is_some_and(u8::is_ascii_whitespace)
                        {
                            return Some((j, kw));
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
                return Some(LValue::ValElem(idx.as_num() as usize));
            }
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
            LValue::ValElem(k) => match &self.val {
                Val::List(items) => Some(items.get(*k).cloned().unwrap_or(Val::Undef)),
                _ => None,
            },
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
            LValue::ValElem(k) => {
                let Val::List(items) = &mut self.val else { return None };
                *items.get_mut(*k)? = v;
            }
            LValue::Var(n) => {
                self.vars.insert(n.clone(), v);
            }
            LValue::Elem(n, k) => {
                let Some(Val::List(items)) = self.vars.get_mut(n) else {
                    return None;
                };
                // Perl grows the array to fit, filling the gap with undef.
                if *k >= items.len() {
                    items.resize(*k + 1, Val::Undef);
                }
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
            let whole = self.vars.get(&format!("@{name}")).cloned();
            // `@a[0,2,3]` is a slice: the elements at those indices, in that
            // order.
            if self.peek("[") {
                self.eat("[");
                let Val::List(items) = whole? else { return None };
                let mut picked = Vec::new();
                loop {
                    let idx = self.expr()?;
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let k = idx.as_num() as usize;
                    picked.push(items.get(k).cloned().unwrap_or(Val::Undef));
                    if !self.eat(",") {
                        break;
                    }
                }
                if !self.eat("]") {
                    return None;
                }
                return Some(Val::List(picked));
            }
            return whole;
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
        // `$val > 1800 and $val -= 3600` assigns on the right of an `and`,
        // which Perl allows because assignment binds tighter than the word
        // operators.
        if let Some(v) = self.try_assignment() {
            return Some(v);
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
        // `\$val` takes a reference, which is ExifTool's way of saying the
        // value is binary and should not be printed as text.
        if self.peek("\\") {
            self.eat("\\");
            let v = self.unary()?;
            // A reference to a list stays whole: what matters is that it does
            // not flatten into an argument list.
            if matches!(v, Val::List(_)) {
                return Some(Val::Reference(Box::new(v)));
            }
            return Some(Val::Binary(perl_bytes(&v)?));
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
        // A Composite tag is handed the list of the values it is built from,
        // and reads them as `@val` and `$val[0]`.
        if self.peek("$val[") {
            self.eat("$val[");
            let idx = self.expr()?;
            if !self.eat("]") {
                return None;
            }
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let k = idx.as_num() as usize;
            let Val::List(items) = &self.val else { return None };
            return Some(items.get(k).cloned().unwrap_or(Val::Undef));
        }
        if self.peek("@val") {
            self.eat("@val");
            return Some(self.val.clone());
        }
        // `$$val` reads through a reference -- what `\\$val` made, or what a
        // helper like DecodeBase64 returned.
        if self.peek("$$val") {
            self.eat("$$val");
            let v = self.vars.get("val").unwrap_or(&self.val).clone();
            return Some(match v {
                Val::Binary(b) => Val::Str(from_bytes(&b)),
                other => other,
            });
        }
        if self.peek("$raw[") || self.peek("@raw") {
            let indexed = self.peek("$raw[");
            self.eat(if indexed { "$raw[" } else { "@raw" });
            if self.raw == Val::Undef {
                return None;
            }
            if !indexed {
                return Some(self.raw.clone());
            }
            let idx = self.expr()?;
            if !self.eat("]") {
                return None;
            }
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let k = idx.as_num() as usize;
            let Val::List(items) = &self.raw else { return None };
            return Some(items.get(k).cloned().unwrap_or(Val::Undef));
        }
        if self.peek("$prt[") {
            self.eat("$prt[");
            let idx = self.expr()?;
            if !self.eat("]") {
                return None;
            }
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let k = idx.as_num() as usize;
            let Val::List(items) = &self.prt else { return None };
            return Some(items.get(k).cloned().unwrap_or(Val::Undef));
        }
        if self.peek("@prt") {
            self.eat("@prt");
            if self.prt == Val::Undef {
                return None;
            }
            return Some(self.prt.clone());
        }
        if self.eat("$val") {
            // `my $val = ...` shadows the value being converted, and two of
            // these conversions rebind it to a decoded copy.
            return Some(self.vars.get("val").unwrap_or(&self.val).clone());
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
        // `$self->Options("Unknown")` asks the reader how it was configured.
        if self.peek("$self->Options(") {
            self.eat("$self->Options(");
            let name = self.expr()?;
            if !self.eat(")") {
                return None;
            }
            return self.state.option(&name.as_string());
        }
        // `$tag` is the name of the tag being converted.
        if self.peek("$tag") && !self.s.get(self.i + 4).is_some_and(|c| ident_char(*c)) {
            self.eat("$tag");
            return Some(Val::Str(self.state.current_tag()?));
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
        // A substitution with nothing bound to it works on `$_`.
        if self.starts_regex_op() {
            let target = LValue::Var("_".to_string());
            return self.bind_operation(&target, false);
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
            // `${val}` is `$val` with the name spelled out, which a string
            // needs when a letter follows it.
            if self.s[self.i..].starts_with(b"${") {
                let Some(end) = self.s[self.i..].iter().position(|c| *c == b'}') else {
                    return None;
                };
                let name = std::str::from_utf8(&self.s[self.i + 2..self.i + end]).ok()?;
                let v = if name == "val" {
                    self.val.clone()
                } else {
                    self.vars.get(name)?.clone()
                };
                out.push_str(&v.as_string());
                self.i += end + 1;
                continue;
            }
            if self.s[self.i..].starts_with(b"$prt[")
                || self.s[self.i..].starts_with(b"@prt")
                || self.s[self.i..].starts_with(b"$raw[")
                || self.s[self.i..].starts_with(b"@raw")
                || self.s[self.i..].starts_with(b"$val[")
                || self.s[self.i..].starts_with(b"@val")
            {
                let v = self.primary()?;
                out.push_str(&v.as_string());
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

    /// Whether what follows is `s`, `tr`, `y` or `m` used as an operator --
    /// which it is only when a delimiter comes next, or `sprintf` would read
    /// as an `s`.
    fn starts_regex_op(&mut self) -> bool {
        self.skip_ws();
        // A `/` where a value is expected can only open a match -- division
        // needs something on its left, and that is a different position.
        if self.s.get(self.i) == Some(&b'/') {
            return true;
        }
        for op in ["tr", "s", "y", "m"] {
            if self.s[self.i..].starts_with(op.as_bytes()) {
                return self
                    .s
                    .get(self.i + op.len())
                    .is_some_and(|c| is_delimiter(*c));
            }
        }
        false
    }

    fn bind_to_value(&mut self, subject: &Val, negated: bool) -> Option<Val> {
        self.subject_after = None;
        self.skip_ws();
        // Any punctuation can quote these: `s{^/}{}` is as good as `s/^\///`,
        // and ExifTool writes both.
        let op = ["tr", "s", "y", "m"]
            .into_iter()
            .find(|o| {
                self.s[self.i..].starts_with(o.as_bytes())
                    && self.s.get(self.i + o.len()).is_some_and(|c| is_delimiter(*c))
            })
            .unwrap_or("");
        self.i += op.len();
        if op.is_empty() && !self.peek("/") {
            return None;
        }
        let (first, open) = self.quoted_part()?;
        let second = if matches!(op, "s" | "tr" | "y") {
            Some(if closing_delimiter(open).is_some() {
                self.quoted_part()?.0
            } else {
                // The closing delimiter doubles as the second one's opening.
                self.delimited(open as char)?
            })
        } else {
            None
        };
        let flags = self.regex_flags();

        if op == "s" {
            let re = build_regex(&first, &flags)?;
            let rep = perl_replacement(&second?);
            let subject = subject.as_string();
            let replaced = if flags.contains('g') {
                re.replace_all(&subject, rep.as_str()).into_owned()
            } else {
                re.replace(&subject, rep.as_str()).into_owned()
            };
            let changed = replaced != subject;
            self.subject_after = Some(Val::Str(replaced));
            return Some(Val::Num(if changed { 1.0 } else { 0.0 }));
        }
        if op == "tr" || op == "y" {
            let f: Vec<char> = first.chars().collect();
            let t: Vec<char> = second?.chars().collect();
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
            #[allow(clippy::cast_precision_loss)]
            return Some(Val::Num(n as f64));
        }
        let re = build_regex(&first, &flags)?;
        let subject = subject.as_string();
        // A `/g` match hands back everything it found -- the captures of each
        // match, or the whole of each when the pattern captures nothing. An
        // empty list is false, which is the same answer a plain match gives.
        if flags.contains('g') && !negated {
            let mut found = Vec::new();
            for c in re.captures_iter(&subject) {
                if c.len() > 1 {
                    for i in 1..c.len() {
                        found.push(Val::Str(c.get(i).map_or_else(String::new, |m| m.as_str().to_string())));
                    }
                } else {
                    found.push(Val::Str(c.get(0).map_or_else(String::new, |m| m.as_str().to_string())));
                }
            }
            self.captures.clear();
            return Some(Val::List(found));
        }
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

    /// Read one delimited part, starting at its opening delimiter. Bracketing
    /// delimiters nest; the rest end at their next unescaped appearance.
    fn quoted_part(&mut self) -> Option<(String, u8)> {
        self.skip_ws();
        let open = *self.s.get(self.i)?;
        if !is_delimiter(open) {
            return None;
        }
        self.i += 1;
        let bracketing = closing_delimiter(open);
        let close = bracketing.unwrap_or(open);
        let mut out = String::new();
        let mut depth = 1i32;
        while self.i < self.s.len() {
            let c = self.s[self.i];
            if c == b'\\' && self.i + 1 < self.s.len() {
                out.push('\\');
                out.push(self.s[self.i + 1] as char);
                self.i += 2;
                continue;
            }
            self.i += 1;
            if bracketing.is_some() && c == open {
                depth += 1;
            } else if c == close {
                depth -= 1;
                if depth == 0 {
                    return Some((out, open));
                }
            }
            out.push(c as char);
        }
        None
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
        let mut method = false;
        if self.peek("$self->") {
            self.eat("$self->");
            method = true;
        } else if self.peek("Image::ExifTool::") {
            self.eat("Image::ExifTool::");
            // Skip the module qualifier, e.g. `Exif::` -- but a good number of
            // these live in ExifTool.pm itself and have none.
            let unqualified = self.i;
            while self.i < self.s.len() {
                if self.s[self.i..].starts_with(b"::") {
                    self.i += 2;
                    break;
                }
                if !ident_char(self.s[self.i]) {
                    self.i = unqualified;
                    break;
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
        // Perl passes the elements of a list, not the list: `PrintFocalRange(@val)`
        // is three arguments. `\@val` is one, and stays whole.
        let args = flatten(&args);
        match call_helper(&name, &args, self.state, method) {
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
        const LIST: &[&str] =
            &["split", "join", "unpack", "pack", "reverse", "substr", "sprintf", "map", "grep"];
        const UNARY: &[&str] = &[
            "length", "hex", "oct", "ord", "chr", "lc", "uc", "lcfirst", "ucfirst", "defined",
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

        // `map` and `grep` run their first argument once per element, with
        // `$_` set to it, so that argument is a piece of source to re-run
        // rather than a value to compute now.
        if name == "map" || name == "grep" {
            self.skip_ws();
            let (from, to) = if self.peek("{") {
                self.eat("{");
                let from = self.i;
                let to = self.scan_to_close(from)?;
                self.i = to + 1;
                self.eat(","); // optional after a block
                (from, to)
            } else {
                let from = self.i;
                let to = self.scan_to_comma(from)?;
                self.i = to + 1;
                (from, to)
            };
            let mut list = vec![self.expr()?];
            while self.eat(",") {
                list.push(self.expr()?);
            }
            if paren && !self.eat(")") {
                self.i = save;
                return None;
            }
            let mut out = Vec::new();
            for item in flatten(&list) {
                self.vars.insert("_".to_string(), item.clone());
                let r = self.run_slice(from, to)?;
                if name == "map" {
                    out.push(r);
                } else if r.truthy() {
                    out.push(item);
                }
            }
            return Some(Val::List(out));
        }

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
            // `split " "` with no second argument splits `$_`.
            if !self.eat(",") {
                let subject = self.vars.get("_").cloned().unwrap_or(Val::Undef);
                let (pat, awk) = split_pat.as_ref()?;
                return Some(Val::List(
                    perl_split(pat, *awk, &subject.as_string(), None)?
                        .into_iter()
                        .map(Val::Str)
                        .collect(),
                ));
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
            } else if paren {
                // With brackets there is no ambiguity about how far the
                // argument reaches: `chr($val & 0xff)` means all of it.
                args.push(self.expr()?);
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
        let mut left = false;
        let mut width = String::new();
        let mut prec: Option<usize> = None;
        while let Some(&c) = it.peek() {
            match c {
                '+' | ' ' => {
                    plus = c == '+';
                    it.next();
                }
                '-' => {
                    left = true;
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
                    // `%.*f` takes its precision from the argument list.
                    if it.peek() == Some(&'*') {
                        it.next();
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        let p = arg.next().map_or(0.0, Val::as_num) as usize;
                        prec = Some(p);
                        continue;
                    }
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
        // Perl prints an argument that is not there as undef: 0 for a number,
        // empty for a string. It does not refuse the format.
        let missing = Val::Undef;
        let v = arg.next().unwrap_or(&missing);
        let mut s = match conv {
            'd' | 'i' => {
                // Perl truncates towards zero here; it does not round. A
                // precision on an integer is a minimum number of digits.
                let n = v.as_num().trunc() as i64;
                let digits = format!("{:0>width$}", n.unsigned_abs(), width = prec.unwrap_or(1));
                let sign = if n < 0 {
                    "-"
                } else if plus {
                    "+"
                } else {
                    ""
                };
                format!("{sign}{digits}")
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
                let sign = if plus && n >= 0.0 { "+" } else { "" };
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
                format!("{sign}{t}")
            }
            'u' => format!("{:0>width$}", to_u64(v)?, width = prec.unwrap_or(1)),
            'x' => format!("{:0>width$x}", to_u64(v)?, width = prec.unwrap_or(1)),
            'X' => format!("{:0>width$X}", to_u64(v)?, width = prec.unwrap_or(1)),
            'o' => format!("{:0>width$o}", to_u64(v)?, width = prec.unwrap_or(1)),
            'b' => format!("{:0>width$b}", to_u64(v)?, width = prec.unwrap_or(1)),
            // A precision on a string is a maximum length.
            's' => {
                let t = v.as_string();
                match prec {
                    Some(p) if t.chars().count() > p => t.chars().take(p).collect(),
                    _ => t,
                }
            }
            _ => return None,
        };
        if let Ok(w) = width.parse::<usize>() {
            // Zero padding goes after the sign, not before it: Perl prints
            // -5 in `%05d` as `-0005`.
            let sign_len = usize::from(s.starts_with(['-', '+']) && zero && !left);
            while s.chars().count() < w {
                if left {
                    s.push(' ');
                } else if zero {
                    s.insert(sign_len, '0');
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

/// What a reference points at.
fn deref(v: &Val) -> &Val {
    match v {
        Val::Reference(inner) => inner,
        other => other,
    }
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
        "defined" => Val::Num(if *args.first()? == Val::Undef { 0.0 } else { 1.0 }),
        "hex" => {
            // Perl reads the leading hex digits and calls the rest zero; it
            // does not fail, and `hex($1)` after a match that did not happen
            // is 0 rather than a refusal.
            let t = args.first()?.as_string();
            let t = t.trim();
            let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
            let digits: String = t.chars().take_while(char::is_ascii_hexdigit).collect();
            #[allow(clippy::cast_precision_loss)]
            Val::Num(u64::from_str_radix(&digits, 16).unwrap_or(0) as f64)
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
                let digits: String =
                    t.trim_start_matches('0').chars().take_while(|c| ('0'..='7').contains(c)).collect();
                u64::from_str_radix(&digits, 8).unwrap_or(0)
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
    // `(H2)6` is that group six times over, which is the only grouping these
    // templates use.
    let tmpl = if tmpl.starts_with('(') {
        let end = tmpl.find(')')?;
        let inner = &tmpl[1..end];
        let rest = &tmpl[end + 1..];
        let count: usize = rest.trim().parse().ok()?;
        &inner.repeat(count)
    } else {
        tmpl
    };
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

fn call_helper(name: &str, args: &[Val], state: &dyn ParseState, method: bool) -> Option<Val> {
    // These take the ExifTool object as their first argument. Written as
    // `$self->Decode(...)` it is implied; written out in full it is there in
    // the list, and everything shifts by one.
    const TAKES_SELF: &[&str] = &[
        "Printable", "Decode", "ConvertID3v1Text", "ConvertExifText", "CalcScaleFactor35efl",
        "AddUnits", "ConvertPascalString", "PrintLensID", "CalcRotation", "ToDMS",
    ];
    let args: &[Val] = if TAKES_SELF.contains(&name) && !method { args.get(1..)? } else { args };
    // Some of these take nothing but the object: `CalcRotation($self)` reads
    // the file's own tags and has no value in hand at all.
    const NOTHING: Val = Val::Undef;
    let first = args.first().unwrap_or(&NOTHING);
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
            first.as_num(),
            args.get(2).map(Val::as_string).as_deref(),
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
        // ASF.pm GetGUID: sixteen bytes, byte-swapped into the printed order.
        "GetGUID" => {
            let bytes = perl_bytes(first)?;
            if bytes.len() != 16 {
                return Some(first.clone());
            }
            let parts = unpack_template("VvvNN", &bytes)?;
            let hex: String = pack_template("NnnNN", &parts)?
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect();
            Val::Str(format!(
                "{}-{}-{}-{}-{}",
                &hex[0..8],
                &hex[8..12],
                &hex[12..16],
                &hex[16..20],
                &hex[20..]
            ))
        }
        // Canon.pm CanonEv: the low five bits are a fraction of a stop, with
        // two of the codes standing for a third and two thirds.
        "CanonEv" => {
            let v = first.as_num();
            let sign = if v < 0.0 { -1.0 } else { 1.0 };
            #[allow(clippy::cast_possible_truncation)]
            let n = v.abs().trunc() as i64;
            let mut frac = f64::from(u32::try_from(n & 0x1f).ok()?);
            #[allow(clippy::cast_precision_loss)]
            let whole = (n - (n & 0x1f)) as f64;
            if frac == 12.0 {
                frac = 32.0 / 3.0;
            } else if frac == 20.0 {
                frac = 64.0 / 3.0;
            }
            Val::Num(sign * (whole + frac) / 32.0)
        }
        // CanonVRD.pm ToneCurvePrint: a count, then that many (x,y) pairs.
        "ToneCurvePrint" => {
            let text = first.as_string();
            let vals: Vec<&str> = text.split_whitespace().collect();
            if vals.len() != 21 {
                return Some(first.clone());
            }
            let Ok(n) = vals[0].parse::<usize>() else {
                return Some(first.clone());
            };
            if !(2..=10).contains(&n) {
                return Some(first.clone());
            }
            Val::Str(
                (0..n)
                    .map(|k| format!("({},{})", vals[1 + k * 2], vals[2 + k * 2]))
                    .collect::<Vec<_>>()
                    .join(" "),
            )
        }
        // LNK.pm DOSTime: a packed DOS date and time, in one 32-bit value.
        "DOSTime" => {
            let v = to_u64(first)?;
            Val::Str(format!(
                "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
                ((v >> 9) & 0x7f) + 1980,
                (v >> 5) & 0x0f,
                v & 0x1f,
                (v >> 27) & 0x1f,
                (v >> 21) & 0x3f,
                (v >> 15) & 0x3e
            ))
        }
        // GPS.pm ConvertTimeStamp: three numbers into a clock time, with the
        // fraction of a second kept only as far as it goes.
        "ConvertTimeStamp" => Val::Str(convert_gps_time_stamp(&first.as_string())?),
        // RIFF.pm ConvertRIFFDate: three layouts, none of them EXIF's.
        "ConvertRIFFDate" => Val::Str(convert_riff_date(&first.as_string())),
        // ExifTool.pm ConvertTimeSpan: ticks, at so many seconds each.
        "ConvertTimeSpan" => convert_time_span(first, args.get(1).map(Val::as_num))?,
        // ExifTool.pm DecodeBits, with no lookup table: the numbers of the
        // bits that are set. A lookup is a hash we have no way to name here,
        // so that form is refused rather than answered with the raw numbers.
        "DecodeBits" => {
            if args.get(1).is_some_and(|l| *l != Val::Undef) {
                return None;
            }
            let bits = args.get(2).map_or(32, |b| b.as_num() as i64);
            let mut set = Vec::new();
            for (chunk, word) in first.as_string().split_whitespace().enumerate() {
                let v: u64 = word.parse().ok()?;
                #[allow(clippy::cast_sign_loss)]
                for i in 0..bits.clamp(0, 64) {
                    if v & (1u64 << i) != 0 {
                        set.push((i + chunk as i64 * bits).to_string());
                    }
                }
            }
            Val::Str(if set.is_empty() { "(none)".to_string() } else { set.join(",") })
        }
        // ExifTool.pm Decode: from the named character set into the internal
        // one, which is UTF-8 unless the reader was told otherwise.
        "Decode" => decode_charset(first, &args.get(1)?.as_string(), args.get(2), state)?,
        // ID3.pm ConvertID3v1Text: Decode, with the charset the reader keeps
        // for ID3v1 -- `Latin` unless it was overridden.
        "ConvertID3v1Text" => {
            let charset = match state.option("CharsetID3")? {
                Val::Undef => "Latin".to_string(),
                other => other.as_string(),
            };
            decode_charset(first, &charset, None, state)?
        }
        // XMP.pm DecodeBase64, which returns a reference: the result is
        // binary, not text.
        "DecodeBase64" => Val::Binary(decode_base64(&first.as_string())?),
        // Pentax.pm PentaxEv: eighths of a stop, with two codes standing for
        // a third and two thirds.
        "PentaxEv" => {
            #[allow(clippy::cast_possible_truncation)]
            let v = first.as_num().trunc() as i64;
            #[allow(clippy::cast_precision_loss)]
            let mut out = v as f64;
            if v & 0x01 != 0 {
                let sign = if v < 0 { -1.0 } else { 1.0 };
                let frac = (v * if v < 0 { -1 } else { 1 }) & 0x07;
                if frac == 3 {
                    out += sign * (8.0 / 3.0 - 3.0);
                } else if frac == 5 {
                    out += sign * (16.0 / 3.0 - 5.0);
                }
            }
            Val::Num(out / 8.0)
        }
        // ExifTool.pm TimeZoneString: a count of minutes, as an offset.
        "TimeZoneString" => {
            let mut min = first.as_num();
            let sign = if min < 0.0 {
                min = -min;
                '-'
            } else {
                '+'
            };
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let min = (min + 0.5).trunc() as u64;
            Val::Str(format!("{sign}{:02}:{:02}", min / 60, min % 60))
        }
        // ExifTool.pm IsInt and IsFloat, which several conversions ask before
        // deciding whether the value is a number at all.
        "IsInt" => Val::Num(if is_perl_int(&first.as_string()) { 1.0 } else { 0.0 }),
        "IsFloat" => Val::Num(if first.as_string().trim().parse::<f64>().is_ok() {
            1.0
        } else {
            0.0
        }),
        // ICC_Profile.pm HexID: the profile ID bytes, or a plain zero when
        // none of them was ever computed.
        "HexID" => {
            let text = first.as_string();
            let vals: Vec<&str> = text.split_whitespace().collect();
            if !vals.iter().any(|v| !v.starts_with('0')) {
                Val::Num(0.0)
            } else {
                let mut out = String::new();
                for v in vals {
                    out.push_str(&format_sprintf("%.2x", &[Val::Str(v.to_string())])?);
                }
                Val::Str(out)
            }
        }
        // MinoltaRaw.pm ConvertWBMode: a mode in the low nibble, and a shift
        // above it.
        "ConvertWBMode" => {
            const MODES: [&str; 11] = [
                "Auto", "Daylight", "Cloudy", "Tungsten", "Flash/Fluorescent",
                "Fluorescent", "Shade", "User 1", "User 2", "User 3", "Temperature",
            ];
            let v = to_u64(first)?;
            let lo = (v & 0x0f) as usize;
            let mut out = MODES.get(lo).map_or_else(|| format!("Unknown ({lo})"), |m| (*m).to_string());
            let hi = v >> 4;
            if (6..=12).contains(&hi) {
                #[allow(clippy::cast_possible_wrap)]
                out.push_str(&format!(" ({})", hi as i64 - 8));
            }
            Val::Str(out)
        }
        // Canon.pm CameraISO: a speed, or one of the codes that is not one.
        "CameraISO" => {
            let v = to_u64(first)?;
            if v == 0x7fff {
                return Some(Val::Undef);
            }
            if v & 0x4000 != 0 {
                #[allow(clippy::cast_precision_loss)]
                Val::Num((v & 0x3fff) as f64)
            } else {
                match v {
                    0 => Val::Str("n/a".into()),
                    14 => Val::Str("Auto High".into()),
                    15 => Val::Str("Auto".into()),
                    16 => Val::Num(50.0),
                    17 => Val::Num(100.0),
                    18 => Val::Num(200.0),
                    19 => Val::Num(400.0),
                    20 => Val::Num(800.0),
                    _ => Val::Str(format!("Unknown ({v})")),
                }
            }
        }
        // Canon.pm PrintFocalRange: one focal length, or the two ends of a
        // zoom.
        "PrintFocalRange" => {
            let short = first.as_num();
            let long = args.get(1)?.as_num();
            let scale = match args.get(2).map(Val::as_num) {
                Some(s) if s != 0.0 => s,
                _ => 1.0,
            };
            Val::Str(if (short - long).abs() < f64::EPSILON {
                format_sprintf("%.1f mm", &[Val::Num(short * scale)])?
            } else {
                format_sprintf("%.1f - %.1f mm", &[Val::Num(short * scale), Val::Num(long * scale)])?
            })
        }
        // Exif.pm PrintCFAPattern: a width, a height, then that many colours.
        "PrintCFAPattern" => {
            const COLOURS: [&str; 7] =
                ["Red", "Green", "Blue", "Cyan", "Magenta", "Yellow", "White"];
            let text = first.as_string();
            let a: Vec<f64> = text.split_whitespace().map(leading_number).collect();
            if a.len() < 2 {
                return Some(Val::Str("<truncated data>".into()));
            }
            if a[0] == 0.0 || a[1] == 0.0 {
                return Some(Val::Str("<zero pattern size>".into()));
            }
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let (cols, end) = (a[1] as usize, 2 + (a[0] * a[1]) as usize);
            if end > a.len() {
                return Some(Val::Str("<invalid pattern size>".into()));
            }
            let mut out = "[".to_string();
            let mut pos = 2usize;
            loop {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let k = a[pos] as usize;
                out.push_str(COLOURS.get(k).copied().unwrap_or("Unknown"));
                pos += 1;
                if pos >= end {
                    break;
                }
                if (pos - 2) % cols == 0 {
                    out.push_str("][");
                } else {
                    out.push(',');
                }
            }
            out.push(']');
            Val::Str(out)
        }
        // ExifTool.pm ConvertFileSize, in the units the reader was asked for.
        "ConvertFileSize" => {
            let v = first.as_num();
            let binary = state.option("ByteUnit").is_some_and(|u| u.as_string() == "Binary");
            let (k, m, g, ks, ms, gs) = if binary {
                (1024.0, 1_048_576.0, 1_073_741_824.0, "KiB", "MiB", "GiB")
            } else {
                (1000.0, 1_000_000.0, 1_000_000_000.0, "kB", "MB", "GB")
            };
            let steps: [(f64, f64, &str, &str); 5] = [
                (10.0 * k, k, ks, "%.1f"),
                (2.0 * m, k, ks, "%.0f"),
                (10.0 * m, m, ms, "%.1f"),
                (2.0 * g, m, ms, "%.0f"),
                (10.0 * g, g, gs, "%.1f"),
            ];
            if v < if binary { 2048.0 } else { 2000.0 } {
                return Some(Val::Str(format!("{} bytes", first.as_string())));
            }
            let mut out = None;
            for (limit, div, unit, fmt) in steps {
                if v < limit {
                    out = Some(format!("{} {unit}", format_sprintf(fmt, &[Val::Num(v / div)])?));
                    break;
                }
            }
            Val::Str(match out {
                Some(t) => t,
                None => format!("{} {gs}", format_sprintf("%.0f", &[Val::Num(v / g)])?),
            })
        }
        // ExifTool.pm PrintHex: every byte, in hex, separated by spaces.
        "PrintHex" => Val::Str(
            perl_bytes(first)?
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<Vec<_>>()
                .join(" "),
        ),
        // ExifTool.pm Printable: control characters become dots, NULs go, and
        // a long value is cut with a marker. `$self` comes first here.
        "Printable" => {
            let value = first;
            if let Val::Binary(b) = value {
                return Some(Val::Str(format!("(Binary data {} bytes)", b.len())));
            }
            if *value == Val::Undef {
                return Some(Val::Str("(undef)".into()));
            }
            let text: String = value
                .as_string()
                .chars()
                .filter(|c| *c != '\0')
                .map(|c| {
                    let n = c as u32;
                    if (0x01..=0x1f).contains(&n) || (0x7f..=0xff).contains(&n) { '.' } else { c }
                })
                .collect();
            let verbose = state.option("Verbose")?.as_num();
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let max = if verbose < 4.0 {
                match args.get(1) {
                    Some(m) if m.as_num() > 0.0 => (m.as_num() as usize).max(20),
                    Some(_) => text.chars().count(),
                    None => 60,
                }
            } else if verbose < 5.0 {
                text.chars().count().min(2048)
            } else {
                text.chars().count()
            };
            Val::Str(if text.chars().count() > max {
                format!("{}[snip]", text.chars().take(max.saturating_sub(6)).collect::<String>())
            } else {
                text
            })
        }
        // Exif.pm CalculateLV: the light value three measurements imply.
        "CalculateLV" => {
            let mut nums = Vec::new();
            for a in args.iter().take(3) {
                let f = perl_float(&a.as_string())?;
                if f <= 0.0 {
                    return Some(Val::Undef);
                }
                nums.push(f);
            }
            if nums.len() < 3 {
                return Some(Val::Undef);
            }
            Val::Num((nums[0] * nums[0] * 100.0 / (nums[1] * nums[2])).ln() / std::f64::consts::LN_2)
        }
        // Exif.pm RedBlueBalance: the level of one channel over green, with
        // the component order given by the table it was found in.
        "RedBlueBalance" => red_blue_balance(args)?,
        // ExifTool.pm Get8u: one byte, through a reference.
        "Get8u" => {
            let bytes = perl_bytes(first)?;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let off = args.get(1)?.as_num() as usize;
            Val::Num(f64::from(*bytes.get(off)?))
        }
        // Photoshop.pm ConvertPascalString: a run of length-prefixed strings.
        "ConvertPascalString" => {
            let bytes = perl_bytes(first)?;
            let mut parts: Vec<String> = Vec::new();
            let mut i = 0usize;
            while i < bytes.len() {
                let n = bytes[i] as usize;
                if i + n >= bytes.len() {
                    break;
                }
                parts.push(from_bytes(&bytes[i + 1..=i + n]));
                i += n + 1;
            }
            let charset = match state.option("CharsetPhotoshop") {
                Some(Val::Undef) | None => "Latin".to_string(),
                Some(other) => {
                    let t = other.as_string();
                    if t.is_empty() { "Latin".to_string() } else { t }
                }
            };
            decode_charset(&Val::Str(parts.join(", ")), &charset, None, state)?
        }
        // PostScript.pm ImageSize: a bounding box, as a width or a height.
        "ImageSize" => {
            let Val::List(vals) = deref(first) else { return None };
            let want_height = args.get(1)?.truthy();
            let two = build_regex(r"^(\d+) (\d+)", "")?;
            let four = build_regex(r"^(\d+) (\d+) (\d+) (\d+)", "")?;
            let (mut w, mut h) = (None, None);
            let first_text = vals.first().filter(|v| v.truthy()).map(Val::as_string);
            let second_text = vals.get(1).filter(|v| v.truthy()).map(Val::as_string);
            if let Some(c) = first_text.as_deref().and_then(|t| two.captures(t)) {
                w = c.get(1).map(|m| leading_number(m.as_str()));
                h = c.get(2).map(|m| leading_number(m.as_str()));
            } else if let Some(c) = second_text.as_deref().and_then(|t| four.captures(t)) {
                let g = |i: usize| c.get(i).map_or(0.0, |m| leading_number(m.as_str()));
                w = Some(g(3) - g(1));
                h = Some(g(4) - g(2));
            }
            match if want_height { h } else { w } {
                Some(v) => Val::Num(v),
                None => Val::Undef,
            }
        }
        // Olympus.pm ExtenderStatus: whether the teleconverter was really on.
        "ExtenderStatus" => {
            let text = first.as_string();
            let info: Vec<&str> = text.split_whitespace().collect();
            if info.len() < 2 || i64::from_str_radix(info[1], 16).unwrap_or(0) == 0 {
                return Some(Val::Num(0.0));
            }
            if format!("{} {}", info[0], info[1]) != "0 04" {
                return Some(Val::Num(1.0));
            }
            let lens_type = args.get(1)?.as_string();
            let re = build_regex(r" F(\d+(\.\d+)?)", "")?;
            let Some(c) = re.captures(&lens_type) else { return Some(Val::Num(1.0)) };
            let max_of_lens = c.get(1).map_or(0.0, |m| leading_number(m.as_str()));
            Val::Num(if args.get(2)?.as_num() - max_of_lens > 0.2 { 1.0 } else { 2.0 })
        }
        // Kodak.pm CalculateRGBLevels: white balance multipliers from a
        // temperature and a polynomial.
        "CalculateRGBLevels" => {
            let a = if let Val::List(items) = deref(first) { items.clone() } else { args.to_vec() };
            if a.get(10).is_some_and(Val::truthy) {
                return Some(Val::Undef); // the software levels win
            }
            #[allow(clippy::cast_possible_truncation)]
            let wbi = a.first()?.as_num().trunc() as i64;
            if !(0..=3).contains(&wbi) {
                return Some(Val::Undef);
            }
            #[allow(clippy::cast_sign_loss)]
            let wbi = wbi as usize;
            let mul: Vec<f64> =
                a.get(wbi + 1)?.as_string().split_whitespace().take(13).map(leading_number).collect();
            let coefs: Vec<f64> =
                a.get(wbi + 5)?.as_string().split_whitespace().map(leading_number).collect();
            let temp = match a.get(9).map(Val::as_num) {
                Some(t) if t != 0.0 => t,
                _ => 6500.0,
            } / 100.0;
            if mul.len() < 3 || coefs.len() < 12 {
                return Some(Val::Undef);
            }
            let mut out = Vec::new();
            let mut n = 0usize;
            for c in 0..3 {
                let mut num = 0.0;
                for i in 0..4 {
                    num += coefs[n] * temp.powi(i);
                    n += 1;
                }
                out.push(format_number(2048.0 / (num * mul[c])));
            }
            Val::Str(out.join(" "))
        }
        // Olympus.pm PrintAFAreas: each area as its two corners.
        "PrintAFAreas" => {
            let text = first.as_string();
            let mut parts = Vec::new();
            for pt in text.split_whitespace() {
                let v = leading_number(pt);
                if v == 0.0 {
                    continue;
                }
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let n = v as u32;
                let name = match n {
                    0x3679_4285 => "Left ",
                    0x7979_8585 => "Center ",
                    0xBD79_C985 => "Right ",
                    _ => "",
                };
                let b = n.to_be_bytes();
                parts.push(format!("{name}({},{})-({},{})", b[0], b[1], b[2], b[3]));
            }
            Val::Str(if parts.is_empty() { "none".to_string() } else { parts.join(", ") })
        }
        // Pentax.pm DecodeAFPoints: a bit field, so many bits per point.
        "DecodeAFPoints" => {
            let text = first.as_string();
            let bytes: Vec<i64> = text
                .split_whitespace()
                .map(|b| {
                    #[allow(clippy::cast_possible_truncation)]
                    let v = leading_number(b).trunc() as i64;
                    v
                })
                .collect();
            if bytes.is_empty() {
                return Some(Val::Str("(none)".into()));
            }
            #[allow(clippy::cast_possible_truncation)]
            let num = args.get(1)?.as_num().trunc() as i64;
            #[allow(clippy::cast_possible_truncation)]
            let bits = args.get(2)?.as_num().trunc() as i64;
            #[allow(clippy::cast_possible_truncation)]
            let mask = args.get(3)?.as_num().trunc() as i64;
            let bit_val = args.get(4).map(Val::as_num);
            let mut next = 1usize;
            let mut byte = bytes[0];
            let mut shift = 8 - bits;
            let mut set = Vec::new();
            let mut i = 1i64;
            loop {
                let field = (byte >> shift) & mask;
                #[allow(clippy::cast_precision_loss)]
                let hit = match bit_val {
                    Some(want) => field as f64 == want,
                    None => field != 0,
                };
                if hit {
                    set.push(i.to_string());
                }
                i += 1;
                if i > num {
                    break;
                }
                shift -= bits;
                if shift < 0 {
                    let Some(b) = bytes.get(next) else { break };
                    byte = *b;
                    next += 1;
                    shift += 8;
                }
            }
            Val::Str(set.join(","))
        }
        // Canon.pm PrintAFPoints1D: the focus point, then the points in use.
        "PrintAFPoints1D" => print_af_points_1d(&perl_bytes(first)?)?,
        // Exif.pm PrintSFR: named columns, then a table of rationals.
        "PrintSFR" => print_sfr(&perl_bytes(first)?)?,
        // IPTC's picture number: a manufacturer, equipment, date and serial.
        "ConvertPictureNumber" => {
            let bytes = perl_bytes(first)?;
            if bytes.iter().all(|b| *b == 0) && bytes.len() == 16 {
                return Some(Val::Str("Unknown".into()));
            }
            if bytes.len() < 16 {
                return Some(Val::Str("<format error>".into()));
            }
            const MAKERS: [&str; 12] = [
                "Associated Press, USA",
                "Eastman Kodak Co, USA",
                "Hasselblad Electronic Imaging, Sweden",
                "Tecnavia SA, Switzerland",
                "Nikon Corporation, Japan",
                "Coatsworth Communications Inc, Canada",
                "Agence France Presse, France",
                "T/One Inc, USA",
                "Associated Newspapers, UK",
                "Reuters London",
                "Sandia Imaging Systems Inc, USA",
                "Visualize, Spain",
            ];
            let v = unpack_template("nNA8n", &bytes)?;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let maker = v.first()?.as_num() as usize;
            let mut out = v.first()?.as_string();
            if let Some(name) = maker.checked_sub(1).and_then(|k| MAKERS.get(k)) {
                out.push_str(&format!(" ({name})"));
            }
            out.push_str(&format!(", equip {}", v.get(1)?.as_string()));
            let date = build_regex(r"(\d{4})(\d{2})(\d{2})", "")?
                .replace(&v.get(2)?.as_string(), "${1}:${2}:${3}")
                .into_owned();
            out.push_str(&format!(", {date}, no. {}", v.get(3)?.as_string()));
            Val::Str(out)
        }
        // PDF.pm ConvertPDFDate: PDF's own layout into EXIF's.
        "ConvertPDFDate" => Val::Str(convert_pdf_date(&first.as_string())?),
        // QuickTime.pm GetRotationAngle: the angle a display matrix implies.
        "GetRotationAngle" => {
            let text = first.as_string();
            let a: Vec<f64> = text.split_whitespace().map(leading_number).collect();
            if a.len() < 2 || (a[0] == 0.0 && a[1] == 0.0) {
                return Some(Val::Undef);
            }
            // ExifTool uses a truncated pi here, and the result is rounded to
            // three decimals, so the difference shows.
            let mut angle = a[1].atan2(a[0]) * 180.0 / 3.14159;
            if angle < 0.0 {
                angle += 360.0;
            }
            Val::Num((angle * 1000.0 + 0.5).trunc() / 1000.0)
        }
        // ExifTool.pm ToFloat: the arguments become plain numbers in place.
        // Nothing reads its return value; the conversion goes on to use the
        // array it changed.
        "ToFloat" => Val::Undef,
        // Exif.pm ConvertExifText: an eight-byte encoding header, then the
        // text in whatever it names.
        "ConvertExifText" => {
            let bytes = perl_bytes(first)?;
            if bytes.len() < 8 {
                return Some(first.clone());
            }
            let id = from_bytes(&bytes[..8]);
            let body = from_bytes(&bytes[8..]);
            let ascii_flex = args.get(1).map(Val::as_string).unwrap_or_default();
            let mut str_val = if build_regex(r"^(ASCII)?(\x00|[\x00 ]+$)", "")?.is_match(&id) {
                // Truncate at the null terminator: the spec says there is not
                // one, and cameras put one there anyway.
                let cut = body.split('\0').next().unwrap_or_default().to_string();
                if ascii_flex == "1" {
                    match state.option("CharsetEXIF") {
                        Some(Val::Undef) | None => cut,
                        Some(enc) => decode_charset(&Val::Str(cut), &enc.as_string(), None, state)?
                            .as_string(),
                    }
                } else {
                    cut
                }
            } else if build_regex(r"^(UNICODE)[\x00 ]$", "")?.is_match(&id) {
                // MicrosoftPhoto writes this little-endian even inside
                // big-endian EXIF, so the byte order has to be guessed.
                decode_charset(&Val::Str(body), "UTF16", Some(&Val::Str("Unknown".into())), state)?
                    .as_string()
            } else if build_regex(r"^(JIS)[\x00 ]{5}$", "")?.is_match(&id) {
                return None; // JIS is a character set we have no table for
            } else {
                format!("{id}{body}")
            };
            while str_val.ends_with(' ') {
                str_val.pop();
            }
            Val::Str(str_val)
        }
        // GoPro.pm AddUnits: each field gets the unit the reader recorded for
        // it while parsing.
        "AddUnits" => {
            let tag = args.get(1)?.as_string();
            let Some(units) = state.tag_extra(&tag, "Units") else {
                return Some(first.clone());
            };
            let units = flatten(&[units]);
            let parts: Vec<String> =
                first.as_string().split_whitespace().map(str::to_string).collect();
            if units.len() != parts.len() {
                return Some(first.clone());
            }
            Val::Str(
                parts
                    .iter()
                    .zip(units.iter())
                    .map(|(a, u)| {
                        let u = u.as_string();
                        if u.is_empty() { a.clone() } else { format!("{a} {u}") }
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            )
        }
        // QuickTime.pm CalcRotation: the angle in the video track's matrix.
        "CalcRotation" => {
            let mut track = None;
            for i in 0..64 {
                let name =
                    if i == 0 { "HandlerType".to_string() } else { format!("HandlerType ({i})") };
                let Some(v) = state.tag_value(&name) else { break };
                if v.as_string() == "vide" {
                    track = state.tag_group1(&name);
                    break;
                }
            }
            let track = track?;
            for i in 0..64 {
                let name = if i == 0 {
                    "MatrixStructure".to_string()
                } else {
                    format!("MatrixStructure ({i})")
                };
                let Some(v) = state.tag_value(&name) else { break };
                if state.tag_group1(&name).as_deref() == Some(track.as_str()) {
                    return call_helper("GetRotationAngle", &[v], state, false);
                }
            }
            Val::Undef
        }
        // ID3.pm PrintGenre: the numbers, in brackets or separated by
        // slashes, replaced by the genres they stand for.
        "PrintGenre" => Val::Str(print_genre(&first.as_string())?),
        // Minolta.pm ConvertWhiteBalance: a mode, or an A2 mode shifted by
        // up to three settings.
        "ConvertWhiteBalance" => {
            use crate::tags::conv_tables_generated::MINOLTA_WHITE_BAL;
            #[allow(clippy::cast_possible_truncation)]
            let v = first.as_num().trunc() as i64;
            if let Some((_, name)) = MINOLTA_WHITE_BAL.iter().find(|(k, _)| *k == v) {
                return Some(Val::Str((*name).to_string()));
            }
            if v & 0xffff_0000 == 0 {
                return Some(Val::Str(format!("Unknown ({})", first.as_string())));
            }
            // Each setting of shift adds 0x10000 to the base mode.
            let base = (v & 0xff00_0000) + 0x0080_0000;
            match MINOLTA_WHITE_BAL.iter().find(|(k, _)| *k == base) {
                Some((_, name)) => {
                    #[allow(clippy::cast_precision_loss)]
                    let shift = (v - base) as f64 / 65536.0;
                    Val::Str(format!("{name}{}", format_sprintf("%+.8g", &[Val::Num(shift)])?))
                }
                None => Val::Str(format!("Unknown (0x{v:x})")),
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

/// ID3.pm PrintGenre: `(17)` and `17/20` both name genres, and a number with
/// no name of its own is shown as itself.
fn print_genre(val: &str) -> Option<String> {
    use crate::tags::conv_tables_generated::ID3_GENRE;
    let name = |n: &str| -> String {
        let k: i64 = n.parse().unwrap_or(-1);
        ID3_GENRE
            .iter()
            .find(|(g, _)| *g == k)
            .map_or_else(|| format!("Unknown ({n})"), |(_, v)| (*v).to_string())
    };
    // `(17)` -> `(Rock)`
    let bracketed = build_regex(r"\((\d+)\)", "")?;
    let mut out = String::new();
    let mut last = 0usize;
    for c in bracketed.captures_iter(val) {
        let m = c.get(0)?;
        out.push_str(&val[last..m.start()]);
        out.push_str(&format!("({})", name(c.get(1)?.as_str())));
        last = m.end();
    }
    out.push_str(&val[last..]);

    // `17` or `17/20` -> the names, keeping the slashes.
    let slashed = build_regex(r"(^|/)(\d+)($|/)", "")?;
    let mut done = String::new();
    let mut last = 0usize;
    while let Some(c) = slashed.captures(&out[last..]) {
        let m = c.get(0)?;
        let (start, end) = (last + m.start(), last + m.end());
        done.push_str(&out[last..start]);
        done.push_str(c.get(1)?.as_str());
        done.push_str(&name(c.get(2)?.as_str()));
        // The trailing slash is left for the next match to open with.
        last = end - c.get(3)?.as_str().len();
    }
    done.push_str(&out[last..]);

    // A name in brackets, possibly repeated after it, is just the name.
    let tidy = build_regex(r"^\(([^)]+)\)(.*)$", "")?;
    if let Some(c) = tidy.captures(&done) {
        let (inner, rest) = (c.get(1)?.as_str(), c.get(2)?.as_str());
        if rest.is_empty() || rest == inner {
            return Some(inner.to_string());
        }
    }
    Some(done)
}

/// PDF.pm ConvertPDFDate: `D:20260827112202+02'00'` into EXIF's layout.
fn convert_pdf_date(date: &str) -> Option<String> {
    let mut date = date.strip_prefix("D:").unwrap_or(date).to_string();
    const DEFAULT: &str = "00000101000000";
    if date.len() < DEFAULT.len() {
        date.push_str(&DEFAULT[date.len()..]);
    }
    let re = build_regex(r"^(\d{4})(\d{2})(\d{2})(\d{2})(\d{2})(\d{2})(.*)", "")?;
    let Some(c) = re.captures(&date) else { return Some(date) };
    let g = |i: usize| c.get(i).map_or("", |m| m.as_str());
    let mut out = format!("{}:{}:{} {}:{}:{}", g(1), g(2), g(3), g(4), g(5), g(6));
    let tz = g(7);
    if !tz.is_empty() {
        if build_regex(r"^\s*Z", "i")?.is_match(tz) {
            // Anything after the Z is a malformed offset OS X used to add.
            out.push('Z');
        } else if let Some(c) = build_regex(r"^\s*([-+])\s*(\d+)[': ]+(\d*)", "")?.captures(tz) {
            let g = |i: usize| c.get(i).map_or("", |m| m.as_str());
            let minutes = if g(3).is_empty() { "00" } else { g(3) };
            out.push_str(&format!("{}{}:{minutes}", g(1), g(2)));
        }
    }
    Some(out)
}

/// Canon.pm PrintAFPoints1D: the 1D bodies name their points by row letter
/// and column number, laid out in this fixed grid.
fn print_af_points_1d(val: &[u8]) -> Option<Val> {
    if val.len() != 8 {
        return Some(Val::Str("Unknown".into()));
    }
    const FOCUS_PTS: [u8; 61] = [
        0, 0, 0x04, 0x06, 0x08, 0x0a, 0x0c, 0x0e, 0x10, 0, 0, 0x21, 0x23, 0x25, 0x27, 0x29, 0x2b,
        0x2d, 0x2f, 0x31, 0x33, 0x40, 0x42, 0x44, 0x46, 0x48, 0x4a, 0x4c, 0x4d, 0x50, 0x52, 0x54,
        0x61, 0x63, 0x65, 0x67, 0x69, 0x6b, 0x6d, 0x6f, 0x71, 0x73, 0, 0, 0x84, 0x86, 0x88, 0x8a,
        0x8c, 0x8e, 0x90, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    const ROWS: &str = "  AAAAAAA  BBBBBBBBBBCCCCCCCCCCCDDDDDDDDDD  EEEEEEE     ";
    let focus = val[0];
    // `unpack('b*', ...)` is the bits of each byte, lowest first.
    let bits: Vec<bool> = val[1..]
        .iter()
        .flat_map(|b| (0..8).map(move |k| b >> k & 1 == 1))
        .collect();
    let rows: Vec<char> = ROWS.chars().collect();
    let (mut focusing, mut points, mut last_row, mut col) = (None, Vec::new(), ' ', 0usize);
    for (k, pt) in FOCUS_PTS.iter().enumerate() {
        let Some(row) = rows.get(k).copied() else { break };
        col = if row == last_row { col + 1 } else { 1 };
        last_row = row;
        let name = format!("{row}{col}");
        if focus == *pt && focusing.is_none() {
            focusing = Some(name.clone());
        }
        if bits.get(k).copied().unwrap_or(false) {
            points.push(name);
        }
    }
    let focusing = focusing.unwrap_or_else(|| {
        if focus == 0xff { "Auto".to_string() } else { format!("Unknown (0x{focus:02x})") }
    });
    Some(Val::Str(format!("{focusing} ({})", points.join(","))))
}

/// Exif.pm PrintSFR: a spatial frequency response table -- column names, then
/// one 64-bit rational per cell.
fn print_sfr(val: &[u8]) -> Option<Val> {
    if val.len() <= 4 {
        return Some(Val::Str(from_bytes(val)));
    }
    let n = usize::from(u16::from_be_bytes([val[0], val[1]]));
    let m = usize::from(u16::from_be_bytes([val[2], val[3]]));
    let rest = from_bytes(&val[4..]);
    let mut cols: Vec<String> = perl_split("\\x00", false, &rest, Some((n + 1) as f64))?;
    let pos = val.len().checked_sub(8 * n * m)?;
    if cols.len() != n + 1 || pos < 4 {
        return Some(Val::Str(from_bytes(val)));
    }
    cols.pop();
    for (i, col) in cols.iter_mut().enumerate() {
        let mut rows = Vec::new();
        for j in 0..m {
            let at = pos + 8 * (i + j * n);
            let num = u32::from_be_bytes(val.get(at..at + 4)?.try_into().ok()?);
            let den = u32::from_be_bytes(val.get(at + 4..at + 8)?.try_into().ok()?);
            rows.push(if den == 0 {
                if num == 0 { "undef".to_string() } else { "inf".to_string() }
            } else {
                format_number(f64::from(num) / f64::from(den))
            });
        }
        col.push('=');
        col.push_str(&rows.join(","));
    }
    Some(Val::Str(cols.join("; ")))
}

/// The float ExifTool digs out of a value that may carry units or other
/// text around it -- the regex CalculateLV and ToFloat both use.
fn perl_float(v: &str) -> Option<f64> {
    let re = build_regex(r"([+-]?(\d|\.\d)\d*(\.\d*)?([Ee]([+-]?\d+))?)", "")?;
    let c = re.captures(v)?;
    c.get(1)?.as_str().parse().ok()
}

/// Exif.pm RedBlueBalance. The first argument says which channel is wanted,
/// and the rest are candidate level strings; the first that yields a value
/// wins, and failing all of them the ratio of the first two.
fn red_blue_balance(args: &[Val]) -> Option<Val> {
    // Indices of R, G, G and B within the level string, one row per layout.
    const LOOKUP: [[usize; 4]; 9] = [
        [0, 1, 2, 3],
        [0, 1, 3, 2],
        [0, 2, 3, 1],
        [1, 0, 3, 2],
        [1, 0, 2, 3],
        [2, 3, 0, 1],
        [0, 1, 1, 2],
        [1, 0, 0, 2],
        [0, 256, 256, 1],
    ];
    let blue = usize::from(args.first()?.truthy());
    let rest = &args[1..];
    for (i, row) in LOOKUP.iter().enumerate() {
        let Some(levels) = rest.get(i) else { break };
        if !levels.truthy() {
            continue;
        }
        let text = levels.as_string();
        let l: Vec<f64> = text.split_whitespace().map(leading_number).collect();
        if l.len() < 2 {
            continue;
        }
        #[allow(clippy::cast_precision_loss)]
        let mut g = row[1] as f64;
        if g < 4.0 {
            if l.len() < 3 {
                continue;
            }
            g = (l.get(row[1]).copied()? + l.get(row[2]).copied()?) / 2.0;
            if g == 0.0 {
                continue;
            }
        } else if l.get(row[blue * 3]).copied()? < 4.0 {
            // Some Nikon bodies scale by one.
            g = 1.0;
        }
        return Some(Val::Num(l.get(row[blue * 3]).copied()? / g));
    }
    // Nothing matched: the ratio of the first two arguments, if both are there.
    let (a, b) = (rest.first()?, rest.get(1)?);
    if a.truthy() && b.truthy() && b.as_num() != 0.0 {
        return Some(Val::Num(a.as_num() / b.as_num()));
    }
    Some(Val::Undef)
}

/// ExifTool.pm IsInt: an optionally signed run of digits, nothing else.
fn is_perl_int(v: &str) -> bool {
    let t = v.strip_prefix(['+', '-']).unwrap_or(v);
    !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit())
}

/// ExifTool.pm Decode, for the character sets its conversions name.
///
/// The destination is the reader's internal set, UTF-8. Charset.pm decomposes
/// the value into code points and Recompose packs them back, truncating at the
/// first NUL -- both of which show in the result, so both are here. A set we
/// have no table for refuses by name rather than passing the bytes through as
/// if they had been converted.
fn decode_charset(
    val: &Val,
    from: &str,
    order: Option<&Val>,
    state: &dyn ParseState,
) -> Option<Val> {
    use crate::formats::font_charset;

    let bytes = perl_bytes(val)?;
    // `$from or $from = $$self{OPTIONS}{Charset}`: an empty charset means the
    // reader's own, and a reader that was not told otherwise reads UTF-8 --
    // which is also what it converts to, so there is nothing to do.
    let named;
    let from = if from.is_empty() {
        named = state.option("Charset").map_or_else(
            || "UTF8".to_string(),
            |c| match c {
                Val::Undef => "UTF8".to_string(),
                other => other.as_string(),
            },
        );
        named.as_str()
    } else {
        from
    };
    if bytes.is_empty() || from == "UTF8" || from == "ASCII" {
        return Some(val.clone());
    }
    let single = match from {
        "Latin" => Some(&font_charset::LATIN),
        "Latin2" => Some(&font_charset::LATIN2),
        "MacRoman" => Some(&font_charset::MACROMAN),
        _ => None,
    };
    if let Some(table) = single {
        // ExifTool short-circuits a value that has nothing to remap, and
        // returns it exactly as it came -- trailing NUL included, which the
        // converting path would have cut.
        if !bytes.iter().any(|b| *b >= 0x80) {
            return Some(val.clone());
        }
        let decoded = table.decode(&bytes);
        return Some(Val::Str(
            decoded.split('\0').next().unwrap_or_default().to_string(),
        ));
    }
    if !matches!(from, "UCS2" | "UTF16" | "Unicode") {
        return None; // a character set we have no table for
    }
    // Two bytes per character. A byte-order mark wins over the argument, and
    // without either it is the order the file is being read in.
    let (mut data, mut big) = (&bytes[..], None);
    if data.starts_with(&[0xfe, 0xff]) {
        data = &data[2..];
        big = Some(true);
    } else if data.starts_with(&[0xff, 0xfe]) {
        data = &data[2..];
        big = Some(false);
    }
    let named = order.map(Val::as_string).unwrap_or_default();
    let mut guess = false;
    let mut big = match big {
        Some(b) => b,
        None if named == "MM" || named == "II" => named == "MM",
        None => {
            // "Unknown" means the value's own bytes decide; anything else
            // means the order the file is being read in.
            guess = named == "Unknown";
            let order = if guess { "II" } else { state.byte_order()? };
            order == "MM"
        }
    };
    let units = |big: bool| -> Vec<u16> {
        data.chunks_exact(2)
            .map(|p| {
                if big {
                    u16::from_be_bytes([p[0], p[1]])
                } else {
                    u16::from_le_bytes([p[0], p[1]])
                }
            })
            .collect()
    };
    let mut uni = units(big);
    if guess {
        // Charset.pm's test: the byte with more distinct values is the low
        // one, and failing that the byte more often zero is the high one.
        let (mut hi, mut lo) = (std::collections::HashSet::new(), std::collections::HashSet::new());
        let (mut zh, mut zl) = (0usize, 0usize);
        for u in &uni {
            hi.insert(u >> 8);
            lo.insert(u & 0xff);
            if u & 0xff00 == 0 {
                zh += 1;
            }
            if u & 0x00ff == 0 {
                zl += 1;
            }
        }
        if hi.len() > lo.len() || (hi.len() == lo.len() && zl > zh) {
            big = !big;
            uni = units(big);
        }
    }
    let mut out = String::new();
    let mut k = 0usize;
    while k < uni.len() {
        let u = uni[k];
        if u == 0 {
            break; // Recompose truncates at the first NUL
        }
        // UTF16 joins a surrogate pair into one character; UCS2 does not, and
        // Perl would pack the halves as characters of their own -- which Rust
        // has no way to hold, so that declines rather than differ.
        if from == "UTF16" && u & 0xfc00 == 0xd800 && k + 1 < uni.len() && uni[k + 1] & 0xfc00 == 0xdc00
        {
            let cp = 0x10000 + ((u32::from(u) & 0x3ff) << 10) + (u32::from(uni[k + 1]) & 0x3ff);
            out.push(char::from_u32(cp)?);
            k += 2;
            continue;
        }
        out.push(char::from_u32(u32::from(u))?);
        k += 1;
    }
    Some(Val::Str(out))
}

/// XMP.pm DecodeBase64. ExifTool truncates at the first character that cannot
/// be part of base64 data, and ignores white space within.
fn decode_base64(text: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bits: Vec<u8> = Vec::new();
    for c in text.bytes() {
        if c == b'=' {
            break;
        }
        if c.is_ascii_whitespace() {
            continue;
        }
        let Some(k) = ALPHABET.iter().position(|a| *a == c) else {
            break; // truncate at the first character that is not base64
        };
        #[allow(clippy::cast_possible_truncation)]
        bits.push(k as u8);
    }
    let mut out = Vec::with_capacity(bits.len() * 3 / 4);
    for chunk in bits.chunks(4) {
        let mut acc: u32 = 0;
        for (i, v) in chunk.iter().enumerate() {
            acc |= u32::from(*v) << (18 - 6 * i);
        }
        for i in 0..chunk.len().saturating_sub(1) {
            #[allow(clippy::cast_possible_truncation)]
            out.push((acc >> (16 - 8 * i)) as u8);
        }
    }
    Some(out)
}

/// GPS.pm ConvertTimeStamp: hours, minutes and seconds as three numbers,
/// re-normalised and printed as a clock time.
fn convert_gps_time_stamp(val: &str) -> Option<String> {
    let mut it = val.split_whitespace().map(|p| p.parse::<f64>().unwrap_or(0.0));
    let (h, m, s) = (it.next().unwrap_or(0.0), it.next().unwrap_or(0.0), it.next().unwrap_or(0.0));
    let mut f = (h * 60.0 + m) * 60.0 + s;
    let mut h = (f / 3600.0).trunc();
    f -= h * 3600.0;
    let mut m = (f / 60.0).trunc();
    f -= m * 60.0;
    let mut ss = format_sprintf("%012.9f", &[Val::Num(f)])?;
    if leading_number(&ss) >= 60.0 {
        ss = "00".to_string();
        m += 1.0;
        if m >= 60.0 {
            m -= 60.0;
            h += 1.0;
        }
    } else {
        // Trim the trailing zeros, and the decimal point with them.
        ss = ss.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    Some(format!("{h:02}:{m:02}:{ss}"))
}

/// RIFF.pm ConvertRIFFDate: the AVI form, and the two cameras that got it
/// wrong in their own ways.
fn convert_riff_date(val: &str) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let part: Vec<&str> = val.split_whitespace().collect();
    if part.len() >= 5 {
        let name = part[1].to_lowercase();
        if let Some(mon) = MONTHS.iter().position(|m| m.to_lowercase() == name) {
            return format!("{:0>4}:{:02}:{:0>2} {}", part[4], mon + 1, part[2], part[3]);
        }
    }
    // "2001/ 1/27  1:42PM", and "2005/11/28/ 09:19".
    if let Some(re) = build_regex(r"(\d{4})/\s*(\d+)/\s*(\d+)/?\s+(\d+):\s*(\d+)\s*(P?)", "") {
        if let Some(c) = re.captures(val) {
            let g = |i: usize| c.get(i).map_or("", |m| m.as_str());
            let hour = g(4).parse::<i64>().unwrap_or(0) + if g(6) == "P" { 12 } else { 0 };
            return format!(
                "{:0>4}:{:0>2}:{:0>2} {hour:02}:{:0>2}:00",
                g(1),
                g(2),
                g(3),
                g(5)
            );
        }
    }
    // "2002-12-16  15:35:01".
    if let Some(re) = build_regex(r"(\d{4})[-/](\d+)[-/](\d+)\s+(\d+:\d+:\d+)", "") {
        if let Some(c) = re.captures(val) {
            let g = |i: usize| c.get(i).map_or("", |m| m.as_str());
            return format!("{}:{}:{} {}", g(1), g(2), g(3), g(4));
        }
    }
    val.to_string()
}

/// ExifTool.pm ConvertTimeSpan: a count of ticks into a readable span. A value
/// that is not a number is passed through untouched, as Perl does.
fn convert_time_span(v: &Val, mult: Option<f64>) -> Option<Val> {
    let text = v.as_string();
    if text.trim().parse::<f64>().is_err() || v.as_num() == 0.0 {
        return Some(v.clone());
    }
    let mult = mult.unwrap_or(0.0);
    let val = if mult == 0.0 { v.as_num() } else { v.as_num() * mult };
    Some(Val::Str(if val < 60.0 {
        format!("{} seconds", format_number(val))
    } else if val < 3600.0 {
        let fmt = if mult >= 60.0 { "%d" } else { "%.1f" };
        let plural = if val == 60.0 && mult != 0.0 { "" } else { "s" };
        format!("{} minute{plural}", format_sprintf(fmt, &[Val::Num(val / 60.0)])?)
    } else if val < 24.0 * 3600.0 {
        format!("{} hours", format_sprintf("%.1f", &[Val::Num(val / 3600.0)])?)
    } else {
        format!("{} days", format_sprintf("%.1f", &[Val::Num(val / (24.0 * 3600.0))])?)
    }))
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

    /// The two tables ported as data, and the functions that read them.
    /// Perl gave every expected value here.
    #[test]
    fn genres_and_minolta_white_balance() {
        let g = |v: &str| {
            eval("Image::ExifTool::ID3::PrintGenre($val)", &Val::Str(v.into()))
                .unwrap()
                .as_string()
        };
        assert_eq!(g("(17)"), "Rock");
        assert_eq!(g("17"), "Rock");
        assert_eq!(g("17/20"), "Rock/Alternative");
        assert_eq!(g("(17)Rock"), "Rock");
        assert_eq!(g("(200)"), "(Unknown (200))");
        let wb = |v: f64| {
            eval("Image::ExifTool::Minolta::ConvertWhiteBalance($val)", &n(v)).unwrap().as_string()
        };
        assert_eq!(wb(2.0), "Cloudy");
        assert_eq!(wb(f64::from(0x0181_0000)), "Daylight+1");
        assert_eq!(wb(99.0), "Unknown (99)");
    }

    /// The batch of named printers around file sizes, levels and patterns.
    /// Perl produced every expected value here first.
    #[test]
    fn more_named_printers() {
        let st = |e: &str, v: &str| {
            eval(e, &Val::Str(v.into())).unwrap().as_string()
        };
        assert_eq!(st("Image::ExifTool::ICC_Profile::HexID($val)", "1 2 255 0"), "0102ff00");
        assert_eq!(st("Image::ExifTool::ICC_Profile::HexID($val)", "0 0 0"), "0");
        let wb = |v: f64| {
            eval("Image::ExifTool::MinoltaRaw::ConvertWBMode($val)", &n(v)).unwrap().as_string()
        };
        assert_eq!(wb(f64::from(0x63)), "Tungsten (-2)");
        assert_eq!(wb(2.0), "Cloudy");
        let iso = |v: f64| {
            eval("Image::ExifTool::Canon::CameraISO($val)", &n(v)).unwrap().as_string()
        };
        assert_eq!(iso(f64::from(0x4064)), "100");
        assert_eq!(iso(17.0), "100");
        assert_eq!(iso(99.0), "Unknown (99)");
        assert_eq!(
            eval("Image::ExifTool::Canon::PrintFocalRange(24,70,1)", &n(0.0)).unwrap().as_string(),
            "24.0 - 70.0 mm"
        );
        assert_eq!(
            eval("Image::ExifTool::Canon::PrintFocalRange(50,50)", &n(0.0)).unwrap().as_string(),
            "50.0 mm"
        );
        assert_eq!(
            st("Image::ExifTool::Exif::PrintCFAPattern($val)", "2 2 0 1 1 2"),
            "[Red,Green][Green,Blue]"
        );
        assert_eq!(st("Image::ExifTool::Exif::PrintCFAPattern($val)", "1"), "<truncated data>");
        let size = |v: f64| eval("ConvertFileSize($val)", &n(v)).unwrap().as_string();
        assert_eq!(size(1500.0), "1500 bytes");
        assert_eq!(size(2500.0), "2.5 kB");
        assert_eq!(size(3_000_000.0), "3.0 MB");
        assert_eq!(st("PrintHex($val)", "AB"), "41 42");
        assert!(
            (eval("Image::ExifTool::Exif::CalculateLV(2.8, 0.01, 100)", &n(0.0))
                .unwrap()
                .as_num()
                - 9.614_709_844_115_21)
                .abs()
                < 1e-12
        );
        assert_eq!(
            eval("Image::ExifTool::Exif::RedBlueBalance(0,$val)", &Val::Str("512 256 256 512".into()))
                .unwrap()
                .as_num(),
            2.0
        );
        assert_eq!(
            eval("Image::ExifTool::Exif::RedBlueBalance(1,$val)", &Val::Str("512 256 256 640".into()))
                .unwrap()
                .as_num(),
            2.5
        );

        struct Quiet;
        impl ParseState for Quiet {
            fn member(&self, _: &str) -> Option<Val> {
                None
            }
            fn option(&self, _: &str) -> Option<Val> {
                Some(Val::Num(0.0))
            }
        }
        assert_eq!(
            eval_with("$self->Printable($val, 0)", &Val::Str("a\u{1}b\0c".into()), &Quiet)
                .unwrap()
                .as_string(),
            "a.bc"
        );
        assert_eq!(
            eval_with("$self->Printable($val)", &Val::Str("x".repeat(80)), &Quiet)
                .unwrap()
                .as_string(),
            format!("{}[snip]", "x".repeat(54))
        );
    }

    /// `map` and `grep` run a block per element, and the statement modifiers
    /// decide whether a statement runs at all. Perl gave every value here.
    #[test]
    fn map_grep_and_the_statement_modifiers() {
        let s = |e: &str, v: &str| eval(e, &Val::Str(v.into())).unwrap().as_string();
        assert_eq!(s("join(\" \",map({ $_/8192 } split(\" \",$val)))", "8192 16384"), "1 2");
        assert_eq!(
            s("join \" \", map { sprintf(\"%.5g\",$_) } split(\" \",$val)", "1 2 3"),
            "1 2 3"
        );
        assert_eq!(
            s("my @a=($val=~/.{4}/sg); @a=grep(!/\\0/,@a); join(\" \",@a)", "ab\0dcdef"),
            "cdef"
        );
        assert_eq!(
            eval("$val += 4294967296 if $val < 0 and $val >= -2147483648; $val * 1e-7", &n(-100.0))
                .unwrap()
                .as_num(),
            429.496_719_6
        );
        assert_eq!(
            eval("$val > 1800 and $val -= 3600; $val / 10", &n(2000.0)).unwrap().as_num(),
            -160.0
        );
        assert_eq!(s("\"${val} m\"", "5"), "5 m");
        assert_eq!(
            s("sprintf(\"%3d %4d\" . \" %3d %4d\" x 1, split(\" \",$val))", "1 2 3 4"),
            "  1    2   3    4"
        );
    }

    /// Character sets, base64, and four more of ExifTool's printers. Every
    /// expected value came back from Perl before it was written down.
    #[test]
    fn character_sets_and_base64() {
        struct Reader;
        impl ParseState for Reader {
            fn member(&self, _: &str) -> Option<Val> {
                None
            }
            fn option(&self, _: &str) -> Option<Val> {
                Some(Val::Undef)
            }
            fn byte_order(&self) -> Option<&str> {
                Some("II")
            }
        }
        let dec = |e: &str, bytes: &[u8]| {
            let v = Val::Str(bytes.iter().map(|b| *b as char).collect());
            eval_with(e, &v, &Reader).unwrap().as_string()
        };
        assert_eq!(dec("$self->Decode($val, \"Latin\")", b"caf\xe9"), "caf\u{e9}");
        // 0xd5 is a right single quote in MacRoman, not an O with a tilde.
        assert_eq!(dec("$self->Decode($val, \"MacRoman\")", b"Mac\xd5s"), "Mac\u{2019}s");
        assert_eq!(dec("$self->Decode($val,\"UCS2\",\"II\")", b"h\0i\0\0\0"), "hi");
        assert_eq!(dec("$self->Decode($val, \"UTF16\", \"MM\")", b"\0h\0i"), "hi");
        // Nothing to remap, so ExifTool hands the value straight back -- NUL
        // and all, where the converting path would have cut it.
        assert_eq!(dec("$self->Decode($val, \"Latin\")", b"plain\0tail"), "plain\0tail");
        // Without a byte order named, and no reader to ask, UTF16 declines.
        assert!(eval("$self->Decode($val, \"UTF16\")", &Val::Str("h\0i\0".into())).is_none());
        assert_eq!(
            eval("Image::ExifTool::XMP::DecodeBase64($val)", &Val::Str("SGVsbG8sIHdvcmxkIQ==".into()))
                .unwrap(),
            Val::Binary(b"Hello, world!".to_vec())
        );
        let pev = |v: f64| eval("Image::ExifTool::Pentax::PentaxEv($val)", &n(v)).unwrap().as_num();
        assert!((pev(11.0) - 4.0 / 3.0).abs() < 1e-12);
        assert!((pev(13.0) - 5.0 / 3.0).abs() < 1e-12);
        assert!((pev(-11.0) + 4.0 / 3.0).abs() < 1e-12);
        assert_eq!(pev(16.0), 2.0);
        let tz = |v: f64| {
            eval("Image::ExifTool::TimeZoneString($val)", &n(v)).unwrap().as_string()
        };
        assert_eq!(tz(-330.0), "-05:30");
        assert_eq!(tz(120.0), "+02:00");
        assert_eq!(eval("IsInt($val)", &Val::Str("-12".into())).unwrap().as_num(), 1.0);
        assert_eq!(eval("IsInt($val)", &Val::Str("1.5".into())).unwrap().as_num(), 0.0);
        assert_eq!(eval("IsFloat($val)", &Val::Str("1.5e3".into())).unwrap().as_num(), 1.0);
    }

    /// `\$val` is not a value to print: it is ExifTool saying the tag holds
    /// binary data. And `$self->Options(...)` asks the reader how it was
    /// configured, which an unconfigured reader answers with undef.
    #[test]
    fn binary_references_and_reader_options() {
        let long: String = "x".repeat(40);
        let v = eval("length($val) > 32 ? \\$val : $val", &Val::Str(long.clone())).unwrap();
        assert_eq!(v, Val::Binary(long.into_bytes()));
        assert_eq!(
            eval("length($val) > 32 ? \\$val : $val", &Val::Str("short".into())).unwrap(),
            Val::Str("short".into())
        );
        // No reader configuration at all: the option is unknown, not unset.
        assert!(eval("$self->Options(\"Unknown\") ? $val : $val & 0x7ff", &n(4096.0)).is_none());

        struct Defaults;
        impl ParseState for Defaults {
            fn member(&self, _: &str) -> Option<Val> {
                None
            }
            fn option(&self, _: &str) -> Option<Val> {
                Some(Val::Undef)
            }
        }
        assert_eq!(
            eval_with("$self->Options(\"Unknown\") ? $val : $val & 0x7ff", &n(4096.0), &Defaults)
                .unwrap()
                .as_num(),
            0.0
        );
    }

    /// The named printers ported this round. Every expected value came out of
    /// Perl before it was written down.
    #[test]
    fn the_named_printers() {
        let guid: String = (1u8..=16).map(|b| b as char).collect();
        assert_eq!(
            eval("Image::ExifTool::ASF::GetGUID($val)", &Val::Str(guid)).unwrap().as_string(),
            "04030201-0605-0807-090A-0B0C0D0E0F10"
        );
        let ev = |v: f64| eval("Image::ExifTool::Canon::CanonEv($val)", &n(v)).unwrap().as_num();
        assert!((ev(44.0) - 4.0 / 3.0).abs() < 1e-12);
        assert!((ev(-44.0) + 4.0 / 3.0).abs() < 1e-12);
        assert_eq!(ev(32.0), 1.0);
        let curve = format!("3 0 0 128 128 255 255{}", " 0".repeat(14));
        assert_eq!(
            eval("Image::ExifTool::CanonVRD::ToneCurvePrint($val)", &Val::Str(curve))
                .unwrap()
                .as_string(),
            "(0,0) (128,128) (255,255)"
        );
        let ts = |v: &str| {
            eval("Image::ExifTool::GPS::ConvertTimeStamp($val)", &Val::Str(v.into()))
                .unwrap()
                .as_string()
        };
        assert_eq!(ts("11 22 2.5"), "11:22:02.5");
        assert_eq!(ts("1 2 3"), "01:02:03");
        assert_eq!(
            eval("Image::ExifTool::LNK::DOSTime($val)", &n(f64::from(0x2c8a_5a1fu32)))
                .unwrap()
                .as_string(),
            "2025:00:31 05:36:20"
        );
        let riff = |v: &str| {
            eval("Image::ExifTool::RIFF::ConvertRIFFDate($val)", &Val::Str(v.into()))
                .unwrap()
                .as_string()
        };
        assert_eq!(riff("Mon Mar 10 15:04:43 2003"), "2003:03:10 15:04:43");
        assert_eq!(riff("2001/ 1/27  1:42PM"), "2001:01:27 13:42:00");
        assert_eq!(riff("2002-12-16  15:35:01"), "2002:12:16 15:35:01");
        assert_eq!(eval("ConvertTimeSpan($val)", &n(30.0)).unwrap().as_string(), "30 seconds");
        assert_eq!(eval("ConvertTimeSpan($val)", &n(90.0)).unwrap().as_string(), "1.5 minutes");
        assert_eq!(eval("ConvertTimeSpan($val, 60)", &n(60.0)).unwrap().as_string(), "1.0 hours");
        assert_eq!(eval("ConvertTimeSpan($val)", &n(90000.0)).unwrap().as_string(), "1.0 days");
        assert_eq!(
            eval("Image::ExifTool::DecodeBits($val, undef, 16)", &Val::Str("5 2".into()))
                .unwrap()
                .as_string(),
            "0,2,17"
        );
        assert_eq!(
            eval("Image::ExifTool::DecodeBits($val, undef, 16)", &Val::Str("0".into()))
                .unwrap()
                .as_string(),
            "(none)"
        );
        // A lookup table is a hash we have no way to name here.
        assert!(eval("Image::ExifTool::DecodeBits($val, %lookup, 16)", &n(1.0)).is_none());
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
        assert!(eval("Image::ExifTool::XMP::PrintLensID($self, @val)", &n(1.0)).is_none());
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
        assert!(eval("Image::ExifTool::TNEF::DecompressRTF($self,$val)", &n(1.0)).is_none());
        assert!(eval("$$self{Model}", &n(1.0)).is_none());
        assert!(eval("$val / 0", &n(1.0)).is_none());
    }
}
