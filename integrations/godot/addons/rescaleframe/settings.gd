# SPDX-License-Identifier: GPL-3.0-only
class_name ReScaleFrameSettings
extends Resource

@export_enum("Off:0", "DLSS:1", "FSR 2:2", "FSR 3:3", "FSR 4:4", "XeSS:5", "FSR 1:6", "Auto:7") var sr_backend := 7
@export_enum("Native:0", "Quality:1", "Balanced:2", "Performance:3", "Ultra Performance:4", "Ultra Quality:5") var quality := 1
@export_range(0, 15, 1) var dlss_preset := 0
@export_enum("DLSS:1", "FSR 3:3", "FSR 4:4", "XeSS:5", "Auto:7") var fg_backend := 7
@export_enum("Off:0", "Fixed:1") var fg_mode := 0
@export_range(1, 15, 1) var generated_frames := 1
@export_enum("Off:0", "On:1", "Boost:2") var latency_mode := 0
@export_range(0, 1000, 1) var frame_limit_fps := 0
@export var gameplay := true

func to_options() -> Dictionary:
    return {"sr_backend": sr_backend, "quality": quality, "dlss_preset": dlss_preset,
        "fg_backend": fg_backend, "fg_mode": fg_mode, "generated_frames": generated_frames,
        "latency_mode": latency_mode, "frame_limit_us": int(1000000.0 / frame_limit_fps) if frame_limit_fps > 0 else 0,
        "gameplay": gameplay}
