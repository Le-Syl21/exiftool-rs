# Changelog

All notable changes to `exiftool-rs` are documented here.

## [0.7.3] - 2026-07-28

### Added

- `exiftool_rs::EXIFTOOL_VERSION` — the Perl ExifTool release this crate mirrors
  (currently 13.59), as a single source of truth.
- Live parity auditor (`parity` binary, dev-only behind the `parity` feature):
  pins and downloads an ExifTool release, then diffs its output against
  exiftool-rs over a corpus — read parity with a ratcheting baseline, and live
  write-parity by IPTC digest. Complements (and regenerates) the test baselines.

### Fixed

- **Accented strings are readable (PSP, RealAudio).** The PSP creator-block and
  RealAudio `Copyright` fields were decoded with `from_utf8_lossy`, turning a
  Latin-1 `©`/`ü` into `�`. They now decode UTF-8-or-Latin-1 like the other
  string fields, and the PSP `Copyright` group matches ExifTool (IFD0 wins by
  default). Live read parity against ExifTool 13.59 is now **195/195 files** —
  full data parity. (The `parity` binary's Perl reader was also taught to decode
  ExifTool's raw Latin-1 output faithfully, and now prints an ISO comparison
  table for the read deltas and write cases.)
- **String output matches ExifTool's display sanitization.** Control characters
  (0x01–0x1F, 0x7F) are rendered as `.`, NULs are dropped, and *trailing*
  whitespace is trimmed (leading whitespace is preserved) — exactly as ExifTool's
  console output does (accented/multibyte text is untouched). This alone moved live read parity against ExifTool 13.59 from
  145/195 to 194/195 files byte-identical (the PSP/RealAudio fix above closes the
  rest, reaching 195/195).
- **IPTC write is no longer destructive (JPEG).** Setting one IPTC tag used to
  drop every other IPTC tag in the file (e.g. 20 tags → 1). Writes now merge
  into the file's existing IPTC — updating or deleting the changed datasets in
  place, preserving the rest (including `CodedCharacterSet`) — matching
  ExifTool byte-for-byte (`CurrentIPTCDigest` identical for set / delete / add).
  The PSD writer still replaces; tracked in
  [#7](https://github.com/Le-Syl21/exiftool-rs/issues/7).

## [0.7.2] - 2026-07-28

### Fixed

- **IPTC string encoding.** Setting an IPTC tag no longer double-encodes
  accented characters — `By-line=Martín` was read back as `MartÃ­n`. Strings
  are now Latin-1-encoded on write (matching ExifTool's default IPTC charset),
  so the written bytes and `CurrentIPTCDigest` match ExifTool. Characters
  outside Latin-1 are substituted with `?`, as ExifTool does without
  `CodedCharacterSet=UTF8`. ([#6](https://github.com/Le-Syl21/exiftool-rs/issues/6))
- **Group-qualified tag filters.** `-IPTC:By-line` (and any `-GROUP:TAG` or
  `-GROUP:*` form) now filters correctly instead of printing nothing; the
  group prefix matches families 0–2 and `*` is a tag wildcard.

## [0.7.1] - 2026-07-24

### Added

- Add Discord community & support link (README, docs, --help, About).

## [0.7.0] - 2026-07-23

### Added

- **Localized PrintConv values under `-lang`.** Enum output values are now
  translated, not just tag descriptions — `Orientation` reads `Horizontale
  (normale)`, `ResolutionUnit` reads `pouce`, and so on. Value output matches
  ExifTool's `-lang` across the corpus (192/195 files) for all 16 ExifTool
  languages, in text, JSON, and the GUI. Translations are generated from
  ExifTool 13.59's `Lang/*.pm`; a group-scoped override layer (derived from
  ExifTool's real output) disambiguates same-named tags that ExifTool localizes
  differently per source table, and values re-read from a standalone XMP file
  are left in English to match ExifTool.
- **81 newly-emitted tags** (Garmin FIT metrics, Android capture tags, HEVC
  colour attributes, GPS destination coordinates) added to all 23 locale files,
  natively translated where ExifTool provides a translation.

### Changed

- **All dependencies updated to their latest releases**, including a migration
  of the optional GUI to **egui / eframe 0.35** (plus brotli 8, xml-rs 1.0,
  rfd 0.17). The name/value/group/`-ee` parity regression suite is unchanged.

### Fixed

- **`-lang` regional codes.** The argument parser stripped the separator, so
  `en_ca`/`en_gb`/`zh_tw` were mangled (and `zh_tw` was forced to `zh`);
  canonicalization now lives in one place and regional variants resolve
  correctly for both descriptions and values.

## [0.6.1] - 2026-06-14

### Performance

- **Memory-mapped reads.** Files are now `mmap`-ed instead of being read fully
  into memory. The container readers walk by offset (MP4/MOV skip `mdat`,
  Matroska stops at the first `Cluster`, Android MP4s keep `moov` at the end), so
  only the header pages are faulted in. A multi-gigabyte video is parsed by
  touching a few MB rather than allocating and reading the whole file: a real
  2 GB MP4 now parses in ~0 ms with ~9 MB RSS (was a multi-minute stall on very
  large files). Falls back to a plain read when mapping is unavailable. (#5)

### Added

- **QuickTime Keys metadata**: Android device info `AndroidMake`, `AndroidModel`,
  `AndroidVersion` (and other mapped `keys`/`ilst` entries), with QuickTime-style
  `meta` (no version/flags) detection. (#5)
- **Color representation** (`colr` atom): `ColorProfiles`, `ColorPrimaries`,
  `TransferCharacteristics`, `MatrixCoefficients`, `VideoFullRangeFlag`.
- **PreviewImage / ThumbnailImage** from QuickTime `mcvr`/`snal`/`tnal` atoms.

### Fixed

- **Rotation**: the per-track `tkhd` matrix was read 4 bytes early (zeroing the
  rotation sub-matrix); now read at the correct offset and computed via an exact
  port of ExifTool's `GetRotationAngle`. `MatrixStructure` is now emitted per
  track.
- **FilePermissions**: emit the ls-style `-rw-rw-r--` string (with the full octal
  mode as the `-n` value) instead of bare `664`.
- **File dates**: `FileModifyDate`/`FileAccessDate`/`FileInodeChangeDate` now use
  local time with a numeric timezone offset, matching `ConvertUnixTime($val,1)`.
- **FileSize**: raw value stored without 32-bit truncation (files > 4 GB).
- **QuickTime duplicate-tag priority**: `mdhd`/`hdlr` tags follow last-wins while
  `tkhd` tags are Priority-0 (first-wins), matching ExifTool's `FoundTag` rule —
  correct primary `HandlerType`/`MediaDuration`/etc. on multi-track files.

### Test corpus

- Added `tests/images/Android.mp4` (sample from #5) with `.tags`/`.vals` oracles;
  0 name and 0 value deltas vs ExifTool 13.53.

## [0.6.0] - 2026-06-12

### Highlights

- **100% value parity** with Perl ExifTool 13.53 across the entire test corpus
  (in addition to the existing 100% tag-name parity). Every tag value the
  reference tool reports is now reproduced byte-for-byte, validated by the
  ratcheting regression suite (`tests/expected_values/*.vals`).

### Added / Fixed (selected)

This release closes out the long tail of value-parity work across most readers:

- **Offset handling**: file-absolute IsOffset tags for embedded EXIF/TIFF in MIFF,
  JP2 (GeoJP2/EXIF uuid), Nikon/Olympus PreviewImageStart, RAF/X3F/RW2.
- **Binary-size reporting**: `(Binary data N bytes)` now uses the formatted-value
  length where ExifTool does (DICOM PixelData, Olympus/Sanyo DataDump), and the
  correct embedded length for FLIR RawThermalImage, Canon CR3 PRVW, TNEF RTF,
  Google HDRPlusMakerNote.
- **Multi-source precedence**: per-source priority for the synthetic multi-format
  test file (CIFF/SPIFF/NITF/PictureInfo/GraphConv/MIE/FotoStation/APP10), Minolta
  CameraSettings (PRIORITY 0), QuickTime mdat, SubIFD pyramid, VCard sub-documents,
  Composite Red/BlueBalance.
- **Conversions**: Pentax TvExposureTimeSetting, Canon Sharpness/RFLensType/
  CustomControls, OlympusE1 BlueBalance rounding, PrintLensID sub-variant
  disambiguation (Pentax/Sigma LensID).
- **Encoding**: Latin-1 raw-byte round-tripping for Real.ra / PSP Copyright.
- **Format-specific**: APP10 PhotoStudio Unicode comment, ASF IsVBR (Metadata
  object), PCAP TimeStamp (reproduces ExifTool's known low-word read behaviour),
  Lytro JSON arrays, Google XMP container, XMP struct-list semantics, and the full
  PLUS LDF MediaSummaryCode vocabulary.

### Resolved issues

- #1 — FLIR R-JPEG support (thermal data, FLIR maker notes).
- #2 — Windows/Tauri build conflict, via the opt-in `win-icon` feature (#3).

### Notes

- Android video device info (`AndroidVersion`/`AndroidMake`/`AndroidModel`, issue
  #4 / PR #5) is **not** included in this release. These come from the QuickTime
  `keys`/`ProcessKeys` metadata mechanism (`com.android.*`), which is distinct from
  the Google/Pixel image maker notes already supported. It remains a candidate for
  a future release.

## [0.5.0] and earlier

See the git history and tags for prior releases.
