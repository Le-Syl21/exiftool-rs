//! Non-blocking performance report — tracks parsing speed over time.
//!
//! This is an `#[ignore]`d test: `cargo test` and CI never run it, so it can
//! never fail the build. Run it explicitly to record a data point:
//!
//! ```sh
//! cargo test --release --test perf_report -- --ignored --nocapture
//! ```
//!
//! It micro-benchmarks the host (CPU / RAM / disk) so runs are comparable across
//! machines and over time — a slower row is then attributable to the machine vs.
//! an actual parsing regression. Output:
//!   - `perf/history.tsv`  — one fixed-width row appended per run (for graphing)
//!   - `perf/last-run.txt` — human-readable detail of the latest run
//!
//! Unlike ordinary tests, leaving these files *is* the point — they are the
//! deliverable, not a transient artifact.

use exiftool_rs::ExifTool;
use std::fmt::Write as _;
use std::hint::black_box;
use std::io::Write as _;
use std::path::Path;
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Iterations per file when timing the corpus (best-of is reported).
const REPS: u32 = 50;

// ---- host micro-benchmarks ------------------------------------------------

/// Integer throughput, in millions of ops/sec (a PCG-style LCG kernel).
fn cpu_mops() -> f64 {
    const ITERS: u64 = 300_000_000;
    let mut acc: u64 = 0x1234_5678;
    let start = Instant::now();
    for _ in 0..ITERS {
        acc = black_box(acc)
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
    }
    black_box(acc);
    let secs = start.elapsed().as_secs_f64();
    (ITERS as f64 / secs) / 1.0e6
}

/// Sequential memory bandwidth, in GB/s (256 MB copies).
fn ram_gbps() -> f64 {
    const N: usize = 256 * 1024 * 1024;
    const REPS: usize = 8;
    let src = vec![0xA5u8; N];
    let mut dst = vec![0u8; N];
    let start = Instant::now();
    for _ in 0..REPS {
        dst.copy_from_slice(black_box(&src));
        black_box(&dst);
    }
    let secs = start.elapsed().as_secs_f64();
    (N as f64 * REPS as f64 / secs) / 1.0e9
}

/// Disk write/read throughput in MB/s (write is fsync'd; read is cache-warm).
fn disk_mbps() -> (f64, f64) {
    const N: usize = 128 * 1024 * 1024;
    let buf = vec![0x5Au8; N];
    let path = std::env::temp_dir().join("exiftool_rs_perf_disk.bin");

    let start = Instant::now();
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&buf).unwrap();
        f.flush().unwrap();
        f.sync_all().unwrap();
    }
    let w = (N as f64 / start.elapsed().as_secs_f64()) / 1.0e6;

    let start = Instant::now();
    let read = std::fs::read(&path).unwrap();
    let r = (read.len() as f64 / start.elapsed().as_secs_f64()) / 1.0e6;
    black_box(&read);

    std::fs::remove_file(&path).ok();
    (w, r)
}

// ---- host description -----------------------------------------------------

fn shell(cmd: &str, args: &[&str]) -> Option<String> {
    Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

fn cpu_model() -> String {
    #[cfg(target_os = "linux")]
    if let Ok(info) = std::fs::read_to_string("/proc/cpuinfo") {
        if let Some(line) = info.lines().find(|l| l.starts_with("model name")) {
            if let Some((_, v)) = line.split_once(':') {
                return v.trim().to_string();
            }
        }
    }
    shell("sysctl", &["-n", "machdep.cpu.brand_string"]).unwrap_or_else(|| "unknown".into())
}

fn ram_gb() -> f64 {
    #[cfg(target_os = "linux")]
    if let Ok(info) = std::fs::read_to_string("/proc/meminfo") {
        if let Some(line) = info.lines().find(|l| l.starts_with("MemTotal")) {
            if let Some(kb) = line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse::<f64>().ok())
            {
                return kb / 1024.0 / 1024.0;
            }
        }
    }
    shell("sysctl", &["-n", "hw.memsize"])
        .and_then(|s| s.parse::<f64>().ok())
        .map(|b| b / 1.0e9)
        .unwrap_or(0.0)
}

fn git_commit() -> String {
    shell("git", &["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "nogit".into())
}

// ---- corpus timing --------------------------------------------------------

struct FileTime {
    name: String,
    bytes: u64,
    best_us: u128,
}

fn time_corpus() -> Vec<FileTime> {
    let dir = Path::new("tests/images");
    let mut out = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for e in entries {
        let path = e.path();
        let bytes = e.metadata().map(|m| m.len()).unwrap_or(0);
        // Warm the FS cache so we measure parse cost, not first-touch I/O.
        let _ = ExifTool::new().extract_info(&path);
        let mut best = u128::MAX;
        for _ in 0..REPS {
            let start = Instant::now();
            let _ = black_box(ExifTool::new().extract_info(&path));
            best = best.min(start.elapsed().as_micros());
        }
        out.push(FileTime {
            name: e.file_name().to_string_lossy().into_owned(),
            bytes,
            best_us: best,
        });
    }
    out
}

#[test]
#[ignore = "perf report: run explicitly with --ignored to record a data point"]
fn perf_report() {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let commit = git_commit();
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(0);
    let ram = ram_gb();
    let cpu = cpu_model();

    eprintln!("benchmarking host (cpu/ram/disk)…");
    let mops = cpu_mops();
    let bw = ram_gbps();
    let (dw, dr) = disk_mbps();

    eprintln!("timing corpus parse…");
    let times = time_corpus();
    let files = times.len();
    let total_bytes: u64 = times.iter().map(|t| t.bytes).sum();
    let total_us: u128 = times.iter().map(|t| t.best_us).sum();
    let total_ms = total_us as f64 / 1000.0;
    let mb_per_s = (total_bytes as f64 / 1.0e6) / (total_us as f64 / 1.0e6);

    std::fs::create_dir_all("perf").unwrap();

    // --- append a fixed-width historical row -------------------------------
    let hist = Path::new("perf/history.tsv");
    let new = !hist.exists();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(hist)
        .unwrap();
    if new {
        writeln!(
            f,
            "epoch\tcommit\tos\tarch\tcores\tram_gb\tcpu_mops\tram_gbps\tdisk_w_mbps\tdisk_r_mbps\tfiles\tcorpus_bytes\tparse_ms\tparse_mb_per_s"
        )
        .unwrap();
    }
    writeln!(
        f,
        "{epoch}\t{commit}\t{os}\t{arch}\t{cores}\t{ram:.1}\t{mops:.0}\t{bw:.1}\t{dw:.0}\t{dr:.0}\t{files}\t{total_bytes}\t{total_ms:.2}\t{mb_per_s:.1}"
    )
    .unwrap();

    // --- pretty last-run report -------------------------------------------
    let mut slow = times.iter().collect::<Vec<_>>();
    slow.sort_by_key(|t| std::cmp::Reverse(t.best_us));
    let mut report = String::new();
    let when = shell(
        "date",
        &["-u", "-d", &format!("@{epoch}"), "+%Y-%m-%d %H:%M:%SZ"],
    )
    .or_else(|| {
        shell(
            "date",
            &["-u", "-r", &epoch.to_string(), "+%Y-%m-%d %H:%M:%SZ"],
        )
    })
    .unwrap_or_else(|| format!("epoch {epoch}"));
    let _ = writeln!(report, "exiftool-rs performance report");
    let _ = writeln!(report, "================================");
    let _ = writeln!(report, "when     : {when}  (commit {commit})");
    let _ = writeln!(
        report,
        "host     : {os}/{arch}, {cores} cores, {ram:.1} GB RAM"
    );
    let _ = writeln!(report, "cpu      : {cpu}");
    let _ = writeln!(report);
    let _ = writeln!(
        report,
        "host benchmarks (normalize cross-machine comparison):"
    );
    let _ = writeln!(report, "  cpu    : {mops:>8.0} Mops/s");
    let _ = writeln!(report, "  ram    : {bw:>8.1} GB/s");
    let _ = writeln!(
        report,
        "  disk   : {dw:>8.0} MB/s write   {dr:.0} MB/s read (warm)"
    );
    let _ = writeln!(report);
    let _ = writeln!(
        report,
        "corpus   : {files} files, {:.2} MB total parsed in {total_ms:.2} ms  ({mb_per_s:.1} MB/s)",
        total_bytes as f64 / 1.0e6
    );
    let _ = writeln!(report);
    let _ = writeln!(report, "slowest files (best-of-{REPS} per file):");
    for t in slow.iter().take(15) {
        let _ = writeln!(
            report,
            "  {:>8.3} ms  {:>10} B  {}",
            t.best_us as f64 / 1000.0,
            t.bytes,
            t.name
        );
    }
    std::fs::write("perf/last-run.txt", &report).unwrap();

    // Echo to the console for immediate feedback.
    eprintln!("\n{report}");
    eprintln!("appended → perf/history.tsv");
}
