//! Run small Perl programs on fuzzed operands in the port's interpreter,
//! through the operations its natives implement: regular expressions
//! (pattern and subject fuzzed), pack/unpack (template fuzzed), sprintf
//! (format fuzzed), tr, substr/index, and utf8/Encode conversions. The
//! programs, and how an input feeds them, are in `interp_programs.rs`.
//!
//! A Perl `die` is fine (each program runs in `eval`); a Rust panic, an
//! abort, a hang or unbounded memory is a bug.

#![no_main]

use libfuzzer_sys::fuzz_target;

include!("../interp_programs.rs");

fuzz_target!(|data: &[u8]| {
    let _ = run_interp_input(data);
});
