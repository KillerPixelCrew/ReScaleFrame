# AC7 native ownership on UE4.18

Scope: this reference records AC7 SR findings through 2 October. Later FG/Reflex, shared Unity
services and unresolved lighting work are summarized in
[current status](../../../../docs/current-status.md). SR acceptance alone does not validate those paths.

This is the working AC7 integration's evidence map, not a portable binary patch recipe.
Always read current guards and structures in source. Values below were checked against that
source on 2 October 2026; private layouts depend on the fingerprinted executable.

## Identity and reference

- Executable SHA-256: `c7da97f5f8a807d4f1264adbb074146fcffe9bdc2ffa98791b822cd28e558f4f`.
- Retained decrypted dump SHA-256: `fafd1db2808d32333caecf4056e8bcd676c2f07e4c1fabc0f270ef29f22d24db`.
- Stock source: `4.18.3-release`, `0a14a8d537a31ecc77488ced41dbaa0166612ef8`.
- Game build, manifests and source revisions: [engine.json](../../../../games/ac7/engine.json),
  [hook map](../../../../docs/research/ue418-hook-map.md) and
  [native manifest](../../../../docs/research/evidence/ac7-native-renderer-refactor-20261001.json).

## Ownership path and research anchors

| Owner | Runtime role | AC7 RVA |
| --- | --- | --- |
| Renderer constructor | Size matching main views before native scene allocation | `0x1113080` |
| Family size recomputation | Native family allocation dimensions | `0x19b6fe0` |
| Pre-visibility setup | Scoped temporal preparation and derived jitter | `0x112afa0` |
| Post-processing owner | Associate the actual velocity pooled reference and view | `0xffb900` |
| Composition context Process | Insert and execute reviewed SR/fallback graph | `0x10b3970` |
| Native graph registration | Register heap node for native Release retirement | `0xe93670` |
| Pooled output request | Native render-target allocation for graph output | `0x10b4870` |
| Parameter producer | Rebuild cached shader values from native matrices | `0x11346d0` |
| WaitForOutstandingTasksOnly | Join CPU view/command recorders before shared mutations | `0x12183c0` |
| Uniform move | Transfers reference and clears source; unsuitable for saving a shared slot | `0xde5cf0` |
| Scene-format policy | Native FP16 scene production before writes | `0x1095620` |

Exact expected entry/patch bytes remain in source and the hook map. Renderer preparation
preflights hooks after decryption; fingerprint recognition alone is not activation proof.

The plugin prepares before vendor activation. A declared primary view carries native family
frame, session, renderer/view/history keys, render/output rectangles and copied camera values
into leased RHI packets. It does not rediscover inputs at Present or use capture IDs as owners.
The runtime selects/evaluates the backend, handles region-local resources and returns output.

## Graph order and sizing

`native_renderer.cpp::make_plan` checks the reviewed chain through tonemap. The registered
SR node first queues a complete native bilinear fallback into an engine pooled target; a
vendor refusal leaves those pixels intact. It resizes reviewed consumers and rebuilds native
uniforms afterward. Post-SR FXAA aliases the input instead of filtering reconstructed colour again.

The latest matched bloom path finds `SceneColorHalfRes` through its descriptor and scene input.
Both that branch and tonemap input zero point to SR. Reconstruction uses preceding owned engine
exposure, avoiding a dependency cycle with the current exposure update. When that branch is
not matched, the node retains auxiliary-before-resize ordering. Do not delete these dependencies
or apply the same order to an arbitrary graph. See [stability corrections](../../../../docs/research/ac7-stability-20261001.md).

Active Ultra inputs at 1600x900 include DLSS 533x300 and XeSS 534x300. Engine allocation padding
is separate from those valid rectangles; quantizing active width to 536 previously caused refusal.
Native `PF_FloatRGBA` descriptors, scene format policy and DXGI enum values are different
namespaces. Read the actual mapping instead of assuming equal numbers mean equal formats.

## Shared view and resource safety

The captured hangar worker crash queued null PS view state. Saving the shared `view+0x10` or
`+0x18` with native move cleared it before publishing replacement. Copy/AddRef now retains the
old handle; move publishes from a local source. Generated uniforms remain held through queued
RHI consumption. Waiting for CPU recorders is distinct from waiting for GPU completion.

View cache `+0x1418` contains `0xcf0` meaningful shader-parameter bytes in the 4096-byte read
bucket. Scene extent `+0x208` and pooled depth `+0x60` are private verified layouts. The pure
parameter producer is used instead of rerunning `InitRHIResources` and dynamic draw setup.
Native pool releases return to the owner thread; quiescence rejects new producers and stop
waits for active scopes/queued references. See [native refactor](../../../../docs/research/ac7-native-renderer-refactor-20261001.md).

## UI and temporal data

AC7 converters and game-instance shared targets are distinct owners. Physical raster follows
output dimensions, while logical layout stays 1920x1080. Borrowed targets require validated
owner/class checks; preserve native glow/depth/repaint and mono/stereo behavior. Briefing's
separate-translucency path remains output-relative rather than a global scene downscale.

Current native packets declare decoded motion -> UV `(0.5, -0.5)` and camera inclusion separately.
The dense motion resolve distinguishes raw unwritten values from valid decoded zero. Projection
and clip-to-previous-clip use the matching view and current/previous jitter. These are AC7 packet
conventions, not an equation to impose on every UE4 producer or vendor SDK.

Terrain HS/DS and TrueSky projection can be the final position producers; checking only VS/PS
misses their jitter. Use [TrueSky guidance](../../truesky-render-integration/SKILL.md) for that branch.
Weapon/missile and moving-vehicle coverage still require colour/depth-to-velocity draw matching
and previous geometry. See [motion research](../../../../docs/research/ac7-motion-vectors.md).

## Validation boundary

The user accepted full-resolution UI, repaired crash/stability and clean wing/cloud overlap,
then accepted the optimized SR compatibility deployment. Native graph/scope WARP fixtures and
explicit RTX backend/lifecycle runs are separate evidence. Full FG/HUD-less surfaces, Reflex,
other GPU visual coverage and complete missing-object velocity are not established by this result.
