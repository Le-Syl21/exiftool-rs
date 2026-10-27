//! Image::ExifTool::IPTC (Image/ExifTool/IPTC.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::{die, Binding, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::{set_deref, substr};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, cap, re, ret};

fn n(i: i64) -> Value {
    Value::Int(i)
}

/// A package hash as a reference (`\%Pkg::name`).
fn global_hash(it: &mut Interp, name: &str) -> Value {
    match it.global(b'%', name) {
        Binding::Hash(h) => Value::HashRef(h),
        _ => Value::Undef,
    }
}

/// `ord(substr($$dataPt, $pos, 1))`
fn ord_at(it: &mut Interp, data: &Value, pos: &Value) -> ER<Value> {
    let c = substr(it, data, pos, Some(&n(1)))?;
    Ok(n(c.to_bytes().first().copied().unwrap_or(0) as i64))
}

/// AutoLoad our writer routines when necessary
pub fn autoload(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let name = it.get_scalar("Image::ExifTool::IPTC::AUTOLOAD");
    let mut args = vec![name];
    args.extend(a);
    it.call_named("Image::ExifTool::DoAutoLoad", args, want)
}

/// Print conversion for CodedCharacterSet
/// Inputs: 0) value
pub fn print_coded_charset(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let mut val = arg(&a, 0);
    let iptc_charset = global_hash(it, "Image::ExifTool::IPTC::iptcCharset");
    let cs = iptc_charset.get(val.to_bytes());
    if cs.truthy() {
        return ret(cs);
    }
    re(r"(.)", "").subst(&mut val, true, |c| [&b" "[..], &cap(c, 1).to_bytes()[..]].concat());
    re(r" \x1b", "").subst(&mut val, true, |_| b", ESC".to_vec());
    re(r"^,? ", "").subst(&mut val, false, |_| Vec::new());
    ret(val)
}

fn handle_coded_charset_impl(it: &mut Interp, et: &Value, val: &Value) -> ER<Value> {
    let iptc_charset = global_hash(it, "Image::ExifTool::IPTC::iptcCharset");
    let mut xlat = iptc_charset.get(val.to_bytes());
    if !xlat.truthy() {
        if re(r"^\x1b\x25", "").is_match(val) {
            // some unknown character set invoked
            xlat = pl::s("bad"); // flag unsupported coding
        } else {
            xlat = pl::method(it, et, "Options", vec![pl::s("CharsetIPTC")])?;
        }
    }
    // no need to translate if Charset is the same
    let charset = pl::method(it, et, "Options", vec![pl::s("Charset")])?;
    if xlat.to_bytes() == charset.to_bytes() {
        xlat = Value::Undef;
    }
    Ok(xlat)
}

/// Handle CodedCharacterSet
/// Inputs: 0) ExifTool ref, 1) CodedCharacterSet value
/// Returns: IPTC character set if translation required (or 'bad' if unknown)
pub fn handle_coded_charset(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(handle_coded_charset_impl(it, &arg(&a, 0), &arg(&a, 1))?)
}

/// The body of TranslateCodedString on the values `$$valPtr` and `$$xlatPtr`.
fn translate_coded_string_impl(it: &mut Interp, et: &Value, val: &mut Value, xlat: &mut Value, read: &Value) -> ER<()> {
    if xlat.eq_str("bad") {
        pl::method(it, et, "Warn", vec![pl::s("Some IPTC characters not converted (unsupported CodedCharacterSet)")])?;
        *xlat = Value::Undef;
    } else if !read.truthy() {
        *val = pl::method(it, et, "Decode", vec![val.clone(), Value::Undef, Value::Undef, xlat.clone()])?;
    } else if !re(r"[\x14\x15\x1b]", "").is_match(val) {
        *val = pl::method(it, et, "Decode", vec![val.clone(), xlat.clone()])?;
    } else {
        // don't yet support reading ISO 2022 shifted character sets
        pl::method(it, et, "Warn", vec![pl::s("Some IPTC characters not converted (ISO 2022 shifting not supported)")])?;
    }
    Ok(())
}

/// Encode or decode coded string
/// Inputs: 0) ExifTool ref, 1) value ptr, 2) IPTC charset (or 'bad') ref
///         3) flag set to decode (read) value from IPTC
/// Updates value on return
pub fn translate_coded_string(it: &mut Interp, a: Vec<Value>, w: Want) -> ER<Vec<Value>> {
    let (et, val_ptr, xlat_ptr, read) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    pl::at(1019);
    let mut xlat = pl::sderef(&xlat_ptr)?;
    // ($$valPtr is a method argument where it is not read for the match:
    // autovivified there, not an error)
    let mut val = if !xlat.eq_str("bad") && read.truthy() {
        pl::at(1024);
        pl::sderef(&val_ptr)?
    } else {
        val_ptr.deref()
    };
    let (val0, xlat0) = (val.clone(), xlat.clone());
    translate_coded_string_impl(it, &et, &mut val, &mut xlat, &read)?;
    if !same_value(&xlat, &xlat0) {
        set_deref(&xlat_ptr, xlat)?;
    }
    if !same_value(&val, &val0) {
        set_deref(&val_ptr, val)?;
    }
    pl::ret_nothing(w)
}

/// The same scalar (both undef, or the same string).
fn same_value(a: &Value, b: &Value) -> bool {
    a.is_undef() == b.is_undef() && a.to_bytes() == b.to_bytes()
}

/// Is this IPTC in a standard location?
/// Inputs: 0) Current metadata path string
/// Returns: true if path is standard, 0 if file type doesn't have standard IPTC,
///          or undef if IPTC is non-standard
fn is_standard_iptc_impl(it: &mut Interp, path: &Value) -> Value {
    let is_standard_iptc = crate::runtime::lex_hash(it, "Image::ExifTool::IPTC", "isStandardIPTC");
    if is_standard_iptc.get(path.to_bytes()).truthy() {
        return n(1);
    }
    match re(r"^(\w+)", "").m(path) {
        Some(m) if !is_standard_iptc.get(cap(&m, 1).to_bytes()).is_undef() => Value::Undef, // non-standard
        _ => n(0),
    }
}

pub fn is_standard_iptc(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(is_standard_iptc_impl(it, &arg(&a, 0)))
}

/// get IPTC info
/// Inputs: 0) ExifTool object reference, 1) dirInfo reference
///         2) reference to tag table
/// Returns: 1 on success, 0 otherwise
pub fn process_iptc(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_table_ptr) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut data_pt = dir_info.get("DataPt");
    let ds = dir_info.get("DirStart");
    let mut pos = if ds.truthy() { ds } else { n(0) };
    let dl = dir_info.get("DirLen");
    let dir_len = if dl.truthy() { dl } else { n(0) };
    let mut dir_end = pl::add(&pos, &dir_len);
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let validate = pl::method(it, &et, "Options", vec![pl::s("Validate")])?;
    let mut success = n(0);
    let (mut last_rec, mut record_ptr, mut record_name) = (Value::Undef, Value::Undef, Value::Undef);
    let seen = Value::new_hash();

    if verbose.truthy() && dir_info.truthy() {
        pl::method(it, &et, "VerboseDir", vec![pl::s("IPTC"), n(0), dir_info.get("DirLen")])?;
    }

    let main = global_hash(it, "Image::ExifTool::IPTC::Main");
    if tag_table_ptr.same_ref(&main) {
        let path = pl::method(it, &et, "MetadataPath", vec![])?;
        let is_std = is_standard_iptc_impl(it, &path);
        if !is_std.is_undef() && !et.path(&["DIR_COUNT", "STD_IPTC"]).truthy() {
            // set flag to ensure we only have one family 1 "IPTC" group
            et.get_or_new_hash("DIR_COUNT").set("STD_IPTC", n(1));
            // calculate MD5 if Digest::MD5 is available (truly standard IPTC only)
            if is_std.truthy() {
                let md5;
                if crate::runtime::require(it, "Digest::MD5").is_ok() {
                    pl::at(1074);
                    if pos.truthy() || !pl::num_eq(&dir_len, &pl::length(&pl::sderef(&data_pt)?)) {
                        // (substr($$dataPt...) as a sub argument autovivifies)
                        let s = substr(it, &data_pt.deref(), &pos, Some(&dir_len))?;
                        md5 = pl::call(it, "Digest::MD5::md5", vec![s])?;
                    } else {
                        md5 = pl::call(it, "Digest::MD5::md5", vec![data_pt.deref()])?;
                    }
                } else {
                    // a zero digest indicates IPTC exists but we don't have Digest::MD5
                    md5 = Value::bytes([0u8; 16]);
                }
                pl::method(it, &et, "FoundTag", vec![pl::s("CurrentIPTCDigest"), md5])?;
            }
        } else {
            if (it.get_scalar("Image::ExifTool::MWG::strict").truthy() || validate.truthy()) && re(r"^(JPEG|TIFF|PSD)$", "").is_match(&et.get("FILE_TYPE")) {
                if it.get_scalar("Image::ExifTool::MWG::strict").truthy() {
                    // ignore non-standard IPTC while in strict MWG compatibility mode
                    pl::method(it, &et, "Warn", vec![pl::concat(&pl::s("Ignored non-standard IPTC at "), &path)])?;
                    return ret(n(1));
                } else {
                    pl::method(it, &et, "Warn", vec![pl::concat(&pl::s("Non-standard IPTC at "), &path), n(1)])?;
                }
            }
            // extract non-standard IPTC
            let dc = et.get_or_new_hash("DIR_COUNT");
            let prev = dc.get("IPTC");
            let count = pl::add(&if prev.truthy() { prev } else { n(0) }, &n(1)); // count non-standard IPTC
            dc.set("IPTC", count.clone());
            et.get_or_new_hash("LOW_PRIORITY_DIR").set("IPTC", n(1)); // lower priority of non-standard IPTC
            et.set("SET_GROUP1", pl::concat(&pl::s("+"), &pl::add(&count, &n(1)))); // add number to family 1 group name
        }
    }
    // begin by assuming default IPTC encoding
    let mut xlat = pl::method(it, &et, "Options", vec![pl::s("CharsetIPTC")])?;
    if xlat.to_bytes() == pl::method(it, &et, "Options", vec![pl::s("Charset")])?.to_bytes() {
        xlat = Value::Undef;
    }

    // quick check for improperly byte-swapped IPTC
    pl::at(1109);
    if pl::ge(&dir_len, &n(4)) && !substr(it, &pl::sderef(&data_pt)?, &pos, Some(&n(1)))?.eq_str("\x1c") && substr(it, &pl::sderef(&data_pt)?, &pl::add(&pos, &n(3)), Some(&n(1)))?.eq_str("\x1c") {
        pl::method(it, &et, "Warn", vec![pl::s("IPTC data was improperly byte-swapped")])?;
        pl::at(1113);
        let s = substr(it, &pl::sderef(&data_pt)?, &pos, Some(&dir_len))?;
        let words = match crate::perl::pack::unpack(b"V*", &pl::concat(&s, &pl::s("\0\0\0")).to_bytes()) {
            Ok(v) => v,
            Err(e) => return die(e),
        };
        let new_data = match crate::perl::pack::pack(b"N*", &words) {
            Ok(b) => b,
            Err(e) => return die(e),
        };
        data_pt = pl::sref(Value::binary(new_data));
        pos = n(0);
        dir_end = pl::add(&pos, &dir_len);
        // NOTE: MUST NOT access $dirInfo DataPt, DirStart or DataLen after this!
    }
    // extract IPTC as a block if specified
    if et.path(&["REQ_TAG_LOOKUP", "iptc"]).truthy() || (et.get("TAGS_FROM_FILE").truthy() && !et.path(&["EXCL_TAG_LOOKUP", "iptc"]).truthy()) {
        pl::at(1123);
        if pos.truthy() || !pl::num_eq(&dir_len, &pl::length(&pl::sderef(&data_pt)?)) {
            // (method arguments: $$dataPt autovivifies there)
            let s = substr(it, &data_pt.deref(), &pos, Some(&dir_len))?;
            pl::method(it, &et, "FoundTag", vec![pl::s("IPTC"), s])?;
        } else {
            pl::method(it, &et, "FoundTag", vec![pl::s("IPTC"), data_pt.deref()])?;
        }
    }
    while pl::le(&pl::add(&pos, &n(5)), &dir_end) {
        pl::at(1130);
        let buff = substr(it, &pl::sderef(&data_pt)?, &pos, Some(&n(5)))?;
        let fields = match crate::perl::pack::unpack(b"CCCn", &buff.to_bytes()) {
            Ok(v) => v,
            Err(e) => return die(e),
        };
        let (id, rec, tag, mut len) = (arg(&fields, 0), arg(&fields, 1), arg(&fields, 2), arg(&fields, 3));
        if !pl::num_eq(&id, &n(0x1c)) {
            if !id.truthy() {
                // scan the rest of the data an give warning unless all zeros
                // (iMatch pads the IPTC block with nulls for some reason)
                pl::at(1136);
                let remaining = substr(it, &pl::sderef(&data_pt)?, &pos, Some(&pl::sub(&dir_end, &pos)))?;
                if !re(r"[^\0]", "").is_match(&remaining) {
                    break;
                }
            }
            pl::method(it, &et, "Warn", vec![pl::sprintf("Bad IPTC data tag (marker 0x%x)", &[id])])?;
            break;
        }
        pos = pl::add(&pos, &n(5)); // step to after field header
        // handle extended IPTC entry if necessary
        if ops::bit_and(&len, &n(0x8000)).truthy() {
            let mut nn = ops::bit_and(&len, &n(0x7fff)); // get num bytes in length field
            if pl::gt(&pl::add(&pos, &nn), &dir_end) || pl::gt(&nn, &n(8)) {
                let msg = format!("Invalid extended IPTC entry (dataset {}:{}, len {})\n", rec.to_text(), tag.to_text(), len.to_text());
                pl::method(it, &et, "VPrint", vec![n(0), pl::s(&msg)])?;
                success = n(0);
                break;
            }
            // determine length (a big-endian, variable sized int)
            len = n(0);
            while nn.truthy() {
                pl::at(1153);
                let o = ord_at(it, &pl::sderef(&data_pt)?, &pos)?;
                len = pl::add(&pl::mul(&len, &n(256)), &o);
                pos = pl::add(&pos, &n(1));
                nn = pl::sub(&nn, &n(1));
            }
        }
        if pl::gt(&pl::add(&pos, &len), &dir_end) {
            let msg = format!("Invalid IPTC entry (dataset {}:{}, len {})\n", rec.to_text(), tag.to_text(), len.to_text());
            pl::method(it, &et, "VPrint", vec![n(0), pl::s(&msg)])?;
            success = n(0);
            break;
        }
        if last_rec.is_undef() || !pl::num_eq(&last_rec, &rec) {
            if validate.truthy() && !last_rec.is_undef() && pl::lt(&rec, &last_rec) {
                pl::method(it, &et, "Warn", vec![pl::s("IPTC doesn't conform to spec: Records out of sequence"), n(1)])?;
            }
            let table_info = tag_table_ptr.get(rec.to_bytes());
            if !table_info.truthy() {
                pl::method(it, &et, "Warn", vec![pl::s(&format!("Unrecognized IPTC record {} (ignored)", rec.to_text()))])?;
                pos = pl::add(&pos, &len);
                continue; // ignore this entry
            }
            let table_name = table_info.path(&["SubDirectory", "TagTable"]);
            if !table_name.truthy() {
                pl::method(it, &et, "Warn", vec![pl::s(&format!("No table for IPTC record {}!", rec.to_text()))])?;
                break; // this shouldn't happen
            }
            record_name = table_info.get("Name");
            record_ptr = pl::call(it, "Image::ExifTool::GetTagTable", vec![table_name])?;
            let msg = pl::concat(&pl::concat(&pl::s("-- "), &record_name), &pl::s(" record --\n"));
            pl::method(it, &et, "VPrint", vec![n(0), et.get("INDENT"), msg])?;
            last_rec = rec.clone();
        }
        pl::at(1181);
        let mut val = substr(it, &pl::sderef(&data_pt)?, &pos, Some(&len))?;

        // add tagInfo for all unknown tags:
        if !record_ptr.get(tag.to_bytes()).truthy() {
            // - no Format so format is auto-detected
            // - no Name so name is generated automatically with decimal tag number
            let ti = Value::new_hash();
            ti.set("Unknown", n(1));
            pl::call(it, "Image::ExifTool::AddTagToTable", vec![record_ptr.clone(), tag.clone(), ti])?;
        }
        let mut tag_info = pl::method(it, &et, "GetTagInfo", vec![record_ptr.clone(), tag.clone()])?;
        if validate.truthy() {
            // $$tagInfo{List} autovivifies an undefined $tagInfo (an unknown
            // tag without the Unknown option): a new empty hash each time
            if tag_info.is_undef() {
                tag_info = Value::new_hash();
            }
        }
        if validate.truthy() && !tag_info.get("List").truthy() && !tag_info.get("Unknown").truthy() {
            if seen.get(tag_info.to_bytes()).truthy() {
                let msg = pl::concat(&pl::concat(&pl::s("Multiple IPTC "), &tag_info.get("Name")), &pl::s(" tags"));
                pl::method(it, &et, "Warn", vec![msg])?;
            }
            seen.set(tag_info.to_bytes(), n(1));
        }
        let mut format = Value::Undef;
        // (could use $$recordPtr{FORMAT} if no Format below, but don't do this to
        //  be backward compatible with improperly written PhotoMechanic tags)
        if tag_info.truthy() {
            format = tag_info.get("Format");
        }
        if !format.truthy() {
            // guess at "int" format if not specified
            if pl::le(&len, &n(4)) && !pl::num_eq(&len, &n(3)) && re(r"[\0-\x08]", "").is_match(&val) {
                format = pl::s("int");
            }
        } else if validate.truthy() {
            let (fmt, mut min, mut max);
            if let Some(m) = re(r"(.*)\[(\d+)(,(\d+))?\]", "").m(&format) {
                fmt = cap(&m, 1);
                min = cap(&m, 2);
                max = if cap(&m, 4).truthy() { cap(&m, 4) } else { cap(&m, 2) };
            } else {
                fmt = format.clone();
                min = n(1);
                max = n(1);
            }
            let fs = pl::call(it, "Image::ExifTool::FormatSize", vec![fmt])?;
            let siz = if fs.truthy() { fs } else { n(1) };
            min = pl::mul(&min, &siz);
            max = pl::mul(&max, &siz);
            if pl::lt(&len, &min) || pl::gt(&len, &max) {
                let should = if pl::num_eq(&min, &max) {
                    min.clone()
                } else if pl::lt(&len, &min) {
                    pl::concat(&min, &pl::s(" min"))
                } else {
                    pl::concat(&max, &pl::s(" max"))
                };
                let what = if pl::lt(&len, &pl::mul(&siz, &min)) { "short" } else { "long" };
                let msg = format!("IPTC {} too {what} ({} bytes; should be {})", tag_info.get("Name").to_text(), len.to_text(), should.to_text());
                pl::method(it, &et, "Warn", vec![pl::s(&msg), n(1)])?;
            }
        }
        if format.truthy() {
            if re(r"^int", "").is_match(&format) {
                if pl::le(&len, &n(8)) {
                    // limit integer conversion to 8 bytes long
                    val = n(0);
                    let mut i = n(0);
                    while pl::lt(&i, &len) {
                        pl::at(1225);
                        let o = ord_at(it, &pl::sderef(&data_pt)?, &pl::add(&pos, &i))?;
                        val = pl::add(&pl::mul(&val, &n(256)), &o);
                        i = pl::add(&i, &n(1));
                    }
                }
            } else if re(r"^string", "").is_match(&format) {
                // some braindead softwares add null terminators
                if re(r"\0+$", "").subst(&mut val, false, |_| Vec::new()) > 0 && validate.truthy() {
                    let msg = pl::concat(&pl::concat(&pl::s("IPTC "), &tag_info.get("Name")), &pl::s(" improperly terminated"));
                    pl::method(it, &et, "Warn", vec![msg, n(1)])?;
                }
                if pl::num_eq(&rec, &n(1)) {
                    // handle CodedCharacterSet tag
                    if pl::num_eq(&tag, &n(90)) {
                        xlat = handle_coded_charset_impl(it, &et, &val)?;
                    }
                // translate characters if necessary and special characters exist
                } else if xlat.truthy() && pl::lt(&rec, &n(7)) && re(r"[\x80-\xff]", "").is_match(&val) {
                    // translate to specified character set
                    translate_coded_string_impl(it, &et, &mut val, &mut xlat, &n(1))?;
                }
            } else if re(r"^digits", "").is_match(&format) {
                if re(r"\0+$", "").subst(&mut val, false, |_| Vec::new()) > 0 && validate.truthy() {
                    let msg = pl::concat(&pl::concat(&pl::s("IPTC "), &tag_info.get("Name")), &pl::s(" improperly terminated"));
                    pl::method(it, &et, "Warn", vec![msg, n(1)])?;
                }
            } else if !re(r"^undef", "").is_match(&format) {
                pl::at(1246);
                pl::warn(it, &format!("Invalid IPTC format: {}", format.to_text()))?; // (this would be a programming error)
            }
        }
        if verbose.truthy() {
            pl::method(
                it,
                &et,
                "VerboseInfo",
                vec![
                    tag.clone(),
                    tag_info.clone(),
                    pl::s("Table"),
                    tag_table_ptr.clone(),
                    pl::s("Value"),
                    val.clone(),
                    pl::s("DataPt"),
                    data_pt.clone(),
                    pl::s("DataPos"),
                    dir_info.get("DataPos"),
                    pl::s("Size"),
                    len.clone(),
                    pl::s("Start"),
                    pos.clone(),
                    pl::s("Extra"),
                    pl::concat(&pl::concat(&pl::s(", "), &record_name), &pl::s(" record")),
                    pl::s("Format"),
                    format.clone(),
                ],
            )?;
        }
        if tag_info.truthy() {
            pl::method(it, &et, "FoundTag", vec![tag_info.clone(), val])?;
        }
        success = n(1);

        pos = pl::add(&pos, &len); // increment to next field
    }
    et.delete("SET_GROUP1");
    et.get_or_new_hash("LOW_PRIORITY_DIR").delete("IPTC");
    ret(success)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::IPTC::AUTOLOAD", autoload);
    it.define_native("Image::ExifTool::IPTC::PrintCodedCharset", print_coded_charset);
    it.define_native("Image::ExifTool::IPTC::HandleCodedCharset", handle_coded_charset);
    it.define_native("Image::ExifTool::IPTC::TranslateCodedString", translate_coded_string);
    it.define_native("Image::ExifTool::IPTC::IsStandardIPTC", is_standard_iptc);
    it.define_native("Image::ExifTool::IPTC::ProcessIPTC", process_iptc);
}
