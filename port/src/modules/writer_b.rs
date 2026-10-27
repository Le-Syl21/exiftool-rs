//! Image::ExifTool::Writer (Image/ExifTool/Writer.pl), half B of its subs, ported sub by sub (see docs/port/PORTING.md).
//!
//! The subs from `HexDump` to `WriteTIFF` (package Image::ExifTool).

use std::cell::RefCell;
use std::rc::Rc;

use crate::interp::eval::{die, Binding, Flow, ER};
use crate::interp::{Interp, Scope, Want};
use crate::modules::exiftool::part2;
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, re, ret};

const ET: &str = "Image::ExifTool";

fn n(i: i64) -> Value {
    Value::Int(i)
}

fn s(x: &str) -> Value {
    Value::str(x)
}

/// `$a || $b`
fn or(a: Value, b: Value) -> Value {
    if a.truthy() {
        a
    } else {
        b
    }
}

/// `$a . $b . ...`
fn cat(parts: &[&Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

/// A lexical of Writer.pl (`my $maxSegmentLen`...), as the loader keeps it.
fn wlex(it: &mut Interp, sigil: u8, name: &str) -> Binding {
    it.global(sigil, &format!("{ET}::<my:Writer.pl>::{name}"))
}

fn wlex_scalar(it: &mut Interp, name: &str) -> Rc<RefCell<Value>> {
    match wlex(it, b'$', name) {
        Binding::Scalar(c) => c,
        _ => unreachable!(),
    }
}

fn wlex_hash(it: &mut Interp, name: &str) -> Value {
    match wlex(it, b'%', name) {
        Binding::Hash(h) => Value::HashRef(h),
        _ => unreachable!(),
    }
}

/// `$Image::ExifTool::name`
fn pkg_scalar(it: &mut Interp, name: &str) -> Value {
    it.get_scalar(&format!("{ET}::{name}"))
}

/// `$$ref` of a SCALAR reference, dying as perl does under strict refs.
fn deref(r: &Value) -> ER<Value> {
    match r {
        Value::ScalarRef(_) | Value::LvRef(_) => Ok(r.deref()),
        Value::Undef => die("Can't use an undefined value as a SCALAR reference"),
        _ => die(format!("Can't use string (\"{}\") as a SCALAR ref while \"strict refs\" in use", r.to_text())),
    }
}

/// `$$ref = VALUE`
fn set_deref(r: &Value, v: Value) -> ER<()> {
    match r {
        Value::ScalarRef(_) | Value::LvRef(_) => pl::set_deref(r, v),
        Value::Undef => die("Can't use an undefined value as a SCALAR reference"),
        _ => die(format!("Can't use string (\"{}\") as a SCALAR ref while \"strict refs\" in use", r.to_text())),
    }
}

/// `substr($$ref, OFF, LEN) = VALUE`
fn substr_assign(r: &Value, off: &Value, len: &Value, v: &Value) -> ER<()> {
    let b = deref(r)?.to_bytes();
    let Some((a, e)) = crate::interp::eval::substr_range(b.len(), off.to_i64(), Some(len.to_i64())) else { return die("substr outside of string") };
    let mut out = b[..a].to_vec();
    out.extend_from_slice(&v.to_bytes());
    out.extend_from_slice(&b[e..]);
    set_deref(r, Value::bytes(out))
}

/// `substr($str, OFF, LEN)` with value arguments.
fn substr(v: &Value, off: &Value, len: Option<&Value>) -> Value {
    pl::substr(v, off.to_i64(), len.map(|l| l.to_i64()))
}

/// `%h = @list`: a hash from a list of key/value pairs.
fn hash_from(list: &[Value]) -> Value {
    let h = Value::new_hash();
    let mut i = 0;
    while i < list.len() {
        h.set(list[i].to_bytes(), arg(list, i + 1));
        i += 2;
    }
    h
}

/// `%$h` in list context (key, value, ...).
fn flatten(h: &Value) -> Vec<Value> {
    let mut v = Vec::new();
    for k in h.keys() {
        v.push(Value::Str(k.clone()));
        v.push(h.get(&k));
    }
    v
}

/// Run a Perl snippet for a builtin with no helper (`rand`, `tell`,
/// `utime`, `rename`...): the snippet sees exactly the lexicals given. `$@`
/// is left as it was; a die inside propagates as it would from the Perl.
fn snippet(it: &mut Interp, code: &str, vars: &[(&str, Value)], want: Want) -> ER<Vec<Value>> {
    let scope = Scope::new(None);
    for (name, v) in vars {
        scope.scalar(name, v.clone());
    }
    let saved_at = it.get_scalar("main::@");
    let had_exit = it.exit_code.is_some();
    let r = it.eval_string(code.as_bytes(), ET, &scope, want);
    it.set_scalar("main::@", saved_at);
    match r {
        Ok(v) => Ok(v),
        Err(e) => {
            if !had_exit {
                if let Some(c) = it.exit_code.take() {
                    return Err(Flow::Exit(c));
                }
            }
            Err(Flow::Die(e))
        }
    }
}

fn snippet1(it: &mut Interp, code: &str, vars: &[(&str, Value)]) -> ER<Value> {
    Ok(snippet(it, code, vars, Want::Scalar)?.pop().unwrap_or_default())
}

/// `Pkg::Func($a, $b)` where the sub writes its arguments (`$_[0] = ...`).
/// (`pl::call_mut` goes through `sub_ref`, whose wrapper takes the aliases
/// for itself, so a native or Perl target never sees them: this resolves the
/// sub first and calls it with the aliases.)
fn call_mut(it: &mut Interp, name: &str, args: &mut [Value], want: Want) -> ER<Vec<Value>> {
    let code = match it.sub(name) {
        Some(c) => c,
        None => return pl::call_mut(it, name, args, want),
    };
    let cells: Vec<_> = args.iter().map(|v| Rc::new(RefCell::new(v.clone()))).collect();
    let al = cells.iter().map(|c| Some(crate::interp::eval::Lv::Scalar(c.clone()))).collect();
    let r = it.call_with_aliases(&code, args.to_vec(), al, want);
    for (a, c) in args.iter_mut().zip(cells) {
        *a = c.borrow().clone();
    }
    r
}

/// `print $out LIST`
fn print(it: &mut Interp, out: &Value, items: &[Value]) -> ER<bool> {
    pl::print_to(it, Some(out), items)
}

/// A native of this crate called in scalar context (only for subs that
/// never write their arguments back through `$_[i]`).
fn c1(it: &mut Interp, f: fn(&mut Interp, Vec<Value>, Want) -> ER<Vec<Value>>, args: Vec<Value>) -> ER<Value> {
    Ok(f(it, args, Want::Scalar)?.pop().unwrap_or_default())
}

fn get_byte_order(it: &mut Interp) -> ER<Value> {
    c1(it, part2::get_byte_order, vec![])
}

fn set_byte_order(it: &mut Interp, order: Value) -> ER<Value> {
    c1(it, part2::set_byte_order, vec![order])
}

fn set16u(it: &mut Interp, args: Vec<Value>) -> ER<Value> {
    c1(it, part2::set16u, args)
}

fn set32u(it: &mut Interp, args: Vec<Value>) -> ER<Value> {
    c1(it, part2::set32u, args)
}

/// `$raf->Read($buff, $len)`
fn raf_read(it: &mut Interp, raf: &Value, buff: &mut Value, len: Value) -> ER<Value> {
    let mut a = [buff.clone(), len];
    let r = pl::method_mut(it, raf, "Read", &mut a, Want::Scalar)?;
    *buff = a[0].clone();
    Ok(r.into_iter().last().unwrap_or_default())
}

/// `$raf->ReadLine($buff)`
fn raf_readline(it: &mut Interp, raf: &Value, buff: &mut Value) -> ER<Value> {
    let mut a = [buff.clone()];
    let r = pl::method_mut(it, raf, "ReadLine", &mut a, Want::Scalar)?;
    *buff = a[0].clone();
    Ok(r.into_iter().last().unwrap_or_default())
}

/// `$raf->Seek($pos, $whence)`
fn raf_seek(it: &mut Interp, raf: &Value, pos: Value, whence: i64) -> ER<bool> {
    Ok(pl::method(it, raf, "Seek", vec![pos, n(whence)])?.truthy())
}

/// `$raf->Tell()`
fn raf_tell(it: &mut Interp, raf: &Value) -> ER<Value> {
    pl::method(it, raf, "Tell", vec![])
}

/// `$self->Warn(...)` / `$self->Error(...)` and the like, result ignored.
fn meth(it: &mut Interp, obj: &Value, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::method(it, obj, name, args)
}

/// `GetTagTable(NAME)`
fn get_tag_table(it: &mut Interp, name: &str) -> ER<Value> {
    pl::call(it, "Image::ExifTool::GetTagTable", vec![s(name)])
}

/// `hex($v)`
fn hex(v: &Value) -> Value {
    let b = v.to_bytes();
    let mut t: &[u8] = &b;
    if t.len() >= 2 && t[0] == b'0' && (t[1] == b'x' || t[1] == b'X') {
        t = &t[2..];
    } else if !t.is_empty() && (t[0] == b'x' || t[0] == b'X') {
        t = &t[1..];
    }
    let mut r: u64 = 0;
    for &c in t {
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            b'_' => continue,
            _ => break,
        };
        r = r.wrapping_mul(16).wrapping_add(d as u64);
    }
    ops::from_uv(r)
}

/// The current time in seconds (`time()`).
fn unix_time() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// `localtime($t)` in list context.
fn localtime(it: &mut Interp, t: i64) -> Vec<Value> {
    crate::interp::time::sync_tz(it);
    crate::interp::time::broken_down(t, true).iter().map(|&x| Value::Int(x)).collect()
}

// ---- HexDump ... ----

/// Dump data in hex and ASCII to console
/// Inputs: 0) data reference, 1) length or undef, 2-N) Options:
/// Options: Start => offset to start of data (default=0)
///          Addr => address to print for data start (default=DataPos+Base+Start)
///          DataPos => position of data within block (relative to Base)
///          Base => base offset for pointers from start of file
///          Width => width of printout (bytes, default=16)
///          Prefix => prefix to print at start of line (default='')
///          MaxLen => maximum length to dump
///          Out => output file reference
///          Len => data length
pub fn hex_dump(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let data_pt = arg(&a, 0);
    let mut len = arg(&a, 1);
    let opts = hash_from(a.get(2..).unwrap_or(&[]));
    let start = or(opts.get("Start"), n(0));
    let mut addr = opts.get("Addr");
    let mut wid = or(opts.get("Width"), n(16));
    let prefix = or(opts.get("Prefix"), s(""));
    let out = match opts.get("Out") {
        o if o.truthy() => o,
        _ => snippet1(it, "\\*STDOUT", &[])?,
    };
    let mut max_len = opts.get("MaxLen");
    let dat_len = pl::sub(&pl::length(&deref(&data_pt)?), &start);
    let mut more = Value::Undef;
    if !opts.get("Len").is_undef() {
        len = opts.get("Len");
    }

    if addr.is_undef() {
        addr = pl::add(&pl::add(&start, &or(opts.get("DataPos"), n(0))), &or(opts.get("Base"), n(0)));
    }
    if len.is_undef() {
        len = dat_len.clone();
    }
    if max_len.truthy() && pl::gt(&len, &max_len) {
        // print one line less to allow for $more line below
        max_len = pl::mul(&pl::int(&pl::div(&pl::sub(&max_len, &n(1)), &wid)?), &wid);
        more = pl::sub(&len, &max_len);
        len = max_len.clone();
    }
    if pl::gt(&len, &dat_len) {
        print(it, &out, &[cat(&[&prefix, &s("    Warning: Attempted dump outside data\n")])])?;
        print(it, &out, &[cat(&[&prefix, &s("    ("), &len, &s(" bytes specified, but only "), &dat_len, &s(" available)\n")])])?;
        len = dat_len.clone();
    }
    let format = pl::sprintf("%%-%ds", &[pl::mul(&wid, &n(3))]);
    let rep = |w: &Value| "H2".repeat(w.to_i64().max(0) as usize);
    let mut tmpl = rep(&wid); // ('(H2)*' would have been nice, but older perl versions don't support it)
    let mut i = n(0);
    while pl::lt(&i, &len) {
        if pl::gt(&wid, &pl::sub(&len, &i)) {
            wid = pl::sub(&len, &i);
            tmpl = rep(&wid);
        }
        let line = pl::sprintf_v(&cat(&[&prefix, &s("%8.4x: ")]), &[pl::add(&addr, &i)]);
        print(it, &out, &[line])?;
        let mut dat = substr(&deref(&data_pt)?, &pl::add(&i, &start), Some(&wid));
        let s_ = pl::join(" ", &pl::unpack(&tmpl, &dat)?);
        print(it, &out, &[pl::sprintf_v(&format, &[s_])])?;
        pl::tr(&mut dat, "\\x00-\\x1f\\x7f-\\xff", ".", "");
        print(it, &out, &[cat(&[&s("["), &dat, &s("]\n")])])?;
        i = pl::add(&i, &wid);
    }
    if more.truthy() {
        let r = print(it, &out, &[cat(&[&prefix, &s("    [snip "), &more, &s(" bytes]\n")])])?;
        return ret(Value::from(r));
    }
    ret(more)
}

/// Print verbose tag information
/// Inputs: 0) ExifTool object reference, 1) tag ID
///         2) tag info reference (or undef)
///         3-N) extra parms:
/// Parms: Index => Index of tag in menu (starting at 0)
///        Value => Tag value
///        DataPt => reference to value data block
///        DataPos => location of data block in file
///        Base => base added to all offsets
///        Size => length of value data within block
///        Format => value format string
///        Count => number of values
///        Extra => Extra Verbose=2 information to put after tag number
///        Table => Reference to tag table
///        Name => Name to use for unknown tag
///        --> plus any of these HexDump() options: Start, Addr, Width
pub fn verbose_info(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (self_, mut tag_id, tag_info) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let parms = hash_from(a.get(3..).unwrap_or(&[]));
    let verbose = self_.path(&["OPTIONS", "Verbose"]);
    let out = self_.path(&["OPTIONS", "TextOut"]);
    let (tag, mut line);
    let mut hex_id = Value::Undef;

    // generate hex number if tagID is numerical
    if !tag_id.is_undef() {
        if re(r"^\d+$", "").is_match(&tag_id) {
            let table = parms.get("Table");
            if table.truthy() && table.get("VARS").truthy() && table.get("ID_FMT").truthy() && table.path(&["VARS", "ID_FMT"]).eq_str("dec") {
                hex_id = tag_id.clone();
            } else {
                hex_id = pl::sprintf("0x%.4x", &[tag_id.clone()]);
            }
        }
    } else {
        tag_id = s("Unknown");
    }
    // get tag name
    if tag_info.truthy() && tag_info.get("Name").truthy() {
        tag = tag_info.get("Name");
    } else if parms.get("Name").truthy() {
        // (used for PNG Plus FPX tags)
        tag = parms.get("Name");
        hex_id = Value::Undef;
    } else {
        let mut prefix = Value::Undef;
        if parms.get("Table").truthy() {
            prefix = parms.get("Table").get("TAG_PREFIX");
        }
        if prefix.truthy() || !hex_id.is_undef() {
            if !prefix.truthy() {
                prefix = s("Unknown");
            }
            tag = cat(&[&prefix, &s("_"), if !hex_id.is_undef() { &hex_id } else { &tag_id }]);
        } else {
            tag = tag_id.clone();
        }
    }
    let data_pt = parms.get("DataPt");
    let mut size = parms.get("Size");
    if !(!size.is_undef() || !data_pt.truthy()) {
        size = pl::length(&deref(&data_pt)?);
    }
    let mut indent = self_.get("INDENT");

    // Level 1: print tag/value information
    line = indent.clone();
    let index = parms.get("Index");
    if !index.is_undef() {
        line = cat(&[&line, &index, &s(") ")]);
        if pl::lt(&pl::length(&index), &n(2)) {
            line = cat(&[&line, &s(" ")]);
        }
        indent = cat(&[&indent, &s("    ")]); // indent everything else to align with tag name
    }
    line = cat(&[&line, &tag]);
    if tag_info.truthy() && tag_info.get("SubDirectory").truthy() {
        line = cat(&[&line, &s(" (SubDirectory) -->")]);
    } else {
        let max_len = pl::sub(&n(90), &pl::length(&line));
        let mut val = parms.get("Value");
        if !val.is_undef() {
            if val.ref_type() == "ARRAY" {
                val = cat(&[&s("["), &pl::join(",", &val.items()), &s("]")]);
            }
            let p = meth(it, &self_, "Printable", vec![val, max_len])?;
            line = cat(&[&line, &s(" = "), &p]);
        } else if data_pt.truthy() {
            let start = or(parms.get("Start"), n(0));
            let sub = substr(&deref(&data_pt)?, &start, Some(&size));
            let p = meth(it, &self_, "Printable", vec![sub, max_len])?;
            line = cat(&[&line, &s(" = "), &p]);
        }
    }
    print(it, &out, &[cat(&[&line, &s("\n")])])?;

    // Level 2: print detailed information about the tag
    if pl::gt(&verbose, &n(1)) && (parms.get("Extra").truthy() || parms.get("Format").truthy() || parms.get("DataPt").truthy() || !size.is_undef() || re(r"/", "").is_match(&tag_id)) {
        line = cat(&[&indent, &s("- Tag ")]);
        if !hex_id.is_undef() {
            line = cat(&[&line, &hex_id]);
        } else {
            re(r"([\0-\x1f\x7f-\xff])", "").subst(&mut tag_id, true, |c| format!("\\x{:02x}", cap(c, 1).to_bytes()[0]).into_bytes());
            line = cat(&[&line, &s("'"), &tag_id, &s("'")]);
        }
        if !parms.get("Extra").is_undef() {
            line = cat(&[&line, &parms.get("Extra")]);
        }
        let format = parms.get("Format");
        if format.truthy() || !size.is_undef() {
            line = cat(&[&line, &s(" (")]);
            if !size.is_undef() {
                line = cat(&[&line, &size, &s(" bytes")]);
                if format.truthy() {
                    line = cat(&[&line, &s(", ")]);
                }
            }
            if format.truthy() {
                line = cat(&[&line, &format]);
                if parms.get("Count").truthy() {
                    line = cat(&[&line, &s("["), &parms.get("Count"), &s("]")]);
                }
            }
            line = cat(&[&line, &s(")")]);
        }
        if pl::gt(&verbose, &n(2)) && parms.get("DataPt").truthy() {
            line = cat(&[&line, &s(":")]);
        }
        print(it, &out, &[cat(&[&line, &s("\n")])])?;
    }

    // Level 3: do hex dump of value
    if pl::gt(&verbose, &n(2)) && parms.get("DataPt").truthy() && (!tag_info.truthy() || !tag_info.get("ReadFromRAF").truthy()) {
        parms.set("Out", out);
        parms.set("Prefix", indent);
        // limit dump length if Verbose < 5
        if pl::lt(&verbose, &n(5)) {
            parms.set("MaxLen", n(if pl::num_eq(&verbose, &n(3)) { 96 } else { 2048 }));
        }
        let mut args = vec![data_pt, size];
        args.extend(flatten(&parms));
        return hex_dump(it, args, want);
    }
    pl::ret_nothing(want)
}

/// Dump trailer information
/// Inputs: 0) ExifTool object ref, 1) dirInfo hash (RAF, DirName, DataPos, DirLen)
/// Notes: Restores current file position before returning
pub fn dump_trailer(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (self_, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let cur_pos = raf_tell(it, &raf)?;
    let trailer = or(dir_info.get("DirName"), s("Unknown"));
    let mut pos = dir_info.get("DataPos");
    let verbose = self_.path(&["OPTIONS", "Verbose"]);
    let html_dump = self_.get("HTML_DUMP");
    let (mut buff, mut buf2) = (Value::Undef, Value::Undef);
    let mut size = dir_info.get("DirLen");
    if pos.is_undef() {
        pos = cur_pos.clone();
    }

    // get full trailer size if not specified
    #[allow(clippy::never_loop)]
    loop {
        if !size.truthy() {
            if !raf_seek(it, &raf, n(0), 2)? {
                break;
            }
            size = pl::sub(&raf_tell(it, &raf)?, &pos);
            if !size.truthy() {
                break;
            }
        }
        if !raf_seek(it, &raf, pos.clone(), 0)? {
            break;
        }
        if html_dump.truthy() {
            let num = raf_read(it, &raf, &mut buff, size.clone())?;
            if !num.truthy() {
                return pl::ret_nothing(want);
            }
            let mut desc = cat(&[&trailer, &s(" trailer")]);
            if pl::str_eq(&trailer, "Unknown") {
                desc = cat(&[&s("["), &desc, &s("]")]);
            }
            meth(it, &self_, "HDump", vec![pos.clone(), num, desc, Value::Undef, n(0x08)])?;
            break;
        }
        let out = self_.path(&["OPTIONS", "TextOut"]);
        let line = pl::sprintf_v(&cat(&[&trailer, &s(" trailer (%d bytes at offset 0x%.4x):\n")]), &[size.clone(), pos.clone()]);
        print(it, &out, &[line])?;
        if !pl::gt(&verbose, &n(2)) {
            break;
        }
        let mut num = size.clone(); // number of bytes to read
        // limit size if not very verbose
        if pl::lt(&verbose, &n(5)) {
            let limit = n(if pl::lt(&verbose, &n(4)) { 96 } else { 512 });
            if pl::gt(&num, &limit) {
                num = limit;
            }
        }
        if !pl::num_eq(&raf_read(it, &raf, &mut buff, num.clone())?, &num) {
            return pl::ret_nothing(want);
        }
        // read the end of the trailer too if not done already
        if pl::gt(&size, &pl::mul(&n(2), &num)) {
            raf_seek(it, &raf, pl::sub(&pl::add(&pos, &size), &num), 0)?;
            raf_read(it, &raf, &mut buf2, num.clone())?;
        } else if pl::gt(&size, &num) {
            raf_seek(it, &raf, pl::add(&pos, &num), 0)?;
            raf_read(it, &raf, &mut buf2, pl::sub(&size, &num))?;
            buff = cat(&[&buff, &buf2]);
            buf2 = Value::Undef;
        }
        hex_dump(it, vec![pl::sref(buff.clone()), Value::Undef, s("Addr"), pos.clone(), s("Out"), out.clone()], Want::Void)?;
        if !buf2.is_undef() {
            print(it, &out, &[s("    [snip "), pl::sub(&size, &pl::mul(&num, &n(2))), s(" bytes]\n")])?;
            hex_dump(it, vec![pl::sref(buf2.clone()), Value::Undef, s("Addr"), pl::sub(&pl::add(&pos, &size), &num), s("Out"), out.clone()], Want::Void)?;
        }
        break;
    }
    let r = raf_seek(it, &raf, cur_pos, 0)?;
    ret(Value::from(r))
}

/// Dump unknown trailer information
/// Inputs: 0) ExifTool ref, 1) dirInfo ref (with RAF, DataPos and DirLen defined)
/// Notes: changes dirInfo elements
pub fn dump_unknown_trailer(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (self_, dir_info) = (arg(&a, 0), arg(&a, 1));
    let mut pos = dir_info.get("DataPos");
    let end_pos = pl::add(&pos, &dir_info.get("DirLen"));
    // account for preview/MPF image trailer
    let value = self_.get("VALUE");
    let mut pre_pos = or(value.get("PreviewImageStart"), self_.get("PreviewImageStart"));
    let mut pre_len = or(value.get("PreviewImageLength"), self_.get("PreviewImageLength"));
    let hid_pos = value.get("HiddenDataOffset");
    let hid_len = value.get("HiddenDataLength");
    let mut tag = s("PreviewImage");
    let mut mp_image_num = 0i64;
    let image = Value::new_hash();
    let mut last_one = false;
    // add HiddenData to list of known trailer blocks
    if hid_pos.truthy() && hid_len.truthy() {
        // call ReadHiddenData to validate hidden data and fix offset if necessary
        crate::runtime::require(it, "Image::ExifTool::Sony")?;
        let mut args = [self_.clone(), hid_pos, hid_len.clone()];
        let dat_pt = call_mut(it, "Image::ExifTool::Sony::ReadHiddenData", &mut args, Want::Scalar)?.pop().unwrap_or_default();
        let hid_pos = args[1].clone();
        if dat_pt.truthy() {
            image.set(hid_pos.to_bytes(), Value::new_array(vec![s("HiddenData"), hid_len]));
        }
    }
    loop {
        // add to Preview block list if valid and in the trailer
        if pre_pos.truthy() && pre_len.truthy() && pl::gt(&pl::add(&pre_pos, &pre_len), &pos) {
            image.set(pre_pos.to_bytes(), Value::new_array(vec![tag.clone(), pre_len.clone()]));
        }
        if last_one {
            break; // checked all images
        }
        // look for MPF images (in the proper order)
        mp_image_num += 1;
        pre_pos = value.get(format!("MPImageStart ({mp_image_num})"));
        if !pre_pos.is_undef() {
            pre_len = value.get(format!("MPImageLength ({mp_image_num})"));
        } else {
            pre_pos = value.get("MPImageStart");
            pre_len = value.get("MPImageLength");
            last_one = true;
        }
        tag = s(&format!("MPImage{mp_image_num}"));
    }
    // dump trailer sections in order
    image.set(end_pos.to_bytes(), Value::new_array(vec![s(""), n(0)])); // add terminator "image"
    let mut keys: Vec<Value> = image.keys().into_iter().map(Value::Str).collect();
    keys.sort_by(|x, y| ops::num_cmp(x, y).unwrap_or(std::cmp::Ordering::Equal));
    for pre_pos in keys {
        if pl::lt(&pos, &pre_pos) {
            // dump unknown trailer data
            dir_info.set("DirName", s("Unknown"));
            dir_info.set("DataPos", pos.clone());
            dir_info.set("DirLen", pl::sub(&pre_pos, &pos));
            dump_trailer(it, vec![self_.clone(), dir_info.clone()], Want::Void)?;
        }
        let e = image.get(pre_pos.to_bytes());
        tag = e.at(0);
        pre_len = e.at(1);
        if !pre_len.truthy() {
            break;
        }
        // dump image if verbose (it is htmlDump'd by ExtractImage)
        if self_.path(&["OPTIONS", "Verbose"]).truthy() {
            dir_info.set("DirName", tag.clone());
            dir_info.set("DataPos", pre_pos.clone());
            dir_info.set("DirLen", pre_len.clone());
            dump_trailer(it, vec![self_.clone(), dir_info.clone()], Want::Void)?;
        }
        pos = pl::add(&pre_pos, &pre_len);
    }
    pl::ret_nothing(want)
}

/// Find last element in linked list
/// Inputs: 0) element in list
/// Returns: Last element in list
pub fn last_in_list(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut element = arg(&a, 0);
    while element.get("Next").truthy() {
        element = element.get("Next");
    }
    ret(element)
}

/// Print verbose value while writing
/// Inputs: 0) ExifTool object ref, 1) heading "eg. '+ IPTC:Keywords',
///         2) value, 3) [optional] extra text after value
pub fn verbose_value(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    if !pl::gt(&arg(&a, 0).path(&["OPTIONS", "Verbose"]), &n(1)) {
        return pl::ret_nothing(want);
    }
    let (self_, str_, mut val, mut xtra) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    let out = self_.path(&["OPTIONS", "TextOut"]);
    if !xtra.truthy() {
        xtra = s("");
    }
    let max_len = pl::sub(&pl::sub(&n(81), &pl::length(&str_)), &pl::length(&xtra));
    val = meth(it, &self_, "Printable", vec![val, max_len])?;
    let r = print(it, &out, &[cat(&[&s("    "), &str_, &s(" = '"), &val, &s("'"), &xtra, &s("\n")])])?;
    ret(Value::from(r))
}

/// Pack Unicode numbers into UTF8 string
/// Inputs: 0-N) list of Unicode numbers
/// Returns: Packed UTF-8 string
pub fn pack_utf8(_it: &mut Interp, mut a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut out: Vec<Value> = Vec::new();
    let band = |x: &Value, m: i64| ops::bit_and(x, &n(m));
    let bor = |m: i64, x: &Value| ops::bit_or(&n(m), x);
    let shr6 = |x: &Value| ops::shift_right(x, &n(6));
    while let Some(mut ch) = a.pop() {
        if pl::lt(&ch, &n(0x80)) {
            out.insert(0, ch);
            continue;
        }
        out.insert(0, bor(0x80, &band(&ch, 0x3f)));
        ch = shr6(&ch);
        if pl::lt(&ch, &n(0x20)) {
            out.insert(0, bor(0xc0, &ch));
            continue;
        }
        out.insert(0, bor(0x80, &band(&ch, 0x3f)));
        ch = shr6(&ch);
        if pl::lt(&ch, &n(0x10)) {
            out.insert(0, bor(0xe0, &ch));
            continue;
        }
        out.insert(0, bor(0x80, &band(&ch, 0x3f)));
        ch = shr6(&ch);
        out.insert(0, bor(0xf0, &band(&ch, 0x07)));
    }
    ret(pl::pack("C*", &out)?)
}

/// Unpack numbers from UTF8 string
/// Inputs: 0) UTF-8 string
/// Returns: List of Unicode numbers (sets $evalWarning on error)
pub fn unpack_utf8(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let str_ = arg(&a, 0).to_bytes();
    let mut out: Vec<Value> = Vec::new();
    // pos($_[0]) = $pos = 0;  # start at beginning of string
    let mut pos = 0usize;
    let mut search = 0usize; // pos($_[0])
    loop {
        let (mut ch, new_pos): (Option<u32>, usize);
        ch = None;
        if let Some(p) = str_[search.min(str_.len())..].iter().position(|&b| b >= 0x80) {
            ch = Some(str_[search + p] as u32);
            new_pos = search + p; // pos($_[0]) - 1
            search = new_pos + 1;
        } else {
            new_pos = str_.len();
        }
        // unpack 7-bit characters
        let len = new_pos - pos;
        if len > 0 {
            out.extend(str_[pos..new_pos].iter().map(|&b| n(b as i64)));
        }
        let Some(ch) = ch else { break };
        pos = new_pos + 1;
        // minimum lead byte for 2-byte sequence is 0xc2 (overlong sequences
        // not allowed), 0xf8-0xfd are restricted by RFC 3629 (no 5 or 6 byte
        // sequences), and 0xfe and 0xff are not valid in UTF-8 strings
        if !(0xc2..0xf8).contains(&ch) {
            out.push(n(b'?' as i64)); // invalid UTF-8
            it.set_scalar(&format!("{ET}::evalWarning"), s("Bad UTF-8"));
            continue;
        }
        // decode 2, 3 and 4-byte sequences
        let mut nb = 1usize;
        let mut val: i64;
        if ch < 0xe0 {
            val = (ch & 0x1f) as i64; // 2-byte sequence
        } else if ch < 0xf0 {
            val = (ch & 0x0f) as i64; // 3-byte sequence
            nb += 1;
        } else {
            val = (ch & 0x07) as i64; // 4-byte sequence
            nb += 2;
        }
        // unless ($_[0] =~ /\G([\x80-\xbf]{$n})/g)
        let ok = search + nb <= str_.len() && str_[search..search + nb].iter().all(|&b| (0x80..=0xbf).contains(&b));
        if !ok {
            search = pos; // restore position
            out.push(n(b'?' as i64)); // invalid UTF-8
            it.set_scalar(&format!("{ET}::evalWarning"), s("Bad UTF-8"));
            continue;
        }
        for &byte in &str_[search..search + nb] {
            val = (val << 6) | (byte as i64 & 0x3f);
        }
        search += nb;
        out.push(n(val)); // save Unicode character value
        pos += nb; // position at end of UTF-8 character
    }
    if want != Want::List {
        return ret(n(out.len() as i64));
    }
    Ok(out)
}

/// Generate a new, random GUID
/// Inputs: <none>
/// Returns: GUID string
pub fn new_guid(it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let tm = localtime(it, unix_time());
    let guid_count = wlex_scalar(it, "guidCount");
    let gc = guid_count.borrow().clone();
    let inc = pl::add(&gc, &n(1));
    if !gc.is_undef() {
        *guid_count.borrow_mut() = inc.clone();
    }
    if !(!gc.is_undef() && pl::lt(&inc, &n(0x100))) {
        *guid_count.borrow_mut() = n(0);
    }
    let gc = guid_count.borrow().clone();
    let r = snippet(it, "(rand(0x10000), rand(0x10000), rand(0x10000))", &[], Want::List)?;
    ret(pl::sprintf(
        "%.4d%.2d%.2d%.2d%.2d%.2d%.2X%.4X%.4X%.4X%.4X",
        &[pl::add(&tm[5], &n(1900)), pl::add(&tm[4], &n(1)), tm[3].clone(), tm[2].clone(), tm[1].clone(), tm[0].clone(), gc, n(std::process::id() as i64 & 0xffff), arg(&r, 0), arg(&r, 1), arg(&r, 2)],
    ))
}

/// Generate a version 4 ("random"), variant 1 UUID (RFC 9562, Section 5.4) (github #421)
/// Inputs: <none>
/// Returns: UUID string
pub fn new_uuid(it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut rnd: Vec<i64> = snippet(it, "map {int(rand(256))} 1 .. 16", &[], Want::List)?.iter().map(|v| v.to_i64()).collect();
    rnd.resize(16, 0);
    rnd[6] = (rnd[6] & 0x0f) | 0x40;
    rnd[8] = (rnd[8] & 0x3f) | 0x80;
    let bytes: Vec<u8> = rnd.iter().map(|&x| x as u8).collect();
    ret(pl::uc(&pl::unpack1("H32", &Value::bytes(bytes))?))
}

/// Make TIFF header for raw data
/// Inputs: 0) width, 1) height, 2) num colour components, 3) bits, 4) resolution
///         5) color-map data for palette-color image (8 or 16 bit)
/// Returns: TIFF header
/// Notes: Multi-byte data must be little-endian
pub fn make_tiff_header(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (w, h, cols, bits, mut res, mut cmap) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4), arg(&a, 5));
    if !res.truthy() {
        res = n(72);
    }
    let save_order = get_byte_order(it)?;
    set_byte_order(it, s("II"))?;
    let pow2 = ops::pow(&n(2), &bits);
    if !cmap.truthy() {
        cmap = s("");
    } else if pl::num_eq(&pl::length(&cmap), &pl::mul(&n(3), &pow2)) {
        // convert to short
        let c: Vec<Value> = pl::unpack("C*", &cmap)?.iter().map(|x| ops::bit_or(x, &ops::shift_left(x, &n(8)))).collect();
        cmap = pl::pack("v*", &c)?;
    } else if !pl::num_eq(&pl::length(&cmap), &pl::mul(&n(6), &pow2)) {
        cmap = s("");
    }
    let cmo = n(if cmap.truthy() { 12 } else { 0 }); // offset due to ColorMap IFD entry
    let b = |x: &[u8]| Value::bytes(x);
    let mut hdr: Vec<Value> = vec![
        b(b"\x49\x49\x2a\0\x08\0\0\0\x0e\0"),       // 0x00 14 menu entries:
        b(b"\xfe\x00\x04\0\x01\0\0\0\x00\0\0\0"),   // 0x0a SubfileType = 0
        b(b"\x00\x01\x04\0\x01\0\0\0"),
        set32u(it, vec![w.clone()])?,                // 0x16 ImageWidth
        b(b"\x01\x01\x04\0\x01\0\0\0"),
        set32u(it, vec![h.clone()])?,                // 0x22 ImageHeight
        b(b"\x02\x01\x03\0"),
        set32u(it, vec![cols.clone()])?,             // 0x2e BitsPerSample
        set32u(it, vec![if pl::num_eq(&cols, &n(1)) { bits.clone() } else { pl::add(&n(0xb6), &cmo) }])?,
        b(b"\x03\x01\x03\0\x01\0\0\0\x01\0\0\0"),   // 0x3a Compression = 1
        b(b"\x06\x01\x03\0\x01\0\0\0"),             // 0x46 PhotometricInterpretation
        set32u(it, vec![n(if cmap.truthy() { 3 } else if pl::num_eq(&cols, &n(1)) { 1 } else { 2 })])?,
        b(b"\x11\x01\x04\0\x01\0\0\0"),             // 0x52 StripOffsets
        set32u(it, vec![pl::add(&pl::add(&n(0xcc), &cmo), &pl::length(&cmap))])?,
        b(b"\x15\x01\x03\0\x01\0\0\0"),
        set32u(it, vec![cols.clone()])?,             // 0x5e SamplesPerPixel
        b(b"\x16\x01\x04\0\x01\0\0\0"),
        set32u(it, vec![h.clone()])?,                // 0x6a RowsPerStrip
        b(b"\x17\x01\x04\0\x01\0\0\0"),             // 0x76 StripByteCounts
        set32u(it, vec![pl::mul(&pl::mul(&pl::mul(&w, &h), &cols), &pl::int(&pl::div(&pl::add(&bits, &n(7)), &n(8))?))])?,
        b(b"\x1a\x01\x05\0\x01\0\0\0"),
        set32u(it, vec![pl::add(&n(0xbc), &cmo)])?,  // 0x82 XResolution
        b(b"\x1b\x01\x05\0\x01\0\0\0"),
        set32u(it, vec![pl::add(&n(0xc4), &cmo)])?,  // 0x8e YResolution
        b(b"\x1c\x01\x03\0\x01\0\0\0\x01\0\0\0"),   // 0x9a PlanarConfiguration = 1
        b(b"\x28\x01\x03\0\x01\0\0\0\x02\0\0\0"),   // 0xa6 ResolutionUnit = 2
    ];
    if cmap.truthy() {
        // 0xb2 ColorMap [optional]
        hdr.push(b(b"\x40\x01\x03\0"));
        hdr.push(set32u(it, vec![pl::mul(&n(3), &pow2)])?);
        hdr.push(b(b"\xd8\0\0\0"));
    }
    hdr.push(b(b"\0\0\0\0")); // 0xb2+$cmo (no IFD1)
    let bps = set16u(it, vec![bits.clone()])?;
    hdr.extend([bps.clone(), bps.clone(), bps]); // 0xb6+$cmo BitsPerSample value
    hdr.push(set32u(it, vec![res.clone()])?);
    hdr.push(b(b"\x01\0\0\0")); // 0xbc+$cmo XResolution = 72
    hdr.push(set32u(it, vec![res])?);
    hdr.push(b(b"\x01\0\0\0")); // 0xc4+$cmo YResolution = 72
    hdr.push(cmap); // 0xcc or 0xd8 (cmap and data go here)
    let hdr = pl::join("", &hdr);
    set_byte_order(it, save_order)?;
    ret(hdr)
}

/// Return current time in EXIF format
/// Inputs: 0) [optional] ExifTool ref, 1) flag to include timezone (0 to disable,
///            undef or 1 to include)
/// Returns: time string
/// - a consistent value is returned for each processed file
pub fn time_now_sub(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (mut self_, mut tz_flag) = (arg(&a, 0), arg(&a, 1));
    let mut time_now;
    if !self_.is_ref() {
        tz_flag = self_;
        self_ = Value::new_hash();
    }
    if self_.get("Now").truthy() {
        time_now = self_.get("Now").at(0);
    } else {
        let time = unix_time();
        let tm = localtime(it, time);
        let tz = pl::call(it, "Image::ExifTool::TimeZoneString", vec![Value::new_array(tm.clone()), n(time)])?;
        time_now = pl::sprintf("%4d:%.2d:%.2d %.2d:%.2d:%.2d", &[pl::add(&tm[5], &n(1900)), pl::add(&tm[4], &n(1)), tm[3].clone(), tm[2].clone(), tm[1].clone(), tm[0].clone()]);
        self_.set("Now", Value::new_array(vec![time_now.clone(), tz]));
    }
    if tz_flag.truthy() || tz_flag.is_undef() {
        time_now = cat(&[&time_now, &self_.get("Now").at(1)]);
    }
    ret(time_now)
}

/// The end of the first match of `pat` in `subject` and its captures
/// (`$subject =~ /PAT/g` in scalar context, from the start).
fn match_g(pat: &str, flags: &str, subject: &Value) -> ER<Option<(usize, pl::Caps)>> {
    let r = match crate::perl::regex::compile(pat.as_bytes(), flags) {
        Ok(r) => r,
        Err(e) => return die(e),
    };
    let b = subject.to_bytes();
    match r.match_at(&b, 0) {
        Ok(Some(sp)) => {
            let end = sp[0].map(|(_, e)| e).unwrap_or(0);
            let caps = sp.iter().map(|g| g.map(|(x, y)| Value::bytes(&b[x..y]))).collect();
            Ok(Some((end, caps)))
        }
        _ => Ok(None),
    }
}

/// `($subject =~ /PAT/g)` in list context starting at `pos` (no groups).
fn all_from(pat: &str, subject: &Value, pos: usize) -> ER<Vec<Value>> {
    let r = match crate::perl::regex::compile(pat.as_bytes(), "") {
        Ok(r) => r,
        Err(e) => return die(e),
    };
    let b = subject.to_bytes();
    let mut out = Vec::new();
    if let Ok(all) = r.match_all(&b, pos) {
        for sp in all {
            let (x, y) = sp[0].unwrap();
            out.push(Value::bytes(&b[x..y]));
        }
    }
    Ok(out)
}

/// Inverse date/time print conversion (reformat to YYYY:mm:dd HH:MM:SS[.ss][+-HH:MM|Z])
/// Inputs: 0) ExifTool object ref, 1) Date/Time string, 2) timezone flag:
///               0     - remove timezone and sub-seconds if they exist
///               1     - add timezone if it doesn't exist
///               undef - leave timezone alone
///         3) flag to allow date-only (YYYY, YYYY:mm or YYYY:mm:dd) or time without seconds
/// Returns: formatted date/time string (or undef and issues warning on error)
/// Notes: currently accepts different separators, but doesn't use DateFormat yet
pub fn inverse_date_time(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, mut val, tz_flag, date_only) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    let mut rtn_val = Value::Undef;
    let mut tz;
    let mut fs = Value::Undef;
    let mut fmt = self_.path(&["OPTIONS", "DateFormat"]);
    // strip off timezone first if it exists
    let mut caps = None;
    if !fmt.truthy() && re(r"([-+])(\d{1,2}):?(\d{2})\s*(DST)?$", "i").subst(&mut val, false, |c| {
        caps = Some(c.clone());
        Vec::new()
    }) > 0
    {
        let c = caps.unwrap();
        tz = pl::sprintf_v(&cat(&[&cap(&c, 1), &s("%.2d:"), &cap(&c, 3)]), &[cap(&c, 2)]);
    } else if !fmt.truthy() && re(r"Z$", "i").subst(&mut val, false, |_| Vec::new()) > 0 {
        tz = s("Z");
    } else {
        tz = s("");
        // allow special value of 'now'
        if pl::str_eq(&pl::lc(&val), "now") {
            return ret(meth(it, &self_, "TimeNow", vec![tz_flag])?);
        }
    }
    // only convert date if a format was specified and the date is recognizable
    if fmt.truthy() {
        let strptime_lib = wlex_scalar(it, "strptimeLib");
        if strptime_lib.borrow().is_undef() {
            let v = if pl::eval_block(it, |it| crate::runtime::require(it, "POSIX::strptime"))?.is_some() {
                s("POSIX::strptime")
            } else if pl::eval_block(it, |it| crate::runtime::require(it, "Time::Piece"))?.is_some() {
                // (call use_locale() to convert localized date/time,
                //  only available in Time::Piece 1.32 and later)
                pl::eval_block(it, |it| snippet(it, "Time::Piece->use_locale()", &[], Want::Void))?;
                s("Time::Piece")
            } else {
                s("")
            };
            *strptime_lib.borrow_mut() = v;
        }
        // handle fractional seconds (%f) and time zone (%z)
        fs = s("");
        tz = s("");
        if re(r"%(f|:?z)", "").is_match(&fmt) {
            if re(r"(.*[^%])%f", "").subst(&mut fmt, false, |c| cap(c, 1).to_bytes().to_vec()) > 0 {
                let mut f2 = Value::Undef;
                if re(r"(.*)(\.\d+)", "").subst(&mut val, false, |c| {
                    f2 = cap(c, 2);
                    cap(c, 1).to_bytes().to_vec()
                }) > 0
                {
                    fs = f2; // (take last .### as fractional seconds)
                }
            }
            let mut colon = Value::Undef;
            if re(r"(.*[^%])%(:?)z", "").subst(&mut fmt, false, |c| {
                colon = cap(c, 2);
                cap(c, 1).to_bytes().to_vec()
            }) > 0
            {
                let mut p = b"(.*)([-+]\\d{2})".to_vec();
                p.extend_from_slice(&colon.to_bytes());
                p.extend_from_slice(b"(\\d{2})");
                let mut t2 = Value::Undef;
                if pl::re_dyn(&p, "")?.subst(&mut val, false, |c| {
                    t2 = cat(&[&cap(c, 2), &s(":"), &cap(c, 3)]);
                    cap(c, 1).to_bytes().to_vec()
                }) > 0
                {
                    tz = t2;
                }
            }
        }
        let mut lib = strptime_lib.borrow().clone();
        let mut wrn = Value::Undef;
        let mut a: Vec<Value>;
        // TryLib: for ($lib=$strptimeLib; ; $lib='')
        'try_lib: loop {
            if !(pl::str_eq(&fmt, "%s")) {
                if !lib.truthy() {
                    if !self_.path(&["OPTIONS", "StrictDate"]).truthy() {
                        break 'try_lib;
                    }
                    let msg = or(wrn.clone(), s("Install POSIX::strptime or Time::Piece for inverse date/time conversions\n"));
                    pl::warn(it, &msg.to_text())?;
                    return pl::ret_undef();
                } else if pl::str_eq(&lib, "POSIX::strptime") {
                    let (v, f) = (val.clone(), fmt.clone());
                    a = pl::eval_block(it, |it| pl::call_list(it, "POSIX::strptime", vec![v, f]))?.unwrap_or_default();
                } else {
                    // protect against a negative epoch time, it can cause a hard crash in Windows
                    if pl::str_eq(&it.get_scalar("main::^O"), "MSWin32") && re(r"%s", "").is_match(&fmt) && re(r"-\d", "").is_match(&val) {
                        pl::warn(it, "Can't convert negative epoch time\n")?;
                        return pl::ret_undef();
                    }
                    let (v, f) = (val.clone(), fmt.clone());
                    a = pl::eval_block(it, |it| {
                        snippet(it, "my $t = Time::Piece->strptime($val, $fmt); ($t->sec, $t->min, $t->hour, $t->mday, $t->_mon, $t->_year)", &[("val", v), ("fmt", f)], Want::List)
                    })?
                    .unwrap_or_default();
                }
                if a.len() > 5 && !a[5].is_undef() && pl::length(&a[5]).to_i64() > 0 {
                    a[5] = pl::add(&a[5], &n(1900)); // add 1900 to year
                } else {
                    wrn = s(&format!("Invalid date/time (no year) using {}\n", lib.to_text()));
                    lib = s("");
                    continue 'try_lib;
                }
                if a.len() > 4 && !a[4].is_undef() && pl::length(&a[4]).to_i64() > 0 {
                    a[4] = pl::add(&a[4], &n(1)); // add 1 to month
                }
                for i in 0..=4 {
                    if a.len() <= i {
                        a.resize(i + 1, Value::Undef);
                    }
                    if a[i].is_undef() || pl::length(&a[i]).to_i64() == 0 {
                        if i < 2 || date_only.truthy() {
                            // (allow missing minutes/seconds)
                            a[i] = s("  ");
                        } else {
                            wrn = s(&format!("Incomplete date/time specification using {}\n", lib.to_text()));
                            lib = s("");
                            continue 'try_lib;
                        }
                    } else if pl::lt(&pl::length(&a[i]), &n(2)) {
                        a[i] = cat(&[&s("0"), &a[i]]); // pad to 2 digits if necessary
                    }
                }
                val = cat(&[&pl::join(":", &[a[5].clone(), a[4].clone(), a[3].clone()]), &s(" "), &pl::join(":", &[a[2].clone(), a[1].clone(), a[0].clone()]), &fs, &tz]);
                break 'try_lib;
            }
            // handle %s format ourself (not supported in Fedora, see forum15032)
            val = pl::call(it, "Image::ExifTool::ConvertUnixTime", vec![val, n(1)])?;
            break 'try_lib;
        }
    }
    if let Some((end, c)) = match_g(r"(\d{4})", "", &val)? {
        // get YYYY
        let yr = cap(&c, 1);
        let mut a = all_from(r"\d{1,2}", &val, end)?; // get mm, dd, HH, and maybe MM, SS
        for x in a.iter_mut() {
            if pl::lt(&pl::length(x), &n(2)) {
                *x = cat(&[&s("0"), x]); // pad to 2 digits if necessary
            }
        }
        if a.len() >= 3 {
            let mut ss = arg(&a, 4); // get SS
            while a.len() < 5 {
                a.push(s("00")); // add MM, SS if not given
            }
            // get sub-seconds if they exist (must be after SS, and have leading ".")
            if !fmt.truthy() {
                fs = s("");
                if a.len() > 5 {
                    if let Some(m) = re(r"(\.\d+)\s*$", "").m(&val) {
                        fs = cap(&m, 1);
                    }
                }
            }
            // add/remove timezone if necessary
            if tz_flag.truthy() {
                if !tz.truthy() {
                    if pl::eval_block(it, |it| crate::runtime::require(it, "Time::Local"))?.is_some() {
                        // determine timezone offset for this time
                        let args = vec![a[4].clone(), a[3].clone(), a[2].clone(), a[1].clone(), pl::sub(&a[0], &n(1)), yr.clone()];
                        let gm = pl::call(it, "Time::Local::timegm", args.clone())?;
                        let lt = pl::call(it, "Image::ExifTool::TimeLocal", args)?;
                        let diff = pl::sub(&gm, &lt);
                        tz = pl::call(it, "Image::ExifTool::TimeZoneString", vec![pl::div(&diff, &n(60))?])?;
                    } else {
                        tz = s("Z"); // don't know time zone
                    }
                }
            } else if !tz_flag.is_undef() {
                tz = s("");
                fs = s(""); // remove timezone and sub-seconds
            }
            if !ss.is_undef() && pl::lt(&ss, &n(60)) {
                ss = cat(&[&s(":"), &ss]);
            } else if date_only.truthy() {
                ss = s("");
            } else {
                ss = s(":00");
            }
            // construct properly formatted date/time string
            if pl::lt(&a[0], &n(1)) || pl::gt(&a[0], &n(12)) {
                pl::warn(it, &format!("Month '{}' out of range 1..12\n", a[0].to_text()))?;
                return pl::ret_undef();
            }
            if pl::lt(&a[1], &n(1)) || pl::gt(&a[1], &n(31)) {
                pl::warn(it, &format!("Day '{}' out of range 1..31\n", a[1].to_text()))?;
                return pl::ret_undef();
            }
            if pl::gt(&a[2], &n(24)) {
                pl::warn(it, &format!("Hour '{}' out of range 0..24\n", a[2].to_text()))?;
                return pl::ret_undef();
            }
            if pl::gt(&a[3], &n(59)) {
                pl::warn(it, &format!("Minutes '{}' out of range 0..59\n", a[3].to_text()))?;
                return pl::ret_undef();
            }
            rtn_val = cat(&[&yr, &s(":"), &a[0], &s(":"), &a[1], &s(" "), &a[2], &s(":"), &a[3], &ss, &fs, &tz]);
        } else if date_only.truthy() {
            let mut l = vec![yr];
            l.extend(a);
            rtn_val = pl::join(":", &l);
        }
    }
    if !rtn_val.truthy() {
        pl::warn(it, "Invalid date/time (use YYYY:mm:dd HH:MM:SS[.ss][+/-HH:MM|Z])\n")?;
    }
    ret(rtn_val)
}

/// Set byte order according to our current preferences
/// Inputs: 0) ExifTool object ref, 1) default byte order
/// Returns: new byte order ('II' or 'MM') and sets current byte order
/// Notes: takes the first of the following that is valid:
///  1) ByteOrder option
///  2) new value for ExifByteOrder
///  3) default byte order passed to this routine
///  4) makenote byte order from last file read
///  5) big endian
pub fn set_preferred_byte_order(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, default) = (arg(&a, 0), arg(&a, 1));
    let mut byte_order = meth(it, &self_, "Options", vec![s("ByteOrder")])?;
    if !byte_order.truthy() {
        byte_order = meth(it, &self_, "GetNewValue", vec![s("ExifByteOrder")])?;
    }
    let byte_order = or(or(or(byte_order, default), self_.get("MAKER_NOTE_BYTE_ORDER")), s("MM"));
    if !set_byte_order(it, byte_order.clone())?.truthy() {
        if meth(it, &self_, "Options", vec![s("Verbose")])?.truthy() {
            pl::warn(it, &format!("Invalid byte order '{}'\n", byte_order.to_text()))?;
        }
        let byte_order = or(self_.get("MAKER_NOTE_BYTE_ORDER"), s("MM"));
        set_byte_order(it, byte_order)?;
    }
    ret(get_byte_order(it)?)
}

/// Assemble a continuing fraction into a rational value
/// Inputs: 0) numerator, 1) denominator
///         2-N) list of fraction denominators, deepest first
/// Returns: numerator, denominator (in list context)
pub fn assemble_rational(it: &mut Interp, mut a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    if a.len() < 3 {
        if want != Want::List {
            return ret(n(a.len() as i64));
        }
        return Ok(a);
    }
    let rest = a.split_off(3);
    let (num, denom, frac) = (a[0].clone(), a[1].clone(), a[2].clone());
    let mut args = vec![pl::add(&pl::mul(&frac, &num), &denom), num];
    args.extend(rest);
    assemble_rational(it, args, want)
}

/// Convert a floating point number (or 'inf' or 'undef' or a fraction) into a rational
/// Inputs: 0) floating point number, 1) optional maximum value (defaults to 0x7fffffff)
/// Returns: numerator, denominator (in list context)
/// Notes:
/// - the returned rational will be accurate to at least 8 significant figures if possible
/// - eg. an input of 3.14159265358979 returns a rational of 104348/33215,
///   which equals    3.14159265392142 and is accurate to 10 significant figures
/// - the returned rational will be reduced to the lowest common denominator except when
///   the input is a fraction in which case the input is returned unchanged
/// - these routines were a bit tricky, but fun to write!
pub fn rationalize(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    if pl::str_eq(&val, "inf") {
        return Ok(vec![n(1), n(0)]);
    }
    if pl::str_eq(&val, "undef") {
        return Ok(vec![n(0), n(0)]);
    }
    if let Some(m) = re(r"^([-+]?\d+)/(\d+)$", "").m(&val) {
        return Ok(vec![cap(&m, 1), cap(&m, 2)]); // accept fractional values
    }
    // Note: Just testing "if $val" doesn't work because '0.0' is true!  (ugghh!)
    if pl::num_eq(&val, &n(0)) {
        return Ok(vec![n(0), n(1)]);
    }
    let sign = if pl::lt(&val, &n(0)) {
        val = pl::neg(&val);
        n(-1)
    } else {
        n(1)
    };
    let (mut num, mut denom) = (Value::Undef, Value::Undef);
    let mut fracs: Vec<Value> = Vec::new();
    let mut frac = val.clone();
    let max_int = or(arg(&a, 1), n(0x7fffffff));
    loop {
        let mut args = vec![pl::int(&pl::add(&frac, &Value::Num(0.5))), n(1)];
        args.extend(fracs.iter().cloned());
        let r = assemble_rational(it, args, Want::List)?;
        let (nn, d) = (arg(&r, 0), arg(&r, 1));
        if pl::gt(&nn, &max_int) || pl::gt(&d, &max_int) {
            if !num.is_undef() {
                break;
            }
            if pl::lt(&val, &n(1)) {
                return Ok(vec![sign, max_int]);
            }
            return Ok(vec![pl::mul(&sign, &max_int), n(1)]);
        }
        num = nn.clone();
        denom = d.clone(); // save last good values
        let err = pl::div(&pl::sub(&pl::div(&nn, &d)?, &val), &val)?; // get error of this rational
        if pl::lt(&pl::abs(&err), &Value::Num(1e-8)) {
            break; // all done if error is small
        }
        let int = pl::int(&frac);
        fracs.insert(0, int.clone());
        frac = pl::sub(&frac, &int);
        if !frac.truthy() {
            break;
        }
        frac = pl::div(&n(1), &frac)?;
    }
    Ok(vec![pl::mul(&num, &sign), denom])
}

/// `$_[1] and substr(${$_[1]}, $_[2], length($val)) = $val;`
fn store(a: &[Value], val: &Value) -> ER<()> {
    let dp = arg(a, 1);
    if dp.truthy() {
        substr_assign(&dp, &arg(a, 2), &pl::length(val), val)?;
    }
    Ok(())
}

/// The arguments after the value (`@_` after `shift`).
fn rest(a: &[Value]) -> Vec<Value> {
    a.get(1..).map(|r| r.to_vec()).unwrap_or_default()
}

/// Utility routines to for writing binary data values
/// Inputs: 0) value, 1) data ref, 2) offset
/// Notes: prototype is (@) so values can be passed from list if desired
pub fn set16s(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    if pl::lt(&val, &n(0)) {
        val = pl::add(&val, &n(0x10000));
    }
    let mut args = vec![val];
    args.extend(rest(&a));
    ret(set16u(it, args)?)
}

pub fn set32s(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    if pl::lt(&val, &n(0)) {
        val = pl::add(&val, &n(0xffffffff));
        val = pl::add(&val, &n(1));
    }
    let mut args = vec![val];
    args.extend(rest(&a));
    ret(set32u(it, args)?)
}

pub fn set64u(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    let two32 = n(4294967296);
    let hi = pl::int(&pl::div(&val, &two32)?);
    let lo = set32u(it, vec![pl::sub(&val, &pl::mul(&hi, &two32))])?; // NOTE: subject to round-off errors!
    let hi = set32u(it, vec![hi])?;
    let val = if pl::str_eq(&get_byte_order(it)?, "MM") { cat(&[&hi, &lo]) } else { cat(&[&lo, &hi]) };
    store(&a, &val)?;
    ret(val)
}

pub fn set64s(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    if pl::lt(&val, &n(0)) {
        val = pl::add(&val, &pl::mul(&n(4294967296), &n(4294967296))); // (temporary hack won't really work due to round-off errors)
    }
    let mut args = vec![val];
    args.extend(rest(&a));
    set64u(it, args, w)
}

fn set_rational(it: &mut Interp, a: &[Value], max: Option<i64>, num_fn: fn(&mut Interp, Vec<Value>, Want) -> ER<Vec<Value>>, den_fn: fn(&mut Interp, Vec<Value>, Want) -> ER<Vec<Value>>) -> ER<Vec<Value>> {
    let mut args = vec![arg(a, 0)];
    if let Some(m) = max {
        args.push(n(m));
    }
    let r = rationalize(it, args, Want::List)?;
    let (numer, denom) = (arg(&r, 0), arg(&r, 1));
    let val = cat(&[&c1(it, num_fn, vec![numer])?, &c1(it, den_fn, vec![denom])?]);
    store(a, &val)?;
    ret(val)
}

pub fn set_rational64u(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    set_rational(it, &a, Some(0xffffffff), part2::set32u, part2::set32u)
}

pub fn set_rational64s(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    set_rational(it, &a, None, set32s, part2::set32u)
}

pub fn set_rational32u(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    set_rational(it, &a, Some(0xffff), part2::set16u, part2::set16u)
}

pub fn set_rational32s(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    set_rational(it, &a, Some(0x7fff), set16s, part2::set16u)
}

pub fn set_fixed16u(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = pl::int(&pl::add(&pl::mul(&arg(&a, 0), &n(0x100)), &Value::Num(0.5)));
    let mut args = vec![val];
    args.extend(rest(&a));
    ret(set16u(it, args)?)
}

/// `int($val * $scale + ($val < 0 ? -0.5 : 0.5))`
fn round_scaled(val: &Value, scale: i64) -> Value {
    pl::int(&pl::add(&pl::mul(val, &n(scale)), &Value::Num(if pl::lt(val, &n(0)) { -0.5 } else { 0.5 })))
}

pub fn set_fixed16s(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    let mut args = vec![round_scaled(&val, 0x100)];
    args.extend(rest(&a));
    set16s(it, args, w)
}

pub fn set_fixed32u(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = pl::int(&pl::add(&pl::mul(&arg(&a, 0), &n(0x10000)), &Value::Num(0.5)));
    let mut args = vec![val];
    args.extend(rest(&a));
    ret(set32u(it, args)?)
}

pub fn set_fixed32s(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    let mut args = vec![round_scaled(&val, 0x10000)];
    args.extend(rest(&a));
    set32s(it, args, w)
}

pub fn set_float(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let packed = pl::pack("f", &[arg(&a, 0)])?;
    let val = c1(it, part2::swap_bytes, vec![packed, n(4)])?;
    store(&a, &val)?;
    ret(val)
}

pub fn set_double(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    // swap 32-bit words (ARM quirk) and bytes if necessary
    let packed = pl::pack("d", &[arg(&a, 0)])?;
    let words = c1(it, part2::swap_words, vec![packed])?;
    let val = c1(it, part2::swap_bytes, vec![words, n(8)])?;
    store(&a, &val)?;
    ret(val)
}

/// `IsFloat($val)`, which may change commas to decimals in `$val`.
fn is_float(it: &mut Interp, val: &mut Value) -> ER<bool> {
    let mut args = [val.clone()];
    let r = call_mut(it, "Image::ExifTool::IsFloat", &mut args, Want::Scalar)?;
    *val = args[0].clone();
    Ok(r.last().is_some_and(|v| v.truthy()))
}

fn is_int(it: &mut Interp, val: &Value) -> ER<bool> {
    Ok(c1(it, part2::is_int, vec![val.clone()])?.truthy())
}

fn is_hex(it: &mut Interp, val: &Value) -> ER<bool> {
    Ok(c1(it, part2::is_hex, vec![val.clone()])?.truthy())
}

fn is_rational(it: &mut Interp, val: &Value) -> ER<bool> {
    Ok(c1(it, part2::is_rational, vec![val.clone()])?.truthy())
}

/// write binary data value (with current byte ordering)
/// Inputs: 0) value, 1) format string
///         2) number of values:
///               undef = 1 for numerical types, or data length for string/undef types
///                  -1 = number of space-delimited values in the input string
///         3) optional data reference, 4) value offset (may be negative for bytes from end)
/// Returns: packed value (and sets value in data) or undef on error
/// Notes: May modify input value to round for integer formats
pub fn write_value(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (mut val, format, mut count, data_pt, offset) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4));
    let proc_ = wlex_hash(it, "writeValueProc").get(format.to_bytes());
    let mut packed;

    if proc_.truthy() {
        let mut vals = pl::split_ws(&val);
        if count.truthy() {
            if pl::lt(&count, &n(0)) {
                count = n(vals.len() as i64);
            }
        } else {
            count = n(1); // assume 1 if count not specified
        }
        packed = s("");
        loop {
            // while ($count--)
            let c = count.clone();
            count = pl::sub(&count, &n(1));
            if !c.truthy() {
                break;
            }
            if vals.is_empty() {
                return pl::ret_undef();
            }
            val = vals.remove(0);
            // validate numerical formats
            if re(r"^int", "").is_match(&format) {
                if !(is_int(it, &val)? || is_hex(it, &val)?) {
                    if !is_float(it, &mut val)? {
                        return pl::ret_undef();
                    }
                    // round to nearest integer
                    val = pl::int(&pl::add(&val, &Value::Num(if pl::lt(&val, &n(0)) { -0.5 } else { 0.5 })));
                    it.set_arg(0, val.clone())?;
                }
            } else if !is_float(it, &mut val)? && !(re(r"^rational", "").is_match(&format) && (pl::str_eq(&val, "inf") || pl::str_eq(&val, "undef") || is_rational(it, &val)?)) {
                return pl::ret_undef();
            }
            let p = pl::call_code(it, &proc_, vec![val.clone()], Want::Scalar)?.pop().unwrap_or_default();
            packed = cat(&[&packed, &p]);
        }
    } else if pl::str_eq(&format, "string") || pl::str_eq(&format, "undef") {
        if pl::str_eq(&format, "string") {
            val = cat(&[&val, &s("\0")]); // null-terminate strings
        }
        if count.truthy() && pl::gt(&count, &n(0)) {
            let diff = pl::sub(&count, &pl::length(&val));
            if diff.truthy() {
                //warn "wrong string length!\n";
                // adjust length of string to match specified count
                if pl::lt(&diff, &n(0)) {
                    if pl::str_eq(&format, "string") {
                        if !count.truthy() {
                            return pl::ret_undef();
                        }
                        val = cat(&[&substr(&val, &n(0), Some(&pl::sub(&count, &n(1)))), &s("\0")]);
                    } else {
                        val = substr(&val, &n(0), Some(&count));
                    }
                } else {
                    val = cat(&[&val, &Value::bytes(vec![0u8; diff.to_i64().max(0) as usize])]);
                }
            }
        } else {
            count = pl::length(&val);
        }
        if data_pt.truthy() {
            substr_assign(&data_pt, &offset, &count, &val)?;
        }
        return ret(val);
    } else {
        pl::warn(it, &format!("Sorry, Can't write {} values on this platform\n", format.to_text()))?;
        return pl::ret_undef();
    }
    if data_pt.truthy() {
        substr_assign(&data_pt, &offset, &pl::length(&packed), &packed)?;
    }
    ret(packed)
}

/// Encode bit mask (the inverse of DecodeBits())
/// Inputs: 0) value to encode, 1) Reference to hash for encoding (or undef)
///         2) optional number of bits per word (defaults to 32), 3) total bits
/// Returns: bit mask or undef on error (plus error string in list context)
pub fn encode_bits(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (val, lookup, mut bits, mut num) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    if !bits.truthy() {
        bits = n(32);
    }
    if !num.truthy() {
        num = bits.clone();
    }
    let words = pl::int(&pl::div(&pl::sub(&pl::add(&num, &bits), &n(1)), &bits)?);
    let mut out_val: Vec<Value> = vec![n(0); words.to_i64().max(0) as usize];
    let wantarray = want == Want::List;
    if !pl::str_eq(&val, "(none)") {
        let vals = pl::split(&re(r"\s*,\s*", ""), &val, 0);
        for val in vals.iter() {
            let mut bit;
            if lookup.truthy() {
                bit = pl::call(it, "Image::ExifTool::ReverseLookup", vec![val.clone(), lookup.clone()])?;
                // (Note: may get non-numerical $bit values from Unknown() tags)
                if bit.is_undef() {
                    if let Some(m) = re(r"\[(\d+)\]", "").m(val) {
                        // numerical bit specification
                        bit = cap(&m, 1);
                    } else {
                        // don't return error string unless more than one value
                        if !(vals.len() > 1 && wantarray) {
                            return pl::ret_undef();
                        }
                        return Ok(vec![Value::Undef, s(&format!("no match for '{}'", val.to_text()))]);
                    }
                }
            } else {
                bit = val.clone();
            }
            if !(is_int(it, &bit)? && pl::lt(&bit, &num)) {
                if !wantarray {
                    return pl::ret_undef();
                }
                return Ok(vec![Value::Undef, s(if is_int(it, &bit)? { "bit number too high" } else { "not an integer" })]);
            }
            let word = pl::int(&pl::div(&bit, &bits)?);
            let wi = word.to_i64().max(0) as usize;
            if out_val.len() <= wi {
                out_val.resize(wi + 1, Value::Undef);
            }
            out_val[wi] = ops::bit_or(&out_val[wi], &ops::shift_left(&n(1), &pl::sub(&bit, &pl::mul(&word, &bits))));
        }
    }
    ret(pl::join(" ", &out_val))
}

/// `UNIVERSAL::isa($outfile,'GLOB')`
fn is_glob(v: &Value) -> bool {
    matches!(v, Value::Handle(_))
}

/// get current position in output file (or end of file if a scalar reference)
/// Inputs: 0) file or scalar reference
/// Returns: Current position or -1 on error
pub fn tell(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let outfile = arg(&a, 0);
    if is_glob(&outfile) {
        return ret(snippet1(it, "tell($outfile)", &[("outfile", outfile)])?);
    }
    ret(pl::length(&deref(&outfile)?))
}

fn tell_v(it: &mut Interp, outfile: &Value) -> ER<Value> {
    c1(it, tell, vec![outfile.clone()])
}

/// write to file or memory
/// Inputs: 0) file or scalar reference, 1-N) list of stuff to write
/// Returns: true on success
pub fn write(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let outfile = arg(&a, 0);
    let items = a.get(1..).unwrap_or(&[]);
    if is_glob(&outfile) {
        return ret(Value::from(pl::print_to(it, Some(&outfile), items)?));
    } else if outfile.ref_type() == "SCALAR" {
        let mut b = outfile.deref().to_bytes().to_vec();
        for x in items {
            b.extend_from_slice(&x.to_bytes());
        }
        set_deref(&outfile, Value::bytes(b))?;
        return ret(n(1));
    }
    ret(n(0))
}

/// `Write($outfile, LIST)`: true on success.
fn write_v(it: &mut Interp, outfile: &Value, items: &[Value]) -> ER<bool> {
    let mut a = vec![outfile.clone()];
    a.extend_from_slice(items);
    Ok(c1(it, write, a)?.truthy())
}

/// Write trailer buffer to file (applying fixups if necessary)
/// Inputs: 0) ExifTool object ref, 1) trailer dirInfo ref, 2) output file ref
/// Returns: 1 on success
pub fn write_trailer_buffer(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, trail_info, outfile) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if self_.path(&["DEL_GROUP", "Trailer"]).truthy() {
        meth(it, &self_, "VPrint", vec![n(0), cat(&[&s("  Deleting trailer ("), &trail_info.get("Offset"), &s(" bytes)\n")])])?;
        self_.set("CHANGED", pl::add(&self_.get("CHANGED"), &n(1)));
        return ret(n(1));
    }
    let pos = tell_v(it, &outfile)?;
    let trail_pt = trail_info.get("OutFile");
    // apply fixup if necessary (AFCP requires this)
    if trail_info.get("Fixup").truthy() {
        if pl::gt(&pos, &n(0)) {
            // shift offsets to final AFCP location and write it out
            let fixup = trail_info.get("Fixup");
            fixup.set("Shift", pl::add(&fixup.get("Shift"), &pos));
            meth(it, &fixup, "ApplyFixup", vec![trail_pt.clone()])?;
        } else {
            meth(it, &self_, "Error", vec![s("Can't get file position for trailer offset fixup"), n(1)])?;
        }
    }
    let d = deref(&trail_pt)?;
    ret(Value::from(write_v(it, &outfile, &[d])?))
}

/// Add trailers as a block
/// Inputs: 0) ExifTool object ref, 1) [optional] trailer data raf,
///         1 or 2-N) trailer types to add (or none to add all)
/// Returns: new trailer ref, or undef
/// - increments CHANGED if trailer was added
pub fn add_new_trailers(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let self_ = arg(&a, 0);
    let mut types: Vec<Value> = a.get(1..).map(|r| r.to_vec()).unwrap_or_default();
    let mut trail_pt = Value::Undef;
    if types.first().is_some_and(|t| t.is_ref()) {
        trail_pt = types.remove(0);
    }
    if !types.first().is_some_and(|t| t.truthy()) && !types.is_empty() {
        types.remove(0); // (in case undef data ref is passed)
    }
    // add all possible trailers if none specified (currently only CanonVRD)
    if types.is_empty() {
        types = vec![s("CanonVRD"), s("CanonDR4")];
    }
    // add trailers as a block (if not done already)
    let extra = pl::pkg_hash(it, "Image::ExifTool::Extra");
    for type_ in types {
        let ti = extra.get(type_.to_bytes());
        if !self_.get("NEW_VALUE").get(ti.to_bytes()).truthy() {
            continue;
        }
        if self_.get(format!("Did{}", type_.to_text())).truthy() {
            continue;
        }
        let mut val = meth(it, &self_, "GetNewValue", vec![type_.clone()])?;
        if !val.truthy() {
            continue;
        }
        // DR4 record must be wrapped in VRD trailer package
        if pl::str_eq(&type_, "CanonDR4") {
            if self_.get("DidCanonVRD").truthy() {
                continue; // (only allow one VRD trailer)
            }
            crate::runtime::require(it, "Image::ExifTool::CanonVRD")?;
            val = pl::call(it, "Image::ExifTool::CanonVRD::WrapDR4", vec![val])?;
            self_.set("DidCanonVRD", n(1));
        }
        let verb = if trail_pt.truthy() { "Writing" } else { "Adding" };
        meth(it, &self_, "VPrint", vec![n(0), s(&format!("  {verb} {} as a block\n", type_.to_text()))])?;
        if trail_pt.truthy() {
            let cur = deref(&trail_pt)?;
            set_deref(&trail_pt, cat(&[&cur, &val]))?;
        } else {
            trail_pt = pl::sref(val);
        }
        self_.set(format!("Did{}", type_.to_text()), n(1));
        self_.set("CHANGED", pl::add(&self_.get("CHANGED"), &n(1)));
    }
    ret(trail_pt)
}

/// Write segment, splitting up into multiple segments if necessary
/// Inputs: 0) file or scalar reference, 1) segment marker
///         2) segment header, 3) segment data ref, 4) segment type
/// Returns: number of segments written, or 0 on error
/// Notes: Writes a single empty segment if data is empty
pub fn write_multi_segment(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (outfile, marker, header, data_pt, mut type_) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4));
    if !type_.truthy() {
        type_ = s("");
    }
    let data = deref(&data_pt)?;
    let len = pl::length(&data);
    let hdr = Value::bytes([0xffu8, marker.to_i64() as u8]);
    let mut count = 0i64;
    let max_segment_len = wlex_scalar(it, "maxSegmentLen").borrow().clone();
    let mut max_len = pl::sub(&max_segment_len, &pl::length(&header));
    if pl::str_eq(&type_, "ICC") {
        max_len = pl::sub(&max_len, &n(2)); // leave room for segment counters
    }
    let num = pl::int(&pl::div(&pl::sub(&pl::add(&len, &max_len), &n(1)), &max_len)?); // number of segments to write
    let mut nn = n(0);
    // write data, splitting into multiple segments if necessary
    // (each segment gets its own header)
    loop {
        count += 1;
        let mut size = pl::sub(&len, &nn);
        if pl::gt(&size, &max_len) {
            size = max_len.clone();
            // avoid starting an Extended EXIF segment with a valid TIFF header
            // (because we would interpret that as a separate EXIF segment)
            if pl::str_eq(&type_, "EXIF") && pl::le(&pl::add(&nn, &max_len), &pl::sub(&len, &n(4))) && re(r"^(MM\0\x2a|II\x2a\0)", "").is_match(&substr(&data, &pl::add(&nn, &max_len), Some(&n(4)))) {
                size = pl::sub(&size, &n(1));
            }
        }
        let mut buff = substr(&data, &nn, Some(&size));
        nn = pl::add(&nn, &size);
        size = pl::add(&size, &pl::length(&header));
        if pl::str_eq(&type_, "ICC") {
            buff = cat(&[&pl::pack("CC", &[n(count), num.clone()])?, &buff]);
            size = pl::add(&size, &n(2));
        }
        // write the new segment with appropriate header
        let seg_hdr = cat(&[&hdr, &pl::pack("n", &[pl::add(&size, &n(2))])?]);
        if !write_v(it, &outfile, &[seg_hdr, header.clone(), buff])? {
            return ret(n(0));
        }
        if pl::ge(&nn, &len) {
            break;
        }
    }
    ret(n(count))
}

/// Write XMP segment(s) to JPEG file
/// Inputs: 0) ExifTool object ref, 1) outfile ref, 2) XMP data ref,
///         3) extended XMP data ref, 4) 32-char extended XMP GUID (or undef if no extended data)
/// Returns: true on success, false on write error
pub fn write_multi_xmp(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, outfile, data_pt, ext_pt, guid) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4));
    let mut success = 1;
    let xmp_app1hdr = pkg_scalar(it, "xmpAPP1hdr");
    let xmp_ext_app1hdr = pkg_scalar(it, "xmpExtAPP1hdr");
    let max_xmp_len = wlex_scalar(it, "maxXMPLen").borrow().clone();

    // write main XMP segment
    let data = deref(&data_pt)?;
    let mut size = pl::add(&pl::length(&data), &pl::length(&xmp_app1hdr));
    if pl::gt(&size, &max_xmp_len) {
        meth(it, &self_, "Error", vec![s(&format!("XMP block too large for JPEG segment! ({} bytes)", size.to_text())), n(1)])?;
        return ret(n(1));
    }
    let mut app1hdr = cat(&[&Value::bytes(b"\xff\xe1"), &pl::pack("n", &[pl::add(&size, &n(2))])?]);
    if !write_v(it, &outfile, &[app1hdr, xmp_app1hdr, data])? {
        success = 0;
    }
    // write extended XMP segment(s) if necessary
    if !guid.is_undef() {
        let ext = deref(&ext_pt)?;
        size = pl::length(&ext);
        let max_len = pl::sub(&max_xmp_len, &n(75)); // maximum size without 75-byte header
        let mut off = n(0);
        while pl::lt(&off, &size) {
            // header(75) = signature(35) + guid(32) + size(4) + offset(4)
            let mut len = pl::sub(&size, &off);
            if pl::gt(&len, &max_len) {
                len = max_len.clone();
            }
            app1hdr = cat(&[&Value::bytes(b"\xff\xe1"), &pl::pack("n", &[pl::add(&pl::add(&len, &n(75)), &n(2))])?]);
            meth(it, &self_, "VPrint", vec![n(0), s(&format!("Writing extended XMP segment ({} bytes)\n", len.to_text()))])?;
            let items = [app1hdr, xmp_ext_app1hdr.clone(), guid.clone(), pl::pack("N2", &[size.clone(), off.clone()])?, substr(&ext, &off, Some(&len))];
            if !write_v(it, &outfile, &items)? {
                success = 0;
            }
            off = pl::add(&off, &max_len);
        }
    }
    ret(n(success))
}

/// Validate an image for writing
/// Inputs: 0) ExifTool object reference, 1) raw value reference
/// Returns: error string or undef on success
pub fn check_image(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, val_ptr) = (arg(&a, 0), arg(&a, 1));
    let v = deref(&val_ptr)?;
    if pl::length(&v).to_i64() != 0 && !re(r"^\xff\xd8", "").is_match(&v) && !meth(it, &self_, "Options", vec![s("IgnoreMinorErrors")])?.truthy() {
        return ret(s("[Minor] Not a valid image"));
    }
    pl::ret_undef()
}

/// check a value for validity
/// Inputs: 0) value reference, 1) format string, 2) optional count
/// Returns: error string, or undef on success
/// Notes: May modify value (if a count is specified for a string, it is null-padded
/// to the specified length, and floating point values are rounded to integer if required)
pub fn check_value(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val_ptr, format, mut count) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut vals: Vec<Value>;

    if pl::str_eq(&format, "string") || pl::str_eq(&format, "undef") {
        if !(count.truthy() && pl::gt(&count, &n(0))) {
            return pl::ret_undef();
        }
        let len = pl::length(&deref(&val_ptr)?);
        if pl::str_eq(&format, "string") {
            if pl::ge(&len, &count) {
                return ret(s("String too long"));
            }
        } else if pl::gt(&len, &count) {
            return ret(s("Data too long"));
        }
        if pl::lt(&len, &count) {
            let pad = pl::sub(&count, &len).to_i64().max(0) as usize;
            let cur = deref(&val_ptr)?;
            set_deref(&val_ptr, cat(&[&cur, &Value::bytes(vec![0u8; pad])]))?;
        }
        return pl::ret_undef();
    }
    if count.truthy() && !pl::num_eq(&count, &n(1)) {
        vals = pl::split_ws(&deref(&val_ptr)?);
        if pl::lt(&count, &n(0)) {
            count = n(vals.len() as i64);
            if !count.truthy() {
                return pl::ret_undef();
            }
        }
    } else {
        count = n(1);
        vals = vec![deref(&val_ptr)?];
    }
    if !pl::num_eq(&n(vals.len() as i64), &count) {
        let str_ = if pl::gt(&n(vals.len() as i64), &count) { "Too many" } else { "Not enough" };
        return ret(s(&format!("{str_} values specified ({} required)", count.to_text())));
    }
    let int_range = wlex_hash(it, "intRange");
    let mut i = n(0);
    while pl::lt(&i, &count) {
        let mut val = if vals.is_empty() { Value::Undef } else { vals.remove(0) };
        i = pl::add(&i, &n(1));
        if re(r"^int", "").is_match(&format) {
            // make sure the value is integer
            if !is_int(it, &val)? {
                if is_hex(it, &val)? {
                    val = hex(&val);
                    set_deref(&val_ptr, val.clone())?;
                } else {
                    // round single floating point values to the nearest integer
                    if !(is_float(it, &mut val)? && pl::num_eq(&count, &n(1))) {
                        return ret(s("Not an integer"));
                    }
                    val = pl::int(&pl::add(&val, &Value::Num(if pl::lt(&val, &n(0)) { -0.5 } else { 0.5 })));
                    set_deref(&val_ptr, val.clone())?;
                }
            }
            let rng = int_range.get(format.to_bytes());
            if !rng.truthy() {
                return ret(s(&format!("Bad int format: {}", format.to_text())));
            }
            if pl::lt(&val, &rng.at(0)) {
                return ret(s(&format!("Value below {} minimum", format.to_text())));
            }
            // (allow 0xfeedfeed code as value for 16-bit pointers)
            if pl::gt(&val, &rng.at(1)) && !pl::num_eq(&val, &n(0xfeedfeed)) {
                return ret(s(&format!("Value above {} maximum", format.to_text())));
            }
        } else if re(r"^rational", "").is_match(&format) || pl::str_eq(&format, "float") || pl::str_eq(&format, "double") {
            // make sure the value is a valid floating point number
            if !is_float(it, &mut val)? {
                // allow 'inf', 'undef' and fractional rational values
                if re(r"^rational", "").is_match(&format) {
                    if pl::str_eq(&val, "inf") || pl::str_eq(&val, "undef") {
                        continue;
                    }
                    if let Some(m) = re(r"^([-+]?\d+)/(\d+)$", "").m(&val) {
                        if !(pl::lt(&cap(&m, 1), &n(0)) && re(r"u$", "").is_match(&format)) {
                            continue;
                        }
                        return ret(s("Must be an unsigned rational"));
                    }
                }
                return ret(s("Not a floating point number"));
            }
            if re(r"^rational\d+u$", "").is_match(&format) && pl::lt(&val, &n(0)) {
                return ret(s("Must be a positive number"));
            }
        }
    }
    pl::ret_undef() // success!
}

/// check new value for binary data block
/// Inputs: 0) ExifTool object ref, 1) tagInfo hash ref, 2) raw value ref
/// Returns: error string or undef (and may modify value) on success
pub fn check_binary_data(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    let (_self, tag_info, val_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut format = tag_info.get("Format");
    if !format.truthy() {
        let table = tag_info.get("Table");
        if table.truthy() && table.get("FORMAT").truthy() {
            format = table.get("FORMAT");
        } else {
            // use default 'int8u' unless specified
            format = s("int8u");
        }
    }
    let mut count = Value::Undef;
    if let Some(m) = re(r"(.*)\[(.*)\]", "").m(&format) {
        format = cap(&m, 1);
        count = cap(&m, 2);
        // can't evaluate $count now because we don't know $size yet
        if re(r"\$size", "").is_match(&count) {
            count = n(-1); // (-1 = any count allowed)
        }
    }
    check_value(it, vec![val_ptr, format, count], w)
}

/// Rename a file (with patch for Windows Unicode file names, and other problem)
/// Inputs: 0) ExifTool ref, 1) old name, 2) new name
/// Returns: true on success
pub fn rename(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, mut old, mut new) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut result;
    let mut try_ = Value::Undef;
    let mut win_uni = false;
    let encode = |it: &mut Interp, f: &mut Value, force: bool| -> ER<bool> {
        let mut args = vec![f.clone()];
        if force {
            args.push(n(1));
        }
        let r = pl::method_mut(it, &self_, "EncodeFileName", &mut args, Want::Scalar)?;
        *f = args[0].clone();
        Ok(r.last().is_some_and(|v| v.truthy()))
    };

    if encode(it, &mut old, false)? {
        encode(it, &mut new, true)?;
        win_uni = true;
    } else if encode(it, &mut new, false)? {
        old = arg(&a, 1);
        encode(it, &mut old, true)?;
        win_uni = true;
    }
    loop {
        if win_uni {
            let (o, nw) = (old.clone(), new.clone());
            result = pl::eval_block(it, |it| {
                snippet1(it, "Win32API::File::MoveFileExW($old, $new, Win32API::File::MOVEFILE_REPLACE_EXISTING() | Win32API::File::MOVEFILE_COPY_ALLOWED())", &[("old", o), ("new", nw)])
            })?
            .unwrap_or_default();
        } else {
            result = snippet1(it, "rename($old, $new)", &[("old", old.clone()), ("new", new.clone())])?;
        }
        if result.truthy() || !pl::str_eq(&it.get_scalar("main::^O"), "MSWin32") {
            break;
        }
        // keep trying for up to 0.5 seconds
        // (patch for Windows denial-of-service susceptibility)
        try_ = pl::add(&or(try_, n(1)), &n(1));
        if pl::gt(&try_, &n(50)) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10)); // sleep for 0.01 sec
    }
    ret(result)
}

/// Delete a file (with patch for Windows Unicode file names)
/// Inputs: 0) ExifTool ref, 1-N) names of files to delete
/// Returns: number of files deleted
pub fn unlink(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let self_ = arg(&a, 0);
    let mut result = 0i64;
    for f in a.iter().skip(1) {
        let mut args = vec![f.clone()];
        let enc = pl::method_mut(it, &self_, "EncodeFileName", &mut args, Want::Scalar)?;
        let file = args[0].clone();
        if enc.last().is_some_and(|v| v.truthy()) {
            if pl::eval_block(it, |it| snippet1(it, "Win32API::File::DeleteFileW($file)", &[("file", file)]))?.is_some_and(|v| v.truthy()) {
                result += 1;
            }
        } else if snippet1(it, "unlink $file", &[("file", file)])?.truthy() {
            result += 1;
        }
    }
    ret(n(result))
}

/// Set file times (Unix seconds since the epoch)
/// Inputs: 0) ExifTool ref, 1) file name or ref, 2) access time, 3) modification time,
///         4) inode change or creation time (or undef for any time to avoid setting)
///         5) flag to suppress warning
/// Returns: 1 on success, 0 on error
pub fn set_file_time(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, mut file, mut atime, mut mtime, mut ctime, no_warn) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4), arg(&a, 5));
    let mut save_file = Value::Undef;
    // local *FH;  (the interpreter keeps a glob as a plain handle; it is
    // closed below when the sub returns, as perl does at the end of the scope)
    let mut fh = Value::Undef;

    // open file by name if necessary
    if !file.is_ref() {
        // (file will be automatically closed when *FH goes out of scope)
        let glob = snippet1(it, "\\*FH", &[])?;
        if !meth(it, &self_, "Open", vec![glob.clone(), file.clone(), s("+<")])?.truthy() {
            let mut success = Value::Undef;
            if !atime.is_undef() || !mtime.is_undef() {
                let t = pl::method_list(it, &self_, "GetFileTime", vec![file.clone()])?;
                if atime.is_undef() {
                    atime = arg(&t, 0);
                }
                if mtime.is_undef() {
                    mtime = arg(&t, 1);
                }
                if !atime.is_undef() && !mtime.is_undef() {
                    let (at, mt, f) = (atime.clone(), mtime.clone(), file.clone());
                    success = pl::eval_block(it, |it| snippet1(it, "utime($atime, $mtime, $file)", &[("atime", at), ("mtime", mt), ("file", f)]))?.unwrap_or_default();
                }
            }
            if !success.truthy() {
                meth(it, &self_, "Warn", vec![s("Error updating file time")])?;
            }
            return ret(success);
        }
        save_file = file.clone();
        file = glob.clone();
        fh = glob;
    }
    let r = set_file_time_body(it, &self_, file, save_file, &mut atime, &mut mtime, &mut ctime, &no_warn);
    if !fh.is_undef() {
        snippet(it, "close($fh)", &[("fh", fh)], Want::Void)?;
    }
    r
}

#[allow(clippy::too_many_arguments)]
fn set_file_time_body(it: &mut Interp, self_: &Value, mut file: Value, mut save_file: Value, atime: &mut Value, mtime: &mut Value, ctime: &mut Value, no_warn: &Value) -> ER<Vec<Value>> {
    // on Windows, try to work around incorrect file times when daylight saving time is in effect
    if pl::str_eq(&it.get_scalar("main::^O"), "MSWin32") {
        if pl::eval_block(it, |it| crate::runtime::require(it, "Win32::API"))?.is_none() {
            meth(it, self_, "Warn", vec![s("Install Win32::API for proper handling of Windows file times")])?;
        } else if pl::eval_block(it, |it| crate::runtime::require(it, "Win32API::File"))?.is_none() {
            meth(it, self_, "Warn", vec![s("Install Win32API::File for proper handling of Windows file times")])?;
        } else {
            // get Win32 handle, needed for SetFileTime
            let f = file.clone();
            let win32_handle = pl::eval_block(it, |it| snippet1(it, "Win32API::File::GetOsFHandle($file)", &[("file", f)]))?.unwrap_or_default();
            if !win32_handle.truthy() {
                meth(it, self_, "Warn", vec![s("Win32API::File GetOsFHandle returned invalid handle")])?;
                return ret(n(0));
            }
            // convert Unix seconds to FILETIME structs
            for time in [&mut *atime, &mut *mtime, &mut *ctime] {
                // set to NULL if not defined (i.e. do not change)
                if time.is_undef() {
                    *time = n(0);
                    continue;
                }
                // convert to 100 ns intervals since 0:00 UTC Jan 1, 1601
                // (89 leap years between 1601 and 1970)
                let wt = pl::mul(&pl::add(time, &n(((1970 - 1601) * 365 + 89) * 24 * 3600)), &Value::Num(1e7));
                let hi = pl::int(&pl::div(&wt, &n(4294967296))?);
                *time = pl::pack("LL", &[pl::int(&pl::sub(&wt, &pl::mul(&hi, &n(4294967296)))), hi])?; // pack FILETIME struct
            }
            let k32 = wlex_scalar(it, "k32SetFileTime");
            if !k32.borrow().truthy() {
                if !k32.borrow().is_undef() {
                    return ret(n(0));
                }
                let v = snippet1(it, "Win32::API->new('KERNEL32', 'SetFileTime', 'NPPP', 'I')", &[])?;
                *k32.borrow_mut() = v.clone();
                if !v.truthy() {
                    meth(it, self_, "Warn", vec![s("Error loading Win32::API SetFileTime")])?;
                    *k32.borrow_mut() = n(0);
                    return ret(n(0));
                }
            }
            let k = k32.borrow().clone();
            if !meth(it, &k, "Call", vec![win32_handle, ctime.clone(), atime.clone(), mtime.clone()])?.truthy() {
                let e = snippet1(it, "Win32::GetLastError()", &[])?;
                meth(it, self_, "Warn", vec![cat(&[&s("Win32::API SetFileTime returned "), &e])])?;
                return ret(n(0));
            }
            return ret(n(1));
        }
    }
    // other OS (or Windows fallback)
    if !atime.is_undef() && !mtime.is_undef() {
        let success;
        let handler = Value::Code(it.sub_ref("Image::ExifTool::SetWarning"));
        let (at, mt) = (atime.clone(), mtime.clone());
        // local $SIG{'__WARN__'} = \&SetWarning; # (this may not be necessary)
        let (success_, at_err) = pl::local_sig_warn(it, handler, |it| {
            let mut success;
            loop {
                it.set_scalar(&format!("{ET}::evalWarning"), Value::Undef);
                // (this may fail on the first try if futimes is not implemented)
                let f = file.clone();
                let (a2, m2) = (at.clone(), mt.clone());
                success = pl::eval_block(it, |it| snippet1(it, "utime($atime, $mtime, $file)", &[("atime", a2), ("mtime", m2), ("file", f)]))?.unwrap_or_default();
                if success.truthy() || save_file.is_undef() {
                    break;
                }
                snippet(it, "close $file", &[("file", file.clone())], Want::Void)?;
                file = save_file.clone();
                save_file = Value::Undef;
            }
            let at_err = it.get_scalar("main::@");
            Ok((success, at_err))
        })?;
        success = success_;
        if !no_warn.truthy() {
            let eval_warning = pkg_scalar(it, "evalWarning");
            if at_err.truthy() || eval_warning.truthy() {
                let w = pl::call(it, "Image::ExifTool::CleanWarning", vec![or(at_err, eval_warning)])?;
                meth(it, self_, "Warn", vec![w])?;
            } else if !success.truthy() {
                meth(it, self_, "Warn", vec![s("Error setting file time")])?;
            }
        }
        return ret(success);
    }
    ret(n(1)) // (nothing to do)
}

/// Add data to hash checksum
/// Inputs: 0) ExifTool ref, 1) RAF ref, 2) data size (or undef to read to end of file),
///         3) data name (or undef for no warnings or messages), 4) flag for no verbose message
/// Returns: number of bytes read and hashed
pub fn image_data_hash(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (self_, raf, mut size, type_, no_msg) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4));
    let hash = self_.get("ImageDataHash");
    if !hash.truthy() {
        return pl::ret_nothing(want);
    }
    let (mut bytes_read, mut nn) = (n(0), n(65536));
    let mut buff = Value::Undef;
    loop {
        if !size.is_undef() {
            if !size.truthy() {
                break;
            }
            nn = if pl::gt(&size, &n(65536)) { n(65536) } else { size.clone() };
            size = pl::sub(&size, &nn);
        }
        if !raf_read(it, &raf, &mut buff, nn.clone())?.truthy() {
            if type_.truthy() && !size.is_undef() {
                meth(it, &self_, "Warn", vec![s(&format!("Error reading {} data", type_.to_text()))])?;
            }
            break;
        }
        meth(it, &hash, "add", vec![buff.clone()])?;
        bytes_read = pl::add(&bytes_read, &pl::length(&buff));
    }
    if self_.path(&["OPTIONS", "Verbose"]).truthy() && bytes_read.truthy() && type_.truthy() && !no_msg.truthy() {
        let msg = cat(&[&self_.get("INDENT"), &s("(ImageDataHash: "), &bytes_read, &s(" bytes of "), &type_, &s(" data)\n")]);
        meth(it, &self_, "VPrint", vec![n(0), msg])?;
    }
    ret(bytes_read)
}

/// Copy data block from RAF to output file in max 64kB chunks
/// Inputs: 0) RAF ref, 1) outfile ref, 2) block size
/// Returns: 1 on success, 0 on read error, undef on write error
pub fn copy_block(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (raf, outfile, mut size) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut buff = Value::Undef;
    loop {
        if !pl::gt(&size, &n(0)) {
            break;
        }
        let nn = if pl::gt(&size, &n(65536)) { n(65536) } else { size.clone() };
        if !pl::num_eq(&raf_read(it, &raf, &mut buff, nn.clone())?, &nn) {
            return ret(n(0));
        }
        if !write_v(it, &outfile, &[buff.clone()])? {
            return pl::ret_undef();
        }
        size = pl::sub(&size, &nn);
    }
    ret(n(1))
}

fn copy_block_v(it: &mut Interp, raf: &Value, outfile: &Value, size: Value) -> ER<Value> {
    c1(it, copy_block, vec![raf.clone(), outfile.clone(), size])
}

/// Copy image data from one file to another
/// Inputs: 0) ExifTool object reference
///         1) reference to list of image data [ position, size, pad bytes ]
///         2) output file ref
/// Returns: true on success
pub fn copy_image_data(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, image_data_blocks, outfile) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let raf = self_.get("RAF");
    let mut err: Option<&str> = None;
    let num = image_data_blocks.len();
    if num > 0 {
        meth(it, &self_, "VPrint", vec![n(0), s(&format!("  Copying {num} image data blocks\n"))])?;
    }
    for data_block in image_data_blocks.items() {
        let (pos, size, pad) = (data_block.at(0), data_block.at(1), data_block.at(2));
        if !raf_seek(it, &raf, pos, 0)? {
            err = Some("read");
            break;
        }
        let result = copy_block_v(it, &raf, &outfile, size)?;
        if !result.truthy() {
            err = Some(if !result.is_undef() { "read" } else { "writ" });
        }
        // pad if necessary
        if pad.truthy() && !write_v(it, &outfile, &[Value::bytes(vec![0u8; pad.to_i64().max(0) as usize])])? {
            err = Some("writ");
        }
        if err.is_some() {
            break;
        }
    }
    if let Some(e) = err {
        meth(it, &self_, "Error", vec![s(&format!("Error {e}ing image data"))])?;
        return ret(n(0));
    }
    ret(n(1))
}

/// `$$h{k} = ($$h{k} || 0) + d` (and `++$$h{k}`, `--$$h{k}`)
fn bump(h: &Value, k: impl AsRef<[u8]>, d: i64) {
    let k = k.as_ref();
    h.set(k, pl::add(&h.get(k), &n(d)));
}

/// A pattern made of a package variable's value (interpolated, so its
/// characters are regex syntax, as in perl) after `^`.
fn re_hdr(hdr: &Value) -> ER<pl::Re> {
    let mut p = b"^".to_vec();
    p.extend_from_slice(&hdr.to_bytes());
    pl::re_dyn(&p, "")
}

/// `$$self{$_}{Fixup}{Start} += 18 if $$self{$_};` for the external pointers
fn shift_fixups(self_: &Value) {
    for k in ["PREVIEW_INFO", "LeicaTrailer", "HiddenData"] {
        if self_.get(k).truthy() {
            let f = self_.get(k).get_or_new_hash("Fixup");
            f.set("Start", pl::add(&f.get("Start"), &n(18)));
        }
    }
}

/// WriteJPEG : Write JPEG image
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid JPEG file, or -1 if
///          an output file was specified and a write error occurred
pub fn write_jpeg(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, dir_info) = (arg(&a, 0), arg(&a, 1));
    let outfile = dir_info.get("OutFile");
    let raf = dir_info.get("RAF");
    let mut s_ = Value::Undef;
    let mut err = false;
    let mut is_exv = false;
    let mut creating_exv = false;

    // check to be sure this is a valid JPG or EXV file
    let ok = pl::num_eq(&raf_read(it, &raf, &mut s_, n(2))?, &n(2)) && s_.to_bytes()[..] == b"\xff\xd8"[..];
    if !ok {
        if !s_.is_undef() && pl::length(&s_).to_i64() > 0 {
            if !(s_.to_bytes()[..] == b"\xff\x01"[..] && pl::num_eq(&raf_read(it, &raf, &mut s_, n(5))?, &n(5)) && pl::str_eq(&s_, "Exiv2")) {
                return ret(n(0));
            }
        } else {
            if !pl::str_eq(&self_.get("FILE_TYPE"), "EXV") {
                return ret(n(0));
            }
            s_ = s("Exiv2");
            creating_exv = true;
        }
        if !write_v(it, &outfile, &[Value::bytes(b"\xff\x01")])? {
            err = true;
        }
        is_exv = true;
    }

    self_.delete("PREVIEW_INFO"); // reset preview information
    self_.delete("DEL_PREVIEW"); // reset flag to delete preview

    if !write_v(it, &outfile, &[s_.clone()])? {
        err = true;
    }
    // figure out what segments we need to write for the tags we have set
    let path = self_.get("PATH");
    let mut st = JpegState {
        self_: self_.clone(),
        raf,
        outfile,
        ch: Value::Undef,
        s_,
        err,
        done_dir: Value::new_hash(),
        is_exv,
        creating_exv,
        verbose: self_.path(&["OPTIONS", "Verbose"]),
        out: self_.path(&["OPTIONS", "TextOut"]),
        rtn_val: 0,
        add_dirs: self_.get("ADD_DIRS"),
        edit_dirs: self_.get("EDIT_DIRS"),
        del_group: self_.get("DEL_GROUP"),
        pn: path.len(),
        path,
    };

    // set input record separator to 0xff (the JPEG marker) to make reading quicker
    pl::local_global(it, b'$', "main::/", |it| {
        it.set_scalar("main::/", Value::bytes(b"\xff"));
        write_jpeg_body(it, &mut st)
    })
}

/// The variables of WriteJPEG shared by its two parts.
struct JpegState {
    self_: Value,
    raf: Value,
    outfile: Value,
    ch: Value,
    s_: Value,
    err: bool,
    done_dir: Value,
    is_exv: bool,
    creating_exv: bool,
    verbose: Value,
    out: Value,
    rtn_val: i64,
    add_dirs: Value,
    edit_dirs: Value,
    del_group: Value,
    path: Value,
    pn: usize,
}

#[allow(unused_assignments)]
fn write_jpeg_body(it: &mut Interp, st: &mut JpegState) -> ER<Vec<Value>> {
    let self_ = st.self_.clone();
    let raf = st.raf.clone();
    let done_dir = st.done_dir.clone();
    let verbose = st.verbose.clone();
    let out = st.out.clone();
    let add_dirs = st.add_dirs.clone();
    let edit_dirs = st.edit_dirs.clone();
    let del_group = st.del_group.clone();
    let path = st.path.clone();
    let pn = st.pn;
    let (is_exv, creating_exv) = (st.is_exv, st.creating_exv);
    let mut length;
    let mut write_buffer = Value::Undef; // used to buffer writing until PreviewImage position is known
    let mut old_outfile = Value::Undef;
    let xmp_app1hdr = pkg_scalar(it, "xmpAPP1hdr");
    let xmp_ext_app1hdr = pkg_scalar(it, "xmpExtAPP1hdr");
    let ps_app13hdr = pkg_scalar(it, "psAPP13hdr");
    let exif_app1hdr = pkg_scalar(it, "exifAPP1hdr");
    let max_segment_len = wlex_scalar(it, "maxSegmentLen").borrow().clone();
    let max_xmp_len = wlex_scalar(it, "maxXMPLen").borrow().clone();
    let jpeg_marker = pl::pkg_hash(it, "Image::ExifTool::jpegMarker");
    let extra = pl::pkg_hash(it, "Image::ExifTool::Extra");
    let write_tiff_ref = Value::Code(it.sub_ref("Image::ExifTool::WriteTIFF"));
    let sof = |m: i64| (m & 0xf0) == 0xc0 && (m == 0xc0 || m & 0x03 != 0);
    let marker_name_of = |it: &mut Interp, m: i64| c1(it, part2::jpeg_marker_name, vec![n(m)]);
    let is = |v: &Value, b: &[u8]| v.to_bytes()[..] == *b;
    let changed = |self_: &Value| bump(self_, "CHANGED", 1);
    macro_rules! wr {
        ($($x:expr),*) => {
            if !write_v(it, &st.outfile, &[$($x),*])? {
                st.err = true;
            }
        };
    }
    macro_rules! vout {
        ($($x:expr),*) => {
            print(it, &out, &[$($x),*])?;
        };
    }
//
// pre-scan image to determine if any create-able segment already exists
//
    let pos = raf_tell(it, &raf)?;
    let mut marker: i64 = 0;
    let mut dir_order: Vec<Value> = Vec::new();
    let dir_count = Value::new_hash();
    'prescan: loop {
        // read up to next marker (JPEG markers begin with 0xff)
        if !raf_readline(it, &raf, &mut st.s_)?.truthy() {
            break;
        }
        // JPEG markers can be padded with unlimited 0xff's
        loop {
            if !raf_read(it, &raf, &mut st.ch, n(1))?.truthy() {
                break 'prescan;
            }
            marker = st.ch.to_bytes().first().copied().unwrap_or(0) as i64;
            if marker != 0xff {
                break;
            }
        }
        let mut dir_name = Value::Undef;
        // stop pre-scan at SOS (end of meta information) or EOI (end of image)
        if marker == 0xda || marker == 0xd9 {
            dir_name = jpeg_marker.get(marker.to_string());
            dir_order.push(dir_name.clone());
            dir_count.set(dir_name.to_bytes(), n(1));
            break;
        }
        // handle SOF markers: SOF0-SOF15, except DHT(0xc4), JPGA(0xc8) and DAC(0xcc)
        if sof(marker) {
            if !raf_seek(it, &raf, n(7), 1)? {
                break;
            }
        // read data for all markers except stand-alone
        // markers 0x00, 0x01 and 0xd0-0xd7 (NULL, TEM, RST0-RST7)
        } else if marker != 0x00 && marker != 0x01 && !(0xd0..=0xd7).contains(&marker) {
            // read record length word
            if !pl::num_eq(&raf_read(it, &raf, &mut st.s_, n(2))?, &n(2)) {
                break;
            }
            let len = pl::unpack1("n", &st.s_)?; // get data length
            if !(!len.is_undef() && pl::ge(&len, &n(2))) {
                break;
            }
            let mut len = pl::sub(&len, &n(2)); // subtract size of length word
            if (marker & 0xf0) == 0xe0 {
                // is this an APP segment?
                let nn = if pl::lt(&len, &n(64)) { len.clone() } else { n(64) };
                if !pl::num_eq(&raf_read(it, &raf, &mut st.s_, nn.clone())?, &nn) {
                    break;
                }
                len = pl::sub(&len, &nn);
                let s_ = st.s_.clone();
                // Note: only necessary to recognize APP segments that we can create,
                // or delete as a group (and the names below should match @delGroups)
                if marker == 0xe0 {
                    if re(r"^JFIF\0", "").is_match(&s_) {
                        dir_name = s("JFIF");
                    }
                    if re(r"^JFXX\0\x10", "").is_match(&s_) {
                        dir_name = s("JFXX");
                    }
                    if re(r"^(II|MM).{4}HEAPJPGM", "s").is_match(&s_) {
                        dir_name = s("CIFF");
                    }
                } else if marker == 0xe1 {
                    if let Some(m) = re(r"^(.{0,4})Exif\0.(.{1,4})", "is").m(&s_) {
                        dir_name = s("IFD0");
                        let (junk, bytes) = (cap(&m, 1), cap(&m, 2));
                        // support multi-segment EXIF
                        if dir_order.last().is_some_and(|d| re(r"^(IFD0|ExtendedEXIF)$", "").is_match(d)) && pl::length(&junk).to_i64() == 0 && !re(r"^(MM\0\x2a|II\x2a\0)", "").is_match(&bytes) {
                            dir_name = s("ExtendedEXIF");
                        }
                    }
                    if re_hdr(&xmp_app1hdr)?.is_match(&s_) {
                        dir_name = s("XMP");
                    }
                    if re_hdr(&xmp_ext_app1hdr)?.is_match(&s_) {
                        dir_name = s("XMP");
                    }
                } else if marker == 0xe2 {
                    if re(r"^ICC_PROFILE\0", "").is_match(&s_) {
                        dir_name = s("ICC_Profile");
                    }
                    if re(r"^FPXR\0", "").is_match(&s_) {
                        dir_name = s("FlashPix");
                    }
                    if re(r"^MPF\0", "").is_match(&s_) {
                        dir_name = s("MPF");
                    }
                } else if marker == 0xe3 {
                    if re(r"^(Meta|META|Exif)\0\0", "").is_match(&s_) {
                        dir_name = s("Meta");
                    }
                } else if marker == 0xe5 {
                    if re(r"^RMETA\0", "").is_match(&s_) {
                        dir_name = s("RMETA");
                    }
                } else if marker == 0xea {
                    if re(r"^AROT\0\0", "").is_match(&s_) {
                        dir_name = s("AROT");
                    }
                } else if marker == 0xeb {
                    if re(r"^JP", "").is_match(&s_) {
                        dir_name = s("JUMBF");
                    }
                } else if marker == 0xec {
                    if re(r"^Ducky", "").is_match(&s_) {
                        dir_name = s("Ducky");
                    }
                } else if marker == 0xed {
                    if re_hdr(&ps_app13hdr)?.is_match(&s_) {
                        dir_name = s("Photoshop");
                    }
                } else if marker == 0xee && re(r"^Adobe", "").is_match(&s_) {
                    dir_name = s("Adobe");
                }
                // initialize doneDir as a flag that the directory exists
                // (unless we are deleting it anyway)
                if !dir_name.is_undef() && !del_group.get(dir_name.to_bytes()).truthy() {
                    done_dir.set(dir_name.to_bytes(), n(0));
                }
            }
            if !raf_seek(it, &raf, len, 1)? {
                break;
            }
        }
        if !dir_name.truthy() {
            dir_name = marker_name_of(it, marker)?;
        }
        bump(&dir_count, dir_name.to_bytes(), 1);
        dir_order.push(dir_name);
    }
    if !(marker != 0 && marker == 0xda) {
        if !is_exv {
            meth(it, &self_, "Error", vec![s("Corrupted JPEG image")])?;
            return ret(n(1));
        }
        if marker != 0 && marker != 0xd9 {
            meth(it, &self_, "Error", vec![s("Corrupted EXV file")])?;
            return ret(n(1));
        }
    }
    if !raf_seek(it, &raf, pos, 0)? {
        meth(it, &self_, "Error", vec![s("Seek error")])?;
        return ret(n(1));
    }
//
// re-write the image
//
    let combined_seg_data: Rc<RefCell<Value>> = Rc::new(RefCell::new(Value::Undef));
    let mut seg_pos = Value::Undef;
    let mut first_seg_pos = Value::Undef;
    let mut extended_xmp = Value::new_hash();
    let mut icc_chunk: Vec<Value> = Vec::new();
    let mut icc_chunk_count = Value::Undef;
    let mut icc_chunks_total = Value::Undef;
    let combined = || Value::ScalarRef(combined_seg_data.clone());
    // read through each segment in the JPEG file
    'marker: loop {
        // read up to next marker (JPEG markers begin with 0xff)
        let mut seg_junk = Value::Undef;
        if !raf_readline(it, &raf, &mut seg_junk)?.truthy() {
            seg_junk = s("");
        }
        // remove the 0xff but write the rest of the junk up to this point
        // (this will handle the data after the first 7 bytes of SOF segments)
        {
            let b = seg_junk.to_bytes();
            if b.last() == Some(&0xff) {
                seg_junk = Value::bytes(&b[..b.len() - 1]);
            }
        }
        if pl::length(&seg_junk).to_i64() > 0 {
            write_v(it, &st.outfile, &[seg_junk])?;
        }
        // JPEG markers can be padded with unlimited 0xff's
        loop {
            if raf_read(it, &raf, &mut st.ch, n(1))?.truthy() {
                marker = st.ch.to_bytes().first().copied().unwrap_or(0) as i64;
                if marker != 0xff {
                    break;
                }
            } else if creating_exv {
                // create EXV from scratch
                marker = 0xd9; // EOI
                dir_order.push(s("EOI"));
                dir_count.set("EOI", n(1));
                break;
            } else {
                meth(it, &self_, "Error", vec![s("Format error")])?;
                return ret(n(1));
            }
        }
        // read the segment data
        let seg_data: Rc<RefCell<Value>> = Rc::new(RefCell::new(Value::Undef));
        // handle SOF markers: SOF0-SOF15, except DHT(0xc4), JPGA(0xc8) and DAC(0xcc)
        if sof(marker) {
            let mut b = Value::Undef;
            let r = raf_read(it, &raf, &mut b, n(7))?;
            *seg_data.borrow_mut() = b;
            if !pl::num_eq(&r, &n(7)) {
                break;
            }
        // read data for all markers except stand-alone
        // markers 0x00, 0x01 and 0xd0-0xd7 (NULL, TEM, EOI, RST0-RST7)
        } else if marker != 0x00 && marker != 0x01 && marker != 0xd9 && !(0xd0..=0xd7).contains(&marker) {
            // read record length word
            if !pl::num_eq(&raf_read(it, &raf, &mut st.s_, n(2))?, &n(2)) {
                break;
            }
            let len = pl::unpack1("n", &st.s_)?; // get data length
            if !(!len.is_undef() && pl::ge(&len, &n(2))) {
                break;
            }
            seg_pos = raf_tell(it, &raf)?;
            let len = pl::sub(&len, &n(2)); // subtract size of length word
            let mut b = Value::Undef;
            let r = raf_read(it, &raf, &mut b, len.clone())?;
            *seg_data.borrow_mut() = b;
            if !pl::num_eq(&r, &len) {
                break;
            }
        }
        // initialize variables for this segment
        let hdr = Value::bytes([0xffu8, marker as u8]); // segment header
        let marker_name = marker_name_of(it, marker)?;
        let dir_name = if dir_order.is_empty() { Value::Undef } else { dir_order.remove(0) }; // get directory name
//
// create all segments that must come before this one
// (nothing comes before SOI or after SOS)
//
        'create: while !pl::str_eq(&marker_name, "SOI") {
            if add_dirs.exists("JFIF") && done_dir.get("JFIF").is_undef() {
                done_dir.set("JFIF", n(1));
                if !done_dir.get("Adobe").is_undef() {
                    // JFIF overrides Adobe APP14 colour components, so don't allow this
                    // (ref https://docs.oracle.com/javase/8/docs/api/javax/imageio/metadata/doc-files/jpeg_metadata.html)
                    meth(it, &self_, "Warn", vec![s("Not creating JFIF in JPEG with Adobe APP14")])?;
                } else {
                    if verbose.truthy() {
                        vout!(s("Creating APP0:\n"));
                        vout!(s("  Creating JFIF with default values\n"));
                    }
                    let jfif = Value::bytes(b"\x01\x02\x01\0\x48\0\x48\0\0");
                    set_byte_order(it, s("MM"))?;
                    let tag_table_ptr = get_tag_table(it, "Image::ExifTool::JFIF::Main")?;
                    let dir_info = hash_from(&[s("DataPt"), pl::sref(jfif.clone()), s("DirStart"), n(0), s("DirLen"), pl::length(&jfif), s("Parent"), s("JFIF")]);
                    // must temporarily remove JFIF from DEL_GROUP so we can
                    // delete JFIF and add it back again in a single step
                    let del_jfif = del_group.get("JFIF");
                    del_group.delete("JFIF");
                    path.set_at(pn, s("JFIF"));
                    let new_data = meth(it, &self_, "WriteDirectory", vec![dir_info, tag_table_ptr])?;
                    if !del_jfif.is_undef() {
                        del_group.set("JFIF", del_jfif);
                    }
                    if !new_data.is_undef() && pl::length(&new_data).to_i64() > 0 {
                        let app0hdr = cat(&[&Value::bytes(b"\xff\xe0"), &pl::pack("n", &[pl::add(&pl::length(&new_data), &n(7))])?]);
                        wr!(app0hdr, Value::bytes(b"JFIF\0"), new_data);
                    }
                }
            }
            // don't create anything before APP0 or APP1 EXIF (containing IFD0)
            if pl::str_eq(&marker_name, "APP0") || dir_count.get("IFD0").truthy() || dir_count.get("ExtendedEXIF").truthy() {
                break 'create;
            }
            // EXIF information must come immediately after APP0
            if add_dirs.exists("IFD0") && done_dir.get("IFD0").is_undef() {
                done_dir.set("IFD0", n(1));
                if verbose.truthy() {
                    vout!(s("Creating APP1:\n"));
                }
                // write new EXIF data
                self_.set("TIFF_TYPE", s("APP1"));
                let tag_table_ptr = get_tag_table(it, "Image::ExifTool::Exif::Main")?;
                let dir_info = hash_from(&[s("DirName"), s("IFD0"), s("Parent"), s("APP1")]);
                path.set_at(pn, s("APP1"));
                let buff = meth(it, &self_, "WriteDirectory", vec![dir_info, tag_table_ptr, write_tiff_ref.clone()])?;
                if !buff.is_undef() && pl::length(&buff).to_i64() > 0 {
                    if pl::gt(&pl::add(&pl::length(&buff), &pl::length(&exif_app1hdr)), &max_segment_len) {
                        if meth(it, &self_, "Options", vec![s("NoMultiExif")])?.truthy() {
                            meth(it, &self_, "Error", vec![s("EXIF is too large for JPEG segment")])?;
                        } else {
                            meth(it, &self_, "Warn", vec![s("Creating multi-segment EXIF"), n(1)])?;
                        }
                    }
                    // switch to buffered output if required
                    if (self_.get("PREVIEW_INFO").truthy() || self_.get("LeicaTrailer").truthy() || self_.get("HiddenData").truthy()) && !old_outfile.truthy() {
                        write_buffer = pl::sref(s(""));
                        old_outfile = st.outfile.clone();
                        st.outfile = write_buffer.clone();
                        // must account for segment, EXIF and TIFF headers
                        shift_fixups(&self_);
                    }
                    // write as multi-segment
                    let nn = c1(it, write_multi_segment, vec![st.outfile.clone(), n(0xe1), exif_app1hdr.clone(), pl::sref(buff), s("EXIF")])?;
                    if !nn.truthy() {
                        st.err = true;
                    } else if pl::gt(&nn, &n(1)) && old_outfile.truthy() {
                        // (punt on this because updating the pointers would be a real pain)
                        meth(it, &self_, "Error", vec![s("Can't write multi-segment EXIF with external pointers")])?;
                    }
                    changed(&self_);
                }
            }
            // APP13 Photoshop segment next
            if dir_count.get("Photoshop").truthy() {
                break 'create;
            }
            if add_dirs.exists("Photoshop") && done_dir.get("Photoshop").is_undef() {
                done_dir.set("Photoshop", n(1));
                if verbose.truthy() {
                    vout!(s("Creating APP13:\n"));
                }
                // write new APP13 Photoshop record to memory
                let tag_table_ptr = get_tag_table(it, "Image::ExifTool::Photoshop::Main")?;
                let dir_info = hash_from(&[s("Parent"), s("APP13")]);
                path.set_at(pn, s("APP13"));
                let buff = meth(it, &self_, "WriteDirectory", vec![dir_info, tag_table_ptr])?;
                if !buff.is_undef() && pl::length(&buff).to_i64() > 0 {
                    if !c1(it, write_multi_segment, vec![st.outfile.clone(), n(0xed), ps_app13hdr.clone(), pl::sref(buff)])?.truthy() {
                        st.err = true;
                    }
                    changed(&self_);
                }
            }
            // then APP1 XMP segment
            if dir_count.get("XMP").truthy() {
                break 'create;
            }
            if add_dirs.exists("XMP") && done_dir.get("XMP").is_undef() {
                done_dir.set("XMP", n(1));
                if verbose.truthy() {
                    vout!(s("Creating APP1:\n"));
                }
                // write new XMP data
                let tag_table_ptr = get_tag_table(it, "Image::ExifTool::XMP::Main")?;
                // specify MaxDataLen so XMP is split if required
                let dir_info = hash_from(&[s("Parent"), s("APP1"), s("MaxDataLen"), pl::sub(&max_xmp_len, &pl::length(&xmp_app1hdr))]);
                path.set_at(pn, s("APP1"));
                let buff = meth(it, &self_, "WriteDirectory", vec![dir_info.clone(), tag_table_ptr])?;
                if !buff.is_undef() && pl::length(&buff).to_i64() > 0 && !c1(it, write_multi_xmp, vec![self_.clone(), st.outfile.clone(), pl::sref(buff), dir_info.get("ExtendedXMP"), dir_info.get("ExtendedGUID")])?.truthy() {
                    st.err = true;
                }
            }
            // then APP2 ICC_Profile segment
            if dir_count.get("ICC_Profile").truthy() {
                break 'create;
            }
            if add_dirs.exists("ICC_Profile") && done_dir.get("ICC_Profile").is_undef() {
                done_dir.set("ICC_Profile", n(1));
                if del_group.get("ICC_Profile").truthy() && !pl::num_eq(&del_group.get("ICC_Profile"), &n(2)) {
                    continue 'create;
                }
                if verbose.truthy() {
                    vout!(s("Creating APP2:\n"));
                }
                // write new ICC_Profile data
                let tag_table_ptr = get_tag_table(it, "Image::ExifTool::ICC_Profile::Main")?;
                let dir_info = hash_from(&[s("Parent"), s("APP2")]);
                path.set_at(pn, s("APP2"));
                let buff = meth(it, &self_, "WriteDirectory", vec![dir_info, tag_table_ptr])?;
                if !buff.is_undef() && pl::length(&buff).to_i64() > 0 {
                    if !c1(it, write_multi_segment, vec![st.outfile.clone(), n(0xe2), Value::bytes(b"ICC_PROFILE\0"), pl::sref(buff), s("ICC")])?.truthy() {
                        st.err = true;
                    }
                    changed(&self_);
                }
            }
            // then APP12 Ducky segment
            if dir_count.get("Ducky").truthy() {
                break 'create;
            }
            if add_dirs.exists("Ducky") && done_dir.get("Ducky").is_undef() {
                done_dir.set("Ducky", n(1));
                if verbose.truthy() {
                    vout!(s("Creating APP12 Ducky:\n"));
                }
                // write new Ducky segment data
                let tag_table_ptr = get_tag_table(it, "Image::ExifTool::APP12::Ducky")?;
                let dir_info = hash_from(&[s("Parent"), s("APP12")]);
                path.set_at(pn, s("APP12"));
                let buff = meth(it, &self_, "WriteDirectory", vec![dir_info, tag_table_ptr])?;
                if !buff.is_undef() && pl::length(&buff).to_i64() > 0 {
                    let size = pl::add(&pl::length(&buff), &n(5));
                    if pl::le(&size, &max_segment_len) {
                        // write the new segment with appropriate header
                        let app12hdr = cat(&[&Value::bytes(b"\xff\xec"), &pl::pack("n", &[pl::add(&size, &n(2))])?]);
                        wr!(app12hdr, s("Ducky"), buff);
                    } else {
                        meth(it, &self_, "Warn", vec![s(&format!("APP12 Ducky segment too large! ({} bytes)", size.to_text()))])?;
                    }
                }
            }
            // then APP14 Adobe segment
            if dir_count.get("Adobe").truthy() {
                break 'create;
            }
            if add_dirs.exists("Adobe") && done_dir.get("Adobe").is_undef() {
                done_dir.set("Adobe", n(1));
                let buff = meth(it, &self_, "GetNewValue", vec![s("Adobe")])?;
                if buff.truthy() {
                    if verbose.truthy() {
                        vout!(s("Creating APP14:\n  Creating Adobe segment\n"));
                    }
                    let size = pl::length(&buff);
                    if pl::le(&size, &max_segment_len) {
                        // write the new segment with appropriate header
                        let app14hdr = cat(&[&Value::bytes(b"\xff\xee"), &pl::pack("n", &[pl::add(&size, &n(2))])?]);
                        wr!(app14hdr, buff);
                        changed(&self_);
                    } else {
                        meth(it, &self_, "Warn", vec![s(&format!("APP14 Adobe segment too large! ({} bytes)", size.to_text()))])?;
                    }
                }
            }
            // finally, COM segment
            if dir_count.get("COM").truthy() {
                break 'create;
            }
            if add_dirs.exists("COM") && done_dir.get("COM").is_undef() {
                done_dir.set("COM", n(1));
                if del_group.get("File").truthy() && !pl::num_eq(&del_group.get("File"), &n(2)) {
                    continue 'create;
                }
                let new_comment = meth(it, &self_, "GetNewValue", vec![s("Comment")])?;
                if !new_comment.is_undef() {
                    if verbose.truthy() {
                        vout!(s("Creating COM:\n"));
                        verbose_value(it, vec![self_.clone(), s("+ Comment"), new_comment.clone()], Want::Void)?;
                    }
                    if !c1(it, write_multi_segment, vec![st.outfile.clone(), n(0xfe), s(""), pl::sref(new_comment)])?.truthy() {
                        st.err = true;
                    }
                    changed(&self_);
                }
            }
            break 'create; // didn't want to loop anyway
        }
        path.set_at(pn, marker_name.clone());
        // decrement counter for this directory since we are about to process it
        bump(&dir_count, dir_name.to_bytes(), -1);
//
// rewrite existing segments
//
        // handle SOF markers: SOF0-SOF15, except DHT(0xc4), JPGA(0xc8) and DAC(0xcc)
        if sof(marker) {
            if verbose.truthy() {
                vout!(cat(&[&s("JPEG "), &marker_name, &s(":\n")]));
            }
            let d = seg_data.borrow().clone();
            wr!(hdr.clone(), d);
            continue 'marker;
        } else if marker == 0xda {
            // SOS
            if let Value::ArrayRef(p) = &path {
                p.borrow_mut().pop();
            }
            if verbose.truthy() {
                vout!(s("JPEG SOS\n"));
            }
            // write SOS segment
            let d = seg_data.borrow().clone();
            st.s_ = pl::pack("n", &[pl::add(&pl::length(&d), &n(2))])?;
            wr!(hdr.clone(), st.s_.clone(), d);
            let mut buff = Value::Undef;
            let mut end_pos;
            let mut trail_info = Value::Undef;
            let mut del_preview = self_.get("DEL_PREVIEW");
            if !del_group.get("Trailer").truthy() {
                trail_info = meth(it, &self_, "IdentifyTrailer", vec![raf.clone()])?;
            }
            let nv_trail = meth(it, &self_, "GetNewValueHash", vec![extra.get("Trailer")])?;
            if !(old_outfile.truthy() || del_preview.truthy() || trail_info.truthy() || del_group.get("Trailer").truthy() || nv_trail.truthy() || self_.get("HiddenData").truthy()) {
                // blindly copy the rest of the file
                while raf_read(it, &raf, &mut buff, n(65536))?.truthy() {
                    if !write_v(it, &st.outfile, &[buff.clone()])? {
                        st.err = true;
                        break;
                    }
                }
                st.rtn_val = 1; // success unless we have a file write error
                break 'marker; // all done
            }
            // write the rest of the image (as quickly as possible) up to the EOI
            let mut ended_with_ff = false;
            loop {
                let nn = raf_read(it, &raf, &mut buff, n(65536))?;
                if !nn.truthy() {
                    break 'marker;
                }
                let mut hit = None;
                if ended_with_ff {
                    hit = match_g(r"^\xd9", "s", &buff)?;
                }
                if hit.is_none() {
                    hit = match_g(r"\xff\xd9", "s", &buff)?;
                }
                if let Some((pos, _)) = hit {
                    st.rtn_val = 1; // the JPEG is OK
                    // write up to the EOI
                    wr!(substr(&buff, &n(0), Some(&n(pos as i64))));
                    buff = substr(&buff, &n(pos as i64), None);
                    break;
                }
                if !pl::num_eq(&nn, &n(65536)) {
                    meth(it, &self_, "Error", vec![s("JPEG EOI marker not found")])?;
                    break 'marker;
                }
                wr!(buff.clone());
                ended_with_ff = is(&substr(&buff, &n(65535), Some(&n(1))), b"\xff");
            }
            // remember position of last data copied
            end_pos = pl::sub(&raf_tell(it, &raf)?, &pl::length(&buff));
            self_.set("TrailerStart", end_pos.clone());
            // write new trailer if specified
            if nv_trail.truthy() {
                // access new value directly to avoid copying a potentially very large data block
                if nv_trail.get("Value").truthy() && nv_trail.get("Value").at(0).truthy() {
                    // (note: "0" will also delete the trailer)
                    meth(it, &self_, "VPrint", vec![n(0), s("  Writing new trailer")])?;
                    wr!(nv_trail.get("Value").at(0));
                    changed(&self_);
                } else if raf_seek(it, &raf, n(0), 2)? && !pl::num_eq(&raf_tell(it, &raf)?, &end_pos) {
                    let t = raf_tell(it, &raf)?;
                    meth(it, &self_, "VPrint", vec![n(0), s("  Deleting trailer ("), pl::sub(&t, &end_pos), s(" bytes)")])?;
                    changed(&self_); // changed if there was previously a trailer
                }
                break 'marker; // all done
            }
            // rewrite existing trailers into buffer
            if trail_info.truthy() {
                let tbuf = pl::sref(s(""));
                raf_seek(it, &raf, pl::neg(&pl::length(&buff)), 1)?; // seek back to just after EOI
                trail_info.set("OutFile", tbuf); // rewrite the trailer
                trail_info.set("ScanForTrailer", n(1)); // scan if necessary
                if !meth(it, &self_, "ProcessTrailers", vec![trail_info.clone()])?.truthy() {
                    trail_info = Value::Undef;
                }
            }
            if old_outfile.truthy() {
                let mut preview_info = Value::Undef;
                // copy HiddenData if necessary
                if self_.get("HiddenData").truthy() {
                    let mut pad = Value::Undef;
                    let hd = self_.get("HiddenData");
                    let hd_off = pl::add(&hd.get("Offset"), &hd.get("Base"));
                    crate::runtime::require(it, "Image::ExifTool::Sony")?;
                    // read HiddenData, updating $hdOff with actual offset if necessary
                    let mut args = [self_.clone(), hd_off, hd.get("Size")];
                    let mut data_pt = call_mut(it, "Image::ExifTool::Sony::ReadHiddenData", &mut args, Want::Scalar)?.pop().unwrap_or_default();
                    let hd_off = args[1].clone();
                    if data_pt.truthy() {
                        // preserve padding to avoid invalidating MPF pointers (yuk!)
                        let pad_len = pl::sub(&hd_off, &end_pos);
                        let ok = pl::ge(&pad_len, &n(0)) && raf_seek(it, &raf, end_pos.clone(), 0)? && pl::num_eq(&raf_read(it, &raf, &mut pad, pad_len.clone())?, &pad_len);
                        if !ok {
                            meth(it, &self_, "Error", vec![s("Error reading HiddenData padding"), n(1)])?;
                            pad = s("");
                        }
                        end_pos = pl::add(&end_pos, &pl::add(&pl::length(&pad), &pl::length(&deref(&data_pt)?))); // update end position
                    } else {
                        // $$dataPt = $pad = '' (autovivifies $dataPt)
                        pad = s("");
                        if data_pt.is_ref() {
                            set_deref(&data_pt, s(""))?;
                        } else {
                            data_pt = pl::sref(s(""));
                        }
                    }
                    let fixup = self_.get("HiddenData").get("Fixup");
                    // set MakerNote pointer and size (subtract 10 for segment and EXIF headers)
                    let p = pl::sub(&pl::add(&pl::length(&deref(&st.outfile)?), &pl::length(&pad)), &n(10));
                    meth(it, &fixup, "SetMarkerPointers", vec![st.outfile.clone(), s("HiddenData"), p])?;
                    let wb = deref(&write_buffer)?;
                    set_deref(&write_buffer, cat(&[&wb, &pad, &deref(&data_pt)?]))?; // keep padding for now
                }
                if self_.get("LeicaTrailer").truthy() {
                    let trail_len;
                    if trail_info.truthy() {
                        trail_len = pl::sub(&trail_info.get("DataPos"), &end_pos);
                    } else {
                        if !raf_seek(it, &raf, n(0), 2)? {
                            st.err = true;
                        }
                        trail_len = pl::sub(&raf_tell(it, &raf)?, &end_pos);
                    }
                    let lt = self_.get("LeicaTrailer");
                    let fixup = lt.get("Fixup");
                    lt.set("TrailPos", end_pos.clone());
                    lt.set("TrailLen", trail_len);
                    // get _absolute_ position of new Leica trailer
                    let abs_pos = pl::add(&tell_v(it, &old_outfile)?, &pl::length(&deref(&st.outfile)?));
                    crate::runtime::require(it, "Image::ExifTool::Panasonic")?;
                    let dat = pl::call(it, "Image::ExifTool::Panasonic::ProcessLeicaTrailer", vec![self_.clone(), abs_pos])?;
                    // allow some junk before Leica trailer (just in case)
                    let junk = pl::sub(&self_.get("LeicaTrailerPos"), &end_pos);
                    // set MakerNote pointer and size (subtract 10 for segment and EXIF headers)
                    let p = pl::add(&pl::sub(&pl::length(&deref(&st.outfile)?), &n(10)), &junk);
                    meth(it, &fixup, "SetMarkerPointers", vec![st.outfile.clone(), s("LeicaTrailer"), p])?;
                    // use this fixup to set the size too (sneaky)
                    let trail_size = if !dat.is_undef() { pl::sub(&pl::length(&dat), &junk) } else { self_.get("LeicaTrailer").get("Size") };
                    fixup.set("Start", pl::sub(&fixup.get("Start"), &n(4)));
                    fixup.set("Shift", pl::add(&fixup.get("Shift"), &n(4)));
                    if !trail_size.is_undef() {
                        meth(it, &fixup, "SetMarkerPointers", vec![st.outfile.clone(), s("LeicaTrailer"), trail_size])?;
                    }
                    fixup.set("Start", pl::add(&fixup.get("Start"), &n(4)));
                    fixup.set("Shift", pl::sub(&fixup.get("Shift"), &n(4)));
                    if !dat.is_undef() {
                        wr!(dat); // write new Leica trailer
                        del_preview = n(1); // delete existing Leica trailer
                    }
                }
                // handle preview image last
                if self_.get("PREVIEW_INFO").truthy() {
                    // locate preview image and fix up preview offsets
                    let sony = re(r"^SONY", "i").is_match(&self_.get("Make"));
                    let scan_len = n(if sony { 65536 } else { 1024 });
                    if pl::lt(&pl::length(&buff), &scan_len) {
                        // make sure we have enough trailer to scan
                        let mut buf2 = Value::Undef;
                        if raf_read(it, &raf, &mut buf2, pl::sub(&scan_len, &pl::length(&buff)))?.truthy() {
                            buff = cat(&[&buff, &buf2]);
                        }
                    }
                    // get new preview image position, relative to EXIF base
                    let mut new_pos = pl::sub(&pl::length(&deref(&st.outfile)?), &n(10)); // (subtract 10 for segment and EXIF headers)
                    let mut junk_len = Value::Undef;
                    // adjust position if image isn't at the start (eg. Olympus E-1/E-300)
                    if let Some((p, c)) = match_g(r"(\xff\xd8\xff.|.\xd8\xff\xdb)(..)", "s", &buff)? {
                        let (jpeg_hdr, mut seg_len) = (cap(&c, 1), cap(&c, 2));
                        junk_len = n(p as i64 - 6);
                        // Sony previewimage trailer has a 32 byte header
                        if sony && pl::gt(&junk_len, &n(32)) {
                            // with some newer Sony models, the makernotes preview pointer
                            // points to JPEG at end of EXIF inside MPImage preview (what a pain!)
                            if is(&jpeg_hdr, b"\xff\xd8\xff\xe1") {
                                // is the first segment EXIF?
                                seg_len = pl::unpack1("n", &seg_len)?; // the EXIF segment length
                                // Sony PreviewImage starts with last 2 bytes of EXIF segment
                                // (and first byte is usually "\0", not "\xff", so don't check this)
                                if pl::gt(&pl::length(&buff), &pl::add(&pl::add(&junk_len, &seg_len), &n(6))) && is(&substr(&buff, &pl::add(&pl::add(&junk_len, &seg_len), &n(3)), Some(&n(3))), b"\xd8\xff\xdb") {
                                    junk_len = pl::add(&junk_len, &pl::add(&seg_len, &n(2)));
                                    // (note: this will not copy the trailer after PreviewImage,
                                    //  which is a 14kB block full of zeros for the A77)
                                }
                            }
                            junk_len = pl::sub(&junk_len, &n(32));
                        }
                        new_pos = pl::add(&new_pos, &junk_len);
                    }
                    // fix up the preview offsets to point to the start of the new image
                    preview_info = self_.get("PREVIEW_INFO");
                    self_.delete("PREVIEW_INFO");
                    let fixup = preview_info.get("Fixup");
                    new_pos = pl::add(&new_pos, &or(preview_info.get("BaseShift"), n(0)));
                    // adjust to absolute file offset if necessary (Samsung STMN)
                    if preview_info.get("Absolute").truthy() {
                        new_pos = pl::add(&new_pos, &pl::add(&tell_v(it, &old_outfile)?, &n(10)));
                    }
                    if preview_info.get("Relative").truthy() {
                        // adjust for our base by looking at how far the pointer got shifted
                        let g = meth(it, &fixup, "GetMarkerPointers", vec![st.outfile.clone(), s("PreviewImage")])?;
                        new_pos = pl::sub(&new_pos, &or(g, n(0)));
                    } else if preview_info.get("ChangeBase").truthy() {
                        // Leica S2 uses relative offsets for the preview only (leica sucks)
                        let maker_offset = meth(it, &fixup, "GetMarkerPointers", vec![st.outfile.clone(), s("LeicaTrailer")])?;
                        if maker_offset.truthy() {
                            new_pos = pl::sub(&new_pos, &maker_offset);
                        }
                    }
                    meth(it, &fixup, "SetMarkerPointers", vec![st.outfile.clone(), s("PreviewImage"), new_pos])?;
                    if !pl::str_eq(&preview_info.get("Data"), "LOAD_PREVIEW") && junk_len.truthy() {
                        // write any junk that existed before the preview image
                        preview_info.set("Junk", substr(&buff, &n(0), Some(&junk_len)));
                    }
                }
                // clean up and write the buffered data
                st.outfile = old_outfile.clone();
                old_outfile = Value::Undef;
                let wb = deref(&write_buffer)?;
                wr!(wb);
                write_buffer = Value::Undef;
                // write preview image
                if preview_info.truthy() && !pl::str_eq(&preview_info.get("Data"), "LOAD_PREVIEW") {
                    // write any junk that existed before the preview image
                    if !preview_info.get("Junk").is_undef() {
                        wr!(preview_info.get("Junk"));
                    }
                    // write the saved preview image
                    wr!(preview_info.get("Data"));
                    preview_info.delete("Data");
                    // (don't increment CHANGED because we could be rewriting existing preview)
                    del_preview = n(1); // remove old preview
                }
            }
            // copy over preview image (or other data) if necessary
            if !del_preview.truthy() {
                let extra_len;
                if trail_info.truthy() {
                    // copy everything up to start of first processed trailer
                    extra_len = if !trail_info.get("DataPos").is_undef() { pl::sub(&trail_info.get("DataPos"), &end_pos) } else { n(0) };
                } else {
                    // copy everything up to end of file
                    if !raf_seek(it, &raf, n(0), 2)? {
                        st.err = true;
                    }
                    extra_len = pl::sub(&raf_tell(it, &raf)?, &end_pos);
                }
                if pl::gt(&extra_len, &n(0)) {
                    if del_group.get("Trailer").truthy() {
                        if verbose.truthy() {
                            vout!(cat(&[&s("  Deleting unknown trailer ("), &extra_len, &s(" bytes)\n")]));
                        }
                        changed(&self_);
                    } else {
                        // copy over unknown trailer
                        if verbose.truthy() {
                            vout!(cat(&[&s("  Preserving unknown trailer ("), &extra_len, &s(" bytes)\n")]));
                        }
                        if !raf_seek(it, &raf, end_pos.clone(), 0)? {
                            st.err = true;
                        }
                        if !copy_block_v(it, &raf, &st.outfile, extra_len)?.truthy() {
                            st.err = true;
                        }
                    }
                }
            }
            // write trailer if necessary
            if trail_info.truthy() {
                if !c1(it, write_trailer_buffer, vec![self_.clone(), trail_info.clone(), st.outfile.clone()])?.truthy() {
                    st.err = true;
                }
                trail_info = Value::Undef;
            }
            break 'marker; // all done parsing file
        } else if marker == 0xd9 && is_exv {
            // write EXV EOI (any trailer will be lost)
            wr!(Value::bytes(b"\xff\xd9"));
            st.rtn_val = 1;
            break 'marker;
        } else if marker == 0x00 || marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            if verbose.truthy() && marker != 0 {
                vout!(cat(&[&s("JPEG "), &marker_name, &s(":\n")]));
            }
            // handle stand-alone markers 0x00, 0x01 and 0xd0-0xd7 (NULL, TEM, RST0-RST7)
            wr!(hdr.clone());
            continue 'marker;
        }
        //
        // NOTE: A 'next' statement after this point will cause $$segDataPt
        //       not to be written if there is an output file, so in this case
        //       the $$self{CHANGED} flags must be updated
        //
        let mut seg_data_pt = Value::ScalarRef(seg_data.clone());
        length = pl::length(&seg_data.borrow());
        if verbose.truthy() {
            vout!(cat(&[&s("JPEG "), &marker_name, &s(" ("), &length, &s(" bytes)\n")]));
        }
        // group delete of APP segments
        if del_group.get(dir_name.to_bytes()).truthy() {
            if verbose.truthy() {
                vout!(cat(&[&s("  Deleting "), &dir_name, &s(" segment\n")]));
            }
            if pl::str_eq(&dir_name, "ICC_Profile") {
                meth(it, &self_, "Warn", vec![s("ICC_Profile deleted. Image colors may be affected")])?;
            }
            changed(&self_);
            continue 'marker;
        }
        let mut seg_type = Value::Undef;
        let mut del = false;
        // rewrite this segment only if we are changing a tag which is contained in its
        // directory (or deleting '*', in which case we need to identify the segment type)
        #[allow(clippy::never_loop)]
        'edit: while edit_dirs.exists(marker_name.to_bytes()) || del_group.get("*").truthy() {
            let sd = deref(&seg_data_pt)?;
            if marker == 0xe0 {
                // APP0 (JFIF, CIFF)
                if re(r"^JFIF\0", "").is_match(&sd) {
                    seg_type = s("JFIF");
                    if del_group.get("JFIF").truthy() {
                        del = true;
                        break 'edit;
                    }
                    if !edit_dirs.get("JFIF").truthy() {
                        break 'edit;
                    }
                    set_byte_order(it, s("MM"))?;
                    let tag_table_ptr = get_tag_table(it, "Image::ExifTool::JFIF::Main")?;
                    let dir_info = hash_from(&[s("DataPt"), seg_data_pt.clone(), s("DataPos"), seg_pos.clone(), s("DataLen"), length.clone(), s("DirStart"), n(5), s("DirLen"), pl::sub(&length, &n(5)), s("Parent"), marker_name.clone()]);
                    let new_data = meth(it, &self_, "WriteDirectory", vec![dir_info, tag_table_ptr])?;
                    if !new_data.is_undef() && pl::length(&new_data).to_i64() > 0 {
                        set_deref(&seg_data_pt, cat(&[&Value::bytes(b"JFIF\0"), &new_data]))?;
                    }
                } else if re(r"^JFXX\0\x10", "").is_match(&sd) {
                    seg_type = s("JFXX");
                    if del_group.get("JFIF").truthy() {
                        del = true;
                    }
                } else if re(r"^(II|MM).{4}HEAPJPGM", "s").is_match(&sd) {
                    seg_type = s("CIFF");
                    if del_group.get("CIFF").truthy() {
                        del = true;
                        break 'edit;
                    }
                    if !edit_dirs.get("CIFF").truthy() {
                        break 'edit;
                    }
                    let new_data = pl::sref(s(""));
                    let raf2 = pl::call(it, "File::RandomAccess::new", vec![s("File::RandomAccess"), seg_data_pt.clone()])?;
                    let dir_info = hash_from(&[s("RAF"), raf2, s("OutFile"), new_data.clone()]);
                    crate::runtime::require(it, "Image::ExifTool::CanonRaw")?;
                    if pl::gt(&pl::call(it, "Image::ExifTool::CanonRaw::WriteCRW", vec![self_.clone(), dir_info])?, &n(0)) {
                        if pl::length(&new_data.deref()).to_i64() > 0 {
                            set_deref(&seg_data_pt, new_data.deref())?;
                        } else {
                            seg_data_pt = Value::Undef;
                            del = true; // delete this segment
                        }
                    }
                }
            } else if marker == 0xe1 {
                // APP1 (EXIF, XMP)
                // check for EXIF data
                if let Some(m) = re(r"^(.{0,4})Exif\0.", "is").m(&sd) {
                    let mut hdr_len = pl::length(&exif_app1hdr);
                    if pl::length(&cap(&m, 1)).to_i64() > 0 {
                        hdr_len = pl::add(&hdr_len, &pl::length(&cap(&m, 1)));
                        meth(it, &self_, "Error", vec![s("Unknown garbage at start of EXIF segment"), n(1)])?;
                    } else if !re(r"^Exif\0", "").is_match(&sd) {
                        meth(it, &self_, "Error", vec![s("Incorrect EXIF segment identifier"), n(1)])?;
                    }
                    seg_type = s("EXIF");
                    if !edit_dirs.get("IFD0").truthy() {
                        break 'edit;
                    }
                    // add this data to the combined data if it exists
                    if !combined_seg_data.borrow().is_undef() {
                        let c = combined_seg_data.borrow().clone();
                        let more = substr(&deref(&seg_data_pt)?, &hdr_len, None);
                        *combined_seg_data.borrow_mut() = cat(&[&c, &more]);
                        seg_data_pt = combined();
                        seg_pos = first_seg_pos.clone();
                        length = pl::length(&combined_seg_data.borrow()); // update length
                    }
                    // peek ahead to see if the next segment is extended EXIF
                    if dir_order.first().is_some_and(|d| pl::str_eq(d, "ExtendedEXIF")) {
                        // initialize combined data if necessary
                        if combined_seg_data.borrow().is_undef() {
                            let d = deref(&seg_data_pt)?;
                            *combined_seg_data.borrow_mut() = d;
                            first_seg_pos = seg_pos.clone();
                            meth(it, &self_, "Warn", vec![s("File contains multi-segment EXIF"), n(1)])?;
                        }
                        continue 'marker; // get the next segment to combine
                    }
                    if done_dir.get("IFD0").truthy() {
                        meth(it, &self_, "Warn", vec![s("Multiple APP1 EXIF records")])?;
                    }
                    done_dir.set("IFD0", n(1));
                    // check del groups now so we can change byte order in one step
                    if del_group.get("IFD0").truthy() || del_group.get("EXIF").truthy() {
                        done_dir.delete("IFD0"); // delete so we will create a new one
                        del = true;
                        break 'edit;
                    }
                    // rewrite EXIF as if this were a TIFF file in memory
                    let dir_info = hash_from(&[
                        s("DataPt"),
                        seg_data_pt.clone(),
                        s("DataPos"),
                        pl::neg(&hdr_len), // (remember: relative to Base!)
                        s("DirStart"),
                        hdr_len.clone(),
                        s("Base"),
                        pl::add(&seg_pos, &hdr_len),
                        s("Parent"),
                        marker_name.clone(),
                        s("DirName"),
                        s("IFD0"),
                    ]);
                    // write new EXIF data to memory
                    let tag_table_ptr = get_tag_table(it, "Image::ExifTool::Exif::Main")?;
                    let buff = meth(it, &self_, "WriteDirectory", vec![dir_info, tag_table_ptr, write_tiff_ref.clone()])?;
                    if !buff.is_undef() {
                        set_deref(&seg_data_pt, Value::Undef)?; // free the old buffer
                        seg_data_pt = pl::sref(buff);
                    } else if !meth(it, &self_, "Options", vec![s("IgnoreMinorErrors")])?.truthy() {
                        break 'marker;
                    }
                    // delete segment if IFD contains no entries
                    if pl::length(&deref(&seg_data_pt)?).to_i64() == 0 {
                        del = true;
                        break 'edit;
                    }
                    if pl::gt(&pl::add(&pl::length(&deref(&seg_data_pt)?), &pl::length(&exif_app1hdr)), &max_segment_len) {
                        if meth(it, &self_, "Options", vec![s("NoMultiExif")])?.truthy() {
                            meth(it, &self_, "Error", vec![s("EXIF is too large for JPEG segment")])?;
                        } else {
                            meth(it, &self_, "Warn", vec![s("Writing multi-segment EXIF"), n(1)])?;
                        }
                    }
                    // switch to buffered output if required
                    if (self_.get("PREVIEW_INFO").truthy() || self_.get("LeicaTrailer").truthy() || self_.get("HiddenData").truthy()) && !old_outfile.truthy() {
                        write_buffer = pl::sref(s(""));
                        old_outfile = st.outfile.clone();
                        st.outfile = write_buffer.clone();
                        // must account for segment, EXIF and TIFF headers
                        shift_fixups(&self_);
                    }
                    // write as multi-segment
                    let nn = c1(it, write_multi_segment, vec![st.outfile.clone(), n(marker), exif_app1hdr.clone(), seg_data_pt.clone(), s("EXIF")])?;
                    if !nn.truthy() {
                        st.err = true;
                    } else if pl::gt(&nn, &n(1)) && old_outfile.truthy() {
                        // (punt on this because updating the pointers would be a real pain)
                        meth(it, &self_, "Error", vec![s("Can't write multi-segment EXIF with external pointers")])?;
                    }
                    *combined_seg_data.borrow_mut() = Value::Undef;
                    set_deref(&seg_data_pt, Value::Undef)?;
                    continue 'marker;
                // check for XMP data
                } else if {
                    let mut p = b"^(".to_vec();
                    p.extend_from_slice(&xmp_app1hdr.to_bytes());
                    p.push(b'|');
                    p.extend_from_slice(&xmp_ext_app1hdr.to_bytes());
                    p.push(b')');
                    pl::re_dyn(&p, "")?.is_match(&sd)
                } {
                    seg_type = s("XMP");
                    if del_group.get("XMP").truthy() {
                        del = true;
                        break 'edit;
                    }
                    bump(&done_dir, "XMP", 1);
                    if !edit_dirs.get("XMP").truthy() {
                        break 'edit;
                    }
                    if pl::gt(&pl::add(&done_dir.get("XMP"), &dir_count.get("XMP")), &n(1)) {
                        // must assemble all XMP segments before writing
                        let mut ext_xmp;
                        let sd = deref(&seg_data_pt)?;
                        if re_hdr(&xmp_ext_app1hdr)?.is_match(&sd) {
                            // save extended XMP data
                            if pl::lt(&pl::length(&sd), &n(75)) {
                                extended_xmp.set("Error", s("Truncated data"));
                            } else {
                                let v = pl::unpack("x67N2", &sd)?;
                                let (size, off) = (arg(&v, 0), arg(&v, 1));
                                let guid = substr(&sd, &n(35), Some(&n(32)));
                                if re(r"[^A-Za-z0-9]", "").is_match(&guid) {
                                    // (technically, should be uppercase)
                                    extended_xmp.set("Error", s("Invalid GUID"));
                                } else {
                                    // remember extended data for each GUID
                                    ext_xmp = extended_xmp.get(guid.to_bytes());
                                    if ext_xmp.truthy() {
                                        if !pl::num_eq(&size, &ext_xmp.get("Size")) {
                                            extended_xmp.set("Error", s("Inconsistent size"));
                                        }
                                    } else {
                                        ext_xmp = Value::new_hash();
                                        extended_xmp.set(guid.to_bytes(), ext_xmp.clone());
                                    }
                                    ext_xmp.set("Size", size);
                                    ext_xmp.set(off.to_bytes(), substr(&sd, &n(75), None));
                                }
                            }
                        } else {
                            // save all main XMP segments (should normally be only one)
                            extended_xmp.get_or_new_array("Main").push(substr(&sd, &pl::length(&xmp_app1hdr), None));
                        }
                        // continue processing only if we have read all the segments
                        if dir_count.get("XMP").truthy() {
                            continue 'marker;
                        }
                        // reconstruct an XMP super-segment
                        let mut super_ = xmp_app1hdr.to_bytes().to_vec();
                        let mut good_guid = s("");
                        for x in extended_xmp.get("Main").items() {
                            // get the HasExtendedXMP GUID if it exists
                            if let Some(m) = re(r#":HasExtendedXMP\s*(=\s*['"]|>)(\w{32})"#, "").m(&x) {
                                // warn of subsequent XMP blocks specifying a different
                                // HasExtendedXMP (have never seen this)
                                if good_guid.truthy() && !is(&good_guid, &cap(&m, 2).to_bytes()) {
                                    meth(it, &self_, "Warn", vec![s("Multiple XMP segments specifying different extended XMP GUID")])?;
                                }
                                good_guid = cap(&m, 2); // GUID for the standard extended XMP
                            }
                            super_.extend_from_slice(&x.to_bytes());
                        }
                        set_deref(&seg_data_pt, Value::bytes(super_))?;
                        // GUID of the extended XMP that we want to read
                        let mut read_guid = or(self_.path(&["OPTIONS", "ExtendedXMP"]), n(0));
                        if pl::str_eq(&read_guid, "1") {
                            read_guid = good_guid.clone();
                        }
                        let mut keys: Vec<Rc<[u8]>> = extended_xmp.keys();
                        keys.sort();
                        for guid in keys {
                            if guid.len() != 32 {
                                continue; // ignore other (internal) keys
                            }
                            let guid_v = Value::Str(guid.clone());
                            if !is(&guid_v, &read_guid.to_bytes()) && !pl::str_eq(&read_guid, "2") {
                                let non = if is(&guid_v, &good_guid.to_bytes()) { "" } else { "non-" };
                                meth(it, &self_, "Warn", vec![s(&format!("Ignored {non}standard extended XMP (GUID {})", guid_v.to_text()))])?;
                                continue;
                            }
                            if !is(&guid_v, &good_guid.to_bytes()) {
                                meth(it, &self_, "Warn", vec![s(&format!("Reading non-standard extended XMP (GUID {})", guid_v.to_text()))])?;
                            }
                            ext_xmp = extended_xmp.get(&guid);
                            if ext_xmp.ref_type() != "HASH" {
                                continue; // (just to be safe)
                            }
                            let size = ext_xmp.get("Size");
                            let mut offsets: Vec<Value> = Vec::new();
                            let mut off = n(0);
                            while pl::lt(&off, &size) {
                                if ext_xmp.get(off.to_bytes()).is_undef() {
                                    break;
                                }
                                offsets.push(off.clone());
                                off = pl::add(&off, &pl::length(&ext_xmp.get(off.to_bytes())));
                            }
                            if pl::num_eq(&off, &size) {
                                // add all XMP to super-segment
                                let mut b = deref(&seg_data_pt)?.to_bytes().to_vec();
                                for o in &offsets {
                                    b.extend_from_slice(&ext_xmp.get(o.to_bytes()).to_bytes());
                                }
                                set_deref(&seg_data_pt, Value::bytes(b))?;
                            } else {
                                meth(it, &self_, "Error", vec![s(&format!("Incomplete extended XMP (GUID {})", guid_v.to_text())), n(1)])?;
                            }
                        }
                        if extended_xmp.get("Error").truthy() {
                            meth(it, &self_, "Error", vec![cat(&[&extended_xmp.get("Error"), &s(" in extended XMP")]), n(1)])?;
                        }
                    }
                    let start = pl::length(&xmp_app1hdr);
                    let tag_table_ptr = get_tag_table(it, "Image::ExifTool::XMP::Main")?;
                    // limit XMP size and create extended XMP if necessary
                    let dir_info = hash_from(&[s("DataPt"), seg_data_pt.clone(), s("DirStart"), start, s("Parent"), marker_name.clone(), s("MaxDataLen"), pl::sub(&max_xmp_len, &pl::length(&xmp_app1hdr))]);
                    let new_data = meth(it, &self_, "WriteDirectory", vec![dir_info.clone(), tag_table_ptr])?;
                    if !new_data.is_undef() {
                        extended_xmp = Value::new_hash();
                        if pl::length(&new_data).to_i64() > 0 {
                            // write multi-segment XMP (XMP plus extended XMP if necessary)
                            if !c1(it, write_multi_xmp, vec![self_.clone(), st.outfile.clone(), pl::sref(new_data), dir_info.get("ExtendedXMP"), dir_info.get("ExtendedGUID")])?.truthy() {
                                st.err = true;
                            }
                            set_deref(&seg_data_pt, Value::Undef)?; // free the old buffer
                            continue 'marker;
                        } else {
                            set_deref(&seg_data_pt, s(""))?; // delete the XMP
                        }
                    } else {
                        if verbose.truthy() {
                            vout!(s("    [XMP rewritten with no changes]\n"));
                        }
                        if pl::gt(&done_dir.get("XMP"), &n(1)) {
                            // re-write original multi-segment XMP
                            for dat in extended_xmp.get("Main").items() {
                                // main XMP
                                if pl::length(&dat).to_i64() == 0 {
                                    continue;
                                }
                                st.s_ = pl::pack("n", &[pl::add(&pl::add(&pl::length(&xmp_app1hdr), &pl::length(&dat)), &n(2))])?;
                                wr!(hdr.clone(), st.s_.clone(), xmp_app1hdr.clone(), dat);
                            }
                            let mut keys: Vec<Rc<[u8]>> = extended_xmp.keys();
                            keys.sort();
                            for guid in keys {
                                // extended XMP
                                if guid.len() != 32 {
                                    continue;
                                }
                                let ext_xmp = extended_xmp.get(&guid);
                                if ext_xmp.ref_type() != "HASH" {
                                    continue;
                                }
                                let size = ext_xmp.get("Size");
                                if !size.truthy() {
                                    continue;
                                }
                                let mut off = n(0);
                                while !ext_xmp.get(off.to_bytes()).is_undef() {
                                    let chunk = ext_xmp.get(off.to_bytes());
                                    st.s_ = pl::pack("n", &[pl::add(&pl::add(&pl::length(&xmp_ext_app1hdr), &pl::length(&chunk)), &n(42))])?;
                                    let n2 = pl::pack("N2", &[size.clone(), off.clone()])?;
                                    wr!(hdr.clone(), st.s_.clone(), xmp_ext_app1hdr.clone(), Value::Str(guid.clone()), n2, chunk.clone());
                                    off = pl::add(&off, &pl::length(&chunk));
                                }
                            }
                            set_deref(&seg_data_pt, Value::Undef)?; // free the old buffer
                            extended_xmp = Value::new_hash();
                            continue 'marker;
                        }
                        // continue on to re-write original single-segment XMP
                    }
                    if pl::length(&deref(&seg_data_pt)?).to_i64() == 0 {
                        del = true;
                    }
                } else if re(r"^http", "").is_match(&sd) || re(r"<exif:", "").is_match(&sd) {
                    meth(it, &self_, "Warn", vec![s("Ignored APP1 XMP segment with non-standard header"), n(1)])?;
                }
            } else if marker == 0xe2 {
                // APP2 (ICC Profile, FPXR, MPF)
                if re(r"^ICC_PROFILE\0", "").is_match(&sd) && pl::ge(&length, &n(14)) {
                    seg_type = s("ICC_Profile");
                    if del_group.get("ICC_Profile").truthy() {
                        del = true;
                        break 'edit;
                    }
                    // must concatenate blocks of profile
                    let mut chunk_num = c1(it, part2::get8u, vec![seg_data_pt.clone(), n(12)])?;
                    let chunks_tot = c1(it, part2::get8u, vec![seg_data_pt.clone(), n(13)])?;
                    if !icc_chunks_total.is_undef() {
                        // abort parsing ICC_Profile if the total chunk count is inconsistent
                        if !pl::num_eq(&chunks_tot, &icc_chunks_total) && !icc_chunk_count.is_undef() {
                            // an error because the accumulated profile data will be lost
                            meth(it, &self_, "Error", vec![s("Inconsistent ICC_Profile chunk count"), n(1)])?;
                            icc_chunk_count = Value::Undef; // abort ICC_Profile parsing
                            chunk_num = Value::Undef; // avoid 2nd warning below
                            changed(&self_); // we are deleting the bad chunks before this one
                        }
                    } else {
                        icc_chunk_count = n(0);
                        icc_chunks_total = chunks_tot.clone();
                        if !chunks_tot.truthy() {
                            meth(it, &self_, "Warn", vec![s("ICC_Profile chunk count is zero")])?;
                        }
                    }
                    if !icc_chunk_count.is_undef() {
                        // save this chunk
                        let ci = chunk_num.to_i64().max(0) as usize;
                        if icc_chunk.len() <= ci {
                            icc_chunk.resize(ci + 1, Value::Undef);
                        }
                        let piece = substr(&deref(&seg_data_pt)?, &n(14), None);
                        if !icc_chunk[ci].is_undef() {
                            meth(it, &self_, "Warn", vec![s(&format!("Duplicate ICC_Profile chunk number {}", chunk_num.to_text()))])?;
                            icc_chunk[ci] = cat(&[&icc_chunk[ci], &piece]);
                        } else {
                            icc_chunk[ci] = piece;
                        }
                        // continue accumulating chunks unless we have all of them
                        icc_chunk_count = pl::add(&icc_chunk_count, &n(1));
                        if !pl::ge(&icc_chunk_count, &icc_chunks_total) {
                            continue 'marker;
                        }
                        icc_chunk_count = Value::Undef; // prevent reprocessing
                        done_dir.set("ICC_Profile", n(1));
                        // combine the ICC_Profile chunks
                        let mut icc_profile = Vec::new();
                        for c in &icc_chunk {
                            if !c.is_undef() {
                                icc_profile.extend_from_slice(&c.to_bytes());
                            }
                        }
                        icc_chunk.clear(); // free memory
                        seg_data_pt = pl::sref(Value::bytes(icc_profile));
                        length = pl::length(&deref(&seg_data_pt)?);
                        let tag_table_ptr = get_tag_table(it, "Image::ExifTool::ICC_Profile::Main")?;
                        let dir_info = hash_from(&[s("DataPt"), seg_data_pt.clone(), s("DataPos"), pl::add(&seg_pos, &n(14)), s("DataLen"), length.clone(), s("DirStart"), n(0), s("DirLen"), length.clone(), s("Parent"), marker_name.clone()]);
                        let new_data = meth(it, &self_, "WriteDirectory", vec![dir_info, tag_table_ptr])?;
                        if !new_data.is_undef() {
                            set_deref(&seg_data_pt, Value::Undef)?; // free the old buffer
                            seg_data_pt = pl::sref(new_data);
                        }
                        if pl::length(&deref(&seg_data_pt)?).to_i64() == 0 {
                            del = true;
                            break 'edit;
                        }
                        // write as ICC multi-segment
                        if !c1(it, write_multi_segment, vec![st.outfile.clone(), n(marker), Value::bytes(b"ICC_PROFILE\0"), seg_data_pt.clone(), s("ICC")])?.truthy() {
                            st.err = true;
                        }
                        set_deref(&seg_data_pt, Value::Undef)?;
                        continue 'marker;
                    } else if !chunk_num.is_undef() {
                        meth(it, &self_, "Warn", vec![s("Invalid or extraneous ICC_Profile chunk(s)")])?;
                        // fall through to preserve this extra profile...
                    }
                } else if re(r"^FPXR\0", "").is_match(&sd) {
                    seg_type = s("FPXR");
                    if del_group.get("FlashPix").truthy() {
                        del = true;
                    }
                } else if re(r"^MPF\0", "").is_match(&sd) {
                    seg_type = s("MPF");
                    if del_group.get("MPF").truthy() {
                        del = true;
                    }
                }
            } else if marker == 0xe3 {
                // APP3 (Kodak Meta)
                if re(r"^(Meta|META|Exif)\0\0", "").is_match(&sd) {
                    seg_type = s("Kodak Meta");
                    if del_group.get("Meta").truthy() {
                        del = true;
                        break 'edit;
                    }
                    if done_dir.get("Meta").truthy() {
                        meth(it, &self_, "Warn", vec![s("Multiple APP3 Meta segments")])?;
                    }
                    done_dir.set("Meta", n(1));
                    if !edit_dirs.get("Meta").truthy() {
                        break 'edit;
                    }
                    // rewrite Meta IFD as if this were a TIFF file in memory
                    let dir_info = hash_from(&[
                        s("DataPt"),
                        seg_data_pt.clone(),
                        s("DataPos"),
                        n(-6), // (remember: relative to Base!)
                        s("DirStart"),
                        n(6),
                        s("Base"),
                        pl::add(&seg_pos, &n(6)),
                        s("Parent"),
                        marker_name.clone(),
                        s("DirName"),
                        s("Meta"),
                    ]);
                    // write new data to memory
                    let tag_table_ptr = get_tag_table(it, "Image::ExifTool::Kodak::Meta")?;
                    let buff = meth(it, &self_, "WriteDirectory", vec![dir_info, tag_table_ptr, write_tiff_ref.clone()])?;
                    if !buff.is_undef() {
                        // update segment with new data
                        let head = substr(&deref(&seg_data_pt)?, &n(0), Some(&n(6)));
                        set_deref(&seg_data_pt, cat(&[&head, &buff]))?;
                    } else if !meth(it, &self_, "Options", vec![s("IgnoreMinorErrors")])?.truthy() {
                        break 'marker;
                    }
                    // delete segment if IFD contains no entries
                    if !pl::gt(&pl::length(&deref(&seg_data_pt)?), &n(6)) {
                        del = true;
                    }
                }
            } else if marker == 0xe5 {
                // APP5 (Ricoh RMETA)
                if re(r"^RMETA\0", "").is_match(&sd) {
                    seg_type = s("Ricoh RMETA");
                    if del_group.get("RMETA").truthy() {
                        del = true;
                    }
                }
            } else if marker == 0xe8 || marker == 0xe9 {
                // APP8/9 (SEAL)
                if re(r"^SEAL\0", "").is_match(&sd) {
                    seg_type = s("SEAL");
                    if del_group.get("SEAL").truthy() {
                        del = true;
                    }
                }
            } else if marker == 0xea {
                // APP10 (AROT)
                if re(r"^AROT\0\0", "").is_match(&sd) {
                    seg_type = s("AROT");
                    if del_group.get("AROT").truthy() {
                        del = true;
                    }
                }
            } else if marker == 0xeb {
                // APP11 (JUMBF)
                if re(r"^JP", "").is_match(&sd) {
                    seg_type = s("JUMBF");
                    if del_group.get("JUMBF").truthy() {
                        del = true;
                    }
                }
            } else if marker == 0xec {
                // APP12 (Ducky)
                if re(r"^Ducky", "").is_match(&sd) {
                    seg_type = s("Ducky");
                    if del_group.get("Ducky").truthy() {
                        del = true;
                        break 'edit;
                    }
                    if done_dir.get("Ducky").truthy() {
                        meth(it, &self_, "Warn", vec![s("Multiple APP12 Ducky segments")])?;
                    }
                    done_dir.set("Ducky", n(1));
                    if !edit_dirs.get("Ducky").truthy() {
                        break 'edit;
                    }
                    let tag_table_ptr = get_tag_table(it, "Image::ExifTool::APP12::Ducky")?;
                    let dir_info = hash_from(&[s("DataPt"), seg_data_pt.clone(), s("DataPos"), seg_pos.clone(), s("DataLen"), length.clone(), s("DirStart"), n(5), s("DirLen"), pl::sub(&length, &n(5)), s("Parent"), marker_name.clone()]);
                    let mut new_data = meth(it, &self_, "WriteDirectory", vec![dir_info, tag_table_ptr])?;
                    if !new_data.is_undef() {
                        set_deref(&seg_data_pt, Value::Undef)?; // free the old buffer
                        // add header to new segment unless empty
                        if pl::length(&new_data).to_i64() > 0 {
                            new_data = cat(&[&s("Ducky"), &new_data]);
                        }
                        seg_data_pt = pl::sref(new_data);
                    }
                    if pl::length(&deref(&seg_data_pt)?).to_i64() == 0 {
                        del = true;
                    }
                }
            } else if marker == 0xed {
                // APP13 (Photoshop)
                if re_hdr(&ps_app13hdr)?.is_match(&sd) {
                    seg_type = s("Photoshop");
                    // add this data to the combined data if it exists
                    if !combined_seg_data.borrow().is_undef() {
                        let c = combined_seg_data.borrow().clone();
                        let more = substr(&deref(&seg_data_pt)?, &pl::length(&ps_app13hdr), None);
                        *combined_seg_data.borrow_mut() = cat(&[&c, &more]);
                        seg_data_pt = combined();
                        length = pl::length(&combined_seg_data.borrow()); // update length
                    }
                    // peek ahead to see if the next segment is photoshop data too
                    if dir_order.first().is_some_and(|d| pl::str_eq(d, "Photoshop")) {
                        // initialize combined data if necessary
                        if combined_seg_data.borrow().is_undef() {
                            let d = deref(&seg_data_pt)?;
                            *combined_seg_data.borrow_mut() = d;
                        }
                        continue 'marker; // get the next segment to combine
                    }
                    if done_dir.get("Photoshop").truthy() {
                        meth(it, &self_, "Warn", vec![s("Multiple Photoshop records")])?;
                        // only rewrite the first Photoshop segment when deleting this group
                        // (to remove multiples when deleting and adding back in one step)
                        if del_group.get("Photoshop").truthy() {
                            del = true;
                            break 'edit;
                        }
                    }
                    done_dir.set("Photoshop", n(1));
                    // process APP13 Photoshop record
                    let tag_table_ptr = get_tag_table(it, "Image::ExifTool::Photoshop::Main")?;
                    let dir_info = hash_from(&[s("DataPt"), seg_data_pt.clone(), s("DataPos"), seg_pos.clone(), s("DataLen"), length.clone(), s("DirStart"), n(14), s("DirLen"), pl::sub(&length, &n(14)), s("Parent"), marker_name.clone()]);
                    let new_data = meth(it, &self_, "WriteDirectory", vec![dir_info, tag_table_ptr])?;
                    if !new_data.is_undef() {
                        set_deref(&seg_data_pt, Value::Undef)?; // free the old buffer
                        seg_data_pt = pl::sref(new_data);
                    }
                    if pl::length(&deref(&seg_data_pt)?).to_i64() == 0 {
                        del = true;
                        break 'edit;
                    }
                    // write as multi-segment
                    if !c1(it, write_multi_segment, vec![st.outfile.clone(), n(marker), ps_app13hdr.clone(), seg_data_pt.clone()])?.truthy() {
                        st.err = true;
                    }
                    *combined_seg_data.borrow_mut() = Value::Undef;
                    set_deref(&seg_data_pt, Value::Undef)?;
                    continue 'marker;
                }
            } else if marker == 0xee {
                // APP14 (Adobe)
                if re(r"^Adobe", "").is_match(&sd) {
                    seg_type = s("Adobe");
                    // delete it and replace it later if editing
                    if del_group.get("Adobe").truthy() || edit_dirs.get("Adobe").truthy() {
                        del = true;
                        // undef $doneDir{Adobe}  (so we can add it back again above)
                        done_dir.set("Adobe", Value::Undef);
                    }
                }
            } else if marker == 0xfe {
                // COM (JPEG comment)
                let mut new_comment = Value::Undef;
                if !done_dir.get("COM").truthy() {
                    done_dir.set("COM", n(1));
                    if !(del_group.get("File").truthy() && !pl::num_eq(&del_group.get("File"), &n(2))) {
                        let tag_info = extra.get("Comment");
                        let nv_hash = meth(it, &self_, "GetNewValueHash", vec![tag_info])?;
                        let mut val = seg_data.borrow().clone();
                        re(r"\0+$", "").subst(&mut val, false, |_| Vec::new()); // allow for stupid software that adds NULL terminator
                        if meth(it, &self_, "IsOverwriting", vec![nv_hash.clone(), val])?.truthy() || del_group.get("File").truthy() {
                            new_comment = meth(it, &self_, "GetNewValue", vec![nv_hash])?;
                        } else {
                            edit_dirs.delete("COM"); // we aren't editing COM after all
                            break 'edit;
                        }
                    }
                }
                let d = deref(&seg_data_pt)?;
                verbose_value(it, vec![self_.clone(), s("- Comment"), d], Want::Void)?;
                if !new_comment.is_undef() {
                    // write out the comments
                    verbose_value(it, vec![self_.clone(), s("+ Comment"), new_comment.clone()], Want::Void)?;
                    if !c1(it, write_multi_segment, vec![st.outfile.clone(), n(0xfe), s(""), pl::sref(new_comment)])?.truthy() {
                        st.err = true;
                    }
                } else if verbose.truthy() {
                    vout!(s("  Deleting COM segment\n"));
                }
                changed(&self_); // increment the changed flag
                seg_data_pt = Value::Undef; // don't write existing comment
            }
            break 'edit; // didn't want to loop anyway
        }

        // delete necessary segments (including unknown segments if deleting all)
        if del || (del_group.get("*").truthy() && !seg_type.truthy() && (0xe0..=0xef).contains(&marker)) {
            if !seg_type.truthy() {
                seg_type = s("unknown");
            }
            if verbose.truthy() {
                vout!(cat(&[&s("  Deleting "), &marker_name, &s(" "), &seg_type, &s(" segment\n")]));
            }
            changed(&self_);
            continue 'marker;
        }
        // write out this segment if $segDataPt is still defined
        if !seg_data_pt.is_undef() && !deref(&seg_data_pt)?.is_undef() {
            // write the data for this record (the data could have been
            // modified, so recalculate the length word)
            let d = deref(&seg_data_pt)?;
            let size = pl::length(&d);
            if pl::gt(&size, &max_segment_len) {
                if !seg_type.truthy() {
                    seg_type = s("Unknown");
                }
                meth(it, &self_, "Error", vec![s(&format!("{} {} segment too large! ({} bytes)", seg_type.to_text(), marker_name.to_text(), size.to_text()))])?;
                st.err = true;
            } else {
                st.s_ = pl::pack("n", &[pl::add(&pl::length(&d), &n(2))])?;
                wr!(hdr.clone(), st.s_.clone(), d);
            }
            set_deref(&seg_data_pt, Value::Undef)?; // free the buffer
        }
    }
    // make sure the ICC_Profile was complete
    if !icc_chunk_count.is_undef() {
        meth(it, &self_, "Error", vec![s("Incomplete ICC_Profile record"), n(1)])?;
    }
    if path.len() > pn {
        if let Value::ArrayRef(p) = &path {
            p.borrow_mut().pop();
        }
    }
    // if oldOutfile is still set, there was an error copying the JPEG
    if old_outfile.truthy() {
        return ret(n(0));
    }
    if st.rtn_val != 0 {
        // add any new trailers we are creating
        let trail_pt = c1(it, add_new_trailers, vec![self_.clone()])?;
        if trail_pt.truthy() {
            let d = deref(&trail_pt)?;
            wr!(d);
        }
    }
    // set return value to -1 if we only had a write error
    if st.rtn_val != 0 && st.err {
        st.rtn_val = -1;
    }
    if creating_exv && st.rtn_val > 0 && !self_.get("CHANGED").truthy() {
        meth(it, &self_, "Error", vec![s("Nothing written")])?;
        st.rtn_val = -1;
    }
    ret(n(st.rtn_val))
}

/// `ReadValue($dataPt, $entry, $format, $count, $size)` in scalar context
fn read_value(it: &mut Interp, args: Vec<Value>) -> ER<Value> {
    c1(it, part2::read_value, args)
}

/// Write to binary data block
/// Inputs: 0) ExifTool object ref, 1) source dirInfo ref, 2) tag table ref
/// Returns: Binary data block or undefined on error
pub fn write_binary_data(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !self_.truthy() {
        return ret(n(1)); // allow dummy access to autoload this package
    }

    // get default format ('int8u' unless specified)
    let data_pt = dir_info.get("DataPt");
    if !data_pt.truthy() {
        return pl::ret_undef();
    }
    let data_len = pl::length(&deref(&data_pt)?);
    let default_format = or(tag_table_ptr.get("FORMAT"), s("int8u"));
    let increment = c1(it, part2::format_size, vec![default_format.clone()])?;
    if !increment.truthy() {
        pl::warn(it, &format!("Unknown format {}\n", default_format.to_text()))?;
        return pl::ret_undef();
    }
    // extract data members first if necessary
    let var_offsets = Value::new_array(Vec::new());
    if tag_table_ptr.get("DATAMEMBER").truthy() {
        dir_info.set("DataMember", tag_table_ptr.get("DATAMEMBER"));
        dir_info.set("VarFormatData", var_offsets.clone());
        meth(it, &self_, "ProcessBinaryData", vec![dir_info.clone(), tag_table_ptr.clone()])?;
        dir_info.delete("DataMember");
        dir_info.delete("VarFormatData");
    }
    let var_offsets = var_offsets.items();
    let dir_start = or(dir_info.get("DirStart"), n(0));
    let mut dir_len = dir_info.get("DirLen");
    if dir_len.is_undef() || pl::gt(&dir_len, &pl::sub(&data_len, &dir_start)) {
        dir_len = pl::sub(&data_len, &dir_start);
    }
    let new_data0 = substr(&deref(&data_pt)?, &dir_start, Some(&dir_len));
    if !new_data0.truthy() {
        return pl::ret_undef();
    }
    let dir_name = dir_info.get("DirName");
    let mut var_size = n(0);
    let mut var_info: Vec<Value> = var_offsets.clone();
    // $dataPt = \$newData;
    let data_pt = pl::sref(new_data0);
    let mut list = pl::method_list(it, &self_, "GetNewTagInfoList", vec![tag_table_ptr.clone()])?;
    list.sort_by(|x, y| ops::num_cmp(&x.get("TagID"), &y.get("TagID")).unwrap_or(std::cmp::Ordering::Equal));
    for tag_info in list {
        let tag_id = tag_info.get("TagID");
        // evaluate conditional tags now if necessary
        if tag_table_ptr.get(tag_id.to_bytes()).ref_type() == "ARRAY" || tag_info.get("Condition").truthy() {
            let write_info = meth(it, &self_, "GetTagInfo", vec![tag_table_ptr.clone(), tag_id.clone()])?;
            if !(write_info.truthy() && write_info.to_bytes() == tag_info.to_bytes()) {
                continue;
            }
        }
        // add offsets for variable-sized tags if necessary
        while !var_info.is_empty() && pl::lt(&var_info[0].at(0), &tag_id) {
            var_size = var_info[0].at(1); // get accumulated variable size
            var_info.remove(0);
        }
        let mut count = n(1);
        let mut format = tag_info.get("Format");
        let entry = pl::add(&pl::mul(&pl::int(&tag_id), &increment), &var_size); // relative offset of this entry
        if format.truthy() {
            if let Some(m) = re(r"(.*)\[(.*)\]", "").m(&format) {
                format = cap(&m, 1);
                count = cap(&m, 2);
                let size = dir_len.clone(); // used in eval
                // evaluate count to allow count to be based on previous values
                //### eval Format size ($size, $self) - NOTE: %val not supported for writing
                let scope = crate::runtime::file_scope(it, ET);
                let (v, _) = pl::eval_site_in(it, &count, ET, Some(scope), &[("size", size), ("self", self_.clone())], Want::Scalar);
                count = v;
                let at = it.get_scalar("main::@");
                if at.truthy() {
                    pl::warn(it, &at.to_text())?;
                    continue;
                }
            } else if pl::str_eq(&format, "string") {
                // string with no specified count runs to end of block
                count = if pl::gt(&dir_len, &entry) { pl::sub(&dir_len, &entry) } else { n(0) };
            }
        } else {
            format = default_format.clone();
        }
        // read/write using variable format if changed in Hook
        if !var_info.is_empty() && pl::num_eq(&var_info[0].at(0), &tag_id) {
            format = var_info[0].at(2);
        }
        let val = read_value(it, vec![data_pt.clone(), entry.clone(), format.clone(), count.clone(), pl::sub(&dir_len, &entry)])?;
        if val.is_undef() {
            continue;
        }
        let nv_hash = meth(it, &self_, "GetNewValueHash", vec![tag_info.clone(), self_.get("CUR_WRITE_GROUP")])?;
        if !pl::gt(&meth(it, &self_, "IsOverwriting", vec![nv_hash.clone(), val.clone()])?, &n(0)) {
            continue;
        }
        let mut new_val = meth(it, &self_, "GetNewValue", vec![nv_hash])?;
        if new_val.is_undef() {
            continue; // can't delete from a binary table
        }
        // update DataMember with new value if necessary
        if tag_info.get("DataMember").truthy() {
            self_.set(tag_info.get("DataMember").to_bytes(), new_val.clone());
        }
        // only write masked bits if specified
        let mask = tag_info.get("Mask");
        if mask.truthy() {
            new_val = ops::bit_or(&ops::bit_and(&ops::shift_left(&new_val, &tag_info.get("BitShift")), &mask), &ops::bit_and(&val, &ops::bit_not(&mask)));
        }
        // set the size
        if tag_info.get("DataTag").truthy() && !tag_info.get("IsOffset").truthy() {
            if !pl::num_eq(&new_val, &n(0xfeedfeed)) {
                pl::warn(it, "Internal error")?;
            }
            let data = meth(it, &self_, "GetNewValue", vec![tag_info.get("DataTag")])?;
            if !data.is_undef() {
                new_val = pl::length(&data);
            }
            let format = or(or(tag_info.get("Format"), tag_table_ptr.get("FORMAT")), s("int32u"));
            if re(r"^int16", "").is_match(&format) && pl::gt(&new_val, &n(0xffff)) {
                meth(it, &self_, "Error", vec![s(&format!("{} is too large (64 KiB max. for this file)", tag_info.get("DataTag").to_text()))])?;
            }
        }
        let mut args = [new_val, format, count, data_pt.clone(), entry];
        let rtn_val = call_mut(it, "Image::ExifTool::WriteValue", &mut args, Want::Scalar)?.pop().unwrap_or_default();
        let new_val = args[0].clone();
        if !rtn_val.is_undef() {
            let name = tag_info.get("Name");
            verbose_value(it, vec![self_.clone(), cat(&[&s("- "), &dir_name, &s(":"), &name]), val], Want::Void)?;
            verbose_value(it, vec![self_.clone(), cat(&[&s("+ "), &dir_name, &s(":"), &name]), new_val], Want::Void)?;
            bump(&self_, "CHANGED", 1);
        } else {
            meth(it, &self_, "Warn", vec![s(&format!("Error packing {} value", tag_info.get("Name").to_text()))])?;
        }
    }
    // add necessary fixups for any offsets
    if tag_table_ptr.get("IS_OFFSET").truthy() && dir_info.get("Fixup").truthy() {
        var_size = n(0);
        var_info = var_offsets.clone();
        let fixup = dir_info.get("Fixup");
        for tag_id in tag_table_ptr.get("IS_OFFSET").items() {
            let tag_info = meth(it, &self_, "GetTagInfo", vec![tag_table_ptr.clone(), tag_id.clone()])?;
            if !tag_info.truthy() {
                continue;
            }
            while !var_info.is_empty() && pl::lt(&var_info[0].at(0), &tag_id) {
                var_size = var_info[0].at(1);
                var_info.remove(0);
            }
            let mut entry = pl::add(&pl::mul(&tag_id, &increment), &var_size); // (no offset to dirStart for new dir data)
            if !pl::le(&entry, &pl::sub(&dir_len, &n(4))) {
                continue;
            }
            // (Ricoh has 16-bit preview image offsets, so can't just assume int32u)
            let format = or(or(tag_info.get("Format"), tag_table_ptr.get("FORMAT")), s("int32u"));
            let offset = read_value(it, vec![data_pt.clone(), entry.clone(), format.clone(), n(1), pl::sub(&dir_len, &entry)])?;
            // ignore if offset is zero (eg. Ricoh DNG uses this to indicate no preview)
            if !offset.truthy() {
                continue;
            }
            meth(it, &fixup, "AddFixup", vec![entry.clone(), tag_info.get("DataTag"), format.clone()])?;
            if !(tag_info.get("DataTag").truthy() && !tag_info.get("OffsetPair").is_undef()) {
                continue;
            }
            // NOTE: here we assume there are no var-sized tags between the
            // OffsetPair tags.  If this ever becomes possible we must recalculate
            // $varSize for the OffsetPair tag here!
            entry = pl::add(&pl::mul(&tag_info.get("OffsetPair"), &increment), &var_size);
            let size = read_value(it, vec![data_pt.clone(), entry.clone(), format.clone(), n(1), pl::sub(&dir_len, &entry)])?;
            if size.is_undef() {
                continue;
            }
            if pl::str_eq(&tag_info.get("DataTag"), "HiddenData") {
                let fx = meth(it, &s("Image::ExifTool::Fixup"), "new", vec![])?;
                self_.set("HiddenData", hash_from(&[s("Offset"), offset, s("Size"), size, s("Fixup"), fx, s("Base"), dir_info.get("Base")]));
                continue;
            }
            // handle the preview image now if this is a JPEG file
            if !(pl::str_eq(&tag_info.get("DataTag"), "PreviewImage") && pl::str_eq(&self_.get("FILE_TYPE"), "JPEG")) {
                continue;
            }
            let mut preview_info = self_.get("PREVIEW_INFO");
            if !preview_info.truthy() {
                let fx = meth(it, &s("Image::ExifTool::Fixup"), "new", vec![])?;
                preview_info = hash_from(&[s("Fixup"), fx]);
                self_.set("PREVIEW_INFO", preview_info.clone());
            }
            // set flag indicating we are using short pointers
            if !pl::str_eq(&format, "int32u") {
                preview_info.set("IsShort", n(1));
            }
            if tag_info.get("IsOffset").truthy() && pl::str_eq(&tag_info.get("IsOffset"), "3") {
                preview_info.set("Absolute", n(1));
            }
            // get the value of the Composite::PreviewImage tag
            let cti = pl::call(it, "Image::ExifTool::GetCompositeTagInfo", vec![s("PreviewImage")])?;
            let d = meth(it, &self_, "GetNewValue", vec![cti])?;
            preview_info.set("Data", d);
            if preview_info.get("Data").is_undef() {
                if pl::ge(&offset, &n(0)) && pl::le(&pl::add(&offset, &size), &dir_info.get("DataLen")) {
                    let src = deref(&dir_info.get("DataPt"))?;
                    preview_info.set("Data", substr(&src, &offset, Some(&size)));
                } else {
                    preview_info.set("Data", s("LOAD_PREVIEW")); // flag to load preview later
                }
            }
        }
    }
    // write any necessary SubDirectories
    if tag_table_ptr.get("IS_SUBDIR").truthy() {
        var_size = n(0);
        var_info = var_offsets.clone();
        for tag_id in tag_table_ptr.get("IS_SUBDIR").items() {
            let mut tag_info = meth(it, &self_, "GetTagInfo", vec![tag_table_ptr.clone(), tag_id.clone()])?;
            if tag_info.is_undef() {
                continue;
            }
            while !var_info.is_empty() && pl::lt(&var_info[0].at(0), &tag_id) {
                var_size = var_info[0].at(1);
                var_info.remove(0);
            }
            let entry = pl::add(&pl::mul(&pl::int(&tag_id), &increment), &var_size);
            if pl::ge(&entry, &dir_len) {
                break;
            }
            // get value for Condition if necessary
            if !tag_info.truthy() {
                let mut more = pl::sub(&dir_len, &entry);
                if pl::gt(&more, &n(128)) {
                    more = n(128);
                }
                let v = substr(&deref(&data_pt)?, &entry, Some(&more));
                tag_info = meth(it, &self_, "GetTagInfo", vec![tag_table_ptr.clone(), tag_id.clone(), pl::sref(v)])?;
                if !tag_info.truthy() {
                    continue;
                }
            }
            let subdir = tag_info.get("SubDirectory");
            if !subdir.truthy() {
                continue;
            }
            let mut start = subdir.get("Start");
            let mut len = Value::Undef;
            if !start.truthy() {
                start = entry.clone();
                len = pl::sub(&dir_len, &start);
            } else if re(r"\$", "").is_match(&start) {
                let mut count = n(1);
                let mut format = or(tag_info.get("Format"), default_format.clone());
                if let Some(m) = re(r"(.*)\[(.*)\]", "").m(&format) {
                    format = cap(&m, 1);
                    count = cap(&m, 2);
                }
                let val = read_value(it, vec![data_pt.clone(), entry.clone(), format.clone(), count.clone(), pl::sub(&dir_len, &entry)])?;
                // ignore directories with a zero offset (ie. missing Nikon ShotInfo entries)
                if !val.truthy() {
                    continue;
                }
                let dir_start = n(0);
                //### eval Start ($val, $dirStart)
                let scope = crate::runtime::file_scope(it, ET);
                let (v, _) = pl::eval_site_in(
                    it,
                    &start,
                    ET,
                    Some(scope),
                    &[("val", val), ("dirStart", dir_start.clone()), ("self", self_.clone()), ("dataPt", data_pt.clone()), ("dirLen", dir_len.clone()), ("entry", entry.clone()), ("tagInfo", tag_info.clone()), ("format", format), ("count", count)],
                    Want::Scalar,
                );
                start = v;
                if pl::lt(&start, &dir_start) || pl::gt(&start, &data_len) {
                    continue;
                }
                len = subdir.get("DirLen");
                if !(len.truthy() && pl::le(&len, &pl::sub(&data_len, &start))) {
                    len = pl::sub(&data_len, &start);
                }
            }
            let subdir_info = hash_from(&[s("DataPt"), data_pt.clone(), s("DirStart"), start.clone(), s("DirLen"), len.clone(), s("TagInfo"), tag_info.clone()]);
            let table = pl::call(it, "Image::ExifTool::GetTagTable", vec![subdir.get("TagTable")])?;
            let dat = meth(it, &self_, "WriteDirectory", vec![subdir_info, table])?;
            if !dat.is_undef() && pl::length(&dat).to_i64() > 0 {
                substr_assign(&data_pt, &start, &len, &dat)?;
            }
        }
    }
    ret(deref(&data_pt)?)
}

/// Write TIFF as a directory
/// Inputs: 0) ExifTool object ref, 1) dirInfo ref, 2) tag table ref
/// Returns: New directory data or undefined on error
pub fn write_tiff(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !self_.truthy() {
        return ret(n(1)); // allow dummy access
    }
    let buff = pl::sref(s(""));
    dir_info.set("OutFile", buff.clone());
    if pl::gt(&meth(it, &self_, "ProcessTIFF", vec![dir_info, tag_table_ptr])?, &n(0)) {
        return ret(buff.deref());
    }
    pl::ret_undef()
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::HexDump", hex_dump);
    it.define_native("Image::ExifTool::VerboseInfo", verbose_info);
    it.define_native("Image::ExifTool::DumpTrailer", dump_trailer);
    it.define_native("Image::ExifTool::DumpUnknownTrailer", dump_unknown_trailer);
    it.define_native("Image::ExifTool::LastInList", last_in_list);
    it.define_native("Image::ExifTool::VerboseValue", verbose_value);
    it.define_native("Image::ExifTool::PackUTF8", pack_utf8);
    it.define_native("Image::ExifTool::UnpackUTF8", unpack_utf8);
    it.define_native("Image::ExifTool::NewGUID", new_guid);
    it.define_native("Image::ExifTool::NewUUID", new_uuid);
    it.define_native("Image::ExifTool::MakeTiffHeader", make_tiff_header);
    it.define_native("Image::ExifTool::TimeNow", time_now_sub);
    it.define_native("Image::ExifTool::InverseDateTime", inverse_date_time);
    it.define_native("Image::ExifTool::SetPreferredByteOrder", set_preferred_byte_order);
    it.define_native("Image::ExifTool::AssembleRational", assemble_rational);
    it.define_native("Image::ExifTool::Rationalize", rationalize);
    it.define_native("Image::ExifTool::Set16s", set16s);
    it.define_native("Image::ExifTool::Set32s", set32s);
    it.define_native("Image::ExifTool::Set64u", set64u);
    it.define_native("Image::ExifTool::Set64s", set64s);
    it.define_native("Image::ExifTool::SetRational64u", set_rational64u);
    it.define_native("Image::ExifTool::SetRational64s", set_rational64s);
    it.define_native("Image::ExifTool::SetRational32u", set_rational32u);
    it.define_native("Image::ExifTool::SetRational32s", set_rational32s);
    it.define_native("Image::ExifTool::SetFixed16u", set_fixed16u);
    it.define_native("Image::ExifTool::SetFixed16s", set_fixed16s);
    it.define_native("Image::ExifTool::SetFixed32u", set_fixed32u);
    it.define_native("Image::ExifTool::SetFixed32s", set_fixed32s);
    it.define_native("Image::ExifTool::SetFloat", set_float);
    it.define_native("Image::ExifTool::SetDouble", set_double);
    it.define_native("Image::ExifTool::WriteValue", write_value);
    it.define_native("Image::ExifTool::EncodeBits", encode_bits);
    it.define_native("Image::ExifTool::Tell", tell);
    it.define_native("Image::ExifTool::Write", write);
    it.define_native("Image::ExifTool::WriteTrailerBuffer", write_trailer_buffer);
    it.define_native("Image::ExifTool::AddNewTrailers", add_new_trailers);
    it.define_native("Image::ExifTool::WriteMultiSegment", write_multi_segment);
    it.define_native("Image::ExifTool::WriteMultiXMP", write_multi_xmp);
    it.define_native("Image::ExifTool::CheckImage", check_image);
    it.define_native("Image::ExifTool::CheckValue", check_value);
    it.define_native("Image::ExifTool::CheckBinaryData", check_binary_data);
    it.define_native("Image::ExifTool::Rename", rename);
    it.define_native("Image::ExifTool::Unlink", unlink);
    it.define_native("Image::ExifTool::SetFileTime", set_file_time);
    it.define_native("Image::ExifTool::ImageDataHash", image_data_hash);
    it.define_native("Image::ExifTool::CopyBlock", copy_block);
    it.define_native("Image::ExifTool::CopyImageData", copy_image_data);
    it.define_native("Image::ExifTool::WriteJPEG", write_jpeg);
    it.define_native("Image::ExifTool::WriteBinaryData", write_binary_data);
    it.define_native("Image::ExifTool::WriteTIFF", write_tiff);
}
