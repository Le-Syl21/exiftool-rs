//! The evaluator: runs the syntax tree with perl's semantics.
//!
//! Lexical variables live in scopes chained to their parent, so closures
//! keep theirs. Package variables, subs and the ExifTool runtime's ported
//! functions live in the interpreter. Every expression is evaluated in
//! scalar or list context, as perl does, and lvalues (`$x`, `$h{k}`,
//! `$a[i]`, `my (...)`) are resolved to the storage they name.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::ast::*;
use super::parse;

#[path = "builtins.rs"]
mod builtins;
#[path = "glob.rs"]
pub mod glob;
#[path = "io.rs"]
pub mod io;
use crate::perl::ops;
use crate::perl::value::Hash;
use crate::perl::Value;

/// Why evaluation stopped early.
#[derive(Debug, Clone)]
pub enum Flow {
    /// `die`, or a fatal runtime error: the value of `$@`.
    Die(Value),
    Return(Vec<Value>),
    Last(Option<Rc<str>>),
    Next(Option<Rc<str>>),
    /// `exit CODE`
    Exit(i32),
    Goto(Rc<str>),
    Redo(Option<Rc<str>>),
}

pub type ER<T> = Result<T, Flow>;

thread_local! {
    /// Where the running Perl code is: its file (the script, a required
    /// file, `(eval N)`) and the line of its current statement; no file
    /// while a native runs (its perl location, in a module's source, is not
    /// known).
    static LOCATION: RefCell<(Option<Rc<str>>, u32)> = const { RefCell::new((None, 1)) };
}

thread_local! {
    /// The handle last read (perl's PL_last_in_gv) and whether $/ made its
    /// records chunks rather than lines.
    static LAST_READ: RefCell<Option<(std::rc::Weak<RefCell<io::FileHandle>>, bool)>> = const { RefCell::new(None) };
}

thread_local! {
    /// Whether `use warnings` is in effect for the running Perl code
    /// (natives: no, as ExifTool's modules do not enable warnings).
    static WARNINGS_ON: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether a warning that perl gives only with warnings enabled is due:
/// `use warnings` in the running code's scope, or -w ($^W).
pub fn warnings_enabled(it: &mut Interp) -> bool {
    WARNINGS_ON.with(|w| w.get()) || it.get_scalar("main::^W").truthy()
}

pub(crate) fn set_last_read(h: &io::HandleRc, chunk: bool) {
    LAST_READ.with(|l| *l.borrow_mut() = Some((Rc::downgrade(h), chunk)));
}

/// perl's " at FILE line N.\n" for a die or warn without a newline, with
/// ", <FH> line M" when a handle has been read from (and not closed).
pub fn location_suffix() -> String {
    let at = LOCATION.with(|l| match &*l.borrow() {
        (Some(f), line) => format!(" at {f} line {line}"),
        (None, _) => " at (eval) line 1".to_string(),
    });
    let fh = LAST_READ.with(|l| {
        let l = l.borrow();
        let (w, chunk) = l.as_ref()?;
        let h = w.upgrade()?;
        let h = h.borrow();
        if h.lines <= 0 {
            return None;
        }
        let name = h.name.clone().unwrap_or_else(|| Rc::from("$fh"));
        Some(format!(", <{name}> {} {}", if *chunk { "chunk" } else { "line" }, h.lines))
    });
    format!("{at}{}.\n", fh.unwrap_or_default())
}

/// `pl::at`: the line of the Perl statement a native is at.
pub fn set_line(line: u32) {
    LOCATION.with(|l| l.borrow_mut().1 = line);
}

thread_local! {
    /// perl's call stack for `caller`: per sub call, the caller's package,
    /// file and line, and the sub's name.
    static CALLERS: RefCell<Vec<(Rc<str>, Option<Rc<str>>, u32, Rc<str>)>> = const { RefCell::new(Vec::new()) };
}

/// `caller(level)`: the caller's package, file, line and the sub's name.
pub fn caller_frame(level: usize) -> Option<(Rc<str>, Option<Rc<str>>, u32, Rc<str>)> {
    CALLERS.with(|c| {
        let c = c.borrow();
        c.len().checked_sub(level + 1).map(|i| c[i].clone())
    })
}

/// " at FILE line N\n" for `caller(level)`, as ExifTool's AUTOLOADs print
/// it; the current location when there is no such frame.
pub fn caller_at(level: usize) -> String {
    let (file, line) = match caller_frame(level) {
        Some((_, f, l, _)) => (f, l),
        None => location(),
    };
    format!(" at {} line {line}\n", file.as_deref().unwrap_or("-"))
}

/// " at FILE line N\n" for the current statement (a native's own
/// `caller(0)` seen from a sub it calls directly in Rust).
pub fn here_at() -> String {
    let (file, line) = location();
    format!(" at {} line {line}\n", file.as_deref().unwrap_or("-"))
}

fn push_caller(package: &Rc<str>, sub: Rc<str>) {
    let (file, line) = location();
    CALLERS.with(|c| c.borrow_mut().push((package.clone(), file, line, sub)));
}

fn pop_caller() {
    CALLERS.with(|c| {
        c.borrow_mut().pop();
    });
}

fn location() -> (Option<Rc<str>>, u32) {
    LOCATION.with(|l| l.borrow().clone())
}

fn set_location(loc: (Option<Rc<str>>, u32)) {
    LOCATION.with(|l| *l.borrow_mut() = loc);
}

pub fn die<T>(msg: impl Into<String>) -> ER<T> {
    let mut m: String = msg.into();
    if !m.ends_with('\n') {
        m.push_str(&location_suffix());
    }
    Err(Flow::Die(Value::from(m)))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Want {
    Scalar,
    List,
    Void,
}

pub type Cell = Rc<RefCell<Value>>;
pub type ArrayRc = Rc<RefCell<Vec<Value>>>;
pub type HashRc = Rc<RefCell<Hash>>;

#[derive(Clone, Debug)]
pub enum Binding {
    Scalar(Cell),
    Array(ArrayRc),
    Hash(HashRc),
}

/// A lexical scope.
#[derive(Default, Debug)]
pub struct Scope {
    vars: RefCell<Vec<(u8, Rc<str>, Binding)>>,
    parent: Option<Rc<Scope>>,
}

impl Scope {
    pub fn new(parent: Option<Rc<Scope>>) -> Rc<Scope> {
        Rc::new(Scope { vars: RefCell::new(Vec::new()), parent })
    }

    pub fn declare(&self, sigil: u8, name: &str, b: Binding) {
        self.vars.borrow_mut().push((sigil, Rc::from(name), b));
    }

    /// Bind a scalar cell (the caller keeps it to read the result back).
    pub fn scalar(&self, name: &str, v: Value) -> Cell {
        let c = Rc::new(RefCell::new(v));
        self.declare(b'$', name, Binding::Scalar(c.clone()));
        c
    }

    pub fn array(&self, name: &str, v: Vec<Value>) -> ArrayRc {
        let a = Rc::new(RefCell::new(v));
        self.declare(b'@', name, Binding::Array(a.clone()));
        a
    }

    pub fn hash(&self, name: &str, h: Hash) -> HashRc {
        let a = Rc::new(RefCell::new(h));
        self.declare(b'%', name, Binding::Hash(a.clone()));
        a
    }

    /// The variables declared directly in this scope.
    pub fn vars(&self) -> Vec<(u8, Rc<str>, Binding)> {
        self.vars.borrow().clone()
    }

    pub(crate) fn lookup(&self, sigil: u8, name: &str) -> Option<Binding> {
        let mut s = Some(self);
        while let Some(sc) = s {
            for (sg, n, b) in sc.vars.borrow().iter().rev() {
                if *sg == sigil && &**n == name {
                    return Some(b.clone());
                }
            }
            s = sc.parent.as_deref();
        }
        None
    }
}

pub type NativeFn = dyn Fn(&mut Interp, Vec<Value>, Want) -> ER<Vec<Value>>;

/// A sub: Perl code with its closure scope, or a ported Rust function.
pub enum Code {
    Perl(Rc<Sub>, Rc<Scope>),
    Native(Rc<str>, Rc<NativeFn>),
}

impl std::fmt::Debug for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Code::Perl(s, _) => write!(f, "sub {:?}", s.name),
            Code::Native(n, _) => write!(f, "native {n}"),
        }
    }
}

/// Storage an lvalue expression names.
#[derive(Clone)]
pub enum Lv {
    Scalar(Cell),
    AElem(ArrayRc, i64),
    HElem(HashRc, Rc<[u8]>),
    Array(ArrayRc),
    Hash(HashRc),
    List(Vec<Lv>),
    /// `undef` in a list assignment: skips one value.
    Skip,
    Substr(Box<Lv>, i64, Option<i64>),
    /// `$#array = N`
    LastIndex(ArrayRc),
    /// `pos($x) = N` (the interpreter keeps positions; see assign_scalar)
    Pos(Box<Lv>),
    /// A literal passed to a sub: `$_[0] = ...` on it is fatal, as in perl.
    ReadOnly(Value),
}

impl Lv {
    pub fn get(&self) -> Value {
        match self {
            Lv::Scalar(c) => c.borrow().clone(),
            Lv::AElem(a, i) => {
                let a = a.borrow();
                index(a.len(), *i).and_then(|i| a.get(i).cloned()).unwrap_or_default()
            }
            Lv::HElem(h, k) => h.borrow().get(k).cloned().unwrap_or_default(),
            Lv::Array(a) => Value::Int(a.borrow().len() as i64),
            Lv::Hash(h) => Value::Int(h.borrow().len() as i64),
            Lv::List(l) => l.last().map(|x| x.get()).unwrap_or_default(),
            Lv::Skip => Value::Undef,
            Lv::Substr(base, off, len) => {
                let s = base.get().to_bytes();
                match substr_range(s.len(), *off, *len) {
                    Some((a, b)) => Value::bytes(&s[a..b]),
                    None => Value::Undef,
                }
            }
            Lv::LastIndex(a) => Value::Int(a.borrow().len() as i64 - 1),
            Lv::Pos(_) => Value::Undef,
            Lv::ReadOnly(v) => v.clone(),
        }
    }

    pub fn set(&self, v: Value) -> ER<()> {
        match self {
            Lv::Scalar(c) => *c.borrow_mut() = v,
            Lv::AElem(a, i) => {
                let mut a = a.borrow_mut();
                let len = a.len();
                let idx = if *i < 0 {
                    match index(len, *i) {
                        Some(x) => x,
                        None => return die(format!("Modification of non-creatable array value attempted, subscript {i}")),
                    }
                } else {
                    *i as usize
                };
                if idx >= a.len() {
                    a.resize(idx + 1, Value::Undef);
                }
                a[idx] = v;
            }
            Lv::HElem(h, k) => h.borrow_mut().insert(k, v),
            Lv::Array(a) => *a.borrow_mut() = vec![v],
            Lv::Hash(_) => return die("Odd number of elements in hash assignment"),
            Lv::List(l) => {
                if let Some(first) = l.first() {
                    first.set(v)?;
                }
            }
            Lv::Skip => {}
            Lv::Substr(base, off, len) => {
                let s = base.get().to_bytes();
                let Some((a, b)) = substr_range(s.len(), *off, *len) else { return die("substr outside of string") };
                let mut n = s[..a].to_vec();
                n.extend_from_slice(&v.to_bytes());
                n.extend_from_slice(&s[b..]);
                base.set(Value::bytes(n))?;
            }
            Lv::LastIndex(a) => {
                let n = (v.to_i64() + 1).max(0) as usize;
                a.borrow_mut().resize(n, Value::Undef);
            }
            Lv::Pos(_) => {}
            Lv::ReadOnly(_) => return die("Modification of a read-only value attempted"),
        }
        Ok(())
    }
}

/// A negative index counts from the end; `None` when before the start.
fn index(len: usize, i: i64) -> Option<usize> {
    if i < 0 {
        let j = len as i64 + i;
        if j < 0 {
            None
        } else {
            Some(j as usize)
        }
    } else {
        Some(i as usize)
    }
}

/// The byte range of `substr(STR, OFF, LEN)`, `None` when outside.
pub fn substr_range(len: usize, off: i64, l: Option<i64>) -> Option<(usize, usize)> {
    let len_i = len as i64;
    let start = if off < 0 { len_i + off } else { off };
    if start > len_i || start < 0 && l.is_some_and(|l| l >= 0 && start + l < 0) {
        return None;
    }
    let start = start.max(0);
    let end = match l {
        None => len_i,
        Some(l) if l < 0 => (len_i + l).max(start),
        Some(l) => (start + l).min(len_i),
    };
    Some((start as usize, end.max(start) as usize))
}

/// The last successful match: `$1`, `$&`, `` $` ``, `$'`, `@-`, `@+`.
#[derive(Clone)]
pub struct LastMatch {
    pub subject: Rc<[u8]>,
    pub spans: Vec<Option<(usize, usize)>>,
}

/// The aliases of one call's `@_`: where each element came from, and its
/// value on entry. Changed elements are written back when the call returns.
struct AliasFrame {
    args: Option<ArrayRc>,
    aliases: Vec<Option<Lv>>,
    originals: Vec<Value>,
}

/// Whether a value is unchanged (the same string storage, number or ref).
fn identical(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Str(x), Value::Str(y)) => Rc::ptr_eq(x, y) || x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::UInt(x), Value::UInt(y)) => x == y,
        (Value::Num(x), Value::Num(y)) => x.to_bits() == y.to_bits(),
        (Value::Undef, Value::Undef) => true,
        _ => a.same_ref(b),
    }
}

/// What `local` saved, restored when the enclosing block ends.
enum Saved {
    Global(Rc<str>, Option<Binding>),
    HElem(HashRc, Rc<[u8]>, Option<Value>),
    AElem(ArrayRc, usize, Value),
    /// `local *FH`: the handle the name had (None: none).
    Handle(Rc<str>, Option<io::HandleRc>),
}

pub type GlobalHook = Rc<dyn Fn(&mut Interp, u8, &str) -> Option<Binding>>;
pub type RequireHook = Rc<dyn Fn(&mut Interp, &str) -> ER<()>>;

pub struct Interp {
    /// Package variables: sigil + fully qualified name.
    globals: HashMap<(u8, Rc<str>), Binding>,
    /// Named subs, fully qualified.
    subs: HashMap<Rc<str>, Rc<Code>>,
    /// Imports of subs not defined yet (perl's stub, which autoloads in
    /// the package of the sub it names): target -> source.
    sub_aliases: HashMap<Rc<str>, Rc<str>>,
    /// `@ISA` for method resolution, as registered by the runtime.
    pub isa: HashMap<Rc<str>, Vec<Rc<str>>>,
    cache: HashMap<Vec<u8>, Rc<Block>>,
    pub package: Rc<str>,
    pub last_match: Option<LastMatch>,
    /// `pos()` of scalars, by cell address.
    /// pos() of scalars by cell address, with the string it applies to
    /// (a new value of the variable resets it, as in perl).
    pos: HashMap<usize, (usize, PosTag)>,
    /// Inserts into `pos` since its dead entries were last dropped.
    pos_inserts: usize,
    saved: Vec<Saved>,
    want_stack: Vec<Want>,
    /// Asked for package variables not defined yet (the runtime's tables).
    pub global_hook: Option<GlobalHook>,
    /// `require Module`.
    pub require_hook: Option<RequireHook>,
    /// What `print` writes to STDOUT.
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    eval_count: usize,
    /// `use strict` (refs and subs) in effect.
    pub strict: bool,
    /// Bareword filehandles, by qualified name.
    handles: HashMap<Rc<str>, io::HandleRc>,
    /// Write STDOUT/STDERR to the process's streams instead of buffers.
    pub passthrough: bool,
    /// The name the next eval_string runs under (a file's path), instead of
    /// `(eval N)`.
    pub source_name: Option<Rc<str>>,
    /// The file each Perl sub was defined in.
    sub_files: HashMap<usize, Rc<str>>,
    /// The perl location (file path, line of its `sub`) of each native that
    /// ports one of ExifTool's subs, by the address of its name.
    native_locs: HashMap<usize, Option<(Rc<str>, u32)>>,
    /// STDOUT output waiting in perl's 8 KiB buffer (passthrough mode).
    stdout_pending: Vec<u8>,
    /// `END` blocks, run by `run_end_blocks`.
    end_blocks: Vec<(Rc<Block>, Rc<Scope>)>,
    /// Set by `exit`.
    pub exit_code: Option<i32>,
    /// The storage each argument of the call being made aliases (`@_`
    /// elements are aliases of the caller's variables in perl).
    pending_aliases: Option<Vec<Option<Lv>>>,
    alias_frames: Vec<AliasFrame>,
    /// The last stat, for `stat _` and `-X _`.
    last_stat: Vec<Value>,
    /// A goto into a nested block, while the blocks on the way are entered.
    goto_target: Option<Rc<str>>,
    /// opendir handles: the entries and the read position.
    dir_handles: HashMap<String, (Vec<Value>, usize)>,
    /// `\&name` references, by name.
    sub_refs: HashMap<Rc<str>, Rc<Code>>,
    /// PORT_TRACE: log ExifTool's sub calls, as scripts/trace_db.pl does.
    trace: Option<std::io::BufWriter<std::fs::File>>,
    trace_depth: usize,
}

impl Default for Interp {
    fn default() -> Self {
        Self::new()
    }
}

const NO: fn() -> Value = || Value::str("");
const YES: fn() -> Value = || Value::Int(1);

fn boolean(b: bool) -> Value {
    if b {
        YES()
    } else {
        NO()
    }
}

/// Names that always live in `main::`.
fn is_special_global(name: &str) -> bool {
    let Some(&first) = name.as_bytes().first() else { return true };
    !(first.is_ascii_alphabetic()) && !name.contains("::")
        || matches!(name, "ENV" | "INC" | "ARGV" | "STDIN" | "STDOUT" | "STDERR" | "_" | "ARGVOUT" | "SIG")
}

impl Interp {
    pub fn new() -> Interp {
        let mut i = Interp {
            globals: HashMap::new(),
            subs: HashMap::new(),
            sub_aliases: HashMap::new(),
            isa: HashMap::new(),
            cache: HashMap::new(),
            package: Rc::from("main"),
            last_match: None,
            pos: HashMap::new(),
            pos_inserts: 0,
            saved: Vec::new(),
            want_stack: Vec::new(),
            global_hook: None,
            require_hook: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
            eval_count: 0,
            strict: true,
            handles: HashMap::new(),
            passthrough: false,
            source_name: None,
            sub_files: HashMap::new(),
            native_locs: HashMap::new(),
            stdout_pending: Vec::new(),
            end_blocks: Vec::new(),
            exit_code: None,
            pending_aliases: None,
            alias_frames: Vec::new(),
            last_stat: Vec::new(),
            goto_target: None,
            dir_handles: HashMap::new(),
            sub_refs: HashMap::new(),
            trace: std::env::var("PORT_TRACE").ok().and_then(|p| std::fs::File::create(p).ok()).map(std::io::BufWriter::new),
            trace_depth: 0,
        };
        i.set_scalar("main::\"", Value::str(" "));
        i.set_scalar("main::/", Value::str("\n"));
        i.set_scalar("main::,", Value::Undef);
        i.set_scalar("main::;", Value::str("\x1c"));
        i.set_scalar("main::]", Value::Num(5.036));
        // $^O as perl names the system.
        let os = match std::env::consts::OS {
            "windows" => "MSWin32",
            "macos" => "darwin",
            o => o,
        };
        i.set_scalar("main::^O", Value::str(os));
        // %ENV from the process environment.
        if let Binding::Hash(h) = i.global(b'%', "main::ENV") {
            for (k, v) in std::env::vars_os() {
                h.borrow_mut().insert(k.to_string_lossy().as_bytes(), Value::str(&v.to_string_lossy()));
            }
        }
        i.set_scalar("main::0", Value::str("exiftool"));
        i.set_scalar("main::@", Value::str(""));
        i
    }

    // ---- registry ----

    /// Register a ported function under its fully qualified name.
    pub fn define_native(&mut self, name: &str, f: impl Fn(&mut Interp, Vec<Value>, Want) -> ER<Vec<Value>> + 'static) {
        let name: Rc<str> = Rc::from(name);
        self.subs.insert(name.clone(), Rc::new(Code::Native(name, Rc::new(f))));
    }

    /// `\&name`: one reference per name (so `\&f == \&f`), resolved by name
    /// at each call, so a sub ported to Rust replaces the Perl one even in
    /// references taken before.
    pub fn sub_ref(&mut self, full: &str) -> Rc<Code> {
        if let Some(c) = self.sub_refs.get(full) {
            return c.clone();
        }
        let k: Rc<str> = Rc::from(full);
        let k2 = k.clone();
        let c = Rc::new(Code::Native(k.clone(), Rc::new(move |i: &mut Interp, args, want| i.call_named(&k2, args, want))));
        self.sub_refs.insert(k, c.clone());
        c
    }

    /// The names of the defined subs that start with `prefix`.
    pub fn sub_names_with_prefix(&self, prefix: &str) -> Vec<String> {
        self.subs.keys().filter(|k| k.starts_with(prefix)).map(|k| k.to_string()).collect()
    }

    pub fn sub(&self, name: &str) -> Option<Rc<Code>> {
        self.subs.get(name).cloned()
    }

    pub fn define_sub(&mut self, name: &str, code: Rc<Code>) {
        self.subs.insert(Rc::from(name), code);
    }

    fn global_key(&self, name: &str) -> Rc<str> {
        if name.contains("::") {
            Rc::from(name.strip_prefix("::").map(|n| format!("main::{n}")).unwrap_or_else(|| name.to_string()))
        } else if is_special_global(name) {
            Rc::from(format!("main::{name}"))
        } else {
            Rc::from(format!("{}::{name}", self.package))
        }
    }

    /// A package variable, created on first use.
    pub fn global(&mut self, sigil: u8, name: &str) -> Binding {
        let key = self.global_key(name);
        if let Some(b) = self.globals.get(&(sigil, key.clone())) {
            return b.clone();
        }
        if let Some(hook) = self.global_hook.clone() {
            if let Some(b) = hook(self, sigil, &key) {
                self.globals.insert((sigil, key), b.clone());
                return b;
            }
        }
        let b = match sigil {
            b'$' => Binding::Scalar(Rc::new(RefCell::new(Value::Undef))),
            b'@' => Binding::Array(Rc::new(RefCell::new(Vec::new()))),
            _ => Binding::Hash(Rc::new(RefCell::new(Hash::default()))),
        };
        self.globals.insert((sigil, key), b.clone());
        b
    }

    /// Package variables whose qualified name starts with `prefix`.
    pub fn globals_with_prefix(&self, prefix: &str) -> Vec<(u8, String, Binding)> {
        self.globals.iter().filter(|((_, n), _)| n.starts_with(prefix)).map(|((s, n), b)| (*s, n.to_string(), b.clone())).collect()
    }

    pub fn set_global(&mut self, sigil: u8, name: &str, b: Binding) {
        let key = self.global_key(name);
        self.globals.insert((sigil, key), b);
    }

    pub fn set_scalar(&mut self, name: &str, v: Value) {
        match self.global(b'$', name) {
            Binding::Scalar(c) => *c.borrow_mut() = v,
            _ => unreachable!(),
        }
    }

    pub fn get_scalar(&mut self, name: &str) -> Value {
        match self.global(b'$', name) {
            Binding::Scalar(c) => c.borrow().clone(),
            _ => unreachable!(),
        }
    }

    fn scalar_cell(&mut self, scope: &Scope, name: &str) -> Cell {
        if let Some(Binding::Scalar(c)) = scope.lookup(b'$', name) {
            return c;
        }
        match self.global(b'$', name) {
            Binding::Scalar(c) => c,
            _ => unreachable!(),
        }
    }

    fn named_array(&mut self, scope: &Scope, name: &str) -> ArrayRc {
        if let Some(Binding::Array(a)) = scope.lookup(b'@', name) {
            return a;
        }
        match self.global(b'@', name) {
            Binding::Array(a) => a,
            _ => unreachable!(),
        }
    }

    fn named_hash(&mut self, scope: &Scope, name: &str) -> HashRc {
        if let Some(Binding::Hash(h)) = scope.lookup(b'%', name) {
            return h;
        }
        match self.global(b'%', name) {
            Binding::Hash(h) => h,
            _ => unreachable!(),
        }
    }

    // ---- entry points ----

    /// Parse (once) and run `code` in `package` with the given lexical
    /// scope, as perl's `eval STRING` at an ExifTool eval site. Errors come
    /// back as `$@` would hold them, and `$@` is set.
    pub fn eval_string(&mut self, code: &[u8], package: &str, scope: &Rc<Scope>, want: Want) -> Result<Vec<Value>, Value> {
        let saved_pkg = std::mem::replace(&mut self.package, Rc::from(package));
        // The capture variables of the code's own matches end with it.
        let saved_match = self.last_match.take();
        let saved_strict = self.strict;
        let r = self.eval_string_inner(code, scope, want);
        self.strict = saved_strict;
        self.last_match = saved_match;
        self.package = saved_pkg;
        match r {
            Ok(v) => {
                self.set_scalar("main::@", Value::str(""));
                Ok(v)
            }
            Err(e) => {
                self.set_scalar("main::@", e.clone());
                Err(e)
            }
        }
    }

    fn eval_string_inner(&mut self, code: &[u8], scope: &Rc<Scope>, want: Want) -> Result<Vec<Value>, Value> {
        let name = self.source_name.take();
        let block = match self.compile(code) {
            Ok(b) => b,
            // a syntax error in a file names the file
            Err(e) => {
                // a string eval that does not compile still takes its number
                if name.is_none() {
                    self.eval_count += 1;
                }
                return Err(match &name {
                    Some(f) => Value::from(e.to_text().replace(&format!("(eval {})", self.eval_count + 1), f)),
                    None => e,
                })
            }
        };
        // perl numbers its string evals; a file is not one
        if name.is_none() {
            self.eval_count += 1;
        }
        let saved = self.saved.len();
        self.want_stack.push(want);
        let saved_loc = location();
        // a file starts without warnings; a string eval has those of its site
        let saved_warn = WARNINGS_ON.with(|w| if name.is_some() { w.replace(false) } else { w.get() });
        set_location((Some(name.unwrap_or_else(|| Rc::from(format!("(eval {})", self.eval_count)))), 1));
        let r = self.exec_block_in(&block, scope.clone(), want);
        set_location(saved_loc);
        WARNINGS_ON.with(|w| w.set(saved_warn));
        self.want_stack.pop();
        self.restore_locals(saved);
        match r {
            Ok(v) | Err(Flow::Return(v)) => Ok(fit(v, want)),
            Err(Flow::Die(e)) => Err(e),
            Err(Flow::Exit(code)) => {
                // exit ends the program: the caller sees it in exit_code.
                self.exit_code = Some(code);
                Err(Value::str("exit\n"))
            }
            Err(Flow::Goto(l)) => Err(Value::from(format!("Can't find label {l}\n"))),
            Err(Flow::Last(_)) | Err(Flow::Next(_)) | Err(Flow::Redo(_)) => Err(Value::str("Can't \"last\" outside a loop block\n")),
        }
    }

    pub fn compile(&mut self, code: &[u8]) -> Result<Rc<Block>, Value> {
        if let Some(b) = self.cache.get(code) {
            return Ok(b.clone());
        }
        let n = self.eval_count + 1;
        let block = parse::parse_program(code, &self.package).map_err(|e| {
            if e.near_fatal {
                // perl ends a string eval's code with a newline, which shows
                // in a near text that reaches the end
                let nl = if code.ends_with(e.near.as_bytes()) { "\n" } else { "" };
                Value::from(format!("{} at (eval {n}) line {}, near \"{}{nl}\"\n", e.msg, e.line, e.near))
            } else if e.fatal {
                Value::from(format!("{} at (eval {n}) line {}.\n", e.msg, e.line))
            } else {
                // perl stops the compilation after a syntax error and says so
                let near = if e.near.trim().is_empty() { ", at EOF".to_string() } else { format!(", near \"{}\"", e.near) };
                Value::from(format!("syntax error at (eval {n}) line {}{near}\nExecution of (eval {n}) aborted due to compilation errors.\n", e.line))
            }
        })?;
        let b = Rc::new(block);
        self.cache.insert(code.to_vec(), b.clone());
        Ok(b)
    }

    /// `warn` with a message already formatted (through `$SIG{__WARN__}`).
    pub fn warn(&mut self, msg: Value) -> ER<()> {
        builtins::warn(self, msg)
    }

    /// Run the `END` blocks, last defined first.
    pub fn run_end_blocks(&mut self) -> ER<()> {
        while let Some((b, scope)) = self.end_blocks.pop() {
            self.run_block(&b, &scope, Want::Void)?;
        }
        Ok(())
    }

    /// Flush every open output handle.
    pub fn flush_handles(&mut self) {
        self.flush_stdout();
        if let Some(t) = &mut self.trace {
            use std::io::Write;
            let _ = t.flush();
        }
        for h in self.handles.values() {
            if let Some(io::Output::File(f)) = &mut h.borrow_mut().output {
                use std::io::Write;
                let _ = f.flush();
            }
        }
    }

    /// Write out what STDOUT's buffer holds.
    pub fn flush_stdout(&mut self) {
        if !self.stdout_pending.is_empty() {
            use std::io::Write;
            let mut o = std::io::stdout();
            let _ = o.write_all(&self.stdout_pending);
            let _ = o.flush();
            self.stdout_pending.clear();
        }
    }

    /// STDOUT in passthrough mode, buffered as PerlIO does it: written out
    /// each time the 8 KiB buffer fills, unless STDOUT is a terminal or `$|`
    /// is set, which write at once.
    pub(crate) fn stdout_write(&mut self, data: &[u8]) {
        use std::io::{IsTerminal, Write};
        let mut o = std::io::stdout();
        if o.is_terminal() || self.get_scalar("main::|").truthy() {
            self.flush_stdout();
            let _ = o.write_all(data);
            let _ = o.flush();
            return;
        }
        const BUF: usize = 8192;
        let mut d = data;
        while !d.is_empty() {
            let room = BUF - self.stdout_pending.len();
            let n = room.min(d.len());
            self.stdout_pending.extend_from_slice(&d[..n]);
            d = &d[n..];
            if self.stdout_pending.len() == BUF {
                self.flush_stdout();
            }
        }
    }

    /// Where perl would report a native's code: the file its sub is written
    /// in (as %INC has it) and the line of the `sub`.
    fn native_location(&mut self, name: &Rc<str>) -> Option<(Rc<str>, u32)> {
        let k = name.as_ptr() as usize;
        if let Some(l) = self.native_locs.get(&k) {
            return l.clone();
        }
        let t = crate::runtime::sub_index::SUB_INDEX;
        let loc = t.binary_search_by(|(n, _, _)| (*n).cmp(name)).ok().map(|i| {
            let (_, rel, line) = t[i];
            let path = match self.global(b'%', "main::INC") {
                Binding::Hash(h) => h.borrow().get(rel.as_bytes()).map(|v| v.to_text()),
                _ => None,
            };
            (Rc::from(path.unwrap_or_else(|| rel.to_string()).as_str()), line)
        });
        self.native_locs.insert(k, loc.clone());
        loc
    }

    /// Call a code value with arguments.
    pub fn call(&mut self, code: &Rc<Code>, args: Vec<Value>, want: Want) -> ER<Vec<Value>> {
        let aliases = self.pending_aliases.take();
        match &**code {
            Code::Native(name, f) => {
                let f = f.clone();
                let traced = self.trace.is_some() && (name.starts_with("Image::ExifTool") || name.starts_with("File::RandomAccess"));
                if traced {
                    let line = format!("{}> {} {}\n", "  ".repeat(self.trace_depth), name, args.iter().map(trace_summary).collect::<Vec<_>>().join("|"));
                    self.trace_write(&line);
                    self.trace_depth += 1;
                }
                // Always a frame of its own: set_arg in this native must never
                // reach the variables of an outer call (no aliases = no-op).
                self.alias_frames.push(AliasFrame { args: None, aliases: aliases.unwrap_or_default(), originals: Vec::new() });
                // a native that ports one of ExifTool's subs runs at that
                // sub's location (pl::at moves the line); others (perl's own
                // modules) keep their caller's, as XS and Carp report it
                let saved_loc = location();
                push_caller(&self.package, name.clone());
                if let Some((file, line)) = self.native_location(name) {
                    set_location((Some(file), line));
                }
                let saved_warn = WARNINGS_ON.with(|w| w.replace(false));
                let r = f(self, args, want).map(|v| fit(v, want));
                WARNINGS_ON.with(|w| w.set(saved_warn));
                pop_caller();
                set_location(saved_loc);
                self.alias_frames.pop();
                if traced {
                    self.trace_depth -= 1;
                    let res = match (&r, want) {
                        (Ok(v), Want::List) => v.iter().map(trace_summary).collect::<Vec<_>>().join("|"),
                        (Ok(v), Want::Scalar) => trace_summary(v.last().unwrap_or(&Value::Undef)),
                        (Ok(_), Want::Void) => "void".to_string(),
                        (Err(_), _) => "DIED".to_string(),
                    };
                    let line = format!("{}< {}\n", "  ".repeat(self.trace_depth), res);
                    self.trace_write(&line);
                }
                r
            }
            Code::Perl(sub, closure) => {
                let scope = Scope::new(Some(closure.clone()));
                let arr: ArrayRc = Rc::new(RefCell::new(args));
                let pushed = aliases.is_some();
                if let Some(al) = aliases {
                    let originals = arr.borrow().clone();
                    self.alias_frames.push(AliasFrame { args: Some(arr.clone()), aliases: al, originals });
                }
                let traced = self.trace.is_some() && sub.name.as_deref().is_some_and(|n| n.starts_with("Image::ExifTool") || n.starts_with("File::RandomAccess"));
                if traced {
                    let line = format!("{}> {} {}\n", "  ".repeat(self.trace_depth), sub.name.as_deref().unwrap(), arr.borrow().iter().map(trace_summary).collect::<Vec<_>>().join("|"));
                    self.trace_write(&line);
                    self.trace_depth += 1;
                }
                let saved_loc = location();
                push_caller(&self.package, sub.name.clone().unwrap_or_else(|| Rc::from("main::__ANON__")));
                let file = self.sub_files.get(&(Rc::as_ptr(sub) as usize)).cloned().or_else(|| saved_loc.0.clone());
                set_location((file, saved_loc.1));
                let r = self.call_perl(sub, scope, arr, want);
                pop_caller();
                set_location(saved_loc);
                if traced {
                    self.trace_depth -= 1;
                    let res = match (&r, want) {
                        (Ok(v), Want::List) => v.iter().map(trace_summary).collect::<Vec<_>>().join("|"),
                        (Ok(v), Want::Scalar) => trace_summary(v.last().unwrap_or(&Value::Undef)),
                        (Ok(_), Want::Void) => "void".to_string(),
                        (Err(_), _) => "DIED".to_string(),
                    };
                    let line = format!("{}< {}\n", "  ".repeat(self.trace_depth), res);
                    self.trace_write(&line);
                }
                if pushed {
                    let frame = self.alias_frames.pop().unwrap();
                    if let Some(arr) = &frame.args {
                        let now = arr.borrow().clone();
                        for (j, al) in frame.aliases.iter().enumerate() {
                            if let (Some(lv), Some(v), Some(orig)) = (al, now.get(j), frame.originals.get(j)) {
                                if !identical(v, orig) {
                                    lv.set(v.clone())?;
                                }
                            }
                        }
                    }
                }
                r
            }
        }
    }

    fn trace_write(&mut self, line: &str) {
        use std::io::Write;
        if let Some(t) = &mut self.trace {
            let _ = t.write_all(line.as_bytes());
        }
    }

    /// Write an argument of the native sub being run back to the caller's
    /// variable (`$_[i] = ...` in the Perl it ports).
    pub fn set_arg(&mut self, i: usize, v: Value) -> ER<()> {
        if let Some(Some(lv)) = self.alias_frames.last().and_then(|f| f.aliases.get(i)).cloned() {
            lv.set(v)?;
        }
        Ok(())
    }

    /// Keep the alias bookkeeping in step when a sub shifts or pops its @_.
    fn alias_shift(&mut self, arr: &ArrayRc, front: bool) {
        if let Some(f) = self.alias_frames.last_mut() {
            if f.args.as_ref().is_some_and(|a| Rc::ptr_eq(a, arr)) {
                if front {
                    if !f.aliases.is_empty() {
                        f.aliases.remove(0);
                    }
                    if !f.originals.is_empty() {
                        f.originals.remove(0);
                    }
                } else {
                    let n = arr.borrow().len();
                    f.aliases.truncate(n);
                    f.originals.truncate(n);
                }
            }
        }
    }

    /// Untrack an @_ that is restructured (unshift, splice, assignment).
    fn alias_forget(&mut self, arr: &ArrayRc) {
        if let Some(f) = self.alias_frames.last_mut() {
            if f.args.as_ref().is_some_and(|a| Rc::ptr_eq(a, arr)) {
                f.aliases.clear();
                f.originals.clear();
            }
        }
    }

    fn call_perl(&mut self, sub: &Rc<Sub>, scope: Rc<Scope>, arr: ArrayRc, want: Want) -> ER<Vec<Value>> {
        scope.declare(b'@', "_", Binding::Array(arr));
        let saved_pkg = std::mem::replace(&mut self.package, sub.package.clone());
        let saved_match = self.last_match.clone();
        let saved = self.saved.len();
        self.want_stack.push(want);
        let r = self.exec_block_in(&sub.body, scope, want);
        self.want_stack.pop();
        self.restore_locals(saved);
        self.last_match = saved_match;
        self.package = saved_pkg;
        match r {
            Ok(v) | Err(Flow::Return(v)) => Ok(fit(v, want)),
            Err(e) => Err(e),
        }
    }

    /// Call a code value with each argument aliased to the given storage
    /// (what perl passes for `f($x, $h{k})`): writes to `$_[i]` reach it.
    pub fn call_with_aliases(&mut self, code: &Rc<Code>, args: Vec<Value>, aliases: Vec<Option<Lv>>, want: Want) -> ER<Vec<Value>> {
        self.pending_aliases = Some(aliases);
        let r = self.call(code, args, want);
        self.pending_aliases = None;
        r
    }

    /// Aliases for the next call (see pl::method_mut).
    pub fn set_pending_aliases(&mut self, al: Vec<Option<Lv>>) {
        self.pending_aliases = Some(al);
    }

    pub fn set_pending_aliases_none(&mut self) {
        self.pending_aliases = None;
    }

    /// Call a named sub (fully qualified, or in the current package).
    pub fn call_named(&mut self, name: &str, args: Vec<Value>, want: Want) -> ER<Vec<Value>> {
        let full = self.sub_key(name);
        match self.subs.get(&*full).cloned() {
            Some(c) => self.call(&c, args, want),
            None => match self.autoload(&full) {
                Some(c) => self.call(&c, args, want),
                None => die(format!("Undefined subroutine &{full} called")),
            },
        }
    }

    fn sub_key(&self, name: &str) -> Rc<str> {
        if let Some(n) = name.strip_prefix("::") {
            Rc::from(format!("main::{n}"))
        } else if name.contains("::") {
            Rc::from(name)
        } else {
            Rc::from(format!("{}::{name}", self.package))
        }
    }

    // ---- statements ----

    fn restore_locals(&mut self, to: usize) {
        while self.saved.len() > to {
            match self.saved.pop().unwrap() {
                Saved::Global(key, b) => {
                    let k = (key.as_bytes()[0], Rc::from(&key[1..]));
                    match b {
                        Some(b) => {
                            self.globals.insert(k, b);
                        }
                        None => {
                            self.globals.remove(&k);
                        }
                    }
                }
                Saved::HElem(h, k, v) => match v {
                    Some(v) => h.borrow_mut().insert(&k, v),
                    None => {
                        h.borrow_mut().remove(&k);
                    }
                },
                Saved::AElem(a, i, v) => {
                    let mut a = a.borrow_mut();
                    if i < a.len() {
                        a[i] = v;
                    }
                }
                Saved::Handle(name, old) => {
                    // the local handle is closed when its scope ends
                    if let Some(h) = self.handles.remove(&name) {
                        let mut fh = h.borrow_mut();
                        if let Some(io::Output::File(f)) = &mut fh.output {
                            use std::io::Write;
                            let _ = f.flush();
                        }
                        fh.output = None;
                        fh.input = None;
                    }
                    if let Some(h) = old {
                        self.handles.insert(name, h);
                    }
                }
            }
        }
    }

    fn exec_block_in(&mut self, block: &Block, scope: Rc<Scope>, want: Want) -> ER<Vec<Value>> {
        // Named subs exist before the code runs, as perl compiles them first.
        for st in block {
            if let Stmt::SubDef(sub) = st {
                let code = Rc::new(Code::Perl(sub.clone(), scope.clone()));
                self.subs.insert(sub.name.clone().unwrap(), code);
                if let (Some(f), _) = location() {
                    self.sub_files.insert(Rc::as_ptr(sub) as usize, f);
                }
            }
        }
        let saved_pkg = self.package.clone();
        let saved_strict = self.strict;
        let saved_warn = WARNINGS_ON.with(|w| w.get());
        let mut last = Vec::new();
        let n = block.len();
        let r = (|| {
            let mut k = 0;
            // Entering this block through a goto into it.
            if let Some(l) = self.goto_target.clone() {
                if let Some(j) = block.iter().position(|s| contains_label(s, &l)) {
                    k = j;
                    if stmt_label(&block[j]) == Some(&*l) {
                        self.goto_target = None;
                    }
                }
            }
            while k < n {
                let st = &block[k];
                let w = if k + 1 == n { want } else { Want::Void };
                match self.exec_stmt(st, &scope, w) {
                    Ok(v) => last = v,
                    // goto LABEL: continue from the label if it is in this block.
                    Err(Flow::Goto(l)) => match block.iter().position(|s| stmt_label(s) == Some(&*l)) {
                        Some(j) => {
                            k = j;
                            continue;
                        }
                        // The label is inside a nested block of this one
                        // (perl jumps into it).
                        None => match block.iter().position(|s| contains_label(s, &l)) {
                            Some(j) => {
                                self.goto_target = Some(l);
                                k = j;
                                continue;
                            }
                            None => return Err(Flow::Goto(l)),
                        },
                    },
                    Err(e) => return Err(e),
                }
                k += 1;
            }
            Ok(std::mem::take(&mut last))
        })();
        self.package = saved_pkg;
        self.strict = saved_strict;
        WARNINGS_ON.with(|w| w.set(saved_warn));
        r
    }

    /// Run a block in a new scope (with `local` undone at its end).
    fn run_block(&mut self, block: &Block, scope: &Rc<Scope>, want: Want) -> ER<Vec<Value>> {
        let saved = self.saved.len();
        let r = self.exec_block_in(block, Scope::new(Some(scope.clone())), want);
        self.restore_locals(saved);
        r
    }

    fn exec_stmt(&mut self, st: &Stmt, scope: &Rc<Scope>, want: Want) -> ER<Vec<Value>> {
        match st {
            Stmt::Expr(e) => match want {
                Want::List => self.list(e, scope),
                Want::Scalar => Ok(vec![self.scalar(e, scope)?]),
                Want::Void => {
                    self.void(e, scope)?;
                    Ok(vec![])
                }
            },
            Stmt::If(arms, otherwise) => {
                if let Some(l) = self.goto_target.clone() {
                    // goto into one of the branches: run it without its test.
                    for (_, body) in arms.iter() {
                        if body.iter().any(|s| contains_label(s, &l)) {
                            return self.run_block(body, scope, want);
                        }
                    }
                    if let Some(b) = otherwise {
                        if b.iter().any(|s| contains_label(s, &l)) {
                            return self.run_block(b, scope, want);
                        }
                    }
                }
                for (cond, body) in arms {
                    let c = self.scalar(cond, scope)?;
                    if c.truthy() {
                        return self.run_block(body, scope, want);
                    }
                    if arms.len() == 1 && otherwise.is_none() {
                        // The value of a false `if` is its condition.
                        return Ok(vec![c]);
                    }
                }
                match otherwise {
                    Some(b) => self.run_block(b, scope, want),
                    None => Ok(vec![]),
                }
            }
            Stmt::While { label, cond, body, until, post } => {
                let mut first = true;
                loop {
                    // A `my` in the condition is visible in the body.
                    let inner = Scope::new(Some(scope.clone()));
                    if !(*post && first) {
                        let c = match cond {
                            Some(c) => self.scalar(c, &inner)?.truthy(),
                            None => true,
                        };
                        if c == *until {
                            break;
                        }
                    }
                    first = false;
                    match self.run_block(body, &inner, Want::Void) {
                        Ok(_) => {}
                        Err(Flow::Last(l)) if label_matches(&l, label) => break,
                        Err(Flow::Next(l)) if label_matches(&l, label) => continue,
                        Err(e) => return Err(e),
                    }
                }
                Ok(vec![])
            }
            Stmt::ForC { label, init, cond, step, body } => {
                let inner = Scope::new(Some(scope.clone()));
                if let Some(i) = init {
                    self.void(i, &inner)?;
                }
                loop {
                    if let Some(c) = cond {
                        if !self.scalar(c, &inner)?.truthy() {
                            break;
                        }
                    }
                    match self.run_block(body, &inner, Want::Void) {
                        Ok(_) => {}
                        Err(Flow::Last(l)) if label_matches(&l, label) => break,
                        Err(Flow::Next(l)) if label_matches(&l, label) => {}
                        Err(e) => return Err(e),
                    }
                    if let Some(s) = step {
                        self.void(s, &inner)?;
                    }
                }
                Ok(vec![])
            }
            Stmt::Foreach { label, var, list, body } => self.foreach(label, var, list, body, scope),
            Stmt::Block(label, body) => match self.run_block(body, scope, want) {
                Ok(v) => Ok(v),
                Err(Flow::Last(l)) | Err(Flow::Next(l)) if label_matches(&l, label) => Ok(vec![]),
                Err(e) => Err(e),
            },
            Stmt::SubDef(_) => Ok(vec![]),
            Stmt::Label(_) => Ok(vec![]),
            Stmt::Line(n) => {
                LOCATION.with(|l| l.borrow_mut().1 = *n);
                Ok(vec![])
            }
            Stmt::Warnings(on) => {
                WARNINGS_ON.with(|w| w.set(*on));
                Ok(vec![])
            }
            Stmt::Modified(cond, e) => {
                let c = self.scalar(cond, scope)?;
                if !c.truthy() {
                    return Ok(vec![c]);
                }
                match want {
                    Want::List => self.list(e, scope),
                    Want::Scalar => Ok(vec![self.scalar(e, scope)?]),
                    Want::Void => {
                        self.void(e, scope)?;
                        Ok(vec![])
                    }
                }
            }
            Stmt::End(b) => {
                self.end_blocks.push((b.clone(), scope.clone()));
                Ok(vec![])
            }
            Stmt::Package(name, block) => match block {
                None => {
                    self.package = name.clone();
                    Ok(vec![])
                }
                Some(b) => {
                    let saved = std::mem::replace(&mut self.package, name.clone());
                    let r = self.run_block(b, scope, want);
                    self.package = saved;
                    r
                }
            },
            Stmt::Use(name, args) => {
                if let Some(pragma) = name.strip_prefix("no ") {
                    if pragma == "strict" {
                        let what = match args {
                            Some(a) => self.list(a, scope)?.iter().map(|v| v.to_text()).collect::<Vec<_>>(),
                            None => vec![],
                        };
                        if what.is_empty() || what.iter().any(|w| w == "refs" || w == "subs") {
                            self.strict = false;
                        }
                    }
                    return Ok(vec![]);
                }
                if &**name == "strict" {
                    self.strict = true;
                    return Ok(vec![]);
                }
                match &**name {
                    "strict" | "warnings" | "integer" | "utf8" | "bytes" | "lib" | "bignum" | "overload" | "Exporter" | "vars" | "feature" => {
                        if &**name == "vars" {
                            // `use vars qw($x @y)`: the package variables exist.
                            if let Some(a) = args {
                                for v in self.list(a, scope)? {
                                    let t = v.to_text();
                                    if let Some(s) = t.chars().next() {
                                        self.global(s as u8, &t[1..]);
                                    }
                                }
                            }
                        }
                    }
                    "constant" => {
                        if let Some(a) = args {
                            let vals = self.list(a, scope)?;
                            if let Some(n) = vals.first() {
                                let n = n.to_text();
                                let rest: Vec<Value> = vals[1..].to_vec();
                                let full = format!("{}::{n}", self.package);
                                self.define_native(&full, move |_, _, _| Ok(rest.clone()));
                            }
                        }
                    }
                    other => {
                        self.require(other)?;
                        // use Module LIST: Module->import(LIST)
                        if let Some(code) = self.find_method(other, "import") {
                            let mut a = vec![Value::str(other)];
                            if let Some(x) = args {
                                a.extend(self.list(x, scope)?);
                            }
                            self.call(&code, a, Want::Void)?;
                        }
                    }
                }
                Ok(vec![])
            }
        }
    }

    fn require(&mut self, module: &str) -> ER<()> {
        match self.require_hook.clone() {
            Some(h) => h(self, module),
            None => Ok(()),
        }
    }

    fn foreach(&mut self, label: &Option<Rc<str>>, var: &Option<(bool, Rc<str>)>, list: &Expr, body: &Block, scope: &Rc<Scope>) -> ER<Vec<Value>> {
        // Each item is aliased: a cell of its own, written back to the
        // array element it came from.
        let items = self.alias_items(list, scope)?;
        let inner = Scope::new(Some(scope.clone()));
        let (global_key, saved) = match var {
            None => {
                let key = (b'$', Rc::from("main::_"));
                let old = self.globals.get(&key).cloned();
                (Some(key), old)
            }
            Some((false, name)) if scope.lookup(b'$', name).is_none() => {
                let key = (b'$', self.global_key(name));
                let old = self.globals.get(&key).cloned();
                (Some(key), old)
            }
            _ => (None, None),
        };
        let mut result = Ok(vec![]);
        for (cell, back) in items {
            match (&global_key, var) {
                (Some(k), _) => {
                    self.globals.insert(k.clone(), Binding::Scalar(cell.clone()));
                }
                (None, Some((_, name))) => inner.declare(b'$', name, Binding::Scalar(cell.clone())),
                _ => unreachable!(),
            }
            let r = self.run_block(body, &inner, Want::Void);
            if let Some(lv) = back {
                lv.set(cell.borrow().clone())?;
            }
            match r {
                Ok(_) => {}
                Err(Flow::Last(l)) if label_matches(&l, label) => break,
                Err(Flow::Next(l)) if label_matches(&l, label) => continue,
                Err(e) => {
                    result = Err(e);
                    break;
                }
            }
        }
        if let Some(k) = global_key {
            match saved {
                Some(b) => {
                    self.globals.insert(k, b);
                }
                None => {
                    self.globals.remove(&k);
                }
            }
        }
        result
    }

    /// The items of a foreach list, each with the storage it aliases.
    fn alias_items(&mut self, list: &Expr, scope: &Rc<Scope>) -> ER<Vec<(Cell, Option<Lv>)>> {
        let mut out = Vec::new();
        let exprs: Vec<&Expr> = match list {
            Expr::List(v) => v.iter().collect(),
            e => vec![e],
        };
        for e in exprs {
            match e {
                Expr::Array(c) => {
                    let a = self.array_of(c, scope, true)?;
                    let n = a.borrow().len();
                    for i in 0..n {
                        let v = a.borrow().get(i).cloned().unwrap_or_default();
                        out.push((Rc::new(RefCell::new(v)), Some(Lv::AElem(a.clone(), i as i64))));
                    }
                }
                Expr::Scalar(_) | Expr::ScalarDeref(_) => {
                    if let Lv::Scalar(c) = self.lvalue(e, scope)? {
                        out.push((c, None));
                    }
                }
                Expr::HElem(..) | Expr::Elem(..) => {
                    let lv = self.lvalue(e, scope)?;
                    out.push((Rc::new(RefCell::new(lv.get())), Some(lv)));
                }
                Expr::Slice(..) | Expr::HSlice(..) => {
                    if let Lv::List(items) = self.lvalue(e, scope)? {
                        for lv in items {
                            out.push((Rc::new(RefCell::new(lv.get())), Some(lv)));
                        }
                    }
                }
                _ => {
                    for v in self.list(e, scope)? {
                        out.push((Rc::new(RefCell::new(v)), None));
                    }
                }
            }
        }
        Ok(out)
    }

    // ---- expressions ----

    fn void(&mut self, e: &Expr, scope: &Rc<Scope>) -> ER<()> {
        match e {
            // List assignment and calls keep their list semantics.
            Expr::Assign(None, l, _) if is_list_lhs(l) => {
                self.scalar(e, scope)?;
            }
            Expr::Call { .. } | Expr::Method { .. } | Expr::CallRef(..) => {
                self.eval_call(e, scope, Want::Void)?;
            }
            Expr::Bin(BinOp::And | BinOp::Or | BinOp::DefOr, ..) | Expr::Cond(..) => {
                self.list(e, scope)?;
            }
            _ => {
                self.scalar(e, scope)?;
            }
        }
        Ok(())
    }

    /// Evaluate in list context.
    pub fn list(&mut self, e: &Expr, scope: &Rc<Scope>) -> ER<Vec<Value>> {
        match e {
            Expr::List(items) => {
                let mut out = Vec::new();
                for i in items {
                    out.extend(self.list(i, scope)?);
                }
                Ok(out)
            }
            Expr::Array(c) => {
                let a = self.array_of(c, scope, false)?;
                let v = a.borrow().clone();
                Ok(v)
            }
            Expr::Hash(c) => {
                let h = self.hash_of(c, scope, false)?;
                let h = h.borrow();
                let mut out = Vec::with_capacity(h.len() * 2);
                for (k, v) in h.iter() {
                    out.push(Value::Str(k.clone()));
                    out.push(v.clone());
                }
                Ok(out)
            }
            Expr::Slice(c, idx) => {
                let idx = self.list(idx, scope)?;
                let a = self.array_of(c, scope, false)?;
                let a = a.borrow();
                Ok(idx.iter().map(|i| index(a.len(), i.to_i64()).and_then(|i| a.get(i).cloned()).unwrap_or_default()).collect())
            }
            Expr::HSlice(c, keys) => {
                let keys = self.list(keys, scope)?;
                let h = self.hash_of(c, scope, false)?;
                let h = h.borrow();
                Ok(keys.iter().map(|k| h.get(&k.to_bytes()).cloned().unwrap_or_default()).collect())
            }
            Expr::ListSlice(l, idx) => {
                let l = self.list(l, scope)?;
                let idx = self.list(idx, scope)?;
                if l.is_empty() {
                    return Ok(vec![]);
                }
                Ok(idx.iter().map(|i| index(l.len(), i.to_i64()).and_then(|i| l.get(i).cloned()).unwrap_or_default()).collect())
            }
            Expr::My(vars, _) => {
                let lv = self.declare(vars, scope)?;
                Ok(lv_values(&lv))
            }
            Expr::Assign(None, l, r) if is_list_lhs(l) => {
                let lv = self.assign_list(l, r, scope)?;
                Ok(lv_values(&lv))
            }
            Expr::Range(a, b) => self.range(a, b, scope),
            Expr::Cond(c, a, b) => {
                if self.scalar(c, scope)?.truthy() {
                    self.list(a, scope)
                } else {
                    self.list(b, scope)
                }
            }
            Expr::Bin(BinOp::And, l, r) => {
                let lv = self.scalar(l, scope)?;
                if !lv.truthy() {
                    Ok(vec![lv])
                } else {
                    self.list(r, scope)
                }
            }
            Expr::Bin(BinOp::Or, l, r) => {
                let lv = self.scalar(l, scope)?;
                if lv.truthy() {
                    Ok(vec![lv])
                } else {
                    self.list(r, scope)
                }
            }
            Expr::Bin(BinOp::DefOr, l, r) => {
                let lv = self.scalar(l, scope)?;
                if !lv.is_undef() {
                    Ok(vec![lv])
                } else {
                    self.list(r, scope)
                }
            }
            Expr::Bin(BinOp::Repeat, l, n) if matches!(**l, Expr::List(_)) => {
                let items = self.list(l, scope)?;
                let n = self.scalar(n, scope)?.to_i64().max(0) as usize;
                let mut out = Vec::with_capacity(items.len() * n);
                for _ in 0..n {
                    out.extend(items.iter().cloned());
                }
                Ok(out)
            }
            Expr::Call { .. } | Expr::Method { .. } | Expr::CallRef(..) => self.eval_call(e, scope, Want::List),
            Expr::Match { .. } => self.do_match(e, scope, Want::List),
            Expr::ReadLine(h) => self.readline(h, scope, Want::List),
            Expr::Do(b) => self.run_block(b, scope, Want::List),
            Expr::Eval(b) => self.eval_block(b, scope, Want::List),
            Expr::EvalStr(code) => self.eval_str_expr(code, scope, Want::List),
            Expr::Map(cb, l) => self.map_grep(cb, l, scope, true),
            Expr::Grep(cb, l) => self.map_grep(cb, l, scope, false),
            Expr::Sort(cb, l) => self.sort(cb, l, scope),
            Expr::Return(v) => self.do_return(v, scope),
            Expr::Local(inner) => {
                let lv = self.localize(inner, scope)?;
                Ok(lv_values(&lv))
            }
            Expr::Un(UnOp::Ref, inner) if matches!(**inner, Expr::List(_)) => {
                let Expr::List(items) = &**inner else { unreachable!() };
                let mut out = Vec::new();
                for i in items {
                    out.push(self.make_ref(i, scope)?);
                }
                Ok(out)
            }
            _ => Ok(vec![self.scalar(e, scope)?]),
        }
    }

    fn do_return(&mut self, v: &Option<P<Expr>>, scope: &Rc<Scope>) -> ER<Vec<Value>> {
        let want = *self.want_stack.last().unwrap_or(&Want::Scalar);
        let vals = match v {
            None => vec![],
            Some(e) => match want {
                Want::List => self.list(e, scope)?,
                _ => vec![self.scalar(e, scope)?],
            },
        };
        Err(Flow::Return(vals))
    }

    /// Evaluate in scalar context.
    pub fn scalar(&mut self, e: &Expr, scope: &Rc<Scope>) -> ER<Value> {
        match e {
            Expr::Const(v) => Ok(v.clone()),
            Expr::Interp(parts) => Ok(Value::bytes(self.interpolate(parts, scope, false)?)),
            Expr::Scalar(name) => Ok(self.read_scalar(name, scope)),
            Expr::ScalarDeref(inner) => {
                let v = self.scalar(inner, scope)?;
                match v {
                    Value::ScalarRef(c) => Ok(c.borrow().clone()),
                    Value::LvRef(l) => Ok(l.get()),
                    Value::Undef => die("Can't use an undefined value as a SCALAR reference"),
                    Value::Str(s) => {
                        self.check_symref(&s, "a SCALAR")?;
                        let name = String::from_utf8_lossy(&s).into_owned();
                        Ok(self.get_scalar(&name))
                    }
                    v @ (Value::Int(_) | Value::UInt(_) | Value::Num(_)) => {
                        self.check_symref(&v.to_bytes(), "a SCALAR")?;
                        Ok(self.get_scalar(&v.to_text()))
                    }
                    _ => die("Not a SCALAR reference"),
                }
            }
            Expr::Array(c) => {
                let a = self.array_of(c, scope, false)?;
                let n = a.borrow().len();
                Ok(Value::Int(n as i64))
            }
            Expr::Hash(c) => {
                let h = self.hash_of(c, scope, false)?;
                let n = h.borrow().len();
                Ok(Value::Int(n as i64))
            }
            Expr::LastIndex(c) => {
                let a = self.array_of(c, scope, false)?;
                let n = a.borrow().len();
                Ok(Value::Int(n as i64 - 1))
            }
            Expr::Elem(c, i) => {
                let i = self.scalar(i, scope)?.to_i64();
                let a = self.array_of(c, scope, true)?;
                let a = a.borrow();
                Ok(index(a.len(), i).and_then(|i| a.get(i).cloned()).unwrap_or_default())
            }
            Expr::HElem(c, k) => {
                let k = self.hash_key(k, scope)?;
                let h = self.hash_of(c, scope, true)?;
                let v = h.borrow().get(&k).cloned().unwrap_or_default();
                Ok(v)
            }
            Expr::Slice(..) | Expr::HSlice(..) | Expr::ListSlice(..) => Ok(self.list(e, scope)?.pop().unwrap_or_default()),
            Expr::My(vars, paren) => {
                let lv = self.declare(vars, scope)?;
                if *paren {
                    Ok(Value::Int(0))
                } else {
                    Ok(lv.get())
                }
            }
            Expr::Local(inner) => Ok(self.localize(inner, scope)?.get()),
            Expr::Assign(op, l, r) => {
                if op.is_none() && is_list_lhs(l) {
                    // A list assignment in scalar context: the number of
                    // values on the right.
                    let n = self.assign_list_count(l, r, scope)?;
                    return Ok(Value::Int(n as i64));
                }
                Ok(self.assign_scalar(*op, l, r, scope)?.get())
            }
            Expr::Bin(op, l, r) => self.binary(*op, l, r, scope),
            Expr::Un(op, inner) => self.unary(*op, inner, scope),
            Expr::Cond(c, a, b) => {
                if self.scalar(c, scope)?.truthy() {
                    self.scalar(a, scope)
                } else {
                    self.scalar(b, scope)
                }
            }
            Expr::Range(..) => die("flip-flop operator is not supported"),
            Expr::List(items) => {
                // The comma operator: the last value.
                let n = items.len();
                for (k, i) in items.iter().enumerate() {
                    if k + 1 == n {
                        return self.scalar(i, scope);
                    }
                    self.void(i, scope)?;
                }
                Ok(Value::Undef)
            }
            Expr::AnonArray(items) => {
                let mut v = Vec::new();
                for i in items {
                    v.extend(self.list(i, scope)?);
                }
                Ok(Value::ArrayRef(Rc::new(RefCell::new(v))))
            }
            Expr::AnonHash(items) => {
                let mut v = Vec::new();
                for i in items {
                    v.extend(self.list(i, scope)?);
                }
                Ok(Value::HashRef(Rc::new(RefCell::new(pairs_to_hash(v)))))
            }
            Expr::AnonSub(sub) => {
                if let (Some(f), _) = location() {
                    self.sub_files.entry(Rc::as_ptr(sub) as usize).or_insert(f);
                }
                Ok(Value::Code(Rc::new(Code::Perl(sub.clone(), scope.clone()))))
            }
            Expr::Call { .. } | Expr::Method { .. } | Expr::CallRef(..) => Ok(self.eval_call(e, scope, Want::Scalar)?.pop().unwrap_or_default()),
            Expr::Match { .. } => Ok(self.do_match(e, scope, Want::Scalar)?.pop().unwrap_or_default()),
            Expr::Subst { target, pat, repl } => self.subst(target, pat, repl, scope),
            Expr::Trans { target, search, repl, flags } => {
                let lv = match target {
                    Some(t) => self.lvalue(t, scope)?,
                    None => Lv::Scalar(self.scalar_cell(scope, "_")),
                };
                let s = lv.get().to_bytes();
                let (out, count) = crate::perl::tr::tr(&s, search, repl, flags);
                if flags.contains('r') {
                    return Ok(Value::bytes(out));
                }
                if out[..] != s[..] {
                    lv.set(Value::bytes(out))?;
                }
                Ok(Value::Int(count as i64))
            }
            Expr::Qr(pat) => {
                let p = self.pattern_bytes(pat, scope)?;
                let flags: String = pat.flags.chars().filter(|c| "imsx".contains(*c)).collect();
                let mut s = format!("(?^{flags}:").into_bytes();
                s.extend_from_slice(&p);
                s.push(b')');
                Ok(Value::bytes(s))
            }
            Expr::Do(b) => Ok(self.run_block(b, scope, Want::Scalar)?.pop().unwrap_or_default()),
            Expr::Eval(b) => Ok(self.eval_block(b, scope, Want::Scalar)?.pop().unwrap_or_default()),
            Expr::EvalStr(code) => Ok(self.eval_str_expr(code, scope, Want::Scalar)?.pop().unwrap_or_default()),
            Expr::Map(..) | Expr::Grep(..) => Ok(Value::Int(self.list(e, scope)?.len() as i64)),
            // sort in scalar context: perl returns undef (the list is
            // still sorted, for any side effects of the comparison).
            Expr::Sort(..) => {
                self.list(e, scope)?;
                Ok(Value::Undef)
            }
            Expr::FileTest(op, f) => {
                let m = match io::file_target(self, f, scope)? {
                    io::Target::Last => {
                        if self.last_stat.is_empty() {
                            Err(std::io::Error::other("no stat"))
                        } else {
                            // Re-read through the mode and size of the last stat.
                            let mode = self.last_stat[2].to_i64();
                            let size = self.last_stat[7].to_i64();
                            return Ok(match op {
                                b'e' => YES(),
                                b'f' => boolean(mode & 0o170000 == 0o100000),
                                b'd' => boolean(mode & 0o170000 == 0o040000),
                                b'p' => boolean(mode & 0o170000 == 0o010000),
                                b's' => if size > 0 { Value::Int(size) } else { Value::Undef },
                                b'z' => boolean(size == 0),
                                _ => YES(),
                            });
                        }
                    }
                    io::Target::Path(p) => std::fs::metadata(&p),
                };
                if let Ok(md) = &m {
                    self.last_stat = io::stat_list(md);
                }
                Ok(match op {
                    b'e' => boolean(m.is_ok()),
                    b'f' => boolean(m.map(|m| m.is_file()).unwrap_or(false)),
                    b'd' => boolean(m.map(|m| m.is_dir()).unwrap_or(false)),
                    b's' => match m {
                        Ok(m) if m.len() > 0 => Value::Int(m.len() as i64),
                        _ => Value::Undef,
                    },
                    b'z' => boolean(m.map(|m| m.len() == 0).unwrap_or(false)),
                    b'p' => {
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::FileTypeExt;
                            boolean(m.map(|m| m.file_type().is_fifo()).unwrap_or(false))
                        }
                        #[cfg(not(unix))]
                        {
                            boolean(false)
                        }
                    }
                    b'r' | b'w' | b'x' => boolean(m.is_ok()),
                    _ => boolean(m.is_ok()),
                })
            }
            Expr::Return(v) => {
                self.do_return(v, scope)?;
                unreachable!()
            }
            Expr::Last(l) => Err(Flow::Last(l.clone())),
            Expr::Next(l) => Err(Flow::Next(l.clone())),
            Expr::Redo(l) => Err(Flow::Redo(l.clone())),
            Expr::Goto(l) => Err(Flow::Goto(l.clone())),
            Expr::ReadLine(h) => Ok(self.readline(h, scope, Want::Scalar)?.pop().unwrap_or_default()),
            Expr::Wantarray => Ok(match self.want_stack.last() {
                Some(Want::List) => Value::Int(1),
                Some(Want::Scalar) => Value::Int(0),
                _ => Value::Undef,
            }),
            Expr::Print { printf, fh, args } => {
                let mut vals = Vec::new();
                for a in args {
                    vals.extend(self.list(a, scope)?);
                }
                if vals.is_empty() && !printf {
                    vals.push(self.read_scalar("_", scope));
                }
                let out = if *printf {
                    let fmt = vals.first().map(|v| v.to_bytes()).unwrap_or_else(|| Rc::from(&b""[..]));
                    crate::perl::sprintf::sprintf(&fmt, &vals[1.min(vals.len())..])
                } else {
                    let sep = self.get_scalar("main::,").to_bytes();
                    let mut o = Vec::new();
                    for (k, v) in vals.iter().enumerate() {
                        if k > 0 {
                            o.extend_from_slice(&sep);
                        }
                        o.extend_from_slice(&v.to_bytes());
                    }
                    o
                };
                let mut out = out;
                if !*printf {
                    out.extend_from_slice(&self.get_scalar("main::\\").to_bytes());
                }
                let h = match fh {
                    Some(e) => {
                        let v = self.scalar(e, scope)?;
                        self.handle_of(&v, false)
                    }
                    None => self.handle_of(&Value::str("*STDOUT"), false),
                };
                Ok(Value::from(match h {
                    Some(h) => self.write_handle(&h, &out),
                    None => false,
                }))
            }
        }
    }

    /// pos() by key (see pos_key), checked against the current value.
    pub(crate) fn pos_at(&self, key: usize, lv: &Lv) -> Option<usize> {
        let (p, tag) = self.pos.get(&key)?;
        tag.matches(&lv.get()).then_some(*p)
    }

    /// Set a pos() (dropping now and then the entries whose string is gone,
    /// as their weak handles keep its memory).
    fn pos_set(&mut self, key: usize, p: usize, tag: PosTag) {
        self.pos.insert(key, (p, tag));
        self.pos_inserts += 1;
        if self.pos_inserts >= 4096 {
            self.pos_inserts = 0;
            self.pos.retain(|_, (_, t)| t.alive());
        }
    }

    /// Under `strict refs`, a string used as a reference is fatal.
    fn check_symref(&self, s: &[u8], what: &str) -> ER<()> {
        if !self.strict {
            return Ok(());
        }
        let mut m = b"Can't use string (\"".to_vec();
        m.extend_from_slice(&s[..s.len().min(32)]);
        m.push(b'"');
        if s.len() > 32 {
            m.extend_from_slice(b"...");
        }
        m.extend_from_slice(format!(") as {what} ref while \"strict refs\" in use{}", location_suffix()).as_bytes());
        Err(Flow::Die(Value::bytes(m)))
    }

    fn read_scalar(&mut self, name: &str, scope: &Scope) -> Value {
        let b = name.as_bytes();
        if b[0].is_ascii_digit() {
            let n: usize = name.parse().unwrap_or(0);
            if n == 0 {
                return self.get_scalar("main::0");
            }
            return self.capture(n);
        }
        match name {
            "&" => return self.capture(0),
            "`" | "'" => {
                let Some(m) = &self.last_match else { return Value::Undef };
                let (s, e) = m.spans[0].unwrap();
                return if name == "`" { Value::bytes(&m.subject[..s]) } else { Value::bytes(&m.subject[e..]) };
            }
            "+" => {
                let Some(m) = &self.last_match else { return Value::Undef };
                for k in (1..m.spans.len()).rev() {
                    if let Some((s, e)) = m.spans[k] {
                        return Value::bytes(&m.subject[s..e]);
                    }
                }
                return Value::Undef;
            }
            _ => {}
        }
        self.scalar_cell(scope, name).borrow().clone()
    }

    fn capture(&self, n: usize) -> Value {
        match &self.last_match {
            Some(m) => match m.spans.get(n).copied().flatten() {
                Some((s, e)) => Value::bytes(&m.subject[s..e]),
                None => Value::Undef,
            },
            None => Value::Undef,
        }
    }

    fn hash_key(&mut self, k: &Expr, scope: &Rc<Scope>) -> ER<Rc<[u8]>> {
        // $h{a,b} joins with $;
        if let Expr::List(items) = k {
            if items.len() > 1 {
                let sep = self.get_scalar("main::;").to_bytes();
                let mut out = Vec::new();
                for (n, i) in items.iter().enumerate() {
                    if n > 0 {
                        out.extend_from_slice(&sep);
                    }
                    out.extend_from_slice(&self.scalar(i, scope)?.to_bytes());
                }
                return Ok(Rc::from(out));
            }
        }
        Ok(self.scalar(k, scope)?.to_bytes())
    }

    // ---- containers ----

    /// The array a container names. `autoviv`: create it through an
    /// undefined reference (element access, lvalues).
    pub fn array_of(&mut self, c: &Container, scope: &Rc<Scope>, autoviv: bool) -> ER<ArrayRc> {
        match c {
            Container::Named(n) => Ok(self.named_array(scope, n)),
            Container::Deref(e) => {
                let v = if autoviv && is_lvalue_like(e) {
                    let lv = self.lvalue(e, scope)?;
                    let v = lv.get();
                    if v.is_undef() {
                        let a: ArrayRc = Rc::new(RefCell::new(Vec::new()));
                        lv.set(Value::ArrayRef(a.clone()))?;
                        return Ok(a);
                    }
                    v
                } else {
                    self.scalar(e, scope)?
                };
                match v {
                    Value::ArrayRef(a) => Ok(a),
                    Value::Undef if autoviv => Ok(Rc::new(RefCell::new(Vec::new()))),
                    Value::Undef => die("Can't use an undefined value as an ARRAY reference"),
                    Value::Str(s) => {
                        self.check_symref(&s, "an ARRAY")?;
                        let name = String::from_utf8_lossy(&s).into_owned();
                        match self.global(b'@', &name) {
                            Binding::Array(a) => Ok(a),
                            _ => unreachable!(),
                        }
                    }
                    v @ (Value::Int(_) | Value::UInt(_) | Value::Num(_)) => {
                        self.check_symref(&v.to_bytes(), "an ARRAY")?;
                        match self.global(b'@', &v.to_text()) {
                            Binding::Array(a) => Ok(a),
                            _ => unreachable!(),
                        }
                    }
                    _ => die("Not an ARRAY reference"),
                }
            }
        }
    }

    pub fn hash_of(&mut self, c: &Container, scope: &Rc<Scope>, autoviv: bool) -> ER<HashRc> {
        match c {
            Container::Named(n) => Ok(self.named_hash(scope, n)),
            Container::Deref(e) => {
                let v = if autoviv && is_lvalue_like(e) {
                    let lv = self.lvalue(e, scope)?;
                    let v = lv.get();
                    if v.is_undef() {
                        let h: HashRc = Rc::new(RefCell::new(Hash::default()));
                        lv.set(Value::HashRef(h.clone()))?;
                        return Ok(h);
                    }
                    v
                } else {
                    self.scalar(e, scope)?
                };
                match v {
                    Value::HashRef(h) => Ok(h),
                    Value::Undef if autoviv => Ok(Rc::new(RefCell::new(Hash::default()))),
                    Value::Undef => die("Can't use an undefined value as a HASH reference"),
                    Value::Str(s) => {
                        self.check_symref(&s, "a HASH")?;
                        let name = String::from_utf8_lossy(&s).into_owned();
                        match self.global(b'%', &name) {
                            Binding::Hash(h) => Ok(h),
                            _ => unreachable!(),
                        }
                    }
                    v @ (Value::Int(_) | Value::UInt(_) | Value::Num(_)) => {
                        self.check_symref(&v.to_bytes(), "a HASH")?;
                        match self.global(b'%', &v.to_text()) {
                            Binding::Hash(h) => Ok(h),
                            _ => unreachable!(),
                        }
                    }
                    _ => die("Not a HASH reference"),
                }
            }
        }
    }

    // ---- lvalues and assignment ----

    pub fn lvalue(&mut self, e: &Expr, scope: &Rc<Scope>) -> ER<Lv> {
        Ok(match e {
            // $1, $&...: read-only values of the last match.
            Expr::Scalar(name) if is_match_var(name) => Lv::Scalar(Rc::new(RefCell::new(self.read_scalar(name, scope)))),
            Expr::Scalar(name) => Lv::Scalar(self.scalar_cell(scope, name)),
            Expr::ScalarDeref(inner) => {
                let v = if is_lvalue_like(inner) {
                    let lv = self.lvalue(inner, scope)?;
                    let v = lv.get();
                    if v.is_undef() {
                        let c: Cell = Rc::new(RefCell::new(Value::Undef));
                        lv.set(Value::ScalarRef(c.clone()))?;
                        return Ok(Lv::Scalar(c));
                    }
                    v
                } else {
                    self.scalar(inner, scope)?
                };
                match v {
                    Value::ScalarRef(c) => Lv::Scalar(c),
                    Value::LvRef(l) => (*l).clone(),
                    Value::Str(s) => {
                        self.check_symref(&s, "a SCALAR")?;
                        let name = String::from_utf8_lossy(&s).into_owned();
                        match self.global(b'$', &name) {
                            Binding::Scalar(c) => Lv::Scalar(c),
                            _ => unreachable!(),
                        }
                    }
                    _ => return die("Not a SCALAR reference"),
                }
            }
            Expr::Elem(c, i) => {
                let i = self.scalar(i, scope)?.to_i64();
                let a = self.array_of(c, scope, true)?;
                // an element of @_ aliased to a literal is read-only
                if let Some(f) = self.alias_frames.last() {
                    if f.args.as_ref().is_some_and(|x| Rc::ptr_eq(x, &a)) {
                        let len = a.borrow().len();
                        if let Some(idx) = index(len, i) {
                            if let Some(Some(Lv::ReadOnly(v))) = f.aliases.get(idx) {
                                return Ok(Lv::ReadOnly(v.clone()));
                            }
                        }
                    }
                }
                Lv::AElem(a, i)
            }
            Expr::HElem(c, k) => {
                let k = self.hash_key(k, scope)?;
                Lv::HElem(self.hash_of(c, scope, true)?, k)
            }
            Expr::Array(c) => Lv::Array(self.array_of(c, scope, true)?),
            Expr::Hash(c) => Lv::Hash(self.hash_of(c, scope, true)?),
            Expr::LastIndex(c) => Lv::LastIndex(self.array_of(c, scope, true)?),
            Expr::My(vars, _) => self.declare(vars, scope)?,
            Expr::Local(inner) => self.localize(inner, scope)?,
            Expr::List(items) => {
                let mut v = Vec::new();
                for i in items {
                    v.push(match i {
                        Expr::Const(Value::Undef) => Lv::Skip,
                        Expr::Call { name, args, .. } if &**name == "undef" && args.is_empty() => Lv::Skip,
                        i => self.lvalue(i, scope)?,
                    });
                }
                Lv::List(v)
            }
            Expr::Slice(c, idx) => {
                let idx = self.list(idx, scope)?;
                let a = self.array_of(c, scope, true)?;
                Lv::List(idx.iter().map(|i| Lv::AElem(a.clone(), i.to_i64())).collect())
            }
            Expr::HSlice(c, keys) => {
                let keys = self.list(keys, scope)?;
                let h = self.hash_of(c, scope, true)?;
                Lv::List(keys.iter().map(|k| Lv::HElem(h.clone(), k.to_bytes())).collect())
            }
            Expr::Cond(c, a, b) => {
                if self.scalar(c, scope)?.truthy() {
                    self.lvalue(a, scope)?
                } else {
                    self.lvalue(b, scope)?
                }
            }
            Expr::Assign(op, l, r) => {
                if op.is_none() && is_list_lhs(l) {
                    self.assign_list(l, r, scope)?
                } else {
                    self.assign_scalar(*op, l, r, scope)?
                }
            }
            Expr::Un(UnOp::PreInc | UnOp::PreDec, inner) => {
                self.scalar(e, scope)?;
                self.lvalue(inner, scope)?
            }
            Expr::Call { name, args, .. } if &**name == "pos" => {
                let lv = match args.first() {
                    Some(e) => self.lvalue(e, scope)?,
                    None => Lv::Scalar(self.scalar_cell(scope, "_")),
                };
                Lv::Pos(Box::new(lv))
            }
            Expr::Call { name, args, .. } if &**name == "substr" && (2..=3).contains(&args.len()) => {
                let base = self.lvalue(&args[0], scope)?;
                let off = self.scalar(&args[1], scope)?.to_i64();
                let len = match args.get(2) {
                    Some(l) => Some(self.scalar(l, scope)?.to_i64()),
                    None => None,
                };
                Lv::Substr(Box::new(base), off, len)
            }
            // A temporary (the target of a match on an rvalue).
            e => Lv::Scalar(Rc::new(RefCell::new(self.scalar(e, scope)?))),
        })
    }

    fn declare(&mut self, vars: &[Expr], scope: &Rc<Scope>) -> ER<Lv> {
        let mut out = Vec::new();
        for v in vars {
            out.push(match v {
                Expr::Scalar(n) => {
                    let c: Cell = Rc::new(RefCell::new(Value::Undef));
                    scope.declare(b'$', n, Binding::Scalar(c.clone()));
                    Lv::Scalar(c)
                }
                Expr::Array(Container::Named(n)) => {
                    let a: ArrayRc = Rc::new(RefCell::new(Vec::new()));
                    scope.declare(b'@', n, Binding::Array(a.clone()));
                    Lv::Array(a)
                }
                Expr::Hash(Container::Named(n)) => {
                    let h: HashRc = Rc::new(RefCell::new(Hash::default()));
                    scope.declare(b'%', n, Binding::Hash(h.clone()));
                    Lv::Hash(h)
                }
                _ => Lv::Skip,
            });
        }
        Ok(if out.len() == 1 && !matches!(vars.first(), None) { out.pop().unwrap() } else { Lv::List(out) })
    }

    /// `local`: save the current value, restored when the block ends.
    fn localize(&mut self, e: &Expr, scope: &Rc<Scope>) -> ER<Lv> {
        match e {
            Expr::Scalar(n) | Expr::Array(Container::Named(n)) | Expr::Hash(Container::Named(n)) => {
                let sigil = match e {
                    Expr::Scalar(_) => b'$',
                    Expr::Array(_) => b'@',
                    _ => b'%',
                };
                let key = self.global_key(n);
                let old = self.globals.get(&(sigil, key.clone())).cloned();
                let mut k = String::with_capacity(key.len() + 1);
                k.push(sigil as char);
                k.push_str(&key);
                self.saved.push(Saved::Global(Rc::from(k), old));
                let b = match sigil {
                    b'$' => Binding::Scalar(Rc::new(RefCell::new(Value::Undef))),
                    b'@' => Binding::Array(Rc::new(RefCell::new(Vec::new()))),
                    _ => Binding::Hash(Rc::new(RefCell::new(Hash::default()))),
                };
                self.globals.insert((sigil, key), b.clone());
                Ok(match b {
                    Binding::Scalar(c) => Lv::Scalar(c),
                    Binding::Array(a) => Lv::Array(a),
                    Binding::Hash(h) => Lv::Hash(h),
                })
            }
            Expr::HElem(c, k) => {
                let k = self.hash_key(k, scope)?;
                let h = self.hash_of(c, scope, true)?;
                let old = h.borrow().get(&k).cloned();
                self.saved.push(Saved::HElem(h.clone(), k.clone(), old));
                h.borrow_mut().remove(&k);
                Ok(Lv::HElem(h, k))
            }
            Expr::Elem(c, i) => {
                let i = self.scalar(i, scope)?.to_i64();
                let a = self.array_of(c, scope, true)?;
                let len = a.borrow().len();
                if let Some(idx) = index(len, i) {
                    let old = a.borrow().get(idx).cloned().unwrap_or_default();
                    self.saved.push(Saved::AElem(a.clone(), idx, old));
                }
                Ok(Lv::AElem(a, i))
            }
            Expr::List(items) => {
                let mut v = Vec::new();
                for i in items {
                    v.push(self.localize(i, scope)?);
                }
                Ok(Lv::List(v))
            }
            Expr::Assign(..) => self.lvalue(e, scope),
            // local *FH: a new, closed handle under the name until the
            // scope ends, then the old one back
            Expr::Const(Value::Str(g)) if g.starts_with(b"*") => {
                let name = String::from_utf8_lossy(&g[1..]).into_owned();
                let name = name.strip_prefix("main::").unwrap_or(&name).to_string();
                if !matches!(name.as_str(), "STDOUT" | "STDERR" | "STDIN") {
                    let key: Rc<str> = if name.contains("::") { Rc::from(name.as_str()) } else { Rc::from(format!("{}::{name}", self.package)) };
                    let old = self.handles.remove(&key);
                    self.saved.push(Saved::Handle(key, old));
                }
                Ok(Lv::Skip)
            }
            Expr::Const(_) => Ok(Lv::Skip),
            _ => die("Can't localize lexical variable"),
        }
    }

    fn assign_scalar(&mut self, op: Option<BinOp>, l: &Expr, r: &Expr, scope: &Rc<Scope>) -> ER<Lv> {
        match op {
            None => {
                // `@a = 5`-like cases on a non-list lhs are handled by
                // is_list_lhs; here the lhs is a scalar.
                let v = self.scalar(r, scope)?;
                let lv = self.lvalue(l, scope)?;
                if let Lv::Pos(target) = &lv {
                    let Some(key) = pos_key(target) else { return Ok(lv) };
                    if v.is_undef() {
                        self.pos.remove(&key);
                    } else {
                        let cur = target.get();
                        let len = cur.to_bytes().len() as i64;
                        let mut p = v.to_i64();
                        if p < 0 {
                            p += len;
                        }
                        let p = p.clamp(0, len);
                        self.pos_set(key, p as usize, value_tag(&cur));
                        // The assignment's value is the position set
                        // (`$pos = pos($$b) = 0` gives 0).
                        return Ok(Lv::Scalar(Rc::new(RefCell::new(Value::Int(p)))));
                    }
                    return Ok(lv);
                }
                self.forget_pos(&lv);
                lv.set(v)?;
                Ok(lv)
            }
            Some(op) => {
                if let (BinOp::Repeat | BinOp::Concat, Expr::Array(_)) = (op, l) {
                    return die("unsupported array compound assignment");
                }
                if let Expr::Array(c) = l {
                    // push-like: @a = (@a, ...) is not this; `@a x= n` rare.
                    let _ = c;
                }
                let lv = self.lvalue(l, scope)?;
                let old = lv.get();
                let new = match op {
                    BinOp::Or => {
                        if old.truthy() {
                            return Ok(lv);
                        }
                        self.scalar(r, scope)?
                    }
                    BinOp::And => {
                        if !old.truthy() {
                            return Ok(lv);
                        }
                        self.scalar(r, scope)?
                    }
                    BinOp::DefOr => {
                        if !old.is_undef() {
                            return Ok(lv);
                        }
                        self.scalar(r, scope)?
                    }
                    op => {
                        let rv = self.scalar(r, scope)?;
                        self.apply_bin(op, &old, &rv)?
                    }
                };
                self.forget_pos(&lv);
                lv.set(new)?;
                Ok(lv)
            }
        }
    }

    fn forget_pos(&mut self, lv: &Lv) {
        if !self.pos.is_empty() {
            if let Lv::Scalar(c) = lv {
                self.pos.remove(&(Rc::as_ptr(c) as usize));
            }
        }
    }

    fn assign_list_count(&mut self, l: &Expr, r: &Expr, scope: &Rc<Scope>) -> ER<usize> {
        let vals = self.list(r, scope)?;
        let n = vals.len();
        let lv = self.lvalue(l, scope)?;
        assign_values(&lv, vals)?;
        Ok(n)
    }

    fn assign_list(&mut self, l: &Expr, r: &Expr, scope: &Rc<Scope>) -> ER<Lv> {
        let vals = self.list(r, scope)?;
        let lv = self.lvalue(l, scope)?;
        if let Lv::Array(a) = &lv {
            self.alias_forget(a);
        }
        assign_values(&lv, vals)?;
        Ok(lv)
    }

    // ---- operators ----

    fn binary(&mut self, op: BinOp, l: &Expr, r: &Expr, scope: &Rc<Scope>) -> ER<Value> {
        match op {
            BinOp::And => {
                let a = self.scalar(l, scope)?;
                if !a.truthy() {
                    return Ok(a);
                }
                self.scalar(r, scope)
            }
            BinOp::Or => {
                let a = self.scalar(l, scope)?;
                if a.truthy() {
                    return Ok(a);
                }
                self.scalar(r, scope)
            }
            BinOp::DefOr => {
                let a = self.scalar(l, scope)?;
                if !a.is_undef() {
                    return Ok(a);
                }
                self.scalar(r, scope)
            }
            _ => {
                let a = self.scalar(l, scope)?;
                let b = self.scalar(r, scope)?;
                self.apply_bin(op, &a, &b)
            }
        }
    }

    pub fn apply_bin(&mut self, op: BinOp, a: &Value, b: &Value) -> ER<Value> {
        use std::cmp::Ordering::*;
        Ok(match op {
            BinOp::Add => ops::add(a, b),
            BinOp::Sub => ops::sub(a, b),
            BinOp::Mul => ops::mul(a, b),
            BinOp::Div => match ops::div(a, b) {
                Ok(v) => v,
                Err(e) => return die(e),
            },
            BinOp::Mod => match ops::modulo(a, b) {
                Ok(v) => v,
                Err(e) => return die(e),
            },
            BinOp::Pow => ops::pow(a, b),
            BinOp::Concat => {
                let mut s = a.to_bytes().to_vec();
                s.extend_from_slice(&b.to_bytes());
                Value::bytes(s)
            }
            BinOp::Repeat => {
                let n = b.to_i64().max(0) as usize;
                Value::bytes(a.to_bytes().repeat(n))
            }
            BinOp::NumEq => boolean(ops::num_cmp(a, b) == Some(Equal)),
            BinOp::NumNe => boolean(ops::num_cmp(a, b) != Some(Equal)),
            BinOp::NumLt => boolean(ops::num_cmp(a, b) == Some(Less)),
            BinOp::NumGt => boolean(ops::num_cmp(a, b) == Some(Greater)),
            BinOp::NumLe => boolean(matches!(ops::num_cmp(a, b), Some(Less | Equal))),
            BinOp::NumGe => boolean(matches!(ops::num_cmp(a, b), Some(Greater | Equal))),
            BinOp::NumCmp => match ops::num_cmp(a, b) {
                Some(o) => Value::Int(o as i64),
                None => Value::Undef,
            },
            BinOp::StrEq => boolean(ops::str_cmp(a, b) == Equal),
            BinOp::StrNe => boolean(ops::str_cmp(a, b) != Equal),
            BinOp::StrLt => boolean(ops::str_cmp(a, b) == Less),
            BinOp::StrGt => boolean(ops::str_cmp(a, b) == Greater),
            BinOp::StrLe => boolean(ops::str_cmp(a, b) != Greater),
            BinOp::StrGe => boolean(ops::str_cmp(a, b) != Less),
            BinOp::StrCmp => Value::Int(ops::str_cmp(a, b) as i64),
            BinOp::BitAnd => {
                if is_string_operand(a) && is_string_operand(b) {
                    let (x, y) = (a.to_bytes(), b.to_bytes());
                    Value::bytes(x.iter().zip(y.iter()).map(|(p, q)| p & q).collect::<Vec<u8>>())
                } else {
                    ops::bit_and(a, b)
                }
            }
            BinOp::BitOr => {
                if is_string_operand(a) && is_string_operand(b) {
                    Value::bytes(string_bitop(&a.to_bytes(), &b.to_bytes(), |p, q| p | q))
                } else {
                    ops::bit_or(a, b)
                }
            }
            BinOp::BitXor => {
                if is_string_operand(a) && is_string_operand(b) {
                    Value::bytes(string_bitop(&a.to_bytes(), &b.to_bytes(), |p, q| p ^ q))
                } else {
                    ops::bit_xor(a, b)
                }
            }
            BinOp::Shl => ops::shift_left(a, b),
            BinOp::Shr => ops::shift_right(a, b),
            BinOp::Xor => boolean(a.truthy() != b.truthy()),
            BinOp::And | BinOp::Or | BinOp::DefOr => unreachable!(),
        })
    }

    fn unary(&mut self, op: UnOp, e: &Expr, scope: &Rc<Scope>) -> ER<Value> {
        Ok(match op {
            UnOp::Not => boolean(!self.scalar(e, scope)?.truthy()),
            UnOp::Neg => ops::negate(&self.scalar(e, scope)?),
            UnOp::Plus => self.scalar(e, scope)?,
            UnOp::BitNot => {
                let v = self.scalar(e, scope)?;
                if is_string_operand(&v) {
                    Value::bytes(v.to_bytes().iter().map(|b| !b).collect::<Vec<u8>>())
                } else {
                    ops::bit_not(&v)
                }
            }
            UnOp::Ref => self.make_ref(e, scope)?,
            UnOp::PreInc | UnOp::PreDec | UnOp::PostInc | UnOp::PostDec => {
                let lv = self.lvalue(e, scope)?;
                let old = lv.get();
                let new = match op {
                    UnOp::PreInc | UnOp::PostInc => string_increment(&old).unwrap_or_else(|| ops::add(&old, &Value::Int(1))),
                    _ => ops::sub(&old, &Value::Int(1)),
                };
                lv.set(new.clone())?;
                match op {
                    UnOp::PreInc | UnOp::PreDec => new,
                    // $x++ on undef returns 0.
                    _ if old.is_undef() && op == UnOp::PostInc => Value::Int(0),
                    _ => old,
                }
            }
        })
    }

    fn make_ref(&mut self, e: &Expr, scope: &Rc<Scope>) -> ER<Value> {
        Ok(match e {
            Expr::Scalar(_) | Expr::ScalarDeref(_) | Expr::My(..) | Expr::Elem(..) | Expr::HElem(..) => match self.lvalue(e, scope)? {
                Lv::Scalar(c) => Value::ScalarRef(c),
                lv @ (Lv::AElem(..) | Lv::HElem(..)) => Value::LvRef(Rc::new(lv)),
                lv => Value::ScalarRef(Rc::new(RefCell::new(lv.get()))),
            },
            Expr::Array(c) => Value::ArrayRef(self.array_of(c, scope, true)?),
            Expr::Hash(c) => Value::HashRef(self.hash_of(c, scope, true)?),
            Expr::Call { name, amp: true, args } if args.len() == 1 && matches!(&args[0], Expr::Array(Container::Named(n)) if &**n == "_") => {
                let key = self.sub_key(name);
                Value::Code(self.sub_ref(&key))
            }
            Expr::AnonSub(_) => self.scalar(e, scope)?,
            // \*FH: a GLOB reference, the handle itself.
            Expr::Const(Value::Str(s)) if s.starts_with(b"*") => match self.handle_of(&Value::Str(s.clone()), true) {
                Some(h) => Value::Handle(h),
                None => Value::Undef,
            },
            e => Value::ScalarRef(Rc::new(RefCell::new(self.scalar(e, scope)?))),
        })
    }

    fn range(&mut self, a: &Expr, b: &Expr, scope: &Rc<Scope>) -> ER<Vec<Value>> {
        let a = self.scalar(a, scope)?;
        let b = self.scalar(b, scope)?;
        if let (Value::Str(s), false) = (&a, a.looks_like_number()) {
            // 'aa'..'ad'
            let mut out = Vec::new();
            let mut cur = Value::Str(s.clone());
            let end = b.to_bytes();
            for _ in 0..1_000_000 {
                let cb = cur.to_bytes();
                if cb.len() > end.len() {
                    break;
                }
                out.push(cur.clone());
                if cb[..] == end[..] {
                    break;
                }
                match string_increment(&cur) {
                    Some(n) => cur = n,
                    None => break,
                }
            }
            return Ok(out);
        }
        let (x, y) = (a.to_i64(), b.to_i64());
        if y < x {
            return Ok(vec![]);
        }
        if y - x > 100_000_000 {
            return die("Out of memory in range");
        }
        Ok((x..=y).map(Value::Int).collect())
    }

    // ---- strings and patterns ----

    /// Build an interpolated string. In a pattern (`regex`), interpolated
    /// values are inserted as is (they are pattern text) unless under \Q.
    pub fn interpolate(&mut self, parts: &[Part], scope: &Rc<Scope>, regex: bool) -> ER<Vec<u8>> {
        let _ = regex;
        let mut out = Vec::new();
        // Active modes: \U / \L / \Q, until \E; and a pending \u / \l.
        let mut modes: Vec<CaseOp> = Vec::new();
        let mut next: Option<CaseOp> = None;
        for p in parts {
            let piece: Vec<u8> = match p {
                Part::Lit(b) => {
                    if regex && modes.contains(&CaseOp::Quote) {
                        // A literal under \Q in a pattern: its escapes were kept
                        // for the regex engine; resolve the simple ones.
                        unescape_pattern_literal(b)
                    } else {
                        b.clone()
                    }
                }
                Part::Scalar(e) => self.scalar(e, scope)?.to_bytes().to_vec(),
                Part::Array(e) => {
                    let vals = self.list(e, scope)?;
                    let sep = self.get_scalar("main::\"").to_bytes();
                    let mut o = Vec::new();
                    for (k, v) in vals.iter().enumerate() {
                        if k > 0 {
                            o.extend_from_slice(&sep);
                        }
                        o.extend_from_slice(&v.to_bytes());
                    }
                    o
                }
                Part::Case(op) => {
                    match op {
                        CaseOp::End => {
                            modes.pop();
                        }
                        CaseOp::UpperNext | CaseOp::LowerNext => next = Some(*op),
                        // \U after \L replaces it, as in perl.
                        CaseOp::Upper | CaseOp::Lower => {
                            if let Some(pos) = modes.iter().rposition(|m| matches!(m, CaseOp::Upper | CaseOp::Lower)) {
                                modes.remove(pos);
                            }
                            modes.push(*op);
                        }
                        _ => modes.push(*op),
                    }
                    continue;
                }
            };
            let mut piece = piece;
            for m in &modes {
                piece = match m {
                    CaseOp::Upper => piece.to_ascii_uppercase(),
                    CaseOp::Lower => piece.to_ascii_lowercase(),
                    CaseOp::Quote => crate::perl::regex::quotemeta(&piece),
                    _ => piece,
                };
            }
            if let Some(op) = next {
                if let Some(first) = piece.first_mut() {
                    *first = if op == CaseOp::UpperNext { first.to_ascii_uppercase() } else { first.to_ascii_lowercase() };
                    next = None;
                }
            }
            out.extend_from_slice(&piece);
        }
        Ok(out)
    }

    fn pattern_bytes(&mut self, pat: &Pat, scope: &Rc<Scope>) -> ER<Vec<u8>> {
        match pat.constant() {
            Some(b) => Ok(b.to_vec()),
            None => self.interpolate(&pat.parts, scope, true),
        }
    }

    pub fn regex(&mut self, pat: &Pat, scope: &Rc<Scope>) -> ER<Rc<crate::perl::regex::PerlRegex>> {
        let mut bytes = self.pattern_bytes(pat, scope)?;
        // An empty pattern reuses the last successful one; ExifTool never
        // relies on it, so treat it as matching the empty string.
        if bytes.is_empty() {
            bytes = b"(?:)".to_vec();
        }
        match crate::perl::regex::compile(&bytes, &pat.flags) {
            Ok(r) => Ok(r),
            Err(e) => die(e),
        }
    }

    fn do_match(&mut self, e: &Expr, scope: &Rc<Scope>, want: Want) -> ER<Vec<Value>> {
        let Expr::Match { target, neg, pat } = e else { unreachable!() };
        let re = self.regex(pat, scope)?;
        // The target, and where its pos() is kept (variables and elements).
        let (subject, cell) = match target {
            Some(t) => match &**t {
                Expr::Scalar(_) | Expr::ScalarDeref(_) | Expr::Elem(..) | Expr::HElem(..) => {
                    let lv = self.lvalue(t, scope)?;
                    let b = lv.get().to_bytes();
                    (b, pos_key(&lv).map(|k| (k, lv)))
                }
                t => (self.scalar(t, scope)?.to_bytes(), None),
            },
            None => {
                let lv = Lv::Scalar(self.scalar_cell(scope, "_"));
                let s = lv.get().to_bytes();
                (s, pos_key(&lv).map(|k| (k, lv)))
            }
        };
        let global = pat.flags.contains('g');
        if global && want == Want::List && !neg {
            let start = cell.as_ref().and_then(|(k, lv)| self.pos_at(*k, lv)).unwrap_or(0);
            let all = re.match_all(&subject, start).map_err(|e| Flow::Die(Value::from(e)))?;
            let mut out = Vec::new();
            for spans in &all {
                if spans.len() == 1 {
                    let (s, e) = spans[0].unwrap();
                    out.push(Value::bytes(&subject[s..e]));
                } else {
                    for g in &spans[1..] {
                        out.push(match g {
                            Some((s, e)) => Value::bytes(&subject[*s..*e]),
                            None => Value::Undef,
                        });
                    }
                }
            }
            if let Some(last) = all.last() {
                self.last_match = Some(LastMatch { subject: subject.clone(), spans: last.clone() });
            }
            if let Some((k, _)) = &cell {
                self.pos.remove(k);
            }
            return Ok(out);
        }
        let start = if global { cell.as_ref().and_then(|(k, lv)| self.pos_at(*k, lv)).unwrap_or(0) } else { 0 };
        let m = re.match_at(&subject, start).map_err(|e| Flow::Die(Value::from(e)))?;
        let matched = m.is_some();
        if let Some(spans) = &m {
            if global {
                if let Some((k, lv)) = &cell {
                    let (_, e) = spans[0].unwrap();
                    let tag = value_tag(&lv.get());
                    self.pos_set(*k, e, tag);
                }
            }
            self.last_match = Some(LastMatch { subject: subject.clone(), spans: spans.clone() });
        } else if global {
            if let Some((k, _)) = &cell {
                if !pat.flags.contains('c') {
                    self.pos.remove(k);
                }
            }
        }
        if *neg {
            return Ok(vec![boolean(!matched)]);
        }
        if want == Want::List && !global {
            return Ok(match m {
                None => vec![],
                Some(spans) if spans.len() == 1 => vec![Value::Int(1)],
                Some(spans) => spans[1..]
                    .iter()
                    .map(|g| match g {
                        Some((s, e)) => Value::bytes(&subject[*s..*e]),
                        None => Value::Undef,
                    })
                    .collect(),
            });
        }
        Ok(vec![boolean(matched)])
    }

    fn subst(&mut self, target: &Option<P<Expr>>, pat: &Pat, repl: &SubstRepl, scope: &Rc<Scope>) -> ER<Value> {
        let re = self.regex(pat, scope)?;
        let lv = match target {
            Some(t) => self.lvalue(t, scope)?,
            None => Lv::Scalar(self.scalar_cell(scope, "_")),
        };
        let subject = lv.get().to_bytes();
        let global = pat.flags.contains('g');
        let mut out = Vec::with_capacity(subject.len());
        let mut last = 0;
        let mut count = 0;
        let mut start = 0;
        loop {
            let Some(spans) = re.match_at(&subject, start).map_err(|e| Flow::Die(Value::from(e)))? else { break };
            let (s, e) = spans[0].unwrap();
            self.last_match = Some(LastMatch { subject: subject.clone(), spans: spans.clone() });
            out.extend_from_slice(&subject[last..s]);
            let r = match repl {
                SubstRepl::Interp(parts) => self.interpolate(parts, scope, false)?,
                SubstRepl::Code(block) => {
                    let mut v = self.run_block(block, scope, Want::Scalar)?;
                    if pat.flags.matches('e').count() > 1 {
                        // /ee: evaluate the result again as code.
                        let code = v.pop().unwrap_or_default().to_bytes();
                        let pkg = self.package.clone();
                        v = self.eval_string(&code, &pkg, scope, Want::Scalar).unwrap_or_default();
                    }
                    v.pop().unwrap_or_default().to_bytes().to_vec()
                }
            };
            out.extend_from_slice(&r);
            count += 1;
            last = e;
            if !global {
                break;
            }
            if e == s {
                // After an empty match, copy one byte and go on.
                if e >= subject.len() {
                    break;
                }
                out.push(subject[e]);
                last = e + 1;
                start = e + 1;
            } else {
                start = e;
            }
            if start > subject.len() {
                break;
            }
        }
        if count == 0 {
            return Ok(if pat.flags.contains('r') { Value::Str(subject) } else { NO() });
        }
        out.extend_from_slice(&subject[last.min(subject.len())..]);
        if pat.flags.contains('r') {
            return Ok(Value::bytes(out));
        }
        self.forget_pos(&lv);
        lv.set(Value::bytes(out))?;
        Ok(Value::Int(count))
    }

    // ---- blocks as values ----

    fn eval_block(&mut self, b: &Block, scope: &Rc<Scope>, want: Want) -> ER<Vec<Value>> {
        match self.run_block(b, scope, want) {
            Ok(v) => {
                self.set_scalar("main::@", Value::str(""));
                Ok(v)
            }
            Err(Flow::Die(e)) => {
                self.set_scalar("main::@", e);
                Ok(if want == Want::List { vec![] } else { vec![Value::Undef] })
            }
            Err(e) => Err(e),
        }
    }

    fn eval_str_expr(&mut self, code: &Expr, scope: &Rc<Scope>, want: Want) -> ER<Vec<Value>> {
        let src = self.scalar(code, scope)?.to_bytes();
        let pkg = self.package.clone();
        let inner = Scope::new(Some(scope.clone()));
        match self.eval_string(&src, &pkg, &inner, want) {
            Ok(v) => Ok(v),
            Err(_) => Ok(if want == Want::List { vec![] } else { vec![Value::Undef] }),
        }
    }

    fn map_grep(&mut self, cb: &Callback, l: &Expr, scope: &Rc<Scope>, map: bool) -> ER<Vec<Value>> {
        let items = self.alias_items(l, scope)?;
        let key = (b'$', Rc::from("main::_"));
        let saved = self.globals.get(&key).cloned();
        let mut out = Vec::new();
        let mut result = Ok(());
        for (cell, back) in items {
            self.globals.insert(key.clone(), Binding::Scalar(cell.clone()));
            let r = match cb {
                Callback::Block(b) => self.run_block(b, scope, if map { Want::List } else { Want::Scalar }),
                Callback::Expr(e) => {
                    if map {
                        self.list(e, scope)
                    } else {
                        self.scalar(e, scope).map(|v| vec![v])
                    }
                }
            };
            let r = match r {
                Ok(v) => v,
                Err(e) => {
                    result = Err(e);
                    break;
                }
            };
            if let Some(lv) = back {
                lv.set(cell.borrow().clone())?;
            }
            if map {
                out.extend(r);
            } else if r.last().is_some_and(|v| v.truthy()) {
                out.push(cell.borrow().clone());
            }
        }
        match saved {
            Some(b) => {
                self.globals.insert(key, b);
            }
            None => {
                self.globals.remove(&key);
            }
        }
        result?;
        Ok(out)
    }

    fn sort(&mut self, cb: &Option<Callback>, l: &Expr, scope: &Rc<Scope>) -> ER<Vec<Value>> {
        let mut items = self.list(l, scope)?;
        let Some(cb) = cb else {
            items.sort_by(|a, b| a.to_bytes()[..].cmp(&b.to_bytes()[..]));
            return Ok(items);
        };
        let a_cell: Cell = Rc::new(RefCell::new(Value::Undef));
        let b_cell: Cell = Rc::new(RefCell::new(Value::Undef));
        let ka = (b'$', self.global_key("a"));
        let kb = (b'$', self.global_key("b"));
        let sa = self.globals.insert(ka.clone(), Binding::Scalar(a_cell.clone()));
        let sb = self.globals.insert(kb.clone(), Binding::Scalar(b_cell.clone()));
        let code = match cb {
            Callback::Expr(e) => match &**e {
                Expr::Const(name) => {
                    let n = self.sub_key(&name.to_text());
                    self.subs.get(&*n).cloned()
                }
                e => match self.scalar(e, scope)? {
                    Value::Code(c) => Some(c),
                    _ => None,
                },
            },
            Callback::Block(_) => None,
        };
        let mut err = None;
        // A merge sort, stable like perl's.
        let mut idx: Vec<usize> = (0..items.len()).collect();
        let mut cmp = |x: usize, y: usize, me: &mut Interp| -> std::cmp::Ordering {
            if err.is_some() {
                return std::cmp::Ordering::Equal;
            }
            *a_cell.borrow_mut() = items[x].clone();
            *b_cell.borrow_mut() = items[y].clone();
            let r = match (cb, &code) {
                (Callback::Block(b), _) => me.run_block(b, scope, Want::Scalar),
                (_, Some(c)) => me.call(c, vec![], Want::Scalar),
                _ => Ok(vec![Value::Int(0)]),
            };
            match r {
                Ok(v) => {
                    let n = v.last().map(|v| v.to_i64()).unwrap_or(0);
                    n.cmp(&0)
                }
                Err(e) => {
                    err = Some(e);
                    std::cmp::Ordering::Equal
                }
            }
        };
        merge_sort(&mut idx, &mut |x, y| cmp(x, y, self));
        for (k, s) in [(ka, sa), (kb, sb)] {
            match s {
                Some(b) => {
                    self.globals.insert(k, b);
                }
                None => {
                    self.globals.remove(&k);
                }
            }
        }
        if let Some(e) = err {
            return Err(e);
        }
        Ok(idx.into_iter().map(|i| std::mem::take(&mut items[i])).collect())
    }

    // ---- calls ----

    fn eval_call(&mut self, e: &Expr, scope: &Rc<Scope>, want: Want) -> ER<Vec<Value>> {
        match e {
            Expr::Call { name, args, amp } => {
                if !*amp && !name.contains("::") {
                    if let Some(r) = io::call(self, name, args, scope, want) {
                        return r;
                    }
                    if let Some(r) = builtins::call(self, name, args, scope, want) {
                        return r;
                    }
                }
                let key = self.sub_key(name);
                if *amp && args.is_empty() && !self.subs.contains_key(&*key) && !name.contains("::") {
                    // A bareword: its own name without strict.
                    if self.strict {
                        return die(format!("Bareword \"{name}\" not allowed while \"strict subs\" in use"));
                    }
                    return Ok(vec![Value::str(name)]);
                }
                // perl evaluates the arguments before it looks the sub up.
                let (a, al) = self.call_args(args, scope, None)?;
                match self.subs.get(&*key).cloned() {
                    Some(c) => {
                        self.pending_aliases = Some(al);
                        self.call(&c, a, want)
                    }
                    None => match self.autoload(&key) {
                        Some(c) => {
                            self.pending_aliases = Some(al);
                            self.call(&c, a, want)
                        }
                        None => die(format!("Undefined subroutine &{key} called")),
                    },
                }
            }
            Expr::CallRef(f, args) => {
                let f = self.scalar(f, scope)?;
                let (a, al) = self.call_args(args, scope, None)?;
                match f {
                    Value::Code(c) => {
                        self.pending_aliases = Some(al);
                        self.call(&c, a, want)
                    }
                    Value::Str(s) => {
                        self.check_symref(&s, "a subroutine")?;
                        let n = String::from_utf8_lossy(&s).into_owned();
                        self.pending_aliases = Some(al);
                        let r = self.call_named(&n, a, want);
                        self.pending_aliases = None;
                        r
                    }
                    Value::Undef => die("Can't use an undefined value as a subroutine reference"),
                    _ => die("Not a CODE reference"),
                }
            }
            Expr::Method { inv, name, args } => {
                let invv = self.scalar(inv, scope)?;
                let mname: Rc<str> = match name {
                    MethodName::Static(n) => n.clone(),
                    MethodName::Dynamic(e) => match self.scalar(e, scope)? {
                        Value::Code(c) => {
                            let mut a = vec![invv];
                            for x in args {
                                a.extend(self.list(x, scope)?);
                            }
                            return self.call(&c, a, want);
                        }
                        v => Rc::from(v.to_text()),
                    },
                };
                let (a, al) = self.call_args(args, scope, Some(invv.clone()))?;
                self.pending_aliases = Some(al);
                let r = self.call_method(&invv, &mname, a, want);
                self.pending_aliases = None;
                r
            }
            _ => unreachable!(),
        }
    }

    /// The arguments of a call, with the storage each one aliases.
    fn call_args(&mut self, args: &[Expr], scope: &Rc<Scope>, inv: Option<Value>) -> ER<(Vec<Value>, Vec<Option<Lv>>)> {
        let mut vals = Vec::new();
        let mut al = Vec::new();
        if let Some(i) = inv {
            vals.push(i);
            al.push(None);
        }
        for x in args {
            match x {
                Expr::Scalar(n) if is_match_var(n) => {
                    vals.push(self.read_scalar(n, scope));
                    al.push(None);
                }
                Expr::Scalar(_) | Expr::ScalarDeref(_) | Expr::Elem(..) | Expr::HElem(..) => {
                    let lv = self.lvalue(x, scope)?;
                    vals.push(lv.get());
                    al.push(Some(lv));
                }
                Expr::Array(c) => {
                    let a = self.array_of(c, scope, false)?;
                    let n = a.borrow().len();
                    for i in 0..n {
                        vals.push(a.borrow()[i].clone());
                        al.push(Some(Lv::AElem(a.clone(), i as i64)));
                    }
                }
                // a literal is read-only: writing $_[i] to it dies
                Expr::Const(c) => {
                    vals.push(c.clone());
                    al.push(Some(Lv::ReadOnly(c.clone())));
                }
                x => {
                    for v in self.list(x, scope)? {
                        vals.push(v);
                        al.push(None);
                    }
                }
            }
        }
        Ok((vals, al))
    }

    /// `$obj->name(args)`: find the method through the class and its @ISA.
    pub fn call_method(&mut self, inv: &Value, name: &str, args: Vec<Value>, want: Want) -> ER<Vec<Value>> {
        let class: Rc<str> = match inv {
            Value::HashRef(h) => match &h.borrow().blessed {
                Some(c) => c.clone(),
                None => return die(format!("Can't call method \"{name}\" on unblessed reference")),
            },
            Value::Undef => return die(format!("Can't call method \"{name}\" on an undefined value")),
            // any other plain value names a class
            v if !v.is_ref() => {
                let t = v.to_text();
                if t.is_empty() {
                    return die(format!("Can't call method \"{name}\" without a package or object reference"));
                }
                Rc::from(t.as_str())
            }
            // a handle is an IO::File unless blessed otherwise (perl loads
            // IO::File for methods on handles)
            Value::Handle(h) => h.borrow().class.clone().unwrap_or_else(|| Rc::from("IO::File")),
            _ => return die(format!("Can't call method \"{name}\" on unblessed reference")),
        };
        if let Some(qualified) = name.strip_prefix("SUPER::") {
            let parents = self.parents_of(&self.package.clone());
            for p in parents {
                if let Some(c) = self.find_method(&p, qualified) {
                    return self.call(&c, args, want);
                }
            }
            return die(self.no_method_message(qualified, &class));
        }
        if name.contains("::") {
            return self.call_named(name, args, want);
        }
        // UNIVERSAL's isa and can come before any AUTOLOAD, as in perl's
        // method resolution
        let mut found = self.find_method(&class, name);
        if found.is_none() && !matches!(name, "isa" | "can") {
            found = self.find_autoload(&class, name);
        }
        match found {
            Some(c) => self.call(&c, args, want),
            None => {
                if name == "isa" {
                    let target = args.get(1).map(|v| v.to_text()).unwrap_or_default();
                    return Ok(vec![boolean(self.class_isa(&class, &target))]);
                }
                if name == "can" {
                    let m = args.get(1).map(|v| v.to_text()).unwrap_or_default();
                    return Ok(vec![match self.find_method(&class, &m) {
                        Some(c) => Value::Code(c),
                        None => NO(),
                    }]);
                }
                die(self.no_method_message(name, &class))
            }
        }
    }

    /// perl's message for a method a class does not have, with its hint
    /// when nothing of that package exists.
    fn no_method_message(&self, name: &str, class: &str) -> String {
        let prefix = format!("{class}::");
        let exists = self.subs.keys().any(|k| k.starts_with(&prefix))
            || self.globals.keys().any(|(_, k)| k.starts_with(&prefix))
            || self.isa.contains_key(class);
        if exists {
            format!("Can't locate object method \"{name}\" via package \"{class}\"")
        } else {
            format!("Can't locate object method \"{name}\" via package \"{class}\" (perhaps you forgot to load \"{class}\"?)")
        }
    }

    /// perl's AUTOLOAD: a missing `Pkg::name` calls `Pkg::AUTOLOAD` with
    /// `$Pkg::AUTOLOAD` set to the full name.
    /// Make `target` the sub `source` is, now or once it is defined.
    pub fn import_sub(&mut self, target: &str, source: &str) {
        match self.subs.get(source).cloned() {
            Some(c) => {
                self.subs.insert(Rc::from(target), c);
            }
            None => {
                self.sub_aliases.insert(Rc::from(target), Rc::from(source));
            }
        }
    }

    fn autoload(&mut self, full: &str) -> Option<Rc<Code>> {
        // an imported stub: the sub it names, or that package's AUTOLOAD
        if let Some(source) = self.sub_aliases.get(full).cloned() {
            if let Some(c) = self.subs.get(&*source).cloned() {
                return Some(c);
            }
            return self.autoload(&source);
        }
        let (pkg, _) = full.rsplit_once("::")?;
        let code = self.subs.get(format!("{pkg}::AUTOLOAD").as_str()).cloned()?;
        self.set_scalar(&format!("{pkg}::AUTOLOAD"), Value::str(full));
        Some(code)
    }

    /// AUTOLOAD for a method, through the class and its parents.
    fn find_autoload(&mut self, class: &str, name: &str) -> Option<Rc<Code>> {
        let code = self.find_method(class, "AUTOLOAD")?;
        let pkg = match &*code {
            Code::Perl(sub, _) => sub.name.as_deref().and_then(|n| n.rsplit_once("::")).map(|(p, _)| p.to_string()).unwrap_or_else(|| class.to_string()),
            Code::Native(n, _) => n.rsplit_once("::").map(|(p, _)| p.to_string()).unwrap_or_else(|| class.to_string()),
        };
        self.set_scalar(&format!("{pkg}::AUTOLOAD"), Value::str(&format!("{class}::{name}")));
        Some(code)
    }

    fn class_isa(&self, class: &str, target: &str) -> bool {
        if class == target {
            return true;
        }
        self.parents_of(class).iter().any(|p| self.class_isa(p, target))
    }

    /// A class's parents: its @ISA as the program set it, then those the
    /// runtime declares.
    fn parents_of(&self, class: &str) -> Vec<Rc<str>> {
        let mut out: Vec<Rc<str>> = Vec::new();
        if let Some(Binding::Array(a)) = self.globals.get(&(b'@', Rc::from(format!("{class}::ISA")))) {
            out.extend(a.borrow().iter().map(|p| Rc::from(p.to_text().as_str())));
        }
        for p in self.isa.get(class).map(|v| v.as_slice()).unwrap_or(&[]) {
            if !out.contains(p) {
                out.push(p.clone());
            }
        }
        out
    }

    fn find_method(&self, class: &str, name: &str) -> Option<Rc<Code>> {
        let key = format!("{class}::{name}");
        if let Some(c) = self.subs.get(key.as_str()) {
            return Some(c.clone());
        }
        // @ISA set by the program itself, then by the runtime.
        if let Some(Binding::Array(a)) = self.globals.get(&(b'@', Rc::from(format!("{class}::ISA")))) {
            for p in a.borrow().iter() {
                if let Some(c) = self.find_method(&p.to_text(), name) {
                    return Some(c);
                }
            }
        }
        for p in self.isa.get(class).map(|v| v.as_slice()).unwrap_or(&[]) {
            if let Some(c) = self.find_method(p, name) {
                return Some(c);
            }
        }
        None
    }
}

/// Where pos() of an lvalue is kept: a scalar's cell, or an element's
/// container and index/key.
fn pos_key(lv: &Lv) -> Option<usize> {
    match lv {
        Lv::Scalar(c) => Some(Rc::as_ptr(c) as usize),
        Lv::AElem(a, i) => Some((Rc::as_ptr(a) as usize).wrapping_mul(31).wrapping_add(*i as usize).wrapping_add(1)),
        Lv::HElem(h, k) => {
            use std::hash::{Hash, Hasher};
            let mut hs = std::collections::hash_map::DefaultHasher::new();
            k.hash(&mut hs);
            Some((Rc::as_ptr(h) as usize).wrapping_mul(17) ^ hs.finish() as usize)
        }
        _ => None,
    }
}

/// Identifies the value a pos() was set on, so a stale pos() is detected:
/// a weak handle on the string itself, as an address could be reused by a
/// new string put in the same variable (from Rust code, which does not
/// clear pos()).
#[derive(Clone)]
pub(crate) enum PosTag {
    Str(std::rc::Weak<[u8]>),
    Other,
}

fn value_tag(v: &Value) -> PosTag {
    match v {
        Value::Str(s) => PosTag::Str(Rc::downgrade(s)),
        _ => PosTag::Other,
    }
}

impl PosTag {
    fn matches(&self, v: &Value) -> bool {
        match (self, v) {
            (PosTag::Str(w), Value::Str(s)) => w.upgrade().is_some_and(|x| Rc::ptr_eq(&x, s)),
            (PosTag::Other, Value::Str(_)) => false,
            (PosTag::Str(_), _) => false,
            (PosTag::Other, _) => true,
        }
    }

    fn alive(&self) -> bool {
        match self {
            PosTag::Str(w) => w.strong_count() > 0,
            PosTag::Other => true,
        }
    }
}

/// The variables of the last match: `$1`.., `$&`, `` $` ``, `$'`, `$+`.
fn is_match_var(name: &str) -> bool {
    let b = name.as_bytes();
    (!b.is_empty() && b.iter().all(|c| c.is_ascii_digit()) && name != "0") || matches!(name, "&" | "`" | "'" | "+")
}

/// A value in a trace line (same format as scripts/trace_db.pl).
fn trace_summary(v: &Value) -> String {
    match v {
        Value::Undef => "U".into(),
        Value::HashRef(h) => match &h.borrow().blessed {
            Some(c) => format!("R:{c}"),
            None => "R:HASH".into(),
        },
        v if v.is_ref() => format!("R:{}", v.ref_type()),
        v => {
            let full = v.to_bytes();
            let b = &full[..full.len().min(40)];
            let mut s = String::from("S:");
            for &c in b {
                if (0x20..=0x7e).contains(&c) && c != b'\\' && c != b'|' {
                    s.push(c as char);
                } else {
                    s.push_str(&format!("\\x{c:02x}"));
                }
            }
            if full.len() > 40 {
                s.push_str(&format!("...#{}", full.len()));
            }
            s
        }
    }
}

/// The label a statement carries, for `goto`.
fn stmt_label(s: &Stmt) -> Option<&str> {
    match s {
        Stmt::Label(l) => Some(l),
        Stmt::While { label: Some(l), .. } | Stmt::ForC { label: Some(l), .. } | Stmt::Foreach { label: Some(l), .. } | Stmt::Block(Some(l), _) => Some(l),
        _ => None,
    }
}

/// Whether a statement is, or contains in an if/else or bare block, the
/// label (where a goto may jump into).
fn contains_label(s: &Stmt, l: &str) -> bool {
    if stmt_label(s) == Some(l) {
        return true;
    }
    match s {
        Stmt::If(arms, otherwise) => arms.iter().any(|(_, b)| b.iter().any(|x| contains_label(x, l))) || otherwise.as_ref().is_some_and(|b| b.iter().any(|x| contains_label(x, l))),
        Stmt::Block(_, b) => b.iter().any(|x| contains_label(x, l)),
        _ => false,
    }
}

fn label_matches(got: &Option<Rc<str>>, mine: &Option<Rc<str>>) -> bool {
    match got {
        None => true,
        Some(l) => mine.as_deref() == Some(&**l),
    }
}

/// Whether an assignment to this is a list assignment.
fn is_list_lhs(e: &Expr) -> bool {
    match e {
        Expr::List(_) | Expr::Array(_) | Expr::Hash(_) | Expr::Slice(..) | Expr::HSlice(..) => true,
        Expr::My(vars, paren) => *paren || matches!(vars.first(), Some(Expr::Array(_) | Expr::Hash(_))),
        Expr::Local(inner) => is_list_lhs(inner),
        _ => false,
    }
}

fn is_lvalue_like(e: &Expr) -> bool {
    matches!(e, Expr::Scalar(_) | Expr::ScalarDeref(_) | Expr::Elem(..) | Expr::HElem(..) | Expr::My(..))
}

/// Assign a list of values to an lvalue, as perl's list assignment: arrays
/// and hashes take everything left.
fn assign_values(lv: &Lv, vals: Vec<Value>) -> ER<()> {
    match lv {
        Lv::Array(a) => *a.borrow_mut() = vals,
        Lv::Hash(h) => *h.borrow_mut() = pairs_to_hash(vals),
        Lv::List(items) => {
            let mut it = vals.into_iter();
            for item in items {
                match item {
                    Lv::Array(a) => *a.borrow_mut() = it.by_ref().collect(),
                    Lv::Hash(h) => *h.borrow_mut() = pairs_to_hash(it.by_ref().collect()),
                    other => other.set(it.next().unwrap_or_default())?,
                }
            }
        }
        other => other.set(vals.into_iter().next().unwrap_or_default())?,
    }
    Ok(())
}

fn lv_values(lv: &Lv) -> Vec<Value> {
    match lv {
        Lv::Array(a) => a.borrow().clone(),
        Lv::Hash(h) => h.borrow().iter().flat_map(|(k, v)| [Value::Str(k.clone()), v.clone()]).collect(),
        Lv::List(items) => items.iter().flat_map(lv_values).collect(),
        Lv::Skip => vec![],
        other => vec![other.get()],
    }
}

pub fn pairs_to_hash(vals: Vec<Value>) -> Hash {
    let mut h = Hash::default();
    let mut it = vals.into_iter();
    while let Some(k) = it.next() {
        let v = it.next().unwrap_or_default();
        h.insert(&k.to_bytes(), v);
    }
    h
}

/// Shape a result for its context: a scalar context takes the last value.
fn fit(mut v: Vec<Value>, want: Want) -> Vec<Value> {
    match want {
        Want::List => v,
        _ => {
            let last = v.pop().unwrap_or_default();
            vec![last]
        }
    }
}

/// A string operand of a bitwise operator: a string never used as a number.
fn is_string_operand(v: &Value) -> bool {
    matches!(v, Value::Str(_))
}

fn string_bitop(a: &[u8], b: &[u8], f: impl Fn(u8, u8) -> u8) -> Vec<u8> {
    let n = a.len().max(b.len());
    (0..n).map(|i| f(*a.get(i).unwrap_or(&0), *b.get(i).unwrap_or(&0))).collect()
}

/// Perl's magic string increment: "aa"++ is "ab", "Az"++ is "Ba", "zz"++
/// is "aaa", "a9"++ is "b0".
fn string_increment(v: &Value) -> Option<Value> {
    let Value::Str(s) = v else { return None };
    if s.is_empty() || v.looks_like_number() {
        return None;
    }
    let letters = s.iter().take_while(|c| c.is_ascii_alphabetic()).count();
    if !s[letters..].iter().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut b = s.to_vec();
    let mut i = b.len();
    loop {
        if i == 0 {
            let first = match b[0] {
                b'0'..=b'9' => b'1',
                b'a'..=b'z' => b'a',
                _ => b'A',
            };
            b.insert(0, first);
            return Some(Value::bytes(b));
        }
        i -= 1;
        match b[i] {
            b'z' => b[i] = b'a',
            b'Z' => b[i] = b'A',
            b'9' => b[i] = b'0',
            c => {
                b[i] = c + 1;
                return Some(Value::bytes(b));
            }
        }
    }
}

/// Inside \Q..\E in a pattern, the literal text still carries its regex
/// escapes; turn `\.` back into `.` before quotemeta re-escapes it.
fn unescape_pattern_literal(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            out.push(b[i + 1]);
            i += 2;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

fn merge_sort(v: &mut Vec<usize>, cmp: &mut dyn FnMut(usize, usize) -> std::cmp::Ordering) {
    if v.len() <= 1 {
        return;
    }
    let mid = v.len() / 2;
    let mut right = v.split_off(mid);
    merge_sort(v, cmp);
    merge_sort(&mut right, cmp);
    let left = std::mem::take(v);
    let (mut i, mut j) = (0, 0);
    while i < left.len() && j < right.len() {
        if cmp(left[i], right[j]) == std::cmp::Ordering::Greater {
            v.push(right[j]);
            j += 1;
        } else {
            v.push(left[i]);
            i += 1;
        }
    }
    v.extend_from_slice(&left[i..]);
    v.extend_from_slice(&right[j..]);
}
