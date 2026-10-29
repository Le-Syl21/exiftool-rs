# The Rust API over the port

`port/src/api/` keeps the high-level API of the current crate
(`exiftool_rs::ExifTool`, `Options`, `Tag`, `Value`...) as a facade over
the port (ARCHITECTURE.md §5). The names, signatures and re-exports are the
root crate's, so code written against `exiftool_rs` builds against
`exiftool_port` by renaming the crate. Behind it, every call is answered by
Image::ExifTool itself, run by the port's interpreter: the facade does not
read, convert or write metadata on its own.

## What each call maps to

| Facade | Image::ExifTool |
|---|---|
| `ExifTool::extract_info(path)` | `new`, `Options(...)`, `ImageInfo($file, @requested_tags)`, then for each key of `GetTagList`: `GetTagName`, `GetGroup` (families 0-3, and 6 for the format), `GetDescription`, `GetValue($key, 'ValueConv')` |
| `extract_info_from_bytes(data, _)` | the same on `\$data` |
| `image_info(path)`, `exiftool_port::image_info` | the same, as a name→value map |
| `set_new_value(tag, value)` | queued; replayed as `SetNewValue($tag, $value, Shift => 0, Protected => 1)` (`Protected` unless the name has a wildcard), as the exiftool application does for `-TAG=VALUE` |
| `set_new_values_from_file(src, tags)` | queued; replayed as `SetNewValuesFromFile($src, @tags)`; returns the number of tags it sets, counted as the application counts them for `-tagsFromFile` |
| `write_info(src, dst)` | a new ExifTool, the queued requests replayed in order, `WriteInfo($src, \$out)` |
| `write_warnings()` | the warnings of `SetNewValue`, `SetNewValuesFromFile` and `WriteInfo` (the `Warning` tags) |
| `get_file_type(path)`, `file_type::detect_from_magic` | `ImageInfo(..., 'FileType')` with `FastScan => 3` |
| `file_type::detect_from_extension` | `GetFileType` |
| `FileType::support()` | `CanCreate`, `CanWrite` |
| `ExifTool::writable_tags(type)` | `CanWrite` and `GetWritableTags` |

Options: `duplicates` → `Duplicates` (on with `extract_embedded`, as `-ee`
turns it on), `fast_scan` → `FastScan`, `requested_tags` → the tag list of
`ImageInfo` (`Group:Tag`, `Group:*`...), `extract_embedded` →
`ExtractEmbedded`, `show_unknown` → `Unknown`, `process_compressed` →
`Compress`, `geolocation` → `Geolocation`, `struct_output` → `Struct`,
`use_mwg` → `Image::ExifTool::MWG::Load`, `lang` → `Lang` (reading and
writing). Extraction always runs with
`PrintConv` on: `print_conv` only chooses which of the two values
`image_info` returns, as in the root crate.

### Languages

`Options::lang` is ExifTool's `Lang` option: `Tag::description` is
`GetDescription` and `Tag::print_value` the PrintConv value in that
language, as `exiftool -lang` prints them, and a write takes values in it
(Writer.pl converts a translated value back), its result read back in it.
The code is taken as `-lang` takes it (`zh-CN` is `zh_cn`), `zh` is
`zh_cn` as in the root crate, English is `None` or `"en"`, and a language
ExifTool does not have is ignored, as ExifTool ignores it.

The languages are ExifTool's 18 (its Lang modules, unchanged) and four the
root crate had and ExifTool does not: `ar`, `bn`, `hi` and `pt`. Those are
Lang modules of the same format (`%Image::ExifTool::Lang::pt::Translate`),
made from the root crate's `locales/*.yml` (descriptions; `locales/values/`
has no values for them) by `scripts/gen_extra_langs.pl`, which dumps them
to `data/Lang_{ar,bn,hi,pt}.pld` as the other modules are dumped and lists
them in `data/extra_langs.rs`. `Options(Lang => ...)` accepts them; they are
not in `@Image::ExifTool::langs`, so `-lang` (the list), `-listx` and
`-lang xx` print Perl's list, as the CLI judge cases (`-lang`, `-E -lang`,
`-q -lang`, `-lang xx`, `-listx`) compare it. `exiftool-rs -lang pt`
works, and the `-lang` list does not show it.

### Values

- `Tag::print_value` is the value of `ImageInfo`, made text as the exiftool
  application makes it: a list joined with `ListSep` (`, `), a structure
  serialized (`{Field=value,...}`, XMPStruct.pl `SerializeStruct`), binary
  data as `(Binary data N bytes, use -b option to extract)`.
- `Tag::raw_value` is the ValueConv value. A number is typed only where its
  text is unchanged: an integer format (family 6 group, `SaveFormat`) gives
  `U8`..`I32`, a space-separated count of them a `List`, a rational or float
  format `F64`; a value Perl holds as a number and that has no format
  (FileSize, ImageWidth of a PNG...) gives `U32`/`I32`/`F64`; anything else
  is a `String`. A list is a `List`, a structure a `Struct` (fields in
  ExifTool's order, `OrderedKeys`), binary data `Binary` (its bytes), or,
  when ExifTool did not extract them, the `(Binary data N bytes, ...)` text.
  `raw_value.to_display_string()` is what `exiftool -n` prints.
- Groups are `GetGroup`'s; `family3` is `Main` or `DocN`.
- `Tag::id` is the tag ID, `Numeric` when it is an integer of 16 bits,
  `Text` otherwise. `Tag::priority` is the tag's `Priority`, 1 when it has
  none.

### Writing

`write_info` writes ExifTool's output into a new file next to `dst`
(created exclusively, under a name that keeps `dst`'s extension), gives it
the permissions of the file it replaces, and renames it over `dst`; on any
failure the temporary file is removed and `dst` is left as it was. In place
(`src` = `dst`) is the same. A destination ExifTool cannot write is refused
as the exiftool application refuses `-o` ("Can't write MP3 files").

It returns the number of requests that took effect, judged by reading the
result back with ExifTool, as the root crate does: a written tag must be
there with its value (or a new value), a deleted tag or group must be gone
(an EXIF tag from its write group only; `PNG:all` the tags ExifTool can
write). Deleting what the file does not have counts 0 and is not an error.
When nothing takes effect, a new destination gets ExifTool's unchanged
copy and the call returns `Ok(0)`; in place it fails and leaves the file
alone ("Nothing to do.").

### Threads

`ExifTool` is `Send + Sync`, and calls can run on several threads at once.
Each call runs on an interpreter of its own (the interpreter is not `Send`),
as each run of the exiftool application is a process of its own. ExifTool
keeps state in its tables from one file to the next, in Perl too: in one
process, XMP.xml gets an AutoFocus composite that a module loaded for an
earlier file defines, and after writing Kodak.jpg's Make the output reads
"Unrecognized MakerNoteUnknown" instead of "Unrecognized MakerNotes". A
call therefore never depends on the ones before it. The price is the
interpreter's start, about as long again as reading a small file
(the root crate's regression suite: 7 s with interpreters kept per thread,
15 s with one per call).

## The GUI

`exiftool-rs-gui` (`cargo build --release --features gui`, giving
`target/release/exiftool-rs-gui`; `src/bin/exiftool_rs_gui/`) is the root crate's
src/gui.rs on the facade, with the same panels, strings and behaviour. Its
own strings are the `_ui.*` entries of the root crate's locales
(`data/gui_text.tsv`, written by `scripts/gen_extra_langs.pl`); the tag
descriptions and values are ExifTool's, read with `Options::lang`, where
the root crate looked them up in its own tables. So changing the language
reads the file again, the Copy button copies the values as shown (in the
language), and an edit starts from, and is saved as, the value in the
language. The model is tested without a window (`cargo test --features
gui`).

## Differences with the root crate

The root crate had its own readers and writers; the facade has ExifTool's.
Where the two differ, the facade is ExifTool's:

- **Formats ExifTool does not write** (MP3/AIFF ID3, FLAC, Ogg, Opus,
  Matroska) are refused with `Error::UnsupportedFileType("Can't write MP3
  files")`; the root crate wrote them. `writable_tags` is ExifTool's whole
  writable set for a type ExifTool writes, and empty otherwise (never
  `None`).
- **Writes ExifTool refuses fail**: SonyILCE9.arw (cut-off preview) gives
  "Error reading PreviewImage data in IFD1"; a HEIF Exif item stored with
  construction method 1 is copied unchanged with ExifTool's warning.
- **Warnings** are ExifTool's words ("Tag 'Foo' is not defined", "Can't
  convert IFD0:Orientation (not in PrintConv)"), and only those: a request
  ExifTool accepts but cannot place (`IPTC:City` in a MOV) is not counted
  and not warned about, as ExifTool says nothing.
- **Printed values are not sanitized for a terminal**: trailing spaces,
  control characters and NULs stay as ExifTool has them (`exiftool -j`
  keeps them; only its text output trims them). The root crate trimmed them.
- **Tag order** is ExifTool's (`GetTagList`, as `exiftool` lists them).
- **`image_info` with duplicates** keys a duplicate `Name [Group0:Group1]`
  as before, but which instance is the plain `Name` is ExifTool's choice.
- **Binary data** ExifTool does not extract unless asked (a preview image,
  a thumbnail) is the `(Binary data N bytes, ...)` text; request the tag
  (`requested_tags`) to get its bytes, as with ExifTool.
- **`extract_info_from_bytes`** identifies the data by its content only;
  the path is not used (no FileName/Directory tags, no type by extension).
- **`get_file_type` and `detect_from_*`** give ExifTool's type; a type the
  enum does not name (ExifTool's `FileType` names a variant, as `Extended
  WEBP`) is found by its extension.

Not provided: the root crate's internal modules (`formats`, `metadata`,
`tags`, `writer`, `composite`, `config`, `encoding`, `geolocation`,
`geotag`, `i18n`, `md5`), whose work is now ExifTool's own; the public
helpers in them (`tag::warning_tag`, `value::format_g15` and
`format_g_prec`, `Tag::priority_rank` are kept, as part of `tag` and
`value`).
