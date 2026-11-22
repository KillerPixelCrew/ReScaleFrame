# AC7 render scopes, views and scene effects source port

## Queued ownership

The existing [scope research](rust-ac7-render-scopes.md) documents the AC7 RHI list layout and
iterator. The A02 changes use the same tail slot and command count and leave the native iterator
prefix and callbacks unchanged. Uniform retirement, graph SR submission and velocity finalization
have no engine recording between their markers. They now use one enqueue call, which checks
capacity for both commands before publishing their chain. On refusal the caller still owns its
lease. Successful end execution releases the lease before decrementing the owner's outstanding
count.

Scopes around engine calls still open and close separately. Close checks the recording-list
pointer before touching the list or marking the ticket closed. The pointer identifies where the
markers are recorded, while the execute callback receives the engine's execution-list pointer.
This accepts a flush segment on the same recording list, as exercised by the existing Rust
fixture. A different list cannot close the ticket. A list discarded by the engine or never
available again can still keep an open scope and its owner pinned.
The full-frame hook keeps its ticket on refusal and deactivates rendering. The still-installed
EndFrame hook can close it when the originating list becomes recordable again.

## View and effects

The reader now calls the same numeric rectangle result `fills_target`. The C struct keeps the
same size, field offset and ABI version because the stored fact is unchanged. The native SR
graph already verifies that its context's view is the view from the active post-process hook,
reads that view's family and frame, and applies its primary-view admission before queueing.
In the compatibility bridge the qualifying scene pass identifies a constant buffer, whose
reference is held until replacement or shutdown. Recombine accepts an upload to that buffer
with the same view extent. This excludes ordinary uploads to other full-target view buffers.
The frame tap supplies a recently bound pixel-stage buffer and has no native view or family
identity. Slot selection and same-allocation reuse at the same extent therefore remain
limitations of the compatibility path. The native path does not consume this heuristic.

A verified compatibility receipt would need a native AC7 scene-pass producer to supply the
session, source/native frame, view and family keys alongside its exact context, constant-buffer
identity and active rectangle. The frame tap would correlate the receipt at the actual draw
with the buffer bound at the verified stage and slot. To consume an earlier upload at
recombine, it would also need the current frame's upload generation to detect subsequent
overwrites, invalidated mappings and pooled buffer reuse. Carry only numeric identities and
copied camera data past the callback, or retain a buffer reference for identity comparison
and release it on replacement, invalidation, frame reset or uninstall as appropriate. The
adapter supplies the native facts and decides primary-camera admission.

The [TrueSky depth evidence](../research/ue418-hook-map.md) still governs effect size, expected
code and first-party shader choice. A mismatched composite draw fingerprint logs one terminal
motion refusal per renderer. Its relay allocation or write can retry on subsequent sky renders
even when the independent cloud-grid decision is terminal. No patch site, expected byte,
effect fingerprint, depth format or shader changed.

UI classification reads bindgen-generated draw and layout constants. The HUD shader guard
calls the CRC-32C implementation already compiled into AC7, removing its native hash forwarder.
The effect guard shares IEEE CRC-32 with other adapters through the Rust SDK. The two algorithms
retain their original fingerprints.

## Source checks and remaining native boundaries

The UI and view synthetic fixtures moved into Rust. Their native executables preserve the
external capture probes and basic C ABI checks. The scope fixture keeps real C++ exception
containment checks, using the adjacent-marker ABI. Rust fixtures cover mismatched and busy
lists, atomic refusal at the command-count limit, lease retirement and independent motion
retry after both grid outcomes.

The native graph fixture still uses `tests/ac7_view_fixture.h` for an engine-layout input.
`render_scope.cpp` and `ac7_scope_owner.h` keep the exception boundary. The scene-colour
port keeps the existing COM retain/release leaves in `scene_color.cpp`; contact shadow keeps
Windows SHA-256 and C++ vector allocation in `contact_shadow.cpp` and `contact_shadow.h`.
TrueSky depth and motion keep the native D3D11 dispatch, compiler and COM boundary in
`truesky_depth.cpp`, `truesky_depth.h`, `truesky_motion.cpp` and `truesky_motion.h`.
`cloud_depth_platform.h` and `ac7_scene_color.h` continue to describe the Rust owners' C
interfaces.

Checks were limited to caller and source review, bindgen 0.72.1 generation, Rust 1.94.1
formatting and Git whitespace checks. Builds, tests, lint and runtime checks are user-owned.
