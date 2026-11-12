# Real foliage temporal shader augmentation

This is a local-source transformation, not a deployment or a ready game renderer.
Tracked code contains only first-party injection/wrappers and identifier mappings.
Licensed expressions are read from the pinned local corpus and cloned only into
untracked staging. No GSC or TAA implementation is redistributed.

## Use and ownership

```text
python tools/anomaly_foliage_shader.py --source .local/anomaly/vanilla/shaders/r3 --output .local/anomaly/staging/foliage-new
python -m unittest discover -s tests -p anomaly_foliage_shader_test.py -v
```

Python 3.12+ is required. The CLI accepts only the reviewed 449-file corpus and a
new, nonlinked destination under `.local/anomaly/staging/`. It hashes the complete
corpus, verifies staged bytes against the original immutable snapshot, and removes
a failed new stage. Its content-addressed manifest pins this tool and the injected
header. That namespace is only the foliage recipe: the native composite cache must
also pin shared core ABI, compiler, target, macros and options.

`transform(source)` returns `(patches, report)`. `patches` has six VS owner assets
and `rsf_anomaly_foliage.h`. The tree wave/still flat/bump owners and detail
wave/still owners are transformed. Their detail/HQ wrappers inherit the changed
owners. The recursive `deffer_tree_s_bump_d.vs` remains blocked, never repaired.

The standalone foliage stage intentionally does **not** modify shared IO or pixel
shaders. Parent integration must merge the returned map with the core worker's
shared IO and pixel changes. `tools/anomaly_temporal_shader.py` and
`games/anomaly/shaders/core/` are separately owned. Tests merge the actual core
`shared_io` and `pixel` transformations into an isolated local fixture, not core
source files and not a pretend contract stub.

## Source transform

The exact current main body is preserved behind `rsf_foliage_current`. A local
copy, `rsf_foliage_previous`, changes only positional-state identifiers to the
reserved previous constants. A first-party main evaluates the unchanged current
path and, only when history is valid, the previous path. It fills the common
previous clip and validity interpolators without changing current `hpos`, normal,
texture coordinates, hemisphere or other shading outputs.

This deliberately preserves the actual `USE_TREEWAVE` logic: when defined, the
original wave owner zeroes its wave result. It is **not** permission to enable
wave by reversing the branch. Still owners contain independent full main bodies
and are transformed independently. Detail `TEXCOORD0.w` remains a **row offset**,
not an instance number: 61 groups of four rows, valid aligned offsets 0..240.

## Shared motion ABI

- `RSF_TEMPORAL=1` before the original common include selects the core IO fields.
- Actual current core spelling: `float4 rsf_previous_clip : TEXCOORD12` and
  **`nointerpolation float rsf_valid : TEXCOORD13`**, not `rsf_previous_valid`.
- Previous clip uses normal perspective interpolation, never `noperspective`.
- Core PS writes `SV_Target2`, `R16G16B16A16_FLOAT`: `(current-minus-previous
  normalized UV, positive Y down, validity, 0)`. Clear all four channels to zero.
  Validity zero is unavailable motion, not a valid stationary vector.
- Foliage previous projection is **unjittered**. On foliage draws, bind core PS
  **`rsf_jitter.zw=(0,0)`**. Its `.xy` still contains actual current UV jitter.
  Core also permits jittered previous projections for other families; blindly
  reusing their previous jitter on foliage would subtract jitter twice.
- Tree prior projection corresponds to original m_VP. Detail must reproduce the
  effective original m_WVP acting on its already world-transformed position.
  Do not assume an identity world matrix without actual capture evidence.

## VS b13, family-specific ABI 1

The engine must reserve VS slot13 and bind this 5056-byte buffer after all engine
constant uploads. Core-family b13 is incompatible with foliage b13. PS slot13 is
independent and holds the core viewport/jitter/control constants.

| Byte offset | Field | Bytes |
| --- | --- | --- |
| 0 | row_major float4x4 rsf_previous_clip_from_world | 64 |
| 64 | row_major float3x4 rsf_previous_tree_xform | 48 |
| 112 | float4 rsf_previous_consts | 16 |
| 128 | float4 rsf_previous_wave | 16 |
| 144 | float4 rsf_previous_wind, tree wind / detail dir2D | 16 |
| 160 | uint4 rsf_foliage_history | 16 |
| 176 | float4 rsf_previous_rows[244] | 3904 |
| 4080 | float4 rsf_previous_item_state[61] | 976 |

Matrix storage is row-major, multiplied as `mul(matrix, column_vector)`. There are
no new SRVs, samplers, UAVs, vertex attributes or textures in this VS augmentation.
Original constant buffers remain intact. FXC reflection verifies these offsets;
tree variants may optimize array use away while retaining the declared layout.

`rsf_foliage_history` is uint4 `(complete_draw_history, current_detail_count,
ABI=1, reserved=0)`. The complete flag must be exactly1, and includes matching
prior camera/view, mesh/deformation route and instance history. For detail the
count is the actual current batch count, at most61. Unsupported draws receive an
initialized zero buffer, never the preceding draw's binding.

Each item state is float4 `(complete_identity_matched_history, previous_effective
scale, previous_fade, reserved=0)`. Valid requires flag1, finite positive scale,
finite fade in (0,1], and reserved0. Prior instance rows and deformation constants
must be finite. Unaligned/out-of-range indices, missing items and bad metadata
invalidate before previous evaluation. Previous clip rejection in the shared PS
also protects against overflow/nonfinite projection results.

Rows0..2 **already contain** previous effective scale and fade, and row3 contains
actual prior colour/hemi. Do not multiply fade or scale a second time. Fade/scale
metadata asserts complete capture, rather than replacing actual prior rows.

## Native hook expectations and source evidence

Read-only source correspondence, not shipping-binary hook verification:
`D:/xray-monolith`, revision `028ccf5f5189b393396480076b11e40852ba3ecd`.

- `FTreeVisual.cpp:137-151` supplies the actual visual transform, consts, computed
  wave and wind. Capture those selected values, not a shader reconstruction from
  guessed time. ST `214-218` and PM `255-267` select the rendered geometry/LOD.
- `dx10DetailManager_VS.cpp:50-101` calculates the two wind/wave routes and still
  route. `104-147` selects visible lists and binds deformation constants.
  `174-195` emits effective per-item rows; `196-230` flushes and resets batches.
- `DetailManager.cpp:335-372` applies fade through `scale_calculated` unless
  rsNoScale, rebuilds visibility lists and may select a different route.

Native capture must retain stable item owner/visual/generation lifetime identity,
actual effective scale/fade, actual selected deformation route, and complete
previous rows. **Remap previous histories to CURRENT batch slots by those stable
identities. Current batch index, upload address and previous upload order are
never identity.** This shader consumes remapped rows but does not establish or
prove native identity capture.

Keep main/scope/view histories separate. New/recycled items, missing previous
state, cuts/reset, incompatible routes/meshes and unmatched LOD/topology invalidate.
Keep uploaded buffers immutable through queued draws. Replace bytecode before
engine constant reflection and input-signature caching, on cache-hit and compile
paths. Allocate/bind/clear motion slot2 with matching depth/sample count for every
relevant scene target binding. Refuse unsupported coverage, including the
recursive root; unwritten or stale vectors cannot count as valid motion.

## Executed validation and limits

Six focused tests pass on the inspected Windows machine. FXC SDK10.0.26100 compiles
13 original/changed tree/detail root pairs, all with changed DXBC and identical
input signatures. Actual shared core PS augmentation compiles four flat/bump
opaque/aref pixel roots with SV_Target2. Includes/geometry/coverage are not invented
by the fixture.

The C++20 fixture builds using native Windows g++ with `-Wall -Wextra -Werror -O2
-static`, then uses hardware D3D11 on Intel(R) UHD Graphics, feature level11.0.
Seven real-asset configurations execute: tree flat/bump, USE_TREEWAVE=1, still
flat/bump, detail wave/still. Actual original VS execution with current constants
matches current clips. Separate original VS execution with prior constants
matches previous clips. Detail offsets4 and240 exercise distinct groups, including
the last group, with changed scale, rigidity, wind and wave.

Seven triangle fixtures use transformed real VS plus actual core-transformed real
PS to write RGBA16_FLOAT MRT2. Native MRT0/1 remain unchanged. Numeric expectations
come from separately read interpolated prior clips; actual MRT2 must match and be
nonzero/valid. Missing history is also rasterized and produces all-zero motion
coverage while preserving native shading. Invalid detail fade/scale/rows and row
indices are exercised on device. This is rendered fixture evidence, not just
helper math or an inventory.

Local reports: `.local/anomaly/foliage-compile-report.json` retains actual compiler
reflection/signatures/resources and bytecode hashes;
`.local/anomaly/foliage-device-report.txt` retains device and numeric output.
Generated licensed source, DXBC and assembly remain temporary/untracked.

No game draw/capture, native hook implementation, dense scene acceptance,
MSAA/ATOC raster coverage, nonzero-jitter device fixture, SR or FG is claimed.
`runtime_ready` stays false. Native `eng/verify.ps1` is left to parent integration
because this worker must not change or use shared build integration.

FXC pitfall: `isfinite` on dynamic previous-row loads crashes this SDK compiler
internally in this combined VS. Differential local probes isolate that lowering;
the first-party row check uses IEEE exponent bits to reject NaN/Inf, and the GPU
NaN-row fixture passes. Signed-index changes and branch attributes did not fix it.
The fixture GS input is built from the full reflected VS output signature; sparse
semantic-only GS declarations previously produced mismatched-register values and
must not be used as verification evidence.
