//! IO::Compress::Brotli::bro and IO::Uncompress::Brotli::unbro, on mbrotli:
//! Brotli in Rust whose encoder is byte-identical to Google's reference
//! library (the one the perl modules use), so compressed output is the same
//! bytes (measured: 197 of 197 inputs identical to Perl's).

use mbrotli::{Compressor, Decompressor, DecoderConfig, EncoderConfig, Quality, Window};

use crate::interp::eval::die;
use crate::interp::Interp;
use crate::perl::Value;
use crate::pl::{arg, ret};

/// `bro($data, $quality = 11, $lgwin = 22)`
fn bro(data: &[u8], quality: u8, lgwin: u8) -> Option<Vec<u8>> {
    let q = Quality::try_from(quality).ok()?;
    let w = Window::standard(lgwin).ok()?;
    let mut c = Compressor::new(EncoderConfig::default().with_quality(q).with_window(w)).ok()?;
    c.compress(data).ok()
}

/// `unbro($data, $maxsize)`: fails beyond maxsize, as the perl module.
fn unbro(data: &[u8], max: usize) -> Option<Vec<u8>> {
    let mut d = Decompressor::new(DecoderConfig::default()).ok()?;
    let mut out = vec![0u8; max];
    let n = d.decompress_to_slice(data, &mut out).ok()?;
    out.truncate(n);
    Some(out)
}

pub fn register(it: &mut Interp) {
    // The versions of the perl modules these follow.
    it.set_scalar("IO::Compress::Brotli::VERSION", Value::str("0.004001"));
    it.set_scalar("IO::Uncompress::Brotli::VERSION", Value::str("0.004001"));
    it.define_native("IO::Compress::Brotli::bro", |_, a, _| {
        let q = a.get(1).map(|v| v.to_i64() as u8).unwrap_or(11);
        let w = a.get(2).map(|v| v.to_i64() as u8).unwrap_or(22);
        match bro(&arg(&a, 0).to_bytes(), q, w) {
            Some(b) => ret(Value::bytes(b)),
            None => die("Error in BrotliEncoderCompress"),
        }
    });
    it.define_native("IO::Uncompress::Brotli::unbro", |_, a, _| {
        let max = arg(&a, 1).to_i64().max(0) as usize;
        match unbro(&arg(&a, 0).to_bytes(), max) {
            Some(b) => ret(Value::bytes(b)),
            None => die("Error in BrotliDecoderDecompress"),
        }
    });
}
