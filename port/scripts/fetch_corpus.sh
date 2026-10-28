#!/bin/sh
# Download a test corpus attached to a release of this repository and unpack
# it into target/corpus/ (see tests/corpus/README.md).
#   sh scripts/fetch_corpus.sh [rawsamples]
set -e
here=$(cd "$(dirname "$0")/.." && pwd)
name=${1:-rawsamples}
case "$name" in
    rawsamples)
        tag=corpus-rawsamples-1
        file=rawsamples-corpus-1.tar.xz
        sha=c1af4007de3de838830a752b1d9257bd1f63cbb935f81a09beb00322bc6c5098
        ;;
    *) echo "unknown corpus: $name" >&2; exit 1 ;;
esac
dest="$here/../target/corpus"
mkdir -p "$dest"
if [ ! -d "$dest/$name" ]; then
    curl -fsSL -o "$dest/$file" "https://github.com/Le-Syl21/exiftool-rs/releases/download/$tag/$file"
    echo "$sha  $dest/$file" | sha256sum -c --quiet
    tar -C "$dest" -xJf "$dest/$file"
    rm "$dest/$file"
fi
echo "$dest/$name"
