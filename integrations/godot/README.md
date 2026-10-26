# ReScaleFrame for Godot

Developer integration for Godot 4.7.2, Forward+ and Windows x64. Direct3D 12 has
SR/FG/PCL implementations. Vulkan has native DLSS/FSR2/FSR3/XeSS SR, FSR4 SR
through a same-adapter DX12 handoff, native FSR3/DLSS frame generation and a XeSS
DX12 presenter, plus PCL/Reflex.
It combines a GDExtension add-on with a small module and guarded patches against
the pinned engine source. Both standard Godot and Godot .NET use the same native
runtime. Official GodotSharp assemblies remain unchanged.

This is an implementation in progress. The standard DX12 runtime has exercised
DLSS, FSR3 and XeSS SR, FSR3/XeSS/DLSS frame generation, DLSS MFG, and PCL/Reflex.
The visual harness now captures actual FSR-generated images and checks temporal
inputs against scene geometry. It does not establish broad image-quality,
physical scanout or latency acceptance. Vulkan FSR2/FSR3 SR now has real GPU
readback and validation-layer coverage on both editions. Vulkan FSR3 FG now runs
with SR Off, provider switching, resize and a secondary native window. Vulkan
PCL/Reflex also runs with native presentation and no camera or FG owner.
XeSS SR passes six standard/.NET generated-image cases with FSR3 FG and PCL.
DLSS FG passes standard/.NET device and midpoint image checks, including DLSS SR
and FG together. Higher-count MFG3 also passes strict GPU validation and quarter-step image
comparisons for all three actual output ordinals. XeSS FG passes standard/.NET lifecycle/C#/PCL checks and
twelve object/camera/combined image cases using 48 actual SDK outputs. FSR4
SR passes both editors' switching/resize checks, C# on .NET and a combined-motion
image comparison plus four complete raw SR frames per edition. Vulkan FSR4 FG routing
builds with exact-provider admission; the current NVIDIA provider list is correctly
refused. AMD execution and generated-image acceptance remain untested. Strict DLSS
lifecycle runs now pass API checks but expose a shutdown crash still under investigation.
Vulkan DLSS SR passes standard/.NET switching, resize, raw input checks and six
DLSS SR + FSR3 FG + PCL image cases, covering 24 actual generated frames.
Standard/.NET Debug and Release exported players have passed the
runtime and lifecycle fixtures, including C# on .NET. A GDScript-only .NET
Release export also passed.
Updated Vulkan Debug/Release exports also pass standard/.NET SR, FG and
camera-free PCL fixtures. SR/FG include C# on .NET. The exported SR fixture saves
actual player images and raw inputs; midpoint-image acceptance remains the
bounded editor suite above.
Fresh Release exports pass FSR4 SR switching/return/resize/raw readbacks on both
editions. Debug exports pass XeSS FG, Off/resume, resize and PCL on both editions,
including C# on .NET. These checks do not establish exported generated-image quality.
See [research and corrections](../../docs/research/godot-engine-integration.md).
The expanded 30-case image suite currently fails four FSR2/FSR3 independent-object
cases, with up to 0.62 output-pixel displacement, despite passing vector checks.
These failures remain visible in the reports; all-provider image acceptance is
unfinished.
The first Vulkan 18-case image matrix also retains FSR2 failures and a teardown
race caught by validation. The presenter retirement correction has new functional
coverage and a passing 12-case SR Off/FSR3 image suite; the original failed
evidence remains available. Process-only controls isolate the present hazard
to RTSS hook injection: official/custom Godot controls have no present hazards
when that module is excluded, without changing global RTSS settings. This is
bounded isolation evidence; other hooks and scenes retain their own validation.
Cold startup FFX waits remain recorded separately from the passing warm image
matrix. No physical-scanout, performance or latency reduction follows from it.

## Build

Run these commands from a Developer PowerShell with CMake on PATH. Preparation
uses installed uv/Python 3.14 and provisions the pinned SDKs and Mesa NIR library.
MSVC and the Windows SDK are described in [tooling](../../docs/tooling.md). Reference checkouts,
engine binaries, vendor binaries and captures remain ignored.

```powershell
./eng/prepare-godot.ps1
./eng/build-godot-vulkan.ps1 # Native Vulkan SR/FG/PCL payloads
./eng/build-godot.ps1 -VS2026 -StageRuntime
./eng/build-godot-engine.ps1
./eng/build-godot-engine.ps1 -Mono
```

The add-on is staged under `.local/godot/package/addons/rescaleframe` and custom
editors under `.local/godot/engines/{standard,mono}`. Source revisions and official
engine archive hashes are pinned in [dependencies.json](dependencies.json).
`eng/patch-godot.py` refuses unexpected revisions or unrelated source changes.
The scripts verify downloaded archives, reference revisions and copied binaries.
Build manifests select immutable engine paths beneath `builds/<hash>/`.
The Vulkan builder uses the pinned Godot Vulkan headers, installed Windows
Vulkan loader and Visual Studio tools. It builds a local import library and
the corrected pinned FidelityFX and Streamline SDKs without installing a global
Vulkan SDK. The corrected Streamline DLLs use a private `streamline-vulkan`
directory, while D3D12 retains its retail SDK payload.
`-StageOnly -StageRuntime` on `build-godot.ps1` repackages already built binaries.
The visual harness also accepts `-Geometry billboard` for a thin camera-facing
geometry control. Its results are diagnostic and do not replace the default box
suite. The analyzer rejects a reference with a different recorded geometry.

For Vulkan SR readbacks on standard and .NET editors:

```powershell
./eng/test-godot.ps1 -EngineBridge -GraphicsApi vulkan -Scenario sr -CaptureFrames
./eng/test-godot.ps1 -EngineBridge -GraphicsApi vulkan -Scenario sr -CaptureFrames -SrBackend 3
./eng/test-godot.ps1 -EngineBridge -GraphicsApi vulkan -Scenario fg -GpuValidation -CaptureFrames
./eng/test-godot.ps1 -EngineBridge -GraphicsApi vulkan -Scenario pcl -NoCSharp -GpuValidation
./eng/test-godot.ps1 -EngineBridge -GraphicsApi vulkan -Scenario pcl -NoCSharp -GpuValidation -NativeVulkanPresentation
./eng/test-godot-visual.ps1 -GraphicsApi vulkan -GpuValidation -ReferenceMsaa -LatencyMode 1
```

`-GpuValidation` additionally requires an available Khronos validation layer.
The harness verifies that the layer DLL actually loaded and records its hash.
Foreground fixtures share a named Windows mutex to prevent concurrent
tests from stealing focus. `-NativeVulkanPresentation` verifies PCL without an
FG owner; it is a test control for the camera-free fixture.
For a portable layer, set `VK_LAYER_PATH` to its manifest directory and
`VK_INSTANCE_LAYERS=VK_LAYER_KHRONOS_validation` in the test process environment.
Validation errors and storage-format warnings fail the run. Raw images flush
after GPU-drained context retirement, with Vulkan formats, frame/view/history
identity, provider ID and camera metadata. Each run includes actual player PNGs,
raw SR input/output images and `frames/decoded/report.html`. Geometry-derived
motion and jitter are checked independently. This SR readback suite has no
independent SR image-quality reference and makes no Vulkan FG or latency claim.

## Project setup

Copy the staged `addons/rescaleframe` directory into the project and enable
ReScaleFrame in Project Settings > Plugins. This registers settings, generates
a persistent project UUID and installs the export plugin. In
`project.godot`, enable the native presentation bridge before engine startup:

```ini
[rescaleframe]
enabled=true
```

Use the custom engine, Forward+ and the `d3d12` or `vulkan` rendering driver,
within the recorded provider acceptance above. Attach a
`ReScaleFrameEffect` resource to the active Camera3D or WorldEnvironment
compositor. This requests actual object and camera motion-vector production.
The editor add-on's Tools menu can attach the resource to a selected node.
An official unmodified engine can run the renderer probe but cannot activate the
SR/FG presentation bridge.

For Inspector controls, add a Node with the `ReScaleFrameController` script,
assign a `ReScaleFrameSettings` resource and set `compositor_target` to the
Camera3D or WorldEnvironment. The controller attaches its motion effect, applies
the settings and provides status/reset/ping methods. Call `apply_settings()`
after changing resource fields in code. Only one controller owns the root
viewport. Its removal disables the effects and removes its own compositor
resource. PCL with SR/FG Off does not require a camera or compositor.

Configure the primary viewport from GDScript:

```gdscript
var runtime := ClassDB.instantiate("ReScaleFrameRuntime") as RefCounted
var accepted: bool = runtime.call("configure", {
    "sr_backend": 1, "quality": 1,
    "fg_backend": 3, "fg_mode": 1, "generated_frames": 1,
    "latency_mode": 1
}, get_viewport().get_viewport_rid())
var status: Dictionary = runtime.call("get_status")
# Call after a camera cut or another application-defined history discontinuity.
runtime.call("reset_history")
```

The C# facade in `addons/rescaleframe/csharp/ReScaleFrame.cs` exposes
`GodotIntegration.CreateEffect()`, `Configure(options, viewport)`,
`GetRuntimeStatus()` and `ResetHistory()`. It constructs the GDExtension resource
instead of requesting a managed wrapper for an engine-private singleton.
The [C# fixture](demo/mono/MonoSmoke.cs) exercises that path.

SR values are Off=0, DLSS=1, FSR2=2, FSR3=3, FSR4=4, XeSS=5, FSR1=6 and Auto=7.
FG values are DLSS=1, FSR3=3, FSR4=4, XeSS=5 and Auto=7. FG mode is Off=0 or
Fixed=1. Quality is Native=0, Quality=1, Balanced=2, Performance=3,
Ultra Performance=4 or Ultra Quality=5 where the selected provider supports it.
Latency mode is Off=0, On=1 or Boost=2. Inspect actual status and provider
capabilities; accepting configuration does not mean a provider executed.

The bridge selects one explicit primary viewport. XR, multiple concurrent
presentation surfaces, Mobile/Compatibility, non-Windows platforms and HDR
output require separate work. SR Off restores native resolution; FG and PCL
remain independent of SR. Native temporal upscaling is currently evaluated before
the external SR pass as a fallback, with an extra pass cost.

## Test and inspect generated images

The aggregate Vulkan runner indexes actual player PNGs, raw/generated-image
reports and failed cases in one HTML report. For example:

```powershell
./eng/test-godot-vulkan-matrix.ps1 -Target editor -Scenario sr,pcl -SrBackend 1,4,5 -ValidationLayerDirectory $env:VK_LAYER_PATH
```

Its defaults cover both editions, editors/exports and separate generated-image
comparisons. This new runner has a preserved failing FSR1 control; its full
matrix has not yet run. Check the [restart checkpoint](../../docs/implementation.md)
before interpreting the latest builds as accepted.

```powershell
./eng/test-godot.ps1 -EngineBridge -Edition standard,mono -GraphicsApi d3d12
./eng/test-godot.ps1 -EngineBridge -Scenario controller
./eng/test-godot.ps1 -EngineBridge -Scenario pcl -NoCSharp
./eng/test-godot.ps1 -EngineBridge -Scenario sr -GraphicsApi vulkan -SrBackend 1 -CaptureFrames
./eng/test-godot.ps1 -EngineBridge -Scenario sr -GraphicsApi vulkan -NoCSharp -MissingSrRuntime
./eng/test-godot-visual.ps1
./eng/verify.ps1 -Configuration Release -VS2026
```

The functional test exercises provider switching, resize, MFG and PCL/input
markers. DLSS-G requires the actual Windows foreground window; a cached Godot
focus flag does not prove eligibility. The visual test runs object-only,
camera-only and combined motion on standard and .NET engines, with C# facade
validation on .NET. Its default SR matrix includes Off, DLSS, FSR2, FSR3 and
XeSS. `-SrBackend` selects a smaller provider set for diagnosis; it does not
turn an unsupported or failing provider into a pass.

The missing-runtime fixture removes the Vulkan SR DLL from its private copy and
checks actual player images through provider switches and resize. SR status
retains the provider failure while the runtime records a spatial fallback.
The SR fixture accepts initial DLSS=1, FSR2=2 or FSR3=3. The DLSS case additionally
requests model K before switching to FSR3. A model request can be overridden by
the driver; this check establishes accepted configuration and continued rendering.

For exported-player validation, build both target configurations, with and
without `-Mono`, then use the same functional scenarios:

```powershell
./eng/build-godot-engine.ps1 -Target template_debug
./eng/build-godot-engine.ps1 -Target template_release
./eng/build-godot-engine.ps1 -Target template_debug -Mono
./eng/build-godot-engine.ps1 -Target template_release -Mono
./eng/test-godot.ps1 -EngineBridge -Exported -ExportConfiguration release
./eng/test-godot.ps1 -EngineBridge -Exported -ExportConfiguration debug -Scenario controller
./eng/test-godot.ps1 -EngineBridge -Exported -Edition mono -NoCSharp
```

The export test checks dependency hashes and launches the exported executable
without a project path. The .NET C# fixture uses its matching official GodotSharp
assemblies and local NuGet packages.

Each visual case runs an animated FG scene and a separate no-FG scene at half
animation steps. Bounded native GPU readbacks save actual generated outputs,
post-tonemap HUD-less colour, normalized depth and normalized motion. JSON
metadata records frame/view/generation identity, camera matrices and pixel units.
`capture-info.json` fingerprints the engine and native DLL. The harness rejects
missing generated frames, bad motion direction/scale, bad camera metadata and
large midpoint errors. It writes decoded PNGs, comparison sheets and
`decoded/analysis.json` under `.local/godot/visual`. Open those images when
assessing artifacts; presentation counters alone cannot validate interpolation.
Each case also writes `decoded/report.html`, and a completed matrix writes an
HTML index next to its JSON manifest. Failed image cases retain their reports
and make the overall suite fail after collecting the other independent cases.

`eng/capture-godot.ps1 -RawProbe` is a separate diagnostic. Its GPU readback forces
submission and can conceal a scheduling defect, so the regression analyzer
rejects such a run as ordinary-path evidence. All capture runs introduce GPU
waits and are unsuitable for performance or latency measurement.
