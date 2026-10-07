# Project Wingman renderer investigation

Research history: findings, hook addresses and pending statuses below apply to their recorded
experiments. Later increments can supersede earlier conclusions. See
[current implementation and validation](../current-status.md) before using this as a feature list.

Investigation: 2 to 3 October 2026. This is static executable, packaged-asset and matched-source
analysis. No Project Wingman injection, instruction patch, frame capture or device validation
was performed. The plugin scaffold only recognizes the examined build and refuses activation.

## Question and approach

Find native scene colour, depth, motion, jitter and output ownership for super resolution, then
identify the HUD producers and a completed HUD-less surface for frame generation. The user
reports native TAA, a resolution-scale option, VR support and HUD elements that follow the
player's head. These are useful leads, but neither head-following nor TAA proves the required
final composition boundary or dense motion coverage.

The game exposes readable native instructions, Unreal registration data and renderer helpers.
Match it against UE4.27.2, use symbol-backed Function ID signatures to seed identities, and
confirm important functions through callers, virtual tables, control flow and private field
accesses. This exposes the engine owners more directly than classifying arbitrary D3D bindings.
The reference source is licensed material kept outside Git; this document records findings,
not copied implementation code.

## Build and reference identity

| Item | Observed value |
| --- | --- |
| Steam app / examined build | `895870` / `16671873` |
| Executable | `ProjectWingman-Win64-Shipping.exe` |
| SHA256 | `092e84225624a4de9c05d2404ff55269bd4a2aa2ff0548f2a183c6bf36abc85a` |
| File size | `159688704` bytes |
| PE machine / image base | x64 `0x8664` / `0x140000000` |
| PE timestamp | `2024-12-08 09:44:06 UTC` |
| Raw fixed version / branch marker | `4.27.2.0` / `++UE4+Release-4.27-CL-0` |
| Reference source | `4.27.2-release`, revision `d94b38ae3446da52224bedd2568c078f828b4039` |
| Launcher symbol reference | UE4.27.2, changelist `18319896`, Development Win64 Editor modules |
| Analysis tool | Ghidra `12.1.4`; installation directory retains an older version name |

The full reference checkout is `references/UnrealEngine-4.27.2`, untracked and self-contained.
Git connectivity checked successfully, with no object alternates. The older UE4.18 reference
was preserved. The game's source-path marker names a `UE4-PW` engine checkout. Its shipping
layout differs from the Editor reference, so this is a matched branch rather than a guarantee
of an unmodified engine or identical compiler output.

D3D11 device creation and DXGI factory imports are present. OpenXR, OpenVR and Oculus libraries
and game-specific stereo components are present. No active DirectX/VR mode or Vulkan execution
was measured. The game's inclusive Blueprint nativization setting and native HUD state machines
also qualify the assumption that a Blueprint-heavy game contains little game-specific native code.

Read-only parsing of the unencrypted version-11 `pakchunk0` index found 25,661 entries.
Index/directory hashes and selected entry payload hashes were checked. No assets were extracted
into the tracked tree. Packaged defaults include TAA, temporal upsampling, base-pass velocity,
deferred rendering, separate translucency and no pre-exposure. See
[engine.json](../../games/project-wingman/engine.json) for the exact defaults. They do not establish
effective settings in a loaded scene. DLSS, FSR, FSR2, XeSS and NIS are disabled in the packaged
project plugin declarations; this is not a runtime backend test.

## Function ID results and limits

Eleven Launcher DLL/PDB GUID and age pairs were verified before import: Core, CoreUObject,
Engine, Renderer, RenderCore, RHI, D3D11RHI, Slate, SlateCore, UMG and HeadMountedDisplay.
The resulting database holds 185,694 unique signatures from 274,661 contributions before
deduplication. The initial game run recorded 12,010 single matches, 3,167 multiple matches and
537 conflict bookmarks. Further auto-analysis finished with 183,217 functions; recovering the
eight-byte `GetDebugName` leaf brought the count to 183,218.

Twenty functions were explicitly named and annotated in the saved game project: nineteen
`UE427_` engine functions and `PW_ConstructWingmanWidgetComponent`. Function ID alone is not
their validation. The main temporal and widget entries needed manual source/caller/vtable
matching after analysis. Earlier useful FID names such as `DrawHUD`, `RequiresSecondaryUpscale`
and `PrimitiveHasVelocityForView` remain leads verified through their implementations.

Database-generation corrections retained for reproduction:

- Ghidra discovers raw `.fidbf` files in FunctionID's `data` directory. Editable `.fidb` containers
  must be exported with `FidDB.saveRawDatabaseFile`, rather than renamed.
- The headless yes/no property uses `true`, and the database choice uses its basename. Register
  the database in the same headless process before population.
- Script failures can accompany exit code zero. Check diagnostics and nonzero signature counts.
- The temporary installed FunctionID prescript optimization was restored and its original hash
  checked. The graphics reference pipeline uses private scripts instead.

Local reports remain in `references/ghidra-pw-ue4272-fid`: `reference-manifest.json`,
`matching-result.json`, `game-fid-bookmarks.json` and population logs. These contain licensed
reference provenance and analysis artifacts and stay untracked.

Two additional x64 databases were built and installed, without replacing system/game DLLs:

| Database | Reference | Stored signatures |
| --- | --- | ---: |
| `RSF_DirectX_Windows26100_x64.fidbf` | Nine Windows DLLs with Microsoft public PDBs | 34,795 |
| `RSF_VulkanLoader_1.4.357_x64.fidbf` | Rebuilt optimized Vulkan loader with full PDB | 3,985 |

DirectX covers D3D9, D3D10, D3D10.1, D3D10Core, D3D11, D3D12, D3D12Core, DXGI and
D3DCompiler_47. PDB matching uses GUID and DBI age: these public symbols have info-stream age
3 but DBI/PE age 1. The Vulkan SDK lacked the matching loader PDB, so the reference was rebuilt
from Loader revision `5f157b62e333c63260d05d81bf66faa216ab0fb8` and Headers revision
`e3b1eec08173d6b825cd3ac88c885a63b621504a`, tag `v1.4.357`. It uses MSVC `19.51.36257.0`,
Release `/O2 /Ob2 /Zi` and `/DEBUG:FULL`. This is not an exact match to the installed loader.
Provenance/results are in `references/ghidra-graphics-fid`. These databases can help analyze
graphics library modules; they do not resolve dynamic COM dispatch automatically or establish
that this game runs Vulkan. They were not used to claim new Project Wingman matches.

## Named native function map

Addresses below are RVAs relative to `0x140000000`, for the fingerprint above only. They are
static integration candidates. Hook prologue bytes, detour safety and live invocation still need
validation before any hook or patch is enabled.

| Ghidra name | RVA | Matched owner / evidence |
| --- | --- | --- |
| `UE427_FDeferredShadingSceneRenderer_Render` | `0x5983720` | `DeferredShadingRenderer.cpp:1338`; EarlyZ, family output and per-view post-processing call |
| `UE427_AddPostProcessingPasses` | `0x5bbfd00` | `PostProcess/PostProcessing.cpp:240`; profiling scope, temporal inputs and both upscale branches |
| `UE427_FDefaultTemporalUpscaler_AddPasses` | `0x5c442b0` | `PostProcess/TemporalAA.cpp:1532`; default vtable and Gen4/Gen5 dispatch |
| `UE427_ITemporalUpscaler_GetDefaultTemporalUpscaler` | `0x5c4d870` | `TemporalAA.cpp:1577`; static singleton return |
| `UE427_FDefaultTemporalUpscaler_GetDebugName` | `0x5c4d860` | Eight-byte `lea/ret`; exact default-upscaler string |
| `UE427_AddGen4MainTemporalAAPasses` | `0x5c41040` | `TemporalAA.cpp:1433`; upscale/history setup, optional Mitchell filter, half-res output |
| `UE427_AddGen5MainTemporalAAPasses` | `0x5c413c0` | `TemporalAA.cpp:980`; alternative multipass rejection/history graph |
| `UE427_AddTemporalAAPass` | `0x5c44910` | `TemporalAA.cpp:629`; Gen4 caller, history, permutations and named texture allocation |
| `UE427_SetupTAASampleWeightParameters` | `0x5c63170` | `TemporalAA.cpp:483`; nine-point weights, Gaussian/CatmullRom and jitter input |
| `UE427_AddTonemapPass` | `0x5c072c0` | `PostProcessTonemap.cpp`; output name, grain option and compute/pixel branches |
| `UE427_UWidgetComponent_GetRenderTarget` | `0x68462f0` | `UMG/Private/Components/WidgetComponent.cpp:1944`; pointer getter and game radar consumers |
| `UE427_UWidgetComponent_UpdateRenderTarget` | `0x6898bf0` | `WidgetComponent.cpp:1710`; creation, resize, clear alpha and material updates |
| `UE427_UWidgetComponent_UpdateMaterialInstanceParameters` | `0x6898390` | `WidgetComponent.cpp:2124`; texture, tint and opacity parameters |
| `UE427_UWidgetComponent_ShouldDrawWidget` | `0x6889130` | `WidgetComponent.cpp:1240`; visibility, off-screen threshold and redraw policy |
| `UE427_UWidgetComponent_DrawWidgetToRenderTarget` | `0x6836c60` | `WidgetComponent.cpp:1258`; size update and draw handoff |
| `UE427_FWidgetRenderer_DrawWindowScale` | `0x6837410` | `UMG/Private/Slate/WidgetRenderer.cpp:148`; root geometry and draw size |
| `UE427_FWidgetRenderer_DrawWindowGeometry` | `0x68375e0` | `WidgetRenderer.cpp:183`; paint-argument construction |
| `UE427_FWidgetRenderer_DrawWindowPaintArgs` | `0x6836f80` | `WidgetRenderer.cpp:208`; Slate paint, draw buffer and renderer queue submission |
| `UE427_UStereoLayerComponent_SetTexture` | `0x6c89630` | `StereoLayerComponent.cpp`; compare/write texture and mark dirty |
| `PW_ConstructWingmanWidgetComponent` | `0x527b310` | Stock base constructor plus custom primary/secondary vtables; reflected game class registration |

Additional verified FID helpers are `PrimitiveHasVelocityForView` at RVA `0x5db3550`,
`DrawHUD` at `0x6d7a620` and `RequiresSecondaryUpscale` at `0x5bd3280`.
The FID `GetJitterOffset` at `0x59b6300` belongs to distance-field AO, not temporal AA.

## Temporal inputs, ownership and reinsertion

The default temporal-upscaler instance is at RVA `0x9115650`, with vtable RVA `0x83f4140`.
Its debug-name slot is `+8` and `AddPasses` is `+0x10`. The caller selects a family temporal
interface at `Family+0xb8`, or the default singleton, then supplies all three RDG textures.
`FPassInputs` offsets are bool `+0`, format `+4`, colour `+8`, depth `+0x10` and velocity `+0x18`.
The four outputs are main colour/rectangle and optional half-resolution colour/rectangle.
The main rectangle follows the secondary view size and need not equal the presentation extent.

The Gen4 helper independently establishes these game-specific view fields:

| Field | Offset / identity |
| --- | --- |
| Primary screen-percentage method | `FViewInfo+0x15f4`; temporal-upscale branch compares enum 1 |
| Active view rectangle | `FViewInfo+0x1670`, four 32-bit coordinates |
| View state | Pointer at `FViewInfo+0x1680` |
| Previous temporal history | `FViewInfo+0x53d0` |
| Output temporal history | `ViewState+0x990` |
| Camera cut | Byte at `FViewInfo+0xd90`, alongside history-validity test |
| Temporal jitter | Float2 at `FViewInfo+0x4fa4`, render-pixel units |

Instruction VA `0x145c449d6` tests the camera-cut byte. VA `0x145c44ec9` loads packed jitter
for the weight helper called at `0x145c44ed0`; the upsampling branch independently reads X/Y
at `0x145c456aa` and `0x145c456b6`. These are instruction-backed static fields, not live
projection/history captures. Do not transplant the Editor structure or AC7 private offsets.

The native temporal entry is a strong SR candidate because it already owns the required
pre-tonemap inputs and output rectangles. Its output remains upstream of native tonemapping,
bloom/exposure consumers and final scaling. Any replacement must preserve requested half-res
outputs, downstream dependencies and native fallback, not just return a texture of a new size.
The user-visible resolution scale's native policy owner and effective runtime rectangles remain
unidentified. Do not substitute a console-variable timer for that investigation.

These calls construct a render-thread graph. An `FRDGTextureRef` is not a D3D texture handle,
and retaining its pointer does not retain immutable pixels. Future integration must identify
the physical RHI resources at execution, preserve graph/pool leases, and carry frame, view,
family, history and stereo-eye identity through recording and GPU consumption. No such hook,
lease path or output reinsertion exists in the scaffold.

The matched stock shader `TemporalAA/TAAStandalone.usf:1995` computes camera reprojection from
depth and `ClipToPrevClip`. At written pixels it replaces that motion with decoded velocity,
testing encoded X greater than zero before decoding. `VelocityCommon.ush` removes current and
previous projection jitter and uses current-minus-previous NDC. `Common.ush` reserves clear zero
and encodes XY with scale `0.2495` and bias `32767/65535`. These are source conventions to test
against the game's active shader, not measured resource formats or complete object coverage.

The game's matched `PrimitiveHasVelocityForView` retains camera-cut/history and projected-size
eligibility. Consequently native TAA does not establish dense raw vectors for every pixel or
every independently moving object. Captures must distinguish unwritten motion, valid zero,
camera fallback and missing object geometry before selecting backend scales/sentinels.

## HUD producers and composition

Packaged HUD evidence includes `HUDRenderTarget`, referenced by `SceneRenderHUD`, its instance
and `SceneRenderHUDUI`. Its serialized default is 512x512 `RTF_RGBA8`; `VR_CaptureTarget` has a
1920x1080 default. `WingmanWidgetPassThrough` uses `SlateUI` in a translucent unlit material.
These defaults establish potential producers, not current target size, contents or all HUD coverage.

Nativized actor construction includes projection/cockpit HUD, pitch ladder, crosshair widgets,
VR panels and stereo layers; subtitles also have a stereo-layer path. `DrawHUD` resets hitbox
state and dispatches the Blueprint receive-draw event with Canvas size. Its identification does
not establish that every visible game HUD passes through `AHUD`.

The custom `UWingmanWidgetComponent` constructor installs primary vtable RVA `0x82c72c0`.
Stock widget vtable RVA `0x85670a0` and the custom vtable share `UpdateRenderTarget` at slot
`+0x8e0`, `ShouldDrawWidget` at `+0x920` and `DrawWidgetToRenderTarget` at `+0x928`.
Later adjacent pointers include secondary vtables; do not treat them as primary virtual slots.

The native component owns its texture at `+0x520` and material instance at `+0x528`. Draw size
is `+0x488`, current draw size `+0x4a0`, the Slate window shared pointer `+0x550/+0x558`, and
widget renderer pointer `+0x568`. The target's static SizeX/SizeY fields are `+0x180/+0x184`
and clear colour starts at `+0x188`. The allocator updates `SlateUI` and `BackColor`; the draw
producer retains normal dirty, prepass, redraw and tick policies. Reference Editor target offsets
were different, which ruled out directly transplanting symbol-derived layouts.

Game native state machine RVA `0x277a060` retrieves a radar widget target and stores it at
actor `+0x3c0`. Another native state machine, RVA `0x4269cb0`, retrieves that target and binds
it to a material's `SlateUI`, with radar success/failure strings supporting its role. Their exact
actor names and active instances remain unresolved. A larger HUD function at RVA `0x26fe0a0`
also references the getter/material name but exceeded the bounded decompilation timeout; it
was not assigned a speculative class name.

`FWidgetRenderer` paints on the game thread and hands its target context to the main Slate
renderer through virtual slot `+0x170` (`AddWidgetRendererUpdate` in matching source). The
renderer queues execution; return from the widget draw does not prove a completed texture.
The final queued target-draw function and current alpha convention remain unvalidated.

The stereo-layer texture setter writes `+0x200` only on change and marks `+0x294` dirty.
Blueprint exec wrapper RVA `0x7300cd0` calls it. Finding this setter does not identify actual
VR compositor submission, eye routing, layer ordering or a reusable desktop HUD image.

A world-space widget can enter scene colour before TAA and tonemapping, while Canvas/Slate
overlays and VR compositor layers enter later. Therefore a pre-Slate or post-tonemap copy is
not yet a proven completed HUD-less image. Extraction must preserve native depth, material,
glow/grade and alpha behavior and account for every active route.

## Scaffold and next validation

The [plugin](../../games/project-wingman/README.md) exports the existing C ABI and exact-build
detection without renderer state or hooks. Preparation/start return `RSF_ERROR_NOT_READY`;
validated quiesce/stop calls succeed. Status remains unprepared, inactive and not ready.
Candidate RVAs are documented here and in engine metadata, rather than compiled into detours.
The shared runtime's explicit plugin-path loader can inspect it; game selection, carrier and
automatic activation are not implemented. No version/ABI bump or game installation change is needed.

The next bounded experiments should establish:

1. Expected instruction bytes and hook safety at the temporal entry, with main/stereo/capture
   view classification and actual effective render/output rectangles. Missing identity/bytes must refuse.
2. Physical RDG/RHI texture formats, leases and execution lifetime; colour/depth/velocity contents,
   jitter/current-previous matrices, camera cuts and independent moving-object coverage.
3. Desktop versus VR HUD instances and draw order, including cockpit projection, radar, subtitles
   and crosshair, then a complete HUD-less image with a reconstructible native HUD path.
4. Engine-owned SR/fallback reinsertion, half-resolution outputs and native downstream processing
   across scale changes, scene transitions, pause, resize and teardown.

The scaffold's DLL-loading contract tests establish ABI and refusal behavior only. The 3 October
Windows/MSVC Release gate passed 34 executed tests with five skips, plus Rust formatting and lint
checks. The Project Wingman contract test passed. Details are in the
[implementation tracker](../implementation.md).
Runtime rendering, motion quality, performance, FG and VR acceptance remain pending.
