# Migration Guide: Perl ExifTool → exiftool-rs

This document describes the differences between the original Perl ExifTool
and exiftool-rs for developers migrating their applications.

## Overview

exiftool-rs is a Rust reimplementation of [ExifTool](https://exiftool.org/) v13.53.
It aims for iso-functionality while being **38-61x faster** and memory-safe.

---

## Output Format Differences

### Exposure Time

| Tag | Perl ExifTool | exiftool-rs |
|-----|--------------|-------------|
| ExposureTime (short) | `4` | `4 s` |
| ExposureTime (fraction) | `1/60` | `1/60 s` |

**exiftool-rs** always appends `" s"` to exposure time values.
If you parse this field, strip the ` s` suffix or use `-n` (numeric mode)
which outputs raw values without print conversions in both versions.

### F-Number

| Perl ExifTool | exiftool-rs |
|--------------|-------------|
| `14.0` | `f/14.0` |
| `2.8` | `f/2.8` |

**exiftool-rs** prefixes f-numbers with `"f/"`.
Use `-n` for raw numeric values.

### Focal Length

Both versions output `"34.0 mm"` for EXIF FocalLength.
However, Canon MakerNotes FocalLength may differ:

| Perl ExifTool | exiftool-rs |
|--------------|-------------|
| `34.0 mm` | `34 mm` |

Canon MakerNotes FocalLength is decoded from a uint16 array and displayed
as an integer in exiftool-rs, while Perl applies FocalUnits division.

### Metering Mode

The EXIF MeteringMode tag (0x9207) has slightly different labels for value 2:

| Perl ExifTool | exiftool-rs |
|--------------|-------------|
| `Center-weighted average` | `Center-weighted average` |

These now match. However, the Canon MakerNotes MeteringMode (decoded from
CameraSettings index 17) maps value 1 differently:

| Value | Perl ExifTool | exiftool-rs |
|-------|--------------|-------------|
| 1 | `Spot` | `Spot` |
| 2 | `Average` | `Average` |

The difference was in earlier versions; both now agree.

### Contrast / Saturation / Sharpness (Canon)

Perl ExifTool applies a Canon-specific print conversion that formats these
as signed values with `+` prefix:

| Perl ExifTool | exiftool-rs |
|--------------|-------------|
| `+1` | `1` |
| `-1` | `-1` |
| `Normal` | `0` |

**exiftool-rs** outputs the raw numeric value. Perl ExifTool applies
the CameraSettings sub-table conversion which maps 0 → "Normal" and
prefixes positive values with "+". Use `-n` in Perl to get matching
numeric output.

### Undefined/Binary Data

Tags containing binary or undefined data are displayed differently:

| Perl ExifTool | exiftool-rs |
|--------------|-------------|
| `0232` (ExifVersion) | `02.32` |
| `(Binary data 1556 bytes, use -b to extract)` | `(Undefined 1556 bytes)` |

**exiftool-rs** shows `(Undefined N bytes)` for binary blobs.
ExifVersion is formatted as `MM.mm` instead of `MMmm`.

---

## Tag Name Differences

### Duplicate Tags

When the same tag exists in multiple IFDs (e.g., XResolution in IFD0 and IFD1),
Perl ExifTool appends a copy number: `XResolution (1)`.
**exiftool-rs** shows both without numbering by default.
Use `-g` (group mode) to distinguish them.

### MakerNotes Tag Names

Tag names from auto-generated tables match the Perl ExifTool names exactly.
However, tags not yet in our tables appear as `Tag0xNNNN` while Perl
shows the full name. There are approximately 0 unknown tags on typical
camera JPEG files (Canon, Nikon, Sony, etc.)

### XMP Tag Names

XMP tag names in exiftool-rs are prefixed with the namespace:

| Perl ExifTool | exiftool-rs |
|--------------|-------------|
| `Title` | `Xmptitle` or `DcTitle` |
| `Creator` | `Dccreator` |

This is because exiftool-rs concatenates the XMP namespace prefix with
the property name. Use the group display (`-g`) for clearer output.

### Composite Tag Names

Composite tags have the same names in both versions:
`GPSPosition`, `ImageSize`, `Megapixels`, `ShutterSpeed`, `Aperture`,
`LightValue`, `DateTimeCreated`, `FOV`, `HyperfocalDistance`.

exiftool-rs adds geolocation composites not present in standard ExifTool
output (requires `-api Geolocation` in Perl):
`GPSCity`, `GPSCountry`, `GPSCountryCode`, `GPSRegion`, `GPSTimezone`.

---

## API Differences

### Rust Crate API

```rust
use exiftool::ExifTool;

// Reading (equivalent to Perl's ImageInfo)
let et = ExifTool::new();
let info = et.image_info("photo.jpg")?;
// info: HashMap<String, String>

// Full tag access
let tags = et.extract_info("photo.jpg")?;
for tag in &tags {
    println!("{}: {} [{}]", tag.name, tag.print_value, tag.group.family0);
}

// Writing (equivalent to Perl's SetNewValue + WriteInfo)
let mut et = ExifTool::new();
et.set_new_value("Artist", Some("John Doe"));
et.set_new_value("XMP:Title", Some("My Photo"));
et.write_info("input.jpg", "output.jpg")?;
```

### Key Differences from Perl API

| Perl | Rust | Notes |
|------|------|-------|
| `$et->ImageInfo($file)` | `et.image_info(file)?` | Returns `HashMap<String, String>` |
| `$et->ExtractInfo($file)` | `et.extract_info(file)?` | Returns `Vec<Tag>` |
| `$et->SetNewValue($tag, $val)` | `et.set_new_value(tag, Some(val))` | |
| `$et->SetNewValue($tag, undef)` | `et.set_new_value(tag, None)` | Delete tag |
| `$et->WriteInfo($src, $dst)` | `et.write_info(src, dst)?` | |
| `$et->Options(Duplicates => 1)` | `options.duplicates = true` | Set before creating ExifTool |
| `$et->GetValue($tag, 'Raw')` | `tag.raw_value` | On `Tag` struct |
| `$et->GetValue($tag, 'PrintConv')` | `tag.print_value` | On `Tag` struct |
| `$et->GetGroup($tag, 0)` | `tag.group.family0` | On `Tag` struct |

---

## CLI Differences

### Supported Options

Most common options are supported identically:

```
-s, -g, -n, -j, -csv, -X, -r, -ext, -b
-TAG=VALUE, -TAG=, -overwrite_original
-tagsFromFile, -stay_open, -if, -p
```

### Not Yet Supported

These Perl ExifTool CLI options are not yet implemented:

```
-htmlDump          HTML diagnostic dump
-a                 Allow duplicate tags (partial: use -g)
-api OPT=VAL       API options
-charset CHARSET   Character encoding
-d FMT             Date format
-D                 Show tag ID numbers
-e                 Don't composite
-ee                Extract embedded files
-ex                Exclude specific tags
-fileOrder TAG     Sort files by tag
-i DIR             Ignore directory
-lang LANG         Output language
-L                 Use Windows Latin1 encoding
-listx             List tags in XML
-m                 Ignore minor errors
-o OUTFILE         Output file
-P                 Preserve file modification date
-password PASS     Password for protected files
-progress          Show progress
-q                 Quiet mode
-scanForXMP        Scan for XMP in all files
-sep STRING        List separator
-struct            Enable structure output
-t                 Tab-delimited output (use -T)
-u                 Show unknown tags
-U                 Show unknown binary tags
-w EXT             Write output to files
-x TAG             Exclude tag
-z                 Read/write compressed data
```

---

## Writing Differences

### Supported Write Formats

| Format | Perl | Rust | Notes |
|--------|------|------|-------|
| JPEG | R/W | R/W | Full EXIF+XMP+IPTC merge |
| TIFF | R/W | R/W | In-place modification |
| PNG | R/W | R/W | tEXt + eXIf chunks |
| WebP | R/W | R/W | EXIF + XMP chunks |
| MP4/MOV | R/W | R/W | ilst atoms + XMP uuid |
| HEIF/AVIF | R/W | R/W | Via ISOBMFF |
| PSD | R/W | R/W | IRB resources |
| PDF | R/W | R/W | Info dict in-place |
| MKV/WebM | R/W | R/W | EBML Tags |
| DNG/CR2/NEF | R/W | R/W | Via TIFF |
| GIF | R/W | R only | |
| FLAC | R/W | R only | |
| MP3/ID3 | R/W | R only | |
| PostScript | R/W | R only | |
| InDesign | R/W | R only | |

### Write Behavior

**exiftool-rs** preserves existing tags when writing to JPEG (merge mode).
Tags not specified in `set_new_value` are kept unchanged.

**Temporary file handling**: exiftool-rs writes to a temp file then renames,
same as Perl ExifTool. Use `-overwrite_original` for in-place modification.

**MakerNotes writing**: Currently supports in-place modification only
(same size or smaller values). Growing values are appended at end of
MakerNote block. The Perl version has more sophisticated offset fixup
for complex MakerNote structures.

---

## Performance

| Benchmark | Perl ExifTool | exiftool-rs | Speedup |
|-----------|--------------|-------------|---------|
| 193 files (batch) | 2.0s | 0.054s | **38x** |
| 193 files (separate) | 25.2s | 0.4s | **61x** |
| Single file | ~130ms | ~2ms | **65x** |

The Rust version is consistently 38-65x faster due to:
- No interpreter startup overhead
- Compiled native code
- Zero-copy parsing where possible
- No garbage collection pauses

---

## Geolocation

Both versions use the same `Geolocation.dat` database (114,877 cities).
exiftool-rs automatically searches for the database in:

1. Current directory
2. `../exiftool/lib/Image/ExifTool/Geolocation.dat`
3. `/usr/share/exiftool/Geolocation.dat`
4. `/usr/local/share/exiftool/Geolocation.dat`
5. Next to the executable

Place `Geolocation.dat` in one of these locations for reverse geocoding.

---

## Known Limitations

1. **Encrypted Sony tags**: Some Sony camera settings are encrypted.
   Perl ExifTool decrypts them; exiftool-rs shows raw values.

2. **Complex PrintConv functions**: Perl ExifTool uses Perl code for some
   print conversions (e.g., calculating aperture from APEX values).
   exiftool-rs only supports hash-based conversions (covers ~95% of tags).

3. **Writing to non-standard formats**: Perl supports writing to ~30 formats.
   exiftool-rs supports 11 write formats covering the most common cases.

4. **User-defined tags**: The `.ExifTool_config` parser supports a subset
   of the Perl syntax. Complex Perl expressions in configs are not supported.

5. **Character encoding**: Perl ExifTool handles many character encodings
   (Latin1, UTF-8, UTF-16, ShiftJIS, etc.). exiftool-rs assumes UTF-8 and
   uses `String::from_utf8_lossy` for non-UTF-8 data.

6. **HTML dump**: The `-htmlDump` diagnostic feature is not implemented.

---

## Compatibility Checklist

For applications migrating from Perl ExifTool:

- [ ] Use `-n` flag if you parse numeric values (avoids format differences)
- [ ] Update XMP tag name parsing (namespace-prefixed in exiftool-rs)
- [ ] Test binary extraction with `-b` (output format matches)
- [ ] Verify write operations on your specific file formats
- [ ] Place `Geolocation.dat` in an accessible location
- [ ] Test `-stay_open` integration (same `{ready}` marker protocol)
- [ ] Handle the `f/` prefix on FNumber if you parse aperture values
- [ ] Handle the ` s` suffix on ExposureTime if you parse shutter speed
