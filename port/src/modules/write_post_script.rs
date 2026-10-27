//! Image::ExifTool::WritePostScript (Image/ExifTool/WritePostScript.pl), ported sub by sub (see docs/port/PORTING.md).
//!
//! Write PostScript meta information

use crate::interp::eval::{Cell, ER};
use crate::interp::{Interp, Want};
use crate::modules::random_access::perl_eval1;
use crate::modules::write_exif::{cat, cell, etf, g32u, get, inc, keys, len, put, read, read_eq, rep, rf, s16u_at, s32u_at, seek, set_byte_order, sort_str, substr, wlex_hash, write};
use crate::perl::regex as pre;
use crate::perl::Value;
use crate::pl::{self, arg, cap, re, ret};

const MODULE: &str = "Image::ExifTool::PostScript";
const FILE: &str = "WritePostScript.pl";

fn n(i: i64) -> Value {
    Value::Int(i)
}

/// `$/`
fn irs(it: &mut Interp) -> Value {
    it.get_scalar("main::/")
}

/// `Image::ExifTool::PostScript::NAME(ARGS)`
fn ps(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("{MODULE}::{name}"), args)
}

/// The end of the first match of `pat` in `s` at or after `start`
/// (`pos($s)` after `$s =~ /pat/g`), with the captures.
fn match_from(pat: &str, flags: &str, s: &[u8], start: usize) -> Option<(usize, pl::Caps)> {
    let r = pre::compile(pat.as_bytes(), flags).unwrap_or_else(|e| panic!("bad pattern /{pat}/: {e}"));
    match r.match_at(s, start) {
        Ok(Some(sp)) => {
            let caps = sp.iter().map(|g| g.map(|(a, b)| Value::bytes(&s[a..b]))).collect();
            Some((sp[0].unwrap().1, caps))
        }
        _ => None,
    }
}

/// Write XMP directory to file, with begin/end tokens if necessary
fn write_xmp_dir_v(it: &mut Interp, outfile: &Value, flags: &Value, data: Vec<Value>) -> ER<Value> {
    let mut success = n(1);
    let nl = irs(it);
    if !flags.get("WROTE_BEGIN").truthy() && !write(it, outfile, vec![pl::concat(&pl::s("%begin_xml_code"), &nl)])? {
        success = n(0);
    }
    if !write(it, outfile, data)? {
        success = n(0);
    }
    if !flags.get("WROTE_BEGIN").truthy() && !write(it, outfile, vec![pl::concat(&pl::s("%end_xml_code"), &nl)])? {
        success = n(0);
    }
    Ok(success)
}

/// Write XMP directory to file, with begin/end tokens if necessary
/// Inputs: 0) outfile ref, 1) flags hash ref, 2-N) data to write
/// Returns: true on success
pub fn write_xmp_dir(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let outfile = arg(&a, 0);
    let flags = arg(&a, 1);
    let data = a.get(2..).map(|s| s.to_vec()).unwrap_or_default();
    ret(write_xmp_dir_v(it, &outfile, &flags, data)?)
}

/// Write a directory inside a PS document
/// Inputs: 0) ExifTool object ref, 1) output file reference,
///         2) Directory name, 3) data reference, 4) flags hash ref
/// Returns: 0=error, 1=nothing written, 2=dir written ok
pub fn write_ps_directory(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, outfile, dir_name, mut data_pt, flags) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3), arg(&a, 4));
    let mut success = n(2);
    let mut len_ = if data_pt.truthy() { len(&data_pt.deref()) } else { n(0) };
    let create = if len_.truthy() { n(0) } else { n(1) };
    let dir_info = Value::new_hash();
    dir_info.set("DataPt", data_pt.clone());
    dir_info.set("DataLen", len_.clone());
    dir_info.set("DirStart", n(0));
    dir_info.set("DirLen", len_.clone());
    dir_info.set("DirName", dir_name.clone());
    dir_info.set("Parent", pl::s("PostScript"));
    // Note: $$flags{WROTE_BEGIN} may be 1 for XMP (it is always 0 for
    // other dirs, but if 1, the begin/end markers were already written)
    //
    // prepare necessary postscript code to support embedded XMP
    //
    let (mut before_xmp, mut after_xmp, mut reported_len) = (Value::Undef, Value::Undef, Value::Undef);
    if dir_name.eq_str("XMP") && len_.truthy() {
        // isolate the XMP
        let d = data_pt.deref().to_bytes();
        let Some((p1, c1)) = match_from(r"(.*)(<\?xpacket begin=.{7,13}W5M0MpCehiHzreSzNTczkc9d)", "s", &d, 0) else {
            pl::method(it, &et, "Warn", vec![pl::s("No XMP packet start")])?;
            return ret(write_xmp_dir_v(it, &outfile, &flags, vec![data_pt.deref()])?);
        };
        before_xmp = cap(&c1, 1);
        let mut xmp = cap(&c1, 2);
        let Some((p2, _)) = match_from(r"<\?xpacket end=.(w|r).\?>", "s", &d, p1) else {
            pl::method(it, &et, "Warn", vec![pl::s("No XMP packet end")])?;
            return ret(write_xmp_dir_v(it, &outfile, &flags, vec![data_pt.deref()])?);
        };
        xmp = pl::concat(&xmp, &Value::bytes(&d[p1..p2]));
        after_xmp = Value::bytes(&d[p2..]);
        // determine if we can adjust the XMP size
        if let Some(c) = re(r"%begin_xml_packet: (\d+)", "s").m(&before_xmp) {
            reported_len = cap(&c, 1);
            let mut p = br"\b".to_vec();
            p.extend_from_slice(&reported_len.to_bytes());
            p.extend_from_slice(br"\b");
            let matches = pl::re_dyn(&p, "s")?.all(&before_xmp);
            if matches.len() != 2 {
                reported_len = Value::Undef;
            }
        }
        // must edit in place if we can't reliably change the XMP length
        if !reported_len.truthy() {
            dir_info.set("InPlace", n(1));
        }
        // process XMP only
        dir_info.set("DataLen", len(&xmp));
        dir_info.set("DirLen", len(&xmp));
        dir_info.set("DataPt", pl::sref(xmp));
    }
    let tag_table_ptr = etf(it, "GetTagTable", vec![pl::s(&format!("Image::ExifTool::{}::Main", dir_name.to_text()))])?;
    let mut val = pl::method(it, &et, "WriteDirectory", vec![dir_info.clone(), tag_table_ptr])?;
    if !val.is_undef() {
        data_pt = pl::sref(val.clone()); // use modified directory
        len_ = len(&val);
    } else if dir_name.eq_str("XMP") {
        if !len_.truthy() {
            return ret(n(1));
        }
        // just write the original XMP
        return ret(write_xmp_dir_v(it, &outfile, &flags, vec![data_pt.deref()])?);
    }
    if !len_.truthy() {
        if create.truthy() || !dir_name.eq_str("XMP") {
            return ret(n(1)); // nothing to create
        }
        // it would be really difficult to delete the XMP,
        // so instead we write a blank XMP record
        let version = it.get_scalar("Image::ExifTool::VERSION");
        val = cat(&[
            Value::bytes(b"<?xpacket begin='\xef\xbb\xbf' id='W5M0MpCehiHzreSzNTczkc9d'?>\n<x:xmpmeta xmlns:x='adobe:ns:meta/' x:xmptk='Image::ExifTool "),
            version,
            pl::s("'>\n</x:xmpmeta>\n"),
        ]);
        if !et.get("OPTIONS").get("Compact").get("NoPadding").truthy() {
            let line = cat(&[rep(b" ", &n(100)), pl::s("\n")]);
            val = pl::concat(&val, &rep(&line.to_bytes(), &n(24)));
        }
        val = pl::concat(&val, &pl::s("<?xpacket end='w'?>"));
        data_pt = pl::sref(val.clone());
        len_ = len(&val);
    }
    //
    // write XMP directory
    //
    if dir_name.eq_str("XMP") {
        if create.truthy() {
            // create necessary PS/EPS code to support XMP
            let l = len_.to_text();
            before_xmp = pl::s(&format!(
                "/pdfmark where {{pop true}} {{false}} ifelse\n\
/currentdistillerparams where {{pop currentdistillerparams\n\
/CoreDistVersion get 5000 ge }} {{false}} ifelse\n\
and not {{userdict /pdfmark /cleartomark load put}} if\n\
[/NamespacePush pdfmark\n\
[/_objdef {{exiftool_metadata_stream}} /type /stream /OBJ pdfmark\n\
[{{exiftool_metadata_stream}} 2 dict begin /Type /Metadata def\n  \
/Subtype /XML def currentdict end /PUT pdfmark\n\
/MetadataString {l} string def % exact length of metadata\n\
/TempString 100 string def\n\
/ConsumeMetadata {{\n\
currentfile TempString readline pop pop\n\
currentfile MetadataString readstring pop pop\n\
}} bind def\n\
ConsumeMetadata\n\
%begin_xml_packet: {l}\n"
            ));
            // note: use q() to get necessary linefeed before %end_xml_packet
            after_xmp = pl::s("\n%end_xml_packet\n[{exiftool_metadata_stream} MetadataString /PUT pdfmark\n");
            if flags.get("EPS").truthy() {
                after_xmp = pl::concat(&after_xmp, &pl::s("[/Document 1 dict begin\n  /Metadata {exiftool_metadata_stream} def currentdict end /BDC pdfmark\n[/NamespacePop pdfmark\n"));
                // write this at end of file
                let nl = irs(it);
                flags.set("TRAILER", pl::concat(&pl::s("[/EMC pdfmark"), &nl));
            } else {
                // PS
                after_xmp = pl::concat(&after_xmp, &pl::s("[{Catalog} {exiftool_metadata_stream} /Metadata pdfmark\n[/NamespacePop pdfmark\n"));
            }
            let nl = irs(it).to_bytes();
            re(r"\n", "s").subst(&mut before_xmp, true, |_| nl.to_vec()); // use proper newline characters
            re(r"\n", "s").subst(&mut after_xmp, true, |_| nl.to_vec());
        } else {
            // replace xmp size in PS code
            if reported_len.truthy() {
                let mut p = br"\b".to_vec();
                p.extend_from_slice(&reported_len.to_bytes());
                p.extend_from_slice(br"\b");
                let lb = len_.to_bytes();
                pl::re_dyn(&p, "s")?.subst(&mut before_xmp, true, |_| lb.to_vec());
            }
        }
        if !write_xmp_dir_v(it, &outfile, &flags, vec![before_xmp, data_pt.deref(), after_xmp])?.truthy() {
            success = n(0);
        }
    //
    // Write Photoshop or ICC_Profile directory
    //
    } else if dir_name.eq_str("Photoshop") || dir_name.eq_str("ICC_Profile") {
        let (start_token, end_token);
        if dir_name.eq_str("Photoshop") {
            start_token = pl::concat(&pl::s("%BeginPhotoshop: "), &len_);
            end_token = pl::s("%EndPhotoshop");
        } else {
            start_token = pl::s("%%BeginICCProfile: (Photoshop Profile) -1 Hex");
            end_token = pl::s("%%EndICCProfile");
        }
        let nl = irs(it);
        if !write(it, &outfile, vec![start_token, nl.clone()])? {
            success = n(0);
        }
        // write as an ASCII-hex comment
        let mut i = n(0);
        let mut wid = n(32);
        while pl::lt(&i, &len_) {
            if pl::gt(&wid, &pl::sub(&len_, &i)) {
                wid = pl::sub(&len_, &i);
            }
            let dat = substr(it, &data_pt.deref(), &i, Some(&wid))?;
            let hex = pl::uc(&pl::unpack1("H*", &dat)?);
            if !write(it, &outfile, vec![pl::s("% "), hex, nl.clone()])? {
                success = n(0);
            }
            i = pl::add(&i, &wid);
        }
        if !write(it, &outfile, vec![end_token, nl])? {
            success = n(0);
        }
    } else {
        pl::method(it, &et, "Warn", vec![pl::concat(&pl::s("Can't write PS directory "), &dir_name)])?;
    }
    ret(success)
}

/// EncodeTag on values.
fn encode_tag_v(it: &mut Interp, tag: &Value, val: &Value) -> ER<Value> {
    let mut val = val.clone();
    if !re(r"^\d+$", "").is_match(&val) {
        re(r"([()\\])", "").subst(&mut val, true, |c| [b"\\", &cap(c, 1).to_bytes()[..]].concat()); // escape brackets and backslashes
        re(r"\n", "").subst(&mut val, true, |_| b"\\n".to_vec()); // escape newlines
        re(r"\r", "").subst(&mut val, true, |_| b"\\r".to_vec()); // escape carriage returns
        re(r"\t", "").subst(&mut val, true, |_| b"\\t".to_vec()); // escape tabs
        // use octal escape codes for other control characters
        re(r"([\x00-\x1f\x7f\xff])", "").subst(&mut val, true, |c| pl::sprintf("\\%.3o", &[crate::modules::write_exif::ord(&cap(c, 1))]).to_bytes().to_vec());
        val = cat(&[pl::s("("), val, pl::s(")")]);
    }
    let mut line = cat(&[pl::s("%%"), tag.clone(), pl::s(": "), val]);
    // postscript line limit is 255 characters (but it seems that
    // the limit may be 254 characters if the DOS CR/LF is used)
    // --> split if necessary using continuation comment "%%+"
    let nl = irs(it);
    let mut nn = n(254);
    while pl::gt(&len(&line), &nn) {
        line = crate::modules::write_exif::substr_repl(&line, &nn, Some(&n(0)), &pl::concat(&nl, &pl::s("%%+")))?;
        nn = pl::add(&nn, &pl::add(&n(254), &len(&nl)));
    }
    Ok(pl::concat(&line, &nl))
}

/// Encode postscript tag/value
/// Inputs: 0) tag ID, 1) value
/// Returns: postscript comment
/// - adds brackets, escapes special characters, and limits line length
pub fn encode_tag(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (tag, val) = (arg(&a, 0), arg(&a, 1));
    ret(encode_tag_v(it, &tag, &val)?)
}

/// `%$h = ()`
fn clear_hash(h: &Value) {
    if let Value::HashRef(r) = h {
        r.borrow_mut().clear();
    }
}

fn write_new_tags_v(it: &mut Interp, et: &Value, outfile: &Value, new_tags: &Value) -> ER<Value> {
    let mut success = n(1);

    // get XMP hint and remove from tags hash
    let xmp_hint = new_tags.get("XMP_HINT");
    new_tags.delete("XMP_HINT");

    let mut tags = keys(new_tags);
    sort_str(&mut tags);
    for tag in tags {
        let tag_info = new_tags.get(tag.to_bytes());
        let nv_hash = pl::method(it, et, "GetNewValueHash", vec![tag_info.clone()])?;
        if !nv_hash.get("IsCreating").truthy() {
            continue;
        }
        let val = pl::method(it, et, "GetNewValue", vec![nv_hash])?;
        pl::method(it, et, "VerboseValue", vec![pl::concat(&pl::s("+ PostScript:"), &tag_info.get("Name")), val.clone()])?;
        let enc = encode_tag_v(it, &tag, &val)?;
        if !write(it, outfile, vec![enc])? {
            success = n(0);
        }
        inc(et, "CHANGED");
    }
    // write XMP hint if necessary
    if xmp_hint.truthy() {
        let nl = irs(it);
        if !write(it, outfile, vec![pl::concat(&pl::s("%ADO_ContainsXMP: MainFirst"), &nl)])? {
            success = n(0);
        }
    }

    clear_hash(new_tags); // all done with new tags
    Ok(success)
}

/// Write new tags information in comments section
/// Inputs: 0) ExifTool object ref, 1) output file ref, 2) reference to new tag hash
/// Returns: true on success
pub fn write_new_tags(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, outfile, new_tags) = (arg(&a, 0), arg(&a, 1), arg(&a, 2));
    ret(write_new_tags_v(it, &et, &outfile, &new_tags)?)
}

/// `WritePSDirectory(...)` called from this file.
fn write_ps_dir(it: &mut Interp, et: &Value, outfile: &Value, mode: &Value, data_pt: Value, flags: &Value) -> ER<bool> {
    let r = write_ps_directory(it, vec![et.clone(), outfile.clone(), mode.clone(), data_pt, flags.clone()], Want::Scalar)?;
    Ok(arg(&r, 0).truthy())
}

/// `$raf->ReadLine($data)`
fn read_line(it: &mut Interp, raf: &Value, data: &Cell) -> ER<Value> {
    let r = cell(raf.clone());
    perl_eval1(it, "$raf->ReadLine($data)", MODULE, &[("raf", &r), ("data", data)])
}

/// Write PS file
/// Inputs: 0) ExifTool object reference, 1) source dirInfo reference
/// Returns: 1 on success, 0 if this wasn't a valid PS file,
///          or -1 if a write error occurred
pub fn write_ps(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    if !et.truthy() {
        return ret(n(1)); // allow dummy access to autoload this package
    }
    let tag_table_ptr = etf(it, "GetTagTable", vec![pl::s("Image::ExifTool::PostScript::Main")])?;
    let raf = dir_info.get("RAF");
    let outfile = dir_info.get("OutFile");
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;
    let out = pl::method(it, &et, "Options", vec![pl::s("TextOut")])?;
    let (data, buff) = (cell(Value::Undef), cell(Value::Undef));
    let flags = Value::new_hash();
    let mut err = Value::Undef;
    let (mut mode, mut end_token) = (Value::Undef, Value::Undef);
    let dos = cell(Value::Undef);
    let mut ps_new_start = Value::Undef;
    let mut xmp_hint = Value::Undef;
    let lines = Value::new_array(vec![]);

    if !read_eq(it, &raf, &data, &n(4))? {
        return ret(n(0));
    }
    if !re(r"^(%!PS|%!Ad|\xc5\xd0\xd3\xc6)", "").is_match(&get(&data)) {
        return ret(n(0));
    }

    if re(r"^%!Ad", "").is_match(&get(&data)) {
        // I've seen PS files start with "%!Adobe-PS"...
        if !(read_eq(it, &raf, &buff, &n(6))? && get(&buff).eq_str("obe-PS")) {
            return ret(n(0));
        }
        put(&data, pl::concat(&get(&data), &get(&buff)));
    } else if re(r"^\xc5\xd0\xd3\xc6", "").is_match(&get(&data)) {
        //
        // process DOS binary PS files
        //
        // save DOS header then seek ahead and check PS header
        if !read_eq(it, &raf, &dos, &n(26))? {
            return ret(n(0));
        }
        put(&dos, pl::concat(&get(&data), &get(&dos)));
        set_byte_order(it, pl::s("II"))?;
        let ps_start = g32u(it, &rf(&dos), n(4))?;
        if !(seek(it, &raf, ps_start.clone(), 0)? && read_eq(it, &raf, &data, &n(4))? && get(&data).eq_str("%!PS")) {
            pl::method(it, &et, "Error", vec![pl::s("Invalid PS header")])?;
            return ret(n(1));
        }
        let e = g32u(it, &rf(&dos), n(8))?;
        raf.set("PSEnd", pl::add(&ps_start, &e));
        let mut base = g32u(it, &rf(&dos), n(20))?;
        s16u_at(it, n(0xffff), &rf(&dos), n(28))?; // ignore checksum
        if base.truthy() {
            let dinfo = Value::new_hash();
            dinfo.set("Parent", pl::s("PS"));
            dinfo.set("RAF", raf.clone());
            dinfo.set("Base", base.clone());
            dinfo.set("NoTiffEnd", n(1)); // no end-of-TIFF check
            put(&buff, pl::method(it, &et, "WriteTIFF", vec![dinfo])?);
            set_byte_order(it, pl::s("II"))?; // (WriteTIFF may change this)
            if get(&buff).truthy() {
                let b = substr(it, &get(&buff), &base, None)?;
                put(&buff, b); // remove header written by WriteTIFF()
            } else {
                // error rewriting TIFF, so just copy over original data
                let l = g32u(it, &rf(&dos), n(24))?;
                if !(seek(it, &raf, base.clone(), 0)? && read_eq(it, &raf, &buff, &l)?) {
                    pl::method(it, &et, "Error", vec![pl::s("Error reading embedded TIFF")])?;
                    return ret(n(1));
                }
                pl::method(it, &et, "Warn", vec![pl::s("Bad embedded TIFF")])?;
            }
            s32u_at(it, n(0), &rf(&dos), n(12))?; // zero metafile pointer
            s32u_at(it, n(0), &rf(&dos), n(16))?; // zero metafile length
            let l = len(&get(&dos));
            s32u_at(it, l, &rf(&dos), n(20))?; // set TIFF pointer
            let l = len(&get(&buff));
            s32u_at(it, l, &rf(&dos), n(24))?; // set TIFF length
        } else if {
            base = g32u(it, &rf(&dos), n(12))?;
            !pl::num_eq(&base, &n(0))
        } {
            // copy over metafile section
            let l = g32u(it, &rf(&dos), n(16))?;
            if !(seek(it, &raf, base.clone(), 0)? && read_eq(it, &raf, &buff, &l)?) {
                pl::method(it, &et, "Error", vec![pl::s("Error reading metafile section")])?;
                return ret(n(1));
            }
            let l = len(&get(&dos));
            s32u_at(it, l, &rf(&dos), n(12))?; // set metafile pointer
        } else {
            put(&buff, pl::s(""));
        }
        ps_new_start = pl::add(&len(&get(&dos)), &len(&get(&buff)));
        s32u_at(it, ps_new_start.clone(), &rf(&dos), n(4))?; // set pointer to start of PS
        if !write(it, &outfile, vec![get(&dos), get(&buff)])? {
            err = n(1);
        }
        seek(it, &raf, pl::add(&ps_start, &n(4)), 0)?; // seek back to where we were
    }
    //
    // rewrite PostScript data
    //
    let sep = ps(it, "GetInputRecordSeparator", vec![raf.clone()])?;
    pl::local_global(it, b'$', "main::/", |it| {
        it.set_scalar("main::/", sep);
        if !(irs(it).truthy() && read_line(it, &raf, &buff)?.truthy()) {
            pl::method(it, &et, "Error", vec![pl::s("Invalid PostScript data")])?;
            return ret(n(1));
        }
        put(&data, pl::concat(&get(&data), &get(&buff)));
        let conforms = match re(r"^%!PS-Adobe-3\.(\d+)\b", "").m(&get(&data)) {
            Some(c) => pl::lt(&cap(&c, 1), &n(2)),
            None => false,
        };
        if !conforms && pl::method(it, &et, "Error", vec![pl::s("Document does not conform to DSC spec. Metadata may be unreadable by other apps"), n(2)])?.truthy() {
            return ret(n(1));
        }
        // (the PS revision number, $psRev, is not used)
        if !write(it, &outfile, vec![get(&data)])? {
            err = n(1);
        }
        if re(r"EPSF", "").is_match(&get(&data)) {
            flags.set("EPS", n(1));
        }

        // get hash of new information keyed by tagID and directories to add/edit
        let new_tags = pl::method(it, &et, "GetNewTagInfoHash", vec![tag_table_ptr.clone()])?;

        // figure out which directories we need to write (PostScript takes priority)
        let ps_map = wlex_hash(it, MODULE, FILE, "psMap");
        pl::method(it, &et, "InitWriteDirs", vec![ps_map, pl::s("PostScript")])?;
        let add_dirs = et.get("ADD_DIRS");
        let edit_dirs = et.get("EDIT_DIRS");
        let done_dir = Value::new_hash();

        // set XMP hint flag (1 for adding, 0 for deleting, undef for no change)
        xmp_hint = Value::Undef;
        if add_dirs.get("XMP").truthy() {
            xmp_hint = n(1);
        }
        if et.get("DEL_GROUP").get("XMP").truthy() {
            xmp_hint = n(0);
        }
        if xmp_hint.truthy() {
            new_tags.set("XMP_HINT", xmp_hint.clone()); // add special tag to newTags list
        }

        loop {
            if lines.is_empty() && !ps(it, "GetNextLine", vec![raf.clone(), lines.clone()])?.truthy() {
                break;
            }
            put(&data, shift(&lines));
            if end_token.truthy() {
                // look for end token
                let mut p = b"^".to_vec();
                p.extend_from_slice(&end_token.to_bytes());
                p.extend_from_slice(br"\s*$");
                if pl::re_dyn(&p, "is")?.is_match(&get(&data)) {
                    end_token = Value::Undef;
                    // found end: process this information
                    if mode.truthy() {
                        if done_dir.get(mode.to_bytes()).truthy() {
                            pl::method(it, &et, "Error", vec![cat(&[pl::s("Multiple "), mode.clone(), pl::s(" directories")]), n(1)])?;
                        }
                        done_dir.set(mode.to_bytes(), n(1));
                        if !write_ps_dir(it, &et, &outfile, &mode, rf(&buff), &flags)? {
                            err = n(1);
                        }
                        // write end token if we wrote the begin token
                        if flags.get("WROTE_BEGIN").truthy() && !write(it, &outfile, vec![get(&data)])? {
                            err = n(1);
                        }
                        put(&buff, Value::Undef);
                    } else if !write(it, &outfile, vec![get(&data)])? {
                        err = n(1);
                    }
                } else {
                    // buffer data in current begin/end block
                    if mode.is_undef() {
                        // pick up XMP in unrecognized blocks for editing in place
                        if re(r"^<\?xpacket begin=.{7,13}W5M0MpCehiHzreSzNTczkc9d", "").is_match(&get(&data)) && edit_dirs.get("XMP").truthy() {
                            put(&buff, get(&data));
                            mode = pl::s("XMP");
                        } else if !write(it, &outfile, vec![get(&data)])? {
                            err = n(1);
                        }
                    } else if mode.eq_str("XMP") {
                        put(&buff, pl::concat(&get(&buff), &get(&data)));
                    } else {
                        // data is ASCII-hex encoded
                        let mut d = get(&data);
                        pl::tr(&mut d, "0-9A-Fa-f", "", "dc"); // remove all but hex characters
                        put(&data, d.clone());
                        let h = pl::pack("H*", &[d])?;
                        put(&buff, pl::concat(&get(&buff), &h)); // translate from hex
                    }
                }
                continue;
            } else if let Some(c) = re(r"^(%{1,2})(Begin)(?!Object:)(.*?)[:\x0d\x0a]", "i").m(&get(&data)) {
                // comments section is over... write any new tags now
                if !new_tags.is_empty() && !write_new_tags_v(it, &et, &outfile, &new_tags)?.truthy() {
                    err = n(1);
                }
                xmp_hint = Value::Undef;
                // the beginning of a data block (can only write XMP and Photoshop)
                if pl::gt(&verbose, &n(1)) {
                    crate::modules::write_exif::print_out(it, &out, cat(&[cap(&c, 2), cap(&c, 3), pl::s("\n")]))?;
                }
                end_token = cat(&[cap(&c, 1), pl::s(if cap(&c, 2).eq_str("begin") { "end" } else { "End" }), cap(&c, 3)]);
                mode = match &*pl::lc(&cap(&c, 3)).to_bytes() {
                    b"_xml_code" => pl::s("XMP"),
                    b"photoshop" => pl::s("Photoshop"),
                    b"iccprofile" => pl::s("ICC_Profile"),
                    _ => Value::Undef,
                };
                if mode.truthy() && edit_dirs.get(mode.to_bytes()).truthy() {
                    put(&buff, pl::s("")); // initialize buffer for this block
                    flags.set("WROTE_BEGIN", n(0));
                } else {
                    mode = Value::Undef; // not editing this directory
                    if !write(it, &outfile, vec![get(&data)])? {
                        err = n(1);
                    }
                    flags.set("WROTE_BEGIN", n(1));
                }
                continue;
            } else if let Some(c) = re(r"^%%(?!Page:|PlateFile:|BeginObject:)(\w+): ?(.*)", "s").m(&get(&data)) {
                // rewrite information from PostScript tags in comments
                let (tag, mut val) = (cap(&c, 1), cap(&c, 2));
                // handle Adobe Illustrator files specially
                // - EVENTUALLY IT WOULD BE BETTER TO FIND ANOTHER IDENTIFICATION METHOD
                //   (because Illustrator doesn't care if the Creator is changed)
                if tag.eq_str("Creator") && re(r"^Adobe Illustrator", "").is_match(&val) {
                    // disable writing XMP to PostScript-format Adobe Illustrator files
                    // because it confuses Illustrator
                    if edit_dirs.get("XMP").truthy() {
                        pl::method(it, &et, "Warn", vec![pl::s("Can't write XMP to PostScript-format Illustrator files")])?;
                        // pretend like we wrote it already so we won't try to add it later
                        done_dir.set("XMP", n(1));
                    }
                    // don't allow "Creator" to be changed in Illustrator files
                    // (we need it to be able to recognize these files)
                    // --> find a better way to do this!
                    if new_tags.get(tag.to_bytes()).truthy() {
                        pl::method(it, &et, "Warn", vec![pl::s("Can't change Postscript:Creator of Illustrator files")])?;
                        new_tags.delete(tag.to_bytes());
                    }
                }
                if new_tags.get(tag.to_bytes()).truthy() {
                    let tag_info = new_tags.get(tag.to_bytes());
                    new_tags.delete(tag.to_bytes()); // write it then forget it
                    if !tag_info.is_ref() {
                        continue;
                    }
                    // decode comment string (reading continuation lines if necessary)
                    val = ps(it, "DecodeComment", vec![val, raf.clone(), lines.clone(), rf(&data)])?;
                    if val.is_array() {
                        let sep = pl::method(it, &et, "Options", vec![pl::s("ListSep")])?;
                        val = join_v(&sep, &val.items());
                    }
                    let nv_hash = pl::method(it, &et, "GetNewValueHash", vec![tag_info.clone()])?;
                    if pl::method(it, &et, "IsOverwriting", vec![nv_hash.clone(), val.clone()])?.truthy() {
                        pl::method(it, &et, "VerboseValue", vec![pl::concat(&pl::s("- PostScript:"), &tag_info.get("Name")), val.clone()])?;
                        val = pl::method(it, &et, "GetNewValue", vec![nv_hash])?;
                        inc(&et, "CHANGED");
                        if val.is_undef() {
                            continue; // next if tag is being deleted
                        }
                        pl::method(it, &et, "VerboseValue", vec![pl::concat(&pl::s("+ PostScript:"), &tag_info.get("Name")), val.clone()])?;
                        let e = encode_tag_v(it, &tag, &val)?;
                        put(&data, e);
                    }
                }
            // (note: Adobe InDesign doesn't put colon after %ADO_ContainsXMP -- doh!)
            } else if let Some(c) = (!xmp_hint.is_undef()).then(|| re(r"^%ADO_ContainsXMP:? ?(.+?)[\x0d\x0a]*$", "s").m(&get(&data))).flatten() {
                // change the XMP hint if necessary
                let nl = irs(it);
                if xmp_hint.truthy() {
                    if cap(&c, 1).eq_str("NoMain") {
                        put(&data, pl::concat(&pl::s("%ADO_ContainsXMP: MainFirst"), &nl));
                    }
                } else {
                    put(&data, pl::concat(&pl::s("%ADO_ContainsXMP: NoMain"), &nl));
                }
                // delete XMP hint flags
                new_tags.delete("XMP_HINT");
                xmp_hint = Value::Undef;
            } else {
                // look for end of comments section
                if !new_tags.is_empty() && (!re(r"^%\S", "").is_match(&get(&data)) || re(r"^%(%EndComments|%Page:|%PlateFile:|%BeginObject:|.*BeginLayer)", "").is_match(&get(&data))) {
                    // write new tags at end of comments section
                    if !write_new_tags_v(it, &et, &outfile, &new_tags)?.truthy() {
                        err = n(1);
                    }
                    xmp_hint = Value::Undef;
                }
                // look for start of drawing commands (AI uses "%AI5_BeginLayer",
                // and Helios uses "%%BeginObject:")
                if re(r"^%(%Page:|%PlateFile:|%BeginObject:|.*BeginLayer)", "").is_match(&get(&data)) || !re(r"^(%.*|\s*)$", "s").is_match(&get(&data)) {
                    // we have reached the first page or drawing command, so create necessary
                    // directories and copy the rest of the file, then all done
                    let plate_file = re(r"^%%PlateFile:", "").is_match(&get(&data));
                    // create Photoshop first, then XMP if necessary
                    for dir in ["Photoshop", "ICC_Profile", "XMP"] {
                        if !(edit_dirs.get(dir).truthy() && !done_dir.get(dir).truthy()) {
                            continue;
                        }
                        if plate_file {
                            // PlateFile comments may contain offsets so we can't edit these files!
                            pl::method(it, &et, "Warn", vec![pl::s("Can only edit PostScript information DCS Plate files")])?;
                            break;
                        }
                        if !(add_dirs.get(dir).truthy() || dir == "XMP") {
                            continue;
                        }
                        flags.set("WROTE_BEGIN", n(0));
                        if !write_ps_dir(it, &et, &outfile, &pl::s(dir), Value::Undef, &flags)? {
                            err = n(1);
                        }
                        done_dir.set(dir, n(1));
                    }
                    // copy rest of file
                    if flags.get("TRAILER").truthy() {
                        // write trailer before %%EOF
                        loop {
                            if !write(it, &outfile, vec![get(&data)])? {
                                err = n(1);
                            }
                            if !lines.is_empty() {
                                put(&data, shift(&lines));
                            } else {
                                if !read_line(it, &raf, &data)?.truthy() {
                                    put(&data, Value::Undef);
                                    break;
                                }
                                if get(&dos).truthy() {
                                    ps(it, "CheckPSEnd", vec![raf.clone(), rf(&data)])?;
                                }
                                let d = get(&data).to_bytes();
                                if let Some((end, _)) = match_from(r"[\x0d\x0a]%%EOF\b", "", &d, 0) {
                                    // split data before "%%EOF"
                                    // (necessary if data contains other newline sequences)
                                    let pos = n(end as i64 - 5);
                                    let tail = substr(it, &get(&data), &pos, None)?;
                                    lines.push(tail);
                                    let head = substr(it, &get(&data), &n(0), Some(&pos))?;
                                    put(&data, head);
                                }
                            }
                            if re(r"^%%EOF\b", "").is_match(&get(&data)) {
                                break;
                            }
                        }
                        if !write(it, &outfile, vec![flags.get("TRAILER")])? {
                            err = n(1);
                        }
                    }
                    // simply copy the rest of the file if any data is left
                    if !get(&data).is_undef() {
                        if !write(it, &outfile, vec![get(&data)])? {
                            err = n(1);
                        }
                        if !lines.is_empty() && !write(it, &outfile, lines.items())? {
                            err = n(1);
                        }
                        while read(it, &raf, &data, n(65536))?.truthy() {
                            if get(&dos).truthy() {
                                ps(it, "CheckPSEnd", vec![raf.clone(), rf(&data)])?;
                            }
                            if !write(it, &outfile, vec![get(&data)])? {
                                err = n(1);
                            }
                        }
                    }
                    break; // all done!
                }
            }
            // write new information or copy existing line
            if !write(it, &outfile, vec![get(&data)])? {
                err = n(1);
            }
        }
        if get(&dos).truthy() && !err.truthy() {
            // must go back and set length of PS section in DOS header (very dumb design)
            if pl::ref_of(&outfile) == "SCALAR" {
                let l = pl::sub(&len(&outfile.deref()), &ps_new_start);
                s32u_at(it, l, &outfile, n(8))?;
            } else {
                let (o, p) = (cell(outfile.clone()), cell(ps_new_start.clone()));
                let ok = perl_eval1(
                    it,
                    "my $pos = tell $outfile; (seek($outfile, 8, 0) and print $outfile Image::ExifTool::Set32u($pos - $psNewStart) and seek($outfile, $pos, 0)) ? 1 : 0",
                    MODULE,
                    &[("outfile", &o), ("psNewStart", &p)],
                )?;
                if !ok.truthy() {
                    pl::method(it, &et, "Error", vec![pl::s("Can't write DOS-style PS files in non-seekable stream")])?;
                    err = n(1);
                }
            }
        }
        // issue warning if we couldn't write any information
        if !err.truthy() {
            let mut not_done: Vec<Value> = Vec::new();
            new_tags.delete("XMP_HINT");
            if !new_tags.is_empty() {
                not_done.push(pl::s("PostScript"));
            }
            for dir in ["Photoshop", "ICC_Profile", "XMP"] {
                if edit_dirs.get(dir).truthy() && !done_dir.get(dir).truthy() && !et.get("DEL_GROUP").get(dir).truthy() {
                    not_done.push(pl::s(dir));
                }
            }
            if !not_done.is_empty() {
                pl::method(it, &et, "Warn", vec![cat(&[pl::s("Couldn't write "), pl::join("/", &not_done), pl::s(" information")])])?;
            }
        }
        if end_token.truthy() {
            pl::method(it, &et, "Error", vec![pl::concat(&pl::s("File missing "), &end_token)])?;
        }
        ret(if err.truthy() { n(-1) } else { n(1) })
    })
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

/// `join $sep, LIST` with the separator held in a value.
fn join_v(sep: &Value, items: &[Value]) -> Value {
    let parts: Vec<Vec<u8>> = items.iter().map(|v| v.to_bytes().to_vec()).collect();
    Value::bytes(parts.join(&sep.to_bytes()[..]))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::PostScript::WriteXMPDir", write_xmp_dir);
    it.define_native("Image::ExifTool::PostScript::WritePSDirectory", write_ps_directory);
    it.define_native("Image::ExifTool::PostScript::EncodeTag", encode_tag);
    it.define_native("Image::ExifTool::PostScript::WriteNewTags", write_new_tags);
    it.define_native("Image::ExifTool::PostScript::WritePS", write_ps);
}
