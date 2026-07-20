#!/usr/bin/env bash
# Show, for one test image, each baseline group delta as `tag  want -> got`.
# Usage: scripts/show_group_deltas.sh <image-file-name> [family1|family2]
set -euo pipefail
cd "$(dirname "$0")/.."
file=$1
fam=${2:-family1}
col=$([ "$fam" = family1 ] && echo 2 || echo 3)
gflag=$([ "$fam" = family1 ] && echo -G1 || echo -G2)

got_file=$(mktemp)
trap 'rm -f "$got_file"' EXIT
./target/release/exiftool-rs "$gflag" "tests/images/$file" 2>/dev/null |
  sed -n 's/^\[\([^]]*\)\] *\([^ ]*\) *:.*/\2\t\1/p' >"$got_file"

awk -F'\t' -v f="$file" -v fam="$fam" '$1==f && $3==fam {print $2}' tests/group_baseline.txt |
  while read -r tag; do
    want=$(awk -F'\t' -v t="$tag" -v c="$col" '$1==t {print $c; exit}' "tests/expected_groups/$file.grps")
    got=$(awk -F'\t' -v t="$tag" '$1==t {print $2; exit}' "$got_file")
    printf '%-32s want=%-18s got=%s\n' "$tag" "$want" "$got"
  done
