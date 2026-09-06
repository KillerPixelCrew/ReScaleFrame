"""Compute the byte offsets of Unreal's view uniform buffer members.

The plugin needs matrices, jitter and camera parameters every frame, and none of it is visible in
a frame capture. It lives in one constant buffer whose layout is fixed by the member table in
SceneView.h, so the offsets can be derived from source and then checked against the buffer the
running game actually binds.

Offsets follow Unreal's uniform buffer rules, which are the HLSL constant buffer rules: every
member is aligned to its natural size, nothing straddles a sixteen byte boundary, and matrices and
four component vectors are sixteen byte aligned.

These offsets describe stock 4.18.3. AC7 is a vendor branch, so treat them as a hypothesis to
confirm against a dumped buffer rather than as ground truth. `--verify` does that comparison.
"""

import argparse
import json
import re
import struct
from pathlib import Path

MEMBER = re.compile(
    r"VIEW_UNIFORM_BUFFER_MEMBER(?P<kind>_EX|_ARRAY)?\(\s*(?P<args>[^)]*)\)")

# size and alignment in bytes, following Unreal's shader parameter types
TYPES = {
    "FMatrix": (64, 16),
    "FVector4": (16, 16),
    "FVector": (12, 4),
    "FVector2D": (8, 4),
    "float": (4, 4),
    "uint32": (4, 4),
    "int32": (4, 4),
}


def parse_members(header_text):
    members = []
    for match in MEMBER.finditer(header_text):
        arguments = [part.strip() for part in match.group("args").split(",")]
        kind = match.group("kind")
        if kind == "_ARRAY":
            # TYPE, NAME, [COUNT]
            if len(arguments) < 2:
                continue
            type_name, name = arguments[0], arguments[1]
            count_text = arguments[2] if len(arguments) > 2 else "[1]"
            count = int(re.sub(r"[^\d]", "", count_text) or "1")
        else:
            if len(arguments) < 2:
                continue
            type_name, name = arguments[0], arguments[1]
            count = 1
        if type_name not in TYPES:
            members.append({"name": name, "type": type_name, "count": count, "unknown": True})
            continue
        members.append({"name": name, "type": type_name, "count": count})
    return members


def compute_offsets(members):
    offset = 0
    laid_out = []
    for member in members:
        if member.get("unknown"):
            laid_out.append({**member, "offset": None})
            continue
        size, alignment = TYPES[member["type"]]
        if member["count"] > 1:
            # Arrays put every element on its own sixteen byte boundary.
            alignment = 16
            stride = (size + 15) & ~15
            total = stride * member["count"]
        else:
            total = size
        offset = (offset + alignment - 1) & ~(alignment - 1)
        # A member may not straddle a sixteen byte boundary.
        if total <= 16 and (offset % 16) + total > 16:
            offset = (offset + 15) & ~15
        laid_out.append({**member, "offset": offset, "size": total})
        offset += total
    return laid_out, (offset + 15) & ~15


def looks_like_matrix(values):
    """A projection or view matrix has finite, bounded entries and is not all zero."""
    if any(value != value or abs(value) > 1e12 for value in values):
        return False
    return any(abs(value) > 1e-6 for value in values)


def verify(laid_out, dump_path, wanted):
    data = Path(dump_path).read_bytes()
    findings = []
    for member in laid_out:
        if member["name"] not in wanted or member.get("offset") is None:
            continue
        offset = member["offset"]
        if offset + member["size"] > len(data):
            findings.append({"name": member["name"], "offset": offset, "status": "past end"})
            continue
        count = member["size"] // 4
        values = struct.unpack_from("<%df" % count, data, offset)
        status = "plausible"
        if member["type"] == "FMatrix" and not looks_like_matrix(values):
            status = "implausible"
        findings.append({"name": member["name"], "offset": hex(offset), "type": member["type"],
                         "status": status,
                         "values": [round(value, 5) for value in values[:8]]})
    return findings


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scene_view_header", type=Path,
                        help="Engine/Source/Runtime/Engine/Public/SceneView.h")
    parser.add_argument("--output", type=Path, default=None)
    parser.add_argument("--verify", type=Path, default=None,
                        help="a constant buffer dumped from the running game")
    parser.add_argument("--interesting", default=("ViewToClip,ClipToView,ClipToPrevClip,"
                                                  "TemporalAAJitter,PrevViewProj,ViewForward,"
                                                  "ViewUp,ViewRight,FieldOfViewWideAngles,"
                                                  "ViewRectMin,ViewSizeAndInvSize,"
                                                  "BufferSizeAndInvSize,WorldCameraOrigin"))
    args = parser.parse_args()

    members = parse_members(args.scene_view_header.read_text(encoding="utf-8", errors="replace"))
    laid_out, total = compute_offsets(members)
    wanted = [name.strip() for name in args.interesting.split(",")]

    print(f"{len(laid_out)} members, {total} bytes total\n")
    print("members the integration needs:")
    for member in laid_out:
        if member["name"] in wanted:
            offset = member.get("offset")
            location = hex(offset) if offset is not None else "unknown"
            print(f"  {location:>8}  {member['type']:<10} {member['name']}")

    if args.verify:
        print(f"\nchecking against {args.verify}:")
        for finding in verify(laid_out, args.verify, wanted):
            print(f"  {finding.get('offset', '?'):>8}  {finding['name']:<24} {finding['status']}")
            if finding.get("values"):
                print(f"            {finding['values']}")

    if args.output:
        args.output.write_text(json.dumps({"total_size": total, "members": laid_out}, indent=2)
                               + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
