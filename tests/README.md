# Test fixtures

These fixtures check contracts, synthetic rendering paths and selected device prerequisites.
A fixture pass establishes the assertions in that fixture. It does not establish injected game
support, full-frame image quality or measured input-to-display latency. The source file headers
and non-obvious setup comments explain the boundary of each fixture.

Native targets are declared in [CMakeLists.txt](CMakeLists.txt) and enabled by the root project's
`BUILD_TESTING` option. See [the repository build instructions](../AGENTS.md#build-and-verify)
for the Windows/MSVC presets and Linux cross-build/Wine equivalents. Registered tests can be
selected with `ctest --preset windows-debug -R <test-name>` after a build. This document describes
source coverage and execution requirements; it records no test run.

## Interpreting results

- `rsf_sdk_c_header` and `rsf_fg_c_header` are OBJECT libraries. Their coverage is compilation as C,
  not execution. C++ runtime assertions for public frame policy are in `plugin_contract.cpp`.
- Tests configured with `SKIP_RETURN_CODE 77` report unavailable device/runtime prerequisites as
  skipped. Most manual SDK fixtures also return 77 with no arguments, so routine CTest does not
  perform their hardware evaluation paths.
- Some older graphics fixtures return **0** for unavailable devices or compilers:
  `d3d11_observer`, `d3d11_state`, `frame_tap`, `motion_decode`, `scene_promote` and `texture_dump`.
  Read their stage/skip output before treating a green suite as coverage of those graphics paths.
  The optional runtime branch in `dlss_backend` also requires its output to establish that it ran.
- WARP executes real Direct3D operations through software rendering. It is useful for arithmetic
  and lifetime contracts, but does not establish support on a vendor GPU. A Wine result describes
  the selected Wine graphics stack; it does not establish Windows/MSVC behavior.
- Several hook fixtures use process-global state and runtime vtables. CTest starts a separate
  executable for each registered test. Carrier startup tests keep the DLL loaded until process
  exit because its worker and hooks can still execute.

## Native contract and policy fixtures

These need Windows API availability, or the corresponding Wine environment, but no graphics
device or deployed vendor SDK unless a row explicitly says otherwise.

| CTest name or compile target | Source | Contract checked |
| --- | --- | --- |
| `rsf_sdk_c_header` (compile only) | [sdk_c_header.c](sdk_c_header.c) | Public Game SDK and inline frame eligibility compile as C. |
| `rsf_fg_c_header` (compile only) | [fg_c_header.c](fg_c_header.c) | FG/controller headers compile as C; ABI version and generated-count width stay explicit. |
| `game_plugin_contract`, `game_project_wingman_plugin_contract`, `game_unity_mono_plugin_contract` | [plugin_contract.cpp](plugin_contract.cpp) | Load each actual plugin DLL beside the fixture, check entry-point ABI, fingerprint recognition, inactive lifecycle and frame eligibility. Synthetic probes do not load a game. |
| `gpu_sr_policy` | [gpu_policy.cpp](gpu_policy.cpp) | PCI vendor defaults and software-adapter exclusion. |
| `ac7_view` | [ac7_view.cpp](ac7_view.cpp), [ac7_view_fixture.h](ac7_view_fixture.h) | Perspective view parsing, byte offsets, matrix consistency, pixel jitter and jitter removal at different rectangles. |
| `ac7_motion_capture` | [ac7_motion_capture.cpp](ac7_motion_capture.cpp) | Velocity diagnostic reason precedence and threshold/nonfinite cases. |
| `ac7_session` | [ac7_session.cpp](ac7_session.cpp) | Output-relative translucency sizing, draw recognition and temporary INI defaults/round trips. |
| `ac7_ui_rules` | [ac7_ui_rules.cpp](ac7_ui_rules.cpp) | UI classification, declaration fingerprints, retained-input false positives and unsafe diversion refusals. |
| `ac7_render_scope` | [ac7_render_scope.cpp](ac7_render_scope.cpp) | Delayed scope identity, nested restoration, bounded tickets, viewless Slate owners and quiescent destruction. |
| `resource_roles` | [resource_roles.cpp](resource_roles.cpp) | Recorded AC7 descriptor shapes, size/aspect guards, role confidence and ambiguous candidates. |
| `ui_identify` | [ui_identify.cpp](ui_identify.cpp) | Bounded membership, address reuse eviction, recency and CRC-based shader overrides. |
| `negotiate` | [negotiate.cpp](negotiate.cpp) | Vendor/API capability combinations, shared sessions, fallbacks and refusal reasons. |
| `frame_assembly` | [frame_assembly.cpp](frame_assembly.cpp) | Camera/resource translation, motion conventions, optional exposure and unusable-frame refusals. |
| `sr_session` | [sr_session.cpp](sr_session.cpp) | Deterministic provider replacement, owned path storage, generation/ID validation and history resets. |
| `fg_session` | [fg_session.cpp](fg_session.cpp) | Quiescent replacement, one chain owner, failed probes/creation/retirement, fallback and CPU marker sequencing. |
| `fg_choice` | [fg_choice.cpp](fg_choice.cpp) | Saved provider/Off selection, renderer masks, corrupt values and failed persistence. |
| `render_links` | [render_links.cpp](render_links.cpp) | Delayed 64-bit frame identity, bounded lifetime tickets, stale reuse and concurrent producer collision. |
| `module_dump_entropy` | [module_dump_entropy.c](module_dump_entropy.c) | Known entropy distributions, sampled windows and small-range fallback. |
| `module_dump_self` | [module_dump_self.c](module_dump_self.c) | Fixture-module dump mapping, live bytes, entropy/reference scans and a known import. CTest supplies a build-directory output path. |
| `fsr_backend` | [fsr_backend.cpp](fsr_backend.cpp) | FSR/XeSS ABI and refusal/capability reporting in compiled-in and compiled-out configurations. |
| `dlss_backend` | [dlss_backend.cpp](dlss_backend.cpp) | DLSS pre-load arguments, ordering, load failure and signature-policy refusals. `RSF_STREAMLINE_BIN` enables additional driver queries. |
| `shared_shim_exports` (MSVC only) | [shared_shim_exports.cpp](shared_shim_exports.cpp) | Actual dinput8 carrier exports, ordinal 1 and version-resource forwarding. |
| `launcher_version` | Launcher target | Built launcher reports the root CMake project version. |
| `unity_sr_d3d12_contract` | [unity_sr_d3d12.cpp](unity_sr_d3d12.cpp) | No-argument native ABI/layout and null/short setup refusals. Hardware evaluation is manual. |
| `ac7_proxy_startup` | [d3d11_present_bridge.cpp](d3d11_present_bridge.cpp) with `--proxy-smoke` | Built AC7 carrier loads, exposes required entry points and allows graphics startup. The carrier stays process-owned. |

## Graphics fixtures

These create real Direct3D objects and, where stated, read back pixels. Hardware-first fixtures
may fall back to WARP. Availability and skip handling are determined by the source and the CTest
properties, not by a general assumption that software rendering is available.

| CTest name | Source | Device path and coverage |
| --- | --- | --- |
| `ac7_capture_io` | [ac7_capture_io.cpp](ac7_capture_io.cpp) | D3D11 WARP and compiler: shader callbacks across all six stages, constant bytes, draw/dispatch capture and lossless colour/depth files in a temporary directory. |
| `constant_override` | [constant_override.cpp](constant_override.cpp) | D3D11 WARP and compiler: inherited VS/PS/GS/HS/DS constants, duplicate slots, pixel effects and restoration after draws/ClearState. |
| `ac7_native_graph` | [ac7_native_graph.cpp](ac7_native_graph.cpp) | D3D11 WARP plus synthetic engine vtables/command queue: native SR insertion, blue fallback readback, delayed leases/frame identity and restored engine sizes/uniforms. |
| `d3d11_observer` | [d3d11_observer.cpp](d3d11_observer.cpp) | Device/swap-chain interception: descriptor callbacks, filtered retention, Present-triggered dumps, logging and restored hooks. CTest supplies its output directory. |
| `d3d11_state` | [d3d11_state.cpp](d3d11_state.cpp) | D3D11 hardware: save/clear/restore pipeline bindings and release getter-owned references. |
| `frame_tap` | [frame_tap.cpp](frame_tap.cpp) | D3D11 runtime and compiler: draw reports, candidate filtering, re-entry, substitution, diversion, composed-colour discovery, depth replay and binding restoration. |
| `motion_decode` | [motion_decode.cpp](motion_decode.cpp) | D3D11 hardware/WARP and compiler: independent packed-motion arithmetic, clear sentinel, valid zero and output scaling. |
| `motion_resolve` | [motion_resolve.cpp](motion_resolve.cpp) | D3D11 WARP: object vectors versus unwritten camera motion and nearer translucent-depth reprojection. |
| `colour_transport` | [colour_transport.cpp](colour_transport.cpp) | D3D11 WARP: analytic encode/decode, FP16 round-trip tolerance and saturated highlight recovery. |
| `native_translucency` | [native_translucency.cpp](native_translucency.cpp) | D3D11 WARP: rect-local opaque snapshots, reactive/coverage masks and identity-keyed cloud-depth handover. |
| `scene_promote` | [scene_promote.cpp](scene_promote.cpp) | D3D11 hardware: typed views and phase-gated promotion plans over synthetic allocations. Game postprocess image correctness is separate. |
| `texture_dump` | [texture_dump.cpp](texture_dump.cpp) | D3D11 hardware/WARP: chosen texels, packed-motion statistics, mip selection, progress output and cross-device refusal. |
| `ui_layer` | [ui_layer.cpp](ui_layer.cpp) | D3D11 hardware/WARP and compiler: transparent layers, colour-preserving alpha-factor/write-mask corrections and multiple-draw coverage. |
| `fullscreen_pass` | [fullscreen_pass.cpp](fullscreen_pass.cpp) | D3D11 hardware/WARP and compiler: premultiplied composition, transfer functions, format tolerances and viewport/scissor restoration. |
| `overlay_host` | [overlay_host.cpp](overlay_host.cpp), [overlay_panel_fixture.cpp](overlay_panel_fixture.cpp) | D3D11 Present hooks and deterministic panel DLL: helper/presenting device adoption, drawn pixels and resize. |
| `shared_surface` | [shared_surface.cpp](shared_surface.cpp) | Matching-adapter D3D11/D3D12: shared NT handles and cross-API fence visibility. Unsupported interoperability skips. |
| `fg_leases` | [fg_leases.cpp](fg_leases.cpp) | D3D12 WARP: COM retention and independent submission/vendor completion gates. CPU-signalled fences deliberately isolate lease behavior. |

## Manual SDK/device paths

The tests below are registered with no SDK arguments and normally skip in routine CTest.
Run the corresponding built executable directly with the required local SDK paths to exercise
the hardware path. The arguments shown follow the executable's own usage checks. SDK builds,
DLL versions and device support still determine whether a supplied configuration can run.

| CTest name / executable | Arguments | Coverage boundary |
| --- | --- | --- |
| `sr_bridge` / `rsf_sr_bridge_test` | `<FFX-dir> <XeSS-dir> [adapter-index]` | FSR/XeSS SR across the D3D11/D3D12 bridge, provider/quality changes, exposure rebuilds and constant-colour readback. |
| `sr_pipeline` / `rsf_sr_pipeline_test` | `<Streamline-dir> <FFX-dir> <XeSS-dir>` | NVIDIA synthetic SR evaluation and provider switching with D3D11 state preserved. |
| `fg_provider` / `rsf_fg_provider_test` | `<backend> <runtime-dir> <vendor-id>` | Provider/chain lifecycle, disabled Present and retirement. Backend IDs: 1 DLSS, 3 FSR3, 4 FSR4, 5 XeFG. |
| `fg_switching` / `rsf_fg_switching_test` | `<FFX-dir> <XeFG-dir> <vendor-id>` | FSR3/XeFG/plain owner replacement with FG disabled. |
| `latency_host` / `rsf_latency_host_test` | `<runtime-dir> <vendor-id> <profile>` | Reflex (1) or PCL (2) API ordering, 64-bit lineage, counts and queue retirement. No physical latency measurement. |
| `d3d11_present_bridge` / `rsf_d3d11_present_bridge_test` | `<Streamline-dir> <vendor-id> [mode]` | Synthetic AC7 bridge resource/token ordering; optional modes enable FG/visible presentation/pacing. |
| `shared_fg_present_bridge` / `rsf_fg_present_bridge_test` | `<backend> <SDK-dir> <vendor-id> <11\|12\|12-hot> [XeSS-dir] [extra-runtime-dir]` | Shared D3D11/D3D12 bridge, provider switching, abort/refusal recovery and generation identity. Extra-runtime routing depends on the selected path; see source setup. |
| `unity_sr_d3d12_contract` / `rsf_unity_sr_d3d12_test` | `<backend> <Streamline-dir> <FFX-dir> <XeSS-dir> [vendor-id] [shared]` | Synthetic D3D12 SR, restored resource states and optionally shared Streamline SR/FG constants. The routine no-argument path runs only contract guards. |

`d3d11_present_bridge` modes are `--enable`, `--ordering-only`, `--enable-visible`,
`--enable-visible-10bit` and `--enable-visible-pacing`. The fixture also reads
`RSF_TEST_FRAME_LIMIT_US` and `RSF_TEST_SUBMISSION_STALL_MS` for its pacing/stall experiments.
`RSF_UNITY_GPU_DEBUG` requests the D3D12 debug layer in the shared FG and Unity SR hardware
fixtures. These options create synthetic experiments; they do not change routine registrations.

Additional manual paths:

- `ac7_view <capture-directory>` reads the bounded `capture00_cb4096.bin` through
  `capture99_cb4096.bin` naming scheme. The routine test uses only synthetic view buffers.
- `ac7_ui_rules <shader-file>` validates the exact local captured HUD-producer instruction guard,
  then mutates the bytes to check refusal. The shader file stays untracked.
- `dlss_backend` reads `RSF_STREAMLINE_BIN` for its optional runtime/device queries. See
  [backend deployment notes](../runtime/backends/README.md) for the required Wine stack.
- `overlay_host <panel-DLL>` can use the shipped egui panel instead of the deterministic fixture;
  `RSF_TEST_RENDERDOC_DLL` optionally loads a locally installed capture DLL.
- `rsf_exposure_probe` is built from [tools/exposure-probe.cpp](../tools/exposure-probe.cpp) but has
  no CTest registration. It is an explicit hardware experiment.

## Managed Unity contract fixture

[unity-managed/ReScaleFrame.Unity.ContractTests.csproj](unity-managed/ReScaleFrame.Unity.ContractTests.csproj)
builds [Program.cs](unity-managed/Program.cs) for `net10.0` and `netstandard2.1`.
`UnityManagedDirectory` must locate the matching game's Unity/URP assemblies. The project uses
the deployed `net472` Harmony DLL, and [Directory.Build.props](unity-managed/Directory.Build.props)
keeps outputs/intermediates under `build/managed/unity-tests`.

The CoreCLR executable checks packet sizes/offsets, all expected URP method signatures and
imported-backbuffer forwarding with a replaced enqueue delegate. It restores the delegate and
bootstrap state even if the check throws. It does not execute a Unity render context.

`rsf_unity_mono_runtime_test <Mono-DLL> <framework-directory> <test-assembly>` loads Mono in an
isolated process and invokes `Program.Run` in the `netstandard2.1` assembly. That path also patches
and unpatches a non-inlined fixture method using the deployed Harmony assembly. The runner is
built but not registered with CTest because its runtime/assembly paths are supplied manually.
