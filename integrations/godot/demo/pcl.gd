# SPDX-License-Identifier: GPL-3.0-only
extends Node3D

var runtime: RefCounted
var frames := 0
var failures: Array[String] = []
var result_path := ""
var initial_markers := 0

func _select_latency(mode: int) -> void:
    if not runtime.call("configure", {"sr_backend": 0, "fg_mode": 0, "latency_mode": mode}, get_viewport().get_viewport_rid()):
        failures.append("Camera-free latency mode %d was refused" % mode)

func _ready() -> void:
    Engine.max_fps = 60
    for arg: String in OS.get_cmdline_user_args():
        if arg.begins_with("--result="):
            result_path = arg.trim_prefix("--result=")
    runtime = ClassDB.instantiate("ReScaleFrameRuntime") as RefCounted
    _select_latency(1)
    var label := Label.new()
    label.text = "ReScaleFrame PCL without a camera or compositor"
    add_child(label)

func _process(_delta: float) -> void:
    frames += 1
    var status: Dictionary = runtime.call("get_status")
    if frames == 30:
        initial_markers = status.get("marker_calls", 0)
        if not runtime.call("request_latency_ping"):
            failures.append("PCL diagnostic message could not be posted")
    elif frames == 45:
        # Synthetic events traverse Godot's input processing and SDK marker path.
        var key := InputEventKey.new()
        key.keycode = KEY_F12
        key.pressed = true
        Input.parse_input_event(key)
        key = InputEventKey.new()
        key.keycode = KEY_F12
        Input.parse_input_event(key)
        Input.parse_input_event(InputEventMouseMotion.new())
        Input.parse_input_event(InputEventJoypadButton.new())
    elif frames == 60:
        _select_latency(2)
        DisplayServer.window_set_size(Vector2i(800, 450))
    elif frames == 80:
        if status.get("latency_mode", 0) != 2:
            failures.append("Reflex Boost did not take effect")
        _select_latency(0)
    elif frames == 100:
        if status.get("latency_mode", 1) != 0:
            failures.append("Reflex Off did not take effect")
        _select_latency(1)
    if frames == 120:
        if status.get("pcl_pings", 0) < 1 or status.get("marker_calls", 0) <= initial_markers or status.get("sleep_calls", 0) < 1:
            failures.append("Camera-free PCL did not reach the SDK")
        if status.get("marker_calls", 0) < (frames - 4) * 6:
            failures.append("Camera-free PCL missed simulation/render/Present marker pairs")
        if status.get("keyboard_inputs", 0) < 1 or status.get("mouse_inputs", 0) < 1 or status.get("controller_inputs", 0) < 1:
            failures.append("Synthetic Godot input did not reach the SDK")
        if status.get("evaluated_frames", 0) != 0 or status.get("active_sr", 0) != 0 or status.get("fg_active", 0) != 0:
            failures.append("Camera-free PCL unexpectedly depended on scene rendering")
        var report := {"passed": failures.is_empty(), "failures": failures, "status": status}
        var file := FileAccess.open(result_path, FileAccess.WRITE)
        if file:
            file.store_string(JSON.stringify(report, "  "))
        else:
            get_tree().quit(2)
            return
        get_tree().quit(0 if failures.is_empty() else 2)
