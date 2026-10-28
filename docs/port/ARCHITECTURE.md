# exiftool-rs: faithful port of ExifTool 13.59

The goal is a Rust clone of Perl ExifTool. It must cover everything ExifTool
does, give identical output for every command and every argument, and run
faster. The port follows the Perl source file by file, function by function.
Each Rust module mirrors one Perl file, keeping its names and structure, so
any Rust function can be compared with the Perl sub it comes from. When
ExifTool releases a new version, the port takes its diff file by file.

Reference contract: `lib/Image/ExifTool/README` (1187 lines), Phil Harvey's
specification of every table key, tagInfo key and flag. Study data used for
this document is in this folder: `inv.txt` (per-file inventory), `deps.txt`
(module dependencies), `uniq.txt` (the 3,420 distinct Perl strings embedded
in tables) and `scan.pl` (the runtime table walker).

## 1. What there is to port

ExifTool 13.59 has 239 files and 352k lines, of which 21 % is code in named
subs (1,206 subs, about 72k lines). The other 79 % is tag tables, and those
tables carry Perl code of their own.

| Kind | Volume | How it is ported |
|---|---|---|
| Tag tables (1,549 tables, 28,667 tagInfo) | ~230k lines | generated (section 4) |
| Perl strings in tables (Condition, *Conv, *Inv, Hook, Format, Start...) | 3,420 distinct, ~14k uses | Perl engine (section 3) |
| Code refs in tables (OTHER subs, Conv subs) | 881 | ported by hand with their module |
| Named subs | 1,206 subs, ~72k lines | one agent per file |
| Lang/* (18 languages), Charset/* (33 maps), TagLookup | ~93k lines | generated |
| `exiftool` CLI script | ~5k lines of code | ported by hand |

Procedural work is concentrated in about 50 files: ExifTool.pm (6.9k sub
lines), Writer.pl (6.5k), QuickTimeStream (2.7k), WriteExif (2.6k),
WriteQuickTime (2.0k), QuickTime, PDF, Exif, XMP, Geotag, WriteXMP.

## 2. The runtime ("socle")

These are ports of the core of ExifTool.pm and Writer.pl, which every
module plugs into. They are written first, by the coordinator, before any
module agent starts. Module agents never change them, they only ask for
additions.

- **`PerlValue`**: Perl's scalar, which every value, data member and
  conversion goes through.
  - Variants: undef, bytes string, number (IV/NV), SCALAR ref (binary),
    ARRAY, HASH, code.
  - Stringification is Perl's `%.15g`; rationals use `%.7g` / `%.10g`.
    `'inf'` and `'undef'` are strings, and 64-bit integers are doubles.
  - Numification keeps the leading numeric prefix (`"12 34" & 0xff` is 12).
  - Truthiness: `""`, `"0"`, 0 and undef are false, while `"0.0"` and
    `"00"` are true.
- **`ExifTool`**: the context object.
  - A typed struct for the framework state: VALUE, TAG_INFO, TAG_EXTRA,
    PRIORITY, FILE_ORDER, DUPL_TAG, COMP_KEYS, LIST_TAGS, DOC_NUM, PATH,
    PROCESSED, BASE, DIR_NAME, OPTIONS, the request and exclude lookups,
    WAS_WARNED...
  - A `HashMap<String, PerlValue>` for the per-file data members
    (`$$self{Model}`, `$$self{CanonFirm}`... 234 distinct ones). It is
    cleared by `Init`, just as Perl deletes every key containing a
    lowercase letter.
  - The byte order lives in the context, with the same save and restore
    points as Perl: ProcessDirectory, HandleTag, ReEntry.
- **Tables**: generated static data, plus a process-level registry that
  reproduces Perl's lazy behaviour:
  - `GetTagTable` loads a module's tables on first use.
  - `AddCompositeTags` runs at module load, so the set of composites depends
    on which modules were loaded, as in Perl.
  - `AddTagToTable` adds unknown tags at runtime.
  - Each tagInfo has a stable identity, because LIST_TAGS and the shared
    PrintConv hashes rely on reference identity.
- **The read pipeline**, ported line for line:
  - `ExtractInfo`: file type candidates from `%fileTypeLookup`, then
    `%magicNumber`, then `Process$TYPE`.
  - `ProcessDirectory`, `HandleTag` and `FoundTag`, with their priority,
    duplicate, list and document rules.
  - `GetTagInfo` with Condition, `ProcessBinaryData`, `ReadValue` and the
    `Get*` readers.
  - `GetValue` with RawConv, ValueConv and PrintConv; hash conversions with
    OTHER, BITMASK and PrintHex; Lang overrides; lists; binary values.
  - `BuildCompositeTags`, `SetFoundTags` with its filters, `GetTagList`
    with its sorts, and `GetGroup` for families 0 to 8.
  - `Warn` and `Error` with their levels and deduplication.
- **The write pipeline**, ported from Writer.pl in the same way:
  - `SetNewValue` and the nvHash structure, `SetNewValuesFromFile`,
    `WriteInfo` with its road maps (`%tiffMap`, `%exifMap`, `%jpegMap`),
    `WriteDirectory`, `IsOverwriting`, `WriteValue`.
  - `ConvInv`, `CheckValue`, `InverseDateTime`, and the CHANGED counter.
- **Module interface**: a ported module provides `Process*` / `Write*`
  functions with the Perl signatures, `(&mut ExifTool, &mut DirInfo,
  &Table) -> i32`. It calls back through the API listed in the read study:
  HandleTag, FoundTag, GetTagInfo, ProcessDirectory, Warn, Options, Decode,
  SetFileType, VPrint/VerboseDir...

## 3. Perl code inside the tables

This is the central technical choice, and it covers four uses:

1. 3,420 distinct strings in tables (Condition, RawConv, ValueConv,
   PrintConv, the inverses, Hook, Format, Start/Base, Validate, IsOffset,
   WriteCheck...). They are evaluated at about 30 sites, each with its own
   variables (the `#### eval` comments in the Perl source list them).
2. User-supplied Perl that the CLI accepts: `-if`, `${tag;expr}` in `-p`,
   `-w` and `-tagsFromFile`, `-api Filter=`, and `.fmt` files.
3. `~/.ExifTool_config`, which is Perl source: user-defined tags, tables,
   composites, lenses and shortcuts.
4. Library helpers called from these strings: 91 module functions, plus
   `ConvertDateTime`, `Decode`, `PrintLensID`, `ToDMS`, `sprintf`, `pack`,
   `unpack`, `tr`, `s///e`...

The proposal is one Perl-subset engine for all four uses. Each string is
parsed once into an AST, cached, and evaluated against the eval site's
variables. It grows out of the existing `conv_expr` interpreter (5.8k lines),
which already parses and evaluates the ValueConv and PrintConv strings. It
has to be extended to cover:

- Conditions, RawConv side effects and the inverse conversions.
- `s///e`, `pos` / `\G`, `local $_`, and list versus scalar context.
- The regex dialect used:
  - backreferences (19 uses)
  - lookahead (36 uses)
  - `/s` on raw bytes, which the magic numbers rely on
  - `tr///` with its `d`, `c` and counting forms

A coverage gate measures the engine. It runs every one of the 3,420 strings
with real data from the test images and compares each result with Perl.
That replaces today's "it parses" counter. If profiling later shows the
interpreter as a bottleneck, the hottest strings can be compiled ahead of
time into Rust, with the interpreter as reference.

The 881 code refs (OTHER subs, conversion subs) are Rust functions written
by the agent porting their module, registered by their Perl symbol name.

## 4. Generators

Perl scripts load ExifTool itself and dump its data, in the way `scan.pl`
walks every table at runtime. The output is Rust static data, never edited
by hand. Running them again against a new ExifTool release regenerates
everything. They cover:

- every table, with every key: table-level keys (PROCESS_PROC by symbol
  name, GROUPS, FORMAT, FIRST_ENTRY, VARS, PRIORITY, DATAMEMBER...) and
  tagInfo keys (Format, Count, Condition, SubDirectory, Groups, Priority,
  the conversions as strings, symbols or hashes, Writable, WriteGroup,
  Protected, Mask...);
- shared structures (PrintConv hashes such as `%canonLensTypes`), emitted
  once and referenced, so identity is kept;
- `%fileTypeLookup`, `%magicNumber`, `%moduleName`, `%mimeType`, and the
  write tables (`%writableType`, the road maps);
- Lang/*, Charset/*, Shortcuts and the TagLookup index.

## 5. What stays in crates

Only standard work goes to crates:

- **CLI**: argv tokenisation and argfiles. Clap cannot express ExifTool's
  syntax (case-sensitive letters, suffixes such as `-g1:2`, and the tag
  operation grammar), so the parser is a hand port of the Perl chain.
- **Paths and directory walking**: walkdir.
- **Compression**: flate2, brotli, lzma.
- **Digests**: md5, sha.
- **Dates**: strftime/strptime for `-d`.
- **XML parsing**, where ExifTool uses its own parser only as a tokenizer.
- **Regex**: the `pcre2` crate, which supports Perl syntax including
  backreferences, on bytes (section 8).
- **GUI**: the existing egui application, ported to the new API.

The high-level Rust API (`ExifTool`, `Options`, `Tag`, `extract_info`,
`write_info`...) is kept as a facade over the new engine, so current users
(Lap, VPin Studio) keep working.

## 6. Tests

1. **ExifTool's own suite** (`t/*.t`): 114 files, about 603 tests and 450
   golden `.out` files. They are ported with a Rust equivalent of
   `t/TestLib.pm`: `check`, `writeCheck`, `testVerbose`, `binaryCompare`,
   and `nearEnough` copied as is. Each module lands with its `.t` tests
   passing.
2. **CLI parity**, which the Perl suite does not cover:
   - every option, run by Perl and by Rust on `t/images` and the corpus,
     with the text output diffed;
   - the `arg_files/`, `config_files/` and `fmt_files/` shipped with
     ExifTool as ready-made fixtures.
3. **Large free corpus**: raw.pixls.us (RAW files from every brand),
   Wikimedia Commons and other sources under free licences, image data
   stripped and metadata kept. It is compared with Perl tag by tag, in print
   mode, `-n` and `-a -u -G1`.
4. **Speed**: every gate run times Rust against Perl on the corpus, per
   format, and a slowdown fails the gate. The comparison page that the CI
   publishes at each release (`pages.yml`, `scripts/parity_report.py`)
   shows these timings: Perl time, Rust time and the gain in percent, per
   format and in total.
5. **Engine coverage**: the 3,420 table strings, as described in section 3.

## 7. Order

Each step can be tested as soon as it lands.

0. **Runtime** (section 2), Perl engine (section 3), generators (section 4),
   TestLib.
1. **Data leaves**: Charset, Lang, Shortcuts, TagLookup.
2. **Pilot**:
   - Modules: JPEG and TIFF containers (in ExifTool.pm), Exif, GPS, Fixup,
     PrintIM, IPTC, ICC_Profile, Photoshop, JPEG APP tables, XMP and
     XMPStruct (read), MakerNotes and Canon.
   - Tests: ExifTool.t (read part), GPS.t, IPTC.t, Photoshop.t, XMP.t
     (read part), Canon.t.
   - Also the CLI read options.
   - Assessment: parity, speed, cost per file. The fleet is planned from
     these numbers.
3. **Vendors**, one agent each: Nikon, Sony, Pentax, Olympus, Panasonic,
   Minolta, FujiFilm, Kodak, Samsung...
4. **Standalone image formats**: BMP, GIF, PNG, Jpeg2000/JXL, MIE...
5. **Audio and video**: ID3/FLAC/Ogg..., RIFF, ASF, Matroska, M2TS, MXF;
   QuickTime last, since it is a hub that needs about 30 modules.
6. **Documents**: PDF, PostScript, ZIP family, EXE, LNK, Font, DICOM...
7. **Write path**: Writer.pl, then WriteExif, WriteXMP, WriteIPTC and the
   other `Write*.pl` files.
8. **Features**: Geotag, Geolocation, Shift, MWG, Validate, HtmlDump.
9. **Full CLI**.

The current crate stays on `main` for fixes until the port replaces it,
as 1.0.

## 8. Decisions (Sylvain, 2026-10-24)

1. **One Perl-subset interpreter** for the tables, the CLI expressions and
   the config file.
2. **Regex: the `pcre2` crate** (bindings to the C library), behind our own
   `PerlRegex` interface. Measured on ExifTool's 114 magic numbers against
   the 194 test files: 0 differences with Perl. fancy-regex failed 41 of
   them (`\0` octal) and differs on `$`, `\Q\E` and `[` inside a class. A
   pure-Rust PCRE2 port was studied and set aside: "ne soyons pas plus
   royaliste que le roi".
3. **`~/.ExifTool_config` is in scope**: as a clone, exiftool-rs supports it,
   through the Perl engine.
