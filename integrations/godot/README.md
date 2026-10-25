# ReScaleFrame for Godot

Developer integration for Godot 4.7.2, Forward+, Windows x64 and Direct3D 12.
It combines a GDExtension add-on with a small module and guarded patches against
the pinned engine source. Both standard Godot and Godot .NET use the same native
runtime. Official GodotSharp assemblies remain unchanged.

This is an implementation in progress. The standard DX12 runtime has exercised
DLSS, FSR3 and XeSS SR, FSR3/XeSS/DLSS frame generation, DLSS MFG, and PCL/Reflex.
The visual harness now captures actual FSR-generated images and checks temporal
inputs against scene geometry. It does not establish broad image-quality,
physical scanout or latency acceptance. Vulkan currently has renderer-probe
coverage only. Standard/.NET Debug and Release exported players have passed the
runtime and lifecycle fixtures, including C# on .NET. A GDScript-only .NET
Release export also passed.
See [research and corrections](../../docs/research/godot-engine-integration.md).
The expanded 30-case image suite currently fails four FSR2/FSR3 independent-object
cases, with up to 0.62 output-pixel displacement, despite passing vector checks.
These failures remain visible in the reports; all-provider image acceptance is
unfinished.

## Build

Run these commands from a Developer PowerShell with CMake on PATH. Preparation
uses installed uv/Python 3.14 and provisions the pinned SDKs and Mesa NIR library.
MSVC and the Windows SDK are described in [tooling](../../docs/tooling.md). Reference checkouts,
engine binaries, vendor binaries and captures remain ignored.

```powershell
./eng/prepare-godot.ps1
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

## Project setup

Copy the staged `addons/rescaleframe` directory into the project and enable
ReScaleFrame in Project Settings > Plugins. This registers settings, generates
a persistent project UUID and installs the export plugin. In
`project.godot`, enable the native presentation bridge before engine startup:

```ini
[rescaleframe]
enabled=true
```

Use the custom engine, Forward+ and the `d3d12` rendering driver. Attach a
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

```powershell
./eng/test-godot.ps1 -EngineBridge -Edition standard,mono -GraphicsApi d3d12
./eng/test-godot.ps1 -EngineBridge -Scenario controller
./eng/test-godot.ps1 -EngineBridge -Scenario pcl -NoCSharp
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
