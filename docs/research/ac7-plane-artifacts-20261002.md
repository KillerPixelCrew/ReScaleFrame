# AC7 aircraft composition artifacts, 2 October 2026

Final status: after the 21:24 deployment the user reports "Works. And is clean!" Native logs
verify Texture2D/RG32F depth bindings, divisor 1 and replacement dispatch. The wing/cloud
correction is accepted with volumetrics present. The latest depth/format findings are in
"Wing masking and cloud floor" below; earlier pending statements and the clean-plane/missing-cloud
run describe superseded experiments. The later SR bridge/FSR4 deployment was also accepted.
No new numeric FPS result or complete independent-object motion coverage was established.

The updated goal is to fix gameplay aircraft artifacts, then improve motion vectors using the
existing research. The user reports apparent missing vertices/triangles on steering surfaces,
including while stationary and across upscalers. They separately identify DLSS-only dark colour
steps at Performance/Ultra Performance and distant rectangular cloud blocks. The latter defects
must not be conflated with an absent aircraft mesh. Background/cloud jitter and blur are now
user-confirmed corrected. No further exposure-only experiment was run after the goal changed.

## Evidence and competing explanations

F9 session `motion-20261001-233456-50356-1` is flight at 533x300 to1600x900, DLSS, with
dense motion and reset0. Its engine colour already contains the affected small surfaces before
reconstruction. Current reversed-Z depth includes the aircraft/steering surfaces, and the decoded
written-vector mask covers them. This does not prove every triangle is present or correctly
projected, but simple complete omission from depth or velocity does not explain these samples.
Raw input/output comparison uses the same diagnostic display curve, not actual AC7 grading.
DLSS amplifies rectangular colour steps in the distant cloud region; this capture alone does
not establish those cloud steps across every backend. The latest five hangar sessions identify
successful backends1,1,2,3,5. FSR4 selection was refused and kept backend3; do not credit FSR4.
The engine exposure guides in those samples are finite, constant approximately5.

Prior research found a custom projection selected by primitive row17.y, using view rows116..119
and120..123. The common native parameter producer at RVA0x19d4b50 copies current matrix
from View+0x430 into uniform+0x740; the previous copy comes from previous matrices+0x370
into uniform+0x780. These are leads for aircraft projection, not a proved cause. The sampled
primitive flags that could be read were0, while the GPU-readback budget left other bindings
unavailable. Native jitter removal at0xff9af0 and derived update0x100b960 do not write this
extra matrix. The earlier suspicion of partial restoration corrupting its last rows is not
supported by those functions. No custom projection patch was applied.

The runtime disables legacy draw/geometry/constant-buffer rewriting under native ownership.
That source audit rules out those active compatibility callbacks in this route, not every
possible mesh/shader defect. Source and Ghidra evidence retain the executable/dump/UE revision
fingerprints recorded in the linked stability and motion research.

## Narrow ordinary translucency back to its owner

An older UI correction wrote scene fields+0x218/+0x21c and+0x220 after native
SetSeparateTranslucencyBufferSize0x10be2b0. At Ultra this could enlarge ordinary shared
translucency by approximately3. Native scaled-view/depth selection then applies to all of its
3D materials, even before the scene's SR node. This broad intervention is a concrete source
ownership error and a candidate for the reported aircraft composition defect.

Later native UnmodifiedTranslucency correction superseded its UI purpose: begin0x1097d20
reads scene BufferSize+0x208 for allocation, uses the temporarily output-sized view and binds
scene depth+0x60. Its dedicated producer already owns those output-sized resources and
normal/scaled unjittered view refs and restores them after resolve. Ordinary separate-layer
size/scale is not the allocation source for this UI path.

The size hook now forwards the native setter without rewriting shared layer extent/scale.
The scoped UI producer, retained queued uniforms, bloom ordering and corrected sky projection
remain. Existing bootstrap depth/view gates are unchanged; under ordinary scale<=1 their
comparisons select the same downscaled/equal cases. This correction has not been shown to
eliminate the aircraft defect in game. No vertices, indices, mesh assets or shader bytecode were
replaced, and no motion coverage gate was changed as part of this candidate.

## Capture correction and validation

F9 previously read constants from inactive stages, including old HS/DS bindings on draws with
no tessellation shader. Those inherited bindings consumed the128-readback budget, shared across
all three sample intervals. Captures now record empty constants for inactive stages and bound
GPU readbacks to128 independently in intervals0,30,59. CPU upload snapshots remain available
without spending this budget. This is a capture correction, not a rendering test or a promise
that every aircraft binding can now be recovered.

MSVC Release proxy/plugin build and the deployment's real-overlay Insert/pixels/resize preflight
passed. Installed backup `.local/deploy-backups/ac7-pair-20261002-000941-391`. No game launch,
new tests or unrelated installation changes. Required retest is the same stationary aircraft
against clouds, with F9, checking both steering surfaces and UI. Missing-object motion coverage
continues from [the prior research](ac7-motion-vectors.md) after this defect is resolved. Cloud
effective depth, custom projection and producer coverage remain open. DLSS-specific banding is
a separate unresolved finding, not the explanation assigned to the aircraft defect.

## Retest and graphics-state correction

The user reports all three defects still present after the00:09 deployment. Flight session
`motion-20261002-084543-34000-1` confirms retained native ordinary-translucency sizing,
533x300 to1600x900, dense motion and384 successful constant readbacks. Pre-SR colour still
contains the aircraft defect. Removing the broad sizing write corrected ownership but did not
fix the artifact. DLSS Performance/Ultra exposure remains a separate unresolved defect.

The next question was whether our mid-frame work preserves the bindings Unreal caches. The
wrapper saved VS/PS/CS only,16 SRVs per stage and eight vertex buffers. It omitted HS/DS/GS
resources, constants and samplers while retaining their shader objects. Restoring CS UAVs with
zero initial counts also reset append/consume counters. Sampler count does not bound texture
count; the prior comment asserting that relationship was wrong. UE4.18's
`D3D11StateCachePrivate.h` caches all shader stages, so the game need not rebind an item foreign
work changed. Streamline v2.14.1's D3D11 `clearCache` calls `ClearState`. This is source evidence,
not proof of when that call executed in this captured game frame.

A standalone D3D11 hardware experiment on the NVIDIA adapter used identical bindings with the
old saver and replacement. After save, `ClearState` and restore, the old implementation lost
VS17/HS18/DS19/GS20/PS127/CS21. A PS CB range16,32 became0,4096 and append counter37 became0.
The replacement returned all six views, range16,32 and counter37. This is device-tested state
preservation, not AC7 visual validation. Replay source is ignored under
`.local/state-preservation-experiment-20261002.cpp`. Microsoft's
[UAV counter contract](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-id3d11devicecontext-cssetunorderedaccessviews)
and [stream-output cursor contract](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-id3d11devicecontext-sosettargets)
explain why restoration preserves counters/cursors rather than resetting them.

The replacement retains128 SRVs,16 samplers and14 CBs across all six stages, shader linkage,
supported CB ranges,32 IA vertex bindings, OM/CS UAVs, predication and stream-output targets.
Foreign conflicting bindings are cleared before restoration. Fixed8192-byte C storage owns a
private snapshot allocation; one restore releases the acquired references and allocation. It
must not be copied. Native SR and motion resolve disable predication for their work and restore
it afterward. Allocation failure or a deferred context refuses evaluation. This does not undo
resource writes or asynchronous query execution. No game hook bytes changed in this correction.

## Measured TrueSky composite and capture

The new flight capture contains live cloud-composite PS CRC32C `0x83524e47`, VS `0xddc582f0`,
3876 indices, stride32, no DSV, target536x300 and viewport533x300. Shader metadata identifies
near/far colour at t0/t1, distance intervals at t2 and scene depth at t3. Cloud inputs are268x152;
depth is536x300, format19. HdrConstants are704-byte CB12 upload blob154. Integer values2,2
at offset560 divide pixel coordinates into the low-resolution grid. Despite the debug name
`lowResDims`, these are divisors, not texture extents. Fields544/568 report266x150 and533x300.
Names require instruction-level interpretation. Shader fingerprints and CB files are local;
the executable/dump and UE revision remain those recorded in the hook map.

The composite tests neighbouring distance ranges and opacity. Its discontinuity branch loads
near/far colour at integer coordinates and interpolates using full-resolution scene depth;
the other branch filters far colour. This can produce rectangular regions, but the captured
constants alone do not establish that this branch paints the aircraft defect. The current
sun/glow shader CB10 depth UV scale is533/536,1,0,0, rejecting the simple assertion that TrueSky
ignores allocation padding. Cloud interpolation, adaptive geometry and material projection
remain competing explanations.

F9 now copies scene colour and atmospheric loss immediately before/after that exact composite
draw on the observed immediate context, plus near/far cloud colour, interval depth and actual
scene depth. Target pointer, interval, thread and draw ordinal pair the samples. The native
draw proceeds unchanged. This uses actual D3D execution rather than issuing GPU work from an
unverified CPU hook. No shader replacement or cloud-quality override is deployed.

F9 reads only CB slots declared by each shader, including stripped UE bytecode. Declaration
failure retains conservative capture. This prevents inherited unused buffers exhausting the
128-per-interval readback budget. Texture snapshot budget is72 per session; callbacks are
disabled outside sampled F9 intervals. Missing-object motion work continues from the existing
research, with these captures supplying the missing producer evidence.

The existing native-graph fixture now matches the current ABI: unconstrained rectangle, exact
backend rectangle, pooled velocity reference, recorder wait and owned uniform refs. No new test
target was added. Release verification passes33 tests with five environment/vendor skips, plus
Rust formatting/lint. Hardware state preservation passes. Game visual acceptance, DLSS exposure
and the separate vendor shutdown fault remain open.

Installed final pair at10:22 with backup
`.local/deploy-backups/ac7-pair-20261002-102210-392`. The actual overlay Insert, pixel rendering
and resize preflight passes. Installed proxy/plugin/overlay hashes match their build artifacts.
No AC7 launch. Required user run: same cloudy aircraft view with SR disabled and DLSS Ultra,
F9 once in each state. The paired samples locate whether cloud composition changes the affected
surfaces; they do not presume the result. The before-draw readback refreshes the D3D11 submission
vtable before forwarding, because Map can switch the runtime's batching implementation.

## Rendering corrections for all three blockers

Aircraft corruption, distant cloud blocks and DLSS Performance/Ultra colour steps are now all
priority-one blockers. Useful state repairs and instrumentation did not satisfy that request.

The captured TrueSky integer-divided composite grid was traced back through the shipped DLL's
root render interface into per-view allocation. Its integer q.downscale setter clamps to2..4;
the float setter also enforces a minimum2. Settings cannot reliably request a full scene grid.
Root render passes the setting into the cloud renderer, which updates view+0x1e4, marks resources
dirty and rebuilds colour, depth and metadata with that divisor. The allocator pads width/divisor
and height/divisor to multiples of2*divisor. Divisor1 is valid in the inspected arithmetic.

An expected-byte patch at TrueSky DLL RVA0x87f6d now passes divisor1 before rendering. Native
reallocation and depth/composite constants follow together. This avoids magnifying the extra
half-resolution cloud grid and retains thin scene-depth coverage before composition. The captured
2x2 inputs and matched shipped code support the mechanism; attribution of every damaged aircraft
pixel still needs game validation. Internal amortization/checkerboard sampling remains native.
The [hook map](ue418-hook-map.md) records the DLL fingerprint, guard, replacement bytes, named
functions and lifetime. No reference shader/source is copied into first-party code.

The second native change hooks GetSceneColorFormat at AC7 RVA0x109f300. Desktop allocation and
shader-parameter consumers receive PF_FloatRGBA, so lighting and sky write FP16 from the start.
Previous experiments widened already-quantized R11 captures and did not test producer precision.
The family allocation owner at RVA0x1095480 can retain existing targets without asking the getter.
A second expected-byte patch at0x1095620 supplies effective scene-format policy4 before cache
comparison. Native UpdateRHI handles reallocation at unchanged dimensions, avoiding a stale packed
target after activation. Its bytes, function name and lifetime are also recorded in the hook map.
This increases scene bandwidth. Full scene-grid cloud production also costs more GPU work,
including SR-off while the native plugin is active.

The DLSS-specific correction belongs to the orchestrator. A GPU pass projects reconstructed
colour over each current source-pixel footprint, using actual dimensions and producer jitter in
render pixels with Y down. It applies three interpolated RGB residual corrections, bounded by
the source neighbourhood and existing reconstructed value. Depth discontinuities suppress
correction over silhouettes. The bounds prevent added highlight peaks and dark ringing without
clipping the source's HDR range; alpha passes through. It runs after DLSS, before native bloom
and tonemap consumption, for every DLSS mode. No preset override, replacement tone curve or
display-HDR requirement. Internal DLSS exposure remains unproven; this corrects observed output
colour drift rather than claiming a proprietary-model root cause.

The pass owns transactional dimension/format-dependent GPU resources and immediate-context state.
Wrong device, mismatched depth, unsupported format, allocation or compilation failure refuses
replacement and retains the native graph. Compilation failure is remembered until pipeline restart.
F9 also writes `_output_uncorrected`, paired with corrected output. No readback runs for normal
gameplay correction.

Explicit GPU experiments reuse the frozen hangar input in the
[colour-domain note](ac7-ui-hdr-20260930.md). Performance default broad bands visibly disappear
after correction; numeric RGB range remains0.00019204617..482.75 and alpha is identical. Source
projection RMSE decreases3.02905 to1.17734, a consistency measure rather than native-image MSE.
Flight Ultra uses the latest533x300 colour and captured depth, output1600x900, 60 evaluations,
identity camera, zero motion/jitter and first-frame reset. Its outer silhouette retains reconstruction
after depth guarding; values remain finite, alpha identical, maximum1.73046875 unchanged.
Producer cloud/aircraft damage remains in that frozen input. This replay cannot test new production.

Rejected variants remain local: unconstrained additive residual caused dark ringing; RGB ratios
increased highlight peaks; pure colour-contrast guarding preserved boundaries but left broad
aircraft bands. Depth guarding targets the observed silhouette regression. Single GPU timestamps
are0.6205 ms for flight and1.4991 ms for the hangar at1600x900 on the current NVIDIA adapter.
These are standalone runs, not AC7 frame-time guarantees. The diagnostic display curve is only
used to view linear captures, never applied in game.

Game acceptance requires the installed build: inspect the cloudy aircraft and distant clouds
with SR off, then DLSS Performance and Ultra, taking F9 after each mode settles. Confirm cloud
constants report divisor1 and scene colour is actually format10; compare paired composite samples
and corrected/uncorrected DLSS output; check sustained temporal stability. Motion-coverage work
resumes from existing research after these three blockers are visually resolved. No autonomous
game launch or new test target.

Installed19:10, backup `.local/deploy-backups/ac7-pair-20261002-191007-200`. Final MSVC Release
full gate passes33 executed tests with five vendor/environment skips; Rust format/clippy pass.
Actual overlay Insert, rendering and resize preflight passes. Proxy/plugin/overlay deployed hashes
match the build artifacts. No AC7 launch. These checks do not replace moving-game acceptance.

## Flight regression: native depth pass omitted

The user confirms clean aircraft and correct hangar exposure after19:10. Both new flight sessions,
`motion-20261002-191851-15564-1` and `motion-20261002-191902-15564-2`, instead show broken
sky/lighting and missing volumetrics. Cloud inputs now534x300 confirm the full grid; native scene
colour is format10. The sky is already almost black before SR, while corrected and uncorrected
DLSS closely retain it. This rejects DLSS colour correction as the origin of this regression.
Near/far cloud inputs have almost uniform RGB and alpha about0.9995; the user reports no volumetrics.

Walking further into the shipped depth producer, RVA0xbfb30, revealed x2/x3/x4 compute-pass
selection only. Divisor1 bypasses constant upload and effect Apply, yet still Dispatches and
Unapplies. Allocation arithmetic accepting1 was insufficient evidence. The earlier change was
incomplete and introduced this regression. Counter underflow is a control-flow consequence of
the unmatched cleanup, not a directly measured counter value.

The correction adds the missing one-to-one depth permutation as first-party game-plugin code.
A guarded conditional includes1 in native x2 pass setup; a guarded native Dispatch call invokes
the point kernel only on matching full-grid resources. It consumes the same CB11/t1/s15/u0 ABI
and normalized radial-distance conversion, preserving near/sky conventions, source offset,
projection and padding. Unlike a two-by-two reduction, each pixel retains its own depth bounds.
Native allocation, binding, Apply and Unapply remain balanced. The original shader is restored
before cleanup; native downsampling forwards unchanged. All sites/fingerprints are in the hook map.

Full resolution waits for successful shader creation and verification of actual native bindings.
Failure leaves a supported native divisor rather than dispatching an unprepared full-grid pass.
The relay data page and shader/device references are owned through quiescent teardown. No global
Dispatch hook, CPU depth readback or change to the accepted colour correction/FP16 allocation.

The standalone NVIDIA GPU experiment `.local/probe-truesky-depth-20261002.cpp` binds the original
shipped x2 kernel and the new x1 kernel against352-byte constants, R32 source depth, point-clamp
sampler and a typed array UAV. It exercises sky0, near1, varying geometry and a one-pixel column.
GPU output matches the respective normalized max/min/range calculations within1.86265e-9 for x1
and9.31323e-10 for x2. This proves the new GPU shader's ABI/math and the interpretation of x2,
not execution through the game patch or restored live volumetrics. No new CTest target or AC7 launch.
The next game run must confirm cloud depth dispatch, nonempty volumetric colour and lighting,
while keeping the aircraft and hangar corrections.

Installed20:05, backup `.local/deploy-backups/ac7-pair-20261002-200538-588`. Final Windows MSVC
Release gate passes33 executed tests, five vendor/environment skips; Rust formatting/lint pass.
Actual overlay Insert, pixel rendering and resize preflight pass. Installed hashes match build.
No game launch. Required user run: same flight view with volumetric clouds, F9 after settling;
check sky/lighting/clouds and that aircraft stays clean, then check hangar exposure.

## Wing masking and cloud floor, after the20:13 retest

The user still sees stationary gaps on thin aircraft surfaces and blocky distant clouds below
Balanced. The parent conversation transcript was reread, including the19:10 regression and20:05
delivery, before implementing this follow-up. The two20:13 captures still have268x152 cloud
colour and the stock x2 depth shader. The log reports shader compilation but no verified bindings
or one-to-one dispatch. This run tested the supported fallback, not the new full cloud grid.

The failed activation is explained by native allocation. The20:05 guard required Texture2DArray
because the shipped compute shader declares an array UAV. Its actual texture owner at DLL
RVA0xeae10 creates a Texture2D UAV. The replacement shader now matches that view directly and
the guard validates the real two-channel resource. Bounded refusal messages include dimensions,
view types, formats and CB size, so another refusal cannot silently resemble a successful patch.

A second loss occurs earlier in depth production. The scene-depth-bounds owner at DLL RVA0xbfb30
requests native format0x16, which maps to R16G16_UNORM. Captured CB196 contains conversion
parameters `(0.1,150000,0,0)`. Its range gives only65535 representable normalized depth steps,
about2.289 native distance units apart. CPU comparison against the captured full scene depth finds median3.215%
and maximum7.683% relative error over aircraft-depth pixels.384 of3185 occupied x2 blocks mix
aircraft and background samples. Both precision and coarse coverage therefore need attention.

The expected-byte format immediate at RVA0xbfc09 now requests enum5/R32G32_FLOAT. This changes
only normalized scene-depth bounds, through the native owner that rebuilds resource and views
together. It preserves normalized units, saturation and near/sky conventions. The one-to-one
kernel retains each scene pixel's own maximum/minimum distance. Consumers continue sampling
floating-point bounds. Exact bytes, owners and consumer sites are in the hook map.

Native cloud allocation still coordinates colour, depth, history and tile sizes. Ultra at1600x900
now requests534x300 padded clouds once the corrected binding gate passes. That exceeds the
stock Balanced cloud resolution without raising opaque scene resolution. A post-SR cloud move
was considered but is not part of this change: atmosphere, cloud lighting and depth would also
need consistent reinsertion before bloom/tonemapping. The earlier implementation is completed
at its existing producer instead. This does not claim the stationary wing pixels are a motion
vector defect. Live visual acceptance remains required with clouds present.

The focused GPU experiment reuses captured536x300 scene depth and CB196/352 bytes on the
RTX4070 Laptop GPU. Native x2 at268x152 and replacement x1 at534x300 both execute correctly
with RG32F; a538x302 padded x1 target also passes. Maximum absolute error against the CPU
floating-point reference is1.78814e-7. Candidate aircraft relative error is at most3.22694e-7
(0.00003227%), versus up to8.57% in actual UNORM GPU output. The earlier7.683% figure was a
CPU round-to-nearest estimate; this GPU result measures the device's storage conversion instead.
Texture2D and one-slice array views produce identical values on this device, but the shipped
factory and replacement now agree explicitly on Texture2D. This is not a live cloud render.
Replay source and outputs are under ignored `.local/probe-cloud-depth-actual-20261002.cpp`
and `.local/cloud-depth-actual-534x300-20261002.log`.

Device recreation rebuilds the replacement shader on its new owner. If bindings change after
full-grid activation and cannot be validated, the callback restores the supported divisor for
the next native frame and suppresses the incompatible dispatch. It never forwards native x2
over already-full-grid targets on that failure path. Native Unapply still runs; the rejected
frame loses its depth update. Refusal stays announced and full-grid reactivation is disabled.

Installed21:24, backup `.local/deploy-backups/ac7-pair-20261002-212445-314`. Final Windows MSVC
Release gate passes33 executed tests with five vendor/environment skips; Rust format/clippy and
actual overlay Insert/pixel/resize checks pass. Installed proxy/plugin/overlay hashes match the
artifacts. AC7 was not launched. User validation: restart Steam normally to remove the still-loaded
Nsight capture modules, launch AC7 normally, inspect the wing/cloud overlap at Balanced and Ultra,
and press F9 after each setting settles. Confirm clouds/lighting remain present, depth output is
DXGI16 and full-grid dispatch/534x300 cloud inputs are recorded. Visual acceptance remains pending.
