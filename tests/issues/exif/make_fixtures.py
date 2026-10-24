"""Write the two fixtures of this folder that the issue reports did not supply.

Usage, from the repository root:
    python3 tests/issues/exif/make_fixtures.py

- `unknown_values.tif`: values the PrintConv hashes of Exif::Main and
  GPS::Main have no entry for (Compression, Orientation, ResolutionUnit,
  YCbCrPositioning, ExposureProgram, MeteringMode, Flash, ColorSpace,
  ExposureMode, GPSAltitudeRef, GPSDifferential). ExifTool prints each as
  `Unknown (value)`, or `Unknown (0x..)` for the PrintHex ColorSpace and Flash.
- `xp_unicode.tif`: the five Windows XP* tags (four as BYTE, the way Windows
  writes them, one as UNDEFINED) and a UserComment with a UNICODE character
  code, in UTF-16 little-endian.

The fixtures of issues #27 (`unicode.tif`, `empty.tif`), #31 (`plain.tif`,
`ascii.tif`, from `gen31.py`) and #34 (`orientation*.tif`) come from the
reports themselves.
"""
import struct
from pathlib import Path

ROOT = Path(__file__).parent


def ifd(entries, offset):
    """An IFD at `offset`: (tag, type, bytes) entries, then their data."""
    data_offset = offset + 2 + 12 * len(entries) + 4
    out = struct.pack("<H", len(entries))
    blobs = b""
    for tag, typ, raw in sorted(entries):
        count = len(raw) if typ in (1, 2, 7) else len(raw) // {3: 2, 4: 4}[typ]
        if len(raw) <= 4:
            out += struct.pack("<HHI", tag, typ, count) + raw.ljust(4, b"\0")
        else:
            out += struct.pack("<HHII", tag, typ, count, data_offset + len(blobs))
            blobs += raw + b"\0" * (len(raw) % 2)
    return out + struct.pack("<I", 0) + blobs


def tiff(ifd0, sub_ifds):
    """A little-endian TIFF: IFD0, then each (pointer tag, entries) sub-IFD."""
    pointers = [(tag, 4, b"\0\0\0\0") for tag, _ in sub_ifds]
    offset = 8 + len(ifd(ifd0 + pointers, 8))
    body = b""
    for tag, entries in sub_ifds:
        pointers = [p if p[0] != tag else (tag, 4, struct.pack("<I", offset)) for p in pointers]
        block = ifd(entries, offset)
        body += block
        offset += len(block)
    return b"II" + struct.pack("<HI", 42, 8) + ifd(ifd0 + pointers, 8) + body


def short(v):
    return struct.pack("<H", v)


def utf16(text):
    return text.encode("utf-16-le")


(ROOT / "unknown_values.tif").write_bytes(tiff(
    [(0x0103, 3, short(99)), (0x0112, 3, short(0)), (0x0128, 3, short(5)),
     (0x0213, 3, short(7))],
    [(0x8769, [(0x8822, 3, short(77)), (0x9207, 3, short(200)),
               (0x9209, 3, short(0x99)), (0xA001, 3, short(7)),
               (0xA402, 3, short(9))]),
     (0x8825, [(0x0005, 1, b"\x05"), (0x001E, 3, short(9))])],
))

(ROOT / "xp_unicode.tif").write_bytes(tiff(
    [(0x9C9B, 1, utf16("Tïtle") + b"\0\0"),
     (0x9C9C, 1, utf16("Comment") + b"\0\0"),
     (0x9C9D, 1, utf16("Auth中") + b"\0\0"),
     (0x9C9E, 1, utf16("a;b") + b"\0\0"),
     (0x9C9F, 7, utf16("Subject") + b"\0\0")],
    [(0x8769, [(0x9286, 7, b"UNICODE\0" + utf16("héllo wörld"))])],
))
