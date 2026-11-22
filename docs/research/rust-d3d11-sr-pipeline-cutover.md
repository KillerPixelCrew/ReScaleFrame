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


## D05 source follow-up

The D05 follow-up traces the existing Rust D3D11 pipeline from provider selection through output
publication, and the D3D12 DLSS backend from device registration through command recording and
viewport release. Its base was 3cadb40a, after the C03 settings and C05 latency changes.

| PORT.md finding | Source evidence |
| --- | --- |
| Give DLSS viewport resources the same ownership as the contexts that use them. | dlss_native12.rs allocates a distinct process-local viewport for each context, reserving IDs 0 and 1 for existing paths. Constants, options, resource tags, evaluation and release use that context's ID. The registration stays leased through the context's Drop. The allocator fixture covers uniqueness and overflow. |
| Use one mapping for adapter-supplied Streamline engine metadata. | streamline_policy.rs maps custom, Unreal and Unity for both dlss_streamline.rs and dlss_native12.rs. Native SDK leaves check the enums. The metadata comes from the adapter and does not choose an engine-specific implementation. |
| Start the requested D3D11 SR provider without requiring DLSS to initialize first. | d3d11_host passes its selected backend in the existing private startup record. dlss_pipeline.rs starts that provider and retains the device and output across switches. It acquires Streamline on the first DLSS request and retires a failed new acquisition without replacing the active alternate backend. The host issues DLSS Present calls after a successful DLSS start or switch. Public startup and service-table layouts retain their existing contracts. |
| Build dense-motion resolve parameters from one conversion. | motion_resolve_params.rs serves dlss_pipeline.rs, native_sr.rs and sr_legacy.rs. Callers supply canonical UV scales, the clip transform, sentinel, depth layer and jitter. The scene path converts NDC explicitly. The legacy path passes zero jitter and the native FG path passes source-frame jitter. Normalized motion is previous-minus-current render pixels. |
| Distinguish original and submitted resources in the requested SR dump. | dlss_pipeline.rs retains the engine colour, depth, motion and exposure labels and adds submitted colour, depth and exposure beside submitted motion. The DLSS descriptor comes from the evaluation call. sr_legacy.rs returns the exact D3D11 bridge frame after dense motion and depth conditioning. The Rust dump fixture checks the resource identities for encoded colour, resolved depth and motion, and suppressed exposure. |
| Commit successful SR quality changes through the same history reset. | dlss_pipeline.rs plans with the selected provider under the frame owner, then commits the range and resets history and the failure limit in one shared path. The alternate-provider fixture reaches the failure limit before changing quality. |
| Switching back to DLSS drops the zero-range fallback and can refuse every later frame. | One DLSS planner resolves absent bounds to the optimal dimensions for start, quality, resize and switch. The alternate-to-DLSS fixture switches with zero reported bounds and evaluates at the optimal size. |
| Resolve a DLSS viewport once for constants, options, tags and evaluation. | dlss_streamline.rs rejects unsupported D3D11 viewport IDs before obtaining a token and supplies the same valid slot to every operation. The Rust route fixture checks the common slot and the native ABI fixture covers invalid IDs. |
| Share the projection-to-forward conversion with an explicit orthographic convention. | streamline_policy.rs computes the forward vector from the projection and view basis for D3D12 DLSS SR, Streamline FG through streamline_frame.h and FSR FG through d3d12_generation/fsr.rs. The zero-W fallbacks remain negative for SR and positive for FG. Its fixture covers both perspective signs and both orthographic choices. |
| Exercise the production DLSS motion-sign rule in the frame-assembly fixture. | dlss_pipeline_tests.rs runs the raw, dense-resolved and unspecified vectors through dlss_motion_scale. The duplicate C++ implementation and vectors were removed. tests/frame_assembly.cpp retains its ABI and frame-assembly tests. |
| dlss_pipeline re-implements the %.9g formatter that frame_capture_vk already exports | The D3D11 frame record uses frame_capture_vk::general. The existing shared formatter fixture now also pins signed NaN. |
| Pass the existing DLSS support and planning records through the pipeline leaves. | dlss_pipeline_platform.h, dlss_pipeline_abi.rs and the C++ leaves now use the existing size-checked DLSS setup, support and plan records. The Rust owner applies the range fallback. The generated declarations and public layouts did not change. |

The first-party startup record construction, Windows absolute-path check and projection math
moved into Rust. The native DLSS files still marshal Streamline C++ types and call Windows and
vendor APIs. streamline_sr.h now provides one SDK options translator for both Direct3D routes.
dlss_preset.h remains the inline C compatibility validation for SDK-free callers. The native
dlss_backend and sr_pipeline fixtures retain the actual facade, optional SDK and real D3D11
device coverage. The synthetic route, switching, history and dump assertions are Rust fixtures.

Source verification used the pinned 1.94.1 toolchain's rustfmt on changed Rust files and
git diff --check. A source comparison matched all 40 callback names and positions in the
private D3D11 pipeline tables and found both size assertions still expected 43 pointers. The
assertions now match the actual tables. Public headers and reused generated records matched
the base after stripping comments and whitespace.
No generated declaration required regeneration. Builds, tests, lint, devices and game
acceptance remain user-owned. This follow-up has source evidence only.
