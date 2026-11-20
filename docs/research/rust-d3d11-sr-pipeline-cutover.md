# D3D11 SR pipeline Rust cutover

The baseline is `5482c49` on `rust/orchestrator-anomaly`. The question was whether the D3D11 SR pipeline in `dlss_pipeline.cpp` (provider session, SR dispatch, reset and fallback, resource leases, output publication, quality and backend switching, the layer feature and teardown) could move into one Rust owner while `rescaleframe/dlss_pipeline.h`, its 16 exports and every SDK, D3D11, shader-pass and capture call stay where they were. This is a source port with synthetic, CPU and ABI checks. No device, window or game ran.

## Ownership after the port

| Boundary | Owner |
| --- | --- |
| Exports | `dlss_pipeline.cpp` keeps all 16 `rsf_dlss_pipeline_*` entry points as one-line forwards to `rsf_core_dlss_pipeline_*`, passing a static leaf table. The core object library still carries them, so the Runtime DLL, the AC7 `dinput8.dll`, the shared loader and the Minecraft DLL export the same names and ordinals. The `rsf_d3d11_sr_api` table, the D3D11 host leaves, `native_sr.cpp`, the AC7 bridge and the Anomaly adapter call the same exports as before. |
| Session and policy | `dlss_pipeline.rs`: Streamline load or shared host, device hand-off, support refusal (`NOT_SUPPORTED` against `STREAMLINE_FAILED`), DLSS render planning and range defaulting, the output target, the per-frame sequence, reset reasons (rebuilt decode, preset, quality, backend, output and colour-pass failures), the 120-failure give-up, log suppression, status counters, the request dump, the layer feature, quality and backend switching, resize and teardown. |
| Reused Rust owners | `frame_assembly.rs` assembles both scene and layer frames. `sr_legacy.rs` (only its visibility changed) is selected, evaluated, warmed up, asked for FG inputs and destroyed directly. `d3d11_output` leaves create and describe textures and `com_ref.rs` holds the device, output, layer output and zero-motion leases. `native_sr.rs` and `native_sr_state.rs` keep calling the exports; `native_regions` and `native_exposure` are unchanged in front of the pipeline. |
| Native leaves | `dlss_pipeline_platform.h` lists 43 calls: Streamline (`share_host`, `load`, `set_device`, `query_support`, `plan`, `release_resources`, `release_viewport`, `shutdown`, `evaluate`, preset validity), the native FG evaluate and capture, the native SR resource release, texture creation, clearing and zero initialisation, the motion decode and dense resolve passes, colour fidelity and colour transport, sharpening validity, defaults, vendor strength and the D3D11 pass, the legacy adapter constructor, the input dump and the two texture dumps. Each contains its own C++ exceptions and returns a failure value. |
| GPL helper | `runtime/graphics/src/sharpening/options.h` is GPL-3.0-only. Its `active` test, option mapping and the D3D11 pass stay behind the `sharpen` leaf, which returns success without allocating when the options are inactive and creates the pass on first active use. Nothing from it was translated. |

Public layouts (`rsf_dlss_pipeline_setup`, `_status`, `_layer`) and the private leaf structures are hand-mirrored in `dlss_pipeline_abi.rs` because bindgen-cli is not installed. `dlss_pipeline.cpp` asserts the same sizes and offsets, the result codes, dump views, format and bind constants with `static_assert`; the Rust module asserts them in a `const` block. Log lines, capture names and descriptions are unchanged, lines are still cut at 511 bytes, and the frame record uses the `%.9g` number form.

## Render input coverage

The migration changes no producer, shader or binding. Each SR input still comes from the adapter through the same frame structure and reaches the same pass with the same parameters.

| Input | Producer | Path through the pipeline | Change |
| --- | --- | --- | --- |
| Scene colour | adapter (`scene_color`, rect-local after `native_regions`) | colour transport encode when DLSS and transport is on, otherwise direct; transport decode and colour fidelity read it after evaluation | none |
| Depth | adapter (`depth`) | assembly, then the dense resolve's R32 depth for DLSS when reconstruction or a shared host needs it; sharpening and fidelity read the submitted depth | none |
| Game motion | adapter (`game_motion`, adapter-declared encoding) | decode pass (R32G32_FLOAT, per-render-size, not retried after a failed build), then the dense resolve for DLSS with the same `decoded_to_pixels`, sentinel and jitter; `motion_convention` sign rule reimplemented as `dlss_motion_scale` with the same truth table | none |
| Exposure | adapter, normalised by `native_exposure` | DLSS exposure, or dropped when transport encodes; alternate exposure policy follows `exposure_manual` | none |
| Opaque colour, translucency layer, reactive, coverage and bias masks | `native_translucency` and `native_regions` | copied into the DLSS frame; legacy adapter reads reactive and transparency | none |
| Cloud or motion depth layer | adapter | dense resolve `depth_layer` and legacy dense motion | none |
| Camera | adapter's view-buffer read | used as given; only `motion_decoded`, the sentinel pair, a zero `motion_scale` and `reset` are filled, as before | none |
| Layer colour and depth | AC7 separate translucency | viewport 1, one to one, R16G16_FLOAT zero motion, alpha carried, `frame_index` from scene counts | none |

Shader parity: no HLSL file, compiled shader or shader-pass source changed (`git diff --stat` touches only the pipeline, its leaf header, `lib.rs`, `sr_legacy.rs` and this note). The decode format, resolve parameters, transport and fidelity arguments and the sharpening `Frame` mapping (including the default motion scale of 1 for the alternate backends) are passed through the leaves unchanged. The full shader and buffer coverage gaps already recorded for each game stay open and are not affected by this port.

## Changed behavior

- The frame-thread entries (start, frame, layer preparation and evaluation, resize, backend selection, stop and quality on an alternate backend) take the owner with `try_lock`. A reentrant or concurrent call now refuses (start with `ALREADY_RUNNING`, the others with `NOT_RUNNING`, uncounted) instead of racing on unsynchronised fields. Single-threaded callers see no difference. `reentrant_frame_thread_calls_refuse_instead_of_racing` covers it.
- The pending reset, give-up flag, failure count, active backend and layer extent now live behind the cross-thread lock. Previously `set_preset` wrote some of them under the guard while the frame thread read them without it.
- `set_preset` writes its log line after releasing the lock rather than inside it.
- Leaves contain exceptions and return a failure. `resize_output` used to turn any exception into `RESOURCE_FAILED`; a throwing plan call now reports `STREAMLINE_FAILED` like a failed plan.
- The frame record path is opened as UTF-8, as `prefix_utf8` promises, instead of through the ANSI code page by `fopen`. ASCII paths are unchanged.

The integration merge gives a replacement output the same black clear as the startup target before publishing it. A refused resize leaves the existing output and its contents in place. The synthetic resize test covers both paths.

## Verification

- Baseline `5482c49`, VS2026 Release, vendor headers from the main checkout: Runtime 381, AC7 `dinput8.dll` 256, `ReScaleFrame.Loader.dll` 273 and `ReScaleFrame.Minecraft.dll` 267 exports, identical to the recorded lists in the main worktree's `build/rust-migration/baseline-exports`. After the port all four match those lists by name and ordinal.
- The full Release tree built with `/W4 /WX`, including every consumer of the exports (AC7 and shared carriers, Minecraft, the D3D11 host, the C++ Anomaly adapter) and the device fixtures `sr_pipeline`, `sr_backend_switch_device`, `anomaly_sr_temporal_device`, `native_reinsert` and `d3d11_host_shared_sr_device`. The fixtures were built only, not run. Debug built the Runtime, both carriers, Minecraft and the pipeline-facing contracts.
- Release passed `anomaly_sr_runtime_api_contract`, `d3d11_host_contract`, `overlay_dll_api(_c11, _no_sr)_contract`, `d3d11_present_api(_c11)_contract`, `d3d11_fg_api(_c11)_contract`, `native_window_contract`, `orchestrator_domain_contract`, `anomaly_sr_pass_contract`, `anomaly_input_events_standard/avx_contract`, `gpu_sr_policy(_embedded)`, `shared_shim_exports`, `game_plugin_contract`, `frame_assembly` and `sharpening_settings` (20/20). Debug passed `anomaly_sr_runtime_api_contract` and `d3d11_host_contract`.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` passed: the orchestrator crate runs 80 tests, 14 of them new. They use fake COM objects, a fake leaf table and the legacy adapter over a fake bridge, and cover start validation and teardown order, refused starts, the decode, resolve and history-reset sequence, render-size rebuild, colour transport, the 120-failure give-up and preset revival, the unretried decode build, the layer lease and history, resize identity on refusal and success, backend refusal and switching through `sr_legacy.rs`, reentrant refusal, the request dump and frame record, settings validation and `%.9g` formatting.

Not done: device fixtures, visible-window runs, the full `eng/verify.ps1`, and AC7, Anomaly, Skyrim, Fallout 4 or Steam game runs with backend, quality, resize, layer and teardown transitions.
