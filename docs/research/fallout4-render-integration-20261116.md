# Fallout 4 AE render integration source contract

This is a source investigation and cold-creation foundation, not rendering
acceptance. The question is where an independent adapter
can obtain complete SR/FG/CPU facts while preserving F4SE and ENB work. Existing
temporal-renderer source was inspected because it names engine callsites and
documents downstream consumers that a final-frame-only hook would miss. No
reference implementation was copied. No worker build, fixture, game launch,
deployment, network installation or REA target change was performed.

## Identity, revisions and baseline

Installed executable: `Fallout4.exe`, version1.11.240.0,55343016 bytes, DX11,
SHA256 `fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`.
The source lead is `references/fallout4-community-shaders` at
`e0e711594291c1a8e447ac758099492514810fbb` (2026-11-14). Its README line33
explicitly lists AE1.11.240, NG1.10.984 and OG1.10.163. Pinned dependencies:

| Local reference | Revision |
| --- | --- |
| `extern/CommonLibF4` | `7f8deca16dc7dd245c9c52151ec9ef27edd9ad60` |
| `extern/CommonLibF4/lib/commonlib-shared` | `5408d22adf937cc00e0ccd4bdeef66021172884d` |
| `extern/community-shaders-shared` | `6e67083a54090d10806484549d07f75b7fbc8dc2` |

Below, CS paths are relative to that reference; CL means its pinned
`extern/CommonLibF4/include/RE`. These are source line coordinates at those
revisions, not published executable RVAs. Commonlib-shared
`include/REL/VariantID.h:45` repeats the last supplied tuple entry into trailing
runtime slots: a two-entry `{OG,NG}` tuple uses the NG numeric ID for AE too.
That still requires the **AE IDDB**, not the NG offset database. Offsets in
three-entry callsite arrays are OG/NG/AE in that order.

Coordinator installation receipt
`.local/fallout4-research/install-mods-20261116-082259/install.json` records74
files with backups, packaged defaults and valid Ian Patterson signatures for
the F4SE DLL/loader. Supplied F4SE0.7.9 archive SHA256 is
`48677acc460d8dca0f2f6303b0ef7fe16bdb8a97bdc37c6ffb96ec604fec1a48`;
ENB0.501 archive SHA256 is
`c3418d1cc28ea2323c6b6b445508064692d401be2d2f4e6c34233c73bc18effc`.
The worker did not install them.

Coordinator baseline PID57896 via `f4se_loader` initialized F4SE0.7.9 and
created HWND8915930. Module receipts include local ENB `d3d11.dll`,
`d3dcompiler_46e.dll`, Steam and RTSS. CloseMainWindow completed, exit0.
Receipts and fresh F4SE logs are in
`.local/fallout4-research/baseline-20261116-082614`. No RSF was installed;
this proves baseline startup/close only. ENB's packaged FPSLimit61 remains in
place, so later frame-rate measurements must distinguish that external limit
from runtime pacing.

## Concrete hook leads

All signatures below describe the **reference thunk** with x64 register/stack
ABI, not a binary-verified reconstruction of the entire containing function.
`RTM` means `BSGraphics::RenderTargetManager*`; target IDs and enums are32-bit.
Calls must be disassembled, matched to their expected callee and given exact
instruction guards before any production hook is admitted. Preserve unused
register/stack arguments where actual consumers remain uncertain.

| Boundary and signature | Address Library IDs, offsets OG/NG/AE | Source evidence and purpose |
| --- | --- | --- |
| Native creation `HRESULT WINAPI(IDXGIAdapter*,D3D_DRIVER_TYPE,HMODULE,UINT,const D3D_FEATURE_LEVEL*,UINT,UINT,const DXGI_SWAP_CHAIN_DESC*,IDXGISwapChain**,ID3D11Device**,D3D_FEATURE_LEVEL*,ID3D11DeviceContext**)` | `BSGraphics::CreateD3DAndSwapChain{224250,2277018,4492363}+{419,411,410}` -> import stub `{438126,2725810,4818708}` | `src/Render/SwapChainHook.cpp:41,154`; final outer device/chain result after wrapper retries |
| Main-loop message processing `void(Main*)` | `Main::Run{1125396,2718225,4484191}+{1e,4e,2e}` -> `Run_WindowsMessageLoop{847266,2228915}` | `TemporalPipeline.cpp:129,140`; source begins main-loop before call and simulation after call |
| Swap boundary `void(Main*)` | `Main::OnIdle{633524,2228917}+{6ec,cdc,cdc}` -> `Main::Swap{1075087,2228913}` | `FrameBoundary.cpp:20,56`; pre/post swap, not proof of input poll or GPU submit |
| World viewport `void(RTM,bool)` | `DrawWorld::Begin{502840,2318286}+26` -> `SetUseDynamicResolutionViewportAsDefaultViewport{676851,2277194}` | `features/Upscaling/src/UpscalingAnchors.h:10`; world-only viewport |
| Jitter `void(BSGraphics::State*)` | same Begin `+{3c1,38c,38c}` -> `UpdateTemporalData{376068,2277095}` | anchors:16; `TemporalRenderHooks.cpp:155`; call native temporal update before publishing owned jitter |
| Render-size publication `void(RTM,NiPoint3*,NiPoint3*,NiPoint3*,NiPoint3*)` | `Render_PreUI{984743,2318321}+{14b,29f,29f}` -> `UpdateDynamicResolution{1115215,2277195}` | anchors:85; hooks:134; publish after vanilla dynamic-resolution update |
| Main material scopes, reference `void(void*)` call wrappers | Render_PreUI `+{17f,2e3,2e3}` -> `DeferredPrePass{56596,2318301}`; `+{1c9,3a6,3a6}` -> `Forward{656535,2318315}` | anchors:93,99; hooks:404,424; main-view sampler bias only. General prepass detour elsewhere uses `void()`; argument consumption needs binary proof |
| SR seam `void(ImageSpaceManager*,u32,u32,u32,u32)` | `Render_UI{587723,2318322}+{9f,83,83}` -> `RenderEffectRange{459505,2316593}` | anchors:34; hooks:200 names its receiver RTM*, but coordinator's native UI disassembly loads ImageSpaceManager from3e5e4b0; before imagespace reads linear Main |
| Full Render_UI scopes `void()` | `Main::DrawWorld_And_UI{408683,2228969}+{163,182,182}`; `DrawGamePause{572620,2228977}+{103,ea,ea}` and `+{1a7,18e,18e}` -> `Render_UICallee{1130087,2318322}` | `RenderHooks.cpp:404,663`; normal and both pause calls; OG callee is a jump thunk |
| HUD-less seam `void(RTM,bool)` | Render_UI `+{e1,c5,c5}` -> viewport-default target above | `RenderHooks.cpp:417,684`; callback **after** native call, before interface drawing |
| First-person alpha `void(BSShaderAccumulator*)` | OG `ForwardAlphaImpl{338205,0}+253`; NG/AE Forward `+53d` -> `RenderAlphaGeometry{1445970,2317903}` | anchors:23; hooks:184; before/after alpha colour difference and stable depth/motion capture |
| Deferred composite draw `void(void*,u32,bool)` | `DeferredComposite{728427,2318313}+{8dc,915,915}` -> `RenderPassImmediately{244233,2318699}` | anchors:42; hooks:280; reduced-size G-buffer proxies |
| SSR ray start `void(RTM,i32,RenderTargetID,SetRenderTargetMode)` | same composite `+{3d9,3ee,3ee}` -> `SetCurrentRenderTarget{1502425,2277188}` | anchors:50; hooks:309; region/unit-UV correction |
| Pooled SSR acquire/release `void(RTM,RenderTargetID)` | same composite `+{5fb,60e,60e}` -> `AcquireRenderTarget{1468639,2277219}`; `+{6ba,6c3,6c3}` -> `ReleaseRenderTarget{1374956,2277220}` | anchors:58,65; restore while pooled target remains mapped |
| Lens flare `void(NiCamera*)` | `RenderLensFlare{676108,2317547}` | hooks:350; main-depth extent consumer |
| Loading temporal update `void(State*)` | `LoadingMenu::Render{135719,2249225}+{2bd,275,275}` -> UpdateTemporalData | anchors:73; hooks:394; keep loading render unscaled |
| Recreate/reset `void(Renderer*,u32)` | `WindowSizeChanged{212827,2276824}`, `ResetWindow{796949,2276825}` | hooks:454,469; resource rebuild may defer until RT0 repopulates |
| Render target creation `void(void*)` | `BSShaderRenderTargets::Create{1118299,2318909}` | hooks:102; run setup after native allocations |
| Pip-Boy prepasses `void(Interface3D::Renderer*)` | `RenderPrepassesAndMenus{1189309,2222566}` | hooks:269; native-resolution scope for dedicated target |
| Companion map registration `void(Callback)`; callback args template-defined | `SetCompanionLocalMapRenderFunc{1182240,2318360}` | hooks:277 and TemporalRenderer declarations; exact callback ABI still to prove |
| Native TAA eligibility `bool(ImageSpaceEffectTemporalAA*)` | vtable `ImageSpaceEffectTemporalAA` ID1340364, slot8 | hooks:130, install:94; suppress only after valid external publication |
| FXAA initialization `void(ImageSpaceManager*)` | `InitEffects{889489,2316625}` | hooks:446; reference disables FXAA, RSF must establish native AA ownership before changing it |

Independent coordinator objdump evidence now establishes **only creation
instructions**, not the other rows: function RVA1824930 prologue
`48 8b c4 55 56 48 8d a8 88 fe ff ff 48 81 ec 68 02 00 00`;
call RVA1824d40 bytes `e8 94 ff a9 00`, return1824d45; thunk22c4cd9 bytes
`ff 25 79 ca 17 00` resolves import IAT2441758. The source AE+410 agrees.
Receipt `.local/fallout4-research/creation-disassembly-20261116.txt` spans
1824930..1824dd0, and the raw candidate receipt is
`creation-import-candidates-20261116.json`. Post-create reads global38e13a0.
The coordinator subsequently verified output ownership from that disassembly:
1824c72 forms arg10=device at renderer+48;1824c79 forms arg9=chain at+70;
1824cda forms arg12=context at+50;1824cd2 loads HWND from+58 into the native
description. All twelve standard arguments reach1824d40. The renderer pointer
comes from38e13a0. These verified creation fields, rather than a Skyrim layout,
are used by the foundation below.

Additional coordinator disassembly confirms Render_UI21fa200, SR call21fa283
to2185420 with arguments(manager,0,13,1,1), and viewport-reset call21fa2c5 to
1839ec0, return21fa2ca. A similar clone21f9950 is not the selected world/pause
path: pause callers c39d6a/c39e0e in functionc39c80 match both source sites.
Ordinary caller c383c2 is in a chained .pdata fragment c3829b, whose true parent
still needs resolution. UI reads ImageSpaceManager pointer3e5e4b0 and RTM
static2f4d750. WorldBegin21f0ac0+26 and+38c call1839ec0 and182d580;
Render_PreUI21f9c90+29f/+2e3/+3a6 call1839ed0/21f38b0/21f9100. Split .pdata
fragments defeated a naive single-range search; coordinator sequential Capstone
decode resolves the three anchors. Receipts: `render-ui-disassembly.txt`,
`render-boundary-callers.json`, `WorldBegin-capstone.txt`,
`RenderPreUICandidate-capstone.txt` and `native-hook-anchor-analysis.json`
under `.local/fallout4-research`. These additional sites are not activated by
the cold-creation foundation.

## Resource, camera and time contracts

Source mapping is through **logical-to-platform tables at every use**, not a
logical ID as a fixed array index (`src/Render/Engine.h:341,360`). `CL/B/BSGraphics.h`
declares RendererData device+48/context+50, windows[32]+58/stride50, target
array+ a58/stride30, depth array+1d48. A window has HWND+0 and chain+18.
These are layout leads, not yet checked against AE instructions. RTM AE table
offsets differ from OG: target IDs+df4, depth IDs+f84; ratio+fb8/fbc,
viewport-default+fe4 and active+fe5 (`BSGraphics.h:1503`). Guard indices, COM
device identity, resource/view identity, format, samples, extent and lifetime.

| Fact | Source contract and outstanding validation |
| --- | --- |
| SR colour | Logical Main1, **linear pre-tonemap** R11G11B10_FLOAT, one mip/slice/sample in reference preflight (`TemporalRendererInternals.h:68,242`; `TemporalResolve.cpp:288`). Copy before processing and publish back to the same native Main before RenderEffectRange. No display gamma encoding for this input. ENB may alter format/content; prove actual descriptors |
| Depth | Main depth logical1, typed depth SRV resolved from native depth target; reference copies to R32_FLOAT, conventional non-reversed0..1 (`TemporalResolve.cpp:96`; `Streamline.cpp:750`). Native allocation format/clear and coverage need capture |
| Motion | GBufferMotionVectors32, R16G16_FLOAT, unjittered motion with camera contribution; DFPrepass writes `(currentNDC-previousNDC)*(-.5,.5)` (`DFPrepass.hlsl:1666`). This is previous-minus-current UV/Y-down, so GameAPI's current-minus-previous UV convention needs sign inversion on both axes. No bias and zero is a valid stationary vector. Verify raw native shader direction/scale before configuring generic decode; do not invent unwritten-zero semantics |
| Temporal masks | Reference explicitly binds null TAA-mask t0 and normals27 z as unused, accepted zero masks (`TemporalResolve.cpp:94`). It does not establish a Skyrim-like target85 mask. Do not fabricate one. Encode shader's .1R+G formula is dormant with null input |
| Colour auxiliaries | MainCopy2 pre-alpha; GBufferAmbient30 alpha carries Pip-Boy material class; MenuBackground15 doubles as VATS coverage; refraction normals14, motion32, depth1. Native alias/format values must be checked. MainCopy is not the HUD-less final |
| Camera | Capture world camera before/after DeferredPrePass, keyed by Main::WorldRootCamera and `useJitter`, source frameCount. State cameraDataCache+140, stride250, camera key+238/useJitter+240 (`FrameBuffer.cpp:49`). Copy matrices/PosAdjust/previousPosAdjust during that scope; later camera cache may describe UI/shadows |
| Matrix/units | `FrameBuffer.h:141` and `TemporalPipeline.cpp:84` invert/transpose native row data before vendor use, include camera-relative origins, derive near/far from projection and FOV in radians. Streamline source uses0.01428222656 metres per engine unit (`Streamline.cpp:900`). Populate CameraABI1 explicitly; never pass a raw native matrix without convention conversion |
| Jitter | Pixel Halton(2,3), phase count8*(outputWidth/renderWidth)^2; native source readback `jx=-offsetX*W/2,jy=offsetY*H/2`. Owned publication compensates camera builder's division by dynamic ratio: `offsetX=-2*jx*ratioX/renderW`, `offsetY=2*jy*ratioY/renderH` (`TemporalRenderer.cpp:200,226`). State offsets+4/+8; AE TAA state+ac/counter+b0, unlike OG+a8/+ac. Clear jitter before imagespace/UI and outside eligible world scopes |
| Reset | LoadingMenu close requests reset; prepass capture latches it outside main/loading menus (`Feature.cpp:44`; `SharedData.cpp:461`). Missed sequential dispatch and resource recreation request reset too. These are CS-owned reset facts, not proof of a native camera-cut flag. Binary locate native cuts/load/teleport/projection changes and report explicit reasons, without a blanket history-reset hack |
| Time | BSTimer realTimeDelta seconds becomes milliseconds for SDK requests (`TemporalResolve.cpp:195`). Engine frameCount is not a unique source/present ownership token, especially pause-only renders; separate session/source/submission/scope identities |

The reference's R11 format and logical target values are not ENB acceptance.
Do not alter ENB's scene precision, exposure or display HDR policy based on
these SDR source assumptions.

## Reduced raster and downstream consumers

The source preserves full-size allocations while rendering an active region;
it does more than change jitter or copy a smaller rectangle. SR succeeds only
after normal world material, lighting, sky/alpha and auxiliary consumers agree
on the active extent. `DynamicResolution.cpp:24` names proxies for normals,
environment, ambient, AO/full+half, emissive, diffuse/specular/full+tiled,
MainCopy, Main, albedo/specular, SSR prepass/depth/ray and blur1. Blur0 is pooled
only between acquire/release. Composite and SSR proxy scopes copy in/out and
restore original native slots. These are required consumer categories to audit,
not a direction to copy the proxy implementation.

After SR publication, `TemporalResolve.cpp:478` expands native depth and
refraction normals using dynamic UV/jitter compensation, plus nearest-sampled
motion/Pip-Boy class/VATS masks (`DepthRefractionUpscalePS.hlsl:31`;
`ChainInputsUpscalePS.hlsl:30`). The fullscreen triangle has baked reduced UVs
and is overridden for full-extent imagespace. Ratio and jitter return to1/0,
then imagespace/TAA policy runs at output extent. VATS target15 is swapped only
during its consuming effect and restored immediately. Lens flare samples main
depth; water samples SSR ray/blur in native region layout. Pip-Boy and companion
map prepasses render unscaled, not through the world camera/ratio.

Sampler bias is scoped to world deferred and forward material draws. Reference
derives log2(render/output), with an additional DLSS-specific term; this is
adapter sampling policy, not a shared vendor-selection rule. Any RSF setting
must be exposed through the generic overlay. Native failure after a reduced
raster is committed cannot safely skip reconstruction and continue an output
resolution chain: establish a valid spatial/native recovery and restore owned
state before switching back on the next frame.

## FG, final output and explicit leases

Depth/motion are frozen at the pre-tonemap seam **before** SR expands/replaces
them. Reference `TemporalFrameGenerationInputs.cpp:230` pairs frame/camera,
render/output extent and a write lease, then captures HUD-less colour later
from RT0 **after imagespace and before interface drawing** (`:364,417`). SDR
reference HUD-less contract is R8G8B8A8_UNORM, full range, gamma2.2,
post-tonemap/LUT, alpha ignored (`TemporalProvider.h:196`). Final WINDOW colour
has UI and belongs to the actual application Present. Do not use pre-tonemap
SR colour as final FG colour or equate HUD-less completion with Present.

First-person alpha may have colour without trustworthy depth/motion. The
reference saves Main into MainCopy before that alpha call and compares colour
after it. Its conditioning shader sets changed-pixel motion to zero and moves
depth toward0.1 (`features/FrameGeneration/Shaders/FrameGeneration/CopyDepthForFrameGenerationCS.hlsl:32`).
That is an explicit heuristic, **not measured missing-vector repair**. Keep a
same-frame pre/post lease available for investigation; do not add a shader
replacement or blindly inherit this heuristic without draw evidence.

For RSF, native texture pointers live only through their synchronous pass/end
lease. Copy depth/motion before native reuse; a COM AddRef retains allocation,
not contents. Key HUD-less colour, camera and those copies to the same
source/submission/view/native frame and actual outer chain. Retain the final
WINDOW/context/outer-chain lease through actual native Present. Pooled SSR
slots and recreated targets must be resolved again, never cached across their
release/rebuild. Shared runtime owns GPU copies, fences, retirement and vendor
resources; the adapter identifies the native producer and restoration point.

## Shader families and established history corrections

`src/Render/ShaderInjectionTargets.h:14` enumerates13 target families.
This is a source coverage inventory, not a native shader census or captured
draw validation. Do not install a blanket replacement shader set.

| Family/pass | Producer evidence and coverage limit |
| --- | --- |
| DFPrepass rigid/skinned/landscape/grass/tree/LOD/instanced | `PrepassTechnique.h:7` identifies flags; `DFPrepass.hlsl:587,1666,2428,2808` has motion MRT5, previous rigid transforms and previous bone palettes. Verify native permutation bindings, previous/current bones, instance and wind history |
| DFPrepass BLEND/hair/tessellation/dismemberment | NG/AE PREPASS_MOTION_VECTOR is `!BLEND || TESSELLATE_DISP_HEIGHT || DISMEMBERMENT_MEATCUFF` (`DFPrepass.hlsl:120`). Ordinary BLEND remains uncovered. MotionVectorFixes expressly does not repair BLEND gaps |
| Sky | `Sky.hlsl:162,189` writes motion at MRT1 from current/previous clip positions. Verify native RT32 binding and camera/cloud variants |
| Effect/Particle | Forward colour/distortion/soft/additive/multiplicative variants are present. No universal motion producer established by this audit; capture their active colour/depth/motion outputs and transparent/first-person overlap |
| DistantTree and Lighting | Source has current/previous constants, but their presence alone is not an active native motion write. Distinguish prepass, z-only, forward and LOD transitions |
| Water | Forward/refraction and SSR/depth consumers need region/extent correction; no universal independent water motion write established |
| Utility | Depth/z/alpha and occlusion routes must keep native semantics; depth-only is not colour-motion coverage |
| DFLight/DFComposite/DFTiledLighting | Deferred G-buffer consumers, not substitutes for object-motion producers; full/half/tiled inputs require matched sampling rectangles |
| Imagespace/BloodSplatter/interface | Post effects, VATS, UI and screen overlays are not world-motion scopes. Audit separation and actual final/HUD-less colour, with native pause/loading paths retained |

`features/MotionVectorFixes/src/MotionVectorFixes.cpp:34,119,145,180` supplies
specific **CPU history corrections**: reset a sequence-positioned object's
previousWorld after NiAVObject::Update; cache player world transforms around
OnIdle_UpdatePlayer and restore previousWorld from that cache; for frozen/menu
or LOD/landscape lighting pass accumulation set geometry previousWorld=current,
except LoadingMenu. Native NiAVObject source fields are world+70 and
previousWorld+c0. Hooks are OnIdle_UpdatePlayer `{1318162,2228929}` with
`void(Main*)`, SetSequencePosition `{854236,2200766}+1d7` -> NiAVObject::Update
`{121052,2270101}` with `void(NiAVObject*,NiUpdateData*)`, and lighting property
vtable ID241915/slot43 with
`RenderPassArray*(BSLightingShaderProperty*,BSGeometry*,u32,BSShaderAccumulator*)`.
The pass-list callback runs on accumulation threads, not necessarily Present's
thread. Verify object lifetime/worker overlap before adopting any correction;
these bounded producer fixes are preferable leads to guessed shader changes.

## Input, simulation and pacing facts

The reference message-loop wrapper emits simulation/input markers immediately
after WindowsMessageLoop, and pre-swap closes simulation/starts submit
(`TemporalPipeline.cpp:140,414,937`). Those are reference marker semantics,
not proof of actual game input consumption or render-submit timing. RSF must
report facts through GameAPI22 and let the orchestrator place/deduplicate all
markers and the one pacing sleep. Do not port LatencyRouter or vendor calls
into the adapter.

Source leads: BSInputDeviceManager singleton `{1284221,2694300,4807767}`,
devices+8 and pollingEnabled+35; BSInputDevice virtual slot1 is
`void Poll(float)` (`CL/B/BSInputDevice.h:28`). Native poll dispatcher, keyboard,
mouse/controller queues, processed WM messages, worker consumers and poll-to-
simulation reservation/binding are **unresolved**. Main flags gameActive+27,
freezeTime+2a/inMenuMode+1d0 and QGameSystemsShouldUpdate are eligibility facts,
not sufficient source ownership. Trace normal, pause, loading, VATS and queued
worker paths before advertising INPUT_TIMELINE. A render-only fallback may
report rendered provenance honestly; it cannot count as completion of the
requested latency integration.

## F4SE/ENB coexistence ownership

CS README line123 says ENB unsupported; `src/Compat/Rules.cpp:37` disables all
features when `Compat.cpp:73` obtains the ENB API. That is CS policy, not proof
RSF must refuse ENB. CS also owns shader replacement, target format changes,
AA, device creation and a DX12 presentation proxy; running both temporal owners
would create real conflicts. An RSF implementation must map those owners and
negotiate or refuse exact simultaneous conflicting ownership, without treating
the mere presence of F4SE or ENB as incompatibility.

Use existing HostABI8 mode3 Begin/End around the verified game creation path
before an ENB wrapper captures a native HWND chain. Preserve every original
argument/output/HRESULT/LastError and keep the outer chain, device/context and
ENB Present pre/post work. End borrows the returned outputs and validates
canonical outer/inner/window/device ownership. Do not replace the outer chain
late, force-release unknown references, bypass ENB exports or transplant Skyrim
ENB RVAs. Root must audit this exact0.501 wrapper's QI/AddRef/Release/GetBuffer,
private data, resize/fullscreen/Present and device/context wrapping requirements.
Steam/RTSS singleton dispatch hazards and all intercepted methods must likewise
be matched to their installed binaries before adapter-specific guards are used.

F4SE is an authorized coexistence entry environment, not a required runtime
dependency for a standalone RSF adapter. Its existing trampolines, callbacks and
thread/lifetime rules must be preserved. Baseline startup above does not prove
RSF hook coexistence, SR/FG, latency or moving visual quality.

## Practical next implementation contract

No new generic architecture or ABI is justified by the source audit. Start
with HostABI8, GameAPI22, FrameABI2 and CameraABI1: adapter reports native
resources/identity/rectangles and uses synchronous SR processing plus distinct
FG_INPUTS and WINDOW leases. Shared runtime chooses backend/quality/latency,
normalizes resources, owns scoped factory/canonical chain dispatch and GPU
lifetime. Adapter supplies colour/motion conventions, world units, hooks,
sampling/extent restoration and native producer eligibility. Do not advertise
capabilities until the corresponding producer actually exists.

Before implementation, coordinator must provide binary-verified AE offsets,
expected bytes, callee/return ABI and ownership for: creation output slots/HWND;
world begin/jitter/render-size seam; SR and both UI paths; camera/cache and
logical target tables; reset/recreate plus auxiliary consumers; native input
poll/consumption/simulation/submit. Validate native resource formats, active
rectangles, motion units/sign/camera contribution and sparse coverage with
existing bounded captures. The known creation instructions above are the first
verified subset, not permission to apply every source offset.

Proposed adapter-local files, only after that contract is supplied:

| File | Responsibility |
| --- | --- |
| `plugin.cpp`, `CMakeLists.txt` | Exact-build lifecycle and generic host setup, overlay-described adapter options; no vendor calls |
| `native_creation.cpp/h`, `compatibility.cpp/h` | Guarded game call/outer leases, F4SE/ENB/other temporal-owner ordering and exact wrapper facts |
| `render_inputs.cpp/h`, `camera.cpp/h` | Logical resource resolution, same-source SR/FG/WINDOW leases, explicit matrix/motion/unit conversion |
| `resolution.cpp/h` | Owned world viewport/jitter, proxy/pooled/auxiliary consumer restoration, native fallback and resize/load transitions |
| `cpu_events.cpp/h` | Verified prospective input reservation/adoption/binding, simulation/render/Present facts and retry-safe hook lifetime |
| `motion_history.cpp/h` if demonstrated necessary | Narrow matched transform-producer corrections, with worker/object lifetime proof and source-specific tests |

This is an independent implementation plan, not a copy of Skyrim. Only the
cold-creation subset below is implemented from this task. Build/native contract
tests, GPU fixtures, captured SR/FG/resource parity, controlled F4SE/ENB switches,
pause/load/resize/close, moving material/first-person/pickup quality and physical
latency remain separate acceptance work for the coordinator.

## Source-frozen cold-creation foundation

After the coordinator supplied native output-slot proof, the task was extended
to implement only the creation foundation. New `games/fallout4/src/plugin.cpp`
performs exact executable detection, size/version-checked lifecycle and dynamic
binding to the existing Runtime. It requests HostABI8 mode3, but declares zero
FG/input/rendered-source capabilities and rendering_ready0. No shader, resource,
frame, camera, input or CPU marker facts are fabricated. Native render hooks
and the independently owned resource/camera readers remain the next stage.

`native_creation.cpp` guards the exact call/thunk bytes before generic host
setup and again before enable. The existing import thunk is hooked, retaining
its current external IAT destination. Only caller1824d45 with renderer38e13a0,
output addresses device+48/context+50/chain+70 and matching HWND+58/description
may open a synchronous scope. Begin occurs before the complete native call;
End occurs once even on cancellation. Successful borrowed outputs are passed
to canonical admission and presentation binding. Original arguments, HRESULT,
LastError and outer chain reference are preserved. The native device must be
absent before first interception; late ownership refuses. Adapter/Runtime are
pinned, shutdown quiesces callbacks and disables only this hook, and a busy
stop retains prepared state. No F4SE entry/import or ENB-specific RVA is added.
The existing generic plugin carrier can load this Entry through its normal
probe/session negotiation; no new carrier implementation was necessary.

The exact existing seven-method/ten-load Steam profile was moved, rather than
duplicated, to `integrations/steam/src/d3d11_chain.cpp` with an internal static
helper header. An adapter-supplied cold-owner callback replaces the Skyrim
global lookup. All Steam guards, dispatch/refcount semantics, immutable relays,
module pins, queued enable/rollback and process-lifetime rules are unchanged.
Skyrim's small wrapper retains its original3330188/+56 cold test; Fallout4
supplies38e13a0/+48. No game address entered the helper or shared runtime.
Mechanical source comparison against the prior implementation passed, allowing
only the header/namespace/function name and cold callback substitution.

CMake adds the helper before both adapters and adds the Fallout4 DLL. Existing
plugin-contract tests gain an exact fingerprint and invalid-ABI/lifecycle
case; they are authored but not run here. Scoped source/JSON/whitespace and
rel32 arithmetic checks passed. No build, GPU fixture, game or deployment was
performed by this worker; the coordinator must validate the extraction against
Skyrim as well as the new Fallout4 startup before further acceptance.


## Native-raster callback implementation, pending coordinator validation

The next source batch implements native-raster SR without treating the remaining
reduced-resolution work as complete. The question was whether source-led seams
could produce correctly owned synchronous SR, pre-mutation FG inputs and final
window facts from the exact installed executable. Coordinator disassembly and
read-only PE comparison establish the following instruction contract; none is
a captured GPU or played-game result.

| Native function or call | Exact guard and use |
| --- | --- |
| WorldBegin21f0ac0, `void()` | Entry `488bc4534881ecb0000000`; only parent returnc382a5 from callc382a0 admits an ordinary-world TLS scope |
| Viewport1839ec0, `void(void*,bool)` | Entire body `8891e40f0000c3`; world call21f0ae6 and HUDless call21fa2c5 must both target this callee |
| RenderUI21fa200, `void()` | Entry `48895c2410564883ec40`; admitted source plus native call returnsc383c7/c39d6f/c39e13 select the whole imagespace lifetime |
| RenderEffectRange2185420, `void(void*,u32,u32,u32,u32)` | Entry `48895c2418555657415541564883ec30`; return21fa288 and actual manager3e5e4b0 with arguments0,13,1,1 select pre-tonemap insertion |
| TemporalAA IsActive2214fc0, `bool(void*)` | Exact27-byte native getter and RTTI vtable2915eb0 slot8; original state+AC DWORD1, effect+8 and state+B0 checks run first |
| Temporal182d580 | Call21f0e4c plus state TAA read182d586 and jitter stores182d647 are guarded; no replacement jitter sequence or native history reset is introduced |

`render_hooks.cpp` calls the CPU owner's `begin_cpu_render` once inside the
selected ordinary world viewport callback. All later passes require the same
session/source/submission, beganQPC and frequency through
`cpu_source_is_current`. CPU owns counters; render hooks add no second ledger.
The approved AFTER_SIMULATION constructor flag describes the ordinary
update-to-redraw boundary, not an invented SIM_END event. Screen remains UNKNOWN
and NO_FG remains explicit until classification and generic whole-frame
admission are established. Host capabilities remain0: emitting resource leases
alone does not yet satisfy complete FRAME/SUBMISSION/input contracts. See the
separate [CPU ownership evidence](fallout4-input-ownership-20261116.md).

The reader supplies explicit CameraABI1, native frame/camera/window keys and
stack-owned COM resource leases. Motion is previous-minus-current UV, converted
by pass motion_to_uv[-1,-1]; host decoding remains identity. Pre-SR FG_INPUTS
begin/end runs before any SR or output-scope mutation. SR uses Main as both
native input and output, preserving native imagespace execution after the
synchronous shared runtime reconstruction. A failed evaluation retains native
TAA. Successful reconstruction excludes native TAA only until whole RenderUI
returns. The saved native jitter is zero during output-space processing and
restored only while the fields still contain the owned zero; an unreadable or
failed restoration retains busy ownership instead of discarding the lease.
Native resolution inputs must genuinely have equal render/output extents.
Reduced native inputs refuse rather than masquerading as another quality.

At return21fa2ca, the reader resolves logical FrameBuffer0, not an assumed outer
GetBuffer alias. Native frame, HWND, context, outer chain and full texture
extent must match the earlier lease. FINAL_SCENE begin/end delivers that
HUDless image before interface drawing. The adapter retains its allocation
reference and an explicit WINDOW begin through native Swap, including the
actual unchanged Present. CPU invokes `finish_render_scope` before FRAME_END
and clearing its identity; only the matching owner can close that WINDOW.
An AddRef retains allocation, not arbitrary future frame contents. See the
separate [native resource/camera proof](fallout4-native-inputs-20261116.md).

Five entry hooks are installed with existing MinHook queued enable/rollback.
Native calls/arguments, including the complete imagespace range and wrappers,
remain unchanged. Adapter/runtime and render trampolines are process-lived.
Quiescence suspends new work; stop disables only owned hooks and retains CPU
ownership until entered callbacks and WINDOW/output-state leases drain.
Startup rollback clears prepared state and uses the CPU owner's retry-safe
cleanup contract. No vendor SDK calls, pacing policy, sleep or game-specific
runtime branch was added. Plugin preparation verifies the native resource and
render guards before generic host creation and reports distinct CPU/render/
creation refusal stages.

### Reduced-quality proof and implementation still required

Root proved native UpdateDynamicResolution1839ed0 writes ratioFB8/FBC and
activationFE5 at183a26b/183a278. SetEnableDynamicResolution183a2d0 instead owns
counterFE8 and must not be used as a guessed activation writer. Root also
proved RenderEffect2185610 selects ImageSpaceManager triangles+28 unit,+38
dynamic,+40 partial. Its dynamic selector is effect+A8 and RTM+FE4, not FE5.
The receiver at RenderEffectRange is the manager from3e5e4b0, not RTM2f4d750,
correcting the source thunk's misleading receiver type. A reduced output scope
must preserve/restore the actual unit/dynamic pointers through all RenderUI
branches, including failure. No such reduced-state write is active yet.

Verified DeferredComposite21f82a0 anchors lead to SetCurrentRenderTarget1839b70,
Acquire6 at183acf0, Release6 at183ad60 and RenderPassImmediately221fb60.
Those prove call relationships, not that cloning CS's GPU proxy framework is
necessary for vanilla native DR. CS's replacement shaders use temporary proxy
allocations for pre-SR composite/SSLR inputs. Existing generic auxiliary SR
transactions can reconstruct post-SR depth/motion/other surfaces, but do not
establish those earlier consumers. Exact post-SR refraction/PipBoy/VATS maps,
formats and last consumers, resize/recreate ownership, lens-flare depth needs
and any independently required pre-SR repair remain to be closed before
quality choices are exposed. This is pending work, not a native-only completion
claim. No reference implementation was copied.

Checks performed here: installed-PE comparison of all five entry guards/eight
rel32 destinations, TAA slot8, temporal state read/write bytes; JSON parse and
scoped diff whitespace checks. No build, fixture, game, GPU, deployment or
commit was performed. Coordinator owns the combined gate and bounded resource,
backend/quality-switch, resize/teardown and moving visual validation.


Auxiliary filter correction: an initial worker message incorrectly called the
existing generic colour path linear. Inspecting the complete
`auxiliary_sr.cpp` shader proves floor/clamp followed by `Source.Load`, so the
existing COLOUR kind already supplies nearest sampling for motion and discrete
Pip-Boy/VATS values. No new nearest mode or framework is needed. The remaining
reference parity question is optional linear reconstruction for refraction
(and depth), not a reason to reject actual R24 resources because the reader
also permits an unobserved R32 format. Persistent VATS can use the existing
RESTORE_CONTENTS snapshot once exact whole-UI last-consumer/write ownership is
verified; a separate temporary allocation is not presumed necessary.


### Native-first batch freeze manifest

The coordinator explicitly selected this coherent native-raster batch for the
next Release gate and bounded F4SE/ENB SR/resource/close run. This freezes a
stage, not the full integration goal. All current optional auxiliary readers,
actual engine delta, CPU rendered-scope facts and unframed native input receipts
are linked, but quality_choices remains bit0 and full FG/input admission is not
advertised. No filter ABI, temporary VATS allocation or CS proxy framework was
added. Live formats/camera/motion and first SR captures precede those decisions.

Adapter files: `games/fallout4/CMakeLists.txt`, `engine.json`,
`src/plugin.cpp`, `src/native_creation.cpp/h`, `src/render_hooks.cpp/h`,
`src/resolution.cpp/h`. Independently owned reader/CPU files linked in the same
batch are `src/render_inputs.cpp/h`, `src/camera.cpp/h` and
`src/cpu_events.cpp/h`; their dedicated notes define their exact proof and
limits. Root CMake registers the game, tests CMake registers the existing
Fallout4 plugin-contract fixture, and `tests/plugin_contract.cpp` supplies its
fingerprint/refusal cases. The prior foundation's integration-helper extraction
is `integrations/steam/CMakeLists.txt`,
`integrations/steam/include/rescaleframe/steam_d3d11_chain.h`,
`integrations/steam/src/d3d11_chain.cpp` and Skyrim's CMake/small wrapper.
No unrelated README, NOTICE or bottom implementation hunk was changed here.

Exact current hook stage: five entry guards (WorldBegin21f0ac0,
Viewport1839ec0, RenderUI21fa200, RenderEffectRange2185420,
TemporalAA2214fc0); eight guarded direct calls (c382a0,21f0ae6,21f0e4c,
21fa283,21fa2c5,c383c2,c39d6a,c39e0e), plus TAA RTTI slot8 and temporal
state read/write guards. Read-only PE comparison, JSON parse and scoped
whitespace checks passed; builds, fixtures and game runs remain coordinator
work. A hash manifest is retained under
`.local/fallout4-research/native-raster-source-freeze-20261116.json`.

### Minimal next generic filter request, not implemented

If observed resources and downstream comparison require the reference's linear
refraction reconstruction, add one opt-in colour-kind value to the existing
`rsf_d3d11_auxiliary_surface.kind`, for example COLOUR_LINEAR after the existing
DEPTH_STENCIL0 and nearest COLOUR1. Existing kind1, restoration flags, default
sampling, call structures, ownership and synchronous transaction stay unchanged.
The runtime GPU owner can reuse its existing colour shader/constants and select
clamped four-tap interpolation for that explicit kind. This is a proposal for
that owner's size/version/compatibility review after observed data, not an ABI
change in this batch. Any depth filter change needs its own observed need and
must preserve stencil semantics; do not silently reinterpret DEPTH_STENCIL0.


## Coordinator gate and tooling disposition, 16 November 2026

After the native-first source freeze, the coordinator ran the coherent Windows
Release VS2026 `eng/verify.ps1` gate. Receipt
`.local/fallout4-research/verify-native-raster-20261116.log` records exit0,
221 passed and14 skipped, with Cargo fmt/Clippy clean. This supersedes the
prior source-stage statement that the authored contract tests were unrun.
It is build/test evidence, not live RSF rendering acceptance. Production remains
frozen while the coordinator prepares a bounded F4SE/ENB startup, native-AA SR,
F10 descriptor/camera/motion capture and close run. No results for that live
stage are yet recorded here. Full reduced-quality SR, generated FG, measured
latency, moving visual quality and broad coexistence remain separate goals.

The earlier reverse-engineering tool failures are retained to avoid treating
missing output as proof. Ghidra initial import/query attempts timed out at330
and900 seconds; provider cleanup was confirmed, and native creation/render
instruction work continued through independent PE/objdump/Capstone receipts.
A later IDA creation dossier succeeded with evidence
`ev_23d7b5341808c4dd5de7ed89fcbb6a33741d75baa15068176293b6904e545d03`.
After idle, its upstream session
`rea-4cb0cda4-8c13-4a88-8d69-3313695373f3` became unreachable/missing.
WindowSizeChanged, ResetWindow and LensFlare pseudocode requests then returned
errors, so they establish no semantic proof. Capstone remains the continuation
path. WindowSizeChanged18179c0 and ResetWindow1817d70 are unactivated candidates;
old address-library deltas remain search leads until exact native semantics
and lifetime are verified. The successful creation dossier does not promote
those later failed queries or the whole integration to proven support.

This follow-up changed only this note, engine metadata and the top Fallout4
tracker section. CPU/input and reader notes, production files, the existing
source hash manifest and unrelated README/NOTICE/bottom tracker work were left
unchanged. JSON parsing and scoped whitespace checks are documentation checks;
this worker ran no builds, tests, game, deployment or commit.


## Static reduced-consumer follow-up

[Exact VATS, triangle, resize and lens-flare evidence](fallout4-reduced-consumers-20261116.md)
records native consumer/producer and chained-unwind proof, corrected search
leads, the offline shader-corpus join, and unresolved live binding/format
requirements. The first native-raster trial PID48088 admitted the scoped chain
then failed physical creation with INVALID_CALL and exitedC0000005, with21/21
files restored. It is a startup failure owned by the shared investigation,
not SR/FG/latency acceptance. Production adapter rendering remains frozen.


The later PID58104 armed startup trace localizes an AV to a null native
BSPlatform::BSBethesdaPlatform owner during path resolution before SR, with
cause still unassigned. [Exact startup audit and bounded control](fallout4-native-startup-av-20261116.md)
separates controller QASmoke speculation from native proof; production remains
frozen and no native null bypass is justified.


## Main-menu startup control and current gate

Coordinator PID60620 (`native-raster-20261116-100638-894`) completed the45-second
main-menu control with1,806 application Presents, foreground1, effective SR0
waiting verified inputs, normal exit0 and20/20 restored manifest entries.
The original custom INI was unchanged with zero controller writes. The latest
`verify-private-usage-20261116.log` gate passed221/14 and the paired device
fixture passed. See the [startup control receipt and limits](fallout4-native-startup-av-20261116.md).
This accepts bounded menu startup and shows route sensitivity; it does not
assign an unmodded QASmoke cause or accept SR, world resources, FG, measured
latency or full mod coexistence. Production changes remain outside this
follow-up's scope.


## Isolated native screen helper freeze

`screen.cpp/h` now supplies guarded native screen facts, linked by Fallout4
CMake but not called or advertised as a capability in this batch. It guards23
instruction sequences and six menu literals against the exact installed SHA:
six borrowed-name getter prologues/literal/return LEAs, native locked IsMenuOpen
and its on-stack test, two freeze readers and QCameraEquals. Native string
objects stay borrowed; no F4SE call, menu hook, new option or container clone
was added. Read-only PE comparison and scoped whitespace checks passed; no
worker build/test/game/deployment was run.

Loading, Main, Pause, Pipboy, Map, VATS, freeze and free/VATS-camera facts remain
explicit refusals. GAMEPLAY requires the caller's guarded ordinary parent,
current CPU source, matching pass/session/submission/native-frame/view identity,
eligible native Main flags and valid reader-owned world-camera/resource lease;
ordinary camera states0/4/8 must match uniquely. Other/ambiguous camera states,
changed owners/observations and missing data retain UNKNOWN. Point queries are
not an atomic menu snapshot, so later consumers must recheck when required.
The helper neither clears NO_FG nor establishes input ownership. Its verification
entry must succeed before future calls; it remains unused pending reader evidence.

CPU-owned proof for next lease wiring: MainSwap calls1818550 atc335e7
(`e8644fbe00`, returnc335ec), after DrawWorldAndUI and late native cleanup.
That native function calls outer COM Present at181859b and retry18185d5.
Filtered entry1818550 is therefore the proved pre-Present SUBMISSION close,
not17eeed0 and not finish_render_scope after Present. Keep FRAME/WINDOW open
through actual Present/retry and close at the admitted Swap end. Exactly one
source/submission/view association must survive; do not invent CPU/GPU stages
or enable capability flags before the full begin/close/lifetime contract is
wired and validated. No Present hook or render_hooks edit was added here.


## Whole native render leases and screen consumer wiring, source freeze

The next adapter subset wires the previously isolated classifier and whole
render leases; it has not been built or game-tested. Declared capabilities
remain0. No input owner, simulation event or vendor policy was added here.
Reader-owned depth-range repair and the combined validation remain separate.

One FRAME and SUBMISSION begin before the unchanged ordinary WorldBegin,
only for parent returnC382A5 after begin_cpu_render associates the existing CPU
source. Guarded reads snapshot RendererData38E13A0, its device/context/window/
outer chain, world graph32DD338+140 and State3D871A0+98. Context/device canonical
identity, immediate context, outer-chain device/HWND and a second unchanged
owner/source observation are required. Native Renderer Begin1818300 is called
atC3328E, before DrawWorldAndUI atC33434; its18183E0 increment of State+98
therefore precedes this native-frame snapshot. COM pins retain allocations,
not earlier contents; FG input contents are handed off synchronously before SR.

The new void(Renderer*) entry hook1818550 admits only callerC335EC with
Renderer3A25D00. It rechecks the current source, frame/view/context/window and
outer chain, including CurrentRendererWindow38E13B0 == RendererData+58 and
its chain at+18. Normal SUBMISSION end precedes the unchanged native function,
which retains its skip flag, native sync/flags, outer Present and retry loop.
FRAME/WINDOW and context/chain/final-image pins remain through Present and end
after Swap, before the CPU owner clears identity or emits CPU FRAME_END.
Foreign calls preserve their original receiver and behavior. Entry LastError
is restored before the native routine; its result/error is left untouched.

Early FRAME/SUBMISSION metadata stays UNKNOWN/NO_FG. Actual FG_INPUTS, final
HUDless and WINDOW metadata can report GAMEPLAY only after valid reader leases
and the guarded ordinary-source/native screen conjunction. Loading/Main/Pause/
Pipboy/Map/VATS, frozen/free cameras, changed facts or source/owner mismatch
remain refused. Refusal is sticky for that source. If it appears before native
Present, completion is withheld so prepare cannot generate; the outstanding
refused SUBMISSION is balanced after Swap before FRAME end. Setting NO_FG on
an end callback alone is not relied on as a generic completion policy.

Read-only comparison against executable SHA
`fdcef37ac1230af6d0b0050eb2142b139ef3a867b37b9211fb6edfcc646072f8`
passed all6 hook-entry,9 direct-call and7 new Present receiver/continuation
instruction guards. Existing screen guards/literals remain required at prepare
and start. Source-order checks confirm begin nesting, pre-Present submission
close and CPU finish-before-FRAME_END; no worker build, fixture or game run was
performed. Bounded receipts distinguish the first scope boundaries and first8
screen conjunctions. Validation must still establish actual balanced owner
counts, reader/SR admission, input provenance and generated output.

Attribution correction retained: the root's same-save no-RSF F4SE/ENB baseline
PID55372 reproduced native4CB273/null50 with the same close stack; its world
module receipt excludes RSF. That close fault is not assigned to this bridge
and is not bypassed. The baseline also reproduced upper-left/black-margin
scaling, so the measured DPI correction addresses the observed environment,
not a demonstrated RSF-introduced scaling defect.


## Whole-lease gate and deferred trial restoration

The root's coherent Release gate `verify-whole-leases-reader-20261116.log`
completed with220 passed/15 skipped and clean Cargo fmt/Clippy. The extra
`anomaly_fg_dlss_sroff_device` skip was explicitly foreground-denied, not an
activity pass. This gate does not establish game SR/FG acceptance.

Trial `native-raster-20261116-112938-741`, PID37012 started
2026-11-16T10:29:40.1829397Z, exhausted the300-second scene handoff and then
the120-second native-Quit handoff. Controller39768 exited with
`result.restored=false`; the game remained live at dispatch. Focus refusal
prevented the requested synthetic keys, and no scene/resource acceptance or
successful close is claimed. Original failure and result receipts are retained.

An ignored, exact-run recovery helper is prepared for root use only after all
Fallout4/F4SE launchers and game processes exit. It binds the run/files receipt
hashes, executable SHA, PID/start identity and20 explicit destinations. Complete
preflight checks containment, reparse paths, original backup hashes and staged
or already-restored destination hashes before any game-target write. Settings
changes refuse the entire preflight. The sole authorized mutable exception is
the trial loader.log: after exit, its exact contents are preserved with verified
hash/length/mtime in new report evidence before original backup restoration or
removal. Unexpected missing log contents refuse unless a prior verified recovery
and its sealed evidence prove removal. Custom INI is never restored or edited.
A separate durable resumed-restoration receipt journals verified operations;
original controller evidence is not rewritten. AST and17 functions-only temporary
fixtures passed, including changed-log sealing, changed-config refusal and no
partial restoration on that refusal. The actual recovery has not been invoked
by this worker; no game input, close, launch, attach or target write occurred.


## ABI23 built and generic-device-tested next batch

Coordinator `verify-depth-domains-r2-20261116.log` completed exit0 with220
passed/15 skipped in107.24seconds, Cargo fmt/Clippy clean. WARP
auxiliary_sr_device152 passed. A prior gate failed a Windows near-macro
collision in new auxiliary-fixture locals; renamed locals corrected that build
failure before the full r2 pass. The explicitly foreground-denied
anomaly_fg_dlss_sroff_device skip is not an activity pass. Rebuilt scoped fixture
`depth-scoped-fixture-20261116.log` completed exit0 with55 sources,41 captures,
39 SR evaluations,34 tags and34 generated callbacks. This is generic device
validation only, with no physical scanout or measured latency conclusion.

The next batch builds the native prepass copied snapshot and required generic
ABI23 depth descriptor, together with whole FRAME/SUBMISSION and guarded
pre-Present close wiring. Exact Runtime/Entry/carrier hashes are in
[engine metadata](../../games/fallout4/engine.json); the
[depth handoff note](fallout4-depth-hook-integration-20261116.md) describes the
producer boundary. Capabilities remain0 and only native scale is declared.
Native snapshot freshness, actual normalization/SR quality, full reduced scales,
FG/latency admission and shader/motion/visual quality audit remain mandatory
pending; the generic fixture does not supply those native facts.

At this consolidation, PID37012 still runs the exact older ABI22 deployment,
not these rebuilt artifacts. Its report112938-741 retains scene/native-Quit
timeouts after focus denial, no keys sent, and result.restored=false. The
receipt-bound deferred recovery remains ready and unexecuted. The existing
failure/result evidence and original backups remain authoritative; no successful
close or restoration is claimed. This follow-up edits documentation/metadata
only and did not build, run or interact with the game, deploy or commit files.
