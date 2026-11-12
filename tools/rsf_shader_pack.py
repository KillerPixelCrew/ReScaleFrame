#!/usr/bin/env python3
"""Build or inspect raw RSFPATCH v1 archives. Never reads a game installation."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
HEADER=struct.Struct('<8sIIQ')
SECTION=struct.Struct('<IIQQ')
def build(source):
    source=Path(source)
    metadata=(source/'metadata.json').read_bytes();rules=(source/'rules.json').read_bytes()
    meta=json.loads(metadata);json.loads(rules)
    payload=bytearray();files={p.relative_to(source/'files').as_posix():p for p in (source/'files').rglob('*') if p.is_file()}
    for name,path in meta.get('payload',{}).items():
        if name in files:raise ValueError('Duplicate payload name: '+name)
        files[name]=source/path
    payload.extend(struct.pack('<I',len(files)))
    for name,path in sorted(files.items()):
        name=name.encode();data=path.read_bytes()
        payload.extend(struct.pack('<IQ',len(name),len(data)));payload.extend(name);payload.extend(data)
    parts=[metadata,rules,bytes(payload)];offset=HEADER.size+3*SECTION.size
    out=bytearray(HEADER.pack(b'RSFPATCH',1,3,HEADER.size))
    for kind,data in enumerate(parts,1):out.extend(SECTION.pack(kind,0,offset,len(data)));offset+=len(data)
    for data in parts:out.extend(data)
    return bytes(out)
def inspect(data):
    if len(data)<HEADER.size:raise ValueError('Truncated header')
    magic,version,count,table=HEADER.unpack_from(data)
    if magic!=b'RSFPATCH' or version!=1:raise ValueError('Pack version')
    if table<HEADER.size or count>(len(data)-table)//SECTION.size:raise ValueError('Section table')
    spans=[(0,HEADER.size),(table,table+count*SECTION.size)];sections={}
    for i in range(count):
        kind,reserved,start,size=SECTION.unpack_from(data,table+i*SECTION.size)
        if reserved or start>len(data) or size>len(data)-start or kind in sections:raise ValueError('Section range')
        spans.append((start,start+size));sections[kind]=data[start:start+size]
    spans.sort()
    if any(b[0]<a[1] for a,b in zip(spans,spans[1:])):raise ValueError('Overlapping sections')
    if not {1,2,3}<=sections.keys():raise ValueError('Missing sections')
    return {'metadata':json.loads(sections[1]),'sha256':hashlib.sha256(data).hexdigest(),'bytes':len(data),'rules':len(json.loads(sections[2])['files'])}
def main():
    parser=argparse.ArgumentParser(description=__doc__);sub=parser.add_subparsers(dest='command',required=True)
    b=sub.add_parser('build');b.add_argument('source',type=Path);b.add_argument('output',type=Path)
    i=sub.add_parser('inspect');i.add_argument('pack',type=Path)
    args=parser.parse_args()
    if args.command=='build':
        data=build(args.source);report=inspect(data);args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_bytes(data)
        if args.output.read_bytes()!=data:raise IOError('Pack readback')
    else:report=inspect(args.pack.read_bytes())
    print(json.dumps(report,indent=2))
if __name__=='__main__':main()
