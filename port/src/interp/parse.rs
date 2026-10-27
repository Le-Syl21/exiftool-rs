//! Parser for the Perl subset.
//!
//! Perl cannot be tokenized ahead of parsing: `/` is a division after a term
//! and a pattern where a term is expected, `{` is a block or a hash, `x` is
//! an operator or a name. The scanner therefore lives in the parser, which
//! always knows which of the two it expects, as perl's own `toke.c` does.

use std::cell::RefCell;
use std::rc::Rc;

use super::ast::*;
use crate::perl::Value;

#[derive(Debug, Clone)]
pub struct ParseError {
    pub msg: String,
    pub line: usize,
    /// The text after the error, for perl's `near "..."`.
    pub near: String,
    /// A fatal compile error with its own message (perl's "Search pattern
    /// not terminated"), rather than a "syntax error".
    pub fatal: bool,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}, near \"{}\" at line {}", self.msg, self.near, self.line)
    }
}

type R<T> = Result<T, ParseError>;

/// Parse a whole program (an eval string, a config file).
pub fn parse_program(src: &[u8], package: &str) -> R<Block> {
    let mut p = Parser { s: src, pos: 0, package: Rc::from(package), line_pos: 0, line: 1 };
    let block = p.statements()?;
    p.ws();
    if p.pos < p.s.len() {
        return Err(p.err("syntax error"));
    }
    Ok(block)
}

thread_local! {
    /// The prototypes of the subs declared in the Perl code parsed so far.
    static PROTOS: RefCell<std::collections::HashMap<String, String>> = RefCell::new(std::collections::HashMap::new());
    /// The modules loaded from their data dumps (see module_loaded).
    static LOADED: RefCell<std::collections::HashSet<String>> = RefCell::new(std::collections::HashSet::new());
}

/// The prototype of a sub, by full name: declared in parsed code, or one of
/// ExifTool's (its modules may come from data dumps, without their source).
fn prototype_of(full: &str) -> Option<String> {
    if let Some(p) = PROTOS.with(|m| m.borrow().get(full).cloned()) {
        return Some(p);
    }
    // perl knows a prototype only once the file declaring it is loaded
    let t = crate::runtime::prototypes::PROTOTYPES;
    let i = t.binary_search_by(|(k, _, _)| (*k).cmp(full)).ok()?;
    LOADED.with(|l| l.borrow().contains(t[i].2)).then(|| t[i].1.to_string())
}

/// A module (by its file's module name) is loaded: the prototypes it
/// declares apply to the code parsed from now on.
pub fn module_loaded(key: &str) {
    LOADED.with(|l| {
        l.borrow_mut().insert(key.to_string());
    });
}

/// A call to a sub with a prototype: each `$` (or `_`) argument is
/// evaluated in scalar context, as perl compiles it.
fn apply_prototype(proto: &str, args: Vec<Expr>) -> Vec<Expr> {
    let slots: Vec<char> = proto.chars().filter(|c| *c != ';').collect();
    let mut rest = false;
    args.into_iter()
        .enumerate()
        .map(|(i, a)| {
            if rest {
                return a;
            }
            match slots.get(i) {
                Some('$') | Some('_') if !matches!(a, Expr::Scalar(_) | Expr::Const(_) | Expr::ScalarDeref(_)) => Expr::Call { name: Rc::from("scalar"), args: vec![a], amp: false },
                Some('@') | Some('%') => {
                    // the rest of the list as it is
                    rest = true;
                    a
                }
                _ => a,
            }
        })
        .collect()
}

/// Parse the text of an interpolated string (the inside of `"..."`).
pub fn parse_interpolated(raw: &[u8], package: &str) -> R<Vec<Part>> {
    let mut p = Parser { s: raw, pos: 0, package: Rc::from(package), line_pos: 0, line: 1 };
    p.interpolate(raw, false)
}

struct Parser<'a> {
    s: &'a [u8],
    pos: usize,
    package: Rc<str>,
    /// The line at `line_pos` (counted incrementally for Stmt::Line).
    line_pos: usize,
    line: u32,
}

/// Builtins that take one argument and bind tighter than comparisons.
const NAMED_UNARY: &[&str] = &[
    "defined", "ref", "scalar", "lc", "uc", "lcfirst", "ucfirst", "length", "chr", "ord", "hex", "oct", "abs",
    "int", "sqrt", "log", "exp", "sin", "cos", "quotemeta", "rand", "srand", "exists", "delete", "each", "keys",
    "values", "shift", "pop", "chomp", "chop", "chdir", "rmdir", "readlink", "undef", "study", "pos", "exit",
    "umask", "localtime", "gmtime", "alarm", "sleep", "require", "caller", "lock", "fc", "close", "fileno",
    "chroot", "readline", "uc", "lc", "eof", "getpwuid", "getgrgid",
];

/// Builtins taking a list.
const LIST_OPS: &[&str] = &[
    "join", "push", "unshift", "splice", "sprintf", "die", "warn", "split", "pack", "unpack", "reverse", "bless",
    "substr", "index", "rindex", "atan2", "open", "binmode", "seek", "read", "sysread", "unlink", "mkdir",
    "chmod", "utime", "select", "sprintf", "wantarray", "time", "times", "local", "kill", "sysseek", "syswrite",
    "tell", "truncate", "rename", "opendir", "readdir", "closedir", "system", "exec", "sprintf", "crypt",
    "lcfirst", "vec", "formline", "waitpid", "stat", "lstat",
];

/// Builtins whose first argument may be a bareword filehandle.
const FH_FUNCS: &[&str] = &["stat", "lstat", "open", "close", "binmode", "eof", "read", "sysread", "seek", "tell", "fileno", "truncate", "flock", "readline", "select", "opendir", "readdir", "closedir", "syswrite", "sysseek"];

/// Builtins that may be called without arguments and take none.
const NO_ARGS: &[&str] = &["time", "times", "wait", "wantarray"];

fn ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn ident_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn closing(c: u8) -> u8 {
    match c {
        b'(' => b')',
        b'[' => b']',
        b'{' => b'}',
        b'<' => b'>',
        c => c,
    }
}

impl<'a> Parser<'a> {
    fn err(&self, msg: &str) -> ParseError {
        let line = 1 + self.s[..self.pos.min(self.s.len())].iter().filter(|&&b| b == b'\n').count();
        let near: String = String::from_utf8_lossy(&self.s[self.pos.min(self.s.len())..(self.pos + 20).min(self.s.len())]).into();
        ParseError { msg: msg.to_string(), line, near, fatal: false }
    }

    fn fatal(&self, msg: &str) -> ParseError {
        ParseError { fatal: true, ..self.err(msg) }
    }

    fn at(&self, i: usize) -> u8 {
        *self.s.get(self.pos + i).unwrap_or(&0)
    }

    fn rest(&self) -> &'a [u8] {
        &self.s[self.pos.min(self.s.len())..]
    }

    /// Skip whitespace, comments and POD.
    fn ws(&mut self) {
        loop {
            while self.pos < self.s.len() && self.s[self.pos].is_ascii_whitespace() {
                self.pos += 1;
            }
            if self.pos < self.s.len() && self.s[self.pos] == b'#' {
                while self.pos < self.s.len() && self.s[self.pos] != b'\n' {
                    self.pos += 1;
                }
                continue;
            }
            // POD: a line starting with =word, up to =cut.
            let line_start = self.pos == 0 || self.s[self.pos - 1] == b'\n';
            if line_start && self.at(0) == b'=' && ident_start(self.at(1)) {
                loop {
                    match self.rest().iter().position(|&b| b == b'\n') {
                        None => {
                            self.pos = self.s.len();
                            break;
                        }
                        Some(n) => {
                            self.pos += n + 1;
                            if self.rest().starts_with(b"=cut") {
                                while self.pos < self.s.len() && self.s[self.pos] != b'\n' {
                                    self.pos += 1;
                                }
                                break;
                            }
                        }
                    }
                }
                continue;
            }
            if line_start && (self.rest().starts_with(b"__END__") || self.rest().starts_with(b"__DATA__")) {
                self.pos = self.s.len();
            }
            break;
        }
    }

    fn starts(&mut self, t: &str) -> bool {
        self.ws();
        self.rest().starts_with(t.as_bytes())
    }

    fn eat(&mut self, t: &str) -> bool {
        if self.starts(t) {
            self.pos += t.len();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, t: &str) -> R<()> {
        if self.eat(t) {
            Ok(())
        } else {
            Err(self.err(&format!("expected '{t}'")))
        }
    }

    /// The identifier at the current position (after whitespace), if any,
    /// with `::` package separators.
    fn peek_ident(&mut self) -> Option<&'a str> {
        self.ws();
        let s = self.rest();
        let mut n = 0;
        if s.starts_with(b"::") {
            n = 2;
        } else if !s.first().is_some_and(|&c| ident_start(c)) {
            return None;
        }
        loop {
            while n < s.len() && ident_char(s[n]) {
                n += 1;
            }
            if s[n..].starts_with(b"::") {
                n += 2;
                continue;
            }
            break;
        }
        std::str::from_utf8(&s[..n]).ok()
    }

    /// Consume `word` if the next identifier is exactly it.
    fn eat_word(&mut self, word: &str) -> bool {
        if self.peek_ident() == Some(word) {
            self.pos += word.len();
            true
        } else {
            false
        }
    }

    // ---- statements ----

    fn statements(&mut self) -> R<Block> {
        let mut out = Vec::new();
        let mut last = 0;
        loop {
            self.ws();
            if self.pos >= self.s.len() || self.at(0) == b'}' {
                return Ok(out);
            }
            let line = self.line_here();
            if let Some(st) = self.statement()? {
                if line != last && !matches!(st, Stmt::SubDef(_)) {
                    out.push(Stmt::Line(line));
                    last = line;
                }
                out.push(st);
            }
        }
    }

    /// The line of the current position (positions only move forward
    /// between calls, except after backtracking: then counted again).
    fn line_here(&mut self) -> u32 {
        if self.pos < self.line_pos {
            self.line_pos = 0;
            self.line = 1;
        }
        let end = self.pos.min(self.s.len());
        self.line += self.s[self.line_pos..end].iter().filter(|&&b| b == b'\n').count() as u32;
        self.line_pos = end;
        self.line
    }

    fn block(&mut self) -> R<Block> {
        self.expect("{")?;
        let b = self.statements()?;
        self.expect("}")?;
        Ok(b)
    }

    /// perl's guess (toke.c) for a `{` that starts a statement: an
    /// anonymous hash if it is empty, or if its first term (a quoted string,
    /// a q// string or a word) is followed by `=>`, or by a comma when that
    /// term does not start with a lower-case letter; a block otherwise.
    fn brace_is_anon_hash(&self) -> bool {
        let s = self.s;
        let mut i = self.pos + 1;
        while i < s.len() && s[i].is_ascii_whitespace() {
            i += 1;
        }
        if s.get(i) == Some(&b'}') {
            return true;
        }
        let first = s.get(i).copied().unwrap_or(0);
        let mut t = i;
        if matches!(first, b'\'' | b'"' | b'`') {
            t += 1;
            while t < s.len() && s[t] != first {
                if s[t] == b'\\' {
                    t += 1;
                }
                t += 1;
            }
            t += 1;
        } else if first == b'q' && matches!(s.get(i + 1), Some(b'q') | Some(b'w') | Some(b'x') | Some(b'r')) && s.get(i + 2).is_some_and(|c| !c.is_ascii_alphanumeric() && *c != b'_') {
            // q-like quote: skip to its closing delimiter
            let open = s[i + 2];
            let close = match open { b'(' => b')', b'[' => b']', b'{' => b'}', b'<' => b'>', c => c };
            t = i + 3;
            while t < s.len() && s[t] != close {
                if s[t] == b'\\' {
                    t += 1;
                }
                t += 1;
            }
            t += 1;
        } else if first == b'q' && s.get(i + 1).is_some_and(|c| !c.is_ascii_alphanumeric() && *c != b'_' && !c.is_ascii_whitespace()) {
            let open = s[i + 1];
            let close = match open { b'(' => b')', b'[' => b']', b'{' => b'}', b'<' => b'>', c => c };
            t = i + 2;
            while t < s.len() && s[t] != close {
                if s[t] == b'\\' {
                    t += 1;
                }
                t += 1;
            }
            t += 1;
        } else {
            while t < s.len() && (s[t].is_ascii_alphanumeric() || s[t] == b'_') {
                t += 1;
            }
        }
        while t < s.len() && s[t].is_ascii_whitespace() {
            t += 1;
        }
        (s.get(t) == Some(&b',') && (first == b'q' || !first.is_ascii_lowercase())) || (s.get(t) == Some(&b'=') && s.get(t + 1) == Some(&b'>'))
    }

    fn statement(&mut self) -> R<Option<Stmt>> {
        self.ws();
        if self.eat(";") {
            return Ok(None);
        }
        if self.at(0) == b'{' && !self.brace_is_anon_hash() {
            return Ok(Some(Stmt::Block(None, self.block()?)));
        }
        // LABEL: (any identifier but a keyword)
        let mut label = None;
        if let Some(id) = self.peek_ident() {
            let mut after = self.pos + id.len();
            while self.s.get(after) == Some(&b' ') {
                after += 1;
            }
            if !id.contains("::")
                && !is_keyword(id)
                && self.s.get(after) == Some(&b':')
                && self.s.get(after + 1) != Some(&b':')
            {
                self.pos = after + 1;
                label = Some(Rc::from(id));
                self.ws();
                // A label on a statement that is not a loop or a block.
                let next = self.peek_ident();
                if !matches!(next, Some("for" | "foreach" | "while" | "until")) && !self.starts("{") {
                    return Ok(Some(Stmt::Label(label.unwrap())));
                }
            }
        }
        let word = self.peek_ident();
        match word {
            Some("if") | Some("unless") => {
                let unless = word == Some("unless");
                self.pos += word.unwrap().len();
                self.expect("(")?;
                let mut cond = self.expr()?;
                self.expect(")")?;
                if unless {
                    cond = Expr::Un(UnOp::Not, Box::new(cond));
                }
                let mut arms = vec![(cond, self.block()?)];
                let mut otherwise = None;
                loop {
                    if self.eat_word("elsif") {
                        self.expect("(")?;
                        let c = self.expr()?;
                        self.expect(")")?;
                        arms.push((c, self.block()?));
                    } else if self.eat_word("else") {
                        otherwise = Some(self.block()?);
                        break;
                    } else {
                        break;
                    }
                }
                return Ok(Some(Stmt::If(arms, otherwise)));
            }
            Some("while") | Some("until") => {
                let until = word == Some("until");
                self.pos += word.unwrap().len();
                self.expect("(")?;
                let cond = if self.starts(")") { None } else { Some(self.expr()?) };
                self.expect(")")?;
                // while (<FH>) is while (defined($_ = <FH>))
                // ... and while (my $x = <FH>) is while (defined(my $x = <FH>))
                let cond = cond.map(|c| match c {
                    Expr::ReadLine(_) => Expr::Call { name: Rc::from("defined"), args: vec![Expr::Assign(None, Box::new(Expr::Scalar(Rc::from("_"))), Box::new(c))], amp: false },
                    Expr::Assign(None, l, r) if matches!(*r, Expr::ReadLine(_)) => Expr::Call { name: Rc::from("defined"), args: vec![Expr::Assign(None, l, r)], amp: false },
                    c => c,
                });
                let body = self.block()?;
                return Ok(Some(Stmt::While { label, cond, body, until, post: false }));
            }
            Some("for") | Some("foreach") => {
                self.pos += word.unwrap().len();
                return self.for_statement(label).map(Some);
            }
            Some("sub") => {
                let save = self.pos;
                self.pos += 3;
                if let Some(name) = self.peek_ident() {
                    self.pos += name.len();
                    self.ws();
                    let pstart = self.pos;
                    self.skip_prototype();
                    if self.pos > pstart {
                        let proto: String = String::from_utf8_lossy(&self.s[pstart + 1..self.pos - 1]).chars().filter(|c| !c.is_whitespace()).collect();
                        let full = if name.contains("::") { name.to_string() } else { format!("{}::{}", self.package, name) };
                        PROTOS.with(|m| m.borrow_mut().insert(full, proto));
                    }
                    if self.eat(";") {
                        return Ok(None);
                    }
                    let body = self.block()?;
                    let full: Rc<str> = if name.contains("::") { Rc::from(name) } else { Rc::from(format!("{}::{}", self.package, name)) };
                    return Ok(Some(Stmt::SubDef(Rc::new(Sub { name: Some(full), package: self.package.clone(), body: Rc::new(body) }))));
                }
                self.pos = save;
            }
            Some("package") => {
                self.pos += 7;
                let name = self.peek_ident().ok_or_else(|| self.err("package name expected"))?;
                self.pos += name.len();
                let name: Rc<str> = Rc::from(name);
                if self.starts("{") {
                    let old = std::mem::replace(&mut self.package, name.clone());
                    let b = self.block();
                    self.package = old;
                    return Ok(Some(Stmt::Package(name, Some(b?))));
                }
                self.expect(";")?;
                self.package = name.clone();
                return Ok(Some(Stmt::Package(name, None)));
            }
            Some("use") | Some("no") => {
                self.pos += word.unwrap().len();
                self.ws();
                if self.at(0).is_ascii_digit() || self.at(0) == b'v' && self.at(1).is_ascii_digit() {
                    // use 5.008;
                    while self.pos < self.s.len() && self.s[self.pos] != b';' {
                        self.pos += 1;
                    }
                    self.eat(";");
                    return Ok(None);
                }
                let name = self.peek_ident().ok_or_else(|| self.err("module name expected"))?;
                self.pos += name.len();
                // perl loads a used module while it compiles: the prototypes
                // it declares apply to the code that follows
                if word == Some("use") {
                    module_loaded(name);
                }
                let args = if self.starts(";") { None } else { Some(self.expr()?) };
                self.eat(";");
                let name = if word == Some("no") { format!("no {name}") } else { name.to_string() };
                return Ok(Some(Stmt::Use(Rc::from(name), args)));
            }
            Some("BEGIN") | Some("END") | Some("INIT") => {
                let save = self.pos;
                self.pos += word.unwrap().len();
                if self.starts("{") {
                    let b = self.block()?;
                    if word == Some("END") {
                        return Ok(Some(Stmt::End(Rc::new(b))));
                    }
                    return Ok(Some(Stmt::Block(None, b)));
                }
                self.pos = save;
            }
            Some("do") => {
                let save = self.pos;
                self.pos += 2;
                if self.starts("{") {
                    let body = self.block()?;
                    if self.eat_word("while") || {
                        false
                    } {
                        let cond = self.expr()?;
                        self.end_statement()?;
                        return Ok(Some(Stmt::While { label, cond: Some(cond), body, until: false, post: true }));
                    }
                    if self.eat_word("until") {
                        let cond = self.expr()?;
                        self.end_statement()?;
                        return Ok(Some(Stmt::While { label, cond: Some(cond), body, until: true, post: true }));
                    }
                }
                self.pos = save;
            }
            _ => {}
        }
        if label.is_some() && self.starts("{") {
            return Ok(Some(Stmt::Block(label, self.block()?)));
        }
        let e = self.expr()?;
        let st = self.modifiers(Stmt::Expr(e))?;
        self.end_statement()?;
        Ok(Some(st))
    }

    fn end_statement(&mut self) -> R<()> {
        self.ws();
        if self.eat(";") || self.pos >= self.s.len() || self.at(0) == b'}' {
            Ok(())
        } else {
            Err(self.unexpected())
        }
    }

    /// The error for a token where an operator was expected. perl's lexer
    /// reads the token first, so an unterminated string or pattern reports
    /// that rather than a syntax error.
    fn unexpected(&mut self) -> ParseError {
        let save = self.pos;
        let c = self.at(0);
        let e = if c == b'"' || c == b'\'' {
            self.pos += 1;
            self.delimited(c).err()
        } else if let Some(w @ ("m" | "s" | "tr" | "y" | "qr" | "q" | "qq")) = self.peek_ident() {
            self.pos += w.len();
            self.quote_like(w).err()
        } else {
            None
        };
        self.pos = save;
        match e {
            Some(e) if e.fatal => e,
            _ => self.err("syntax error"),
        }
    }

    fn modifiers(&mut self, st: Stmt) -> R<Stmt> {
        let Stmt::Expr(e) = st else { return Ok(st) };
        let word = self.peek_ident();
        Ok(match word {
            Some("if") => {
                self.pos += 2;
                Stmt::Modified(self.expr()?, e)
            }
            Some("unless") => {
                self.pos += 6;
                Stmt::Modified(Expr::Un(UnOp::Not, Box::new(self.expr()?)), e)
            }
            Some("while") | Some("until") => {
                self.pos += 5;
                let cond = self.expr()?;
                Stmt::While { label: None, cond: Some(cond), body: vec![Stmt::Expr(e)], until: word == Some("until"), post: false }
            }
            Some("for") | Some("foreach") => {
                self.pos += word.unwrap().len();
                let list = self.expr()?;
                Stmt::Foreach { label: None, var: None, list, body: vec![Stmt::Expr(e)] }
            }
            _ => Stmt::Expr(e),
        })
    }

    fn for_statement(&mut self, label: Option<Rc<str>>) -> R<Stmt> {
        let mut var = None;
        if self.eat_word("my") || self.eat_word("our") || self.eat_word("local") {
            self.expect("$")?;
            let name = self.peek_ident().ok_or_else(|| self.err("loop variable expected"))?;
            self.pos += name.len();
            var = Some((true, Rc::from(name)));
        } else if self.starts("$") {
            self.pos += 1;
            let name = self.peek_ident().ok_or_else(|| self.err("loop variable expected"))?;
            self.pos += name.len();
            var = Some((false, Rc::from(name)));
        }
        self.expect("(")?;
        if var.is_none() {
            // C-style when a top-level ';' follows the first expression.
            let init = if self.starts(";") { None } else { Some(self.expr()?) };
            if self.eat(";") {
                let cond = if self.starts(";") { None } else { Some(self.expr()?) };
                self.expect(";")?;
                let step = if self.starts(")") { None } else { Some(self.expr()?) };
                self.expect(")")?;
                let body = self.block()?;
                return Ok(Stmt::ForC { label, init, cond, step, body });
            }
            self.expect(")")?;
            let body = self.block()?;
            return Ok(Stmt::Foreach { label, var: None, list: init.unwrap_or(Expr::List(vec![])), body });
        }
        let list = if self.starts(")") { Expr::List(vec![]) } else { self.expr()? };
        self.expect(")")?;
        let body = self.block()?;
        Ok(Stmt::Foreach { label, var, list, body })
    }

    /// The prototype of the sub a call names (as perl resolves the name in
    /// the current package).
    fn sub_prototype(&self, id: &str) -> Option<String> {
        let full = match id.strip_prefix("::") {
            Some(n) => format!("main::{n}"),
            None if id.contains("::") => id.to_string(),
            None => format!("{}::{id}", self.package),
        };
        prototype_of(&full)
    }

    fn skip_prototype(&mut self) {
        if self.starts("(") {
            while self.pos < self.s.len() && self.s[self.pos] != b')' {
                self.pos += 1;
            }
            self.pos += 1;
        }
    }

    // ---- expressions, lowest precedence first ----

    pub fn expr(&mut self) -> R<Expr> {
        let mut e = self.low_and()?;
        loop {
            if self.eat_word("or") {
                e = Expr::Bin(BinOp::Or, Box::new(e), Box::new(self.low_and()?));
            } else if self.eat_word("xor") {
                e = Expr::Bin(BinOp::Xor, Box::new(e), Box::new(self.low_and()?));
            } else {
                return Ok(e);
            }
        }
    }

    fn low_and(&mut self) -> R<Expr> {
        let mut e = self.low_not()?;
        while self.eat_word("and") {
            e = Expr::Bin(BinOp::And, Box::new(e), Box::new(self.low_not()?));
        }
        Ok(e)
    }

    fn low_not(&mut self) -> R<Expr> {
        if self.eat_word("not") {
            if self.starts(")") || self.starts(";") {
                return Ok(Expr::Un(UnOp::Not, Box::new(Expr::Const(Value::Undef))));
            }
            return Ok(Expr::Un(UnOp::Not, Box::new(self.low_not()?)));
        }
        self.comma()
    }

    /// A terminator: nothing more of an expression can follow.
    fn at_terminator(&mut self) -> bool {
        self.ws();
        if self.pos >= self.s.len() {
            return true;
        }
        if matches!(self.at(0), b')' | b']' | b'}' | b';' | b':') {
            return true;
        }
        matches!(self.peek_ident(), Some("or" | "and" | "if" | "unless" | "while" | "until" | "for" | "foreach" | "xor"))
    }

    fn comma(&mut self) -> R<Expr> {
        let first = self.assign()?;
        if !(self.starts(",") || self.starts("=>")) {
            return Ok(first);
        }
        let mut items = vec![first];
        while self.eat(",") || self.eat("=>") {
            if self.at_terminator() {
                break;
            }
            if self.starts(",") || self.starts("=>") {
                continue;
            }
            items.push(self.assign()?);
        }
        Ok(Expr::List(items))
    }

    fn assign(&mut self) -> R<Expr> {
        let lhs = self.ternary()?;
        self.ws();
        const OPS: &[(&str, Option<BinOp>)] = &[
            ("**=", Some(BinOp::Pow)),
            ("||=", Some(BinOp::Or)),
            ("&&=", Some(BinOp::And)),
            ("//=", Some(BinOp::DefOr)),
            ("<<=", Some(BinOp::Shl)),
            (">>=", Some(BinOp::Shr)),
            ("+=", Some(BinOp::Add)),
            ("-=", Some(BinOp::Sub)),
            ("*=", Some(BinOp::Mul)),
            ("/=", Some(BinOp::Div)),
            (".=", Some(BinOp::Concat)),
            ("%=", Some(BinOp::Mod)),
            ("x=", Some(BinOp::Repeat)),
            ("&=", Some(BinOp::BitAnd)),
            ("|=", Some(BinOp::BitOr)),
            ("^=", Some(BinOp::BitXor)),
        ];
        for (t, op) in OPS {
            if self.rest().starts_with(t.as_bytes()) {
                self.pos += t.len();
                let rhs = self.assign()?;
                return Ok(Expr::Assign(*op, Box::new(lhs), Box::new(rhs)));
            }
        }
        if self.at(0) == b'=' && !matches!(self.at(1), b'=' | b'~' | b'>') {
            self.pos += 1;
            let rhs = self.assign()?;
            return Ok(Expr::Assign(None, Box::new(lhs), Box::new(rhs)));
        }
        Ok(lhs)
    }

    fn ternary(&mut self) -> R<Expr> {
        let c = self.range()?;
        if self.starts("?") {
            self.pos += 1;
            let a = self.assign()?;
            self.expect(":")?;
            let b = self.assign_or_ternary()?;
            return Ok(Expr::Cond(Box::new(c), Box::new(a), Box::new(b)));
        }
        Ok(c)
    }

    /// The else branch of `?:` (right-associative, and may be an assignment
    /// target only through parentheses).
    fn assign_or_ternary(&mut self) -> R<Expr> {
        self.ternary()
    }

    fn range(&mut self) -> R<Expr> {
        let a = self.oror()?;
        self.ws();
        if self.rest().starts_with(b"...") {
            self.pos += 3;
            return Ok(Expr::Range(Box::new(a), Box::new(self.oror()?)));
        }
        if self.rest().starts_with(b"..") {
            self.pos += 2;
            return Ok(Expr::Range(Box::new(a), Box::new(self.oror()?)));
        }
        Ok(a)
    }

    fn oror(&mut self) -> R<Expr> {
        let mut e = self.andand()?;
        loop {
            self.ws();
            let r = self.rest();
            if r.starts_with(b"||") && !r.starts_with(b"||=") {
                self.pos += 2;
                e = Expr::Bin(BinOp::Or, Box::new(e), Box::new(self.andand()?));
            } else if r.starts_with(b"//") && !r.starts_with(b"//=") {
                self.pos += 2;
                e = Expr::Bin(BinOp::DefOr, Box::new(e), Box::new(self.andand()?));
            } else {
                return Ok(e);
            }
        }
    }

    fn andand(&mut self) -> R<Expr> {
        let mut e = self.bitor()?;
        loop {
            self.ws();
            let r = self.rest();
            if r.starts_with(b"&&") && !r.starts_with(b"&&=") {
                self.pos += 2;
                e = Expr::Bin(BinOp::And, Box::new(e), Box::new(self.bitor()?));
            } else {
                return Ok(e);
            }
        }
    }

    fn bitor(&mut self) -> R<Expr> {
        let mut e = self.bitand()?;
        loop {
            self.ws();
            let r = self.rest();
            if r.starts_with(b"|") && !r.starts_with(b"||") && !r.starts_with(b"|=") {
                self.pos += 1;
                e = Expr::Bin(BinOp::BitOr, Box::new(e), Box::new(self.bitand()?));
            } else if r.starts_with(b"^") && !r.starts_with(b"^=") {
                self.pos += 1;
                e = Expr::Bin(BinOp::BitXor, Box::new(e), Box::new(self.bitand()?));
            } else {
                return Ok(e);
            }
        }
    }

    fn bitand(&mut self) -> R<Expr> {
        let mut e = self.equality()?;
        loop {
            self.ws();
            let r = self.rest();
            if r.starts_with(b"&") && !r.starts_with(b"&&") && !r.starts_with(b"&=") {
                self.pos += 1;
                e = Expr::Bin(BinOp::BitAnd, Box::new(e), Box::new(self.equality()?));
            } else {
                return Ok(e);
            }
        }
    }

    /// A word operator (`eq`, `lt`, ...) at the current position.
    fn word_op(&mut self, words: &[(&str, BinOp)]) -> Option<BinOp> {
        let id = self.peek_ident()?;
        for (w, op) in words {
            if id == *w {
                self.pos += w.len();
                return Some(*op);
            }
        }
        None
    }

    fn equality(&mut self) -> R<Expr> {
        let mut e = self.relational()?;
        loop {
            self.ws();
            let r = self.rest();
            let op = if r.starts_with(b"<=>") {
                self.pos += 3;
                Some(BinOp::NumCmp)
            } else if r.starts_with(b"==") {
                self.pos += 2;
                Some(BinOp::NumEq)
            } else if r.starts_with(b"!=") {
                self.pos += 2;
                Some(BinOp::NumNe)
            } else {
                self.word_op(&[("eq", BinOp::StrEq), ("ne", BinOp::StrNe), ("cmp", BinOp::StrCmp)])
            };
            match op {
                Some(op) => e = Expr::Bin(op, Box::new(e), Box::new(self.relational()?)),
                None => return Ok(e),
            }
        }
    }

    fn relational(&mut self) -> R<Expr> {
        let mut e = self.uni()?;
        loop {
            self.ws();
            let r = self.rest();
            let op = if r.starts_with(b"<=") && !r.starts_with(b"<=>") {
                self.pos += 2;
                Some(BinOp::NumLe)
            } else if r.starts_with(b">=") {
                self.pos += 2;
                Some(BinOp::NumGe)
            } else if r.starts_with(b"<") && !r.starts_with(b"<<") && !r.starts_with(b"<=>") {
                self.pos += 1;
                Some(BinOp::NumLt)
            } else if r.starts_with(b">") && !r.starts_with(b">>") {
                self.pos += 1;
                Some(BinOp::NumGt)
            } else {
                self.word_op(&[("lt", BinOp::StrLt), ("gt", BinOp::StrGt), ("le", BinOp::StrLe), ("ge", BinOp::StrGe)])
            };
            match op {
                Some(op) => e = Expr::Bin(op, Box::new(e), Box::new(self.uni()?)),
                None => return Ok(e),
            }
        }
    }

    /// The level named unary operators parse their argument at.
    fn uni(&mut self) -> R<Expr> {
        self.shift()
    }

    fn shift(&mut self) -> R<Expr> {
        let mut e = self.additive()?;
        loop {
            self.ws();
            let r = self.rest();
            if r.starts_with(b"<<") && !r.starts_with(b"<<=") {
                self.pos += 2;
                e = Expr::Bin(BinOp::Shl, Box::new(e), Box::new(self.additive()?));
            } else if r.starts_with(b">>") && !r.starts_with(b">>=") {
                self.pos += 2;
                e = Expr::Bin(BinOp::Shr, Box::new(e), Box::new(self.additive()?));
            } else {
                return Ok(e);
            }
        }
    }

    fn additive(&mut self) -> R<Expr> {
        let mut e = self.mult()?;
        loop {
            self.ws();
            let r = self.rest();
            let op = match r {
                [b'+', c, ..] if *c != b'=' && *c != b'+' => BinOp::Add,
                [b'+'] => BinOp::Add,
                [b'-', c, ..] if *c != b'=' && *c != b'-' && *c != b'>' => BinOp::Sub,
                [b'.', c, ..] if *c != b'=' && *c != b'.' => BinOp::Concat,
                _ => return Ok(e),
            };
            self.pos += 1;
            e = Expr::Bin(op, Box::new(e), Box::new(self.mult()?));
        }
    }

    fn mult(&mut self) -> R<Expr> {
        let mut e = self.bind()?;
        loop {
            self.ws();
            let r = self.rest();
            let op = match r {
                [b'*', c, ..] if *c != b'*' && *c != b'=' => BinOp::Mul,
                [b'/', c, ..] if *c != b'/' && *c != b'=' => BinOp::Div,
                [b'%', c, ..] if *c != b'=' => BinOp::Mod,
                [b'x', c, ..] if !ident_char(*c) || c.is_ascii_digit() => {
                    if *c == b'=' && r.get(2) != Some(&b'=') && r.get(2) != Some(&b'~') {
                        return Ok(e); // x= is an assignment
                    }
                    if *c == b'=' && r.get(2) == Some(&b'>') {
                        return Ok(e);
                    }
                    BinOp::Repeat
                }
                _ => return Ok(e),
            };
            self.pos += 1;
            e = Expr::Bin(op, Box::new(e), Box::new(self.bind()?));
        }
    }

    fn bind(&mut self) -> R<Expr> {
        let e = self.unary()?;
        self.ws();
        let r = self.rest();
        let neg = if r.starts_with(b"=~") {
            false
        } else if r.starts_with(b"!~") {
            true
        } else {
            return Ok(e);
        };
        self.pos += 2;
        let rhs = self.unary()?;
        let target = Some(Box::new(e));
        Ok(match rhs {
            Expr::Match { target: None, neg: _, pat } => Expr::Match { target, neg, pat },
            Expr::Subst { target: None, pat, repl } => {
                let e = Expr::Subst { target, pat, repl };
                if neg {
                    Expr::Un(UnOp::Not, Box::new(e))
                } else {
                    e
                }
            }
            Expr::Trans { target: None, search, repl, flags } => {
                let e = Expr::Trans { target, search, repl, flags };
                if neg {
                    Expr::Un(UnOp::Not, Box::new(e))
                } else {
                    e
                }
            }
            // `$x =~ $re`: the expression is the pattern.
            other => Expr::Match { target, neg, pat: Pat { parts: vec![Part::Scalar(other)], flags: Rc::from("") } },
        })
    }

    fn unary(&mut self) -> R<Expr> {
        self.ws();
        let r = self.rest();
        match r {
            [b'!', c, ..] if *c != b'~' && *c != b'=' => {
                self.pos += 1;
                Ok(Expr::Un(UnOp::Not, Box::new(self.unary()?)))
            }
            [b'!'] => Err(self.err("operand expected")),
            [b'~', ..] => {
                self.pos += 1;
                Ok(Expr::Un(UnOp::BitNot, Box::new(self.unary()?)))
            }
            [b'\\', ..] => {
                self.pos += 1;
                Ok(Expr::Un(UnOp::Ref, Box::new(self.unary()?)))
            }
            [b'+', b'+', ..] => {
                self.pos += 2;
                Ok(Expr::Un(UnOp::PreInc, Box::new(self.unary()?)))
            }
            [b'-', b'-', ..] => {
                self.pos += 2;
                Ok(Expr::Un(UnOp::PreDec, Box::new(self.unary()?)))
            }
            [b'-', c, d, ..] if c.is_ascii_alphabetic() && !ident_char(*d) && b"edfrwxszlpSbcugktTBoRWXOMAC".contains(c) && *d != b'>' && *d != b'=' && *d != b',' && !self.fat_comma_after(2) => {
                // A file test: -e $file
                let op = *c;
                self.pos += 2;
                let arg = self.unary()?;
                Ok(Expr::FileTest(op, Box::new(arg)))
            }
            [b'-', c, ..] if ident_start(*c) => {
                // -bareword is the string "-bareword"; -func(...) is a negation.
                let save = self.pos;
                self.pos += 1;
                let id = self.peek_ident().unwrap();
                let after = self.pos + id.len();
                let next = {
                    let mut k = after;
                    while k < self.s.len() && self.s[k] == b' ' {
                        k += 1;
                    }
                    self.s.get(k).copied().unwrap_or(0)
                };
                if next != b'(' && !id.contains("::") && self.is_plain_bareword(id) {
                    self.pos = after;
                    return Ok(Expr::Const(Value::str(&format!("-{id}"))));
                }
                self.pos = save + 1;
                Ok(Expr::Un(UnOp::Neg, Box::new(self.pow()?)))
            }
            [b'-', ..] => {
                self.pos += 1;
                Ok(Expr::Un(UnOp::Neg, Box::new(self.pow_unary()?)))
            }
            [b'+', c, ..] if *c != b'=' => {
                self.pos += 1;
                Ok(Expr::Un(UnOp::Plus, Box::new(self.unary()?)))
            }
            _ => self.pow(),
        }
    }

    /// The operand of unary minus: `-2**2` is `-(2**2)`, `--$x` handled above.
    fn pow_unary(&mut self) -> R<Expr> {
        self.ws();
        match self.rest() {
            [b'!' | b'~' | b'\\' | b'-' | b'+', ..] => self.unary(),
            _ => self.pow(),
        }
    }

    fn is_plain_bareword(&self, id: &str) -> bool {
        !NAMED_UNARY.contains(&id) && !LIST_OPS.contains(&id) && !matches!(id, "e" | "d" | "f")
    }

    fn pow(&mut self) -> R<Expr> {
        let base = self.postfix()?;
        self.ws();
        if self.rest().starts_with(b"**") && !self.rest().starts_with(b"**=") {
            self.pos += 2;
            let exp = self.unary()?;
            return Ok(Expr::Bin(BinOp::Pow, Box::new(base), Box::new(exp)));
        }
        Ok(base)
    }

    fn postfix(&mut self) -> R<Expr> {
        let mut e = self.term()?;
        loop {
            let before_ws = self.pos;
            self.ws();
            let r = self.rest();
            if r.starts_with(b"->") {
                self.pos += 2;
                self.ws();
                match self.at(0) {
                    b'[' => {
                        self.pos += 1;
                        let i = self.expr()?;
                        self.expect("]")?;
                        e = Expr::Elem(Container::Deref(Box::new(e)), Box::new(i));
                    }
                    b'{' => {
                        let k = self.hash_key()?;
                        e = Expr::HElem(Container::Deref(Box::new(e)), Box::new(k));
                    }
                    b'(' => {
                        let args = self.paren_args()?;
                        e = Expr::CallRef(Box::new(e), args);
                    }
                    b'@' if self.at(1) == b'*' => {
                        self.pos += 2;
                        e = Expr::Array(Container::Deref(Box::new(e)));
                    }
                    b'%' if self.at(1) == b'*' => {
                        self.pos += 2;
                        e = Expr::Hash(Container::Deref(Box::new(e)));
                    }
                    b'$' if self.at(1) == b'*' => {
                        self.pos += 2;
                        e = Expr::ScalarDeref(Box::new(e));
                    }
                    b'$' => {
                        self.pos += 1;
                        let id = self.peek_ident().ok_or_else(|| self.err("method name expected"))?;
                        self.pos += id.len();
                        let args = if self.starts("(") { self.paren_args()? } else { vec![] };
                        e = Expr::Method { inv: Box::new(e), name: MethodName::Dynamic(Box::new(Expr::Scalar(Rc::from(id)))), args };
                    }
                    _ => {
                        let id = self.peek_ident().ok_or_else(|| self.err("method name expected"))?;
                        self.pos += id.len();
                        let args = if self.starts("(") { self.paren_args()? } else { vec![] };
                        e = Expr::Method { inv: Box::new(e), name: MethodName::Static(Rc::from(id)), args };
                    }
                }
                continue;
            }
            // Implicit arrow between subscripts: $x{a}{b}, $x[0][1].
            let adjacent = self.pos == before_ws;
            let subscripted = matches!(e, Expr::Elem(..) | Expr::HElem(..) | Expr::CallRef(..));
            if adjacent && subscripted {
                if r.starts_with(b"[") {
                    self.pos += 1;
                    let i = self.expr()?;
                    self.expect("]")?;
                    e = Expr::Elem(Container::Deref(Box::new(e)), Box::new(i));
                    continue;
                }
                if r.starts_with(b"{") {
                    let k = self.hash_key()?;
                    e = Expr::HElem(Container::Deref(Box::new(e)), Box::new(k));
                    continue;
                }
                if r.starts_with(b"(") && matches!(e, Expr::CallRef(..)) {
                    let args = self.paren_args()?;
                    e = Expr::CallRef(Box::new(e), args);
                    continue;
                }
            }
            if r.starts_with(b"++") {
                self.pos += 2;
                e = Expr::Un(UnOp::PostInc, Box::new(e));
                continue;
            }
            if r.starts_with(b"--") {
                self.pos += 2;
                e = Expr::Un(UnOp::PostDec, Box::new(e));
                continue;
            }
            self.pos = before_ws;
            return Ok(e);
        }
    }

    /// `{KEY}` of a hash subscript: a bareword (or -bareword) is a string.
    fn hash_key(&mut self) -> R<Expr> {
        self.expect("{")?;
        self.ws();
        let s = self.rest();
        let mut n = 0;
        if s.first() == Some(&b'-') {
            n = 1;
        }
        if s.get(n).is_some_and(|&c| ident_start(c)) {
            let mut m = n;
            while m < s.len() && ident_char(s[m]) {
                m += 1;
            }
            let mut k = m;
            while k < s.len() && s[k].is_ascii_whitespace() {
                k += 1;
            }
            if s.get(k) == Some(&b'}') {
                let key = Value::bytes(&s[..m]);
                self.pos += k + 1;
                return Ok(Expr::Const(key));
            }
        }
        let k = self.expr()?;
        self.expect("}")?;
        Ok(k)
    }

    fn paren_args(&mut self) -> R<Vec<Expr>> {
        self.expect("(")?;
        if self.eat(")") {
            return Ok(vec![]);
        }
        let e = self.expr()?;
        self.expect(")")?;
        Ok(match e {
            Expr::List(v) => v,
            e => vec![e],
        })
    }

    // ---- terms ----

    fn term(&mut self) -> R<Expr> {
        self.ws();
        let c = self.at(0);
        match c {
            b'$' => self.scalar_term(),
            b'@' => self.array_term(),
            b'%' => self.hash_term(),
            b'&' => self.amp_call(),
            b'(' => {
                self.pos += 1;
                let inner = if self.starts(")") { Expr::List(vec![]) } else { self.expr()? };
                self.expect(")")?;
                let list = match inner {
                    Expr::List(v) => Expr::List(v),
                    e => Expr::List(vec![e]),
                };
                if self.starts("[") {
                    self.pos += 1;
                    let idx = self.expr()?;
                    self.expect("]")?;
                    return Ok(Expr::ListSlice(Box::new(list), Box::new(idx)));
                }
                Ok(list)
            }
            b'[' => {
                self.pos += 1;
                let inner = if self.starts("]") { Expr::List(vec![]) } else { self.expr()? };
                self.expect("]")?;
                Ok(Expr::AnonArray(flatten_list(inner)))
            }
            b'{' => {
                self.pos += 1;
                let inner = if self.starts("}") { Expr::List(vec![]) } else { self.expr()? };
                self.expect("}")?;
                Ok(Expr::AnonHash(flatten_list(inner)))
            }
            b'\'' => {
                self.pos += 1;
                let raw = self.delimited(b'\'')?;
                Ok(Expr::Const(Value::bytes(single_quoted(&raw, b'\''))))
            }
            b'"' => {
                self.pos += 1;
                let raw = self.delimited(b'"')?;
                self.interp_expr(&raw)
            }
            b'`' => {
                self.pos += 1;
                let raw = self.delimited(b'`')?;
                let cmd = self.interp_expr(&raw)?;
                Ok(Expr::Call { name: Rc::from("readpipe"), args: vec![cmd], amp: false })
            }
            b'/' => {
                self.pos += 1;
                let raw = self.delimited_or(b'/', "Search pattern not terminated")?;
                let flags = self.regex_flags();
                Ok(Expr::Match { target: None, neg: false, pat: self.pattern(&raw, flags)? })
            }
            b'<' if self.rest().starts_with(b"<<") && (self.at(2) == b'"' || self.at(2) == b'\'' || self.at(2) == b'~' || ident_start(self.at(2))) => {
                self.heredoc()
            }
            b'<' => {
                // <FH>, <$fh>, <>
                let r = self.rest();
                let end = r.iter().position(|&b| b == b'>').ok_or_else(|| self.err("syntax error"))?;
                let inner = std::str::from_utf8(&r[1..end]).unwrap_or("").to_string();
                self.pos += end + 1;
                let h = if let Some(v) = inner.strip_prefix('$') {
                    Expr::Scalar(Rc::from(v))
                } else if inner.is_empty() {
                    Expr::Const(Value::str("*STDIN"))
                } else {
                    Expr::Const(Value::str(&format!("*{inner}")))
                };
                Ok(Expr::ReadLine(Box::new(h)))
            }
            b'*' if ident_start(self.at(1)) => {
                // A glob: *NAME (a filehandle here).
                self.pos += 1;
                let id = self.peek_ident().unwrap();
                self.pos += id.len();
                let name = if id.contains("::") || matches!(id, "STDIN" | "STDOUT" | "STDERR") { id.to_string() } else { format!("{}::{id}", self.package) };
                Ok(Expr::Const(Value::str(&format!("*{name}"))))
            }
            b'0'..=b'9' => self.number(),
            b'.' if self.at(1).is_ascii_digit() => self.number(),
            _ if ident_start(c) || (c == b':' && self.at(1) == b':') => self.word_term(),
            0 => Err(self.err("unexpected end of code")),
            _ => Err(self.err("syntax error")),
        }
    }

    fn interp_expr(&mut self, raw: &[u8]) -> R<Expr> {
        let parts = self.interpolate(raw, false)?;
        Ok(match parts.as_slice() {
            [] => Expr::Const(Value::str("")),
            [Part::Lit(b)] => Expr::Const(Value::bytes(b)),
            _ => Expr::Interp(parts),
        })
    }

    /// A heredoc: `<<"EOF"`, `<<'EOF'`, `<<EOF`, `<<~EOF`. The body is the
    /// lines after the current one, which are then skipped.
    fn heredoc(&mut self) -> R<Expr> {
        self.pos += 2;
        let indent = if self.at(0) == b'~' {
            self.pos += 1;
            true
        } else {
            false
        };
        let (tag, interpolate) = match self.at(0) {
            q @ (b'"' | b'\'') => {
                self.pos += 1;
                let raw = self.delimited(q)?;
                (raw, q == b'"')
            }
            _ => {
                let id = self.peek_ident().ok_or_else(|| self.err("heredoc tag expected"))?;
                self.pos += id.len();
                (id.as_bytes().to_vec(), true)
            }
        };
        // The body starts at the next line.
        let line_end = self.rest().iter().position(|&b| b == b'\n').map_or(self.s.len(), |n| self.pos + n);
        let mut k = line_end + 1;
        let mut lines: Vec<&[u8]> = Vec::new();
        let mut end = None;
        while k <= self.s.len() {
            let e = self.s[k.min(self.s.len())..].iter().position(|&b| b == b'\n').map_or(self.s.len(), |n| k + n);
            let line = &self.s[k.min(self.s.len())..e];
            let trimmed: &[u8] = if indent { trim_start(line) } else { line };
            if trimmed == &tag[..] {
                end = Some(e);
                break;
            }
            lines.push(line);
            k = e + 1;
        }
        let end = end.ok_or_else(|| self.err("heredoc terminator not found"))?;
        let strip = if indent {
            let last = &self.s[k..end];
            last.len() - trim_start(last).len()
        } else {
            0
        };
        let mut body = Vec::new();
        for l in &lines {
            body.extend_from_slice(&l[strip.min(l.len() - trim_start(l).len())..]);
            body.push(b'\n');
        }
        // Splice the body out of the source: continue parsing the current
        // line, then jump past the terminator when the line ends.
        let rest_of_line = self.s[self.pos..line_end].to_vec();
        let mut new_src = self.s[..self.pos].to_vec();
        new_src.extend_from_slice(&rest_of_line);
        new_src.push(b'\n');
        let after = (end + 1).min(self.s.len());
        new_src.extend_from_slice(&self.s[after..]);
        // The parser borrows its source: leak the rebuilt one (heredocs are
        // only in config files, parsed once).
        let leaked: &'static [u8] = Box::leak(new_src.into_boxed_slice());
        // SAFETY of lifetimes: 'static outlives 'a.
        self.s = leaked;
        if interpolate {
            self.interp_expr(&body)
        } else {
            Ok(Expr::Const(Value::bytes(body)))
        }
    }

    fn number(&mut self) -> R<Expr> {
        let s = self.rest();
        if s.starts_with(b"0x") || s.starts_with(b"0X") || s.starts_with(b"0b") || s.starts_with(b"0B") {
            let radix = if s[1] | 0x20 == b'x' { 16 } else { 2 };
            let mut n = 2;
            let mut digits = String::new();
            while n < s.len() && (s[n].is_ascii_hexdigit() || s[n] == b'_') {
                if s[n] != b'_' {
                    digits.push(s[n] as char);
                }
                n += 1;
            }
            self.pos += n;
            let v = u64::from_str_radix(&digits, radix).map_err(|_| self.err("bad number"))?;
            return Ok(Expr::Const(crate::perl::ops::from_uv(v)));
        }
        let mut n = 0;
        let mut text = String::new();
        let mut float = false;
        while n < s.len() && (s[n].is_ascii_digit() || s[n] == b'_') {
            if s[n] != b'_' {
                text.push(s[n] as char);
            }
            n += 1;
        }
        if s.get(n) == Some(&b'.') && s.get(n + 1) != Some(&b'.') {
            float = true;
            text.push('.');
            n += 1;
            while n < s.len() && (s[n].is_ascii_digit() || s[n] == b'_') {
                if s[n] != b'_' {
                    text.push(s[n] as char);
                }
                n += 1;
            }
        }
        if matches!(s.get(n), Some(b'e' | b'E')) && (s.get(n + 1).is_some_and(|c| c.is_ascii_digit()) || matches!(s.get(n + 1), Some(b'+' | b'-')) && s.get(n + 2).is_some_and(|c| c.is_ascii_digit())) {
            float = true;
            text.push('e');
            n += 1;
            if matches!(s[n], b'+' | b'-') {
                text.push(s[n] as char);
                n += 1;
            }
            while n < s.len() && s[n].is_ascii_digit() {
                text.push(s[n] as char);
                n += 1;
            }
        }
        self.pos += n;
        if !float {
            // A leading 0 makes an octal literal.
            if text.len() > 1 && text.starts_with('0') {
                let v = u64::from_str_radix(&text[1..], 8).map_err(|_| self.err("Illegal octal digit"))?;
                return Ok(Expr::Const(crate::perl::ops::from_uv(v)));
            }
            if let Ok(v) = text.parse::<i64>() {
                return Ok(Expr::Const(Value::Int(v)));
            }
            if let Ok(v) = text.parse::<u64>() {
                return Ok(Expr::Const(Value::UInt(v)));
            }
        }
        let v: f64 = text.parse().map_err(|_| self.err("bad number"))?;
        Ok(Expr::Const(Value::Num(v)))
    }

    /// The text up to the closing delimiter (the opening one consumed),
    /// with backslashes kept. Bracketing delimiters nest.
    fn delimited(&mut self, open: u8) -> R<Vec<u8>> {
        let msg = match open {
            b'"' => "Can't find string terminator '\"' anywhere before EOF".to_string(),
            o => format!("Can't find string terminator \"{}\" anywhere before EOF", closing(o) as char),
        };
        self.delimited_or(open, &msg)
    }

    fn delimited_or(&mut self, open: u8, unterminated: &str) -> R<Vec<u8>> {
        let close = closing(open);
        let mut depth = 0;
        let mut out = Vec::new();
        while self.pos < self.s.len() {
            let c = self.s[self.pos];
            if c == b'\\' && self.pos + 1 < self.s.len() {
                out.push(c);
                out.push(self.s[self.pos + 1]);
                self.pos += 2;
                continue;
            }
            if c == close && depth == 0 {
                self.pos += 1;
                return Ok(out);
            }
            if open != close {
                if c == open {
                    depth += 1;
                } else if c == close {
                    depth -= 1;
                }
            }
            out.push(c);
            self.pos += 1;
        }
        Err(self.fatal(unterminated))
    }

    fn regex_flags(&mut self) -> Rc<str> {
        let mut n = 0;
        while self.at(n).is_ascii_alphabetic() {
            n += 1;
        }
        let f = String::from_utf8_lossy(&self.s[self.pos..self.pos + n]).into_owned();
        self.pos += n;
        Rc::from(f)
    }

    fn pattern(&mut self, raw: &[u8], flags: Rc<str>) -> R<Pat> {
        let parts = self.interpolate(raw, true)?;
        Ok(Pat { parts, flags })
    }

    /// Quote-like operators: q qq qw m s tr y qr.
    fn quote_like(&mut self, word: &str) -> R<Expr> {
        self.ws();
        let open = self.at(0);
        if open == 0 {
            return Err(self.fatal(match word {
                "m" | "qr" => "Search pattern not terminated",
                "s" => "Substitution pattern not terminated",
                "tr" | "y" => "Transliteration pattern not terminated",
                _ => "Can't find string terminator anywhere before EOF",
            }));
        }
        self.pos += 1;
        let first = match word {
            "m" | "qr" => self.delimited_or(open, "Search pattern not terminated")?,
            "s" => self.delimited_or(open, "Substitution pattern not terminated")?,
            "tr" | "y" => self.delimited_or(open, "Transliteration pattern not terminated")?,
            _ => self.delimited(open)?,
        };
        let second_msg = if word == "s" { "Substitution replacement not terminated" } else { "Transliteration replacement not terminated" };
        match word {
            "q" => Ok(Expr::Const(Value::bytes(single_quoted(&first, closing(open))))),
            "qq" => self.interp_expr(&first),
            "qw" => Ok(Expr::List(
                first.split(|c| c.is_ascii_whitespace()).filter(|w| !w.is_empty()).map(|w| Expr::Const(Value::bytes(single_quoted(w, closing(open))))).collect(),
            )),
            "m" | "qr" => {
                let flags = self.regex_flags();
                let pat = if open == b'\'' { Pat { parts: vec![Part::Lit(first)], flags } } else { self.pattern(&first, flags)? };
                if word == "qr" {
                    Ok(Expr::Qr(pat))
                } else {
                    Ok(Expr::Match { target: None, neg: false, pat })
                }
            }
            "s" | "tr" | "y" => {
                let second = if open != closing(open) {
                    self.ws();
                    let o2 = self.at(0);
                    if o2 == 0 {
                        return Err(self.fatal(second_msg));
                    }
                    self.pos += 1;
                    self.delimited_or(o2, second_msg)?
                } else {
                    self.delimited_or(open, second_msg)?
                };
                let flags = self.regex_flags();
                if word == "s" {
                    let pat = self.pattern(&first, Rc::from(flags.replace(['e', 'g', 'r'], "")))?;
                    let pat = Pat { parts: pat.parts, flags: flags.clone() };
                    let repl = if flags.contains('e') {
                        SubstRepl::Code(Rc::new(parse_program(&second, &self.package)?))
                    } else if open == b'\'' {
                        SubstRepl::Interp(vec![Part::Lit(second)])
                    } else {
                        SubstRepl::Interp(self.interpolate(&second, false)?)
                    };
                    Ok(Expr::Subst { target: None, pat, repl })
                } else {
                    Ok(Expr::Trans { target: None, search: first, repl: second, flags })
                }
            }
            _ => unreachable!(),
        }
    }

    fn word_term(&mut self) -> R<Expr> {
        let id = self.peek_ident().unwrap();
        let start = self.pos;
        self.pos += id.len();
        // NAME => is a string.
        if self.starts("=>") {
            return Ok(Expr::Const(Value::str(id)));
        }
        // Quote-like operators need a delimiter right after the word.
        if matches!(id, "q" | "qq" | "qw" | "m" | "s" | "tr" | "y" | "qr") {
            let save = self.pos;
            let mut k = self.pos;
            while k < self.s.len() && self.s[k] == b' ' {
                k += 1;
            }
            let d = self.s.get(k).copied().unwrap_or(0);
            let immediate = k == save;
            if d == 0 && matches!(id, "m" | "s" | "tr" | "y" | "qr") {
                return self.quote_like(id);
            }
            if !d.is_ascii_alphanumeric() && d != b'_' && !d.is_ascii_whitespace() && d != 0 && d != b'=' && d != b',' && d != b';' && d != b')' && (immediate || matches!(d, b'{' | b'/' | b'(' | b'[' | b'<' | b'!' | b'|')) {
                return self.quote_like(id);
            }
            self.pos = save;
        }
        match id {
            "my" | "our" | "local" | "state" => {
                let vars = if self.starts("(") {
                    self.pos += 1;
                    let mut v = Vec::new();
                    while !self.eat(")") {
                        if self.eat(",") {
                            continue;
                        }
                        if self.eat_word("undef") {
                            v.push(Expr::Const(Value::Undef));
                            continue;
                        }
                        v.push(self.decl_var()?);
                    }
                    (v, true)
                } else {
                    (vec![self.decl_var_or_term(id)?], false)
                };
                return Ok(match id {
                    "my" | "state" => Expr::My(vars.0, vars.1),
                    "local" => Expr::Local(Box::new(if vars.1 { Expr::List(vars.0) } else { vars.0.into_iter().next().unwrap() })),
                    _ => {
                        if vars.1 {
                            Expr::List(vars.0)
                        } else {
                            vars.0.into_iter().next().unwrap()
                        }
                    }
                });
            }
            "not" => {
                // `not` where a term is expected: return not $x, ($a, not $b)
                if self.starts(")") || self.at_terminator() {
                    return Ok(Expr::Un(UnOp::Not, Box::new(Expr::Const(Value::Undef))));
                }
                return Ok(Expr::Un(UnOp::Not, Box::new(self.assign()?)));
            }
            "sub" => {
                self.skip_prototype();
                let body = self.block()?;
                return Ok(Expr::AnonSub(Rc::new(Sub { name: None, package: self.package.clone(), body: Rc::new(body) })));
            }
            "do" => {
                if self.starts("{") {
                    return Ok(Expr::Do(Rc::new(self.block()?)));
                }
                let arg = self.unary()?;
                return Ok(Expr::Call { name: Rc::from("do"), args: vec![arg], amp: false });
            }
            "eval" => {
                if self.starts("{") {
                    return Ok(Expr::Eval(Rc::new(self.block()?)));
                }
                let arg = if self.starts("(") {
                    let mut a = self.paren_args()?;
                    if a.is_empty() { Expr::Scalar(Rc::from("_")) } else { a.remove(0) }
                } else if self.term_follows() {
                    self.uni()?
                } else {
                    Expr::Scalar(Rc::from("_"))
                };
                return Ok(Expr::EvalStr(Box::new(arg)));
            }
            "__PACKAGE__" => return Ok(Expr::Const(Value::str(&self.package))),
            "__FILE__" => return Ok(Expr::Const(Value::str("(eval)"))),
            "__LINE__" => {
                let line = 1 + self.s[..start].iter().filter(|&&b| b == b'\n').count();
                return Ok(Expr::Const(Value::Int(line as i64)));
            }
            "wantarray" => {
                if self.starts("(") {
                    self.paren_args()?;
                }
                return Ok(Expr::Wantarray);
            }
            "return" => {
                if self.at_terminator() {
                    return Ok(Expr::Return(None));
                }
                return Ok(Expr::Return(Some(Box::new(self.comma()?))));
            }
            "last" | "next" | "redo" => {
                let label = match self.peek_ident() {
                    Some(l) if !is_keyword(l) => {
                        self.pos += l.len();
                        Some(Rc::from(l))
                    }
                    _ => None,
                };
                return Ok(match id {
                    "next" => Expr::Next(label),
                    "redo" => Expr::Redo(label),
                    _ => Expr::Last(label),
                });
            }
            "goto" => {
                let label = self.peek_ident().ok_or_else(|| self.err("goto LABEL expected"))?;
                self.pos += label.len();
                return Ok(Expr::Goto(Rc::from(label)));
            }
            "map" | "grep" | "sort" => return self.list_callback(id),
            "print" | "printf" => {
                let paren = self.starts("(");
                if paren {
                    self.pos += 1;
                }
                let fh = self.print_handle()?;
                let args = if paren {
                    let e = if self.starts(")") { Expr::List(vec![]) } else { self.expr()? };
                    self.expect(")")?;
                    flatten_list(e)
                } else if self.at_terminator() {
                    vec![]
                } else {
                    flatten_list(self.comma()?)
                };
                return Ok(Expr::Print { printf: id == "printf", fh: fh.map(Box::new), args });
            }
            _ => {}
        }
        // Builtins whose first argument may be a bareword filehandle.
        if FH_FUNCS.contains(&id) {
            let paren = self.starts("(");
            if paren {
                self.pos += 1;
            }
            let mut args = Vec::new();
            if let Some(h) = self.peek_ident() {
                let after = self.pos + h.len();
                let next = self.s[after..].iter().find(|c| !c.is_ascii_whitespace()).copied();
                if !NAMED_UNARY.contains(&h) && !LIST_OPS.contains(&h) && h != "my" && matches!(next, Some(b',' | b')' | b';') | None) {
                    self.pos = after;
                    args.push(Expr::Const(Value::str(&format!("*{h}"))));
                    self.eat(",");
                }
            }
            if paren {
                if !self.starts(")") {
                    args.extend(flatten_list(self.expr()?));
                }
                self.expect(")")?;
            } else if !self.at_terminator() && (args.is_empty() || self.term_follows()) && self.term_follows_op(true) {
                args.extend(flatten_list(self.comma()?));
            }
            return Ok(Expr::Call { name: Rc::from(id), args, amp: false });
        }
        // Indirect object syntax: `import File::Glob qw(...)`, `new Foo(...)`.
        if matches!(id, "import" | "new" | "unimport") {
            if let Some(class) = self.peek_ident() {
                if class.contains("::") || class.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                    self.pos += class.len();
                    let args = if self.starts("(") {
                        self.paren_args()?
                    } else if self.at_terminator() {
                        vec![]
                    } else {
                        flatten_list(self.comma()?)
                    };
                    return Ok(Expr::Method { inv: Box::new(Expr::Const(Value::str(class))), name: MethodName::Static(Rc::from(id)), args });
                }
            }
        }
        // Builtins and subs.
        let is_unary = NAMED_UNARY.contains(&id);
        let is_list = LIST_OPS.contains(&id);
        if self.starts("(") {
            let mut args = self.paren_args()?;
            if !is_unary && !is_list {
                if let Some(p) = self.sub_prototype(id) {
                    args = apply_prototype(&p, args);
                }
            }
            return Ok(Expr::Call { name: Rc::from(id), args, amp: false });
        }
        if self.starts("->") || id.ends_with("::") {
            // A class name.
            return Ok(Expr::Const(Value::str(id.trim_end_matches("::"))));
        }
        if is_unary {
            if NO_ARGS.contains(&id) || !self.term_follows() {
                return Ok(Expr::Call { name: Rc::from(id), args: vec![], amp: false });
            }
            let arg = self.uni()?;
            return Ok(Expr::Call { name: Rc::from(id), args: vec![arg], amp: false });
        }
        if is_list {
            if NO_ARGS.contains(&id) || self.at_terminator() || !self.term_follows_op(true) {
                return Ok(Expr::Call { name: Rc::from(id), args: vec![], amp: false });
            }
            let args = flatten_list(self.comma()?);
            return Ok(Expr::Call { name: Rc::from(id), args, amp: false });
        }
        // A user sub called as a list operator: NAME $arg, ...
        self.ws();
        let quote_arg = matches!(self.peek_ident(), Some("q" | "qq" | "qw")) && {
            let w = self.peek_ident().unwrap().len();
            matches!(self.at(w), b'(' | b'{' | b'[' | b'/' | b'<' | b'!' | b'|')
        };
        if (matches!(self.at(0), b'$' | b'"' | b'\'' | b'@') && !self.starts("$)")) || quote_arg {
            let mut args = flatten_list(self.comma()?);
            if let Some(p) = self.sub_prototype(id) {
                args = apply_prototype(&p, args);
            }
            return Ok(Expr::Call { name: Rc::from(id), args, amp: false });
        }
        Ok(Expr::Call { name: Rc::from(id), args: vec![], amp: true })
    }

    /// Whether `=>` follows (after spaces) the `n` bytes at the position.
    fn fat_comma_after(&self, n: usize) -> bool {
        let r = &self.rest()[n.min(self.rest().len())..];
        let k = r.iter().take_while(|c| c.is_ascii_whitespace()).count();
        r[k..].starts_with(b"=>") || r[k..].starts_with(b",")
    }

    /// The filehandle of `print FH LIST`, `print $fh LIST`, `print {$fh}`.
    fn print_handle(&mut self) -> R<Option<Expr>> {
        self.ws();
        if self.at(0) == b'{' {
            self.pos += 1;
            let e = self.expr()?;
            self.expect("}")?;
            return Ok(Some(e));
        }
        if let Some(h) = self.peek_ident() {
            let after = self.pos + h.len();
            let mut k = after;
            while k < self.s.len() && self.s[k].is_ascii_whitespace() {
                k += 1;
            }
            let next = self.s.get(k).copied().unwrap_or(0);
            // A bareword followed by a term (not an operator or a comma).
            let word_after = {
                let mut m = k;
                while m < self.s.len() && ident_char(self.s[m]) {
                    m += 1;
                }
                std::str::from_utf8(&self.s[k..m]).unwrap_or("")
            };
            let all_caps = h.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
            let followed_by_term = k > after && matches!(next, b'"' | b'\'' | b'$' | b'@' | b'0'..=b'9')
                || next == b';'
                || next == 0
                || next == b')'
                || (all_caps && k > after && (next == b'<' && self.s.get(k + 1) == Some(&b'<') || ident_start(next) && !matches!(word_after, "x" | "eq" | "ne" | "lt" | "gt" | "le" | "ge" | "cmp" | "and" | "or" | "xor" | "if" | "unless" | "for" | "foreach")));
            if !NAMED_UNARY.contains(&h) && !LIST_OPS.contains(&h) && !matches!(h, "map" | "grep" | "sort" | "join" | "sprintf" | "scalar" | "reverse") && followed_by_term && (h.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') || next != b'(') {
                self.pos = after;
                return Ok(Some(Expr::Const(Value::str(&format!("*{h}")))));
            }
        }
        if self.at(0) == b'$' && ident_start(self.at(1)) {
            // print $fh "..." : a scalar followed by a term without a comma.
            let save = self.pos;
            self.pos += 1;
            let id = self.peek_ident().unwrap();
            let after = self.pos + id.len();
            let mut k = after;
            while k < self.s.len() && self.s[k] == b' ' {
                k += 1;
            }
            let next = self.s.get(k).copied().unwrap_or(0);
            let word_after = {
                let mut m = k;
                while m < self.s.len() && ident_char(self.s[m]) {
                    m += 1;
                }
                std::str::from_utf8(&self.s[k..m]).unwrap_or("")
            };
            let term_start = matches!(next, b'"' | b'\'' | b'$' | b'@' | b'(' | b'\\' | b'0'..=b'9')
                || (ident_start(next) && !matches!(word_after, "x" | "eq" | "ne" | "lt" | "gt" | "le" | "ge" | "cmp" | "and" | "or" | "xor" | "if" | "unless" | "for" | "foreach" | "while" | "until" | "isa"));
            if k > after && term_start && self.s.get(after) != Some(&b'[') && self.s.get(after) != Some(&b'{') {
                self.pos = after;
                return Ok(Some(Expr::Scalar(Rc::from(id))));
            }
            self.pos = save;
        }
        Ok(None)
    }

    /// Whether a term starts at the current position (for optional
    /// arguments of named operators).
    fn term_follows(&mut self) -> bool {
        self.term_follows_op(false)
    }

    /// `list_op`: after a list operator such as `split`, a `/` starts a
    /// pattern, as perl expects a term there.
    fn term_follows_op(&mut self, list_op: bool) -> bool {
        self.ws();
        let r = self.rest();
        match r.first() {
            None => false,
            // &name / &$code is a term; && and &= are operators.
            Some(b'&') => matches!(r.get(1), Some(c) if ident_start(*c) || *c == b'$' || *c == b'{' || *c == b':'),
            Some(b')' | b']' | b'}' | b';' | b',' | b':' | b'?' | b'=' | b'<' | b'>' | b'|' | b'*' | b'^' | b'.' | b'!') => {
                // `!` starts a term unless it is `!=`/`!~`.
                r[0] == b'!' && !matches!(r.get(1), Some(b'=' | b'~'))
            }
            Some(b'/') => list_op,
            Some(b'-') => !matches!(r.get(1), Some(b' ' | b'=' | b'-')),
            Some(b'+') => !matches!(r.get(1), Some(b' ' | b'=' | b'+')),
            Some(b'x') if !r.get(1).is_some_and(|&c| ident_char(c)) => false,
            Some(_) => !matches!(
                self.peek_ident(),
                Some("or" | "and" | "if" | "unless" | "while" | "until" | "for" | "foreach" | "xor" | "eq" | "ne" | "lt" | "gt" | "le" | "ge" | "cmp" | "x")
            ),
        }
    }

    fn list_callback(&mut self, id: &str) -> R<Expr> {
        let paren = self.starts("(");
        if paren {
            self.pos += 1;
        }
        let cb = if self.starts("{") {
            // A block (map/grep/sort), unless it is an anonymous hash
            // followed by a comma.
            let save = self.pos;
            match self.block() {
                Ok(b) if !self.starts(",") => Some(Callback::Block(Rc::new(b))),
                _ => {
                    self.pos = save;
                    None
                }
            }
        } else {
            None
        };
        let (cb, list) = match cb {
            Some(cb) => {
                self.eat(",");
                let list = if (paren && self.starts(")")) || self.at_terminator() { Expr::List(vec![]) } else { self.comma()? };
                (Some(cb), list)
            }
            None if id == "sort" => {
                // sort SUBNAME LIST / sort $subref LIST / sort LIST. As perl
                // (toke.c force_word): after `sort` a word that is not a
                // keyword is the sort sub, even before `(`; inside
                // `sort(...)`, a word directly followed by `(` is a call
                let mut cb = None;
                // sort $subref LIST: a scalar followed by a term, not by an operator
                if self.at(0) == b'$' && ident_start(self.at(1)) {
                    let mut e = self.pos + 1;
                    while e < self.s.len() && ident_char(self.s[e]) {
                        e += 1;
                    }
                    let mut f = e;
                    while f < self.s.len() && self.s[f].is_ascii_whitespace() {
                        f += 1;
                    }
                    if f > e && matches!(self.s.get(f), Some(b'@' | b'$' | b'(' | b'\'' | b'"')) || (f > e && self.s.get(f).is_some_and(|c| c.is_ascii_alphabetic())) {
                        let v = Expr::Scalar(Rc::from(String::from_utf8_lossy(&self.s[self.pos + 1..e]).into_owned()));
                        self.pos = e;
                        cb = Some(Callback::Expr(Box::new(v)));
                    }
                }
                if cb.is_none() {
                if let Some(name) = self.peek_ident() {
                    let after = self.pos + name.len();
                    let next = self.s[after..].iter().find(|c| !c.is_ascii_whitespace()).copied();
                    let keyword = NAMED_UNARY.contains(&name) || LIST_OPS.contains(&name) || is_keyword(name) || matches!(name, "map" | "grep" | "sort" | "reverse" | "glob" | "print" | "printf" | "wantarray" | "lc" | "uc" | "exists" | "delete");
                    let call = paren && self.s.get(after) == Some(&b'(');
                    if !keyword && !call && next != Some(b',') && !(next == Some(b'=') && self.s[after..].iter().filter(|c| !c.is_ascii_whitespace()).nth(1) == Some(&b'>')) {
                        self.pos = after;
                        cb = Some(Callback::Expr(Box::new(Expr::Const(Value::str(name)))));
                    }
                }
                }
                let list = if (paren && self.starts(")")) || self.at_terminator() { Expr::List(vec![]) } else { self.comma()? };
                (cb, list)
            }
            None => {
                // map EXPR, LIST
                let e = self.assign()?;
                self.expect(",")?;
                let list = self.comma()?;
                (Some(Callback::Expr(Box::new(e))), list)
            }
        };
        if paren {
            self.expect(")")?;
        }
        let list = Box::new(list);
        Ok(match id {
            "map" => Expr::Map(cb.unwrap(), list),
            "grep" => Expr::Grep(cb.unwrap(), list),
            _ => Expr::Sort(cb, list),
        })
    }

    /// A variable in a `my (...)` list.
    fn decl_var(&mut self) -> R<Expr> {
        self.ws();
        let sigil = self.at(0);
        if sigil == b'*' && ident_start(self.at(1)) {
            // local *FH: a glob (a filehandle here).
            return self.term();
        }
        if !matches!(sigil, b'$' | b'@' | b'%') {
            return Err(self.err("variable expected"));
        }
        self.pos += 1;
        let name = self.var_name().ok_or_else(|| self.err("variable name expected"))?;
        Ok(match sigil {
            b'$' => Expr::Scalar(name),
            b'@' => Expr::Array(Container::Named(name)),
            _ => Expr::Hash(Container::Named(name)),
        })
    }

    /// After `my`/`local`: a variable, or for `local` any lvalue term.
    fn decl_var_or_term(&mut self, kw: &str) -> R<Expr> {
        if kw == "local" {
            return self.postfix();
        }
        self.decl_var()
    }

    fn amp_call(&mut self) -> R<Expr> {
        self.pos += 1;
        self.ws();
        let target = if self.at(0) == b'$' {
            self.pos += 1;
            let id = self.peek_ident().ok_or_else(|| self.err("variable expected"))?;
            self.pos += id.len();
            Some(Expr::Scalar(Rc::from(id)))
        } else if self.at(0) == b'{' {
            self.pos += 1;
            let e = self.expr()?;
            self.expect("}")?;
            Some(e)
        } else {
            None
        };
        match target {
            Some(t) => {
                let args = if self.starts("(") { self.paren_args()? } else { vec![Expr::Array(Container::Named(Rc::from("_")))] };
                Ok(Expr::CallRef(Box::new(t), args))
            }
            None => {
                let id = self.peek_ident().ok_or_else(|| self.err("sub name expected"))?;
                self.pos += id.len();
                let args = if self.starts("(") { self.paren_args()? } else { vec![Expr::Array(Container::Named(Rc::from("_")))] };
                Ok(Expr::Call { name: Rc::from(id), args, amp: true })
            }
        }
    }

    /// A variable name after a sigil: identifier, `::name`, digits, `^W` or
    /// one punctuation character.
    fn var_name(&mut self) -> Option<Rc<str>> {
        let c = self.at(0);
        if ident_start(c) || (c == b':' && self.at(1) == b':' && ident_start(self.at(2))) {
            let s = self.rest();
            let mut n = 0;
            loop {
                while n < s.len() && ident_char(s[n]) {
                    n += 1;
                }
                if s[n..].starts_with(b"::") {
                    n += 2;
                    continue;
                }
                break;
            }
            let name = String::from_utf8_lossy(&s[..n]).into_owned();
            self.pos += n;
            let name = name.strip_prefix("::").map(|x| format!("main::{x}")).unwrap_or(name);
            return Some(Rc::from(name));
        }
        if c.is_ascii_digit() {
            let mut n = 0;
            while self.at(n).is_ascii_digit() {
                n += 1;
            }
            let name = String::from_utf8_lossy(&self.s[self.pos..self.pos + n]).into_owned();
            self.pos += n;
            return Some(Rc::from(name));
        }
        if c == b'^' && self.at(1).is_ascii_uppercase() {
            let name = format!("^{}", self.at(1) as char);
            self.pos += 2;
            return Some(Rc::from(name));
        }
        if b"&`'+/\\,;!@0<>]$.|\"?".contains(&c) && c != 0 {
            self.pos += 1;
            return Some(Rc::from((c as char).to_string()));
        }
        None
    }

    /// What follows `$`, `@`, `%` or `$#` when it is a dereference: `$name`
    /// (a scalar holding the reference), `{EXPR}`, or another `$` level.
    /// Returns `None` when the sigil is followed by a plain name.
    fn deref_target(&mut self) -> R<Option<Expr>> {
        match self.at(0) {
            b'{' => {
                // ${name} is the variable, ${ EXPR } a dereference.
                let save = self.pos;
                self.pos += 1;
                self.ws();
                let id = match self.peek_ident() {
                    Some(id) => Some(id),
                    None => {
                        // ${3}, ${^WARNING_BITS}
                        let r = self.rest();
                        let n = r.iter().take_while(|c| c.is_ascii_digit()).count();
                        if n > 0 {
                            std::str::from_utf8(&r[..n]).ok()
                        } else if r.first() == Some(&b'^') {
                            let m = 1 + r[1..].iter().take_while(|&&c| ident_char(c)).count();
                            std::str::from_utf8(&r[..m]).ok()
                        } else {
                            None
                        }
                    }
                };
                if let Some(id) = id {
                    let after = self.pos + id.len();
                    let mut k = after;
                    while k < self.s.len() && self.s[k].is_ascii_whitespace() {
                        k += 1;
                    }
                    if self.s.get(k) == Some(&b'}') {
                        self.pos = k + 1;
                        return Ok(Some(Expr::Const(Value::str(&format!("\0name:{id}")))));
                    }
                }
                self.pos = save + 1;
                let e = self.expr()?;
                self.expect("}")?;
                Ok(Some(e))
            }
            b'$' if ident_start(self.at(1)) || self.at(1) == b'$' || self.at(1) == b'{' || self.at(1) == b':' => {
                self.pos += 1;
                // The inner level, without subscripts.
                match self.deref_target()? {
                    Some(Expr::Const(Value::Str(s))) if s.starts_with(b"\0name:") => {
                        let name = String::from_utf8_lossy(&s[6..]).into_owned();
                        Ok(Some(Expr::Scalar(Rc::from(name))))
                    }
                    Some(inner) => Ok(Some(Expr::ScalarDeref(Box::new(inner)))),
                    None => {
                        let name = self.var_name().ok_or_else(|| self.err("variable name expected"))?;
                        Ok(Some(Expr::Scalar(name)))
                    }
                }
            }
            _ => Ok(None),
        }
    }

    fn scalar_term(&mut self) -> R<Expr> {
        self.pos += 1;
        // $#array / $#{expr} / $#$ref
        if self.at(0) == b'#' && (self.at(1) == b'{' || self.at(1) == b'$' || ident_start(self.at(1))) {
            self.pos += 1;
            return Ok(match self.deref_target()? {
                Some(Expr::Const(Value::Str(s))) if s.starts_with(b"\0name:") => Expr::LastIndex(Container::Named(Rc::from(String::from_utf8_lossy(&s[6..]).as_ref()))),
                Some(t) => Expr::LastIndex(Container::Deref(Box::new(t))),
                None => Expr::LastIndex(Container::Named(self.var_name().ok_or_else(|| self.err("array name expected"))?)),
            });
        }
        let container = match self.deref_target()? {
            Some(Expr::Const(Value::Str(s))) if s.starts_with(b"\0name:") => {
                let name: Rc<str> = Rc::from(String::from_utf8_lossy(&s[6..]).as_ref());
                return self.subscripts_named(name);
            }
            Some(t) => t,
            None => {
                let name = self.var_name().ok_or_else(|| self.err("variable name expected"))?;
                return self.subscripts_named(name);
            }
        };
        // $$ref[i] / $$ref{k} / $$ref
        match self.at(0) {
            b'[' => {
                self.pos += 1;
                let i = self.expr()?;
                self.expect("]")?;
                Ok(Expr::Elem(Container::Deref(Box::new(container)), Box::new(i)))
            }
            b'{' => {
                let k = self.hash_key()?;
                Ok(Expr::HElem(Container::Deref(Box::new(container)), Box::new(k)))
            }
            _ => Ok(Expr::ScalarDeref(Box::new(container))),
        }
    }

    fn subscripts_named(&mut self, name: Rc<str>) -> R<Expr> {
        match self.at(0) {
            b'[' => {
                self.pos += 1;
                let i = self.expr()?;
                self.expect("]")?;
                Ok(Expr::Elem(Container::Named(name), Box::new(i)))
            }
            b'{' => {
                let k = self.hash_key()?;
                Ok(Expr::HElem(Container::Named(name), Box::new(k)))
            }
            _ => Ok(Expr::Scalar(name)),
        }
    }

    fn container_after_sigil(&mut self) -> R<Container> {
        Ok(match self.deref_target()? {
            Some(Expr::Const(Value::Str(s))) if s.starts_with(b"\0name:") => Container::Named(Rc::from(String::from_utf8_lossy(&s[6..]).as_ref())),
            Some(t) => Container::Deref(Box::new(t)),
            None => Container::Named(self.var_name().ok_or_else(|| self.err("variable name expected"))?),
        })
    }

    fn array_term(&mut self) -> R<Expr> {
        self.pos += 1;
        let c = self.container_after_sigil()?;
        match self.at(0) {
            b'[' => {
                self.pos += 1;
                let i = if self.starts("]") { Expr::List(vec![]) } else { self.expr()? };
                self.expect("]")?;
                Ok(Expr::Slice(c, Box::new(i)))
            }
            b'{' => {
                self.pos += 1;
                let k = self.expr()?;
                self.expect("}")?;
                Ok(Expr::HSlice(c, Box::new(k)))
            }
            _ => Ok(Expr::Array(c)),
        }
    }

    fn hash_term(&mut self) -> R<Expr> {
        self.pos += 1;
        let c = self.container_after_sigil()?;
        Ok(Expr::Hash(c))
    }

    // ---- interpolation ----

    /// Split the inside of a double-quoted string or a pattern into
    /// literal text and interpolated expressions. In a pattern, escapes are
    /// left for the regex engine.
    fn interpolate(&mut self, raw: &[u8], regex: bool) -> R<Vec<Part>> {
        let mut parts = Vec::new();
        let mut lit = Vec::new();
        let mut i = 0;
        let flush = |lit: &mut Vec<u8>, parts: &mut Vec<Part>| {
            if !lit.is_empty() {
                parts.push(Part::Lit(std::mem::take(lit)));
            }
        };
        while i < raw.len() {
            let c = raw[i];
            if c == b'\\' && i + 1 < raw.len() {
                let e = raw[i + 1];
                let case = match e {
                    b'U' => Some(CaseOp::Upper),
                    b'L' => Some(CaseOp::Lower),
                    b'Q' => Some(CaseOp::Quote),
                    b'E' => Some(CaseOp::End),
                    b'u' => Some(CaseOp::UpperNext),
                    b'l' => Some(CaseOp::LowerNext),
                    _ => None,
                };
                if let Some(op) = case {
                    flush(&mut lit, &mut parts);
                    parts.push(Part::Case(op));
                    i += 2;
                    continue;
                }
                if regex {
                    lit.push(c);
                    lit.push(e);
                    i += 2;
                    continue;
                }
                let (bytes, n) = string_escape(&raw[i + 1..]);
                lit.extend_from_slice(&bytes);
                i += 1 + n;
                continue;
            }
            if c == b'$' && i + 1 < raw.len() {
                if let Some(end) = interp_var_end(raw, i, regex) {
                    flush(&mut lit, &mut parts);
                    let src = &raw[i..end];
                    let mut sub = Parser { s: src, pos: 0, package: self.package.clone(), line_pos: 0, line: 1 };
                    let e = sub.postfix()?;
                    parts.push(Part::Scalar(e));
                    i = end;
                    continue;
                }
            }
            if c == b'@' && i + 1 < raw.len() && (ident_start(raw[i + 1]) || raw[i + 1] == b'{' || raw[i + 1] == b'$' || raw[i + 1] == b'_') {
                if let Some(end) = interp_var_end(raw, i, regex) {
                    flush(&mut lit, &mut parts);
                    let src = &raw[i..end];
                    let mut sub = Parser { s: src, pos: 0, package: self.package.clone(), line_pos: 0, line: 1 };
                    let e = sub.postfix()?;
                    parts.push(Part::Array(e));
                    i = end;
                    continue;
                }
            }
            lit.push(c);
            i += 1;
        }
        flush(&mut lit, &mut parts);
        Ok(parts)
    }
}

/// Words that start statements or expressions, never labels.
fn is_keyword(w: &str) -> bool {
    matches!(
        w,
        "if" | "unless" | "while" | "until" | "for" | "foreach" | "my" | "our" | "local" | "return" | "print" | "printf" | "sub" | "package" | "use" | "no"
            | "do" | "eval" | "else" | "elsif" | "last" | "next" | "redo" | "goto" | "and" | "or" | "not" | "xor" | "defined" | "undef" | "require"
            | "q" | "qq" | "qw" | "m" | "s" | "tr" | "y" | "qr" | "default" | "__END__" | "__DATA__"
    )
}

fn trim_start(l: &[u8]) -> &[u8] {
    let n = l.iter().take_while(|c| **c == b' ' || **c == b'\t').count();
    &l[n..]
}

fn flatten_list(e: Expr) -> Vec<Expr> {
    match e {
        Expr::List(v) => v,
        e => vec![e],
    }
}

/// The inside of a single-quoted string: only `\\` and the delimiter escape.
fn single_quoted(raw: &[u8], delim: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'\\' && i + 1 < raw.len() && (raw[i + 1] == b'\\' || raw[i + 1] == delim) {
            out.push(raw[i + 1]);
            i += 2;
        } else {
            out.push(raw[i]);
            i += 1;
        }
    }
    out
}

/// A double-quoted escape (after the backslash): the bytes and how many
/// input bytes it used.
fn string_escape(s: &[u8]) -> (Vec<u8>, usize) {
    let c = s[0];
    let one = |b: u8| (vec![b], 1);
    match c {
        b'n' => one(b'\n'),
        b't' => one(b'\t'),
        b'r' => one(b'\r'),
        b'f' => one(0x0c),
        b'e' => one(0x1b),
        b'a' => one(0x07),
        b'0'..=b'7' => {
            let mut n = 0;
            let mut v: u32 = 0;
            while n < 3 && n < s.len() && (b'0'..=b'7').contains(&s[n]) {
                v = v * 8 + (s[n] - b'0') as u32;
                n += 1;
            }
            (encode_char(v), n)
        }
        b'x' => {
            if s.get(1) == Some(&b'{') {
                let end = s.iter().position(|&b| b == b'}').unwrap_or(s.len());
                let hex = String::from_utf8_lossy(&s[2..end]).replace('_', "");
                let v = u32::from_str_radix(hex.trim(), 16).unwrap_or(0);
                (encode_char(v), end + 1)
            } else {
                let mut n = 1;
                while n < 3 && n < s.len() && s[n].is_ascii_hexdigit() {
                    n += 1;
                }
                let v = u32::from_str_radix(std::str::from_utf8(&s[1..n]).unwrap_or("0"), 16).unwrap_or(0);
                (vec![v as u8], n)
            }
        }
        b'c' if s.len() > 1 => (vec![s[1].to_ascii_uppercase() ^ 64], 2),
        b'N' if s.get(1) == Some(&b'{') => {
            let end = s.iter().position(|&b| b == b'}').unwrap_or(s.len());
            let name = String::from_utf8_lossy(&s[2..end]).into_owned();
            let v = name.strip_prefix("U+").and_then(|h| u32::from_str_radix(h, 16).ok()).unwrap_or(0xFFFD);
            (char::from_u32(v).unwrap_or('\u{FFFD}').to_string().into_bytes(), end + 1)
        }
        other => one(other),
    }
}

/// A character code in a byte string: below 256 a byte, above as UTF-8 (as
/// perl does once the string is upgraded).
fn encode_char(v: u32) -> Vec<u8> {
    if v < 256 {
        vec![v as u8]
    } else {
        char::from_u32(v).unwrap_or('\u{FFFD}').to_string().into_bytes()
    }
}

/// The end of an interpolated variable starting at `raw[i]` (`$` or `@`),
/// or `None` when it is not one (a `$` anchor in a pattern).
fn interp_var_end(raw: &[u8], i: usize, regex: bool) -> Option<usize> {
    let mut k = i + 1;
    let at = |k: usize| raw.get(k).copied().unwrap_or(0);
    // Leading derefs: $$name
    while at(k) == b'$' && (ident_start(at(k + 1)) || at(k + 1) == b'$' || at(k + 1) == b'{') {
        k += 1;
    }
    match at(k) {
        b'{' => {
            k = matching(raw, k)? + 1;
        }
        c if ident_start(c) || (c == b':' && at(k + 1) == b':') => {
            loop {
                while ident_char(at(k)) {
                    k += 1;
                }
                if at(k) == b':' && at(k + 1) == b':' && ident_start(at(k + 2)) {
                    k += 2;
                    continue;
                }
                break;
            }
        }
        c if c.is_ascii_digit() => {
            while at(k).is_ascii_digit() {
                k += 1;
            }
            return Some(k);
        }
        b'&' | b'@' if !regex && raw[i] == b'$' => return Some(k + 1),
        // Punctuation variables a string interpolates ("line$/"); in a
        // pattern `$/` is an anchor followed by a slash.
        b'/' | b',' | b';' | b'!' if !regex && raw[i] == b'$' && k == i + 1 => return Some(k + 1),
        _ => return None,
    }
    // Subscripts.
    loop {
        if at(k) == b'[' {
            let end = matching(raw, k)?;
            let inside = &raw[k + 1..end];
            // A character class in a pattern is not a subscript.
            let index_like = !inside.is_empty() && (inside.iter().all(|c| c.is_ascii_digit() || *c == b'-') || (inside[0] == b'$' && inside[1..].iter().all(|&c| ident_char(c))));
            if !index_like || (regex && !inside.iter().all(|c| c.is_ascii_digit() || *c == b'$' || ident_char(*c) || *c == b'-')) {
                return Some(k);
            }
            k = end + 1;
        } else if at(k) == b'{' {
            let end = matching(raw, k)?;
            let inside = &raw[k + 1..end];
            // A quantifier in a pattern: $x{2,3}
            if regex && inside.iter().all(|c| c.is_ascii_digit() || *c == b',') {
                return Some(k);
            }
            k = end + 1;
        } else if at(k) == b'-' && at(k + 1) == b'>' && (at(k + 2) == b'[' || at(k + 2) == b'{') {
            k += 2;
        } else {
            return Some(k);
        }
    }
}

fn matching(raw: &[u8], open_at: usize) -> Option<usize> {
    let open = raw[open_at];
    let close = closing(open);
    let mut depth = 0;
    let mut k = open_at;
    while k < raw.len() {
        let c = raw[k];
        if c == b'\\' {
            k += 2;
            continue;
        }
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return Some(k);
            }
        }
        k += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(src: &str) -> Block {
        parse_program(src.as_bytes(), "Image::ExifTool").unwrap_or_else(|e| panic!("{src}: {e}"))
    }

    #[test]
    fn table_strings() {
        for s in [
            "$val * 2",
            "$$self{FacesDetected} < 1 ? undef : $val",
            "$$self{TempTest2}=$val; $$self{OPTIONS}{Unknown}<2 ? undef : $val",
            "substr($val, 7)",
            "$val=~/(\\d+) (\\d+)/ ? \"$2.$1\" : \"0.$val\"",
            "Image::ExifTool::Exif::RedBlueBalance(1,@val)",
            "unpack(\"N\",$val)",
            "$val>0 ? int(512*log($val)/log(2)+0.5) : 0",
            "return $val=~/^(none|\\xff\\xd8\\xff)/s ? undef : \"Not a valid image\"",
            "($val =~ s/^-// ? \"$val m Below\" : \"$val m Above\") . \" Sea Level\"",
            "$$self{Model} =~ /^(SLT-A(58|99V?)|HV|NEX-(3N|5R|5T|6|VG900|VG30E)|ILCE-([35]000|3500))\\b/",
            "$$valPt =~ /^TDHD\\x01\\0\\0\\0/",
            "$self->{MPEG_Vers} == 3",
            "\"$val cm/s\"",
            "$_ = $val; if (/^(19|20)(\\d{2}):(\\d{2}):(\\d{2}) (\\d{2}):(\\d{2})$/) { return \"$2$3\\0\\0$4$5\\0\\0$6\\0\\0\\0\\0\"; } elsif (/^Unknown\\s*\\((.*)\\)$/i) { return 1 }",
            "$$self{DIR_NAME} ne 'IFD0' or not $$self{SubfileType} or not require Image::ExifTool::Sony or Image::ExifTool::Sony::SetARW($self, $valPt)",
            "my @a = split ' ', $val; join ' ', map { sprintf('%.2f', $_) } @a",
            "sprintf('%.4x', $val) . ($val & 0x8000 ? ' (on)' : '')",
            "$val =~ tr/ //d; $val",
            "+{ 1 => 'On', 2 => 'Off' }->{$val}",
            "@{[ $val ]}",
            "$val[0] + $val[1]",
            "$prt[0] =~ /x/ ? $val[1] : undef",
            "-$val",
            "$val ? -$val : 0",
            "length $val > 3",
            "defined $val and $val ne ''",
            "$val =~ s/(\\d+)/$1*2/ge; $val",
            "qw(a b c)",
            "my %h = (a => 1, -b => 2); $h{-b}",
            "$#val",
            "$#{$val}",
            "foreach (@val) { s/x/y/ } @val",
            "for (my $i=0; $i<3; ++$i) { $x .= $i }",
            "sub foo { return shift } foo(3)",
        ] {
            ok(s);
        }
    }

    #[test]
    fn interpolation() {
        let b = ok("\"a $val b $$self{Make} c @{[ 1+2 ]} d $val[0] e ${val}f\"");
        let Stmt::Expr(Expr::Interp(parts)) = &b[0] else { panic!() };
        assert_eq!(parts.len(), 11);
    }
}
