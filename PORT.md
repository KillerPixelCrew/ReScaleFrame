# Remaining C++ port checklist

The **62 agent assignments** below cover the 804 port inventory entries from `de8b7510` and all 508 open Rust review findings from `1605ae07` on `rust/orchestrator-anomaly`. Each item belongs to one chunk. Paths are relative to the repository root.

## Picking up a chunk

1. Choose a chunk from the [assignment index](#assignment-index) and use its ID when dispatching the work. Its file checkboxes, fixtures and Rust findings form the brief for one agent. Within a chunk, address the high-severity findings first.
2. The primary Rust citation or native file path identifies the owner. Read other cited locations as evidence. If a fix also changes a file assigned to another chunk, agree on one writer and hand off the caller changes. Coordinate edits to shared crate roots, build targets and binding generators the same way.
3. Implement related findings together when they share a cause. Check off each original entry only after its own evidence supports closure. Report the commit, changed paths, closed items and anything still pending. The user owns build and test execution.

### Chunks with high-severity findings

| Chunk | First issue to investigate | High findings |
| --- | --- | ---: |
| [F01 · Fallout 4 startup and CPU ownership](#f01) | Fallout 4 worker address shadows the worker function. | 1 |
| [R02 · RDR2 camera, renderer and presentation](#r02) | RDR2 unit test constructs a struct with a field private to another module. | 1 |
| [N07 · Anomaly SR swap, jitter and temporal history](#n07) | Engine fields may still reference twin resources after a failed restore. | 1 |
| [W01 · Project Wingman integration](#w01) | Wingman compute packet lifetime can exceed its render graph. | 1 |
| [U02 · Unity package session and routing](#u02) | Unity window subclass teardown can break the forwarding chain. | 1 |
| [U04 · Unity Vulkan bridge](#u04) | A Unity Vulkan resize can replace a shared image while GPU work still uses it. | 1 |
| [G02 · Frame tap, constants and scene promotion](#g02) | Frame-tap slots, overridden constants, pending Map pointers and promoted views share restoration obligations. | 4 |
| [V04 · Streamline and NGX Vulkan](#v04) | Vulkan capture allocation failure can close the same native owner twice. | 1 |

## Assignment index

Files include implementation, header, fixture and supporting entries. The findings column counts open Rust review entries. Each link opens the complete assignment.

| Area | Chunk | Files | Findings |
| --- | --- | ---: | ---: |
| Core, loader and overlay | [C01 · GameSDK, contracts and plugin sessions](#c01) | 17 | 3 |
|  | [C02 · Render host and frontend](#c02) | 12 | 14 |
|  | [C03 · Shared settings and utilities](#c03) | 6 | 4 |
|  | [C04 · Generation policy, frame identity and leases](#c04) | 28 | 8 |
|  | [C05 · CPU receipts, latency and pacing](#c05) | 19 | 12 |
|  | [L01 · Loader entry and carrier forwarding](#l01) | 18 | 0 |
|  | [L02 · Loader census and diagnostics](#l02) | 23 | 0 |
|  | [O01 · Shared overlay input and windows](#o01) | 13 | 4 |
|  | [O02 · Shared overlay rendering and panel](#o02) | 20 | 23 |
| Graphics and presentation | [G01 · Graphics observers and device identity](#g01) | 14 | 3 |
|  | [G02 · Frame tap, constants and scene promotion](#g02) | 12 | 10 |
|  | [G03 · Graphics state, composition and resource roles](#g03) | 31 | 6 |
|  | [G04 · Colour, depth, motion and fullscreen passes](#g04) | 31 | 5 |
|  | [G05 · Sharpening and shader assets](#g05) | 24 | 4 |
|  | [G06 · Shader patching and compilation](#g06) | 7 | 4 |
|  | [G07 · Texture and frame capture](#g07) | 18 | 13 |
|  | [D01 · D3D11 rendering host](#d01) | 8 | 8 |
|  | [D02 · D3D11 presentation and shared surfaces](#d02) | 12 | 4 |
|  | [D03 · SR sessions and bridging](#d03) | 19 | 11 |
|  | [D04 · D3D12 SR and generation hosts](#d04) | 12 | 16 |
|  | [D05 · D3D11 and D3D12 DLSS](#d05) | 13 | 12 |
|  | [D06 · D3D12 Streamline generation and compatibility](#d06) | 15 | 7 |
|  | [D07 · D3D12 FidelityFX and XeSS](#d07) | 13 | 1 |
|  | [D08 · OpenGL interop and rendering](#d08) | 5 | 10 |
|  | [V01 · Vulkan SR and provider admission](#v01) | 12 | 10 |
|  | [V02 · Vulkan FG and shared WSI](#v02) | 15 | 4 |
|  | [V03 · Vulkan DX12 interop and generation](#v03) | 7 | 5 |
|  | [V04 · Streamline and NGX Vulkan](#v04) | 21 | 15 |
|  | [V05 · FidelityFX Vulkan bridges](#v05) | 27 | 13 |
|  | [V06 · XeSS Vulkan bridges](#v06) | 15 | 7 |
| Games | [A01 · AC7 carrier and renderer lifecycle](#a01) | 10 | 9 |
|  | [A02 · AC7 render scopes, views and scene effects](#a02) | 19 | 8 |
|  | [A03 · AC7 F9 motion capture](#a03) | 7 | 7 |
|  | [A04 · AC7 velocity shader transforms](#a04) | 5 | 7 |
|  | [A05 · AC7 velocity pass and history](#a05) | 7 | 7 |
|  | [N01 · Anomaly startup and observation](#n01) | 23 | 8 |
|  | [N02 · Anomaly input and overlay](#n02) | 14 | 4 |
|  | [N03 · Anomaly geometry](#n03) | 10 | 3 |
|  | [N04 · Anomaly motion and foliage](#n04) | 23 | 12 |
|  | [N05 · Anomaly shader loading and temporal patches](#n05) | 9 | 2 |
|  | [N06 · Anomaly SR stage and reconstruction](#n06) | 18 | 18 |
|  | [N07 · Anomaly SR swap, jitter and temporal history](#n07) | 12 | 9 |
|  | [N08 · Anomaly frame generation](#n08) | 9 | 8 |
|  | [F01 · Fallout 4 startup and CPU ownership](#f01) | 7 | 11 |
|  | [F02 · Fallout 4 render inputs and resolution](#f02) | 15 | 10 |
|  | [F03 · Fallout 4 render hook transactions](#f03) | 3 | 12 |
|  | [R01 · RDR2 startup, settings and input](#r01) | 10 | 9 |
|  | [R02 · RDR2 camera, renderer and presentation](#r02) | 9 | 12 |
|  | [R03 · RDR2 capture and diagnostics](#r03) | 6 | 11 |
|  | [S01 · Skyrim startup, CPU input and compatibility](#s01) | 12 | 13 |
|  | [S02 · Skyrim rendering and presentation](#s02) | 15 | 12 |
|  | [W01 · Project Wingman integration](#w01) | 10 | 10 |
|  | [M01 · Minecraft native integration](#m01) | 2 | 6 |
| Engine and platform integrations | [I01 · Steam and Windows integration](#i01) | 4 | 2 |
|  | [U01 · Unity Mono startup and configuration](#u01) | 8 | 7 |
|  | [U02 · Unity package session and routing](#u02) | 3 | 9 |
|  | [U03 · Unity D3D11 and D3D12 bridges](#u03) | 8 | 7 |
|  | [U04 · Unity Vulkan bridge](#u04) | 4 | 7 |
|  | [U05 · Unity Editor preview](#u05) | 3 | 3 |
|  | [E01 · Godot engine hooks and render graph](#e01) | 19 | 5 |
|  | [E02 · Godot host services and D3D12](#e02) | 8 | 14 |
|  | [E03 · Godot Vulkan](#e03) | 5 | 10 |
| **Total** | | **804** | **508** |

## Scope

- **375 C++ implementation files**, including 168 under `tests/`. All have the `.cpp` suffix in this revision.
- **418 headers and 2 supporting files** to review with their callers. This includes inline implementations, private callback tables, C ABI declarations and shader data stored in C++.
- **9 additional files** containing C++ patches or a generated C++ test fixture. These entries appear with their owning components.

Check off a path when its C++ has been ported or removed. For a header containing only C declarations or a file containing only data, record that finding when closing the item. Keep any remaining first-party C++ unchecked and identify the exact caller and technical constraint if the port is blocked.

Coverage includes tracked fixtures, tools, and the bundled MFG unlocker sources. Upstream provenance and licensing are recorded in [the dependency notes](docs/dependencies.md). The scope excludes standalone C translation units, shader-only files, build outputs, and untracked dependency and reference checkouts.

## Files by area

| Area | C++ implementations | Headers | Supporting files |
| --- | ---: | ---: | ---: |
| `apps/` | 1 | 0 | 0 |
| `cmake/` | 0 | 0 | 1 |
| `games/` | 67 | 151 | 0 |
| `integrations/` | 17 | 22 | 0 |
| `loader/` | 18 | 14 | 0 |
| `runtime/` | 103 | 218 | 1 |
| `sdk/` | 0 | 4 | 0 |
| `tests/` | 168 | 7 | 0 |
| `tools/` | 1 | 1 | 0 |
| `ui/` | 0 | 1 | 0 |
| **Total** | **375** | **418** | **2** |

## Rust code review

All five reviews covered the 538 tracked Rust files and 244,697 lines at `1605ae07`, including standalone crates, build scripts, tests and generated bindings. Each review ledger contains one file-specific observation for every path in the Git manifest. The source links point to `894a55f9`, which has identical Rust files. Severity reflects source inspection. Build, test and device confirmation remain separate.

| Focus | Files reviewed | High | Medium | Low | Open findings |
| --- | ---: | ---: | ---: | ---: | ---: |
| Quality | 538 | 5 | 23 | 7 | 35 |
| Level of abstraction (Altitude) | 538 | 6 | 158 | 22 | 186 |
| Simplicity | 538 | 0 | 3 | 53 | 56 |
| Duplication | 538 | 0 | 116 | 78 | 194 |
| Architectural boundaries | 538 | 0 | 30 | 7 | 37 |
| **Total findings** | | **11** | **330** | **167** | **508** |

The counts refer to finding entries. Related entries across review areas may share a root cause. One withdrawn Altitude finding is annotated in the Vulkan Streamline chunk and excluded from the counts.

## C++ introduced during the port

A sweep of `git ls-files '*.cpp'` on 2026-11-22 found first-party C++ that the port itself added and that no chunk lists. Each belongs to the chunk named after it.

- [x] [games/project-wingman/src/native_platform.c](games/project-wingman/src/native_platform.c) (W01; C++ removed, C11 SEH leaf for MinHook, the RHI texture probe, the execution report and the GameSDK export)
- [x] [games/skyrim-se/src/render_detours.c](games/skyrim-se/src/render_detours.c) (S02; C++ removed, C11 leaf, `__try/__finally` scopes, filter on 0xE06D7363; observer_platform.h and session_platform.h now hold C declarations only)
- [x] [integrations/steam/src/exception_guards.c](integrations/steam/src/exception_guards.c) (I01; C++ removed, C11 SEH leaf that contains only 0xE06D7363)
- [x] [runtime/graphics/src/observer_exception_guards.c](runtime/graphics/src/observer_exception_guards.c) (G01; C++ removed, C11 SEH leaf; the three private observer headers now hold C declarations only)
- [ ] [runtime/orchestrator/src/d3d11_host_foreign.cpp](runtime/orchestrator/src/d3d11_host_foreign.cpp) (D01)
- [ ] [tests/ac7_render_scope_exceptions.cpp](tests/ac7_render_scope_exceptions.cpp) (A02)
- [ ] [tests/anomaly_motion_contract_main.cpp](tests/anomaly_motion_contract_main.cpp) (N04)
- [ ] [tests/anomaly_sr_swap_profile.cpp](tests/anomaly_sr_swap_profile.cpp) (N07)

## Core, loader and overlay

<a id="c01"></a>

### C01 · GameSDK, contracts and plugin sessions

Resolve shared ABI, session, option and provider contract findings at their owner and port the associated interfaces and fixtures.

**Coordinate shared edits**

- Versioned GameSDK events, host callbacks and session leases ([C02](#c02), [C04](#c04), [C05](#c05)).

**Port files (12)**

- [x] [runtime/contract/include/rescaleframe/backend.h](runtime/contract/include/rescaleframe/backend.h)
- [x] [runtime/contract/include/rescaleframe/backend_registry.h](runtime/contract/include/rescaleframe/backend_registry.h)
- [x] [runtime/contract/include/rescaleframe/frame_generation.h](runtime/contract/include/rescaleframe/frame_generation.h)
- [x] [runtime/contract/src/negotiate.cpp](runtime/contract/src/negotiate.cpp)
- [x] [runtime/orchestrator/include/rescaleframe/plugin_session.h](runtime/orchestrator/include/rescaleframe/plugin_session.h)
- [x] [runtime/orchestrator/include/rescaleframe/runtime.h](runtime/orchestrator/include/rescaleframe/runtime.h)
- [x] [runtime/orchestrator/src/plugin_callbacks.c](runtime/orchestrator/src/plugin_callbacks.c) (C++ removed; C11 SEH leaf that contains only the MSVC C++ exception code 0xE06D7363)
- [x] [runtime/orchestrator/src/rust_export.h](runtime/orchestrator/src/rust_export.h)
- [x] [sdk/game/include/rescaleframe/game_api.h](sdk/game/include/rescaleframe/game_api.h)
- [x] [sdk/game/include/rescaleframe/game_frame.h](sdk/game/include/rescaleframe/game_frame.h)
- [x] [sdk/game/include/rescaleframe/game_plugin_util.h](sdk/game/include/rescaleframe/game_plugin_util.h)
- [x] [sdk/game/include/rescaleframe/game_renderer.h](sdk/game/include/rescaleframe/game_renderer.h)

**Fixtures (6)**

- [x] [tests/negotiate.cpp](tests/negotiate.cpp)
- [x] [tests/orchestrator_domains.cpp](tests/orchestrator_domains.cpp)
- [x] [tests/plugin_contract.cpp](tests/plugin_contract.cpp)
- [x] [tests/plugin_session.cpp](tests/plugin_session.cpp)
- [x] [tests/plugin_session_fixture.cpp](tests/plugin_session_fixture.cpp)
- [x] [tests/plugin_session_fixture_throw.c](tests/plugin_session_fixture_throw.c) (deliberate foreign exception fixture; C11, raises the C++ exception code since C cannot throw an object)

**Rust review findings (3)**

- [x] **Altitude | Medium | Make the negotiation contract's fallback independent of capability-table order.** backend_registry.h permits callers to order capability records arbitrarily. With no preferred vendor, choose_fg and choose_sr use the first available matching record, so permuting identical capabilities changes the selected provider. The current consumer is tests/negotiate.cpp via the C shim. Apply an explicit deterministic preference among viable candidates (or declare caller-supplied ranking) and test the same request with reordered records. [runtime/contract/src/lib.rs:117](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/contract/src/lib.rs#L117)

- [x] **Boundaries | Low | Retire or wire up the unused vendor negotiation contract.** rsf_contract_negotiate implements a vendor-neutral SR/FG/UI/latency choice, but its only native caller is the forwarding shim rsf_negotiate in runtime/contract/src/negotiate.cpp:12, and rsf_negotiate is called only by tests/negotiate.cpp. The Runtime actually chooses SR backends through rsf_gpu_select_sr_backend (runtime/orchestrator/src/lib.rs:385, used by callback_host.rs:798, d3d11_host.rs:663 and render_host.rs:529) and FG providers through fg_choice.rs. AGENTS.md Rule 3 asks for one implementation of backend choice; here a second, divergent policy (bridge routing, UI-mode fallback, NEEDS_RESTART) is maintained and tested while production follows another, and the only reason negotiate.cpp remains first-party C++ is to rename one symbol. Either route the orchestrator backend choice through this contract and export rsf_negotiate directly from Rust, or delete the crate, the shim, backend_registry.h negotiation types and tests/negotiate.cpp. [runtime/contract/src/lib.rs:192](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/contract/src/lib.rs#L192)

- [x] **Duplication | Low | Share Windows absolute-path admission for plugin and FidelityFX loading.** Plugin preparation, FSR4 FG probe/create at runtime/orchestrator/src/fsr4_fg_bridge.rs:711 and the standalone FSR4 SR owner at runtime/vulkan/fsr4/owner.rs:287 all hand-parse the same UTF-16 drive or UNC prefix. Each accepts a colon and separator after any first code unit, so even `1:/vendor` passes this gate and only fails downstream as a loader error. Vulkan timing implements the prefix yet again at runtime/orchestrator/src/vulkan_timing.rs:27 with its own length limit. Use one Windows path-component predicate for a rooted drive or UNC path, retaining caller-specific bounds and the SR owner's optional default-directory route. [runtime/orchestrator/src/plugin_session.rs:110](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/plugin_session.rs#L110)

<a id="c02"></a>

### C02 · Render host and frontend

Port control and rendering callbacks, session selection and frontend wiring across linked and DLL consumers.

**Coordinate shared edits**

- Versioned GameSDK events, host callbacks and session leases ([C01](#c01), [C04](#c04), [C05](#c05)).
- Shared settings, frontend requests and live overlay state ([C03](#c03), [O02](#o02)).

**Port files (8)**

- [x] [runtime/orchestrator/include/rescaleframe/callback_render_host.h](runtime/orchestrator/include/rescaleframe/callback_render_host.h)
- [x] [runtime/orchestrator/include/rescaleframe/native_window.h](runtime/orchestrator/include/rescaleframe/native_window.h)
- [x] [runtime/orchestrator/include/rescaleframe/render_frontend.h](runtime/orchestrator/include/rescaleframe/render_frontend.h)
- [x] [runtime/orchestrator/include/rescaleframe/render_host.h](runtime/orchestrator/include/rescaleframe/render_host.h)
- [x] [runtime/orchestrator/include/rescaleframe/render_links.h](runtime/orchestrator/include/rescaleframe/render_links.h)
- [x] [runtime/orchestrator/src/callback_render_host_platform.cpp](runtime/orchestrator/src/callback_render_host_platform.cpp)
- [x] [runtime/orchestrator/src/native_window_platform.cpp](runtime/orchestrator/src/native_window_platform.cpp)
- [x] [runtime/orchestrator/src/render_host_platform.c](runtime/orchestrator/src/render_host_platform.c) (C++ removed; C11 SEH leaf for the Streamline native-interface call)

**Fixtures (4)**

- [x] [tests/callback_host_abi.cpp](tests/callback_host_abi.cpp)
- [x] [tests/native_window.cpp](tests/native_window.cpp)
- [x] [tests/render_frontend.cpp](tests/render_frontend.cpp)
- [x] [tests/render_links.cpp](tests/render_links.cpp)

**Rust review findings (14)**

- [x] **Boundaries | Medium | Take the engine spatial upscaler ratios from the adapter, not a Runtime table.** The shared callback render host hard-codes "the engine's spatial upscaler scale by quality index" (SPATIAL_RATIOS = 1.0/1.5/1.7/2.0/3.0/1.3) and, for backend 6, derives the render extent it publishes through render_config from that table (prepare at :812-818), logging "FSR1 selected through the engine's own spatial upscaling path". The only consumer is Unity Mono (games/unity-mono/src/runtime_host.cpp:54), so these are Unity URP spatial-upscaler facts living in the Runtime; the adapter's set_engine_spatial callback (games/unity-mono/src/bridge_state.rs:295) is told only (spatial, quality) and never reports the extent its engine actually renders at. AGENTS.md Rule 1 keeps engine quality conventions in the integration and Rule 3 has the integration report facts. If a second engine adopts this host, or URP changes a preset, the Runtime publishes a render size the engine does not use. Have set_engine_spatial (or a new appended field) return the engine's render extent for the requested quality, and drop the ratio table from callback_host.rs. [runtime/orchestrator/src/callback_host.rs:65](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/callback_host.rs#L65)

- [x] **Duplication | Medium | Use one callback-render-host setup binding in Unity and the runtime.** games/unity-mono/src/runtime_abi.rs:31 generates the 128-byte rsf_callback_host_setup from the public header, including offsets for each directory and callback. games/unity-mono/src/runtime_setup.rs:318 fills it for the call at :339. runtime/orchestrator/src/callback_host.rs:73 redeclares the same fields manually and consumes them at :949 and :990. Same-sized vendor path pointers are easy to exchange without changing the record size, leaving the Rust constructor and reader independently type-correct. Put the generated public setup in shared bindings and use it at runtime admission, leaving each side's private platform table local. [runtime/orchestrator/src/callback_host.rs:73](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/callback_host.rs#L73)

- [x] **Duplication | Medium | Bind SR12 command lists to recording slots with one allocator.** runtime/orchestrator/src/callback_host.rs:222 and games/unity-mono/src/package_views.rs:57 have the same search-reuse-or-bind algorithm over three nullable command-list pointers. The callback host calls it for FG input normalization and SR at :711 and :719, while the Unity package calls it per view at games/unity-mono/src/package_state.rs:688. Both must return the original slot for a known list and refuse a fourth distinct list. Share a small three-slot binding type and its exhaustion/reuse tests, with a separate instance for every existing host/view and the same reset boundaries. [runtime/orchestrator/src/callback_host.rs:222](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/callback_host.rs#L222)

- [x] **Altitude | Medium | Request an adapter drain for a preset-only DLSS change.** The overlay preset path marks the host dirty and disables its plan, but only backend/quality changes call the adapter policy callback. At unchanged output size the host rebuilds only on a RECONFIGURE pass, and Unity bridge.cpp:206 emits that flag only for a pending bridge policy change. A preset-only edit can leave the old context active and the plan disabled indefinitely. Give the host a generation-specific drain request for every context change and acknowledge it before rebuilding. [runtime/orchestrator/src/callback_host.rs:618](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/callback_host.rs#L618)

- [x] **Duplication | Medium | Apply one backend/quality admission rule at callback-host startup and in the overlay.** runtime/orchestrator/src/callback_host.rs:934 accepts quality 5 for a non-DLSS request, and callback_host/tests.rs:853 explicitly starts the spatial backend at that quality. The live intent path at callback_host.rs:640 requires quality <= 4 even for an enable-only change, where it uses the stored quality. Thus disabling or restarting an admitted quality-5 spatial session through the panel silently does nothing until the user changes quality. Share a predicate for concrete backend and quality, preserve the startup-only Auto handling, and exercise an enable-only change from the existing quality-5 fixture. [runtime/orchestrator/src/callback_host.rs:640](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/callback_host.rs#L640)

- [x] **Altitude | Medium | Confirm FG submission with the captured source ID instead of the native frame counter.** Host::scene records both pass.native_frame and pass.source_frame_id, and fg_hudless and fg_window receive the latter. On queue completion, submit calls fg_submitted(self.session, slot.native_frame). fg12.rs rejects a receipt unless it matches the stored source ID, so adapters whose engine and input counters differ cannot retire or present those FG inputs correctly. Use the matched scene's source and session for FG and keep the native counter for locating the recording. [runtime/orchestrator/src/render_host.rs:951](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/render_host.rs#L951)

- [x] **Altitude | Medium | Give Present a published settings snapshot instead of reading the INI every frame.** Host::poll already refreshes saved render settings on a one-second cadence and after a successful write. Host::overlay reads the same INI section again on every Present whenever the state lock is available, before deciding whether the panel or HUD will draw. settings_get and status_get also perform I/O while holding that graphics lock. Publish the validated settings on poll/apply, then let frame and status readers copy that snapshot without putting file access on the presentation path. [runtime/orchestrator/src/render_host.rs:1289](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/render_host.rs#L1289)

- [x] **Boundaries | Medium | Give the render host's INI-only diagnostic keys live overlay controls.** render_host.rs:1641-1655 reads OverlayProbe and SRCaptureDirectory, and render_host.rs:1708 reads GraphicsDiagnostics, straight from the adapter's INI section once at host creation. They gate the overlay probe (cursor-hide loop and panel open/close), bounded SR capture (sr12_capture at :741, fg_capture_outputs at :1769) and rsf graphics diagnostics, but none of them is surfaced through rsf_overlay_stats/host_options or rsf_render_settings, and they are not re-read by poll() (:539), so they can only be changed by editing the INI and restarting the game. AGENTS.md Rule 2 requires every option, including diagnostics, to have a live overlay control through the generic overlay service; these keys are configuration the Runtime owns but exposes only as restart-time file edits. [runtime/orchestrator/src/render_host.rs:1641](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/render_host.rs#L1641)

- [x] **Duplication | Low | Use one SR backend display-name lookup for the shared overlay.** runtime/orchestrator/src/callback_host.rs:272 and runtime/orchestrator/src/overlay_service.rs:775 both translate a backend ID into the overlay's backend_name at :432 and :820 respectively. The temporal provider labels match, but ID 6 appears as FSR1 on the callback path and Spatial on the service path, while ID 0 appears as Off or None. Give shared SR backend metadata one canonical display label, and pass an engine-specific spatial algorithm label explicitly only when the adapter can identify it. [runtime/orchestrator/src/callback_host.rs:272](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/callback_host.rs#L272)

- [x] **Simplicity | Low | Callback host applies sharpening twice when a context is created on a due poll** prepare() calls sr12_set_sharpening with s.sharpening right after a successful sr12_create (callback_host.rs:859). scene() then continues on the same pass to the poll at :722-727, whose deadline is still 0 on the first pass after start (sharpening_poll initialised to 0 at :969), so it re-reads the preferences INI through read_sharpening and calls sr12_set_sharpening again with the same values. The tests pin the duplication: the first pass emits "sharpen 1 0.4" twice before "evaluate" (callback_host/tests.rs:791-793, :1320). Simpler: one apply path, e.g. prepare sets sharpening_poll to now + SHARPENING_POLL_MS after applying, or prepare leaves sharpening to the poll by resetting the deadline to 0 and the scene path applies it once. Tradeoff: none for behavior; removes one file read and one SDK call per context creation and makes the order of sharpening updates single-sourced. [runtime/orchestrator/src/callback_host.rs:859](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/callback_host.rs#L859)

- [x] **Simplicity | Low | render_host keeps the SR context key as eight loose State fields compared and stored one by one** State (render_host.rs:253-266) holds context_backend, quality, context_preset, context_exposure, width, height, output_width and output_height only to describe the live SR context. process() rebuilds the key implicitly and compares it field by field in a nine-clause condition (:819-827), configure() writes the same eight fields one assignment at a time (:753-760), and State::new zeroes each (:423-430). Simpler: #[derive(Clone, Copy, PartialEq)] struct ContextKey { backend, quality, preset, exposure: bool, render: (u32, u32), output: (u32, u32) }; process builds it once from s and the pass, calls configure when s.context.is_null() || s.key != key, and configure stores s.key = key. status() and the log line read s.key.render / s.key.output. Tradeoff: status() reports width/height and s.quality (overlay quality at :1276) before any context exists as zeros, which a Default key keeps; the comparison stays exact, so behavior is unchanged. [runtime/orchestrator/src/render_host.rs:819](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/render_host.rs#L819)

- [x] **Duplication | Low | Share the base and output FPS sampler across runtime frontends.** RenderHost::status maintains a tick, two counter baselines, backend identity, FPS values and an output-valid flag, with the same 500 ms sampling window and reset conditions as Rates::sample in runtime/orchestrator/src/overlay_service.rs:362. The D3D12 frontend and its panel call this copy at render_host.rs:1177 and :1292; overlay_service records its copy during accepted Presents. Their application-present deltas already differ in ordinary versus wrapping subtraction. Move the existing Rates object to a shared module, passing each owner's measured counters, backend, validity and tick, and exercise both callers' reset cases with one sampler test. [runtime/orchestrator/src/render_host.rs:1049](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/render_host.rs#L1049)

- [x] **Simplicity | Low | render_host and overlay_service each map the overlay intent onto rsf_render_settings and back, field by field** render_host.rs:1426-1492 apply_intent and overlay_service.rs:668-716 settings_intent are the same rsf_overlay_intent -> rsf_render_settings mapping (sharpening block, sr_backend, sr_enabled, sr_quality, dlss_preset, fg_backend/fg_enabled, fg_mode/generated_frames, latency, frame_limit_us, status_overlay), written twice; callback_host.rs:581-595 repeats the 12-field intent -> Sharpening copy a third time. The reverse direction is mirrored too: render_host.rs:1323-1334 copies the 12 sharpening_* fields from Settings to rsf_overlay_stats one assignment at a time, and overlay_service.rs:902-914 does the same in a struct literal. Simpler: put two helpers beside sharpening_of/set_sharpening in settings_store.rs (or overlay_abi) - fn apply_intent(&mut Settings, &rsf_overlay_intent) -> bool and fn sharpening_stats(&mut rsf_overlay_stats, &Settings) (or intent_sharpening(&Intent, base) -> Sharpening) - and call them from all three hosts, removing roughly 60 lines of parallel assignments. Tradeoff/behavior to keep: the two copies differ today - overlay_service normalizes enabled and performance_hud to 0/1 while render_host passes them raw and lets settings_apply (render_host.rs:1190-1192) reject values >1; render_host also reports whether anything changed. The shared helper must pick one rule (keeping render_host's raw copy plus validation preserves both callers' accepted inputs only if overlay_service then validates too), so this is a small behavior decision, not a pure move. [runtime/orchestrator/src/render_host.rs:1426](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/render_host.rs#L1426)

- [x] **Duplication | Low | Stop duplicating conflicting DLSS preset policy in host test fixtures.** The render-host and callback-host fake validators (runtime/orchestrator/src/callback_host/tests.rs:178) admit every preset through 15. OpenGL's fake at runtime/orchestrator/src/gl_pipeline/tests.rs:402 duplicates the actual allowlist in runtime/backends/dlss/include/rescaleframe/dlss_preset.h:7, which rejects 7, for example. The render-host settings test uses 16 as its only invalid preset, so the divergent behavior is invisible. Delegate integration fixtures to one contract implementation or inject explicit accept/refuse results and assert the forwarded preset, with the numeric cases tested once against the production predicate. [runtime/orchestrator/src/render_host/tests.rs:165](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/render_host/tests.rs#L165)

<a id="c03"></a>

### C03 · Shared settings and utilities

Port settings storage, formatting, hashes and shared utilities. Consolidate settings interpretation at the owning layer.

**Coordinate shared edits**

- Shared settings, frontend requests and live overlay state ([C02](#c02), [O02](#o02)).

**Port files (6)**

- [x] [runtime/common/include/rescaleframe/capture_names.h](runtime/common/include/rescaleframe/capture_names.h)
- [x] [runtime/common/include/rescaleframe/log.h](runtime/common/include/rescaleframe/log.h)
- [x] [runtime/common/include/rescaleframe/sha256_file.h](runtime/common/include/rescaleframe/sha256_file.h)
- [x] [runtime/orchestrator/src/render_settings_store.cpp](runtime/orchestrator/src/render_settings_store.cpp)
- [x] [runtime/orchestrator/src/render_settings_store.h](runtime/orchestrator/src/render_settings_store.h)
- [x] [runtime/orchestrator/src/settings_store_format.cpp](runtime/orchestrator/src/settings_store_format.cpp)

**Rust review findings (4)**

- [x] **Boundaries | Medium | Move the sharpening range rules into the Rust settings owner.** settings_store.rs:5-6 owns the INI keys but leaves "range checks and sanitization" to the C++ facade, and returns raw stored values. The only definition of rsf_sharpening_defaults, sanitize, valid and vendor_sharpness (including the backend 2..4 vendor-mode rule) is C++ in runtime/orchestrator/include/rescaleframe/sharpening_settings.h. Every Rust owner calls back into C++ for this pure data rule via its platform table: callback_host.rs:149-150/361/597, render_host.rs:164-165/506/1189, overlay_service.rs:345 (overlay_platform.cpp:72), dlss_pipeline.rs:1841, gl_pipeline.rs:149, sr12.rs:1366, vulkan_sr.rs:918, vulkan_fsr4_sr.rs:588 (each with its own *_platform.cpp shim), and the settings store itself needs settings_store_format.cpp for "%.9g". The Rust store and its callers cannot validate or sanitize without first-party C++, so this blocks removing the platform .cpp files and splits one settings policy across two languages. Define the defaults, valid and sanitize in Rust (exported for the C header), and have the Rust owners use them directly. [runtime/orchestrator/src/settings_store.rs:5](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/settings_store.rs#L5)

- [x] **Altitude | Medium | Preserve the selected FG mode across unrelated settings edits.** read_with_fg_mode returns the saved mode alongside fg_enabled, but every full settings write serializes only the boolean into the FG key. With FG=2 or FG=3, changing sharpening or the performance HUD through render_host::settings_apply rewrites FG=1 and loses Auto or Dynamic. Retain the requested mode in the settings model or preserve the existing mode on updates that leave FG enabled; add a round-trip test with an unrelated field change. [runtime/orchestrator/src/settings_store.rs:78](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/settings_store.rs#L78)

- [x] **Simplicity | Low | settings_store spells its 22 keys twice and their field order three times** KEYS (settings_store.rs:46-69) lists the 22 owned key names; settings_values (:72-98) lists the same fields in the same order by position; sharpening_values (:100-116) repeats the last 12 positions (tied to KEYS only by SHARPENING_START = 10); and read_with_fg_mode (:283-308) spells all 22 key strings again as literals next to their fields. The write paths depend on KEYS[i] matching settings_values[i], while the read path does not use KEYS at all, so renaming or reordering a key needs three or four coordinated edits. Simpler: derive the read from the same table - build the fallback row as settings_values(&defaults_with_sharpening), map KEYS.zip(row) through file.integer/file.decimal by Value kind, and unpack with one fn from_values([Value; 22]) -> Settings (the inverse of settings_values); define sharpening_values as the tail of settings_values (or as settings_values(&with_sharpening)[SHARPENING_START..]). Behavior to keep: the read normalizes SR and StatusOverlay to 0/1 and returns the raw FG value beside fg_enabled, so those three stay explicit after the table read. Tradeoff: from_values is itself a 22-line positional unpacker, so the line count barely drops; the gain is a single source for key names and order. [runtime/orchestrator/src/settings_store.rs:280](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/settings_store.rs#L280)

- [x] **Duplication | Low | Use one temporary INI fixture for settings-store and render-host tests.** Settings-store rollback tests and the settings_apply integration fixture at runtime/orchestrator/src/render_host/tests.rs:429 independently create process-and-test named INIs, remove a stale file, encode its path to terminated UTF-16, decode it back for contents and Drop, and silently turn a failed read into an empty string. The render-host version additionally exposes section read/write and runtime_directory. Share a test helper owning the PathBuf and Win32 path, provide a read that distinguishes a missing file from other errors, and keep section-specific helpers on top. This also lets both handle non-Unicode temporary-directory paths without to_str().unwrap(). [runtime/orchestrator/src/settings_store.rs:574](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/settings_store.rs#L574)

<a id="c04"></a>

### C04 · Generation policy, frame identity and leases

Port frame assembly, generation selection and leased inputs while preserving frame identity and callback lifetime.

**Coordinate shared edits**

- Versioned GameSDK events, host callbacks and session leases ([C01](#c01), [C02](#c02), [C05](#c05)).

**Port files (19)**

- [x] [runtime/orchestrator/include/rescaleframe/d3d11_fg_api.h](runtime/orchestrator/include/rescaleframe/d3d11_fg_api.h)
- [x] [runtime/orchestrator/include/rescaleframe/fg_choice.h](runtime/orchestrator/include/rescaleframe/fg_choice.h)
- [x] [runtime/orchestrator/include/rescaleframe/fg_leases.h](runtime/orchestrator/include/rescaleframe/fg_leases.h)
- [x] [runtime/orchestrator/include/rescaleframe/fg_session.h](runtime/orchestrator/include/rescaleframe/fg_session.h)
- [x] [runtime/orchestrator/include/rescaleframe/frame_assembly.h](runtime/orchestrator/include/rescaleframe/frame_assembly.h)
- [x] [runtime/orchestrator/include/rescaleframe/frame_sequencer.h](runtime/orchestrator/include/rescaleframe/frame_sequencer.h)
- [x] [runtime/orchestrator/include/rescaleframe/gpu_policy.h](runtime/orchestrator/include/rescaleframe/gpu_policy.h)
- [x] [runtime/orchestrator/include/rescaleframe/native_fg.h](runtime/orchestrator/include/rescaleframe/native_fg.h)
- [x] [runtime/orchestrator/include/rescaleframe/native_scene.h](runtime/orchestrator/include/rescaleframe/native_scene.h)
- [x] [runtime/orchestrator/src/d3d11_fg_api.cpp](runtime/orchestrator/src/d3d11_fg_api.cpp)
- [x] [runtime/orchestrator/src/fg_auto_choice.h](runtime/orchestrator/src/fg_auto_choice.h)
- [x] [runtime/orchestrator/src/fg_leases_com.cpp](runtime/orchestrator/src/fg_leases_com.cpp)
- [x] [runtime/orchestrator/src/fg_providers.cpp](runtime/orchestrator/src/fg_providers.cpp)
- [x] [runtime/orchestrator/src/fg_session_platform.cpp](runtime/orchestrator/src/fg_session_platform.cpp)
- [x] [runtime/orchestrator/src/native_fg.cpp](runtime/orchestrator/src/native_fg.cpp)
- [x] [runtime/orchestrator/src/native_fg_dispatch.cpp](runtime/orchestrator/src/native_fg_dispatch.cpp)
- [x] [runtime/orchestrator/src/native_fg_platform.cpp](runtime/orchestrator/src/native_fg_platform.cpp)
- [x] [runtime/orchestrator/src/native_fg_platform.h](runtime/orchestrator/src/native_fg_platform.h)
- [x] [runtime/orchestrator/src/native_scene_platform.cpp](runtime/orchestrator/src/native_scene_platform.cpp)

**Fixtures (9)**

- [x] [tests/d3d11_fg_api.cpp](tests/d3d11_fg_api.cpp)
- [x] [tests/fg_choice.cpp](tests/fg_choice.cpp)
- [x] [tests/fg_leases.cpp](tests/fg_leases.cpp)
- [x] [tests/fg_leases_faults.cpp](tests/fg_leases_faults.cpp)
- [x] [tests/fg_provider.cpp](tests/fg_provider.cpp)
- [x] [tests/fg_session.cpp](tests/fg_session.cpp)
- [x] [tests/fg_switching.cpp](tests/fg_switching.cpp)
- [x] [tests/frame_assembly.cpp](tests/frame_assembly.cpp)
- [x] [tests/gpu_policy.cpp](tests/gpu_policy.cpp)

**Rust review findings (8)**

- [x] **Altitude | Medium | Keep a scoped FG scene capture with its scene owner.** The public D3D11 capture entry in native_fg_dispatch.cpp:40 passes three native function pointers into Rust to swap the thread-local scene out, set the new scene, capture and swap the old scene back. Both scene storage and scoped restoration already live in fg_scene.rs, and native_sr.rs already uses its guard. Let the scene owner bracket one capture callback directly, preserving set_scene's explicit refusal behavior and binding rules. This keeps outer-resource lifetime and restoration in one implementation instead of depending on a matched pair of native swaps. [runtime/orchestrator/src/fg_dispatch.rs:66](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/fg_dispatch.rs#L66)

- [x] **Duplication | Medium | Validate the FG provider retirement descriptor identically before polling or sealing.** Session switching inspects the provider retirement fence/value itself, while the lease API validates the same rsf_fg_retirement at runtime/orchestrator/src/fg_leases.rs:221. For a non-null fence targeting UINT64_MAX, sealing rejects the descriptor immediately, but Session::switch waits for a value it can never accept as completed: a smaller counter returns NOT_READY and UINT64_MAX means device removal. These ABI paths are exercised by tests/fg_session.cpp:82 and tests/fg_leases_faults.cpp:98. Parse the descriptor once into immediate or fenced retirement, reject the reserved sentinel up front, and reuse FenceState::check for polling. [runtime/orchestrator/src/fg_session.rs:84](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/fg_session.rs#L84)

- [x] **Altitude | Medium | Record provider marker failure in the owning FG frame sequence.** rsf_fg_session_marker commits the marker to Ledger, then returns the provider's result without marking the sequence failed. If the provider refuses an input-source marker, continuing with later markers can make prepare_source approve the frame, while retrying the failed marker poisons the ledger as an out-of-order duplicate. Make the session explicitly invalidate the frame on provider failure and require abort before reuse. Cover a provider refusal in the public session fixture alongside its existing out-of-order-marker case. [runtime/orchestrator/src/fg_session.rs:405](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/fg_session.rs#L405)

- [x] **Altitude | Medium | Preserve the resources covered by an explicit FG input lease as well as its metadata.** When a matched explicit FG capture already exists, native FG evaluation sets explicit_inputs and preserves slot.pass at line 1144. It nevertheless copies the SR frame's depth and motion into those same slot surfaces, can reallocate them for different extents, and sets slot.prepared from SR success. Prepare can then tag SR pixels under the explicit producer's provenance, or lose an independently valid FG input on SR failure. Give SR staging its own storage or reuse explicit inputs only after proving their resource identity and conventions match. [runtime/orchestrator/src/native_fg.rs:1028](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/native_fg.rs#L1028)

- [x] **Altitude | Medium | Scope measured FG frame time to the source timeline that produced it.** Input-sourced Prepare falls back from engine delta to a process-global previous input timestamp with no session or generation, then substitutes 16.67 ms if there is no usable predecessor. The rendered-source path already declines when neither measurement is available. A first frame or a session transition can therefore send the provider an invented 60 Hz interval or a clamped cross-session gap. Store timing per source timeline, reset it at discontinuities, and make absent timing explicit before interpolation. [runtime/orchestrator/src/native_fg.rs:1587](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/native_fg.rs#L1587)

- [x] **Altitude | Low | Give the FG lease owner a terminal device-removal teardown operation.** The public scaffolding retains the handle and COM references if either fence reports DeviceRemoved. Destroy repeats the failing collection, cancel rejects submitted slots, and tests/fg_leases_faults.cpp:119 only recovers by changing a synthetic removed fence back into a completed one. A real caller cannot release the failed owner after verified device teardown. Add an explicit owner-level device-loss teardown distinct from successful retirement, with a test that leaves the fence removed. There is no production consumer yet. [runtime/orchestrator/src/fg_leases.rs:274](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/fg_leases.rs#L274)

- [x] **Simplicity | Low | Orchestrator owners keep bindgen Option<fn> tables and unwrap a slot on every call** Four orchestrator owners store their raw bindgen Option<fn> tables and unwrap a slot on every call. (1) fg_session.rs: valid_provider (:110-121) and rsf_core_fg_session_create (:250-257) reject any missing provider or host slot, but Context and Session keep the raw rsf_generation_provider and rsf_fg_host and call 16 .unwrap()( sites (:55, :70, :80, :101, :170, :175, :198, :221, :324, :327, :345, :405, :460, :486, :509, :539). (2) fg12.rs:32-44: the native! macro expands to PLATFORM.get().unwrap().op.unwrap() at about 60 call sites (plus 6 in fg12/colors.rs), and rsf_core_fg12_bind_platform only asserts struct_size. (3) dx12_vulkan_interop.rs:118-310: rsf_dx12vk_platform is borrowed &static and its 16 slots are unwrapped per call with no up-front check. (4) auxiliary_sr.rs:892 copies *platform into AuxiliarySr unchecked, then unwraps slots at 19 sites. (5) frame_tap.rs: the bindgen rsf_frame_tap_platform is passed as &Platform into about 20 hook and control exports and unwrapped at 51 sites (e.g. :312, :561, :2066, :2578, :2874), with no up-front check. (6) native_fg.rs:64-72: the call! macro does .expect("native FG platform entry ...") on an rsf_nfg_platform Option slot at each of its 81 call sites, and the table pointer itself is re-checked by platform() on each of about 15 exports; native_fg_cpu.rs:51-57 repeats the same call! .expect macro over rsf_nfg_cpu_platform (69 sites across native_fg_cpu.rs and its submodules). overlay_host.rs:48-50 does the same through leaf(entry) = entry.expect(...) over rsf_overlay_host_platform and the loaded panel API (48 sites). overlay_service.rs:58-65 copies rsf_overlay_platform into a OnceLock without checking any slot and unwraps it at 33 sites; overlay_native12.rs does the same with rsf_overlay12_platform. shader_patch.rs:111-127 stores the bindgen rsf_patch_platform in a OnceLock after only a struct_size assert, then unwraps or expects its leaves at 12 sites (shader_patch.rs:477, pack.rs:177, reflection.rs:124 and nine in shader_patch/d3d11.rs:88-215). Simpler: convert each table once at bind or create time into a Rust struct of plain unsafe extern "C" fn pointers, returning INVALID when a slot is missing. This is the pattern callback_host, d3d11_present and dlss_streamline already use with hand-written plain-fn Platform structs. The per-call unwraps then disappear, and a missing slot fails at bind rather than panicking mid-frame inside an extern "C" callback. Tradeoff: one conversion function per table, and for fg12 a bind result instead of a void assert. This is distinct from the Anomaly (fg_stage.rs), Skyrim and Unity entries, which cover game crates. [runtime/orchestrator/src/fg_session.rs:110](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/fg_session.rs#L110)

- [x] **Duplication | Low | Classify the presentation path from the scene identity already found.** An eligible FG Prepare first obtains final_scene through scene_for_window and immediately invokes scene_present_path with the same window. Both public paths independently validate the pass and call find_window in runtime/orchestrator/src/native_scene.rs:175 and :195. Native window admission repeats that pair at runtime/orchestrator/src/native_window.rs:207 and :214. On a matched frame the same thread-local registry slot is borrowed and copied twice through native wrappers before direct or sampled classification. Provide one internal lookup returning the scene and path, or pass the copied scene to a path classifier while retaining the public single-purpose ABI wrappers. [runtime/orchestrator/src/native_fg.rs:1276](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/native_fg.rs#L1276)

<a id="c05"></a>

### C05 · CPU receipts, latency and pacing

Port CPU event admission, input receipts, vendor latency and pacing decisions, including Vulkan latency paths.

**Coordinate shared edits**

- Versioned GameSDK events, host callbacks and session leases ([C01](#c01), [C02](#c02), [C04](#c04)).

**Port files (12)**

- [x] [runtime/orchestrator/include/rescaleframe/native_cpu.h](runtime/orchestrator/include/rescaleframe/native_cpu.h)
- [x] [runtime/orchestrator/include/rescaleframe/native_latency_vulkan.h](runtime/orchestrator/include/rescaleframe/native_latency_vulkan.h)
- [x] [runtime/orchestrator/include/rescaleframe/native_latency_vulkan_requirements.h](runtime/orchestrator/include/rescaleframe/native_latency_vulkan_requirements.h)
- [x] [runtime/orchestrator/src/latency_amd_vulkan.cpp](runtime/orchestrator/src/latency_amd_vulkan.cpp)
- [x] [runtime/orchestrator/src/latency_amd_vulkan_platform.h](runtime/orchestrator/src/latency_amd_vulkan_platform.h)
- [ ] [runtime/orchestrator/src/latency_antilag.cpp](runtime/orchestrator/src/latency_antilag.cpp)
- [x] [runtime/orchestrator/src/latency_antilag.h](runtime/orchestrator/src/latency_antilag.h)
- [x] [runtime/orchestrator/src/latency_policy.h](runtime/orchestrator/src/latency_policy.h)
- [x] [runtime/orchestrator/src/native_fg_cpu_platform.h](runtime/orchestrator/src/native_fg_cpu_platform.h)
- [ ] [runtime/orchestrator/src/native_latency_streamline_exceptions.cpp](runtime/orchestrator/src/native_latency_streamline_exceptions.cpp) (native Streamline callback exceptions)
- [x] [runtime/orchestrator/src/native_latency_vulkan.cpp](runtime/orchestrator/src/native_latency_vulkan.cpp)
- [x] [runtime/orchestrator/src/native_latency_vulkan_presentation.h](runtime/orchestrator/src/native_latency_vulkan_presentation.h)

**Fixtures (8)**

- [x] [tests/input_receipts.cpp](tests/input_receipts.cpp)
- [x] [tests/latency_antilag.cpp](tests/latency_antilag.cpp)
- [x] [tests/latency_event_contract.cpp](tests/latency_event_contract.cpp)
- [x] [tests/native_antilag_vulkan.cpp](tests/native_antilag_vulkan.cpp)
- [x] [tests/native_cpu_reservation.cpp](tests/native_cpu_reservation.cpp)
- [x] [tests/native_fg_cpu.cpp](tests/native_fg_cpu.cpp)
- [x] [tests/native_latency_vulkan.cpp](tests/native_latency_vulkan.cpp)
- [x] [tests/xell_sleep_concurrency.cpp](tests/xell_sleep_concurrency.cpp)

**Rust review findings (12)**

- [x] **Altitude | Medium | Retain unframed input observed after the frame-begin timestamp used for flushing.** fact delivers FRAME_BEGIN to the sink before flush_pending. While that sink runs, another thread can enqueue a valid window or input receipt with a later QPC timestamp. The flush correctly declines to associate it with the earlier frame but then unconditionally clears its slot, so the next eligible frame never sees it. Remove only consumed or demonstrably expired receipts and keep newer matching-session observations for the next frame. [runtime/orchestrator/src/native_cpu.rs:556](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/native_cpu.rs#L556)

- [x] **Altitude | Medium | Keep actual SDK marker delivery separate from producer-marker ordering.** frame_marker sets the CPU ledger's marker bit before submitting it to the vendor. dispatch_boundary only traces a refusal for simulation begin/end, so a later render start can see both required bits and forward additional vendor markers even though the SDK missed a prerequisite. Keep the producer observation, but record the delivery outcome per frame and stop vendor-dependent markers and statistics after a rejected prerequisite. [runtime/orchestrator/src/native_fg_cpu.rs:355](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/native_fg_cpu.rs#L355)

- [x] **Altitude | Medium | Report missing render-submit timing when Present arrives without its callback.** latency_present calls start_render_marker from Present begin. For ordinary non-prospective frames, start_render_marker checks simulation progress but does not require any observed render submission. It can therefore emit the vendor's submit-start timestamp at Present even though the actual CPU-to-GPU handoff is unknown. Have the submission or explicit render callback own that marker and leave it unavailable when only the Present callback arrives. [runtime/orchestrator/src/native_fg_cpu.rs:1207](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/native_fg_cpu.rs#L1207)

- [x] **Duplication | Medium | Classify observed CPU pacing and marker boundaries once for native and Vulkan latency.** Vulkan fact() and runtime/orchestrator/src/native_fg_cpu.rs:963 independently suppress the same three event stages when either substituted-input or derived-boundary flags are present. Both then test the current event version and KEYED bit to interpret input sample versus explicit pacing at vulkan_latency.rs:414 and native_fg_cpu.rs:1020. These gates determine whether a reported CPU fact can cause a vendor marker or sleep. Share the observed-event and pacing-boundary predicates, retaining native reservation-specific suppression and each owner's provider dispatch. [runtime/orchestrator/src/vulkan_latency.rs:398](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_latency.rs#L398)

- [x] **Altitude | Medium | Report the active XeLL provider's result in Vulkan latency status.** The XeLL option, sleep and marker paths store the native result in the provider snapshot, but get_status copies only mode, availability and counters. Its public last_result and refused_calls still come from Streamline's STATS, while Lease::mark drops the XeLL return code. A refused XeLL operation can consequently appear successful to an overlay or integration. Forward the selected owner's result and track its refusals at the same boundary as its counters. [runtime/orchestrator/src/vulkan_latency.rs:658](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_latency.rs#L658)

- [x] **Duplication | Medium | Share the Streamline input-to-marker decision across Vulkan and D3D.** Vulkan token_input, reached at runtime/orchestrator/src/vulkan_latency.rs:386, rejects a foreign nonzero window message before emitting a controller sample and rejects empty input. D3D input_for at runtime/orchestrator/src/streamline_host.rs:737 accepts the same input-kind mask, emits controller marker 13 before checking the message, ignores a foreign message and returns success for empty input. Both separately encode registered PCL ping 8 and controller sample 13 for the same SDK. Derive eligible markers and invalid/no-op classification once, then let each owner apply its own token locking, counters and documented public result mapping. [runtime/orchestrator/src/vulkan_latency_frames/streamline.rs:179](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_latency_frames/streamline.rs#L179)

- [x] **Altitude | Low | Return Anti-Lag binding refusal through the native wrapper.** Context::bind uses try_lock and can leave the old device bound when an update or render-end call is in progress. The production Rust caller in native_fg_cpu.rs:473 checks the boolean, but rsf_core_antilag_bind drops it and the AntiLagOwner wrapper in latency_antilag.h:59 exposes a void bind. Native callers cannot tell whether it is safe to pace the new device. Propagate the binding outcome and exercise the busy-rebind case in the existing native fixture. [runtime/orchestrator/src/latency_antilag.rs:206](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/latency_antilag.rs#L206)

- [x] **Simplicity | Low | native_cpu repeats one stage-mask predicate and spells CPU stage sets as raw bit numbers** record() (native_cpu.rs:715-726) computes rendered_end = rendered && ((stage == SIMULATION_END && (mask == 1 || mask == 5)) || (stage == FRAME_END && (mask == 9 || mask == 13))). Eleven lines later it computes unbound_end = refused_binding && the identical stage/mask disjunction. The same stage sets appear as unnamed literals across the module and its readers: partial_end masks 1, 3 and 7 (:714), stage_mask & 9 == 9 and == 15 (:393), stage_mask == 3 (:299), stage_mask & 1/2/4 tests (:420-431), stage_mask != 1 (:649), and fg12.rs:812 stage_mask & 15 == 15. Each literal is an OR of 1 << FRAME_BEGIN, INPUT_SAMPLE, SIMULATION_BEGIN and SIMULATION_END. Simpler: one helper, e.g. fn skips_input_or_simulation(stage, mask) -> bool, used by both rendered_end and unbound_end, plus named const masks (BEGUN = bit(FRAME_BEGIN), INPUT = BEGUN | bit(INPUT_SAMPLE), SIMULATED = ..., COMPLETE = 15) built from the stage constants already declared at :32-38. Readers then see the CPU-stage contract instead of decoding 5, 9 and 13. Tradeoff: none in behaviour; a few const definitions. [runtime/orchestrator/src/native_cpu.rs:715](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/native_cpu.rs#L715)

- [x] **Simplicity | Low | native_fg_cpu Frame tracks each latency marker as two booleans and repeats the attempt protocol four times** Frame (native_fg_cpu.rs:121-128) holds render_started/render_starting, submit_ended/submit_ending, present_started/present_starting and present_ended/present_ending. Each pair encodes one three-state marker attempt (none, in flight, marked); "both true" never occurs. pacing_started/pacing_finished (:119-120, pacing_in_flight at :155) is the same idea. The attempt protocol is hand-written four times with the same shape: refuse when done or in flight, set the in-flight flag and copy the Frame under the ledger lock, call frame_marker, then re-lock, clear the in-flight flag, set done = marked and OR failed. See start_render_marker (:1071-1089 plus the finish closure), end_render_marker (:1176-1198), latency_present begin (:1217-1250) and end (:1253-1266). Simpler: enum Marker { Idle, Pending, Done } per marker field, plus one helper fn attempt(source, field: fn(&mut Frame) -> &mut Marker, marker_id, precondition) -> (i32, bool) that runs the lock/claim/call/settle sequence once. The four call sites then keep only their own preconditions. Tradeoff: an accessor closure or field selector per call, and Frame stays Copy. Readers such as route_receipt (:887) and frame_acquire (:323-324) test render_started || render_starting, which becomes != Idle. [runtime/orchestrator/src/native_fg_cpu.rs:121](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/native_fg_cpu.rs#L121)

- [x] **Altitude | Low | Let the render-hitch monitor sleep or stop with its owning session.** The diagnostic thread wakes every two milliseconds until it takes 36 samples. If debugging is disabled or the session ends before enough hitches occur, end_session removes the current target but leaves the thread polling an empty WATCH forever. Give the worker a session cancellation or condition-variable wakeup, and preserve the total sample budget independently of the worker lifetime. [runtime/orchestrator/src/native_fg_cpu/hitch.rs:147](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/native_fg_cpu/hitch.rs#L147)

- [x] **Simplicity | Low | native_fg_cpu calls its own Win32 helpers through the C++ platform table** native_fg_cpu/win32.rs:43-240 defines 11 #[no_mangle] Rust helpers: rsf_core_nfg_signal_completion, wait_completion, owned_window_thread, post_ping, input_idle_ms, tick_millis, qpc_frequency, thread_id, duplicate_render_thread, sample_render_thread and describe_address. native_fg.cpp:65-76 copies those Rust symbols into the rsf_nfg_cpu_platform table, and the same Rust module calls them back through call!(p, ...).expect(...): tick_millis (native_fg_cpu.rs:380, :1332), wait_completion (:392, :587), signal_completion (:289), thread_id (:877; diagnostics.rs:385; hitch.rs:77), qpc_frequency (:1297; diagnostics.rs:60; hitch.rs:103), owned_window_thread (:869, :1465), post_ping (:1556), input_idle_ms (diagnostics.rs:186) and duplicate_render_thread (hitch.rs:83). hitch.rs:105 additionally re-checks the Option slots sample_render_thread and describe_address, which are always these same Rust functions. Simpler: call super::win32::{tick_millis, wait_completion, ...} directly as plain Rust functions, drop the 11 slots from rsf_nfg_cpu_platform and the matching rows in native_fg.cpp, and keep in the table only the leaves that really live in C++ (host, acquire, marker, pace ...). Tradeoff: tests that stub these slots would need a cfg(test) seam. Inference: no test stubs for them were found in this crate. [runtime/orchestrator/src/native_fg_cpu/win32.rs:43](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/native_fg_cpu/win32.rs#L43)

- [x] **Altitude | Low | Keep each device's Reflex sync state with the report that names it.** The audit initializes each incoming sync report's enabled flag from the previous process-wide report, even if report.device changes. A failed request or one with neither enable nor disable then logs the old device's setting alongside the new device pointer in reflex_audit.cpp:138. Track the last accepted configuration per device or mark it unknown when switching device identities. [runtime/orchestrator/src/reflex_audit.rs:252](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/reflex_audit.rs#L252)

<a id="l01"></a>

### L01 · Loader entry and carrier forwarding

Port process entry, proxy forwarding and module selection for the thin loader and launcher.

**Coordinate shared edits**

- Carrier entry, game recognition and existing forwarding exports ([A01](#a01), [W01](#w01)).

**Port files (14)**

- [x] [apps/launcher/src/main.cpp](apps/launcher/src/main.cpp)
- [x] [cmake/version.h.in](cmake/version.h.in) (template for the C/C++ version header)
- [x] [loader/proxy/src/dlss_bridge.h](loader/proxy/src/dlss_bridge.h)
- [x] [loader/proxy/src/plugin_carrier.cpp](loader/proxy/src/plugin_carrier.cpp)
- [x] [loader/proxy/src/preferences.h](loader/proxy/src/preferences.h)
- [x] [loader/proxy/src/shim_loader.cpp](loader/proxy/src/shim_loader.cpp)
- [x] [loader/proxy/src/thin_config.cpp](loader/proxy/src/thin_config.cpp)
- [x] [loader/proxy/src/thin_config.h](loader/proxy/src/thin_config.h)
- [x] [loader/proxy/src/thin_library.h](loader/proxy/src/thin_library.h)
- [x] [loader/proxy/src/thin_loader.cpp](loader/proxy/src/thin_loader.cpp)
- [x] [loader/proxy/src/thin_startup.cpp](loader/proxy/src/thin_startup.cpp)
- [x] [loader/proxy/src/thin_startup.h](loader/proxy/src/thin_startup.h)
- [x] [loader/proxy/src/xinput_forwarding.cpp](loader/proxy/src/xinput_forwarding.cpp)
- [x] [loader/src/bootstrap.cpp](loader/src/bootstrap.cpp)

**Fixtures (4)**

- [x] [tests/shared_shim_exports.cpp](tests/shared_shim_exports.cpp)
- [x] [tests/thin_loader.cpp](tests/thin_loader.cpp)
- [x] [tests/thin_loader_fixture.cpp](tests/thin_loader_fixture.cpp)
- [x] [tests/xinput_carrier.cpp](tests/xinput_carrier.cpp)

<a id="l02"></a>

### L02 · Loader census and diagnostics

Move the API census, static scan and diagnostic capture behind the loader's Rust interfaces.

**Port files (16)**

- [x] [loader/census/include/rescaleframe/census_d3d12_observer.h](loader/census/include/rescaleframe/census_d3d12_observer.h)
- [x] [loader/census/include/rescaleframe/vendor_census.h](loader/census/include/rescaleframe/vendor_census.h)
- [x] [loader/census/src/census_internal.h](loader/census/src/census_internal.h)
- [x] [loader/census/src/core.cpp](loader/census/src/core.cpp)
- [x] [loader/census/src/decode.cpp](loader/census/src/decode.cpp)
- [x] [loader/census/src/hooks.cpp](loader/census/src/hooks.cpp)
- [x] [loader/census/src/static_scan.cpp](loader/census/src/static_scan.cpp)
- [x] [loader/census/src/wrap_d3d.cpp](loader/census/src/wrap_d3d.cpp)
- [x] [loader/census/src/wrap_fsr.cpp](loader/census/src/wrap_fsr.cpp)
- [x] [loader/census/src/wrap_generic.h](loader/census/src/wrap_generic.h)
- [x] [loader/census/src/wrap_misc.cpp](loader/census/src/wrap_misc.cpp)
- [x] [loader/census/src/wrap_ngx.cpp](loader/census/src/wrap_ngx.cpp)
- [x] [loader/census/src/wrap_vk.cpp](loader/census/src/wrap_vk.cpp)
- [x] [loader/diagnostics/include/rescaleframe/frame_capture.h](loader/diagnostics/include/rescaleframe/frame_capture.h)
- [x] [loader/diagnostics/include/rescaleframe/module_dump.h](loader/diagnostics/include/rescaleframe/module_dump.h)
- [x] [loader/diagnostics/src/module_dump_internal.h](loader/diagnostics/src/module_dump_internal.h)

- [x] [loader/census/src/decode_d3d_platform.cpp](loader/census/src/decode_d3d_platform.cpp) (introduced D3D resource query leaf)
- [x] [loader/census/src/census_platform.cpp](loader/census/src/census_platform.cpp) (converted to the C11 leaf `census_platform.c`: native descriptor catalog, vendor tables and guarded NGX and Streamline calls; `census_internal.h` is now a C header)
- [x] [loader/census/src/hook_platform.cpp](loader/census/src/hook_platform.cpp) (converted to the C11 leaf `hook_platform.c`: MinHook calls and the TEB getter)
- [x] [loader/census/src/guarded_platform.cpp](loader/census/src/guarded_platform.cpp) (converted to the C11 leaf `guarded_platform.c`: SEH reads, code-pointer and vtable-slot checks; the `guard` lambda helper is gone, each guarded call owns its `__try`)
- [x] [loader/census/src/d3d_guarded_platform.cpp](loader/census/src/d3d_guarded_platform.cpp) (converted to the C11 leaf `d3d_guarded_platform.c`: guarded QueryInterface, Release and description reads through `lpVtbl`)

**Fixtures (10)**

- [x] [tests/loader/vendor_census_native.cpp](tests/loader/vendor_census_native.cpp) (converted to `tests/loader/vendor_census_native.c`: C vtable fake resource and token objects, DXGI and D3D12 slots from SDK `*Vtbl` offsets, `CreateThread`, `PUT` payload writes; the remaining C++ is the next entry)
- [ ] [tests/loader/vendor_census_ngx_platform.cpp](tests/loader/vendor_census_ngx_platform.cpp) (introduced SDK fixture leaf, now the whole of the old fixture driver's C++; blocked: `NVSDK_NGX_Parameter` is a C++ class whose virtual slots follow MSVC overload ordering, so only a C++ compiler can select the Set overload slots; the called probe is `rsf_test_ngx_check_slots` and four typed setters declared in the C header `tests/loader/ngx_param_probe.h`, caller `vendor_census_native.c` run_ngx and check_vtable_slots)
- [x] [tests/census_present_coexistence.cpp](tests/census_present_coexistence.cpp)
- [ ] [tests/loader/module_dump_rtti_fixture.cpp](tests/loader/module_dump_rtti_fixture.cpp) (native RTTI specimen; blocked: the `module_dump_v2` test needs a compiler-emitted MSVC RTTI type descriptor, complete object locator and vftable, which only a C++ compiler produces, and the call is `rsf_test_module_dump_v2` with the vftable RVA of a derived class; the file is 25 lines)
- [x] [tests/module_dump_v2.cpp](tests/module_dump_v2.cpp)
- [x] [tests/vendor_census.cpp](tests/vendor_census.cpp)
- [x] [tests/vendor_census_asi.cpp](tests/vendor_census_asi.cpp)
- [x] [tests/vendor_census_fixture.cpp](tests/vendor_census_fixture.cpp)
- [ ] [tests/loader/vendor_layout_facts.cpp](tests/loader/vendor_layout_facts.cpp) (native SDK layout evidence; FidelityFX, XeSS and Vulkan moved to `vendor_layout_facts.c`, which also owns `main`; blocked for the rest: `sl.h` declares the `sl::` namespace types and the FSR 2 header declares `Fsr212::` types, both C++ only, and the Streamline structs need measurement on real storage because they have constructors; the C driver calls `rsf_test_layout_facts_cxx` when `RSF_CENSUS_LAYOUT_CXX` is set for the sl and fsr2 targets)
- [x] [tests/vendor_census_layouts.cpp](tests/vendor_census_layouts.cpp)
- [x] [tests/vendor_census_ngx_params.h](tests/vendor_census_ngx_params.h)

<a id="o01"></a>

### O01 · Shared overlay input and windows

Port overlay input, cursor, pointer, visibility and host window ownership through the existing service.

**Coordinate shared edits**

- Overlay input capture and renderer shutdown ([O02](#o02)).

**Port files (11)**

- [x] [loader/proxy/src/overlay_host.cpp](loader/proxy/src/overlay_host.cpp)
- [x] [loader/proxy/src/overlay_host.h](loader/proxy/src/overlay_host.h)
- [x] [loader/proxy/src/overlay_host_platform.h](loader/proxy/src/overlay_host_platform.h)
- [x] [runtime/graphics/include/rescaleframe/overlay_input.h](runtime/graphics/include/rescaleframe/overlay_input.h)
- [x] [runtime/graphics/src/overlay_cursor_capture.h](runtime/graphics/src/overlay_cursor_capture.h)
- [x] [runtime/graphics/src/overlay_cursor_platform.h](runtime/graphics/src/overlay_cursor_platform.h)
- [x] [runtime/graphics/src/overlay_input.cpp](runtime/graphics/src/overlay_input.cpp)
- [x] [runtime/graphics/src/overlay_input_route.h](runtime/graphics/src/overlay_input_route.h)
- [x] [runtime/graphics/src/overlay_pointer_platform.h](runtime/graphics/src/overlay_pointer_platform.h)
- [x] [runtime/graphics/src/overlay_visibility_platform.h](runtime/graphics/src/overlay_visibility_platform.h)
- [x] [runtime/graphics/src/overlay_window_platform.h](runtime/graphics/src/overlay_window_platform.h)

**Fixtures (2)**

- [x] [tests/overlay_host.cpp](tests/overlay_host.cpp)
- [x] [tests/overlay_hotkey.cpp](tests/overlay_hotkey.cpp)

**Rust review findings (4)**

- [x] **Altitude | Medium | Keep egui textures alive through the draw that retires them.** Host::carry_textures drains both atlas updates and texture frees before renderer_draw in draw_target and present. ui/overlay/src/overlay.rs:407 forwards egui's TexturesDelta.free directly, whose epaint contract schedules those frees after painting. A mesh can therefore lose a texture needed by its last frame. Upload texture changes before drawing and retire the released IDs after the draw attempt, at one shared host-owned frame boundary. [runtime/orchestrator/src/overlay_host.rs:378](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/overlay_host.rs#L378)

- [x] **Duplication | Medium | Handle a poisoned overlay panel once across both D3D11 draw routes.** The supplied-target and swapchain paths independently collect input, call the same panel.frame, process its intent, carry atlas textures and draw. A layout failure returns directly from draw_target, while present at runtime/orchestrator/src/overlay_host.rs:624 closes the panel and latches failure. The panel sets poisoned after a panic and every later frame then refuses (ui/overlay/src/ffi.rs:98 and :112), so wrapped-device drawing keeps invoking an unusable panel. Share layout/failure handling across both entries, preserving their distinct target acquisition and the intentional intent-on-draw-failure contracts documented at runtime/orchestrator/src/overlay_host_tests.rs:585 and :753. [runtime/orchestrator/src/overlay_host.rs:437](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/overlay_host.rs#L437)

- [x] **Altitude | Medium | Apply hotkey debounce to toggle edges, not explicit panel visibility requests.** Visibility::request applies the 250 ms debounce to EXTERNAL requests as well as window and polling edges. A close-button intent or overlay_host::close_after_failure immediately following an open can therefore be discarded, leaving input captured by a panel that just stopped drawing. The test explicitly rejects an external hide after two milliseconds. Make set_visible idempotently honor the requested state and use debounce only to reconcile duplicate physical toggle observations. [runtime/orchestrator/src/overlay_visibility.rs:40](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/overlay_visibility.rs#L40)

- [x] **Altitude | Low | Count observed raw mouse input separately from all WM_INPUT messages.** The route increments raw_mouse_reports for every visible WM_INPUT after calling a void leaf. record_raw_mouse in overlay_input.cpp:69 can return before reading the packet, or ignore a keyboard/HID record, a failed read or absolute motion. The diagnostic therefore cannot establish that a mouse report reached the panel. Return the decoded device kind or count the mouse report at the decoder and expose total WM_INPUT traffic separately if needed. [runtime/orchestrator/src/overlay_input_route.rs:265](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/overlay_input_route.rs#L265)

<a id="o02"></a>

### O02 · Shared overlay rendering and panel

Port panel and status rendering, overlay device resources and the UI-to-Runtime option mappings.

**Coordinate shared edits**

- Shared settings, frontend requests and live overlay state ([C02](#c02), [C03](#c03)).
- Overlay input capture and renderer shutdown ([O01](#o01)).

**Port files (15)**

- [x] [runtime/graphics/include/rescaleframe/overlay_renderer.h](runtime/graphics/include/rescaleframe/overlay_renderer.h)
- [x] [runtime/graphics/src/overlay_renderer.cpp](runtime/graphics/src/overlay_renderer.cpp)
- [x] [runtime/graphics/src/overlay_renderer_platform.h](runtime/graphics/src/overlay_renderer_platform.h)
- [x] [runtime/orchestrator/include/rescaleframe/overlay_api.h](runtime/orchestrator/include/rescaleframe/overlay_api.h)
- [x] [runtime/orchestrator/include/rescaleframe/overlay_d3d12.h](runtime/orchestrator/include/rescaleframe/overlay_d3d12.h)
- [x] [runtime/orchestrator/include/rescaleframe/overlay_fg_stats.h](runtime/orchestrator/include/rescaleframe/overlay_fg_stats.h)
- [x] [runtime/orchestrator/src/overlay_d3d12.cpp](runtime/orchestrator/src/overlay_d3d12.cpp)
- [x] [runtime/orchestrator/src/overlay_native12.cpp](runtime/orchestrator/src/overlay_native12.cpp)
- [x] [runtime/orchestrator/src/overlay_native12_platform.h](runtime/orchestrator/src/overlay_native12_platform.h)
- [x] [runtime/orchestrator/src/overlay_panel.h](runtime/orchestrator/src/overlay_panel.h)
- [x] [runtime/orchestrator/src/overlay_platform.cpp](runtime/orchestrator/src/overlay_platform.cpp)
- [x] [runtime/orchestrator/src/overlay_platform.h](runtime/orchestrator/src/overlay_platform.h)
- [x] [runtime/orchestrator/src/overlay_wrapped12_platform.h](runtime/orchestrator/src/overlay_wrapped12_platform.h)
- [x] [runtime/orchestrator/src/sharpening_overlay.h](runtime/orchestrator/src/sharpening_overlay.h)
- [x] [ui/overlay/include/rescaleframe/overlay.h](ui/overlay/include/rescaleframe/overlay.h)

**Fixtures (5)**

- [x] [tests/overlay_dll_api.cpp](tests/overlay_dll_api.cpp)
- [x] [tests/overlay_dll_api_device.cpp](tests/overlay_dll_api_device.cpp)
- [x] [tests/overlay_dll_api_no_sr.cpp](tests/overlay_dll_api_no_sr.cpp)
- [x] [tests/overlay_native12.cpp](tests/overlay_native12.cpp)
- [x] [tests/overlay_panel_fixture.cpp](tests/overlay_panel_fixture.cpp)

**Rust review findings (23)**

- [x] **Duplication | Medium | Skip empty overlay draw calls before resolving textures on both renderers.** D3D11 builds draw commands at runtime/orchestrator/src/overlay_renderer11.rs:364, skipping zero indices and empty clipped rectangles before texture lookup. D3D12 independently computes the same clip bounds but resolves the descriptor first and propagates a missing texture as an error; overlay_native12.rs:881 then fails the entire panel. In a mixed frame an empty draw referencing an absent texture can therefore disable D3D12 while D3D11 still renders the other draws. Move empty-draw detection and clipped rectangle construction into overlay_geometry, leaving each backend's resource binding and policy for malformed nonempty calls separate. [runtime/orchestrator/src/overlay_native12.rs:380](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/overlay_native12.rs#L380)

- [x] **Altitude | Medium | Allow the native D3D12 panel to recover after its failed owner retires.** A startup failure or slot wait timeout sets the process-global failed flag. retire_failed can subsequently release every panel, renderer and fence reference but never clears failed, and later Present calls return before initialization. Calls on a replacement device cannot even advance old-owner cleanup because retirement is gated on the incoming device and queue. Separate retiring work from readiness, finish it using the retained original owner, then allow a new device or an explicit retry to start the overlay. [runtime/orchestrator/src/overlay_native12.rs:739](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/overlay_native12.rs#L739)

- [x] **Duplication | Medium | Normalize saved FG backends with the same rule used for live settings.** Config::valid at runtime/orchestrator/src/overlay_service.rs:303 requires Auto when an adapter advertises zero FG choices. Config::sanitize tests the same setting independently but skips normalization entirely for zero choices. A concrete FGBackend read at runtime/orchestrator/src/settings_store.rs:290 is therefore published by configure even after it reports that invalid saved values were replaced; subsequent unrelated panel edits reusing this settings snapshot fail validation. Share an FG-backend admission predicate and replace any inadmissible saved value with the validated adapter default. [runtime/orchestrator/src/overlay_service.rs:330](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/overlay_service.rs#L330)

- [x] **Altitude | Medium | Preserve Auto as the requested FG setting when building panel statistics.** State::panel_stats replaces the requested Auto value with the currently selected provider, even though State::status returns the saved Auto request and exposes the effective backend separately. The panel passes this substituted value to its FG controls in ui/overlay/src/ffi.rs:340, so it displays an explicit vendor selection that the user did not make. Forward the saved requested value and keep the runtime's resolved provider solely in the effective field. [runtime/orchestrator/src/overlay_service.rs:862](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/overlay_service.rs#L862)

- [x] **Altitude | Medium | Require an adapter to acknowledge an editable overlay option.** Config accepts nonempty adapter options without an option_changed callback, and apply_intent treats a missing callback as success. It updates the panel cache and logs applied despite having no way to change or persist the adapter setting. overlay_api.h:98 assigns both responsibilities to the adapter. Reject this configuration, or expose such options explicitly as read-only, and update the cached value only after acknowledgment. [runtime/orchestrator/src/overlay_service.rs:1076](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/overlay_service.rs#L1076)

- [x] **Duplication | Medium | Share the generated overlay wire types with the overlay frontend.** Overlay frame export ui/overlay/src/ffi.rs:90 reads the manually declared RsfOverlayStats and returns RsfOverlayIntent from ui/overlay/src/abi.rs:259. The runtime constructs and consumes the same records at runtime/orchestrator/src/overlay_service.rs:816 and :1066 using generated types in runtime/orchestrator/src/overlay_abi.rs:519 and :795. The manual stats documentation already diverges on zero SR choice masks. Generate one dependency-free overlay ABI module from overlay.h for both crates, keep semantic defaults and safe model conversion in UI code, and retain independent ABI compatibility assertions. [ui/overlay/src/abi.rs:49](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/ui/overlay/src/abi.rs#L49)

- [x] **Duplication | Medium | Define the selectable SR quality levels from the same capability list the runtime accepts.** The UI's Quality::ALL and from_abi stop at 4, and ui/overlay/src/panel.rs:881 builds every quality control from that list. Vulkan XeSS accepts 5 in runtime/orchestrator/src/vulkan_sr.rs:87 and selects SDK Ultra Quality from the six-element mapping at runtime/orchestrator/src/vulkan_sr/xess.rs:31 and :422. A host reporting that supported level gets an unknown-quality label at panel.rs:922 and no way to select it there. Share quality identifiers and expose per-backend supported choices to the panel so level 5 is selectable for XeSS while unsupported backends omit it. [ui/overlay/src/model.rs:31](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/ui/overlay/src/model.rs#L31)

- [x] **Duplication | Medium | Define sharpening defaults once for the runtime, panel and recording fixtures.** runtime/orchestrator/include/rescaleframe/sharpening_settings.h:26 supplies twelve default values that ui/overlay/src/model.rs:361 hardcodes again for the panel state at :317. runtime/orchestrator/src/callback_host/tests.rs:181, render_host/tests.rs:168 and integrations/godot/native/platform/fake.rs:89 also copy that tuple into their fake defaults leaves. Changing a runtime default can leave the panel initialization and passing tests on the old values. Generate named defaults for both languages from one definition, keeping each wrapper's ABI header distinct, or use deliberate sentinel defaults in forwarding tests and verify the canonical tuple once. [ui/overlay/src/model.rs:361](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/ui/overlay/src/model.rs#L361)

- [x] **Altitude | Medium | Reconcile refused quality and enable requests with an explicit apply result.** Selection keeps requested_quality and requested_enabled until effective stats equal the request. If a host refuses a quality change, the old quality stays in effect and the requested control stays pending indefinitely. Clicking that same choice cannot retry because upscale_tab compares it with the pending selection. Return a request-specific applied, pending or refused result in the snapshot and clear the pending selection on refusal. [ui/overlay/src/panel.rs:191](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/ui/overlay/src/panel.rs#L191)

- [x] **Altitude | Medium | Keep every offered integration tab reachable at supported panel scales.** show includes Game when the adapter supplies options, but draw_rail stops creating buttons when one does not fit, and only the selected tab body can scroll. At 720p with the 1.5x panel size the rail has 272 points. With sharpening offered, the fourth tab, Game, needs 290 points and has no status-strip shortcut. Make the rail scroll or provide overflow navigation so an adapter's controls remain reachable at that scale. [ui/overlay/src/panel.rs:667](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/ui/overlay/src/panel.rs#L667)

- [x] **Altitude | Medium | Let the pacing owner resolve the VRR frame limit from the effective output mode.** The panel multiplies the display interval plus 300 microseconds by requested_generated + 1, then emits only a fixed frame_limit_us. With 4x requested but generation suspended, choosing VRR on a 120 Hz display caps real frames at about 29 fps. The pacing owner receives no VRR intent to recalculate after a provider, generation count or refresh change. Send the selected limiter policy and resolve its interval from effective state at the orchestrator. [ui/overlay/src/panel.rs:1317](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/ui/overlay/src/panel.rs#L1317)

- [x] **Duplication | Medium | Share the FG deferral-reason catalog across the runtime and overlay.** Native FG assigns code 7 for invalid input at runtime/orchestrator/src/native_fg.rs:1194 and :1211. The panel receives that code via runtime/orchestrator/src/overlay_service.rs:888 but its independent numeric text table skips 7, so frames_tab at :1074 labels an invalid source as waiting for SDK-confirmed generation. OverlayService::description at :487 and runtime/orchestrator/src/render_host.rs:1141 independently translate overlapping subsets. Generate the identifiers from native_fg.h and share a complete reason-to-description table with explicit invalid-input and unknown cases, leaving host-level status precedence at each caller. [ui/overlay/src/panel.rs:2294](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/ui/overlay/src/panel.rs#L2294)

- [x] **Altitude | Medium | Leave presented FPS unknown when generation is active without an aggregate counter.** overlay_service.rs:891 reports aggregate-counter validity independently from vendor.active. Meter substitutes application_presented_frames when the aggregate is unavailable, then the HUD labels the result Presented. During active interpolation this counts only source Presents and can report identical rendered and presented rates while additional outputs are displayed. Keep the rendered rate and mark presented unavailable until an aggregate source is valid, using the application counter only for a known pass-through interval. [ui/overlay/src/performance.rs:28](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/ui/overlay/src/performance.rs#L28)

- [x] **Simplicity | Low | overlay_native12 Panel keeps its started state as three parallel Options and unwraps them on every Present** Panel (overlay_native12.rs:552-569) holds ui: Option<Ui>, native: Option<Renderer> and fence: Option<Fence>. They are only set together by initialize (:625-631) and only cleared together by retire_failed (:692-694). native.is_none() is the started test (:648, :745). rsf_core_overlay12_present then re-opens the invariant with .unwrap() at :765 (native), :766 and :776 (fence), :782 (native), :833 (ui), :870 (ui) and :891 (fence), and Fence::finish_event unwraps self.event at :166. Simpler: one started: Option<Started { ui, renderer, fence }> field, matched once at the top of present (let Some(started) = &mut panel.started else { initialize }), so the seven unwraps become field accesses and "half-initialized" cannot be expressed. device/queue could live in Started too, since they are written in the same initialize. Tradeoff: retire_failed must take the whole struct, which it already does field by field. Fields drop in declaration order today, and the struct must keep the same order (atlas and graphics before ui and renderer) to preserve teardown order. This is separate from the per-call platform.X.unwrap() pattern in the same file (19 Option slots, which the tests stub selectively). [runtime/orchestrator/src/overlay_native12.rs:552](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/overlay_native12.rs#L552)

- [x] **Duplication | Low | Use one elapsed-frame policy for the shared overlay panel.** The native D3D12 host subtracts millisecond ticks and sends the elapsed time directly to collect_input; the D3D11 host at runtime/orchestrator/src/overlay_host.rs:335 computes the same panel input in QPC ticks but substitutes 1/60 s for the first frame and any gap over 250 ms. After one second hidden, the D3D12 route can advance the panel clock by one second while D3D11 advances it by one nominal frame; ui/overlay/src/overlay.rs:368 accepts deltas up to one second. Share normalization of elapsed seconds across both hosts, preserving each clock source and GPU submission path. [runtime/orchestrator/src/overlay_native12.rs:813](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/overlay_native12.rs#L813)

- [x] **Duplication | Low | Use one draw-admission decision for the wrapped overlay.** D3D11-on-12 starts a local timer after host startup and gates back-buffer wrapping on visibility, the performance HUD and eight seconds of startup hint. Its draw leaf then calls the host's draw_target, which repeats the same gate at runtime/orchestrator/src/overlay_host.rs:411 against a second clock initialized earlier at :331. A change to hint duration can make the wrapper suppress a requested frame or repeatedly acquire and flush a wrapped buffer the host declines. Expose one host-owned draw decision that the wrapper can consult before acquiring the back buffer, keeping resource acquisition conditional. [runtime/orchestrator/src/overlay_wrapped12.rs:195](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/overlay_wrapped12.rs#L195)

- [x] **Altitude | Low | Document that a zero SR choice mask disables the upscaling controls.** The public Rust ABI field tells a host that zero selects a default list. overlay.h:149, the safe Stats model, panel.rs:548 and the no-SR fixture instead treat zero as an explicit declaration of no SR. A host following the Rust field contract can hide its functioning upscaler. Align the Rust documentation with the header and state that every SR-capable host must send its actual backend mask. [ui/overlay/src/abi.rs:165](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/ui/overlay/src/abi.rs#L165)

- [x] **Simplicity | Low | Make the reported quality a single value in the overlay model.** Stats exposes both quality_raw and its decoded Option<Quality>. borrow_stats, Stats::default and set_quality keep them in sync manually, while panel.rs reads one for selection/reconciliation and the other for unknown-level text. Store the ABI value once and decode it with a quality() accessor where needed. This preserves the unknown numeric level and existing C ABI, at the cost of updating Rust callers of the two public fields and doing a small match on read. [ui/overlay/src/model.rs:152](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/ui/overlay/src/model.rs#L152)

- [x] **Simplicity | Low | Use the input event position to draw the overlay cursor.** Every visible frame converts both host mouse coordinates from pixels to points twice. raw_input sanitizes the coordinates, scales them and uses the result for egui motion and button events. frame repeats that work for paint_cursor. Return the computed Pos2 alongside RawInput and pass it directly to the painter. This adds one tuple element and keeps cursor placement and hit testing on the same conversion while preserving raw pixel history for move detection. [ui/overlay/src/overlay.rs:228](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/ui/overlay/src/overlay.rs#L228)

- [x] **Simplicity | Low | Draw available tabs directly from the fixed tab list.** show allocates a Vec of at most five tabs each visible frame and scans it to check the active tab. draw_rail then searches Tab::ALL twice for each visible tab to obtain the widget ID and Controls index. Compute availability once for the fixed five entries, check the current tab against it, and iterate indexed offered entries with a separate displayed-row index. This retains the order, fallback tab, IDs and compact row positions without a heap allocation or repeated position searches. [ui/overlay/src/panel.rs:336](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/ui/overlay/src/panel.rs#L336)

- [x] **Simplicity | Low | Limit the segmented control to the single-row layout its callers use.** All twelve private segments callers pass one column per item and construct every Segment with sub: None. The widget nevertheless computes multiple rows and paints a separate two-line subtitle layout; each caller must repeat its own column count. Derive the column count from items.len(), lay out one row and remove the subtitle field and branch. Keep newline-aware font size and height for the existing Depth RCAS and Depth DAS labels. A future layout needing subtitles or wrapping could add that behavior when it has a caller. [ui/overlay/src/panel.rs:1745](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/ui/overlay/src/panel.rs#L1745)

- [x] **Boundaries | Low | Replace the AC7 scene wording for the generic FG scene-unsupported reason.** waiting_reason maps fg_reason 4 to "Waiting for flight, hangar, or briefing". Code 4 is the vendor-neutral RSF_NATIVE_FG_REASON_SCENE_UNSUPPORTED (runtime/orchestrator/include/rescaleframe/native_fg.h:23, set in runtime/orchestrator/src/fg12.rs:824), which every FG-capable integration can produce, while flight/hangar/briefing are Ace Combat 7 screen classes (games/ac7/src/native_renderer.cpp:253-266). AGENTS.md Rule 2 forbids game-specific wording or controls in ui/overlay, so a Skyrim, Fallout 4 or Unity session paused on an ineligible screen is told to wait for an AC7 hangar. The runtime already words the same code generically in overlay_service.rs:490 ("Scene is not eligible"). Use a game-neutral phrase here, or have the adapter describe its eligible screens through the overlay service if that detail is wanted. The paint_cursor doc comment at ui/overlay/src/overlay.rs:513 ("because AC7 hides the system cursor") should be made neutral at the same time. [ui/overlay/src/panel.rs:2299](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/ui/overlay/src/panel.rs#L2299)

- [x] **Simplicity | Low | Check detailed HUD inputs before formatting the text.** Overlay::frame calls HudDetail::of on every frame with detailed HUD enabled. That allocates and formats SR and frame-limit strings before HudCache can compare its key, then update clones both strings into rows on a miss. Key the cache by cheap display inputs (optional active SR backend and quality, and frame-limit microseconds), format them directly into rows only on a miss, and preserve the distinction between compact and detailed HUD. This may refresh once when two raw limits round to the same visible FPS, but unchanged frames no longer rebuild either string. [ui/overlay/src/panel.rs:2349](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/ui/overlay/src/panel.rs#L2349)

## Graphics and presentation

<a id="g01"></a>

### G01 · Graphics observers and device identity

Port D3D11 and D3D12 observer registration, device identity and callback retirement.

**Coordinate shared edits**

- Observer callbacks, saved D3D state and slot restoration ([G02](#g02), [G03](#g03)).

**Port files (12)**

- [x] [runtime/graphics/include/rescaleframe/d3d11_observer.h](runtime/graphics/include/rescaleframe/d3d11_observer.h)
- [x] [runtime/graphics/include/rescaleframe/d3d12_observer.h](runtime/graphics/include/rescaleframe/d3d12_observer.h)
- [x] [runtime/graphics/src/d3d11_observer.cpp](runtime/graphics/src/d3d11_observer.cpp)
- [x] [runtime/graphics/src/d3d11_observer_callbacks.h](runtime/graphics/src/d3d11_observer_callbacks.h)
- [x] [runtime/graphics/src/d3d11_observer_capture.h](runtime/graphics/src/d3d11_observer_capture.h)
- [x] [runtime/graphics/src/d3d11_observer_hooks.h](runtime/graphics/src/d3d11_observer_hooks.h)
- [x] [runtime/graphics/src/d3d11_observer_module.h](runtime/graphics/src/d3d11_observer_module.h)
- [x] [runtime/graphics/src/d3d11_observer_present.h](runtime/graphics/src/d3d11_observer_present.h)
- [x] [runtime/graphics/src/d3d12_observer.cpp](runtime/graphics/src/d3d12_observer.cpp)
- [x] [runtime/graphics/src/d3d12_observer_platform.h](runtime/graphics/src/d3d12_observer_platform.h)
- [x] [runtime/graphics/src/device_owner.h](runtime/graphics/src/device_owner.h)
- [x] [runtime/graphics/src/vtable_patch.h](runtime/graphics/src/vtable_patch.h)

**Fixtures (2)**

- [x] [tests/d3d11_observer.cpp](tests/d3d11_observer.cpp)
- [x] [tests/d3d12_observer.cpp](tests/d3d12_observer.cpp)

**Rust review findings (3)**

- [x] **Altitude | Medium | Associate captured resources with the selected presenting device.** adopt_device replaces the retained device and clears its immediate context but leaves textures and constant buffers from earlier devices in one bounded inventory. record_texture counts those entries toward capacity, and record_buffer replaces by byte size alone. A helper device can fill the texture slots before the real Present or overwrite a constant buffer later, so dump::write tries foreign resources and misses the actual device's inputs. Partition or filter inventory by COM device identity and transfer only matching candidates when Present selects the owner. [runtime/orchestrator/src/d3d11_observer_capture.rs:203](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/d3d11_observer_capture.rs#L203)

- [x] **Altitude | Medium | Retain or verify the storage behind D3D11 probe vtables.** probe returns raw vtable addresses after releasing the temporary device, chain and chain1 interfaces. d3d11_observer.cpp:838 reads those tables to form patch slots, and Hooks retains slot addresses for later retirement. An object-local proxy vtable can disappear when the temporary object's final reference drops. Admit only independently verified resident tables, or retain the backing probe owner for the entire period the slots are read or patched. [runtime/orchestrator/src/d3d11_observer_hooks/probe.rs:124](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/d3d11_observer_hooks/probe.rs#L124)

- [x] **Duplication | Medium | Share checked on-disk PE section lookup between the observer and Skyrim compatibility code.** runtime/orchestrator/src/d3d11_observer_module.rs:31 and :62 validate PE32+ headers and translate RVAs into bounded file bytes for :135. games/skyrim-se/src/compatibility.rs:98 and :119 repeat the DOS/NT/COFF walk, section iteration and raw-range check for exact Community Shaders guards at :154. Skyrim additionally verifies fingerprint facts and executable section flags, while the observer needs the preferred image base and refuses virtual tails. Share a read-only PE32+ view returning bounded bytes and section characteristics, leaving profile limits and comparison rules at the callers. Run a common set of truncated-table and virtual-tail vectors against that parser. [runtime/orchestrator/src/d3d11_observer_module.rs:62](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/d3d11_observer_module.rs#L62)

<a id="g02"></a>

### G02 · Frame tap, constants and scene promotion

Port slot substitution, constant overrides and promotion as one lifetime chain. Address the restoration findings before releasing borrowed views.

**Coordinate shared edits**

- Borrowed view and constant-buffer identity across render, capture and velocity ([A02](#a02), [A03](#a03), [A05](#a05)).
- Observer callbacks, saved D3D state and slot restoration ([G01](#g01), [G03](#g03)).

**Port files (9)**

- [x] [runtime/graphics/include/rescaleframe/constant_buffer_read.h](runtime/graphics/include/rescaleframe/constant_buffer_read.h)
- [x] [runtime/graphics/include/rescaleframe/constant_twin.h](runtime/graphics/include/rescaleframe/constant_twin.h)
- [x] [runtime/graphics/include/rescaleframe/frame_tap.h](runtime/graphics/include/rescaleframe/frame_tap.h)
- [x] [runtime/graphics/include/rescaleframe/scene_promote.h](runtime/graphics/include/rescaleframe/scene_promote.h)
- [x] [runtime/graphics/src/constant_buffer_read.cpp](runtime/graphics/src/constant_buffer_read.cpp)
- [x] [runtime/graphics/src/constant_twin.cpp](runtime/graphics/src/constant_twin.cpp)
- [x] [runtime/graphics/src/frame_tap.cpp](runtime/graphics/src/frame_tap.cpp)
- [x] [runtime/graphics/src/frame_tap_platform.h](runtime/graphics/src/frame_tap_platform.h)
- [x] [runtime/graphics/src/scene_promote.cpp](runtime/graphics/src/scene_promote.cpp)

- [ ] [runtime/orchestrator/src/frame_tap_callbacks.cpp](runtime/orchestrator/src/frame_tap_callbacks.cpp) (introduced by the port to contain foreign C++ callback exceptions)

**Fixtures (3)**

- [x] [tests/constant_override.cpp](tests/constant_override.cpp)
- [x] [tests/frame_tap.cpp](tests/frame_tap.cpp)
- [x] [tests/scene_promote.cpp](tests/scene_promote.cpp)

**Rust review findings (10)**

- [x] **Altitude | High | Retain frame-tap slot ownership until restoration succeeds.** restore clears each replacement before calling the fallible platform writer and ignores its result. Failed-install rollback also clears the original pointer. A refused restoration can therefore leave a live detour with no recorded owner or forward target, while ordinary uninstall returns success and permits a new installation. Return the restoration result and retain forwarding state and the outstanding slots until restoration succeeds or the owner is explicitly retired. [runtime/orchestrator/src/frame_tap.rs:331](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/frame_tap.rs#L331)

- [x] **Altitude | High | Retain overridden constant buffers until their original bindings are restored.** begin_constant_override binds the replacement first and saves only the raw pointer from the binding snapshot. The native seed_geometry leaf releases its queried references, relying on the context to keep the bound buffers alive. Replacing a binding can drop that last reference, leaving end_constant_override to rebind a stale address after the draw. Acquire an owned reference to each original before replacing it and release it after restoring the corresponding slot. [runtime/orchestrator/src/frame_tap.rs:1424](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/frame_tap.rs#L1424)

- [x] **Altitude | High | Expire pending Map pointers with the frame-tap installation that observed them.** The process-wide PENDING table stores mapped CPU pointers until a matching Unmap by resource address. Uninstall drains in-flight hooks but neither uninstall nor retire_render clears this table. A Map followed by uninstall and an unobserved Unmap leaves a dangling entry. After reinstall, a later Unmap on that address can pass the old pointer to the new constant watch. Clear pending mappings at drained teardown and tie their identity to the observed context and installation. [runtime/orchestrator/src/frame_tap.rs:3171](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/frame_tap.rs#L3171)

- [x] **Altitude | High | Tie promotion rebuilds to successful retirement of the tap's borrowed views.** Promote::prepare drops its prior Frame on the assumption that the caller cleared the tap. apply_tail in loader/proxy/src/dlss_bridge.c:3618 calls rsf_frame_tap_set_plan(NULL) but ignores a refusal, then prepares new replacements. A concurrent admitted callback can make that clear fail while the tap still holds the old views. Require successful detachment before replacing the frame and make plan publication carry an ownership lease so a refused transition keeps the last valid plan alive. [runtime/orchestrator/src/scene_promote.rs:354](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/scene_promote.rs#L354)

- [x] **Duplication | Medium | Report truncated pixel inputs consistently to every frame-tap draw callback.** divert_facts and consider_target_draw independently enumerate the occupied pixel-shader slots into rsf_frame_tap_target_draw. The reporting branch sets inputs_truncated at runtime/orchestrator/src/frame_tap.rs:1544, but the verdict and override paths at :1334 and :1391 stop at the same MAX_INPUTS limit and leave the flag zero. Thus a callback accepting the public record cannot tell those lists were cut short, even though consumers such as games/ac7/src/scene_color.rs:123 use that flag to refuse incomplete evidence. Extract the bounded input-list collector and populate its count and truncation fields for every draw callback. [runtime/orchestrator/src/frame_tap.rs:1089](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/frame_tap.rs#L1089)

- [x] **Altitude | Medium | Pin the original blend object while its address keys the patched-state cache.** begin_divert obtains a temporary reference to the bound blend, patched_blend_for caches its raw address alongside an owned patched blend, and release_saved drops the reference after restoring the draw. If the game releases that original and D3D11 reuses its address, a later draw gets the previous object's patched color and blend settings. Retain each non-null original until cache eviction, as Samplers already does for the analogous sampler-clone cache. [runtime/orchestrator/src/frame_tap.rs:1151](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/frame_tap.rs#L1151)

- [x] **Altitude | Medium | Invalidate every shadowed pipeline object when ClearState resets the context.** The ClearState detour calls invalidate_geometry, which clears the input layout, vertex shader, constants and target but leaves pixel_shader, blend_state and depth_stencil_state unchanged. Subsequent candidate_passes and common_facts still read those old addresses until new bind calls arrive. This can select a draw using a shader that is no longer bound and report stale blend/depth facts. Reset those fields with the context state and reseed them when the caller requires the actual bindings. [runtime/orchestrator/src/frame_tap.rs:1887](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/frame_tap.rs#L1887)

- [x] **Altitude | Medium | Scale the viewport from the binding actually forwarded.** When the replacement target and depth have different extents and the plan says DEPTH_REFUSE, plan_targets returns the game's original views. After forwarding those views, settle_target_substitution asks only whether the plan has an applicable replacement and marks the original target as substituted. Subsequent viewport and scissor requests are scaled for an output that was never bound. Return the actual substitution decision with the forwarded target and use that fact to update the viewport policy. [runtime/orchestrator/src/frame_tap.rs:2677](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/frame_tap.rs#L2677)

- [x] **Altitude | Low | Declare constant-watch write access when registering the callback.** The bridge's refresh_constant_watch must register a callback and then call set_constant_watch_writable because every registration silently restores write mode. The same mutable-pointer callback is used for a copied writable upload or the game's own read-only memory, depending on that second call. Accept the access mode in one registration and expose const data to observational callbacks, leaving a separate live mode change only for callers that actually switch behavior. [runtime/orchestrator/src/frame_tap.rs:2289](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/frame_tap.rs#L2289)

- [x] **Duplication | Low | Use the frame tap's plan type for scene promotion in the same crate.** The generator explicitly requests the plan, substitution and gate callback twice (tools/generate-rust-bindings.py:69 and :304), creating two nominal Rust definitions of a single contract. Scene promotion fills the first at runtime/orchestrator/src/scene_promote.rs:507 and frame_tap.rs:2208 reads the second defined at runtime/orchestrator/src/frame_tap_abi.rs:337. The proxy passes the very same plan between these calls at loader/proxy/src/dlss_bridge.c:3632 and :3645. Generate the common types once, with the zero default required by promotion, and import them into scene_promote_abi as this crate already does for resource-role records. [runtime/orchestrator/src/scene_promote_abi.rs:50](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/scene_promote_abi.rs#L50)

<a id="g03"></a>

### G03 · Graphics state, composition and resource roles

Port state preservation, role selection, UI composition and resource views together with the device-facing fixtures.

**Coordinate shared edits**

- Observer callbacks, saved D3D state and slot restoration ([G01](#g01), [G02](#g02)).

**Port files (25)**

- [x] [runtime/graphics/include/rescaleframe/d3d11_state.h](runtime/graphics/include/rescaleframe/d3d11_state.h)
- [x] [runtime/graphics/include/rescaleframe/resource_ref.h](runtime/graphics/include/rescaleframe/resource_ref.h)
- [x] [runtime/graphics/include/rescaleframe/resource_roles.h](runtime/graphics/include/rescaleframe/resource_roles.h)
- [x] [runtime/graphics/include/rescaleframe/srv_cache.h](runtime/graphics/include/rescaleframe/srv_cache.h)
- [x] [runtime/graphics/include/rescaleframe/srv_format.h](runtime/graphics/include/rescaleframe/srv_format.h)
- [x] [runtime/graphics/include/rescaleframe/ui_identify.h](runtime/graphics/include/rescaleframe/ui_identify.h)
- [x] [runtime/graphics/include/rescaleframe/ui_layer.h](runtime/graphics/include/rescaleframe/ui_layer.h)
- [x] [runtime/graphics/src/d3d11_state.cpp](runtime/graphics/src/d3d11_state.cpp)
- [x] [runtime/graphics/src/d3d11_state_private.h](runtime/graphics/src/d3d11_state_private.h)
- [x] [runtime/graphics/src/resource_ref.cpp](runtime/graphics/src/resource_ref.cpp)
- [x] [runtime/graphics/src/ui_layer.cpp](runtime/graphics/src/ui_layer.cpp)
- [x] [runtime/orchestrator/include/rescaleframe/native_composition.h](runtime/orchestrator/include/rescaleframe/native_composition.h)
- [x] [runtime/orchestrator/include/rescaleframe/native_translucency.h](runtime/orchestrator/include/rescaleframe/native_translucency.h)
- [x] [runtime/orchestrator/src/com_ref.cpp](runtime/orchestrator/src/com_ref.cpp)
- [x] [runtime/orchestrator/src/native_regions.cpp](runtime/orchestrator/src/native_regions.cpp)
- [x] [runtime/orchestrator/src/native_regions.h](runtime/orchestrator/src/native_regions.h)
- [x] [runtime/orchestrator/src/native_regions_com.cpp](runtime/orchestrator/src/native_regions_com.cpp)
- [x] [runtime/orchestrator/src/native_reinsert.h](runtime/orchestrator/src/native_reinsert.h)
- [x] [runtime/orchestrator/src/native_reinsert_platform.cpp](runtime/orchestrator/src/native_reinsert_platform.cpp)
- [x] [runtime/orchestrator/src/native_translucency.cpp](runtime/orchestrator/src/native_translucency.cpp)
- [x] [runtime/orchestrator/src/native_translucency_com.cpp](runtime/orchestrator/src/native_translucency_com.cpp)
- [x] [runtime/orchestrator/src/sampler_bias_bridge.h](runtime/orchestrator/src/sampler_bias_bridge.h)
- [x] [runtime/orchestrator/src/sampler_bias_com.cpp](runtime/orchestrator/src/sampler_bias_com.cpp)
- [x] [runtime/orchestrator/src/depth_domains_abi.cpp](runtime/orchestrator/src/depth_domains_abi.cpp) (shared SDK predicate thunk)
- [ ] [runtime/orchestrator/src/native_composition_unload.cpp](runtime/orchestrator/src/native_composition_unload.cpp) (CRT teardown registration)

**Fixtures (8)**

- [x] [tests/d3d11_state.cpp](tests/d3d11_state.cpp)
- [x] [tests/native_reinsert.cpp](tests/native_reinsert.cpp)
- [x] [tests/native_translucency.cpp](tests/native_translucency.cpp)
- [x] [tests/native_translucency_abi.cpp](tests/native_translucency_abi.cpp)
- [x] [tests/resource_roles.cpp](tests/resource_roles.cpp)
- [x] [tests/sampler_bias.cpp](tests/sampler_bias.cpp)
- [x] [tests/ui_identify.cpp](tests/ui_identify.cpp)
- [x] [tests/ui_layer.cpp](tests/ui_layer.cpp)

**Rust review findings (6)**

- [x] **Altitude | Medium | Include session identity in the translucency mask lease.** Ledger::open receives session_id but only uses it to match pending cloud depth. The mask key used by open_frame and take contains family, view, native frame and rectangle alone. Two sessions reusing those values on the same D3D11 device can be treated as one open scope, and SR can read the previous session's opaque snapshot and masks. Include session_id in Key and in the same-view close rule, and test identical keys from distinct sessions. [runtime/orchestrator/src/native_translucency.rs:159](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/native_translucency.rs#L159)

- [x] **Duplication | Low | Use the public all-zero depth-domain rule in region preparation.** The depth kernel chooses legacy copying at runtime/orchestrator/src/native_regions.rs:282 using a field-by-field empty_layout predicate that enumerates every scalar, bitwise float and both transform arrays. The published contract at sdk/game/include/rescaleframe/game_renderer.h:45 defines the same decision as any nonzero byte, already used in the OpenGL evaluation path through runtime/orchestrator/src/gl_pipeline_platform.cpp:172. A newly appended field would immediately request mapping in those callers but be ignored by the field list until it too is edited. Reuse the published requested predicate for the full descriptor and keep the separate unused-domain validation at native_regions.rs:303. [runtime/orchestrator/src/native_regions.rs:124](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/native_regions.rs#L124)

- [x] **Duplication | Low | Unbind D3D11 output targets once per temporal-mask dispatch.** The temporal-mask entry invoked from runtime/orchestrator/src/native_regions.cpp:117 unbinds output targets before its optional readable copy at runtime/orchestrator/src/native_regions.rs:804, then repeats the call at :842. The intervening code only creates or looks up SRVs, optionally copies a texture and builds constants; none of it binds an output. The leaf at runtime/orchestrator/src/d3d11_output_platform.cpp:89 issues an actual OMSetRenderTargets call both times. Keep the first unbind so the copy remains legal and remove the second call from the per-frame path. [runtime/orchestrator/src/native_regions.rs:842](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/native_regions.rs#L842)

- [x] **Simplicity | Low | native_reinsert and native_exposure re-implement the SRV cache that native_regions already shares** native_regions.rs:399-424 exports pub(super) SourceView::get(cache, device, texture, format). It returns the cached view when the retained texture matches, otherwise creates an SRV with rsf_d3d11_output_create_srv and retains the texture so its identity cannot be reused. native_translucency.rs:15 and :544-553 already reuse it. native_reinsert.rs:97-136 declares its own SourceView { view, source } and Conversions::source_view with the same body (match retained pointer, else create SRV, retain, insert). native_exposure.rs:34-113 declares a third SourceView { view, source } with a two-entry round-robin of the same create-and-retain step. motion_d3d11.rs:31-92 is a fourth, generic SourceViews<N> LRU (with idle expiry) shared by motion_decode and motion_resolve. Simpler: native_reinsert keeps an Option<native_regions::SourceView> and calls SourceView::get, deleting its private struct and method. native_exposure can do the same with [Option<SourceView>; 2], or use motion_d3d11::SourceViews<2>. Tradeoff: SourceView::get drops the cache entry before creating the replacement while native_reinsert keeps the old view on failure; the shared helper would need to commit only after success (native_exposure already documents "Commit the replacement only after the new SRV is valid"). [runtime/orchestrator/src/native_reinsert.rs:97](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/native_reinsert.rs#L97)

- [x] **Duplication | Low | Reuse the same R32 mask texture and UAV owner in both D3D11 passes.** Translucency creates three MaskTarget objects for reactive, coverage and bias output at runtime/orchestrator/src/native_translucency.rs:467. Region merging independently defines the same two-ComRef type and constructor at runtime/orchestrator/src/native_regions.rs:563 and creates two targets at :797. Both allocate a single-sample, single-mip R32_FLOAT texture with shader-read and unordered-access flags, then call the same rsf_regions_create_uav leaf. Expose one constructor beside the shared region resources so allocation flags, creation-failure cleanup and view lifetime are maintained together; each pass still chooses its own target count. [runtime/orchestrator/src/native_translucency.rs:440](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/native_translucency.rs#L440)

- [x] **Altitude | Low | Give the frame-tap classifier the actual render extent or the adapter's supported range.** Without a known render size, at_render_resolution enforces a fixed half-output minimum for every adapter. frame_tap.rs::role_of always passes zero render dimensions, even though the runtime itself recognizes 3x output/input as Ultra Performance. At that ratio, a correctly shaped motion, depth or scene-color input cannot match any RENDER_SIZED rule. Pass the current render extent when known, or make the uncertain range part of the adapter's rule and retain candidate confidence. [runtime/orchestrator/src/resource_roles.rs:35](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/resource_roles.rs#L35)

<a id="g04"></a>

### G04 · Colour, depth, motion and fullscreen passes

Port colour transport, depth and motion processing and the fullscreen effects, with the format and normalization fixtures.

**Coordinate shared edits**

- Shader inputs, formats and compilation interfaces ([G05](#g05), [G06](#g06)).

**Port files (26)**

- [x] [runtime/graphics/include/rescaleframe/colour_fidelity.h](runtime/graphics/include/rescaleframe/colour_fidelity.h)
- [x] [runtime/graphics/include/rescaleframe/colour_transport.h](runtime/graphics/include/rescaleframe/colour_transport.h)
- [x] [runtime/graphics/include/rescaleframe/depth_replay.h](runtime/graphics/include/rescaleframe/depth_replay.h)
- [x] [runtime/graphics/include/rescaleframe/depth_scale.h](runtime/graphics/include/rescaleframe/depth_scale.h)
- [x] [runtime/graphics/include/rescaleframe/fullscreen_pass.h](runtime/graphics/include/rescaleframe/fullscreen_pass.h)
- [x] [runtime/graphics/include/rescaleframe/motion_decode.h](runtime/graphics/include/rescaleframe/motion_decode.h)
- [x] [runtime/graphics/include/rescaleframe/motion_resolve.h](runtime/graphics/include/rescaleframe/motion_resolve.h)
- [x] [runtime/graphics/include/rescaleframe/present_blit.h](runtime/graphics/include/rescaleframe/present_blit.h)
- [x] [runtime/graphics/src/colour_d3d11_platform.h](runtime/graphics/src/colour_d3d11_platform.h)
- [x] [runtime/graphics/src/colour_fidelity.cpp](runtime/graphics/src/colour_fidelity.cpp)
- [x] [runtime/graphics/src/colour_transport.cpp](runtime/graphics/src/colour_transport.cpp)
- [x] [runtime/graphics/src/depth_platform.h](runtime/graphics/src/depth_platform.h)
- [x] [runtime/graphics/src/depth_replay.cpp](runtime/graphics/src/depth_replay.cpp)
- [x] [runtime/graphics/src/depth_scale.cpp](runtime/graphics/src/depth_scale.cpp)
- [x] [runtime/graphics/src/depth_scale_private.h](runtime/graphics/src/depth_scale_private.h)
- [x] [runtime/graphics/src/fullscreen_pass.cpp](runtime/graphics/src/fullscreen_pass.cpp)
- [x] [runtime/graphics/src/fullscreen_platform.h](runtime/graphics/src/fullscreen_platform.h)
- [x] [runtime/graphics/src/motion_d3d11_platform.h](runtime/graphics/src/motion_d3d11_platform.h)
- [x] [runtime/graphics/src/motion_decode.cpp](runtime/graphics/src/motion_decode.cpp)
- [x] [runtime/graphics/src/motion_decode_platform.h](runtime/graphics/src/motion_decode_platform.h)
- [x] [runtime/graphics/src/motion_resolve.cpp](runtime/graphics/src/motion_resolve.cpp)
- [x] [runtime/graphics/src/motion_resolve_platform.h](runtime/graphics/src/motion_resolve_platform.h)
- [x] [runtime/graphics/src/present_blit.cpp](runtime/graphics/src/present_blit.cpp)
- [x] [runtime/orchestrator/src/motion_convention.h](runtime/orchestrator/src/motion_convention.h)
- [x] [tools/exposure_probe/ngx.cpp](tools/exposure_probe/ngx.cpp) (pinned NGX virtual-parameter calls; replaced by C11 `tools/exposure_probe/ngx.c`, which calls the SDK vtable slots directly)
- [x] [tools/exposure-probe.cpp](tools/exposure-probe.cpp)

**Fixtures (6)**

- [x] [tests/colour_transport.cpp](tests/colour_transport.cpp)
- [x] [tests/depth_scale.cpp](tests/depth_scale.cpp)
- [x] [tests/exposure_normalization.cpp](tests/exposure_normalization.cpp)
- [x] [tests/fullscreen_pass.cpp](tests/fullscreen_pass.cpp)
- [x] [tests/motion_decode.cpp](tests/motion_decode.cpp)
- [x] [tests/motion_resolve.cpp](tests/motion_resolve.cpp)

**Rust review findings (5)**

- [x] **Altitude | Medium | Distinguish a missing optional depth layer from a rejected supplied layer.** When a non-null depth layer has the wrong device, format, extent or fails SRV creation, resolve sets has_layer to zero and can report success exactly as if no layer had been requested. native_sr.rs:208 supplies the layer for motion reconstruction, and native_fg.rs:1083 clears it downstream on the assumption D3D11 consumed it. Return a failure or an explicit layer-use result so the caller can choose whether scene-depth-only reprojection is acceptable. [runtime/orchestrator/src/motion_resolve.rs:162](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/motion_resolve.rs#L162)

- [x] **Boundaries | Low | Move Unreal motion and jitter conventions out of the runtime backend crate.** runtime/backends/rsf-upscaler is a Runtime workspace crate, yet it encodes engine storage conventions: Encoding::UNREAL_G16R16 and MotionVectors::unreal_object_only (motion.rs:42, :73), Jitter::from_unreal_clip and MotionToPixels::unreal (frame.rs:40, :97), recommended_phase_count tuned for r.TemporalAASamples (frame.rs:68), and AC7 fixtures in lib.rs tests. AGENTS.md Rule 1 places native engine layouts and conventions in games/<id> or integrations/, with the runtime receiving adapter-supplied facts such as rsf_motion_decode_params. No crate, CMake target or C++ file depends on rsf-upscaler (the only mention is a comment in ui/overlay/src/model.rs:10), so the Runtime-side copy also disagrees silently with the live decode path. Delete the crate, or keep only the vendor-neutral check()/Reasons/Quality types in runtime and move the Unreal encodings and AC7 tests to games/ac7 where the decode parameters are actually produced. [runtime/backends/rsf-upscaler/src/motion.rs:42](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/backends/rsf-upscaler/src/motion.rs#L42)

- [x] **Duplication | Low | Use the same rectangle and jitter predicates for auxiliary color and depth scaling.** DepthScale::render checks its integer source/output rectangles and finite +/-16384-pixel jitter at runtime/orchestrator/src/depth_scale.rs:176. The auxiliary surface validator independently implements the same integer output bounds and jitter limit at runtime/orchestrator/src/auxiliary_sr.rs:103 and :117, consumed by SurfacePlan validation and the draw path. A change to allowed auxiliary coordinates currently needs two implementations with separate boundary tests. Share the integer rectangle and jitter predicates, leaving auxiliary color's fractional source bounds in Mapping. [runtime/orchestrator/src/depth_scale.rs:78](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/depth_scale.rs#L78)

- [x] **Simplicity | Low | Auxiliary SR reaches the Rust depth-scale owner through two C layers** AuxiliarySr (auxiliary_sr.rs:548-561, 687-692, 769-770, 808-814, 862) drives depth scaling through rsf_aux_platform.depth_create/reserve/prepare/redraw/commit/restore/discard/destroy. auxiliary_sr.cpp:179 fills those slots with the public rsf_depth_scale_* functions, and runtime/graphics/src/depth_scale.cpp:154-191 forwards each one to rsf_core_depth_scale_* with its static platform, which lands back in DepthScale in this same orchestrator crate (depth_scale.rs:291-402). Each call therefore goes Rust -> C++ table -> C++ wrapper -> Rust export, the handle travels as an opaque rsf_depth_scale*, and every step re-checks nulls, thread ownership and ABI fields that the Rust caller already holds. Simpler: give AuxiliarySr an Option<Box<DepthScale>> created with the depth Platform pointer (one pointer in rsf_aux_platform instead of eight leaves) and call reserve/render/commit/restore/discard directly; keep the public rsf_depth_scale_* C entry points only for their remaining C caller (tests/depth_scale.cpp). Tradeoff: DepthScale becomes pub(crate) and the aux platform gains one pointer; removes eight Option leaves and two forwarding layers. [runtime/orchestrator/src/depth_scale.rs:291](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/depth_scale.rs#L291)

- [x] **Duplication | Low | Centralize adoption of partially populated D3D11 resource arrays.** Depth scaling wraps all fourteen returned COM slots before checking the native result, rejects any missing resource and unwraps the result array. Depth replay repeats this with three slots at runtime/orchestrator/src/depth_replay.rs:301, fullscreen reinsertion spells out nine slots at runtime/orchestrator/src/fullscreen_pass.rs:195, and motion resolve lists six at runtime/orchestrator/src/motion_resolve.rs:112. Each create path owns even partial outputs on failure, but their native status conventions differ. Add a const-generic all-or-none adopt operation beside ComRef, with each caller still translating its own status, so a future slot addition cannot skip release or validation. [runtime/orchestrator/src/depth_scale.rs:319](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/depth_scale.rs#L319)

<a id="g05"></a>

### G05 · Sharpening and shader assets

Port host wrappers, options and bindings for sharpening. Track shader provenance with each payload.

**Coordinate shared edits**

- Shader inputs, formats and compilation interfaces ([G04](#g04), [G06](#g06)).

**Port files (19)**

- [x] [runtime/backends/common/sharpening.h](runtime/backends/common/sharpening.h)
- [x] [runtime/graphics/src/sharpening/d3d11.cpp](runtime/graphics/src/sharpening/d3d11.cpp)
- [x] [runtime/graphics/src/sharpening/d3d11.h](runtime/graphics/src/sharpening/d3d11.h)
- [x] [runtime/graphics/src/sharpening/d3d12.cpp](runtime/graphics/src/sharpening/d3d12.cpp)
- [x] [runtime/graphics/src/sharpening/d3d12.h](runtime/graphics/src/sharpening/d3d12.h)
- [x] [runtime/graphics/src/sharpening/d3d12_platform.h](runtime/graphics/src/sharpening/d3d12_platform.h)
- [x] [runtime/graphics/src/sharpening/da_das.h](runtime/graphics/src/sharpening/da_das.h) (unchanged licensed shader payload)
- [x] [runtime/graphics/src/sharpening/da_das_spirv.h](runtime/graphics/src/sharpening/da_das_spirv.h) (unchanged licensed shader payload)
- [x] [runtime/graphics/src/sharpening/da_rcas.h](runtime/graphics/src/sharpening/da_rcas.h) (unchanged licensed shader payload)
- [x] [runtime/graphics/src/sharpening/da_rcas_spirv.h](runtime/graphics/src/sharpening/da_rcas_spirv.h) (unchanged licensed shader payload)
- [x] [runtime/graphics/src/sharpening/depth_color.inc](runtime/graphics/src/sharpening/depth_color.inc) (C++ raw string containing shared HLSL)
- [x] [runtime/graphics/src/sharpening/pass_payload.cpp](runtime/graphics/src/sharpening/pass_payload.cpp) (retained GPL shader access and constant packing; now GPL-3.0-only C11 `pass_payload.c`, byte-identical output to the C++ unit)
- [x] [runtime/graphics/src/sharpening/options.h](runtime/graphics/src/sharpening/options.h) (now a C11 header, GPL-3.0-only; the HLSL and SPIR-V payload headers use C literals and arrays)
- [x] [runtime/graphics/src/sharpening/rcas.h](runtime/graphics/src/sharpening/rcas.h) (unchanged licensed shader payload)
- [x] [runtime/graphics/src/sharpening/rcas_spirv.h](runtime/graphics/src/sharpening/rcas_spirv.h) (unchanged licensed shader payload)
- [x] [runtime/orchestrator/include/rescaleframe/sharpening_settings.h](runtime/orchestrator/include/rescaleframe/sharpening_settings.h)
- [x] [runtime/orchestrator/src/sharpening_pass_options.h](runtime/orchestrator/src/sharpening_pass_options.h)
- [x] [runtime/orchestrator/src/sharpening_vulkan.h](runtime/orchestrator/src/sharpening_vulkan.h)
- [x] [runtime/orchestrator/src/sharpening_vulkan_core.h](runtime/orchestrator/src/sharpening_vulkan_core.h)

**Fixtures (7)**

- [x] [tests/engine_sharpening.cpp](tests/engine_sharpening.cpp)
- [x] [tests/sharpening.cpp](tests/sharpening.cpp)
- [x] [tests/sharpening_d3d12.cpp](tests/sharpening_d3d12.cpp)
- [x] [tests/sharpening_settings_native.cpp](tests/sharpening_settings_native.cpp) (C11 SDK descriptor and CRT fixture probes)
- [x] [tests/sharpening_settings.cpp](tests/sharpening_settings.cpp)
- [x] [tests/sharpening_vulkan.cpp](tests/sharpening_vulkan.cpp)
- [x] [tests/sharpening_vulkan_cpu.cpp](tests/sharpening_vulkan_cpu.cpp)

**Rust review findings (4)**

- [x] **Simplicity | Low | Pass the admitted sharpening device and slot through recording.** record returns early unless initialize succeeds. Neither free_slot, prepare nor create_views can remove the device, yet the remaining path tests self.device four times and indexes the same slot separately in each helper. After choosing the index, borrow the device once and the slot mutably from the disjoint slots field. Let prepare and create_views take these references and use the same ones for refusal cleanup and recording. This adds explicit helper parameters while preserving the failed-bind latch, allocation order and slot retirement behavior. [runtime/orchestrator/src/vulkan_sharpening.rs:145](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_sharpening.rs#L145)

- [x] **Duplication | Low | Report optional Vulkan sharpening failure from one owner.** Shared sharpenvk_record already owns the once-only fallback diagnostic. FidelityFX SR at runtime/orchestrator/src/vulkan_sr.rs:745 and XeSS at runtime/orchestrator/src/vulkan_sr/xess.rs:597 keep separate flags, repeat the message and perform their own logging because native_sr_vulkan.cpp:399 and native_xess_vulkan.cpp:254 select the silent execute wrapper. DLSS and FSR4 already use the reporting run wrapper. Delegate once-per-owner logging to one facade path for all four providers, including failed allocation of its Rust owner, while preserving each caller's successful vendor output. [runtime/orchestrator/src/vulkan_sharpening.rs:372](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_sharpening.rs#L372)

- [x] **Duplication | Low | Keep sharpening activation policy out of the Vulkan owner's fake platform.** The off/vendor/zero-strength owner test at :372 passes through a fake that implements its own mode/strength predicate. The real callback at runtime/orchestrator/src/sharpening_vulkan.h:164 sanitizes first and then calls runtime/graphics/src/sharpening/options.h:37. For an active filter with NaN sharpness and MAS off, the fake returns inactive while native sanitization at options.h:26 restores 0.4 and dispatches. Make the owner fixture explicitly supply the callback's active/inactive result and test the option combinations against the actual native policy at its boundary. [runtime/orchestrator/src/vulkan_sharpening/tests.rs:204](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_sharpening/tests.rs#L204)

- [x] **Duplication | Low | Generate the Vulkan sharpening callback and pass records from their native header.** Rust manually mirrors the native facts, pass and 15-slot callback table in runtime/orchestrator/src/sharpening_vulkan_core.h:22, :30 and :37. Vulkan sharpening builds a pass at runtime/orchestrator/src/vulkan_sharpening.rs:260 and native code consumes it through the table populated at runtime/orchestrator/src/sharpening_vulkan.h:218. The Rust assertions at :104 check only selected field offsets and table size, so reordering equally sized fields or callbacks can compile on both sides. Add these private records to tools/generate-rust-bindings.py alongside the related Vulkan records at :279, blocklisting the shared resource and setup types. [runtime/orchestrator/src/vulkan_sharpening_abi.rs:40](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_sharpening_abi.rs#L40)

<a id="g06"></a>

### G06 · Shader patching and compilation

Port shader patch sessions, compiler interfaces and the cache with their CPU and device fixtures.

**Coordinate shared edits**

- Shader inputs, formats and compilation interfaces ([G04](#g04), [G05](#g05)).

**Port files (5)**

- [x] [runtime/graphics/include/rescaleframe/shader_compile.h](runtime/graphics/include/rescaleframe/shader_compile.h)
- [x] [runtime/shader_patch/include/rescaleframe/shader_patch.h](runtime/shader_patch/include/rescaleframe/shader_patch.h)
- [x] [runtime/shader_patch/src/export.cpp](runtime/shader_patch/src/export.cpp)
- [x] [runtime/shader_patch/src/platform.cpp](runtime/shader_patch/src/platform.cpp)
- [x] [runtime/shader_patch/src/platform.h](runtime/shader_patch/src/platform.h)

**Fixtures (2)**

- [x] [tests/shader_patch.cpp](tests/shader_patch.cpp)
- [x] [tests/shader_patch_device.cpp](tests/shader_patch_device.cpp)

**Rust review findings (4)**

- [x] **Altitude | Medium | Use one admitted root record for shader preparation and metadata queries.** Session open stores Option<Root> while prepare_variant independently walks raw JSON. The session fixture explicitly asserts open succeeds, root_count includes both roots, root returns FORMAT for the missing patched hash, yet prepare succeeds for that same root. Clients cannot enumerate the set they can prepare. Validate roots once before claiming them, or represent genuinely optional metadata in a queryable root record, and make enumeration and compilation use that same record. [runtime/orchestrator/src/shader_patch.rs:308](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/shader_patch.rs#L308)

- [x] **Altitude | Medium | Report incomplete shader inventory dumps to the requesting adapter.** Once the directory exists, dump ignores each DXBC write, disassembly result and assembly file write and returns OK. Anomaly's motion_stage.cpp:351 forwards that result, so a full disk or disassembler refusal looks like a complete inventory. Continue collecting independent entries but return a partial/failure result and identify unsuccessful shader hashes in the dump report. [runtime/orchestrator/src/shader_patch/d3d11.rs:217](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/shader_patch/d3d11.rs#L217)

- [x] **Quality | Low | Failed hook enable frees trampolines of hooks that are already live** attach() enables the six CreateXShader hooks in order (lines 101-105). If hook_enable fails for hook i, the rollback at lines 106-111 calls hook_remove on every created target. hook_remove in platform.cpp:229-233 does MH_DisableHook and then MH_RemoveHook, which frees the trampoline. Hooks 0..i-1 were already enabled, so an engine thread that entered create::<I> (line 42) can still load ORIGINALS[I] (line 62) and call the freed trampoline. ID3D11Device shader creation is free-threaded, so concurrent creation during loading can reach this. reflex_audit.rs:75-80 handles the same case by disabling and keeping the trampolines. This is a race inference: the code path is confirmed, but the crash depends on timing. No test covers a partial enable failure. [runtime/orchestrator/src/shader_patch/d3d11.rs:106](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/shader_patch/d3d11.rs#L106)

- [x] **Duplication | Low | Test shader-pack admission without maintaining a second JSON parser.** Pack::load calls the platform parser through runtime/orchestrator/src/shader_patch/pack.rs:177; production uses yyjson_read at runtime/shader_patch/src/platform.cpp:71. The Rust test Reader and fake_parse instead implement a different JSON grammar: a Unicode escape is parsed into one u8 and unrecognized backslash escapes are accepted as their literal byte. The malformed-JSON assertion at shader_patch/tests.rs:477 consequently exercises a different parser. Test Rust-specific builder rules with explicit visitor events, and run syntax/Unicode fixtures through the existing native parser contract, retaining fake compiler and reflection leaves. [runtime/orchestrator/src/shader_patch/tests.rs:38](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/shader_patch/tests.rs#L38)

<a id="g07"></a>

### G07 · Texture and frame capture

Port input dumps, texture readback, capture metadata and completion; account for partial allocation and retirement.

**Coordinate shared edits**

- Capture publication after a submitted command or queue is retired ([V02](#v02), [V04](#v04), [V05](#v05), [V06](#v06)).

**Port files (12)**

- [x] [runtime/backends/common/frame_capture12.h](runtime/backends/common/frame_capture12.h)
- [x] [runtime/backends/common/frame_capture_vk.h](runtime/backends/common/frame_capture_vk.h)
- [x] [runtime/graphics/include/rescaleframe/texture_dump.h](runtime/graphics/include/rescaleframe/texture_dump.h)
- [x] [runtime/graphics/src/texture_dump.cpp](runtime/graphics/src/texture_dump.cpp)
- [x] [runtime/graphics/src/texture_dump_platform.h](runtime/graphics/src/texture_dump_platform.h)
- [x] [runtime/orchestrator/include/rescaleframe/input_dump.h](runtime/orchestrator/include/rescaleframe/input_dump.h)
- [x] [runtime/orchestrator/include/rescaleframe/input_dump_d3d12.h](runtime/orchestrator/include/rescaleframe/input_dump_d3d12.h)
- [x] [runtime/orchestrator/include/rescaleframe/input_dump_vulkan.h](runtime/orchestrator/include/rescaleframe/input_dump_vulkan.h)
- [x] [runtime/orchestrator/include/rescaleframe/named_frame_dump.h](runtime/orchestrator/include/rescaleframe/named_frame_dump.h)
- [x] [runtime/orchestrator/src/input_dump.cpp](runtime/orchestrator/src/input_dump.cpp)
- [x] [runtime/orchestrator/src/input_dump_platform.h](runtime/orchestrator/src/input_dump_platform.h)
- [x] [runtime/orchestrator/src/input_dump_vulkan.cpp](runtime/orchestrator/src/input_dump_vulkan.cpp)

**Fixtures (6)**

- [x] [tests/capture_metadata.cpp](tests/capture_metadata.cpp)
- [x] [tests/input_dump.cpp](tests/input_dump.cpp)
- [x] [tests/input_dump_d3d12.cpp](tests/input_dump_d3d12.cpp)
- [x] [tests/input_dump_vulkan.cpp](tests/input_dump_vulkan.cpp)
- [x] [tests/input_dump_vulkan_cpu.cpp](tests/input_dump_vulkan_cpu.cpp)
- [x] [tests/texture_dump.cpp](tests/texture_dump.cpp)

**Rust review findings (13)**

- [x] **Altitude | Medium | Preserve capture view and resource generation in legacy filenames.** The D3D12 exporter matches submissions with session, frame, view and resource generation, but writes raw/JSON files using only session, frame, role and ordinal. Its event file also omits view and generation. Vulkan's legacy writer at frame_capture_vk.rs:673 fixes the ordinal at zero and omits the same identity. Two captures of the same role for different views of one source overwrite each other even though their metadata describes different inputs. Have the capture owner derive artifact names from the complete identity and distinguish repeated evaluations when necessary. [runtime/orchestrator/src/frame_capture12.rs:348](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/frame_capture12.rs#L348)

- [x] **Duplication | Medium | Serialize shared legacy capture metadata once for D3D12 and Vulkan.** D3D12 readback publication and Vulkan publish_legacy call separate legacy_json writers (runtime/orchestrator/src/frame_capture_vk.rs:609). Both interpolate the unescaped role into JSON, emit the same identity, extents and jitter, and manually maintain the same five row-vector matrix names and encodings. A role containing a quote makes both sidecars invalid. Share the frame/camera writer and JSON string escaping, with separate layout, reset, provider and completion fields so existing backend-specific evidence is retained. [runtime/orchestrator/src/frame_capture12.rs:502](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/frame_capture12.rs#L502)

- [x] **Altitude | Medium | Apply the same execution proof to the Vulkan legacy file and the core ticket.** For a shared-completion readback with both outputs enabled, full flush can reject the core ticket because gpu_ready has no execution proof, then map the very same allocation and write it as a legacy capture. A drain can retire an abandoned command buffer without executing its image copy. The default capture in native_sr_fsr4_vulkan.cpp uses this shared path. Make completion an attribute of the readback, require it for either publication and retain drain-only proof solely for freeing the allocation. [runtime/orchestrator/src/frame_capture_vk.rs:518](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/frame_capture_vk.rs#L518)

- [x] **Duplication | Medium | Use one Rust pixel descriptor for capture publication and input-dump ingestion.** The input-dump ABI reads a hand-written seven-field Pixels record (runtime/orchestrator/src/input_dump/abi.rs:192). Vulkan publishes a separate manual definition of the same rsf_dump_pixels at runtime/orchestrator/src/frame_capture_vk.rs:38 and :523; D3D12 imports that producer type and reaches the reader through rsf_input_dump_pixels at runtime/backends/common/frame_capture12.h:130. The intervening C boundary lets the two Rust definitions drift independently on format, byte count or pointer offsets. Generate or declare the descriptor once from the public texture-dump header and import it in producers and consumers. [runtime/orchestrator/src/input_dump.rs:73](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/input_dump.rs#L73)

- [x] **Altitude | Medium | Derive capture completeness from the operations the selected frame actually performed.** The record-based path requires only final_color and a written frame record. input_dump/abi.rs:78 initializes sr_result to success before any SDK result arrives, and roles with no reservation do not count as failures. A frame using SR can consequently get a complete manifest with no scene color, motion, depth or evaluation result. Derive required evidence from the selected path, keep unobserved SDK results explicit, and preserve final-only completion for a frame explicitly identified as native. [runtime/orchestrator/src/input_dump.rs:478](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/input_dump.rs#L478)

- [x] **Altitude | Medium | Certify GPU completion for the recordings a submission actually contains.** gpu_submission encodes the capture ticket and the current count of all image reservations. gpu_completed advances one count per API/device, and gpu_ready treats every earlier index as executed. input_dump_vulkan.rs:142 publishes that count when one snapshot queue retires. An earlier reservation on another command buffer or queue of the same device is thereby eligible for mapping even if its copy never ran. Attach reservation IDs to the submitted command batch and advance only those receipts on completion. [runtime/orchestrator/src/input_dump/abi.rs:272](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/input_dump/abi.rs#L272)

- [x] **Duplication | Medium | Use the same camera availability check when writing and updating capture metadata.** The first frame.json written by rsf_core_dump_begin_record at runtime/orchestrator/src/input_dump/abi.rs:80 calls the camera available whenever its struct_size is large enough. Capture::fill_sr_metadata at runtime/orchestrator/src/input_dump.rs:400 also checks RSF_GAME_CAMERA_ABI_VERSION before trusting the same embedded record. A full-size camera from another ABI is consequently labelled available in the file while the owner considers it replaceable. Reuse the size-and-version predicate for both the flag and enrichment decision, leaving the dimension match as a separate enrichment requirement. [runtime/orchestrator/src/input_dump/metadata.rs:352](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/input_dump/metadata.rs#L352)

- [x] **Altitude | Medium | Use the same source-provenance rule in every capture artifact.** manifest and frame.json emit null for source_frame_id when RSF_FRAME_FLAG_AMBIGUOUS_ID is set. The same capture's stage-result and SR-parameter writers instead emit record.frame_id as a known source, although matches_pass expressly permits that value to be a native frame counter without a known source. Centralize identity serialization and keep the native frame ID separately so tools cannot join a status file to an input frame on fabricated evidence. [runtime/orchestrator/src/input_dump/metadata.rs:425](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/input_dump/metadata.rs#L425)

- [x] **Altitude | Medium | Distinguish device loss from a pending Vulkan capture fence.** Owner::join recognizes DEVICE_LOST and discards the snapshot but converts the state to FenceOnly and returns NOT_READY, just as for a timeout. The only destroy path calls join again, so a permanently lost device prevents release of the command pool, fence and native capture owner. Return a terminal device-lost outcome and provide teardown after loss is established, while preserving retry for genuinely pending work. [runtime/orchestrator/src/input_dump_vulkan.rs:131](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/input_dump_vulkan.rs#L131)

- [x] **Boundaries | Medium | Derive the velocity preview from adapter-supplied motion encoding, not an Unreal constant.** The Runtime texture dump hard-codes Unreal's encoded-velocity convention: VELOCITY_SCALE = 0.499*0.5 and VELOCITY_BIAS = 32767/65535 (sample.rs:12-13) with "x != 0 means written" for view 1 (sample.rs:180-184). The public header names it "Treat the texture as Unreal velocity" (runtime/graphics/include/rescaleframe/texture_dump.h:46-48). The Runtime dlss_pipeline.rs:1345-1349 and :1520-1525 then label any game motion with zero_means_unwritten as VIEW_VELOCITY, and the AC7 C carrier requests it directly (loader/proxy/src/dinput8_proxy.c:762). AGENTS.md Rule 1 keeps engine conventions out of runtime/orchestrator; the adapter already supplies motion encoding facts (scale/bias/sentinel) to the motion decode owner, so previews of a non-Unreal encoded motion buffer are decoded with the wrong constants. Pass the adapter's decode parameters into the dump request (or dump only the already-decoded motion) and retire the engine-named view. [runtime/orchestrator/src/texture_dump/sample.rs:12](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/texture_dump/sample.rs#L12)

- [x] **Duplication | Low | Share D3D12 readback allocation and copy leaves between frame capture and FG color pairs.** FrameCapture12 and runtime/orchestrator/src/fg12/colors.rs:25 both request a COPY_DEST buffer and the same six-field footprint, then record a subresource-zero copy and map/unmap it. Runtime/backends/common/frame_capture12.h:64 and runtime/orchestrator/src/fg12_platform.cpp:94 independently allocate and fill that footprint, transition and restore the source, and map the identical byte range. The common capture leaf checks single-slice textures while the FG pair has its own format and matching-extent rules. Use one footprint type and shared readback leaves, retaining each caller's admission, fence ownership and file format. [runtime/orchestrator/src/frame_capture12.rs:44](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/frame_capture12.rs#L44)

- [x] **Duplication | Low | Share the capture-ticket test sink between Vulkan and D3D12.** The Vulkan reserve/failed/pixels callbacks and runtime/orchestrator/src/frame_capture12/tests.rs:145 simulate the same input-dump ticket API with independent ticket counters and tuple logs. Vulkan's pixels callback ignores the supplied native format and its reserve callback ignores the description, while D3D12 preserves or validates those fields. Both suites exercise publication and failure on owner destruction (Vulkan :463 and D3D12 :657). Extract a test sink recording the full common callback payload so both can assert it, while keeping the Vulkan event model and D3D12 COM/fence fixtures separate. [runtime/orchestrator/src/frame_capture_vk/tests.rs:118](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/frame_capture_vk/tests.rs#L118)

- [x] **Duplication | Low | Share Vulkan capture command-pool and fence operations.** The public snapshot owner and the DLSS real-Present capture owner at runtime/orchestrator/src/dlss_vulkan_capture.rs:81 each declare the same ten Vulkan pool, command-buffer and fence operations. Their native implementations separately resolve the same functions and populate the same Vulkan create structures at runtime/orchestrator/src/input_dump_vulkan.cpp:53 and runtime/orchestrator/src/native_fg_dlss_vulkan_capture.cpp:106. The snapshot owner allocates one queue family at open, while DLSS creates a slot lazily per observed family and preserves WSI waits. Factor the common command/fence leaves and slot allocation/reset/release into a reusable component; leave submission, completion evidence and wait consumption with each owner. [runtime/orchestrator/src/input_dump_vulkan.rs:27](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/input_dump_vulkan.rs#L27)

<a id="d01"></a>

### D01 · D3D11 rendering host

Port host setup, output admission and reduced-frame handling through the shared D3D11 host.

**Coordinate shared edits**

- Shared device, chain and SR handoff ([D02](#d02), [D03](#d03)).

**Port files (5)**

- [x] [runtime/orchestrator/include/rescaleframe/d3d11_host.h](runtime/orchestrator/include/rescaleframe/d3d11_host.h)
- [x] [runtime/orchestrator/src/d3d11_host.cpp](runtime/orchestrator/src/d3d11_host.cpp)
- [x] [runtime/orchestrator/src/d3d11_host_platform.h](runtime/orchestrator/src/d3d11_host_platform.h)
- [x] [runtime/orchestrator/src/d3d11_output_platform.cpp](runtime/orchestrator/src/d3d11_output_platform.cpp)
- [x] [runtime/orchestrator/src/d3d11_output_platform.h](runtime/orchestrator/src/d3d11_output_platform.h)

**Fixtures (3)**

- [x] [tests/d3d11_host.cpp](tests/d3d11_host.cpp)
- [ ] [tests/d3d11_host_rendered_fg_device.cpp](tests/d3d11_host_rendered_fg_device.cpp)
- [ ] [tests/d3d11_host_shared_sr_device.cpp](tests/d3d11_host_shared_sr_device.cpp)

**Rust review findings (8)**

- [x] **Altitude | Medium | Carry depth inversion and the far-plane convention as independent D3D11 facts.** install_bridge copies setup.depth_inverted into both depth_inverted and depth_infinite. The public host setup can therefore express neither reversed finite depth nor forward infinite depth, even though the presenter forwards both flags independently and FSR generation uses them for separate create flags. Append a size/version-checked far-plane convention supplied by the adapter; retain the legacy interpretation only for old callers and cover all four combinations. [runtime/orchestrator/src/d3d11_host.rs:584](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/d3d11_host.rs#L584)

- [x] **Altitude | Medium | Report the inputs observed on the actual D3D11 SR pass.** The host sets color, depth, motion and jitter availability on every report, including configure-only requests. Skyrim resolution.rs constructs those requests with output dimensions and a swapchain but no input textures or valid camera. overlay_service.rs:834 turns the bitmask into availability indicators, so a preparation request can claim all inputs were supplied. Leave the input receipt untouched on preparation or derive its bits from the qualified evaluation's resource and camera checks. [runtime/orchestrator/src/d3d11_host.rs:1234](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/d3d11_host.rs#L1234)

- [x] **Duplication | Medium | Decode versioned CPU source packets once for dispatch and diagnostics.** runtime/orchestrator/src/d3d11_host.rs:64 and runtime/orchestrator/src/native_cpu.rs:22 independently define the original source packet as the base event plus 24 bytes. The dispatcher at native_cpu.rs:180 rejects intermediate sizes between that prefix and the full record, while the D3D11 diagnostic reader at d3d11_host.rs:1901 treats every such size as the legacy prefix and decodes generation and abort reason after dispatch. A future extension can change which fields the two paths consider valid. Share a bounded decoder using the actual field offset, with distinct legacy/full/malformed results, and let diagnostics report the raw header and rejection without interpreting malformed payloads. [runtime/orchestrator/src/d3d11_host.rs:1901](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/d3d11_host.rs#L1901)

- [x] **Altitude | Medium | Check the pass session at the common D3D11 SR admission boundary.** host_process validates role, size, swapchain and camera but forwards an ordinary pass to Host::process without comparing its session_id to the host. Other render-pass and auxiliary entry points at lines 2143 and 2642 perform that check. A mismatched pass targeting the bound chain can drive the host's context and label captures with a foreign session. Check session identity before any graphics initialization, evaluation or capture in the shared process path. [runtime/orchestrator/src/d3d11_host.rs:2213](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/d3d11_host.rs#L2213)

- [x] **Duplication | Medium | Use the generated swapchain dispatch request on both sides of the Steam facade.** integrations/steam/src/chain_abi.rs:18 describes the public 72-byte dispatch record with typed argument and result unions. integrations/steam/src/lib.rs:167 builds it and :178 forwards it to runtime/orchestrator/src/d3d11_host.rs:2370. The runtime redeclares the record at d3d11_host_abi.rs:99 with a signed result and a raw five-word argument union; the Present adapter at d3d11_host.rs:2425 constructs that mirror. A new largest method argument or result interpretation currently requires updating the raw representation separately. Move the generated request and method constants to shared D3D11 host bindings, and use the named Present and Release variants at each end. [runtime/orchestrator/src/d3d11_host_abi.rs:99](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/d3d11_host_abi.rs#L99)

- [x] **Duplication | Medium | Compare reserved auxiliary surface identity with one predicate.** Ticket::temporal_surfaces at runtime/orchestrator/src/d3d11_host_reduced.rs:236 compares minimum size, texture, kind, flags and output rectangle. Host::process checks it at d3d11_host.rs:972, then calls temporal_prepare at :1100, whose SurfacePlan::accepts at auxiliary_sr.rs:155 repeats the same identity fields before checking source bounds and jitter. The earlier check distinguishes an unrelated request from a failed preparation, so keep both admission points. Share a typed immutable surface key or matcher and retain the stricter mapping checks in the products owner to avoid future identity fields being enforced on only one route. [runtime/orchestrator/src/d3d11_host_reduced.rs:236](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/d3d11_host_reduced.rs#L236)

- [x] **Simplicity | Low | D3D11 host advanced options are a positional index protocol mirrored in four places** The five overlay options exist only as positions: initialize builds the `advanced` toggle array in order capture-next, transport, correction, capture-failures, capture-sequence (d3d11_host.rs:735-750); host_options rebuilds the current values in the same order with literal zeros at 0 and 4 (:2190-2196); action() decodes index 0|4, 3 and 1|2 with nested ifs choosing the flag and INI key (:1932-1971); and rsf_core_d3d11_host_capture_control maps its operation back to the magic indices 4 and 3 (:2344-2348). Adding, removing or reordering one option means editing all four in lockstep with nothing tying them together. Simpler: one `enum Advanced { CaptureNext, Transport, Correction, CaptureFailures, CaptureSequence }` plus a const table of (label, kind: OneShot(&AtomicBool) | Persisted(&AtomicBool, ini key, bumps_generation)), from which the toggle array, the current values and action() are all derived, and capture_control names the variant. Tradeoff: a small table type; the overlay ABI still receives the same ordered option array. [runtime/orchestrator/src/d3d11_host.rs:1932](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/d3d11_host.rs#L1932)

- [x] **Boundaries | Low | Call the auxiliary SR owner directly instead of through its own C exports.** d3d11_host_reduced.rs declares rsf_auxiliary_sr_create/reserve/seal/commit/restore and the rest as unsafe extern "C" imports and wraps the returned opaque pointer in Auxiliary, but those symbols are #[no_mangle] functions of the same crate (runtime/orchestrator/src/auxiliary_sr.rs:886-1002) operating on the Rust AuxiliarySr type. The D3D11 host therefore crosses an unchecked C ABI to reach a sibling Rust module: the handle loses its type, every call re-validates raw pointers and slices, the slice lengths are narrowed to u32, and a signature drift between the two declarations is not caught by the compiler. This is an avoidable unsafe FFI boundary inside one Rust owner. Make AuxiliarySr pub(crate), implement Products for it (or hold Box<AuxiliarySr>) and call its methods directly; keep the C exports only for the remaining native callers, if any. [runtime/orchestrator/src/d3d11_host_reduced.rs:55](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/d3d11_host_reduced.rs#L55)

<a id="d02"></a>

### D02 · D3D11 presentation and shared surfaces

Port swapchain dispatch, presentation synchronization and shared surface ownership with their consumer fixtures.

**Coordinate shared edits**

- Shared device, chain and SR handoff ([D01](#d01), [D03](#d03)).
- OpenGL shared surfaces, device ownership and linked Minecraft callers ([D08](#d08), [M01](#m01)).

**Port files (8)**

- [x] [runtime/orchestrator/include/rescaleframe/d3d11_present_api.h](runtime/orchestrator/include/rescaleframe/d3d11_present_api.h)
- [x] [runtime/orchestrator/src/d3d11_present_api.cpp](runtime/orchestrator/src/d3d11_present_api.cpp)
- [x] [runtime/presentation/include/rescaleframe/d3d11_chain_dispatch.h](runtime/presentation/include/rescaleframe/d3d11_chain_dispatch.h)
- [x] [runtime/presentation/include/rescaleframe/d3d11_present_bridge.h](runtime/presentation/include/rescaleframe/d3d11_present_bridge.h)
- [x] [runtime/presentation/include/rescaleframe/shared_surface.h](runtime/presentation/include/rescaleframe/shared_surface.h)
- [ ] [runtime/presentation/src/d3d11_present_bridge.cpp](runtime/presentation/src/d3d11_present_bridge.cpp)
- [x] [runtime/presentation/src/d3d11_present_platform.h](runtime/presentation/src/d3d11_present_platform.h)
- [x] [runtime/presentation/src/shared_surface.cpp](runtime/presentation/src/shared_surface.cpp)

**Fixtures (4)**

- [x] [tests/d3d11_present_api.cpp](tests/d3d11_present_api.cpp)
- [ ] [tests/d3d11_present_bridge.cpp](tests/d3d11_present_bridge.cpp)
- [x] [tests/fg_present_bridge.cpp](tests/fg_present_bridge.cpp)
- [x] [tests/shared_surface.cpp](tests/shared_surface.cpp)

**Rust review findings (4)**

- [x] **Duplication | Medium | Reject nonfinite view-space scales consistently at presentation setup.** runtime/orchestrator/src/d3d11_host.rs:357 and sr_legacy.rs:118 require a finite positive units_to_meters value. The presentation entry at d3d11_present.rs:210 reimplements the rule as a NaN check and a lower bound, which admits positive infinity. generation_setup then forwards infinity to the provider at :771. The refusal vectors at d3d11_present_tests.rs:1403 omit infinity. Share a finite-positive unit-scale predicate for these ingress points and exercise positive infinity in the public presentation setup fixture. [runtime/orchestrator/src/d3d11_present.rs:210](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/d3d11_present.rs#L210)

- [x] **Altitude | Medium | Preserve pending physical-chain creation across a host-load refusal.** ensure_physical consumes physical_pending before attempting host_generation_load. If that operation refuses, the method returns E_FAIL without publishing a chain, selecting fallback or restoring the pending flag. Subsequent Present and buffer calls immediately report WAS_STILL_DRAWING even after the host is ready. Keep a distinct creating state and return to pending on retryable refusal, or publish a terminal fault/fallback that callers can act on. [runtime/orchestrator/src/d3d11_present.rs:960](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/d3d11_present.rs#L960)

- [x] **Duplication | Medium | Read one provider status snapshot when publishing a presentation owner.** Owner::initialize at runtime/orchestrator/src/d3d11_present.rs:1322 queries status through paced_by_host, then :1332 queries it again for publication and :1334 queries a third time when logging is enabled. Lazy creation at :992 and switching at :1493 repeat the pacing/publication pair. Each reaches the same provider.status callback declared at :717 and substituted by d3d11_present_tests.rs:260. If the cached provider status changes between reads, published pacing ownership and reported status can describe different observations. Take one snapshot per publication transition and derive HOST_PACED, the public snapshot and optional log from it, preserving later per-frame refreshes. [runtime/orchestrator/src/d3d11_present.rs:1322](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/d3d11_present.rs#L1322)

- [x] **Altitude | Medium | Keep a retryable physical Present result distinct from device loss.** The facade forwards Present flags to the physical chain, then permanently sets faulted for every failing HRESULT on both native D3D12 and D3D11 paths. A nonblocking Present can return DXGI_ERROR_WAS_STILL_DRAWING while the device is healthy; the next application Present then receives DEVICE_REMOVED without retrying. Track whether submitted work is safely fenced separately from whether DXGI displayed the frame, and preserve retryable Present outcomes when the fence is known. [runtime/orchestrator/src/d3d11_present.rs:1828](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/d3d11_present.rs#L1828)

<a id="d03"></a>

### D03 · SR sessions and bridging

Port generic SR setup, legacy bridging and auxiliary reconstruction. Trace the public SR API into each backend.

**Coordinate shared edits**

- Shared device, chain and SR handoff ([D01](#d01), [D02](#d02)).
- Provider setup, feature guards and teardown across the Runtime and backends ([D04](#d04), [D05](#d05), [D06](#d06), [D07](#d07)).

**Port files (15)**

- [x] [runtime/backends/common/sr_helpers.h](runtime/backends/common/sr_helpers.h)
- [x] [runtime/orchestrator/include/rescaleframe/d3d11_sr_api.h](runtime/orchestrator/include/rescaleframe/d3d11_sr_api.h)
- [x] [runtime/orchestrator/include/rescaleframe/native_sr.h](runtime/orchestrator/include/rescaleframe/native_sr.h)
- [x] [runtime/orchestrator/include/rescaleframe/sr_bridge.h](runtime/orchestrator/include/rescaleframe/sr_bridge.h)
- [x] [runtime/orchestrator/include/rescaleframe/sr_session.h](runtime/orchestrator/include/rescaleframe/sr_session.h)
- [x] [runtime/orchestrator/src/auxiliary_sr.cpp](runtime/orchestrator/src/auxiliary_sr.cpp)
- [x] [runtime/orchestrator/src/auxiliary_sr.h](runtime/orchestrator/src/auxiliary_sr.h)
- [x] [runtime/orchestrator/src/auxiliary_sr_platform.h](runtime/orchestrator/src/auxiliary_sr_platform.h)
- [x] [runtime/orchestrator/src/d3d11_sr_api.cpp](runtime/orchestrator/src/d3d11_sr_api.cpp)
- [ ] [runtime/orchestrator/src/native_sr.cpp](runtime/orchestrator/src/native_sr.cpp)
- [x] [runtime/orchestrator/src/native_sr_completion.h](runtime/orchestrator/src/native_sr_completion.h)
- [x] [runtime/orchestrator/src/sr_bridge_platform.cpp](runtime/orchestrator/src/sr_bridge_platform.cpp)
- [x] [runtime/orchestrator/src/sr_legacy_adapter.cpp](runtime/orchestrator/src/sr_legacy_adapter.cpp)
- [x] [runtime/orchestrator/src/sr_legacy_adapter.h](runtime/orchestrator/src/sr_legacy_adapter.h)
- [x] [runtime/orchestrator/src/sr_session_backends.cpp](runtime/orchestrator/src/sr_session_backends.cpp)

**Fixtures (4)**

- [x] [tests/auxiliary_sr.cpp](tests/auxiliary_sr.cpp)
- [x] [tests/sr_backend_switch_device.cpp](tests/sr_backend_switch_device.cpp)
- [x] [tests/sr_bridge.cpp](tests/sr_bridge.cpp)
- [x] [tests/sr_session.cpp](tests/sr_session.cpp)

**Rust review findings (11)**

- [x] **Duplication | Medium | Reuse the saved D3D11 state guard for auxiliary passes.** runtime/orchestrator/src/auxiliary_sr.rs:213 stores the context, opaque saved state and restore-on-drop flag for auxiliary render, commit and rollback. runtime/orchestrator/src/d3d11_output.rs:66 already owns that save/restore pair for colour and reinsertion calls, for example colour_d3d11.rs:113. The auxiliary route adds clear/copy operations and restores early before releasing frame references at auxiliary_sr.rs:855. Its generated state type is a separate Rust declaration of the same 8192-byte C record. Import one state binding, construct the shared guard only after save succeeds, and drop it explicitly at the existing early-restore point. Keep auxiliary clear/copy operations on the caller. [runtime/orchestrator/src/auxiliary_sr.rs:213](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/auxiliary_sr.rs#L213)

- [x] **Duplication | Medium | Reuse one fake IUnknown header for orchestrator ownership tests.** runtime/orchestrator/src/com_ref.rs:64 already has a test vtable and reference-counted Fake. auxiliary_sr.rs:1010, callback_host/tests.rs:14, sr_bridge/tests.rs:12 and sr12/tests.rs:11 separately implement the same vtable prefix and AddRef/Release counter for production COM calls. The fixtures differ usefully in their payloads (width, device, resource descriptor or release-event log), but only some explicitly guard over-release or provide a callable E_NOINTERFACE stub. Move the common repr(C) header, refcount and default QueryInterface into com_ref::test, embed it first in the specialized fixtures, and keep their descriptive payloads and optional last-release instrumentation. [runtime/orchestrator/src/auxiliary_sr.rs:1010](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/auxiliary_sr.rs#L1010)

- [x] **Duplication | Medium | Decode region-preparation failures once for SR and explicit FG.** The native prepare_regions leaf at runtime/orchestrator/src/native_sr.cpp:62 returns 1 after publishing all three textures, 0 on refusal, and leaf_fault on a caught exception. The SR path at runtime/orchestrator/src/native_sr.rs:176 explicitly stops on the exception code; capture_on_stream repeats the call but checks only for zero, so that same exception reaches fg.decode with the still-null vectors output. Wrap the callback in one result decoder returning the three textures only on success. Each caller can then map refusal and fault into its own status and history policy. [runtime/orchestrator/src/native_sr.rs:337](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/native_sr.rs#L337)

- [x] **Altitude | Medium | Invalidate the FG history when its own input validation faults.** capture_validated handles a device or depth-layout leaf fault by calling discard_history(NOT_READY). That helper always resets the SR history, even though this path prepares only FG inputs and native_sr_state.rs maintains independent histories. A capture fault can therefore reset a healthy SR frame and leave the old FG history eligible if the source retries. Give the failure handler the affected history owner and cover both slots in the test. [runtime/orchestrator/src/native_sr.rs:447](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/native_sr.rs#L447)

- [x] **Altitude | Medium | Expose the SR bridge's retained-on-timeout destruction result.** The public destroy function returns void both when it consumes the bridge and when wait or warm-up retirement times out and the entire SDK/device/texture owner remains live. The caller cannot know whether retrying will release outstanding work or touch an already freed handle, so the implementation abandons recoverable failures until process exit. Provide a checked close that consumes the handle only on successful retirement and returns a retryable status otherwise. [runtime/orchestrator/src/sr_bridge.rs:1003](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/sr_bridge.rs#L1003)

- [x] **Altitude | Medium | Preserve the supplied far-plane convention independently of depth direction.** The legacy adapter copies the caller's rsf_sr_open_desc and then overwrites depth_infinite with inverted_depth. Its evaluate path forwards the frame's actual far_plane independently. A reversed-Z projection with a finite far plane, or a standard-Z infinite projection, is therefore opened under a mismatching vendor convention. Carry both producer facts through create, using the existing open.depth_infinite field or an explicit legacy argument. [runtime/orchestrator/src/sr_legacy.rs:132](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/sr_legacy.rs#L132)

- [x] **Duplication | Low | Share the destructor-backed owner for opaque graphics handles.** The D3D11-to-D3D12 SR bridge's Owned and runtime/orchestrator/src/gl_pipeline.rs:158 have identical NonNull storage, destructor callback, null-to-Option constructor, accessor and Drop. Native FG normalization uses the same representation for MotionPass at runtime/orchestrator/src/native_sr.rs:258. The callers wrap shared fences and surfaces, GL/SR sessions and textures, or decode/resolve passes respectively, each with its own destructor. Move that exact non-COM owner to a common module and keep each higher-level owner's drop order and GPU completion rules at the caller. [runtime/orchestrator/src/sr_bridge.rs:112](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/sr_bridge.rs#L112)

- [x] **Simplicity | Low | sr_bridge keeps one prepared shape as three parallel Options and re-checks them after prepare** Bridge holds surfaces: Option<Surfaces>, output: Option<ComRef> and shape: Option<Shape> (sr_bridge.rs:200-203). They are only ever cleared together (release_surfaces, :422-426) and set together at the end of a successful prepare (:480-482). evaluate() still has to re-destructure them after prepare returned OK, with an unreachable (Some(surfaces), Some(output)) else INIT_FAILED branch (:624-626). Simpler: one prepared: Option<Prepared { shape: Shape, surfaces: Surfaces, output: ComRef }>; prepare compares prepared.as_ref().map(|p| p.shape) with the new shape and returns Result<&Prepared, i32>, so evaluate gets the surfaces and output directly with no second Option check, and release_surfaces becomes prepared = None. Tradeoff: none in behavior; drop order is unchanged because the three fields sit next to each other between session and fence today, so the combined field takes their place. [runtime/orchestrator/src/sr_bridge.rs:200](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/sr_bridge.rs#L200)

- [x] **Simplicity | Low | Key prepared bridge textures by the dimensions used to allocate them.** evaluate collects five texture descriptions so Shape can copy the color and motion formats, although input_accepted already requires COLOR_FORMAT and MOTION_FORMAT. prepare also allocates output-sized textures but the key contains only render dimensions and those two fixed formats. Validate descriptions in place, use the constants when allocating, and key the set by render and output dimensions. Steady-size frames keep reusing their allocations, while a new output size with unchanged render dimensions requires one fence wait and reallocation. [runtime/orchestrator/src/sr_bridge.rs:589](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/sr_bridge.rs#L589)

- [x] **Simplicity | Low | Orchestrator re-declares the struct_size prefix check in eleven modules beside the crate-visible has_capacity** sr_session.rs:54 exports pub(crate) has_capacity<T> (non-null and leading u32 >= size_of::<T>()), and dlss_streamline.rs:11 already imports it. The same body is re-declared privately at dlss_pipeline.rs:437, fg12.rs:90 (full_prefix, plus checked<T> at :94 returning Option<&T>), fg_dispatch.rs:20 (full_prefix), fg_session.rs:106, fg_scene.rs:89 (full_pass, Pass only), frame_assembly.rs:5, native_cpu.rs:132, native_fg_cpu.rs:1438 (compares as u32), native_scene.rs:25 (full_prefix) sr12.rs:1281 (full_pass returning Option<&Pass>) and input_dump.rs:747 (pub(super) checked<T> returning Option<&T>). Simpler: keep one crate helper pair, has_capacity<T> and a checked<T> -> Option<&T> built on it, and import both, so there is one definition of the prefix contract instead of eleven. Tradeoff: an extra crate import per module. native_fg_cpu compares in u32 rather than usize, which gives the same answer for any real struct size. Inference: other modules read struct_size inline (grep shows 67 cast::<u32>().read() sites in 41 files). Only the eleven named helper copies were checked to be identical. [runtime/orchestrator/src/sr_session.rs:54](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/sr_session.rs#L54)

- [x] **Duplication | Low | Announce the selected FSR provider once per successful SR switch.** The D3D12 FSR owner logs its version name at runtime/orchestrator/src/d3d12_sr/fsr.rs:84 during provider open. Session::replace forwards the same logger to open, obtains that version from the provider, and writes its name again after planning and committing. The sr12 session selection at runtime/orchestrator/src/sr12.rs:494 therefore emits the same line twice on a successful switch, and the first copy can appear even when planning subsequently refuses it. Emit the active-version announcement at commit, preserving the provider's diagnostics for unavailable candidates and any direct-open reporting contract. [runtime/orchestrator/src/sr_session.rs:252](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/sr_session.rs#L252)

<a id="d04"></a>

### D04 · D3D12 SR and generation hosts

Port shared D3D12 SR, FG, provider dispatch and retirements. Coordinate provider-table changes with the vendor packages.

**Coordinate shared edits**

- Provider setup, feature guards and teardown across the Runtime and backends ([D03](#d03), [D05](#d05), [D06](#d06), [D07](#d07)).

**Port files (10)**

- [x] [runtime/backends/common/d3d12_helpers.h](runtime/backends/common/d3d12_helpers.h) (C declarations only)
- [x] [runtime/backends/common/fg_helpers.h](runtime/backends/common/fg_helpers.h) (C declarations only)
- [x] [runtime/backends/common/generation12_checks.h](runtime/backends/common/generation12_checks.h)
- [x] [runtime/backends/common/generation12_platform.h](runtime/backends/common/generation12_platform.h)
- [x] [runtime/backends/common/sr12_platform.h](runtime/backends/common/sr12_platform.h)
- [x] [runtime/orchestrator/include/rescaleframe/native_fg_d3d12.h](runtime/orchestrator/include/rescaleframe/native_fg_d3d12.h)
- [x] [runtime/orchestrator/include/rescaleframe/native_sr_d3d12.h](runtime/orchestrator/include/rescaleframe/native_sr_d3d12.h)
- [x] [runtime/orchestrator/src/fg12_platform.cpp](runtime/orchestrator/src/fg12_platform.cpp) (install dispatch in fg12/native.rs; capture lifetime in the C11 leaf fg12_platform.c)
- [x] [runtime/orchestrator/src/fg12_platform.h](runtime/orchestrator/src/fg12_platform.h)
- [x] [runtime/orchestrator/src/sr12_platform.cpp](runtime/orchestrator/src/sr12_platform.cpp)

**Fixtures (2)**

- [x] [tests/fg12_abi.cpp](tests/fg12_abi.cpp)
- [x] [tests/sr12_abi.cpp](tests/sr12_abi.cpp)

**Rust review findings (16)**

- [x] **Altitude | Medium | Preserve wait failure, timeout and device status as distinct D3D12 results.** The shared wait helper turns every WaitForSingleObject result except WAIT_OBJECT_0 into false, then returns DXGI_ERROR_DEVICE_HUNG for both a timeout and WAIT_FAILED. It also replaces the fence-arm HRESULT with E_FAIL. Callers through runtime/backends/common/d3d12_helpers.h:42 see a device diagnosis for an expired wait or bad event. Preserve the arm result, distinguish timeout and Win32 failure, and report device loss/hang only when the device supplies that evidence. [runtime/orchestrator/src/d3d12_fence.rs:60](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/d3d12_fence.rs#L60)

- [x] **Altitude | Medium | Expose unsuccessful provider teardown to the presentation owner.** The shared destroy callback returns void even when Session::retire declines and deliberately keeps the allocation alive. FSR can refuse while disabling or waiting for callbacks, and XeSS can refuse FG or XeLL destruction. d3d11_present.rs:1459 discards the outgoing handle and starts creating a replacement regardless, losing the retry handle and potentially overlapping HWND or SDK ownership. Add a checked teardown/retirement result and retain the outgoing session until it acknowledges completion. [runtime/orchestrator/src/d3d12_generation.rs:418](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/d3d12_generation.rs#L418)

- [x] **Altitude | Medium | Deduplicate XeLL input markers only after the SDK accepts them.** Xess::marker sets input_sampled before the SDK marker callback returns. If that call fails, the first attempt returns FEATURE_FAILED but every retry for the same frame returns OK without contacting XeLL. This makes the shared presentation marker route report delivery that never happened. Keep the attempted call serialized and set input_sampled only on SDK success. [runtime/orchestrator/src/d3d12_generation/xess.rs:351](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/d3d12_generation/xess.rs#L351)

- [x] **Duplication | Medium | Apply the same FG12 option validation at install and live update.** Installation validates mode and generated count but stores the request in OWNER without checking its reflex_mode. Runtime/orchestrator/src/fg12.rs:1266 (live update via runtime/orchestrator/src/fg12_platform.cpp:303) rejects reflex_mode above BOOST, and runtime/orchestrator/src/native_fg.rs:673 independently rejects it when install forwards the setup. Thus the local D3D12 presenter and shared settings can start with different options. Use one FG12-specific validator for the two entry points before publishing or forwarding settings, preserving any broader mode support in the shared owner. [runtime/orchestrator/src/fg12.rs:320](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/fg12.rs#L320)

- [x] **Altitude | Medium | Give failed FG12 installation a rollback boundary for shared settings and callbacks.** Before native install reports success, the installer publishes the requested graphics API, FG options and logger, including its borrowed user pointer, to shared owners. On INIT_FAILED it only resets INSTALL. native_fg.rs:686 retains that sink, and subsequent native FG diagnostics can invoke it after the failed installer released its user context. Stage callback registration with the installer and publish on success, or restore the previous registrations and shared options on every failed return. [runtime/orchestrator/src/fg12.rs:384](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/fg12.rs#L384)

- [x] **Duplication | Medium | Use the pacing owner's eligibility check before aborting older FG tokens.** D3D12 Present preparation walks its own CPU ring and calls host_abort on every older same-session row without a completed Present, then marks it complete regardless of the result. The shared pacing owner at runtime/orchestrator/src/native_fg_cpu.rs:305 and :1386 independently retires stale tokens, requiring ended, unsubmitted work with no render start or in-flight pacing, and its capacity path checks whether the host accepted the abort. An older recorded or pacing source can pass the FG12 scan but fail those checks, and a refused abort is then hidden by the completion bit. Route the sweep through the shared source ledger, retaining eligibility and abort result per token. [runtime/orchestrator/src/fg12.rs:882](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/fg12.rs#L882)

- [x] **Altitude | Medium | Associate failed FG capability queries with the current Present.** D3D12 prepare returns immediately when provider.status fails, before recording last_result, reason or source_frame_id and before configuring the provider off. In the same backend generation, status_snapshot can still return the preceding source's successful preparation and active vendor status through rsf_fg12_status, which the callback render host reads. Make prepare publish a current-frame failure and deactivate the cached state on the early exit, and carry that result through retirement. [runtime/orchestrator/src/fg12.rs:912](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/fg12.rs#L912)

- [x] **Duplication | Medium | Derive Reflex frame-limit intervals consistently across the native FG paths.** The D3D12 prepare callback and runtime/orchestrator/src/native_fg.rs:1410 independently construct rsf_fg_options from the same native options, capability and frame eligibility. Both cap the generated count and disable generation when ineligible. The latter divides the requested rendered-frame interval by generated+1 when Reflex paces eligible fixed FG, while fg12.rs:942 passes the full interval to that same pacing owner. At two generated frames the two configure callbacks receive a threefold difference. Share the provider-option calculation with explicit eligibility, keeping resource capture and provider submission in their respective owners. [runtime/orchestrator/src/fg12.rs:924](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/fg12.rs#L924)

- [x] **Duplication | Medium | Count interpolated preparation consistently in native FG status.** D3D12 increments tagged_frames on every successful provider prepare, including eligible frames submitted with interpolate=0 when FG is off. The D3D11 bridge at runtime/orchestrator/src/native_fg.rs:1645 increments the same public field only when prepare succeeds and interpolation was requested. The counter reaches Unity package status at games/unity-mono/src/package_state.rs:512 and the Anomaly runtime-tagged display at games/anomaly/src/fg_stage.rs:610. Give both producers the same successful-interpolation predicate, or publish distinct counters if disabled preparation is independently useful. [runtime/orchestrator/src/fg12.rs:1030](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/fg12.rs#L1030)

- [x] **Altitude | Medium | Keep inferred native-frame provenance attached to every SR12 capture operation.** For a pass with source_frame_id zero, evaluate marks only a temporary identity ambiguous for dump_begin_record. Its subsequent readbacks, SR parameters and result use the unflagged record. input_dump::Capture::wants explicitly requires the ambiguity flag to match, so the capture selects the frame and rejects its own inputs and results. Construct the record with the correct provenance once and use it consistently, including at FG handoff where a native counter alone cannot identify an input frame. [runtime/orchestrator/src/sr12.rs:1011](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/sr12.rs#L1011)

- [x] **Boundaries | Medium | Bring the D3D12 SR test platform table back in line with the owner.** sr12.rs:118-120 declares the platform fields create_exposure_texture, create_exposure_upload and write_exposure_upload (matching sr12_platform.cpp:324-326/368-369), but the synthetic table in sr12/tests.rs:441-494 still initialises a field named create_unit_exposure (fn at :248, field at :460) and omits the three current fields. Because the module is compiled under #[cfg(test)] (sr12.rs:24-25) with Platform from super::*, the orchestrator crate test build cannot compile, so none of its owner tests (including the boundary contracts for SR/FG/overlay) run. Observed while checking the Rust-to-C++ platform table boundary; replace the stale fake with the three current callbacks. [runtime/orchestrator/src/sr12/tests.rs:460](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/sr12/tests.rs#L460)

- [x] **Duplication | Low | Use one coexistence check when opening D3D12 generation providers.** The FSR constructor checks for a loaded Streamline DLL without a host before validating its supported UI modes. The XeFG constructor in runtime/orchestrator/src/d3d12_generation/xess.rs:509 checks UI_LAYER first, then repeats the same restart rule at line 512. With UI_LAYER and an unowned loaded Streamline module, the two native create callers (runtime/backends/fsr/src/fsr_generation.cpp:302 and runtime/backends/xess/src/xess_generation.cpp:263) therefore receive different errors for the same two failed prerequisites. Put the coexistence decision in a shared preflight with explicit precedence, leaving each provider to declare its supported options. [runtime/orchestrator/src/d3d12_generation/fsr.rs:779](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/d3d12_generation/fsr.rs#L779)

- [x] **Duplication | Low | Share ownership of resolved D3D12 provider bindings.** The SR loader closes a partially produced handle on bind failure, rejects success with a null handle, then unbinds before dropping the DLL. The FG loader independently implements the same rollback and destruction contract at runtime/orchestrator/src/d3d12_generation.rs:75 and :139. Both serve live FSR and XeSS constructors, including runtime/orchestrator/src/d3d12_sr/fsr.rs:47 and runtime/orchestrator/src/d3d12_generation/xess.rs:515. Extract the raw binding owner and result-to-handle conversion into sdk_module, retaining the SR compatibility lease and the FG optional latency DLL in their existing release order. [runtime/orchestrator/src/d3d12_sr.rs:35](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/d3d12_sr.rs#L35)

- [x] **Duplication | Low | Share the bounded FidelityFX provider enumeration used for SR and FG.** SR open and the FG probe/create path at runtime/orchestrator/src/d3d12_generation/fsr.rs:741 each allocate 32 version records, check the returned count, invoke ffx_version::select, and reject a zero winner. Their native leaves also duplicate the SDK query, parallel ID/name buffers, count check and record conversion at runtime/backends/fsr/src/fsr_backend.cpp:122 and runtime/backends/fsr/src/fsr_generation.cpp:87. Only the device source, queried context kind and failure diagnostics differ. Parameterize the native enumeration by device and context kind, and wrap the bounded candidates and admitted winner in ffx_version so SR can still log its list and FG can probe silently. [runtime/orchestrator/src/d3d12_sr/fsr.rs:90](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/d3d12_sr/fsr.rs#L90)

- [x] **Altitude | Low | Retire failed diagnostic readbacks when their device generation is drained.** Colors::poll retires a completed fence or a missing fence after a generation switch. A fence reporting DeviceRemoved falls through forever, even after the presentation owner drains the old generation. Colors::wants then refuses every subsequent final/HUD-less capture because pending stays occupied. Preserve the distinction between pending and failed completion, discard the unusable pair on verified teardown, and allow the next generation to capture. [runtime/orchestrator/src/fg12/colors.rs:136](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/fg12/colors.rs#L136)

- [x] **Simplicity | Low | Reuse the validated external exposure view during normalization.** On each D3D12 evaluation with external exposure, inputs_valid describes the texture at line 718 and checks its SRV format. normalize immediately describes the retained texture a second time solely to obtain that same format. Store the validated format beside the retained COM reference and pass the pair to write_srv, using R32_FLOAT for the absent input as today. This trades one stored format for one fewer native description per evaluation and preserves the existing reference lifetime. [runtime/orchestrator/src/sr12.rs:894](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/sr12.rs#L894)

<a id="d05"></a>

### D05 · D3D11 and D3D12 DLSS

Port Streamline startup, DLSS frame planning, evaluate and shutdown across shared and owned registrations.

**Coordinate shared edits**

- Provider setup, feature guards and teardown across the Runtime and backends ([D03](#d03), [D04](#d04), [D06](#d06), [D07](#d07)).

**Port files (11)**

- [x] [runtime/backends/dlss/include/rescaleframe/dlss.h](runtime/backends/dlss/include/rescaleframe/dlss.h)
- [x] [runtime/backends/dlss/include/rescaleframe/dlss_native12.h](runtime/backends/dlss/include/rescaleframe/dlss_native12.h)
- [x] [runtime/backends/dlss/include/rescaleframe/dlss_preset.h](runtime/backends/dlss/include/rescaleframe/dlss_preset.h)
- [ ] [runtime/backends/dlss/src/dlss_native12.cpp](runtime/backends/dlss/src/dlss_native12.cpp)
- [ ] [runtime/backends/dlss/src/dlss_streamline.cpp](runtime/backends/dlss/src/dlss_streamline.cpp)
- [ ] [runtime/backends/dlss/src/streamline_frame.h](runtime/backends/dlss/src/streamline_frame.h)
- [ ] [runtime/backends/dlss/src/streamline_load.h](runtime/backends/dlss/src/streamline_load.h)
- [ ] [runtime/backends/dlss/src/streamline_sr.h](runtime/backends/dlss/src/streamline_sr.h)
- [x] [runtime/orchestrator/include/rescaleframe/dlss_pipeline.h](runtime/orchestrator/include/rescaleframe/dlss_pipeline.h)
- [x] [runtime/orchestrator/src/dlss_pipeline.cpp](runtime/orchestrator/src/dlss_pipeline.cpp)
- [x] [runtime/orchestrator/src/dlss_pipeline_platform.h](runtime/orchestrator/src/dlss_pipeline_platform.h)

**Fixtures (2)**

- [x] [tests/dlss_backend.cpp](tests/dlss_backend.cpp)
- [x] [tests/sr_pipeline.cpp](tests/sr_pipeline.cpp)

**Rust review findings (12)**

- [x] **Altitude | Medium | Give DLSS viewport resources the same ownership as the contexts that use them.** sr12.rs:443 can create multiple SR contexts from one device registration. Each context has its own quality and used_viewport flag, but Route::viewport derives the same viewport solely from registration type. Their options/tags can overwrite one another, and dropping either evaluated context calls free_viewport on the surviving context's ID. Allocate distinct viewport IDs per independent view or enforce a single live context per registered viewport and release it through that owner. [runtime/orchestrator/src/dlss_native12.rs:160](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/dlss_native12.rs#L160)

- [x] **Duplication | Medium | Use one mapping for adapter-supplied Streamline engine metadata.** The owned D3D12 registration passes Unity through and maps all other rsf_dlss_setup engine values to custom. The D3D11 startup at runtime/orchestrator/src/dlss_streamline.rs:217 passes both Unreal and Unity through. Both native initialization leaves cast the result to the same sl::EngineType (runtime/backends/dlss/src/dlss_native12.cpp:113 and runtime/backends/dlss/src/dlss_streamline.cpp:268). An adapter declaring RSF_DLSS_ENGINE_UNREAL thus reports different engine identity solely by taking the other API. Share the supported enum mapping and keep render API, directory and logging preferences local. [runtime/orchestrator/src/dlss_native12.rs:302](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/dlss_native12.rs#L302)

- [x] **Altitude | Medium | Start the requested D3D11 SR provider without requiring DLSS to initialize first.** The shared host chooses an SR backend from the owning adapter, but d3d11_host.rs:892 starts the pipeline and calls pipeline_select only after startup succeeds. Pipeline::start unconditionally loads Streamline, requires DLSS support and asks DLSS for a plan. An Intel or AMD device lacking DLSS cannot reach its selected XeSS or FSR backend. Give startup the selected backend and initialize only its dependencies, while sharing device/output ownership and provider-switch retirement. [runtime/orchestrator/src/dlss_pipeline.rs:791](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/dlss_pipeline.rs#L791)

- [x] **Duplication | Medium | Build dense-motion resolve parameters from one conversion.** The DLSS scene pipeline converts canonical UV motion into previous-minus-current render pixels and fills the D3D11 resolve record. The native FG preparation at runtime/orchestrator/src/native_sr.rs:344 and the alternate SR backend at runtime/orchestrator/src/sr_legacy.rs:238 construct the same record and duplicate both signed axis products. Native FG and DLSS also separately derive sentinel availability from the two decoder flags. The legacy path explicitly uses zero jitter, the scene path can supply a depth layer and handle NDC input, and native FG takes camera jitter. Share the canonical conversion and parameter constructor, with the encoding, jitter and depth layer explicit in the caller, so a sign or sentinel fix reaches all three resolve calls. [runtime/orchestrator/src/dlss_pipeline.rs:1069](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/dlss_pipeline.rs#L1069)

- [x] **Altitude | Medium | Distinguish original and submitted resources in the requested SR dump.** write_dump labels frame.scene_color and frame.depth as the backend's inputs. When colour transport runs, evaluate_scene passes dlss.color_in = encoded and no exposure; when dense normalization runs, it substitutes dlss.depth. A requested capture can therefore pair the backend output with different color/depth inputs from the same frame, obscuring reconstruction defects. Capture the descriptor handed to the selected provider at dispatch, and label engine originals separately. [runtime/orchestrator/src/dlss_pipeline.rs:1514](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/dlss_pipeline.rs#L1514)

- [x] **Duplication | Medium | Commit successful SR quality changes through the same history reset.** The public quality setter at runtime/orchestrator/src/dlss_pipeline.cpp:460 reaches two copies of the post-plan state update. The DLSS branch at runtime/orchestrator/src/dlss_pipeline.rs:1923 calls restart_history, while the legacy FSR/XeSS branch at :1898 only sets reset_pending. After 120 vendor evaluation failures the common gate at :877 refuses every subsequent frame, so changing quality succeeds but cannot restart an alternate backend. Share the quality/range commit and call restart_history on either successful plan, preserving the provider-specific planning operations. [runtime/orchestrator/src/dlss_pipeline.rs:1898](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/dlss_pipeline.rs#L1898)

- [x] **Quality | Medium | Switching back to DLSS drops the zero-range fallback and can refuse every later frame.** select_backend(SR_DLSS) stores Range::reported(plan) (line 2069), which keeps min/max exactly as the SDK reported them, while start, set_quality and resize_output use Range::of, whose comment says an SDK build that reports no range must accept the optimal size rather than be read as any size. The plan leaf copies slDLSSGetOptimalSettings renderWidthMin/Max straight through (dlss_streamline/planning.rs:148-151). When those are zero, evaluate_scene (line 912) rejects every frame because rw > range.max[0] == 0, so after a FSR/XeSS -> DLSS switch the pipeline returns INVALID for all frames until quality, resize or restart rebuilds the range with Range::of. The backend-switch test (dlss_pipeline_tests.rs:1141) uses a fixture plan with non-zero min/max, so the path is untested. [runtime/orchestrator/src/dlss_pipeline.rs:2069](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/dlss_pipeline.rs#L2069)

- [x] **Altitude | Medium | Resolve a DLSS viewport once for constants, options, tags and evaluation.** The public frame contract describes scene viewport 0 and layer viewport 1. evaluate maps any other value to slot 0 for options, tags and evaluation, but the shared-host constants callback receives the unmodified frame.viewport. The frame fixture asserts viewport 7 reaches the host while the slot is 0. Reject an unsupported viewport before touching the SDK, or pass one consistently validated viewport identity through all four operations. [runtime/orchestrator/src/dlss_streamline/frame.rs:64](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/dlss_streamline/frame.rs#L64)

- [x] **Duplication | Low | Share the projection-to-forward conversion with an explicit orthographic convention.** DLSS SR evaluation signs view_to_world[8..11] from view_to_clip[11]. FSR FG prepare does the same calculation at runtime/orchestrator/src/d3d12_generation/fsr.rs:619 and :247. The perspective branches agree, while a zero coefficient selects negative forward in DLSS SR and positive forward in FSR FG. The native DLSS FG caller at runtime/backends/dlss/src/dlss_generation.cpp:736 already expresses this distinction as an orthographic sign argument to runtime/backends/dlss/src/streamline_frame.h:12. Give the Rust evaluations the same shared pure conversion and explicit fallback, with cases for both perspective signs and zero. [runtime/orchestrator/src/dlss_native12.rs:586](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/dlss_native12.rs#L586)

- [x] **Duplication | Low | Exercise the production DLSS motion-sign rule in the frame-assembly fixture.** The pipeline applies its Rust sign conversion to both axes at runtime/orchestrator/src/dlss_pipeline.rs:1092. The sign and reconstruction checks at tests/frame_assembly.cpp:105 instead invoke the separate constexpr implementation in runtime/orchestrator/src/motion_convention.h:7, whose only code caller is that test. Editing the native copy can make those reprojection checks pass without changing evaluation. Move the raw, dense-resolved and unspecified convention vectors to a Rust test of the production conversion, or run the native fixture through the actual public pipeline evaluation. [runtime/orchestrator/src/dlss_pipeline.rs:377](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/dlss_pipeline.rs#L377)

- [x] **Simplicity | Low | dlss_pipeline re-implements the %.9g formatter that frame_capture_vk already exports** dlss_pipeline.rs:386-421 g9 is a private printf %.9g formatter: special-cases NaN, inf and zero, formats {:.8e}, splits the exponent, trims trailing zeros and chooses the scientific form outside -4..9. frame_capture_vk.rs:571-607 already exports pub(crate) general(value) = general_precision(value, 9) with the same algorithm, and frame_capture12.rs:11 imports it. Two test suites pin the same cases (dlss_pipeline_tests.rs:1231-1247 g9_matches_printf and frame_capture_vk/tests.rs:562-577 general_matches_iostream_precision_nine; both cover 0.1f32, 1e-5 or 1e-7, 123456789 and 1234567890). The only behavioural difference is that g9 prints -nan for a negative NaN where general prints nan. Simpler: delete g9, call crate::frame_capture_vk::general (or move the formatter to a small shared module), and keep one test table. Tradeoff: the diagnostic frame record would print nan instead of -nan for a sign-negative NaN, unless general adopts the -nan branch, which matches printf. [runtime/orchestrator/src/dlss_pipeline.rs:386](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/dlss_pipeline.rs#L386)

- [x] **Duplication | Low | Pass the existing DLSS support and planning records through the pipeline leaves.** The pipeline maintains a private six-output range and eight-output support struct alongside the generated public types at runtime/orchestrator/src/dlss_streamline_abi.rs:7 and :42. The native leaves at runtime/orchestrator/src/dlss_pipeline.cpp:109 and :121 construct those public records, call the DLSS facade, and copy each output field back into the private layouts consumed by pipeline start and quality changes. Pass initialized public records across those leaves directly, keeping inputs and struct_size in the public plan and converting to the internal Range only where the pipeline applies its fallback rules. [runtime/orchestrator/src/dlss_pipeline_abi.rs:32](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/dlss_pipeline_abi.rs#L32)

<a id="d06"></a>

### D06 · D3D12 Streamline generation and compatibility

Port generation, latency instrumentation and bundled compatibility handling with explicit module and callback ownership.

**Coordinate shared edits**

- Provider setup, feature guards and teardown across the Runtime and backends ([D03](#d03), [D04](#d04), [D05](#d05), [D07](#d07)).

**Port files (14)**

- [x] [runtime/backends/dlss/include/rescaleframe/streamline_host.h](runtime/backends/dlss/include/rescaleframe/streamline_host.h)
- [ ] [runtime/backends/dlss/src/dlss_generation.cpp](runtime/backends/dlss/src/dlss_generation.cpp) Still first-party C++. D04 removed its use of the fg_helpers.h and d3d12_helpers.h wrappers only. The Streamline host leaf still uses `sl::` SDK records, feature templates, `sl_matrix_helpers` and `ComPtr` D3D12/DXGI ownership.
- [ ] [runtime/backends/dlss/src/mfg_unlock.cpp](runtime/backends/dlss/src/mfg_unlock.cpp)
- [ ] [runtime/backends/dlss/src/mfg_unlock.h](runtime/backends/dlss/src/mfg_unlock.h)
- [ ] [runtime/backends/dlss/src/reflex_audit.cpp](runtime/backends/dlss/src/reflex_audit.cpp)
- [ ] [runtime/backends/dlss/src/reflex_audit.h](runtime/backends/dlss/src/reflex_audit.h)
- [ ] [runtime/backends/dlss/src/streamline_diagnostics.h](runtime/backends/dlss/src/streamline_diagnostics.h)
- [x] [runtime/backends/dlss/third_party/mfg_unlocker/dlssg_provider_policy.cpp](runtime/backends/dlss/third_party/mfg_unlocker/dlssg_provider_policy.cpp)
- [x] [runtime/backends/dlss/third_party/mfg_unlocker/dlssg_provider_policy.h](runtime/backends/dlss/third_party/mfg_unlocker/dlssg_provider_policy.h)
- [ ] [runtime/backends/dlss/third_party/mfg_unlocker/midpoint_fix.cpp](runtime/backends/dlss/third_party/mfg_unlocker/midpoint_fix.cpp)
- [ ] [runtime/backends/dlss/third_party/mfg_unlocker/midpoint_fix.h](runtime/backends/dlss/third_party/mfg_unlocker/midpoint_fix.h)
- [ ] [runtime/backends/dlss/third_party/mfg_unlocker/ngx_mfg_gate.h](runtime/backends/dlss/third_party/mfg_unlocker/ngx_mfg_gate.h)
- [ ] [runtime/backends/dlss/third_party/mfg_unlocker/protected_pointer.h](runtime/backends/dlss/third_party/mfg_unlocker/protected_pointer.h)
- [x] [runtime/backends/dlss/third_party/mfg_unlocker/universal_wrapper_profile.h](runtime/backends/dlss/third_party/mfg_unlocker/universal_wrapper_profile.h)

**Fixtures (1)**

- [x] [tests/latency_host.cpp](tests/latency_host.cpp)

**Rust review findings (7)**

- [x] **Altitude | Medium | Settle the token when the combined Streamline begin operation fails to sleep.** rsf_core_slhost_begin first commits acquire, advancing last_begin and occupying a token slot, then returns the sleep result directly. If the SDK sleep fails, the token remains live with sleep_claimed set, so repeating begin for that ID cannot succeed and destroy refuses while it remains. A caller using the combined public operation has no indication that it must now abort a partially acquired frame. Abort the token on failed begin, or return an explicit retained-token outcome while preventing a second sleep. [runtime/orchestrator/src/streamline_host.rs:464](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/streamline_host.rs#L464)

- [x] **Duplication | Medium | Deduplicate the controller sample across both Streamline marker entry points.** Generation's provider marker at runtime/orchestrator/src/streamline_host/generation.rs:516 calls marker_for, which maps INPUT_SAMPLE with controller=1 to PCL marker 13 but never reads or sets Token::controller_sampled. The input-receipt route at runtime/orchestrator/src/streamline_host.rs:759 emits the identical marker and does set that bit. A controller marker followed by a controller receipt, or two controller marker calls for one token, can therefore notify the SDK twice. Route both through one locked emit-once helper, set the bit only on success, and keep the physical input counters specific to actual receipts. [runtime/orchestrator/src/streamline_host.rs:649](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/streamline_host.rs#L649)

- [x] **Duplication | Medium | Share revision and source ordering across D3D12 generation providers.** DLSS-G Copied::stage_prepare, complete_prepare and abort repeat the FSR/XeSS state transitions in runtime/orchestrator/src/d3d12_generation.rs:253, :264 and :276. Both reject stale revisions and non-increasing frame IDs, consume a successful late completion's ID even after abort, and suppress activity from invalidated prepares. DLSS also publishes an SDK retirement fence and dynamic capabilities; FSR/XeSS record different observation counters. Extract only the revision/last-prepare/history state machine and run the interleaving tests from both providers against it, leaving statistics and SDK retirement data local. [runtime/orchestrator/src/streamline_host/generation.rs:124](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/streamline_host/generation.rs#L124)

- [x] **Altitude | Medium | Keep Reflex and DLSS-G state coherent after a partial configuration failure.** Session configure applies the new Reflex mode and interval before calling the SDK generation-options operation. If generation configuration fails, the copied options/status still describe the previous request while the host already uses the new Reflex settings. Reapplying the old request hits the equality shortcut and skips restoring the driver. Roll back the accepted Reflex change or record the partial result so the next configure reconciles both SDK states. [runtime/orchestrator/src/streamline_host/generation.rs:472](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/streamline_host/generation.rs#L472)

- [x] **Simplicity | Low | Represent an empty Streamline token slot once.** acquire publishes a nonzero source ID only after the SDK returns a nonnull handle. Retirement clears only the ID, leaving a handle that no caller may use. sleep, marker_for and generation.rs:603 nevertheless test for a missing handle after looking up a live ID. Use an array of Option<Token> with a required NonNull handle, publish Some on acquire, and take the slot on retirement. This moves the presence check into ring lookup while keeping the six-slot rotation, marker ordering and retry behavior. [runtime/orchestrator/src/streamline_host.rs:186](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/streamline_host.rs#L186)

- [x] **Simplicity | Low | Keep one flag for a successfully prepared DLSS-G history.** Snapshot.history_valid and prepared_enabled are private, initialized false, cleared together under the same mutex by configured, stage_prepare and abort, and both assigned enabled by complete_prepare. polled requires both to report activity, and the concurrency tests mirror the pair. Keep history_valid alone and use it for both reset and activity decisions. The revision still rejects stale publication, and last_prepare still records consumed sources. A separate flag would become useful only if a transition could change one without the other. [runtime/orchestrator/src/streamline_host/generation.rs:52](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/streamline_host/generation.rs#L52)

- [x] **Simplicity | Low | Borrow the boxed host for non-owning FFI tests.** The ring-retirement, PCL-profile and generation-reload tests convert a Box to a raw pointer and reconstruct the Box after calling functions that only borrow the host. The ring and reload cases repeat the transfer. Keep a mutable Box in each test and pass its as_mut() result at each FFI call. This removes unsafe ownership reconstruction and keeps cleanup automatic if an assertion fails, while exercising the same C entry points. [runtime/orchestrator/src/streamline_host/tests.rs:276](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/streamline_host/tests.rs#L276)

<a id="d07"></a>

### D07 · D3D12 FidelityFX and XeSS

Port the two providers and FSR4 compatibility controls, coordinating the shared D3D12 dispatch contract with D04.

**Coordinate shared edits**

- Provider setup, feature guards and teardown across the Runtime and backends ([D03](#d03), [D04](#d04), [D05](#d05), [D06](#d06)).

**Port files (12)**

- [x] [runtime/backends/fsr/include/rescaleframe/fsr_backend.h](runtime/backends/fsr/include/rescaleframe/fsr_backend.h)
- [x] [runtime/backends/fsr/src/ffx_version.h](runtime/backends/fsr/src/ffx_version.h)
- [x] [runtime/backends/fsr/src/fsr4_compat.cpp](runtime/backends/fsr/src/fsr4_compat.cpp)
- [x] [runtime/backends/fsr/src/fsr4_compat.h](runtime/backends/fsr/src/fsr4_compat.h)
- [x] [runtime/backends/fsr/src/fsr4_compat_core.cpp](runtime/backends/fsr/src/fsr4_compat_core.cpp) (deleted; the Vulkan FSR4 bridge calls the C lease API directly)
- [x] [runtime/backends/fsr/src/fsr4_compat_core.h](runtime/backends/fsr/src/fsr4_compat_core.h) (deleted with the .cpp; `fsr4_compat_platform.h` carries the C declarations)
- [x] [runtime/backends/fsr/src/fsr4_compat_platform.h](runtime/backends/fsr/src/fsr4_compat_platform.h)
- [x] [runtime/backends/fsr/src/fsr_backend.cpp](runtime/backends/fsr/src/fsr_backend.cpp)
- [x] [runtime/backends/fsr/src/fsr_generation.cpp](runtime/backends/fsr/src/fsr_generation.cpp)
- [x] [runtime/backends/xess/include/rescaleframe/xess_backend.h](runtime/backends/xess/include/rescaleframe/xess_backend.h)
- [x] [runtime/backends/xess/src/xess_backend.cpp](runtime/backends/xess/src/xess_backend.cpp)
- [x] [runtime/backends/xess/src/xess_generation.cpp](runtime/backends/xess/src/xess_generation.cpp)

**Fixtures (1)**

- [x] [tests/fsr_backend.cpp](tests/fsr_backend.cpp)

**Rust review findings (1)**

- [x] **Quality | Low | FSR4 INT8 compatibility refuses silently when MinHook cannot create the hook.** install() uses `create_hook(...)?` and returns None without setting diagnostic.refusal. The native platform create_hook in runtime/backends/fsr/src/fsr4_compat_core.cpp:61 returns nullptr when MH_Initialize or MH_CreateHook fails (for example a target already hooked by another tool), and rsf_fsr4_acquire_int8_adapter only logs when diagnostic.refusal is non-null. On NVIDIA/Intel the FSR4 INT8 path then silently uses the SDK native refusal with no log line, which AGENTS.md treats as indistinguishable from a patch that worked. Set a refusal string such as "resident hook creation failed" before returning, as the enable_hook failure branch already does. [runtime/backends/fsr/compat/src/lib.rs:163](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/backends/fsr/compat/src/lib.rs#L163)

<a id="d08"></a>

### D08 · OpenGL interop and rendering

Port OpenGL sharing, the rendering pipeline and its surface fixture alongside linked consumers.

**Coordinate shared edits**

- OpenGL shared surfaces, device ownership and linked Minecraft callers ([M01](#m01), [D02](#d02)).

**Port files (4)**

- [x] [runtime/orchestrator/include/rescaleframe/gl_pipeline.h](runtime/orchestrator/include/rescaleframe/gl_pipeline.h)
- [x] [runtime/orchestrator/src/gl_pipeline_platform.cpp](runtime/orchestrator/src/gl_pipeline_platform.cpp)
- [x] [runtime/presentation/include/rescaleframe/gl_shared_surface.h](runtime/presentation/include/rescaleframe/gl_shared_surface.h)
- [x] [runtime/presentation/src/gl_shared_surface.cpp](runtime/presentation/src/gl_shared_surface.cpp)

**Fixtures (1)**

- [x] [tests/gl_shared_surface.cpp](tests/gl_shared_surface.cpp)

**Rust review findings (10)**

- [x] **Altitude | Medium | Make the OpenGL child-window and swapchain resize one recoverable operation.** Pipeline::prepare resizes the child window first and returns on a failed DXGI resize without restoring its old extent. Configure keeps the preceding surfaces, width and height on this return, leaving the visible window at the rejected size. A subsequent request for the old Shape takes the fast path and never repairs the mismatch. Roll back the window resize or track each actual extent and reconcile both before reporting the surviving configuration. [runtime/orchestrator/src/gl_pipeline.rs:505](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/gl_pipeline.rs#L505)

- [x] **Altitude | Medium | Publish the result of the complete OpenGL SR reconfiguration.** select_sr stores the result of creating the candidate provider before surface allocation and resize finish. The existing rejected_switch_keeps_published_session_and_a_retry_rebuilds test explicitly observes FEATURE_FAILED from configure when the third replacement texture fails, but status reports requested FSR3, effective DLSS and sr_result OK. Preserve the active session and record the final configuration result, with a separate field if provider discovery or evaluation results must also be retained. [runtime/orchestrator/src/gl_pipeline.rs:573](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/gl_pipeline.rs#L573)

- [x] **Altitude | Medium | Carry the presented OpenGL view identity through to FG.** Evaluate and hudless accept the engine's pass and keep its view_key, but the generic present entry accepts only a frame ID and constructs a window pass with view_key fixed to 1. Minecraft happens to supply 1 in games/minecraft/native/frame.rs:78. A caller supplying a different valid view cannot match its captured depth, motion and HUD-less inputs in fg12::View::matches. Pass the presented view explicitly, or retain the frame-to-view association and reject ambiguous frames. [runtime/orchestrator/src/gl_pipeline.rs:720](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/gl_pipeline.rs#L720)

- [x] **Altitude | Medium | Keep an owned presentation window retryable when destruction fails.** Pipeline::close checks the GL drain and close results, but discards the result of window_destroy, clears both window pointers and marks the pipeline closed. The Win32 leaf returns failure when the owning thread does not process the destruction request, so close can report success while leaving the child alive with no handle to retry. Model the remaining window teardown as a distinct step, retain its handle on failure and retry only the outstanding work. [runtime/orchestrator/src/gl_pipeline.rs:757](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/gl_pipeline.rs#L757)

- [x] **Altitude | Medium | Return a cleanup owner for partially opened OpenGL pipelines.** After FG hook installation succeeds, a failed or unintercepted DXGI chain causes open to forget the chain and return an error with a null output. The GL sharing object and any owned child window also remain live, and fg12's process-wide registration makes reopening return NEEDS_RESTART. The caller cannot retire these resources or detach its borrowed logger. Separate hook registration from per-pipeline resources and return a cleanup-only handle when callbacks still require them. [runtime/orchestrator/src/gl_pipeline.rs:924](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/gl_pipeline.rs#L924)

- [x] **Altitude | Medium | Make the GL semaphore operations report whether the ownership transfer succeeded.** The native release and acquire leaves return void even though their GL semaphore operations can set error flags. Begin schedules a D3D12 queue wait on the assumed GL signal, and settle marks the slot idle after an assumed GL wait. A rejected signal can leave the queue waiting indefinitely, while a rejected acquire lets the caller reuse a texture without the promised synchronization. Return the GL result from both leaves and retain an explicit failed-transfer state until recovery is proven. [runtime/orchestrator/src/gl_shared_surface.rs:432](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/gl_shared_surface.rs#L432)

- [x] **Altitude | Medium | Have GL submission reacquire the batch released by begin.** The begin operation validates and releases textures and retains their D3D12 resources, but stores only the slot in Phase::Recording. Submit accepts a second caller-supplied texture list and uses it for the GL acquire without comparing it with the first. Supplying a different valid batch, or zero textures, can complete the operation without restoring the original textures' ownership. Store the released batch with the recording and consume it at submit, or reject a mismatch before executing commands. [runtime/orchestrator/src/gl_shared_surface.rs:832](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/gl_shared_surface.rs#L832)

- [x] **Boundaries | Low | Use the shared Runtime SR and FG Auto policies in the OpenGL pipeline.** gl_pipeline.rs:43 and :429 resolve SR Auto by trying DLSS, FSR4, XeSS, FSR3, FSR2 in a fixed order until one session is created, regardless of GPU vendor, while the Runtime Auto owner rsf_gpu_select_sr_backend (lib.rs:385) picks the vendor preference and then FSR3/XeSS from the servable choices and is what callback_host, d3d11_host, render_host, Unity and Godot use. gl_pipeline.rs:524 likewise resolves FG Auto with rsf_fg_default_backend(vendor, software) alone and skips rsf_fg_select_auto_backend (fg_choice.rs:47) and its served-provider/FSR3 fallback. The same Auto setting therefore selects different backends on the OpenGL host (for example DLSS attempted first on AMD/Intel, Off instead of FSR3 for an unknown vendor), and backend choice is split across two Runtime policies instead of one owner. [runtime/orchestrator/src/gl_pipeline.rs:43](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/gl_pipeline.rs#L43)

- [x] **Simplicity | Low | Rust hosts reach the sibling sr12 and fg12 owners through C++ wrapper leaves** gl_pipeline.rs:98-109 has platform slots sr_create, sr_plan, sr_evaluate, sr_set_sharpening, sr_destroy, fg_install, fg_options, fg_cpu, fg_hudless, fg_submitted, fg_window and fg_status. gl_pipeline_platform.cpp:199-210 and :233-235 fill them with rsf_sr12_* and rsf_fg12_*. Those are C++ wrappers: sr12_platform.cpp:393-396 rsf_sr12_create just returns rsf_core_sr12_create(setup, out, &platform), and fg12_platform.cpp:240-304 makes each rsf_fg12_* call bind_platform() and forward to rsf_core_fg12_*. Both are #[no_mangle] Rust exports in this crate (sr12.rs:1287, fg12.rs:392-1274). The same detour is used by render_host (render_host_platform.cpp:118-122) and the callback host (callback_render_host_platform.cpp:87-107). So a Rust owner calls a C++ leaf, which calls a Rust export, which re-validates raw pointers and struct sizes the caller already built in Rust. Simpler: register the sr12 and fg12 platform tables once (fg12 already keeps a OnceLock PLATFORM, fg12.rs:32), and have gl_pipeline, render_host and callback_host call crate::sr12 and crate::fg12 functions directly. The twelve per-host slots and their C++ fills go away. Tradeoff: sr12 must hold its platform table globally rather than take it per rsf_sr12_create, and the host must ensure the one-time bind ran before its first FG call, which is what bind_platform() does inside every wrapper today. gl_pipeline does the same for its nine gl_* slots (gl_pipeline.rs:75-83): gl_pipeline_platform.cpp:177-184 and :230-231 fill them with rsf_gl_graphics, rsf_gl_texture_create, rsf_gl_begin and the others, which are #[no_mangle] Rust exports of gl_shared_surface.rs:718-881 in the same crate, so a typed gl_shared_surface::Context is handled as an opaque Handle. native_window.rs:161-166 likewise receives native_scene.rs:171 rsf_native_scene_for_window back as its query_scene function-pointer argument (native_window_platform.cpp:31-32) instead of calling crate::native_scene directly. overlay_host.rs:191 calls the platform load_panel slot, which loader/proxy/src/overlay_host.cpp:27-30 implements as rsf_core_overlay_panel_open from overlay_panel.rs:87 in this crate. The panel table it returns has already been checked by complete() (overlay_panel.rs:29-35), but it is then unwrapped through leaf() at each use. overlay_service.rs gets input_counters_get, input_routes_get and fg_status_get through rsf_overlay_platform (overlay_platform.cpp:90-92). Those are C++ forwards to this crate's rsf_core_overlay_input_counters (overlay_input_route.rs:363, via runtime/graphics/src/overlay_input.cpp:424-427), rsf_core_native_input_routes_get (native_fg_cpu.rs:1490, via native_fg.cpp:115-117) and rsf_core_native_fg_status_get (native_fg.rs:693, via native_fg_platform.cpp:229-232). Meanwhile overlay_service.rs:7 already calls native_cpu::rsf_native_input_receipts_get directly. This is related to but distinct from the auxiliary_sr C-export entry (PORT.md, Boundaries) and the depth_scale entry. [runtime/orchestrator/src/gl_pipeline.rs:98](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/gl_pipeline.rs#L98)

- [x] **Duplication | Low | Use one reserve-then-initialize operation for graphics owner handles.** GL context open, texture creation at runtime/orchestrator/src/gl_shared_surface.rs:754 and pipeline open at runtime/orchestrator/src/gl_pipeline.rs:969 all call the shared allocator, then separately cast uninitialized storage, write the completed object, publish it, or rebuild a Box to free it on failure. SR session open at runtime/orchestrator/src/d3d12_sr.rs:109 already keeps a Box<MaybeUninit<_>> until Box::write succeeds. Put that initialization transaction beside allocate_handle, taking a fallible initializer and caller-specific allocation error. This preserves the necessary allocation-before-GPU-side-effects ordering and removes three manual cleanup branches. [runtime/orchestrator/src/gl_shared_surface.rs:697](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/gl_shared_surface.rs#L697)

<a id="v01"></a>

### V01 · Vulkan SR and provider admission

Port the shared Vulkan SR services, split routes and provider-selection boundaries.

**Coordinate shared edits**

- Shared Vulkan resource state, provider selection and queue retirement ([V02](#v02), [V03](#v03), [V04](#v04), [V05](#v05), [V06](#v06)).

**Port files (8)**

- [x] [runtime/orchestrator/include/rescaleframe/native_sr_split_vulkan.h](runtime/orchestrator/include/rescaleframe/native_sr_split_vulkan.h)
- [x] [runtime/orchestrator/include/rescaleframe/native_sr_vulkan.h](runtime/orchestrator/include/rescaleframe/native_sr_vulkan.h)
- [x] [runtime/orchestrator/src/native_sr_split_lease.h](runtime/orchestrator/src/native_sr_split_lease.h)
- [x] [runtime/orchestrator/src/native_sr_split_vulkan.cpp](runtime/orchestrator/src/native_sr_split_vulkan.cpp)
- [ ] [runtime/orchestrator/src/native_sr_vulkan.cpp](runtime/orchestrator/src/native_sr_vulkan.cpp)
- [x] [runtime/orchestrator/src/native_vulkan_timing.h](runtime/orchestrator/src/native_vulkan_timing.h)
- [x] [runtime/orchestrator/src/vulkan_fidelityfx_native.h](runtime/orchestrator/src/vulkan_fidelityfx_native.h)
- [x] [runtime/orchestrator/src/vulkan_sdk_native.h](runtime/orchestrator/src/vulkan_sdk_native.h)

**Fixtures (4)**

- [x] [tests/native_sr_split_vulkan.cpp](tests/native_sr_split_vulkan.cpp)
- [x] [tests/native_sr_vulkan.cpp](tests/native_sr_vulkan.cpp)
- [x] [tests/native_sr_xess_range_check.py](tests/native_sr_xess_range_check.py) (writes a C++ contract fixture)
- [x] [tests/native_vulkan_exports.cpp](tests/native_vulkan_exports.cpp)

**Rust review findings (10)**

- [x] **Duplication | Medium | Share Vulkan image allocation while preserving FidelityFX's nonlocal memory fallback.** FidelityFX allocates its output and zero mask at :635 by repeating create-image, query requirements, choose a memory type, allocate and bind through the same image callback table used by runtime/orchestrator/src/vulkan_fg_resources.rs:94. The shared allocator checks success handles and rolls back a partial replacement. StagingImage instead publishes partial handles for later release and its memory_type at :486 permits a compatible nonlocal type. Factor the allocation transaction into the shared resource owner with an explicit memory-choice policy, then adapt its result to FidelityFX's INIT_FAILED contract. [runtime/orchestrator/src/vulkan_sr.rs:446](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_sr.rs#L446)

- [x] **Altitude | Medium | Give the SR output capture the same stage meaning across Vulkan providers.** The FidelityFX path records sr-output-linear after its optional post-sharpening dispatch. Vulkan DLSS records the identically named input before sharpening, and the owner fixtures explicitly assert both orderings. A comparison of SDK outputs can therefore include RSF's filter on one provider only. Capture the raw SDK output before post-processing on every path, labeling any separately requested final sharpened output by its actual stage. [runtime/orchestrator/src/vulkan_sr.rs:753](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_sr.rs#L753)

- [x] **Altitude | Medium | Return a cleanup-only Vulkan SR owner when failed creation cannot retire the SDK.** If FidelityFx::create fails after creating an effect and Drop cannot destroy it, Drop forgets the module and device-dispatch lease and abandons the native owner. rsf_core_srvk_create leaves out null, so no caller can retry cleanup even if destruction becomes possible later. Preserve the partially constructed owner for checked destruction, as the Vulkan FG creation path already does, while refusing evaluation and plan queries on it. [runtime/orchestrator/src/vulkan_sr.rs:803](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_sr.rs#L803)

- [x] **Altitude | Medium | Honor XeSS context destruction failures during constructor rollback.** XessOwner::destroy preserves a published owner if the SDK refuses destruction, but Drop during failed initialization ignores the same destroy_context result, releases the private images, closes the native handle and releases the library lease. A failure after context creation can thus leave a live SDK context without its cleanup owner. Keep the partially constructed owner and its dependencies reachable until checked teardown succeeds, and inject a rollback refusal in the initialization fixture. [runtime/orchestrator/src/vulkan_sr/xess.rs:662](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_sr/xess.rs#L662)

- [x] **Duplication | Medium | Exercise the production Vulkan SR admission path in XeSS owner fixtures.** XeSS fixtures invoke a second create/plan/evaluate/retire/destroy surface, including tests/native_xess_vulkan.cpp:129 and :152. Its owner_admit normalizes the input and checks frame_admit before calling XeSS directly. The actual rsf_core_srvk_evaluate at runtime/orchestrator/src/vulkan_sr.rs:907 also validates sharpening, starts capture and records the backend result; its create branch calls XessOwner::create directly at :833. Route end-to-end fixture calls through the production dispatcher with the XeSS fake platform, and test any isolated owner behavior directly so the separate fixture ABI cannot drift. [runtime/orchestrator/src/vulkan_sr/xess.rs:678](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_sr/xess.rs#L678)

- [x] **Simplicity | Low | Set fake Vulkan blit capabilities on the fake device.** fallback_probe reads the native format feature mask from call.output.copy_source, so the fallback test writes LINEAR_BLIT into a resource field that represents copy-source availability. The setup already supplies a Fake through call.device. Give Fake a blit_features field and have the probe read it there. This adds one fixture field and lets the existing supported, unsupported and alias cases use ordinary resource descriptors. [runtime/orchestrator/src/vulkan_sr/owner_tests.rs:428](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_sr/owner_tests.rs#L428)

- [x] **Simplicity | Low | Store each XeSS private image descriptor once.** Private carries an Image alongside OwnedImage. Creation writes formats and dimensions into the first, passes them to ensure_image, then copies the allocated image handle back. texture reads the first handle and format, the second view, and takes the allocation dimensions again from its caller. Keep the four OwnedImage records, pass requested format and extent directly at allocation, and construct the SDK texture from the resulting record. Build the output capture descriptor locally with setup.device and GENERAL layout. This adds a small conversion at the capture boundary and keeps active render dimensions independent of the maximum allocation. [runtime/orchestrator/src/vulkan_sr/xess.rs:334](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_sr/xess.rs#L334)

- [x] **Simplicity | Low | Remove the global raw pointer from the XeSS test fixture.** with_state publishes a pointer to a stack Fake, then passes a separate mutable borrow to the test. The callback helper state() can manufacture a mutable reference with any lifetime and leaves the pointer installed if an assertion unwinds. Keep the SDK-session serialization mutex and put the fake event log and configurable results in a thread-local RefCell, borrowing it only to edit or inspect data between calls. The synchronous fake callbacks can take their own short borrows, at the cost of slightly more explicit test assertions. [runtime/orchestrator/src/vulkan_sr/xess/tests.rs:66](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_sr/xess/tests.rs#L66)

- [x] **Duplication | Low | Share the Vulkan SR test callback macro between sibling fixtures.** The XeSS fixture and runtime/orchestrator/src/vulkan_sr/owner_tests.rs:89 define the same leaf macro that turns a typed inline body into an unsafe extern C callback. The fixture tables already compose across modules: the shared platform uses XeSS OPS, and the XeSS routing test calls that shared platform. Define the macro once under cfg(test) in their parent module so callback calling-convention and syntax changes apply to both tables. [runtime/orchestrator/src/vulkan_sr/xess/tests.rs:76](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_sr/xess/tests.rs#L76)

- [x] **Duplication | Low | Use the admitted Vulkan FSR4 setup when constructing the split owner.** The public create path discards the two validated records returned by admit_setup. Its provider callback at native_sr_split_vulkan.cpp:46 goes through runtime/orchestrator/src/native_sr_fsr4_vulkan.cpp:177 to runtime/orchestrator/src/vulkan_fsr4_sr.rs:475, where the same function rereads and revalidates both input structures. Move owner initialization into a Rust function accepting the admitted Setup/Handoff, have independent ABI entries validate once at their own boundary, and keep the public NOT_COMPILED and private result mappings at the appropriate entry. [runtime/orchestrator/src/vulkan_sr_split.rs:101](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_sr_split.rs#L101)

<a id="v02"></a>

### V02 · Vulkan FG and shared WSI

Port common frame-generation resources, transfers, submission and swapchain lifetime for Vulkan.

**Coordinate shared edits**

- Capture publication after a submitted command or queue is retired ([G07](#g07), [V04](#v04), [V05](#v05), [V06](#v06)).
- Shared Vulkan resource state, provider selection and queue retirement ([V01](#v01), [V03](#v03), [V04](#v04), [V05](#v05), [V06](#v06)).

**Port files (9)**

- [x] [runtime/orchestrator/include/rescaleframe/native_fg_vulkan.h](runtime/orchestrator/include/rescaleframe/native_fg_vulkan.h)
- [x] [runtime/orchestrator/src/native_fg_ffx_vulkan.h](runtime/orchestrator/src/native_fg_ffx_vulkan.h)
- [ ] [runtime/orchestrator/src/native_fg_vulkan.cpp](runtime/orchestrator/src/native_fg_vulkan.cpp)
- [ ] [runtime/orchestrator/src/native_fg_vulkan_dispatch.cpp](runtime/orchestrator/src/native_fg_vulkan_dispatch.cpp)
- [x] [runtime/orchestrator/src/native_fg_vulkan_platform.h](runtime/orchestrator/src/native_fg_vulkan_platform.h)
- [x] [runtime/orchestrator/src/vulkan_fg_copy.h](runtime/orchestrator/src/vulkan_fg_copy.h)
- [x] [runtime/orchestrator/src/vulkan_fg_frames.h](runtime/orchestrator/src/vulkan_fg_frames.h)
- [x] [runtime/orchestrator/src/vulkan_fg_resources.h](runtime/orchestrator/src/vulkan_fg_resources.h)
- [x] [runtime/orchestrator/src/vulkan_fg_resources_native.h](runtime/orchestrator/src/vulkan_fg_resources_native.h)

**Fixtures (8)**

- [x] [tests/vulkan-fg/dispatch_fixture.cpp](tests/vulkan-fg/dispatch_fixture.cpp) (fake provider callbacks)
- [ ] [tests/vulkan-fg/ffx_fixture.cpp](tests/vulkan-fg/ffx_fixture.cpp) (SDK records and Vulkan callbacks)
- [ ] [tests/ffx_vulkan_wsi_dispatch.cpp](tests/ffx_vulkan_wsi_dispatch.cpp)
- [x] [tests/native_ffx_vulkan_harness.h](tests/native_ffx_vulkan_harness.h)
- [x] [tests/native_ffx_vulkan_reset.cpp](tests/native_ffx_vulkan_reset.cpp)
- [x] [tests/native_ffx_vulkan_submission.cpp](tests/native_ffx_vulkan_submission.cpp)
- [x] [tests/native_ffx_vulkan_wsi_route.cpp](tests/native_ffx_vulkan_wsi_route.cpp)
- [x] [tests/native_fg_vulkan_dispatch.cpp](tests/native_fg_vulkan_dispatch.cpp)

**Rust review findings (4)**

- [x] **Duplication | Medium | Use one image-barrier ABI for native Vulkan and Vulkan-to-DX12 command recording.** The shared Vulkan recorder sends its nine-field barrier to a native callback at :109. The DX12 bridge defines the identical 40-byte record at runtime/orchestrator/src/vulkan_dx12_fg.rs:325 and sends it through GraphicsOps::record at :518. Their native declarations are also maintained separately in runtime/orchestrator/src/vulkan_fg_copy.h:6 and runtime/orchestrator/src/native_fg_dx12_vulkan_platform.h:34. Use the existing barrier type and header for both with a queue-family-aware constructor for external acquire/release. Keep the distinct copy/blit descriptors, which carry different extent and layout information. [runtime/orchestrator/src/vulkan_fg_copy.rs:20](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_fg_copy.rs#L20)

- [x] **Altitude | Medium | Apply the stored source's FG veto in the shared presentation admission check.** Implicit PresentSource reports true for both matches and allows_fg. The shared present_candidate checks submission/completion but never checks NO_FG or AMBIGUOUS_ID on the stored frame. Vulkan FFX and DLSS pass those values straight through, while vulkan_dx12_fg.rs separately rejects the stored flags. Enforce the stored veto in the common predicate, with the explicit Present identity able to impose an additional veto; test both provider call styles. [runtime/orchestrator/src/vulkan_fg_frames.rs:248](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_fg_frames.rs#L248)

- [x] **Simplicity | Low | Let the WSI route determine which FidelityFX owner can release it.** Owner.admitted mirrors Route.owner solely to guard withdraw. Route::release already compares the caller ID under its mutex before resetting the route, so both a refused applicant and a repeated release leave another owner untouched. Call route().release(self.id()) at the existing two teardown points and remove admitted and withdraw. This takes one extra mutex acquisition on failed admission while keeping the route held through module unload. [runtime/orchestrator/src/vulkan_ffx_fg.rs:840](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_ffx_fg.rs#L840)

- [x] **Duplication | Low | Give Vulkan FG recording owners a typed submission-plan function.** DLSS-G at runtime/orchestrator/src/vulkan_dlss_fg.rs:533, FidelityFX at runtime/orchestrator/src/vulkan_ffx_fg.rs:606 and the Vulkan-to-DX12 bridge at runtime/orchestrator/src/vulkan_dx12_fg.rs:1036 each assemble raw pointers to the six recording slots and initialize a three-mask plan that the shared FFI function immediately overwrites. The bridge even converts an existing command slice back into pointer/count. Return the plan from one Rust function taking a fixed array of mutable slot references and a command slice, adapt the C entry once if retained, and leave backend-specific fence and capture actions at the callers. [runtime/orchestrator/src/vulkan_fg_frames.rs:175](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_fg_frames.rs#L175)

<a id="v03"></a>

### V03 · Vulkan DX12 interop and generation

Port Vulkan-to-D3D12 graphics and queue handoff shared by the FSR4 and XeSS FG consumers.

**Coordinate shared edits**

- Shared Vulkan resource state, provider selection and queue retirement ([V01](#v01), [V02](#v02), [V04](#v04), [V05](#v05), [V06](#v06)).

**Port files (5)**

- [x] [runtime/orchestrator/src/native_dx12_vulkan_interop.cpp](runtime/orchestrator/src/native_dx12_vulkan_interop.cpp)
- [x] [runtime/orchestrator/src/native_dx12_vulkan_interop.h](runtime/orchestrator/src/native_dx12_vulkan_interop.h)
- [x] [runtime/orchestrator/src/native_dx12_vulkan_platform.h](runtime/orchestrator/src/native_dx12_vulkan_platform.h)
- [x] [runtime/orchestrator/src/native_fg_dx12_vulkan.cpp](runtime/orchestrator/src/native_fg_dx12_vulkan.cpp)
- [x] [runtime/orchestrator/src/native_fg_dx12_vulkan_platform.h](runtime/orchestrator/src/native_fg_dx12_vulkan_platform.h)

**Fixtures (2)**

- [x] [tests/native_dx12_vulkan_interop.cpp](tests/native_dx12_vulkan_interop.cpp)
- [x] [tests/vulkan_interop_fixture.h](tests/vulkan_interop_fixture.h)

**Rust review findings (5)**

- [x] **Altitude | Medium | Resolve Vulkan dispatch where the typed function pointers live.** The native interop open function calls Rust to enumerate thirteen procedure names, Rust calls its C++ lookup thunk once per name, and native_dx12_vulkan_interop.cpp:65 assigns the resulting untyped array to Vulkan function pointers by position. Changing one list's order can bind a non-null address with the wrong calling signature and still pass Rust's completeness check. Let native open resolve each named procedure into its typed member and return one status to the Rust owner. [runtime/orchestrator/src/dx12_vulkan_interop.rs:60](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/dx12_vulkan_interop.rs#L60)

- [x] **Duplication | Medium | Validate Vulkan image descriptors through the existing shared predicate.** The bridge checks device identity, nonzero image, extent, shader-read layout, exact format and copy-source permission before staging depth, motion and HUD-less inputs at :932 and :997. DLSS-G repeats these checks at runtime/orchestrator/src/vulkan_dlss_fg.rs:401, while FidelityFX's directly consumed guides at runtime/orchestrator/src/vulkan_ffx_fg.rs:450 omit the copy permission. Move or reuse runtime/orchestrator/src/vulkan_sr.rs:104 valid_image for the shared descriptor fields and require copy_source only at the copy callers, so both presentation and reconstruction interpret the same descriptor consistently. [runtime/orchestrator/src/vulkan_dx12_fg.rs:897](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_dx12_fg.rs#L897)

- [x] **Altitude | Medium | Resize retired FG guide images independently of the presentation swapchain.** After waiting for the slot's consumer timeline and source fences, prepare returns OUT_OF_DATE whenever guide width or height differs from its first use. A render-scale or quality change can therefore disable the FSR4/XeSS bridge on each reused slot even though its output dimensions and swapchain still match. The importer currently retains images until whole-owner destruction. Add a checked replacement/release operation at the imported-image owner and resize only the drained depth and motion pair. [runtime/orchestrator/src/vulkan_dx12_fg.rs:945](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_dx12_fg.rs#L945)

- [x] **Duplication | Low | Derive Vulkan FG capture group IDs in the recording ledger.** Both the DX12 bridge and DLSS-G at runtime/orchestrator/src/vulkan_dlss_fg.rs:150 encode the shared recording_sequence and the guides/HUD-less discriminator into the same integer. The bridge uses shifts for recording, completion and discard at :857, :985 and :1060, while DLSS uses multiplication for SDK capture and submission discard at :484 and :552. Put the group calculation on the common recording type in vulkan_fg_frames and use it at each capture lifecycle step so recording and retirement cannot adopt different encodings. [runtime/orchestrator/src/vulkan_dx12_fg.rs:717](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_dx12_fg.rs#L717)

- [x] **Simplicity | Low | Use the committed timeline values to identify first-use images.** Before submitting the first Present the fallback images and each imported backbuffer start uninitialized. The only writes to fallback_initialized and buffer.initialized occur together with last_done and buffer.done after a successful present_submit. The done value is allocated with checked_add(2), so zero uniquely means no successful submission. Gate fallback clear on last_done == 0 and choose the backbuffer acquire barrier using buffer.done != 0. The tradeoff is documenting that zero sentinel once; the independent guide and HUD-less submission states still need their own flags. [runtime/orchestrator/src/vulkan_dx12_fg.rs:1183](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_dx12_fg.rs#L1183)

<a id="v04"></a>

### V04 · Streamline and NGX Vulkan

Port SDK loading, DLSS SR and FG, capture and session lifetime across the Runtime and standalone bridges.

**Coordinate shared edits**

- Capture publication after a submitted command or queue is retired ([G07](#g07), [V02](#v02), [V05](#v05), [V06](#v06)).
- Shared Vulkan resource state, provider selection and queue retirement ([V01](#v01), [V02](#v02), [V03](#v03), [V05](#v05), [V06](#v06)).

**Port files (18)**

- [x] [runtime/orchestrator/include/rescaleframe/native_fg_dlss_vulkan.h](runtime/orchestrator/include/rescaleframe/native_fg_dlss_vulkan.h)
- [x] [runtime/orchestrator/include/rescaleframe/native_streamline_vulkan.h](runtime/orchestrator/include/rescaleframe/native_streamline_vulkan.h)
- [ ] [runtime/orchestrator/src/native_dlss_vulkan.cpp](runtime/orchestrator/src/native_dlss_vulkan.cpp)
- [ ] [runtime/orchestrator/src/native_dlss_vulkan_platform.h](runtime/orchestrator/src/native_dlss_vulkan_platform.h)
- [ ] [runtime/orchestrator/src/native_fg_dlss_vulkan.cpp](runtime/orchestrator/src/native_fg_dlss_vulkan.cpp)
- [ ] [runtime/orchestrator/src/native_fg_dlss_vulkan_capture.cpp](runtime/orchestrator/src/native_fg_dlss_vulkan_capture.cpp)
- [ ] [runtime/orchestrator/src/native_fg_dlss_vulkan_capture.h](runtime/orchestrator/src/native_fg_dlss_vulkan_capture.h)
- [ ] [runtime/orchestrator/src/native_fg_dlss_vulkan_capture_ledger.h](runtime/orchestrator/src/native_fg_dlss_vulkan_capture_ledger.h)
- [ ] [runtime/orchestrator/src/native_fg_dlss_vulkan_platform.h](runtime/orchestrator/src/native_fg_dlss_vulkan_platform.h)
- [x] [runtime/vulkan/ngx/vulkanstory_ngx.h](runtime/vulkan/ngx/vulkanstory_ngx.h)
- [ ] [runtime/vulkan/streamline/sdk_fg_options_checks.h](runtime/vulkan/streamline/sdk_fg_options_checks.h) (native DLSS-G SDK layout assertions)
- [ ] [runtime/vulkan/streamline/sdk_records_checks.h](runtime/vulkan/streamline/sdk_records_checks.h) (native SDK layout assertions)
- [ ] [runtime/vulkan/streamline/bridge.cpp](runtime/vulkan/streamline/bridge.cpp)
- [ ] [runtime/vulkan/streamline/present-capture.h](runtime/vulkan/streamline/present-capture.h)
- [ ] [runtime/vulkan/streamline/sdk-fixes/ngx-vulkan-dispatch.h](runtime/vulkan/streamline/sdk-fixes/ngx-vulkan-dispatch.h)
- [ ] [runtime/vulkan/streamline/sdk-fixes/prepare.py](runtime/vulkan/streamline/sdk-fixes/prepare.py) (patches the Streamline SDK source)
- [ ] [runtime/vulkan/streamline/sdk-fixes/selected-device-config.h](runtime/vulkan/streamline/sdk-fixes/selected-device-config.h)
- [ ] [runtime/vulkan/streamline/sdk-fixes/selected_device.py](runtime/vulkan/streamline/sdk-fixes/selected_device.py) (writes C++ changes to Streamline device selection)

**Fixtures (5)**

- [ ] [runtime/vulkan/streamline/sdk-fixes/dispatch-tests.cpp](runtime/vulkan/streamline/sdk-fixes/dispatch-tests.cpp)
- [x] [tests/native_dlss_vulkan.cpp](tests/native_dlss_vulkan.cpp)
- [ ] [tests/native_fg_dlss_vulkan.cpp](tests/native_fg_dlss_vulkan.cpp)
- [ ] [tests/streamline_selected_device.cpp](tests/streamline_selected_device.cpp)
- [ ] [tests/streamline_vulkan_dispatch.cpp](tests/streamline_vulkan_dispatch.cpp)

**Rust review findings (15)**

- [x] **Altitude | High | Close a native Vulkan capture exactly once on Rust owner allocation failure.** rsf_core_dlss_capture_open passes a fully constructed Owner to allocate_handle. On allocation refusal that helper never moves its argument into the allocation, so normal Rust drop calls Owner::drop and closes native at line 203. The caller then calls ops.close(native) again. Let Owner be the sole cleanup owner once constructed and remove the second close, or allocate uninitialized storage before transferring the handle. [runtime/orchestrator/src/dlss_vulkan_capture.rs:446](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/dlss_vulkan_capture.rs#L446)

- [x] **Duplication | Medium | Share the common Vulkan FG source-record admission rules.** The DLSS-G record path at :412 and FidelityFX prepare and HUD-less paths at runtime/orchestrator/src/vulkan_ffx_fg.rs:498 and :542 independently validate the same frame ID, camera ABI, extent agreement, frame time, near/far planes, FOV, jitter, and four transforms. FidelityFX additionally checks world_to_view at :417, while DLSS checks the setup motion/depth flags here and FidelityFX checks them only in guides_valid at :464. Extract the overlapping record predicates into vulkan_fg_frames and retain provider-specific matrix and guide requirements at their callers so a camera ABI change has one common admission rule. [runtime/orchestrator/src/vulkan_dlss_fg.rs:366](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_dlss_fg.rs#L366)

- [x] **Quality | Medium | A WSI error during active Vulkan DLSS-G quarantines the owner and the global FG claim for good** present() (lines 617-627) sets slot.inputs = Inputs::Unknown when a Present with FG active is not accepted. That covers VK_ERROR_OUT_OF_DATE_KHR (-1000001004), which a window resize produces, and the case where sample() refuses. Nothing ever moves Unknown back to Retired: Inputs::retire (lines 117-129) returns false for Unknown on every call. record() (lines 439-442) then returns NOT_READY for every later frame mapped to that slot. drain() (lines 685-689) also returns NOT_READY, and it checks retire() only after configure(false) and a successful vkDeviceWaitIdle with an unchanged timeout epoch, by which point no queued input work can still be outstanding. As a result rsf_core_dlgvk_destroy (line 980) never succeeds. The vulkan_dlss_fg Claim and the vulkan_fg.rs PrimaryClaim (OCCUPIED) stay held, so rsf_core_fgvk_create returns NOT_READY for every provider (DLSS, FFX, FSR4, XeSS) for the rest of the process, and the swapchain recreated after the resize gets no FG. owner_tests.rs:326-344 asserts this quarantine (destroy returns NOT_READY after a present_result of -1000001004) and drops the owner by hand. No path is tested in which the device-idle drain retires Unknown inputs. [runtime/orchestrator/src/vulkan_dlss_fg.rs:626](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_dlss_fg.rs#L626)

- [x] **Duplication | Medium | Generate both views of the NGX entry table from one declaration.** Rust resolves and publishes twelve opaque addresses in NgxEntries at :59 and :87. The native shim independently repeats all twelve fields at runtime/vulkan/ngx/vulkanstory_ngx.c:31 and casts them to different function signatures when called, for example CreateFeature1 at :413. A same-sized field reorder on one side still compiles and can dispatch the wrong SDK function. Give the private table one header definition and generate/include its Rust layout, retaining the nullable optional entries and native call wrappers. [runtime/vulkan/ngx/src/lib.rs:10](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/vulkan/ngx/src/lib.rs#L10)

- [x] **Altitude | Medium | Return the acquired Streamline frame token with the begin result.** LatencySession.cs:160-164 calls BeginFrame and then CurrentFrameToken to associate a frame ID with an SDK token. begin releases its acquisition lease before that second FFI call, so another successful begin can replace the global current pointer in between. Return the newly acquired token from the checked begin operation and store it against the requesting frame immediately, keeping the legacy getter only for compatibility. [runtime/vulkan/streamline/lifecycle/src/frame_token.rs:77](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/vulkan/streamline/lifecycle/src/frame_token.rs#L77)

- [x] **Altitude | Medium | Check the requested Streamline configuration before reporting an initialized session.** VulkanStorySlInitializeFeatures takes a plugin directory, project ID and requested features. The initialized fast path returns success before reading any of them. A PCL-only session thus reports successful DLSS-G initialization without loading DLSS-G, and a different project is likewise silently accepted. Store the effective initialization tuple and return success only for compatible requests, or explicitly report the active configuration. [runtime/vulkan/streamline/lifecycle/src/lifecycle.rs:241](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/vulkan/streamline/lifecycle/src/lifecycle.rs#L241)

- [x] **Altitude | Medium | Return a checked outcome when the Streamline interposer cannot unload.** Lifecycle::unload silently returns when the session is busy, including the condition covered by overlapping_lifecycle_call_is_refused_without_waiting. Through VulkanStorySlUnload the managed Dispose still frees the bridge library, clears its teardown ledger and marks itself disposed. Return BUSY or the retained shutdown error through a checked unload export and keep the bridge and disposal state until unloading succeeds. [runtime/vulkan/streamline/lifecycle/src/lifecycle.rs:348](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/vulkan/streamline/lifecycle/src/lifecycle.rs#L348)

- [x] **Simplicity | Low | Import the five Vulkan FG resource functions from their Rust owner.** The DLSS-G owner redeclares five functions as foreign symbols and passes a raw platform-table pointer each time it creates or releases images or handles fence submission. All five definitions already have pub(super) visibility in vulkan_fg_resources.rs and take a platform reference. Import the definitions, obtain a valid reference to the GPU table at admission, and keep the existing exported symbols for native callers. This requires checking the table once and lets Rust check future signature changes at the DLSS-G call sites. [runtime/orchestrator/src/vulkan_dlss_fg.rs:48](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_dlss_fg.rs#L48)

- [x] **Duplication | Low | Share the bounded swapchain-image enumeration used by Vulkan FG providers.** The DLSS-G images export and the FidelityFX owner at runtime/orchestrator/src/vulkan_ffx_fg.rs:468 (called by its public images route at :954) separately query the available count, clamp caller capacity, fill the array, reject overreported writes, and return VK_INCOMPLETE for a short buffer. Different SDK proxy quirks need the same protection against exposing uninitialized tail handles. Extract the bounded enumeration once, accepting a closure that captures each native chain and preserves its result codes, and exercise count-only, query failure, short, oversized, and overreported fills against it. [runtime/orchestrator/src/vulkan_dlss_fg.rs:825](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_dlss_fg.rs#L825)

- [x] **Duplication | Low | Reuse the atomic ownership guard across Vulkan singleton owners.** The SR creator at :249 takes an atomic flag and stores a guard that releases it on Drop. Vulkan DLSS-G repeats the compare-exchange, orderings and destructor at runtime/orchestrator/src/vulkan_dlss_fg.rs:88 for its creator at :760, and the FG dispatcher repeats them at runtime/orchestrator/src/vulkan_fg.rs:31 for :175. Keep each independent ownership flag but have one guard borrow the selected static AtomicBool, so failed construction and successful destruction use one release mechanism. [runtime/orchestrator/src/vulkan_dlss_sr.rs:24](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_dlss_sr.rs#L24)

- [x] **Duplication | Low | Share the Vulkan SR capture input names and resource mapping.** After evaluation DLSS and XeSS at runtime/orchestrator/src/vulkan_sr/xess.rs:583 emit the same color texture twice as sr-source-jittered and sr-linear-color, then the depth and motion inputs under the same two role strings. FSR4 repeats the same four mappings before its external-queue copy at runtime/orchestrator/src/vulkan_fsr4_sr.rs:250. These names identify resources in cross-backend dumps. Provide a shared iterator over the four (source image, role) pairs while preserving each caller's stage, reset semantics, and the optional DLSS bias mask. [runtime/orchestrator/src/vulkan_dlss_sr.rs:208](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_dlss_sr.rs#L208)

- [x] **Duplication | Low | Construct Streamline Vulkan worker calls from one frame-identity helper.** DLSS SR evaluation writes both token IDs from the frame while effect release at :111 writes both as zero. DLSS-G already constructs the same versioned rsf_slvk_call from an optional record at runtime/orchestrator/src/vulkan_dlss_fg.rs:201, for feature calls at :330 and recording at :499. Move that construction beside the call ABI and pass the platform-specific invoke function separately, so all SDK operations propagate version, callback context and optional token identity consistently. [runtime/orchestrator/src/vulkan_dlss_sr.rs:307](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_dlss_sr.rs#L307)

- [x] **Duplication | Low | Route Vulkan DLSS preset changes through the production preset validator.** The runtime preset setter at runtime/orchestrator/src/vulkan_sr.rs:973 reaches an independent literal allowlist here before releasing the current effect. Native D3D12 startup at runtime/backends/dlss/src/dlss_native12.cpp:368 and callback-host settings at runtime/orchestrator/src/callback_host.rs:615 use the common SDK policy in runtime/backends/dlss/include/rescaleframe/dlss_preset.h:7. An SDK model-list update can consequently make a setting accepted by the overlay unusable on Vulkan. Bind the same production predicate into the Vulkan setter or generate both from a single model list. [runtime/orchestrator/src/vulkan_dlss_sr.rs:319](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_dlss_sr.rs#L319)

- [x] **Boundaries | Low | Rename the game-named Vulkan provider exports under runtime/vulkan.** The Vulkan provider owners are generic vendor code (NGX, FidelityFX, XeSS-FG, Streamline), but their C ABI is named after one game: rsf_vulkanstory_ngx_get_entries/rsf_vulkanstory_ngx_load_error here, the VulkanStoryFsr3*/VulkanStoryFsr4* records mirrored in runtime/vulkan/fsr3/owner.rs:90-167 and fsr4/owner.rs:119, the VulkanStoryXessFrame record in xess-fg/session.rs:29, and the VulkanStorySl* exports that streamline/lifecycle/src/lifecycle.rs:18 serves through bridge.cpp. AGENTS.md Rule 1 places game-named control entry points in the integration, not the runtime, and says an existing violation is not a pattern to follow; runtime/vulkan/README.md keeps the names only as compatibility ABI for the VulkanStory host. A second Vulkan integration would have to call VulkanStory-named functions or fork the bridges. Publish neutral rsf_vk_* names (the rsf_core_* Rust exports already are) and keep any VulkanStory aliases in that host or a thin compatibility shim owned by its integration. [runtime/vulkan/ngx/src/lib.rs:87](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/vulkan/ngx/src/lib.rs#L87)

- [x] **Simplicity | Low | Store the sticky Streamline shutdown result as one optional value.** Session maintains shutdown_attempted and shutdown_result separately. Initialization and unload test both, shutdown returns the prior result on repeat, and successful initialization resets both fields. Use Option<i32> for the attempt result. Reserve Some(0) before invoking the native callback under the session lock and overwrite it with the SDK result if called. None then means no attempt, Some(0) allows reinitialization, and a nonzero result keeps the module resident. The published initialized bit remains necessary for the lock-free API. [runtime/vulkan/streamline/lifecycle/src/lifecycle.rs:100](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/vulkan/streamline/lifecycle/src/lifecycle.rs#L100)

- **Withdrawn after review (Altitude).** Vulkan DLSS SR staging initialization at recording time follows the caller contract. On failure the caller must submit that command buffer in recording order or retire the context before reuse. [Caller contract, lines 73–75](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/include/rescaleframe/native_sr_vulkan.h#L73)

<a id="v05"></a>

### V05 · FidelityFX Vulkan bridges

Port the FSR3 and FSR4 bridge owners, private ABI and SDK patch payloads alongside their state fixtures.

**Coordinate shared edits**

- Capture publication after a submitted command or queue is retired ([G07](#g07), [V02](#v02), [V04](#v04), [V06](#v06)).
- Shared Vulkan resource state, provider selection and queue retirement ([V01](#v01), [V02](#v02), [V03](#v03), [V04](#v04), [V06](#v06)).

**Port files (22)**

- [ ] [runtime/orchestrator/src/native_fg_fsr4_vulkan.cpp](runtime/orchestrator/src/native_fg_fsr4_vulkan.cpp)
- [x] [runtime/orchestrator/src/native_fg_fsr4_vulkan.h](runtime/orchestrator/src/native_fg_fsr4_vulkan.h)
- [ ] [runtime/orchestrator/src/native_fg_fsr4_vulkan_platform.h](runtime/orchestrator/src/native_fg_fsr4_vulkan_platform.h)
- [ ] [runtime/orchestrator/src/native_sr_fsr4_vulkan.cpp](runtime/orchestrator/src/native_sr_fsr4_vulkan.cpp)
- [x] [runtime/orchestrator/src/native_sr_fsr4_vulkan.h](runtime/orchestrator/src/native_sr_fsr4_vulkan.h)
- [x] [runtime/orchestrator/src/native_sr_fsr4_vulkan_platform.h](runtime/orchestrator/src/native_sr_fsr4_vulkan_platform.h)
- [x] [runtime/vulkan/fsr3/bridge.cpp](runtime/vulkan/fsr3/bridge.cpp)
- [x] [runtime/vulkan/fsr3/bridge_platform.h](runtime/vulkan/fsr3/bridge_platform.h)
- [ ] [runtime/vulkan/fsr3/sdk-fixes/descriptor-pool-size.json](runtime/vulkan/fsr3/sdk-fixes/descriptor-pool-size.json) (C++ edits applied to the FidelityFX SDK)
- [ ] [runtime/vulkan/fsr3/sdk-fixes/line-width.json](runtime/vulkan/fsr3/sdk-fixes/line-width.json) (C++ edits applied to the FidelityFX SDK)
- [ ] [runtime/vulkan/fsr3/sdk-fixes/presenter-cpu-drain.json](runtime/vulkan/fsr3/sdk-fixes/presenter-cpu-drain.json) (C++ edits applied to the FidelityFX SDK)
- [ ] [runtime/vulkan/fsr3/sdk-fixes/swapchain-lifetime.json](runtime/vulkan/fsr3/sdk-fixes/swapchain-lifetime.json) (C++ edits applied to the FidelityFX SDK)
- [ ] [runtime/vulkan/fsr3/sdk-fixes/swapchain-trace.h](runtime/vulkan/fsr3/sdk-fixes/swapchain-trace.h)
- [ ] [runtime/vulkan/fsr3/sdk-fixes/swapchain-trace.json](runtime/vulkan/fsr3/sdk-fixes/swapchain-trace.json) (C++ edits applied to the FidelityFX SDK)
- [ ] [runtime/vulkan/fsr3/wsi-dispatch.h](runtime/vulkan/fsr3/wsi-dispatch.h)
- [ ] [runtime/vulkan/fsr4-fg/bridge.cpp](runtime/vulkan/fsr4-fg/bridge.cpp)
- [x] [runtime/vulkan/fsr4-fg/bridge_api.h](runtime/vulkan/fsr4-fg/bridge_api.h)
- [x] [runtime/vulkan/fsr4-fg/bridge_contract.h](runtime/vulkan/fsr4-fg/bridge_contract.h)
- [ ] [runtime/vulkan/fsr4-fg/bridge_platform.h](runtime/vulkan/fsr4-fg/bridge_platform.h)
- [ ] [runtime/vulkan/fsr4/bridge.cpp](runtime/vulkan/fsr4/bridge.cpp)
- [x] [runtime/vulkan/fsr4/bridge_api.h](runtime/vulkan/fsr4/bridge_api.h)
- [ ] [runtime/vulkan/fsr4/bridge_platform.h](runtime/vulkan/fsr4/bridge_platform.h)

**Fixtures (5)**

- [x] [tests/native_fsr4_fg_bridge.cpp](tests/native_fsr4_fg_bridge.cpp)
- [x] [tests/native_fsr4_fg_contract.cpp](tests/native_fsr4_fg_contract.cpp)
- [x] [tests/native_fsr4_fg_state.cpp](tests/native_fsr4_fg_state.cpp)
- [x] [tests/native_sr_fsr4_state.cpp](tests/native_sr_fsr4_state.cpp)
- [x] [tests/native_sr_fsr4_vulkan.cpp](tests/native_sr_fsr4_vulkan.cpp)

**Rust review findings (13)**

- [x] **Duplication | Medium | Compute DXGI present-count deltas the same way on both FSR paths.** The Vulkan/D3D12 FSR4 bridge always subtracts the previous count with wrapping arithmetic, while the D3D12 generation owner at runtime/orchestrator/src/d3d12_generation/fsr.rs:684 distinguishes a low-valued counter reset from a rollover. Both native callbacks read IDXGISwapChain::GetLastPresentCount (runtime/vulkan/fsr4-fg/bridge.cpp:340 and runtime/backends/fsr/src/fsr_generation.cpp:287). A decrease from 100 to 1 therefore reports over four billion presents on the bridge and one on the D3D12 path. Use one tested delta function with explicit reset and rollover cases when publishing presentation statistics. [runtime/orchestrator/src/fsr4_fg_bridge.rs:653](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/fsr4_fg_bridge.rs#L653)

- [x] **Duplication | Medium | Share the external-image input copy sequence between Vulkan SR and FG.** FSR4 SR emits producer-to-transfer-source, discard-or-acquire shared target, copy, restore producer and release target to external GENERAL. Vulkan-to-DX12 FG builds the same five steps in runtime/orchestrator/src/vulkan_dx12_fg.rs:384 for prepare at :978 and HUD-less at :1012. SR computes access/family masks separately in runtime/orchestrator/src/native_sr_fsr4_vulkan.cpp:105, while FG qualifies the transfer for a format-converting blit and uses equal families for the first target barrier. Use one tested planner for these external copies, retaining explicit format-conversion and first-use family choices and leaving SR output continuation separate. [runtime/orchestrator/src/vulkan_fsr4_sr.rs:258](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_fsr4_sr.rs#L258)

- [x] **Altitude | Medium | Reset FSR4 SR history across a missing source and record the decision once.** The split owner accepts any frame_id greater than last_frame but derives dispatch.reset only from history.valid, the explicit reset flag and resource generation. If a frame is skipped without an explicit reset, the SDK reuses older history as consecutive. record_inputs also writes only the incoming flag, so captures can disagree with the first-frame or generation-change reset sent to the SDK. Compute one continuity decision against the last successful source and use it for input captures, SDK dispatch and output metadata. [runtime/orchestrator/src/vulkan_fsr4_sr.rs:373](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_fsr4_sr.rs#L373)

- [x] **Altitude | Medium | Retain the standalone FSR3 swapchain when creation rollback refuses destruction.** After wsi_create succeeds, a failed replacement-function query or incomplete table calls wsi_destroy and discards its result before wsi_release frees the capsule. The explicit destroy operation keeps that same capsule on failure. Preserve a cleanup owner or return a documented retry handle on this constructor path, and exercise a failed rollback in the fake platform. [runtime/vulkan/fsr3/owner.rs:673](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/vulkan/fsr3/owner.rs#L673)

- [x] **Altitude | Medium | Retire the old interpolation link before replacing an FSR3 swapchain.** Fsr3SwapchainRuntime.Recreate forwards directly to this operation. It replaces the swapchain's handle while the paired FgContext still refers to the old chain. The in-file fixture demonstrates that disable and destroy then refuse until another frame evaluates, so closing immediately after resize has no cleanup path. Disable, drain and unlink the old pair before recreation, or keep both generations explicitly retireable. [runtime/vulkan/fsr3/owner.rs:710](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/vulkan/fsr3/owner.rs#L710)

- [x] **Duplication | Medium | Avoid a second wait for the same standalone FSR3 present queue during retirement.** For a matching linked chain, prepare_destroy calls fg_disable at :808, which waits for queued presents at :1014 and clears UI registration. prepare_destroy then issues the same wait again at :813; full destruction at :883 and :888 repeats this sequence. The in-file fixture at :1942 explicitly expects both calls, and runtime/vulkan/fsr3/bridge.cpp:229 dispatches the SDK WaitForPresents operation for each. Return a drained-chain result from the checked unlink operation and reuse it within the serialized retirement attempt, retaining the independent wait when no matching chain was drained. [runtime/vulkan/fsr3/owner.rs:813](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/vulkan/fsr3/owner.rs#L813)

- [x] **Altitude | Medium | Enforce exclusive interpolation ownership on both sides of the FSR3 pairing.** Proxy evaluation checks the incoming FgContext's former swapchain but unconditionally overwrites the destination swapchain's active_fg. Two independently created FG contexts can therefore both retain the same swapchain pointer. Retiring the presenter unlinks only the last context, leaving the first with a dangling owner for fg_disable. Reject an occupied destination or retire its existing context before installing the new pair, and exercise two contexts in the fixture. [runtime/vulkan/fsr3/owner.rs:1103](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/vulkan/fsr3/owner.rs#L1103)

- [x] **Duplication | Medium | Generate the FSR4 SR bridge call records for both Rust owners.** The standalone SR bridge hand-declares BridgeCreate, BridgeFrame and BridgeInfo at :68, :84 and :110 and consumes them in create, evaluate and info at :774, :968 and :822. Across the same C ABI the orchestrator sends the generated layouts from runtime/orchestrator/src/vulkan_fsr4_sr_abi.rs:13, :60 and :143 through runtime/orchestrator/src/vulkan_fsr4_sr.rs:98 and :424. The standalone assertions at :226 check only selected offsets, so a same-size field swap can survive there. Generate the three dependency-free records in one include usable by both builds and keep the different legacy managed frame local. [runtime/vulkan/fsr4/owner.rs:68](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/vulkan/fsr4/owner.rs#L68)

- [x] **Altitude | Low | Populate the FSR4 Present status from every outcome of the bridge operation.** rsf_core_fsr4fg_present initializes status.result to success and Bridge::present fills it only after the normal submission path. Validation, wait, command recording and terminal failures therefore return an error alongside a status claiming success. vulkan_fsr4_fg.rs:289 passes both results onward and vulkan_dx12_fg.rs:1235 reconstructs the failure by preferring the return code. Finalize the valid status structure in the bridge entry point on all exits so direct bridge callers receive a coherent snapshot. [runtime/orchestrator/src/fsr4_fg_bridge.rs:923](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/fsr4_fg_bridge.rs#L923)

- [x] **Duplication | Low | Convert FidelityFX runtime directories once for the Vulkan bridge owners.** Both FG probe and create (:73 and :127) reserve a 32768-unit stack buffer and call the conversion leaf at runtime/orchestrator/src/native_fg_fsr4_vulkan.cpp:11. SR create at runtime/orchestrator/src/vulkan_fsr4_sr.rs:479 instead validates UTF-8, reserves an owned vector, encodes UTF-16 and appends NUL for the same bridge input. Use one fallible helper producing an owned terminated UTF-16 directory, specify the shared length and empty-path contract, and map allocation and invalid-input errors at each public API. FG's 64-KiB stack buffers and conversion-only native callback can then be removed. [runtime/orchestrator/src/vulkan_fsr4_fg.rs:73](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_fsr4_fg.rs#L73)

- [x] **Simplicity | Low | Use SDK binding presence to admit standalone FSR3 operations.** open stores the process-wide SDK exactly once, after all five exports resolve. swapchain_create still probes three separate fields of the all-null fallback, and fg_destroy documents an unreachable missing-destroy branch for a live context. Keep the fallback at rsf_core_fsr3_sdk for its native callers, use SDK.get() once in operations available before open, and use the bound table for live owners. Preserve the existing null-argument return codes and native has-context/has-effect checks. This makes the all-or-none invariant explicit without changing the C table layout. [runtime/vulkan/fsr3/owner.rs:612](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/vulkan/fsr3/owner.rs#L612)

- [x] **Duplication | Low | Allocate D3D12 local completion values consistently in the FSR4 bridges.** The FSR4 SR bridge increments its local fence after submitting GPU work at :659 and again on the recovery path at :695, then treats UINT64_MAX as device removal when waiting at :492. Its FG counterpart at runtime/orchestrator/src/fsr4_fg_bridge.rs:445 explicitly refuses that reserved value before signalling. Share a checked local completion-value allocator across the two bridges and use it on normal and recovery signals, leaving their separate shared-fence ordering and error-retention policies intact. [runtime/vulkan/fsr4/owner.rs:659](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/vulkan/fsr4/owner.rs#L659)

- [x] **Simplicity | Low | Build the FSR4 dispatch directly from the caller frame.** The public evaluation copies textures, dimensions, camera and fence values into LegacyFrame, passes motion scale and pre-exposure separately, then submit copies them again into Dispatch. Let each ABI entry construct the dispatch plus ready/done values once, and have the shared submit step fill slot and sharpness after allocator admission. Preserve public zero-far conversion, legacy delta and exposure defaults, and the recovery signal on refusal. This keeps two small input conversions at the ABI boundary and removes a temporary record from every public evaluation. [runtime/vulkan/fsr4/owner.rs:987](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/vulkan/fsr4/owner.rs#L987)

<a id="v06"></a>

### V06 · XeSS Vulkan bridges

Port XeSS Vulkan requirements, frame generation and present/capture handling for the integrated and standalone bridge.

**Coordinate shared edits**

- Capture publication after a submitted command or queue is retired ([G07](#g07), [V02](#v02), [V04](#v04), [V05](#v05)).
- Shared Vulkan resource state, provider selection and queue retirement ([V01](#v01), [V02](#v02), [V03](#v03), [V04](#v04), [V05](#v05)).

**Port files (12)**

- [x] [runtime/orchestrator/include/rescaleframe/native_xess_vulkan_requirements.h](runtime/orchestrator/include/rescaleframe/native_xess_vulkan_requirements.h)
- [ ] [runtime/orchestrator/src/native_fg_xess_vulkan.cpp](runtime/orchestrator/src/native_fg_xess_vulkan.cpp)
- [x] [runtime/orchestrator/src/native_fg_xess_vulkan.h](runtime/orchestrator/src/native_fg_xess_vulkan.h)
- [x] [runtime/orchestrator/src/native_fg_xess_vulkan_platform.h](runtime/orchestrator/src/native_fg_xess_vulkan_platform.h)
- [ ] [runtime/orchestrator/src/native_fg_xess_vulkan_present.cpp](runtime/orchestrator/src/native_fg_xess_vulkan_present.cpp)
- [ ] [runtime/orchestrator/src/native_xess_vulkan.cpp](runtime/orchestrator/src/native_xess_vulkan.cpp)
- [ ] [runtime/orchestrator/src/native_xess_vulkan.h](runtime/orchestrator/src/native_xess_vulkan.h)
- [ ] [runtime/vulkan/xess-fg/bridge.cpp](runtime/vulkan/xess-fg/bridge.cpp)
- [x] [runtime/vulkan/xess-fg/bridge_api.h](runtime/vulkan/xess-fg/bridge_api.h)
- [x] [runtime/vulkan/xess-fg/capture_api.h](runtime/vulkan/xess-fg/capture_api.h)
- [ ] [runtime/vulkan/xess-fg/diagnostic-capture.h](runtime/vulkan/xess-fg/diagnostic-capture.h)
- [x] [runtime/vulkan/xess-fg/session_abi.h](runtime/vulkan/xess-fg/session_abi.h)

**Fixtures (3)**

- [x] [tests/native_fg_xess_vulkan_interop.cpp](tests/native_fg_xess_vulkan_interop.cpp)
- [x] [tests/native_fg_xess_vulkan_present.cpp](tests/native_fg_xess_vulkan_present.cpp)
- [ ] [tests/native_xess_vulkan.cpp](tests/native_xess_vulkan.cpp)

**Rust review findings (7)**

- [x] **Duplication | Medium | Share the public XeSS FG Present record and status between Rust producer and consumer.** XessSubmission::present at :450 fills BridgeFrame and reads BridgeStatus. The selected callback at runtime/orchestrator/src/native_fg_xess_vulkan_present.cpp:20 forwards both unchanged at runtime/vulkan/xess-fg/bridge.cpp:943 to runtime/vulkan/xess-fg/session.rs:950, which decodes separate manual IntegratedFrame and IntegratedStatus declarations at :60 and :118. Both sides maintain the same matrices, resources, fence ordering and result fields from runtime/vulkan/xess-fg/bridge_api.h:35. Generate these two public records once for both Rust builds and retain the conversion to the distinct legacy frame with its extra UI resource. [runtime/orchestrator/src/vulkan_xess_fg.rs:54](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/orchestrator/src/vulkan_xess_fg.rs#L54)

- [x] **Duplication | Medium | Use the generated GameSDK frame layout for XeSS diagnostic capture.** Capture::source at :327 copies a hand-written 480-byte frame layout with seven unnamed identity words and a 420-byte tail. RsfXessBridgeCaptureSource at runtime/vulkan/xess-fg/bridge.cpp:472 passes the actual GameSDK struct generated at sdk/game/src/lib.rs:157, and native publication expects that complete struct inside the readback record at runtime/vulkan/xess-fg/diagnostic-capture.h:33. Extract a dependency-free generated frame/camera binding for both builds or generate this projection from the header, preserving opaque padding while removing hand-maintained field positions and tail size. [runtime/vulkan/xess-fg/capture.rs:50](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/vulkan/xess-fg/capture.rs#L50)

- [x] **Boundaries | Medium | Give environment-only diagnostic options overlay controls.** AGENTS.md Rule 2 requires every option the runtime or an integration reads, diagnostic ones included, to have a live overlay control that saves it. These Rust owners read options only from process environment variables at construction and expose no overlay-service option: XeSS-FG capture reads RSF_VULKAN_FRAME_CAPTURE_DIR, RSF_FRAME_CAPTURE_FIRST, RSF_FRAME_CAPTURE_COUNT and RSF_XESS_DIAGNOSTIC_PROBE (capture.rs:242-257); the shared capture config reads the same first/count pair (runtime/orchestrator/src/frame_capture_vk.rs:131-141); D3D12 capture reads RSF_FRAME_CAPTURE_DIR and RSF_FRAME_CAPTURE_RESET_SR (runtime/orchestrator/src/frame_capture12.rs:122-126); Vulkan timing reads RSF_VULKAN_TIMING_FILE (runtime/orchestrator/src/vulkan_timing.rs:101); AC7 reads RSF_AC7_PIXEL_DIAGNOSTICS (games/ac7/src/renderer.rs:84). Each requires restarting the game with a changed environment to try, which Rule 2 exists to avoid, and none appears in the panel. Move these into the shared settings store and declare them through the overlay service (runtime-owned ones as runtime options, the AC7 flag as an adapter host option), with a restart-required flag where an owner truly reads once, and keep the environment only as an override if still needed. [runtime/vulkan/xess-fg/capture.rs:242](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/vulkan/xess-fg/capture.rs#L242)

- [x] **Duplication | Medium | Use one timeline-pair admission rule before queuing a failed bridge handoff.** Legacy XeSS Present at runtime/vulkan/xess-fg/bridge.cpp:939 reaches present_frame, which rejects ready == 0 at session.rs:619, yet the separate recovery guard at :715 accepts ready=0 and a fresh positive done. recover_handoff then queues a wait for zero and signals that done value despite lacking producer readiness. The FSR4 SR recovery at runtime/vulkan/fsr4/owner.rs:681 and FG handoff at runtime/orchestrator/src/fsr4_fg_bridge.rs:462 both reject zero ready explicitly. Extract a checked recovery-pair predicate and use it across the bridges, preserving their own error codes and valid failed-frame release paths. [runtime/vulkan/xess-fg/session.rs:715](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/vulkan/xess-fg/session.rs#L715)

- [x] **Altitude | Medium | Attach XeSS timing samples and denominators to their presenting session.** Sessions and the native GPU callback accumulate into one static batch, which survives destruction or recreation. Present phases are recorded before handoff, ID, Present and status errors are returned, but end_present increments the denominator only on success. Failed work therefore raises the next successful batch's average, potentially in a different session. Pass a session-owned timing accumulator to the callback and count each phase's accepted samples, or commit a complete per-frame measurement with its result. [runtime/vulkan/xess-fg/timing.rs:140](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/vulkan/xess-fg/timing.rs#L140)

- [x] **Simplicity | Low | Read each XeSS capture setting independently.** read_variable returns true for 32 or more UTF-16 units without changing the buffer. Config reuses that buffer for first frame, count and probe. For example, a count of 1 followed by an overlong probe setting enables probe-only mode and suppresses readbacks. Return an optional bounded value per lookup, preserving the numeric parser, count clamp and exact probe match for supported inputs. Treat oversized values like missing values so each setting uses its own default. This removes dependence on the preceding setting and changes only malformed-input handling. [runtime/vulkan/xess-fg/capture.rs:249](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/vulkan/xess-fg/capture.rs#L249)

- [x] **Simplicity | Low | Describe an aliased HUD-less fallback texture once.** The shared Vulkan presenter passes buffer.shared.dx12_resource for both color and HUD-less when generation is disabled. present_resources_match describes each separately to compare it to the same expected BGRA8 extent; record_present_inputs already handles the alias when issuing transitions. Validate color once, then accept HUD-less immediately on pointer equality or describe and validate it when distinct. One pointer comparison removes a repeated native texture query on pass-through frames without changing the resource-role tags. [runtime/vulkan/xess-fg/session.rs:531](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/runtime/vulkan/xess-fg/session.rs#L531)

## Games

<a id="a01"></a>

### A01 · AC7 carrier and renderer lifecycle

Port startup, renderer admission and retirement, including how a refused install is reported.

**Coordinate shared edits**

- Carrier entry, game recognition and existing forwarding exports ([L01](#l01), [W01](#w01)).

**Port files (8)**

- [x] [games/ac7/include/rescaleframe/ac7_carrier.h](games/ac7/include/rescaleframe/ac7_carrier.h)
- [x] [games/ac7/include/rescaleframe/ac7_hooks.h](games/ac7/include/rescaleframe/ac7_hooks.h)
- [x] [games/ac7/include/rescaleframe/ac7_memory.h](games/ac7/include/rescaleframe/ac7_memory.h)
- [x] [games/ac7/include/rescaleframe/ac7_native_renderer.h](games/ac7/include/rescaleframe/ac7_native_renderer.h)
- [x] [games/ac7/src/ac7_platform.h](games/ac7/src/ac7_platform.h)
- [x] [games/ac7/src/ac7_sites.h](games/ac7/src/ac7_sites.h)
- [ ] [games/ac7/src/native_renderer.cpp](games/ac7/src/native_renderer.cpp)
- [x] [games/ac7/src/plugin.cpp](games/ac7/src/plugin.cpp)

**Fixtures (2)**

- [ ] [tests/ac7_native_graph.cpp](tests/ac7_native_graph.cpp)
- [x] [tests/ac7_session.cpp](tests/ac7_session.cpp)

**Rust review findings (9)**

- [x] **Boundaries | Medium | Expose the AC7 carrier's startup toggles as adapter host options.** rsf_ac7_carrier_install_decrypted reads RSF_ENABLE_JITTER, RSF_JITTER_RVA, RSF_JITTER_ALL_VIEWS, RSF_TRANSLUCENT_VELOCITY(_RVA), RSF_FULL_TRANSLUCENCY(_RVA) and RSF_MOTION_CAPTURE (carrier.rs:278-316), and rsf_ac7_carrier_override_start_extent reads RSF_DLSS_OUTPUT_WIDTH/HEIGHT (:367-379), through the proxy's read_number, which only consults the process environment and the proxy INI (loader/proxy/src/dinput8_proxy.c:241-280). They are read once when the decrypted image is installed and are not published as AC7 host options, so the overlay neither shows nor saves them. AGENTS.md Rule 2 requires every option an integration reads to have a live overlay control declared through the overlay service host_options. Declare these as AC7 adapter host options (flagged restart-required where a patch is only applied at install) and persist them through the shared settings store; keep the RVA overrides as developer-only if they must remain. [games/ac7/src/carrier.rs:278](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/carrier.rs#L278)

- [x] **Altitude | Medium | Pass the admitted Present extent into automatic startup.** on_present passes rsf_bridge_present_extent to try_auto_start. Rust reads the dimensions, discards them, consumes STARTUP_ATTEMPTED and invokes a parameterless callback. That callback, start_dlss (dinput8_proxy.c:1135), reads observer status again and can refuse after the attempt has been spent. Pass the admitted extent to a start-at-extent operation. The manual start path can obtain its own snapshot before calling the same operation. [games/ac7/src/carrier.rs:357](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/carrier.rs#L357)

- [x] **Duplication | Medium | Share the GameSDK call-header validator across adapters.** AC7 prepare validates both args and host at games/ac7/src/lib.rs:219 and :224. Fallout 4 repeats the same unsafe two-word check at games/fallout4/src/session.rs:79 for its prepare at :355 and :360. Unity Mono performs the same reads at games/unity-mono/src/lifecycle.rs:115 but returns Result to its prepare at :153. A change to short-header handling or error precedence currently needs independent changes across plugins. Put one validator for known versioned GameSDK call types beside the Rust SDK bindings, read size before version, and preserve each exported result code and the separate size-only probe contract. [games/ac7/src/lib.rs:170](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/lib.rs#L170)

- [x] **Altitude | Medium | Distinguish a prepared renderer from one retained only for cleanup.** When hook installation fails with a guard still held, renderer::prepare returns a pinned, quiescing renderer and false. Lifecycle::prepare saves the pointer but drops the preparation result, so GameSDK status reports prepared=1 and start forwards the request to a renderer that must refuse it. The refusals_match_the_former_plugin test explicitly expects that status. Track a cleanup-only state separately, retain its handle for stop, and report prepared and admit start only after successful preparation. [games/ac7/src/lifecycle.rs:223](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/lifecycle.rs#L223)

- [x] **Simplicity | Low | AC7 Ops re-unwraps leaves that publish already proved present.** `Ops(rsf_ac7_platform)` keeps the five `Option` leaves after `Ops::complete` (lib.rs:117) has required all of them, yet every `Leaves` method repeats `self.0.<leaf>.ok_or(Threw)?` (lib.rs:131-158), mapping an impossible missing leaf onto the native-exception result. Build `Ops` once from the table with plain fn pointers (`struct Ops { prepare: PrepareFn, start: StartFn, ... }`, constructed by `Some(Ops { prepare: table.prepare?, ... })` in `rsf_ac7_publish_api`) and drop `complete` and the five `ok_or` branches. Behavior is unchanged because publish refuses an incomplete table before `OPS` is set; the tradeoff is one small named struct instead of reusing the bindgen table type. [games/ac7/src/lib.rs:107](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/lib.rs#L107)

- [x] **Duplication | Low | Reuse the AC7 synthetic image setup in renderer, site and patch tests.** Renderer Fake::new plants the hook, helper and depth bytes at games/ac7/src/renderer/tests.rs:48 exactly as site admission tests do at games/ac7/src/sites/tests.rs:12. It then seeds velocity and scene precision as games/ac7/src/patches/tests.rs:21 does at a nonzero module base. This duplicates which tables make up each admissible test image, so adding a guard family requires editing both admission and transaction fixtures. Put two base-aware seed methods on the existing test-only fake_code::Image. Compose both for renderer tests and just the relevant one for the narrower suites, keeping independent expected-RVA and failure assertions. [games/ac7/src/renderer/tests.rs:48](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/renderer/tests.rs#L48)

- [x] **Simplicity | Low | AC7 NativeCode keeps validated Option leaves and re-checks them per call.** `NativeCode::new` (sites.rs:34) only returns `Some` when `matches` and `write` are present, but `NativeCode` still wraps the raw `&rsf_ac7_code_leaves`, so `Code::matches` and `Code::write` (sites.rs:41-52) repeat `let Some(..) = self.0.matches else { return false }` on every compare and write. renderer.rs compounds it: `NativeLeaves` stores a copied `rsf_ac7_code_leaves` with the two leaves wrapped back into `Some(...)` (renderer.rs:328-333) and `NativeLeaves::code()` rebuilds `NativeCode::new(&self.code)` as an `Option` for every `matches`/`write`/`log` (renderer.rs:387-408). Store the unwrapped pointers once (`struct NativeCode { matches: MatchesFn, write: WriteFn, log: rsf_game_log_fn, user: *mut c_void }`) and let `NativeLeaves` hold a `NativeCode` directly, with `log_to` setting its log/user. This removes three dead failure branches per call path and the per-call Option reconstruction without changing refusals, since both constructors still reject incomplete tables. Same pattern as the AC7 Ops entry for lib.rs. [games/ac7/src/sites.rs:27](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/sites.rs#L27)

- [x] **Altitude | Low | Make exact-build admission accept a read-only code view.** rsf_ac7_build_admit only compares live bytes and logs mismatches, but NativeCode::new rejects its callback table unless a write function is present. The actual Rust ABI test at sites/tests.rs:119 supplies a dummy refusing writer and explicitly asserts that removing it fails admission. Separate read/compare capability from the patch writer, validate only the callbacks used at each entry point and keep mandatory write validation on mutation paths. [games/ac7/src/sites.rs:36](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/sites.rs#L36)

- [x] **Duplication | Low | Check the AC7 target initializer fingerprint once during admission.** Target initialization serves two roles: native_renderer.cpp calls it at games/ac7/src/native_renderer.cpp:2110 and HOOK_SITES also identifies it as hook 23 at games/ac7/src/sites.rs:143. HELPERS repeats exactly the same address and 16 bytes at :216. admit walks both arrays at :256 and :276 before installing hooks, so every successful preparation makes the same guarded read twice. The assertion at games/ac7/src/sites/tests.rs:128 counts table lengths and therefore accepts the duplication. Keep the entry in the hook table, document the direct call there, and check uniqueness across both tables in the admission test. [games/ac7/src/sites.rs:216](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/sites.rs#L216)

<a id="a02"></a>

### A02 · AC7 render scopes, views and scene effects

Port scoped render commands, view data, UI, scene colour, contact shadows and TrueSky resource handling.

**Coordinate shared edits**

- Borrowed view and constant-buffer identity across render, capture and velocity ([A03](#a03), [A05](#a05), [G02](#g02)).

**Port files (15)**

- [x] [games/ac7/include/rescaleframe/ac7_render_scope.h](games/ac7/include/rescaleframe/ac7_render_scope.h)
- [x] [games/ac7/include/rescaleframe/ac7_scene_color.h](games/ac7/include/rescaleframe/ac7_scene_color.h)
- [x] [games/ac7/include/rescaleframe/ac7_ui_rules.h](games/ac7/include/rescaleframe/ac7_ui_rules.h)
- [x] [games/ac7/include/rescaleframe/ac7_view.h](games/ac7/include/rescaleframe/ac7_view.h)
- [x] [games/ac7/src/ac7_scope_owner.h](games/ac7/src/ac7_scope_owner.h)
- [x] [games/ac7/src/cloud_depth_platform.h](games/ac7/src/cloud_depth_platform.h)
- [x] [games/ac7/src/contact_shadow.cpp](games/ac7/src/contact_shadow.cpp)
- [x] [games/ac7/src/contact_shadow.h](games/ac7/src/contact_shadow.h)
- [ ] [games/ac7/src/render_scope.cpp](games/ac7/src/render_scope.cpp)
- [x] [games/ac7/src/scene_color.cpp](games/ac7/src/scene_color.cpp)
- [x] [games/ac7/src/truesky_depth.cpp](games/ac7/src/truesky_depth.cpp)
- [x] [games/ac7/src/truesky_depth.h](games/ac7/src/truesky_depth.h)
- [x] [games/ac7/src/truesky_motion.cpp](games/ac7/src/truesky_motion.cpp)
- [x] [games/ac7/src/truesky_motion.h](games/ac7/src/truesky_motion.h)
- [x] [games/ac7/src/ui_rules.cpp](games/ac7/src/ui_rules.cpp)

**Fixtures (4)**

- [x] [tests/ac7_render_scope.cpp](tests/ac7_render_scope.cpp)
- [x] [tests/ac7_ui_rules.cpp](tests/ac7_ui_rules.cpp)
- [x] [tests/ac7_view.cpp](tests/ac7_view.cpp)
- [x] [tests/ac7_view_fixture.h](tests/ac7_view_fixture.h)

**Rust review findings (8)**

- [x] **Duplication | Medium | Share fallible allocation for Rust-owned FFI handles.** AC7 allocates and initializes nonzero-sized Scopes and Ticket objects at games/ac7/src/render_scope.rs:286 and :346. Vulkan FSR3 repeats the unsafe allocation at runtime/vulkan/fsr3/owner.rs:331 for swapchains and FG contexts at :640 and :946, returning NonNull instead of a raw pointer. RDR2 capture creation at games/rdr2/src/capture_state.rs:112 uses a third generic implementation at :117 that omits the nonzero-size guard. Current callers allocate concrete nonzero types. Use one small fallible allocation helper returning Option<Box<T>>, with one documented size contract, and convert to raw handles and existing error codes in the owning exports. This removes three separate implementations of the allocator preconditions. [games/ac7/src/render_scope.rs:135](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/render_scope.rs#L135)

- [x] **Altitude | Medium | Queue point-in-time scope markers in one operation.** retire_renderer_uniforms (native_renderer.cpp:814-822), the native SR submission at line 982 and velocity finalization at line 1716 open and immediately close a leased ticket. No engine work occurs between those calls. Open transfers ownership and increments outstanding, but close can refuse after the first marker was published. These callers then discard the open ticket, preventing idle and lease retirement. Add an enqueue operation for these callers that admits and links both markers together before transferring ownership. Keep separate open/close for scopes spanning genuine engine work. [games/ac7/src/render_scope.rs:380](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/render_scope.rs#L380)

- [x] **Altitude | Medium | Bind each queued scope ticket to its originating command list.** The ticket stores its owner and markers but omits the native list used by open. Full-frame begin and end arrive in separate hooks (native_renderer.cpp:575-598) and end supplies its own list. Close accepts any recording list, even though the ticket lifetime assumes that both markers execute in order. Store the recording-list identity in the ticket, validate it on close and preserve the ticket for retirement on mismatch. [games/ac7/src/render_scope.rs:394](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/render_scope.rs#L394)

- [x] **Altitude | Medium | Track cloud motion relay installation independently of the full-grid transition.** The per-sky-render apply callback returns immediately once cloud resolution is installed or refused. install_motion returns no outcome and may leave its slot empty after a transient allocation or write refusal. If depth installation reaches either terminal state, no later sky render retries the independent composite draw observer, so the renderer can keep running without cloud motion depth. Give motion its own installed, retryable or refused outcome, and use the full-grid state only to gate the depth and resolution work. [games/ac7/src/truesky.rs:353](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/truesky.rs#L353)

- [ ] **Altitude | Medium | Report a full-target view separately from player-camera identity.** read_view labels a zero-origin view filling its own buffer as is_main_view, and ac7_view.h describes that as the camera the player sees. The compatibility bridge treats it as camera admission at dlss_bridge.c:914, yet lines 2359-2367 document a separate translucency upload passing the same test and previously causing every evaluation to fail. Expose the actual rectangle fact, and have the adapter establish primary-camera identity from the corresponding scene pass, view and family before selecting backend inputs. [games/ac7/src/view.rs:177](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/view.rs#L177)

- [x] **Duplication | Low | Reuse the compile-time ASCII widening used by the Windows adapters.** TrueSky converts its NUL-terminated module name at games/ac7/src/truesky.rs:40 using the same byte-to-u16 loop that games/ac7/src/carrier.rs:27 uses for DLL, executable and window-class pointers. Godot repeats it at integrations/godot/native/window.rs:29 for a length-delimited class comparison at :108. Share one const helper for ASCII bytes which preserves exactly the input length, including a terminator only when supplied, and keep the names in their owning integrations. This gives all three callers the same ASCII and length contract. [games/ac7/src/truesky.rs:29](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/truesky.rs#L29)

- [x] **Duplication | Low | Reuse one IEEE CRC-32 implementation for binary fingerprints.** AC7 checks its effect bytes at games/ac7/src/truesky.rs:168 using a slice fold. RDR2 computes the same reflected polynomial, initial value and final complement in games/rdr2/src/capture_state/recordings.rs:180; games/rdr2/src/native_capture.cpp:104 calls that loop for both pixel and vertex shader bytecode. Fixes or faster implementations currently need to preserve identical persisted hash values in two places. Share a pure crc32_ieee(&[u8]) function with the existing checksum utilities, retaining the RDR2 pointer/length contract and AC7 file-size check in their adapters. The existing CRC-32C function uses a different polynomial. [games/ac7/src/truesky.rs:159](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/truesky.rs#L159)

- [x] **Duplication | Low | Generate the numeric draw and layout classes consumed across the AC7 UI ABI.** The seven draw and four layout result values at games/ac7/src/ui.rs:7 and :15 repeat the cast macros at games/ac7/include/rescaleframe/ac7_ui_rules.h:33 and :72. Although tools/generate-rust-bindings.py:113 requests both families, games/ac7/src/ui_abi.rs:19 emits only their u32 aliases. The classifier returns its private numbers and the native dispatch at loader/proxy/src/dlss_bridge.c:534 switches on the header values; changing one side can silently route a different layout. Express those values as bindgen-readable integer constants while retaining the u32 return ABI, regenerate, and consume the generated constants in the classifiers. [games/ac7/src/ui.rs:7](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/ui.rs#L7)

<a id="a03"></a>

### A03 · AC7 F9 motion capture

Port the F9 capture boundary and reconcile the observation, record, session and artifact findings.

**Coordinate shared edits**

- Borrowed view and constant-buffer identity across render, capture and velocity ([A02](#a02), [A05](#a05), [G02](#g02)).

**Port files (6)**

- [x] [games/ac7/include/rescaleframe/ac7_motion_capture.h](games/ac7/include/rescaleframe/ac7_motion_capture.h)
- [x] [games/ac7/src/motion_capture.cpp](games/ac7/src/motion_capture.cpp)
- [x] [games/ac7/src/motion_capture_observations.h](games/ac7/src/motion_capture_observations.h)
- [x] [games/ac7/src/motion_capture_records.h](games/ac7/src/motion_capture_records.h)
- [x] [games/ac7/src/motion_capture_session.h](games/ac7/src/motion_capture_session.h)
- [ ] [games/ac7/src/motion_capture_trampolines.cpp](games/ac7/src/motion_capture_trampolines.cpp) (typed Unreal calls, SEH and native callbacks)

**Fixtures (2)**

- [x] [tests/ac7_capture_io.cpp](tests/ac7_capture_io.cpp)
- [x] [tests/ac7_motion_capture.cpp](tests/ac7_motion_capture.cpp)

**Rust review findings (7)**

- [x] **Duplication | Medium | Use the observer binding for AC7 input-layout elements.** The observer publishes the generated rsf_observer_layout_element at runtime/orchestrator/src/d3d11_observer_abi.rs:12 and :40. The native capture callback passes that exact pointer through at games/ac7/src/motion_capture.cpp:886, but the Rust consumer at games/ac7/src/motion_capture_observations.rs:234 interprets it as a handwritten five-field struct. The velocity cache also takes this mirror at games/ac7/src/velocity_pass_cache.rs:348. Reordering two of the first four u32 fields could silently change capture and velocity classification while the manual size and last-offset assertions still pass. Generate the narrow observer element binding once in a shared ABI module and import it on both sides, leaving snapshot ownership and cache limits local. [games/ac7/src/motion_capture_observations.rs:51](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/motion_capture_observations.rs#L51)

- [x] **Altitude | Medium | Bind the contact-light before/after pair to its draw.** begin_lighting_pair arms a single interval-wide flag before the before-draw record exists. draw_capture consumes it for either contact shader hash, without checking the originating context, draw or target. motion_capture.cpp:542 even omits the draw index on the lighting row. If two matching callbacks interleave, the second draw can consume the first draw's after sample, and the artifacts cannot establish which draw changed the pixels. Carry the same draw identity through both callbacks and complete only the matching recorded pair. [games/ac7/src/motion_capture_observations/colour.rs:130](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/motion_capture_observations/colour.rs#L130)

- [x] **Altitude | Medium | Snapshot shader observations before writing capture files.** Publisher::finish keeps the global observations mutex from before formatting engine records until after the final session.json write, including each of up to 4,096 shader file writes. The shader creation hook in motion_capture.cpp:869 needs the same mutex even between armed captures, so file I/O can block shader creation. Snapshot ordered shader handles, counters and readback totals under the lock, using shared immutable bytecode storage to avoid copying the payloads, then publish from that snapshot after releasing the lock. [games/ac7/src/motion_capture_records/artifacts.rs:427](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/motion_capture_records/artifacts.rs#L427)

- [x] **Duplication | Medium | Keep one AC7 site description for capture and renderer admission.** The F9 root table records the postprocess and widget queue addresses and guard bytes at games/ac7/src/motion_capture_records/root_hooks.rs:25 and :42. Renderer admission at games/ac7/src/renderer.rs:153 checks independently maintained entries at games/ac7/src/sites.rs:132 and :136. Standalone capture uses its table from games/ac7/src/motion_capture.cpp:397 and skips these hooks when the native owner is active. The addresses agree today, but the postprocess checks cover 18 versus 16 bytes and widget queue covers 15 versus 16. Define each address and longest verified byte sequence once in AC7, retaining the consumer-specific prefix lengths, ownership flags and detour order so a site update reaches both paths. [games/ac7/src/motion_capture_records/root_hooks.rs:25](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/motion_capture_records/root_hooks.rs#L25)

- [x] **Altitude | Medium | Retain root-hook ownership until removal succeeds.** Capture shutdown calls the void remove callback and unconditionally clears REGISTRATION. The actual callback (motion_capture.cpp:393, ac7_hooks.h:9) discards the results of both DisableHook and RemoveHook. An unsuccessful removal leaves no tracked target for a retry, and install similarly reports RolledBack without evidence of complete removal. Have the native batch return the targets still owned, preserve their registration until retired, and report partial teardown to the caller. [games/ac7/src/motion_capture_records/root_hooks.rs:176](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/motion_capture_records/root_hooks.rs#L176)

- [x] **Simplicity | Low | AC7 draw commit takes a fallible row closure that every caller pre-builds.** `commit(capture, row: impl FnOnce() -> Result<Bytes, fmt::Error>)` defers row construction until admission under the records lock, but all four callers (draws.rs:257, draws/before.rs:156, draws/geometry.rs:171, draws/ordinary.rs:178) format the row first and pass `|| Ok(row)`, so the closure, the inner `?` and the `fmt::Result` return are dead generality. Change it to `fn commit(capture, row: Bytes) -> bool` (admit, `try_reserve`, push) and have the exports return `write_x(...).is_ok() && commit(capture, row)`. Behavior is identical because formatting already happens before the lock; the only tradeoff is losing a lazy-build option nobody uses. [games/ac7/src/motion_capture_records/draws.rs:108](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/motion_capture_records/draws.rs#L108)

- [x] **Altitude | Low | Keep Windows stream formatting out of JSON sampler records.** write_lod reproduces unquoted Windows stream tokens such as inf and nan(ind) for nonfinite sampler limits. When a sampled descriptor contains one, the draw writer reports success but tools/analyze-ac7-motion-capture.py:16 rejects the row with json.loads and aborts analysis of the capture. Give the JSON float writer responsibility for nonfinite values, using null as write_float already does while preserving six significant digits for finite sampler LODs. [games/ac7/src/motion_capture_records/draws/before.rs:63](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/motion_capture_records/draws/before.rs#L63)

<a id="a04"></a>

### A04 · AC7 velocity shader transforms

Finish the shader container and stage transforms, keeping emitted bytecode and shader inputs traceable.

**Coordinate shared edits**

- Shader transform outputs consumed by the velocity pass ([A05](#a05)).

**Port files (4)**

- [x] [games/ac7/src/shader_bytes.h](games/ac7/src/shader_bytes.h)
- [x] [games/ac7/src/velocity_container.h](games/ac7/src/velocity_container.h)
- [x] [games/ac7/src/velocity_shader.cpp](games/ac7/src/velocity_shader.cpp)
- [x] [games/ac7/src/velocity_shader.h](games/ac7/src/velocity_shader.h)

**Fixtures (1)**

- [x] [tests/ac7_velocity_shader.cpp](tests/ac7_velocity_shader.cpp)

**Rust review findings (7)**

- [x] **Duplication | Medium | Share the GPU history hash, record layout and lookup across AC7 velocity shaders.** The native geometry helper compiled at games/ac7/src/velocity_geometry.rs:638 and the domain helper compiled at games/ac7/src/velocity_tessellation.rs:723 independently implement the same FNV mix, 32-probe limit, 64-byte record addressing and nearest unambiguous clip selection at velocity_geometry.rs:753 and games/ac7/src/velocity_tessellation.rs:760. The GPU table writer at games/ac7/src/velocity_pass.cpp:63 independently repeats the hash and field offsets. A one-sided change can turn populated records into misses that look like ordinary fallback motion. Package shared HLSL source for the hash, record offsets and common lookup, with the writer and all readers embedding it at compilation. Keep emission ordinals, tessellation corner keys, beam interpolation and the t0/t127 bindings with their respective stages. [games/ac7/src/velocity_geometry.rs:753](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/velocity_geometry.rs#L753)

- [x] **Duplication | Medium | Share the geometry and domain helper-instruction rewrite.** The domain program builds its helper instructions at games/ac7/src/velocity_tessellation.rs:646 using the same opcode filtering, output declaration rewrite, raw resource handling, temporary-register rebasing and output mapping as the native geometry transform at games/ac7/src/velocity_geometry.rs:433 and :555. Only the helper input operands have stage-specific meanings: domain point/corner/control point versus geometry vertex/temporary. A change to handling a declaration or an invalid register currently needs to be repeated. Put the common instruction loop in velocity_stage with a callback for input remapping, preserving each stage's separate placement of the rewritten code. [games/ac7/src/velocity_tessellation.rs:518](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/velocity_tessellation.rs#L518)

- [x] **Altitude | Medium | Keep intermediate shader signatures inside the Rust stage linker.** Renderer::make_variant passes the hull output signature directly to the domain transform (velocity_pass.cpp:246-248). Rust calls a native append callback per hull output, velocity_shader.cpp copies the names into owning C++ strings, then semantic_views constructs another vector and Rust reimports the same entries for domain linkage. Provide one tessellation-link operation returning both hull programs, the domain program and the final metadata needed for stream output. Only finished products need cross into the D3D owner. [games/ac7/src/velocity_tessellation.rs:656](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/velocity_tessellation.rs#L656)

- [x] **Altitude | Medium | Classify SM5 declarations consistently before splitting shader bodies.** Instruction.declaration covers only opcodes 88 through 106. velocity_geometry.rs recognizes native raw and structured resource declarations when checking slot 127, but native_declaration adds only stream and instance-count opcodes. A permitted native raw or structured resource on another slot goes through native_body after its initial executable MOV, so the resulting shader can fail D3D creation. Have the shared decoder classify the supported declaration opcodes and keep stage-specific phase placement in the stage linker. [games/ac7/src/velocity_tokens.rs:121](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/velocity_tokens.rs#L121)

- [x] **Duplication | Medium | Use the common DXBC stage reader and writer for AC7 vertex transforms.** The vertex transform calls a private Stage reader at games/ac7/src/velocity_vertex.rs:391 and reassembles its shader, ISGN and OSGN at :402. games/ac7/src/velocity_stage.rs:42 and :64 already implement the same typed stage read/write for the geometry path at games/ac7/src/velocity_geometry.rs:262. The vertex reader adds a 32-register limit, and the pixel writer at games/ac7/src/velocity_pixel.rs:380 preserves the original output signature. Use the common reader with a vertex-local register check and OSGN-only selection, then reuse write_stage for vertex. Factor chunk substitution beneath write_stage so the pixel path can replace only code and inputs. Keep unaffected chunks byte-identical and preserve allocation failures. [games/ac7/src/velocity_vertex.rs:135](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/velocity_vertex.rs#L135)

- [x] **Duplication | Low | Share traversal of the DXBC chunk offset table.** The AC7 velocity signature reader calls chunks at games/ac7/src/velocity_container.rs:200. Its read_container implementation validates the offset-table size, reads each offset at 32 + 4*i, computes the table end and delegates to chunk_at. The synthetic patch service called at runtime/orchestrator/src/shader_patch.rs:770 repeats the same traversal in runtime/orchestrator/src/shader_patch/dxbc.rs:47. Both already import runtime/graphics/src/dxbc_chunks.rs:14, but that helper checks only one chunk. Extend it with a checked iterator yielding offset and borrowed chunk from the declared container slice. Keep AC7's trailer acceptance and 32-chunk limit, and the service's exact-length, hash and refusal diagnostics at their call sites. [games/ac7/src/velocity_container.rs:114](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/velocity_container.rs#L114)

- [x] **Duplication | Low | Reuse the velocity-stage compiler callback adapter for pixel shaders.** The pixel transform called at games/ac7/src/velocity_shader.cpp:93 initializes pointer/size outputs, calls the compiler, checks the container length, and borrows the result at games/ac7/src/velocity_pixel.rs:417. games/ac7/src/velocity_stage.rs:130 already implements that exact pointer and lifetime contract for geometry and tessellation, including its use at games/ac7/src/velocity_geometry.rs:595. The pixel path then parses chunks, which can stay pixel-local. Import CompileHelper and call compile_bytecode for the pixel helper so all three paths apply future callback or size-contract changes together. [games/ac7/src/velocity_pixel.rs:417](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/velocity_pixel.rs#L417)

<a id="a05"></a>

### A05 · AC7 velocity pass and history

Port the velocity pass and consolidate variant publication, frame history and stream or commit decisions.

**Coordinate shared edits**

- Borrowed view and constant-buffer identity across render, capture and velocity ([A02](#a02), [A03](#a03), [G02](#g02)).
- Shader transform outputs consumed by the velocity pass ([A04](#a04)).

**Port files (7)**

- [x] [games/ac7/include/rescaleframe/ac7_velocity_pass.h](games/ac7/include/rescaleframe/ac7_velocity_pass.h)
- [x] [games/ac7/src/velocity_commit.h](games/ac7/src/velocity_commit.h)
- [x] [games/ac7/src/velocity_history.h](games/ac7/src/velocity_history.h)
- [x] [games/ac7/src/velocity_pass.cpp](games/ac7/src/velocity_pass.cpp)
- [x] [games/ac7/src/velocity_pass_cache.h](games/ac7/src/velocity_pass_cache.h)
- [x] [games/ac7/src/velocity_renderer.h](games/ac7/src/velocity_renderer.h)
- [x] [games/ac7/src/velocity_stream.h](games/ac7/src/velocity_stream.h)

**Rust review findings (7)**

- [x] **Altitude | Medium | Commit model record capacity after native allocation succeeds.** begin_model charges draw.records to the frame before Renderer::model (velocity_pass.cpp:324-331) attempts to grow the GPU history buffer. If raw_buffer fails, that draw returns without storing anything, but its charge survives. Later draws in the same frame may request larger allocations or hit RECORD_LIMIT for work that never ran. Split frame rotation from record reservation and finalize or cancel the reservation when native buffer preparation reports its outcome. [games/ac7/src/velocity_history.rs:335](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/velocity_history.rs#L335)

- [x] **Altitude | Medium | Apply the same view identity to velocity accumulation and commit.** begin_view returns SAME_FRAME solely from the native frame and the session/history lookup. Renderer::view (velocity_pass.cpp:345) then accepts a matching depth resource without comparing view_key or family_key. Two views sharing history, depth and frame can contribute to one motion target, but commit_view still attributes it exclusively to the first view/family. Include view and family in same-frame admission, or allocate separate accumulation slots for simultaneous views while retaining their shared temporal-history identity. [games/ac7/src/velocity_history.rs:337](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/velocity_history.rs#L337)

- [x] **Duplication | Medium | Retain one shader bytecode allocation for AC7 capture and velocity.** The same D3D11 creation callback passes the identical byte span to both consumers at loader/proxy/src/dlss_bridge.c:642 and :643. With capture enabled, games/ac7/src/motion_capture_observations.rs:141 copies the full payload for ordered F9 shader artifacts while games/ac7/src/velocity_pass_cache.rs:173 independently copies it for variant construction. Each cache permits 64 MiB of bytecode, so overlapping observations can occupy twice the payload memory. Fan out one fallibly allocated immutable payload to both consumers, keeping their separate retention budgets, invalidation, artifact ordinals and the full-span versus declared-container hash conventions. The variant builder can retain this payload during its unlocked callback instead of copying all stages again at velocity_pass_cache.rs:247. [games/ac7/src/velocity_pass_cache.rs:166](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/velocity_pass_cache.rs#L166)

- [x] **Altitude | Medium | Include beam-layout classification in velocity variant identity.** Renderer::variant passes g.strides[0] from the bound vertex buffer. The input-layout object can be rebound with a different stride. prepare caches successes and refusals before it computes beam_layout(request.stride, elements), even though make_variant passes that flag into the vertex shader transform. A first draw can therefore determine the wrong beam transformation or refusal for later draws. Compute the beam-layout decision before lookup and include it in VariantKey. [games/ac7/src/velocity_pass_cache.rs:234](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/velocity_pass_cache.rs#L234)

- [x] **Altitude | Medium | Preserve the reason for variant build failure before caching it.** admit records REFUSED as a permanent negative entry for the renderer generation. Renderer::make_variant (velocity_pass.cpp:279,293) returns that same result for unsupported bytecode and D3D shader or stream-output creation failures, including recoverable resource failures. A single failed allocation can thus suppress later eligible draws after the device recovers. Return distinct deterministic refusal and retryable native failure outcomes from the builder and cache only the former. [games/ac7/src/velocity_pass_cache.rs:320](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/velocity_pass_cache.rs#L320)

- [x] **Altitude | Medium | Reject a built slot when its renderer generation has retired.** record silently drops a late result when the native variant list has been reset, but admit still returns its slot as a successful result. reset_discards_slots_and_late_results explicitly expects slot zero in this case. Renderer::variant at velocity_pass.cpp:228 checks only the vector length, which cannot establish that a recycled index names the just-built variant. Validate generation before returning a slot and keep the owning renderer live until the draw has finished using it. [games/ac7/src/velocity_pass_cache.rs:323](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/velocity_pass_cache.rs#L323)

- [x] **Altitude | Medium | Invalidate the old shader identity even when its replacement is too large to capture.** dlss_bridge.c:642-643 forwards the same creation notification to the F9 observer and the velocity cache. Both wrappers return before invalidating the address when the new bytecode exceeds 1 MiB. If D3D reuses a same-stage shader address, later draws can select the prior object's velocity variant and F9 can report its prior hash. Treat object creation as an unconditional lifetime boundary, removing the previous observation before deciding whether to retain the replacement payload. [games/ac7/src/velocity_pass_cache.rs:337](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/ac7/src/velocity_pass_cache.rs#L337)

<a id="n01"></a>

### N01 · Anomaly startup and observation

Port bootstrap, profile admission and observer lifetime. Attribute each guarded read and callback to its owner.

**Coordinate shared edits**

- Anomaly session startup, quiesce and callback retirement ([N02](#n02), [N03](#n03), [N04](#n04), [N06](#n06), [N08](#n08)).

**Port files (19)**

- [ ] [games/anomaly/src/bootstrap_platform.cpp](games/anomaly/src/bootstrap_platform.cpp) (native profile and stage table)
- [x] [games/anomaly/src/bootstrap.cpp](games/anomaly/src/bootstrap.cpp)
- [ ] [games/anomaly/src/build_profile.h](games/anomaly/src/build_profile.h)
- [ ] [games/anomaly/src/engine_guards.h](games/anomaly/src/engine_guards.h)
- [x] [games/anomaly/src/engine_slots.h](games/anomaly/src/engine_slots.h)
- [ ] [games/anomaly/src/native_hook_ops.h](games/anomaly/src/native_hook_ops.h)
- [ ] [games/anomaly/src/native_hooks.cpp](games/anomaly/src/native_hooks.cpp)
- [ ] [games/anomaly/src/native_hooks.h](games/anomaly/src/native_hooks.h)
- [x] [games/anomaly/src/native_memory.h](games/anomaly/src/native_memory.h)
- [x] [games/anomaly/src/native_observer.h](games/anomaly/src/native_observer.h)
- [ ] [games/anomaly/src/native_observer_platform.cpp](games/anomaly/src/native_observer_platform.cpp)
- [x] [games/anomaly/src/native_observer_platform.h](games/anomaly/src/native_observer_platform.h)
- [x] [games/anomaly/src/native_profile.h](games/anomaly/src/native_profile.h) (generated projection from the Rust profile owner)
- [x] [games/anomaly/src/plugin.cpp](games/anomaly/src/plugin.cpp)
- [x] [games/anomaly/src/runtime_module.h](games/anomaly/src/runtime_module.h)
- [x] [games/anomaly/temporal/frame_plan.h](games/anomaly/temporal/frame_plan.h)
- [x] [games/anomaly/temporal/history.h](games/anomaly/temporal/history.h)
- [x] [games/anomaly/temporal/math.h](games/anomaly/temporal/math.h)
- [x] [games/anomaly/temporal/planning.h](games/anomaly/temporal/planning.h)

**Fixtures (6)**

- [x] [tests/anomaly_bootstrap.cpp](tests/anomaly_bootstrap.cpp)
- [ ] [tests/anomaly_build_select.h](tests/anomaly_build_select.h)
- [x] [tests/anomaly_lifecycle.cpp](tests/anomaly_lifecycle.cpp)
- [ ] [tests/anomaly_native_hook_leaves.cpp](tests/anomaly_native_hook_leaves.cpp) (native detour fixture functions)
- [x] [tests/anomaly_native_hooks.cpp](tests/anomaly_native_hooks.cpp)
- [x] [tests/anomaly_plugin.cpp](tests/anomaly_plugin.cpp)

**Rust review findings (8)**

- [x] **Duplication | Medium | Use the same FG capture settings decoder for the Anomaly panel and scheduler.** The panel re-reads the start, count and interval keys with its own defaults and clamps at games/anomaly/src/adapter_options.rs:190, :198 and :206. The scheduler separately decodes the same keys at games/anomaly/src/fg_stage/options.rs:32, and Capture::claim consumes those values at :77. For a stored -1, both cast to u32, but the panel clamps start/interval to i32::MAX while the scheduler uses u32::MAX. Decode persisted values once into CaptureSettings using a documented range representable by the panel, and derive both its getters and the scheduler configuration from it, preserving count and interval limits. [games/anomaly/src/adapter_options.rs:190](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/adapter_options.rs#L190)

- [x] **Duplication | Medium | Use the generated plugin-session options type in both Rust callers.** Anomaly constructs its own 128-byte options record in prepare_observation at games/anomaly/src/bootstrap.rs:407, while the callback host builds a second manual mirror at runtime/orchestrator/src/callback_host.rs:97 and :1034. Both target plugin_session.h and repeat its ABI version. The public Rust SDK already exports the matching generated type and version at sdk/game/src/plugin_session_abi.rs:7 and :9 with checks for every field offset. Reuse these definitions for both calls and their test callbacks; currently a header update can leave either manual type stale while the generated layout checks still pass. [games/anomaly/src/bootstrap.rs:69](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/bootstrap.rs#L69)

- [x] **Altitude | Medium | Keep input-hook installation separate from the stages-configured flag.** With the FG bridge installed, install_input can attach the DirectInput observer successfully and then fail the guarded engine input hook. The function returns no outcome, and orchestrate immediately marks all stages configured. A retry after a session or overlay refusal skips both hooks, leaving input observation partial for that process with only a log explaining why. Return the per-hook result, retain completed steps and retry only the unfinished hook when cold startup is retried. [games/anomaly/src/bootstrap.rs:296](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/bootstrap.rs#L296)

- [x] **Duplication | Medium | Share partial-pivot matrix inversion while preserving adapter tolerances.** Anomaly's SR and FG camera builders at games/anomaly/src/sr_pass.rs:103 and games/anomaly/src/fg_stage/camera.rs:34 call frame, which performs four inversions. Its local implementation at games/anomaly/src/camera.rs:120 duplicates the augmented-matrix pivot selection, normalization, and elimination in sdk/game/src/camera_math.rs:5 used by AC7 at games/ac7/src/view.rs:111. Anomaly checks all inputs for finiteness and uses a 1e-300 pivot floor, reciprocal multiplication and a zero-factor skip; the SDK uses 1e-12 and division. Consolidate the elimination in the SDK with explicit numeric policies and reference-vector tests for near-singular inputs before migrating either caller. [games/anomaly/src/camera.rs:120](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/camera.rs#L120)

- [x] **Altitude | Medium | Reuse the verified executable identity across cold startup and session preparation.** bootstrap::enter synchronously hashes the entire running executable to choose a profile, then calls Runtime session prepare. Observer::prepare calls identify, which invokes running_build and hashes the same file again before checking the mapped image. This duplicates file I/O before DirectInput creation. Share the verified build identity for the same process image across these calls, preserving the independent mapped-site checks and full verification for standalone plugin entry. [games/anomaly/src/native_observer.rs:62](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/native_observer.rs#L62)

- [x] **Altitude | Low | Read each variable-length instruction guard as one range.** Image::matches_hex checks the range as a whole and then calls the native exception-guarded copy leaf once per byte. Motion and SR stage startup invoke this for prologues, call setups and reconstruction guards, even though Read already accepts a length and fixed-size sites use a single copy. Decode the expected span, read the bounded live span once and compare slices, preserving the existing first-failing-site report. [games/anomaly/src/engine_guards.rs:109](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/engine_guards.rs#L109)

- [x] **Duplication | Low | Give Windows module-path lookups one explicit terminator contract.** Project Wingman's carrier at games/project-wingman/src/carrier.rs:31 and RDR2's worker at games/rdr2/src/runtime_worker.rs:64 each allocate 32,768 WCHARs, call GetModuleFileNameW, reject truncation and return an unterminated Vec. Their callers use those slices for deployment paths at :70 and :284 respectively. Anomaly repeats the same lookup at games/anomaly/src/executable.rs:13 but retains a trailing NUL for the prepare call at games/anomaly/src/bootstrap.rs:388 and strips it again to hash the executable. Use a shared bounded Win32 lookup returning an owned path with explicit terminated and unterminated accessors. Keep per-game selection and file naming local. [games/anomaly/src/executable.rs:13](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/executable.rs#L13)

- [x] **Duplication | Low | Consolidate the raw UTF-16 scans at Anomaly configuration boundaries.** Cold startup at games/anomaly/src/bootstrap.rs:196 and overlay setup at games/anomaly/src/overlay.rs:881 use ini::path, which scans for NUL, rejects empty paths and returns the terminator. The FG options reader at games/anomaly/src/fg_stage/options.rs:211, motion configuration at games/anomaly/src/motion/stage/options.rs:45 and SR configuration at games/anomaly/src/sr_stage.rs:2701 independently walk the same borrowed pointer format, excluding the terminator. This spreads the unsafe pointer contract across four implementations. Use a shared slice-until-NUL primitive usable by standalone Rust builds, with wrappers retaining each caller's null/empty handling and explicit terminator inclusion where Win32 needs it. [games/anomaly/src/ini.rs:12](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/ini.rs#L12)

<a id="n02"></a>

### N02 · Anomaly input and overlay

Port the input hooks and overlay integration, including the handoff between gameplay input and overlay capture.

**Coordinate shared edits**

- Anomaly session startup, quiesce and callback retirement ([N01](#n01), [N03](#n03), [N04](#n04), [N06](#n06), [N08](#n08)).

**Port files (9)**

- [x] [games/anomaly/src/input_hooks.cpp](games/anomaly/src/input_hooks.cpp)
- [x] [games/anomaly/src/input_hooks.h](games/anomaly/src/input_hooks.h)
- [x] [games/anomaly/src/input_profile.h](games/anomaly/src/input_profile.h)
- [x] [games/anomaly/src/overlay_hooks.cpp](games/anomaly/src/overlay_hooks.cpp)
- [x] [games/anomaly/src/overlay_hooks.h](games/anomaly/src/overlay_hooks.h)
- [x] [games/anomaly/src/overlay_input_gate.h](games/anomaly/src/overlay_input_gate.h)
- [x] [games/anomaly/src/overlay_platform.h](games/anomaly/src/overlay_platform.h)
- [x] [games/anomaly/src/overlay_service.cpp](games/anomaly/src/overlay_service.cpp)
- [x] [games/anomaly/src/overlay_service.h](games/anomaly/src/overlay_service.h)

**Fixtures (5)**

- [x] [tests/anomaly_input_hooks.cpp](tests/anomaly_input_hooks.cpp)
- [x] [tests/anomaly_overlay_api.cpp](tests/anomaly_overlay_api.cpp)
- [x] [tests/anomaly_overlay_hooks.cpp](tests/anomaly_overlay_hooks.cpp)
- [x] [tests/anomaly_overlay_input_gate.cpp](tests/anomaly_overlay_input_gate.cpp)
- [x] [tests/anomaly_overlay_visibility.cpp](tests/anomaly_overlay_visibility.cpp)

**Rust review findings (4)**

- [x] **Altitude | Medium | Report consumed input from the gate operation that identified it.** hook_get_device_data (overlay_hooks.cpp:147-150) calls Rust to read and filter the device, carries a hardware boolean back to C++, then re-enters Rust with data/count pointers, flags and HRESULT to determine whether to notify. The second call locks the device registry again to rediscover the kind even though the first call held the originating entry and knew the consumed hardware count. Produce the notification decision and kind during the read, release the lock, then invoke the observer once. Preserve peek, loss and synthetic-release handling. [games/anomaly/src/input.rs:483](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/input.rs#L483)

- [x] **Boundaries | Medium | Give the Anomaly overlay diagnostics INI keys adapter option rows.** AGENTS.md Rule 2 requires every option an integration reads, diagnostic ones included, to have a live overlay control. The Anomaly overlay start reads OverlayKeyPoll at overlay.rs:936 and OverlayCapture, OverlayCaptureFrames (1-16), OverlayCaptureStride (1-600) and OverlayCaptureCycles (1-8) at overlay.rs:473-485 straight from the adapter INI, and only at start. None of them is registered through rsf_anomaly_overlay_option_add: adapter_options.rs:62-125 registers the SR stage, bridge, F10 capture and FG capture rows, and the shared service only sees REGISTRY. To try a panel capture, a user has to edit the INI and restart the game. Register them as restart-marked toggle/number rows next to the FG capture rows, or re-read them when the option changes. OverlayCaptureDir is a path, so it is the only one the number/toggle option kinds cannot express. [games/anomaly/src/overlay.rs:473](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/overlay.rs#L473)

- [x] **Altitude | Medium | Return the actual adapter-option apply and save result.** The shared service uses option_changed's return value to update its state and log applied or refused. Anomaly wraps every setter as void and returns 1 for any known index. For example, set_scope_hold changes live state only if its INI write succeeds, so a read-only configuration yields an applied log even though neither live nor saved state changed. Make the adapter setter return its outcome and pass it through to the existing runtime callback. [games/anomaly/src/overlay.rs:1149](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/overlay.rs#L1149)

- [x] **Duplication | Low | Reuse the local timestamp formatter for Anomaly capture directories.** The overlay capture root at games/anomaly/src/overlay.rs:494 and FG capture root at games/anomaly/src/fg_stage/options.rs:154 both append a timestamp from GetLocalTime with the same six fields and YYYYMMDD-HHMMSS formatting. The implementations at overlay.rs:1215 and games/anomaly/src/fg_stage/options.rs:233 differ only in zero initialization. Move the timestamp formatter into one adapter-local capture utility and keep the two root names and custom-directory handling local. A naming or precision change then applies consistently to both capture mechanisms. [games/anomaly/src/overlay.rs:1215](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/overlay.rs#L1215)

<a id="n03"></a>

### N03 · Anomaly geometry

Port geometry admission, observation and retirement together with their contract fixtures.

**Coordinate shared edits**

- Anomaly session startup, quiesce and callback retirement ([N01](#n01), [N02](#n02), [N04](#n04), [N06](#n06), [N08](#n08)).

**Port files (8)**

- [x] [games/anomaly/src/geometry_feed.cpp](games/anomaly/src/geometry_feed.cpp)
- [ ] [games/anomaly/src/geometry_feed.h](games/anomaly/src/geometry_feed.h)
- [ ] [games/anomaly/src/geometry_profile.h](games/anomaly/src/geometry_profile.h)
- [x] [games/anomaly/src/geometry_registry_platform.h](games/anomaly/src/geometry_registry_platform.h)
- [ ] [games/anomaly/src/geometry_retirement.h](games/anomaly/src/geometry_retirement.h)
- [ ] [games/anomaly/src/native_geometry.cpp](games/anomaly/src/native_geometry.cpp)
- [ ] [games/anomaly/src/native_geometry.h](games/anomaly/src/native_geometry.h)
- [ ] [games/anomaly/src/native_geometry_platform.h](games/anomaly/src/native_geometry_platform.h)

**Fixtures (2)**

- [ ] [tests/anomaly_geometry_hooks.cpp](tests/anomaly_geometry_hooks.cpp)
- [ ] [tests/anomaly_geometry_tests.cpp](tests/anomaly_geometry_tests.cpp)

**Rust review findings (3)**

- [x] **Altitude | Medium | Capture skinned palettes as bounded spans instead of two native callbacks per bone.** On each captured skinned draw, write_palette asks platform.bone for one matrix, validates and transposes it, then calls writer.bone to push the matrix into a C++ vector already reserved by writer.begin. With the admitted 1,024-bone limit this can cross the FFI boundary 2,048 times for one palette. Have the native reader supply a bounded batch of guarded matrices and the storage owner lend a destination span, or publish one completed palette. Preserve all-or-nothing validity if any matrix fails. [games/anomaly/src/native_geometry.rs:483](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/native_geometry.rs#L483)

- [x] **Duplication | Low | Use one X-Ray matrix conversion for geometry and motion.** Geometry capture transposes and widens world/bone Fmatrix values after an affine check at games/anomaly/src/native_geometry.rs:395 and :489. Motion transposes and widens with the same indexing at games/anomaly/src/motion.rs:808. Its Rust camera reader at games/anomaly/src/motion/stage/draw.rs:45 calls that C export through MaybeUninit to obtain a matrix. Use a game-local pure function returning the transposed f64 array, have the C export write its result for native callers, and call it directly in the Rust readers. Keep the finite/affine check on geometry only so perspective camera matrices remain supported. [games/anomaly/src/native_geometry.rs:395](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/native_geometry.rs#L395)

- [x] **Duplication | Low | Share Anomaly's nullable-empty FFI slice conversion.** GuardProfile's accessors at games/anomaly/src/native_geometry_abi.rs:154 use the same unsafe zero-count branch and from_raw_parts construction as games/anomaly/src/motion.rs:268 for borrowed palettes and deformation constants. The motion and SR guard readers manually repeat the branch for unique guards at games/anomaly/src/engine_guards/motion.rs:51 and games/anomaly/src/engine_guards/sr.rs:58. All permit NULL only with zero elements; guard profiles live for the module while draw inputs are callback-scoped. Centralize the conversion and document the nonempty pointer, length and lifetime preconditions; keep the slices tied to their respective enclosing borrows. [games/anomaly/src/native_geometry_abi.rs:167](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/native_geometry_abi.rs#L167)

<a id="n04"></a>

### N04 · Anomaly motion and foliage

Port motion draw hooks, target ownership and foliage history. Use the draw and device fixtures to trace binding restoration.

**Coordinate shared edits**

- Anomaly session startup, quiesce and callback retirement ([N01](#n01), [N02](#n02), [N03](#n03), [N06](#n06), [N08](#n08)).
- Motion history, shader identity and published frame resources ([N05](#n05), [N07](#n07), [N08](#n08)).

**Port files (16)**

- [ ] [games/anomaly/src/motion_bridge.cpp](games/anomaly/src/motion_bridge.cpp)
- [ ] [games/anomaly/src/motion_constants.h](games/anomaly/src/motion_constants.h)
- [x] [games/anomaly/src/motion_d3d11.cpp](games/anomaly/src/motion_d3d11.cpp)
- [x] [games/anomaly/src/motion_d3d11.h](games/anomaly/src/motion_d3d11.h)
- [x] [games/anomaly/src/motion_device_platform.h](games/anomaly/src/motion_device_platform.h)
- [x] [games/anomaly/src/motion_foliage.cpp](games/anomaly/src/motion_foliage.cpp)
- [x] [games/anomaly/src/motion_foliage.h](games/anomaly/src/motion_foliage.h)
- [x] [games/anomaly/src/motion_history.h](games/anomaly/src/motion_history.h)
- [x] [games/anomaly/src/motion_policy.h](games/anomaly/src/motion_policy.h)
- [ ] [games/anomaly/src/motion_profile.h](games/anomaly/src/motion_profile.h)
- [ ] [games/anomaly/src/motion_stage.cpp](games/anomaly/src/motion_stage.cpp)
- [ ] [games/anomaly/src/motion_stage.h](games/anomaly/src/motion_stage.h)
- [ ] [games/anomaly/src/motion_stage_platform.h](games/anomaly/src/motion_stage_platform.h)
- [x] [games/anomaly/src/motion_target.cpp](games/anomaly/src/motion_target.cpp)
- [x] [games/anomaly/src/motion_target.h](games/anomaly/src/motion_target.h)
- [x] [games/anomaly/src/motion_target_platform.h](games/anomaly/src/motion_target_platform.h)

**Fixtures (8)**

- [x] [tests/anomaly_foliage_shader_device.cpp](tests/anomaly_foliage_shader_device.cpp)
- [x] [tests/anomaly_motion_contract.cpp](tests/anomaly_motion_contract.cpp)
- [ ] [tests/anomaly_motion_device.cpp](tests/anomaly_motion_device.cpp)
- [ ] [tests/anomaly_motion_far_device.cpp](tests/anomaly_motion_far_device.cpp)
- [ ] [tests/anomaly_motion_hooks_native.cpp](tests/anomaly_motion_hooks_native.cpp) (native detour fixture driver)
- [x] [tests/anomaly_motion_hooks.cpp](tests/anomaly_motion_hooks.cpp)
- [ ] [tests/anomaly_motion_tess_chain_device.cpp](tests/anomaly_motion_tess_chain_device.cpp)
- [x] [tests/anomaly_motion_tess_device.cpp](tests/anomaly_motion_tess_device.cpp)

**Rust review findings (12)**

- [x] **Duplication | Medium | Use the geometry producer's draw kinds and verdicts in motion history selection.** Stage::draw passes facts.verdict to CoreDrawInput at games/anomaly/src/motion/stage/draw.rs:302. The decision logic compares it with local literal aliases at games/anomaly/src/motion.rs:880; the registry already returns the named ABI codes defined at games/anomaly/src/geometry_registry_abi.rs:23, :25 and :31. The adjacent rigid/skinned/single-bone literals at motion.rs:875 also repeat games/anomaly/src/native_geometry_abi.rs:14, which the snapshot classifier already imports. Use those same constants in the history decision, converting the registry's i32 constants to the existing u8 representation in one place. This keeps classification and history admission synchronized if codes change. [games/anomaly/src/motion.rs:875](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/motion.rs#L875)

- [x] **Altitude | Medium | Distinguish an unreadable render option from observed multisampling.** open_window maps both a failed options_word3 read and a successfully observed DX10_MSAA bit to MULTISAMPLED, then increments msaa_frames. A target_facts read failure takes another path that does not increment target_failed. Diagnostics can therefore attribute missing motion to enabled MSAA without that fact ever being observed, or omit the actual descriptor failure. Carry distinct option-read, descriptor-read, multisampled and target-allocation outcomes through admission and report the actual refusal. [games/anomaly/src/motion/stage.rs:494](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/motion/stage.rs#L494)

- [x] **Duplication | Medium | Rotate Anomaly model and foliage histories from one frame identity.** The stage constructs a FrameIdentity for model history at games/anomaly/src/motion/stage.rs:525 and immediately passes the same four scalar values through the foliage C entry at :535. games/anomaly/src/motion.rs:367 and games/anomaly/src/motion/foliage.rs:100 separately implement same-frame reuse, rollover and discontinuity handling using the same follows predicate. A change made on just one path can pair model and foliage data with different predecessor frames. Construct the identity once and share the rotation decision or current/previous frame container, while preserving foliage's map-capacity recycling and the model retirement watermarks; keep the scalar C entry as an adapter for its native callers. [games/anomaly/src/motion/stage.rs:535](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/motion/stage.rs#L535)

- [x] **Duplication | Medium | Use one motion frame-target description between Anomaly motion and FG.** Motion produces FrameTarget at games/anomaly/src/motion/stage.rs:587 and returns it through games/anomaly/src/motion_stage.cpp:399. The six facts are copied straight back to Rust at games/anomaly/src/fg_stage_platform.cpp:61 using the generated FG type at games/anomaly/src/fg_stage_abi.rs:882. The two 24-byte representations differ only in the order of the final main and written flags; size checks cannot catch swapping them. Define the common payload in a game-local C header and import its generated Rust type in both owners, retaining the existing platform callback for fixtures. Coordinate the private ABI versions when aligning the fields. [games/anomaly/src/motion/stage/abi.rs:193](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/motion/stage/abi.rs#L193)

- [x] **Duplication | Medium | Use the range-aware option builder for Anomaly motion controls.** Motion's entry helper duplicates the descriptor construction in games/anomaly/src/adapter_options.rs:35, but hardcodes minimum=0 and infers a toggle only from the upper bound. The registration at games/anomaly/src/motion/stage/options.rs:156 therefore advertises zero as a valid capture interval even though its setter at :259 immediately saves at least one. Both call games/anomaly/src/overlay.rs:155. Share the range-based construction, accept the motion pack's stable mutable label pointer as well as static labels, and register interval as 1..=i32::MAX. This fixes the displayed range without changing capture timing or persistence. [games/anomaly/src/motion/stage/options.rs:95](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/motion/stage/options.rs#L95)

- [x] **Altitude | Medium | Keep the motion pack's opaque handle opaque to the draw stage.** Platform::open_pack returns a void handle and a PackInfo containing only the manifest. During device creation Pack::constant_ops casts the handle directly to the private Rust RuntimePack type. That works only because motion_stage.cpp currently forwards the exact allocation from open_runtime_pack, a dependency neither the platform signature nor its size/version check expresses. Include the constant operations in PackInfo while opening the pack, or retain a typed RuntimePack directly when Rust owns both sides. [games/anomaly/src/motion/stage/shaders.rs:294](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/motion/stage/shaders.rs#L294)

- [x] **Altitude | Medium | Invalidate window readiness when the engine replaces the position surface.** window::begin overwrites position_rtv and position_surface but computes fresh using only frame, view, reset generation and context. A second window in the same frame can therefore inherit target_ok after changing surface. Stage::open_window validates the descriptor, resizes and clears the motion target only if fresh, so the new surface can be paired with old motion data or extent. Track the surface and relevant view identity in the readiness decision and revalidate when either changes. [games/anomaly/src/motion/window.rs:54](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/motion/window.rs#L54)

- [x] **Altitude | Medium | Distinguish no binding from a binding that cannot be inspected.** bind_close returns NONE when the backend pointer is null even if cache_may_hold_motion is still set. WindowFacts::read accepts a null backend, and Stage::open_window treats every result other than FAULT as permission to continue to device replacement, which can release the old RTV. Return an unresolved cleanup result when the previous engine cache may still borrow the RTV, and retain its owner until the original cache is verified clear. [games/anomaly/src/motion_target.rs:161](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/motion_target.rs#L161)

- [x] **Duplication | Low | Reuse the Anomaly host-log callback wrapper in motion and FG.** games/anomaly/src/motion/stage.rs:131 and games/anomaly/src/fg_stage.rs:48 both store rsf_game_log_fn plus a user address and implement identical borrowed CStr delivery and fallible CString conversion for owned messages. Motion logs guard failures through it at :352 and FG logs missing runtime entries at :122. The motion copy additionally derives Default for its initially empty logger. Move this small wrapper to one adapter-local module and reuse it in both stages so the user-pointer conversion, interior-NUL handling, and callback lifetime contract have one implementation. [games/anomaly/src/motion/stage.rs:131](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/motion/stage.rs#L131)

- [x] **Duplication | Low | Use one Anomaly frame-dump directory resolver for SR and motion.** Motion configures DumpApi at games/anomaly/src/motion/stage/capture.rs:76, selecting RSF_DUMP_DIR or LOCALAPPDATA/ReScaleFrame/Anomaly at :58. SR repeats the resolution at games/anomaly/src/sr_stage.rs:715 for capture setup at :1301, but filters out environment values of length 32,768 or greater. With the same environment the two owners of the same frame-dump service can select different destinations. Move resolution to frame_dump.rs, choose one documented length rule, and retain mode-specific capture admission and failure messages. [games/anomaly/src/motion/stage/capture.rs:58](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/motion/stage/capture.rs#L58)

- [x] **Altitude | Low | Keep matrix validation failures distinct from bone-count refusals.** decide_core already rejects a skinned palette over CORE_MAX_BONES. The subsequent pack_core can still fail for an unusable WVP or bone matrix, but upload_core translates every false result to TooManyBones, even for a rigid draw with no bones. Return a typed packing refusal from the Rust implementation and preserve its reason in the draw counters, retaining the boolean wrapper for native callers that need only success or failure. [games/anomaly/src/motion/stage/draw.rs:326](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/motion/stage/draw.rs#L326)

- [x] **Simplicity | Low | Anomaly motion device and binder tables unwrap bindgen Option leaves on every use.** The private `rsf_anomaly_motion_device_platform` and `rsf_anomaly_motion_bind_platform` are bindgen structs of `Option` leaves that are never validated, so each use is an `unwrap()`: motion_device.rs:76-78, 89, 105, 108, 128-131, 281, 294; motion_target.rs:42, 54, 145, 183; motion/stage.rs:629-630 and motion/stage/shaders.rs:452, 480-481 for `device.release/retain`. The neighbouring private motion-stage table (motion/stage/abi.rs:114-167) already declares plain `unsafe extern "C" fn` fields and is admitted once by size and version in `checked_platform`. Declare these two tables the same way (or mark the leaves non-nullable in the bindgen header), so the Rust side calls `(p.release)(x)` directly; a missing native leaf then fails at the C++/Rust contract instead of as a panic inside an `extern "C"` call, and ~20 unwraps disappear. Related to the Anomaly FG table entry for fg_stage.rs. [games/anomaly/src/motion_device.rs:76](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/motion_device.rs#L76)

<a id="n05"></a>

### N05 · Anomaly shader loading and temporal patches

Port shader loading, reflection and interception. Track shader-language assets separately from their host code.

**Coordinate shared edits**

- Motion history, shader identity and published frame resources ([N04](#n04), [N07](#n07), [N08](#n08)).

**Port files (9)**

- [x] [games/anomaly/src/engine_shader_includes.cpp](games/anomaly/src/engine_shader_includes.cpp) (guarded native engine includes)
- [x] [games/anomaly/src/runtime_shaders.cpp](games/anomaly/src/runtime_shaders.cpp)
- [x] [games/anomaly/src/runtime_shaders.h](games/anomaly/src/runtime_shaders.h)
- [x] [games/anomaly/src/shader_d3d.cpp](games/anomaly/src/shader_d3d.cpp)
- [x] [games/anomaly/src/shader_d3d.h](games/anomaly/src/shader_d3d.h)
- [x] [games/anomaly/src/shader_loader.cpp](games/anomaly/src/shader_loader.cpp) (deleted; include opening moved to `shader_loader/d3d.rs`, fixture marshalling moved to `tests/anomaly_motion_shader_facade.h`)
- [x] [games/anomaly/src/shader_loader.h](games/anomaly/src/shader_loader.h) (deleted; its C++ fixture types live in `tests/anomaly_motion_shader_facade.h` until N04 retires the three device fixtures)
- [x] [games/anomaly/src/shader_loader_platform.h](games/anomaly/src/shader_loader_platform.h) (C11 declarations only)
- [x] [tools/anomaly_shader_constants.h](tools/anomaly_shader_constants.h)

**Fixtures (1)**

- [x] [tests/anomaly_temporal_shader_device.cpp](tests/anomaly_temporal_shader_device.cpp)

**Rust review findings (2)**

- [x] **Duplication | Medium | Share shader-family IDs between Anomaly manifest admission, draws and reports.** Manifest admission at games/anomaly/src/shader_loader/manifest.rs:150 assigns the row position in FAMILIES as its numeric family. Draw decisions use separately assigned constants at games/anomaly/src/motion.rs:870, while replacement logs and counter reports index a second name table at games/anomaly/src/motion/stage/shaders.rs:29 and games/anomaly/src/motion/stage/report.rs:244. An insertion or reorder can misclassify draws or relabel counts even if both tables still compile. Define stable explicit family IDs with one name/stage mapping, retain the special diagnostic label 'other' for the zero bucket, and size counters from the same definition. [games/anomaly/src/shader_loader/manifest.rs:8](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/shader_loader/manifest.rs#L8)

- [x] **Altitude | Medium | Capture a complete reflection description at the native boundary.** D3DShaderReflector::reflect already owns the COM reflector, output vectors and counts, but calls Rust solely to drive indexes. Rust calls C++ for each signature, binding, buffer begin, variable and buffer finish. The callback protocol also treats unreadable signature rows as a successful enumeration, so compatibility checks receive shortened signatures without a completeness flag. Have the reflection owner return a complete snapshot or an explicit incomplete result in one operation, then perform compatibility checks on the snapshot. [games/anomaly/src/shader_loader/reflection.rs:47](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/shader_loader/reflection.rs#L47)

<a id="n06"></a>

### N06 · Anomaly SR stage and reconstruction

Port the live SR stage, D3D11 pass and reconstruction, including refusing a frame and releasing partial resources.

**Coordinate shared edits**

- Anomaly session startup, quiesce and callback retirement ([N01](#n01), [N02](#n02), [N03](#n03), [N04](#n04), [N08](#n08)).
- SR target publication, swap rollback and engine-owned texture fields ([N07](#n07)).

**Port files (11)**

- [ ] [games/anomaly/src/sr_d3d11.cpp](games/anomaly/src/sr_d3d11.cpp)
- [ ] [games/anomaly/src/sr_d3d11.h](games/anomaly/src/sr_d3d11.h)
- [ ] [games/anomaly/src/sr_d3d11_platform.h](games/anomaly/src/sr_d3d11_platform.h)
- [ ] [games/anomaly/src/sr_pass.h](games/anomaly/src/sr_pass.h)
- [x] [games/anomaly/src/sr_plan.h](games/anomaly/src/sr_plan.h)
- [ ] [games/anomaly/src/sr_profile.h](games/anomaly/src/sr_profile.h)
- [x] [games/anomaly/src/sr_reconstruction.h](games/anomaly/src/sr_reconstruction.h)
- [x] [games/anomaly/src/sr_settings.h](games/anomaly/src/sr_settings.h)
- [ ] [games/anomaly/src/sr_stage.cpp](games/anomaly/src/sr_stage.cpp)
- [ ] [games/anomaly/src/sr_stage.h](games/anomaly/src/sr_stage.h)
- [ ] [games/anomaly/src/sr_stage_platform.h](games/anomaly/src/sr_stage_platform.h)

**Fixtures (9)**

- [ ] [tests/anomaly_sr_hook_bridge.cpp](tests/anomaly_sr_hook_bridge.cpp) (native detour and PE fixture seam)
- [x] [tests/anomaly_sr_runtime_api_bridge.cpp](tests/anomaly_sr_runtime_api_bridge.cpp) (actual Runtime import and fixture entry)
- [x] [tests/anomaly_sr_hooks.cpp](tests/anomaly_sr_hooks.cpp)
- [x] [tests/anomaly_sr_pass.cpp](tests/anomaly_sr_pass.cpp)
- [x] [tests/anomaly_sr_plan.cpp](tests/anomaly_sr_plan.cpp)
- [ ] [tests/anomaly_sr_profile.cpp](tests/anomaly_sr_profile.cpp)
- [x] [tests/anomaly_sr_reconstruction.cpp](tests/anomaly_sr_reconstruction.cpp)
- [x] [tests/anomaly_sr_roundtrip.cpp](tests/anomaly_sr_roundtrip.cpp)
- [x] [tests/anomaly_sr_runtime_api.cpp](tests/anomaly_sr_runtime_api.cpp)

**Rust review findings (18)**

- [x] **Altitude | Medium | Report whether failed constant correction restored all engine state.** A failed vector or dirty-byte write triggers restoration of every touched slot. Slot::restore discards the result of both native writes, and sr_swap.rs:1125 receives the same false for untouched data and a partially corrected live buffer. Return distinct clean-refusal and incomplete-rollback outcomes, retain enough state to verify or retry restoration while the staging buffers are valid, and surface incomplete restoration to the swap owner. [games/anomaly/src/sr_reconstruction.rs:85](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_reconstruction.rs#L85)

- [x] **Duplication | Medium | Resolve Anomaly's vendor runtime directories once for SR and FG.** SR configuration at games/anomaly/src/sr_stage.rs:932 calls its own three-key directory resolver. FG repeats the SRStreamlineDir, SRFidelityFXDir and SRXeSSDir mapping and the adjacent ReScaleFrame subdirectory fallbacks in games/anomaly/src/fg_stage/options.rs:136 and :156. FG converts path units to a lossy String, then re-encodes them to reuse the SR normalization routine at :224. Both stages depend on the same package layout. Extract a shared UTF-16 resolver with an injected INI reader, then let SR keep PathBuf and convert once at FG's UTF-8 boundary. Leave the FG capture directory's verbatim-path behavior separate. [games/anomaly/src/sr_stage.rs:374](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_stage.rs#L374)

- [x] **Boundaries | Medium | Give the remaining Anomaly SR keys overlay rows.** AGENTS.md Rule 2 requires every option an integration reads to have a live overlay control. A restart-only option still needs a control that says so and saves the value. Ini::options reads several numeric and toggle keys from the [Anomaly] INI with no row in the shared panel: SRScale (sr_stage.rs:455, :397, re-read only after a device reset at :1416), SRStageCaptureStart/Count/Interval (:459-465), SRCutDistance and SRCutAngle (:474-475) and SRRequireSignature (:476). register_live_options at :2711 adds only colour transport, exposure, first-person zero, jitter, mip bias and mip offset. adapter_options.rs:62-125 adds only SRStage and SRStageCapture. The cut thresholds decide when Stage 4/5 resets history, and SRScale sets the Stage 2/3 render size. Today both can be tuned only by editing the INI and forcing a device reset or a restart. The adapter registers 25 of the RSF_OVERLAY_SERVICE_OPTION_LIMIT (32) rows, which leaves room for these seven. [games/anomaly/src/sr_stage.rs:455](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_stage.rs#L455)

- [x] **Altitude | Medium | Make native guard availability part of SR stage admission.** The generic leaf! fallback treats a missing pointer-returning callback as null. verify_guards has the opposite convention: null means every guard passed. Stage::register accepts the raw platform table, so a partially populated table can reach installation without the required guard check. Require the guard callback when binding the platform or use a typed outcome that distinguishes unavailable verification, a failed site and verified success. [games/anomaly/src/sr_stage.rs:1145](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_stage.rs#L1145)

- [x] **Duplication | Medium | Share the frame-dump admission transaction between Anomaly SR and motion.** SR capture_arm and games/anomaly/src/motion/stage/capture.rs:102 independently abandon unfinished captures, poll DumpApi, raise a scheduled request, check requested, begin the record and spend the scheduled budget only after acceptance. SR increments from world frames and suppresses scheduled requests during loading; motion counts native main-frame/generation identities and requires a written target. Keep those predicates and the distinct resource metadata with each stage, but give their existing frame_dump::DumpApi one operation taking the record and scheduled flag and returning whether a request was accepted. Preserve manual F10 requests when scheduling is false and share acceptance/retry fixtures. [games/anomaly/src/sr_stage.rs:1599](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_stage.rs#L1599)

- [x] **Altitude | Medium | Pass the seam operation directly between its two Rust owners.** controller_seam wraps the locked State in a raw cookie. sr_stage.cpp:447 stores it in a shared Host field, invokes the C++ SwapController facade, re-enters Rust Swap::seam, then calls back through Host::upscale or round_trip into Rust to reconstruct the mutable State reference. Correctness depends on an undocumented-in-types synchronous call stack and clearing the cookie before it expires. Let Swap::seam accept a scoped Rust operation borrowing the stage state, and retain native leaves for the engine and D3D operations it performs. [games/anomaly/src/sr_stage.rs:1784](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_stage.rs#L1784)

- [x] **Altitude | Medium | Publish the actual bilinear fallback result.** temporal_seam increments the driver's bilinear count before draw_pass(UPSCALE) returns. A reinsertion refusal also sets the status text to 'bilinear fallback drawn' before attempting that draw. If state capture or the fallback pass fails, the swap owner reports no world image while driver diagnostics report a fallback draw. Submit one fallback outcome including its initiating reason after draw_pass returns and distinguish attempted, succeeded and failed recovery. [games/anomaly/src/sr_stage.rs:1944](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_stage.rs#L1944)

- [x] **Boundaries | Medium | Let the Runtime choose and default the Anomaly SR backend.** AGENTS.md Rule 3 gives the orchestrator the choice of SR backend and every other vendor policy. The Anomaly adapter makes that choice itself. Ini::options at sr_stage.rs:479-492 sanitizes the shared SRBackend key with an adapter fallback of DLSS (BACKEND_DLSS) whatever the GPU. Stage::choices at :2572-2610 decides which vendors are offered by probing vendor file names (sl.interposer.dll plus nvngx_dlss.dll, amd_fidelityfx_upscaler_dx12.dll plus amd_fidelityfx_loader_dx12.dll, libxess.dll) in the adapter-resolved vendor directories. When the saved backend is not offered, it substitutes its own order: DLSS, XeSS, FSR3, FSR4. RSF_ANOMALY_SR_ADAPTER_BACKENDS (sr_stage_abi.rs:27 = 58) also leaves out RSF_SR_AUTO. The Runtime already owns this policy in rsf_gpu_select_sr_backend (runtime/orchestrator/src/lib.rs:385): it prefers DLSS on NVIDIA, XeSS on Intel and FSR3 otherwise. Concrete effect: on an AMD GPU with the full vendor package installed, Anomaly Stage 5 defaults to DLSS, which only fails once the pipeline starts, while every Runtime-hosted game picks FSR3. Report the installed vendor directories as facts, offer Auto, and take the default and fallback from the Runtime. [games/anomaly/src/sr_stage.rs:2572](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_stage.rs#L2572)

- [x] **Duplication | Medium | Derive the overlay mip-bias range from the SR option bounds.** The SRMipBias parser at games/anomaly/src/sr_stage.rs:431 uses clamp_mip_offset at games/anomaly/src/sr_stage/math.rs:68, whose generated limits are -2.0 and +1.0 at games/anomaly/src/sr_stage_abi.rs:11. The live overlay separately hardcodes minimum=-20, maximum=0 and divisor=10, so a supported +0.5 can be read from the INI but cannot be selected in the panel. Generate the integer panel bounds from the same constants and scale factor, and use that factor for getter/setter conversion. Cover both endpoints and a positive value through the descriptor. [games/anomaly/src/sr_stage.rs:2769](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_stage.rs#L2769)

- [x] **Altitude | Medium | Keep pending GPU query results distinct from terminal failures.** The timer_clock and timer_timestamp callbacks in sr_stage.cpp:490-500 collapse every HRESULT other than S_OK into false. Ring::collect interprets false as still pending and never frees the slot. A permanently failed query eventually leaves the cursor stuck on that slot and all subsequent timing intervals skipped until device replacement. Return ready, pending or failed; recycle or recreate failed queries and count their samples as skipped. [games/anomaly/src/sr_stage/gpu_timer.rs:70](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_stage/gpu_timer.rs#L70)

- [x] **Duplication | Medium | Exercise the production Win32 INI callbacks in the SR package test.** The disk-backed Windows fixture at games/anomaly/src/sr_stage/tests.rs:1191 installs its own GetPrivateProfileIntW/StringW wrappers. Production registers games/anomaly/src/ini.rs:67 and :77 at games/anomaly/src/sr_stage.cpp:597. The copies already widen names differently: the test decodes UTF-8, while production widens individual bytes. All current test keys are ASCII, so it can pass after a production callback regression. Expose the two real leaves within the crate and point this fixture's platform table at them, keeping the in-memory reader for scenarios that intentionally inject fake INI responses. [games/anomaly/src/sr_stage/tests.rs:1151](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_stage/tests.rs#L1151)

- [x] **Altitude | Low | Create only the depth state required by temporal preparation.** Pass::new has already acquired the rasterizer and blend state that Pass::draw uses for all draws. Compiler::temporal calls States::new again just for no_depth, crossing the device boundary to create two extra states and immediately dropping them. Failure to create either unused state disables the temporal path. Request the no-depth state directly and keep the existing shared rasterizer and blend leases. [games/anomaly/src/sr_d3d11/passes.rs:118](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_d3d11/passes.rs#L118)

- [x] **Altitude | Low | Store the creating device in the temporal target cache key.** sr_stage.rs:1984 calls TemporalTargets::matches while preparing a temporal frame. The owner stores both extents and format but omits the device passed to new, so each lookup crosses FFI to get the device from its own color texture and releases the returned COM reference. Retain the creating device identity alongside the other key fields for the lifetime of these textures and match it directly. [games/anomaly/src/sr_d3d11/targets.rs:661](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_d3d11/targets.rs#L661)

- [x] **Duplication | Low | Read swap counter buckets with the same constants used to increment them.** The SR stage declares six skip and six restore indices by hand at games/anomaly/src/sr_stage.rs:63 and :69, then reports those array elements at :2456 and :2465. The swap controller uses the generated constants at games/anomaly/src/sr_swap_abi.rs:11 and :18 when recording skips and restoring the seam at games/anomaly/src/sr_swap.rs:872 and :1054. If ABI bucket assignments change, counts can silently appear under the wrong labels. Import the generated constants for reporting and convert to usize only at the indexing boundary. [games/anomaly/src/sr_stage.rs:63](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_stage.rs#L63)

- [x] **Simplicity | Low | Anomaly SR stage state is an Option<Box> that every accessor lazily recreates.** `Stage::state` is `Mutex<Option<Box<State>>>` only so `Stage::new` can be `const`; nothing ever stores `None` again, yet eleven call sites (`with`, `install`, `quiesce`, `prepare`, `seam`, `combine_end`, `frame_end`, `report` and others, e.g. sr_stage.rs:855, 1119, 1215, 1359, 1693, 1794, 2183, 2232, 2393) repeat `guard.get_or_insert_with(Default::default)` before touching it. `install` also drops the guard and immediately re-locks it (sr_stage.rs:1213-1215) with nothing in between. Make the field `LazyLock<Mutex<State>>` (`LazyLock::new` is const) or `Mutex<State>` built by a const-friendly `State::new`, so accessors use `self.lock()` directly and `install` keeps one guard. Same behavior: the first access still creates the default state; the tradeoff is none beyond losing the never-used ability to drop the boxed state. [games/anomaly/src/sr_stage.rs:692](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_stage.rs#L692)

- [x] **Quality | Low | Reset the GPU timing report baseline when the D3D11 device changes.** Timer::begin resets accumulated milliseconds and sample counts when the device changes (sr_stage/gpu_timer.rs:122), but report retains the previous device totals. After a report with one swapped sample, the first report on a replacement device with no samples prints n=18446744073709551615; if enough new samples arrive first, even the computed average can use milliseconds from the wrong device. Reset or generation-tag the timing baseline together with the ring. The test named report_emits_deltas_and_survives_a_timer_reset has no query leaves and never switches device: exercise the replacement after a nonzero sample and assert the count and average. [games/anomaly/src/sr_stage.rs:2478](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_stage.rs#L2478)

- [x] **Duplication | Low | Share the Halton sample calculation used by Anomaly and Skyrim.** Anomaly generates centered base-2/base-3 pixel jitter at games/anomaly/src/sr_stage/math.rs:112 and Skyrim converts the same sequence into signed projection offsets at games/skyrim-se/src/resolution.rs:228. Their digit loops at math.rs:85 and games/skyrim-se/src/resolution.rs:123 differ in precision, and only the externally callable Anomaly path handles bases below two. Put the radical-inverse calculation in a shared math module, preserve the existing arithmetic precision or verify the float differences with sequence vectors, and keep indexing, phase counts and projection signs with each adapter. [games/anomaly/src/sr_stage/math.rs:85](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_stage/math.rs#L85)

- [x] **Altitude | Low | Keep mapped row data inside the mapping guard.** map returns a Copy Layout containing the mapped pointer and row pitch both inside Mapping and as an independent tuple element. The burst writer consumes Mapping with is_some_and, which unmaps the texture, then continues using the independent Layout for dimensions. Those dimensions stay valid, but the same object still exposes an expired row pointer. Return pointer-free descriptor metadata separately and expose mapped rows only through a borrow of Mapping. [games/anomaly/src/sr_stage/staging.rs:160](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_stage/staging.rs#L160)

<a id="n07"></a>

### N07 · Anomaly SR swap, jitter and temporal history

Port swap ownership, projection jitter, temporal history and tonemap routing. Address engine references before releasing twin resources.

**Coordinate shared edits**

- Motion history, shader identity and published frame resources ([N04](#n04), [N05](#n05), [N08](#n08)).
- SR target publication, swap rollback and engine-owned texture fields ([N06](#n06)).

**Port files (7)**

- [ ] [games/anomaly/src/sr_jitter.h](games/anomaly/src/sr_jitter.h)
- [x] [games/anomaly/src/sr_swap.cpp](games/anomaly/src/sr_swap.cpp)
- [ ] [games/anomaly/src/sr_swap.h](games/anomaly/src/sr_swap.h)
- [x] [games/anomaly/src/sr_swap_platform.h](games/anomaly/src/sr_swap_platform.h)
- [x] [games/anomaly/src/sr_temporal.cpp](games/anomaly/src/sr_temporal.cpp)
- [x] [games/anomaly/src/sr_temporal.h](games/anomaly/src/sr_temporal.h)
- [x] [games/anomaly/src/sr_tonemap.h](games/anomaly/src/sr_tonemap.h)

**Fixtures (6)**

- [x] [tests/anomaly_sr_jitter_reference.h](tests/anomaly_sr_jitter_reference.h) (independent native projection oracle)
- [x] [tests/anomaly_sr_jitter.cpp](tests/anomaly_sr_jitter.cpp)
- [x] [tests/anomaly_sr_jitter_device.cpp](tests/anomaly_sr_jitter_device.cpp)
- [x] [tests/anomaly_sr_swap.cpp](tests/anomaly_sr_swap.cpp)
- [x] [tests/anomaly_sr_temporal_device.cpp](tests/anomaly_sr_temporal_device.cpp)
- [x] [tests/anomaly_temporal_tests.cpp](tests/anomaly_temporal_tests.cpp)

**Rust review findings (9)**

- [x] **Altitude | High | Retain twin resources while any engine field may still reference them.** Swap::restore records a write fault if restoring CRT/CTexture fields or globals fails, but unconditionally closes the window and increments its restore counter. Shutdown then skips further restoration on that closed window and calls release_twins, while an engine field may still hold the old borrowed surface or view. Track incomplete restoration separately from closed, retain the twin leases until all affected fields are verified restored and return that cleanup outcome to quiesce. [games/anomaly/src/sr_swap.rs:843](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_swap.rs#L843)

- [x] **Duplication | Medium | Run the projection-jitter conformance cases against the Rust implementation.** Main and first-person world swaps call the Rust jitter_projection in games/anomaly/src/sr_swap.rs:95, while the numerical and device fixtures call a distinct C++ implementation at games/anomaly/src/sr_jitter.h:97. In tests/anomaly_sr_jitter.cpp:110 the expected value and jitter_engine_projection both call temporal::jitter_projection on the same transposed input, so that comparison cannot detect disagreement in Rust. Keep the C++ implementation as an independent oracle for the existing swap integration tests, and run the actual Rust function against the perspective, orthographic and refusal vectors, using a narrow test entry or Rust unit tests. The device fixture at tests/anomaly_sr_jitter_device.cpp:217 can then exercise the production transform as well. [games/anomaly/src/sr_swap.rs:95](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_swap.rs#L95)

- [x] **Altitude | Medium | Invalidate the engine's texture-cache ranges as ranges.** Each Stage 2 exchange and restoration sends 86 guarded write calls across FFI to invalidate RCache. The a_failed_exchange_rolls_every_write_back fixture confirms that 84 of these clear individual pointer slots in six contiguous ranges. The native write leaf already accepts a byte length. Clear each verified range with a bounded zero-filled span, preserving neighboring fields, handling partial failures and keeping table layout policy in the adapter. [games/anomaly/src/sr_swap.rs:576](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_swap.rs#L576)

- [x] **Quality | Medium | Roll back a world swap when clearing the engine binding caches fails.** After exchanging the targets and dimensions, prepare calls invalidate_caches. A guarded write failure records a refusal there, but prepare continues, increments swapped and returns true for this frame. The engine can reuse an old SRV or constants table while rendering with the twin targets and reduced viewport. Return the cache invalidation result and close the exchange before allowing the engine to draw. Add a fixture that refuses a backend cache write during prepare. [games/anomaly/src/sr_swap.rs:1030](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_swap.rs#L1030)

- [x] **Quality | Medium | Preserve the already rendered world when sun-camera recovery closes the swap.** A sun-begin or sun-end failure closes the world window after geometry has drawn into reduced targets. close_with_output restores pointer bindings and viewport, then the later seam sees open=false and skips the only transfer from twins to engine targets. World colour, position and depth produced before the failure are therefore missing from subsequent passes. Preserve the twins through the seam or perform a verified transfer before resuming stock drawing. sun_refusal_returns_the_frame_to_output_size checks memory and viewport restoration only; assert the recovery transfer and its inputs as well. [games/anomaly/src/sr_swap.rs:1164](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_swap.rs#L1164)

- [x] **Quality | Medium | Restore the first-person camera after a partial rmNear jitter update.** The registered rmNear callback can write the jittered first-person projection successfully and then fail to write the full transform or upload the projection. fault only disables later swaps. The current first-person draw resumes with engine matrices or cached projection from different updates. Save the original first-person pair before mutation, attempt to restore both and the uploaded projection on any failure, and cover failure on the second write and upload in the fixture. [games/anomaly/src/sr_swap.rs:1227](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_swap.rs#L1227)

- [x] **Duplication | Medium | Give the Anomaly SR stage and swap controller the same frame-facts and counters types.** Two independent C headers define the same 152-byte frame facts and 352-byte counters, yielding games/anomaly/src/sr_swap_abi.rs:116 and :373 and games/anomaly/src/sr_stage_abi.rs:333 and :513. At games/anomaly/src/sr_stage.cpp:412 the bridge copies every frame field into FrameFacts before games/anomaly/src/sr_swap.cpp:110 passes it on to Rust; at sr_stage.cpp:466 it copies every counter back, mostly by positional aggregate initialization. A new member can remain silently zero on one path. Use one game-local C declaration per payload with typedefs for the existing names, import the shared generated types in both Rust modules and pass/copy each struct directly. [games/anomaly/src/sr_swap_abi.rs:116](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_swap_abi.rs#L116)

- [x] **Duplication | Medium | Use the SR settings binding throughout Anomaly's temporal driver.** The temporal driver manually declares the four fields already generated at games/anomaly/src/sr_stage_abi.rs:40 and copies options.initial field by field at games/anomaly/src/sr_temporal.rs:758. Live updates take another route: games/anomaly/src/sr_stage.cpp:548 copies the same fields into C++ SrSettings and games/anomaly/src/sr_temporal.cpp:93 sends them back to Rust as the manual Settings record. A later field can disappear on either route. Use the common C record/binding for transport and retain the temporal driver's nonzero defaults, equality and admission behavior through explicit helpers. [games/anomaly/src/sr_temporal.rs:145](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_temporal.rs#L145)

- [x] **Quality | Low | Report the current Anomaly exposure and first-person motion settings.** The Stage 5 report reads exposure_auto and first_person_zero from the copy set by rsf_anomaly_sr_configure. The live overlay setters update the stage atomics in sr_stage.rs:1043-1050, and subsequent draws use those atomics while reports keep printing the startup settings. Read the live snapshot when generating the report or propagate changes to the reporting owner. Assert the reported values after toggling each control. [games/anomaly/src/sr_temporal.rs:1125](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/sr_temporal.rs#L1125)

<a id="n08"></a>

### N08 · Anomaly frame generation

Port FG configuration, window and input ownership, and the generation-stage calls and fixtures.

**Coordinate shared edits**

- Anomaly session startup, quiesce and callback retirement ([N01](#n01), [N02](#n02), [N03](#n03), [N04](#n04), [N06](#n06)).
- Motion history, shader identity and published frame resources ([N04](#n04), [N05](#n05), [N07](#n07)).

**Port files (7)**

- [x] [games/anomaly/src/fg_inputs.cpp](games/anomaly/src/fg_inputs.cpp)
- [x] [games/anomaly/src/fg_inputs_platform.h](games/anomaly/src/fg_inputs_platform.h)
- [x] [games/anomaly/src/fg_policy.h](games/anomaly/src/fg_policy.h)
- [x] [games/anomaly/src/fg_policy_platform.h](games/anomaly/src/fg_policy_platform.h)
- [x] [games/anomaly/src/fg_stage.h](games/anomaly/src/fg_stage.h)
- [x] [games/anomaly/src/fg_stage_platform.cpp](games/anomaly/src/fg_stage_platform.cpp)
- [x] [games/anomaly/src/fg_stage_platform.h](games/anomaly/src/fg_stage_platform.h)

**Fixtures (2)**

- [x] [tests/anomaly_fg_device.cpp](tests/anomaly_fg_device.cpp)
- [x] [tests/anomaly_fg_policy.cpp](tests/anomaly_fg_policy.cpp)

**Rust review findings (8)**

- [x] **Altitude | Medium | Distinguish a constant-buffer allocation failure from a permanent shader refusal.** Pipeline::create collapses shader creation and constant-buffer allocation into None. Owner::run turns either into a latched Shader result until device replacement. The refused_constant_buffer_releases_shader_and_latches_on_same_device test clears the simulated allocation failure and confirms that no later dispatch runs. Return the failing operation from pipeline creation, report resource failures as Resources and allow allocation to retry on the same device. [games/anomaly/src/fg_inputs.rs:212](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/fg_inputs.rs#L212)

- [x] **Altitude | Medium | Count tag eligibility separately from completed tagging.** Policy::ui_boundary increments tagged before fg_stage checks that a live swapchain, immediate context or backbuffer exists. Even a frame refused by the first live_chain check increments the published tagged counter without making a final-scene pass call. The stage already exposes runtime_tagged from actual runtime status. Name the adapter counter eligible_at_ui_boundary or increment an accepted-pass counter only after both pass notifications succeed, and use the runtime count for completed tags. [games/anomaly/src/fg_policy.rs:164](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/fg_policy.rs#L164)

- [x] **Simplicity | Medium | Anomaly FG keeps validated Option tables and unwraps them at every call.** `Tables::bind` (fg_stage.rs:77-105) rejects any missing present/FG leaf, but `Tables` stores the raw bindgen structs, so roughly twenty call sites re-open the Option with `.unwrap()`: `fg.set_log/options` and `present.install` (386-388), `is_owner` (407), `cpu` (429), `pass` (450), `bind_source` (454), `inputs_ready` (463), `capture` (535), `dump_inputs` (559), `status` (585), `has_owner/backend/backend_choices` (595-597, 848, 1165-1167), `present_event` (716). The same pattern covers the native `rsf_anomaly_fg_platform` (`sr_owns_depth`, `engine_depth_view`, `depth_texture`, `motion_target`, `immediate_context`, `backbuffer`, `output_window` at 474-506, 1004-1054) and `rsf_anomaly_input_platform` inside fg_inputs.rs (`describe/device/same_device/motion_view/dispatch` unwraps at 146-250 next to `?` checks of the other leaves at 45-48 and 102-104), except those platform tables are never validated at all, so a short native table would panic across `extern "C"`. Build one `Bindings` struct of plain fn pointers at install (from `Tables::bind`, the platform and its `inputs` table, failing install if any is missing) and call fields directly. This removes the unwrap noise and the mixed `?`/`unwrap` styles, and turns the unchecked platform leaves into a single install-time refusal; behavior for complete tables is unchanged. [games/anomaly/src/fg_stage.rs:71](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/fg_stage.rs#L71)

- [x] **Altitude | Medium | Accept FG input resources without requiring a DLSS SR frame.** Anomaly's SR-off input pass produces depth and motion for the selected FG backend. To call the generic D3D11 FG table, the adapter constructs a 488-byte rsf_dlss_frame with unused SR quality, preset, color and exposure fields; camera and identity travel separately in the GameSDK pass. This couples an FG producer to the DLSS SR ABI version and offers two places for dimensions or camera facts to disagree. Add a versioned, vendor-neutral FG resource descriptor to the runtime capture service and adapt existing SR-frame callers at the runtime boundary. [games/anomaly/src/fg_stage.rs:521](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/fg_stage.rs#L521)

- [x] **Altitude | Medium | Keep inferred frame closure out of simulation timing events.** When the next CInput::OnFrame arrives with a different native frame and the previous source never rendered, stage_input_begin emits SIMULATION_END for the old source. Stage::cpu stamps that event with QPC at the new input callback. This gives the latency consumer a simulation-end time that was never observed and can include the whole loading or idle interval. Close the bookkeeping with the skipped-frame fact, and let the runtime mark simulation end unavailable unless a verified end callback supplied it. [games/anomaly/src/fg_stage.rs:877](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/fg_stage.rs#L877)

- [x] **Duplication | Medium | Share X-Ray window identification across the overlay and FG bridge.** The FG bridge callback registered at games/anomaly/src/fg_stage.rs:364 checks HWND validity, current process ID and exact _XRAY_1.5 class before attaching its subclass. games/anomaly/src/overlay.rs:1181 duplicates all three checks and the class name for the overlay paths at :1094 and :1121. A class or admission-rule update must otherwise stay synchronized to prevent the overlay and presentation bridge from selecting different windows. Use one game-local predicate; keep subclass registration and borderless correction exclusively in the FG callback. [games/anomaly/src/fg_stage/window.rs:23](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/fg_stage/window.rs#L23)

- [x] **Altitude | Medium | Tie the cached subclass registration to the window lifetime.** accept_window skips SetWindowSubclass whenever the HWND equals the last successfully subclassed value. The subclass never clears that value on WM_NCDESTROY. If X-Ray recreates its window and Windows reuses the handle, the presentation bridge accepts the new window without its message observer or placement correction. Clear the matching registration when the window dies, or query the actual subclass association instead of relying on a process-wide last HWND. [games/anomaly/src/fg_stage/window.rs:42](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/fg_stage/window.rs#L42)

- [x] **Simplicity | Low | Anomaly FG options round-trip INI values through the native config and several string encodings.** `Options::read` fills an `rsf_anomaly_fg_config` (the C struct for the native `rsf_anomaly_fg_configure` path, with its pointer fields left null) only so `admit` can copy the same fields back out (options.rs:119-135, 173-207). The directory values then travel PathBuf -> `encode_wide` Vec<u16> -> lossy String (146) -> `encode_utf16` Vec<u16> -> `sr_stage::plain_directory` -> lossy String (225-226) -> CString (229), while `from_native` goes wide -> String -> UTF-16 -> String again. Have both entry points produce a small Rust `Settings` value (or call `admit` with typed arguments) instead of the C struct, and keep each directory as UTF-16 until one final `plain_directory` + UTF-8 CString conversion. Same outputs, one fewer mirrored struct and three fewer encoding passes per directory. [games/anomaly/src/fg_stage/options.rs:115](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/anomaly/src/fg_stage/options.rs#L115)

<a id="f01"></a>

### F01 · Fallout 4 startup and CPU ownership

Port session creation, CPU event routing and worker handoff. Address the worker name collision first.

**Coordinate shared edits**

- World and Present identity shared by CPU admission, pass publication and recovery ([F02](#f02), [F03](#f03)).
- Steam chain installation and cold device creation ([S01](#s01), [I01](#i01)).

**Port files (8)**

- [ ] [games/fallout4/src/cpu_events.cpp](games/fallout4/src/cpu_events.cpp)
- [x] [games/fallout4/src/cpu_events.h](games/fallout4/src/cpu_events.h)
- [x] [games/fallout4/src/cpu_events_platform.h](games/fallout4/src/cpu_events_platform.h)
- [x] [games/fallout4/src/fallout4_platform.h](games/fallout4/src/fallout4_platform.h)
- [ ] [games/fallout4/src/host_boundary.cpp](games/fallout4/src/host_boundary.cpp) (typed creation call and foreign exception guards)
- [x] [games/fallout4/src/native_creation.cpp](games/fallout4/src/native_creation.cpp)
- [x] [games/fallout4/src/native_creation.h](games/fallout4/src/native_creation.h)
- [x] [games/fallout4/src/plugin.cpp](games/fallout4/src/plugin.cpp)

**Rust review findings (11)**

- [x] **Quality | High | Fix the worker name collision that prevents the Fallout 4 adapter from compiling.** Inside native_cycle the local worker variable at line 459 has type usize and shadows the module function worker(). The join-retry branch calls worker(), which therefore tries to call an integer. This affects the module included by the Fallout 4 crate even when the branch never executes. Rename the address to worker_address or qualify the lock helper as self::worker(). Identified by source inspection. [games/fallout4/src/cpu_events.rs:511](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/cpu_events.rs#L511)

- [x] **Altitude | Medium | Give every native CPU callback an unwind-safe retirement guard.** raw_enter increments ENTERED and RAW_WRITERS, but observe_raw in cpu_events.cpp:56-63 calls raw_exit only after the native original returns normally. The idle and poll shells likewise leave Rust counters and TLS scopes open on a C++ unwind. If the engine catches that exception outside the detour, later input attribution and quiescence can remain blocked. Use native RAII for unconditional cleanup as observe_swap already does, keeping normal-return observation separate from cancellation cleanup. [games/fallout4/src/cpu_events.rs:1695](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/cpu_events.rs#L1695)

- [x] **Duplication | Medium | Use one Fallout 4 CPU identity for worker handoff and render validation.** The worker ticket's session, source, submission, begin QPC and frequency repeat the generated record at games/fallout4/src/cpu_abi.rs:21. games/fallout4/src/cpu_events.rs:2959 copies all five fields out, :2946 copies them back, and :2747 repeats their equality test. The resolution producer performs this round trip at games/fallout4/src/resolution.rs:483 and :487. Use the generated record as the Rust identity with ZERO and equality support, retaining separate checks for rendered state and nonzero source/submission. Adding an identity field would then automatically reach the handoff and comparison paths. [games/fallout4/src/cpu_events/handoff.rs:10](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/cpu_events/handoff.rs#L10)

- [x] **Duplication | Medium | Share Fallout 4's exact-build renderer address definitions.** CPU handoff reads the renderer pointer and checks its expected object in games/fallout4/src/cpu_events.rs:495 using the RVAs at games/fallout4/src/cpu_events/native.rs:53. Resolution repeats both literals at games/fallout4/src/resolution.rs:124, as do depth-domain reads at games/fallout4/src/depth_domains.rs:758 and :1287. The creation hook at games/fallout4/src/creation.rs:31 shares the pointer RVA but legitimately permits a cold null owner. Keep the verified RVAs and native member offsets in one Fallout 4 build-layout module and import them at each consumer, retaining cold-owner behavior and each probe's guarded read order. [games/fallout4/src/cpu_events/native.rs:53](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/cpu_events/native.rs#L53)

- [x] **Duplication | Medium | Bind Fallout 4 CPU hook targets to their verified entry bytes.** hook_target at games/fallout4/src/cpu_events.rs:100 reads HOOK_RVAS, while guards at games/fallout4/src/cpu_events/native.rs:269 independently repeat the entry addresses. PlayerControls is listed at :18 and its identical nine-byte guard is copied at :339 and :370, so start at games/fallout4/src/cpu_events.rs:3238 checks the same entry twice. Updating only the target table can verify one address and pass a different one to the hook creator. Keep each hook's address with its byte expectation in a single indexed descriptor; reference that descriptor wherever a separate notification preflight needs it and keep caller-site guards distinct. [games/fallout4/src/cpu_events/native.rs:339](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/cpu_events/native.rs#L339)

- [x] **Simplicity | Medium | Fallout 4 and remaining Anomaly leaf tables are validated complete, then every call re-handles a missing leaf.** Same pattern as the AC7 and Anomaly FG/motion entries, in tables not yet listed. Fallout 4: Ops is documented as "every entry is present" after Ops::complete (lib.rs:89), yet all 13 wrappers re-handle None via ok_or(Threw), is_some_and or an E_FAIL arm (lib.rs:105-229), and creation.rs:290/303/333 wraps already non-Option Begin/End/Bind back in Some(..) to pass them through; cpu_events.rs:76 complete() then hook_created/peek_thunk/finish_render_scope/deliver/create/remove re-check (105-118, 651, 3225, 3283); depth_domains.rs:443 and render_inputs.rs:433 Leaves::complete gate every FFI entry (depth_domains.rs:1798, render_inputs.rs:1667), yet each leaf wrapper falls back to zero facts or a skipped call. Anomaly: sr_swap.rs:1249 platform_valid proves 10 leaves, then read/write/set_viewport/upload/mip/make/drop/round_trip/upscale/correct silently return false when None (273-323, 592, 950, 1060, 1080, 1124); sr_temporal.rs:91 validate_api proves 12 leaves, then 10 sites .expect("validated ..."). The dead branches also blur intent: a missing leaf in sr_swap reads as an engine write fault. Simpler: convert once at publish/create into a struct of plain extern fn pointers, as render_hooks.rs:266 (Fallout 4) already does, and call them directly. Tradeoff: tests that build partial tables must supply every leaf (sr_swap/tests.rs and the Fallout 4 fakes already do). [games/fallout4/src/lib.rs:75](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/lib.rs#L75)

- [x] **Duplication | Medium | Centralize the unsafe Win32 typed-export lookup used by Rust adapters.** Fallout 4 resolves host functions at games/fallout4/src/session.rs:248 through a safe generic helper at :93 that accepts a raw name pointer and arbitrary pointer-sized Copy return type. RDR2 uses equivalent GetProcAddress and transmute_copy logic at games/rdr2/src/runtime_worker.rs:159 for the session prepare call at :174. Anomaly adds a NUL check at games/anomaly/src/runtime_module.rs:63, and Unity Mono already accepts &CStr at games/unity-mono/src/mono.rs:88. Use one unsafe resolver over a borrowed live HMODULE and &CStr, documenting the required function signature; keep each adapter's symbol list, ownership and missing-export behavior local. Size assertions alone cannot verify the ABI of the resolved function. [games/fallout4/src/session.rs:93](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/session.rs#L93)

- [x] **Altitude | Medium | Verify the Fallout 4 executable once per preparation transaction.** A successful Session::prepare invokes the full executable hash four times: directly, through verify_with_inputs, again through render_hooks::prepare and finally through cpu_events::prepare. Each invokes file_sha256 on the running executable before startup can proceed. Pass an exact-build admission result to the three subsystem preparation functions and keep live byte, owner and cold-device checks at their respective use sites. [games/fallout4/src/session.rs:213](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/session.rs#L213)

- [x] **Simplicity | Low | Fallout 4 worker ticket transitions take proof flags production always passes as true, plus ignored inputs.** reserve(constructed), arm(native_valid), begin, bind, consumed, complete and join each take a bool that only short-circuits to false; every production call in cpu_events.rs passes the literal true (1275, 1356, 1501, 1551, 1589, 2529, 2715) because the caller already folded its proof into the preceding && chain. Only handoff/tests.rs passes false. protects_bound also takes _revision and _queued that it ignores, so bound_current_valid (cpu_events.rs:894) reads QUEUE_SEQUENCE into a value nobody compares, and predecessor_raw is stored under allow(dead_code) and never read. Simpler: drop the bool and unused parameters and the dead field; callers already refuse before calling, and tests express a failed proof by not calling the transition. Tradeoff: the Rust signatures stop matching the ported worker_handoff.h test vocabulary. [games/fallout4/src/cpu_events/handoff.rs:176](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/cpu_events/handoff.rs#L176)

- [x] **Duplication | Low | Make the Fallout 4 log-message constructor return a C string.** message strips interior NULs and appends a terminator, but returns Vec<u8>. CPU diagnostics in games/fallout4/src/cpu_events.rs:196, cold-creation reporting in games/fallout4/src/creation.rs:312 and DPI reporting in games/fallout4/src/dpi.rs:166 each rescan the constructed bytes with CStr::from_bytes_with_nul before invoking the same host log. Return an owned CString or provide an Ops helper accepting the formatted String so the conversion happens once. Preserve DPI's propagation of a native log failure and the other callers' best-effort behavior. [games/fallout4/src/lib.rs:275](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/lib.rs#L275)

- [x] **Quality | Low | Send Fallout 4 cold-prepare refusals to the requesting host.** Session::prepare routes executable mismatch, cold-owner mismatch, render-guard failure and hook-init failure through refuse before create_host installs self.services. On first prepare self.services.log is null, so the C++ log leaf silently skips the reason even when args.host provided a logger. Pass the current prepare host to the refusal logger and keep the stored reason available through status. [games/fallout4/src/session.rs:122](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/session.rs#L122)

<a id="f02"></a>

### F02 · Fallout 4 render inputs and resolution

Port camera, depth domains, render-input selection and resolution recovery with the matching fixtures.

**Coordinate shared edits**

- World and Present identity shared by CPU admission, pass publication and recovery ([F01](#f01), [F03](#f03)).

**Port files (13)**

- [x] [games/fallout4/src/camera.h](games/fallout4/src/camera.h)
- [ ] [games/fallout4/src/depth_domains.cpp](games/fallout4/src/depth_domains.cpp)
- [x] [games/fallout4/src/depth_domains.h](games/fallout4/src/depth_domains.h)
- [x] [games/fallout4/src/depth_domains_platform.h](games/fallout4/src/depth_domains_platform.h)
- [x] [games/fallout4/src/native_input_state.h](games/fallout4/src/native_input_state.h)
- [ ] [games/fallout4/src/render_inputs.cpp](games/fallout4/src/render_inputs.cpp)
- [ ] [games/fallout4/src/render_inputs.h](games/fallout4/src/render_inputs.h)
- [x] [games/fallout4/src/render_inputs_platform.h](games/fallout4/src/render_inputs_platform.h)
- [ ] [games/fallout4/src/resolution.cpp](games/fallout4/src/resolution.cpp)
- [x] [games/fallout4/src/resolution.h](games/fallout4/src/resolution.h)
- [x] [games/fallout4/src/resolution_platform.h](games/fallout4/src/resolution_platform.h)
- [ ] [games/fallout4/src/screen.cpp](games/fallout4/src/screen.cpp)
- [x] [games/fallout4/src/screen.h](games/fallout4/src/screen.h)

**Fixtures (2)**

- [x] [tests/fallout4_camera.cpp](tests/fallout4_camera.cpp)
- [x] [tests/fallout4_input_state.cpp](tests/fallout4_input_state.cpp)

**Rust review findings (10)**

- [x] **Duplication | Medium | Resolve the Fallout 4 TLS render context consistently across producers.** Temporal sample at games/fallout4/src/depth_domains.rs:741 and raster recovery at :1271 both read the TLS index, block, context and global fallback. Resolution independently repeats the address walk at games/fallout4/src/resolution.rs:361. Raster and resolution reject a null slot array explicitly; the temporal path can attempt to read index*8 from a null base before failing. Extract common checked index/block resolution and context fallback, retaining the depth paths' extra TLS device-context read and preserving the ordering and diagnostic stages. Each sampling point must still take a fresh snapshot. [games/fallout4/src/depth_domains.rs:741](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/depth_domains.rs#L741)

- [x] **Duplication | Medium | Use the same projection-to-pixel jitter conversion for Fallout 4 camera and depth admission.** The camera builder checks that only projection elements 8 and 9 vary at games/fallout4/src/camera.rs:259 and converts those differences to signed half-viewport pixel offsets at :284. Depth composition repeats both operations at games/fallout4/src/depth_domains.rs:1733 and :1736 to validate the submitted camera. A change to either axis convention or viewport scaling can therefore make valid camera frames fail depth admission. Share the pure projection comparison and pixel conversion while keeping separate native snapshots, the camera's magnitude bound, the depth comparison tolerance and stage-specific refusal reporting. [games/fallout4/src/depth_domains.rs:1736](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/depth_domains.rs#L1736)

- [x] **Duplication | Medium | Exercise the production snapshot comparators in the Fallout 4 fixture.** games/fallout4/src/render_inputs.rs:544 and :556 decide which owner and camera fields invalidate the actual render lease at :1193, :1494 and :1628. games/fallout4/src/native_input_state.h:16 and :20 duplicate those decisions for tests/fallout4_input_state.cpp:24, whose poisoned-padding and per-element cases can pass when the production predicate regresses. Keep the C++ receipt layout and its assertions, move the additional per-field cases into the existing Rust test at render_inputs.rs:1839 and, if retaining a native padding fixture, pass its input to the actual Rust comparisons through a fixture shim. [games/fallout4/src/render_inputs.rs:556](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/render_inputs.rs#L556)

- [x] **Quality | Medium | Recheck the temporal colour and motion records before publishing their lease.** read_inputs reads the colour and motion records and acquires their texture references, then rechecks State, which contains only the slot numbers. If a record is replaced in place during acquisition, the unchanged slot numbers pass and the temporal path can submit previous resources or write the old colour target. The raster recovery reader already rereads the actual records at the same mapped slots. Apply that comparison to the temporal reader and cover replacement with unchanged slot numbers; this is a source-level failure condition, not a game observation. [games/fallout4/src/render_inputs.rs:1199](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/render_inputs.rs#L1199)

- [x] **Duplication | Medium | Share the colour, motion and depth lease checks across Fallout 4 input readers.** The temporal and raster entry points called at games/fallout4/src/render_inputs.cpp:240 and :253 independently assemble the same colour/motion/depth slot triples, resolve the three SRVs and validate their associated colour RTV and extents at games/fallout4/src/render_inputs.rs:1066 and :1413. The raster path intentionally restricts depth to R24, checks sample quality and requires the auxiliary surfaces, while the temporal path also admits R32 and reports individual refusal stages. A changed motion format or colour-RTV rule currently needs two edits. Share the leased-view bundle and its validation result, pass the depth/quality requirements explicitly, and leave history, auxiliary admission and reporting in their respective callers. [games/fallout4/src/render_inputs.rs:1413](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/render_inputs.rs#L1413)

- [x] **Quality | Medium | Give a retained Fallout 4 resolution scope a recovery path after the frame advances.** end_output and finish_source require FrameOwner::frame_size_current to match the original native frame even on a cleanup retry. If a restore write fails at source retirement and the frame counter advances, the retry returns before clearing any pending work or decrementing OWNERS. render_admit_world rejects every new world while resolution_busy is true, and render_hooks::stop also refuses. Distinguish a same-frame write retry from a superseded frame: revalidate individual owners and written values at a safe boundary and retire writes that are proved superseded. Exercise a restore failure followed by frame advancement. [games/fallout4/src/resolution.rs:708](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/resolution.rs#L708)

- [x] **Duplication | Medium | Publish the SDK screen-class constants once for Rust adapters.** sdk/game/include/rescaleframe/game_frame.h:58 defines the wire values. games/fallout4/src/screen.rs:16 repeats four ordinals for the classifier at :437, games/unity-mono/src/package_state.rs:55 repeats MENU and FLIGHT for :661, and games/rdr2/src/renderer.rs:42 repeats UNKNOWN and FLIGHT for :337. The producers choose screens for different reasons, but all write the same numeric field, so a stale literal silently names another screen. Expose the typed C macros through the shared rsf-game-sys bindings or a checked constant shim and import those values in each adapter. [games/fallout4/src/screen.rs:16](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/screen.rs#L16)

- [x] **Duplication | Low | Test the motion-scale conversion that Fallout 4 actually submits.** The native fixture and the unit test at games/fallout4/src/camera.rs:675 call the exported function at :425, which derives two negated crop scales itself. The production SR input builder at games/fallout4/src/render_inputs.rs:1332 calls a different conversion at games/fallout4/src/camera.rs:394. Both use raster_transform, but a sign or axis correction can land in just one and leave the tests green. Have the export delegate to motion_to_uv after clearing its output, preserving the existing null and refusal behavior. [games/fallout4/src/camera.rs:425](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/camera.rs#L425)

- [x] **Altitude | Low | Retain the validated depth maps with the temporal snapshot.** capture computes near, world and projection maps, including matrix inversions and forward-W bounds, solely to call is_none and discard them. make later revalidates the live source against that same immutable snapshot and repeats math(snapshot) before using the maps. Store the derived maps with an admitted temporal snapshot and consume them after the live-source checks. Keep the raster-only recovery path and all execution-time identity checks independent. [games/fallout4/src/depth_domains.rs:1612](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/depth_domains.rs#L1612)

- [x] **Duplication | Low | Use one bitwise raster identity check for camera history.** games/fallout4/src/camera.rs:194 compares all four viewport float bit patterns and the integer crop to decide whether camera history is valid at camera.rs:330. games/fallout4/src/render_inputs.rs:585 repeats the predicate on the same CameraRaster type to decide history reset at render_inputs.rs:1256 and same-frame admission at render_inputs.rs:1298. A future raster field or signed-zero change must currently reach both predicates to keep reprojection and reset decisions consistent. Put the bitwise comparison on CameraRaster and keep the signed-zero fixture exercising that shared operation. [games/fallout4/src/render_inputs.rs:585](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/render_inputs.rs#L585)

<a id="f03"></a>

### F03 · Fallout 4 render hook transactions

Port hook lifetime, render scopes, imagespace, publication and rollback in their actual callback order.

**Coordinate shared edits**

- World and Present identity shared by CPU admission, pass publication and recovery ([F01](#f01), [F02](#f02)).

**Port files (3)**

- [ ] [games/fallout4/src/render_hooks.cpp](games/fallout4/src/render_hooks.cpp)
- [x] [games/fallout4/src/render_hooks.h](games/fallout4/src/render_hooks.h)
- [x] [games/fallout4/src/render_hooks_platform.h](games/fallout4/src/render_hooks_platform.h)

**Rust review findings (12)**

- [x] **Duplication | Medium | Derive render callback return addresses from the call sites verified at startup.** games/fallout4/src/render_hooks.rs:138 independently lists the three calls into imagespace and the Effects call. Seams admit raw caller addresses using four separately maintained literals at games/fallout4/src/render_hooks/imagespace.rs:27 and :29. The same split occurs for DR/prepass in games/fallout4/src/render_hooks/producer.rs:35 versus the producer call guard table at games/fallout4/src/render_hooks.rs:111. All of these guards require a five-byte rel32 CALL. Name the call descriptors once and use their computed return RVA both in callback admission and guard fixtures; preserve each caller's separate world, argument and owner checks. [games/fallout4/src/render_hooks/imagespace.rs:27](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/render_hooks/imagespace.rs#L27)

- [x] **Altitude | Medium | Carry Effects-call admission through the matching return.** before_effects rejects a call whose manager differs from the verified engine manager. The native shell always invokes after_effects, which checks only the return address and range arguments before reporting, retiring the current recovery token and draining auxiliaries. The effects_return test passes manager zero and expects a drain. Return a call-scoped admission receipt from entry and use it at exit, including the explicit cleanup policy needed for an already-owned matching source. [games/fallout4/src/render_hooks/imagespace.rs:233](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/render_hooks/imagespace.rs#L233)

- [x] **Duplication | Medium | Pass one screen classification record between the Fallout 4 classifier and publisher.** games/fallout4/src/screen.rs:122 and games/fallout4/src/render_hooks/publication.rs:87 manually define the same five-field NativeScreen ABI and separately initialize its unknown camera sentinel. The native bridge at games/fallout4/src/render_hooks.cpp:277 simply forwards the classifier result into the publication callback's current and leased outputs. Updating either Rust declaration can change camera/fact interpretation while preserving the existing size and final-offset checks. Declare this game-local screen result once, use it on both Rust boundaries, and share the constructor for empty or refused values. [games/fallout4/src/render_hooks/publication.rs:87](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/render_hooks/publication.rs#L87)

- [x] **Altitude | Medium | Return one framebuffer lease with the facts it validates.** The HUD-less publisher calls read_framebuffer, framebuffer and framebuffer_extent separately. Each native leaf reads or overwrites the implicit world.hudless field. FrameLease holds only a callback table, so its Drop clears whichever framebuffer is in that field, and refusal reporting performs another lookup. Have acquisition return the captured identity, extent and owned lease together, and scope release to that acquisition. Preserve partial read diagnostics as part of the same result. [games/fallout4/src/render_hooks/publication.rs:486](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/render_hooks/publication.rs#L486)

- [x] **Duplication | Medium | Generate the reduced-frame transaction ABI once for the host and caller.** Fallout 4's Request and Surface at games/fallout4/src/render_hooks/recovery.rs:98 and :69 and its operation/result flags at :22 reproduce the host's request at runtime/orchestrator/src/d3d11_host_abi.rs:174, flags at :28 and surface at runtime/orchestrator/src/auxiliary_sr_abi.rs:27. games/fallout4/src/render_hooks.cpp:172 forwards the request to the very host that interprets the operation at runtime/orchestrator/src/d3d11_host.rs:1646. A changed flag or same-sized field reordering could make one side release resources the other still holds. Generate the public d3d11_host.h request, surface and constants into a shared Rust ABI module; keep native lease management and the five Fallout 4 surface roles in the adapter. [games/fallout4/src/render_hooks/recovery.rs:98](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/render_hooks/recovery.rs#L98)

- [x] **Duplication | Medium | Share the output-owner record between Fallout 4 recovery and resolution.** Resolution preflight writes the context, manager and three triangle identities at games/fallout4/src/resolution.rs:452 using the local OutputOwner declared at :76. games/fallout4/src/render_hooks.cpp:165 forwards that same object to recovery, which independently declares its layout at games/fallout4/src/render_hooks/recovery.rs:134 and compares it at SEAL and FINISH. Both size checks allow the context and manager fields to be interchanged undetected. Use one game-local repr(C) type for both Rust callers and the same C header declaration for the native bridge, retaining the existing default and whole-record equality. [games/fallout4/src/render_hooks/recovery.rs:134](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/render_hooks/recovery.rs#L134)

- [x] **Altitude | Medium | Make retirement report whether the host released the recovery token.** call returns true for RSF_OK even when RETIRE or CANCEL omits NEXT_REDUCTION_ALLOWED and retains the token and its lease. The refused_retire_retains_token_and_retries_without_finish fixture explicitly expects true with the token still live. Imagespace exit tests only the return value when deciding whether to report pending recovery. Represent released, still owned and failed separately, or make retire return true only once its ownership obligation is gone. [games/fallout4/src/render_hooks/recovery.rs:406](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/render_hooks/recovery.rs#L406)

- [x] **Simplicity | Medium | Fallout 4 render hooks mirror one native World through five partial ABI views.** scope.rs World, producer.rs World, publication.rs World, imagespace.rs World and recovery.rs Facts are each a separate repr(C) projection of the same render_hooks.cpp World, each with its own fill leaf (scope_world, producer_world, publication_world, imagespace_world, recovery facts at render_hooks.cpp:246-385), its own NativeXWorld struct in render_hooks_platform.h, its own size/offset assert block, and its own Leaves::world() that hand-writes a zeroed literal including a five-field CpuIdentity. Most fields recur (cpu, pass, frame_pass, context, inputs, published, frame_open, submission_open, auxiliary_open, refused), and the inputs-then-pass-else-frame_pass choice is re-spelled in imagespace.rs:61, producer.rs:166 and scope.rs:285. Simpler: one NativeWorld view (the union of these fields, about 20) with one fill leaf and one assert block, passed to every seam; a World::state() helper used everywhere. Tradeoff: each seam can see fields it does not use, and the single view grows when any seam needs a new fact, but a new or renamed World field changes one struct pair instead of up to five. [games/fallout4/src/render_hooks/scope.rs:54](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/render_hooks/scope.rs#L54)

- [x] **Duplication | Low | Share the bounded instruction-byte reader used by Fallout 4 hook admission.** CPU hook preflight at games/fallout4/src/cpu_events/native.rs:82 and render hook preflight at games/fallout4/src/render_hooks.rs:150 independently call ReadProcessMemory, check the returned byte count and compare the prefix. The former accepts an RVA and a maximum of 16 bytes; the latter accepts an absolute address, up to 32 bytes for entries, and enforces a separate 16-byte guard limit at :168. A single exact-span reader/comparator with explicit call-site limits avoids independently maintaining short-read and bounds behavior. Keep the per-hook expected bytes, address selection and independent verification points in their existing owners. [games/fallout4/src/render_hooks.rs:150](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/render_hooks.rs#L150)

- [x] **Duplication | Low | Define the resolution-report stage IDs once for all Fallout 4 seams.** The imagespace module assigns four ResolutionStage numbers at games/fallout4/src/render_hooks/imagespace.rs:36, publication assigns two at games/fallout4/src/render_hooks/publication.rs:38, and scope assigns source-retired at games/fallout4/src/render_hooks/scope.rs:33. Their common reporting callback at games/fallout4/src/render_hooks.cpp:296 casts the integers back to the enum in games/fallout4/src/resolution.h:6. Native static assertions check the native ordinals but do not check the separately typed Rust literals. Define a C-compatible stage enumeration once and import its generated constants into the Rust producers and reporter, preserving the numeric reporting ABI. [games/fallout4/src/render_hooks/imagespace.rs:36](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/render_hooks/imagespace.rs#L36)

- [x] **Quality | Low | Report a failed output restore in the Fallout 4 resolution receipt.** When end_output returns false, leave_imagespace calls refuse and immediately emits UI_RESTORED with RSF_OK. publication_report_state forwards that result to the resolution log, so the recorded result contradicts the failed cleanup. Carry the restore outcome through to the receipt and update imagespace_exit_refuses_a_failed_output_end_on_the_frame_pass, which explicitly expects the success bit on failure. [games/fallout4/src/render_hooks/imagespace.rs:188](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/render_hooks/imagespace.rs#L188)

- [x] **Duplication | Low | Share the baseline recovery callback table across Fallout 4 render-seam tests.** Imagespace tests at games/fallout4/src/render_hooks/imagespace/tests.rs:185, producer tests at games/fallout4/src/render_hooks/producer/tests.rs:217, publication tests at games/fallout4/src/render_hooks/publication/tests.rs:276 and scope tests at games/fallout4/src/render_hooks/scope/tests.rs:230 each construct a full RecoveryLeaves table. Even the identical unused adopt, lease and preflight callbacks are reimplemented in each file. These fixtures exercise different callers and record different events for same_source, raster_current or refusal. Provide one test-only baseline for the common callbacks and override the observed or deliberately unreachable slots in each fixture, so extending RecoveryLeaves does not require four sets of unrelated stubs. [games/fallout4/src/render_hooks/imagespace/tests.rs:185](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/fallout4/src/render_hooks/imagespace/tests.rs#L185)

<a id="r01"></a>

### R01 · RDR2 startup, settings and input

Port ASI startup, attach, settings and Runtime session workers with their input and configuration boundaries.

**Coordinate shared edits**

- ASI and renderer session ownership and submitted capture resources ([R02](#r02), [R03](#r03)).

**Port files (9)**

- [x] [games/rdr2/src/asi_entry.cpp](games/rdr2/src/asi_entry.cpp)
- [x] [games/rdr2/src/asi_platform.h](games/rdr2/src/asi_platform.h)
- [x] [games/rdr2/src/attach_entry.cpp](games/rdr2/src/attach_entry.cpp)
- [x] [games/rdr2/src/engine_settings.h](games/rdr2/src/engine_settings.h)
- [x] [games/rdr2/src/engine_settings_policy.h](games/rdr2/src/engine_settings_policy.h)
- [x] [games/rdr2/src/native_settings.cpp](games/rdr2/src/native_settings.cpp)
- [x] [games/rdr2/src/native_settings.h](games/rdr2/src/native_settings.h)
- [x] [games/rdr2/src/runtime_setup.h](games/rdr2/src/runtime_setup.h)
- [x] [games/rdr2/src/runtime_worker_native.h](games/rdr2/src/runtime_worker_native.h)

**Fixtures (1)**

- [x] [tests/rdr2_engine_settings.cpp](tests/rdr2_engine_settings.cpp)

**Rust review findings (9)**

- [x] **Duplication | Medium | Validate loaded PE headers once for both RDR2 modules.** games/rdr2/src/asi.rs:334 and games/rdr2/src/platform.rs:202 read the DOS signature, NT displacement, timestamp and SizeOfImage through equivalent guarded memory callbacks. The ASI uses checked_add_signed and accepts a negative displacement if the resulting address remains representable, while the plugin rejects a negative displacement before addition. ASI ForceD3D12 at asi.rs:356 also needs the NT address for sections, whereas renderer admission at games/rdr2/src/renderer.rs:706 needs only the facts. Share a checked header reader parameterized by the memory callback that returns both the validated NT address and image facts, leaving expected fingerprints and section checks at their callers. [games/rdr2/src/asi.rs:334](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/asi.rs#L334)

- [x] **Boundaries | Medium | Give the RDR2 startup and diagnostic INI keys saved overlay controls.** arm() and attach() read [RDR2] Enable, ForceD3D12, NativeCapture, CameraProbe, CameraProbeDirectory, CameraReference, CameraReferenceLibrary, FrameProbe, FrameProbeLibrary, FrameProbePrefix, Census, CensusDirectory, CensusFlags and TraceD3D12 through GetPrivateProfile* leaves. runtime_worker.rs:228 and :287-302 add NativeSettingsProbe, NativeSettingsProbePath, DumpModule and DumpDirectory. None of these keys has an overlay row through the overlay service, and the shared settings store does not own them, so each change needs an INI edit and a restart. Rule 2 explicitly covers diagnostic and experimental options: a restart-only option still needs a control that says so and saves the value. Describe these keys as restart-required adapter options through the overlay service, persisted in the shared settings store, and have the ASI read the stored values at the next start. [games/rdr2/src/asi.rs:452](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/asi.rs#L452)

- [x] **Quality | Medium | Retry RDR2 adapter detection after a failed DXGI query.** The settings hook caches (adapter index, vendor) even when the native adapter_vendor leaf returns zero because factory creation, enumeration or GetDesc1 failed. Subsequent loads/applies for that index skip detection permanently and pin the FSR2 carrier even on NVIDIA after DXGI becomes available. Keep the immediate fallback, but only cache a successful vendor identity or retry a cached zero. Test a failed lookup followed by a successful lookup for the same index. [games/rdr2/src/engine_settings.rs:205](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/engine_settings.rs#L205)

- [x] **Altitude | Medium | Publish a quality selection only after both pending settings collections accept it.** pin_pending publishes SELECTION, invokes own twice and unconditionally returns true. own returns silently on an unrecognized collection version. The quality tick then marks PENDING_INITIALIZED and can request reconfiguration with only one or neither collection updated; later ticks skip the unchanged request. Validate both collections and prepare their changes together, report refusal to the tick, and publish the selection only on successful application. [games/rdr2/src/engine_settings.rs:424](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/engine_settings.rs#L424)

- [x] **Quality | Medium | Retain a refused RDR2 plugin session that still owns cleanup.** Both ASI startup and explicit attachment use run_session. It discards the local session pointer on every non-OK prepare. plugin_session.h permits BUSY with a nonnull inactive session and requires retrying quiesce/stop. Losing that handle can permanently block another attachment or preparation for the module. Keep the returned owner and drain it before releasing it or allowing another start; likewise account for start failure after a successful prepare. [games/rdr2/src/runtime_worker.rs:258](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/runtime_worker.rs#L258)

- [x] **Duplication | Low | Keep a single set of engine quality and refresh test vectors.** games/rdr2/src/engine_settings/tests.rs:14, :45 and :105 and tests/rdr2_engine_settings.cpp:9, :18 and :49 repeat the same capability lists, four vendors, five quality expectations and 31550000/262912 refresh repair. Both reach the Rust policies at games/rdr2/src/engine_settings.rs:33, :60 and :100. The native test also independently checks byte offsets and the C++ default-argument bridge. Keep those ABI checks and one call per export in native code, and maintain the complete vendor/quality/refresh matrix in a single fixture shared with Rust, so a future measured mode only needs one expected vector. [games/rdr2/src/engine_settings/tests.rs:14](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/engine_settings/tests.rs#L14)

- [x] **Altitude | Low | Report settings-probe RVAs only for observed addresses in the game image.** The settings-value and row detours turn unreadable pointers into zero and subtract the image base with wrapping arithmetic. The JSON can therefore name a very large vtable_rva or getter_rva for a failed read, or mislabel an address in another DLL as an RDR2 offset. Keep the optional addresses through serialization, emit a module-relative offset only after checking the image bounds, and distinguish external addresses from unreadable ones. [games/rdr2/src/native_settings.rs:170](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/native_settings.rs#L170)

- [x] **Duplication | Low | Share the Windows carriers' tick-prefixed file logger.** games/rdr2/src/runtime_worker.rs:46 and games/project-wingman/src/carrier.rs:51 both use a process-local path, a mutex, append-open, GetTickCount64, raw message bytes and a newline. The host callback adapters at runtime_worker.rs:58 and carrier.rs:63 also repeat the same null check and CStr conversion. Parameterize one small logger by its path with one instance in each carrier so future line-format or write fixes apply to both. Keep each adapter's filename, initialization timing and messages local. [games/rdr2/src/runtime_worker.rs:46](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/runtime_worker.rs#L46)

- [x] **Simplicity | Low | RDR2 run_session branches on an Option mode argument at seven points to serve two callers.** run_session takes asi: Option<(HMODULE, &rsf_rdr2_runtime_leaves)> only to tell the ASI start from the research attach start, then tests it at lines 177, 188, 216, 222, 247, 255 and 259: three refusals log only in ASI mode, the native settings probe and ScriptHook start run only in ASI mode, the log prefix differs, and the exit code is prepared/started in ASI mode but 1/0 in attach mode. Simpler: a shared helper that resolves exports, fingerprints, fills the probe and services, creates the host and runs prepare/start, returning Result<(), Refusal> with the refusal text and raw result; rsf_rdr2_asi_start_runtime and rsf_rdr2_attach_start_runtime each log and map that result and run their own extras (settings probe between create and prepare is the one ordering point, passed as an optional hook closure). Tradeoff: the settings probe must still run between host creation and prepare, so the ASI caller supplies that step as a callback rather than inline code. [games/rdr2/src/runtime_worker.rs:165](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/runtime_worker.rs#L165)

<a id="r02"></a>

### R02 · RDR2 camera, renderer and presentation

Port camera and renderer facts, frame routing and presentation observation. Repair the private-field fixture construction first.

**Coordinate shared edits**

- ASI and renderer session ownership and submitted capture resources ([R01](#r01), [R03](#r03)).

**Port files (9)**

- [x] [games/rdr2/src/camera_probe.cpp](games/rdr2/src/camera_probe.cpp)
- [x] [games/rdr2/src/camera_probe.h](games/rdr2/src/camera_probe.h)
- [x] [games/rdr2/src/native_platform.cpp](games/rdr2/src/native_platform.cpp)
- [x] [games/rdr2/src/native_platform.h](games/rdr2/src/native_platform.h)
- [x] [games/rdr2/src/plugin.cpp](games/rdr2/src/plugin.cpp)
- [x] [games/rdr2/src/render_camera.h](games/rdr2/src/render_camera.h)
- [x] [games/rdr2/src/social_club.cpp](games/rdr2/src/social_club.cpp)
- [x] [games/rdr2/src/social_club_guard.cpp](games/rdr2/src/social_club_guard.cpp) (MSVC SEH around Social Club COM call)
- [x] [games/rdr2/src/social_club.h](games/rdr2/src/social_club.h)

**Fixtures (1)**

- [x] [tests/rdr2_camera.cpp](tests/rdr2_camera.cpp)

**Rust review findings (12)**

- [x] **Quality | High | Construct the RDR2 renderer fixture without accessing private ResourceFacts padding.** The sr_pass test uses struct update syntax on platform::ResourceFacts from the sibling renderer module. That construction requires access to every omitted field, including platform-private _reserved, and prevents compilation of the RDR2 Rust unit tests. Start from ResourceFacts::default() and assign its crate-visible fields, or provide a constructor in platform. Identified from Rust field visibility; tests were not run. [games/rdr2/src/renderer/tests.rs:73](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/renderer/tests.rs#L73)

- [x] **Altitude | Medium | Preserve failure to identify the worker TLS slot in the camera capture.** Snapshot::read_worker replaces a failed TLS index read with zero and continues through slot 0. If that slot and its view field are readable, the capture marks unrelated bytes as valid_view. tools/analyze-rdr2-camera-probe.py:21 accepts the record when the parameters are also readable. Require a successfully read in-range index and a non-null slots base before following the pointer chain; leave valid_view clear on failure. [games/rdr2/src/camera_probe.rs:105](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/camera_probe.rs#L105)

- [x] **Quality | Medium | Report camera-capture write failures before consuming the whole burst.** The upscaler callback ignores fs::write and always advances the index and decrements remaining. A nonexistent camera output directory or a full disk can consume all 64 requested records with no usable camera evidence and no indication of failure. Check the write result, distinguish failed attempts from saved records, and report the path and error once or end the burst with an explicit failure. [games/rdr2/src/camera_probe.rs:266](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/camera_probe.rs#L266)

- [x] **Altitude | Medium | Derive FSR camera flags from the observed context creation.** The FSR dispatch adapter always sets the normalized flags to NGX_DEPTH_INVERTED. sample consequently accepts inverted depth and process marks motion unjittered for every FSR context. The fsr_create hook already receives the actual context flags but logs only the first four calls. Store the relevant translated flags for each successful creation, invalidate replaced contexts and decline unsupported or unobserved conventions instead of publishing fixed camera facts. [games/rdr2/src/renderer.rs:491](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/renderer.rs#L491)

- [x] **Quality | Medium | Keep a forced FSR history reset local to the fallback dispatch.** After an RSF frame, the FSR fallback casts the const dispatch description to mutable and sets its reset byte, then returns without restoring it. A reused description can keep resetting native history, and writing a read-only description would fault. Use a writable copy covering the verified full 2.2 description, including its tail, for this call. Test that native FSR receives the forced reset, caller bytes stay intact and extension fields survive. [games/rdr2/src/renderer.rs:501](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/renderer.rs#L501)

- [x] **Duplication | Medium | Centralize the history transition when either RDR2 upscaler falls back.** The DLSS path at games/rdr2/src/renderer.rs:412 and FSR path at :495 both set native_gap after a host replacement, require a native reset on the next fallback, log a changed result, clear the gap after native success and invalidate the RSF frame. The history test at games/rdr2/src/renderer/tests.rs:36 checks only its initial state. Put that transition on History and cover replacement, failed fallback, successful retry and next-frame invalidation once. Keep the NGX status mask versus FFX_OK test, native argument writes and logging text at their respective call sites. [games/rdr2/src/renderer.rs:508](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/renderer.rs#L508)

- [x] **Altitude | Medium | Revalidate the queue and swapchain that qualify an RDR2 SR pass.** attach_present stops reading the game's active swapchain after its first attachment and attach_queue ignores every later candidate. process admits SR whenever both stored addresses are nonzero. If an engine reconfiguration replaces either object, evaluation can be accepted while submission or Present is observed only on the retired object. Track the active pair and its generation, fall back on mismatch, and require matching observations before admitting SR on the replacement. [games/rdr2/src/renderer.rs:599](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/renderer.rs#L599)

- [x] **Altitude | Medium | Observe RDR2 input independently of the host's pacing decision.** The sleep detour calls input_thread only when process_latency accepts the sleep. That call is the only place that installs the window/controller observers and assigns INPUT_SOURCE. If the optional sleep service is absent or declines ownership, native pacing proceeds but the adapter never installs the observers on that path, and controller_received drops unkeyed reads. Establish input observation from the verified game-thread event regardless of sleep ownership; use the callback result only to decide whether to forward native sleep. [games/rdr2/src/scene.rs:206](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/scene.rs#L206)

- [x] **Duplication | Medium | Use one resource-free render-pass copy for retained frame identity.** RDR2 copies the complete SR pass into retained state at games/rdr2/src/scene.rs:613 then clears ten borrowed resource pointers. Fallout 4 copies its FG input pass at games/fallout4/src/render_hooks/publication.rs:426 and additionally clears native_frame_context and temporal_mask, both pointer fields in sdk/game/src/lib.rs:331 and :333. RDR2's helper would retain either non-null field from its input after the callback lease. Put the resource-free copy in the SDK alongside the pass type, use it at both retention sites, and test with every borrowed pointer populated. Preserve scalar identities and camera data; this finding does not require either pointer to be populated by today's RDR2 producer. [games/rdr2/src/scene.rs:230](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/scene.rs#L230)

- [x] **Quality | Medium | Match the RDR2 WINDOW receipt to the backbuffer that received the completed scene.** completed_scene records the exact target resource in saved.surface. At PRESENT_END, marker checks only that it is nonzero, retrieves the swapchain current index and passes that buffer to the host without comparing identities. If Present has already advanced the index, the previous frame camera and source are published with the next buffer. Capture the buffer identity at the actual Present boundary, compare it to the blit target, and withhold the association when they differ. Include a rotating two-buffer fixture. [games/rdr2/src/scene.rs:432](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/scene.rs#L432)

- [x] **Duplication | Low | Use one RAGE camera fixture for the Rust decoder and its C++ ABI test.** games/rdr2/src/camera/tests.rs:33 and tests/rdr2_camera.cpp:36 independently construct the same 0x800-byte view with the same native offsets, matrices, jitter and dimensions. Both then test the same static point, reset and refusal cases against the same Rust decoder through games/rdr2/src/camera.rs:218. A changed view layout requires editing both constructions, and passing both suites still covers just one trajectory. Share a versioned synthetic fixture and expected values, keep detailed reprojection and refusal coverage in Rust, and keep a focused native ABI call plus the C++ capture-file CLI. [games/rdr2/src/camera/tests.rs:33](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/camera/tests.rs#L33)

- [x] **Duplication | Low | Use the ASI status formatter for Social Club hook results.** games/rdr2/src/social_club.rs:98 and games/rdr2/src/asi.rs:200 both call the same AsiPlatform.hook_status leaf, check for null and convert the returned C string to owned text. Social Club calls its private copy at :164 and :185, whereas the D3D12 trace uses the existing method at asi.rs:424. An unknown status is printed as a bare number in one log and with MH_STATUS in the other. Use AsiPlatform::status_text for all three messages and retain its labeled numeric fallback. [games/rdr2/src/social_club.rs:98](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/social_club.rs#L98)

<a id="r03"></a>

### R03 · RDR2 capture and diagnostics

Port capture observation, registration, GPU ownership and publication alongside the capture fixture.

**Coordinate shared edits**

- ASI and renderer session ownership and submitted capture resources ([R01](#r01), [R02](#r02)).

**Port files (6)**

- [x] [games/rdr2/src/capture_ngx.cpp](games/rdr2/src/capture_ngx.cpp) (CRT and NGX exception leaves; now the C11 leaf `capture_ngx.c`, SEH replaces `catch (...)`)
- [x] [games/rdr2/src/capture_state.h](games/rdr2/src/capture_state.h)
- [x] [games/rdr2/src/d3d12_trace.h](games/rdr2/src/d3d12_trace.h)
- [x] [games/rdr2/src/diagnostics.h](games/rdr2/src/diagnostics.h)
- [x] [games/rdr2/src/native_capture.cpp](games/rdr2/src/native_capture.cpp)
- [x] [games/rdr2/src/native_capture.h](games/rdr2/src/native_capture.h)

**Fixtures (2)**

- [x] [tests/rdr2_capture_ngx.cpp](tests/rdr2_capture_ngx.cpp) (native NGX fixture object; now the C11 leaf `rdr2_capture_ngx.c` calling the NgxParam `Set` vtable slots)
- [x] [tests/rdr2_capture.cpp](tests/rdr2_capture.cpp)

**Rust review findings (11)**

- [x] **Altitude | Medium | Detach completed capture metadata before writing the RDR2 bundle.** The native poller takes g->mutex at native_capture.cpp:727 and calls this export, which writes camera files, formats up to 4096 draw records, writes and renames the manifest, and logs before returning. Present takes that same mutex even after the capture has drained, so filesystem delays can stall presentation. Transfer an owned bundle snapshot and tail lease to the poller under the mutex, publish after unlocking, and finalize the matching sequence under the lock. [games/rdr2/src/capture_state/bundle.rs:100](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/capture_state/bundle.rs#L100)

- [x] **Altitude | Medium | Keep the target identity of enabled RDR2 capture hooks.** The registry collapses each enabled slot to Hook::Enabled and returns before reading the next object's target. native_capture.cpp:539 binds the same draw and reset roles for every created command list, so a second vtable implementation silently misses observation while the registry still reports the first install. Record the enabled target and compare subsequent objects; either support target-specific original dispatch or report the unobserved implementation in capture completeness. [games/rdr2/src/capture_state/hooks.rs:86](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/capture_state/hooks.rs#L86)

- [x] **Altitude | Medium | Keep unavailable pipeline observations explicit in capture artifacts.** remember_pipeline in native_capture.cpp:112 passes null for an unrecognized descriptor and Rust removes those facts. get_draw_binding nevertheless succeeds for a bound target and fills Pipeline with zeros; the draw bundle and image JSON then serialize shader CRCs, blend state and write mask as observed values. Carry a pipeline-known flag or optional pipeline snapshot from binding to serialization and mark these fields unavailable while retaining the draw and PSO identity. [games/rdr2/src/capture_state/recordings.rs:274](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/capture_state/recordings.rs#L274)

- [x] **Quality | Medium | Invalidate capture state after an aliasing barrier with no named destination.** native_capture.cpp:303 forwards pResourceAfter for every aliasing barrier. D3D12 permits a null destination to cover any alias, but capture_alias does nothing for null, so state() can still return an earlier concrete state. The NGX capture path at native_capture.cpp:423 uses that value to insert readback barriers. Record a wildcard invalidation for that command list, propagate it through submission in command order, and refuse copies until the resource state is explicitly observed again. [games/rdr2/src/capture_state/recordings.rs:335](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/capture_state/recordings.rs#L335)

- [x] **Altitude | Medium | Retire capture copies discarded by a command-list reset.** A shot recorded on a list that is reset before submission keeps its old generation forever. reset_list advances the generation, before_submit can no longer claim that shot, and collect_ready requires a completed fence. Even after timeout the pending entry keeps drained false, blocking publication and every future capture. At successful reset, detach provably unsubmitted, unclaimed receipts from the abandoned generation and mark their artifacts unavailable; preserve claimed or submitted leases until completion proof. [games/rdr2/src/capture_state/shots.rs:290](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/capture_state/shots.rs#L290)

- [x] **Quality | Medium | Retire failed RDR2 readbacks when the fence reports device removal.** GetCompletedValue returning UINT64_MAX is a terminal device-removal signal. Readback::is_complete rejects it and collect_ready keeps the shot in pending exactly as for an unfinished fence. Timeout only clears active/inputs_remaining, so can_publish and can_arm remain false and the old buffer, fence, queue or backbuffers stay held across device replacement. Classify terminal device loss separately, record the readback failure and retire its leases after confirming removal without mapping the buffer. [games/rdr2/src/capture_state/shots.rs:290](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/capture_state/shots.rs#L290)

- [x] **Duplication | Medium | Apply one foreground F10 edge rule to both RDR2 capture workers.** games/rdr2/src/diagnostics.rs:175 tracks the previous value of foreground AND F10 for camera/RenderDoc requests. games/rdr2/src/capture_state.rs:137 tracks raw F10 first and requires foreground only on a physical rising edge for the native readback poller called at games/rdr2/src/native_capture.cpp:710. Holding F10 while switching into the game can therefore trigger a camera/RenderDoc capture without the matching GPU capture even when both pollers observed the key beforehand. Share a tested edge transition for both workers, keep their request files and capture actions separate, and cover focus changes while the key stays down. [games/rdr2/src/diagnostics.rs:175](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/diagnostics.rs#L175)

- [x] **Duplication | Low | Use the same bounded directory admission for RDR2 camera and GPU capture.** games/rdr2/src/asi.rs:472 and :518 read CameraProbeDirectory for the two installers. games/rdr2/src/camera_probe.rs:154 and games/rdr2/src/capture_state/install.rs:114 independently scan the UTF-16 pointer for a terminator within 196 units. The camera path rejects length zero, while the native-capture entry called at games/rdr2/src/native_capture.cpp:774 builds root-relative request and log paths from an empty input. Normal INI startup already filters empty values. Use one adapter-local nonempty bounded directory reader for both entry points and test the empty, maximum and unterminated cases once. [games/rdr2/src/capture_state/install.rs:114](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/capture_state/install.rs#L114)

- [x] **Duplication | Low | Reuse the capture recorder's fallible map entry helper for swapchain metadata.** games/rdr2/src/capture_state/metadata.rs:183 looks for an existing pointer key, reserves one slot only if absent and returns entry.or_default for the HDR and colour-space callbacks at :208 and :223. games/rdr2/src/capture_state/recordings.rs:96 provides exactly this operation for list bindings and RTV entries at :220 and :237. Both deliberately allow an existing key to update even under allocation pressure. Move try_entry to capture_state and call it from both owners to keep that failure behavior consistent. [games/rdr2/src/capture_state/metadata.rs:183](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/capture_state/metadata.rs#L183)

- [x] **Altitude | Low | Capture NGX scalar metadata as one observation.** native_capture.cpp:472 enumerates Rust-owned keys across FFI, branches on a returned type flag, reads the corresponding native NGX parameter, and crosses back into Rust for each of 18 fields. Each write searches the same temporal records again by InputId. Let the metadata owner resolve the input once and fill its scalar array in one operation using a typed native read callback that preserves the signed fallback and explicit unavailable values. [games/rdr2/src/capture_state/metadata.rs:419](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/capture_state/metadata.rs#L419)

- [x] **Altitude | Low | Report whether the frame-probe request worker actually started.** rsf_rdr2_start_frame_probe returns the native initialization result even when CreateThread fails. asi_entry.cpp:39 forwards that result and asi.rs reports frame-probe success while no worker can consume the request file or F10. Return a distinct worker-start failure after successful initialization and include it in the existing startup report. [games/rdr2/src/diagnostics.rs:201](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/rdr2/src/diagnostics.rs#L201)

<a id="s01"></a>

### S01 · Skyrim startup, CPU input and compatibility

Port the cold creation routes, session, event proofs and compatibility handoff.

**Coordinate shared edits**

- Steam chain installation and cold device creation ([F01](#f01), [I01](#i01)).
- Session retirement and the callbacks consuming Skyrim render inputs ([S02](#s02)).

**Port files (12)**

- [x] [games/skyrim-se/src/compatibility.cpp](games/skyrim-se/src/compatibility.cpp) (now the C11 leaf `compatibility.c`)
- [x] [games/skyrim-se/src/compatibility.h](games/skyrim-se/src/compatibility.h)
- [x] [games/skyrim-se/src/cpu_events.cpp](games/skyrim-se/src/cpu_events.cpp) (now the C11 leaf `cpu_events.c`)
- [x] [games/skyrim-se/src/cpu_events.h](games/skyrim-se/src/cpu_events.h)
- [x] [games/skyrim-se/src/cpu_policy.h](games/skyrim-se/src/cpu_policy.h)
- [x] [games/skyrim-se/src/input_ownership.h](games/skyrim-se/src/input_ownership.h)
- [x] [games/skyrim-se/src/native_creation.cpp](games/skyrim-se/src/native_creation.cpp) (now the C11 leaf `native_creation.c`)
- [x] [games/skyrim-se/src/native_creation_platform.h](games/skyrim-se/src/native_creation_platform.h)
- [x] [games/skyrim-se/src/observer_platform.h](games/skyrim-se/src/observer_platform.h)
- [x] [games/skyrim-se/src/plugin.cpp](games/skyrim-se/src/plugin.cpp) (already the C11 `plugin.c` since 7f28d061)
- [x] [games/skyrim-se/src/session_platform.h](games/skyrim-se/src/session_platform.h)
- [x] [games/skyrim-se/src/guarded_read.cpp](games/skyrim-se/src/guarded_read.cpp) (native SEH read and write guards)

**Fixtures (1)**

- [x] [tests/skyrim_input.cpp](tests/skyrim_input.cpp)

**Rust review findings (13)**

- [x] **Altitude | Medium | Keep repeated initializer lookups under the same Community Shaders handoff.** The GetProcAddress detour replaces SKSEPlugin_Load only while the phase is Armed. The first lookup sets Captured, so a second lookup of the same module and function before invocation receives the unwrapped entry and can initialize without exclusion or withdrawal. In Captured, return the same wrapper for the same original/module identity and make the initializer transaction enforce its single-invocation rule. [games/skyrim-se/src/compatibility/initializer.rs:262](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/compatibility/initializer.rs#L262)

- [x] **Duplication | Medium | Centralize Skyrim's shared frame, window and target-table addresses.** games/skyrim-se/src/camera/render.rs:11, games/skyrim-se/src/cpu_policy.rs:31 and games/skyrim-se/src/presentation.rs:20 each spell out the same frame/window RVAs. The camera reader at camera/render.rs:99, CPU identity producer at cpu_policy.rs:364 and presentation recheck at presentation.rs:194 must agree for the same frame. games/skyrim-se/src/auxiliary.rs:11 and games/skyrim-se/src/render_inputs.rs:18 likewise repeat the same target base and 0x30 stride for different slots. Put these verified addresses and the target-index operation in the existing Skyrim layout module. Preserve each owner's live reads and checks so a build update changes each native fact in one place. [games/skyrim-se/src/cpu_policy.rs:31](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/cpu_policy.rs#L31)

- [x] **Altitude | Medium | Restore the screen classification with a nested Skyrim render scope.** cpu_events.cpp:743 saves only the previous scope ID before begin_render changes both scope and screen. end_render restores just the ID. If a native render callback nests, a parent pass subsequently receives the child's screen via render_identity, which also uses it to determine FG eligibility. Have begin_render return a receipt for both scope-local values and restore them together while retaining frame-wide counters. [games/skyrim-se/src/cpu_policy.rs:295](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/cpu_policy.rs#L295)

- [x] **Quality | Medium | Commit poll completion atomically with the Skyrim overlap check.** An exclusive poll can pass the conflicts check at end_poll, then pause before storing its generation. A poll on another thread can enter and finish in that interval, increment conflicts and store zero. The first poll then overwrites zero with its old generation. Once both writers leave, epochs.snapshot and the poll_sample_status caller in cpu_events.cpp:652 can report a sampled source despite the overlap. Serialize poll begin/end/invalidation bookkeeping or use a generation-checked atomic publication, and test this interleaving with synchronized threads. [games/skyrim-se/src/cpu_sources.rs:188](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/cpu_sources.rs#L188)

- [x] **Duplication | Medium | Use the same cold renderer-owner predicate for Skyrim creation and compatibility.** games/skyrim-se/src/creation.rs:166 reads OWNER and OWNER_DEVICE to decide Steam preparation at :187. games/skyrim-se/src/compatibility.rs:176 repeats that pointer chain to decide whether the original Community Shaders initializer may run at games/skyrim-se/src/compatibility/initializer.rs:290 and :336. Both accept a missing owner or device and refuse unreadable memory, but only the latter rejects zero module base explicitly, uses checked addition and preserves read failure as None. Put the checked observation beside the shared owner offsets, accepting a guarded read callback. Both admission callers can then require Some(true). [games/skyrim-se/src/creation.rs:166](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/creation.rs#L166)

- [x] **Duplication | Medium | Use one D3D11 creation-scope and native-chain binding on both sides of device creation.** games/fallout4/src/platform_abi.rs:132 and :157 contain generated chain and scope records. Skyrim manually defines the identical wire objects at games/skyrim-se/src/platform.rs:28 and :50, initializes them at games/skyrim-se/src/creation.rs:304, and lends the final device/context/swapchain fields on return. Runtime interprets a third pair at runtime/orchestrator/src/d3d11_host_abi.rs:110 and :119 through d3d11_host.rs:2447 and :2480. The manual Skyrim checks cover record sizes but could miss same-sized pointer fields exchanged within the chain. Share the generated public records and keep adapter-specific call frames private. [games/skyrim-se/src/platform.rs:28](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/platform.rs#L28)

- [x] **Duplication | Medium | Use one public D3D11 host setup binding for adapters and runtime.** games/fallout4/src/platform_abi.rs:61 already has the generated 160-byte rsf_d3d11_host_setup used for creation at games/fallout4/src/session.rs:201. Skyrim builds the same payload with handwritten HostSetup and nested MotionDecode at games/skyrim-se/src/session.rs:110 and :529. The runtime interprets it using a third definition at runtime/orchestrator/src/d3d11_host_abi.rs:58 before validation at runtime/orchestrator/src/d3d11_host.rs:2273. The two manual versions depend on selected offset assertions and can silently reinterpret same-size fields after a header change. Generate the public setup, motion parameters and callback signature once from d3d11_host.h in a shared ABI crate. Keep adapter settings and private platform callbacks local. [games/skyrim-se/src/session.rs:110](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/session.rs#L110)

- [x] **Altitude | Medium | Propagate the Runtime's Skyrim host activation result.** Leaves::set_active returns Ok whenever the native wrapper did not throw, irrespective of the returned status. The Runtime returns INVALID for an unknown host. Session::start can consequently enable admission for an inactive host, and suspend_host can report success after a refused deactivation. Preserve both the exception and Runtime result and publish the new lifecycle state only after RSF_OK. [games/skyrim-se/src/session.rs:245](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/session.rs#L245)

- [x] **Altitude | Medium | Keep failed Skyrim subsystem rollback visible to the session owner.** Stage::retire tests is_ok() on Result<bool, Threw>, so normal false from a busy presentation or sampling stop, or a refused CPU disable, counts as successful cleanup. Creation discards stop's boolean too. After a later stage fails, Session::start can report a clean refusal while a prior subsystem still owns live hooks or leases. Track outstanding rollback stages and return BUSY until each stop acknowledges release; exercise a false stop separately from a thrown stop. [games/skyrim-se/src/session.rs:376](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/session.rs#L376)

- [x] **Duplication | Medium | Bind Skyrim CPU services without repeating native hook preparation.** Session::prepare calls CPU_PREPARE first with the upstream host at games/skyrim-se/src/session.rs:476, then with the bound runtime services at :574. games/skyrim-se/src/cpu_events.cpp:762 routes both to the same Control::prepare, which rereads the named callback and 13 hook guards at games/skyrim-se/src/cpu_hooks.rs:227, requeries clock frequency at :248 and overwrites the saved services. The second call needs the bound callbacks but the hooks are already prepared and pinned. Separate native preparation from service binding, publish the final table once, and retain the independent live admission check at cpu_hooks.rs:286 before enabling. [games/skyrim-se/src/session.rs:574](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/session.rs#L574)

- [x] **Quality | Medium | Do not report a successful Skyrim restart while admission is withdrawn.** start returns OK whenever self.enabled is true. After a successful start, quiesce clears ACCEPTING and quiesces the subsystems but leaves enabled true, so a caller invoking the public start hook again gets success while status.active stays zero. An activation exception also leaves enabled true before returning NOT_READY, causing the same false success on retry. Distinguish installed-but-quiesced hooks from an accepting session and either resume all required stages or return an explicit refusal until stop/prepare. [games/skyrim-se/src/session.rs:597](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/session.rs#L597)

- [x] **Simplicity | Low | Skyrim threads the one static platform table through every export and re-validates it per call.** platform.rs already has register/get for the process-lived table, but about twenty exports (auxiliary, compatibility/initializer, cpu_hooks, cpu_policy, plugin, postprocess, presentation, render_inputs, render_observer, resolution, sampling) take platform: *const Platform and call platform::inspect on every call, re-reading the size/version header (30 inspect/get sites across 14 files). Every C++ caller passes the same creation_platform() static (sampling.cpp:72, resolution.cpp:94, render_inputs.cpp:134/138, presentation_inputs.cpp:97, plugin.cpp:218, auxiliary.cpp:139), and session::publish already inspects leaves.platform before any lifecycle call, yet register only runs late in Session::prepare, so earlier exports cannot use get() and cpu_policy.rs:236 has to explain "the upstream CPU prepare runs before session registration of this same platform". Simpler: register the table once in session::publish, drop the platform parameter from the exports, and read platform::get() (or take &Platform internally). Tradeoff: the C++ export signatures change, and a fixture that wants a private table must publish it first instead of passing it per call. [games/skyrim-se/src/platform.rs:161](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/platform.rs#L161)

- [x] **Duplication | Low | Use one identity predicate for reusing a process-lived D3D11 host.** games/skyrim-se/src/session.rs:296 and games/fallout4/src/session.rs:100 compare exactly four fields of the same rsf_game_host_services type: session ID, user pointer, log callback and CPU-event callback. Skyrim uses its copy to admit a cached host at :553 and Fallout uses its copy at :260. Adding a callback that participates in rebinding identity currently requires finding and updating both predicates. Give the shared D3D11 host support code a named upstream-identity comparison, preserve the existing four-field contract, and test each field independently. [games/skyrim-se/src/session.rs:296](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/session.rs#L296)

<a id="s02"></a>

### S02 · Skyrim rendering and presentation

Port camera and render inputs, presentation, resolution, sampling and auxiliary render effects.

**Coordinate shared edits**

- Session retirement and the callbacks consuming Skyrim render inputs ([S01](#s01)).

**Port files (14)**

- [x] [games/skyrim-se/src/auxiliary.cpp](games/skyrim-se/src/auxiliary.cpp)
- [x] [games/skyrim-se/src/auxiliary.h](games/skyrim-se/src/auxiliary.h)
- [x] [games/skyrim-se/src/camera.h](games/skyrim-se/src/camera.h)
- [x] [games/skyrim-se/src/postprocess_platform.h](games/skyrim-se/src/postprocess_platform.h)
- [x] [games/skyrim-se/src/presentation_inputs.cpp](games/skyrim-se/src/presentation_inputs.cpp)
- [x] [games/skyrim-se/src/presentation_inputs.h](games/skyrim-se/src/presentation_inputs.h)
- [x] [games/skyrim-se/src/render_inputs.cpp](games/skyrim-se/src/render_inputs.cpp)
- [x] [games/skyrim-se/src/render_inputs.h](games/skyrim-se/src/render_inputs.h)
- [x] [games/skyrim-se/src/render_observer_platform.h](games/skyrim-se/src/render_observer_platform.h)
- [x] [games/skyrim-se/src/resolution.cpp](games/skyrim-se/src/resolution.cpp)
- [x] [games/skyrim-se/src/resolution.h](games/skyrim-se/src/resolution.h)
- [x] [games/skyrim-se/src/sampling.cpp](games/skyrim-se/src/sampling.cpp)
- [x] [games/skyrim-se/src/sampling.h](games/skyrim-se/src/sampling.h)
- [x] [games/skyrim-se/src/screen_classification.h](games/skyrim-se/src/screen_classification.h)

**Fixtures (1)**

- [x] [tests/skyrim_camera.cpp](tests/skyrim_camera.cpp)

**Rust review findings (12)**

- [x] **Duplication | Medium | Share the D3D11 auxiliary surface, pass and restore-scope bindings.** Skyrim declares Surface, Pass and Scope at games/skyrim-se/src/auxiliary.rs:17, :30 and :40, sends the pass at :279 and reuses the scope at :294. Runtime already generates the same surface with every offset checked at runtime/orchestrator/src/auxiliary_sr_abi.rs:27 and manually declares the other two at runtime/orchestrator/src/d3d11_host_abi.rs:156 and :165 for the corresponding host calls at runtime/orchestrator/src/d3d11_host.rs:2612 and :2657. Reordering session/frame/view identifiers would preserve the size checked in Skyrim and break restore identity. Generate all three public records once and import them in both crates, keeping Skyrim texture selection and runtime lease management local. [games/skyrim-se/src/auxiliary.rs:17](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/auxiliary.rs#L17)

- [x] **Altitude | Medium | Make auxiliary reconstruction depend on entering the output-resolution scope.** apply accepts the Runtime's auxiliary upscale and stores the result of enter_output_resolution only as a cleanup flag. The caller then reports reconstruction success even when resolution_enter declined its mutation lock or could not read producer ratios, so post-processing can consume reconstructed surfaces with the reduced-size viewport. On enter refusal, restore the Runtime scope, retain any outstanding cleanup receipt, and return failure to the reduced-resolution producer. [games/skyrim-se/src/auxiliary.rs:282](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/auxiliary.rs#L282)

- [x] **Quality | Medium | Invalidate Skyrim SR history when a sampled frame never reaches evaluation.** read_frame replaces HISTORY before render_inputs.rs reads or validates the textures. If frame N has the right camera but a missing motion target, produce returns before calling the runtime. Frame N+1 sees a consecutive camera receipt, can omit RESET, and publishes the previous jitter for N although SR last consumed N-1. Track the last accepted SR frame separately from camera sampling and force a reset after a missed publication. Cover a good frame, one refused input lease, then a good frame; preserve separate FG timing. [games/skyrim-se/src/camera/render.rs:178](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/camera/render.rs#L178)

- [x] **Altitude | Medium | Retain the Skyrim TAA restore receipt when the native write fails.** The post-process hook clears the engine's TAA-enabled byte for one native call. Its destructor invokes this void restore once, ignores the guarded write's boolean and drops the stack receipt. If the owner still matches and that write fails, TAA can remain disabled on later native fallback frames with no retry or diagnostic. Keep an outstanding owner-checked receipt on failure, report it, and drain it before admitting another bypass or ending the session. [games/skyrim-se/src/postprocess.rs:133](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/postprocess.rs#L133)

- [x] **Altitude | Medium | Record the outcome of the Skyrim window-begin callback in its lease.** The native render leaf returns false if the callback throws. use_buffer retains the context and chain and publishes an Open Window before checking that result, so retirement sends a normal end event even when begin never completed. Represent successful and indeterminate begin separately, clear a never-opened reservation or run explicit recovery for partial host work, and keep final-scene and window accounting consistent. [games/skyrim-se/src/presentation.rs:375](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/presentation.rs#L375)

- [x] **Duplication | Medium | Share Skyrim's native TAA owner and history-clear operation.** games/skyrim-se/src/resolution.rs:108 follows manager+0x1f0 to TAA, reads its mask at +0x30 and sets bit zero when resolution ownership changes. games/skyrim-se/src/postprocess.rs:49 and :58 independently locate the same TAA object and set that bit after reconstruction, then temporarily clear the enable byte. A layout or history-bit change can otherwise reset a different history in the two paths. Factor the owner lookup and checked mask read/OR into a single Skyrim TAA helper, keeping each caller's native write result and postprocess-only enable/restore receipt. [games/skyrim-se/src/resolution.rs:108](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/resolution.rs#L108)

- [x] **Duplication | Medium | Share the validation of render dimensions returned by the host.** games/skyrim-se/src/resolution.rs:169 and games/fallout4/src/resolution.rs:401 each construct a sized, otherwise empty rsf_game_render_config, ask their host for a plan, then accept only enabled plans with the requested output and nonzero render dimensions bounded by it. The Skyrim callback writes into an out-struct while Fallout also returns a success flag, and each game applies its ratios differently. Share the zeroed sized request and the pure checked-render-extent method on the SDK type, retaining the respective callback failures and native configuration in each adapter. Both will then apply future validation changes consistently. [games/skyrim-se/src/resolution.rs:179](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/resolution.rs#L179)

- [x] **Altitude | Medium | Bind the cached renderer-upload flags to their observed frame.** record_per_frame_flags stores only a byte in thread-local storage and never clears it. If a subsequent frame or session has no successful dirty upload on that thread, output_ready still accepts the old observation and enter/leave pass the previous flags to the engine's upload. Store the frame and owner/session with the observation, check them when opening the output scope, and retain the admitted flags in the receipt for its matching restore. [games/skyrim-se/src/resolution.rs:330](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/resolution.rs#L330)

- [x] **Duplication | Medium | Share Skyrim's single-hook binding publication and enable rollback.** games/skyrim-se/src/sampling.rs:223 and games/skyrim-se/src/presentation.rs:440 separately create and publish a permanent trampoline, install a copied binding before enabling its hook, restore the previous binding after enable failure, and set LIVE only on success. Both entry points are started by the Skyrim lifecycle, but sampling requires the bias service and may owe a sampler restoration, while presentation has optional capture and can owe a window end. Extract the create/publish/enable/rollback transaction for these two hooks. Keep their busy checks, owner-specific cleanup and export requirements separate and exercise the shared failure path with a fake enable callback. [games/skyrim-se/src/sampling.rs:258](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/sampling.rs#L258)

- [x] **Duplication | Low | Share the finite matrix comparison used to admit camera history.** games/skyrim-se/src/camera/render.rs:129 takes two 16-float matrices by value and checks every component for finiteness and absolute difference at most 0.001, before accepting the previous jitter at :188. games/fallout4/src/render_inputs.rs:589 compares borrowed matrices by the same rule during history reset at :1257 and producer validation at :1140. Only Fallout tests the tolerance and NaN behavior at :1911. Put the finite component comparison next to shared camera math, take borrowed matrices and an explicit absolute tolerance, and route both call sites through the same edge-case tests. [games/skyrim-se/src/camera/render.rs:129](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/camera/render.rs#L129)

- [x] **Duplication | Low | Generate the texture-capture request used by Skyrim and the D3D11 host.** games/skyrim-se/src/presentation.rs:25 manually defines the same 64-byte request as runtime/orchestrator/src/d3d11_host_abi.rs:142. The before/after interface callback constructs it with two string pointers and a texture, then calls the resolved capture export at presentation.rs:409. runtime/orchestrator/src/d3d11_host.rs:2734 accepts the other Rust type. Both check the total size but different offsets, so a swap of name and description could go undetected. Use the same public-header binding for the request at both ends while leaving the diagnostic names and frame identity in the Skyrim producer. [games/skyrim-se/src/presentation.rs:25](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/presentation.rs#L25)

- [x] **Duplication | Low | Put Skyrim's guarded 32-bit memory reader on Platform.** games/skyrim-se/src/render_inputs.rs:123, games/skyrim-se/src/cpu_policy.rs:188 and games/skyrim-se/src/camera/render.rs:51 each define a four-byte guarded read and little-endian conversion from the same Platform. The frame identity recheck at render_inputs.rs:306, CPU frame facts at cpu_policy.rs:364 and camera cache capture at camera/render.rs:99 all require the same None-on-failure contract. Expose read_u32 alongside Platform::read_usize and have the callers retain their own fallback or refusal behavior. [games/skyrim-se/src/render_inputs.rs:123](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/skyrim-se/src/render_inputs.rs#L123)

<a id="w01"></a>

### W01 · Project Wingman integration

Port the game entry, observation, camera and execution paths. Tie compute packets to their render-graph lifetime.

**Coordinate shared edits**

- Carrier entry, game recognition and existing forwarding exports ([L01](#l01), [A01](#a01)).

**Port files (9)**

- [x] [games/project-wingman/src/native_camera.cpp](games/project-wingman/src/native_camera.cpp)
- [x] [games/project-wingman/src/native_camera.h](games/project-wingman/src/native_camera.h)
- [x] [games/project-wingman/src/native_execution.cpp](games/project-wingman/src/native_execution.cpp)
- [x] [games/project-wingman/src/native_execution.h](games/project-wingman/src/native_execution.h)
- [x] [games/project-wingman/src/native_observer.cpp](games/project-wingman/src/native_observer.cpp)
- [x] [games/project-wingman/src/native_observer.h](games/project-wingman/src/native_observer.h)
- [x] [games/project-wingman/src/native_read.h](games/project-wingman/src/native_read.h)
- [x] [games/project-wingman/src/plugin.cpp](games/project-wingman/src/plugin.cpp)
- [x] [loader/proxy/src/wingman_loader.cpp](loader/proxy/src/wingman_loader.cpp)

**Fixtures (1)**

- [x] [tests/wingman_camera.cpp](tests/wingman_camera.cpp)

**Rust review findings (10)**

- [x] **Quality | High | Expire Wingman compute packets when their render graph ends.** record_compute inserts unowned RDG identities under a parameter-block address before original_compute runs. When construction fails or RDG culls the pass, take never removes that entry and only plugin stop clears it. A later unrelated pass reusing the address can consume the stale packet, and native_execution.cpp:148 then tries to acquire COM textures through old graph objects. Even without address reuse, 256 abandoned entries prevent recording new packets. Bind records to graph and pass lifetime, retire on graph discard, and test address reuse after a skipped execution. [games/project-wingman/src/execution.rs:269](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/project-wingman/src/execution.rs#L269)

- [x] **Quality | Medium | Reject nonfinite jitter before marking a Wingman camera valid.** decode reads jitter_pixels from engine memory and never checks either component. With otherwise admissible matrices and timing, NaN jitter passes decode, camera_valid becomes 1, and the RHI observer sends those pixels through host.render_pass. Validate both components before admission and add a guarded-copy fixture with one nonfinite jitter component. [games/project-wingman/src/camera.rs:81](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/project-wingman/src/camera.rs#L81)

- [x] **Duplication | Medium | Build carrier plugin-session requests with one SDK helper.** games/project-wingman/src/carrier.rs:127 and games/rdr2/src/runtime_worker.rs:193 both build the same sized AMD64 probe, zeroed GameSDK services, nonzero tick-based session ID, log callback and versioned plugin-session options, then prepare and start in that order. Wingman starts an observation-only session, while RDR2 must call render_host_create to populate its services before prepare at runtime_worker.rs:242. Provide shared sized constructors for the probe/services and a session request borrowing the paths, digest and prepared services. Keep executable identity, host creation, activation and result reporting local so adding an SDK header field does not require updating parallel initializers. [games/project-wingman/src/carrier.rs:127](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/project-wingman/src/carrier.rs#L127)

- [x] **Altitude | Medium | Retain the cleanup handle from a refused Wingman session prepare.** The carrier resolves only prepare and start, then discards the local session on any nonzero prepare result. plugin_session.h explicitly permits BUSY with a non-null inactive session that the caller must retain and quiesce/stop. The adapter can therefore leave cleanup ownership in the runtime with no handle available to retry it. Store the pending session and resolve its cleanup operations, as the Anomaly carrier already does for a retained refusal. [games/project-wingman/src/carrier.rs:151](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/project-wingman/src/carrier.rs#L151)

- [x] **Quality | Medium | Keep Wingman start retryable after an execution hook refuses to enable.** start rolls back through stop, which clears the execution host once callbacks drain. observer::start then returns NATIVE_REFUSED but leaves its own host installed, so status still says prepared and every subsequent start fails at execution::start because its host is missing. The same split occurs when the observer hook enable fails. Retain the prepared host during start rollback or transition both owners to unprepared and allow prepare to run again; test a one-time enable failure followed by a retry. [games/project-wingman/src/execution.rs:135](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/project-wingman/src/execution.rs#L135)

- [x] **Duplication | Medium | Decode the size-only GameSDK probe once for exact-build detectors.** Project Wingman registers detect at games/project-wingman/src/lib.rs:99 and Fallout 4 at games/fallout4/src/lib.rs:345. Their bodies at :24 and :282 respectively repeat the short-probe check, null pointer checks, CStr borrows and ASCII comparisons. RDR2 repeats the same decoding at games/rdr2/src/lib.rs:130, while Anomaly at games/anomaly/src/lib.rs:113 searches several local builds. Put the size-only probe-to-borrowed-fields conversion and case-insensitive comparison in the Rust GameSDK with one short/null-input contract. Keep PE machine requirements, accepted executable names, hashes, and multi-build selection inside each adapter. [games/project-wingman/src/lib.rs:24](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/project-wingman/src/lib.rs#L24)

- [x] **Duplication | Low | Share row-major camera multiplication while preserving the scalar precision.** games/project-wingman/src/camera.rs:41 multiplies flat row-major 4x4 f32 matrices in increasing inner-index order for the view identity check at :102 and reprojection at :118. games/rdr2/src/camera.rs:57 implements the same indexing and summation with f64 accumulators for current and previous clip transforms at :170 and :174. Both already use sdk/game/src/camera_math.rs:5 for inversion. Put the row-major product alongside that helper with a scalar parameter retaining each caller's precision and operand order, and verify it with non-commuting reference matrices. [games/project-wingman/src/camera.rs:41](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/project-wingman/src/camera.rs#L41)

- [x] **Boundaries | Low | Give the Wingman ObserveNative gate a saved overlay control.** rsf_wingman_carrier_start reads [ProjectWingman] ObserveNative from ReScaleFrame.ini with GetPrivateProfileIntW and returns before loading the Runtime when it is 0. It is the only switch for the experimental native observer, and no overlay row, settings_store key or restart-required control exists for it. Rule 2 covers experimental options: a restart-only option still needs a control that says so and saves the value. Move the key into the shared settings store and give it a restart-required overlay row (or a Runtime-owned diagnostic enable), so the carrier reads the stored value rather than a private INI key. [games/project-wingman/src/carrier.rs:79](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/project-wingman/src/carrier.rs#L79)

- [x] **Simplicity | Low | Project Wingman keeps two copies of one lifecycle: two ACCEPTING flags and two stored hosts.** observer.rs and execution.rs each own a static ACCEPTING flag and a Mutex State holding Option<rsf_game_host_services> (observer.rs:50/88, execution.rs:67/88). observer::prepare/start/quiesce/stop always drive execution::prepare/start/quiesce/stop in the same call, so both hosts are written from the same *host and cleared together, and both flags flip in the same quiesce (observer.rs:191-192) and stop. Readers then ask different copies of the same fact: rsf_wingman_exec_take reads execution host plus execution ACCEPTING (execution.rs:272), rsf_wingman_observe reads observer ACCEPTING plus observer host. Simpler: one lifecycle state (host, accepting) owned by observer.rs and read by execution.rs, leaving execution.rs only its sites/created/installed/pending. Tradeoff: execution.rs gains a dependency on the observer state, and the start order (execution hooks enabled before the observer hook) must set the single flag after both succeed. [games/project-wingman/src/execution.rs:67](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/project-wingman/src/execution.rs#L67)

- [x] **Duplication | Low | Read the Wingman view jitter once per captured temporal packet.** The native TAA hook at games/project-wingman/src/native_execution.cpp:128 enters capture, which copies JITTER into pass.camera.jitter_pixels at games/project-wingman/src/execution.rs:182. Six lines later read_camera enters games/project-wingman/src/camera.rs:81 and copies the same bytes into the same field again. Successful captures perform two guarded native reads, and an early camera refusal can carry jitter from the first one. Capture jitter once and pass its Option into the decoder, preserving the fallback value for refused packets and the independent public read_camera entry. [games/project-wingman/src/execution.rs:182](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/project-wingman/src/execution.rs#L182)

<a id="m01"></a>

### M01 · Minecraft native integration

Port the JNI bridge and review the frame and session boundaries against the Java callers.

**Coordinate shared edits**

- OpenGL shared surfaces, device ownership and linked Minecraft callers ([D08](#d08), [D02](#d02)).

**Port files (2)**

- [x] [games/minecraft/native/jni.cpp](games/minecraft/native/jni.cpp)
- [x] [games/minecraft/native/minecraft_platform.h](games/minecraft/native/minecraft_platform.h)

**Rust review findings (6)**

- [x] **Boundaries | Medium | Move Minecraft runtime options into the shared settings store and overlay.** frame::options builds rsf_gl_pipeline_options from rsf_minecraft_configuration: SR backend, quality, FG backend, FG mode (FIXED/OFF chosen here from fg != 0), generated frames, Reflex mode, frame limit (converted from fps to us here), debug, DLSS preset and the twelve sharpening fields. The Java mod stores and edits these in TaaSettings, SodiumConfig and TaaScreen. The runtime GL pipeline (runtime/orchestrator/src/gl_pipeline.rs) has no settings_store or overlay service use, so these options bypass the shared settings storage and the shared overlay. That breaks Rule 1 (settings storage stays generic) and Rule 2 (each option needs a live overlay control). The adapter also turns the FG backend into an FG mode and the limit into an interval, which is Rule 3 pacing and FG policy. Let the Runtime own these keys and overlay rows as on the d3d11 host, and have Java report only game facts. [games/minecraft/native/frame.rs:128](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/minecraft/native/frame.rs#L128)

- [x] **Duplication | Medium | Give Minecraft native status fields a shared index definition.** games/minecraft/native/frame.rs:189 serializes 25 status values in an implicit order. games/minecraft/src/main/java/org/kpc/rescaleframecraft/render/NativeRenderer.java:72 uses indices 2, 3, 14 and 15 to set generation limits and :239 decodes the same array again for display. games/minecraft/native/frame/tests.rs:222 checks the Rust-side order against another tuple, but a Java index change can pass that test and alter live limits. Define the append-only field names and indices once and generate Rust writers and Java accessors, retaining prefix-length handling for older callers. [games/minecraft/native/frame.rs:181](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/minecraft/native/frame.rs#L181)

- [x] **Altitude | Medium | Preserve query failure when publishing Minecraft status.** The runtime platform declares return values for status_get and latency_status_get, but Ops wraps both as unit. rsf_minecraft_status then packs zero-initialized fields and returns a nonzero count even if the pipeline or latency query failed, and jni.cpp copies those fields to Java as a successful snapshot. Propagate pipeline status failure to the existing zero-count path, and encode latency availability separately so a failed latency query does not erase valid graphics status. [games/minecraft/native/lib.rs:175](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/minecraft/native/lib.rs#L175)

- [x] **Quality | Medium | Convert the Java runtime directory to standard Unicode before opening Minecraft resources.** jni.cpp:26 and :50 pass GetStringUTFChars directly to rsf_minecraft_open. JNI modified UTF-8 encodes supplementary characters as surrogate pairs, so open_log replaces those bytes with replacement characters and setup forwards the same nonstandard encoding as runtime_directory_utf8. A valid installation path containing an emoji or supplementary ideograph can lose its log and vendor runtime lookup. Take GetStringChars and length, preserve UTF-16 for the file path and explicitly encode standard UTF-8 for the runtime call; cover a path with a supplementary character. [games/minecraft/native/lib.rs:303](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/minecraft/native/lib.rs#L303)

- [x] **Altitude | Medium | Propagate a failed surface query before constructing the camera pass.** camera_pass ignores the Runtime's get_surfaces result. On a query that leaves the initialized record unchanged, frame::pass marks the camera valid with zero extents and computes reciprocal motion scales from zero, then both evaluate and hudless submit the packet. Return a Result from camera_pass and pass the surface-query error back through JNI before dispatch, so callers see the original failure. [games/minecraft/native/lib.rs:421](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/minecraft/native/lib.rs#L421)

- [x] **Simplicity | Low | Minecraft threads the platform table through every export but reads it once, then re-checks each validated leaf.** All twelve rsf_minecraft_* exports take a platform pointer and call ops(platform), which returns the cached OnceLock Ops after the first call and ignores the argument from then on (lib.rs:51-58). Ops is documented as "every entry is present" after Ops::complete (61-79), yet all 17 wrappers re-handle None with map_or(RSF_BACKEND_ERROR_INVALID_ARGUMENT), if let Some or is_some_and (85-211); line 32 even notes that error is "returned only if jni.cpp ever lent an incomplete table", which complete() already refuses. Simpler: one rsf_minecraft_publish(table) that converts the table once into plain extern fn pointers, and exports that take only their real arguments and call through the published Ops. Tradeoff: jni.cpp must publish before the first call instead of passing its static table each time. [games/minecraft/native/lib.rs:51](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/minecraft/native/lib.rs#L51)

## Engine and platform integrations

<a id="i01"></a>

### I01 · Steam and Windows integration

Port the Steam chain and Windows input facts, coordinating shared Steam interfaces with Fallout 4 and Skyrim.

**Coordinate shared edits**

- Steam chain installation and cold device creation ([F01](#f01), [S01](#s01)).

**Port files (4)**

- [x] [integrations/steam/include/rescaleframe/steam_d3d11_chain.h](integrations/steam/include/rescaleframe/steam_d3d11_chain.h)
- [x] [integrations/steam/src/d3d11_chain.cpp](integrations/steam/src/d3d11_chain.cpp)
- [x] [integrations/steam/src/d3d11_chain_platform.h](integrations/steam/src/d3d11_chain_platform.h)
- [x] [integrations/windows/xinput_facts.h](integrations/windows/xinput_facts.h)

**Rust review findings (2)**

- [x] **Simplicity | Low | Steam relay state is four loose atomics re-validated on every forwarded call** prepare publishes SERVICE_USER, STEAM_BASE, PLATFORM and SERVICE (lib.rs:118-121, stored at :603-606) once, after Ops::resolve already proved platform.dispatch is Some (:461) and service non-null (:542), and before any hook is created or enabled (:608-646). Yet every forwarded Present/Release/Resize call goes through handled() (:178-199), which transmutes the SERVICE pointer back into an Option<fn>, re-dereferences PLATFORM and re-checks its dispatch Option, with a NOT_READY fallback that cannot occur once hooks exist; continuation() separately reloads STEAM_BASE. Simpler: publish one `OnceLock<Relay { dispatch: plain fn, service: plain fn, user, base }>` (a Send wrapper for the user pointer) before the hooks are created, and have handled()/continuation() read that record directly. Tradeoff: one small wrapper type; removes the pointer/fn transmute, four Acquire loads per call and the dead not-ready branch. [integrations/steam/src/lib.rs:118](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/steam/src/lib.rs#L118)

- [x] **Duplication | Low | Share Present request assembly between Steam's two forwarding paths.** integrations/steam/src/lib.rs:215 and :263 independently fill the same PresentArgs and dispatch it before native continuation. The Present1 pair at :230 and :324 repeats the same operation with the parameters pointer from integrations/steam/src/chain_abi.rs:62. The early entry continues through the original Steam handler while the inner route uses the saved method, so the continuation must remain distinct. Share just the typed dispatch request and handled-result extraction per Present method; a future flag or field change will then reach early and inner calls together. [integrations/steam/src/lib.rs:263](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/steam/src/lib.rs#L263)

<a id="u01"></a>

### U01 · Unity Mono startup and configuration

Port the plugin, Runtime setup and configuration from managed entry through session startup.

**Coordinate shared edits**

- Unity package callback tables, device events and checked retirement ([U02](#u02), [U03](#u03), [U04](#u04), [U05](#u05)).

**Port files (7)**

- [x] [games/unity-mono/include/rescaleframe/unity_runtime_config.h](games/unity-mono/include/rescaleframe/unity_runtime_config.h)
- [x] [games/unity-mono/include/rescaleframe/unity_runtime_host.h](games/unity-mono/include/rescaleframe/unity_runtime_host.h)
- [x] [games/unity-mono/src/plugin.cpp](games/unity-mono/src/plugin.cpp)
- [x] [games/unity-mono/src/plugin_platform.h](games/unity-mono/src/plugin_platform.h)
- [x] [games/unity-mono/src/runtime_host.cpp](games/unity-mono/src/runtime_host.cpp)
- [x] [games/unity-mono/src/runtime_graphics.cpp](games/unity-mono/src/runtime_graphics.cpp)
- [x] [games/unity-mono/src/runtime_platform.h](games/unity-mono/src/runtime_platform.h)

**Fixtures (2)**

- [x] [tests/unity_mono_runtime.cpp](tests/unity_mono_runtime.cpp)
- [x] [tests/unity_sr_d3d12.cpp](tests/unity_sr_d3d12.cpp) (deleted: the DirectXMath leaves are Rust, the preset validator is the C11 leaf `tests/unity_sr_d3d12_preset.c`, and the Streamline call is Rust in `tests/game-sdk/src/unity_sr_d3d12/streamline.rs`)

**Rust review findings (7)**

- [x] **Altitude | Medium | Close Mono bridge callback admission when proving its final drain.** stop checks bridge.drained, runs managed Stop on another thread, then releases native objects. The Mono implementation at bridge.cpp:323 only samples callback/packet/fence counts. A message or managed logging callback can still enter after that sample while the bridge clears services and unhooks. Use a checked close that prevents new callback leases and verifies existing work has drained before returning the release right, as the package D3D12 route already does. [games/unity-mono/src/lifecycle.rs:229](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/lifecycle.rs#L229)

- [x] **Boundaries | Medium | Remove the Unity Mono Auto FG dependency on the FSR3 directory.** fg_start chooses the FSR3 directory for both Auto and Off, then returns NO_PATHS before passing the per-provider locations to Runtime if that directory is unset. A deployment with Streamline available and FSR3 omitted therefore cannot install the switchable host for Auto even if Runtime would choose DLSS. Pass the available directories and a provider-independent runtime root to the host, and let Runtime validate the directory for its selected backend. Cover Auto with only one non-FSR provider configured. [games/unity-mono/src/runtime_setup.rs:208](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/runtime_setup.rs#L208)

- [x] **Boundaries | Medium | Expose the Unity Mono graphics diagnostics in the shared overlay.** fg_start reads GraphicsDebug and FGCaptureDirectory exclusively from UnitySR via Win32 profile calls. It enables the debug layer before graphics creation and sends the capture destination to the Runtime once after installation. The Mono adapter provides no overlay descriptors or saved preferences for either key, so a user must edit the INI and restart to try capture. Add a saved, restart-labelled debug-layer control and a capture-directory control that calls the Runtime capture configuration while running. [games/unity-mono/src/runtime_setup.rs:218](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/runtime_setup.rs#L218)

- [x] **Quality | Medium | Attempt Unity SR startup once when Mono arrives near the polling deadline.** If mono_loaded first succeeds in the final two seconds of the 90-second wait, start sleeps MONO_SETTLE and then checks the original deadline before its first sr_start call. It returns SR_NOT_READY even though the runtime is available and the callback host was never asked to start. Attempt once after settling, applying the remaining deadline only to retries, and cover late Mono availability with a controllable clock. [games/unity-mono/src/runtime_setup.rs:366](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/runtime_setup.rs#L366)

- [x] **Duplication | Medium | Reuse the executable fingerprint across Unity SR startup retries.** games/unity-mono/src/runtime_setup.rs:296 hashes the whole executable on each sr_start call. The combined starter at :366 invokes that entire route again every 500 ms when runtime/orchestrator/src/callback_host.rs:1068 reports that plugin preparation or activation failed and cleanup completed. A delayed Unity startup can therefore reread an unchanged executable many times before its 90-second deadline. Build an owned executable name and digest once per combined start and lend them to each retry. Keep the standalone SR start doing its own verification, and keep rechecking the actual host readiness on each attempt. [games/unity-mono/src/runtime_setup.rs:367](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/runtime_setup.rs#L367)

- [x] **Quality | Low | Resolve rooted Unity configuration paths consistently on Windows.** resolve recognizes a leading backslash only for strings of length two or more and never recognizes a leading forward slash. Thus a valid runtime directory value of a single backslash resolves to the INI directory, and /sdk resolves beneath the INI directory rather than at the drive root. Both plugin and package path_setting use this branch. Classify root components before the length-two drive check and test both separator forms, including the existing single-backslash fixture. [games/unity-mono/src/runtime_config.rs:24](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/runtime_config.rs#L24)

- [x] **Simplicity | Low | Two Unity config exports are kept with no caller** runtime_config.rs:5 states the exports behind unity_runtime_config.h no longer have a caller in the tree. A repository search confirms it: rsf_unity_config_path_setting (runtime_config.rs:167-189) and rsf_unity_config_utf8 (:193-209) are referenced only by the inline helpers in games/unity-mono/include/rescaleframe/unity_runtime_config.h, and no .cpp/.h/.cs/build file includes that header. The Rust callers use path_setting and utf8 directly. Simpler: delete both exports and the header (it is already listed for retirement in PORT.md:265), leaving runtime_config.rs as plain crate-internal helpers. Tradeoff: an out-of-tree consumer of the header would break; none is demonstrated. [games/unity-mono/src/runtime_config.rs:167](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/runtime_config.rs#L167)

<a id="u02"></a>

### U02 · Unity package session and routing

Port settings, host services, routing and window admission, including safe subclass retirement.

**Coordinate shared edits**

- Unity package callback tables, device events and checked retirement ([U01](#u01), [U03](#u03), [U04](#u04), [U05](#u05)).

**Port files (3)**

- [ ] [integrations/unity/native/package.cpp](integrations/unity/native/package.cpp)
- [x] [integrations/unity/native/package.h](integrations/unity/native/package.h)
- [x] [integrations/unity/native/package_state.h](integrations/unity/native/package_state.h)

**Rust review findings (9)**

- [x] **Quality | High | Keep the Unity window forwarding chain alive when a later subclass prevents detachment.** unload clears window and previous even though the restore leaf has no result. In package.cpp:88 another subclass above input_proc prevents the restore, leaving that subclass able to call input_proc. It now falls through to DefWindowProcW instead of the saved Unity procedure, or targets unloaded plugin code after DLL unload. Use an owner-thread subclass mechanism with independent removal, or retain the module and forwarding receipt until a verified detach. Cover shutdown with another subclass installed above the Unity hook. [games/unity-mono/src/package_state/services.rs:355](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/package_state/services.rs#L355)

- [x] **Altitude | Medium | Require a valid render plan before publishing a Unity SR view.** Leaves::sr_plan discards the Runtime result and returns zero-initialized dimensions on refusal. create_view then calls view_planned unconditionally, enabling the managed render config and setting temporal_ready for a nonzero backend even if it has no valid input extent. Return a fallible plan, validate the dimensions against the output and release the newly created context on refusal before publishing it. [games/unity-mono/src/package_state.rs:338](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/package_state.rs#L338)

- [x] **Altitude | Medium | Use the latency owner's snapshot for Unity latency availability and mode.** Package::status takes latency_available and latency_mode from fg.vendor, then independently fills latency_owner, markers and sleep counts from native_latency_status. That second record already has available and effective_mode at native_fg_cpu.rs:1534. A provider transition or independently active latency path can therefore expose a mode belonging to a different owner, while a failed read preserves old counters. Populate the latency fields together from one snapshot and explicitly mark an unavailable read. [games/unity-mono/src/package_state.rs:510](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/package_state.rs#L510)

- [x] **Boundaries | Medium | Have the Runtime own the Unity package SR fallback and quality admission.** Unity rejects Ultra Quality when the last active Auto provider was DLSS (line 456), then independently retries a failed vendor context as backend zero in create_view. package.cpp:139-163 queries the command list adapter and calls rsf_sr12_create, so a provider failure and the next chosen context are split across two owners. On a first Auto request there is no previous active DLSS value to enforce the same quality rule. Resolve capabilities and fallback against the current device inside the Runtime, return the effective backend, dimensions and original refusal, and let Unity apply that plan to its engine. [games/unity-mono/src/package_state.rs:729](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/package_state.rs#L729)

- [x] **Altitude | Medium | Tie Unity's cached Auto FG decision to the graphics device.** request_generation keeps the first nonzero adapter-derived backend for the entire process. A successful device shutdown and subsequent initialization in routing.rs:161-175 reloads the graphics bridge without invalidating that value, so Auto can request the previous adapter's provider. Let the Runtime resolve Auto for the live device or invalidate a device-keyed decision when the Unity device is retired. [games/unity-mono/src/package_state/presentation.rs:183](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/package_state/presentation.rs#L183)

- [x] **Duplication | Medium | Give Win32 input message classification a single implementation.** The Unity package at games/unity-mono/src/package_state/services.rs:389, Unity Mono at games/unity-mono/src/bridge.cpp:266, RDR2 at games/rdr2/src/native_platform.cpp:111 and AC7 at games/ac7/src/native_renderer.cpp:297 repeat the same keyboard/mouse ranges and raw-input device mapping. The package uses a raw-type leaf while the three native callbacks call GetRawInputData directly. They then attach different source-frame and lifetime facts to the same input kind. Put the Win32 message-to-GameSDK-kind operation in shared Windows support, let the package call it through its native leaf, and keep callback admission and frame attribution at each integration. [games/unity-mono/src/package_state/services.rs:389](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/package_state/services.rs#L389)

- [x] **Boundaries | Medium | Retire Unity SR contexts when their camera disappears.** ReScaleFrameUpscaler.cs:104 sends the camera entity ID as the view key. Views::entry appends each new ID, while take drains the entire table only on global reconfiguration or session retirement. A scene that repeatedly creates and destroys cameras retains every old SR context and its GPU resources until one of those global events. Have the integration report when a camera is removed, or age out inactive views, and have the Runtime release each context once its last submission retires. Cover camera churn with other cameras still rendering. [games/unity-mono/src/package_views.rs:84](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/package_views.rs#L84)

- [x] **Simplicity | Low | Unity package re-adopts the constant 55-leaf platform table on every export and callback** Leaves::adopt (package_state.rs:204-267) null-checks the table, copies it and unwraps 55 Option leaves. It runs at the top of 18 exports (package_state.rs:771,802,819,844,869,890; routing.rs:196,210,220,234; presentation.rs:218,236,245; services.rs:459,470,491,504,511) and inside host_cpu_event via the host services user pointer (services.rs:442), so every window message, CPU event, render route and log line repeats the validation of package.cpp's one constant table, and every export carries a platform parameter plus a None branch with its own fallback result (0, FAILED, FEATURE_FAILED, device==WAITING). Leaves also stores `table` only so services.rs:303 can lend it back as the user pointer for that re-adoption. Simpler: adopt once in rsf_unity_package_load (or the first export) into a static OnceLock<Leaves>, drop the platform argument from the exports and the user-pointer round trip, and keep `&Leaves` parameters internally so the fake-table tests still inject leaves. Tradeoff: a package.cpp/package_state.h signature change and one ordering rule (load before other exports), in exchange for removing 19 per-call validations and their divergent refusal results. [games/unity-mono/src/package_state.rs:204](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/package_state.rs#L204)

- [x] **Duplication | Low | Use the same preview resource-state bit for Unity pass creation and consumption.** games/unity-mono/src/d3d11_pass.rs:51 declares a local bit and sets it at :166. games/unity-mono/src/package_state/routing.rs:17 independently declares the same bit and tests it at :103 to tell the Editor presenter which D3D12 state the texture is in. The routing test at games/unity-mono/src/package_state.rs:2439 supplies the literal again, so its synthetic packet can continue passing if the actual writer changes. Expose RSF_UNITY_PREVIEW_SOURCE_COMMON from integrations/unity/native/package.h:16 through the common binding, refer to it in both Rust modules and the native D3D11 header, and test the real plan-to-route handoff. [games/unity-mono/src/package_state/routing.rs:17](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/package_state/routing.rs#L17)

<a id="u03"></a>

### U03 · Unity D3D11 and D3D12 bridges

Port shared frame state, submission resources and per-frame ownership from both native bridges.

**Coordinate shared edits**

- Unity package callback tables, device events and checked retirement ([U01](#u01), [U02](#u02), [U04](#u04), [U05](#u05)).

**Port files (8)**

- [x] [games/unity-mono/include/rescaleframe/unity_bridge.h](games/unity-mono/include/rescaleframe/unity_bridge.h)
- [ ] [games/unity-mono/src/bridge.cpp](games/unity-mono/src/bridge.cpp)
- [x] [games/unity-mono/src/bridge.h](games/unity-mono/src/bridge.h)
- [x] [games/unity-mono/src/bridge_interop.h](games/unity-mono/src/bridge_interop.h)
- [x] [games/unity-mono/src/bridge_state.h](games/unity-mono/src/bridge_state.h)
- [x] [games/unity-mono/src/d3d11_pass.h](games/unity-mono/src/d3d11_pass.h)
- [x] [games/unity-mono/src/d3d11_state.h](games/unity-mono/src/d3d11_state.h)
- [ ] [integrations/unity/native/bridge.cpp](integrations/unity/native/bridge.cpp)

**Rust review findings (7)**

- [x] **Boundaries | Medium | Move the Unity interop submission ring and retirement mechanism into the Runtime.** The package owns a process-wide ring that allocates D3D11/D3D12 fence values, tracks outstanding copies and decides when a slot or the entire bridge can retire. Both integrations/unity/native/bridge.cpp:330-378 and vulkan_bridge.cpp:384-435 drive the ring directly. d3d11_state.rs:413-414 and vulkan_state.rs:680-690 consult it before releasing graphics objects. These decisions need queue, fence and submission identity but no Unity camera or frame semantics. Put them behind a per-bridge Runtime handle with acquire, submit and drain operations. Keep Unity texture acquisition and Unity event timing in the package so other adapters can use the same resource lifetime guarantees. [games/unity-mono/src/bridge_interop.rs:36](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/bridge_interop.rs#L36)

- [x] **Altitude | Medium | Commit a Unity D3D12 reconfiguration only after the bridge delivers it.** admit_frame clears pending before bridge.cpp:209 creates or resets the command list. Those operations can fail and return before the host sees RSF_GAME_RENDER_RECONFIGURE, yet config now exposes the new generation as settled. Return the pending generation in the admission receipt and acknowledge it after the reconfiguration is successfully delivered; preserve newer generations and leave failed attempts retryable. [games/unity-mono/src/bridge_state.rs:350](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/bridge_state.rs#L350)

- [x] **Boundaries | Medium | Preserve each Unity view submission receipt until the queue flush.** record_source replaces the entry at source_frame_id % 8 without comparing session or view_key. Two accepted passes with the same frame and different view keys overwrite one another, as can frames eight apart before a flush. games/unity-mono/src/bridge.cpp:113 forwards only the surviving receipts to Runtime as RSF_GAME_RENDER_SUBMISSION. Keep pending receipts by full session, source and view identity, deduplicate exact repeats, and refuse new work or report exhaustion before overwriting an unconfirmed receipt. Cover two views in one frame and a delayed window callback. [games/unity-mono/src/bridge_state.rs:393](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/bridge_state.rs#L393)

- [x] **Duplication | Medium | Build the common Unity render-pass fields once across all three graphics routes.** games/unity-mono/src/d3d11_pass.rs:110, integrations/unity/native/vulkan_bridge.cpp:406 and games/unity-mono/src/bridge.cpp:78 unpack the same Unity packet into frame/session/view identities, reset flag, camera, rects, motion scaling and previous clip transform. D3D11 uses it from integrations/unity/native/bridge.cpp:301, Vulkan at vulkan_bridge.cpp:463 and the direct D3D12 path at games/unity-mono/src/bridge.cpp:222. Direct D3D12 also binds packet resources and treats an inputs-only probe as camera-valid FG input; the other routes supply their own surfaces. Expose a shared packet-to-scalar-pass builder in the Unity Rust bridge, then retain those explicit route-specific role and resource adjustments. A new camera or identity field will then propagate to all three paths. [games/unity-mono/src/d3d11_pass.rs:110](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/d3d11_pass.rs#L110)

- [x] **Quality | Medium | Validate source compatibility before copying Unity motion or depth into a shared texture.** shared_format unconditionally selects RG16_FLOAT for motion and R32_FLOAT for depth. The fixture at line 427 accepts RGBA16_FLOAT motion, but integrations/unity/native/bridge.cpp:349 uses CopyResource, which cannot convert that texture to RG16_FLOAT and returns no HRESULT. The bridge proceeds with missing or stale input. Accept only copy-compatible source formats and matching sample/mip/array layouts, or run an explicit conversion. Correct the RGBA16 motion fixture to verify refusal or conversion. [games/unity-mono/src/d3d11_pass.rs:189](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/d3d11_pass.rs#L189)

- [x] **Altitude | Medium | Require both graphics timelines to retire before replacing a Unity shared surface.** prepare_surface waits only for the D3D12 ring before destroying the previous shared surface. bridge.cpp:372 records the D3D11 output copy after hand-back and signals a separate tail; input-copy failures can also leave an unproved D3D11 copy. The checked close already requires both proofs. Apply equivalent retirement to the old surface on key change, keeping it owned and declining replacement while either side is unproved. [games/unity-mono/src/d3d11_state.rs:520](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/d3d11_state.rs#L520)

- [x] **Altitude | Low | Make the shared Unity completion check reject device removal.** Interop::drained treats u64::MAX as completed, and the unit test asserts that behavior. Every real fence consumer must repair the predicate: d3d11_state.rs:412, vulkan_state.rs:690, preview.rs:468 and vulkan_bridge.cpp:127 each filter the sentinel separately. Let the completion owner distinguish removed, pending and completed before callers decide whether to release resources, and update the test to exercise that contract. [games/unity-mono/src/bridge_interop.rs:94](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/bridge_interop.rs#L94)

<a id="u04"></a>

### U04 · Unity Vulkan bridge

Port resolver, shared surfaces, presentation and their retirement. Account for every GPU user before replacing an image on resize.

**Coordinate shared edits**

- Unity package callback tables, device events and checked retirement ([U01](#u01), [U02](#u02), [U03](#u03), [U05](#u05)).

**Port files (4)**

- [x] [games/unity-mono/src/vulkan_resolver.h](games/unity-mono/src/vulkan_resolver.h)
- [x] [games/unity-mono/src/vulkan_state.h](games/unity-mono/src/vulkan_state.h)
- [ ] [integrations/unity/native/vulkan_bridge.cpp](integrations/unity/native/vulkan_bridge.cpp)
- [x] [integrations/unity/native/vulkan_bridge.h](integrations/unity/native/vulkan_bridge.h)

**Rust review findings (7)**

- [x] **Quality | High | Retire every user of a Unity Vulkan shared image before replacing it on resize.** The six shared image entries belong to the device and are reused across three command slots. On a changed extent or format prepare_image immediately destroys the old Vulkan image, memory and D3D12 resource. execute_admitted waits only for its current slot unless the packet is a policy reconfiguration. A normal render-size change in slot 1 can free the image while slot 0 still uses it. Defer destruction until all prior Vulkan and D3D12 users complete, or prove a full drain before replacement. Cover resize with another slot outstanding. [games/unity-mono/src/vulkan_state.rs:759](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/vulkan_state.rs#L759)

- [x] **Altitude | Medium | Preserve the Vulkan dispatch owner when caching interceptor originals.** vulkan_bridge.cpp:734 passes a device-specific lookup result to Rust without the VkDevice; Resolver stores only the function slot. A later lookup for another device overwrites the original, and resolver_device_function returns that cached pointer without considering the device passed by the swapchain destroy caller at line 664. Key dispatch entries by the originating instance or device, or resolve the continuation from the supplied device at call time. [games/unity-mono/src/vulkan_resolver.rs:141](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/vulkan_resolver.rs#L141)

- [x] **Quality | Medium | Give the fake Vulkan resolver its exact callback context type.** interop_lookups_run_in_order_and_short_circuit erases the pointer to (Vec::new(), found) as c_void, so the callback cast at line 534 cannot constrain the vector element type or the integer width. The byte-string equality checks do not uniquely determine a vector element type, and an unconstrained found defaults to i32 while the callback reads usize. Declare the local as (Vec<Vec<u8>>, usize) or use one named fixture type on both sides of the cast. This is a source inspection of the test path. [games/unity-mono/src/vulkan_resolver.rs:543](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/vulkan_resolver.rs#L543)

- [x] **Boundaries | Medium | Bind the Unity Vulkan window packet to the Present that consumes it.** set_window stores a single copied frame, and window returns it for every present until a new packet, fault or release. vulkan_bridge.cpp:550-560 retrieves that packet before looking up the intercepted swapchain, and :593 reports its source identity to Runtime. If no new window event arrives, a later Present can report the previous source again; the registry also has no match from the packet to the presenting swapchain. Carry the intended presentation identity, consume the matching window receipt once, and forward a Present with no matching receipt along the native path. [games/unity-mono/src/vulkan_state.rs:352](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/vulkan_state.rs#L352)

- [x] **Duplication | Medium | Share the Unity graphics-device creation and publication transaction.** integrations/unity/native/bridge.cpp:254 and integrations/unity/native/vulkan_bridge.cpp:316 enter separate Rust initializers. games/unity-mono/src/d3d11_state.rs:418 and games/unity-mono/src/vulkan_state.rs:585 both claim a single creator, prepare device and slots outside the mutex, destroy partial transfers before relinquishing the claim, then swap the finished record into state and return a borrowed view. D3D11 latches failure until release, while Vulkan allows retry and additionally requires an active borrower and a Vulkan command pool. Extract the claim/publication transition parameterized by the two admission and failure policies, leaving device preparation, destruction and sync proofs on their existing backends. [games/unity-mono/src/vulkan_state.rs:585](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/vulkan_state.rs#L585)

- [x] **Altitude | Medium | Keep swapchain semaphore ownership until Vulkan retirement is proven.** swapchain_retire removes the registry entry before invoking a void Retire callback and returns success unconditionally. The production leaf at vulkan_bridge.cpp:720 ignores vkDeviceWaitIdle's result and destroys the semaphores. Make the callback report its wait outcome, retain the handles on unproved completion and distinguish explicit device-loss teardown from normal retirement. [games/unity-mono/src/vulkan_state.rs:1085](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/vulkan_state.rs#L1085)

- [x] **Simplicity | Low | Unity Vulkan admission is two booleans for a three-state lifecycle** State.retiring is read only in enter() (vulkan_state.rs:407), and always together with closed (`self.retiring || self.closed`); every other gate (open :345, claim_device :419, can_borrow_device :427, close :479, commands_retired :667) reads closed alone. release() (:495-499) sets both, reopen() (:486-489) clears both, stop_admission (:797) sets retiring and close() (:478-484) sets closed, so retiring && closed carries nothing beyond closed. Simpler: one `admission: Admission { Open, Retiring, Closed }` field, with enter() requiring Open, the closed checks reading `== Closed`, stop_admission moving Open to Retiring, close() moving to Closed and release/reopen assigning Closed/Open. Tradeoff: none beyond touching the tests that poke the two flags (:1248); behavior is identical. [games/unity-mono/src/vulkan_state.rs:299](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/vulkan_state.rs#L299)

<a id="u05"></a>

### U05 · Unity Editor preview

Port preview window and GPU lifetime and make the fixture exercise the production fence path.

**Coordinate shared edits**

- Unity package callback tables, device events and checked retirement ([U01](#u01), [U02](#u02), [U03](#u03), [U04](#u04)).

**Port files (3)**

- [x] [games/unity-mono/src/preview_platform.h](games/unity-mono/src/preview_platform.h)
- [x] [integrations/unity/native/preview.cpp](integrations/unity/native/preview.cpp)
- [x] [integrations/unity/native/preview.h](integrations/unity/native/preview.h)

**Rust review findings (3)**

- [x] **Altitude | Medium | Invalidate the Editor preview's cached window when its parent disappears.** The region owner enumerates a parent only while its stored HWND is zero. If the Editor destroys that parent and its child preview, later position attempts keep using the stale handles, and a failed position neither clears the record nor withdraws its visible flag. Track window destruction or validate the pair before reuse, withdraw the old preview chain, and recreate it under the newly observed parent. [games/unity-mono/src/preview.rs:203](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/preview.rs#L203)

- [x] **Boundaries | Medium | Separate the Unity Editor window policy from its DXGI presenter.** The package creates and resizes the preview swapchain, acquires its backbuffer, schedules a copy, signals its own fence, calls Present and counts base frames. integrations/unity/native/preview.cpp:97 and :198 implement CreateSwapChainForHwnd and Present for that Rust owner. The child window selection, visibility and source production require Unity knowledge, while the copy, swapchain, presentation and retirement need only a destination HWND, a queue and a leased source. Move the latter into a per-output Runtime presenter and return its presentation outcome to the Editor. [games/unity-mono/src/preview.rs:566](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/preview.rs#L566)

- [x] **Duplication | Medium | Drive the real preview signal-failure transition in the retirement tests.** integrations/unity/native/preview.cpp:224 calls the production Rust present entry, which records a copy, assigns its fence value, faults on signal failure and advances the ring only after signalling at games/unity-mono/src/preview.rs:562. The test helper at :784 reproduces recording/submission/advance while skipping the signal, and the fault-retention test at :977 sets faulted directly. Removing the production fault assignment could leave the test green. Extract the post-record submission transition on Graphics, exercise it with a refusing signal leaf, and assert the slot, retained source and release verdict through that path. [games/unity-mono/src/preview.rs:784](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/games/unity-mono/src/preview.rs#L784)

<a id="e01"></a>

### E01 · Godot engine hooks and render graph

Port the SCons engine module and engine patch hooks, including frame, viewport, graph and pass-record interfaces.

**Coordinate shared edits**

- Godot module-to-extension ABI, pass records and driver routing ([E02](#e02), [E03](#e03)).

**Port files (16)**

- [ ] [eng/patch-godot.py](eng/patch-godot.py) (inserts integration hooks into the Godot source checkout)
- [ ] [integrations/godot/modules/rescaleframe/bridge.cpp](integrations/godot/modules/rescaleframe/bridge.cpp)
- [x] [integrations/godot/modules/rescaleframe/cpu_facts.h](integrations/godot/modules/rescaleframe/cpu_facts.h)
- [ ] [integrations/godot/modules/rescaleframe/d3d12.cpp](integrations/godot/modules/rescaleframe/d3d12.cpp)
- [x] [integrations/godot/modules/rescaleframe/frame.h](integrations/godot/modules/rescaleframe/frame.h)
- [ ] [integrations/godot/modules/rescaleframe/graph.cpp](integrations/godot/modules/rescaleframe/graph.cpp)
- [ ] [integrations/godot/modules/rescaleframe/graph.h](integrations/godot/modules/rescaleframe/graph.h)
- [x] [integrations/godot/modules/rescaleframe/host.h](integrations/godot/modules/rescaleframe/host.h)
- [ ] [integrations/godot/modules/rescaleframe/motion.cpp](integrations/godot/modules/rescaleframe/motion.cpp)
- [x] [integrations/godot/modules/rescaleframe/pass.h](integrations/godot/modules/rescaleframe/pass.h)
- [x] [integrations/godot/modules/rescaleframe/primary.h](integrations/godot/modules/rescaleframe/primary.h)
- [ ] [integrations/godot/modules/rescaleframe/register_types.cpp](integrations/godot/modules/rescaleframe/register_types.cpp)
- [ ] [integrations/godot/modules/rescaleframe/register_types.h](integrations/godot/modules/rescaleframe/register_types.h)
- [ ] [integrations/godot/modules/rescaleframe/rsf_godot_engine.h](integrations/godot/modules/rescaleframe/rsf_godot_engine.h)
- [x] [integrations/godot/modules/rescaleframe/viewport.h](integrations/godot/modules/rescaleframe/viewport.h)
- [ ] [integrations/godot/modules/rescaleframe/vulkan.cpp](integrations/godot/modules/rescaleframe/vulkan.cpp)

**Fixtures (3)**

- [ ] [tests/godot_graph_handoff.cpp](tests/godot_graph_handoff.cpp)
- [ ] [tests/godot_graph_stubs/drivers/vulkan/rendering_device_driver_vulkan.h](tests/godot_graph_stubs/drivers/vulkan/rendering_device_driver_vulkan.h)
- [ ] [tests/godot_graph_stubs/servers/rendering/rendering_device_graph.h](tests/godot_graph_stubs/servers/rendering/rendering_device_graph.h)

**Rust review findings (5)**

- [x] **Boundaries | Medium | Report Godot input sampling from its input callback independently of frame begin.** rsf_godot_cpu_begin emits FRAME_BEGIN and INPUT_SAMPLE together whenever it opens a frame. eng/patch-godot.py:66 and :110-116 call that same function both on main-loop iteration entry and before Windows event processing, so iteration entry can report an input sample without observing the input path. The Runtime uses INPUT_SAMPLE for pacing. Split frame opening from input-sample reporting, emit the latter only from the verified engine sampling boundary, and leave an iteration with no sample unreported. [integrations/godot/modules/rescaleframe/frame.rs:121](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/modules/rescaleframe/frame.rs#L121)

- [x] **Altitude | Medium | Keep ownership of a started Godot native host until its boot transaction completes.** boot marks READY after native start, clears it on an attach refusal, and publishes even a zero session returned by the later status query. shutdown consults only READY, so an attach failure after successful start skips native shutdown, while a failed session query can admit configuration with no frame identity. Track started ownership separately, roll it back on attach or session failure, and publish ready only after a nonzero session is established. [integrations/godot/modules/rescaleframe/host.rs:189](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/modules/rescaleframe/host.rs#L189)

- [x] **Duplication | Medium | Generate the public camera and CPU event records once for Godot's standalone Rust object.** integrations/godot/modules/rescaleframe/pass.rs:28 and cpu_facts.rs:11 hand-declare the same wire records generated at sdk/game/src/lib.rs:91 and :483. The engine consumes the CPU record directly at integrations/godot/modules/rescaleframe/bridge.cpp:111 and copies the camera at :155. Its SCons rules invoke rustc directly, so importing the entire Cargo SDK is unnecessary. Generate a core-only source module for these two public records and their ABI constants from the SDK headers, include it in both build paths and list it as a SCsub dependency. Keep Godot's private Record and CPU sequencing local. This removes a second source update and offset checklist for every public layout change. [integrations/godot/modules/rescaleframe/pass.rs:28](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/modules/rescaleframe/pass.rs#L28)

- [x] **Altitude | Medium | Bind the Godot final-scene receipt to its primary render target.** primary_set changes the current target and resource generation but leaves recorded_source and pass.rs's LAST record intact. final_scene_admitted checks the current target and previous source frame independently. If the target changes within a frame, bridge.cpp:496 can submit the new target's color using the old target's camera and view key. Invalidate the stored receipt on retarget and require its target and generation to match the final-scene admission. [integrations/godot/modules/rescaleframe/primary.rs:145](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/modules/rescaleframe/primary.rs#L145)

- [x] **Boundaries | Medium | Keep Godot camera reset requests pending until their pass is recorded.** decide clears RESET_REQUESTED during rsf_godot_sr_plan. bridge.cpp:456-459 can then return for missing resolved motion or depth before rsf_godot_sr_record. A retry for the same source with unchanged color and extent can lose the camera change, because the previous evaluated frame is still its immediate predecessor. Snapshot a reset generation while planning and acknowledge that generation only when recording succeeds, preserving newer requests and covering a same-frame retry. [integrations/godot/modules/rescaleframe/primary.rs:156](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/modules/rescaleframe/primary.rs#L156)

<a id="e02"></a>

### E02 · Godot host services and D3D12

Port extension control, compositor diagnostics, window input and D3D12 rendering through the existing native API.

**Coordinate shared edits**

- Godot module-to-extension ABI, pass records and driver routing ([E01](#e01), [E03](#e03)).

**Port files (8)**

- [x] [integrations/godot/native/api.h](integrations/godot/native/api.h)
- [ ] [integrations/godot/native/effect.cpp](integrations/godot/native/effect.cpp)
- [ ] [integrations/godot/native/effect.h](integrations/godot/native/effect.h)
- [ ] [integrations/godot/native/extension.cpp](integrations/godot/native/extension.cpp)
- [x] [integrations/godot/native/godot_platform.h](integrations/godot/native/godot_platform.h)
- [ ] [integrations/godot/native/runtime.cpp](integrations/godot/native/runtime.cpp)
- [ ] [integrations/godot/native/runtime_control.cpp](integrations/godot/native/runtime_control.cpp)
- [ ] [integrations/godot/native/runtime_control.h](integrations/godot/native/runtime_control.h)

**Rust review findings (14)**

- [x] **Boundaries | Medium | Return a reusable Godot status header on D3D12.** lib.rs:155 accepts a status output only when its header carries the current Godot ABI (3 in api.h). The D3D12 session initializes its stored status with ABI 1 and status writes that whole record back to the caller. A second rsfg_status call with the same initialized output is therefore refused after the first one succeeds. Vulkan returns the current ABI. Preserve the negotiated output header or initialize the D3D12 snapshot with ABI_VERSION, and pass the same buffer twice through the exported entry point in the regression test. [integrations/godot/native/control.rs:40](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/native/control.rs#L40)

- [x] **Duplication | Medium | Derive the packaged D3D12 vendor paths once for Godot and Unity.** integrations/godot/native/control.rs:109 and games/unity-mono/src/package_state/presentation.rs:49 independently append the same four paths to the supplied SDK root, including the nested Streamline interposer. Godot lends the resulting pointers to its host at control.rs:119 and Unity lends them to SR setup at presentation.rs:68. A package directory rename currently needs two implementations and two sets of literal expectations. Share an owned UTF-8 vendor-path builder and keep boot-time pointer lifetimes, engine metadata and Vulkan-specific directory overrides in their current owners. [integrations/godot/native/control.rs:109](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/native/control.rs#L109)

- [x] **Duplication | Medium | Expose one interpretation of simultaneous FG session and provider-switch failures.** Godot's status builder at integrations/godot/native/control.rs:452 prefers a nonzero switch result to the session result. The Unity package at games/unity-mono/src/package_state.rs:496 prefers the nonzero session result and skips the switch query. Both read the same present-switch service as shown at integrations/godot/native/runtime.cpp:194 and integrations/unity/native/package.cpp:216. Consequently the two status APIs explain the same pair of failures differently. Return both facts from a common runtime status helper and define the legacy aggregate field precedence there, with a test where both results are nonzero. [integrations/godot/native/control.rs:452](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/native/control.rs#L452)

- [x] **Boundaries | Medium | Use Godot's D3D12 latency owner for availability and mode.** apply_fg copies low-latency availability and effective mode from FG, while apply_latency reads the latency owner and counters from a different snapshot. runtime.cpp:196 takes the former fields from fg.vendor.effective_reflex and low_latency_available, and leaf_latency_facts at :204 omits the authoritative mode and availability. With latency operating independently of FG, rsfg_status can report a pacing owner and paced frames alongside unavailable or incorrect mode. Carry available and effective_mode from rsf_native_latency_status_get into LatencyFacts, and populate all latency status fields from that result. [integrations/godot/native/control.rs:461](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/native/control.rs#L461)

- [x] **Altitude | Medium | Publish a Godot D3D12 binding only after its completion fence exists.** render stores the device/queue pair before leaf_d3d12_bind attempts CreateFence. After a refusal the next pass on that pair skips binding and can record work, but leaf_d3d12_signal requires a non-null fence, so pending slots can never retire. The existing failed-fence test expects that second pass to evaluate. Make device, queue and fence a single successful binding or keep an explicit retryable failed state that refuses recording. [integrations/godot/native/d3d12.rs:193](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/native/d3d12.rs#L193)

- [x] **Altitude | Medium | Publish the render receipt used by Godot Present after successful recording.** render stores Rendered::of(pass) before capture admission, slot wait, context creation or final-scene recording. If any of those steps refuses, present still sees the current frame and reuses its screen and camera instead of marking the SR miss. Keep attempted pass identity separate and commit the rendered receipt only when the relevant operation succeeds, with a regression case for same-frame refusal followed by Present. [integrations/godot/native/d3d12.rs:204](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/native/d3d12.rs#L204)

- [x] **Boundaries | Medium | Bind the Godot HUD-less capture slot to a successfully admitted source.** final_scene picks last_slot(next_slot) and excludes only pending work from a different frame. Submitted work leaves pending immediately, and the SR path increments next_slot before d3d12_wait returns. A refused slot wait can therefore be followed by final_scene using that very slot without a completion proof. runtime.cpp:305-318 flushes and records the capture in that slot. Record a slot receipt after successful acquisition, keyed by session and source, and require that receipt or independently acquire a retired slot for HUD-less capture. [integrations/godot/native/d3d12.rs:328](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/native/d3d12.rs#L328)

- [x] **Duplication | Medium | Pass Godot's composed settings to the engine without a second dictionary round trip.** integrations/godot/native/runtime_control.cpp:49 reads a Dictionary using the 21-field schema from integrations/godot/native/extension.rs:57 and compose at :117 writes typed Settings. The engine callback at integrations/godot/modules/rescaleframe/bridge.cpp:77 converts that record back to a Dictionary, only for configure_parse at :242 to map all the keys back into the same record. A new setting must survive three field lists to reach the native renderer. Add an engine-side typed configure path shared by the extension callback and the public Dictionary method after its own parse. Preserve the stricter extension argument checks, root viewport admission and scaling updates. [integrations/godot/native/extension.rs:117](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/native/extension.rs#L117)

- [x] **Boundaries | Medium | Read the Godot ABI size before borrowing the complete header.** checked borrows and copies both u32 words before testing struct_size. Public rsfg_attach_engine, rsfg_boot and rsfg_configure in runtime.cpp forward the caller pointer directly. A record containing only its four-byte size word therefore triggers an out-of-bounds header read instead of a clean refusal. Read just the size first, reject any value smaller than the required record, then read the version. For output records validate the header through raw reads without constructing a reference to the uninitialized output body. [integrations/godot/native/lib.rs:48](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/native/lib.rs#L48)

- [x] **Duplication | Medium | Use the public sharpening binding for Godot's settings and render requests.** integrations/godot/native/platform.rs:27 duplicates every field of the generated rsf_sharpening_options at runtime/orchestrator/src/render_abi.rs:801 and games/unity-mono/src/package_abi.rs:280. Godot's C++ validation leaf at integrations/godot/native/runtime.cpp:136 receives the public type, while the Rust caller supplies its handwritten Sharpening. Its size assertion cannot detect a swap of adjacent floats such as motion_threshold and motion_scale_limit. Publish a shared generated type with field offset checks, add required test derives there, and use it for the Godot Settings/SrEvaluate members. [integrations/godot/native/platform.rs:27](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/native/platform.rs#L27)

- [x] **Boundaries | Medium | Route Godot CPU events and PCL ping requests through the Runtime session.** Window::cpu and Window::latency_ping pass a Vulkan-booted boolean into the adapter table to select one of two latency paths. runtime.cpp:185-189 maps those choices to rsf_fg12_cpu versus rsf_latencyvk_cpu and rsf_native_latency_ping versus rsf_latencyvk_ping, plus a separate Vulkan Present marker call. Rule 3 assigns ping routing and marker mapping to the orchestrator. Bind one versioned CPU-event and ping service to the active Runtime session at setup, and report actual Present begin/end with the window and source identity. Keep Godot window admission in the adapter. [integrations/godot/native/platform.rs:1338](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/native/platform.rs#L1338)

- [x] **Duplication | Medium | Test Godot's Auto delegation without copying the runtime vendor policy.** The fake selectors at integrations/godot/native/platform/fake.rs:654 and :672 implement vendor preferences and fallback order again from runtime/orchestrator/src/lib.rs:385 and runtime/orchestrator/src/fg_choice.rs:47. integrations/godot/native/vulkan/tests.rs:598 expects the fake to return vendor-specific results even though production vulkan.rs:179 and :185 delegate those decisions. The adapter test can therefore pass after the real policy changes. Make the fake record its arguments and return test-selected sentinel backends, assert forwarding and capability masks in Godot, and exercise vendor choice tables against the actual runtime implementation. [integrations/godot/native/platform/fake.rs:654](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/native/platform/fake.rs#L654)

- [x] **Altitude | Medium | Retain the owning HWND with a Godot window procedure that cannot be detached.** release keeps previous when an intervening subclass or a failed SetWindowLongPtr prevents restoration, but clears input unconditionally. A later accept can publish a different HWND; attach skips installing its observer because previous still belongs to the old window, and a retry can no longer address the old HWND. Store forwarding ownership per window, preserve the original HWND for retirement and admit the replacement independently. [integrations/godot/native/window.rs:185](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/native/window.rs#L185)

- [x] **Duplication | Low | Share the Godot native boot and status fixtures across renderer tests.** integrations/godot/native/control/tests.rs:6 and integrations/godot/native/vulkan/tests.rs:7 build the same versioned Setup with the same engine version, project UUID and null log fields. integrations/godot/native/d3d12/tests.rs:13 and integrations/godot/native/vulkan/sr/tests.rs:33 repeat the literal records alongside the fake platform boot. A new required setup field currently needs four fixture edits and can make whole suites accidentally test boot refusal. Put a parameterized Setup constructor and successful backend-specific boot helpers in the existing platform::fake module, allowing negative cases to override fields and failure controls locally. [integrations/godot/native/d3d12/tests.rs:13](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/native/d3d12/tests.rs#L13)

<a id="e03"></a>

### E03 · Godot Vulkan

Port Vulkan creation, render, SR and presentation, with adapter facts feeding Runtime provider policy.

**Coordinate shared edits**

- Godot module-to-extension ABI, pass records and driver routing ([E01](#e01), [E02](#e02)).

**Port files (4)**

- [x] [integrations/godot/native/vulkan.cpp](integrations/godot/native/vulkan.cpp)
- [x] [integrations/godot/native/vulkan_control.h](integrations/godot/native/vulkan_control.h)
- [x] [integrations/godot/native/vulkan_creation.cpp](integrations/godot/native/vulkan_creation.cpp)
- [x] [integrations/godot/native/vulkan_creation.h](integrations/godot/native/vulkan_creation.h)

- [x] [runtime/orchestrator/src/native_sr_vulkan_session.cpp](runtime/orchestrator/src/native_sr_vulkan_session.cpp) (introduced Runtime Vulkan SR session adapter)

**Fixtures (1)**

- [x] [tests/godot_vulkan_creation.cpp](tests/godot_vulkan_creation.cpp)

**Rust review findings (10)**

- [x] **Boundaries | Medium | Put fixed-guide allocation and recreation policy on the Runtime Vulkan presenter.** Godot decides that FSR4 and XeSS have fixed guides, caches their first SR dimensions, and invalidates presenter_generation on certain changes. vulkan.cpp:532 turns this adapter-side decision into VK_ERROR_OUT_OF_DATE_KHR. The Runtime presenter owns the actual provider allocations; adding another provider or changing its allocation behavior requires synchronizing independent code in the Godot DLL. Pass the session, view, generation and extents to the Runtime presenter and have it return whether recreation is necessary. Godot should continue performing its engine-owned WSI recreation. [integrations/godot/native/vulkan.rs:70](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/native/vulkan.rs#L70)

- [x] **Duplication | Medium | Derive Godot Vulkan's Auto masks and explicit admission from the same capability facts.** integrations/godot/native/vulkan.rs:170 builds SR/FG choice masks from XeSS readiness, FSR4 build/graph/interop availability, and XeSS FG build/interop/window availability. The explicit checks at :295 and :326 repeat those combinations before accepting settings. vulkan/tests.rs:611 checks the masks returned to Runtime, while the configure path can independently accept or reject the same provider. Calculate the common per-provider prerequisites once from a guarded snapshot and reuse them in both paths. Keep the documented explicit-only DLSS-G/FSR4 FG rules, capacity probes and live presenter checks separate. [integrations/godot/native/vulkan.rs:170](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/native/vulkan.rs#L170)

- [x] **Duplication | Medium | Use one reconstruction generation key for Godot configuration changes.** integrations/godot/native/vulkan.rs:277 and integrations/godot/native/control.rs:314 compare backend, quality and DLSS preset field by field, conditionally increment a generation, then copy the same Settings structure. D3D12 uses this generation to retire stale SR contexts at integrations/godot/native/d3d12.rs:228 and Vulkan compares it with presenter readiness at vulkan.rs:398. A future reconstruction setting added to only one comparison would keep old resources eligible on the other renderer. Give Settings one reconstruction key or change predicate and cover each field and a generation-neutral setting once. Keep Vulkan device admission and D3D12 FG option delivery separate. [integrations/godot/native/vulkan.rs:277](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/native/vulkan.rs#L277)

- [x] **Altitude | Medium | Keep FG admission and recording outcomes together for each Godot Vulkan pass.** vulkan.cpp:626 shuttles a mutable VkFgPass through fg_begin, Runtime enable_options, a separate backend query, fg_admit and fg_recorded. The final-scene branch then rereads the session-wide fg_result, which may still describe an earlier pass when no presenter exists. Have the pass owner perform that sequence around typed recording callbacks and return the current pass outcome directly, including no presenter; publish diagnostics separately. [integrations/godot/native/vulkan.rs:436](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/native/vulkan.rs#L436)

- [x] **Duplication | Medium | Merge Vulkan extension names with one checked helper.** integrations/godot/native/vulkan/creation.rs:63 and games/unity-mono/src/vulkan_resolver.rs:243 retain the supplied order, compare NUL-terminated names by content and append absent requirements. Both add the Win32 memory/semaphore pair in their device paths at creation.rs:447 and vulkan_resolver.rs:286. Unity guards the u32 count limit and skips null entries during comparison, while Godot also merges vendor lists and converts the final length back to u32. Share a checked ordered merge with explicit null and count handling, and keep feature-chain construction, availability tests and retry policy with each caller. [integrations/godot/native/vulkan/creation.rs:63](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/native/vulkan/creation.rs#L63)

- [x] **Boundaries | Medium | Move XeSS feature-chain compatibility and fallback into Runtime device negotiation.** Godot decides which promoted Vulkan structures to insert for XeSS, which features to clear, and in what order to retry device creation without XeSS, interoperability and latency. vulkan_creation.cpp:175-192 patches the cloned chain before calling the existing Runtime XeSS requirement service. The compatibility rules concern the SDK and Vulkan structures, and would have to be repeated by another engine. Extend the Runtime negotiation interface to own the optional stages and report which were accepted. Leave Godot responsible for lending its base request, invoking its loader and returning the resulting device. [integrations/godot/native/vulkan/creation.rs:82](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/native/vulkan/creation.rs#L82)

- [x] **Altitude | Medium | Associate Vulkan feature readiness with the Godot device that acquired it.** A new device creation attempt clears the global XeSS/interop flags before the driver accepts it, whereas adapter facts change only on success. A failed secondary creation can therefore invalidate the live render device's capabilities; destroying a device does not remove its recorded adapter/readiness either. Build a capability receipt locally, publish it for the successful VkDevice and retire only that device's receipt; use the rendering device's receipt for provider admission. [integrations/godot/native/vulkan/creation.rs:371](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/native/vulkan/creation.rs#L371)

- [x] **Duplication | Medium | Drain the Godot Vulkan presenter once per shutdown attempt.** integrations/godot/native/vulkan.rs:241 disables and drains the presenter, then calls retire_contexts under the same guard. When an SR context is live, integrations/godot/native/vulkan/sr.rs:97 immediately drains it again before the independent device-idle check. Both calls reach rsf_fgvk_drain on the same presenter at integrations/godot/native/vulkan.cpp:317, and vulkan/sr/tests.rs:450 explicitly expects both calls. A successful first drain is followed by another synchronization and failure point with no intervening submission. Separate already-drained context destruction from live reconfiguration, keep the first disable/drain and device-idle proof on shutdown, and assert one presenter drain in the fixture. [integrations/godot/native/vulkan/sr.rs:97](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/native/vulkan/sr.rs#L97)

- [x] **Boundaries | Medium | Put Vulkan SR provider dispatch behind a single Runtime context.** Godot keeps separate generic and FSR4 context handles, chooses which create, plan, evaluate and destroy entry points to call, applies the DLSS preset, and decides when to run the spatial fallback. vulkan.cpp:325-418 forwards those choices to the respective Runtime APIs, while :564-566 repeats the dispatch for command retirement. Every caller of this path must understand provider-specific lifetime and continuation rules. Accept Godot's graph handoff, frame resources and callbacks through a generic Runtime session and return its effective plan, continuation and result. [integrations/godot/native/vulkan/sr.rs:188](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/native/vulkan/sr.rs#L188)

- [x] **Altitude | Medium | Keep diagnostic failures from skipping Godot's Vulkan spatial fallback.** The SR path propagates a failed dump or log with ? before deciding whether a refused evaluation needs fallback. The native leaves explicitly return failure on a caught exception. A diagnostic failure can therefore leave a reduced frame without reconstruction or turn a completed evaluation into an unreported failure. Record the actual evaluation/fallback outcome independently and treat diagnostic delivery as best effort. [integrations/godot/native/vulkan/sr.rs:252](https://github.com/KillerPixelCrew/ReScaleFrame/blob/894a55f9157a71835986ef3e9b64dc8023eedb0c/integrations/godot/native/vulkan/sr.rs#L252)

## Shader headers outside the C++ count

The following tracked Anomaly headers contain shader source and use the same file extension as native headers.

- [games/anomaly/shaders/core/rsf_core_pixel.h](games/anomaly/shaders/core/rsf_core_pixel.h)
- [games/anomaly/shaders/core/rsf_core_tess.h](games/anomaly/shaders/core/rsf_core_tess.h)
- [games/anomaly/shaders/core/rsf_core_vertex.h](games/anomaly/shaders/core/rsf_core_vertex.h)
- [games/anomaly/shaders/foliage/rsf_anomaly_foliage.h](games/anomaly/shaders/foliage/rsf_anomaly_foliage.h)
