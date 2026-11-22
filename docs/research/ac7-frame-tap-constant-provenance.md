# AC7 compatibility constant provenance

This source review uses migration base fb27ab2f, the AC7 executable and UE 4.18.3 revision
pinned in the [native ownership reference](../../.agents/skills/ue418-render-integration/references/ac7-native-ownership.md),
and the [A02 compatibility trace](ac7-render-scope-port-a02.md). No new executable analysis or
runtime capture was used.

The compatibility bridge gets a recent width-matched PS buffer from the tap, then compares upload
identity and extent against the retained scene candidate. A02 records the native renderer's
actual graph view and family independently. Those native callbacks require successful native
activation, so they cannot certify a compatibility draw when native activation refuses. Neither
the full-target rectangle nor pointer retention identifies the player camera. The compatibility
recombine gate can occur before the later role-signature report, so a current-frame proof cannot
be borrowed from that later observation.

## Observed write to actual draw

The frame tap now offers opt-in provenance for one immediate context, one explicitly chosen pixel
slot and one COM-retained constant buffer. The caller must have evidence for the slot, renderer
thread, write coverage and complete mapped contents. The tap supports default-usage buffers for
unboxed full-width Updates and dynamic buffers for paired WRITE_DISCARD Map/Unmap at subresource
zero. An UpdateSubresource1 with nonzero flags has no receipt. A separate Context1 interface
pointer is refused because the install patches the observed table only.

The existing watch runs with its existing data and mutation lease. The dedicated const observer
runs afterward on the forwarded bytes, or on the still-mapped memory before Unmap. Its upload ID
is tentative until the original call returns. The tap stores identities only. The observer must
copy its data during that callback. Present, accepted plan changes, vtable rewrites, ClearState,
uninstall and installing a different tracking configuration invalidate receipts. A missed refresh
lock or incomplete repatch also prevents relying on the old observation.

Right before an unmodified direct draw, after any before-draw research callback, the tap reads the
real PS slot with a COM getter. On Context1 it also reads the first constant and range, in
16-byte constants. A shifted or short range refuses. A target/input/candidate/research callback
after the native call can request that draw's ID. If any intervening event invalidates the
sample, the post-draw query reports zero. DrawAuto and indirect draws have no receipt. Existing
pass, target-draw, upload and geometry callback records retain their layout and ABI.

## Source replays

| Observed sequence | Result for the tracked buffer |
| --- | --- |
| Arm, full immediate default-buffer Update, upload observer, native forward, direct draw uses the exact PS slot and full range, post-draw query | Observer, draw and CURRENT queries can agree on one nonzero generation. The update may have passed through the existing writable scratch first. |
| Successful dynamic WRITE_DISCARD, matching Unmap, forward, same direct draw | The mapped address is only observable before Unmap. Same serial can commit after return. READ, partial/unsupported writes and unmatched Unmap fail closed. |
| Upload A, bind A, overwrite A by a second qualifying upload, draw | The draw matches only the second ID. A previously copied receipt from the same address no longer matches. |
| Upload, copy into the retained buffer (full, region, region1, structure count or resolve), or Context1 DiscardResource | Destination invalidation removes the old generation. Copies do not manufacture a new upload ID. CopyResource uses the actual forwarded destination after substitution. |
| ExecuteCommandList with either RestoreContextState value, or ClearState | The generation clears even when the native bindings are restored, since the tap does not have a deferred writer journal. A fresh observed upload is required. |
| Map, ClearState/rewrite/end-frame/plan replacement/uninstall, later Unmap | The tracked mapping is removed with its generation. Unmap cannot revive it. An interrupted write may produce no receipt on Windows runtimes that rewrite the table. |
| Current draw uses another slot/buffer, truncated Context1 range, override, divert, or runs off the registering thread | No draw ID. An old upload ID also cannot certify an unrelated draw or any native view. |

The observations cover only intercepted D3D11 calls. A cached native vtable method, another
unobserved writer, and a deferred execution bypass are outside the proof. The tracked resource
must have only the constant-buffer bind flag, excluding UAV or view writes. The caller still
owns evidence that the immediate writes were valid and that CPU Map contents were fully filled.
No texture sampling, buffer readback, borrowed-pointer caching or GPU work was added.

## A02 handoff

A compatibility native producer must first identify its session, native frame, view and family,
rectangle, scene pass, context, PS slot and actual buffer. Before the upload, arm that exact
buffer/slot with the dedicated const observer. During the observer copy the decoded camera with
its UPLOAD generation. In the corresponding post-draw callback accept it only when DRAW returns
that same ID and the adapter's native receipt still matches the draw. At recombine check CURRENT
against both IDs and the same adapter receipt before using the copy. A zero, mismatch, gate that
precedes the qualifying draw, or missing producer leaves the existing native-colour fallback
available. The compatibility bridge was not wired to this opt-in facility. The native family/view
producer and final camera PORT finding stay with A02.

Checks are source-only. The Context1 DiscardResource and PSGetConstantBuffers1 slots were
matched to 117 and 129 in the Windows SDK 10.0.26100.0 C vtable. The default PS getter is slot 77.
Targeted bindgen 0.72.1, pinned Rust 1.94.1 formatting, isolated Rust source analysis, locked
offline Cargo metadata, and focused C and C++ syntax checks cover the contracts. The sequences
above are source replays. No build, linker, test, lint, device execution or game acceptance.
