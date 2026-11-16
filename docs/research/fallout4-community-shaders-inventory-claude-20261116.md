# Fallout 4 Community Shaders source inventory (Claude, independent)

Source review only. No build, game, GPU, process, REA or executable analysis was performed, and
nothing here is runtime evidence for Community Shaders (CS) or for RSF. Root runs a separate
inventory and the executable verification; this note is meant to be reconciled with it. No CS code
was copied into RSF.

## Question and method

The question is what CS actually does to make Upscaling and Frame Generation work on Fallout 4, and
which of those mechanisms the current RSF Fallout 4 adapter (commit `c32d8f7` plus the uncommitted
tree at the time of reading) omits or contradicts. Native-only (no ENB, no F4SE) is the target
phase.

Method: tracked-file manifest with line counts, full reads of the core temporal resolve and hook
files, targeted reads and grep for every other subsystem, then a mechanism-by-mechanism comparison
against `games/fallout4/src`. Line anchors are `path:line` at the pinned revisions.

Limits: this is a full inventory by structure, not a deep review of everything. Parts of the temporal
internals (most of `TemporalPipeline.cpp`, `DX12SwapChain.cpp` outside the present path, `Streamline.cpp`
outside constants and tagging, latency backends, ReShade, RCAS) are mapped by function list only, and
the non-temporal features are inventoried by files, hooks and shaders without correctness review. See
the coverage table and "Unknowns and skipped areas".

## Revisions

| Repository | Revision | Commit date | Files |
| --- | --- | --- | --- |
| `references/fallout4-community-shaders` | `e0e711594291c1a8e447ac758099492514810fbb` (HEAD = origin/main) | 2026-11-14 | 644 tracked entries (640 files + 4 gitlinks) |
| `extern/CommonLibF4` | `7f8deca16dc7dd245c9c52151ec9ef27edd9ad60` | 2026-11-12 | 1911 |
| `extern/community-shaders-shared` | `6e67083a54090d10806484549d07f75b7fbc8dc2` | 2026-11-09 | 1169 |
| `extern/Streamline` (fork tag `cs-streamline-v2.14.16-68bad8f`) | `68bad8fe3ad61d25b1f94a17939c34ab3ce17d88` | 2026-11-09 | 477 |
| `extern/bethesda-devbench` | `1d804c5241019ecc456a4c7d385109c90f56ce1f` | 2026-11-08 | 295 |

All four submodules were initialized by root and match their gitlinks. The parent reports
`extern/bethesda-devbench` as modified-content (` M`), while the submodule's own status is clean.
I did not change any checkout.

Manifest (ignored): `.local/fallout4-research/community-shaders-inventory-claude-20261116/manifest-claude.tsv`
(path, extension, line count for all 644 entries).

## File accounting and coverage

| Bucket | Files | Lines | Coverage in this review |
| --- | --- | --- | --- |
| `src/Render` | 135 | 31532 | Temporal path read in full (`TemporalRenderer*.{h,cpp}`, `TemporalResolve`, `TemporalRenderHooks`, `TemporalFrameGenerationInputs`, `Engine.h`, `FrameBuffer`, `FrameBoundary`); `TemporalPipeline.cpp` 60-450, 891-1287; `DX12SwapChain.cpp` 1855-2114 plus function map; other files by function map and header comments |
| `src/other` (Compat, Host, Menu, Settings, Telemetry, Utils, World, Capture, root) | 116 | 16700 | `XSEPlugin.cpp`, `Compat/*` read in full; rest by header comment and grep |
| `features/Upscaling`, `FrameGeneration`, `MotionVectorFixes` | 38 | 8187 | All shaders, anchors, `Upscaling.cpp`, `DynamicResolution.cpp`, `MotionVectorFixes.cpp`, settings read in full; `Streamline.cpp` 660-780, 1060-1120, 1290-1400 |
| `features/*` other (25 features) | 156 | 23711 | File list, settings/shader-define headers by grep; not read line by line |
| `package/Shaders` | 79 | 24130 | All listed; `ImageSpaceVS.hlsli`, `ISGammaResize`, `ISHDR` resize section, DR-related lines of `ISTemporalAA`, `ISSSLRRaytracing`, `ISMotionBlur`, `ISDepthOfField` read |
| `package/Textures`, `Meshes`, `F4SE` | 7 | (binary) | Excluded: assets |
| `tests` | 18 | 10313 | Test targets and temporal-related assertions grepped; TSV data not read |
| build, CI, scripts | 35 | 4464 | `xmake.lua`, `xmake/shared.lua`, `.gitmodules`, `pr.yml` steps read; `scripts/rdoc/*` not read |
| `extern` vendored headers (ENB, ReShade, RenderDoc, AntiLag2) | 18 | 9350 | Excluded except ENB API use in `Compat.cpp` |
| `docs` | 31 | 686 | `deviations/Upscaling.md`, `FrameGeneration.md`, `MotionVectorFixes.md`, README requirement lines read |
| root meta | 11 | 1318 | README requirement lines only |

Explicit exclusions: submodule internals beyond what CS imports (`xmake/shared.lua` list), Streamline
plugin source, CommonLibF4 beyond the referenced `BSGraphics.h` and `ImageSpaceManager.h` layout,
DevBench, Nexus text, license text, binary assets, test data tables, the RenderDoc Python tooling.

## Lifecycle and global contracts

- Entry: F4SE plugin. `F4SEPlugin_Version` declares Address Library NG/AE, layout dependent, latest
  runtime (`src/XSEPlugin.cpp:34-50`). `F4SEPlugin_Load` builds the settings registry, parses shader
  ownership, sets OG/NG/AE runtime, runs compatibility detection, installs shader subclass hooks and
  the shader-compile gate, registers the swap-chain creation router, prepares and activates all
  features, installs the menu, camera capture, telemetry, profiling, frame annotations, the
  `Main::Swap` frame boundary and the device-creation hook (`XSEPlugin.cpp:70-136`).
- `kPostPostLoad` freezes the temporal request and registers the DearModdingUI host;
  `kGameDataReady` runs feature data-load and `TemporalPipeline::OnDataLoaded` (`XSEPlugin.cpp:52-68`).
- Requirements claimed in README:33-35, 123-130: AE 1.11.240, NG 1.10.984 or OG 1.10.163; F4SE and
  Address Library required; DX12-capable GPU; DLSS needs RTX, FSR4 needs supported Radeon; DLSS/FSR
  and FG need borderless windowed. The borderless claim is enforced in code: replacement creation
  refuses exclusive fullscreen (`src/Render/TemporalPipeline.cpp:1095-1104`). Device creation falls
  back to feature level 11_0 for boot, but the proxy needs 11_1 (`src/Render/SwapChainHook.cpp:25-28`).
- ENB: `Compat/Rules.cpp:38` disables every feature when `ENB_API::RequestENBAPI()` returns non-null
  (`Compat.cpp:73`). CS upscaling and FG are never active together with ENB. Standalone Upscaling,
  AAAFrameGeneration and Isotope disable HDR/Upscaling/FG/RenderDoc (`Rules.cpp:18,26-35`); RenoDX
  disables HDR only. Plugin identity is read from `F4SEPlugin_Version` without running DllMain
  (`Compat/PluginScan.h`).
- CS is an F4SE + Address Library + CommonLibF4 design. Every hook is an ID-plus-offset call-site
  anchor (`src/Render/EngineCallSite.h`) or an ID detour. None of its mechanisms apply unchanged to
  RSF's native-only adapter, which must use guarded AE RVAs.

## Subsystem inventory

| Subsystem | Files | What it does (code) | Anchor |
| --- | --- | --- | --- |
| Feature framework | `src/Feature.*`, `FeatureState.h`, `FeatureCategories.h` | Register/Prepare/Activate, live toggle, quarantine on fault, restart-required settings | `Feature.cpp` |
| Settings | `src/Settings/*` (12) | TOML schema registry, live binding, presets, delta save, JSON for host | `SettingsSchema.h`, `FeatureConfig.cpp` |
| Menu/host UI | `src/Menu/*` (13), `src/Host/*` (7) | DearModdingUI client pages, shader-compile overlay, profiler panel | `Host/HostClient.cpp` |
| Compatibility | `src/Compat/*` (6) | Plugin/ENB/RenoDX detection rules above | `Compat/Rules.cpp` |
| Device and presentation | `SwapChainHook`, `D3D11Bootstrap`, `DX12SwapChain`, `DXGISwapChainProxy`, `DXGISwapChainFacadeContract` | Replace engine D3D11 device+swap-chain creation with D3D11 device plus a D3D11-facing proxy over a private D3D12 swap chain; shared textures and shared fences | `TemporalPipeline.cpp:1067-1279`; `DX12SwapChain.cpp:48-130,714-753` |
| Temporal pipeline | `TemporalPipeline*`, `TemporalProvider.h`, `TemporalPresentation.h`, `TemporalRenderSettings.h`, `TemporalRenderSizing.h` | Requested/effective topology, provider admission, reset epochs, FG transactions, latency timeline, status | `TemporalPipeline.cpp:227-366` |
| Temporal renderer | `TemporalRenderer*`, `TemporalResolve`, `TemporalRenderHooks`, `TemporalRenderResources`, `TemporalFrameGenerationInputs`, `TemporalDiagnostics` | Engine hooks, DR publication, jitter, SR resolve, post-SR chain inputs, FG capture, debug views | detailed below |
| Engine accessors | `Engine.h`, `RendererContext`, `EngineCallSite`, `RenderTargetFormats` | Logical-to-platform target resolution at every use, DR state, imagespace triangle swap, unscaled scope, target swap | `Engine.h:151-265,342-504` |
| Camera record | `FrameBuffer.{h,cpp}`, `CanonicalDepth` | World camera from `State::cameraDataCache` (key +0x238, jitter flag +0x240) captured around DeferredPrePass; canonical depth CS for ReShade | `FrameBuffer.cpp:22-84` |
| Frame boundary/latency | `FrameBoundary`, `LatencyRouter`, `FrameGenerationCpuTiming.h` | Pre/post `Main::Swap` callbacks; Reflex/PCL or Anti-Lag 2 routing | `FrameBoundary.cpp:20-66` |
| Shader injection | `ShaderInjection*`, `ShaderSubclassHooks`, `ShaderFamilyDescriptor`, `ShaderVariantCompilation`, `ShaderDefineProvider`, `ShaderCompileGate`, `ShaderObservers`, `NativeShaderFamily`, `PrepassTechnique`, `SubstrateSlots` | Replace stock shaders by family/technique with CS-compiled variants; feature define contributions; compile batching and gate | `ShaderInjection.h`, `ShaderFamilyDescriptor.cpp` |
| Shared constant data | `SharedData*`, `SharedFeatureData.h` | b4-b7 substrate, FrameBuffer DR params, scoped MipBias in b5, history-reset latch | `SharedData.cpp:431-500` |
| Render hook registry | `RenderHooks.{h,cpp}`, `HookRegistry.h`, `LightingRenderPasses`, `ForwardDraw` | Pre/post DeferredPrePass, DeferredComposite, Render_UI, pre-interface, lights, shadows, Z-prepass PS/VS selectors, command-buffer draw redirection | `RenderHooks.cpp:600-1150` |
| World renderer extras | `OccluderBatch*`, `OccluderGather`, `OccluderPasses`, `PrecipitationOcclusion`, `SkyReflectionCube`, `ShadowCascades`, `SunShadowLights`, `LocalLights`, `TerrainWater`, `ObjectLOD`, `OverlayShape`, `PrepassExtraTargets` | Feature-owned passes and data | header comments |
| HDR/UI separation | `HdrCompose`, `InterfaceLayer`, `features/HDRDisplay` | HDR10 container, UI redirected to separate proxy layer via blend-state translation | `InterfaceLayer.cpp:98-206` |
| Diagnostics | `Annotation`, `FrameAnnotations`, `FrameProfiler`, `RgbaPreview`, `Telemetry`, `Capture` | Markers, GPU timers, debug snapshots, telemetry fields | |
| Utilities | `src/Utils/*` (43) incl. `ShaderCache/*` (16) | Detours, hook ownership, FXC compile + disk cache with dependency trace and revalidation | `Utils/ShaderCache/ShaderCache.cpp` |
| World/weather | `src/World/*` (21) | Weather lock, sky, sun/moon placement, game-time keepalive in menus | `World/WeatherControl.h` |

## Feature inventory

All features start inactive and are opt-in (README:88). Shader column lists feature-owned shader
sources; most features also contribute defines to replaced stock shaders.

| Feature | Files | Main mechanism | Feature shaders |
| --- | --- | --- | --- |
| Upscaling | 29 | Temporal SR (TAA, FSR3, DLSS, FSR4 via Streamline D3D12), DR proxies, mip bias, Reflex/Anti-Lag 2, RCAS, ReShade bridge, Agility bootstrap | 8 (below) + shared RCAS |
| FrameGeneration | 7 | DLSS-G / FSR3-G / FSR4-G presentation providers on the D3D12 proxy | `CopyDepthForFrameGenerationCS` |
| MotionVectorFixes | 2 | Previous-transform corrections | none |
| HDRDisplay | 9 | HDR10 container, UI composite | `InterfaceCompositeCS` + shared HDROutput/UIBrightness |
| LightLimitFix | 24 | Clustered lights, local shadows, light sidecar | `LocalShadowCopyCS` + shared cluster CS |
| DynamicCubemaps, IBL | 8, 4 | Dynamic reflection capture, SH/IBL consumers | `PrepareCaptureCS`, composite includes |
| ScreenSpaceGI, ScreenSpaceShadows, VolumetricShadows, Skylighting, TerrainShadows, CloudShadows | 6, 6, 4, 5, 5, 5 | Screen/space lighting features | prepare/debug CS + shared |
| SubsurfaceScattering, VanillaPBR, ExtendedMaterials | 10, 10, 4 | Material features | `PrepareCS`, `BlurMatchCS`, `BlurDetailCS` |
| TerrainBlending, TerrainVariation, LODBlending, UnifiedWater, WaterEffects, SkySync | 3, 4, 4, 6, 5, 3 | Terrain/water/sky features | `WaterEffects/Debug`, consumers |
| PerformanceOverlay, RenderDoc, Screenshot, RemoteControl, CSEditor | 3, 5, 5, 12, 6 | Tooling | none |

`package/Shaders` holds 79 files: 28 replaced imagespace shaders (`ISBlur`, `ISBokehBlur`, `ISCopy`,
`ISDepthOfField`, `ISDoubleVision`, `ISFXAA`, `ISFullScreenColor`, `ISGamma`, `ISGammaResize`,
`ISHDR`, `ISLocalMap`, `ISModMenu`, `ISModMenuGlow`, `ISMotionBlur`, `ISPipboyScreen`,
`ISRadialBlur`, `ISRefraction`, `ISSAO*` x4, `ISSSLR*` x2, `ISSunbeams`, `ISTemporalAA`,
`ISVatsTarget`, `ISWaterDisplacement`), replaced world shaders (`DFComposite`, `DFLight`,
`DFPrepass`, `DFTiledLighting`, `DistantTree`, `Effect`, `Sky`, `Water`, `ZPrePass`), and `Common/`
plus `FO4/` includes and consumers. Stock replacement is identity-checked against
`tests/data/stock-shader-identity-{AE,OG}.tsv`.

## Build, test and package contracts

- xmake, C++latest because MSVC C++23 breaks pinned Streamline headers (`xmake.lua:47-48`); vcpkg
  detours, directx-headers/math/tex/tk, magic-enum, toml++, optional tracy and NvPerf.
- Shared sources and shaders come from `community-shaders-shared` through an explicit, asserted list
  (`xmake/shared.lua:1-121`), including `Common/FrameBuffer.hlsli` and the RCAS shader.
- The package installs Streamline and vendor DLLs from `features/Upscaling/Shaders/Upscaling/Streamline/`
  including `D3D12/D3D12Core.dll` (`xmake.lua:131-152,338`), fetched by `scripts/fetch-sdks.ps1`.
- CI (`.github/workflows/pr.yml`): lint (bash, PowerShell parse, clang-format), Windows releasedbg
  build, `xmake test -g unit`, `xmake test -g shaders`, package.
- Tests: `FeatureConfigTests` (schemas, opt-in defaults), `AgilityBootstrapTests`,
  `ShaderABIChecks` (incl. `FrameDataCB::DynamicResolutionParams1/2`), `RuntimeShaderTests` (compiles
  every Upscaling/FG shader variant, `tests/RuntimeShaderTests.cpp:72,103-112`), `ShaderCacheTests`,
  `StockShaderIdentityTests`. No test exercises temporal pipeline behavior, DR, SR or FG.

## Upscaling: how it actually works

### Setup, admission and resources

- Device creation is replaced only if SR, FG or HDR was requested at freeze and the chain is
  windowed. The real D3D11 device is created first, then `DX12SwapChain::Initialize` builds a private
  D3D12 device (Agility bootstrap), queues, shared fence pair and the D3D11-facing proxy chain; any
  failure keeps native creation (`TemporalPipeline.cpp:1067-1279`).
- SR runs on D3D12. `EvaluateD3D12SuperResolution` (`DX12SwapChain.cpp:1412`) does D3D11 `Signal`,
  D3D12 queue `Wait`, record, D3D12 `Signal`, D3D11 `Wait` (`:1457-1458,:1524-1526`): GPU waits only.
  Inputs are D3D11 textures with D3D12 aliases (`resource12`).
- Render size comes from the provider's optimal-size query for quality 0-4
  (`TemporalRenderer.cpp:113-153`), committed only after resource and spatial-fallback preflight
  (`:179-197`).
- Resources are created after `BSShaderRenderTargets::Create`, deferred while `WindowSizeChanged` is
  running (RT0 not yet repopulated), and invalidated on `ResetWindow` (`TemporalRenderHooks.cpp:92-116,449-475`).
- Scene-colour contract: kMain must be full-size `R11G11B10_FLOAT`, one mip, no MSAA
  (`TemporalRendererInternals.h:242-251`), and the RTV must reference the same texture at screen size
  (`TemporalResolve.cpp:12-42`).

### Per-frame order (world frame)

| Step | Engine site (CS anchor) | CS action | Units/notes |
| --- | --- | --- | --- |
| 1 | `DrawWorld::Begin+0x26 -> SetUseDynamicResolutionViewportAsDefaultViewport` | Force argument `true` while driving a vendor SR (`TemporalRenderHooks.cpp:129-134`) | RTM +0xFE4 |
| 2 | `DrawWorld::Begin+0x38C -> State::UpdateTemporalData` | Before: native fallback, quarantine finalize, `SetTAAState(enabled)` for any temporal method. After: `ConfigureUpscaling` (size query, preflight, Halton jitter, mip bias) (`:155-184`, `TemporalRenderer.cpp:106-224`) | TAA state +0xA8 |
| 3 | `Render_PreUI+0x29F -> UpdateDynamicResolution` | After native call: write `State::offsetX/Y = -2*jx*ratio/renderW, 2*jy*ratio/renderH`, build proxies, `SetDynamicResolutionState(ratio, ratio, active)` (`TemporalRenderer.cpp:226-260`) | jitter in render pixels, Halton(2,3)-0.5, phases `8*(out/in)^2` (`Internals.h:125-151`). Comment: camera builder divides offsets by the ratio |
| 4 | `Render_PreUI+0x2E3 -> DeferredPrePass`, `+0x3A6 -> Forward` | Scope `MipBias = log2(render/output) - (DLSS ? 1 : 0)` into b5 for owned shaders only (`TemporalRenderHooks.cpp:407-435`, `Internals.h:153-159`) | Needs replaced shaders |
| 5 | `DeferredComposite+0x3EE -> SetCurrentRenderTarget` (SSLR ray start) | Proxy pass with **unit triangle**: G-buffer normal, environment, kMainCopy (`:327-345`) | Stock SSLR assumes TexCoord [0,1] (`UpscalingAnchors.h:50`) |
| 6 | `+0x60E -> AcquireRenderTarget`, `+0x6C3 -> ReleaseRenderTarget` | Proxy pooled kSSRBlurred0 per acquire; on release copy kSSR, kSSRBlurred1 back (`:347-369`) | Water reads them in region layout |
| 7 | `DeferredComposite+0x915 -> RenderPassImmediately` | Proxy pass over 11 G-buffer/light targets, copy kMain back (`:298-325`) | BSDFComposite samples DR G-buffers |
| 8 | `RenderLensFlare` (detour) | Swap main depth SRV for render-size depth copy when ratio != 1 (`:371-396`) | Visibility reads main depth |
| 9 | Forward `+0x53D -> RenderAlphaGeometry` (OG ForwardAlphaImpl +0x253) | FG only: copy pre-alpha kMain to kMainCopy, then capture depth/motion with colour-difference conditioning (`TemporalFrameGenerationInputs.cpp:10-197`) | |
| 10 | `Interface3D::RenderPrepassesAndMenus`, `SetCompanionLocalMapRenderFunc` | `UnscaledRenderScope` for Pip-Boy target and local map (`TemporalRenderHooks.cpp:276-296`, `Engine.h:216-265`) | These run inside Render_UI before the resolve |
| 11 | Pre-imagespace (wraps every Render_UI call site, `RenderHooks.cpp:657-667`) | Request SR/FG reset on history-reset frames; record full-effects path (`TemporalRenderHooks.cpp:214-227`) | |
| 12 | `Render_UI+0x83 -> ImageSpaceManager::RenderEffectRange` | `ResolveSceneBeforeImagespace` (below), then call native; restore VATS swap (`:200-212`, `TemporalResolve.cpp:639-689`) | SR seam on linear kMain |
| 13 | `Render_UI+0xC5 -> SetUseDynamicResolutionViewportAsDefaultViewport`, after native call | `FinishPreUiSeam`: zero offsets, ReShade, HUD-less capture from RT0, `SetTAAState(method == TAA)` (`TemporalResolve.cpp:691-710`, `RenderHooks.cpp:684-694`) | Before interface drawing |
| 14 | Post-imagespace | Skip/miss checks, `SetTAAState(false)`, **offsets 0 and DR (1,1,false)**, restore unit-triangle and VATS swaps (`TemporalRenderHooks.cpp:229-274`) | Ratio stays 1 until next step 3 |
| 15 | `ImageSpaceEffectTemporalAA` vfunc 8 (IsActive) | Return false while a vendor SR published to kMain (`:118-127`) | |
| 16 | `ImageSpaceManager::InitEffects` | Set FXAA effect inactive (`:437-447`) | |
| 17 | `LoadingMenu::Render+0x275 -> UpdateTemporalData` | DR (1,1,false) after native call (`:398-405`) | |

### The resolve (`TemporalResolve.cpp`)

1. Copy kMain (full allocation) into `upscalingTexture` (`:307-312`).
2. Encode at render size (`EncodeTexturesCS`): t0 TAA mask is bound null and the G-buffer normal's z
   feeds transparency, so reactive and transparency masks are zero (deviation doc "Pending");
   DLSS gets 5x5 closest-depth longest-motion dilation blended by a near factor, FSR gets raw motion;
   typeless depth is copied into typed R32_FLOAT (`:75-123`, shader).
3. Provider request: input extent `(0,0,renderW,renderH)` of full-size textures, output full size,
   jitter in render pixels, `reset` on pending reset or engine frame discontinuity (`:125-133`),
   colour declared linear pre-tonemap with automatic exposure, camera from the captured world record
   (`:140-209`). Streamline constants: `mvecScale (1,1)`, `jitterOffset = -jitter`, depth not
   inverted, camera motion included, motion not jittered, projection rebuilt from FOV, clipToPrevClip
   from unjittered current/previous view-projection (`Streamline.cpp:685-761`); tags at
   `Streamline.cpp:1084-1099`.
4. Publish provider output with `CopyResource` into full kMain (`Internals.h:36-51`). On failure or
   skip: spatial bilinear fallback from the render rect (`SpatialFallbackPS`), and after 30
   consecutive skips (`TemporalPipelineState.h:646`) or any failure, native TAA from the next frame
   until restart (`TemporalResolve.cpp:324-397`, `TemporalRenderHooks.cpp:160-165`).
5. Post-SR chain inputs at full extent (`:475-637`): main depth via SV_Depth plus refraction normals
   sampled at `uv - jitter*texel` (`DepthRefractionUpscalePS`); motion, Pip-Boy mask (logical
   kGBufferAmbient alpha) and VATS mask by **nearest** sampling, values unscaled
   (`ChainInputsUpscalePS`). VATS writes into a separate full-size copy that is pointer-swapped into
   logical kMenuBackground only for the chain, because that logical also serves as the menu
   background (`:607-614`, `:682-685`, `Engine.h:383-416`).
6. Then offsets 0, DR (1,1,false), and the dynamic screen triangle is replaced by the unit one
   (`TemporalResolve.cpp:672-686`, `Engine.h:168-194`).

### Dynamic-resolution proxies (`DynamicResolution.cpp`)

During composite and SSLR passes CS replaces 17 logical targets (`:23-41`) with render-size
textures, copies the render rect in (`CopySubresourceRegion`, `:304-336`), scales all 100
`renderTargetData` width/height entries (`:384-391`), repoints already-bound PS SRVs (`:396-412`),
clears the viewport-as-default flag, sets DR to (1,1,false) for the pass, optionally uses the unit
triangle, overrides main depth with a render-size R32 copy and regenerates `kSAOCameraZ` linear depth
(`:587-649`), then copies back and restores everything (`:418-461,534-546`). Abort and release unwind
partial overrides (`:651-695`).

### Imagespace UV precision

The engine bakes `2*ratio` into an fp16 dynamic-triangle UV buffer. CS injects a VS that restores
the exact scale from its own constant within one fp16 ulp (`package/Shaders/FO4/ImageSpaceVS.hlsli`,
`features/Upscaling/src/ShaderDefines.h`). This applies only to CS-compiled imagespace shaders.

## Frame Generation: how it actually works

- Per-frame decision is latched at the SR seam: FG proxy active, capture resources ready, and not
  paused / MainMenu / LoadingMenu unless `frame_generation_allow_in_menus`
  (`TemporalRenderer.cpp:48-79`). The latch expires after one frame so loading screens drop FG.
- Inputs: depth and motion copied at the SR seam (before SR) from native render-rect data into
  output-size shared R32_FLOAT / R16G16_FLOAT textures; outside the render rect depth=1, motion=0
  (`CopyDepthForFrameGenerationCS`). They are tagged with render extent `(0,0,renderW,renderH)`,
  HUD-less and UI with output extent (`Streamline.cpp:1313-1331`).
- First-person alpha conditioning: where pre-alpha and post-alpha kMain differ, motion fades to 0 and
  depth toward `min(depth, 0.1)` (shader, `TemporalFrameGenerationInputs.cpp:97-197`). The deviation
  doc labels this a heuristic for BLEND motion gaps.
- HUD-less: copy of RT0 (kFrameBuffer, must be R8G8B8A8_UNORM, output size) after imagespace and
  before interface drawing (`TemporalFrameGenerationInputs.cpp:364-447`). SDR mode is
  `kHudlessAndFinal`; with HDR the UI is a separate premultiplied layer from `InterfaceLayer`.
- Present (`DX12SwapChain.cpp:1855-2114`): D3D11 signals, D3D12 waits, copy proxy back buffer to the
  real back buffer, validate options, `PrepareFrame` with depth/motion/HUD-less/final colour and
  frozen constants for that slot, execute, signal, D3D11 waits on the GPU, then present through the
  Streamline-hooked chain and collect generated/presented counts. Two frame slots.
- Constants are frozen at capture per slot (`TemporalPipeline.cpp:1619-1653`); `queryFrameState`
  accepts them only when the real-frame id matches (`:1151-1197`). FG reset is pending until a
  prepared Present consumes it; SR reset follows it while FG is on (deviation doc).
- Latency (`TemporalPipeline.cpp:129-148,416-419,891-1020`): sleep and live-config apply before
  `Main::Run -> WindowsMessageLoop`; simulation start and input sample after it returns; simulation
  end and render-submit start before `Main::Swap`; render-submit end and present start/end around the
  proxy Present. Presents off the main-loop thread (loading screens) pause pacing. Reflex is forced on
  under DLSS-G (`Upscaling.cpp:256-266`).
- Resize goes through the proxy `ResizeBuffersImpl` with provider quiesce, drain and restore
  (`DX12SwapChain.cpp:2581-2766`).

## Camera, depth and motion corrections

- Camera record: first `cameraDataCache` entry whose reference camera is `Main::WorldRootCamera` and
  `useJitter` is set; view, projection, unjittered current/previous view-projection, pos-adjust
  current/previous (`FrameBuffer.cpp:49-75`). Captured at pre/post DeferredPrePass.
- First-person projection inverse is read from the renderer shadow state +0x8A0
  (`Engine.h:282-296`) for canonical depth only. SDK depth stays the native partitioned depth;
  the deviation doc lists world-projection SDK depth as "Pending".
- MotionVectorFixes (`MotionVectorFixes.cpp:95-183`): cache player 3D world transforms before
  `Main::OnIdle_UpdatePlayer` and restore them as `previousWorld` afterwards; reset `previousWorld`
  after `TESObjectREFR::SetSequencePosition -> NiAVObject::Update`; before lighting pass lists, set
  `previousWorld = world` for frozen/menu frames and LOD/landscape geometry, skipped during
  LoadingMenu. BLEND motion gaps are not repaired.

## Claims versus code

| Claim | Status |
| --- | --- |
| DLSS/FSR/FG need borderless windowed (README:129) | Code-enforced at creation |
| Upscaling deviation: "offscreen cameras (precipitation, facegen) lock dynamic resolution" | No code anchor at this revision. `UnscaledRenderScope` is used only for Interface3D, companion local map and `SkyReflectionCube.cpp:748`; no DR write in `PrecipitationOcclusion.cpp` |
| Reactive/transparency masks | Zero in code (null t0), matching "Pending" |
| World-projection SDK depth | Not implemented ("Pending"); partitioned depth sent |
| ENB unsupported (README:123) | Code disables all features |
| FSR4 requires Radeon | Capability fields only (`Upscaling.cpp:389-427`); gating is in Streamline/provider code not read here |

## Comparison with RSF Fallout 4 (native-only)

RSF anchors: `games/fallout4/src/render_hooks.cpp` hook table `:555-563` (UpdateDynamicResolution
1839ED0, DeferredPrePass 21F38B0, native present 1818550, WorldBegin 21F0AC0, viewport setter
1839EC0, Render_UI 21FA200, RenderEffectRange 2185420, TAA IsActive 2214FC0), `resolution.cpp`,
`render_inputs.cpp`, `cpu_events.cpp:1024-1034`.

### Matching mechanisms

- SR seam on the same call (`Render_UI -> RenderEffectRange`, RSF `render_hooks.cpp:474-489`) and
  HUD-less at the return of the Render_UI viewport-setter call (RSF caller 21FA2CA, `:341-344`).
- DR ratio written after native `UpdateDynamicResolution` returns at Render_PreUI (RSF
  `:376-406`, `resolution.cpp:154-188`), same fields as CommonLibF4 (`BSGraphics.h:1525-1529`:
  +0xFB8/+0xFBC/+0xFE5/+0xFE4).
- Native TAA excluded via IsActive while SR published (RSF `:497-503`).
- Post-SR full-extent depth, motion, Pip-Boy (30), VATS (15) and refraction normals (14) through the
  generic auxiliary service (RSF `:183-218`).
- Same camera cache selection rule (RSF `render_inputs.cpp:262-266`).
- Different but equivalent triangle handling: RSF clears +0xFE4, and the verified AE selector at
  21856F5 (`cmp byte [rip+0xDC9038]` resolves to 2F4E734 = RTM 2F4D750 + 0xFE4) picks the unit
  triangle (+0x28) instead of the dynamic one (+0x38). CS keeps +0xFE4 true and swaps the pointer.
  RSF's version holds only while every native write to +0xFE4 is re-cleared (RSF `:336`,
  `resolution.cpp:245-252`).

### Mechanisms RSF omits

| # | CS mechanism | RSF state | Expected native effect at ratio < 1 (to verify) |
| --- | --- | --- | --- |
| 1 | DeferredComposite and SSLR proxy passes with unit triangle and pooled BlurH proxy | None | Stock SSLR assumes [0,1] TexCoord: sliding/missing reflections; composite G-buffer sampling depends on unverified stock DR handling |
| 2 | Lens-flare depth override | None | Flare visibility tests wrong depth region |
| 3 | Unscaled scope for `Interface3D::RenderPrepassesAndMenus` (Pip-Boy target) and companion local map | None; RSF's reduced ratio is live inside Render_UI before the seam | Pip-Boy screen or local map rendered into a sub-rect |
| 4 | After Render_UI: offsets 0 and DR (1,1,false) until next world frame | RSF restores the reduced ratio, activation, +0xFE4 and jitter at Render_UI exit (`resolution.cpp:253-273`) and native ratio only at frame close (`cpu_events.cpp:540`, `resolution.cpp:274-286`) | Any draw between Render_UI return and Present sees reduced DR state. Contradictory assumption about who owns that interval |
| 5 | VATS mask in a separate full copy pointer-swapped into logical 15 for the chain only | RSF expands logical 15 in place with `RSF_D3D11_AUXILIARY_RESTORE_CONTENTS` (`render_hooks.cpp:210`) | CS states logical 15 doubles as menu background; in-place rewrite and restore touches that target |
| 6 | Own Halton jitter with `8*(out/in)^2` phases | RSF measures native TAA jitter from the projection (`camera.cpp:101-104`) | Native phase count unknown; possible short sequence for DLSS at Quality and below |
| 7 | Mip bias in DeferredPrePass/Forward | None | Needs a native sampler-table route; CS's route needs replaced shaders |
| 8 | fp16 dynamic-triangle UV restore | None | Sub-texel drift in pre-SR imagespace passes; needs shader replacement in CS form |
| 9 | MotionVectorFixes previous-transform corrections | None (CPU hooks do not include OnIdle_UpdatePlayer, SetSequencePosition or pass-list hooks) | Stale previous transforms for player, animated objects, frozen/menu and LOD/landscape motion |
| 10 | FG first-person alpha conditioning | None | Depth/motion on transparent first-person pixels unconditioned for FG |
| 11 | FXAA disable at `InitEffects` | None | Only matters if native FXAA runs with TAA enabled; unverified |
| 12 | LoadingMenu DR reset, WindowSizeChanged/ResetWindow/Create resource rebuild hooks | RSF refuses loading screens and checks owner identity per frame | Adequate if host-side resize/retire is correct; not a known gap |
| 13 | `renderTargetData` size scaling during proxy passes | None | Only needed with proxies |

Differences in the other direction, as code facts only: RSF builds same-source depth-domain
mappings (`depth_domains.cpp`) where CS sends native partitioned depth and lists world-projection
SDK depth as "Pending"; RSF checks per-frame owner, view, window and lease identity and refuses on
mismatch, where CS falls back to a spatial resolve and later native TAA. Neither side's resulting
image quality is established by this review. CS's partitioned SDK depth and zero masks are recorded
as its current state, not as a target for RSF.

### The Quality 1 native image

`user-native-quality1-failure.png` (2559x1599 capture of a 2560x1600 output): the upper-left
~1707x1067 region holds the **whole view at render size**, sharp, and the rest of the frame holds the
same view at full size. This is consistent with render-rect contents holding a reduced, pre-SR-like
image reaching the presented frame 1:1 alongside a full-size image. A screenshot shows only the final
composite; it does not identify the first writer that corrupted it or the order of writes. The source
review cannot pick the cause. Candidates, ranked only by how directly CS source addresses them, all
unproven, each with a rejection test:

1. **In-place auxiliary restore (row 5).** If any restored auxiliary or its restore path aliases a
   scene-colour target used by the chain (logical 15 is shared with the menu background in CS's own
   comment), restoring "reduced" contents at Effects return writes the pre-SR image back. Reject if
   per-frame logs of logical 14/15/30 platform slots and texture pointers never equal kMain, kMainCopy,
   RT0 or any target the chain writes, and disabling the VATS auxiliary does not change the image.
2. **Reduced DR state after Render_UI (row 4).** A post-Render_UI copy or composite running with
   ratio 0.667 and FE4=1 would place a minified full image in the render rect. Reject if a probe at
   native Present shows ratio 1 / activation 0 already, or if setting CS's (1,1,false) at Render_UI
   exit leaves the symptom unchanged.
3. **Publication outside the active rect only.** If the full-size output is published to one target
   while a consumer reads the original kMain region (or the reverse), the upper-left keeps input
   content. Reject by reading back kMain and RT0 at Render_UI return versus Present.

These are unproven hypotheses from source, not findings; none has been tested.

## Unknowns and skipped areas

- Where the engine bakes the fp16 dynamic-triangle UVs (UpdateDynamicResolution, triangle update or
  draw) is not visible in source; it decides whether RSF's post-call ratio write reaches those UVs.
- Stock (non-replaced) behavior of BSDFComposite, SSLR, lens flare and ISHDR/ISGammaResize under
  native DR is described only by CS comments and CS's replacement shaders.
- Native TAA jitter phase count and whether FXAA can run alongside TAA.
- `TemporalPipeline.cpp` 450-890 and 1287-2195 (configuration apply, status, FG transactions),
  `DX12SwapChain.cpp` outside the present path, `Streamline.cpp` outside constants and tagging,
  `AntiLag2Latency`, `StreamlineLatency`, `ReShadeRuntime`, `RCAS` were mapped by function list only.
- Non-temporal features were inventoried by structure, not reviewed for render correctness.
