# The reference Perl of the second judge set (cli_cases_full_modules.txt):
# the host's Perl (Ubuntu 24.04, perl 5.38, the same packaged modules) plus
# the optional modules the host lacks and the port provides:
# Compress::Raw::Lzma, Unicode::LineBreak (Unicode::GCString),
# POSIX::strptime, IO::String, and File::StatX (from CPAN, built here),
# with Term::ReadKey as on the host.
FROM ubuntu:24.04
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        perl libarchive-zip-perl libio-compress-brotli-perl \
        libcompress-raw-lzma-perl libunicode-linebreak-perl util-linux \
        libposix-strptime-perl libio-string-perl libterm-readkey-perl \
        build-essential cpanminus ca-certificates \
    && cpanm --notest File::StatX \
    && rm -rf /var/lib/apt/lists/* /root/.cpanm
