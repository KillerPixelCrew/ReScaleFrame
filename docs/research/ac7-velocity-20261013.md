# AC7 temporal velocity, 13 October

## Coverage capture correction, 14 October

The latest capture, `motion-20261014-201536-44500-1`, is a briefing screen. The earlier complete
flight capture has four sampled mesh-particle draws with actual buffer SRVs bound at VS slot 127.
That confirms the queued previous-transform binding reaches game draws, not the contents or
accuracy of those transforms. The stride-8 draws were initially considered GPU sprites, but
their retained input signatures have float4 position/old-position attributes and corner UVs:
they are instanced CPU sprites already handled by counter/corner keys. Stride alone was misleading.

The same flight capture has no particle geometry snapshots. The recorder shared 24 buffer reads
across early translucency draws and copied every bound IA slot, including stale slots not used by
the layout. Early generic geometry therefore hid later producers. Capture now reads declared
slots only, reserves separate bounded budgets for generic geometry, sprites, mesh particles,
curves and native RG16-UNORM velocity-target candidates, and avoids repeated particle shader/layout
samples within each sampled interval. Every read remains capped at 16 KiB; at most 52 buffer reads
are allowed per sampled interval. These categories select diagnostic candidates, not runtime
rendering support. Native-role/frame/primitive/material identities accompany the snapshots.

Mesh-particle samples also copy the actual VS 127 buffer view; native velocity candidates sample
buffer views at VS 0/1/127 for bone/history inspection. SRV offsets and ranges are respected.
Readback temporarily disables and restores predication, so a suppressed draw cannot leave a false
empty staging sample. WARP checks exhaust generic reads before a late particle, exclude an unused
slot, preserve predication, and compare a sliced previous-transform buffer byte for byte.
The new recorder has not yet collected the affected gameplay data. Weapon/vehicle/kill-cam and
particle visual coverage remain open; no speculative GPU-sprite producer change was added.

MSVC Release verification passed: 45 native tests, seven opt-in skips, formatting and Clippy.
The recorder update was deployed with proxy/overlay backup
`.local/deploy-backups/ac7-pair-20261014-204634-162` and dependency backup
`.local/deploy-backups/ac7-sr-20261014-204633-809`. No game launch or termination was performed.

## Ocean-cutscene dips and remaining work cost, 14 October

Game acceptance: after testing deployed `0af8c9e`, the user replied "Fully fixed now" to the
same port-to-ocean cutscene retest. This closes the reported cutscene performance regression;
cinematic FG had already been confirmed working. It does not establish visual velocity coverage
for carrier/refuelling weapons, moving vehicles, kill-cam or every particle/translucent producer.

The user reports that the buffer-bandwidth build is much better, with remaining dips into the
30s during the introductory camera pullback from the port to ocean, ships and approaching planes.
The new log contains ordinary flight samples around 70-80 rendered FPS and cinematic samples
above 80, with intermittent frame-time spikes. No new F9 capture covers the reported dip, so
shader compilation remains a candidate, not a confirmed attribution of those frames.

The material variant key includes native PS and layout identity. VS/GS/SO objects were shared,
but identical PS/HS/DS code still caused fresh device shader creation for each variant. These
stages now share exact-bytecode objects on the owning device too. A native tessellation fixture
switches to a separate material shader object containing identical code; all motion/depth checks
still pass and the count of actual variant shader creations stays unchanged. Genuinely new
shader code still needs its first device creation. Slow-frame CPU summaries distinguish shader
preparation, resource preparation and total callback cost, and report shader/model counts.

Fixed-emission VS/patch draws now use actual SO record counts. Validated GPU indirect arguments
also produce record and dispatch counts in the private argument buffer, without CPU readback.
This avoids scanning unused indirect capacity and removes zero initialization/copies for these
routes. Native variable GS emission and DrawAuto retain their bounded zero-tail route. Hash
dispatch uses two dimensions when needed to respect the D3D11 per-dimension group limit.
Native predication applies to history insertion as well as capture/raster, with state restored.
Microsoft's [SetPredication contract](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-id3d11devicecontext-setpredication)
covers Dispatch and DispatchIndirect as well as drawing.

WARP fixtures cover an emitter shrinking through an indirect draw, a returning particle without
history, a predicated-out draw, and active indirect beam/ribbon/animated-trail strips. The retained
native shader/tessellation corpus passes. A separate RTX 4070 Laptop reproduction uses ten
eight-vertex indirect draws with a 64,000-vertex bound. Across samples 20-49 the median GPU time
is 6.08 ms for `a014d94` and 2.31 ms for the exact-count path. The new run also has a slow initial
phase, including a 17.67 ms sample in that window; this benchmark does not establish elimination
of cold-start or game cutscene dips. Raw logs are private under `.local/ac7-20261014/performance`.

MSVC Release verification passed: 45 native checks, seven opt-in skips, formatting and Clippy.
The complete producer fixture, including the material-alias creation count, also passes on the
RTX 4070 Laptop GPU. The proxy and overlay were deployed with matching dependency backups:
`.local/deploy-backups/ac7-pair-20261014-195939-253` and
`.local/deploy-backups/ac7-sr-20261014-195938-743`. The game was not launched or closed; the
ocean/approaching-aircraft scene was subsequently confirmed fixed in the user-run retest above.

## Per-draw buffer bandwidth regression, 14 October

The user confirmed that cinematic FG works in the deployed compilation/classifier build, but
reported roughly 10 rendered FPS versus 70 before the velocity work. The new flight log spends
about 60-70 ms in Present while SR evaluation remains below 1 ms. F9 sampling itself adds
synchronous readback cost, so sampled native-scope durations cannot stand in for steady GPU time.

Source inspection found that one large draw grows shared SO storage and every later small draw
clears and copies the whole allocation in both directions. A private hardware reproduction warms
storage with an empty indirect draw bounded to 64,000 input vertices, then submits 100 eight-vertex
draws per frame. D3D11 disjoint/timestamp queries measure the work after shader/allocation warm-up.
The first run used the default Intel UHD adapter: the old path took roughly 410 ms, versus 14 ms
with bounded transfers. Selecting the high-performance adapter explicitly gives the RTX 4070
Laptop GPU (`10de:2860`): the committed old pass takes 13.15-15.10 ms over frames 2-11, and the
final corrected pass takes 3.34-3.49 ms. These are standalone workload measurements, not game FPS.
Baseline source is `23c7236`; logs and reproduction sources are private under
`.local/ac7-20261014/performance`.

The correction grows scratch storage geometrically and initializes a zero source once per
allocation. Each draw zeros only its bounded SO range with one GPU region copy. SO output now
has a raw read-only SRV, so the history compute pass reads it directly. The earlier failed
SO/UAV combination did not imply that SO/SRV was unsupported; both hardware and WARP accept the
read-only binding and the complete producer checks pass. Native variable emissions, indirect
bounds and predication keep their prior zero-tail behavior. No particle or material route is
disabled. The regression fixture grows shared storage with an empty indirect draw before checking
small moving, clipped and resampled curve draws. Live performance recovery still needs a rerun.

MSVC Release verification passed: 45 native checks, seven opt-in skips, formatting and Clippy.
The retained native material/tessellation corpus also passes. The corrected proxy and overlay
were deployed to the established AC7 installation with backups under
`.local/deploy-backups/ac7-pair-20261014-185941-827` and matching SR dependencies under
`.local/deploy-backups/ac7-sr-20261014-185941-387`. The game was not launched or closed.

## Rendered cutscenes and compilation stalls, 14 October follow-up

The new run reported 1-2 FPS stalls around scene changes and no FG in rendered cutscenes.
Extracted capture 5 is the hangar cinematic, 6 is the mission-opening cinematic, and 7 is flight.
The same-run log rejects the mission cinematic with screen 0/detail `0x12006`: the possessed
player plane and Engine CameraActor are recognized, but the classifier requires target == pawn.
The adapter now classifies that exact known combination as a rendered replay. Pause, pending
camera targets, camera-owner changes and native cut resets keep their existing behavior. An
independent native-layout fixture calls the actual classifier across these transitions.

The log also records multi-second gaps before Present while SDK evaluation remains near 1 ms.
Source review found repeated `D3DCompile` in the velocity geometry callback: each VS/PS/layout
variant compiled the same large unrolled history algorithm with a different varying declaration.
The pass now compiles one fixed interface and links native inputs/outputs and packed varying
copies directly into DXBC. A regression check proves material/interface changes cause one HLSL
compilation per process, not one per variant. Identical vertex, raster GS and SO shader objects
are also shared by bytecode within the owning device. Existing WARP motion/depth/opacity and
retained-signature validation pass with the linked variants. No producer coverage is disabled.

MSVC Release verification passed: 45 native tests and seven opt-in vendor skips, formatting
and Clippy. These checks confirm the source correction, not restored game FPS; the updated
build needs a new live run to measure residual GPU/replay cost and cinematic FG activation.

The seven new F9 captures are `motion-20261014-174422-38604-1` through
`motion-20261014-174913-38604-7`. Capture 6 contains 34 accepted components whose retained
current/previous transforms differ, and packed velocity has nonzero moving samples in all three
sampled frames. Joining draw command scopes to native role 18 identifies 96 sampled material
records and 11 VS/PS pairs in that capture. This proves producer selection and buffer writes,
not correct motion on every visible object. Capture 4 shows stationary weapons in the hangar;
it does not establish moving-vehicle coverage. Capture 7 ended without the CPU/draw JSONL files.

The captured material corpus supplies 13 ordinary VS, 16 PS and a complete matching native
12-input-control-point VS/HS/DS chain. WARP accepts their transformed shader objects. The
retained-chain fixture now loads the actual native VS when supplied, replacing its earlier
synthetic interface for this check. An initial ordinary-VS check refused that tessellation VS
because it has world-position output; the matched tessellation route succeeds. No production
fallback or coverage restriction was added. Private extraction/statistics are under
`.local/ac7-20261014/coverage`; the original captures were read without modification.

## Beam, ribbon and animated-trail producers, 14 October

The correspondence defect was at CPU production: fills clear ParticleId and can duplicate
current into OldPosition. Draw-index history then renames geometry when a trail grows or changes
subdivision. Searches restricted to 80-byte increments found unrelated debug/physics/container
code. Paired 160-byte writes and chained unwind owners located the beam fills; reads of the
asynchronous header's vertex pointer located the trail fills. Ghidra control flow and layouts
match 4.18.3 `ParticleSystemRender.cpp`, `ParticleHelper.h` and the beam/trail factory. The five
functions are named in the existing database; the installed executable still matches the hash below.

| Owner | RVA | Guarded first 16 bytes |
| --- | --- | --- |
| Beam no noise | `0x190e740` | `4c8bdc5553565741564157498dabd8fe` |
| Beam noise | `0x1904d40` | `4c8bdc55498dabd8fcffff4881ec2004` |
| Beam interpolated noise | `0x1907980` | `4055488dac24c0fbffff4881ec400500` |
| Ribbon fill | `0x190d510` | `4055535741544157488dac2450feffff` |
| Animated trail fill | `0x190ce60` | `48894c24085553574157488dac2438ff` |

Hooks forward the original fill, then tag CPU-owned vertices before asynchronous upload. Header
count/stride/pointer are +0x88/+0x8c/+0x90. Beam count/stride/data/payload/sheets are
+0x44/+0x48/+0x60/+0x12c/+0x154; payload triangle count +0x58 bounds `(triangles+2)*sheets`.
Trail count/stride/data/indices/payload are +0x64/+0x68/+0x80/+0x88/+0x138; payload flags carry
14-bit links and +0x1c gives interpolation count. Particle flags +0x5c carry the 26-bit counter.
Complete ranges preflight; bad counts or cyclic links invalidate stale tags.

Three unused fields in the matched 80-byte stream carry counter pairs, subtype, sheet and sample
fraction as normal-float bit patterns. Used geometry/material fields remain native. A private
layout widens old position to read its fourth word; the colour layout is restored unchanged.
Descriptor/stride and shader use masks select this route. GPU history interpolates bracketing
prior rendered samples and stores endpoint aliases, preserving existing segments under new heads.
Triangle strips have a bounded SO expansion. Existing guards cover worker lifetime; no continuous
readback or CPU pointer crosses the queued rendering boundary.

Native fixtures check all five detours, exact tags, used-field preservation, bad counts and cycles.
WARP checks curved resampling, new heads, order, factory reuse, clipping and cuts through the full
pass. This is source and synthetic evidence; no new AC7 run or gameplay acceptance is claimed.

MSVC Release `eng/verify.ps1 -Configuration Release -VS2026` passed: 45 native tests passed,
seven opt-in vendor tests skipped, formatting and Clippy passed.

## Tessellated material velocity

The dedicated material pass now handles native triangle tessellation, including the 12-input/
3-output control-point form retained from AC7. It appends stable primitive/particle keys to the
VS output and relays them through the native hull control phase. Optimized identity hull shaders
can omit that phase; the variant materializes it and copies fields by semantic, preserving native
register packing. The domain variant retains the native material and final displaced depth.

UE4.27.2 `VelocityShader.usf:35-99` interpolates current/prior control-point clip coordinates
through tessellation. The new path follows that correspondence strategy: a private integer/factor-1
hull variant captures three native patch-corner positions, and the normal domain variant evaluates
those corners and interpolates their current/prior motion at its native domain sample. The normal
draw retains its native tessellation factors and partitioning. Changing adaptive sampling therefore
does not require the prior frame to have evaluated the same interior domain point. Interior-only
displacement changes have no separate history; motion uses corner interpolation.

Corner capture has a fixed bound independent of tessellation density. The engine mesh bound is
three vertices per primitive; indirect patch validation converts it using the actual input control-
point count. Capture, lookup and retirement remain on the immediate GPU stream without CPU
readback. Domain bytecode supplies the SO signature through the documented
[D3D11 previous-stage stream-output contract](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-id3d11device-creategeometryshaderwithstreamoutput).

The WARP fixture passes changing fractional-even density, independent VS/domain deformation,
opacity-qualified depth, camera cuts, 12-control-point direct/indirect continuity and native stage
restoration. The retained PN HS/DS blobs also accept the variants with matching synthetic VS
inputs; the retained capture has no matching native tessellation VS creation blob. Initial checks
found subnormal corner identifiers being lost and implicit hull passthrough mapping added fields
by register rather than semantic. Normal float corner identifiers and an explicit keyed control
phase corrected those failures. Temporary GPU readbacks used to diagnose them were removed.

The pass also preserves UE4.27's AlphaComposite behavior: its opacity is not clipped by the
translucent/additive/modulate cutoff. A full producer fixture checks below-cutoff AlphaComposite
coverage separately. Combined custom GS/tessellation, multiple GS streams and GS instancing
remain refused. Gameplay acceptance and the broader affected-scene coverage audit remain open.

MSVC Release `eng/verify.ps1 -Configuration Release -VS2026` passed with 45 native tests,
seven opt-in vendor skips, formatting and Clippy. Retained shader validation also passed.

## Native geometry-shader producer

The material pass now retains single-stream triangle geometry shaders and records their final
emitted positions. Each emission carries a key formed from the complete upstream triangle keys
and emission ordinal; native material varyings and strip boundaries remain intact. Mesh-particle
lookup still uses previous-transform keys, and CPU sprites retain their particle/corner identity.
The shader's declared output bound sizes stream-output storage, including strip expansion; GPU
indirect argument validation continues to use the input mesh capacity. Native GS identity also
separates model history. Multiple streams and GS instancing currently refuse.

The retained 4 October capture contains one GS, HS and DS creation, with no recorded draws using
them. Disassembly shows a triangle/array-slice forwarding GS and a PN-triangle domain shader.
That inventory establishes shader presence, not gameplay coverage. The new WARP fixture changes
both VS translation and independent GS deformation, checks their combined motion, opacity
clipping, camera-cut invalidation and restoration of the native shader. Initial execution exposed
an invalid read from a DXBC output register; retaining the native position in a temporary before
emission fixed it. Shader creation alone had not caught that defect.

MSVC Release `eng/verify.ps1 -Configuration Release -VS2026` passed: 45 native tests passed,
seven opt-in vendor tests skipped, plus formatting and Clippy. This is build and synthetic GPU
evidence, not AC7 gameplay acceptance.

Tessellation still needs a previous-surface route, including correspondence when adaptive domain
sampling changes. The pass continues to refuse it. Affected gameplay remains untested; the full
motion goal is active. The sections below record preceding increments and their then-current gaps.

## Mesh-particle correspondence

The instanced mesh-particle path now keys stored generated geometry by the complete instance
transform and vertex ID. Lookup uses the native previous transform for that particle in the
current sorted order. It does not assume that SV_InstanceID or a pooled factory address stays
fixed across frames. Unavailable previous-transform data uses camera motion at the fragment's
own depth, without selecting another particle's pose.

Native/source matching located `FMeshParticleVertexFactoryShaderParametersVS::Bind` at RVA
`0x1874330` through the `PrevTransformBuffer` and `PrevTransform0` UTF-16 references. Its vtable
at RVA `0x2dac808` identifies `SetMesh` at `0x188cfa0`, entry bytes
`48 89 5c 24 08 48 89 6c 24 10 48 89 74 24 20 57`.
Ghidra decompilation confirms the factory uniform at +`0x488`, previous-transform SRV at +`0x4a0`,
and shader resource/+`0x48` -> native vertex shader/+`0x38`. The matching 4.18.3 source is
`MeshParticleVertexFactory.cpp:50-88,269-271` and `MeshParticleVertexFactory.ush:185-205`.

The guarded producer hook queues an extra reserved-slot binding. It copies and retains the RHI
shader/SRV references; execution uses the actual RHI command-list context and its vertex SRV
method at virtual offset `0x1d0`, corroborated by the native command executor at `0x120ccd0`.
References retire through the renderer's existing owner-thread mechanism. Stop waits for pending
bindings. The game-owned source resource remains native; no CPU/GPU transform readback is needed.

The complete WARP test now reverses two mesh instances, moves them independently and replaces
their factory address. The foreground motion still follows its own previous transform. The
native graph fixture separately calls the actual producer hook, executes its queued binding and
checks exact retention/retirement. These are source, queued-ABI and synthetic GPU checks; no new
AC7 run was made. Native geometry/tessellation-stage input routes still remain unfinished.

Release verification passed 45 native tests, with seven opt-in vendor skips, plus formatting and
Clippy. A later deployment rebuild found Visual Studio 18.10.4 marked incomplete/reboot-required
by the Installer. The existing build recovered through CMake's documented local instance
specification using build number `18.10.12306.391`; no installation/global settings changed.

## Indirect arguments and sorted particle coverage

The next source increment adds indexed and nonindexed indirect replay with the exact borrowed
argument buffer and byte offset. Graphics ABI 15 preserves that draw contract; game ABI 16 carries
a copied native mesh-element vertex bound. In matched 4.18.3 `MeshBatch.h`, and AC7's previously
matched 0x60-byte element loop, NumPrimitives and NumInstances are at +0x34/+0x38. GPU-sprite
submission in `ParticleGpuSimulation.cpp:2962-2963` supplies both. The producer sums selected
elements with overflow checks rather than estimating capacity from a texture or draw order.

Arguments are copied into private GPU indirect storage and validated there against the native
bound. Both motion draws consume that same GPU snapshot; no CPU Map/readback or per-draw fence
is introduced. An oversized submission produces no partial velocity writes. Direct and indirect
history share vertex identity when their geometry corresponds.

The complete WARP producer fixture now covers indexed/nonindexed indirect calls, a nonzero
argument offset, capacity refusal, direct/indirect continuity and cuts. The actual frame-tap hook
fixture verifies the borrowed buffer/offset and distinct indirect kinds. A two-particle fixture
reverses draw order, moves the foreground particle and changes its pooled vertex-factory address;
the prior billboard still matches by particle counter and corner. These are synthetic results,
not new flight acceptance. GS/tessellation and mesh-particle instance correspondence remain open.

## Later source increment: dedicated material velocity/depth producer

The separate pass is now implemented and wired to AC7's static/dynamic translucency DrawMesh
owner, enabled with the native reconstruction configuration. ABI 15 carries copied primitive,
vertex-factory and material identity, element mask, blend mode and opacity clip value. Native
colour draws are forwarded unchanged. Their queued scope reaches the actual D3D11 geometry
callback; it does not read later CPU mesh/view state.

The pass reuses the game's exact material and vertex shader instructions in independently
validated variants. Standard material evaluation and discards remain. For additive base-pass
shaders, the guarded final scalar-times-RGB operation supplies opacity even though output alpha
is zero. The pass clips against the copied material setting plus `1/255`, writes near-or-equal
depth, and writes packed motion. Other shader shapes refuse with bounded diagnostics.

Current generated clip positions are recorded with stream output. CPU sprite keys include their
stable particle counter and corner UV, so sorting need not substitute a different particle's
history. The previous generated geometry carries billboard, vertex deformation and skeletal
pose as actually rendered. View/native-frame continuity, camera cuts and inactive-model retirement
control history. Missing or ambiguous geometry history uses the native camera transform at this
fragment's own depth. Stream-output storage and shader-readable raw storage are separate D3D11
buffers, copied on the same GPU stream; the combined allocation was rejected by WARP.

The material pass commits its motion/depth into the leased engine allocations from a queued
marker at the start of native post-processing, before its consumers execute. It leaves native
colour composition intact. Current and previous clip coordinates use matching interpolation;
deriving current motion from rasterized SV_Position introduced a measurable rounding error and
was corrected. Valid zero motion remains encoded as the native bias, never the clear sentinel.

New WARP checks exercise the complete generated-geometry/history/opacity/depth/commit path across
two moving frames and a camera cut, preserving clipped opaque motion and native shaders. All
retained candidate flight VS/PS blobs used in this investigation accept the generated shader
variants, including packed particle varyings. The current Release gate passes 45 native tests,
with seven opt-in vendor tests skipped, plus formatting and Clippy. This remains synthetic and
build evidence. No new AC7 game run was performed.

Still unfinished: native GS/tessellation and indirect geometry need their own input route; the
current triangle-list path explicitly refuses those cases. Sprite sorting/emitter identity,
mesh-particle instance correspondence and affected carrier/refuelling/vehicle/kill-cam scenes
still need broader producer validation. These are remaining implementation/coverage work, not a
claim that a successful build finished the full goal. Earlier notes below describe the first
increment and are retained as history.

The requested outcome includes missing weapons/attachments, moving vehicles, animated meshes,
kill-cam, particles and a complete translucent velocity/depth pass. This increment repairs native
opaque eligibility and prepares the missing particle-stream evidence. It does not complete that
outcome or port the translucent pass.

## Native opaque producer

The existing captures measured small-object rejection and unchanged root transforms. The native
velocity shaders already carry previous primitive transforms and skeletal buffers. Removing their
motion-blur selection restrictions retains those producers instead of inventing object motion
from depth. Source: UE4.18.3 `0a14a8d537a31ecc77488ced41dbaa0166612ef8`,
`VelocityRendering.cpp` and `PrimitiveSceneInfo.cpp`. Executable SHA-256:
`c7da97f5f8a807d4f1264adbb074146fcffe9bdc2ffa98791b822cd28e558f4f`. Decrypted dump:
`fafd1db2808d32333caecf4056e8bcd676c2f07e4c1fabc0f270ef29f22d24db`.

Capstone checked the retained dump again. Read-only Ghidra decompilation of the existing AC7
database confirmed `ShouldRenderVelocities`, `HasVelocity`'s consumers and native mesh history.
Private evidence is under `.local/ac7-velocity-20261013`. No new gameplay was run.

| RVA | Native bytes | Replacement | Behavior |
| --- | --- | --- | --- |
| `0x118a4e3` | `75 0e` | `90 90` | Consider velocity when native TAA is off; keep the camera-cut test immediately afterward |
| `0x10fdd0c` | `76 38` | `90 90` | Retain small visible movable opaque primitives |
| `0x11838c4` | `77 88` | `eb 88` | Retain unchanged-root movable primitives after a successful native history lookup |

The last change deliberately follows history lookup at `0x1183869` and missing-history rejection
at `0x1183870`. It does not force `AlwaysHasVelocity` at `0x118384c`, which would bypass history.
Visibility, mobility, main-pass/material relevance, shader support and previous-geometry production
remain native. This does not manufacture absent previous bones or turn repeated engine cuts into
valid history. Kill-cam still needs a same-build capture to distinguish pass selection from cuts
and absent geometry/history.

The group preflights all sites before writing. A failed write attempts guarded rollback. Stop
restores owned bytes after quiescence and refuses foreign replacement bytes. The native graph
fixture checks mismatch rejection, complete installation, idempotence and guarded restoration.
Capture hooks recognize the owned size-gate replacement without weakening the unowned guard.

## Full translucent pass remains required

The checked UE4.27.2 reference is `d94b38ae3446da52224bedd2568c078f828b4039`.
`FTranslucentVelocityMeshProcessor` retains even nonmoving geometry because this pass writes
depth. It uses depth writes with near-or-equal comparison. `MaterialTemplate.ush` clips
translucent/additive/modulate opacity against the material clip value plus `1/255`.
These semantics are absent from the experimental AC7 blend-gate patch, which remains opt-in.
The new opaque patch group does not admit translucent batches.

UE4.18 `BasePassPixelShader.usf:1099-1100` writes additive RGB multiplied by opacity but writes
zero output alpha. A generic replay reading output alpha cannot reproduce the material's opacity
clipping. Likewise, the sprite previous-position helper returns current geometry despite an
`OldPosition` stream. Enabling eligibility alone solves neither problem.

The native translucency DrawMesh owner was matched at RVA `0x1158250`, entry
`4c 89 4c 24 20 4c 89 44 24 18 48 89 54 24 10 48`. Its unwind chain includes
`0x11582c9..0x115840d`; the earlier scale consumer was a function fragment. Read-only Ghidra
decompilation confirms the material query through mesh+`0x98`, feature level view+`0x13e8`,
blend query at material virtual offset `0x198`, vertex factory mesh+`0x90` and proxy argument.
That owner is located, not hooked by this increment.

## Capture inputs needed to complete coverage

F9 now records actual input-layout elements and up to 24 bounded vertex-stream reads per sampled
interval inside the native translucency execution scope. Each stream records its buffer offset,
stride, allocation size, sampled bytes and blob identity, alongside shader/draw/view identity.
Reads occur synchronously while the draw's buffers still hold that geometry. `cb_<id>.bin` blobs
remain deduplicated and budgeted; geometry records explicitly identify their vertex-stream use.
There is no continuous vertex readback outside the opt-in capture.

The source sprite stream stores a stable particle counter in OldPosition.w and previous centers
in OldPosition.xyz. Fresh affected-scene captures must establish AC7's actual declarations,
emitter identity, billboard/deformation history and opacity producer. The earlier captures did
not retain vertex streams. Needed scenes are carrier launch/refuelling, moving ground vehicles,
animated aircraft/kill-cam, and smoke/contrails or other additive/translucent effects.

F9 motion summaries now select the packed-velocity and decoded-motion views rather than RAW.
Packed clear zero and decoded sentinel are unwritten; decoded zero is valid. Lossless raw files
remain the original GPU values.

## Validation

MSVC Release `eng/verify.ps1 -Configuration Release -VS2026` passed: 44 native tests passed,
seven opt-in vendor tests skipped; Rust formatting and Clippy passed. The WARP capture fixture
verifies exact particle-layout attributes and a 96-byte vertex-stream readback. These are build
and synthetic results, not weapon, vehicle, particle, kill-cam or translucent-pass game acceptance.
The shared-motion and Project Wingman changes already present in the working tree were preserved.
