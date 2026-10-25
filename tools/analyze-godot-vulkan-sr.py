"""Inspect real Vulkan SR input/output readbacks and geometry-derived motion."""
import argparse
import html
import json
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

from importlib.util import module_from_spec, spec_from_file_location

spec = spec_from_file_location("godot_capture", Path(__file__).with_name("analyze-godot-capture.py"))
capture = module_from_spec(spec)
spec.loader.exec_module(capture)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("directory", type=Path)
    parser.add_argument("--report", required=True, type=Path)
    args = parser.parse_args()
    report = json.loads(args.report.read_text(encoding="utf-8-sig"))
    poses = {p["process_frame"]: p for p in report["poses"]}
    output = args.directory / "decoded"
    output.mkdir(exist_ok=True)
    frames, failures, findings, panels_html = {}, [], [], []
    if not report.get("passed"):
        failures.append("Engine fixture failed")
    if (args.directory / "capture-errors.log").exists():
        failures.append("Native readback reported an error")
    for path in sorted(args.directory.glob("*.json")):
        meta, pixels = capture.load_image(path)
        resources = frames.setdefault(meta["frame"], {})
        if meta["role"] in resources:
            failures.append(f"Duplicate image identity: {path.name}")
        resources[meta["role"]] = (meta, pixels)
    required = {"sr-linear-color", "sr-depth", "sr-motion-uv", "sr-output-linear"}
    for frame, resources in sorted(frames.items()):
        if not required.issubset(resources) or frame not in poses or frame - 1 not in poses:
            failures.append(f"Frame {frame}: missing input, output or independent scene pose")
            continue
        identities = {(v[0]["session"], v[0]["view"], v[0]["generation"]) for v in resources.values()}
        if len(identities) != 1:
            failures.append(f"Frame {frame}: image identity mismatch")
        for role, (meta, data) in resources.items():
            if "vk_format" not in meta or meta.get("completion") != "owner-drained-context-retirement":
                failures.append(f"Frame {frame}: missing Vulkan format or completion evidence")
            if not np.isfinite(data).all():
                failures.append(f"Frame {frame}: nonfinite {role}")
        meta, depth = resources["sr-depth"]
        width, height = meta["render_size"]
        if any(resources[role][1].shape[:2] != (height, width) for role in required - {"sr-output-linear"}):
            failures.append(f"Frame {frame}: input extent mismatch")
            continue
        if resources["sr-output-linear"][1].shape[:2] != tuple(reversed(meta["output_size"])):
            failures.append(f"Frame {frame}: output extent mismatch")
        if depth.min() < 0 or depth.max() > 1 or np.count_nonzero(depth) < 100:
            failures.append(f"Frame {frame}: invalid or empty normalized depth")
        motion = resources["sr-motion-uv"][1] * np.array([width, height])
        metrics = capture.measure_motion(meta, depth, motion, resources["sr-linear-color"][1], poses[frame], poses[frame - 1])
        metrics["frame"] = frame
        jitter = capture.fit_raster_jitter(meta, depth, resources["sr-linear-color"][1], poses[frame])
        metrics["raster_jitter_fit"] = jitter
        if jitter is None or jitter["metadata_error_pixels"] > 0.02:
            failures.append(f"Frame {frame}: raster jitter does not match the box planes")
        for region in ("object", "floor"):
            values = metrics[region]
            if values["pixels"] < 100 or values.get("ground_truth_error_p95", float("inf")) > 0.03:
                failures.append(f"Frame {frame}: {region} motion disagrees with scene geometry")
            if values.get("camera_metadata_error_p95", float("inf")) > 0.01:
                failures.append(f"Frame {frame}: camera metadata disagrees with scene poses")
        if metrics["camera_translation_error"] > 1e-4:
            failures.append(f"Frame {frame}: incorrect camera translation")
        image = resources["sr-output-linear"][1]
        metrics["output_rgb_variance"] = float(np.var(image[..., :3]))
        if metrics["output_rgb_variance"] < 1e-5:
            failures.append(f"Frame {frame}: SDK output is empty or constant")
        findings.append(metrics)
        previews = [("Linear scene input (preview mapping)", resources["sr-linear-color"][1], "linear"),
                    ("Actual SDK SR output (preview mapping)", image, "linear"),
                    ("Depth (logarithmic)", depth, "depth"), ("Motion (render pixels)", motion, "motion")]
        sheet = Image.new("RGB", (640 * 4, 384), "#10151d")
        draw = ImageDraw.Draw(sheet)
        for index, (title, data, role) in enumerate(previews):
            sheet.paste(capture.png(data, role).resize((640, 360)), (index * 640, 24))
            draw.text((index * 640 + 8, 6), title, fill="white")
        name = f"frame-{frame}-sr-guides.png"
        sheet.save(output / name)
        panels_html.append(f'<h2>Source {frame}</h2><a href="{name}"><img src="{name}" style="width:100%"></a>')
    if len(findings) != 4:
        failures.append("Expected four complete source frames")
    summary = {"passed": not failures, "scope": "Vulkan SR inputs and actual output readbacks; no independent SR quality reference, FG or latency acceptance",
               "failures": failures, "frames": findings}
    (output / "analysis.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    state = "PASS" if summary["passed"] else "FAIL"
    errors = "".join(f"<li>{html.escape(value)}</li>" for value in failures)
    (output / "report.html").write_text(f'<!doctype html><html lang="en"><meta charset="utf-8"><title>Vulkan SR: {state}</title>'
        f'<body style="font:16px system-ui;background:#10151d;color:#e4ebf4"><h1>{state}: Vulkan SR readbacks</h1>'
        f'<p>{summary["scope"]}. Motion hue shows direction; brightness saturates at 3 render pixels.</p><ul>{errors}</ul>'
        + "".join(panels_html) + '<p><a href="analysis.json">Numeric geometry checks</a></p></body></html>', encoding="utf-8")
    print(json.dumps({"passed": summary["passed"], "frames": len(findings), "failures": failures}))
    if failures:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
