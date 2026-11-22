# Godot GDExtension on the C interface (E02)

## Outcome

`ReScaleFrame.Godot.Native.dll` contains no first-party C++. The GDExtension entry
`rsf_godot_initialize`, the `ReScaleFrameEffect` (`CompositorEffect`) and `ReScaleFrameRuntime`
(`RefCounted`) classes, their bound methods, the `_render_callback` virtual, the Variant and
Dictionary conversions and every RenderingDevice call are Rust in
`integrations/godot/native/gdextension.rs` and `gdextension/`. They are written against Godot's C
interface, so godot-cpp is no longer compiled or linked. `runtime_platform.c` is the only native
translation unit. It is a C11 leaf with the layout assertions, the CMake-selected optional Vulkan
provider symbols, the CMake project version and the DLL unload hook.

Removed: `effect.cpp`, `effect.h`, `extension.cpp`, `runtime.cpp`, `runtime_control.cpp`,
`runtime_control.h` and `script_dictionary.h` (which existed only to adapt godot-cpp's Dictionary).

## Interface choices

The add-on declares `compatibility_minimum = "4.7"`, and godot-cpp at the pinned revision
`8070599885b6df675fcad033dd032ce44be2b850` used the 4.7 entry points. The Rust code uses the same
ones from the pinned `gdextension_interface.json`:

- `classdb_register_extension_class6` with `GDExtensionClassCreationInfo6`, and
  `classdb_construct_object3`. In 4.7 a RefCounted object from `construct_object3` already holds
  one reference, which `create_instance_func` hands to the engine. godot-cpp's
  `memnew_result` adopts it without an extra increment for the same reason.
- `Object.is_class(StringName)` replaces the deprecated `object_cast_to` and
  `classdb_get_class_tag` for the `Ref<RenderSceneBuffersRD>` conversion. A non-RD scene buffer
  converts to an empty reference and the frame leaf refuses, as before.
- `get_godot_version2` refuses a 4.x engine older than 4.7, as godot-cpp's initializer did.
- `NOTIFICATION_POSTINITIALIZE` is sent through `Object.notification` when the engine requests it.

Method binds use the pinned `extension_api-4-7.json` hashes. Builtin sizes follow its `float_64`
table (Variant 24, Dictionary 8, String 8, StringName 8, PackedByteArray 16, Callable 16).
Single-precision `Transform3D` (basis rows, then origin) and `Projection` (columns) are copied in
memory order. These are the same orders the C++ loops used.

Ptrcall encodings follow Godot's `method_ptrcall.h`. Integers and enums are `i64`, booleans one
byte and RIDs `u64`. Objects are passed as pointers to object pointers and builtins by pointer.
Returned builtins are assigned into constructed storage. A returned `Ref<T>` arrives as one owned
reference, which `Ref` releases with `RefCounted.unreference` and `object_destroy`, as godot-cpp's
`Ref<T>` did.

## Preserved behaviour

- Effect construction sets `POST_TRANSPARENT`, resolved color, resolved depth and motion
  vectors. The virtual is matched by name and the 4.7 hash `2153422729`.
- Effect leaves keep their table, order and refusals. The readback keeps the shader text,
  `allow_cache`, the empty specialization array typed to `RDPipelineSpecializationConstant`, the
  sampler, storage buffer and uniform bindings, the `RDUniform`-typed uniform array, the dispatch
  and `buffer_get_data(buffer, 0, 0)`. `rsf_godot_effect_record_motion` still frees uniform
  set, buffer, sampler, pipeline and shader in that order. The readback bytes stay owned by the
  callback record until `effect.rs` has classified them.
- `configure` checks the RID, reads each option with `Dictionary.has` and the const index, and
  classifies INT, BOOL or other. It converts with Godot's to-type constructors, the same ones
  godot-cpp's `int64_t(Variant)` and `double(Variant)` call. The Variant call path keeps
  godot-cpp's argument count and `variant_can_convert_strict` checks.
- Status Dictionaries receive the same keys, order and Variant types from `effect.rs` and
  `extension.rs`. The renderer and driver names come from RenderingServer, or empty without one.
- `request_frame_dump` schedules a custom Callable through `call_on_render_thread`.
- The unload hook registers `rsf_godot_host_unload` with `atexit` from a `.CRT$XCU` initializer,
  which is how MSVC lowers the former C++ static destructor. The same pattern is used in
  `runtime/orchestrator/src/native_composition_unload.c`.

`rsf_godot_renderer_admission` and the `rsf_godot_extension_capture` export existed only for
the C++ classes and were removed. The remaining `rsf_godot_effect_*` and `rsf_godot_extension_*`
entries are the Rust owners' C-shaped seams and are still used by the fixtures.

## Build wiring

`integrations/godot/CMakeLists.txt` builds `rsf_godot` from `exports.def`, `runtime_platform.c`
and the Rust staticlib when `RSF_GODOT_NATIVE=ON`, which `eng/build-godot.ps1` now passes in place
of `RSF_GODOT_CPP_DIR`. `exports.def` now also exports `rsf_godot_initialize`, which godot-cpp's
`GDE_EXPORT` used to provide. The godot-cpp pin in `dependencies.json` stays as the source of the
API description and its license text is still staged.

## Checks

The integration tip did not build while this was done (generated bindings missing derives, a
missing `)` in `tests/CMakeLists.txt`, a stale Anomaly FG source and a yyjson line-ending pin).
Verification therefore ran in a throwaway worktree with only those tip defects patched. The branch
does not carry those patches. See the commit message for the exact commands and results.
