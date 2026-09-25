#!/bin/sh
# Build the starting corpus from tests/images: each file, prefixed with the
# byte that makes fuzz_targets/extract.rs pick its own extension back.
set -eu
cd "$(dirname "$0")"
mkdir -p corpus/extract
for f in ../tests/images/*.*; do
    ext=$(printf '%s' "${f##*.}" | tr 'A-Z' 'a-z')
    n=$(grep -nx "$ext" extensions.txt | cut -d: -f1) || continue
    printf "\\$(printf '%03o' $((n - 1)))" > "corpus/extract/$(basename "$f")"
    cat "$f" >> "corpus/extract/$(basename "$f")"
done
echo "$(ls corpus/extract | wc -l) seeds in corpus/extract"
