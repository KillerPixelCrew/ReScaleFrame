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

The [TrueSky depth evidence](ue418-hook-map.md) still governs effect size, expected
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

## PORT.md handoff

Source-closed paths:

- `games/ac7/include/rescaleframe/ac7_render_scope.h`
- `games/ac7/include/rescaleframe/ac7_ui_rules.h`
- `games/ac7/include/rescaleframe/ac7_view.h`
- `games/ac7/src/ui_rules.cpp`
- `tests/ac7_render_scope.cpp`
- `tests/ac7_ui_rules.cpp`
- `tests/ac7_view.cpp`

The other inventoried paths remain in use at the native boundaries listed above.

Source-closed review findings:

- Queue point-in-time scope markers in one operation.
- Bind each queued scope ticket to its originating command list.
- Track cloud motion relay installation independently of the full-grid transition.
- Generate the numeric draw and layout classes consumed across the AC7 UI ABI.

Open review findings:

- Report a full-target view separately from player-camera identity. The reader and adapter
  now distinguish rectangle coverage from scene-pass buffer association. G02 and an AC7
  native producer still need to carry the compatibility receipt described above.
- Share fallible allocation for Rust-owned FFI handles. Shared helper and AC7 adoption
  landed in `3f5527b6`; Vulkan FSR3 and RDR2 caller adoption require their owners.
- Reuse the compile-time ASCII widening used by the Windows adapters. Shared helper and AC7
  adoption landed in `3f5527b6`; Godot caller adoption requires its owner.
- Reuse one IEEE CRC-32 implementation for binary fingerprints. Shared helper and AC7
  adoption landed in `3f5527b6`; RDR2 caller adoption requires its owner.

`3f5527b6` can be applied independently of the later A02 changes. The later changes use
its AC7 adoption and build on it.

## Residual source port from 821642ae

The native graph and compatibility route have separate producers. In
`loader/proxy/src/dlss_bridge.c`, successful `rsf_plugin_session_start` sets
`native_owner` and suppresses compatibility `on_pass` installation. When that
start refuses, the compatibility draw callbacks remain active. The native graph
producer in `games/ac7/src/native_renderer.cpp` only publishes stage data from the
active renderer. Its SR plan checks the graph context's view against the view at
`hooked_postprocess`, then reads that view's family and native frame. The pixel
uniform observers in `sites.rs` and `native_renderer.cpp` are diagnostic hooks
within the same native lifecycle. They cannot provide a compatibility receipt
when that lifecycle refuses.

The compatibility path's `on_pass` reads the buffer offered by
`runtime/orchestrator/src/frame_tap.rs`. The frame tap picks a recent PS constant
buffer of the configured size and retains it for the callback. Its pass index
counts qualifying tap observations. That index has no join to AC7's native frame,
view or family. Compatibility `observe_constants` invalidates and repopulates the
cached camera by buffer identity and extent, which leaves same-allocation reuse
ambiguous. A genuine receipt requires a producer available in compatibility
mode, a verified native-to-D3D constant-buffer binding at the scene draw, and a
generation-aware upload join before recombine. The source reviewed here does not
establish that chain. The rectangle-versus-camera review finding stays open.

`scope_facts.rs` now owns the guarded reads for a renderer's single primary view
and the primary translucency scope. It uses the existing native layouts:
renderer view storage and count at +0xb8/+0xc0, view family at +0, family frame
at +0x68, render and output rects at +0x70/+0x80, unconstrained rect at +0x90,
and the three capture flags at +0xc42. Native still supplies the existing
fault-contained read, current configuration and session, and owns hook timing,
the lease and its enqueue. The source renderer gate retains its nonzero output
requirement. The scope's mip bias still uses render/output width and the existing
-0.3 offset and -2 lower bound. The Rust calculation widens the rectangle
coordinates before subtraction.

The scene-colour ABI now calls Rust directly. It retains the existing
capture-derived selection and per-frame association and keeps the old
retain-before-release order when a replacement includes the same allocation.
The contact-shadow hash still uses the System32 CryptHashCertificate2 export,
the same SHA-256 and DXBC guards, and the original rewrite. Rust resolves the
hash export. The native hook lends its existing vector via the shared fallible
resize callback and consumes it only when the transform succeeds.

TrueSky's replacement depth-bounds and composite-motion HLSL bodies moved to
separate source files. Rust uses the same System32 compiler, strict cs_5_0
profile and entry names. The one-to-one depth relay keeps native binding
validation and dispatch. For composite motion, Rust reads the actual t1/t2
SRVs and b12 constants from the same context, checks their descriptors and
viewport, then writes the viewport-sized R32_FLOAT target. It restores the
saved CS shader and class instances, t0/t1, u0 and b0/b1. The UAV initial-count
argument stays null on bind and restore. Device replacement, terminal
per-device pipeline refusal, retryable target allocation, and callback-owned
state retirement retain their existing behavior. The returned texture is
borrowed from that state for the native stage callback. The graph fixture now
gets its existing synthetic view bytes from a Rust object linked only to the
fixture.

Newly source-closed PORT paths are `scene_color.cpp`, `contact_shadow.cpp`,
`truesky_depth.cpp`, `truesky_depth.h`, `truesky_motion.cpp`,
`truesky_motion.h` and `tests/ac7_view_fixture.h`. The scene-colour,
contact-shadow and cloud-platform headers still declare the live C boundaries.
`render_scope.cpp` and `ac7_scope_owner.h` still contain or declare the C++
exception leaves. The three earlier fixture closures keep their existing C++
ABI, exception or capture-probe checks.

Validation uses source comparisons, regenerated bindgen 0.72.1 records,
Rust 1.94.1 formatting, locked offline Cargo metadata, and Git whitespace
checks. Compilation, linking, fixtures and device execution remain unverified.

## AC7 C11 contracts

The UI classifier ABI probe uses a C11 launcher. It still checks null classification and
layout, accepts an optional exact 1,104-byte captured shader, and requires the classifier
to reject a copy with its final byte flipped. The CTest name, Rust library and C++ link
driver are unchanged. The cloud depth platform header is a C declaration and layout
contract for the three Rust implementations in `cloud_depth/d3d11.rs`.

Strict C11 syntax checks passed for the launcher and the cloud header, including a
56-byte state layout assertion. Linking and fixture execution remain pending.

## Compatibility camera admission at b5a7434c

The question is whether the compatibility route can verify that its camera
belongs to the scene the player sees. This source pass reuses the AC7 executable,
decrypted image and UE 4.18.3 revision recorded in the
[native ownership reference](../../.agents/skills/ue418-render-integration/references/ac7-native-ownership.md).
It traces the active callback branches and their resource lifetimes. The
[existing pixel-bind research](ac7-native-renderer-refactor-20261001.md)
establishes the RHI pixel uniform setter and slot argument, but its
null-binding diagnostic does not establish a compatibility camera producer.

| Source boundary | Facts available to the compatibility consumer |
| --- | --- |
| `view.rs::read_view` | `fills_target` describes a zero-origin view matching its own recorded buffer extent. Projection and jitter checks validate the decoded data. A full-target capture or translucency view can also satisfy them. |
| `dlss_bridge.c::rsf_bridge_start` and the native renderer | Successful plugin activation sets `native_owner` and omits the compatibility pass callback. In compatibility mode that callback remains installed, but the native SR producer in `hooked_postprocess`/`make_plan` requires an active renderer. That producer matches the graph context's view key and reads its family and native frame. The native diagnostic uniform hooks also require activation. |
| `frame_tap.rs` pixel constants and reconstruction pass callbacks | The retained PS buffer is a recent changed binding selected by byte width. The tap reports it with the compatible scene/depth/motion signature and context. The pass index is the tap's observation counter. There is no native session/source frame, view/family or proven camera slot in this record. |
| `frame_tap.rs` and `frame_tap/detours.rs` upload callbacks | The installed context admits qualifying whole-subresource zero-index `UpdateSubresource`/`UpdateSubresource1` calls and paired zero-index `WRITE_DISCARD` Map/Unmap. The callback carries the resource, bytes and user pointer, with no write serial or native identity. Boxed updates, unmatched mappings and resource copies do not produce upload records. `ClearState` and non-restoring command-list execution can drop tap binding observations without notifying the bridge's retained upload candidate. |
| `dlss_bridge.c::on_gate` | Recombine runs before the later reconstruction input observation. It can only use a current Present interval upload to a previously observed buffer, with a compatible extent. The composite gate can keep the latest candidate for a lens, direction and distance check. These are compatibility heuristics. They cannot establish the native producer of an upload. |

The bridge now names rectangle refusals and camera candidates accordingly.
It invalidates the upload when the observed context, scene/depth/motion/exposure
set, View buffer or decoded extent changes. Recombine uses it only on that
context with the active reinsertion plan's scene-colour source. The composite
gate also checks that association before updating its continuity sample.
An unrelated cached camera can no longer veto recombine when that association fails.
Present, successful plan replacement, detach and shutdown clear the candidate.
The existing continuity check can still postpone a gate for a later candidate.
When association or backend evaluation fails, the recombine gate still calls
`rsf_promote_seed` with reconstruction disabled and keeps the game's colour as
its source. The composite and Present compatibility routes and native activation
still use their existing entry points.

The remaining join needs a producer available when native activation refuses,
with native session/source frame, view and family identity established at the
scene pass. It must associate those facts with the exact D3D11 context, bound
shader stage and slot and buffer used by that draw, then account for the
buffer's write generation through recombine, including bypassed writes,
command-list execution and invalidation. The pinned source provides neither
this compatibility producer nor an upload generation that can validate it.
Pointer retention and the stricter compatibility association do not resolve
same-allocation reuse. PORT line 1754 remains open.

Validation is source-only. Strict MinGW C11 syntax passed for the bridge.
Targeted bindgen reproduction, pinned formatting, locked offline Cargo metadata
and Git whitespace checks cover this change. Runtime behavior remains unverified.
