# Anomaly core mesh motion, ABI 1

This is an implemented source transform and a tested shader producer, not an
engine integration or a declaration preview. `tools/anomaly_temporal_shader.py`
consumes the user's pinned local r3 tree. Original shader bodies and their cloned
skinning functions are generated only beneath ignored `.local/anomaly/core*`.
No GSC/TAA implementation is embedded in tracked files.

## Run

From the repository root, with Python 3.12+, Windows SDK FXC and MSVC installed:

```text
python tools/anomaly_temporal_shader.py --source .local/anomaly/vanilla/shaders/r3 --output .local/anomaly/core-stage
python -m unittest discover -s tests -p anomaly_temporal_shader_test.py -v
python tests/anomaly_temporal_shader_verify.py --output .local/anomaly/core-handoff-new
```

Output names must be fresh. `RSF_FXC` and `RSF_VCVARS64` override discovered tools.
Missing assets, FXC, MSVC or a working D3D11 device are failures, not skipped tests.
The standalone verifier preserves DXBC, FXC assembly/reflection, original/new
hashes, compiler logs, a runnable device fixture and `device-report.json`.
Nothing launches or deploys a game. Do not copy this generated tree into a game.
The manifest is an injection allowlist, not blanket replacement permission.

The reviewed input is 449 files, corpus SHA-256
`fdc260e0608f259d876c4ae0f11a6f2d8c03c001cf1fe82548794af1c6826a67`.
The entire corpus is checked before transforms. A linked source/output component,
changed/added/missing input, escaped destination or existing destination is refused.
Staging verifies all written shader bytes before publishing `.rsf-core.json` and
rolls back newly created output on failure. Original assets are never written.

## Implemented paths and computation

- Twelve `deffer_base*.vs` roots, flat/bump, detail, HQ and LM wrappers.
- Six `deffer_model*.vs` roots with SKIN_NONE and SKIN_0 through SKIN_4.
  These also cover first-person meshes that actually use these roots. This is not
  a claim about every first-person material or the separate `hud3d` shaders.
- Forty non-ATOC `deffer_base*.ps` roots, including alpha reference, LM, detail,
  detail bump, ordinary/steep parallax and their HQ wrappers.

The current entrypoint executes the original shader body. It does not replace
its current transform, skinning, normal/tangent, material, position/depth packing,
parallax, discard or coverage logic. A wrapper adds the actual previous
homogeneous clip position. Static geometry uses its unchanged object vertex.
Model SKIN_0..4 execute a private clone of the local native skin functions with
only function names and the bone-array identifier remapped. They use the real
previous palette, not current deformation with a previous camera. Original
swizzles, index rounding, row addressing, weights and homogeneous-W restoration
remain in those generated functions. FXC eliminates unused cloned normal work.

SKIN_0 has no palette access. Its single bone was folded into the draw world
matrix by the engine. Supply the prior **bone-composed** WVP, not actor WVP.
SKIN_NONE uses the raw current vertex only when the caller explicitly certifies
that this draw is rigid and its vertex positions are history-stable. CPU/software
skinning without a previous vertex stream is deliberately invalid coverage.
There is no fake software-skin fallback.

The original `skin.h` stays byte-identical. Only four VS owners, four PS owners,
the shared IO declarations and two terrain exclusion prefixes are transformed.
Two terrain wrappers include a core owner; their exclusion prefixes keep their
compiled output byte-identical rather than accidentally adding an unpaired VS.
The twenty ATOC-only prepass PS roots also remain byte-identical. Neither group
is advertised as a motion producer.

## VS constants, slot b13

Bind a dedicated 3824-byte constant buffer to **VS b13**, with these exact bytes.
All scalar values are IEEE float32, including flags. Offsets are reflected in
actual FXC DXBC. Unused palette entries still occupy their declared bytes.

| Byte offset | Size | HLSL member | Meaning |
| ---: | ---: | --- | --- |
| 0 | 64 | `rsf_previous_wvp` | Four mathematical matrix rows, explicit `row_major float4x4`; `mul(matrix, vertex)` |
| 64 | 3744 | `rsf_previous_bones[234]` | 234 float4 rows, at most 78 affine bones, same native row indexing and mesh palette order |
| 3808 | 16 | `rsf_history` | X: matching prior draw exists; Y: prior palette exists; Z: SKIN_NONE is known rigid; W: ABI version 1 |

Only exact flag value `1.0f` is accepted. Static and SKIN_0 require X/W. SKIN_1..4
additionally require Y. SKIN_NONE additionally requires Z. Missing/unbound history
or an ABI mismatch produces invalid coverage. Do not set Y for a copied current
palette. X requires object, topology, deformation mode, view, frame and reset
identity to match the actual prior draw.

Previous WVP includes the correct previous model/world, view and projection for
that draw. It **may include previous jitter**, but the PS must then receive that
same previous jitter in UV units. An unjittered previous WVP requires previous
jitter UV zero. Do not memcpy the engine's column-major uploaded matrix bytes:
this buffer explicitly stores mathematical rows. Bone rows are already native
float4 rows, with no new transpose or palette reindexing.

First-person/HUD and scopes must have independent histories and their own prior
camera/projection, including any projection/FOV change and bone-composed world.
A world-camera previous WVP is not valid for a first-person draw. Viewport depth
compression changes depth testing, not the motion UV convention below.

## Shared VS to PS ABI

`common_iostructs.h` adds these fields under `RSF_TEMPORAL` to **both**
`v2p_flat`/`v2p_bumped` and `p_flat`/`p_bumped`, before existing fields:

```hlsl
float4 rsf_previous_clip : TEXCOORD12;
nointerpolation float rsf_valid : TEXCOORD13;
```

The clip value is the real prior homogeneous clip after prior deformation and
model/view/projection. It retains default perspective-correct interpolation
against the **current** raster clip W. Its XY/W ratio is taken in the PS.
`noperspective` on the clip would be wrong. Validity is per draw, so it is flat.
The core wrappers define `RSF_TEMPORAL` before `common.h`. Another family's VS
must define the same macro and fill both fields, or it must not be paired with
this PS. The foliage worker owns its own previous-state b13 layout and VS logic;
these IO fields and PS writer are shared, not foliage deformation assumptions.

## PS constants, slot b13

Bind a separate 48-byte constant buffer to **PS b13**. Stage slots are independent.

| Byte offset | Size | Member | Meaning |
| ---: | ---: | --- | --- |
| 0 | 16 | `rsf_viewport` | Current viewport origin XY in render-target pixels, reciprocal current viewport width/height in ZW |
| 16 | 16 | `rsf_jitter` | Current jitter UV in XY, previous jitter UV in ZW; normalized viewport UV, Y down |
| 32 | 16 | `rsf_pixel_control` | X = float ABI 1; other lanes reserved zero |

Current position comes from the actual rasterized `SV_Position.xy`, not packed
G-buffer Z or a second reconstructed current transform:

```text
currentUV  = (SV_Position.xy - viewportOrigin) / viewportExtent - currentJitterUV
previousUV = previousClip.xy / previousClip.w * (0.5, -0.5) + 0.5 - previousJitterUV
motionUV   = currentUV - previousUV
```

The prior clip is normalized by its own projection, so previous viewport pixel
extent is not part of this formula. Jitters must be expressed in their own
view's normalized UV units. Resize/view-rectangle changes require caller history
invalidation unless its prior-view mapping is known compatible. Do not apply a
second half-pixel offset: D3D11 SV_Position already uses pixel centers.

## MRT and coverage

- **PS SV_Target2 / OM RTV index 2**, `DXGI_FORMAT_R16G16B16A16_FLOAT`.
- Channels: `(current-minus-previous UV X, UV Y down, validity, 0)`.
- Clear every sample to `(0,0,0,0)` at the corresponding view/frame boundary.
  A valid stationary surface is `(0,0,1,0)`, not the clear value.
- Match the existing scene MRT dimensions, array slice and sample count. Bind
  slot 2 on every relevant scene target bind, not just once at frame start.
- Use the original depth/stencil test, rasterizer, sample mask and sample count.
  Disable blending on MRT2 and enable its full write mask. Preserve original
  blend/write settings on MRT0/1. Preserve/restore prior high-slot CBs and RTVs.
- The native pixel shader executes first. Discard affects all targets. Explicit
  `SV_Coverage` is copied unchanged. ATOC-only prepass bytecode is unchanged;
  its target/depth/stencil state must stay native. DX10.0 ATOC's main PS relies on
  that prepass coverage, not a new clip in this wrapper. Do not run the main pass
  with missing prepass depth/stencil and call its writes alpha-correct.
- Missing state, invalid ABI, nonfinite prior clip/jitter, prior W <= 1e-6,
  invalid render reciprocals or half-float velocity overflow write all-zero
  motion **without discarding native geometry**. The shader does not invent a
  zero vector with validity 1. Prior W rejection is conservative, not repaired
  camera motion.
- No hardware average resolve of velocity/validity is supplied. Select a valid,
  depth-consistent sample or implement a separately specified resolve. Never
  average invalid samples into a supposedly valid velocity.

Unknown/unsupported draws that occlude earlier motion must invalidate their
covered motion samples, not simply leave slot2 unwritten. A frame clear alone
does not prevent stale vectors from an underlying previously drawn surface.

## Cache and engine metadata boundary

The handoff contains the replacement source, real original/new DXBC hashes,
semantic signatures, resource bindings and constant offsets for each tested
permutation. Original resource names, hardware bindings and constant offsets
are checked for preservation; the additional cbuffers are explicitly b13.
VS input layout semantics/types/masks are unchanged. No vertex-buffer or index-buffer
conversion is introduced. Keep the draw's original IA index format, stride and
offsets; neither an extra previous-vertex stream nor SV_VertexID lookup is added.
The device fixtures use non-indexed draws, so they do not independently validate
engine R16_UINT/R32_UINT indexed submission. Packed bone indices retain their
original native attribute formats and decoding, including D3DCOLOR XYZ swizzles.
Original outputs survive; VS gains TEXCOORD12/13 and PS gains only SV_Target2 plus
its existing coverage.

Replacement must happen before the engine consumes bytecode for **all** of
CreateShader, D3DReflect/constant tables and VS input-signature extraction/cache.
The reviewed source correspondences are `r4.cpp:977-1051` for PS/VS creation and
metadata, `1820-1904` for cache-hit/source-compile paths and `1170-1193` for
includes. They are not shipping executable hook addresses. Device creation
interception alone is insufficient. D3DCompile interception alone misses cache
hits. Keep the returned bytecode pointer/size and lifetime coherent throughout
that boundary and publish engine shader/metadata caches atomically per variant.

Use `.rsf-core.json`'s `cache_namespace` as a namespace component. It includes
source corpus, transformed file hashes, tool hash, first-party HLSL hashes and
ABI. Also key by root, entrypoint, exact ordered/defined macro set, target,
compiler identity/version, compiler flags and engine's original cache identity.
The checked fixture permutations are listed in `fxc-report.json`; they are not
a census of live engine permutations. Refuse unknown layouts/roots/permutations,
slot collisions, mismatched original bytecode hashes and bound HS/DS. Do not
reuse stock cache entries as temporal entries, strip reflection, or update only
one side of a linked shader interface. Bind history after native per-draw uploads
so the engine cannot overwrite it, while retaining the rest of its native CBs.

The parent integration still owns actual per-draw prior state, view history,
reflection/cache hooks, temporal MRT allocation/binding/resolve and retirement.
Those are not implemented or claimed here. `runtime_ready` remains false.

## Executed verification and limits

On 11 November 2026, the standalone Windows fixture completed **458 FXC old/new
pairs**: 96 changed VS permutations, 340 changed PS permutations, twenty
byte-identical ATOC-only prepasses and two byte-identical excluded terrain VS.
The matrix includes all supported root filenames, all six model skin modes,
static-sun VS variants, non-MSAA and 2/4/8-sample PS variants, plus DX10.0 ATOC,
DX10.1 ATOC and explicit sample alpha-test branches. Existing native compiler
warnings remain recorded. Every pair's actual FXC signature and cbuffer/resource
reflection is inspected, not inferred from declarations.

MSVC `/W4 /WX` compiled the first-party device fixture. A real D3D11 **hardware**
device executed the FXC game shader programs. It completed 192 VS checks
(unchanged original outputs plus independent CPU current/prior clip formulas,
world and distinct first-person prior projections), 860 native/new PS material
and alpha/coverage comparisons, 776 missing-prior/invalid-W checks and 340 valid
stationary/jitter-cancellation checks. Skin fixtures use unequal bone matrices,
nontrivial weights, packed index swizzles and separate prior deformation.
PS fixtures use unequal vertex clip W, nonzero current/prior jitter, actual FP16
MRT2 and per-sample GPU readback at 1/2/4/8 samples. They execute native/new
stages in isolation with first-party inputs, not a captured engine draw.

Fixture corrections retained for interpretation: synthetic VS/PS linkage must
preserve signature register order, native packed hemi/material W may be a NaN
bit pattern rather than an invalid numeric result, and DX10.0 ATOC main shaders
do not independently reject alpha without their native prepass. These are not
excuses to alter the original shader's position/coverage semantics.

Remaining family gaps: terrain, trees/details/LOD, particles, water, sky/clouds,
forward/transparency/distortion/emissive/projected PDA, tessellated domain output,
software-skinned SKIN_NONE, CPU-streamed dynamic meshes/decals and separate hud3d.
Foliage augmentation is separate work. No game launch, live cache interception,
SR/FG dispatch, full scene coverage, deployed shader or visual acceptance is
claimed. Built and hardware-fixture-tested is not game-tested.
