# SPDX-License-Identifier: GPL-3.0-only
@tool
extends EditorExportPlugin

const BASE := "res://addons/rescaleframe/"
const DESTINATION := "addons/rescaleframe/"

func _get_name() -> String:
    return "ReScaleFrame"

func _supports_platform(platform: EditorExportPlatform) -> bool:
    return platform.get_os_name() == "Windows"

func _export_begin(features: PackedStringArray, _is_debug: bool, _path: String, _flags: int) -> void:
    if not features.has("windows") or not features.has("x86_64"):
        return
    var tags := PackedStringArray(["windows", "x86_64"])
    # The engine loads this exact disk path before GDExtension initialization.
    # Godot's built-in GDExtension exporter also copies its library to the root.
    for relative: String in ["bin/ReScaleFrame.Godot.Native.dll", "LICENSE.txt", "godot-cpp-LICENSE.md", "Godot-LICENSE.txt", "Godot-COPYRIGHT.txt", "runtime-manifest.json"]:
        add_shared_object(BASE + relative, tags, DESTINATION + relative.get_base_dir())
    var raw: Variant = JSON.parse_string(FileAccess.get_file_as_string(BASE + "runtime-manifest.json"))
    if not raw is Dictionary or raw.get("schema_version") != 1 or not raw.get("files") is Array:
        push_error("ReScaleFrame runtime manifest is missing or invalid. Stage the runtime package before export.")
        return
    for entry: Variant in raw.files:
        if not entry is Dictionary or not entry.get("path") is String or not entry.get("sha256") is String:
            push_error("Invalid ReScaleFrame runtime manifest entry.")
            return
        var relative: String = entry.path
        if not relative.begins_with("runtime/") or relative.contains("..") or relative.contains("\\") or relative.is_absolute_path():
            push_error("Invalid ReScaleFrame runtime export path: " + relative)
            return
        var source := BASE + relative
        if FileAccess.get_sha256(source) != entry.sha256:
            push_error("ReScaleFrame runtime is missing or differs from its manifest: " + source)
        # A missing dependency also makes the native export copy fail.
        add_shared_object(source, tags, DESTINATION + relative.get_base_dir())
