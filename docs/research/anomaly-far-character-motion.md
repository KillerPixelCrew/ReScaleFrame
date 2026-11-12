# Anomaly: motion vectors of far skinned characters

Status: investigated on 12 November 2026, fixture-tested, NOT game-tested. Readiness flags stay 0.

The user reports ghosting and smearing mostly on far skinned characters (real 3D meshes, not billboards), also in
zoomed and scoped views. They described it as looking as if those characters had no motion vectors at all. This note
records what X-Ray does with a character as distance grows, what our motion path gives it, the evidence, and the
capture that settles the remaining question. Engine paths are in `D:/xray-monolith/src`.

## 1. Engine: what changes with distance

| Hypothesis | Engine source | Verdict |
| --- | --- | --- |
| Animation LOD: bones of distant skeletons updated less often | `Layers/xrRender/SkeletonRigid.cpp:13-28` gates `CalculateBones` on `UCalc_Interval` (100 ms, `Include/xrRender/Kinematics.h:17`) only when `bForceExact` is false. Every rendered skeleton is forced: `add_leafs_Dynamic`/`add_Dynamic` call `pV->CalculateBones(TRUE)` for `MT_SKELETON_ANIM/RIGID` (`Layers/xrRender/r__dsgraph_build.cpp:777-797`, `983-1003`). `CKinematicsAnimated::OnCalculateBones` advances the tracks by the real frame delta, clamped to 66 ms (`SkeletonAnimated.cpp:624-640`). | Refuted. A drawn character has a fresh palette every rendered frame at any distance. Our previous palette is never identical to the current one for a moving body, and it never jumps several frames. |
| Object transform updated at a lower rate far away | The scheduler interval does grow with distance (`xrEngine/xrSheduler.cpp:359-363`, `shedule_Scale` = distance/200 in `xrEngine/xr_object.h:103`, monsters `t_min=100 t_max=250` in `xrGame/CustomMonster.cpp:805-806`). But position and rotation are applied in `UpdateCL` (`CustomMonster.cpp:532-545`: `UpdatePositionAnimation`, `XFORM().translate_over`). `UpdateCL` runs every frame for a "crow", and `CObject::renderable_Render` (`xr_object.cpp:411`) makes every rendered object a crow (`CGameObject::renderable_Render` and `CAI_Stalker::renderable_Render` call the inherited one). | Refuted for visible NPCs: their XFORM changes every rendered frame. |
| LOD skeleton meshes | `CSkeletonX_PM` changes only the index slide window; vertex positions and palette are the same. `m_lod` billboards replace the skeleton when SSA is below `r_ssaLOD_A` (`r__dsgraph_build.cpp:782-793`). | No effect on vectors. The user confirmed the far characters are 3D meshes, so `m_lod` is not involved. |
| Distance-dependent shader element | `CRender::rimp_select_sh_dynamic` (`Layers/xrRenderPC_R4/r4.cpp:66-76`): `SE_R2_NORMAL_HQ` when distance minus the bounding radius is below `r__dtex_range` (50 m), else `SE_R2_NORMAL_LQ`. `CBlender_deffer_model` (`blender_deffer_model.cpp:106,126`) and `uber_deffer` (`Layers/xrRender/uber_deffer.cpp:93,144`) make HQ `deffer_model_bump-hq`/`_d-hq` (or the tessellation chain) and LQ `deffer_model_bump`/`deffer_model_flat` with `deffer_base_bump`/`_aref_bump`/`_flat`/`_aref_flat`. | This is the only distance-dependent change on the motion path. A character beyond about 50 m is drawn only through the LQ pairs. No game capture so far has measured one. |
| Env-mapped gear | `CBlender_Model_EbB` (`Layers/xrRender/Blender_Model_EbB.cpp:194-236`) is deferred through the same `uber_deffer` pairs unless the material is blended; only blended materials use the forward `model_env_lq`, which draws outside the G-buffer window without depth writes. | Not a cause for opaque gear. |
| Distance culling of small parts | Anomaly's `IsValuableToRender` (`r__dsgraph_build.cpp:574`) drops small dynamic visuals beyond configured distances. | A part that disappears has no history. When it reappears it is invalid for one frame. This does not cause smear. |

## 2. Our motion path at distance

- Shader coverage: every compiled skinned model VS in the user's shader cache (`deffer_model_bump`, `-hq`, `_d-hq`,
  `flat`, SKIN_0..4, tessellation variants) belongs to the `core_model` family. Every one was replaced at runtime in
  wave 7 (`core_model.replaced_from_cache=25`, all 31 `core_pixel` roots replaced, zero refusals). The coordinator's
  inventory is in `.local/anomaly/shader-inventory/inventory.json`.
- LQ pairs end to end: `anomaly_motion_far_device` (new) draws all 41 HQ/LQ VS+PS pairs with SKIN_0..4 (as cached)
  through the production loader, `linkable`, `decide_core`, the b13 packers and `MotionDevice`. Two scenes: an
  orthographic mover, and a 0.6 x 1.8 m character 100 m away under a 10 degree perspective (a scoped view) that
  walks 5 cm. All 20 LQ pairs link. Native output is bit-identical. Validity is 1 on every covered pixel. The far
  character covers 8 px and moves 0.084 px, with a worst error of 0.0016 px. Missing history writes zero. Our LQ
  shaders are correct, and small coverage loses nothing (item 4: no MSAA in the motion window, the target follows
  the bound render size, and the depth-equal replay runs only for unpaired draws, which cannot win against an
  earlier depth-writing draw).
- Previous state: `MotionHistory` keys `{owner, visual, generation}` + domain. It returns the previous snapshot only
  from the immediately preceding frame of the same view, with matching bone count and kind. Combined with
  section 1, previous palette = palette the engine drew last frame, previous world = last frame's XFORM. The vectors
  describe exactly what was rendered. Therefore the "keep the last two distinct palettes and reproject over the
  true interval / mark invalid when the palette did not change" fix was not applied. The engine never draws a stale
  pose for a visible character, and if it did, a zero skinning delta would still be the true motion of the rendered
  mesh, and marking it invalid would turn it into camera-only motion (the smear). It is instrumented instead
  (`stale_palette`, below).

## 3. Game evidence available today (wave 7, `p1` run)

`.local/anomaly/far-ghost/bands.py` bins the SR-pipeline dumps by depth (near plane 0.2 m, `xrEngine/device.h:21`).
For every valid pixel it measures object motion against the camera reconstruction from `clip_to_previous`. The
camera fit has a median of 0.0014-0.0039 px. `submitted.py` compares the adapter's motion target with
`motion_submitted`, which is what the backend received.

- Movers (walking NPCs) appear at 20-50 m with valid object motion of 0.12-1.2 px/frame (median). The backend
  received exactly that: raw minus submitted is 0.0000-0.0002 px. Units are previous-minus-current pixels
  (scale -853, -480).
- No object-motion pixel exists beyond 50 m in any of the 11 frames, and the overlays
  (`.local/anomaly/far-ghost/p1-*.png`) show no character there. The existing captures therefore cannot answer the
  question beyond `r__dtex_range`.

## 4. Conclusion

No engine or shader mechanism leaves a far character without vectors. Fixtures prove the LQ path correct, and the
game data proves it correct up to 50 m. Two causes remain open, and the next capture separates them:

1. Runtime pairing or history beyond 50 m: far LQ draws arriving unpaired (stock shaders, replayed invalid, so
   camera-only) or invalid at runtime for a reason the fixtures do not reproduce. The new per-band counters show
   this directly.
2. Correct vectors that the upscaler still does not use well. At 853x480 render size, a character 50-150 m away is
   2-4 px wide and moves 0.05-0.3 px per frame. Its 3x3 neighbourhood always contains its own colours, so history
   clamping cannot reject the character colour left behind on the background. The result is a trail that looks like
   missing vectors. The inputs that address this are the per-pixel hints, which the D3D11 SR API does not yet
   carry (Stage 6):
   - DLSS through Streamline: `kBufferTypeBiasCurrentColorHint`.
   - FSR3/FSR4: reactive mask, or transparency-and-composition mask.
   - XeSS: responsive pixel mask.

   The motion target's fourth channel is unused (written 0), so a skinned/moving-object mask could be carried there.
   That changes `games/anomaly/shaders/core/rsf_core_pixel.h`, the PS b13 ABI and the SR consumer (`sr_*`, runtime),
   which this task does not own.

Also note: before w27, single-bone NPC parts (RM_SINGLE: held weapons, gear) were invalid (`unsupported_kind`,
19-20 draws per frame with NPCs in view), so those parts were camera-only at every distance. The w27 fix makes them
valid but has not run in the game yet. It is not specific to distance.

## 5. Instrumentation added

`games/anomaly/src/motion_stage.cpp` now logs, every report window:

```
Anomaly motion character bands | 0-20m valid=.. invalid=.. unpaired=.. no_previous=.. stale_palette=.. hq=.. lq=.. tess=.. | 20-50m ... | 50-100m ... | 100-150m ... | 150m+ ... | unknown ...
```

The counts cover main-pass hardware-skinned, single-bone and software-skinned draws, by distance from the camera to
the draw's world translation (`camera_distance`, from the engine view and `RCache.xforms.m_w` at the flush):

- valid/invalid: the history decision.
- unpaired: no linkable variant pair, replayed invalid.
- no_previous: invalid because no previous snapshot exists.
- stale_palette: bones unchanged while the object moved more than 1 mm.
- hq/lq/tess: the engine element by the bound VS root (`model_element`).

The existing `Anomaly motion unpaired (...)` line names the stock VS+PS roots of unpaired draws.
`.local/anomaly/far-ghost/band_counters.py <log>` summarises the band lines.

## 6. Capture that settles it

Build from this tree with `Motion=1`, `SRStage=5`, DLSS Quality, and the shared-host fixture run first.

1. A far NPC walking or running at 50-150 m: Cordon road, or the start village from the hill, with `demo_record`.
   Hold still and track it in normal view, then zoomed in (aim down sights or binoculars). Take F10 sequences of at
   least 8 consecutive SR captures (`SRStageCapture` with interval 1), then the same view with `SRStage=0` and an
   F12 sequence for the stock reference.
2. Read:
   - The `Anomaly motion character bands` lines over the capture windows. For 50-100 m and 100-150 m: `lq` > 0,
     `unpaired` = 0, `invalid` = 0, `stale_palette` = 0 means cause 1 is excluded. Otherwise the `unpaired (...)`
     line names the pair to fix.
   - `py -3 .local/anomaly/far-ghost/bands.py <sr-pipeline dir>` and `submitted.py`: object-motion pixels must appear
     in the 50-150 m bands with validity 1. `p1-*`-style overlays from `overlay.py` show where.
   - The final images: if the far NPC trails with valid vectors and correct submitted motion, the cause is 2, and
     the fix is the Stage 6 mover hint (DLSS bias-current-colour, FSR reactive, XeSS responsive).
3. `r__dtex_range 200` (console), same view: every character becomes HQ. If the smear changes with it, the element
   switch is involved after all.

## Tests

- `anomaly_motion_far_device` (new): 41 HQ/LQ pairs × near and far scenes against the CPU model, debug layer clean.
- `anomaly_motion_contract far-character` (new): bands, camera distance from a rotated view, element names, stale
  palette rules.
