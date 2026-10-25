# SPDX-License-Identifier: GPL-3.0-only
extends "res://scene.gd"

var runtime: RefCounted
var poses: Array[Dictionary] = []
var secondary: Window

func _matrix(value: Projection) -> Array[float]:
    var result: Array[float] = []
    for column in range(4):
        for row in range(4):
            result.append(value[column][row])
    return result

func _ready() -> void:
    super._ready()
    Engine.max_fps = 60
    runtime = ClassDB.instantiate("ReScaleFrameRuntime") as RefCounted
    _select(3, 1)

func _select(sr: int, fg: int) -> void:
    _require(runtime.call("configure", {"sr_backend": sr, "quality": 1, "fg_backend": 3,
        "fg_mode": fg, "generated_frames": 1, "latency_mode": 1}, get_viewport().get_viewport_rid()), "Vulkan FG configuration refused")

func _sample(phase: String, sr: int, active: bool) -> void:
    var image := get_viewport().get_texture().get_image()
    _require(image != null and not image.is_empty(), "Actual player image missing")
    if image:
        _require(image.save_png(result_path.get_basename() + "-" + phase + ".png") == OK, "Cannot save player PNG")
    var status: Dictionary = runtime.call("get_status")
    status["phase"] = phase
    samples.append(status)
    _require(status.active_sr == sr, "Incorrect active SR in " + phase)
    _require(status.fg_available == 1 and status.fg_backend == 3 and status.max_generated_frames == 1, "Native FSR3 FG owner missing")
    _require(status.fg_active == int(active), "Incorrect FG state in " + phase)
    _require(status.pcl_available == 1 and status.latency_available == 1 and status.latency_mode == 1 and status.sleep_calls > 8 and status.marker_calls > 8, "PCL/Reflex missing in " + phase)
    if active:
        _require(status.generated_callbacks > 8 and status.tagged_frames > 8 and status.fg_result == 0, "Actual generation callbacks missing in " + phase)

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
    label.text = "Vulkan FSR3 FG\nSR %s, FG %s, actual generated callbacks %s" % [status.get("active_sr", 0), status.get("fg_active", 0), status.get("generated_callbacks", 0)]
    if frame == 30:
        _require(runtime.call("request_latency_ping"), "FG latency ping could not be posted")
    elif frame == 45:
        _require(status.pcl_pings > 0, "FG latency ping did not reach the SDK")
        _sample("sr3-fg", 3, true)
    elif frame == 60:
        _select(0, 1)
    elif frame == 70 and not OS.has_environment("RSF_GODOT_SINGLE_WINDOW"):
        get_viewport().gui_embed_subwindows = false
        secondary = Window.new()
        secondary.unfocusable = true
        secondary.size = Vector2i(300, 180)
        secondary.position = Vector2i(1000, 100)
        var text := Label.new()
        text.text = "Secondary native Vulkan swapchain"
        secondary.add_child(text)
        add_child(secondary)
        secondary.show()
    elif frame == 75 and is_instance_valid(secondary):
        DisplayServer.window_set_size(Vector2i(720, 405))
    elif frame == 88:
        _sample("sr-off-fg", 0, true)
    elif frame == 90:
        if is_instance_valid(secondary):
            secondary.queue_free()
        _select(0, 0)
    elif frame == 105:
        _sample("off", 0, false)
    elif frame == 110:
        _select(3, 1)
        runtime.call("reset_history")
        DisplayServer.window_set_size(Vector2i(800, 450))
    elif frame == 145:
        _sample("resized", 3, true)
        _require(status.output_width == 800 and status.output_height == 450, "FG did not follow resize")
    elif frame == 165:
        var mono_status: Dictionary = {}
        if mono_test:
            mono_status = mono_test.call("validate_mono")
            _require(mono_status.get("passed", false), "C# facade failed")
        var file := FileAccess.open(result_path, FileAccess.WRITE)
        if file:
            file.store_string(JSON.stringify({"passed": failures.is_empty(), "failures": failures, "poses": poses, "samples": samples, "mono": mono_status}, "  "))
        runtime = null
        get_tree().quit(0 if failures.is_empty() else 2)
