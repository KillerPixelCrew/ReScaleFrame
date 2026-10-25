"""Decode actual FG outputs and guides, then compare motion with scene geometry."""
import argparse
import html
import json
import re
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw


def load_image(path):
    meta = json.loads(path.read_text())
    formats = {9: (np.float16, 4), 10: (np.float16, 4), 33: (np.float16, 2),
               34: (np.float16, 2), 41: (np.float32, 1), 27: (np.uint8, 4), 28: (np.uint8, 4),
               29: (np.uint8, 4), 87: (np.uint8, 4), 90: (np.uint8, 4), 91: (np.uint8, 4)}
    vk_formats = {97: (np.float16, 4), 83: (np.float16, 2), 100: (np.float32, 1), 126: (np.float32, 1)}
    source_formats, format_key = (vk_formats, "vk_format") if "vk_format" in meta else (formats, "dxgi_format")
    if meta[format_key] not in source_formats:
        raise ValueError(f"Unsupported capture format: {path}: {format_key}={meta[format_key]}")
    dtype, channels = source_formats[meta[format_key]]
    width, height = meta["width"], meta["height"]
    data = np.fromfile(path.with_suffix(".raw"), np.uint8)
    expected = meta["offset"] + (height - 1) * meta["row_pitch"] + width * channels * np.dtype(dtype).itemsize
    if data.size < expected:
        raise ValueError(f"Truncated readback: {path}")
    rows = np.ndarray((height, width * channels * np.dtype(dtype).itemsize), np.uint8,
                      buffer=data, offset=meta["offset"], strides=(meta["row_pitch"], 1)).copy()
    image = rows.view(dtype).reshape(height, width, channels).astype(np.float64)
    if dtype == np.uint8:
        image /= 255.0
    if format_key == "dxgi_format" and meta[format_key] in (87, 90, 91):
        image = image[..., [2, 1, 0, 3]]
    return meta, image


def png(image, role):
    if "motion" in role:
        magnitude = np.linalg.norm(image[..., :2], axis=-1)
        hsv = np.zeros((*magnitude.shape, 3), np.uint8)
        hsv[..., 0] = ((np.arctan2(image[..., 1], image[..., 0]) / (2 * np.pi) + 1) % 1 * 255).astype(np.uint8)
        hsv[..., 1] = 255
        hsv[..., 2] = np.clip(magnitude / 3.0 * 255, 0, 255).astype(np.uint8)
        return Image.fromarray(hsv, "HSV").convert("RGB")
    if "depth" in role:
        values = image[..., 0]
        visible = np.log1p(np.maximum(values, 0) * 1000)
        visible /= max(float(np.max(visible)), 1e-9)
        return Image.fromarray(np.clip(visible * 255, 0, 255).astype(np.uint8)).convert("RGB")
    color = image[..., :3]
    if "linear" in role:
        color = np.power(np.maximum(color, 0) / (1 + np.maximum(color, 0)), 1 / 2.2)
    return Image.fromarray(np.clip(color * 255, 0, 255).astype(np.uint8))


def projection(pose, width, height):
    if "projection" in pose:
        gl = np.asarray(pose["projection"]).reshape(4, 4)
        correction = np.eye(4)
        correction[2, 2], correction[3, 2] = -0.5, 0.5
        result = gl @ correction
        # This fixture uses a perspective camera with KEEP_HEIGHT. Camera3D's
        # script projection uses the output viewport, while RendererSceneCull
        # builds its raster projection from the rounded internal extent.
        result[0, 0] = result[1, 1] * height / width
        return result
    # Legacy capture from the same fixed validation scene. New captures record the
    # actual Camera3D projection, rather than relying on these pinned defaults.
    near, far = 0.05, 4000.0
    focal = 1 / np.tan(np.deg2rad(75.0) / 2)
    return np.array([[focal * height / width, 0, 0, 0], [0, focal, 0, 0],
                     [0, 0, near / (far - near), -1], [0, 0, near * far / (far - near), 0]])


def pixels(clip, width, height):
    ndc = clip[..., :3] / clip[..., 3:4]
    return np.stack(((ndc[..., 0] + 1) * width / 2 - 0.5,
                     (1 - ndc[..., 1]) * height / 2 - 0.5), axis=-1)


def material_centroid(image, roi):
    """Integrate blue material coverage, retaining subpixel reconstructed edges."""
    image = image[roi]
    weights = np.maximum(image[..., 2] - image[..., 0] - 0.005, 0)
    total = weights.sum()
    if not np.isfinite(weights).all() or total <= 1e-9:
        return None
    y, x = np.indices(weights.shape)
    return np.array([(x * weights).sum(), (y * weights).sum()]) / total


def fit_raster_jitter(meta, depth, linear_color, pose):
    """Infer raster jitter from known box planes, independently of SDK matrices."""
    height, width = depth.shape[:2]
    if linear_color.shape[:2] != (height, width):
        return None
    y, x = np.mgrid[:height, :width]
    camera = np.asarray(pose["camera"]).reshape(4, 4)
    box_inverse = np.linalg.inv(np.asarray(pose["box_world"]).reshape(4, 4))
    clip_to_local = np.linalg.inv(projection(pose, width, height)) @ camera @ box_inverse
    def local(jx, jy):
        clip = np.stack(((x + 0.5 - jx) / width * 2 - 1, 1 - (y + 0.5 - jy) / height * 2,
                         depth[..., 0], np.ones((height, width))), axis=-1)
        value = clip @ clip_to_local
        return (value / value[..., 3:4])[..., :3]
    base = local(0, 0)
    dx, dy = local(1, 0) - base, local(0, 1) - base
    visible_planes = np.sign((camera[3] @ box_inverse)[:3]) * 0.5
    distance = np.abs(base - visible_planes)
    face = distance.argmin(axis=-1)
    # Exclude seams where a small raster offset changes which face is nearest.
    mask = ((linear_color[..., 2] > linear_color[..., 0] * 2) & (linear_color[..., 2] > 0.3)
            & (np.partition(distance, 1, axis=-1)[..., 1] > 0.07) & (distance.min(axis=-1) < 0.03))
    axes = face[mask]
    if axes.size < 100:
        return None
    index = np.arange(axes.size)
    coefficients = np.stack((dx[mask][index, axes], dy[mask][index, axes]), axis=1)
    target = visible_planes[axes] - base[mask][index, axes]
    fitted, _, rank, _ = np.linalg.lstsq(coefficients, target, rcond=None)
    if rank != 2 or np.linalg.cond(coefficients) > 1e5:
        return None
    return {"pixels": int(axes.size), "fitted_pixels": fitted.tolist(),
            "metadata_error_pixels": float(np.linalg.norm(fitted - meta["jitter_pixels"])),
            "plane_residual_p95": float(np.percentile(np.abs(coefficients @ fitted - target), 95))}


def measure_motion(meta, depth, motion, color, current, previous):
    height, width = depth.shape[:2]
    y, x = np.mgrid[:height, :width]
    jitter = np.asarray(meta["jitter_pixels"])
    clip = np.stack(((x + 0.5 - jitter[0]) / width * 2 - 1, 1 - (y + 0.5 - jitter[1]) / height * 2,
                     depth[..., 0], np.ones((height, width))), axis=-1)
    current_projection = projection(current, width, height)
    previous_projection = projection(previous, width, height)
    camera = np.asarray(current["camera"]).reshape(4, 4)
    previous_camera = np.asarray(previous["camera"]).reshape(4, 4)
    world = clip @ np.linalg.inv(current_projection) @ camera
    world /= world[..., 3:4]
    valid = (depth[..., 0] > 1e-6) & np.isfinite(motion[..., :2]).all(axis=-1)
    box = np.asarray(current["box_world"]).reshape(4, 4)
    local = world @ np.linalg.inv(box)
    blue = valid & (np.max(np.abs(local[..., :3]), axis=-1) < 0.503)
    floor = valid & ~blue & (np.abs(world[..., 1] + 0.6) < 0.03)
    previous_world = world.copy()
    if np.any(blue):
        previous_box = np.asarray(previous["box_world"]).reshape(4, 4)
        previous_world[blue] = world[blue] @ np.linalg.inv(box) @ previous_box
    current_pixels = np.stack((x - jitter[0], y - jitter[1]), axis=-1)
    expected = pixels(previous_world @ np.linalg.inv(previous_camera) @ previous_projection, width, height) - current_pixels
    native = pixels(clip @ np.asarray(meta["clip_to_previous_clip"]).reshape(4, 4), width, height) - current_pixels
    error = np.linalg.norm(motion[..., :2] - expected, axis=-1)
    native_error = np.linalg.norm(motion[..., :2] - native, axis=-1)
    camera_expected = pixels(world @ np.linalg.inv(previous_camera) @ previous_projection, width, height) - current_pixels
    camera_metadata_error = np.linalg.norm(native - camera_expected, axis=-1)

    def stats(mask):
        if not np.any(mask):
            return {"pixels": 0}
        return {"pixels": int(mask.sum()), "mean_motion_pixels": float(np.linalg.norm(motion[..., :2], axis=-1)[mask].mean()),
                "zero_motion_fraction": float((np.linalg.norm(motion[..., :2], axis=-1)[mask] < 1e-5).mean()),
                "ground_truth_error_p50": float(np.median(error[mask])),
                "ground_truth_error_p95": float(np.percentile(error[mask], 95)),
                "ground_truth_error_p99": float(np.percentile(error[mask], 99)),
                "ground_truth_error_max": float(error[mask].max()),
                "camera_metadata_error_p95": float(np.percentile(camera_metadata_error[mask], 95)),
                "native_camera_reprojection_error_p50": float(np.median(native_error[mask]))}

    translation_error = float(np.max(np.abs(np.asarray(meta["view_to_world"]).reshape(4, 4)[3, :3] - camera[3, :3])))
    return {"object": stats(blue), "floor": stats(floor), "camera_translation_error": translation_error,
            "nonfinite_motion_pixels": int((~np.isfinite(motion[..., :2]).all(axis=-1)).sum()),
            "floor_world_y_p50": float(np.median(world[..., 1][floor])) if np.any(floor) else None}


def write_report(output, summary, capture_info):
    """Keep failed images just as inspectable as passing images."""
    escape = html.escape
    def number(value):
        return f"{value:.4f}" if value is not None else "n/a"
    rows, images = [], []
    for frame in summary["frames"]:
        identity = frame["frame"]
        rows.append("<tr>" + "".join(f"<td>{cell}</td>" for cell in (
            identity, number(frame["object"].get("ground_truth_error_p95")),
            number(frame["floor"].get("ground_truth_error_p95")),
            number(frame.get("source_current_material_centroid_error_pixels")),
            number(frame.get("generated_midpoint_material_centroid_error_pixels")),
            number(frame.get("interpolation_residual_after_source_displacement_pixels")))) + "</tr>")
        sheet = output / f"frame-{identity}-generated-comparison.png"
        panels = []
        if sheet.exists():
            panels.append(f'<a href="{escape(sheet.name)}"><img class="comparison" src="{escape(sheet.name)}" alt="Source, actual generated image, independent midpoint reference, and error"></a>')
        guides = []
        for role, title in (("fg-motion-pixels", "Motion: previous minus current pixels"), ("fg-depth", "Depth: logarithmic preview")):
            for path in output.glob(f"*-{identity}-{role}-*.png"):
                guides.append(f'<figure><a href="{escape(path.name)}"><img src="{escape(path.name)}" alt="{escape(title)}"></a><figcaption>{escape(title)}</figcaption></figure>')
        images.append(f'<section><h2>Source frame {identity}</h2>{"".join(panels)}<div class="guides">{"".join(guides)}</div></section>')
    failed = "".join(f"<li>{escape(message)}</li>" for message in summary["failures"])
    state = "PASS" if summary["passed"] else "FAIL"
    document = f'''<!doctype html><html lang="en"><meta charset="utf-8">
<title>Godot actual generated-frame report: {state}</title>
<style>body{{font:15px system-ui;background:#10151d;color:#e4ebf4;margin:24px}}a{{color:#80c9ff}}
table{{border-collapse:collapse}}td,th{{padding:8px;border:1px solid #465263;text-align:right}}
.comparison{{width:100%;height:auto}}.guides{{display:flex;gap:16px}}figure{{margin:12px 0;max-width:48%}}
figure img{{max-width:100%;height:auto}}code{{overflow-wrap:anywhere}}li{{color:#ffb6ad}}</style>
<h1>{state}: actual generated-frame comparison</h1>
<p>Godot {".NET" if capture_info.get("mono") else "standard"}; {escape(str(capture_info.get("mode", "unknown")))} motion;
SR {escape(str(capture_info.get("sr_backend", "unknown")))}; FSR3 frame generation.</p>
<p>This box/floor fixture checks temporal inputs and image displacement. It does not establish broad scene quality or latency.
Images include the actual SDK-generated output. Click an image for full resolution.</p><ul>{failed}</ul>
<p>All errors below are in output pixels, except vector errors, which use render pixels.
Source displacement and interpolation residual are diagnostics; they do not relax the midpoint image gate.</p>
<table><tr><th>Source frame</th><th>Object motion p95</th><th>Floor motion p95</th><th>Source SR displacement</th>
<th>Generated midpoint error</th><th>Residual after source displacement</th></tr>{"".join(rows)}</table>
<p>Motion hue encodes direction, brightness encodes magnitude and saturates at 3 render pixels.
Black is zero motion. Depth previews use a logarithmic scale; raw values remain in the readbacks.</p>
{"".join(images)}<p><a href="analysis.json">Numerical report</a> · <a href="../capture-info.json">Capture fingerprints</a>
· <a href="../result.json">Scene poses and runtime status</a></p>
<p>Engine SHA256: <code>{escape(capture_info.get("engine_sha256", "unknown"))}</code><br>
Native SHA256: <code>{escape(capture_info.get("native_sha256", "unknown"))}</code></p></html>'''
    (output / "report.html").write_text(document, encoding="utf-8")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("capture", type=Path)
    parser.add_argument("--reference", type=Path)
    parser.add_argument("--variant", default="", help="Keep an alternate reference experiment in its own decoded directory")
    parser.add_argument("--check", action="store_true", help="Fail on missing captures or incorrect temporal inputs")
    args = parser.parse_args()
    report = json.loads((args.capture / "result.json").read_text())
    poses = {p["process_frame"]: p for p in report["poses"]}
    frames = {}
    if args.variant and not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]*", args.variant):
        parser.error("Use a simple analysis variant name")
    output = args.capture / ("decoded-" + args.variant if args.variant else "decoded")
    output.mkdir(exist_ok=True)
    for path in sorted((args.capture / "frames").glob("*.json")):
        meta, data = load_image(path)
        frames.setdefault(meta["frame"], {})[meta["role"]] = (meta, data)
        png(data, meta["role"]).save(output / (path.stem + ".png"))
    references = {}
    if args.reference:
        for path in (args.reference / "frames").glob("*fg-hudless-color*.json"):
            meta, data = load_image(path)
            references[meta["frame"]] = data
    findings = []
    failures = []
    if not report.get("passed"):
        failures.append("The engine fixture reported failure")
    requested_sr = report.get("sr_backend", 0)
    actual_sr = report.get("status", {}).get("active_sr", 0)
    if not report.get("reference") and (actual_sr == 0 if requested_sr == 7 else actual_sr != requested_sr):
        failures.append("The requested SR provider did not execute")
    for frame, resources in sorted(frames.items()):
        required = {"fg-depth", "fg-motion-pixels", "fg-hudless-color"}
        if not required.issubset(resources) or frame not in poses or frame - 1 not in poses:
            continue
        meta, depth = resources["fg-depth"]
        identities = {(item[0]["session"], item[0]["view"], item[0]["generation"]) for item in resources.values()}
        if len(identities) != 1:
            failures.append(f"Frame {frame}: guide/output session, view or resource generation does not match")
        motion = resources["fg-motion-pixels"][1]
        color = resources["fg-hudless-color"][1]
        metrics = measure_motion(meta, depth, motion, color, poses[frame], poses[frame - 1])
        if "sr-linear-color" in resources:
            jitter = fit_raster_jitter(meta, depth, resources["sr-linear-color"][1], poses[frame])
            if jitter is not None:
                metrics["raster_jitter_fit"] = jitter
        metrics["frame"] = frame
        if "fg-generated-before-ui" in resources:
            generated = resources["fg-generated-before-ui"][1]
            metrics["actual_generated_frame_captured"] = True
            panels = [("HUD-less base input", color), ("Actual generated output", generated)]
            reference = references.get(frame * 2 - 1)
            if reference is not None and reference.shape != generated.shape:
                height, width = generated.shape[:2]
                scale_y, scale_x = reference.shape[0] // height, reference.shape[1] // width
                if scale_y > 0 and scale_x > 0 and reference.shape[:2] == (height * scale_y, width * scale_x):
                    reference = reference.reshape(height, scale_y, width, scale_x, reference.shape[2]).mean(axis=(1, 3))
            if reference is not None and reference.shape == generated.shape:
                delta = np.abs(generated[..., :3] - reference[..., :3])
                metrics["generated_midpoint_rgb_error_mean"] = float(delta.mean())
                metrics["generated_midpoint_rgb_error_p95"] = float(np.percentile(delta, 95))
                # Inspect geometry edges instead of diluting errors over a flat
                # background. The HUD occupies the top 110 rows in this fixture.
                edges = np.zeros(reference.shape[:2], dtype=bool)
                rgb = reference[..., :3]
                edges[1:] |= np.max(np.abs(rgb[1:] - rgb[:-1]), axis=-1) > 0.03
                edges[:, 1:] |= np.max(np.abs(rgb[:, 1:] - rgb[:, :-1]), axis=-1) > 0.03
                for _ in range(3):
                    expanded = edges.copy()
                    expanded[1:] |= edges[:-1]
                    expanded[:-1] |= edges[1:]
                    expanded[:, 1:] |= edges[:, :-1]
                    expanded[:, :-1] |= edges[:, 1:]
                    edges = expanded
                edges[:110] = False
                metrics["generated_midpoint_edge_pixels"] = int(edges.sum())
                metrics["generated_midpoint_edge_rgb_error_mean"] = float(delta[edges].mean())
                metrics["base_midpoint_edge_rgb_error_mean"] = float(np.abs(color[..., :3] - rgb)[edges].mean())
                def box_pixels(image):
                    return ((image[..., 2] > image[..., 0] * 1.4) &
                            (image[..., 1] > image[..., 0] * 1.3) & (image[..., 2] > 0.2))
                reference_box = box_pixels(reference)
                generated_box = box_pixels(generated)
                base_box = box_pixels(color)
                if reference_box.any() and generated_box.any() and base_box.any():
                    reference_center = np.mean(np.argwhere(reference_box), axis=0)
                    metrics["generated_midpoint_object_centroid_error_pixels"] = float(np.linalg.norm(
                        np.mean(np.argwhere(generated_box), axis=0) - reference_center))
                    metrics["base_midpoint_object_centroid_error_pixels"] = float(np.linalg.norm(
                        np.mean(np.argwhere(base_box), axis=0) - reference_center))
                    yy, xx = np.where(reference_box)
                    roi = np.s_[max(0, yy.min() - 6):yy.max() + 7, max(0, xx.min() - 6):xx.max() + 7]
                    metrics["generated_midpoint_object_rgb_error_mean"] = float(delta[roi].mean())
                    metrics["base_midpoint_object_rgb_error_mean"] = float(np.abs(color[..., :3] - rgb)[roi].mean())
                    reference_centroid = material_centroid(reference, roi)
                    generated_centroid = material_centroid(generated, roi)
                    base_centroid = material_centroid(color, roi)
                    if reference_centroid is not None and generated_centroid is not None and base_centroid is not None:
                        metrics["generated_midpoint_material_centroid_error_pixels"] = float(np.linalg.norm(generated_centroid - reference_centroid))
                        metrics["base_midpoint_material_centroid_error_pixels"] = float(np.linalg.norm(base_centroid - reference_centroid))
                        # Distinguish SR reconstruction displacement already present
                        # in the source from displacement introduced by interpolation.
                        # This diagnostic does not relax the end-to-end image gate.
                        current_reference = references.get(frame * 2)
                        if current_reference is not None:
                            height, width = generated.shape[:2]
                            sy, sx = current_reference.shape[0] // height, current_reference.shape[1] // width
                            if current_reference.shape != generated.shape and sy > 0 and sx > 0 and current_reference.shape[:2] == (height * sy, width * sx):
                                current_reference = current_reference.reshape(height, sy, width, sx, current_reference.shape[2]).mean(axis=(1, 3))
                            if current_reference.shape == generated.shape:
                                current_centroid = material_centroid(current_reference, roi)
                                if current_centroid is not None:
                                    source_error = base_centroid - current_centroid
                                    generated_error = generated_centroid - reference_centroid
                                    metrics["source_current_material_centroid_error_vector_pixels"] = source_error.tolist()
                                    metrics["source_current_material_centroid_error_pixels"] = float(np.linalg.norm(source_error))
                                    metrics["generated_midpoint_material_centroid_error_vector_pixels"] = generated_error.tolist()
                                    metrics["interpolation_residual_after_source_displacement_pixels"] = float(np.linalg.norm(generated_error - source_error))
                panels += [("Rendered half-step reference", reference), ("Absolute error x4", delta * 4)]
            sheet = Image.new("RGB", (color.shape[1] * len(panels), color.shape[0] + 24), "#121820")
            draw = ImageDraw.Draw(sheet)
            for i, (title, data) in enumerate(panels):
                sheet.paste(png(data, title), (i * color.shape[1], 24))
                draw.text((i * color.shape[1] + 8, 6), title, fill="white")
            sheet.save(output / f"frame-{frame}-generated-comparison.png")
        findings.append(metrics)
        if metrics["nonfinite_motion_pixels"]:
            failures.append(f"Frame {frame}: motion contains nonfinite values")
        for region in ("object", "floor"):
            values = metrics[region]
            if values["pixels"] < 100 or values["ground_truth_error_p95"] > 0.03:
                failures.append(f"Frame {frame}: {region} motion disagrees with rendered geometry")
            if values.get("camera_metadata_error_p95", float("inf")) > 0.01:
                failures.append(f"Frame {frame}: {region} camera reprojection metadata is incorrect")
        if metrics["camera_translation_error"] > 1e-4:
            failures.append(f"Frame {frame}: camera transform serialization is incorrect")
        if metrics.get("raster_jitter_fit", {}).get("metadata_error_pixels", 0) > 0.02:
            failures.append(f"Frame {frame}: jitter disagrees with the rendered box planes")
        if not report.get("reference") and not metrics.get("actual_generated_frame_captured"):
            failures.append(f"Frame {frame}: actual generated output is missing")
        if args.reference and "generated_midpoint_edge_rgb_error_mean" not in metrics:
            failures.append(f"Frame {frame}: matching rendered midpoint reference is missing")
        if metrics.get("generated_midpoint_edge_rgb_error_mean", 0) > 0.12:
            failures.append(f"Frame {frame}: generated geometry edge error exceeds the fixture limit")
        if args.reference and metrics.get("generated_midpoint_material_centroid_error_pixels", float("inf")) > 0.35:
            failures.append(f"Frame {frame}: generated object misses its independently rendered midpoint")
    if len(findings) < 4:
        failures.append("Fewer than four complete matching guide/output frames")
    if report.get("probe", {}).get("capture_moving_pixels", 0):
        failures.append("The raw probe forced GPU submission; this run cannot validate ordinary scheduling")
    info_path = args.capture / "capture-info.json"
    capture_info = json.loads(info_path.read_text(encoding="utf-8-sig")) if info_path.exists() else {}
    if capture_info.get("reset_sr_history"):
        failures.append("Forced SR history resets are a diagnostic, not ordinary temporal-path acceptance")
    summary = {"capture": str(args.capture), "reference": str(args.reference) if args.reference else None,
               "passed": not failures, "failures": failures, "frames": findings}
    (output / "analysis.json").write_text(json.dumps(summary, indent=2) + "\n")
    write_report(output, summary, capture_info)
    print(json.dumps(summary, indent=2))
    if args.check and failures:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
