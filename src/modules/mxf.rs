//! Image::ExifTool::MXF (Image/ExifTool/MXF.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read MXF meta information

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::exiftool::part2;
use crate::modules::random_access::{cell, raf_read_cell, substr};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

const MODULE: &str = "Image::ExifTool::MXF";

/// `Image::ExifTool::NAME(ARGS)` (the functions imported with :DataAccess :Utils).
fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

/// `$knownType{$type}` (a true value for the decoded MXF value types)
fn known_type(it: &mut Interp, type_: &Value) -> bool {
    crate::runtime::lex_hash(it, MODULE, "knownType").get(type_.to_bytes()).truthy()
}

fn ul_impl(v: &Value) -> ER<Value> {
    Ok(pl::join(".", &pl::unpack("H8H4H4H8H8", v)?))
}

/// Convert 16 bytes to UL format
/// Inputs: 0) 16-byte value
/// Returns: UL string
pub fn ul(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(ul_impl(&arg(&a, 0))?)
}

/// Convert latitude and/or longitude in [d]ddmmss(N|S|E|W) format
/// Inputs: 0) string value to convert
/// Returns: numerical lat and/or long
pub fn conv_lat_lon(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let val = arg(&a, 0);
    let mut conv_val: Vec<Value> = Vec::new();
    for ne in ["NS", "EW"] {
        let Some(m) = pl::re_dyn(format!(r"(\d{{2,3}})(\d{{2}})(\d{{2}})([{ne}])").as_bytes(), "")?.m(&val) else { continue };
        let c4 = cap(&m, 4);
        let sign = if c4.eq_str("N") || c4.eq_str("E") { n(1) } else { n(-1) };
        let mins = pl::add(&cap(&m, 2), &pl::div(&cap(&m, 3), &n(60))?);
        conv_val.push(pl::add(&cap(&m, 1), &pl::mul(&pl::div(&mins, &n(60))?, &sign)));
    }
    ret(pl::join(" ", &conv_val))
}

fn read_mxf_value_impl(it: &mut Interp, et: &Value, val: &Value, type_: &Value) -> ER<Value> {
    let mut val = val.clone();
    let len = pl::length(&val).to_i64();
    let t = type_.to_bytes();
    let tstr = &t[..];

    if tstr == b"UTF-16" {
        val = pl::method(it, et, "Decode", vec![val, pl::s("UTF16")])?;
    } else if tstr == b"ProductVersion" {
        let mut a = pl::unpack("n*", &val)?;
        while a.len() < 5 {
            a.push(n(0));
        }
        let rel = match a[4].to_text().as_str() {
            "0" => "unknown",
            "1" => "released",
            "2" => "debug",
            "3" => "patched",
            "4" => "beta",
            "5" => "private build",
            _ => "",
        };
        a[4] = if !rel.is_empty() { pl::s(rel) } else { pl::concat(&pl::s("unknown "), &a[4]) };
        val = pl::concat(&pl::concat(&pl::join(".", &a[0..4]), &pl::s(" ")), &a[4]);
    } else if tstr == b"VersionType" {
        val = pl::join(".", &pl::unpack("C*", &val)?);
    } else if tstr == b"Timestamp" {
        let mut a = pl::unpack("nC*", &val)?;
        let mut max: Vec<i64> = vec![3000, 12, 31, 24, 59, 59, 249];
        for x in &a {
            if !(!max.is_empty() && pl::le(x, &n(max[0]))) {
                break;
            }
            max.remove(0);
        }
        if !max.is_empty() {
            val = pl::concat(&pl::concat(&pl::s("Invalid (0x"), &pl::unpack1("H*", &val)?), &pl::s(")"));
        } else {
            a[6] = pl::mul(&a[6], &n(4));
            val = pl::sprintf("%.4d:%.2d:%.2d %.2d:%.2d:%.2d.%.3d", &a);
        }
    } else if tstr == b"Position" || tstr == b"Length" {
        val = call(it, "Get64u", vec![pl::sref(val), n(0)])?;
    } else if tstr == b"Boolean" {
        val = pl::s(if val.eq_str("\0") { "False" } else { "True" });
    } else if re(r"^(Alt|Lat|Lon)$", "").is_match(type_) && len == 4 {
        // split into nibbles after swapping byte order (see reference 8b)
        val = pl::unpack1("H*", &pl::pack("N", &pl::unpack("V", &val)?)?)?;
        if tstr == b"Alt" {
            // drop satellite information and only support altitudes up to +/-99999 m
            if !(re(r"^[abc]..", "").subst(&mut val, false, |_| Vec::new()) > 0 || re(r"^[def]..", "").subst(&mut val, false, |_| b"-".to_vec()) > 0) {
                val = pl::concat(&val, &pl::s(" (from earth centre)"));
            }
        } else {
            re(r"(...)", "").subst(&mut val, false, |c| [&cap(c, 1).to_bytes()[..], b"."].concat()); // insert decimal point after 3rd character
            if tstr == b"Lat" {
                re(r"^f", "").subst(&mut val, false, |_| b"-".to_vec()); // south with first digit = 0
            } else {
                re(r"^e", "").subst(&mut val, false, |_| b"-".to_vec()); // east with first digit = 0
                re(r"^f", "").subst(&mut val, false, |_| b"-1".to_vec()); // east with first digit = 1
            }
        }
    } else if re(r"(Array|Batch)", "").is_match(type_) && len > 16 {
        let u = pl::unpack("NN", &val)?;
        let (count, size) = (arg(&u, 0), arg(&u, 1));
        // validate data length
        if !pl::num_eq(&n(len), &pl::add(&n(8), &pl::mul(&count, &size))) {
            pl::method(it, et, "Warn", vec![pl::s("Bad array or batch size")])?;
        }
        let mut a: Vec<Value> = Vec::new();
        let mut i = n(0);
        while pl::lt(&i, &count) {
            let pos = pl::add(&n(8), &pl::mul(&i, &size));
            if pl::gt(&pl::add(&pos, &size), &n(len)) {
                break;
            }
            a.push(substr(it, &val, &pos, Some(&size))?);
            i = pl::add(&i, &n(1));
        }
        if re(r"^StrongReference", "").is_match(type_) {
            for x in a.iter_mut() {
                *x = pl::join("-", &pl::unpack("H8H4H4H4H12", x)?);
            }
        } else if tstr == b"BatchOfUL" || re(r"^WeakReference", "").is_match(type_) {
            for x in a.iter_mut() {
                *x = read_mxf_value_impl(it, et, x, &pl::s("UL"))?;
            }
        }
        val = Value::new_array(a);
    } else if len == 32 {
        // 32-byte types include UMID, PackageID
        let p1 = pl::join(".", &pl::unpack("H8H4H4H8", &val)?);
        let p2 = pl::join(" ", &pl::unpack("x12H2H6", &val)?);
        let p3 = pl::join("-", &pl::unpack("x16H8H4H4H4H12", &val)?);
        val = pl::concat(&pl::concat(&pl::concat(&pl::concat(&p1, &pl::s(" ")), &p2), &pl::s(" ")), &p3);
    } else if len == 16 {
        // convert remaining 16-byte known types as GUID or UL
        // - this covers all UID's, UL, Label, WeakReference, StrongReference
        if tstr == b"UL" || tstr == b"WeakReference" {
            // reverse high/low word if a reversed GUID is stored in a UL type
            // (see ref 8b, section 5.5.8)
            // - a GUID will have high bit of byte 8 set (reversed, this is in byte 0)
            if pl::unpack1("C", &val)?.to_i64() & 0x80 == 0 {
                return ul_impl(&val);
            }
            val = pl::concat(&pl::substr(&val, 8, None), &pl::substr(&val, 0, Some(8)));
        }
        val = pl::join("-", &pl::unpack("H8H4H4H4H12", &val)?); // compact GUID format
    } else {
        // Note: a 64-byte extended UMID contains date/time, GPS coordinates, etc,
        // which would could be decoded here (ref 8b, section 5)...
        val = pl::unpack1("H*", &val)?;
    }
    Ok(val)
}

/// Read MXF-specific Format types
/// Inputs: 0) ExifTool ref, 1) value, 2) MXF value type
/// Returns: formatted value
/// Note: All types recognized here should be defined in the %knownType lookup
pub fn read_mxf_value(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| ret(read_mxf_value_impl(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?))
}

/// Process MXF Primer (to build lookup for translating local tag ID's)
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_primer(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let data_pt = dir_info.get("DataPt");
    let end = dir_info.get("DirLen");
    if !pl::gt(&end, &n(8)) {
        return ret(n(0));
    }
    let count = call(it, "Get32u", vec![data_pt.clone(), n(0)])?;
    let size = call(it, "Get32u", vec![data_pt.clone(), n(4)])?;
    if !pl::ge(&size, &n(18)) {
        return ret(n(0));
    }
    pl::method(it, &et, "VerboseDir", vec![pl::s("MXF Primer"), count.clone()])?;
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    // (`$$et{MXFInfo}{Primer}` read vivifies MXFInfo; `$$primer{$local} = ...`
    // vivifies only the lexical)
    let mut primer = et.get_or_new_hash("MXFInfo").get("Primer");
    if primer.is_undef() {
        primer = Value::new_hash();
    }
    let mut pos = n(8);
    let mut i = n(0);
    while pl::lt(&i, &count) {
        if pl::gt(&pl::add(&pos, &size), &end) {
            break;
        }
        let local = call(it, "Get16u", vec![data_pt.clone(), pos.clone()])?;
        let global = ul_impl(&substr(it, &data_pt.deref(), &pl::add(&pos, &n(2)), Some(&n(16)))?)?;
        pos = pl::add(&pos, &size);
        // save this entry in the primer lookup table
        primer.set(local.to_bytes(), global.clone());
        // print lookup details in verbose mode
        if verbose.truthy() {
            let mut indx = pl::concat(&i, &pl::s(")"));
            if pl::lt(&pl::length(&indx), &n(3)) {
                indx = pl::concat(&indx, &pl::s(" "));
            }
            let fmt = pl::concat(&pl::concat(&pl::concat(&pl::concat(&pl::s("  | "), &indx), &pl::s(" 0x%.4x => '")), &global), &pl::s("'\n"));
            let msg = pl::sprintf_v(&fmt, &[local]);
            pl::method(it, &et, "VPrint", vec![n(0), msg])?;
        }
        i = pl::add(&i, &n(1));
    }
    ret(n(1))
}

/// Read tags from an MXF local set
/// Inputs: 0) ExifTool ref, 1) dirInfo ref, 2) tag table ref
/// Returns: 1 on success
pub fn process_local_set(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    pl::local_topic(it, |it| process_local_set_body(it, &a))
}

fn process_local_set_body(it: &mut Interp, a: &[Value]) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(a, 0), arg(a, 1), arg(a, 2));
    let data_pt = dir_info.get("DataPt");
    let data_pos = dir_info.get("DataPos");
    let end = dir_info.get("DirLen");
    let mxf_info = et.get("MXFInfo");
    let primer = mxf_info.get("Primer");
    let strong_ref = Value::new_array(Vec::new());
    let groups = Value::new_array(Vec::new());
    let (mut instance, mut edit_rate, mut track_id, mut lang_code, mut text_lang) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef, Value::Undef);
    let process_local_set_ref = Value::Code(it.sub_ref("Image::ExifTool::MXF::ProcessLocalSet"));

    pl::method(it, &et, "VerboseDir", vec![pl::s("MXF LocalSet"), Value::Undef, end.clone()])?;

    // loop through all tags in this local set
    let mut pos = n(0);
    while pl::lt(&pl::add(&pos, &n(4)), &end) {
        let loc = call(it, "Get16u", vec![data_pt.clone(), pos.clone()])?; // get key (local tag ID)
        let len = call(it, "Get16u", vec![data_pt.clone(), pl::add(&pos, &n(2))])?; // get length
        pos = pl::add(&pos, &n(4));
        if pl::gt(&pl::add(&pos, &len), &end) {
            break;
        }
        let mut tag = primer.get(loc.to_bytes());
        let extra;
        let (mut val, mut type_, mut lang_info) = (Value::Undef, Value::Undef, Value::Undef);
        if tag.truthy() && tag_table_ptr.get(tag.to_bytes()).truthy() {
            extra = pl::sprintf(", Local 0x%.4x", &[loc.clone()]);
        } else {
            tag = loc.clone();
            // $et->Warn('Missing local key for at least one tag');
            extra = pl::s(", NOT IN PRIMER!");
        }
        let mut tag_info = tag_table_ptr.get(tag.to_bytes());
        // handle our MXF-specific format types
        if tag_info.truthy() {
            type_ = tag_info.get("Type");
            if type_.truthy() && known_type(it, &type_) {
                let s = substr(it, &data_pt.deref(), &pos, Some(&len))?;
                val = read_mxf_value_impl(it, &et, &s, &type_)?;
                if re(r"^StrongReference", "").is_match(&type_) {
                    if val.is_ref() {
                        for v in val.items() {
                            strong_ref.push(v);
                        }
                    } else {
                        strong_ref.push(val.clone());
                    }
                }
                // remember instance UID of this local set
                if tag_info.get("Name").eq_str("InstanceUID") {
                    instance = val.clone();
                    // set language code for text
                    // (only works if InstanceUID comes before text)
                    let obj = mxf_info.get(instance.to_bytes());
                    if obj.truthy() {
                        text_lang = obj.get("LangCode");
                    }
                } else if type_.eq_str("UTF-16") && text_lang.truthy() {
                    lang_info = pl::call(it, "Image::ExifTool::GetLangInfo", vec![tag_info.clone(), text_lang.clone()])?;
                }
            }
        }
        // get tagInfo ref the standard way to handle Unknown tags
        tag_info = if lang_info.truthy() { lang_info } else { pl::method(it, &et, "GetTagInfo", vec![tag_table_ptr.clone(), tag.clone()])? };
        // set Binary flag to extract all unknown-format tags as Binary data
        if tag_info.truthy() && tag_info.get("Unknown").truthy() && tag_info.get("Binary").is_undef() {
            let known = tag_info.get("Format").truthy() || (type_.truthy() && known_type(it, &type_));
            tag_info.set("Binary", Value::from(!known));
        }
        let key = pl::method(
            it,
            &et,
            "HandleTag",
            vec![
                tag_table_ptr.clone(),
                tag.clone(),
                val,
                pl::s("Extra"),
                extra,
                pl::s("TagInfo"),
                tag_info.clone(),
                pl::s("DataPt"),
                data_pt.clone(),
                pl::s("DataPos"),
                data_pos.clone(),
                pl::s("Start"),
                pos.clone(),
                pl::s("Size"),
                len.clone(),
                pl::s("ProcessProc"),
                process_local_set_ref.clone(),
            ],
        )?;
        pos = pl::add(&pos, &len);
        if !key.truthy() {
            continue;
        }
        // save information to allow later fixup of durations and group1 names
        // (necessary because we don't have all the information we need
        //  to do this on the fly when the file is parsed linearly)
        groups.push(et.get("TAG_EXTRA").get(key.to_bytes()));
        if !tag_info.truthy() {
            continue;
        }
        let name = tag_info.get("Name");
        if tag_info.get("IsDuration").truthy() {
            mxf_info.get_or_new_hash("FixDuration").set(key.to_bytes(), n(1));
        } else if tag_info.get("LanguageCode").truthy() {
            lang_code = et.get("VALUE").get(key.to_bytes());
        } else if name.eq_str("EditRate") {
            edit_rate = et.get("VALUE").get(key.to_bytes());
        } else if re(r"TrackID$", "").is_match(&name) {
            track_id = et.get("VALUE").get(key.to_bytes());
            let g1 = mxf_info.get_or_new_hash("Group1");
            if !g1.get(track_id.to_bytes()).truthy() {
                // save lookup to convert TrackID to our group 1 name
                let nt = pl::add(&mxf_info.get("NumTracks"), &n(1));
                mxf_info.set("NumTracks", nt.clone());
                g1.set(track_id.to_bytes(), pl::concat(&pl::s("Track"), &nt));
            }
        }
    }

    // save object information now that we know the instance UID
    if instance.truthy() {
        let mut obj_info = mxf_info.get(instance.to_bytes());
        if obj_info.truthy() {
            let sr = obj_info.get_or_new_array("StrongRef");
            for v in strong_ref.items() {
                sr.push(v);
            }
            let gr = obj_info.get_or_new_array("Groups");
            for v in groups.items() {
                gr.push(v);
            }
        } else {
            obj_info = Value::new_hash();
            obj_info.set("StrongRef", strong_ref.clone());
            obj_info.set("Groups", groups.clone());
            mxf_info.set(instance.to_bytes(), obj_info.clone());
        }
        // save instance UID's in groups hash (used to remove duplicates later)
        for (i, g) in groups.items().into_iter().enumerate() {
            let g = if g.is_undef() {
                let h = Value::new_hash();
                groups.set_at(i, h.clone());
                h
            } else {
                g
            };
            g.set("UID", instance.clone());
        }
        obj_info.set("Name", et.get("DIR_NAME"));
        if !track_id.is_undef() {
            obj_info.set("TrackID", track_id.clone());
        }
        if edit_rate.truthy() {
            obj_info.set("EditRate", edit_rate.clone());
        }
        if lang_code.truthy() {
            obj_info.set("LangCode", lang_code.clone());
        } else {
            lang_code = obj_info.get("LangCode");
        }
        if lang_code.truthy() {
            // pre-set language code in all children
            for ul in obj_info.get("StrongRef").items() {
                let mut obj = mxf_info.get(ul.to_bytes());
                if !obj.truthy() {
                    obj = Value::new_hash();
                    obj.set("StrongRef", Value::new_array(Vec::new()));
                    obj.set("Groups", Value::new_array(Vec::new()));
                    obj.set("Name", pl::s("XXX"));
                    mxf_info.set(ul.to_bytes(), obj.clone());
                }
                obj.set("LangCode", lang_code.clone());
            }
        }
        // save instance UID's of Preface's
        if et.get("DIR_NAME").eq_str("Preface") {
            mxf_info.get_or_new_array("Preface").push(instance.clone());
        }
    }
    ret(n(1))
}

fn set_groups_impl(mxf_info: &Value, instance: &Value, path_info: &Value, track_id: &Value) -> ER<()> {
    let mut track_id = track_id.clone();
    let obj_info = mxf_info.get(instance.to_bytes());
    if !(obj_info.truthy() && !obj_info.get("DidGroups").truthy()) {
        return Ok(());
    }
    obj_info.set("DidGroups", n(1)); // avoid reprocessing this object
    if !obj_info.get("TrackID").is_undef() {
        track_id = obj_info.get("TrackID");
    }
    let (mut g1, mut g5, mut path, mut name_count, mut set_source) = (Value::Undef, Value::Undef, Value::Undef, Value::Undef, false);
    // generate group 1 name for different tracks
    if !track_id.is_undef() {
        obj_info.set("TrackID", track_id.clone());
        g1 = mxf_info.get_or_new_hash("Group1").get(track_id.to_bytes());
        // build a lookup to determine edit rates based on group 1 name
        let edit_rate = obj_info.get("EditRate");
        if !edit_rate.is_undef() {
            mxf_info.get_or_new_hash("EditRate").set(g1.to_bytes(), edit_rate);
        }
        // save the TimeCodeComponent instance UID (for determining Duration later)
        if obj_info.get("Name").eq_str("TimecodeComponent") {
            let in_what = if mxf_info.get("InSource").truthy() { "Source" } else { "Other" };
            mxf_info.get_or_new_hash("BestDuration").set(in_what, instance.clone());
        }
    }
    // set flag if we are in the SourcePackage (contains our preferred TimecodeComponent)
    let name = obj_info.get("Name");
    if name.eq_str("SourcePackage") {
        mxf_info.set("InSource", n(1));
        set_source = true;
    }

    // generate group 5 path names if requested
    if path_info.truthy() {
        let nc = path_info.get("NameCount");
        name_count = if nc.truthy() { nc } else { Value::new_hash() };
        // (`push @$path` vivifies the lexical $path, not $$pathInfo{Path})
        path = path_info.get("Path");
        if path.is_undef() {
            path = Value::new_array(Vec::new());
        }
        let c = name_count.get(name.to_bytes());
        name_count.set(name.to_bytes(), pl::add(&if c.truthy() { c } else { n(0) }, &n(1)));
        path.push(pl::concat(&name, &name_count.get(name.to_bytes())));
        g5 = pl::join("-", &path.items());
        path_info.set("NameCount", Value::new_hash()); // use new name count for child objects
    }
    let gr = obj_info.get_or_new_array("Groups");
    for (i, groups) in gr.items().into_iter().enumerate() {
        if !(g1.truthy() || g5.truthy()) {
            continue;
        }
        let groups = if groups.is_undef() {
            let h = Value::new_hash();
            gr.set_at(i, h.clone());
            h
        } else {
            groups
        };
        if g1.truthy() {
            groups.set("G1", g1.clone());
        }
        if g5.truthy() {
            groups.set("G5", g5.clone());
        }
    }
    // walk through remaining objects in tree
    for ul in obj_info.get_or_new_array("StrongRef").items() {
        set_groups_impl(mxf_info, &ul, path_info, &track_id)?;
    }
    if path_info.truthy() {
        if let Value::ArrayRef(p) = &path {
            p.borrow_mut().pop();
        }
        path_info.set("NameCount", name_count);
    }
    if set_source {
        mxf_info.delete("InSource");
    }
    Ok(())
}

/// Walk MXF tree to set group names
/// Inputs: 0) MXF information hash ref, 1) object instance UID,
///         2) path information hash ref (only for group 5 names), 3) track ID number
/// Notes: also generates lookup table for EditRate based on group 1 name
///        and the instance UID of the SequenceSet with the preferred Duration value
pub fn set_groups(_it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    set_groups_impl(&arg(&a, 0), &arg(&a, 1), &arg(&a, 2), &arg(&a, 3))?;
    pl::ret_nothing(want)
}

fn convert_durations_impl(et: &Value, mxf_info: &Value) -> ER<()> {
    let value_hash = et.get("VALUE");
    let info_hash = et.get("TAG_INFO");
    let tag_extra = et.get("TAG_EXTRA");
    let edit_hash = mxf_info.get("EditRate");
    // (`keys %{$$mxfInfo{FixDuration}}` vivifies it)
    for tag in mxf_info.get_or_new_hash("FixDuration").keys() {
        let tag = Value::bytes(&tag[..]);
        // loop through all instances of this tag name
        let mut i = 0;
        let mut key = tag.clone();
        loop {
            let tag_info = info_hash.get(key.to_bytes());
            if !tag_info.truthy() {
                break;
            }
            // test IsDuration flag to be sure ("next" otherwise)
            if tag_info.get("IsDuration").truthy() {
                let g1 = tag_extra.get_or_new_hash(key.to_bytes()).get("G1");
                if g1.truthy() {
                    let edit_rate = edit_hash.get(g1.to_bytes());
                    if edit_rate.truthy() {
                        pl::at(2798);
                        let v = pl::div(&value_hash.get(key.to_bytes()), &edit_rate)?;
                        value_hash.set(key.to_bytes(), v);
                    }
                }
            }
            i += 1;
            key = pl::concat(&tag, &Value::from(format!(" ({i})")));
        }
    }
    Ok(())
}

/// Convert all duration values to seconds
/// Inputs: 0) ExifTool object ref, 1) MXF information hash ref
pub fn convert_durations(_it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    convert_durations_impl(&arg(&a, 0), &arg(&a, 1))?;
    pl::ret_nothing(want)
}

/// Read information in a MXF file
/// Inputs: 0) ExifTool ref, 1) dirInfo ref
/// Returns: 1 on success, 0 if this wasn't a valid MXF file
pub fn process_mxf(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let unknown = pl::method(it, &et, "Options", vec![pl::s("Unknown")])?;
    let buff = cell(Value::Undef);
    let b = || buff.borrow().clone();
    let (mut header_end, mut footer_pos) = (Value::Undef, Value::Undef);

    // read enough to allow skipping over run-in if it exists
    pl::at(2816);
    part2::check_invocant(it, &raf, "Read")?;
    if !raf_read_cell(it, &raf, &buff, n(65547))?.truthy() {
        return ret(n(0));
    }
    let rx = match crate::perl::regex::compile(br"\x06\x0e\x2b\x34\x02\x05\x01\x01\x0d\x01\x02", "") {
        Ok(r) => r,
        Err(e) => return crate::interp::eval::die(e),
    };
    let Some(sp) = rx.match_at(&b().to_bytes(), 0).ok().flatten() else {
        return ret(n(0));
    };
    let mut start = n(sp[0].unwrap().1 as i64 - 11);

    pl::method(it, &et, "SetFileType", vec![])?;
    call(it, "SetByteOrder", vec![pl::s("MM")])?;
    if !pl::method(it, &raf, "Seek", vec![start.clone(), n(0)])?.truthy() {
        pl::method(it, &et, "Warn", vec![pl::s("Seek error")])?;
        return ret(n(1));
    }
    let tag_table_ptr = call(it, "GetTagTable", vec![pl::s("Image::ExifTool::MXF::Main")])?;

    // determine header length and type

    // initialize MXF information lookups
    let mxf_info = Value::new_hash();
    mxf_info.set("Primer", Value::new_hash()); // convert local keys to global UL
    mxf_info.set("Group1", Value::new_hash()); // group 1 names base on TrackID
    mxf_info.set("NumTracks", n(0)); // counts number of tracks in file
    mxf_info.set("FixDuration", Value::new_hash()); // names of all Duration tags that need fixing
    mxf_info.set("Preface", Value::new_array(Vec::new())); // instance UID's for all Preface objects
    et.set("MXFInfo", mxf_info.clone());

    // set group 1 name for all tags (so we can overwrite with track number later)
    et.set("SET_GROUP1", pl::s("MXF"));
    let process_local_set_ref = Value::Code(it.sub_ref("Image::ExifTool::MXF::ProcessLocalSet"));

    loop {
        let mut pos = pl::method(it, &raf, "Tell", vec![])?;
        // did we just finish parsing the header partition?
        if header_end.truthy() && pl::ge(&pos, &header_end) {
            // all done if it was a closed and complete header
            if mxf_info.get("HeaderType").eq_str("ClosedCompleteHeader") && !verbose.truthy() {
                break;
            }
            header_end = Value::Undef; // (only test this once)
            // skip directly to footer if possible
            if footer_pos.truthy() && pl::gt(&footer_pos, &pos) && (!verbose.truthy() || !unknown.truthy()) {
                pl::method(it, &et, "VPrint", vec![n(0), pl::s("[Skipping to footer. Use Unknown option to parse body partitions]\n")])?;
                if !pl::method(it, &raf, "Seek", vec![footer_pos.clone(), n(0)])?.truthy() {
                    break;
                }
                pos = footer_pos.clone();
            }
        }
        // read the next KLV Key and first byte of Value
        if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(17))?, &n(17)) {
            break;
        }
        let tag = pl::substr(&b(), 0, Some(16)); // get KLV Key (global tag ID)
        let mut len = call(it, "Get8u", vec![Value::ScalarRef(buff.clone()), n(16)])?; // get KLV Length
        let nn;
        if pl::ge(&len, &n(0x80)) {
            nn = n(len.to_i64() & 0x7f);
            if !pl::num_eq(&raf_read_cell(it, &raf, &buff, nn.clone())?, &nn) {
                break;
            }
            len = n(0);
            for bb in pl::unpack("C*", &b())? {
                len = pl::add(&pl::mul(&len, &n(256)), &bb);
            }
        } else {
            nn = n(0);
        }
        // convert tag ID to ASCII UL notation
        let ul = ul_impl(&tag)?;
        let mut tag_info = tag_table_ptr.get(ul.to_bytes());
        if !tag_info.truthy() {
            if let Some(m) = re(r"^060e2b34\.0253\.0101\.(0d|0f)", "").m(&ul) {
                if cap(&m, 1).eq_str("0d") || verbose.truthy() || unknown.truthy() {
                    // generate some unknown set tags automatically
                    let name = if cap(&m, 1).eq_str("0d") { "UserOrganizationPublicUse" } else { "Experimental" };
                    tag_info = Value::new_hash();
                    tag_info.set("Name", pl::s(name));
                    let local_set = crate::runtime::lex_hash(it, MODULE, "localSet");
                    for k in local_set.keys() {
                        tag_info.set(&k[..], local_set.get(&k[..]));
                    }
                    call(it, "AddTagToTable", vec![tag_table_ptr.clone(), ul.clone(), tag_info.clone()])?;
                }
            }
        }
        let mut val = Value::Undef;
        let data_pt;
        if tag_info.truthy() && !tag_info.get("Unknown").truthy() && pl::lt(&len, &n(10000000)) {
            // save information about header/footer positions for skipping over body partitions
            if tag_info.get("Name").eq_str("Primer") && mxf_info.get("HeaderSize").truthy() {
                // footer position relative to header start and only valid if non-zero
                if mxf_info.get("FooterPos").truthy() {
                    footer_pos = pl::add(&start, &mxf_info.get("FooterPos"));
                }
                // header length is relative to start of Primer
                header_end = pl::add(&pos, &mxf_info.get("HeaderSize"));
            } else if tag_info.get("IsHeader").truthy() {
                // save position of header start to allow calculation of footer position
                start = pos.clone();
            }
            if !pl::num_eq(&raf_read_cell(it, &raf, &buff, len.clone())?, &len) {
                break; // get KLV Value
            }
            data_pt = Value::ScalarRef(buff.clone());
            let type_ = tag_info.get("Type");
            if type_.truthy() && known_type(it, &type_) {
                val = read_mxf_value_impl(it, &et, &b(), &type_)?;
            }
        } else if (tag_info.truthy() && (!tag_info.get("Unknown").truthy() || unknown.truthy())) || verbose.truthy() {
            if tag_info.truthy() {
                // set Binary flag to extract all unknown-format tags as Binary data
                if tag_info.get("Unknown").truthy() && tag_info.get("Binary").is_undef() {
                    let type_ = tag_info.get("Type");
                    let known = tag_info.get("Format").truthy() || (type_.truthy() && known_type(it, &type_));
                    tag_info.set("Binary", Value::from(!known));
                }
            } else {
                let id = pl::unpack1("H*", &tag)?;
                tag_info = Value::new_hash();
                tag_info.set("Name", pl::concat(&pl::s("MXF_"), &id));
                tag_info.set("Description", pl::concat(&pl::s("MXF "), &id));
                // note: don't add unknown tags to table because we don't
                // want them to be extracted with the Unknown option
            }
            // read the first 64kB max
            let nr = if pl::lt(&len, &n(65536)) { len.clone() } else { n(65536) };
            let vc = cell(Value::Undef);
            if !pl::num_eq(&raf_read_cell(it, &raf, &vc, nr.clone())?, &nr) {
                break;
            }
            val = vc.borrow().clone();
            data_pt = Value::ScalarRef(vc.clone());
            let more = pl::sub(&len, &nr);
            if more.truthy() && !pl::method(it, &raf, "Seek", vec![more, n(1)])?.truthy() {
                break;
            }
        } else {
            if !pl::method(it, &raf, "Seek", vec![len.clone(), n(1)])?.truthy() {
                break; // skip this value
            }
            continue;
        }
        pl::method(
            it,
            &et,
            "HandleTag",
            vec![
                tag_table_ptr.clone(),
                ul,
                val,
                pl::s("TagInfo"),
                tag_info,
                pl::s("DataPt"),
                data_pt,
                pl::s("DataPos"),
                pl::add(&pl::add(&pos, &n(17)), &nn),
                pl::s("Size"),
                len,
                pl::s("ProcessProc"),
                process_local_set_ref.clone(),
            ],
        )?;
    }
    // walk entire MXF object tree to fix family 1 group names
    let mut path_info = Value::Undef;
    let did = Value::new_hash();
    let best_dur = Value::new_hash();
    if pl::method(it, &et, "Options", vec![pl::s("SavePath")])?.truthy() {
        path_info = Value::new_hash();
        path_info.set("Path", Value::new_array(vec![pl::s("MXF")]));
    }
    for preface in mxf_info.get("Preface").items() {
        set_groups_impl(&mxf_info, &preface, &path_info, &Value::Undef)?;
    }
    // convert Duration values to seconds based on the appropriate EditRate
    convert_durations_impl(&et, &mxf_info)?;

    // remove tags to keep only the one from the most recent instance of the object
    let tag_extra = et.get("TAG_EXTRA");
    let file_order = et.get("FILE_ORDER");
    // also determine our best Duration value
    let bd = mxf_info.get("BestDuration");
    if bd.truthy() {
        let s = bd.get("Source");
        let instance = if s.truthy() { s } else { bd.get("Other") };
        if instance.truthy() {
            best_dur.set(pl::concat(&pl::s("Duration "), &instance).to_bytes(), n(1));
        }
    }
    // process tags in reverse order to preserve the last found of each identical tag
    let mut keys: Vec<Value> = tag_extra.keys().into_iter().map(|k| Value::bytes(&k[..])).collect();
    keys.sort_by(|x, y| {
        let (fx, fy) = (file_order.get(x.to_bytes()), file_order.get(y.to_bytes()));
        crate::perl::ops::num_cmp(&fy, &fx).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut last_c1 = Value::Undef;
    for tag in keys {
        let te = tag_extra.get(tag.to_bytes());
        let instance = te.get("UID");
        if !instance.truthy() {
            continue;
        }
        te.delete("UID"); // (no longer needed)
        if let Some(m) = re(r"^(\S+)", "").m(&tag) {
            last_c1 = cap(&m, 1); // get tag name without index number
        }
        let utag = pl::concat(&pl::concat(&last_c1, &pl::s(" ")), &instance); // instance-specific tag name
        if did.get(utag.to_bytes()).truthy() {
            pl::call(it, "Image::ExifTool::DeleteTag", vec![et.clone(), tag.clone()])?; // delete the duplicate
        } else {
            did.set(utag.to_bytes(), n(1));
            if best_dur.get(utag.to_bytes()).truthy() {
                // save best duration value
                let val = et.get("VALUE").get(tag.to_bytes());
                pl::method(it, &et, "HandleTag", vec![tag_table_ptr.clone(), pl::s("060e2b34.0101.0102.07020201.01030000"), val])?;
            }
        }
    }

    // clean up and return
    et.delete("SET_GROUP1");
    et.delete("MXFInfo");
    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::MXF::UL", ul);
    it.define_native("Image::ExifTool::MXF::ConvLatLon", conv_lat_lon);
    it.define_native("Image::ExifTool::MXF::ReadMXFValue", read_mxf_value);
    it.define_native("Image::ExifTool::MXF::ProcessPrimer", process_primer);
    it.define_native("Image::ExifTool::MXF::ProcessLocalSet", process_local_set);
    it.define_native("Image::ExifTool::MXF::SetGroups", set_groups);
    it.define_native("Image::ExifTool::MXF::ConvertDurations", convert_durations);
    it.define_native("Image::ExifTool::MXF::ProcessMXF", process_mxf);
}
