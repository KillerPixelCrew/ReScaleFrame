"""Check the view uniform buffer layout against the buffers the game actually bound.

The offsets below were not read off engine source, which is not available here, and not guessed
from a single buffer, which is how a plausible wrong answer gets written down. They come from
relationships that must hold between fields, checked across every captured buffer:

  ClipToPrevClip == ClipToTranslatedWorld * T(PrevPreViewTranslation - PreViewTranslation)
                                          * PrevTranslatedWorldToClip

That identity ties five offsets together at once. It holds to a millionth across captures where the
camera moved as much as 43 units, and a wrong offset for any one of them breaks it. The camera
basis is confirmed the same way, by having to equal the rows of ViewToTranslatedWorld.

Stock 4.18 order predicts all of this except one thing: `ViewToClipNoAA` does not exist in 4.18, so
everything after `ViewToClip` sits 0x40 earlier than a later engine would put it.

Usage: python3 tools/verify-view-layout.py <directory of *_cb4096.bin>
"""

import argparse
import struct
from pathlib import Path

# Offsets in bytes into the 4096 byte view uniform buffer.
LAYOUT = {
    "TranslatedWorldToClip": 0x000,
    "WorldToClip": 0x040,
    "TranslatedWorldToView": 0x080,
    "ViewToTranslatedWorld": 0x0C0,
    "TranslatedWorldToCameraView": 0x100,
    "CameraViewToTranslatedWorld": 0x140,
    "ViewToClip": 0x180,
    "ClipToView": 0x1C0,
    "ClipToTranslatedWorld": 0x200,
    "SVPositionToTranslatedWorld": 0x240,
    "ScreenToWorld": 0x280,
    "ScreenToTranslatedWorld": 0x2C0,
    "ViewForward": 0x300,
    "ViewUp": 0x310,
    "ViewRight": 0x320,
    "HMDViewNoRollUp": 0x330,
    "HMDViewNoRollRight": 0x340,
    "InvDeviceZToWorldZTransform": 0x350,
    "ScreenPositionScaleBias": 0x360,
    "WorldCameraOrigin": 0x370,
    "TranslatedWorldCameraOrigin": 0x380,
    "WorldViewOrigin": 0x390,
    "PreViewTranslation": 0x3A0,
    "PrevProjection": 0x3B0,
    "PrevViewProj": 0x3F0,
    "PrevViewRotationProj": 0x430,
    "PrevViewToClip": 0x470,
    "PrevClipToView": 0x4B0,
    "PrevTranslatedWorldToClip": 0x4F0,
    "PrevTranslatedWorldToView": 0x530,
    "PrevViewToTranslatedWorld": 0x570,
    "PrevTranslatedWorldToCameraView": 0x5B0,
    "PrevCameraViewToTranslatedWorld": 0x5F0,
    "PrevWorldCameraOrigin": 0x630,
    "PrevWorldViewOrigin": 0x640,
    "PrevPreViewTranslation": 0x650,
    "PrevInvViewProj": 0x660,
    "PrevScreenToTranslatedWorld": 0x6A0,
    "ClipToPrevClip": 0x6E0,
    "GlobalClippingPlane": 0x720,
    "ViewRectMin": 0x7E0,
    "ViewSizeAndInvSize": 0x7F0,
    "BufferSizeAndInvSize": 0x800,
}

VIEW_BUFFER_BYTES = 4096


def floats(path):
    data = path.read_bytes()[:VIEW_BUFFER_BYTES]
    if len(data) < VIEW_BUFFER_BYTES:
        return None
    return struct.unpack("<%df" % (VIEW_BUFFER_BYTES // 4), data)


def matrix(values, offset):
    base = offset // 4
    return [[values[base + row * 4 + column] for column in range(4)] for row in range(4)]


def vector(values, offset, count=3):
    base = offset // 4
    return [values[base + index] for index in range(count)]


def multiply(left, right):
    return [[sum(left[row][k] * right[k][column] for k in range(4)) for column in range(4)]
            for row in range(4)]


def translation(delta):
    result = [[1.0 if row == column else 0.0 for column in range(4)] for row in range(4)]
    result[3][0], result[3][1], result[3][2] = delta
    return result


def worst_difference(left, right):
    return max(abs(left[row][column] - right[row][column])
               for row in range(4) for column in range(4))


def is_perspective_view(values):
    """A 3D view rather than the orthographic one the interface renders through."""
    view_to_clip = matrix(values, LAYOUT["ViewToClip"])
    rotated = any(abs(matrix(values, LAYOUT["TranslatedWorldToView"])[row][row] - 1.0) > 1e-3
                  for row in range(3))
    return rotated and abs(view_to_clip[2][3] - 1.0) < 1e-3


def check(values):
    """Every relationship that has to hold if the offsets are right. Returns failures."""
    failures = []

    # The one that ties the previous-frame block to the current one.
    pre_view = vector(values, LAYOUT["PreViewTranslation"])
    prev_pre_view = vector(values, LAYOUT["PrevPreViewTranslation"])
    delta = [prev_pre_view[k] - pre_view[k] for k in range(3)]
    computed = multiply(multiply(matrix(values, LAYOUT["ClipToTranslatedWorld"]),
                                 translation(delta)),
                        matrix(values, LAYOUT["PrevTranslatedWorldToClip"]))
    difference = worst_difference(computed, matrix(values, LAYOUT["ClipToPrevClip"]))
    if difference > 0.01:
        failures.append(f"ClipToPrevClip does not follow from its parts (off by {difference:.5f})")

    # Translated world is world shifted so the camera sits at the origin.
    camera = vector(values, LAYOUT["WorldCameraOrigin"])
    if max(abs(camera[k] + pre_view[k]) for k in range(3)) > 1.0:
        failures.append("PreViewTranslation is not the negated camera position")
    translated_camera = vector(values, LAYOUT["TranslatedWorldCameraOrigin"])
    if max(abs(value) for value in translated_camera) > 1.0:
        failures.append("TranslatedWorldCameraOrigin is not at the origin")

    # The camera basis has to be the rows of ViewToTranslatedWorld, in Unreal's view space order
    # where Z is forward.
    view_to_world = matrix(values, LAYOUT["ViewToTranslatedWorld"])
    for name, row in (("ViewRight", 0), ("ViewUp", 1), ("ViewForward", 2)):
        stored = vector(values, LAYOUT[name])
        if max(abs(stored[k] - view_to_world[row][k]) for k in range(3)) > 1e-4:
            failures.append(f"{name} is not row {row} of ViewToTranslatedWorld")
        length = sum(value * value for value in stored) ** 0.5
        if abs(length - 1.0) > 1e-3:
            failures.append(f"{name} is not a unit vector (length {length:.5f})")

    # ViewToClip and ClipToView must invert each other.
    product = multiply(matrix(values, LAYOUT["ViewToClip"]), matrix(values, LAYOUT["ClipToView"]))
    identity = [[1.0 if row == column else 0.0 for column in range(4)] for row in range(4)]
    if worst_difference(product, identity) > 0.01:
        failures.append("ViewToClip and ClipToView are not inverses")

    # Sizes come in a value and its reciprocal.
    for name in ("ViewSizeAndInvSize", "BufferSizeAndInvSize"):
        size = vector(values, LAYOUT[name], 4)
        for axis in range(2):
            if size[axis] <= 0 or abs(size[axis] * size[axis + 2] - 1.0) > 1e-3:
                failures.append(f"{name} axis {axis} is not a size and its reciprocal")
    return failures


def describe(values):
    view_to_clip = matrix(values, LAYOUT["ViewToClip"])
    import math
    vertical_fov = 2.0 * math.atan(1.0 / view_to_clip[1][1]) if view_to_clip[1][1] else 0.0
    aspect = view_to_clip[1][1] / view_to_clip[0][0] if view_to_clip[0][0] else 0.0
    view_size = vector(values, LAYOUT["ViewSizeAndInvSize"], 4)
    buffer_size = vector(values, LAYOUT["BufferSizeAndInvSize"], 4)
    return (f"view {view_size[0]:.0f}x{view_size[1]:.0f} in buffer "
            f"{buffer_size[0]:.0f}x{buffer_size[1]:.0f}, "
            f"vertical fov {math.degrees(vertical_fov):.1f} deg, aspect {aspect:.4f}, "
            f"near {view_to_clip[3][2]:.3f}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--pattern", default="*_cb4096.bin")
    arguments = parser.parse_args()

    paths = sorted(arguments.directory.glob(arguments.pattern))
    if not paths:
        print(f"no buffers matching {arguments.pattern} in {arguments.directory}")
        return 2

    checked = 0
    failed = 0
    for path in paths:
        values = floats(path)
        if values is None or not is_perspective_view(values):
            continue
        checked += 1
        failures = check(values)
        status = "ok" if not failures else "FAILED"
        print(f"{path.name}: {status}  {describe(values)}")
        for failure in failures:
            print(f"    {failure}")
            failed += 1

    print(f"\n{checked} perspective views checked, {failed} failures")
    if checked == 0:
        print("no perspective views found; the retained buffer may be an interface view")
        return 2
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
