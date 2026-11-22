# D3D11 graphics state and composition Rust port

G03 replays `550ea5a7` onto `89f6df5d` in the
`NightHammer1000/port-G03-integrated` worktree. The original branch remains
at `550ea5a7`. The replay preserves D01's Rust output functions and COM
adoption, G02's tap callbacks and declaration linking, G04's fullscreen and
shader compiler paths, and G07's capture sources and target wiring.

The shared Rust COM owner holds the D3D11 bindings. The full scope retains
six shader stages, high SRV slots, constant ranges, UAV bindings and
counters, stream-output cursors, predication, and raster and output state.
Depth replay and fullscreen drawing retain their existing narrow scopes.
Rust owns UI texture allocation and the shared-to-local retry, resource
reference operations, sampler changes, shader dispatch, and reinsertion.
The four composition HLSL payloads use the existing G04 compiler with the
same entry, profile and flags. The Runtime DLL still exports the three
translucency entry points, and embedded callers link the same symbols.

## Findings

| Finding | Source disposition |
| --- | --- |
| Translucency mask session identity | Opening, lookup, same-view closure and cloud adoption include session. Rust ledger cases reuse all the other keys. The native fixture asks for a foreign session's masks before the original scope merges its layer. |
| Depth-domain request predicate | Rust calls the SDK bytewise predicate through `depth_domains_abi.cpp`. The existing signed-zero and unused-domain coverage remains in the Rust tests. |
| Repeated temporal-mask unbind | Rust issues one unbind before optional readable copy and dispatch. |
| Duplicate composition SRV caches | Regions, reinsert, translucency and scalar exposure share `SourceView`. The key includes the retained texture and view format, and replacement commits after success. Exposure retains its two-entry rotation. |
| Duplicate R32 mask allocation | Region and translucency passes share the texture and UAV constructor. |
| Fixed classifier minimum | AC7 supplies a one-third lower bound. Matches below the legacy band are candidates, and the tap corroborates the colour, motion and depth extents. Known render extent takes precedence. The Rust resource-role fixture covers the smaller extent, minimum and known-extent override. |

## PORT dispositions

- Rust replacements for `d3d11_state.cpp`, `resource_ref.cpp`, `ui_layer.cpp`,
  `com_ref.cpp`, `native_regions.cpp`, `native_regions_com.cpp`,
  `native_reinsert_platform.cpp`, `native_translucency.cpp`,
  `native_translucency_com.cpp`, and `sampler_bias_com.cpp`.
- Removed unused `srv_cache.h` and `sampler_bias_bridge.h`. The former's live
  successors are the Rust caches, and the latter's fixture uses the Rust entry.
- Retained declarations for `d3d11_state.h`, `d3d11_state_private.h`,
  `resource_ref.h`, `resource_roles.h`, `srv_format.h`, `ui_identify.h`,
  `ui_layer.h`, `native_composition.h`, `native_translucency.h`,
  `native_regions.h`, and `native_reinsert.h`. The SRV header forwards to the
  D01 Rust mapping. Private native region and reinsert callers compile against
  declarations with C linkage. Runtime DLL export directives remain in the
  shared export manifest.
- `native_composition_unload.c` isolates CRT retirement for DLL and embedded
  module unload as a C11 initializer ([leaf closure](rust-core-leaves-closure.md)). `depth_domains_abi.cpp` isolates the public SDK's inline
  depth-domain predicate.
- Fixtures replaced in Rust are `native_reinsert.cpp`, `resource_roles.cpp`,
  `sampler_bias.cpp`, and `ui_identify.cpp`. The first three keep their CTest
  names and link through the existing graphics fixture entry. The registry
  cases run as Rust unit tests.
- Native fixtures still in C++ at the initial replay were `d3d11_state.cpp`, `native_translucency.cpp`,
  `native_translucency_abi.cpp`, and `ui_layer.cpp` under `tests/`. These
  retained their device and DLL/embedded link coverage.

## Checks

Source parity checks cover state/resource exports, D3D vtable members, shader
bytes, unique output ownership, the preserved integrated paths and fixture
targets. Rust 1.94.1 formatting and locked offline Windows metadata passed.
The G03 resource-role and output bindings regenerated with bindgen 0.72.1.
The repository-wide binding run stops on D01's admission invocation because
its frontend include list cannot find `rescaleframe/game_frame.h`. The targeted
G03 invocation succeeds and unrelated generated outputs were discarded.
Builds, lints, fixtures, device and game runs are pending.

## Fixture and C11 follow-up from 67cbc699

The eight G03 fixture bodies now live in Rust. This follow-up replaces the four
remaining C++ fixture bodies and retains their CTest names and skip policies.

| PORT entry under `tests/` | Disposition |
| --- | --- |
| `d3d11_state.cpp` | Replaced by `d3d11_state.rs` in the linked graphics fixture archive. It binds and reads real render targets, pixel and compute SRVs, samplers, pixel and vertex constant buffers, topology, rasterizer, viewport and scissor state. It exercises independent and nested saves, fresh save after unwind and the narrow fullscreen scope. |
| `ui_layer.cpp` | Replaced by `ui_layer.rs` in the linked graphics fixture archive. It uses actual RGBA8 staging readback, ring and written-frame checks, dynamic color constants, shader draws and three blend states. It checks transparent clear, unpatched and masked alpha, and patched alpha-over at one and two draws. |
| `native_translucency.cpp` | Replaced by the WARP fixture in `tests/translucency/src/lib.rs`. It imports the Runtime DLL and reads the physical mask, snapshot, layer and cloud textures. Session and frame refusal, layer merge, cloud admission and borrowed cloud depth across the next composite remain covered. |
| `native_translucency_abi.cpp` | Replaced by the CPU-only body in the same external fixture crate. One target imports the Runtime DLL and the other links OrchestratorCore with no DLL import. Both use the same fixture body and the same generated mask record as the runtime, including the 56-byte layout and motion-depth offset. |

The separate DLL fixture archive has no dependency on the Rust orchestrator.
Its CMake targets use the Runtime import library without inheriting the Runtime
target's public embedded-core dependencies. The embedded fixture links the
object target and uses the same C entry launcher.

`d3d11_state_private.h` was retired because its C++ fixture was the last
include. The independent state entry still serves the Rust fullscreen pass
and the linked fixture. `depth_domains_abi.cpp` became a C11 Cargo native
object using the same SDK inline predicate. The module-unload C++ shim stays
in OrchestratorCore for both DLL and embedded teardown.

Pinned Rust 1.94.1 formatting, bindgen 0.72.1 regeneration of the split
translucency ABI, C11 predicate parsing and locked offline Windows metadata
passed. Source checks compare Windows API names, fixture cases and linkage,
ABI layout, and the unchanged CRT unload shim. No builds, fixture runs,
lints or device work were performed.
