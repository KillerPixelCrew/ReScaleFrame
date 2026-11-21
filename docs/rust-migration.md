# Rust orchestrator and integration migration

Branch `rust/orchestrator-anomaly` starts at `2db07f4`. The first cutover replaces the orchestrator behind its existing C interfaces. The second replaces the Anomaly adapter. Each production path has one implementation during the move.

## Remaining integration ports

Carry the same Rust ownership and public C ABI through every in-tree game
and engine integration. Retain the engine-required managed entry points,
JNI and Godot binding surfaces, and the native calls that require SDK types
or fault guards. Adapter code continues to supply engine facts through
the existing GameSDK interfaces, with backend and pacing policy in the
Runtime. Preserve the supported paths, feature refusals and package names.

| Integration | Native ownership and entry points to trace |
| --- | --- |
| Project Wingman | The game DLL and its dedicated XInput startup owner are in Rust. Both XInput carriers share the native forwarding leaf. Current binary and live verification remain open. |
| RDR2 | The game DLL renderer, ASI and attach workers, input guard, probes, Social Club backbuffer owner, capture admission, recording and readback receipts, artifact publication and diagnostic workers are in Rust. The D3D12 failure trace is installed and written by Rust using native caller and stack facts. Continue capture lease ownership. |
| Unity Mono and Unity package | Mono loader, plugin lifecycle, Runtime setup, package services, per-view D3D12 SR, presentation and input, D3D11 and Vulkan bridge state, Vulkan resolver, shared device, images, command pool and slots, DXGI swapchain and resize, and Editor preview admission and scheduling are in Rust. Continue checked package release. |
| Steam, Fallout 4 and Skyrim SE | The shared Steam chain, both games' cold creation, identity and session lifecycle, and Skyrim SE observer admission, pre-HDR camera cache, sampler, presentation window and auxiliary ownership are in Rust. Continue both games' rendering, CPU facts, compatibility and recovery. |
| AC7 | The game DLL's GameSDK, native site admission, scene and velocity patches, renderer transaction, TrueSky, queued scopes, view buffer, UI, scene colour, contact-shadow rewrite, and all five velocity shader stages and their codecs are in Rust. Continue motion capture and game-side carrier ownership. |
| Godot | The native control, D3D12 and Vulkan sessions and SR contexts, D3D12 submission ledger, window and input routing, and Vulkan creation requirements are in Rust. Continue Vulkan Auto provider routing, effect diagnostics and engine bindings while preserving the GDExtension and compositor contracts. |
| Minecraft | The native session, surface, message observer, frame packets, settings and status are in Rust. The Java mod, typed JNI marshaling and linked Runtime entry points retain their existing contracts. |

Merge coherent ownership changes to the migration branch and remove their
worker worktrees. The user runs builds and verification. Record each
candidate's revision and feature set, and record binary ABI, focused
fixture, device, and game or engine evidence separately. The earlier
orchestrator and Anomaly verification gaps remain open.

## What the current build and callers require

| Surface | Evidence in this tree | Migration consequence |
| --- | --- | --- |
| Runtime linkage | `runtime/orchestrator/CMakeLists.txt` builds a core object library, a DLL with additional D3D11 and overlay services, and a separately linked native Vulkan archive. `loader/proxy` and the Minecraft target also link core objects. | Check both linkage forms. A successful DLL import test covers only one. Preserve enabled optional exports for each build configuration. |
| Plugin lifetime | `plugin_session.rs` validates the plugin table, copies host services, counts readers and can return a live handle after failed preparation when retirement is busy. Anomaly bootstrap resolves prepare and start by name. | Preserve result and handle together, hold the module through the last callback, and retry retirement. |
| Input and output | The game SDK describes frame identity, camera, CPU events, passes and resource leases. The Anomaly SR driver supplies scene inputs and consumes an output texture. FG obtains presentation and tagging services separately. | Keep source frame, view, format, extent, ownership and retirement visible at each call. Preserve separate SR output and final presentation ownership. Add an ABI field only for a demonstrated missing fact. |
| Additional Anomaly imports | The SR and motion stages share `frame_dump.rs` to borrow the carrier-loaded Runtime and resolve its capture exports. The SR driver and live overlay use the colour-transport setter. The shader and overlay owners query patching, diagnostic and named texture services. | Check direct lookups as well as table entries. Keep optional-service behavior and the adapter's single Runtime instance. |
| GPU and vendor work | Sessions, provider selection, pacing, presentation, diagnostics and overlay live across core and linked libraries. Native D3D11, D3D12, GL and Vulkan have distinct submission and retirement paths. | Port policy once. Keep foreign SDK calls at their existing owner and retain API-specific synchronization. Anomaly exercises only part of this surface. |
| Game-owned work | Anomaly builds its engine hooks, input handling, shader interception, motion history, SR reinsertion and FG input preparation in the game DLL. Its motion patch pack is built separately. | Retain verified build profiles, live settings, shader pack layout, failure paths and packaging when changing implementation language. |

## Order of work and acceptance

1. Establish the native baseline from the branch revision. Record runtime exports and direct/embedded consumers for matching features. Run the existing focused CPU and C ABI tests. Fix discovered correctness defects at their owner and carry the regression coverage across the cutover.
2. Introduce one workspace crate for core code. Start with SR backend selection, whose two scalar C exports already have a native caller test. Link the same implementation into both forms, retire the corresponding C++ implementation and verify symbol names and behavior. Port plugin sessions and frame sequencing next. Exercise short structs, version refusal, busy shutdown and failed startup.
3. Move settings, diagnostics, overlay services, rendering sessions, latency, graphics and presentation ownership. Keep the published headers authoritative. Inventory each service's callers and failure paths before switching it. Compare exports against the baseline for each enabled backend and exercise existing consumers on their graphics APIs.
4. Run the current C++ Anomaly adapter with the new runtime. Check standard and AVX profiles, overlay changes, quality and backend switches, camera movement, moving geometry, UI, FG, resize, fallback and teardown against recorded acceptance. Capture the evaluation frame and resource identity for rendering comparisons. Recheck other linked and DLL consumers before retiring the old core.
5. Port Anomaly behind its existing game DLL entry and package it with the same runtime. Repeat the fixture, device and game checks on the Rust adapter. Record known pre-existing gaps separately from regressions. Other games retain their current adapters.

The [Anomaly release record](releases/v0.1.2-anomaly.md) distinguishes the shipped payload from later shader-provider work. It supplies historical acceptance only. Record validation of this branch in the [implementation tracker](implementation.md).

## C ABI comparison

The GameSDK and runtime contract headers match `2db07f4` byte for byte, including GameSDK ABI 24. The source audit at `6414a05` compared the same 88 headers across the SDK, runtime, and Anomaly. Four headers differed after removing comments and whitespace. `native_fg.h` adds three linked-runtime declarations, `d3d11_present_bridge.h` adds `rsf_d3d11_present_source_queue`, and `overlay_api.h` raises `RSF_OVERLAY_SERVICE_OPTION_LIMIT` from 24 to 32. `shader_compile.h` changes the compiler helper's native ownership. All existing C function signatures and public record layouts matched. At `2ceec77` the public include tree still matches that audit and the tracked carrier `.def` files still match `2db07f4`.

On November 21, 2026, both archived MSVC Release trees (`build/rust-migration` and `build/rust-migration-streamline`) matched the recorded case-sensitive export names and ordinals for Runtime (381), AC7 (256), Loader (273), Minecraft (267), and the staged C++ Anomaly adapter (3). Neither archived Anomaly DLL imports the Runtime DLL statically. Both archives predate `d4de507`.

The Runtime version entry point now returns the workspace version from
Rust with the existing C signature and static string lifetime. On the
pinned Rust toolchain, the rebuilt Streamline-enabled Runtime returned
`0.1.2` on two direct DLL calls, matching `VERSION`. All four Release
export inventories matched again. Five focused MSVC DLL and embedded
contracts, 333 locked Rust tests, formatting and Clippy passed. The
rebuilt Godot extension also matched all 398 export names and ordinals
and kept its embedded Runtime linkage.

Callers still need to account for documented behavior changes. Frame assembly requires the output record to advertise its capacity and refuses a zero or short capacity before writing. Overlay settings callbacks can read the committed values, and a second write returns `BUSY` while the first is active. Vulkan capture and SR recording require completion proof before readbacks can be published or reused. The [implementation tracker](implementation.md) records the fixture coverage for those changes.

## Shader patch and Anomaly configuration ownership

The Runtime DLL publishes the generic shader patch service through
`rsf_get_shader_patch_api`. Anomaly borrows that table from the
carrier-loaded Runtime. Rust owns the shared pack rules, claim registry,
proof of the original shader, transformed output, reflection policy,
cache and interception inventory. The adapter owns the admitted motion
manifest and its lookup. The merged Runtime reopened the old C++ cache,
and Standard compiled new replacements in a cold game cache.
[Integrated shader patch evidence](research/rust-shader-patch.md#integrated-runtime-and-cold-cache-standard-run)

The Rust SR stage owns the retained INI, vendor directories and live
settings. The native facade no longer keeps its second configuration
record. Standard verified the packaged DLSS and XeSS directory defaults
on the exact staged Release candidate.
[SR configuration evidence](research/rust-anomaly-sr-configuration.md#coordinator-integration-and-game-checks)

The shared presentation and overlay diagnostics now name the graphics
path. Native D3D12 Streamline registration follows the adapter's backend
and runtime-switching request. The DXGI fullscreen description preserves
the refresh rate supplied by the caller. The Runtime overlay and embedded
carrier use the same host owner and matching panel ABI check.

## Integrated linkage audit

CMake builds `rsf_orchestrator_rust` once for each selected target and
configuration. The Runtime DLL, the two combined DirectInput carriers
and Minecraft link it through `rsf_orchestrator_core`. The Runtime
shader patch library links that Rust owner and the existing native
compiler, reflection, JSON and hook leaves. The thin carrier loads the
Runtime DLL. Anomaly borrows the already-loaded module with
`GetModuleHandleW` and resolves the C tables by name. The public GameSDK,
runtime contract and shader patch headers match the baseline byte for byte.

The remaining shared D3D11 SR surface admission and release sequence now
run in Rust. The pipeline closes admission before releasing the surface,
and the public C entry points retain their signatures and exports in the
rebuilt DLL and embedded consumers.
[Surface and teardown checks](implementation.md#rust-orchestrator-and-first-anomaly-port)

## Source audit checkpoint

The integrated ABI audit examined `41aa6bd` and traced the later
Streamline host and FSR4 compatibility link paths. It matched 29
generated ABI files, the 18 frame-tap C entries and their native table,
and the SDK guards on the D3D12 provider calls and CMake targets. The
audit found one layout defect in the private D3D11 state snapshots.
The Windows SDK defines 253 class-instance entries, and both sides
now check the corrected layout. See commit `bc9256f`.

Source review traced Anomaly's by-name Runtime lookups and its linked
Graphics calls. Windows link verification still needs to cover binaries
that consume more than one Rust
archive. Anomaly links its adapter archive and the shared Graphics
owner. The Runtime and combined carriers also link the FSR4 compatibility
archive. With the corrected Streamline runtime present, the Runtime
also links the Streamline Vulkan lifecycle archive. The standalone
FidelityFX 4, Streamline and XeSS Vulkan bridges use the Windows GNU
target.

Build and fixture results below apply to their recorded candidates.
The Runtime and Anomaly binaries found under `build/` at `afe5eee`
predate the merged policy cutover.

| Check | Evidence still needed for the merged source |
| --- | --- |
| Build and ABI | Windows builds of the Runtime DLL, thin and combined carriers, Minecraft, Unity Native, Godot Native and Anomaly. Compare each PE export inventory against its matching SDK configuration, including ordinals and carrier forwarders. Check that Anomaly still resolves the already-loaded Runtime by name. The `native_vulkan_dll_exports` fixture checks a subset of named entry points through `GetProcAddress`. The standalone Windows GNU Vulkan bridges also need link verification. |
| Focused contracts | Run `input_receipts_contract`, `native_cpu_reservation_contract`, `native_fg_cpu_contract`, `d3d11_host_contract`, `sharpening_settings`, and the Rust input receipt and D3D11 capture budget cases. Check `plugin_session_lifetime` and `plugin_session_lifetime_embedded`, `callback_host_abi` and `callback_host_abi_embedded`, the Anomaly lifecycle and both profiles' input and SR hook contracts, and the enabled Vulkan provider contracts. Record the build commit and feature set with the results. |
| Device | Check provider switches, fullscreen and resize, GPU retirement, D3D11 state restoration, OpenGL sharing, capture completion and overlay drawing on the affected APIs. |
| Game | Use the same candidate on Standard and AVX. Record movement and held input, SR and FG switching, overlay capture and release, resize and fallback, and the observed shutdown sequence. Record an engine quit and explicit plugin retirement separately. |

## Native policy cutover

The source audit of the Runtime and combined carriers identified input
receipt routing and D3D11 capture budgets as the last native policy
helpers in the production core. The rights holder confirmed both helpers
were written for ReScaleFrame and authorized MIT. Commit `b2b1c5a` records
that authorization. The D3D11 host owns the capture budgets in Rust.
The Rust CPU dispatcher owns receipt replay, and FG frame routing uses
the Rust unique-owner selector. Their former native implementations and
private callbacks are removed. The CPU event and dispatch C signatures
are unchanged. Build and export verification of this cutover remain open.
The archived Release inventories in `build/rust-migration/baseline-exports`
give the following expected ordinals for builds with matching options.

| Binary | `rsf_native_cpu_dispatch` | `rsf_native_cpu_event` |
| --- | ---: | ---: |
| Runtime DLL | 227 | 228 |
| AC7 carrier | 123 | 124 |
| Loader carrier | 140 | 141 |
| Minecraft DLL | 134 | 135 |

`rsf_negotiate` in `runtime/contract` has a native fixture and no
production caller. The running Runtime and adapter use the Rust backend
and session owners.

## Verification coverage

| Requirement | Recorded evidence | Still open |
| --- | --- | --- |
| Core ownership | Rust owns plugin sessions, frame assembly, capture, provider admission, latency and pacing, presentation buffers, readbacks, paired physical DXGI swapchain and queue, and shared D3D11 panel state. The OpenGL and FidelityFX Vulkan WSI routes also use Rust ownership. The merged Standard clone exercised native rendering, DLSS SR and a DLSS-G to FSR3 and back provider transition. [Implementation tracker](implementation.md) · [Shared overlay host evidence](research/rust-overlay-host.md#integrated-runtime-and-standard-game-run) | The latest fullscreen, D3D11-on-12 overlay wrapping and provider cutovers need device verification. |
| Stable C ABI and linked consumers | The merged shader patch and configuration build kept all eight checked Release PE inventories and Anomaly's by-name Runtime lookup. The enabled Debug and Vulkan Streamline-disabled Release builds matched the four core export inventories and Anomaly's three exports. Both built and ran the public shader patch cache-reopen probe. [Build variants](research/rust-shader-patch.md#avx-cold-cache-and-build-variants) | Recheck affected artifacts after further changes. |
| C++ Anomaly baseline | Standard and AVX ran the existing C++ adapter with the Rust Runtime. Standard exercised the overlay, held-key gameplay, SR switching and DLSS-G. Both shut down through the engine. [C++ adapter baseline](research/rust-anomaly-cpp-baseline.md) | The baseline covers the exact recorded executable and payload hashes. |
| Rust Anomaly game acceptance | Both profiles reloaded the Rust manifest in the foreground and drew the merged overlay through resize. Standard switched native and DLSS SR and changed FG providers. AVX switched SR providers with DLSS-G active and returned gameplay mouse input after closing the panel. The combined build compiled byte-identical cold shader caches in both profiles and passed the native AVX opt-out run with a gameplay backbuffer. Earlier Standard evidence covers refused-SR spatial recovery. [Cold-cache results](research/rust-shader-patch.md#avx-cold-cache-and-build-variants) · [AVX opt-out](research/rust-anomaly-sr-configuration.md#merged-avx-opt-out-retest) | Verify held-key gameplay on the Rust adapter. The pointer-bound Forward attempt left the player indoors and the capture position stationary. [Movement attempt](research/rust-overlay-host.md#pointer-bound-movement-attempt) |
| Device and shader evidence | Offscreen SR and sharpening runs, exact Vulkan capture readbacks and live Anomaly results are recorded separately from synthetic contracts. Standard and AVX verified raw SR inputs and outputs. Matched AVX evaluations cover camera cuts, pans, small window-region motion, animated first-person geometry and zero fill with missing generated motion. The pending OpenGL, D3D12 readback, overlay and presentation fixtures link on the merged MSVC Release build. [SR device results](research/rust-anomaly-sr-pass.md#offscreen-device-acceptance) · [First-person zero fill](research/rust-overlay-host.md#first-person-zero-fill-with-missing-motion) · [Merged fixture builds](research/rust-overlay-host.md#merged-device-fixture-builds) | Moving world characters, OpenGL sharing, D3D12 readback, D3D11-on-12 overlay, fullscreen and windowed Vulkan DLSS-G remain open. |
