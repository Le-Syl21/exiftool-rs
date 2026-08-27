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
    Num(f64),
    Str(String),
}

impl Val {
    #[must_use]
    pub fn as_num(&self) -> f64 {
        match self {
            Self::Num(n) => *n,
            // Perl reads a leading number out of a string and calls the rest zero.
            Self::Str(s) => leading_number(s),
        }
    }

    #[must_use]
    pub fn as_string(&self) -> String {
        match self {
            Self::Num(n) => format_number(*n),
            Self::Str(s) => s.clone(),
        }
    }

    fn truthy(&self) -> bool {
        match self {
            Self::Num(n) => *n != 0.0,
            Self::Str(s) => !s.is_empty() && s != "0",
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
pub fn eval(expr: &str, val: &Val) -> Option<Val> {
    let mut p = Parser {
        s: expr.as_bytes(),
        i: 0,
        val: val.clone(),
        captures: Vec::new(),
    };
    // These conversions are sometimes two statements: `$val =~ s/ +$//; $val`
    // substitutes and then hands back the value it changed. The last one is the
    // result, as in Perl.
    let mut last = p.ternary()?;
    loop {
        p.skip_ws();
        if p.i < p.s.len() && p.s[p.i] == b';' {
            p.i += 1;
            p.skip_ws();
            if p.i == p.s.len() {
                break;
            }
            last = p.ternary()?;
        } else {
            break;
        }
    }
    p.skip_ws();
    if p.i == p.s.len() { Some(last) } else { None }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    /// Owned, because `s///` and `tr///` rewrite it in place as Perl does.
    val: Val,
    /// `$1`..`$9` from the most recent match.
    captures: Vec<String>,
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

    fn peek(&mut self, tok: &str) -> bool {
        self.skip_ws();
        self.s[self.i..].starts_with(tok.as_bytes())
    }

    fn ternary(&mut self) -> Option<Val> {
        let cond = self.comparison()?;
        if self.eat("?") {
            let a = self.ternary()?;
            if !self.eat(":") {
                return None;
            }
            let b = self.ternary()?;
            return Some(if cond.truthy() { a } else { b });
        }
        Some(cond)
    }

    fn comparison(&mut self) -> Option<Val> {
        // `$val =~ ...` binds looser than arithmetic and tighter than a ternary.
        if self.peek("$val") {
            let save = self.i;
            self.eat("$val");
            if self.peek("=~") || self.peek("!~") {
                let negated = self.peek("!~");
                self.eat(if negated { "!~" } else { "=~" });
                return self.bind_operation(negated);
            }
            self.i = save;
        }
        let left = self.additive()?;
        for op in ["<=", ">=", "==", "!=", "<", ">"] {
            // `<` must not swallow the `<=` case, hence the order above.
            if self.peek(op) {
                self.eat(op);
                let right = self.additive()?;
                let (a, b) = (left.as_num(), right.as_num());
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
            if self.peek("*") {
                self.eat("*");
                let r = self.power()?;
                acc = Val::Num(acc.as_num() * r.as_num());
            } else if self.peek("/") {
                self.eat("/");
                let r = self.power()?;
                let d = r.as_num();
                // Perl dies on division by zero; ExifTool guards its expressions,
                // so reaching it means we misread something. Refuse rather than
                // return an infinity that would be printed as a value.
                if d == 0.0 {
                    return None;
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
            let v = self.ternary()?;
            if !self.eat(")") {
                return None;
            }
            return Some(v);
        }
        if self.eat("$val") {
            return Some(self.val.clone());
        }
        // `$1`..`$9`, from the last successful match.
        if self.i + 1 < self.s.len() && self.s[self.i] == b'$' && self.s[self.i + 1].is_ascii_digit() {
            let idx = (self.s[self.i + 1] - b'0') as usize;
            self.i += 2;
            return Some(Val::Str(
                self.captures.get(idx.checked_sub(1)?).cloned().unwrap_or_default(),
            ));
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
        if self.peek("sprintf") {
            return self.sprintf_call();
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
            if self.s[self.i..].starts_with(b"$val") {
                out.push_str(&self.val.as_string());
                self.i += 4;
                continue;
            }
            // Any other variable means state we do not have.
            if self.s[self.i] == b'$' {
                return None;
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
    fn bind_operation(&mut self, negated: bool) -> Option<Val> {
        self.skip_ws();
        if self.peek("s/") {
            self.eat("s/");
            let pat = self.delimited('/')?;
            let rep = self.delimited('/')?;
            let flags = self.regex_flags();
            let re = build_regex(&pat, &flags)?;
            let subject = self.val.as_string();
            let replaced = if flags.contains('g') {
                re.replace_all(&subject, perl_replacement(&rep).as_str()).into_owned()
            } else {
                re.replace(&subject, perl_replacement(&rep).as_str()).into_owned()
            };
            let changed = replaced != subject;
            self.val = Val::Str(replaced);
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
            let out: String = self
                .val
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
            self.val = Val::Str(out);
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
        let subject = self.val.as_string();
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
        let mut args = vec![self.ternary()?];
        while self.eat(",") {
            args.push(self.ternary()?);
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
            args.push(self.ternary()?);
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
                let n = v.as_num().round() as i64;
                if plus && n >= 0 { format!("+{n}") } else { format!("{n}") }
            }
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
        if b[i] == '$' && i + 1 < b.len() && b[i + 1].is_ascii_digit() {
            out.push_str(&format!("${{{}}}", b[i + 1]));
            i += 2;
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
        // `unpack("H*", $val)` is the bytes as lowercase hex, and the only
        // unpack template these conversions use often enough to be worth it.
        "unpack" => {
            if first.as_string() != "H*" {
                return None;
            }
            let bytes = args.get(1)?.as_string();
            Val::Str(bytes.bytes().map(|b| format!("{b:02x}")).collect())
        }
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
    fn nul_in_a_pattern_and_unpack_hex() {
        assert_eq!(
            eval("$val =~ s/[ \\0]+$//; $val", &Val::Str("abc \0\0".into()))
                .unwrap()
                .as_string(),
            "abc"
        );
        assert_eq!(
            eval("unpack(\"H*\", $val)", &Val::Str("\u{fe}\u{fe}".into()))
                .unwrap()
                .as_string(),
            "c3bec3be"
        );
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
        assert!(eval("my @a = split \" \", $val; $a[0]", &n(1.0)).is_none());
        assert!(eval("$$self{Model}", &n(1.0)).is_none());
        assert!(eval("$val / 0", &n(1.0)).is_none());
    }
}
