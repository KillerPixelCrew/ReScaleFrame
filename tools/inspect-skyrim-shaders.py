"""Inventory bounded DXBC containers in an extracted Skyrim FXP without modifying it.

This identifies original bytecode and output signatures. MRT semantics and active
permutations still require native shader-family and draw evidence.
"""

import argparse
import collections
import hashlib
import json
import struct
from pathlib import Path

KNOWN_FXP = "72cfc73eb0938d5949f0b3a8683722a33b671a34b41b750ba138de8d154c5fbc"
# The constructor strings and sequential loader calls were matched in 1.7.104.
NATIVE_FAMILIES = ("BloodSplatter", "DistantTree", "RunGrass", "Particle", "Sky",
                   "Effect", "Lighting", "Utility", "Water")


def native_groups(data, shaders):
    by_offset = {shader["offset"]: shader for shader in shaders}
    position = 0
    groups = []
    for family in NATIVE_FAMILIES:
        if position + 8 > len(data):
            raise ValueError("Native group header bounds")
        vertices, pixels = struct.unpack_from("<II", data, position)
        if vertices + pixels > len(shaders):
            raise ValueError("Native group count exceeds corpus")
        group = {"family": family, "offset": position, "vs": vertices, "ps": pixels}
        position += 8
        for stage, count, metadata_size, groups_offset in (
                ("vs", vertices, 44, 40), ("ps", pixels, 80, 76)):
            descriptors = set()
            for _ in range(count):
                if position + metadata_size > len(data):
                    raise ValueError("Native record bounds")
                magic, size, descriptor = struct.unpack_from("<III", data, position)
                if magic != 0x11223344 or descriptor in descriptors:
                    raise ValueError("Native marker or duplicate stage descriptor")
                descriptors.add(descriptor)
                shader = by_offset.get(position + metadata_size)
                if shader is None or shader["bytes"] != size or shader["stage"] != stage:
                    raise ValueError("Native record disagrees with DXBC inventory")
                shader.update(native_family=family, native_descriptor=descriptor,
                              native_record_offset=position,
                              native_constant_groups=list(data[position + groups_offset:position + groups_offset + 4]))
                position += metadata_size + size
        group["end"] = position
        groups.append(group)
    return {"groups": groups, "classified_containers": sum(g["vs"] + g["ps"] for g in groups),
            "unclassified_tail_offset": position,
            "limitation": "Remaining image-space/compute groups are not classified yet."}


def inventory(path):
    data = path.read_bytes()
    shaders = []
    rejected = []
    position = 0
    while (position := data.find(b"DXBC", position)) >= 0:
        start = position
        position += 4
        try:
            if start + 32 > len(data):
                raise ValueError("short header")
            size, count = struct.unpack_from("<II", data, start + 24)
            if size < 32 or size > len(data) - start or count > (size - 32) // 4:
                raise ValueError("container bounds")
            blob = data[start:start + size]
            chunks = []
            stage = None
            outputs = []
            ranges = []
            for index in range(count):
                offset = struct.unpack_from("<I", blob, 32 + index * 4)[0]
                if offset < 32 + count * 4 or offset + 8 > size:
                    raise ValueError("chunk header bounds")
                length = struct.unpack_from("<I", blob, offset + 4)[0]
                end = offset + 8 + length
                if end > size or any(offset < b and a < end for a, b in ranges):
                    raise ValueError("chunk bounds or overlap")
                ranges.append((offset, end))
                kind = blob[offset:offset + 4]
                chunks.append(kind.decode("ascii", "replace"))
                if kind in (b"SHDR", b"SHEX"):
                    if length < 8 or length % 4:
                        raise ValueError("program bounds")
                    version, words = struct.unpack_from("<II", blob, offset + 8)
                    if words * 4 != length:
                        raise ValueError("program word count")
                    stage = {0: "ps", 1: "vs", 2: "gs", 3: "hs", 4: "ds", 5: "cs"}.get(version >> 16)
                if kind == b"OSGN":
                    if length < 8:
                        raise ValueError("output signature bounds")
                    body = blob[offset + 8:end]
                    elements = struct.unpack_from("<I", body)[0]
                    if elements > (length - 8) // 24:
                        raise ValueError("output element count")
                    for element in range(elements):
                        row = 8 + element * 24
                        name, semantic_index, system_value, component, register = struct.unpack_from("<5I", body, row)
                        terminator = body.find(b"\0", name)
                        if name < 8 + elements * 24 or terminator < name:
                            raise ValueError("output semantic bounds")
                        outputs.append({"semantic": body[name:terminator].decode("ascii"),
                                        "index": semantic_index, "system_value": system_value,
                                        "component_type": component, "register": register,
                                        "mask": body[row + 20]})
            if stage is None:
                raise ValueError("no recognized shader program")
            shaders.append({"offset": start, "bytes": size,
                            "sha256": hashlib.sha256(blob).hexdigest(), "stage": stage,
                            "chunks": chunks, "outputs": outputs})
            position = start + size
        except (ValueError, struct.error, UnicodeDecodeError) as error:
            rejected.append({"offset": start, "reason": str(error)})
    digest = hashlib.sha256(data).hexdigest()
    native = native_groups(data, shaders) if digest == KNOWN_FXP else None
    return {"input_sha256": digest, "input_bytes": len(data), "native": native,
            "count": len(shaders), "stages": dict(collections.Counter(s["stage"] for s in shaders)),
            "rejected_candidates": rejected, "shaders": shaders,
            "limitations": "No DXBC checksum validation, draw coverage or runtime observation. Native families are mapped only for the exact known FXP's first nine groups. Only OSGN output signatures decoded."}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("fxp", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    result = inventory(args.fxp)
    with args.output.open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(result, stream, indent=2)
        stream.write("\n")
    print(json.dumps({key: value for key, value in result.items() if key != "shaders"}, indent=2))
