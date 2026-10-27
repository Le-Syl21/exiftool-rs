//! Image::ExifTool::Kandao (Image/ExifTool/Kandao.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Kandao MP4 metadata ('kfix' and 'kvar' atoms) and KVAR files. The
//! tag tables are data.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::quick_time_stream::{cat, get32u, get_tag_table, hash_of, m, n, new_doc, opt, raf_read, raf_seek, raf_tell, read_value, s, scalar, set_byte_order, warn};
use crate::perl::Value;
use crate::pl::{self, arg, cap, re, ret};
use crate::runtime;

const PKG: &str = "Image::ExifTool::Kandao";

fn format_size(it: &mut Interp, fmt: Value) -> ER<Value> {
    scalar(crate::modules::exiftool::part2::format_size(it, vec![fmt], Want::Scalar))
}

/// Extract information from a Kandao 'kfix' and 'kvar' atoms
/// Inputs: 0) ExifTool ref, 1) dirInfo reference, 2) tag table ref
/// Returns: 1 on success
pub fn process_kandao(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info, tag_tbl) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    let mut raf = dir_info.get("RAF");
    if !raf.truthy() {
        raf = pl::method(it, &s("File::RandomAccess"), "new", vec![dir_info.get("DataPt")])?;
    }
    let dir_name = dir_info.get("DirName");
    let dp = dir_info.get("DataPos");
    let base = dir_info.get("Base");
    let data_pos = pl::add(&if dp.truthy() { dp } else { n(0) }, &if base.truthy() { base } else { n(0) });
    let verbose = opt(it, &et, "Verbose")?;
    let ee = opt(it, &et, "ExtractEmbedded")?;
    // extract as a block from MP4 if specified
    let block_info = dir_info.get("BlockInfo");
    if block_info.truthy() {
        let block_name = block_info.get("Name");
        let block_extract = opt(it, &et, "BlockExtract")?;
        let lc_name = pl::lc(&block_name);
        if block_extract.truthy() || et.get("REQ_TAG_LOOKUP").get(lc_name.to_bytes()).truthy() || (et.get("TAGS_FROM_FILE").truthy() && !et.get("EXCL_TAG_LOOKUP").get(lc_name.to_bytes()).truthy()) {
            m(it, &et, "FoundTag", vec![block_info.clone(), dir_info.get("DataPt")])?;
            if block_extract.truthy() && pl::gt(&block_extract, &n(1)) {
                return ret(n(1));
            }
        }
    }
    let format_hash = runtime::lex_hash(it, PKG, "format");
    let mut buff = Value::Undef;
    let buff_ref = pl::sref(Value::Undef); // (\$buff)
    let mut err = Value::Undef;
    set_byte_order(it, s("II"))?;
    if !pl::num_eq(&raf_read(it, &raf, &mut buff, n(4))?, &n(4)) {
        return ret(n(0));
    }
    pl::set_deref(&buff_ref, buff.clone())?;
    let nn = get32u(it, &buff_ref, n(0))?;
    m(it, &et, "VerboseDir", vec![dir_name.clone(), nn.clone()])?;
    let mut i = n(0);
    while pl::lt(&i, &nn) {
        if !pl::num_eq(&raf_read(it, &raf, &mut buff, n(0x2c))?, &n(0x2c)) {
            err = s("");
            break;
        }
        pl::set_deref(&buff_ref, buff.clone())?;
        let mut tag = pl::substr(&buff, 0, Some(0x20));
        re(r"\0+$", "").subst(&mut tag, false, |_| Vec::new());
        let mut fmt = pl::substr(&buff, 0x20, Some(8));
        re(r"\0+$", "").subst(&mut fmt, false, |_| Vec::new());
        let mut format = format_hash.get(fmt.to_bytes());
        if !format.truthy() {
            err = cat(&[&s("Unknown Kandao format '"), &fmt, &s("'")]);
            break;
        }
        let mut num = get32u(it, &buff_ref, n(0x28))?;
        let fs = format_size(it, format.clone())?;
        let size = pl::mul(&num, &fs);
        let parms_data_pos = pl::add(&data_pos, &raf_tell(it, &raf)?);
        let mut parms_format = format.clone();
        if !pl::num_eq(&raf_read(it, &raf, &mut buff, size.clone())?, &size) {
            err = s("");
            break;
        }
        pl::set_deref(&buff_ref, buff.clone())?;
        let mut tag_info = m(it, &et, "GetTagInfo", vec![tag_tbl.clone(), tag.clone(), buff_ref.clone()])?;
        if !tag_info.truthy() {
            let mut name = pl::ucfirst(&pl::lc(&tag));
            re(r"_([a-z])", "").subst(&mut name, true, |c| {
                let mut v = b"_".to_vec();
                v.extend(cap(c, 1).to_bytes().to_ascii_uppercase());
                v
            });
            name = pl::call(it, "Image::ExifTool::MakeTagName", vec![name])?;
            m(it, &et, "VPrint", vec![n(0), cat(&[&et.get("INDENT"), &s("[adding "), &dir_name, &s(" '"), &tag, &s("']\n")])])?;
            tag_info = pl::call(it, "Image::ExifTool::AddTagToTable", vec![tag_tbl.clone(), tag.clone(), hash_of(vec![("Name", name)])])?;
        }
        let rec_len = tag_info.get("RecordSize");
        if rec_len.truthy() {
            if verbose.truthy() {
                m(it, &et, "VerboseInfo", vec![tag.clone(), tag_info.clone(), s("DataPt"), buff_ref.clone(), s("DataPos"), parms_data_pos.clone(), s("Index"), i.clone(), s("Format"), parms_format.clone()])?;
            }
            let tbl = pl::call(it, "Image::ExifTool::GetTagTable", vec![tag_info.get("SubDirectory").get("TagTable")])?;
            let st = tag_info.get("SubDirectory").get("Start");
            let mut pt = if st.truthy() { st } else { n(0) };
            let sub_dir_info = hash_of(vec![("DataPt", buff_ref.clone()), ("DataPos", parms_data_pos.clone()), ("DirLen", rec_len.clone()), ("DirName", tag.clone())]);
            let n_limit = n(10000);
            let mut size_limit = size.clone();
            if ee.truthy() && pl::gt(&pl::int(&pl::div(&pl::sub(&size, &pt), &rec_len)?), &n_limit) && m(it, &et, "Warn", vec![cat(&[&s("Extracting only the first "), &n_limit, &s(" "), &tag_info.get("Name"), &s(" records")]), n(2)])?.truthy() {
                size_limit = pl::add(&pt, &pl::mul(&rec_len, &n_limit));
            }
            while pl::le(&pl::add(&pt, &rec_len), &size_limit) {
                new_doc(&et);
                sub_dir_info.set("DirStart", pt.clone());
                m(it, &et, "ProcessDirectory", vec![sub_dir_info.clone(), tbl.clone()])?;
                if !ee.truthy() {
                    m(it, &et, "Warn", vec![s("Use the ExtractEmbedded option to extract all timed metadata"), n(7)])?;
                    break;
                }
                pt = pl::add(&pt, &rec_len);
            }
            et.delete("DOC_NUM");
        } else {
            let val;
            if fmt.eq_str("U8") && pl::gt(&num, &n(1)) && !tag_info.get("Format").truthy() {
                val = buff_ref.clone();
            } else {
                // override format if necessary
                if tag_info.get("Format").truthy() {
                    format = tag_info.get("Format");
                    parms_format = cat(&[&parms_format, &s(" read as "), &format]);
                    let fs = format_size(it, format.clone())?;
                    num = pl::int(&pl::div(&size, &fs)?);
                }
                val = read_value(it, &buff_ref, n(0), format.clone(), num.clone(), size.clone())?;
            }
            let key = m(it, &et, "HandleTag", vec![tag_tbl.clone(), tag.clone(), val, s("DataPt"), buff_ref.clone(), s("DataPos"), parms_data_pos.clone(), s("Index"), i.clone(), s("Format"), parms_format.clone()])?;
            if !dir_name.eq_str("KVAR") {
                m(it, &et, "SetGroup", vec![key, dir_name.clone()])?;
            }
        }
        i = pl::add(&i, &n(1));
    }
    if !err.is_undef() {
        let msg = if err.truthy() { err } else { cat(&[&s("Truncated "), &dir_name, &s(" record")]) };
        warn(it, &et, msg)?;
    }
    ret(n(1))
}

/// Process Kandao KVAR file
/// Inputs: 0) ExifTool ref, 1) dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid KVAR file
pub fn process_kvar(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let mut buff = Value::Undef;

    // verify this is a valid KVAR file
    if !pl::num_eq(&raf_read(it, &raf, &mut buff, n(44))?, &n(44)) {
        return ret(n(0));
    }
    if !re(r"^.{2}\0\0[A-Z].{31}(CHAR|BOOL|[US](8|16|32|64)|FLOAT|DOUBLE)\0", "s").is_match(&buff) {
        return ret(n(0));
    }
    m(it, &et, "SetFileType", vec![s("KVAR"), s("application/x-kandaostudio")])?;
    if !raf_seek(it, &raf, n(0), 0)? {
        warn(it, &et, s("Seek error"))?;
        return ret(n(1));
    }
    dir_info.set("DirName", s("KVAR"));
    let tag_table_ptr = get_tag_table(it, "Image::ExifTool::Kandao::Main")?;
    let r = m(it, &et, "ProcessDirectory", vec![dir_info, tag_table_ptr])?;
    ret(r)
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::Kandao::ProcessKandao", process_kandao);
    it.define_native("Image::ExifTool::Kandao::ProcessKVAR", process_kvar);
}
