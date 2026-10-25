//! Compress::Zlib, IO::Uncompress::{RawInflate,Gunzip} and
//! IO::Compress::RawDeflate, as ExifTool calls them, on zlib itself.

use std::cell::RefCell;
use std::rc::Rc;

use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::Value;

const Z_OK: i64 = 0;
const Z_STREAM_END: i64 = 1;
const Z_DATA_ERROR: i64 = -3;

enum Stream {
    Inflate(Decompress),
    Deflate(Compress),
}

thread_local! {
    static STREAMS: RefCell<Vec<Option<Stream>>> = const { RefCell::new(Vec::new()) };
}

/// A stream object: a blessed hash holding the index of its zlib stream.
fn new_object(class: &str, s: Stream) -> Value {
    let id = STREAMS.with(|v| {
        let mut v = v.borrow_mut();
        v.push(Some(s));
        v.len() - 1
    });
    let h = Value::new_hash();
    if let Value::HashRef(r) = &h {
        r.borrow_mut().blessed = Some(Rc::from(class));
    }
    h.set("id", Value::Int(id as i64));
    h
}

fn with_stream<T>(obj: &Value, f: impl FnOnce(&mut Stream) -> T) -> Option<T> {
    let id = obj.get("id").to_i64() as usize;
    STREAMS.with(|v| v.borrow_mut().get_mut(id).and_then(|s| s.as_mut()).map(f))
}

/// Option `-WindowBits => -MAX_WBITS` asks for a raw stream.
fn raw_window(args: &[Value]) -> bool {
    let mut i = 0;
    while i + 1 < args.len() {
        if args[i].to_text().trim_start_matches('-') == "WindowBits" && args[i + 1].to_i64() < 0 {
            return true;
        }
        i += 2;
    }
    false
}

fn inflate_all(d: &mut Decompress, input: &[u8]) -> (Vec<u8>, i64) {
    let mut out = Vec::with_capacity(input.len() * 4 + 64);
    let start_in = d.total_in();
    loop {
        let consumed = (d.total_in() - start_in) as usize;
        if out.capacity() - out.len() < 4096 {
            out.reserve(out.capacity().max(4096));
        }
        match d.decompress_vec(&input[consumed..], &mut out, FlushDecompress::None) {
            Ok(Status::StreamEnd) => return (out, Z_STREAM_END),
            Ok(_) => {
                let now = (d.total_in() - start_in) as usize;
                if now >= input.len() && out.capacity() > out.len() {
                    return (out, Z_OK);
                }
            }
            Err(_) => return (out, Z_DATA_ERROR),
        }
    }
}

fn deflate_some(c: &mut Compress, input: &[u8], flush: FlushCompress) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() + 64);
    let start_in = c.total_in();
    loop {
        let consumed = (c.total_in() - start_in) as usize;
        if out.capacity() - out.len() < 1024 {
            out.reserve(out.capacity().max(4096));
        }
        let before = out.len();
        let st = c.compress_vec(&input[consumed..], &mut out, flush).unwrap_or(Status::StreamEnd);
        let done_input = (c.total_in() - start_in) as usize >= input.len();
        match flush {
            FlushCompress::Finish if st == Status::StreamEnd => return out,
            FlushCompress::Finish => {}
            _ if done_input && out.len() < out.capacity() => return out,
            _ if done_input && out.len() == before => return out,
            _ => {}
        }
    }
}

fn arg(a: &[Value], i: usize) -> Value {
    a.get(i).cloned().unwrap_or_default()
}

/// The data of an IO::(Un)Compress input: a scalar ref is the data, a
/// plain string a file name.
fn input_bytes(v: &Value) -> Option<Vec<u8>> {
    if v.is_scalar_ref() {
        return Some(v.deref().to_bytes().to_vec());
    }
    std::fs::read(v.to_text()).ok()
}

/// Store an IO::(Un)Compress output: into a scalar ref or a file.
fn output(it: &mut Interp, target: &Value, data: Vec<u8>) -> bool {
    match target {
        Value::ScalarRef(c) => {
            *c.borrow_mut() = Value::bytes(data);
            true
        }
        Value::LvRef(l) => l.set(Value::bytes(data)).is_ok(),
        v => {
            let _ = it;
            std::fs::write(v.to_text(), data).is_ok()
        }
    }
}

fn io_call(it: &mut Interp, a: Vec<Value>, err_var: &str, f: impl Fn(&[u8]) -> Option<Vec<u8>>) -> ER<Vec<Value>> {
    let Some(input) = input_bytes(&arg(&a, 0)) else {
        it.set_scalar(err_var, Value::str("input file does not exist"));
        return Ok(vec![Value::from(false)]);
    };
    match f(&input) {
        Some(out) => {
            let ok = output(it, &arg(&a, 1), out);
            it.set_scalar(err_var, Value::str(""));
            Ok(vec![Value::from(ok)])
        }
        None => {
            it.set_scalar(err_var, Value::str("inflate error"));
            Ok(vec![Value::from(false)])
        }
    }
}

pub fn register(it: &mut Interp) {
    it.define_native("Compress::Zlib::Z_OK", |_, _, _| Ok(vec![Value::Int(Z_OK)]));
    it.define_native("Compress::Zlib::Z_STREAM_END", |_, _, _| Ok(vec![Value::Int(Z_STREAM_END)]));
    it.define_native("Compress::Zlib::MAX_WBITS", |_, _, _| Ok(vec![Value::Int(15)]));
    it.define_native("Compress::Zlib::inflateInit", |_, a, want| {
        let obj = new_object("Compress::Zlib::inflateStream", Stream::Inflate(Decompress::new(!raw_window(&a))));
        Ok(if want == Want::List { vec![obj, Value::Int(Z_OK)] } else { vec![obj] })
    });
    it.define_native("Compress::Zlib::deflateInit", |_, a, want| {
        let obj = new_object("Compress::Zlib::deflateStream", Stream::Deflate(Compress::new(Compression::new(6), !raw_window(&a))));
        Ok(if want == Want::List { vec![obj, Value::Int(Z_OK)] } else { vec![obj] })
    });
    it.define_native("Compress::Zlib::inflateStream::inflate", |_, a, want| {
        let input = arg(&a, 1).to_bytes();
        let (out, st) = with_stream(&arg(&a, 0), |s| match s {
            Stream::Inflate(d) => inflate_all(d, &input),
            Stream::Deflate(_) => (Vec::new(), Z_DATA_ERROR),
        })
        .unwrap_or((Vec::new(), Z_DATA_ERROR));
        let v = if st == Z_DATA_ERROR { Value::Undef } else { Value::bytes(out) };
        Ok(if want == Want::List { vec![v, Value::Int(st)] } else { vec![v] })
    });
    it.define_native("Compress::Zlib::deflateStream::deflate", |_, a, want| {
        let input = arg(&a, 1).to_bytes();
        let out = with_stream(&arg(&a, 0), |s| match s {
            Stream::Deflate(c) => deflate_some(c, &input, FlushCompress::None),
            Stream::Inflate(_) => Vec::new(),
        })
        .unwrap_or_default();
        Ok(if want == Want::List { vec![Value::bytes(out), Value::Int(Z_OK)] } else { vec![Value::bytes(out)] })
    });
    it.define_native("Compress::Zlib::deflateStream::flush", |_, a, want| {
        let out = with_stream(&arg(&a, 0), |s| match s {
            Stream::Deflate(c) => deflate_some(c, &[], FlushCompress::Finish),
            Stream::Inflate(_) => Vec::new(),
        })
        .unwrap_or_default();
        Ok(if want == Want::List { vec![Value::bytes(out), Value::Int(Z_OK)] } else { vec![Value::bytes(out)] })
    });
    it.define_native("Compress::Zlib::compress", |_, a, _| {
        let mut c = Compress::new(Compression::new(6), true);
        Ok(vec![Value::bytes(deflate_some(&mut c, &arg(&a, 0).to_bytes(), FlushCompress::Finish))])
    });
    it.define_native("Compress::Zlib::uncompress", |_, a, _| {
        let mut d = Decompress::new(true);
        let (out, st) = inflate_all(&mut d, &arg(&a, 0).to_bytes());
        Ok(vec![if st == Z_STREAM_END { Value::bytes(out) } else { Value::Undef }])
    });
    it.define_native("Compress::Zlib::crc32", |_, a, _| {
        let prev = a.get(1).map(|v| v.to_i64() as u32).unwrap_or(0);
        Ok(vec![Value::Int(crc32_combine_from(prev, &arg(&a, 0).to_bytes()) as i64)])
    });
    it.define_native("IO::Uncompress::RawInflate::rawinflate", |it, a, _| {
        io_call(it, a, "IO::Uncompress::RawInflate::RawInflateError", |inp| {
            let mut d = Decompress::new(false);
            let (out, st) = inflate_all(&mut d, inp);
            (st != Z_DATA_ERROR).then_some(out)
        })
    });
    it.define_native("IO::Uncompress::Gunzip::gunzip", |it, a, _| {
        io_call(it, a, "IO::Uncompress::Gunzip::GunzipError", |inp| {
            use std::io::Read;
            let mut out = Vec::new();
            flate2::read::MultiGzDecoder::new(inp).read_to_end(&mut out).ok().map(|_| out)
        })
    });
    it.define_native("IO::Compress::RawDeflate::rawdeflate", |it, a, _| {
        io_call(it, a, "IO::Compress::RawDeflate::RawDeflateError", |inp| {
            let mut c = Compress::new(Compression::new(6), false);
            Some(deflate_some(&mut c, inp, FlushCompress::Finish))
        })
    });
}

/// crc32 continuing from a previous value (bitwise, as zlib's crc32()).
fn crc32_combine_from(prev: u32, data: &[u8]) -> u32 {
    let mut crc = !prev;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}
