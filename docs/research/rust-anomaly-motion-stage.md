# Anomaly motion-stage owner in Rust

This cutover answers who owns the motion stage between the native shader, scene
and draw callbacks. It starts from the merged adapter at `a68567a`. The
[staged C++ baseline](rust-anomaly-cpp-baseline.md) at `8322179a36c7` supplies
the DLL and shader pack for comparison. The [Standard and AVX fingerprints and
source revision](../../games/anomaly/engine.json) still govern the engine
boundary. This work uses the verified engine addresses, expected bytes,
buffer layouts and shader rules.

The native shim keeps the exact `create_shader`, `phase_scene_begin` and
`phase_scene_end` guards and trampolines, both verified compiler return sites,
the empty source-name and D3DCompile setup guards, and the hull/domain creation
and wrapper guards. It still reads the selected profile's D3D11 device and
context, RCache, engine frame and projection fields under the existing guards.
The actual write to the RTV2 cache and the constant-buffer copies remain behind
the native SEH leaves. The Draw/DrawIndexed/instancing entry points keep their
original calling conventions. The private stage table checks its size and
version before Rust uses the leaves. The native guard implementation matches
the pre-port source byte for byte.

RTV2 stores current minus previous viewport UV, with Y down, validity in the
third channel and zero in the fourth. The existing b13 and replay owners
retain the draw and resource boundaries described in the earlier shader and
producer evidence. This scope uses that inventory for unchanged forward, UI
and unsupported geometry paths.

| Boundary | Owner and evidence |
| --- | --- |
| Startup and settings | Rust reads the motion INI options and registers with the existing overlay registry. Observe mode still prepares variants with Motion off so the player can enable it live. Disable, foliage, invalidation, reload and shader dump use the existing controls. The capture controls save the schedule for the next start, when the process assigns the Runtime dump service. `SRStageCapture` retains precedence. |
| Shader creation and reload | After the engine creates its stock shader, Rust admits the guarded compile frame, identifies the exact source hash and selects the corresponding pack entry and stage. [The existing loader](rust-anomaly-shader-loader.md) still checks the compiled original, cache result and reflection. Native leaves supply the exact compiler and engine includer, D3D11 creation and reflection storage. Rust owns the pack, immutable original request, device, stock and replacement leases. Reload publishes the replacement registry only after preparation and object creation succeed for every retained root. The previous generation stays usable on failure. Successful reload clears linkage and both histories. |
| G-buffer window | Rust consumes the native CRenderTarget, its `rt_Position` CRT at `+0x378` and the engine's Main/Scope identity. It runs the existing target and binder owners and sizes RGBA16F RTV2 to the actual position texture, including an SR twin. Main and Scope histories remain per-view and split windows reuse the existing frame. A new device clears linkage and histories. The published `FrameTarget` still carries the native engine frame, Main flag, extent and written flag for the SR seam and SR-off FG. [Producer and seam evidence](anomaly-sr-seam.md) |
| Core, foliage and tessellation draws | After the original `CBackend::constants.flush`, Rust records the supplied camera and copied draw snapshot and invokes the existing history decisions and b13 packers. Core rigid and SKIN_NONE use the previous world, SKIN_0 uses the bone-composed world, and hardware skinning uses the previous palette. The stock-reflected tree and detail layouts retain their staged constants, per-view history, route and row poison. A verified VS/PS pair can swap directly. A bound hull or domain shader requires a core VS and a verified, same-device VS/HS/DS/PS chain. Geometry shaders, missing stages and unlinked interfaces keep stock. [History](rust-anomaly-motion-history.md), [foliage](anomaly-foliage-motion.md), [tessellation](anomaly-tess-motion.md), [character elements and distance counters](anomaly-far-character-motion.md) |
| Draw completion | The existing thread-local pending action accepts only the matching immediate context. A prepared replacement chain pins its objects through the draw and stock restoration, including cancellation at the next flush or scene boundary. Refused draws keep the existing optional zero-motion replay. Its native state scope preserves the depth-write test and restores the touched GPU state. The Runtime copies the three motion diagnostic images under the accepted capture record. |

The old capture scheduler tested `stats.frames`, which counted both Main and
Scope window openings, against a schedule documented in main-view frames. The
old close path also completed whichever active record it held without checking
the closing window's identity. Rust counts distinct Main engine frames and
requires the accepted reset generation, immediate context, position resource
and motion texture at close. It spends the requested count only after the
Runtime accepts the record. A fake dump-table regression interleaves Scope, a
repeated Main opening, an admission refusal and a close from a different reset
generation. It checks the exact source IDs and counts the emitted images.

The previous variant registry could replace an entry under a given stock address
without invalidating the cached linkage decision for that address. The Rust
registry invalidates the cache on replacement. A CPU regression replaces a
linked VS under the same key with one that fails linkage, while a pending chain
keeps the former shader alive until it is dropped. The selection tests also
cover foreign devices, partial tessellation, bound geometry and four-stage
linkage.

## Verification

`build/anomaly-motion-stage` and its FetchContent directory are in the worker
worktree. The MinHook source was read from the coordinator's pinned
`c3fcafdc10146beb5919319d0683e44e3c30d537` checkout. The three yyjson inputs
were copied only after matching `runtime/shader_patch/yyjson.lock.json`. CMake
used the coordinator's staged vendor directory for device fixture registration.
Vendor SDK headers were absent from this build configuration.

```powershell
cmake --preset windows-x64-vs18 -B build/anomaly-motion-stage `
  "-DFETCHCONTENT_BASE_DIR=$PWD/build/anomaly-motion-stage/_deps" `
  "-DFETCHCONTENT_SOURCE_DIR_MINHOOK=D:/Coding/ReScaleFrame/build/windows-x64/_deps/minhook-src" `
  "-DRSF_ANOMALY_SR_VENDOR_ROOT=D:/Coding/ReScaleFrame/build/releases/rdr2/stage-rdr2-a2bcc0c3/ReScaleFrame"
$targets = @(
    'rsf_game_anomaly', 'rsf_anomaly_patch_pack', 'rsf_anomaly_plugin_test',
    'rsf_anomaly_lifecycle_test', 'rsf_anomaly_bootstrap_test', 'rsf_anomaly_motion_contract_test',
    'rsf_anomaly_motion_hooks_test', 'rsf_anomaly_sr_hooks_test', 'rsf_anomaly_geometry_hooks_test',
    'rsf_anomaly_motion_device_test', 'rsf_anomaly_motion_tess_device_test',
    'rsf_anomaly_motion_tess_chain_device_test', 'rsf_anomaly_motion_far_device_test',
    'rsf_anomaly_fg_device_test', 'rsf_anomaly_fg_policy_test')
$cpu = '^anomaly_(plugin_contract|lifecycle_contract|bootstrap_contract|motion_contract|motion_unknown_selection|motion_hooks_contract|shader_tool_contract|sr_hooks(_avx)?_contract|geometry_hooks(_avx)?_contract|fg_policy_contract|fg_policy_unknown_selection|input_events_(standard|avx)_contract)$'
foreach ($configuration in 'Debug', 'Release') {
    cmake --build build/anomaly-motion-stage --config $configuration --target $targets --parallel 2
    ctest --test-dir build/anomaly-motion-stage -C $configuration -R $cpu --output-on-failure
}
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
git diff --check
```

MSVC 19.52.37029 and Windows SDK 10.0.26100 built the adapter, the
four motion device fixtures and the FG device fixture in Debug and Release.
Each configuration passed 15 CPU contracts. Four cases inside the shader-tool
suite skipped because the licensed vanilla fixture was absent. Rust 1.100.0
passed formatting, workspace Clippy and 251 locked workspace tests, including
the three new motion-stage cases.

DUMPBIN and the PE/disassembly audit compared both builds to the staged
`8322179a36c7` `ReScaleFrame.Game.Anomaly.dll`. The export directory name and
the full name and ordinal inventory match:

| Ordinal | Export |
| --- | --- |
| 1 | `rsf_anomaly_observation` |
| 2 | `rsf_anomaly_start` |
| 3 | `rsf_get_game_plugin_api` |

Both builds borrow `ReScaleFrame.Runtime.dll` with `GetModuleHandleW`.
The bootstrap session prepare and start queries, the shader-patch API
and all eight motion input-dump functions resolve by name with
`GetProcAddress`. The staged Runtime exports these names. The PE import and
delay-import tables have no Runtime DLL entry. The shader API module and export
lookup call RVAs are `0x4d9f8` and `0x4da11` in the baseline, `0x26dcf6`
and `0x26dd1e` in Debug, and `0x900b8` and `0x900d1` in Release.

Both 95,049-byte motion packs matched the staged pack byte for byte with
SHA-256 `d2aa077a561ee71c084f806edb2f7cdcb0bf4ccaa17907a788324043f8622bb2`.
The device executables were built for later testing. Live detour admission,
real shader acquisition and reload, motion images and moving-game SR and FG
output remain unverified on this revision.
