# Changelog

All notable changes to `exiftool-rs` are documented here.

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
