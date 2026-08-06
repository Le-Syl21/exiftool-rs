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
        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut actual: BTreeSet<String> = tags.iter().map(|t| t.name.clone()).collect();
        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut expected: BTreeSet<String> = std::fs::read_to_string(&expected_path)
            .unwrap()
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect();

        // The File:System pseudo-tags below are derived from Unix `stat`/permission
        // bits (and the oracle was generated on Unix). They have no equivalent in the
        // current Windows code path, so drop them from the comparison there — Windows
        // still validates name parity for every real format tag.
        #[cfg(windows)]
        {
            const UNIX_FS_TAGS: [&str; 4] = [
                "FileModifyDate",
                "FileAccessDate",
                "FileInodeChangeDate",
                "FilePermissions",
            ];
            for t in UNIX_FS_TAGS {
                actual.remove(t);
                expected.remove(t);
            }
        }

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

#[cfg(unix)]
const VALUE_BASELINE: &str = "tests/value_baseline.txt";

/// Mirror src/main.rs::sanitize_display_value — the `-s` display sanitization.
#[cfg(unix)]
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

/// The value oracle (`.vals`) was produced by Perl ExifTool on a machine in
/// `Europe/Paris`. A handful of tags (gzip/psp/palm/… mtimes) convert a Unix
/// timestamp to *local* time, so their printed value depends on the process
/// timezone. CI runners are UTC, which would spuriously diverge from the oracle.
/// Pin the test process to the oracle's timezone (Paris) so the conversion is
/// deterministic on any machine — local dev, Linux CI, macOS CI alike.
///
/// The zone is spelled as a self-contained POSIX rule rather than
/// `Europe/Paris`: a named zone needs the tzdata database, which minimal
/// sandboxes (Nix builds, scratch containers) don't ship — libc then silently
/// falls back to UTC and every QuickTime date drifts by the UTC offset
/// (issue #9). The rule below is byte-identical to `Europe/Paris` for every
/// post-1996 timestamp, i.e. the whole corpus.
#[cfg(unix)]
fn force_oracle_tz() {
    use std::sync::Once;
    static TZ_INIT: Once = Once::new();
    TZ_INIT.call_once(|| {
        std::env::set_var("TZ", "CET-1CEST,M3.5.0,M10.5.0/3");
        extern "C" {
            fn tzset();
        }
        // SAFETY: tzset() only reads the TZ env var and updates libc's global
        // timezone state; it is safe to call once before any localtime_r.
        unsafe { tzset() };
    });
}

#[cfg(unix)]
fn current_value_deltas() -> (BTreeSet<(String, String)>, usize) {
    use std::collections::HashMap;
    force_oracle_tz();
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

#[cfg(unix)]
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

#[cfg(unix)]
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

// Unix-only: the value oracle pins local-time conversions to Europe/Paris via
// `force_oracle_tz()`, which relies on libc `tzset`/`localtime_r`. Windows has no
// `localtime_r` (gzip.rs falls back to a heuristic that can't reproduce IANA
// zones), so the local-time tags would diverge there. Name parity still runs on
// Windows; value parity is validated against the Unix Perl reference.
#[cfg(unix)]
#[test]
fn regression_tag_values() {
    let (current, tested) = current_value_deltas();
    assert!(tested >= 100, "Expected at least 100 files, got {tested}");

    if std::env::var_os("UPDATE_VALUE_BASELINE").is_some() {
        write_value_baseline(&current);
        eprintln!(
            "Wrote {VALUE_BASELINE}: {} value delta(s) over {tested} files.",
            current.len()
        );
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

// ── Tag-GROUP parity, with its own ratcheting baseline ─────────────────────
//
// Same ratchet as the name and value parity above, applied to the group a tag is
// assigned to. `tests/expected_groups/<file>.grps` holds, for every tag ExifTool
// extracts, the family-1 and family-2 group it puts the tag in:
//
//     <tag name><TAB><family1><TAB><family2>
//
// Regenerate the oracle with `scripts/gen_group_baselines.sh` (needs Perl
// ExifTool); see that script for the exact recipe.
//
// A delta is keyed on (file, tag, family), so a family-1 fix tightens the net
// even while family 2 is still wrong for the same tag. Unlike the `.vals`
// oracle, nothing is excluded here: the volatile System/ExifTool pseudo-tags
// have unstable VALUES but perfectly stable GROUPS.
//
// Tags we do not extract at all are simply skipped — that gap is already
// measured by `regression_tag_names`.
//
// Regenerate: UPDATE_GROUP_BASELINE=1 cargo test --release --test regression regression_tag_groups

const GROUP_BASELINE: &str = "tests/group_baseline.txt";

/// `(file, tag, "family1" | "family2")`.
type GroupDelta = (String, String, &'static str);

fn current_group_deltas() -> (BTreeSet<GroupDelta>, usize) {
    use std::collections::HashMap;
    let images_dir = Path::new("tests/images");
    let expected_dir = Path::new("tests/expected_groups");

    let mut entries: Vec<_> = std::fs::read_dir(images_dir)
        .unwrap()
        .map(|e| e.unwrap())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let mut deltas = BTreeSet::new();
    let mut tested = 0;

    for entry in entries {
        let file_name = entry.file_name().to_string_lossy().to_string();
        let grps_path = expected_dir.join(format!("{file_name}.grps"));
        if !grps_path.exists() {
            continue;
        }

        let tags = safe_extract(&entry.path()).unwrap_or_default();
        // First occurrence per tag name, the same convention the oracle uses.
        let mut actual: HashMap<&str, (&str, &str)> = HashMap::new();
        for t in &tags {
            actual
                .entry(t.name.as_str())
                .or_insert((t.group.family1.as_str(), t.group.family2.as_str()));
        }

        let content = std::fs::read_to_string(&grps_path).unwrap_or_default();
        for line in content.lines() {
            let mut it = line.splitn(3, '\t');
            let (name, want1, want2) = match (it.next(), it.next(), it.next()) {
                (Some(n), Some(a), Some(b)) => (n, a, b),
                _ => continue,
            };
            let Some(&(got1, got2)) = actual.get(name) else {
                continue;
            };
            if got1 != want1 {
                deltas.insert((file_name.clone(), name.to_string(), "family1"));
            }
            if got2 != want2 {
                deltas.insert((file_name.clone(), name.to_string(), "family2"));
            }
        }
        tested += 1;
    }

    (deltas, tested)
}

fn read_group_baseline() -> BTreeSet<GroupDelta> {
    std::fs::read_to_string(GROUP_BASELINE)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut it = l.splitn(3, '\t');
            let file = it.next()?.to_string();
            let tag = it.next()?.to_string();
            let family = match it.next()? {
                "family1" => "family1",
                "family2" => "family2",
                _ => return None,
            };
            Some((file, tag, family))
        })
        .collect()
}

fn write_group_baseline(deltas: &BTreeSet<GroupDelta>) {
    let mut out = String::from(
        "# Group-parity baseline: (file, tag, family) whose group differs from ExifTool.\n\
         # New deltas fail regression_tag_groups; fixes tighten it.\n\
         # Oracle: tests/expected_groups/*.grps, produced by scripts/gen_group_baselines.sh\n\
         # Regenerate: UPDATE_GROUP_BASELINE=1 cargo test --release --test regression regression_tag_groups\n\
         # Format: <file>\\t<tag>\\t<family1|family2>\n",
    );
    for (file, tag, family) in deltas {
        out.push_str(file);
        out.push('\t');
        out.push_str(tag);
        out.push('\t');
        out.push_str(family);
        out.push('\n');
    }
    std::fs::write(GROUP_BASELINE, out).unwrap();
}

#[test]
fn regression_tag_groups() {
    let (current, tested) = current_group_deltas();
    assert!(tested >= 100, "Expected at least 100 files, got {tested}");

    if std::env::var_os("UPDATE_GROUP_BASELINE").is_some() {
        write_group_baseline(&current);
        eprintln!(
            "Wrote {GROUP_BASELINE}: {} group delta(s) over {tested} files.",
            current.len()
        );
        return;
    }

    // Enforced in release only (debug panics on overflow, skewing the corpus),
    // exactly like its name- and value-parity siblings.
    if cfg!(debug_assertions) {
        eprintln!("debug build: group parity not enforced. Run `cargo test --release`.");
        return;
    }

    let baseline = read_group_baseline();
    let regressions: Vec<_> = current.difference(&baseline).collect();
    let improvements = baseline.difference(&current).count();
    if improvements > 0 {
        eprintln!(
            "✨ {improvements} group delta(s) fixed — tighten with \
             `UPDATE_GROUP_BASELINE=1 cargo test --release --test regression regression_tag_groups`."
        );
    }
    assert!(
        regressions.is_empty(),
        "{} NEW tag-group delta(s) vs ExifTool:\n{}\n\nIf intentional, regenerate with \
         UPDATE_GROUP_BASELINE=1.",
        regressions.len(),
        regressions
            .iter()
            .map(|(f, t, fam)| format!("  {f}\t{t}\t{fam}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

// ── Full-multiset value parity, both extraction modes ──────────────────────
//
// The `.vals` ratchet above has two blind spots, both of which hid real bugs:
//
//   1. it keeps only the FIRST occurrence per tag name, so emitting a tag twice
//      where ExifTool emits it once is invisible to it;
//   2. it only ever runs the default extraction mode, so everything reached
//      through `-ee` (ExtractEmbedded) is untested but for `Garmin.fit`.
//
// This ratchet compares the COMPLETE multiset of `name<TAB>value` pairs — every
// occurrence, not one per name — for both modes, against
// `tests/expected_multi/<file>.mvals` and `<file>.ee.mvals`. Those oracles come
// from Perl ExifTool 13.59 via `scripts/gen_multiset_baselines.sh`; see that
// script for the exact recipe (in short: `-S` so there is no column padding to
// mistake for data, records split on `Name: ` headers so multi-line values stay
// on one line, and the same `-s` sanitization the crate applies).
//
// A delta carries an occurrence index so multiplicity ratchets too: emitting a
// pair a third time when the baseline records two is a NEW delta.
//
// Regenerate: UPDATE_MULTISET_BASELINE=1 cargo test --release --test regression regression_tag_multiset

#[cfg(unix)]
const MULTISET_BASELINE: &str = "tests/multiset_baseline.txt";

/// `(file, "default" | "ee", "extra" | "missing", name, value, occurrence)`.
#[cfg(unix)]
type MultiDelta = (String, &'static str, &'static str, String, String, u32);

/// The two extraction modes the ratchet covers: `(Options.extract_embedded,
/// oracle suffix, baseline label)`.
#[cfg(unix)]
const MULTISET_MODES: [(u8, &str, &str); 2] = [(0, ".mvals", "default"), (1, ".ee.mvals", "ee")];

/// `safe_extract`, with an explicit `extract_embedded` setting.
#[cfg(unix)]
fn safe_extract_with_ee(path: &Path, extract_embedded: u8) -> Option<Vec<exiftool_rs::Tag>> {
    use exiftool_rs::Options;
    let path = path.to_path_buf();
    let result = panic::catch_unwind(move || {
        let et = ExifTool::with_options(Options {
            extract_embedded,
            ..Default::default()
        });
        et.extract_info(&path)
    });
    match result {
        Ok(Ok(tags)) => Some(tags),
        _ => None,
    }
}

/// Returns `(deltas, files compared)` — files being counted once per mode.
#[cfg(unix)]
fn current_multiset_deltas() -> (BTreeSet<MultiDelta>, usize) {
    use std::collections::BTreeMap;
    force_oracle_tz();
    let images_dir = Path::new("tests/images");
    let expected_dir = Path::new("tests/expected_multi");

    let mut entries: Vec<_> = std::fs::read_dir(images_dir)
        .unwrap()
        .map(|e| e.unwrap())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let mut deltas = BTreeSet::new();
    let mut tested = 0;

    for entry in entries {
        let file_name = entry.file_name().to_string_lossy().to_string();
        for (extract_embedded, suffix, mode) in MULTISET_MODES {
            let oracle = expected_dir.join(format!("{file_name}{suffix}"));
            if !oracle.exists() {
                continue;
            }

            // Signed occurrence count per pair: ours adds, ExifTool's subtracts.
            // What is left is the multiset difference in both directions.
            let mut balance: BTreeMap<(String, String), i64> = BTreeMap::new();

            let tags = safe_extract_with_ee(&entry.path(), extract_embedded).unwrap_or_default();
            for t in &tags {
                if FIT_VOLATILE_TAGS.contains(&t.name.as_str()) {
                    continue;
                }
                let key = (t.name.clone(), sanitize_value(&t.print_value));
                *balance.entry(key).or_default() += 1;
            }

            let content = std::fs::read(&oracle)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default();
            for line in content.lines() {
                let mut it = line.splitn(2, '\t');
                let (name, value) = match (it.next(), it.next()) {
                    (Some(n), Some(v)) => (n, v),
                    _ => continue,
                };
                *balance
                    .entry((name.to_string(), value.to_string()))
                    .or_default() -= 1;
            }

            for ((name, value), count) in balance {
                let kind = if count > 0 { "extra" } else { "missing" };
                for occurrence in 0..count.unsigned_abs() as u32 {
                    deltas.insert((
                        file_name.clone(),
                        mode,
                        kind,
                        name.clone(),
                        value.clone(),
                        occurrence,
                    ));
                }
            }
            tested += 1;
        }
    }

    (deltas, tested)
}

#[cfg(unix)]
fn fmt_multi_delta(d: &MultiDelta) -> String {
    format!("{}\t{}\t{}\t{}\t{}\t{}", d.0, d.1, d.2, d.5, d.3, d.4)
}

#[cfg(unix)]
fn read_multiset_baseline() -> BTreeSet<MultiDelta> {
    std::fs::read(MULTISET_BASELINE)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut it = l.splitn(6, '\t');
            let file = it.next()?.to_string();
            let mode = match it.next()? {
                "default" => "default",
                "ee" => "ee",
                _ => return None,
            };
            let kind = match it.next()? {
                "extra" => "extra",
                "missing" => "missing",
                _ => return None,
            };
            let occurrence: u32 = it.next()?.parse().ok()?;
            let name = it.next()?.to_string();
            let value = it.next()?.to_string();
            Some((file, mode, kind, name, value, occurrence))
        })
        .collect()
}

#[cfg(unix)]
fn write_multiset_baseline(deltas: &BTreeSet<MultiDelta>) {
    let mut out = String::from(
        "# Multiset value-parity baseline: every `name<TAB>value` pair we emit a\n\
         # different number of times than ExifTool, for both extraction modes.\n\
         # New deltas fail regression_tag_multiset; fixes tighten it.\n\
         # Oracle: tests/expected_multi/*.mvals, from scripts/gen_multiset_baselines.sh\n\
         # Regenerate: UPDATE_MULTISET_BASELINE=1 cargo test --release --test regression regression_tag_multiset\n\
         # Format: <file>\\t<default|ee>\\t<extra|missing>\\t<occurrence>\\t<tag>\\t<value>\n",
    );
    for d in deltas {
        out.push_str(&fmt_multi_delta(d));
        out.push('\n');
    }
    std::fs::write(MULTISET_BASELINE, out).unwrap();
}

#[cfg(unix)]
#[test]
fn regression_tag_multiset() {
    let (current, tested) = current_multiset_deltas();
    assert!(
        tested >= 200,
        "Expected at least 200 (file, mode) comparisons, got {tested}"
    );

    if std::env::var_os("UPDATE_MULTISET_BASELINE").is_some() {
        let default_deltas = current.iter().filter(|d| d.1 == "default").count();
        let ee_deltas = current.len() - default_deltas;
        write_multiset_baseline(&current);
        eprintln!(
            "Wrote {MULTISET_BASELINE}: {} delta(s) over {tested} (file, mode) pairs \
             — {default_deltas} default, {ee_deltas} -ee.",
            current.len()
        );
        return;
    }

    // Enforced in release only (debug panics on overflow, skewing the corpus),
    // exactly like its name-, value- and group-parity siblings.
    if cfg!(debug_assertions) {
        eprintln!("debug build: multiset parity not enforced. Run `cargo test --release`.");
        return;
    }

    let baseline = read_multiset_baseline();
    let regressions: Vec<_> = current.difference(&baseline).collect();
    let improvements = baseline.difference(&current).count();
    if improvements > 0 {
        eprintln!(
            "✨ {improvements} multiset delta(s) fixed — tighten with \
             `UPDATE_MULTISET_BASELINE=1 cargo test --release --test regression regression_tag_multiset`."
        );
    }
    const SHOW: usize = 20;
    assert!(
        regressions.is_empty(),
        "{} NEW multiset delta(s) vs ExifTool:\n{}\n\nIf intentional, regenerate with \
         UPDATE_MULTISET_BASELINE=1.",
        regressions.len(),
        regressions
            .iter()
            .take(SHOW)
            .map(|d| format!("  {}", fmt_multi_delta(d)))
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

// ── Garmin FIT parity, both extraction modes ────────────────────────────────
//
// Without -ee, ExifTool reports only the first message of each type; with -ee it
// reports the whole time series (one set of tags per `Record` message, each in
// its own family-3 document).
//
// Both tests compare the complete "name<TAB>value" multiset against an ExifTool
// 13.59 baseline, volatile system tags excluded. They exist because the main
// harness only covers the default mode: -ee support once shipped broken (5 tags
// per message instead of 13) without any test noticing.

/// Tags whose value depends on the machine, the clock or the crate version.
#[cfg(unix)]
const FIT_VOLATILE_TAGS: &[&str] = &[
    "Directory",
    "ExifToolVersion",
    "FileAccessDate",
    "FileInodeChangeDate",
    "FileModifyDate",
    "FileName",
    "FilePermissions",
    "FileSize",
];

/// Asserts that `Garmin.fit` extracted with the given `extract_embedded` setting
/// yields exactly the `name<TAB>value` multiset recorded in `baseline`.
///
/// On failure, reports the tag counts and the actual offending pairs rather than
/// dumping two sorted lists of ~180 lines.
#[cfg(unix)]
fn assert_fit_parity(extract_embedded: u8, baseline: &str, mode: &str) {
    use exiftool_rs::Options;
    use std::collections::BTreeMap;

    // FIT record timestamps (GPSDateTime/TimeStamp) are rendered in the machine's
    // local time — the baseline was frozen in one zone, so their offset differs on
    // any other zone (e.g. UTC CI runners). Drop them from BOTH sides here, the same
    // way the machine-state File* dates are already excluded; this stays FIT-local
    // and does not touch how the shared multiset test compares GPSDateTime elsewhere
    // (where it is UTC and portable). The instant is still parsed; only its
    // non-portable zone rendering is not value-compared.
    let is_local_time_fit = |name: &str| name == "GPSDateTime" || name == "TimeStamp";

    let expected = std::fs::read_to_string(baseline)
        .unwrap_or_else(|e| panic!("missing baseline {baseline}: {e}"));
    let mut want: Vec<String> = expected
        .lines()
        .filter(|l| !is_local_time_fit(l.split('\t').next().unwrap_or("")))
        .map(|l| l.to_string())
        .collect();
    want.sort();

    let et = ExifTool::with_options(Options {
        extract_embedded,
        ..Default::default()
    });
    let tags = et
        .extract_info(Path::new("tests/images/Garmin.fit"))
        .expect("FIT extraction");

    let mut got: Vec<String> = tags
        .iter()
        .filter(|t| !FIT_VOLATILE_TAGS.contains(&t.name.as_str()) && !is_local_time_fit(&t.name))
        .map(|t| format!("{}\t{}", t.name, sanitize_value(&t.print_value)))
        .collect();
    got.sort();

    if got == want {
        return;
    }

    // Multiset difference: a tag repeated N times upstream must appear N times
    // here too, so count occurrences rather than comparing sets.
    let mut delta: BTreeMap<&str, i32> = BTreeMap::new();
    for line in &got {
        *delta.entry(line.as_str()).or_default() += 1;
    }
    for line in &want {
        *delta.entry(line.as_str()).or_default() -= 1;
    }

    let mut unexpected: Vec<String> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for (line, count) in &delta {
        let target = if *count > 0 {
            &mut unexpected
        } else {
            &mut missing
        };
        for _ in 0..count.abs() {
            target.push((*line).replace('\t', " = "));
        }
    }

    const SHOW: usize = 8;
    let sample = |label: &str, list: &[String]| -> String {
        if list.is_empty() {
            return String::new();
        }
        let mut s = format!("\n  {} {}:", list.len(), label);
        for line in list.iter().take(SHOW) {
            s.push_str(&format!("\n    {line}"));
        }
        if list.len() > SHOW {
            s.push_str(&format!("\n    ... and {} more", list.len() - SHOW));
        }
        s
    };

    panic!(
        "Garmin.fit parity broken in {mode} mode: {} tags extracted, {} expected; \
         {} name/value pairs differ.{}{}",
        got.len(),
        want.len(),
        unexpected.len() + missing.len(),
        sample("unexpected (we emit, ExifTool does not)", &unexpected),
        sample("missing (ExifTool emits, we do not)", &missing),
    );
}

#[cfg(unix)]
#[test]
fn fit_default_parity() {
    assert_fit_parity(0, "tests/expected_values/Garmin.fit.vals", "default");
}

#[cfg(unix)]
#[test]
fn fit_extract_embedded_parity() {
    assert_fit_parity(1, "tests/expected_values/Garmin.fit.ee.vals", "-ee");
}
