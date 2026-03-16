//! Sony MakerNotes decryption.
//!
//! Two separate ciphers:
//! 1. Word-based LFSR cipher (SRF/SR2 IFDs) — 32-bit word XOR with keystream
//! 2. Byte substitution cipher (Tag 0x2010, 0x9050, 0x940x) — b³ mod 249

/// Sony LFSR word-based decryption (for SR2SubIFD, SRF2).
///
/// Decrypts `data` in-place using a 32-bit seed key.
/// Processes 4-byte (u32) words only; trailing bytes unmodified.
pub fn sony_decrypt_words(data: &mut [u8], start: usize, key: u32) {
    if start >= data.len() {
        return;
    }

    let slice = &mut data[start..];
    let words = slice.len() / 4;
    if words == 0 {
        return;
    }

    // Step 1: Generate initial 4 keystream words from seed key
    let mut pad = [0u32; 128]; // 0x80 entries
    let mut k = key;
    for i in 0..4 {
        let lo = (k & 0xFFFF).wrapping_mul(0x0EDD).wrapping_add(1);
        let hi = (k >> 16)
            .wrapping_mul(0x0EDD)
            .wrapping_add((k & 0xFFFF).wrapping_mul(0x02E9))
            .wrapping_add(lo >> 16);
        k = ((hi & 0xFFFF) << 16) | (lo & 0xFFFF);
        pad[i] = k;
    }

    // Step 2: Extend to 127-word keystream using LFSR
    pad[3] = (pad[3] << 1 | (pad[0] ^ pad[2]) >> 31) & 0xFFFFFFFF;
    for i in 4..0x7F {
        pad[i] = ((pad[i - 4] ^ pad[i - 2]) << 1 | (pad[i - 3] ^ pad[i - 1]) >> 31) & 0xFFFFFFFF;
    }

    // Step 3: XOR data words with evolving keystream
    let mut pi = 0x7F_usize;
    for j in 0..words {
        let offset = j * 4;
        let word = u32::from_be_bytes([
            slice[offset],
            slice[offset + 1],
            slice[offset + 2],
            slice[offset + 3],
        ]);

        // Evolve keystream: pad[i] = pad[i+1] ^ pad[i+65]
        let new_pad = pad[(pi + 1) & 0x7F] ^ pad[(pi + 65) & 0x7F];
        pad[pi & 0x7F] = new_pad;

        let decrypted = word ^ new_pad;
        let bytes = decrypted.to_be_bytes();
        slice[offset] = bytes[0];
        slice[offset + 1] = bytes[1];
        slice[offset + 2] = bytes[2];
        slice[offset + 3] = bytes[3];

        pi = pi.wrapping_add(1);
    }
}

/// Sony byte substitution cipher (for Tag2010, Tag9050, Tag940x).
///
/// Based on `c = b³ mod 249`. Bytes 249-255 pass through unchanged.
/// This is its own inverse (applying twice restores original).
pub fn sony_decipher(data: &mut [u8]) {
    for byte in data.iter_mut() {
        let b = *byte;
        if b < 249 {
            *byte = SONY_DECIPHER_TABLE[b as usize];
        }
        // Bytes 249-255 are unchanged
    }
}

/// Sony encipher (same algorithm, inverse table).
pub fn sony_encipher(data: &mut [u8]) {
    for byte in data.iter_mut() {
        let b = *byte;
        if b < 249 {
            *byte = SONY_ENCIPHER_TABLE[b as usize];
        }
    }
}

/// Decipher lookup table: maps encrypted byte → plaintext byte.
/// Generated from the ExifTool Sony.pm tr/// tables (Decipher function).
static SONY_DECIPHER_TABLE: [u8; 249] = {
    let mut table = [0u8; 249];
    // Build from c = b³ mod 249 relationship
    // The table is the inverse of the encipher: decipher[encipher[b]] = b
    // From ExifTool: the decipher tr/// maps each encrypted value to sequential 0x02..0xF8
    // We pre-compute it here

    // Direct from Perl: encrypted values in order for plaintext 0x02..0xF8
    let encrypted: [u8; 247] = [
        0x08, 0x1b, 0x40, 0x7d, 0xd8, 0x5e, 0x0e, 0xe7, 0x04, 0x56, 0xea, 0xcd, 0x05, 0x8a, 0x70, 0xb6,
        0x69, 0x88, 0x20, 0x30, 0xbe, 0xd7, 0x81, 0xbb, 0x92, 0x0c, 0x28, 0xec, 0x6c, 0xa0, 0x95, 0x51,
        0xd3, 0x2f, 0x5d, 0x6a, 0x5c, 0x39, 0x07, 0xc5, 0x87, 0x4c, 0x1a, 0xf0, 0xe2, 0xef, 0x24, 0x79,
        0x02, 0xb7, 0xac, 0xe0, 0x60, 0x2b, 0x47, 0xba, 0x91, 0xcb, 0x75, 0x8e, 0x23, 0x33, 0xc4, 0xe3,
        0x96, 0xdc, 0xc2, 0x4e, 0x7f, 0x62, 0xf6, 0x4f, 0x65, 0x45, 0xee, 0x74, 0xcf, 0x13, 0x38, 0x4b,
        0x52, 0x53, 0x54, 0x5b, 0x6e, 0x93, 0xd0, 0x32, 0xb1, 0x61, 0x41, 0x57, 0xa9, 0x44, 0x27, 0x58,
        0xdd, 0xc3, 0x10, 0xbc, 0xdb, 0x73, 0x83, 0x18, 0x31, 0xd4, 0x15, 0xe5, 0x5f, 0x7b, 0x46, 0xbf,
        0xf3, 0xe8, 0xa4, 0x2d, 0x82, 0xb0, 0xbd, 0xaf, 0x8c, 0x5a, 0x1f, 0xda, 0x9f, 0x6d, 0x4a, 0x3c,
        0x49, 0x77, 0xcc, 0x55, 0x11, 0x06, 0x3a, 0xb3, 0x7e, 0x9a, 0x14, 0xe4, 0x25, 0xc8, 0xe1, 0x76,
        0x86, 0x1e, 0x3d, 0xe9, 0x36, 0x1c, 0xa1, 0xd2, 0xb5, 0x50, 0xa2, 0xb8, 0x98, 0x48, 0xc7, 0x29,
        0x66, 0x8b, 0x9e, 0xa5, 0xa6, 0xa7, 0xae, 0xc1, 0xe6, 0x2a, 0x85, 0x0b, 0xb4, 0x94, 0xaa, 0x03,
        0x97, 0x7a, 0xab, 0x37, 0x1d, 0x63, 0x16, 0x35, 0xc6, 0xd6, 0x6b, 0x84, 0x2e, 0x68, 0x3f, 0xb2,
        0xce, 0x99, 0x19, 0x4d, 0x42, 0xf7, 0x80, 0xd5, 0x0a, 0x17, 0x09, 0xdf, 0xad, 0x72, 0x34, 0xf2,
        0xc0, 0x9d, 0x8f, 0x9c, 0xca, 0x26, 0xa8, 0x64, 0x59, 0x8d, 0x0d, 0xd1, 0xed, 0x67, 0x3e, 0x78,
        0x22, 0x3b, 0xc9, 0xd9, 0x71, 0x90, 0x43, 0x89, 0x6f, 0xf4, 0x2c, 0x0f, 0xa3, 0xf5, 0x12, 0xeb,
        0x9b, 0x21, 0x7c, 0xb9, 0xde, 0xf1, 0x00,
    ];

    // Build decipher table: for plaintext value p (0x02..0xF8),
    // encrypted[p-2] is the encrypted byte, so decipher[encrypted[p-2]] = p
    let mut i = 0;
    while i < 247 {
        let enc_byte = encrypted[i];
        let plain_byte = (i + 2) as u8;
        table[enc_byte as usize] = plain_byte;
        i += 1;
    }

    // Bytes 0 and 1 map to themselves (not in the translation range)
    table[0] = 0;
    table[1] = 1;

    table
};

/// Encipher lookup table (inverse of decipher).
static SONY_ENCIPHER_TABLE: [u8; 249] = {
    let mut table = [0u8; 249];
    let mut i = 0;
    while i < 249 {
        table[SONY_DECIPHER_TABLE[i] as usize] = i as u8;
        i += 1;
    }
    table
};
