# AC7 native renderer in Rust

A01 closes the last first-party C++ of the AC7 renderer: `games/ac7/src/native_renderer.cpp`
(2716 lines at `9c8fe985`) and its fixture `tests/ac7_native_graph.cpp`. The question was whether
the renderer object, the 65 detours, their engine relays, pass leases and TrueSky relay handlers
could move into the existing `rsf-ac7` crate without a behaviour change for the shipping Steam
9855922 build. The sites, offsets and expected bytes are unchanged; their evidence stays in
[the hook map](ue418-hook-map.md), [the site note](rust-ac7-native-sites.md) and
[engine.json](../../games/ac7/engine.json). No game, device or GPU fixture ran for this change.

## Layout

| Former C++ | Rust |
| --- | --- |
| Renderer object, `Hook` enum, MinHook `Site` records, guards, typed reads, pass leases, retired list | `games/ac7/src/native_renderer.rs` |
| Tick, simulation, input markers, frame tasks, RHI frame and family scopes, renderer identity, screen classification | `native_renderer/frame.rs` |
| SR graph node and vtable, graph plan, pass Process and descriptor hooks, post-process velocity and final scene scopes, generated uniforms | `native_renderer/graph.rs` |
| Owned view rectangles at construction, temporal sample phases around visibility | `native_renderer/views.rs` |
| Base pass, translucency and its scene colour lease, UnmodifiedTranslucency producer, translucent mesh identity, mesh particle bindings, scene colour precision | `native_renderer/translucency.rs` |
| Beam, ribbon and trail curve tags | `native_renderer/particles.rs` |
| Contact-shadow correction at the pixel shader factory | `native_renderer/shaders.rs` |
| Pixel uniform evidence hooks | `native_renderer/evidence.rs` |
| Widget raster targets | `native_renderer/widgets.rs` |
| Slate window and final scene texture bind scopes | `native_renderer/slate.rs` |
| TrueSky depth dispatch and composite_tile relays, relay memory, TrueSky leaves, sky render and projection | `native_renderer/sky.rs` |
| `renderer_leaves()`, detour binding, code compare and protected write leaves | `native_renderer/leaves.rs` |
| `tests/ac7_native_graph.cpp` | `tests/ac7_native_graph.rs` (crate module under `warp-fixture`) and the C11 launcher `tests/ac7_native_graph.c` |

`renderer.rs`, `truesky.rs`, `sites.rs` and `patches.rs` still receive the same
`rsf_ac7_renderer_leaves`, `rsf_ac7_truesky_leaves` and `rsf_ac7_code_leaves` tables, now filled
by Rust. `ac7_sites.h` keeps only C declarations.

Codex left an uncommitted probe ABI (`cloud_depth/dispatch.rs` and a
`rsf_ac7_truesky_depth_probe` struct in `cloud_depth_platform.h`) for a C++ dispatch relay. With
the relay in Rust, the binding checks from that file live in `sky.rs`, and the probe struct and its
generator entry were withdrawn.

## What stays native and why

- **Guarded engine reads.** `rsf_ac7_copy_memory` and `rsf_ac7_read_memory` stay the SEH leaf
  `ac7_memory.c`. The archive's interface now links `rsf_ac7_memory`, because crate code calls it.
- **The shader factory exception.** The former code caught a C++ exception from the engine's
  pixel shader factory and logged `native shader factory failed`. Rust cannot catch a foreign
  exception, so `native_renderer_platform.c` calls the factory under `__except` filtered on the
  MSVC C++ exception code `0xE06D7363`, matching `catch (...)` under `/EHsc`. Access violations
  still propagate. No other engine call was contained before and none is now. The other former
  `try` blocks only caught `std::bad_alloc`. Rust uses fallible allocation and `try_reserve` with
  the same refusals.
- **MinHook, input facts, scope creation.** These are called through a run-time table
  (`rsf_ac7_native_calls`, defined in `native_renderer_platform.c`) that `plugin.c` binds with
  `rsf_ac7_native_renderer_bind` before publishing the GameSDK table. The fixture launcher binds
  the same table. A first version called them as link-time externs, which broke every other archive
  consumer: in Debug, shared generics place instances in native renderer objects, so
  `rsf_ac7_view_test`, `rsf_ac7_ui_rules_test` and the carriers pulled those objects and failed on
  `MH_EnableHook` and the other missing symbols. With the table, each consumer's link lists exactly
  the externals it had before the port.

## Behaviour notes

Each detour keeps its signature, guard kind (entry or outer), offsets, conditions, engine call
order, refusal and log text. Points where the Rust differs by construction:

- Allocation failure. `new (std::nothrow)` becomes a fallible Rust allocation with the same
  refusal. The window and texture binding leases now use the owned COM reference itself as the
  lease object, so they no longer allocate. A bloom walk allocation failure now frees the
  unregistered SR node; the former exception path leaked it.
- Bounded state. The graph plan's consumer list is a 64-entry array, which the 64-step walk
  already bounded. Thread-local state has no destructor, so hooks still reach it during thread
  teardown.
- Temporal phases. `std::clamp(std::round(x), 8, 255)` of a NaN from an empty rectangle was
  undefined and later divided by. It now keeps the stock eight phases.
- Diagnostics. The null View uniform caller uses `RtlCaptureStackBackTrace(1, ...)` in place of
  `_ReturnAddress()`. Frame 0 of the null pixel uniform stack is now inside the Rust detour.
  Report counters are atomics, and each pixel enqueue hook keeps its own budget, as each template
  instance did. `%p` prints as sixteen uppercase hex digits, as MSVC printed it, and formatted
  lines keep their former buffer limits.
- TrueSky depth relay. Shader readiness is read under the cloud lock and the relay uses its own
  shader reference, instead of reading the field after the lock is released.
- The renderer leaf snapshot refuses a null output pointer.

## Verification

- `cargo build -p rsf-ac7 --locked` and `--features warp-fixture`: pass.
- `cargo clippy -p rsf-ac7 --all-targets --locked`, with and without `warp-fixture`: no warning in
  the new or changed AC7 files. Pre-existing warnings remain elsewhere, and `-D warnings` stops
  earlier in `rsf-game-sys` with the installed 1.100 clippy.
- `rustfmt --edition 2024 --check` on the new modules, `lib.rs`, `guards.rs` and `sites_abi.rs`:
  pass.
- `cargo test -p rsf-ac7` does not link on Windows at the tip. The staticlib's externs, such as
  `rsf_compile_system_shader`, resolve only in the CMake link.
- `cmake --preset windows-x64-vs18`, with the pinned yyjson copied from the main checkout because
  the sandbox download differed, and two local fixes to the known tip CMake breakage that were
  reverted afterwards.
- `cmake --build --preset windows-debug-vs18 --target rsf_game_ac7`: compiles plugin.c,
  `native_renderer_platform.c` and the render scope library. The link reports only six
  pre-existing externals of the A03 to A05 modules (`rsf_d3d11_state_save/restore`,
  `rsf_read_constant_buffer`, `rsf_dump_texture_bytes`,
  `rsf_frame_tap_set_research_callbacks` and `..._phase_callbacks`). The same six appear with the
  renderer reference removed from plugin.c, and in `rsf_ac7_view_test` and
  `rsf_ac7_ui_rules_test`. They need `ReScaleFrame::Graphics`, which needs the broken
  `rsf-orchestrator`.
- `rsf_ac7_native_graph_test` needs `rsf_orchestrator` for `rsf_native_sr_evaluate`, so it cannot
  build at the tip. A temporary probe target linked the same launcher and fixture with a stub
  returning `RSF_DLSS_PIPELINE_ERROR_NOT_RUNNING` and abort stubs for the six externals. It exited
  0 with no failed check on WARP. Changing the SR node's descriptor format from 10 to 11 made it
  fail with `SR graph keeps linear floating point output`, so the checks are live. The probe and
  the stubs were not committed.
- bindgen-cli 0.72.1 and libclang are not installed here. `sites_abi.rs` carries the new
  `rsf_ac7_native_calls` struct in bindgen's layout with size and offset assertions, and the
  generator allowlist names it. Regenerate to confirm the byte-identical output.

Still pending: the real `ac7_native_graph` CTest entry and the game DLL link once the tip builds,
then device and game validation.
