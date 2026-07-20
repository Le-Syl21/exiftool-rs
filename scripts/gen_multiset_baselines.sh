#!/bin/bash
# Regenerate the multiset value oracle:
#     tests/expected_multi/<image>.mvals       (default extraction)
#     tests/expected_multi/<image>.ee.mvals    (-ee, ExtractEmbedded)
#
# Each file holds the COMPLETE multiset of tags Perl ExifTool emits:
#
#     <tag name><TAB><printed value>
#
# one line per EMITTED TAG, duplicates included, sorted with LC_ALL=C.
#
# Why a second value oracle
# -------------------------
# `tests/expected_values/*.vals` keeps only the FIRST occurrence per tag name and
# only covers the default mode, so it is blind to two whole classes of divergence:
# emitting a tag twice where ExifTool emits it once, and everything reached only
# through -ee. This oracle keeps every occurrence and covers both modes.
#
# Recipe
# ------
# `-S` prints `Name: value` with no column padding. The padding of the default
# output is presentation, not data; `-S` removes it so the parse can anchor on a
# single unambiguous separator. (Do NOT parse the default padded output: names
# containing `-` and names 32 characters or longer are formatted differently
# there, which is exactly how 301 lines of the `.vals` oracle ended up without a
# TAB and therefore silently uncompared.)
#
# No `-G` option is used, so no `--sort` is needed: with `-G` and no explicit tag
# list exiftool re-sorts its output by group, which would scramble file order.
#
# A printed value may contain newlines. Records are therefore split on a newline
# that is followed by a `Name: ` header rather than on every newline, and the
# newlines left inside a record are collapsed the same way the crate's `-s`
# display sanitization does it: NULs dropped, every other control character (this
# includes \n and \r) replaced by `.`, trailing whitespace trimmed. The result is
# always exactly one line per tag, so `name<TAB>value` never loses a field.
# A continuation line that itself looks like `Word: something` would be split off
# as a spurious tag; no corpus file currently triggers that.
#
# Volatile tags (clock, inode, path, crate version) are excluded — same list as
# FIT_VOLATILE_TAGS in tests/regression.rs.
#
# Usage (from the crate root):
#   ./scripts/gen_multiset_baselines.sh [path/to/exiftool/dir]
#
# Default Perl ExifTool location: /home/sylvain/dev/exiftool (run `perl exiftool`
# from inside that directory). Current oracle version: 13.59.
#
# NOTE: ZIP-based containers (OOXML.docx, OpenDoc.ods, iWork.numbers,
# CaptureOne.eip) need the Perl module Archive::Zip. Verify with
# `perl -MArchive::Zip -e1` before trusting anything this script writes.

set -uo pipefail

CRATE_DIR="$(cd "$(dirname "$0")/.." && pwd)"
EXIFTOOL_DIR="${1:-/home/sylvain/dev/exiftool}"

IMAGES_DIR="$CRATE_DIR/tests/images"
OUT_DIR="$CRATE_DIR/tests/expected_multi"

if [ ! -f "$EXIFTOOL_DIR/exiftool" ]; then
  echo "Perl ExifTool not found at $EXIFTOOL_DIR/exiftool" >&2
  exit 1
fi

if ! perl -MArchive::Zip -e1 >/dev/null 2>&1; then
  echo "Archive::Zip is missing: ZIP containers would be extracted only partially." >&2
  exit 1
fi

# Splits an ExifTool `-S` stream into one `name<TAB>value` line per tag.
PARSE='
  BEGIN {
    %skip = map { $_ => 1 } qw(
      Directory ExifToolVersion FileAccessDate FileInodeChangeDate
      FileModifyDate FileName FilePermissions FileSize
    );
    $/ = undef;
  }
  my $text = <>;
  defined $text or exit 0;
  # The stream-final newline is a line terminator, not part of the last value.
  # (chomp would be a no-op here: $/ is undef for the slurp above.)
  $text =~ s/\n\z//;
  # Split before every line that opens a new tag record.
  my @rec = split /\n(?=[A-Za-z][-A-Za-z0-9_]*:\ )/, $text;
  for my $r (@rec) {
    next unless $r =~ /^([A-Za-z][-A-Za-z0-9_]*):\ (.*)$/s;
    my ($name, $value) = ($1, $2);
    next if $skip{$name};
    $value =~ s/\0//g;                 # NULs are dropped
    $value =~ s/[\x01-\x1f\x7f]/./g;   # every other control char -> "."
    $value =~ s/\s+$//;                # trailing whitespace trimmed
    print "$name\t$value\n";
  }
'

mkdir -p "$OUT_DIR"
echo "Perl ExifTool $(cd "$EXIFTOOL_DIR" && perl exiftool -ver)"

count=0
for img in "$IMAGES_DIR"/*; do
  [ -f "$img" ] || continue
  base="$(basename "$img")"
  (cd "$EXIFTOOL_DIR" && perl exiftool -S "$img" 2>/dev/null) |
    perl -e "$PARSE" | LC_ALL=C sort > "$OUT_DIR/$base.mvals"
  (cd "$EXIFTOOL_DIR" && perl exiftool -S -ee "$img" 2>/dev/null) |
    perl -e "$PARSE" | LC_ALL=C sort > "$OUT_DIR/$base.ee.mvals"
  count=$((count + 1))
done

echo "Wrote $((count * 2)) .mvals file(s) to $OUT_DIR"
