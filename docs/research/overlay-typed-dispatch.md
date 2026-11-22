# Required overlay callback dispatch

Source commit `3f6fae42` on `NightHammer1000/overlay-typed-dispatch`,
from `af319530`. This covers the overlay portion of the PORT finding
about required platform callbacks. The other owners named in that finding
are outside this change.

| Paths | Disposition |
| --- | --- |
| `runtime/orchestrator/src/overlay_host.rs`, `runtime/orchestrator/src/overlay_host_native.rs`, `runtime/orchestrator/src/overlay_host_tests.rs` | The real and fixture D3D11 hosts construct a required-pointer table in Rust. The test-only raw panel loader still exercises DLL loader status and panel export admission. Texture, input and target restore calls no longer check a nullable callback on every use. |
| `runtime/orchestrator/src/overlay_native12.rs`, `runtime/orchestrator/src/overlay_native12/native.rs` | Native D3D12 still had nullable callbacks at the base. Production constructs the typed table directly. The old C entries keep their nullable record type and reject an incomplete table at entry. The atlas and retirement fixtures retain the leaves and lifetime checks they exercise. |
| `runtime/orchestrator/src/overlay_wrapped12.rs`, `runtime/orchestrator/src/overlay_wrapped12/native.rs` | The wrapped overlay uses the same typed dispatch pattern. Its old C entry signatures and fixture-injected wrapping and admission calls remain. |
| `runtime/orchestrator/src/overlay_service.rs` | Already direct at the base. Its optional adapter callbacks and the bound-status override are genuine choices. The file is unchanged. |

The source check compared signatures for all 26 D3D11 host slots including
the test-only loader, 19 native D3D12 slots and 10 wrapped slots. The same
check found no per-use required callback unwraps in the three owners.
The C headers, generated nullable records and legacy C symbol signatures
are unchanged. Logger, renderer readiness, frame leases, and admitted
panel lifetime retain their separate optional state.

Targeted bindgen 0.72.1 regeneration twice produced the unchanged private
overlay binding. Rust 1.94.1 formatting, locked offline Cargo metadata and
source diff checks passed. `PORT.md` and the implementation tracker were
left untouched. Builds, tests, lint and device execution were outside the
dispatch and remain unverified.
