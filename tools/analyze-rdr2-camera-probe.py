#!/usr/bin/env python3
"""Inspect RSFCAM1 snapshots without treating guessed offsets as proven camera semantics."""
import argparse
import json
import math
from pathlib import Path
import struct

import numpy as np


def read_sample(path):
    data = path.read_bytes()
    if len(data) not in (2416, 2568) or data[:8] != b"RSFCAM1\0":
        raise ValueError(f"Invalid camera snapshot: {path}")
    version, size, capture, index, provider, thread = struct.unpack_from("<6I", data, 8)
    if version not in (1, 2) or size != len(data):
        raise ValueError(f"Unsupported camera snapshot version: {path}")
    tick, qpc, base, tls, view, context, parameters, caller = struct.unpack_from("<8Q", data, 32)
    frame, tls_index, valid_view, valid_parameters = struct.unpack_from("<4I", data, 96)
    if not valid_view or not valid_parameters:
        raise ValueError(f"Unreadable source in snapshot: {path}")
    raw = data[112:2160]
    parms = data[2160:]
    floats = np.frombuffer(raw, dtype="<f4").astype(np.float64)

    def mat(offset):
        return floats[offset // 4:offset // 4 + 16].reshape(4, 4)

    def scalar(offset):
        return float(floats[offset // 4])

    current_view, projection, camera_world = mat(0x40), mat(0x80), mat(0xc0)
    rejected_inverse_candidate = mat(0x520)
    matrices = {hex(o): mat(o).tolist() for o in [0, 0x40, 0x80, 0xc0, 0x100, 0x140, 0x180,
                                                   0x1c0, 0x240, 0x280, 0x2c0, 0x340, 0x380,
                                                   0x3c0, 0x410, 0x470, 0x4c0, 0x520]}
    finite = all(np.isfinite(m).all() for m in [current_view, projection, camera_world, rejected_inverse_candidate])
    if not finite:
        raise ValueError(f"Non-finite matrix candidate in {path}")

    def residual(matrix):
        return float(np.max(np.abs(matrix - np.eye(4))))

    near, far, fov_degrees = scalar(0x630), scalar(0x634), scalar(0x640)
    # Independently build an analytic inverse from the scalar camera properties, not np.linalg.inv(P).
    # Its agreement checks the hypothesised row-vector, -Z-forward reversed-depth projection layout.
    tan_half_fov = math.tan(math.radians(fov_degrees) / 2)
    aspect = scalar(0x66c)
    shift_x, shift_y = scalar(0x65c) + scalar(0x664), scalar(0x660) + scalar(0x668)
    z_scale = near / (far - near)
    z_translate = near * far / (far - near)
    analytic_inverse = np.array([[tan_half_fov * aspect, 0, 0, 0], [0, tan_half_fov, 0, 0],
                                 [0, 0, 0, 1 / z_translate],
                                 [shift_x * tan_half_fov * aspect, shift_y * tan_half_fov, -1, z_scale / z_translate]])
    jitter_offset = 0x38 if provider == 2 else 0x20
    jitter = struct.unpack_from("<2f", parms, jitter_offset)
    result = dict(file=path.name, capture=capture, sample=index, provider=provider, thread=thread,
                  tick=tick, qpc=qpc, module_base=hex(base), tls=hex(tls), view_pointer=hex(view),
                  context=hex(context), parameter_pointer=hex(parameters), caller_rva=hex(caller),
                  global_frame_counter_observation=frame, tls_index=tls_index, matrices=matrices,
                  near=near, far=far, vertical_fov_degrees=fov_degrees,
                  vertical_fov_radians=math.radians(fov_degrees),
                  projection_fov_radians=2 * math.atan(1 / projection[1, 1]),
                  jitter_parameter_candidate=jitter,
                  view_times_camera_identity_error=residual(current_view @ camera_world),
                  rejected_offset_0x520_inverse_error=residual(current_view @ projection @ rejected_inverse_candidate),
                  projection_times_independent_analytic_inverse_error=residual(projection @ analytic_inverse),
                  camera_basis_orthogonality_error=float(np.max(np.abs(camera_world[:3, :3] @ camera_world[:3, :3].T - np.eye(3)))),
                  camera_basis_determinant=float(np.linalg.det(camera_world[:3, :3])),
                  camera_position=camera_world[3, :3].tolist(),
                  projection_offsets=projection[2, :2].tolist(),
                  viewport=[scalar(o) for o in range(0x674, 0x68c, 4)],
                  projection_parameter_fields={hex(o):scalar(o) for o in range(0x630, 0x670, 4)})
    viewport_width, viewport_height = math.floor(scalar(0x67c) + 0.5), math.floor(scalar(0x680) + 0.5)
    result["viewport_jitter_pixel_error"] = [abs(-scalar(0x664) * viewport_width / 2 - jitter[0]),
                                           abs(scalar(0x668) * viewport_height / 2 - jitter[1])]
    if version == 2:
        width, height, job, draw_context = struct.unpack_from("<2I2Q", data, 2416)
        result.update(render_extent=[width, height], job=hex(job), draw_context=hex(draw_context))
        if width and height:
            result["jitter_pixel_error"] = [abs(-scalar(0x664) * width / 2 - jitter[0]),
                                           abs(scalar(0x668) * height / 2 - jitter[1])]
    if provider == 2:
        result["fsr2_parameter_near_far_fov"] = struct.unpack_from("<3f", parms, 0x64)
        result["fsr2_parameter_render_size"] = struct.unpack_from("<2I", parms, 0x5c)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    paths = sorted(args.directory.glob("camera-*.bin"))
    if not paths:
        raise ValueError("No camera snapshots found")
    rows = [read_sample(path) for path in paths]
    summary = dict(samples=len(rows), providers=sorted({r["provider"] for r in rows}),
                   threads=sorted({r["thread"] for r in rows}),
                   max_view_inverse_error=max(r["view_times_camera_identity_error"] for r in rows),
                   max_projection_inverse_error=max(r["projection_times_independent_analytic_inverse_error"] for r in rows),
                   max_fov_error=max(abs(r["vertical_fov_radians"] - r["projection_fov_radians"]) for r in rows))
    summary["max_jitter_pixel_error"] = max(max(r["viewport_jitter_pixel_error"]) for r in rows)
    pairs = [(p,r) for p,r in zip(rows,rows[1:]) if p["capture"] == r["capture"] and
             r["sample"] == p["sample"] + 1 and p["file"].split("-")[1] == r["file"].split("-")[1]]
    if pairs:
        summary["max_previous_view_error"] = max(float(np.max(np.abs(np.array(r["matrices"]["0x140"]) -
                                                                 np.array(p["matrices"]["0x40"])))) for p,r in pairs)
    args.output.write_text(json.dumps(dict(summary=summary, samples=rows), indent=2, allow_nan=False) + "\n", encoding="utf-8")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
