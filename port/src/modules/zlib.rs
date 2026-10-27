//! Compress::Zlib, IO::Uncompress::{RawInflate,Gunzip} and
//! IO::Compress::RawDeflate, as ExifTool calls them, on zlib itself.

use std::cell::RefCell;
use std::rc::Rc;

use flate2::{Decompress, FlushDecompress, Status};

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::perl::Value;

const Z_OK: i64 = 0;
const Z_STREAM_END: i64 = 1;
const Z_DATA_ERROR: i64 = -3;

enum Stream {
    Inflate(Decompress),
    Deflate(ZDeflate),
}

/// A zlib deflate stream made as Compress::Raw::Zlib makes it: memLevel 9
/// (MAX_MEM_LEVEL, its default and Compress::Zlib's) where flate2 uses 8.
/// The memory level sets the size of zlib's symbol buffer, so where blocks
/// end: with 8 the compressed bytes differ from perl's.
struct ZDeflate {
    strm: Box<libz_sys::z_stream>,
}

/// zlib's allocator, on the C runtime's.
extern "C" fn z_alloc(_: libz_sys::voidpf, items: libz_sys::uInt, size: libz_sys::uInt) -> libz_sys::voidpf {
    extern "C" {
        fn calloc(n: usize, size: usize) -> *mut std::ffi::c_void;
    }
    unsafe { calloc(items as usize, size as usize) }
}

extern "C" fn z_free(_: libz_sys::voidpf, p: libz_sys::voidpf) {
    extern "C" {
        fn free(p: *mut std::ffi::c_void);
    }
    unsafe { free(p) }
}

/// What a deflate call did: input bytes taken, output bytes written, zlib's
/// return code.
struct ZStep {
    consumed: usize,
    produced: usize,
    ret: i32,
}

impl ZDeflate {
    /// deflateInit2(level, Z_DEFLATED, ±MAX_WBITS, MAX_MEM_LEVEL, Z_DEFAULT_STRATEGY):
    /// a zlib header and trailer unless `raw`.
    fn new(level: i32, raw: bool) -> Self {
        let mut strm = Box::new(libz_sys::z_stream {
            next_in: std::ptr::null_mut(),
            avail_in: 0,
            total_in: 0,
            next_out: std::ptr::null_mut(),
            avail_out: 0,
            total_out: 0,
            msg: std::ptr::null_mut(),
            state: std::ptr::null_mut(),
            zalloc: z_alloc,
            zfree: z_free,
            opaque: std::ptr::null_mut(),
            data_type: 0,
            adler: 0,
            reserved: 0,
        });
        let bits = if raw { -15 } else { 15 };
        let r = unsafe { libz_sys::deflateInit2_(&mut *strm, level, libz_sys::Z_DEFLATED, bits, 9, libz_sys::Z_DEFAULT_STRATEGY, libz_sys::zlibVersion(), std::mem::size_of::<libz_sys::z_stream>() as i32) };
        assert_eq!(r, libz_sys::Z_OK, "deflateInit2");
        ZDeflate { strm }
    }

    fn deflate(&mut self, input: &[u8], output: &mut [u8], finish: bool) -> ZStep {
        let s = &mut *self.strm;
        s.next_in = input.as_ptr() as *mut u8;
        s.avail_in = input.len() as libz_sys::uInt;
        s.next_out = output.as_mut_ptr();
        s.avail_out = output.len() as libz_sys::uInt;
        let ret = unsafe { libz_sys::deflate(s, if finish { libz_sys::Z_FINISH } else { libz_sys::Z_NO_FLUSH }) };
        let consumed = input.len() - s.avail_in as usize;
        let produced = output.len() - s.avail_out as usize;
        s.next_in = std::ptr::null_mut();
        s.next_out = std::ptr::null_mut();
        ZStep { consumed, produced, ret }
    }
}

impl Drop for ZDeflate {
    fn drop(&mut self) {
        unsafe { libz_sys::deflateEnd(&mut *self.strm) };
    }
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

/// IO::Uncompress::RawInflate's rawinflate, with its default Transparent
/// mode. Its magic check reads the first block (BlockSize, 16 KiB) into a
/// string and makes one inflate call with LimitOutput into that same string
/// once emptied: so at most its allocated length less one byte of output,
/// raised to Bufsize (4 KiB) plus perl's rounding. If that call fails, or
/// it took the whole input without reaching the end of the stream, the
/// input is passed through as it is. Past that check, a stream cut short
/// gives what was inflated and an error. Data after the end of the stream
/// is ignored.
fn raw_inflate(input: &[u8]) -> (Vec<u8>, Option<&'static str>) {
    const BLOCK: usize = 16 * 1024;
    let first = &input[..input.len().min(BLOCK)];
    let eof = input.len() <= BLOCK;
    // The output room is that of `$buffer`, a lexical of _isRawx, after
    // `substr($buffer, 0) = <first block>` (sv_grow rounds to 8 bytes),
    // raised to Bufsize by the XS inflate, less one byte. Perl keeps that
    // allocation from one call to the next, so in a process that inflated
    // something before, the room (and with it the transparent/error
    // outcome on a truncated stream) depends on the earlier calls: an
    // accident of perl's memory reuse, not of the data. This follows a
    // perl that has not inflated anything yet.
    let mut len = ((first.len() + 2).max(STRLEN_NEW_MIN) + 7) & !7;
    if len - 1 < BUFSIZE {
        len = (BUFSIZE + 2 + 7) & !7;
    }
    let room = len - 1;
    let mut d = Decompress::new(false);
    let mut probe = vec![0u8; room];
    match d.decompress(first, &mut probe, FlushDecompress::Sync) {
        Err(_) => return (input.to_vec(), None),
        Ok(Status::StreamEnd) => {}
        Ok(_) => {
            if eof && d.total_in() as usize >= input.len() {
                return (input.to_vec(), None);
            }
        }
    }
    let (out, st) = inflate_all(&mut Decompress::new(false), input);
    match st {
        Z_STREAM_END => (out, None),
        Z_DATA_ERROR => (out, Some("Inflation Error: data error")),
        _ => (out, Some("unexpected end of file")),
    }
}

/// A perl string used as Compress::Raw::Zlib's output buffer: its bytes and
/// its allocated length (SvLEN). The XS code gives zlib the free space of
/// the buffer and grows it by Bufsize, doubling at each step, and zlib cuts
/// stored blocks to the space it is given: so the compressed bytes of
/// incompressible data depend on these sizes, which are reproduced here.
struct PerlBuf {
    data: Vec<u8>,
    len: usize,
}

/// PERL_STRLEN_NEW_MIN on a 64-bit perl.
const STRLEN_NEW_MIN: usize = 16;
/// Compress::Raw::Zlib's default Bufsize.
const BUFSIZE: usize = 4096;

impl PerlBuf {
    /// `my $out = ''` (or undef) once the XS code has forced it to a string:
    /// sv_grow(sv, 1) on an unallocated scalar.
    fn fresh() -> Self {
        let mut b = PerlBuf { data: Vec::new(), len: 0 };
        b.grow(0, 1);
        b
    }

    /// sv_grow(sv, newlen) with SvCUR(sv) == cur (perl 5.38, no perl malloc).
    fn grow(&mut self, cur: usize, newlen: usize) {
        let mut newlen = newlen + 1; // the COW byte
        if newlen > self.len {
            let minlen = cur + (cur >> 2) + STRLEN_NEW_MIN;
            if newlen < minlen {
                newlen = minlen;
            }
            if self.len != 0 {
                newlen = (newlen + 7) & !7;
            }
            self.len = newlen;
        }
    }
}

/// Compress::Raw::Zlib's deflate (finish false) or flush(Z_FINISH) (finish
/// true), appending to `out` as with AppendOutput.
fn xs_deflate(c: &mut ZDeflate, out: &mut PerlBuf, input: &[u8], finish: bool) -> i32 {
    let prefix = out.data.len();
    let mut avail = out.len - prefix;
    let mut bufinc = BUFSIZE;
    let mut pos = 0;
    let mut ret = libz_sys::Z_OK;
    loop {
        if !finish && pos >= input.len() {
            break;
        }
        if avail == 0 {
            out.grow(prefix, out.len + bufinc);
            avail = bufinc;
            bufinc *= 2;
        }
        let mut tmp = vec![0u8; avail];
        let step = c.deflate(&input[pos..], &mut tmp, finish);
        pos += step.consumed;
        out.data.extend_from_slice(&tmp[..step.produced]);
        let available_before = avail;
        avail -= step.produced;
        ret = step.ret;
        // the second of two consecutive flushes
        if finish && available_before == avail && ret == libz_sys::Z_BUF_ERROR {
            ret = libz_sys::Z_OK;
        }
        if ret != libz_sys::Z_OK || (finish && avail != 0) {
            break;
        }
    }
    if finish && ret == libz_sys::Z_STREAM_END {
        libz_sys::Z_OK
    } else {
        ret
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
        let obj = new_object("Compress::Zlib::deflateStream", Stream::Deflate(ZDeflate::new(6, raw_window(&a))));
        Ok(if want == Want::List { vec![obj, Value::Int(Z_OK)] } else { vec![obj] })
    });
    it.define_native("Compress::Zlib::inflateStream::inflate", |it, a, want| {
        let input = arg(&a, 1).to_bytes();
        let (out, st, used) = with_stream(&arg(&a, 0), |s| match s {
            Stream::Inflate(d) => {
                let before = d.total_in();
                let (o, st) = inflate_all(d, &input);
                (o, st, (d.total_in() - before) as usize)
            }
            Stream::Deflate(_) => (Vec::new(), Z_DATA_ERROR, 0),
        })
        .unwrap_or((Vec::new(), Z_DATA_ERROR, 0));
        // Like Compress::Zlib, the input argument keeps what was not consumed.
        it.set_arg(1, Value::bytes(&input[used.min(input.len())..]))?;
        let v = if st == Z_DATA_ERROR { Value::Undef } else { Value::bytes(out) };
        Ok(if want == Want::List { vec![v, Value::Int(st)] } else { vec![v] })
    });
    it.define_native("Compress::Zlib::deflateStream::deflate", |_, a, want| {
        let input = arg(&a, 1).to_bytes();
        let out = with_stream(&arg(&a, 0), |s| match s {
            Stream::Deflate(c) => {
                let mut out = PerlBuf::fresh();
                xs_deflate(c, &mut out, &input, false);
                out.data
            }
            Stream::Inflate(_) => Vec::new(),
        })
        .unwrap_or_default();
        Ok(if want == Want::List { vec![Value::bytes(out), Value::Int(Z_OK)] } else { vec![Value::bytes(out)] })
    });
    it.define_native("Compress::Zlib::deflateStream::flush", |_, a, want| {
        let out = with_stream(&arg(&a, 0), |s| match s {
            Stream::Deflate(c) => {
                let mut out = PerlBuf::fresh();
                xs_deflate(c, &mut out, &[], true);
                out.data
            }
            Stream::Inflate(_) => Vec::new(),
        })
        .unwrap_or_default();
        Ok(if want == Want::List { vec![Value::bytes(out), Value::Int(Z_OK)] } else { vec![Value::bytes(out)] })
    });
    it.define_native("Compress::Zlib::compress", |_, a, _| {
        // one output string for the deflate and the flush, as Compress::Zlib::compress
        let mut c = ZDeflate::new(6, false);
        let mut out = PerlBuf::fresh();
        xs_deflate(&mut c, &mut out, &arg(&a, 0).to_bytes(), false);
        xs_deflate(&mut c, &mut out, &[], true);
        Ok(vec![Value::bytes(out.data)])
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
        let err_var = "IO::Uncompress::RawInflate::RawInflateError";
        let Some(input) = input_bytes(&arg(&a, 0)) else {
            it.set_scalar(err_var, Value::str("input file does not exist"));
            return Ok(vec![Value::from(false)]);
        };
        let (out, err) = raw_inflate(&input);
        let ok = output(it, &arg(&a, 1), out);
        it.set_scalar(err_var, Value::str(err.unwrap_or("")));
        Ok(vec![if err.is_some() { Value::Undef } else { Value::from(ok) }])
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
            // IO::Compress::Base: one syswrite of the data into a fresh
            // buffer, then the close into another
            let mut c = ZDeflate::new(6, true);
            let (mut body, mut trailer) = (PerlBuf::fresh(), PerlBuf::fresh());
            xs_deflate(&mut c, &mut body, inp, false);
            xs_deflate(&mut c, &mut trailer, &[], true);
            body.data.extend_from_slice(&trailer.data);
            Some(body.data)
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
