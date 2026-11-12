# Anomaly: motion vectors through the tessellation pass

Status (13 November 2026): built and fixture-tested on the RTX 4070. Not deployed and not game-tested. Rendering-readiness
flags stay 0.

## Problem

With `r4_enable_tessellation on`, X-Ray draws the HQ element of every PN material as VS -> `DX11\tess.hs` ->
`DX11\tess.ds` -> PS (`uber_deffer.cpp:100-144`, `r_TessPass`). HQ applies within `r__dtex_range` of the camera
(`r4.cpp:68-76`), so near characters use this path and distant ones do not. The motion stage used to swap only its
VS/PS pair. The stock domain shader never wrote the motion PS's `TEXCOORD12`/`13`, so the PS read validity 0. DLSS then
fell back to camera-only motion, which caused heavy ghosting on near moving characters. Section 13 of
`anomaly-sr-seam.md` describes the diagnosis. The ghosting worker then made these draws refuse the swap, so their motion
was defined but invalid.

The user's shader cache (`appdata/shaders_cache/r4`, read-only listing) holds exactly these tessellation permutations:

| Stage | Cached permutations |
| --- | --- |
| VS | `deffer_model_bump(tess_pn,)_0.._4`, `deffer_model_bump(tess_pn,use_tdetail,use_tdetail_bump,)_2/_3` |
| HS/DS | `dx11\tess(tess_pn,)`, `dx11\tess(tess_pn,use_tdetail,use_tdetail_bump,)` |
| PS | `deffer_base_bump(tess_pn,)`, `deffer_base_bump(tess_pn,use_tdetail,use_tdetail_bump,)` |

None uses `TESS_HM` (displacement) or a static `deffer_base_bump` VS with tessellation. Every one of these is a model
(character, NPC gear or first-person) permutation.

## Design

The whole chain is swapped per draw, and only when all four stages have verified variants and every interface links.

- **Shader tool** (`tools/anomaly_temporal_shader.py`, merged by `tools/anomaly_motion_deploy.py`):
  - `common_iostructs.h`: under `RSF_TESS_VERTEX`, `v2p_bumped` gains `float4 rsf_previous_position : TEXCOORD14` and
    `float3 rsf_previous_normal : TEXCOORD15`, after every native field and before `SV_Position`.
  - Under `RSF_TESS_CONTROL`, `p_bumped` gains the same two fields at its end. The hull input (`p_bumped`, no
    `SV_Position`) therefore stays a register prefix of the VS output. The domain output (`v2p_bumped` without the extras)
    keeps the PS interface unchanged.
  - Motion VS (`deffer_model_bump.vs`, `deffer_base_bump.vs`; `RSF_TESS_VERTEX` is set when `TESS_PN` or `TESS_HM` is
    defined): writes the previous object-space position (`rsf_prev_skinning_n(v).P`, or `v.P`) and normal. The normal is
    scaled as the native body scales it: `2 * N` for models, `unpack_bx4` of the swizzled `Nh` for static geometry.
  - `dx11/tess.hs`: copies the four RSF fields per control point, next to the native field copies.
  - `dx11/tess.ds`: computes the previous surface point at the same domain location.
    - The uvw is the one left after the native edge snap.
    - The previous control points and normals are transformed with `rsf_previous_wv`.
    - The native `ComputePNPatch`/`ComputePatchVertex` rebuild the previous PN patch, plus the carried patch difference
      described below.
    - The native `ComputeDisplacedVertex` is applied along the previous normal.
    - The native edge rule applies: `minc == 0` keeps the flat point.
    - The result is projected with `rsf_previous_projection`, and the shader writes `TEXCOORD12`/`13`.
    - Validity is 1 only when all three control points are valid and the DS history is valid.
  - The tool inserts first-party statements after exact native anchors and refuses when an anchor matches zero or more
    than one time. Generated trees stay under `.local/anomaly/`.
- **First-party header** `games/anomaly/shaders/core/rsf_core_tess.h`:
  - DS `b13` `RSFTessPrevious` (144 bytes): `rsf_previous_wv` @0, `rsf_previous_projection` @64, and `rsf_tess_history`
    @128 = (valid, 0, 0, ABI 1).
  - The carry helpers.
- **Loader** (`shader_loader.*`):
  - New families `tess_hull` and `tess_domain`; manifest stages `hs`/`ds`; root names may have one subdirectory
    (`dx11/tess.hs`).
  - The reflection includes the patch-constant signature.
  - `compatible('h'/'d')`:
    - Stock control-point fields are kept (registers may shift) plus exactly the RSF fields.
    - Hull: `TEXCOORD12` xyzw, `13` x, `14` xyzw and `15` xyz in and out. Domain: those four in, `TEXCOORD12`/`13` out.
    - The patch-constant signature is identical.
    - The hull adds no binding; the domain adds only `RSFTessPrevious` at `b13`.
  - `tess_chain_linkable(vs, hs, ds, ps)`: each control-point interface links by name, index, register and mask, and every
    domain patch-constant input is a hull patch-constant output.
  - The byte-exact original recompile and the disk cache are the same as for VS/PS.
- **Hook sites** (`motion_profile.h`): `CreateShader<SHS/SDS>` (`ShaderResourceTraits.h`) calls the same
  `CRender::shader_compile`, which calls the already detoured `create_shader` (0x838de0) with `hs_5_0`/`ds_5_0`. The caller
  frame, the defines and the recompile proof are therefore those of VS/PS, and no new detour is installed. New guards
  are checked at install:
  - The dispatch at 0x839123 (`cmp al,'h'`, which calls `create_shader<SHS>` 0x835fb0, else `create_shader<SDS>` 0x835f00).
  - Both templates through their `mov [rcx+10h], rax` object store (`SHS::sh`/`SDS::sh` at `shader_object_offset`).
  - Both `CreateHWShader` wrappers through their `ID3D11Device` call (+0x80 `CreateHullShader`, +0x88
    `CreateDomainShader`).
  - `tests/anomaly_motion_evidence.py --verify` re-derives all of these from the installed executable, including the
    `RC_dest_hull`/`RC_dest_domain` constant-table destinations.
- **Draw time** (`motion_stage.cpp on_flush`):
  - With HS/DS bound and no GS, the stage looks up variants for the bound VS, HS, DS and PS. The VS must be a core
    family. The chain linkage is cached per stock 4-tuple.
  - The core history decision (`decide_core`) also returns `previous_wv` (prior V * W, bone-composed for SKIN_0) and
    `previous_projection`.
  - VS `b13`, DS `b13` and PS `b13` are uploaded, then the four variants are bound. The draw detour restores the stock
    VS, HS, DS and PS.
  - Any missing variant, linkage failure or geometry shader keeps the stock shaders and the invalidation replay.
  - Counters: `Anomaly motion tessellation chains swapped/no_variant/unlinked/geometry_shader/pack_refused`, and the
    `tessellated` class reported as valid/invalid instead of unpaired.

What animates in the tessellation stages: nothing over time. The tessellation factor `triLOD` is a per-draw constant,
and both positions are evaluated at the same uvw anyway. The displacement maps (`s_tbump`, `s_tbumpX`, `s_tdetailBumpX`)
are sampled at `tcdh`/`tcdbump`, which are vertex attributes times the material constant `dt_params`, so the height
texel is the same in both frames. Only the control points, the normals and the camera move. The displacement itself is
active only with `TESS_HM` and `USE_TDETAIL`, and no cached permutation uses it.

## Finding: the engine's TESS_HM hull shader

- The engine compiles `dx11\tess.hs` with `TESS_HM` through `d3dcompiler_43`. In that build, the PN patch construction is
  split into a 4-instance fork phase.
- The phase writes some non-indexed outputs from every instance with instance-dependent values: the yz components of
  `f3B120`/`f3B021`, and `f3N110`/`f3N011`. Which instance wins is up to the hardware.
- The native HM surface is therefore not the textbook PN patch, and it does not rotate with the object.
- A fixture with previous equal to current showed the textbook previous patch up to 3 px away from the native surface on a
  64 px target.
- The domain variant therefore adds this frame's delivered-minus-textbook patch difference to the previous patch,
  carried with the control triangle's rigid frame (`rsf_tess_carry_patch`). The result is exact when nothing moved and
  under rigid motion. It is the plain textbook patch when the hull delivers that, as the `TESS_PN`-only builds do (the
  difference is then rounding).
- The same stock HM hull bytecode also makes NVIDIA's user-mode driver (`nvwgf2umx.dll+0x490a3b`) crash in
  `CreateHullShader`:
  - Engine HM hull: 3 of 40 fresh processes that only create it, with nothing else loaded.
  - `TESS_PN` hull: 0 of 40.
  - With and without the debug layer.
  - The RSF HM variant crashes at a similar rate.
- This is an engine-shader and driver issue independent of RSF, and no cached permutation uses `TESS_HM`. The device
  fixture therefore runs HM only with `RSF_TESS_HM=1`.

## Tests

- `anomaly_motion_tess_chain_device` (new, `tests/anomaly_motion_tess_chain_device.cpp`): runs on the NVIDIA adapter
  with the debug layer, against a deployment with the tess roots (`.local/anomaly/motion-deploy-tess` by default). For
  all seven cached permutations:
  - The production loader proves the original and checks the variant.
  - Linkage refusals: a stock hull, a stock domain, a stock VS, or the untessellated VS variant in the chain.
  - A skinned mover: an octahedron that PN-bulges toward a sphere under `triLOD` 8, a rigid previous palette (SKIN_1..4)
    or a bone-composed previous world (SKIN_0), and a different previous view and projection.
  - Motion equals `P_prev * V_prev * B_prev * x` (x from pixel centre and D32 depth) at every covered pixel with
    validity 1. Worst error is 4.7e-5 UV (0.003 px), where a flat-triangle previous position would be off by up to
    1.2 px.
  - Native MRT0/MRT1 and depth are bit-identical to the stock chain, the debug layer is clean, and no history gives
    zero motion.
  - With `RSF_TESS_HM=1`, the two HM permutations also pass (0.0036 px).
  - 12 of 12 consecutive runs passed.
- `anomaly_motion_contract`:
  - `tess-layout`: header packoffsets and DS `b13` packing.
  - `tess-policy`: the split multiplies back to the previous WVP, including SKIN_0.
  - `tess-compatibility`: hull/domain reflection rules.
  - `tess-linkage`: chain refusals.
  - `tess-manifest`: `hs`/`ds` records, subdirectory names, refusals.
- `anomaly_motion_device` compiles the `dx11/tess.*` roots through the production loader and now runs on the NVIDIA
  adapter.
- `anomaly_motion_tess_device` (first-party probe) still shows why a stock HS/DS can never carry the motion interpolants.
- `tools/anomaly_temporal_shader.py verify_fxc` checks 44 tessellation pairs with FXC 47 (interfaces, patch constants,
  `b13` layout) next to the 458 existing pairs.

## Game capture list (next run)

Deploy a motion folder regenerated with this tool. `.local/anomaly/motion-deploy-tess` is one, namespace
`cd2c8238...`. The older `.local/anomaly/motion-deploy` has no tess roots. Use the profile with `r4_enable_tessellation on`, `Motion=1`, and `SRStage=5` DLSS.

1. Log: `Anomaly motion variant dx11/tess.hs tess_hull` and `dx11/tess.ds tess_domain` lines, with no `kept stock` for
   them. `Anomaly motion tessellation chains swapped>0 no_variant~0 unlinked=0` while near characters are in view.
   `Anomaly motion classes`: `valid tessellated>0` and `unpaired tessellated~0`.
2. Near moving NPC (walk beside one within 10-20 m): motion-target captures (F10 or `MotionCapture`) show validity 1 on
   the NPC body and vectors that follow its limbs. A/B the ghosting against the previous build (`e991e89`+w27 refusal
   build) with the same route.
3. The player's own tessellated content (first-person hands and weapon parts drawn with `tess_pn`): validity 1 and
   plausible vectors while turning and walking. No ghost trail on the weapon against the sky.
4. A/B with `r4_enable_tessellation off` (vid_restart): motion should look the same as with tessellation on, with
   classes moving from `tessellated` to `skinned`/`single_bone`.
5. No new D3D errors or crashes at level load, when the tess hull/domain shaders are created.
