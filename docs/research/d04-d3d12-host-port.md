# D04 D3D12 SR and generation host port

Reviewed the D04 findings from `894a55f9` against integrated base `d539c0f6`.
D07 already dispatched FidelityFX and XeSS SR through Rust and moved the
FidelityFX queries into Rust. U02 supplied the shared SR planning path.
This work preserves those owners and their vendor receipts. It updates
shared host policy, provider tables and resource leases.

## Closed paths

- `runtime/backends/common/generation12_platform.h` contains C ABI declarations.
- `runtime/backends/common/sr12_platform.h` contains C ABI declarations. Rust
  sequences frame and device validation through registered callbacks.
- `runtime/orchestrator/include/rescaleframe/native_fg_d3d12.h` contains C ABI declarations.
- `runtime/orchestrator/include/rescaleframe/native_sr_d3d12.h` contains C ABI declarations.
- `runtime/orchestrator/src/fg12_platform.h` contains C ABI declarations.
- `tests/fg12_abi.cpp` moved to `tests/fg12_abi.c` with both CTest registrations.
- `tests/sr12_abi.cpp` moved to `tests/sr12_abi.c` with both CTest registrations.

## Rust findings

- **Preserve wait failure, timeout and device status as distinct D3D12 results.** Fence-arm HRESULTs pass through. Timeout returns HRESULT_FROM_WIN32(WAIT_TIMEOUT), WAIT_FAILED returns the captured Win32 HRESULT, and the fence sentinel still reports device removal.
- **Expose unsuccessful provider teardown to the presentation owner.** The appended, size-checked callback consumes the handle on success. The presenter unpublishes and retries refused sessions before replacement, retains refused probe and create handles, and keeps shared inputs alive on final refusal. A synthetic fixture covers the retry receipts.
- **Deduplicate XeLL input markers only after the SDK accepts them.** XeLL keeps marker delivery serialized and records a sample only after SDK success.
- **Apply the same FG12 option validation at install and live update.** One FG12 validator checks size, mode, count and Reflex mode at both entry points. The C ABI fixture covers install refusal.
- **Give failed FG12 installation a rollback boundary for shared settings and callbacks.** The presentation bridge retains process-lifetime trampolines. The runtime publishes shared options, graphics selection, the logger and window filter after success. The installing thread can emit synchronous receipts through its borrowed logger.
- **Use the pacing owner's eligibility check before aborting older FG tokens.** FG12 uses the shared source ledger to retire ended, unsubmitted sources with no render-start attempt or in-flight pacing. It marks a row failed only after Streamline accepts the abort. Present completion releases the exact source token.
- **Associate failed FG capability queries with the current Present.** A failed or missing capability query publishes the current source and result, clears active vendor state and requests FG off. Retirement respects that result.
- **Derive Reflex frame-limit intervals consistently across the native FG paths.** Both hosts use native_fg::provider_options. Eligible fixed Reflex FG receives the rendered-frame interval divided by the admitted number of presents.
- **Count interpolated preparation consistently in native FG status.** FG12 counts successful preparation only when interpolation was requested.
- **Keep inferred native-frame provenance attached to every SR12 capture operation.** One flagged frame record reaches capture begin, readbacks, result and handoff. FG12 rejects ambiguous inputs. The synthetic fixture checks capture receipts.
- **Bring the D3D12 SR test platform table back in line with the owner.** The fake table creates the texture and upload separately and checks the upload size and value.
- **Use one coexistence check when opening D3D12 generation providers.** Both initializers use one coexistence preflight. The restart requirement precedes unsupported provider options.
- **Share ownership of resolved D3D12 provider bindings.** D07 superseded the duplication. SR binds typed exports through the shared SdkModule and drops its FSR compatibility lease before the module. FG owns an opaque dispatch binding which closes before the primary and optional XeLL modules. The old SR opaque binding and unbind path are gone.
- **Share the bounded FidelityFX provider enumeration used for SR and FG.** Both paths use one bounded, typed DX12 version query and one nonzero version admission rule. SR keeps its candidate logging.
- **Retire failed diagnostic readbacks when their device generation is drained.** A removed-device readback is discarded after the presentation generation drains. Pending work still waits for completion.
- **Reuse the validated external exposure view during normalization.** SR12 stores the validated view format with the retained COM reference and uses R32_FLOAT when exposure is absent.

## Native ownership follow-up at 84a8b9eb

The follow-up moves the five remaining paths to Rust owners with native ABI and
SDK adapters. The base implementation and exact boundary are recorded here.

| PORT path | Source evidence |
| --- | --- |
| `d3d12_helpers.h` | `d3d12_host.rs` owns barriers, copies, allocator/list creation, and UAV creation. A copy transitions source then destination before copying, and restores destination then source. Command creation returns both owned handles only after a successful close. The native fence entry calls `d3d12_fence.rs` with the same requested value, event, budget, and raw arm HRESULT. |
| `fg_helpers.h` | `d3d12_generation/admission.rs` owns direct-command and device checks and the frame and resource validation shared with Streamline. Validation retains result precedence, including the disabled-interpolation exit, source provenance, the exact FOV cutoff, optional HUD-less/UI resources and per-resource stale-generation checks. The adapter supplies the canonical game-frame predicates. Native setup-header checks, DXGI descriptor marshalling and the SDK-off provider table remain available to standalone backends. |
| `generation12_checks.h` | The existing five-slot table forwards setup, frame, command and module-presence checks to Rust after the setup header is admitted. The runtime DLL loader retains its absolute path check, bounded Windows paths, load flags and log callback. FSR and XeSS still use their existing D07 setup/probe call order and SDK gates. |
| `fg12_platform.cpp` | `d3d12_host/fg.rs` owns the swapchain query, texture reuse/allocation and fence work. Texture reuse keeps the original shape fields, exact device pointer and retained output lease. The native public entry points, presentation bridge and G07 capture receipts remain in place. |
| `sr12_platform.cpp` | `d3d12_host/sr.rs` owns adapter discovery, normalization pipeline and resource creation and device admission. `d3d12_host/normalize.hlsl` contains the same kernel bytes. The pipeline uses four SRVs, four UAVs, six constants and the same System32 compiler, target and flags. Native borrowed command recording and mapping, provider callbacks, G05 sharpening and G07 captures still use their existing table entries. |

The follow-up was checked with Rust 1.94.1 formatting, bindgen 0.72.1 for
the affected ABI inputs, and locked offline Windows Cargo metadata. The generated
files matched the base. Build, test, lint and device results are pending.

## Residual platform bindings at 0251a81a

The C02 cutover at **e993c6ad** already called the Rust hosts for render-host
and OpenGL dispatch. D07 at **48fac36b** already owned the SDK-gated FidelityFX
and XeSS provider tables. This pass follows those entry paths and keeps the
export declarations for the Runtime DLL and embedded carrier.

| D04 source | Evidence and retained boundary |
| --- | --- |
| runtime/backends/common/d3d12_helpers.h | The typed wrappers still call d3d12_host.rs for barriers, copies, command lists and UAVs. The fence declaration now matches the Rust i32 ABI. The Win32 adapter returns the same bits as HRESULT, with arm failure, timeout, wait failure and device removal still resolved by d3d12_fence.rs. |
| runtime/backends/common/fg_helpers.h | d3d12_generation/admission.rs clears the output slots and checks both structure sizes before the two ABI versions. d3d12_generation.rs supplies the same single-sample flip-discard DXGI descriptor. disabled.rs supplies the existing NOT_COMPILED callbacks and null checked teardown. Streamline still calls the canonical SDK frame predicates through the thin native adapter. |
| runtime/backends/common/generation12_checks.h | The Rust admission module publishes the shared check table. The C++ adapter supplies the canonical frame predicate entry and the existing bounded Windows loader, preserving the absolute path check, search flags and load-error log. fsr_generation.cpp and xess_generation.cpp still gate the enabled table on their existing compile definitions and pass null in the SDK-off paths. |
| runtime/orchestrator/src/fg12_platform.cpp | fg12/native.rs binds the existing presentation and latency owners and the Rust DXGI, fence and readback leaves. The native bridge catches presentation install exceptions and retains the process-lifetime capture object. Its record, ready-only flush and submitted calls keep their original slot and exact frame arguments. The CPU service still routes to the same FG event and latency ping callbacks. |
| runtime/orchestrator/src/sr12_platform.cpp | sr12/native.rs binds C02 creation and the native exports to the same Rust table. It records normalization with one descriptor heap, the same root slots, six constants and the same dispatch order. Region copy, unit exposure and motion readback preserve subresource zero, the original extents and 256-byte row pitch. The capture façade retains its existing allocation and teardown boundary. G05 sharpening, G07 frame receipts and the D05/D06 Streamline leaves retain their owners. |

This change uses the existing SR shader bytes, SR/FG input producer and
resource contracts, capture request and handoff points, and overlay routing.
It adds no new producers or render passes. The source comparison covers the
baseline and new callback slots, export definitions, SDK gates and command
parameters. Rust 1.94.1 formatting, bindgen 0.72.1 regeneration of the FG12 ABI
and locked offline Windows Cargo metadata were checked. Link, test and device
verification are pending.

## Typed provider admission and loader at fb27ab2f

This pass starts from the D04/G07 capture cutover c7265405 and the shared
GameSDK policy at 935757d5, both present at fb27ab2f. It applies a fresh
source change with no cherry-picks.

| D04 checklist path | Source evidence |
| --- | --- |
| runtime/backends/common/generation12_checks.h | Exposes a C declaration for the five-slot Rust table in d3d12_generation/admission.rs. The FSR provider and FSR4 capability probe in runtime/backends/fsr/src/fsr_generation.cpp and the XeSS provider in runtime/backends/xess/src/xess_generation.cpp call it solely under their existing SDK gates. The disabled calls still pass null and FSR retains its capture platform pointer. |
| runtime/backends/common/fg_helpers.h | The D06 Streamline descriptor, setup, command and SDK-off adapters call the existing Rust entry points. Frame admission uses the same GameSDK predicates directly from Rust, with the established invalid, stale and not-ready precedence. The C++ SDK descriptor and other vendor leaves stay in their existing owners. |
| runtime/backends/common/d3d12_helpers.h | Removes the unused C++ forwards for barriers, copies, lists and UAVs. Streamline and the Vulkan FidelityFX bridge retain the typed fence adapter with the same 10-second default, raw fence value and HRESULT. |
| runtime/orchestrator/src/fg12_platform.cpp | Includes the presentation ABI directly. The explicit versus implicit install call remains inside its C++ exception boundary. The original module attach and detach hooks still enclose the Rust capture owner. FG12 binding is now a Rust-only function. |

sdk_module.rs shares the D3D12 drive/UNC path and Windows loader with D07
super resolution. Provider file names retain the 1,024 UTF-16 directory
and 1,200 UTF-16 full-path limits from generation12_checks.h's legacy
loader chain. LoadLibraryExW keeps the provider directory and System32
search flags. FG retains its callback, the 511-byte diagnostic limit and
the Windows error number. Non-ASCII paths in these diagnostics are now
emitted as UTF-8, truncated at a complete character. Path refusal remains
silent. No change was made to module adoption, partial binding cleanup,
primary/latency module ordering or SDK teardown.

MSVC 18 x64 /Zs /W4 /WX parsed the FG12 boundary and the FSR, XeSS and
Streamline FG translation units with their SDK gates enabled and disabled.
Rust 1.94.1 formatting, byte-for-byte bindgen 0.72.1 reproduction of the
FG12 and XeLL bindings, and locked offline Windows Cargo metadata with all
features passed. Link and runtime checks remain pending.

## Initial port verification

- Pinned Rust 1.94.1 rustfmt checked the changed Rust sources. Bindgen CLI
  0.72.1 regenerated the two changed Runtime tables. Unrelated generator
  changes were dropped.
- Clang parsed the two C11 fixtures and five headers for Windows x64.
  Its C++ parser could not use the installed MSVC STL, which requires
  Clang 22. MSVC 18 `cl /Zs /W4 /WX` passed the fixtures, both native
  host leaves, the FG session fixture, and the enabled FidelityFX, XeSS
  and Streamline sources.
- Build, test execution, lint and device verification belong to the user.
  Source checks do not establish GPU ownership behavior on a device.
