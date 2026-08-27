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
        val,
    };
    let v = p.ternary()?;
    p.skip_ws();
    if p.i == p.s.len() { Some(v) } else { None }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    val: &'a Val,
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
    fn helper_call(&mut self) -> Option<Val> {
        const HELPERS: [(&str, fn(f64) -> String); 3] = [
            ("Image::ExifTool::Exif::PrintExposureTime", print_exposure_time),
            ("Image::ExifTool::Exif::PrintFNumber", print_f_number),
            ("Image::ExifTool::Exif::PrintFraction", crate::tags::exif::print_fraction),
        ];
        let save = self.i;
        for (name, f) in HELPERS {
            if self.peek(name) {
                self.eat(name);
                if !self.eat("(") {
                    self.i = save;
                    return None;
                }
                let v = self.ternary()?;
                if !self.eat(")") {
                    self.i = save;
                    return None;
                }
                return Some(Val::Str(f(v.as_num())));
            }
        }
        None
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
        return "0".to_string();
    }
    let s = format!("{val:.1}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
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
        assert!(eval("Image::ExifTool::GPS::ToDMS($self, $val, 1)", &n(1.0)).is_none());
        assert!(eval("$val =~ s/ +$//", &n(1.0)).is_none());
        assert!(eval("$$self{Model}", &n(1.0)).is_none());
        assert!(eval("$val / 0", &n(1.0)).is_none());
    }
}
