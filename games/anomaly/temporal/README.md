# Anomaly temporal boundary

Pure first-party C++20, header-only, adapter-local support logic. These types do not install hooks, allocate GPU resources, produce a velocity texture, activate SR/FG, or establish rendering readiness. No engine headers or reference-source code are imported. Do not export these STL-owning types across a DLL boundary.

The source constraints come from `docs/research/anomaly-world-space-ui.md`, referencing the read-only Anomaly 1.5.3 source revision `028ccf5`. Those source findings are not verified shipping-binary hooks. Numeric fixtures are not game or GPU acceptance.

## Files

- `planning.h`: independent output, world-render and PDA-content dimensions.
- `math.h`: explicit matrix convention, finite arithmetic, inverse, projection jitter and current-to-previous pixel motion.
- `history.h`: per-source-view camera/instance histories and lifecycle retirement.
- `frame_plan.h`: source-frame-bound geometry and unjittered UI canvas plans. No Present ownership is inferred.

## Hook feeding contract

1. At a verified source render-view boundary, identify **main versus scope** and call `begin_frame`. Supply the extended, monotonic source render counter for that view, the actual unjittered world view/projection, actual render-pixel jitter and independent dimensions. Scope source frames advance without Present. A global source counter can have gaps between samples of one view. Do not use a swap-chain Present count or the immediately preceding other-view frame.
2. Supply the first-person view/projection and jitter separately when the HUD/item producer uses them. Omission means unavailable, never "use main camera". `FrameInput`'s default zero camera matrices intentionally fail validation. An identity camera must be supplied deliberately, not used as missing-data substitution.
3. At the actual rigid instance producer, populate `InstanceSnapshot::world` from that instance's world transform. Default snapshots fail validation. Capture against the `FrameToken` returned for that source view.
4. For skinning, snapshot the **final local-space shader palette in its actual shader order**, including a stable nonzero `palette_layout` identity. The source's CLBone/SkeletonX producer/consumer distinction matters: copy current-only state before the engine overwrites or reuses it. Do not advance history in every skinning/shadow/main draw call. Snapshot the whole instance palette once; batch-local subsets must be mapped to that stable layout by the adapter.
5. For deformed geometry, provide a nonzero schema/layout ID and all exact shader deformation constants needed to reproduce the draw, including relevant wind/wave/time values. The schema is adapter-defined, not an inferred engine layout. Skinned snapshots can additionally contain deformation. Rigid snapshots reject otherwise-ignored bones/deformation rather than claiming camera-only coverage.
6. For detail/grass items, provide that **actual item's** world placement/rotation and effective fade/scale, plus its deformation constants. Key the item lifetime, not an upload index, batch index, cache slot or raw pointer alone. Repacking a batch must preserve each item's key. Changing fade/scale belongs in the snapshot, not in the identity key.
7. Obtain stable nonzero `owner` and `visual` identities from verified lifecycle tracking. `generation` is nonzero and monotonically increasing for reuse of the owner/visual pair. Distinguish different render instances even when they share a visual. Namespacing IDs is the adapter's responsibility. On destruction/cache-item retirement, call `retire` before that identity can be reused. New content in a recycled address/slot requires a new generation. These primitives cannot prove that a caller's identity came from a real lifecycle hook.
8. Consumers use the frozen current/previous snapshots and the matching `cameras(token, domain)` pair to render geometric motion. Evaluate the real previous skinning/deformation/fade path, not only the previous camera. `previous == nullptr` means missing history; it is not permission to substitute another object, identity bones, current pose, zero motion or a camera-only path. Missing deformation inputs reject capture. A valid CPU pair is still not evidence of a complete GPU motion pass.

The history owner is single-render-thread and noncopyable. External lifecycle notifications must be serialized onto that owner. There is no hidden locking around maps or producer reads. Snapshot allocation and checked matrix indexing can throw; contain exceptions inside the adapter, not across the C ABI.

## Frame and instance invariants

- Main and scope keep independent source timelines, cameras and object maps. World and first-person coordinate spaces also use independent instance slots, even for the same owner/visual/generation.
- `begin_frame` advances a view only for a strictly newer source frame. Same-frame identical input returns the same token. Changed input for the same frame is `Conflict`; older source frames are `OutOfOrder`. Invalid inputs do not mutate history.
- `capture` deep-copies producer data into immutable owned storage on the first capture. Identical repeated passes return the same immutable snapshot objects and frozen previous state. Conflicting same-frame updates return no usable snapshots and must not be ignored by the adapter.
- Only instances captured in the immediately previous source frame **of that view/domain** can contribute previous state. A missing/cull-only interval breaks object history even if an older snapshot is still held by a consumer. View counters need not differ by one.
- Owner, visual or generation changes do not match. Changes of instance kind, palette layout/count or deformation layout/count do not match previous state. Schema IDs must change when ordering or semantics change, not merely when values change.
- Retirement clears both view histories for the retired owner/visual generation and all older generations. Persistent retirement watermarks reject attempts to resurrect those generations. A higher generation starts with no inherited history.
- `CaptureStatus::MissingProjection` distinguishes a missing first-person camera from a stale/foreign frame token. Neither supplies fallback data.
- Consumers may keep immutable snapshot storage alive, but must discard queued draw/submission work when its instance retires or its view resets. A held `shared_ptr` is not a resource lease, a valid current token, or proof that an object is still live.

Bone palettes are bounded to 1024 matrices per snapshot and deformation blocks to 256 float4-equivalent values. Instance maps retain only current/previous source frames per view. Retirement watermarks persist for the history object's lifetime; the adapter owns session/level lifetime and memory budgeting. Destroy the history owner only when the associated source session and queued work are fully retired. CPU matrices/constants use double precision; check representability and the actual shader ABI when converting to GPU float buffers.

## Reset policy

`camera_cut` and any change to output, render or PDA dimensions conservatively reset previous camera and instance data for that view. A PDA-only resize currently resets that view too; this is conservative invalidation, not conflating its texture dimensions. `first_person_cut` drops only first-person previous data. Ordinary projection changes are tracked as real motion; discontinuous projection changes must be declared cuts by the producer.

Use `reset_view` for aborted/missing view production, a source counter restart, load/teleport discontinuity or a reset confined to that view. Use `reset_all` for a device-wide/source-session discontinuity. Extend wrapping source counters before submission, or explicitly reset before restarting them. All reset paths invalidate prior tokens without reviving retired instance generations. Fresh tokens have an owner identity and serial, so another history owner or a restarted source frame cannot alias an old token. Session identity/serial exhaustion fails closed.

## Math and dimensional conventions

- Matrices use row-major storage and **column vectors**: `clip = projection * view * world * position`. Convert engine storage/vector conventions at the hook boundary. Do not blindly copy the engine's matrix bytes.
- Camera space is left-handed, +Z forward; clip depth is D3D [0, W]. Matrices and inputs must be finite. World/bone/view transforms are affine; camera view/projection must be invertible. Object scale may be singular because projecting forward does not require an object inverse.
- Input projections are unjittered. `jitter_projection` applies pixel offsets with +X right and +Y down, scaled by the **render** dimensions and homogeneous W. Never apply it to an already jittered projection.
- UV origin is top-left. `pixel_motion` returns `(previous_uv - current_uv) * motion_grid_size`, excluding jitter. `pixel_motion_from_jittered` removes the two supplied render-pixel jitter offsets. Vendor sign/units conversion belongs at the explicit adapter/runtime boundary.
- Invalid/nonfinite arithmetic, overflow, near-zero/negative clip W or depth-clipped points yield no motion value. Offscreen XY values are not clamped. Inverse uses scaled partial pivoting with a 1e-12 relative pivot threshold. Matrix indices are checked individually before linearization.
- Each texture dimension must be in [2, 16384]. Render dimensions cannot exceed output dimensions; PDA dimensions are independent and need not share output size or aspect ratio. No global `Device` dimensions are mutated.
- `FrameViewPlan` uses render-sized jittered world/first-person geometry, output-sized reconstruction and screen UI, and independently sized unjittered PDA content. A PDA content canvas is a UV-space source texture, **not** output-space FG UI. Projecting it on a physical screen remains scene geometry with the appropriate world/first-person motion and occlusion.
- Scope planning is offscreen. Main planning says `RequiresHostDecision`, since a main/camera-ready source frame still does not establish a valid Present lease.

## Geometry producer (`../src/native_geometry.*`, `geometry_profile.h`, `geometry_feed.*`)

Standard DX11 executable only, every site guarded by expected bytes (`anomaly_geometry_evidence.py --verify` re-checks them, the RTTI slots, hierarchy vtables, world-matrix address and bone stride against the installed binary).

- `IdentityRegistry` gives each live `{CRender owner, visual}` a monotonic generation. Create/CreateChild/Duplicate register the returned visual and its hierarchy (FHierrarhyVisual, CKinematics, CKinematicsAnimated, FLOD) after retiring any pooled tree; Delete retires the tree before the engine pools or frees it. Nested factory creations get no identity until the enclosing model returns. Any unreadable hierarchy latches capture off until the next guarded install.
- Render hooks (Fvisual, FProgressive, CSkeletonX_ST/PM, FTreeVisual_ST/PM) open a thread-local `DrawScope` and classify the draw once at entry from `CRender::phase` (shadow-map and Z-fill versus main) and the registry: `identified`, `ignored_shadow`, `ignored_static_world` (unregistered rigid visual whose live world matrix is exactly `Fidentity`: level geometry, camera-only motion is exact), `unidentified_dynamic`, `unidentified_deformed` (unregistered tree), `unidentified_ambiguous`, `unidentified_unreadable`, `unidentified_nocapture`. The world matrix is read at entry only for that classification, never as a motion input.
- Draw-time call site: `CBackend::constants.flush` (`0x84b930`) runs once per D3D draw immediately before `DrawIndexed`. `geometry_observe_draw()` counts every flush by pass and by inside/outside a hooked visual and, for an identified main-pass draw, calls `capture_current_draw()` and hands the copy to the optional `DrawSink`. `capture_current_draw()` copies the world `Fmatrix` (module + 0xd63c18) and, for hardware-skinned draws, the `mRenderTransform` of bones `0..RMS_bonecount-1`, exactly as `CSkeletonX::_Render` uploads them, converted from row-vector engine bytes to this directory's column-vector `Mat4`.
- Context producers: `RenderViewScope` (opened by the observer's `CRender::Render` hook) raises `{Main|Scope, World}` from the engine's `IsSVPFrame` predicate. `r_dsgraph_render_hud` raises `FirstPerson`; `r_dsgraph_render_sorted`/`render_emissive` raise it between `rmNear` and `rmNormal` only. Shadow and Z-fill draws inherit the main-view context but are never captured.
- `feed_current_draw()` / `feed_draw()` map a capture to `InstanceSnapshot` for `TemporalHistory::capture`. Rigid and hardware-skinned draws only. Single-bone (world is replaced inside the original Render), software-skinned (no palette) and tree (needs foliage deformation constants) draws are refused as invalid coverage. A draw without a view and projection domain is refused as `MissingContext`; nothing is guessed.
- Retirements reach the render-thread history owner through `RetirementQueue`, never directly from the engine lifecycle thread.
- `[Anomaly] Mode=Observe` installs these hooks after the boundary hooks and logs registered/retired/identified/unidentified counters plus the per-pass and per-type coverage rows and the draw-time line. There is still no history consumer wired to the `DrawSink`, so counters prove hook invocation only. See `docs/research/anomaly-geometry-coverage.md`.

## Verification

`tests/anomaly_temporal_tests.cpp` is a standalone native executable; no game, engine or GPU dependency. Its optional argument selects an exact test case; unknown selections fail. The default executes every registered test with explicit checks (not `assert`, so Release tests remain active).

The isolated worker build and actual execution results are recorded in `.local/anomaly/temporal-worker.md`. The parent registered these contracts through `tests/anomaly_support.cmake` and rebuilt the main Windows/MSVC Debug target: all 17 internal cases passed, and unknown-case selection correctly failed. Adapter integration remains pending. Passing these tests establishes CPU behavior only; source-to-binary hooks, real resource formats/lifetimes, dense geometry motion coverage, game captures, SR/FG and image quality remain unverified.
