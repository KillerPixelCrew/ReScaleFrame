# SPDX-License-Identifier: GPL-3.0-only
extends Node3D

var effect: CompositorEffect
var world: WorldEnvironment
var camera: Camera3D
var moving_box: MeshInstance3D
var label: Label
var mono_test: Node
var frame := 0
var disabled_callbacks := 0
var result_path := ""
var failures: Array[String] = []
var samples: Array[Dictionary] = []

func _ready() -> void:
    for arg: String in OS.get_cmdline_user_args():
        if arg.begins_with("--result="):
            result_path = arg.trim_prefix("--result=")
    if not ClassDB.class_exists("ReScaleFrameEffect"):
        _require(false, "Native extension did not load")
        _finish()
        return
    world = WorldEnvironment.new()
    world.name = "WorldEnvironment"
    world.environment = Environment.new()
    world.environment.background_mode = Environment.BG_COLOR
    world.environment.background_color = Color(0.03, 0.05, 0.09)
    world.environment.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
    world.environment.ambient_light_color = Color.WHITE
    world.environment.ambient_light_energy = 0.5
    add_child(world)
    world.compositor = Compositor.new()
    replace_effect(ClassDB.instantiate("ReScaleFrameEffect") as CompositorEffect)
    camera = Camera3D.new()
    add_child(camera)
    camera.position = Vector3(0, 2.5, 6)
    camera.look_at(Vector3.ZERO)
    camera.current = true
    moving_box = MeshInstance3D.new()
    moving_box.mesh = BoxMesh.new()
    var material := StandardMaterial3D.new()
    material.albedo_color = Color(0.1, 0.65, 0.9)
    moving_box.material_override = material
    add_child(moving_box)
    var floor_mesh := MeshInstance3D.new()
    var plane := PlaneMesh.new()
    plane.size = Vector2(8, 8)
    floor_mesh.mesh = plane
    floor_mesh.position.y = -0.6
    add_child(floor_mesh)
    var light := DirectionalLight3D.new()
    light.rotation_degrees = Vector3(-45, -25, 0)
    add_child(light)
    label = Label.new()
    label.position = Vector2(12, 12)
    add_child(label)
    get_viewport().scaling_3d_scale = 0.67
    if FileAccess.file_exists("res://MonoSmoke.cs"):
        mono_test = load("res://MonoSmoke.cs").new() as Node
        add_child(mono_test)

func replace_effect(value: CompositorEffect) -> void:
    effect = value
    var effects: Array[CompositorEffect] = [effect]
    world.compositor.compositor_effects = effects

func _process(_delta: float) -> void:
    if effect == null:
        return
    frame += 1
    if frame < 80 or result_path.is_empty():
        moving_box.position.x = sin(frame / 30.0) * 1.2
    if frame >= 80:
        camera.position.x = sin((frame - 80) / 60.0) * 0.6
        camera.look_at(Vector3.ZERO)
    var status: Dictionary = effect.call("get_status")
    label.text = "ReScaleFrame renderer probe (%s)\n%s -> %s, callbacks: %s\nSR / FG / latency: not connected" % [
        status.driver, status.internal_size, status.output_size, status.callbacks]
    if result_path.is_empty():
        return
    if frame == 35:
        effect.call("request_motion_capture")
    elif frame == 55:
        _check_sample(status, "object_motion", 1)
    elif frame == 60:
        DisplayServer.window_set_size(Vector2i(800, 450))
    elif frame == 95:
        effect.call("request_motion_capture")
    elif frame == 115:
        _check_sample(status, "camera_motion_after_resize", 2)
        _require(status.output_size == Vector2i(800, 450), "Output did not follow window resize")
        disabled_callbacks = status.callbacks
        effect.enabled = false
    elif frame == 125:
        # One already queued frame may complete after disabling the effect.
        _require(status.callbacks <= disabled_callbacks + 2, "Disabled compositor kept receiving callbacks")
        disabled_callbacks = status.callbacks
        effect.enabled = true
    elif frame == 160:
        _require(status.callbacks > disabled_callbacks, "Compositor did not resume after re-enabling")
        _finish()

func _check_sample(status: Dictionary, phase: String, expected_captures: int) -> void:
    status["phase"] = phase
    samples.append(status)
    _require(status.driver in ["d3d12", "vulkan"], "Unexpected graphics API")
    _require(status.renderer == "forward_plus", "Forward+ renderer was not selected")
    _require(status.buffers_available, "Scene buffers are missing")
    _require(status.native_device_available and status.native_queue_available and status.native_textures_available,
        "Native device, queue or texture handles are missing")
    _require(status.internal_size.x < status.output_size.x and status.internal_size.y < status.output_size.y,
        "Internal render size does not reflect reduced-scale rendering")
    _require(not status.rendering_ready and not status.sr_active and not status.fg_active and not status.latency_active,
        "Probe incorrectly reports an active rendering pipeline")
    _require(status.motion_captures == expected_captures and status.capture_succeeded, "Motion capture failed")
    _require(status.capture_moving_pixels > 0 and status.capture_nonfinite_pixels == 0,
        "Motion capture has no finite moving pixels")

func _require(condition: bool, message: String) -> void:
    if not condition:
        failures.append(message)
        push_error(message)

func _finish() -> void:
    if result_path.is_empty():
        get_tree().quit(2)
        return
    var mono_status: Dictionary = {}
    if mono_test:
        mono_status = mono_test.call("validate_mono")
        _require(mono_status.get("passed", false), "C# wrapper or C#-created native effect failed")
    var report := {"passed": failures.is_empty(), "failures": failures, "samples": samples, "mono": mono_status}
    var file := FileAccess.open(result_path, FileAccess.WRITE)
    if file:
        file.store_string(JSON.stringify(report, "  "))
    else:
        push_error("Cannot write validation report: " + result_path)
        get_tree().quit(2)
        return
    print("RSF_GODOT_VALIDATION ", JSON.stringify(report))
    get_tree().quit(0 if failures.is_empty() else 2)
