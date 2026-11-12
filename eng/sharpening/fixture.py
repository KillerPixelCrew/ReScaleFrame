# SPDX-License-Identifier: GPL-3.0-only
# RCAS math derived from OptiScaler 2fff542 and AMD FidelityFX (MIT).
# See runtime/graphics/src/sharpening/THIRD_PARTY.md.
"""Compare OptiScaler's linear HDR RCAS with perceptually encoded RCAS.

Writes a CSV of every sample and a PPM with original/linear/encoded rows.
This is an analytic fixture, not a GPU or game acceptance test.
"""
import argparse
import csv
import zlib
import struct
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('--output', type=Path, default=Path('.local/sharpening/fixture'))
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
# Two geometrically identical ramps and edges, a thousand times apart in radiance.
signals = [[scale*v for v in [0.1,0.1,0.11,0.12,0.13,0.16,0.18,0.19,0.2,0.2,0.2]] for scale in (1,500)]

def encode(x):
    return (x/(x+4))**(1/2.2)

def decode(x):
    t = min(x,0.999999)**2.2
    return 4*t/(1-t)

def rcas(values, encoded):
    result = []
    for i,e in enumerate(values):
        d,f = values[max(i-1,0)],values[min(i+1,len(values)-1)]
        taps = [e,d,f,e]
        local_scale = 1 if encoded else max(1,e,*taps)
        source = [encode(v) if encoded else v/local_scale for v in taps]
        center = encode(e) if encoded else e/local_scale
        lo,hi = min(source),max(source)
        hit_min = lo/max(4*hi,1e-5)
        hit_max = (1-hi)/min(4*lo-4,-1e-5)
        lobe = max(-0.1875,min(max(-hit_min,hit_max),0))*0.8
        out = (sum(source)*lobe+center)/(4*lobe+1)
        if encoded:
            # Same local HDR limiter used by the port's storeColor.
            out = decode(min(max(out,min(source+[center])),max(source+[center])))
        else:
            out *= local_scale
        result.append(out)
    return result

rows=[]
with (args.output/'numbers.csv').open('w',newline='') as file:
    writer=csv.writer(file); writer.writerow(['edge','sample','original','linear','encoded'])
    for name,source in zip(('dark','bright'),signals):
        linear,encoded=rcas(source,False),rcas(source,True)
        for i,(a,b,c) in enumerate(zip(source,linear,encoded)):
            writer.writerow([name,i,a,b,c])
        print(f'{name}: center {source[5]:.9g}, linear {linear[5]:.9g}, encoded {encoded[5]:.9g}; '
              f'delta/center {(linear[5]-source[5])/source[5]:.6g} vs {(encoded[5]-source[5])/source[5]:.6g}')
        for signal in (source,linear,encoded):
            # Match display exposure per scene; use the same exposure in each comparison.
            exposure = 1 if name=='dark' else 1/500
            row=[]
            for value in signal:
                display = (value*exposure/(value*exposure+0.18))**(1/2.2)
                row += [max(0,min(255,round(display*255)))]*3*32
            rows.extend([bytes(row)]*32)
width=len(signals[0])*32
(args.output/'comparison.ppm').write_bytes(f'P6\n{width} {len(rows)}\n255\n'.encode()+b''.join(rows))

# PNG has the same pixels as the PPM and can be previewed without an image toolkit.
def chunk(kind, data):
    return struct.pack('>I',len(data))+kind+data+struct.pack('>I',zlib.crc32(kind+data))
header = struct.pack('>IIBBBBB',width,len(rows),8,2,0,0,0)
pixels = zlib.compress(b''.join(b'\x00'+row for row in rows))
(args.output/'comparison.png').write_bytes(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',header)+chunk(b'IDAT',pixels)+chunk(b'IEND',b''))
