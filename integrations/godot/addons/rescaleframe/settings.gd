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

@export_group("Native temporal post-upscale sharpening")
@export_enum("Off:0", "RCAS:1", "Depth RCAS:2", "Depth DAS:3", "Vendor FSR only:4") var sharpening_mode := 0
@export_range(0, 1, 0.001) var sharpening_sharpness := 0.4
@export_range(0, 1, 1) var sharpening_mas := 0
@export_range(-1, 1, 0.001) var sharpening_motion_sharpness := 0.2
@export_range(0, 100, 0.001) var sharpening_motion_threshold := 0.0
@export_range(0.01, 100, 0.001) var sharpening_motion_scale_limit := 10.0
@export_range(0, 1, 1) var sharpening_contrast_enabled := 0
@export_range(0, 2, 0.001) var sharpening_contrast := 0.0
@export_range(0, 600, 0.001) var sharpening_depth_scale := 35.0
@export_range(0, 1, 0.000001) var sharpening_depth_bias := 0.001
@export_range(0, 1, 1) var sharpening_clamp_output := 0
@export_range(0, 1, 1) var sharpening_debug := 0

func to_options() -> Dictionary:
    return {"sr_backend": sr_backend, "quality": quality, "dlss_preset": dlss_preset,
        "fg_backend": fg_backend, "fg_mode": fg_mode, "generated_frames": generated_frames,
        "latency_mode": latency_mode, "frame_limit_us": int(1000000.0 / frame_limit_fps) if frame_limit_fps > 0 else 0,
        "gameplay": gameplay,
        "sharpening_mode": sharpening_mode,
        "sharpening_sharpness": sharpening_sharpness,
        "sharpening_mas": sharpening_mas,
        "sharpening_motion_sharpness": sharpening_motion_sharpness,
        "sharpening_motion_threshold": sharpening_motion_threshold,
        "sharpening_motion_scale_limit": sharpening_motion_scale_limit,
        "sharpening_contrast_enabled": sharpening_contrast_enabled,
        "sharpening_contrast": sharpening_contrast,
        "sharpening_depth_scale": sharpening_depth_scale,
        "sharpening_depth_bias": sharpening_depth_bias,
        "sharpening_clamp_output": sharpening_clamp_output,
        "sharpening_debug": sharpening_debug}
