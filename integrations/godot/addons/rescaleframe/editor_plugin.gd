# SPDX-License-Identifier: GPL-3.0-only
@tool
extends EditorPlugin

var runtime_export := preload("res://addons/rescaleframe/export_plugin.gd").new()

func _enter_tree() -> void:
    _register_setting("rescaleframe/enabled", false, TYPE_BOOL)
    _register_setting("rescaleframe/native_library", "res://addons/rescaleframe/bin/ReScaleFrame.Godot.Native.dll", TYPE_STRING, PROPERTY_HINT_FILE, "*.dll")
    _register_setting("rescaleframe/runtime_directory", "res://addons/rescaleframe/runtime", TYPE_STRING, PROPERTY_HINT_DIR)
    _register_setting("rescaleframe/project_id", "", TYPE_STRING)
    if ProjectSettings.get_setting("rescaleframe/project_id", "").is_empty():
        var bytes := Crypto.new().generate_random_bytes(16)
        bytes[6] = (bytes[6] & 15) | 64
        bytes[8] = (bytes[8] & 63) | 128
        var value := bytes.hex_encode()
        ProjectSettings.set_setting("rescaleframe/project_id", "%s-%s-%s-%s-%s" % [value.substr(0, 8), value.substr(8, 4), value.substr(12, 4), value.substr(16, 4), value.substr(20)])
        ProjectSettings.save()
    add_export_plugin(runtime_export)
    add_tool_menu_item("ReScaleFrame: Attach Renderer Probe", _attach_probe)

func _exit_tree() -> void:
    remove_export_plugin(runtime_export)
    remove_tool_menu_item("ReScaleFrame: Attach Renderer Probe")

func _register_setting(key: String, value: Variant, type: int, hint := PROPERTY_HINT_NONE, hint_string := "") -> void:
    if not ProjectSettings.has_setting(key):
        ProjectSettings.set_setting(key, value)
    ProjectSettings.set_initial_value(key, value)
    ProjectSettings.add_property_info({"name": key, "type": type, "hint": hint, "hint_string": hint_string})

func _attach_probe() -> void:
    var selected := EditorInterface.get_selection().get_selected_nodes()
    if selected.size() != 1 or not (selected[0] is WorldEnvironment or selected[0] is Camera3D):
        push_warning("Select one WorldEnvironment or Camera3D to attach ReScaleFrame.")
        return
    if not ClassDB.class_exists("ReScaleFrameEffect"):
        push_error("ReScaleFrame's native GDExtension is not loaded.")
        return
    var target: Node = selected[0]
    var previous: Compositor = target.get("compositor")
    var compositor: Compositor = previous.duplicate() if previous else Compositor.new()
    var effects: Array[CompositorEffect] = compositor.compositor_effects.duplicate()
    for effect: CompositorEffect in effects:
        if effect.is_class("ReScaleFrameEffect"):
            push_warning("This compositor already contains ReScaleFrame.")
            return
    effects.append(ClassDB.instantiate("ReScaleFrameEffect") as CompositorEffect)
    compositor.compositor_effects = effects
    var undo := get_undo_redo()
    undo.create_action("Attach ReScaleFrame Renderer Probe")
    undo.add_do_property(target, "compositor", compositor)
    undo.add_undo_property(target, "compositor", previous)
    undo.commit_action()
