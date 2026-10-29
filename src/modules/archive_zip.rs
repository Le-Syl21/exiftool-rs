//! Archive::Zip, the part ExifTool's ZIP.pm and iWork.pm use: reading an
//! archive's central directory, its members' attributes and contents.
//!
//! This follows Archive::Zip 1.68 (Archive.pm, Member.pm, ZipFileMember.pm):
//! the end-of-central-directory search, the offset correction for data in
//! front of the archive, member order, and the status codes (AZ_OK 0,
//! AZ_ERROR 2, AZ_FORMAT_ERROR 3, AZ_IO_ERROR 4).

use std::rc::Rc;

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::Value;
use crate::pl::{arg, ret};

const AZ_OK: i64 = 0;
const AZ_ERROR: i64 = 2;
const AZ_FORMAT_ERROR: i64 = 3;
const AZ_IO_ERROR: i64 = 4;
/// $Archive::Zip::ChunkSize: how far from the end the EOCD is searched.
const CHUNK_SIZE: usize = 32768;
const EOCD_LENGTH: usize = 18;

fn u16le(b: &[u8], p: usize) -> u64 {
    b.get(p..p + 2).map(|x| u16::from_le_bytes([x[0], x[1]]) as u64).unwrap_or(0)
}

fn u32le(b: &[u8], p: usize) -> u64 {
    b.get(p..p + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]) as u64).unwrap_or(0)
}

fn blessed(class: &str) -> Value {
    let h = Value::new_hash();
    if let Value::HashRef(r) = &h {
        r.borrow_mut().blessed = Some(Rc::from(class));
    }
    h
}

/// The bytes behind a filehandle: its file, or its in-memory data.
fn handle_bytes(it: &mut Interp, fh: &Value) -> Option<Vec<u8>> {
    it.handle_contents(fh)
}

/// readFromFileHandle: the archive's members from its central directory.
fn read_from_file_handle(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let zip = arg(&a, 0);
    let Some(data) = handle_bytes(it, &arg(&a, 1)) else { return ret(Value::Int(AZ_IO_ERROR)) };
    let len = data.len();
    // _findEndOfCentralDirectory
    if len < EOCD_LENGTH + 4 {
        return ret(Value::Int(AZ_FORMAT_ERROR));
    }
    let tail = len.min(CHUNK_SIZE.max(512)).min(len);
    let start = len - tail;
    let Some(rel) = data[start..].windows(4).rposition(|w| w == b"PK\x05\x06") else {
        return ret(Value::Int(AZ_FORMAT_ERROR));
    };
    let eocd = start + rel;
    // _readEndOfCentralDirectory (the zip64 records are not read: ExifTool's
    // test files and the archives it sees have none)
    let p = eocd + 4;
    if p + EOCD_LENGTH > len {
        return ret(Value::Int(AZ_IO_ERROR));
    }
    let cd_size = u32le(&data, p + 8) as usize;
    let cd_offset = u32le(&data, p + 12) as usize;
    let comment_len = u16le(&data, p + 16) as usize;
    let comment = data.get(p + 18..p + 18 + comment_len).unwrap_or(&[]).to_vec();
    zip.set("zipfileComment", Value::bytes(comment));
    let Some(cd_start) = eocd.checked_sub(cd_size) else { return ret(Value::Int(AZ_IO_ERROR)) };
    // garbage at the beginning of the archive shifts every offset
    let eocd_offset = cd_start as i64 - cd_offset as i64;
    zip.set("eocdOffset", Value::Int(eocd_offset));
    zip.set("_data", Value::bytes(data.clone()));
    let members = Value::new_array(Vec::new());
    let mut q = cd_start;
    loop {
        let Some(sig) = data.get(q..q + 4) else { return ret(Value::Int(AZ_IO_ERROR)) };
        if sig == b"PK\x05\x06" {
            break;
        }
        if sig != b"PK\x01\x02" {
            return ret(Value::Int(AZ_FORMAT_ERROR));
        }
        let h = q + 4;
        if h + 42 > len {
            return ret(Value::Int(AZ_IO_ERROR));
        }
        let name_len = u16le(&data, h + 24) as usize;
        let extra_len = u16le(&data, h + 26) as usize;
        let com_len = u16le(&data, h + 28) as usize;
        let name = data.get(h + 42..h + 42 + name_len).unwrap_or(&[]).to_vec();
        let com = data.get(h + 42 + name_len + extra_len..h + 42 + name_len + extra_len + com_len).unwrap_or(&[]).to_vec();
        let dir = name.last() == Some(&b'/');
        let m = blessed(if dir { "Archive::Zip::DirectoryMember" } else { "Archive::Zip::ZipFileMember" });
        m.set("versionMadeBy", Value::Int(u16le(&data, h) as i64));
        m.set("versionNeededToExtract", Value::Int(u16le(&data, h + 2) as i64));
        m.set("bitFlag", Value::Int(u16le(&data, h + 4) as i64));
        m.set("compressionMethod", Value::Int(u16le(&data, h + 6) as i64));
        m.set("lastModFileDateTime", Value::Int(u32le(&data, h + 8) as i64));
        m.set("crc32", Value::Int(u32le(&data, h + 12) as i64));
        m.set("compressedSize", Value::Int(u32le(&data, h + 16) as i64));
        m.set("uncompressedSize", Value::Int(u32le(&data, h + 20) as i64));
        m.set("internalFileAttributes", Value::Int(u16le(&data, h + 34) as i64));
        m.set("externalFileAttributes", Value::Int(u32le(&data, h + 36) as i64));
        m.set("localHeaderRelativeOffset", Value::Int(u32le(&data, h + 38) as i64));
        m.set("fileName", Value::bytes(name));
        m.set("fileComment", Value::bytes(com));
        m.set("_zip", zip.clone());
        members.push(m);
        q = h + 42 + name_len + extra_len + com_len;
    }
    zip.set("members", members);
    ret(Value::Int(AZ_OK))
}

/// A member's contents: (data, status).
fn member_contents(m: &Value) -> (Value, i64) {
    let zip = m.get("_zip");
    let data = zip.get("_data").to_bytes();
    let off = m.get("localHeaderRelativeOffset").to_i64() + zip.get("eocdOffset").to_i64();
    if off < 0 {
        return (Value::Undef, AZ_IO_ERROR);
    }
    let off = off as usize;
    if data.get(off..off + 4) != Some(&b"PK\x03\x04"[..]) {
        return (Value::Undef, AZ_FORMAT_ERROR);
    }
    let name_len = u16le(&data, off + 26) as usize;
    let extra_len = u16le(&data, off + 28) as usize;
    let start = off + 30 + name_len + extra_len;
    let size = m.get("compressedSize").to_i64() as usize;
    let Some(raw) = data.get(start..start + size) else { return (Value::Undef, AZ_IO_ERROR) };
    match m.get("compressionMethod").to_i64() {
        0 => (Value::bytes(raw), AZ_OK),
        8 => {
            let mut d = flate2::Decompress::new(false);
            let mut out = Vec::with_capacity(m.get("uncompressedSize").to_i64().max(0) as usize + 64);
            loop {
                let consumed = d.total_in() as usize;
                if out.capacity() - out.len() < 4096 {
                    out.reserve(out.capacity().max(4096));
                }
                match d.decompress_vec(&raw[consumed..], &mut out, flate2::FlushDecompress::Finish) {
                    Ok(flate2::Status::StreamEnd) => return (Value::bytes(out), AZ_OK),
                    Ok(_) if (d.total_in() as usize) >= raw.len() && out.len() < out.capacity() => return (Value::bytes(out), AZ_OK),
                    Ok(_) => {}
                    Err(_) => return (Value::Undef, AZ_ERROR),
                }
            }
        }
        _ => (Value::Undef, AZ_ERROR),
    }
}

fn contents(_it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let zip = arg(&a, 0);
    let mut m = arg(&a, 1);
    if m.is_undef() {
        return Ok(if want == Want::List { vec![Value::Undef, Value::Int(AZ_ERROR)] } else { vec![Value::Undef] });
    }
    if !m.is_ref() {
        m = member_named(&zip, &m.to_bytes());
        if m.is_undef() {
            return Ok(if want == Want::List { vec![Value::Undef, Value::Int(AZ_ERROR)] } else { vec![Value::Undef] });
        }
    }
    let (c, st) = member_contents(&m);
    Ok(if want == Want::List { vec![c, Value::Int(st)] } else { vec![c] })
}

fn member_named(zip: &Value, name: &[u8]) -> Value {
    for m in zip.get("members").items() {
        if &m.get("fileName").to_bytes()[..] == name {
            return m;
        }
    }
    Value::Undef
}

pub fn register(it: &mut Interp) {
    it.set_scalar("Archive::Zip::VERSION", Value::str("1.68"));
    it.define_native("Archive::Zip::new", |_, _, _| {
        let z = blessed("Archive::Zip::Archive");
        z.set("members", Value::new_array(Vec::new()));
        ret(z)
    });
    it.define_native("Archive::Zip::AZ_OK", |_, _, _| ret(Value::Int(AZ_OK)));
    it.define_native("Archive::Zip::Archive::readFromFileHandle", read_from_file_handle);
    it.define_native("Archive::Zip::Archive::zipfileComment", |_, a, _| ret(arg(&a, 0).get("zipfileComment")));
    it.define_native("Archive::Zip::Archive::members", |_, a, want| {
        let m = arg(&a, 0).get("members").items();
        Ok(if want == Want::List { m } else { vec![Value::Int(m.len() as i64)] })
    });
    it.define_native("Archive::Zip::Archive::memberNamed", |_, a, _| ret(member_named(&arg(&a, 0), &arg(&a, 1).to_bytes())));
    it.define_native("Archive::Zip::Archive::membersMatching", |_, a, want| {
        let pat = arg(&a, 1).to_bytes();
        let re = match crate::perl::regex::compile(&pat, "") {
            Ok(r) => r,
            Err(e) => return crate::interp::eval::die(e),
        };
        let m: Vec<Value> = arg(&a, 0).get("members").items().into_iter().filter(|m| re.is_match(&m.get("fileName").to_bytes())).collect();
        Ok(if want == Want::List { m } else { vec![Value::Int(m.len() as i64)] })
    });
    it.define_native("Archive::Zip::Archive::contents", contents);
    for class in ["Archive::Zip::ZipFileMember", "Archive::Zip::DirectoryMember"] {
        for field in ["versionMadeBy", "versionNeededToExtract", "bitFlag", "compressionMethod", "lastModFileDateTime", "crc32", "compressedSize", "uncompressedSize", "internalFileAttributes", "externalFileAttributes", "fileName", "fileComment", "localHeaderRelativeOffset"] {
            it.define_native(&format!("{class}::{field}"), move |_, a, _| ret(arg(&a, 0).get(field)));
        }
        it.define_native(&format!("{class}::contents"), |_, a, want| {
            let (c, st) = member_contents(&arg(&a, 0));
            Ok(if want == Want::List { vec![c, Value::Int(st)] } else { vec![c] })
        });
        it.define_native(&format!("{class}::isDirectory"), move |_, a, _| ret(Value::from(arg(&a, 0).get("fileName").to_bytes().last() == Some(&b'/'))));
    }
}
