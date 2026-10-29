//! Image::ExifTool::TagLookup (Image/ExifTool/TagLookup.pm), ported sub by sub (see docs/port/PORTING.md).
//! The lookup tables (`%tagLookup`, `@tableList`, `%tagExists`...) stay
//! the module's Perl data; these are its four subs.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, n, re, ret, s};
use crate::runtime;

const MODULE: &str = "Image::ExifTool::TagLookup";

/// A file lexical hash of TagLookup.pm (`my %tagLookup`...).
fn my_hash(it: &mut Interp, name: &str) -> Value {
    runtime::lex_hash(it, MODULE, name)
}

/// A file lexical array of TagLookup.pm (`my @tableList`).
fn my_array(it: &mut Interp, name: &str) -> Value {
    runtime::lex_array(it, MODULE, name)
}

/// `keys %$h` as values.
fn keys(h: &Value) -> Vec<Value> {
    h.keys().into_iter().map(Value::Str).collect()
}

/// `/^PAT$/` with PAT interpolated (no quotemeta).
fn anchored(p: &Value) -> Vec<u8> {
    let mut v = b"^".to_vec();
    v.extend_from_slice(&p.to_bytes());
    v.push(b'$');
    v
}

//------------------------------------------------------------------------------
/// Add tag names to lookup corresponding to flattened XMP tags for all structure fields
/// Inputs: 0) tag table ref for flattened tags, 1) tagID, 2) lowercase tag name,
///         3) tag ID list ref, 4) reference to list of lowercase tag names
///         5) table number in lookup, 6) tagInfo hash for parent structure
///         7) tag ID of top-level structure
/// Notes: Used only for user-defined structures
pub fn add_fields(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (tag_table_ptr, tag_id, lc_tag, tag_ids, lc_tags, tnum) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4), arg(&a, 5));
    let (mut tag_info, mut base_id) = (arg(&a, 6), arg(&a, 7));
    if pl::gt(&pl::length(&tag_id), &n(500)) {
        return pl::ret_nothing(want); // avoid deep recursion
    }
    if !tag_info.truthy() {
        tag_info = tag_table_ptr.get(tag_id.to_bytes());
        base_id = tag_id.clone();
    }
    let mut str_table = tag_info.get("Struct");
    if !str_table.is_ref() {
        let str_name = str_table.clone();
        str_table = pl::pkg_hash(it, "Image::ExifTool::UserDefined::xmpStruct").get(str_table.to_bytes());
        if !str_table.truthy() {
            pl::warn(it, &format!("No definition for structure '{}'\n", str_name.to_text()))?;
            return pl::ret_nothing(want);
        }
        if !str_table.get("STRUCT_NAME").truthy() {
            str_table.set("STRUCT_NAME", pl::concat(&s("XMP "), &str_name));
        }
        tag_info.set("Struct", str_table.clone()); // replace name with hash ref
        tag_info.delete("SubDirectory"); // deprecated use of SubDirectory in Struct tags
    }
    // inherit NAMESPACE from parent table if it doesn't exist
    if !str_table.exists("NAMESPACE") {
        str_table.set("NAMESPACE", tag_table_ptr.get("NAMESPACE"));
    }
    let special_struct = my_hash(it, "specialStruct");
    for field in keys(&str_table) {
        if special_struct.get(field.to_bytes()).truthy() {
            continue;
        }
        let id = pl::concat(&tag_id, &pl::ucfirst(&field));
        // use name of existing flattened tag if already defined
        let flat_info = tag_table_ptr.get(id.to_bytes());
        let field_info = str_table.get(field.to_bytes());
        let mut flat_name = Value::Undef;
        if pl::ref_of(&field_info) == "HASH" {
            flat_name = field_info.get("FlatName");
        }
        if !flat_name.truthy() {
            flat_name = field.clone();
            pl::tr(&mut flat_name, "-_a-zA-Z0-9", "", "dc"); // remove illegal characters
        }
        let lc = if flat_info.truthy() && flat_info.get("Name").truthy() { pl::lc(&flat_info.get("Name")) } else { pl::concat(&lc_tag, &pl::lc(&flat_name)) };
        let copy_id = base_id.clone();
        tag_ids.push(pl::sref(copy_id));
        tag_ids.push(id.clone());
        lc_tags.push(lc.clone());
        lc_tags.push(lc.clone());
        if !(pl::ref_of(&field_info) == "HASH" && field_info.get("Struct").truthy()) {
            continue;
        }
        // recursively add flattened tags for all sub-fields
        add_fields(it, vec![tag_table_ptr.clone(), id, lc, tag_ids.clone(), lc_tags.clone(), tnum.clone(), field_info, base_id.clone()], Want::Void)?;
    }
    pl::ret_nothing(want)
}

//------------------------------------------------------------------------------
/// Add tags to the lookup (for user-defined tags, etc)
/// Inputs: 0) tag table ref, 1) table name
pub fn add_tags(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let (mut tag_table_ptr, mut table) = (arg(&a, 0), arg(&a, 1));
    let table_num_hash = my_hash(it, "tableNumHash");
    let table_list = my_array(it, "tableList");
    let tag_lookup = my_hash(it, "tagLookup");

    // generate lookup for table numbers if not done already
    if table_num_hash.keys().is_empty() {
        let mut tnum = 0i64;
        for tbl in table_list.items() {
            table_num_hash.set(tbl.to_bytes(), n(tnum));
            tnum += 1;
        }
    }
    let mut more_tables: Vec<Value> = Vec::new();
    let more_table_refs = Value::new_hash();
    loop {
        let mut tnum = table_num_hash.get(table.to_bytes());
        if tnum.is_undef() {
            // add new table to list
            tnum = n(table_list.len() as i64);
            table_list.push(table.clone());
        }
        for mut tag_id in pl::call_list(it, "Image::ExifTool::TagTableKeys", vec![tag_table_ptr.clone()])? {
            let mut new_info = tag_table_ptr.get(tag_id.to_bytes());
            if pl::ref_of(&new_info) == "HASH" {
                if !new_info.get("Name").truthy() {
                    let nm = pl::call(it, "Image::ExifTool::MakeTagName", vec![tag_id.clone()])?;
                    new_info.set("Name", nm);
                }
                // use new ID of tag in Composite table in case it changed
                if !new_info.get("NewTagID").is_undef() {
                    tag_id = new_info.get("NewTagID");
                }
            } else {
                let h = Value::new_hash();
                h.set("Name", new_info);
                tag_table_ptr.set(tag_id.to_bytes(), h.clone());
                new_info = h;
            }
            let mut lc_tag = pl::lc(&new_info.get("Name"));
            let (lc_tags, tag_ids) = (Value::new_array(vec![]), Value::new_array(vec![]));
            let (mut existed, mut is_flat) = (false, false);
            // if this is a structure, add all flattened tag names to lookup
            if new_info.get("Struct").truthy() {
                let lc_flat = if new_info.get("FlatName").truthy() { pl::lc(&new_info.get("FlatName")) } else { lc_tag.clone() };
                add_fields(it, vec![tag_table_ptr.clone(), tag_id.clone(), lc_flat, tag_ids.clone(), lc_tags.clone(), tnum.clone()], Want::Void)?;
            }
            // add tags to lookup even though we don't know if they are writable
            // (to save speed by not having to load the module)
            'tags: loop {
                let mut lookup = tag_lookup.get(lc_tag.to_bytes());
                if !lookup.truthy() {
                    lookup = Value::new_hash();
                    tag_lookup.set(lc_tag.to_bytes(), lookup.clone());
                }
                if lookup.get(tnum.to_bytes()).truthy() {
                    let le = lookup.get(tnum.to_bytes());
                    let ids = if pl::ref_of(&le) == "ARRAY" { le } else { Value::new_array(vec![le]) };
                    if tag_id.is_ref() {
                        // a reference points to the tag ID of the base structure for
                        // a flattened tag.  There must be only one of these,
                        // and it must come first.
                        if !ids.at(0).is_ref() {
                            ids.unshift(tag_id.clone());
                        }
                    // only add new tag ID if it didn't already exist
                    } else if {
                        pl::at(13999);
                        let r = pl::re_dyn(&anchored(&tag_id), "")?;
                        ids.items().iter().any(|v| r.is_match(v))
                    } {
                        if !is_flat {
                            existed = true;
                        }
                    } else {
                        ids.push(tag_id.clone());
                    }
                    // only update lookup if there is now more than one entry
                    if ids.len() > 1 {
                        lookup.set(tnum.to_bytes(), ids);
                    }
                } else {
                    lookup.set(tnum.to_bytes(), tag_id.clone());
                }
                if tag_ids.is_empty() {
                    break;
                }
                loop {
                    tag_id = shift(&tag_ids);
                    lc_tag = shift(&lc_tags);
                    if !tag_table_ptr.get(tag_id.to_bytes()).truthy() {
                        break; // don't waste time if it exists
                    }
                    if tag_ids.is_empty() {
                        break 'tags;
                    }
                }
                is_flat = true;
            }
            // add any new subdirectory tables (unless done already)
            if existed || !new_info.get("SubDirectory").truthy() {
                continue;
            }
            let sub_table = new_info.get("SubDirectory").get("TagTable");
            if !(sub_table.truthy() && table_num_hash.get(sub_table.to_bytes()).is_undef()) {
                continue;
            }
            if more_table_refs.get(sub_table.to_bytes()).truthy() {
                continue;
            }
            // no strict 'refs';
            let sub_hash = pl::pkg_hash(it, &sub_table.to_text());
            if sub_hash.keys().is_empty() {
                continue;
            }
            // save new table to process after this one
            more_tables.push(sub_table.clone());
            more_table_refs.set(sub_table.to_bytes(), sub_hash.clone());
            // save source table name so we can load it when necessary
            sub_hash.set("SRC_TABLE", table.clone());
        }
        if more_tables.is_empty() {
            break;
        }
        table = more_tables.remove(0);
        if !table.truthy() {
            break;
        }
        tag_table_ptr = more_table_refs.get(table.to_bytes());
    }
    pl::ret_nothing(want)
}

/// `shift @$a`
fn shift(a: &Value) -> Value {
    match a {
        Value::ArrayRef(r) => {
            let mut v = r.borrow_mut();
            if v.is_empty() {
                Value::Undef
            } else {
                v.remove(0)
            }
        }
        _ => Value::Undef,
    }
}

//------------------------------------------------------------------------------
/// Return true if the tag exists
/// Inputs: 0) tag name (case insensitive)
/// Returns: true if tag exists
pub fn tag_exists(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let tag = pl::lc(&arg(&a, 0));
    if my_hash(it, "tagExists").get(tag.to_bytes()).truthy() || my_hash(it, "tagLookup").get(tag.to_bytes()).truthy() {
        return ret(n(1));
    }
    ret(n(0))
}

//------------------------------------------------------------------------------
/// Find information for writable tags (case insensitive)
/// Inputs: 0) tag name (case insensitive)
/// Returns: Scalar context: tag info or false if none found
///          List context: list of all matching tagInfo hashes
pub fn find_tag_info(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| find_tag_info_body(it, a, want))
}

fn find_tag_info_body(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let tag = arg(&a, 0);
    let lc_tag = pl::lc(&tag);
    let mut tag_info_list: Vec<Value> = Vec::new();
    let mut lookups: Vec<Value> = Vec::new();
    let tag_lookup = my_hash(it, "tagLookup");
    let table_list = my_array(it, "tableList");
    let composite_modules = my_hash(it, "compositeModules");
    let mut lookup = tag_lookup.get(lc_tag.to_bytes());
    if !lookup.truthy() && re(r"[?*]", "").is_match(&lc_tag) {
        // allow wildcards in tag name
        let mut pat = lc_tag.clone();
        re(r"\*", "").subst(&mut pat, true, |_| br"[-\w]*".to_vec());
        re(r"\?", "").subst(&mut pat, true, |_| br"[-\w]".to_vec());
        pl::at(14064);
        let r = pl::re_dyn(&anchored(&pat), "")?;
        let mut tags: Vec<Value> = keys(&tag_lookup).into_iter().filter(|k| r.is_match(k)).collect();
        if !tags.is_empty() {
            tags.sort_by(ops::str_cmp);
            for t in &tags {
                lookups.push(tag_lookup.get(t.to_bytes()));
            }
            lookup = if lookups.is_empty() { Value::Undef } else { lookups.remove(0) };
        }
    }
    while lookup.truthy() {
        let mut nums = keys(&lookup);
        nums.sort_by(|x, y| ops::num_cmp(x, y).unwrap_or(std::cmp::Ordering::Equal));
        for table_num in nums {
            let table = pl::call(it, "Image::ExifTool::GetTagTable", vec![table_list.at(table_num.to_i64())])?;
            let le = lookup.get(table_num.to_bytes());
            let tag_ids;
            if pl::ref_of(&le) == "ARRAY" {
                tag_ids = le;
                // if first entry is a reference, it indicates that this is a
                // flattened tag and refers to the tag ID of the containing top-level
                // structure, so now is the time to generate the flattened tags
                if tag_ids.at(0).is_ref() {
                    // (remove from the lookup since we only need to do this once)
                    let root_id_ptr = shift(&tag_ids);
                    runtime::require(it, "Image::ExifTool::XMP")?;
                    pl::call_list(it, "Image::ExifTool::XMP::AddFlattenedTags", vec![table.clone(), root_id_ptr.deref()])?;
                }
            } else {
                tag_ids = Value::new_array(vec![le]);
            }
            for tag_id in tag_ids.items() {
                let mut info_list = pl::call_list(it, "Image::ExifTool::GetTagInfoList", vec![table.clone(), tag_id.clone()])?;
                if info_list.is_empty() {
                    let mut reload = false;
                    // call write proc if it exists in case it adds tags to the table
                    let write_proc = table.get("WRITE_PROC");
                    if write_proc.truthy() {
                        let r = pl::call_code(it, &write_proc, vec![], Want::Scalar)?;
                        if r.last().is_some_and(|v| v.truthy()) {
                            reload = true;
                        }
                    }
                    // load module with composite tag if necessary
                    let comp_mod = composite_modules.get(lc_tag.to_bytes());
                    if comp_mod.truthy() && eval_require(it, &comp_mod)? {
                        reload = true;
                    }
                    if reload {
                        info_list = pl::call_list(it, "Image::ExifTool::GetTagInfoList", vec![table.clone(), tag_id.clone()])?;
                    }
                }
                if info_list.len() == 1 {
                    tag_info_list.extend(info_list);
                } else if info_list.len() > 1 {
                    // must check tag list in case tags have different names
                    for tag_info in info_list {
                        if lc_tag.to_bytes()[..] != pl::lc(&tag_info.get("Name")).to_bytes()[..] {
                            continue;
                        }
                        tag_info_list.push(tag_info);
                    }
                }
            }
        }
        lookup = if lookups.is_empty() { Value::Undef } else { lookups.remove(0) };
    }
    if want == Want::List {
        Ok(tag_info_list)
    } else {
        ret(tag_info_list.into_iter().next().unwrap_or_default())
    }
}

/// `eval "require $module"`: true if it loaded (`$@` set as perl's eval).
fn eval_require(it: &mut Interp, module: &Value) -> ER<bool> {
    let code = pl::concat(&s("require "), module);
    let (v, _) = pl::eval_site(it, &code, MODULE, &[], Want::Scalar);
    Ok(v.truthy())
}

/// TagLookup.pm's module-level code that depends on the run: insert any
/// user-defined tags into our tag lookup, then any other queued tags from
/// plug-in modules. The module's dump calls it once loaded.
fn init_tag_lookup(it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    if let crate::interp::Binding::Hash(h) = it.global(b'%', "Image::ExifTool::UserDefined") {
        let user = Value::HashRef(h);
        let mut tables = user.keys();
        tables.sort();
        for table in tables {
            let t = user.get(&table);
            pl::call(it, "Image::ExifTool::TagLookup::AddTags", vec![t, Value::Str(table)])?;
        }
    }
    if let crate::interp::Binding::Array(a) = it.global(b'@', "Image::ExifTool::pluginTags") {
        let queued: Vec<Value> = a.borrow().clone();
        for args in queued {
            let a = args.items();
            pl::call(it, "Image::ExifTool::TagLookup::AddTags", vec![a.first().cloned().unwrap_or_default(), a.get(1).cloned().unwrap_or_default()])?;
        }
        a.borrow_mut().clear();
    }
    Ok(vec![])
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::TagLookup::InitTagLookup", init_tag_lookup);
    it.define_native("Image::ExifTool::TagLookup::AddFields", add_fields);
    it.define_native("Image::ExifTool::TagLookup::AddTags", add_tags);
    it.define_native("Image::ExifTool::TagLookup::TagExists", tag_exists);
    it.define_native("Image::ExifTool::TagLookup::FindTagInfo", find_tag_info);
}
