import struct,json
from pathlib import Path
root=Path(__file__).parent
settings=[0]*26
settings[0]=52
settings[25]=32
blocks=[(0x102d,struct.pack("<26H",*settings)),(0x1029,struct.pack("<4H",2,224,0,0))]
body=b"";entries=[]
for tag,data in blocks:
 entries.append(struct.pack("<HII",tag,len(data),len(body)))
 body+=data
dir_offset=len(body)
body+=struct.pack("<H",len(entries))+b"".join(entries)+struct.pack("<I",dir_offset)
header=b"II"+struct.pack("<I",26)+b"HEAPCCDR"+bytes(12)
(root/"focal.crw").write_bytes(header+body)
(root/"manifest.json").write_text(json.dumps([str(root/"focal.crw")]))
