import struct
from pathlib import Path
root=Path(__file__).parent
for name,raw in [("plain.tif",b"oplus_DEMO_TEXT"),("ascii.tif",b"ASCII\0\0\0oplus_DEMO_TEXT")]:
    # IFD0 -> ExifIFD -> UserComment (UNDEFINED, tag 0x9286).
    data=b"II"+struct.pack("<HIH",42,8,1)
    data+=struct.pack("<HHII",0x8769,4,1,26)+struct.pack("<I",0)
    data+=struct.pack("<H",1)+struct.pack("<HHII",0x9286,7,len(raw),44)
    data+=struct.pack("<I",0)+raw
    (root/name).write_bytes(data)

