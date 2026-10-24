import struct,json
from pathlib import Path
root=Path(__file__).parent
attrs=[]
def attr(name,kind,data):attrs.append(name.encode()+b"\0"+kind.encode()+b"\0"+struct.pack("<I",len(data))+data)
channels=b"".join(n+b"\0"+struct.pack("<iB3xii",1,0,1,1) for n in [b"B",b"G",b"R"])+b"\0"
attr("channels","chlist",channels)
attr("compression","compression",bytes([0]))
for name in ["dataWindow","displayWindow"]:attr(name,"box2i",struct.pack("<4i",0,0,0,0))
attr("lineOrder","lineOrder",bytes([0]))
attr("pixelAspectRatio","float",struct.pack("<f",1))
attr("screenWindowCenter","v2f",struct.pack("<2f",0,0))
attr("screenWindowWidth","float",struct.pack("<f",1))
attr("capDate","string",b"2026:01:02 03:04:05")
for name,value in [("isoSpeed",200),("expTime",0.125),("focus",2.5),("latitude",12.5),("longitude",-45.25),("altitude",10)]:attr(name,"float",struct.pack("<f",value))
header=bytes([0x76,0x2f,0x31,0x01])+struct.pack("<I",2)+b"".join(attrs)+b"\0"
# One uncompressed scanline with three HALF channels, one pixel.
file=header+struct.pack("<Q",len(header)+8)+struct.pack("<ii",0,6)+bytes(6)
(root/"camera.exr").write_bytes(file)
(root/"manifest.json").write_text(json.dumps([str(root/"camera.exr")]))

