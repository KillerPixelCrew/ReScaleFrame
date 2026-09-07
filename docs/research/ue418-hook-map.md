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

### The dynamic-mesh gate, located

Found 7 September 2026 in `Ace7Game.exe.dump` (71,385,127 bytes, entropy 6.267 after decryption), image base `0x140000000`.

The chain starts at the wide literal `L"Velocity"` at `142af9008`, the pooled render-target name from `VelocityRendering.cpp:896`. Its only referrer is `FUN_1411869c0`, which also reads `L"r.MotionBlurDebug"` and then branches between two implementations: `FDeferredShadingSceneRenderer::RenderVelocities` and its parallel and serial paths. The serial path `FUN_141186cc0` loops views, and `FUN_141184dc0` walks dynamic mesh elements and calls `FUN_141182390` with the argument list of `FVelocityDrawingPolicyFactory::DrawDynamicMesh`: command list, view, drawing context, mesh, `bPreFog`, render state, proxy, hit proxy id, instanced-stereo flag.

The gate is the first thing that function does:

```text
1411823d5  CALL qword ptr [RAX + 0x198]   ; Material->GetBlendMode()
1411823db  CMP  EAX,0x1                   ; 83 F8 01
1411823de  JA   0x14118274c               ; 0F 87 68 03 00 00  -> return false
1411823e4  MOV  RAX,qword ptr [RBX]
1411823ea  CALL qword ptr [RAX + 0x20]    ; GetMaterialDomain()
1411823ed  CMP  EAX,0x3                   ; MD_UI
1411823f0  JZ   0x14118274c
```

`CMP EAX,1` with `JA` is `BlendMode == BLEND_Opaque || BlendMode == BLEND_Masked` compiled as an unsigned compare, matching `VelocityRendering.cpp:544`. The domain check that follows is `ShouldIncludeDomainInMeshPass` and should be left alone; excluding UI-domain materials is wanted.

Proposed patch: at RVA `0x11823de`, replace the six bytes `0F 87 68 03 00 00` with six `0x90`. That drops only the blend-mode rejection and leaves the domain check, the movable test and `SupportsVelocity` in place, so a material without a usable permutation still refuses rather than drawing wrongly.

This is the dynamic path only. The static equivalent at `VelocityRendering.cpp:507`, inside `AddVelocityStaticMesh`, has not been located; it runs when a primitive is added to the scene rather than during rendering, and whichever path AC7's icons take decides whether one patch or both are needed.

### Game-tested: the icons write velocity

7 September 2026, briefing screen, `RSF_TRANSLUCENT_VELOCITY=1`, render scale 50%. The gate patched
on the expected bytes: `translucent velocity gate patched at rva 0x11823de, was 0f 87 68 03 00 00`.

The velocity target went from empty to written. An F10 dump of the same screen minutes earlier, with
the patch off, recorded `fraction_unwritten` 1.0000 and both ranges exactly zero at 1024×576 and at
2048×1152. With the patch on, the render-resolution target recorded:

| Target | Unwritten | x range | y range |
| --- | --- | --- | --- |
| 1024×576 | 0.9995 | −0.0010 to 0.0005 | −0.0010 to 0.0003 |
| 2048×1152 | 1.0000 | 0 | 0 |

The decoded preview places those pixels as small aircraft glyphs clustered where the enemy symbols
and the `TRIGGER` marker sit on the map. They are the icons, and nothing else in the frame writes.

So the blend-mode rejection was the only thing stopping them, the cook does contain a usable
velocity permutation for those materials, and the engine supplies `PreviousLocalToWorld` for them
once a draw reaches the pass. The default-material substitution reasoning holds for sprite-like
icons.

A prediction to retain, because it was wrong: this screen was expected to show nothing, on the
grounds that its captures contained no velocity draws at all and a still map under a still camera
would have no movable primitives for the patch to admit. Both halves of that were unsound. The
captures were of the unpatched game, where the gate is exactly what removed those draws, so their
absence could not say what happens once it is cut; and the icons are movable primitives whether or
not the camera is.

Why the relief stays out, from AC7's own class layout. An SDK dump of the shipped build gives
`Nimbus.CampaignBriefingWidget` three fields: `BriefingMesh` (`UStaticMesh`) at `0x0518`,
`BriefingCloudMaterial` (`UMaterialInstanceConstant`) at `0x0520`, and `BriefingMeshActor`
(`AStaticMeshActor`) at `0x0528`. The map is a static mesh actor.

`AddVelocityStaticMesh` gates on `StaticMesh->PrimitiveSceneInfo->Proxy->IsMovable()` before it
reaches the blend mode check at all, so a static mesh actor never enters the velocity draw list
whatever is done to that check. The icons are separate movable objects, which is exactly why they
gained vectors and the relief did not. Not a permutation problem, and not something a wider gate
cut would reach.

It also means the relief cannot want object velocity: nothing about it moves, only the camera does.
What would cover it is camera motion reconstructed from depth, and separate translucency binds depth
read only, so those pixels carry the depth of whatever opaque surface is behind them. That is the
open question for its reconstruction quality, separate from getting it into the input at all.

Measured in flight, and it changes nothing there. Four captures during cannon fire with the patch
off recorded 0.9196 unwritten; four with it on recorded 0.9196. Both sit inside the 4.7 to 8.3 per
cent written that this document records for clear-sky flight, so the opaque path is untouched and
the tracers gained no vectors. Cannon fire covers a lot of pixels, so a change would not have been
subtle.

That is the shape of the whole result. The cut reaches translucent geometry that is movable and
falls into the default-material substitution, which wants a material that writes every pixel, is not
two-sided, and does not modify mesh position. Map icons qualify. Tracers, ribbons and particle
sprites are two-sided, as are the holographic sheets of the relief, so `SupportsVelocity` refuses
them for want of a permutation the cook never produced. No wider cut reaches those: the missing
thing is a compiled shader, not a branch.

What it does not do. Coverage is 0.05% of the frame in the briefing: the contour relief and the
dotted terrain grid write nothing.
The full-size target stays empty because the scene renders at half scale here. Magnitudes are around
a thousandth of a screen width, near a pixel at this resolution, from a still camera, so this says
the vectors exist and not that they are correct. Mission replay, where the camera and the symbols
both move, is what would test that.

The missing relief in the reconstruction is a separate problem and this does not address it: that
layer is absent from the backend's input because of which colour target is tapped, not because of
motion vectors. Before patching, establish whether the cooked shader library contains velocity permutations for those materials, since a gate cut that reaches an absent permutation gains nothing.

## Screen percentage per context: FGraphicsSettingsManager

Found 7 September 2026 in the Ghidra project while looking for what overwrites `r.ScreenPercentage`
on every screen change. AC7 keeps its own per-context table rather than one value:

| Item | Address | Evidence |
| --- | --- | --- |
| `FGraphicsSettingsManager` load of six per-context percentages | `FUN_1405f5260` | calls `FUN_1405f4590(this, x, index, flag)` six times with index 0..5, matching the SDK's `EGraphicsScreenPercentageSettings` (Gameplay 0, NonGameplay 1, VRNonGameplay 2, VRGameplay 3, VRAirShow 4, MPGameplay 5) |
| per-context reader | `FUN_1405f4590` | the callee above; takes the context index |
| FName globals | `DAT_14356eec0` .. `DAT_14356ef18` | built by static initialisers such as `FUN_14015fc30` (`"NonGameplayScreenPercentage"`); strings at `0x14265cc18` (Gameplay), `0x14265cc50` (NonGameplay), `0x14265ce30`, `0x14265d0a8`, `0x14265d320` |
| `r.ScreenPercentage` reference in the manager | `FUN_14015fb40` | data reference to the string at `0x14265cbd8` |

This is why a mission load puts the console value back to 100 and why the proxy has re-written it
on a timer. The [representation plan](../representation-plan.md) applies the render scale through
this table instead (expected-byte guarded, M3) and keeps the console write as the announced
fallback. Names are not yet assigned in the Ghidra project; they will be once the per-context
semantics are confirmed by a patch that takes.

## The engine's UI composition path is compiled in

Strings present in `Ace7Game.exe`: `r.HDR.UI.CompositeMode` at `0x142bc44c8`, `0x142bc5140`,
`0x142cce798`; `FCompositePS0` and `FCompositePS1` at `0x142bc7d58` and `0x142bc7e28`;
`FSlateElementVS` at `0x142bc5320`. Referencing functions: `FUN_140315560` (cvar registration),
`FUN_141343660`, `FUN_14164a840`.

`FUN_141343660` is `FSlateRHIRenderer::DrawWindow_RenderThread` (4.18.3 `SlateRHIRenderer.cpp:601`).
Its `bCompositeUI` is `DAT_143c783c3` (`GRHISupportsHDROutput`, read at `0x1413436ed`) and
`DAT_1436a2e96` (`GSupportsVolumeTextureRendering`) and a platform mask through `FUN_140df9ea0` and
the cvar at `DAT_143c8f250+4` and `FUN_141220f10` (`IsHDREnabled`, reading `DAT_143c85b00`). When
true it allocates a `PF_B8G8R8A8` UI target and an HDR-format source target at viewport size,
snapshots the back buffer, redirects Slate, and composites through `FUN_141351120`
(`FCompositePS<1>`, scRGB) or `FUN_1413515d0` (`FCompositePS<0>`, PQ), both through a 32³ LUT with
no SDR mode. Recorded because it is the in-engine blueprint for UI extraction; not used, because it
covers only Slate, the front-end interface on the screens that matter is widget quads, and the
composite would need intercepting.

## The interface converter

`UWidgetToTextureConverter_Setup` at `0x1404d5c10` stores a caller-supplied `FVector2D` into
`DrawSize` at `this+0x28` (the SDK's offset); `UWidgetToTextureConverter_SetupVirtualWindow` at
`0x1404d69a0` builds the `SVirtualWindow` from it. The front-end caller at `0x1406243e0` stores the
converter at `[rdi+0xd48]` (`AUIManagerActor::FrontWindowConverter`) and passes the constants at
`0x1425f1cac` (1920.0f) and `0x1425f1ce4` (1080.0f). The interface is rasterized at 1920x1080
regardless of the render scale.

## The vertex declarations that name an interface draw

Which draws are the interface has to be decided from what a draw is made of, and the vertex
declaration is the part of that which exists at creation, before anything has drawn. These are read
from 4.18.3 at `0a14a8d537a3`, not from a later engine: the reference checkout's default branch is
5.8.2, where `FSimpleElementVertex` carries an `FDFVector4` position and every offset after it
moves. A fingerprint taken from that branch matches nothing in this game, and would have looked
like bad luck rather than a mistake.

Unreal's D3D11 RHI writes the semantic name `"ATTRIBUTE"` for every element of every declaration in
the engine and puts the element index in the semantic index
(`D3D11VertexDeclaration.cpp:56, 61`), so a name distinguishes nothing. Format, input slot, byte
offset, semantic index and the per-instance flag are all there is, and they are enough.

| Declaration | Elements (slot, offset, format, semantic index) | Stride | Source |
| --- | --- | --- | --- |
| `FSlateVertexDeclaration` | 0/0 `R32G32B32A32_FLOAT` #0; 0/16 `R32G32_FLOAT` #1; 0/24 `R32G32_FLOAT` #2; 0/32 `B8G8R8A8_UNORM` #3; 0/36 `R16G16_UINT` #4 | 40 | `SlateShaders.cpp:52-58`, `RenderingCommon.h:140-155` |
| `FSlateInstancedVertexDeclaration` | the five above plus 1/0 `R32G32B32A32_FLOAT` #5, per instance | 40 + 16 | `SlateShaders.cpp:73-82` |
| `FSimpleElementVertexDeclaration` | 0/0 `R32G32B32A32_FLOAT` #0; 0/16 `R32G32_FLOAT` #1; 0/24 `R32G32B32A32_FLOAT` #2; 0/40 `B8G8R8A8_UNORM` #3 | 44 | `BatchedElements.h:33-70` |

`VET_Color` is `B8G8R8A8_UNORM` and `VET_UShort2` is `R16G16_UINT` in this RHI
(`D3D11VertexDeclaration.cpp:42, 49`), which is where those two formats come from.

The converter's own render targets are recognised separately, by the shape
`FWidgetRenderer::CreateTargetFor` asks for: PF_B8G8R8A8 with a transparent clear, one mip, no
array, no multisampling, bound as both render target and shader resource
(`WidgetRenderer.cpp:68-117`), at the `DrawSize` recorded above. A shape match is a candidate only.
Several things in an AC7 frame are 1920x1080, and what settles it is a Slate-layout draw writing
into one, which the frame tap sees and the classifier requires.

Implemented in `games/ac7/src/ui_rules.cpp` as `rsf_ac7_ui_classify_layout` and
`rsf_ac7_ui_is_widget_target`, both pure functions with no device, tested in
`tests/ac7_ui_rules.cpp` including a case that asserts the UE5 layout is refused. Nothing runtime
yet consumes them: this is source-verified identification, not a measured hook.

## Implementation follow-up

The later [capture work](ac7-frame-capture.md) established live resources, view data, jitter, and render-scale control. The research proxy now evaluates DLSS. The [representation plan](../representation-plan.md) carries the rest: UI extraction, the presentation bridge, and the vendor contracts. The source inspection itself remains distinct from those later runtime results.
