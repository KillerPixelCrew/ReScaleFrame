"""Identify view uniform buffer fields by how they behave across captures.

The stock 4.18 layout stops predicting AC7's buffer partway through, so offsets cannot simply be
read off engine source. What can be done instead is watch the same buffer in different states,
menu, briefing, flight, and let fields declare themselves: a jitter changes every frame and stays
sub-pixel, a projection matrix holds its aspect ratio, a previous-frame matrix sits near identity
while the camera is still and departs from it when it moves.

Takes the constant buffers a run of the loader wrote, and reports what each region does.
"""

import argparse
import json
import struct
from pathlib import Path


def load(path):
    data = path.read_bytes()
    return list(struct.unpack("<%df" % (len(data) // 4), data))


def finite(value):
    return value == value and abs(value) < 1e30


def classify_slots(buffers):
    """Per float slot: does it hold still, and how far does it move."""
    width = min(len(buffer) for buffer in buffers)
    slots = []
    for index in range(width):
        values = [buffer[index] for buffer in buffers if finite(buffer[index])]
        if not values:
            slots.append({"constant": True, "distinct": 0, "min": 0.0, "max": 0.0})
            continue
        distinct = len(set(round(value, 6) for value in values))
        slots.append({
            "constant": distinct <= 1,
            "distinct": distinct,
            "min": min(values),
            "max": max(values),
        })
    return slots


def find_projection(buffers, slots):
    """A projection matrix keeps a constant ratio between its first two diagonal entries."""
    found = []
    width = len(slots)
    for index in range(0, width - 16, 4):
        ratios = []
        ok = True
        for buffer in buffers:
            a, b = buffer[index], buffer[index + 5]
            if abs(a) < 0.01 or abs(b) < 0.01 or not finite(a) or not finite(b):
                ok = False
                break
            # The off diagonal of the first two rows must be empty for a plain projection.
            if abs(buffer[index + 1]) > 1e-6 or abs(buffer[index + 4]) > 1e-6:
                ok = False
                break
            ratios.append(b / a)
        if ok and ratios and max(ratios) - min(ratios) < 1e-3:
            found.append({"offset": hex(index * 4), "aspect": round(ratios[0], 5)})
    return found


def find_near_identity(buffers, slots):
    """A current-to-previous transform sits at identity whenever nothing moved."""
    found = []
    width = len(slots)
    for index in range(0, width - 16, 4):
        identity_count = 0
        departs = False
        for buffer in buffers:
            block = buffer[index:index + 16]
            if not all(finite(value) for value in block):
                break
            diagonal = abs(block[0] - 1) < 0.02 and abs(block[5] - 1) < 0.02
            off = abs(block[1]) < 0.02 and abs(block[4]) < 0.02
            if diagonal and off:
                identity_count += 1
            elif abs(block[0]) > 0.1:
                departs = True
        if identity_count >= max(1, len(buffers) // 4):
            found.append({"offset": hex(index * 4), "identity_in": identity_count,
                          "also_departs": departs})
    return found


def find_jitter(buffers, slots):
    """Temporal jitter is sub-pixel, changes constantly, and is centred on zero."""
    candidates = []
    for index in range(0, len(slots) - 1):
        pair = [slots[index], slots[index + 1]]
        if any(entry["distinct"] < max(2, len(buffers) // 3) for entry in pair):
            continue
        if any(abs(entry["min"]) > 1.0 or abs(entry["max"]) > 1.0 for entry in pair):
            continue
        # Must actually straddle zero rather than merely being small.
        if not all(entry["min"] < 0.0 < entry["max"] for entry in pair):
            continue
        spread = max(entry["max"] - entry["min"] for entry in pair)
        if spread < 1e-4:
            continue
        candidates.append({"offset": hex(index * 4), "spread": round(spread, 6),
                           "range": [round(pair[0]["min"], 5), round(pair[0]["max"], 5)],
                           "distinct": pair[0]["distinct"]})
    return candidates


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--pattern", default="capture*_cb4096.bin")
    parser.add_argument("--output", type=Path, default=None)
    args = parser.parse_args()

    paths = sorted(args.directory.glob(args.pattern))
    if not paths:
        raise SystemExit(f"no buffers matching {args.pattern}")
    buffers = [load(path) for path in paths]
    print(f"{len(buffers)} buffers, {len(buffers[0]) * 4} bytes each\n")

    slots = classify_slots(buffers)
    varying = sum(1 for slot in slots if not slot["constant"])
    print(f"{varying} of {len(slots)} float slots vary across captures\n")

    projections = find_projection(buffers, slots)
    print("projection-shaped matrices (constant aspect across every capture):")
    for entry in projections:
        print(f"  {entry['offset']:>8}  aspect {entry['aspect']}")

    identities = find_near_identity(buffers, slots)
    print("\nnear-identity matrices (previous-frame transforms):")
    for entry in identities[:8]:
        print(f"  {entry['offset']:>8}  identity in {entry['identity_in']} captures"
              f"{', departs elsewhere' if entry['also_departs'] else ''}")

    jitter = find_jitter(buffers, slots)
    print(f"\njitter candidates, sub-pixel and straddling zero ({len(jitter)} found):")
    for entry in jitter[:12]:
        print(f"  {entry['offset']:>8}  spread {entry['spread']}  range {entry['range']}"
              f"  {entry['distinct']} distinct")

    if args.output:
        args.output.write_text(json.dumps(
            {"buffers": [str(path.name) for path in paths], "projections": projections,
             "near_identity": identities, "jitter_candidates": jitter}, indent=2) + "\n",
            encoding="utf-8")


if __name__ == "__main__":
    main()
