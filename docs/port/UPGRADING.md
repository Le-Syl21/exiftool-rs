# Moving the port to a new ExifTool release

The port follows one ExifTool release (`EXIFTOOL_VERSION`, today 13.60).
Most of a release comes with the regenerated dumps: the tables and their
Perl strings, the anonymous subs of the tables, the module-level data, the
Lang modules. What does not is the code: every named sub of ExifTool and
of the `exiftool` script runs as a Rust native (`define_native` in
src/modules/, `cli::define` in src/cli/), and the dumps hold no named sub.
So each sub that a release changes or adds is ported by hand, as the first
port was (PORTING.md, CLI.md); a sub added and not ported does not exist
in the port ("Undefined subroutine" where the tables call it).

In the commands below, `OLD` is the ExifTool tree the port follows now
(`~/dev/exiftool`) and `NEW` a tree of the new release. Never edit either:
make NEW a copy under `port/target/scratch/` (`rsync -a --exclude=.git`),
or use a clone read-only. Every perl and port run goes under
`prlimit --as=4000000000 -- timeout N` (the judge scripts do it).

## 1. Fetch the release and read its diff

```sh
git clone https://github.com/exiftool/exiftool ~/.cache/claude-work/etNEW
cd ~/.cache/claude-work/etNEW && git log --oneline -3      # "Update to 13.NN"
git diff --stat OLDSHA NEWSHA                              # the files touched
rsync -a --exclude=.git ~/.cache/claude-work/etNEW/ port/target/scratch/etNEW/
```

`Changes` (the release notes) says what each change is for; read it first.

## 2. List the subs the release changes

From `port/`:

```sh
python3 -I scripts/upgrade_diff.py OLD NEW          # summary
python3 -I scripts/upgrade_diff.py OLD NEW --diff   # with each sub's diff
```

For each Perl file that differs, it prints the subs whose body changed,
were added or were removed, each marked `NATIVE` or `NOT NATIVE` (an
added sub, to write as a new native and register), and the count of
changed lines outside subs (tables and module-level code: dumped).
A sub is compared from its `sub NAME` line to the first `}` in column 0,
as ExifTool writes them, so a table hunk after a sub is not attributed to
it. Also read by hand:

- the `exiftool` script's changes outside subs (its main program is
  `main::<program>` in src/cli/program.rs and options.rs: new lexicals go
  in `LEXICALS` there);
- new keys of dirInfo, tagInfo or `$$et{...}` that the changed subs read,
  and where else they are read (`grep -rn KEY NEW/lib`): a key set by a
  table's code string or by one sub may be read by another;
- new module-level `my` variables that a native sub now uses: they are in
  the dump (found through the Perl subs that use them), and a native reads
  them with `runtime::lex_hash(it, MODULE, "name")`.

## 3. Regenerate

```sh
sh scripts/dump_all.sh NEW/lib                    # data/*.pld, data/index.rs
perl scripts/gen_extra_langs.pl NEW/lib           # data/Lang_{ar,bn,hi,pt}.pld, extra_langs.rs
python3 -I scripts/gen_sub_index.py NEW/lib > src/runtime/sub_index.rs
python3 -I scripts/gen_prototypes.py NEW/lib > src/runtime/prototypes.rs
python3 -I scripts/remap_lines.py OLD NEW --write # the natives' Perl line numbers
perl scripts/dump_code_strings.pl NEW/lib > tests/data/code_strings.txt  # the tables' code strings
```

(each under `prlimit --as=4000000000 -- timeout 3000`.) The dumps of
unchanged modules come out byte for byte the same, except `ID3.pld`,
which holds two hash addresses (`HASH(0x...)`) that change on every run:
keep its old version unless ID3.pm changed.

`remap_lines.py` moves the `pl::at(N)` lines (and those of the few local
helpers that pass a line to it) through the diff of each Perl file, so
that perl's message locations stay right. Run it before editing the
natives (it reads OLD's subs to know which file a line belongs to). It
lists the lines whose statement the release changed: set those by hand
while porting that change.

Commit the regenerated data on its own.

## 4. Port the native changes

For each sub of step 2, apply the change to its Rust port (a new sub: a
new native, registered in its file's `register`), line by line, with the rules of PORTING.md (faithful, same order, the `pl`
helpers, `pl::at(LINE)` with the NEW line before a statement that can
die or warn). `port_status.py NEW FILE` checks that every sub of a file
is still registered. Group the commits by module.

Then the version: `EXIFTOOL_VERSION` (src/api/mod.rs), `$version` in
src/cli/program.rs, the crate description (Cargo.toml), the doc comments
of src/lib.rs and src/cli/, the docs here, and the golden output of
`-ver -v` (`CLI_GOLDEN_UPDATE=1`, below).

## 5. Judges

All against NEW, from `port/`, after `cargo build --release --example run_t
--example exiftool-rs -j2`:

```sh
sh scripts/run_suite.sh NEW                 # NEW's own t/: same count or more, all pass
sh scripts/compare_images.sh NEW            # every t/images file identical
PORT_BIN=$PWD/target/release/examples/exiftool-rs python3 -I scripts/compare_cli.py NEW
for f in scripts/cli_cases_*.txt; do        # the other case files, the same way
    CLI_CASES=$f PORT_BIN=$PWD/target/release/examples/exiftool-rs python3 -I scripts/compare_cli.py NEW
done
EXIFTOOL_SRC=$PWD/target/scratch/etNEW PERL_CMD=scripts/full_modules/perl.sh \
    CLI_CASES=scripts/cli_cases_full_modules.txt \
    PORT_BIN=$PWD/target/release/examples/exiftool-rs python3 -I scripts/compare_cli.py NEW
cargo test -j2 --release --no-fail-fast
python3 -I scripts/compare_corpus.py rawsamples NEW   # tests/corpus/README.md
```

`compare_cli.py` runs NEW's `exiftool` script with the host's perl.
The Docker image of the second set needs no rebuild (its Perl modules do
not depend on ExifTool); `EXIFTOOL_SRC` is the tree it mounts. The case
`-ver -v` (kind `S`) compares with the port's golden output: rewrite it
with `CLI_GOLDEN_UPDATE=1` once, and check its diff is only the version.

Notes on the judges:

- A few tests of `cargo test` (official.rs, parse_sources.rs,
  parse_testlib.rs, debug_run.rs) read ExifTool's tree at
  `port/../../exiftool`, and `api_media_write.rs` reads written files
  back with `$EXIFTOOL_DIR` (default the same tree): that must be NEW.
  In a worktree, where `../../exiftool` does not exist, link it to NEW
  for the run and remove the link after.
- `compare_corpus.py` expects the corpus at `../target/corpus/rawsamples`
  (a link to the unpacked corpus will do). Perl takes more than its hour
  on three Hasselblad 3FR files with `-j -n` (FixUTF8, quadratic on their
  large binary value: exiftool/exiftool#496), so the `-n` side times out
  on the whole corpus. Compare a hard-linked copy without them
  (`cp -al`, then `compare_corpus.py rawsamples_fast NEW`), and those
  three in text mode (`-a -u -G1 -s -n`, which escapes nothing), one by
  one.
- Known differences that do not come from a release, listed in the case
  files' headers (`-k`/`-pause`: the port has no Term::ReadKey). A case
  that differs once and not when run alone (`compare_cli.py NEW FILTER`)
  is timing, not a difference.

The Perl oracles of the root crate's tests (`tests/expected*`, which the
port's api_regression.rs reads too) come from ExifTool. Regenerate them
with NEW, with the oracles' time zone:

```sh
export TZ="CET-1CEST,M3.5.0,M10.5.0/3"
bash scripts/gen_multiset_baselines.sh NEW       # tests/expected_multi
bash scripts/gen_group_baselines.sh NEW          # tests/expected_groups
# tests/expected/*.tags: the recipe in scripts/test_iso.sh, with NEW's exiftool
```

They are deterministic: only the files whose output changed come out
different (delete the oracles they create for images that had none).
`tests/expected_values/*.vals` (first value of each tag) has no script:
edit the same files by hand from the new `.mvals`. The root crate, which
stays on the release it was made for, then has the changes as new
deltas: add them to its baselines (`tests/*_baseline.txt`) and check with
`cargo test --release --test regression` at the root.

The other tests whose expected values are ExifTool's (`api_*`,
`tests/data/expected*`) say which release they were taken from: when
NEW's output differs, take the new output from NEW's Perl (not from the
port), update the test, and list each change in the commit message.

## 13.59 to 13.60 (2026-10-28, commit 9804093)

`upgrade_diff.py` found 26 subs changed and one added, all ported:

| Perl | Sub | Change |
|---|---|---|
| exiftool | ReadStayOpen, main program | `-stay_open` on piped stdin ends at EOF (`$stayOpenStdin`, set from `-p`/`-S` on the handle) instead of polling forever |
| ExifTool.pm | ProcessBinaryData | no next-index unknown tag after a `LookAhead` tag |
| BuildTagLookup.pm | new | RawConv that calls `FindFirmVer` with a lookup counts as setting a data member |
| Canon.pm | FindFirmVer (new) | firmware version in CameraInfo (new tag FirmwareInfo, with `LookAhead`), sets CanonFirm/CanonFirmVer |
| ExifTool.pm | ProcessJPEG | `DirStart` of an in-memory JPEG; the Xiaomi HEIC "mess" (`XiaomiMess`: APP segments after a 4-byte header, after EOI, junk between segments); APP2 `XMTH` Xiaomi preview, assembled over several segments |
| Canon.pm | PrintLensID | with an RF extender in LensModel, also try without scaling (EOS R5m2), named `+ RF1.4x` |
| Exif.pm | ProcessExif | warning "First entry for DIR has an invalid format. Ignoring possibly corrupted IFD"; "(end is N but EXIF size is only M)" |
| GM.pm | Process_mrld | channel number masked with 0x0fffffff |
| Garmin.pm | ProcessFIT | developer field names made unique (`-IDX`); NaN tested numerically, other invalid values with `eq` (no `lc`) |
| Geolocation.pm | ReadDatabase | a truncated database is an error ("Error reading FILE (truncated file?)") instead of an endless loop |
| Geotag.pm | LoadTrackLog | `goto DoneFix` replaced by a `$doneFix` flag handled at the top of the loop |
| HtmlDump.pm | Print | the title is HTML-escaped |
| LigoGPS.pm | ProcessLigoGPS | a comment |
| M2TS.pm | ProcessM2TS | trailer signature `####` as well as `&&&&` |
| Nikon.pm | PrepareNikonOffsets | first tag of a list taken without its Condition; "Data too short" always warned |
| PDF.pm | ReadPDF | relaxed xref entry syntax |
| PGF.pm | ProcessPGF | nesting limited to 10 (`PGFReEntryDepth`) |
| PNG.pm | FoundPNG | `local $$et{PROCESSED} = {}` around an embedded directory |
| Protobuf.pm | ProcessProtobuf | `signed`/`unsigned` read as `undef`; the format passed to HandleTag |
| QuickTime.pm | HandleItemInfo | Xiaomi EXIF item read as JPEG APP segments (ProcessJPEG, `XiaomiMess`); `$$et{BASE}` moved to the item's data |
| QuickTime.pm | ProcessHybrid | atom may end 4 bytes before its parent |
| QuickTimeStream.pl | ProcessFreeGPS | LIGOGPSINFO at offset 116 too |
| Red.pm | ProcessR3D | directory up to 10000 bytes, tag 0x1000 type 0xad |
| WriteQuickTime.pl | WriteItemInfo | Xiaomi EXIF item rewritten by WriteJPEG; non-QuickTime errors demoted (`DemoteErrors`) |
| WriteQuickTime.pl | WriteQuickTime | `WriteLast` ordered by its value (Samsung `sefd` last); option `QuickTimeUndLang`; IText language packed properly |
| Writer.pl | DumpTrailer | output indented (`INDENT`, HexDump `Prefix`) |
| Writer.pl | WriteJPEG | the Xiaomi mess (header, APP segments after EOI, no SOS) |

The interpreter got what the new script needs: `-S` (socket), file tests
on a handle with no file name (they stat the descriptor, as perl does),
and the 2-argument open of `-` (stdin, or stdout for `>-`), which
ExifTool's Open uses for the file name `-`: `exiftool-rs -` (an image on
stdin) and `-@ -` failed with "Error opening ..." before. The corpus
also found that a logical `xor` warned about its operands ("isn't
numeric", Nikon's FileInfo Condition under the script's `$^W`): it no
longer does, as in perl.

With the dumps: the new tags and tables (Canon, DJI, Nikon, NikonCustom,
Pentax, QuickTime, XMP, FujiFilm, Garmin...), the option
`QuickTimeUndLang`, Protobuf's `%protoOnly`.

Oracles changed by 13.60 (tests/expected*): CanonRaw.cr3 has the new
FirmwareInfo (`1.0.0 34(00) at offset 0x5c1`); Olympus2.jpg's
WhiteBalanceBracket is WhiteBalanceFineTune. No other test expectation
changed.

Judges against 13.60 (port/target/scratch/et1360, a copy of the release):

- its suite: 114 scripts, 114 pass, 604/604 tests (13.59 had 603);
- compare_images: 194 identical, 0 different;
- compare_cli: cli_cases 171/171, options 190/192, program 121/121,
  subs_a 84/86, subs_b 80/80, get_image_info 103/103, set_image_info
  69/69, full modules 30/30. The 4 differences are the known `-k` and
  `-pause` prompts (Term::ReadKey). A `-diff` case of subs_a differed
  once in a full run and never alone or in a rerun (file times);
- cargo test --release: all pass (215, with the new facade test);
- rawsamples: 455 files identical in both modes (print and `-n`); the
  three Hasselblad 3FR files identical in print mode and in `-n` text
  mode (Perl's `-j -n` on them was stopped after an hour).
