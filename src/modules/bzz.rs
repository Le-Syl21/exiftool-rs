//! Image::ExifTool::BZZ (Image/ExifTool/BZZ.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Utility to decode BZZ compressed data (DjVu).
//!
//! The Perl runs under `use integer`: every operation is perl's signed IV
//! arithmetic, which is Rust's wrapping `i64` arithmetic (with perl's
//! rules for shifts), so the decoder works on `i64` values. The BZZ object
//! stays a Perl hash (`$self`); the subs that use it read its members into
//! `Zp` on entry and write them back before returning, so the hash holds
//! what the Perl would leave in it. The default tables (@default_ztable_*)
//! stay Perl data, read from the file lexicals.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::bless;
use crate::perl::Value;
use crate::pl::{self, arg, n, ret};
use crate::runtime;

const MODULE: &str = "Image::ExifTool::BZZ";

// constants
const FREQMAX_C: i64 = 4;
const CTXIDS_C: i64 = 3;
const MAXBLOCK_C: i64 = 4096;

pub fn freqmax(_it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(n(FREQMAX_C))
}

pub fn ctxids(_it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(n(CTXIDS_C))
}

pub fn maxblock(_it: &mut Interp, _a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(n(MAXBLOCK_C))
}

/// `$x << $n` under `use integer`
fn shl(x: i64, n: i64) -> i64 {
    if n < 0 {
        return shr(x, -n);
    }
    if n >= 64 {
        return 0;
    }
    ((x as u64) << n) as i64
}

/// `$x >> $n` under `use integer` (arithmetic shift)
fn shr(x: i64, n: i64) -> i64 {
    if n < 0 {
        return shl(x, -n);
    }
    if n >= 64 {
        return if x < 0 { -1 } else { 0 };
    }
    x >> n
}

/// `$$a[$i]` of an integer array (undef reads as 0)
fn at(v: &[i64], i: i64) -> i64 {
    let i = if i < 0 { i + v.len() as i64 } else { i };
    if i < 0 {
        return 0;
    }
    v.get(i as usize).copied().unwrap_or(0)
}

/// `$$a[$i] = $x`
fn set_at(v: &mut Vec<i64>, i: i64, x: i64) {
    let i = if i < 0 { i + v.len() as i64 } else { i };
    if i < 0 {
        return;
    }
    let i = i as usize;
    if i >= v.len() {
        v.resize(i + 1, 0);
    }
    v[i] = x;
}

fn int_array(v: &Value) -> Vec<i64> {
    v.items().iter().map(|x| x.to_i64()).collect()
}

/// The members of a BZZ object that the decoder uses.
struct Zp {
    a: i64,
    code: i64,
    fence: i64,
    buffer: i64,
    scount: i64,
    delay: i64,
    byte: i64,
    pos: i64,
    data_len: i64,
    /// `$$self{size}` (None while it doesn't exist)
    size: Option<i64>,
    data: std::rc::Rc<[u8]>,
    ffzt: Vec<i64>,
    p: Vec<i64>,
    m: Vec<i64>,
    up: Vec<i64>,
    dn: Vec<i64>,
    ctx: Vec<i64>,
}

impl Zp {
    fn load(s: &Value) -> Zp {
        let size = s.get("size");
        Zp {
            a: s.get("a").to_i64(),
            code: s.get("code").to_i64(),
            fence: s.get("fence").to_i64(),
            buffer: s.get("buffer").to_i64(),
            scount: s.get("scount").to_i64(),
            delay: s.get("delay").to_i64(),
            byte: s.get("byte").to_i64(),
            pos: s.get("Pos").to_i64(),
            data_len: s.get("DataLen").to_i64(),
            size: if size.is_undef() { None } else { Some(size.to_i64()) },
            data: s.get("DataPt").deref().to_bytes(),
            ffzt: int_array(&s.get("ffzt")),
            p: int_array(&s.get("p")),
            m: int_array(&s.get("m")),
            up: int_array(&s.get("up")),
            dn: int_array(&s.get("dn")),
            ctx: int_array(&s.get("ctx")),
        }
    }

    /// Write the members the decoder changes back to the object.
    fn store(&self, s: &Value) {
        s.set("a", n(self.a));
        s.set("code", n(self.code));
        s.set("fence", n(self.fence));
        s.set("buffer", n(self.buffer));
        s.set("scount", n(self.scount));
        s.set("delay", n(self.delay));
        s.set("byte", n(self.byte));
        s.set("Pos", n(self.pos));
        if let Some(sz) = self.size {
            s.set("size", n(sz));
        }
        let cx = s.get("ctx");
        if let Value::ArrayRef(arr) = &cx {
            let mut arr = arr.borrow_mut();
            for (i, x) in self.ctx.iter().enumerate() {
                if i < arr.len() {
                    arr[i] = n(*x);
                } else {
                    arr.push(n(*x));
                }
            }
        }
    }
}

//------------------------------------------------------------------------------
// New - create new BZZ object
// Inputs: 0) reference to BZZ object or BZZ class name
// Returns: blessed BZZ object ref
fn new_obj(that: &Value) -> Value {
    // (local $_)
    let r = pl::ref_of(that);
    let class = if !r.is_empty() {
        pl::s(&r)
    } else if that.truthy() {
        that.clone()
    } else {
        pl::s("Image::ExifTool::BZZ")
    };
    let h = Value::new_hash();
    bless(&h, &class);
    h
}

pub fn new(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(new_obj(&arg(&a, 0)))
}

//------------------------------------------------------------------------------
// Initialize BZZ object
// Inputs: 0) BZZ object ref, 1) data ref, 2) true for DjVu compatibility
fn init_obj(it: &mut Interp, self_: &Value, data_pt: &Value, djvucompat: &Value) -> ER<()> {
    // Create machine independent ffz table
    let mut ffzt: Vec<i64> = Vec::new();
    for i in 0..256i64 {
        set_at(&mut ffzt, i, 0);
        let mut j = i;
        while j & 0x80 != 0 {
            let x = at(&ffzt, i) + 1;
            set_at(&mut ffzt, i, x);
            j = shl(j, 1);
        }
    }
    // Initialize table
    let p = int_array(&runtime::lex_array(it, MODULE, "default_ztable_p"));
    let m = int_array(&runtime::lex_array(it, MODULE, "default_ztable_m"));
    let up = int_array(&runtime::lex_array(it, MODULE, "default_ztable_up"));
    let default_dn = int_array(&runtime::lex_array(it, MODULE, "default_ztable_dn"));
    let mut dn = default_dn.clone();
    // Patch table (and lose DjVu compatibility)
    if !djvucompat.truthy() {
        for j in 0..256i64 {
            let mut a = (0x10000 - at(&p, j)) & 0xffff;
            while a >= 0x8000 {
                a = shl(a, 1) & 0xffff;
            }
            if at(&m, j) > 0 && a + at(&p, j) >= 0x8000 && a >= at(&m, j) {
                set_at(&mut dn, j, at(&default_dn, at(&default_dn, j)));
            }
        }
    }
    let arr = |v: &[i64]| Value::new_array(v.iter().map(|x| n(*x)).collect());
    self_.set("ffzt", arr(&ffzt));
    self_.set("p", arr(&p));
    self_.set("m", arr(&m));
    self_.set("up", arr(&up));
    self_.set("dn", arr(&dn));
    self_.set("ctx", arr(&[0; 300]));
    self_.set("DataPt", data_pt.clone());
    self_.set("Pos", n(0));
    pl::at(162);
    let data = pl::sderef(data_pt)?.to_bytes();
    self_.set("DataLen", n(data.len() as i64));
    self_.set("a", n(0));
    self_.set("buffer", n(0));
    self_.set("fence", n(0));
    self_.set("blocksize", n(0));
    // Read first 16 bits of code
    let code;
    if data.len() >= 2 {
        code = ((data[0] as i64) << 8) | data[1] as i64;
        self_.set("Pos", n(2));
    } else if !data.is_empty() {
        code = shl(data[0] as i64, 8) | 0xff;
        self_.set("Pos", n(1));
    } else {
        code = 0xffff;
    }
    self_.set("code", n(code));
    self_.set("byte", n(code & 0xff));
    // Preload buffer
    self_.set("delay", n(25));
    self_.set("scount", n(0));
    // Compute initial fence
    self_.set("fence", n(if code >= 0x8000 { 0x7fff } else { code }));
    Ok(())
}

pub fn init(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    init_obj(it, &arg(&a, 0), &arg(&a, 1), &arg(&a, 2))?;
    // (returns the value of the last statement: the fence)
    let _ = want;
    ret(arg(&a, 0).get("fence"))
}

//------------------------------------------------------------------------------
// Decode data block
// Inputs: 0) optional BZZ object ref, 1) optional data ref
// Returns: decoded data or undefined on error
// Notes: If called without a data ref, an input BZZ object ref must be given and
//        the BZZ object must have been initialized by a previous call to Init()
pub fn decode(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    // Decode input stream
    // (local $_)
    let mut a = a.into_iter();
    let first = a.next().unwrap_or_default();
    let self_;
    let mut data_pt;
    if first.is_ref() && pl::call(it, "UNIVERSAL::isa", vec![first.clone(), pl::s("Image::ExifTool::BZZ")])?.truthy() {
        self_ = first;
        data_pt = a.next().unwrap_or_default();
    } else {
        self_ = new_obj(&pl::s("Image::ExifTool::BZZ"));
        data_pt = first;
    }
    if data_pt.truthy() {
        init_obj(it, &self_, &data_pt, &n(1))?;
    } else {
        data_pt = self_.get("DataPt");
        if !data_pt.truthy() {
            return pl::ret_undef();
        }
    }
    let _ = data_pt;
    let mut z = Zp::load(&self_);
    let r = decode_zp(&mut z, &self_);
    z.store(&self_);
    ret(r)
}

fn decode_zp(z: &mut Zp, self_: &Value) -> Value {
    // Decode block size
    let mut nn: i64 = 1;
    let mm: i64 = 1 << 24;
    while nn < mm {
        let za = 0x8000 + shr(z.a, 1);
        let b = decode_sub_zp(z, za, None);
        nn = shl(nn, 1) | b;
    }
    z.size = Some(nn - mm);

    if z.size == Some(0) {
        return pl::s("");
    }
    if z.size.unwrap_or(0) > MAXBLOCK_C * 1024 {
        return Value::Undef;
    }
    // Allocate
    if self_.get("blocksize").to_i64() < z.size.unwrap_or(0) {
        self_.set("blocksize", n(z.size.unwrap_or(0)));
    }
    // Decode Estimation Speed
    let mut fshift: i64 = 0;
    let za = 0x8000 + shr(z.a, 1);
    if decode_sub_zp(z, za, None) != 0 {
        fshift += 1;
        let za = 0x8000 + shr(z.a, 1);
        if decode_sub_zp(z, za, None) != 0 {
            fshift += 1;
        }
    }
    // Prepare Quasi MTF
    let mut mtf: Vec<i64> = (0..256).collect();
    let mut freq: Vec<i64> = vec![0; FREQMAX_C as usize];
    let mut fadd: i64 = 4;
    // Decode
    let mut mtfno: i64 = 3;
    let mut markerpos: i64 = -1;
    let mut dat: Vec<i64> = Vec::new();
    let mut i: i64 = 0;
    'byte: while i < z.size.unwrap_or(0) {
        // dummy loop avoids use of "goto" statement
        'dummy: loop {
            let mut ctxid = CTXIDS_C - 1;
            if ctxid > mtfno {
                ctxid = mtfno;
            }
            let mut cp: i64 = 0;
            let mut imtf: i64 = 0;
            while imtf < 2 {
                if decoder_zp(z, Some(cp + ctxid)) != 0 {
                    mtfno = imtf;
                    set_at(&mut dat, i, at(&mtf, mtfno));
                    // (a "goto" here could give a segfault due to a Perl bug)
                    break 'dummy; // do rotation
                }
                cp += CTXIDS_C;
                imtf += 1;
            }
            let mut bits: i64 = 1;
            while bits < 8 {
                if decoder_zp(z, Some(cp)) != 0 {
                    let mut nn: i64 = 1;
                    let mm = shl(1, bits);
                    while nn < mm {
                        let b = decoder_zp(z, Some(cp + nn));
                        nn = shl(nn, 1) | b;
                    }
                    mtfno = imtf + nn - mm;
                    set_at(&mut dat, i, at(&mtf, mtfno));
                    break 'dummy; // do rotation
                }
                cp += imtf;
                bits += 1;
                imtf = shl(imtf, 1);
            }
            mtfno = 256;
            set_at(&mut dat, i, 0);
            markerpos = i;
            i += 1;
            continue 'byte; // no rotation necessary
        }
        // Rotate mtf according to empirical frequencies (new!)
        // Adjust frequencies for overflow
        fadd = fadd.wrapping_add(shr(fadd, fshift));
        if fadd > 0x10000000 {
            fadd = shr(fadd, 24);
            for f in freq.iter_mut() {
                *f = shr(*f, 24);
            }
        }
        // Relocate new char according to new freq
        let mut fc = fadd;
        if mtfno < FREQMAX_C {
            fc = fc.wrapping_add(at(&freq, mtfno));
        }
        let mut k = mtfno;
        while k >= FREQMAX_C {
            let x = at(&mtf, k - 1);
            set_at(&mut mtf, k, x);
            k -= 1;
        }
        while k > 0 && fc >= at(&freq, k - 1) {
            let x = at(&mtf, k - 1);
            set_at(&mut mtf, k, x);
            let f = at(&freq, k - 1);
            set_at(&mut freq, k, f);
            k -= 1;
        }
        let d = at(&dat, i);
        set_at(&mut mtf, k, d);
        set_at(&mut freq, k, fc);
        // when "goto" was used, Perl 5.8.6 could segfault here
        // unless "next" was explicitly stated
        i += 1;
    }
    //
    // Reconstruct the string
    //
    let size = z.size.unwrap_or(0);
    if markerpos < 1 || markerpos >= size {
        return Value::Undef;
    }
    // Allocate pointers
    // Prepare count buffer
    let mut count: Vec<i64> = vec![0; 256];
    let mut posn: Vec<i64> = Vec::new();
    // Fill count buffer
    // (no integer: the values here are small and positive, so UV and IV
    // operations agree)
    i = 0;
    while i < markerpos {
        let c = at(&dat, i);
        let cnt = at(&count, c);
        set_at(&mut count, c, cnt + 1);
        set_at(&mut posn, i, shl(c, 24) | (cnt & 0xffffff));
        i += 1;
    }
    set_at(&mut posn, i, 0); // (initialize marker entry just to be safe)
    i += 1;
    while i < size {
        let c = at(&dat, i);
        let cnt = at(&count, c);
        set_at(&mut count, c, cnt + 1);
        set_at(&mut posn, i, shl(c, 24) | (cnt & 0xffffff));
        i += 1;
    }
    // Compute sorted char positions
    let mut last: i64 = 1;
    for i in 0..256 {
        let tmp = count[i];
        count[i] = last;
        last += tmp;
    }
    // Undo the sort transform
    i = 0;
    last = size - 1;
    while last > 0 {
        let nn = at(&posn, i);
        // (no integer)
        let c = ((nn as u64) >> 24) as i64;
        last -= 1;
        set_at(&mut dat, last, c);
        i = at(&count, c) + (nn & 0xffffff);
    }
    // Final check and return decoded data
    if i != markerpos {
        return Value::Undef;
    }
    dat.pop(); // (last byte isn't real)
    Value::bytes(dat.iter().map(|x| *x as u8).collect::<Vec<u8>>())
}

//------------------------------------------------------------------------------
// Inputs: 0) BZZ object ref, 1) ctx
// Returns: decoded bit
/// `ctx` is the index in `$$self{ctx}` of the element passed (aliased).
fn decoder_zp(z: &mut Zp, idx: Option<i64>) -> i64 {
    let ctx = idx.map(|i| at(&z.ctx, i)).unwrap_or(0);
    let zz = z.a + at(&z.p, ctx);
    if zz <= z.fence {
        z.a = zz;
        return ctx & 1;
    }
    // must pass $_[1] so subroutine can modify value (darned C++ pass-by-reference!)
    let mut c = ctx;
    let r = decode_sub_zp(z, zz, idx.map(|_| &mut c));
    if let Some(i) = idx {
        set_at(&mut z.ctx, i, c);
    }
    r
}

/// `decoder` called from Perl: the context is a value, written back
/// through `$_[1]`.
pub fn decoder(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let self_ = arg(&a, 0);
    let ctx_v = arg(&a, 1);
    let mut z = Zp::load(&self_);
    let ctx = ctx_v.to_i64();
    let zz = z.a + at(&z.p, ctx);
    if zz <= z.fence {
        z.a = zz;
        z.store(&self_);
        return ret(n(ctx & 1));
    }
    let mut c = ctx;
    let mut changed = false;
    let r = decode_sub_core(&mut z, zz, if ctx_v.is_undef() { None } else { Some((&mut c, &mut changed)) });
    z.store(&self_);
    if changed {
        it.set_arg(1, n(c))?;
    }
    ret(n(r))
}

fn decode_sub_zp(z: &mut Zp, zz: i64, ctx: Option<&mut i64>) -> i64 {
    let mut changed = false;
    decode_sub_core(z, zz, ctx.map(|c| (c, &mut changed)))
}

//------------------------------------------------------------------------------
// Inputs: 0) BZZ object ref, 1) z, 2) ctx (or undef)
// Returns: decoded bit
fn decode_sub_core(zp: &mut Zp, mut z: i64, ctx: Option<(&mut i64, &mut bool)>) -> i64 {
    // ensure that we have at least 16 bits of encoded data available
    if zp.scount < 16 {
        // preload byte by byte until we have at least 24 bits
        while zp.scount <= 24 {
            if zp.pos < zp.data_len {
                zp.byte = zp.data.get(zp.pos as usize).map_or(0, |b| *b as i64);
                zp.pos += 1;
            } else {
                zp.byte = 0xff;
                zp.delay -= 1;
                if zp.delay < 1 {
                    // setting size to zero forces error return from Decode()
                    zp.size = Some(0);
                    return 0;
                }
            }
            zp.buffer = shl(zp.buffer, 8) | zp.byte;
            zp.scount += 8;
        }
    }
    // Save bit
    let mut a = zp.a;
    let mut bit;
    let mut code;
    if let Some((c, _)) = &ctx {
        bit = **c & 1;
        // Avoid interval reversion
        let d = 0x6000 + shr(z.wrapping_add(a), 2);
        if z > d {
            z = d;
        }
    } else {
        bit = 0;
    }
    // Test MPS/LPS
    code = zp.code;
    if z > code {
        bit ^= 1;
        // LPS branch
        z = 0x10000 - z;
        a += z;
        code += z;
        // LPS adaptation
        if let Some((c, changed)) = ctx {
            *c = at(&zp.dn, *c);
            *changed = true;
        }
        // LPS renormalization
        let sft = if a >= 0xff00 { at(&zp.ffzt, a & 0xff) + 8 } else { at(&zp.ffzt, shr(a, 8) & 0xff) };
        zp.scount -= sft;
        zp.a = shl(a, sft) & 0xffff;
        code = (shl(code, sft) & 0xffff) | (shr(zp.buffer, zp.scount) & (shl(1, sft) - 1));
    } else {
        // MPS adaptation
        if let Some((c, changed)) = ctx {
            if a >= at(&zp.m, *c) {
                *c = at(&zp.up, *c);
                *changed = true;
            }
        }
        // MPS renormalization
        zp.scount -= 1;
        zp.a = shl(z, 1) & 0xffff;
        code = (shl(code, 1) & 0xffff) | (shr(zp.buffer, zp.scount) & 1);
    }
    // Adjust fence and save new code
    zp.fence = if code >= 0x8000 { 0x7fff } else { code };
    zp.code = code;
    bit
}

/// `decode_sub` called from Perl: the context (if defined) is written
/// back through `$_[2]`.
pub fn decode_sub(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let self_ = arg(&a, 0);
    let zz = arg(&a, 1).to_i64();
    let ctx_v = arg(&a, 2);
    let mut z = Zp::load(&self_);
    let mut c = ctx_v.to_i64();
    let mut changed = false;
    let r = decode_sub_core(&mut z, zz, if ctx_v.is_undef() { None } else { Some((&mut c, &mut changed)) });
    z.store(&self_);
    if changed {
        it.set_arg(2, n(c))?;
    }
    ret(n(r))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::BZZ::FREQMAX", freqmax);
    it.define_native("Image::ExifTool::BZZ::CTXIDS", ctxids);
    it.define_native("Image::ExifTool::BZZ::MAXBLOCK", maxblock);
    it.define_native("Image::ExifTool::BZZ::new", new);
    it.define_native("Image::ExifTool::BZZ::Init", init);
    it.define_native("Image::ExifTool::BZZ::Decode", decode);
    it.define_native("Image::ExifTool::BZZ::decoder", decoder);
    it.define_native("Image::ExifTool::BZZ::decode_sub", decode_sub);
}
