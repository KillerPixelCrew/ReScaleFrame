# Fallout 4 native reduced-frame recovery adapter, 2026-11-16

## Scope and evidence

This batch implements the adapter side of the approved [recovery contract](native-reduced-frame-recovery-proposal-20261116.md). Root has now built and generic-device-tested the combined implementation; it is undeployed at this freeze. It does not resolve the rejected reduced image, moving shader coverage, FG or latency acceptance. Native-only validation remains first; F4SE/ENB compatibility is deferred, not dropped.

The reused executable is Fallout4.exe 1.11.240.0, SHA256 `fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`. The complete pinned CS/reference inventory and shader coverage remain in the proposal and [full shader/buffer audit](fallout4-full-shader-buffer-audit-20261116.md). Reference code was not copied. No REA session was used, reopened or changed for this batch.

The preceding intention-only commit `e495934` passed root's full Release gate:224 passed/14 skipped,102.79 seconds, helper0.01 seconds, Entry SHA256 `F7B95E792207530BC11E5DDAB53165514AF9E4489057CDE3911F3C570F3318AF`, Runtime prefix `22830370`. At the original source freeze those built binaries were distinct from the recovery source; the later combined build below supersedes its unbuilt status. The root-owned bounded native run was untouched. This worker performed no build, compiled/executed fixture, device operation, process query, attachment, input, UI, deployment, restoration or commit.

## Why the fact boundary changes

The existing temporal reader correctly requires current CPU attribution, valid camera/history and partitioned-depth mapping. Those predicates cannot also be the sole way to recover a reduced native image: a rejected temporal projection is not evidence that the native colour allocation or producer raster vanished. Conversely, a surviving COM allocation is not evidence that it still holds this source's contents.

`NativeDepthSnapshot` now has a separate `raster_valid` flag. `capture_native_raster` reuses the verified native TLS/renderer/frame/view/depth owners and producer raster checks, independently of temporal matrices, near/world math, cache/history and sampled-CPU eligibility. It retains the existing copied CPU identity as the source association, but does not call `cpu_source_is_current` to promote an invalid sample. Current identity, native frame, graph/view, engine Context, immediate context/device, HWND, outer chain, allocation dimensions and mapped depth owner must still match. The separate temporal capture/mapping/current checks and their native byte guards remain unchanged.

The producer check still compares actual RS viewport to the native Context shadow, target descriptors, logical-to-physical maps, actual OM depth/colour views, allocation descriptors and scissor coverage. Temporal calls additionally retain all original near/world/unjittered camera-port comparisons. The raster-only branch validates the native normalized port independently, not by inventing a camera. Its crop remains integer LTRB derived from covered sample centres using the existing half-open `ceil(edge-0.5)` rule. Actual viewport XYWH remains separate; no guessed rounded render extent is substituted.

The new `read_native_raster_inputs` leases Main logical1, depth1, motion32 and native14/30/15 allocations through the existing verified RTM maps and renderer arrays. It verifies canonical device/context/outer-chain identity, full native output extent, actual SRV/RTV resources, view formats/subresources and native owner/map rereads. It does not use `GetBuffer`, invent a framebuffer alias or publish a camera/depth domain. Main's proved native texture is also its readable colour alias, consistently supplied by the temporal reader. This is the temporal reader's only changed output assignment.

## Commitment and native consumer order

| Boundary | Adapter action and obligation |
| --- | --- |
| Ordinary WorldBegin21F0AC0, caller returnC382A5 | Existing one FRAME/SUBMISSION/source association. No second source or CPU ledger. A previous recovery token prevents overwriting the World object. |
| Guarded UpdateDynamicResolution1839ED0, selected return21F9F34 | Original five-argument routine executes once. Core planning provides desired dimensions. Before any adapter FB8/FBC/FE5 write, independently preflight required resources, native full-output owner/geometry/context, and generic RESERVE. FE8 remains native-owned/read-only. Failed preflight makes no adapter ratio commitment. |
| Prepass21F38B0, selected return21F9F78 | Original producer executes once. Capture raster/resource facts even when temporal or sampled-CPU attribution refuses; capture temporal depth separately with all original guards. Foreign/nested producers clear the previous snapshot. |
| RenderEffectRange2185420 at21FA283, return21FA288 | Revalidate same-frame raster/resources and retained output geometry, then SEAL originals before temporal reader, CPU and screen gates. Only the actual native producer crop refines the reservation. |
| Genuine temporal admission | Preserve pre-mutation FG_INPUTS and the same valid depth descriptor. Evaluate once through existing `upscale_auxiliaries`, using the actual fractional viewport and jitter descriptors. The generic owner adopts sealed originals; there is no duplicate evaluation or adapter GPU implementation. |
| Every admitted seam return/unwind | A scope guard invokes FINISH. Temporal rejection with valid native raster reaches camera-independent spatial resolution. Unknown native producer/output ownership withholds resource calls, explicitly refuses and retains the unresolved token; no foreign image is called recovered. |
| Original Effects0..13 return | RETIRE restores logical15's explicitly flagged VATS/menu contents at the previously proved final consumer boundary. Flags0 resources retain reconstructed contents and later native writes. Whole-UI/Swap cleanup retries retain ownership on refusal. |
| Native UI/Swap end | Existing native output and input state retirement remains independent of the generic token. No next reduction or World reset while a token/resource/native-state obligation remains. |

Native seams, expected-byte guards, original call signatures and original NativePresent routine body are unchanged. Existing callback wrappers continue restoring the original routine's LastError. No new hook, source ID, native frame, input/SIM event, setting, vendor choice or pacing decision is added.

Full-output preflight copies the native engine Context, imagespace manager and original triangle28/38/40 identities. Reservation, seal and unresolved finish require that same association. The adapter never changes those pointers or reference counts. Existing scoped ratio1/FE4false/FE5false/jitter and explicit viewport-builder mechanism selects native unit geometry; the genuine partial48 path is preserved. Native same-value viewport writes, superseded-field ownership and failed read/write cleanup still use the existing retirement logic. A borrowed geometry change does not manufacture an owned pointer-restoration obligation after output has already completed.

## Required native data and transaction outcomes

All five downstream allocations are conservatively required before reduction. There is no newly proved inactive-consumer classifier, so a missing refraction/PipBoy/VATS allocation is not silently omitted. Host preflight must accept each format and alias association. Raw D24/stencil, RG16 motion, refraction14, PipBoy30 and VATS15 are expanded numerically with the same integer sample envelope and zero recovery resampling offset. Zero offset preserves the native jittered sample lattice; it is not a zero-jitter camera claim. Partitioned raw depth is never replaced by normalized vendor depth. Temporal processing retains its separate fractional/jitter mapping and real normalized depth domains.

The adapter consumes the exact landed56-byte `rsf_d3d11_reduced_frame` ABI1 header/export. No local public declaration or Host8/Game23 layout change exists. Missing export removes reduced quality availability and prevents ratio commitment; the standard Native control remains. RESERVE is resource-only. SEAL precedes temporal work. FINISH disposition/flags and the real attempted/temporal result come only from the host. Cancellation is limited to an unused reservation. Generic acknowledgement with NEXT_REDUCTION_ALLOWED is required before dropping the token and COM pins.

`full_output_resolved` and temporal `published` are distinct. Only an actual host-defined full image with current native ownership can suppress native TAA. A spatial finish marks the source refused for FG and does not claim SR success, valid temporal inputs, generated frames or lower latency. Existing final/WINDOW NO_FG facts remain sticky. A cleanup refusal preserves the defined-output distinction and blocks the next reduction rather than reverting Main to a reduced snapshot or clearing BUSY by inference.

Quiesce prevents new reservations and temporal evaluation. Already committed sources may still capture genuine raster facts and finish/retire on their owner while suspended. Missing producer, changed frame/view/renderer/context/window/resource or unsafe output setup is an explicit unresolved frame, not a spatial fallback success. There is no safe proof here for rescuing a recreated or foreign native owner, so its obligations remain retained. Permanent owner loss still requires a separately verified lifecycle solution.

## Static checks and remaining acceptance

The ignored `native-recovery-adapter-freeze-20261116.json` records the eight owned production hashes and static checks. Exact render/depth/resolution byte-guard bodies, NativePresent body, temporal capture/mapping/current functions, framebuffer reader and existing native state cleanup bodies match HEAD. The temporal resource reader differs only by the verified Main readable alias. CPU source/helper/tests are unchanged. Each original prepass, DR, Effects, RenderUI and Present call remains exactly once. Source checks establish RESERVE before ratio writes, SEAL before temporal admission and token-gated World reset; `git diff --check` passes.

No fake-resource branch fixture was added for the process-bound native reader. Root's shared owner owns real GPU transaction/device fixtures; the combined build and explicit hidden fixture results below now establish that narrower device scope. Native source/resource facts still require game validation. Required next observations include invalid temporal history/projection with valid raster, safe provider refusal, source refusal/quiesce, settings/quality/Off transitions, missing preflight resources, resize/owner loss, VATS/menu restoration and same-source before/after/HUDless/final Present captures. Native TAA history reset on return from resolved frames remains an independent proof/acceptance blocker. Full shader/buffer and moving visual coverage remain mandatory; no stationary frame, successful SDK return or source review closes them.


## Root combined build and explicit device checks

Authority: `.local/fallout4-research/reduced-recovery-built-20261116.json`, source head `e4f283df078e357e695f60970196f612c8f2e8a9`. Root's Windows Release VS2026 `verify-reduced-recovery-20261116.log` exited0:224 passed,14 skipped,238 total,113.57seconds test time; Rust fmt/Clippy clean. Device/contract durations were depth_scale0.04s, auxiliary_sr0.10s and d3d11_host0.07s. Handoff and camera contracts each passed0.01s. These results build the recovery adapter and native-cycle receipts; they do not supply a native recovered frame or a newly observed failed predicate.

| Built artifact | SHA256 |
| --- | --- |
| Runtime | `95CBC864DE2723D93E90F1651599F273ED5DFA970D89E763C452241AC49AC54C` |
| Fallout4 Entry | `9B5C79DF0B2DD2E5036B5DF1CC71BBCFF8773B1A124030B8746B40849F8D4E1F` |
| xinput1_3 carrier | `E4A809E7A7A7D802C7C83691727575FC288EB515130D54D60B608BDD7CF22C98` |

The explicit hidden recovery r4 build/run exited0. Its log `test-reduced-recovery-shared-sr-r4-20261116.log` retains actual temporal result0/flags75, real pre-submit failure result-1/flags77 with spatial recovery, no-camera/frozen-settings recovery, and suspended retirement. Cold RESERVE refusal before scene/pipeline readiness remained enforced. This is actual hidden device-fixture coverage, not injected native Fallout4 producer/camera/consumer validation. `test-recovery-hidden-marker-20261116.log` separately passed the plain hidden marker admission/fallback/keyed-bound derivation/failed-start/repeated-end check, with no physical scanout or latency measurement.

All three earlier fixture corrections and their failure logs remain retained:

- Initial `build-reduced-recovery-shared-sr-20261116.log` exited1 for C4456, a shadowed raster local; no fixture ran. The local was renamed.
- r2 build exited0, but `test-reduced-recovery-shared-sr-r2-20261116.log` exited1 because the fixture expected cold source1 RESERVE before scene/pipeline readiness. The fixture expectation was corrected; production readiness was not relaxed.
- r3 build exited0, but `test-reduced-recovery-shared-sr-r3-20261116.log` exited1 because reactivation omitted the required plan refresh. The fixture supplied that plan before retrying; no shipping policy changed.

The authority records unchanged shipping artifact hashes across these fixture-only corrections. r4 build/run and the hidden marker passed afterward. The shipping recovery batch is built and generic-device-tested but undeployed at this freeze. PID56312's user-rejected flicker/split-image defects and all three partial captures remain failed evidence, not repaired by these fixtures. Full native recovery capture, moving shader/buffer/near-depth coverage, TAA-history transitions, stable FG and end-to-end latency acceptance remain open. Native-only completion stays first; compatibility follows afterward.
