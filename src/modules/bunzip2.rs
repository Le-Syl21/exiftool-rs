//! IO::Uncompress::Bunzip2::bunzip2, as Rawzor.pm calls it, on bzip2's
//! reference code (the bzip2 crate's port of libbzip2).

use bzip2::{Decompress, Status};

use crate::interp::Interp;
use crate::modules::zlib::{input_bytes, output};
use crate::perl::Value;
use crate::pl::arg;

const ERR_VAR: &str = "IO::Uncompress::Bunzip2::Bunzip2Error";

/// One bzip2 stream from `input`, with IO::Uncompress::Base's defaults:
/// Transparent (data that does not start with "BZh" is passed through)
/// and no MultiStream (data after the first stream is ignored). On error,
/// the message perl's module gives.
fn bunzip2(input: &[u8]) -> Result<Vec<u8>, &'static str> {
    if input.is_empty() || !input.starts_with(b"BZh") {
        return Ok(input.to_vec());
    }
    let mut d = Decompress::new(false);
    let mut out = Vec::with_capacity(input.len() * 4 + 64);
    loop {
        let consumed = d.total_in() as usize;
        if out.capacity() - out.len() < 4096 {
            out.reserve(out.capacity().max(4096));
        }
        let before = (d.total_in(), d.total_out());
        match d.decompress_vec(&input[consumed..], &mut out) {
            Ok(Status::StreamEnd) => return Ok(out),
            Ok(_) => {
                if (d.total_in(), d.total_out()) == before && d.total_in() as usize >= input.len() {
                    return Err("unexpected end of file");
                }
            }
            Err(e) => {
                let magic = format!("{e:?}").to_lowercase().contains("magic");
                return Err(if magic { "Inflation Error: Magic Error" } else { "Inflation Error: Data Error" });
            }
        }
    }
}

pub fn register(it: &mut Interp) {
    it.set_scalar("IO::Uncompress::Bunzip2::VERSION", Value::str("2.204"));
    it.define_native("IO::Uncompress::Bunzip2::bunzip2", |it, a, _| {
        let Some(input) = input_bytes(&arg(&a, 0)) else {
            it.set_scalar(ERR_VAR, Value::str("input file does not exist"));
            return Ok(vec![Value::from(false)]);
        };
        match bunzip2(&input) {
            Ok(out) => {
                let ok = output(it, &arg(&a, 1), out);
                it.set_scalar(ERR_VAR, Value::str(""));
                Ok(vec![Value::from(ok)])
            }
            Err(e) => {
                // perl leaves what its read loop had produced: nothing for
                // a bad header, the blocks read so far for a stream cut
                // short (Rawzor ignores the output when the call fails)
                output(it, &arg(&a, 1), Vec::new());
                it.set_scalar(ERR_VAR, Value::str(e));
                Ok(vec![Value::Undef])
            }
        }
    });
}
