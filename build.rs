use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn main() {
    // (not run again for every change of the sources)
    println!("cargo:rerun-if-changed=build.rs");
    // As on main: the Windows icon/version resource, for the executables
    // (exiftool-rs, exiftool-rs-gui) only when asked for with the `win-icon`
    // feature, so that a program using the library does not get it.
    #[cfg(all(target_os = "windows", feature = "win-icon"))]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.compile().expect("Failed to compile Windows resources");
    }
    build_info();
}

/// What `exiftool-rs -ver -v` says it was built with (src/build_info.rs):
/// the compiler, the target, and the version of each crate in the build, as
/// Cargo.lock has them, so that no version is written by hand.
fn build_info() {
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let rustc_v = std::process::Command::new(rustc)
        .arg("-V")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    // "rustc 1.99.1 (58cf8f9c3 2026-10-13)": the version
    let rustc_v = rustc_v.split_whitespace().nth(1).unwrap_or("unknown").to_string();
    let target = std::env::var("TARGET").unwrap_or_default();
    let deps = match lock_file() {
        Some(p) => {
            println!("cargo:rerun-if-changed={}", p.display());
            lock_versions(&std::fs::read_to_string(&p).unwrap_or_default())
        }
        None => Vec::new(),
    };
    let mut src = format!("pub const RUSTC_VERSION: &str = {rustc_v:?};\npub const TARGET: &str = {target:?};\npub const DEPS: &[(&str, &str)] = &[\n");
    for (name, ver) in deps {
        src += &format!("    ({name:?}, {ver:?}),\n");
    }
    src += "];\n";
    std::fs::write(out.join("build_info.rs"), src).unwrap();
}

/// The Cargo.lock of the build: this package's, or (as a dependency of
/// another one) the workspace's, above the target directory.
fn lock_file() -> Option<PathBuf> {
    let own = Path::new(&std::env::var_os("CARGO_MANIFEST_DIR")?).join("Cargo.lock");
    if own.exists() {
        return Some(own);
    }
    let out = PathBuf::from(std::env::var_os("OUT_DIR")?);
    out.ancestors().map(|d| d.join("Cargo.lock")).find(|p| p.exists())
}

/// The crates this package depends on, directly or not, with their version:
/// Cargo.lock's packages, followed from this one by its dependency lists
/// (whose "name version" entries tell two versions of a crate apart).
fn lock_versions(lock: &str) -> Vec<(String, String)> {
    struct Pkg {
        name: String,
        version: String,
        deps: Vec<String>,
    }
    let mut pkgs: Vec<Pkg> = Vec::new();
    let mut in_deps = false;
    for line in lock.lines() {
        let l = line.trim();
        if l == "[[package]]" {
            pkgs.push(Pkg { name: String::new(), version: String::new(), deps: Vec::new() });
            in_deps = false;
        } else if let Some(p) = pkgs.last_mut() {
            let q = |s: &str| s.trim().trim_matches(|c| c == '"' || c == ',').to_string();
            if let Some(v) = l.strip_prefix("name = ") {
                p.name = q(v);
            } else if let Some(v) = l.strip_prefix("version = ") {
                p.version = q(v);
            } else if l.starts_with("dependencies = [") {
                in_deps = !l.ends_with(']');
            } else if in_deps {
                if l == "]" {
                    in_deps = false;
                } else {
                    p.deps.push(q(l));
                }
            }
        }
    }
    let find = |entry: &str| -> Option<usize> {
        let mut it = entry.split_whitespace();
        let (name, ver) = (it.next()?, it.next());
        pkgs.iter().position(|p| p.name == name && ver.is_none_or(|v| p.version == v))
    };
    let me = std::env::var("CARGO_PKG_NAME").unwrap_or_default();
    let mut queue: Vec<usize> = pkgs.iter().position(|p| p.name == me).into_iter().collect();
    let mut seen: HashMap<String, String> = HashMap::new();
    let mut done = vec![false; pkgs.len()];
    let mut out = Vec::new();
    // breadth first: where two versions are in the build, the one nearer
    // to this package
    while !queue.is_empty() {
        let mut next = Vec::new();
        for i in queue {
            if std::mem::replace(&mut done[i], true) {
                continue;
            }
            for d in &pkgs[i].deps {
                if let Some(j) = find(d) {
                    if !seen.contains_key(&pkgs[j].name) {
                        seen.insert(pkgs[j].name.clone(), pkgs[j].version.clone());
                        out.push((pkgs[j].name.clone(), pkgs[j].version.clone()));
                    }
                    next.push(j);
                }
            }
        }
        queue = next;
    }
    out
}
