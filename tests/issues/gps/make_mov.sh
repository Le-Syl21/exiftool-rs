#!/bin/sh
# Regenerate gps.mov (issue #26): a one-second black MOV whose Keys hold an
# ISO 6709 location, with the command given in the issue. The committed file
# (1033 bytes) was written by Lavf63.1.100, as its Encoder tag says; other
# FFmpeg versions produce different bytes.
cd "$(dirname "$0")" || exit 1
ffmpeg -hide_banner -loglevel error -y \
  -f lavfi -i color=c=black:s=16x16:r=1 -t 1 -c:v mpeg4 \
  -metadata com.apple.quicktime.location.ISO6709=+12.5000-045.2500+010.000/ \
  -movflags use_metadata_tags gps.mov
