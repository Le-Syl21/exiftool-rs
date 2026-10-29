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
| `write_info(src, dst)` | the queued requests replayed in order (after `SetNewValue()` clears those of an earlier call), `WriteInfo($src, \$out)` |
| `register_extension(ext)`, `clear_extensions()` | none: a Rust writer `write_info` calls for a type `CanWrite` refuses (see Extensions) |
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
writing), `binary` → `Binary`; `fresh_interpreter` (see Threads) and
`trim` (see Values) are the facade's own. Extraction always runs with
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
  ExifTool leaves out the bytes of some binary tags (a preview image, a
  JpgFromRaw: those it reads with `ExtractBinary`) unless its `Binary`
  option is set or the tag is requested by name: set `Options::binary`
  (`-b`), or name the tag in `requested_tags`, to have them. ExifTool.jpg's
  PreviewImage is `(Binary data 1039273 bytes, ...)` without, its 5777
  bytes with either. The printed value stays the text.
  `raw_value.to_display_string()` is what `exiftool -n` prints.
- `Options::trim` (off by default: values are ExifTool's) strips, at the
  start and end of each printed value and of each text raw value (each
  item of a list, each field of a structure), white space and invisible
  characters: ASCII white space, NUL and the other C0 controls, DEL, C1
  controls, and Unicode white space, zero-width characters and BOM
  (U+00A0, U+1680, U+180E, U+2000-U+200F, U+2028, U+2029, U+202F,
  U+205F, U+2060, U+3000, U+FEFF). Numbers and binary data are not
  touched. Olympus2.jpg's ImageDescription, `OLYMPUS DIGITAL CAMERA` and
  nine spaces, becomes `OLYMPUS DIGITAL CAMERA`; GPS.jpg's IPTC City, one
  space, becomes empty (the tag stays).
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

It returns the number of requests that changed the file, judged by reading
the result back with ExifTool, as the root crate does: a written tag must be
there with its value (or a new value) that the file did not have, a deleted
tag or group must be gone (an EXIF tag from its write group only; `PNG:all`
the tags ExifTool can write). Deleting what the file does not have, or
writing the value it already has, counts 0 and is not an error: `Artist=y`
on a file whose Artist is `y` returns `Ok(0)`; with another request that
changes something, only that one counts.

When the write changes nothing, the call returns `Ok(0)`, and in place the
file is not touched: no temporary file, no rename, the same modification
time. That is when ExifTool's `WriteInfo` returns 2 ("1 image files
unchanged": nothing written, as for a deletion of what is not there, when
the exiftool application erases its temporary file), when ExifTool's
output has the input's bytes, and for an extension when its output is the
input or holds the same values (nothing counted). Perl differs on one
point: writing the value already there, `WriteInfo` returns 1 and the
application says "1 image files updated", renaming a new file with the
same bytes over the old one (JPEG, PNG, M4A alike); the facade counts 0
and leaves the file alone. A new destination gets the unchanged copy, as
`exiftool -o` writes it ("1 image files copied"). When none of the
requests can be written at all (a tag ExifTool does not define, `IPTC:City`
in a MOV), a new destination gets the unchanged copy and the call returns
`Ok(0)`; in place it fails and leaves the file alone ("Nothing to do.").

### Extensions

ExifTool's own ways of being extended work in the port as they do in
Perl, since the port runs Perl code: a config file (`.ExifTool_config`,
`-config`, `%Image::ExifTool::UserDefined`) adds tags, composite tags,
shortcuts and options; a custom module can be loaded by it. The Rust API
adds one for what is better written in Rust, a format engine:
`exiftool_port::extension::FormatExtension`.

```rust
pub trait FormatExtension: Send + Sync {
    fn name(&self) -> &str;
    fn file_types(&self) -> &[&str];   // ExifTool FileType names: "MP3", "FLAC"...
    fn write(&self, input: &[u8], file_type: &str, edits: &[NewValue]) -> Result<Vec<u8>>;
    // the edits for the tags a copy queues without a destination group
    // (default: each by its name; see "Equivalent tags in a copy")
    fn copy_edits(&self, file_type: &str, copied: &[CopiedTag]) -> Vec<NewValue> { ... }
}

impl ExifTool {
    pub fn register_extension<E: FormatExtension + 'static>(&mut self, ext: E);
    pub fn clear_extensions(&mut self);
}
```

`write_info` calls an extension only when ExifTool cannot write the
source's type itself: ExifTool's write is tried first, and only when it
fails is the source's `FileType` (as `ImageInfo` finds it) looked up; if
`CanWrite` says no and an extension lists that type (the one registered
last, if several do), the extension writes the whole new file. Its output
then goes through what any write goes through: a temporary file renamed
over the destination, the requests that took effect counted by reading it
back with ExifTool. Nothing changes for a type ExifTool writes. The edits
are the queued requests in order, a copy (`set_new_values_from_file`)
given as one edit per new value ExifTool's `SetNewValuesFromFile` queues
(the facade runs Writer.pl's code for it, with each `SetNewValue` call
recorded instead of made): the tag name, the value ExifTool copies (the
printed value, the ValueConv value for a `TAG#`; a list joined with ", ";
no binary data, structures, list-item deletions `-<`, or tags of the
source's File, System, Composite and ExifTool groups), and the destination
group ExifTool gives it. As in ExifTool, a tag copied without a destination
group (the whole file, a tag list of `Title` or `-all`) goes to the
destination's preferred group for its name, whatever group it had in the
source: a FLAC's `Vorbis:Title` copied to an MP3 is an edit of `Title`,
written as its ID3 Title. A group the tag list names is kept, as ExifTool
keeps it (`Vorbis:Title` alone copies to `Vorbis:Title`; `all:all` to each
tag's own group; `Vorbis:Title>ID3:Title` to ID3), so a copy that names a
group the destination does not have writes nothing. A destination name ExifTool gives a type to must be one of the
extension's types (`.opus` is `OGG`). The warnings of such a write are
empty: ExifTool's (`Sorry, ID3:Title doesn't exist or isn't writable`) would be wrong
there.

The built-in extensions, registered by `ExifTool::new` when the
`extra-writers` cargo feature is on (the default), are the root crate's
writers for the formats Perl ExifTool reads and refuses to write, in
`port/src/api/extra_writers/` (copied from the root crate's src/writer/ and
adapted: tag names as ExifTool reads them, the port's reader in their
tests):

| Extension | Types | Writes |
|---|---|---|
| `Id3Writer` | `MP3`, `AIFF` | ID3v2 frames (in the tag's version; ID3v2.3 for a new tag) and the ID3v1 trailer of an MP3; the `ID3 ` chunk of an AIFF. Groups `ID3`, `ID3v1`, `ID3v2*` |
| `VorbisWriter` | `FLAC`, `OGG`, `OPUS` | the Vorbis comments of FLAC, Ogg Vorbis, Ogg FLAC and Opus files (Ogg pages re-paginated, CRCs recomputed). Group `Vorbis`; a field is matched by the name ExifTool gives it (Vorbis.pm), a new one takes ExifTool's field for the name |
| `MatroskaWriter` | `MKV`, `WEBM` | the segment Title (`Matroska:Title`, read as `Info:Title`) and SimpleTags, positions relocated (SeekHead, Cues, Clusters) |

A request without a group goes to the writer of the file, for a tag of its
format, as ExifTool writes a tag named without a group to the preferred
group that has it; another name is left out, so that a copy from another
format puts nothing in the file that its format does not have:

| Writer | Tags written without a group (ExifTool's names) |
|---|---|
| `Id3Writer` | ID3.pm's: `Title`, `Artist`, `Album`, `Track`, `Year`, `Genre`, `Composer`, `EncodedBy`, `Grouping`, `Band`, `PartOfSet`, `Compilation`, `Subtitle`, `Conductor`, `Publisher`, `Copyright`, `BeatsPerMinute`, `Comment`, `Lyrics` (ID3v2 frames; `Title`, `Artist`, `Album`, `Year`, `Comment`, `Track`, `Genre` in ID3v1 too) |
| `VorbisWriter` | the Vorbis.pm comments with a text value: `Title`, `Version`, `Album`, `TrackNumber`, `Artist`, `Performer`, `Copyright`, `License`, `Organization`, `Description`, `Genre`, `Date`, `Location`, `Contact`, `ISRCNumber`, `ReplayGain*`, `EncodedUsing`, `EncodedBy`, `Comment`, `Director`, `Producer`, `Composer`, `Actor`, `Encoder`, `EncoderOptions` (not `Vendor`, `CoverArt*`, `Picture`) |
| `MatroskaWriter` | the segment `Title` and Matroska.pm's standardized SimpleTags (`%StdTag`, but for its structures): `Artist`, `Composer`, `Lyrics`, `Genre`, `Comment`, `Copyright`, `EncodedBy`, `Subtitle`, `Conductor`, `Director`, `Publisher`, `Description`, `License`, `Encoder`, `DateReleased`, `DateTimeOriginal` (`DATE_RECORDED`)... a new one under its standard field name |

The tags two formats both have carry over in a copy (FLAC to MP3: Title,
Copyright; MP3 to FLAC: Title, Artist, Album, Genre, Comment, Composer,
Copyright...). A tag the source has under a name of the destination's
format is copied whatever it is in the source, as ExifTool copies it to a
format it writes (a Matroska track's `TrackNumber` becomes a Vorbis
`TRACKNUMBER`, as it becomes an M4A's track number).

A request for another group (`EXIF:Artist` in an MP3) is not written and not counted. Perl
ExifTool reads the files they write back with the new values
(`tests/api_media_write.rs` checks each case with the port's reader and,
when found at `$EXIFTOOL_DIR`, with Perl ExifTool 13.59). Without the
feature, or after `clear_extensions`, these formats are refused as Perl
refuses them ("Can't write MP3 files").

The exiftool-rs application (`examples/exiftool_rs.rs`, the port of the
`exiftool` script) does not use them: it stays ExifTool's, so `exiftool-rs
-Title=x a.mp3` says "Can't write MP3 files" as `exiftool` does. Reading
stays ExifTool's for every type (it reads all of these).

#### Equivalent tags in a copy

ExifTool alone would leave out the tags the formats name differently (ID3
`Year` is no Vorbis tag, an M4A's `TrackNumber` is no ID3 `Track`). For a
copy to a format the built-in extensions write, these are also written as
the destination's tag of the same meaning, their values converted
(`FormatExtension::copy_edits`, `port/src/api/extra_writers/equivalents.rs`).
Only for a tag copied **without a destination group** (the whole file, a
tag list of `Year` or `-all`): never for a tag copied to a group the tag
list names (`ID3:Year`, `Year>ID3:Year`, `all:all`), never for a
`set_new_value` request, and never for a format ExifTool writes itself,
whose copies stay exactly ExifTool's. The names are ExifTool's (ID3.pm,
Vorbis.pm, Matroska.pm `%StdTag`, QuickTime.pm `%ItemList`); a Vorbis field
Vorbis.pm does not list is named as ExifTool names it when reading
(`DISCNUMBER` is `Discnumber`) and written under its usual field name. The
M4A's iTunes tags are a source only (ExifTool writes M4A).

| Meaning | ID3 (MP3, AIFF) | Vorbis (FLAC, Ogg, Opus) | Matroska (MKV, WebM) | QuickTime ItemList (source) |
|---|---|---|---|---|
| date | `RecordingTime` (TDRC), `ReleaseTime` (TDRL), `Year` (TYER/TYE/ID3v1, with `Date` TDAT and `Time` TIME); written as `Year` | `Date` (`DATE`) | `DateReleased` (`DATE_RELEASED`); `DateTimeOriginal` (`DATE_RECORDED`, not the segment's Info date) as a source | `ContentCreateDate` (©day) |
| track | `Track` (TRCK, `n/total`) | `TrackNumber` (`TRACKNUMBER`) + `Tracktotal` (`TRACKTOTAL`; `TOTALTRACKS` as a source) | `PartNumber` (`PART_NUMBER`) + `TotalParts` (`TOTAL_PARTS`) | `TrackNumber` (trkn, `n of total`) |
| disc | `PartOfSet` (TPOS, `n/total`) | `Discnumber` (`DISCNUMBER`) + `Disctotal` (`DISCTOTAL`; `TOTALDISCS` as a source) | (none) | `DiskNumber` (disk, `n of total`) |
| album artist | `Band` (TPE2) | `Albumartist` (`ALBUMARTIST`; `ALBUM ARTIST` as a source) | (none) | `AlbumArtist` (aART) |
| compilation | `Compilation` (TCMP, Yes/No) | `Compilation` (`COMPILATION`, 1/0) | (none) | `Compilation` (cpil, Yes/No) |
| beats per minute | `BeatsPerMinute` (TBPM) | `BPM` (`BPM`, read as `Bpm`) | `BPM` (`BPM`) | `BeatsPerMinute` (tmpo) |
| grouping | `Grouping` (TIT1) | `Grouping` (`GROUPING`) | (none) | `Grouping` (©grp) |
| lyrics | `Lyrics` (USLT) | `Lyrics` (`LYRICS`) | `Lyrics` (`LYRICS`) | `Lyrics` (©lyr) |

Values are converted to the destination's form: a date (`2024`,
`2024-05-01`, ExifTool's `2024:05:01 12:00:00`, `...T12:00:00Z`) is its year
for ID3 `Year` (TYER and ID3v1 hold four digits), an ISO 8601 date for a
Vorbis `DATE` (`2024-05-01T12:00:00`), the Matroska spec's form for
`DATE_RELEASED` (`2024-05-01 12:00:00`); an ID3v2.3 `Year` is completed by
its TDAT (`DDMM`) and TIME (`HHMM`), which are no Vorbis `DATE` of their
own. A number with its total (`1/5`, `1 of 5`) is `1/5` in ID3, one tag
and its total in Vorbis (`TRACKNUMBER=1`, `TRACKTOTAL=5`) and Matroska
(`PART_NUMBER`, `TOTAL_PARTS`); a total given apart joins its number. A
compilation flag is 1/0 in Vorbis. Text is copied as it is.

Matroska's choices, per its tagging spec: the writer puts new SimpleTags in
a whole-file Tag (no Targets: level 50, the album), where `TOTAL_PARTS` is
the number of tracks, and `PART_NUMBER` is the track's number as ffmpeg
and mkvtoolnix map a file's track number; the date is `DATE_RELEASED`, as
they map a file's date (`DATE_RECORDED` is read too). There is no disc
number, album artist, compilation or grouping: a disc number is the
`PART_NUMBER` of a level-40 Tag and an album artist the `ARTIST` of the
album's Tag, which ExifTool does not tell from the track's. Pairs that are
not unambiguous are left out: Vorbis `ORGANIZATION` or `LABEL` for ID3
`Publisher`, Vorbis `DESCRIPTION` for a comment, a Matroska album title.
The tags of the same name in every format (`Title`, `Artist`, `Album`,
`Genre`, `Composer`, `Comment`, `Copyright`...) already carry over by name.

Precedence: what the destination's format takes by name comes first. A
tag the source has under the destination's own name, of another format
(an M4A's `TrackNumber` for a Vorbis `TRACKNUMBER`) or unrelated (an XMP
`Date`), is written by its name as ExifTool would, the former converted;
no equivalent is then made for that destination tag (nor its total). A tag
of the destination's own format is never converted (FLAC to Ogg keeps
`TRACKNUMBER=1/5`; its `DISCNUMBER`, which Vorbis.pm does not list, is
written too). Among the source's equivalents, the one of the
destination's name comes first, then the order above (a recording time
before a year). A later `set_new_value` request for the destination's tag
overrides the equivalent, as it overrides any copied tag. The test matrix
(`tests/api_media_copy.rs`: the six formats and an M4A, to each of the
six) reads each output back with the port and with Perl ExifTool 13.59.

The same idea was proposed to ExifTool itself, for the formats it writes:
<https://github.com/exiftool/exiftool/issues/497>.

### Threads

An `ExifTool` keeps one interpreter, and one `Image::ExifTool` object on
it, for all its calls, as a Perl program that makes one
`Image::ExifTool->new` and reads (or writes) file after file with it, and
as the exiftool application does for the files of one command. Each call
starts with `ClearOptions()` and `SetNewValue()` (no option or new value
stays from the call before), then sets its options. What ExifTool itself
keeps from one file to the next stays, as in Perl: its tables, the modules
loaded. With one `Image::ExifTool` object in Perl, as with one `ExifTool`
here, XMP.xml has no AutoFocus composite read first, and has one ("On")
read after Nikon.jpg, whose module defines it; after writing Kodak.jpg's
Make, the output reads "Unrecognized MakerNoteUnknown" instead of
"Unrecognized MakerNotes". Two `ExifTool` objects never share anything.

`Options::fresh_interpreter` (off by default) runs each call on an
interpreter of its own instead, with a new `Image::ExifTool` object, as
each run of the exiftool application is a process of its own: no call
then depends on the ones before it, at the price of the interpreter's
start (about 16 ms) for each call. Reading ExifTool's 194 test images with
one `ExifTool`: 4.46 s with `fresh_interpreter`, 2.57 s without (-42 %);
the output is the same but for XMP.xml's AutoFocus above (and the order of
some composite tags, as ExifTool's composite table grows).

`ExifTool` is `Send + Sync`. The interpreter is not `Send` (it holds
`Rc`s, some shared with caches of its thread), so it lives on a thread of
its own, started on the object's first call and ended when the object is
dropped, which runs the calls one at a time: threads that share an
`ExifTool` take turns (one `ExifTool` per thread reads in parallel). A
panic in a call is the caller's, and the interpreter it leaves is
replaced. With `fresh_interpreter`, calls run on the caller's thread, in
parallel. Turning `use_mwg` off after a call with it replaces the
interpreter (MWG cannot be unloaded).

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
  Matroska) are written by the built-in extensions, the root crate's
  writers (see Extensions); without them they are refused with
  `Error::UnsupportedFileType("Can't write MP3 files")`, as ExifTool
  refuses them. `writable_tags` is ExifTool's whole
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
  keeps them; only its text output trims them). The root crate trimmed them;
  `Options::trim` does.
- **Tag order** is ExifTool's (`GetTagList`, as `exiftool` lists them).
- **`image_info` with duplicates** keys a duplicate `Name [Group0:Group1]`
  as before, but which instance is the plain `Name` is ExifTool's choice.
- **Binary data** ExifTool does not extract unless asked (a preview image,
  a JpgFromRaw) is the `(Binary data N bytes, ...)` text; set
  `Options::binary` or request the tag (`requested_tags`) to get its bytes,
  as with ExifTool.
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
