# Representation plan: retained research base

The plan itself (UI diversion, the ABI 2 frame-callback sketches, milestones and deletion lists) is
not the current design and was removed; Git history holds it. This page keeps its appendix, the
research the plan was built on, as evidence. For what is implemented and its limits see
[current status](current-status.md) and the SDK headers (`sdk/game/include/rescaleframe/`), and for
frame generation see [the frame-generation plan](frame-generation-plan.md).

## Runtime map: facts that shaped the design
- The tree does not compile (`hunt_shape_dirty` undeclared at four sites); `rsf_frame_tap_set_divert`
  declared, unimplemented; `draws_diverted` inserted mid-struct without an ABI bump.
- Draw hooks forward first then report (`frame_tap.cpp:1391-1425`); the tap does not hook
  `OMSetBlendState`, `OMSetDepthStencilState`, `PSSetShader`, `RSSetState`, `ClearDepthStencilView`.
- `overlay_renderer.cpp:543-551` holds the premultiplied blend state; `present_blit` hardcodes
  alpha 1 and no blend; no test covers either.
- The UI rule lives in `dlss_bridge.c:514-567`, against `AGENTS.md:98-101`.
- `report_geometry` skips substituted targets; `invalidate_geometry` does not reset gates.

## Reference findings (Skyrim CS, UE 4.18.3)
- Premultiplied-over everywhere: Skyrim CS `HDROutputCS.hlsl:106-111`, UE
  `CompositeUIPixelShader.usf:101-103`, FFX `USE_PREMUL_ALPHA`.
- Skyrim CS UI buffer: `R8G8B8A8_UNORM`, RTV+SRV(+UAV), cleared `{0,0,0,0}` before UI and after
  Present, no depth (`HDRDisplay.cpp:836-892`, `DX12SwapChain.cpp:112-141, 283`); diverted at one
  engine stage; one compositor per frame; alpha-ops patch `HDRDisplay.cpp:928-1006`.
- Slate alpha `BO_Add, BF_One, BF_InverseSourceAlpha` (`SlateRHIRenderingPolicy.cpp:726-732`);
  base-pass `BLEND_Translucent` alpha `BF_Zero, BF_InverseSourceAlpha` (`BasePassRendering.h:1105`);
  Slate material `BLEND_Modulate` writes RGB only; `InvertAlpha` flips the convention.
- UE's own composition path `r.HDR.UI.CompositeMode` (`SlateRHIRenderer.cpp:611-702, 768-870`),
  gated on HDR; `FSlateVertex` layout (`SlateShaders.cpp:51-62`); `WidgetComponent.cpp:268-345,
  573-588, 1391-1445`; `WidgetRenderer.cpp:68-117` (`CreateTargetFor` PF_B8G8R8A8 transparent;
  `MakeRoot(DrawSize/Scale, Scale)`); frame order `GameViewportClient.cpp:1276 → 1364 → Slate`.
- Skyrim CS bridge: shared NT handles + shared fence, no D3D11On12 (`DX12SwapChain.cpp:11-141,
  217-345`). Checkout lacks `Runtime/RHI` and `Windows/D3D11RHI`.

## Luma's generic Unreal path, read on 7 Sep 2026
Checked after a report that it delivered full DLSS in AC7; the report was withdrawn and the source
agrees with the withdrawal. `main.cpp:908` fixes `sr_render_resolution_scale` at 1.0 with the comment
`DLAA only`. It substitutes DLSS for the engine's TAA pass at matching extents and drives no
resolution of its own, so there is no working AC7 upscale to take a shortcut from. What it does
contribute is above: the mip bias (`:1486`), dynamic resolution (`:917`), CPU-side view uniforms at
`Map`/`Unmap` (`:1344-1463`), confirm-once-then-cache-by-hash (`:719-800`), and skip-and-copy
(`:800-825`).

Two of its conventions we checked and already match, so nothing changes. Jitter: Luma takes it from
the projection, `matrix_a.m20`/`m21` of `ViewToClip` (`:1410`), deliberately not from the cbuffer's
jitter field, which `shader_detect.hpp:17` calls unreliable across UE versions; it converts with
`x * width * 0.5` and `y * height * -0.5` (`:1062`). We read `TemporalAAJitter` at `0x720` and use
the identical conversion against the view rect (`view_uniforms.cpp:244-256`), and
`ac7-frame-capture.md:67` records that the field was verified equal to row 2 columns 0/1 of
`ViewToClip` in this build. The projection is the more portable source and is in the same buffer, so
it is the fallback if a game update moves the field. Its SR settings for UE — HDR colour, inverted
depth, unjittered motion vectors, auto exposure — are what `dlss_streamline.cpp:478-499` already
sends.

## The briefing tail from the run log (`.local/observe/rsf-dump.log` ~425120-425240)
Glow pass (1920x1080 → 1920x1080, no depth) → scene lighting into scene colour `0x308F4390` with
depth `0x307329D0` → four widget quads (6 indices, reading 1920x1080 widget textures, writing
1024x576 `R8G8B8A8_TYPELESS` `0x308F6550`/`0x308F6190`, **scene depth bound**) → tonemap into
composite `0x308F3FD0` → unwatched step → `0x308E2770` → AC7's UI composite pass reads
`0x308E2770` + `0x308F6550` + glow/lens → composite again → upscale to the R10G10B10A2 back buffer.
Ghidra: `FUN_141343660` = `DrawWindow_RenderThread`; gate = `DAT_143c783c3 && DAT_1436a2e96 &&
FUN_140df9ea0(...) && DAT_143c8f250+4 != 0 && FUN_141220f10()`; composites `FUN_141351120`
(scRGB) / `FUN_1413515d0` (PQ) through a 32³ LUT.

## AC7 SDK facts
Converter holders: `AUIManagerActor::FrontWindowConverter 0x0D48`, `ANimbusHUD::HudWidgetConverter
0x0470`, `HudPostProcessConverter 0x0478`, `AStereoWidgetActor` `0x0370/0x0378`.
`UWidgetToTextureConverter {DrawSize 0x28, Widget 0x30, RenderTarget 0x48, DownSampleRT 0xC0,
BlurXRT 0xC8, BlurYRT 0xD0, RenderTargetWithGlow 0xD8}`. AC7's `UWidgetComponent`:
`bEnableDownsampleRenderTarget 0x0828`, `DownsampledRenderTargetArray[2] 0x0830`, `GlowRenderTarget
0x0840`, no `Space` field in the dump. `UNimbusGameInstance::HUDWidgetRenderTexture 0x01F0`.
`AUIManagerActor::FullscreenMovieWidget 0x0D60`, `ManaComponent 0x0F10`; `UManaComponent::Play/Stop/
IsPlaying`, `EManaComponentStatus::Playing = 5`. `EGraphicsScreenPercentageSettings` Gameplay 0,
NonGameplay 1, VR 2-4, MPGameplay 5; `EGraphicsSettingsWindowsDrawScale` S50..S200. `UTexture` native
span `0x006B..0x00C7` holds `Resource` (offset unmeasured). `ANimbusHUD::DrawWidget*` immediate draws
bypass converters.

## Binary facts found this session (record in the hook map)
`FGraphicsSettingsManager`: `FUN_1405f5260` → six `FUN_1405f4590(this, x, 0..5, flag)`; FName
globals `DAT_14356eec0..ef18`; `FUN_14015fb40` references `r.ScreenPercentage` at `0x14265cbd8`.
`r.HDR.UI.CompositeMode` strings `0x142bc44c8`, `0x142bc5140`, `0x142cce798`; `FCompositePS0/1`
`0x142bc7d58`/`0x142bc7e28`; `FSlateElementVS` `0x142bc5320`; referencing functions
`FUN_140315560`, `FUN_141343660`, `FUN_14164a840`. `UWidgetToTextureConverter_Setup 0x1404d5c10`,
`_SetupVirtualWindow 0x1404d69a0`, front-end caller `0x1406243e0`, constants `0x1425f1cac`
(1920.0f) / `0x1425f1ce4` (1080.0f).

## Vendor contracts and D3D11 FG precedents
Streamline `ProgrammingGuideDLSS_G.md:264` (UI premultiplied, alpha 0 where none, not
R10G10B10A2; HUD-less same colour space/post-processing; tags `eValidUntilPresent`; null tags in
loading/menu/video `:345`; `enableUserInterfaceRecomposition`; chain recreated on toggle `:966`;
D3D11 has no device proxy). FFX `ffx_framegeneration.h:60-64` flags, composite `(1−a)·color + ui`,
`ENABLE_INTERNAL_UI_DOUBLE_BUFFERING` lifetime rule, HUD-less double-buffered under async compute,
DX12-only backend. XeFG `xefg_swapchain.h:100-111, 268-294` resource types and UI modes,
premultiplied default, `BACKBUFFER_HUDLESS_UITEXTURE` preferred for UE (`guide:594-597`), D3D12 +
flip model + XeLL, non-Intel max 1 frame; XeSS-SR D3D11 Arc-only (`libxess_dx11.dll`). Blueprints:
`fo4test` (`DX11Hooks.cpp:42-116`, `DX12SwapChain.cpp`, `Upscaling.cpp:495-511`), FO4 CS
(`Upscaling.cpp:2177-2217`), OptiScaler `Dx11wDx12SC` (hidden-HWND real chain, no D3D11 HUD-fix).
SpecialK shader-CRC HUD registry (`d3d11.cpp:3297-3313`); ReShade `generic_depth_addon.cpp` picker.

## What exists today
One backend (DLSS SR, five `eOnlyValidNow` tags, D3D11 manual hooking); `sl.dlss_g.dll` present and
unreferenced; no FSR/XeSS code; no D3D12, proxy, shared handles, fences, or frame identity;
orchestrator stub; everything real in `dinput8.dll`; Rust `rsf-upscaler` capability model unused;
`frame_assembly.h` includes `dlss.h`; output texture not shareable; overlay ABI 2 append-only with
pinned offsets; 14 documented + 13 undocumented config keys; CI never compiles the Streamline branch
nor runs `cargo test`.
