//! The runtime: loading ExifTool's modules as perl does, with their data
//! from the dumps and their subs from the Rust ports.
//!
//! `require Image::ExifTool::BMP` loads the modules BMP needs, then BMP's
//! package variables, registers its ported subs, and replays what its
//! top-level code did (AddCompositeTags). `%INC` records it, as in perl.

pub mod pld;

use std::rc::Rc;

use crate::interp::eval::{die, Binding, ER};
use crate::interp::{Interp, Want};
use crate::perl::Value;

#[path = "../../data/index.rs"]
mod index;

/// A module's ported subs.
pub type Registrar = fn(&mut Interp);

/// The Rust ports, by module.
fn registrar(module: &str) -> Option<Registrar> {
    crate::modules::REGISTRARS.iter().find(|(m, _)| *m == module).map(|(_, r)| *r)
}

fn module_data(module: &str) -> Option<&'static [u8]> {
    index::MODULES.iter().find(|(m, _)| *m == module).map(|(_, d)| *d)
}

/// The `%INC` key of a module: Image::ExifTool::BMP is Image/ExifTool/BMP.pm.
fn inc_key(module: &str) -> String {
    format!("{}.pm", module.replace("::", "/"))
}

/// A file lexical of a module (`my %moduleName` in ExifTool.pm), kept by
/// the loader in the unreachable package `MODULE::<my>`.
pub fn lexical(it: &mut Interp, sigil: u8, module: &str, name: &str) -> Binding {
    it.global(sigil, &format!("{module}::<my>::{name}"))
}

pub fn lex_hash(it: &mut Interp, module: &str, name: &str) -> crate::perl::Value {
    match lexical(it, b'%', module, name) {
        Binding::Hash(h) => crate::perl::Value::HashRef(h),
        _ => unreachable!(),
    }
}

pub fn lex_array(it: &mut Interp, module: &str, name: &str) -> crate::perl::Value {
    match lexical(it, b'@', module, name) {
        Binding::Array(a) => crate::perl::Value::ArrayRef(a),
        _ => unreachable!(),
    }
}

pub fn lex_scalar(it: &mut Interp, module: &str, name: &str) -> crate::interp::eval::Cell {
    match lexical(it, b'$', module, name) {
        Binding::Scalar(c) => c,
        _ => unreachable!(),
    }
}

/// A scope holding a module's file lexicals (`my` variables at file level),
/// for eval sites: table strings see them as perl's eval does.
pub fn file_scope(it: &Interp, module: &str) -> std::rc::Rc<crate::interp::Scope> {
    let scope = crate::interp::Scope::new(None);
    let prefix = format!("{module}::<my>::");
    for (sigil, name, b) in it.globals_with_prefix(&prefix) {
        scope.declare(sigil, &name[prefix.len()..], b);
    }
    scope
}

/// A new interpreter with the runtime installed.
pub fn new_interp() -> Interp {
    let mut it = Interp::new();
    it.require_hook = Some(Rc::new(|it: &mut Interp, m: &str| require(it, m)));
    install_core(&mut it);
    crate::modules::core::register(&mut it);
    crate::modules::zlib::register(&mut it);
    crate::modules::archive_zip::register(&mut it);
    crate::modules::brotli::register(&mut it);
    it
}

/// `require MODULE` (or a file).
pub fn require(it: &mut Interp, what: &str) -> ER<()> {
    let is_file = what.contains('/') || what.ends_with(".pm") || what.ends_with(".pl");
    let key = if is_file { what.to_string() } else { inc_key(what) };
    let inc = match it.global(b'%', "main::INC") {
        Binding::Hash(h) => h,
        _ => unreachable!(),
    };
    if inc.borrow().contains(key.as_bytes()) {
        return Ok(());
    }
    if is_file {
        return require_file(it, what, &key);
    }
    // Pragmas and modules perl provides: nothing to load.
    if matches!(what, "strict" | "warnings" | "vars" | "Exporter" | "integer" | "utf8" | "bytes" | "constant" | "lib" | "POSIX" | "Encode" | "Time::Local" | "Scalar::Util" | "Digest::MD5" | "Digest::SHA" | "IO::File" | "File::Basename" | "Cwd" | "Carp" | "overload" | "Config" | "feature" | "File::Spec" | "File::Find" | "Unicode::Normalize" | "File::Glob" | "Time::HiRes" | "Win32" | "Sys::Hostname" | "Compress::Zlib" | "IO::Uncompress::RawInflate" | "IO::Uncompress::Gunzip" | "IO::Compress::RawDeflate" | "Archive::Zip" | "IO::Compress::Brotli" | "IO::Uncompress::Brotli") {
        inc.borrow_mut().insert(key.as_bytes(), Value::str(&key));
        return Ok(());
    }
    // A module not ported yet runs from its Perl source; the subs ported so
    // far replace their Perl versions once it is loaded.
    if !crate::modules::is_ported(what) {
        if let Some(path) = source_path(&key) {
            inc.borrow_mut().insert(key.as_bytes(), Value::str(&path));
            run_source(it, &path)?;
            if let Some(r) = registrar(what) {
                r(it);
            }
            return Ok(());
        }
    }
    let Some(data) = module_data(what) else {
        return die(format!("Can't locate {key} in @INC (you may need to install the {what} module) (@INC entries checked: exiftool-rs)"));
    };
    inc.borrow_mut().insert(key.as_bytes(), Value::str(&key));
    // Every module was dumped with Image::ExifTool loaded first.
    if what != "Image::ExifTool" {
        require(it, "Image::ExifTool")?;
    }
    // As perl compiles a module: its `use`d modules and its subs first, then
    // its top-level code (the variables, then the calls it made).
    for dep in pld::requires(data).map_err(|e| crate::interp::Flow::Die(Value::from(format!("loading {what}: {e}\n"))))? {
        require(it, &dep)?;
    }
    if let Some(r) = registrar(what) {
        r(it);
    }
    let md = match pld::load(it, what, data) {
        Ok(m) => m,
        Err(e) => return die(format!("loading {what}: {e}")),
    };
    for (name, args) in md.calls {
        it.call_named(&name, args, Want::Void)?;
    }
    Ok(())
}

/// Where ExifTool's Perl source is, for the modules not ported yet
/// (`EXIFTOOL_LIB`, or the checkout next to this repository).
pub fn perl_lib() -> String {
    std::env::var("EXIFTOOL_LIB").unwrap_or_else(|_| format!("{}/../../exiftool/lib", env!("CARGO_MANIFEST_DIR")))
}

fn source_path(key: &str) -> Option<String> {
    let p = format!("{}/{key}", perl_lib());
    std::path::Path::new(&p).exists().then_some(p)
}

/// Run a module's Perl source, as `require` does.
fn run_source(it: &mut Interp, path: &str) -> ER<()> {
    let src = match std::fs::read(path) {
        Ok(s) => s,
        Err(e) => return die(format!("{path}: {e}")),
    };
    let scope = crate::interp::Scope::new(None);
    let saved = it.strict;
    it.strict = false;
    let r = it.eval_string(&src, "main", &scope, Want::Scalar);
    it.strict = saved;
    // The file's `my` variables, for the module's ported subs: the same
    // names the data loader gives them (MODULE::<my>::name).
    let module = module_of_path(path);
    for (sigil, name, b) in scope.vars() {
        it.set_global(sigil, &format!("{module}::<my>::{name}"), b);
    }
    match r {
        Ok(_) => Ok(()),
        Err(e) => Err(crate::interp::Flow::Die(e)),
    }
}

/// The package a library file defines: .../Image/ExifTool/Exif.pm is
/// Image::ExifTool::Exif, and a Write*.pl file belongs to its module.
fn module_of_path(path: &str) -> String {
    let rel = path.rsplit_once("/lib/").map(|(_, r)| r).unwrap_or(path);
    let rel = rel.trim_end_matches(".pm").trim_end_matches(".pl");
    let m = rel.replace('/', "::");
    match m.strip_prefix("Image::ExifTool::Write") {
        Some("r") => "Image::ExifTool".to_string(),
        Some(sub) if !sub.is_empty() => format!("Image::ExifTool::{sub}"),
        _ => m,
    }
}

/// `require "file.pl"` / `do FILE`: run a Perl source file.
fn require_file(it: &mut Interp, path: &str, key: &str) -> ER<()> {
    let path = if std::path::Path::new(path).exists() { path.to_string() } else { source_path(path).unwrap_or_else(|| path.to_string()) };
    let path = path.as_str();
    let src = match std::fs::read(path) {
        Ok(s) => s,
        Err(_) => return die(format!("Can't locate {path} in @INC")),
    };
    if let Binding::Hash(inc) = it.global(b'%', "main::INC") {
        inc.borrow_mut().insert(key.as_bytes(), Value::str(path));
    }
    let scope = crate::interp::Scope::new(None);
    let saved = it.strict;
    it.strict = false;
    let r = it.eval_string(&src, "main", &scope, Want::Scalar);
    it.strict = saved;
    // The file's `my` variables, under MODULE::<my>::name when the module
    // has none of that name (a Write*.pl file shares its module's package
    // but has its own lexicals: `my $pdfVer` in WritePDF.pl must not hide
    // PDF.pm's), and always under MODULE::<my:FILE>::name.
    let module = module_of_path(path);
    let file = path.rsplit('/').next().unwrap_or(path);
    for (sigil, name, b) in scope.vars() {
        let shared = format!("{module}::<my>::{name}");
        if it.globals_with_prefix(&shared).iter().all(|(s, n, _)| *s != sigil || n != &shared) {
            it.set_global(sigil, &shared, b.clone());
        }
        it.set_global(sigil, &format!("{module}::<my:{file}>::{name}"), b);
    }
    // The ported subs of this file replace the Perl ones it defined.
    if let Some(r) = registrar(&file_key(path)) {
        r(it);
    }
    match r {
        Ok(v) => {
            if !v.last().is_some_and(|v| v.truthy()) {
                return die(format!("{path} did not return a true value"));
            }
            Ok(())
        }
        Err(e) => Err(crate::interp::Flow::Die(e)),
    }
}

/// The registrar key of a library file: its path as a package name
/// (Image/ExifTool/XMPStruct.pl is Image::ExifTool::XMPStruct).
fn file_key(path: &str) -> String {
    let rel = path.rsplit_once("/lib/").map(|(_, r)| r).unwrap_or(path);
    rel.trim_end_matches(".pm").trim_end_matches(".pl").replace('/', "::")
}

/// Subs perl itself provides that ExifTool and its tests use.
fn install_core(it: &mut Interp) {
    // Exporter::import: alias the requested (or default) symbols into the
    // caller's package.
    it.define_native("Exporter::import", |it, args, _| {
        let Some(pkg) = args.first().map(|v| v.to_text()) else { return Ok(vec![]) };
        let caller = it.package.to_string();
        let mut names: Vec<String> = args[1..].iter().map(|v| v.to_text()).collect();
        if names.is_empty() {
            if let Binding::Array(a) = it.global(b'@', &format!("{pkg}::EXPORT")) {
                names = a.borrow().iter().map(|v| v.to_text()).collect();
            }
        }
        let mut expanded = Vec::new();
        for n in names {
            if let Some(tag) = n.strip_prefix(':') {
                if let Binding::Hash(h) = it.global(b'%', &format!("{pkg}::EXPORT_TAGS")) {
                    let list = h.borrow().get(tag.as_bytes()).cloned().unwrap_or_default();
                    expanded.extend(list.items().iter().map(|v| v.to_text()));
                }
            } else {
                expanded.push(n);
            }
        }
        for n in expanded {
            let (sigil, bare) = match n.as_bytes().first() {
                Some(b'$') => (b'$', &n[1..]),
                Some(b'@') => (b'@', &n[1..]),
                Some(b'%') => (b'%', &n[1..]),
                Some(b'&') => (b'&', &n[1..]),
                _ => (b'&', n.as_str()),
            };
            if sigil == b'&' {
                let code = pld::code_by_name(it, &format!("{pkg}::{bare}"));
                if let Value::Code(c) = code {
                    it.define_sub(&format!("{caller}::{bare}"), c);
                }
            } else {
                let b = it.global(sigil, &format!("{pkg}::{bare}"));
                it.set_global(sigil, &format!("{caller}::{bare}"), b);
            }
        }
        Ok(vec![])
    });
    it.define_native("Exporter::export_ok_tags", |it, args, _| {
        // Add the named tags' symbols to @EXPORT_OK of the current package.
        let pkg = it.package.to_string();
        let tags = match it.global(b'%', &format!("{pkg}::EXPORT_TAGS")) {
            Binding::Hash(h) => h,
            _ => unreachable!(),
        };
        let ok = match it.global(b'@', &format!("{pkg}::EXPORT_OK")) {
            Binding::Array(a) => a,
            _ => unreachable!(),
        };
        for t in &args {
            let list = tags.borrow().get(&t.to_bytes()).cloned().unwrap_or_default();
            ok.borrow_mut().extend(list.items());
        }
        Ok(vec![])
    });
    it.define_native("UNIVERSAL::isa", |it, args, _| {
        let target = args.get(1).map(|v| v.to_text()).unwrap_or_default();
        let r = match args.first() {
            Some(Value::HashRef(h)) => match &h.borrow().blessed {
                Some(c) => class_isa(it, c, &target),
                None => target == "HASH",
            },
            Some(Value::ArrayRef(_)) => target == "ARRAY",
            Some(Value::ScalarRef(_)) => target == "SCALAR",
            Some(Value::Code(_)) => target == "CODE",
            Some(Value::Handle(_)) => target == "GLOB",
            Some(v @ Value::Str(_)) => class_isa(it, &v.to_text(), &target),
            _ => false,
        };
        Ok(vec![Value::from(r)])
    });
}

fn class_isa(it: &mut Interp, class: &str, target: &str) -> bool {
    if class == target {
        return true;
    }
    let parents = match it.global(b'@', &format!("{class}::ISA")) {
        Binding::Array(a) => a.borrow().iter().map(|v| v.to_text()).collect::<Vec<_>>(),
        _ => vec![],
    };
    parents.iter().any(|p| class_isa(it, p, target))
}
