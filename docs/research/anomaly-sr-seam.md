# Anomaly SR seam: world/output resolution split and linear-colour boundary

Status: source and retained-binary research plus a design. Nothing here has been built,
injected or game-tested. No hook below changes rendering today. Every statement is either
cited to source/binary evidence or marked **UNVERIFIED**.

- Reference engine: `D:/xray-monolith`, commit `028ccf5` ("X-Ray Monolith Engine for
  S.T.A.L.K.E.R. Anomaly 1.5.3"). Source-to-binary correspondence is checked only where a
  binary citation is given.
- Binary: `AnomalyDX11.exe`, SHA-256 `c857b6f4...2fb4e`, image base `0x140000000`
  (see `docs/research/anomaly-native-hooks.md`). Binary evidence is the retained IDA output in
  `.local/anomaly/rea-evidence/` (`0003-procedure_pseudo_code.json` = `CRender::Render`,
  `0012-procedure_pseudo_code.json` = `CRender::RenderToTarget`), read with
  `.local/anomaly/sr-seam/dump_code.py`. Stage 0 (section 8) added an IDA analysis of a private copy of the
  executable (`.local/anomaly/sr-stage0/ida/`; the install was not touched). Its results are in
  `games/anomaly/src/sr_profile.h` and are re-checked against the installed file by
  `py -3 -I tests/anomaly_sr_evidence.py --verify` (output: `.local/anomaly/sr-stage0/verify.txt`).
- Packed vanilla shaders: `.local/anomaly/vanilla/shaders/r3` (paths below are relative to it).
- AVX executable: nothing here applies to `AnomalyDX11AVX.exe` until mapped separately.
- MSAA (`o.dx10_msaa`) is out of scope: the design refuses SR when MSAA is enabled.

## Summary

1. The R4 renderer sizes every screen target, the depth buffer, the viewport, the `screen_res`
   and `pos_decompression_params` shader constants, and the pixel-space quads of every full-screen
   pass from `Device.dwWidth/dwHeight`. Viewports follow the size of the last bound target. This
   consistency allows the world to be resized by changing those values in one bounded window,
   instead of patching about 90 renderer sites one by one.
2. **Recommended mechanism ("W-phase twin swap")**: the engine keeps all its own targets at output
   size. The adapter allocates render-resolution twins for the world-phase subset. On real world
   frames only, from the prologue of `CRenderTarget::phase_scene_prepare` to the seam, it swaps
   `Device.dwWidth/dwHeight`, `HW.pBaseZB` and the swapped targets' surfaces to the twins. At the
   seam it swaps everything back. UI, PDA content, scope copy, menus, screenshots and reset never
   see a render-size resource.
3. **Recommended colour seam (S-B)**: after the distortion mask and before
   `phase_sunshafts`/`phase_blur`, inside `phase_combine`. At that point `generic0` holds the
   complete composited scene (sky, clouds, deferred combine, forward/transparent, volumetrics) in
   the engine's "low" tonemap space. Bloom has been extracted but not applied. Distortion has been
   recorded but not applied. Promoting only the **render-size twin** of `generic0` (and its copy
   partner `generic_temp`) to `R16G16B16A16_FLOAT` makes that low value exactly invertible over its
   whole range. SR receives `x = f^-1(low)`, which is exposure-scaled linear radiance, and the
   output is re-tonemapped with the stock `f` into the engine's own UNORM8 `generic0` at output size.
   All post-processing then runs unchanged at output resolution.
4. **Depth**: read the rasterized D24 world depth from an SRV-capable `R24G8_TYPELESS` twin. Use
   the standard, non-inverted, finite-far convention, with the first-person range compressed to
   [0, 0.02] by `rmNear`. Formulas for converting linear view Z are given for reference, but are
   not used as the primary source.
5. **Jitter**: jitter `Device.mProject` for the world window and re-upload it to `RCache`. First-person
   projections are rebuilt locally at five sites, and each calls `rmNear()` immediately after
   uploading; a `rmNear` detour adds the same jitter there. Shadow, SSAO and fluid passes set their
   own matrices and viewports and stay unjittered.
6. **First implementation step**: map and byte-guard the new sites (`phase_scene_prepare`,
   `phase_sunshafts`, `phase_blur`, `rmNear`, `CRenderTarget`/`CRT`/`CTexture` field offsets,
   `CTexture::surface_set`, the `RCache` texture cache). Done statically in Stage 0 (section 8). Then implement the twin swap at **scale 1.0**,
   with FP16 `generic0` and the f^-1/f round trip. Accept it with a pixel comparison against stock
   rendering before reducing resolution.

## 1. Render target allocation and the resolution split

### 1.1 How R4 sizes its targets

- Device dimensions come from the swap chain: `dxRenderDeviceRender.cpp:72-77` sets
  `dwWidth/dwHeight = HW.m_ChainDesc.BufferDesc.{Width,Height}` after reset, and
  `dxRenderDeviceRender.cpp:172-173` does the same at creation. Binary: `Device.dwWidth/dwHeight` are `dword_140D37668/140D3766C` (RVA
  `0xd37668/0xd3766c`), passed directly as the `u_setrt(W,H,...)` arguments in the
  `CRender::Render` early-out (`0003` decompile, call to `sub_140859CB0`).
- Depth/stencil: `CHW::UpdateViews`, `dx10HW.cpp:1017-1058`, creates `pBaseZB` at swap-chain size
  as `DXGI_FORMAT_D24_UNORM_S8_UINT`, which is **not typeless**, so it cannot be read by a shader. Binary:
  `HW.pBaseRT = qword_140CEB8E8`, `HW.pBaseZB = qword_140CEB8F0`, `HW.pContext = qword_140CEB8D8`,
  `HW.m_pSwapChain = qword_140CEB8E0`. These come from the `Render` early-out arguments and from
  `RenderToTarget` using `GetBuffer` at swap-chain vtable +72 and `CopyResource` at context vtable +376.
- All screen targets are created once in `CRenderTarget::CRenderTarget`
  (`r4_rendertarget.cpp:293-1082`) from `Device.dwWidth/dwHeight`:
  - G-buffer: `rt_Position` FP16 (400); `rt_Color` (albedo) and `rt_Accumulator` (405-429). On
    D3D10+, `b_MRT_mixdepth` is forced TRUE (`HWCaps.cpp:242`, `r4.cpp:195`), so `rt_Color` is
    `A8R8G8B8` and `rt_Accumulator` is FP16 (409-410).
  - Generic LDR8 targets `generic0/1/generic` (433-435), with the comment "we should change their
    formats into D3DFMT_A16B16G16R16F" at 432; `generic_temp` (437-440); `generic2` FP16 (471-472,
    `advancedpp`).
  - `rt_secondVP` (442), `rt_dof` (443), `rt_ui_pda` (444), sunshafts (446-447), blur pyramid
    w/2, w/4, w/8 (450-457), `pp_bloom` (459), SMAA (670-679), half depth (705-725), HDAO
    (744-754), and the async screenshot staging texture (790-806).
  - Fixed-size, independent of screen size: bloom `BLOOM_size_X/Y` (646-668), luminance 64/8/1
    (681-703), shadow maps `smapsize` (494-499).
  - `CRenderTarget::dwWidth/dwHeight` start at device size (1080-1081). After that, every `u_setrt`
    sets them to the size of the bound target (32-120).
- Binary: `CRender::Target` is at `CRender+0x768` (`*(a1+1896)` in both decompiles).
  `RenderToTarget` takes `rt_ui_pda` at `Target+0x3D0` and `rt_secondVP` at `Target+0x3C0`.
  Stage 0 mapped every `ref_rt` from the constructor (`0x853770`): each `ref_rt::create` call
  (`0x8a15d0`) takes `lea rdx,"$user$..."` and `lea rcx,[r15+offset]`. Examples: `rt_Position +0x378`,
  `rt_Color +0x380`, `rt_Accumulator +0x388`, `rt_Generic_0 +0x3A8`, `rt_Generic_1 +0x3B0`,
  `rt_Generic +0x370`, `rt_Generic_temp +0x3B8`, `rt_Generic_2 +0x420`, `rt_MSAADepth +0x358`,
  `rt_half_depth +0x590`, `rt_ssao_temp +0x588`. All 26 are in `sr_profile.h` `target_slots`.
  `CRenderTarget::dwWidth/dwHeight` are at `+0x8/+0xC` (`u_setrt` at `0x859cb0`).
- `CRT` layout (`SH_RT.h:6-38`) is binary-confirmed from `CRT::create` (`0x8a1250`) and `_CreateRT`
  (`0x873f60`, which allocates 0x50 bytes): `dwReference +0x0`, `pSurface +0x10`, `pRT +0x18`,
  `pZRT +0x20`, `pUAView +0x28`, `pTexture +0x30`, `dwWidth +0x38`, `dwHeight +0x3C`, `fmt +0x40`,
  `_order +0x48`. `CRT::create` turns `D24S8` (75) into `R24G8_TYPELESS` (44) with a
  `D24_UNORM_S8_UINT` view (`dx10SH_RT.cpp:79-82,121,162`; binary `cmp ecx,4Bh` then `lea r13d,[rbx+2Ah]`).

### 1.2 Engine invariants that make a bounded swap work

- Viewport: `CRender::rmNear/rmFar/rmNormal` (`r4.cpp:834-858`) build `{0, 0, Target->get_width(),
  Target->get_height()}`, with depth ranges [0, 0.02], [0.99999, 1] and [0, 1]. These are the
  *last bound target's* dimensions, and they are used for shadow maps too (`phase_smap_D.cpp:33-34`
  calls `rmNormal` with the shadow map bound). **Do not override the viewport in `rmNormal`.**
  Binary: `rmNormal` is the first virtual call in `CRender::Render`, at vtable +456 (slot 57).
  The `CRender` RTTI vtable (`0xb669f8`) confirms slot 55 `rmNear = 0x83a450` (MaxDepth 0.02), slot 56
  `rmFar = 0x83a3c0` (MinDepth 0.99999) and slot 57 `rmNormal = 0x83a4d0`, matching source order
  (`xrEngine/Render.h:351-353`).
- `CBackend::OnFrameBegin` calls `rmNormal` and binds base RT/ZB each frame
  (`R_Backend_Runtime.cpp:40-55`).
- Full-screen passes use pretransformed pixel quads built from `Device.dwWidth`. The stub VS divides
  them by `screen_res` (`stub_screen_space.vs:9-10`). `screen_res = Device.dwWidth/Height`
  (`Blender_Recorder_StandartBinding.cpp:389-396`). Half/quarter-size passes rely on the viewport
  staying at full size, with quads covering a corner (`rendertarget_phase_blur.cpp:27-40`).
- G-buffer reconstruction: `pos_decompression_params{,2}` use `Device.fFOV`, `Device.fASPECT` and
  `Device.dwWidth/Height` (`r4.cpp:106-125`). `gbuffer_load_data` (`gbuffer_stage.h:115-143`) uses
  `Sample(uv)` without MSAA but reconstructs view XY from `pos2d` (`SV_Position`), so position
  texture size, `pos2d` space and constants must agree.
- All render-target binds go through `CBackend::set_RT/set_ZB` and then a single
  `OMSetRenderTargets` in `ApplyRTandZB` (`dx10R_Backend_Runtime.h:15-47,746-753`). Pointers are
  read when `u_setrt` runs, so swapping the `HW.pBaseZB` global and the `CRT` view pointers is
  picked up by every later bind.
- Shader resources are bound by name, through the `CTexture` that `CRT::create` registers
  (`pTexture->surface_set`). `CTexture::surface_set` releases the old SRV and creates a new one each
  call (`dx10SH_Texture.cpp:55-117`). The engine already calls it every frame for `t_LUM_src/dest`
  and the env maps (`phase_combine.cpp:57-58,235-237`). `CBackend::set_Textures` caches by
  `CTexture*` (`R_Backend_Runtime.cpp:208-240`), so after a mid-frame surface change, cached slots
  that reference a swapped texture must be invalidated. Binary: `RCache = 0x140D63BC0`;
  `STextureList* T +0x640`, `textures_ps[16] +0x658`, `textures_vs[4] +0x6D8`,
  `textures_gs/hs/ds/cs[16] +0x6F8/+0x778/+0x7F8/+0x878`, `ctable +0x5C8`, `pRT[4] +0x578`, `pZB +0x598`,
  `m_bChangedRTorZB +0x980` (from `CBackend::Invalidate` at `0x842c50` and `u_setrt`). The minimal
  texture-cache invalidation is `T = nullptr` plus zeroing those arrays, because `set_Textures`
  returns early on `T == _T` and then compares `CTexture*`, which does not change when a surface is
  swapped. The full `CBackend::Invalidate` (what `OnFrameBegin` calls) also unmaps the xform constants
  and resets the state, sampler and SRV managers, so calling it mid-frame is heavier than needed.
  Whether a forced constant re-setup is also needed is still **UNVERIFIED** (runtime; see 1.5).

Consequence: during a window in which `Device.dwWidth/dwHeight`, `HW.pBaseZB` and the world
targets all describe render size, every engine computation that depends on size is consistent
without per-site patches.

### 1.3 Target classification

W = world window (render size, twin swapped in); P = post window (output size, engine original);
F = fixed size; O = outside `CRender::Render` (output size, engine original).

| Target (name) | Phase | Twin format | Why |
| --- | --- | --- | --- |
| `rt_Position` `$user$position` | W, then upsampled for P | FP16 (stock) | G-buffer; P passes `Load` it at output coordinates (`depth_of_field.ps:31`, `ogse_sunshafts_mask.ps:13-15`) |
| `rt_Color` `$user$albedo` | W, then upsampled for P | UNORM8 (stock) | Albedo in W; P post-chain scratch with `CopyResource` into `generic0` (`rendertarget_phase_{dof,lut,smaa,...}.cpp`) |
| `rt_Accumulator` | W | FP16 (stock) | Lighting accumulation |
| `rt_Generic_0` `$user$generic0` | W twin, P original | **FP16** (promoted) | SR input; P post reads/writes the output UNORM8 original |
| `rt_Generic_temp` | W | **FP16** (promoted) | `CopyResource` from `generic0` (`phase_combine.cpp:284-288`) needs identical format; it is also the opaque-only colour (see 2.6) |
| `rt_Generic_1` `$user$generic1` | W, **kept through P** | UNORM8 (stock) | High/bloom source in W, then the distortion mask; `combine_2` reads it only by UV (`combine_2_naa.ps:48`) |
| `rt_Generic_2` | W | FP16 (stock) | Volumetric light accumulation |
| `rt_half_depth`, `rt_ssao_temp` | W | stock | SSAO/HDAO |
| `HW.pBaseZB` | W twin, P original | `R24G8_TYPELESS` + SRV | World depth/stencil; SR depth input |
| `rt_Generic` | P | stock | Sunshafts/MSAA scratch, after the seam |
| blur, `pp_bloom`, `dof`, sunshafts, SMAA | P | stock | All after the seam |
| bloom1/2, luminance pool, shadow maps | F | stock | Size does not depend on the screen |
| `rt_ui_pda`, `rt_secondVP`, backbuffer, async screenshot | O | stock | Copies of the output-size backbuffer |

### 1.4 Minimal guarded hook set

| Id | Site | Evidence | Action |
| --- | --- | --- | --- |
| H1 | `CRenderTarget::phase_scene_prepare` (`r4_rendertarget_phase_scene.cpp:4-28`), called only from `CRender::Render` (`r4_R_render.cpp:278,285`) | RVA `0x8e4310`; the only callers are the real calls at `0x897d2c`/`0x897d66` in `Render` (section 8) | Prologue: if the frame is eligible, swap dims, `pBaseZB` and twins, apply jitter and mip bias. Epilogue: call `rmNormal` so the viewport matches the render size (Render's own `rmNormal` at line 215 used the previous frame's output-size target) |
| H2a/b | `CRenderTarget::phase_sunshafts` / `phase_blur` (`rendertarget_phase_{sunshafts,blur}.cpp:3`), called only at `phase_combine.cpp:374,378` | RVAs `0x8f07c0`/`0x8f2590`; one real call each (`0x8bf273`, `0x8bf27b`), not inlined (section 8) | Prologue of whichever runs first in a W frame: the seam (section 5). Other calls pass through. `phase_blur` runs unconditionally, including `_menu_pp`; `phase_sunshafts` only when `!_menu_pp` and `ps_sunshafts_mode` is 2 or 3 |
| H3 | `CRender::Render` (RVA `0x8977c0`, already guarded) | `native_profile.h` | Epilogue: restore `generic1` and anything the seam did not restore; refuse further SR if the seam did not fire. `phase_combine` (`0x8bc980`) is a real call at `0x8985bf`, the last call before the epilogue |
| H4 | `CRender::rmNear` (`0x83a450`, vtable slot 55, verified) | RTTI and `geometry_profile.h` | Inside the W window only: jitter the first-person projection (section 4). The geometry producer already detours this function (`geometry_site_rm_near`): H4 must extend that detour, not install a second hook on the same bytes |
| H5 | `reset_begin/reset_end` (`0x83a020/0x83a1c0`, already guarded) | `anomaly-native-hooks.md` | Destroy and recreate twins; reset SR history |
| H6 | `SetCacheXform` (`0x823250`, already guarded) | `anomaly-native-hooks.md` | Capture the unjittered view and projection |

Data sites, all verified in Stage 0 (section 8): `Device.dwWidth/dwHeight` (`0xd37668/0xd3766c`),
`HW.pBaseZB` (`0xceb8f0`, read by `phase_scene_prepare` and the `Render` early-out), `CRender::Target`
(`+0x768`), the `CRenderTarget` `ref_rt` offsets for every twinned target, the `CRT` and `CTexture`
layouts (`CTexture`: `pSurface +0x50`, `desc_cache +0x70`, `desc +0x78`, `m_pSRView +0xA8`),
`CTexture::surface_set` (`0x87ac30`), `dx10SamplerStateCache::SetMipLODBias` (`0x7fe170`,
`dx10SamplerStateCache.cpp:201-222`) on `SSManager` (`0x140D63A20`), and `Device.mView/mProject/mFullTransform`
(`0xd376c8/0xd37708/0xd37748`).

Eligibility, checked in H1: not an SVP frame (`Device.m_SecondViewport.IsSVPFrame()`,
`device.cpp:751-754`), MSAA off, twins valid for the current generation, and SR configured. Menu,
no-level and first-after-reset frames never reach `phase_scene_prepare`
(`r4_R_render.cpp:217-238`), so H1 is inherently world-only. That is why H1 is used and not the
`Render` prologue.

**Swap mechanics (choose one in Stage 0).** (a) Swap the contents of each engine `CRT`
(`pSurface`, `pRT`, `pZRT`, `pUAView`, `dwWidth`, `dwHeight`, `fmt`) with an adapter-owned record,
then call `pTexture->surface_set(twin)`. (b) Swap the `CTexture::pSurface/m_pSRView` fields directly
with cached SRVs. (a) touches fewer layout fields that are hard to discover; (b) avoids per-frame
SRV creation. In both, invalidate the `RCache` texture-slot cache after each swap.

Stage 0 choice: **(b)**, now that every field it needs is binary-verified. Swap the `CRT` record fields
(a pointer and value exchange that leaves reference counts unchanged), and exchange `CTexture::pSurface`,
`m_pSRView`, `desc_cache` and `desc` with the twin's pre-created values, so `desc_enshure` does not
re-query the swapped texture. `surface_set` would instead Release and recreate the SRV on every swap
and restore, and drop the CTexture reference on the original surface each time. (b) is valid only
while nothing calls `surface_set` on a swapped texture inside the window. The only per-frame
`surface_set` targets are `t_LUM_src/dest` (`CRenderTarget+0x4C8/+0x4D0`) and the env maps, and none
of them is twinned. This is a design decision; nothing has been built or game-tested.

Rejected alternatives:

- **Allocate the engine's targets at render size** by changing `Device.dwWidth` around the
  `CRenderTarget` constructor. The same constructor allocates `rt_ui_pda`, `rt_secondVP`, SMAA, blur
  and other targets that must stay at output size. Both the PDA and the scope copy are
  `CopyResource` from the output-size backbuffer (`r4_R_render.cpp:584-587`), which requires
  identical dimensions.
- **Patch each `Device.dwWidth` read**: about 93 renderer sites, plus shader constant binders.
- **Override the viewport in `rm*`**: this breaks shadow maps (see 1.2).

### 1.5 Every "target size == screen size" assumption, and how the window satisfies it

| Assumption | Sites | Satisfied by |
| --- | --- | --- |
| Pixel quads from `Device.dwWidth` | `phase_combine.cpp:215-228,447-492`, `phase_scene.cpp:84-101`, `rendertarget.cpp:126-146,1200-1218`, all `rendertarget_phase_*.cpp` | Dimension swap in W; restored in P |
| `screen_res`, `pos_decompression_params` | `StandartBinding.cpp:389-396`, `r4.cpp:106-125` | Same; binders run when an element's constant table binds. Constant-cache staleness across a swap is **UNVERIFIED** and needs a forced constant re-setup |
| Jitter-texture tiling | `phase_combine.cpp:215-216`, `rendertarget.cpp:198-199` | Dimension swap |
| `CopyResource` same size/format | `phase_combine.cpp:286`; P `rt_Color` to `generic0` copies | Twin pairs share size/format inside each window |
| RTV/DSV dimension match | Every `u_setrt(..., HW.pBaseZB)` | W uses the render-size DSV twin; P uses the original. Stock already binds `pBaseZB` with half-size blur targets (`rendertarget_phase_blur.cpp:30`). Whether D3D11 drops such binds is **UNVERIFIED**: check with the debug layer in Stage 1 |
| Viewport = last bound target | `r4.cpp:834-858` | H1 epilogue `rmNormal`; the seam calls `rmNormal` after restoring |
| `Device.dwWidth` read by UI code inside `Render` | `r_dsgraph_render_hud_ui` calls `g_hud->RenderActiveItemUI()` (`r__dsgraph_render.cpp:583`); `OnRenderPPUI_main/PP` (`phase_combine.cpp:303,354`) | **UNVERIFIED**: compare item-attached UI (detectors, PDA in hand) at scale 1.0 and 0.5 |
| 3D fluid screen size | `FluidManager.SetScreenSize(Device.dwWidth, ...)` at create/reset (`r4.cpp:501,587`) | **UNVERIFIED**: fluid targets stay at output size. Either set the fluid screen size to render size when SR is configured, or refuse SR while fluid volumes are active |
| Minmax-SM auto threshold | Swap-chain area (`rendertarget.cpp:1256-1263`) | Unaffected; it uses swap-chain dimensions |
| MT thread reading `Device.dwWidth` during render | `device.cpp:341-350` releases the second thread, which runs `seqFrameMT` (`device.cpp:175-187`) concurrently with rendering | **UNVERIFIED**: matrices use `_saved` copies, but dimensions have none. Audit `seqFrameMT` consumers |

## 2. The colour seam

### 2.1 Stock colour order (non-MSAA)

From `CRender::Render` (`r4_R_render.cpp:209-542`) and `CRenderTarget::phase_combine`
(`r4_rendertarget_phase_combine.cpp:39-663`):

1. G-buffer fill (`phase_scene_begin`, 339), including first-person geometry (`r_dsgraph_render_hud`, 340),
   wallmarks and item UI (438-451), lights into the FP16 accumulator (486-533).
2. `generic0/1` cleared and bound as MRT (79-92). **Sky** (`RenderSky`) and **clouds** (99-114).
   `sky2.ps:31-32` applies the same extended-Reinhard curve as the scene (`/2.89 = 1.7^2`).
   `clouds.ps:20-24` writes `color * scale` without the curve.
3. **combine_1** (143-282): `color` is genuine pre-tonemap radiance (`combine_1.ps:136-144`). It is
   then tonemapped: `low = f(color*scale)`, `high = color*scale/9` (`common_functions.h:21-30`).
   Alpha is the sky-fog blend, and the pass blends `INVSRCALPHA/SRCALPHA` over the sky
   (`blender_combine.cpp:19-21`).
4. `generic_temp = copy(generic0)` (284-288): the opaque scene plus sky, before forward geometry.
5. **Forward** (290-304) into `generic0` only: particles, glass, water, strict-sorted and
   first-person sorted geometry, including the physical PDA `pda_overlay` material, rain and
   thunderbolts (`render_forward`, `r4_R_render.cpp:544-565`).
6. **Volumetric** lights (306-309), tonemapped with the same `tonemap()` into both MRTs
   (`combine_volumetric.ps:45`).
7. **Bloom** and **luminance adaptation** (`phase_bloom`, 324). Fixed-size targets read `generic1`
   (`bloom_build.ps:10-19`); `phase_luminance` rewrites the `$user$tonemap` pool entry
   (`phase_bloom.cpp:146-147`, `phase_luminance.cpp:121`).
8. **Distortion mask** geometry into `generic1` (329-356).
9. Sunshafts (373-374), blur (378), new bloom (381), DOF (383-386), LUT (388), gas mask (390-394),
   night vision (396-397), SMAA (399-405).
10. **combine_2** into `rt_Color` (415-535; `PP_Complex` is forced TRUE at 413). It loads `generic0`,
    adds bloom, applies motion blur, optional Uncharted2, `img_corrections` and the distortion
    offset (`combine_2_naa.ps:48-82`).
11. Lens flares (540-541), `phase_pp` to the backbuffer (`phase_PP.cpp:114-172`).
12. After `Render` returns: tracers (`Level_bullet_manager.cpp:1031-1032`), the scope copy
    (`Level.cpp:776-777`), screen UI (`Level.cpp:779`). The PDA content was copied *before* the
    world render (`Level.cpp:716-760`).

### 2.2 What is linear where

- Genuine pre-tonemap colour exists only shader-locally in `combine_1.ps` (and in its volumetric
  equivalent). No stock FP16 scene texture exists. This confirms `PROGRESS.md`.
- `generic0` is a composite in "low" space: sky and volumetrics use `f`; combine_1 uses `f`; clouds
  are approximately `f` for small values (`f'(0) = 1`); forward materials blend in that space by
  construction. `generic1` holds `x/9` for the bloom extraction, then the distortion mask.
- `f(x) = x(1 + x/W^2)/(1 + x)` with `W = 1.7`, and `x = colour * s_tonemap` (`combine_1.vs:21-22`).
  On UNORM8, `f` saturates at `x = W = 1.7`. Above that, `generic0` is clipped and only the 8-bit
  `generic1 = x/9` remains. On FP16, `f` is strictly monotonic and invertible:

  ```text
  b = 1 - y;  D = sqrt(b*b + 4*y/W^2)
  x = 2*y / (b + D)          if b > 0   (y < 1, cancellation-free)
  x = (W^2/2) * (D - b)      otherwise
  ```

  `.local/anomaly/sr-seam/tonemap_inverse_check.py` (run with `py -3 -I`) measured a float64
  round-trip error of at most 6.2e-16 for x in (0, 20], and FP16 storage error of at most 3.5e-4.
  For UNORM8 the relative error reaches 0.15 at x = 2 and 0.92 at x = 20 (clipped). This is
  arithmetic, not a game measurement.

### 2.3 Candidate seams

| Seam | Location | SR input | Cost / breakage | Verdict |
| --- | --- | --- | --- | --- |
| S-A | Prologue of `phase_bloom` (`phase_combine.cpp:324`) | Same `generic0` as S-B | Bloom and luminance run at output size, so W `generic1` must be rebuilt (`x/9` and skyblend alpha) at output size. The distortion pass then needs an output-size depth with world content for its geometry depth test, and runs with a jittered projection at output size | Workable, more surgery |
| **S-B** | **First of `phase_sunshafts`/`phase_blur`** (374/378) | **Complete composite in low space; FP16 twin makes it invertible** | Bloom and luminance stay in W (fixed-size targets). The distortion mask is rendered at render size against the real world depth and consumed by UV. Only `generic0`/`generic_temp` twins change format. Post runs stock at output size | **Recommended** |
| S-C | Linear FP16 MRT written inside `combine_1` | True radiance | Misses sky/clouds (drawn before), forward and volumetrics (after). Every forward material would have to be moved after SR at output size, or rewritten for a linear target. Needs new shader permutations and cache/reflection hooks (`tools/anomaly_shader_README.md`, cache section) | Rejected for the first implementation; revisit only if f^-1 shows measurable artefacts |
| S-D | After `combine_2`, before `phase_pp` | Final LDR including grain, motion blur, SMAA, LUT and lens dirt | Temporal SR on post-processed noise | Rejected (spatial fallback only) |

Why S-B: `generic0` is identical at S-A and S-B. Bloom extraction (step 7) and the distortion mask
(step 8) only write `generic1`, the fixed-size bloom targets and the luminance pool. So the later
seam costs nothing in SR input, and it keeps both of those passes in the world window, where the
real depth and the matching `generic1` live. Bloom *application* and distortion *application*
still happen after SR in `combine_2`, which is the preferred order.

### 2.4 What must be promoted, and what that changes

- Only the W twins of `generic0` and `generic_temp` become `R16G16B16A16_FLOAT`. The engine's own
  `generic0` stays UNORM8 for P. Mixed-format MRT with `generic1` UNORM8 is permitted, as the stock
  G-buffer already mixes formats (`mrtmixdepth`). FP16 blending is required and is supported on
  feature level 11 hardware.
- No shader recompilation and no permutation changes. Behavioural differences, all **UNVERIFIED**
  and to be measured:
  1. Writes above 1 are no longer clamped before blending: combine_1 for x > 1.7, sky, additive
     forward particles, and the fog blend of overbright pixels. Reinsertion saturates
     `f(SR(x))`, so single-layer results match stock. Stacked blends that cross 1 differ slightly.
  2. Forward materials that sample `$user$generic0` (`distort.s:9`) see the FP16 W twin.
  3. `accum_sun` binds `generic0` as `s_smap` in `SE_SUN_LUMINANCE` (`blender_light_direct.cpp:81-90`).
     Whether that element is used at runtime is **UNVERIFIED**.
- MSAA: hard-coded UNORM resolves (`phase_combine.cpp:314-321`) and the per-sample combine paths
  are why MSAA is refused rather than patched.

### 2.5 Inputs available at S-B in linear HDR

| Input | Source | Form |
| --- | --- | --- |
| Colour | W `generic0`, through f^-1 (adapter pass) | FP16, exposure-scaled linear `x` |
| Opaque-only colour | W `generic_temp` (copy before forward, step 4), through f^-1 | FP16; DLSS `ColorBeforeTransparency` / FSR opaque-only (`dlss_pipeline.h:165-169`) |
| Depth | W DSV twin, SRV `R24_UNORM_X8` | Device depth (section 3) |
| Motion | RTV2 from the core/foliage shader work, `R16G16B16A16_FLOAT` | Current-minus-previous UV, Y down, validity in Z (`games/anomaly/shaders/core/README.md`) |
| Exposure | `$user$tonemap` 1x1 R32F (`r2_RT_luminance_cur`, `phase_combine.cpp:57-58`) | Overwritten by `phase_luminance` *before* S-B, so it must be snapshotted (2.6) |
| Bloom | Not in the SR input (good) | Applied in P |

### 2.6 Exposure

Stage 5 default: feed `x` (already multiplied by the engine's adapted scale) with a constant 1.0
exposure. Refinement: snapshot the 1x1 `$user$tonemap` surface in a `phase_bloom` prologue
detour, before `phase_luminance` overwrites it. Then feed `x / scale` with the snapshot as the
exposure texture (`native_sr.cpp:49-87` accepts a 1x1 float texture). Compare history stability
during eye adaptation. Choosing between the two is a measurement, not a guess.

## 3. Depth

- Convention: `Fmatrix::build_projection_HAT` (`xrCore/_matrix.h:632-659`) is row-vector D3D style:
  `_33 = Q = f/(f-n)`, `_43 = -Q*n`, `_34 = 1`. Device depth is `d = _33 + _43/z_view`, standard (not
  inverted), finite far plane. World near is `VIEWPORT_NEAR = 0.2` (`device.h:21`; the camera passes its
  own near to `ApplyDevice`, `CameraManager.cpp:459`, so read `n = -_43/_33` from the captured matrix).
  The far plane is `CurrentEnv->far_plane`.
- First person: projection rebuilt with `psHUD_FOV*83` and `R_VIEWPORT_NEAR = 0.005`
  (`device.h:22`; `r__dsgraph_render.cpp:509-512`). The viewport depth range is [0, 0.02]
  (`r4.cpp:834-840`), so `d_hud = 0.02 * (Q_h + _43h/z)`.
- Sky: `sky2.vs:25` forces `z = w` (d = 1) under `rmFar` [0.99999, 1]. In the G-buffer, sky keeps the
  cleared position `z = 0` (`phase_scene.cpp:14-15`).
- **Primary method (D1)**: give SR the rasterized depth from the W DSV twin, created as
  `R24G8_TYPELESS` like the engine's own `CRT::create` path (`dx10SH_RT.cpp:79-82,162`). This is exact
  device depth for world, first-person and sky pixels, consistent with what was actually
  rasterized and with the stencil the world passes use. Set `depth_inverted = 0`. Leave
  `depth_infinite` at zero.
- Reference method (D2), from linear view Z: `d = _33 + _43/z` with the world matrix. Not used as
  the primary source, for three reasons: (1) first-person pixels cannot be told apart in the
  G-buffer, so D2 would put the weapon "inside" walls it renders in front of; (2) packed Z includes
  the parallax virtual-height offset (`deffer_base_flat.ps:34-39`), so it is not the rasterized
  surface; (3) sky Z = 0 needs a special case.
- Pipeline acceptance: the D3D12 path explicitly accepts `D24_UNORM_S8_UINT/R24G8_TYPELESS`
  (`native_sr_d3d12.cpp:201-204`). The D3D11 `dlss_pipeline`/`motion_resolve` route documents
  `R32_FLOAT` device depth (`motion_resolve.h:9-13`); whether it accepts R24G8 is **UNVERIFIED**. If
  it does not, copy depth to R32F with a small pass before submission (Stage 4 fixture).
- Camera reconstruction caveat: pixels without valid motion are reprojected with the *world*
  camera (`motion_resolve.h:9-13`). This is wrong for first-person pixels. Unwritten first-person
  pixels (`d < 0.02`; world geometry only reaches this within 2% of the near plane) should get zero
  screen motion, not world reprojection. The heuristic is **UNVERIFIED**.
- Output-size depth for P and later passes: tracers draw with `UIRender` after `Render` returns
  (`Level_bullet_manager.cpp:1031-1032`). Whether they depth-test `pBaseZB` is **UNVERIFIED**. To be
  safe, the seam writes a nearest-upsampled copy of the W depth into the engine's output `pBaseZB`
  (a full-screen `SV_Depth` pass; stencil cleared).

## 4. Jitter and mip bias

- Main projection: `CCameraManager::ApplyDevice` builds `Device.mProject` (`CameraManager.cpp:459`);
  `device.cpp:335-344` computes `mFullTransform` and calls `SetCacheXform`, which uploads to `RCache`
  (`dxRenderDeviceRender.cpp:411-415`). In H1, save the unjittered pair (also captured by H6), add the
  jitter, recompute `mFullTransform` with the engine's own multiplication order, and re-upload through
  the original `SetCacheXform` trampoline:
  `P._31 += 2*jx/W_r`, `P._32 -= 2*jy/H_r` (row vectors: `clip.x += jx_ndc * clip.w`; jx and jy are
  render pixels, X right, Y down). The emissive phase re-reads `Device.mProject`
  (`r4_R_render.cpp:506`), so it is jittered too. Restore both matrices at the seam, so motion-vector
  CPU history, flares, tracers and UI see the unjittered camera.
- First person: **six** sites in the shipped binary rebuild a projection locally, upload it with
  `R_xforms::set_P`, then call `rmNear()`: `r_dsgraph_render_hud` (`0x84ffa0`,
  `r__dsgraph_render.cpp:502-520`), `_hud_ui` (`0x8505a0`, 538-583), `_sorted` (`0x850cc0`, 595-613),
  `_emissive` (`0x84ec80`, 626-645), HUD-mode particles (`CParticleEffect::Render`, `0x8d8420`,
  `ParticleEffect.cpp:599-611`), and the sixth, missed in the first draft: HUD-mode point lights
  (`CRenderTarget::accum_point`, `0x90dca0`, `r4_rendertarget_accum_point.cpp:19-31`). The
  `ParticleEffect.cpp:785` copy is the `_EDITOR` build and is not compiled. The four `r_dsgraph` sites
  call `rmNear` through vtable slot 55; the particle and point-light sites call `0x83a450` directly. The
  verifier checks completeness: every `set_P` call in `.text` that is followed within 0x120 bytes by an
  `rmNear` call belongs to one of these six functions. H4: inside the W window, when
  `Device.mProject._31 == _32 == 0` (freshly built), apply the same pixel jitter, recompute
  `mFullTransform`, and call `R_xforms::set_P` (`R_Backend_xform.cpp:32-42`, out of line at `0x83ecd0`,
  `rcx = &RCache.xforms` `0x140D63C18`, `rdx = &matrix`, copied to `xforms+0xC0`). Each site restores
  `Device.mProject = Pold` (the jittered main matrix) afterwards, so no double jitter accumulates. The
  debug-only `rmNear` callers (`HOM.cpp:419`, `r__sector.cpp:51`) have no direct call in the binary and
  are also excluded by the W-window check.
- Untouched: sun and spot shadow passes set their own projections and viewports
  (`phase_smap_D.cpp:33-45`, `phase_smap_S.cpp:22-24`), as do SSAO (`phase_ssao.cpp:3-9`) and fluid.
  Full-screen passes use no projection.
- Known side effects (**UNVERIFIED** magnitude): culling and HOM use the jittered `mFullTransform`
  (subpixel, harmless). The motion-blur matrices in `phase_combine.cpp:126-140` include the jitter
  delta, which becomes micro-blur when `r2_mblur > 0`. `pos_decompression_params` ignores jitter, so
  lighting reconstruction is offset by at most half a render pixel.
- Motion convention: the core shader ABI subtracts the current and previous jitter in UV inside the
  PS (`core/README.md`, PS b13), so RTV2 motion is unjittered. Foliage previous projection is
  unjittered (`PROGRESS.md`).
- Sequence and history: Halton(2,3) with phase count `8 * (output/render)^2` (standard practice). A
  separate main-view counter: SVP frames do not advance it, and the native SR history rule requires
  `native_frame == previous + 1` (`native_sr.cpp:131-135`).
- Mip bias: the engine already toggles sampler state per phase by recreating every sampler through
  `SSManager.SetMaxAnisotropy` in `phase_scene_begin/disable_aniso` (`phase_scene.cpp:33,58-61`;
  `dx10SamplerStateCache.cpp:172-199`). Use `SSManager.SetMipLODBias(user + log2(render_w/output_w))`
  over the world window, then restore the user value `ps_r__tf_Mipbias`
  (`dxRenderDeviceRender.cpp:102`) at the seam. Forward materials (PDA, particles) are included,
  because the window spans `render_forward`. UI and post are excluded. Binary: `SSManager = 0x140D63A20`,
  `SetMipLODBias = 0x7fe170` (`rcx = &SSManager`, bias in `xmm1`; the current value is cached at
  `SSManager+0x19C`, and nothing is rebuilt when the value is unchanged), `SetMaxAnisotropy = 0x7fe060`,
  `ps_r__tf_Mipbias = 0x140D607A0`. All three `SetMipLODBias` references are tail jumps: `SetupStates`
  (`0x8232a0`) and two console handlers. Each change releases and recreates every sampler state, as
  `phase_scene_begin`'s `SetMaxAnisotropy` already does every frame.

## 5. Reinsertion at S-B, and what stays at output size

At the seam (H2 prologue, before the original `phase_sunshafts`/`phase_blur` runs), in order:

1. Adapter pass (Anomaly tonemap convention, stays in `games/anomaly`): W `generic0` and
   `generic_temp`, through f^-1, into render-size FP16 colour and opaque-only colour.
2. Motion preparation (section 6, Stage 4), depth SRV, and the exposure choice (2.6).
3. Queue a spatial fallback into the adapter's output-size FP16 target, then call
   `rsf_native_sr_evaluate(context, &pass)` (`native_sr.h:10-16`). Pass fields: `role = RSF_GAME_RENDER_SR`,
   render/output rects, `camera` (unjittered matrices converted to RSF's convention, plus jitter pixels),
   `previous_clip_to_clip`, `motion_to_uv = (1, 1)`, `history_key` for the main view, `native_frame` for
   the main view, `color_output` = the adapter's FP16 output-size target. The output format must match
   the pipeline's (`native_sr.cpp:178-185`). Inputs are rect-local at render size (149-153).
4. Re-tonemap into the **engine's** output-size UNORM8 `generic0`: `rgb = saturate(f(sr_out))`;
   alpha = nearest-upsampled W `generic0.a` (whether P consumers read it is **UNVERIFIED**).
5. Nearest-upsample W `rt_Position` (packed normal, view Z, mtl; never filtered) and W `rt_Color`
   into the engine originals. Upsample depth into the original `pBaseZB` (section 3).
6. Restore `Device.dwWidth/dwHeight`, `HW.pBaseZB`, the twinned surfaces except `generic1`, the
   unjittered matrices and the mip bias. Invalidate the `RCache` texture and constant caches.
   Call `rmNormal`.
7. Return to the original function. Everything from sunshafts to `phase_pp` runs stock at output size.

At H3 (`Render` epilogue): restore `generic1` to the original, and verify that the seam fired.
Defensive refusal: if a W frame reaches H3 without the seam, restore everything, log once and
disable SR. That frame is lost, but the next one renders stock.

Output-size consumers:

- Screen HUD and UI: `HUD().RenderUI()` runs after `Render` returns (`Level.cpp:779`), with all
  originals restored.
- PDA content: drawn into the backbuffer and copied into `rt_ui_pda` *before* `Render`
  (`Level.cpp:716-760`, `r4_R_render.cpp:584-587`). That copy stays at output size with no change.
  The physical screen material (`pda_overlay`, first-person sorted, forward) samples it at render size
  in the world window. Its surface pixels are reconstructed by SR like any other world surface. Its
  content animation needs the hints in `anomaly-world-space-ui.md`, not object motion. An
  output-size PDA surface pass remains an optional later experiment.
- Scope: SVP frames (`IsSVPFrame`, every `frameDelay` frames) are not eligible in H1, so they render
  stock at output size, and the backbuffer copy into `rt_secondVP` (`Level.cpp:776-777`) is unchanged.
  `End` suppresses their Present (`dxRenderDeviceRender.cpp:383-385`). Main-view SR history skips
  them (section 4). Cost: one native-size render every `frameDelay` frames while scoped. A cheaper
  render-size spatial path for SVP is a later optimisation.
- Menus, loading and first frame after reset: never reach H1.

## 6. Staged implementation plan

Rule 1 split: the twin swap, tonemap inverse, upsample passes, jitter and mip bias, motion
validity conversion and all RVAs live in `games/anomaly/`. Shared needs go into generic,
size-checked runtime ABIs.

**Stage 0: site mapping (no game run).** Static part done; see section 8. The Observe-mode live
acceptance below has not been run.
Find RVAs and 16-byte guards for H1, H2a/b, H4, `R_xforms::set_P`, `CTexture::surface_set`,
`SetMipLODBias` and the `SSManager` instance. Confirm the calls at `phase_combine.cpp:374,378` and
`r4_R_render.cpp:278,285` are real call instructions (no LTCG inlining). Map the `CRenderTarget`
`ref_rt` offsets through the constructor's `$user$...` string arguments, plus the `CRT`/`CTexture`
fields and the `CBackend` texture/constant cache fields.
Tests without the game: a manifest test that reads the installed exe file and checks every guard,
in the same pattern as the existing `native_profile` checks.
Live acceptance: Observe mode logs per-frame order `prepare → seam → Render exit` exactly once per
world frame, with zero on menu and SVP frames. Do not run IDA/Ghidra jobs during game runs.

**Stage 1: twin swap at scale 1.0 (render size = output size).**
Twins allocated in H5; FP16 `generic0`/`generic_temp`; f^-1 then f round trip; nearest upsample;
depth copy; no jitter; SR off.
Tests without the game: (a) a hardware D3D11 fixture of the f/f^-1 and upsample passes against the
CPU reference in `tonemap_inverse_check.py`; (b) a swap/restore unit test on mock `CRT`/`CTexture`
records with source layouts, including the H3 refusal path; (c) debug-layer validation of mixed
MRT formats and DSV twin binds.
Live acceptance: F12 captures of stock and scale 1.0 in l01_escape (day, night with volumetrics,
rain, PDA open, scope, menu, resize/`vid_restart`) differ only where clamping semantics changed.
No D3D11 debug-layer errors. About 8000 frames with no fault.

**Stage 2: spatial resolution reduction (0.5 to 0.77 per axis).**
Same seam; the adapter does a bilinear/spatial upscale instead of temporal SR.
Tests without the game: render/output plan arithmetic, and the resize/reset lifecycle.
Live acceptance: geometry and lighting are registered (no offset G-buffer or corner rendering).
HUD, PDA content and scope stay sharp at output size. Item-attached UI scales correctly (1.5
hazard). Fluid levels are checked or refused. GPU time is measured against stock.

**Stage 3: jitter and mip bias (spatial upscale still active).**
Tests without the game: the Halton sequence and NDC-offset formula, and the first-person no-double-jitter
rule, in temporal CPU tests.
Live acceptance: on a stationary scene, the per-frame raw world capture moves by the expected subpixel
offsets. The first-person weapon moves with the same offset (no relative shimmer). Shadow maps and UI
do not move. Texture sharpness matches the native-resolution reference.

**Stage 4: motion and depth inputs.**
Depends on the core/foliage MRT2 binding (`PROGRESS.md` checklist).
- The pipeline's decode treats a stored zero as "unwritten" only through `zero_means_unwritten`
  (`motion_decode.h:57-80`). Anomaly's valid stationary vector is `(0, 0, 1, 0)`, so a stationary
  first-person weapon would be misread as unwritten and reprojected with the world camera.
  **Proposal**: a generic ABI append to `rsf_motion_decode_params`, a validity channel index with a
  threshold. Interim: an adapter pass that writes the sentinel where validity is zero.
- Set `motion_camera_included = 0` so the resolve runs for sentinel pixels (`dlss_pipeline.cpp:752-772`;
  written vectors replace camera motion, `motion_resolve.h:9-13`). The flag's name versus this use
  is **UNVERIFIED** until a fixture confirms it.
- Unwritten first-person pixels get zero motion (section 3).
- R24 versus R32F depth acceptance.
Tests without the game: synthetic validity/sentinel decode fixture; depth-format fixture. Run the
shared-host test before any deploy, because ctest skips the D3D11-to-D3D12 Streamline path.
Live acceptance: raw-guide dumps (`rsf_input_dump`) for a camera pan, a strafing NPC, the weapon at
rest and in sway, sky, and particles.

**Stage 5: temporal SR.**
`rsf_dlss_pipeline_start` from the adapter, with Anomaly motion parameters,
`RSF_DLSS_ENGINE_CUSTOM` and `view_space_to_meters = 1`. The AC7 equivalent is
`loader/proxy/src/dlss_bridge.c:3740-3774`. Per frame, `rsf_native_sr_evaluate` at the seam.
Tests without the game: ABI and contract tests for pass construction (reset rules, rects, format
refusal).
Live acceptance, compared with native and spatial: stationary convergence, panning, weapon ghosting,
foliage, PDA text readability and scrolling, scope alternation with no history contamination, reset
on load and teardown, and the exposure A/B (2.6). Do not tune quality by forcing a DLSS preset.

**Stage 6: optional hints.**
Opaque-only colour from `generic_temp`; reactive and transparency masks from the forward contribution
(`|generic0 - generic_temp|`), localised to particles and the PDA. State which backend reads each:
FSR reactive/T&C, XeSS responsive mask, DLSS opaque colour (`dlss_pipeline.h:165-178`). Consider a
policy to disable SMAA and engine AA while SR is active, as a settings decision.

## 7. Unverified assumptions register

1. Resolved in Stage 0 (section 8): RVAs, guards and non-inlining for `phase_scene_prepare`,
   `phase_sunshafts`, `phase_blur`, the `rmNear`/`rmFar` slots, `R_xforms::set_P`, `CTexture::surface_set`
   and `SetMipLODBias`.
2. Resolved in Stage 0 (section 8): the `CRenderTarget` `ref_rt` offsets, the `CRT` fields, and the
   `CTexture`, `CBackend` cache and `Device.mFullTransform` layouts. Still open: the identity of the
   extra unconditional call `0x8f0670` in `phase_combine` after SMAA. It has no counterpart in source
   `028ccf5` and runs in P, after the seam.
3. D3D11 behaviour for RTV/DSV dimension mismatch in stock blur binds.
4. Constant and texture cache invalidation needed at the swap points.
5. UI code inside `Render` reading `Device.dwWidth`; the concurrent `seqFrameMT` reading dimensions;
   3D fluid screen size.
6. P consumers of `generic0.a`; tracer depth test against `pBaseZB`; use of `SE_SUN_LUMINANCE`.
7. Visual effect of unclamped FP16 writes before saturation; motion-blur jitter; lighting
   reconstruction offset.
8. D3D11 pipeline depth format acceptance; the meaning of `motion_camera_included` for sparse
   resolve; the first-person `d < 0.02` heuristic.
9. RSF matrix convention conversion from X-Ray row-vector storage (see the temporal README: native
   bytes must be converted, not copied).
10. Exposure choice (2.6); SVP cost at native size.
11. That source `028ccf5` matches the shipped binary for every function not mapped in section 8. The
    mapped functions match source structurally, with two differences: the sixth first-person site,
    `accum_point`, and the extra `phase_combine` call noted in item 2.

## 8. Stage 0 site map (standard DX11 executable)

Static evidence only. No game run, no hook installed, rendering unchanged. RVAs are relative to image
base `0x140000000`. The data lives in `games/anomaly/src/sr_profile.h`. `tests/anomaly_sr_evidence.py --verify`
re-derives every value from the installed exe and fails on any mismatch. Four negative runs, each
mutating one profile copy (a guard byte, the `generic0` slot, `crt_texture_offset`,
`backend_textures_ps_offset`), each failed with that name (`.local/anomaly/sr-stage0/negative.txt`).
All functions are x64 `__fastcall`, with `this` in `rcx`. Guards are the 16 bytes at the RVA; where 16
bytes are not unique in `.text`, the profile adds the shortest unique prefix (`sr_unique_guards`).

| Site | RVA | Guard (first 16 bytes; unique length) | Arguments | Source | How found |
| --- | --- | --- | --- | --- | --- |
| `CRenderTarget::phase_scene_prepare` (H1) | `0x8e4310` | `40534883ec50f7058c0f480000400000` (16) | `this` | `r4_rendertarget_phase_scene.cpp:4` | `Render` calls it in both z-prepass branches; body tests `o.dx10_msaa`, binds `rt_Position(+0x378)->pRT` with `HW.pBaseZB` |
| `CRenderTarget::phase_combine` | `0x8bc980` | `488bc45556415441554156488da888fe` (16) | `this` | `r4_rendertarget_phase_combine.cpp:39` | Last call of `Render`; starts with two `surface_set` calls on `t_LUM_src/dest` |
| `CRenderTarget::phase_sunshafts` (H2a) | `0x8f07c0` | `4c8bdc554156498d6ba14881ece80000` (16) | `this` | `rendertarget_phase_sunshafts.cpp:3` | Call after `!_menu_pp && ps_sunshafts_mode-2 <= 1` in `phase_combine` |
| `CRenderTarget::phase_blur` (H2b) | `0x8f2590` | `48895c242055565741564157488bec48` (17) | `this` | `rendertarget_phase_blur.cpp:3` | Unconditional call right after it, followed by `phase_pp_bloom`, `dof` (`R2FLAG_DOF`), `lut`, gas mask, night vision, SMAA |
| `CRender::rmNear` (H4) | `0x83a450` | `40534883ec40488b01ff507848c74424` (62) | `this` | `r4.cpp:834` | RTTI slot 55; already detoured by the geometry producer |
| `CRender::rmFar` | `0x83a3c0` | same 16 bytes (62) | `this` | `r4.cpp:843` | RTTI slot 56 |
| `R_xforms::set_P` | `0x83ecd0` | `48895c240848896c2410488974241848` (28) | `this=&RCache.xforms`, `rdx=const Fmatrix*` | `R_Backend_xform.cpp:32` | `SetCacheXform(view, project)` calls set_V then tail-jumps here; copies to `+0xC0` |
| `R_xforms::set_V` | `0x83f970` | `48895c240848896c2410488974241857` (26) | same | `R_Backend_xform.cpp:19` | Copies to `+0x80` |
| `CTexture::surface_set` | `0x87ac30` | `48895c24185556574883ec40488bfa48` (16) | `this`, `rdx=ID3D11Resource*` | `dx10SH_Texture.cpp:55` | Last call of `CRT::create` (`pTexture->surface_set(pSurface)`) |
| `dx10SamplerStateCache::SetMipLODBias` | `0x7fe170` | `40574881eca0000000f30f10819c0100` (16) | `this=&SSManager`, `xmm1=float` | `dx10SamplerStateCache.cpp:201` | Tail jump from `SetupStates` after `SetMaxAnisotropy` (`dxRenderDeviceRender.cpp:101-102`) |
| `dx10SamplerStateCache::SetMaxAnisotropy` | `0x7fe060` | `40574881eca0000000488bf983fa0173` (16) | `this`, `edx=u32` | `dx10SamplerStateCache.cpp:172` | First call of `phase_scene_begin` (`0x8e3d20`) |
| `CBackend::Invalidate` | `0x842c50` | `48895c240848896c2410488974241857` (28) | `this=&RCache` | `R_Backend_Runtime.cpp:63` | First call of `CBackend::OnFrameBegin` (`0x842fb0`, called from `dxRenderDeviceRender::Begin`) |
| `CRenderTarget::u_setrt(W,H,...)` | `0x859cb0` | `4883ec288951084489410c4c3b0d76a4` (16) | `this, edx=W, r8d=H, r9=RTV0, [rsp+28h..]=RTV1, RTV2, DSV` | `r4_rendertarget.cpp:111` | `Render` early-out; stores `W/H` at `+0x8/+0xC` |
| `PS::CParticleEffect::Render` | `0x8d8420` | `40534881ec80010000488bd9e87fa40f` (16) | `this`, `xmm1=float` | `ParticleEffect.cpp:575` | RTTI `CParticleEffect@PS` slot 7 |
| `CRenderTarget::accum_point` | `0x90dca0` | `488bc45553488da8d8feffff4881ec18` (16) | `this, rdx=light*` | `r4_rendertarget_accum_point.cpp:3` | Direct `rmNear` caller after `set_P(&Device.mProject)` with a psHUD_FOV projection |

Real call instructions (`sr_call_sites`, 16 bytes from the `E8`): `Render → phase_scene_prepare` at
`0x897d2c` (z-prepass, `r4_R_render.cpp:278`) and `0x897d66` (285); `Render → phase_combine` at `0x8985bf` (538);
`phase_combine → phase_sunshafts` at `0x8bf273` (374); `phase_combine → phase_blur` at `0x8bf27b` (378). Each
callee has exactly these callers in `.text`, and each passes `this` from `CRender+0x768` (`Render`) or `r14`
(`phase_combine`). LTCG inlined none of them, and no duplicate bodies exist.

First-person `set_P → rmNear` pairs (`sr_first_person_sites`): hud `0x850410 → 0x85041b`, hud_ui
`0x8509b5 → 0x850ab4`, sorted `0x851135 → 0x851140`, emissive `0x84f0f5 → 0x84f100`, particles
`0x8d8a8e → 0x8d8a9a`, point lights `0x90e16c → 0x90e178`. `psHUD_FOV = 0x140D37F78`.

Globals and layouts (all re-derived by the verifier): `Device.dwWidth/dwHeight 0xd37668/0xd3766c`,
`Device.mView/mProject/mFullTransform 0xd376c8/0xd37708/0xd37748`; `HW.pDevice/pContext/m_pSwapChain/pBaseRT/pBaseZB
0xceb8d0/0xceb8d8/0xceb8e0/0xceb8e8/0xceb8f0`; `RImplementation 0xd64e40` (`Target +0x768`; option words
`0xd652a0` (`mrtmixdepth` bit 26, `fp16_blend` bit 28) and `0xd652ac` (`advancedpp` bit 12, `dx10_msaa`
bit 14)); `ps_sunshafts_mode 0xceb36c`; `RCache 0xd63bc0` with the cache fields in 1.2; `SSManager 0xd63a20`
(`m_uiMipLODBias +0x19C`); `ps_r__tf_Mipbias 0xd607a0`, `ps_r__tf_Anisotropic 0xceb878`; the
`CRenderTarget`, `CRT` and `CTexture` layouts in 1.1 and 1.4. The IDA database names all of these
(`.local/anomaly/sr-stage0/ida/AnomalyDX11.exe.i64`).

How sites were found, and lookups that failed first:

- `phase_scene_prepare` and `phase_combine` came from the `CRender::Render` decompile, matched against
  `r4_R_render.cpp` call order. The `$user$...` strings led to the `CRenderTarget` constructor.
- `set_P`/`set_V` were first swapped: the decompiler showed `Render` calling `0x83ecd0` with `0xd37708`
  and `0x83f970` with `0xd376c8`, and these were first read as view and projection. The z-prepass `m_zfill`
  product and `SetCacheXform(view, project)` (set_V then set_P) settled it: `0xd376c8` is `mView`, and
  `set_P = 0x83ecd0` writes `m_p` at `+0xC0` as in source.
- `rmFar` was first guessed at `0x83a490` from the gap between `rmNear` and `rmNormal`. That address is
  inside an instruction; the RTTI slot 56 gives `0x83a3c0`.
- `CParticleEffect` RTTI is not found under `.?AVCParticleEffect@@`; it is namespaced (`CParticleEffect@PS`).
- The source text lists five first-person sites. A binary scan of every `set_P` call followed by `rmNear`
  found the sixth (`accum_point`, HUD-mode lights). The `ParticleEffect.cpp:785` copy is editor-only.
- `SetMipLODBias` has no `E8` callers. All three references are `E9` tail jumps, found through IDA xrefs.
- `.pdata` splits large functions into chained unwind chunks. The verifier follows `UNW_FLAG_CHAININFO`
  to attribute call sites to their primary function.
- `phase_combine` contains an unconditional call to `0x8f0670` after SMAA that has no counterpart in source
  `028ccf5`. It is after the seam and was not identified.

Not found / still open: nothing on the Stage 0 list remains unmapped. Runtime properties are still
**UNVERIFIED**: constant-cache staleness across a swap, D3D11 RTV/DSV size-mismatch behaviour, and the
`seqFrameMT` dimension reads. They need Stage 1's debug-layer and live runs.

Stage 1 readiness: every site the scale-1.0 twin swap needs is mapped and guarded: H1, H2a/b, H3, H5
(`reset_begin/end`, already guarded), `rmNormal`, `Device` dimensions, `HW.pBaseZB`, the `generic0`/`generic_temp`/`generic1`/`rt_Position`/`rt_Color`
slots, the `CRT`/`CTexture` fields for swap (b), and the `RCache` texture-cache fields. H4 and mip bias are
mapped but not needed until Stage 3.

## Sources

Engine source (read-only, `D:/xray-monolith`): `src/Layers/xrRenderPC_R4/{r4_rendertarget.cpp,
r4_rendertarget_phase_{combine,scene,PP,bloom,luminance,smap_D,smap_S,ssao}.cpp, r4_R_render.cpp,
r4.cpp, blender_combine.cpp, blender_light_direct.cpp, blender_bloom_build.cpp, r2_types.h}`,
`src/Layers/xrRender/{rendertarget_phase_*.cpp, r__dsgraph_render.cpp, ParticleEffect.cpp,
R_Backend_Runtime.cpp, R_Backend_xform.cpp, dxRenderDeviceRender.cpp, Blender_Recorder_StandartBinding.cpp,
SH_RT.h, HWCaps.cpp}`, `src/Layers/xrRenderDX10/{dx10HW.cpp, dx10SH_RT.cpp, dx10SH_Texture.cpp,
dx10R_Backend_Runtime.h, StateManager/dx10SamplerStateCache.cpp}`, `src/xrEngine/{device.cpp, device.h,
CameraManager.cpp, Render.h, IGame_Level.cpp}`, `src/xrCore/_matrix.h`, `src/xrGame/{Level.cpp,
HUDManager.cpp, Level_bullet_manager.cpp}`.

Shaders (local, not redistributed): `combine_1.{ps,vs}`, `combine_2_naa.ps`, `common_functions.h`,
`common_defines.h`, `sky2.{ps,vs}`, `clouds.{ps,vs}`, `combine_volumetric.ps`, `bloom_build.ps`,
`gbuffer_stage.h`, `stub_screen_space.vs`, `depth_of_field.ps`, `ogse_sunshafts_mask.ps`, `distort.s`.

RSF: `runtime/orchestrator/include/rescaleframe/{native_sr.h, dlss_pipeline.h, sr_bridge.h,
native_sr_d3d12.h, d3d11_present_api.h}`, `runtime/orchestrator/src/{native_sr.cpp, dlss_pipeline.cpp,
sr_bridge.cpp, sr_legacy_adapter.cpp, native_sr_d3d12.cpp}`, `runtime/graphics/include/rescaleframe/
{motion_decode.h, motion_resolve.h}`, `sdk/game/include/rescaleframe/{game_renderer.h, game_frame.h}`,
`loader/proxy/src/dlss_bridge.c`, `games/anomaly/shaders/core/README.md`,
`docs/research/anomaly-{native-hooks,world-space-ui}.md`, `tools/anomaly_shader_README.md`.

Binary: `.local/anomaly/rea-evidence/0003-procedure_pseudo_code.json` (CRender::Render, first 100
of 484 lines), `0012-procedure_pseudo_code.json` (CRender::RenderToTarget),
`.local/anomaly/native-sites-dx11.json`. Stage 0: IDA database `.local/anomaly/sr-stage0/ida/AnomalyDX11.exe.i64`
(private copy, same SHA-256), `games/anomaly/src/sr_profile.h`, `tests/anomaly_sr_evidence.py`,
`tests/anomaly_sr_profile.cpp`, `.local/anomaly/sr-stage0/{verify,negative,build,ctest}.txt|log`. Extra
source read for Stage 0: `xrRender/{R_Backend.h, r_backend_xform.h, SH_Texture.h, xrRender_console.cpp,
ParticleEffect.cpp}`, `xrRenderPC_R4/{r4.h, r4_rendertarget_accum_point.cpp}`,
`xrRenderDX10/StateManager/dx10SamplerStateCache.h`.
