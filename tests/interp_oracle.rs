//! Run every code string of ExifTool's tables through the interpreter and
//! through perl, with the same variables, and compare the results.
//!
//! The strings run as at ExifTool's eval sites: in package Image::ExifTool,
//! scalar context, with `$val`, `@val`, `$self`, `$valPt`, `$tagInfo`. The
//! ExifTool functions they call are not available on either side, so those
//! calls fail the same way; what is compared is the language.

use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;

use exiftool_port::interp::{Interp, Scope, Want};
use exiftool_port::perl::value::Hash;
use exiftool_port::perl::Value;

fn records() -> Vec<(String, Vec<u8>)> {
    let data = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/code_strings.txt")).unwrap();
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let nl = data[i..].iter().position(|&b| b == b'\n').unwrap() + i;
        let head = std::str::from_utf8(&data[i..nl]).unwrap();
        let (kind, len) = head.rsplit_once(' ').unwrap();
        let len: usize = len.parse().unwrap();
        out.push((kind.to_string(), data[nl + 1..nl + 1 + len].to_vec()));
        i = nl + 1 + len + 1;
    }
    out
}

const VALUES: &[&[u8]] = &[
    b"123",
    b"0",
    b"-5",
    b"1.5",
    b"abc",
    b"2021:03:04 05:06:07",
    b"1 2 3 4",
    b"",
    b"0x1f",
    b"\x01\x02\x03\x04\x00\xff",
    b"Canon EOS 5D",
    b"65535",
    b"3 1 4 1 5 9 2 6",
    b"+12.5e3",
];

const SELF_FIELDS: &[(&str, &str)] = &[
    ("Make", "Canon"),
    ("Model", "Canon EOS 5D Mark III"),
    ("DIR_NAME", "IFD0"),
    ("FILE_TYPE", "JPEG"),
    ("TIFF_TYPE", "TIFF"),
    ("Compression", "6"),
    ("SubfileType", "1"),
    ("FacesDetected", "2"),
];

const PERL_HARNESS: &str = r#"
use strict; no strict 'vars'; no warnings;
binmode STDOUT;
unshift @INC, sub { return unless $_[1] =~ m{^Image/ExifTool/}; my $src = "1;"; open my $fh, '<', \$src; return $fh; };
my $input = do { local $/; <STDIN> }; close STDIN;
my $ipos = 0;
sub rd { return undef if $ipos >= length $input; my $nl = index($input, "\n", $ipos); my $l = substr($input, $ipos, $nl - $ipos); $ipos = $nl + 1; my $b = substr($input, $ipos, $l); $ipos += $l + 1; return $b; }
sub out { my $v = shift; if (!defined $v) { print "U\n"; } else { $v = "$v"; $v =~ s/\b(HASH|ARRAY|SCALAR|CODE|REF)\(0x[0-9a-f]+\)/$1(ADDR)/g; print "S", unpack('H*', $v), "\n"; } }
package Image::ExifTool;
while (1) {
    my $code = main::rd(); last unless defined $code;
    my $v = main::rd();
    my $self = bless { __FIELDS__ }, 'Image::ExifTool';
    $$self{OPTIONS} = { Unknown => 0, PrintConv => 1 };
    my $val = $v;
    my @val = ($v, $v);
    my @prt = ($v, $v);
    my @raw = ($v, $v);
    my $valPt = \$val;
    my $tagInfo = { Name => 'Test' };
    $_ = undef;
    $@ = '';
    my $res = eval $code;
    my $err = $@;
    $err =~ s/ at \(eval \d+\).*//s;
    print $err eq '' ? "OK\n" : "E" . unpack('H*', $err) . "\n";
    main::out($res);
    main::out($val);
}
"#;

#[derive(Debug, PartialEq)]
struct Outcome {
    err: Option<Vec<u8>>,
    res: Option<Vec<u8>>,
    val: Option<Vec<u8>>,
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
}

fn normalize_ref(v: &[u8]) -> Vec<u8> {
    let s = String::from_utf8_lossy(v).into_owned();
    let mut out = String::new();
    let mut rest = s.as_str();
    while let Some(p) = rest.find("(0x") {
        let before = &rest[..p];
        let kind_start = before.rfind(|c: char| !c.is_ascii_uppercase()).map_or(0, |i| i + 1);
        let kind = &before[kind_start..];
        let after = &rest[p + 3..];
        let n = after.find(|c: char| !c.is_ascii_hexdigit()).unwrap_or(after.len());
        if matches!(kind, "HASH" | "ARRAY" | "SCALAR" | "CODE" | "REF") && after[n..].starts_with(')') {
            out.push_str(before);
            out.push_str("(ADDR)");
            rest = &after[n + 1..];
        } else {
            out.push_str(&rest[..p + 3]);
            rest = after;
        }
    }
    out.push_str(rest);
    if out == s {
        v.to_vec()
    } else {
        out.into_bytes()
    }
}

fn perl_outcomes(cases: &[(&[u8], &[u8])]) -> Vec<Outcome> {
    let fields: Vec<String> = SELF_FIELDS.iter().map(|(k, v)| format!("{k} => '{v}'")).collect();
    let script = PERL_HARNESS.replace("__FIELDS__", &fields.join(", "));
    let mut input = Vec::new();
    for (code, v) in cases {
        input.extend_from_slice(format!("{}\n", code.len()).as_bytes());
        input.extend_from_slice(code);
        input.push(b'\n');
        input.extend_from_slice(format!("{}\n", v.len()).as_bytes());
        input.extend_from_slice(v);
        input.push(b'\n');
    }
    let mut child = Command::new("perl")
        .arg("-e")
        .arg(&script)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        use std::io::Write;
        let _ = stdin.write_all(&input);
    });
    let out = child.wait_with_output().unwrap();
    writer.join().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let parse = |l: &str| if l == "U" { None } else { Some(unhex(&l[1..])) };
    lines
        .chunks(3)
        .map(|c| Outcome { err: if c[0] == "OK" { None } else { Some(unhex(&c[0][1..])) }, res: parse(c[1]), val: parse(c[2]) })
        .collect()
}

fn our_outcome(it: &mut Interp, code: &[u8], v: &[u8]) -> Outcome {
    let scope = Scope::new(None);
    let mut h = Hash::default();
    for (k, val) in SELF_FIELDS {
        h.insert(k.as_bytes(), Value::str(val));
    }
    let mut opts = Hash::default();
    opts.insert(b"Unknown", Value::Int(0));
    opts.insert(b"PrintConv", Value::Int(1));
    h.insert(b"OPTIONS", Value::HashRef(Rc::new(RefCell::new(opts))));
    h.blessed = Some(Rc::from("Image::ExifTool"));
    scope.scalar("self", Value::HashRef(Rc::new(RefCell::new(h))));
    let val = scope.scalar("val", Value::bytes(v));
    scope.array("val", vec![Value::bytes(v), Value::bytes(v)]);
    scope.array("prt", vec![Value::bytes(v), Value::bytes(v)]);
    scope.array("raw", vec![Value::bytes(v), Value::bytes(v)]);
    scope.scalar("valPt", Value::ScalarRef(val.clone()));
    let mut ti = Hash::default();
    ti.insert(b"Name", Value::str("Test"));
    scope.scalar("tagInfo", Value::HashRef(Rc::new(RefCell::new(ti))));
    it.set_scalar("main::_", Value::Undef);
    let r = it.eval_string(code, "Image::ExifTool", &scope, Want::Scalar);
    let strip = |e: &[u8]| -> Vec<u8> {
        // " at (eval N) line N." / ", near ..." endings, as on the perl side.
        match e.windows(9).position(|w| w == b" at (eval") {
            Some(p) => e[..p].to_vec(),
            None => e.to_vec(),
        }
    };
    let (err, res) = match r {
        Ok(mut v) => (None, v.pop().unwrap_or_default()),
        Err(e) => (Some(strip(&e.to_bytes())), Value::Undef),
    };
    let to = |v: &Value| if v.is_undef() { None } else { Some(normalize_ref(&v.to_bytes())) };
    let val_now = val.borrow().clone();
    Outcome { err, res: to(&res), val: to(&val_now) }
}

#[test]
fn table_strings_match_perl() {
    if Command::new("perl").arg("-v").output().is_err() {
        return;
    }
    let recs = records();
    let mut cases: Vec<(&[u8], &[u8])> = Vec::new();
    for (_, code) in &recs {
        for v in VALUES {
            cases.push((code, v));
        }
    }
    let expected = perl_outcomes(&cases);
    assert_eq!(expected.len(), cases.len(), "perl harness output is incomplete");
    let mut it = Interp::new();
    let mut same = 0;
    let mut skipped = 0;
    let mut failures = Vec::new();
    for (k, (code, v)) in cases.iter().enumerate() {
        let exp = &expected[k];
        // Calls into ExifTool itself fail on both sides; the messages are
        // compared like any other result.
        let ours = our_outcome(&mut it, code, v);
        if ours == *exp {
            same += 1;
            continue;
        }
        // Both failed: perl's compile errors word things differently.
        if ours.err.is_some() && exp.err.as_ref().is_some_and(|e| e.starts_with(b"syntax error") || e.starts_with(b"Global symbol")) {
            skipped += 1;
            continue;
        }
        failures.push(format!(
            "{}\n  val={:?}\n  perl: err={:?} res={:?} val={:?}\n  ours: err={:?} res={:?} val={:?}",
            String::from_utf8_lossy(code).trim(),
            String::from_utf8_lossy(v),
            exp.err.as_ref().map(|e| String::from_utf8_lossy(e).into_owned()),
            exp.res.as_ref().map(|e| String::from_utf8_lossy(e).into_owned()),
            exp.val.as_ref().map(|e| String::from_utf8_lossy(e).into_owned()),
            ours.err.as_ref().map(|e| String::from_utf8_lossy(e).into_owned()),
            ours.res.as_ref().map(|e| String::from_utf8_lossy(e).into_owned()),
            ours.val.as_ref().map(|e| String::from_utf8_lossy(e).into_owned()),
        ));
    }
    let n = failures.len();
    // One example per distinct string is enough to read.
    let mut seen = std::collections::HashSet::new();
    let shown: Vec<String> = failures.into_iter().filter(|f| seen.insert(f.lines().next().unwrap().to_string())).take(60).collect();
    eprintln!("{same} identical, {skipped} compile errors on both sides, {n} different, of {}", cases.len());
    assert!(n == 0, "{n} of {} evaluations differ from perl:\n{}", cases.len(), shown.join("\n"));
    let _ = hex;
}
