//! Google HDRP MakerNote decoder.
//!
//! Decodes the Google HDR+ protobuf maker note stored in XMP GCamera:HdrPlusMakernote.
//! The encoding is: base64 → HDRP header stripped → XOR decrypt → gunzip → protobuf.
//!
//! References:
//! - ExifTool Google.pm ProcessHDRP (Perl source)
//! - <https://github.com/jakiki6/ruminant/blob/master/ruminant/modules/images.py>

#![allow(dead_code)]

use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

// ============================================================================
// Public entry point
// ============================================================================

/// Decode base64-encoded HDRP maker note data and return extracted tags.
///
/// `b64_data` is the raw base64 string from XMP GCamera:HdrPlusMakernote.
/// Returns a list of MakerNotes tags extracted from the protobuf payload.
pub fn decode_hdrp_makernote(b64_data: &str) -> Vec<Tag> {
    // Step 1: base64 decode
    let raw = match base64_decode(b64_data.trim()) {
        Some(v) => v,
        None => return Vec::new(),
    };

    // Step 2: decrypt + gunzip
    let decompressed = match hdrp_decrypt_gunzip(&raw) {
        Some(v) => v,
        None => return Vec::new(),
    };

    // Step 3: parse protobuf and map to tags
    parse_hdrp_protobuf(&decompressed)
}

// ============================================================================
// Base64 decoder
// ============================================================================

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const TABLE: [i8; 256] = {
        let mut t = [-1i8; 256];
        let mut i = 0u8;
        // A-Z = 0-25
        while i < 26 {
            t[(b'A' + i) as usize] = i as i8;
            i += 1;
        }
        // a-z = 26-51
        i = 0;
        while i < 26 {
            t[(b'a' + i) as usize] = (i + 26) as i8;
            i += 1;
        }
        // 0-9 = 52-61
        i = 0;
        while i < 10 {
            t[(b'0' + i) as usize] = (i + 52) as i8;
            i += 1;
        }
        t[b'+' as usize] = 62;
        t[b'/' as usize] = 63;
        t[b'=' as usize] = 0; // padding
        t
    };

    let bytes: Vec<u8> = s.bytes().filter(|&b| !b.is_ascii_whitespace()).collect();
    if bytes.is_empty() {
        return None;
    }

    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut i = 0;
    while i + 3 < bytes.len() {
        let a = TABLE[bytes[i] as usize];
        let b = TABLE[bytes[i + 1] as usize];
        let c = TABLE[bytes[i + 2] as usize];
        let d = TABLE[bytes[i + 3] as usize];
        if a < 0 || b < 0 {
            return None;
        }
        out.push(((a as u8) << 2) | ((b as u8) >> 4));
        if bytes[i + 2] != b'=' {
            out.push(((b as u8) << 4) | ((c as u8) >> 2));
        }
        if bytes[i + 3] != b'=' {
            out.push(((c as u8) << 6) | (d as u8));
        }
        i += 4;
    }
    Some(out)
}

// ============================================================================
// HDRP decryption (port of Perl ProcessHDRP XOR key schedule)
// ============================================================================

/// Decrypt the HDRP payload (after stripping "HDRP\x02" or "HDRP\x03" header).
/// Returns decrypted bytes, or None if not an HDRP stream.
fn hdrp_decrypt(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 5 || &data[0..4] != b"HDRP" {
        return None;
    }
    let _ver = data[4];

    // Data after the 5-byte HDRP header
    let payload = &data[5..];
    let pad = (8usize.wrapping_sub(payload.len() % 8)) & 7;
    let mut words: Vec<u32> = {
        let mut padded = payload.to_vec();
        padded.extend_from_slice(&[0u8; 8][..pad]);
        padded
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    };

    // Initial key: 0x2515606b4a7791cd (hi=0x2515606b, lo=0x4a7791cd)
    let mut hi: u64 = 0x2515606b;
    let mut lo: u64 = 0x4a7791cd;

    let mut i = 0;
    while i < words.len() {
        // Rotate the 64-bit key using the xorshift-then-multiply from Perl
        // All arithmetic kept as 64-bit to avoid wrapping issues

        // Perl: $lo ^= $lo >> 12 | ($hi & 0xfff) << 20;
        // Perl: $hi ^= $hi >> 12;
        // Note: Perl's `|` has lower precedence than `^=`, so:
        //   $lo ^= (($lo >> 12) | (($hi & 0xfff) << 20))
        let new_lo = lo ^ ((lo >> 12) | ((hi & 0xfff) << 20));
        let new_hi = hi ^ (hi >> 12);
        lo = new_lo & 0xffffffff;
        hi = new_hi & 0xffffffff;

        // Perl: $hi ^= ($hi & 0x7f) << 25 | $lo >> 7;
        // Perl: $lo ^= ($lo & 0x7f) << 25;
        let new_hi = hi ^ (((hi & 0x7f) << 25) | (lo >> 7));
        let new_lo = lo ^ ((lo & 0x7f) << 25);
        lo = new_lo & 0xffffffff;
        hi = new_hi & 0xffffffff;

        // Perl: $lo ^= $lo >> 27 | ($hi & 0x7ffffff) << 5;
        // Perl: $hi ^= $hi >> 27;
        let new_lo = lo ^ ((lo >> 27) | ((hi & 0x7ffffff) << 5));
        let new_hi = hi ^ (hi >> 27);
        lo = new_lo & 0xffffffff;
        hi = new_hi & 0xffffffff;

        // Multiply key by 0x2545f4914f6cdd1d (64-bit × 64-bit, keep low 64 bits)
        // Perl uses 16-bit chunks to avoid overflow; we use u128 in Rust
        let key64: u64 = (hi << 32) | lo;
        let product = (key64 as u128).wrapping_mul(0x2545f4914f6cdd1d_u128);
        let result64 = product as u64;
        lo = result64 & 0xffffffff;
        hi = (result64 >> 32) & 0xffffffff;

        // XOR the key lo/hi with the current 64-bit word
        words[i] ^= lo as u32;
        i += 1;
        if i < words.len() {
            words[i] ^= hi as u32;
            i += 1;
        }
    }

    // Reassemble into bytes, remove padding
    let mut result: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    if pad > 0 {
        result.truncate(result.len().saturating_sub(pad));
    }
    Some(result)
}

// ============================================================================
// DEFLATE decompressor (RFC 1951) + gzip wrapper (RFC 1952)
// ============================================================================

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize, // next byte to load into bit buffer
    bits: u32,  // LSB-first bit buffer
    nbits: u32, // number of valid bits in buffer
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        BitReader {
            data,
            pos: 0,
            bits: 0,
            nbits: 0,
        }
    }

    fn fill(&mut self) {
        while self.nbits <= 24 && self.pos < self.data.len() {
            self.bits |= (self.data[self.pos] as u32) << self.nbits;
            self.pos += 1;
            self.nbits += 8;
        }
    }

    fn read_bits(&mut self, n: u32) -> Option<u32> {
        if n == 0 {
            return Some(0);
        }
        self.fill();
        if self.nbits < n {
            return None;
        }
        let val = self.bits & ((1u32 << n) - 1);
        self.bits >>= n;
        self.nbits -= n;
        Some(val)
    }

    /// Align to byte boundary, discarding partial-byte bits.
    fn align_byte(&mut self) {
        let rem = self.nbits & 7;
        if rem != 0 {
            self.bits >>= rem;
            self.nbits -= rem;
        }
    }

    /// Read one byte at byte boundary.
    /// Call align_byte() first if needed.
    fn read_aligned_byte(&mut self) -> Option<u8> {
        if self.nbits >= 8 {
            let b = (self.bits & 0xff) as u8;
            self.bits >>= 8;
            self.nbits -= 8;
            Some(b)
        } else {
            // Buffer empty or partial - should be 0 after align_byte
            if self.pos < self.data.len() {
                let b = self.data[self.pos];
                self.pos += 1;
                Some(b)
            } else {
                None
            }
        }
    }

    /// Read 2-byte LE u16 at byte boundary.
    fn read_aligned_u16_le(&mut self) -> Option<u16> {
        let lo = self.read_aligned_byte()? as u16;
        let hi = self.read_aligned_byte()? as u16;
        Some(lo | (hi << 8))
    }
}

/// Build a Huffman decode table.
/// `lengths[i]` = bit length for symbol i.
/// Returns (table, max_bits) where table is indexed by reversed bit pattern.
fn build_huffman(lengths: &[u8]) -> Option<(Vec<(u16, u8)>, u8)> {
    let max_bits = *lengths.iter().max()? as usize;
    if max_bits == 0 {
        return Some((vec![], 0));
    }

    // Count symbols at each bit length
    let mut bl_count = vec![0u32; max_bits + 1];
    for &l in lengths {
        if l > 0 {
            bl_count[l as usize] += 1;
        }
    }

    // Compute starting codes
    let mut next_code = vec![0u32; max_bits + 2];
    let mut code = 0u32;
    for bits in 1..=max_bits {
        code = (code + bl_count[bits - 1]) << 1;
        next_code[bits] = code;
    }

    // Build table: for each symbol, store (symbol, length)
    // Table indexed by reversed code (so we can peek max_bits and reverse)
    let table_size = 1usize << max_bits;
    let mut table: Vec<(u16, u8)> = vec![(0xffff, 0); table_size];

    for (sym, &len) in lengths.iter().enumerate() {
        if len == 0 {
            continue;
        }
        let c = next_code[len as usize];
        next_code[len as usize] += 1;
        // Reverse the code bits so we can use LSB-first bit reading
        let rev = reverse_bits(c, len);
        // Fill all entries that start with this code
        let step = 1usize << len;
        let mut idx = rev as usize;
        while idx < table_size {
            table[idx] = (sym as u16, len);
            idx += step;
        }
    }
    Some((table, max_bits as u8))
}

fn reverse_bits(v: u32, n: u8) -> u32 {
    let mut r = 0u32;
    let mut v = v;
    for _ in 0..n {
        r = (r << 1) | (v & 1);
        v >>= 1;
    }
    r
}

fn huffman_decode(br: &mut BitReader, table: &[(u16, u8)], max_bits: u8) -> Option<u16> {
    br.fill();
    if max_bits == 0 {
        return None;
    }
    let peeked = br.bits & ((1 << max_bits) - 1);
    let (sym, len) = table[peeked as usize];
    if len == 0 || sym == 0xffff {
        return None;
    }
    br.bits >>= len;
    br.nbits -= len as u32;
    Some(sym)
}

// RFC 1951 fixed Huffman code lengths
fn fixed_literal_lengths() -> Vec<u8> {
    let mut v = Vec::with_capacity(288);
    for i in 0u16..288 {
        let l = if i < 144 {
            8
        } else if i < 256 {
            9
        } else if i < 280 {
            7
        } else {
            8
        };
        v.push(l);
    }
    v
}

fn fixed_distance_lengths() -> Vec<u8> {
    vec![5u8; 32]
}

// Length/distance decode tables from RFC 1951
fn decode_length(code: u16, br: &mut BitReader) -> Option<u16> {
    if code < 257 {
        return None;
    }
    if code == 285 {
        return Some(258);
    }
    let extra_bits_table = [
        0u32, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
    ];
    let base_table = [
        3u16, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99,
        115, 131, 163, 195, 227, 258,
    ];
    let idx = (code - 257) as usize;
    if idx >= 29 {
        return None;
    }
    let extra = br.read_bits(extra_bits_table[idx])?;
    Some(base_table[idx] + extra as u16)
}

fn decode_distance(code: u16, br: &mut BitReader) -> Option<u32> {
    if code >= 30 {
        return None;
    }
    let extra_bits_table = [
        0u32, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12,
        12, 13, 13,
    ];
    let base_table = [
        1u32, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025,
        1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
    ];
    let extra = br.read_bits(extra_bits_table[code as usize])?;
    Some(base_table[code as usize] + extra)
}

fn inflate_block(
    br: &mut BitReader,
    lit_table: &[(u16, u8)],
    lit_max: u8,
    dist_table: &[(u16, u8)],
    dist_max: u8,
    out: &mut Vec<u8>,
) -> Option<bool> {
    loop {
        let sym = huffman_decode(br, lit_table, lit_max)?;
        if sym < 256 {
            out.push(sym as u8);
        } else if sym == 256 {
            return Some(false); // end of block
        } else {
            let length = decode_length(sym, br)? as usize;
            let dist_code = huffman_decode(br, dist_table, dist_max)?;
            let dist = decode_distance(dist_code, br)? as usize;
            if dist > out.len() {
                return None;
            }
            let start = out.len() - dist;
            for i in 0..length {
                let b = out[start + i % dist];
                out.push(b);
            }
        }
    }
}

fn deflate_decompress(data: &[u8]) -> Option<Vec<u8>> {
    let mut br = BitReader::new(data);
    let mut out = Vec::new();

    loop {
        let bfinal = br.read_bits(1)?;
        let btype = br.read_bits(2)?;

        match btype {
            0 => {
                // Stored block
                br.align_byte();
                let len = br.read_aligned_u16_le()? as usize;
                let nlen = br.read_aligned_u16_le()? as usize;
                if (len ^ nlen) & 0xffff != 0xffff {
                    return None;
                }
                for _ in 0..len {
                    out.push(br.read_aligned_byte()?);
                }
            }
            1 => {
                // Fixed Huffman
                let lit_lens = fixed_literal_lengths();
                let dist_lens = fixed_distance_lengths();
                let (lit_table, lit_max) = build_huffman(&lit_lens)?;
                let (dist_table, dist_max) = build_huffman(&dist_lens)?;
                inflate_block(
                    &mut br,
                    &lit_table,
                    lit_max,
                    &dist_table,
                    dist_max,
                    &mut out,
                )?;
            }
            2 => {
                // Dynamic Huffman
                let hlit = br.read_bits(5)? as usize + 257;
                let hdist = br.read_bits(5)? as usize + 1;
                let hclen = br.read_bits(4)? as usize + 4;

                // Code length alphabet order
                let cl_order = [
                    16usize, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
                ];
                let mut cl_lengths = vec![0u8; 19];
                for i in 0..hclen {
                    cl_lengths[cl_order[i]] = br.read_bits(3)? as u8;
                }
                let (cl_table, cl_max) = build_huffman(&cl_lengths)?;

                // Decode literal/length + distance code lengths
                let total = hlit + hdist;
                let mut all_lengths = Vec::with_capacity(total);
                while all_lengths.len() < total {
                    let code = huffman_decode(&mut br, &cl_table, cl_max)?;
                    match code {
                        0..=15 => all_lengths.push(code as u8),
                        16 => {
                            let extra = br.read_bits(2)? as usize + 3;
                            let last = *all_lengths.last()?;
                            for _ in 0..extra {
                                all_lengths.push(last);
                            }
                        }
                        17 => {
                            let extra = br.read_bits(3)? as usize + 3;
                            all_lengths.resize(all_lengths.len() + extra, 0);
                        }
                        18 => {
                            let extra = br.read_bits(7)? as usize + 11;
                            all_lengths.resize(all_lengths.len() + extra, 0);
                        }
                        _ => return None,
                    }
                }
                let lit_lens = &all_lengths[..hlit];
                let dist_lens = &all_lengths[hlit..hlit + hdist];
                let (lit_table, lit_max) = build_huffman(lit_lens)?;
                let (dist_table, dist_max) = build_huffman(dist_lens)?;
                inflate_block(
                    &mut br,
                    &lit_table,
                    lit_max,
                    &dist_table,
                    dist_max,
                    &mut out,
                )?;
            }
            _ => return None,
        }

        if bfinal == 1 {
            break;
        }
    }
    Some(out)
}

/// Decompress gzip-compressed data (RFC 1952 wrapper + DEFLATE payload).
fn gunzip(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 18 {
        return None;
    }
    if data[0] != 0x1f || data[1] != 0x8b {
        return None;
    }
    let method = data[2];
    if method != 8 {
        return None;
    }
    let flags = data[3];

    let mut pos = 10usize;
    // Skip FEXTRA
    if flags & 0x04 != 0 {
        if pos + 2 > data.len() {
            return None;
        }
        let xlen = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2 + xlen;
    }
    // Skip FNAME
    if flags & 0x08 != 0 {
        while pos < data.len() && data[pos] != 0 {
            pos += 1;
        }
        pos += 1;
    }
    // Skip FCOMMENT
    if flags & 0x10 != 0 {
        while pos < data.len() && data[pos] != 0 {
            pos += 1;
        }
        pos += 1;
    }
    // Skip CRC16
    if flags & 0x02 != 0 {
        pos += 2;
    }

    if pos + 8 > data.len() {
        return None;
    }
    let compressed = &data[pos..data.len() - 8];
    deflate_decompress(compressed)
}

fn hdrp_decrypt_gunzip(data: &[u8]) -> Option<Vec<u8>> {
    let decrypted = hdrp_decrypt(data)?;
    gunzip(&decrypted)
}

// ============================================================================
// Protobuf parser → tag extraction
// ============================================================================

/// Read a varint from data starting at `pos`. Returns (value, new_pos) or None.
fn read_varint(data: &[u8], pos: usize) -> Option<(u64, usize)> {
    let mut val = 0u64;
    let mut shift = 0u32;
    let mut p = pos;
    loop {
        if p >= data.len() {
            return None;
        }
        let b = data[p] as u64;
        p += 1;
        val |= (b & 0x7f) << shift;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 70 {
            return None;
        } // malformed
    }
    Some((val, p))
}

/// Read length-delimited bytes from data at `pos`. Returns (bytes, new_pos) or None.
fn read_len_delimited(data: &[u8], pos: usize) -> Option<(&[u8], usize)> {
    let (len, p) = read_varint(data, pos)?;
    let end = p + len as usize;
    if end > data.len() {
        return None;
    }
    Some((&data[p..end], end))
}

/// Tag info for a known protobuf field path.
struct FieldDef {
    path: &'static str, // e.g. "1-1", "12-8"
    name: &'static str,
    fmt: FieldFmt,
    group2: &'static str,
}

#[derive(Clone, Copy, Debug)]
enum FieldFmt {
    String,
    Binary,
    Unsigned,     // varint as unsigned integer
    Float,        // 4-byte IEEE float
    FloatDiv1000, // float / 1000 (ExposureTime conversion)
    UnixTimeMs,   // varint / 1000 → Unix timestamp with milliseconds
}

static HDRP_FIELDS: &[FieldDef] = &[
    FieldDef {
        path: "1-1",
        name: "ImageName",
        fmt: FieldFmt::String,
        group2: "Image",
    },
    FieldDef {
        path: "1-2",
        name: "ImageData",
        fmt: FieldFmt::Binary,
        group2: "Image",
    },
    FieldDef {
        path: "2",
        name: "TimeLogText",
        fmt: FieldFmt::Binary,
        group2: "Image",
    },
    FieldDef {
        path: "3",
        name: "SummaryText",
        fmt: FieldFmt::Binary,
        group2: "Image",
    },
    FieldDef {
        path: "9-3",
        name: "FrameCount",
        fmt: FieldFmt::Unsigned,
        group2: "Image",
    },
    FieldDef {
        path: "12-1",
        name: "DeviceMake",
        fmt: FieldFmt::String,
        group2: "Device",
    },
    FieldDef {
        path: "12-2",
        name: "DeviceModel",
        fmt: FieldFmt::String,
        group2: "Device",
    },
    FieldDef {
        path: "12-3",
        name: "DeviceCodename",
        fmt: FieldFmt::String,
        group2: "Device",
    },
    FieldDef {
        path: "12-4",
        name: "DeviceHardwareRevision",
        fmt: FieldFmt::String,
        group2: "Device",
    },
    FieldDef {
        path: "12-6",
        name: "HDRPSoftware",
        fmt: FieldFmt::String,
        group2: "Device",
    },
    FieldDef {
        path: "12-7",
        name: "AndroidRelease",
        fmt: FieldFmt::String,
        group2: "Device",
    },
    FieldDef {
        path: "12-8",
        name: "SoftwareDate",
        fmt: FieldFmt::UnixTimeMs,
        group2: "Time",
    },
    FieldDef {
        path: "12-9",
        name: "Application",
        fmt: FieldFmt::String,
        group2: "Device",
    },
    FieldDef {
        path: "12-10",
        name: "AppVersion",
        fmt: FieldFmt::String,
        group2: "Device",
    },
    FieldDef {
        path: "12-12-1",
        name: "ExposureTimeMin",
        fmt: FieldFmt::FloatDiv1000,
        group2: "Camera",
    },
    FieldDef {
        path: "12-12-2",
        name: "ExposureTimeMax",
        fmt: FieldFmt::FloatDiv1000,
        group2: "Camera",
    },
    FieldDef {
        path: "12-13-1",
        name: "ISOMin",
        fmt: FieldFmt::Float,
        group2: "Camera",
    },
    FieldDef {
        path: "12-13-2",
        name: "ISOMax",
        fmt: FieldFmt::Float,
        group2: "Camera",
    },
    FieldDef {
        path: "12-14",
        name: "MaxAnalogISO",
        fmt: FieldFmt::Float,
        group2: "Camera",
    },
];

fn make_tag(name: &str, group2: &str, raw: Value, print: String) -> Tag {
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: name.to_string(),
        group: TagGroup {
            // ExifTool shows these as [MakerNotes] with -G (family1 defaults to "MakerNotes"
            // when GROUPS doesn't specify family1 for the HDRPlusMakerNote table)
            family0: "MakerNotes".into(),
            family1: "MakerNotes".into(),
            family2: group2.into(),
        },
        raw_value: raw,
        print_value: print,
        priority: 0,
    }
}

/// Parse protobuf data recursively, collecting tags.
/// `prefix` is the dot-separated path built so far (e.g. "" at top, "12-" inside field 12).
fn parse_protobuf(data: &[u8], prefix: &str, tags: &mut Vec<Tag>) {
    let mut pos = 0;
    while pos < data.len() {
        let (tag_word, p) = match read_varint(data, pos) {
            Some(v) => v,
            None => break,
        };
        let field_id = tag_word >> 3;
        let wire_type = tag_word & 7;
        pos = p;

        let path = if prefix.is_empty() {
            format!("{}", field_id)
        } else {
            format!("{}{}", prefix, field_id)
        };

        match wire_type {
            0 => {
                // varint
                let (val, p2) = match read_varint(data, pos) {
                    Some(v) => v,
                    None => break,
                };
                pos = p2;
                // Check if this path is a known field
                if let Some(def) = HDRP_FIELDS.iter().find(|d| d.path == path) {
                    let (raw, print) = match def.fmt {
                        FieldFmt::Unsigned => {
                            let s = format!("{}", val);
                            (Value::U32(val as u32), s)
                        }
                        FieldFmt::UnixTimeMs => {
                            // ExifTool: ConvertUnixTime($val / 1000, 1, 3)
                            // Convert ms timestamp to ExifTool datetime format
                            let secs = (val / 1000) as i64;
                            let ms = val % 1000;
                            let s = format_unix_time_ms(secs, ms as u32);
                            (Value::String(s.clone()), s)
                        }
                        _ => {
                            let s = format!("{}", val);
                            (Value::U32(val as u32), s)
                        }
                    };
                    tags.push(make_tag(def.name, def.group2, raw, print));
                }
            }
            1 => {
                // 64-bit
                if pos + 8 > data.len() {
                    break;
                }
                pos += 8;
            }
            2 => {
                // length-delimited
                let (bytes, p2) = match read_len_delimited(data, pos) {
                    Some(v) => v,
                    None => break,
                };
                pos = p2;

                // Check known fields first
                if let Some(def) = HDRP_FIELDS.iter().find(|d| d.path == path) {
                    match def.fmt {
                        FieldFmt::String => {
                            let s = crate::encoding::decode_utf8_or_latin1(bytes).to_string();
                            tags.push(make_tag(def.name, def.group2, Value::String(s.clone()), s));
                        }
                        FieldFmt::Binary => {
                            // Store as binary; ExifTool shows "(Binary data N bytes)"
                            let print = format!(
                                "(Binary data {} bytes, use -b option to extract)",
                                bytes.len()
                            );
                            tags.push(make_tag(
                                def.name,
                                def.group2,
                                Value::Binary(bytes.to_vec()),
                                print,
                            ));
                        }
                        FieldFmt::Float | FieldFmt::FloatDiv1000 => {
                            // 4-byte float in a len-delimited wrapper
                            if bytes.len() >= 4 {
                                let f =
                                    f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                                let v = if matches!(def.fmt, FieldFmt::FloatDiv1000) {
                                    f as f64 / 1000.0
                                } else {
                                    f as f64
                                };
                                let s = format_float(v);
                                tags.push(make_tag(def.name, def.group2, Value::F64(v), s));
                            }
                        }
                        _ => {
                            // Try to recurse as sub-message
                            parse_protobuf(bytes, &format!("{}-", path), tags);
                        }
                    }
                } else {
                    // Unknown field - try to recurse as sub-message to find known sub-fields
                    parse_protobuf(bytes, &format!("{}-", path), tags);
                }
            }
            5 => {
                // 32-bit
                if pos + 4 > data.len() {
                    break;
                }
                let bytes = &data[pos..pos + 4];
                pos += 4;

                if let Some(def) = HDRP_FIELDS.iter().find(|d| d.path == path) {
                    if matches!(def.fmt, FieldFmt::Float | FieldFmt::FloatDiv1000) {
                        let f = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                        let v = if matches!(def.fmt, FieldFmt::FloatDiv1000) {
                            f as f64 / 1000.0
                        } else {
                            f as f64
                        };
                        let s = format_float(v);
                        tags.push(make_tag(def.name, def.group2, Value::F64(v), s));
                    }
                }
            }
            3 | 4 => {
                // Deprecated group start/end - skip
            }
            _ => break, // Unknown wire type, stop parsing
        }
    }
}

/// Parse the decompressed HDRP protobuf payload.
fn parse_hdrp_protobuf(data: &[u8]) -> Vec<Tag> {
    let mut tags = Vec::new();
    parse_protobuf(data, "", &mut tags);
    tags
}

// ============================================================================
// Utilities
// ============================================================================

/// Format a float value like ExifTool/Perl's default stringification.
/// Perl uses up to ~15 significant digits, removes trailing zeros.
fn format_float(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    // Use Perl-like %.15g formatting: up to 15 significant digits, no trailing zeros
    format_g(v, 15)
}

/// Format like printf("%.{prec}g", v) - significant digits, strip trailing zeros
fn format_g(v: f64, prec: usize) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let abs = v.abs();
    // Use scientific notation if exponent >= prec or exponent < -4
    let exp = abs.log10().floor() as i32;
    if exp >= prec as i32 || exp < -4 {
        // Scientific notation
        let _mantissa = v / 10f64.powi(exp);
        let decimals = prec.saturating_sub(1);
        let s = format!("{:.prec$e}", v, prec = decimals);
        // Perl uses e+XX format
        normalize_sci(s)
    } else {
        // Fixed notation with enough precision
        let decimals = if exp >= 0 {
            prec.saturating_sub(exp as usize + 1)
        } else {
            prec + (-exp - 1) as usize
        };
        let s = format!("{:.prec$}", v, prec = decimals);
        // Strip trailing zeros after decimal point
        strip_trailing_zeros(s)
    }
}

fn normalize_sci(s: String) -> String {
    // Convert Rust "1.234e5" to Perl "1.234e+05" or "1.234e-05"
    if let Some(e_pos) = s.find('e') {
        let mantissa = &s[..e_pos];
        let exp_str = &s[e_pos + 1..];
        let mantissa = strip_trailing_zeros(mantissa.to_string());
        let exp: i32 = exp_str.parse().unwrap_or(0);
        if exp >= 0 {
            format!("{}e+{:02}", mantissa, exp)
        } else {
            format!("{}e-{:02}", mantissa, -exp)
        }
    } else {
        s
    }
}

fn strip_trailing_zeros(s: String) -> String {
    if s.contains('.') {
        let s = s.trim_end_matches('0').to_string();
        let s = s.trim_end_matches('.').to_string();
        s
    } else {
        s
    }
}

/// Format a Unix timestamp (seconds + milliseconds) in ExifTool's datetime format.
/// ExifTool uses ConvertUnixTime($val/1000, 1, 3) which gives "YYYY:MM:DD HH:MM:SS.mmm+TZ"
/// We use UTC since we don't have timezone info.
fn format_unix_time_ms(secs: i64, ms: u32) -> String {
    // Simple Unix timestamp to date conversion
    // Days since epoch
    let (year, month, day, hour, min, sec) = unix_to_datetime(secs);
    format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}.{:03}+00:00",
        year, month, day, hour, min, sec, ms
    )
}

fn unix_to_datetime(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    // Compute date from Unix timestamp (seconds since 1970-01-01 00:00:00 UTC)
    let sec = (secs % 60) as u32;
    let min_total = secs / 60;
    let min = (min_total % 60) as u32;
    let hour_total = min_total / 60;
    let hour = (hour_total % 24) as u32;
    let days = hour_total / 24;

    // Convert days since epoch to year/month/day
    // Algorithm from https://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    (y as i32, m as u32, d as u32, hour, min, sec)
}

// ============================================================================
// Value enum extensions needed
// ============================================================================

// We need Value::U64 and Value::F64 - check if they exist
// If not, we'll use Value::String as fallback

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_decode() {
        // "HDRP" base64 encoded
        let encoded = "SERS"; // first 3 bytes of "HDRP..."
        let decoded = base64_decode(encoded);
        assert!(decoded.is_some());
    }

    #[test]
    fn test_reverse_bits() {
        assert_eq!(reverse_bits(0b101, 3), 0b101);
        assert_eq!(reverse_bits(0b001, 3), 0b100);
        assert_eq!(reverse_bits(0b110, 3), 0b011);
    }

    #[test]
    fn test_deflate_stored() {
        // A stored block: BFINAL=1 BTYPE=00, then LEN/NLEN then data
        let data = b"Hello";
        let len = data.len() as u16;
        let nlen = !len;
        let mut block = vec![0x01u8]; // BFINAL=1, BTYPE=00 (stored)
        block.extend_from_slice(&len.to_le_bytes());
        block.extend_from_slice(&nlen.to_le_bytes());
        block.extend_from_slice(data);
        let result = deflate_decompress(&block);
        assert_eq!(result, Some(b"Hello".to_vec()));
    }
}

#[cfg(test)]
mod actual_data_tests {
    use super::*;

    #[test]
    fn test_actual_hdrp_decode() {
        // Read b64 from file if available
        let b64_path = "/tmp/hdrp_b64.txt";
        if let Ok(b64) = std::fs::read_to_string(b64_path) {
            let b64 = b64.trim();

            // Step 1: base64 decode
            let raw = base64_decode(b64).expect("b64 decode failed");
            assert!(raw.len() > 5, "too short: {}", raw.len());
            assert_eq!(&raw[..4], b"HDRP", "not HDRP: {:?}", &raw[..4]);
            assert_eq!(raw[4], 3, "wrong version: {}", raw[4]);

            // Step 2: decrypt
            let decrypted = hdrp_decrypt(&raw).expect("decrypt failed");
            assert!(decrypted.len() > 2, "decrypt too short");
            assert_eq!(
                decrypted[0], 0x1f,
                "not gzip magic[0]: {:02x}",
                decrypted[0]
            );
            assert_eq!(
                decrypted[1], 0x8b,
                "not gzip magic[1]: {:02x}",
                decrypted[1]
            );

            // Step 3: gunzip
            let decompressed = gunzip(&decrypted).expect("gunzip failed");
            assert!(!decompressed.is_empty(), "decompressed is empty");
            eprintln!(
                "decompressed {} bytes, first 20: {}",
                decompressed.len(),
                decompressed
                    .iter()
                    .take(20)
                    .map(|b| format!("{:02x}", b))
                    .collect::<Vec<_>>()
                    .join(" ")
            );

            // Step 4: parse protobuf
            let tags = parse_hdrp_protobuf(&decompressed);
            eprintln!("Found {} tags", tags.len());
            for tag in &tags {
                eprintln!("  {} = {}", tag.name, tag.print_value);
            }
            assert!(!tags.is_empty(), "no tags found");
        } else {
            eprintln!("Test file not found, skipping");
        }
    }
}
