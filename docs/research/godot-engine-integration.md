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

## FSR2 history and raster-jitter controls, 25 October 2026

Question: does the remaining DX12 FSR2 source displacement come from missing
vectors, incorrect raster jitter or history reconstruction? The bounded control
keeps the same object poses, source frames and MSAA reference, and changes only
the SDK's reset flag on the selected capture frames. This is preferable to a
permanent reset workaround, which would discard the temporal feature being tested.

`linear-normal-fsr2-object` retains the ordinary FSR2 history; its midpoint errors
against `linear-reset-reference-object` are 0.516, 0.546, 0.578 and 0.454 output
pixels. `linear-reset-fsr2-object` supplies the same four inputs with SR history
reset and gives 0.193, 0.162, 0.110 and 0.253 pixels. The SDK reset flags and
actual linear inputs/output are captured, rather than inferred from settings.
The native DLL for these runs is
`3B3455B79534039B9B34DF29AE7ECF716C83899BE71FF7BD7FC4D1F00E31ADAE`.
The analyzer refuses to count forced-reset runs as normal temporal acceptance.
This implicates history/reconstruction, but does not establish its root cause
or repair the ordinary FSR2 path. It also does not establish a vendor defect.

The analyzer now estimates raster jitter from box planes and raw depth,
independently of SDK camera matrices. Source review corrected an assumption:
Camera3D's script projection uses output aspect, while RendererSceneCull builds
the raster projection using rounded internal dimensions. The fixture uses
KEEP_HEIGHT, so the independent reconstruction now accounts for that aspect.
On the FSR2 control, fitted jitter differs from supplied jitter by at most
0.00122 render pixels, with maximum object-vector error below 0.0026 pixels and
no zero moving-object vectors. This evidence rejects a large missing-vector or
raster-jitter error in these frames; it does not cover arbitrary scene geometry.

The primary 30-case matrix was reanalyzed into separate `decoded-jitter-fit`
directories. `.local/godot/reanalysis-jitter-fit-20261025.json` still reports
26 passes and the same four image failures. Maximum fitted-jitter error across
all 120 source frames is 0.008311 render pixels, below the 0.02-pixel gate.
Original captures, primary reports, thresholds and failures remain intact.

## FSR2 geometry control, 25 October 2026

The remaining question was whether the cube's FSR2 displacement still reflects
missing object vectors, or depends on its reconstruction geometry. A fresh
ordinary Vulkan capture with the current staged runtime retains all four image
failures: `visual-20261025-164837-931.json`. Its midpoint centroid errors are
0.5240, 0.5517, 0.5848 and 0.4603 output pixels. Object-motion p95 error remains
below 0.001424 render pixels. Most displacement is already in the SR source.

The visual fixture now has an explicit `-Geometry billboard` diagnostic. It
keeps the same unit-box topology and records its complete transform, but aligns
the box with the camera and makes it 0.02 world units thick. Source/reference
poses and material remain paired. This reduces depth variation on the visible
surface without replacing temporal history or changing acceptance thresholds.
Both the capture metadata and scene report record the geometry, and the analyzer
rejects reference/capture geometry mismatches.

`visual-20261025-165209-485.json` passes all four actual generated-image checks
for this control. Maximum material-centroid error is 0.314833 output pixels and
object-motion p95 error is below 0.000809 render pixels. The rendered comparison
images for frame 14 in both runs were opened. This supports geometry-dependent
FSR2 reconstruction as the next investigation target; it does not isolate
motion dilation from depth clipping or shading, prove a vendor defect, or fix
the ordinary cube. The ordinary failure remains a failure.

The first, 0.002-unit-thick control (`visual-20261025-164915-303.json`) passed
its image gates but failed the existing minimum geometric-pixel count on one
frame. Depth/projection precision left only 95 accepted object samples there.
The 0.02-unit control retains 676 to 678 accepted object samples. Its nearly
front-facing plane does not independently constrain both raster-jitter axes on
every frame, so it is not additional jitter acceptance. The cube's existing
plane-fit evidence remains separate. `fsr2-geometry-mismatch.log` preserves a
negative control rejected for pairing billboard inputs with a box reference.

Source checks also confirmed the supplied vertical FOV conversion and render-pixel
motion scaling against the pinned Godot projection helpers and FSR2 dispatch.
They did not reveal an incorrect convention to patch. The fixture uses SDK
FSR2 2.3.3; Godot's built-in FSR2 is 2.2.1, so the latter would not be an
identical provider oracle. No production rendering change follows from this
bounded control. Other Godot providers and the remaining failed image cases
still require work.

## Native Vulkan SR and raw image validation, 25 October 2026

Question: can the developer integration use Godot's native Vulkan images and
command buffers without routing SR through D3D12? A reusable C ABI service now
lives in `runtime/orchestrator`, while the engine adapter owns Godot formats,
motion conventions and render graph placement. It supports actual FSR2 and FSR3
providers only. Requested DLSS/XeSS/FSR4, Vulkan FG and Vulkan PCL are refused;
Auto currently selects FSR3. This replaces the earlier Vulkan probe-only status
for this specific SR subset.

The service loads the pinned Vulkan FidelityFX SDK 1.1.4, revision
`c6efa6bf7f2027b3ec94f28578bb5965eabb9e55`, selects and verifies the exact provider
version, and records into the engine-owned command buffer. It submits no queue.
Images carry device, format, extent, layout and optional copy-source capability.
The callback lease lasts through GPU completion. The SDK writes a private output;
only an accepted dispatch copies it into Godot's fallback output. The owner drains
GPU work before context replacement or destruction, including settings, device
and output-size changes. Camera cuts, frame gaps, view/session changes and resource
generations invalidate history. No borrowed image handle survives in runtime state.

The first Godot run refused D32-plus-stencil input (Vulkan format 130) instead of
guessing a compatible format. The adapter now resolves depth to owned R32F while
producing dense RG16F UV motion. Source review also found that Godot's upscaled
output lacked transfer-destination usage. The guarded engine patch adds it, plus
transfer-source usage for scene colour and normalized motion diagnostics. These
are allocation contracts, not image-layout workarounds.

The first Khronos validation run failed even though player PNGs existed. It found
a null Win32 surface HINSTANCE and two storage-image format mismatches in both
Godot's FSR2 fallback and the pinned Vulkan SDK: dilated depth was declared `r16f`
but allocated R32F; prepared colour was declared `rgba16` (UNORM) but allocated
RGBA16F. Guarded patches match the declarations to the reviewed allocations.
Godot's surface creation now uses the same GetModuleHandle fallback as its window
class registration. Preserve the failed evidence in
`validation/standard/sr-20261025-135424-170`; do not relabel that run as validated.
The existing FSR3 luma-history and swapchain-lifetime corrections remain applied
only to the staged SDK sources. The reference checkout stays clean.

`eng/build-godot-vulkan.ps1` now reproduces the local SDK build. It uses Godot's
pinned Vulkan-Headers, the installed Windows Vulkan loader, a locally generated
MSVC import library and installed Visual Studio tools. No global SDK is installed.
Default and staged-copy shader compiler launches failed; explicitly selecting the
original pinned glslang executable succeeded with the same command/permutations.
The original and staged executable hashes matched. The reason for the location
dependent failure remains unknown; path length, antivirus and DrivePool were not
established causes. Failed SDK logs remain under `.local/godot`. The recipe pins
the successful compiler location and records its hash
`8106440BE591596425D7EF401E62B8E21CE778BE7A4AA8D240D319B614691A8C`.
The completed reproducible runtime is
`2955CC3513B0843FB1C41A9793506AEA408434D21FF2D85EAC331077C15A0F98`;
`fsr3-sdk-build.json` records source, patch, compiler and runtime fingerprints.

Khronos ValidationLayers was absent initially. The portable official Windows
nightly archive published 25 October was downloaded and verified without registry
or global environment changes. Its source revision is
`8fa483ff4e2706601f4b1a1f5a4666b42673ba85`, archive SHA256
`0693DCA1739EE70FBD8A1ACA1C59DBEF4EB2DE95785BC49DB4FCD062E50199D4`,
and loaded DLL SHA256
`E86FA4F70676A120F00A32BEAC6979CEB3CAFCE70DE42FA9107C89F587AB206E`.
Each validation run checks actual process module loading and preserves its receipt.
Errors, VUIDs, synchronization hazards and undefined storage-format warnings fail
the harness. Loader naming warnings for unrelated installed overlays remain visible.

Final editor fingerprints:

| Artifact | SHA256 |
| --- | --- |
| Standard editor | `BFDCE691DBB4407587DA4E49EE931174361086413A819D061726CF4570FC3617` |
| .NET editor | `DD4EA65C4EC6C5D1F431D530FBDCFABBC4C6BD428343261C974A2B0605D723BA` |
| Native extension | `6CD1E97EB0BF1D655B958BE57FB257505CA830B95A313FC1CB65B3DDED15D815` |

Validation manifests `validation-20261025-140919-328.json` and
`validation-20261025-140941-594.json` cover both editions, initial FSR2/FSR3,
provider replacement, resize, history reset and C# calls. Both ran with the loaded
validation layer and passed. They retain actual player PNGs, 64 raw guide/output
images across 16 source frames, and browsable `frames/decoded/report.html` files.
Readbacks are recorded beside SDK use, retain owned host-coherent buffers through
submission, and flush only after the owner's GPU-drained context retirement.
Metadata records Vulkan formats, actual provider ID, frame/view/generation,
SDK reset and camera/jitter values. Capture is opt-in and capped at 16 frames.
It changes timing and therefore provides no performance or latency evidence.

The independent geometry checks passed: maximum motion p95 error is 0.000338
render pixels, camera metadata error 0.000038 pixels and fitted jitter error
0.001325 pixels. All images are finite. A preserved negative-control copy with
zero motion fails all object/floor vector gates; a second copy with one-pixel
jitter corruption fails all raster-jitter gates
(`validation-controls/vulkan-20261025-141059/controls.json`). These copies do not
modify the original captures. SR output previews were opened and inspected.
This suite has no independent Vulkan SR image-quality reference, so input and
readback passes must not be described as broad SR visual acceptance.

The real device fixture separately passed 24 Vulkan FSR2/FSR3 GPU dispatches and
pixel readbacks, known-colour replacement/reset and foreign-device guards
(`test-vulkan-device-final.log`). The Release MSVC native gate passed 45 executed
tests, nine opt-in skips, formatting and Clippy
(`verify-vulkan-readback-final.log`). Both editions passed the DX12 six-phase
SR/FG/MFG/PCL regression on the new binaries
(`validation-20261025-141020-597.json`). No export templates were rebuilt for this
Vulkan increment; earlier export acceptance belongs to the recorded DX12 builds.

## Native Vulkan FSR3 presentation, 25 October 2026

Question: can native Vulkan frame generation consume the completed HUD-less
Godot image and present real generated frames while retaining engine ownership
of recording/submission? The selected route uses the pinned FidelityFX Vulkan
swapchain implementation. Its entire WSI table must own creation, image
enumeration, acquisition, presentation and destruction. Sending the opaque proxy
handle to a native Vulkan WSI function is invalid. This is a runtime service
under `runtime/orchestrator`; the Godot module supplies engine queues, resources
and source identity through C ABI calls.

Source review found that Godot requested one queue per family. The guarded patch
now requests up to four when this integration booted, reserves indices 1..3 from
Godot's virtual queue allocator, and admits the presentation service only when
four distinct same-family queues and Vulkan 1.2 timeline semaphores are available.
Godot now enables its queried timeline feature. The graphics queue retains
Godot's locking; SDK submissions use its host callback. The queue mutex is
recursive because SDK submission can reenter from the engine's Present call.
The other three queues are reserved for compute, presentation and acquisition.
Input acquisition waits cover all commands for the proxy, including preparation.

The first actual WSI run failed validation: Godot left proxy images in PRESENT,
while the SDK requires SHADER_READ_ONLY. Proxy images are regular owned images;
only the SDK's real WSI images enter PRESENT. The custom render-pass final layout
now reflects that distinction. Native secondary windows retain their own WSI
functions. Mixed presentation batches split their per-image completion semaphores;
the no-command-buffer case fans out the shared wait list through a submission,
so a binary semaphore is never consumed twice. The functional fixture opens and
closes a second native Window while root-window FG remains active.

FG preparation runs at the normalized depth/motion producer, before tonemapping.
The completed SDR RGBA8 scene is copied after tonemapping and before canvas into
six owned HUD-less slots. The SDK is configured only after actual engine
submission identified both callbacks' command buffers. Recording alone never
enables generation. The frame record includes session, source frame, view,
resource generation, camera transforms, physical frame duration and raster jitter.
Motion is previous-minus-current UV, scaled by render width/height for the SDK;
depth is inverted R32F. Cuts and identity/generation gaps reset history. Missing,
stale or incomplete input bypasses generation. Async interpolation is explicitly
disabled. A graphics-queue fence after SDK Present covers its interpolation
callback's HUD-less reads; source-copy and consumer fences protect slot reuse.
The SDK presents its own images afterwards. CPU waits occur on busy slot reuse
or explicit retirement, not on every ordinary source frame.

The source/runtime boundary now has separate drain and destroy operations.
The engine drains deferred presentation before releasing views/semaphores, then
destroys the chain through its SDK owner. Failed retirement preserves ownership.
SR context retirement also drains FG before waiting on the shared Vulkan device.
The public C ABI rejects short enclosing/embedded structures and mismatched ABI
versions before reading embedded data. Native contract tests cover these guards.

Two more SDK defects were found through actual validation runs:

- The global descriptor pool defines six pool-size entries but reports five,
  omitting STORAGE_BUFFER. FG creates pipelines requiring that descriptor type.
  A guarded staged patch computes the actual array count.
- The present timeline semaphore signals before the presenter thread's final
  CPU queue operations. A GPU wait alone therefore allowed `vkDeviceWaitIdle`
  to race `vkQueueSubmit` on the AMD FSR Presenter Thread. Both presenter paths
  now publish a CPU completion counter/event after processing an entry, and
  retirement waits for the last scheduled interpolated entry before GPU waits.
  SR retirement was also missing the FG drain and now performs it first.

`sdk-fixes/descriptor-pool-size.json` and `presenter-cpu-drain.json` retain the
reviewed anchors against SDK revision `c6efa6bf7f2027b3ec94f28578bb5965eabb9e55`.
The final SDK runtime is
`A527D594F05128F7E7B98E5A30D3F092C44DE5300F1B9DB2EB2527AAA0DB2358`.
Its build recipe hash is
`F132F7379D4DEEBDCE4407EC6C75C91EAF4FC462B5E7EBE98A4E4BEF04A39470`;
the build receipt records all corrections. Reference sources remain unchanged.

The first image matrix, `visual-20261025-143936-149.json`, preserves 18 cases
and their raw SDK-generated images. Thirteen passed. Both editions failed
FSR2 object-only and combined-motion image gates; the .NET FSR3 combined case
also failed its core-validation run because of the presenter retirement race.
The FSR2 failure also exists on Vulkan, so it is not isolated to D3D12.
This is evidence of the remaining displacement, not proof of a vendor defect.
The analyzer now decodes Vulkan formats and converts captured UV motion to render
pixels before applying the same geometry and midpoint gates. Vulkan reference
runs keep SR/FG Off and independently render half steps, with owned diagnostic
guide copies only on selected frames. Generated and reference binaries must match.
The generation callback also records actual output before returning to the SDK;
readback allocations survive until drained retirement. Images were opened and
inspected, including object/floor motion and the generated HUD.

After the retirement corrections, standard/.NET functional FG fixtures passed
with SR Off, Off/resume, resize and a secondary native window
(`validation-20261025-150454-666.json`). Both editions passed SR readback checks
again (`validation-20261025-150615-593.json`) and the full DX12 regression passed
(`validation-20261025-150647-855.json`). The Release MSVC native gate passed 45
executed tests, nine opt-in skips, formatting and Clippy
(`verify-vulkan-fg-final.log`). These are core-validation results; synchronization
validation has the separate unresolved result below. No export templates were
rebuilt for this increment.

The follow-up image suite `visual-20261025-150801-117.json` passed all 12
standard/.NET cases: SR Off and FSR3, each with object, camera and combined motion,
against new 8x-MSAA half-step references. It contains 48 actual generated images.
Maximum midpoint material-centroid error is 0.335683 output pixels; motion p95
error is at most 0.001876 render pixels, camera metadata error 0.000019 pixels
and fitted raster-jitter error 0.008120 pixels. This closes the repeated FSR3
combined-motion case on the corrected runtime. It does not remove the original
failed run or the FSR2 image failures. The standalone Vulkan device fixture also
passed both SR providers again with the final SDK binary
(`test-native-vulkan-after-fg-sdk.log`).

Final binaries for these functional runs:

| Artifact | SHA256 |
| --- | --- |
| Standard editor | `50C12366AC3BB386976AD4544D25AE8B24A487ADBA91FB3745B3D0E9BB5CEC78` |
| .NET editor | `E0A6400CCCDB0E27E5578C210C244C0D897E197440189EB335951C382F8E18C8` |
| Native extension | `3B055794D98AC3ED45A531ABDCBB23734AB4766D69D95D0F7F66AF5D51C9BFEC` |

Synchronization validation is enabled by the documented `validate_sync` setting
([LunarG guide](https://vulkan.lunarg.com/doc/view/latest/windows/synchronization_usage.html)).
The harness now checks the loaded layer's enabled-settings report and records
the actual layer/module inventory. It catches PRESENT_AFTER_WRITE after
`vkCmdEndRenderPass`, including in a single-window control and official Godot
without the integration (`runtime-20261025-145951-849`). Filtering implicit layers
did not eliminate it; RTSSHooks64.dll remains injected even when its Vulkan layer
is filtered. Disabling individual layer environment flags did not establish a
cause. Do not attribute this hazard to FSR, RTSS, the HDR layer or Godot without
an isolating control. Those failed runs remain failures, including
`fg-20261025-144937-267`, `fg-20261025-145258-867` and later controls.

A uniquely named, byte-identical test executable was prepared for a temporary
RTSS application-specific detection exclusion using its installed documented
profile API. The SDK accepted the in-memory property but could not persist it.
A scoped write probe confirmed that the installed Profiles directory is not
writable from this session. The attempted profile was removed and no existing
profile changed. This control did not launch a test and supplies no renderer
acceptance. RTSS remains running. The synchronization hazard is still open.

The next NVIDIA/PCL dependency is now prepared: clean Streamline source at
`2122257e0fce486f91b385aa63b9a09b0a34b363`, with a corrected Production build
using the retained Vulkan semaphore and NGX dispatch corrections. The receipt at
`vulkan/host/artifacts/streamline-production/runtime/streamline-sdk-build.json`
records common DLL hash
`CE85930D3EFF5F3A806D036279B76714FE515BD3B81AAD6360FBE57AD4E0800D`
and interposer hash
`AAFE189D8DC80AD947F4BF7B033531AAE7DAE96A3F98EBC877D487764977EB3B`.
This is a source build, not Godot Vulkan PCL/DLSS acceptance. It is not staged
over the active retail D3D12 SDK. The NVIDIA skill catalog had no close
Streamline/Reflex match; the pinned SDK source remains the integration reference.

## Native Vulkan PCL/Reflex, 25 October 2026

The question was whether Godot could run PCL/Reflex before input polling and
retain matching simulation, render and Present identity without a camera, SR or
an FG presentation owner. The D3D12 host already supplied those boundaries, but
the Vulkan adapter still refused latency options. Attaching marker calls to the
3D callback would omit camera-free frames and place sleep after input.

The native runtime now initializes Streamline before Vulkan instance creation.
The guarded Godot context patch installs its instance resolver through
`volkInitializeCustom`, so instance/device creation and subsequent instance and
device dispatch lookups share the interposer. The same private runtime module
stays mapped through Godot device/instance destruction after SDK shutdown.
The reusable `native_latency_vulkan.h` interface has size/version-checked setup
and status structures. It preserves frame-limit units in microseconds.

CPU frame begin obtains a token before Windows input polling. Pacing applies
Reflex Off/On/Boost and sleeps once for that source frame. Simulation markers
follow the existing engine boundaries; render-submit start occurs at the queued
RenderingServer draw boundary. The pinned SDK retains only six frame tokens.
The host keeps a bounded identity map and checks the SDK token's current index
before use. A late pacing event cannot sleep using a newer frame's token.
Nonrendered frames are aborted, and duplicate common markers are suppressed.

The Win32 surface records its HWND, so Present markers follow the actual root
window on both native Godot WSI and the FSR proxy. They do not depend on a 3D
callback or a captured FG input. A completed source identity is consumed once;
secondary windows cannot reuse it. The actual registered Windows ping message
passes through the input window procedure to a PCL marker on the CPU source
token. Godot input events use that token too. Keyboard/mouse event counters
retain real engine dequeue timing; controller input emits marker 13 once per
source. Keyboard/mouse events do not spoof controller or deprecated input
markers. Synthetic keyboard, mouse and controller events in the fixture test
that route; they are not physical-device input tests.

The first device-validation run reached the SDK but failed with two concrete
API errors: enabling `VK_NV_low_latency2` omitted its `VK_KHR_present_id`
dependency, and the compute backend called `vkCreatePrivateDataSlot` without
the privateData feature. Source inspection of the pinned interposer and compute
backend explained both errors. The guarded SDK build now adds the dependency,
merges privateData when supported, publishes actual feature enablement through
the shared parameter store and creates private-data slots only when enabled.
This preserves the internal VkTable ABI used by the signed retail plugins.
The SDK's older Vulkan headers lack the LL2 extension-name macro, so the
dependency check uses its registered string name.

The initial PCL device-validation build was staged separately in `streamline-vulkan`.
Its receipt at
`.local/godot/vulkan/host/artifacts/streamline-production-pcl/runtime/streamline-sdk-build.json`
records source `2122257e0fce486f91b385aa63b9a09b0a34b363`, correction hash
`30797A862E07B1EABA2B00F42F5C946C35C575F5960CCC72A1431E9307A0C392`,
common DLL hash
`868A02D49F383997035B868B4F1ED98B5168EBCD5F1BD16C4F7AEE2D22AB7272`
and interposer hash
`69C41F12B6E5D0D66347E88900319A4CC7257351C00C53750214B679FD178139`.
CMake checks recipe/correction/binary hashes and embeds the interposer pin.
Staging retains the signed PCL/Reflex binaries and notices. D3D12 uses its
existing separate retail SDK directory.

Bounded Windows/RTX 5080 evidence, with the recorded Khronos layer actually
loaded and core validation enabled:

| Receipt under `.local/godot` | Scope |
| --- | --- |
| `validation-20261025-154458-090.json` | Standard/.NET camera-free native WSI, no FG owner, On/Boost/Off, resize, synthetic inputs and ping |
| `validation-20261025-154516-266.json` | Same PCL fixture using the FSR presentation owner with SR/FG Off |
| `validation-20261025-154534-250.json` | Standard/.NET FSR3 FG with PCL, SR-independent generation, Off/resume, resize, secondary window, C# and raw GPU frames |
| `validation-20261025-154556-487.json` | Standard/.NET SR switching/reset/resize, actual raw image checks and C# |
| `validation-20261025-154619-206.json` | Standard/.NET D3D12 SR/FG/MFG/PCL regression |
| `visual-20261025-153904-884.json/.html` | 12 PCL-enabled Vulkan image cases, 48 actual generated images, SR Off/FSR3, object/camera/combined motion and both editions |

The 12-case image suite passes unchanged gates with new 8x MSAA independent
midpoint references. Maximum material-centroid displacement is
0.335802173 output pixels; maximum motion p95 error is 0.001875748 render
pixels, camera metadata p95 error 0.000018796 render pixels, and fitted raster
jitter error 0.008119276 pixels. The .NET combined-motion frame 14 comparison
was also opened for visual inspection. These simple scenes do not establish
broad image quality. PCL availability, accepted markers/sleeps and report
availability do not measure input-to-display latency reduction.

The image suite used standard engine hash
`BFB655A0827800DCBE4FB32112B1A062C2A779B803260DE0245CA1938FB9907A`,
.NET hash
`2B5418A69CE269FA1CF1AC4BB8E6E12F10BD6F52DD844DC3F2BD2623371686A5`
and native runtime
`5EC71F23CB49EA952D0D879954A4E9161E14E9D761E82808BB92445CD224AB13`.
The later functional receipts use the explicit test-flag correction and session
cleanup, with standard engine
`31A03F40ECD5C6962C49630E138658FF19CA5ED2A3AC45F274EDF9AB64576448`,
.NET engine
`16FD6A0552CAB85D751E1DA2F92482C7EEFFA8E2C646C1E92E8DC7E1FAA1FAC6`
and native runtime
`F7329FD12D3B032BD588ADF1C518230018532E8DADAA734E415B793B8C7EB372`.
Each receipt and capture fingerprints its actual binaries, rather than relying
on these filenames alone. Template/export acceptance above belongs to earlier
D3D12 binaries; Vulkan templates have not yet been rebuilt for this increment.

Failed controls remain preserved. `pcl-20261025-152921-681` omitted the layer
environment; the harness stopped it when the required DLL was absent.
`pcl-20261025-152953-821` reached PCL but failed the two device API checks above.
`fg-20261025-153857-570` accidentally retained the native-WSI diagnostic flag as
an empty environment variable, so it had no FG owner and rejected FG options.
A separate .NET/PowerShell probe confirmed that assigning null through the
string setter leaves an empty variable present. The control now requires the
value `1`, and environment restoration removes originally absent variables.
Named Windows mutexes serialize foreground validation across scripts. These
failures were not converted into acceptance or removed from the evidence.

These early PCL counters were not proof of a driver sleep target. A later
ownership audit found that FSR's swapchain code calls the system loader directly,
so Streamline never observed its real primary swapchain. Its LL2 backend can
return success with no current swapchain. The first secondary window then became
the most recently created chain. The earlier image results remain SR/FG evidence,
but do not establish PCL driver ownership.

The engine now carries explicit primary-window identity from Win32 platform
data to the Vulkan surface and sets the NV latency-mode creation flag only for
that window. The corrected LL2 backend ignores secondary chains. A private FSR
WSI dispatch ABI 2 routes real create/destroy, image queries, acquire and Present
through host callbacks with explicit synchronous leases. Native Godot WSI uses
the same broker. The callbacks never retain caller data or Godot objects.

Routing real WSI exposed two further validation failures:
`fg-20261025-160843-868` reported overlapping `vkSetLatencyMarkerNV`/sleep and
the AMD presenter thread's `vkQueuePresentKHR`;
`fg-20261025-161808-125` showed the corresponding `vkAcquireNextImageKHR`
race after Present alone was serialized. The final broker uses the LL2 backend's
own mutex for the immediate WSI/latency calls, including acquire/image queries.
The blocking timeline wait after Reflex sleep stays outside that SDK mutex.
Reflex options are refreshed once per CPU source frame, so an unchanged mode
and microsecond interval reach a resized successor. The simulation thread ID is
preserved when the Reflex API forwards PCL options.

The final corrected SDK receipts are:

- Streamline: `vulkan/host/artifacts/streamline-production-pcl-wsi/runtime/streamline-sdk-build.json`,
  correction hash `6DBB51FDCA4CF230178AFFDB1594BFE55C9182D4AF66C7283E23165C9F78D5E8`,
  common DLL `4905A476770CDFD7AEA03C0216B13B8CD781082C89D03566FBCC374F15CC1607`,
  interposer `C3D186BC196F14D907A2008B40CC4E2B3D026483A3A6BB44759026781AFF6B6F`.
- FidelityFX: `vulkan/runtime/fsr3-sdk-build.json`, runtime DLL
  `273AA4A84B3BE01AFE0F0CA111B291BB37BD9A94F703DC39EC1A96D8B788288D`,
  WSI header `BA7EEFF4CD344F8636740A3397EE54AF3564790D154280B3068A1FDE4F8A9FB3`.

The first SDK ownership build also stopped at a corrupt staged NVAPI library:
its hash was `753CA1D58DB01F810D1185778E9D6016D0AC91DD851D6B04FF4277AA453DBE6A`,
while the pinned LFS pointer and clean reference both require
`901E479E548D41C30688C7481F0C6353C0BE7FDEE978796D27D87C32E19CE172`.
That failed output remains under `streamline-production-pcl-owners-corrupt-nvapi`.
The builder now verifies copies, checks the three build libraries against their
LFS content identities and records those identities in the receipt. The cause
of the mismatched copy was not established. It also restores originally absent
Git environment variables by removing them, rather than leaving empty values.

Final acceptance after these corrections:

| Receipt under `.local/godot` | Scope |
| --- | --- |
| `validation-20261025-162924-233.json` | Standard/.NET FG/PCL, SR Off, Off/resume, C#, actual GPU dumps, primary resize while a secondary window remains open, and asserted SDK owner traces; core validation passes |
| `validation-20261025-162947-366.json` | Standard/.NET camera-free native WSI with no FG owner, On/Boost/Off, resize, synthetic input and ping; core validation passes |
| `visual-20261025-163005-264.json/.html` | Four combined-motion cases, SR Off/FSR3 and both editions, with PCL enabled; 16 actual generated images pass against new independent MSAA midpoint references |

The final image suite's maximum material-centroid displacement is 0.273574387
output pixels. Its scope is combined motion; the earlier 12-case suite supplies
separate object/camera SR/FG evidence, not final PCL ownership evidence.
The final standard engine hash is
`E60D28D55689EF5DA3E28A98023032E21C6EB8C5264B5BF224EAF41AEB0034EF`,
the .NET engine hash is
`64E12816663F9840EAB59C4757A9161C8528775820B5CF5FF7EA585A71FD2218`,
and the staged, GPU-tested native DLL is
`15A17E960B77F87C66957EBB16EB6B8B1971885AC81EDB164997A70B0107EF23`.
The native gate subsequently relinked the DLL as
`1DF5C8A3B16C2D0047B8DD520F104117F5C520A1EB947C8EC2AB003FCDAF2C5D`.
An explicit binary comparison found only two differing bytes, in the PE and
debug-directory timestamps; code and other sections match. The receipt is
`native-built-vs-staged-final-pcl.json`. The GPU-tested staged payload remains
preserved. The explicit device fixture also passed 24 FSR2/FSR3 GPU dispatches,
readback/reset and foreign-device checks against the final FidelityFX DLL.

The native Release gate adds synthetic SDK-boundary tests for ABI refusal,
microsecond options, token expiry, duplicate pacing/markers, aborted frames,
Present identity consumption, session reinitialization, and WSI callback routing
and retirement. They are not GPU SDK tests. The gate discovers 56 tests,
executes 47 and skips nine opt-in fixtures;
formatting and Clippy pass. The native Vulkan service and Godot private exports
do not change game ABI 13 or game-plugin readiness flags.

## Vulkan SR failure output, 25 October 2026

Initial inspection inferred an unwritten output because the engine callback
ignores the native SR return value. A later trace of Forward+ corrected that
conclusion: its built-in FSR2 pass already writes the upscaled texture before
the native callback on the current temporal path. The missing-DLL fixture below
does not prove that the previous engine produced blank frames. The runtime now
provides explicit failure recovery independently of that preceding pass: a separate,
size/version-checked spatial fallback call. It checks image ownership, layouts,
transfer-source usage and RGBA16F blit/filter support before recording. A linear
blit fills the output, restores the input's shader-read layout and leaves output
in the graph's transfer-destination layout. It never submits a queue or retains
images; the existing callback lease lasts through GPU completion. The Godot
adapter calls it on SR creation/evaluation/retirement refusal, keeps `active_sr`
zero, increments refused frames and preserves the original provider error.

The Release native gate passes 47 executed tests with nine opt-in skips, plus
formatting and Clippy (`verify-sr-fallback.log`). The explicit RTX 5080 Vulkan
fixture passes two full-image gradient readbacks, including borders/alpha,
layout reuse and invalid-owner guards, then 24 FSR2/FSR3 dispatch/readbacks
(`test-native-vulkan-sr-fallback.log`). Core validation reports no API errors;
this does not settle the earlier synchronization-validation present hazard.

`validation-20261025-171133-378.json` passes standard/.NET fixtures with the
FidelityFX Vulkan DLL deliberately removed only from each new fixture's private
payload. Both requested SR providers remain refused, while actual player PNGs
retain the cyan object before and after resize. The standard FSR2 PNG was opened
for inspection. No C# is exercised in this failure fixture. The outer command
returned a stale prior native-command exit code of one despite the successful
script/manifest; retain the manifest and player results as the test evidence.
The ordinary regression `validation-20261025-171211-760.json` exits successfully
and passes both editions, C# on .NET, four raw SR frames per edition and core
validation. The tested native DLL is
`8FEEA9C797C6BB9D1CBAF1D499D8784FB4BF1A1DB0131C38F7D7211E0C18B04C`;
engine identities remain those in the preceding PCL acceptance. These tests
prove bounded failure recovery, not FSR2 moving-object reconstruction quality.

## Native Vulkan DLSS SR, 25 October 2026

DLSS SR now uses the same early Streamline initialization as PCL, rather than
loading another interposer or acquiring render-thread frame tokens. A separate
size/version-checked session setup selects SR while preserving latency ABI 1.
A synchronous SDK-call lease holds the session mutex and validates the original
64-bit session/source identity against the SDK's six retained token handles.
Non-frame operations explicitly use zero identities. The synthetic boundary
checks callback errors, foreign/expired tokens, ABI refusal and an older render
lease leaving the newer CPU pacing token unchanged.

The native SR service checks support on the borrowed physical device and queries
optimal render dimensions. One DLSS owner uses viewport one, leaving root-window
latency/presentation ownership separate. It copies RGBA16F scene colour,
normalized R32F depth and RG16F UV motion into owned images/views. Engine inputs
return to shader-read layout before SDK evaluation; the SDK only touches owned
resources. Camera constants use row vectors, inverted depth, pixel jitter and
motion scale divided by render dimensions. Matching source/session/view/resource
identity controls reset. HDR scene colour, automatic exposure and the requested
model reach DLSS options. An accepted dispatch copies private output into the
engine's transfer-destination image. Refusal retains the original error and uses
the explicit spatial recovery above. The caller owns submission and GPU drain.
Raw readbacks record `sr_backend=1`; `provider_version_id=0` means no SDK version
query was established. Payload manifests carry the actual DLL identities.

The private Vulkan payload now includes unmodified, signed retail `sl.dlss.dll`
(`73BF52C0CFAA5900A8F3F4A91306E4625E7CCA696DFB305AAE44C9F97B582E1F`)
and `nvngx_dlss.dll`
(`3975567B8943C53ACCE397F2B72380092F84F162D00B0D2C7D08A1025C563983`),
with their notices. Corrected common/interposer identities and pinned source
remain those in the PCL section. No new SDK source correction was required here.

Failures retained during development:

- `test-native-vulkan-dlss.log`: pixel checks passed, but validation rejected
  an output without transfer-destination usage, which NGX clears. The private
  output now declares that usage. The standalone fixture also enables surface
  and swapchain extensions required by its Reflex/WSI feature set.
- `validation/standard/sr-20261025-172033-266`: the first engine raw-capture run
  crashed because the new path had not initialized its capture helper. The
  corrected path initializes it with the owner device/dispatch/memory properties.
- `test-native-vulkan-dlss-final.log` and `...-final-verbose.log`: changing the
  preset in place passed all pixel checks but crashed during SDK shutdown.
  The pinned DLSS source queues a release lambda with a three-Present delay
  (`dlssEntry.cpp` around 373); the standalone fixture has no Presents.
  `Generic::collectGarbage` called that lambda during common shutdown after the
  feature plugin was unloaded. The SDK dump reports access violation
  `0xC0000005` in common at offset `0x72442`, reading an address outside the
  dump's loaded modules. Source shutdown order and the deferred-task log support
  the lifetime diagnosis; this is not a reconstruction of the signed DLL.
  The model setter now requires drained GPU work and frees the old SDK feature
  before applying a changed model. Engine model changes already retire the
  whole context. `test-native-vulkan-dlss-model-retirement.log` now passes
  default/K pixel readbacks and clean shutdown, with no queued release lambda
  or validation errors.
- The first FG regression rejected a verbose SDK info message quoting VUID
  `03328` while explaining removal of `VK_EXT_buffer_device_address`. It was
  not a layer error. The harness classifies only that exact source notice;
  injected real VUID, synchronization and SDK errors remain detectable
  (`vulkan-validation-log-filter-check.txt`). Original logs remain intact.

`validation-20261025-172115-185.json` passes standard/.NET DLSS-to-FSR3 switching,
resize/reset, actual player PNGs, C# and four raw SR frames per edition.
`visual-20261025-172254-662.json/.html` passes all six object/camera/combined
DLSS SR + FSR3 FG + PCL cases, covering 24 actual generated images against
independent MSAA midpoint renders. The .NET combined frame-14 comparison was
opened. Those runs used native DLL
`56ED1AD7B78F77A18BAC17B65A64DB17889D1F3572A4DE6CED11993093817235`.
They precede the final live-model retirement correction; their default-model
scope does not exercise that setter transition.

The final model/switching engine run
`validation-20261025-173343-430.json` passes both editions, the K model request,
DLSS-to-FSR3 switching, resize, raw inputs and C#. Its native DLL is
`EE22B1A698E6DD906E1C294A0DC72805C37857543368F379DE2E1F54713978F3`.
The Release gate (`verify-dlss-model-retirement.log`) executes 47 tests and skips
ten opt-in fixtures, including the newly registered DLSS device test; formatting
and Clippy pass. The explicit default/K Vulkan device run is separate GPU
evidence, not an executed device check in that gate. Model configuration and
continued rendering do not prove a driver-selected effective model.
The final shared-session regressions also pass both editions with core
validation: `validation-20261025-173608-568.json` covers FSR3 FG/PCL, SR Off,
Off/resume, resize while a secondary window remains open and asserted SDK root
ownership; `validation-20261025-173639-484.json` covers camera-free native WSI,
On/Boost/Off, resize, input and ping with no FG owner. These use the final native
DLL above and do not establish synchronization-validation acceptance.

## Vulkan exported players, 25 October 2026

All four custom export templates were rebuilt from the pinned engine and guarded
module/patch source. These are built artifacts, not the earlier DX12 templates:

| Template | SHA-256 |
| --- | --- |
| Standard Release | `AB0906BA31F335A83E0927767B111B180935154DC0608BC39D05A0531071559F` |
| Standard Debug | `99753194D36EAFA2111A366A029AAADBDFD8450FA89E351BCB410D02058AB596` |
| .NET Release | `D8957C8DA2575492CB56803983A7E2FBEFFBAEDE7CC1833D90ACF207034B1E3B` |
| .NET Debug | `8D281F54B826F9654004676F81C8CDB436DA5925103C2785D7973B6CEF3E36F6` |

The following Vulkan exported-player receipts each pass standard and .NET with
core validation. Players launch without a source-project path and carry their
own verified vendor payload. All use the final native DLL `EE22B1A6...13978F3`
recorded in full above. Both exported native copies, at the root and early-load
add-on path, are checked against the staged source DLL. A separate post-run audit
also verifies all 12 player hashes and both native copies in
`vulkan-export-acceptance-20261025.json`.

| Receipt under `.local/godot` | Configuration and scope |
| --- | --- |
| `validation-20261025-174232-223.json` | Release SR: DLSS, K request, FSR3, resize/reset, PNGs and four raw input/output frames per edition |
| `validation-20261025-174527-447.json` | Release FSR3 FG/PCL: SR Off, Off/resume, resize with secondary window and asserted root ownership |
| `validation-20261025-174636-278.json` | Release camera-free native-WSI PCL, with no FG owner |
| `validation-20261025-174806-357.json` | Debug SR, matching the Release SR scope |
| `validation-20261025-174932-005.json` | Debug FG/PCL, matching the Release FG scope |
| `validation-20261025-175048-977.json` | Debug camera-free native-WSI PCL |

The SR/FG fixtures exercise C# on .NET; the PCL fixture is GDScript-only on both
editions. The .NET Release K-request player PNG was opened for inspection.
These tests establish exported runtime execution, image visibility, input
conventions and lifecycle. The independent midpoint comparisons above remain
editor evidence; exported midpoint-quality and measured latency are not claimed.

The first Release FG attempt (`fg-20261025-174319-872`) was terminated by the
harness's five-second foreground deadline while startup logs were still
processing SDK manifests, before its main window became available. It is not a
renderer failure or a passing test. Foreground acquisition now has a measured
30-second deadline, still requires the actual foreground HWND, and remains
inside the functional harness's original 60-second overall bound. The corrected
run passes. Focus timing and exported-library checks are script changes;
PowerShell parsing and `git diff --check` pass, without a new native/Rust change.

A separate preparation query confirms that the pinned `libxess.dll` exports its
Vulkan API. `xessVKGetRequiredInstanceExtensions` returns success, minimum Vulkan
1.3 and zero extra instance extensions (`xess-vulkan-instance-requirements.json`).
Its DLL hash is `251659DD84A3E84DE67C886A4186E01F3ECA49B00641906FE38BB6B807E5D5B7`.
This is a CPU API query only. Device-extension/feature preparation and actual
XeSS Vulkan execution remain pending.

## Remaining work and validation limits

Vulkan FSR2/FSR3 SR has the bounded engine/device/readback evidence above. Vulkan
FSR3 FG has a completed-frame presentation owner and real submission identity,
with the bounded evidence above. Vulkan PCL has the engine/device evidence above.
Vulkan DLSS SR has the bounded engine/device/image evidence above.
Other FG/SR providers,
synchronization validation and broader acceptance remain unfinished. Any interop route needs
same-adapter external memory/fences and graph-owned submission splits. The Unity
Vulkan queue-access API cannot be assumed to exist in Godot.

The earlier expanded SR-enabled image matrix retains its FSR2/FSR3 object-motion
failures. Earlier passing
cases do not establish all-provider image acceptance. Broader scene-quality coverage remains
pending; a box and floor do not establish transparent, particle, skinned or
occlusion-heavy scene quality.
No shipped game, non-Windows platform, Mobile/Compatibility renderer, HDR display
output, XR device or input-to-display latency claim follows from these runs.

## Capture storage cleanup, 25 October 2026

Removed 85.20 GiB of duplicate binaries/import caches from completed fixture
projects and 6.51 GiB of obsolete raw readbacks. Compact reports, decoded images,
selected regression raw inputs and current package/build inputs remain. Cleanup
manifests are `dump-cleanup-files.json` and `dump-cleanup-raw-result.json` under
`.local/godot`; old fixture binary paths in historical receipts may no longer
exist. New captures continue normally.

Capture and functional fixture projects now junction their runtime directory to
an immutable cache keyed by sorted payload paths/hashes. Native add-on binaries
and scripts remain private copies. A later package change gets a new cache key;
it cannot mutate an existing fixture's SDK payload. Fault-injection fixtures keep
private runtime copies. Four fresh capture projects passed junction and every
payload-file hash check (`runtime-cache-validation.json`), and loaded/executed
the SDKs with validation. Their XeSS image comparisons still fail; cache
validation is not image acceptance. PowerShell parsing and diff checks pass.
