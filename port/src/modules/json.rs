//! Image::ExifTool::JSON (Image/ExifTool/JSON.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read JSON files

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::substr;
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

/// `Image::ExifTool::NAME(ARGS)`
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `my (..., %flags) = @_` from the arguments after the first `skip`: the
/// key/value pairs, a later key replacing an earlier one, as in a hash.
fn flags_of(a: &[Value], skip: usize) -> Value {
    let h = Value::new_hash();
    let rest = if a.len() > skip { &a[skip..] } else { &[] };
    for kv in rest.chunks(2) {
        h.set(kv[0].to_bytes(), kv.get(1).cloned().unwrap_or_default());
    }
    h
}

/// `%flags` in a list.
fn flatten(h: &Value) -> Vec<Value> {
    h.keys().into_iter().flat_map(|k| [Value::from(k.clone()), h.get(&k)]).collect()
}

/// Store a tag value
/// Inputs: 0) ExifTool ref, 1) tag table, 2) tag ID, 3) value, 4) tagInfo flags
pub fn found_tag(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, tag_table_ptr, mut tag, val) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    let flags = flags_of(&a, 4);

    // special case to reformat ON1 tag names
    if re(r"^settings\w{8}-\w{4}-\w{4}-\w{4}-\w{12}(Data|Metadata.+)$", "").subst(&mut tag, false, |c| [&b"ON1_Settings"[..], &cap(c, 1).to_bytes()[..]].concat()) > 0 && et.get("FILE_TYPE").eq_str("JSON") {
        pl::method(it, &et, "OverrideFileType", vec![pl::s("ONP"), pl::s("application/on1")])?;
    }

    // avoid conflict with special table entries
    let special_tags = pl::pkg_hash(it, "Image::ExifTool::specialTags");
    if special_tags.get(tag.to_bytes()).truthy() {
        tag = pl::concat(&tag, &pl::s("!"));
    }

    if !tag_table_ptr.get(tag.to_bytes()).truthy() {
        let mut name = tag.clone();
        pl::tr(&mut name, ":", "_", ""); // use underlines in place of colons in tag name
        re(r"^c2pa", "i").subst(&mut name, false, |_| b"C2PA".to_vec()); // hack to fix "C2PA" case
        name = call(it, "MakeTagName", vec![name])?;
        let mut desc = call(it, "MakeDescription", vec![name.clone()])?;
        re(r"^C2 PA", "").subst(&mut desc, false, |_| b"C2PA".to_vec()); // hack to get "C2PA" correct
        let msg = pl::concat(&pl::concat(&pl::s("[adding "), &tag), &pl::s("]\n"));
        pl::method(it, &et, "VPrint", vec![n(0), et.get("INDENT"), msg])?;
        let info = Value::new_hash();
        info.set("Name", name);
        info.set("Description", desc);
        for k in flags.keys() {
            info.set(&k, flags.get(&k));
        }
        info.set("Temporary", n(1));
        call(it, "AddTagToTable", vec![tag_table_ptr.clone(), tag.clone(), info])?;
    }
    ret(pl::method(it, &et, "HandleTag", vec![tag_table_ptr, tag, val])?)
}

/// `FoundTag($et, $tagTablePtr, $tag, $val, LIST)`
fn found_tag_with(it: &mut Interp, et: &Value, tag_table_ptr: &Value, tag: &Value, val: &Value, flags: Vec<Value>) -> ER<Vec<Value>> {
    let mut args = vec![et.clone(), tag_table_ptr.clone(), tag.clone(), val.clone()];
    args.extend(flags);
    found_tag(it, args, Want::Scalar)
}

/// `ProcessTag($et, $tagTablePtr, $tag, $val, LIST)`
fn process_tag_with(it: &mut Interp, et: &Value, tag_table_ptr: &Value, tag: &Value, val: &Value, flags: Vec<Value>) -> ER<Vec<Value>> {
    let mut args = vec![et.clone(), tag_table_ptr.clone(), tag.clone(), val.clone()];
    args.extend(flags);
    process_tag(it, args, Want::Void)
}

/// Process a JSON tag
/// Inputs: 0) ExifTool ref, 1) tag table, 2) tag ID, 3) value, 4) tagInfo flags
/// - expands structures into flattened tags as required
pub fn process_tag(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| {
        let (et, tag_table_ptr, tag, val) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
        let flags = flags_of(&a, 4);

        let rt = pl::ref_of(&val);
        if rt == "HASH" {
            if pl::method(it, &et, "Options", vec![pl::s("Struct")])?.truthy() {
                let mut f = flatten(&flags);
                f.extend([pl::s("Struct"), n(1)]);
                found_tag_with(it, &et, &tag_table_ptr, &tag, &val, f)?;
                if !pl::gt(&pl::method(it, &et, "Options", vec![pl::s("Struct")])?, &n(1)) {
                    return pl::ret_nothing(want);
                }
            }
            // support hashes with ordered keys
            for k in pl::call_list(it, "Image::ExifTool::OrderedKeys", vec![val.clone()])? {
                it.set_scalar("main::_", k.clone());
                let sep = if re(r"^\d", "").is_match(&k) && re(r"\d$", "").is_match(&tag) { "_" } else { "" };
                let mut tg = pl::concat(&pl::concat(&tag, &pl::s(sep)), &pl::ucfirst(&k));
                re(r"([^a-zA-Z])([a-z])", "").subst(&mut tg, true, |c| [&cap(c, 1).to_bytes()[..], &cap(c, 2).to_bytes().to_ascii_uppercase()[..]].concat());
                let mut f = flatten(&flags);
                f.extend([pl::s("Flat"), n(1)]);
                let v = val.get(k.to_bytes());
                process_tag_with(it, &et, &tag_table_ptr, &tg, &v, f)?;
            }
        } else if rt == "ARRAY" {
            for v in val.items() {
                it.set_scalar("main::_", v.clone());
                let mut f = flatten(&flags);
                f.extend([pl::s("List"), n(1)]);
                process_tag_with(it, &et, &tag_table_ptr, &tag, &v, f)?;
            }
        } else if !val.is_undef() {
            return found_tag_with(it, &et, &tag_table_ptr, &tag, &val, flatten(&flags));
        }
        pl::ret_nothing(want)
    })
}

/// Extract meta information from a JSON file
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference, 2) tag table ref
/// Returns: 1 on success, 0 if this wasn't a recognized JSON file
pub fn process_json(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| {
        let (et, dir_info, mut tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
        let mut raf = dir_info.get("RAF");
        let _struct_opt = pl::method(it, &et, "Options", vec![pl::s("Struct")])?;
        let database = Value::new_hash();
        let mut data_pt = Value::Undef;

        if !raf.truthy() {
            data_pt = dir_info.get("DataPt");
            let dir_len = dir_info.get("DirLen");
            pl::at(128);
            if dir_info.get("DirStart").truthy() || (dir_len.truthy() && !dir_len.eq_str(&pl::length(&pl::sderef(&data_pt)?).to_text())) {
                pl::at(129);
                let buff = substr(it, &pl::sderef(&dir_info.get("DataPt"))?, &dir_info.get("DirStart"), Some(&dir_len))?;
                data_pt = pl::sref(buff);
            }
            raf = pl::call(it, "File::RandomAccess::new", vec![pl::s("File::RandomAccess"), data_pt.clone()])?;
            // extract as a block if requested
            let block_name = if dir_info.get("BlockInfo").truthy() { dir_info.get("BlockInfo").get("Name") } else { pl::s("") };
            let block_extract = pl::method(it, &et, "Options", vec![pl::s("BlockExtract")])?;
            if block_name.truthy() && (block_extract.truthy() || et.get("REQ_TAG_LOOKUP").get(pl::lc(&block_name).to_bytes()).truthy() || (et.get("TAGS_FROM_FILE").truthy() && !et.get("EXCL_TAG_LOOKUP").get(pl::lc(&block_name).to_bytes()).truthy())) {
                // (a method argument `$$dataPt` autovivifies an undefined
                // $dataPt, but dies on a string)
                pl::at(139);
                if !data_pt.is_undef() {
                    pl::sderef(&data_pt)?;
                }
                let data = data_pt.deref();
                pl::method(it, &et, "FoundTag", vec![dir_info.get("BlockInfo"), data])?;
                if block_extract.truthy() && pl::gt(&block_extract, &n(1)) {
                    return ret(n(1));
                }
            }
            pl::method(it, &et, "VerboseDir", vec![pl::s("JSON")])?;
        }

        // read information from JSON file into database structure
        let mtv = pl::method(it, &et, "Options", vec![pl::s("MissingTagValue")])?;
        let cs = pl::method(it, &et, "Options", vec![pl::s("Charset")])?;
        let err = pl::call(it, "Image::ExifTool::Import::ReadJSON", vec![raf.clone(), database.clone(), mtv, cs])?;

        if err.truthy() || database.keys().is_empty() {
            return ret(n(0));
        }

        if !data_pt.truthy() {
            pl::method(it, &et, "SetFileType", vec![])?;
        }

        if !tag_table_ptr.truthy() {
            tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::JSON::Main")])?;
        }

        // remove any old tag definitions in case they change flags
        for key in pl::call_list(it, "Image::ExifTool::TagTableKeys", vec![tag_table_ptr.clone()])? {
            if tag_table_ptr.get(key.to_bytes()).get("Temporary").truthy() {
                tag_table_ptr.delete(key.to_bytes());
            }
        }

        // extract tags from JSON database
        let mut keys = database.keys();
        keys.sort();
        for key in keys {
            let entry = database.get(&key);
            for tag in pl::call_list(it, "Image::ExifTool::OrderedKeys", vec![entry.clone()])? {
                let val = entry.get(tag.to_bytes());
                // (ignore SourceFile if generated automatically by ReadJSON)
                if tag.eq_str("SourceFile") && !val.is_undef() && val.eq_str("*") {
                    continue;
                }
                process_tag_with(it, &et, &tag_table_ptr, &tag, &val, Vec::new())?;
            }
        }
        ret(n(1))
    })
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::JSON::FoundTag", found_tag);
    it.define_native("Image::ExifTool::JSON::ProcessTag", process_tag);
    it.define_native("Image::ExifTool::JSON::ProcessJSON", process_json);
}
