#!/bin/bash
# `perl ARGS...` in the container of ./Dockerfile (image exiftool-rs-fullperl,
# built with `docker build -t exiftool-rs-fullperl:local scripts/full_modules`):
# the host's Perl plus Compress::Raw::Lzma and Unicode::LineBreak, for the
# second judge set. The current directory and EXIFTOOL_SRC (ExifTool's tree,
# read-only) are mounted at the same paths, so paths mean the same on both
# sides; EXIFTOOL_HOME is passed through.
#
#   PERL_CMD=scripts/full_modules/perl.sh CLI_CASES=scripts/cli_cases_full_modules.txt \
#       python3 -I scripts/compare_cli.py ~/dev/exiftool
set -e
src=${EXIFTOOL_SRC:?EXIFTOOL_SRC must name ExifTool\'s source tree}
args=(--rm -i -u "$(id -u):$(id -g)" -v "$PWD:$PWD" -v "$src:$src:ro" -w "$PWD" -v /etc/localtime:/etc/localtime:ro)
if [ -n "$EXIFTOOL_HOME" ]; then
    args+=(-v "$EXIFTOOL_HOME:$EXIFTOOL_HOME" -e "EXIFTOOL_HOME=$EXIFTOOL_HOME")
fi
exec docker run "${args[@]}" exiftool-rs-fullperl:local \
    prlimit --as=4000000000 -- timeout 300 perl "$@"
