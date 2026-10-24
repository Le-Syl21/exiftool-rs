import struct
from pathlib import Path
u16=lambda x:struct.pack("<H",x)
u32=lambda x:struct.pack("<I",x)
def entry(tag,typ,count,value):return u16(tag)+u16(typ)+u32(count)+value
settings=[0]*26
settings[0]=52
settings[22]=65535  # unknown lens type
settings[23]=672   # long focal, in focal units
settings[24]=224   # short focal, in focal units
settings[25]=32    # focal units per mm
cs=struct.pack("<26H",*settings)
fl=struct.pack("<4H",2,224,320,240)
# Absolute TIFF offsets: Make=38, ExifIFD=44, MakerNote=62,
# CameraSettings=92, FocalLength block=144.
maker=u16(2)+entry(1,3,26,u32(92))+entry(2,3,4,u32(144))+u32(0)+cs+fl
tiff=b"II"+u16(42)+u32(8)+u16(2)
tiff+=entry(0x010f,2,6,u32(38))+entry(0x8769,4,1,u32(44))+u32(0)
tiff+=b"Canon\0"+u16(1)+entry(0x927c,7,len(maker),u32(62))+u32(0)+maker
Path(__file__).with_name("lens.tif").write_bytes(tiff)

