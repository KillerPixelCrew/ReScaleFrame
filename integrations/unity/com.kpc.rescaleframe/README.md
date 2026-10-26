# ReScaleFrame for URP

Reusable Unity package for ReScaleFrame's shared reconstruction, frame generation and
latency backends. The package owns URP inputs and output reinsertion; vendor execution
and presentation remain in ReScaleFrame. It uses Unity's native plugin interfaces and
the URP upscaler framework, without Harmony or executable fingerprints.

The initial target is Unity 6000.6, URP 17.6, Windows x64, with Direct3D 11, Direct3D 12
and Vulkan. Compilation and runtime acceptance are separate; the repository's
`docs/research/unity-urp-package.md` records current evidence.

RTSS compatibility is required across the supported APIs, including active generation
and runtime provider changes. Validation keeps RTSS enabled.

## Install

For development, build and stage the package from the ReScaleFrame checkout:

```powershell
./eng/build-unity-package.ps1 -VS2026
```

In Unity Package Manager, use **Add package from disk** and select this directory's
`package.json`. The Unity project manifest can also reference it:

```json
"com.kpc.rescaleframe": "file:D:/Coding/ReScaleFrame/integrations/unity/com.kpc.rescaleframe"
```

The package enables `ENABLE_UPSCALER_FRAMEWORK` for Standalone builds. This is the
compile switch used by URP 17.6 for its custom upscaler API. After compilation, choose
**Tools > ReScaleFrame > Set Up Active URP Asset**. This selects ReScaleFrame, creates
the asset's options, and adds the renderer feature to its renderers.

Enable post-processing on the game camera, as required by URP's upscaler pass.
Select the graphics APIs in Windows Player Settings. The native plugin must retain
**Load on startup** so presentation and Vulkan external-memory setup happen before
graphics initialization. The build processor copies the runtime dependencies and
writes the current project's engine version and GUID into the player configuration.

## Runtime controls

The ReScaleFrame options sub-asset exposes independent SR, quality, requested DLSS
model, FG provider, generation count, latency mode and frame limit. SR offers Off,
FSR1, FSR2, FSR3, FSR4, XeSS, DLSS and Auto. FG offers DLSS, FSR3, FSR4, XeSS and
Auto, with a separate enable switch. Requested counts are constrained by the active
backend's SDK capacity. DLSS model presets remain requests subject to driver overrides.

```csharp
var options = ReScaleFrameRuntime.Options;
options.superResolution = SuperResolution.XeSS;
options.generateFrames = true;
options.frameGeneration = FrameGeneration.FSR3;
options.latency = LatencyMode.On;
ReScaleFrameRuntime.Apply(options);

// Inform the integration when gameplay enters a menu or loading transition.
ReScaleFrameRuntime.SetGameplay(false);

if (ReScaleFrameRuntime.TryGetStatus(out var status))
    Debug.Log($"Active SR: {status.ActiveSR}, FG: {status.FrameGenerationActive}");
```

Application focus and paused simulation also defer gameplay interpolation. History
resets on context recreation, camera discontinuity, render-size and settings changes.
Provider replacement follows the runtime's drained presentation transition.

PCL/input markers are supplied independently of enabling generated frames. Input and
simulation boundaries are inserted around Unity's player loop. Windows keyboard,
mouse, raw input and PCL ping messages are forwarded; the optional Input System assembly
also marks keyboard, mouse and gamepad state events. A custom input system can call
`ReScaleFrameRuntime.NotifyInput` at consumption (keyboard `1`, mouse `2`, gamepad `4`).
Reflex availability and actual PCL/latency-report state come from the SDK. A requested
latency mode does not establish measured input-to-display latency.

## Rendering and ownership

The upscaler consumes HDR colour before post-processing/UI, raw device depth, dense
URP object/camera motion, actual projection jitter, camera matrices and frame timing.
Typed intermediate textures preserve a consistent cross-API resource contract.
FSR1 runs EASU and RCAS in URP; temporal providers execute through the shared runtime.
A complete spatial result is queued before native execution and remains when a vendor
refuses an evaluation. Jitter starts only after temporal reconstruction is prepared.

DX12 submits through Unity's native graphics interface. DX11 shares same-adapter
textures and fences with the shared D3D12 execution device. Vulkan imports D3D12
resources and fences through Win32 external-memory/semaphore extensions, with Unity
queue-access callbacks ordering uploads and downloads. Vulkan presentation uses an
input-transparent child surface so the shared FG backend owns the displayed chain;
the Unity window retains input and focus ownership.

Native resource leases and GPU completion govern reuse and retirement. A busy
shutdown retains resources. Native DLLs must not be replaced while loaded by an editor
or player; the staging/build steps refuse known running targets.

The editor Game view has its own presentation host, sharing one Streamline registration
between SR, FG and PCL. An input-transparent native surface displays completed frames
inside the Game view. Submission follows UI rendering at Unity's end-of-frame boundary.
Keep the Game tab visible during preview. Native DLL updates require an editor restart.
The Game view must be focused for foreground-only driver features. Unity's normal
background-rendering setting controls whether an unfocused preview advances.

The demo HUD includes quality modes, DLSS model requests, SDK-limited MFG multipliers,
and separate Base/FG-output FPS. Output FPS uses valid presentation counters; it is
not the requested multiplier times Base FPS. Editor and standalone paths have separate
runtime acceptance. Custom shaders and render features must produce appropriate motion
vectors for their animated content. Camera stacks, XR, HDR display output and unusual
render-target compositions require their own acceptance; the initial validation scene
does not certify them.

Unity 6000.6 can report DX12 depth-backbuffer barrier errors at reduced render scale,
including with stock URP Bilinear and ReScaleFrame disabled. Unity tracks this as
[UUM-136054](https://issuetracker-mig.prd.it.unity3d.com/issues/multiple-d3d12-errors-are-thrown-when-playing-urp-based-builds-due-to-d3d12-depth-backbuffer-barrier-mismatch).

## Demo and validation

Import **Temporal Demo**, then choose **Tools > ReScaleFrame > Create Temporal
Validation Scene**. The scene supplies independent object motion and runtime controls.
The editor's `ValidationScene.BuildPlayer` helper builds this scene without replacing
the project's build scene list. Standalone validation accepts:

```text
-force-d3d11 | -force-d3d12 | -force-vulkan
--rsf-sr=DLSS --rsf-fg=FSR3 --rsf-generate=true --rsf-latency=On
--rsf-output=<capture directory> --rsf-quit-after=20
```

SDK binaries and licenses are staged under `Runtime~` and copied to the player's
StreamingAssets. The package does not depend on WSGM or the game-mod carrier.
