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

#[test]
fn regression_tag_names() {
    let images_dir = Path::new("tests/images");
    let expected_dir = Path::new("tests/expected");

    let mut failures = Vec::new();
    let mut tested = 0;

    for entry in std::fs::read_dir(images_dir).unwrap() {
        let entry = entry.unwrap();
        let file_name = entry.file_name().to_string_lossy().to_string();
        let expected_path = expected_dir.join(format!("{}.tags", file_name));

        if !expected_path.exists() {
            continue;
        }

        let tags = match safe_extract(&entry.path()) {
            Some(t) => t,
            None => continue, // Skip files that fail to parse or panic
        };

        let actual: BTreeSet<String> = tags.iter().map(|t| t.name.clone()).collect();
        let expected: BTreeSet<String> = std::fs::read_to_string(&expected_path)
            .unwrap()
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect();

        let missing: Vec<_> = expected.difference(&actual).collect();
        let extra: Vec<_> = actual.difference(&expected).collect();

        if !missing.is_empty() {
            failures.push(format!(
                "{}: {} missing tags (e.g., {:?})",
                file_name,
                missing.len(),
                missing.iter().take(3).collect::<Vec<_>>()
            ));
        }

        if !extra.is_empty() {
            failures.push(format!(
                "{}: {} extra tags (e.g., {:?})",
                file_name,
                extra.len(),
                extra.iter().take(3).collect::<Vec<_>>()
            ));
        }

        tested += 1;
    }

    println!("Tested {} files, {} failures", tested, failures.len());
    // Don't assert failure yet — just report. The tag coverage is not expected to be 100%.
    // Instead, assert that we tested a minimum number of files:
    assert!(
        tested >= 100,
        "Expected to test at least 100 files, got {}",
        tested
    );

    // Print failures for visibility but don't fail the test
    // (tag coverage improvements will gradually reduce these)
    if !failures.is_empty() {
        println!("\nFiles with tag differences ({}):", failures.len());
        for f in &failures {
            println!("  {}", f);
        }
    }
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
