#!/usr/bin/env python3
# Compare exiftool-rs vs Perl ExifTool via -G1 -json. Reports name/value/group
# deltas as ordered (key,value) multisets. Perl is the reference.
import json, subprocess, sys, os
PERL_DIR = "/home/sylvain/dev/exiftool"
RS = "/home/sylvain/.cache/claude-work/exr/target/release/exiftool-rs"
VOL = {"Directory","ExifToolVersion","FileAccessDate","FileInodeChangeDate",
       "FileModifyDate","FileName","FilePermissions","FileSize"}
def run(cmd, cwd=None):
    out = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True).stdout
    try: d = json.loads(out)
    except Exception: return {}
    return d[0] if d else {}
def norm(obj):
    r={}
    for k,v in obj.items():
        if k=="SourceFile": continue
        name=k.split(":")[-1]
        if name in VOL: continue
        r[k]=str(v)
    return r
def cmp_file(path, ee):
    f=(["-ee"] if ee else [])
    p=norm(run(["perl","exiftool"]+f+["-json",path], cwd=PERL_DIR))
    r=norm(run([RS]+f+["-json",path]))
    miss=[(k,p[k]) for k in p if k not in r or r[k]!=p[k]]
    extra=[(k,r[k]) for k in r if k not in p]
    return miss,extra
if __name__=="__main__":
    imgs=os.path.abspath(sys.argv[1] if len(sys.argv)>1 else "/home/sylvain/.cache/claude-work/exr/tests/images")
    files=sorted(os.path.join(imgs,f) for f in os.listdir(imgs))
    tm=te=0
    for path in files:
        for ee in (False,True):
            miss,extra=cmp_file(path,ee)
            n=len(miss)+len(extra)
            if n: 
                (te:=te+n) if ee else (tm:=tm+n)
                if len(sys.argv)>2: print(f"{'ee ' if ee else 'def'} {os.path.basename(path):24} miss={len(miss)} extra={len(extra)}")
    print(f"TOTAL  defaut={tm}  ee={te}")
