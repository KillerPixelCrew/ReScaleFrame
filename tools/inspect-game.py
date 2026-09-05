"""Read-only PE fingerprint and exact source-anchor search. Does not load the game."""

import argparse
import hashlib
import json
import mmap
import re
import struct
from pathlib import Path


ANCHORS = [
    "t.IdleWhenNotForeground", "r.OneFrameThreadLag", "r.ScreenPercentage",
    "r.PostProcessAAQuality", "r.TemporalAASamples", "r.Tonemapper.MergeWithUpscale.Mode",
    "TemporalAA", "SceneDepthZ", "Velocity", "PostProcessUpscale",
    "FEngineLoop::Tick", "UGameEngine::Tick", "UEngine::Tick", "causeevent",
    "DrawHUD", "SlateUI", "++UE4+Release-4.18", "4.18.3",
]


def inspect(path):
    with path.open("rb") as stream:
        digest = hashlib.sha256()
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
        with mmap.mmap(stream.fileno(), 0, access=mmap.ACCESS_READ) as data:
            def unpack(fmt, offset):
                return struct.unpack_from(fmt, data, offset)

            pe = unpack("<I", 0x3C)[0]
            if data[:2] != b"MZ" or data[pe:pe + 4] != b"PE\0\0":
                raise ValueError("Not a PE executable")
            machine, count, timestamp = unpack("<HHI", pe + 4)
            optional_size = unpack("<H", pe + 20)[0]
            optional = pe + 24
            magic = unpack("<H", optional)[0]
            if magic != 0x20B:
                raise ValueError("This inspector expects PE32+ (64-bit)")
            image_base = unpack("<Q", optional + 24)[0]
            sections = []
            for index in range(count):
                pos = optional + optional_size + index * 40
                name = data[pos:pos + 8].split(b"\0")[0].decode("ascii", "replace")
                virtual_size, rva, raw_size, raw_offset = unpack("<IIII", pos + 8)
                sections.append(dict(name=name, rva=rva, virtual_size=virtual_size,
                                     raw_offset=raw_offset, raw_size=raw_size))

            def rva_offset(rva):
                for section in sections:
                    if section["rva"] <= rva < section["rva"] + section["raw_size"]:
                        return section["raw_offset"] + rva - section["rva"]
                raise ValueError(f"Unmapped file RVA: {rva:#x}")

            def file_location(offset):
                for section in sections:
                    if section["raw_offset"] <= offset < section["raw_offset"] + section["raw_size"]:
                        return {"file_offset": hex(offset), "section": section["name"],
                                "rva": hex(section["rva"] + offset - section["raw_offset"])}
                return {"file_offset": hex(offset), "section": None, "rva": None}

            def cstring(offset):
                end = data.find(b"\0", offset, min(offset + 1024, len(data)))
                if end < 0:
                    raise ValueError("Unterminated PE name")
                return data[offset:end].decode("ascii", "replace")

            imports = []
            import_rva, import_size = unpack("<II", optional + 112 + 8)
            if import_rva:
                descriptor = rva_offset(import_rva)
                for index in range(min(import_size // 20 + 1, 1024)):
                    original, _, _, name_rva, first = unpack("<IIIII", descriptor + index * 20)
                    if not name_rva:
                        break
                    dll = cstring(rva_offset(name_rva))
                    functions = []
                    thunk = rva_offset(original or first)
                    for slot in range(65536):
                        value = unpack("<Q", thunk + slot * 8)[0]
                        if not value:
                            break
                        functions.append(f"ordinal:{value & 0xFFFF}" if value >> 63
                                         else cstring(rva_offset(value) + 2))
                    imports.append({"dll": dll, "functions": functions})

            anchors = {anchor: {} for anchor in ANCHORS}
            for encoding in ("ascii", "utf-16-le"):
                encoded = {anchor.encode(encoding): anchor for anchor in ANCHORS}
                expression = re.compile(b"|".join(re.escape(value) for value in encoded))
                for match in expression.finditer(data):
                    anchor = encoded[match.group()]
                    bucket = anchors[anchor].setdefault(encoding, {"count": 0, "first_locations": []})
                    bucket["count"] += 1
                    if len(bucket["first_locations"]) < 8:
                        bucket["first_locations"].append(file_location(match.start()))
            return dict(path=str(path), size_bytes=len(data), sha256=digest.hexdigest(),
                        machine=hex(machine), pe_timestamp_raw=timestamp, image_base=hex(image_base),
                        entry_point_rva=hex(unpack("<I", optional + 16)[0]), sections=sections,
                        imports=imports, exact_string_anchors=anchors,
                        limits="File-only inspection. String matches are not resolved functions, live addresses, or proof of executed rendering paths. Delay-loaded and dynamically resolved imports are not enumerated.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("executable", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    result = inspect(args.executable)
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"sha256": result["sha256"], "bytes": result["size_bytes"],
                      "machine": result["machine"],
                      "matched_anchors": [name for name, matches in result["exact_string_anchors"].items() if matches],
                      "graphics_imports": [entry for entry in result["imports"] if entry["dll"].lower() in ("dxgi.dll", "d3d11.dll", "d3d12.dll")]}))
