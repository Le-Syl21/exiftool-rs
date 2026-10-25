//! File::RandomAccess (File/RandomAccess.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Buffer to support random access reading of sequential file. The object
//! stays the blessed hash of the Perl (FILE_PT, BUFF_PT, BASE, POS, LEN,
//! TESTED...), and the file is read through the interpreter's perl
//! filehandles: `read`, `seek`, `tell`, `<$fp>`, `binmode` and `close` run
//! as the Perl calls them (see `perl_eval`).
//!
//! This file also holds the local helpers the ports of IPTC, ICC_Profile,
//! Photoshop, PrintIM, Fixup and JPEG share (`substr`, `perl_eval`,
//! `ref_name`, `bless`...): they are built on the public Interp API only.

use std::cell::RefCell;
use std::rc::Rc;

use crate::interp::eval::{Cell, ER};
use crate::interp::{Binding, Flow, Interp, Scope, Want};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, ret};

// ---- local helpers (shared by this agent's ports) ----

fn n(i: i64) -> Value {
    Value::Int(i)
}

/// `substr(STR, OFF, LEN)` (rvalue) as the interpreter's builtin does it:
/// an undefined LEN is 0, and outside the string it warns and gives undef.
pub(crate) fn substr(it: &mut Interp, s: &Value, off: &Value, len: Option<&Value>) -> ER<Value> {
    let s = s.to_bytes();
    let off = off.to_i64();
    let len = len.map(|l| if l.is_undef() { 0 } else { l.to_i64() });
    Ok(match crate::interp::eval::substr_range(s.len(), off, len) {
        Some((a, b)) => Value::bytes(&s[a..b]),
        None => {
            it.warn(Value::str("substr outside of string at (eval) line 1.\n"))?;
            Value::Undef
        }
    })
}

/// `index(STR, SUBSTR, POS)` as the interpreter's builtin.
pub(crate) fn index(s: &Value, sub: &Value, pos: Option<&Value>) -> Value {
    let s = s.to_bytes();
    let sub = sub.to_bytes();
    let start = pos.map(|p| p.to_i64()).unwrap_or(0).clamp(0, s.len() as i64) as usize;
    if sub.is_empty() {
        return n(start as i64);
    }
    n(s[start..].windows(sub.len()).position(|w| w == &sub[..]).map_or(-1, |p| (p + start) as i64))
}

/// `ref $v`: the class of a blessed reference, else its type.
pub(crate) fn ref_name(v: &Value) -> Value {
    if let Value::HashRef(h) = v {
        if let Some(c) = &h.borrow().blessed {
            return Value::str(c);
        }
    }
    Value::str(v.ref_type())
}

/// `bless $ref, $class`
pub(crate) fn bless(v: &Value, class: &Value) {
    if let Value::HashRef(h) = v {
        h.borrow_mut().blessed = Some(Rc::from(class.to_text().as_str()));
    }
}

/// `$$ref = $val` on a scalar reference.
pub(crate) fn set_deref(r: &Value, v: Value) -> ER<()> {
    match r {
        Value::ScalarRef(c) => *c.borrow_mut() = v,
        Value::LvRef(l) => l.set(v)?,
        _ => {}
    }
    Ok(())
}

/// A new scalar cell (a `my` variable handed to `perl_eval`).
pub(crate) fn cell(v: Value) -> Cell {
    Rc::new(RefCell::new(v))
}

/// Run a short Perl statement in `package` with the given `my` scalars in
/// scope, for the perl builtins a native can't call directly (I/O on perl
/// filehandles, `print`, matches that keep pos() on the caller's scalar,
/// method calls that write their arguments). `$@` is left as it was; an
/// error is a die.
pub(crate) fn perl_eval(it: &mut Interp, code: &str, package: &str, vars: &[(&str, &Cell)], want: Want) -> ER<Vec<Value>> {
    let scope = Scope::new(None);
    for (name, c) in vars {
        scope.declare(b'$', name, Binding::Scalar((*c).clone()));
    }
    let saved_err = it.get_scalar("main::@");
    let r = it.eval_string(code.as_bytes(), package, &scope, want);
    it.set_scalar("main::@", saved_err);
    r.map_err(Flow::Die)
}

/// `perl_eval` in scalar context.
pub(crate) fn perl_eval1(it: &mut Interp, code: &str, package: &str, vars: &[(&str, &Cell)]) -> ER<Value> {
    Ok(perl_eval(it, code, package, vars, Want::Scalar)?.pop().unwrap_or_default())
}

/// `print LIST` to the selected handle.
pub(crate) fn print(it: &mut Interp, s: Value) -> ER<()> {
    let c = cell(s);
    perl_eval(it, "print $s", "main", &[("s", &c)], Want::Void)?;
    Ok(())
}

/// `$obj->Read($buff, $len)` through the method call, so the buffer is
/// written as an alias whatever implementation runs: returns the number
/// of bytes read and updates `buff`.
#[allow(dead_code)]
pub(crate) fn raf_read(it: &mut Interp, raf: &Value, buff: &mut Value, len: Value) -> ER<Value> {
    let (r, b, l) = (cell(raf.clone()), cell(buff.clone()), cell(len));
    let v = perl_eval1(it, "$raf->Read($buff, $len)", "File::RandomAccess", &[("raf", &r), ("buff", &b), ("len", &l)])?;
    *buff = b.borrow().clone();
    Ok(v)
}

/// `$obj->ReadLine($buff)`, as `raf_read`.
#[allow(dead_code)]
pub(crate) fn raf_read_line(it: &mut Interp, raf: &Value, buff: &mut Value) -> ER<Value> {
    let (r, b) = (cell(raf.clone()), cell(buff.clone()));
    let v = perl_eval1(it, "$raf->ReadLine($buff)", "File::RandomAccess", &[("raf", &r), ("buff", &b)])?;
    *buff = b.borrow().clone();
    Ok(v)
}

/// `read($fp, $buff, $num)`: the count (undef on error) and the data.
fn perl_read(it: &mut Interp, fp: &Value, buff: &mut Value, num: &Value) -> ER<Value> {
    let (f, b, l) = (cell(fp.clone()), cell(buff.clone()), cell(num.clone()));
    let v = perl_eval1(it, "read($fp, $buff, $num)", "File::RandomAccess", &[("fp", &f), ("buff", &b), ("num", &l)])?;
    *buff = b.borrow().clone();
    Ok(v)
}

fn errno(it: &mut Interp) -> Value {
    it.get_scalar("main::!")
}

// constants
const CHUNK_SIZE: i64 = 8192; // size of chunks to read from file (must be power of 2)
const SKIP_SIZE: i64 = 65536; // size to skip when fast-forwarding over sequential data
const SLURP_CHUNKS: i64 = 16; // read this many chunks at a time when slurping

/// Create new RandomAccess object
/// Inputs: 0) reference to RandomAccess object or RandomAccess class name
///         1) file reference or scalar reference
///         2) flag set if file is already random access (disables automatic SeekTest)
pub fn new(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (that, file_pt, is_random) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let r = ref_name(&that);
    let class = if r.truthy() { r } else { that };
    let self_ = Value::new_hash();

    if ref_name(&file_pt).eq_str("SCALAR") {
        // string i/o
        self_.set("BUFF_PT", file_pt.clone());
        self_.set("BASE", n(0));
        self_.set("POS", n(0));
        self_.set("LEN", pl::length(&file_pt.deref()));
        self_.set("TESTED", n(-1));
        bless(&self_, &class);
    } else {
        // file i/o
        let buff = Value::binary(b"");
        self_.set("FILE_PT", file_pt); // file pointer
        self_.set("BUFF_PT", buff); // reference to file data
        self_.set("BASE", n(0)); // location of start of buffer in file
        self_.set("POS", n(0)); // current position in buffer
        self_.set("LEN", n(0)); // length of data in buffer
        self_.set("TESTED", n(0)); // 0=untested, 1=passed, -1=failed (requires buffering)
        bless(&self_, &class);
        if !is_random.truthy() {
            seek_test_impl(it, &self_)?;
        }
    }
    ret(self_)
}

/// Enable DEBUG code
/// Inputs: 0) reference to RandomAccess object
pub fn debug(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let self_ = arg(&a, 0);
    let h = Value::new_hash();
    self_.set("DEBUG", h.clone());
    ret(h)
}

fn seek_test_impl(it: &mut Interp, self_: &Value) -> ER<Value> {
    if !self_.get("TESTED").truthy() {
        let fp = cell(self_.get("FILE_PT"));
        if perl_eval1(it, "seek($fp, 1, 1) and seek($fp, -1, 1)", "File::RandomAccess", &[("fp", &fp)])?.truthy() {
            self_.set("TESTED", n(1)); // test passed
        } else {
            self_.set("TESTED", n(-1)); // test failed (requires buffering)
        }
    }
    Ok(if pl::num_eq(&self_.get("TESTED"), &n(1)) { n(1) } else { n(0) })
}

/// Perform seek test and turn on buffering if necessary
/// Inputs: 0) reference to RandomAccess object
/// Returns: 1 if seek test passed (ie. no buffering required)
/// Notes: Must be done before any other i/o
pub fn seek_test(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(seek_test_impl(it, &arg(&a, 0))?)
}

fn tell_impl(it: &mut Interp, self_: &Value) -> ER<Value> {
    let rtn_val;
    if pl::lt(&self_.get("TESTED"), &n(0)) {
        rtn_val = pl::add(&self_.get("POS"), &self_.get("BASE"));
    } else {
        let fp = cell(self_.get("FILE_PT"));
        rtn_val = perl_eval1(it, "tell($fp)", "File::RandomAccess", &[("fp", &fp)])?;
    }
    Ok(rtn_val)
}

/// Get current position in file
/// Inputs: 0) reference to RandomAccess object
/// Returns: current position in file
pub fn tell(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(tell_impl(it, &arg(&a, 0))?)
}

fn seek_impl(it: &mut Interp, self_: &Value, num: &Value, whence: &Value) -> ER<Value> {
    let whence = if whence.is_undef() { n(0) } else { whence.clone() };
    let mut rtn_val = Value::Undef;
    if pl::lt(&self_.get("TESTED"), &n(0)) {
        let new_pos;
        if pl::num_eq(&whence, &n(0)) {
            new_pos = pl::sub(num, &self_.get("BASE")); // from start of file
        } else if pl::num_eq(&whence, &n(1)) {
            new_pos = pl::add(num, &self_.get("POS")); // relative to current position
        } else if self_.get("NoBuffer").truthy() && self_.get("FILE_PT").truthy() {
            new_pos = n(-1); // (can't seek relative to end if no buffering)
        } else {
            slurp_impl(it, self_)?; // read whole file into buffer
            new_pos = pl::add(num, &self_.get("LEN")); // relative to end of file
        }
        if pl::ge(&new_pos, &n(0)) &&
            // can't go backwards in unbuffered non-seekable file
            (!self_.get("NoBuffer").truthy() || pl::ge(&new_pos, &self_.get("POS")))
        {
            self_.set("POS", new_pos);
            rtn_val = n(1);
        }
    } else {
        let (fp, nm, wh) = (cell(self_.get("FILE_PT")), cell(num.clone()), cell(whence));
        rtn_val = perl_eval1(it, "seek($fp, $num, $whence)", "File::RandomAccess", &[("fp", &fp), ("num", &nm), ("whence", &wh)])?;
    }
    Ok(rtn_val)
}

/// Seek to position in file
/// Inputs: 0) reference to RandomAccess object
///         1) position, 2) whence (0 or undef=from start, 1=from cur pos, 2=from end)
/// Returns: 1 on success
/// Notes: When buffered, this doesn't quite behave like seek() since it will return
///        success even if you seek outside the limits of the file.  However if you
///        do this, you will get an error on your next Read().
pub fn seek(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(seek_impl(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?)
}

/// The body of Read: `buf` is `$_[0]`, and `set` tells whether it was
/// assigned.
fn read_impl(it: &mut Interp, self_: &Value, buf: &mut Value, set: &mut bool, len_in: &Value) -> ER<Value> {
    let mut len = len_in.clone();
    let mut rtn_val;

    // protect against reading too much at once
    // (also from dying with a "Negative length" error)
    if ops::bit_and(&len, &n(0xf8000000)).truthy() {
        if pl::lt(&len, &n(0)) {
            return Ok(n(0));
        }
        // read in smaller blocks because Windows attempts to pre-allocate
        // memory for the full size, which can lead to an out-of-memory error
        let max_len = n(0x4000000); // (MUST be less than bitmask in "if" above)
        let mut num = read_impl(it, self_, buf, set, &max_len)?;
        if pl::lt(&num, &max_len) {
            return Ok(num);
        }
        loop {
            len = pl::sub(&len, &max_len);
            if pl::le(&len, &n(0)) {
                break;
            }
            let l = if pl::lt(&len, &max_len) { len.clone() } else { max_len.clone() };
            let mut buff = Value::Undef;
            let mut bset = false;
            let nn = read_impl(it, self_, &mut buff, &mut bset, &l)?;
            if !nn.truthy() {
                break;
            }
            *buf = pl::concat(buf, &buff);
            *set = true;
            num = pl::add(&num, &nn);
            if pl::lt(&nn, &l) {
                break;
            }
        }
        return Ok(num);
    }
    // read through our buffer if necessary
    if pl::lt(&self_.get("TESTED"), &n(0)) {
        // purge old data before reading in NoBuffer mode
        if self_.get("NoBuffer").truthy() && !purge_impl(it, self_)?.truthy() {
            return Ok(n(0));
        }
        let mut buff = Value::Undef;
        let new_pos = pl::add(&self_.get("POS"), &len);
        // number of bytes to read from file
        let mut num = pl::sub(&new_pos, &self_.get("LEN"));
        if pl::gt(&num, &n(0)) && self_.get("FILE_PT").truthy() {
            // read data from file in multiples of $CHUNK_SIZE
            num = pl::add(&ops::bit_or(&pl::sub(&num, &n(1)), &n(CHUNK_SIZE - 1)), &n(1));
            num = perl_read(it, &self_.get("FILE_PT"), &mut buff, &num)?;
            if num.truthy() {
                let bp = self_.get("BUFF_PT");
                set_deref(&bp, pl::concat(&bp.deref(), &buff))?;
                self_.set("LEN", pl::add(&self_.get("LEN"), &num));
            } else if num.is_undef() {
                let e = errno(it);
                self_.set("ERROR", e);
            }
        }
        // number of bytes left in data buffer
        num = pl::sub(&self_.get("LEN"), &self_.get("POS"));
        if pl::le(&len, &num) {
            rtn_val = len.clone();
        } else if pl::le(&num, &n(0)) {
            *buf = pl::s("");
            *set = true;
            return Ok(n(0));
        } else {
            rtn_val = num;
        }
        // return data from our buffer
        *buf = substr(it, &self_.get("BUFF_PT").deref(), &self_.get("POS"), Some(&rtn_val))?;
        *set = true;
        self_.set("POS", pl::add(&self_.get("POS"), &rtn_val));
    } else {
        // read directly from file
        if buf.is_undef() {
            *buf = pl::s("");
            *set = true;
        }
        rtn_val = perl_read(it, &self_.get("FILE_PT"), buf, &len)?;
        *set = true;
        if rtn_val.is_undef() {
            let e = errno(it);
            self_.set("ERROR", e);
            rtn_val = n(0);
        }
    }
    if self_.get("DEBUG").truthy() {
        let pos = pl::sub(&tell_impl(it, self_)?, &rtn_val);
        let dbg = self_.get("DEBUG");
        let cur = dbg.get(pos.to_bytes());
        if !(cur.truthy() && pl::gt(&cur, &rtn_val)) {
            dbg.set(pos.to_bytes(), rtn_val.clone());
        }
    }
    Ok(rtn_val)
}

/// Read from the file
/// Inputs: 0) reference to RandomAccess object, 1) buffer, 2) bytes to read
/// Returns: Number of bytes read
pub fn read(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let self_ = arg(&a, 0);
    let mut buf = arg(&a, 1);
    let mut set = false;
    let r = read_impl(it, &self_, &mut buf, &mut set, &arg(&a, 2))?;
    if set {
        it.set_arg(1, buf)?;
    }
    ret(r)
}

fn read_line_impl(it: &mut Interp, self_: &Value, buf: &mut Value, set: &mut bool) -> ER<Value> {
    let rtn_val;
    let fp = self_.get("FILE_PT");

    if pl::lt(&self_.get("TESTED"), &n(0)) {
        let (mut num, mut buff);
        if self_.get("NoBuffer").truthy() && !purge_impl(it, self_)?.truthy() {
            return Ok(n(0));
        }
        let mut pos = self_.get("POS");
        let eol = it.get_scalar("main::/");
        if fp.truthy() {
            // make sure we have some data after the current position
            while pl::le(&self_.get("LEN"), &pos) {
                buff = Value::Undef;
                num = perl_read(it, &fp, &mut buff, &n(CHUNK_SIZE))?;
                if !num.truthy() {
                    if num.is_undef() {
                        let e = errno(it);
                        self_.set("ERROR", e);
                    }
                    return Ok(n(0));
                }
                let bp = self_.get("BUFF_PT");
                set_deref(&bp, pl::concat(&bp.deref(), &buff))?;
                self_.set("LEN", pl::add(&self_.get("LEN"), &num));
            }
            // scan and read until we find the EOL (or hit EOF)
            loop {
                pos = index(&self_.get("BUFF_PT").deref(), &eol, Some(&pos));
                if pl::ge(&pos, &n(0)) {
                    pos = pl::add(&pos, &pl::length(&eol));
                    break;
                }
                pos = self_.get("LEN"); // have scanned to end of buffer
                buff = Value::Undef;
                num = perl_read(it, &fp, &mut buff, &n(CHUNK_SIZE))?;
                if !num.truthy() {
                    if num.is_undef() {
                        let e = errno(it);
                        self_.set("ERROR", e);
                    }
                    break;
                }
                let bp = self_.get("BUFF_PT");
                set_deref(&bp, pl::concat(&bp.deref(), &buff))?;
                self_.set("LEN", pl::add(&self_.get("LEN"), &num));
            }
        } else {
            // string i/o
            pos = index(&self_.get("BUFF_PT").deref(), &eol, Some(&pos));
            if pl::lt(&pos, &n(0)) {
                pos = self_.get("LEN");
                if pl::gt(&self_.get("POS"), &pos) {
                    self_.set("POS", pos.clone());
                }
            } else {
                pos = pl::add(&pos, &pl::length(&eol));
            }
        }
        // read the line from our buffer
        rtn_val = pl::sub(&pos, &self_.get("POS"));
        *buf = substr(it, &self_.get("BUFF_PT").deref(), &self_.get("POS"), Some(&rtn_val))?;
        *set = true;
        self_.set("POS", pos);
    } else {
        let f = cell(fp);
        *buf = perl_eval1(it, "my $line = <$fp>; $line", "File::RandomAccess", &[("fp", &f)])?;
        *set = true;
        if !buf.is_undef() {
            rtn_val = pl::length(buf);
        } else {
            rtn_val = n(0);
        }
    }
    if self_.get("DEBUG").truthy() {
        let pos = pl::sub(&tell_impl(it, self_)?, &rtn_val);
        let dbg = self_.get("DEBUG");
        let cur = dbg.get(pos.to_bytes());
        if !(cur.truthy() && pl::gt(&cur, &rtn_val)) {
            dbg.set(pos.to_bytes(), rtn_val.clone());
        }
    }
    Ok(rtn_val)
}

/// Read a line from file (end of line is $/)
/// Inputs: 0) reference to RandomAccess object, 1) buffer
/// Returns: Number of bytes read
pub fn read_line(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let self_ = arg(&a, 0);
    let mut buf = arg(&a, 1);
    let mut set = false;
    let r = read_line_impl(it, &self_, &mut buf, &mut set)?;
    if set {
        it.set_arg(1, buf)?;
    }
    ret(r)
}

fn slurp_impl(it: &mut Interp, self_: &Value) -> ER<()> {
    let fp = self_.get("FILE_PT");
    if !fp.truthy() {
        return Ok(());
    }
    // read whole file into buffer (in large chunks)
    loop {
        let mut buff = Value::Undef;
        let num = perl_read(it, &fp, &mut buff, &n(CHUNK_SIZE * SLURP_CHUNKS))?;
        if !num.truthy() {
            if num.is_undef() {
                let e = errno(it);
                self_.set("ERROR", e);
            }
            break;
        }
        let bp = self_.get("BUFF_PT");
        set_deref(&bp, pl::concat(&bp.deref(), &buff))?;
        self_.set("LEN", pl::add(&self_.get("LEN"), &num));
    }
    Ok(())
}

/// Read whole file into buffer (without changing read pointer)
/// Inputs: 0) reference to RandomAccess object
pub fn slurp(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    slurp_impl(it, &arg(&a, 0))?;
    pl::ret_nothing(w)
}

fn purge_impl(it: &mut Interp, self_: &Value) -> ER<Value> {
    if !self_.get("FILE_PT").truthy() {
        return Ok(n(1));
    }
    if pl::lt(&self_.get("POS"), &n(0)) {
        return Ok(n(0)); // error if we can't read from here
    }
    if pl::gt(&self_.get("POS"), &n(CHUNK_SIZE)) {
        let pos = self_.get("POS");
        let purge = pl::sub(&pos, &ops::modulo(&pos, &n(CHUNK_SIZE)).unwrap_or(Value::Int(0)));
        if pl::ge(&purge, &self_.get("LEN")) {
            // read up to current position in 64k chunks, discarding as we go
            while pl::gt(&self_.get("POS"), &self_.get("LEN")) {
                self_.set("BASE", pl::add(&self_.get("BASE"), &self_.get("LEN")));
                self_.set("POS", pl::sub(&self_.get("POS"), &self_.get("LEN")));
                let bp = self_.get("BUFF_PT");
                set_deref(&bp, pl::s(""))?;
                let mut b = bp.deref();
                let len = perl_read(it, &self_.get("FILE_PT"), &mut b, &n(SKIP_SIZE))?;
                set_deref(&bp, b)?;
                self_.set("LEN", len);
                if self_.get("LEN").is_undef() {
                    let e = errno(it);
                    self_.set("ERROR", e);
                    break;
                }
                if pl::lt(&self_.get("LEN"), &n(SKIP_SIZE)) {
                    break;
                }
            }
        } else if pl::gt(&purge, &n(0)) {
            let bp = self_.get("BUFF_PT");
            let rest = substr(it, &bp.deref(), &purge, None)?;
            set_deref(&bp, rest)?;
            self_.set("BASE", pl::add(&self_.get("BASE"), &purge));
            self_.set("POS", pl::sub(&self_.get("POS"), &purge));
            self_.set("LEN", pl::sub(&self_.get("LEN"), &purge));
        }
    }
    Ok(n(1))
}

/// Purge internal buffer [internal use only]
/// Inputs: 0) reference to RandomAccess object
/// Returns: 1 on success, or 0 if current buffer position is negative
/// Notes: This is called only in NoBuffer mode
pub fn purge(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(purge_impl(it, &arg(&a, 0))?)
}

/// Set binary mode
/// Inputs: 0) reference to RandomAccess object
pub fn bin_mode(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let self_ = arg(&a, 0);
    let fp = self_.get("FILE_PT");
    if fp.truthy() {
        let f = cell(fp);
        return ret(perl_eval1(it, "binmode($fp)", "File::RandomAccess", &[("fp", &f)])?);
    }
    ret(fp)
}

/// Close the file and free the buffer
/// Inputs: 0) reference to RandomAccess object
pub fn close(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let self_ = arg(&a, 0);

    if self_.get("DEBUG").truthy() {
        // local $_;
        let saved_us = it.get_scalar("main::_");
        it.set_scalar("main::_", Value::Undef);
        let r = close_debug(it, &self_);
        it.set_scalar("main::_", saved_us);
        r?;
        self_.delete("DEBUG");
    }
    // close the file
    if self_.get("FILE_PT").truthy() {
        let f = cell(self_.get("FILE_PT"));
        perl_eval(it, "close($fp)", "File::RandomAccess", &[("fp", &f)], Want::Void)?;
        self_.delete("FILE_PT");
    }
    // reset the buffer
    let empty_buff = Value::binary(b"");
    self_.set("BUFF_PT", empty_buff);
    self_.set("BASE", n(0));
    self_.set("LEN", n(0));
    self_.set("POS", n(0));
    ret(n(0))
}

/// The DEBUG report of Close.
fn close_debug(it: &mut Interp, self_: &Value) -> ER<()> {
    if seek_impl(it, self_, &n(0), &n(2))?.truthy() {
        let t = tell_impl(it, self_)?;
        self_.get("DEBUG").set(t.to_bytes(), n(0)); // set EOF marker
        let mut last = Value::Undef;
        let mut tot = n(0);
        let mut bad = n(0);
        let mut keys: Vec<Value> = self_.get("DEBUG").keys().into_iter().map(Value::from).collect();
        keys.sort_by(|a, b| ops::num_cmp(a, b).unwrap_or(std::cmp::Ordering::Equal));
        for k in keys {
            it.set_scalar("main::_", k.clone());
            let pos = k.clone();
            let len = self_.get("DEBUG").get(k.to_bytes());
            if !last.is_undef() && pl::lt(&last, &pos) {
                let bytes = pl::sub(&pos, &last);
                tot = pl::add(&tot, &bytes);
                seek_impl(it, self_, &last, &Value::Undef)?;
                let mut buff = Value::Undef;
                let mut set = false;
                read_impl(it, self_, &mut buff, &mut set, &bytes)?;
                let mut warn = pl::s("");
                if pl::re(r"[^\0]", "").is_match(&buff) {
                    bad = pl::add(&bad, &pl::sub(&pos, &last));
                    warn = pl::s(" - NON-ZERO!");
                }
                let fmt = pl::concat(&pl::s("0x%.8x - 0x%.8x (%d bytes)"), &pl::concat(&warn, &pl::s("\n")));
                let line = pl::sprintf_v(&fmt, &[last.clone(), pos.clone(), bytes]);
                print(it, line)?;
            }
            let cur = pl::add(&pos, &len);
            if !(!last.is_undef() && pl::gt(&last, &cur)) {
                last = cur;
            }
        }
        print(it, pl::concat(&tot, &pl::s(" bytes missed")))?;
        if bad.truthy() {
            print(it, pl::concat(&pl::concat(&pl::s(", "), &bad), &pl::s(" non-zero!")))?;
        }
        print(it, pl::s("\n"))?;
    } else {
        pl::warn(it, "File::RandomAccess DEBUG not working (file already closed?)\n")?;
    }
    Ok(())
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("File::RandomAccess::new", new);
    it.define_native("File::RandomAccess::Debug", debug);
    it.define_native("File::RandomAccess::SeekTest", seek_test);
    it.define_native("File::RandomAccess::Tell", tell);
    it.define_native("File::RandomAccess::Seek", seek);
    it.define_native("File::RandomAccess::Read", read);
    it.define_native("File::RandomAccess::ReadLine", read_line);
    it.define_native("File::RandomAccess::Slurp", slurp);
    it.define_native("File::RandomAccess::Purge", purge);
    it.define_native("File::RandomAccess::BinMode", bin_mode);
    it.define_native("File::RandomAccess::Close", close);
}
