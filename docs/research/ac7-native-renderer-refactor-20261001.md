# AC7 native renderer refactor, 1 October 2026

Research history: findings, hook addresses and pending statuses below apply to their recorded
experiments. Later increments can supersede earlier conclusions. See
[current implementation and validation](../current-status.md) before using this as a feature list.

Current status, 2 October: the user accepted the corrected native SR/UI path, hangar crash and
wing/cloud result, followed by the GPU-fence bridge/FSR4 compatibility deployment. The build-only
and pending notes below are the dated development trail. Later corrections preserve exact active
backend rectangles inside padded allocations, route matched SceneColorHalfRes bloom/exposure
through SR, and use the verified native TrueSky depth producer. See [stability](ac7-stability-20261001.md),
[aircraft/cloud corrections](ac7-plane-artifacts-20261002.md) and
[final SR acceptance](sr-interop-performance-20261002.md#user-acceptance-and-release-replacement).
This acceptance does not complete missing-object motion coverage, FG surfaces or latency markers.

The question is how to stop the UI from briefly using a stale size or low-resolution image after
pause/menu transitions, and how to make SR integration follow engine ownership. Repeated texture
promotion, draw classification and Present-based rediscovery were selecting allocations rather
than graph roles. The September captures also showed that queued RHI work outlives CPU pass scopes.
The native graph is the intervention point because it still owns descriptors, dependencies,
rectangles and resource lifetimes before those choices become D3D bindings.

## Implementation

### Hangar worker recording race, 22:34 correction

The latest crash still reads null at RVA `0xe204d6`. The CPU probe identifies reference
enqueue `0x18d21c0`, called from material parameters `0x110dc30` at `0x110dd39`,
BasePass parameters `0xec5440`, and shared mesh drawing `0xec8b10`. The worker stack
continues through `0x1142863`, `0x1142d60`, `0x11584b8`, `0x1157e82`, `0x1167b62`,
`0x1158858`. The pixel shader is `8446d04c917833569c768094e8c051c1c0028a4e`, expected
slot1 layout `0x0ab00bb1`. The CPU creator dereferences the normal/scaled View reference
and queues null. This narrows earlier snapshot/lifetime hypotheses to a worker shared-view
read, though no instruction-level interleaving was captured.

Source inspection found a concrete null window: saving `view+0x10` or `view+0x18` with
native move `0xde5cf0` cleared the shared slot before replacement. Generated buffer
retention fixed a separate ownership omission but could not prevent a worker recording null
in that window. Save now copies and AddRefs the old RHI buffer (count at +8); one native
move publishes from a local source. Restore consumes the saved owned reference.

Shared rect, matrices, cached parameters and scene bindings also require recording ownership.
Helper `0x12183c0` waits for outstanding CPU recording graph events and clears their list.
It matches UE4.18.3 `WaitForOutstandingTasksOnly`, used by
`WaitForTasksClearSnapshotsAndDelete` in SceneRendering.cpp. It runs on the render owner
before SR/UI shared-view changes and restoration, outside plugin locks. It does not wait
for GPU completion. Generated buffer retention until queued RHI completion remains necessary.
The helper is expected-byte guarded and named/saved in Ghidra.

MSVC Release builds passed. Deployment backup:
`.local/deploy-backups/ac7-pair-20261001-223402-360`. Existing actual-overlay preflight
passed Insert toggling, pixels and resize. No new tests or autonomous game launch.
The correction needs the same hangar camera-pan retest. Background jitter and edge aliasing
remain reported symptoms, with no claimed visual fix. Source revision and dump fingerprints
remain those recorded below; local origin decompilations are in
`.local/ac7-ultra-repeat-20261001/`.

The runtime now loads and prepares the AC7 DLL after decryption and before graphics activation.
SDK ABI 10 supplies prepare/start/quiesce/stop/status, a copied render configuration and execution
callbacks. The game DLL owns native hooks and conventions; vendor evaluation and graphics-state
restoration remain in the runtime. Detection is still separate from rendering readiness.

At the renderer constructor, the plugin changes only matching renderer-owned main views. It
uses the backend's requested size, quantized to the engine's four-pixel granularity, then calls
native `ComputeFamilySize` before scene allocation. It leaves the source view and widget logical
sizes alone. The old half-second screen-percentage repair is disabled under native ownership.

At native pre-visibility setup, matching main views temporarily select the engine's temporal
preparation. Their original AA modes return immediately afterward, so DOF/SSR/graph selection retain
the game's choices. When the allocated scene extent and render rectangle disagree, jitter is
withheld. If the allocation fits the output rectangle, that transition frame renders at native
size. This avoids temporal sampling of padded regions during engine allocation hysteresis.

At `FRenderingCompositePassContext::Process`, before dependency gathering, the plugin walks the
native input-zero chain to the tonemapper. Every downstream node must be a reviewed role. It
inserts a first-party heap SR node and registers it with the native graph, which retires it through
`Release`. The SR node also depends on all original tonemap branches, including additional
dependencies. Bloom and grading therefore finish at the original size before SR changes the view.
This ordering was corrected during source review: wrapping input zero alone would resize the view
before the graph ran the later auxiliary branches.

The node queues the native bilinear upscale as a complete fallback into an engine pooled target.
Its queued SR marker then resolves leased native resources, evaluates the runtime and overwrites
that target on success. A vendor refusal leaves the native fallback pixels intact. The output is
linear `PF_FloatRGBA`; native tonemapping, grading, materials, HUD composition and final output
continue afterward. This concerns internal scene colour, not HDR display output. No preset is forced.

Descriptor hooks resize only the reviewed downstream chain. Its rectangles and shader parameters
switch in the same graph, and the following FXAA node aliases its input instead of filtering SR
again. The plugin uses the native uniform-parameter producer and native RHI uniform creation/move
ownership, then restores the original parameters, uniform handle, matrices and scene extent after
CPU graph construction. It does not re-run dynamic drawing-resource initialization and does not
rewrite uploaded GPU constant buffers. Widget layout/DrawSize, glow and depth processing remain
native; full-resolution downstream composition removes the current downscale point rather than
rasterizing a guessed set of UI draws.

The compatibility texture matcher, 120-frame UI retirement, 300-frame chain scan, 240-frame
restake, promotion and constant-buffer rewrite are bypassed under native ownership. D3D binding
shadows remain for opt-in captures. A native prepare refusal retains the compatibility renderer.

## Identity, units and lifetime

Every queued marker copies the session, native family frame, view/pass keys, rectangles and camera
parameters while their owner exists. It does not read the latest CPU frame at execution. Native
family frames are not simulation/input IDs and do not authorize FG or latency claims. The persistent
view-state key distinguishes SR histories; camera cuts, native-frame gaps, owner and size changes
reset them. Jitter is in input pixels. The AC7 producer declares decoded motion to UV as
`(0.5, -0.5)`, current minus previous, Y down. The runtime translates that convention to the legacy
alternate-backend NDC basis; it does not apply the conversion twice. This is source-derived and
still needs live temporal comparison, especially the custom-projection motion branch.

Pooled references are acquired on the render thread. RHI completion retires leases to that owner;
it does not call the engine's non-atomic pool release on the RHI thread. Native pool refs prevent
reuse until command consumption; D3D command ordering protects subsequent GPU use. Quiescence
rejects new producers, and stop refuses while commands/scopes/retired references remain. Inactive
CPU drain hooks remain until retirement completes. A partial hook activation overlapping a
forwarding call pins its inactive module rather than unloading live return code.

F9 receives native execution packets and keeps independent widget/graph observers. It does not
detour the controller's post-process owner again. `native.jsonl` gains `rhi_scope_begin` and
`rhi_scope_restore`, copied native frame/scope/role and leased resource addresses. No view pointer
is dereferenced after CPU retirement. Raw captures stay ignored.

## Evidence and checks

The executable fingerprint is
`c7da97f5f8a807d4f1264adbb074146fcffe9bdc2ffa98791b822cd28e558f4f`.
The retained dump is
`fafd1db2808d32333caecf4056e8bcd676c2f07e4c1fabc0f270ef29f22d24db`.
Unreal reference revision is `0a14a8d537a31ecc77488ced41dbaa0166612ef8` (4.18.3).
The [native manifest](evidence/ac7-native-renderer-refactor-20261001.json) records RVAs and guarded
entry bytes. All hooks preflight before installation; helpers are matched to native source/call
sequences. New helper functions are named and saved in the local Ghidra program.

The relevant source owners are `SceneRendering.cpp` (renderer and shader parameters),
`SceneVisibility.cpp` (temporal preparation), `SceneView.cpp` (`ComputeFamilySize` and scaled rect),
`RenderUtils.cpp` (four-pixel quantization), `RenderingCompositionGraph.cpp/.h` (dependency ordering,
heap-node retirement and pooled output), `PostProcessUpscale.cpp` (fallback), and
`PostProcessTonemap.cpp/PostProcessTonemap.usf` (auxiliary inputs and downstream use).

`eng/verify.ps1 -VS2026 -Configuration Release` passes: 33 tests passed, five vendor/explicit
hardware experiments skipped, 38 total; Rust formatting and Clippy passed. The new Windows WARP
harness drives the private vtable through an independent graph walker and queued executor. It
checks auxiliary-before-SR order, descriptor propagation, FXAA bypass, source/output rectangles,
shader-only rebuild and restoration, producer quantization, secondary-view exclusion, delayed
frame 41 after CPU frame 900, graph retirement before RHI execution, native pool leases and blue
fallback pixels after the stopped backend refuses. The scope test checks nested scopes, capacity,
quiescence and refusal to unload during callbacks. These are synthetic tests, not AC7 runs.

## Initial validation request and limits, 1 October

This build is not game-validated. Recheck briefing, hangar, flight, pause/resume, quality changes,
lighting/reflections, UI sharpness/size and live shimmer. F9 once per scene collects execution
identity and paired SR inputs/outputs. The screenshot remains a useful independent comparison.
The engine graph must actually accept these private layouts in AC7 before `rendering_ready` becomes 1.

Only the reviewed origin-zero main view/chain is supported by this native path. Letterboxed,
secondary and unreviewed graph paths refuse SR; renderer output-size changes and sub-rect support
need further migration. Exposure is still supplied through the pipeline's auto-exposure fallback;
this refactor does not claim that the exposure/banding report is solved. Per-object velocity
coverage, cloud depth/motion, full engine-owned UI isolation for FG and simulation-frame handoff
remain open. Earlier lighting/reflection and borrowed-translucency capture fixes remain in the
compatibility path and must not be credited as validation of this new graph.

The matching proxy, AC7 DLL and real overlay were explicitly deployed on 1 October after the gate.
The helper verified installed hashes and backed up the prior pair at
`.local/deploy-backups/ac7-pair-20261001-031408-000`. The real ABI 4 overlay Insert/draw/resize
preflight passed. This deployment did not launch AC7 or verify its new native rendering path.

## Continued implementation

The refactor continues beyond the 03:14 deployment. No new tests were written in this continuation.
A focused MSVC Release build of the proxy, orchestrator and AC7 DLL succeeds. These source changes
have not replaced the installed 03:14 build.

The runtime now owns the renderer configuration callback directly. The proxy forwards enable
intent; backend status supplies output and planned input dimensions under the runtime's existing
metadata lock. The duplicate proxy configuration snapshot and SRW lock were removed.

`rsf_dlss_pipeline_resize_output` prepares an output texture and, for FSR/XeSS, a replacement
backend bridge before committing the new dimensions. DLSS queries its input range and releases
its old feature at commit. Decode resources and temporal history reset on the new size. Native
execution checks the packet's output dimensions; the host also follows swap-chain resize events.
This closes the startup-size-only backend allocation. Origin-offset view support remains open.

The AC7 graph adapter retains the tonemapper's input-two eye-adaptation reference. All auxiliary
producers execute before the SR node, and its pool lease remains valid until queued consumption.
The runtime copies the current eye-adaptation red channel to a 1x1 R32_FLOAT resource without
readback. The stock source writes the smoothed multiplicative exposure scale there
(`PostProcessEyeAdaptation.usf:191`). This value is passed to the SR pipeline instead of leaving
DLSS in auto exposure when the engine guide exists. Absent or incompatible guides still use the
backend fallback. This is source-inspected and built; live exposure/banding behavior remains
unverified. The scalar texture and cached source views release with the pipeline device.

Remaining work includes origin-offset/cropped views, complete native UI producer/isolation
ownership, legacy carrier cleanup and lifecycle integration, and the final completion audit.
The refactor goal remains active. Builds are not proof of image correctness.

### Lifecycle ownership continuation

The runtime session now owns module cleanup through RAII, validates the returned API size/version,
and keeps an inactive session when failed preparation cannot quiesce/stop immediately. A BUSY
prepare with a non-null session is a retained owner, not a successful activation. The proxy tracks
that distinction instead of treating any session pointer as prepared.

Lifecycle calls execute outside the runtime session mutex. Transitions reject overlapping
operations, and status readers retain the module operation until their callback returns. Final
successful stop still requires caller serialization against new use of the retired handle. The
AC7 plugin similarly marks transitions before native calls, releases its lifecycle mutex while
they run and publishes their result afterward. Status reports actual native-controller activity,
including command-ownership deactivation, rather than only the earlier start flag.

The proxy now exposes a frontend-safe shutdown request. The graphics owner handles it at its next
Present boundary, quiesces producers and retries stop while native commands/references drain. The
pipeline/device and callback services remain alive until plugin stop succeeds. Only then are held
inputs, frame tap and vendor resources released. Cleanup does not run inside DllMain. Startup no
longer publishes enable intent before its quality setup begins.

Focused MSVC Release builds of proxy, orchestrator and game DLL pass. No new tests were written
or run during this lifecycle continuation. Cropped-view support, native UI producer/isolation
ownership and remaining carrier migration are still unfinished; the active goal remains active.

### Cropped-view implementation continuation

The native runtime now prepares region-local colour, raw packed motion and depth guides for a
nonzero-origin or padded view rectangle. Colour and motion use GPU subresource copies; depth uses
an integer-load compute conversion to R32_FLOAT, avoiding partial depth-stencil texture copies.
The pass copies only declared view pixels and leaves motion encoding unchanged. Resources rebuild
on device/format/extent changes and release with native pipeline resources. The enclosing native
execution state guard restores graphics bindings. No CPU readback is introduced.

The backend evaluates at the output rectangle's width/height. Its result is copied into that
rectangle of both native output resources, preserving the native fallback outside it. SR history
also resets on input/output rectangle changes. Whole engine surface configuration is stored
separately from the backend crop extent, so a letterboxed evaluation cannot change the dimensions
used by later renderer construction. Quality selection replans the whole surface before publishing
its renderer configuration.

Native primary-view selection now uses the renderer-owned unconstrained rectangle at `+0x90`
and excludes scene/reflection/planar captures at `+0xc42..0xc44`, matched to the nearby camera-cut,
cursor and locked-view fields in stock SceneView layout and the native constructor. Constrained
rectangles are scaled relative to the whole surface instead of requiring origin zero. Graph
output allocations retain the whole-surface extent while packet rectangles describe active
pixels. Cached shader camera dimensions are checked against rectangle widths/heights, not Max.

Focused MSVC Release builds pass. This is implementation/build evidence only; no new tests were
written or run and no new AC7 execution was performed. Live letterboxing, allocation padding,
depth/bloom normalization and inset native consumer behavior still require final validation.
The completion claim remains unproven. Native UI producer/isolation and remaining carrier cleanup
still require implementation.

### Native widget producer continuation

Question: can AC7 rasterize its UI at output density while keeping logical layout and positions
stable through preset and pause transitions? The widget queue is the ownership boundary: its
PrepareTargets call allocates source/glow targets, PrepareWindow lays out the logical window, and
FWidgetRenderer draws it. Final D3D composition cannot recover detail lost at this producer.

The retained dump and UE4.18 WidgetRenderer.cpp show DrawWindow constructing root geometry as
DrawSize / Scale with a Scale layout transform. Scaling DrawSize and Scale together preserves
logical geometry. Native PrepareTargets resizes the main target when DrawSize changes, but only
creates the three half-size glow intermediates and final glow target once. The implementation
therefore scopes a temporary physical DrawSize to allocation, restores logical DrawSize before
PrepareWindow, adjusts only that converter's DrawWindow arguments, and resizes owned glow siblings
through InitCustomFormat and UpdateResourceImmediate. A changed primary target sets the native
refresh-dirty byte +0xe0 so RefreshFPS cannot skip painting the replacement. Other refresh timing,
linear-gamma/PF_B8G8R8A8 creation policy and game glow parameters remain under engine control.

Ownership is queue-local TLS, nested scopes restore the previous owner, and no UObject pointer is
retained across queues or GC. Borrowed/shared converters (+0x78) are excluded. Density is an explicit
policy derived from the measured 1920x1080 HUD design canvas: integer ceiling of whole output/design
size, minimum one, capped at eight and 16384 pixels per target axis. It is not proof that every
widget uses that design canvas; live DPI/glow appearance still needs validation. Disabled sessions
use density one on the next owned converter queue. Immediate teardown cannot force dormant widgets
to queue again; their engine-owned allocations remain until normal recreation/GC.

Hooks: QueueRender 0x4d6340, PrepareTargets 0x4d6660, DrawScaledWindow 0x1380d80. Helpers:
InitCustomFormat 0x1ab0200 and UpdateResourceImmediate 0x1ab7dc0. All check sixteen expected bytes
before activation and announce the refusing RVA. The capture-only queue detour is skipped when the
native controller owns it; the PrepareWindow observer remains. The live Ghidra program was verified
as Ace7Game.exe.dump, image base 0x7ff741350000, project AC7. All five functions were named and the
program saved. QueueRender replaces the older QueueWidgetDraw analyst name at the same address.

The carrier no longer writes screen percentage during native preset changes, enable/disable or
manual scale intent. Overlay scale reports the planned renderer configuration. FSR/XeSS now receive
the scalar engine exposure guide through the adapter; an exposure-policy change prepares/plans a
replacement vendor context before committing, preserves the prior context on failure, and resets
history through the session. Auto exposure remains the fallback for frames without the guide.
This corrects the earlier adapter which opened auto exposure unconditionally and omitted exposure
from submitted frame resources. SDR display mode does not change the linear pre-tonemap input.

Focused Windows MSVC Release builds of game DLL, proxy and orchestrator pass. No new tests were
written or run, per the user's implementation-first instruction. Latest changes are not deployed
or game-tested. Borrowed target ownership, native UI isolation/FG simulation identity and final
integration validation remain open. This is continued implementation, not a completed refactor.

### Native UI composition ownership continuation

Question: where does the native HUD pass receive its separate UI raster, and can the runtime
preserve the exact pre-HUD scene without guessing D3D bindings? Live Ghidra was revalidated as
Ace7Game.exe.dump at image base 0x7ff741350000. HUD Process 0xfcbc90 has one graph colour input;
the initial expectation of a second graph UI input was wrong. All eight shader variants call
SetShaderParameters 0xfcf880, which binds the render target at view+0x13a0, its resource+0x70 and
RHI texture+0x30. A null view target selects an engine fallback rather than a usable UI layer.
The helper was named and saved, and its expected entry bytes guard this additional layout use.

The plugin now takes a D3D reference to the actual view-owned UI resource during native CPU pass
preparation and leases it through the queued begin/end scope. SDK ABI3 appends ui_input without
reordering fields. The runtime composition owner copies scene/UI at the begin marker and composed
output at the matching end marker, carrying session/family/view/native-frame/scope identity.
These GPU snapshots preserve contents independently of later engine resource reuse. They are
bounded to one current composition packet; readers run on the execution owner and must match
identity. They do not establish final Present or simulation/input identity.

Copies are demand-driven, disabled by default. F9 requests two composition scopes and writes
native_pre_hud, native_ui_raster and native_composed via the existing dump path. A future FG
consumer can explicitly request continuous capture, but these surfaces cannot authorize FG on
native frame identity alone. UI alpha/effect conventions are still unverified and the raw raster
is not advertised as a complete premultiplied compositable UI layer. In particular HUD distortion,
masking or other shader effects may transform it at composition. Native UI unjitter and borrowed
converter producer ownership still need review.

Focused Windows MSVC Release proxy/runtime/game builds pass. No new tests were written or run.
No new deployment or AC7 execution occurred. Simulation handoff and final native image/lifecycle
validation remain incomplete; the original fundamental-refactor goal stays active.

### Renderer source identity continuation

Live Ghidra and SceneView.h:414 confirm that the existing 0xff9af0 helper removes projection jitter,
zeros its tracked value and recomputes derived matrices. resize_consumers calls it before the
pure uniform producer, so the reviewed post-SR graph already supplies unjittered current view
parameters to HUD composition. An additional GPU-buffer jitter override would duplicate that
engine work. This is source/binary evidence, not a new image-stability measurement.

The native controller now hooks outer Tick 0x3a3f70, PollGameDeviceState 0xcea130 and renderer
retirement 0x113a0c0, using the previously researched CPU/handoff mappings and sixteen expected
entry bytes. Tick establishes a thread-local ownership scope. The first device poll inside that
scope assigns a per-integration 64-bit source-frame ID; polls outside Tick and renderers created
before polling receive no association. Native idle/disabled polling can still mean no actual
input sample, so this metadata does not claim a vendor simulation marker or authorize FG.

The renderer constructor binds that source ID to the renderer key and a unique submission ticket.
A fixed 512-entry table refuses overflow and never evicts live bindings. Several families may
share a source frame while retaining different submission IDs. Native packets resolve the
embedded family owner (family minus0x10) and copy identity before queued execution. SDK ABI4 appends
source_frame_id and submission_id without changing earlier offsets; runtime composition snapshots
carry them too. No render-thread consumer reads the most recent game-thread ID.

Retirement removes the CPU association at helper entry, after Render has returned, before native
wait/deletion can free and reuse the address. An initial post-call removal was corrected during
review because another allocation could reuse the freed address before removal. Packets already
own their metadata; a late CPU lookup refuses identity instead of receiving a reused frame. Stop
waits for live renderer associations and queued leases. The outer Tick return path has a separate
in-flight count: after short producers/resources drain, stop disables hooks, then returns BUSY
until that Tick returns. Counting Tick with short producers would prevent Present-time shutdown
from ever disabling it. Failed partial preparation pins either kind of in-flight return path.

Source ownership remains distinct from final HUD/Present association and vendor latency ordering.
Those still require native integration and runtime evidence. The borrowed widget target's owner
is identified as UNimbusGameInstance.HUDWidgetRenderTexture +0x1f0 / StereoUIRenderTexture +0x1f8
in the SDK, but its allocation producer has not yet been migrated. Focused Windows MSVC Release
builds pass, no tests written/run, not deployed or game-validated. The full refactor stays active.

### Shared game-instance UI producer continuation

Question: who allocates the borrowed HUD canvas, and does its logical size match the measured
1920x1080 policy? InitCustomFormat xrefs led to UNimbusGameInstance::Init at RVA0x3cd130. Live
Ghidra shows it assigning new UTextureRenderTarget2D objects to +0x1f0 and +0x1f8 and initializing
both with literal width0x780/height0x438, format2, linear-gamma1. SDK fields identify them as
HUDWidgetRenderTexture and StereoUIRenderTexture. Thus 1920x1080 is verified allocation/design
state for these shared canvases, rather than only a size seen in captures. Init and GetPrivateStaticClass
0x923e60 were named and the live AC7 program saved; expected16byte entries are recorded.

The allocator hook scopes InitCustomFormat to the game-instance initializer and those exact two
object fields. Other target initialization forwards unchanged. When output configuration becomes
available later or changes, the borrowed converter queue resolves GetWorld -> OwningGameInstance
+0x140, reproduces the native class-hierarchy check against NimbusGameInstance, selects the mono/
stereo field using converter+0x79, and resizes that target before raster work. Arbitrary borrowed
textures and unrelated worlds are excluded. No game-instance pointer is retained across calls.

Shared converters restore the verified1920x1080 logical DrawSize after PrepareTargets; stock
first binding would otherwise copy the new physical canvas size into layout. DrawWindow scales
physical DrawSize and Scale together, and glow-enabled converters still resize their private
source/intermediates before combining into the shared target. Integer density is selected from
whole output size; disabled queues restore density1. Format, gamma and engine resource creation/
update remain native. A resized canvas sets the native refresh-dirty flag for each contributor
queued during that same outer Tick, since clearing it invalidates existing pixels. This is scoped
to the actual replacement input frame, not a fixed-age or settling timer. Normal RefreshFPS resumes
on later frames. Dormant contributors not queued in that frame still require transition validation.

This replaces the earlier owned-only restriction for the specifically verified shared canvases.
It does not authorize resizing other borrowed render targets. Target recreation and stereo/DPI/
glow behavior remain game-validation requirements. Focused Windows MSVC Release proxy/runtime/game
builds pass. No tests written/run, no deployment or game execution. Final presentation/latency
integration and the original completion audit remain unfinished.

### Ownership audit correction

Source review found a shutdown gap when the final RHI commands release pooled-target leases
inside native renderer retirement after the last postprocess callback. Retirement now drains
retired refs on the render owner both before and after the native wait/delete helper. Quiescence
therefore retains the CPU retirement observer until its resources have drained; final release
remains on the engine pool owner, not the Present/RHI caller. This is code/source evidence only.

Native packets now explicitly mark the declared primary view using the same unconstrained
rectangle and capture exclusions as producer sizing. Runtime composition refuses secondary
views, zero identities and mismatched session/family/view/native-frame/source-frame/submission/
scope/rectangle pairs. Bounded capture requests count attempted primary scopes at begin, including
refusals, instead of retrying indefinitely until two successful copies. Readable snapshots still
require a successful paired end. No Present or vendor FG validity is inferred from these checks.

Focused Windows MSVC Release builds pass. No tests written/run, no deployment or game execution.
The next latency mapping must distinguish message pumping/device polling/FinishedInputThisFrame
from engine simulation at the indirect virtual Tick slot0x268 in FEngineLoop. Wrapping the whole
outer Tick as simulation would include input/media/render preparation and misstate the boundary.
The current copied source IDs remain metadata until those semantic markers and final presentation
association are connected. The original refactor remains unfinished.

### Engine update-to-render boundary continuation

Question: where does simulation/update end for a submitted renderer? UE4.18 GameEngine.cpp:
1100-1360 runs world/viewport updates then RedrawViewports and PostRenderAllViewports inside Tick.
The GameEngine Tick return is later than submission; outer Tick also includes input/media work.

GameEngine.cpp string xrefs and live decompilation identified UGameEngine Tick0x1781e80. Its
negative-delta check, module/loading work, world iteration, viewport tick and final virtual
slot0x418 call match the source. Direct caller0x3c7a30 is UNimbusGameEngine Tick: it updates a
Nimbus subsystem then calls the base. SDK confirms the derived class. Base Tick vtable slot
RVA0x2d510e0 points to0x1781e80; derived Tick slot0x285ca88 points to0x3c7a30. Derived Redraw
slot0x285cc38 points to0x177cdd0, shared with the base implementation, which updates then draws
the game viewport. GEngine is globalRVA0x3cbbc28. All three functions were named/saved in live AC7.

Expected-byte hooks now scope derived/base updates, de-duplicate nested base calls, validate the
actual GEngine object, and mark the update-to-render boundary at RedrawViewports entry. Renderer
construction copies AFTER_SIMULATION into its identity binding only after that boundary in the
input source frame. Queued SR/HUD packets and composition snapshots carry the flag. Secondary
renderers created during updates do not inherit a late global flag on the consuming thread.
This is association evidence only, not a submitted Reflex/PCL marker or final Present proof.

GameEngine Tick/redraw now use long-call lifetime guards because synchronous rendering can reach
Present while they remain on the stack. Retirement keeps that lifetime guard across native waits
and uses short producer guards around pre/post CPU state and pool draining. Quiescent stop can
disable hooks after short producers/resources drain, then wait for long return paths. Otherwise
Present-time shutdown could wait forever for an enclosing call it cannot disable.

Reflex skill/checklists, local SDK headers/provider source and the official Streamline Reflex
ProgrammingGuideReflex.md version2.14.1 were reviewed (raw NVIDIA-RTX/Streamline GitHub, accessed
1October2026). No new Streamline sleep or marker calls were added. vendor/streamline contains
headers; no sl.reflex.dll/sl.pcl.dll was found there, which does not inventory other references or
the game installation. Native Off/On/Boost commands are not wired yet. No runtime marker order,
frame-token, AppCalledSleep, ReflexState, toggle, VSync/limiter/resize/fullscreen/threading,
fallback or performance result is claimed. Reflex/PCL completion check remains FAIL/incomplete.

Focused Windows MSVC Release builds pass for proxy/runtime/game. No new tests, deployment or game
execution. Final presentation association and actual vendor marker/sleep routing remain open.

### CPU event handoff continuation

SDK ABI5 appends a CPU-event callback to copied host services. Events carry integration session,
source-frame token, semantic stage, QPC ticks and frequency; callbacks borrow the packet only for
that call and cannot perform immediate-context graphics work. The plugin now reserves the source
token at outer Tick entry, before native message pumping/device sampling, while binding renderer
identity only after the actual device poll begins. This corrects the earlier late token creation
at Poll and creates a real pre-input notification for a future Reflex sleep consumer.

The existing mapped hooks emit FRAME_BEGIN before original outer Tick, INPUT_SAMPLE immediately
before Poll, SIMULATION_BEGIN before the derived/base engine update, SIMULATION_END before native
RedrawViewports (or at update return for a non-rendering update), and FRAME_END after outer Tick.
FRAME_END closes CPU ownership only, not GPU or Present. Nested updates de-duplicate events. If
quiescence starts mid-frame, an already-open frame still emits its closing events while the
long-call lifetime guard retains the controller/services. No new native sites were needed.

The runtime owns a bounded128-record CPU sequence cache, copies all metadata, checks exact
session/source identity, stage order, clock frequency and monotonic QPC timestamps, and refuses
eviction of live frames. Ended records may be reused; missing/overwritten IDs refuse lookup.
Idle/loading frames can explicitly end before input/simulation and remain partial. Rejected
sequences close at FRAME_END so a failure cannot permanently consume capacity. The optional vendor
consumer runs at the real CPU callback outside the sequence lock; its lifetime/configuration must
be changed only while producers are quiescent. No GPU resources or vendor tokens are owned by this
CPU cache. Runtime cleanup happens after plugin CPU producers stop.

The proxy forwards CPU events to the runtime without doing per-frame GPU work. F9 native identity
JSON now records the paired GPU packet source/submission/native/scope IDs and flags alongside the
matching CPU stage mask, ended/failed state, QPC frequency and timestamps. A missing record is
explicit, not guessed from the latest CPU frame. Capture output is implementation instrumentation;
no new captures were taken here.

Focused Windows MSVC Release proxy/runtime/game builds pass. No tests written/run, no deployment
or AC7 execution. Reflex/PCL completion remains FAIL/incomplete: no consumer currently calls
Streamline sleep/markers, no Off/On/Boost native run commands are wired, and no marker/frame-token,
AppCalledSleep, statistics, runtime toggle/resize/VSync/limiter/threading/fallback/performance
validation is claimed. Actual rendering/submission/Present event association is still separate.

### Viewport submission provenance continuation

Question: does game-viewport draw completion identify final presentation? UE4.18 UnrealClient.cpp:
1110-1235 queues BeginRenderFrame, calls ViewportClient::Draw/flushes its canvas, then queues the
end-drawing task with native present/vsync/latency-trigger flags. Slate may control presentation
instead; SlateRHIRenderer::DrawWindow_RenderThread later calls EndDrawingViewport for its window.
Treating the game's end-drawing task as final Present would conflate two ownership boundaries.

Live Ghidra traced RedrawViewports to FViewport::Draw RVA0x1ac3600. The same function reads client
+0x50, size+0xb8/+0xbc, enqueues begin via virtual0x130, draws/flushed canvas, then queues captured
viewport+flags end work through allocator0x1ac21d0 or executes it synchronously. Its target viewport
comes from GEngine.GameViewport+0x720 and the viewport-client's native+0xa0 field in RedrawViewports.
SDK independently confirms GameViewport+0x720. Source/binary mapping identifies Draw, not a final
Present. It was expected-byte guarded, named and saved in the live AC7 Ghidra program.

The new Draw hook scopes only that actual main game viewport while an input source frame is
owned. Renderer construction copies the opaque viewport key alongside source/submission identity.
SDK ABI6 appends viewport_key; queued SR/HUD packets and runtime composition snapshots retain it,
paired-end validation includes it, and F9 identity JSON exports it. Consumers never dereference
this key. Outer Tick resets/restores viewport scope so reentrancy cannot inherit an earlier frame's
viewport. The original bShouldPresent is passed unchanged; no Present is forced.

This establishes scene submission provenance, not a Slate-window or DXGI swapchain identity.
Slate source-path string xrefs were absent for all eight retained paths; that failed lead is
recorded, rather than treating absent strings/xrefs as unsupported presentation. ConditionalResizeViewport
log strings remain another lead. The next trace must bind the queued Slate draw buffer/window
ownership and its actual RHI viewport before runtime presentation can consume this scene packet.

Focused Windows MSVC Release builds pass. No tests written/run, deployment or game execution.
Vendor Reflex/PCL sink, final presentation association and original completion audit remain open.

### Pass completion callback correction and Slate window trace

Source review found a semantic mismatch: the private scope observer intentionally reports the
restored parent at end, but the public render_pass callback promises the actual pass before/after
execution. Passing the observer notification directly to the runtime made its new composition
begin/end identity comparison fail. No runtime capture validation had established that path.

SDK ABI7 separates optional scope_state diagnostics from semantic render_pass events. A new
private create_passes entry retains legacy current-scope observer behavior, while independently
notifying the same ticket at execution begin/end. Resources resolve at both markers: output-pool
publication happens after the CPU Process body, so begin can precede publication but end is queued
after it. The matching end packet now contains the real completed output, before lease retirement.
Exceptions in a semantic consumer cannot suppress the separate scope-state restoration. The proxy
uses scope_state solely for capture correlation and removes its unused global last-pass copy.
Existing observer behavior is preserved; tests were not changed or run per user instruction.

The Slate source-path/resize strings had no xrefs. Switching to r.HDR.UI.CompositeMode lookup
located DrawWindow_RenderThread RVA0x1343660; its viewport/cmd-list parameters, batch/target work and
final EndDrawingViewport call match SlateRHIRenderer.cpp:601-915. This is a discovery anchor, not
evidence that AC7 or its display uses HDR. Private window setup is0x1345310; it iterates draw-buffer
shared window lists, finds viewport info in renderer+0x208, and queues window tasks. Task executor
0x1346520 uses captured renderer+0x10, viewport-info+0x18, element-list+0x20, window+0x28 and flags
+0x30/+0x31, then calls window rendering before recycling task storage. The enqueue allocator
0x133c6c0 is a next mapping lead; task owner data is initialized after it returns.

Window rendering calls FRHICommandListImmediate EndDrawingViewport0x120c340 with viewport-info
RHI viewport+0x70, present=true and native vsync. That queues an RHI command with next/execute at
0/+8, viewport+0x10, present+0x18 and vsync+0x19; execute address0x120dc20 is not yet a defined
function in the live database. The native call can force command dispatch/wait afterward. These
four mapped functions were named/saved. The next association must carry source/window data through
the actual task and RHI command, then match the native swapchain; it cannot use a latest frame ID.
FRHIViewport exposes native swapchain/backbuffer access in RHIResources.h:740+, but its shipped
vtable mapping and resource lease still need implementation/validation.

Focused Windows MSVC Release proxy/runtime/game builds pass. No tests, deployment or game run.
Completion remains unproven; window/Present handoff and vendor marker/sleep consumer are unfinished.

### Queued Slate window ownership implementation

SDK ABI8 appends opaque window/RHI-viewport keys and a plugin-leased native swapchain. The game
plugin hooks the four mapped Slate private/allocator/task/window functions with sixteen expected
entry bytes. Private drawing binds the input source frame to the main game window using the
source-matched GameEngine weak window at +0xdf8, reference controller+0xe00/strong count+8, and
main viewport via +0x720/+0xa0. This is a source/binary layout, still awaiting live validation.

The specific task allocator returns the allocated task before captured window fields are filled.
A bounded512-entry copied binding therefore records its source/window owner before enqueue. No
live binding is evicted. Duplicate live task addresses poison identity instead of associating a
reused task with an old frame. Task execution consumes the binding before native task storage is
recycled, validates captured window+0x28 and renderer/info/element-list+0x10/+0x18/+0x20, then
carries it in a render-thread lexical scope. Non-main windows refuse association. Synchronous
private-to-window calls use that same scoped producer identity. No latest global frame is read.

Window rendering resolves viewport-info+0x70 as FRHIViewport. RHIResources.h:740+ and public
D3D11Viewport.h:91 expose GetNativeSwapChain as the first virtual after the destructor; the DX11
override returns IDXGISwapChain. QueryInterface verifies the interface and retains its reference
before queued scopes. The shipped virtual method/layout and window fields need runtime confirmation;
no vtable preflight/game execution is claimed from source review alone. The plugin queues WINDOW
begin before Slate commands and end after native window processing. The scope crosses native
EndDrawingViewport dispatch, so its copied source/window identity is active at the driver's Present.
The lease releases only after matching execution end. Quiescence retains pending CPU tasks and
RHI tickets; long-call guards keep the module/controller alive during dispatch and Present.

The runtime keeps a bounded execution-thread window stack with copied metadata only. It compares
Present callback and leased engine swapchain through canonical IUnknown identity, rather than raw
interface pointer equality. Mismatched end invalidates borrowed state before lease retirement;
overflow blocks matching until untracked nesting ends. The proxy feeds semantic window callbacks
and emits at most six match diagnostics at its Present callback. Original present/vsync/clear
arguments forward unchanged; no Present is forced and no backbuffer reference blocks resizing.

This implements window/source ownership matching but does not prove current scene-resource
agreement or successful real Present. The existing observer callback omits Present flags/result,
so test/occluded/generated presentation handling still needs the actual presentation boundary.
Reflex/PCL vendor consumer is also unattached. Current readiness stays false. Focused Windows MSVC
Release builds pass, no tests written/run, no deployment or AC7 execution. Final integration,
shipped-layout verification and the original completion audit remain unfinished.

### Real Present completion contract continuation

The old graphics observer exposed only a before-Present swapchain callback. That could not
distinguish test calls, an occluded/error return, or successful invocation from accepted Present.
Observer ABI7 appends a size-checked paired event containing the same swapchain, sync interval,
flags and returned HRESULT. Begin precedes legacy frame/overlay work; completion follows the
original chain's return. Interval/flags/result forward unchanged and callback exceptions cannot
interrupt the game's Present. Existing recursion handling still forwards inner re-entry without
issuing duplicate events. Present1 is not newly intercepted by this change.

DXGI_PRESENT_TEST takes a direct original-chain path with paired events but bypasses frame
counters, device setup, capture/readback, settings changes, overlay and backend activation. The
runtime ignores test events. The observer's frames_presented now increments only after a non-test
call returns S_OK. It counts accepted calls, not physical scanout or generated frames. This
corrects the earlier pre-return attempt counter without inferring latency from its growth.

The native runtime copies a numeric before/after Present record on the execution thread, including
source/window/viewport/scope identity from canonical COM matching, actual interval/flags/HRESULT,
completion/accepted status and QPC begin/end/frequency. Acceptance means S_OK, so occlusion and
other status/error results are not counted as accepted. Pair mismatch invalidates the record.
No swapchain pointer or other borrowed resource persists in this completed record. CPU FRAME_END
still does not replace actual Present completion. The proxy wires the paired observer event to
this runtime owner separately from the existing before-Present overlay/settings callback.

Focused Windows MSVC Release proxy/runtime/game builds pass. No tests written/run, deployment or
AC7 execution. Scene-resource agreement, Present1/generated-frame coverage and the vendor
sleep/marker consumer remain incomplete. These events do not by themselves authorize FG. Original
full-refactor completion and live results remain unproven.

### Present1 and presentation hook lifetime continuation

Observer ABI8 adds the presentation method (Present or Present1) to paired events. Installation
queries the dummy chain for IDXGISwapChain1 and, when available, resolves slot22 to its genuine
on-disk function before applying the same overlay-aware detour route as Present. An unavailable
interface or failed detour logs lack of coverage and leaves Present intact. There is no new
shared-vtable fallback for Present1. Runtime coverage still needs verification on the game chain;
a dummy interface is not proof that every external proxy's implementation is intercepted.

Both methods share one templated observation body and one templated detour installer. Present1
forwards the original DXGI_PRESENT_PARAMETERS pointer, interval and flags exactly. Shared recursion
depth prevents internal Present1-to-Present forwarding from issuing duplicate frame events or
running overlays/backend work twice. Re-entry uses the matching genuine route. Test/return-result
handling and accepted-call counting are identical; numeric runtime records validate method as
part of their begin/end pair. Generated frames are not identified merely by method or success.

Source review also found that installation enabled a detour before publishing its forwarding and
genuine pointers. The common installer now publishes the complete route through an atomic mode
flag before enabling its entry and clears it on activation failure. A live Present call count
prevents uninstall from clearing routes/resources underneath callback/forwarding execution.
Uninstall still requires quiescent graphics producers; this is a lifetime guard, not proof of
safe arbitrary concurrent uninstallation. Genuine executable clones retain the prior process-life
policy for return-path safety.

Focused Windows MSVC Release builds pass. No tests written/run, no deployment or game execution.
Scene-resource agreement, vendor sleep/marker activation and live Present1/overlay/generated-frame
validation remain open. The original full refactor remains active and unproven.

### Capture-independent scene/window identity agreement

The runtime now tracks primary post-update HUD composition identity independently of optional
GPU snapshots. A bounded64-record execution-thread metadata cache copies session/source/viewport,
submission/family/view/native frame/scope, flags and output rectangle. End must match begin and
resolve valid input/output resources. No resources or engine objects are retained. A second
primary composition in the same source frame marks the declaration ambiguous rather than choosing
whichever was observed last. An unresolved live record cannot be evicted by a later modulo collision.

Present window matching requires a unique completed composition with exact integration session,
source-frame token and main viewport, plus the post-update flags. The numeric Present record
stores that submission/view/native-frame declaration before the original call and rechecks it
when the call returns. Changed or ambiguous declarations lose scene_matched. F9 does not need to
be armed and no GPU copies/readback are added to ordinary rendering. Reset occurs after plugin
work retires, on the execution owner.

This is declared engine ownership agreement only. The exact scene texture sampled by Slate and
final HUD-less resource/alpha conventions remain unproven; scene_matched does not authorize FG.
That distinction prevents same-frame metadata from being mistaken for a verified pixel dependency.
Focused Windows MSVC Release builds pass; no tests, deployment or game execution. Shader/resource
consumer validation, vendor sleep/marker setup and the original completion audit remain open.

### Final family surface producer continuation

Source review corrected another boundary assumption: completed HUD composition is an intermediate
postprocess output and cannot alone identify the texture Slate consumes. FSceneViewport exposes
a different game-thread buffered Slate handle and render-thread render target. SceneViewport.cpp:
1388-1402 selects those resources by calling thread; EnqueueBeginRenderFrame publishes the
render-thread target before family rendering. Slate policy subsequently resolves a batch resource
to TextureRHI and binds it through pixel shader parameters. Matching frame IDs alone does not
establish that dependency.

SDK ABI9 appends scene_surface and FINAL_SCENE role10. The native postprocess hook now queues a
primary final-scene scope around the entire native PostProcessing Process, independently of SR
being enabled. It reads the source-matched family render target at+0x20, invokes FRenderTarget's
GetRenderTargetTexture slot+8, resolves the RHI texture through native-resource slot+0x30 and takes
a D3D reference on the render owner before queuing. The leased underlying surface survives CPU
renderer retirement until RHI scope end, then releases with the pass lease. No game-object pointer
is retained and no GPU copy/readback occurs. Shipped virtual layout and source/capture agreement
still need live validation.

The runtime scene identity owner now records this full native postprocess completion rather than
the earlier HUD intermediate. Its paired end requires the same retained surface identity; stored
surface_key is numeric comparison metadata only, never dereferenced after lease retirement. It
is not a resource handle or proof of later sampling. Output remains the original native target,
without reinsertion or composition changes from this marker.

The Slate window call into rendering policy was traced to DrawElements RVA0x133ff40, named/saved
in live Ghidra. Native pixel-parameter helper0x1351ce0 was inspected but is not yet proven to be
the ordinary textured batch consumer; the examined call involves a general shader/material
parameter path. Treating its raw argument as a scene texture would be premature. Raw decompiles
remain in .local; next trace must establish the ordinary TextureRHI producer/consumer arguments
and queue identity before comparison against final family surface.

Focused Windows MSVC Release builds pass. No new tests, deployment or game run. Exact sampled
texture/late canvas contributions, vendor sleep/marker wiring and original completion validation
remain open. The final-scene marker concerns native postprocessing completion, not physical
scanout or automatic FG authorization.

### Ordinary Slate texture consumer implementation

The ordinary non-material branch in DrawElements resolves TextureRHI from texture-object resource
or TSlateTexture and calls RVA0x1351e10. Live decompilation shows shader+0xc0 texture bind index,
+0xc2 nonzero bound-resource count, third argument TextureRHI and fourth argument sampler-ref.
It queues separate pixel texture/sampler commands and consumes the sampler ref afterward. This
matches SlateRHIRenderingPolicy.cpp:769-878. The earlier0x1351ce0 candidate belongs to the material
path and is not used for this ordinary-consumer hook. BindTextureAndSampler was named/saved.

Expected-byte guarded site32 observes this specific engine binder only while the validated main
window rendering scope is active. SDK ABI10 appends sampled_texture/texture_slot and role11.
The native controller retains a CPU numeric final-surface declaration keyed to source/viewport;
it filters the ordinary binder against that exact declaration before queuing any validation work.
Font/icon/menu textures therefore do not allocate leases or queued markers. Duplicate final
producer declarations are ambiguous; stale/overwritten source keys refuse. These CPU keys are
comparison data only and never dereferenced.

For a candidate, the plugin resolves the actual RHI texture to its native D3D resource, takes a
reference, and queues binding begin/end around the original texture/sampler commands. The lease
survives to execution end. Original shader parameters/sampler ownership are passed unchanged and
no GPU copy/readback is added. On the graphics stream, the runtime checks the leased texture
against the completed final family surface and exact session/source/viewport, records the bound
window, and invalidates ambiguous multi-window declarations. Numeric Present records now separate
scene declaration agreement from scene_texture_bound evidence.

The event proves the mapped ordinary pixel-shader texture binding, not independent inspection of
every subsequent indexed draw or an arbitrary material/custom/stereo path. The source branch draws
after its parameter/gamma/invert-alpha setup; material/custom consumers remain explicit gaps.
Later canvas contributions, HUD-less guide construction and shipped runtime sampling still require
validation. No FG readiness or vendor marker acceptance is inferred from a matching address.
Focused Windows MSVC Release builds pass, no tests written/run, no deployment or game execution.
# Launch crash correction, 1 October 2026

## Captured UI branch resolution and native allocation correction

### Correction after the 20:16-20:18 retest

### UI acceptance and Ultra Performance failure

### Repeated crash: producer availability and queued uniform lifetime

The 21:35 hangar capture and crash trace establish an explicit null binding, rather than a
buffer becoming absent only at commit. For shader SHA1 8446d04c917833569c768094e8c051c1c0028a4e,
required mask0x82, the setter records slot1=0 followed by non-null material/policy bindings in
slots7,6,4,5,3,2 before the indexed draw. Required slot1's layout hash is0x0ab00bb1. The null
command at2a176f00188 contains the ordinary pixel uniform executor0xe16220 and a zero payload
at+0x20. The user reports failure at a fixed hangar camera-pan position, consistent with a
particular material becoming visible. That material/producer remains to identify.

Ghidra finds only four creators of this exact command executor:0xe9b880 takes a raw buffer,
0xedb9d0 extracts a buffer from object+0x38,0x18d21c0 reads a reference at+0, and0xf4c620
creates an immediate uniform from parameter data. Bounded CPU-origin probes now observe all
four and record a stack when a bound slot1 is queued with a null buffer. They forward unchanged.
This closes the gap left by the narrow View helper probe; no crash fix is claimed until the
origin is known. The latest capture stays untracked at motion-20261001-213540-44968-1.

The 21:17 run still crashes at the same pixel resource-table read. Neither earlier scaled-buffer
availability nor retained generated uniforms is sufficient to resolve the blocker. No null
argument was recorded by the narrower pixel View helper observer. Therefore the missing PS
slot has not yet been proven to be the View buffer; earlier View-specific attribution was a
hypothesis, not a verified producer diagnosis. The user prohibits launching AC7 autonomously.

Instrumentation now observes actual RHI pixel uniform writes at RVA 0xe39520 and the pixel
resource-table commit at 0xe1f8d0. The queued pixel command at 0x120cf50 dispatches context
vtable slot0x1e0; the dump's context vtable maps that slot to0xe39520. Its state-cache write and
bound-uniform array match stock RHISetShaderUniformBuffer for SF_Pixel. Commit reads required
shader bits at+0x30, dirty bits atRHI+0x40e6 and bound uniform entries atRHI+0x3f90. The observer
records a bounded ring of128 setter arguments and, on a missing required buffer, prints its
expected layout hash plus the preceding16 binds. This distinguishes explicit null writes from
omitted bindings after shader-state reset and avoids assuming the buffer's type. Both hooks
forward unchanged; this diagnostic build does not claim a crash fix.

Entries:0xe39520 `48 89 5C 24 10 55 56 57 41 56 41 57 48 83 EC 20`;
0xe1f8d0 `48 89 5C 24 20 55 41 56 41 57 48 83 EC 30 0F B7`.
MSVC Release built; installed overlay preflight passes. A user-driven reproduction is required
to collect the binding sequence missing from the existing sparse minidumps.

The next run confirms successful 533x300 ->1600x900 SR evaluations, then the identical null
pixel uniform crash at 0xe204d6. The UI-selector-only correction was insufficient. The new
dump/log are preserved in .local/ac7-ultra-repeat-20261001. This crash remains a blocker.

Two producer defects were identified. First, the stock scaled-view producer is conditional on
ordinary translucency, while AC7's enlarged material policies can select that slot in other
passes. The render-pass wrapper now ensures a valid scaled uniform for every participating
view with cached parameters and a main uniform before material selection. It uses the matched
native producer at RVA 0x116ea00, expected entry `4C 8B DC 55 56 57 41 56 48 81 EC 78 0D 00 00 48`.
Earlier notes incorrectly called this 0x1169a00 due to address subtraction; no earlier runtime
code invoked the mistaken address. Ghidra's correct function name is saved.

Second, RHICommandList.h's FRHICommandSetShaderUniformBuffer stores FUniformBufferRHIParamRef,
a raw pointer, without retaining it. The inspected AC7 pixel enqueue helper at 0xe9b880 likewise
writes the raw uniform into queued command+0x20. Our temporary UI and post-SR uniforms were
released when native view state was restored on the CPU. They lacked an owner through queued
RHI execution. Generated uniforms now receive explicit RHI references in a bounded renderer
ownership table before publication. After native renderer retirement has waited for recording
tasks and dispatched prior work, an appended RHI completion scope transfers those references
to render-owner retirement. Restore therefore cannot release the last owner before queued binds.
Failure retains buffers/module with a log; stop refuses while unqueued owners remain. No stale
uniform is evicted. CPU renderer retirement alone is not treated as RHI completion.

A read-only pixel View-uniform enqueue hook at 0xe97d80 records up to eight null arguments,
caller RVA, UI scope, renderer and thread, forwarding unchanged. Its expected entry is
`48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 57`. It provides a source lead if a null remains.
These are verified source omissions and built corrections; which caused the observed null
slot and whether the game is now stable still require runtime verification. Cloud work is deferred.

The user confirms all UI is fixed by the UnmodifiedTranslucency producer build deployed at
20:37. Ultra Performance nevertheless shows an aliased jittering scene and then crashes during
preset/backend switching. The log explicitly rejects 536x300 input against DLSS's exact
533x300 accepted extent at 1600x900 output. This is an active-view/allocation-padding error,
not evidence that DLSS evaluated a poor reconstruction. prepare_owned_views rounded active
rectangles to four pixels, conflating the buffer's allocation granularity with the view.

Active rectangles now preserve the exact backend size. Allocation remains engine-owned and
may be padded; the pre-visibility check accepts the exact active rectangle inside its expected
padded allocation. The existing region-local input preparation crops padded color/depth/motion
before evaluation. No backend range is relaxed and no preset is forced. This affects every
preset, including non-multiple-of-four XeSS/FSR dimensions.

The saved crash log/dump under .local/ac7-ultra-20261001 shows a null R8 uniform-buffer argument
at game RVA 0xe204d6. Disassembly maps it to the surface-resource table reader at 0xe204b0,
called by pixel resource-table commit at 0xe1f8d0. Stack/context analysis identifies buffer
index 1; the shader resource mask is 0x82. The caller follows the stock D3D11 SetResourcesFromTables
logic which requires a bound buffer. Some RHI heap/state memory is absent from the minidump;
the exact shader-bind producer remains unproven.

Source audit identifies an incomplete UI view contract: patched material selectors can read
View+0x18's scaled-view uniform even when no ordinary translucency path created it. The UI
scope previously supplied only View+0x10. It now supplies the same fully initialized unjittered
uniform through both slots, retaining the extra RHI reference and moving/restoring each old
reference through the matched engine helper. This closes a verified producer omission but
does not by itself prove the reported crash's upstream cause. MSVC Release built; exact-size
Ultra Performance behavior, preset transitions and crash correction need a game retest.

Implementation continuation: hooks now wrap BeginUnmodifiedTranslucency (0x1097d20), its
resolve (0x109d320) and RenderTranslucencyPass (0x1168d40). The render wrapper carries the
renderer on the producer thread and keeps the module alive through the enclosing native call.
Begin uses the native enlarged-depth allocator (0x109e0d0, pool scene+0x1b8) and depth producer
(0xee93f0) before changing the main view. It then saves rect/matrices/cached parameters/uniform,
temporarily selects output-size BufferSize, regenerates an unjittered native uniform and binds
the populated depth pool as the UI pass's depth. Native begin/allocation/draw therefore agree on
output extent. Resolve queues the original resource resolve before restoring all borrowed state;
failed begin and unmatched enclosing-return paths also restore it. Temporary pool references
and native moved uniforms bound the lifetimes. Glow/extra contributors repeat this scoped setup.
All new entry/helper bytes are checked after decryption. Windows MSVC Release builds; runtime
layout, depth sampling, UI detail and jitter behavior remain subject to the next game run.

F9 native route capture now records eight consecutive frames per request rather than two, so
the next run can compare cloud input, reconstruction output and downstream outputs across a
jitter cycle. This is opt-in, bounded instrumentation, not a claimed cloud correction. The older
compatibility route keeps two frames. Capture readback may hitch.

The user confirms that briefing terrain/icons are sharp, but splash/menu/hangar/flight/pause UI
remains low resolution, with menu jitter and vertically moving clouds. The new allocation hook
logs 1600x904, yet captures still record UnmodifiedTranslucency and NimbusHUDCombine at 928x524.
This disproves the claim that the ordinary separate-translucency producer owns all AC7 UI layers.
It helped the briefing path; it is insufficient for the separate game-specific UI path.

Ghidra string references to UnmodifiedTranslucency identify its actual allocator at RVA
0x109f780. It receives a packed extent from BeginUnmodifiedTranslucency (0x1097d20) and
ResolveUnmodifiedTranslucency (0x109d320); both explicitly read the scene BufferSize at +0x208.
The allocator caches a pool at scene+0x1c0 and checks its descriptor before reuse. Begin binds
the scene depth pool at +0x60 and queues a viewport from the incoming view's +0x70 rect. Merely
enlarging this allocation would leave depth, viewport and shader view mismatched.
RenderTranslucencyPass (0x1168d40) supplies the main view to these paths for pass 4, including
glow/extra layers, and after the ordinary pass 2. All four functions were named and saved in the
confirmed dump project. They are newly identified root sites, not installed hooks. The next UI
implementation must own this pass's target, matching depth, unjittered view and resolve together.
Stock separate-translucency extent changes cannot substitute for that contract.

The supplied 334x510, 30fps cloud video spans 4.57 seconds. Integer translation matching of the
first twelve decoded frames finds repeated vertical shifts of 0, 1 and 3 recorded pixels while
the visible pause-panel edge is stationary. This establishes instability, not its source. Three
pause-capture samples have near-identity camera reprojection, changing subpixel jitter and no
integer cloud translation in the sampled SR input or backend output. Sparse samples cannot
exclude an intervening oscillation. Cloud producer jitter, downstream composition and temporal
handling still need direct consecutive-frame evidence. No cloud fix is claimed.

Eleven captures from the 20:06-20:07 run show splash/title/menu, briefing, hangar, flight and
pause. In the pause capture `motion-20261001-200740-43352-11`, native SR input is 928x524 and
backend/graph outputs are 1600x900. NimbusHUDCombine and its input/output snapshots are 928x524;
Nimbus composite and final output are 1600x900. The 1920x1080 widget resource alone therefore
does not establish full-output UI: its separate rendered branch has already lost resolution.
The former `native_pre_hud` label records the input of this HUD branch, not necessarily the main
SR scene. New captures label it `native_ui_branch_input`. No capture-side scaling created these
dimensions. Earlier reliance on widget-target sizing was insufficient.

Walking from the measured branch to its allocator identifies native
FSceneRenderTargets::SetSeparateTranslucencyBufferSize, RVA `0x10be2b0`, expected entry
`48 89 5C 24 08 57 48 83 EC 20 65 48 8B 04 25 58`. Ghidra's matching function reads BufferSize
at +0x208, writes layer dimensions at +0x218/+0x21c and writes scale at +0x220. Stock UE4.18.3
PostProcess/SceneRenderTargets.cpp has the matching producer. The function was renamed and saved
in the confirmed Ace7Game.exe.dump project. The existing compatibility immediate at +0x79 of
this function remains 1.0 when native sizing bypasses screen-percentage settings; native mode
therefore unintentionally inherits a scene-sized layer.

A guarded native producer hook now runs the original then derives scale from actual padded
scene dimensions and runtime output dimensions before allocation/view/depth setup. The maximum
axis ratio preserves output density on both axes; the other axis may be padded slightly. The
hook refuses ratios outside 1..4 and requires all eleven existing enlarged-layer depth/view
patch windows at preparation. It logs the first six chosen extents. Inactive native mode keeps
the original behavior. No new cvar or timer controls this allocation. This correction is built
with Windows MSVC Release; post-correction UI detail, sizing and transitions need game validation.
The captured Streamline process-exit crash and missing presentCommon warning remain separate
open defects; this allocation change does not claim to address them.

The subsequent launch reached native SR evaluation: the log records six successful results for
928x524 input and 1600x900 output. The user nevertheless reports pixelation/blurring and an overlay
message claiming that no pass bound reconstruction inputs. That message reads the compatibility
pass counter, which the native callback did not increment. Native callbacks now increment the
counter and publish copied resource-presence bits for the overlay; they do not retain textures for
UI status. Success of SR evaluation is not proof that its output survives the remaining graph.
F9 now additionally captures native SR input, backend output, graph destination and completed
primary pass outputs. These bounded snapshots distinguish reconstruction quality from later
sampling/composition loss. MSVC Release builds; image correction is unresolved pending captures.
The same run reports missing Streamline presentCommon bookkeeping, a separate unresolved issue.

The first run of the ABI10 deployment activated native SR and initialized the overlay, then
crashed on the render thread. The question was which native resource contract failed before
gameplay. The runtime log reports a read from address `2` at AC7 plugin RVA `0x2298`.
Disassembly of the exact installed DLL maps this to `node_process` dereferencing the result of
`native_texture(plan.velocity)` before COM AddRef. The adjacent call at RVA `0x228a` produces that
value. The crash dump and log are retained locally under
`.local/ac7-native-launch-crash-20261001`.

Matching UE4.18.3 source at revision `0a14a8d537a31ecc77488ced41dbaa0166612ef8`,
`PostProcessing.cpp:1262`, declares VelocityRT as `TRefCountPtr<IPooledRenderTarget>&`.
The retained AC7 decompilation at RVA `0xffb900` passes its fourth argument to the native input
pass constructor at RVA `0xff1d40`, agreeing with this pooled-target contract. Our hook correctly
dereferenced that reference, but subsequently misclassified the pool as an RHI texture. Slot
`0x30` on that object returns its reference count; the observed value `2` was treated as a COM
pointer. The earlier texture interpretation was wrong.

The SR producer now resolves the pool's shader-readable RHI texture through the existing
`pool_texture(plan.velocity, false)` path before retrieving and retaining its native D3D resource.
That COM reference remains owned by the queued pass lease and is released at lease retirement.
No velocity scale, encoding, coverage gate or SR insertion boundary changed. The Windows MSVC
Release build passed and the matching DLL set was redeployed with hash-checked installation.
The real overlay preflight again passed Insert, visible pixels and resize. A successful AC7
relaunch remains pending; this correction is not evidence that later native paths are valid.
