//! Image::ExifTool::AES (Image/ExifTool/AES.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! AES encryption with cipher-block chaining.
//!
//! The file's `my` variables ($seeded, $nr, @cbc, @state, @RoundKey) are
//! only ever touched by the subs of this file, which are all ported, so
//! they live here in Rust (`St`). Their elements are the small unsigned
//! integers the Perl stores (bytes, or undef which every use reads as 0),
//! and the bit operations on them are perl's UV operations, so plain
//! `u64` arithmetic gives the same values. The constant tables (@sbox,
//! @rsbox, @rcon) stay Perl data, read once from the file lexicals.

use std::cell::RefCell;

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{perl_eval, perl_eval1};
use crate::perl::{ops, Value};
use crate::pl::{self, arg, n, ret};
use crate::runtime;

const MODULE: &str = "Image::ExifTool::AES";

#[derive(Default)]
struct St {
    seeded: bool,
    /// `$nr` (undef reads as 0)
    nr: i64,
    /// `@cbc`
    cbc: Vec<u64>,
    /// `@state`
    state: [[u64; 4]; 4],
    /// `@RoundKey`
    round_key: Vec<u64>,
    sbox: Vec<u64>,
    rsbox: Vec<u64>,
    rcon: Vec<u64>,
}

thread_local! {
    static ST: RefCell<St> = RefCell::new(St::default());
}

/// `$array[$i]` on one of the Perl-held tables (undef as 0, negative
/// indices from the end, as perl).
fn at(v: &[u64], i: i64) -> u64 {
    let i = if i < 0 { i + v.len() as i64 } else { i };
    if i < 0 {
        return 0;
    }
    v.get(i as usize).copied().unwrap_or(0)
}

/// `$array[$i] = $x` (extending the array as perl does).
fn set_at(v: &mut Vec<u64>, i: i64, x: u64) {
    let i = if i < 0 { i + v.len() as i64 } else { i };
    if i < 0 {
        return; // (perl dies "Modification of non-creatable array value"; never reached)
    }
    let i = i as usize;
    if i >= v.len() {
        v.resize(i + 1, 0);
    }
    v[i] = x;
}

/// Load the constant tables from the file lexicals the first time.
fn load_tables(it: &mut Interp) {
    if ST.with(|s| !s.borrow().sbox.is_empty()) {
        return;
    }
    let get = |it: &mut Interp, name: &str| -> Vec<u64> { runtime::lex_array(it, MODULE, name).items().iter().map(ops::to_uv).collect() };
    let (sbox, rsbox, rcon) = (get(it, "sbox"), get(it, "rsbox"), get(it, "rcon"));
    ST.with(|s| {
        let mut s = s.borrow_mut();
        s.sbox = sbox;
        s.rsbox = rsbox;
        s.rcon = rcon;
    });
}

fn with_st<T>(it: &mut Interp, f: impl FnOnce(&mut St) -> T) -> T {
    load_tables(it);
    ST.with(|s| f(&mut s.borrow_mut()))
}

/// The bytes of a string (`unpack 'C*', $str`).
fn unpack_c(v: &Value) -> Vec<u64> {
    v.to_bytes().iter().map(|b| *b as u64).collect()
}

/// `pack 'C*', LIST` (values are bytes here).
fn pack_c(v: &[u64]) -> Value {
    Value::bytes(v.iter().map(|x| *x as u8).collect::<Vec<u8>>())
}

//------------------------------------------------------------------------------
// This function produces 4*($nr+1) round keys.
// The round keys are used in each round to encrypt the states.
// Inputs: 0) key string (must be 16, 24 or 32 bytes long)
fn key_expansion_st(s: &mut St, key: &Value) -> ER<()> {
    let key_b = unpack_c(key); // convert the key into a byte array
    let nk = (key_b.len() / 4) as i64; // number of 32-bit words in the key
    s.nr = nk + 6; // number of rounds

    // temporary variables (all unsigned characters)
    let mut temp = [0u64; 4];

    // The first round key is the key itself.
    let mut i: i64 = 0;
    while i < nk {
        for j in 0..4 {
            let x = at(&key_b, i * 4 + j);
            set_at(&mut s.round_key, i * 4 + j, x);
        }
        i += 1;
    }
    // All other round keys are found from the previous round keys.
    while i < (4 * (s.nr + 1)) {
        for j in 0..4 {
            temp[j as usize] = at(&s.round_key, (i - 1) * 4 + j);
        }
        // ($i % $nk)
        pl::at(115);
        let m = pl::modulo(&n(i), &n(nk))?.to_i64();
        if m == 0 {
            // rotate the 4 bytes in a word to the left once
            // [a0,a1,a2,a3] becomes [a1,a2,a3,a0]
            temp = [temp[1], temp[2], temp[3], temp[0]];

            // take a four-byte input word and apply the S-box
            // to each of the four bytes to produce an output word.
            for t in temp.iter_mut() {
                *t = at(&s.sbox, *t as i64);
            }

            temp[0] ^= at(&s.rcon, i / nk);
        } else if nk > 6 && m == 4 {
            for t in temp.iter_mut() {
                *t = at(&s.sbox, *t as i64);
            }
        }
        for j in 0..4 {
            let x = at(&s.round_key, (i - nk) * 4 + j) ^ temp[j as usize];
            set_at(&mut s.round_key, i * 4 + j, x);
        }
        i += 1;
    }
    Ok(())
}

pub fn key_expansion(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let key = arg(&a, 0);
    with_st(it, |s| key_expansion_st(s, &key))?;
    pl::ret_nothing(want)
}

//------------------------------------------------------------------------------
// This function adds the round key to state.
// The round key is added to the state by an XOR function.
fn add_round_key_st(s: &mut St, round: i64) {
    for i in 0..4 {
        let k = round * 16 + i * 4;
        for j in 0..4 {
            s.state[j as usize][i as usize] ^= at(&s.round_key, k + j);
        }
    }
}

pub fn add_round_key(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let round = arg(&a, 0).to_i64();
    with_st(it, |s| add_round_key_st(s, round));
    pl::ret_nothing(want)
}

//------------------------------------------------------------------------------
// Substitute the values in the state matrix with values in an S-box
fn sub_bytes_st(s: &mut St) {
    for i in 0..4 {
        for j in 0..4 {
            s.state[i][j] = at(&s.sbox, s.state[i][j] as i64);
        }
    }
}

pub fn sub_bytes(it: &mut Interp, _a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    with_st(it, sub_bytes_st);
    pl::ret_nothing(want)
}

fn inv_sub_bytes_st(s: &mut St) {
    for i in 0..4 {
        for j in 0..4 {
            s.state[i][j] = at(&s.rsbox, s.state[i][j] as i64);
        }
    }
}

pub fn inv_sub_bytes(it: &mut Interp, _a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    with_st(it, inv_sub_bytes_st);
    pl::ret_nothing(want)
}

//------------------------------------------------------------------------------
// Shift the rows in the state to the left.
// Each row is shifted with different offset.
// Offset = Row number. So the first row is not shifted.
fn shift_rows_st(s: &mut St) {
    let st = &mut s.state;
    // rotate first row 1 columns to left
    st[1] = [st[1][1], st[1][2], st[1][3], st[1][0]];

    // rotate second row 2 columns to left
    st[2] = [st[2][2], st[2][3], st[2][0], st[2][1]];

    // rotate third row 3 columns to left
    st[3] = [st[3][3], st[3][0], st[3][1], st[3][2]];
}

pub fn shift_rows(it: &mut Interp, _a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    with_st(it, shift_rows_st);
    pl::ret_nothing(want)
}

fn inv_shift_rows_st(s: &mut St) {
    let st = &mut s.state;
    // rotate first row 1 columns to right
    st[1] = [st[1][3], st[1][0], st[1][1], st[1][2]];

    // rotate second row 2 columns to right
    st[2] = [st[2][2], st[2][3], st[2][0], st[2][1]];

    // rotate third row 3 columns to right
    st[3] = [st[3][1], st[3][2], st[3][3], st[3][0]];
}

pub fn inv_shift_rows(it: &mut Interp, _a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    with_st(it, inv_shift_rows_st);
    pl::ret_nothing(want)
}

//------------------------------------------------------------------------------
// Find the product of {02} and the argument to xtime modulo 0x1b
// Note: returns an integer which may need to be trimmed to 8 bits
fn xtime_u(x: u64) -> u64 {
    (x << 1) ^ (((x >> 7) & 1).wrapping_mul(0x1b))
}

pub fn xtime(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(ops::from_uv(xtime_u(ops::to_uv(&arg(&a, 0)))))
}

//------------------------------------------------------------------------------
// Multiply numbers in the field GF(2^8)
fn mult_u(x: u64, y: u64) -> u64 {
    ((y & 1).wrapping_mul(x))
        ^ ((y >> 1 & 1).wrapping_mul(xtime_u(x)))
        ^ ((y >> 2 & 1).wrapping_mul(xtime_u(xtime_u(x))))
        ^ ((y >> 3 & 1).wrapping_mul(xtime_u(xtime_u(xtime_u(x)))))
        ^ ((y >> 4 & 1).wrapping_mul(xtime_u(xtime_u(xtime_u(xtime_u(x))))))
}

pub fn mult(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    ret(ops::from_uv(mult_u(ops::to_uv(&arg(&a, 0)), ops::to_uv(&arg(&a, 1)))))
}

//------------------------------------------------------------------------------
// Mix the columns of the state matrix
fn mix_columns_st(s: &mut St) {
    let st = &mut s.state;
    for i in 0..4 {
        let t0 = st[0][i];
        let t2 = st[0][i] ^ st[1][i] ^ st[2][i] ^ st[3][i];
        let mut t1 = st[0][i] ^ st[1][i];
        t1 = xtime_u(t1) & 0xff;
        st[0][i] ^= t1 ^ t2;
        t1 = st[1][i] ^ st[2][i];
        t1 = xtime_u(t1) & 0xff;
        st[1][i] ^= t1 ^ t2;
        t1 = st[2][i] ^ st[3][i];
        t1 = xtime_u(t1) & 0xff;
        st[2][i] ^= t1 ^ t2;
        t1 = st[3][i] ^ t0;
        t1 = xtime_u(t1) & 0xff;
        st[3][i] ^= t1 ^ t2;
    }
}

pub fn mix_columns(it: &mut Interp, _a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    with_st(it, mix_columns_st);
    pl::ret_nothing(want)
}

fn inv_mix_columns_st(s: &mut St) {
    let st = &mut s.state;
    for i in 0..4 {
        let a = st[0][i];
        let b = st[1][i];
        let c = st[2][i];
        let d = st[3][i];
        st[0][i] = (mult_u(a, 0x0e) ^ mult_u(b, 0x0b) ^ mult_u(c, 0x0d) ^ mult_u(d, 0x09)) & 0xff;
        st[1][i] = (mult_u(a, 0x09) ^ mult_u(b, 0x0e) ^ mult_u(c, 0x0b) ^ mult_u(d, 0x0d)) & 0xff;
        st[2][i] = (mult_u(a, 0x0d) ^ mult_u(b, 0x09) ^ mult_u(c, 0x0e) ^ mult_u(d, 0x0b)) & 0xff;
        st[3][i] = (mult_u(a, 0x0b) ^ mult_u(b, 0x0d) ^ mult_u(c, 0x09) ^ mult_u(d, 0x0e)) & 0xff;
    }
}

pub fn inv_mix_columns(it: &mut Interp, _a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    with_st(it, inv_mix_columns_st);
    pl::ret_nothing(want)
}

//------------------------------------------------------------------------------
// Encrypt (Cipher) or decrypt (InvCipher) a block of data with CBC
// Inputs: 0) string to cipher (must be 16 bytes long)
// Returns: cipher'd string
fn cipher_st(s: &mut St, input: &[u64]) -> Value {
    // copy the input PlainText to state array and apply the CBC
    for i in 0..4 {
        for j in 0..4 {
            let k = (i * 4 + j) as i64;
            s.state[j][i] = at(input, k) ^ at(&s.cbc, k);
        }
    }

    // add the First round key to the state before starting the rounds
    add_round_key_st(s, 0);

    // there will be $nr rounds; the first $nr-1 rounds are identical
    let mut round = 1;
    loop {
        sub_bytes_st(s);
        shift_rows_st(s);
        if round < s.nr {
            mix_columns_st(s);
            add_round_key_st(s, round);
        } else {
            // MixColumns() is not used in the last round
            let nr = s.nr;
            add_round_key_st(s, nr);
            break;
        }
        round += 1;
    }

    // the encryption process is over
    // copy the state array to output array (and save for CBC)
    for i in 0..4 {
        for j in 0..4 {
            let x = s.state[j][i];
            set_at(&mut s.cbc, (i * 4 + j) as i64, x);
        }
    }
    pack_c(&s.cbc) // return packed ciphertext
}

pub fn cipher(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let input = unpack_c(&arg(&a, 0)); // unpack input plaintext
    ret(with_st(it, |s| cipher_st(s, &input)))
}

fn inv_cipher_st(s: &mut St, input: Vec<u64>) -> Value {
    let mut out: Vec<u64> = Vec::new();

    // copy the input CipherText to state array
    for i in 0..4 {
        for j in 0..4 {
            s.state[j][i] = at(&input, (i * 4 + j) as i64);
        }
    }

    // add the First round key to the state before starting the rounds
    let nr = s.nr;
    add_round_key_st(s, nr);

    // there will be $nr rounds; the first $nr-1 rounds are identical
    let mut round = nr - 1;
    loop {
        inv_shift_rows_st(s);
        inv_sub_bytes_st(s);
        add_round_key_st(s, round);
        // InvMixColumns() is not used in the last round
        if round <= 0 {
            break;
        }
        inv_mix_columns_st(s);
        round -= 1;
    }

    // copy the state array to output array and reverse the CBC
    for i in 0..4 {
        for j in 0..4 {
            let k = (i * 4 + j) as i64;
            set_at(&mut out, k, s.state[j][i] ^ at(&s.cbc, k));
        }
    }
    s.cbc = input; // update CBC for next block
    pack_c(&out) // return packed plaintext
}

pub fn inv_cipher(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let input = unpack_c(&arg(&a, 0)); // unpack input ciphertext
    ret(with_st(it, |s| inv_cipher_st(s, input)))
}

//------------------------------------------------------------------------------
// Encrypt/Decrypt using AES-CBC algorithm (with fixed 16-byte blocks)
// Inputs: 0) data reference (with leading 16-byte initialization vector when decrypting)
//         1) encryption key (16, 24 or 32 bytes for AES-128, AES-192 or AES-256)
//         2) encrypt flag (false for decryption, true with length 16 bytes to
//            encrypt using this as the CBC IV, or true with other length to
//            encrypt with a randomly-generated IV)
//         3) flag to disable padding
// Returns: error string, or undef on success
// Notes: encrypts/decrypts data in place (encrypted data returned with leading IV)
pub fn crypt(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (data_pt, key, mut encrypt, no_pad) = (arg(&a, 0), arg(&a, 1), arg(&a, 2), arg(&a, 3));
    load_tables(it);

    // validate key length
    let key_len = key.to_bytes().len();
    if !(key_len == 16 || key_len == 24 || key_len == 32) {
        return ret(Value::from(format!("Invalid AES key length ({key_len})")));
    }
    pl::at(348);
    let mut data = pl::sderef(&data_pt)?.to_bytes().to_vec();
    let part_len = data.len() % 16;
    let mut pos: usize;
    if encrypt.truthy() {
        if encrypt.to_bytes().len() == 16 {
            let iv = unpack_c(&encrypt);
            ST.with(|s| s.borrow_mut().cbc = iv);
        } else {
            // generate a random 16-byte CBC initialization vector
            if !ST.with(|s| s.borrow().seeded) {
                perl_eval(it, "srand(time() & ($$ + ($$<<15)))", MODULE, &[], Want::Void)?;
                ST.with(|s| s.borrow_mut().seeded = true);
            }
            for i in 0..16 {
                let r = perl_eval1(it, "int(rand(256))", MODULE, &[])?;
                ST.with(|s| set_at(&mut s.borrow_mut().cbc, i, ops::to_uv(&r)));
            }
            encrypt = ST.with(|s| pack_c(&s.borrow().cbc));
        }
        let mut d = encrypt.to_bytes().to_vec();
        d.extend_from_slice(&data);
        data = d; // add IV to the start of the data
        // add required padding so we can recover the
        // original string length after decryption
        // (padding bytes have value set to padding length)
        let pad_len = 16 - part_len;
        if !(pad_len == 16 && no_pad.truthy()) {
            data.extend(std::iter::repeat_n(pad_len as u8, pad_len));
        }
        pos = 16; // start encrypting at byte 16 (after the IV)
    } else if part_len != 0 {
        return ret(pl::s("Invalid AES ciphertext length"));
    } else if data.len() >= 32 {
        // take the CBC initialization vector from the start of the data
        let iv: Vec<u64> = data[..16].iter().map(|b| *b as u64).collect();
        ST.with(|s| s.borrow_mut().cbc = iv);
        data = data[16..].to_vec();
        pos = 0; // start decrypting from byte 0 (now that IV is removed)
    } else {
        pl::set_deref(&data_pt, pl::s(""))?; // empty text
        return pl::ret_undef();
    }
    // the KeyExpansion routine must be called before encryption
    with_st(it, |s| key_expansion_st(s, &key))?;

    // loop through the data and convert in blocks
    let data_len = data.len();
    let last = data_len as i64 - 16;
    let enc = encrypt.truthy();
    while pos as i64 <= last {
        // cipher this block
        let input: Vec<u64> = data[pos..pos + 16].iter().map(|b| *b as u64).collect();
        let out = with_st(it, |s| if enc { cipher_st(s, &input) } else { inv_cipher_st(s, input) });
        data[pos..pos + 16].copy_from_slice(&out.to_bytes());
        pos += 16;
    }
    if !(enc || no_pad.truthy()) {
        // remove padding if necessary (padding byte value gives length of padding)
        let pad_len = *data.last().unwrap_or(&0) as usize;
        if pad_len > 16 {
            pl::set_deref(&data_pt, Value::bytes(&data))?;
            return ret(pl::s("AES decryption error (invalid pad byte)"));
        }
        data.truncate(data_len - pad_len);
    }
    pl::set_deref(&data_pt, Value::bytes(&data))?;
    pl::ret_undef()
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::AES::KeyExpansion", key_expansion);
    it.define_native("Image::ExifTool::AES::AddRoundKey", add_round_key);
    it.define_native("Image::ExifTool::AES::SubBytes", sub_bytes);
    it.define_native("Image::ExifTool::AES::InvSubBytes", inv_sub_bytes);
    it.define_native("Image::ExifTool::AES::ShiftRows", shift_rows);
    it.define_native("Image::ExifTool::AES::InvShiftRows", inv_shift_rows);
    it.define_native("Image::ExifTool::AES::xtime", xtime);
    it.define_native("Image::ExifTool::AES::Mult", mult);
    it.define_native("Image::ExifTool::AES::MixColumns", mix_columns);
    it.define_native("Image::ExifTool::AES::InvMixColumns", inv_mix_columns);
    it.define_native("Image::ExifTool::AES::Cipher", cipher);
    it.define_native("Image::ExifTool::AES::InvCipher", inv_cipher);
    it.define_native("Image::ExifTool::AES::Crypt", crypt);
}
