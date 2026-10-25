# SPDX-License-Identifier: GPL-3.0-only
extends Node3D

var runtime: RefCounted
var frames := 0
var failures: Array[String] = []
var result_path := ""
var initial_markers := 0

func _ready() -> void:
    Engine.max_fps = 60
    for arg: String in OS.get_cmdline_user_args():
        if arg.begins_with("--result="):
            result_path = arg.trim_prefix("--result=")
    runtime = ClassDB.instantiate("ReScaleFrameRuntime") as RefCounted
    if not runtime.call("configure", {"sr_backend": 0, "fg_mode": 0, "latency_mode": 1}, get_viewport().get_viewport_rid()):
        failures.append("Camera-free PCL configuration was refused")
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
    if frames == 120:
        if status.get("pcl_pings", 0) < 1 or status.get("marker_calls", 0) <= initial_markers or status.get("sleep_calls", 0) < 1:
            failures.append("Camera-free PCL did not reach the SDK")
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
