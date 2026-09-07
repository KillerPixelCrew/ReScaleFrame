"""Recover Unreal native function names and addresses from a dumped module.

Unreal's generated code registers every native (C++ backed) reflected function through a static
table in the module's own data:

    static const FNameNativePtrPair Funcs[] = {
        { "WasRecentlyRendered", &AActor::execWasRecentlyRendered },
        ...
    };
    FNativeFunctionRegistrar::RegisterFunctions(Class, Funcs, ARRAY_COUNT(Funcs));

On x64 each entry is sixteen bytes: a pointer to an ANSI name followed by a pointer to the exec
thunk. The tables survive into the shipped binary because the engine needs them at startup, so a
decrypted dump carries a direct name-to-address mapping for thousands of functions.

The names alone are ambiguous, because the table stores the bare function name and several classes
declare, for example, `GetOwner`. Each table holds exactly one class's native functions, so this
tool matches a whole table against the per-class native sets from a reflection SDK dump
(see parse-ue-sdk.py) and takes the class that accounts for the table.

A match names the exec thunk. The thunk is a generated argument unpacker, not the engine function
itself; the real member function is the call it makes after unpacking, which this tool does not
resolve because it does not disassemble. Treat the output as verified names for the thunks and as
leads one call deep.
"""

import argparse
import json
import struct
from collections import defaultdict
from pathlib import Path

IMAGE_SCN_CNT_CODE = 0x00000020
IMAGE_SCN_MEM_EXECUTE = 0x20000000

ENTRY_SIZE = 16
MAX_NAME = 128


def read_headers(data):
    """Return the image base, the image size and the section table."""
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    section_count = struct.unpack_from("<H", data, pe + 6)[0]
    optional_size = struct.unpack_from("<H", data, pe + 20)[0]
    optional = pe + 24
    magic = struct.unpack_from("<H", data, optional)[0]
    if magic != 0x20B:
        raise SystemExit("only PE32+ (x64) modules are supported")
    image_base = struct.unpack_from("<Q", data, optional + 24)[0]
    image_size = struct.unpack_from("<I", data, optional + 56)[0]

    sections = []
    for index in range(section_count):
        position = optional + optional_size + index * 40
        name = data[position:position + 8].split(b"\0")[0].decode("ascii", "replace")
        virtual_size, rva, raw_size, raw_offset = struct.unpack_from("<IIII", data, position + 8)
        characteristics = struct.unpack_from("<I", data, position + 36)[0]
        sections.append({
            "name": name,
            "rva": rva,
            "virtual_size": virtual_size,
            "raw_offset": raw_offset,
            "raw_size": raw_size,
            "characteristics": characteristics,
            "executable": bool(characteristics & (IMAGE_SCN_MEM_EXECUTE | IMAGE_SCN_CNT_CODE)),
        })
    return image_base, image_size, sections


class Image:
    """Address translation over the dumped sections."""

    def __init__(self, data, image_base, sections):
        self.data = data
        self.image_base = image_base
        self.sections = sections
        self.code_ranges = [(image_base + s["rva"], image_base + s["rva"] + s["virtual_size"])
                            for s in sections if s["executable"]]

    def offset_of(self, va):
        rva = va - self.image_base
        if rva < 0:
            return None
        for section in self.sections:
            if section["rva"] <= rva < section["rva"] + section["virtual_size"]:
                offset = section["raw_offset"] + (rva - section["rva"])
                if offset < section["raw_offset"] + section["raw_size"]:
                    return offset
                return None
        return None

    def is_code(self, va):
        return any(start <= va < end for start, end in self.code_ranges)

    def ansi_string(self, va):
        """Return a plausible C identifier at va, or None."""
        offset = self.offset_of(va)
        if offset is None:
            return None
        end = self.data.find(b"\0", offset, offset + MAX_NAME + 1)
        if end < 0 or end == offset:
            return None
        raw = self.data[offset:end]
        if not (raw[0:1].isalpha() or raw[0:1] == b"_"):
            return None
        for byte in raw:
            if not (byte == 0x5F or 0x30 <= byte <= 0x39 or 0x41 <= byte <= 0x5A
                    or 0x61 <= byte <= 0x7A):
                return None
        return raw.decode("ascii")


def scan_entries(image):
    """Find every eight-byte aligned position that looks like an FNameNativePtrPair."""
    entries = {}
    for section in image.sections:
        if section["executable"] or section["raw_size"] < ENTRY_SIZE:
            continue
        base = section["raw_offset"]
        size = min(section["raw_size"], section["virtual_size"])
        window = image.data[base:base + size]
        words = [value for (value,) in struct.iter_unpack("<Q", window[:len(window) // 8 * 8])]
        for index in range(len(words) - 1):
            name_pointer = words[index]
            code_pointer = words[index + 1]
            if not image.is_code(code_pointer):
                continue
            if name_pointer < image.image_base:
                continue
            name = image.ansi_string(name_pointer)
            if name is None:
                continue
            va = image.image_base + section["rva"] + index * 8
            entries[va] = {"name": name, "target": code_pointer}
    return entries


def group_runs(entries):
    """Chain entries that sit at a sixteen byte stride into candidate tables."""
    runs = []
    remaining = dict(entries)
    for va in sorted(entries):
        if va not in remaining:
            continue
        # Only start a run where the previous slot is not itself part of one.
        if va - ENTRY_SIZE in remaining:
            continue
        run = []
        cursor = va
        while cursor in remaining:
            run.append({"address": cursor, **remaining.pop(cursor)})
            cursor += ENTRY_SIZE
        runs.append(run)
    return runs


def match_runs(runs, native_by_class, minimum):
    """Attribute each run to the class whose native function set accounts for it."""
    owners = defaultdict(list)
    for cls, names in native_by_class.items():
        for name in names:
            owners[name].append(cls)

    matched, unmatched = [], []
    for run in runs:
        names = [entry["name"] for entry in run]
        counts = defaultdict(int)
        for name in names:
            for cls in owners.get(name, ()):
                counts[cls] += 1
        if not counts:
            unmatched.append({"address": run[0]["address"], "size": len(run), "names": names[:8]})
            continue

        best = max(counts, key=lambda cls: (counts[cls], -len(native_by_class[cls])))
        covered = counts[best]
        # A table is one class's functions. Accepting a partial cover would attribute another
        # class's entries to this one, so require the whole table to be accounted for.
        if covered < len(names) or covered < minimum:
            unmatched.append({"address": run[0]["address"], "size": len(run),
                              "names": names[:8], "best": best, "covered": covered})
            continue

        declared = len(native_by_class[best])
        for entry in run:
            matched.append({
                "address": f"{entry['target']:x}",
                "table_entry": f"{entry['address']:x}",
                "symbol": f"{best}::exec{entry['name']}",
                "class": best,
                "function": entry["name"],
                "table_size": len(run),
                "class_native_count": declared,
                "exact": len(names) == declared,
            })
    return matched, unmatched


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("dump", type=Path, help="decrypted module dump")
    parser.add_argument("index", type=Path, help="SDK index from parse-ue-sdk.py")
    parser.add_argument("output", type=Path, help="JSON name map to write")
    parser.add_argument("--minimum", type=int, default=1,
                        help="smallest table size to accept a match for")
    arguments = parser.parse_args()

    data = arguments.dump.read_bytes()
    image_base, image_size, sections = read_headers(data)
    image = Image(data, image_base, sections)

    print(f"image base 0x{image_base:x}, size 0x{image_size:x}, {len(sections)} sections")
    for section in sections:
        kind = "code" if section["executable"] else "data"
        print(f"  {section['name']:<10} {kind}  rva 0x{section['rva']:08x}"
              f"  raw 0x{section['raw_offset']:08x}  size 0x{section['virtual_size']:x}")

    index = json.loads(arguments.index.read_text(encoding="utf-8"))
    native_by_class = {cls: bucket["native"]
                       for cls, bucket in index["functions_by_class"].items() if bucket["native"]}

    entries = scan_entries(image)
    runs = group_runs(entries)
    matched, unmatched = match_runs(runs, native_by_class, arguments.minimum)

    named = {}
    conflicts = 0
    for record in matched:
        existing = named.get(record["address"])
        if existing and existing["symbol"] != record["symbol"]:
            conflicts += 1
            continue
        named[record["address"]] = record

    arguments.output.write_text(json.dumps({
        "dump": str(arguments.dump),
        "image_base": f"{image_base:x}",
        "index": str(arguments.index),
        "functions": sorted(named.values(), key=lambda r: r["address"]),
        "unmatched_tables": sorted(unmatched, key=lambda r: -r["size"])[:200],
    }, indent=1), encoding="utf-8")

    classes = {record["class"] for record in named.values()}
    exact = sum(1 for record in named.values() if record["exact"])
    print()
    print(f"candidate entries  {len(entries)}")
    print(f"candidate tables   {len(runs)}")
    print(f"matched entries    {len(matched)} in {len(classes)} classes")
    print(f"  whole-class      {exact}")
    print(f"  distinct targets {len(named)}, address conflicts {conflicts}")
    print(f"unmatched tables   {len(unmatched)}")
    print(f"written            {arguments.output}")


if __name__ == "__main__":
    main()
