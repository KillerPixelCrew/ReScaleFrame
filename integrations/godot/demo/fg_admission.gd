# SPDX-License-Identifier: GPL-3.0-only
# Explicit negative admission fixture. Run only when device discovery lacks FG4.
extends "res://scene.gd"

var runtime: RefCounted
var baseline: Dictionary = {}
var immediate: Dictionary = {}
var continued: Dictionary = {}
var rejected := false
const SETTINGS := ["requested_sr", "requested_fg_backend", "requested_fg_mode",
    "requested_generated_frames", "requested_latency_mode", "requested_dlss_preset"]

func _ready() -> void:
    super._ready()
    Engine.max_fps = 60
    _require("--expect-fg4-unsupported" in OS.get_cmdline_user_args(), "Explicit negative FG4 expectation is missing")
    runtime = ClassDB.instantiate("ReScaleFrameRuntime") as RefCounted
    _require(runtime.call("configure", {"sr_backend": 0, "quality": 1, "fg_backend": 3,
        "fg_mode": 1, "generated_frames": 1, "latency_mode": 1}, get_viewport().get_viewport_rid()),
        "Working FSR3 FG baseline was refused")

func _working(status: Dictionary) -> void:
    _require(status.get("active_sr", -1) == 0 and status.get("sr_result", -1) == 0, "Native SR Off did not remain active")
    _require(status.get("fg_backend", 0) == 3 and status.get("fg_available", 0) == 1 and
        status.get("fg_active", 0) == 1 and status.get("fg_result", -1) == 0 and status.get("generated_frames", 0) == 1,
        "Working FSR3 FG owner was lost")
    _require(status.get("latency_mode", 0) == 1 and status.get("pcl_available", 0) == 1 and
        status.get("latency_available", 0) == 1 and status.get("sleep_calls", 0) > 8 and status.get("marker_calls", 0) > 8,
        "PCL/Reflex was lost")

func _unchanged(status: Dictionary) -> void:
    for key: String in SETTINGS:
        _require(status.has(key) and baseline.has(key) and status[key] == baseline[key], "Refused FG4 request changed " + key)
    _require(status.get("session_id", 0) == baseline.get("session_id", -1), "Refused FG4 request changed the runtime session")
    _require(status.get("fg_backend", 0) == 3, "Refused FG4 request changed the provider")

func _snapshot(phase: String) -> Dictionary:
    var image := get_viewport().get_texture().get_image()
    _require(image != null and not image.is_empty(), "Actual player image is missing")
    var visible := 0
    if image:
        _require(image.save_png(result_path.get_basename() + "-" + phase + ".png") == OK, "Cannot save actual player PNG")
        for y in range(64, image.get_height() - 16, 2):
            for x in range(16, image.get_width() - 16, 2):
                var pixel := image.get_pixel(x, y)
                if pixel.b > pixel.r * 1.5 and pixel.g > pixel.r * 1.5 and pixel.b > 0.2:
                    visible += 1
    _require(visible > 100, "The cyan object is missing after admission refusal")
    var status: Dictionary = runtime.call("get_status")
    status["visible_object_samples"] = visible
    status["phase"] = phase
    samples.append(status)
    return status

func _process(_delta: float) -> void:
    if runtime == null:
        return
    frame += 1
    moving_box.position.x = sin(frame / 20.0) * 1.5
    camera.position.x = sin(frame / 45.0) * 1.2
    camera.look_at(Vector3.ZERO)
    var status: Dictionary = runtime.call("get_status")
    label.text = "FG4 negative admission\nWorking FSR3 FG, tagged %s / generated callbacks %s" % [status.get("tagged_frames", 0), status.get("generated_callbacks", 0)]
    if frame == 30:
        _require(runtime.call("request_latency_ping"), "PCL ping could not be posted")
    elif frame == 45:
        baseline = _snapshot("baseline")
        _working(baseline)
        _require(baseline.get("tagged_frames", 0) > 12 and baseline.get("generated_callbacks", 0) > 12,
            "FSR3 had no actual generation activity before admission")
        _require(baseline.get("pcl_pings", 0) > 0, "PCL ping did not reach the SDK")
    elif frame == 46:
        rejected = not runtime.call("configure", {"sr_backend": 0, "quality": 1, "fg_backend": 4,
            "fg_mode": 1, "generated_frames": 1, "latency_mode": 1}, get_viewport().get_viewport_rid())
        _require(rejected, "Expected unsupported exact FG4 request was accepted")
        immediate = runtime.call("get_status")
        _unchanged(immediate)
        _working(immediate)
    elif frame == 75:
        continued = _snapshot("continued")
        _unchanged(continued)
        _working(continued)
        for key: String in ["source_frame_id", "tagged_frames", "generated_callbacks", "sleep_calls", "marker_calls"]:
            _require(continued.get(key, 0) > baseline.get(key, 0) + 8, "Activity did not continue after FG4 refusal: " + key)
    elif frame == 90:
        var mono_status: Dictionary = {}
        if mono_test:
            mono_status = mono_test.call("validate_mono")
            _require(mono_status.get("passed", false), "C# facade failed after admission refusal")
            _unchanged(mono_status.get("runtime_status", {}))
        var file := FileAccess.open(result_path, FileAccess.WRITE)
        if file:
            file.store_string(JSON.stringify({"passed": failures.is_empty(), "failures": failures, "samples": samples,
                "scenario": "negative_exact_fg4_admission", "expected_unsupported_backend": 4,
                "scope": "Unsupported exact FG4 request preserves working FSR3 FG and PCL; no FG4 rendering or AMD unsupported claim",
                "rejected": rejected, "baseline": baseline, "immediate": immediate, "continued": continued, "mono": mono_status}, "  "))
        else:
            _require(false, "Cannot save negative admission report")
        runtime = null
        get_tree().quit(0 if failures.is_empty() else 2)
