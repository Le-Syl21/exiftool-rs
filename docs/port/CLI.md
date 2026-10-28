# Porting the exiftool application

The `exiftool` script of ExifTool 13.59 (`../exiftool/exiftool`, 5076 lines
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
| `subs_a.rs` | the small subs before the main program (329-399): `Image::ExifTool::EndDir`, `Image::ExifTool::End`, `Exit`, `Warn`, `Error`, `WarnOnce`, `SigInt`, `SigCont`, `Cleanup`; and the subs of 3682-4233 (after SetImageInfo, up to ProcessFiles) |
| `set_image_info.rs` | `SetImageInfo` (3131-3681) |
| `get_image_info.rs` | `GetImageInfo` (2109-3130) |
| `subs_b.rs` | the subs of 4234-5075 (`ProcessFiles` ... `PrintErrors`) |
| `options.rs` | the option chain of the argument loop (690-1449): `parse_option` |
| `program.rs` | the rest of the main program: the BEGIN block and module loading (1-83), the lexicals' initial values (84-328), the command loop (400-689), the end of the argument loop and the processing of the command (1450-2108), as `main::<program>` |

### The contract between `options.rs` and `program.rs`

The argument loop (script lines 625-1471) is `for (;;) { ... }`. Its head
(625-689: the `-execute` and end-of-arguments handling, then `$_ = shift`)
and its tail (1450-1470: a file name) are `program.rs`'s; the option chain
between them (`if (not $endOfOpts and s/^(-|\xe2\x88\x92)//) { ... }`,
lines 690-1449) is

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
- Strings stay byte strings (`Value`), never Rust `String`s for data.
- The script runs with `$^W = 1`: where a statement of the script can warn
  in perl ("Use of uninitialized value..."), keep a comment `// (perl warns
  here if X is undef)` when the port cannot warn yet.

## Judges

From `port/`, all three must pass before every commit:

```sh
sh scripts/run_suite.sh /home/sylvain/dev/exiftool       # 114 scripts, 114 pass, 603/603 tests ok
sh scripts/compare_images.sh /home/sylvain/dev/exiftool  # 194 identical, 0 different
python3 -I scripts/compare_cli.py /home/sylvain/dev/exiftool   # no new differences
```

`compare_cli.py` runs `scripts/cli_cases.txt` (`CLI_CASES=FILE` for one's
own cases) with Perl's exiftool and with `run_t exiftool`, each in a copy of
the images, and compares output, exit status and written files. Its known
differences today (to keep, not to add to): `-lang fr` (character strings),
`-p ... -q t/images/` ("Use of uninitialized value" warnings), `-ver -v`
(the Perl installation).
