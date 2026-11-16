#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Inventory SR evidence without treating a successful API result as visual acceptance."""
import argparse
import json
from pathlib import Path


def inspect(bundle):
    errors = []
    try:
        manifest = json.loads((bundle / "manifest.json").read_text(encoding="utf-8-sig"))
        frame = json.loads((bundle / "frame.json").read_text(encoding="utf-8-sig"))
    except (OSError, ValueError) as error:
        return {"bundle": str(bundle), "classification": "invalid", "errors": [str(error)]}
    for key in ("session_id", "source_frame_id"):
        if manifest.get(key) is None or manifest.get(key) != frame.get(key):
            errors.append(f"{key} missing or mismatched")
    images = {image.get("name"): image for image in manifest.get("images", [])}
    def captured(name):
        image = images.get(name, {})
        if image.get("status") != "captured":
            return False
        if not image.get("raw") and not image.get("png"):
            errors.append(f"{name}: captured status without an artifact")
            return False
        for field in ("raw", "png"):
            value = image.get(field)
            if value:
                target = (bundle / value).resolve()
                if not target.is_relative_to(bundle.resolve()) or not target.is_file():
                    errors.append(f"{name}: missing or unsafe {field} artifact")
                    return False
        return True
    evaluated = frame.get("camera_available") is True and captured("sr_backend_output")
    final = captured("final_color")
    spatial = captured("spatial_recovered")
    complete = manifest.get("status") == "complete" and manifest.get("matched_present") is True and manifest.get("present_result") == 0 and final
    if errors:
        classification = "invalid"
    elif evaluated and manifest.get("sr_result") == 0 and complete:
        classification = "evaluated-presented"
    elif spatial:
        classification = "spatial-recovery"
    else:
        classification = "partial-or-failed"
    return {"bundle": str(bundle), "classification": classification,
            "native_frame": manifest.get("native_frame"), "source_frame_id": manifest.get("source_frame_id"),
            "render": frame.get("render"), "output": frame.get("output"),
            "sr_result": manifest.get("sr_result"), "present_result": manifest.get("present_result"),
            "camera_available": frame.get("camera_available"), "backend_output_captured": evaluated,
            "final_captured": final, "visual_acceptance": "not-established", "errors": errors}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="+", type=Path, help="Bundles or capture parent directories")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    bundles = set()
    for path in args.paths:
        if (path / "manifest.json").is_file():
            bundles.add(path.resolve())
        elif path.is_dir():
            bundles.update(item.parent.resolve() for item in path.glob("*/manifest.json"))
    if not bundles:
        parser.error("No capture manifests found")
    rows = [inspect(path) for path in sorted(bundles)]
    result = json.dumps({"captures": rows}, indent=2)
    if args.output:
        args.output.write_text(result + "\n", encoding="utf-8")
    print(result)
    return 2 if any(row["errors"] for row in rows) else 0


if __name__ == "__main__":
    raise SystemExit(main())
