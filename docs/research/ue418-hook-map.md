# UE4.18 source map for the AC7 plugin

An authorized, selected-source checkout of `EpicGames/UnrealEngine` tag `4.18.3-release` was inspected on 5 September 2026, at commit `0a14a8d537a31ecc77488ced41dbaa0166612ef8`. This engine checkout is held outside the repository. Engine setup, dependency downloads, and builds were not run.

This is stock engine source used to understand behavior. It does not establish AC7's exact patch level, function addresses, structure offsets, or custom renderer changes. Epic source links require the linked account's access. The reference checkout remains separate from the proposed runtime codebase.

## Installed game evidence

The inspected installation contains `Ace7Game.exe` at the root of Steam's `ACE COMBAT 7` game directory.

| Item | Observed value |
| --- | --- |
| Steam application | 502500 |
| Installed build ID | 9855922 |
| Steam state flags | 4 |
| PE machine | AMD64 / x64 (`0x8664`) |
| Executable size | 65,331,992 bytes |
| SHA-256 | `c7da97f5f8a807d4f1264adbb074146fcffe9bdc2ffa98791b822cd28e558f4f` |
| Static graphics imports | `D3D11CreateDevice`, `CreateDXGIFactory`, `CreateDXGIFactory1` |
| Engine branch marker | `++UE4+Release-4.18` occurs in the executable |
| TrueSky component | `Engine/Binaries/ThirdParty/Simul/Win64/TrueSkyPluginRender_MT.dll` is installed |

The executable has no populated file/product version in the inspected Windows version metadata. The branch marker supports using UE4.18 as the reference; it does not prove the game uses stock 4.18.3. Static imports also do not enumerate dynamically resolved or delay-loaded APIs.

The executable's `.text` is encrypted on disk and the entry point sits in a Steam DRM `.bind`
section, so none of the hook leads below can be resolved to addresses from the shipped file. The
string and import evidence comes from the plaintext `.rdata`. See [ghidra-tooling.md](ghidra-tooling.md).

The read-only [fingerprint](evidence/ac7-executable.json) records sections, imports, and candidate string locations. It found `t.IdleWhenNotForeground`, `r.OneFrameThreadLag`, `r.ScreenPercentage`, `r.PostProcessAAQuality`, `r.TemporalAASamples`, `r.Tonemapper.MergeWithUpscale.Mode`, `SceneDepthZ`, and other source leads in the file. Several have multiple occurrences. These are substring matches in file data, not resolved functions or validated hooks.

## Engine boundaries to investigate

Paths below are relative to the reference engine's `Engine/` directory. Exact revision links are included in [source-map.md](source-map.md).

| Required information | Stock source entry | Why it matters |
| --- | --- | --- |
| Before-input frame start | `Source/Runtime/Launch/Private/LaunchEngineLoop.cpp`, `FEngineLoop::Tick` | Platform messages are pumped and gamepad input is polled before `GEngine->Tick`; entering `UGameEngine::Tick` is too late for before-input sleep in this stock order |
| Frame handoff | `Source/Runtime/Renderer/Private/SceneRendering.cpp`, `FRendererModule::BeginRenderingViewFamily` | Carries a view-family frame number into the render command; a better association lead than a global present-time counter |
| Scene/depth/velocity | `Source/Runtime/Renderer/Private/DeferredShadingRenderer.cpp`, velocity selection and `GPostProcessing.Process` | Passes the actual velocity target into post-processing; accounts for GBuffer versus separate velocity rendering |
| TAA graph insertion | `Source/Runtime/Renderer/Private/PostProcess/PostProcessing.cpp`, `AddTemporalAA` | Connects current colour, history, velocity, and final output inside the composite graph |
| TAA execution/output | `Source/Runtime/Renderer/Private/PostProcess/PostProcessTemporalAA.cpp`, `Process` and `ComputeOutputDesc` | Exposes input/output rectangles, pixel/compute variants, scene depth, history ownership, and output allocation |
| Jitter generation | `Source/Runtime/Renderer/Private/SceneVisibility.cpp`, temporal jitter block | Produces pixel jitter and applies it to the projection; disabling TAA can also disable jitter and clear history |
| Camera reprojection | `Source/Runtime/Engine/Private/SceneView.cpp`, view uniform setup | Provides `ClipToPrevClip` and current/previous temporal jitter |
| Velocity conventions | `Shaders/Private/Common.ush`, velocity helpers; `PostProcessTemporalCommon.ush` | Explains packed velocity and camera reprojection behavior |
| Exposure | `PostProcessTemporalAA.cpp`, eye-adaptation resource selection; `PostProcessing.cpp`, eye-adaptation graph | Shows where exposure-related data exists; validate units and timing rather than assuming a modern pre-exposure field |
| Final spatial scaling | `PostProcessing.cpp`, screen-percentage tail; `PostProcessUpscale.cpp` | Handles `ViewRect` versus `UnscaledViewRect`; scaling can be merged into tone mapping |
| Scene/HUD boundary | `Source/Runtime/Engine/Private/GameViewportClient.cpp`, `UGameViewportClient::Draw` | Queues scene rendering before Canvas HUD work and subsequent Canvas flush |
| Slate and presentation | `Source/Runtime/SlateRHIRenderer/Private/SlateRHIRenderer.cpp`, `DrawWindow_RenderThread` | Covers another UI path ending in viewport presentation; Slate alone does not cover all Canvas HUD draws |
| DX11 swap-chain creation | `Source/Runtime/Windows/D3D11RHI/Private/Windows/WindowsD3D11Viewport.cpp` | Uses the factory's `CreateSwapChain` with the DX11 device |
| Native device/present | `D3D11Device.cpp`, `RHIGetNativeDevice`; `D3D11Viewport.cpp`, `PresentChecked`/`RHIEndDrawingViewport` | Connects engine RHI objects to native DX11 device and presentation behavior |

## Changes to the proposed hook strategy

**Use an outer-frame hook for XeLL.** Stock `FEngineLoop::Tick` pumps Windows messages at line 3220, polls game devices at 3275, and calls `GEngine->Tick` at 3292. This confirms the earlier concern about relying on `UGameEngine::Tick` alone. Locate the outer loop using multiple validated references, then verify AC7's actual input order before inserting sleep. `t.IdleWhenNotForeground` and `r.OneFrameThreadLag` are present in the executable and can help discovery; they are not unique function signatures.

**Carry identity through the render command.** `BeginRenderingViewFamily` writes the view-family frame number and enqueues `FDrawSceneCommand`. This gives us a concrete place to associate the plugin's monotonic frame token with a rendered view. Stock comments specifically caution against substituting `GFrameNumberRenderThread` for this association. The RHI handoff and final present association still require tracing in AC7.

**Preserve the engine's temporal setup while replacing its evaluation.** TAA is connected before motion blur and the later tone-map path. Its output descriptor inherits the input dimensions and its ordinary draw uses matching source/destination rectangles. A larger XeSS output therefore requires coordinated graph/allocation/rectangle changes. Merely changing the TAA shader or returning another texture pointer is insufficient.

**Account for both spatial upscale routes.** `PostProcessing.cpp` can use `FRCPassPostProcessUpscale`, or set `bDoScreenPercentageInTonemapper` and remove that standalone pass. The installed executable contains the controlling `r.Tonemapper.MergeWithUpscale.Mode` name. The plugin must detect the actual route and avoid redundant scaling after SR in either case.

**Do not reuse the motion-blur flattened texture as ordinary motion.** Stock `PostProcessVelocityFlatten.usf` combines object/camera motion, applies aspect/sign adjustments, and stores a polar velocity representation with depth in the active packing path. Reuse the understanding of its camera fallback, not an assumption that the output is ready for XeSS. Supply decoded Cartesian motion with the selected SR/FG backend's units and dilation requirements.

**Capture before all relevant HUD paths.** Stock `UGameViewportClient::Draw` submits the scene before calling HUD `PostRender` and flushing Canvas. Slate draws later through its own renderer. Capture must be ordered on the render/GPU work stream; returning from a game-thread enqueue does not mean rendering is finished. AC7's target markers, cockpit UI, subtitles, and trueSky ordering remain game-specific validation items.

## Separate translucency carries no velocity, and what could be done about it

Source read on 7 September 2026 against `4.18.3-release`, commit `0a14a8d`, prompted by the briefing screen losing its relief in reconstruction. See [the capture evidence](ac7-frame-capture.md).

The second layer in that frame is stock separate translucency. `FSceneRenderTargets::GetSeparateTranslucency` allocates a pooled `PF_FloatRGBA` target cleared to black, which matches the captured `R16G16B16A16_FLOAT` target, its on-demand allocation, and the black unused one in the hangar. `BeginRenderingSeparateTranslucency` binds scene depth as `FExclusiveDepthStencil::DepthRead_StencilWrite`, so the layer tests depth and never writes it. Depth-based camera-motion reconstruction therefore cannot serve this layer: at those pixels the depth buffer describes whatever opaque surface is behind it.

Velocity excludes it by blend mode in both paths of `FVelocityDrawingPolicyFactory`, at `VelocityRendering.cpp:507` for static meshes and `:544` for dynamic ones:

```cpp
if (BlendMode == BLEND_Opaque || BlendMode == BLEND_Masked)
```

The shader side is more permissive than that gate, which is what makes a patch plausible. `FVelocityVS::ShouldCache` compiles velocity shaders when the material is the special engine material, is masked, is opaque and two-sided, or may modify mesh position. The default material is a special engine material, so its velocity shaders exist in any cooked build. Both draw paths then substitute that default proxy when the material `WritesEveryPixel()`, is not two-sided, and does not modify mesh position.

So cutting the gate is worth trying, with a known boundary. A translucent material that writes every pixel, is not two-sided, and does not modify mesh position falls into the default-material substitution and can be drawn with a shader the build already contains. A two-sided translucent material does not, and needs a permutation the cook had no reason to produce, so `SupportsVelocity()` is expected to refuse it. Sprite-like icons are the favourable case and large holographic sheets are the unfavourable one, which suits the mission-replay goal of moving vehicle symbols.

4.27 shows Epic did not solve this by relaxing the gate. It adds `EMeshPass::TranslucentVelocity` with `FTranslucentVelocityMeshProcessor`, a per-material `IsTranslucencyWritingVelocity()` opt-in, and a matching permutation condition, while leaving the opaque gate as it was. A faithful backport would mean a new pass and new permutations; the gate cut is the cheap approximation of it, not the same thing.

Reconstructing 4.27's pass and injecting it is not a route. It rests on the mesh-drawing architecture introduced in 4.22, so `FMeshPassProcessor`, `EMeshPass` and cached mesh draw commands have no counterpart in 4.18; `IsTranslucencyWritingVelocity()` reads a cooked material property AC7's materials do not carry; and the permutation it selects is compiled offline. Each of those needs the cook pipeline, not a runtime patch.

Doing it ourselves at the D3D11 level, by re-issuing the translucent draws into the velocity target with our own shader, runs into the previous transform. In 4.18 `PreviousLocalToWorld` is not part of the primitive uniform buffer. `FVelocityVS::SetMesh` sets it as a loose shader parameter per draw, from `Scene->MotionBlurInfoData.GetPrimitiveMotionBlurInfo`, and only during the velocity pass. It is not bound while translucency draws, so intercepting those draws yields the current transform and not the previous one, and we would have to match draws across frames ourselves to recover it.

That is the strongest argument for the gate cut over the alternatives: when the engine's own velocity pass runs, it supplies `PreviousLocalToWorld` from bookkeeping it already maintains. Whether translucent primitives are registered in `MotionBlurInfoData` at all is the open question, since nothing has ever needed them there; if they are not, the pass runs and writes zero motion.

Untested. No address has been located in `Ace7Game.exe`, nothing has been patched, and whether AC7's icon materials meet the substitution conditions is unknown. Before patching, establish whether the cooked shader library contains velocity permutations for those materials, since a gate cut that reaches an absent permutation gains nothing.

## Implementation follow-up

The later [capture work](ac7-frame-capture.md) established live resources, view data, jitter, and render-scale control. The research proxy now evaluates DLSS. Next, turn these stock-source leads into verified AC7 function/shader/frame identities and complete output reinsertion. The source inspection itself remains distinct from those later runtime results.
