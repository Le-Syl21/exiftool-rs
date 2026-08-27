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
        for line in text.lines() {
            for key in ["PrintConv", "ValueConv"] {
                let Some(i) = line.find(key) else { continue };
                let rest = &line[i + key.len()..];
                // PrintConvInv and ValueConvInv are the write direction: they
                // turn a printed value back into a raw one, which a reader never
                // does. Counting them would inflate the target by half.
                if rest.starts_with("Inv") {
                    continue;
                }
                let Some(j) = rest.find("=>") else { continue };
                let after = rest[j + 2..].trim_start();
                // Only the single-quoted form is an expression; { ... } is a table.
                if !after.starts_with('\'') {
                    continue;
                }
                let body = &after[1..];
                let Some(end) = body.find('\'') else { continue };
                *counts.entry(body[..end].to_string()).or_default() += 1;
            }
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
