# Closing C++ the port introduced (W01, S02, I01, G01)

Four first-party C++ files that no chunk owned are now C11 leaves. Each only does what Rust
cannot express: containing foreign C++ exceptions with SEH, MinHook calls, and COM calls on
vendor objects. No Rust caller changed, so the exported symbol names, signatures and table
layouts are the ones the Rust crates already declare.

## Exception policy

MSVC raises a C++ exception as SEH code `0xE06D7363`. Each former `catch (...)` guard is now
`__try` with an `__except` filter that accepts only that code, the same filter used by
`runtime/orchestrator/src/plugin_callbacks.c`. Access violations and other faults still
propagate to the caller, as they did under `/EHsc`. A Rust panic is also raised as
`0xE06D7363` on MSVC, so panics stay contained as before. Scopes that C++ destructors closed
on unwind use `__try/__finally`.

One guard deliberately keeps `EXCEPTION_EXECUTE_HANDLER`, because the C++ source already used
it: the Wingman `acquire_texture` engine/COM probe, which treats any fault as "no texture".

## Files

| Old file | New file | Notes |
| --- | --- | --- |
| `integrations/steam/src/exception_guards.cpp` (I01) | `exception_guards.c` | Dispatch guard returns `RSF_ERROR_NOT_READY` on a foreign exception. The layout `static_assert` is `_Static_assert`. |
| `runtime/graphics/src/observer_exception_guards.cpp` (G01) | `observer_exception_guards.c` | The seven callback guards, texture extent, the present-ops table, probe create and `QueryInterface`, `rsf_observer_install_guarded` and the shared `rsf_observer12_try` frame. COM calls use `COBJMACROS` and `initguid.h`, so no `dxguid` link is needed. The three private headers (`d3d11_observer_callbacks.h`, `d3d11_observer_hooks.h`, `d3d11_observer_present.h`) now hold C declarations (typedef structs and `_Static_assert`). Probe create writes straight into the caller's attempt record, so outputs written before an exception stay visible for release. |
| `games/skyrim-se/src/render_detours.cpp` (S02) | `render_detours.c` | The five observers keep their typed continuations and the actual `_ReturnAddress()` of the pre-HDR call. The activity lease and the TAA restore receipt are `__try/__finally`, so Leave and restore still run when the engine raises. `observer_platform.h` and `session_platform.h` now hold C declarations. |
| `games/project-wingman/src/native_platform.cpp` (W01) | `native_platform.c` | MinHook create/enable/disable, the RHI native-resource probe, release, execution report and the GameSDK export. `report_execution` releases the context then the device in a `__finally`, matching the former `ComPtr` order. |

Moving more logic into Rust was considered for the Wingman report and the D3D11 observer COM
calls. They are single vendor-vtable calls whose only purpose is to sit inside the exception
frame, so a Rust port would have added a COM binding dependency without removing the native
frame. They stay native.

## Checked

- `cl /std:c11 /W4 /WX /c` (MSVC 14.44, VS 2022 Build Tools) on all four files with the
  include paths of their CMake targets: clean.
- A scratch executable linked against the Steam and observer leaves confirmed that a real C++
  `throw` (with a destructor) through `rsf_steam_dispatch_guard` returns `RSF_ERROR_NOT_READY`,
  the log guard swallows a throw, `rsf_observer12_try` returns 0 for a throw and 1 for a normal
  call, `rsf_observer_install_guarded` maps a throw to `RSF_OBSERVER_ERROR_NO_DEVICE`, and an
  access violation raised inside the Steam dispatch guard and inside `rsf_observer12_try`
  propagates.
- CMake: `cmake --preset windows-x64` fails in this environment before generation, at the
  pinned yyjson hash check (`runtime/shader_patch/yyjson.cmake`), which is outside these files.
  The four targets, the Rust links and CTest were therefore not built. The Rust sources were
  not changed.
