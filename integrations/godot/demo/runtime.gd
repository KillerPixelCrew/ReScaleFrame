# SPDX-License-Identifier: GPL-3.0-only
extends "res://scene.gd"

var bridge: Object
var phase := 0
var phase_frame := 0
var previous_evaluations := 0
var options: Dictionary = {}
var settled_counters: Dictionary = {}
var cases: Array[Dictionary] = [
    {"name": "dlss_sr_pcl", "sr_backend": 1, "fg_backend": 1, "fg_mode": 0, "generated_frames": 1},
    {"name": "fsr3_sr_fg", "sr_backend": 3, "fg_backend": 3, "fg_mode": 1, "generated_frames": 1},
    {"name": "xess_sr_fg", "sr_backend": 5, "fg_backend": 5, "fg_mode": 1, "generated_frames": 1},
    {"name": "independent_dlss_fg", "sr_backend": 0, "fg_backend": 1, "fg_mode": 1, "generated_frames": 1},
    {"name": "dlss_sr_mfg", "sr_backend": 1, "fg_backend": 1, "fg_mode": 1, "generated_frames": 3},
    {"name": "pcl_without_sr_fg", "sr_backend": 0, "fg_backend": 1, "fg_mode": 0, "generated_frames": 1}
]

func _ready() -> void:
    super._ready()
    Engine.max_fps = 60
    if not Engine.has_singleton("ReScaleFrame"):
        _require(false, "Engine bridge singleton is missing")
        _finish()
        return
    bridge = ClassDB.instantiate("ReScaleFrameRuntime") as RefCounted
    _require(bridge.call("get_status").get("engine_bridge_ready", false), "Native runtime did not initialize")
    _select_case()

func _select_case() -> void:
    phase_frame = 0
    settled_counters = {}
    DisplayServer.window_move_to_foreground()
    options = cases[phase].duplicate()
    options.erase("name")
    options["quality"] = 1
    options["latency_mode"] = 1
    options["gameplay"] = 1
    _require(bridge.call("configure", options, get_viewport().get_viewport_rid()), "Valid runtime configuration was refused")
    var invalid := options.duplicate()
    invalid["sr_backend"] = 99
    _require(not bridge.call("configure", invalid, get_viewport().get_viewport_rid()), "Invalid backend was accepted")

func _process(_delta: float) -> void:
    if bridge == null or phase >= cases.size():
        return
    frame += 1
    phase_frame += 1
    moving_box.position.x = sin(frame / 30.0) * 1.2
    camera.position.x = sin(frame / 90.0) * 0.6
    camera.look_at(Vector3.ZERO)
    var status: Dictionary = bridge.call("get_status")
    status["window_focused"] = DisplayServer.window_is_focused()
    label.text = "%s\nSR: %s, FG: %s, tagged: %s\nPCL: %s, markers: %s\nEvaluated: %s, refused: %s" % [
        cases[phase].name, status.get("active_sr", 0), status.get("fg_active", 0),
        status.get("tagged_frames", 0), status.get("pcl_available", 0),
        status.get("marker_calls", 0), status.get("evaluated_frames", 0), status.get("refused_frames", 0)]
    if phase == 1 and phase_frame == 40:
        DisplayServer.window_set_size(Vector2i(800, 450))
    if phase == 0 and phase_frame == 30:
        var key := InputEventKey.new()
        key.keycode = KEY_F9
        key.pressed = true
        Input.parse_input_event(key)
        var mouse := InputEventMouseMotion.new()
        mouse.relative = Vector2(1, 0)
        Input.parse_input_event(mouse)
        var controller := InputEventJoypadButton.new()
        controller.button_index = JOY_BUTTON_A
        controller.pressed = true
        Input.parse_input_event(controller)
        key = key.duplicate()
        key.pressed = false
        Input.parse_input_event(key)
        controller = controller.duplicate()
        controller.pressed = false
        Input.parse_input_event(controller)
    if phase_frame == 30:
        _require(bridge.call("request_latency_ping"), "Cannot post the registered PCL ping")
    if phase_frame == 80:
        settled_counters = status.duplicate()
    if phase_frame < 120:
        return
    status["phase"] = cases[phase].name
    samples.append(status)
    print("RSF_GODOT_RUNTIME_PHASE ", JSON.stringify(status))
    _require(status.get("active_sr", 0) == options.sr_backend, "Requested SR backend did not execute: " + cases[phase].name)
    if options.sr_backend == 0:
        _require(status.get("render_width", 0) == status.get("output_width", -1) and
            status.get("render_height", 0) == status.get("output_height", -1), "SR Off did not restore native resolution")
    _require(status.get("evaluated_frames", 0) > previous_evaluations, "No new SR/guide evaluations: " + cases[phase].name)
    previous_evaluations = status.get("evaluated_frames", 0)
    _require(status.get("pcl_available", 0) != 0 and status.get("marker_calls", 0) > 0, "PCL did not execute")
    _require(status.get("marker_calls", 0) > settled_counters.get("marker_calls", 0), "PCL markers stopped in this provider phase")
    _require(status.get("pcl_pings", 0) >= phase + 1, "The PCL message did not reach input sampling")
    _require(status.get("keyboard_inputs", 0) > 0 and status.get("mouse_inputs", 0) > 0 and status.get("controller_inputs", 0) > 0,
        "PCL did not observe keyboard, mouse and controller consumption")
    if options.fg_mode == 1:
        _require(status.get("fg_backend", 0) == options.fg_backend, "Requested FG provider did not become the owner")
        _require(status.get("fg_active", 0) != 0 and status.get("tagged_frames", 0) > 0, "Frame generation is inactive: " + cases[phase].name)
        var valid_flags: int = status.get("presentation_statistics_flags", 0)
        var generation_stable: bool = status.get("provider_generation", 0) == settled_counters.get("provider_generation", -1)
        var base_delta: int = status.get("source_frame_id", 0) - settled_counters.get("source_frame_id", 0)
        var presented_delta: int = status.get("total_presented", 0) - settled_counters.get("total_presented", 0)
        var generated_delta: int = status.get("generated_presented", 0) - settled_counters.get("generated_presented", 0)
        var confirmed: bool = generated_delta > 0 if valid_flags & 2 else presented_delta > base_delta and base_delta > 0
        _require(generation_stable and valid_flags & 1 and confirmed,
            "No confirmed generated presentation from valid provider counters: " + cases[phase].name)
    else:
        _require(status.get("fg_active", 0) == 0, "Frame generation remained active after Off")
    var image := get_viewport().get_texture().get_image()
    _require(image != null and not image.is_empty(), "Completed scene readback is empty")
    if image != null and not image.is_empty():
        _require(image.save_png(result_path.get_basename() + "-" + cases[phase].name + ".png") == OK,
            "Cannot save completed-scene validation image")
    if not failures.is_empty():
        _finish()
        return
    phase += 1
    if phase == cases.size():
        _finish()
    else:
        _select_case()
