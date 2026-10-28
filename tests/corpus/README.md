# Test corpora

Large corpora are not stored in the repository: each one is a
`.tar.xz` file attached to a GitHub release of this repository, and
`port/scripts/fetch_corpus.sh` downloads and unpacks it into
`target/corpus/`, checking its SHA-256.

| Corpus | Release | Files | Size | Source |
|---|---|---|---|---|
| `rawsamples` | [`corpus-rawsamples-1`](https://github.com/Le-Syl21/exiftool-rs/releases/tag/corpus-rawsamples-1) | 458 | 175 MB | [RAW-Samples.ch](https://rawsamples.ch/) |

## rawsamples

RAW files from nearly every camera brand (Canon, Nikon, Sony, Fujifilm,
Panasonic, Olympus, Pentax, Sigma, Leica, Kodak, Hasselblad, phones...),
taken from the archive of RAW-Samples.ch (October 2019), nested archives
unpacked.

Only the metadata is kept: every byte that ExifTool 13.59 does not read was
set to zero (the image data), and the scan data of embedded JPEG images was
zeroed except its markers. For each file, ExifTool reports exactly the same
metadata (`-a -u -n`, duplicates included) as for the original. One file
of the source archive fails its CRC check and was left out.

RAW-Samples.ch offers its files under a Creative Commons licence (its legal
page names CC BY-SA 2.5 Switzerland and links CC BY-NC-SA 4.0); these
derived files are shared under the same terms, not under this repository's
GPL.

Thanks to RAW-Samples.ch and the photographers who contributed to it, and to
[julyx10](https://github.com/julyx10), author of
[Lap](https://github.com/julyx10/lap), whose many detailed reports and
testing on these files led us to use them.
