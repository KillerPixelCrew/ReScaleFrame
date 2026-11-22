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

## Streamline SR payloads after `fb27ab2f`

Checked Streamline 2.14.1 at `2122257e0fce486f91b385aa63b9a09b0a34b363`. The SR
owners in Rust now select the camera fields and options, pack the six preset
slots, and consume one common optimal-settings record. The D3D11 planner keeps
the reported minimum and maximum sizes. Native D3D12 continues to reject a
successful plan with a zero optimal dimension before publishing its quality.

With MSVC x64, `sl::Constants` is 456 bytes. SR writes offsets 32 through
159 and 224 through 447, leaving the native version header, optional lens
matrix and later defaults alone. D3D12 still sets the motion-jittered flag
explicitly to false. The 88-byte `sl::DLSSOptions` has dimensions at
offsets 32 through 43 and six presets at 60 through 83. Planning applies
dimensions only. The SDK optimal-settings object is 64 bytes, with its
seven contiguous result fields at offsets 32 through 59. The C++ shim
asserts the copied field sizes and relative offsets against the SDK headers.
Bindgen derives the Rust layout of the private C payloads.

The owned D3D11 session caches the two common Present hooks after both
lookups succeed. The shared route still delegates Present to the D3D12 host.
C++ guards the vendor calls, including the Present hook's `bool&` skip
argument. It also constructs the versioned SDK objects, frame and viewport
arguments, and the resource and tag arrays. SDK preferences still borrow
native arrays through shutdown. Signature verification stays in its single
translation unit. Native D3D12 keeps the DirectXMath inverse and determinant
as its singularity oracle. `streamline_frame.h` still declares the shared
constants entry and `streamline_load.h` still serves the D06/MFG callers.

MSVC syntax passed with the pinned headers present and with Streamline
compiled out. The unmodified D06 generation source also passed SDK-enabled
syntax against the shared headers. Bindgen 0.72.1 regeneration, Rust 1.94.1
formatting and locked offline Cargo metadata passed. Build, link, runtime
and device behavior remain unverified.

## Direct3D SR SDK calls in Rust after `f43a03ae`

Question: can Rust build the Streamline records and call the SDK for both Direct3D SR routes, so
`dlss_native12.cpp` and `streamline_sr.h` can go and `dlss_streamline.cpp` shrinks to what only
the vendor C++ headers can answer? The SDK exports are plain C entry points (`SL_API extern "C"`)
whose C++ references are pointers at the ABI, and the only virtual member in the records, the
frame token's conversion operator, is never called. Nothing in the call path needs a C++ compiler
once the record layouts are known, so the answer is yes.

What moved. `streamline_sr/sdk.rs` builds `sl::Preferences`, `Constants`, `DLSSOptions`,
`DLSSOptimalSettings`, `Resource`, `ResourceTag`, `ViewportHandle`, `AdapterInfo` and
`FeatureRequirements` and types the entry points. The header, preferences, constants, resource and
tag records are the Vulkan bridge's `rsf_slvk_sdk_*` records, which describe the same 2.14.1
layouts; the D3D-only records are in `streamline_sr_records.h` with bindgen output in
`streamline_sr/records_abi.rs`. `dlss_streamline/sdk.rs` holds the D3D11 and shared-host tables
and `dlss_native12/sdk.rs` the D3D12 table, replacing the two C++ platform tables. They keep the
signatures the Rust owners already used, so the session, registration and fixture code did not
change. The private payload records (`rsf_sl_sr_*`) never crossed a language boundary after this,
so `streamline_sr_payload.h`, its bindgen output and Codex's unfinished additions to both were
removed in favour of plain Rust types in `streamline_sr.rs`. `dlss_native12/xm.rs` is the
scalar transcription of the SSE `XMMatrixInverse` the Fallout 4 adapter already pins, kept as a
copy so the D3D12 singularity gate keeps bit-identical inverse and determinant values.

Defaults. The D3D11 startup extends the `sl::Preferences` constructor flags
(`eDisableCLStateTracking | eAllowOTA | eLoadDownloadedPlugins`) with its own two; the D3D12
registration assigns its flags. The records keep the INVALID_FLOAT lens matrix, the 40.0 relative
depth separation, the false orthographic, dilated and jittered flags, and the `DLSSOptions`
defaults for exposure and indicator axes. Both D3D12 `slInit` and the D3D11 `slInit` keep their
log levels, and the D3D12 path still installs no message callback.

What stays native. `dlss_streamline.cpp` checks every record, GUID, enum and buffer-type value
Rust copies against the pinned headers with `static_assert`, and `rsf_dlss_sdk_compiled()`
constructs each SDK record once to confirm the struct versions and defaults Rust writes. The
SDK path stays unavailable when the headers are absent or the check fails, as `RSF_HAVE_STREAMLINE=0`
did before. The same file defines `rsf_dlss_verify_runtime_signature`, the vendor's
`sl::security::verifyEmbeddedSignature`, which D06's `mfg_unlock.h` lease adapter still calls, and
its `extern "C"` alias `rsf_dlss_sdk_verify_signature` for the Rust loaders. It stays native because
`sl_security.h` defines globals and pulls in WinTrust and cert parsing it owns. Both are C++ because `sl.h` and `sl_security.h` are C++ only;
there is no first-party logic left in the file.

Callers. `streamline_load.h` and `streamline_frame.h` are now C declarations. The Rust loader is
exported as `rsf_streamline_load_interposer` and `dlss_generation.cpp` calls it instead of the
removed inline C++ loader. `rsf_streamline_common_set` takes an opaque constants record and is
`extern "C"`; the D06 file changed only in that definition and its loader call. The unused
`rsf_streamline_camera_forward` export was removed with its inline C++ caller. No D05 file
includes `streamline_diagnostics.h` any more: the D3D11 route logs through the Rust sink. Its
remaining callers are `dlss_generation.cpp` (D06/D04) only, since D06 ported `mfg_unlock.cpp`
and `reflex_audit.cpp`.

Source evidence. The 48 orchestrator resolution errors at this tip (D06, D01 and other chunks) stop
`cargo check` before type checking, so the changed modules were compiled in a scratch crate that
includes the real `dlss_streamline`, `dlss_native12`, `streamline_sr`, `streamline_log`,
`streamline_policy`, `sdk_module`, `sr_session`, `render_abi` and `gl_pipeline_abi` sources with
stubbed `com_ref`, `input_dump`, `d3d11_d3d12`, `d3d11_present_abi` and `d3d12_sr`.
`cargo check`, `cargo clippy --lib --tests -D warnings` and 15 unit tests passed there
(record builders, tag extents and pointers, the DirectXMath inverse, the route fixtures). MSVC
`/W4 /WX /Zs` passed for `dlss_streamline.cpp`, `dlss_generation.cpp` and `mfg_unlock.cpp` with the
pinned 2.14.1 headers, and a linked probe of `dlss_streamline.cpp` returned
`rsf_dlss_sdk_compiled() == 1` and refused an unsigned module. No SDK call was made, no device was
created and no game was run, so slInit, tagging and evaluation against a real interposer remain
unverified.
