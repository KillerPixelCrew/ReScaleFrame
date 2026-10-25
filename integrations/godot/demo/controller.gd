# SPDX-License-Identifier: GPL-3.0-only
extends "res://scene.gd"

var controller: ReScaleFrameController
var observer: RefCounted
var paused_callbacks := 0
var marker_start := 0
var idle_evaluations := 0

func _ready() -> void:
    super._ready()
    Engine.max_fps = 60
    process_mode = Node.PROCESS_MODE_ALWAYS
    observer = ClassDB.instantiate("ReScaleFrameRuntime") as RefCounted
    controller = _new_controller(3, 1)

func _new_controller(sr: int, fg: int) -> ReScaleFrameController:
    var instance := ReScaleFrameController.new()
    instance.settings = ReScaleFrameSettings.new()
    instance.settings.sr_backend = sr
    instance.settings.fg_backend = 3
    instance.settings.fg_mode = fg
    instance.settings.latency_mode = 1
    instance.compositor_target = world if sr != 0 or fg != 0 else null
    add_child(instance)
    return instance

func _process(_delta: float) -> void:
    if observer == null:
        return
    frame += 1
    label.text = "ReScaleFrame controller, frame %s" % frame
    if not get_tree().paused:
        moving_box.position.x = sin(frame / 20.0) * 1.5
    var status: Dictionary = observer.call("get_status")
    if frame == 30:
        _require(controller.request_latency_ping(), "Controller cannot post a PCL diagnostic message")
    elif frame == 60:
        _require(status.active_sr == 3 and status.fg_backend == 3 and status.fg_active != 0, "Controller settings did not activate SR and FG")
        _require(status.pcl_pings > 0, "Controller PCL message did not reach the SDK")
        marker_start = status.marker_calls
        samples.append(status)
    elif frame == 70:
        get_tree().paused = true
    elif frame == 80:
        paused_callbacks = status.generated_callbacks
    elif frame == 90:
        _require(status.fg_active == 0 and status.generated_callbacks == paused_callbacks, "FG continued interpolating the paused scene")
        _require(status.marker_calls > marker_start, "PCL stopped while the scene was paused")
        samples.append(status)
        get_tree().paused = false
    elif frame == 95:
        var previous := camera
        camera = Camera3D.new()
        camera.transform = previous.transform
        add_child(camera)
        camera.current = true
        previous.queue_free()
    elif frame == 97:
        Engine.time_scale = 0.25
    elif frame == 100:
        camera.position = Vector3(40, 2.5, 6)
        camera.look_at(Vector3.ZERO)
        controller.reset_history()
    elif frame == 105:
        Engine.time_scale = 1.0
    elif frame == 120:
        controller.enabled = false
    elif frame == 140:
        _require(status.active_sr == 0 and status.fg_active == 0 and status.latency_mode == 0, "Disabling the controller left effects active")
        samples.append(status)
        controller.settings.sr_backend = 1
        controller.enabled = true
    elif frame == 175:
        _require(status.active_sr == 1 and status.fg_active != 0, "The controller did not reactivate after Off")
        samples.append(status)
        controller.queue_free()
    elif frame == 185:
        _require(status.active_sr == 0 and status.fg_active == 0, "Removing the controller left effects active")
        camera.queue_free()
        controller = _new_controller(0, 0)
    elif frame == 195:
        idle_evaluations = status.evaluated_frames
        marker_start = status.marker_calls
    elif frame == 200:
        _require(controller.request_latency_ping(), "The replacement controller could not post a PCL message")
    elif frame == 220:
        _require(status.active_sr == 0 and status.fg_active == 0 and status.pcl_available != 0 and status.pcl_pings >= 2,
            "PCL without SR/FG failed after replacing the controller")
        _require(status.evaluated_frames == idle_evaluations and status.marker_calls > marker_start,
            "PCL without a 3D camera relied on stale scene rendering")
        samples.append(status)
        observer = null
        _finish()
