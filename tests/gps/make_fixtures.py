"""Build the GPS fixtures: a GPS IFD in a JPEG and in a CR3, for two places.

Run from the repository root: python3 tests/gps/make_fixtures.py

`tokyo` encodes 35 40.5087' N 139 43.157' E as 35/1 405087/10000 0/1 and
139/1 43157/1000 0/1, 38.5 m above sea level. `southwest` is in the southern
and western hemispheres, 12 m below sea level, with seconds that round (36.00
and 35.99), and a time stamp whose fraction is longer than the six digits
PrintConv keeps. Each has a date stamp, so GPSDateTime is built. The status and measure mode are
known values in `tokyo` and unknown ones in `southwest` (empty, and "9"), which
PrintConv shows as "Unknown (...)". A JPEG keeps its GPS IFD behind a pointer in IFD0; a CR3 keeps it
as IFD0 of the standalone TIFF in its CMT4 box, which takes a different route
through the EXIF reader.
"""
import struct

OUT = "tests/gps/"


def ifd(entries, at):
    """Little-endian IFD at offset `at`: (tag, type, count, data) sorted by tag,
    values over four bytes in a data area after the table."""
    entries = sorted(entries)
    table = 2 + len(entries) * 12 + 4
    out = struct.pack("<H", len(entries))
    area = b""
    for tag, kind, count, data in entries:
        out += struct.pack("<HHI", tag, kind, count)
        if len(data) <= 4:
            out += data.ljust(4, b"\0")
        else:
            out += struct.pack("<I", at + table + len(area))
            area += data
            if len(area) % 2:
                area += b"\0"
    return out + struct.pack("<I", 0) + area


def ascii_(tag, s):
    return (tag, 2, len(s) + 1, s.encode() + b"\0")


def rationals(tag, values):
    return (tag, 5, len(values), b"".join(struct.pack("<II", n, d) for n, d in values))


def gps_entries(p):
    return [
        (0, 1, 4, bytes([2, 3, 0, 0])),
        ascii_(1, p["lat_ref"]),
        rationals(2, p["lat"]),
        ascii_(3, p["lon_ref"]),
        rationals(4, p["lon"]),
        (5, 1, 1, bytes([p["below_sea_level"]])),
        rationals(6, [p["altitude"]]),
        rationals(7, p["time"]),
        ascii_(9, p["status"]),
        ascii_(10, p["mode"]),
        ascii_(0x1D, p["date"]),
    ]


HEADER = b"II*\0\x08\0\0\0"


def tiff_gps_as_ifd0(p):
    return HEADER + ifd(gps_entries(p), 8)


def tiff_with_gps_pointer(p):
    gps_at = 8 + 2 + 12 + 4
    pointer = (0x8825, 4, 1, struct.pack("<I", gps_at))
    return HEADER + ifd([pointer], 8) + ifd(gps_entries(p), gps_at)


def jpeg(p):
    app1 = b"Exif\0\0" + tiff_with_gps_pointer(p)
    return b"\xff\xd8\xff\xe1" + struct.pack(">H", len(app1) + 2) + app1 + b"\xff\xd9"


def box(kind, body):
    return struct.pack(">I", len(body) + 8) + kind + body


def cr3(p):
    canon = bytes.fromhex("85c0b687820f11e08111f4ce462b6a48")
    moov = box(b"moov", box(b"uuid", canon + box(b"CMT4", tiff_gps_as_ifd0(p))))
    return box(b"ftyp", b"crx \0\0\0\x01crx isom") + moov


PLACES = {
    "tokyo": dict(
        lat_ref="N",
        lat=[(35, 1), (405087, 10000), (0, 1)],
        lon_ref="E",
        lon=[(139, 1), (43157, 1000), (0, 1)],
        below_sea_level=0,
        altitude=(385, 10),
        time=[(5, 1), (3, 1), (245, 10)],
        date="2026:10:07",
        status="A",
        mode="3",
    ),
    "southwest": dict(
        lat_ref="S",
        lat=[(33, 1), (51, 1), (3600, 100)],
        lon_ref="W",
        lon=[(151, 1), (12, 1), (3599, 100)],
        below_sea_level=1,
        altitude=(12, 1),
        time=[(23, 1), (59, 1), (591234567, 10000000)],
        date="2026:10:08",
        status="",
        mode="9",
    ),
}

for name, place in PLACES.items():
    for ext, build in (("jpg", jpeg), ("cr3", cr3)):
        with open(f"{OUT}gps_{name}.{ext}", "wb") as f:
            f.write(build(place))
