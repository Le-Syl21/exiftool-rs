//! Differential tests against the real perl: sprintf, numification and
//! stringification over a grid of formats and values. Skipped when perl is
//! not installed.

use exiftool_port::perl::sprintf::sprintf;
use exiftool_port::perl::Value;
use std::process::Command;

const VALUES: &[&str] = &[
    "0", "1", "-1", "7", "-7", "255", "65535", "4294967295", "-2147483648",
    "9007199254740993", "1e20", "-1e20", "0.5", "1.5", "2.5", "-0.5", "2.675",
    "0.1", "0.3", "3.14159265358979", "1234567.891", "0.0001", "0.00001",
    "1e-10", "123456789012345", "1234567890123456", "1e15", "99.995", "-99.995",
    "12abc", "  42", "abc", "", "0.0", "1e3", "inf", "-inf", "nan", "0x1A",
    "1_000", ".5", "5.", "-0", "+3",
];

const FORMATS: &[&str] = &[
    "%d", "%5d", "%-5d|", "%05d", "%+d", "% d", "%.3d", "%u", "%x", "%X", "%#x",
    "%.4x", "%o", "%#o", "%b", "%e", "%.2e", "%E", "%f", "%.0f", "%.1f",
    "%.2f", "%10.4f", "%-10.2f|", "%+.1f", "%g", "%.3g", "%.10g", "%.15g",
    "%#g", "%G", "%s", "%5s", "%-5s|", "%.2s",
];

fn perl_available() -> bool {
    Command::new("perl").arg("-e").arg("1").status().map(|s| s.success()).unwrap_or(false)
}

/// The value as a perl expression: numbers as number literals where perl
/// would read them as numbers, the rest as strings.
fn perl_literal(v: &str) -> String {
    format!("'{}'", v.replace('\\', "\\\\").replace('\'', "\\'"))
}

#[test]
fn sprintf_matches_perl() {
    if !perl_available() {
        eprintln!("perl not found, skipped");
        return;
    }
    // One perl run for the whole grid: string values are numified by sprintf
    // itself, as they would be in ExifTool.
    let mut script = String::from("no warnings; $| = 1;\n");
    for f in FORMATS {
        for v in VALUES {
            script.push_str(&format!(
                "print sprintf('{}', {}), \"\\x1e\\x1f\";\n",
                f.replace('\'', "\\'"),
                perl_literal(v)
            ));
        }
    }
    let out = Command::new("perl").arg("-e").arg(&script).output().unwrap();
    let expected: Vec<Vec<u8>> = split_sep(&out.stdout);
    let mut i = 0;
    let mut failures = Vec::new();
    for f in FORMATS {
        for v in VALUES {
            let got = sprintf(f.as_bytes(), &[Value::str(v)]);
            if got != expected[i] {
                failures.push(format!(
                    "sprintf({f:?}, {v:?}): perl {:?}, ours {:?}",
                    String::from_utf8_lossy(&expected[i]),
                    String::from_utf8_lossy(&got)
                ));
            }
            i += 1;
        }
    }
    assert!(failures.is_empty(), "{} mismatches:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn numeric_strings_match_perl() {
    if !perl_available() {
        return;
    }
    // Stringification of the numified value, and of a float computation.
    let mut script = String::from("no warnings;\n");
    for v in VALUES {
        script.push_str(&format!("print 0+{}, \"\\x00\", ({}) ? 1 : 0, \"\\x00\";\n", perl_literal(v), perl_literal(v)));
    }
    let out = Command::new("perl").arg("-e").arg(&script).output().unwrap();
    let expected: Vec<&[u8]> = out.stdout.split(|&b| b == 0).collect();
    let mut failures = Vec::new();
    for (k, v) in VALUES.iter().enumerate() {
        let num = exiftool_port::perl::ops::add(&Value::Int(0), &Value::str(v)).to_string();
        let truth = if Value::str(v).truthy() { "1" } else { "0" };
        if num.as_bytes() != expected[2 * k] {
            failures.push(format!("0+{v:?}: perl {:?}, ours {num:?}", String::from_utf8_lossy(expected[2 * k])));
        }
        if truth.as_bytes() != expected[2 * k + 1] {
            failures.push(format!("truth of {v:?}: perl {:?}, ours {truth}", String::from_utf8_lossy(expected[2 * k + 1])));
        }
    }
    assert!(failures.is_empty(), "{} mismatches:\n{}", failures.len(), failures.join("\n"));
}

fn split_sep(out: &[u8]) -> Vec<Vec<u8>> {
    let mut parts = Vec::new();
    let mut cur = Vec::new();
    let mut i = 0;
    while i < out.len() {
        if out[i] == 0x1e && out.get(i + 1) == Some(&0x1f) {
            parts.push(std::mem::take(&mut cur));
            i += 2;
        } else {
            cur.push(out[i]);
            i += 1;
        }
    }
    parts
}

#[test]
fn pack_unpack_match_perl() {
    if !perl_available() {
        return;
    }
    let data: Vec<u8> = (0u8..=255).rev().chain(0u8..64).collect();
    let templates = [
        "C*", "c*", "n*", "N*", "v*", "V*", "s*", "S*", "l*", "L*", "q*", "Q*", "n!*", "v!*",
        "N!*", "s>*", "l>*", "q<2", "H*", "h*", "H5", "B*", "b16", "a10", "A10", "Z10", "Z*",
        "x3n2", "x5V", "f*", "d*", "f>2", "d>1", "C/a", "w*", "@10C3", "x2X1C", "a*",
    ];
    let hex: String = data.iter().map(|b| format!("{b:02x}")).collect();
    let mut script = format!("no warnings; my $d = pack('H*', '{hex}');\n");
    for t in templates {
        script.push_str(&format!("print join(',', map {{ unpack('H*', $_) }} unpack('{t}', $d)), \"\\x1e\\x1f\";\n"));
    }
    let out = Command::new("perl").arg("-e").arg(&script).output().unwrap();
    let expected = split_sep(&out.stdout);
    let mut failures = Vec::new();
    for (k, t) in templates.iter().enumerate() {
        let ours = exiftool_port::perl::pack::unpack(t.as_bytes(), &data)
            .map(|v| {
                v.iter()
                    .map(|x| x.to_bytes().iter().map(|b| format!("{b:02x}")).collect::<String>())
                    .collect::<Vec<_>>()
                    .join(",")
                    .into_bytes()
            })
            .unwrap_or_else(|e| e.into_bytes());
        if ours != expected[k] {
            failures.push(format!(
                "unpack({t:?}): perl {:?}\n    ours {:?}",
                String::from_utf8_lossy(&expected[k]),
                String::from_utf8_lossy(&ours)
            ));
        }
    }
    // pack: integers and strings back to bytes.
    let cases: &[(&str, &[&str])] = &[
        ("n*", &["1", "65535", "65536", "-1"]),
        ("N", &["4294967296"]),
        ("v2", &["258", "3"]),
        ("C*", &["1", "255", "256", "-1", "3.9"]),
        ("H*", &["0aFf3"]),
        ("H4", &["ab"]),
        ("A6", &["ab"]),
        ("a6", &["ab"]),
        ("Z4", &["abcdef"]),
        ("Z*", &["ab"]),
        ("C0U*", &["233", "8364"]),
        ("w*", &["0", "127", "128", "16384"]),
        ("f", &["1.5"]),
        ("d>", &["-2.25"]),
        ("s<", &["-2"]),
        ("x2n", &["1"]),
        ("B12", &["101100111"]),
    ];
    let mut script = String::from("no warnings;\n");
    for (t, args) in cases {
        let list: Vec<String> = args.iter().map(|a| perl_literal(a)).collect();
        script.push_str(&format!("print unpack('H*', pack('{t}', {})), \"\\x1e\\x1f\";\n", list.join(", ")));
    }
    let out = Command::new("perl").arg("-e").arg(&script).output().unwrap();
    let expected = split_sep(&out.stdout);
    for (k, (t, args)) in cases.iter().enumerate() {
        let vals: Vec<Value> = args.iter().map(|a| Value::str(a)).collect();
        let ours: String = exiftool_port::perl::pack::pack(t.as_bytes(), &vals)
            .map(|b| b.iter().map(|x| format!("{x:02x}")).collect())
            .unwrap_or_else(|e| e);
        if ours.as_bytes() != &expected[k][..] {
            failures.push(format!("pack({t:?}, {args:?}): perl {:?}, ours {ours:?}", String::from_utf8_lossy(&expected[k])));
        }
    }
    assert!(failures.is_empty(), "{} mismatches:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn tr_matches_perl() {
    if !perl_available() {
        return;
    }
    // (subject, search, replace, flags) as ExifTool writes them.
    let cases: &[(&str, &str, &str, &str)] = &[
        ("a\\0b\\0c", "\\0", "", "d"),
        ("Ab-1_x!? z", "-_a-zA-Z0-9", "", "dc"),
        ("1,2,3", ",", " ", ""),
        ("1,2,3", ",", "", ""),
        ("aabbccdd", "a-c", "A-C", "s"),
        ("hello world", "a-y", "b-z", ""),
        ("a-b-c", "\\-", "_", ""),
        ("2010:01:02", ":", "-", ""),
        ("abc\\x01\\x7f", "\\x00-\\x1f\\x7f", ".", ""),
        ("abcdef", "a-f", "AB", ""),
        ("abcdef", "a-f", "AB", "d"),
        ("x  y   z", " ", "", "s"),
        ("x  y   z", " ", " ", "s"),
        ("A1b2", "0-9", "", "cd"),
        ("1.2.3", ".", " ", ""),
    ];
    let mut script = String::from("no warnings;\n");
    for (s, a, b, f) in cases {
        script.push_str(&format!(
            "{{ my $s = \"{s}\"; my $n = ($s =~ tr/{a}/{b}/{f}); print unpack('H*', $s), ' ', $n, \"\\x1e\\x1f\"; }}\n"
        ));
    }
    let out = Command::new("perl").arg("-e").arg(&script).output().unwrap();
    let expected = split_sep(&out.stdout);
    let mut failures = Vec::new();
    for (k, (s, a, b, f)) in cases.iter().enumerate() {
        // The subject is a double-quoted perl string: expand its escapes the
        // same way for our side.
        let subject = perl_dq(s);
        let (res, n) = exiftool_port::perl::tr::tr(&subject, a.as_bytes(), b.as_bytes(), f);
        let ours = format!("{} {n}", res.iter().map(|x| format!("{x:02x}")).collect::<String>());
        if ours.as_bytes() != &expected[k][..] {
            failures.push(format!("tr/{a}/{b}/{f} on {s:?}: perl {:?}, ours {ours:?}", String::from_utf8_lossy(&expected[k])));
        }
    }
    assert!(failures.is_empty(), "{} mismatches:\n{}", failures.len(), failures.join("\n"));
}

/// The escapes of a double-quoted perl string used in the cases above.
fn perl_dq(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            match b[i + 1] {
                b'0' => {
                    out.push(0);
                    i += 2;
                }
                b'x' => {
                    out.push(u8::from_str_radix(&s[i + 2..i + 4], 16).unwrap());
                    i += 4;
                }
                c => {
                    out.push(c);
                    i += 2;
                }
            }
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}
