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
    pl::sderef(r)
}

/// perl's " at FILE line LINE." for ExifTool.pm's LINE (with the ", <FH>
/// line N" part of the current location, if any), whichever native runs:
/// the subs here are also called directly by other natives, whose own file
/// is then the current location, and `pl::at` would also move their line.
fn et_suffix(it: &mut Interp, line: u32) -> String {
    let file = match it.global(b'%', "main::INC") {
        Binding::Hash(h) => h.borrow().get(b"Image/ExifTool.pm".as_slice()).map(|v| v.to_text()),
        _ => None,
    }
    .unwrap_or_else(|| "Image/ExifTool.pm".to_string());
    let cur = crate::interp::eval::location_suffix();
    let rest = match cur.find(", <") {
        Some(i) => cur[i..].to_string(),
        None => ".\n".to_string(),
    };
    format!(" at {file} line {line}{rest}")
}

/// Runs `f`; a die of its own (a message that ends with the current
/// location) is reported at ExifTool.pm line LINE, as perl reports it from
/// the statement of this sub (see et_suffix).
fn located<T>(it: &mut Interp, line: u32, f: impl FnOnce(&mut Interp) -> ER<T>) -> ER<T> {
    match f(it) {
        Err(Flow::Die(v)) if !v.is_ref() => {
            let cur = crate::interp::eval::location_suffix();
            let b = v.to_bytes();
            if b.ends_with(cur.as_bytes()) {
                let mut m = b[..b.len() - cur.len()].to_vec();
                m.extend_from_slice(et_suffix(it, line).as_bytes());
                return Err(Flow::Die(Value::bytes(m)));
            }
            Err(Flow::Die(v))
        }
        r => r,
    }
}

/// Dies as `$obj->METHOD(...)` does in perl when `$obj` is undefined or an
/// unblessed reference ("Can't call method ... on an undefined value").
/// For natives that make the call through a string eval (the RandomAccess
/// Read helpers), which would otherwise report the eval's location: call
/// it after `pl::at(LINE)`. (A local helper: pl.rs has none.)
pub(crate) fn check_invocant(it: &mut Interp, obj: &Value, method: &str) -> ER<()> {
    let unblessed = match obj {
        Value::Undef => true,
        Value::HashRef(h) => h.borrow().blessed.is_none(),
        Value::ScalarRef(_) | Value::ArrayRef(_) | Value::Code(_) | Value::LvRef(_) => true,
        _ => false,
    };
    if unblessed {
        pl::method(it, obj, method, vec![])?;
    }
    Ok(())
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
    if arg(&a, 2).truthy() {
        return located(it, 6022, |_| unpack_ctx(&skip_template(&arg(&a, 2), &fmt), &deref_scalar(&arg(&a, 1))?.to_bytes(), want));
    }
    located(it, 6023, |_| unpack_ctx(&fmt.to_bytes(), &deref_scalar(&arg(&a, 1))?.to_bytes(), want))
}

/// same, but with reversed byte order
pub fn do_unpack_rev(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let std = unpack_std(it, &arg(&a, 0));
    let fmt = my_hash(it, "unpackRev").get(std.to_bytes());
    if arg(&a, 2).truthy() {
        return located(it, 6029, |_| unpack_ctx(&skip_template(&arg(&a, 2), &fmt), &deref_scalar(&arg(&a, 1))?.to_bytes(), want));
    }
    located(it, 6030, |_| unpack_ctx(&fmt.to_bytes(), &deref_scalar(&arg(&a, 1))?.to_bytes(), want))
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
        let data = located(it, 6059, |_| deref_scalar(&data_pt))?;
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
        val = substr(&located(it, 6061, |_| deref_scalar(&data_pt))?, pos.to_i64(), Some(bytes.to_i64()));
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
    let val = substr(&located(it, 6073, |_| deref_scalar(&data_pt))?, pos.to_i64(), Some(8));
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
        let m = format!("Unknown format {}{}", format.to_text(), et_suffix(it, 6293));
        pl::warn(it, &m)?;
        len = n(1);
    }
    if size.is_undef() {
        size = pl::sub(&n(located(it, 6296, |_| deref_scalar(&data_pt))?.to_bytes().len() as i64), &offset);
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
        let mut v = substr(&located(it, 6310, |_| deref_scalar(&data_pt))?, offset.to_i64(), Some(pl::mul(&count, &len).to_i64()));
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
    let mut image = located(it, 6418, |_| deref_scalar(&image_pt))?;
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
    let old = it.swap_global(b'$', "main::_", Binding::Scalar(Rc::new(std::cell::RefCell::new(v))));
    let r = f(it);
    it.swap_global(b'$', "main::_", old);
    r
}

/// A reference to an element: `\$val[i]` / `\$val{k}`.
fn elem_ref(lv: crate::interp::eval::Lv) -> Value {
    Value::LvRef(Rc::new(lv))
}

fn filter_v(it: &mut Interp, slf: &Value, filter: &Value, val_pt: &Value) -> ER<Value> {
    // (`defined $$valPt` autovivifies an undefined $valPt; a string dies)
    if filter.is_undef() || val_pt.is_undef() || located(it, 6511, |_| deref_scalar(val_pt))?.is_undef() {
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
    if !tag_list.truthy() || located(it, 5672, |_| pl::aderef(tag_list))?.is_empty() {
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
                    matches = grep_re(&located(it, 5475, |_| re_full("^", &tag, "( |$)", "i"))?, &rtn_tags.items());
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

// ---- Perl operations run through the interpreter ----

/// Whether a variable still holds the value it was given (the same string
/// storage, number or reference), as the interpreter judges @_ aliases.
fn unchanged(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Str(x), Value::Str(y)) | (Value::UStr(x), Value::UStr(y)) => Rc::ptr_eq(x, y) || x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::UInt(x), Value::UInt(y)) => x == y,
        (Value::Num(x), Value::Num(y)) => x.to_bits() == y.to_bits(),
        (Value::Undef, Value::Undef) => true,
        _ => a.same_ref(b),
    }
}

/// Run a Perl expression for an operation the shared helpers do not provide
/// (`open`, `stat`, file tests, `print` to a handle, `pos()=`, and calls
/// that must see their arguments as aliases), with the given lexicals.
/// Returns the result and the values the variables hold afterwards. `$@`
/// is left as it was (the expression is not an `eval` in the Perl).
fn perl_op(it: &mut Interp, code: &str, vars: &[(&str, Value)], arr: Option<Vec<Value>>, want: Want) -> ER<(Vec<Value>, Vec<Value>, Vec<Value>)> {
    let scope = crate::interp::Scope::new(None);
    let cells: Vec<Cell> = vars.iter().map(|(name, v)| scope.scalar(name, v.clone())).collect();
    let a = arr.map(|v| scope.array("a", v));
    let saved = it.get_scalar("main::@");
    match it.eval_string(code.as_bytes(), ET, &scope, want) {
        Ok(r) => {
            it.set_scalar("main::@", saved);
            let vals = cells.iter().map(|c| c.borrow().clone()).collect();
            let av = a.map(|a| a.borrow().clone()).unwrap_or_default();
            Ok((r, vals, av))
        }
        Err(e) => match it.exit_code {
            Some(code) => Err(Flow::Exit(code)),
            None => Err(Flow::Die(e)),
        },
    }
}

/// `perl_op` for a scalar result.
fn perl_scalar(it: &mut Interp, code: &str, vars: &[(&str, Value)]) -> ER<Value> {
    Ok(perl_op(it, code, vars, None, Want::Scalar)?.0.pop().unwrap_or_default())
}

/// `eval { BLOCK }`: `$@` set as perl does, `None` when the block died.
fn eval_block<T>(it: &mut Interp, f: impl FnOnce(&mut Interp) -> ER<T>) -> ER<Option<T>> {
    match f(it) {
        Ok(v) => {
            it.set_scalar("main::@", pl::s(""));
            Ok(Some(v))
        }
        Err(Flow::Die(e)) => {
            it.set_scalar("main::@", e);
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// `eval { require MODULE }`
fn eval_require(it: &mut Interp, module: &str) -> ER<bool> {
    Ok(eval_block(it, |it| runtime::require(it, module))?.is_some())
}

/// `print $out STRING` (or `print STRING` with no handle)
fn print_to(it: &mut Interp, out: &Value, s: Value) -> ER<()> {
    if out.is_undef() {
        perl_op(it, "print $s", &[("s", s)], None, Want::Void)?;
    } else {
        perl_op(it, "print $out $s", &[("out", out.clone()), ("s", s)], None, Want::Void)?;
    }
    Ok(())
}

// ---- alternate files, file helpers ----

/// Finish generating tags after extracting information from a file
/// Inputs: 0) ExifTool ref
/// Notes: The sequencing here is a bit tricky because tags from the main file
///        may be used in the names of alternate files, so we finish generating
///        all main file tags first (including all Composite tags which don't
///        rely on alternate files) before extracting tags from alternate files,
///        then we finish by generating the remaingin Composite tags.
pub fn done_extract(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let slf = arg(&a, 0);
    // extract information from alternate files if necessary
    let mut opts = slf.get("OPTIONS");

    // generate ImageDataHash if requested
    if slf.get("ImageDataHash").truthy() {
        let digest = pl::method(it, &slf.get("ImageDataHash"), "hexdigest", vec![])?;
        // (don't store empty digest)
        if !(pl::str_eq(&digest, "d41d8cd98f00b204e9800998ecf8427e")
            || pl::str_eq(&digest, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
            || pl::str_eq(&digest, "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"))
        {
            pl::method(it, &slf, "FoundTag", vec![pl::s("ImageDataHash"), digest])?;
        }
    }
    // generate Validate tag if requested
    if opts.get("Validate").truthy() {
        let v = slf.get("REQ_TAG_LOOKUP").get("validate");
        pl::call(it, "Image::ExifTool::Validate::FinishValidate", vec![slf.clone(), v])?;
    }
    // generate geolocation tags if requested
    if opts.get("Geolocation").truthy() {
        let mut arg_v = Value::Undef;
        let (mut defaults, mut tags, mut coord, mut refs, mut city): (Vec<Value>, Vec<Value>, Vec<Value>, Vec<Value>, Vec<Value>) = Default::default();
        let (mut done_city, mut both) = (false, false);
        let args = pl::split(&re(r"\s*,\s*", ""), &opts.get("Geolocation"), 0);
        for mut x in args {
            if pl::str_eq(&lc(&x), "both") {
                both = true;
                continue;
            }
            if re(r"^\$", "").subst(&mut x, false, |_| Vec::new()) == 0 {
                defaults.push(x);
                continue;
            }
            tags.push(x); // argument is a tag name
        }
        if tags.is_empty() {
            // default tags to read if not specified
            tags = "GPSLatitude GPSLongitude GPSLatitudeRef GPSLongitudeRef
                       GPSCoordinates LocationShownGPSLatitude LocationShownGPSLongitude
                       XMP:City State CountryCode Country
                       IPTC:City Province-State Country-PrimaryLocationCode Country-PrimaryLocationName
                       LocationShownCity LocationShownProvinceState LocationShownCountryCode LocationShownCountryName"
                .split_whitespace()
                .map(pl::s)
                .collect();
        }
        // get information for specified tags
        let o = Value::new_hash();
        o.set("PrintConv", n(0));
        o.set("Duplicates", n(0));
        let tag_list = Value::new_array(tags.clone());
        let info = pl::method(it, &slf, "GetInfo", vec![tag_list.clone(), o])?; // (returns tags in proper case)
        tags = tag_list.items();
        opts = slf.get("OPTIONS"); // (necessary because GetInfo changes the OPTIONS hash)
        for tag in &tags {
            let val = info.get(tag.to_bytes());
            if val.is_undef() {
                continue;
            }
            pl::method(it, &slf, "VPrint", vec![n(0), cat(&[&pl::s("Found "), tag, &pl::s(" ("), &val, &pl::s(")\n")])])?;
            if re("Coordinates", "").is_match(tag) {
                if !arg(&coord, 0).is_undef() && !arg(&coord, 1).is_undef() {
                    continue;
                }
                coord = pl::split_ws(&val);
                continue;
            }
            let nn: Option<usize> = if re("Latitude", "").is_match(tag) {
                Some(0)
            } else if re("Longitude", "").is_match(tag) {
                Some(1)
            } else {
                None
            };
            if let Some(nn) = nn {
                let set = |list: &mut Vec<Value>, v: Value| {
                    if list.len() <= nn {
                        list.resize(nn + 1, Value::Undef);
                    }
                    list[nn] = v;
                };
                if re("Ref$", "").is_match(tag) {
                    if !arg(&refs, nn).truthy() {
                        set(&mut refs, val);
                    }
                } else if arg(&coord, nn).is_undef() {
                    set(&mut coord, val);
                }
                continue;
            }
            // handle city tags (save info for first city found)
            if re("City", "").is_match(tag) {
                if !city.is_empty() {
                    done_city = true;
                    continue;
                }
                city.push(val);
            } else if !city.is_empty() {
                if !done_city {
                    city.push(val);
                }
                if done_city {
                    continue;
                }
            }
        }
        if !arg(&coord, 0).is_undef() && !arg(&coord, 1).is_undef() {
            if arg(&refs, 0).truthy() && pl::gt(&coord[0], &n(0)) && pl::str_eq(&refs[0], "S") {
                coord[0] = pl::neg(&coord[0]);
            }
            if arg(&refs, 1).truthy() && pl::gt(&coord[1], &n(0)) && pl::str_eq(&refs[1], "W") {
                coord[1] = pl::neg(&coord[1]);
            }
            arg_v = join(b",", &coord);
        } else if !city.is_empty() {
            arg_v = join(b",", &city);
        }
        if arg_v.is_undef() {
            // use specified default values if no tags found
            arg_v = join(b",", &defaults);
            if pl::str_eq(&arg_v, "1") {
                arg_v = Value::Undef;
            }
            both = true; // use 'both' GPS and place names if provided
        }
        if arg_v.truthy() {
            if both {
                arg_v = pl::concat(&arg_v, &pl::s(",both"));
            }
            arg_v = encode(it, vec![slf.clone(), arg_v, pl::s("UTF8")], Want::Scalar)?.pop().unwrap_or_default();
            runtime::require(it, "Image::ExifTool::Geolocation")?;
            if opts.get("Verbose").truthy() {
                let db_info = it.get_scalar("Image::ExifTool::Geolocation::dbInfo");
                if db_info.truthy() {
                    print_to(it, &Value::Undef, cat(&[&pl::s("Loaded "), &db_info, &pl::s("\n")]))?;
                } else {
                    print_to(it, &Value::Undef, pl::s("Error loading Geolocation.dat\n"))?;
                }
            }
            with_set_warning(it, |it| {
                *pkg_scalar(it, "evalWarning").borrow_mut() = Value::Undef;
                opts.set("GeolocMulti", opts.get("Duplicates"));
                pl::method(it, &slf, "VPrint", vec![n(0), cat(&[&pl::s("Geolocation arguments: '"), &arg_v, &pl::s("'\n")])])?;
                let r = pl::call_list(it, "Image::ExifTool::Geolocation::Geolocate", vec![arg_v.clone(), opts.clone()])?;
                let (cities, dist) = (arg(&r, 0), arg(&r, 1));
                opts.delete("GeolocMulti");
                let ok = cities.truthy()
                    && (cities.len() < 2 || dist.truthy() || !warn(it, vec![slf.clone(), pl::s("Multiple Geolocation cities are possible"), n(2)], Want::Scalar)?.pop().unwrap_or_default().truthy());
                if ok {
                    if cities.len() > 1 {
                        pl::method(it, &slf, "FoundTag", vec![pl::s("GeolocationWarning"), pl::s(&format!("Search matched {} cities", cities.len()))])?;
                    }
                    for c in cities.items() {
                        if c.to_bytes() != cities.at(0).to_bytes() {
                            let dc = pl::add(&slf.get("DOC_COUNT"), &n(1));
                            slf.set("DOC_COUNT", dc.clone());
                            slf.set("DOC_NUM", dc);
                        }
                        let geo = pl::call_list(it, "Image::ExifTool::Geolocation::GetEntry", vec![c.clone(), opts.get("Lang")])?;
                        let g = |i: usize| arg(&geo, i);
                        let found = |it: &mut Interp, name: &str, v: Value| -> ER<()> {
                            pl::method(it, &slf, "FoundTag", vec![pl::s(name), v])?;
                            Ok(())
                        };
                        found(it, "GeolocationCity", g(0))?;
                        if g(1).truthy() {
                            found(it, "GeolocationRegion", g(1))?;
                        }
                        if g(2).truthy() {
                            found(it, "GeolocationSubregion", g(2))?;
                        }
                        found(it, "GeolocationCountryCode", g(3))?;
                        if g(4).truthy() {
                            found(it, "GeolocationCountry", g(4))?;
                        }
                        if g(5).truthy() {
                            found(it, "GeolocationTimeZone", g(5))?;
                        }
                        found(it, "GeolocationFeatureCode", g(6))?;
                        if g(10).truthy() {
                            found(it, "GeolocationFeatureType", g(10))?;
                        }
                        found(it, "GeolocationPopulation", g(7))?;
                        found(it, "GeolocationPosition", cat(&[&g(8), &pl::s(" "), &g(9)]))?;
                        if dist.truthy() {
                            found(it, "GeolocationDistance", dist.at(0).at(0))?;
                            found(it, "GeolocationBearing", dist.at(0).at(1))?;
                            if let Value::ArrayRef(d) = &dist {
                                if !d.borrow().is_empty() {
                                    d.borrow_mut().remove(0);
                                }
                            }
                        }
                        if !opts.get("Duplicates").truthy() {
                            break;
                        }
                    }
                    slf.delete("DOC_NUM");
                } else if pkg_scalar(it, "evalWarning").borrow().truthy() {
                    let w = pl::call(it, "Image::ExifTool::CleanWarning", vec![])?;
                    warn(it, vec![slf.clone(), w], Want::Scalar)?;
                }
                Ok(())
            })?;
        }
    }
    // generate tags for user-defined parameters that ended with '#'
    if !opts.get("UserParam").is_empty() {
        let mut do_msg = opts.get("Verbose");
        let table = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::UserParam")])?;
        let up = opts.get("UserParam");
        let mut keys: Vec<Value> = up.keys().into_iter().map(Value::Str).collect();
        keys.sort_by(ops::str_cmp);
        for k in keys {
            if !re("#$", "").is_match(&k) {
                continue;
            }
            if do_msg.truthy() {
                pl::method(it, &slf, "VPrint", vec![n(0), pl::s("UserParam tags:\n")])?;
                do_msg = Value::Undef;
            }
            pl::method(it, &slf, "HandleTag", vec![table.clone(), k.clone(), up.get(k.to_bytes())])?;
        }
    }
    if opts.get("Composite").truthy() && (!opts.get("FastScan").truthy() || pl::lt(&opts.get("FastScan"), &n(5))) {
        // build all composite tags except those requiring tags from alternate files
        pl::method(it, &slf, "BuildCompositeTags", vec![])?;
    }
    let alt = slf.get("ALT_EXIFTOOL");
    let mut g8s: Vec<Value> = alt.keys().into_iter().map(Value::Str).collect();
    g8s.sort_by(ops::str_cmp);
    for g8 in g8s {
        let alt_exif_tool = slf.get("ALT_EXIFTOOL").get(g8.to_bytes());
        if alt_exif_tool.get("DID_EXTRACT").truthy() {
            continue; // avoid extracting twice
        }
        alt_exif_tool.set("OPTIONS", slf.get("OPTIONS"));
        alt_exif_tool.set("GLOBAL_TIME_OFFSET", slf.get("GLOBAL_TIME_OFFSET"));
        alt_exif_tool.set("REQ_TAG_LOOKUP", slf.get("REQ_TAG_LOOKUP"));
        alt_exif_tool.set("ReqTagAlreadySet", n(1));
        let mut file_name = alt_exif_tool.get("ALT_FILE");
        // allow tags from the main file to be used in the alternate file names
        // (eg. -file1 '$originalfilename')
        if re(r"\$", "").is_match(&file_name) {
            let mut tags: Vec<Value> = slf.get("VALUE").keys().into_iter().map(Value::Str).collect();
            tags.sort_by(ops::str_cmp);
            tags.reverse();
            file_name = pl::method(it, &slf, "InsertTagValues", vec![file_name, Value::new_array(tags), pl::s("Warn")])?;
            if file_name.is_undef() {
                continue;
            }
        }
        pl::method(it, &alt_exif_tool, "ExtractInfo", vec![file_name.clone()])?;
        let err = alt_exif_tool.get("VALUE").get("Error");
        if err.truthy() {
            warn(it, vec![slf.clone(), cat(&[&err, &pl::s(" \""), &file_name, &pl::s("\"")])], Want::Scalar)?;
        }
        // set family 8 group name for all tags
        for k in alt_exif_tool.get("VALUE").keys() {
            alt_exif_tool.get_or_new_hash("TAG_EXTRA").get_or_new_hash(&k).set("G8", g8.clone());
        }
        // prepare our sorted list of found tags
        let found = set_found_tags(it, vec![alt_exif_tool.clone()], Want::Scalar)?.pop().unwrap_or_default();
        alt_exif_tool.set("FoundTags", found);
        alt_exif_tool.set("DID_EXTRACT", n(1));
    }
    // if necessary, build composite tags that rely on tags from alternate files
    if slf.get("DoAltComposite").truthy() {
        return pl::method_list(it, &slf, "BuildCompositeTags", vec![n(1)]).map(|v| if want == Want::List { v } else { vec![v.into_iter().last().unwrap_or_default()] });
    }
    pl::ret_nothing(want)
}

/// IsUTF8 with the string position it leaves (`pos($$strPt)`).
fn is_utf8_v(s: &[u8], trunc: bool) -> (i64, Option<usize>) {
    let high = crate::perl::regex::compile(br"([\x80-\xff])", "").unwrap();
    let find_high = |from: usize| -> Option<(u8, usize)> {
        match high.match_at(s, from) {
            Ok(Some(sp)) => sp[0].map(|(a, e)| (s[a], e)),
            _ => None,
        }
    };
    // pos($$strPt) = 0; # start at beginning of string
    let Some((mut ch, mut pos)) = find_high(0) else { return (0, None) };
    let mut rtn_val = 1;
    loop {
        // minimum lead byte for 2-byte sequence is 0xc2 (overlong sequences
        // not allowed), 0xf8-0xfd are restricted by RFC 3629 (no 5 or 6 byte
        // sequences), and 0xfe and 0xff are not valid in UTF-8 strings
        if ch < 0xc2 || ch >= 0xf8 {
            return (-1, Some(pos));
        }
        // determine number of bytes remaining in sequence
        let n: usize = if ch < 0xe0 {
            1
        } else if ch < 0xf0 {
            2
        } else {
            // character code is greater than 0xffff if more than 2 extra bytes
            // were required in the UTF-8 character
            rtn_val = 2;
            3
        };
        let cont = crate::perl::regex::compile(format!(r"\G([\x80-\xbf]{{{n}}})").as_bytes(), "").unwrap();
        let m = match cont.match_at(s, pos) {
            Ok(Some(sp)) => sp[1],
            _ => None,
        };
        let Some((a, e)) = m else {
            if trunc && pos + n > s.len() {
                return (rtn_val, None);
            }
            return (-1, None);
        };
        pos = e;
        let c1 = &s[a..e];
        // the following is ref https://www.cl.cam.ac.uk/%7Emgk25/ucs/utf8_check.c
        if n == 2 {
            if (ch == 0xe0 && (c1[0] & 0xe0) == 0x80) || (ch == 0xed && (c1[0] & 0xe0) == 0xa0) || (ch == 0xef && c1[0] == 0xbf && (c1[1] & 0xfe) == 0xbe) {
                return (-1, Some(pos));
            }
        } else if (ch == 0xf0 && (c1[0] & 0xf0) == 0x80) || (ch == 0xf4 && c1[0] > 0x8f) || ch > 0xf4 {
            return (-1, Some(pos));
        }
        match find_high(pos) {
            Some((c, p)) => {
                ch = c;
                pos = p;
            }
            None => break,
        }
    }
    (rtn_val, None)
}

/// Does a string contain valid UTF-8 characters?
/// Inputs: 0) string reference, 1) true to allow last character to be truncated
/// Returns: 0=regular ASCII, -1=invalid UTF-8, 1=valid UTF-8 with maximum 16-bit
///          wide characters, 2=valid UTF-8 requiring 32-bit wide characters
/// Notes: Changes current string position
/// (see http://www.fileformat.info/info/unicode/utf8.htm for help understanding this)
pub fn is_utf8(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (str_pt, trunc) = (arg(&a, 0), arg(&a, 1));
    // (`pos($$strPt) = 0` autovivifies an undefined $strPt: no high
    // characters then; a string dies)
    if str_pt.is_undef() {
        return ret(n(0));
    }
    let s = located(it, 4677, |_| deref_scalar(&str_pt))?;
    let (r, pos) = is_utf8_v(&s.to_bytes(), trunc.truthy());
    // (a failed //g match resets the position)
    let p = pos.map(|p| n(p as i64)).unwrap_or_default();
    perl_op(it, "pos($$p) = $n", &[("p", str_pt), ("n", p)], None, Want::Void)?;
    ret(n(r))
}

/// EncodeFileName: returns the flag and the new value for `$_[1]`, if any.
fn encode_file_name_v(it: &mut Interp, slf: &Value, file: &Value, force: bool) -> ER<(i64, Option<Value>)> {
    let mut file = file.clone();
    if pl::str_eq(&file, "-") {
        return Ok((0, None)); // special case for stdin pipe
    }
    let mut enc = slf.path(&["OPTIONS", "CharsetFileName"]);
    let mut has_special_chars = false;
    if re(r"[\x80-\xff]", "").is_match(&file) {
        has_special_chars = true;
        if !enc.truthy() && pl::str_eq(&os_name(it), "MSWin32") {
            if is_utf8_v(&file.to_bytes(), false).0 < 0 {
                if enc.is_undef() {
                    warn(it, vec![slf.clone(), pl::s("FileName encoding must be specified")], Want::Scalar)?;
                }
                return Ok((0, None));
            } else {
                enc = pl::s("UTF8"); // assume UTF8
            }
        }
    }
    if has_special_chars || force || slf.path(&["OPTIONS", "WindowsLongPath"]).truthy() || slf.path(&["OPTIONS", "WindowsWideFile"]).truthy() {
        if !enc.truthy() {
            enc = pl::s("UTF8");
        }
        if pl::str_eq(&os_name(it), "MSWin32") {
            let r = with_set_warning(it, |it| {
                if eval_require(it, "Win32API::File")? {
                    if slf.path(&["OPTIONS", "WindowsLongPath"]).truthy() {
                        file = windows_long_path(it, vec![slf.clone(), file.clone()], Want::Scalar)?.pop().unwrap_or_default();
                    }
                    // recode as UTF-16LE and add null terminator
                    let d = decode(it, vec![slf.clone(), file.clone(), enc.clone(), Value::Undef, pl::s("UTF16"), pl::s("II")], Want::Scalar)?.pop().unwrap_or_default();
                    return Ok(Some(pl::concat(&d, &pl::s("\0\0"))));
                }
                Ok(None)
            })?;
            if let Some(v) = r {
                return Ok((1, Some(v)));
            }
            warn(it, vec![slf.clone(), pl::s("Install Win32API::File for Windows wide/long file name support")], Want::Scalar)?;
        } else if !pl::str_eq(&enc, "UTF8") {
            // recode as UTF-8 for other platforms if necessary
            let d = decode(it, vec![slf.clone(), file.clone(), enc.clone(), Value::Undef, pl::s("UTF8")], Want::Scalar)?.pop().unwrap_or_default();
            return Ok((0, Some(d)));
        }
    }
    Ok((0, None))
}

/// Encode file name for calls to system i/o routines
/// Inputs: 0) ExifTool ref, 1) file name in CharsetFileName encoding,
///         2) flag to force conversion even if no special characters
/// Returns: true if Windows Unicode routines should be used (in which case
///          the file name will be encoded as a null-terminated UTF-16LE string)
pub fn encode_file_name(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (r, new) = encode_file_name_v(it, &arg(&a, 0), &arg(&a, 1), arg(&a, 2).truthy())?;
    if let Some(v) = new {
        it.set_arg(1, v)?;
    }
    ret(n(r))
}

/// Rebuild a path as an absolute long path to be usable in Windows system calls
/// Inputs: 0) ExifTool ref, 1) path string (CharsetFileName)
/// Returns: normalized long path (CharsetFileName)
/// Note: this should only be called for Windows systems
pub fn windows_long_path(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (slf, mut path) = (arg(&a, 0), arg(&a, 1));
    let debug = slf.path(&["OPTIONS", "Debug"]);
    let out = slf.path(&["OPTIONS", "TextOut"]);
    let mut suffix = pl::s("");
    let mut long_path;

    // remove common suffixes to make cache more effective
    let mut c1 = Value::Undef;
    if re(r"(_original|_exiftool_tmp|:Zone\.Identifier)$", "").subst(&mut path, false, |c| {
        c1 = cap(c, 1);
        Vec::new()
    }) > 0
    {
        suffix = c1;
        if path.to_bytes().is_empty() || re(r"[:./\\]$", "").is_match(&path) {
            // don't remove suffix if it could be the whole file name
            path = pl::concat(&path, &suffix);
            suffix = pl::s("");
        }
    }
    if !slf.get("LONG_PATH_IN").is_undef() && slf.get("LONG_PATH_IN").to_bytes() == path.to_bytes() {
        return ret(pl::concat(&slf.get("LONG_PATH_OUT"), &suffix));
    }

    if debug.truthy() {
        print_to(it, &out, cat(&[&pl::s("WindowsLongPath input : "), &path, &suffix, &pl::s("\n")]))?;
    }

    let k32 = my_scalar(it, "k32GetFullPathName");
    loop {
        // (cheap goto)
        long_path = path.clone();
        tr(&mut long_path, b"/", b"\\\\", ""); // convert slashes to backslashes
        if re(r"^\\\\\?\\", "").is_match(&long_path) {
            break; // already a device path in the format we want
        }

        if !k32.borrow().truthy() {
            // need to import (once) GetFullPathNameW
            if !k32.borrow().is_undef() {
                break;
            }
            if !eval_require(it, "Win32::API")? {
                warn(it, vec![slf.clone(), pl::s("Install Win32::API to use WindowsLongPath option")], Want::Scalar)?;
                break;
            }
            let f = pl::method(it, &pl::s("Win32::API"), "new", vec![pl::s("KERNEL32"), pl::s("GetFullPathNameW"), pl::s("PNPP"), pl::s("I")])?;
            *k32.borrow_mut() = f;
            if !k32.borrow().truthy() {
                *k32.borrow_mut() = n(0);
                warn(it, vec![slf.clone(), pl::s("Error loading Win32::API GetFullPathNameW")], Want::Scalar)?;
                break;
            }
        }
        let e = slf.path(&["OPTIONS", "CharsetFileName"]);
        let enc = if e.truthy() { e } else { pl::s("UTF8") };
        let enc_path = decode(it, vec![slf.clone(), long_path.clone(), enc.clone(), Value::Undef, pl::s("UTF16"), pl::s("II")], Want::Scalar)?.pop().unwrap_or_default(); // need to encode to UTF16
        let k = k32.borrow().clone();
        let len_req = pl::add(&pl::method(it, &k, "Call", vec![enc_path.clone(), n(0), n(0), n(0)])?, &n(1)); // first pass gets length required, +1 for safety (null?)
        let full_path = Value::bytes(vec![0u8; (len_req.to_i64().max(0) * 2) as usize]); // create buffer to hold full path
        let (_, vals, _) = perl_op(it, "$k->Call($e, $l, $f, 0)", &[("k", k), ("e", enc_path), ("l", len_req), ("f", full_path)], None, Want::Void)?; // fullPath is UTF16 now
        let full_path = vals[3].clone();
        long_path = decode(it, vec![slf.clone(), full_path, pl::s("UTF16"), pl::s("II"), enc], Want::Scalar)?.pop().unwrap_or_default();

        if (long_path.to_bytes().len() as i64) <= 247 - suffix.to_bytes().len() as i64 {
            break;
        }

        if re(r"^\\\\", "").is_match(&long_path) {
            long_path = pl::concat(&pl::s(r"\\?\UNC"), &substr(&long_path, 1, None));
        } else {
            long_path = pl::concat(&pl::s(r"\\?\"), &long_path);
        }
        break;
    }
    // this may be called repeatedly for the same file file (exists, stat, open),
    // so cache the last return value (without any of the suffixes that we use)
    slf.set("LONG_PATH_IN", path);
    slf.set("LONG_PATH_OUT", long_path.clone());
    if debug.truthy() {
        print_to(it, &out, cat(&[&pl::s("WindowsLongPath return: "), &long_path, &suffix, &pl::s("\n")]))?;
    }
    ret(pl::concat(&long_path, &suffix))
}

/// `Win32API::File::NAME()`
fn win32(it: &mut Interp, name: &str) -> ER<Value> {
    pl::call(it, &format!("Win32API::File::{name}"), vec![])
}

/// Modified perl open() routine to properly handle special characters in file names
/// Inputs: 0) ExifTool ref, 1) filehandle, 2) filename,
///         3) mode: '<' or undef = read, '>' = write, '+<' = update
/// Returns: true on success
/// Note: Must call like "$et->Open(\*FH,$file)", not "$et->Open(FH,$file)" to avoid
///       "unopened filehandle" errors due to a change in scope of the filehandle
pub fn open(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (slf, fh, mut file, mut mode) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));

    re(r"^([\s&])", "").subst(&mut file, false, |c| [&b"./"[..], &cap(c, 1).to_bytes()].concat()); // protect leading whitespace or ampersand
    // default to read mode ('<') unless input is a trusted pipe
    if !mode.truthy() {
        mode = pl::s(if re(r"\|$", "").is_match(&file) && slf.get("TRUST_PIPE").truthy() { "" } else { "<" });
    }
    slf.delete("TRUST_PIPE");
    if mode.truthy() {
        let (wide, new) = encode_file_name_v(it, &slf, &file, false)?;
        if let Some(v) = new {
            file = v;
        }
        if wide != 0 {
            // handle Windows Unicode file name
            let r = with_set_warning(it, |it| -> ER<Option<Value>> {
                let (mut access, mut create) = (Value::Undef, Value::Undef);
                if pl::str_eq(&mode, ">") || pl::str_eq(&mode, ">>") {
                    eval_block(it, |it| {
                        access = win32(it, "GENERIC_WRITE")?;
                        if pl::str_eq(&mode, ">>") {
                            access = ops::bit_or(&access, &win32(it, "FILE_APPEND_DATA")?);
                            create = win32(it, "OPEN_ALWAYS")?;
                        } else {
                            create = win32(it, "CREATE_ALWAYS")?;
                        }
                        Ok(())
                    })?;
                } else {
                    eval_block(it, |it| {
                        access = win32(it, "GENERIC_READ")?;
                        if pl::str_eq(&mode, "+<") {
                            access = ops::bit_or(&access, &win32(it, "GENERIC_WRITE")?); // update
                        }
                        create = win32(it, "OPEN_EXISTING")?;
                        Ok(())
                    })?;
                }
                let mut share = n(0);
                eval_block(it, |it| {
                    if !ops::bit_and(&access, &win32(it, "GENERIC_WRITE")?).truthy() {
                        share = ops::bit_or(&win32(it, "FILE_SHARE_READ")?, &win32(it, "FILE_SHARE_WRITE")?);
                    }
                    Ok(())
                })?;
                let wh = eval_block(it, |it| pl::call(it, "Win32API::File::CreateFileW", vec![file.clone(), access.clone(), share.clone(), Value::new_array(vec![]), create.clone(), n(0), Value::new_array(vec![])]))?.unwrap_or_default();
                if !wh.truthy() {
                    return Ok(None);
                }
                let fd = eval_block(it, |it| pl::call(it, "Win32API::File::OsFHandleOpenFd", vec![wh.clone(), n(0)]))?.unwrap_or_default();
                if fd.is_undef() || pl::lt(&fd, &n(0)) {
                    eval_block(it, |it| pl::call(it, "Win32API::File::CloseHandle", vec![wh.clone()]))?;
                    return Ok(None);
                }
                Ok(Some(pl::concat(&pl::s("&="), &fd))) // specify file by descriptor
            })?;
            match r {
                Some(f) => file = f,
                None => return pl::ret_undef(),
            }
        } else {
            // add leading space to protect against leading characters like '>'
            // in file name, and trailing "\0" to protect trailing spaces
            file = cat(&[&pl::s(" "), &file, &pl::s("\0")]);
        }
    }
    // (open sets our copy of the handle variable, not the caller's)
    let (r, _, _) = perl_op(it, "open $fh, $spec", &[("fh", fh), ("spec", pl::concat(&mode, &file))], None, Want::Scalar)?;
    ret(r.into_iter().last().unwrap_or_default())
}

/// Check to see if a file exists (with Windows Unicode support)
/// Inputs: 0) ExifTool ref, 1) file name, 2) flag if we are writing this file
/// Returns: true if file exists
pub fn exists(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (slf, mut file, writing) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));

    let (wide, new) = encode_file_name_v(it, &slf, &file, false)?;
    if let Some(v) = new {
        file = v;
    }
    if wide != 0 {
        let wh = with_set_warning(it, |it| {
            let wh = eval_block(it, |it| {
                let args = vec![file.clone(), win32(it, "GENERIC_READ")?, win32(it, "FILE_SHARE_READ")?, Value::new_array(vec![]), win32(it, "OPEN_EXISTING")?, n(0), Value::new_array(vec![])];
                pl::call(it, "Win32API::File::CreateFileW", args)
            })?
            .unwrap_or_default();
            if wh.truthy() {
                eval_block(it, |it| pl::call(it, "Win32API::File::CloseHandle", vec![wh.clone()]))?;
            }
            Ok(wh)
        })?;
        if !wh.truthy() {
            return ret(n(0));
        }
    } else if writing.truthy() {
        // (named pipes already exist, but we pretend that they don't
        //  so we will be able to write them, so test for pipe with -p)
        return ret(perl_scalar(it, "-e $file and not -p $file", &[("file", file)])?);
    } else {
        return ret(perl_scalar(it, "-e $file", &[("file", file)])?);
    }
    ret(n(1))
}

fn is_directory_v(it: &mut Interp, et: &Value, file: &Value) -> ER<Value> {
    let mut file = file.clone();
    let (wide, new) = encode_file_name_v(it, et, &file, false)?;
    if let Some(v) = new {
        file = v;
    }
    if wide != 0 {
        let yes = with_set_warning(it, |it| {
            let attrs = eval_block(it, |it| pl::call(it, "Win32API::File::GetFileAttributesW", vec![file.clone()]))?.unwrap_or_default();
            let d = eval_block(it, |it| win32(it, "FILE_ATTRIBUTE_DIRECTORY"))?.unwrap_or_default();
            let dir_bit = if d.truthy() { d } else { n(0) };
            Ok(attrs.truthy() && !pl::num_eq(&attrs, &Value::Int(0xffffffff)) && ops::bit_and(&attrs, &dir_bit).truthy())
        })?;
        if yes {
            return Ok(n(1));
        }
    } else {
        return perl_scalar(it, "-d $file", &[("file", file)]);
    }
    Ok(n(0))
}

/// Return true if file is a directory (with Windows Unicode support)
/// Inputs: 0) ExifTool ref, 1) file name
/// Returns: true if file is a directory (false if file isn't, or doesn't exist)
pub fn is_directory(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(is_directory_v(it, &arg(&a, 0), &arg(&a, 1))?)
}

/// Create directory for specified file
/// Inputs: 0) ExifTool ref, 1) complete file name including path
/// Returns: '' = directory created, undef = nothing done, otherwise error string
pub fn create_directory(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    // (local $_: the loop variable is a Rust one)
    let (slf, file) = (arg(&a, 0), arg(&a, 1));
    let mut err = Value::Undef;
    let mut dir = file.clone();
    re(r"[^\/]*$", "").subst(&mut dir, false, |_| Vec::new()); // remove filename from path specification
    if dir.truthy() && !is_directory_v(it, &slf, &dir)?.truthy() {
        let parts = pl::split(&re(r"\/", ""), &dir, 0);
        dir = pl::s("");
        let k32 = my_scalar(it, "k32CreateDir");
        for part in parts {
            dir = pl::concat(&dir, &part);
            let pc = is_pc(it, vec![], Want::Scalar)?.pop().unwrap_or_default();
            if !part.to_bytes().is_empty() && !is_directory_v(it, &slf, &dir)?.truthy() &&
                // don't try to create a network drive root directory
                !(pc.truthy() && re(r"^//[^/]*$", "").is_match(&dir))
            {
                let success;
                // create directory since it doesn't exist
                let mut d2 = dir.clone(); // (must make a copy in case EncodeFileName recodes it)
                let (wide, new) = encode_file_name_v(it, &slf, &d2, false)?;
                if let Some(v) = new {
                    d2 = v;
                }
                if wide != 0 {
                    // handle Windows Unicode directory names
                    if k32.borrow().is_undef() {
                        if !eval_require(it, "Win32::API")? {
                            err = pl::s("Install Win32::API to create directories with Unicode names");
                            break;
                        }
                        let f = pl::method(it, &pl::s("Win32::API"), "new", vec![pl::s("KERNEL32"), pl::s("CreateDirectoryW"), pl::s("PP"), pl::s("I")])?;
                        *k32.borrow_mut() = f;
                        if !k32.borrow().truthy() {
                            *k32.borrow_mut() = n(0);
                            // give this error once, then just "Error creating" for subsequent attempts
                            return ret(pl::s("Error loading Win32::API CreateDirectoryW"));
                        }
                    }
                    let k = k32.borrow().clone();
                    success = if k.truthy() { pl::method(it, &k, "Call", vec![d2, n(0)])? } else { Value::Undef };
                } else {
                    success = perl_scalar(it, "mkdir($d2, 0777)", &[("d2", d2)])?;
                }
                if !success.truthy() {
                    err = pl::concat(&pl::s("Error creating directory "), &dir);
                    break;
                }
                err = pl::s("");
            }
            dir = pl::concat(&dir, &pl::s("/"));
        }
    }
    ret(err)
}

/// `(stat $file)[8, 9, 10]`
fn stat_times(it: &mut Interp, file: &Value) -> ER<Vec<Value>> {
    Ok(perl_op(it, "(stat $file)[8, 9, 10]", &[("file", file.clone())], None, Want::List)?.0)
}

/// Get file times (Unix seconds since the epoch)
/// Inputs: 0) ExifTool ref, 1) file name or ref
/// Returns: 0) access time, 1) modification time, 2) creation time (or undefs on error)
pub fn get_file_time(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (slf, mut file) = (arg(&a, 0), arg(&a, 1));

    // open file by name if necessary
    if !file.is_ref() {
        // (local *FH: the glob is a name here)
        let opened = perl_scalar(it, "$self->Open(\\*FH, $file)", &[("self", slf.clone()), ("file", file.clone())])?;
        if !opened.truthy() {
            if is_directory_v(it, &slf, &file)?.truthy() {
                let rtn = stat_times(it, &file)?;
                if !arg(&rtn, 0).is_undef() {
                    return Ok(rtn);
                }
            }
            warn(it, vec![slf.clone(), cat(&[&pl::s("GetFileTime error for '"), &file, &pl::s("'")])], Want::Scalar)?;
            return pl::ret_nothing(want);
        }
        file = perl_scalar(it, "*FH", &[])?; // (not \*FH, so *FH will be kept open until $file goes out of scope)
    }
    // on Windows, try to work around incorrect file times when daylight saving time is in effect
    if pl::str_eq(&os_name(it), "MSWin32") {
        if !eval_require(it, "Win32::API")? {
            warn(it, vec![slf.clone(), pl::s("Install Win32::API for proper handling of Windows file times"), n(1)], Want::Scalar)?;
        } else if !eval_require(it, "Win32API::File")? {
            warn(it, vec![slf.clone(), pl::s("Install Win32API::File for proper handling of Windows file times"), n(1)], Want::Scalar)?;
        } else {
            // get Win32 handle, needed for GetFileTime
            let win32_handle = eval_block(it, |it| pl::call(it, "Win32API::File::GetOsFHandle", vec![file.clone()]))?.unwrap_or_default();
            if !win32_handle.truthy() {
                warn(it, vec![slf.clone(), pl::s("Win32API::File::GetOsFHandle returned invalid handle")], Want::Scalar)?;
                return pl::ret_nothing(want);
            }
            // get FILETIME structs
            let zero = pack_v(b"LL", &[n(0), n(0)])?;
            let k32 = my_scalar(it, "k32GetFileTime");
            if !k32.borrow().truthy() {
                if !k32.borrow().is_undef() {
                    return pl::ret_nothing(want);
                }
                let f = pl::method(it, &pl::s("Win32::API"), "new", vec![pl::s("KERNEL32"), pl::s("GetFileTime"), pl::s("NPPP"), pl::s("I")])?;
                *k32.borrow_mut() = f;
                if !k32.borrow().truthy() {
                    warn(it, vec![slf.clone(), pl::s("Error loading Win32::API GetFileTime")], Want::Scalar)?;
                    *k32.borrow_mut() = n(0);
                    return pl::ret_nothing(want);
                }
            }
            let k = k32.borrow().clone();
            let (r, vals, _) = perl_op(it, "$k->Call($h, $ctime, $atime, $mtime)", &[("k", k), ("h", win32_handle), ("ctime", zero.clone()), ("atime", zero.clone()), ("mtime", zero)], None, Want::Scalar)?;
            if !r.last().is_some_and(|v| v.truthy()) {
                let e = pl::call(it, "Win32::GetLastError", vec![])?;
                warn(it, vec![slf.clone(), pl::concat(&pl::s("Win32::API GetFileTime returned "), &e)], Want::Scalar)?;
                return pl::ret_nothing(want);
            }
            // convert FILETIME structs to Unix seconds
            let mut times = Vec::new();
            for time in [&vals[3], &vals[4], &vals[2]] {
                let lh = match pack::unpack(b"LL", &time.to_bytes()) {
                    Ok(v) => v,
                    Err(e) => return die(e),
                }; // unpack FILETIME struct
                let (lo, hi) = (arg(&lh, 0), arg(&lh, 1));
                // FILETIME is in 100 ns intervals since 0:00 UTC Jan 1, 1601
                // (89 leap years between 1601 and 1970)
                times.push(pl::sub(&pl::mul(&pl::add(&pl::mul(&hi, &n(4294967296)), &lo), &Value::Num(1e-7)), &n(((1970 - 1601) * 365 + 89) * 24 * 3600)));
            }
            return Ok(times);
        }
    }
    // other os (or Windows fallback)
    stat_times(it, &file)
}

// ---- autoloading ----

/// DoAutoLoad: the result, and the final values of the arguments (written
/// back to the caller's variables, as @_ aliases them).
/// (`caller` is " at FILE line N\n" of DoAutoLoad's call, for caller(0))
fn do_auto_load_v(it: &mut Interp, autoload: &Value, args: Vec<Value>, want: Want, caller: String) -> ER<(Vec<Value>, Vec<Value>)> {
    let call_info = pl::split(&re("::", ""), autoload, 0);
    let mut file = pl::s("Image/ExifTool/Write");

    if call_info.last().is_some_and(|v| pl::str_eq(v, "DESTROY")) {
        return Ok((pl::ret_nothing(want)?, args));
    }
    if call_info.len() == 4 {
        // load Image/ExifTool/WriteMODULE.pl
        file = cat(&[&file, &call_info[2], &pl::s(".pl")]);
    } else if call_info.last().is_some_and(|v| pl::str_eq(v, "ShiftTime")) {
        file = pl::s("Image/ExifTool/Shift.pl"); // load Shift.pl
    } else {
        // load Image/ExifTool/Writer.pl
        file = pl::concat(&file, &pl::s("r.pl"));
    }
    // attempt to load the package
    if !eval_require(it, &file.to_text())? {
        let e = it.get_scalar("main::@");
        return Err(Flow::Die(cat(&[&pl::s("Error while attempting to call "), autoload, &pl::s("\n"), &e, &pl::s("\n")])));
    }
    let name = autoload.to_text();
    if it.sub(&name).is_none() {
        // reproduce Perl's standard 'undefined subroutine' message:
        return Err(Flow::Die(Value::from(format!("Undefined subroutine {name} called{caller}"))));
    }
    // call the function (with @_ aliased to our arguments)
    let (r, _, after) = perl_op(it, "no strict 'refs'; &$f(@a)", &[("f", autoload.clone())], Some(args), want)?;
    Ok((r, after))
}

/// Write back the arguments a call changed (`$_[i]` aliases).
fn write_back(it: &mut Interp, before: &[Value], after: &[Value], offset: usize) -> ER<()> {
    for (i, (b, a)) in before.iter().zip(after.iter()).enumerate() {
        if !unchanged(a, b) {
            it.set_arg(i + offset, a.clone())?;
        }
    }
    Ok(())
}

/// Utility to load our write routines if required (called via AUTOLOAD)
/// Inputs: 0) autoload function, 1-N) function arguments
/// Returns: result of function or dies if function not available
pub fn do_auto_load(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let mut a = a;
    let autoload = if a.is_empty() { Value::Undef } else { a.remove(0) };
    let caller = crate::interp::eval::caller_at(0);
    let (r, after) = do_auto_load_v(it, &autoload, a.clone(), want, caller)?;
    write_back(it, &a, &after, 1)?;
    Ok(r)
}

/// AutoLoad our writer routines when necessary
pub fn autoload(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let name = pkg_scalar(it, "AUTOLOAD").borrow().clone();
    // (DoAutoLoad's caller is this sub's statement)
    pl::at(5597);
    let (r, after) = do_auto_load_v(it, &name, a.clone(), want, crate::interp::eval::here_at())?;
    write_back(it, &a, &after, 0)?;
    Ok(r)
}

// ---- dates, trailers ----

/// `Image::ExifTool::ShiftTime($val, $shift, $dir, $offset)` (ShiftTime
/// updates its first argument): returns the shifted value.
fn shift_time(it: &mut Interp, val: &Value, shift: &Value, dir: &Value, offset: &Value) -> ER<Value> {
    let vars = [("v", val.clone()), ("s", shift.clone()), ("d", dir.clone()), ("o", offset.clone())];
    let (_, vals, _) = perl_op(it, "Image::ExifTool::ShiftTime($v, $s, $d, $o)", &vars, None, Want::Void)?;
    Ok(vals[0].clone())
}

/// `$fmt =~ s/(^|[^%])((%%)*)CODE/$1$2$rep/g`
fn subst_code(fmt: &mut Value, code: &str, rep: &Value) -> ER<()> {
    let r = pl::re_dyn(format!("(^|[^%])((%%)*){code}").as_bytes(), "")?;
    let rb = rep.to_bytes();
    r.subst(fmt, true, |c| [&cap(c, 1).to_bytes()[..], &cap(c, 2).to_bytes()[..], &rb[..]].concat());
    Ok(())
}

/// Convert date/time from Exif format
/// Inputs: 0) ExifTool object reference, 1) Date/time in EXIF format
/// Returns: Formatted date/time string
pub fn convert_date_time(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (slf, mut date) = (arg(&a, 0), arg(&a, 1));
    let mut fmt = slf.path(&["OPTIONS", "DateFormat"]);
    let mut shift = slf.path(&["OPTIONS", "GlobalTimeShift"]);
    if shift.truthy() {
        let mut offset = slf.get("GLOBAL_TIME_OFFSET");
        let (mut g, mut t, dir);
        let mut matches: Vec<Value> = Vec::new();
        let mut caps = None;
        re(r"^((\d?[A-Z][-\w]*\w:)*)([A-Z][-\w]*\w)([-+])", "i").subst(&mut shift, false, |c| {
            caps = Some(c.clone());
            Vec::new()
        });
        if let Some(c) = caps {
            g = cap(&c, 1);
            t = cap(&c, 3);
            dir = n(if pl::str_eq(&cap(&c, 4), "-") { -1 } else { 1 });
        } else {
            g = Value::Undef;
            t = Value::Undef;
            let mut c1 = Value::Undef;
            let m = re(r"^([-+])", "").subst(&mut shift, false, |c| {
                c1 = cap(c, 1);
                Vec::new()
            }) > 0;
            dir = n(if m && pl::str_eq(&c1, "-") { -1 } else { 1 });
        }
        if !offset.truthy() {
            offset = Value::new_hash();
            slf.set("GLOBAL_TIME_OFFSET", offset.clone());
            // (see forum16692 for a discussion about why this code was added)
            if t.truthy() {
                // determine initial shift from specified tag
                let keys: Vec<Value> = slf.get("VALUE").keys().into_iter().map(Value::Str).collect();
                matches = if keys.is_empty() { Vec::new() } else { grep_re(&located(it, 6593, |_| re_full("^", &t, r"( \(|$)", "i"))?, &keys) };
                matches.sort_by(ops::str_cmp);
                if g.truthy() && !matches.is_empty() {
                    re(":$", "").subst(&mut g, false, |_| Vec::new());
                    matches = group_matches_v(it, &slf, &g, &Value::new_array(matches), Want::List)?;
                }
            }
            if matches.is_empty() && slf.get("TAGS_FROM_FILE").truthy() && slf.path(&["OPTIONS", "RequestTags"]).truthy() {
                // determine initial shift from first requested date/time tag
                let mut req_date = grep_re(&re("date", "i"), &slf.path(&["OPTIONS", "RequestTags"]).items());
                while !req_date.is_empty() {
                    t = req_date.remove(0);
                    let keys: Vec<Value> = slf.get("VALUE").keys().into_iter().map(Value::Str).collect();
                    matches = if keys.is_empty() { Vec::new() } else { grep_re(&located(it, 6604, |_| re_full("^", &t, r"( \(|$)", "i"))?, &keys) };
                    matches.sort_by(ops::str_cmp);
                    let ti = slf.get("TAG_INFO");
                    while !matches.is_empty() {
                        // select the first tag that calls this routine in its PrintConv
                        let pc = ti.get(matches[0].to_bytes()).get("PrintConv");
                        if pc.truthy() && re("ConvertDateTime", "").is_match(&pc) {
                            req_date.clear();
                            break;
                        }
                        matches.remove(0);
                    }
                }
            }
            if !matches.is_empty() {
                let val = pl::method(it, &slf, "GetValue", vec![matches[0].clone(), pl::s("ValueConv")])?;
                if !val.is_undef() {
                    shift_time(it, &val, &shift, &dir, &offset)?;
                }
            }
        }
        date = shift_time(it, &date, &shift, &dir, &offset)?;
    }
    // only convert date if a format was specified and the date is recognizable
    if fmt.truthy() {
        // separate time zone if it exists
        let mut tz = Value::Undef;
        let mut c1 = Value::Undef;
        if re(r"([-+]\d{2}:\d{2}|Z)$", "").subst(&mut date, false, |c| {
            c1 = cap(c, 1);
            Vec::new()
        }) > 0
        {
            tz = c1;
        }
        // a few cameras use incorrect date/time formatting:
        // - slashes instead of colons in date (RolleiD330, ImpressCam)
        // - date/time values separated by colon instead of space (Polariod, Sanyo, Sharp, Vivitar)
        // - single-digit seconds with leading space (HP scanners)
        let mut a = re(r"\d+", "").all(&date); // be very flexible about date/time format
        a.reverse();
        if !a.is_empty() && pl::ge(a.last().unwrap(), &n(1000)) && pl::lt(a.last().unwrap(), &n(3000)) && eval_require(it, "POSIX")? {
            while a.len() > 6 {
                a.remove(0); // remove superfluous entries
            }
            while a.len() < 3 {
                a.insert(0, n(1)); // add month and day if necessary
            }
            while a.len() < 6 {
                a.insert(0, n(0)); // add h,m,s if necessary
            }
            a[4] = pl::sub(&a[4], &n(1)); // base month is 1
            // parse our %f fractional seconds first (and round up seconds if necessary)
            // - if there are multiple %f codes, they all get the same number of digits as the first
            if let Some(c) = re(r"%(-?)\.?(\d*)f", "").m(&fmt) {
                let (neg, dig) = (cap(&c, 1), cap(&c, 2));
                let mut frac = match re(r"(\.\d+)", "").m(&date) {
                    Some(c) => cap(&c, 1),
                    None => pl::s(""),
                };
                let flen = |f: &Value| n(f.to_bytes().len() as i64);
                if !frac.truthy() {
                    if dig.truthy() {
                        frac = pl::concat(&pl::s("."), &Value::bytes(b"0".repeat(dig.to_i64().max(0) as usize)));
                    }
                } else if !dig.to_bytes().is_empty() {
                    let d1 = pl::add(&dig, &n(1));
                    if pl::gt(&d1, &flen(&frac)) {
                        let k = pl::sub(&d1, &flen(&frac)).to_i64().max(0) as usize;
                        frac = pl::concat(&frac, &Value::bytes(b"0".repeat(k)));
                    } else if pl::lt(&d1, &flen(&frac)) {
                        frac = pl::sprintf_v(&cat(&[&pl::s("%."), &dig, &pl::s("f")]), &[frac]);
                        loop {
                            let mut c1 = Value::Undef;
                            if !(re(r"^(\d)", "").subst(&mut frac, false, |c| {
                                c1 = cap(c, 1);
                                Vec::new()
                            }) > 0
                                && !pl::str_eq(&c1, "0"))
                            {
                                break;
                            }
                            // this is a pain, but we must round up to the next second
                            a[0] = pl::add(&a[0], &n(1));
                            if pl::lt(&a[0], &n(60)) {
                                break;
                            }
                            a[0] = n(0);
                            a[1] = pl::add(&a[1], &n(1));
                            if pl::lt(&a[1], &n(60)) {
                                break;
                            }
                            a[1] = n(0);
                            a[2] = pl::add(&a[2], &n(1));
                            if pl::lt(&a[2], &n(24)) {
                                break;
                            }
                            a[2] = n(0);
                            runtime::require(it, "Image/ExifTool/Shift.pl")?;
                            a[3] = pl::add(&a[3], &n(1));
                            let dim = pl::call(it, "Image::ExifTool::DaysInMonth", vec![pl::add(&a[4], &n(1)), a[5].clone()])?;
                            if pl::le(&a[3], &dim) {
                                break;
                            }
                            a[3] = n(1);
                            a[4] = pl::add(&a[4], &n(1));
                            if pl::lt(&a[4], &n(12)) {
                                break;
                            }
                            a[4] = n(0);
                            a[5] = pl::add(&a[5], &n(1));
                            break; // (this was a goto)
                        }
                    }
                }
                if neg.truthy() {
                    re(r"^\.", "").subst(&mut frac, false, |_| Vec::new());
                }
                subst_code(&mut fmt, r"%-?\.?\d*f", &frac)?;
            }
            // parse %z and %s ourself (to handle time zones properly)
            if re(r"%:?[sz]", "").is_match(&fmt) {
                // use system time zone unless otherwise specified
                if !tz.truthy() && eval_require(it, "Time::Local")? {
                    let tl = time_local_v(it, a.clone())?;
                    tz = time_zone_string_v(it, Value::new_array(a.clone()), tl)?;
                }
                // remove colon, setting to UTC if time zone is not numeric
                if !(tz.truthy() && re(r"^[-+]\d{2}:\d{2}$", "").is_match(&tz)) {
                    tz = pl::s("+00:00");
                }
                subst_code(&mut fmt, "%:z", &tz)?; // convert '%:z' format codes
                re(":", "").subst(&mut tz, false, |_| Vec::new());
                subst_code(&mut fmt, "%z", &tz)?; // convert '%z' format codes
                if re("%s", "").is_match(&fmt) && eval_require(it, "Time::Local")? {
                    // calculate seconds since the Epoch, UTC
                    let gm = pl::call(it, "Time::Local::timegm", a.clone())?;
                    let s = pl::sub(&gm, &pl::mul(&n(60), &pl::sub(&tz, &pl::mul(&pl::int(&pl::div(&tz, &n(100))?), &n(40)))));
                    subst_code(&mut fmt, "%s", &s)?; // convert '%s' format codes
                }
            }
            a[5] = pl::sub(&a[5], &n(1900)); // strftime year starts from 1900
            let mut args = vec![fmt.clone()];
            args.extend(a);
            date = pl::call(it, "POSIX::strftime", args)?; // generate the formatted date/time
            // apparently strftime can set the UTF-8 flag (argh!), so reset this if necessary
            if re(r"[\x80-\xff]", "").is_match(&fmt) {
                let r = Value::ScalarRef(Rc::new(std::cell::RefCell::new(date.clone())));
                pl::method(it, &slf, "Sanitize", vec![r.clone()])?;
                date = r.deref();
            }
        } else if slf.path(&["OPTIONS", "StrictDate"]).truthy() {
            date = Value::Undef;
        }
    }
    ret(date)
}

/// `$raf->Read($buff, $len)`: the count and the bytes read.
fn raf_read(it: &mut Interp, raf: &Value, len: &Value) -> ER<(Value, Value)> {
    let (r, vals, _) = perl_op(it, "$raf->Read($buff, $len)", &[("raf", raf.clone()), ("buff", Value::Undef), ("len", len.clone())], None, Want::Scalar)?;
    Ok((r.into_iter().last().unwrap_or_default(), vals[1].clone()))
}

fn identify_trailer_v(it: &mut Interp, slf: &Value, raf: &Value, offset: &Value) -> ER<Value> {
    let offset = if offset.truthy() { offset.clone() } else { n(0) };
    let pos = pl::method(it, raf, "Tell", vec![])?;
    let mut typ = Value::Undef;
    while pl::method(it, raf, "Seek", vec![pl::neg(&offset), n(2)])?.truthy() {
        let mut len = pl::method(it, raf, "Tell", vec![])?;
        if !pl::gt(&len, &n(0)) {
            break;
        }
        // read up to 64 bytes before specified offset from end of file
        if pl::gt(&len, &n(64)) {
            len = n(64);
        }
        if !pl::method(it, raf, "Seek", vec![pl::neg(&len), n(1)])?.truthy() {
            break;
        }
        let (got, buff) = raf_read(it, raf, &len)?;
        if !pl::num_eq(&got, &len) {
            break;
        }
        let m = |p: &'static str| re(p, "s").is_match(&buff);
        let t = if re(r"AXS(!|\*).{8}$", "s").is_match(&buff) {
            "AFCP"
        } else if re(r"\xa1\xb2\xc3\xd4$", "").is_match(&buff) {
            "FotoStation"
        } else if re(r"cbipcbbl$", "").is_match(&buff) {
            "PhotoMechanic"
        } else if re(r"^CANON OPTIONAL DATA\0", "").is_match(&buff) {
            "CanonVRD"
        } else if m(r"~\0\x04\0zmie~\0\0\x06.{4}[\x10\x18]\x04$") || m(r"~\0\x04\0zmie~\0\0\x0a.{8}[\x10\x18]\x08$") {
            "MIE"
        } else if re(r"\0\0(QDIOBS|SEFT)$", "").is_match(&buff) {
            "Samsung"
        } else if m(r"8db42d694ccc418790edff439fe026bf$") {
            "Insta360"
        } else if re(r"\0{6}/NIKON APP$", "").is_match(&buff) {
            "NikonApp"
        } else if re(r"\xff{4}\x1b\*9HWfu\x84\x93\xa2\xb1$", "").is_match(&buff) {
            "Vivo"
        } else if m(r"jxrs...\0$") {
            "OnePlus"
        } else if slf.get("ProcessGoogleTrailer").truthy() {
            // check for Google trailer information if specific XMP tags exist
            "Google"
        } else {
            ""
        };
        if !t.is_empty() {
            typ = pl::s(t);
        }
        break;
    }
    pl::method(it, raf, "Seek", vec![pos, n(0)])?; // restore original file position
    if typ.truthy() {
        let h = Value::new_hash();
        h.set("RAF", raf.clone());
        h.set("DirName", typ);
        return Ok(h);
    }
    Ok(Value::Undef)
}

/// Identify trailer ending at specified offset from end of file
/// Inputs: 0) RAF reference, 1) offset from end of file (0 by default)
/// Returns: Trailer info hash (with RAF and DirName set),
///          or undef if no recognized trailer was found
/// Notes: leaves file position unchanged
pub fn identify_trailer(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(identify_trailer_v(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?)
}

/// Read/rewrite trailer information (including multiple trailers)
/// Inputs: 0) ExifTool object ref, 1) DirInfo ref:
/// - requires RAF and DirName
/// - OutFile is a scalar reference for writing
/// - scans from current file position for each trailer if ScanForTrailer is set
///   (current file position is just after JPEG EOF for a JPEG image)
/// Returns: 1 if trailer was processed or couldn't be processed (or written OK)
///          0 if trailer was recognized but offsets need fixing (or write error)
/// - DirName, DirLen, DataPos, Offset, Fixup and OutFile are updated
/// - preserves current file position and byte order
pub fn process_trailers(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (slf, dir_info) = (arg(&a, 0), arg(&a, 1));
    let mut dir_name = dir_info.get("DirName");
    let outfile = dir_info.get("OutFile");
    let mut offset = {
        let o = dir_info.get("Offset");
        if o.truthy() {
            o
        } else {
            n(0)
        }
    };
    let mut fixup = dir_info.get("Fixup");
    let raf = dir_info.get("RAF");
    let pos = pl::method(it, &raf, "Tell", vec![])?;
    let byte_order = get_byte_order(it, vec![], Want::Scalar)?.pop().unwrap_or_default();
    let mut success = n(1);
    let path = slf.get("PATH");

    // get position of end of file
    pl::method(it, &raf, "Seek", vec![n(0), n(2)])?;
    let end = pl::method(it, &raf, "Tell", vec![])?;
    slf.set("FileEnd", end);

    loop {
        // loop through all trailers
        pl::method(it, &raf, "Seek", vec![pos.clone()])?;
        let out_buff: Cell = Rc::new(std::cell::RefCell::new(Value::Undef));
        // trailer-processing procs residing in modules of a different name
        let mut module = pl::s(match &*dir_name.to_bytes() {
            b"Insta360" => "QuickTimeStream.pl",
            b"NikonApp" => "Nikon.pm",
            b"Vivo" => "Trailer.pm",
            b"OnePlus" => "Trailer.pm",
            b"Google" => "Trailer.pm",
            _ => "",
        });
        if !module.truthy() {
            module = pl::concat(&dir_name, &pl::s(".pm"));
        }
        runtime::require(it, &format!("Image/ExifTool/{}", module.to_text()))?;
        re(r"(Stream)?\..*", "").subst(&mut module, false, |_| Vec::new()); // remove extension and change QuickTimeStream to QuickTime
        let proc_ = format!("Image::ExifTool::{}::Process{}", module.to_text(), dir_name.to_text());
        if outfile.truthy() {
            // write to local buffer so we can add trailer in proper order later
            if outfile.deref().truthy() {
                dir_info.set("OutFile", Value::ScalarRef(out_buff.clone()));
                *out_buff.borrow_mut() = pl::s("");
            }
            // must generate new fixup if necessary so we can shift
            // the old fixup separately after we prepend this trailer
            dir_info.delete("Fixup");
        }
        dir_info.delete("DirLen"); // reset trailer length
        dir_info.set("Offset", offset.clone()); // set offset from end of file
        dir_info.set("Trailer", n(1)); // set Trailer flag in case proc cares
        // add trailer and DirName to SubDirectory PATH
        path.push(pl::s("Trailer"));
        path.push(dir_name.clone());
        //
        // Call proc to read or write this trailer (see the Perl for the
        // inputs and returns of the procs)
        //
        let mut result = pl::call(it, &proc_, vec![slf.clone(), dir_info.clone()])?;

        // restore PATH (pop last 2 items)
        if let Value::ArrayRef(p) = &path {
            let mut p = p.borrow_mut();
            let k = p.len().saturating_sub(2);
            p.truncate(k);
        }

        let (mut data_pos, mut dir_len) = (dir_info.get("DataPos"), dir_info.get("DirLen"));
        if outfile.truthy() {
            if pl::lt(&result, &n(0)) {
                // copy or delete the trailer ourself
                result = n(1);
                if slf.get("TrailerStart").truthy() {
                    if !data_pos.truthy() {
                        data_pos = slf.get("TrailerStart");
                    }
                    if !dir_len.truthy() {
                        dir_len = pl::sub(&pl::sub(&slf.get("FileEnd"), &offset), &data_pos);
                    }
                }
                if slf.path(&["DEL_GROUP", "Trailer"]).truthy() || slf.get("DEL_GROUP").get(dir_name.to_bytes()).truthy() {
                    let bytes = if dir_len.truthy() { cat(&[&pl::s(" ("), &dir_len, &pl::s(" bytes)")]) } else { pl::s("") };
                    pl::method(it, &slf, "VPrint", vec![n(0), cat(&[&pl::s("Deleting "), &dir_name, &pl::s(" trailer"), &bytes, &pl::s("\n")])])?;
                    slf.set("CHANGED", pl::add(&slf.get("CHANGED"), &n(1)));
                } else if data_pos.truthy() && dir_len.truthy() {
                    pl::method(it, &slf, "VPrint", vec![n(0), cat(&[&pl::s("Copying "), &dir_name, &pl::s(" trailer ("), &dir_len, &pl::s(" bytes)\n")])])?;
                    let ok = pl::method(it, &raf, "Seek", vec![data_pos.clone()])?.truthy() && {
                        let (r, vals, _) = perl_op(it, "$raf->Read($$out, $len)", &[("raf", raf.clone()), ("out", dir_info.get("OutFile")), ("len", dir_len.clone())], None, Want::Scalar)?;
                        let _ = vals;
                        pl::num_eq(&r.into_iter().last().unwrap_or_default(), &dir_len)
                    };
                    if !ok {
                        result = n(0);
                    }
                } else {
                    result = n(0);
                }
            }
            if pl::gt(&result, &n(0)) {
                let ob = out_buff.borrow().clone();
                if ob.truthy() {
                    // write trailers to OutFile in original order
                    assign_deref(&outfile, pl::concat(&ob, &outfile.deref()))?;
                    // must adjust old fixup start if it exists
                    if fixup.truthy() {
                        fixup.set("Start", pl::add(&fixup.get("Start"), &n(ob.to_bytes().len() as i64)));
                    }
                    *out_buff.borrow_mut() = pl::s(""); // free memory
                }
                if dir_info.get("Fixup").truthy() {
                    if fixup.truthy() {
                        // add fixup for subsequent trailers to the fixup for this trailer
                        // (but first we must adjust for the new start position)
                        let start = dir_info.get("Fixup").get("Start");
                        fixup.set("Shift", pl::add(&fixup.get("Shift"), &start));
                        fixup.set("Start", pl::sub(&fixup.get("Start"), &start));
                        pl::method(it, &dir_info.get("Fixup"), "AddFixup", vec![fixup.clone()])?;
                    }
                    fixup = dir_info.get("Fixup"); // save fixup
                }
            } else {
                let msg = cat(&[&pl::s("Error rewriting "), &dir_name, &pl::s(" trailer")]);
                if error(it, vec![slf.clone(), msg, n(2)], Want::Scalar)?.pop().unwrap_or_default().truthy() {
                    success = n(0);
                }
                break;
            }
        } else if pl::lt(&result, &n(0)) {
            // can't continue if we must scan for this trailer
            success = n(0);
            break;
        }
        if !(pl::gt(&result, &n(0)) && dir_len.truthy()) {
            break;
        }
        offset = pl::add(&offset, &dir_len);
        if data_pos.truthy() && slf.get("TrailerStart").truthy() && pl::le(&data_pos, &slf.get("TrailerStart")) {
            break;
        }
        // look for next trailer
        let mut next_trail = identify_trailer_v(it, &slf, &raf, &offset)?;
        // process Google trailer after all others if necessary and not done already
        if !next_trail.truthy() {
            if !slf.get("ProcessGoogleTrailer").truthy() {
                break;
            }
            next_trail = Value::new_hash();
            next_trail.set("DirName", pl::s("Google"));
            next_trail.set("RAF", raf.clone());
        }
        dir_name = next_trail.get("DirName");
        dir_info.set("DirName", dir_name.clone());
    }
    set_byte_order(it, vec![byte_order], Want::Scalar)?; // restore original byte order
    pl::method(it, &raf, "Seek", vec![pos])?; // restore original file position
    dir_info.set("OutFile", outfile); // restore original outfile
    dir_info.set("Offset", offset); // return offset from EOF to start of first trailer
    dir_info.set("Fixup", fixup); // return fixup information
    ret(success)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Purge", purge);
    it.define_native("Image::ExifTool::CombineInfo", combine_info);
    it.define_native("Image::ExifTool::GetTableName", get_table_name);
    it.define_native("Image::ExifTool::GetTagIndex", get_tag_index);
    it.define_native("Image::ExifTool::FindValue", find_value);
    it.define_native("Image::ExifTool::NextTagKey", next_tag_key);
    it.define_native("Image::ExifTool::AddCleanup", add_cleanup);
    it.define_native("Image::ExifTool::Warn", warn);
    it.define_native("Image::ExifTool::Error", error);
    it.define_native("Image::ExifTool::AddTagsToLookup", add_tags_to_lookup);
    it.define_native("Image::ExifTool::ExpandFlags", expand_flags);
    it.define_native("Image::ExifTool::SetupTagTable", setup_tag_table);
    it.define_native("Image::ExifTool::IsPC", is_pc);
    it.define_native("Image::ExifTool::IsFloat", is_float);
    it.define_native("Image::ExifTool::IsInt", is_int);
    it.define_native("Image::ExifTool::IsHex", is_hex);
    it.define_native("Image::ExifTool::IsRational", is_rational);
    it.define_native("Image::ExifTool::RoundFloat", round_float);
    it.define_native("Image::ExifTool::ToFloat", to_float);
    it.define_native("Image::ExifTool::SwapBytes", swap_bytes);
    it.define_native("Image::ExifTool::SwapWords", swap_words);
    it.define_native("Image::ExifTool::DoUnpackStd", do_unpack_std);
    it.define_native("Image::ExifTool::DoUnpackRev", do_unpack_rev);
    it.define_native("Image::ExifTool::DoPackStd", do_pack_std);
    it.define_native("Image::ExifTool::DoPackRev", do_pack_rev);
    it.define_native("Image::ExifTool::DoUnpack", do_unpack);
    it.define_native("Image::ExifTool::DoUnpackDbl", do_unpack_dbl);
    it.define_native("Image::ExifTool::Get8s", get8s);
    it.define_native("Image::ExifTool::Get8u", get8u);
    it.define_native("Image::ExifTool::Get16s", get16s);
    it.define_native("Image::ExifTool::Get16u", get16u);
    it.define_native("Image::ExifTool::Get32s", get32s);
    it.define_native("Image::ExifTool::Get32u", get32u);
    it.define_native("Image::ExifTool::GetFloat", get_float);
    it.define_native("Image::ExifTool::GetDouble", get_double);
    it.define_native("Image::ExifTool::Get16uRev", get16u_rev);
    it.define_native("Image::ExifTool::Get32uRev", get32u_rev);
    it.define_native("Image::ExifTool::GetRational32s", get_rational32s);
    it.define_native("Image::ExifTool::GetRational32u", get_rational32u);
    it.define_native("Image::ExifTool::GetRational64s", get_rational64s);
    it.define_native("Image::ExifTool::GetRational64u", get_rational64u);
    it.define_native("Image::ExifTool::GetFixed16s", get_fixed16s);
    it.define_native("Image::ExifTool::GetFixed16u", get_fixed16u);
    it.define_native("Image::ExifTool::GetFixed32s", get_fixed32s);
    it.define_native("Image::ExifTool::GetFixed32u", get_fixed32u);
    it.define_native("Image::ExifTool::Set8s", set8s);
    it.define_native("Image::ExifTool::Set8u", set8u);
    it.define_native("Image::ExifTool::Set16u", set16u);
    it.define_native("Image::ExifTool::Set32u", set32u);
    it.define_native("Image::ExifTool::Set16uRev", set16u_rev);
    it.define_native("Image::ExifTool::GetByteOrder", get_byte_order);
    it.define_native("Image::ExifTool::SetByteOrder", set_byte_order);
    it.define_native("Image::ExifTool::ToggleByteOrder", toggle_byte_order);
    it.define_native("Image::ExifTool::FormatSize", format_size);
    it.define_native("Image::ExifTool::ReadValue", read_value);
    it.define_native("Image::ExifTool::Decode", decode);
    it.define_native("Image::ExifTool::Encode", encode);
    it.define_native("Image::ExifTool::DecodeBits", decode_bits);
    it.define_native("Image::ExifTool::ValidateImage", validate_image);
    it.define_native("Image::ExifTool::ValidTagName", valid_tag_name);
    it.define_native("Image::ExifTool::MakeTagName", make_tag_name);
    it.define_native("Image::ExifTool::MakeDescription", make_description);
    it.define_native("Image::ExifTool::GetDescriptions", get_descriptions);
    it.define_native("Image::ExifTool::Filter", filter);
    it.define_native("Image::ExifTool::Printable", printable);
    it.define_native("Image::ExifTool::ConvertTimeSpan", convert_time_span);
    it.define_native("Image::ExifTool::TimeLocal", time_local);
    it.define_native("Image::ExifTool::GetTimeZone", get_time_zone);
    it.define_native("Image::ExifTool::TimeZoneString", time_zone_string);
    it.define_native("Image::ExifTool::ConvertUnixTime", convert_unix_time);
    it.define_native("Image::ExifTool::GetUnixTime", get_unix_time);
    it.define_native("Image::ExifTool::ConvertFileSize", convert_file_size);
    it.define_native("Image::ExifTool::ConvertDuration", convert_duration);
    it.define_native("Image::ExifTool::ConvertBitrate", convert_bitrate);
    it.define_native("Image::ExifTool::ConvertFileName", convert_file_name);
    it.define_native("Image::ExifTool::InverseFileName", inverse_file_name);
    it.define_native("Image::ExifTool::LimitLongValues", limit_long_values);
    it.define_native("Image::ExifTool::HDump", hdump);
    it.define_native("Image::ExifTool::JpegMarkerName", jpeg_marker_name);
    it.define_native("Image::ExifTool::IsSameID", is_same_id);
    it.define_native("Image::ExifTool::SplitFileName", split_file_name);
    it.define_native("Image::ExifTool::GroupMatches", group_matches);
    it.define_native("Image::ExifTool::RemoveTagsFromList", remove_tags_from_list);
    it.define_native("Image::ExifTool::CopyAltInfo", copy_alt_info);
    it.define_native("Image::ExifTool::ExpandShortcuts", expand_shortcuts);
    it.define_native("Image::ExifTool::AddCompositeTags", add_composite_tags);
    it.define_native("Image::ExifTool::ParseArguments", parse_arguments);
    it.define_native("Image::ExifTool::SetFoundTags", set_found_tags);
    it.define_native("Image::ExifTool::DoneExtract", done_extract);
    it.define_native("Image::ExifTool::IsUTF8", is_utf8);
    it.define_native("Image::ExifTool::EncodeFileName", encode_file_name);
    it.define_native("Image::ExifTool::WindowsLongPath", windows_long_path);
    it.define_native("Image::ExifTool::Open", open);
    it.define_native("Image::ExifTool::Exists", exists);
    it.define_native("Image::ExifTool::IsDirectory", is_directory);
    it.define_native("Image::ExifTool::CreateDirectory", create_directory);
    it.define_native("Image::ExifTool::GetFileTime", get_file_time);
    it.define_native("Image::ExifTool::DoAutoLoad", do_auto_load);
    it.define_native("Image::ExifTool::AUTOLOAD", autoload);
    it.define_native("Image::ExifTool::ConvertDateTime", convert_date_time);
    it.define_native("Image::ExifTool::IdentifyTrailer", identify_trailer);
    it.define_native("Image::ExifTool::ProcessTrailers", process_trailers);
}
