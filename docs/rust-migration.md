# Rust orchestrator and integration migration

The Rust orchestrator replaced the C++ orchestrator behind its existing C interfaces, and the Rust
Anomaly adapter replaced the C++ one. Each production path has one implementation. Git history
records every cutover; the per-slice narration that used to live here is not kept. Verification is
tracked in [the implementation tracker](implementation.md), and the policy for what stays native is in
[Retained native code](design.md#retained-native-code).

Remove first-party C++ wherever Rust can preserve the required ABI, behaviour and lifetime
guarantees. SDK calls, COM calls, JNI and an existing C++ wrapper are candidates for review. Retain a
native leaf only for a concrete constraint that prevents a correct Rust replacement, and record that
constraint with the affected caller.

## C ABI comparison

The GameSDK stays at the ABI in `sdk/game/include/rescaleframe/game_api.h`. The source audit at
`6414a05` compared 88 headers across the SDK, Runtime and Anomaly against `2db07f4`. Four headers
differed after removing comments and whitespace: `native_fg.h` added three linked-runtime
declarations, `d3d11_present_bridge.h` added `rsf_d3d11_present_source_queue`, `overlay_api.h` raised
`RSF_OVERLAY_SERVICE_OPTION_LIMIT` from 24 to 32, and `shader_compile.h` changed the compiler helper's
native ownership. Every existing C function signature and public record layout matched, and the four
carrier `.def` files still matched `2db07f4`.

On 21 November 2026 both archived MSVC Release trees (`build/rust-migration` and
`build/rust-migration-streamline`) matched the recorded case-sensitive export names and ordinals:
Runtime 381, AC7 256, Loader 273, Minecraft 267, the staged C++ Anomaly adapter 3, and the rebuilt
Godot extension 398. The Runtime, combined loader and Minecraft consume the same
`rsf_orchestrator_core` object target; the Runtime DLL retains the native Vulkan service archive with
whole-archive linkage. Callers must account for two documented behaviour changes: frame assembly
requires the output record to advertise its capacity and refuses a zero or short capacity before
writing, and a second overlay settings write returns `BUSY` while the first is active.

The Anomaly C++ adapter baseline (Standard and AVX executable hashes, and what each profile
exercised) is recorded in [the C++ baseline note](research/rust-anomaly-cpp-baseline.md).

## Retained C++ and why

Twelve first-party `.cpp` files remain. Each group is blocked by a concrete constraint.

| Group | Files | Blocking constraint |
| --- | --- | --- |
| Godot engine glue | `integrations/godot/modules/rescaleframe/{bridge,d3d12,graph,motion,vulkan}.cpp` | The module registers C++ engine classes and reads engine render-graph structures; the engine's SCons build compiles them |
| Vendor layout checks | `tests/dlss_sr_layout_facts.cpp`, `tests/loader/vendor_layout_facts.cpp` | They include the Streamline C++ headers (`sl.h`, `sl_dlss.h`, ...) and compare their layouts with the Rust records; only a C++ compiler can evaluate those headers |
| MSVC RTTI and NGX fixtures | `tests/loader/module_dump_rtti_fixture.cpp`, `tests/loader/vendor_census_ngx_platform.cpp` | They build MSVC RTTI objects and C++ NGX parameter-platform types |
| Streamline fixtures | `tests/streamline_selected_device.cpp`, `tests/streamline_vulkan_dispatch.cpp` | They include the C++ SDK-fix headers (`selected-device-config.h`, `ngx-vulkan-dispatch.h`, `sdk_fg_options_checks.h`) that patch Streamline sources |
| Godot graph fixture | `tests/godot_graph_handoff.cpp` | It includes `graph.cpp` from the Godot module directly |

Other native code is C11: SEH guards that contain foreign C++ exceptions, MinHook calls and COM
calls on vendor objects. The SDK patchers that edit vendor sources (`runtime/vulkan/streamline/sdk-fixes`,
`runtime/vulkan/fsr3/sdk-fixes`) also remain.
