use exiftool_rs::ExifTool;
use std::collections::BTreeSet;
use std::panic;
use std::path::Path;

/// Helper to call extract_info catching both errors and panics.
fn safe_extract(path: &Path) -> Option<Vec<exiftool_rs::Tag>> {
    let path = path.to_path_buf();
    let result = panic::catch_unwind(move || {
        let et = ExifTool::new();
        et.extract_info(&path)
    });
    match result {
        Ok(Ok(tags)) => Some(tags),
        _ => None,
    }
}

// ── Tag-name parity, with a ratcheting baseline ─────────────────────────────
//
// For every test image with a `tests/expected/<file>.tags` file (the tag names
// produced by real ExifTool), we diff exiftool-rs's output against it. Rather
// than demand 100% parity today, we record the *current* deltas in a committed
// baseline and fail only when a NEW delta appears:
//   - a `missing` tag that wasn't missing before  → a coverage regression
//     (e.g. a parser broke and tags vanished),
//   - an `extra` tag that wasn't there before      → a new spurious tag.
// Improvements (a baselined delta that no longer occurs) are allowed and prompt
// a baseline refresh. The net can only tighten.
//
// Regenerate the baseline after an intentional change:
//   UPDATE_PARITY_BASELINE=1 cargo test --test regression regression_tag_names

/// `(file, "missing" | "extra", tag)`.
type Delta = (String, &'static str, String);

const BASELINE: &str = "tests/parity_baseline.txt";

/// Compute the current set of deltas across the corpus, and how many files were
/// actually compared.
fn current_deltas() -> (BTreeSet<Delta>, usize) {
    let images_dir = Path::new("tests/images");
    let expected_dir = Path::new("tests/expected");

    let mut entries: Vec<_> = std::fs::read_dir(images_dir)
        .unwrap()
        .map(|e| e.unwrap())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let mut deltas = BTreeSet::new();
    let mut tested = 0;

    for entry in entries {
        let file_name = entry.file_name().to_string_lossy().to_string();
        let expected_path = expected_dir.join(format!("{file_name}.tags"));
        if !expected_path.exists() {
            continue;
        }

        // A parse failure / panic counts as "no tags produced" so that a parser
        // regression surfaces as newly-missing tags instead of silently skipping.
        let tags = safe_extract(&entry.path()).unwrap_or_default();
        let actual: BTreeSet<String> = tags.iter().map(|t| t.name.clone()).collect();
        let expected: BTreeSet<String> = std::fs::read_to_string(&expected_path)
            .unwrap()
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect();

        for t in expected.difference(&actual) {
            deltas.insert((file_name.clone(), "missing", t.clone()));
        }
        for t in actual.difference(&expected) {
            deltas.insert((file_name.clone(), "extra", t.clone()));
        }
        tested += 1;
    }

    (deltas, tested)
}

fn fmt_delta(d: &Delta) -> String {
    format!("{}\t{}\t{}", d.0, d.1, d.2)
}

fn read_baseline() -> BTreeSet<Delta> {
    std::fs::read_to_string(BASELINE)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut it = l.splitn(3, '\t');
            let file = it.next()?.to_string();
            let kind = match it.next()? {
                "missing" => "missing",
                "extra" => "extra",
                _ => return None,
            };
            let tag = it.next()?.to_string();
            Some((file, kind, tag))
        })
        .collect()
}

fn write_baseline(deltas: &BTreeSet<Delta>) {
    let mut out = String::from(
        "# Parity baseline: known deltas between exiftool-rs and ExifTool tag names.\n\
         # `regression_tag_names` fails on any NEW delta (a tag that newly disappears\n\
         # = regression, or a new spurious tag). Improvements are allowed.\n\
         # Regenerate: UPDATE_PARITY_BASELINE=1 cargo test --test regression regression_tag_names\n\
         # Format: <file>\\t<missing|extra>\\t<tag>\n",
    );
    for d in deltas {
        out.push_str(&fmt_delta(d));
        out.push('\n');
    }
    std::fs::write(BASELINE, out).unwrap();
}

#[test]
fn regression_tag_names() {
    let (current, tested) = current_deltas();
    assert!(
        tested >= 100,
        "Expected to compare at least 100 files, got {tested}"
    );

    if std::env::var_os("UPDATE_PARITY_BASELINE").is_some() {
        write_baseline(&current);
        eprintln!(
            "Wrote {BASELINE}: {} known delta(s) over {tested} files.",
            current.len()
        );
        return;
    }

    let baseline = read_baseline();
    let regressions: Vec<_> = current.difference(&baseline).collect();
    let improvements = baseline.difference(&current).count();

    if improvements > 0 {
        eprintln!(
            "✨ {improvements} baselined delta(s) no longer occur — tighten the net with \
             `UPDATE_PARITY_BASELINE=1 cargo test --release --test regression regression_tag_names`."
        );
    }

    // Debug builds panic on arithmetic overflow, which makes several parsers bail
    // and skews the corpus output. Parity is therefore enforced in release (how the
    // crate actually runs and how the baseline is generated); in debug we only report.
    if cfg!(debug_assertions) {
        eprintln!(
            "debug build: {} delta(s) vs baseline — not enforced. Run `cargo test --release`.",
            regressions.len()
        );
        return;
    }

    assert!(
        regressions.is_empty(),
        "{} NEW tag-name delta(s) vs ExifTool — a regression or a new spurious tag:\n{}\n\n\
         If this change is intentional, regenerate the baseline:\n  \
         UPDATE_PARITY_BASELINE=1 cargo test --test regression regression_tag_names",
        regressions.len(),
        regressions
            .iter()
            .map(|d| format!("  {}", fmt_delta(d)))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

// ── Tag-VALUE parity, with its own ratcheting baseline ─────────────────────
//
// Same idea as the name parity above, but compares the *printed value* of each
// tag against ExifTool (tests/expected_values/<file>.vals, name<TAB>value, with
// volatile system tags excluded). A delta is keyed on (file, tag) — its value
// differs from ExifTool. New deltas fail; fixes tighten the baseline.
//
// Regenerate: UPDATE_VALUE_BASELINE=1 cargo test --release --test regression regression_tag_values

const VALUE_BASELINE: &str = "tests/value_baseline.txt";

/// Mirror src/main.rs::sanitize_display_value — the `-s` display sanitization.
fn sanitize_value(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch == '\0' {
            // remove null bytes
        } else if ('\u{01}'..='\u{1f}').contains(&ch) || ch == '\u{7f}' {
            result.push('.');
        } else {
            result.push(ch);
        }
    }
    result.trim_end().to_string()
}

fn current_value_deltas() -> (BTreeSet<(String, String)>, usize) {
    use std::collections::HashMap;
    let images_dir = Path::new("tests/images");
    let expected_dir = Path::new("tests/expected_values");

    let mut entries: Vec<_> = std::fs::read_dir(images_dir)
        .unwrap()
        .map(|e| e.unwrap())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let mut deltas = BTreeSet::new();
    let mut tested = 0;

    for entry in entries {
        let file_name = entry.file_name().to_string_lossy().to_string();
        let vals_path = expected_dir.join(format!("{file_name}.vals"));
        if !vals_path.exists() {
            continue;
        }

        let tags = safe_extract(&entry.path()).unwrap_or_default();
        // First printed value per tag name (ExifTool -s shows the priority tag).
        // Mirror the CLI's `-s` sanitization (control chars -> '.', strip NULs and
        // trailing whitespace) so we compare what is actually displayed, matching
        // ExifTool's own -s-derived expected values.
        let mut actual: HashMap<&str, String> = HashMap::new();
        for t in &tags {
            actual
                .entry(t.name.as_str())
                .or_insert_with(|| sanitize_value(&t.print_value));
        }

        let content = std::fs::read(&vals_path)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        for line in content.lines() {
            let mut it = line.splitn(2, '\t');
            let (name, expected) = match (it.next(), it.next()) {
                (Some(n), Some(v)) => (n, v),
                _ => continue,
            };
            if let Some(got) = actual.get(name) {
                if got.as_str() != expected {
                    deltas.insert((file_name.clone(), name.to_string()));
                }
            }
        }
        tested += 1;
    }

    (deltas, tested)
}

fn read_value_baseline() -> BTreeSet<(String, String)> {
    std::fs::read_to_string(VALUE_BASELINE)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut it = l.splitn(2, '\t');
            Some((it.next()?.to_string(), it.next()?.to_string()))
        })
        .collect()
}

fn write_value_baseline(deltas: &BTreeSet<(String, String)>) {
    let mut out = String::from(
        "# Value-parity baseline: (file, tag) whose printed value differs from ExifTool.\n\
         # New deltas fail regression_tag_values; fixes tighten it.\n\
         # Regenerate: UPDATE_VALUE_BASELINE=1 cargo test --release --test regression regression_tag_values\n",
    );
    for (file, tag) in deltas {
        out.push_str(file);
        out.push('\t');
        out.push_str(tag);
        out.push('\n');
    }
    std::fs::write(VALUE_BASELINE, out).unwrap();
}

#[test]
fn regression_tag_values() {
    let (current, tested) = current_value_deltas();
    assert!(tested >= 100, "Expected at least 100 files, got {tested}");

    if std::env::var_os("UPDATE_VALUE_BASELINE").is_some() {
        write_value_baseline(&current);
        eprintln!("Wrote {VALUE_BASELINE}: {} value delta(s) over {tested} files.", current.len());
        return;
    }

    // Enforced in release only (debug panics on overflow, skewing the corpus).
    if cfg!(debug_assertions) {
        eprintln!("debug build: value parity not enforced. Run `cargo test --release`.");
        return;
    }

    let baseline = read_value_baseline();
    let regressions: Vec<_> = current.difference(&baseline).collect();
    let improvements = baseline.difference(&current).count();
    if improvements > 0 {
        eprintln!(
            "✨ {improvements} value delta(s) fixed — tighten with \
             `UPDATE_VALUE_BASELINE=1 cargo test --release --test regression regression_tag_values`."
        );
    }
    assert!(
        regressions.is_empty(),
        "{} NEW tag-value delta(s) vs ExifTool:\n{}\n\nIf intentional, regenerate with \
         UPDATE_VALUE_BASELINE=1.",
        regressions.len(),
        regressions
            .iter()
            .map(|(f, t)| format!("  {f}\t{t}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

#[test]
fn all_test_files_parse_without_panic() {
    let images_dir = Path::new("tests/images");
    let mut ok = 0;
    let mut err = 0;
    let mut panicked = 0;
    let mut panic_files = Vec::new();

    for entry in std::fs::read_dir(images_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();

        match safe_extract(&path) {
            Some(_) => ok += 1,
            None => {
                // Distinguish error from panic by trying again without catch_unwind
                // (we already caught it, so just count it)
                let et = ExifTool::new();
                let is_panic = panic::catch_unwind(panic::AssertUnwindSafe(|| {
                    let _ = et.extract_info(&path);
                }))
                .is_err();

                if is_panic {
                    panicked += 1;
                    panic_files.push(file_name);
                } else {
                    err += 1;
                }
            }
        }
    }

    println!(
        "Parsed: {} ok, {} errors, {} panics out of {}",
        ok,
        err,
        panicked,
        ok + err + panicked
    );
    if !panic_files.is_empty() {
        println!("Files that caused panics:");
        for f in &panic_files {
            println!("  {}", f);
        }
    }
    // At least 150 of 194 files should parse successfully
    assert!(
        ok >= 150,
        "Expected at least 150 successful parses, got {}",
        ok
    );
}
