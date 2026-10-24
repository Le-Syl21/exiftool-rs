import struct
from pathlib import Path
u16=lambda x:struct.pack("<H",x)
u32=lambda x:struct.pack("<I",x)
def entry(tag,typ,count,value):return u16(tag)+u16(typ)+u32(count)+value
make=b"SAMSUNG\0"; lens=b"NX 45mm F1.8\0"; firmware=b"1.00\0"; spare=bytes(30)
exif_offset=38+len(make)
lens_offset=exif_offset+30
maker_offset=lens_offset+len(lens)
firmware_offset=maker_offset+42
spare_offset=firmware_offset+len(firmware)
maker=u16(3)+entry(0xa001,2,len(firmware),u32(firmware_offset))+entry(0xa002,2,len(spare),u32(spare_offset))+entry(0xa003,3,1,u16(10)+u16(0))+u32(0)+firmware+spare
exif=u16(2)+entry(0x927c,7,len(maker),u32(maker_offset))+entry(0xa434,2,len(lens),u32(lens_offset))+u32(0)
tiff=b"II"+u16(42)+u32(8)+u16(2)+entry(0x010f,2,len(make),u32(38))+entry(0x8769,4,1,u32(exif_offset))+u32(0)+make+exif+lens+maker
root=Path(__file__).parent
(root/"samsung.srw").write_bytes(tiff)
with_serial=bytearray(tiff)
with_serial[-30:]=b"DEMO123"+bytes(23)
(root/"serial.srw").write_bytes(with_serial)

