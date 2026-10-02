# UE4.18 source map for the AC7 plugin

## TrueSky depth storage and actual view type, 2 October follow-up

Captures `motion-20261002-201334-66924-1` and `motion-20261002-201349-66924-2` retain
268x152 clouds at533x300 rendering. The20:05 kernel compiled but the activation guard never
passed. `TRUESKY_EnsureTexture2DResourceViews`, DLL RVA `0xeae10`, creates a Texture2D UAV,
dimension4. The previous array-view requirement was inferred from the native shader declaration
and was wrong for the actual allocation. The first-party x1 shader now declares Texture2D and
stores two float bounds; the guard checks mip0, one sample/slice,352-byte CB and RG32F views.

`TRUESKY_GenerateNormalizedSceneDepthBounds`, RVA `0xbfb30`, owns normalized scene depth at
view `+0x2d8`. Its texture-ensure call sets native format0x16, mapped to DXGI35/R16G16_UNORM.
Expected16 at RVA `0xbfc05`: `c7 44 24 20 16 00 00 00 ff 50 58 8b 87 94 00 00`.
Replace only the immediate at `0xbfc09`, `16 00 00 00` -> `05 00 00 00`.
`TRUESKY_MapPixelFormatToDxgiFormat`, RVA `0xefb70`, maps5 to DXGI16/R32G32_FLOAT.
Both helper names and the existing producer name are saved in Ghidra. Module/effect fingerprints
are unchanged from the sections below. The native ensure function compares the format, retires
the old allocation and creates matching resource/SRV/UAV views. It retains ownership and lifetime.

The format patch installs with the guarded depth route before any divisor1 activation and restores
after render calls quiesce. A refusal leaves the supported native grid and is logged. Captured
conversion constants at CB offset208 are `(0.1,150000,0,0)`: depth bounds store normalized radial
distance. UNORM16 therefore steps by about2.289 native distance units; captured aircraft bounds incur
up to7.683% relative quantization error. The original shader already saturates before storing.
Consumers at RVAs `0xaa650`, `0xab000`, `0xabe30` sample floating-point bounds; focused review
found no integer reinterpretation or raw resource copy. This supports format promotion, but does
not establish that quantization alone explains every damaged aircraft pixel.

The native view allocator remains responsible for cloud colour, history, checkerboard and tile
resources. At1600x900 output, Ultra's533x300 scene produces534x300 padded clouds at divisor1,
above stock Balanced's roughly29% of output resolution on each axis. Opaque rendering remains at the
selected SR scale. This is a conservative floor, with additional cloud GPU work. The previous
clean-plane report coincided with missing volumetrics and is not acceptance of this correction.

## Missing TrueSky depth permutation, 2 October 2026

User confirms the aircraft is clean and hangar exposure correct in the19:10 build, but flight sky,
lighting and volumetric clouds are broken. Captures `motion-20261002-191851-15564-1` and
`motion-20261002-191902-15564-2` show this damage before DLSS; near/far TrueSky colour is nearly
constant and transparent. Divisor1 allocation succeeded, but the depth producer did not support it.
The earlier allocation-only conclusion below is corrected: valid allocation is not a valid render path.

`TRUESKY_GenerateNormalizedSceneDepthBounds`, TrueSky DLL RVA `0xbfb30`, selects effect
`screenspace_depth_downscale` passes x2, x3 or x4. For divisor1 it skips both constant upload and
effect Apply, then still Dispatches and Unapplies. The cleanup can decrement an unmatched effect
apply count. This is binary/control-flow evidence; the internal counter was not sampled in game.
The function is named and saved in Ghidra.

At DLL RVA `0xbff07`, expected16:
`41 83 FC 02 75 1D 48 8B 4B 08 4C 8D 43 18 48 8B`.
Change only the conditional opcode at `0xbff0b`, `75` (JNZ) to `77` (JA). Nonzero divisors1/2
now enter native x2 binding/Apply/cleanup; x3/x4 keep their original paths. Divisor zero is not valid.
At DLL RVA `0xc003f`, expected16:
`41 FF 92 48 01 00 00 48 8B 4B 08 4C 8D 83 B0 01`.
Replace the seven-byte Dispatch call with `FF 15 <rel32> 90`. A nearby non-executable data page
holds the callback pointer; the native Win64 context/count argument ABI is retained. The callback
forwards through the current context vtable, avoiding a global D3D hook or cached batching method.

The first-party one-to-one kernel runs only for matching full scene/depth extents with native
CB11, t1, s15 and u0 texture-array bindings. Constants are the measured352-byte MixedResolution
layout: depth conversion208, source dims224, target dims240, source offset248, tangent FOV328.
Native depth converts to normalized ray distance, not raw device Z. Output is (max,min,range,0);
one sample has max==min and range0. Native x2 shader is restored immediately after Dispatch,
before native Unapply, including its cached pass state. Downsampled resources forward unchanged.

The shipped `mixed_resolution.fxo` is45263 bytes, CRC32 IEEE `78881ef3`, SHA256
`07eca6e47c1506d5b63236761a97dcf5cdb879480203a63588e978f1210a4d09`. Its x2 compute shader is
3540 bytes, SHA256 `410c6b8f7d63b80a909e117d1c604fee43db8955c6857c4f1d259db8f0021b12`.
The effect-file fingerprint and both code guards must match. Reference bytecode remains ignored.
Full cloud resolution activates only after shader creation and native binding verification in a
supported native pass; refusal leaves native resolution. On quiescent teardown restore the divisor,
selector and call before freeing the relay and shader/device references. FP16 and colour fixes stay.

An explicit NVIDIA GPU experiment executes the original x2 and new x1 kernels with the native ABI,
near/sky depths and a one-pixel geometry column. Maximum error against their respective normalized
ray-depth results is9.31323e-10 and1.86265e-9. This verifies shader execution/layout and conversion,
not the game's clouds or the patched native call site. Moving-game lighting/volumetrics await retest.
See [capture evidence and correction](ac7-plane-artifacts-20261002.md).

## Cloud production and scene colour precision, 2 October 2026

Aircraft artifacts, distant-cloud blocks and DLSS Performance/Ultra colour defects are all
priority-one blockers. The state correction below did not establish their resolution.

`AC7_FSceneRenderTargets_GetSceneColorFormat`, RVA `0x109f300`, hook47, expected entry:
`83 B9 58 02 00 00 02 41 BA 0A 00 00 00 45 8B C2`.
Matching UE4.18.3 `PostProcess/SceneRenderTargets.cpp:2165` selects desktop scene formats from
the cached setting at scene+0x230. Feature level is +0x258; alpha requirement is +0x260.
`AC7_FSceneRenderTargets_AllocateSceneColor`, RVA `0x1095010`, calls the getter at `0x10951b1`;
scene shader-parameter production at `0x1134770` also calls it. Active desktop callers receive
PF_FloatRGBA (10), producing FP16 lighting/sky colour before quantization. Mobile/inactive calls
forward. Both functions are named and saved in the game-dump Ghidra program. There are now48
guarded entry hooks. This changes producer precision, rather than widening a captured texture.

The family allocation owner also caches the format and can retain an existing target without
calling the getter. `AC7_FSceneRenderTargets_AllocateFamilyRenderTargets`, RVA `0x1095480`,
matches UE4.18.3 `FSceneRenderTargets::Allocate`, including format comparison and UpdateRHI.
At RVA `0x1095620`, expected16:
`48 8B 05 29 33 BC 02 48 8B CB 8B 40 04 89 45 D7`.
Replace first13 with `B8 04 00 00 00 90 90 48 8B CB 90 90 90`. This supplies format policy4
while preserving RCX setup and the following store. The engine observes any cache change and
retires/rebuilds targets itself, including activation at unchanged dimensions. It overrides the
code's effective policy rather than repeatedly setting a console variable or releasing live
pooled targets. The patch is guarded/applied during native start, logged, and restored after
calls quiesce; refusal prevents activation. The function is named and saved in Ghidra.

The shipped `TrueSkyPluginRender_MT.dll` SHA256 is
`c6c58ea3a355ed345888b1528e84aa753c6fc69db9c8fb8789e729c7651004d0`, size1711616 bytes.
Its Ghidra image base is `0x180000000`, separate from the AC7 dump. Root interface render is
`TRUESKY_RenderInterfaceViewFrame`, DLL RVA `0x879c0`. At DLL RVA `0x87f6d`, expected16:
`8B 87 10 02 00 00 89 44 24 38 48 8B 45 D0 0F 29`.
Replace only the first6 `8B 87 10 02 00 00` with `B8 01 00 00 00 90`.
This replaces the q.downscale load with divisor1 immediately before the native per-view consumer,
covering later sequence/actor settings. Integer and float setter clamps otherwise enforce at least2.

`TRUESKY_RenderCloudViewWithResolutionDivisor`, DLL RVA `0xac1c0`, consumes argument8, updates
view+0x1e4, marks +0x431 dirty and invokes the native rebuild through vtable+8. That entry is
`TRUESKY_RebuildCloudViewResolutionResources`, DLL RVA `0xbdfe0`, vtable `0x121ae0`.
It divides width/height by the nonzero divisor and pads to a multiple of 2*divisor. Divisor1 is
arithmetically valid here. Depth generation at `0xbfb30` and composite metadata consume the same
argument. Internal checkerboard/amortized buffers and stochastic update remain engine-owned.
All three functions are named and saved in the DLL Ghidra program.

The patch is applied once under a mutex at the active AC7 native TrueSky RenderFrame hook before
forwarding, after the DLL loads. Full expected bytes and page protection are checked; success or
refusal is logged. Teardown restores the original six bytes after calls quiesce. While active,
the change includes SR-off views. It increases cloud work and FP16 scene bandwidth. Game cloud
and aircraft appearance, and temporal stability, require the user-run comparison.
See [producer evidence and replay limits](ac7-plane-artifacts-20261002.md).

## TrueSky composite and graphics-state audit, 2 October 2026

User retest rejects the ordinary-translucency sizing correction as the artifact fix. The latest
flight capture identifies TrueSky PS CRC32C83524e47, VSddc582f0 and current704-byte HdrConstants
CB12 blob154. Near/far/interval inputs268x152, scene/depth536x300 and viewport533x300. The
integer2x2 colour branch and adaptive3876-index composite are measured shader/draw evidence,
not an aircraft-cause conclusion. Paired F9 colour/loss snapshots and actual cloud/depth inputs
will isolate this stage. [Details and failed hypotheses](ac7-plane-artifacts-20261002.md).

Runtime D3D11 state preservation now covers complete six-stage bindings and preserves UAV
counters and CB ranges. Standalone NVIDIA hardware comparison proves omissions/counter reset
in the old saver. UE4.18 D3D11StateCachePrivate caches those bindings; foreign work must restore
them. No new native hook, patch RVA or expected instruction bytes in this runtime correction.
The 10:22 state-correction build retained47 entry hooks. Visual acceptance was outstanding.

## Ordinary translucency ownership correction, 2 October 2026

Hook33 at0x10be2b0 now forwards native sizing without rewriting layer+0x218/+0x21c or scale+0x220.
The superseding UnmodifiedTranslucency begin0x1097d20 allocates from BufferSize+0x208,
which the scoped UI producer already owns. Global enlargement was broader than that UI boundary
and could affect 3D depth/view selection. Native UI/depth resource preparation remains scoped.
Built/deployed, aircraft defect and UI preservation await retest. See
[source evidence and rejected projection hypothesis](ac7-plane-artifacts-20261002.md).

## Bloom source and temporal sample ownership, 1 October 2026

Downsample ComputeOutputDesc0xfb7a00, expected entry:
`48 89 5C 24 08 57 48 83 EC 40 48 8B 01 48 8B DA`.
Constructor DebugName at node+0xc8 identifies SceneColorHalfRes. Common scene input
identity is required before routing this producer and tonemap through one SR output.
Its native descriptor halves parent extent, so bloom/histogram inherit output dimensions.

GetEffectiveViewState0x1126090, expected entry:
`83 B9 B0 09 00 00 02 4C 8B 89 10 14 00 00 75 58`.
Native view state at view+0x1410, stereo sharing handled by getter; eye manager index
at state+0xbd8 and existing pooled refs at +0xbe0/+0xbe8. SR consumes preceding engine
exposure without depending cyclically on the new current histogram; tonemap consumes
the engine's current update. Both mapped helpers are named/saved in Ghidra.

GetCurrentTemporalAASampleIndex0x10ecdd0, hook46, expected entry plus padding:
`0F B6 81 1C 0C 00 00 C3 CC CC CC CC CC CC CC CC`.
Eight-byte getter returns byte+0xc1c; count is +0xc1d. Native pre-visibility reads it
through vtable+0x60 before Halton/Gaussian calculation. FrameIndexMod8 is +0xc20,
not the reconstruction phase. Hook supplies area-scaled phase only inside selected
primary view preparation; native projection and history update code remains responsible
for matrices. Function created/named/saved in Ghidra. Built/deployed, runtime retest pending.
See [evidence and corrected earlier assumptions](ac7-stability-20261001.md).

## TrueSky projection adapter, 1 October 2026

`AC7_TrueSky_RenderFrame`, RVA `0x222afa0`, expected first16 bytes:
`40 55 41 54 41 55 41 57 48 8D AC 24 38 FC FF FF`.
`AC7_TrueSky_AdaptProjectionMatrix`, RVA `0x2225470`, expected first16 bytes:
`48 83 EC 08 80 3D 15 02 69 01 00 0F 28 D9 F3 0F`.
Both named/saved in Ghidra. RenderFrame copies ProjMatrix at parameters+0x50, adapts it,
then calls resolved StaticRenderFrame at plugin+0xa8. Parameters Uid at +0xb0 supplies
the native view. Adapter negates M20/M23 but omits M21; jitter therefore reverses vertically.
Expected-byte guarded hooks44/45 correct M21 only in a matched primary jittered view scope.
See [capture evidence, ownership and validation](ac7-stability-20261001.md).

## Outstanding recording task ownership, 1 October 2026

RVA `0x12183c0`, expected entry bytes
`40 53 48 83 EC 20 8B 1D 94 0C A6 02 85 DB 0F 84`, is the no-argument native
WaitForOutstandingTasksOnly helper. Renderer retirement `0x113a0c0` calls it before
snapshot destruction, matching UE4.18.3 SceneRendering.cpp's
WaitForTasksClearSnapshotsAndDelete. Binary code iterates outstanding graph events, joins
unfinished tasks and releases/clears the event array. Named and saved in Ghidra as
`AC7_FRHICommandListImmediate_WaitForOutstandingRecordingTasks`.

The plugin now calls it before changing/restoring shared SR/UI View state on the render
owner. Null PS enqueue `0x18d21c0` was traced through `0x110dc30` and `0xec5440`
to worker BasePass drawing. Saving View buffers by move exposed null in +0x10/+0x18;
copy/AddRef now saves them without clearing the shared source. See
[race evidence and validation limits](ac7-native-renderer-refactor-20261001.md).
Expected-byte guarded helper invocation, no code patch at this site. Built and deployed;
runtime crash correction awaits user retest.

## FG source-frame association, 30 September 2026

The [render handoff investigation](ac7-render-frame-handoff.md) identifies AC7's source-matched
BeginRenderingViewFamily (`0x111ac10`), renderer construction (`0x1113080`), family copy
(`0x1113350`), queued draw task (`0x11241b0`), render helper (`0x112ff00`) and retirement/delete
(`0x113a0c0`). The renderer owns its copied family at `+0x10`; the task captures the renderer
at `+0x10`. These are static matched/named sites, not installed hooks. The note and its replay
manifest record byte-span hashes, source revision, calling-convention evidence, native counter
differences and RHI/GPU lifetime limits. No Present association or latency measurement is claimed.

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

The [FG CPU boundary investigation](ac7-fg-cpu-boundaries.md) now identifies static candidates for
the outer loop, Windows message pump, Slate device polling and engine virtual tick in the retained
decrypted image. These hooks are not installed or runtime validated. The render handoff remains open.

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

## Native renderer and widget roots, located on 30 September

The [renderer ownership audit](ac7-renderer-roots-20260930.md) follows a capture-proven
GBuffer/tonemap allocation alias back to the engine. Live CodeBrowser MCP selected the matching
decrypted dump; string references, callers and source control flow established these roots:

| Function | RVA | Capture guard bytes |
| --- | --- | --- |
| `AC7_FPostProcessing_Process` | `0xffb900` | `4c 8b dc 55 49 8d ab e8 fc ff ff 48 81 ec 10 04 00 00` |
| `AC7_FDeferredShadingSceneRenderer_Render` | `0xef5cc0` | Static match only; no detour |
| `AC7_UWidgetToTextureConverter_PrepareSlateWindow` | `0x4d69a0` | `4c 8b dc 55 48 81 ec 10 03 00 00` |
| `AC7_UWidgetToTextureConverter_QueueWidgetDraw` | `0x4d6340` | `40 55 57 48 8d 6c 24 a8 48 81 ec 58 01 00 00` |
| `AC7_AddTemporalAA` | `0xff6ad0` | `48 89 5c 24 10 48 89 7c 24 18 55 41 54 41 55 41 56 41 57` |
| `AC7_FRCPassPostProcessTemporalAA_Process` | `0x10aeca0` | `40 55 53 56 41 54 41 55 41 56 41 57 48 8d ac 24 f0 fd ff ff` |
| `AC7_FRenderingCompositionGraph_RecursivelyProcess` | `0x10b3f10` | `4c 89 44 24 18 48 89 4c 24 08 53 48 83 ec 30` |

Names are saved in Ghidra. The [manifest](evidence/ac7-renderer-roots-20260930.json) holds
prologues, 128-byte hashes, source revision and capture evidence. Optional F9 observation hooks
install after decryption, refuse mismatched bytes, roll back partial installation and preserve
arguments. Post-processing and converter scopes are now game-captured in process 220716. The
later main-temporal/graph capture records pass descriptors and candidate RHI command associations;
their runtime validation is pending. The RHI current-command write at `0x120e67c`, bytes
`48 89 05 d5 9c a6 02`, is checked before reading the global at `0x3c78358`. The audit and manifest
also map descriptor gathering (`0x10b3cc0`), context execution (`0x10b3970`), main TAA output
description (`0x1098f40`), RHI execution (`0x120dea0`) and pre-visibility/jitter setup (`0x112afa0`).
These are native/source matches. No native graph insertion or UI producer-size change is applied yet.

The [process-215904 graph capture](ac7-graph-capture-crash-20260930.md) further identifies the
runtime vtable methods for Tonemap (`0x10b0220/0x1098ff0`), FXAA (`0xfbfe90/0xfb6f20`), material
(`0x1004960/0xff7b50`), NimbusHUDCombine (`0xfcbc90/0xfb7bc0`), Nimbus composite
(`0xfc86c0/0xfb7b50`) and UnmodifiedTranslucency (`0x10037d0/0xff7a00`), as Process/ComputeOutputDesc
pairs. Their descriptor names and sampled RHI draw associations establish their roles. Names and
fingerprints are saved in the manifest/Ghidra. No descriptor/rectangle override is applied yet.

## Small-object velocity culling, located on 30 September

The coverage follow-up traced the known `RenderDynamicVelocitiesMeshElementsInner` at RVA
`0x1184dc0` into the `FPrimitiveSceneInfo::ShouldRenderVelocity` analogue at `0x10fdc00`, via the
call at `0x1184e60`, before the existing dynamic draw at `0x1182390`. This is a native/source match
using the decrypted image, not a source-derived address. See
[coverage evidence](evidence/ac7-velocity-coverage-20260930.json) and the
[motion investigation](ac7-motion-vectors.md#attached-weapons-and-moving-ground-vehicles).

The bounds-size comparison matches `PrimitiveSceneInfo.cpp:562-570`. It uses view size threshold
`+0x1174`, LOD factor `+0xc2c`, proxy radius `+0x118`, and constant `0.02` at RVA `0x2671470`.
At RVA `0x10fdd0c`, bytes `76 38` are `JBE` to false return `0x10fdd46`. The proposed diagnostic
patch is `90 90`, bypassing size rejection while preserving other eligibility checks. It has not
been applied or tested on the user's carrier-launch/refuelling examples or ground vehicles.

The later `HasVelocity` analogue at `0x1183820` matches camera-cut/mobility rejection, an
always-velocity flag, history lookup, and transform equality with tolerance `0.0001`, stored at
RVA `0x2580188`. Do not bypass missing history to invent object motion. The three functions were
named and decompiled using Ghidra headless on 30 September, in the ignored
`references/ghidra-ac7-motion` project. Opt-in observation detours now forward native decisions
unchanged; no eligibility bypass is applied. See the motion investigation for capture validation.

## Separate translucency carries no velocity, and what could be done about it

The [30 September motion investigation](ac7-motion-vectors.md) adds native shader evidence and
shader-replacement routes. The ordinary producer carries total camera/object motion; a custom
projection branch selects different view matrices and jitter handling. Stock CPU sprite inputs
also carry `OldPosition` even though their velocity helper returns current position. AC7's actual
sprite stream/history remains to be captured. The coverage gates above are located, not patched.

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

Hangar null-slot CPU-origin probes, all forwarding unchanged:

| RVA | Input convention | Expected entry |
| --- | --- | --- |
| 0xe9b880 | raw uniform pointer | `48 89 5C 24 18 48 89 6C 24 20 56 48 83 EC 20 41` |
| 0xedb9d0 | object+0x38 uniform | `48 89 5C 24 20 56 48 83 EC 20 41 80 78 06 00 48` |
| 0x18d21c0 | reference+0 uniform | `48 89 5C 24 20 56 48 83 EC 20 41 80 78 06 00 48` |
| 0xf4c620 | immediate creation | `40 53 56 57 48 83 EC 30 41 80 78 06 00 4D 8B D1` |

These are the four Ghidra xref creators of pixel uniform command executor0xe16220.
They log CPU stacks only for a bound slot1 whose actual uniform is null. Crash unresolved.

Direct crash-binding diagnostics: pixel setter `0xe39520`, entry
`48 89 5C 24 10 55 56 57 41 56 41 57 48 83 EC 20`; pixel table commit `0xe1f8d0`, entry
`48 89 5C 24 20 55 41 56 41 57 48 83 EC 30 0F B7`. The queued command0x120cf50 dispatches
context vtable+0x1e0 to the setter. Observers copy bounded numeric bind records and report
missing required uniforms/layout hashes; forwarding remains unchanged. Crash not yet fixed.

Repeated crash follow-up: SetupDownsampledTranslucencyViewUniformBuffer is `0x116ea00`, entry
`4C 8B DC 55 56 57 41 56 48 81 EC 78 0D 00 00 48`. Earlier 0x1169a00 was a subtraction error
in notes and was never invoked by earlier code. The corrected native helper is byte-checked
and ensures missing scaled buffers before material selection. Pixel View enqueue at
`0xe97d80`, entry `48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 57`, has a bounded null-input
observer. Queued uniform-pointer lifetime is documented in the native renderer refactor notes;
generated buffers now remain owned through a post-retirement RHI completion scope.

Native UnmodifiedTranslucency producer correction, 1 October:

| RVA | Boundary | Expected entry bytes |
| --- | --- | --- |
| 0x1097d20 | BeginUnmodifiedTranslucency hook | `48 89 5C 24 18 55 56 57 48 83 EC 30 83 B9 58 02` |
| 0x109d320 | ResolveUnmodifiedTranslucency hook | `40 57 48 83 EC 40 83 B9 58 02 00 00 02 48 8B FA` |
| 0x1168d40 | RenderTranslucencyPass hook | `40 55 56 57 41 56 48 8D 6C 24 C1 48 81 EC B8 00` |
| 0x109e0d0 | Enlarged depth allocator helper | `4C 89 44 24 18 53 56 57 48 81 EC A0 00 00 00 48` |
| 0xee93f0 | Native depth resampling helper | `48 89 5C 24 08 55 56 57 41 54 41 55 41 56 41 57` |

The last two match stock GetDownsampledTranslucencyDepth/DownsampleDepthSurface through
the shipped scaled-view producer at 0x116ea00. Temporary output-size UI rect, native unjittered
uniform and populated enlarged depth exist only between UI begin/resolve; state is restored
after queued resolve. Source/binary matched and MSVC built; game validation pending.

1 October native UI sizing: FSceneRenderTargets::SetSeparateTranslucencyBufferSize is matched
at RVA `0x10be2b0`, entry `48 89 5C 24 08 57 48 83 EC 20 65 48 8B 04 25 58`.
The native hook derives layer +0x218/+0x21c and scale +0x220 from actual BufferSize +0x208 and
runtime output size before allocation. It depends on the eleven enlarged depth/view patches.
Ghidra name saved as AC7_FSceneRenderTargets_SetSeparateTranslucencyBufferSize.
See the captured UI branch correction in ac7-native-renderer-refactor-20261001.md.

The later [capture work](ac7-frame-capture.md) established live resources, view data, jitter, and render-scale control. The research proxy now evaluates DLSS. The [representation plan](../representation-plan.md) carries the rest: UI extraction, the presentation bridge, and the vendor contracts. The source inspection itself remains distinct from those later runtime results.

## 29 September: existing translucency scale site, revised policy

No new hook address or expected bytes. The existing 15-byte window at RVA `0x10be329`
(`73 0D 40 84 FF 74 08 F3 0F 10 0D 58 61 4B 01`) still becomes the documented aligned
immediate stub. Its scale now defaults to `100 / applied_scene_percent`, keeping full-output
briefing dimensions across scene quality changes. The four depth sites remain as
recorded. [Decision, inherited evidence and validation limits](ac7-consumer-session.md).

## Enlarged separate translucency: shader view and depth consumers

29 September 2026. Actual layer draws at 1600x904 bound 800x452 VS/PS views. Matching
`4.18.3-release` source identifies `FTranslucencyDrawingPolicyFactory::DrawMesh` at
`TranslucentRendering.cpp:581` and `FSceneTextureShaderParameters::Set` at
`PostProcess/SceneRenderTargets.cpp:2619-2627` as additional Scale < 1 consumers.
Capstone inspection of the recorded decrypted AC7 image confirms each COMISS compares the
scene-context scale at +0x220 against float 1.0 at RVA 0x2574494. Only the JBE opcode changes to
JE, preserving the separate-pass and depth-availability checks. The nine-byte compare/branch
window is required at runtime, not just the two branch bytes.

| Compare RVA | Branch RVA | Expected nine bytes | Replacement branch |
| --- | --- | --- | --- |
| 0x11583a6 | 0x11583ad | `0F 2F 80 20 02 00 00 76 0E` | `74 0E` |
| 0x1025b12 | 0x1025b19 | `0F 2F 86 20 02 00 00 76 04` | `74 04` |
| 0x10294d2 | 0x10294d9 | `0F 2F 86 20 02 00 00 76 04` | `74 04` |
| 0x102ada2 | 0x102ada9 | `0F 2F 86 20 02 00 00 76 04` | `74 04` |
| 0x102c672 | 0x102c679 | `0F 2F 86 20 02 00 00 76 04` | `74 04` |
| 0x102e792 | 0x102e799 | `0F 2F 86 20 02 00 00 76 04` | `74 04` |
| 0x1031902 | 0x1031909 | `0F 2F 86 20 02 00 00 76 04` | `74 04` |

The first site forms the DrawMesh scaled-view boolean in the code fragment covered by unwind
range 0x11582c9..0x115840d. The remaining sites are six instantiations of the sampled-depth
selection. Individual shader-stage names are not assigned to those instances without additional
call evidence. All eleven allocation/view/depth windows now preflight before any behaviour changes;
a write failure attempts guarded rollback and prevents a scale above one. The full-output follow-up
capture verifies 1600x904 bound view buffers. It did not settle the visible shimmer because the
terrain's domain stage was not yet covered by the unjitter override.

Ghidra bridge unavailable in this session; source identities are recorded here for later annotation,
not claimed as names applied to a Ghidra program. [Capture and subsequent stage evidence](ac7-consumer-session.md#captured-enlarged-view-mismatch-and-tessellation-gap).

## Native renderer ownership, 1 October 2026

[Refactor implementation](ac7-native-renderer-refactor-20261001.md) and
[guarded-entry manifest](evidence/ac7-native-renderer-refactor-20261001.json) now describe installed
native controller hooks, rather than capture-only candidates. Constructor `0x1113080` sizes owned
views before allocation, using family recomputation `0x19b6fe0`. Pre-visibility `0x112afa0` scopes
native temporal preparation. Post-processing `0xffb900` captures the velocity reference, and graph
context execution `0x10b3970` inserts a registered SR/fallback node before tonemap. Native node
registration is `0xe93670`; pooled output surface request is `0x10b4870`. Reviewed pass/descriptor
entries retain their September addresses and gain copied RHI scopes and downstream size ownership.

The CPU shader parameter cache is at view `+0x1418`, with `0xcf0` meaningful bytes and native
uniform handle at `+0x10`. Setup producer `0x11346d0` rebuilds parameters from current/previous
view matrices; GRHI at `0x3c783d0`, virtual `+0xf0`, creates a single-frame uniform using layout
`0x3caa6c8`. Move assignment `0xde5cf0` transfers references and clears its source. This narrower
path avoids the dynamic drawing-resource initialization in `InitRHIResources` (`0x1127a20`).
Scene extent is `+0x208`, depth pool `+0x60`. These are matched private layouts and synthetic-tested
adapters. They are not yet AC7-validated. The manifest records every expected entry byte sequence.

### Owned widget raster producers, 1 October continuation

QueueRender scopes ownership without retaining UObject pointers. PrepareTargets allocates physical
raster density; DrawScaledWindow preserves logical geometry. Borrowed converters are excluded.
Expected bytes and source/runtime evidence are in the [native manifest](evidence/ac7-native-renderer-refactor-20261001.json)
and [research](ac7-native-renderer-refactor-20261001.md). Source inspected and MSVC built, game pending.

| RVA | Native function | Expected entry bytes |
| --- | --- | --- |
| 0x4d6340 | AC7_UWidgetToTextureConverter_QueueRender | `405557488d6c24a84881ec580100000f` |
| 0x4d6660 | AC7_UWidgetToTextureConverter_PrepareTargets | `48895c2408574883ec5080797800488b` |
| 0x1380d80 | AC7_FWidgetRenderer_DrawScaledWindow | `488bc44889580848897010488978184c` |
| 0x1ab0200 | AC7_UTextureRenderTarget2D_InitCustomFormat | `0fb644242883e0018991d00000004489` |
| 0x1ab7dc0 | AC7_UTextureRenderTarget2D_UpdateResourceImmediate | `48895c2410574883ec4048837970000f` |

HUD SetShaderParameters RVA `0xfcf880`, entry `488bc45553574155488d68a14881ec88`, binds view+0x13a0 UI
render target/resource+0x70/RHI texture+0x30 in all eight variants. Live Ghidra named/saved;
ABI3 leases UI input and runtime snapshots pre/post composition on demand. Source inspected and
MSVC built, game/alpha conventions and simulation identity pending. See native refactor research.

Native source identity continuation hooks the previously mapped outer Tick/device poll/renderer
retirement sites. Tick scopes poll IDs; constructor binds renderer; queued packets copy IDs;
retirement unbinds before deletion. ABI4, MSVC built only. Expected bytes:

| RVA | Function | Entry |
| --- | --- | --- |
| 0x3a3f70 | AC7_FEngineLoop_Tick | `488bc448895810488970185557415441` |
| 0x113a0c0 | AC7_FSceneRenderer_WaitForTasksClearSnapshotsAndDelete | `48895c2408574883ec20488bfa488bd9` |
| 0xcea130 | AC7_FSlateApplication_PollGameDeviceState | `83b9b001000000488bd17522f20f1082` |

Native helper0xff9af0 removes projection jitter and recomputes matrices before the reviewed
post-SR uniform rebuild. No additional HUD GPU-buffer override added. See native refactor notes.

Shared UI producer: UNimbusGameInstance Init RVA0x3cd130 creates HUDWidgetRenderTexture+1f0
and StereoUIRenderTexture+1f8 at literal1920x1080, format2/linear1. Hook scopes target allocator
1ab0200 to those objects. Borrowed queue validates OwningGameInstance+140 and native class hierarchy
against GetPrivateStaticClass923e60, resizes selected target before raster, keeps logical1920x1080.

| RVA | Function | Expected entry |
| --- | --- | --- |
| 0x3cd130 | AC7_UNimbusGameInstance_Init | `48895c2410488974241848897c242055` |
| 0x923e60 | AC7_UNimbusGameInstance_GetPrivateStaticClass | `4c8bdc4883ec78488b050ae115034885` |

Live Ghidra named/saved; MSVC build only; native lifecycle/output transition validation pending.

Engine update-to-render hooks, MSVC build only:

| RVA | Function | Expected entry |
| --- | --- | --- |
| 0x3c7a30 | AC7_UNimbusGameEngine_Tick | `48895c2408574883ec300f2974242041` |
| 0x1781e80 | AC7_UGameEngine_Tick | `488bc444884018488948085553565741` |
| 0x177cdd0 | AC7_UGameEngine_RedrawViewports | `48895c2408574883ec20488bd90fb6fa` |

GEngine3cbbc28 object validation; Redraw entry closes update phase before constructor binding.
AFTER_SIMULATION is copied association evidence, not a vendor marker/Present proof. Names saved.

FViewport Draw RVA0x1ac3600 entry4055565741544155488dac2470feffff, traced from current GEngine
GameViewport720/native viewporta0. Constructor-bound opaque viewport_key (ABI6) survives queued
SR/HUD scopes. Native flags forwarded unchanged; this is not final Slate/Present association.
Named/saved in live Ghidra, MSVC built only; source UnrealClient.cpp1110-1235.

Queued Slate window hooks, MSVC built only:

| RVA | Function | Expected entry |
| --- | --- | --- |
| 0x1345310 | AC7_FSlateRHIRenderer_DrawWindowsPrivate | `488bc4488950105553488da888feffff` |
| 0x133c6c0 | AC7_SlateWindowDrawTask_Allocate | `4056574883ec4848895c2440488bf148` |
| 0x1346520 | AC7_SlateWindowDrawTask_Execute | `48895c241048896c2418488974242057` |
| 0x1343660 | AC7_FSlateRHIRenderer_DrawWindowRenderThread | `4c8bdc555357498dab08faffff4881ec` |

Main weak window GameEngine+df8/refe00, viewport+720/clienta0; bounded copied task bindings;
viewport-info RHI+70; FRHIViewport native swapchain getter slot8 source matched/QI leased. Runtime
shipped getter/layout still needs validation. ABI8 window metadata remains active across native
RHI window drawing and actual COM-matched Present observer; flags/result and scene agreement open.

Ordinary Slate PS texture/sampler binder RVA0x1351e10, expected48895c2418555657415541564883ec20.
Arguments shader/cmdlist/TextureRHI/samplerref; texture index+c0/count+c2, sampler index+c4/count+c6.
Mapped from ordinary DrawElements branch, named/saved in Ghidra. ABI10 candidate-only resource lease
and queued binding comparison against final family surface; MSVC built, game/material/custom
coverage pending. Source SlateRHIRenderingPolicy769-878.
