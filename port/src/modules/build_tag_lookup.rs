//! Image::ExifTool::BuildTagLookup (Image/ExifTool/BuildTagLookup.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Utility to build tag lookup tables in Image::ExifTool::TagLookup.pm, and
//! the TagNames documentation. The `%docs` and other tables stay the
//! module's Perl data; these are its twelve subs.

use std::cmp::Ordering;

use crate::interp::eval::{die, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, perl_eval};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, n, re, ret, s};
use crate::runtime;

const MODULE: &str = "Image::ExifTool::BuildTagLookup";
const ET: &str = "Image::ExifTool";

// ---- local helpers ----

/// A file lexical scalar of BuildTagLookup.pm (`my $numbersFirst`...).
fn my_scalar(it: &mut Interp, name: &str) -> Value {
    runtime::lex_scalar(it, MODULE, name).borrow().clone()
}

fn set_my_scalar(it: &mut Interp, name: &str, v: Value) {
    *runtime::lex_scalar(it, MODULE, name).borrow_mut() = v;
}

/// A file lexical hash of BuildTagLookup.pm (`my %docs`...).
fn my_hash(it: &mut Interp, name: &str) -> Value {
    runtime::lex_hash(it, MODULE, name)
}

/// A file lexical array of BuildTagLookup.pm (`my @pluginTables`).
fn my_array(it: &mut Interp, name: &str) -> Value {
    runtime::lex_array(it, MODULE, name)
}

/// `keys %$h` as values.
fn keys(h: &Value) -> Vec<Value> {
    h.keys().into_iter().map(Value::Str).collect()
}

/// `keys %$h` where perl dies when `$h` is not a hash (an undefined `$h`
/// becomes an empty hash, as perl autovivifies it).
fn hkeys(h: &Value) -> ER<Vec<Value>> {
    match h {
        Value::HashRef(_) => Ok(keys(h)),
        Value::Undef => Ok(Vec::new()),
        v if v.is_ref() => die("Not a HASH reference"),
        v => die(format!("Can't use string (\"{}\") as a HASH ref while \"strict refs\" in use", v.to_text())),
    }
}

/// `sort keys %$h`
fn sorted_keys(h: &Value) -> Vec<Value> {
    let mut k = keys(h);
    k.sort_by(ops::str_cmp);
    k
}

/// `sort { $a <=> $b } LIST`
fn sort_num(mut v: Vec<Value>) -> Vec<Value> {
    v.sort_by(|x, y| ops::num_cmp(x, y).unwrap_or(Ordering::Equal));
    v
}

/// `$a eq $b`
fn eq(a: &Value, b: &Value) -> bool {
    a.to_bytes()[..] == b.to_bytes()[..]
}

/// Concatenate several values (`"$a$b$c"`).
fn cat(parts: &[&Value]) -> Value {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(&p.to_bytes());
    }
    Value::bytes(v)
}

/// `length $v` as a number.
fn len(v: &Value) -> i64 {
    v.to_bytes().len() as i64
}

/// `$$h{KEY}` as the base of a longer element access (`$$h{KEY}{...}`):
/// perl creates the inner hash when it is undefined, even to read it.
fn viv(h: &Value, key: impl AsRef<[u8]>) -> Value {
    if h.is_hash() {
        h.get_or_new_hash(key)
    } else {
        Value::Undef
    }
}

/// `ref $v`
fn rtype(v: &Value) -> String {
    pl::ref_of(v)
}

/// `/^PAT$/`-like patterns with an interpolated part (no quotemeta).
fn dyn_re(parts: &[&[u8]], flags: &str) -> ER<pl::Re> {
    pl::re_dyn(&parts.concat(), flags)
}

/// `$s =~ s/RE//` (first match, removed)
fn strip(v: &mut Value, pat: &str, flags: &str) -> usize {
    re(pat, flags).subst(v, false, |_| Vec::new())
}

/// `hex($v)` (a leading "0x" is allowed, as in perl).
fn hex(v: &Value) -> Value {
    let b = v.to_bytes();
    let mut t: &[u8] = &b;
    if t.len() >= 2 && t[0] == b'0' && (t[1] == b'x' || t[1] == b'X') {
        t = &t[2..];
    }
    let digits: Vec<u8> = t.iter().copied().take_while(|c| c.is_ascii_hexdigit() || *c == b'_').filter(|c| *c != b'_').collect();
    let txt = String::from_utf8_lossy(&digits);
    match u64::from_str_radix(if txt.is_empty() { "0" } else { &txt }, 16) {
        Ok(u) => ops::from_uv(u),
        Err(_) => Value::Num(txt.chars().fold(0f64, |f, c| f * 16.0 + c.to_digit(16).unwrap_or(0) as f64)),
    }
}

/// A Perl statement on bareword filehandles or the file system (`open`,
/// `print HTMLFILE`, `close`, `rename`, backticks...) with `$f`/`$g` bound,
/// in this package.
fn sn(it: &mut Interp, code: &str, f: &Value, g: &Value) -> ER<Vec<Value>> {
    let (cf, cg) = (cell(f.clone()), cell(g.clone()));
    perl_eval(it, code, MODULE, &[("f", &cf), ("g", &cg)], Want::List)
}

/// `sn` in scalar context.
fn sn1(it: &mut Interp, code: &str, f: &Value, g: &Value) -> ER<Value> {
    let (cf, cg) = (cell(f.clone()), cell(g.clone()));
    Ok(perl_eval(it, code, MODULE, &[("f", &cf), ("g", &cg)], Want::Scalar)?.pop().unwrap_or_default())
}

/// `print HTMLFILE LIST`: true on success.
fn hp(it: &mut Interp, v: Value) -> ER<bool> {
    Ok(sn1(it, "print HTMLFILE $f", &v, &Value::Undef)?.truthy())
}

/// `print PODFILE LIST`: true on success.
fn pp(it: &mut Interp, v: Value) -> ER<bool> {
    Ok(sn1(it, "print PODFILE $f", &v, &Value::Undef)?.truthy())
}

/// `close NAME`: true on success.
fn close(it: &mut Interp, handle: &str) -> ER<bool> {
    Ok(sn1(it, &format!("close {handle}"), &Value::Undef, &Value::Undef)?.truthy())
}

/// `<NAME>` in scalar context: the next line, or undef.
fn read_line(it: &mut Interp, handle: &str) -> ER<Value> {
    sn1(it, &format!("my $l = <{handle}>; $l"), &Value::Undef, &Value::Undef)
}

/// `EscapeHTML($v)`
fn escape_html(it: &mut Interp, v: &Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::HTML::EscapeHTML", vec![v.clone()])
}

/// `$et->GetGroup($tagInfo)` in list context.
fn get_group(it: &mut Interp, et: &Value, tag_info: &Value) -> ER<Vec<Value>> {
    pl::method_list(it, et, "GetGroup", vec![tag_info.clone()])
}

/// `GetTagInfoList($table, $id)`
fn get_tag_info_list(it: &mut Interp, table: &Value, id: &Value) -> ER<Vec<Value>> {
    pl::call_list(it, "Image::ExifTool::GetTagInfoList", vec![table.clone(), id.clone()])
}

/// `GetTagTable($name)`
fn get_tag_table(it: &mut Interp, name: &Value) -> ER<Value> {
    pl::call(it, "Image::ExifTool::GetTagTable", vec![name.clone()])
}

/// `sprintf` with a format built at run time (`sprintf "  %-${w}s", ...`).
fn sprintf_dyn(fmt: &str, args: &[Value]) -> Value {
    pl::sprintf_v(&s(fmt), args)
}

/// `$$h{KEY} & MASK` (numeric bit and).
fn has_bits(v: &Value, mask: i64) -> bool {
    ops::bit_and(v, &n(mask)).truthy()
}

/// `$ref->[i]` for an array element that may be missing.
fn at(v: &[Value], i: usize) -> Value {
    v.get(i).cloned().unwrap_or_default()
}

/// The ordering of a sort comparator's result (`<=>`, `cmp`, a sub).
fn ord_of(v: &Value) -> Ordering {
    v.to_i64().cmp(&0)
}

fn cmp_val(o: Ordering) -> Value {
    n(match o {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    })
}

/// `sort { COMPARATOR } LIST` with a comparator that may die: the first
/// error ends the sort and is returned.
fn sort_try(it: &mut Interp, list: &mut [Value], mut f: impl FnMut(&mut Interp, &Value, &Value) -> ER<Value>) -> ER<()> {
    let mut err = None;
    list.sort_by(|x, y| {
        if err.is_some() {
            return Ordering::Equal;
        }
        match f(it, x, y) {
            Ok(v) => ord_of(&v),
            Err(e) => {
                err = Some(e);
                Ordering::Equal
            }
        }
    });
    match err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// `sort { NumbersFirst($a,$b) } LIST`
fn sort_nf(it: &mut Interp, list: &mut [Value]) {
    let nf = my_scalar(it, "numbersFirst");
    let ci = my_scalar(it, "caseInsensitive").truthy();
    list.sort_by(|x, y| ord_of(&numbers_first_rs(x, y, &nf, ci)));
}

/// `eval $code` for the hash of a DecodeBits PrintConv (`\%Some::hash` or
/// `{ 0 => 'a', ... }`). A string starting with `{ WORD =>` is an
/// anonymous hash for perl, which the interpreter would parse as a block:
/// it is evaluated as `+{...}`, the expression perl sees.
fn eval_bits(it: &mut Interp, code: &Value, self_: &Value) -> Value {
    let mut code = code.clone();
    if re(r#"^\s*\{\s*(?:[-\w]+|'[^']*'|"[^"]*")\s*(?:,|=>)"#, "").is_match(&code) {
        code = pl::concat(&s("+"), &code);
    }
    let scope = runtime::file_scope(it, MODULE);
    let (v, _) = pl::eval_site_in(it, &code, MODULE, Some(scope), &[("self", self_.clone())], Want::Scalar);
    v
}

//------------------------------------------------------------------------------
/// New - create new BuildTagLookup object
/// Inputs: 0) reference to BuildTagLookup object or BuildTagLookup class name
pub fn new(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| new_body(it, a))
}

fn new_body(it: &mut Interp, a: Vec<Value>) -> ER<Vec<Value>> {
    let that = arg(&a, 0);
    let class = if that.is_ref() {
        s(&rtype(&that))
    } else if that.truthy() {
        that.clone()
    } else {
        s(MODULE)
    };
    let self_ = Value::new_hash();
    if let Value::HashRef(h) = &self_ {
        h.borrow_mut().blessed = Some(std::rc::Rc::from(class.to_text().as_str()));
    }
    let (subdirs, is_shortcut) = (Value::new_hash(), Value::new_hash());
    let count = Value::new_hash();
    count.set("unique tag names", n(0));
    count.set("total tags", n(0));
//
// loop through all tables, accumulating TagLookup and TagName information
//
    let tag_name_info = Value::new_hash();
    let id = Value::new_hash();
    let long_id = Value::new_hash();
    let long_name = Value::new_hash();
    let short_name = Value::new_hash();
    let table_num_h = Value::new_hash();
    let tag_lookup = Value::new_hash();
    let tag_exists = Value::new_hash();
    let no_lookup = Value::new_hash();
    let table_writable = Value::new_hash();
    let sep_table = Value::new_hash();
    let case = Value::new_hash();
    let structs = Value::new_hash();
    let composite_modules = Value::new_hash();
    let is_plugin_h = Value::new_hash();
    let flattened = Value::new_hash();
    let struct_lookup = Value::new_hash();
    let write_pseudo = Value::new_array(vec![]);
    let dup_xmp_tag = Value::new_hash();
    self_.set("TAG_NAME_INFO", tag_name_info.clone());
    self_.set("ID_LOOKUP", id.clone());
    self_.set("LONG_ID", long_id.clone());
    self_.set("LONG_NAME", long_name.clone());
    self_.set("SHORT_NAME", short_name.clone());
    self_.set("TABLE_NUM", table_num_h.clone());
    self_.set("TAG_LOOKUP", tag_lookup.clone());
    self_.set("TAG_EXISTS", tag_exists.clone());
    self_.set("FLATTENED", flattened.clone());
    self_.set("TABLE_WRITABLE", table_writable.clone());
    self_.set("SEPARATE_TABLE", sep_table.clone());
    self_.set("STRUCTURES", structs.clone());
    self_.set("STRUCT_LOOKUP", struct_lookup.clone()); // lookup for Struct hash ref based on Struct name
    self_.set("COMPOSITE_MODULES", composite_modules.clone());
    self_.set("COUNT", count.clone());
    self_.set("WRITE_PSEUDO", write_pseudo.clone());

    pl::call(it, "Image::ExifTool::LoadAllTables", vec![])?;
    let all_tables = pl::pkg_hash(it, "Image::ExifTool::allTables");
    let table_order = pl::pkg_array(it, "Image::ExifTool::tableOrder");
    let mut table_names = sorted_keys(&all_tables);
    // add Shortcuts after other tables
    table_names.push(s("Image::ExifTool::Shortcuts::Main"));
    // add plug-in modules last
    for t in my_array(it, "pluginTables").items() {
        table_names.push(t.clone());
        is_plugin_h.set(t.to_bytes(), n(1));
    }

    let mut table_num = 0i64;
    let et = pl::method(it, &s(ET), "new", vec![])?;
    // create lookup for short table names
    for table_name in &table_names {
        let mut short = table_name.clone();
        strip(&mut short, r"^Image::ExifTool::", "");
        strip(&mut short, r"::Main$", "");
        re(r"::", "").subst(&mut short, false, |_| b" ".to_vec());
        if !pl::str_eq(&short, "Nikon AVITags") {
            re(r"^(.+)Tags$", "").subst(&mut short, false, |c| pl::ucfirst(&cap(c, 1)).to_bytes().to_vec());
        }
        re(r"^Exif\b", "").subst(&mut short, false, |_| b"EXIF".to_vec());
        // change underlines to dashes in XMP-mwg group names
        // (we used underlines just because Perl variables can't contain dashes)
        re(r"^XMP mwg_", "").subst(&mut short, false, |_| b"XMP mwg-".to_vec());
        short_name.set(table_name.to_bytes(), short); // remember short name
        table_num_h.set(table_name.to_bytes(), n(table_num));
        table_num += 1;
    }
    // validate DICOM UID values
    let uid = pl::pkg_hash(it, "Image::ExifTool::DICOM::uid");
    for v in keys(&uid).iter().map(|k| uid.get(k.to_bytes())) {
        if !re(r"[\0-\x1f\x7f-\xff]", "").is_match(&v) {
            continue;
        }
        pl::warn(it, &format!("Warning: Special characters in DICOM UID value ({})\n", v.to_text()))?;
    }
    // make lookup table to check for shortcut tags
    let shortcuts = pl::pkg_hash(it, "Image::ExifTool::Shortcuts::Main");
    for tag in keys(&shortcuts) {
        let entry = shortcuts.get(tag.to_bytes());
        // ignore if shortcut tag name includes itself
        if rtype(&entry) == "ARRAY" {
            let r = dyn_re(&[b"^", &tag.to_bytes(), b"$"], "")?;
            if entry.items().iter().any(|e| r.is_match(e)) {
                continue;
            }
        }
        is_shortcut.set(pl::lc(&tag).to_bytes(), n(1));
    }
    let format_ok = my_hash(it, "formatOK");
    let shortcut_notes = my_hash(it, "shortcutNotes");
    let format_number = pl::pkg_hash(it, "Image::ExifTool::Exif::formatNumber");
    let format_size = pl::pkg_array(it, "Image::ExifTool::Exif::formatSize");
    let write_iptc = Value::Code(it.sub_ref("Image::ExifTool::IPTC::WriteIPTC"));
    let write_binary_data = Value::Code(it.sub_ref("Image::ExifTool::WriteBinaryData"));
    let pbd = Value::Code(it.sub_ref("Image::ExifTool::ProcessBinaryData"));
    let nikon_enc = Value::Code(it.sub_ref("Image::ExifTool::Nikon::ProcessNikonEncrypted"));
    let sony_enc = Value::Code(it.sub_ref("Image::ExifTool::Sony::ProcessEnciphered"));
    for table_name in &table_names {
        // create short table name
        let short = short_name.get(table_name.to_bytes());
        let info = Value::new_array(vec![]);
        tag_name_info.set(table_name.to_bytes(), info.clone());
        let is_plugin = is_plugin_h.get(table_name.to_bytes()).truthy();
        let table;
        let mut shortcut = false;
        let (is_offset, datamember, has_subdir) = (Value::new_hash(), Value::new_hash(), Value::new_hash());
        if pl::str_eq(&short, "Shortcuts") {
            // can't use GetTagTable() for Shortcuts (not a normal table)
            table = shortcuts.clone();
            shortcut = true;
        } else if is_plugin {
            table = get_tag_table(it, table_name)?;
            // don't add to allTables list because this messes our table order
            all_tables.delete(table_name.to_bytes());
            if let Value::ArrayRef(r) = &table_order {
                r.borrow_mut().pop();
            }
        } else {
            table = get_tag_table(it, table_name)?;
        }
        let table_num = table_num_h.get(table_name.to_bytes());
        let write_proc = table.get("WRITE_PROC");
        let vars = if table.get("VARS").truthy() { table.get("VARS") } else { Value::new_hash() };
        long_id.set(table_name.to_bytes(), n(0));
        long_name.set(table_name.to_bytes(), n(0));
        // save all tag names
        let (mut binary_table, mut no_id, mut is_iptc, mut is_xmp) = (false, false, false, false);
        if write_proc.truthy() && write_proc.same_ref(&write_iptc) {
            is_iptc = true;
        }
        // generate flattened tag names for structure fields if this is an XMP table
        if (table.get("GROUPS").truthy() && pl::str_eq(&table.get("GROUPS").get("0"), "XMP")) || vars.get("ADD_FLATTENED").truthy() {
            pl::call_list(it, "Image::ExifTool::XMP::AddFlattenedTags", vec![table.clone()])?;
            is_xmp = true;
        }
        let prt_id = vars.get("ID_FMT");
        if is_xmp || re(r"^(Shortcuts|ASF.*)$", "").is_match(&short) || (prt_id.truthy() && pl::str_eq(&prt_id, "none")) {
            no_id = true;
        }
        if table.get("WRITE_PROC").truthy() && table.get("WRITE_PROC").same_ref(&write_binary_data) && !table.get("CHECK_PROC").truthy() {
            pl::warn(it, &format!("Binary table {} doesn't have a CHECK_PROC\n", table_name.to_text()))?;
        }
        let pp_ = table.get("PROCESS_PROC");
        let process_binary_data = (pp_.truthy() && (pp_.same_ref(&pbd) || pp_.same_ref(&nikon_enc) || pp_.same_ref(&sony_enc))) || (table.get("VARS").truthy() && table.get("VARS").get("IS_BINARY").truthy());
        if vars.get("ID_LABEL").truthy() || process_binary_data {
            let sz = if table.get("FORMAT").truthy() {
                let fs = pl::call(it, "Image::ExifTool::FormatSize", vec![table.get("FORMAT")])?;
                if fs.truthy() {
                    fs
                } else {
                    n(1)
                }
            } else {
                n(1)
            };
            binary_table = true;
            id.set(table_name.to_bytes(), if vars.get("ID_LABEL").truthy() { vars.get("ID_LABEL") } else { cat(&[&s("Index"), &sz]) });
        } else if is_iptc && table.get("PROCESS_PROC").truthy() {
            //only the main IPTC table has a PROCESS_PROC
            id.set(table_name.to_bytes(), s("Record"));
        } else if !no_id {
            id.set(table_name.to_bytes(), s("Tag ID"));
        }
        set_my_scalar(it, "caseInsensitive", if is_xmp { n(1) } else { Value::Undef });
        set_my_scalar(it, "numbersFirst", n(2));
        if table.get("VARS").truthy() && table.get("VARS").get("ALPHA_FIRST").truthy() {
            set_my_scalar(it, "numbersFirst", n(-1));
        }
        let keys_ = sorted_tag_table_keys_rs(it, &table)?;
        set_my_scalar(it, "numbersFirst", n(1));
        let mut def_format = table.get("FORMAT");
        // use default format for binary data tables
        if !def_format.truthy() && binary_table {
            def_format = s("int8u");
        }

        'tag_id: for tag_id_orig in keys_ {
            let mut tag_id = tag_id_orig;
            let mut tag_names: Vec<Value> = Vec::new();
            let mut values: Vec<Value> = Vec::new();
            let mut require: Vec<Value> = Vec::new();
            let mut write_group_l: Vec<Value> = Vec::new();
            let mut writable_l: Vec<Value> = Vec::new();
            let info_array: Vec<Value>;
            if shortcut {
                // must build a dummy tagInfo list since Shortcuts is not a normal table
                let tag_info = Value::new_hash();
                tag_info.set("Name", tag_id.clone());
                tag_info.set("Notes", shortcut_notes.get(tag_id.to_bytes()));
                tag_info.set("Writable", n(1));
                tag_info.set("Require", Value::new_hash());
                let entries = table.get(tag_id.to_bytes());
                for (i, e) in entries.items().into_iter().enumerate() {
                    tag_info.get("Require").set(i.to_string(), e);
                }
                info_array = vec![tag_info];
            } else {
                info_array = get_tag_info_list(it, &table, &tag_id)?;
            }
            for tag_info in &info_array {
                if binary_table && tag_info.get("Writable").truthy() && !pl::str_eq(&tag_info.get("Writable"), "1") {
                    pl::warn(it, &format!("{} {} Writable should be 1, not {}\n", table_name.to_text(), tag_info.get("Name").to_text(), tag_info.get("Writable").to_text()))?;
                }
                let mut name = tag_info.get("Name");
                if tag_info.get("WritePseudo").truthy() {
                    write_pseudo.push(name.clone());
                    if !tag_info.get("Protected").truthy() {
                        pl::warn(it, &format!("Writable pseudo tag {} is not protected!\n", name.to_text()))?;
                    }
                }
                if !(tag_info.get("SubDirectory").truthy() || tag_info.get("Struct").truthy()) {
                    let lc = pl::lc(&name);
                    if case.get(lc.to_bytes()).truthy() && !eq(&case.get(lc.to_bytes()), &name) {
                        pl::warn(it, &format!("Different case for {} {} {}\n", table_name.to_text(), name.to_text(), case.get(lc.to_bytes()).to_text()))?;
                    }
                    case.set(lc.to_bytes(), name.clone());
                }
                let mut format = tag_info.get("Format");
                // check TagID's to make sure they don't start with 'ID-'
                let grps = get_group(it, &et, tag_info)?;
                for g in &grps {
                    if re(r"^ID-", "i").is_match(g) {
                        pl::warn(it, &format!("Group name starts with 'ID-' for {} {}\n", short.to_text(), name.to_text()))?;
                    }
                }
                if is_xmp && !tag_info.get("Avoid").truthy() && !tag_info.get("Struct").truthy() && (tag_info.get("Writable").truthy() || table.get("WRITABLE").truthy()) {
                    if dup_xmp_tag.get(name.to_bytes()).truthy() {
                        pl::warn(it, &format!("Duplicate writable XMP tag {}\n", name.to_text()))?;
                    }
                    dup_xmp_tag.set(name.to_bytes(), n(1));
                }
                // validate Name (must not start with a digit or else XML output will not be valid;
                // must not start with a dash or exiftool command line may get confused)
                if !re(r"^[_A-Za-z][-\w]+$", "").is_match(&name) &&
                    // single-character subdirectory names are allowed
                    (!tag_info.get("SubDirectory").truthy() || !re(r"^[_A-Za-z]$", "").is_match(&name))
                {
                    pl::warn(it, &format!("Warning: Invalid tag name {} '{}'\n", short.to_text(), name.to_text()))?;
                }
                // validate list type
                if tag_info.get("List").truthy() && !re(r"^(1|Alt|Bag|Seq|array|string)$", "").is_match(&tag_info.get("List")) {
                    pl::warn(it, &format!("Warning: Unknown List type ({}) for {} in {}\n", tag_info.get("List").to_text(), name.to_text(), table_name.to_text()))?;
                }
                if tag_info.get("Hook").truthy() && tag_info.get("Unknown").truthy() {
                    pl::warn(it, &format!("Warning: Unknown tag with Hook - {} {}\n", short.to_text(), name.to_text()))?;
                }
                // accumulate information for consistency check of BinaryData tables
                if process_binary_data && table.get("WRITABLE").truthy() {
                    // can't currently write tag if Condition accesses $valPt
                    if tag_info.get("Condition").truthy() && !tag_info.get("SubDirectory").truthy() && re(r"\$valPt", "").is_match(&tag_info.get("Condition")) && (tag_info.get("Writable").truthy() || tag_info.get("Writable").is_undef()) {
                        pl::warn(it, &format!("Warning: Tag not writable due to Condition - {} {}\n", short.to_text(), name.to_text()))?;
                    }
                    if tag_info.get("IsOffset").truthy() {
                        is_offset.set(tag_id.to_bytes(), name.clone());
                    }
                    if tag_info.get("SubDirectory").truthy() {
                        has_subdir.set(tag_id.to_bytes(), name.clone());
                    }
                    // require DATAMEMBER for writable var-format tags, Hook and DataMember tags
                    if format.truthy() && re(r"^var_", "").is_match(&format) {
                        datamember.set(tag_id.to_bytes(), name.clone());
                        if !(!tag_info.get("Writable").is_undef() && !tag_info.get("Writable").truthy()) {
                            pl::warn(it, &format!("Warning: Var-format tag is writable - {} {}\n", short.to_text(), name.to_text()))?;
                        }
                        // also need DATAMEMBER for tags used in length of var-sized value
                        for c in re(r"\$val\{(.*?)\}", "").each(&format) {
                            let mut idv = cap(&c, 1);
                            if re(r"^0x", "").is_match(&idv) {
                                idv = hex(&idv); // convert from hex if necessary
                            }
                            if rtype(&table.get(idv.to_bytes())) == "HASH" {
                                datamember.set(idv.to_bytes(), table.get(idv.to_bytes()).get("Name"));
                            }
                            if !datamember.get(idv.to_bytes()).truthy() {
                                pl::warn(it, &format!("Warning: Unknown ID ({}) used in Format - {} {}\n", idv.to_text(), short.to_text(), name.to_text()))?;
                            }
                        }
                    } else if tag_info.get("Hook").truthy() || (tag_info.get("RawConv").truthy() && re(r"\$self(->)?\{\w+\}\s*=(?!~)", "").is_match(&tag_info.get("RawConv"))) {
                        datamember.set(tag_id.to_bytes(), name.clone());
                    }
                    if format.truthy() && re(r"\$val\{", "").is_match(&format) && (tag_info.get("Writable").truthy() || tag_info.get("Writable").is_undef()) {
                        pl::warn(it, &format!("Warning: $val{{}} used in Format of writable tag - {} {}\n", short.to_text(), name.to_text()))?;
                    }
                }
                if tag_info.get("Hidden").truthy() {
                    if !pl::str_eq(&tag_info.get("Hidden"), "2") && !tag_info.get("Unknown").truthy() && (!tag_info.get("RawConv").truthy() || !re(r"Unknown|undef", "").is_match(&tag_info.get("RawConv"))) {
                        pl::warn(it, &format!("Warning: {} {} is Hidden contrary to guidelines\n", short.to_text(), name.to_text()))?;
                    }
                    if tag_info.same_ref(&info_array[0]) {
                        continue 'tag_id; // hide all tags with this ID if first tag in list is hidden
                    } else {
                        continue; // only hide this tag
                    }
                }
                let mut writable = Value::Undef;
                if !tag_info.get("Writable").is_undef() {
                    writable = tag_info.get("Writable");
                    // validate Writable
                    let ok = format_ok.get(writable.to_bytes()).truthy() || match re(r"(.*)\[", "").m(&writable) {
                        Some(c) => format_ok.get(cap(&c, 1).to_bytes()).truthy(),
                        None => false,
                    };
                    if !ok {
                        pl::warn(it, &format!("Warning: Unknown Writable ({}) - {} {}\n", writable.to_text(), short.to_text(), name.to_text()))?;
                    }
                } else if !tag_info.get("SubDirectory").truthy() {
                    writable = table.get("WRITABLE");
                }
                // in general, we can't write unless we have a WRITE_PROC
                if writable.truthy() && !(table.get("WRITE_PROC").truthy() || re(r"Shortcuts", "").is_match(table_name) || pl::str_eq(&writable, "2")) {
                    writable = Value::Undef;
                }
                //if ($writable and $$tagInfo{Unknown} and $$table{GROUPS}{0} ne 'MakerNotes') {
                //    warn "Warning: Writable Unknown tag - $short $name\n";
                //}
                // validate some characteristics of obvious date/time tags
                let g = get_group(it, &et, tag_info)?;
                let g1 = at(&g, 1);
                let g2 = at(&g, 2);
                if tag_info.get("List").truthy() && pl::str_eq(&g2, "Time") && writable.truthy() && !tag_info.get("Protected").truthy() && !tag_info.get("PrintConvInv").truthy() {
                    // (this is a problem because shifting Time:All would create a new list entry)
                    pl::warn(it, &format!("Writable List-type Time tag {}:{} has no PrintConvInv and is not Protected!\n", g1.to_text(), name.to_text()))?;
                }
                if tag_info.get("PrintConv").truthy() && pl::str_eq(&tag_info.get("PrintConv"), "$self->ConvertDateTime($val)") {
                    if !pl::str_eq(&g2, "Time") {
                        pl::warn(it, &format!("{} {} should be in 'Time' group!\n", short.to_text(), name.to_text()))?;
                    }
                    if writable.truthy() && tag_info.get("Shift").is_undef() && !pl::str_eq(&short, "PostScript") {
                        pl::warn(it, &format!("{} {} is not shiftable!\n", short.to_text(), name.to_text()))?;
                    }
                    if writable.truthy() && (!tag_info.get("PrintConvInv").truthy() || !re(r"InverseDateTime", "").is_match(&tag_info.get("PrintConvInv"))) {
                        pl::warn(it, &format!("{} {} missing InverseDateTime PrintConvInv\n", short.to_text(), name.to_text()))?;
                    }
                } else if re(r"DateTime(?!Stamp)", "").is_match(&name) && (!viv(tag_info, "Groups").get("2").truthy() || !pl::str_eq(&viv(tag_info, "Groups").get("2"), "Time")) && !pl::str_eq(&short, "DICOM") {
                    pl::warn(it, &format!("{} {} should be in 'Time' group!\n", short.to_text(), name.to_text()))?;
                }
                if pl::str_eq(&name, "DateTimeOriginal") && (!tag_info.get("Description").truthy() || !pl::str_eq(&tag_info.get("Description"), "Date/Time Original")) {
                    pl::warn(it, &format!("Need Description for {} DateTimeOriginal\n", short.to_text()))?;
                }
                // validate Description (can't contain special characters)
                if tag_info.get("Description").truthy() && !eq(&tag_info.get("Description"), &escape_html(it, &tag_info.get("Description"))?) {
                    // this is a problem because the Escape option currently only
                    // escapes descriptions if the default Lang option isn't default
                    pl::warn(it, &format!("{} description contains special characters!\n", name.to_text()))?;
                }
                // generate structure lookup
                let mut struct_ = tag_info.get("Struct");
                let mut str_table = Value::Undef;
                if struct_.is_ref() {
                    str_table = struct_.clone();
                    struct_ = str_table.get("STRUCT_NAME");
                    if struct_.truthy() {
                        let old_table = struct_lookup.get(struct_.to_bytes());
                        if old_table.truthy() && !eq(&old_table, &str_table) {
                            pl::warn(it, &format!("Duplicate XMP structure with name {}\n", struct_.to_text()))?;
                        } else {
                            struct_lookup.set(struct_.to_bytes(), str_table.clone());
                        }
                    } else {
                        pl::warn(it, &format!("Missing STRUCT_NAME for structure in {}\n", tag_info.get("Name").to_text()))?;
                        str_table = Value::Undef;
                    }
                } else if struct_.truthy() {
                    str_table = struct_lookup.get(struct_.to_bytes());
                    if !str_table.truthy() {
                        if !re(r" ", "").is_match(&struct_) {
                            struct_ = cat(&[&s("XMP "), &struct_]);
                        }
                        pl::warn(it, &format!("Missing {} structure!\n", struct_.to_text()))?;
                        struct_ = Value::Undef;
                    }
                }
                let _ = &str_table;
                // validate SubIFD flag
                let mut subdir = tag_info.get("SubDirectory");
                let is_sub = subdir.truthy() && subdir.get("Start").truthy() && re(r"\$val\b", "").is_match(&subdir.get("Start"));
                if tag_info.get("SubIFD").truthy() {
                    if !is_sub {
                        pl::warn(it, &format!("Warning: Wrong SubDirectory Start for SubIFD tag - {} {}\n", short.to_text(), name.to_text()))?;
                    }
                } else if is_sub && !process_binary_data {
                    pl::warn(it, &format!("Warning: SubIFD flag not set for {} {}\n", short.to_text(), name.to_text()))?;
                }
                if tag_info.get("Notes").truthy() {
                    let mut note = tag_info.get("Notes");
                    // remove leading/trailing blank lines
                    strip(&mut note, r"^\s+", "");
                    strip(&mut note, r"\s+$", "");
                    // remove leading/trailing spaces on each line
                    re(r"(^[ \t]+|[ \t]+$)", "m").subst(&mut note, true, |_| Vec::new());
                    values.push(cat(&[&s("("), &note, &s(")")]));
                }
                if is_xmp && (!eq(&pl::lc(&tag_id), &pl::lc(&name)) || tag_info.get("NotFlat").truthy()) {
                    let note = if tag_info.get("NotFlat").truthy() {
                        s("NOT a flattened tag!")
                    } else if tag_info.get("RootTagInfo").truthy() {
                        // add note about different XMP Tag ID
                        tag_id.clone()
                    } else {
                        cat(&[&s("tag ID is '"), &tag_id, &s("'")])
                    };
                    if tag_info.get("Notes").truthy() {
                        if let Some(last) = values.last_mut() {
                            let nb = note.to_bytes();
                            re(r"^\(", "").subst(last, false, |_| [&b"("[..], &nb[..], b"; "].concat());
                        }
                    } else {
                        values.push(cat(&[&s("("), &note, &s(")")]));
                    }
                }
                let mut write_group;
                if pl::str_eq(&short, "Extra") {
                    write_group = if tag_info.get("WriteOnly").truthy() { s("-") } else { g1.clone() };
                } else {
                    write_group = tag_info.get("WriteGroup");
                }
                if !write_group.truthy() {
                    if writable.truthy() {
                        write_group = table.get("WRITE_GROUP");
                    }
                    if !write_group.truthy() {
                        write_group = s("-");
                    }
                }
                if !format.is_undef() {
                    // validate Format
                    let ok = format_ok.get(format.to_bytes()).truthy() || pl::str_eq(&short, "PICT") || match re(r"^(var_)?(.*)\[", "").m(&format) {
                        Some(c) => format_ok.get(cap(&c, 2).to_bytes()).truthy(),
                        None => false,
                    };
                    if !ok {
                        pl::warn(it, &format!("Warning: Unknown Format ({}) for {} {}\n", format.to_text(), short.to_text(), name.to_text()))?;
                    }
                } else {
                    format = def_format.clone();
                }
                if subdir.truthy() {
                    let sub_table = if subdir.get("TagTable").truthy() { subdir.get("TagTable") } else { table_name.clone() };
                    values.push(short_name.get(sub_table.to_bytes()));
                } else if struct_.truthy() {
                    values.push(struct_.clone());
                    structs.set(struct_.to_bytes(), n(1));
                }
                for type_ in ["Require", "Desire"] {
                    let req = tag_info.get(type_);
                    if req.is_ref() {
                        for k in sort_num(keys(&req)) {
                            require.push(req.get(k.to_bytes()));
                        }
                    } else if req.truthy() {
                        require.push(req);
                    }
                }
                let mut print_conv = tag_info.get("PrintConv");
                if tag_info.get("Mask").truthy() {
                    let mut val = tag_info.get("Mask");
                    let bsh = tag_info.get("BitShift");
                    if bsh.truthy() {
                        values.push(pl::sprintf("[val >> %d & 0x%x]", &[bsh.clone(), ops::shift_right(&val, &bsh)]));
                    } else {
                        values.push(pl::sprintf("[val & 0x%x]", &[val.clone()]));
                    }
                    // verify that all values are within the mask
                    if rtype(&print_conv) == "HASH" {
                        val = ops::shift_right(&val, &tag_info.get("BitShift"));
                        for k in keys(&print_conv) {
                            if !re(r"^\d+$", "").is_match(&k) || pl::num_eq(&ops::bit_and(&k, &val), &k) {
                                continue;
                            }
                            let hx = pl::sprintf("0x%.2x", &[k.clone()]);
                            pl::warn(it, &format!("{} {} PrintConv value {} is not in Mask!\n", short.to_text(), name.to_text(), hx.to_text()))?;
                        }
                    }
                }
                let pct = rtype(&print_conv);
                if pct == "HASH" || pct == "ARRAY" {
                    let mut print_conv_list: Vec<Value> = Vec::new();
                    let mut index_list: Vec<Value> = Vec::new();
                    let mut index = Value::Undef;
                    let mut value_conv_hash = Value::Undef;
                    if pct == "ARRAY" {
                        let pcs = print_conv.items();
                        for (index_n, pc) in pcs.iter().enumerate() {
                            if rtype(pc) == "HASH" {
                                if pc.keys().is_empty() {
                                    continue;
                                }
                                print_conv_list.push(pc.clone());
                                index_list.push(n(index_n as i64));
                            } else if pc.truthy() && pl::str_eq(pc, "REPEAT") && index_n != 0 {
                                print_conv_list.push(pcs[index_n - 1].clone());
                                index_list.push(s("N"));
                            } else {
                                continue;
                            }
                            // collapse values with identical PrintConv's
                            let l = print_conv_list.len();
                            if l >= 2 && eq(&print_conv_list[l - 1], &print_conv_list[l - 2]) {
                                let il = index_list.len();
                                if index_list[il - 2].is_ref() {
                                    index_list[il - 2].push(index_list[il - 1].clone());
                                } else {
                                    index_list[il - 2] = Value::new_array(vec![index_list[il - 2].clone(), index_list[il - 1].clone()]);
                                }
                                print_conv_list.pop();
                                index_list.pop();
                            }
                        }
                        print_conv = if print_conv_list.is_empty() { Value::Undef } else { print_conv_list.remove(0) };
                        index = if index_list.is_empty() { Value::Undef } else { index_list.remove(0) };
                    } else if rtype(&tag_info.get("ValueConv")) == "HASH" {
                        value_conv_hash = tag_info.get("ValueConv");
                    }
                    while !print_conv.is_undef() {
                        if !index.is_undef() {
                            // (print indices of original values if reorganized)
                            let mut sfx = s("");
                            let mut idx = if tag_info.get("Relist").truthy() { tag_info.get("Relist").at(index.to_i64()) } else { index.clone() };
                            if idx.is_ref() {
                                let iv = idx.items();
                                if iv.len() > 1 {
                                    sfx = s("s");
                                }
                                // collapse consecutive number ranges
                                let mut iout: Vec<Value> = Vec::new();
                                let mut rng_start = Value::Undef;
                                for i in 0..iv.len() {
                                    if i + 1 < iv.len() && (pl::str_eq(&iv[i + 1], "N") || pl::num_eq(&iv[i + 1], &pl::add(&iv[i], &n(1)))) {
                                        if rng_start.is_undef() {
                                            rng_start = iv[i].clone();
                                        }
                                        continue;
                                    }
                                    let pre = if !rng_start.is_undef() { cat(&[&rng_start, &s("-")]) } else { s("") };
                                    iout.push(cat(&[&pre, &iv[i]]));
                                }
                                idx = pl::join(", ", &iout);
                                re(r"(.*),", "").subst(&mut idx, false, |c| [&cap(c, 1).to_bytes()[..], b" and"].concat());
                            } else if !tag_info.get("Relist").truthy() {
                                while !print_conv_list.is_empty() && eq(&print_conv, &print_conv_list[0]) {
                                    print_conv_list.remove(0);
                                    index = if index_list.is_empty() { Value::Undef } else { index_list.remove(0) };
                                }
                                if !pl::num_eq(&idx, &index) {
                                    idx = cat(&[&idx, &s("-"), &index]);
                                    sfx = s("s");
                                }
                            }
                            values.push(cat(&[&s("[Value"), &sfx, &s(" "), &idx, &s("]")]));
                        }
                        if tag_info.get("SeparateTable").truthy() {
                            subdir = n(1);
                            let mut st = tag_info.get("SeparateTable");
                            if pl::str_eq(&st, "1") {
                                st = name.clone();
                            }
                            // add module name if not specified
                            if !re(r" ", "").is_match(&st) {
                                if let Some(c) = re(r"^(\w+)", "").m(&short) {
                                    st = cat(&[&cap(&c, 1), &s(" "), &st]);
                                }
                            }
                            values.push(st.clone());
                            sep_table.set(st.to_bytes(), print_conv.clone());
                            // add PrintHex flag to PrintConv so we can check it later
                            if tag_info.get("PrintHex").truthy() {
                                print_conv.set("PrintHex", n(1));
                            }
                            if tag_info.get("PrintInt").truthy() {
                                print_conv.set("PrintInt", n(1));
                            }
                            if tag_info.get("PrintString").truthy() {
                                print_conv.set("PrintString", n(1));
                            }
                        } else {
                            set_my_scalar(it, "caseInsensitive", n(0));
                            let mut pk = hkeys(&print_conv)?;
                            if tag_info.get("PrintSort").truthy() {
                                let nf = my_scalar(it, "numbersFirst");
                                let pc = print_conv.clone();
                                pk.sort_by(|x, y| ord_of(&numbers_first_rs(&pc.get(x.to_bytes()), &pc.get(y.to_bytes()), &nf, false)));
                            } else {
                                sort_nf(it, &mut pk);
                            }
                            let mut nn = values.len();
                            let mut bits = Value::Undef;
                            for k in &pk {
                                if pl::str_eq(k, "") && pl::str_eq(&print_conv.get(k.to_bytes()), "") {
                                    continue;
                                }
                                if pl::str_eq(k, "BITMASK") {
                                    bits = print_conv.get(k.to_bytes());
                                    continue;
                                }
                                if pl::str_eq(k, "OTHER") {
                                    continue;
                                }
                                let mut idx_s = k.clone();
                                if tag_info.get("PrintInt").truthy() {
                                    strip(&mut idx_s, r"\.\d+$", "");
                                }
                                if (tag_info.get("PrintHex").truthy() || print_conv.get("BITMASK").truthy()) && re(r"^-?\d+$", "").is_match(&idx_s) {
                                    let dig = if tag_info.get("PrintHex").truthy() { tag_info.get("PrintHex") } else { n(1) };
                                    if pl::ge(&idx_s, &n(0)) {
                                        idx_s = pl::sprintf("0x%.*x", &[dig, idx_s.clone()]);
                                    } else if format.truthy() {
                                        if let Some(c) = re(r"int(16|32)", "").m(&format) {
                                            // mask off unused bits of signed integer hex value
                                            let mask = if pl::str_eq(&cap(&c, 1), "16") { n(0xffff) } else { n(0xffffffff) };
                                            idx_s = pl::sprintf("0x%.*x", &[dig, ops::bit_and(&idx_s, &mask)]);
                                        }
                                    }
                                } else if tag_info.get("PrintString").truthy() || !re(r"^[+-]?(?=\d|\.\d)\d*(\.\d*)?$", "").is_match(k) {
                                    // translate unprintable values
                                    if re(r"([\x00-\x1f\x7f-\xff])", "").subst(&mut idx_s, true, |c| format!("\\x{:02x}", cap(c, 1).to_bytes()[0]).into_bytes()) > 0 {
                                        idx_s = cat(&[&s("\""), &idx_s, &s("\"")]);
                                    } else {
                                        idx_s = cat(&[&s("'"), &idx_s, &s("'")]);
                                    }
                                }
                                values.push(cat(&[&idx_s, &s(" = "), &print_conv.get(k.to_bytes())]));
                                if value_conv_hash.truthy() {
                                    for v in keys(&value_conv_hash) {
                                        if !eq(&value_conv_hash.get(v.to_bytes()), k) {
                                            continue;
                                        }
                                        let last = values.len() - 1;
                                        values[last] = cat(&[&v, &s(" => "), &values[last]]);
                                        break;
                                    }
                                }
                                // validate all PrintConv values
                                if re(r"[\0-\x1f\x7f-\xff]", "").is_match(&print_conv.get(k.to_bytes())) {
                                    pl::warn(it, &format!("Warning: Special characters in {} {} PrintConv ({})\n", short.to_text(), name.to_text(), print_conv.get(k.to_bytes()).to_text()))?;
                                }
                            }
                            if bits.truthy() {
                                let mut bk = hkeys(&bits)?;
                                sort_nf(it, &mut bk);
                                for k in bk {
                                    values.push(cat(&[&s("Bit "), &k, &s(" = "), &bits.get(k.to_bytes())]));
                                }
                            }
                            // organize values into columns if specified
                            let mut cols = tag_info.get("PrintConvColumns");
                            if !cols.truthy() && values.len() as i64 - nn as i64 >= 6 {
                                // do columns if more than 6 short entries
                                let mut max_len = 0i64;
                                for v in &values[nn..] {
                                    if max_len < len(v) {
                                        max_len = len(v);
                                    }
                                }
                                let num = n(values.len() as i64 - nn as i64);
                                cols = pl::int(&pl::div(&n(50), &n(max_len + 2))?); // (50 chars max width)
                                // have 3 rows minimum
                                while cols.truthy() && pl::lt(&pl::div(&num, &cols)?, &n(3)) {
                                    cols = pl::sub(&cols, &n(1));
                                }
                            }
                            if cols.truthy() {
                                let new_v: Vec<Value> = values.split_off(nn);
                                let mut v = s("[!HTML]<table class=cols><tr>");
                                let rows = pl::int(&pl::div(&pl::sub(&pl::add(&n(new_v.len() as i64), &cols), &n(1)), &cols)?).to_i64();
                                nn = 0;
                                loop {
                                    v = pl::concat(&v, &s("\n  <td>"));
                                    let mut i = 0i64;
                                    while i < rows && ((nn as i64) + i) < new_v.len() as i64 {
                                        if i != 0 {
                                            v = pl::concat(&v, &s("\n  <br>"));
                                        }
                                        let e = escape_html(it, &new_v[nn + i as usize])?;
                                        v = pl::concat(&v, &e);
                                        i += 1;
                                    }
                                    v = pl::concat(&v, &s("</td>"));
                                    nn = (nn as i64 + rows) as usize;
                                    if nn >= new_v.len() {
                                        break;
                                    }
                                    v = pl::concat(&v, &s("<td>&nbsp;&nbsp;</td>")); // add spaces between columns
                                }
                                values.push(pl::concat(&v, &s("</tr></table>\n")));
                            }
                        }
                        if print_conv_list.is_empty() {
                            break;
                        }
                        print_conv = print_conv_list.remove(0);
                        index = if index_list.is_empty() { Value::Undef } else { index_list.remove(0) };
                    }
                // look inside scalar PrintConv for a bit/byte conversion
                // (see Photoshop:PrintFlags for use of "$byte" decoding)
                } else if print_conv.truthy() {
                    if let Some(c) = re(r"DecodeBits\(\$(val|byte),\s*(\\\%[\w:]+|\{.*\})\s*\)", "s").m(&print_conv) {
                        let type_ = if pl::str_eq(&cap(&c, 1), "byte") { "Byte" } else { "Bit" };
                        self_.set("Model", s("")); // needed for Nikon ShootingMode
                        let bits = eval_bits(it, &cap(&c, 2), &self_);
                        self_.delete("Model");
                        let err = it.get_scalar("main::@");
                        if err.truthy() {
                            it.warn(err)?;
                        } else {
                            let mut bk = hkeys(&bits)?;
                            sort_nf(it, &mut bk);
                            for k in bk {
                                values.push(cat(&[&s(type_), &s(" "), &k, &s(" = "), &bits.get(k.to_bytes())]));
                            }
                        }
                    }
                }
                if subdir.truthy() && !tag_info.get("SeparateTable").truthy() {
                    if writable.truthy() {
                        // subdirectories are only writable if specified explicitly
                        let tw = tag_info.get("Writable");
                        if (tw.truthy() && pl::str_eq(&writable, "1")) || pl::str_eq(&writable, "2") {
                            writable = s("yes");
                        }
                        writable = pl::concat(&s("-"), &if tw.truthy() { writable.clone() } else { s("") });
                        if tw.truthy() && has_bits(&if tag_info.get("Protected").truthy() { tag_info.get("Protected") } else { n(0) }, 0x01) {
                            writable = pl::concat(&writable, &s("!"));
                        }
                        if tag_info.get("List").truthy() || tag_info.get("IsList").truthy() {
                            writable = pl::concat(&writable, &s("+"));
                        }
                        if !tag_info.get("Permanent").is_undef() {
                            if !tag_info.get("Permanent").truthy() {
                                writable = pl::concat(&writable, &s("^"));
                            }
                        } else if !table.get("PERMANENT").is_undef() && !table.get("PERMANENT").truthy() {
                            writable = pl::concat(&writable, &s("^"));
                        }
                    } else {
                        writable = s("-");
                    }
                } else {
                    // not writable if we can't do the inverse conversions
                    let mut no_print_conv_inv = false;
                    if writable.truthy() {
                        for c in ["PrintConv", "ValueConv"] {
                            if !tag_info.get(c).truthy() {
                                continue;
                            }
                            let inv = format!("{c}Inv");
                            if !tag_info.get(&inv).is_undef() {
                                continue;
                            }
                            // (undefined inverse conversion overrides hash lookup)
                            if !tag_info.exists(&inv) {
                                let rt = rtype(&tag_info.get(c));
                                if rt == "HASH" || rt == "ARRAY" {
                                    continue;
                                }
                                if tag_info.get("WriteAlso").truthy() {
                                    continue;
                                }
                            }
                            if c == "ValueConv" {
                                writable = Value::Undef;
                            } else {
                                no_print_conv_inv = true;
                            }
                        }
                    }
                    if !writable.truthy() {
                        writable = s("no");
                    } else {
                        if pl::str_eq(&writable, "1") || pl::str_eq(&writable, "2") {
                            writable = if format.truthy() { format.clone() } else { s("yes") };
                        }
                        let mut count = if tag_info.get("Count").truthy() { tag_info.get("Count") } else { n(1) };
                        // adjust count to Writable size if different than Format
                        if writable.truthy() && format.truthy() && !eq(&writable, &format) && !pl::str_eq(&writable, "string") && !pl::str_eq(&format, "string") && format_number.get(writable.to_bytes()).truthy() && format_number.get(format.to_bytes()).truthy() {
                            let n1 = format_number.get(format.to_bytes());
                            let n2 = format_number.get(writable.to_bytes());
                            count = pl::mul(&count, &pl::div(&format_size.at(n1.to_i64()), &format_size.at(n2.to_i64()))?);
                        }
                        if !pl::num_eq(&count, &n(1)) {
                            if pl::lt(&count, &n(0)) {
                                count = s("n");
                            }
                            writable = cat(&[&writable, &s("["), &count, &s("]")]);
                        }
                        if no_print_conv_inv {
                            writable = pl::concat(&writable, &s("~"));
                        }
                        // add a '*' if this tag is protected or a '!' for unsafe tags
                        if tag_info.get("Protected").truthy() {
                            if has_bits(&tag_info.get("Protected"), 0x02) {
                                writable = pl::concat(&writable, &s("*"));
                            }
                            if has_bits(&tag_info.get("Protected"), 0x01) {
                                writable = pl::concat(&writable, &s("!"));
                            }
                        }
                        if tag_info.get("Avoid").truthy() {
                            writable = pl::concat(&writable, &s("/"));
                        }
                    }
                    if struct_.truthy() {
                        writable = s("=struct");
                    }
                    if !tag_info.get("Flat").is_undef() {
                        writable = pl::concat(&writable, &s("_"));
                    }
                    if tag_info.get("List").truthy() || tag_info.get("IsList").truthy() {
                        writable = pl::concat(&writable, &s("+"));
                    }
                    if tag_info.get("Mandatory").truthy() {
                        writable = pl::concat(&writable, &s(":"));
                    }
                    if !tag_info.get("Permanent").is_undef() {
                        if !tag_info.get("Permanent").truthy() {
                            writable = pl::concat(&writable, &s("^"));
                        }
                    } else if !table.get("PERMANENT").is_undef() && !table.get("PERMANENT").truthy() {
                        writable = pl::concat(&writable, &s("^"));
                    }
                    // separate tables link like subdirectories (flagged with leading '-')
                    if subdir.truthy() {
                        writable = pl::concat(&s("-"), &writable);
                    }
                }
                // don't duplicate a tag name unless an entry is different
                let lc_name = pl::lc(&name);
                // check for conflicts with shortcut names
                if is_shortcut.get(lc_name.to_bytes()).truthy() && !pl::str_eq(&short, "Shortcuts") && (tag_info.get("Writable").truthy() || !tag_info.get("SubDirectory").truthy()) {
                    pl::warn(it, &format!("WARNING: {} {} is a shortcut tag!\n", short.to_text(), name.to_text()))?;
                }
                if tag_info.get("Unknown").truthy() {
                    name = pl::concat(&name, &s("?"));
                }
                let same = !tag_names.is_empty() && eq(tag_names.last().unwrap(), &name) && eq(write_group_l.last().unwrap(), &write_group) && eq(writable_l.last().unwrap(), &writable);
                if !same {
                    tag_names.push(name.clone());
                    write_group_l.push(write_group.clone());
                    writable_l.push(writable.clone());
                }
//
// add this tag to the tag lookup unless NO_LOOKUP is set or shortcut or plug-in tag
//
                if shortcut || is_plugin {
                    continue;
                }
                // count tags
                if tag_info.get("SubDirectory").truthy() {
                    if vars.get("NO_LOOKUP").truthy() {
                        continue;
                    }
                    if !subdirs.get(lc_name.to_bytes()).truthy() {
                        subdirs.set(lc_name.to_bytes(), n(0));
                    }
                    subdirs.set(lc_name.to_bytes(), pl::add(&subdirs.get(lc_name.to_bytes()), &n(1)));
                } else {
                    count.set("total tags", pl::add(&count.get("total tags"), &n(1)));
                    let te = tag_exists.get(lc_name.to_bytes());
                    let sd = subdirs.get(lc_name.to_bytes());
                    if !(te.truthy() && (!sd.truthy() || pl::num_eq(&sd, &te))) && !no_lookup.get(lc_name.to_bytes()).truthy() {
                        count.set("unique tag names", pl::add(&count.get("unique tag names"), &n(1)));
                    }
                    // don't add to tag lookup if specified
                    if vars.get("NO_LOOKUP").truthy() {
                        no_lookup.set(lc_name.to_bytes(), n(1));
                        continue;
                    }
                }
                if !tag_exists.get(lc_name.to_bytes()).truthy() {
                    tag_exists.set(lc_name.to_bytes(), n(0));
                }
                tag_exists.set(lc_name.to_bytes(), pl::add(&tag_exists.get(lc_name.to_bytes()), &n(1)));
                // only add writable tags to lookup table (for speed)
                let wflag = tag_info.get("Writable");
                if !(write_proc.truthy() && (wflag.truthy() || (table.get("WRITABLE").truthy() && wflag.is_undef() && !tag_info.get("SubDirectory").truthy()))) {
                    continue;
                }
                if !tag_lookup.get(lc_name.to_bytes()).truthy() {
                    tag_lookup.set(lc_name.to_bytes(), Value::new_hash());
                }
                // add to lookup for flattened tags if necessary
                if tag_info.get("RootTagInfo").truthy() {
                    if !flattened.get(lc_name.to_bytes()).truthy() {
                        flattened.set(lc_name.to_bytes(), Value::new_hash());
                    }
                    flattened.get(lc_name.to_bytes()).set(table_num.to_bytes(), tag_info.get("RootTagInfo").get("TagID"));
                }
                // remember number for this table
                let mut tag_ids = tag_lookup.get(lc_name.to_bytes()).get(table_num.to_bytes());
                // must allow for duplicate tags with the same name in a single table!
                if tag_ids.truthy() {
                    if rtype(&tag_ids) == "HASH" {
                        tag_ids.set(tag_id.to_bytes(), n(1));
                        continue;
                    } else if eq(&tag_id, &tag_ids) {
                        continue;
                    } else {
                        let h = Value::new_hash();
                        h.set(tag_ids.to_bytes(), n(1));
                        h.set(tag_id.to_bytes(), n(1));
                        tag_ids = h;
                    }
                } else {
                    tag_ids = tag_id.clone();
                }
                table_writable.set(table_name.to_bytes(), n(1));
                tag_lookup.get(lc_name.to_bytes()).set(table_num.to_bytes(), tag_ids);
                // keep track of extra modules needed for Composite tags
                if tag_info.get("Module").truthy() {
                    composite_modules.set(lc_name.to_bytes(), tag_info.get("Module"));
                }
            }
//
// save TagName information
//
            let tag_id_str;
            if let Some(c) = re(r"^(-)?\d+(\.\d+)?$", "").m(&tag_id) {
                if cap(&c, 1).truthy() {
                    tag_id_str = tag_id.clone();
                } else if !prt_id.is_undef() {
                    if pl::str_eq(&prt_id, "hex") {
                        tag_id_str = pl::sprintf("0x%.4x", &[tag_id.clone()]);
                    } else if pl::str_eq(&prt_id, "str") {
                        tag_id_str = cat(&[&s("'"), &tag_id, &s("'")]);
                    } else {
                        tag_id_str = tag_id.clone();
                    }
                } else if !cap(&c, 2).truthy() && !binary_table && !is_iptc && !(re(r"^CanonCustom", "").is_match(&short) && pl::lt(&tag_id, &n(256))) {
                    tag_id_str = pl::sprintf("0x%.4x", &[tag_id.clone()]);
                } else if pl::lt(&tag_id, &n(0x10000)) {
                    tag_id_str = tag_id.clone();
                } else {
                    tag_id_str = pl::sprintf("0x%.8x", &[tag_id.clone()]);
                }
            } else if pl::str_eq(&short, "DICOM") {
                let mut t = tag_id.clone();
                re(r"_", "").subst(&mut t, false, |_| b",".to_vec());
                tag_id_str = t;
            } else if re(r"^0x([0-9a-f]+)\.(\d+)$", "").is_match(&tag_id) {
                // DR4 tags like '0x20500.0'
                tag_id_str = tag_id.clone();
            } else {
                // convert non-printable characters to hex escape sequences
                if re(r"([\x00-\x1f\x7f-\xff])", "").subst(&mut tag_id, true, |c| format!("\\x{:02x}", cap(c, 1).to_bytes()[0]).into_bytes()) > 0 {
                    re(r"\\x00", "").subst(&mut tag_id, true, |_| br"\0".to_vec());
                    if pl::str_eq(&tag_id, r"jP\x1a\x1a") {
                        continue; // ignore abnormal JP2 signature tag
                    }
                    tag_id_str = cat(&[&s("\""), &tag_id, &s("\"")]);
                } else {
                    tag_id_str = cat(&[&s("'"), &tag_id, &s("'")]);
                }
            }
            let l = len(&tag_id_str);
            if pl::lt(&long_id.get(table_name.to_bytes()), &n(l)) {
                long_id.set(table_name.to_bytes(), n(l));
            }
            for t in &tag_names {
                let l = len(t);
                if pl::lt(&long_name.get(table_name.to_bytes()), &n(l)) {
                    long_name.set(table_name.to_bytes(), n(l));
                }
            }
            info.push(Value::new_array(vec![tag_id_str, Value::new_array(tag_names), Value::new_array(writable_l), Value::new_array(values), Value::new_array(require), Value::new_array(write_group_l)]));
        }
        // do consistency check of writable BinaryData tables
        if process_binary_data && table.get("WRITABLE").truthy() {
            let lookup = Value::new_hash();
            lookup.set("IS_OFFSET", is_offset.clone());
            lookup.set("IS_SUBDIR", has_subdir.clone());
            lookup.set("DATAMEMBER", datamember.clone());
            for var in sorted_keys(&lookup) {
                let hash = lookup.get(var.to_bytes());
                if table.get(var.to_bytes()).truthy() {
                    for tag_id in table.get(var.to_bytes()).items() {
                        if hash.get(tag_id.to_bytes()).truthy() {
                            hash.delete(tag_id.to_bytes());
                            continue;
                        }
                        let m = pl::sprintf("Warning: Extra %s for %s tag %d (0x%.4x)\n", &[var.clone(), short.clone(), tag_id.clone(), tag_id.clone()]);
                        it.warn(m)?;
                    }
                }
                for tag_id in sorted_keys(&hash) {
                    let m = pl::sprintf("Warning: Missing %s for %s %s, tag %s (0x%.4x)\n", &[var.clone(), short.clone(), hash.get(tag_id.to_bytes()), tag_id.clone(), tag_id.clone()]);
                    it.warn(m)?;
                }
            }
        }
    }
    // save information about structures
    let special_tags = pl::pkg_hash(it, "Image::ExifTool::specialTags");
    for str_name in keys(&structs) {
        let struct_ = struct_lookup.get(str_name.to_bytes());
        let full_name = cat(&[&s(if re(r" ", "").is_match(&str_name) { "" } else { "XMP " }), &str_name, &s(" Struct")]);
        let info = Value::new_array(vec![]);
        tag_name_info.set(full_name.to_bytes(), info.clone());
        let order = if struct_.get("SORT_ORDER").truthy() { struct_.get("SORT_ORDER").items() } else { sorted_keys(&struct_) };
        for tag in order {
            let tag_info = struct_.get(tag.to_bytes());
            if !(rtype(&tag_info) == "HASH" && !pl::str_eq(&tag, "NAMESPACE") && !pl::str_eq(&tag, "GROUPS")) {
                continue;
            }
            if special_tags.get(tag.to_bytes()).truthy() {
                pl::warn(it, &format!("WARNING: {} Struct containes {}\n", str_name.to_text(), tag.to_text()))?;
            }
            let mut writable = tag_info.get("Writable");
            let mut vals: Vec<Value> = Vec::new();
            if !writable.truthy() {
                writable = tag_info.get("Struct");
                if writable.is_ref() {
                    writable = writable.get("STRUCT_NAME");
                }
                if writable.truthy() {
                    vals.push(writable.clone());
                    structs.set(writable.to_bytes(), n(1));
                    writable = cat(&[&s("="), &writable]);
                } else if !tag_info.get("Writable").is_undef() {
                    writable = s("no");
                } else {
                    writable = s("string");
                }
            }
            if tag_info.get("List").truthy() || tag_info.get("IsList").truthy() {
                writable = pl::concat(&writable, &s("+"));
            }
            if tag_info.get("Notes").truthy() {
                vals.push(cat(&[&s("("), &tag_info.get("Notes"), &s(")")]));
            }
            // handle PrintConv lookups in Structure elements
            let print_conv = tag_info.get("PrintConv");
            if rtype(&print_conv) == "HASH" {
                let mut pk = keys(&print_conv);
                sort_nf(it, &mut pk);
                for k in pk {
                    if re(r"^(OTHER|BITMASK)$", "").is_match(&k) {
                        continue;
                    }
                    vals.push(cat(&[&k, &s(" = "), &print_conv.get(k.to_bytes())]));
                }
            }
            let nm = if tag_info.get("Name").truthy() { tag_info.get("Name") } else { pl::ucfirst(&tag) };
            info.push(Value::new_array(vec![tag.clone(), Value::new_array(vec![nm]), Value::new_array(vec![writable]), Value::new_array(vals), Value::new_array(vec![]), Value::new_array(vec![])]));
        }
    }
    ret(self_)
}

//------------------------------------------------------------------------------
/// Rewrite this file to build the lookup tables
/// Inputs: 0) BuildTagLookup object reference
///         1) output tag lookup module name (eg. 'lib/Image/ExifTool/TagLookup.pm')
/// Returns: true on success
pub fn write_tag_lookup(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    // local ($_, *INFILE, *OUTFILE);
    pl::local_topic(it, |it| write_tag_lookup_body(it, a))
}

fn write_tag_lookup_body(it: &mut Interp, a: Vec<Value>) -> ER<Vec<Value>> {
    let (self_, file) = (arg(&a, 0), arg(&a, 1));
    let tag_lookup = self_.get("TAG_LOOKUP");
    let tag_exists = self_.get("TAG_EXISTS");
    let flattened = self_.get("FLATTENED");
    let table_writable = self_.get("TABLE_WRITABLE");
    let u = Value::Undef;
//
// open/create necessary files and transfer file headers
//
    let tmp_file = cat(&[&file, &s("_tmp")]);
    if !sn1(it, "open(INFILE, $f) ? 1 : 0", &file, &u)?.truthy() {
        pl::warn(it, &format!("Can't open {}\n", file.to_text()))?;
        return ret(n(0));
    }
    if !sn1(it, r#"open(OUTFILE, ">$f") ? 1 : 0"#, &tmp_file, &u)?.truthy() {
        pl::warn(it, &format!("Can't create temporary file {}\n", tmp_file.to_text()))?;
        close(it, "INFILE")?;
        return ret(n(0));
    }
    let mut success = false;
    loop {
        let line = read_line(it, "INFILE")?;
        if line.is_undef() {
            break;
        }
        if !sn1(it, "print OUTFILE $f", &line, &u)?.truthy() {
            break;
        }
        if re(r"^#\+{4} Begin", "").is_match(&line) {
            success = true;
            break;
        }
    }
    let out = |it: &mut Interp, v: Value| -> ER<bool> { Ok(sn1(it, "print OUTFILE $f", &v, &Value::Undef)?.truthy()) };
    out(it, s("\n# list of tables containing writable tags\n"))?;
    out(it, s("my @tableList = (\n"))?;

//
// write table list
//
    let all_tables = pl::pkg_hash(it, "Image::ExifTool::allTables");
    let table_names = sorted_keys(&all_tables);
    let wr_num = Value::new_hash(); // translate from allTables index to writable tables index
    let mut count = 0i64;
    let mut num = 0i64;
    for table_name in &table_names {
        if table_writable.get(table_name.to_bytes()).truthy() {
            out(it, cat(&[&s("\t'"), table_name, &s("',\n")]))?;
            wr_num.set(count.to_string(), n(num));
            num += 1;
        }
        count += 1;
    }
//
// write the tag lookup table
//
    // verify that certain critical tag names aren't duplicated
    for tag in ["filename", "directory"] {
        if !tag_lookup.get(tag).truthy() {
            continue;
        }
        let nk = tag_lookup.get(tag).keys().len();
        if nk > 1 {
            pl::warn(it, &format!("Warning: {nk} writable '{tag}' tags!\n"))?;
        }
    }
    out(it, s(");\n\n# lookup for all writable tags\nmy %tagLookup = (\n"))?;
    let esc_re = re(r"([\x00-\x1f,\x7f-\xff])", "");
    let esc = |m: &pl::Caps| format!("\\x{:02x}", cap(m, 1).to_bytes()[0]).into_bytes();
    for tag in sorted_keys(&tag_lookup) {
        out(it, cat(&[&s("\t'"), &tag, &s("' => { ")]))?;
        let table_nums = sort_num(keys(&tag_lookup.get(tag.to_bytes())));
        let mut entries: Vec<Value> = Vec::new();
        for table_num in table_nums {
            let mut tag_id = tag_lookup.get(tag.to_bytes()).get(table_num.to_bytes());
            let root_id = viv(&flattened, tag.to_bytes()).get(table_num.to_bytes());
            let entry;
            if rtype(&tag_id) == "HASH" || root_id.truthy() {
                if rtype(&tag_id) != "HASH" {
                    let h = Value::new_hash();
                    h.set(tag_id.to_bytes(), n(1));
                    tag_id = h;
                }
                let mut tag_ids = sorted_keys(&tag_id);
                for t in tag_ids.iter_mut() {
                    if re(r"^\d+$", "").is_match(t) {
                        *t = pl::sprintf("0x%x", &[t.clone()]);
                    } else {
                        let mut quot = s("'");
                        // escape non-printable characters in tag ID if necessary
                        if esc_re.subst(t, true, esc) > 0 {
                            quot = s("\"");
                        }
                        *t = cat(&[&quot, t, &quot]);
                    }
                }
                // reference to root structure ID must come first in lookup
                // (so we can generate the flattened tags just before we need them)
                if root_id.truthy() {
                    tag_ids.insert(0, cat(&[&s("\\'"), &root_id, &s("'")]));
                }
                entry = cat(&[&s("["), &pl::join(",", &tag_ids), &s("]")]);
            } else if re(r"^\d+$", "").is_match(&tag_id) {
                entry = pl::sprintf("0x%x", &[tag_id.clone()]);
            } else {
                let mut quot = s("'");
                // escape non-printable characters in tag ID if necessary
                if esc_re.subst(&mut tag_id, true, esc) > 0 {
                    quot = s("\"");
                }
                entry = cat(&[&quot, &tag_id, &quot]);
            }
            let wn = wr_num.get(table_num.to_bytes());
            entries.push(cat(&[&wn, &s(" => "), &entry]));
        }
        out(it, pl::join(", ", &entries))?;
        out(it, s(" },\n"))?;
    }
//
// write tag exists lookup
//
    out(it, s(");\n\n# lookup for non-writable tags to check if the name exists\n"))?;
    out(it, s("my %tagExists = (\n"))?;
    for tag in sorted_keys(&tag_exists) {
        if tag_lookup.get(tag.to_bytes()).truthy() {
            continue;
        }
        out(it, cat(&[&s("\t'"), &tag, &s("' => 1,\n")]))?;
    }
//
// write module lookup for writable composite tags
//
    let composite_modules = self_.get("COMPOSITE_MODULES");
    out(it, s(");\n\n# module names for writable Composite tags\n"))?;
    out(it, s("my %compositeModules = (\n"))?;
    for k in sorted_keys(&composite_modules) {
        out(it, cat(&[&s("\t'"), &k, &s("' => '"), &composite_modules.get(k.to_bytes()), &s("',\n")]))?;
    }
    out(it, s(");\n\n"))?;
//
// finish writing TagLookup.pm and clean up
//
    if success {
        success = false;
        loop {
            let line = read_line(it, "INFILE")?;
            if line.is_undef() {
                break;
            }
            if !success && !re(r"^#\+{4} End", "").is_match(&line) {
                continue;
            }
            out(it, line)?;
            success = true;
        }
    }
    close(it, "INFILE")?;
    if !close(it, "OUTFILE")? {
        success = false;
    }
//
// return success code
//
    if success {
        // local (*ORG, *TMP);
        // only rename the file if something changed
        if !sn1(it, "open(ORG, $f) ? 1 : 0", &file, &u)?.truthy() {
            return ret(n(0));
        }
        if !sn1(it, "open(TMP, $f) ? 1 : 0", &tmp_file, &u)?.truthy() {
            return ret(n(0));
        }
        let mut changed = false;
        loop {
            let r1 = sn(it, "my $b; my $n = read ORG, $b, 65536; ($n, $b)", &u, &u)?;
            let r2 = sn(it, "my $b; my $n = read TMP, $b, 65536; ($n, $b)", &u, &u)?;
            let (n1, buff) = (at(&r1, 0), at(&r1, 1));
            let (n2, buf2) = (at(&r2, 0), at(&r2, 1));
            if !eq(&n1, &n2) {
                changed = true;
                break;
            }
            if !n1.truthy() {
                break;
            }
            if !eq(&buff, &buf2) {
                changed = true;
                break;
            }
        }
        close(it, "ORG")?;
        close(it, "TMP")?;
        if changed {
            if !sn1(it, "rename($f, $g)", &tmp_file, &file)?.truthy() {
                pl::warn(it, &format!("Error renaming {}\n", tmp_file.to_text()))?;
                success = false;
            }
        } else {
            sn(it, "unlink($f)", &tmp_file, &u)?;
        }
    } else {
        sn(it, "unlink($f)", &tmp_file, &u)?;
        pl::warn(it, "Error rewriting file\n")?;
    }
    ret(if success { n(1) } else { n(0) })
}

//------------------------------------------------------------------------------
/// Sort numbers first numerically, then strings alphabetically
/// - case-insensitive sorting set by global variable $caseInsensitive
/// - two global variables are used to change the sort algorithm:
///   $numbersFirst: -1 = put numbers after other strings
///                   1 = put numbers before other strings
///                   2 = put numbers first, but negative numbers last
pub fn numbers_first(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let nf = my_scalar(it, "numbersFirst");
    let ci = my_scalar(it, "caseInsensitive").truthy();
    ret(numbers_first_rs(&arg(&a, 0), &arg(&a, 1), &nf, ci))
}

fn numbers_first_rs(a: &Value, b: &Value, numbers_first: &Value, case_insensitive: bool) -> Value {
    let num_re = re(r"^(-?[0-9]+)(\.(\d*))?$", "");
    let mut rtn_val;
    let (mut b_num, mut b_dec) = (Value::Undef, Value::Undef);
    if let Some(c) = num_re.m(b) {
        b_num = cap(&c, 1);
        b_dec = cap(&c, 3);
    }
    if let Some(c) = num_re.m(a) {
        if !b_num.is_undef() {
            if pl::num_eq(numbers_first, &n(2)) && pl::lt(&b_num, &n(0)) {
                b_num = pl::add(&b_num, &Value::Num(1e9));
            }
            let mut a_int = cap(&c, 1);
            if pl::num_eq(numbers_first, &n(2)) && pl::lt(&a_int, &n(0)) {
                a_int = pl::add(&a_int, &Value::Num(1e9));
            }
            // compare integer part as a number
            rtn_val = spaceship(&a_int, &b_num);
            if !rtn_val.truthy() {
                let a_dec = if cap(&c, 3).truthy() { cap(&c, 3) } else { n(0) };
                if !b_dec.truthy() {
                    b_dec = n(0);
                }
                // compare decimal part as an integer too
                // (so that "1.10" comes after "1.9")
                rtn_val = spaceship(&a_dec, &b_dec);
            }
        } else {
            rtn_val = pl::neg(numbers_first);
        }
    } else if !b_num.is_undef() {
        rtn_val = numbers_first.clone();
    } else {
        let (mut a2, mut b2) = (a.clone(), b.clone());
        // expand numbers to 3 digits (with restrictions to avoid messing up
        // ascii-hex tags -- Nikon LensID's are 23 characters long)
        let exp_re = re(r"^(APP|DMC-\w+ |dvtm_.*|(IDB|SHB)-)?[.0-9 ]*$", "");
        if exp_re.is_match(&a2) && len(&a2) < 23 {
            re(r"(\d+)", "").subst(&mut a2, true, |c| pl::sprintf("%.3d", &[cap(c, 1)]).to_bytes().to_vec());
        }
        if exp_re.is_match(&b2) && len(&b2) < 23 {
            re(r"(\d+)", "").subst(&mut b2, true, |c| pl::sprintf("%.3d", &[cap(c, 1)]).to_bytes().to_vec());
        }
        rtn_val = Value::Undef;
        if case_insensitive {
            rtn_val = cmp_val(ops::str_cmp(&pl::lc(&a2), &pl::lc(&b2)));
        }
        if !rtn_val.truthy() {
            rtn_val = cmp_val(ops::str_cmp(&a2, &b2));
        }
    }
    rtn_val
}

/// `$x <=> $y` (undef for NaN)
fn spaceship(x: &Value, y: &Value) -> Value {
    match ops::num_cmp(x, y) {
        Some(o) => cmp_val(o),
        None => Value::Undef,
    }
}

//------------------------------------------------------------------------------
/// Convert our pod-like documentation to pod
/// (funny, I know, but the pod headings must be hidden to prevent confusing
///  the pod parser)
/// Inputs: 0-N) documentation strings
pub fn doc2pod(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |_| ret(doc2pod_rs(&a)))
}

fn doc2pod_rs(a: &[Value]) -> Value {
    let mut doc = arg(a, 0);
    for d in a.iter().skip(1) {
        doc = pl::concat(&doc, d);
    }
    re(r"\n~", "").subst(&mut doc, true, |_| b"\n=".to_vec());
    re(r"L<[^>]+?\|(http[^>]+)>", "").subst(&mut doc, true, |c| [&b"L<"[..], &cap(c, 1).to_bytes()[..], b">"].concat()); // POD doesn't support text for http links
    re(r"L<([^>]+?)\|[^>]+\.html(#\w+)?>", "").subst(&mut doc, true, |c| cap(c, 1).to_bytes().to_vec()); // remove relative HTML links
    doc
}

//------------------------------------------------------------------------------
/// Convert our pod-like documentation to html
/// Inputs: 0) string
pub fn doc2html(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(doc2html_rs(it, &arg(&a, 0))?)
}

fn doc2html_rs(it: &mut Interp, v: &Value) -> ER<Value> {
    let mut doc = escape_html(it, v)?;
    let home_page = my_scalar(it, "homePage");
    let hp_q = crate::perl::regex::quotemeta(&home_page.to_bytes());
    let c = |c: &pl::Caps, i: usize| cap(c, i).to_bytes().to_vec();
    re(r"\n\n", "").subst(&mut doc, true, |_| b"</p>\n\n<p>".to_vec());
    re(r"B&lt;(.*?)&gt;", "s").subst(&mut doc, true, |m| [&b"<b>"[..], &c(m, 1), b"</b>"].concat());
    re(r"C&lt;(.*?)&gt;", "s").subst(&mut doc, true, |m| [&b"<code>"[..], &c(m, 1), b"</code>"].concat());
    re(r"I&lt;(.*?)&gt;", "s").subst(&mut doc, true, |m| [&b"<i>"[..], &c(m, 1), b"</i>"].concat());
    // L<some text|https://exiftool.org/struct.html#Fields> --> <a href="../struct.html#Fields">some text</a>
    dyn_re(&[br"L&lt;([^&]+?)\|", &hp_q, br"/(.*?)&gt;"], "s")?.subst(&mut doc, true, |m| [&b"<a href=\"../"[..], &c(m, 2), b"\">", &c(m, 1), b"</a>"].concat());
    // L<https://exiftool.org/struct.html> --> <a href="https://exiftool.org/struct.html">https://exiftool.org/struct.html</a>
    dyn_re(&[br"L&lt;", &hp_q, br"/(.*?)&gt;"], "s")?.subst(&mut doc, true, |m| [&b"<a href=\"../"[..], &c(m, 1), b"\">", &c(m, 1), b"</a>"].concat());
    // L<XMP DICOM Tags|Image::ExifTool::TagNames/XMP DICOM Tags> --> <a href="XMP.html#DICOM">XMP DICOM Tags</a>
    // (specify "Image::ExifTool::TagNames" to link to another html file)
    re(r"L&lt;([^&]+?)\|Image::ExifTool::TagNames/(\w+) ([^/&|]+) Tags&gt;", "s").subst(&mut doc, true, |m| [&b"<a href=\""[..], &c(m, 2), b".html#", &c(m, 3), b"\">", &c(m, 1), b"</a>"].concat());
    // L<DICOM Tags|Image::ExifTool::TagNames/DICOM Tags> --> <a href="DICOM.html">DICOM Tags</a>
    re(r"L&lt;([^&]+?)\|Image::ExifTool::TagNames/(\w+) Tags&gt;", "s").subst(&mut doc, true, |m| [&b"<a href=\""[..], &c(m, 2), b".html\">", &c(m, 1), b"</a>"].concat());
    // L<dc|/XMP dc Tags> --> <a href="#dc">dc</a>
    // (a relative POD link turns into a relative HTML link)
    re(r"L&lt;([^&]+?)\|/\w+ ([^/&|]+) Tags&gt;", "s").subst(&mut doc, true, |m| [&b"<a href=\"#"[..], &c(m, 2), b"\">", &c(m, 1), b"</a>"].concat());
    // L<sample config file|../config.html> --> <a href="../config.html">sample config file</a>
    re(r"L&lt;([^&]+?)\|(.+?)&gt;", "s").subst(&mut doc, true, |m| [&b"<a href=\""[..], &c(m, 2), b"\">", &c(m, 1), b"</a>"].concat());
    // L<http://some.web.site/> --> <a href="http://some.web.site">http://some.web.site</a>
    re(r"L&lt;(http.*?)&gt;", "s").subst(&mut doc, true, |m| [&b"<a href=\""[..], &c(m, 1), b"\">", &c(m, 1), b"</a>"].concat());
    Ok(doc)
}

//------------------------------------------------------------------------------
/// Tweak order of tables
/// Inputs: 0) table list ref, 1) reference to tweak hash
pub fn tweak_order(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| tweak_order_rs(it, &arg(&a, 0), &arg(&a, 1)))?;
    pl::ret_nothing(want)
}

fn tweak_order_rs(_it: &mut Interp, sorted_tables: &Value, tweak_order: &Value) -> ER<()> {
    let mut tweak = sorted_keys(tweak_order);
    let mut sorted: Vec<Value> = Vec::new();
    let (has_main, module) = (Value::new_hash(), Value::new_hash());
    // apply manual tweaks
    while !tweak.is_empty() {
        let table = tweak.remove(0);
        let first = tweak_order.get(table.to_bytes());
        if tweak_order.get(first.to_bytes()).truthy() {
            tweak.push(table); // must defer this till later
            continue;
        }
        tweak_order.delete(table.to_bytes()); // because the table won't move again
        let r = dyn_re(&[b"^Image::ExifTool::", &table.to_bytes(), br"\b"], "")?;
        let all = sorted_tables.items();
        let moving: Vec<Value> = all.iter().filter(|t| r.is_match(t)).cloned().collect();
        let mut not_moving: Vec<Value> = all.iter().filter(|t| !r.is_match(t)).cloned().collect();
        let mut after: Vec<Value> = Vec::new();
        let rf = dyn_re(&[b"^Image::ExifTool::", &first.to_bytes(), br"\b"], "")?;
        while let Some(last) = not_moving.last() {
            if rf.is_match(last) {
                break;
            }
            after.insert(0, not_moving.pop().unwrap());
        }
        let mut new_list = not_moving;
        new_list.extend(moving);
        new_list.extend(after);
        if let Value::ArrayRef(r) = sorted_tables {
            *r.borrow_mut() = new_list;
        }
    }
    // flag modules which have a "Main" table, and organize tables by module name
    for t in sorted_tables.items() {
        match re(r"^Image::ExifTool::(\w+)::(\w+)", "").m(&t) {
            None => sorted.push(t),
            Some(c) => {
                let m1 = cap(&c, 1);
                if pl::str_eq(&cap(&c, 2), "Main") {
                    has_main.set(m1.to_bytes(), n(1));
                }
                if !module.get(m1.to_bytes()).truthy() {
                    let arr = Value::new_array(vec![]);
                    module.set(m1.to_bytes(), arr.clone());
                    sorted.push(arr);
                }
                module.get(m1.to_bytes()).push(t);
            }
        }
    }
    // force MWG::Composite table first
    let mut mwg = vec![s("Image::ExifTool::MWG::Composite")];
    let mwg_arr = if module.get("MWG").truthy() { module.get("MWG") } else { module.get_or_new_array("MWG") };
    for t in mwg_arr.items() {
        if !re(r"Composite", "").is_match(&t) {
            mwg.push(t);
        }
    }
    if let Value::ArrayRef(r) = &mwg_arr {
        *r.borrow_mut() = mwg;
    }
    // make sure that the main table always comes first in each file
    // and that all other tables from this group follow
    let mut out: Vec<Value> = Vec::new();
    for entry in sorted {
        if !entry.is_ref() {
            out.push(entry);
            continue;
        }
        let Some(c) = re(r"^Image::ExifTool::(\w+)::(\w+)", "").m(&entry.at(0)) else {
            return die("Internal error");
        };
        let m1 = cap(&c, 1);
        let main = cat(&[&s("Image::ExifTool::"), &m1, &s("::Main")]);
        // main table must come first (even if it doesn't exist)
        if has_main.get(m1.to_bytes()).truthy() {
            out.push(main.clone());
        }
        for t in entry.items() {
            if !eq(&t, &main) {
                out.push(t);
            }
        }
    }
    if let Value::ArrayRef(r) = sorted_tables {
        *r.borrow_mut() = out;
    }
    Ok(())
}

//------------------------------------------------------------------------------
/// Get a list of sorted tag ID's from a table
/// Inputs: 0) tag table ref
/// Returns: list of sorted keys
pub fn sorted_tag_table_keys(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let k = sorted_tag_table_keys_rs(it, &arg(&a, 0))?;
    if want == Want::List {
        Ok(k)
    } else {
        pl::ret_undef()
    }
}

fn sorted_tag_table_keys_rs(it: &mut Interp, table: &Value) -> ER<Vec<Value>> {
    let vars = if table.get("VARS").truthy() { table.get("VARS") } else { Value::new_hash() };
    let mut keys_ = pl::call_list(it, "Image::ExifTool::TagTableKeys", vec![table.clone()])?;
    if vars.get("ID_FMT").truthy() && pl::str_eq(&vars.get("ID_FMT"), "none") {
        // sort by tag name if ID not shown
        let name = Value::new_hash();
        for key in &keys_ {
            let tag_info = get_tag_info_list(it, table, key)?.into_iter().next().unwrap_or_default();
            name.set(key.to_bytes(), tag_info.get("Name"));
        }
        keys_.sort_by(|x, y| ops::str_cmp(&name.get(x.to_bytes()), &name.get(y.to_bytes())).then_with(|| ops::str_cmp(x, y)));
        Ok(keys_)
    } else {
        let sort_proc = vars.get("SORT_PROC");
        if sort_proc.truthy() {
            sort_try(it, &mut keys_, |it, x, y| Ok(pl::call_code(it, &sort_proc, vec![x.clone(), y.clone()], Want::Scalar)?.pop().unwrap_or_default()))?;
        } else {
            sort_nf(it, &mut keys_);
        }
        Ok(keys_)
    }
}

//------------------------------------------------------------------------------
/// Get the order that we want to print the tables in the documentation
/// Inputs: 0-N) Extra tables to add at end
/// Returns: tables in the order we want
pub fn get_table_order(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let v = get_table_order_rs(it, a)?;
    if want == Want::List {
        Ok(v)
    } else {
        ret(n(v.len() as i64))
    }
}

fn get_table_order_rs(it: &mut Interp, extra: Vec<Value>) -> ER<Vec<Value>> {
    let got_table = Value::new_hash();
    let mut table_names = pl::pkg_array(it, "Image::ExifTool::tableOrder").items();
    let mut ordered_tables: Vec<Value> = Vec::new();
    let main_tables = Value::new_hash();
    let mut out_of_order: Vec<Value> = Vec::new();
    let mut last_table = s("");

    while !table_names.is_empty() {
        let mut table_name = table_names.remove(0);
        if got_table.get(table_name.to_bytes()).truthy() {
            continue;
        }
        if let Some(c) = re(r"^Image::ExifTool::(\w+)::Main", "").m(&table_name) {
            main_tables.set(cap(&c, 1).to_bytes(), n(1));
        } else if last_table.truthy() && !dyn_re(&[b"^", &last_table.to_bytes(), b"::"], "")?.is_match(&table_name) {
            out_of_order.push(table_name.clone());
        }
        last_table = match re(r"^(Image::ExifTool::\w+)", "").m(&table_name) {
            Some(c) => cap(&c, 1),
            None => Value::Undef,
        };
        ordered_tables.push(table_name.clone());
        got_table.set(table_name.to_bytes(), n(1));
        let table = get_tag_table(it, &table_name)?;
        // recursively scan through tables in subdirectories
        let mut more_tables: Vec<Value> = Vec::new();
        let ci = if table.get("GROUPS").truthy() { if pl::str_eq(&table.get("GROUPS").get("0"), "XMP") { n(1) } else { s("") } } else { table.get("GROUPS") };
        set_my_scalar(it, "caseInsensitive", ci);
        if table.get("VARS").truthy() && table.get("VARS").get("ALPHA_FIRST").truthy() {
            set_my_scalar(it, "numbersFirst", n(-1));
        }
        let keys_ = sorted_tag_table_keys_rs(it, &table)?;
        set_my_scalar(it, "numbersFirst", n(1));
        for k in &keys_ {
            let info_array = get_tag_info_list(it, &table, k)?;
            for tag_info in &info_array {
                let subdir = tag_info.get("SubDirectory");
                if !subdir.truthy() {
                    continue;
                }
                table_name = subdir.get("TagTable");
                if !table_name.truthy() {
                    continue;
                }
                if got_table.get(table_name.to_bytes()).truthy() {
                    continue; // next if table already loaded
                }
                more_tables.push(table_name.clone()); // must scan this one too
            }
        }
        more_tables.extend(table_names);
        table_names = more_tables;
    }
    // clean up the order for tables which are out of order
    // (groups all Canon and Kodak tables together)
    let fix_order = Value::new_hash();
    for t in &out_of_order {
        let Some(c) = re(r"^Image::ExifTool::(\w+)", "").m(t) else { continue };
        // only re-order tables which have a corresponding main table
        if !main_tables.get(cap(&c, 1).to_bytes()).truthy() {
            continue;
        }
        fix_order.set(cap(&c, 1).to_bytes(), Value::new_array(vec![])); // fix the order of these tables
    }
    let mut sorted_tables: Vec<Value> = Vec::new();
    let fix_pos = Value::new_hash();
    for t in &ordered_tables {
        let m = re(r"^Image::ExifTool::(\w+)", "").m(t);
        if let Some(c) = m.filter(|c| fix_order.get(cap(c, 1).to_bytes()).truthy()) {
            let m1 = cap(&c, 1);
            let fix = fix_order.get(m1.to_bytes());
            if fix.is_empty() {
                let pos = sorted_tables.len().to_string();
                if !fix_pos.get(&pos).truthy() {
                    fix_pos.set(&pos, Value::new_array(vec![]));
                }
                fix_pos.get(&pos).push(m1.clone());
            }
            fix.push(t.clone());
        } else {
            sorted_tables.push(t.clone());
        }
    }
    // insert back in better order
    let mut positions = keys(&fix_pos);
    positions.sort_by(|x, y| ops::num_cmp(y, x).unwrap_or(Ordering::Equal)); // (reverse sort)
    for pos in positions {
        let fix = fix_pos.get(pos.to_bytes());
        let p = pos.to_i64() as usize;
        for m in fix.items() {
            let ins = fix_order.get(m.to_bytes()).items();
            let p = p.min(sorted_tables.len());
            sorted_tables.splice(p..p, ins);
        }
    }
    // add the extra tables
    sorted_tables.extend(extra);
    // tweak the table order
    let st = Value::new_array(sorted_tables);
    let tweak = my_hash(it, "tweakOrder");
    pl::local_topic(it, |it| tweak_order_rs(it, &st, &tweak))?;

    Ok(st.items())
}

//------------------------------------------------------------------------------
/// Open HTMLFILE and print header and description
/// Inputs: 0) Filename, 1) optional category
/// Returns: True on success
pub fn open_html_file(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let ok = open_html_file_rs(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?;
    ret(n(ok as i64))
}

fn open_html_file_rs(it: &mut Interp, htmldir: &Value, category: &Value, sep_table: &Value) -> ER<bool> {
    let html_file;
    let mut head;
    let title;
    let mut url = Value::Undef;
    let class;
    let mut category = category.clone();
    let mut top = s("");
    let u = Value::Undef;

    if category.truthy() {
        let mut names = pl::split_ws(&category);
        class = if names.is_empty() { Value::Undef } else { names.remove(0) };
        html_file = cat(&[htmldir, &s("/TagNames/"), &class, &s(".html")]);
        head = category.clone();
        if re(r"^\S+ .+ Struct$", "").is_match(&head) {
            names.pop();
        } else {
            head = pl::concat(&head, &s(if sep_table.truthy() { " Values" } else { " Tags" }));
        }
        let mut t = head.clone();
        re(r" .* ", "").subst(&mut t, false, |_| b" ".to_vec());
        title = t;
        if !names.is_empty() {
            url = pl::join("_", &names);
        }
    } else {
        html_file = cat(&[htmldir, &s("/TagNames/index.html")]);
        category = s("ExifTool");
        class = s("ExifTool");
        head = s("ExifTool Tag Names");
        title = head.clone();
    }
    let created_files = my_hash(it, "createdFiles");
    let docs = my_hash(it, "docs");
    if created_files.get(html_file.to_bytes()).truthy() {
        if !sn1(it, r#"open(HTMLFILE, ">>${f}_tmp") ? 1 : 0"#, &html_file, &u)?.truthy() {
            return Ok(false);
        }
    } else {
        if !sn1(it, r#"open(HTMLFILE, ">${f}_tmp") ? 1 : 0"#, &html_file, &u)?.truthy() {
            return Ok(false);
        }
        let doc_type = my_scalar(it, "docType");
        hp(it, cat(&[&doc_type, &s("<html>\n")]))?;
        hp(it, s("<!-- (this file generated automatically by Image::ExifTool::BuildTagLookup) -->\n"))?;
        hp(it, cat(&[&s("<head>\n<title>"), &title, &s("</title>\n")]))?;
        hp(it, s("<link rel=stylesheet type='text/css' href='style.css' title='Style'>\n"))?;
        hp(it, s("</head>\n<body>\n"))?;
        if !eq(&category, &class) && docs.get(class.to_bytes()).truthy() {
            if !hp(it, cat(&[&s("<h2 class=top>"), &class, &s(" Tags</h2>\n")]))? {
                return Ok(false);
            }
            let d = doc2html_rs(it, &docs.get(class.to_bytes()))?;
            if !hp(it, cat(&[&s("<p>"), &d, &s("</p>\n")]))? {
                return Ok(false);
            }
        } else {
            top = s(" class=top");
        }
    }
    if url.truthy() {
        head = cat(&[&s("<a name='"), &url, &s("'>"), &head, &s("</a>")]);
    }
    if !hp(it, cat(&[&s("<h2"), &top, &s(">"), &head, &s("</h2>\n")]))? {
        return Ok(false);
    }
    if docs.get(category.to_bytes()).truthy() {
        let d = doc2html_rs(it, &docs.get(category.to_bytes()))?;
        hp(it, cat(&[&s("<p>"), &d, &s("</p>\n")]))?;
    }
    created_files.set(html_file.to_bytes(), n(1));
    Ok(true)
}

//------------------------------------------------------------------------------
/// Close all html files and write trailers
/// Returns: true on success
/// Inputs: 0) BuildTagLookup object reference
pub fn close_html_files(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let ok = close_html_files_rs(it, &arg(&a, 0))?;
    ret(n(ok as i64))
}

fn close_html_files_rs(it: &mut Interp, self_: &Value) -> ER<bool> {
    let preserve_date = self_.get("PRESERVE_DATE");
    let mut success = true;
    let u = Value::Undef;
    // get the date
    let t = sn(it, "my @t = localtime; @t", &u, &u)?;
    let (day, mon, yr) = (at(&t, 3), at(&t, 4), at(&t, 5));
    let month = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let yr = pl::add(&yr, &n(1900));
    let mname = s(month.get(mon.to_i64() as usize).copied().unwrap_or(""));
    let date = cat(&[&mname, &s(" "), &day, &s(", "), &yr]);
    let mut count_new_files = 0i64;
    let mut count_same_files = 0i64;
    let created_files = my_hash(it, "createdFiles");
    let create_date = my_scalar(it, "createDate");
    for html_file in keys(&created_files) {
        let tmp_file = cat(&[&html_file, &s("_tmp")]);
        let mut file_date = date.clone();
        if preserve_date.truthy() {
            let lines = sn(it, "my @l = `grep '<i>Last revised' $f`; @l", &html_file, &u)?;
            if let Some(last) = lines.last() {
                if let Some(c) = re(r"<i>Last revised (.*)</i>", "").m(last) {
                    file_date = cap(&c, 1);
                }
            }
        }
        if !sn1(it, r#"open(HTMLFILE, ">>$f") ? 1 : 0"#, &tmp_file, &u)?.truthy() {
            success = false;
            continue;
        }
        // write the trailers
        hp(it, s("<hr>\n"))?;
        hp(it, s("(This document generated automatically by Image::ExifTool::BuildTagLookup)\n"))?;
        if pl::str_eq(&html_file, "html/TagNames/index.html") {
            hp(it, cat(&[&s("<br><i>Created "), &create_date, &s("</i>\n")]))?;
        }
        hp(it, cat(&[&s("<br><i>Last revised "), &file_date, &s("</i>\n")]))?;
        hp(it, s("<p class=lf><a href="))?;
        if re(r"index\.html$", "").is_match(&html_file) {
            hp(it, s("'../index.html'>&lt;-- Back to ExifTool home page</a></p>\n"))?;
        } else {
            hp(it, s("'index.html'>&lt;-- ExifTool Tag Names</a></p>\n"))?;
        }
        if !hp(it, s("</body>\n</html>\n"))? {
            success = false;
        }
        if !close(it, "HTMLFILE")? {
            success = false;
        }
        // check for differences and only use new file if it was changed
        // (so the date only gets updated if changes were really made)
        let mut use_new_file = false;
        if success {
            if !sn1(it, "open(TEMPFILE, $f) ? 1 : 0", &tmp_file, &u)?.truthy() {
                success = false;
                break;
            }
            if sn1(it, "open(HTMLFILE, $f) ? 1 : 0", &html_file, &u)?.truthy() {
                loop {
                    let line = read_line(it, "HTMLFILE")?;
                    if line.is_undef() {
                        break;
                    }
                    let new_line = read_line(it, "TEMPFILE")?;
                    if !new_line.is_undef() {
                        if re(r"^<br><i>Last revised", "").is_match(&line) {
                            continue;
                        }
                        if eq(&line, &new_line) {
                            continue;
                        }
                    }
                    // files are different -- use the new file
                    use_new_file = true;
                    break;
                }
                if !read_line(it, "TEMPFILE")?.is_undef() {
                    use_new_file = true;
                }
                close(it, "HTMLFILE")?;
            } else {
                use_new_file = true;
            }
            close(it, "TEMPFILE")?;
            if use_new_file {
                count_new_files += 1;
                if !sn1(it, "rename($f, $g)", &tmp_file, &html_file)?.truthy() {
                    pl::warn(it, "Error renaming temporary file\n")?;
                    success = false;
                }
            } else {
                count_same_files += 1;
                sn(it, "unlink($f)", &tmp_file, &u)?; // erase new file and use existing file
            }
        }
        if !success {
            break;
        }
    }
    // save number of files processed so we can check the results later
    viv(self_, "COUNT").set("HTML files changed", n(count_new_files));
    viv(self_, "COUNT").set("HTML files unchanged", n(count_same_files));
    Ok(success)
}

//------------------------------------------------------------------------------
/// Get bitmask for POD documentation
/// Inputs: mask string from HTML docs
/// Returns: mask string for POD, or ''
pub fn pod_mask(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(pod_mask_rs(&arg(&a, 0)))
}

fn pod_mask_rs(mask: &Value) -> Value {
    let Some(c) = re(r"^\[val( >> (\d+))? \& (0x[\da-f]+)\]", "").m(mask) else {
        return s("");
    };
    let sh = if cap(&c, 2).truthy() { cap(&c, 2) } else { n(0) };
    pl::sprintf(" & 0x%.2x", &[ops::shift_left(&hex(&cap(&c, 3)), &sh)])
}

//------------------------------------------------------------------------------
/// Write the TagName HTML and POD documentation
/// Inputs: 0) BuildTagLookup object reference
///         1) output pod file (eg. 'lib/Image/ExifTool/TagNames.pod')
///         2) output html directory (eg. 'html')
/// Returns: true on success
/// Notes: My apologies for the patchwork code, but this is only used to generate the docs.
pub fn write_tag_names(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (self_, pod_file, htmldir) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let u = Value::Undef;
    let mut sep_tables: Vec<Value> = Vec::new();
    let mut structs_l: Vec<Value> = Vec::new();
    let tag_name_info = self_.get("TAG_NAME_INFO");
    if !tag_name_info.truthy() {
        return ret(n(0));
    }
    let id_label = self_.get("ID_LOOKUP");
    let short_name = self_.get("SHORT_NAME");
    let sep_table = self_.get("SEPARATE_TABLE");
    let structs = self_.get("STRUCTURES");
    let struct_lookup = self_.get("STRUCT_LOOKUP");
    let mut success = true;
    let columns = 6i64; // number of columns in html index
    let percent = 100 / columns;
    let docs = my_hash(it, "docs");
    let note_font = my_scalar(it, "noteFont");
    let note_font_small = my_scalar(it, "noteFontSmall");
    let riff_spec = my_hash(it, "riffSpec");
    let xmp_spec = my_hash(it, "xmpSpec");
    let exif_spec = pl::pkg_hash(it, "Image::ExifTool::Validate::exifSpec");
    let std_xlat_ns = pl::pkg_hash(it, "Image::ExifTool::XMP::stdXlatNS");

    // open the file and write the header
    if !sn1(it, r#"open(PODFILE, ">$f") ? 1 : 0"#, &pod_file, &u)?.truthy() {
        return ret(n(0));
    }
    {
        let code = cat(&[&s("\""), &docs.get("ExifTool"), &s("\"")]);
        let scope = runtime::file_scope(it, MODULE);
        let (v, _) = pl::eval_site_in(it, &code, MODULE, Some(scope), &[("self", self_.clone())], Want::Scalar);
        docs.set("ExifTool", v);
    }
    pp(it, doc2pod_rs(&[docs.get("PodHeader"), docs.get("ExifTool"), docs.get("ExifTool2")]))?;
    sn(it, r#"mkdir "$f/TagNames", 0777"#, &htmldir, &u)?;
    if !open_html_file_rs(it, &htmldir, &u, &u)? {
        return ret(n(0));
    }
    hp(it, s("<blockquote>\n"))?;
    hp(it, s("<table width='100%' class=frame><tr><td>\n"))?;
    hp(it, s("<table width='100%' class=inner cellspacing=1><tr class=h>\n"))?;
    hp(it, s(&format!("<th colspan={columns}><span class=l>Tag Table Index</span></th></tr>\n")))?;
    hp(it, s(&format!("<tr class=b><td width='{percent}%'>\n")))?;

    // get the full table list, adding shortcuts and plug-in tables to the end
    let mut extra = vec![s("Image::ExifTool::Shortcuts::Main")];
    extra.extend(my_array(it, "pluginTables").items());
    let mut table_names = get_table_order_rs(it, extra)?;

    // get list of headings and add any missing ones
    let mut heading = s("xxx");
    let mut table_index_names: Vec<Value> = Vec::new();
    let mut headings: Vec<Value> = Vec::new();
    for table_name in &table_names {
        let short = short_name.get(table_name.to_bytes());
        let mut names = pl::split_ws(&short);
        let class = if names.is_empty() { Value::Undef } else { names.remove(0) };
        if !names.is_empty() && !pl::str_eq(&class, "Other") {
            // add heading for tables without a Main
            if !eq(&heading, &class) {
                heading = class.clone();
                table_index_names.push(heading.clone());
                headings.push(heading.clone());
            }
        } else {
            heading = short.clone();
            headings.push(heading.clone());
        }
        table_index_names.push(table_name.clone());
    }
    table_names = table_index_names;
    // print html index of headings only
    let mut count = 0i64;
    let lines = (headings.len() as i64 + columns - 1) / columns;
    for table_name in &headings {
        if count != 0 {
            if count % lines != 0 {
                hp(it, s("<br>\n"))?;
            } else {
                hp(it, s(&format!("</td><td width='{percent}%'>\n")))?;
            }
        }
        let mut short = short_name.get(table_name.to_bytes());
        if !short.truthy() {
            short = table_name.clone();
        }
        let url;
        // handle various tables in "Other.pm"
        if let Some(c) = re(r"^Other (.*)", "").m(&short) {
            short = cap(&c, 1);
            url = cat(&[&s("Other.html#"), &cap(&c, 1)]);
        } else {
            url = cat(&[&short, &s(".html")]);
        }
        hp(it, cat(&[&s("<a href='"), &url, &s("'>"), &short, &s("</a>")]))?;
        count += 1;
    }
    hp(it, s("\n</td></tr></table></td></tr></table></blockquote>\n"))?;
    let d = doc2html_rs(it, &docs.get("ExifTool2"))?;
    hp(it, cat(&[&s("<p>"), &d, &s("</p>\n")]))?;

    // write all the tag tables
    while !table_names.is_empty() || !sep_tables.is_empty() || !structs_l.is_empty() {
        while !sep_tables.is_empty() {
            let table_name = sep_tables.remove(0);
            let print_conv = sep_table.get(table_name.to_bytes());
            if rtype(&print_conv) != "HASH" {
                continue;
            }
            sep_table.set(table_name.to_bytes(), n(1));
            let mut notes = print_conv.get("Notes");
            if notes.truthy() {
                // remove unnecessary whitespace
                strip(&mut notes, r"^\s+", "");
                strip(&mut notes, r"\s+$", "");
                re(r"(^[ \t]+|[ \t]+$)", "m").subst(&mut notes, true, |_| Vec::new());
            }
            let mut head = table_name.clone();
            strip(&mut head, r"^.* ", "s");
            close(it, "HTMLFILE")?;
            if open_html_file_rs(it, &htmldir, &table_name, &n(1))? {
                if notes.truthy() {
                    let d = doc2html_rs(it, &notes)?;
                    hp(it, cat(&[&s("<p>"), &d, &s("</p>\n")]))?;
                }
                hp(it, s("<blockquote>\n"))?;
                hp(it, s("<table class=frame><tr><td>\n"))?;
                hp(it, s("<table class='inner sep' cellspacing=1>\n"))?;
                let mut align = s(" class=r");
                let mut wid = 0i64;
                let mut keys_v: Vec<Value> = Vec::new();
                let mut bits_v: Vec<Value> = Vec::new();
                let mut pk = keys(&print_conv);
                sort_nf(it, &mut pk);
                for k in pk {
                    if re(r"^(Notes|PrintHex|PrintInt|PrintString|OTHER|BITMASK)$", "").is_match(&k) {
                        continue;
                    }
                    if align.truthy() && re(r"[^\d]", "").is_match(&k) {
                        align = s("");
                    }
                    let w = len(&k) + len(&print_conv.get(k.to_bytes()));
                    if wid < w {
                        wid = w;
                    }
                    keys_v.push(k.clone());
                    if !re(r"[\0-\x1f\x7f-\xff]", "").is_match(&print_conv.get(k.to_bytes())) {
                        continue;
                    }
                    pl::warn(it, &format!("Warning: Special characters in {} PrintConv ({})\n", table_name.to_text(), print_conv.get(k.to_bytes()).to_text()))?;
                }
                let bits = print_conv.get("BITMASK");
                if bits.truthy() {
                    align = s("");
                    let mut bk = keys(&bits);
                    sort_nf(it, &mut bk);
                    for k in bk {
                        let w = len(&k) + len(&bits.get(k.to_bytes())) + 4;
                        if wid < w {
                            wid = w;
                        }
                        bits_v.push(k.clone());
                        if !re(r"[\0-\x1f\x7f-\xff]", "").is_match(&bits.get(k.to_bytes())) {
                            continue;
                        }
                        pl::warn(it, &format!("Warning: Special characters in {} PrintConv BITMASK ({})\n", table_name.to_text(), bits.get(k.to_bytes()).to_text()))?;
                    }
                }
                if wid < len(&table_name) + 7 {
                    wid = len(&table_name) + 7;
                }
                // print in multiple columns if there is room
                let mut cols = 110 / (wid + 4);
                let items = (keys_v.len() + bits_v.len()) as i64;
                if cols < 1 || cols > items || items < 4 {
                    cols = 1;
                }
                let rows = (items + cols - 1) / cols;
                hp(it, s("<tr class=h>"))?;
                for _c in 0..cols {
                    hp(it, cat(&[&s("<th>Value</th><th>"), &head, &s("</th>")]))?;
                }
                hp(it, s("</tr>\n"))?;
                for r in 0..rows {
                    hp(it, s("<tr>"))?;
                    for c in 0..cols {
                        let (mut key, mut val) = (Value::Undef, Value::Undef);
                        let mut bit = false;
                        let nn = r + c * rows;
                        if nn < keys_v.len() as i64 {
                            key = keys_v[nn as usize].clone();
                            val = print_conv.get(key.to_bytes());
                        } else if nn < items {
                            bit = true;
                            let b = &bits_v[nn as usize - keys_v.len()];
                            key = cat(&[&s("Bit "), b]);
                            val = bits.get(b.to_bytes());
                        }
                        let mut index;
                        let prt;
                        if !key.is_undef() {
                            index = key.clone();
                            prt = cat(&[&s("= "), &escape_html(it, &val)?]);
                            if print_conv.get("PrintInt").truthy() {
                                strip(&mut index, r"\.\d+$", "");
                            }
                            if print_conv.get("PrintHex").truthy() {
                                let mut dec = Value::Undef;
                                re(r"(\.\d+)$", "").subst(&mut index, false, |m| {
                                    dec = cap(m, 1);
                                    Vec::new()
                                }); // remove decimal
                                index = pl::sprintf("0x%x", &[index.clone()]);
                                if dec.truthy() {
                                    index = pl::concat(&index, &dec); // add back decimal
                                }
                            } else if print_conv.get("PrintString").truthy() || !re(r"^[+-]?(?=\d|\.\d)\d*(\.\d*)?$", "").is_match(&index) {
                                index = escape_html(it, &index)?;
                                if !bit {
                                    index = cat(&[&s("'"), &index, &s("'")]);
                                }
                            }
                        } else {
                            index = s("&nbsp;");
                            prt = s("&nbsp;");
                        }
                        let ic;
                        let pc;
                        if c & 0x01 != 0 {
                            pc = s(" class=b");
                            ic = if align.truthy() { s(" class='r b'") } else { pc.clone() };
                        } else {
                            ic = align.clone();
                            pc = s("");
                        }
                        hp(it, cat(&[&s("<td"), &ic, &s(">"), &index, &s("</td><td"), &pc, &s(">"), &prt, &s("</td>\n")]))?;
                    }
                    hp(it, s("</tr>"))?;
                }
                hp(it, s("</table></td></tr></table></blockquote>\n\n"))?;
            }
        }
        if table_names.is_empty() && structs_l.is_empty() {
            break;
        }
        let mut is_struct = Value::Undef;
        let table_name;
        let short;
        if !structs_l.is_empty() {
            let mut tn = structs_l.remove(0);
            if pl::num_eq(&structs.get(tn.to_bytes()), &n(2)) {
                continue; // only list each structure once
            }
            structs.set(tn.to_bytes(), n(2));
            is_struct = struct_lookup.get(tn.to_bytes());
            if !is_struct.truthy() {
                pl::warn(it, &format!("Missing structure {}\n", tn.to_text()))?;
                continue;
            }
            if !re(r" ", "").is_match(&tn) {
                tn = cat(&[&s("XMP "), &tn]);
            }
            tn = cat(&[&tn, &s(" Struct")]);
            short = tn.clone();
            table_name = tn;
            let mut max_len = 0i64;
            for k in keys(&is_struct) {
                if max_len < len(&k) {
                    max_len = len(&k);
                }
            }
            viv(&self_, "LONG_ID").set(table_name.to_bytes(), n(max_len));
        } else {
            table_name = table_names.remove(0);
            short = short_name.get(table_name.to_bytes());
            if !short.truthy() {
                // this is just an index heading
                pp(it, cat(&[&s("\n=head2 "), &table_name, &s(" Tags\n")]))?;
                if docs.get(table_name.to_bytes()).truthy() {
                    pp(it, doc2pod_rs(&[docs.get(table_name.to_bytes())]))?;
                }
                continue;
            }
        }
        let is_exif = pl::str_eq(&table_name, "Image::ExifTool::Exif::Main");
        let is_riff = pl::str_eq(&table_name, "Image::ExifTool::RIFF::Info");
        let is_xmp_main = pl::str_eq(&table_name, "Image::ExifTool::XMP::Main");
        let info = tag_name_info.get(table_name.to_bytes());
        let mut id = id_label.get(table_name.to_bytes());
        let hid;
        let mut show_grp = false;
        // widths of the different columns in the POD documentation
        let (mut w_id, mut w_tag, w_req, mut w_grp) = (8i64, 36i64, 24i64, 10i64);
        let composite;
        let derived0;
        let mut notes = Value::Undef;
        let mut long_tags = Value::Undef;
        let mut prefix = Value::Undef;
        let mut was_long = 0i64;
        if pl::str_eq(&short, "Shortcuts") {
            derived0 = s("<th>Refers To</th>");
            composite = 2;
        } else if is_struct.truthy() {
            derived0 = s("");
            composite = 0;
            notes = is_struct.get("NOTES");
        } else {
            let table = get_tag_table(it, &table_name)?;
            notes = table.get("NOTES");
            if table.get("NAMESPACE").truthy() {
                let ns_v = std_xlat_ns.get(table.get("NAMESPACE").to_bytes());
                let ns = if ns_v.truthy() { ns_v } else { table.get("NAMESPACE") };
                let msg = cat(&[&s("These tags belong to the ExifTool XMP-"), &ns, &s(" family 1 group.")]);
                if notes.truthy() {
                    strip(&mut notes, r"\s+$", "");
                    notes = cat(&[&notes, &s("\n\n"), &msg]);
                } else {
                    notes = msg;
                }
            }
            if table.get("VARS").truthy() {
                long_tags = table.get("VARS").get("LONG_TAGS");
            }
            if pl::str_eq(&viv(&table, "GROUPS").get("0"), "Composite") {
                composite = 1;
                derived0 = s("<th>Derived From</th>");
            } else {
                composite = 0;
                derived0 = s("");
            }
        }
        let mut derived = derived0;
        let mut pod_id_len = viv(&self_, "LONG_ID").get(table_name.to_bytes()).to_i64();
        if notes.truthy() {
            // remove unnecessary whitespace
            strip(&mut notes, r"^\s+", "");
            strip(&mut notes, r"\s+$", "");
            re(r"(^[ \t]+|[ \t]+$)", "m").subst(&mut notes, true, |_| Vec::new());
            if let Some(c) = re(r"leading '(.*?_)' which", "").m(&notes) {
                prefix = cap(&c, 1);
                pod_id_len -= len(&prefix);
            }
        }
        if pod_id_len <= w_id {
            pod_id_len = w_id;
        } else if pl::str_eq(&short, "DICOM") {
            pod_id_len = 10;
        } else {
            // align tag names in secondary columns if possible
            let col = if pod_id_len <= 10 { 12 } else { 20 };
            if pod_id_len < col {
                pod_id_len = col;
            }
        }
        if id.truthy() {
            let mut h = cat(&[&s("<th>"), &id, &s("</th>")]);
            re(r" ", "").subst(&mut h, true, |_| b"&nbsp;".to_vec());
            hid = h;
            w_tag -= pod_id_len - w_id;
            w_id = pod_id_len;
            let long_tag = viv(&self_, "LONG_NAME").get(table_name.to_bytes()).to_i64();
            if w_tag < long_tag {
                if w_id - long_tag + w_tag >= 6 {
                    // don't let ID column get too narrow
                    w_id -= long_tag - w_tag;
                    w_tag = long_tag;
                }
                if w_id <= viv(&self_, "LONG_ID").get(table_name.to_bytes()).to_i64() {
                    was_long += 1;
                }
            }
        } else if composite != 0 {
            w_tag += w_id - w_req;
            hid = s("");
        } else {
            w_tag += 9;
            hid = s("");
        }
        if pl::str_eq(&short, "EXIF") || pl::str_eq(&short, "Extra") {
            derived = s("<th>Group</th>");
            show_grp = true;
            if pl::str_eq(&short, "Extra") {
                w_grp += 8; // tweak Group column width for "Extra" tags
            }
            w_tag -= w_grp + 1;
        }
        let head = if re(r" ", "").is_match(&short) { "head3" } else { "head2" };
        let str_ = if is_struct.truthy() { "" } else { " Tags" };
        pp(it, cat(&[&s(&format!("\n={head} ")), &short, &s(str_), &s("\n")]))?;
        if docs.get(short.to_bytes()).truthy() {
            pp(it, doc2pod_rs(&[docs.get(short.to_bytes())]))?;
        }
        if notes.truthy() {
            pp(it, cat(&[&s("\n"), &doc2pod_rs(&[notes.clone()]), &s("\n")]))?;
        }
        let mut line = s("\n");
        if id.truthy() {
            // shift over 'Index' heading by one character for a bit more balance
            if pl::str_eq(&id, "Index") {
                id = s(" Index");
            }
            line = pl::concat(&line, &sprintf_dyn(&format!("  %-{w_id}s"), &[id.clone()]));
        } else {
            line = pl::concat(&line, &s(" "));
        }
        let tag_name_heading = s(if pl::str_eq(&short, "XMP") {
            "Namespace"
        } else if is_struct.truthy() {
            "Field Name"
        } else {
            "Tag Name"
        });
        line = pl::concat(&line, &sprintf_dyn(&format!(" %-{w_tag}s"), &[tag_name_heading.clone()]));
        if composite != 0 {
            line = pl::concat(&line, &sprintf_dyn(&format!(" %-{w_req}s"), &[s(if composite == 2 { "Refers To" } else { "Derived From" })]));
        }
        if show_grp {
            line = pl::concat(&line, &sprintf_dyn(&format!(" %-{w_grp}s"), &[s("Group")]));
        }
        line = pl::concat(&line, &s(" Writable"));
        pp(it, line.clone())?;
        re(r"^(\s*\w.{6}\w) ", "").subst(&mut line, false, |c| [&cap(c, 1).to_bytes()[..], b"\t"].concat()); // change space to tab after long ID label (eg. "Sequence")
        re(r"\S", "").subst(&mut line, true, |_| b"-".to_vec());
        re(r"- -", "").subst(&mut line, true, |_| b"---".to_vec());
        pl::tr(&mut line, "\t", " ", ""); // change tab back to space
        pp(it, cat(&[&line, &s("\n")]))?;
        close(it, "HTMLFILE")?;
        if !open_html_file_rs(it, &htmldir, &short, &u)? {
            success = false;
        }
        if notes.truthy() {
            let d = doc2html_rs(it, &notes)?;
            hp(it, cat(&[&s("<p>"), &d, &s("</p>\n")]))?;
        }
        hp(it, s("<blockquote>\n"))?;
        hp(it, s("<table class=frame><tr><td>\n"))?;
        hp(it, s("<table class=inner cellspacing=1>\n"))?;
        hp(it, cat(&[&s("<tr class=h>"), &hid, &s("<th>"), &tag_name_heading, &s("</th>\n")]))?;
        hp(it, cat(&[&s("<th>Writable</th>"), &derived, &s("<th>Values / "), &note_font, &s("Notes</span></th></tr>\n")]))?;
        let mut row_class = s("1");
        let mut info_count = 0i64;
        for info_list in info.items() {
            info_count += 1;
            let il = info_list.items();
            let (mut tag_id_str, tag_names, writable, values, require, write_group) = (at(&il, 0), at(&il, 1), at(&il, 2), at(&il, 3), at(&il, 4), at(&il, 5));
            let align;
            let mut id_str = Value::Undef;
            let mut w;
            let tip;
            let mut w_tag2 = w_tag;
            if !id.truthy() {
                id_str = s("  ");
                align = Value::Undef;
            } else if re(r"^-?\d+(\.\d+)?$", "").is_match(&tag_id_str) {
                w = w_id - 3;
                let mut is = sprintf_dyn(&format!("  %{w}g    "), &[tag_id_str.clone()]);
                let mut too_long = len(&is) - 6 - w;
                if too_long != 0 {
                    if too_long > 3 {
                        too_long = 3;
                    }
                    is = pl::substr(&is, 0, Some(-too_long));
                }
                id_str = is;
                align = s(" class=r");
            } else {
                if prefix.truthy() {
                    let pb = prefix.to_bytes();
                    dyn_re(&[b"^'", &pb], "")?.subst(&mut tag_id_str, false, |_| b"'".to_vec());
                }
                w = w_id;
                let over = len(&tag_id_str) - w;
                if over > 0 {
                    // shift over tag name if there is room
                    if over <= w_tag - len(&tag_names.at(0)) {
                        w_tag2 -= over;
                        w += over;
                        was_long -= 1;
                    } else {
                        // put tag name on next line if ID is too long
                        id_str = cat(&[&s("  "), &tag_id_str, &s("\n   "), &s(&" ".repeat(w.max(0) as usize))]);
                        if long_tags.truthy() {
                            long_tags = pl::sub(&long_tags, &n(1));
                        } else {
                            pl::warn(it, &format!("Notice: Split {} line\n", tag_names.at(0).to_text()))?;
                        }
                    }
                }
                if id_str.is_undef() {
                    id_str = sprintf_dyn(&format!("  %-{w}s "), &[tag_id_str.clone()]);
                }
                align = s("");
            }
            let mut reqs: Vec<Value> = Vec::new();
            let mut tags = tag_names.items();
            let mut w_grp_l = write_group.items();
            let mut vals = writable.items();
            let mut wr_str = if vals.is_empty() { Value::Undef } else { vals.remove(0) };
            let mut subdir = false;
            let mask_re = re(r"^\[val( >> \d+)? \& 0x[\da-f]+\]", "");
            let mut masks: Vec<Value> = values.items().into_iter().filter(|v| mask_re.is_match(v)).collect();
            let tag = if tags.is_empty() { Value::Undef } else { tags.remove(0) };
            // if this is a subdirectory or structure, print subdir name (from values) instead of writable
            if re(r"^[-=]", "").is_match(&wr_str) {
                subdir = true;
                if !masks.is_empty() {
                    // combine any mask into the format string
                    wr_str = pl::concat(&wr_str, &pod_mask_rs(&masks[0]));
                    masks.remove(0);
                    vals = values.items().into_iter().filter(|v| !mask_re.is_match(v)).collect();
                } else {
                    vals = values.items();
                }
                // remove Notes if subdir has Notes as well
                if re(r"^\(", "").is_match(&at(&vals, 0)) && vals.len() >= writable.len() {
                    vals.remove(0);
                }
                for v in vals.iter_mut() {
                    if re(r"^\(", "").is_match(v) {
                        *v = s("-");
                    }
                }
                // fill in any missing entries from non-directory tags
                for i in 0..writable.len() {
                    while vals.len() <= i {
                        vals.push(Value::Undef);
                    }
                    if vals[i].is_undef() {
                        vals[i] = writable.at(i as i64);
                    }
                    if !masks.is_empty() {
                        vals[i] = pl::concat(&vals[i], &pod_mask_rs(&masks[0]));
                        masks.remove(0);
                    }
                }
                let v0 = at(&vals, 0);
                if sep_table.get(v0.to_bytes()).truthy() {
                    strip(&mut wr_str, r"^[-=]", "");
                    if !wr_str.truthy() {
                        wr_str = s("no");
                    }
                } else if structs.get(v0.to_bytes()).truthy() {
                    let flags = match re(r"([+_]+)$", "").m(&wr_str) {
                        Some(c) => cap(&c, 1),
                        None => s(""),
                    };
                    wr_str = cat(&[&v0, &s(" Struct"), &flags]);
                } else {
                    wr_str = v0;
                }
                if !vals.is_empty() {
                    vals.remove(0);
                }
            } else if wr_str.truthy() && !pl::str_eq(&wr_str, "no") && !masks.is_empty() {
                // fill in missing entries if masks are different
                let mask = masks.remove(0);
                while masks.len() > vals.len() {
                    if eq(&masks[vals.len()], &mask) {
                        break;
                    }
                    vals.push(wr_str.clone());
                    if tags.len() < vals.len() {
                        tags.push(tag.clone());
                    }
                }
                // add Mask to Writable column in POD doc
                wr_str = pl::concat(&wr_str, &pod_mask_rs(&mask));
            }
            let mut pod = sprintf_dyn(&format!("%s%-{w_tag2}s"), &[id_str.clone(), tag.clone()]);
            let mut t_grp = w_grp;
            if id.truthy() && len(&tag) > w_tag2 {
                let mut made_room = false;
                if show_grp {
                    let g0 = at(&w_grp_l, 0);
                    let w_grp0 = len(&if g0.truthy() { g0 } else { s("-") });
                    if composite == 0 && w_grp > w_grp0 {
                        t_grp = w_grp - (len(&tag) - w_tag2);
                        if t_grp < len(&n(w_grp0)) {
                            t_grp = len(&n(w_grp0));
                        } else {
                            made_room = true;
                        }
                    }
                }
                if !made_room {
                    pl::warn(it, &format!("Warning: Pushed {}\n", tag.to_text()))?;
                }
            }
            if show_grp {
                let g = if w_grp_l.is_empty() { Value::Undef } else { w_grp_l.remove(0) };
                pod = pl::concat(&pod, &sprintf_dyn(&format!(" %-{t_grp}s"), &[if g.truthy() { g } else { s("-") }]));
            }
            if composite != 0 {
                reqs = require.items();
                w = w_req; // Keep writable column in line
                if len(&tag) > w_tag2 {
                    w -= len(&tag) - w_tag2;
                }
                let r = if reqs.is_empty() { Value::Undef } else { reqs.remove(0) };
                pod = pl::concat(&pod, &sprintf_dyn(&format!(" %-{w}s"), &[if r.truthy() { r } else { s("") }]));
            }
            pod = cat(&[&pod, &s(" "), &wr_str]);
            // limit line length to 82 characters (even if it means messing up column alignment)
            if len(&pod) > 82 && !re(r"\n", "").is_match(&pod) {
                let remove = len(&pod) - 82;
                pl::re_dyn(format!(r"(\w)\s{{{remove}}}").as_bytes(), "")?.subst(&mut pod, false, |c| cap(c, 1).to_bytes().to_vec());
            }
            pp(it, cat(&[&pod, &s("\n")]))?;
            let num_tags = tag_names.len();
            let mut nn = 0usize;
            while !tags.is_empty() || !reqs.is_empty() || !vals.is_empty() {
                let more = !tags.is_empty() || !reqs.is_empty();
                line = s("  ");
                if id.truthy() {
                    line = pl::concat(&line, &s(&" ".repeat((w_id + 1).max(0) as usize)));
                }
                let t = if tags.is_empty() { Value::Undef } else { tags.remove(0) };
                line = pl::concat(&line, &sprintf_dyn(&format!("%-{w_tag2}s"), &[if t.truthy() { t } else { s("") }]));
                if composite != 0 {
                    let r = if reqs.is_empty() { Value::Undef } else { reqs.remove(0) };
                    line = pl::concat(&line, &sprintf_dyn(&format!(" %-{w_req}s"), &[if r.truthy() { r } else { s("") }]));
                }
                if show_grp {
                    let g = if w_grp_l.is_empty() { Value::Undef } else { w_grp_l.remove(0) };
                    line = pl::concat(&line, &sprintf_dyn(&format!(" %-{w_grp}s"), &[if g.truthy() { g } else { s("-") }]));
                }
                nn += 1;
                if !vals.is_empty() {
                    let mut val = vals.remove(0);
                    // use writable if this is a note
                    let wr = writable.at(nn as i64);
                    if subdir && (re(r"^\(", "").is_match(&val) || re(r"=", "").is_match(&val) || (wr.truthy() && !re(r"^[-=]", "").is_match(&wr))) {
                        val = wr.clone();
                        if !val.is_undef() {
                            strip(&mut val, r"^[-=]", "");
                        } else {
                            // done with tag if nothing else to print
                            if !more {
                                break;
                            }
                        }
                    }
                    if !val.is_undef() {
                        line = cat(&[&line, &s(" "), &val]);
                        if !masks.is_empty() {
                            line = pl::concat(&line, &pod_mask_rs(&masks[0]));
                            masks.remove(0);
                        }
                    }
                }
                strip(&mut line, r"\s+$", ""); // trim trailing white space
                pp(it, cat(&[&line, &s("\n")]))?;
            }
            let mut html_tags: Vec<Value> = Vec::new();
            for t in tag_names.items() {
                html_tags.push(escape_html(it, &t)?);
            }
            let spec = (is_exif && exif_spec.get(hex(&tag_id_str).to_bytes()).truthy())
                || (is_riff && re(r"(\w+)", "").m(&tag_id_str).is_some_and(|c| riff_spec.get(cap(&c, 1).to_bytes()).truthy()))
                || (is_xmp_main && re(r"([-\w]+)", "").m(&tag_id_str).is_some_and(|c| xmp_spec.get(cap(&c, 1).to_bytes()).truthy()));
            if spec && !html_tags.is_empty() {
                // underline "unknown" makernote tags only
                let i = if pl::str_eq(&tag_id_str, "0x927c") { html_tags.len() - 1 } else { 0 };
                html_tags[i] = cat(&[&s("<u>"), &html_tags[i], &s("</u>")]);
            }
            row_class = if row_class.truthy() { s("") } else { s(" class=b") };
            let mut is_subdir = false;
            if re(r"^[-=]", "").is_match(&writable.at(0)) {
                is_subdir = true;
                for i in 0..writable.len() {
                    let mut v = writable.at(i as i64);
                    if re(r"^[-=](.+)", "").subst(&mut v, false, |c| cap(c, 1).to_bytes().to_vec()) > 0 {
                        writable.set_at(i, v);
                    }
                }
            }
            // add tooltip for hex conversion of Tag ID
            if let Some(c) = re(r"^(0x[0-9a-f]+)(\.\d+)?$", "i").m(&tag_id_str) {
                let fmt = cat(&[&s(" title='"), &tag_id_str, &s(" = %u%s'")]);
                let d = if cap(&c, 2).truthy() { cap(&c, 2) } else { s("") };
                tip = pl::sprintf_v(&fmt, &[hex(&cap(&c, 1)), d]);
            } else if let Some(c) = re(r"^(\d+)(\.\d*)?$", "").m(&tag_id_str) {
                tip = pl::sprintf(" title='%u = 0x%x'", &[cap(&c, 1), cap(&c, 1)]);
            } else {
                tip = s("");
                // use copyright symbol in QuickTime UserData tags
                re(r#"^"\\xa9"#, "").subst(&mut tag_id_str, false, |_| b"\"&copy;".to_vec());
                // escape necessary characters in html
                re(r">", "").subst(&mut tag_id_str, true, |_| b"&gt;".to_vec());
                re(r"<", "").subst(&mut tag_id_str, true, |_| b"&lt;".to_vec());
            }
            // add tooltip for special writable attributes
            let mut wtip = s("");
            let wattr = [("_", "Flattened"), ("+", "List"), ("/", "Avoid"), ("~", "Writable only with -n"), ("!", "Unsafe"), ("*", "Protected"), (":", "Mandatory"), ("^", "Deletable")];
            let mut has_attr: Vec<u8> = Vec::new();
            for wstr in writable.items() {
                let Some(c) = re(r"([+/~!*:_^]+)$", "").m(&wstr) else { continue };
                for ch in cap(&c, 1).to_bytes().iter() {
                    if has_attr.contains(ch) {
                        continue;
                    }
                    has_attr.push(*ch);
                }
            }
            if !has_attr.is_empty() {
                let mut wt = String::from(" title='");
                for (i, ch) in has_attr.iter().enumerate() {
                    if i != 0 {
                        wt.push('\n');
                    }
                    let c = *ch as char;
                    let desc = wattr.iter().find(|(k, _)| k.as_bytes()[0] == *ch).map(|(_, d)| *d).unwrap_or("");
                    wt.push_str(&format!(" {c} = {desc}"));
                }
                wt.push('\'');
                wtip = s(&wt);
            }
            // print this row in the tag table
            hp(it, cat(&[&s("<tr"), &row_class, &s(">\n")]))?;
            if id.truthy() {
                hp(it, cat(&[&s("<td"), &align, &tip, &s(">"), &tag_id_str, &s("</td>\n")]))?;
            }
            hp(it, cat(&[&s("<td>"), &pl::join("\n  <br>", &html_tags), &s("</td>\n")]))?;
            hp(it, cat(&[&s("<td class=c"), &wtip, &s(">"), &pl::join("<br>", &writable.items()), &s("</td>\n")]))?;
            if composite != 0 {
                hp(it, cat(&[&s("<td class=n>"), &pl::join("\n  <br>", &require.items()), &s("</td>\n")]))?;
            }
            if show_grp {
                hp(it, cat(&[&s("<td class=c>"), &pl::join("<br>", &write_group.items()), &s("</td>\n")]))?;
            }
            hp(it, s("<td>"))?;
            if !values.is_empty() {
                if is_subdir {
                    let mut small_note = false;
                    let mut vout: Vec<Value> = Vec::new();
                    for i in 0..values.len() {
                        let mut v = values.at(i as i64);
                        if re(r"^\(", "").is_match(&v) {
                            v = doc2html_rs(it, &v)?;
                            values.set_at(i, v.clone());
                            // set the note font
                            if num_tags < 2 {
                                small_note = true;
                            }
                            vout.push(cat(&[if small_note { &note_font_small } else { &note_font }, &v, &s("</span>")]));
                            continue;
                        }
                        // make text in square brackets small
                        if re(r"^\[", "").is_match(&v) {
                            if strip(&mut v, r"^\[!HTML\]", "") > 0 {
                                values.set_at(i, v.clone());
                                vout.push(v);
                            } else {
                                v = doc2html_rs(it, &v)?;
                                values.set_at(i, v.clone());
                                vout.push(cat(&[&s("<span class=s>"), &v, &s("</span>")]));
                            }
                            continue;
                        }
                        if re(r"=", "").is_match(&v) {
                            vout.push(v);
                            continue;
                        }
                        let mut names = pl::split_ws(&v);
                        let mut suffix = " Tags";
                        if sep_table.get(v.to_bytes()).truthy() {
                            sep_tables.push(v.clone());
                            suffix = " Values";
                        }
                        // handle structures specially
                        if structs.get(v.to_bytes()).truthy() {
                            // assume XMP module for this struct unless otherwise specified
                            if !re(r" ", "").is_match(&v) {
                                names.insert(0, s("XMP"));
                            }
                            structs_l.push(v.clone()); // list this later
                            // hack to put Area Struct in with XMP tags,
                            // even though it is only used by the MWG module
                            if pl::str_eq(&v, "Dimensions") {
                                structs_l.push(s("Area"));
                            }
                            suffix = " Struct";
                        }
                        let first = if names.is_empty() { Value::Undef } else { names.remove(0) };
                        let mut url = cat(&[&first, &s(".html")]);
                        if !names.is_empty() {
                            url = cat(&[&url, &s("#"), &pl::join("_", &names)]);
                        }
                        vout.push(cat(&[&s("--&gt; <a href='"), &url, &s("'>"), &v, &s(suffix), &s("</a>")]));
                    }
                    // put small note last
                    if small_note && !vout.is_empty() {
                        let f = vout.remove(0);
                        vout.push(f);
                    }
                    hp(it, pl::join("\n  <br>", &vout))?;
                } else {
                    let (mut close_s, mut br) = (s(""), s(""));
                    for i in 0..values.len() {
                        let mut v = values.at(i as i64);
                        if strip(&mut v, r"^\[!HTML\]", "") > 0 {
                            values.set_at(i, v.clone());
                            if close_s.truthy() {
                                hp(it, close_s.clone())?;
                            }
                            hp(it, v)?;
                            close_s = s("");
                            br = s("");
                        } else {
                            if re(r"^\(", "").is_match(&v) {
                                // notes can use POD syntax
                                v = cat(&[&note_font, &doc2html_rs(it, &v)?, &s("</span>")]);
                            } else {
                                v = escape_html(it, &v)?;
                            }
                            if !close_s.truthy() {
                                v = cat(&[&s("<span class=s>"), &v]);
                                close_s = s("</span>");
                            }
                            values.set_at(i, v.clone());
                            hp(it, cat(&[&br, &v]))?;
                            br = s("\n  <br>");
                        }
                    }
                    if close_s.truthy() {
                        hp(it, close_s)?;
                    }
                }
            } else {
                hp(it, s("&nbsp;"))?;
            }
            hp(it, s("</td></tr>\n"))?;
        }
        if long_tags.truthy() {
            pl::warn(it, &format!("{} unaccounted-for long tags in {}\n", long_tags.to_text(), table_name.to_text()))?;
        }
        if was_long != 0 && long_tags.is_undef() {
            pl::warn(it, &format!("Notice: Long tags in {} table\n", table_name.to_text()))?;
        }
        if info_count == 0 {
            pp(it, s("  [no tags known]\n"))?;
            let mut cols = 3;
            if hid.truthy() {
                cols += 1;
            }
            if derived.truthy() {
                cols += 1;
            }
            hp(it, s(&format!("<tr><td colspan={cols} class=c><i>[no tags known]</i></td></tr>\n")))?;
        }
        hp(it, s("</table></td></tr></table></blockquote>\n\n"))?;
    }
    if !close(it, "HTMLFILE")? {
        success = false;
    }
    if !close_html_files_rs(it, &self_)? {
        success = false;
    }
    if !pp(it, doc2pod_rs(&[docs.get("PodTrailer")]))? {
        success = false;
    }
    if !close(it, "PODFILE")? {
        success = false;
    }
    ret(n(success as i64))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::BuildTagLookup::new", new);
    it.define_native("Image::ExifTool::BuildTagLookup::WriteTagLookup", write_tag_lookup);
    it.define_native("Image::ExifTool::BuildTagLookup::NumbersFirst", numbers_first);
    it.define_native("Image::ExifTool::BuildTagLookup::Doc2Pod", doc2pod);
    it.define_native("Image::ExifTool::BuildTagLookup::Doc2Html", doc2html);
    it.define_native("Image::ExifTool::BuildTagLookup::TweakOrder", tweak_order);
    it.define_native("Image::ExifTool::BuildTagLookup::SortedTagTableKeys", sorted_tag_table_keys);
    it.define_native("Image::ExifTool::BuildTagLookup::GetTableOrder", get_table_order);
    it.define_native("Image::ExifTool::BuildTagLookup::OpenHtmlFile", open_html_file);
    it.define_native("Image::ExifTool::BuildTagLookup::CloseHtmlFiles", close_html_files);
    it.define_native("Image::ExifTool::BuildTagLookup::PodMask", pod_mask);
    it.define_native("Image::ExifTool::BuildTagLookup::WriteTagNames", write_tag_names);
}
