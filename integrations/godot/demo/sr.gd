# SPDX-License-Identifier: GPL-3.0-only
extends "res://scene.gd"

var runtime: RefCounted
var poses: Array[Dictionary] = []
var initial_backend := 2
var second_backend := 3
var expect_sr_failure := false

func _matrix(value: Projection) -> Array[float]:
    var values: Array[float] = []
    for column in [value.x, value.y, value.z, value.w]:
        for component in [column.x, column.y, column.z, column.w]:
            values.append(component)
    return values

func _ready() -> void:
    super._ready()
    Engine.max_fps = 60
    runtime = ClassDB.instantiate("ReScaleFrameRuntime") as RefCounted
    for argument: String in OS.get_cmdline_user_args():
        if argument == "--expect-sr-failure":
            expect_sr_failure = true
        if argument.begins_with("--initial-sr="):
            initial_backend = argument.trim_prefix("--initial-sr=").to_int()
    _select(initial_backend)
    second_backend = 3 if initial_backend == 1 else 5 - initial_backend
    if RenderingServer.get_current_rendering_driver_name() == "vulkan":
        _require(not runtime.call("configure", {"sr_backend": 3, "fg_mode": 1, "fg_backend": 2}, get_viewport().get_viewport_rid()), "Unsupported Vulkan FG provider was accepted")
        _require(not runtime.call("configure", {"sr_backend": 3, "latency_mode": 3}, get_viewport().get_viewport_rid()), "Invalid Vulkan latency mode was accepted")

func _select(backend: int) -> void:
    _require(runtime.call("configure", {"sr_backend": backend, "quality": 1, "fg_mode": 0, "latency_mode": 0}, get_viewport().get_viewport_rid()), "SR configuration was refused")

func _snapshot(name: String) -> void:
    var image := get_viewport().get_texture().get_image()
    _require(image != null and not image.is_empty(), "The player image is missing")
    if image == null or image.is_empty():
        return
    _require(image.save_png(result_path.get_basename() + "-" + name + ".png") == OK, "Cannot save the actual player image")
    # The cyan object must survive SR rejection. Status counters alone cannot
    # distinguish a valid spatial fallback from an unwritten upscaled image.
    var object_samples := 0
    for y in range(64, image.get_height() - 16, 2):
        for x in range(16, image.get_width() - 16, 2):
            var pixel := image.get_pixel(x, y)
            if pixel.b > pixel.r * 1.5 and pixel.g > pixel.r * 1.5 and pixel.b > 0.2:
                object_samples += 1
    _require(object_samples > 100, "The rendered cyan object is missing from the player image")
    # Read status after the GPU readback has synchronized the last rendered source.
    var status: Dictionary = runtime.call("get_status")
    status["visible_object_samples"] = object_samples
    var metadata := FileAccess.open(result_path.get_basename() + "-" + name + ".json", FileAccess.WRITE)
    if metadata:
        metadata.store_string(JSON.stringify(status, "  "))
    samples.append(status)

func _check_provider(status: Dictionary, backend: int) -> void:
    if expect_sr_failure:
        _require(status.active_sr == 0 and status.sr_result != 0 and status.refused_frames > 20 and status.evaluated_frames == 0,
            "Missing SDK must retain the SR failure while producing spatial output")
    else:
        _require(status.active_sr == backend and status.evaluated_frames > 20, "SR provider did not execute")

func _process(_delta: float) -> void:
    if runtime == null:
        return
    frame += 1
    moving_box.position.x = sin(frame / 20.0) * 1.5
    camera.position.x = sin(frame / 45.0) * 1.2
    camera.look_at(Vector3.ZERO)
    poses.append({"process_frame": frame, "camera": _matrix(Projection(camera.global_transform)),
        "projection": _matrix(camera.get_camera_projection()), "box_world": _matrix(Projection(moving_box.global_transform))})
    var status: Dictionary = runtime.call("get_status")
    label.text = "Godot native SR: %s\nEvaluated %s, refused %s" % [status.get("active_sr", 0), status.get("evaluated_frames", 0), status.get("refused_frames", 0)]
    if frame == 45:
        _check_provider(status, initial_backend)
        _snapshot("sr%s" % initial_backend)
    elif frame == 50 and initial_backend == 1:
        _require(runtime.call("configure", {"sr_backend": 1, "quality": 1, "dlss_preset": 11, "fg_mode": 0}, get_viewport().get_viewport_rid()), "DLSS K model request was refused")
    elif frame == 58 and initial_backend == 1:
        _check_provider(status, 1)
        _require(status.requested_dlss_preset == 11, "DLSS K request was not retained")
        _snapshot("sr1-preset-k")
    elif frame == 60:
        _select(second_backend)
    elif frame == 85:
        _check_provider(status, second_backend)
        _snapshot("sr%s" % second_backend)
    elif frame == 90:
        DisplayServer.window_set_size(Vector2i(800, 450))
        runtime.call("reset_history")
    elif frame == 125:
        _check_provider(status, second_backend)
        _require(status.output_width == 800 and status.output_height == 450, "SR did not follow resize/reset")
        _snapshot("resized")
    elif frame == 150:
        var mono_status: Dictionary = {}
        if mono_test:
            mono_status = mono_test.call("validate_sr_mono")
            _require(mono_status.get("passed", false), "C# SR facade validation failed")
        var file := FileAccess.open(result_path, FileAccess.WRITE)
        if file:
            file.store_string(JSON.stringify({"passed": failures.is_empty(), "failures": failures, "samples": samples, "mono": mono_status, "poses": poses, "expected_sr_failure": expect_sr_failure}, "  "))
        else:
            get_tree().quit(2)
            return
        runtime = null
        get_tree().quit(0 if failures.is_empty() else 2)
