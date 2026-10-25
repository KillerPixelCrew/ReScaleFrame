# SPDX-License-Identifier: GPL-3.0-only
extends "res://scene.gd"

var control: RefCounted
var mode := "combined"
var reference := false
var reference_msaa := false
var raw_probe := false
var sr_backend := 0
var animation_offset := 0.0
var poses: Array[Dictionary] = []

func _ready() -> void:
    super._ready()
    Engine.max_fps = 60
    for arg: String in OS.get_cmdline_user_args():
        if arg.begins_with("--mode="):
            mode = arg.trim_prefix("--mode=")
        if arg == "--reference":
            reference = true
        if arg == "--reference-msaa":
            reference_msaa = true
        if arg == "--raw-probe":
            raw_probe = true
        if arg.begins_with("--sr="):
            sr_backend = arg.trim_prefix("--sr=").to_int()
        if arg.begins_with("--animation-offset="):
            animation_offset = arg.trim_prefix("--animation-offset=").to_float()
    if reference:
        Engine.max_fps = 120
        if reference_msaa:
            get_viewport().msaa_3d = Viewport.MSAA_8X
        else:
            DisplayServer.window_set_size(Vector2i(1280, 720))
    control = ClassDB.instantiate("ReScaleFrameRuntime") as RefCounted
    _require(control.call("configure", {"sr_backend": 0 if reference else sr_backend, "fg_backend": 3, "fg_mode": 0 if reference else 1,
        "generated_frames": 1, "latency_mode": 0}, get_viewport().get_viewport_rid()), "Visual capture configuration failed")
    DisplayServer.window_move_to_foreground()

func _process(_delta: float) -> void:
    if control == null:
        return
    frame += 1
    var animation_frame := (frame * 0.5 if reference else float(frame)) - animation_offset
    moving_box.position.x = sin(animation_frame / 20.0) * 1.5 if mode != "camera" else 0.0
    camera.position.x = sin(animation_frame / 45.0) * 1.2 if mode != "object" else 0.0
    camera.look_at(Vector3.ZERO)
    var status: Dictionary = control.call("get_status")
    label.text = "Godot visual capture: %s\nSR %s, FSR3 FG\nInput frames: %s / tagged: %s" % [mode, sr_backend, status.get("evaluated_frames", 0), status.get("tagged_frames", 0)]
    poses.append({"process_frame": frame, "animation_frame": animation_frame, "camera": _matrix(Projection(camera.global_transform)),
        "projection": _matrix(camera.get_camera_projection()),
        "box_world": _matrix(Projection(moving_box.global_transform))})
    if frame == 60:
        var image := get_viewport().get_texture().get_image()
        image.save_png(result_path.get_basename() + "-base.png")
    if raw_probe and frame == 32:
        effect.call("request_motion_capture")
    if frame >= 90:
        var mono_status: Dictionary = {}
        if mono_test:
            mono_status = mono_test.call("validate_mono")
            _require(mono_status.get("passed", false), "C# facade failed during visual capture")
        var report := {"passed": failures.is_empty(), "failures": failures, "mode": mode, "reference": reference, "reference_msaa": reference_msaa, "poses": poses,
            "status": status, "probe": effect.call("get_status"), "mono": mono_status, "sr_backend": 0 if reference else sr_backend}
        var file := FileAccess.open(result_path, FileAccess.WRITE)
        if file:
            file.store_string(JSON.stringify(report, "  "))
        else:
            _require(false, "Cannot save visual capture manifest")
        print("RSF_GODOT_VISUAL ", JSON.stringify(status))
        control = null
        get_tree().quit(0 if failures.is_empty() else 2)

func _matrix(value: Projection) -> Array[float]:
    var result: Array[float] = []
    for column in range(4):
        for row in range(4):
            result.append(value[column][row])
    return result
