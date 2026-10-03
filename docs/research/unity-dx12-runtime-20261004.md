# Unity Mono DX12 runtime and shared overlay

This increment implements normal drop-in loading, native DX12 SR and shared overlay controls
for Drag'n Wash. Mono/Harmony provides the inspected URP camera and graph hooks. The game
plugin owns those hooks and resource leases; the orchestrator owns vendor contexts and UI.

## Deployment and startup

The build script produces the managed helper, native plugin, runtime, shared shim and Rust
overlay. The deployment script adds `version.dll`, `ReScaleFrame.ini` and `ReScaleFrame/`.
It refuses unowned replacements, backs up previous RSF files and verifies the original-file
baseline. The 4 October deployments preserved all 277 original game files. Paths in the INI
are relative to its directory. No manual injector is needed.

The shared shim dispatches AC7 to its legacy entry and Unity to the Mono lifecycle. Its
`dinput8.dll`, `ReScaleFrame.Loader.dll` and `carriers/version.dll` outputs are identical.
The version alias must stay in a carrier subdirectory: beside test executables it caused
eight fixture crashes through Windows version-library resolution. A separate startup crash
was traced to `__chkstk` in DllMain because automatic path buffers exceeded Unity's loader
thread stack; static executable storage and a MAX_PATH system buffer corrected it.

Actual Mono enumeration found `Unity Root Domain`. Requiring a child domain had refused
startup. Mono P/Invoke also did not call UnityPluginLoad for the preloaded plugin. The guarded
registry fallback uses the matched UnityPlayer PDB's GetInterfaceSplitImpl at RVA `0x74fc80`,
PDB GUID `223bd7a7-393f-40cf-9b0c-20abedf13a2e`, age 1, full player SHA256
`bbdf3e73522223ea45c3a54049b716f9da3e632994636734c5d53326aa2c36d0`, and these bytes:

```
48 83 ec 38 48 89 4c 24 20 48 8d 4c 24 20 48 89 54 24 28 e8 78 ff ff ff
```

That lookup returns public IUnityGraphics and IUnityGraphicsD3D12v7 interfaces. The live log
confirms lookup and DLSS preparation on the game's own DX12 device. Native header reference:
Unity NativeRenderingPlugin `522254181faf188efa8b50c3e3bf6fce720b26e4`, stored outside Git.

## Hook and lifetime corrections

Nine exact managed methods are preflighted. CreateCameraData recomputes dimensions and MSAA
after stacked-camera initialization, so exact vendor extents and single sampling are reapplied
afterward. Temporal AA must be set on copied camera data: changing the component in the later
additional-camera prefix cannot change the earlier AA copy. That mistake allowed provider
preparation without evaluation and is corrected in the next deployed helper.

Projection comes from the private raw field and GL.GetGPUProjectionMatrix. The public no-jitter
GPU compatibility method returns zero in these assemblies. Shader-source motion conventions
are current-minus-previous UV displacement; normalization converts to previous-minus-current
render pixels. Moving-object and custom fluid/decal coverage remain unverified.

The replacement STP graph pass declares its dependencies and queues complete spatial fallback
before its event. Native output is copied back only after successful evaluation. Active
rectangles may be smaller than pooled physical textures. Packet COM leases, allocators and
lists live through Unity's completion fence. Resize and settings changes join prior completion
values before replacing contexts. Unknown completion leaves the adapter owned and inactive.

## Overlay and evidence

The overlay pass follows UniversalRenderer.OnAfterRendering and declares the backbuffer
read/write. A separate Unity event permits queue access and flushes prior commands. The
existing egui renderer runs through D3D11On12 on that same device/queue; SR remains native
DX12. Insert opens DLSS, FSR1/2/3/4, XeSS and the five existing quality modes. Refusal retains
fallback and reports its result. Unity FG and Reflex are not implemented.

The 4 October game log confirms initialization, Insert opening the panel, and vendor/quality
requests preparing FSR2, FSR3, FSR4, XeSS and DLSS at changed extents. This proves attachment
and control delivery, not moving-image quality or successful scene evaluation.

MSVC Release and managed builds pass. Shipped-Mono fixtures initially passed eight metadata contracts,
packet layout and Harmony patch/unpatch. Rust workspace tests pass (55), format and Clippy
pass. The corrected full VS2026 Release gate passes: 37 native tests, six opt-in skips,
Rust format and Clippy. The earlier eight crashes are resolved by relocating the alias.

The AA-enabled run exposed another refusal: the live explicit input extent was 1706x1066,
one pixel smaller than DLSS's 1707x1067 plan. The early guard fell through to Unity's original
STP, which raised NullReferenceException in STP.Execute. The replacement now keeps a spatial
copy when temporal resources are unavailable. A bounded live resource log then identified
the dimension truncation and depth format: scene format 26, depth format 20 (D32_FLOAT_S8X24),
motion format 34, output format 10, all single-sample. The render-scale scalar now rounds
allocations upward to cover both planned axes, with exact camera extents restored after
adaptive performance. The native normalizer accepts the depth/stencil resource and creates
its depth-only R32_FLOAT_X8X24 shader view. These corrections are built and redeployed;
The next normal Steam launch confirms DLSS SR result 0 for frames 5, 6 and 7 on view 15334,
with accepted=3 and refused=0 at 1707x1067 -> 2560x1600. The process remained running and
Player.log contained no new render exception. This establishes actual game evaluation and
output reinsertion submission, not visual acceptance. The full Release gate passes again
after the depth-view change (37 tests, six opt-in skips, format and Clippy).

Earlier standalone DX12 GPU readback fixtures passed DLSS, FSR2 2.3.4, FSR3 3.1.5,
FSR4 4.1.1 INT8 and XeSS 2.0.2 on RTX 4070 Laptop with the debug layer. FSR/XeSS also passed
on Intel. Stationary synthetic inputs do not establish game image acceptance. FSR1 uses URP's
spatial path. Plugin rendering_ready remains zero pending visual and scene-transition acceptance.

## Distant-object shimmer, 4 October

The user reports FSR2, FSR3 and XeSS shimmering and jittering on distant geometry before
settling. Successful dispatch counters alone did not establish correct temporal inputs.
Inspection found that Unity's CalculateJitterMatrix produces a positive Y projection
translation from its jitter value, while RSF forwarded that value unchanged to every provider.
AMD's [Unity URP integration](https://github.com/GPUOpen-Effects/FidelityFX-FSR2-Unity-URP/blob/main/src/patch/0001-Added-FSR2-support-for-URP.patch)
uses a negative Y translation for its API jitter, consistent with the
[FSR2 projection example](https://gpuopen.com/manuals/fidelityfx_sdk/reference_documentation/sdk/effect_components/fidelityfx_fsr2/ffx_fsr2/).
Intel's [XeSS guide](https://github.com/intel/xess/blob/main/doc/xess_sr_developer_guide_english.md#jitter)
likewise subtracts the API Y jitter when modifying projection.

The Unity native DX12 SR adapter now negates the forwarded Y jitter for its FSR/XeSS sessions;
the existing DLSS path is unchanged. This changes metadata at evaluation, not raster jitter,
motion vectors, frame identity or resource lifetime. Logs now include producer history-reset
flags and projection jitter for the first evaluations and sparse 600-frame checkpoints.
The hypothesis is that correcting this mismatch improves stationary temporal convergence.
Reject that hypothesis if matched distant geometry still shakes after history settles;
then inspect raw motion/depth and moving-object coverage. Unity STP.Jit16's name does not
prove a 16-frame sequence: its inspected implementation evaluates Halton at the supplied index.
No unsupported sequence-length claim or sequence replacement was made.

MSVC Release verification passes after this correction: 37 native tests, six opt-in skips,
format and Clippy. The correction is deployed alongside the game, preserving all 277 original
files. A new user-run visual comparison remains pending. A successful
SDK call or stationary synthetic fixture does not prove the reported image defect is fixed.

### Recording rejects the first sign hypothesis

The supplied `Aufzeichnung 2026-10-04 002551.mp4` is 10.633 seconds, 566x572 at 30 fps.
The user confirms the skyline is static. Extracted frames show building grids repeatedly
losing/recovering definition while the overlaid artwork stays fixed. No backend selection is
visible in the crop, so it cannot independently identify the provider or quality mode.
The associated log exercises XeSS modes, reports successful evaluations, and shows reset=0
at the sampled settled checkpoints. This rejects the prior correction as a complete fix.

The earlier interpretation compared Unity's pre-GPU translation with vendor projection
examples and omitted GL.GetGPUProjectionMatrix's Y conversion. The replacement derives
`rasterProjection * inverse(noJitterProjection)` after that conversion and reports its X
translation times width/2 and negative Y translation times height/2. It removes the extra
FSR/XeSS-specific sign flip. Live logging confirms measured raster jitter matches the
original projection jitter in this player. The first sign-change explanation above is retained
as a failed approach, not an accepted diagnosis.

The native normalizer now reads back four 8x8 motion regions during the first 12 evaluations
of each context. Each command slot has its own 8 KiB READBACK buffer. Mapping occurs only
after the caller has joined that slot's previous Unity completion value; copies temporarily
transition normalized motion to COPY_SOURCE and restore it before vendor evaluation. Capture
stops after 12 frames and pending samples drain on subsequent slot reuse. No CPU fence wait
or permanent per-frame readback was added.

The next live DX12 run reports mean=0, max=0 render pixels and invalid=0 for every sampled
motion region in both DLSS and XeSS, including XeSS Ultra Performance 854x534. The same
XeSS context reaches accepted=1200/refused=0 with reset=0. These bounded samples support
the static-motion premise; they do not establish every object's coverage or whole-image
convergence. GPU jitter correction and diagnostics are deployed, all 277 original files
remain unchanged, and Release verification passes (37 tests, six skips, format and Clippy).
Visual resolution of the skyline defect remains pending user comparison.

### Correct the post-processing boundary

The user identifies a center-to-edge stabilization pattern and asks about DoF or blur.
The shipped RenderPostProcessingRenderGraph records DoF before its temporal AA/STP block;
motion blur follows that block. Our original STP replacement was therefore pre-tonemap/UI,
but already after DoF. That boundary was wrong for this integration: pre-tonemap alone did
not establish a clean SR input. The invoked Streamline DLSS-SR skill and Intel's guide require
the reconstructed scene to feed subsequent output-resolution post-processing.

The existing RenderPostProcessingRenderGraph prefix now reconstructs its incoming HDR scene
and replaces that argument before the original method records any post-processing effects.
It updates the camera target extent to output size through URP's owned resolution helper.
The later STP prefix passes that already reconstructed texture through without a second
evaluation, using camera/frame identity. FSR1 retains its original spatial path. DoF, motion
blur, bloom and tone mapping remain enabled after temporal SR; this is an ordering correction,
not an effect-disable workaround. Source inspection confirms the post target is the native
backbuffer or URP's STP output-size cameraColor allocation. HUD and RSF overlay remain later.

The source increment builds and is deployed, with all 277 original files unchanged. The actual
shipped Mono fixture passes the nine managed contracts and Harmony cleanup. Native validation
from the immediately preceding diagnostic increment passes, including FSR2/XeSS debug-layer
GPU readback. No matched native/bilinear capture or MSE is available; visual acceptance,
active DoF/blur confirmation and output-edge inspection remain pending the new game test.

The next game run confirms `before post-processing; DoF=True motionBlur=False`, valid depth
and motion, and DLSS result 0 at 1707x1067 -> 2560x1600. Live quality changes also prepare
XeSS at 1506x942 and evaluate successfully. No new Player.log render exception was found.
This confirms the active DoF dependency and new execution boundary, not pixel acceptance.
Build command: `eng/build-unity-sr.ps1 -VS2026 -UnityManagedDirectory <player Managed>`.
Run normally through Steam with RSF's enabled control on for SR or off for the original path.
The deployment INI defaults to backend 1, quality 1; the overlay controls requests in process.
Final backbuffer dimensions are 2560x1600. Disabled/native and bilinear matched captures,
regional output-edge comparisons, MSE, moving/skinned coverage and resize acceptance are
still unavailable; fallback is the queued spatial copy if a provider refuses evaluation.

## Packaging and automatic startup selection

The user confirms the ordering correction is much better. The complete Claw test ZIP includes
the shared carrier, native/managed plugin, runtime, Rust overlay, all vendor DLLs/notices,
relative configuration and checksums. Every ZIP entry is reopened and SHA256-compared against
its staged payload. Original game binaries, Unity assemblies, logs and deployment baselines
are explicitly excluded. Device acceptance on MSI Claw 8 A2VM remains pending.

Auto is now the deployment/package default (`Backend=7`). Startup keeps the game at native
resolution until a render callback supplies the actual D3D12 device. Its adapter LUID is
resolved through DXGI, rather than choosing the first adapter in a hybrid-GPU machine.
The shared runtime policy selects NVIDIA DLSS, Intel XeSS and AMD FSR3. AMD selection is
conservative for unknown/newer architectures too; RDNA4 automatic FSR4 classification is
not implemented, and experimental FSR4 remains a manual choice. Software adapters disable
SR. Adapter identification failure refuses automatic activation with a reason. Manual
overlay backend selection overrides Auto for the current process; startup retries Auto on
the next normal launch. Vendor preference is distinct from successful feature support.

Commit preparation also identified a carrier export regression: version exports occupied
ordinal 1. The shared carrier now preserves DirectInput8Create at ordinal 1, while Windows
version functions forward by name. Unity uses those named imports. A new fixture verifies
the DirectInput ordinal, all 17 named version exports and actual Windows version-resource
calls. The clean staged source is checked independently of concurrent AC7/Project Wingman
changes; appended overlay ABI features are guarded so the Unity code also builds against
the repository's existing shared panel ABI.

Packaging after the Auto increment takes the verified Release first-party binaries and the
hash-verified deployed vendor payload. This avoids modifying a running game installation;
the manifest records that source distinction. The workspace gate passes 39 native tests
with six opt-in skips, format and Clippy. The isolated staged snapshot's Rust tests pass
53 tests (the baseline overlay plus the scoped FSR1/refusal controls), independent of the
concurrent AC7 overlay extensions. Auto selection has vendor-policy fixture evidence;
its MSI Claw startup and GPU quality acceptance remain pending.

The final isolated staged snapshot passes VS2026 Release verification: 37 native tests,
five opt-in skips, format and Clippy, plus 53 Rust workspace tests. The full workspace with
concurrent work passes 39 native tests and six skips. The shared shim's non-null DirectInput
ordinal assertion was strengthened afterward and its focused forwarding/policy checks pass.
