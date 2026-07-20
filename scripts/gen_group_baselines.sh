#!/bin/bash
# Regenerate the group oracle: tests/expected_groups/<image>.grps
#
# One file per corpus image, holding the family-1 and family-2 group that Perl
# ExifTool assigns to every tag it extracts:
#
#     <tag name><TAB><family1><TAB><family2>
#
# One line per tag NAME, sorted with LC_ALL=C. Nothing is excluded: unlike the
# `.vals` oracle we keep the volatile System/ExifTool pseudo-tags, because their
# VALUES move but their GROUPS do not — and they are a large share of the
# group deltas we are chasing.
#
# Recipe
# ------
# `-G1:2` prints both families at once as `[family1:family2] Name: value`, so a
# single pass keeps the two families consistent with each other. When the two
# families carry the same name ExifTool collapses them to `[Name]` (this is what
# the ExifTool-family pseudo-tags do: family1 = family2 = "ExifTool"), so a
# bracket without a colon means "both families hold this value".
#
# `--sort` is required: with a `-G` option and no explicit tag list, exiftool
# re-sorts its output by group. `--sort` restores the plain `-S` file order, so
# the "first occurrence wins" rule below picks the same tag the `.vals` oracle
# picked.
#
# Values may contain colons, brackets and newlines, so parsing anchors on the
# leading `[group] Name: ` pattern and ignores everything that does not match
# (continuation lines of multi-line values).
#
# Usage (from the crate root):
#   ./scripts/gen_group_baselines.sh [path/to/exiftool/dir]
#
# Default Perl ExifTool location: /home/sylvain/dev/exiftool (run `perl exiftool`
# from inside that directory). Current oracle version: 13.59.

set -uo pipefail

CRATE_DIR="$(cd "$(dirname "$0")/.." && pwd)"
EXIFTOOL_DIR="${1:-/home/sylvain/dev/exiftool}"

IMAGES_DIR="$CRATE_DIR/tests/images"
OUT_DIR="$CRATE_DIR/tests/expected_groups"

if [ ! -f "$EXIFTOOL_DIR/exiftool" ]; then
  echo "Perl ExifTool not found at $EXIFTOOL_DIR/exiftool" >&2
  exit 1
fi

mkdir -p "$OUT_DIR"
echo "Perl ExifTool $(cd "$EXIFTOOL_DIR" && perl exiftool -ver)"

count=0
for img in "$IMAGES_DIR"/*; do
  [ -f "$img" ] || continue
  base="$(basename "$img")"
  (cd "$EXIFTOOL_DIR" && perl exiftool -G1:2 -S --sort "$img" 2>/dev/null) |
    perl -ne '
      next unless /^\[([^\]]*)\]\s+([A-Za-z][-A-Za-z0-9_]*):\s/;
      my ($grp, $name) = ($1, $2);
      # "[A:B]" -> family1 A, family2 B; "[A]" -> both families are A.
      my ($f1, $f2) = $grp =~ /:/ ? split(/:/, $grp, 2) : ($grp, $grp);
      next if $seen{$name}++;   # first occurrence wins
      print "$name\t$f1\t$f2\n";
    ' | LC_ALL=C sort > "$OUT_DIR/$base.grps"
  count=$((count + 1))
done

echo "Wrote $count .grps file(s) to $OUT_DIR"
