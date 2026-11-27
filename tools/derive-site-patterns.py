#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Derive unique byte patterns for an adapter's code sites and globals.

Reads a PE32+ executable (or a decrypted image dump whose RVAs equal file offsets) read-only and,
for every site in a spec file, prints the shortest pattern found that matches exactly once in the
executable sections and resolves back to the site's recorded RVA:

  code NAME RVA [LEN]  pattern starts at the site and covers at least LEN bytes of it; rel32
                       branches and RIP displacements are wildcards. When no pattern starting at
                       the site is unique, it starts at an earlier instruction (an interior site)
  data NAME RVA        pattern starts at an instruction whose RIP-relative operand addresses the
                       global (or, for a function, at a rel32 call or jump to it); the operand is
                       decoded

Output is one line per site: NAME KIND PATTERN [disp NEXT] -> RVA, the kind being `at` (the site
is the pattern start), `at+N` (the site is N bytes into the pattern) or `rip` (the site is the
decoded RIP-relative target). Usage:

  py -3 tools/derive-site-patterns.py IMAGE SPEC [--mapped] [--also IMAGE ...]

With `--also`, every spec line carries one RVA per image (IMAGE first, then LEN) and each pattern must resolve
uniquely to its RVA in every image: one site description for several builds. Bytes that differ
between the builds become wildcards, so the builds must encode each instruction with the same
length; a function whose own entry diverges is found through a common call to it instead, and its
common entry prefix is printed as `check`.

Requires capstone and numpy. The images are opened read-only.
"""

import argparse
import itertools
import re
import struct
import sys

import capstone
import numpy as np

MAX_BYTES = 96
# At least this many fixed bytes before uniqueness is tested, so a pattern carries real evidence.
MIN_FIXED = 10
# A code-site pattern also covers what a hook overwrites and what the old entry guards checked.
MIN_CODE_LENGTH = 16
REFERENCES = 8


class Image:
    def __init__(self, path, mapped):
        with open(path, 'rb') as handle:
            raw = handle.read()
        nt = struct.unpack_from('<I', raw, 0x3c)[0]
        count = struct.unpack_from('<H', raw, nt + 6)[0]
        optional = struct.unpack_from('<H', raw, nt + 20)[0]
        size = struct.unpack_from('<I', raw, nt + 24 + 56)[0]
        table = nt + 24 + optional
        sections = []
        for index in range(count):
            entry = table + 40 * index
            vsize, va, rsize, roffset = struct.unpack_from('<IIII', raw, entry + 8)
            flags = struct.unpack_from('<I', raw, entry + 36)[0]
            sections.append((va, vsize, roffset, rsize, flags))
        if mapped:
            self.bytes = raw[:size].ljust(size, b'\0')
        else:
            image = bytearray(size)
            image[:sections[0][2] if sections else 0x400] = raw[:sections[0][2] if sections else 0x400]
            for va, vsize, roffset, rsize, _ in sections:
                length = min(rsize, vsize if vsize else rsize)
                image[va:va + length] = raw[roffset:roffset + length]
            self.bytes = bytes(image)
        self.code = [(va, va + max(vsize, 1)) for va, vsize, _, _, flags in sections
                     if flags & 0x20000000]
        # Function starts from the exception directory (data directory 3), for true instruction
        # boundaries before an interior site.
        pdata, pdata_size = struct.unpack_from('<II', self.bytes, nt + 24 + 112 + 3 * 8)
        self.functions = sorted(
            struct.unpack_from('<II', self.bytes, entry)
            for entry in range(pdata, pdata + pdata_size - 11, 12))

    def function_start(self, rva):
        """The start of the function (or unwind chunk) containing rva, or None."""
        import bisect
        index = bisect.bisect_right(self.functions, (rva, 1 << 32)) - 1
        if index >= 0 and self.functions[index][0] <= rva < self.functions[index][1]:
            return self.functions[index][0]
        return None

    def matches(self, regex, limit=3):
        found = []
        for start, end in self.code:
            for hit in regex.finditer(self.bytes, start, end):
                found.append(hit.start())
                if len(found) >= limit:
                    return found
        return found


def regex_of(pattern):
    parts = []
    for byte in pattern:
        parts.append(b'.' if byte is None else re.escape(bytes([byte])))
    return re.compile(b'(?=' + b''.join(parts) + b')', re.DOTALL)


def text(pattern):
    return ' '.join('??' if byte is None else f'{byte:02X}' for byte in pattern)


DISASM = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
DISASM.detail = True


def masked(instruction):
    """Instruction bytes with rel32 branch targets and RIP displacements wildcarded."""
    raw = list(instruction.bytes)
    out = list(raw)
    groups = set(instruction.groups)
    relative = groups & {capstone.CS_GRP_JUMP, capstone.CS_GRP_CALL}
    if relative and instruction.imm_size == 4:
        for index in range(instruction.imm_offset, instruction.imm_offset + 4):
            out[index] = None
    for operand in instruction.operands:
        if operand.type == capstone.x86.X86_OP_MEM and operand.mem.base == capstone.x86.X86_REG_RIP:
            for index in range(instruction.disp_offset, instruction.disp_offset + instruction.disp_size):
                out[index] = None
    return out


def same(image, at, pattern):
    window = image.bytes[at:at + len(pattern)]
    return len(window) == len(pattern) and all(
        byte is None or byte == window[index] for index, byte in enumerate(pattern))


def common(images, starts, max_bytes=MAX_BYTES):
    """Masked instructions from starts, merged across the images, while their lengths agree."""
    streams = [DISASM.disasm(image.bytes[start:start + max_bytes + 16], start)
               for image, start in zip(images, starts)]
    for instructions in zip(*streams):
        parts = [masked(instruction) for instruction in instructions]
        if any(len(part) != len(parts[0]) for part in parts):
            return
        yield [byte if all(other[index] == byte for other in parts) else None
               for index, byte in enumerate(parts[0])]


def grow(images, starts, min_length, max_bytes=MAX_BYTES):
    """Extend a pattern instruction by instruction from starts until it is unique in every image.

    The candidate set of a longer pattern is a subset of its prefix's, so each image is searched
    once and later extensions only recheck the remaining candidates."""
    pattern = []
    candidates = None
    for part in common(images, starts, max_bytes):
        pattern += part
        if len(pattern) > max_bytes:
            return None
        if sum(byte is not None for byte in pattern) < MIN_FIXED:
            continue
        if candidates is None:
            candidates = [image.matches(regex_of(pattern), limit=1 << 30) for image in images]
        else:
            candidates = [[at for at in found if same(image, at, pattern)]
                          for image, found in zip(images, candidates)]
        if all(found == [start] for found, start in zip(candidates, starts)) \
                and len(pattern) >= min_length:
            return pattern
        if any(start not in found for found, start in zip(candidates, starts)):
            return None
    return None


def entry(images, starts, length=MIN_CODE_LENGTH):
    """The common masked entry of a function, at least `length` bytes when the builds agree."""
    pattern = []
    for part in common(images, starts):
        pattern += part
        if len(pattern) >= length:
            break
    return pattern


def references(image, target):
    """Instruction starts whose RIP-relative operand or rel32 branch resolves to target."""
    found = []
    data = np.frombuffer(image.bytes, dtype=np.uint8)
    for start, end in image.code:
        for phase in range(4):
            first = start + phase
            count = (end - first) // 4
            if count <= 0:
                continue
            words = np.frombuffer(image.bytes, dtype='<i4', count=count, offset=first)
            positions = first + 4 * np.arange(count, dtype=np.int64)
            for tail in (0, 1, 2, 4):
                hits = np.nonzero(positions + 4 + tail + words.astype(np.int64) == target)[0]
                found.extend(int(positions[hit]) for hit in hits)
    del data
    result = []
    for position in sorted(set(found)):
        # Longest decode first, so REX and operand-size prefixes stay part of the instruction.
        for back in range(7, 0, -1):
            begin = position - back
            decoded = list(DISASM.disasm(image.bytes[begin:begin + 16], begin, 1))
            if not decoded:
                continue
            instruction = decoded[0]
            if instruction.size <= back:
                continue
            groups = set(instruction.groups)
            relative = groups & {capstone.CS_GRP_JUMP, capstone.CS_GRP_CALL}
            if relative and instruction.imm_size == 4 and instruction.imm_offset == back:
                if instruction.address + instruction.size + struct.unpack_from(
                        '<i', image.bytes, position)[0] == target:
                    result.append((begin, back, instruction.size))
                    break
            if instruction.disp_offset == back and instruction.disp_size == 4 and any(
                    operand.type == capstone.x86.X86_OP_MEM
                    and operand.mem.base == capstone.x86.X86_REG_RIP
                    for operand in instruction.operands):
                if instruction.address + instruction.size + instruction.disp == target:
                    result.append((begin, back, instruction.size))
                    break
    return result


def instruction_starts_before(image, rva, reach=64):
    """Instruction starts within reach before rva, nearest first, decoded from the start of the
    function containing rva so that every start is a real instruction boundary."""
    start = image.function_start(rva)
    if start is None or start == rva:
        return []
    starts, end = [], start
    for instruction in DISASM.disasm(image.bytes[start:rva], start):
        starts.append(instruction.address)
        end = instruction.address + instruction.size
    if end != rva:
        return []
    return [begin for begin in reversed(starts) if rva - begin <= reach]


def derive(images, kind, rvas, length=0):
    if kind == 'code':
        cover = max(MIN_CODE_LENGTH, length)
        limit = max(MAX_BYTES, cover + 32)
        pattern = grow(images, rvas, cover, limit)
        if pattern is not None:
            return ('at', pattern, None)
        # Then a pattern that starts at an earlier instruction of the same function (the same
        # distance before the site in every image), and only then a longer one from the site,
        # which may run into the next function.
        before = [instruction_starts_before(image, rva) for image, rva in zip(images, rvas)]
        for begin in before[0]:
            delta = rvas[0] - begin
            starts = [rva - delta for rva in rvas]
            if not all(start in found for start, found in zip(starts, before)):
                continue
            pattern = grow(images, starts, delta + cover, limit + delta)
            if pattern is not None:
                return (f'at+{delta}', pattern, None)
        pattern = grow(images, rvas, cover, 2 * limit)
        if pattern is not None:
            return ('at', pattern, None)
    best = None
    # A few references are enough to choose a short unique one; heavily used globals have hundreds.
    # Across builds, only references with the same operand position can share a pattern.
    found = [references(image, rva)[:REFERENCES] for image, rva in zip(images, rvas)]
    for first in found[0]:
        for others in itertools.product(*[[ref for ref in refs if ref[1:] == first[1:]]
                                          for refs in found[1:]]):
            pattern = grow(images, [first[0]] + [ref[0] for ref in others], 0)
            if pattern is not None and (best is None or len(pattern) < len(best[1])):
                best = ('rip', pattern, first)
    return best


def resolve(image, kind, pattern, operand):
    hits = image.matches(regex_of(pattern))
    if len(hits) != 1:
        return None
    if kind.startswith('at'):
        return hits[0] + int(kind[3:] or 0)
    _, disp, size = operand
    return hits[0] + size + struct.unpack_from('<i', image.bytes, hits[0] + disp)[0]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('image')
    parser.add_argument('spec')
    parser.add_argument('--mapped', action='store_true', help='RVAs equal file offsets (dump)')
    parser.add_argument('--references', type=int, default=REFERENCES,
                        help='references tried per data site')
    parser.add_argument('--also', nargs='+', default=[], metavar='IMAGE',
                        help='further builds; spec lines then carry one RVA per image')
    arguments = parser.parse_args()
    globals()['REFERENCES'] = arguments.references
    images = [Image(path, arguments.mapped) for path in [arguments.image] + arguments.also]
    failed = 0
    with open(arguments.spec, encoding='utf-8') as spec:
        for line in spec:
            line = line.split('#', 1)[0].strip()
            if not line:
                continue
            kind, name, *values = line.split()
            # One RVA per image, then the optional covered length of a code site.
            rvas = [int(value, 16) for value in values[:len(images)]]
            length = int(values[len(images)]) if len(values) > len(images) else 0
            assert len(rvas) == len(images), f'{name}: one RVA per image'
            shown = '/'.join(f'{rva:#x}' for rva in rvas)
            result = derive(images, kind, rvas, length)
            if result is None:
                print(f'{name} {kind} NOT-FOUND -> {shown}')
                failed += 1
                continue
            found, pattern, operand = result
            resolved = [resolve(image, found, pattern, operand) for image in images]
            suffix = '' if operand is None else f' disp={operand[1]} next={operand[2]}'
            if kind == 'code' and found == 'rip':
                suffix += f' check="{text(entry(images, rvas))}"'
            status = 'ok' if resolved == rvas else f'MISMATCH {resolved}'
            print(f'{name} {found} "{text(pattern)}"{suffix} -> {shown} {status}')
            sys.stdout.flush()
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
