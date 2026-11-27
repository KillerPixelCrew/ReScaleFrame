"""Read-only Skyrim 1.7.104 hook preflight. Never loads or patches the executable."""

import argparse
import hashlib
import json
import struct
from pathlib import Path


EXPECTED_SHA256 = "846efccf0c1374d71f892907f46549560f2fcb0a75cb87a3eed438baa0f1402f"
CANDIDATES = {
    77245: ("jitter caller", 0x133),
    107148: ("post-processing caller", 0x1E7),
    82084: ("interface rendering", None),
    68617: ("input event dispatch caller", 0x7B),
    411451: ("render target manager", None),
    77303: ("native texture allocation", None),
}


def inspect(executable, table):
    image = executable.read_bytes()
    digest = hashlib.sha256(image).hexdigest()
    if digest != EXPECTED_SHA256:
        raise ValueError("Executable SHA-256 does not match the researched build")
    pe = struct.unpack_from("<I", image, 0x3C)[0]
    if image[:2] != b"MZ" or image[pe:pe + 4] != b"PE\0\0":
        raise ValueError("Invalid PE")
    machine, section_count, timestamp = struct.unpack_from("<HHI", image, pe + 4)
    optional = pe + 24
    optional_size = struct.unpack_from("<H", image, pe + 20)[0]
    if machine != 0x8664 or struct.unpack_from("<H", image, optional)[0] != 0x20B:
        raise ValueError("Expected x64 PE32+")
    image_size = struct.unpack_from("<I", image, optional + 56)[0]
    sections = []
    for index in range(section_count):
        offset = optional + optional_size + index * 40
        virtual_size, rva, raw_size, raw = struct.unpack_from("<IIII", image, offset + 8)
        flags = struct.unpack_from("<I", image, offset + 36)[0]
        sections.append((rva, min(virtual_size, raw_size), raw, flags))

    def location(rva, size):
        for start, length, raw, flags in sections:
            if start <= rva and rva + size <= start + length:
                return raw + rva - start, bool(flags & 0x20000000)
        raise ValueError(f"RVA {rva:#x} has no complete file-backed range")

    data = table.read_bytes()
    if len(data) < 72 or data[:8] != b"MITIDTAB":
        raise ValueError("Expected independent MITIDTAB, not Nexus Address Library format")
    major, minor, header_size = struct.unpack_from("<HHI", data, 8)
    version = struct.unpack_from("<4H", data, 16)
    stamp, extent, count, stride, flags, revision = struct.unpack_from("<6I", data, 24)
    if major != 1 or header_size < 64 or not 16 <= stride <= 4096 or flags:
        raise ValueError("Unsupported table header")
    if version != (1, 7, 104, 0) or (stamp, extent) != (timestamp, image_size):
        raise ValueError("Table identity does not match executable")
    if data[48:64].rstrip(b"\0") != b"SkyrimSE.exe":
        raise ValueError("Table module mismatch")
    end = header_size + count * stride
    if len(data) != end + 8:
        raise ValueError("Truncated table or unexpected trailing data")
    checksum = 0xCBF29CE484222325
    for byte in data[:end]:
        checksum = ((checksum ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    if checksum != struct.unpack_from("<Q", data, end)[0]:
        raise ValueError("Table checksum mismatch")
    records = {}
    previous = -1
    for index in range(count):
        ident, rva = struct.unpack_from("<QQ", data, header_size + index * stride)
        if ident <= previous or rva >= image_size:
            raise ValueError("Invalid table record order or range")
        previous = ident
        records[ident] = rva
    results = []
    for ident, (name, displacement) in CANDIDATES.items():
        row = {"id": ident, "name": name, "status": "unmapped"}
        rva = records.get(ident, 0)
        if rva:
            row.update(rva=hex(rva), status="reference candidate only")
            if displacement is not None:
                site = rva + displacement
                offset, executable_section = location(site, 5)
                code = image[offset:offset + 5]
                row.update(site_rva=hex(site), bytes=code.hex(" "), executable=executable_section)
                if code[0] == 0xE8 and executable_section:
                    target = site + 5 + struct.unpack_from("<i", code, 1)[0]
                    _, target_executable = location(target, 1)
                    row.update(status="rel32 call observed; semantics unverified",
                               target_rva=hex(target), target_executable=target_executable)
                else:
                    row["status"] = "rejected: expected executable rel32 call absent"
        results.append(row)
    return {"executable_sha256": digest, "table_sha256": hashlib.sha256(data).hexdigest(),
            "table_format": [major, minor], "table_revision": revision,
            "candidates": results, "limitation": "Static preflight, not hook or runtime validation"}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", type=Path)
    parser.add_argument("table", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    try:
        result = inspect(args.executable, args.table)
    except (ValueError, OSError, struct.error) as error:
        parser.exit(1, f"Refused: {error}\n")
    # Exclusive creation prevents replacing earlier research evidence.
    with args.output.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(result, stream, indent=2)
        stream.write("\n")
    print(json.dumps(result, indent=2))
