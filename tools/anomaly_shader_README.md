# Local Anomaly shader inventory and bounded include previews

This is source-inspection and patch-planning tooling, **not a temporal shader
implementation**. It does not launch or deploy to a game, modify engine source,
produce motion vectors, or replace an executable. Python 3.12 or later is required.

All proprietary input and generated shader trees stay under `.local/anomaly/`.
The tracked header and synthetic fixtures are independently authored first-party
material. No GSC or TAA addon shader implementations are redistributed here.

## Commands

Run from the repository root, using a fresh output name each time:

```text
python tools/anomaly_shader_tool.py inventory --source .local/anomaly/vanilla/shaders/r3 --output .local/anomaly/inventory.json
python tools/anomaly_shader_tool.py plan --source .local/anomaly/vanilla/shaders/r3 --output .local/anomaly/patch-plan.json
python tools/anomaly_shader_tool.py stage-include --source .local/anomaly/vanilla/shaders/r3 --output .local/anomaly/staging/constants-preview
python -m unittest discover -s tests -p "anomaly_shader_test.py" -v
```

Inventory records original byte hashes, family/stage, include dependencies,
entrypoint input-structure/semantic candidates, lexical conditional axes, texture
names, tonemap references and Lua material texture bindings. It strips HLSL and
Lua comments and resolves includes case-insensitively from the supplied r3 root,
matching the first lookup in the engine includer. External shader-root fallback
is reported missing, not guessed. This is **not preprocessing, bytecode reflection,
or a census of live engine permutations**. Conditional structure fields and
macro aliases are intentionally retained as candidates.

`plan` additionally checks the reviewed corpus fingerprint and emits every
identified scene-geometry wrapper with its prior-state requirements, skin modes,
conditional axes and interface/engine prerequisites. Other stage roots remain
explicitly unclassified. Shadow-only roots are excluded from scene motion.

`stage-include` validates the entire original corpus digest and file count, the
patched file digest, the exact anchor-line digest, and the first-party include
digest. It inserts one declaration-only include after the pinned common-header
guard. Every other source byte is preserved. The tool refuses changed/added/
missing assets, mismatched anchors, bad include names, pre-existing destinations,
linked output paths and destinations outside `.local/anomaly/staging/`. Errors
during writing remove the newly created preview. Readback verifies every staged
asset before writing `.rsf-stage.json`.

The header does not assign a hardware constant-buffer slot. No previous-state
uploads or output semantics exist yet. Unused declarations can be optimized away.
The manifest says `runtime_ready: false` and `motion_output_implemented: false`.
Do not deploy the preview as a temporal implementation.

## Reviewed input

Inspected on 11 November 2026. Local reference engine revision:
`028ccf5f5189b393396480076b11e40852ba3ecd`.
These are source correspondences, not verified shipping binary hook addresses.

The supplied r3 subtree contains **449 files**, not 948. The archive-wide extracted
file count must not be used as the r3 fixture count. Canonical corpus hashing uses
SHA-256 over UTF-8 JSON containing sorted `[relative_path, sha256]` pairs, compact
separators and ASCII escaping:

```text
fdc260e0608f259d876c4ae0f11a6f2d8c03c001cf1fe82548794af1c6826a67
```

The raw `common.h` SHA-256 and the exact second-line anchor hash are pinned in
`anomaly_shader_vanilla_profile.json`; neither original text nor snippets are
embedded in the profile.

There are no unresolved quoted includes within this r3 tree. One unguarded source
recursion is real: `deffer_tree_s_bump_d.vs:1-2` defines `USE_TREEWAVE` and includes
itself. The plan blocks that root rather than silently repairing it. An engine
cache might supply bytecode for such a name; its live use is not established.

## Colour and tonemap evidence

Paths below are relative to the r3 shader root unless prefixed with an engine
source filename.

- `combine_1.ps:68-77,127-150`: obtains albedo, reconstructed normal/position,
  accumulated lighting and hemisphere terms; combines diffuse and specular,
  then applies distance fog. **The `color` value after line 144 and before the
  tonemap call at line 150 is a genuine shader-local pre-tonemap scene value.**
  It is not already available as a complete stock FP16 scene texture.
- `combine_1.vs:21-22` supplies the exposure scale through the interpolator.
  `common_functions.h:21-29` applies exposure, a nonlinear low-range mapping and
  a separate scaled high channel. `common_defines.h:11` sets the high-range
  divisor to 9. The low/high outputs are not interchangeable with raw radiance.
- `r4_rendertarget.cpp:431-444,464` allocates generic low/high targets as LDR8,
  including the MSAA equivalents. Upcasting them cannot recover lost values.
  The FP16 accumulator is lighting, not the completed diffuse/specular/fog scene.
- `sky2.vs:28-29` prescales sky colour; `sky2.ps:24-32` performs its own nonlinear
  mapping. `clouds.vs:28-38` scrolls UVs and applies exposure before its pixel
  shader. A raw-scene side output from `combine_1` alone misses both paths.
- `r4_rendertarget_phase_combine.cpp:83-110,118,257-280` draws sky/clouds first,
  then stencil-limited scene combine. `blender_combine.cpp:19-20` uses inverse
  source-alpha/source-alpha blending. The combine shader's sky-fog alpha must
  therefore be preserved in any separate FP16 composition design.
- Forward geometry follows at phase-combine lines 284-303, then volumetric combine
  at 306-309. `combine_volumetric.ps:29-45` separately tonemaps its light contribution.
  Converting only the deferred combine does not make these later writes linear.
- `combine_2_naa.ps:63-75` reads generic0, adds bloom and can apply another optional
  tone curve before image corrections. The AA path differs:
  `combine_2_aa.ps:67-76` filters generic0 and combines bloom. Final-combine names
  do not establish a pre-tonemap input.
- `r4_rendertarget_phase_combine.cpp:314-324` hardcodes UNORM resolves before bloom.
  A target format change must also address resolves, copies, bloom/exposure,
  forward materials, sky, clouds and volumetrics. No such complete change is
  implemented by this tool.

## Depth and output packing

- `gbuffer_stage.h:5-11,95-109`: target0 stores packed normal in XY, **linear view Z
  in Z**, and encoded hemisphere/material in W; target1 stores albedo/specular.
  This is not device depth, and W is not a homogeneous coordinate.
- `gbuffer_stage.h:119-128` samples or loads target0 per MSAA sample and reconstructs
  view XY from view Z and `pos_decompression_params` using pixel coordinates.
  `INTERNAL_AMD` changes the normal packing branch, not the depth channel.
- `deffer_base_flat.ps:34-39` and other geometry pixel shaders apply a normal-based
  virtual-height offset before packing. `common_defines.h:9` supplies its scale.
  Packed depth is therefore not always the exact interpolated mesh-surface Z.
- `r4_rendertarget_phase_scene.cpp:14-22` clears position to zero and hardware depth
  to one. `common_defines.h:21-23` leaves `SKY_WITH_DEPTH` disabled; the normal
  sky helper uses near-zero view Z as sky. An R32 conversion must define a sky/
  invalid convention, not treat the packed target as normalized hardware depth.
- `r__dsgraph_render.cpp:502-535,595-621` changes projection for first-person HUD
  geometry and calls `rmNear()`. `r4.cpp:834-840` compresses its hardware viewport
  depth to 0 through 0.02. Its packed view Z and vectors need the correct
  view/projection identity. World-camera reprojection alone is not correct.

## Geometry and permutation census

The generated plan lists exact filenames, entrypoint owners and all observed
conditional axes. The identified roots are:

| Family | Roots | Previous inputs required |
| --- | ---: | --- |
| Deferred static `deffer_base_*` | 12 VS | Previous object-to-clip; preserve flat/bump, detail, HQ and LM variants |
| Terrain `deffer_impl_flat{,_d}` | 2 VS | Same transform contract; both include the bumped implementation and enable detail |
| Deferred model `deffer_model_*` | 6 VS | Each has SKIN_NONE and SKIN_0 through SKIN_4; per-object and deformation history |
| Tree wave / still `deffer_tree_*`, `deffer_tree_s_*` | 6 + 6 VS | Prior instance transform; wave variants additionally need previous wind/wave phase; one still-wrapper root is blocked |
| Detail grass wave / still | 1 + 1 VS | Prior per-instance rows, correct instance identity; wave variant additionally needs prior wave/dir2D |
| LOD billboard | 1 VS | Previous CPU-generated billboard vertices and interpolation factor, not camera history alone |
| Deferred particles | 1 VS | Prior particle/vertex data and matching alpha/sample coverage |
| Forward model | 7 VS | All six skin modes; separate transparent/distortion/emissive policy |
| Forward particles | 2 VS | Prior dynamic geometry and explicit transparency coverage |
| Water | 6 VS | Previous time/parameters for positional deformation, separate animated-material handling |
| Sky / clouds | 1 + 1 VS | Sky rotation/projection and valid coverage; cloud geometry and scrolling content policy |
| Tessellation | 1 HS + 1 DS | Previous surface after PN/height displacement at matching barycentrics, not pre-domain VS clips |

There are 55 identified geometry-stage roots including the blocked recursive root.
The plan separately lists 44 unclassified roots and 9 excluded shadow roots. Those
unclassified entries include postprocess and volume infrastructure as well as
additional geometry/attached-UI candidates; they are **not coverage claims**.
`hud3d.vs`, CPU-streamed decals, effects/sun and fluid paths still require pass-role
and coverage review. No live permutation census or runtime motion coverage is
claimed.

Model deformation details matter:

- `SKIN_0` has no palette lookup in `skin.h:83-97`. It is the single-bone path:
  `SkeletonX.cpp:67-73,222-227` folds that bone into the world matrix. History must
  retain the composed draw transform, not merely actor world.
- `SKIN_NONE` can also mean software skinning. `SkeletonX.cpp:60-65,196-241` and
  its software renderer show that prior CPU-deformed vertices can be necessary.
- `SKIN_1` puts its index in normal W; `SKIN_2` uses TEXCOORD0 Z/W plus normal-W
  weight; `SKIN_3` adds a BINORMAL-W index and TANGENT-W weight; `SKIN_4` has four
  TEXCOORD1 indices and three weights in normal/tangent/binormal W. Preserve the
  original swizzle and row indexing. `sbones_array` has 234 float4 rows, not 234
  bone matrices (`skin.h:6-58,99-247`).
- `deffer_tree_flat.vs:21-29` and its bumped counterpart zero the wave result when
  `USE_TREEWAVE` is defined. The macro name is not permission to invert this logic.
- Detail instance data is indexed through TEXCOORD0 W and contains 61 groups of
  four rows (`deffer_detail_{w,s}_flat.vs`). Current batch position is not stable
  identity across frames.
- `uber_deffer.cpp:100-145` chooses PN, height-map, or combined tessellation, with
  LM/detail/detail-bump axes. `dx11/tess.ds:39-90` changes the final surface after
  interpolation. VS-only output augmentation is insufficient.
- Alpha-test/ATOC/MSAA wrappers change coverage and sometimes the pixel return
  shape (`deffer_base_aref_flat.ps:1-23,29-64,91-101`). Motion must match depth and
  sample coverage; a declaration-only include does not accomplish this.

## PDA material consumer

`pda_overlay.s:2-11` selects `model_def_lplanes` / `pda_overlay`, binds `$user$ui`
to texture `s_pdascreen`, uses `smp_base`, enables depth testing but not depth
writes, uses source-alpha/inverse-source-alpha blending, alpha reference zero,
strict sorting and distortion registration. `pda_overlay.ps:27-53` samples that
texture, modifies content with time-dependent glitches and applies `pda_params`
to opacity/intensity. `model_def_lplanes.vs:15,28-49` uses the draw WVP and all six
skin entrypoints. It is a projected material, not an output-space HUD texture.
Surface motion and scrolling/glitching UV content require different treatment.

## What the DLL-side integration must still hook

1. Coherent replacement bytecode before the engine's creation/reflection boundary.
   `r4.cpp:1015-1051` creates the VS, reflects its constants, extracts its input
   signature and caches that signature. Pixel reflection is at 977-1001; HS/DS
   delegate through the corresponding creation helpers at 1139-1145. Replacing
   only the D3D device creation call leaves other engine metadata stale.
2. Both cache-hit and compile paths. `r4.cpp:1820-1904` loads CRC-checked precompiled/
   local cached bytecode before reaching D3DCompile. The cache identity must include
   source corpus, injected ABI/include, compile options and target. A D3DCompile
   interception alone misses hits. The include resolver is at 1170-1193.
3. Per-draw previous-state binding after correct object, skeleton, instance and
   view selection. Track main, scope and first-person histories separately, with
   invalidation on cuts, missing identity, deformation changes and resets.
4. Scene MRT allocation/binding and clears. `phase_scene_begin:40-44` currently
   binds two scene outputs. `r4_rendertarget.cpp:32-120` has overloads that either
   leave slot2 untouched or explicitly set/clear it. Binding an RTV once is not
   enough. Update VS/HS/DS/PS interfaces and the engine cache together; preserve
   alpha masks and sample counts. Sky/forward/volume coverage and transparent
   composition remain independent work. No motion sign, units, validity encoding
   or resolve policy is established by this prototype.

## Executed verification

The eight focused unittest cases run synthetic failure tests, real-fixture hashes/
semantics/PDA inventory checks, CLI staging/readback and an optional Windows SDK
FXC regression. On this inspected machine FXC 10.0.26100.0 compiled **130 original/
staged variant pairs** with byte-identical DXBC. This covers the planned VS roots
except the recursive source, six skin modes where present, twelve HS/DS detail/
tessellation combinations and the PDA pixel shader. Existing compiler warnings
remain and match between each pair. It does not cover all pixel/MSAA permutations.

Byte identity is intentional: unused prototype constants disappear. It verifies
that include insertion does not change these compiled shaders, **not that temporal
inputs exist or are bound**. No GPU draw, SR, FG, game launch or visual acceptance
was performed. Local JSON outputs retain per-variant byte hashes and warning counts.
