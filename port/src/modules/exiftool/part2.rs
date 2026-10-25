//! Image::ExifTool (ExifTool.pm), ported sub by sub (see docs/port/PORTING.md).
//! This part: the subs from `Purge` to `JpegMarkerName` (lines 4388-7254).

use std::rc::Rc;

use crate::interp::eval::{die, substr_range, Binding, Cell, Flow, ER};
use crate::interp::{Interp, Want};
use crate::perl::{ops, pack, Value};
use crate::pl::{self, arg, cap, re, ret};
use crate::runtime;

const ET: &str = "Image::ExifTool";

// ---- local helpers (Perl operations the shared helpers do not cover) ----

fn n(i: i64) -> Value {
    Value::Int(i)
}

/// `$Image::ExifTool::name` (a `use vars` package variable).
fn pkg_scalar(it: &mut Interp, name: &str) -> Cell {
    match it.global(b'$', &format!("{ET}::{name}")) {
        Binding::Scalar(c) => c,
        _ => unreachable!(),
    }
}

/// `%Image::ExifTool::name` as a hash reference.
fn pkg_hash(it: &mut Interp, name: &str) -> Value {
    match it.global(b'%', name) {
        Binding::Hash(h) => Value::HashRef(h),
        _ => unreachable!(),
    }
}

/// A file lexical of ExifTool.pm (`my %name`).
fn my_hash(it: &mut Interp, name: &str) -> Value {
    runtime::lex_hash(it, ET, name)
}

fn my_scalar(it: &mut Interp, name: &str) -> Cell {
    runtime::lex_scalar(it, ET, name)
}

fn my_array(it: &mut Interp, name: &str) -> Value {
    runtime::lex_array(it, ET, name)
}

/// `substr(STR, OFF, LEN)` as an rvalue: undef when outside the string.
fn substr(s: &Value, off: i64, len: Option<i64>) -> Value {
    let b = s.to_bytes();
    match substr_range(b.len(), off, len) {
        Some((a, e)) => Value::bytes(&b[a..e]),
        None => Value::Undef,
    }
}

/// `$$ref` of a SCALAR reference, dying as perl does under strict refs.
fn deref_scalar(r: &Value) -> ER<Value> {
    match r {
        Value::ScalarRef(_) | Value::LvRef(_) => Ok(r.deref()),
        Value::Undef => die("Can't use an undefined value as a SCALAR reference"),
        _ => die(format!("Can't use string (\"{}\") as a SCALAR ref while \"strict refs\" in use", r.to_text())),
    }
}

/// `$$ref = VALUE` on a SCALAR reference.
fn assign_deref(r: &Value, v: Value) -> ER<()> {
    match r {
        Value::ScalarRef(c) => {
            *c.borrow_mut() = v;
            Ok(())
        }
        Value::LvRef(l) => l.set(v),
        Value::Undef => die("Can't use an undefined value as a SCALAR reference"),
        _ => die(format!("Can't use string (\"{}\") as a SCALAR ref while \"strict refs\" in use", r.to_text())),
    }
}

/// `substr($$ref, OFF, LEN) = VALUE`
fn substr_assign(r: &Value, off: i64, len: i64, v: &Value) -> ER<()> {
    let s = deref_scalar(r)?.to_bytes();
    let Some((a, b)) = substr_range(s.len(), off, Some(len)) else { return die("substr outside of string") };
    let mut out = s[..a].to_vec();
    out.extend_from_slice(&v.to_bytes());
    out.extend_from_slice(&s[b..]);
    assign_deref(r, Value::bytes(out))
}

/// `unpack(TEMPLATE, DATA)`, in the context of the sub returning it.
fn unpack_ctx(template: &[u8], data: &[u8], want: Want) -> ER<Vec<Value>> {
    match pack::unpack(template, data) {
        Ok(v) if want != Want::List => Ok(vec![v.into_iter().next().unwrap_or_default()]),
        Ok(v) => Ok(v),
        Err(e) => die(e),
    }
}

fn pack_v(template: &[u8], args: &[Value]) -> ER<Value> {
    match pack::pack(template, args) {
        Ok(b) => Ok(Value::bytes(b)),
        Err(e) => die(e),
    }
}

/// `lc` on a byte string.
fn lc(v: &Value) -> Value {
    Value::bytes(v.to_bytes().to_ascii_lowercase())
}

/// `ucfirst` on a byte string.
fn ucfirst(v: &Value) -> Value {
    let mut b = v.to_bytes().to_vec();
    if let Some(c) = b.first_mut() {
        *c = c.to_ascii_uppercase();
    }
    Value::bytes(b)
}

/// `$s =~ tr/SEARCH/REPLACE/FLAGS`
fn tr(s: &mut Value, search: &[u8], replace: &[u8], flags: &str) -> usize {
    let (out, count) = crate::perl::tr::tr(&s.to_bytes(), search, replace, flags);
    if !flags.contains('r') {
        *s = Value::bytes(out);
    }
    count
}

/// `join(SEP, LIST)`
fn join(sep: &[u8], list: &[Value]) -> Value {
    let parts: Vec<Vec<u8>> = list.iter().map(|v| v.to_bytes().to_vec()).collect();
    Value::bytes(parts.join(sep))
}

/// `gmtime(T)` / `localtime(T)` in list context.
fn time_list(t: &Value, local: bool) -> Vec<Value> {
    crate::interp::time::broken_down(t.to_f64().floor() as i64, local).iter().map(|&x| Value::Int(x)).collect()
}

/// `local $SIG{'__WARN__'} = \&SetWarning;` for the duration of `f`.
fn with_set_warning<T>(it: &mut Interp, f: impl FnOnce(&mut Interp) -> ER<T>) -> ER<T> {
    let sig = pkg_hash(it, "main::SIG");
    let had = sig.exists("__WARN__");
    let old = sig.get("__WARN__");
    let code = it.sub_ref("Image::ExifTool::SetWarning");
    sig.set("__WARN__", Value::Code(code));
    let r = f(it);
    if had {
        sig.set("__WARN__", old);
    } else {
        sig.delete("__WARN__");
    }
    r
}

/// `ref $v`: the class of a blessed reference, else its type.
fn ref_of(v: &Value) -> String {
    if let Value::HashRef(h) = v {
        if let Some(c) = &h.borrow().blessed {
            return c.to_string();
        }
    }
    v.ref_type().to_string()
}

/// `$^O`
fn os_name(it: &mut Interp) -> Value {
    it.get_scalar("main::^O")
}

// ---- Purge ... ----

/// Purge cached tag information (`Purge(;$)`)
pub fn purge(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let purge_flag = my_scalar(it, "purgeFlag");
    let v = arg(&a, 0);
    *purge_flag.borrow_mut() = if v.truthy() { v } else { n(0) };
    let purge_tags = my_array(it, "purgeTags");
    let flag = purge_flag.borrow().clone();
    if !purge_tags.is_empty() && pl::ge(&pl::length(&n(purge_tags.len() as i64)), &flag) {
        for t in purge_tags.items() {
            if t.get("IsProtobuf").is_undef() {
                t.get("Table").delete(t.get("TagID").to_bytes());
            }
        }
        if let Value::ArrayRef(arr) = &purge_tags {
            arr.borrow_mut().clear();
        }
    }
    pl::ret_nothing(want)
}

/// Combine information from a list of info hashes
/// Unless Duplicates is enabled, first entry found takes priority
/// Inputs: 0) ExifTool object reference, 1-N) list of info hash references
/// Returns: Combined information hash reference
pub fn combine_info(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    // (local $_: not used here)
    let mut args = a.into_iter();
    let slf = args.next().unwrap_or_default();
    let combined_info = Value::new_hash();
    let have_info = Value::new_hash();

    if slf.path(&["OPTIONS", "Duplicates"]).truthy() {
        for info in args {
            if !info.truthy() {
                break;
            }
            for tag in info.keys() {
                combined_info.set(&tag, info.get(&tag));
            }
        }
    } else {
        for info in args {
            if !info.truthy() {
                break;
            }
            for tag in info.keys() {
                let tag_name = pl::call(it, "Image::ExifTool::GetTagName", vec![Value::Str(tag.clone())])?;
                if have_info.get(tag_name.to_bytes()).truthy() {
                    continue;
                }
                have_info.set(tag_name.to_bytes(), n(1));
                combined_info.set(&tag, info.get(&tag));
            }
        }
    }
    ret(combined_info)
}

/// Get tag table name
/// Inputs: 0) ExifTool object reference, 1) tag key
/// Returns: Table name if available, otherwise ''
pub fn get_table_name(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (slf, tag) = (arg(&a, 0), arg(&a, 1));
    let tag_info = slf.get("TAG_INFO").get(tag.to_bytes());
    if !tag_info.truthy() {
        return ret(pl::s(""));
    }
    ret(tag_info.get("Table").get("SHORT_NAME"))
}

/// Get tag index number
/// Inputs: 0) ExifTool object reference, 1) tag key
/// Returns: Table index number, or undefined if this tag isn't indexed
pub fn get_tag_index(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (slf, tag) = (arg(&a, 0), arg(&a, 1));
    let tag_info = slf.get("TAG_INFO").get(tag.to_bytes());
    if !tag_info.truthy() {
        return pl::ret_undef();
    }
    ret(tag_info.get("Index"))
}

/// Find value for specified tag
/// Inputs: 0) ExifTool ref, 1) tag name, 2) tag group (family 1)
/// Returns: value or undef
pub fn find_value(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, tag, grp) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut val = Value::Undef;
    let value = et.get("VALUE");
    let mut i: i64 = 0;
    loop {
        let key = pl::concat(&tag, &if i != 0 { pl::s(&format!(" ({i})")) } else { pl::s("") });
        if value.get(key.to_bytes()).is_undef() {
            break;
        }
        let g = pl::method(it, &et, "GetGroup", vec![key.clone(), n(1)])?;
        if g.to_bytes() == grp.to_bytes() {
            val = value.get(key.to_bytes());
            break;
        }
        i += 1;
    }
    ret(val)
}

/// Get tag key for next existing tag
/// Inputs: 0) ExifTool ref, 1) tag key or case-sensitive tag name
/// Returns: Key of next existing tag, or undef if no more
/// Notes: This routine is provided for iterating through duplicate tags in the
///        ValueConv of Composite tags.
pub fn next_tag_key(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (slf, mut tag) = (arg(&a, 0), arg(&a, 1));
    let mut c1 = Value::Undef;
    let i = if re(r" \((\d+)\)$", "").subst(&mut tag, false, |c| {
        c1 = cap(c, 1);
        Vec::new()
    }) > 0
    {
        pl::add(&c1, &n(1))
    } else {
        n(1)
    };
    tag = pl::concat(&pl::concat(&pl::concat(&tag, &pl::s(" (")), &i), &pl::s(")"));
    if !slf.get("VALUE").get(tag.to_bytes()).is_undef() {
        return ret(tag);
    }
    pl::ret_undef()
}

// ---- utilities ----

/// Add cleanup routine to call before returning from Extract
/// Inputs: 0) ExifTool ref, 1) code ref to routine with ExifTool ref as an argument
pub fn add_cleanup(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (slf, sub) = (arg(&a, 0), arg(&a, 1));
    if !slf.get("Cleanup").truthy() {
        slf.set("Cleanup", Value::new_array(Vec::new()));
    }
    let list = slf.get("Cleanup");
    list.push(sub);
    ret(n(list.len() as i64))
}

/// `eval { $str =~ /$noWarn/ }`, setting `$@` as the eval does.
fn eval_match(it: &mut Interp, s: &Value, pat: &Value) -> bool {
    match pl::re_dyn(&pat.to_bytes(), "") {
        Ok(r) => {
            it.set_scalar("main::@", pl::s(""));
            r.is_match(s)
        }
        Err(Flow::Die(e)) => {
            it.set_scalar("main::@", e);
            false
        }
        Err(_) => false,
    }
}

/// Add warning tag
/// Inputs: 0) ExifTool object reference, 1) warning message
///         2) 0=normal warning, 1=minor, 2=minor with behavioural change when
///            ignored, 3=warning shouldn't be issued with Validate option,
///            bit 0x04 set causes warning count to not be incremented
/// Returns: true if warning tag was added
pub fn warn(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (slf, mut str, mut ignorable) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let no_warn = slf.path(&["OPTIONS", "NoWarning"]);
    let mut no_count = false;
    while ignorable.truthy() {
        if ops::bit_and(&ignorable, &n(0x04)).truthy() {
            no_count = true;
            ignorable = ops::bit_and(&ignorable, &n(0x03));
            if !ignorable.truthy() {
                break;
            }
        }
        let ignorable = ops::bit_and(&ignorable, &n(0x03));
        if slf.path(&["OPTIONS", "IgnoreMinorErrors"]).truthy() {
            return ret(n(0));
        }
        if pl::str_eq(&ignorable, "3") && slf.path(&["OPTIONS", "Validate"]).truthy() {
            return ret(n(0));
        }
        if !no_warn.is_undef() && eval_match(it, &str, &no_warn) {
            return ret(n(1));
        }
        str = pl::concat(&pl::s(if pl::str_eq(&ignorable, "2") { "[Minor] " } else { "[minor] " }), &str);
        break;
    }
    if !(!no_warn.is_undef() && eval_match(it, &str, &no_warn)) {
        // add each warning only once but count number of occurrences
        let was_warned = slf.get_or_new_hash("WAS_WARNED");
        if was_warned.get(str.to_bytes()).truthy() {
            if !no_count {
                let c = was_warned.get(str.to_bytes());
                was_warned.set(str.to_bytes(), pl::add(&c, &n(1)));
            }
        } else {
            pl::method(it, &slf, "FoundTag", vec![pl::s("Warning"), str.clone()])?;
            slf.get_or_new_hash("WAS_WARNED").set(str.to_bytes(), n(1));
        }
    }
    ret(n(1))
}

/// Add error tag
/// Inputs: 0) ExifTool object reference, 1) error message, 2) true if minor
/// Returns: true if error tag was added, otherwise warning was added
pub fn error(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (slf, mut str, ignorable) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if slf.get("DemoteErrors").truthy() {
        if warn(it, vec![slf.clone(), str], Want::Scalar)?.pop().unwrap_or_default().truthy() {
            let d = slf.get("DemoteErrors");
            slf.set("DemoteErrors", pl::add(&d, &n(1)));
        }
        return ret(n(1));
    } else if ignorable.truthy() {
        if slf.path(&["OPTIONS", "IgnoreMinorErrors"]).truthy() {
            warn(it, vec![slf.clone(), str], Want::Scalar)?;
            return ret(n(0));
        }
        str = pl::concat(&pl::s("[minor] "), &str);
    }
    pl::method(it, &slf, "FoundTag", vec![pl::s("Error"), str])?;
    ret(n(1))
}

/// Add tags to TagLookup (used for writing)
/// Inputs: 0) source hash of tag definitions, 1) name of destination tag table
pub fn add_tags_to_lookup(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (tag_hash, table) = (arg(&a, 0), arg(&a, 1));
    if it.sub("Image::ExifTool::TagLookup::AddTags").is_some() {
        return it.call_named("Image::ExifTool::TagLookup::AddTags", vec![tag_hash, table], want);
    }
    let plugin_tags = pkg_hash(it, "Image::ExifTool::pluginTags");
    if !plugin_tags.get(tag_hash.to_bytes()).truthy() {
        // queue these tags until TagLookup is loaded
        let list = match it.global(b'@', "Image::ExifTool::pluginTags") {
            Binding::Array(a) => a,
            _ => unreachable!(),
        };
        list.borrow_mut().push(Value::new_array(vec![tag_hash.clone(), table]));
        // set flag so we don't load same tags twice
        plugin_tags.set(tag_hash.to_bytes(), n(1));
        return ret(n(1));
    }
    ret(pl::s(""))
}

/// Expand tagInfo Flags
/// Inputs: 0) tagInfo hash ref
/// Notes: $$tagInfo{Flags} must be defined to call this routine
pub fn expand_flags(_it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let tag_info = arg(&a, 0);
    let flags = tag_info.get("Flags");
    if ref_of(&flags) == "ARRAY" {
        for f in flags.items() {
            tag_info.set(f.to_bytes(), n(1));
        }
    } else if ref_of(&flags) == "HASH" {
        for key in flags.keys() {
            tag_info.set(&key, flags.get(&key));
        }
    } else {
        tag_info.set(flags.to_bytes(), n(1));
    }
    pl::ret_nothing(want)
}

/// Set up tag table (must be done once for each tag table used)
/// Inputs: 0) Reference to tag table
/// Notes: - generates 'Name' field from key if it doesn't exist
///        - stores 'Table' pointer and 'TagID' value
///        - expands 'Flags' for quick lookup
pub fn setup_tag_table(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let tag_table_ptr = arg(&a, 0);
    let avoid = tag_table_ptr.get("AVOID");
    for tag_id in pl::call_list(it, "Image::ExifTool::TagTableKeys", vec![tag_table_ptr.clone()])? {
        let info_array = pl::call_list(it, "Image::ExifTool::GetTagInfoList", vec![tag_table_ptr.clone(), tag_id.clone()])?;
        // process conditional tagInfo arrays
        for tag_info in &info_array {
            tag_info.set("Table", tag_table_ptr.clone());
            tag_info.set("TagID", tag_id.clone());
            if !tag_info.get("Name").truthy() {
                tag_info.set("Name", make_tag_name_v(&tag_id));
            }
            if tag_info.get("Flags").truthy() {
                expand_flags(it, vec![tag_info.clone()], Want::Void)?;
            }
            if !avoid.is_undef() {
                tag_info.set("Avoid", avoid.clone());
            }
            // calculate BitShift from Mask if necessary
            if tag_info.get("Mask").truthy() && tag_info.get("BitShift").is_undef() {
                let (mask, mut bit_shift) = (tag_info.get("Mask"), n(0));
                while !ops::bit_and(&mask, &ops::shift_left(&n(1), &bit_shift)).truthy() {
                    bit_shift = pl::add(&bit_shift, &n(1));
                }
                tag_info.set("BitShift", bit_shift);
            }
        }
        if info_array.len() <= 1 {
            continue;
        }
        // add an "Index" member to each tagInfo in a list
        for (index, tag_info) in info_array.iter().enumerate() {
            tag_info.set("Index", n(index as i64));
        }
    }
    pl::ret_nothing(want)
}

/// Is this a PC system?
/// Returns: true for PC systems
/// uses lookup for O/S names which may use a backslash as a directory separator
/// (ref File::Spec of PathTools-3.2701)
pub fn is_pc(it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let os = os_name(it);
    ret(my_hash(it, "isPC").get(os.to_bytes()))
}

/// `IsFloat` on a value, translating ',' to '.' in it as the Perl does.
fn is_float_mut(v: &mut Value) -> bool {
    if re(r"^[+-]?(?=\d|\.\d)\d*(\.\d*)?([Ee]([+-]?\d+))?$", "").is_match(v) {
        return true;
    }
    // allow comma separators (for other locales)
    if !re(r"^[+-]?(?=\d|,\d)\d*(,\d*)?([Ee]([+-]?\d+))?$", "").is_match(v) {
        return false;
    }
    tr(v, b",", b".", ""); // but translate ',' to '.'
    true
}

/// Utilities to check for numerical types
/// Inputs: 0) value;  Returns: true if value is a numerical type
/// Notes: May change commas to decimals in floats for use in other locales
pub fn is_float(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut v = arg(&a, 0);
    let orig = v.clone();
    let r = is_float_mut(&mut v);
    if r && v.to_bytes() != orig.to_bytes() {
        it.set_arg(0, v)?;
    }
    ret(n(if r { 1 } else { 0 }))
}

pub fn is_int(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(Value::from(re(r"^[+-]?\d+$", "").is_match(&arg(&a, 0))))
}

pub fn is_hex(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(Value::from(re(r"^(0x)?[0-9a-f]{1,8}$", "i").is_match(&arg(&a, 0))))
}

pub fn is_rational(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(Value::from(re(r"^[-+]?\d+/\d+$", "").is_match(&arg(&a, 0))))
}

fn round_float_v(val: &Value, sig: &Value) -> Value {
    let fmt = pl::concat(&pl::concat(&pl::s("%."), sig), &pl::s("g"));
    pl::sprintf_v(&fmt, &[val.clone()])
}

/// round floating point value to specified number of significant digits
/// Inputs: 0) value, 1) number of sig digits;  Returns: rounded number
pub fn round_float(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(round_float_v(&arg(&a, 0), &arg(&a, 1)))
}

/// Convert strings to floating point numbers (or undef)
/// Inputs: 0-N) list of strings (may be undef)
/// Returns: last value converted
pub fn to_float(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    // (local $_: the loop variable is a Rust one)
    let mut last = Value::Undef;
    for (i, v) in a.iter().enumerate() {
        last = v.clone();
        if v.is_undef() {
            continue;
        }
        // (add 0 to convert "0.0" to "0" for tests)
        let nv = match re(r"((?:[+-]?)(?=\d|\.\d)\d*(?:\.\d*)?(?:[Ee](?:[+-]?\d+))?)", "").m(v) {
            Some(c) => pl::add(&cap(&c, 1), &n(0)),
            None => Value::Undef,
        };
        it.set_arg(i, nv.clone())?;
        last = nv;
    }
    ret(last)
}

// ---- reading binary data values ----

fn swap_bytes_v(it: &mut Interp, val: &Value, bytes: &Value) -> Value {
    if !pkg_scalar(it, "swapBytes").borrow().truthy() {
        return val.clone();
    }
    let mut bytes = bytes.clone();
    let mut new_val = Vec::new();
    loop {
        let old = bytes.clone();
        bytes = pl::sub(&bytes, &n(1));
        if !old.truthy() {
            break;
        }
        new_val.extend_from_slice(&substr(val, bytes.to_i64(), Some(1)).to_bytes());
    }
    Value::bytes(new_val)
}

/// Swap bytes in data if necessary
/// Inputs: 0) data, 1) number of bytes
/// Returns: swapped data
pub fn swap_bytes(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(swap_bytes_v(it, &arg(&a, 0), &arg(&a, 1)))
}

fn swap_words_v(it: &mut Interp, v: &Value) -> Value {
    if !(pkg_scalar(it, "swapWords").borrow().truthy() && v.to_bytes().len() == 8) {
        return v.clone();
    }
    pl::concat(&substr(v, 4, Some(4)), &substr(v, 0, Some(4)))
}

/// Swap words.  Inputs: 8 bytes of data, Returns: swapped data
pub fn swap_words(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(swap_words_v(it, &arg(&a, 0)))
}

/// `$unpackStd{$t}`
fn unpack_std(it: &mut Interp, t: &Value) -> Value {
    pkg_hash(it, "Image::ExifTool::unpackStd").get(t.to_bytes())
}

/// "x$pos $fmt"
fn skip_template(pos: &Value, fmt: &Value) -> Vec<u8> {
    let mut t = b"x".to_vec();
    t.extend_from_slice(&pos.to_bytes());
    t.push(b' ');
    t.extend_from_slice(&fmt.to_bytes());
    t
}

/// Unpack value, letting unpack() handle byte swapping
/// Inputs: 0) unpack template, 1) data reference, 2) offset
/// Returns: unpacked number
/// - uses value of %unpackStd to determine the unpack template
/// - can only be called for 'S' or 'L' templates since these are the only
///   templates for which you can specify the byte ordering.
pub fn do_unpack_std(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let fmt = unpack_std(it, &arg(&a, 0));
    let data = deref_scalar(&arg(&a, 1))?;
    if arg(&a, 2).truthy() {
        return unpack_ctx(&skip_template(&arg(&a, 2), &fmt), &data.to_bytes(), want);
    }
    unpack_ctx(&fmt.to_bytes(), &data.to_bytes(), want)
}

/// same, but with reversed byte order
pub fn do_unpack_rev(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let std = unpack_std(it, &arg(&a, 0));
    let fmt = my_hash(it, "unpackRev").get(std.to_bytes());
    let data = deref_scalar(&arg(&a, 1))?;
    if arg(&a, 2).truthy() {
        return unpack_ctx(&skip_template(&arg(&a, 2), &fmt), &data.to_bytes(), want);
    }
    unpack_ctx(&fmt.to_bytes(), &data.to_bytes(), want)
}

/// Pack value
/// Inputs: 0) template, 1) value, 2) data ref (or undef), 3) offset (if data ref)
/// Returns: packed value
pub fn do_pack_std(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let fmt = unpack_std(it, &arg(&a, 0));
    let val = pack_v(&fmt.to_bytes(), &[arg(&a, 1)])?;
    if arg(&a, 2).truthy() {
        substr_assign(&arg(&a, 2), arg(&a, 3).to_i64(), val.to_bytes().len() as i64, &val)?;
    }
    ret(val)
}

/// same, but with reversed byte order
pub fn do_pack_rev(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let std = unpack_std(it, &arg(&a, 0));
    let fmt = my_hash(it, "unpackRev").get(std.to_bytes());
    let val = pack_v(&fmt.to_bytes(), &[arg(&a, 1)])?;
    if arg(&a, 2).truthy() {
        substr_assign(&arg(&a, 2), arg(&a, 3).to_i64(), val.to_bytes().len() as i64, &val)?;
    }
    ret(val)
}

/// Unpack value, handling the byte swapping manually
/// Inputs: 0) # bytes, 1) unpack template, 2) data reference, 3) offset
/// Returns: unpacked number
/// - uses value of $swapBytes to determine byte ordering
pub fn do_unpack(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (mut bytes, template, data_pt, pos) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    let val;
    if pkg_scalar(it, "swapBytes").borrow().truthy() {
        let data = deref_scalar(&data_pt)?;
        let mut v = Vec::new();
        loop {
            let old = bytes.clone();
            bytes = pl::sub(&bytes, &n(1));
            if !old.truthy() {
                break;
            }
            v.extend_from_slice(&substr(&data, pl::add(&pos, &bytes).to_i64(), Some(1)).to_bytes());
        }
        val = Value::bytes(v);
    } else {
        val = substr(&deref_scalar(&data_pt)?, pos.to_i64(), Some(bytes.to_i64()));
    }
    if val.is_undef() {
        return pl::ret_undef();
    }
    unpack_ctx(&template.to_bytes(), &val.to_bytes(), want)
}

/// Unpack double value
/// Inputs: 0) unpack template, 1) data reference, 2) offset
/// Returns: unpacked number
pub fn do_unpack_dbl(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (template, data_pt, pos) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let val = substr(&deref_scalar(&data_pt)?, pos.to_i64(), Some(8));
    if val.is_undef() {
        return pl::ret_undef();
    }
    // swap bytes and 32-bit words (ARM quirk) if necessary, then unpack value
    let sb = swap_bytes_v(it, &val, &n(8));
    let sw = swap_words_v(it, &sb);
    unpack_ctx(&template.to_bytes(), &sw.to_bytes(), want)
}

fn with_args(first: Vec<Value>, a: Vec<Value>) -> Vec<Value> {
    let mut v = first;
    v.extend(a);
    v
}

// Inputs: 0) data reference, 1) offset into data
pub fn get8s(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    do_unpack_std(it, with_args(vec![pl::s("c")], a), w)
}
pub fn get8u(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    do_unpack_std(it, with_args(vec![pl::s("C")], a), w)
}
pub fn get16s(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    do_unpack(it, with_args(vec![n(2), pl::s("s")], a), w)
}
pub fn get16u(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    do_unpack_std(it, with_args(vec![pl::s("S")], a), w)
}
pub fn get32s(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    do_unpack(it, with_args(vec![n(4), pl::s("l")], a), w)
}
pub fn get32u(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    do_unpack_std(it, with_args(vec![pl::s("L")], a), w)
}
pub fn get_float(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    do_unpack(it, with_args(vec![n(4), pl::s("f")], a), w)
}
pub fn get_double(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    do_unpack_dbl(it, with_args(vec![pl::s("d")], a), w)
}
pub fn get16u_rev(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    do_unpack_rev(it, with_args(vec![pl::s("S")], a), w)
}
pub fn get32u_rev(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    do_unpack_rev(it, with_args(vec![pl::s("L")], a), w)
}

/// A scalar-context call of one of the readers above.
fn scalar_of(r: ER<Vec<Value>>) -> ER<Value> {
    Ok(r?.pop().unwrap_or_default())
}

type Reader = fn(&mut Interp, Vec<Value>, Want) -> ER<Vec<Value>>;

// rationals may be a floating point number, 'inf' or 'undef'
fn get_rational(it: &mut Interp, a: Vec<Value>, get: Reader, step: i64, sig: i64) -> ER<Vec<Value>> {
    let (data_pt, pos) = (arg(&a, 0), arg(&a, 1));
    let rat_numer = my_scalar(it, "ratNumer");
    let rat_denom = my_scalar(it, "ratDenom");
    let num = scalar_of(get(it, vec![data_pt.clone(), pos.clone()], Want::Scalar))?;
    *rat_numer.borrow_mut() = num.clone();
    let den = scalar_of(get(it, vec![data_pt, pl::add(&pos, &n(step))], Want::Scalar))?;
    *rat_denom.borrow_mut() = den.clone();
    if !den.truthy() {
        return ret(pl::s(if num.truthy() { "inf" } else { "undef" }));
    }
    // round off to a reasonable number of significant figures
    ret(round_float_v(&pl::div(&num, &den)?, &n(sig)))
}

pub fn get_rational32s(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    get_rational(it, a, get16s, 2, 7)
}
pub fn get_rational32u(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    get_rational(it, a, get16u, 2, 7)
}
pub fn get_rational64s(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    get_rational(it, a, get32s, 4, 10)
}
pub fn get_rational64u(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    get_rational(it, a, get32u, 4, 10)
}

pub fn get_fixed16s(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (data_pt, pos) = (arg(&a, 0), arg(&a, 1));
    let val = pl::div(&scalar_of(get16s(it, vec![data_pt, pos], Want::Scalar))?, &n(0x100))?;
    let r = if pl::lt(&val, &n(0)) { Value::Num(-0.5) } else { Value::Num(0.5) };
    ret(pl::div(&pl::int(&pl::add(&pl::mul(&val, &n(1000)), &r)), &n(1000))?)
}
pub fn get_fixed16u(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (data_pt, pos) = (arg(&a, 0), arg(&a, 1));
    let v = pl::div(&scalar_of(get16u(it, vec![data_pt, pos], Want::Scalar))?, &n(0x100))?;
    ret(pl::div(&pl::int(&pl::add(&pl::mul(&v, &n(1000)), &Value::Num(0.5))), &n(1000))?)
}
pub fn get_fixed32s(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (data_pt, pos) = (arg(&a, 0), arg(&a, 1));
    let val = pl::div(&scalar_of(get32s(it, vec![data_pt, pos], Want::Scalar))?, &n(0x10000))?;
    // remove insignificant digits
    let r = if pl::gt(&val, &n(0)) { Value::Num(0.5) } else { Value::Num(-0.5) };
    ret(pl::div(&pl::int(&pl::add(&pl::mul(&val, &Value::Num(1e5)), &r)), &Value::Num(1e5))?)
}
pub fn get_fixed32u(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (data_pt, pos) = (arg(&a, 0), arg(&a, 1));
    // remove insignificant digits
    let v = pl::div(&scalar_of(get32u(it, vec![data_pt, pos], Want::Scalar))?, &n(0x10000))?;
    ret(pl::div(&pl::int(&pl::add(&pl::mul(&v, &Value::Num(1e5)), &Value::Num(0.5))), &Value::Num(1e5))?)
}

// Inputs: 0) value, 1) data ref, 2) offset
pub fn set8s(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    do_pack_std(it, with_args(vec![pl::s("c")], a), w)
}
pub fn set8u(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    do_pack_std(it, with_args(vec![pl::s("C")], a), w)
}
pub fn set16u(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    do_pack_std(it, with_args(vec![pl::s("S")], a), w)
}
pub fn set32u(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    do_pack_std(it, with_args(vec![pl::s("L")], a), w)
}
pub fn set16u_rev(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    do_pack_rev(it, with_args(vec![pl::s("S")], a), w)
}

/// Get current byte order ('II' or 'MM')
pub fn get_byte_order(it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let v = pkg_scalar(it, "currentByteOrder").borrow().clone();
    ret(v)
}

fn set_unpack_std(it: &mut Interp, from: &str) {
    let src = my_hash(it, from);
    if let (Value::HashRef(dst), Value::HashRef(s)) = (pkg_hash(it, "Image::ExifTool::unpackStd"), src) {
        let copy = s.borrow().clone();
        let mut d = dst.borrow_mut();
        d.clear();
        for (k, v) in copy.iter() {
            d.insert(k, v.clone());
        }
    }
}

/// Set byte ordering
/// Inputs: 0) 'MM'=motorola, 'II'=intel (will translate 'BigEndian', 'LittleEndian')
/// Returns: 1 on success
pub fn set_byte_order(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut order = arg(&a, 0);

    if pl::str_eq(&order, "MM") {
        // big endian (Motorola)
        set_unpack_std(it, "unpackMotorola");
    } else if pl::str_eq(&order, "II") {
        // little endian (Intel)
        set_unpack_std(it, "unpackIntel");
    } else if re(r"^Big", "i").is_match(&order) {
        order = pl::s("MM");
        set_unpack_std(it, "unpackMotorola");
    } else if re(r"^Little", "i").is_match(&order) {
        order = pl::s("II");
        set_unpack_std(it, "unpackIntel");
    } else {
        return ret(n(0));
    }
    let val = unpack_ctx(b"S", b"A ", Want::Scalar)?.pop().unwrap_or_default();
    let native_order;
    if pl::num_eq(&val, &n(0x4120)) {
        // big endian
        native_order = pl::s("MM");
    } else if pl::num_eq(&val, &n(0x2041)) {
        // little endian
        native_order = pl::s("II");
    } else {
        pl::warn(it, &pl::sprintf("Unknown native byte order! (pattern %x)\n", &[val]).to_text())?;
        return ret(n(0));
    }
    *pkg_scalar(it, "currentByteOrder").borrow_mut() = order.clone(); // save current byte order

    // swap bytes if our native CPU byte ordering is not the same as the EXIF
    *pkg_scalar(it, "swapBytes").borrow_mut() = Value::from(order.to_bytes() != native_order.to_bytes());

    // little-endian ARM has big-endian words for doubles (thanks Riku Voipio)
    // (Note: Riku's patch checked for '0ff3', but I think it should be 'f03f' since
    // 1 is '000000000000f03f' on an x86 -- so check for both, but which is correct?)
    let pack1d = pack_v(b"d", &[n(1)])?.to_bytes();
    *pkg_scalar(it, "swapWords").borrow_mut() = Value::from(&pack1d[..] == b"\0\0\x0f\xf3\0\0\0\0" || &pack1d[..] == b"\0\0\xf0\x3f\0\0\0\0");
    ret(n(1))
}

/// Change byte order
pub fn toggle_byte_order(it: &mut Interp, _a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    let cur = pkg_scalar(it, "currentByteOrder").borrow().clone();
    set_byte_order(it, vec![pl::s(if pl::str_eq(&cur, "II") { "MM" } else { "II" })], w)
}

pub fn format_size(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(my_hash(it, "formatSize").get(arg(&a, 0).to_bytes()))
}

/// Read value from binary data (with current byte ordering)
/// Inputs: 0) data reference, 1) value offset, 2) format string,
///         3) number of values (or undef to use all data),
///         4) valid data length relative to offset (or undef to use all data),
///         5) optional pointer to returned rational
/// Returns: converted value, or undefined if data isn't there
///          or list of values in list context
pub fn read_value(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (data_pt, mut offset, format, mut count, mut size, rat_pt) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4), arg(&a, 5));

    let mut len = my_hash(it, "formatSize").get(format.to_bytes());
    if !len.truthy() {
        pl::warn(it, &format!("Unknown format {}", format.to_text()))?;
        len = n(1);
    }
    if size.is_undef() {
        size = pl::sub(&n(deref_scalar(&data_pt)?.to_bytes().len() as i64), &offset);
    }
    if !count.truthy() {
        if !count.is_undef() || pl::lt(&size, &len) {
            return ret(pl::s(""));
        }
        count = pl::int(&pl::div(&size, &len)?);
    }
    // make sure entry is inside data
    if pl::gt(&pl::mul(&len, &count), &size) {
        count = pl::int(&pl::div(&size, &len)?); // shorten count if necessary
        if pl::lt(&count, &n(1)) {
            return pl::ret_undef(); // return undefined if no data
        }
    }
    let mut vals: Vec<Value> = Vec::new();
    let proc_ = my_hash(it, "readValueProc").get(format.to_bytes());
    let is_rational = my_hash(it, "isRational").get(format.to_bytes()).truthy();
    let call_proc = |it: &mut Interp, offset: &Value| -> ER<Vec<Value>> {
        match &proc_ {
            Value::Code(c) => it.call(c, vec![data_pt.clone(), offset.clone()], Want::List),
            _ => die("Not a CODE reference"),
        }
    };
    if !proc_.truthy() {
        // handle undef/binary/string (also unsupported unicode/complex)
        let mut v = substr(&deref_scalar(&data_pt)?, offset.to_i64(), Some(pl::mul(&count, &len).to_i64()));
        // truncate string at null terminator if necessary
        if pl::str_eq(&format, "string") {
            re(r"\0.*", "s").subst(&mut v, false, |_| Vec::new());
        }
        vals.push(v);
    } else if is_rational && rat_pt.truthy() {
        // store rationals separately as string fractions
        let mut rat: Vec<Value> = Vec::new();
        let rat_numer = my_scalar(it, "ratNumer");
        let rat_denom = my_scalar(it, "ratDenom");
        loop {
            vals.extend(call_proc(it, &offset)?);
            let num = rat_numer.borrow().clone();
            let den = rat_denom.borrow().clone();
            rat.push(pl::concat(&pl::concat(&num, &pl::s("/")), &den));
            count = pl::sub(&count, &n(1));
            if pl::le(&count, &n(0)) {
                break;
            }
            offset = pl::add(&offset, &len);
        }
        assign_deref(&rat_pt, join(b" ", &rat))?;
    } else {
        loop {
            vals.extend(call_proc(it, &offset)?);
            count = pl::sub(&count, &n(1));
            if pl::le(&count, &n(0)) {
                break;
            }
            offset = pl::add(&offset, &len);
        }
    }
    if want == Want::List {
        return Ok(vals);
    }
    if vals.len() > 1 {
        return ret(join(b" ", &vals));
    }
    ret(vals.into_iter().next().unwrap_or_default())
}

/// Decode string with specified encoding
/// Inputs: 0) ExifTool object ref, 1) string to decode
///         2) source character set name (undef for current Charset)
///         3) optional source byte order (2-byte and 4-byte fixed-width sets only)
///         4) optional destination character set (defaults to Charset setting)
///         5) optional destination byte order (2-byte and 4-byte fixed-width only)
/// Returns: string in destination encoding
/// Note: ExifTool ref may be undef if character both character sets are provided
///       (but in this case no warnings will be issued)
pub fn decode(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (slf, mut val, mut from, from_order, mut to, to_order) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4), arg(&a, 5));
    if !from.truthy() {
        from = slf.path(&["OPTIONS", "Charset"]);
    }
    if !to.truthy() {
        to = slf.path(&["OPTIONS", "Charset"]);
    }
    if from.to_bytes() != to.to_bytes() && !val.to_bytes().is_empty() {
        runtime::require(it, "Image::ExifTool::Charset")?;
        let cs_type = pkg_hash(it, "Image::ExifTool::Charset::csType");
        let cs1 = cs_type.get(from.to_bytes());
        let cs2 = cs_type.get(to.to_bytes());
        if cs1.truthy() && cs2.truthy() && !ops::bit_and(&cs2, &n(0x002)).truthy() {
            // treat as straight ASCII if no character will need remapping
            if ops::bit_and(&ops::bit_or(&cs1, &cs2), &n(0x680)).truthy() || re(r"[\x80-\xff]", "").is_match(&val) {
                let uni = pl::call(it, "Image::ExifTool::Charset::Decompose", vec![slf.clone(), val.clone(), from.clone(), from_order])?;
                val = pl::call(it, "Image::ExifTool::Charset::Recompose", vec![slf.clone(), uni, to.clone(), to_order])?;
            }
        } else if slf.truthy() {
            let set = if cs1.truthy() { to.clone() } else { from.clone() };
            let key = pl::concat(&pl::s("DecodeWarn"), &set);
            if !slf.get(key.to_bytes()).truthy() {
                let msg = pl::concat(&pl::concat(&pl::s("Unsupported character set ("), &set), &pl::s(")"));
                warn(it, vec![slf.clone(), msg], Want::Scalar)?;
                slf.set(key.to_bytes(), n(1));
            }
        }
    }
    ret(val)
}

/// Encode string (in Charset encoding) to specified encoding
/// Inputs: 0) ExifTool object ref, 1) string, 2) destination character set name,
///         3) optional destination byte order (2-byte and 4-byte fixed-width sets only)
/// Returns: string in specified encoding
pub fn encode(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    let (slf, val, to, to_order) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    decode(it, vec![slf, val, Value::Undef, Value::Undef, to, to_order], w)
}

/// Decode bit mask
/// Inputs: 0) value to decode, 1) Reference to hash for decoding (or undef)
///         2) optional bits per word (defaults to 32)
pub fn decode_bits(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (vals, lookup, mut bits) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if !bits.truthy() {
        bits = n(32);
    }
    let mut bit_list: Vec<Value> = Vec::new();
    let mut num = n(0);
    for val in pl::split_ws(&vals) {
        let mut i = n(0);
        while pl::lt(&i, &bits) {
            if ops::bit_and(&val, &ops::shift_left(&n(1), &i)).truthy() {
                let nn = pl::add(&i, &num);
                if !lookup.truthy() {
                    bit_list.push(nn);
                } else if lookup.get(nn.to_bytes()).truthy() {
                    bit_list.push(lookup.get(nn.to_bytes()));
                } else {
                    bit_list.push(pl::concat(&pl::concat(&pl::s("["), &nn), &pl::s("]")));
                }
            }
            i = pl::add(&i, &n(1));
        }
        num = pl::add(&num, &bits);
    }
    if bit_list.is_empty() {
        return ret(pl::s("(none)"));
    }
    ret(join(if lookup.truthy() { b", " } else { b"," }, &bit_list))
}

// ---- tag names, descriptions, filters ----

/// Validate an extracted image and repair if necessary
/// Inputs: 0) ExifTool object reference, 1) image reference, 2) tag name or key
/// Returns: image reference or undef if it wasn't valid
/// Note: should be called from RawConv, not ValueConv
pub fn validate_image(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (slf, image_pt, tag) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut image = deref_scalar(&image_pt)?;
    if pl::str_eq(&image, "none") {
        return pl::ret_undef();
    }
    let ok = re(r"^(Binary data|\xff\xd8\xff)", "").is_match(&image) || {
        // the first byte of the preview of some Minolta cameras is wrong,
        // so check for this and set it back to 0xff if necessary
        let fixed = re(r"^.(\xd8\xff\xdb)", "s").subst(&mut image, false, |c| [&b"\xff"[..], &cap(c, 1).to_bytes()].concat()) > 0;
        if fixed {
            assign_deref(&image_pt, image.clone())?;
        }
        fixed
    } || pl::method(it, &slf, "Options", vec![pl::s("IgnoreMinorErrors")])?.truthy();
    if !ok {
        // issue warning only if the tag was specifically requested
        let name = pl::call(it, "Image::ExifTool::GetTagName", vec![tag.clone()])?;
        if slf.get("REQ_TAG_LOOKUP").get(lc(&name).to_bytes()).truthy() {
            warn(it, vec![slf.clone(), pl::concat(&tag, &pl::s(" is not a valid JPEG image")), n(1)], Want::Scalar)?;
            return pl::ret_undef();
        }
    }
    ret(image_pt)
}

/// Validate a tag name argument (including group name and wildcards, etc)
/// Inputs: 0) tag name
/// Returns: true if tag name is valid
/// - a tag name may contain [-_A-Za-z0-9], but may not start with [-0-9]
/// - tag names may contain wildcards [?*], and end with a hash [#]
/// - may have group name prefixes (which may have family number prefix), separated by colons
/// - a group name may be zero or more characters
pub fn valid_tag_name(_it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let tag = arg(&a, 0);
    let m = re(r"^(([-\w]*|\d*\*):)*[_a-zA-Z?*][-\w?*]*#?$", "").m(&tag);
    if want == Want::List {
        return Ok(match m {
            Some(c) => vec![cap(&c, 1), cap(&c, 2)],
            None => vec![],
        });
    }
    ret(Value::from(m.is_some()))
}

fn make_tag_name_v(name: &Value) -> Value {
    let mut name = name.clone();
    tr(&mut name, b"-_a-zA-Z0-9", b"", "dc"); // remove illegal characters
    name = ucfirst(&name); // capitalize first letter
    // must at least 2 characters long and not start with - or 0-9-
    if name.to_bytes().len() < 2 || re(r"^[-0-9]", "").is_match(&name) {
        name = pl::concat(&pl::s("Tag"), &name);
    }
    name
}

/// Generate a valid tag name based on the tag ID or name
/// Inputs: 0) tag ID or name
/// Returns: valid tag name
pub fn make_tag_name(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(make_tag_name_v(&arg(&a, 0)))
}

/// Make description from a tag name
/// Inputs: 0) tag name 1) optional tagID to add at end of description
/// Returns: description
pub fn make_description(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (tag, mut tag_id) = (arg(&a, 0), arg(&a, 1));
    // start with the tag name and force first letter to be upper case
    let mut desc = ucfirst(&tag);
    // translate underlines to spaces
    tr(&mut desc, b"_", b" ", "");
    // remove hex TagID from name (to avoid inserting spaces in the number)
    if tag_id.is_undef() {
        let mut c1 = Value::Undef;
        if re(r" (0x[\da-f]+)$", "i").subst(&mut desc, false, |c| {
            c1 = cap(c, 1);
            Vec::new()
        }) > 0
        {
            tag_id = c1;
        }
    }
    let pair = |c: &pl::Caps| [&cap(c, 1).to_bytes()[..], b" ", &cap(c, 2).to_bytes()[..]].concat();
    // put a space between lower/UPPER case and lower/number combinations
    re(r"([a-z])([A-Z\d])", "").subst(&mut desc, true, pair);
    // put a space between acronyms and words
    re(r"([A-Z])([A-Z][a-z])", "").subst(&mut desc, true, pair);
    // put spaces after numbers (if more than one character follows the number)
    re(r"(\d)([A-Z]\S)", "").subst(&mut desc, true, pair);
    // add TagID to description
    if !tag_id.is_undef() {
        desc = pl::concat(&pl::concat(&desc, &pl::s(" ")), &tag_id);
    }
    ret(desc)
}

/// Get descriptions for all tags in an array
/// Inputs: 0) ExifTool ref, 1) reference to list of tag keys
/// Returns: reference to hash lookup for descriptions
/// Note: Returned descriptions are NOT escaped by ESCAPE_PROC
pub fn get_descriptions(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    // (local $_: the loop variable is a Rust one)
    let (slf, tags) = (arg(&a, 0), arg(&a, 1));
    let desc = Value::new_hash();
    let old_escape = slf.get("ESCAPE_PROC");
    slf.delete("ESCAPE_PROC");
    for t in tags.items() {
        let d = pl::method(it, &slf, "GetDescription", vec![t.clone()])?;
        desc.set(t.to_bytes(), d);
    }
    slf.set("ESCAPE_PROC", old_escape);
    ret(desc)
}

/// `local $_ = VALUE` around `f`; returns what `f` left in `$_` too.
fn with_local_topic<T>(it: &mut Interp, v: Value, f: impl FnOnce(&mut Interp) -> ER<T>) -> ER<T> {
    let old = it.global(b'$', "main::_");
    it.set_global(b'$', "main::_", Binding::Scalar(Rc::new(std::cell::RefCell::new(v))));
    let r = f(it);
    it.set_global(b'$', "main::_", old);
    r
}

/// A reference to an element: `\$val[i]` / `\$val{k}`.
fn elem_ref(lv: crate::interp::eval::Lv) -> Value {
    Value::LvRef(Rc::new(lv))
}

fn filter_v(it: &mut Interp, slf: &Value, filter: &Value, val_pt: &Value) -> ER<Value> {
    if filter.is_undef() || deref_scalar(val_pt)?.is_undef() {
        return Ok(n(1));
    }
    let mut rtn_val = Value::Undef;
    let v = deref_scalar(val_pt)?;
    if !v.is_ref() {
        let out = with_local_topic(it, v, |it| {
            //### eval Filter ($_, $self)
            let scope = crate::interp::Scope::new(None);
            scope.scalar("self", slf.clone());
            let _ = it.eval_string(&filter.to_bytes(), ET, &scope, Want::Void);
            Ok(it.get_scalar("main::_"))
        })?;
        if !out.is_undef() {
            assign_deref(val_pt, out)?;
            rtn_val = n(1);
        }
    } else if ref_of(&v) == "SCALAR" {
        let val = Value::ScalarRef(Rc::new(std::cell::RefCell::new(v.deref()))); // make a copy to avoid filtering twice
        rtn_val = filter_v(it, slf, filter, &val)?;
        assign_deref(val_pt, val)?;
    } else if ref_of(&v) == "ARRAY" {
        let arr = Rc::new(std::cell::RefCell::new(v.items())); // make a copy to avoid filtering twice
        let len = arr.borrow().len();
        for i in 0..len {
            let r = elem_ref(crate::interp::eval::Lv::AElem(arr.clone(), i as i64));
            if filter_v(it, slf, filter, &r)?.truthy() {
                rtn_val = n(1);
            }
        }
        assign_deref(val_pt, Value::ArrayRef(arr))?;
    } else if ref_of(&v) == "HASH" {
        let copy = v.hash_copy(); // make a copy to avoid filtering twice
        let Value::HashRef(h) = &copy else { unreachable!() };
        for k in copy.keys() {
            let r = elem_ref(crate::interp::eval::Lv::HElem(h.clone(), k));
            if filter_v(it, slf, filter, &r)?.truthy() {
                rtn_val = n(1);
            }
        }
        assign_deref(val_pt, copy.clone())?;
    } else {
        rtn_val = n(1);
    }
    Ok(rtn_val)
}

/// Apply filter to value(s) if necessary
/// Inputs: 0) ExifTool ref, 1) filter expression, 2) reference to value to filter
/// Returns: true unless a filter returned undef; changes value if necessary
pub fn filter(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (slf, flt, val_pt) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    ret(filter_v(it, &slf, &flt, &val_pt)?)
}

/// Return printable value
/// Inputs: 0) ExifTool object reference
///         1) value to print, 2) line length limit (undef defaults to 60, 0=unlimited)
/// Returns: Printable string
pub fn printable(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (slf, mut out_str, mut max_len) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if out_str.is_undef() {
        return ret(pl::s("(undef)"));
    }
    if ref_of(&out_str) == "SCALAR" {
        return ret(pl::s(&format!("(Binary data {} bytes)", out_str.deref().to_bytes().len())));
    }
    tr(&mut out_str, br"\x01-\x1f\x7f-\xff", b".", "");
    re(r"\x00", "").subst(&mut out_str, true, |_| Vec::new());
    let verbose = slf.path(&["OPTIONS", "Verbose"]);
    if pl::lt(&verbose, &n(4)) {
        if max_len.truthy() {
            if pl::lt(&max_len, &n(20)) {
                max_len = n(20); // minimum length is 20
            }
        } else if !max_len.is_undef() {
            max_len = n(out_str.to_bytes().len() as i64); // 0 is unlimited
        } else {
            max_len = n(60); // default maximum is 60
        }
    } else {
        max_len = n(out_str.to_bytes().len() as i64);
        // limit to 2048 characters if verbose < 5
        if pl::gt(&max_len, &n(2048)) && pl::lt(&verbose, &n(5)) {
            max_len = n(2048);
        }
    }

    // limit length if necessary
    if pl::gt(&n(out_str.to_bytes().len() as i64), &max_len) {
        out_str = pl::concat(&substr(&out_str, 0, Some(pl::sub(&max_len, &n(6)).to_i64())), &pl::s("[snip]"));
    }
    ret(out_str)
}

// ---- date/time and other conversions ----

/// Print conversion for time span value
/// Inputs: 0) time ticks, 1) number of seconds per tick (default 1)
/// Returns: readable time
pub fn convert_time_span(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (mut val, mult) = (arg(&a, 0), arg(&a, 1));
    if is_float_mut(&mut val) && !pl::num_eq(&val, &n(0)) {
        if mult.truthy() {
            val = pl::mul(&val, &mult);
        }
        if pl::lt(&val, &n(60)) {
            val = pl::concat(&val, &pl::s(" seconds"));
        } else if pl::lt(&val, &n(3600)) {
            let fmt = if mult.truthy() && pl::ge(&mult, &n(60)) { "%d" } else { "%.1f" };
            let s = if pl::num_eq(&val, &n(60)) && mult.truthy() { "" } else { "s" };
            val = pl::sprintf(&format!("{fmt} minute{s}"), &[pl::div(&val, &n(60))?]);
        } else if pl::lt(&val, &n(24 * 3600)) {
            val = pl::sprintf("%.1f hours", &[pl::div(&val, &n(3600))?]);
        } else {
            val = pl::sprintf("%.1f days", &[pl::div(&val, &n(24 * 3600))?]);
        }
    }
    ret(val)
}

fn time_local_v(it: &mut Interp, a: Vec<Value>) -> ER<Value> {
    let mut tm = pl::call(it, "Time::Local::timelocal", a)?;
    if pl::str_eq(&os_name(it), "MSWin32") {
        // patch for ActivePerl timezone bug
        let mut t2 = time_list(&tm, true);
        t2[5] = pl::add(&t2[5], &n(1900));
        let t2 = pl::call(it, "Time::Local::timelocal", t2)?;
        // adjust timelocal() return value to be consistent with localtime()
        tm = pl::add(&tm, &pl::sub(&tm, &t2));
    }
    Ok(tm)
}

/// Patched timelocal() that fixes ActivePerl timezone bug
/// Inputs/Returns: same as timelocal()
/// Notes: must 'require Time::Local' before calling this routine.
/// Also note that year should be full year, and not relative to 1900 as with localtime
pub fn time_local(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(time_local_v(it, a)?)
}

fn get_time_zone_v(it: &mut Interp, tm: &Value, gm: &Value) -> ER<Value> {
    // compute the number of minutes between localtime and gmtime
    let mut min = pl::sub(&pl::add(&pl::mul(&tm.at(2), &n(60)), &tm.at(1)), &pl::add(&pl::mul(&gm.at(2), &n(60)), &gm.at(1)));
    if !pl::num_eq(&tm.at(3), &gm.at(3)) {
        // account for case where one date wraps to the first of the next month
        if !pl::num_eq(&pl::abs(&pl::sub(&tm.at(3), &gm.at(3))), &n(1)) {
            let d = if pl::num_eq(&tm.at(3), &n(1)) { n(1) } else { n(-1) };
            gm.set_at(3, pl::sub(&tm.at(3), &d));
        }
        // adjust for the +/- one day difference
        min = pl::add(&min, &pl::mul(&pl::mul(&pl::sub(&tm.at(3), &gm.at(3)), &n(24)), &n(60)));
    }
    // MirBSD patch to round to the nearest 30 minutes because
    // it includes leap seconds in localtime but not gmtime
    if pl::str_eq(&os_name(it), "mirbsd") {
        let r = if pl::gt(&min, &n(0)) { Value::Num(0.5) } else { Value::Num(-0.5) };
        min = pl::mul(&pl::int(&pl::add(&pl::div(&min, &n(30))?, &r)), &n(30));
    }
    Ok(min)
}

/// Get time zone in minutes
/// Inputs: 0) localtime array ref, 1) gmtime array ref
/// Returns: time zone offset in minutes
pub fn get_time_zone(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(get_time_zone_v(it, &arg(&a, 0), &arg(&a, 1))?)
}

fn time_zone_string_v(it: &mut Interp, mut min: Value, t: Value) -> ER<Value> {
    if min.is_ref() {
        let gm = Value::new_array(time_list(&t, false));
        min = get_time_zone_v(it, &min, &gm)?;
    }
    let mut sign = "+";
    if pl::lt(&min, &n(0)) {
        sign = "-";
        min = pl::neg(&min);
    }
    min = pl::int(&pl::add(&min, &Value::Num(0.5))); // round off to nearest minute
    let h = pl::int(&pl::div(&min, &n(60))?);
    Ok(pl::sprintf("%s%.2d:%.2d", &[pl::s(sign), h.clone(), pl::sub(&min, &pl::mul(&h, &n(60)))]))
}

/// Get time zone string
/// Inputs: 0) time zone offset in minutes
///     or  0) localtime array ref, 1) corresponding time value
/// Returns: time zone string ("+/-HH:MM")
pub fn time_zone_string(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(time_zone_string_v(it, arg(&a, 0), arg(&a, 1))?)
}

/// Convert Unix time to EXIF date/time string
/// Inputs: 0) Unix time value, 1) non-zero to convert to local time, 2) number of
///         digits after the decimal for fractional seconds, negative to trim
///         trailing zeros, or undef to use SystemTimeRes
/// Returns: EXIF date/time string (with timezone for local times)
pub fn convert_unix_time(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (time, to_local, mut dec) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    if pl::num_eq(&time, &n(0)) {
        return ret(pl::s("0000:00:00 00:00:00"));
    }
    let mut trim = false;
    let static_vars = pkg_hash(it, "Image::ExifTool::static_vars");
    if dec.is_undef() {
        let r = static_vars.get("SystemTimeRes");
        dec = if r.truthy() { r } else { n(0) };
    }
    if pl::lt(&dec, &n(0)) {
        dec = pl::neg(&dec);
        trim = true;
    }
    let mut itime = pl::int(&time);
    let mut frac = pl::sub(&time, &itime);
    if pl::lt(&frac, &n(0)) {
        frac = pl::add(&frac, &n(1));
        itime = pl::sub(&itime, &n(1));
    }
    dec = pl::sprintf("%.*f", &[dec, frac]);
    // remove number before decimal and increment integer time if necessary
    let mut c1 = Value::Undef;
    if re(r"^(\d)", "").subst(&mut dec, false, |c| {
        c1 = cap(c, 1);
        Vec::new()
    }) > 0
        && pl::str_eq(&c1, "1")
    {
        itime = pl::add(&itime, &n(1));
    }
    if trim {
        re(r"\.?0+$", "").subst(&mut dec, false, |_| Vec::new()); // trim trailing zeros if specified
    }
    let tm;
    let tz;
    if !to_local.truthy() {
        tm = time_list(&itime, false);
        tz = pl::s("");
    } else if static_vars.get("KeepUTCTime").truthy() {
        tm = time_list(&itime, false);
        tz = pl::s("Z");
    } else {
        tm = time_list(&itime, true);
        tz = time_zone_string_v(it, Value::new_array(tm.clone()), itime.clone())?;
    }
    let fmt = pl::concat(&pl::concat(&pl::s("%4d:%.2d:%.2d %.2d:%.2d:%.2d"), &dec), &pl::s("%s"));
    let s = pl::sprintf_v(&fmt, &[pl::add(&tm[5], &n(1900)), pl::add(&tm[4], &n(1)), tm[3].clone(), tm[2].clone(), tm[1].clone(), tm[0].clone(), tz]);
    ret(s)
}

/// Get Unix time from EXIF-formatted date/time string with optional timezone
/// Inputs: 0) EXIF date/time string, 1) non-zero if time is local, or 2 to assume UTC
/// Returns: Unix time (seconds since 0:00 GMT Jan 1, 1970) or undefined on error
pub fn get_unix_time(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (time_str, mut is_local) = (arg(&a, 0), arg(&a, 1));
    if pl::str_eq(&time_str, "0000:00:00 00:00:00") {
        return ret(n(0));
    }
    let mut tm: Vec<Value> = match re(r"^(\d+)[-:](\d+)[-:](\d+)\s+(\d+):(\d+):(\d+)(.*)", "").m(&time_str) {
        Some(c) => (1..=7).map(|i| cap(&c, i)).collect(),
        None => Vec::new(),
    };
    if tm.len() != 7 {
        return pl::ret_undef();
    }
    // (eval { require Time::Local }: always available)
    runtime::require(it, "Time::Local")?;
    it.set_scalar("main::@", pl::s(""));
    let (tz_str, mut tz_sec) = (tm.pop().unwrap(), n(0));
    // use specified timezone offset (if given) instead of local system time
    // if we are converting a local time value
    if is_local.truthy() {
        if let Some(c) = re(r"(?:Z|([-+])(\d+):(\d+))", "i").m(&tz_str) {
            // use specified timezone if one exists
            if cap(&c, 1).truthy() {
                let m = if pl::str_eq(&cap(&c, 1), "-") { n(-60) } else { n(60) };
                tz_sec = pl::mul(&pl::add(&pl::mul(&cap(&c, 2), &n(60)), &cap(&c, 3)), &m);
            }
            is_local = Value::Undef; // convert using GMT corrected for specified timezone
        } else if pl::str_eq(&is_local, "2") {
            is_local = Value::Undef;
        }
    }
    tm[1] = pl::sub(&tm[1], &n(1)); // convert month
    tm.reverse(); // change to order required by timelocal()
    let mut val = if is_local.truthy() { time_local_v(it, tm)? } else { pl::sub(&pl::call(it, "Time::Local::timegm", tm)?, &tz_sec) };
    // handle fractional seconds
    if tz_str.truthy() {
        if let Some(c) = re(r"^(\.\d+)", "").m(&tz_str) {
            val = pl::add(&val, &cap(&c, 1));
        }
    }
    ret(val)
}

/// Print conversion for file size
/// Inputs: 0) file size in bytes, 1) optional ExifTool ref
/// Returns: converted file size
pub fn convert_file_size(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (val, et) = (arg(&a, 0), arg(&a, 1));
    let lt = |x: f64| pl::lt(&val, &Value::Num(x));
    let f = |fmt: &str, d: f64| -> ER<Vec<Value>> { ret(pl::sprintf(fmt, &[pl::div(&val, &Value::Num(d))?])) };
    if et.truthy() && pl::str_eq(&et.path(&["OPTIONS", "ByteUnit"]), "Binary") {
        if lt(2048.0) {
            return ret(pl::concat(&val, &pl::s(" bytes")));
        }
        if lt(10240.0) {
            return f("%.1f KiB", 1024.0);
        }
        if lt(2097152.0) {
            return f("%.0f KiB", 1024.0);
        }
        if lt(10485760.0) {
            return f("%.1f MiB", 1048576.0);
        }
        if lt(2147483648.0) {
            return f("%.0f MiB", 1048576.0);
        }
        if lt(10737418240.0) {
            return f("%.1f GiB", 1073741824.0);
        }
        f("%.0f GiB", 1073741824.0)
    } else {
        if lt(2000.0) {
            return ret(pl::concat(&val, &pl::s(" bytes")));
        }
        if lt(10000.0) {
            return f("%.1f kB", 1000.0);
        }
        if lt(2000000.0) {
            return f("%.0f kB", 1000.0);
        }
        if lt(10000000.0) {
            return f("%.1f MB", 1000000.0);
        }
        if lt(2000000000.0) {
            return f("%.0f MB", 1000000.0);
        }
        if lt(10000000000.0) {
            return f("%.1f GB", 1000000000.0);
        }
        f("%.0f GB", 1000000000.0)
    }
}

/// Convert seconds to duration string (handles negative durations)
/// Inputs: 0) floating point seconds
/// Returns: duration string in form "S.SS s", "H:MM:SS" or "DD days HH:MM:SS"
pub fn convert_duration(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut time = arg(&a, 0);
    // (IsFloat may change the value it is given, as the Perl's does)
    if !is_float_mut(&mut time) {
        return ret(time);
    }
    if pl::num_eq(&time, &n(0)) {
        return ret(pl::s("0 s"));
    }
    let mut sign = if pl::gt(&time, &n(0)) {
        pl::s("")
    } else {
        time = pl::neg(&time);
        pl::s("-")
    };
    if pl::lt(&time, &n(30)) {
        return ret(pl::sprintf_v(&pl::concat(&sign, &pl::s("%.2f s")), &[time]));
    }
    time = pl::add(&time, &Value::Num(0.5)); // to round off to nearest second
    let mut h = pl::int(&pl::div(&time, &n(3600))?);
    time = pl::sub(&time, &pl::mul(&h, &n(3600)));
    let m = pl::int(&pl::div(&time, &n(60))?);
    time = pl::sub(&time, &pl::mul(&m, &n(60)));
    if pl::gt(&h, &n(24)) {
        let d = pl::int(&pl::div(&h, &n(24))?);
        h = pl::sub(&h, &pl::mul(&d, &n(24)));
        sign = pl::concat(&pl::concat(&sign, &d), &pl::s(" days "));
    }
    ret(pl::sprintf_v(&pl::concat(&sign, &pl::s("%d:%.2d:%.2d")), &[h, m, pl::int(&time)]))
}

/// Print conversion for bitrate values
/// Inputs: 0) bitrate in bits per second
/// Returns: human-readable bitrate string
/// Notes: returns input value without formatting if it isn't numerical
pub fn convert_bitrate(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut bitrate = arg(&a, 0);
    if !is_float_mut(&mut bitrate) {
        return ret(bitrate);
    }
    let mut units: Vec<&str> = vec!["bps", "kbps", "Mbps", "Gbps"];
    loop {
        let u = if units.is_empty() { "" } else { units.remove(0) };
        if pl::ge(&bitrate, &n(1000)) && !units.is_empty() {
            bitrate = pl::div(&bitrate, &n(1000))?;
            continue;
        }
        let fmt = if pl::lt(&bitrate, &n(100)) { "%.3g" } else { "%.0f" };
        return ret(pl::sprintf(&format!("{fmt} {u}"), &[bitrate]));
    }
}

/// Convert file name for printing
/// Inputs: 0) ExifTool ref, 1) file name in CharsetFileName character set
/// Returns: converted file name in external character set
pub fn convert_file_name(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (slf, mut val) = (arg(&a, 0), arg(&a, 1));
    let enc = slf.path(&["OPTIONS", "CharsetFileName"]);
    if enc.truthy() {
        val = decode(it, vec![slf, val, enc], Want::Scalar)?.pop().unwrap_or_default();
    }
    ret(val)
}

/// Inverse conversion for file name (encode in CharsetFileName)
/// Inputs: 0) ExifTool ref, 1) file name in external character set
/// Returns: file name in CharsetFileName character set
pub fn inverse_file_name(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (slf, mut val) = (arg(&a, 0), arg(&a, 1));
    let enc = slf.path(&["OPTIONS", "CharsetFileName"]);
    if enc.truthy() {
        val = encode(it, vec![slf, val, enc], Want::Scalar)?.pop().unwrap_or_default();
    }
    tr(&mut val, b"\\\\", b"/", ""); // make sure we are using forward slashes
    ret(val)
}

/// Limit length of long values (to be used in PrintConv)
/// Inputs: 0) string value, 1) ExifTool ref
/// Returns: length-limited value
pub fn limit_long_values(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (mut str, slf) = (arg(&a, 0), arg(&a, 1));
    let lim = slf.path(&["OPTIONS", "LimitLongValues"]);
    if pl::gt(&n(str.to_bytes().len() as i64), &lim) && pl::ge(&lim, &n(5)) {
        str = pl::concat(&substr(&str, 0, Some(pl::sub(&lim, &n(5)).to_i64())), &pl::s("[...]"));
    }
    ret(str)
}

/// Save information for HTML dump
/// Inputs: 0) ExifTool hash ref, 1) start offset, 2) data size
///         3) comment string, 4) tool tip (or SAME), 5) flags, 6) IFD name
pub fn hdump(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let slf = arg(&a, 0);
    let dump = slf.get("HTML_DUMP");
    if !dump.truthy() {
        return pl::ret_nothing(want);
    }
    let (mut pos, mut len, com, mut tip, flg, ifd) = (arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4), arg(&a, 5), arg(&a, 6));
    if slf.get("BASE").truthy() {
        pos = pl::add(&pos, &slf.get("BASE"));
    }
    // skip structural data blocks which have been removed from the middle of this dump
    // (SkipData list contains ordered [start,end+1] offsets to skip)
    if slf.get("SkipData").truthy() {
        let end = pl::add(&pos, &len);
        for skip in slf.get("SkipData").items() {
            if pl::le(&end, &skip.at(0)) {
                break;
            }
            if pl::ge(&pos, &skip.at(1)) {
                pos = pl::add(&pos, &pl::sub(&skip.at(1), &skip.at(0)));
                continue;
            }
            if !pl::num_eq(&pos, &skip.at(0)) {
                pl::method(it, &dump, "Add", vec![pos.clone(), pl::sub(&skip.at(0), &pos), com.clone(), tip.clone(), flg.clone(), ifd.clone()])?;
                len = pl::sub(&len, &pl::sub(&skip.at(0), &pos));
                tip = pl::s("SAME");
            }
            pos = skip.at(1);
        }
    }
    it.call_method(&dump, "Add", vec![dump.clone(), pos, len, com, tip, flg, ifd], want)
}

/// Get JPEG marker name
/// Inputs: 0) Jpeg number
/// Returns: marker name
pub fn jpeg_marker_name(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let marker = arg(&a, 0);
    let jpeg_marker = pkg_hash(it, "Image::ExifTool::jpegMarker");
    let mut marker_name = jpeg_marker.get(marker.to_bytes());
    if !marker_name.truthy() {
        marker_name = jpeg_marker.get(ops::bit_and(&marker, &n(0xf0)).to_bytes());
        let m = if marker_name.truthy() { re(r"^([A-Z]+)\d+$", "").m(&marker_name) } else { None };
        if let Some(c) = m {
            marker_name = pl::concat(&cap(&c, 1), &ops::bit_and(&marker, &n(0x0f)));
        } else {
            marker_name = pl::sprintf("marker 0x%.2x", &[marker]);
        }
    }
    ret(marker_name)
}

/// Does group name match the tag ID?
/// Inputs: 0) tag ID, 1) group name (with "ID-" removed)
/// Returns: true on success
pub fn is_same_id(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (mut id, mut grp) = (arg(&a, 0), arg(&a, 1));
    loop {
        if grp.to_bytes() == id.to_bytes() {
            return ret(n(1)); // decimal ID's or raw ID's
        }
        if re(r"^\d+$", "").is_match(&id) {
            // numerical numerical ID's may be in hex
            if re(r"^0x0*", "").subst(&mut grp, false, |_| Vec::new()) > 0 && grp.to_bytes() == pl::sprintf("%x", &[id.clone()]).to_bytes() {
                return ret(n(1));
            }
        } else {
            // other ID's may conform to ExifTool group name conventions
            let mut tmp = id.clone();
            if re(r"([^-_A-Za-z0-9])", "").subst(&mut tmp, true, |c| format!("{:02x}", cap(c, 1).to_bytes().first().copied().unwrap_or(0)).into_bytes()) > 0
                && grp.to_bytes() == tmp.to_bytes()
            {
                return ret(n(1));
            }
        }
        if re(r"-.*", "").subst(&mut id, false, |_| Vec::new()) == 0 {
            break; // remove language code if it exists
        }
    }
    ret(n(0))
}

/// Split file name into directory and name parts
/// Inptus: 0) file name
/// Returns: 0) directory, 1) filename
pub fn split_file_name(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let file = arg(&a, 0);
    // (eval { require File::Basename } always succeeds here)
    runtime::require(it, "File::Basename")?;
    it.set_scalar("main::@", pl::s(""));
    let dir = pl::call(it, "File::Basename::dirname", vec![file.clone()])?;
    let name = pl::call(it, "File::Basename::basename", vec![file])?;
    Ok(vec![dir, name])
}

// ---- requested tags ----

/// `/^$pat$/i` built from a string at run time.
fn re_full(pre: &str, pat: &Value, post: &str, flags: &str) -> ER<pl::Re> {
    let mut p = pre.as_bytes().to_vec();
    p.extend_from_slice(&pat.to_bytes());
    p.extend_from_slice(post.as_bytes());
    pl::re_dyn(&p, flags)
}

/// `grep /RE/, LIST`
fn grep_re(r: &pl::Re, list: &[Value]) -> Vec<Value> {
    list.iter().filter(|v| r.is_match(v)).cloned().collect()
}

fn group_matches_v(it: &mut Interp, slf: &Value, group: &Value, tag_list: &Value, want: Want) -> ER<Vec<Value>> {
    let tag_list = if tag_list.is_ref() { tag_list.clone() } else { Value::new_array(vec![tag_list.clone()]) };
    let mut matches: Vec<Value> = Vec::new();
    // check each group name individually (eg. "Author:1IPTC")
    let mut grps = pl::split(&re(":", ""), group, 0);
    let mut fmys: Vec<Value> = vec![Value::Undef; grps.len()];
    for g in 0..grps.len() {
        let (mut c1, mut c2) = (Value::Undef, Value::Undef);
        if re(r"^(\d*)(id-)?", "i").subst(&mut grps[g], false, |c| {
            c1 = cap(c, 1);
            c2 = cap(c, 2);
            Vec::new()
        }) > 0
        {
            if !c1.to_bytes().is_empty() {
                fmys[g] = c1;
            }
            if c2.truthy() {
                fmys[g] = n(7);
                continue; // (don't convert tag ID's to lower case)
            }
        }
        grps[g] = lc(&grps[g]);
        if pl::str_eq(&grps[g], "copy0") {
            grps[g] = pl::s(""); // accept 'Copy0' for primary tag
        }
    }
    for tag in tag_list.items() {
        let groups = pl::method_list(it, slf, "GetGroup", vec![tag.clone(), n(-1)])?;
        let mut g = 0;
        while g < grps.len() {
            let grp = grps[g].clone();
            if pl::str_eq(&grp, "*") || pl::str_eq(&grp, "all") {
                g += 1;
                continue;
            }
            let f = fmys[g].clone();
            if !f.is_undef() {
                let fi = f.to_i64();
                let gf = if fi >= 0 { groups.get(fi as usize).cloned().unwrap_or_default() } else { Value::Undef };
                if gf.is_undef() {
                    break;
                }
                if pl::num_eq(&f, &n(7)) {
                    let id = pl::method(it, slf, "GetTagID", vec![tag.clone()])?;
                    if is_same_id(it, vec![id, grp.clone()], Want::Scalar)?.pop().unwrap_or_default().truthy() {
                        g += 1;
                        continue;
                    }
                } else if grp.to_bytes() == lc(&gf).to_bytes() {
                    g += 1;
                    continue;
                }
                break;
            } else {
                let r = re_full("^", &grp, "$", "i")?;
                if grep_re(&r, &groups).is_empty() {
                    break;
                }
            }
            g += 1;
        }
        if g == grps.len() {
            if want != Want::List {
                return ret(tag);
            }
            matches.push(tag);
        }
    }
    if want == Want::List {
        Ok(matches)
    } else {
        ret(matches.into_iter().next().unwrap_or_default())
    }
}

/// Get list of tags in specified group
/// Inputs: 0) ExifTool ref, 1) group spec (case insensitive), 2) tag key or reference to list of tag keys
/// Returns: list of matching tags in list context, or first match in scalar context
/// Notes: Group spec may contain multiple groups separated by colons, each
///        possibly with a leading family number
pub fn group_matches(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    group_matches_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2), want)
}

/// RemoveTagsFromList: returns the new tag list (what the Perl stores in `$_[0]`).
fn remove_tags_from_list_v(tags: &Value, list1: &Value, list2: &Value, exclude: &Value, inv: bool) -> Value {
    // (local $_: the loop variable is a Rust one)
    let mut filtered_tags: Vec<Value> = Vec::new();

    let renumber = |list: &Value, i: i64| {
        let items: Vec<Value> = list
            .items()
            .into_iter()
            .filter_map(|x| {
                if pl::lt(&x, &n(i)) {
                    Some(x)
                } else if pl::num_eq(&x, &n(i)) {
                    None
                } else {
                    Some(pl::sub(&x, &n(1)))
                }
            })
            .collect();
        if let Value::ArrayRef(a) = list {
            *a.borrow_mut() = items;
        }
    };
    if !list1.is_empty() || !list2.is_empty() {
        while !tags.is_empty() {
            let Value::ArrayRef(arr) = tags else { break };
            let tag = arr.borrow_mut().pop().unwrap_or_default();
            let i = arr.borrow().len() as i64;
            if exclude.get(tag.to_bytes()).truthy() != inv {
                // remove index of excluded tag from each list
                renumber(list1, i);
                renumber(list2, i);
            } else {
                filtered_tags.insert(0, tag);
            }
        }
    } else {
        for t in tags.items() {
            if exclude.get(t.to_bytes()).truthy() == inv {
                filtered_tags.push(t);
            }
        }
    }
    Value::new_array(filtered_tags) // update tag list
}

/// Remove specified tags from returned tag list, updating indices in other lists
/// Inputs: 0) tag list ref, 1) index list ref, 2) index list ref, 3) hash ref,
///         4) true to include tags from hash instead of excluding
/// Returns: nothing, but updates input lists
pub fn remove_tags_from_list(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let r = remove_tags_from_list_v(&arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3), arg(&a, 4).truthy());
    it.set_arg(0, r.clone())?;
    ret(r)
}

/// Copy tags from alternate input file
/// Inputs: 0) ExifTool ref, 1) family 8 group, 2) list ref for tag keys to copy
/// - updates tag key list to match keys newly added to $self
pub fn copy_alt_info(_it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (slf, g8, tags) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let Some(c) = re(r"(\d+)", "").m(&g8) else { return pl::ret_nothing(want) };
    let et = slf.get("ALT_EXIFTOOL").get(g8.to_bytes());
    if !et.truthy() {
        return pl::ret_nothing(want);
    }
    let alt_order = pl::mul(&pl::add(&cap(&c, 1), &n(1)), &n(100000)); // increment file order
    for (i, tag) in tags.items().into_iter().enumerate() {
        let mut vtag = tag.clone();
        let rep = [&b" #["[..], &g8.to_bytes()[..], b"]"].concat();
        re(r"( |$)", "").subst(&mut vtag, false, |_| rep.clone());
        if slf.get("VALUE").get(vtag.to_bytes()).is_undef() {
            slf.get_or_new_hash("VALUE").set(vtag.to_bytes(), et.get("VALUE").get(tag.to_bytes()));
            slf.get_or_new_hash("TAG_INFO").set(vtag.to_bytes(), et.get("TAG_INFO").get(tag.to_bytes()));
            let extra = et.get("TAG_EXTRA").get(tag.to_bytes());
            slf.get_or_new_hash("TAG_EXTRA").set(vtag.to_bytes(), if extra.truthy() { extra } else { Value::new_hash() });
            let fo = et.get("FILE_ORDER").get(tag.to_bytes());
            slf.get_or_new_hash("FILE_ORDER").set(vtag.to_bytes(), pl::add(&if fo.truthy() { fo } else { n(0) }, &alt_order));
        }
        tags.set_at(i, vtag);
    }
    pl::ret_nothing(want)
}

/// The keys of `%Image::ExifTool::Shortcuts::Main` matching `/^\Q$tag\E$/i`:
/// the first one (`my ($match) = grep ...`).
fn shortcut_match(main: &Value, tag: &Value) -> ER<Value> {
    let q = Value::bytes(crate::perl::regex::quotemeta(&tag.to_bytes()));
    let r = re_full("^", &q, "$", "i")?;
    for k in main.keys() {
        let k = Value::Str(k);
        if r.is_match(&k) {
            return Ok(k);
        }
    }
    Ok(Value::Undef)
}

fn cat(parts: &[&Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

fn expand_shortcuts_v(it: &mut Interp, tag_list: &Value, remove_suffix: bool) -> ER<()> {
    if !tag_list.truthy() || tag_list.is_empty() {
        return Ok(());
    }

    runtime::require(it, "Image::ExifTool::Shortcuts")?;
    let main = pkg_hash(it, "Image::ExifTool::Shortcuts::Main");

    // expand shortcuts
    let suffix = pl::s(if remove_suffix { "" } else { "#" });
    let empty = pl::s("");
    let mut expanded_tags: Vec<Value> = Vec::new();
    'entry: for entry in tag_list.items() {
        // skip things like options hash references in list
        if entry.is_ref() {
            expanded_tags.push(entry);
            continue;
        }
        // remove leading '-'
        let c = re(r"^(-?)(.*)", "s").m(&entry).unwrap_or_default();
        let (excl, mut tag) = (cap(&c, 1), cap(&c, 2));
        let mut post;
        let mut posts: Vec<Value> = Vec::new();
        let pre;
        let mut v;
        // handle redirection
        let redirect = if !excl.truthy() { re(r"(.+?)([-+]?[<>].+)", "s").m(&tag) } else { None };
        if let Some(c) = redirect {
            tag = cap(&c, 1);
            post = cap(&c, 2);
            if re(r"^[-+]?>", "").is_match(&post) || !re(r"\$", "").is_match(&post) {
                // expand shortcuts in postfix (rhs of redirection)
                let c2 = re(r"([-+]?[<>])(.+:)?(.+)", "").m(&post).unwrap_or_default();
                let (op, mut p2, mut t2) = (cap(&c2, 1), cap(&c2, 2), cap(&c2, 3));
                if p2.is_undef() {
                    p2 = pl::s("");
                }
                v = if re(r"#$", "").subst(&mut t2, false, |_| Vec::new()) > 0 { suffix.clone() } else { empty.clone() }; // ValueConv suffix
                let m = shortcut_match(&main, &t2)?;
                if m.truthy() {
                    for x in main.get(m.to_bytes()).items() {
                        if re(r"^-", "").is_match(&x) {
                            continue; // ignore excluded tags
                        }
                        if p2.truthy() && re(r"(.+:)(.+)", "").is_match(&x) {
                            posts.push(cat(&[&op, &x, &v]));
                        } else {
                            posts.push(cat(&[&op, &p2, &x, &v]));
                        }
                    }
                    if posts.is_empty() {
                        continue 'entry;
                    }
                    post = posts.remove(0);
                }
            }
        } else {
            post = pl::s("");
        }
        // handle group names
        if let Some(c) = re(r"(.+:)(.+)", "").m(&tag) {
            pre = cap(&c, 1);
            tag = cap(&c, 2);
        } else {
            pre = pl::s("");
        }
        v = if re(r"#$", "").subst(&mut tag, false, |_| Vec::new()) > 0 { suffix.clone() } else { empty.clone() }; // ValueConv suffix
        // loop over all postfixes
        loop {
            // expand the tag name
            let m = shortcut_match(&main, &tag)?;
            if m.truthy() {
                let list = main.get(m.to_bytes()).items();
                if excl.truthy() {
                    // entry starts with '-', so exclude all tags in this shortcut
                    for x in list {
                        if re(r"^-", "").is_match(&x) {
                            continue; // ignore excluded exclude tags
                        }
                        // group of expanded tag takes precedence
                        if pre.truthy() && re(r"(.+:)(.+)", "").is_match(&x) {
                            expanded_tags.push(cat(&[&excl, &x]));
                        } else {
                            expanded_tags.push(cat(&[&excl, &pre, &x]));
                        }
                    }
                } else if !pre.to_bytes().is_empty() || !post.to_bytes().is_empty() || v.truthy() {
                    for x in list {
                        let c = re(r"(-?)(.+:)?(.+)", "").m(&x).unwrap_or_default();
                        if cap(&c, 2).truthy() {
                            // group from expanded tag takes precedence
                            expanded_tags.push(cat(&[&x, &v, &post]));
                        } else {
                            expanded_tags.push(cat(&[&cap(&c, 1), &pre, &cap(&c, 3), &v, &post]));
                        }
                    }
                } else {
                    expanded_tags.extend(list);
                }
            } else {
                expanded_tags.push(cat(&[&excl, &pre, &tag, &v, &post]));
            }
            if posts.is_empty() {
                break;
            }
            post = posts.remove(0);
        }
    }
    if let Value::ArrayRef(a) = tag_list {
        *a.borrow_mut() = expanded_tags;
    }
    Ok(())
}

/// Expand shortcuts
/// Inputs: 0) reference to list of tags, 1) set to remove trailing '#'
/// Notes: Handles leading '-' for excluded tags, trailing '#' for ValueConv,
///        multiple group names, and redirected tags
pub fn expand_shortcuts(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    expand_shortcuts_v(it, &arg(&a, 0), arg(&a, 1).truthy())?;
    pl::ret_nothing(want)
}

/// Add hash of Composite tags to our composites
/// Inputs: 0) hash reference to table of Composite tags to add or module name,
///         1) override existing tag definition
pub fn add_composite_tags(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    // (local $_: the loop variables are Rust ones)
    let (mut add, over_ride) = (arg(&a, 0), arg(&a, 1));
    let module;
    let mut prefix;
    if !add.is_ref() {
        prefix = add.clone();
        re(r".*::", "").subst(&mut prefix, false, |_| Vec::new());
        module = add.clone();
        let name = pl::concat(&add, &pl::s("::Composite"));
        add = pkg_hash(it, &name.to_text());
        prefix = pl::concat(&prefix, &pl::s("-"));
    } else {
        module = Value::Undef;
        prefix = pl::s("UserDefined-");
    }
    let mut default_groups = add.get("GROUPS");
    let comp_table = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::Composite")])?;

    // make sure default groups are defined in families 0 and 1
    if default_groups.truthy() {
        for (k, d) in [("0", "Composite"), ("1", "Composite"), ("2", "Other")] {
            if !default_groups.get(k).truthy() {
                default_groups.set(k, pl::s(d));
            }
        }
    } else {
        default_groups = Value::new_hash();
        default_groups.set("0", pl::s("Composite"));
        default_groups.set("1", pl::s("Composite"));
        default_groups.set("2", pl::s("Other"));
        add.set("GROUPS", default_groups.clone());
    }
    setup_tag_table(it, vec![add.clone()], Want::Void)?; // generate Name, TagID, etc
    let special_tags = pkg_hash(it, "Image::ExifTool::specialTags");
    let composite_id = my_hash(it, "compositeID");
    let mut ids: Vec<Value> = add.keys().into_iter().map(Value::Str).collect();
    ids.sort_by(ops::str_cmp);
    for tag_id in ids {
        if special_tags.get(tag_id.to_bytes()).truthy() {
            continue; // must skip special tags
        }
        let tag_info = add.get(tag_id.to_bytes());
        let mut new = pl::concat(&prefix, &tag_id); // new tag ID for Composite table
        if tag_info.get("Writable").truthy() {
            tag_info.set("Module", module.clone());
        }
        if over_ride.truthy() && tag_info.get("Override").is_undef() {
            tag_info.set("Override", n(1));
        }
        tag_info.set("IsComposite", n(1));
        // handle Composite tags with the same name
        if composite_id.get(tag_id.to_bytes()).truthy() {
            // determine if we want to override this tag
            // (=0 keep both, >0 override, <0 keep existing)
            let o1 = tag_info.get("Override");
            let first = composite_id.get(tag_id.to_bytes()).at(0);
            let o2 = comp_table.get(first.to_bytes()).get("Override");
            let over = pl::sub(&if o1.truthy() { o1 } else { n(0) }, &if o2.truthy() { o2 } else { n(0) });
            if pl::lt(&over, &n(0)) {
                continue;
            }
            if over.truthy() {
                // remove existing tags with this ID
                for x in composite_id.get(tag_id.to_bytes()).items() {
                    comp_table.delete(x.to_bytes());
                }
                composite_id.delete(tag_id.to_bytes());
            }
        }
        // make sure new TagID is unique by adding index if necessary
        // (could only happen for UserDefined tags now that module name is added to tag ID)
        let mut nn: i64 = 0;
        while comp_table.get(new.to_bytes()).truthy() {
            let old = nn;
            nn += 1;
            if old != 0 {
                re(r"-\d+$", "").subst(&mut new, false, |_| Vec::new());
            }
            new = pl::concat(&new, &pl::s(&format!("-{nn}")));
        }
        // use new ID and save it so we can use it in TagLookup
        if tag_id.to_bytes() != new.to_bytes() {
            tag_info.set("NewTagID", new.clone());
        }

        // add new ID to lookup of Composite tag ID's
        if !composite_id.get(tag_id.to_bytes()).truthy() {
            composite_id.set(tag_id.to_bytes(), Value::new_array(Vec::new()));
        }
        composite_id.get(tag_id.to_bytes()).unshift(new.clone()); // (most recent one first)

        // convert scalar Require/Desire/Inhibit entries
        let (mut hashes, mut scalars): (Vec<&str>, Vec<&str>) = (Vec::new(), Vec::new());
        let used = Value::new_hash();
        for typ in ["Require", "Desire", "Inhibit"] {
            let req = tag_info.get(typ);
            if !req.truthy() {
                continue;
            }
            if ref_of(&req) == "HASH" {
                hashes.push(typ);
            } else {
                scalars.push(typ);
            }
        }
        if !scalars.is_empty() {
            // make lookup for indices that are used
            for typ in &hashes {
                for k in tag_info.get(typ).keys() {
                    used.set(&k, n(1));
                }
            }
            let mut next: i64 = 0;
            for typ in &scalars {
                while used.get(next.to_string()).truthy() {
                    next += 1;
                }
                let h = Value::new_hash();
                h.set(next.to_string(), tag_info.get(typ));
                next += 1;
                tag_info.set(typ, h);
            }
        }
        // add this Composite tag to our main Composite table
        tag_info.set("Table", comp_table.clone());
        // (use the original TagID, even if we changed it, so don't do this:)
        tag_info.set("TagID", new.clone());
        // save tag under new ID in Composite table
        comp_table.set(new.to_bytes(), tag_info.clone());
        // set all default groups in tag
        let mut groups = tag_info.get("Groups");
        if !groups.truthy() {
            groups = Value::new_hash();
            tag_info.set("Groups", groups.clone());
        }
        // fill in default groups
        for k in default_groups.keys() {
            if !groups.get(&k).truthy() {
                groups.set(&k, default_groups.get(&k));
            }
        }
        // set flag indicating group list was built
        tag_info.set("GotGroups", n(1));
    }
    pl::ret_nothing(want)
}

/// Parse function arguments and set member variables accordingly
/// Inputs: Same as ImageInfo()
/// - sets REQUESTED_TAGS, REQ_TAG_LOOKUP, IO_TAG_LIST, FILENAME, RAF, OPTIONS
pub fn parse_arguments(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let mut args = a.into_iter();
    let slf = args.next().unwrap_or_default();
    let options = slf.get("OPTIONS");
    let mut old_group_opts: Vec<Value> = slf.get("OPTIONS").keys().into_iter().map(Value::Str).filter(|k| re("^Group", "").is_match(k)).collect();
    let exclude = Value::new_array(Vec::new());
    let mut was_exclude_opt = false;

    slf.set("REQUESTED_TAGS", Value::new_array(Vec::new()));
    if !slf.get("ReqTagAlreadySet").truthy() {
        slf.set("REQ_TAG_LOOKUP", Value::new_hash());
    }
    slf.set("EXCL_TAG_LOOKUP", Value::new_hash());
    slf.set("IO_TAG_LIST", Value::Undef);
    slf.delete("EXCL_XMP_LOOKUP");

    // handle our input arguments
    for mut arg in args.by_ref() {
        if arg.is_ref() && !pl::call(it, "overload::Method", vec![arg.clone(), pl::s("\"\"")])?.truthy() {
            if ref_of(&arg) == "ARRAY" {
                slf.set("IO_TAG_LIST", arg.clone());
                for x in arg.items() {
                    if let Some(c) = re(r"^-(.*)", "").m(&x) {
                        exclude.push(cap(&c, 1));
                    } else {
                        slf.get("REQUESTED_TAGS").push(x);
                    }
                }
            } else if ref_of(&arg) == "HASH" {
                for opt in arg.keys() {
                    let opt = Value::Str(opt);
                    // a single new group option overrides all old group options
                    if !old_group_opts.is_empty() && re("^Group", "").is_match(&opt) {
                        for o in &old_group_opts {
                            options.delete(o.to_bytes());
                        }
                        old_group_opts.clear();
                    }
                    pl::method(it, &slf, "Options", vec![opt.clone(), arg.get(opt.to_bytes())])?;
                    if pl::str_eq(&opt, "Exclude") {
                        was_exclude_opt = true;
                    }
                }
            } else if ref_of(&arg) == "SCALAR" || pl::call(it, "UNIVERSAL::isa", vec![arg.clone(), pl::s("GLOB")])?.truthy() {
                if !slf.get("RAF").is_undef() {
                    continue;
                }
                // convert image data from UTF-8 to character stream if necessary
                // (patches RHEL 3 UTF8 LANG problem)
                let mut repack = false;
                if ref_of(&arg) == "SCALAR" && pl::ge(&it.get_scalar("main::]"), &Value::Num(5.006)) {
                    repack = slf.path(&["OPTIONS", "EncodeHangs"]).truthy() || {
                        // eval { require Encode; Encode::is_utf8($$arg) }
                        runtime::require(it, "Encode")?;
                        match pl::call(it, "Encode::is_utf8", vec![arg.deref()]) {
                            Ok(v) => {
                                it.set_scalar("main::@", pl::s(""));
                                v.truthy()
                            }
                            Err(Flow::Die(e)) => {
                                it.set_scalar("main::@", e);
                                false
                            }
                            Err(e) => return Err(e),
                        }
                    } || it.get_scalar("main::@").truthy();
                }
                if repack {
                    let data = arg.deref();
                    let buff = with_set_warning(it, |it| {
                        // repack by hand if Encode isn't available
                        if slf.path(&["OPTIONS", "EncodeHangs"]).truthy() || it.get_scalar("main::@").truthy() {
                            let t: &[u8] = if pl::lt(&it.get_scalar("main::]"), &Value::Num(5.010000)) { b"U0C*" } else { b"C0C*" };
                            let codes = match pack::unpack(t, &data.to_bytes()) {
                                Ok(v) => v,
                                Err(e) => return die(e),
                            };
                            pack_v(b"C*", &codes)
                        } else {
                            pl::call(it, "Encode::encode", vec![pl::s("utf8"), data.clone()])
                        }
                    })?;
                    arg = Value::ScalarRef(Rc::new(std::cell::RefCell::new(buff)));
                }
                let raf = pl::method(it, &pl::s("File::RandomAccess"), "new", vec![arg])?;
                slf.set("RAF", raf);
                // set filename to empty string to indicate that
                // we have a file but we didn't open it
                slf.set("FILENAME", pl::s(""));
            } else if pl::call(it, "UNIVERSAL::isa", vec![arg.clone(), pl::s("File::RandomAccess")])?.truthy() {
                slf.set("RAF", arg);
                slf.set("FILENAME", pl::s(""));
            } else {
                pl::warn(it, &format!("Don't understand ImageInfo argument {}\n", arg.to_text()))?;
            }
        } else if !slf.get("FILENAME").is_undef() {
            if let Some(c) = re(r"^-(.*)", "").m(&arg) {
                exclude.push(cap(&c, 1));
            } else {
                slf.get("REQUESTED_TAGS").push(arg);
            }
        } else {
            slf.set("FILENAME", arg);
        }
    }
    // add additional requested tags to lookup
    if options.get("RequestTags").truthy() {
        for t in options.get("RequestTags").items() {
            slf.get_or_new_hash("REQ_TAG_LOOKUP").set(t.to_bytes(), n(1));
        }
    }
    // expand shortcuts in tag arguments if provided
    if !slf.get("REQUESTED_TAGS").is_empty() {
        expand_shortcuts_v(it, &slf.get("REQUESTED_TAGS"), false)?;
        // initialize lookup for requested tags
        for t in slf.get("REQUESTED_TAGS").items() {
            let Some(c) = re(r"^(.*:)?([-\w?*]*)#?$", "").m(&t) else { continue };
            if cap(&c, 2).truthy() {
                slf.get_or_new_hash("REQ_TAG_LOOKUP").set(lc(&cap(&c, 2)).to_bytes(), n(1));
            }
            if !cap(&c, 1).truthy() {
                continue;
            }
            for g in pl::split(&re(":", ""), &cap(&c, 1), 0) {
                slf.get_or_new_hash("REQ_TAG_LOOKUP").set(pl::concat(&lc(&g), &pl::s(":")).to_bytes(), n(1));
            }
        }
    }
    if !exclude.is_empty() || was_exclude_opt {
        // must add existing excluded tags
        if options.get("Exclude").truthy() {
            for x in options.get("Exclude").items() {
                exclude.push(x);
            }
        }
        options.set("Exclude", exclude.clone());
        // expand shortcuts in new exclude list
        expand_shortcuts_v(it, &options.get("Exclude"), true)?; // (also remove '#' suffix)
    }
    // generate lookup for excluded tags
    if options.get("Exclude").truthy() {
        for x in options.get("Exclude").items() {
            if let Some(c) = re(r"([-\w]+)#?$", "").m(&x) {
                slf.get_or_new_hash("EXCL_TAG_LOOKUP").set(lc(&cap(&c, 1)).to_bytes(), n(1));
            }
            if let Some(c) = re(r"(xmp-.*:[-\w]+)#?", "i").m(&x) {
                if !slf.get("EXCL_XMP_LOOKUP").truthy() {
                    slf.set("EXCL_XMP_LOOKUP", Value::new_hash());
                }
                slf.get("EXCL_XMP_LOOKUP").set(lc(&cap(&c, 1)).to_bytes(), n(1));
            }
        }
        // exclude list is used only for EXCL_TAG_LOOKUP when TAGS_FROM_FILE is set
        if slf.get("TAGS_FROM_FILE").truthy() {
            options.set("Exclude", Value::Undef);
        }
    }
    pl::ret_nothing(want)
}

/// `sort { $$fileOrder{$a} <=> $$fileOrder{$b} } LIST`
fn sort_by_file_order(file_order: &Value, mut list: Vec<Value>) -> Vec<Value> {
    list.sort_by(|a, b| ops::num_cmp(&file_order.get(a.to_bytes()), &file_order.get(b.to_bytes())).unwrap_or(std::cmp::Ordering::Equal));
    list
}

/// Set list of found tags from previously requested tags
/// Inputs: 0) ExifTool object reference
/// Returns: 0) Reference to list of found tag keys (in order of requested tags)
///          1) Reference to list of indices for tags requested by value
///          2) Reference to list of indices for tags specified by wildcard or "all"
/// Notes: index lists are returned in increasing order
pub fn set_found_tags(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    // (local $_: the loop variables are Rust ones)
    let slf = arg(&a, 0);
    let options = slf.get("OPTIONS");
    let req_tags = {
        let r = slf.get("REQUESTED_TAGS");
        if r.truthy() {
            r
        } else {
            Value::new_array(Vec::new())
        }
    };
    let duplicates = options.get("Duplicates");
    let exclude = options.get("Exclude");
    let mut file_order = slf.get("FILE_ORDER");
    let mut group_options: Vec<Value> = Vec::new();
    // ignore empty group options
    let mut gk: Vec<Value> = options.keys().into_iter().map(Value::Str).filter(|k| re("^Group", "").is_match(k)).collect();
    gk.sort_by(ops::str_cmp);
    for k in gk {
        if options.get(k.to_bytes()).truthy() {
            group_options.push(k);
        }
    }
    let do_dups = duplicates.truthy() || exclude.truthy() || !group_options.is_empty();
    let mut tag;
    let mut rtn_tags = Value::Undef;
    let by_value = Value::new_array(Vec::new());
    let wild_tags = Value::new_array(Vec::new());

    // only return requested tags if specified
    if !req_tags.is_empty() {
        if !rtn_tags.truthy() {
            rtn_tags = Value::new_array(Vec::new());
        }
        // scan through the requested tags and generate a list of tags we found
        let mut tag_hash = slf.get("VALUE");
        for req_tag in req_tags.items() {
            let mut matches: Vec<Value> = Vec::new();
            let (mut group, mut all_grp, mut all_tag, mut by_val, mut g8) = (Value::Undef, false, false, false, Value::Undef);
            let mut et = slf.clone();
            if let Some(c) = re(r"^(.*):(.+)", "").m(&req_tag) {
                group = cap(&c, 1);
                tag = cap(&c, 2);
                if re(r"^(\*|all)$", "i").is_match(&group) {
                    all_grp = true;
                } else if let Some(c) = re(r"\bfile(\d+):", "i").m(&req_tag) {
                    g8 = pl::concat(&pl::s("File"), &cap(&c, 1));
                    let alt = slf.get("ALT_EXIFTOOL").get(g8.to_bytes());
                    et = if alt.truthy() { alt } else { slf.clone() };
                    file_order = et.get("FILE_ORDER");
                    tag_hash = et.get("VALUE");
                } else if !re(r"^[-\w:]*$", "").is_match(&group) {
                    let msg = pl::concat(&pl::concat(&pl::s("Invalid group name '"), &group), &pl::s("'"));
                    warn(it, vec![slf.clone(), msg], Want::Scalar)?;
                    group = pl::s("invalid");
                }
            } else {
                tag = req_tag.clone();
            }
            if re(r"#$", "").subst(&mut tag, false, |_| Vec::new()) > 0 && options.get("PrintConv").truthy() {
                by_val = true;
            }
            let keys: Vec<Value> = tag_hash.keys().into_iter().map(Value::Str).collect();
            if !tag_hash.get(req_tag.to_bytes()).is_undef() && !do_dups {
                matches = vec![tag.clone()];
            } else if re(r"^(\*|all)$", "i").is_match(&tag) {
                // tag name of '*' or 'all' matches all tags
                if do_dups || all_grp {
                    matches = keys.into_iter().filter(|k| !re("#", "").is_match(k)).collect();
                } else {
                    matches = keys.into_iter().filter(|k| !re(" ", "").is_match(k)).collect();
                }
                if matches.is_empty() {
                    continue; // don't want entry in list for '*' tag
                }
                all_tag = true;
            } else if re(r"[*?]", "").is_match(&tag) {
                // allow wildcards in tag names
                tr(&mut tag, b"-_A-Za-z0-9*?", b"", "dc"); // sterilize
                re(r"\*", "").subst(&mut tag, true, |_| br"[-\w]*".to_vec());
                re(r"\?", "").subst(&mut tag, true, |_| br"[-\w]".to_vec());
                if do_dups || all_grp {
                    tag = pl::concat(&tag, &pl::s(r"( \(.*)?"));
                }
                matches = grep_re(&re_full("^", &tag, "$", "i")?, &keys);
                if matches.is_empty() {
                    continue; // don't want entry in list for wildcard tags
                }
                all_tag = true;
            } else if do_dups || !group.is_undef() {
                tr(&mut tag, b"-_A-Za-z0-9", b"", "dc"); // sterilize
                // must also look for tags like "Tag (1)"
                // (but be sure not to match temporary ValueConv entries like "Tag #")
                matches = grep_re(&re_full("^", &tag, r"( \(|$)", "i")?, &keys);
            } else if re(r"^[-\w]+$", "").is_match(&tag) {
                // find first matching value
                // (use in list context to return value instead of count)
                matches = grep_re(&re_full("^", &tag, "$", "i")?, &keys).into_iter().take(1).collect();
            } else {
                let msg = pl::concat(&pl::concat(&pl::s("Invalid tag name '"), &tag), &pl::s("'"));
                warn(it, vec![slf.clone(), msg], Want::Scalar)?;
            }
            if !group.is_undef() && !all_grp {
                // keep only specified group
                matches = group_matches_v(it, &et, &group, &Value::new_array(matches), Want::List)?;
                if !(!matches.is_empty() || !all_tag) {
                    continue;
                }
            }
            if matches.len() > 1 {
                // maintain original file order for multiple tags
                matches = sort_by_file_order(&file_order, matches);
                // return only the highest priority tag unless duplicates wanted
                if !(do_dups || all_tag || all_grp) {
                    tag = matches.remove(0);
                    let p = et.get("PRIORITY").get(tag.to_bytes());
                    let mut old_priority = if p.truthy() { p } else { n(1) };
                    for m in &matches {
                        let mut priority = et.get("PRIORITY").get(m.to_bytes());
                        if priority.is_undef() {
                            priority = n(1);
                        }
                        if !pl::ge(&priority, &old_priority) {
                            continue;
                        }
                        tag = m.clone();
                        old_priority = if priority.truthy() { priority } else { n(1) };
                    }
                    matches = vec![tag.clone()];
                }
            } else if matches.is_empty() {
                // put entry in return list even without value (value is undef)
                matches = vec![pl::concat(&tag, &pl::s(if by_val { " #(0)" } else { " (0)" }))];
                // bogus file order entry to avoid warning if sorting in file order
                slf.get_or_new_hash("FILE_ORDER").set(matches[0].to_bytes(), n(999999999));
            }
            // copy over necessary information for tags from alternate files
            if g8.truthy() {
                let list = Value::new_array(matches);
                copy_alt_info(it, vec![slf.clone(), g8.clone(), list.clone()], Want::Void)?;
                matches = list.items();
                // restore variables to original values for main file
                file_order = slf.get("FILE_ORDER");
                tag_hash = slf.get("VALUE");
            }
            let base = rtn_tags.len() as i64;
            // save indices of tags extracted by value
            if by_val {
                for i in base..base + matches.len() as i64 {
                    by_value.push(n(i));
                }
            }
            // save indices of wildcard tags
            if all_tag {
                for i in base..base + matches.len() as i64 {
                    wild_tags.push(n(i));
                }
            }
            for m in matches {
                rtn_tags.push(m);
            }
        }
    } else {
        // no requested tags, so we want all tags
        let keys: Vec<Value> = slf.get("VALUE").keys().into_iter().map(Value::Str).collect();
        let all_tags: Vec<Value> = if do_dups {
            keys
        } else {
            // only include tag if it doesn't end in a copy number
            keys.into_iter().filter(|k| !re(" ", "").is_match(k)).collect()
        };
        rtn_tags = Value::new_array(all_tags);
    }

    // filter excluded tags and group options
    while (exclude.truthy() || !group_options.is_empty()) && !rtn_tags.is_empty() {
        if exclude.truthy() {
            let excl = Value::new_hash();
            for pat in exclude.items() {
                let mut group = Value::Undef;
                if let Some(c) = re(r"^(.*):(.+)", "").m(&pat) {
                    group = cap(&c, 1);
                    tag = cap(&c, 2);
                    if re(r"^(\*|all)$", "i").is_match(&group) {
                        group = Value::Undef;
                    } else if !re(r"^[-\w:]*$", "").is_match(&group) {
                        let msg = pl::concat(&pl::concat(&pl::s("Invalid group name '"), &group), &pl::s("'"));
                        warn(it, vec![slf.clone(), msg], Want::Scalar)?;
                        group = pl::s("invalid");
                    }
                } else {
                    tag = pat.clone();
                }
                let mut matches: Vec<Value>;
                if re(r"^(\*|all)$", "i").is_match(&tag) {
                    matches = rtn_tags.items();
                } else {
                    // allow wildcards in tag names
                    re(r"\*", "").subst(&mut tag, true, |_| br"[-\w]*".to_vec());
                    re(r"\?", "").subst(&mut tag, true, |_| br"[-\w]".to_vec());
                    matches = grep_re(&re_full("^", &tag, "( |$)", "i")?, &rtn_tags.items());
                }
                if group.truthy() && !matches.is_empty() {
                    matches = group_matches_v(it, &slf, &group, &Value::new_array(matches), Want::List)?;
                }
                for m in matches {
                    excl.set(m.to_bytes(), n(1));
                }
            }
            if !excl.is_empty() {
                // remove excluded tags from return list(s)
                rtn_tags = remove_tags_from_list_v(&rtn_tags, &by_value, &wild_tags, &excl, false);
                if rtn_tags.is_empty() {
                    break; // all done if nothing left
                }
            }
            if duplicates.truthy() && group_options.is_empty() {
                break;
            }
        }
        // filter groups if requested, or to remove duplicates
        let keep_tags = Value::new_hash();
        let want_group = Value::new_hash();
        let mut all_groups = n(1);
        // build hash of requested/excluded group names for each group family
        let mut want_order: i64 = 0;
        for group_opt in &group_options {
            let Some(c) = re(r"^Group(\d*(:\d+)*)", "").m(group_opt) else { continue };
            let family = if cap(&c, 1).truthy() { cap(&c, 1) } else { n(0) };
            if !want_group.get(family.to_bytes()).truthy() {
                want_group.set(family.to_bytes(), Value::new_hash());
            }
            let o = options.get(group_opt.to_bytes());
            let group_list = if ref_of(&o) == "ARRAY" { o } else { Value::new_array(vec![o]) };
            for g in group_list.items() {
                // groups have priority in order they were specified
                want_order += 1;
                let (group_name, want_v);
                if let Some(c) = re(r"^-(.*)", "").m(&g) {
                    // excluded group begins with '-'
                    group_name = cap(&c, 1);
                    want_v = n(0); // we don't want tags in this group
                } else {
                    group_name = g.clone();
                    want_v = n(want_order); // we want tags in this group
                    all_groups = n(0); // don't want all groups if we requested one
                }
                want_group.get(family.to_bytes()).set(group_name.to_bytes(), want_v);
            }
        }
        // loop through all tags and decide which ones we want
        let best_tag = Value::new_hash();
        'gr_tag: for t in rtn_tags.items() {
            tag = t;
            let mut want_tag = all_groups.clone(); // want tag by default if want all groups
            for family in want_group.keys() {
                let family = Value::Str(family);
                let group = pl::method(it, &slf, "GetGroup", vec![tag.clone(), family.clone()])?;
                let wanted = want_group.get(family.to_bytes()).get(group.to_bytes());
                if wanted.is_undef() {
                    continue;
                }
                if !wanted.truthy() {
                    continue 'gr_tag; // skip tag if group excluded
                }
                // take lowest non-zero want flag
                if want_tag.truthy() && pl::lt(&want_tag, &wanted) {
                    continue;
                }
                want_tag = wanted;
            }
            if !want_tag.truthy() {
                continue;
            }
            if duplicates.truthy() {
                keep_tags.set(tag.to_bytes(), n(1));
                continue;
            }
            // determine which tag we want to keep
            let tag_name = pl::call(it, "Image::ExifTool::GetTagName", vec![tag.clone()])?;
            let best = best_tag.get(tag_name.to_bytes());
            if !best.is_undef() {
                if pl::gt(&want_tag, &keep_tags.get(best.to_bytes())) {
                    continue;
                }
                if pl::num_eq(&want_tag, &keep_tags.get(best.to_bytes())) {
                    // want two tags with the same name -- keep the latest one
                    if let Some(c) = re(r" \((\d+)\)$", "").m(&tag) {
                        let tag_num = cap(&c, 1);
                        match re(r" \((\d+)\)$", "").m(&best) {
                            None => continue,
                            Some(c2) => {
                                if pl::gt(&cap(&c2, 1), &tag_num) {
                                    continue;
                                }
                            }
                        }
                    }
                }
                // this tag is better, so delete old best tag
                keep_tags.delete(best.to_bytes());
            }
            keep_tags.set(tag.to_bytes(), want_tag); // keep this tag (for now...)
            best_tag.set(tag_name.to_bytes(), tag.clone()); // this is our current best tag
        }
        // include only tags we want to keep in return lists
        rtn_tags = remove_tags_from_list_v(&rtn_tags, &by_value, &wild_tags, &keep_tags, true);
        break;
    }
    slf.set("FOUND_TAGS", rtn_tags.clone()); // save found tags

    // return reference to found tag keys (and list of indices of tags to extract by value)
    if want == Want::List {
        Ok(vec![rtn_tags, by_value, wild_tags])
    } else {
        ret(rtn_tags)
    }
}

pub fn register(it: &mut Interp) {
    let subs: &[(&str, Reader)] = &[
        ("Purge", purge),
        ("CombineInfo", combine_info),
        ("GetTableName", get_table_name),
        ("GetTagIndex", get_tag_index),
        ("FindValue", find_value),
        ("NextTagKey", next_tag_key),
        ("AddCleanup", add_cleanup),
        ("Warn", warn),
        ("Error", error),
        ("AddTagsToLookup", add_tags_to_lookup),
        ("ExpandFlags", expand_flags),
        ("SetupTagTable", setup_tag_table),
        ("IsPC", is_pc),
        ("IsFloat", is_float),
        ("IsInt", is_int),
        ("IsHex", is_hex),
        ("IsRational", is_rational),
        ("RoundFloat", round_float),
        ("ToFloat", to_float),
        ("SwapBytes", swap_bytes),
        ("SwapWords", swap_words),
        ("DoUnpackStd", do_unpack_std),
        ("DoUnpackRev", do_unpack_rev),
        ("DoPackStd", do_pack_std),
        ("DoPackRev", do_pack_rev),
        ("DoUnpack", do_unpack),
        ("DoUnpackDbl", do_unpack_dbl),
        ("Get8s", get8s),
        ("Get8u", get8u),
        ("Get16s", get16s),
        ("Get16u", get16u),
        ("Get32s", get32s),
        ("Get32u", get32u),
        ("GetFloat", get_float),
        ("GetDouble", get_double),
        ("Get16uRev", get16u_rev),
        ("Get32uRev", get32u_rev),
        ("GetRational32s", get_rational32s),
        ("GetRational32u", get_rational32u),
        ("GetRational64s", get_rational64s),
        ("GetRational64u", get_rational64u),
        ("GetFixed16s", get_fixed16s),
        ("GetFixed16u", get_fixed16u),
        ("GetFixed32s", get_fixed32s),
        ("GetFixed32u", get_fixed32u),
        ("Set8s", set8s),
        ("Set8u", set8u),
        ("Set16u", set16u),
        ("Set32u", set32u),
        ("Set16uRev", set16u_rev),
        ("GetByteOrder", get_byte_order),
        ("SetByteOrder", set_byte_order),
        ("ToggleByteOrder", toggle_byte_order),
        ("FormatSize", format_size),
        ("ReadValue", read_value),
        ("Decode", decode),
        ("Encode", encode),
        ("DecodeBits", decode_bits),
        ("ValidateImage", validate_image),
        ("ValidTagName", valid_tag_name),
        ("MakeTagName", make_tag_name),
        ("MakeDescription", make_description),
        ("GetDescriptions", get_descriptions),
        ("Filter", filter),
        ("Printable", printable),
        ("ConvertTimeSpan", convert_time_span),
        ("TimeLocal", time_local),
        ("GetTimeZone", get_time_zone),
        ("TimeZoneString", time_zone_string),
        ("ConvertUnixTime", convert_unix_time),
        ("GetUnixTime", get_unix_time),
        ("ConvertFileSize", convert_file_size),
        ("ConvertDuration", convert_duration),
        ("ConvertBitrate", convert_bitrate),
        ("ConvertFileName", convert_file_name),
        ("InverseFileName", inverse_file_name),
        ("LimitLongValues", limit_long_values),
        ("HDump", hdump),
        ("JpegMarkerName", jpeg_marker_name),
        ("IsSameID", is_same_id),
        ("SplitFileName", split_file_name),
        ("GroupMatches", group_matches),
        ("RemoveTagsFromList", remove_tags_from_list),
        ("CopyAltInfo", copy_alt_info),
        ("ExpandShortcuts", expand_shortcuts),
        ("AddCompositeTags", add_composite_tags),
        ("ParseArguments", parse_arguments),
        ("SetFoundTags", set_found_tags),
    ];
    for (name, f) in subs {
        it.define_native(&format!("{ET}::{name}"), *f);
    }
}
