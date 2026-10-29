//! Unicode::GCString (part of Unicode::LineBreak), as the exiftool script
//! uses it to align the columns of its plain output in a language with wide
//! characters: `Unicode::GCString->new($str)->columns`, on unicode-width
//! (pure Rust).
//!
//! The columns of a string are the sum of its characters' widths, under
//! Unicode::LineBreak's default (non East Asian) context: East Asian Wide
//! and Fullwidth characters take 2 columns, Ambiguous ones 1, combining
//! marks, format characters and controls none. Measured against
//! Unicode::LineBreak 2019.001 on every code point: 15244 of 1.1 million
//! differ, none of them in the text of ExifTool's languages. Most come from
//! its character database, Unicode 8.0: 10553 characters unassigned there
//! (1 column) and Wide since (CJK extensions, Tangut...) or the emoji that
//! became Wide in Unicode 9 (U+231A, U+1F600...: 1 there, 2 here); 4232
//! marks of no width here, 1 column there (marks added since Unicode 8, and
//! spacing vowel signs of Indic scripts that extend a grapheme, such as
//! U+09BE); 75 Hangul jamo and tone marks (U+1160 to U+11FF alone, U+302E:
//! 2 there, 0 here); a handful of others (U+17D8, U+FFF9...). The
//! characters that unicode-width counts as 1 column and Unicode::LineBreak
//! as none (U+2028, U+0600...) are set apart in `columns`.

use std::rc::Rc;

use unicode_width::UnicodeWidthChar;

use crate::interp::Interp;
use crate::perl::utf8;
use crate::perl::Value;

/// The code points of a perl string: its characters, or its bytes (as
/// Latin-1) for a byte string.
fn code_points(v: &Value) -> Vec<u32> {
    let (b, utf8) = v.str_form();
    if utf8 {
        utf8::chars(&b).collect()
    } else {
        b.iter().map(|&c| c as u32).collect()
    }
}

/// The columns of a string, as Unicode::GCString's `columns`.
pub fn columns(cps: &[u32]) -> usize {
    cps.iter()
        .map(|&c| match c {
            // of no width for Unicode::LineBreak, 1 column for unicode-width:
            // the line and paragraph separators, and the format characters
            // that prefix a number (Prepended_Concatenation_Mark, those of
            // Unicode 8)
            0x2028 | 0x2029 | 0x600..=0x605 | 0x6dd | 0x70f | 0x110bd => 0,
            _ => char::from_u32(c).and_then(|c| c.width()).unwrap_or(0),
        })
        .sum()
}

pub fn register(it: &mut Interp) {
    // The versions of the perl modules this follows.
    it.set_scalar("Unicode::LineBreak::VERSION", Value::str("2019.001"));
    it.set_scalar("Unicode::GCString::VERSION", Value::str("2013.10"));
    // Unicode::GCString->new($str): the object keeps the string
    it.define_native("Unicode::GCString::new", |_, a, _| {
        let s = a.get(1).cloned().unwrap_or_default();
        let h = Value::new_hash();
        if let Value::HashRef(r) = &h {
            r.borrow_mut().blessed = Some(Rc::from("Unicode::GCString"));
        }
        h.set("str", s);
        Ok(vec![h])
    });
    it.define_native("Unicode::GCString::columns", |_, a, _| {
        let s = a.first().map(|o| o.get("str")).unwrap_or_default();
        Ok(vec![Value::Int(columns(&code_points(&s)) as i64)])
    });
    it.define_native("Unicode::GCString::as_string", |_, a, _| Ok(vec![a.first().map(|o| o.get("str")).unwrap_or_default()]));
}

#[cfg(test)]
mod tests {
    use super::columns;

    /// Unicode::LineBreak 2019.001's columns (perl 5.38, Ubuntu 24.04).
    #[test]
    fn columns_as_linebreak() {
        let cases: &[(&str, usize)] = &[
            ("abc", 3),
            ("日本語", 6),
            ("カメラ", 6),
            ("ｶﾒﾗ", 3),
            ("한국어", 6),
            ("Привет", 6),
            ("e\u{301}", 1),
            ("\u{2026}\u{b1}\u{b0}\u{d7}\u{2192}", 5),
            ("\u{3000}\u{ff01}\u{30fc}\u{ffe0}\u{3250}", 10),
            ("\t\n\0\u{7f}\u{200b}\u{feff}\u{ad}\u{2028}\u{3099}\u{fe0f}\u{e0001}\u{600}", 0),
            ("\u{ac00}\u{11a8}\u{1100}\u{1161}", 4),
            ("\u{e01}\u{e33}\u{a0}\u{e000}\u{f0000}\u{378}", 6),
            ("\u{1b000}\u{20000}\u{2329}\u{2e80}", 8),
        ];
        for (s, want) in cases {
            let cps: Vec<u32> = s.chars().map(|c| c as u32).collect();
            assert_eq!(columns(&cps), *want, "{s:?}");
        }
    }
}
