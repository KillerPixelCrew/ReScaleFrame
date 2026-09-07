# Source map

Inspected source snapshots for the AC7 research. Links pin the exact revision. Sparse checkouts contain selected source directories; SDK binaries and recursive submodules were not fetched. No upstream source was edited.
Epic Unreal Engine links require authorized Epic GitHub access. The selected engine source is a local reference, not a resolved map of AC7 binary addresses.

| Reference | Source | What to inspect |
| --- | --- | --- |
| SpecialK | [README.md:34](https://github.com/SpecialKO/SpecialK/blob/5e0c979e5a2a3d6168114cb5b6a531faffedf338/README.md#L34) | Loading options and why Special K prefers early injection |
| SpecialK | [src/injection/injection.cpp:1556](https://github.com/SpecialKO/SpecialK/blob/5e0c979e5a2a3d6168114cb5b6a531faffedf338/src/injection/injection.cpp#L1556) | CBT and shell hook installation in the payload |
| SKIF | [src/utility/injection.cpp:417](https://github.com/SpecialKO/SKIF/blob/3957f32c6d7a938537a45e9f662a5f02ca93780a/src/utility/injection.cpp#L417) | Frontend starts the separate injection host |
| reshade | [source/dxgi/dxgi.cpp:629](https://github.com/crosire/reshade/blob/358c345ca2fe64f86e67c694f8379c356627adcb/source/dxgi/dxgi.cpp#L629) | Factory interception and swap-chain wrapping |
| reshade | [source/d3d11/d3d11.cpp:25](https://github.com/crosire/reshade/blob/358c345ca2fe64f86e67c694f8379c356627adcb/source/d3d11/d3d11.cpp#L25) | Combined D3D11 device/swap-chain creation route |
| reshade | [source/dxgi/dxgi_swapchain.cpp:160](https://github.com/crosire/reshade/blob/358c345ca2fe64f86e67c694f8379c356627adcb/source/dxgi/dxgi_swapchain.cpp#L160) | COM interface forwarding reference |
| Luma-Framework | [Source/Games/Unreal Engine/main.cpp:908](https://github.com/Filoppi/Luma-Framework/blob/64bb9954d13e67873a1060466d4d54c8ad567751/Source/Games/Unreal%20Engine/main.cpp#L908) | Generic Unreal path fixes the SR scale at 1.0 |
| Luma-Framework | [Source/Games/Unreal Engine/main.cpp:721](https://github.com/Filoppi/Luma-Framework/blob/64bb9954d13e67873a1060466d4d54c8ad567751/Source/Games/Unreal%20Engine/main.cpp#L721) | TAA candidates are validated against bound resources |
| Luma-Framework | [Source/Games/Unreal Engine/includes/shader_detect.hpp:23](https://github.com/Filoppi/Luma-Framework/blob/64bb9954d13e67873a1060466d4d54c8ad567751/Source/Games/Unreal%20Engine/includes/shader_detect.hpp#L23) | Shader bytecode and constant-buffer discovery |
| Luma-Framework | [Source/Games/Unreal Engine/main.cpp:1396](https://github.com/Filoppi/Luma-Framework/blob/64bb9954d13e67873a1060466d4d54c8ad567751/Source/Games/Unreal%20Engine/main.cpp#L1396) | Per-view globals found by shape at map time, not by a fixed offset |
| Luma-Framework | [Shaders/Unreal Engine/Luma_MotionVec_UE4_Decode.hlsl:23](https://github.com/Filoppi/Luma-Framework/blob/64bb9954d13e67873a1060466d4d54c8ad567751/Shaders/Unreal%20Engine/Luma_MotionVec_UE4_Decode.hlsl#L23) | UE velocity decoding, camera reconstruction, and dilation |
| Luma-Framework | [LICENSE.md:17](https://github.com/Filoppi/Luma-Framework/blob/64bb9954d13e67873a1060466d4d54c8ad567751/LICENSE.md#L17) | Additional restrictions beyond standard MIT |
| ac7-ultrawide | [README.md:8](https://github.com/massimilianodelliubaldini/ac7-ultrawide/blob/a7048d34b83cf98dd4858d620b8e7d19d0eff811/README.md#L8) | EXE patches and 3Dmigoto installation |
| ace-combat-uevr | [src/Plugin.cpp:28](https://github.com/keton/ace-combat-uevr/blob/e4b26acdb98c3c01d33a58fa53c719f1b93b9f5b/src/Plugin.cpp#L28) | AC7 camera and cockpit compatibility logic |
| UEVR | [src/mods/vr/FFakeStereoRenderingHook.cpp:275](https://github.com/praydog/UEVR/blob/4ee5c6b6162dee2291fc75f9dfc57667f6d45a2d/src/mods/vr/FFakeStereoRenderingHook.cpp#L275) | Engine tick discovery reference |
| UEVR | [src/mods/vr/RenderTargetPoolHook.cpp:64](https://github.com/praydog/UEVR/blob/4ee5c6b6162dee2291fc75f9dfc57667f6d45a2d/src/mods/vr/RenderTargetPoolHook.cpp#L64) | Named render-target discovery reference |
| UEVR | [LICENSE:3](https://github.com/praydog/UEVR/blob/4ee5c6b6162dee2291fc75f9dfc57667f6d45a2d/LICENSE#L3) | No general reuse grant in this snapshot |
| UESDK | [src/sdk/UGameEngine.cpp:25](https://github.com/praydog/UESDK/blob/ef3984803c6d481346650a4cec587343b5e53409/src/sdk/UGameEngine.cpp#L25) | Engine tick discovery strategies |
| UE4SS | [UE4SS/src/Signatures.cpp:183](https://github.com/UE4SS-RE/RE-UE4SS/blob/24b126628ea99671e592cd76048655b2313d7c19/UE4SS/src/Signatures.cpp#L183) | Game-specific signature override mechanism |
| patternsleuth | [patternsleuth/src/resolvers/unreal/game_loop.rs:31](https://github.com/trumank/patternsleuth/blob/1d90b02c7610b595f940af04ce97e0f6feb06923/patternsleuth/src/resolvers/unreal/game_loop.rs#L31) | Outer frame loop discovery by string references |
| skyrim-community-shaders | [src/Features/Upscaling/DX12SwapChain.cpp:100](https://github.com/community-shaders/skyrim-community-shaders/blob/09ea74f66e12bad462b9ae4f16a9f6cacc02993d/src/Features/Upscaling/DX12SwapChain.cpp#L100) | DX11/DX12 shared fences |
| fallout4-community-shaders | [features/Upscaling/src/DX12SwapChain.cpp:702](https://github.com/northaxosky/fallout4-community-shaders/blob/c256c11d4024b9c95b15019b769d52bc3363470a/features/Upscaling/src/DX12SwapChain.cpp#L702) | DX11-facing proxy with DX12 presentation |
| fallout4-community-shaders | [features/Upscaling/src/UpscalingAnchors.h:6](https://github.com/northaxosky/fallout4-community-shaders/blob/c256c11d4024b9c95b15019b769d52bc3363470a/features/Upscaling/src/UpscalingAnchors.h#L6) | Game-specific engine anchors |
| OptiScaler | [OptiScaler/upscalers/xess/XeSSFeature_Dx11.cpp:2](https://github.com/optiscaler/OptiScaler/blob/da70e61e1542a0b99adcb24168ff941e42109567/OptiScaler/upscalers/xess/XeSSFeature_Dx11.cpp#L2) | Native DX11 XeSS backend |
| OptiScaler | [OptiScaler/framegen/xefg/XeFG_Dx12.cpp:304](https://github.com/optiscaler/OptiScaler/blob/da70e61e1542a0b99adcb24168ff941e42109567/OptiScaler/framegen/xefg/XeFG_Dx12.cpp#L304) | Capability-based MFG configuration |
| OptiScaler | [OptiScaler/inputs/FG/Upscaler_Inputs_Dx11wDx12.cpp:47](https://github.com/optiscaler/OptiScaler/blob/da70e61e1542a0b99adcb24168ff941e42109567/OptiScaler/inputs/FG/Upscaler_Inputs_Dx11wDx12.cpp#L47) | Existing DX11-to-DX12 FG input path |
| xess | [doc/xess_sr_developer_guide_english.md:611](https://github.com/intel/xess/blob/8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0/doc/xess_sr_developer_guide_english.md#L611) | Native DX11 supports the Intel-optimized implementation |
| xess | [doc/xess_sr_developer_guide_english.md:367](https://github.com/intel/xess/blob/8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0/doc/xess_sr_developer_guide_english.md#L367) | Input motion-vector conventions |
| xess | [doc/xess_fg_developer_guide_english.md:444](https://github.com/intel/xess/blob/8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0/doc/xess_fg_developer_guide_english.md#L444) | MFG allocation limit and dynamic frame-count selection |
| xess | [doc/xess_fg_developer_guide_english.md:28](https://github.com/intel/xess/blob/8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0/doc/xess_fg_developer_guide_english.md#L28) | HUD-less and UI-only composition choices |
| xess | [doc/xess_fg_developer_guide_english.md:699](https://github.com/intel/xess/blob/8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0/doc/xess_fg_developer_guide_english.md#L699) | Validity, copy, and reuse contracts |
| xess | [doc/xess_fg_developer_guide_english.md:807](https://github.com/intel/xess/blob/8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0/doc/xess_fg_developer_guide_english.md#L807) | Low-res undilated versus high-res dilated vectors |
| xess | [doc/xell_developer_guide_english.md:292](https://github.com/intel/xess/blob/8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0/doc/xell_developer_guide_english.md#L292) | Sleep before input sampling and frame-ID ordering |
| xess | [doc/xess_naming_structure_and_examples_english.md:7](https://github.com/intel/xess/blob/8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0/doc/xess_naming_structure_and_examples_english.md#L7) | Separate SR, FG, and latency feature names |
| xess | [inc/xess_fg/xefg_swapchain.h:188](https://github.com/intel/xess/blob/8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0/inc/xess_fg/xefg_swapchain.h#L188) | Header-level capability contract |
| XeSSInspector | [README.md:17](https://github.com/GameTechDev/XeSSInspector/blob/e72902915e95ce4b1782582297b1e9d65d6273ec/README.md#L17) | SDK inputs, frame dumps, and XeLL marker validation |
| XeSSUnrealPlugin | [README.md:44](https://github.com/GameTechDev/XeSSUnrealPlugin/blob/ff24958095288eff63a9ff0f88a4a88855b94d77/README.md#L44) | Editor/project integration, not a binary-game drop-in |
| Streamline | [README.md:119](https://github.com/NVIDIA-RTX/Streamline/blob/e8aaa6eaac968711fb62473d4ae8256dde20919b/README.md#L119) | Current NVIDIA feature surface |
| Streamline | [docs/ProgrammingGuideDirectSR.md:13](https://github.com/NVIDIA-RTX/Streamline/blob/e8aaa6eaac968711fb62473d4ae8256dde20919b/docs/ProgrammingGuideDirectSR.md#L13) | DirectSR is a DX12 SR route, not the complete vendor SR/FG stack |
| FidelityFX-SDK | [README.md:7](https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK/blob/60f4ea81909200d8542eca14dccb2628b763a9a3/README.md#L7) | Separate AMD upscaling, interpolation, and presentation components |
| dxvk-remix | [README.md:5](https://github.com/NVIDIAGameWorks/dxvk-remix/blob/e876135b37295dc203ccdb7b20a8089629588201/README.md#L5) | D3D9 fixed-function renderer replacement scope |
| egui-directx11 | [src/lib.rs:234](https://github.com/NekomaruQwQ/egui-directx11/blob/38201f9b9620a26c3e3740775fcaf24487f1dde1/src/lib.rs#L234) | DX11 state restoration responsibility |
| egui-directx11 | [Cargo.toml:19](https://github.com/NekomaruQwQ/egui-directx11/blob/38201f9b9620a26c3e3740775fcaf24487f1dde1/Cargo.toml#L19) | Renderer and egui version coupling |
| skyrim-community-shaders-vulkan | [.gitmodules:6](https://github.com/community-shaders/skyrim-community-shaders/blob/eeb28dda597d220b54e65b4a1007fbd638b5e63a/.gitmodules#L6) | Custom Streamline and DXVK dependencies |
| Streamline-CS | [include/sl_xess.h:6](https://github.com/doodlum/Streamline/blob/579442cb54615dddb8f34a78def6f5ca7d4ec659/include/sl_xess.h#L6) | Custom Vulkan XeSS plugin covers SR only |
| Streamline-CS | [source/plugins/sl.xess/xessEntry.cpp:6](https://github.com/doodlum/Streamline/blob/579442cb54615dddb8f34a78def6f5ca7d4ec659/source/plugins/sl.xess/xessEntry.cpp#L6) | Native Vulkan XeSS-SR calls through the custom plugin |
| UnrealEngine-4.18 | [Engine/Source/Runtime/Launch/Private/LaunchEngineLoop.cpp:3092](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Source/Runtime/Launch/Private/LaunchEngineLoop.cpp#L3092) | Outer frame loop precedes platform/gamepad input and engine tick |
| UnrealEngine-4.18 | [Engine/Source/Runtime/Renderer/Private/SceneRendering.cpp:2178](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Source/Runtime/Renderer/Private/SceneRendering.cpp#L2178) | View-family frame identity and render-command handoff |
| UnrealEngine-4.18 | [Engine/Source/Runtime/Renderer/Private/DeferredShadingRenderer.cpp:1343](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Source/Runtime/Renderer/Private/DeferredShadingRenderer.cpp#L1343) | Scene post-processing receives the velocity target |
| UnrealEngine-4.18 | [Engine/Source/Runtime/Renderer/Private/PostProcess/PostProcessing.cpp:885](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Source/Runtime/Renderer/Private/PostProcess/PostProcessing.cpp#L885) | TAA graph inputs and output insertion |
| UnrealEngine-4.18 | [Engine/Source/Runtime/Renderer/Private/PostProcess/PostProcessing.cpp:1988](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Source/Runtime/Renderer/Private/PostProcess/PostProcessing.cpp#L1988) | Spatial upscaling may be merged into tone mapping |
| UnrealEngine-4.18 | [Engine/Source/Runtime/Renderer/Private/PostProcess/PostProcessTemporalAA.cpp:826](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Source/Runtime/Renderer/Private/PostProcess/PostProcessTemporalAA.cpp#L826) | TAA resource dimensions, pixel/compute paths, and depth |
| UnrealEngine-4.18 | [Engine/Source/Runtime/Renderer/Private/PostProcess/PostProcessTemporalAA.cpp:1122](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Source/Runtime/Renderer/Private/PostProcess/PostProcessTemporalAA.cpp#L1122) | TAA output inherits input dimensions |
| UnrealEngine-4.18 | [Engine/Source/Runtime/Renderer/Private/SceneVisibility.cpp:2248](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Source/Runtime/Renderer/Private/SceneVisibility.cpp#L2248) | Pixel jitter and projection modification |
| UnrealEngine-4.18 | [Engine/Source/Runtime/Engine/Private/SceneView.cpp:2284](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Source/Runtime/Engine/Private/SceneView.cpp#L2284) | Camera reprojection and temporal jitter uniforms |
| UnrealEngine-4.18 | [Engine/Shaders/Private/Common.ush:1585](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Shaders/Private/Common.ush#L1585) | Packed velocity decoding |
| UnrealEngine-4.18 | [Engine/Source/Runtime/SlateRHIRenderer/Private/SlateRHIRenderer.cpp:601](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Source/Runtime/SlateRHIRenderer/Private/SlateRHIRenderer.cpp#L601) | `DrawWindow_RenderThread`; the `r.HDR.UI.CompositeMode` UI target, HUD-less snapshot and composite (`:611-702, 768-870`) |
| UnrealEngine-4.18 | [Engine/Source/Runtime/SlateRHIRenderer/Private/SlateRHIRenderingPolicy.cpp:726](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Source/Runtime/SlateRHIRenderer/Private/SlateRHIRenderingPolicy.cpp#L726) | Slate blend states: alpha accumulates as `BF_One, BF_InverseSourceAlpha` |
| UnrealEngine-4.18 | [Engine/Source/Runtime/SlateRHIRenderer/Private/SlateShaders.cpp:51](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Source/Runtime/SlateRHIRenderer/Private/SlateShaders.cpp#L51) | `FSlateVertexDeclaration`: the five-element, 40-byte input layout used as the Slate fingerprint |
| UnrealEngine-4.18 | [Engine/Source/Runtime/Renderer/Private/BasePassRendering.h:1105](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Source/Runtime/Renderer/Private/BasePassRendering.h#L1105) | `BLEND_Translucent` alpha `BF_Zero, BF_InverseSourceAlpha`: alpha stays 0 from a transparent clear |
| UnrealEngine-4.18 | [Engine/Source/Runtime/UMG/Private/Components/WidgetComponent.cpp:268](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Source/Runtime/UMG/Private/Components/WidgetComponent.cpp#L268) | World-space widget scene proxy: a quad reading the widget render target through `Widget3DPassThrough` materials (`:573-588`, `:1391-1445`) |
| UnrealEngine-4.18 | [Engine/Source/Runtime/UMG/Private/Slate/WidgetRenderer.cpp:68](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Source/Runtime/UMG/Private/Slate/WidgetRenderer.cpp#L68) | `CreateTargetFor` (`PF_B8G8R8A8`, transparent clear) and `DrawWindow` geometry `MakeRoot(DrawSize / Scale, Scale)` |
| UnrealEngine-4.18 | [Engine/Source/Runtime/Engine/Private/GameViewportClient.cpp:1276](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Source/Runtime/Engine/Private/GameViewportClient.cpp#L1276) | Frame order: `BeginRenderingViewFamily`, then HUD `PostRender` (`:1364`), then Slate |
| UnrealEngine-4.18 | [Engine/Source/Runtime/Renderer/Private/PostProcess/SceneRenderTargets.cpp:281](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Source/Runtime/Renderer/Private/PostProcess/SceneRenderTargets.cpp#L281) | `ComputeDesiredSize`: scene buffers sized from the family size, which screen percentage shrinks |
| skyrim-community-shaders | `src/Features/HDRDisplay.cpp:836` | `SetUIBuffer`: the UI buffer redirect, transparent clear, no depth; `:928-1006` the alpha-op blend patch |
| skyrim-community-shaders | `src/Features/Upscaling/DX12SwapChain.cpp:11` | D3D11 to D3D12 interop: shared NT handles, shared fence, swap chain proxy, per-Present sync (`:217-345`) |
| fo4test | `src/DX12SwapChain.cpp:75` | The smallest working D3D11 to D3D12 proxy for frame generation; `src/DX11Hooks.cpp:42` the creation hook; `src/Upscaling.cpp:495` HUD-less capture at an engine hook |
| OptiScaler | `OptiScaler/with_dx12/dx11_with_dx12_sc.cpp:851` | `Dx11wDx12SC`: hidden-window real chain, `IDXGISwapChain4` proxy, shared handles and fence for D3D11 games |
| SpecialK | `src/render/d3d11/d3d11.cpp:3297` | Per-shader CRC32 HUD classification with a draw filter in the hooked `Draw*` entry points |
| Streamline | `docs/ProgrammingGuideDLSS_G.md:263` | HUD-less and UI colour/alpha requirements, premultiplied alpha, lifetimes, null tags when ineligible |
| FidelityFX-SDK | `Kits/FidelityFX/framegeneration/include/ffx_framegeneration.h:60` | UI composition flags; the premultiplied composite shader in `FrameInterpolationSwapchainUiComposition.hlsl:31` |
| xess | `inc/xess_fg/xefg_swapchain.h:100` | Resource types and UI composition modes; the guide's Unreal recommendation at `doc/xess_fg_developer_guide_english.md:594` |

`references/UnrealEngine` is an empty, failed clone of the full engine repository (no objects; it
needs Epic-linked GitHub access) and must not be mistaken for the 4.18.3 checkout above.
| UnrealEngine-4.18 | [Engine/Shaders/Private/PostProcessVelocityFlatten.usf:48](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Shaders/Private/PostProcessVelocityFlatten.usf#L48) | Motion blur flattening includes camera motion and polar packing |
| UnrealEngine-4.18 | [Engine/Source/Runtime/Engine/Private/GameViewportClient.cpp:973](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Source/Runtime/Engine/Private/GameViewportClient.cpp#L973) | Scene enqueue, Canvas HUD, and flush ordering |
| UnrealEngine-4.18 | [Engine/Source/Runtime/SlateRHIRenderer/Private/SlateRHIRenderer.cpp:601](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Source/Runtime/SlateRHIRenderer/Private/SlateRHIRenderer.cpp#L601) | Slate UI rendering and viewport presentation |
| UnrealEngine-4.18 | [Engine/Source/Runtime/Windows/D3D11RHI/Private/Windows/WindowsD3D11Viewport.cpp:85](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Source/Runtime/Windows/D3D11RHI/Private/Windows/WindowsD3D11Viewport.cpp#L85) | DX11 swap-chain creation route |
| UnrealEngine-4.18 | [Engine/Source/Runtime/Windows/D3D11RHI/Private/D3D11Viewport.cpp:268](https://github.com/EpicGames/UnrealEngine/blob/0a14a8d537a31ecc77488ced41dbaa0166612ef8/Engine/Source/Runtime/Windows/D3D11RHI/Private/D3D11Viewport.cpp#L268) | RHI presentation boundary |

## Repository revisions

| Repository | Revision | Clean |
| --- | --- | --- |
| 3Dmigoto | `8f329bd94fecc9bbcb9211ffd42a95dd7fe6b43e` | True |
| ac7-ultrawide | `a7048d34b83cf98dd4858d620b8e7d19d0eff811` | True |
| ace-combat-uevr | `e4b26acdb98c3c01d33a58fa53c719f1b93b9f5b` | True |
| dxvk-remix | `e876135b37295dc203ccdb7b20a8089629588201` | True |
| egui | `f08c4ff4048ede30412ce6b3d57b415b5c043fe3` | True |
| egui-directx11 | `38201f9b9620a26c3e3740775fcaf24487f1dde1` | True |
| fallout4-community-shaders | `c256c11d4024b9c95b15019b769d52bc3363470a` | True |
| fo4test | `4096d7e9984c11a211b50c7262dc6f9400d6660f` | True |
| FidelityFX-SDK | `60f4ea81909200d8542eca14dccb2628b763a9a3` | True |
| Luma-Framework | `64bb9954d13e67873a1060466d4d54c8ad567751` | True |
| OptiScaler | `da70e61e1542a0b99adcb24168ff941e42109567` | True |
| patternsleuth | `1d90b02c7610b595f940af04ce97e0f6feb06923` | True |
| reshade | `358c345ca2fe64f86e67c694f8379c356627adcb` | True |
| rtx-remix | `61b49807a1fa92812ae5791bdeced84f91a0007f` | True |
| SKIF | `3957f32c6d7a938537a45e9f662a5f02ca93780a` | True |
| skyrim-community-shaders | `09ea74f66e12bad462b9ae4f16a9f6cacc02993d` | True |
| skyrim-community-shaders-vulkan | `eeb28dda597d220b54e65b4a1007fbd638b5e63a` | True |
| SpecialK | `5e0c979e5a2a3d6168114cb5b6a531faffedf338` | True |
| Streamline | `e8aaa6eaac968711fb62473d4ae8256dde20919b` | True |
| Streamline-CS | `579442cb54615dddb8f34a78def6f5ca7d4ec659` | True |
| UE-Modding-Tools | `9b3efa4ad72afb06c582b1095c0757cf022b0e9c` | True |
| UE4SS | `24b126628ea99671e592cd76048655b2313d7c19` | True |
| UESDK | `ef3984803c6d481346650a4cec587343b5e53409` | True |
| UEVR | `4ee5c6b6162dee2291fc75f9dfc57667f6d45a2d` | True |
| UnrealEngine-4.18 | `0a14a8d537a31ecc77488ced41dbaa0166612ef8` | True |
| xess | `8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0` | True |
| XeSSInspector | `e72902915e95ce4b1782582297b1e9d65d6273ec` | True |
| XeSSUnrealPlugin | `ff24958095288eff63a9ff0f88a4a88855b94d77` | True |
