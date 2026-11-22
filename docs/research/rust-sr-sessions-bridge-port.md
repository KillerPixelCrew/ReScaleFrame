# D03 SR sessions and bridging

Source review covers the Rust owners and their C calls as of D01
`d8a38a47` and D05 `6c01497a`. The D03 owner changes and native fixture
updates are separate commits. The worktree carries no PORT.md changes.

## Source-port coverage

The following PORT.md paths have corresponding source changes or an audited
C ABI retained around the migrated owner.

| PORT.md path | Source evidence |
| --- | --- |
| `runtime/backends/common/sr_helpers.h` | The frame wire validator now lives in Rust and is shared by direct DLSS, the SR session and the cross-API bridge. Win32 loaders and D3D12 resource checks stay at the native boundary. |
| `runtime/orchestrator/include/rescaleframe/sr_bridge.h` | Adds checked close with consume-on-success semantics and keeps the compatible void destroy export. |
| `runtime/orchestrator/src/auxiliary_sr_platform.h` | The generated Rust table imports the canonical D3D11 state type. Auxiliary clear and copy stay with the transaction and the guard drops before rollback releases the frame. |
| `runtime/orchestrator/src/sr_session_backends.cpp` | Wires the Rust session to its providers and Rust validation. The managed FidelityFX table preserves failure diagnostics and leaves the success announcement to session commit. |
| `tests/auxiliary_sr.cpp` | The device harness asserts that the retirement step restores viewport and primitive topology as well as restoring leased contents. |
| `tests/sr_backend_switch_device.cpp` | The device harness includes a direct FSR3 startup mode with a null Streamline directory and checks the selected backend and evaluations. Existing transition/timing modes remain available. Requires the D01 startup export. |
| `tests/sr_bridge.cpp` | The device harness asserts checked bridge close. Rust bridge fixtures cover a timed-out close with retained leases, retry and output-only reallocation. |
| `tests/sr_session.cpp` | The native ABI harness covers sharpening refusal and checks the committed version announcement across successful and refused switches. |

The public ABI declarations in `runtime/orchestrator/include/rescaleframe/d3d11_sr_api.h`,
`runtime/orchestrator/include/rescaleframe/native_sr.h` and
`runtime/orchestrator/include/rescaleframe/sr_session.h` remain in force.
At this baseline, `runtime/orchestrator/src/auxiliary_sr.h`,
`runtime/orchestrator/src/native_sr_completion.h` and
`runtime/orchestrator/src/sr_legacy_adapter.h` remained C++ interfaces used by
native callers. The following native paths remained in the implementation
(all since removed, see below):
`runtime/orchestrator/src/auxiliary_sr.cpp`,
`runtime/orchestrator/src/d3d11_sr_api.cpp`,
`runtime/orchestrator/src/native_sr.cpp`,
`runtime/orchestrator/src/sr_bridge_platform.cpp` and
`runtime/orchestrator/src/sr_legacy_adapter.cpp`. They carry D3D and shader
operations, native fault containment, service table construction or legacy
setup marshalling. All four device/ABI harnesses remain native fixtures.

## Rust review findings closed in source

- **Reuse the saved D3D11 state guard for auxiliary passes.** One generated state binding and the output owner's save/restore pair cover preparation, commit and rollback.
- **Reuse one fake IUnknown header for orchestrator ownership tests.** The four fixture families embed or reuse the same header, callable QueryInterface and checked refcount. The D3D12 fixture still records its last release.
- **Decode region-preparation failures once for SR and explicit FG.** The shared decoder publishes the three textures only for complete success. Explicit FG takes the failure path before calling its motion decoder.
- **Invalidate the FG history when its own input validation faults.** Device and depth faults reset FG alone. The synthetic source fixture checks both history slots. The guard retirement below later removed the fault sources themselves.
- **Expose the SR bridge's retained-on-timeout destruction result.** Close returns NOT_READY with the owner live and consumes it only on success. A fake-queue fixture checks the retained leases and a successful retry.
- **Preserve the supplied far-plane convention independently of depth direction.** The Rust adapter preserves `open.depth_infinite`. The C++ factory explicitly supplies the old derived convention for legacy pipeline callers whose setup has no independent field.
- **Share the destructor-backed owner for opaque graphics handles.** Bridge, OpenGL and native FG normalization share the same non-COM destructor owner. Each client retains its own GPU retirement ordering.
- **sr_bridge keeps one prepared shape as three parallel Options and re-checks them after prepare**. The prepared shape, interop surfaces and private output have one owner, returned directly on successful preparation.
- **Key prepared bridge textures by the dimensions used to allocate them.** Descriptions are checked in place. Allocations key both render and display dimensions, using fixed formats. The output-only resize fixture checks retirement before reallocation.
- **Orchestrator re-declares the struct_size prefix check in eleven modules beside the crate-visible has_capacity**. All eleven named copies use the shared pair. The gate still reads only the size field before accepting a full record.
- **Announce the selected FSR provider once per successful SR switch.** Managed opens retain SDK failure logs and announce the version after commit, while direct opens retain their success log and version getter.

## Integration and verification

D05's host startup and D01's `rsf_dlss_pipeline_start_backend` export are
the common mechanism. D01 wires Anomaly to that export and supplies
`rsf_d3d11_sr_admit` for directory-package and device-based admission. The
remaining N06 work is to offer Auto in Anomaly and call Runtime admission for
the default and effective provider using the adapter's directory and device
facts. The legacy pipeline setup still has no independent far-plane
convention. The shared provider's open descriptor can carry it.

Auxiliary bindings were regenerated with bindgen 0.72.1 against the existing
D3D11 state ABI. The changed Rust files passed the Rust 1.94.1 formatting
check and the patch passed the whitespace check. Builds, lint, test and
device execution remain user-owned.

## Residual source cutover from `84a8b9eb`

| PORT path | Source evidence |
| --- | --- |
| `runtime/orchestrator/src/d3d11_sr_api.cpp` | Removed. `d3d11_sr_api.rs` constructs the existing GameSDK table and preserves the size-before-version check, reset-on-evaluate wrapper, stop reset, and quality guard. `dll_exports.c` exports the query for Runtime DLL clients. |
| `runtime/orchestrator/src/sr_session_backends.cpp` | Removed. `sr_session.rs` binds the same managed FSR and XeSS getters and uses the shared frame validator directly. |
| `runtime/orchestrator/src/sr_legacy_adapter.cpp` | Removed. `sr_legacy.rs` builds the session setup and calls the existing bridge, motion resolve and capture services. The private C declaration in `sr_legacy_adapter.h` remains for the legacy pipeline caller. |
| `runtime/orchestrator/src/sr_bridge_platform.cpp` | Removed. `sr_bridge/platform.rs` implements the Windows platform and capture calls. `d3d11_d3d12.rs` shares command, copy, event and fence operations with the existing native FG path. The bridge owner still controls all waits, leases and consume-on-success retirement. |
| `runtime/orchestrator/src/auxiliary_sr.cpp` and `auxiliary_sr.h` | Removed. `auxiliary_sr/platform.rs` populates the existing generated table and uses the same shader, state helpers, resource flags and draw order. The C++ convenience wrapper is local to the remaining device fixture. |
| `runtime/orchestrator/src/native_sr.cpp` | Removed, with `native_sr_completion.h`. See [the D03 guard retirement](#d03-native-sr-guard-retirement). |
| `tests/sr_session.cpp` | Removed. The CTest entry invokes the Rust fixture with the same deterministic provider cases, including copied paths, replacement refusal, reset, stale generations and retirement counts. |
| `tests/sr_bridge.cpp` | Removed. Its Rust fixture keeps the explicit hardware arguments, backend/quality sequence, exposure switches, pixel readback and checked close. The C entry now links the embedded core. The separate D3D11 Runtime API fixture still exercises the DLL query. |

`sr_helpers.h` retains vendor-facing native helpers. `auxiliary_sr_platform.h`
still defines the private callback ABI shared with the D3D11 host.
The auxiliary and backend-switch device fixture bodies remain native.

The base contained two conflicting definitions of `ComRef::from_result`.
The shared implementation now uses the existing typed HRESULT and continues
to release non-null failed COM outputs. Both the bridge and FG reuse it.

Source checked against the existing C headers and the pinned Windows bindings.
Rust 1.94.1 formatting, targeted bindgen 0.72.1 regeneration and locked offline
Cargo metadata passed. The four regenerated binding files were unchanged.

## D03 device fixtures from `e5d73636`

- `tests/auxiliary_sr.cpp` was replaced by `tests/auxiliary_sr.rs` and
  `tests/auxiliary_sr/regions.rs`. The Rust body keeps the WARP device,
  reserve/seal/commit/restore sequence, frames 7, 81 and 82, packed-HDR
  capability and fallback, second-device refusals with unchanged bindings
  and output pointers, and the UNORM and D24 normalization and admission
  checks. The ordinary and packed auxiliary cases reuse the same transaction.
  The fixture now calls the Rust cancel operation directly, restoring the
  seam used by the original recovery assertions after the C++ wrapper lost it.
- `tests/sr_backend_switch_device.cpp` was replaced by
  `tests/sr_backend_switch_device.rs` and a C argument forwarder. The Rust
  body opens the Runtime DLL beside the fixture and calls its published
  pipeline exports. NVIDIA selection, FSR3 startup on any enumerated adapter
  with a null Streamline directory, optional manual exposure, 12 warm-up
  frames, and the FSR4/XeSS/FSR3/DLSS/FSR4 sequence are preserved. Baseline
  measures eight frames per successful switch. Select measures 12 and
  checks both frame timing bounds at 250 ms. The original CTest entries
  keep their vendor gates, timeouts and skip handling.

All 111 extracted auxiliary assertion and readback label occurrences match
the original source. Rust 1.94.1 formatting, targeted bindgen 0.72.1 with
unchanged generated bindings, locked offline Cargo metadata, and MSVC C11
syntax-only checks for both launchers passed. The fixtures remain unverified
by compilation or execution.

## D03 native composition and shared loader from `f4871e47`

- `native_sr.rs` calls Rust composition owners for region preparation,
  depth-domain validation, state save and restore, scalar exposure,
  translucency, mask composition, motion normalization and output reinsertion.
  The same Rust owner schedules the FG capture stages and publishes the
  existing `rsf_native_sr_capture_fg` C export. The COM identity checks and
  GPU cache owners still use the same device and stream.
- `native_sr.cpp` still runs as the exception boundary for pipeline resize,
  frame evaluation and FG capture. It also retains the public evaluation
  wrapper and the private ordered evaluation entry. Rust still writes
  `ordered = (result != leaf_fault)` before translating the result, and the
  C++ exception handler still writes zero and returns `RSF_ERROR_NOT_READY`.
  The D01 host still consumes its original receipt and owns the device-life
  and retirement checks.
- `sr_helpers.h` was removed. Its only remaining executable call was
  `load_module` under `streamline_load.h`. The shared Rust SDK loader now
  serves that caller and native Streamline SR with the same directory and
  System32 search flags and immediate Win32 error capture. The native
  interposer path keeps absolute-path admission, signature verification
  when requested, and existing log formatting. The other deleted inline
  helpers had no callers at this base.

Rust 1.94.1 formatting, bindgen 0.72.1 regeneration of the four relevant
bindings with unchanged output, locked offline metadata, and MSVC C++20
syntax-only checks passed. MSVC checked the native SR boundary and both
Streamline-header consumers with the SDK disabled, then checked the
consumers with the local SDK headers enabled. Build, link and runtime
behavior remain unverified.

<a id="d03-native-sr-guard-retirement"></a>

## D03 native SR guard retirement

`native_sr.cpp` held four `catch (...)` wrappers and the public
`rsf_native_sr_evaluate`. Each wrapped callee is a Rust export of this crate:
`rsf_dlss_pipeline_resize_output` and `rsf_dlss_pipeline_on_frame`
(`dlss_pipeline/exports.rs`), `rsf_native_fg_capture` (`native_fg.rs`) and
`rsf_core_native_sr_evaluate` (`native_sr.rs`). No first-party C++ runs
between the guard and the vendor SDK. The D3D11 FSR and XeSS backends are C
(`fsr_backend.c`, `xess_backend.c`), and the Streamline calls keep their own
leaf guards in `dlss_streamline.cpp`. A Rust panic cannot leave an
`extern "C"` Rust function, which aborts instead. A foreign exception crossing
the Rust frames would already be undefined behavior before the outer catch.
The same exports are called unguarded by `d3d11_sr_api.rs`, the D3D11 host
resize leaf and `loader/proxy/src/dlss_bridge.c`. No guard was therefore
converted to a C11 SEH leaf.

- `native_sr/platform.rs` calls the pipeline resize, frame and FG capture
  exports directly. `rsf_native_sr_evaluate` is a Rust `no_mangle` export, and
  `rust_exports.c` adds its `/export:` so the Runtime DLL, the AC7 carrier, the
  shared loader and the Minecraft DLL keep the name.
- The D01 host's `sr_evaluate` entry is now `rsf_native_sr_evaluate_ordered` in
  Rust. Every native write of the call is recorded on the immediate context
  before it returns, so it always reports an ordered receipt. Only the removed
  C++ handler ever reported zero. `native_sr_completion.h` was deleted,
  `d3d11_host_foreign.h` includes `native_sr.h` for the capture declaration,
  and the regenerated `d3d11_host_foreign_abi.rs` drops the old guarded entry.
- With no producer left, `LEAF_FAULT` and its branches were removed.
  Validation and region, mask and reinsertion helpers return `bool` or
  `Option`. A region refusal still returns `RESOURCE_FAILED` without touching
  either history. The FG fault reset that closed the "Invalidate the FG history
  when its own input validation faults" finding has no fault source left.
  `discard_history` still resets only the named owner, and the unit test checks
  both slots.
