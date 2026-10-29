# Porting the exiftool application

The `exiftool` script of ExifTool 13.60 (`../exiftool/exiftool`, 5082 lines
before `__END__`) is ported to Rust in `port/src/cli/`, with the method of
PORTING.md: faithful, sub by sub, judged at every step against Perl.

## How the port runs while it is under way

`run_t /path/to/exiftool ARGS` runs the script itself in the port's
interpreter (no Perl needed: the library is built in). For that program:

- `cli::register` defines the ported subs as natives (`main::GetImageInfo`,
  `Image::ExifTool::EndDir`...) with `cli::define`; a native defined that way
  wins over the script's own definition of the sub.
- The script's file lexicals (`my @tags`, `my $outFormat`... lines 84-328)
  are also the package variables `main::<my>::NAME`: the script and the
  natives see the same variables. A native reads them with
  `cli::lex_scalar(it, "outFormat")`, `cli::lex_array(it, "tags")`,
  `cli::lex_hash(it, "printFmt")`. `$_`, `@ARGV`, `$mt` (a lexical too),
  `$SIG{...}` are what the script has at that moment.
- A sub that is not ported yet is still the script's Perl, called with
  `pl::call(it, "main::Name", args)` like any other.

`main::<program>` (the main program, below) runs in place of the script's
top-level code with `EXIFTOOL_NATIVE_MAIN=1`. The application itself is the
binary `exiftool-rs` (`cargo build --release --example exiftool-rs`), named
as the crate's binary on main so that it is never mistaken for Perl's
`exiftool`: it reads no Perl file. `PORT_BIN=target/release/examples/exiftool-rs`
makes the CLI judge run it.

## The parts

| File | Ports (script lines) |
|---|---|
| `subs_a.rs` | the small subs before the main program (329-399): `Image::ExifTool::EndDir`, `Image::ExifTool::End`, `Exit`, `Warn`, `Error`, `WarnOnce`, `SigInt`, `SigCont`, `Cleanup`; and the subs of 3684-4235 (after SetImageInfo, up to ProcessFiles) |
| `set_image_info.rs` | `SetImageInfo` (3133-3683) |
| `get_image_info.rs` | `GetImageInfo` (2111-3132) |
| `subs_b.rs` | the subs of 4236-5082 (`ProcessFiles` ... `PrintErrors`) |
| `options.rs` | the option chain of the argument loop (691-1451): `parse_option` |
| `program.rs` | the rest of the main program: the BEGIN block and module loading (1-83), the lexicals' initial values (84-328), the command loop (400-690), the end of the argument loop and the processing of the command (1452-2110), as `main::<program>` |

### The contract between `options.rs` and `program.rs`

The argument loop (script lines 626-1473) is `for (;;) { ... }`. Its head
(626-690: the `-execute` and end-of-arguments handling, then `$_ = shift`)
and its tail (1452-1472: a file name) are `program.rs`'s; the option chain
between them (`if (not $endOfOpts and s/^(-|\xe2\x88\x92)//) { ... }`,
lines 691-1451) is

```rust
/// One argument that starts with "-" (already in `$_`, the dash removed by
/// the caller as the script does): returns true for the script's `next`
/// (go to the next argument), false when it falls through to the end of
/// the loop's body.
pub fn parse_option(it: &mut Interp) -> ER<bool>
```

The only control statement of the chain that leaves it is `next` (140 of
them); `$pass`, `@nextPass`, `$endOfOpts`, `$badCmd` are file lexicals.
`program.rs` calls `options::parse_option` after removing the dash, exactly
where the script's `if` is.

## Rules (in addition to PORTING.md)

- Port line by line, in script order, keeping the script's comments (in
  English, as they are) and naming each Rust function after its Perl sub.
- A native receives the sub's `@_` and returns its return list, as every
  native (`fn(&mut Interp, Vec<Value>, Want) -> ER<Vec<Value>>`); a sub that
  changes `$_[N]` writes it back with `it.set_arg`.
- `print`, `printf`, `warn`, `die`, `open`, `binmode`, `exit` behave as perl's:
  use the interpreter's (`pl::print_to`, `pl::warn`...), never `println!` or
  `std::process::exit` (the script's `exit` ends the program through the
  interpreter, which runs the END blocks).
- Strings stay byte strings (`Value`), never Rust `String`s for data. A
  character string (perl's UTF-8 flag, from `Encode::decode_utf8`,
  `pack('U0C*')`, `chr(0x100)`...) is `Value::UStr`, held in UTF-8: the
  `pl` helpers (`length`, `substr`, `lc`, `join`, `sprintf`, `re`...) count
  and match its characters, as perl does.
- The script runs with `$^W = 1`: where a statement of the script can warn
  in perl ("Use of uninitialized value...", "Argument ... isn't numeric"),
  the interpreter warns as perl does in Perl code; a native that must give
  the warning calls `pl::warn_uninit(it, &v, "$name", "op")` or
  `pl::warn_num(it, &v, "", "addition (+)")` (with `pl::at(LINE)`), and
  otherwise keeps a comment `// (perl warns here if X is undef)`.

## Judges

From `port/`, all three must pass before every commit:

```sh
sh scripts/run_suite.sh /home/sylvain/dev/exiftool       # 114 scripts, 114 pass, 604/604 tests ok
sh scripts/compare_images.sh /home/sylvain/dev/exiftool  # 194 identical, 0 different
python3 -I scripts/compare_cli.py /home/sylvain/dev/exiftool   # no new differences
```

`compare_cli.py` runs `scripts/cli_cases.txt` (`CLI_CASES=FILE` for one's
own cases) with Perl's exiftool and with `run_t exiftool`, each in a copy of
the images, and compares output, exit status and written files. It has no
known difference: the cases whose output is the port's own by design, or
needs a Perl module the host's Perl lacks, are told apart by their kind:

- `S` (`-ver -v`, case 108): checked against the port's own golden output,
  `scripts/cli_golden/<args>.txt`, with the version numbers (and the
  target) masked but ExifTool's and perl's, not against Perl
  (`CLI_GOLDEN_UPDATE=1` rewrites the golden files after a deliberate
  change). It is the program's own output, so it runs with `PORT_BIN`
  only (under `run_t exiftool`, the script prints its own and the case is
  skipped).
- `RM`/`WM`: Perl needs Compress::Raw::Lzma or Unicode::LineBreak, which
  the port provides and the host's Perl lacks; the Perl side runs in the
  container of the second judge set below (`PERL_FULL_CMD` to run it
  otherwise). Today, the plain output in Japanese and Chinese, whose
  columns Unicode::GCString aligns (the host's Perl counts characters):
  `-lang ja -G1 -a` and `-lang zh_cn -G1 -a` on Canon.jpg (cli_cases.txt,
  cases 53 and 57), `-lang zh-CN -Model` (cli_cases_options.txt) and
  `-lang ja -a -G1` (cli_cases_subs_a.txt). The other languages' plain
  output is the same either way (a character, a column).

### `-ver -v`

`-ver` alone prints `13.60` as Perl does (scripts read it); with `-v` the
lines that describe perl and its optional modules are the port's own:

```
exiftool-rs version 0.0.0 (port of ExifTool 13.60)
Rust version 1.99.1 (x86_64-unknown-linux-gnu)
Platform: linux
Included libraries:
  Perl interpreter             own Rust implementation (perl 5.38 semantics)
  Perl regular expressions     own Rust implementation, based on pcre2 0.2.11 (PCRE2 10.46, C, static)
  ...
  Memory allocator             rusty_alloc 2.2.7
```

Nothing there is written by hand: build.rs reads the crates' versions in
Cargo.lock (followed from this package, so the versions actually built),
the compiler's from `rustc -V` and the target from Cargo; PCRE2's and
zlib's versions are asked of the libraries (`pcre2_config`,
`zlibVersion`), and "static" or "shared" is where their code is (dladdr:
in the executable, or in a system library a build script picked). The
allocator is the program's (`build_info::set_allocator`, which
`exiftool-rs` and `run_t` call; the library alone lists none). The perl
version is the one the port follows, that of the judges' Perl (the
interpreter's `$]` still says 5.036). Perl's other lines (`-v9`: Current
Dir, Script Name...; `-v2`: Include directories) are kept.

### The second judge set: the optional modules

The host's Perl lacks Compress::Raw::Lzma and Unicode::LineBreak, which the
port provides (src/modules/lzma.rs, gcstring.rs). Their reference is the
Perl of `scripts/full_modules/Dockerfile` (Ubuntu 24.04's perl 5.38 and
packaged modules, as the host, plus those two), run by
`scripts/full_modules/perl.sh` with the case's directory and ExifTool's
tree mounted at their own paths:

```sh
docker build -t exiftool-rs-fullperl:local scripts/full_modules
PERL_CMD=scripts/full_modules/perl.sh CLI_CASES=scripts/cli_cases_full_modules.txt \
    python3 -I scripts/compare_cli.py /home/sylvain/dev/exiftool
```

`scripts/cli_cases_full_modules.txt` covers 7z archives with an encoded
header (`scripts/full_modules/7z`, made with `7z a -mhc=on`: several files,
a directory, UTF-8 names, empty files, stored and bzip2 data, 300 files)
and the plain output in ja, zh_cn, zh_tw, ko, ru, fr and de.

What the two modules follow, and where they part from Perl's:

- Compress::Raw::Lzma: the raw LZMA1 decoder 7Z.pm uses is the port's own
  (lzma.rs says why no crate would do). On the headers of the test
  archives cut at every length tried, decoded in one call or two, zeros,
  random and corrupted data, its output, status and leftover input are
  liblzma's (xz 5.4.5). A bad filter option dies with perl's message
  (`Lc + Lp must be <= 4`), at the caller's line where perl names a line
  of Lzma.pm. As in Perl, an archive whose encoded header is over 32 KB
  compressed never ends: 7Z.pm gives each 32 KB block to a new decoder,
  the second fails, and its read loop waits for bytes that never come (a
  3000-file archive does it, so none is a case).
- Unicode::GCString: `columns` on unicode-width; gcstring.rs lists the code
  points where it differs from Unicode::LineBreak 2019.001 (whose
  character database is Unicode 8.0), none of them in the text of
  ExifTool's languages.

## Languages exiftool-rs adds

`-lang` also takes `ar`, `bn`, `hi` and `pt` (Lang modules made from the
root crate's locales, see API.md "Languages"), and `zh` for `zh_cn`, as
exiftool-rs did before the port. Perl rejects those codes; everything else
`-lang` does is Perl's, the list it prints included (`-lang`, `-lang xx`,
`-listx` show ExifTool's 18 languages, as the judge cases compare them).
