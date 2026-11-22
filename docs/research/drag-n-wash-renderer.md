# Drag'n Wash renderer investigation

Examined on 3 October 2026. This is ReScaleFrame's third game research case and first Unity game.
The intended implementation is a shared [Unity Mono plugin](unity-mono-plugin.md) using reflection
and Harmony, with Drag'n Wash as its first URP RenderGraph validation case.

## Question, method and limits

Identify the shipped engine/backend, managed renderer owners, temporal input production and
scene/UI boundaries before choosing hooks. Mono assemblies are readable and the player ships
native PDBs, so read-only PE/PDB inspection, complete ILSpy projects and serialized settings were
more direct than executable signature scans or Present-time texture classification.

No game was launched, injected, modified or captured. No Unity plugin or Harmony patch was built
or executed. Source-derived contracts below are leads for runtime work, not rendering support.
The [engine summary](../../games/drag-n-wash/engine.json) and
[structured static evidence](evidence/drag-n-wash-static-20261003.json) carry the same limits.

## Exact build

| Item | Inspected value |
| --- | --- |
| Steam app / build | `4739660` / `25286774`, from local Steam manifest |
| Executable | `DragNWash.exe`, 667,648 bytes, PE x64 `0x8664` |
| Executable SHA256 | `5fdfffe386a2f43b77626cd3d70554d84c6588c94d309544924d6fab088ddafc` |
| Player | `UnityPlayer.dll`, 36,340,136 bytes |
| Player SHA256 | `bbdf3e73522223ea45c3a54049b716f9da3e632994636734c5d53326aa2c36d0` |
| Unity | `6000.3.14f1 (d68c3f99a318)`, player version resource; serialized BuildSettings agrees |
| Scripting | Mono: managed game assemblies and `mono-2.0-bdwgc.dll`; player PDB name agrees |
| Renderer | Serialized `PC_RPAsset` selects URP `PC_Renderer`, Forward+ enum value `2` |
| Packaged API order | BuildSettings `[18, 2]`, matched to shipped `GraphicsDeviceType`: D3D12, D3D11 |
| URP package identity | Embedded PackageCache ID `3b809f23691d` |
| Core package identity | Embedded PackageCache ID `02dfbe8e43de` |

The URP/Core assembly versions are `0.0.0.0`. The embedded PackageCache IDs are recorded as
identifiers, not asserted Git revisions or package semantic versions. No external Graphics
checkout was used to establish these method contracts. The shipped decompiles are the reference.

`UnityPlayer.dll` imports `D3D11CreateDevice`, `D3D11On12CreateDevice` and DXGI factories.
These imports do not determine the game's selected API. The packaged D3D12 directory likewise
does not prove D3D12 execution. A live player log/device observation is still required.

The executable's CodeView GUID is `2964b514-c5f1-4838-b709-967f3017719c`, age 1; its shipped
`WindowsPlayer_player_Master_mono_x64.pdb` information stream matches both. The player's GUID
is `223bd7a7-393f-40cf-9b0c-20abedf13a2e`, age 1; its shipped
`UnityPlayer_Win64_player_mono_x64.pdb` matches. This verifies pairing, not symbol coverage or
native hook sites. The PDBs were not imported into Ghidra in this investigation.

Mono exports include `mono_domain_get`, `mono_get_root_domain`, `mono_domain_assembly_open`,
`mono_assembly_get_image`, `mono_class_from_name`, `mono_class_get_method_from_name`,
`mono_runtime_invoke`, thread attach/detach and assembly enumeration. They support investigating
a bootstrap into the existing runtime. They were not called against a running game.

## Local reference material and reproduction

The complete ILSpy project outputs live under ignored `references/drag-n-wash/decompiled/`.
Twelve assembly projects contain 2,327 C# files:

- `Assembly-CSharp`, `GatorDragonGamesExtensions`, `Naelstrof.UnityScriptableSettings`.
- URP Runtime, Config Runtime and 2D Runtime; Core Runtime and Core Runtime Shared.
- `UnityEngine.CoreModule`, `UnityEngine.UIModule`.
- `com.naelstrof-raliv.fluid-rendering-for-games`, `com.naelstrof.skinnedmeshdecals`.

This is complete decompilation of those assemblies, not a decompile of every framework library
in the installation. `references/drag-n-wash/decompile-manifest.json` records their original
hashes and output file counts. Published evidence records hashes/counts, not decompiled code.

Run the PE inspector separately for the executable and player, creating the output directory:

```powershell
New-Item -ItemType Directory -Force .local/drag-n-wash | Out-Null
python tools/inspect-game.py '<game directory>/DragNWash.exe' .local/drag-n-wash/executable.json
python tools/inspect-game.py '<game directory>/UnityPlayer.dll' .local/drag-n-wash/player.json
ilspycmd --disable-updatecheck -p -r '<game directory>/DragNWash_Data/Managed' -o references/drag-n-wash/decompiled/Assembly-CSharp '<game directory>/DragNWash_Data/Managed/Assembly-CSharp.dll'
```

ILSpy version was `11.0.0.9375`. The read-only local helpers
`.local/drag-n-wash/scan_assets.py`, `native_metadata.py` and `export_evidence.py` retain the
asset scan, PE export/CodeView/MSF7 PDB pairing and publication whitelist. They are local
investigation artifacts. Asset parsing used isolated Python 3.12.14, UnityPy 1.25.4 and
TypeTreeGeneratorAPI 0.0.10. No assets or assembly files were saved back to the game.

## Serialized policy and corrections

`globalgamemanagers` GraphicsSettings and its sole PC quality tier both reference
`globalgamemanagers.assets` object 2288, `PC_RPAsset`. It selects renderer object 2289,
`PC_Renderer`. Its defaults are HDR scene colour enabled, MSAA 4, automatic upscaling,
render scale `0.9700000286102295`, opaque texture requested and depth texture not globally
requested. `boot.config` disables HDR display output; HDR scene radiance and display HDR are
different contracts.

The apparent 97% scale is corrected by inspecting
`UniversalRenderPipeline.InitializeStackedCameraData`: values within 0.05 of 1 become 1 before
camera sizing. For these defaults the source-derived camera scale is 1.0 before any later
adaptive-performance adjustment. The scale field alone does not prove reduced-resolution
rendering. Runtime effective scale remains unmeasured.

The seven packaged scenes have one serialized camera each, HDR/MSAA allowed and hardware
dynamic resolution disabled. Their additional camera data selects base cameras, empty camera
stacks, default renderer and AA None. Post-processing is enabled on six; `level2` disables it.
Six scenes also contain `CameraSettingsListener`, linked to a settings asset. Its enable and
settings-change callbacks select None with MSAA disabled for zero, otherwise SMAA with MSAA
allowed. They never select TAA. Effective saved settings were not inspected.

The shipped `RenderGraphSettings.enableRenderCompatibilityMode` getter returns false; its
setter is inert/obsolete in this compiled variant. Thus the decompiled player path supplies a
RenderGraph adapter target. Do not assume an older URP compatibility-mode recorder is present.

The renderer contains active SSAO, decal and custom fluid renderer features. The fluid feature
enqueues colour, height and blit passes at `AfterRenderingTransparents`, replacing camera colour
with a newly recorded graph texture. An opaque-only SR tap would miss that later composition.
Preserve the custom passes before the reconstruction boundary and verify their motion coverage.

## Temporal producers and reconstruction

`UniversalRenderer.GetRenderPassInputs` requests motion for a pass declaring
`ScriptableRenderPassInput.Motion`, enabled TAA or CameraAndObjects motion blur. Motion in turn
requests depth. Bundled code and shader references do not prove either pass runs with the game's
default SMAA/MSAA policy. The temporal post-process shader reference is non-null in packaged
`PostProcessData`, object 2292; its executed variants still require capture.

`MotionVectorRenderPass` records full-screen camera motion followed by opaque renderer-list
object motion using the `MotionVectors` LightMode tag and previous-object data. Its producer
allocates `_MotionVectorTexture` as `R16G16_SFloat`, with a separate depth attachment. The clear
is black. Raw direction, units, jitter removal, zero-vector validity and actual deformation,
particle, fluid, decal and transparent coverage have not been verified from shipped shaders or
live textures. Do not import UE4's packed velocity convention.

`UniversalCameraData.IsTemporalAAEnabled` requires requested TAA, post-processing, allocated
history, a single-sample target, a base camera without a stack, no hardware dynamic resolution
and renderer motion support. This renderer's `SupportsMotionVectors()` returns true. These
conditions are incompatible with simply leaving the packaged MSAA path intact and claiming
TAA/STP support. Preparation must establish the matching resources and camera policy.

TAA uses frame-indexed Halton(2,3) jitter centred on zero, scaled by its jitter setting.
`TemporalAA.CalculateJitterMatrix` converts render-pixel offsets with `2/width`, `2/height`
into projection translation. The STP path has a separate negative `STP.Jit16` sequence and
disables the same scaling rule. Capture the actual selected sequence and matrices; do not
mix STP jitter with TAA state or an independently modified camera projection.

`PostProcessPassRenderGraph.RenderPostProcessingRenderGraph` records optional NaN cleanup,
SMAA and depth of field, then TAA/STP, followed by motion blur/Panini, bloom, lens flares and
the native uber post-process colour/tonemap pass. `RenderTemporalAA` receives source colour,
camera depth and motion. `RenderSTP` receives colour, resolved depth and motion, creates a
linear output-resolution UAV and updates camera resolution for later passes. These are the
first candidate SR recorder boundaries. Preserve native downstream processing and fallback.

CPU graph recording is earlier than resource materialization/render execution. The helper must
record dependencies and invoke the native orchestrator inside an ordered execution pass, with
matching camera/view/history identity. No resource handle, lifetime, API state or fence contract
has yet been verified live. [Shared plugin design](unity-mono-plugin.md) records those obligations.

## UI and frame-generation boundary

All 21 successfully decoded serialized canvases use `ScreenSpaceOverlay` enum value 0. This
supports investigating a common output-resolution UI boundary, but runtime-instantiated UI and
custom world geometry remain unmeasured.

In `UniversalRenderer.OnAfterRendering`, post-processing/final blit and custom `AfterRendering`
passes precede `DrawScreenSpaceUIPass.RenderOverlay` for the last base camera with SDR output.
The scene surface immediately before that overlay pass is a candidate completed HUD-less image.
It must be retained before later writes. HDR instead has offscreen UI and later composition;
the SDR boundary cannot be generalized to that path without an adapter and validation.

The game's serialized UI does not establish a separate alpha surface, complete HUD classification
or FG readiness. The shared runtime still owns FG/presentation and latency. Neither higher
presentation counts nor managed patch success would establish latency improvement.

## Failed approaches and uncertainty

The shell's `python` resolved to Inkscape's interpreter, which lacked pip and produced an
unexpected venv layout. Asset tools instead used the app's bundled Python in an isolated uv venv.
ILSpy was already installed and was used without reinstalling or updating it.

Initial UnityPy reads could not reconstruct stripped MonoBehaviour schemas. Loading the shipped
managed assemblies into TypeTreeGeneratorAPI recovered the selected URP/camera payloads.
Loading all assemblies initially failed on duplicate `System.Memory.dll` assembly identity;
`Yarn.*` framework duplicates were excluded from schema input. No game file was changed.

The final scan decoded 2,967 selected objects and retained 563 failed reads. PlayerSettings and
UniversalRenderPipelineGlobalSettings did not decode fully. Most other failures concern custom
types, lights and decals. Failed/partial objects do not support field claims. Generated header
script pointers were unsuitable for identity, so class identity was resolved independently from
MonoBehaviour headers and their MonoScript references. Published snapshots whitelist successfully
read payload fields and omit unrelated/authentication data. No exact URP package semantic version
or global settings contents were inferred from failed parses.

## Runtime use and next experiments

Current runtime use: the shared Unity Mono native scaffold recognizes this exact Drag'n Wash
fingerprint and exposes an inactive lifecycle. It retains no services and refuses preparation.
There is no managed bootstrap, Unity renderer implementation, deployment route, Harmony
dependency or successful game/device test.

The scaffold builds on Windows/MSVC and its synthetic DLL contract passes. That test does not
load the game, Mono, Harmony, URP or a graphics device.

First load an inert helper into the actual Mono domain, establish the live API/pipeline/camera,
and count the proposed Harmony hook executions. Then prove a native copy-through graph pass
before SR, including resize, scene/camera changes, settings overwrite and disable/cleanup.
Capture raw colour/depth/motion and camera/jitter data at consumption, retaining current frame,
camera/view identity and resource lifetime. Verify custom fluid/deformation motion and the final
SDR overlay boundary before assigning support. The shared adapter should refuse unsupported
method/resource contracts and retain native rendering.

Focused documentation checks validate JSON consistency, fingerprints, local Markdown references,
ignored reference paths and whitespace. They do not substitute for build, injected game,
rendering-device or visual acceptance checks. No native/SDK/Rust change requires a full gate here.
