# SPDX-License-Identifier: GPL-3.0-only
class_name ReScaleFrameController
extends Node

signal configuration_applied
signal configuration_failed(message: String)
signal status_changed(status: Dictionary)

@export var settings: ReScaleFrameSettings
@export var compositor_target: Node
@export var enabled := true:
    set(value):
        enabled = value
        if is_node_ready() and not Engine.is_editor_hint():
            apply_settings()

static var _active: WeakRef
var _runtime: RefCounted
var _target: Node
var _effect: CompositorEffect
var _owns_effect := false
var _status_elapsed := 0.0

func _ready() -> void:
    if Engine.is_editor_hint():
        return
    if settings == null:
        settings = ReScaleFrameSettings.new()
    if enabled:
        apply_settings()

func _failure(message: String) -> bool:
    configuration_failed.emit(message)
    push_error(message)
    return false

func _attach_motion() -> bool:
    if is_instance_valid(_target) and _effect != null:
        return true
    _target = compositor_target
    if _target == null:
        _target = get_parent() if get_parent() is Camera3D or get_parent() is WorldEnvironment else get_viewport().get_camera_3d()
    if not (_target is Camera3D or _target is WorldEnvironment):
        return _failure("Assign a Camera3D or WorldEnvironment as ReScaleFrame's compositor target.")
    var previous: Compositor = _target.get("compositor")
    var compositor: Compositor = previous.duplicate() if previous else Compositor.new()
    var effects: Array[CompositorEffect] = compositor.compositor_effects.duplicate()
    for effect: CompositorEffect in effects:
        if effect.is_class("ReScaleFrameEffect"):
            _effect = effect
            return true
    _effect = ClassDB.instantiate("ReScaleFrameEffect") as CompositorEffect
    effects.append(_effect)
    compositor.compositor_effects = effects
    _target.set("compositor", compositor)
    _owns_effect = true
    return true

func apply_settings() -> bool:
    if Engine.is_editor_hint():
        return false
    if _active != null and is_instance_valid(_active.get_ref()) and _active.get_ref() != self:
        return _failure("Only one ReScaleFrameController can own the primary viewport.")
    if not enabled and _runtime == null:
        return true
    if not ClassDB.class_exists("ReScaleFrameRuntime"):
        return _failure("ReScaleFrame's native extension is not loaded.")
    if _runtime == null:
        _runtime = ClassDB.instantiate("ReScaleFrameRuntime") as RefCounted
    var status: Dictionary = _runtime.call("get_status")
    if not status.get("graphics_driver_supported", false) or not status.get("renderer_supported", false):
        return _failure("ReScaleFrame currently requires Forward+ and Direct3D 12. Vulkan rendering support is still in development.")
    if not status.get("engine_bridge_ready", false):
        return _failure("ReScaleFrame requires its custom engine and rescaleframe/enabled at startup.")
    if enabled and settings != null and (settings.sr_backend != 0 or settings.fg_mode != 0) and not _attach_motion():
        return false
    var options := settings.to_options() if enabled and settings != null else _off_options()
    if not _runtime.call("configure", options, get_viewport().get_viewport_rid()):
        return _failure("ReScaleFrame refused these settings. Check the selected provider and quality.")
    _active = weakref(self)
    configuration_applied.emit()
    return true

func get_status() -> Dictionary:
    return _runtime.call("get_status") if _runtime != null else {"engine_bridge_ready": false}

func reset_history() -> void:
    if _runtime != null:
        _runtime.call("reset_history")

func request_latency_ping() -> bool:
    return _runtime != null and _runtime.call("request_latency_ping")

func _off_options() -> Dictionary:
    return {"sr_backend": 0, "fg_mode": 0, "latency_mode": 0}

func _process(delta: float) -> void:
    if _runtime == null:
        return
    _status_elapsed += delta
    if _status_elapsed >= 0.25:
        _status_elapsed = 0.0
        status_changed.emit(get_status())

func _exit_tree() -> void:
    if _active != null and _active.get_ref() == self:
        if _runtime != null:
            _runtime.call("configure", _off_options(), get_viewport().get_viewport_rid())
        _active = null
    if _owns_effect and is_instance_valid(_target):
        var compositor: Compositor = _target.get("compositor")
        if compositor != null:
            var effects: Array[CompositorEffect] = compositor.compositor_effects.duplicate()
            effects.erase(_effect)
            compositor.compositor_effects = effects
    _effect = null
    _runtime = null
