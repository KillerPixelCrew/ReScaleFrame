#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Recover an imported program's original file from a saved Ghidra database, read-only.

When a game is no longer installed, the executable a Ghidra project was made from is still inside
the project: the import keeps the whole file as `FileBytes`, one chained buffer of the file's size.
This reads a program database file (`<project>.rep/idata/NN/~XXXXXXXX.db/db.N.gbf`) without Ghidra:

  LocalBufferFile   block 0 is the header (block size: big-endian int at +20); every other block is
                    flags (1, bit 0 = empty), buffer id (4, big-endian), then the buffer.
  ChainedBuffer     index node: type 8, length (4, MSB = obfuscated), next index id (4), data ids
                    (4 each); data node: type 9, then data. Obfuscated data is XOR'd with Ghidra's
                    128-byte mask, phased by the offset within each data node.

Every index root whose length equals SIZE is tried; the first fully allocated one is written to
OUTPUT and its SHA-256 printed, so it can be compared with the researched build's. Usage:

  py -3 -I tools/ghidra/file-bytes.py DB_GBF SIZE OUTPUT
"""

import hashlib
import struct
import sys

# ghidra.framework/DB ChainedBuffer.XOR_MASK_BYTES.
MASK = bytes.fromhex(
    '59ea6723dab800b8c348dd8b21d69478 35ab2b7eb24f824e0e16c457128e7ee6'
    'b6bd56915772e691dc522ef21ab7d66f dadee848b1bb506ff4dd11eef267fe48'
    '8dae691ae0268c248e177651e260d7e6 8365d5f07ff2a0d64bbd24d8abea9ea6'
    '48943e7b2cf4cedc6911f83ca73f5d77 943fe48e4820db5632c187012ee37f40')
INDEX_NODE, DATA_NODE = 8, 9


def recover(path, size, output):
    with open(path, 'rb') as database:
        block = struct.unpack('>i', database.read(32)[20:24])[0]
        space = block - 5
        database.seek(0, 2)
        blocks = database.tell() // block
        where, roots = {}, []
        for index in range(1, blocks):
            database.seek(index * block)
            prefix = database.read(10)
            if prefix[0] & 1:
                continue
            where[struct.unpack('>i', prefix[1:5])[0]] = index * block + 5
            if prefix[5] == INDEX_NODE and struct.unpack('>i', prefix[6:10])[0] & 0x7fffffff == size:
                roots.append(struct.unpack('>i', prefix[1:5])[0])

        def buffer(ident, length=space):
            database.seek(where[ident])
            return database.read(length)

        per_node = space - 1
        nodes = -(-size // per_node)
        mask = MASK * (-(-per_node // len(MASK)))
        for root in roots:
            obfuscated = struct.unpack('>i', buffer(root)[1:5])[0] < 0
            ids, ident = [], root
            while ident != -1 and len(ids) < nodes:
                raw = buffer(ident)
                count = (space - 9) // 4
                ids += struct.unpack(f'>{count}i', raw[9:9 + count * 4])
                ident = struct.unpack('>i', raw[5:9])[0]
            ids = ids[:nodes]
            if len(ids) < nodes or any(ident not in where for ident in ids):
                print(f'root {root}: not fully allocated (a layered copy)')
                continue
            data = bytearray(size)
            for number, ident in enumerate(ids):
                start = number * per_node
                length = min(per_node, size - start)
                chunk = buffer(ident, length + 1)
                if chunk[0] != DATA_NODE:
                    raise SystemExit(f'root {root}: buffer {ident} is not a data node')
                chunk = chunk[1:]
                if obfuscated:
                    chunk = (int.from_bytes(chunk, 'little')
                             ^ int.from_bytes(mask[:length], 'little')).to_bytes(length, 'little')
                data[start:start + length] = chunk
            with open(output, 'xb') as out:
                out.write(data)
            print(f'root {root}: {size} bytes, sha256 {hashlib.sha256(data).hexdigest()}')
            return 0
    print(f'no complete chained buffer of {size} bytes')
    return 1


if __name__ == '__main__':
    if len(sys.argv) != 4:
        raise SystemExit(__doc__)
    sys.exit(recover(sys.argv[1], int(sys.argv[2]), sys.argv[3]))
