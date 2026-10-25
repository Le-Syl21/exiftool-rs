# Porting a Perl file to Rust

This is the method every port follows. The reference example is
`port/src/modules/gps.rs`, the port of `GPS.pm`: read it before starting.

## How the port runs

The crate in `port/` runs ExifTool. A module not ported yet runs from its
Perl source in the crate's Perl interpreter; each sub ported to Rust is
registered as a *native* under its Perl name and replaces the Perl version
everywhere, including in tables and in references taken earlier. Perl code
calls natives and natives call Perl code the same way, so a file can be
ported one sub at a time and stay correct at every step.

ExifTool's whole test suite runs unchanged on top of this (114 scripts,
603 tests). It passes today, and it must still pass after every ported sub.

## The rules

1. **Port, don't rewrite.** Each Perl sub becomes one Rust function that
   follows it line by line: same steps, same order, same variable names (in
   snake_case), same comments. Don't simplify, don't fix bugs, don't skip
   branches you think are unused. If the Perl does something odd, the Rust
   does the same odd thing.
2. **Perl semantics through the helpers.** Values are `perl::Value`, Perl
   scalars with their string/number duality. Every operation on them goes
   through `crate::pl` (and `perl::hv` for `$$h{k}` / `$$a[i]`): `pl::add`,
   `pl::lt`, `pl::concat`, `pl::sprintf`, `pl::re(...).m/subst/all/each`,
   `pl::split`... Never do Rust arithmetic, comparisons or `format!` on Perl
   values: `"0.0"` is true, `"12abc" + 1` is 13, numbers print with `%.15g`,
   and the helpers know it.
3. **The signature.** Every ported sub is
   `pub fn name(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>>`
   where `a` is `@_`, `want` is the calling context (`wantarray`), and the
   result is the returned list (`pl::ret(v)` for a scalar, `pl::ret_nothing(want)`
   for `return;`). Register it in the file's `register` with its full Perl
   name: `it.define_native("Image::ExifTool::GPS::ToDMS", to_dms);`.
4. **Calls.** A sub of the same file that is already ported: call the Rust
   function directly. Anything else: `pl::call(it, "Pkg::Func", args)` or
   `pl::method(it, &et, "Method", args)` (list forms: `call_list`,
   `method_list`). These work whether the target is ported or still Perl.
5. **Data.** The ExifTool object (`$self`/`$et`), tag tables, tagInfo hashes
   and dirInfo hashes stay Perl hashes, as in ExifTool (table strings read
   them directly): `et.get("OPTIONS")`, `tag_info.set("Name", ...)`. A
   file's `my` variables (`my %fileTypeLookup`) are reachable as
   `runtime::lex_hash(it, "Image::ExifTool", "fileTypeLookup")` (also
   `lex_array`, `lex_scalar`), whether the module is interpreted or not.
6. **Perl's special behaviours** that ported code must keep:
   - `$_[0] = ...` writes the caller's variable: `it.set_arg(0, v)?`.
   - `die` returns `Err(Flow::Die(..))`: use `interp::eval::die(msg)`; an
     `eval { }` is a match on the `ER` result, setting `$@`
     (`it.set_scalar("main::@", ...)`) as perl does.
   - `warn` goes through `$SIG{__WARN__}` (ExifTool collects warnings that
     way): `pl::warn(it, msg)`.
   - `local $x`: save, set, and restore on every exit path, errors included.
   - `eval $string` sites (Condition, RawConv, ValueConv...): run the string
     with `it.eval_string(code, package, &scope, want)`, with a `Scope`
     holding exactly the variables the Perl eval site has in scope (the
     `#### eval` comments list them), so table strings see `$val`, `$self`,
     `@val`... under the same names.
7. **Your files only.** Edit only the Rust files of your Perl file (they are
   already created and registered). Don't change `src/interp`, `src/perl`,
   `src/runtime`, `src/pl.rs` or other modules; if you need something there,
   write a local helper in your file and say so in your report.
8. **English** for code, comments and commit messages. Commits end with the
   two trailer lines given to you.

## Helpers that already exist

Use these instead of writing your own (the pilot ports each wrote some of
them; they are now shared):

| Perl | Rust |
|---|---|
| `$raf->Read($buff, $len)` and any call that writes its arguments | `pl::method_mut(it, &raf, "Read", &mut [buff, len], want)` / `pl::call_mut` |
| `ref $obj` (blessed class) | `pl::ref_of` |
| `substr`, `%`, `join`, `lc`, `uc`, `ucfirst`, `tr///`, `pack`, `unpack` | `pl::substr`, `pl::modulo`, `pl::join`, `pl::lc`..., `pl::tr`, `pl::pack`, `pl::unpack` / `unpack1` |
| `%Pkg::x`, `@Pkg::x` | `pl::pkg_hash`, `pl::pkg_array` |
| `\$v`, `\$$h{k}`, `$$ref = v`, `&$code(...)` | `pl::sref`, `pl::elem_ref`, `pl::set_deref`, `pl::call_code` |
| `local $_`, `local $X`, `local $SIG{__WARN__}` | `pl::local_topic`, `pl::local_global`, `pl::local_sig_warn` |
| `eval { ... }` | `pl::eval_block` (sets `$@`) |
| `eval $string` at an eval site | `pl::eval_site_in(it, code, pkg, Some(runtime::file_scope(it, MODULE)), &[("val", v), ("self", et), ("@val", arr)], want)` |
| `print $fh LIST` | `pl::print_to` |
| `Get16u`, `Get32u`, `ReadValue`, `SetByteOrder`... | already ported: call `crate::modules::exiftool::part2::get16u(...)` etc. directly |

## Before you start

Your worktree may have been created from `main`, which has no `port/`:
if so, run `git reset --hard perl-port` first (the worktree is clean).

## Checking a port

All commands take the ExifTool checkout explicitly; use
`/home/sylvain/dev/exiftool`. Build with `-j2`.

1. `cargo build --release --example run_t -j2` in `port/`: no errors, no
   warnings.
2. **The suite**: `sh scripts/run_suite.sh /home/sylvain/dev/exiftool` must
   print `114 scripts, 114 pass, 603/603 tests ok`.
3. **Every test image**: `sh scripts/compare_images.sh /home/sylvain/dev/exiftool`
   must print `194 identical, 0 different` (everything extracted, printed
   and raw, duplicates and unknown tags included, perl against the port).
4. **Function oracle** for subs that compute values: write cases (one call
   per line, tab-separated, see `scripts/fn_oracle.pl`) covering the
   branches, run the script under perl and under the port, and diff:

       cd /home/sylvain/dev/exiftool
       perl -Ilib PORT/scripts/fn_oracle.pl cases.txt > perl.txt
       EXIFTOOL_LIB=$PWD/lib PORT/target/release/examples/run_t PORT/scripts/fn_oracle.pl cases.txt > port.txt
       diff perl.txt port.txt

5. **Call traces**, when the result differs and you need to find where:
   `scripts/trace_db.pl` logs every ExifTool sub call under perl,
   `PORT_TRACE=file` does the same in the port, and `scripts/trace_diff.py`
   prints the first calls that differ (see the comments in those files).

A port is done when all of these pass and `scripts/port_status.py LIB FILE`
shows every sub of the file ported.

## Who ports what (pilot)

| Agent | Perl file(s) | Rust file(s) |
|---|---|---|
| 1 | ExifTool.pm, subs from line 1 to 4386 (`new` ... `Init`) | `modules/exiftool/part1.rs` |
| 2 | ExifTool.pm, subs from line 4388 to 7254 (`Purge` ... `JpegMarkerName`) | `modules/exiftool/part2.rs` |
| 3 | ExifTool.pm, subs from line 7255 to the end (`DirStart` ... `ProcessBinaryData`) | `modules/exiftool/part3.rs` |
| 4 | Exif.pm | `modules/exif.rs` |
| 5 | XMP.pm, XMPStruct.pl | `modules/xmp.rs`, `modules/xmp_struct.rs` |
| 6 | IPTC.pm, ICC_Profile.pm, Photoshop.pm, PrintIM.pm, Fixup.pm, JPEG.pm, File/RandomAccess.pm | the matching files |
| 7 | MakerNotes.pm, Canon.pm | `modules/maker_notes.rs`, `modules/canon.rs` |

The module-level code of a file (table definitions, `AddCompositeTags`
calls, config-file loading in ExifTool.pm) stays as it is: the tables come
from the Perl, or from the data dumps once a file is ported completely.
