# Porting a Perl file to Rust

This is the method every port follows. The reference example is
`src/modules/gps.rs`, the port of `GPS.pm`: read it before starting.

## How the port runs

The crate runs ExifTool. A module not ported yet runs from its
Perl source in the crate's Perl interpreter; each sub ported to Rust is
registered as a *native* under its Perl name and replaces the Perl version
everywhere, including in tables and in references taken earlier. Perl code
calls natives and natives call Perl code the same way, so a file can be
ported one sub at a time and stay correct at every step.

ExifTool's whole test suite runs unchanged on top of this (114 scripts,
604 tests). It passes today, and it must still pass after every ported sub.

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
| `$$r`, `@$r`, `%$r` read under strict refs | `pl::sderef`, `pl::aderef`, `pl::hderef` (see "Faithful errors") |
| the line of the Perl statement (for message locations) | `pl::at(LINE)` |

## Faithful errors: dereferences and locations

Perl's errors are part of what ExifTool does (an `eval` turns them into
warnings, or a file stops being read), so a port gives the same ones.

**Dereferencing.** ExifTool's modules `use strict`. Under it, perl:
- dies on `$$r`, `@$r`, `%$r` (a whole-container or scalar read) when `$r`
  is undef ("Can't use an undefined value as a SCALAR reference" / "as an
  ARRAY reference" / "as a HASH reference"), a string (the "strict refs"
  message), or another kind of reference ("Not an ARRAY reference"):
  use `pl::sderef(&r)?`, `pl::aderef(&r)?`, `pl::hderef(&r)?`;
- does **not** die but autovivifies `$r` (when `$r` is a variable or an
  element) for an element access `$$r{k}`, `$r->[i]`, `exists $$r{k}`,
  for `keys %$r`, `push @$r, ...`, `foreach (@$r)`, `f(@$r)` (sub
  arguments are lvalues), and a few more;
- does not die on `defined $$r`.
These rules have exceptions: check each case you touch with a one-line
`perl -Mstrict -e '...'` before porting it. `Value::deref()` and friends
return undef silently: keep them only where perl does not die.

**Locations.** A native runs at the location of its Perl sub: messages
that perl ends with " at FILE line N." (a `die`/`warn` without a final
newline, the strict dereference errors, "Illegal division by zero",
"Undefined subroutine", a method called on undef...) name the module's
file and, by default, the line of the `sub`. Before each statement that
can produce such a message, call `pl::at(LINE)` with the line of that
statement in the Perl source, so the message names the same line as
perl's. Statements that cannot die need nothing.

## Before you start

Your worktree may have been created from a branch that does not have
the port yet (before 1.0, the port was in `port/`, next to the old
crate): if so, ask which branch to start from.

## Checking a port

All commands take the ExifTool checkout explicitly; use
`/home/sylvain/dev/exiftool`, a tree of the release the port follows
(`EXIFTOOL_VERSION`; UPGRADING.md for a new one). Build with `-j2`.

1. `cargo build --release --example run_t -j2`: no errors, no
   warnings.
2. **The suite**: `sh scripts/run_suite.sh /home/sylvain/dev/exiftool` must
   print `114 scripts, 114 pass, 604/604 tests ok`.
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
