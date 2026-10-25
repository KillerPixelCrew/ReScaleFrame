"""Measure saved FSR2 color coverage without changing any acceptance gate."""
import argparse
import importlib.util
import json
from pathlib import Path

import numpy as np

SPEC = importlib.util.spec_from_file_location(
    "godot_capture", Path(__file__).resolve().parents[1] / "analyze-godot-capture.py")
CAPTURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CAPTURE)


def erode(mask, count=3):
    for _ in range(count):
        padded = np.pad(mask, 1)
        mask = np.logical_and.reduce([
            padded[y:y + mask.shape[0], x:x + mask.shape[1]]
            for y, x in ((1, 1), (0, 1), (2, 1), (1, 0), (1, 2))])
    return mask


def weights(rgb):
    return np.maximum(rgb[..., 2] - rgb[..., 0] - 0.005, 0)


def measure(source, reference):
    cyan = (reference[..., 2] > reference[..., 0] + 0.15) & (
        reference[..., 1] > reference[..., 0] + 0.1)
    yy, xx = np.where(cyan)
    if xx.size < 50:
        raise ValueError("Reference cyan cube missing")
    bounds = [int(xx.min()) - 6, int(yy.min()) - 6, int(xx.max()) + 7, int(yy.max()) + 7]
    x0, y0, x1, y1 = bounds
    roi = np.s_[y0:y1, x0:x1]
    reference_weights, source_weights = weights(reference[roi]), weights(source[roi])
    y, x = np.mgrid[y0:y1, x0:x1]
    position = np.stack((x, y), axis=-1)
    ref_total, source_total = reference_weights.sum(), source_weights.sum()
    center = (position * reference_weights[..., None]).sum(axis=(0, 1)) / ref_total
    offset = position - center
    delta = source_weights - reference_weights
    core = erode(cyan)[roi]
    contribution = offset * delta[..., None] / source_total
    core_shift = contribution[core].sum(axis=0)
    boundary_shift = contribution[~core].sum(axis=0)
    source_center = (position * source_weights[..., None]).sum(axis=(0, 1)) / source_total
    error = source_center - center
    if not np.allclose(error, core_shift + boundary_shift, atol=1e-10):
        raise ValueError("Coverage first-moment decomposition failed")
    rows = slice(int(yy.min() + (yy.max() - yy.min()) * 0.30),
                 int(yy.min() + (yy.max() - yy.min()) * 0.75) + 1)
    return {
        "roi_xyxy": bounds, "reference_centroid": center.tolist(),
        "source_centroid": source_center.tolist(), "centroid_error_vector_pixels": error.tolist(),
        "source_coverage_mass": float(source_total), "reference_coverage_mass": float(ref_total),
        "core_contribution_pixels": core_shift.tolist(), "boundary_contribution_pixels": boundary_shift.tolist(),
        "core_signed_mass_delta": float(delta[core].sum()),
        "boundary_signed_mass_delta": float(delta[~core].sum()),
        "horizontal_profile_output_x": list(range(x0, x1)),
        "horizontal_profile_rows": [rows.start, rows.stop],
        "reference_horizontal_profile": weights(reference[rows, x0:x1]).mean(axis=0).tolist(),
        "source_horizontal_profile": weights(source[rows, x0:x1]).mean(axis=0).tolist(),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("capture", type=Path)
    parser.add_argument("reference", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    report = json.loads((args.capture / "result.json").read_text())
    reference_report = json.loads((args.reference / "result.json").read_text())
    poses = {pose["process_frame"]: pose for pose in report["poses"]}
    reference_poses = {pose["process_frame"]: pose for pose in reference_report["poses"]}
    references = {}
    for path in (args.reference / "frames").glob("*fg-hudless-color*.json"):
        meta, rgb = CAPTURE.load_image(path)
        references[reference_poses[meta["frame"]]["animation_frame"]] = rgb
    frames = []
    for path in sorted((args.capture / "frames").glob("*fg-hudless-color*.json")):
        meta, rgb = CAPTURE.load_image(path)
        time = poses[meta["frame"]]["animation_frame"]
        reference = references[time]
        if rgb.shape != reference.shape:
            raise ValueError("Matched output extents required")
        result = measure(rgb, reference)
        result.update(frame=meta["frame"], animation_frame=time, session=meta["session"],
                      view=meta["view"], generation=meta["generation"], source=str(path))
        frames.append(result)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps({
        "scope": "Diagnostic coverage first moments, no acceptance gate or inferred SDK stage",
        "capture": str(args.capture), "reference": str(args.reference), "frames": frames,
    }, indent=2) + "\n")
    for frame in frames:
        print(json.dumps({key: frame[key] for key in (
            "frame", "centroid_error_vector_pixels", "core_contribution_pixels", "boundary_contribution_pixels")}))


if __name__ == "__main__":
    main()
