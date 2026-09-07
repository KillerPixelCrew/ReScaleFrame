"""Recover Unreal reflected function names and addresses from a dumped module.

Unreal's generated code describes every reflected function twice, in two static arrays that survive
into a shipped binary because the engine reads them during startup registration:

    struct FNameNativePtrPair   { const char* NameUTF8; Native Pointer; };          // CoreNative.h
    struct FClassFunctionLinkInfo { UFunction* (*CreateFuncPtr)(); const char* FuncNameUTF8; };

`FNameNativePtrPair Funcs[]` holds the class's native functions and points at their exec thunks.
`FClassFunctionLinkInfo FuncInfo[]` holds every function of the class, native or not, and points at
its `Z_Construct_UFunction_<Class>_<Name>` reflection constructor. The two have their members in
opposite order, so on x64 both are sixteen byte entries alternating a name pointer and a code
pointer, out of phase with each other.

That matters, because scanning for one shape finds the other shifted by a word, which pairs every
name with the *next* entry's pointer and yields plausible-looking nonsense. This tool therefore
classifies each word of the data sections as a name pointer, a code pointer or neither, takes
maximal alternating spans, and reads the phase from where the span begins rather than assuming it.

Names alone are ambiguous, because the arrays store the bare function name and many classes declare
a `GetOwner`. Each array holds exactly one class, so a whole span is matched against the per-class
function sets of a reflection SDK dump (see parse-ue-sdk.py). Consecutive arrays of the same shape
run together; the engine emits each sorted case-insensitively by name, so a name that does not
advance marks the boundary between two of them.

A match names the exec thunk and the reflection constructor. The thunk is a generated argument
unpacker, not the engine function itself: the real member function is the call it makes after
unpacking, which this tool does not resolve because it does not disassemble.
"""

import argparse
import json
import struct
from collections import defaultdict
from pathlib import Path

IMAGE_SCN_CNT_CODE = 0x00000020
IMAGE_SCN_MEM_EXECUTE = 0x20000000

OTHER, NAME, CODE = 0, 1, 2
MAX_NAME = 128

# `FNameNativePtrPair` leads with the name, `FClassFunctionLinkInfo` with the constructor.
NATIVE_TABLE = "FNameNativePtrPair"
LINK_TABLE = "FClassFunctionLinkInfo"


def read_headers(data):
    """Return the image base, the image size and the section table."""
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    section_count = struct.unpack_from("<H", data, pe + 6)[0]
    optional_size = struct.unpack_from("<H", data, pe + 20)[0]
    optional = pe + 24
    if struct.unpack_from("<H", data, optional)[0] != 0x20B:
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
            "executable": bool(characteristics & (IMAGE_SCN_MEM_EXECUTE | IMAGE_SCN_CNT_CODE)),
        })
    return image_base, image_size, sections


class Image:
    """Address translation and pointer classification over the dumped sections."""

    def __init__(self, data, image_base, sections):
        self.data = data
        self.image_base = image_base
        self.sections = sections
        self.code_ranges = [(image_base + s["rva"], image_base + s["rva"] + s["virtual_size"])
                            for s in sections if s["executable"]]
        self.data_ranges = [(image_base + s["rva"], image_base + s["rva"] + s["virtual_size"])
                            for s in sections if not s["executable"]]

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

    def is_data(self, va):
        return any(start <= va < end for start, end in self.data_ranges)

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


def classify_section(image, section):
    """Classify every aligned word of one section as a name pointer, code pointer or neither."""
    base = section["raw_offset"]
    size = min(section["raw_size"], section["virtual_size"])
    window = image.data[base:base + size // 8 * 8]
    words = [value for (value,) in struct.iter_unpack("<Q", window)]

    kinds = bytearray(len(words))
    names = {}
    for index, value in enumerate(words):
        if image.is_code(value):
            kinds[index] = CODE
        elif image.is_data(value):
            text = image.ansi_string(value)
            if text is not None:
                kinds[index] = NAME
                names[index] = text
    return words, kinds, names


def alternating_spans(kinds, minimum_words):
    """Yield maximal spans whose words alternate between a name and a code pointer."""
    spans = []
    index = 0
    total = len(kinds)
    while index < total:
        if kinds[index] == OTHER:
            index += 1
            continue
        start = index
        index += 1
        while index < total and kinds[index] != OTHER and kinds[index] != kinds[index - 1]:
            index += 1
        if index - start >= minimum_words:
            spans.append((start, index - start))
    return spans


def split_sorted(entries):
    """Split one span where the name order restarts, which marks the next array."""
    segments = []
    current = []
    for entry in entries:
        if current and entry["name"].lower() <= current[-1]["name"].lower():
            segments.append(current)
            current = []
        current.append(entry)
    if current:
        segments.append(current)
    return segments


def read_span(image, section, words, kinds, names, start, length):
    """Turn one alternating span into entries, taking the phase from its first word."""
    shape = NATIVE_TABLE if kinds[start] == NAME else LINK_TABLE
    name_first = shape == NATIVE_TABLE
    entries = []
    for position in range(start, start + length - 1, 2):
        name_index = position if name_first else position + 1
        code_index = position + 1 if name_first else position
        if kinds[name_index] != NAME or kinds[code_index] != CODE:
            break
        entries.append({
            "address": image.image_base + section["rva"] + position * 8,
            "name": names[name_index],
            "target": words[code_index],
        })
    return shape, entries


def attribute(entries, sets, shape):
    """Split one span into per-class arrays.

    Sorted order marks most boundaries, but two arrays whose names happen to stay in order run
    together and one class's set will not account for the whole span. Each array is contiguous and
    holds a single class, so taking the class that explains the longest run from the current
    position recovers the boundary the ordering hid.
    """
    catalogue = sets["native"] if shape == NATIVE_TABLE else sets["all"]
    arrays = []
    position = 0
    while position < len(entries):
        best, best_length = None, 0
        for candidate in sets["owners"].get(entries[position]["name"], ()):
            members = catalogue.get(candidate)
            if not members:
                continue
            cursor = position
            while cursor < len(entries) and entries[cursor]["name"] in members:
                cursor += 1
            length = cursor - position
            if length > best_length or (length == best_length and best
                                        and len(members) < len(catalogue[best])):
                best, best_length = candidate, length
        if best is None:
            arrays.append((None, entries[position:position + 1]))
            position += 1
            continue
        arrays.append((best, entries[position:position + best_length]))
        position += best_length
    return arrays


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("dump", type=Path, help="decrypted module dump")
    parser.add_argument("index", type=Path, help="SDK index from parse-ue-sdk.py")
    parser.add_argument("output", type=Path, help="JSON name map to write")
    parser.add_argument("--minimum", type=int, default=2,
                        help="smallest array size to accept a match for")
    arguments = parser.parse_args()

    data = arguments.dump.read_bytes()
    image_base, image_size, sections = read_headers(data)
    image = Image(data, image_base, sections)

    print(f"image base 0x{image_base:x}, size 0x{image_size:x}, {len(sections)} sections")

    index = json.loads(arguments.index.read_text(encoding="utf-8"))
    native = {cls: set(bucket["native"]) for cls, bucket in index["functions_by_class"].items()
              if bucket["native"]}
    every = {cls: set(bucket["all"]) for cls, bucket in index["functions_by_class"].items()
             if bucket["all"]}
    owners = defaultdict(set)
    for catalogue in (native, every):
        for cls, members in catalogue.items():
            for member in members:
                owners[member].add(cls)
    sets = {"native": native, "all": every, "owners": owners}

    results = []
    rejected = []
    span_count = 0
    for section in sections:
        if section["executable"] or section["raw_size"] < 16:
            continue
        words, kinds, names = classify_section(image, section)
        for start, length in alternating_spans(kinds, minimum_words=2):
            shape, entries = read_span(image, section, words, kinds, names, start, length)
            if not entries:
                continue
            span_count += 1
            catalogue = native if shape == NATIVE_TABLE else every
            arrays = []
            for segment in split_sorted(entries):
                arrays.extend(attribute(segment, sets, shape))
            for owner, segment in arrays:
                declared = len(catalogue.get(owner, ())) if owner else 0
                if owner is None or len(segment) < arguments.minimum:
                    rejected.append({
                        "address": f"{segment[0]['address']:x}", "shape": shape,
                        "size": len(segment), "best": owner, "declared": declared,
                        "names": [e["name"] for e in segment][:8],
                    })
                    continue
                for entry in segment:
                    if shape == NATIVE_TABLE:
                        symbol = f"{owner}::exec{entry['name']}"
                    else:
                        symbol = f"Z_Construct_UFunction_{owner}_{entry['name']}"
                    results.append({
                        "address": f"{entry['target']:x}",
                        "table_entry": f"{entry['address']:x}",
                        "symbol": symbol,
                        "shape": shape,
                        "class": owner,
                        "function": entry["name"],
                        "table_size": len(segment),
                        "declared": declared,
                        "exact": len(segment) == declared,
                    })

    # Several names can share one address. These are not contradictions: the linker folds
    # identical function bodies, and generated thunks for functions with the same signature are
    # identical. Keep one name and record the rest, rather than discarding a real address.
    by_address = defaultdict(list)
    for record in results:
        by_address[record["address"]].append(record)
    named = []
    folded = 0
    for records in by_address.values():
        unique = sorted({record["symbol"]: record for record in records}.values(),
                        key=lambda record: (not record["exact"], record["symbol"]))
        primary = dict(unique[0])
        if len(unique) > 1:
            folded += 1
            primary["folded_with"] = [record["symbol"] for record in unique[1:]]
        named.append(primary)

    named.sort(key=lambda record: record["address"])
    arguments.output.write_text(json.dumps({
        "dump": str(arguments.dump),
        "image_base": f"{image_base:x}",
        "index": str(arguments.index),
        "functions": named,
        "rejected_tables": sorted(rejected, key=lambda r: -r["size"])[:200],
    }, indent=1), encoding="utf-8")

    execs = [record for record in named if record["shape"] == NATIVE_TABLE]
    constructors = [record for record in named if record["shape"] == LINK_TABLE]
    print()
    print(f"alternating spans  {span_count}")
    print(f"exec thunks        {len(execs)} in {len({r['class'] for r in execs})} classes"
          f", {sum(1 for r in execs if r['exact'])} from whole-class arrays")
    print(f"reflection ctors   {len(constructors)} in {len({r['class'] for r in constructors})}"
          f" classes, {sum(1 for r in constructors if r['exact'])} from whole-class arrays")
    print(f"folded addresses   {folded} carrying more than one name")
    print(f"rejected arrays    {len(rejected)} ({sum(r['size'] for r in rejected)} entries)")
    print(f"written            {arguments.output}")


if __name__ == "__main__":
    main()
