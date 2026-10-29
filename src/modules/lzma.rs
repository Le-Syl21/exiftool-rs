//! Compress::Raw::Lzma, the part 7Z.pm uses to read an encoded 7z header:
//! `Lzma::Filter::Lzma1(%opts)` and `Compress::Raw::Lzma::RawDecoder->new(
//! Filter => $filter)`, whose `code($input, $output)` decodes a raw LZMA1
//! stream (no .lzma header, size unknown) as liblzma's raw decoder does.
//!
//! The LZMA1 decoder is the port's own (LZMA as liblzma decodes it, after
//! the LZMA SDK's specification): a raw stream given in pieces, with no end
//! marker and no size, must decode exactly as far as liblzma's does, since
//! 7Z.pm loops until it has the header's size. liblzma normalizes its range
//! coder before each bit and stops at the first byte it lacks, keeping the
//! symbol it was in; both pure-Rust decoders measured stop elsewhere at the
//! end of the input: lzma-rs normalizes after each bit (197 of the 198
//! bytes of a 7z header, and 7Z.pm then waits forever for the last one),
//! lzma-rust2 after a symbol that fills its output, and its streaming
//! decoder keeps a margin of input back. Measured against liblzma (Perl's
//! module) on the headers of the test archives cut at every length tried,
//! split in two calls, zeros and random data: the same output and status.
//!
//! As liblzma, `code` returns LZMA_OK with all that decodes of the input
//! given so far, LZMA_STREAM_END after an end marker, and LZMA_DATA_ERROR
//! (9) for data it rejects (no output, the input left), after which the
//! decoder answers LZMA_PROG_ERROR (11). Each call decodes the input given
//! so far from its start (the headers are small), and returns what it adds.

use std::rc::Rc;

use crate::interp::eval::{die, ER};
use crate::interp::{Interp, Want};
use crate::perl::Value;

const LZMA_OK: i64 = 0;
const LZMA_STREAM_END: i64 = 1;
const LZMA_DATA_ERROR: i64 = 9;
const LZMA_PROG_ERROR: i64 = 11;

/// Why decoding stopped.
#[derive(Debug, PartialEq)]
enum Stop {
    /// The input ends (LZMA_OK: more may come).
    Input,
    /// The end of payload marker (LZMA_STREAM_END).
    End,
    /// Data liblzma rejects (LZMA_DATA_ERROR).
    Error,
}

/// The range decoder, on the whole input.
struct RangeDecoder<'a> {
    data: &'a [u8],
    pos: usize,
    range: u32,
    code: u32,
}

const PROB_INIT: u16 = 1024;

impl RangeDecoder<'_> {
    /// Before each bit, as liblzma: a byte more when the range is low.
    fn normalize(&mut self) -> Result<(), Stop> {
        if self.range < 1 << 24 {
            let b = *self.data.get(self.pos).ok_or(Stop::Input)?;
            self.pos += 1;
            self.range <<= 8;
            self.code = (self.code << 8) | b as u32;
        }
        Ok(())
    }

    fn bit(&mut self, p: &mut u16) -> Result<usize, Stop> {
        self.normalize()?;
        let bound = (self.range >> 11) * *p as u32;
        if self.code < bound {
            self.range = bound;
            *p += (2048 - *p) >> 5;
            Ok(0)
        } else {
            self.range -= bound;
            self.code -= bound;
            *p -= *p >> 5;
            Ok(1)
        }
    }

    fn tree(&mut self, probs: &mut [u16], bits: u32) -> Result<usize, Stop> {
        let mut m = 1;
        for _ in 0..bits {
            m = (m << 1) | self.bit(&mut probs[m])?;
        }
        Ok(m - (1 << bits))
    }

    fn reverse_tree(&mut self, probs: &mut [u16], bits: u32) -> Result<usize, Stop> {
        let (mut m, mut sym) = (1, 0);
        for i in 0..bits {
            let b = self.bit(&mut probs[m])?;
            m = (m << 1) | b;
            sym |= b << i;
        }
        Ok(sym)
    }

    fn direct(&mut self, bits: u32) -> Result<usize, Stop> {
        let mut r = 0;
        for _ in 0..bits {
            self.normalize()?;
            self.range >>= 1;
            let b = if self.code >= self.range {
                self.code -= self.range;
                1
            } else {
                0
            };
            r = (r << 1) | b;
        }
        Ok(r)
    }
}

/// A length decoder (matches, repeated matches).
struct LenDecoder {
    choice: u16,
    choice2: u16,
    low: [[u16; 8]; 16],
    mid: [[u16; 8]; 16],
    high: [u16; 256],
}

impl LenDecoder {
    fn new() -> Self {
        LenDecoder { choice: PROB_INIT, choice2: PROB_INIT, low: [[PROB_INIT; 8]; 16], mid: [[PROB_INIT; 8]; 16], high: [PROB_INIT; 256] }
    }

    /// The length less 2.
    fn decode(&mut self, rc: &mut RangeDecoder, pos_state: usize) -> Result<usize, Stop> {
        if rc.bit(&mut self.choice)? == 0 {
            rc.tree(&mut self.low[pos_state], 3)
        } else if rc.bit(&mut self.choice2)? == 0 {
            Ok(8 + rc.tree(&mut self.mid[pos_state], 3)?)
        } else {
            Ok(16 + rc.tree(&mut self.high, 8)?)
        }
    }
}

/// Decode the raw LZMA1 stream `data` (lc, lp, pb, dictionary size) as far
/// as it goes: the output of the symbols decoded whole, and why it stopped.
fn decode(data: &[u8], lc: u32, lp: u32, pb: u32, dict_size: u32) -> (Vec<u8>, Stop) {
    let mut out = Vec::new();
    // (the range coder's first byte is always 0, as liblzma checks)
    match data.first() {
        None => return (out, Stop::Input),
        Some(&b) if b != 0 => return (out, Stop::Error),
        _ => {}
    }
    if data.len() < 5 {
        return (out, Stop::Input);
    }
    let mut rc = RangeDecoder { data, pos: 5, range: u32::MAX, code: u32::from_be_bytes([data[1], data[2], data[3], data[4]]) };
    let stop = symbols(&mut rc, &mut out, lc, lp, pb, dict_size).unwrap_err();
    (out, stop)
}

/// The symbols of a stream, into `out`, each when decoded whole; ends with
/// why decoding stopped.
fn symbols(rc: &mut RangeDecoder, out: &mut Vec<u8>, lc: u32, lp: u32, pb: u32, dict_size: u32) -> Result<(), Stop> {
    let mut is_match = [[PROB_INIT; 16]; 12];
    let mut is_rep = [PROB_INIT; 12];
    let mut is_rep0 = [PROB_INIT; 12];
    let mut is_rep1 = [PROB_INIT; 12];
    let mut is_rep2 = [PROB_INIT; 12];
    let mut is_rep0_long = [[PROB_INIT; 16]; 12];
    let mut literal = vec![PROB_INIT; 0x300 << (lc + lp)];
    let mut pos_slot = [[PROB_INIT; 64]; 4];
    // (the SDK's SpecPos with one place more in front: it reads index
    // distance - slot - 1 + m, m from 1, which starts at 0 for slot 4)
    let mut pos_special = [PROB_INIT; 1 + 114];
    let mut align = [PROB_INIT; 16];
    let mut len_dec = LenDecoder::new();
    let mut rep_len_dec = LenDecoder::new();
    let (mut state, mut rep) = (0usize, [0usize; 4]);
    let (pb_mask, lp_mask) = ((1usize << pb) - 1, (1usize << lp) - 1);
    loop {
        let pos_state = out.len() & pb_mask;
        if rc.bit(&mut is_match[state][pos_state])? == 0 {
            let prev = out.last().copied().unwrap_or(0) as usize;
            let lit_state = ((out.len() & lp_mask) << lc) + (prev >> (8 - lc));
            let probs = &mut literal[0x300 * lit_state..0x300 * (lit_state + 1)];
            let mut sym = 1usize;
            if state >= 7 {
                let mut match_byte = out[out.len() - rep[0] - 1] as usize;
                while sym < 0x100 {
                    let match_bit = (match_byte >> 7) & 1;
                    match_byte <<= 1;
                    let b = rc.bit(&mut probs[((1 + match_bit) << 8) + sym])?;
                    sym = (sym << 1) | b;
                    if match_bit != b {
                        break;
                    }
                }
            }
            while sym < 0x100 {
                sym = (sym << 1) | rc.bit(&mut probs[sym])?;
            }
            out.push(sym as u8);
            state = if state < 4 { 0 } else if state < 10 { state - 3 } else { state - 6 };
            continue;
        }
        let len;
        if rc.bit(&mut is_rep[state])? == 1 {
            if out.is_empty() {
                return Err(Stop::Error);
            }
            if rc.bit(&mut is_rep0[state])? == 0 {
                if rc.bit(&mut is_rep0_long[state][pos_state])? == 0 {
                    // a short rep: one byte at rep0
                    state = if state < 7 { 9 } else { 11 };
                    out.push(out[out.len() - rep[0] - 1]);
                    continue;
                }
            } else {
                let dist;
                if rc.bit(&mut is_rep1[state])? == 0 {
                    dist = rep[1];
                } else {
                    if rc.bit(&mut is_rep2[state])? == 0 {
                        dist = rep[2];
                    } else {
                        dist = rep[3];
                        rep[3] = rep[2];
                    }
                    rep[2] = rep[1];
                }
                rep[1] = rep[0];
                rep[0] = dist;
            }
            len = rep_len_dec.decode(rc, pos_state)?;
            state = if state < 7 { 8 } else { 11 };
        } else {
            len = len_dec.decode(rc, pos_state)?;
            let slot = rc.tree(&mut pos_slot[len.min(3)], 6)?;
            let dist = if slot < 4 {
                slot
            } else {
                let direct = (slot >> 1) as u32 - 1;
                let base = (2 | (slot & 1)) << direct;
                if slot < 14 {
                    base + rc.reverse_tree(&mut pos_special[base - slot..], direct)?
                } else {
                    base + (rc.direct(direct - 4)? << 4) + rc.reverse_tree(&mut align, 4)?
                }
            };
            rep = [dist, rep[0], rep[1], rep[2]];
            state = if state < 7 { 7 } else { 10 };
            if dist == u32::MAX as usize {
                return Err(Stop::End);
            }
        }
        // the match: len + 2 bytes from rep0 + 1 back, within what is decoded
        // and the dictionary
        if rep[0] >= out.len().min(dict_size as usize) {
            return Err(Stop::Error);
        }
        let from = out.len() - rep[0] - 1;
        for i in 0..len + 2 {
            out.push(out[from + i]);
        }
    }
}

fn arg(a: &[Value], i: usize) -> Value {
    a.get(i).cloned().unwrap_or_default()
}

/// The `Key => value` pairs of a call, from `start`, without a leading `-`
/// on the keys (as Compress::Raw::Lzma's ParseParameters accepts either).
fn pairs(a: &[Value], start: usize) -> Vec<(String, Value)> {
    a.get(start..)
        .unwrap_or(&[])
        .chunks(2)
        .map(|p| (p[0].to_text().trim_start_matches('-').to_string(), p.get(1).cloned().unwrap_or_default()))
        .collect()
}

fn blessed_hash(class: &str) -> Value {
    let h = Value::new_hash();
    if let Value::HashRef(r) = &h {
        r.borrow_mut().blessed = Some(Rc::from(class));
    }
    h
}

/// `Lzma::Filter::Lzma1(%opts)`: the filter's options (DictSize, Lc, Lp, Pb),
/// with liblzma's defaults (8 MiB, lc 3, lp 0, pb 2), checked as the perl
/// module checks them.
fn filter_lzma1(a: &[Value]) -> ER<Value> {
    let (mut dict, mut lc, mut lp, mut pb) = (1i64 << 23, 3i64, 0i64, 2i64);
    for (k, v) in pairs(a, 0) {
        match k.as_str() {
            "DictSize" => dict = v.to_i64(),
            "Lc" => lc = v.to_i64(),
            "Lp" => lp = v.to_i64(),
            "Pb" => pb = v.to_i64(),
            _ => {}
        }
    }
    if !(4096..=1536 << 20).contains(&dict) {
        return die(format!("Dictsize {dict} not in range 4KiB - 1536Mib"));
    }
    for (name, v) in [("Lc", lc), ("Lp", lp)] {
        if !(0..=4).contains(&v) {
            return die(format!("{name} {v} not in range 0-4"));
        }
    }
    if lc + lp > 4 {
        return die("Lc + Lp must be <= 4");
    }
    if !(0..=4).contains(&pb) {
        return die(format!("Pb {pb} not in range 0-4"));
    }
    let f = blessed_hash("Lzma::Filter::Lzma1");
    for (k, v) in [("DictSize", dict), ("Lc", lc), ("Lp", lp), ("Pb", pb)] {
        f.set(k, Value::Int(v));
    }
    Ok(f)
}

pub fn register(it: &mut Interp) {
    // The version of the perl module this follows.
    it.set_scalar("Compress::Raw::Lzma::VERSION", Value::str("2.209"));
    for (name, v) in [("LZMA_OK", LZMA_OK), ("LZMA_STREAM_END", LZMA_STREAM_END), ("LZMA_DATA_ERROR", LZMA_DATA_ERROR), ("LZMA_PROG_ERROR", LZMA_PROG_ERROR)] {
        it.define_native(&format!("Compress::Raw::Lzma::{name}"), move |_, _, _| Ok(vec![Value::Int(v)]));
    }
    it.define_native("Lzma::Filter::Lzma1", |_, a, _| Ok(vec![filter_lzma1(&a)?]));
    // RawDecoder->new(Filter => $filter or [$filter], AppendOutput => 0,
    // ConsumeInput => 1): ($decoder, $status) in list context
    it.define_native("Compress::Raw::Lzma::RawDecoder::new", |_, a, want| {
        let mut filter = None;
        let (mut append, mut consume) = (false, true);
        for (k, v) in pairs(&a, 1) {
            match k.as_str() {
                "Filter" => filter = Some(if v.is_array() { v.at(0) } else { v }),
                "AppendOutput" => append = v.truthy(),
                "ConsumeInput" => consume = v.truthy(),
                _ => {}
            }
        }
        let filter = match filter {
            Some(f) => f,
            None => filter_lzma1(&[])?,
        };
        let d = blessed_hash("Compress::Raw::Lzma::Decoder");
        for k in ["Lc", "Lp", "Pb", "DictSize"] {
            d.set(k, filter.get(k));
        }
        d.set("AppendOutput", Value::from(append));
        d.set("ConsumeInput", Value::from(consume));
        d.set("in", Value::bytes(Vec::new()));
        d.set("done", Value::Int(0));
        d.set("ended", Value::Int(0));
        Ok(if want == Want::List { vec![d, Value::Int(LZMA_OK)] } else { vec![d] })
    });
    // $decoder->code($input, $output)
    it.define_native("Compress::Raw::Lzma::Decoder::code", |it, a, _| {
        let d = arg(&a, 0);
        let input = arg(&a, 1).to_bytes();
        let (out, status, consumed) = match d.get("ended").to_i64() {
            LZMA_STREAM_END => (Vec::new(), LZMA_STREAM_END, false),
            0 => {
                let mut all = d.get("in").to_bytes().to_vec();
                all.extend_from_slice(&input);
                let p = |k: &str| d.get(k).to_i64() as u32;
                let (o, stop) = decode(&all, p("Lc"), p("Lp"), p("Pb"), p("DictSize"));
                let done = d.get("done").to_i64() as usize;
                let out = o.get(done.min(o.len())..).unwrap_or(&[]).to_vec();
                d.set("done", Value::Int((done + out.len()) as i64));
                d.set("in", Value::bytes(all));
                let status = match stop {
                    Stop::Input => LZMA_OK,
                    Stop::End => LZMA_STREAM_END,
                    Stop::Error => LZMA_DATA_ERROR,
                };
                if status != LZMA_OK {
                    d.set("ended", Value::Int(status));
                }
                // (an error leaves no output, and the input as it was)
                if status == LZMA_DATA_ERROR {
                    (Vec::new(), status, false)
                } else {
                    (out, status, true)
                }
            }
            _ => (Vec::new(), LZMA_PROG_ERROR, false),
        };
        // (liblzma takes all the input it is given)
        if consumed && d.get("ConsumeInput").truthy() {
            it.set_arg(1, Value::bytes(Vec::new()))?;
        }
        let out = if d.get("AppendOutput").truthy() {
            let mut o = arg(&a, 2).to_bytes().to_vec();
            o.extend_from_slice(&out);
            o
        } else {
            out
        };
        it.set_arg(2, Value::bytes(out))?;
        Ok(vec![Value::Int(status)])
    });
}

#[cfg(test)]
mod tests {
    use super::{decode, Stop};

    /// Streams as liblzma (Compress::Raw::Lzma 2.209, xz 5.4.5) decodes them.
    #[test]
    fn short_streams() {
        // a stream of 10 zeros: 5 bytes of output with liblzma
        assert_eq!(decode(&[0; 10], 3, 0, 2, 1 << 23), (vec![0; 5], Stop::Input));
        assert_eq!(decode(&[0; 6], 3, 0, 2, 1 << 23).0.len(), 1);
        assert_eq!(decode(&[0; 5], 3, 0, 2, 1 << 23).0.len(), 0);
        assert_eq!(decode(&[1], 3, 0, 2, 1 << 23), (vec![], Stop::Error));
        assert_eq!(decode(&[], 3, 0, 2, 1 << 23), (vec![], Stop::Input));
    }
}
