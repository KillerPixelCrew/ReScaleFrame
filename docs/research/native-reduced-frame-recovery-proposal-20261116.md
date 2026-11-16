# Proposed reduced-frame recovery contract, 2026-11-16


Implementation status, 2026-11-16: root approved the independent request contract, and the generic implementation has now passed the coordinator Release gate and real device fixtures. Native deployment and game/quality acceptance are not established by this evidence. No worker build, test execution, GPU work or game run was performed. The proposal below retains the design history; the implementation and narrower completion classification are recorded at the end.
## Scope and conclusion

This is a source-only proposal for root review, not an implementation or a claimed visual fix. RSF commit8703a65 records the completed five-scale native-only captures and PID64776's complete Quality1/return-native handoffs, normal exit and21-entry restoration. Those results do not resolve the earlier user-rejected duplicated scene rectangle. No capture here proves that image was a fallback frame.

A separate source gap is proved: `configure_native_resolution` can publish reduced raster before `emit_sr` later refuses identity, reader, temporal depth/camera, screen, output preparation or actual evaluation. Existing auxiliary rollback restores reduced originals; it does not reconstruct a full image. Returning to native effects is not proof of recovery. The proposed fix is a **preflighted, same-source recovery obligation** whose GPU work belongs to the generic host. A camera-independent native raster lease supplies facts; it does not promote invalid temporal inputs.

No production/header/test file was edited. No build, test, device, game, process query, UI, REA, attachment, deployment or commit was performed. ENB research stays deferred. The executable contract remains Fallout4.exe1.11.240.0 SHA256 `fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`; the existing native geometry proof is reused, not replaced by reference names.

## Reference inventory reused and recovery walk

The complete [root inventory](fallout4-community-shaders-inventory-root-20261116.md) and [independent Claude inventory](fallout4-community-shaders-inventory-claude-20261116.md) remain the semantic inventory. Pinned CS revision is `e0e711594291c1a8e447ac758099492514810fbb`. Consumed pins: CommonLibF4 `7f8deca16dc7dd245c9c52151ec9ef27edd9ad60`, shared code `6e67083a54090d10806484549d07f75b7fbc8dc2`, Streamline fork `68bad8fe3ad61d25b1f94a17939c34ab3ce17d88`, DevBench `1d804c5241019ecc456a4c7d385109c90f56ce1f`. This task reopens the recovery assumptions, not the unrelated feature inventory.

| Mechanism inspected in full relevant path | Reference behavior | RSF comparison |
| --- | --- | --- |
| `TemporalRenderer.cpp`, PrepareRenderSize/ConfigureUpscaling | Query requested size, verify resources and external resolve/recovery prerequisites, then commit requested size | Size planning exists. Required recovery preflight before native reduction is missing. |
| PublishDynamicResolution | Build reduced proxies before publishing ratios/jitter | Deliberately different full-allocation native path. Producer/consumer mapping still needs acceptance; importing only proxies would not close it. |
| `TemporalResolve.cpp`, PreflightExternalResolve | Check Main/RTV identity, full allocation, source dimensions, input/publication resources, shaders, constants and raster/sampler states | Existing auxiliary preparation allocates too late and requires a temporal SR call. No reusable resource-only reservation is exposed. |
| PerformUpscaling | Capture source before provider work; distinguish skipped, failed and unsafe submission; recover safe skips/failures and publication failures | Normal temporal success exists. Early admission exits and safe failure recovery remain a gap. Unsafe completion must remain distinct. |
| ApplySpatialFallback/SpatialFallbackPS | Bilinear numeric source sampling bounded to the active input's last sample centre, private publication target, then publish into Main | Initial proposal uses existing numeric nearest machinery as a smaller, explicit recovery-quality difference. It does not change successful temporal SR or claim equal fallback quality. |
| UpscaleChainInputs/Depth/MotionAndMasks | Expand raw native downstream inputs after temporal **or spatial** resolution; separate VATS full copy because logical15 also holds menu background | Existing nearest auxiliary transaction is reusable for compatible surfaces, but it currently participates only in temporal evaluation. VATS content lifetime must survive recovery too. |
| `DynamicResolution.cpp`, target/proxy/depth override scopes | Swap resources, dimensions and inherited SRVs together; pooled acquire/release handling; partial setup owns unwind; Abort/Release restores slots/depth/geometry | No proxy framework is proposed. Its coordinated ownership and failure distinctions are required lessons, not a vanilla sampling proof. |
| RenderUI completion, native fallback/quarantine | Restore native state at a frame boundary, preserve outstanding scope cleanup, invalidate FG capture, schedule later native behavior | Existing RSF scope/auxiliary BUSY ownership is retained. A recovery failure must not clear a pending cleanup or report temporal success. |

Reference recovery is not universally guaranteed either: missing/foreign resources or an unsafe submission return unresolved. Its later native fallback is not proof that the already consumed reduced frame was repaired. Reference unsupported-mod restrictions do not reduce RSF's goal.

Source anchors, file hashes and line locations are retained in `.local/fallout4-research/native-reduced-recovery-proposal-anchors-20261116.json`. The proposal below is independently specified; reference code is not copied.

## Native facts and commitment order

Introduce a private adapter `NativeRasterLease`, independently of temporal camera conversion. It copies source/session/submission/native frame, native view/window/context/chain/resource generation, actual Main logical-map/resource/view identity, allocation/format, producer raster/sample envelope, and required downstream raw resource leases. It uses the existing guarded world/prepass/pre-imagespace callbacks and canonical device checks. It does not set `camera_valid`, invent a depth domain, substitute motion or issue a FRAME/INPUT/SIM event.

Separate two questions currently coupled in `read_native_inputs`:

- Is this the same ordinary native producer, and are these actual colour/raster/resources safe to copy for its native consumers?
- Are its camera, history, normalized vendor depth, motion and scene facts sufficient for temporal SR/FG?

A bad projection/history can fail the second while the first remains valid. A null/changed view, foreign device, intervening producer or unknown crop fails the first too. In that case recovery cannot guess ownership. Existing temporal guards remain unchanged.

Proposed sequence:

1. At the existing post-native-DR/pre-triangle point, query the host plan. Resolve actual allocation identities without reading unfinished contents. Validate native full-output scope prerequisites, expected bytes, context owner and required consumers. Ask the generic host to **RESERVE** recovery resources before writing any reduced ratio.
2. Publish reduction only after successful reservation for this exact source/configuration/resource tuple. Store the token as an outstanding source obligation. On any failed prerequisite, leave native raster unchanged. An old successful token is never reused after a new failure.
3. At the pre-imagespace seam, collect the independent raster lease **before temporal reader gates**. **SEAL** immutable raw Main and required auxiliary contents while owner/source association is still valid and before a provider can write.
4. Borrow the existing guarded native full-output state using raster-owner facts rather than requiring a valid temporal depth/camera descriptor. Refactor this fact boundary; do not weaken `native_depth_snapshot_current` or falsely satisfy it. The existing unit/nonpartial selection, genuine partial path, geometry identity/refcount preservation, LastError and conditional restoration obligations remain.
5. Attempt existing temporal processing only with genuinely valid temporal inputs. Whatever its outcome, **FINISH** the sealed obligation before native effects consume the source. The host chooses whether existing temporal publication completed or a safe spatial resolve is required. An early temporal refusal still reaches FINISH.
6. A complete temporal or spatial resolve drives the native full-output consumer scope through its verified last consumer. A distinct `resolved_full_output` fact can exclude native TAA for an already resolved current image; `temporal_published` remains false for spatial recovery. An unresolved source never suppresses TAA on a fabricated success, nor claims that native TAA necessarily rescues it.
7. Restore VATS contents at the existing Effects0..13 boundary and retire other auxiliary snapshots after their final consumer, then retire the ticket at source end. Do not discard an actual cleanup failure. No output-scope extension through lateRenderEnd/Swap is proposed without a demonstrated consumer; the previous late-call audit found no established final-copy draw there.

Every early return/exception after reduction needs an explicit finish/unsafe-cleanup route. A local source-obligation guard should make omissions reviewable. Cancellation is not recovery: it is permitted only when native producers have not consumed the reduced plan, and never returns a full-output flag. After content is consumed, inability to SEAL/FINISH is an unresolved current frame and blocks later reduction until a fresh full-native owner/resource validation and successful reservation.

Resource maps can legitimately change on resize/pool reuse. A retained COM reference preserves allocation, not frame contents. Initially require the preflight and seam identities to match; unexpected reassociation refuses. If native Main is discovered to be allocated only later, move reservation to a verified pre-producer allocation boundary rather than assuming the old map denotes the current frame.

## Smallest generic contract

The public auxiliary entry cannot be called with a fabricated SR pass: it requires `camera_valid`, roleSR and a real temporal evaluation, and rejects Main as an auxiliary. Its implementation also performs snapshots and GPU work immediately, so it is not a pre-commit preflight.

Propose one new export in the existing generic D3D11 host header, with a new independently versioned request. No existing structure is extended, so GameABI23, HostABI8 and the existing SR/FG tables need not change. This is a proposal, not a landed header:

```c
#define RSF_D3D11_REDUCED_FRAME_ABI_VERSION 1u
/* Operations: RESERVE=0, SEAL=1, FINISH=2, RETIRE=3, CANCEL=4. */
typedef struct rsf_d3d11_reduced_frame {
    uint32_t struct_size, abi_version;
    uint32_t operation, disposition;          /* disposition: output */
    uint64_t token;                           /* zero on RESERVE; output token */
    const rsf_game_render_pass* owner;         /* borrowed facts, not a temporal call */
    uint32_t surface_count, reserved;         /* reserved must be zero */
    const rsf_d3d11_auxiliary_surface* surfaces;
    int32_t temporal_result;                  /* output, never caller-selected */
    uint32_t result_flags;                    /* output */
} rsf_d3d11_reduced_frame;
int32_t rsf_d3d11_host_reduced_frame(
    void* services_user, void* immediate_context,
    rsf_d3d11_reduced_frame* request);
```

Expected Windows-x64 layout: size56/alignment8; offsets0,4,8,12,16,24,32,36,40,48,52 respectively. Static layout assertions and C header compilation are required in implementation. No STL, exceptions or owned allocations cross this boundary.

`owner` reuses copied identity, source/submission/native/view/window/resource generation, swapchain, render_rect/output_rect and leased colour input/output/readable fields. Its full current GameRenderPass size is checked under the already negotiated GameABI23. This endpoint explicitly does **not** interpret camera_valid, history, normalized depth domains, screen as temporal admission, or dispatch a render callback. No redundant crop or camera structure is added. Input/output colour may alias, using a private snapshot; auxiliary duplicate/canonical aliases remain rejected. Initial FO4 use has the same native Main as colour input/output/readable. Opaque engine keys are never dereferenced.

Surface descriptors reuse existing auxiliary kind, typed numeric format, rectangles and RESTORE_CONTENTS semantics. At most8, all declared surfaces required for that transaction. Omission means the adapter proved that consumer is not needed, not that a required missing depth/mask may be replaced with zeros. The host copies descriptors/identity and bounded labels and takes its own COM leases only after validating all borrowed resources. No caller-owned arrays/owner pointer survive the synchronous call.

Operation meanings:

| Operation | Required behavior |
| --- | --- |
| RESERVE | Validate current host/user/session/thread/chain/device, identity, plan/configuration generation, every descriptor and required format capability. Allocate/compile cached snapshots/outputs/shaders/constants/state without reading/writing native contents or changing bindings. Return a nonzero opaque token only on complete readiness. One outstanding token per host. |
| SEAL | Require exact token/source and current raster/resource association. Refine actual integer sample crop within reserved allocation capacity, recording planned versus actual size. Snapshot raw originals before temporal work. Preserve graphics bindings. No native output is committed yet. |
| FINISH | If the matching actual temporal call plus every auxiliary commit completed, retain that output. Otherwise produce spatial colour and required raw auxiliary outputs from sealed originals, but only when submission safety is proved. Prepare all products before the first native commit; ordered commit/rollback is one transaction. No duplicate temporal evaluation. |
| RETIRE | Drain exact pending content obligations on the graphics owner, including while host callbacks are suspended. Preserve committed Main and later writes on flags0 resources; restore only explicit RESTORE_CONTENTS resources. Repeat after an acknowledged empty token is idempotent, but a different pending token/source never disappears. |
| CANCEL | Release an unused pre-producer reservation with no native content claim. A sealed/published transaction requires FINISH/RETIRE, not cancellation. It never marks a consumed reduced frame recovered. |

Proposed dispositions: NONE0, RESERVED1, SEALED2, TEMPORAL3, SPATIAL4, CANCELLED5, UNSAFE6, CLEANUP_PENDING7. Proposed output bits: FULL_OUTPUT_DEFINED1, TEMPORAL_OUTPUT2, SPATIAL_OUTPUT4, NEXT_REDUCTION_ALLOWED8, CLEANUP_PENDING16, REQUIRED_RECOVERY_FAILED32, TEMPORAL_ATTEMPTED64. TEMPORAL and SPATIAL are mutually exclusive. A cleanup refusal retains the prior published-output bits; it does not rewrite a successful publication as an evaluation failure. FULL_OUTPUT_DEFINED means the required footprint was reconstructed from admitted native data, not a claim that every source pixel is visually or temporally correct.

ReturnRSF_OK means the requested operation completed, including a successful spatial FINISH. `temporal_result` and TEMPORAL_ATTEMPTED preserve the real attempted/not-attempted distinction. Size, version, operation and reserved bits are checked before resource reads; host/token/thread/session gates precede borrowed graphics dereferences. Wrong pointers/owners are not inspected in an attempt to rescue a frame.

Negotiation is explicit export presence plus request size/version. The new adapter resolves it before declaring reduced-quality implementation availability; an older runtime cannot enable reduction through this path. Existing adapters keep their unchanged ABI/contracts. No host callback table is appended and no new option is required. If an implementation instead appends a services table, its GameABI/table versions must be bumped and consumers negotiated coherently; that larger alternative is not proposed here.

## Reuse and necessary internal changes

Keep the mechanism in `d3d11_host.cpp` and `auxiliary_sr.cpp`, with one host-owned ticket and the existing presentation/graphics-owner serialization. Refactor existing preparation into resource reservation, immutable sealing, output preparation, commit and retirement helpers shared by temporal auxiliary and spatial paths. Do not add an independent graphics framework or engine-named branch.

The matching existing `process_upscale` call can adopt the sole sealed ticket by exact identity/resources, so no token field is appended to GameRenderPass. A foreign pass remains BUSY/refused. Reuse its originals and caches instead of double-copying Main and each auxiliary. Keep an immutable colour source until FINISH so partial provider publication cannot poison recovery input.

The host needs an internal attempt disposition from the real interop transaction: **no GPU work submitted**, **ordered/quiescent completion established**, or **unsafe/unknown completion**. A nonzero SDK result alone does not establish safe rollback or fallback. Reuse actual D3D11/D3D12 fence/queue retirement evidence; do not issue another write while a provider may still write the same output. Unknown/removed-device cases return UNSAFE, not a success with native code expected to repair it.

Current `process` discards the result of auxiliary restore on failure. The implementation must propagate pending rollback/retirement before trying recovery, not infer an empty transaction from the evaluation result. It must also avoid restoring reduced Main after a successful spatial publication merely because a later VATS cleanup fails. Existing accepted-commit semantics already preserve Main; retain that distinction.

RESERVE freezes the host settings/configuration generation for this input obligation. A settings change after commitment cannot resize/reconfigure provider resources underneath it. The core may skip that temporal attempt and recover the old committed crop, then apply the new request after draining. No backend-selection or skip-count policy is placed in the adapter.

## Spatial sampling and native downstream resources

Initial recovery uses the existing **numeric nearest** colour and depth/stencil operators, with no transfer function and no new filter option. It is a visibly lower-quality emergency resolve, not a substitute for successful requested SR. The source is the actual integer sample envelope asserted by the producer, not the whole allocation or a guessed texture index.

For destination pixel centre `p`, output edge rectangleO and source edge rectangleS, use the current nearest mapping `floor(Smin + ((p - Omin) / Oextent) * Sextent)`, clamped only to the declared valid source-centre set. Both rectangles are integer for this initial recovery operation. Native fractional viewportV must cover every centre inS: first centre atSmin+0.5, last atSmax-0.5, with the producer's right/bottom boundary rule. It need not cover the integer right/bottom texel edges. A viewport959.987 can cover centres0.5..959.5; a viewport ending at959.4 cannot. Do not round a viewport and assume coverage.

Use the same integer sample envelope and zero **resampling offset** for colour and all recovery auxiliaries. This deliberately preserves the source's jittered sample lattice. Zero offset is an operator choice, not a report that native jitter was zero, and does not require camera conversion. Do not combine camera-free colour recovery with jitter-subtracted auxiliary output. Successful temporal processing keeps its existing independently validated jitter/fractional-viewport mapping. No matrix, normalized depth or fabricated motion is passed to a vendor for recovery.

This restriction is what lets the current integer depth/stencil contract suffice. If real native evidence requires fractional affine recovery rather than a common integer sample lattice, the existing depth operator needs a versioned extension before enabling that case; silently mixing maps is not acceptable.

Required resource rules:

- Main R11G11B10 numeric colour and supported UNORM/float colour formats use existing texture/load/RTV capability checks. No typed UAV support is presumed. Snapshot source before writing an aliased destination. Preserve outside output_rect; normally output_rect covers the whole native image, preventing a stale surrounding rectangle.
- Raw native D24/stencil is reconstructed for native consumers with the existing depth/stencil helper. Preserve encoded near/world partition values and stencil bits. Never write normalized vendor depth into engine DSV storage. Vendor depth-domain normalization remains a separate genuinely admitted temporal operation.
- Motion and other auxiliaries retain raw values/declared units. Numeric resampling does not prove all native motion blur, forward or particle consumers correct; the required shader/buffer audit still applies. No current-as-previous or zero-motion substitution is permitted.
- Resolve actual logical targets and canonical owner at reservation and seal. Required refraction/motion/depth/mask failures prevent reduction. Optional means the native consumer is proved inactive/unneeded for the source. A feature that can activate mid-source without such an association requires conservative preflight of its resource or native-mode retention.
- Logical15's VATS/menu dual use requires either the existing full snapshot plus RESTORE_CONTENTS at Effects0..13 return, or an independently proved view-swap lifetime. Reuse the existing former contract initially. Do not blanket-restore flags0 resources after native clears/writes. Any active VATS failure remains a required output/cleanup failure, not an ignored optional mask.

## Outcomes, transitions and honest status

| Situation | Required result |
| --- | --- |
| Required preflight fails | No token/no reduced publication. Stay native for this source. A later source needs fresh full-native owner/resource validation plus a new complete reservation; no stale readiness reuse. |
| Temporal inputs invalid but raster/colour ownership valid | No temporal call or valid-input claim. FINISH spatially resolves the same sealed source and required native consumers. |
| Safe temporal skip/failure/publication failure | Recover from sealed original, retain original temporal result and reset obligation, report spatial output separately. |
| Unsafe provider ordering, foreign/new owner or lost producer association | Do not access or write foreign/new resources. Current frame is unresolved, not silently recovered. Block later reduction; preserve cleanup ownership until the valid graphics owner can drain or report permanent loss. This inability is an explicit completion limitation. |
| Cleanup fails after defined output | Keep defined-output disposition plus CLEANUP_PENDING; retain BUSY obligations and retry only on the correct owner. Do not rerun SR, roll committed Main back to reduced, or clear ownership merely to resume. |
| SR Off/backend/quality changes after commitment | Finish/recover old committed source first. Core applies desired changes at a safe boundary; adapter does not select a fallback vendor. Native inputs remain full when the new reservation is unavailable. |
| Resize/resource recreation | Drain before replacing owned resources. A change between reserve/seal refuses; no stale COM content inference. Native allocation/map reassociation needs guarded synchronization, or reduced support for that transition stays unaccepted. |
| Quiesce/stop | Stop new reservations, finish or explicitly classify the current obligation, allow owner-thread retirement while suspended, return BUSY until drained. No new hot-resume policy is invented. Cold start still bootstraps from a verified full-native scene. |

A spatially recovered source is not an accepted temporal input or a generated frame. It creates no input token, markers or provider evaluation count. Existing explicit FG-input provenance is preserved; recovery never manufactures it. The core owns whether generation is refused/reset for a recovered source. No pacing decisions or vendor calls enter the adapter.

Keep requested settings separate from effective output: spatial FINISH has effective temporal backend0 and an explicit recovered-output reason, with actual temporal result/attempted state retained. Do not publish sr_result0 or increment temporal success because a spatial copy worked. Existing reason/status can report this without new status ABI; the request result flags and bounded logs carry the detailed outcome. Desired backend/quality remain saved unchanged.

Capture should reuse the existing dump owner and immutable ticket inputs. SEAL must not consume an armed capture as if it were an evaluation. If a real evaluation occurs, reuse its matching capture record. If only spatial FINISH occurs, expose it as an actual recovery product with unavailable temporal constants/input products and actual source/view/resource identity. A partial recovery capture is not a complete SR acceptance. Preparation-only work still cannot consume the request.

Native TAA history on returning from resolved/suppressed frames needs a verified native reset path. Do not assume generic SR reset resets engine history, or copy an unverified native field write from the reference. If the current native reset producer is not established, SR-Off/reload transition acceptance remains a blocker even after spatial recovery exists. Full shader/motion coverage and the rejected image also remain blockers.

## Proposed implementation and validation batch, not executed

Production helpers: one `Host::reduced_frame` operation dispatcher; shared `AuxiliarySurfaces` reserve/seal/prepare-spatial/commit/retire primitives; an internal real-submission-safety result; a native raster-only fact reader and output-scope entry; a source obligation guard around all pre-imagespace returns. No new vendor integration, temporal admission relaxation, shader replacement/proxy framework or exported normalizer is required.

Extend existing real fixtures, not a new mock-only pipeline:

1. **Existing auxiliary WARP fixture:** call the shared production spatial preparation/commit/retirement helpers with real R11 Main, raw D24/stencil, RG16 motion and mask textures. Seed a uniquely coloured active crop and conspicuous stale outside allocation. Verify every output pixel follows the centre mapping, the whole declared output is written, raw depth partition/stencil bits and motion values survive, and original snapshots are unchanged. Include offset crop and fractional viewport covering all centres, plus truly uncovered-centre refusal. CPU readback is test-only.
2. **Transaction failures in that fixture:** unsupported/foreign resource before any mutation; canonical duplicates/colour alias; invalid bounds/format; prepare failure after another surface prepared but before commit; partial commit with rollback; pending rollback and later retirement retry. Verify Main is not spuriously restored after successful recovery, VATS restores exact original at its consumer boundary, and flags0 later native writes survive retirement.
3. **Existing hidden host/shared-SR device fixture:** drive real RESERVE/SEAL/FINISH with an admitted native raster lease but no valid temporal camera/depth publication, proving recovery does not need or create one. Require cold RESERVE to refuse before scene/pipeline readiness, without a ticket, token or content change, then retain the actual cold/no-source-token SR refusal and successful bootstrap Present. After scene readiness, use an actual missing-depth pre-submit failure to require FINISH spatial flags and real full-output pixels without adding temporal/FG success counters. This corrects the original cold-spatial expectation: recovery cannot be reserved before its own readiness contract is established. Then drive a real successful temporal call and prove FINISH does not spatially overwrite it or duplicate auxiliaries.
4. **Real failure boundaries:** induce supported invalid temporal metadata/pre-submit rejection through existing call arguments and assert no provider submission before spatial recovery. For post-submit failure, use only a real existing fence/device safety outcome or a focused production-helper fixture that exercises rollback state. A mocked SDK return cannot prove GPU quiescence; unavailable unsafe-failure injection remains unvalidated, not green acceptance.
5. **ABI/lifetime/owner negatives:** C and C++ layout/offset assertions for56bytes; short size/wrong version/unknown op/reserved bits; missing export keeps reduced capability unavailable; foreign host/session/thread/token/source/view/chain/resource generation; wrong owner refused before pointer reads; cancel after seal refused; duplicate FINISH is idempotent without reevaluation; mismatched pending token remains owned; suspended owner RETIRE drains; quiesce BUSY until complete. Check graphics bindings, including inherited high slots, UAV counters and nondefault OM/raster state.
6. **Transition fixtures:** settings/quality/backend/Off changes between reserve, seal and finish; resize/recreated resource between phases; preflight failure then later fresh native validation; stop while sealed or cleanup-pending. Check frozen current-source dimensions, deferred core reconfiguration and reset/status/capture semantics.

After root's coherent Release gate and explicit hidden device runs, bounded native-only rejection experiments should capture a successful requested reduced source, a legitimate loading/camera-history rejection with still-valid raster ownership, a standard settings/quality/Off transition and a safe provider-preparation failure. Use existing live controls and ordinary native transitions; no hidden rejection option. If a reliable failure cannot be induced through supported paths, retain the real device failure fixture as that narrower evidence rather than fabricate a game failure. Collect matched current-source colour before/after resolve, downstream/HUDless/final Present and the bounded stage rows, with VATS/menu and moving views where their consumers are active. Confirm no reduced commit on preflight rejection, no stale surrounding rectangle after recovery, honest spatial status and native history/reset behavior. Unsafe ownership loss must visibly refuse and block the next reduction; it cannot be counted as successfully recovered.

All proposed tests and runs remain unperformed. Root must settle the contract before production/header/test implementation.

## Approved implementation source freeze, 2026-11-16

Root authorized implementation after the source-only proposal. The public declaration is in
`runtime/orchestrator/include/rescaleframe/d3d11_host.h`. The request remains 56 bytes,
alignment 8, with token at 16, owner at 24, surfaces at 40 and temporal_result at 48.
Its independent ABI is 1; Host ABI 8, Game ABI 23 and the existing SR/FG tables are
unchanged. The existing host contract target now also compiles the declaration as C.
`stddef.h` makes the public header's existing wchar_t use self-contained in C.

The implementation uses one host-owned ticket and the existing `AuxiliarySurfaces`
cache and numeric nearest shader. RESERVE validates the configured graphics owner,
current planned configuration generation, same source/session/view/native/submission/
window/resource identities, canonical device, immediate context, required surfaces and
formats. It allocates snapshots, outputs, views, shaders and state without native-content
copies, draws, readbacks or binding changes. A private allocation helper in
`runtime/graphics/src/depth_scale_private.h` and `depth_scale.cpp` is also called by the
unchanged public depth prepare function, so raw D24 readiness and execution use the same
allocation primitive. Public depth API/layout are unchanged.

SEAL accepts an actual integer source crop refinement within the already admitted
allocations and snapshots the produced Main and raw consumers. It does not consume a
capture request. Recovery mapping is integer, zero operator offset and numeric nearest;
it does not invent camera validity or normalized temporal depth. The adapter still owns
proof that the native fractional viewport covers these pixel centres and that every
required later consumer has been supplied.

A genuinely valid temporal pass makes exactly one existing
`rsf_d3d11_host_upscale_auxiliaries` call. It supplies its real fractional colour mapping
and jitter and is adopted only by the exact sealed identity/resources. Private auxiliary
products are redrawn from the same sealed originals, including raw depth/stencil through
a private redraw helper. No second snapshot or temporal evaluation is introduced. A
plain process_upscale call cannot omit required reserved auxiliaries and gain temporal
success. New requested settings are deferred while the ticket owns its generation; a
changed generation skips that temporal attempt and leaves FINISH responsible for the
already committed raster.

FINISH retains the actual matching temporal publication or redraws/commits the spatial
products from immutable originals. Main's sealed destination backup supplies pixels
outside output_rect, even if an intervening failed temporal attempt touched Main. A
successful commit drops Main rollback obligations. RETIRE restores only surfaces carrying
RESTORE_CONTENTS and preserves flags-zero later native writes, including raw depth and
motion. Retirement failure keeps the already published Main and its output flags. Exact
completed tickets acknowledge repeated FINISH/RETIRE until a new reservation; CANCEL
only acknowledges unused reservations and never claims a full reconstructed image.

### Native destination ordering proof, not GPU quiescence

The initial proposal asked for a general no-submission/quiescent/unsafe receipt. The
approved implementation uses a narrower fact that suffices for these destinations:
provider work is isolated from native write destinations, and any actual native writes
are ordered on the same immediate context. It does not claim all private provider work
has completed, and it adds no CPU wait or production readback.

The source chain establishing that fact is:

- `dlss_pipeline.cpp` allocates its own output via create_cleared_target and passes
  self.output as frame resources.color_out. Decode, fidelity correction and sharpening
  also target owned output. The adapter's Main is not the provider destination.
- `native_fg.cpp::rsf_native_fg_evaluate` copies D3D11 inputs into owned shared slot
  textures, translates the provider destination to slot.output and uses the existing
  D3D11 signal, D3D12 wait/signal and D3D11 wait before copying successful results into
  the pipeline output. A failure can leave private slot work pending, but cannot grant
  that work a native Main write pointer.
- `sr_legacy_adapter.cpp` maps the same private pipeline output through sr_bridge.
  `sr_bridge.cpp` copies native inputs to owned shared surfaces, substitutes its own
  D3D12 output, orders the queues using existing fences, and copies only successful
  output back to the private D3D11 pipeline output. It also orders refused recorded
  work before subsequent slot reuse.
- The direct D3D11 provider route receives the same private pipeline destination and
  records on the supplied immediate context. Native publication is subsequently and
  exclusively `native_sr.cpp` calling `native_reinsert` after pipeline success.
  `native_reinsert.cpp` preflights both native destinations, renders all conversions
  privately, then queues native CopySubresourceRegion calls on that context.

The private current-thread receipt in `native_sr_completion.h` records this native
write ordering after a normal evaluation return. It is not inferred from the SDK result.
Exceptions classify it unknown; the host separately checks device life. RESERVE rejects
aliasing the pipeline's private output as native input, Main or raw consumers, preserving
the isolation premise. Foreign temporal resources are rejected before auxiliary work or
capture. Unknown completion, device removal, owner loss or a partial required commit
retains the ticket as BUSY and blocks another reduction. Such cases are unresolved current
frames, not a successful native fallback. Failed legacy auxiliary rollback is also
propagated as BUSY instead of being discarded.

### Actual output and capture semantics

All disposition, temporal_result and result_flags fields are overwritten from host state.
Spatial success reports effective temporal backend 0 and a spatial-recovery reason; it
does not increment temporal success or FG counters. TEMPORAL_ATTEMPTED distinguishes an
actual admitted host attempt from camera/API or changed-generation refusal before it.
Only RETIRE or unused CANCEL returns NEXT_REDUCTION_ALLOWED.

If a recovery publication consumes an armed capture, the native colour image comes from
the sealed original and the actual recovered product is named spatial_recovered. Its
result is spatial_recovery, not a fabricated sr result 0. Temporal constants and other
unavailable temporal products remain unavailable. This partial diagnostic is not SR
capture acceptance.

Root additionally requested a bounded receipt at the existing capture Present refusal.
The original WINDOW/session/source/native/view/scene-path predicate still short circuits
in the same order and calls no extra native or graphics query. Each first failing term
can emit one row, at most six rows per host. Reached/passed masks, capture and reached
WINDOW identities, existing event-thread value and completed flag are recorded. The
scene path is logged only when its original predicate was reached; otherwise UINT32_MAX
is an explicit unreached sentinel. No capture matching behavior was changed.

### Fixture coverage verified by the coordinator

| Existing fixture | Coverage in the passing coordinator run |
| --- | --- |
| depth_scale_device | Allocation-only reserve preserves packed native bits and viewport; unsupported format refuses; the existing real prepare/commit path uses those reserved allocations. |
| auxiliary_sr_device | Reserve and seal are separate; actual native writes after reserve are observed by seal. A temporal fractional/jitter product can be abandoned and spatially redrawn from the same originals. Offset integer crop maps Main and raw D24/stencil/motion exactly; outside pixels survive. Packed R11 Main uses its real descriptor and positive finite nearest values. RESTORE_CONTENTS restores the designated consumer while successful Main and flags-zero later depth/motion writes survive. Existing rollback, malformed and foreign-resource negatives remain. |
| d3d11_host_contract | C and C++ 56-byte layout/offset assertions; independent request size/version/op/reserved checks and unconfigured/foreign user refusal precede graphics reads. |
| Explicit hidden shared-SR device fixture | Cold RESERVE refuses without ticket/token/content mutation, followed by the exact actual cold no-token EVALUATE_FAILED and successful bootstrap Present. An established-scene missing-depth pre-submit refusal recovers spatially; a real temporal success is adopted once. Separate no-camera and post-reservation SR-Off cases recover without temporal attempts, duplicate success or new vendor counters. Tests cover wrong token/context/session/thread/view/resource generation/output extent, cancel before seal and refusal after seal, repeated finish/retire, deferred planning and suspended owner drain. Existing actual provider reset sequence remains checked. |

No fixture result above has been executed by this worker. No synthetic SDK return is
used as a replacement for the real cold or invalid-input failure. No device-loss result
or driver allocation failure is fabricated as a passed GPU experiment. The malformed,
unsupported and foreign-resource paths exercise real preflight refusal; actual
removed-device/unknown-completion behavior remains a guarded source path pending any
separately authorized experiment.

The explicit hidden target is EXCLUDE_FROM_ALL. After its focused build, coordinator
invocation is:

```powershell
& './build/windows-x64/bin/Release/rsf_d3d11_host_shared_sr_device.exe' 'D:/Coding/ReScaleFrame/.local/fg-provider-sdk-comparison-20261115-202429'
```

It keeps its window hidden, FG Off, and uses a scratch SDK/capture root. It neither
accesses a game installation nor establishes scanout, native visual quality, latency or
moving-scene acceptance. Root owns the combined Release gate and all device execution.
The source/test SHA manifest is
`.local/fallout4-research/reduced-recovery-source-freeze-20261116.json`.

This implementation closes the demonstrated missing recovery mechanism only after its
checks and native handoff validate. It does not attribute the earlier duplicated image,
prove missing shader/motion coverage, reset native TAA history by guesswork, or waive an
unsafe native raster owner. Native producer/consumer integration and matched success,
rejection, quality and transition captures remain required.

## Coordinator build and device evidence, 2026-11-16

The coordinator's retained receipt is
`.local/fallout4-research/reduced-recovery-built-20261116.json`, identifying source HEAD
`e4f283df078e357e695f60970196f612c8f2e8a9` plus the uncommitted recovery batch. The
complete Release gate `verify-reduced-recovery-20261116.log` exited 0: 224 passed,
14 skipped, 238 total, 113.57 seconds of tests, with Cargo fmt and Clippy clean.
Within that gate, depth_scale_device passed in 0.04 seconds, auxiliary_sr_device in
0.10 seconds and d3d11_host_contract in 0.07 seconds. These are actual coordinator
build/device results, not worker execution or a source-review substitute.

The explicitly built hidden shared-SR fixture finally passed in
`build-reduced-recovery-shared-sr-r4-20261116.log` and
`test-reduced-recovery-shared-sr-r4-20261116.log`, both exit 0. Its actual observations:

- Cold RESERVE returned NOT_READY with no recovery ticket, CPU/provider token or
  native-content mutation. The first actual evaluation still returned EVALUATE_FAILED
  without backend constants, then the cold bootstrap Present returned S_OK.
- Source2 completed real temporal SR and ticket retirement: temporal_result 0,
  flags 75, which are FULL_OUTPUT_DEFINED, TEMPORAL_OUTPUT, NEXT_REDUCTION_ALLOWED
  and TEMPORAL_ATTEMPTED. FINISH did not spatially overwrite the temporal result.
- Source6 used a genuine missing-depth pre-submit refusal, temporal_result -1,
  then spatial reconstruction and retirement: flags 77, replacing TEMPORAL_OUTPUT
  with SPATIAL_OUTPUT. This is established-scene recovery, not cold recovery or a
  fabricated SDK error.
- Sources11 and12 passed no-camera and frozen SR-Off spatial recovery, including
  owner negatives, cancellation, reactivation planning and suspended retirement.
  The provider's eight actual SR evaluations remained unchanged by these cases.
- All eight assembled reset checks passed: sources2/3/4/5/7/8/9/10 respectively
  observed reset 1/0/1/0/1/0/1/0. Final sampled output channels were 0.25/0.5/0.75.
  FG remained requested Off, effective Off and tagged 0.

The exact-two-argument plain hidden marker fixture also passed in
`test-recovery-hidden-marker-20261116.log`: generic marker admission, fallback refusal,
keyed/bound derivation, failed start and repeated-end checks, plus shared SR/CPU-token
and presentation/retirement checks. No optional visible mode was used. This synthetic
marker result is not game input-owner, physical latency or FG acceptance.

### Retained failed attempts and fixture-only corrections

The shipping artifacts did not change across these corrections:

| Attempt | Actual result and correction |
| --- | --- |
| r1 | `build-reduced-recovery-shared-sr-20261116.log` failed C4456/WX because the captured render JSON string named raster shadowed the newly added owner. Only that local and its use were renamed captured_raster. No fixture execution occurred. |
| r2 | The explicit build passed, but `test-reduced-recovery-shared-sr-r2-20261116.log` exited 1 before a useful failure line. The fixture incorrectly demanded cold RESERVE success before the first actual scene configured the host. It was corrected to require refusal, unchanged initialized native texture bytes and no token, while retaining the original real evaluation failure and bootstrap Present. Source/phase/line diagnostics were added. |
| r3 | The explicit build passed and prior recovery cases plus source11 passed, but `test-reduced-recovery-shared-sr-r3-20261116.log` exited 1 at source12 RESERVE. Source11's active0/active1 transitions correctly advanced requested_generation; the fixture had omitted the producer's real configure-only planning step before the next reservation. Camera-validity-zero RECONFIGURE planning was added before querying/reserving. Generation guards and suspended drain were preserved. |
| r4 | Explicit build and fixture execution passed with those fixture-only corrections. Earlier failed logs remain retained; no production readiness rule or warning was suppressed. |

### Exact built checkpoint and remaining limits

The receipt records SHA256 values:

| Built artifact | SHA256 |
| --- | --- |
| ReScaleFrame.Runtime.dll | `95CBC864DE2723D93E90F1651599F273ED5DFA970D89E763C452241AC49AC54C` |
| ReScaleFrame.Game.Fallout4.dll | `9B5C79DF0B2DD2E5036B5DF1CC71BBCFF8773B1A124030B8746B40849F8D4E1F` |
| plugin-carrier xinput1_3.dll | `E4A809E7A7A7D802C7C83691727575FC288EB515130D54D60B608BDD7CF22C98` |

As of this receipt, the checkpoint was not deployed or native-game validated. The
Release and real device results establish the tested generic mechanism and negative
contracts; they do not establish native output-consumer completeness, successful
recovery in a live game, physical scanout, moving-scene reconstruction quality,
performance gains, FG or latency. The earlier duplicated-image rejection remains
unattributed. Required matched native success/rejection/transition captures and user
quality acceptance remain open; root owns the next bounded native trial.
