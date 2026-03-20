//! BZZ decompression for DjVu files.
//!
//! Port of Image::ExifTool::BZZ (based on DjVuLibre ZPCodec + BWT + MTF).
//! All arithmetic is integer (matching Perl's `use integer`).

// ZP-Coder probability tables
static P: [u32; 256] = [
    0x8000, 0x8000, 0x8000, 0x6bbd, 0x6bbd, 0x5d45, 0x5d45, 0x51b9,
    0x51b9, 0x4813, 0x4813, 0x3fd5, 0x3fd5, 0x38b1, 0x38b1, 0x3275,
    0x3275, 0x2cfd, 0x2cfd, 0x2825, 0x2825, 0x23ab, 0x23ab, 0x1f87,
    0x1f87, 0x1bbb, 0x1bbb, 0x1845, 0x1845, 0x1523, 0x1523, 0x1253,
    0x1253, 0x0fcf, 0x0fcf, 0x0d95, 0x0d95, 0x0b9d, 0x0b9d, 0x09e3,
    0x09e3, 0x0861, 0x0861, 0x0711, 0x0711, 0x05f1, 0x05f1, 0x04f9,
    0x04f9, 0x0425, 0x0425, 0x0371, 0x0371, 0x02d9, 0x02d9, 0x0259,
    0x0259, 0x01ed, 0x01ed, 0x0193, 0x0193, 0x0149, 0x0149, 0x010b,
    0x010b, 0x00d5, 0x00d5, 0x00a5, 0x00a5, 0x007b, 0x007b, 0x0057,
    0x0057, 0x003b, 0x003b, 0x0023, 0x0023, 0x0013, 0x0013, 0x0007,
    0x0007, 0x0001, 0x0001, 0x5695, 0x24ee, 0x8000, 0x0d30, 0x481a,
    0x0481, 0x3579, 0x017a, 0x24ef, 0x007b, 0x1978, 0x0028, 0x10ca,
    0x000d, 0x0b5d, 0x0034, 0x078a, 0x00a0, 0x050f, 0x0117, 0x0358,
    0x01ea, 0x0234, 0x0144, 0x0173, 0x0234, 0x00f5, 0x0353, 0x00a1,
    0x05c5, 0x011a, 0x03cf, 0x01aa, 0x0285, 0x0286, 0x01ab, 0x03d3,
    0x011a, 0x05c5, 0x00ba, 0x08ad, 0x007a, 0x0ccc, 0x01eb, 0x1302,
    0x02e6, 0x1b81, 0x045e, 0x24ef, 0x0690, 0x2865, 0x09de, 0x3987,
    0x0dc8, 0x2c99, 0x10ca, 0x3b5f, 0x0b5d, 0x5695, 0x078a, 0x8000,
    0x050f, 0x24ee, 0x0358, 0x0d30, 0x0234, 0x0481, 0x0173, 0x017a,
    0x00f5, 0x007b, 0x00a1, 0x0028, 0x011a, 0x000d, 0x01aa, 0x0034,
    0x0286, 0x00a0, 0x03d3, 0x0117, 0x05c5, 0x01ea, 0x08ad, 0x0144,
    0x0ccc, 0x0234, 0x1302, 0x0353, 0x1b81, 0x05c5, 0x24ef, 0x03cf,
    0x2b74, 0x0285, 0x201d, 0x01ab, 0x1715, 0x011a, 0x0fb7, 0x00ba,
    0x0a67, 0x01eb, 0x06e7, 0x02e6, 0x0496, 0x045e, 0x030d, 0x0690,
    0x0206, 0x09de, 0x0155, 0x0dc8, 0x00e1, 0x2b74, 0x0094, 0x201d,
    0x0188, 0x1715, 0x0252, 0x0fb7, 0x0383, 0x0a67, 0x0547, 0x06e7,
    0x07e2, 0x0496, 0x0bc0, 0x030d, 0x1178, 0x0206, 0x19da, 0x0155,
    0x24ef, 0x00e1, 0x320e, 0x0094, 0x432a, 0x0188, 0x447d, 0x0252,
    0x5ece, 0x0383, 0x8000, 0x0547, 0x481a, 0x07e2, 0x3579, 0x0bc0,
    0x24ef, 0x1178, 0x1978, 0x19da, 0x2865, 0x24ef, 0x3987, 0x320e,
    0x2c99, 0x432a, 0x3b5f, 0x447d, 0x5695, 0x5ece, 0x8000, 0x8000,
    0x5695, 0x481a, 0x481a, 0, 0, 0, 0, 0,
];

static M: [u32; 258] = [
    0x0000, 0x0000, 0x0000, 0x10a5, 0x10a5, 0x1f28, 0x1f28, 0x2bd3,
    0x2bd3, 0x36e3, 0x36e3, 0x408c, 0x408c, 0x48fd, 0x48fd, 0x505d,
    0x505d, 0x56d0, 0x56d0, 0x5c71, 0x5c71, 0x615b, 0x615b, 0x65a5,
    0x65a5, 0x6962, 0x6962, 0x6ca2, 0x6ca2, 0x6f74, 0x6f74, 0x71e6,
    0x71e6, 0x7404, 0x7404, 0x75d6, 0x75d6, 0x7768, 0x7768, 0x78c2,
    0x78c2, 0x79ea, 0x79ea, 0x7ae7, 0x7ae7, 0x7bbe, 0x7bbe, 0x7c75,
    0x7c75, 0x7d0f, 0x7d0f, 0x7d91, 0x7d91, 0x7dfe, 0x7dfe, 0x7e5a,
    0x7e5a, 0x7ea6, 0x7ea6, 0x7ee6, 0x7ee6, 0x7f1a, 0x7f1a, 0x7f45,
    0x7f45, 0x7f6b, 0x7f6b, 0x7f8d, 0x7f8d, 0x7faa, 0x7faa, 0x7fc3,
    0x7fc3, 0x7fd7, 0x7fd7, 0x7fe7, 0x7fe7, 0x7ff2, 0x7ff2, 0x7ffa,
    0x7ffa, 0x7fff, 0x7fff,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0,
];

static UP: [usize; 256] = [
     84,   3,   4,   5,   6,   7,   8,   9,  10,  11,  12,  13,  14,  15,  16,  17,
     18,  19,  20,  21,  22,  23,  24,  25,  26,  27,  28,  29,  30,  31,  32,  33,
     34,  35,  36,  37,  38,  39,  40,  41,  42,  43,  44,  45,  46,  47,  48,  49,
     50,  51,  52,  53,  54,  55,  56,  57,  58,  59,  60,  61,  62,  63,  64,  65,
     66,  67,  68,  69,  70,  71,  72,  73,  74,  75,  76,  77,  78,  79,  80,  81,
     82,  81,  82,   9,  86,   5,  88,  89,  90,  91,  92,  93,  94,  95,  96,  97,
     82,  99,  76, 101,  70, 103,  66, 105, 106, 107,  66, 109,  60, 111,  56,  69,
    114,  65, 116,  61, 118,  57, 120,  53, 122,  49, 124,  43,  72,  39,  60,  33,
     56,  29,  52,  23,  48,  23,  42, 137,  38,  21, 140,  15, 142,   9, 144, 141,
    146, 147, 148, 149, 150, 151, 152, 153, 154, 155,  70, 157,  66,  81,  62,  75,
     58,  69,  54,  65,  50, 167,  44,  65,  40,  59,  34,  55,  30, 175,  24, 177,
    178, 179, 180, 181, 182, 183, 184,  69, 186,  59, 188,  55, 190,  51, 192,  47,
    194,  41, 196,  37, 198, 199,  72, 201,  62, 203,  58, 205,  54, 207,  50, 209,
     46, 211,  40, 213,  36, 215,  30, 217,  26, 219,  20,  71,  14,  61,  14,  57,
      8,  53, 228,  49, 230,  45, 232,  39, 234,  35, 138,  29,  24,  25, 240,  19,
     22,  13,  16,  13,  10,   7, 244, 249,  10,  89, 230,   0,   0,   0,   0,   0,
];

static DN: [usize; 256] = [
    145,   4,   3,   1,   2,   3,   4,   5,   6,   7,   8,   9,  10,  11,  12,  13,
     14,  15,  16,  17,  18,  19,  20,  21,  22,  23,  24,  25,  26,  27,  28,  29,
     30,  31,  32,  33,  34,  35,  36,  37,  38,  39,  40,  41,  42,  43,  44,  45,
     46,  47,  48,  49,  50,  51,  52,  53,  54,  55,  56,  57,  58,  59,  60,  61,
     62,  63,  64,  65,  66,  67,  68,  69,  70,  71,  72,  73,  74,  75,  76,  77,
     78,  79,  80,  85, 226,   6, 176, 143, 138, 141, 112, 135, 104, 133, 100, 129,
     98, 127,  72, 125, 102, 123,  60, 121, 110, 119, 108, 117,  54, 115,  48, 113,
    134,  59, 132,  55, 130,  51, 128,  47, 126,  41,  62,  37,  66,  31,  54,  25,
     50, 131,  46,  17,  40,  15, 136,   7,  32, 139, 172,   9, 170,  85, 168, 248,
    166, 247, 164, 197, 162,  95, 160, 173, 158, 165, 156, 161,  60, 159,  56,  71,
     52, 163,  48,  59,  42, 171,  38, 169,  32,  53,  26,  47, 174, 193,  18, 191,
    222, 189, 218, 187, 216, 185, 214,  61, 212,  53, 210,  49, 208,  45, 206,  39,
    204, 195, 202,  31, 200, 243,  64, 239,  56, 237,  52, 235,  48, 233,  44, 231,
     38, 229,  34, 227,  28, 225,  22, 223,  16, 221, 220,  63,   8,  55, 224,  51,
      2,  47,  87,  43, 246,  37, 244,  33, 238,  27, 236,  21,  16,  15,   8, 241,
    242,   7,  10, 245,   2,   1,  83, 250,   2, 143, 246,   0,   0,   0,   0,   0,
];

const FREQMAX: usize = 4;
const CTXIDS: usize = 3;
const MAXBLOCK: usize = 4096 * 1024;

// Build the ffzt (find first zero) lookup table
fn make_ffzt() -> [u32; 256] {
    let mut t = [0u32; 256];
    for i in 0..256 {
        let mut count = 0u32;
        let mut j = i as u8;
        while j & 0x80 != 0 {
            count += 1;
            j = j.wrapping_shl(1);
        }
        t[i] = count;
    }
    t
}

struct Zp<'a> {
    data: &'a [u8],
    pos: usize,
    data_len: usize,
    code: u32,
    byte: u32,
    a: u32,
    buffer: u32,
    fence: u32,
    scount: i32,
    delay: i32,
    ffzt: [u32; 256],
    ctx: Vec<usize>,   // context states (indices into P/M/UP/DN tables)
    error: bool,
}

impl<'a> Zp<'a> {
    fn new(data: &'a [u8]) -> Self {
        let ffzt = make_ffzt();
        let data_len = data.len();
        let (code, pos) = if data_len >= 2 {
            (((data[0] as u32) << 8) | data[1] as u32, 2)
        } else if data_len == 1 {
            (((data[0] as u32) << 8) | 0xff, 1)
        } else {
            (0xffff, 0)
        };
        let byte = code & 0xff;
        let fence = if code >= 0x8000 { 0x7fff } else { code };
        Zp {
            data,
            pos,
            data_len,
            code,
            byte,
            a: 0,
            buffer: 0,
            fence,
            scount: 0,
            delay: 25,
            ffzt,
            ctx: vec![0usize; 300],
            error: false,
        }
    }

    /// Ensure at least 16 bits available in buffer.
    fn preload(&mut self) {
        if self.scount < 16 {
            while self.scount <= 24 {
                if self.pos < self.data_len {
                    self.byte = self.data[self.pos] as u32;
                    self.pos += 1;
                } else {
                    self.byte = 0xff;
                    self.delay -= 1;
                    if self.delay < 1 {
                        self.error = true;
                        return;
                    }
                }
                self.buffer = (self.buffer << 8) | self.byte;
                self.scount += 8;
            }
        }
    }

    /// Decode one bit using context state at ctx_states[ctx_idx].
    fn decode(&mut self, ctx_idx: usize) -> u32 {
        let ctx = self.ctx[ctx_idx];
        let z = self.a.wrapping_add(P[ctx]);
        if z <= self.fence {
            self.a = z;
            return (ctx & 1) as u32;
        }
        self.decode_sub(z, Some(ctx_idx))
    }

    /// Decode one bit without context (used for size/fshift).
    fn decode_nc(&mut self, z: u32) -> u32 {
        if z <= self.fence {
            self.a = z;
            return 0;
        }
        self.decode_sub(z, None)
    }

    /// Core decode_sub - mirrors Perl decode_sub.
    fn decode_sub(&mut self, z_in: u32, ctx_idx: Option<usize>) -> u32 {
        self.preload();
        if self.error { return 0; }

        let mut z = z_in;
        let bit: u32;
        let code = self.code;

        let (initial_bit, has_ctx) = if let Some(ci) = ctx_idx {
            let ctx = self.ctx[ci];
            let ib = (ctx & 1) as u32;
            // Avoid interval reversion
            let d = 0x6000u32.wrapping_add((z.wrapping_add(self.a)) >> 2);
            if z > d { z = d; }
            (ib, true)
        } else {
            (0u32, false)
        };

        if z > code {
            bit = initial_bit ^ 1;
            // LPS branch
            let diff = 0x10000u32.wrapping_sub(z);
            self.a = self.a.wrapping_add(diff);
            let new_code = code.wrapping_add(diff);
            // LPS adaptation
            if has_ctx {
                let ci = ctx_idx.unwrap();
                self.ctx[ci] = DN[self.ctx[ci]];
            }
            // LPS renormalization
            let a = self.a;
            let sft = if a >= 0xff00 {
                self.ffzt[(a & 0xff) as usize] + 8
            } else {
                self.ffzt[((a >> 8) & 0xff) as usize]
            } as i32;
            self.scount -= sft;
            self.a = (a << sft) & 0xffff;
            let sc = self.scount;
            self.code = ((new_code << sft) & 0xffff) | ((self.buffer >> sc) & ((1 << sft) - 1));
        } else {
            bit = initial_bit;
            // MPS adaptation
            if has_ctx {
                let ci = ctx_idx.unwrap();
                let ctx = self.ctx[ci];
                if self.a >= M[ctx] {
                    self.ctx[ci] = UP[ctx];
                }
            }
            // MPS renormalization
            self.scount -= 1;
            self.a = (z << 1) & 0xffff;
            let sc = self.scount;
            self.code = ((code << 1) & 0xffff) | ((self.buffer >> sc) & 1);
        }
        self.fence = if self.code >= 0x8000 { 0x7fff } else { self.code };
        bit
    }
}

/// Decode BZZ-compressed data. Returns decompressed bytes or None on error.
pub fn decode(data: &[u8]) -> Option<Vec<u8>> {
    if data.is_empty() {
        return None;
    }

    let mut zp = Zp::new(data);

    // Decode block size
    let mut n: u32 = 1;
    let m: u32 = 1 << 24;
    while n < m {
        let a = zp.a;
        let z = 0x8000u32.wrapping_add(a >> 1);
        let b = zp.decode_nc(z);
        if zp.error { return None; }
        n = (n << 1) | b;
    }
    let size = (n - m) as usize;
    if size == 0 { return Some(Vec::new()); }
    if size > MAXBLOCK { return None; }

    // Decode fshift
    let a = zp.a;
    let z = 0x8000u32.wrapping_add(a >> 1);
    let mut fshift: u32 = 0;
    if zp.decode_nc(z) != 0 {
        fshift += 1;
        let a2 = zp.a;
        let z2 = 0x8000u32.wrapping_add(a2 >> 1);
        if zp.decode_nc(z2) != 0 {
            fshift += 1;
        }
    }
    if zp.error { return None; }

    // Quasi-MTF
    let mut mtf: Vec<u32> = (0..256).collect();
    let mut freq = [0u32; FREQMAX];
    let mut fadd: u32 = 4;
    let mut mtfno: usize = 3;
    let mut markerpos: i32 = -1;
    let mut dat = vec![0u32; size];

    let mut i = 0;
    while i < size {
        // Decode MTF index
        let ctxid = CTXIDS.saturating_sub(1).min(mtfno);
        let mut cp: usize = 0;
        let mut found = false;

        // Check positions 0..1 in MTF
        for im in 0..2usize {
            if zp.decode(cp + ctxid) != 0 {
                mtfno = im;
                dat[i] = mtf[mtfno];
                found = true;
                break;
            }
            cp += CTXIDS;
        }

        if !found {
            // Decode bit-length then value
            let mut bits: u32 = 1;
            let mut imtf: usize = 2;
            let mut found2 = false;
            while bits < 8 {
                if zp.decode(cp) != 0 {
                    let mut nn: u32 = 1;
                    let mm: u32 = 1 << bits;
                    while nn < mm {
                        let b = zp.decode(cp + nn as usize);
                        nn = (nn << 1) | b;
                    }
                    mtfno = imtf + (nn - mm) as usize;
                    dat[i] = mtf[mtfno];
                    found2 = true;
                    break;
                }
                cp += imtf;
                imtf <<= 1;
                bits += 1;
            }
            if !found2 {
                // Marker byte
                mtfno = 256;
                dat[i] = 0;
                markerpos = i as i32;
                i += 1;
                if zp.error { return None; }
                continue;
            }
        }

        if zp.error { return None; }

        // Rotate MTF
        fadd = fadd.wrapping_add(fadd >> fshift);
        if fadd > 0x10000000 {
            fadd >>= 24;
            for f in freq.iter_mut() {
                *f >>= 24;
            }
        }

        let fc = fadd + if mtfno < FREQMAX { freq[mtfno] } else { 0 };
        let mut k = mtfno;
        while k >= FREQMAX {
            mtf[k] = mtf[k - 1];
            k -= 1;
        }
        while k > 0 && fc >= freq[k - 1] {
            mtf[k] = mtf[k - 1];
            freq[k] = freq[k - 1];
            k -= 1;
        }
        mtf[k] = dat[i];
        freq[k] = fc;

        i += 1;
    }

    // Validate marker position
    if markerpos < 1 || markerpos as usize >= size {
        return None;
    }
    let marker = markerpos as usize;

    // BWT inverse
    let mut count = [0usize; 256];
    let mut posn = vec![0u32; size];

    for ii in 0..marker {
        let c = dat[ii] as usize;
        posn[ii] = ((c as u32) << 24) | (count[c] as u32 & 0xffffff);
        count[c] += 1;
    }
    // skip marker entry (posn[marker] = 0)
    for ii in (marker + 1)..size {
        let c = dat[ii] as usize;
        posn[ii] = ((c as u32) << 24) | (count[c] as u32 & 0xffffff);
        count[c] += 1;
    }

    // Compute prefix sums of count
    let mut last: usize = 1;
    for c in 0..256 {
        let tmp = count[c];
        count[c] = last;
        last += tmp;
    }

    // Undo BWT sort transform
    let out_len = size - 1;
    let mut out = vec![0u8; out_len];
    let mut idx: usize = 0;
    let mut pos = out_len;
    while pos > 0 {
        let n = posn[idx];
        let c = (n >> 24) as usize;
        pos -= 1;
        out[pos] = c as u8;
        idx = count[c] + (n & 0xffffff) as usize;
    }

    if idx != marker {
        return None;
    }

    Some(out)
}
