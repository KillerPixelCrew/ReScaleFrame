# Frame generation contracts: DLSS-G, FidelityFX, XeSS-FG

Written 7 September 2026 from the vendored and referenced SDKs, not from summaries. Revisions:
Streamline 2.12.0 (`vendor/streamline`, `references/Streamline`), FidelityFX SDK
`60f4ea81909200d8542eca14dccb2628b763a9a3` (FrameGeneration 4.0.1), XeSS SDK 3.0.2
(`8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0`). This is the evidence behind the vendor-neutral
contract in the [representation plan](../representation-plan.md).

## None of them runs on D3D11

| SDK | Frame generation API | Evidence |
| --- | --- | --- |
| DLSS-G | D3D12, Vulkan | `sl.dlss_g.dll` manifest `"rhi" : ["d3d12", "vk"]` (the DLL is binary-only; `sl.dlss` ships `dlss.json` with `["d3d11","d3d12","vk"]`); the DLL contains `"%s is not supported on D3D11!"`; the DLSS-G guide's requirement check tests only `eD3D12Supported` and `eVulkanSupported` (`ProgrammingGuideDLSS_G.md:180-182`); MFG "support is currently limited to D3D12, only" (`:565`) |
| FSR 3.1 FG | D3D12 | `Kits/FidelityFX/backend/` holds only `dx12`; the swap chain "for DirectX 12 implements `IDXGISwapChain4`" (`frame-interpolation-swap-chain.md:49-60`); no D3D11 mention in any technique doc |
| XeSS-FG | D3D12 | `xess_fg_developer_guide_english.md:75-84`: "DirectX 12", Intel driver 32.0.101.7029+ or any non-Intel GPU with SM 6.4, XeLL 1.3.0+ required; "On non-Intel GPUs the maximum number of generated frames is one"; only `xefg_swapchain_d3d12.h` exists |

Super resolution on D3D11: DLSS-SR yes (`dlss.json`), XeSS-SR yes but Intel Arc only
(`xess_d3d11.h:29`, guide `:611-616`, separate `libxess_dx11.dll`), FSR-SR no in the vendored
SDK (ffx_api ships DX12 only). Consequence for a D3D11 game: every frame generation path, and two of
three super resolution paths, go through a DX11 to DX12 presentation bridge.

## One UI contract

All three want the same two inputs and the same blend:

| Term | DLSS-G (`ProgrammingGuideDLSS_G.md:263-264`) | FidelityFX (`ffx_framegeneration.h`, `FrameInterpolationSwapchainUiComposition.hlsl:31-41`) | XeSS-FG (`xess_fg_developer_guide_english.md:571-582, 854-872`) |
| --- | --- | --- | --- |
| HUD-less colour | "full viewable scene, without any HUD/UI", same colour space and post-processing as the back buffer, same dimensions unless extents are given | `HUDLessColor` "may be empty"; used for UI extraction from the back buffer | "full scene color buffer with all post-processing applied, same texture format and color space as the back buffer, but without any UI or HUD" |
| UI | `UI Color and Alpha`: RGB **premultiplied by alpha**, alpha 0 where no UI, non-zero where UI; or `UI Alpha` alone (preferred for performance; if both, only alpha is used); formula `Final = UI.RGB + (1 - UI.A) * Hudless.RGB`; "do NOT use formats like R10G10B10A2" | `RegisterUiResourceDX12{uiResource, flags}` with `FFX_FRAMEGENERATION_UI_COMPOSITION_FLAG_USE_PREMUL_ALPHA`; shader `(1 - ui.a) * color + ui.rgb` under `FFX_UI_PREMUL`, `lerp` otherwise | `XEFG_SWAPCHAIN_RES_UI`: "same texture format, color space, and size as the back buffer"; `FinalColor = UIonly.RGB + (1 - UIonly.Alpha) * HUDlessColor.RGB`; premultiplied by default, `XEFG_SWAPCHAIN_INIT_FLAG_UITEXTURE_NOT_PREMUL_ALPHA` opts out |
| Lifetime | tag `eValidUntilPresent`; "if the tagged buffers are going to be reused, destroyed or changed in any way before the frame is presented, their life-cycle needs to be specified" | `ENABLE_INTERNAL_UI_DOUBLE_BUFFERING`: "the application can safely reuse the UI texture immediately after Present() returns. When not set, the application must keep the UI texture valid until async composition of the current frame is complete"; HUD-less "needs to be double buffered by the application" under async compute | `XEFG_SWAPCHAIN_RV_UNTIL_NEXT_PRESENT` or `RV_ONLY_NOW` per tag |
| When not eligible | "all tags should be set to null pointers" during loading, pause, menu, video (`:345`) | `frameGenerationEnabled = false` and an empty UI resource | disable during full-screen menus and loading (`:541`) |

So one layer serves all three: `R8G8B8A8_UNORM` at back-buffer extent (never R10G10B10A2, which is
AC7's back buffer), premultiplied, alpha 0 where there is no interface, cleared to transparent
each frame, and HUD-less colour that carries the game's grade (the reinserted composite, not the
linear scene colour the debug view shows).

Quality knobs on top of the contract: DLSS-G `DLSSGOptions::enableUserInterfaceRecomposition`
("HUD and scene are interpolated separately and composited later", `:608-616`) and
`DLSSGFlags::eEnableFullscreenMenuDetection` (keys off `kBufferTypeUIColorAndAlpha`, `:595-600`);
XeSS-FG UI modes `AUTO / NONE / BACKBUFFER_UITEXTURE / HUDLESS_UITEXTURE / BACKBUFFER_HUDLESS /
BACKBUFFER_HUDLESS_UITEXTURE` (`xefg_swapchain.h:268-294`), with
`BACKBUFFER_HUDLESS_UITEXTURE` named "the preferred option for applications based on Unreal
Engine" (`guide:594-597`) and Intel's advice to try no composition first (`xefg_swapchain.h:508`);
FidelityFX's three strategies (present callback, separate UI surface, HUD-less-only extraction,
`frame-interpolation-swap-chain.md:83-92`).

## Who owns the swap chain

| SDK | Model | Consequence for a bridge |
| --- | --- | --- |
| DLSS-G | Attaches to any chain created through Streamline's proxy DXGI factory unless manual hooking is used (`:192`); manual hooking calls `slUpgradeInterface` on the factory, device and queue **before any base method is used** (`ProgrammingGuide.md:1495`); "for D3D11 there is NO proxy for a device in any scenario" (`ProgrammingGuideManualHooking.md:139-143`); the chain must be torn down and recreated on every DLSS-G on/off (`:966`); the host renders off-screen while DLSS-G is loaded | the bridge's D3D12 side creates the real chain through the upgraded factory and presents on the proxy; toggling FG rebuilds the inner chain |
| FidelityFX | `ffxCreateContextDescFrameGenerationSwapChainForHwndDX12` / `WrapDX12` / `NewDX12` return a chain FFX owns (`frame-interpolation-swap-chain.md:49-60`); `presentCallback` optional; UI resource registered per frame | the FFX chain is the inner chain; back buffers re-acquired after wrap |
| XeSS-FG | `xefgSwapChainD3D12InitFromSwapChainDesc` builds a proxy `IDXGISwapChain4` returned by `xefgSwapChainD3D12GetSwapChainPtr` (`xefg_swapchain_d3d12.h:216-232`); one proxy chain per process (`guide:359-360`); flip model; XeLL context on the same device (`xell_d3d12.h:28`) | the XeFG proxy is the inner chain; `xefgSwapChainSetPresentId` before every Present; markers through XeLL |

A Streamline peculiarity decides where super resolution runs: Streamline binds one render API and
one device per process (`ProgrammingGuide.md:578`), so DLSS-SR on the game's D3D11 device and
DLSS-G on a D3D12 device cannot share an instance. When DLSS-G owns frame generation, DLSS-SR moves
to the D3D12 proxy device through the bridge; the plan's M7 measures that round trip.

## Frame identity and eligibility

FidelityFX interpolates on `frameID` passed to `ffxDispatchDescFrameGenerationPrepare` and
`ffxConfigureDescFrameGeneration`; XeSS-FG on `xefgSwapChainSetPresentId`; DLSS-G on the frame
token from `slGetNewFrameToken` and, for latency, PCL markers with consistent IDs. The plan's
`present_index` is the contiguous per-Present counter the bridge assigns for the first two, and the
plugin's `frame_id` from the input boundary is what DLSS-G's markers need. A frame whose identity
is ambiguous is not interpolated.

Eligibility follows the same rule in all three guides: no generation during loading, video,
full-screen menus, or when the interface covers most of the screen. The plan's `screen_policy`
supplies that from engine facts (video from `UManaComponent`, context from the game's own screen
percentage table) with the heuristics named as fallbacks.

## Precedents in the references

`references/fo4test` (Fallout 4, D3D11 → D3D12 proxy for FSR3 FG): `D3D11CreateDeviceAndSwapChain`
hook, `DXGISwapChainProxy : IDXGISwapChain`, shared NT-handle textures, `OpenSharedFence`, HUD-less
copied at an engine hook, no UI resource (`DX11Hooks.cpp:42-116`, `DX12SwapChain.cpp:75-196`,
`Upscaling.cpp:495-511`). `references/fallout4-community-shaders`: the same with production
hardening, two-deep HUD-less buffers, and `DisableFrameGeneration` on a format mismatch
(`Upscaling.cpp:2177-2217`). `references/skyrim-community-shaders`: the UI buffer
(`HDRDisplay.cpp:836-892`), the alpha-op blend patch (`:928-1006`), the FFX UI registration with
premultiplied alpha (`FidelityFX.cpp:166-183`), and the flicker rule "exactly one compositor per
frame" (`:167`). `references/OptiScaler`: `Dx11wDx12SC` (hidden-HWND real chain, `IDXGISwapChain4`
proxy, shared handles and fence) and a D3D12-only HUD-fix heuristic; on D3D11 it feeds FG the
HUD-included back buffer. Its `render_ui.hlsl` / `render_ui_pm.hlsl` naming is inverted relative
to its selector; derive the maths rather than copy it. None of these implements a mod-owned UI
layer for an injected D3D11 game.
