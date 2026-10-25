# Godot developer integration, 25 October 2026

## Question and scope

Connect the shared SR, FG and PCL runtime to a developer's Godot project, including
the standard and .NET editions. This is an engine integration under
`integrations/godot`, independent of game detection, injection and Mono/Harmony.
The initial platform is Windows x64, Godot 4 Forward+. DX12 and Vulkan require
separate execution and presentation evidence.

## Sources and decisions

Godot 4.7.2 stable is pinned to `ed1daf0bf001b61586d9930840f2f1394092c079`.
godot-cpp is pinned to `8070599885b6df675fcad033dd032ce44be2b850`, using its
4.7 single-precision API. Both official Windows editions were downloaded and
checked against the release SHA512 sums. The checked-in
[dependency lock](../../integrations/godot/dependencies.json) records these inputs.
References, engine binaries, SDKs and generated validation projects remain ignored.

The public CompositorEffect interface supplies scene buffers, camera transforms
and a jittered GPU projection on the render thread. It does not expose
`RenderingDevice::driver_callback_add`. Inspecting the matching engine source
found that method adds resource dependencies to the render graph and runs a
native callback while recording the actual driver command buffer. Godot uses
this mechanism internally for MetalFX.

The full runtime therefore needs a small engine module and bounded source changes,
alongside a GDExtension for project-facing resources and C# access. The patcher
reconstructs expected changes from the pinned Git source and refuses unrelated
changes. It does not match private executable layouts or patch a shipped game.
Godot renderer code is studied in the reference checkout; the first-party adapter
and build workflow remain in this monorepo.

## Producer and consumer contracts

The inspected Forward+ order is opaque/sky/transparency, post-transparent
compositors, native temporal upscaling/TAA, then post-processing and tonemapping.
`render_forward_clustered.cpp` supplies the SR hook immediately before tonemapping.
The initial implementation retains native temporal reconstruction as a complete
fallback, then records the vendor overwrite into the full-resolution upscaled
target. This has an additional native temporal-pass cost; it is not an optimized
single-upscaler path yet.

The separate FG hook follows post-processing/tonemapping and precedes viewport
canvas/UI composition. It records HUD-less colour. Actual window association is
made at DX12 Present, using the copied source-frame identity. SR scene colour
cannot replace this completed-image boundary.

Source-derived conventions:

- `scene_forward_clustered.glsl` writes previous-minus-current UV motion after
  removing current and previous jitter. The adapter supplies `motion_to_uv = -1`
  on both axes to the existing normalizer. The normalizer produces the shared
  previous-minus-current pixel convention.
- The source projection uses reverse depth. The adapter copies corrected,
  unjittered camera matrices and measures pixel jitter from the native NDC jitter
  and internal extent. The public scene projection getter includes jitter and
  must not be treated as an unjittered matrix.
- Internal and output extents are separate. DLSS's 640x360 Quality plan requested
  427x240 while Godot's scalar calculation initially produced 426x240. A guarded
  render-buffer sizing hook applies the exact admitted dimensions.
- SR Off must restore native resolution and stop selecting native temporal
  upscaling. An initial feedback loop retained the previous XeSS input dimensions;
  explicit Off sizing and mode selection corrected that behavior.

The module selects one explicit primary viewport. Other viewports, reflection
probes, XR and multiple presentation surfaces are not silently associated with it.
Resize/resource replacement, source discontinuity and explicit history reset
invalidate temporal history. Metadata is copied into graph callback packets;
borrowed scene objects do not escape their recording callback.

Native callback work restores Godot's D3D12 pipeline, root-signature, descriptor
heap and dynamic-state caches. Command-list identity is carried to the actual
queue submission. Per-slot GPU fences govern descriptor/context reuse; recording
completion alone never marks FG inputs submitted. HUD-less submission depends
on the SR output in the graph and establishes the completed guide packet.

## Experiments and corrections

The GDExtension renderer probe built with Windows MSVC 19.51 / VS2026. Standard
and .NET 4.7.2 engines passed the DX12 and Vulkan animated probe: native colour,
depth, motion, device/queue handles, independent object motion, camera motion,
resize and disable/resume. The .NET probe constructed the native resource from
C# and queried it through the same API.

Direct velocity readback failed because Godot does not allocate that texture with
copy-source usage. Bounded diagnostics now sample it through a compute pass into
an owned buffer before reading CPU data. DX12 observed 879 finite moving pixels
in the object-only sample at 428x241, with maximum absolute UV motion about
0.001024. The readback is explicitly requested and stalls; it is not a per-frame
rendering path.

The first real vendor SR calls failed input validation. D3D12 resource descriptors
showed RGBA16 typeless colour/output and RG16 typeless velocity, despite the
engine's typed float views. The shared normalizer now accepts those float-view
families and creates typed SRVs. An explicit RTX 5080 device fixture with
`RSF_SR_TYPELESS_INPUTS=1` evaluated DLSS successfully and accepted independent
SR/FG constants on one token. Relative SDK paths failed the initial fixture;
the successful invocation used absolute runtime directories.

The standard custom-engine DX12 harness subsequently passed:

- DLSS SR with FG off and PCL/Reflex markers.
- FSR3 SR/FG and XeSS SR/FG, including a window resize and provider replacement.
- DLSS-G with SR Off at native resolution.
- DLSS SR with three generated frames requested, and observed active generation.
- PCL/Reflex with SR and FG Off.

FG activity is checked against valid provider statistics. FSR and XeSS do not
provide a valid generated-presented counter in this path. The harness instead
compares total presentation deltas against actual source-frame deltas within
one settled provider generation. It does not multiply requested counts into an
invented output FPS. DLSS required bringing the validation window to the foreground;
an unfocused run did not establish active interpolation. RTSS remained running.
Completed engine-frame PNGs were saved and the DLSS/MFG scene/UI capture was
inspected. This is not physical scanout, latency measurement or broad image-quality
acceptance.

The custom .NET run exposed a real interop failure: official GodotSharp cannot
construct a wrapper for the module's API_NONE singleton. Initial C# calls through
`Engine.GetSingleton` therefore failed. The correction routes configuration and
status through a GDExtension RefCounted facade, with size/version-checked C engine
services. Godot objects and C++ containers never cross that DLL contract. The
module remains outside the core managed API, avoiding a replacement GodotSharp
package. The corrected standard and .NET functional harnesses subsequently passed all six DX12
phases, including SR-independent DLSS-G, MFG and PCL with SR/FG Off. Real Windows
foreground ownership was required. A fixture helper now establishes and verifies
that ownership instead of trusting Godot's cached focus flag; the earlier failed
runs had the terminal in the foreground despite that flag reporting true.

Some one-frame headless editor imports exited with access violations or an
EditorNode teardown error. The harness now permits 30 iterations after import;
this needs continued repeat-run coverage. Native DLL load failures include the
Windows loader error. Do not hide either condition with a successful test report.

## Actual generated-frame capture and motion correction

The earlier harness checked functional status and saved engine/base-frame PNGs.
It did not capture the interpolated image. That was insufficient for FG visual
acceptance, and the user's report of severe motion artifacts exposed the gap.
The functional passes above must not be interpreted as image-quality passes.

The corrected diagnostic path copies normalized depth/motion and post-tonemap
HUD-less colour on their owning D3D12 command lists, and copies the actual FSR
SDK interpolation output inside its generation callback. Readbacks retire only
after the corresponding queue fence. Files include frame, view, generation,
camera matrices, pixel units, DXGI format and row pitch. Capture is opt-in and
bounded; its GPU waits make it unsuitable for performance or latency measurement.

The decisive experiment compared ordinary execution against an explicitly
requested raw-motion probe. In the old ordinary path, all normalized motion
pixels on frames 30 through 33 were zero. Godot's raw probe observed 70,392 moving
pixels with maximum absolute UV motion 0.00263214. Requesting that probe forced
GPU submission and made normalized motion valid on frame 32 only. Thus the
producer existed; its ordinary native consumer did not receive valid data.

Source inspection found that `RenderingDeviceGraph::add_driver_callback` left
`RecordedCommand::self_stages` empty, unlike compute/copy commands. The guarded
patch now declares all-command stages for native callbacks, whose internals can
issue compute, graphics and copies. Repeating the ordinary capture without the
raw probe produced valid motion on all four frames. No per-frame CPU readback
was added to the integration. The regression analyzer rejects raw-probe runs
because their forced submission can conceal this scheduling defect.

The same dumps identified two camera metadata errors. The initial serializer
stored Godot's column-vector transform as SDK rows without the mathematical
transpose, leaving camera translation at the wrong indices. It now copies
Godot columns into SDK rows, placing translation at 12 through 14. SDK camera
clip space also requires positive Y up; applying Godot's render-target Y flip
to these matrices created a separate reprojection error. Texture-space motion
already contains the render-target convention and is unchanged.

The deterministic fixture now runs a second no-FG scene at half animation steps.
The analyzer unprojects captured depth using independently recorded Camera3D
poses, reprojects static geometry, and maps the moving box through its actual
previous/current transforms. This tests dense object and camera vectors rather
than inferring correctness from counters or a few sampled pixels.

On the RTX 5080 standard-engine combined-motion run, frames 12 through 15 had
object motion error below 0.003 pixels at the 95th percentile, camera metadata
reprojection error below 0.00002 pixels, and zero nonfinite motion pixels. Actual
generated box centroids were within 0.081 pixels of separately rendered midpoint
images, compared with about 1.0 to 1.13 pixels for the un-interpolated base image.
Generated box RGB error was 0.0019 to 0.0035, compared with 0.0095 to 0.0106 for
the base. Whole-scene geometry-edge error was also recorded; it did not improve
uniformly, so these results establish this small fixture's temporal correctness,
not general artifact-free interpolation. Actual generated comparison sheets were
opened and inspected. The analyzer rejects the preserved old broken capture.

Local evidence is under `.local/godot/visual`: `before-matrix-combined`,
`before-raw-probe-combined`, `clip-convention-combined`, `clip-reference-combined`
and `mono-csharp-clip-combined`. The .NET capture also exercised construction and
configuration through the C# facade. `eng/test-godot-visual.ps1` automates separate
object, camera and combined cases for both editions and writes binary hashes,
decoded images, numerical reports and an aggregate manifest.

Repeat runs exposed a separate intermittent startup crash, including an RTSS
hook-chain backtrace before the first scene frame. This failure remains separate
from the motion diagnosis. The developer integration now supplies its swapchain
creation site explicitly to the shared host instead of installing DXGI detours.
The shared API retains the existing detour path for shipped-game adapters. A real
D3D12 FSR3 device fixture passed the explicit creation path with 37 active
generation samples and 75 total presented frames. Standard-engine actual-frame
comparison passed after this change. The explicit path's FSR/XeSS switching
fixture also passed, including the plain-presentation fallback and retention of
one application latency handle. RTSS has remained running throughout these
experiments.

The completed automatic image matrix passed six cases: standard/.NET times
object-only, camera-only and combined motion, with four actual generated images
per case and separately rendered midpoint references. All 24 generated-frame
checks passed with motion error below 0.003 pixels at the 95th percentile and
maximum midpoint box-centroid error 0.081 pixels. .NET also checked its C# facade.
The latest analyzer was rerun against all six preserved datasets. The aggregate
manifest is `.local/godot/visual-20261025-101835-935.json`; standard engine SHA256
is `4011F784890FE106D63442D18E75E7EF1879399A9B79673F4FD2732EBE98C2C9`
and .NET engine SHA256 is
`95554F3369143D3ECF03D8DBF474D384F2C7E5DE68E3936DA08533273A5D1422`.
Each capture records the matching native DLL hash too. These hashes describe the
tested builds, not every later rebuild. Repeat coverage found no startup failure
in this completed matrix; it does not establish every RTSS configuration.

The Release `eng/verify.ps1 -VS2026` gate passed after the shared native changes:
44 tests ran and eight opt-in device tests were skipped, followed by successful
Rust formatting/Clippy checks. The explicit FSR3 presentation and switching
device fixtures above were run separately on Windows, rather than counting the
skipped gate entries as device acceptance.

## SR-enabled motion correction and stronger image checks

The six-case image matrix above used SR Off. Repeating the combined-motion
capture with DLSS SR enabled exposed another producer convention. In Godot's
temporal path, static pixels can contain raw velocity `(-1, -1)`; its native
FSR2 shader reconstructs camera motion for those pixels. The ordinary Compositor
probe path had produced dense velocities, so its earlier success did not cover
this branch. Passing that sentinel to the shared normalizer produced about
490 pixels of false movement. The preserved `before-sentinel-combined` dataset
records that failure. This corrects the earlier assumption that the same raw
velocity convention applied with SR both Off and On.

The engine module now owns an RD compute pass, `motion.cpp`, before the native
SR callback. It preserves valid object velocities and reconstructs only sentinel
pixels from resolved depth and the current-to-previous camera transform. It
removes the actual raster jitter before reprojection. The output is an RG16F
texture owned by the current render buffers, with declared graph dependencies;
the shared runtime still receives dense UV motion with the documented direction.
This is an engine convention, not a vendor-specific decoder in the orchestrator.

The corrected DLSS/FSR3 combined captures each contain four actual FSR-generated
images with motion error below 0.003 pixels at the 95th percentile. Evidence:
`resolved-sentinel-bytes-combined` and `fsr3-resolved-sentinel-combined` under
`.local/godot/visual`. These are distinct from the earlier SR-Off matrix.

The midpoint reference now renders at twice the output width/height and half
animation steps, then area-averages to the capture resolution. A hard blue-pixel
threshold had moved the measured centroid by about 0.7 pixels when SR changed
edge coverage. The analyzer retains that diagnostic but gates the centroid on
material coverage within the geometry-bounded object region. The limit remains
0.35 pixels. Deliberately translating output by one or two pixels still fails
the check, as does the preserved broken-motion capture. Geometry-derived motion
and camera errors remain separate gates. This avoids treating edge-threshold
changes as motion while still rejecting displaced generated geometry.

## Lifecycle, exports and reproducibility

The controller fixture now exercises pause/resume, changing Camera3D identity,
a 40 m cut, Off/re-enable, controller removal/replacement and PCL with no camera.
An identity change or explicit reset invalidates history. Captured source 100
carries the reset flag and is presented without interpolation; the FSR prepare
dispatch for source 101 resumes with reset set. Requiring an SDK dispatch on the
cut itself was an incorrect first assertion, since the common FG gate correctly
skips it. Source review also found that the FSR/XeSS backend reset inputs ignored
the common frame reset flag; both now include it. That source finding alone does
not establish that the old Godot cut was interpolated.

PCL render-submit start is emitted at the queued RenderingServer draw boundary,
before scene work, using the same session/source identity as Present. The native
host retains only copied metadata after the resource callback. A frame without
a camera receives current-frame presentation identity instead of reusing the
previous scene's texture metadata. Physical frame duration comes from the CPU
clock, independently of `Engine.time_scale`. PCL sleep is placed before the
Windows event pump; input sampling and simulation start follow the pump. The
movable-window loop has the corresponding guarded entry, and no-present frames
end their token without inventing a render submission.

The functional fixture posts the actual registered PCL diagnostic message to
the owned window in every provider phase, then checks new ping/marker counts.
Synthetic keyboard, mouse and controller events exercise input consumption.
These checks establish SDK plumbing, not physical input-to-display latency.
DLSS-G tests establish actual Windows foreground ownership rather than trusting
Godot's cached focus flag.

Debug/release export templates are built separately for standard and .NET.
The export plugin copies the native library to the engine's exact disk path,
copies the allowlisted vendor payload and notices, and verifies hashes. Exported
validation launches the player without `--path`, so it must load its own PCK
and dependencies. The native module's relative-path resolution was corrected
for that case. The .NET fixture includes a solution file required by export;
the optional C# facade lives under `.gdignore`, allowing a GDScript-only .NET
project to exclude it from Godot's C# import/export scan.

Artifact investigation found two separate symptoms: a previously staged engine
path could crash while a byte-identical fresh path started, and one later copy
was demonstrably corrupt. In the latter case, 64 consecutive 4 KiB pages at
offset `0x1d00000` were zero despite an unchanged source hash and equal length.
The failed temporary copy is preserved locally. No operating-system or storage
root cause is established. Build staging now writes verified buffers with
write-through, flushes, checks SHA256 and rejects a mismatch. Engines/templates
are staged under immutable content-hash directories. Every harness project is
new, preventing stale binaries, C# helpers or outputs from satisfying a test.

`prepare-godot.ps1` now provisions the pinned local Python environment, engine
and binding references, Streamline, FidelityFX, XeSS and the Godot Mesa NIR
library. Downloads are hash-checked; reference checkouts and SDKs stay ignored.
The cached preparation path passed. A clean machine download/build remains a
different acceptance claim.

The final D3D12 lifecycle batch passed the following local manifests, with
native DLL SHA256
`F741F31C92BD971F79F5FF033FF165E5CEAE80F91F8426D24E496F3F8EF6CC6C`:

| Manifest under `.local/godot` | Coverage |
| --- | --- |
| `validation-20261025-120838-623.json` | Standard/.NET controller lifecycle, C# on .NET |
| `validation-20261025-120920-090.json` | Standard/.NET six runtime/provider/PCL phases |
| `validation-20261025-121014-881.json` | Standard/.NET Release exported runtime |
| `validation-20261025-121127-381.json` | Standard/.NET Release exported controller |
| `validation-20261025-121215-141.json` | Standard/.NET Debug exported runtime |
| `validation-20261025-121321-579.json` | Standard/.NET Debug exported controller |
| `validation-20261025-121414-556.json` | .NET Release export using GDScript without C# |

The tested editor hashes are
`7077468833E8D46E4C187AFDF96CD5762E13E6AD4B1B15EEF59EA79ACFF6527F`
(standard) and
`609793B6AD73D26A5F393E59CA9539AD678B556F1E0FD4A9F80B0FA9D583695A`
(.NET). Release template hashes are
`6464F7F9D7EC83F3E9FFAD7DF05929BC642FD262C86B05AFBF6626C7916D076E`
and `D0DB2B31EF35E0E6E6668D291777FF0C4DF4CF4AD065701AE3983CC71520A1D1`;
Debug template hashes are
`5A4AC73D143980FA5691FBE69E640C162CBA21E21AE3C43215E9FC9F6FDD54AB`
and `96BA4E3E67AB1FAE9ACB06EB37D59930795DDB92D3821B85E4A63C058CEF0A5A`.
Standard precedes .NET in each pair. Exported-player manifests record those
same executable hashes. The Release native gate again passed 44 executed tests,
eight opt-in skips, formatting and Clippy (`verify-pacing-native.log`).

## New image failures retained by the wider matrix

Adding FSR2 and separating object-only from combined motion caught a failure the
earlier successful combined captures did not establish away. FSR2 object-only
generated centroid errors were about 0.49 to 0.62 pixels against the independent
midpoint, despite motion error below 0.003 render pixels. FSR3 object-only also
crossed the 0.35 pixel gate on one captured frame. DLSS and XeSS object-only
passed that gate in this run. These are image failures, not successful tests
because a provider counter increased.

The preserved initial dataset is
`visual-20261025-121506-052-standard-sr2-object`. A later-frame FSR2 capture passed
at slower/reversed motion. The controlled `fsr2-aged-same-pose-object` experiment
then kept the original poses/velocity while moving capture from source 12 to
source 40; it still failed. Therefore the initial warmup hypothesis is rejected
for this case. Do not fix this by choosing slower motion or increasing the gate.

The analyzer now compares the HUD-less source against an independently rendered
current-frame reference as well as comparing the actual generated output against
the midpoint. In the captured FSR2 case, most displacement already exists before
interpolation: source error vectors were approximately `(0.28..0.60, -0.28..-0.33)`
output pixels. The interpolation residual after subtracting source displacement
was 0.007 to 0.117 pixels. This localizes most of the remaining error to the SR
source/reconstruction path; it does not establish the underlying cause or waive
the end-to-end failure. `decoded/report.html` shows both metrics and links the
actual images, depth and motion. The original image gate remains unchanged.

A hard-link staging optimization was rejected by the local DrivePool volume and
aborted one partial matrix before its combined reference run. Those filesystem
errors are not renderer results. Verified byte copies were restored and the
full matrix restarted with new directories. Preserve the aborted evidence rather
than counting its missing images as visual acceptance.

The restarted matrix completed as
`.local/godot/visual-20261025-122552-743.json`, with an HTML index of the same name.
It contains 30 cases: two editions, three motion modes and SR Off/DLSS/FSR2/FSR3/XeSS,
all using FSR3 frame generation. There are 120 actual generated images plus
independently rendered references. All vector/camera gates passed; maximum
object/floor motion p95 error was 0.002918 render pixels and maximum camera
metadata error was 0.002433 render pixels. Twenty-six end-to-end image cases
passed. FSR2 and FSR3 object-only cases failed on both editions; maximum midpoint
centroid error was 0.616172 output pixels. The suite therefore fails overall.
Both passing and failing image reports are available under each `decoded` folder.
An additional standard-engine reference renders the midpoint with 8x MSAA at
the output resolution. Source inspection confirms that Godot resolves its HDR
colour before tonemapping, avoiding the primary reference's averaging of SDR
pixels. This control still fails all four FSR2 object frames, with midpoint errors
0.453 to 0.578 pixels (`linear-msaa-reference-object`). Reference filtering explains
a small part of the difference and does not remove the FSR2 failure. Alternate
analyses use `--variant` and separate decoded directories so they cannot overwrite
the primary suite's comparisons.
Standard FSR3 object-only passes this MSAA control (maximum 0.330 pixels), while
its primary SDR-downsampled reference crossed the gate on one frame. Therefore
the borderline FSR3 result is sensitive to reference sampling; it is not yet a
confirmed integration defect. SR Off, DLSS and XeSS object-only also pass the
MSAA control. The primary matrix and its original failures remain intact.
The additional combined-motion FSR1/Auto matrix passed all four standard/.NET
cases and 16 actual generated images (`visual-20261025-123530-525.json`). FSR1
is executed by Godot before the normalization callback; its status now reports
that engine spatial path instead of labeling it SR Off. Auto reports the actual
selected provider. Neither result removes the temporal FSR2/FSR3 failures above.

PCL from startup with no Camera3D or compositor passed on standard/.NET editors
and Release exports (`validation-20261025-123414-203.json` and
`validation-20261025-123436-537.json`). The standard/.NET official-engine producer
probes also passed again on DX12 and Vulkan
(`validation-20261025-123507-927.json`). The Vulkan result remains a producer probe,
not Vulkan runtime SR/FG acceptance.

The latest shared-host regression fixtures passed both explicit engine creation
and the legacy detour route (`test-final-explicit-hot.log` and
`test-final-detour-hot.log`). Each recorded 21 active FSR samples and seven active
XeSS samples, with backbuffer identity and one application latency handle retained
through provider transitions. These real device checks cover the shared changes
without claiming Unity or shipped-game deployment acceptance.

## Remaining work and validation limits

Vulkan has renderer-probe evidence only. It still needs graph-owned submission
splits, same-adapter external memory/fences and a completed-frame presentation
owner. The Unity Vulkan queue-access API cannot be assumed to exist in Godot.

The expanded SR-enabled image matrix completed against the final staged
binaries; its FSR2/FSR3 object-motion failures remain unresolved. Earlier passing
cases do not establish all-provider image acceptance. Broader scene-quality coverage remains
pending; a box and floor do not establish transparent, particle, skinned or
occlusion-heavy scene quality.
No shipped game, non-Windows platform, Mobile/Compatibility renderer, HDR display
output, XR device or input-to-display latency claim follows from these runs.
