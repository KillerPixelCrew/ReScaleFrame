"""Attribute code in a dumped module to engine source files.

Unreal's check and ensure macros embed __FILE__, so a shipped binary keeps the source path of
every file containing one, and the code referencing that path is the code from that file. This
turns an unnamed function list into a map of which engine source file each region came from,
without an engine build or any symbol matching.

A reference proves the path string is used near that address. It does not prove the function was
compiled from that file: inlining moves checks across file boundaries. Treat it as attribution,
not as ground truth.
"""

import argparse
import json
import re
import struct
from collections import defaultdict
from pathlib import Path


def read_sections(data):
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    count = struct.unpack_from("<H", data, pe + 6)[0]
    optional_size = struct.unpack_from("<H", data, pe + 20)[0]
    optional = pe + 24
    image_base = struct.unpack_from("<Q", data, optional + 24)[0]
    sections = []
    for index in range(count):
        position = optional + optional_size + index * 40
        name = data[position:position + 8].split(b"\0")[0].decode("ascii", "replace")
        virtual_size, rva, raw_size, raw_offset = struct.unpack_from("<IIII", data, position + 8)
        sections.append({"name": name, "rva": rva, "virtual_size": virtual_size,
                         "raw_offset": raw_offset, "raw_size": raw_size})
    return image_base, sections


def find_source_paths(data, pattern):
    """Locate every embedded source path and the RVA it lives at."""
    expression = re.compile(pattern.encode("ascii"))
    paths = {}
    for match in expression.finditer(data):
        start = match.start()
        # Walk back to the start of the C string, then forward to its terminator.
        begin = start
        while begin > 0 and 0x20 <= data[begin - 1] < 0x7f:
            begin -= 1
        end = data.index(b"\0", start)
        text = data[begin:end].decode("ascii", "replace")
        if len(text) > 8:
            paths[begin] = text  # raw offset equals RVA in a ReScaleFrame module dump
    return paths


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dump", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--pattern", default=r"Engine[\\/]Source[\\/]",
                        help="regex identifying an engine source path")
    parser.add_argument("--top", type=int, default=25, help="how many files to summarise")
    args = parser.parse_args()

    data = args.dump.read_bytes()
    image_base, sections = read_sections(data)
    paths = find_source_paths(data, args.pattern)
    if not paths:
        raise SystemExit("no source paths matched")

    code = [entry for entry in sections if entry["name"] == ".text"]
    if not code:
        raise SystemExit("no .text section")

    # One pass over the code: at every offset, read a displacement and see whether it lands on a
    # known path string. Doing it per string instead would mean one pass per file.
    references = defaultdict(list)
    for section in code:
        size = max(section["raw_size"], section["virtual_size"])
        block = data[section["raw_offset"]:section["raw_offset"] + size]
        base_rva = section["rva"]
        unpack = struct.unpack_from
        for position in range(len(block) - 4):
            displacement = unpack("<i", block, position)[0]
            if displacement == 0:
                continue
            target = base_rva + position + 4 + displacement
            path = paths.get(target)
            if path is not None:
                references[path].append(base_rva + position - 3)

    summary = sorted(((len(sites), path) for path, sites in references.items()), reverse=True)
    report = {
        "image_base": hex(image_base),
        "source_paths_found": len(paths),
        "source_paths_referenced": len(references),
        "files": {path: [hex(image_base + site) for site in sorted(set(sites))]
                  for path, sites in references.items()},
        "limits": ("A reference means the path string is used near that address, which is where a "
                   "check or ensure macro sits. Inlining can move a check into another file's "
                   "code, so this is attribution rather than proof of origin."),
    }
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    print(f"{len(paths)} source paths embedded, {len(references)} referenced from code")
    for count, path in summary[:args.top]:
        print(f"  {count:>5}  {path}")


if __name__ == "__main__":
    main()
