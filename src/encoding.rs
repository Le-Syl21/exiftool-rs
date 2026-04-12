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

#[cfg(test)]
mod tests {
    use super::*;

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
