# SPDX-License-Identifier: GPL-3.0-only
extends "res://scene.gd"

var runtime: RefCounted
var secondary: Window
var initial_session := 0
var changes: Array[Dictionary] = []
var baseline: Dictionary = {}

func _ready() -> void:
    super._ready()
    Engine.max_fps = 60
    DisplayServer.window_set_size(Vector2i(1280, 720))
    runtime = ClassDB.instantiate("ReScaleFrameRuntime") as RefCounted
    _select(1, true, 1)

func _select(backend: int, enabled: bool, count: int) -> void:
    var change := {"process_frame": frame, "backend": backend, "enabled": enabled, "count": count}
    changes.append(change)
    print("RSF_FG_LIFECYCLE_CHANGE ", JSON.stringify(change))
    _require(runtime.call("configure", {"sr_backend": 0, "quality": 1, "fg_backend": backend,
        "fg_mode": int(enabled), "generated_frames": count, "latency_mode": 1},
        get_viewport().get_viewport_rid()), "Lifecycle configuration refused: " + JSON.stringify(change))

func _sample(phase: String, backend: int, active: bool, count: int, size: Vector2i) -> void:
    var status: Dictionary = runtime.call("get_status")
    status["phase"] = phase
    status["process_frame"] = frame
    samples.append(status)
    print("RSF_FG_LIFECYCLE_SAMPLE ", JSON.stringify(status))
    _require(status.active_sr == 0, "SR unexpectedly activated in " + phase)
    _require(status.fg_backend == backend and status.fg_available == 1, "Wrong presentation owner in " + phase)
    _require(status.fg_active == int(active) and status.fg_result == 0, "FG state/result incorrect in " + phase)
    _require(status.output_width == size.x and status.output_height == size.y, "Wrong output extent in " + phase)
    _require(status.window_foreground == 1 and status.window_visible == 1, "Primary window lost foreground in " + phase)
    _require(status.pcl_available == 1 and status.latency_available == 1 and status.latency_mode == 1,
        "PCL/Reflex unavailable in " + phase)
    _require(status.sleep_calls > 8 and status.marker_calls > 8, "SDK latency events missing in " + phase)
    if initial_session == 0:
        initial_session = status.session_id
    _require(status.session_id == initial_session, "Root session changed in " + phase)
    if active:
        _require(status.generated_frames == count and status.max_generated_frames >= count and status.tagged_frames > 8,
            "Requested interpolation count or input leases missing in " + phase)
        if backend == 3:
            _require(status.generated_callbacks > 8, "Actual FFX callbacks missing in " + phase)
    if phase == "initial":
        baseline = status.duplicate(true)
    elif phase == "resumed":
        _require(status.tagged_frames > baseline.tagged_frames and status.source_frame_id > baseline.source_frame_id,
            "DLSS input processing did not resume")
    var image := get_viewport().get_texture().get_image()
    _require(image != null and not image.is_empty(), "Player image missing in " + phase)
    if image:
        _require(image.save_png(result_path.get_basename() + "-" + phase + ".png") == OK,
            "Cannot save lifecycle player image")

func _process(_delta: float) -> void:
    if runtime == null:
        return
    frame += 1
    moving_box.position.x = sin(frame / 20.0) * 1.5
    camera.position.x = sin(frame / 45.0) * 1.2
    camera.look_at(Vector3.ZERO)
    label.text = "DLSS Vulkan lifecycle: source %d" % frame
    if frame == 35:
        _sample("initial", 1, true, 1, Vector2i(1280, 720))
        _select(1, false, 1)
    elif frame == 55:
        _sample("off", 1, false, 1, Vector2i(1280, 720))
        _select(1, true, 1)
    elif frame == 85:
        _sample("resumed", 1, true, 1, Vector2i(1280, 720))
        DisplayServer.window_set_size(Vector2i(1360, 765))
    elif frame == 115:
        _sample("active-resize", 1, true, 1, Vector2i(1360, 765))
        _select(1, true, 3)
    elif frame == 145:
        _sample("count-three", 1, true, 3, Vector2i(1360, 765))
        _select(1, true, 1)
    elif frame == 175:
        _sample("count-one-return", 1, true, 1, Vector2i(1360, 765))
        _select(3, true, 1)
    elif frame == 205:
        _sample("ffx-owner", 3, true, 1, Vector2i(1360, 765))
        _select(1, true, 1)
    elif frame == 235:
        _sample("dlss-owner-return", 1, true, 1, Vector2i(1360, 765))
        get_viewport().gui_embed_subwindows = false
        secondary = Window.new()
        secondary.unfocusable = true
        secondary.size = Vector2i(300, 180)
        secondary.position = Vector2i(1500, 100)
        var text := Label.new()
        text.text = "Secondary native Vulkan chain"
        secondary.add_child(text)
        add_child(secondary)
        secondary.show()
    elif frame == 265:
        _require(is_instance_valid(secondary) and secondary.visible, "Secondary window was not live")
        _sample("secondary-live", 1, true, 1, Vector2i(1360, 765))
        secondary.size = Vector2i(340, 200)
        DisplayServer.window_set_size(Vector2i(1280, 720))
    elif frame == 295:
        _sample("secondary-primary-resize", 1, true, 1, Vector2i(1280, 720))
        secondary.queue_free()
    elif frame == 325:
        _sample("secondary-retired", 1, true, 1, Vector2i(1280, 720))
        var mono_status: Dictionary = {}
        if mono_test:
            mono_status = mono_test.call("validate_mono")
            _require(mono_status.get("passed", false), "C# facade failed")
        var report := {"passed": failures.is_empty(), "failures": failures, "changes": changes,
            "samples": samples, "mono": mono_status, "scope": "Lifecycle and strict API validation; no generated-image quality claim"}
        var file := FileAccess.open(result_path, FileAccess.WRITE)
        if file:
            file.store_string(JSON.stringify(report, "  "))
        else:
            _require(false, "Cannot save lifecycle report")
        runtime = null
        get_tree().quit(0 if failures.is_empty() else 2)
