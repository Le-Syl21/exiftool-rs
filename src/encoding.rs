//! Text encoding utilities for metadata decoding.
//!
//! Many file formats store text metadata in Latin-1 (ISO 8859-1) or other
//! non-UTF-8 encodings. These helpers provide correct decoding instead of
//! the lossy `String::from_utf8_lossy()` which silently replaces bytes
//! >= 0x80 with U+FFFD.

/// Decode bytes as Latin-1 (ISO 8859-1) to String.
///
/// Each byte maps directly to its Unicode code point (U+0000–U+00FF),
/// which is the correct mapping for ISO 8859-1.
pub fn decode_latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// Try decoding as UTF-8 first; fall back to Latin-1 if invalid.
///
/// This matches Perl ExifTool's behavior for fields that are historically
/// Latin-1 but may contain valid UTF-8 in modern files.
pub fn decode_utf8_or_latin1(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => decode_latin1(bytes),
    }
}

/// Encode a UTF-8 string as Latin-1 (ISO 8859-1) bytes — the inverse of
/// [`decode_latin1`].
///
/// Each code point U+0000–U+00FF maps to a single byte; anything above is
/// not representable in Latin-1 and is substituted with `?`, matching Perl
/// ExifTool's default behaviour when writing a character the IPTC internal
/// charset can't hold (the alternative is declaring `CodedCharacterSet` =
/// UTF8). Without this, a UTF-8 `&str` written straight to an IPTC-IIM
/// dataset double-encodes: `í` (U+00ED → UTF-8 `C3 AD`) reads back as `Ã­`
/// under the default Latin-1 IPTC charset.
pub fn encode_latin1(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| {
            let cp = c as u32;
            if cp <= 0xFF {
                cp as u8
            } else {
                b'?'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_latin1_roundtrip() {
        // Inverse of decode_latin1 across the representable range.
        assert_eq!(encode_latin1("hello"), b"hello");
        assert_eq!(encode_latin1("éüñ"), vec![0xE9, 0xFC, 0xF1]);
        assert_eq!(encode_latin1("©®ö"), vec![0xA9, 0xAE, 0xF6]);
        // The reported case: "Martín" → 'í' is 0xED, one byte.
        assert_eq!(
            encode_latin1("Martín"),
            vec![b'M', b'a', b'r', b't', 0xED, b'n']
        );
    }

    #[test]
    fn test_encode_latin1_substitutes_unrepresentable() {
        // Beyond Latin-1 (e.g. Cyrillic, emoji) → '?'.
        assert_eq!(encode_latin1("Пример"), b"??????");
        assert_eq!(encode_latin1("a😀b"), b"a?b");
    }

    #[test]
    fn test_encode_decode_latin1_inverse() {
        let s = "Àéîõü©";
        assert_eq!(decode_latin1(&encode_latin1(&s)), s);
    }

    #[test]
    fn test_decode_latin1_ascii() {
        assert_eq!(decode_latin1(b"hello"), "hello");
    }

    #[test]
    fn test_decode_latin1_high_bytes() {
        // 0xE9 = é, 0xFC = ü, 0xF1 = ñ
        assert_eq!(decode_latin1(&[0xE9, 0xFC, 0xF1]), "éüñ");
    }

    #[test]
    fn test_decode_latin1_full_range() {
        // 0xA9 = ©, 0xAE = ®, 0xF6 = ö
        assert_eq!(decode_latin1(&[0xA9, 0xAE, 0xF6]), "©®ö");
    }

    #[test]
    fn test_decode_utf8_or_latin1_valid_utf8() {
        assert_eq!(decode_utf8_or_latin1("café".as_bytes()), "café");
    }

    #[test]
    fn test_decode_utf8_or_latin1_latin1_fallback() {
        // 0xE9 alone is invalid UTF-8 but valid Latin-1 for 'é'
        assert_eq!(decode_utf8_or_latin1(&[0x63, 0x61, 0x66, 0xE9]), "café");
    }

    #[test]
    fn test_decode_utf8_or_latin1_pure_ascii() {
        assert_eq!(decode_utf8_or_latin1(b"hello"), "hello");
    }

    #[test]
    fn test_decode_utf8_or_latin1_empty() {
        assert_eq!(decode_utf8_or_latin1(b""), "");
        assert_eq!(decode_latin1(b""), "");
    }
}
