"""Replay pinned FSR2 nearest-depth motion selection from captured Godot inputs.

This is a CPU source-model diagnostic, not an internal SDK GPU readback. It does
not change the generated-image gates or establish a production correction.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path

import numpy as np


def load_capture_module():
    spec = importlib.util.spec_from_file_location(
        "godot_capture", Path(__file__).with_name("analyze-godot-capture.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def replay(depth, motion):
    """Match FindNearestDepth's ordered, strict comparison for reverse depth."""
    height, width = depth.shape
    y, x = np.indices(depth.shape)
    nearest_depth = depth.copy()
    selected = motion.copy()
    selected_offsets = np.zeros((*depth.shape, 2), np.int32)
    for dx, dy in ((1, 0), (0, 1), (0, -1), (-1, 0),
                   (-1, 1), (1, 1), (-1, -1), (1, -1)):
        valid = (x + dx >= 0) & (x + dx < width) & (y + dy >= 0) & (y + dy < height)
        yy, xx = np.clip(y + dy, 0, height - 1), np.clip(x + dx, 0, width - 1)
        replace = valid & (depth[yy, xx] > nearest_depth)
        nearest_depth[replace] = depth[yy, xx][replace]
        selected[replace] = motion[yy, xx][replace]
        selected_offsets[replace] = (dx, dy)
    return nearest_depth, selected, selected_offsets


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("capture", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    capture = load_capture_module()
    groups, inputs = {}, []
    for path in sorted((args.capture / "frames").glob("*sr-*.json")):
        meta, image = capture.load_image(path)
        groups.setdefault(meta["frame"], {})[meta["role"]] = (meta, image)
        inputs.append({"file": path.name,
                       "metadata_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                       "raw_sha256": hashlib.sha256(path.with_suffix(".raw").read_bytes()).hexdigest()})
    findings = []
    for frame, resources in sorted(groups.items()):
        if not {"sr-depth", "sr-motion-uv", "sr-linear-color"}.issubset(resources):
            continue
        meta, depth = resources["sr-depth"]
        if meta.get("sr_backend") != 2:
            raise ValueError("This diagnostic requires an actual FSR2 capture")
        if meta["view_to_clip"][10] <= 0:
            raise ValueError("This diagnostic models the pinned reverse-depth Godot fixture only")
        identities = {(v[0]["session"], v[0]["view"], v[0]["generation"]) for v in resources.values()}
        if len(identities) != 1:
            raise ValueError(f"Mismatched resource identities in frame {frame}")
        motion = resources["sr-motion-uv"][1][..., :2]
        color = resources["sr-linear-color"][1]
        if depth.shape[:2] != motion.shape[:2] or depth.shape[:2] != color.shape[:2]:
            raise ValueError(f"Input extents differ in frame {frame}")
        if not np.isfinite(depth).all() or not np.isfinite(motion).all():
            raise ValueError(f"Nonfinite depth or motion in frame {frame}")
        nearest, selected, offsets = replay(depth[..., 0], motion)
        # Restrict to the cyan material. Background receives foreground dilation
        # intentionally, so its difference from original flow is not an error.
        mask = (color[..., 2] > color[..., 0] * 2) & (color[..., 2] > 0.3)
        if mask.sum() < 100:
            raise ValueError(f"Insufficient cyan material samples in frame {frame}")
        delta = (selected - motion) * np.asarray(meta["output_size"])
        distances = np.linalg.norm(delta[mask], axis=-1)
        worst = np.unravel_index(np.argmax(np.where(mask, np.linalg.norm(delta, axis=-1), -1)), mask.shape)
        y, x = worst
        findings.append({
            "frame": frame, "session": meta["session"], "view": meta["view"],
            "generation": meta["generation"], "material_pixels": int(mask.sum()),
            "render_size": meta["render_size"], "output_size": meta["output_size"],
            "selection_delta_output_pixels_p50_p95_p99_max": np.percentile(distances, [50, 95, 99, 100]).tolist(),
            "selection_delta_x_output_pixels_min_max": [float(delta[..., 0][mask].min()), float(delta[..., 0][mask].max())],
            "selection_delta_mean_xy_output_pixels": delta[mask].mean(axis=0).tolist(),
            "selected_zero_motion_material_pixels": int(np.count_nonzero(np.linalg.norm(selected[mask], axis=-1) == 0)),
            "worst_sample": {"xy": [int(x), int(y)], "selected_offset_xy": offsets[y, x].tolist(),
                             "depth": float(depth[y, x, 0]), "selected_depth": float(nearest[y, x]),
                             "motion_uv": motion[y, x].tolist(), "selected_motion_uv": selected[y, x].tolist()},
        })
    if len(findings) != 4:
        raise ValueError("Expected four complete captured SR frames")
    report = {"scope": "CPU replay of pinned FSR2 2.3.3 reverse-depth motion selection; no GPU intermediate readback or quality acceptance",
              "model_sdk_commit": "c6efa6bf7f2027b3ec94f28578bb5965eabb9e55",
              "model_source": "sdk/include/FidelityFX/gpu/fsr2/ffx_fsr2_reconstruct_dilated_velocity_and_previous_depth.h:FindNearestDepth",
              "capture": str(args.capture.resolve()), "inputs": inputs, "frames": findings}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"frames": len(findings), "output": str(args.output)}))


if __name__ == "__main__":
    main()
