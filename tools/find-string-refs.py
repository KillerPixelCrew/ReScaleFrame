"""Count rip-relative references from code to given strings in a dumped module.

Answers whether the code that would use a string is still present. Raw offsets equal virtual
addresses in a ReScaleFrame module dump, so a file offset is an RVA.
"""

import argparse
import struct
from pathlib import Path


def sections(data):
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    count = struct.unpack_from("<H", data, pe + 6)[0]
    optional_size = struct.unpack_from("<H", data, pe + 20)[0]
    optional = pe + 24
    base = struct.unpack_from("<Q", data, optional + 24)[0]
    found = []
    for index in range(count):
        position = optional + optional_size + index * 40
        name = data[position:position + 8].split(b"\0")[0].decode("ascii", "replace")
        virtual_size, rva, raw_size, raw_offset = struct.unpack_from("<IIII", data, position + 8)
        found.append((name, rva, virtual_size, raw_offset, raw_size))
    return base, found


def occurrences(data, needle):
    found = []
    position = data.find(needle)
    while position >= 0:
        found.append(position)
        position = data.find(needle, position + 1)
    return found


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dump", type=Path)
    parser.add_argument("strings", nargs="+")
    parser.add_argument("--encoding", default="utf-16-le")
    args = parser.parse_args()

    data = args.dump.read_bytes()
    base, table = sections(data)
    code = [entry for entry in table if entry[0] in (".text",)]
    if not code:
        raise SystemExit("no .text section")

    for text in args.strings:
        needle = text.encode(args.encoding) + b"\0\0"
        targets = set(occurrences(data, needle))
        if not targets:
            print(f"{text:<12} string not present")
            continue
        references = []
        for _, rva, virtual_size, raw_offset, raw_size in code:
            block = data[raw_offset:raw_offset + max(raw_size, virtual_size)]
            for position in range(len(block) - 4):
                displacement = struct.unpack_from("<i", block, position)[0]
                if rva + position + 4 + displacement in targets:
                    references.append(rva + position - 3)
        listed = ", ".join(hex(base + value) for value in references[:6])
        print(f"{text:<12} {len(targets)} occurrence(s), {len(references)} code reference(s)"
              + (f": {listed}" if references else ""))


if __name__ == "__main__":
    main()
