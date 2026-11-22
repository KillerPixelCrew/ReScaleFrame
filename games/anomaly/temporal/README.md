# Anomaly temporal boundary

First-party Rust fixture model for Anomaly temporal behavior. The model consumes copied fixture inputs and is independent of the live SR and motion implementations. Its C interface is private to the native geometry fixture and transfers plain records and an opaque owner.

The source constraints come from `docs/research/anomaly-world-space-ui.md`, referencing the read-only Anomaly 1.5.3 source revision `028ccf5`. Those source findings are not verified shipping-binary hooks. Numeric fixtures are not game or GPU acceptance.

## Files

- `../src/temporal_fixture/oracle.rs` owns per-view history, retirement and source-frame-bound geometry and UI plans.
- `../src/temporal_fixture/oracle/math.rs` handles independent output, world and PDA sizes, finite matrix arithmetic, column-vector projection jitter and pixel motion.
- `fixture_platform.h` declares the native geometry fixture's plain data records.
- `../src/temporal_types.h` defines the native matrix and identity representations used by geometry and motion consumers.

## Hook feeding contract

1. At a verified source render-view boundary, identify **main versus scope** and call `begin_frame`. Supply the extended, monotonic source render counter for that view, the actual unjittered world view/projection, actual render-pixel jitter and independent dimensions. Scope source frames advance without Present. A global source counter can have gaps between samples of one view. Do not use a swap-chain Present count or the immediately preceding other-view frame.
2. Supply the first-person view/projection and jitter separately when the HUD/item producer uses them. Omission means unavailable, never "use main camera". `FrameInput`'s default zero camera matrices intentionally fail validation. An identity camera must be supplied deliberately, not used as missing-data substitution.
3. At the actual rigid instance producer, populate `InstanceSnapshot::world` from that instance's world transform. Default snapshots fail validation. Capture against the `FrameToken` returned for that source view.
4. For skinning, snapshot the **final local-space shader palette in its actual shader order**, including a stable nonzero `palette_layout` identity. The source's CLBone/SkeletonX producer/consumer distinction matters: copy current-only state before the engine overwrites or reuses it. Do not advance history in every skinning/shadow/main draw call. Snapshot the whole instance palette once; batch-local subsets must be mapped to that stable layout by the adapter.
5. For deformed geometry, provide a nonzero schema/layout ID and all exact shader deformation constants needed to reproduce the draw, including relevant wind/wave/time values. The schema is adapter-defined, not an inferred engine layout. Skinned snapshots can additionally contain deformation. Rigid snapshots reject otherwise-ignored bones/deformation rather than claiming camera-only coverage.
6. For detail/grass items, provide that **actual item's** world placement/rotation and effective fade/scale, plus its deformation constants. Key the item lifetime, not an upload index, batch index, cache slot or raw pointer alone. Repacking a batch must preserve each item's key. Changing fade/scale belongs in the snapshot, not in the identity key.
7. Obtain stable nonzero `owner` and `visual` identities from verified lifecycle tracking. `generation` is nonzero and monotonically increasing for reuse of the owner/visual pair. Distinguish different render instances even when they share a visual. Namespacing IDs is the adapter's responsibility. On destruction/cache-item retirement, call `retire` before that identity can be reused. New content in a recycled address/slot requires a new generation. These primitives cannot prove that a caller's identity came from a real lifecycle hook.
8. Consumers use the frozen current/previous snapshots and the matching `cameras(token, domain)` pair to render geometric motion. Evaluate the real previous skinning/deformation/fade path for the matched object. Missing previous snapshots and deformation inputs remain explicit. A valid CPU pair alone cannot establish GPU motion coverage.

The history owner is single-render-thread and noncopyable. External lifecycle notifications must be serialized onto that owner. Snapshot maps and producer reads need no internal lock. The C bridge borrows plain inputs for the call and retains Rust-owned snapshots.

## Frame and instance invariants

- Main and scope keep independent source timelines, cameras and object maps. World and first-person coordinate spaces also use independent instance slots, even for the same owner/visual/generation.
- `begin_frame` advances a view only for a strictly newer source frame. Same-frame identical input returns the same token. Changed input for the same frame is `Conflict`; older source frames are `OutOfOrder`. Invalid inputs do not mutate history.
- `capture` deep-copies producer data into immutable owned storage on the first capture. Identical repeated passes return the same immutable snapshot objects and frozen previous state. Conflicting same-frame updates return no usable snapshots and must not be ignored by the adapter.
- Only instances captured in the immediately previous source frame **of that view/domain** can contribute previous state. A missing/cull-only interval breaks object history even if an older snapshot is still held by a consumer. View counters need not differ by one.
- Owner, visual or generation changes do not match. Changes of instance kind, palette layout/count or deformation layout/count do not match previous state. Schema IDs must change when ordering or semantics change, not merely when values change.
- Retirement clears both view histories for the retired owner/visual generation and all older generations. Persistent retirement watermarks reject attempts to resurrect those generations. A higher generation starts with no inherited history.
- `CaptureStatus::MissingProjection` distinguishes a missing first-person camera from a stale/foreign frame token. Neither supplies fallback data.
- Consumers may keep immutable snapshot storage alive, but must discard queued draws when the instance retires or the view resets. Resource leases and token validity follow their own lifecycles.

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
- The frame plan uses render-sized jittered world and first-person geometry, output-sized reconstruction and screen UI, and independently sized unjittered PDA content. A PDA content canvas is a UV-space source texture. Projecting it on a physical screen remains scene geometry with the appropriate world or first-person motion and occlusion.
- Scope planning is offscreen. Main planning says `RequiresHostDecision`, since a main/camera-ready source frame still does not establish a valid Present lease.

## Geometry producer (`../src/native_geometry.rs`, `native_geometry_platform.c`, `geometry_profile.rs`)

Standard DX11 executable only, every site guarded by expected bytes (`anomaly_geometry_evidence.py --verify` re-checks them, the RTTI slots, hierarchy vtables, world-matrix address and bone stride against the installed binary).

- The Rust identity registry gives each live `{CRender owner, visual}` a monotonic generation. Create/CreateChild/Duplicate register the returned visual and its hierarchy (FHierrarhyVisual, CKinematics, CKinematicsAnimated, FLOD) after retiring any pooled tree; Delete retires the tree before the engine pools or frees it. Nested factory creations get no identity until the enclosing model returns. Any unreadable hierarchy latches capture off until the next guarded install.
- Render hooks (Fvisual, FProgressive, CSkeletonX_ST/PM, FTreeVisual_ST/PM) open a thread-local draw scope and classify the draw once at entry from `CRender::phase` (shadow-map and Z-fill versus main) and the registry: `identified`, `ignored_shadow`, `ignored_static_world` (unregistered rigid visual whose live world matrix is exactly `Fidentity`: level geometry, camera-only motion is exact), `unidentified_dynamic`, `unidentified_deformed` (unregistered tree), `unidentified_ambiguous`, `unidentified_unreadable`, `unidentified_nocapture`. The world matrix is read at entry only for that classification, never as a motion input.
- Draw-time call site: `CBackend::constants.flush` (`0x84b930`) runs once per D3D draw immediately before `DrawIndexed`. The flush hook counts every flush by pass and by inside/outside a hooked visual and, for an identified main-pass draw, captures the draw and hands the copy to the optional draw sink. The capture copies the world `Fmatrix` (module + 0xd63c18) and, for hardware-skinned draws, the `mRenderTransform` of bones `0..RMS_bonecount-1`, exactly as `CSkeletonX::_Render` uploads them, converted from row-vector engine bytes to this directory's column-vector `Mat4`.
- Context producers: the render-view scope (opened by the observer's `CRender::Render` hook through `rsf_anomaly_geometry_render_scene`) raises `{Main|Scope, World}` from the engine's `IsSVPFrame` predicate. `r_dsgraph_render_hud` raises `FirstPerson`; `r_dsgraph_render_sorted`/`render_emissive` raise it between `rmNear` and `rmNormal` only. Shadow and Z-fill draws inherit the main-view context but are never captured.
- The motion owner maps a capture to its history snapshot (`rsf_anomaly_geometry_snapshot_kind` classifies it; the contract fixture feeds `TemporalHistory` through the C oracle API). Rigid and hardware-skinned draws only. Single-bone (world is replaced inside the original Render), software-skinned (no palette) and tree (needs foliage deformation constants) draws are refused as invalid coverage. A draw without a view and projection domain is refused as `MissingContext`; nothing is guessed.
- Retirements reach the render-thread history owner through the Rust retirement queue, never directly from the engine lifecycle thread.
- `[Anomaly] Mode=Observe` installs these hooks after the boundary hooks and logs registered/retired/identified/unidentified counters plus the per-pass and per-type coverage rows and the draw-time line. There is still no history consumer wired to the `DrawSink`, so counters prove hook invocation only. See `docs/research/anomaly-geometry-coverage.md`.

## Verification

`tests/anomaly_temporal_tests.c` launches the Rust cases in
`games/anomaly/src/temporal_fixture/`. Each capture lends retained
Rust snapshots to its case, which checks identity and frozen values
across later source frames and retirement. The geometry fixture uses
the same model through a private C bridge. An optional argument
selects an exact case. An unknown selection fails, and the default runs
all 17 checks in Debug and Release. The model runs without the game or
a GPU.

The isolated worker build and actual execution results are recorded in `.local/anomaly/temporal-worker.md`. The parent registered these contracts through `tests/anomaly_support.cmake` and rebuilt the main Windows/MSVC Debug target: all 17 internal cases passed, and unknown-case selection correctly failed. Adapter integration remains pending. Passing these tests establishes CPU behavior only; source-to-binary hooks, real resource formats/lifetimes, dense geometry motion coverage, game captures, SR/FG and image quality remain unverified.

Those execution results precede the Rust fixture and oracle ports. The
latest port used pinned formatting, binding regeneration, locked offline
metadata, MSVC syntax checks and source comparison.
