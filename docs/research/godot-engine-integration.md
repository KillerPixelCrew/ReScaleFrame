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

## Remaining work and validation limits

Vulkan FSR2/FSR3 SR has the bounded engine/device/readback evidence above. Vulkan
FSR3 FG has a completed-frame presentation owner and real submission identity,
with the bounded evidence above. Vulkan PCL, other FG/SR providers,
synchronization validation and broader acceptance remain unfinished. Any interop route needs
same-adapter external memory/fences and graph-owned submission splits. The Unity
Vulkan queue-access API cannot be assumed to exist in Godot.

The expanded SR-enabled image matrix completed against the final staged
binaries; its FSR2/FSR3 object-motion failures remain unresolved. Earlier passing
cases do not establish all-provider image acceptance. Broader scene-quality coverage remains
pending; a box and floor do not establish transparent, particle, skinned or
occlusion-heavy scene quality.
No shipped game, non-Windows platform, Mobile/Compatibility renderer, HDR display
output, XR device or input-to-display latency claim follows from these runs.
