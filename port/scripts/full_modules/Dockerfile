# The reference Perl of the second judge set (cli_cases_full_modules.txt):
# the host's Perl (Ubuntu 24.04, perl 5.38, the same packaged modules) plus
# the two optional modules the host lacks and the port provides,
# Compress::Raw::Lzma and Unicode::LineBreak (Unicode::GCString).
FROM ubuntu:24.04
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        perl libarchive-zip-perl libio-compress-brotli-perl \
        libcompress-raw-lzma-perl libunicode-linebreak-perl util-linux \
    && rm -rf /var/lib/apt/lists/*
