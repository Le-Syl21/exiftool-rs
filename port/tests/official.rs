//! ExifTool's own test scripts (t/*.t), run unchanged by the interpreter on
//! top of the runtime.

use exiftool_port::interp::{Scope, Want};

/// Run one test script from the ExifTool checkout; returns its output.
fn run_t(name: &str) -> (String, Option<i32>) {
    let root = format!("{}/../../exiftool", env!("CARGO_MANIFEST_DIR"));
    std::env::set_current_dir(&root).unwrap();
    let src = std::fs::read(format!("t/{name}.t")).unwrap();
    let mut it = exiftool_port::runtime::new_interp();
    let scope = Scope::new(None);
    it.strict = false;
    let r = it.eval_string(&src, "main", &scope, Want::Void);
    if let Err(e) = &r {
        if it.exit_code.is_none() {
            it.stderr.extend_from_slice(format!("died: {}", e.to_text()).as_bytes());
        }
    }
    let _ = it.run_end_blocks();
    it.flush_handles();
    let mut out = String::from_utf8_lossy(&it.stdout).into_owned();
    if !it.stderr.is_empty() {
        out.push_str("\n--- stderr ---\n");
        out.push_str(&String::from_utf8_lossy(&it.stderr));
    }
    (out, it.exit_code)
}

fn passed(out: &str) -> bool {
    let plan = out.lines().next().and_then(|l| l.strip_prefix("1..")).and_then(|n| n.trim().parse::<usize>().ok());
    let oks = out.lines().filter(|l| l.starts_with("ok ")).count();
    plan.is_some_and(|n| n == oks) && !out.lines().any(|l| l.starts_with("not ok"))
}

#[test]
fn bmp() {
    let (out, code) = run_t("BMP");
    assert!(passed(&out) && code.unwrap_or(0) == 0, "t/BMP.t:\n{out}");
}
