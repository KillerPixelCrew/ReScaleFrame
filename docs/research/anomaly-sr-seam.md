# Anomaly SR seam: world/output resolution split and linear-colour boundary

Status: source and retained-binary research plus a design. Stage 1 (section 9) is built,
fixture-tested and game-tested in Observe mode (`.local/anomaly/wave4-game/result.json`). Stage 2
(section 10) is built and game-accepted at 3ac6cb0 (`.local/anomaly/wave5-game/result.json`). Stage 3
(section 11) is built and fixture-tested behind `SRStage=3`; it has not been injected or
game-tested. Stages 4 and 5 (section 12) are built and fixture-tested behind `SRStage=4` and `SRStage=5`,
with real DLSS, FSR3, FSR4 and XeSS evaluations through the adapter's input path on an RTX 4070 Laptop GPU;
they have not been injected or game-tested. With the keys off, no hook below changes rendering. Every statement is either cited
to source/binary evidence or marked **UNVERIFIED**.

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
- G-buffer reconstruction: `pos_decompression_params` uses `Device.fFOV`, `Device.fASPECT` and
  `Device.dwWidth/Height`; `pos_decompression_params2` contains dimensions only (`r4.cpp:106-125`). The
  Stage 3 binder correction below adds the raster center offset. `gbuffer_load_data` (`gbuffer_stage.h:115-143`) uses
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
| `screen_res`, `pos_decompression_params` | `StandartBinding.cpp:389-396`, `r4.cpp:106-125` | Dimension swap and forced ctable invalidation at both boundaries (implemented in Stage 2); binders run again on the next constant-table bind. The Stage 3 binder correction below also supplies raster jitter |
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
- Camera consumers: sun cascade fitting temporarily borrows the saved unjittered Device camera. Deferred
  position reconstruction uses the saved raster jitter through the guarded constant binder correction below.
  Culling/HOM boundaries and motion blur remain subject to the acceptance limits in that consumer audit;
  their effects have not been measured in this task.
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
Live acceptance: Observe mode logs per-frame order `prepare Ã¢â€ â€™ seam Ã¢â€ â€™ Render exit` exactly once per
world frame, with zero on menu and SVP frames. Do not run IDA/Ghidra jobs during game runs.

**Stage 1: twin swap at scale 1.0 (render size = output size).** Built; see section 9 for what was
implemented, what was deferred and the live acceptance comparison.
Original plan: twins allocated in H5; FP16 `generic0`/`generic_temp`; f^-1 then f round trip; nearest upsample;
depth copy; no jitter; SR off.
Tests without the game: (a) a hardware D3D11 fixture of the f/f^-1 and upsample passes against the
CPU reference in `tonemap_inverse_check.py`; (b) a swap/restore unit test on mock `CRT`/`CTexture`
records with source layouts, including the H3 refusal path; (c) debug-layer validation of mixed
MRT formats and DSV twin binds.
Live acceptance: F12 captures of stock and scale 1.0 in l01_escape (day, night with volumetrics,
rain, PDA open, scope, menu, resize/`vid_restart`) differ only where clamping semantics changed.
No D3D11 debug-layer errors. About 8000 frames with no fault.

**Stage 2: spatial resolution reduction (0.5 to 0.77 per axis).** Built; see section 10.
Same seam; the adapter does a bilinear/spatial upscale instead of temporal SR.
Tests without the game: render/output plan arithmetic, and the resize/reset lifecycle.
Live acceptance: geometry and lighting are registered (no offset G-buffer or corner rendering).
HUD, PDA content and scope stay sharp at output size. Item-attached UI scales correctly (1.5
hazard). Fluid levels are checked or refused. GPU time is measured against stock.

**Stage 3: jitter and mip bias (spatial upscale still active).** Built; see section 11.
Tests without the game: the Halton sequence and NDC-offset formula, and the first-person no-double-jitter
rule, in temporal CPU tests.
Live acceptance: on a stationary scene, the per-frame raw world capture moves by the expected subpixel
offsets. The first-person weapon moves with the same offset (no relative shimmer). Shadow maps and UI
do not move. Texture sharpness matches the native-resolution reference.

**Stage 4: motion and depth inputs.** Built; see section 12.
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

**Stage 5: temporal SR.** Built; see section 12.
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
8. Resolved by fixture (section 12): the D3D11 pipeline accepts the R24G8 depth twin (the dense resolve reads it
   through `R24_UNORM_X8` and hands backends an `R32_FLOAT` copy), and `motion_camera_included = 0` makes the
   resolve reconstruct camera motion for sentinel pixels while written vectors replace it. Still open: the
   first-person `d < 0.02` heuristic in game.
9. Resolved (section 12): `rsf_camera_frame` is row-major with row vectors, like X-Ray's `Fmatrix`, so the
   CPU-side `Device.mView`/`mProject` bytes are copied; only the uploaded shader constants are transposed.
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

Real call instructions (`sr_call_sites`, 16 bytes from the `E8`): `Render Ã¢â€ â€™ phase_scene_prepare` at
`0x897d2c` (z-prepass, `r4_R_render.cpp:278`) and `0x897d66` (285); `Render Ã¢â€ â€™ phase_combine` at `0x8985bf` (538);
`phase_combine Ã¢â€ â€™ phase_sunshafts` at `0x8bf273` (374); `phase_combine Ã¢â€ â€™ phase_blur` at `0x8bf27b` (378). Each
callee has exactly these callers in `.text`, and each passes `this` from `CRender+0x768` (`Render`) or `r14`
(`phase_combine`). LTCG inlined none of them, and no duplicate bodies exist.

First-person `set_P Ã¢â€ â€™ rmNear` pairs (`sr_first_person_sites`): hud `0x850410 Ã¢â€ â€™ 0x85041b`, hud_ui
`0x8509b5 Ã¢â€ â€™ 0x850ab4`, sorted `0x851135 Ã¢â€ â€™ 0x851140`, emissive `0x84f0f5 Ã¢â€ â€™ 0x84f100`, particles
`0x8d8a8e Ã¢â€ â€™ 0x8d8a9a`, point lights `0x90e16c Ã¢â€ â€™ 0x90e178`. `psHUD_FOV = 0x140D37F78`.

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

## 9. Stage 1 implementation (scale 1.0)

12 November 2026. Built and tested without the game. Not injected, not game-tested. No SR provider is
called and rendering-readiness flags stay 0.

**Selection.** `[Anomaly] SRStage=1` swaps; `SRStageCapture=1` dumps frames (optional
`SRStageCaptureStart=600`, `SRStageCaptureCount=3` (at most 16), `SRStageCaptureInterval=300`, all in
world frames). Both require `Mode=Observe`, because the native observer's `prepare()` verifies the
executable. `SRStage` values other than 1 are off. `SRStageCapture=1` with `SRStage=0` installs the same
hooks but never swaps, so a stock run can dump the same seam image.

**Code.** `games/anomaly/src/sr_swap.{h,cpp}` (state machine, no D3D), `sr_d3d11.{h,cpp}` (twins and the
pass), `sr_stage.{h,cpp}` (hooks, engine reads, diagnostics, capture), `sr_tonemap.h` (CPU reference) and
`shaders/sr/rsf_sr_roundtrip.hlsl` (RSF-authored, embedded at build time, compiled at runtime with the
system `d3dcompiler_47`). `bootstrap.cpp` reads the keys; `native_observer.cpp` installs Stage 1 after the
geometry hooks, calls `sr::frame_end()` inside its existing `IRenderDeviceRender::End` detour before the
original End (so before Present), adds the SR lines to its 120-frame log, and calls `sr::quiesce()`.

**Hooks.** One guarded set of four: H1 `phase_scene_prepare`, H2a `phase_sunshafts`, H2b `phase_blur`, and
H3 on `phase_combine` (its epilogue is `CRender::Render`'s epilogue for world frames, since
`phase_combine` is the last call; this avoids a second detour on `Render`, which the observer owns).
Before installing, all four 16-byte prologues, the 17-byte `phase_blur` guard and all five
`sr_call_sites` must match. Any mismatch, or a `HookSet` refusal, logs the site name and leaves the game
stock. `rmNear` is not patched again: `native_geometry` now has a callback slot
(`geometry_set_rm_near_callback`) that its own detour calls after the original. Stage 1 only counts calls
inside the window; Stage 3 jitter will use the slot. If the geometry hooks were refused, the slot never
fires and the install log says `rmNear slot=unavailable`.

**Swap (mechanics b).** At H1, when the frame is eligible, `generic0` (`+0x3A8`) and `generic_temp` (`+0x3B8`)
exchange `CRT::{pSurface, pRT, pZRT, pUAView, dwWidth, dwHeight, fmt}` and
`CTexture::{pSurface, desc_cache, desc, m_pSRView}` with RSF-owned `R16G16B16A16_FLOAT` twins. Reference
counts are not touched. Before the exchange each target must be a colour CRT of device size and
A8R8G8B8 whose CTexture binds the same surface, and `this` must equal `CRender::Target`. Otherwise the
stage refuses for the rest of the process. After every exchange and restore, `RCache.T` and all
per-stage `CTexture*` slots are cleared (section 1.2). Stage 1 does **not** swap `Device.dwWidth/dwHeight`
or `HW.pBaseZB`, and has no depth twin and no jitter. At scale 1.0 those are identities; they arrive with
Stage 2.

Eligibility: the swap is selected and not refused; not an SVP frame (the geometry `IsSVPFrame` reader, and an
unreadable value counts as SVP); MSAA off (`dx10_msaa` bit); valid layout; twins available. Menu, loading and
first-after-reset frames never reach H1. Twins are keyed on `CRenderTarget*`, both CRT and surface
addresses, `HW.pDevice`, the observer's reset count and the device size. Any change (reset,
`vid_restart`, resolution change, new device) drops them and creates new ones on the next eligible frame.

Exits, all restoring the exact original bytes: the seam (normal); H3 with the window still open (seam
missed: restore, then refuse); the next H1 finding a stale window; `frame_end` finding a window
(defensive: restore, then refuse); quiesce; and rollback when an engine write faults part way. A field that
changed inside the window (as `surface_set` would) is still restored, counted as `foreign`, and refuses
further swaps.

**Round trip.** After the restore at the seam, one full-screen draw reads the twin and writes
`saturate(f(f^-1(y)))` into the engine's own UNORM8 `generic0` RTV, with nearest mapping (identity at
scale 1.0). The inverse is the cancellation-free form from 2.2. Inputs are clamped to `[0, 65504]`
first, so negatives and NaN give 0 and +inf gives 255, which is what the stock UNORM8 conversion stores.
Alpha is `saturate(a)`. The complete immediate-context state is saved and restored around the draw
(`rsf_d3d11_state_save`), so the engine's RCache, StateManager and SRV caches stay truthful.

**Diagnostics** (the observer's 120-frame log, plus refusal lines when they happen):

```text
Anomaly SR stage window frames= no_world= prepare= eligible= swapped= skipped off= scope= msaa= layout= twins= | seams= passthrough= round_trips= rt_failed= | restores seam= combine_end= stale_prepare= frame_end= shutdown= rollback= | rm_near_in_window= foreign= write_faults=
Anomaly SR stage totals stage= swapped= restores= round_trips= twin_generation= twin_drops= twin_refs_at_drop= captures= images= capture_failed= refused=
```

`twin_refs_at_drop` is diagnostic. Stock `phase_combine` calls `surface_get()` (an AddRef) on both copy
operands and never releases them, so the engine leaks references on its own targets every frame, and during the window on the
twins too. Dropped twins are therefore never actually freed until the process ends: about 2 Ãƒâ€” 8 bytes
per pixel per reset.

**Capture** (`SRStageCapture=1`, the core frame dump service, default directory
`%LOCALAPPDATA%/ReScaleFrame/Anomaly/website/scene-*` or `RSF_DUMP_DIR`). On scheduled main-view world frames,
and on F10: `anomaly_seam_fp16_input` (the FP16 twin at the seam, SRStage=1 only),
`anomaly_seam_generic0` (the engine UNORM8 `generic0` as every post pass receives it: after the round trip,
or stock at the seam) and `final_color` (the back buffer before Present, with UI). Each image has a PNG
and a raw `.bin`.

**Tests without the game** (`build/wk-sr1`, Release; registered in `tests/anomaly_support.cmake`):

- `anomaly_sr_swap_contract`: 14 cases on fake CRenderTarget/CRT/CTexture/RCache memory with profile offsets.
  Normal frame (sunshafts seam, blur pass-through), blur-only seam, combine without seam, stale prepare,
  window at frame end, shutdown mid-window, SVP/MSAA/menu/stock frames, seven layout mismatches, twin
  failures, a write fault at each of the 22 exchange writes, foreign change, round-trip failure,
  reset/resize/new-surface/new-device recreation. After every exit path the engine bytes are compared with
  the original bytes.
- `anomaly_sr_hooks_contract`: the geometry `rmNear` detour (redirected onto a fixture) calls the slot only
  while registered and only while enabled. `sr::install` refuses on an empty image, a wrong image size, the
  17th `phase_blur` byte, a call site that is no longer `E8`, and guarded bytes outside a real PE image, and
  writes nothing in each case. INI parsing is also covered.
- `anomaly_sr_roundtrip_device` on Intel UHD Graphics (hardware, debug layer on): all 65,536 FP16 patterns in
  four channels gave 262,122 of 262,144 exact and a worst difference of 1 code value. The stock curve
  `f(x)` for x in [0, 1e4] (65,536 samples) gave 64,888 exact and a worst difference of 1 against float64
  `f` written to UNORM8. The float64 `f^-1(f(x))` relative error is 4.9e-16. Nearest mapping, state
  preservation and twin creation/refusal also pass. Mixed FP16 + UNORM8 MRT with a D24S8 DSV produced 0
  debug-layer errors. Informational, register item 3: a 64x64 RTV with a 128x128 DSV (stock `phase_blur`)
  raises a debug-layer error, but the runtime still binds both.
- `ctest -R "anomaly|thin"`: 63 of 64 passed. The failure is `anomaly_overlay_visibility`, which is built
  only from the overlay sources another worker is changing. `py -3 -I tests/anomaly_sr_evidence.py --verify`
  passes. Logs: `.local/anomaly/sr-stage1/`.

**Live acceptance (to run).** Use the same save in l01_escape, the same standing view and the same time of day,
with weather fixed if possible. Run A: `Mode=Observe`, `SRStageCapture=1`, `SRStage=0`. Run B: the same with
`SRStage=1`. Compare `anomaly_seam_generic0.bin` between A and B. It should be identical apart from
clamping-semantics areas (2.4 item 1: overbright sky, additive particles, stacked blends crossing 1) and
temporal noise. Compare `final_color` the same way. Per-frame variation (grain, rain, animated foliage) has
to be bounded by the A-vs-A difference between two stock captures. Then F12 at the same view with UI,
plus PDA open, scope (SVP frames: `skipped scope` grows, `swapped` does not), menu (`no_world` grows),
`vid_restart` and a resolution change (`twin_generation` and `twin_drops` grow by one each). Expected
in a steady world view: `eligible == swapped == seams == round_trips == restores seam` per window; and
`combine_end = stale_prepare = frame_end = rollback = foreign = write_faults = rt_failed = layout =
twins = 0`; `refused=no`. Also expected: no D3D11 debug-layer errors when the layer is enabled, and
about 8000 frames without a fault.

Open or **UNVERIFIED**: whether any W-phase shader samples `$user$generic0` before `phase_combine` clears it.
If one does, it now sees last frame's FP16 twin instead of last frame's post output; only `distort.s`
samples it, and it does so in the forward pass. Also open: the effect of unclamped FP16 blending, the stock
`seqFrameMT` dimension reads (irrelevant at 1.0), and the `generic0.a` consumers.

## 10. Stage 2 implementation (render size below output, bilinear at the seam)

12 November 2026. Built and tested without the game. Not injected, not game-tested. No SR provider is
called and rendering-readiness flags stay 0.

**Selection.** `[Anomaly] SRStage=2` with `SRScale=<0.5..1.0>` (default 0.67), `Mode=Observe` as for Stage 1.
`SRScale` is parsed as a decimal number; missing, unparsable, non-positive or non-finite values select 0.67,
values outside the range are clamped (`sr_plan.h`). The key is re-read after every device reset, so a new value
applies at the next `vid_restart`. `SRStageCapture` works unchanged. `SRStage` values other than 1 and 2 are off.

**Render size.** One scale for both axes: `render = floor(output * scale + 0.5)`, kept inside
`[ceil(output / 2), output]` and never below 1 (1920x1080 at 0.67 gives 1286x724; 1280x720 gives 858x482). The
render aspect differs from `Device.fASPECT` by the rounding only: the projection is unchanged, and the pixel-to-NDC
mapping (`pos_decompression_params`, the viewport) uses the render dimensions, so both cover the same frustum.

**What the world window swaps.** At H1, on an eligible frame, in this order (all in `sr_swap.cpp`):

| Field | Window value | Evidence and reason |
| --- | --- | --- |
| The nine `sr_world_targets` (`generic0`, `generic_temp`, `rt_Position`, `rt_Color`, `rt_Accumulator`, `generic1`, and when their option is on `generic2`, `rt_half_depth`, `rt_ssao_temp`) | Render-size twins: `generic0`/`generic_temp` FP16 (section 2.4), the others in the engine's own format, with views built from the engine views' descriptions and a UAV exactly when the engine target has one | Every target the W phase renders into (1.3). `rt_half_depth` keeps the engine's halving: `Device.dwWidth / 2` with `o.ssao_half_data` (ctor `0x85544b`: `test al,8; shr r8d,1; shr r9d,1`), so its twin is `render / 2`. Targets of paths Stage 2 does not twin (`msaadepth`, `generic0_r`, `generic1_r`, `accum_temp`) must be null, else the stage refuses |
| `Device.dwWidth/dwHeight` | Render size | Every full-screen quad, light scissor and binder inside the window reads them (table below) |
| `HW.pBaseZB` | Render-size depth twin, `R24G8_TYPELESS` with a `D24_UNORM_S8_UINT` DSV and an `R24_UNORM_X8` SRV | Section 3. The engine DSV must be a single-sample D24S8 Texture2D view of output size, else `skipped depth` and refusal |
| `HW.pBaseRT` | A render-size stand-in RTV in the back buffer view's format | `phase_occq` binds `HW.pBaseRT` with `HW.pBaseZB` (`r4_rendertarget_phase_occq.cpp:6`, colour writes off). The fixture shows D3D11 **drops** an output-size RTV with a render-size DSV entirely, so without the stand-in the occlusion queries would run against whatever was bound before. Nothing else in the window binds `HW.pBaseRT`; the back buffer is rewritten by `phase_pp` |
| SSAO `set_viewport`'s static `D3D11_VIEWPORT` (`0xceebd0`) | `render / 2`, only when its magic-static guard (`0xd79284`) says initialised | `r4_rendertarget_phase_ssao.cpp:3-10` declares `static D3D_VIEWPORT viewport[1] = {0, 0, w, h, 0, 1}`: the first call fixes the size for the life of the process (binary: `cmp [guard], tls_epoch; jg init`, the init path stores only Width/Height). Its callers (`phase_downsamp` with `ssao_half_data`, `phase_ssao` with `ssao_blur_on` and no `ssao_opt_data`) always pass `Device / 2` first. In the window it must describe render/2, outside output/2. If the first call of the process happens inside a window, the restore writes `output / 2`, what stock would have stored |
| `RCache` texture cache (Stage 1) and `RCache.ctable` | Cleared after the exchange and after the restore | `set_Constants` returns early on `ctable == C` (`R_Backend_Runtime`); clearing it re-runs every binder (`screen_res`, `pos_decompression_params{,2}`) with the current `Device` size at the next `set_Element` |

After the original `phase_scene_prepare` returns (H1 epilogue) the viewport is set to `{0, 0, render, 0..1}`, what
`rmNormal` gives for a render-size target: `phase_scene_prepare` binds `rt_Position` with
`u_setrt(Device.dwWidth, ...)`, but the viewport is still the one `CRender::Render`'s first `rmNormal` set from the
previous frame's output-size target, and the z-prepass draws before any other `rmNormal`.

**Seam (S-B).** Restore every field above (a changed `Device` size, `HW.pBaseZB` or `HW.pBaseRT`, or a changed twinned
field, counts as `foreign` and refuses further swaps), write `CRenderTarget::dwWidth/dwHeight` = output (they follow
the last `u_setrt`, a twin), invalidate the caches, then one full-screen draw at output size
(`shaders/sr/rsf_sr_upscale.hlsl`):

- `SV_Target0` engine UNORM8 `generic0` = `saturate(f(bilinear(f^-1(y))))`: the filter runs on the exposure-scaled
  linear `x`, with texel-centre alignment and clamped neighbours. Alpha is nearest.
- `SV_Target1` engine `rt_Position` = nearest (packed normal and view Z are never filtered). P reads it in
  `blur`, `dof`, `nightvision`, `combine_2` and the sunshafts mask (`blender_{blur,dof,nightvision,combine}.cpp`).
- `SV_Target2` engine `generic1` = bilinear (the distortion mask, read by UV in `combine_2_naa.ps:48`). This deviates
  from 1.3 ("kept through P"): restoring it at the seam with the rest leaves a single exit point and no render-size
  resource visible to P, for one extra 4-byte output per pixel.
- `SV_Depth` engine `HW.pBaseZB` = nearest depth twin; its stencil is cleared to 0 first. No pass after the seam tests
  stencil against it (SMAA, the only post pass that enables stencil, binds no depth view).
- `rt_Color` is not upsampled: every P pass that binds it (`dof`, `lut`, `gasmask_*`, `nightvision`, `smaa`,
  `combine_2`) writes it full screen before anything reads it, and no P blender samples `$user$albedo`.
- Then the viewport returns to output size. Everything from sunshafts to `phase_pp` runs stock.

At scale 1.0 the pass equals the Stage 1 round trip bit for bit (fixture), so `SRStage=2 SRScale=1.0` exercises
every Stage 2 swap at output size and is the first A/B against stock.

**Exits and refusals.** As Stage 1, extended to the globals: seam (normal), `combine_end` without a seam (restore,
viewport to output, refuse), stale prepare (the H1 hook closes it before it reads `Device.dwWidth`), `frame_end`,
shutdown (engine memory only, no D3D call) and rollback (a failed write part way restores every field written so far,
including the globals). Ineligible frames (SVP scope, MSAA, menu/no world, swap disabled) touch nothing. Layout or
depth mismatches refuse for the process. An allocation failure (`CreateTexture2D` and friends) is not a refusal:
that frame and every frame with the same twin key stay stock (`skipped twins`, `alloc_failed`), and a new key (scale,
size, reset) tries again.

**Twin lifetime.** The key is the stage, `CRenderTarget*`, every CRT and surface address, `HW.pBaseZB`, `HW.pBaseRT`,
`HW.pDevice`, the observer's reset count, output and render size. Any change (scale, resolution, `vid_restart`, a new
view or device) drops the twins and builds new ones at the next eligible frame (`twin_generation` and `twin_drops`
grow by one). As in Stage 1, the engine leaks `surface_get` references on the `generic0`/`generic_temp` twins, so
those two are freed only at process end. At 1920x1080 and 0.67 the twins take about 53 MiB with every option on
(the totals line prints `twin_mib`).

**Size- and projection-dependent code inside the window** (requirement 2; engine source `028ccf5`):

| Site | Reads | Handling |
| --- | --- | --- |
| `screen_res` binder (`Blender_Recorder_StandartBinding.cpp:389-396`), `pos_decompression_params{,2}` (`r4.cpp:106-125`) | `Device.dwWidth/dwHeight` (and `fFOV`, `fASPECT`) at constant-table bind | Device swap plus `ctable` invalidation at both swap points |
| Full-screen quads and jitter tiling: `phase_combine.cpp:215-216` (combine_1), `accum_direct.cpp:53,227,367,550,755,838,981,1063,1095`, `accum_reflected.cpp:30`, `draw_rain.cpp:10,172`, `phase_scene.cpp:84` (albedo_wo only), `phase_bloom.cpp:85,352` (texel offsets into the render-size `generic1`; kernel aspect) | `Device` size per call | Device swap |
| `phase_scene_prepare`, terrain prepass, `phase_occq` `u_setrt(Device.dwWidth, ...)` (`phase_scene.cpp:10`, `r4_R_render.cpp:327`, `phase_occq.cpp:6`) | `Device` size, `HW.pBaseZB`, `HW.pBaseRT` | Device, depth and back-buffer stand-in swaps |
| Light scissor (`r4_rendertarget_enable_scissor.cpp:102-106`) | `Device` size | Device swap |
| `rmNear/rmFar/rmNormal` (`r4.cpp:834-858`) | Last bound target (`CRenderTarget::dwWidth`) | Twins carry render size; H1 epilogue viewport; seam writes output back |
| SSAO: `phase_downsamp` (`phase_ssao.cpp:127-132`), `phase_ssao` (50-56), `set_viewport` static | `Device` size, a viewport frozen at the first call | Device swap; static viewport swap (above) |
| HDAO dispatch (`phase_hdao.cpp:37-38`) | `CRenderTarget::dwWidth/dwHeight` | Render size after the twin binds |
| 3D fluid (`dx103DFluidRenderer.cpp:440-490`, `fluid_raydata_*.ps:10`, `fluid_common_render.h:169,283`) | Ray-data targets sized at create/reset from the output (`r4.cpp:501,587`); scene depth and ray data sampled by UV; composite with `PrepareCBuffer(Device.dwWidth)` | Registered by construction (UV sampling, own `rmNormal` per target); no saving for the ray-cast part. **UNVERIFIED** in game |
| SSAO noise/kernel factors (`phase_combine.cpp:176-182`, `phase_ssao.cpp:39-46`), shadow maps (`phase_smap_D.cpp:33`), bloom/luminance fixed targets, `Device.mProject` | FOV, own viewports, fixed sizes, NDC | Unaffected |
| Item-attached UI `g_hud->RenderActiveItemUI()` (`r__dsgraph_render.cpp:584`) | UI scale cached at reset from the output size (`ui_base.cpp:113`) | **UNVERIFIED**: 3D-anchored (LIT) item UI is projected and unaffected; screen-space parts would be drawn at output scale into a render-size target. Game acceptance item |
| `OnRenderPPUI_main` -> `draw_wnds_rects` (`phase_combine.cpp:303`, `GamePersistent.cpp:844-848`) | UI windows' PP rects; `pp_start` scales by `getTarget()->get_width()` over `Device.dwWidth` | **UNVERIFIED**, probably consistent (both render size in the window) |
| Concurrent thread: `seqFrameMT` (sound, network) and `seqParallel` (HOM `MT_RENDER`, details `MT_CALC`, AI visibility, ALife, `CMapManager::Update`) | Not the dimensions, except possibly the map manager | **UNVERIFIED** for `CMapManager::Update` |
| `Device.fWidth_2/fHeight_2` | Set at reset; no renderer reader | Unaffected |

**Diagnostics.** The 120-frame window line appends
`| depth= alloc_failed= viewports= ssao_viewport= | gpu_world_ms swapped=<ms> (n=) stock=<ms> (n=) gpu_seam_ms=<ms>`,
and the totals line adds `render=WxH output=WxH scale= twin_mib=` before `refused=`. `round_trips` counts the seam
pass in both stages. `gpu_world_ms` is the GPU time from the H1 prologue to the first seam call, from D3D11
timestamp queries read back a few frames later without a stall; `swapped` covers frames with an open window and
`stock` every other world frame, so a capture-only run (`SRStage=0`, `SRStageCapture=1`) gives the stock reference
for the same view. `gpu_seam_ms` is the seam pass. Install line:
`Anomaly SR stage hooks installed: SRStage=2 ... SRScale=0.670 (default), bilinear upscale at the seam`.

**Capture** (`SRStageCapture=1`): `anomaly_seam_fp16_input` (the render-size FP16 `generic0` twin before the upscale),
`anomaly_seam_generic0` (the engine UNORM8 `generic0` after the upscale, as P receives it) and `final_color`. The
frame record carries render and output size.

**Hooks and guards.** The same four detours. Stage 2 also requires the first 0x46 bytes of `set_viewport`
(`ssao_set_viewport_code`, which encodes the guard and viewport displacements) before installing.
`tests/anomaly_sr_evidence.py --verify` re-derives the world-target table against the constructor slots, the absent
slots, the `rt_half_depth` halving, the SSAO dispatch bits in `phase_combine` (`0x8bca75`), the `set_viewport` code,
both data addresses (viewport in `.data` with `{0,0,0,0,0,1}`, guard in `.bss`), its four callers and that nothing
else references either address (`.local/anomaly/sr-stage2/verify.txt`). Six negative profile mutations each fail by
name (`.local/anomaly/sr-stage2/negative.txt`).

**Live acceptance (to run).** Same save and view as Stage 1. Runs: A `SRStage=0 SRStageCapture=1` (stock and the
stock `gpu_world_ms`), B `SRStage=2 SRScale=1.0` (every swap at output size; seam and final images should match A
as Stage 1 did, apart from clamping areas and noise), C `SRStage=2` (0.67), D `SRScale=0.5`. Check:

1. Geometry and lighting registered: no offset G-buffer, no lighting in a corner, no half-screen SSAO (with
   `r2_ssao_half_data` on and off), fog, sky, rain, volumetrics, wallmarks and decals aligned; `anomaly_seam_fp16_input`
   is a full image at render size.
2. HUD, PDA content, scope and menus sharp at output size; scope frames counted as `skipped scope` with no swap.
3. Item-attached UI (detectors, PDA in hand) at the right place and scale at 0.67 and 0.5 (the 1.5 hazard).
4. Fluid volumes (smoke, anomaly fields with `r3_volumetric_smoke` on) registered and occluded correctly.
5. GPU time: `gpu_world_ms swapped` in C and D against `stock` in A at the same view, and the frame time.
6. `vid_restart` and a resolution change with an edited `SRScale`: `twin_generation` and `twin_drops` +1 each, the
   "SRScale a -> b" line, `render=` follows.

Expected in a steady world view: `eligible == swapped == seams == round_trips == restores seam`,
`viewports == 2 * swapped`, `ssao_viewport == swapped` once any SSAO static exists (else 0), and
`combine_end = stale_prepare = frame_end = rollback = foreign = write_faults = rt_failed = layout = twins = depth =
alloc_failed = 0`, `refused=no`; no D3D11 debug-layer errors.

Open or **UNVERIFIED**: everything marked so in the table; whether lower-resolution forward passes that sample
`$user$generic0`/`generic_temp` (glass, `distort.s`) behave like the Stage 1 glass-pane finding; texture sharpness
(no mip bias until Stage 3); and the effect of the bilinear filter on the alpha channel consumers (none known).

## 11. Stage 3 implementation (jitter and mip bias, spatial upscale still active)

12 November 2026. Built and tested without the game. Not injected, not game-tested. No SR provider is called and
rendering-readiness flags stay 0.

**Selection.** `[Anomaly] SRStage=3` is Stage 2 (same keys, same swap, same bilinear seam pass) plus, inside the world
window: a subpixel jitter of every world and first-person projection, and a lowered texture mip bias. Extra keys:
`SRJitter=0` keeps the mip bias and drops the jitter (default 1); `SRMipBias=<offset>` is added to `log2(render/output)`
(decimal, clamped to [-2, 1], missing or unparsable 0; -0.3 until DLSS J/K left a trail behind far characters on the aliased ground), `SRMipBias=off` leaves the bias alone. `SRStageCapture=1`
with Stage 3 takes a burst of `SRStageCaptureCount` (default 8, at most 32) consecutive main-view world frames from
world frame `SRStageCaptureStart`; F10 still takes a single core dump.

**Jitter source and units** (`games/anomaly/src/sr_jitter.h`). Halton(2,3) at index `i mod N` + 1, minus 0.5, so samples
lie in [-0.5, 0.5) render pixels; `N = ceil(8 * (output/render)^2)`, clamped to [8, 64] (18 at 0.67, 32 at 0.5, 8 at 1.0).
The sequence restarts at index 0 on a change of render size, output size or scale, a device reset (`resets` generation)
and a history break: any frame without a world since the last main-view H1 (level load, loading screens, menu). Scope
frames neither take nor advance a sample. The unit is RSF's: render pixels, +X right, +Y down, the rasterized image
moves by `(+x, +y)`. That is `rsf_camera_frame::jitter_pixels` unchanged (`sdk/game/include/rescaleframe/game_frame.h`;
the AC7 producer derives the same quantity as `clip_x * W/2, clip_y * -H/2`, `games/ac7/src/view_uniforms.cpp:254-257`),
so the future SR pass needs no conversion. NDC offset `(2x/W, -2y/H)`; PS b13 UV `(x/W, y/H)`.

**Main projection (H1).** After the twin exchange, `Device.mProject` becomes the jittered copy: for X-Ray row vectors
every row `i` gets `M[i][0] += 2x/W * M[i][3]`, `M[i][1] -= 2y/H * M[i][3]` (for `build_projection`, only `_31`, `_32`);
this is `temporal::jitter_projection` applied to the transpose. `Device.mFullTransform = mul(mProject, mView)` is recomputed
in `Fmatrix::mul` order (`_matrix.h:116`), and `R_xforms::set_P(&RCache.xforms, &Device.mProject)` (`0x83ecd0`) re-uploads
it, which also recomputes `m_vp`/`m_wvp` (`R_Backend_xform.cpp:32-42`). The z-prepass, G-buffer, lights, forward and
emissive phases all bind `Device.mProject` (every `set_xform_project` in R4 restores from it: `r2_R_sun.cpp:833,1077,1430`,
`accum_spot.cpp:42,299`, `accum_reflected.cpp:17`, `WallmarksEngine.cpp:379,506`, `r4_R_render.cpp:506`), so they are
jittered together. Shadow maps use the light's own matrices (`r2_R_sun.cpp:799,1041,1385`, `r2_R_lights.cpp:118`) and stay
unjittered. Sun cascades read `Device.mProject` only for the caster-culling frustum rays (`r2_R_sun.cpp:1134-1219`); their
placement follows the camera position snapped to shadow texels (`:1175,1252`), so shadow maps do not move.

**First person (H4, the shared rmNear slot).** The six sites (section 4) rebuild `Device.mProject` with `psHUD_FOV`
(so `_31 = _32 = 0`), compute `mFullTransform`, `set_xform_project`, then call `rmNear`. The geometry detour calls the SR
slot after the original `rmNear`. Rule (`first_person_action`): a matrix equal to the jittered main projection, or to the
last first-person matrix the adapter wrote, is never jittered again; a matrix with `_31 = _32 = 0` is jittered by the same
pixel offset, `mFullTransform` recomputed with the current `mView`, and uploaded through `set_P`; any other matrix is
counted `fp_foreign` and left alone. Each site restores the jittered main pair itself (`Pold`, `FTold`). The particle
site's `ApplyTexgen(Device.mFullTransform)` (`ParticleEffect.cpp:612`) therefore sees the jittered transform too. Without
the slot (geometry hooks refused) Stage 3 turns jitter off, since a weapon without jitter would shimmer against the world.

**Mip bias.** `SetMipLODBias(&SSManager, saved + delta)` (`0x7fe170`, bias in `xmm1`) in the H1 prologue, `delta =
clamp(log2(render_w/output_w) + SRMipBias, -3, 1)`; `saved` is `SSManager.m_uiMipLODBias` (`+0x19C`, the user's
`r__tf_mipbias`) read at that moment. 1286/1920 with the default gives -0.878. SetMipLODBias returns early on an unchanged
value and otherwise recreates every sampler from its own description (`dx10SamplerStateCache.cpp:201-222`), as
`phase_scene_begin`'s per-frame `SetMaxAnisotropy` already does; both rebuilds preserve each other's field. A delta of 0
calls nothing (`mip unchanged`). Default offset -0.3 is the repo's AC7 policy (`games/ac7/src/native_renderer.cpp:1357`);
guidance elsewhere ranges from 0 (XeSS) to -1 (FSR 2). Already-bound samplers keep the previous bias until the engine
rebinds them (**UNVERIFIED** magnitude; the main pass changes sampler lists constantly).

**Restore on every exit.** `SwapController::restore` restores the projection and the bias before the twins on every path:
seam, `combine_end`, stale prepare, `frame_end`, shutdown and rollback. The saved `Device.mProject`/`mFullTransform` bytes
are written back and `set_P` re-uploads them (not on shutdown, which writes engine memory only; the next frame's
`SetCacheXform` uploads `Device.mProject`). The bias goes back through `SetMipLODBias` on every path including shutdown,
because nothing else would restore it. A `Device.mProject` or bias that differs from the adapter's value at restore counts
as `foreign` and refuses further swaps. A write or call fault while applying rolls the whole window back. A non-finite
projection is not jittered (`jitter_invalid`); the window still opens.

**Motion and SR coordination.** At every H1 the adapter publishes a `JitterState` (`sr::current_jitter()`: main-view
frame counter, sequence index and phase count, current and previous applied jitter in pixels, render sizes, reset flag)
and its UV form (`sr::current_jitter_uv()`, lock-free). The motion history records the camera from RCache, so its previous
camera carries the previous jitter; the core PS b13 `rsf_jitter` is `(current.xy, previous.zw)` from this state and
removes both (`shaders/core/rsf_core_pixel.h`). Scope frames publish zeros (separate, unjittered history). A main frame
whose window did not open publishes current 0 and the real previous value. The only edit in the motion files is
`motion_stage.cpp`'s PS b13 fill reading `sr::current_jitter_uv()` (foliage draws keep previous 0, as before); the
foliage worker's tree path also reads `previous_u/v` from it. `motion::set_jitter_uv` is no longer the source.

**Guards.** Stage 3 adds no detour; it calls two engine functions and relies on six call sites. Before installing it
requires the 28-byte `set_P` unique guard, the 16-byte `SetMipLODBias` guard (it encodes the `+0x19C` field), and at each
of the six `sr_first_person_sites` an `E8` to `set_P` followed within 0x120 bytes by `call [rax+0x1b8]` (slot 55) or an `E8`
to `rmNear` (`0x83a450`). `tests/anomaly_sr_evidence.py --verify` adds: `Device.mView/mProject/mFullTransform` adjacency,
slot 55 = `+0x1b8`, the call forms (4 virtual, 2 direct) and that `SetMipLODBias` compares `xmm1` with the cached bias.

**Diagnostics.** Every 120 frames, with `SRStage=3`, a third line:

```text
Anomaly SR stage 3 window jittered= jitter_invalid= fp_jittered= fp_already= fp_foreign= projection_restores= | mip applied= restored= unchanged= | sequence frame= index=i/N resets= last=(x,y) previous=(x,y) applied= | mip window= user= | burst n/N
```

Expected in a steady world view: `jittered == swapped == projection_restores == mip applied == mip restored`;
`fp_jittered` about one per first-person site per frame (weapon, hud_ui, sorted, emissive, HUD particles, HUD lights, as
present); `fp_foreign = jitter_invalid = 0`; `resets` grows only on load, menu, `vid_restart` or a scale or resolution change.

**Burst capture.** Stage 3 `SRStageCapture=1` copies, for each of the consecutive frames, the render-size FP16 `generic0`
twin at the seam and the back buffer before Present into staging textures, maps them three frames after the last one and
writes `<dump dir>/sr-jitter-<session>/world_NN.bin` (RGBA16F, rows packed), `final_NN.bin` (back buffer format) and
`burst.json` (per frame: engine frame, jitter frame, sequence index and phase count, applied jitter and previous jitter
in pixels, render size, window mip bias, `fp_jittered`, sizes and DXGI formats). `py -3 -I tests/anomaly_sr_evidence.py
--measure-jitter <dir> [--region x0 y0 x1 y1] [--final-region x0 y0 x1 y1]` measures each world frame's displacement
against frame 0 (Hann-windowed cross-correlation, 1/100 px refinement) and compares it with the recorded jitter
difference; `--region` restricts it to, for example, the weapon, `--final-region` checks that a HUD rectangle does not
move. `--measure-self-test` recovers synthetic Halton offsets within 0.015 px.

**Tests without the game** (`build/wk-sr3`, Release; `tests/anomaly_support.cmake`):

- `anomaly_sr_jitter_contract`: Halton values and stratification, phase counts, sequence wrap and every reset trigger;
  the NDC offset (only `_31/_32` change for a perspective matrix, bit-exact elsewhere, points at several depths move by
  exactly `(+x, +y)` pixels, orthographic matrices shift through the last row, equal to `temporal::jitter_projection`);
  `Fmatrix::mul` order; the first-person rule (including that a second application would double the offset); the mip bias
  delta and clamps; the published state (scope zeros, previous carried across a scope frame and across a frame without a
  window, previous UV in the previous render size); the CPU motion PS on a static scene over 19 frames.
- `anomaly_sr_swap_contract` gains three cases on fake `Device` matrices and a fake sampler cache: the jittered window,
  six first-person sites each jittered once, rmNear without rebuild, rmNear on the main matrix and on a foreign matrix,
  restore at the seam with the stock projection re-uploaded; every non-seam exit (combine_end, frame_end, stale prepare,
  shutdown) and every apply fault (each matrix write, `set_P`, `SetMipLODBias`) restoring the exact engine bytes and the
  user's bias; foreign projection or bias changes; scope/MSAA/off frames untouched; `SRJitter=0`, `SRMipBias=off`, zero
  delta and a non-finite projection.
- `anomaly_sr_hooks_contract`: `SRStage=3`, `SRJitter`, `SRMipBias` parsing; install refusals for a changed `set_P` or
  `SetMipLODBias` byte and for each first-person call form (wrong `set_P` target, slot 56 instead of 55, direct `rmFar`).
- `anomaly_sr_jitter_device` on Intel UHD Graphics (hardware, debug layer on): the unchanged `rsf_core_pixel.h` writer with
  production `pack_pixel` bytes and the production jitter state, 18 consecutive frames of a static tilted plane at 96x54:
  worst motion 3.5e-5 UV (0.003 render px) over 93,312 pixels; with the b13 jitter zeroed the motion equals the jitter
  difference (worst error 4.3e-5 UV); a (+3, -2) px pan between frames survives the jitter removal (worst 5.0e-5 UV).
- `anomaly_sr_measure_self_test`: the burst measurement on a synthetic burst.
- `py -3 -I tests/anomaly_sr_evidence.py --verify` passes (`.local/anomaly/sr-stage3/verify.txt`).

**Live acceptance (to run).** Same save and standing view as Stage 2, no movement, weather and time fixed if possible.
Runs: A `SRStage=2` (0.67, `SRStageCapture=1`); B `SRStage=3 SRJitter=0` (mip bias only); C `SRStage=3` with
`SRStageCapture=1` (burst of 8; `SRStageCaptureCount=18` covers a whole sequence at 0.67); D as C with `SRMipBias=0`;
E as C with `Motion=1` and F10 motion captures. Check:

1. C burst: `--measure-jitter` world displacement equals the recorded jitter difference within 0.05 render px over
   the whole frame and over a static, non-periodic region of at least 64 px (animated foliage, rain, grain and
   striped textures add noise or alias). Correction, 13 November: the first estimator (Hann-windowed
   cross-correlation) recovered only 74-80% of each shift, so the e991e89 burst read 0.163 px. The
   Gauss-Newton/Catmull-Rom estimator measures the same burst at 0.025 px worst; recorded jitter units, sign
   and frame timing were correct. Captures now wait for 30 frames clear of the loading screen.
2. Weapon: `--region` on the weapon gives the same displacement as the world (no relative shimmer); `fp_jittered` > 0.
3. Shadows and UI: shadow edges move with the surfaces they fall on (same displacement, no extra crawl); `--final-region`
   on a HUD element measures 0.
4. Texture sharpness: B and D at 0.67 against stock 1.0 (A at `SRScale=1.0`) on distant ground textures; default C is
   meant to be slightly sharper than native (offset -0.3).
5. Motion: E on the static view gives zero motion apart from animated content, and the same vectors as Stage 2 for a pan.
6. Counters as above; a level load, the menu, `vid_restart` and a scale change each add one `resets`.

Open or **UNVERIFIED**: the sampler rebind lag above; the jittered `mFullTransform` in culling, HOM and the light scissor
(subpixel); motion-blur matrices with `r2_mblur > 0` (section 4); jittered PDA and item-attached UI drawn through the
first-person sites; and the jittered final image flickering by up to half a render pixel until temporal SR (Stage 5)
consumes it. The initial Stage 3 implementation also left `pos_decompression_params` unjittered; the 13 November
correction below now repairs that deferred-ray mismatch, with visual acceptance pending.

### Sun cascade camera correction (12 November 2026)

The user reproduced distant sun-lit flicker with Stage 5 and removed it by turning the live Subpixel jitter option off.
Source review then found a producer-side consequence of the world-window jitter: `CRender::render_sun_cascade` copies
`Device.mProject`, multiplies by `Device.mView`, inverts that camera matrix and fits the shadow cascade to its frustum
(`r2_R_sun.cpp:1127-1136`, xray-monolith 028ccf5f5189b393396480076b11e40852ba3ecd). Its position snapping does not
stabilize jitter-dependent frustum extents. This corrects the earlier broad assumption that every sun pass should simply
inherit the jittered Device camera. Rasterized world geometry still uses that camera; shadow fitting now temporarily uses
the saved main camera.

The detour wraps `CRender::render_sun_cascades` once around its entire cascade loop, with `this` in rcx. It is the modern
sun branch's sole call in `CRender::Render`; the legacy `R2FLAGEXT_SUN_OLD` branch is unchanged. The source's sunshafts
check, last-cascade reset-chain save/restore, vector begin/end at CRender+0x970/+0x978, stride 0x68 and indexed loop call
identify the wrapper independently of prologue similarity. Each cascade reads the complete Device projection; neither
build directly reads Device.mFullTransform in this function. The end of each cascade restores RCache from Device.mView
and Device.mProject, which is why restoring Device alone is insufficient.

| Build | Wrapper RVA | Sole main-render call | Cascade fit RVA | Call expected16 |
| --- | --- | --- | --- | --- |
| Standard AnomalyDX11.exe | 0x815a30 | 0x8984a8 | 0x813170 | `e883d5f7ffeb15e8ecd6f7ff488bcee8` |
| AVX AnomalyDX11AVX.exe | 0x80dfd0 | 0x88e0d6 | 0x80be60 | `e8f5fef7ffeb15e85e00f8ff488bcee8` |

Both wrapper entries have expected16 `48895c240848896c2410488974241848`; this is not unique. The checked shortest unique
prefix is 30 bytes: `48895c240848896c2410488974241848897c242041564883ec20488bd948`. MinHook relocates the first five-byte
integer instruction (`mov [rsp+8],rbx`), with no RIP operand or VEX instruction. The cascade fit itself differs considerably:
standard uses four 16-byte projection loads; AVX uses two 32-byte loads, with a different frame. No fit-frame offset is read
by the adapter. Discovery and decoded operands are retained in `.local/anomaly/sun-cascade/{locate,decode,guards}.py`,
`sites.json` and `disassembly.txt`; the permanent checks are in `tests/anomaly_sr_evidence.py`. REA had no open database
and its Ghidra Java runtime was unavailable, so this bounded read used the existing PE/Capstone tools rather than opening
an additional database or changing tool installation.

While an SR world window actually carries jitter, `SwapController::sun_cascades_begin` saves the current jittered full
matrix, verifies the current projection against the window's jittered main projection, and writes the saved unjittered
Device.mProject and Device.mFullTransform. The original loop runs without the stage mutex, allowing existing draw
callbacks. `rm_near` does not inject first-person jitter during the shadow window. On return, `sun_cascades_end` restores
the exact jittered projection/full pair and invokes the existing guarded set_P path, rebuilding RCache.m_p/m_vp/m_wvp.
No RCache matrix is written directly. Scoped cleanup covers C++ unwind; reentrant calls borrow the suspended camera;
quiesce invalidates the pending restoration token. A failed matrix write or projection upload closes the SR window through
the existing rollback, restores stock state and refuses further swaps. Scope frames, SR off and jitter off are pass-through.
Matrix units, render-frame identity, vendor inputs and GPU resource leases are unchanged. No new option is added.

The report line is `Anomaly SR sun cascades calls=... unjittered=... restored=... refused=...`. In a steady main view with
sunlight and jitter, `unjittered == restored`, both advance with sun wrapper calls, and `refused=0`. With jitter off,
`calls` may advance while `unjittered/restored` stop. A sunless frame need not call the wrapper. Existing SR jitter/restore,
foreign-change and write-fault counters should remain clean.

Synthetic tests cover three cascade observations of an invariant unjittered camera, exact Device and RCache restoration,
no-jitter pass-through, reentrant borrowing, quiesce during the call and five write/upload fault paths. Both profile hook
fixtures reject altered entry/extended guards and the call displacement. Static evidence passes on both executable hashes,
including these new sites and negative mutations. This is a built and fixture-tested correction; the visual flicker fix
still needs the coordinator's game run at the same sun-lit view with jitter on, followed by scope/HUD and moving-camera
checks. No game was launched and the game installation was not modified by this task.


Validation for this correction: MSVC Release configure and the full build in `build/wk-avx` passed; the requested
`ctest --test-dir build/wk-avx -C Release -R "anomaly|thin|motion" --output-on-failure` passed 93/93 without skips
(104.53 seconds). `py -3 -I tests/anomaly_profile_evidence.py` passed 36/36, including all four evidence scripts for
both executables and 28 negative mutations. `git diff --check` passed for the affected files. Logs and bounded binary
analysis are under `.local/anomaly/sun-cascade/` (`configure.log`, `build-all.log`, `ctest.log`, `evidence-suite.log`,
`sites.json`, `disassembly.txt`). The assigned-directory restriction prevents using the hardcoded shared build directory
in `eng/verify.ps1`; the explicit configure/full build/requested tests above are the native checks for this task.
No Rust behavior or public ABI changed. Visual acceptance remains pending.

### Jittered deferred position reconstruction (13 November 2026)

The user confirmed that the previous receiver-depth adjustment was the wrong remedy: it did not remove the flicker and
it distorted character shadows. The earlier console observation was insufficient evidence for that conclusion. The
adjustment, option, overlay control, native scale sites, guards and associated tests have been removed. The adapter no
longer changes the user's sun depth scales. The separately accepted unjittered sun-cascade wrapper remains.

There are two camera descriptions during the world window. Rasterization uses the render-size Device projection with
`_31 += 2*jx/W * _34`, `_32 -= 2*jy/H * _34` and the corresponding full transform; positive render-pixel jitter is right
and down. Deferred shaders instead used `cl_pos_decompress_params::setup` (`r4.cpp:106-116`), which derives
`v = [HorzTan, VertTan, 2*HorzTan/W, 2*VertTan/H]` from unjittered FOV and aspect. The vanilla `gbuffer_stage.h:128`
rebuilds `P.xy = depth * (pixel_center * v.zw - v.xy)`. A scene point projects from unjittered pixel q to p=q+j, so using
the stock vector at p reconstructs the wrong ray by `depth * j * v.zw`. The error grows with distance and can put a grazing
sun receiver on alternating sides of its shadow surface. Changing shadow depth scales hides or moves the comparison
error instead of repairing the ray, and can detach or distort contact shadows.

The correction is `v.xy += (jx,jy) * v.zw`, leaving zw unchanged. Substituting p=q+j cancels the extra term exactly. The
numeric fixture projects view-space points through `jitter_engine_projection`, converts NDC to pixel coordinates, then
reconstructs the original point with these constants. It covers 18 Halton phases, three aspect/size pairs (including odd
render sizes), three FOVs and depths 0.5, 5, 40 and 200; tolerance is 2e-6 times one plus the coordinate magnitude.
It also proves the stock lateral error has the predicted sign and size.

**Native mechanism.** Detour the registered binder's slot-0 implementation, run its original first, then correct the CPU
constant-buffer staging copy before the engine uploads it. Find the literal `pos_decompression_params` registration,
follow its r8 binder object's vptr, identify `cl_pos_decompress_params` through RTTI, and inspect slot 0 and every stage
branch. This avoids replacing FOV arithmetic or calling an unproven overloaded set_c entry. The exact-build profile
contains the complete binder body, all successful GetCBuffer branches, registration arguments and the reflection-size
store. Full bytes and per-build addresses are recorded in the [profile note](anomaly-avx-profile.md#deferred-position-binder).
The binaries retain the [fingerprints](anomaly-avx-profile.md#scope-and-evidence) listed there; no runtime scan is used.

Both binders inline `RCache.set_c` for PS, VS, GS, HS, DS and CS. `R_constant.destination` is at +0x10, stage bits are
1/2/8/16/32/64, and six `(u16 byte_offset, u16 class)` pairs start at +0x14 with stride 4. A float4 has class 1. The
adapter calls the original `R_constants::GetCBuffer(&RCache.constants, C, stage)` with constants at backend+0x5c0;
it preserves the engine's stage/slot selection. The getter extracts destination nibbles at shifts 16/12/8/20/24/28 and
selects PS/VS/GS/HS/DS/CS arrays at backend+0x328/+0x2b8/+0x398/+0x408/+0x478/+0x4e8. There are 14 supported slots.
Reflection's GetDesc writes `D3D11_SHADER_BUFFER_DESC` on the constructor stack, and its Size is stored at buffer+0x58.
The CPU data pointer is +0x60 and changed byte +0x68. The already verified geometry Flush proof observes that byte,
uploads that data through UpdateSubresource, then clears it. Thus a corrected CPU float4 and changed=1 use the normal
engine upload protocol in every destination stage, including compute.

All destinations are preflighted for class, slot, size, pointers and finite floats before any correction. Aliased storage
is corrected once; distinct stages are all corrected. A failed write rolls attempted destinations back and refuses further
SR swaps. The fixtures cover all six stages independently and together, aliasing, every write-failure position and invalid
class/slot/size/flags/data. The hook and extended-guard fixtures run against both profiles, with negative byte mutations.

Only an open world window with successfully applied jitter is corrected. SR off, scope frames, jitter off and calls after
the seam use the stock binder result. The correction uses the saved raster jitter even while the sun wrapper lends
Device an unjittered camera: cascade fitting needs stable frustum corners, but accumulation inside that wrapper samples
the already jittered G-buffer. Testing the current Device projection or suppressing correction while sun_suspended would
reintroduce this defect. The existing cache invalidation at open/restore forces the binder to run again across the window
boundary; corrected constants are not retained as post-window stock state.

The periodic report is `Anomaly SR position reconstruction corrected=... stock=... refused=...`. Counts are binder
invocations, not frames or individual shader stages. In an active jittered world, corrected must rise and refused must
stay zero; stock can rise outside that window. The sun cascade counters must still balance unjittered/restored with zero
refusals. There is no new setting: this is required camera consistency, controlled by the existing SR/jitter window.

**Consumer audit.** Source revision `028ccf5f5189b393396480076b11e40852ba3ecd`, files under `src/Layers` unless
otherwise stated, and the user's vanilla r3 shader corpus were searched for FOV/aspect/tan, dimensions and projection/full
transform readers. These rows are source/corpus review, not new game acceptance or independent native mappings of every
optional effect. The binder and its upload path above are proven in both shipped binaries.

| Consumer | Inputs and effect | Required camera and disposition |
| --- | --- | --- |
| Deferred direct/spot/point light and sun receivers | gbuffer_stage reconstructs depth with pos_decompression_params | Jittered raster rays; corrected by this binder, including accumulation inside the sun wrapper. |
| Sun cascade fitting (r2_R_sun.cpp:1134) | Copies Device.mProject and inverts the view frustum | Unjittered fit; existing wrapper retained. Legacy FOV-built sun paths also describe a stable shadow fit, not G-buffer inversion. |
| Volumetric sun and light volume raster | accum_volumetric_sun.ps uses gbuffer_load_data; accum_volumetric.vs uses m_P | Jittered receiver rays corrected; volume vertices follow current RCache projection. Shadow-map matrices remain light-space. No extra fix identified. |
| Rain shadow fit and wet-surface passes (r4_R_rain.cpp:57-67,268) | Rebuilds a FOV/aspect projection for rain coverage, later restores Device.mProject; rain_layer and rain_patch_normal read gbuffer_load_data | Unjittered coverage fit is intentional; jittered depth reconstruction now corrected. No separate visible fault established. |
| Forward water/refraction (water/waterd shaders) | m_VP rasterizes water; gbuffer_load_data(PosTc,hpos.xy) reconstructs the receiver | Current jittered matrix and rays inside the window; binder fixes reconstruction. Distorted UV versus screen-position approximations are pre-existing shader behavior, not changed here. |
| SSAO main/hbao and half-depth (phase_ssao.cpp:3-9,42-47; ssao_calc.ps) | Device dimensions/static viewport; FOV scales noise/kernel radius; common G-buffer load | Render dimensions already repaired by Stage 2. FOV controls cone size, which jitter does not change. Main reconstruction gets corrected rays. |
| SSAO uv_to_eye (ssao.ps:86-89,160) | Uses (2uv-1)*v.xy*depth as a symmetric extent helper | Corrected xy changes its extents by relative 2jx/W and 2jy/H, at most about 0.21% at 853x481. This helper is not a matching inverse camera; the change is subpixel within the screen. No separate patch; validate enabled SSAO visually. |
| HDAO PS/CS (phase_hdao.cpp; ssao_hdao*.ps) | screen_res/dwWidth/dwHeight choose texels, clamp taps and compute dispatch groups | Jitter does not change dimensions or linear depth. Stage 2 dimensions and normal constant binding apply; no additional FOV-based inverse found in this corpus. |
| Screen-space sunshafts (rendertarget_phase_sunshafts.cpp; ogse_sunshafts*.ps) | Sizes, depth mask, m_VP-projected world sun and radial blur | Runs after the seam with restored camera/output size. Existing nearest-upsampled jittered depth can still move silhouettes; outside this world-ray fix and needs a capture with the effect enabled. |
| Light scissor and occlusion query (enable_scissor.cpp:34,71; light_vis.cpp:25-31; phase_occq.cpp) | Full transform bounds volumes; FOV derives conservative near-camera safe area; query volume is rasterized | Jittered raster bounds/query must match depth; current matrix provides them outside shadow-fit scopes. FOV safe-area size needs no ray correction. Threshold visibility at silhouette edges remains an unmeasured subpixel effect. |
| Lens flare visibility (xrEngine/xr_efflensflare.cpp; dxLensFlareRender.cpp) | World-space ray queries from camera toward sun; full transform only for screen blend; vertices use current RCache | Visibility rays are unjittered world directions; raster placement follows current camera, or restored camera after seam. No FOV-derived deferred inverse found. |
| m_texgen / mVPTexgen and clip planes (StandartBinding.cpp:70-130; R_Backend_Runtime.cpp:170) | RCache world/view/projection products; inverse current full matrix for clip planes | Current draw camera, which is jittered for main geometry and light-space for shadow rendering. Not an independent unjittered FOV reconstruction. |
| screen_res / screen_params binders (StandartBinding.cpp:390-406; r4.cpp:118) | Width/height and reciprocals; FOV/aspect/tan/far range; pos_decompression_params2 is dimensions only | Render-size values inside window; jitter changes center, not extent. No active screen_params reference in this vanilla r3 corpus; params2 usage in depth_downs is commented. No extra correction. |
| Culling/HOM/detail and LOD (r4_R_render.cpp:250-278; HOM.cpp:24,184-202; DetailManager.cpp:269; r2_R_calculate.cpp:18) | ViewBase/HOM formed before H1 or uses saved full transform; some later render_main/detail readers see current full matrix; FOV sets LOD scale | Stable unjittered culling is preferable; later current-matrix reads can move a boundary subpixel. No observed rejection or additional native patch justified here. |
| Wallmarks (WallmarksEngine.cpp:376-379,504-506) | Temporarily changes projection _43 for depth bias, preserving _31/_32 | Current jittered center survives; deliberate decal depth offset, not a second screen-to-view inverse. No patch. |
| Offline/debug camera tools (r__screenshot.cpp; r__pixel_calculator.cpp) | Explicit 60-degree cubemap or orthographic camera | Separate capture/calculation views, not main deferred reconstruction. Not covered by this game acceptance. |
| HOM debug depth boxes (occRasterizer.cpp:144-166) | Inverse _11/_22 ignores projection center | DEBUG-only visualization; not Release deferred shading. No patch. |
| First-person HUD, particles, point lights (r__dsgraph_render.cpp; ParticleEffect.cpp; accum_point.cpp) | Rebuild FOV-specific local projections then rmNear | Same raster jitter is already applied at six verified rmNear sites and restored by engine. Their different FOV extent is intentional. |
| Motion blur/combine and CPU screen projection | phase_combine.cpp:136-140 retains matrices captured before seam; D3DUtils transforms points with current full matrix | Window matrices include jitter; possible micro-blur when r2_mblur > 0 is an existing unmeasured post-effect issue. Post-seam UI/tracers use restored camera. No additional confirmed flicker fix. |

The only newly confirmed visible cause addressed here is the deferred ray mismatch. Optional effect variants, the SSAO
extent approximation, post-seam depth sampling and culling edge cases above remain visual acceptance limits, not claims
that another native site has been mapped or fixed. Bounded disassembly, site JSON and validation logs are under
`.local/anomaly/position-reconstruction/`; no analysis database or game process was opened.

For the first game check, use the normal near/far sun depth scales (1.0 if the console experiment left 0.9995), verify
hook install without refusal, rising corrected and zero refused, then compare the same grazing sun-lit ground and character
contact shadows with jitter toggled. Verify scope/SR-off pass-through and enabled SSAO, rain, water and volumetric effects.
This worker did not alter those console values or any installed file. This correction is built and fixture/static-tested;
its visual acceptance is pending. Rendering readiness stays zero.


Validation (13 November 2026): MSVC Release configure and full build in `build/wk-avx` passed; the requested
`ctest --test-dir build/wk-avx -C Release -R "anomaly|thin|motion|shader_patch" --output-on-failure` passed 96/96,
without skips, in 92.78 seconds. `py -3 -I tests/anomaly_profile_evidence.py` passed 48/48: all four evidence scripts
against both hashes, with 40 negative mutations rejected. Package unittests passed 10/10 and `git diff --check` passed.
The first CTest run passed 94/96 but failed FSR3 activation in two FG fixtures: its cached vendor root held AMD DLL
SHA-256 `02297bee...`, while the current SDK/header pin uses `e6843252...`. Pointing only the assigned build's
`RSF_ANOMALY_SR_VENDOR_ROOT` at local fixture junctions (unchanged Streamline/XeSS and the current pinned FidelityFX
signedbin) fixed both failures; the entire requested selection then passed. This is a test-input correction, not a game
or Runtime change. Reproduce with `-DRSF_ANOMALY_SR_VENDOR_ROOT=D:/Coding/ReScaleFrame/.local/anomaly/position-reconstruction/vendors`.
Logs: `.local/anomaly/position-reconstruction/{configure,build-full,configure-pinned-vendors,build-final,ctest,
ctest-stale-vendors,evidence-suite,package-tests,fg-pinned-probe}.log`. The explicit assigned-directory build/tests
replace `eng/verify.ps1`, which hardcodes the prohibited shared build directory. No Rust behavior or public ABI changed.

## 12. Stages 4 and 5 implementation (SR inputs and temporal SR at the seam)

12 November 2026. Built and tested without the game, including real DLSS, FSR3, FSR4 and XeSS evaluations through the
adapter's input path on an RTX 4070 Laptop GPU. Not injected, not game-tested. Rendering-readiness flags stay 0.

**Selection.** `Mode=Observe`, as for Stages 1 to 3. `Motion=1` should be on: without a motion target every pixel is
reconstructed from depth and the camera (counted as `motion_missing`).

- `SRStage=4`: Stage 3 plus the SR inputs prepared at the seam (linear colour, depth twin, motion with the first-person
  rule) and dumped by `SRStageCapture`. The output is still the bilinear pass and no provider is called.
- `SRStage=5`: Stage 4 plus the shared RSF SR pipeline at the seam. Any refusal draws the bilinear pass instead.

| Key | Default | Meaning |
| --- | --- | --- |
| `SR`, `SRBackend`, `SRQuality`, `DLSSPreset` | 1, 1, 1, 0 | Shared frontend keys, read at startup and persisted by the overlay. Backends 1 DLSS, 3 FSR3, 4 FSR4, 5 XeSS; qualities 0 Native to 3 Performance. Ultra performance (a third of the output) is below the window's half-size floor and is not offered. `SR=0` keeps the world window closed, so the frame renders stock at output size. `DLSSPreset` is forwarded only as the user's choice |
| `SRExposure` | `constant` | `constant`: a 1x1 `R32_FLOAT` exposure of 1.0 with the exposure-scaled `x` (2.6 default). `auto`: no exposure texture, so each backend auto-exposes. This is the exposure A/B |
| `SRFirstPersonZero` | 1 | Unwritten pixels with depth below 0.02 (the `rmNear` range) get a valid zero vector instead of world-camera reconstruction |
| `SRCutDistance`, `SRCutAngle` | 4 m, 45 degrees | Camera cut: a larger move or turn between consecutive main-view frames resets history. Clamped to [0.1, 1000] m and [1, 180] degrees |
| `SRStreamlineDir`, `SRFidelityFXDir`, `SRXeSSDir` | `<ini dir>\ReScaleFrame\{streamline,fidelityfx,xess}` | Vendor runtimes, in the AC7/RDR2 package layout. FSR3 and FSR4 both load from `fidelityfx` |
| `SRRequireSignature` | 0 | Refuse an unsigned `sl.interposer.dll` |
| `SRScale`, `SRJitter`, `SRMipBias` | as Stage 3 | In Stage 5 `SRScale` is used only while no backend plans the render size. Stage 5 refuses every pass without jitter |
| `SRStageCapture` and its keys | as Stage 2 | Scheduled single frames (count at most 16) and F10 |

**Runtime access (Rule 1).** The adapter borrows the carrier-loaded Runtime through a new generic export,
`rsf_get_d3d11_sr_api` (`runtime/orchestrator/include/rescaleframe/d3d11_sr_api.h`, API version 1). It has the same
size/version contract as `rsf_get_d3d11_present_api` and `rsf_get_overlay_api`: a short capacity gives
`INVALID_ARGUMENT` without reading the version, a mismatch gives `ABI_MISMATCH`, and a failure writes nothing. The table
carries `pipeline_abi_version` and the existing pipeline entries AC7 drives: `start` (`rsf_dlss_pipeline_start`), `evaluate`
(`rsf_native_sr_evaluate`), `reset`, `stop`, `status`, `select_backend`, `set_quality`, `set_preset`, `resize_output`,
`output_texture` and `request_dump`. Nothing is forked. `reset` adds `RSF_GAME_RENDER_RESET` to evaluates until one
reaches a backend. Engine identity is adapter data: `RSF_DLSS_ENGINE_CUSTOM`, the engine version string, project id
`8d3b6f52-1c4e-4f7a-b0d9-2e6a51c7f3a4` and `view_space_to_meters = 1` (`sr_pass.h`).

The shared pipeline needed two generic corrections. Both leave AC7's behaviour unchanged:

1. A decode marked by the validity channel never reached a backend correctly. `has_motion_sentinel` (DLSS) and the
   legacy adapter's `has_sentinel` (FSR, XeSS) followed `zero_means_unwritten` alone. With `validity_enabled`, DLSS therefore
   skipped the dense resolve and received the decode's `R32G32` field with -1000 in every unwritten pixel, and the FSR/XeSS
   resolve read -1000 as real motion. Both sites now use `zero_means_unwritten || validity_enabled`
   (`dlss_pipeline.cpp` `marks_unwritten`). A negative fixture run with the old rule fails: wrong format and 108,080 wrong pixels.
2. FSR and XeSS contexts were always opened with reversed, infinite-far depth. `rsf_dlss_pipeline_setup.standard_depth` is
   appended (pipeline ABI 5); non-zero opens them with standard, finite depth. Zero keeps the old convention. DLSS
   reads the per-frame `camera.depth_inverted` either way.

**Adapter flow** (`sr_temporal.{h,cpp}`, `sr_pass.h`, `sr_settings.h`, `sr_overlay.h`, `sr_stage.cpp`,
`shaders/sr/rsf_sr_upscale.hlsl`):

- Install (Stage 5): the table is resolved by name from `ReScaleFrame.Runtime.dll`. Its size, version, pipeline ABI and
  every entry are checked; a refusal leaves the seam bilinear.
- H1, main view only: `Device.mView` and the still unjittered `Device.mProject` are read before Stage 3 jitters them,
  together with the cut test against the previous main-view frame and the frame time.
- Render size (Stage 5): the size the selected backend plans for its quality (pipeline status), held to
  [ceil(output / 2), output]. A quality or backend change resizes the twins at the next H1. `SRScale` applies only while no
  backend plans.
- Seam, after the restore: one render-size draw (`rsf_prepare_ps`) writes the SR colour, f^-1 of the FP16 `generic0`
  twin, and the SR motion. The motion is this frame's main-view motion target (`motion::frame_target()`; the frame,
  view and size must match) with the first-person rule, or an all-invalid zero texture when there is no target. Stage 5
  then builds the pass (`build_pass`) and calls `evaluate`. On success the output-size pass (`rsf_reinsert_ps`) writes
  `saturate(f(SR))` into the engine's UNORM8 `generic0`, with Stage 2's nearest `rt_Position`, bilinear `generic1` and
  nearest depth. Every other outcome draws the Stage 2 bilinear pass, so the post chain always receives a world image.
- Frame boundary (`IRenderDeviceRender::End`, after the frame's SR work, before Present): this is the drained boundary.
  The pipeline starts here on `HW.pDevice`; a refused start is not retried until the settings or the device change. A
  new device stops the pipeline and starts it again. A new output size calls `resize_output`, so the next H1 plans for it.
  Requested settings apply here (`select_backend`, `set_quality`, `set_preset`), and a backend this GPU refuses is not
  asked for again until the request changes. `SR=0` closes the window.
- Quiesce: `stop` before the engine's device goes, and the SR targets are dropped.

**What the pipeline receives, and who reads it.**

| Input | Value | Read by |
| --- | --- | --- |
| Colour | Render-size RGBA16F f^-1(`generic0` twin): exposure-scaled linear radiance | All backends; DLSS through RSF colour transport (pipeline default) |
| Depth | `R24G8_TYPELESS` depth twin: standard device depth, finite far, first person in [0, 0.02] | Dense resolve (`R24_UNORM_X8` view), then an `R32_FLOAT` copy to every backend; `standard_depth = 1` for FSR/XeSS |
| Motion | Prepared RGBA16F. Decode: scale 1, bias 0, output scale 1, sentinel -1000, validity channel 2 above 0.5, `zero_means_unwritten = 0`. `motion_to_uv = (1, 1)`, `motion_camera_included = 0` | Pipeline decode and dense resolve. Every backend receives dense previous-minus-current pixels: written vectors as given, sentinel pixels from depth and `clip_to_previous_clip` |
| Jitter | `sr::current_jitter()` current (render pixels, +X right, +Y down), unchanged; previous jitter in the camera record | All backends (the D3D11 pipeline does not read the previous jitter) |
| Exposure | `constant`: 1x1 `R32_FLOAT` 1.0. `auto`: none. Pre-exposure is 1 | DLSS (colour transport encode/decode); FSR and XeSS switch between a manual and an auto-exposure context |
| Reset | `RSF_GAME_RENDER_RESET` on the first pass of a pipeline session or device, a settings change, a jitter-sequence restart (level load, menu, loading, `vid_restart`, scale or resolution change) and a camera cut | All. The pipeline also restarts history on a size, rectangle, session or non-consecutive `native_frame` change; scope frames do not advance `native_frame` |
| Rectangles | Render `{0, 0, rw, rh}`, output `{0, 0, ow, oh}`; inputs are origin-zero, so no region copy | All |
| Camera | `view_to_clip` = unjittered `mProject` (copied: X-Ray and RSF are both row-major with row vectors), `clip_to_view`, `view_to_world`, `world_to_view`, `clip_to_previous_clip` = inv(V P) Vp Pp of the previous main-view frame (identity on a reset) and its inverse, near `-_43/_33`, far `_43/(1-_33)`, FOV `2 atan(1/_22)`, `depth_inverted = 0`, `motion_jittered = 0`, frame time | DLSS: matrices, planes, FOV, camera basis. FSR: near, far, FOV, frame time, `view_space_to_meters`. The resolve: `clip_to_previous_clip` |

Not supplied yet (Stage 6, next): opaque-only colour (`generic_temp` through f^-1; DLSS `ColorBeforeTransparency`,
FSR opaque-only), reactive (FSR reactive, XeSS responsive mask), transparency and composition (FSR T&C, DLSS transparency
hint) and the DLSS bias mask. The D3D11 `rsf_native_sr_evaluate` path takes these only through the
`RSF_GAME_RENDER_TRANSLUCENCY` scope protocol (`native_translucency`). A cheap Stage 6 therefore needs either that scope
protocol from the adapter or a generic per-pass entry for the masks. Neither was cheap enough to add here.

**Overlay.** With `SRStage=5` the adapter offers the backends whose runtime DLLs are in the configured directories:
`sl.interposer.dll` and `nvngx_dlss.dll` for DLSS, the FidelityFX upscaler and loader for FSR3 and FSR4, and `libxess.dll`
for XeSS. Qualities are Native to Performance, and the DLSS preset is available as the user's choice. The overlay fixes
its choices when it configures, cold and before a device exists, so whether a backend runs on this GPU is reported later
through `sr_report`. The report gives the effective backend and quality, the render and output sizes, frame counts, the
inputs the pass actually carried (colour, depth and jitter; motion only with a target; exposure only when supplied) and
the jitter. It also gives a reason, for example "XeSS refused on this GPU", "The SR pipeline needs DLSS support on this
GPU", "SR disabled by setting" or the pipeline refusal code. Settings apply at the frame boundary. FG stays unavailable.
Without `SRStage=5` the panel still reports SR as unavailable. `overlay_service.cpp` reaches the SR stage through the
header-only `sr_overlay.h` link, so neither compiles against the other.

**Diagnostics.** Every 120 frames with `SRStage=4` or `5`, after the Stage 2 and 3 lines:

```text
Anomaly SR stage 5 evaluated dlss= fsr3= fsr4= xess= | refused not_running= disabled= prepare= pass= (sizes= camera= projection= jitter= resources=) pipeline= (invalid= abi= not_running= streamline= not_supported= resource= decode= frame= evaluate= other=) reinsert= | resets first= device= settings= sequence= cut= | prepared= bilinear= motion_missing=
Anomaly SR stage 5 pipeline bound= running= backend= requested= switch= quality=applied/requested preset= render=WxH output=WxH frames_evaluated= frames_refused= last= (name) | starts= start_failed= (last r) stops= selects=ok/total qualities=ok/total presets= resizes=ok/total | SR= exposure= first_person_zero= | reason
```

Expected in a steady world view with SR on: the active backend's `evaluated` equals `swapped`, `prepared == swapped`,
`bilinear = 0`, every `refused` counter 0, `motion_missing = 0` with `Motion=1`, and resets only at load, menu,
`vid_restart`, settings changes and cuts. The pipeline's own lines (start, backend, render size, first evaluate) go to the
same adapter log.

**Capture** (`SRStageCapture=1` or F10, the core frame dump service): `anomaly_seam_fp16_input`, `anomaly_sr_color`
(HDR), `anomaly_sr_depth`, `anomaly_sr_motion` (prepared motion, dense x20), `anomaly_motion_target` (raw RTV2),
`anomaly_sr_output` (the reconstruction, Stage 5), `anomaly_seam_generic0` and `final_color`. In Stage 5 a captured frame
also asks the pipeline for its own guides through `request_dump`. They are written to
`<dump dir>\sr-pipeline-<session>\frame_<id>_{input,output,motion,motion_decoded,motion_submitted,depth,exposure}.bin`
with `_frame.json`, which holds the submitted jitter, motion scale, reset and clip-to-previous matrix. The pipeline's
`rsf_input_dump_texture` guides do not join the adapter's record, because they need an SR pass scope.

**Tests without the game** (`build/wk-sr5`, Release; `tests/anomaly_support.cmake`):

- `anomaly_sr_pass_contract`, 7 cases:
  - The decode parameters, and their CPU reference on (0,0,1,0) against (0,0,0,0) and on the 0.5 threshold.
  - The X-Ray projection planes.
  - The camera handed over: `clip_to_previous_clip` reprojects a world point exactly, and a reset gives identity.
  - Jitter units, the depth convention, rectangles and flags.
  - Every `build_pass` refusal: sizes, resources, camera, projection, and jitter (not applied, scope, stale size, non-finite).
  - The reset order, the cut rule, the settings sanitiser and the plan bounds.
- `anomaly_sr_runtime_api_contract`, 4 cases. It is a client that imports the Runtime DLL:
  - The export's refusals, with nothing written on failure.
  - The idle pipeline's answers, and argument refusals before any device use (short pass, short setup, ABI 4 setup,
    zero decode scale).
  - The adapter's table validation on tampered tables.
  - The driver lifecycle on a fake table. It checks the start setup (engine, project, `standard_depth`, validity decode,
    directories) and settings at the boundary only. It checks that a refused FSR4 is not retried, that SR=0 and a
    resolution change behave as above, and that a device change stops and restarts. It checks the reset reasons in the
    submitted flags, counted pipeline and build refusals, the overlay report reasons and the log lines. A Stage 4 driver
    never starts.
- `anomaly_sr_temporal_device` (Intel UHD Graphics, debug layer on, 0 errors), with the production parameters on the Runtime
  graphics library's decode and resolve:
  - Decode: (0,0,1,0) decodes to a valid (0, 0), and (0,0,0,0) and any validity of 0.5 or below decode to (-1000, -1000).
    The stored-zero rule misreads (0,0,1,0) as unwritten.
  - Resolve: valid zero stays (0, 0), and the unwritten texel gets -0.366 px of camera motion, equal to the CPU
    reprojection.
  - Prepare: f^-1 worst relative error 8.7e-4. The motion mapping, with the first-person rule on and off, matches in all
    128 texels.
  - Reinsert: worst difference 0 UNORM8 codes against `saturate(f(x))`.
- `anomaly_sr_provider_device`: an RTX 4070 Laptop GPU, with the vendor runtimes of
  `build/releases/rdr2/stage-rdr2-a2bcc0c3/ReScaleFrame`. It is registered when `RSF_ANOMALY_SR_VENDOR_ROOT` (default: the
  newest RDR2 stage) exists. The fixture follows the seam sequence: `TemporalDriver` bound by name, start at the boundary,
  `make_temporal_targets`, prepare, `build_pass`, Runtime evaluate, reinsert into an engine-like UNORM8 `generic0`. It
  renders 24 jittered frames per backend at 960x540 output:

  | Run | Render | Frames evaluated | Converged error, mean / worst UNORM8 codes |
  | --- | --- | --- | --- |
  | DLSS | 640x360 | 24/24 | 0.28 / 0.9 |
  | DLSS, auto exposure | 640x360 | 24/24 | 0.28 / 0.9 |
  | FSR3 | 640x360 | 24/24 | 0.34 / 1.2 |
  | FSR4 (INT8 compatibility path) | 640x360 | 24/24 | 0.31 / 1.1 |
  | XeSS | 565x318 | 24/24 | 0.25 / 0.7 |
  | DLSS again | 640x360 | 24/24 | 0.28 / 0.9 |

  End to end, one DLSS frame after a 0.5 m camera move was checked against the pipeline's dump of the dense motion it
  submitted (`R16G16_FLOAT`). All 122,320 valid-zero and first-person pixels stayed zero, all 108,080 unwritten pixels
  matched the camera reconstruction, and no sentinel reached DLSS.
- `anomaly_sr_hooks_contract`: Stage 4 and 5 key parsing, plus the earlier cases.
- `ctest -R "anomaly|thin|motion|overlay|present_api|dlss|sr_"`: 102 of 102 passed, with 3 opt-in skips. The full `ctest`
  passed 192 of 192, with 11 opt-in skips.
- Opt-in fixtures that ctest skips:
  - `rsf_sr_pipeline_test` with the same runtimes (AC7 conventions, `standard_depth = 0`): every backend evaluates.
  - The shared-host D3D11-to-D3D12 Streamline fixture, `fixtures/fg/Release/rsf_d3d11_present_bridge_test.exe <streamline>
    0x10de`, and its `--switch <fidelityfx> <xess>` form, with `RSF_OVERLAY_DLL` set to `target/release/rescaleframe_overlay.dll`:
    PASS.
  - The RDR2 D3D12 shared-host fixture, `rsf_unity_sr_d3d12_test` for backends 1, 3, 4 and 5 with `RSF_RDR2_HOST_INI`,
    `RSF_RDR2_HOST_ROOT` and `RSF_OVERLAY_DLL`: PASS.
- `py -3 -I tests/anomaly_sr_evidence.py --verify` and `--measure-self-test`, and `tests/anomaly_motion_evidence.py --verify`:
  PASS.

Logs: `.local/anomaly/sr-stage5/`.

**Live acceptance (to run).** Use the same save in l01_escape as Stages 2 and 3, with `Mode=Observe`, `Motion=1` and
`SRStage=5`. Compare with native (`SRStage=0`) and with spatial (`SRStage=3`) at the same view, and take an
`SRStageCapture`/F10 of each check:

1. Stationary convergence: on a still view, distant fences, wires and foliage edges settle within about a second, with no
   residual Halton crawl. The counters show `evaluated == swapped` and `bilinear = 0`.
2. Panning: a slow and a fast mouse pan, with no smearing of static geometry. The sky, and terrain with motion validity 0,
   follow the camera reconstruction.
3. Weapon ghosting: the weapon at rest, in sway and in an aim transition, with no trail behind it against the sky or
   walls. Repeat with `SRFirstPersonZero=0` to see what the rule changes.
4. Foliage: trees and grass in wind, with no ghosting or shimmer worse than native. `MotionFoliage=1`.
5. PDA text: the PDA open and scrolled, with legible text and no smear while scrolling. No reactive mask yet (Stage 6).
6. Scope alternation: scope in and out repeatedly. Scope frames stay stock (`skipped scope`), and after a scope frame the
   main view resumes without a history reset or contamination: `resets` does not grow per scope frame.
7. Reset on load and teardown: a level load, the menu and `vid_restart` each add one `resets sequence` (or `device`). A
   quit or quiesce logs the pipeline stop with no fault.
8. Exposure A/B: `SRExposure=constant` against `auto` during eye adaptation (dark interior to daylight). Compare history
   stability and banding.
9. Each backend: DLSS, FSR3, FSR4 and XeSS from the panel at each quality. Check the render size the counters report, the
   panel's effective backend and reason, and the absence of a hitch beyond the switch frame.

Before any deploy, run the shared-host fixture above, as it was run here.

**Open or UNVERIFIED.**

- The shared D3D11 pipeline starts only where DLSS is supported. On a GPU without DLSS it refuses at start, and FSR and
  XeSS are not reachable through it. This is an existing property of `rsf_dlss_pipeline_start`, reported rather than
  changed.
- Streamline logs "presentCommon() was not observed" in the fixture. The D3D11 pipeline loads Streamline with manual
  hooking and no Present hook, which is AC7's D3D11 path too. Whether its bookkeeping or garbage collection matters over
  a long session is UNVERIFIED.
- The dense resolve is called without the current jitter at both pipeline sites, so camera-reconstructed pixels carry
  up to half a render pixel of offset. This is a generic follow-up.
- RSF colour transport for DLSS was designed against AC7's dark HDR. Its effect on Anomaly's `x` range is UNVERIFIED.
- The first-person `d < 0.02` rule, the exposure choice (2.6), the cut thresholds, and the interaction of unclamped FP16
  writes with the reconstruction all need game measurements.
- SVP (scope) frames render at native size, and their cost is unmeasured.

## 13. Ghosting on moving characters and shimmer around poles (13 November)

User report with `SRStage=5` and DLSS: heavy ghosting on moving characters and shimmer around power poles. Built and
fixture-tested, not game-tested.

**What an invalid pixel becomes.** The motion target marks a pixel valid only through its validity channel. Every
other pixel is decoded to the sentinel and reconstructed from depth and the camera (section 12). For anything that moves
by itself that is wrong by the object's own motion, and DLSS then ghosts it. `anomaly_motion_contract mover` shows
this on the CPU: a walking, turning, skinning character gets the true vector to 3e-5 px through the adapter chain, while
camera-only reconstruction misses it by 20 px. `anomaly_sr_temporal_device thin` shows it through the production decode
and resolve on the RTX 4070: an unwritten mover misses by at least 3.4 px.

**Characters, cause 1: tessellated materials (`r4_enable_tessellation on`).** `uber_deffer.cpp:100-144` builds the HQ
element of a PN or displacement material as VS, `DX11\tess.hs`, `DX11\tess.ds` and PS from the same `deffer_model_bump`
and `deffer_base_bump` roots the motion stage allowlists. The game's shader cache holds
`deffer_model_bump(tess_pn,)_0` to `_4.vs`, so skinned models use it. The HQ element is chosen within `r__dtex_range`
(50 m in the run profile) of the camera (`r4.cpp:68-76`), so it affects near characters only. The motion stage swapped
in its VS/PS pair whatever was bound between them. `tess.ds` writes plain `v2p_bumped`, so the motion PS read
TEXCOORD12/13 that nothing wrote. `anomaly_motion_tess_device` reproduces this with first-party HLSL: the debug layer
reports "Domain Shader - Pixel Shader linkage error ... (TEXCOORD,12)", and the RTX 4070 without the layer gives the PS
(0, 0) with validity 0. The CPU counters still called these draws valid. Fix: with a hull, domain or geometry shader
bound, the pair is not swapped (`intermediate_stage_bound`, as `core/README.md` requires). The draw gets the
invalidation replay and is counted under `tessellated`. That makes the result defined but still camera-only. Real
motion for these draws needs HS/DS motion variants from the shader tool, plus hooks on the separate HS/DS
`create_shader` template (`r4.cpp:934`, not the hooked `0x838de0`), plus binding at draw time. Until then the user can
confirm the cause with `r4_enable_tessellation off`.

**Characters, cause 2: single-bone parts (`unsupported_kind`).** CSkeletonX `RM_SINGLE` children (weapons held by
NPCs, gear, many first-person weapon parts) render with `SKIN_0`. The engine folds the bone into the world
(`SkeletonX.cpp:67-75`: `W = m_w * LL_GetTransform_R(RMS_boneid)`, `set_xform_world`) before the draw. The policy refused
them as `unsupported_kind`: 6 per frame indoors (first-person weapon) and 19-20 per frame with NPCs in view. The cache
holds a `_0` permutation for every model root and no SKIN_NONE model permutation, so these draws are single-bone, not
software-skinned. Fix: the snapshot taken at the draw-time flush already holds the bone-composed world, so
`to_snapshot` accepts `SingleBone` as a rigid snapshot. `decide_core` makes it valid on a `CoreModel` `SKIN_0` variant
with the previous composed world and no palette, which is what the shader ABI asks for. Software skinning and trees on a
core shader stay `unsupported_kind`.

Verified and correct: identified skinned draws use the previous frame's palette, read at the draw (not the same frame),
and the object's previous world. A distant NPC in the wave 7 run had valid, plausibly scaled vectors.

**Poles.** Wave 7 dumps (`d2`, DLSS, moving camera): a power pole is static world with valid shader motion. Terrain and
sky around it are camera-reconstructed. On four moving-camera frames the shader vectors and the depth reconstruction of
the same static pixels agree to a median of 0.003-0.009 px (p90 at most 0.035 px), so the pole edge has no conflicting
vectors. The game worker measured a static pole at about 0.001 px and saw no change with `SRMipBias` off, and jitter
passed (0.04 px whole frame). The `thin` fixture checks a 0.75 px pole with jitter and a moving camera: the
pole/background step equals the true parallax step to 0.014 px. The resolve without jitter (as `dlss_pipeline.cpp`
calls it) reconstructs the ground to 0.026 px, and to 0.004 px when given the jitter. That generic gap is real but too
small to cause visible shimmer. No motion, jitter or depth cause was found for the pole shimmer, and it was not
reproduced. The remaining candidates need a capture: the output-size passes after the seam read the
nearest-upsampled, jittered render-size `rt_Position` and depth (section 5), which can flicker by one render pixel at
silhouettes against the sky (DOF, screen-space sunshafts and other effects that are off in the test profile). Another
candidate is alpha-tested wires at the run's total mip bias (`r__tf_mipbias -0.5` plus log2 of the render scale).
Reactive or transparency masks would not help alpha-tested geometry: it is opaque, with depth and valid motion, and
masks only reduce history. The D3D11 SR API has no mask channel yet (Stage 6).

**Diagnostics added.** Every 120 frames: `Anomaly motion classes | valid ... | invalid ... | unpaired ...` by class
(`static_world, rigid, skinned, single_bone, soft_skinned, tree, detail, no_visual, tessellated`), and
`Anomaly motion unpaired (...)`, which lists the top ten stock VS+PS root pairs with their class and count.

**Vendor directories.** The carrier hands over a long-path INI name, so the default directories reached Streamline as
`\\?\D:\...\streamline`. Streamline appends `/sl.common.dll` and no plugin loaded (wave 7 c1). `plain_directory` (in
`sr_settings.h`) strips `\\?\` and `\\?\UNC\` and uses backslashes, for the defaults and explicit keys alike. A refused
start now logs "SR is OFF (bilinear upscale)" with the three directories and their keys. The overlay reason says "SR off
(bilinear): Streamline failed to start; check SRStreamlineDir in the log" instead of the generic "did not start".

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

RSF: `runtime/orchestrator/include/rescaleframe/{native_sr.h, dlss_pipeline.h, d3d11_sr_api.h, sr_bridge.h,
native_sr_d3d12.h, d3d11_present_api.h, overlay_api.h}`, `runtime/orchestrator/src/{native_sr.cpp, dlss_pipeline.cpp,
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
