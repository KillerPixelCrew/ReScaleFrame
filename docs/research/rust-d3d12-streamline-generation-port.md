# D3D12 Streamline generation and compatibility port

D06 uses the existing D3D12 Streamline host and C05 input receipt policy at
baseline `8adf010b`. No engine producer, shader contract or game acceptance
changed. The local Streamline contract is documented in the
startup port. D05 owns the shared SR
camera and loader helpers. This change preserves their token and constants
entry points and the public Streamline host ABI 2.

## Closed Rust review findings

| Finding | Source evidence |
| --- | --- |
| Settle the token when the combined Streamline begin operation fails to sleep. | `streamline_host.rs` shares combined begin between the host and generation provider. A failed sleep retires the acquired slot and preserves consumed source and SDK index ordering. Split acquire/sleep retains its existing explicit abort contract. The fake SDK regression wraps the ring onto the failed slot. |
| Deduplicate the controller sample across both Streamline marker entry points. | `streamline_host.rs` sends PCL marker 13 through one locked emit-once operation for explicit markers and C05 receipts. SDK refusal leaves it retryable, and only receipts count physical input. Rust doubles and the device fixture exercise both call orders. |
| Share revision and source ordering across D3D12 generation providers. | `d3d12_generation.rs` shares prepare admission, successful source consumption, revision invalidation, history continuity and activity gating with `streamline_host/generation.rs`. Existing FSR/XeSS and DLSS-G interleaving fixtures use this shared state through their respective providers. Counters and borrowed SDK fences are still provider-owned. |
| Keep Reflex and DLSS-G state coherent after a partial configuration failure. | `streamline_host/generation.rs` publishes accepted effective Reflex mode and checks mode and interval from the host before the equality shortcut. The regression rejects new DLSS-G options after successful Reflex configuration and reapplies the previously accepted request. |
| Represent an empty Streamline token slot once. | `streamline_host.rs` stores six optional tokens with required nonnull handles. Explicit abort, complete required markers and ordered Present retirement all take the slot. |
| Keep one flag for a successfully prepared DLSS-G history. | `streamline_host/generation.rs` uses the shared optional last interpolated source for reset and activity decisions. The successfully consumed source watermark remains separate. |
| Borrow the boxed host for non-owning FFI tests. | `streamline_host/tests.rs` passes mutable borrows in the ring retirement, PCL profile and generation reload tests. |

## Native inventory at the initial port

The paths below are relative to `runtime/backends/dlss/`.

| Paths | Ownership |
| --- | --- |
| `include/rescaleframe/streamline_host.h`, `src/dlss_generation.cpp` | Public DLL ABI and typed Streamline, COM and Win32 calls remain native. Rust owns the generation session, frame state and the common Present hook pair. An incomplete hook lookup remains retryable. The hooks stay borrowed from the live host module, and their calls take place outside the cache mutex. |
| `src/mfg_unlock.cpp`, `src/mfg_unlock.h` | Rust builds the wrapper path, owns its loader reference and verifies the resolved GetState address belongs to that wrapper. Rust adopts the unique implementation-module reference for the patch callback. Missing or mismatched ownership records refusal. Signature checks, PE inspection and instruction writes remain native. |
| `src/reflex_audit.cpp`, `src/reflex_audit.h`, `src/streamline_diagnostics.h` | Rust borrows the NVAPI query export from the already loaded driver. Audit installation state, listener lifetime and log filtering are already Rust-owned. The native leaves retain the typed NVAPI detours, MinHook calls, module pinning and exception guards around caller log callbacks. |
| `third_party/mfg_unlocker/dlssg_provider_policy.cpp`, `third_party/mfg_unlocker/dlssg_provider_policy.h`, `third_party/mfg_unlocker/midpoint_fix.cpp`, `third_party/mfg_unlocker/midpoint_fix.h`, `third_party/mfg_unlocker/ngx_mfg_gate.h`, `third_party/mfg_unlocker/protected_pointer.h`, `third_party/mfg_unlocker/universal_wrapper_profile.h` | The pinned MIT compatibility bundle and its exact verification gates are retained as native dependencies. See the [bundle provenance](../../runtime/backends/dlss/third_party/mfg_unlocker/README.md). |

## Follow-up from `821642ae`

| Paths | Source evidence |
| --- | --- |
| `dlss_generation.cpp`, `streamline_generation_records.h`, `streamline_generation_records.rs` | Rust assembles the DLSS-G, Reflex and PCL options, common FG camera fields and resource tag plan. The typed SDK leaves use the checked Streamline 2.14.1 value records also used by Vulkan. The camera keeps the SDK inverse routine, finite check and shared SR constants gate. The tag leaves still lend the resources through Present, with the original tag order and extents. Rust chooses the latest Reflex report and formats both summaries and the 64-row ring. |
| `reflex_audit.cpp`, `reflex_audit.rs`, `reflex_nvapi_abi.rs` | Bindgen 0.72.1 produced the five version-1 NVAPI records from the local NVIDIA MIT header `nvapi.h` dated 2026-09-02, SHA-256 `4ee1bff7568ceec72124b8b3e43bf65ccb69dbb878b08c634b128f7edcd28f3b`. Rust performs the borrowed sleep-status query, checks each detour record version, deduplicates and emits reports. The native detours still get the actual caller PC, call their typed original once and return its result. MinHook operations, pinning and the native log exception guard stay on the typed boundary. The SDK declaration sizes and key offsets have native assertions. Binding generation uses the header as C with SAL declarations, so unrelated C++ SDK declarations are excluded. |
| `mfg_unlock.cpp`, `mfg_state.rs` | Rust owns the signature-gated wrapper load, bounded executable-section enumeration and unique pinned-profile match. The scan bounds come from the retained module and PE image size. A failed section read refuses the scan. Rust enumerates and retains exactly one matching NGX implementation through the existing bundle predicate. The typed compatibility leaves retain the expected-byte publication, page protection restore, SDK provider classification and bundle patch operations. The existing serialized startup/reload and shutdown order continues to govern the calls. |
| `tests/generation/src/latency.rs`, `tests/generation/latency_abi.h`, `tests/CMakeLists.txt` | The Rust device fixture replaces `tests/latency_host.cpp` and links the same Runtime DLL entry points. It retains the 256x256 hidden chain, Space sample, real command submission and Present, two input receipt routes, 12-frame sleep counts, 96 markers and source reset refusal while a token is live. CTest keeps the same target, timeout, and skip code. |

The public `streamline_host.h` ABI 2 stays in C and the remaining
`dlss_generation.cpp` leaves own SDK startup, COM identity and validation,
typed SDK calls, output marshaling and public export glue.
`mfg_unlock.h` retains the C++ lease used by the SDK host and
`streamline_diagnostics.h` retains the exception boundary shared with D05.
The D04 host and D05 provider ownership paths were preserved.

Verification used pinned Rust 1.94.1 formatting, binding regeneration, locked
offline Cargo metadata, source inspection and a whitespace check. Runtime
behavior and device acceptance remain unverified on this change.

## Residual policy replay on `9716068b`

| PORT path | Source disposition |
| --- | --- |
| `dlss_generation.cpp` | Partial. Rust formats the provider version ID and display string. The native leaf reads the typed `sl::FeatureVersion`, copies its six version fields, and retains Streamline class construction, COM ownership, SDK calls and the disabled exports. |
| `mfg_unlock.cpp`, `mfg_unlock.h` | Partial and retained, respectively. Rust recognizes the wrapper profile and identifies candidate providers. The native class still owns the compatibility handle at the SDK host boundary and lends its signature check, Ada device check, expected-byte publication and temporal/provider mutation calls. |
| `reflex_audit.cpp`, `reflex_audit.h`, `streamline_diagnostics.h` | Retained. The native files supply SDK-typed detours and capture their real caller PCs, use MinHook, and guard log callbacks that may throw. Rust already owns the diagnostic policy and the NVAPI records. |
| `dlssg_provider_policy.cpp` | Retired. `mfg_provider.rs` checks both exports, reads the embedded Windows version resource, maps all 16 eligible versions to the same native temporal profiles, and owns the 64 process-lifetime module slots. Every cached lookup still checks export identity. A duplicate or full table releases the temporary reference. The native patch caller holds its own provider reference for the operation. |
| `dlssg_provider_policy.h` | Reduced to the native profile enum and internal C ABI declarations and adapters. |
| `universal_wrapper_profile.h` | Retired. `mfg_state.rs` checks the same 10 bytes, accepts the same compiled maxima and both exact branch tails, and still refuses ambiguous scans and foreign NOP publication. |
| `midpoint_fix.cpp`, `midpoint_fix.h` | Partial and retained, respectively. Rust selects the profile. Native code keeps all six payload profiles, hashes, adapter checks, descriptor checks and protection-sensitive mutation. |
| `ngx_mfg_gate.h`, `protected_pointer.h` | Retained unchanged. Their scans, instruction and pointer writes, cache flush, ownership checks and protection recovery remain together at the native mutation boundary. |
| `streamline_host.h`, `tests/latency_host.cpp` | Existing dispositions hold. Header remains the public ABI 2. The fixture already routes through the Rust generation crate. |

Source checks compare the wrapper publication leaf, gate and protected publication
helpers, SDK-disable exports and native ABI class header to the base. The Rust
provider profile mapping replaces the former C++ presence assertion by deriving
eligibility from the same selection used at the native boundary. The six native
profile records and their SDK payload checks retain their original definitions.

## Native receipt forwarding on `3066e6ad`

| Open PORT path | Source disposition |
| --- | --- |
| `dlss_generation.cpp` | Partial. Rust formats the two feature support codes and the accepted Reflex options receipt. The native leaf keeps typed SDK queries, result mapping, COM and Streamline class ownership, and the driver status query following accepted options. |
| `mfg_unlock.cpp` | Partial. Rust reads the retained provider's path, checks eligibility, and formats wrapper and provider receipts after publishing status and releasing the update lock. The native leaf lends the original signature and adapter checks, applies the exact wrapper write, and returns the temporal result, gate result and failure code. Ineligible providers supply their native failure code without a patch call. Path diagnostics now use UTF-8 with the existing 511-byte payload limit. |
| `mfg_unlock.h` | Retained. The noncopyable C++ lease continues to pair the opaque Rust owner with the native Streamline host lifetime. |
| `reflex_audit.cpp` | Partial. The Rust audit resolves the four existing NVAPI interface IDs in their original order from the already loaded driver, alongside the existing sleep status query. Native code associates those addresses with the typed detours, captures real caller addresses, invokes the original trampolines exactly once, and supplies the existing MinHook operations and module pin. |
| `reflex_audit.h` | Retained. It declares the optional native audit and its driver report path, including their SDK-disabled definitions. |
| `streamline_diagnostics.h` | Retained. D3D12 generation and MFG receipts use its native C++ exception guard to invoke caller log functions from Rust. Legacy D3D11 Streamline also uses the header. |
| `midpoint_fix.cpp` | Retained. Its native payload decompression, SHA-256 and descriptor checks, SDK adapter and module validation, and publication form one checked operation. Rust still supplies the same 16 version receipts and six immutable profile selections. |
| `midpoint_fix.h` | Retained. It declares the native adapter check, mutation and result interface. |
| `ngx_mfg_gate.h` | Retained. The candidate scan, expected bytes, adapter/provider admission and publication ledger are unchanged. |
| `protected_pointer.h` | Retained. The exact CAS, page-protection verification and retry, flush, rollback and indeterminate-publication checks are unchanged. |

The modified native calls stay in D06. No V04 source, shared module declarations,
or binding generators changed. MSVC `/Zs /W4 /WX` checked the three modified native
translation units with Streamline and NVAPI both enabled and both disabled.
It also checked the native midpoint translation unit. Source comparison covered
the pinned provider mapping, wrapper publication body, native detour bodies,
and C++ disable gates. Pinned formatting, targeted binding regeneration and
locked offline metadata passed. Build, device and runtime checks remain pending.

## Native closure

| PORT path | Source disposition |
| --- | --- |
| `mfg_unlock.cpp` | Deleted. `mfg_state.rs` calls the Rust adapter check, wrapper publication and provider preparation directly instead of the native platform table. The wrapper write keeps the exact `0f 42 d1` expectation, refuses a foreign NOP, and restores and rechecks page protection. Streamline's embedded-signature check stays in `dlss_streamline.cpp` beside `sl_security.h`, and the host lends it as a C function pointer only when a signature is required. |
| `mfg_unlock.h` | Retained. It now declares the Rust owner in C. The inline `rsf::MfgUnlock` adapter remains only because the D04 file `dlss_generation.cpp` still holds that class. |
| `midpoint_fix.cpp`, `midpoint_fix.h` | Deleted. `mfg_midpoint.rs` keeps the six payload profiles, both kernel tables and their slot assertions, all thirteen failure codes, the CUDA LUID and SM 8.9 admission, the unique registration-table scan, source fatbin and PTX SHA-256, the LZ decoder, the 104-multiply transform, the output SHA-256 and the protected descriptor publication. SEH-guarded copies become `ReadProcessMemory`, and the same-value write that privatizes a copy-on-write page uses `WriteProcessMemory`. The read-only sections are copied once for the table scan. The bundle's log callback was never installed by ReScaleFrame, so its unreachable messages and unread readiness flag are dropped. |
| `ngx_mfg_gate.h`, `protected_pointer.h` | Deleted. `mfg_gate.rs` keeps the 13-byte pattern, ambiguity refusal, count-one target and unwind-range checks, 64-slot publication ledger and foreign-edit refusal. The two-byte publication keeps every protection check, revalidation, flush and rollback step. Only a certified exchange by this call counts as published. The unused pointer publisher's semantics are kept in `mfg_midpoint.rs`. |
| `dlssg_provider_policy.h` | Deleted. Its C declarations became internal Rust calls in `mfg_provider.rs`. |
| `reflex_audit.cpp` | Deleted. `reflex_audit.rs` exports the three audit entry points and calls MinHook directly, like the other orchestrator hooks. Each naked detour entry passes the real caller PC from `[rsp]` in r8 and its kind in r9, then calls the original trampoline once. The NVAPI record versions and key offsets have Rust compile-time assertions. NVAPI headers are no longer needed for the native build, so the audit is present whenever the driver exports the interfaces. |
| `reflex_audit.h` | C declarations only. |
| `streamline_diagnostics.h` | Retained. No D06 file includes it now. `dlss_generation.cpp` (D04) and `dlss_streamline.cpp` (D05) still use its C++ exception guard. The MFG and audit logs now call the host log directly, as `d3d12_generation.rs` and `d3d11_host.rs` already do. |

Verification on `f43a03ae`: the integration tip does not compile `rsf-orchestrator`
(48 errors in other modules) or configure CMake (`tests/CMakeLists.txt:448`). The D06
modules add no crate diagnostics against that baseline. A scratch crate that includes
the real `mfg_state.rs`, `mfg_*.rs` and `reflex_audit.rs` passed clippy with
`-D warnings` and seven tests. Two opt-in tests mapped a local 310.9.1
`nvngx_dlssg.dll` without running its code. They found one registration table and one
unpatched gate, and the Rust transform reproduced the pinned output SHA-256 `579ACF33...`.
MSVC `/Zs /W4 /WX` passed for the two headers as C11 and for `dlss_generation.cpp` with
and without Streamline. Linking, CTest and device runs remain pending.

## D3D12 Streamline host native leaf

`dlss_generation.cpp` is deleted. `streamline_host/native.rs` now implements the 25-entry
startup table, the 24-entry platform table, the DLSS-G provider, the 22 public
`rsf_streamline_host_*` entries and `rsf_streamline_common_set`. The Rust owner in
`streamline_host.rs` is unchanged and still calls every leaf under its own locks.

| Former native piece | Rust replacement |
| --- | --- |
| `sl::` records | `streamline_sdk.rs` loads the shared Vulkan record mirrors once and adds `ViewportHandle`, `Version`, `FeatureVersion`, `AdapterInfo`, `DLSSGState` v4, `ReflexReport`, `ReflexReport2`, `ReflexState` v2 and `PCLState`. Each constructor writes the SDK GUID and structure version, including the 128 nested Reflex report headers. Compile-time assertions repeat the MSVC sizes and offsets. A scratch `cl /Zs` check against the pinned 2.14.1 headers confirmed every size, offset, GUID and the enum values used. `vulkan_streamline_camera.rs` and `streamline_generation_records.rs` now take their records from this module, which also removes clippy's duplicate-module error. |
| `PFun_sl*` | Typed `extern "C"` pointers in `streamline_sdk::api`. Streamline exports these with C linkage, and the `sl::` references are pointers on Windows x64. |
| `sl::matrixFullInvert` | `streamline_sdk::full_invert`, the same cofactor expansion. A singular input keeps the unscaled cofactors. |
| `ComPtr` | `Option<ID3D12Device>`, `ID3D12CommandQueue` and `IDXGIFactory2` from the `windows` crate. Upgrade, retain-native and alias keep the original reference transfers. The queue-device check compares interface pointers, as before. |
| `rsf::MfgUnlock` | A raw `rsf_mfg_state` from `rsf_core_mfg_create`, with direct `rsf_core_mfg_*` calls. Prime passes `rsf_dlss_sdk_verify_signature` only when a signature is required. |
| `rsf::emit_streamline_log`, `rsf_logf` | The host log called directly, as in the D06 MFG and audit code. |
| `sl::DLSSGOptions{}` for off | `rsf_slhost_pack_options(off, 1, ...)` with `flags` cleared, which matches the SDK member defaults. |
| `RSF_HAVE_STREAMLINE` stubs | Removed. The Rust mirrors need no SDK header at build time, which matches the D05 decision that retired `rsf_dlss_sdk_compiled()`. `rsf_dlss` becomes an INTERFACE target for the public headers and the Rust archive. |
| `__declspec(dllexport)` | `rust_exports.c` exports the 22 host entries from every binary that links `rsf_orchestrator_core`. That includes the Runtime DLL, Godot and the Unity package. |

`mfg_unlock.h`, `reflex_audit.h` and `streamline_diagnostics.h` lost their last includer and
are deleted. The interposer load and the MFG prime use the Rust `rsf_streamline_load_interposer`
and `rsf_dlss_sdk_verify_signature` from `streamline_sr`.

Verification on `3e9e0c7d`: the tip does not compile `rsf-orchestrator` or configure CMake
(`tests/CMakeLists.txt:451`). With this change, `cargo check` reports the same 150 error
lines as the tip, `cargo clippy --all-targets -D warnings` reports none in the touched files,
and `cargo fmt --check` passes. A scratch crate including `streamline_sdk.rs` passed clippy
with `-D warnings` and both of its tests. Bindgen 0.72.1 output is unchanged. MSVC `/Zs /W4
/WX` passed `rust_exports.c`, and `streamline_frame.h` and `streamline_load.h` as C11. Linking, CTest and device runs remain
pending.
