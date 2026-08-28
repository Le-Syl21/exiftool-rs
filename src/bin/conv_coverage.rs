//! How much of ExifTool's conversion language `tags::conv_expr` actually reads.
//!
//! The evaluator's coverage is a number that decides what to build next, so it
//! must not rest on anyone's word: this reads the expressions straight out of a
//! Perl ExifTool checkout, runs every one of them, and reports what it could
//! not do -- ranked by how often ExifTool uses it, since that is what makes one
//! gap worth more than another.
//!
//! Usage:
//!   cargo run --release --features parity --bin conv_coverage [-- <exiftool lib dir>]
//!
//! Default checkout: /home/sylvain/dev/exiftool.

use std::collections::HashMap;
use std::path::PathBuf;

use exiftool_rs::tags::conv_expr::{eval_composite, eval_with, ParseState, Val};

/// Answers every parse-state member, and remembers that it was asked.
///
/// This counter measures whether the evaluator can *express* a conversion, not
/// whether the reader already tracks the state that conversion reads. The two
/// are different jobs, so the expressions that lean on state are counted here
/// and reported separately rather than being quietly folded in.
#[derive(Default)]
struct Probe {
    asked: std::cell::Cell<bool>,
}

impl ParseState for Probe {
    fn member(&self, _: &str) -> Option<Val> {
        self.asked.set(true);
        Some(Val::Num(1.0))
    }

    /// An option nobody set, which is how ExifTool runs unless told otherwise.
    fn option(&self, _: &str) -> Option<Val> {
        self.asked.set(true);
        Some(Val::Undef)
    }

    fn byte_order(&self) -> Option<&str> {
        self.asked.set(true);
        Some("II")
    }

    fn tag_value(&self, name: &str) -> Option<Val> {
        self.asked.set(true);
        // Enough for a conversion that walks the extracted tags looking for a
        // video track, and no more.
        match name {
            "HandlerType" => Some(Val::Str("vide".to_string())),
            "MatrixStructure" => Some(Val::Str("1 0 0 0 1 0 0 0 1".to_string())),
            _ => None,
        }
    }

    fn tag_group1(&self, _: &str) -> Option<String> {
        self.asked.set(true);
        Some("Track1".to_string())
    }

    fn tag_extra(&self, _: &str, _: &str) -> Option<Val> {
        self.asked.set(true);
        Some(Val::List(vec![Val::Str("m".to_string())]))
    }

    fn current_tag(&self) -> Option<String> {
        self.asked.set(true);
        Some("ProbeTag".to_string())
    }
}

/// Whether Perl itself can compile the expression.
///
/// Some of these cannot be evaluated by ExifTool either -- `PrintConv =>
/// '$val m'` in MXF.pm is a search pattern that is never terminated -- and its
/// own eval dies on them, leaving the raw value. Refusing those is the same
/// answer ExifTool gives, so they are counted apart from the real gaps rather
/// than held against the evaluator or quietly forgiven.
fn perl_compiles(expr: &str) -> Option<bool> {
    let out = std::process::Command::new("perl")
        .arg("-e")
        // ExifTool evaluates these inside a module that runs under `strict`,
        // and the eval inherits it -- so an undeclared variable is a compile
        // error there just as it is here.
        .arg("use strict; use warnings; my ($val, $self, $tag) = (42, undef, 'T'); \
              my (@val, @prt, @raw) = ((1,2), (1,2), (1,2)); \
              eval $ARGV[0]; \
              print 'DIES' if $@ and $@ =~ /syntax error|not terminated|requires explicit package/")
        .arg(expr)
        .output()
        .ok()?;
    Some(!String::from_utf8_lossy(&out.stdout).contains("DIES"))
}

/// Every PrintConv/ValueConv written as a Perl expression, with its occurrence
/// count. Hash-table conversions are not expressions and are not counted.
fn collect(lib: &PathBuf) -> Vec<(usize, String)> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let dir = lib.join("Image/ExifTool");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("cannot read {}", dir.display());
        std::process::exit(2);
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "pm") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        // Scanned over the whole file rather than line by line: a good number
        // of these expressions run to three or four lines, and cutting them at
        // the newline counted a fragment nothing could evaluate.
        let b: Vec<char> = text.chars().collect();
        let mut i = 0usize;
        while i < b.len() {
            let Some(key) = ["PrintConv", "ValueConv"]
                .into_iter()
                .find(|k| b[i..].starts_with(&k.chars().collect::<Vec<_>>()[..]))
            else {
                i += 1;
                continue;
            };
            i += key.len();
            // PrintConvInv and ValueConvInv are the write direction: they turn
            // a printed value back into a raw one, which a reader never does.
            // Counting them would inflate the target by half.
            if b[i..].starts_with(&['I', 'n', 'v']) {
                continue;
            }
            while i < b.len() && b[i].is_whitespace() {
                i += 1;
            }
            if !b[i..].starts_with(&['=', '>']) {
                continue;
            }
            i += 2;
            while i < b.len() && b[i].is_whitespace() {
                i += 1;
            }
            // Only the single-quoted form is an expression; { ... } is a table.
            if b.get(i) != Some(&'\'') {
                continue;
            }
            i += 1;
            let mut body = String::new();
            while i < b.len() {
                if b[i] == '\\' && i + 1 < b.len() {
                    body.push(b[i]);
                    body.push(b[i + 1]);
                    i += 2;
                    continue;
                }
                if b[i] == '\'' {
                    i += 1;
                    break;
                }
                body.push(b[i]);
                i += 1;
            }
            // Perl's own escapes inside a single-quoted string: only the quote
            // and the backslash mean anything.
            let body = body.replace("\\'", "'").replace("\\\\", "\\");
            *counts.entry(body).or_default() += 1;
        }
    }
    let mut v: Vec<(usize, String)> = counts.into_iter().map(|(e, n)| (n, e)).collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    v
}

fn main() {
    let lib = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("/home/sylvain/dev/exiftool/lib"), PathBuf::from);

    let exprs = collect(&lib);
    let (mut ok_d, mut ok_o, mut no_d, mut no_o) = (0usize, 0usize, 0usize, 0usize);
    let (mut state_d, mut state_o) = (0usize, 0usize);
    let mut misses: Vec<(usize, String)> = Vec::new();

    for (n, e) in &exprs {
        let probe = Probe::default();
        // A Composite tag is handed the values it is built from and their
        // printed forms, and says so by reading `@val`, `$val[0]` or `$prt[0]`;
        // everything else gets a number, which exercises the arithmetic
        // without dividing by zero.
        let composite =
            ["$val[", "@val", "$prt[", "@prt", "$raw[", "@raw"].iter().any(|m| e.contains(m));
        // A Composite tag can be built from a dozen parts; a short list would
        // make one of these read past the end and refuse for the wrong reason.
        let parts: Vec<Val> = (1..=12).map(|k| Val::Num(f64::from(k))).collect();
        // A single number does not exercise every conversion: some read a
        // whole row of them and would divide by an absent one, which Perl
        // would die on too. So a refusal is tried again with a value that has
        // several fields before it counts as a gap.
        let done = if composite {
            eval_composite(e, &parts, &parts, &parts, &probe).is_some()
        } else {
            eval_with(e, &Val::Num(3.0), &probe).is_some()
                || eval_with(e, &Val::Str("1 2 3 4 5 6 7 8".to_string()), &probe).is_some()
        };
        if done {
            if probe.asked.get() {
                state_d += 1;
                state_o += n;
            }
            ok_d += 1;
            ok_o += n;
        } else {
            no_d += 1;
            no_o += n;
            misses.push((*n, e.clone()));
        }
    }

    // Split the refusals: a gap in the evaluator, or an expression Perl
    // cannot compile either.
    let mut broken_o = 0usize;
    let mut broken: Vec<(usize, String)> = Vec::new();
    let mut perl_missing = false;
    misses.retain(|(n, e)| match perl_compiles(e) {
        Some(false) => {
            broken_o += n;
            broken.push((*n, e.clone()));
            false
        }
        Some(true) => true,
        None => {
            perl_missing = true;
            true
        }
    });

    let total_o = ok_o + no_o;
    let total_d = ok_d + no_d;
    println!("COUNTER ONE — conversion expressions understood by tags::conv_expr");
    println!("  occurrences : {ok_o} / {total_o}  ({}%)", 100 * ok_o / total_o.max(1));
    println!("  distinct    : {ok_d} / {total_d}");
    // The count that can reach 100%: an expression Perl itself cannot compile
    // is one ExifTool prints the raw value for, and no evaluator can do more.
    let evaluable = total_o - broken_o;
    println!(
        "  of the evaluable: {ok_o} / {evaluable}  ({}%)",
        100 * ok_o / evaluable.max(1)
    );
    println!("  state-fed   : {state_o} occurrences ({state_d} distinct) read a parse-state");
    println!("                member, and are right only once the reader tracks it");
    if !broken.is_empty() {
        println!(
            "  unevaluable: {broken_o} occurrences ({} distinct) are not valid Perl -- ExifTool's",
            broken.len()
        );
        println!("                own eval dies on them, so it prints the raw value too:");
        for (n, e) in &broken {
            let shown: String = e.chars().take(72).collect();
            println!("     {n:5}  {shown}");
        }
    }
    if perl_missing {
        println!("  (no perl on PATH: refusals could not be checked against it)");
    }
    println!();
    println!("  still refused, by how often ExifTool uses it:");
    for (n, e) in misses.iter().take(80) {
        let shown: String = e.chars().take(88).collect();
        println!("  {n:5}  {shown}");
    }
    if misses.len() > 80 {
        println!("  … and {} more distinct expressions", misses.len() - 80);
    }
}
