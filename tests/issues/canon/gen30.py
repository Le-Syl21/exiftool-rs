import struct,json
from pathlib import Path
root=Path(__file__).parent
blocks=[(0x080a,b"Canon\0Canon EOS 300D DIGITAL\0"),(0x101c,struct.pack("<H",100)),(0x102a,struct.pack("<3h",6,0,224))]
body=b"";entries=[]
for tag,data in blocks:
 entries.append(struct.pack("<HII",tag,len(data),len(body)))
 body+=data
dir_offset=len(body)
body+=struct.pack("<H",len(entries))+b"".join(entries)+struct.pack("<I",dir_offset)
header=b"II"+struct.pack("<I",26)+b"HEAPCCDR"+bytes(12)
(root/"iso.crw").write_bytes(header+body)
(root/"iso-manifest.json").write_text(json.dumps([str(root/"iso.crw")]))
