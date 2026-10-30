# CLAUDE.md - Project Guide for AI Assistants

## Project Overview

exiftool-rs is a faithful Rust port of Perl ExifTool 13.60: ExifTool's own
modules, dumped and embedded (`data/`), run by a Perl interpreter written in
Rust, with ExifTool's subs and the `exiftool` application ported to Rust one
by one as natives. The output must be ExifTool's, for every command and
every argument.

The reference documentation is `docs/port/`; read it before changing code:

- `docs/port/ARCHITECTURE.md`: the design (runtime, Perl engine, dumps,
  tests, decisions).
- `docs/port/PORTING.md`: the method every port of a Perl file follows,
  and how a port is checked.
- `docs/port/CLI.md`: the `exiftool` application's port and the judges.
- `docs/port/API.md`: the Rust API (`exiftool_rs::ExifTool`...), a facade
  over Image::ExifTool.
- `docs/port/UPGRADING.md`: taking a new ExifTool release.

The Perl source of the release followed is the reference: `$EXIFTOOL_DIR`,
by default `../exiftool` next to this repository. Never edit it.

## Layout

```
src/
├── lib.rs           # crate root: the API's re-exports and the engine's modules
├── main.rs          # exiftool-rs, the application (the port of the `exiftool` script)
├── api/             # the Rust API facade (ExifTool, Options, Tag, Value...)
│   └── extra_writers/  # writers for the types ExifTool reads only (feature extra-writers)
├── cli/             # the `exiftool` script's subs and main program
├── interp/          # the Perl interpreter (parser, evaluator, builtins, I/O)
├── perl/            # Perl's values, hashes, pack, sprintf, tr, regexes (PCRE2)
├── runtime/         # loading the embedded modules (data/), prototypes, sub index
├── modules/         # natives: ExifTool's subs ported to Rust, one file per Perl file
├── build_info.rs    # what -ver -v lists (from build.rs)
└── bin/
    ├── exiftool_rs_gui/  # the GUI (feature gui)
    └── parity/           # the live parity auditor (feature parity)
data/                # the dumps of ExifTool's modules (scripts/dump_all.sh), embedded
examples/            # run_t (runs ExifTool's t/*.t: the judges), API examples
tests/               # integration tests and their fixtures (images, expected outputs)
scripts/             # the judges, the dump and generator scripts
fuzz/                # cargo-fuzz targets (own workspace, nightly)
locales/             # the GUI's texts and the four languages ExifTool lacks
assets/              # icons (GUI window, Windows resource)
```

## Key Commands

Build and test with `-j8` (`-j4` for a release build with the GUI). Every
perl, run_t or exiftool-rs run of the judges goes under `prlimit --as=4000000000 -- timeout N` (the scripts do
it). Scratch files go to `target/scratch/`, never to /tmp.

```bash
cargo build -j8 --release                         # the CLI, target/release/exiftool-rs
cargo build -j8 --release --features gui          # and the GUI, exiftool-rs-gui
EXIFTOOL_DIR=~/dev/exiftool cargo test -j8 --release   # tests (skips those needing ExifTool without it)

# The judges (docs/port/CLI.md), all must pass before a commit:
bash scripts/run_suite.sh ~/dev/exiftool          # 114 scripts, 114 pass, 604/604 tests ok
bash scripts/compare_images.sh ~/dev/exiftool     # 194 identical, 0 different
PORT_BIN=$PWD/target/release/exiftool-rs python3 -I scripts/compare_cli.py ~/dev/exiftool

# Windows type-check from Linux, without mingw:
CC_x86_64_pc_windows_gnu=gcc cargo check -j8 --release --target x86_64-pc-windows-gnu --features gui
```

## Rules

- Faithful port: a Rust function mirrors the Perl sub it ports, its names
  and structure; a difference with Perl's output is a bug of the port
  (PORTING.md).
- `data/` is generated (`scripts/dump_all.sh`, `scripts/gen_extra_langs.pl`):
  never edit it by hand.
- Nothing is named `exiftool` (the binary is `exiftool-rs`), so that it is
  never mistaken for Perl's.
- Code, comments and commit messages in English.
