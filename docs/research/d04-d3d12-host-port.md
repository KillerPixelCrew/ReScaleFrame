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

## Native paths still open

| Path | What remains |
| --- | --- |
| `runtime/backends/common/d3d12_helpers.h` | Shared COM adapter calls for barriers, copies, fences, allocator and list creation, and UAV creation. The presentation bridge, frame capture, sharpening and vendor leaves share this header. |
| `runtime/backends/common/fg_helpers.h` | Setup and resource validation, DXGI descriptor construction and stub callbacks for Streamline and the generation checks. |
| `runtime/backends/common/generation12_checks.h` | The FSR and XeSS factories bind shared C++ validation, Win32 module detection and runtime loading callbacks here. |
| `runtime/orchestrator/src/fg12_platform.cpp` | COM calls, bridge installation, FrameCapture12 and exported C ABI stubs. |
| `runtime/orchestrator/src/sr12_platform.cpp` | COM and D3DCompiler setup, root signature and descriptor recording, FrameCapture12 and backend callbacks. D05 owns the direct DLSS callback migration. |

## Verification

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
