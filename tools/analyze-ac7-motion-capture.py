"""Summarize an AC7 F9 capture and write derived reports beside its source files.

Engine eligibility, sampled D3D11 draws and paired backend images have separate timelines.
The report counts actual recorded evidence and missing files, not presumed rendering support.
Writes/overwrites analysis.json and fNNN_written.pgm masks; recorded capture inputs are retained.
"""
import argparse
import collections
import json
import struct
from pathlib import Path


def records(path):
    """Read nonempty JSONL rows, treating an absent optional timeline as empty."""
    if not path.exists():
        return []
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line]


def main():
    """Analyze sampled intervals 0/30/59, respecting row pitch for R16G16_UNORM motion.

    Nonzero encoded components mark written pixels; (32767,32767) marks encoded zero motion.
    This occupancy mask does not validate displacement or deferred-command coverage.
    """
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("capture", type=Path)
    args = parser.parse_args()
    root = args.capture.resolve()
    session = json.loads((root / "session.json").read_text(encoding="utf-8"))
    engine = records(root / "engine.jsonl")
    draws = records(root / "draws.jsonl")
    reasons = collections.Counter(row["reason"] for row in engine)
    stages = collections.Counter(p.name.split("_")[1] for p in root.glob("shader_*.dxbc"))
    samples = []
    for interval in (0, 30, 59):
        prefix = f"f{interval:03}"
        frame_file = root / f"{prefix}_frame.json"
        motion_metadata = root / f"{prefix}_motion_raw.json"
        motion_file = root / f"{prefix}_motion.bin"
        sample = {"interval": interval, "frame_metadata": frame_file.exists()}
        sample["raw_resources"] = {
            role: (root / f"{prefix}_{role}.bin").exists()
            for role in ("input", "output", "motion", "motion_decoded", "depth")
        }
        if frame_file.exists():
            sample["frame"] = json.loads(frame_file.read_text(encoding="utf-8"))
        if motion_file.exists() and motion_metadata.exists():
            meta = json.loads(motion_metadata.read_text(encoding="utf-8"))
            data = motion_file.read_bytes()
            expected = meta["row_pitch"] * meta["height"]
            if len(data) != expected:
                sample["error"] = "motion byte count differs from metadata"
            elif meta["format"] == 35:
                width, height = meta["width"], meta["height"]
                mask = bytearray()
                written = zero_motion = 0
                for y in range(height):
                    row = data[y * meta["row_pitch"]:y * meta["row_pitch"] + width * 4]
                    for x, v in struct.iter_unpack("<HH", row):
                        valid = x != 0 or v != 0
                        written += valid
                        zero_motion += valid and x == 32767 and v == 32767
                        mask.append(255 if valid else 0)
                sample["written_pixels"] = written
                sample["total_pixels"] = width * height
                sample["written_fraction"] = written / (width * height)
                sample["encoded_zero_motion_pixels"] = zero_motion
                (root / f"{prefix}_written.pgm").write_bytes(
                    f"P5\n{width} {height}\n255\n".encode("ascii") + mask)
        samples.append(sample)
    report = {
        "session": session,
        "reasons": dict(reasons),
        "components_observed": len({row["component"] for row in engine}),
        "native_views_observed": len({row["view"] for row in engine}),
        "gpu_draws": sum(row["kind"] == "draw" for row in draws),
        "gpu_dispatches": sum(row["kind"] == "dispatch" for row in draws),
        "shader_stages": dict(stages),
        "samples": samples,
        "limits": "Counts show recorded coverage. They do not establish correct displacement, "
                  "pixel ownership, or motion for unobserved deferred command-list draws.",
    }
    path = root / "analysis.json"
    path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(json.dumps({
        "capture": str(root),
        "reasons": report["reasons"],
        "gpu_draws": report["gpu_draws"],
        "gpu_dispatches": report["gpu_dispatches"],
        "paired_samples": sum(all(s["raw_resources"].values()) and s["frame_metadata"] for s in samples),
        "analysis": str(path),
    }, indent=2))


if __name__ == "__main__":
    main()
