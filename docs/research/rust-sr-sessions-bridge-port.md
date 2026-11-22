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
`runtime/orchestrator/src/auxiliary_sr.h`,
`runtime/orchestrator/src/native_sr_completion.h` and
`runtime/orchestrator/src/sr_legacy_adapter.h` remain C++ interfaces used by
native callers. The following native paths remain in the implementation and
should stay open if the checklist requires removing their C++:
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
- **Invalidate the FG history when its own input validation faults.** Device and depth faults reset FG alone. The synthetic source fixture checks both history slots.
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
