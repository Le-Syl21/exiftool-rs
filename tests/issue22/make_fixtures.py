"""Build the issue #22 PNG fixtures (synthetic metadata, a 4x3 RGB image).

Run from the repository root: python3 tests/issue22/make_fixtures.py
The ICC profile is tests/images/ICC_Profile.icc (from ExifTool's test suite).
"""
import struct
import zlib

OUT = "tests/issue22/"


def chunk(typ, data):
    return struct.pack(">I", len(data)) + typ + data + struct.pack(">I", zlib.crc32(typ + data))


def compress(data):
    return zlib.compress(data, 9)


ICC = open("tests/images/ICC_Profile.icc", "rb").read()
XMP = (
    b'<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF '
    b'xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description '
    b'rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:description><rdf:Alt>'
    b'<rdf:li xml:lang="x-default">Fixture description</rdf:li></rdf:Alt></dc:description>'
    b"</rdf:Description></rdf:RDF></x:xmpmeta>"
)
NOT_XMP = b"<description>Fixture descriptive text, not XMP</description>"
PIXELS = b"".join(
    b"\0" + b"".join(bytes([x * 60, y * 80, 120]) for x in range(4)) for y in range(3)
)
TIFF = b"MM\0*\0\0\0\x08\0\x01\x01\x31\0\x02\0\0\0\x05\0\0\0\x1a\0\0\0\0test\0"


def raw_profile(kind, payload):
    text = b"\n" + kind + b"\n" + b"%8d\n" % len(payload) + payload.hex().encode() + b"\n"
    return chunk(b"zTXt", b"Raw profile type " + kind + b"\0\0" + compress(text))


def png(head, xmp_chunk, tail=b"", trailer=b""):
    out = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 4, 3, 8, 2, 0, 0, 0))
    out += head
    out += chunk(b"gAMA", struct.pack(">I", 45455))
    out += chunk(b"iCCP", b"Fixture ICC\0\0" + compress(ICC))
    out += chunk(b"pHYs", struct.pack(">IIB", 2835, 2835, 1))
    out += chunk(b"tIME", struct.pack(">HBBBBB", 2026, 10, 7, 12, 34, 56))
    out += chunk(b"tEXt", b"Author\0Fixture PNG author")
    out += xmp_chunk + tail
    out += chunk(b"IDAT", compress(PIXELS)) + chunk(b"IEND", b"") + trailer
    return out


fixtures = {
    # The XMP keyword in each of the three text chunk types.
    "text": png(b"", chunk(b"tEXt", b"XML:com.adobe.xmp\0" + XMP)),
    "itxt": png(b"", chunk(b"iTXt", b"XML:com.adobe.xmp\0\0\0\0\0" + XMP)),
    "ztxt": png(b"", chunk(b"zTXt", b"XML:com.adobe.xmp\0\0" + compress(XMP))),
    # With a language code the keyword is no longer the XMP directory but an
    # unknown text tag, which ExifTool reads as PNG:XMLcomadobexmp.
    # (with an eXIf chunk).
    "lang": png(
        chunk(b"eXIf", TIFF), chunk(b"iTXt", b"XML:com.adobe.xmp\0\0\0en\0\0" + NOT_XMP)
    ),
    # Everything else a PNG group deletion can reach, and what it must not.
    "sink": png(
        chunk(b"sRGB", b"\0")
        + chunk(b"cHRM", struct.pack(">8I", 31270, 32900, 64000, 33000, 30000, 60000, 15000, 6000))
        + chunk(b"sBIT", b"\x08\x08\x08")
        + chunk(b"cICP", b"\x01\x0d\x00\x01")
        + raw_profile(b"exif", TIFF)
        + raw_profile(b"foo", b"abc")
        + chunk(b"tEXt", b"Comment\0Fixture comment")
        + chunk(b"zTXt", b"Title\0\0" + compress(b"Fixture title"))
        + chunk(b"iTXt", b"Description\0\0\0fr\0Description\0Fixture description fr")
        + raw_profile(b"xmp", XMP.replace(b"description", b"format"))
        + chunk(b"seAl", b'<seal seal="1"/>'),
        chunk(b"iTXt", b"XML:com.adobe.xmp\0\0\0\0\0" + XMP),
        chunk(b"PLTE", b"\0\0\0\xff\xff\xff") + chunk(b"bKGD", b"\0\0\0\0\0\0"),
        b"FIXTURE TRAILER",
    ),
}
for name, data in fixtures.items():
    with open(OUT + "issue22_" + name + ".png", "wb") as f:
        f.write(data)
