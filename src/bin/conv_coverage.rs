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

use exiftool_rs::tags::conv_expr::{eval_with, ParseState, Val};

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
        // A value that exercises arithmetic without dividing by zero.
        if eval_with(e, &Val::Num(3.0), &probe).is_some() {
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

    let total_o = ok_o + no_o;
    let total_d = ok_d + no_d;
    println!("COUNTER ONE — conversion expressions understood by tags::conv_expr");
    println!("  occurrences : {ok_o} / {total_o}  ({}%)", 100 * ok_o / total_o.max(1));
    println!("  distinct    : {ok_d} / {total_d}");
    println!("  state-fed   : {state_o} occurrences ({state_d} distinct) read a parse-state");
    println!("                member, and are right only once the reader tracks it");
    println!();
    println!("  still refused, by how often ExifTool uses it:");
    for (n, e) in misses.iter().take(25) {
        let shown: String = e.chars().take(88).collect();
        println!("  {n:5}  {shown}");
    }
    if misses.len() > 25 {
        println!("  … and {} more distinct expressions", misses.len() - 25);
    }
}
