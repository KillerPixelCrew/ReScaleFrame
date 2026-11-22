# D3D12 Streamline generation and compatibility port

D06 uses the existing D3D12 Streamline host and C05 input receipt policy at
baseline `8adf010b`. No engine producer, shader contract or game acceptance
changed. The local Streamline contract is documented in the
[startup port](rust-d3d12-streamline-host-startup.md). D05 owns the shared SR
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
The complete compatibility bundle listed above still uses its pinned native
implementation. The D04 host and D05 provider ownership paths were preserved.

Verification used pinned Rust 1.94.1 formatting, binding regeneration, locked
offline Cargo metadata, source inspection and a whitespace check. Runtime
behavior and device acceptance remain unverified on this change.
