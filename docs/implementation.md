# Implementation tracker

## Unity Mono research and shared plugin direction, 3 October 2026

- [x] Fingerprint Drag'n Wash Steam build `25286774`, Unity `6000.3.14f1`, Mono and packaged
      URP Forward+/RenderGraph path. Verify player/executable PDB GUID/age pairing. Completely
      decompile 12 selected game/renderer assemblies into ignored references. Parse selected
      pipeline/camera/canvas settings with strict reads and retain parser failures.
      [Research](research/drag-n-wash-renderer.md) and [game evidence](../games/drag-n-wash/engine.json).
- [x] Record the shared Unity Mono plugin direction using reflection and Harmony, with pipeline
      adapters and the existing native orchestrator. Map camera policy, temporal production,
      pre-tonemap reconstruction and SDR overlay boundaries from the shipped managed code.
      [Design and validation sequence](research/unity-mono-plugin.md).
- [ ] Implement the native/managed bootstrap in the player's existing Mono domain, generic
      capability discovery and guarded Harmony patches. No Unity plugin is built or tested yet.
- [ ] Validate render-thread native copy-through, API state/fences, view/frame identity and graph
      resource lifetime, then temporal inputs and output-resolution SR reinsertion.
- [ ] Game-test custom fluid/deformation coverage, scene/settings/resize transitions, HUD-less/UI
      boundaries, teardown, FG and latency. No game launch, injection or installation change
      occurred during the static investigation. Built/synthetic/game/device readiness stays false.

2 October documentation: repository skills now separate general renderer analysis, shared
Unreal 4 concepts, UE4.18/AC7 evidence and TrueSky production. The parent transcript, accepted
implementation and failed experiments were reviewed; metadata/local-reference/whitespace
checks pass. This is documentation validation, not a new game/device test.
[Scope and checks](research/skills-session-update-20261002.md).

ReScaleFrame is a monorepo. All first-party components share this history and release version. Separate runtime/plugin DLLs do not imply separate repositories.

## Active AC7 scope correction, 1 October 2026

2 October release acceptance: the user confirms the latest 22:17 deployment works and requests
replacement of the broken v0.1.0 GitHub package. The previously accepted wing/cloud plugin is
unchanged; the optimized GPU-fence bridge and FSR4 INT8 compatibility are now accepted in game.
This is user-reported visual acceptance, separate from the explicit RTX4070 device fixtures.
The package now includes the proxy, AC7 plugin, overlay, DLSS, FSR 2/3/4 and XeSS SR runtimes,
all accompanying notices, exact source and checksums. Frame generation and Reflex remain deferred.
[Research and validation](research/sr-interop-performance-20261002.md).

Release follow-up: startup hotkey hint added through overlay ABI5. It draws above the completed
frame for eight seconds, fades over the final second and is dismissed on opening the panel.
No mouse input, settings changes or reconstruction participation. Native Release gate passes
33 executed tests/five skips; Rust workspace tests pass53. Actual Rust overlay host verifies hint
pixels with panel closed, Insert toggling, settings panel rendering and resize. The game plugin
hash remains unchanged; proxy/overlay hashes change for the hint. No agent AC7 launch.

2 October FSR4 compatibility: implemented a fingerprint-guarded, device-leased INT8 capability
hook in the installed SDK4.1.1.2740. This targets the native decision behind OptiScaler's scoped
adapter/driver substitutions, leaving actual GPU identity and FP8 checks intact. RTX4070 output
watermark confirms FSR4-I8 at3x;80 frozen-input Ultra frames and84 lifecycle fixture frames pass.
The D3D12 bridge uses GPU ordering and three command slots. User acceptance is recorded above;
no new measured in-game FPS comparison was supplied.
[Mechanism and proof](research/sr-interop-performance-20261002.md#fsr4-int8-compatibility-implementation).

2 October21:30 user acceptance: wing artifacts and cloud blockiness are clean. The latest log
confirms native Texture2D/RG32F validation, divisor1 activation and one-to-one depth dispatch.
Follow-up XeSS/FSR performance work replaces current-frame CPU transfer waits with GPU fence
ordering and three guarded command slots.240 standalone Ultra evaluations produce byte-identical
outputs before/after; lifecycle and full-pipeline hardware checks pass. The initial official FSR4
refusal on RTX4070 was subsequently resolved by the fingerprint-guarded INT8 hook recorded above.
[Evidence and limits](research/sr-interop-performance-20261002.md).

2 October wing/cloud follow-up: reviewed the earlier transcript and20:13 captures before changing
the implementation. The20:05 kernel never activated because its guard required an array UAV;
TrueSky's texture owner creates a Texture2D UAV. Corrected the one-to-one kernel and guard to
that native layout. A new expected-byte patch at TrueSky RVA0xbfc09 promotes only normalized
scene-depth bounds from R16G16_UNORM to R32G32_FLOAT through the native allocation owner.
Captured aircraft depth has up to8.57% relative error in UNORM GPU output; RG32F reduces it below
0.000034% in the standalone replay. Full scene-grid clouds at Ultra exceed the user's observed
stock Balanced cloud-resolution floor. Installed21:24, backup `ac7-pair-20261002-212445-314`.
Release gate33 passed/five skips, Rust format/lint and real overlay Insert/render/resize passed.
No AC7 launch; live wing/cloud acceptance remains pending. [Evidence](research/ac7-plane-artifacts-20261002.md).

2 October flight regression: user confirms clean aircraft and corrected hangar exposure after19:10,
but sky/lighting/volumetric clouds are broken. New captures locate the damage before DLSS.
TrueSky divisor1 skipped its x2/x3/x4 depth-pass selection while still dispatching and cleaning up.
Added a guarded native pass/call correction and first-party one-to-one normalized depth kernel;
full resolution waits for actual shader/binding readiness. Original shader and native cleanup are
preserved. Explicit NVIDIA GPU comparison passes for x1 and shipped x2. Flight visual acceptance
is pending; accepted aircraft/colour/FP16 fixes retained. Installed20:05, backup
`ac7-pair-20261002-200538-588`; Release gate33 passed with five skips, Rust format/lint and real
overlay Insert/render/resize preflight passed. No AC7 launch. [Evidence](research/ac7-plane-artifacts-20261002.md).

2 October rendering correction: all three current defects are priority-one blockers. Implemented
native TrueSky scene-grid production with a guarded DLL instruction patch and native FP16 scene
colour allocation. Added a depth-guarded GPU colour correction after DLSS and before native bloom/
tonemap, without changing presets. Frozen Performance GPU replay removes broad bands; Ultra
flight replay preserves silhouettes after guarding. Native cloud/precision changes are built,
not yet game-accepted. Installed19:10 with backup `ac7-pair-20261002-191007-200`. MSVC Release
gate passes33 executed tests, five vendor/environment skips; Rust format/clippy and actual overlay
Insert/pixel/resize preflight pass. Installed hashes match the build. No autonomous game launch or new test target. Full evidence and failed
variants are in [the artifact research](research/ac7-plane-artifacts-20261002.md).

2 October retest: aircraft, distant cloud blocks and DLSS Performance/Ultra exposure defects
persist. The00:09 ordinary-translucency correction is not the artifact fix. Complete mid-frame
D3D11 state preservation is built and device-tested in a standalone binding/counter experiment.
F9 now pairs the TrueSky composite's colour/loss outputs and records its cloud/depth inputs;
CB readbacks select shader-declared slots. Release gate passes33 tests with five environment
skips, format/lint pass. No new test target or game launch. Artifact and exposure fixes remain
game-unverified. [Evidence](research/ac7-plane-artifacts-20261002.md).

Installed10:22, backup `ac7-pair-20261002-102210-392`; real-overlay Insert/render/resize preflight
passes. Game was not launched. Await paired F9 cloud/aircraft comparisons with SR off and Ultra.

2 October 00:09: updated goal is gameplay aircraft artifacts, then motion-vector improvements.
Latest flight sample has affected surfaces in depth/written motion and a pre-reconstruction colour
defect. Removed obsolete global separate-translucency enlargement; native ordinary sizing restored,
scoped UI preparation retained. F9 skips inactive stages and budgets readbacks per sample. Release
and overlay preflight pass; plane/UI retest pending. [Evidence](research/ac7-plane-artifacts-20261002.md).
User confirms background/cloud stability and blur improved. DLSS banding remains separate.

23:19 build installed after user confirms stable background/clouds. Bloom common-source
rewiring now generates native bloom/exposure from reconstructed scene; source identity
must match or late path remains. Native view-state jitter cycles scale with output/render
area (32/72 at Performance/Ultra), replacing ineffective console tuning under native ownership.
F9 saves exposure guide. Release/overlay preflight pass. Bloom/edge/exposure retest pending;
aircraft colour banding and cloud-overlap smear still open. See stability research note.

22:52 stability corrections installed. User confirms hangar crash no longer reproduced.
FSR/XeSS packed scene-colour rejection corrected with explicit linear RGBA16 conversion.
TrueSky native projection adapter now preserves vertical jitter handedness for matched primary
views. DLSS receives explicit-sentinel dense motion to preserve valid zero aircraft vectors.
F9 includes submitted motion. MSVC and shader compilation plus overlay preflight pass;
visual/backend retest pending. [Evidence and limits](research/ac7-stability-20261001.md).

22:34 correction deployed: CPU trace maps null PS View slot to worker BasePass material
recording. Saving shared View uniforms by move cleared their slots during replacement.
Save now takes an owned reference, publishes from a local replacement, and joins native
outstanding recording tasks before shared view changes/restoration. MSVC Release build and
overlay Insert/render/resize preflight passed. Hangar crash and shimmer retest pending;
no autonomous game launch. See the native renderer research note for evidence and limits.

Hangar retest: fixed camera-pan position produces explicit PS slot1=null before indexed draw,
not merely a missing binding at commit. Shader SHA1 and expected layout recorded in research.
Mapped four exact CPU command creators; bounded null-slot CPU stack probes installed. Release
build/overlay preflight pass. Crash still unresolved; user-driven reproduction required. No game launch.

21:17 crash persists at the same PS uniform read. No null View-helper argument recorded;
missing buffer type remains unproven. Direct RHI pixel setter/commit observers now record
expected layout hash and preceding bindings. Diagnostic build only; crash remains a blocker.
User explicitly prohibits autonomous AC7 launches. Preserve SR/UI corrections; cloud work deferred.

Crash blocker only: successful533x300 SR confirmed, same null PS uniform crash recurred.
Corrected scaled-uniform availability before all participating translucent views and explicit
ownership of generated view uniforms through queued RHI completion. Raw queued uniform
pointers were previously released at CPU view restoration. Bounded null-binder diagnostics
added. Release builds and overlay deployment preflight pass; gameplay crash fix unvalidated.
Cloud investigation deferred until the crash is resolved.

User confirms all UI fixed by the native UnmodifiedTranslucency producer build. Ultra Performance
was refused: active view536x300 exceeded exact DLSS533x300. Corrected active rect sizing versus
allocation padding and completed UI normal/scaled uniform selector ownership. Release built;
Ultra Performance and crash retest pending. Dump confirms null PS uniform slot1; its exact bind
producer is unproven, so the source omission correction is not yet a game-confirmed crash fix.

UnmodifiedTranslucency correction now implemented: native begin/resolve/render wrapper owns
output-size allocation/viewport, engine-resampled matching depth and unjittered native uniform,
with saved scene state restored after resolve. MSVC Release built; UI game retest pending.
Eight-frame opt-in native route snapshots added for cloud jitter attribution. Cloud fix remains open.

Retest correction: briefing is user-confirmed sharp, other UI remains low resolution/jittery.
The ordinary separate-translucency allocation hook does not own AC7 UnmodifiedTranslucency.
Mapped its actual allocation/begin/resolve and render-pass callers in Ghidra; full-resolution
depth/view/resolve ownership remains to implement. Cloud video confirms vertical instability;
cause unresolved. Do not report all UI fixed or broaden back into FG/latency.

Eleven new captures isolate a scene-sized UI/translucency branch despite full-output SR.
Native SetSeparateTranslucencyBufferSize ownership implemented at guarded RVA 0x10be2b0;
actual scene/output dimensions drive allocation before rasterization. MSVC Release built;
corrected UI capture validation pending. Renamed misleading pre-HUD capture label to UI branch input.

After the launch-crash correction, AC7 logged successful native SR evaluations but the user
reports blurry/pixelated output. Corrected stale compatibility-derived overlay status and added
bounded F9 SR/backend/graph/downstream images. MSVC Release built; visual defect remains open.

First ABI10 game run crashed in native SR resource retention. VelocityRT was incorrectly treated
as an RHI texture instead of a pooled target. Corrected the pool-to-texture lookup, built Release
and redeployed matching DLLs with overlay preflight passing. AC7 relaunch remains pending.
The crash mechanism and evidence are recorded in the native renderer refactor notes.

The immediate goal is correct super resolution and improved motion-vector coverage. The native
refactor expanded into latency and frame-generation infrastructure before its SR path had been
validated in AC7. That expansion is deferred: Reflex/PCL wiring, FG/MFG handoff and broader
presentation tracing are not prerequisites for this goal. Preserve existing work without extending
it unless a measured SR defect requires it.

The earlier compatibility path has user-accepted upscaling and a user-confirmed lighting/reflection
correction. The newer engine-owned SR/UI implementation builds with MSVC Release but has not been
deployed or game-validated. Neither result proves that current UI blur, temporal instability or
hangar banding is resolved. Motion research establishes native encoding, camera-transform leads
and velocity eligibility sites; missing-object coverage is still unvalidated.

Next, audit only the native SR enablement and input/output path against the existing captures and
matching UE4 source. Establish one bounded candidate for game validation: intact scene colour and
postprocessing, matching depth/motion/view identity, correct jitter and history, and output-size UI.
Do not add another generic architecture layer. Prepare a matching DLL set and exact capture
instructions once that path is reviewable. Compare native and reduced-resolution briefing,
hangar, flight and pause/resume before declaring an improvement.

Then resolve measured motion conventions and coverage, starting with carrier-launch/refuelling
weapons and attached missiles, followed by moving vehicles. Match colour draws to velocity writes
or a verified rejection reason before changing the producer. Audit TrueSky separately using its
depth/history and stationary-camera evidence. No universal cloud or object-motion fix is claimed.

This scope correction is documentation-only; it adds no runtime experiment or build result.

Deployment on 1 October: explicitly installed the current Release proxy, ABI10 AC7 plugin and
matching Rust overlay in the user-named AC7 installation. Installed SHA256 values match build
outputs. The deployment preflight loaded the real overlay and passed Insert open/close, visible
pixel rendering and swap-chain resize. Matching FSR/XeSS runtimes and notices were also installed
by the deployment procedure. Previous files are backed up under
`.local/deploy-backups/ac7-pair-20261001-195716-425` and
`.local/deploy-backups/ac7-sr-20261001-195716-201`. Native SR and UI behavior in AC7 remain unvalidated;
this overlay preflight is not a gameplay test.

## AC7 release

- [x] Prepare the 0.1.0 Windows x64 package with the accepted proxy/overlay, retail DLSS runtime,
      dependency notices, source-commit manifest and archive checksums. Installation, usage,
      supported scope and removal are documented in the release notes and packaged README.
      Packaging does not change rendering code. [Packaging procedure](dependencies.md#packaging-the-ac7-release).
- [x] Published [v0.1.0 for AC7](https://github.com/KillerPixelCrew/ReScaleFrame/releases/tag/v0.1.0)
      from `4386a9079d68df3f9f26e55f69d1a01f42bcb306`. Windows package, corresponding-source ZIP
      and SHA256SUMS uploaded; GitHub asset digests match the local files and the downloaded
      checksum file matches. Packaged retail runtime initialized and reported DLSS support in the
      hardware load/query test. Framework and AC7 READMEs are separated.

## Repository foundation

- [x] Native CMake targets for bootstrap, orchestrator, AC7 plugin, and launcher.
- [x] Minimal C ABI for plugin metadata and executable recognition.
- [x] Rust workspace with an egui status surface.
- [x] Architecture, UE4.18 source leads, and presentation research retained in the repository.
- [x] Local Release build, C/C++ SDK compatibility, plugin contract checks, Rust formatting, and Clippy verification.
- [x] GitHub Actions workflow for native and Rust verification; individual run results are tracked in Actions.
      Every run from 7 to 8 September failed at the first MSVC warning, the secure-CRT deprecation
      of `fopen`, because nothing in the tree had ever been built with MSVC. Fixed 26 September
      along with the three other things MSVC refused: a test variable named `small`, which a
      Windows header defines as a macro; a fixture that added `dllexport` to functions its header
      declared plainly; and a missing `stdlib.h`. The suite then passes on Windows, 22 of 22, with
      Visual Studio 2026 through the `-VS2026` presets. CI has not run since.
- [x] The frame tap survives the Windows D3D11 runtime. First MSVC run of `tests/frame_tap.cpp`
      failed 23 checks that had always passed under Wine, and the cause is a property of the stock
      runtime rather than of the tree: it rewrites the work-submission entries of its heap vtable on
      every flush, which discards patched hooks, so no Windows run of this project could ever have
      observed a draw after the first read-back. Measured with a probe on 26 September, fixed by a
      sentinel check on every hook entry and pass-through hooks on the flush-class calls, tap ABI 8.
      [Evidence](research/d3d11-runtime-vtable-rewrite.md).

## Native AC7 renderer refactor

- [x] SDK ABI 2 and runtime-owned plugin prepare/start/quiesce/stop/status; native preparation
      precedes graphics activation. Recognition still does not report rendering readiness.
- [x] Renderer-owned view sizing before allocation, native temporal preparation, pre-tonemap
      graph SR node with native spatial fallback and auxiliary dependencies, downstream
      descriptor/rectangle/uniform ownership and FXAA bypass. Native mode bypasses the binding
      matcher, GPU constant-buffer rewriting and UI/chain/reinsert ageing timers.
- [x] Copied queued RHI identity, render-thread pool-reference retirement and F9 native execution
      records. Windows MSVC Release gate: 33 passed, five explicit vendor/device experiments skipped.
      Independent Windows WARP graph/queue/pixel tests passed; no new AC7 run yet.
- [ ] Validate this native path in AC7: briefing, hangar, flight, pause/resume, preset transitions,
      lighting/reflections, UI sharpness and temporal stability. `rendering_ready` remains zero.
- [x] Continued source implementation: runtime-owned native configuration, transactional output
      resizing and a leased engine eye-adaptation guide converted to scalar exposure. Focused
      MSVC Release build succeeds; new image behavior and backend resize paths are not game-validated.
- [x] Lifecycle source integration: failed-prepare module retention, calls outside lifecycle locks,
      transition/status reader ownership, actual controller activity and deferred graphics-owner
      shutdown before backend release. MSVC Release build passes; live teardown remains unverified.
- [x] Cropped-view source path: region-local colour/depth/motion guides, rect-local backend
      evaluation, native rect reinsertion, rectangle history resets and separate whole-surface
      configuration. Primary constrained views are scaled from renderer-owned surface dimensions.
      MSVC Release build passes; actual cropped image behavior remains unverified.
- [ ] Complete native UI producer/isolation ownership, carrier cleanup and
      lifecycle integration. The active refactor goal remains unfinished.
- [ ] Validate exposure input ownership, output resizing, native UI isolation for
      FG and simulation-frame handoff. Velocity coverage/cloud reprojection remains separate work.
      [Implementation and evidence](research/ac7-native-renderer-refactor-20261001.md).

## First working target

- [x] AC7 upscaling accepted by the user on 29 September 2026, including stable full-output
      briefing terrain/icons, automatic startup/reinsertion and the simplified Insert overlay.
      The final capture verifies zero-jitter view twins in the terrain's hull/domain stages as
      well as VS/PS. Temporary automatic capture disabled after acceptance.
      [Final result and scope](research/ac7-consumer-session.md#accepted-result).


- [x] Decrypted module capture and import map. Cross-built, Wine-tested, and run in AC7 on 6 September; that run was not MSVC-verified. [Evidence](research/ghidra-tooling.md).
- [x] AC7 frame/resource analysis and live view-buffer mapping. Captures identify sparse velocity and a separate half-size mask, plus a temporal-filter candidate. [Evidence](research/ac7-frame-capture.md).
- [x] Temporal jitter enabled through the engine AA gate and measured in perspective views. Game-tested; the recorded patch build was not MSVC-verified.
- [x] Console-variable access and `r.ScreenPercentage` changes verified in game; the recorded build was not MSVC-verified.
- [x] Rust input model: quality levels, UE motion encoding, and all-blocker viability reporting. Unit-tested; no GPU calls. [Backend guide](../runtime/backends/README.md).
- [x] Rust jitter/motion unit conversions and scale-dependent jitter sequence length. Unit-tested against recorded values; live C/C++ sign/scale agreement remains open below.
- [x] Jitter sample count follows render scale; a timer restores scale after mission loading. Game-tested through a full mission.
- [x] Streamline DLSS adapter: loading, device handover, support/render-size queries, tags, constants, and evaluation. Runs in AC7.
- [x] View layout mapped through matrix identities, including camera basis, projection, and `ClipToPrevClip`. Offline check: 11 captured perspective views, no failures.
- [x] D3D11 motion decode with bias/scale parameters, unwritten sentinel, and compute-state handling. Cross-built and tested under Wine/DXVK with source-encoded values and an axis flip. Game-buffer dumps matched the reference range and unwritten fraction.
- [x] AC7 view reader with matrix/size checks, main-view classification, pixel jitter, and unjittered projection. Recorded dataset: 50 recognized buffers, ten perspective views, secondary views marked. [Review](review.md) identifies remaining validation defects.
- [x] `TemporalAAJitter` located at `0x720` by comparing pre/post-patch captures, then checked against projection entries and live pixel offsets.
- [x] Orchestrator frame assembly converts plugin camera/resource data into a DLSS frame and rejects unusable combinations. Unit-tested without GPU work.
- [x] Live DLSS evaluation. Initial run: 2,176 recognized passes, 2,175 evaluations, no refusals. Mission run on 7 September: 7,917 evaluations, no refusals, 1024Ã—576 input and 2048Ã—1152 output. Recorded images show recovered detail and a complete scene; flight showed no obvious smearing. F7 is a debug display; reinsertion, grading, HUD, and controlled motion validation remain pending. [Evidence](research/ac7-frame-capture.md).
- [x] Conditional composed-colour selection for the DLSS bridge. Persistent input watch and AC7
      recombine rule are cross-built and synthetic-tested under Wine. Each frame falls back to the
      identified colour unless a matching composition is observed; no new per-frame allocations.
      Briefing relief presence was reported game-tested on 7 September. Reconstruction quality,
      hangar/flight regression, resource overwrite timing, and F6 remain open. [Decision and checks](research/ac7-composed-scene-color.md).
- [x] Conditional translucent depth for the research DLSS bridge. IA/VS shadowing and immediate
      depth-only replay use startup allocations and restore disturbed state. Same-frame composed
      layer/source-depth identity gates backend selection; absent layers keep original depth.
      Synthetic pixel readback covers accumulation, opaque occlusion, frame reset and state
      restoration under Wine. Game-tested in the briefing on 7 September: 733,861 of 744,726
      candidate draws replayed. Flight regression remains unverified.
      [Decision and checks](research/ac7-translucent-depth.md).
- [x] Separate translucency at its own resolution. 4.18 halves the layer and doubles it back on
      composite, so at a 50% render scale the briefing relief reached any reconstruction as a
      quarter-resolution image. Patching the halving out fixed the relief and the cannon tracers at
      once. The scale is now a four-byte immediate inside the patched instruction, aligned so it can
      be rewritten while the game runs, and derived from the render scale in effect so it follows a
      quality level instead of being fixed. Going above the scene's resolution needs the engine to
      size the layer's depth to match, which four one-byte patches enable by narrowing `Scale < 1.f`
      to `Scale == 1.f`; they are no-ops for every scale the engine produces on its own.
      Game-tested on 7 September: the briefing relief draws at 2048Ã—1152 inside a 1024Ã—576 scene
      with a matching depth, and reaches the reconstruction. Flight, the post-mission replay and
      other heavy screens are unverified. [Evidence](research/ac7-frame-capture.md).
- [x] Consumer session controls: automatic DLSS startup and deferred reinsertion; Insert opens an
      overlay containing only Enable DLSS and presets. Choices persist per user. Repeated preset
      changes use the last applied screen percentage, reset history and rebuild reinsertion;
      disable restores native rendering. Built and synthetic-tested on Windows; game validation
      pending. [Evidence and checks](research/ac7-consumer-session.md).
- [x] Default separate-translucency path renders at nominal full output resolution across
      scene presets and uses unjittered VS/PS view twins before direct recombine. The previous 1:1
      DLSS layer resolve is opt-in. Built and synthetic-tested; engine padding, pass classification,
      the user reported the half-output briefing looks good; the requested full-output sharpness
      change and flight regression still require a new game run.
      [Decision and limits](research/ac7-consumer-session.md).
- [x] Correct pixel-to-world unjitter math after the full-output terrain jitter report. The old
      helper and fixture both applied the linear-depth screen-matrix correction to device-depth
      SVPosition. Corrected fixture fails before the fix; scaled-view/depth regressions and the
      Windows Release gate pass. Full-output visual retest pending.
      [Evidence](research/ac7-consumer-session.md#full-output-jitter-pixel-to-world-unjitter-correction).
- [x] Captured and corrected enlarged-layer view/depth selection: 1600x904 terrain draws had
      800x452 views despite unjittered twins. Seven further engine gates now select the sized view
      and sampled depth above scale one; a follow-up game capture verifies 1600x904 bound views.
      This alone did not resolve the user's shimmer report.
- [x] Extend unjitter to tessellation and geometry stages (tap ABI 13). The captured terrain uses
      12-control-point patches and a VS that hands positions to DS. Synthetic tessellation/GS
      pixel readback passes; the final all-stage capture and user visual acceptance confirm the briefing fix. Prevent
      stale view twins from replacing smaller material allocations after address reuse.
      [Evidence](research/ac7-consumer-session.md#captured-enlarged-view-mismatch-and-tessellation-gap).
- [ ] Game-validate automatic startup, all five presets, disable/re-enable, saved choices, and the
      briefing layer's fixed visible dimensions and temporal stability.
- [ ] Reinsert the result. A debug view exists behind F7 and is game-tested: a full screen draw over
      the back buffer from inside the Present hook, with a rough tonemap so linear scene colour is
      viewable. It is what showed the reconstruction moving, which is the only way ghosting and a
      motion vector's sign can be judged. It is not the real path, which reinserts the reconstructed
      scene before the game's own composite so the grade and the interface survive. That is the
      remaining structural piece and the reason the picture is ungraded and has no HUD.

      The whole path is built behind F6 and has been run against the game. It took three runs to get
      there and each failed differently, which is worth keeping because the failures were all the
      same mistake about bindings.

      First run: nothing happened at all. The tail walk had taken a 2048Ã—32 strip as the composite,
      a UI bar the final draw also reads, and promoted that. Zero gates opened, so F6 did precisely
      nothing. Fixed by rejecting any input less than half the height of the target it is drawn into.

      Second run: the panel said no tail. The rule required the final draw to have exactly one input,
      and it has seven, because D3D11 leaves shader resource slots bound until something replaces
      them. `frame_tap.h` warns about this in as many words and the tail code ignored it, so the tail
      had never been identified in any run. This is the same failure as the composite selection, the
      interface format and the layer identity: a running game binds more than it reads.

      Third run: the tail is found and reinsertion runs, and it blew up the interface. Later runs
      the same day fixed the blow-up (the plan went stale on screen changes and is now restaked)
      and left the picture cleaner but the interface soft, with the briefing's 3D environment
      missing under F6.

      Superseded, 7 September, and then reversed. The interface was never a target to promote by
      format: AC7 rasterizes it at a fixed 1920x1080 and, on the briefing and hangar, draws it as
      world-space widget quads into its own render-resolution `R8G8B8A8` layer, depth-tested
      against the scene, before compositing it itself. Extraction into a layer of our own was built
      and run under M2 below, and it showed the opposite of what was assumed here: the quads read
      the scene and its glow chain and the game keeps processing them, so taking them out
      discolours the frame. Promoting the layer they draw into, with the viewport scaled, is the
      route again, now as `scene_promote` with the layers named by the classifier rather than by a
      rule about formats. The account of the three runs stays because the failures were all the
      same mistake about bindings.

      Finding the tail: the frame tap shadows the output merger and describes the draws into a
      render target it is asked to watch, with extent, viewport, ordinal within the pass, and every
      pixel shader input with its slot number. The loader points it at the swap chain's back buffer,
      takes the composite among what that draw reads (eight-bit colour, at least half the target's
      height), then watches that. An "interface target" used to be picked out of the composite's
      inputs by its format; that rule is retired, since the surface it picked is a render-resolution
      target of unknown role and converter targets are 1920x1080. None of this is in a capture here,
      because the exported action list records render target bindings and not shader resource
      bindings, which [ac7-frame-capture.md](research/ac7-frame-capture.md) states as a limitation
      twice.

      Doing the substitution: `runtime/graphics/scene_reinsert` promotes the composite and the
      interface target to output resolution, points scene colour at the reconstruction, and hands
      the tap a plan. The tap then swaps those bindings before forwarding them and scales viewports
      and scissor rectangles while a promoted target is bound, so the game tonemaps and grades the
      reconstruction with its own shaders, draws its own interface over it at output resolution, and
      its final upscale into the back buffer becomes a copy.

      Two decisions in that, both load bearing. Scene colour is substituted only after the composite
      has been bound in the frame, because the scene passes read scene colour while they are still
      writing it and an ungated substitution is a feedback loop rather than an upscale. And the
      reconstruction now runs at that same moment rather than at Present, because it is the one
      point where the scene is finished and nothing downstream has read it; evaluating at Present
      would leave the scene a frame behind the interface drawn over it. The price is a mid-frame
      evaluate, so the whole pipeline is saved and restored around it through
      `runtime/graphics/d3d11_state`.

      Cross-built with mingw-w64 and tested under Wine on DXVK, against a real device: the watch
      across seven states, the substitution checked by asking the context what actually got bound,
      the state save checked stage by stage after being deliberately disturbed, and the plan checked
      for what it gates and what it leaves alone. What no test here can reach is whether the
      substitution produces a correct picture, which needs the game's own shaders reading the game's
      own constants.

      Known and not fixed: bloom is still computed from the render resolution scene, so the glow
      composited over the reconstruction is low resolution. Post process shaders that address texels
      rather than sampling normalised will address the wrong ones, because their constants still
      describe the buffer the engine believes it has. Both are visible only in a rendered result.
- [ ] In-game overlay. The three pieces are now joined by a caller: `loader/proxy/overlay_host`
      loads the egui DLL, builds the renderer against the game's device, subclasses the window the
      swap chain presents to, and lays out and draws one frame per Present. F5 opens it. It starts
      as soon as the game has a device rather than at F8, so the panel can be opened to see that
      the backend is not running and why, and it fills its refusal line in the order the pipeline
      actually fails.

      The bridge applies start, scale, debug view, reinsertion, dump and capture intents after
      drawing on the Present thread. Quality and the master enable toggle remain unwired.

      F5 game runs reached layout but failed in back-buffer view creation. The observer could
      select a helper device before the presenting device existed. The host now derives its
      device/context from the presenting chain, checks back-buffer ownership, and releases its
      target references each frame. The worker-thread target experiment was discarded.

      Cross-built with mingw-w64. A regression test creates the wrong observer device deliberately
      and checks actual rendered pixels, target restoration and ResizeBuffers. The real egui DLL
      also passes that test under Wine with DXVK and RenderDoc, without DLSS. The old host
      reproduces the view-creation access violation in that two-device setup. AC7 validation of
      the fix remains pending. See [the investigation](research/ac7-overlay-device.md).
## Representation

The [representation plan](representation-plan.md) covers everything after the SR input chain: UI
extraction into a mod-owned premultiplied layer, the DX11 to DX12 presentation bridge, and a
vendor-neutral contract that DLSS, FSR and XeSS implement for super resolution and frame generation.
Each milestone ends deployed to the game and is judged by the log lines named in the plan. Status
classes are the tracker's: built, synthetic-tested, game-tested, device-tested.

- [x] M0. Compile again and decide the four uncommitted files. Built and synthetic-tested
      (`ec7f156`).
- [ ] M1. Classify every UI draw per screen, change nothing. The mechanism is built and
      synthetic-tested; the run that answers the question has not happened.
  - [x] The classifier as a pure function over shadowed facts, in `games/ac7/src/ui_rules.cpp`
        with a no-device test (`445fc54`).
  - [x] Vertex declaration and widget-target fingerprints, verified against UE 4.18.3 at
        `0a14a8d537a3` rather than the checkout's default 5.8.2 branch, where the same structures
        have a different shape (`0b16d2c`).
  - [x] Observer creation hooks for input layouts and both shader stages, with the texture
        descriptor reported for every texture rather than only filtered ones. Observer ABI 5
        (`1599517`).
  - [x] Re-entry counted rather than flagged, so a hook may issue context calls (`ad16848`).
  - [x] The tap shadows the pixel shader, blend and depth-stencil state, and reports them with the
        layout, vertex shader, stride and topology it already held. Frame tap ABI 7 (`cc64734`).
  - [x] Membership sets held by address with eviction before reuse, and Castagnoli shader hashes
        with the published check value pinned (`9210a29`).
  - [x] A candidate prefilter inside the tap, so the classifier sees the few draws in a frame worth
        describing rather than a callback per draw (`3039edc`).
  - [x] The loader wires creation to classification and reports the counts, `RSF_UI_CLASSIFY` on by
        default because it changes nothing.
  - [x] The run, 7 Sep 2026. It killed two rules rather than confirming them: converter targets
        cannot be found by descriptor shape (over 180 textures matched, and Slate also draws into a
        1920x3304 target no size list would hold), and the frame's own target had been wired to the
        composite rather than the back buffer. Both corrected; a converter target is now confirmed
        by watching Slate write into it. Twelve Slate declarations were named correctly by their
        element signature, and seventeen addresses were forgotten on reuse, so the registry's
        eviction fires in a real frame.
  - [x] Shader hashes printed in the trace and named from settings, so a wrong rule can be
        corrected without a rebuild (`8ac6959`).
  - [ ] A second run under the corrected rules. The counts from the first are superseded and are
        not carried forward as measurements. Screens are still reported by extent rather than by
        name: which screen the game is on is M3's question.

M2, extraction. Built, game-tested, and the test changed the design: the mechanism works and the
insertion point is wrong. Five runs on 7 September 2026.

- [x] `fullscreen_pass`: one triangle, four modes, the premultiplied composite all three frame
      generation SDKs specify. Verified by breaking it (`e1572f4`).
- [x] `ui_layer`: `R8G8B8A8_UNORM` at back-buffer extent, cleared to zero, double buffered,
      shareable. Its test measures under DXVK that Unreal's translucent blend leaves coverage at
      zero and that the patched blend accumulates, which is what the divert rests on (`061f55b`).
- [x] The divert primitive: retarget through the originals, viewport scaled by the fraction covered,
      blend alpha patched from a cache, everything restored, refusals counted with a reason
      (`a45ce13`).
- [x] Wired end to end on F3 (`fd67216`).
- [x] The runs. The interface reaches the screen at native resolution, 59584 draws diverted with no
      refusals and every written frame composited, which is the thing promotion could not do. Three
      faults were found and two of them were mine: the composite read the tail walk's back buffer
      field, which that walk releases after thirty-two frames, so the interface was diverted and
      never put back; and the colour transform belonged at the composite rather than at the layer.
- [x] Measured against the game rather than assumed: AC7's interface blend is already
      `ONE / INV_SRC_ALPHA` on colour and alpha with write mask `0xf`, so coverage was never the
      problem. The quads read the scene and its blur and glow chain as inputs, at render resolution,
      which makes them composites rather than overlays.
- [x] **The route changes.** Compositing at present skips AC7's own UI composite, its glow and its
      grade, and on the title screen the diverted widget texture is the whole picture, so the frame
      comes out flat and discoloured. Promotion of AC7's interface target is the correction: the
      game composites it, so the colour and the glow are the game's, and the promoted target is
      itself the premultiplied layer frame generation wants. The argument recorded here against
      promotion was wrong on its premise and is marked superseded rather than deleted.
- [x] `scene_promote` replacing `scene_reinsert`, 26 September. The plan promotes the composite,
      the interface layers and the chain. A layer is a target the classifier has seen a widget quad
      drawn into, refreshed every frame and dropped after 120 presents without one, so a layer the
      pool retires ages out instead of holding a plan slot; a chain target is an eight-bit
      render-resolution input of a composite draw, confirmed by watching the draw that writes it
      read the composite, with the composite watched again every 300 presents so a chain that
      appears after the tail walk still rebuilds the plan. Every set change rebuilds the plan on the
      present thread. Deleted with it: the shape hunt and its collection, the format rule for the
      interface target, and the tail relooks; the tap has four watch slots and no hunt. Built and
      synthetic-tested with MSVC on Windows. Not game-tested. The run has to answer two things the
      code cannot: whether the intermediate between the tonemap and the UI composite is filled by a
      draw (then it is promoted) or by a copy (then the log says it was never drawn into while
      watched, and a `CopyResource` hook is the next piece), and whether dropping the scene depth at
      the quads costs anything visible on the briefing.
- [x] The first Windows game run, 26 September, which took the day: the game crashed at launch
      (Steam's overlay and our vtable hook on Present forwarding to each other), then presented
      nothing (the genuine entry is patched too, by RivaTuner on this machine), then found no
      scene pass (the swap chain size was read once, at its DPI-scaled startup size), then
      refused every F6 (typeless surfaces need typed views). Each is measured and fixed in
      [the coexistence note](research/windows-present-hook-coexistence.md). At the end of it:
      Streamline 2.14.1 loads, DLSS evaluates every frame at 800x450 to 1600x900 with zero
      refusals, the panel's mouse works the way SpecialK's does, and F6 promotion ran for over
      13,000 frames with the composite, the chain target and two interface layers promoted, two
      depth mismatches per frame where the quads drop the scene's depth, and the layers aging out
      of the plan when the game moved to a screen without widget quads. Whether the picture is
      right is the next line.
- [ ] The judgement: the front end sharp, and the menu shimmer gone without closing the jitter
      gate. Not yet written down; the run above ended with the picture in front of the user.
- [ ] egui drawn into the layer, so it rides generated frames later.

M4, the presentation bridge, has its riskiest piece answered as far as this machine can answer it.

- [x] `shared_surface`: D3D11 textures and a fence created shareable and opened on D3D12, with the
      export, open and signal round trip covered (`510281f`).
- [x] Measured, and the answer is yes. Under a prefix built by `eng/wine-test-prefix.sh` from an
      installed Proton's DXVK and vkd3d-proton, every stage passes: a D3D11 texture opens on a
      D3D12 device on the same adapter, and a fence signalled on one side is seen on the other.
      Risk 7 is retired. Under the default prefix WineD3D returns `E_NOTIMPL` and the fixture skips,
      which is why the script exists: a fixture that had only run there would have reported the
      bridge impossible and been wrong about the only environment that matters.
- [ ] `texture_dump` fails under DXVK with the subprocess killed, reproducibly, while passing under
      WineD3D. Unexamined.
- [ ] The swap chain facade, the ring, and the intercept.

M5, the vendor-neutral contract, is built and tested without hardware.

- [x] `backend.h`: reconstruction and generation providers, capabilities, resources with their
      D3D12 states and lifetimes, and the swap chain ownership every vendor takes differently.
- [x] `negotiate`: a pure function over what each vendor said, so the combinations nobody has the
      hardware to try are decided and tested here. The Streamline single-session conflict, the
      no-bridge refusal, the multiplier cap, and the two different causes that produce the same
      D3D12 route (`fc54132`).
- [ ] Wrapping today's Streamline path behind it, the orchestrator session, and SDK ABI 2.

M6 and M8 have their vendor structure, with no implementation behind it.

- [x] `rsf_backend_fsr` and `rsf_backend_xess`: providers that exist whether or not the SDK headers
      do, answering `NOT_COMPILED` rather than failing to link, with capabilities filled from what
      each SDK is rather than from what this machine has. Both branches compile (`fc54132`,
      `7fa92f0`).
- [ ] Everything that does the work.

M3 has its foundation only.

- [x] `game_frame.h`: the frame record, the screen classes, the eligibility rules, and
      `rsf_camera_frame` moved out of the orchestrator into the SDK where a plugin can see it
      (`b23d18d`).
- [ ] `screen_policy`: video, graphics context and loading facts from the game. Needs Ghidra work on
      the `UManaComponent` anchors, and is better done with the extraction result in hand, because
      it decides which screens extraction is armed on.
- [ ] `graphics_settings`: the per-context render scale through `FGraphicsSettingsManager`.
- [ ] M2. Divert into the UI layer and composite at present with FG off: the layer, `fullscreen_pass`,
      `composite`, the divert primitive with the alpha-op blend patch, `scene_promote` replacing
      `scene_reinsert` (interface targets deleted, chain targets added), egui in the layer.
- [ ] M3. Frame identity, eligibility, and the game's own per-context screen percentage table.
- [ ] M4. Presentation bridge with a pass-through present and no FG.
- [ ] M5. Migration: `backend.h`, game SDK ABI 2, orchestrator session, `dlss_bridge.c` deleted,
      overlay ABI 3; `rendering_ready` flips only with this milestone's evidence.
- [ ] M6. Frame generation, FidelityFX first (any D3D12 GPU, no XeLL, no Streamline device conflict).
- [ ] M7. Frame generation, DLSS-G; DLSS-SR moves to the D3D12 proxy while DLSS-G owns FG.
- [ ] M8. Frame generation, XeFG with XeLL, non-Intel mode on the development machine.
- [ ] M9. Super resolution per vendor behind `rsf_sr_provider` (FSR via the bridge, XeSS-SR D3D11
      on Arc).
- [ ] M10. Claw device run: XeSS-SR D3D11, XeFG 3x/4x, XeLL. The only device-tested milestone.

Retained from the earlier list and folded into those milestones: the loader and orchestrator
handshake (M5), plugin lifecycle (M5), the synthetic bridge (M4), XeLL and MFG on the Claw (M8,
M10), frame identity and scene/HUD boundaries (M1 to M3), reinsertion into post-processing (M2),
combined SR and MFG (M6 to M10). Still outside the plan:

- [ ] Standalone profiles, native egui rendering/input, and handheld controls.
- [ ] WSGM launch/profile integration, limiter coordination, and rendered-frame AutoTDP inputs.
- [ ] Matched-condition visual, frame-time, latency, and power measurements.

## AC7 motion and velocity improvements

The engine's camera transform is already available through `ClipToPrevClip`; camera motion is
not being estimated from images. The current DLSS path decodes the sparse object-velocity buffer
and asks Streamline to resolve unwritten pixels using depth and that transform. The tasks below
include completed offline investigation and proposed changes. The
[30 September research](research/ac7-motion-vectors.md) found a conversion discrepancy against
native shaders and a valid-zero limitation in the deployed Streamline kernel. Corrections and
producer extensions have not been game-tested.

- [x] Disassemble the 207 recorded native VS/PS blobs and the deployed Streamline motion kernel.
      The ordinary velocity producer stores total current-minus-previous NDC; the inspected
      temporal consumer replaces depth-derived camera motion at written pixels and exports colour
      only. The custom projection branch has different jitter selection and remains unidentified.
- [x] Compare RenoDX and Luma shader replacement at pinned revisions. Author an independent
      packed-output plus float-motion sidecar probe. It compiles as `ps_5_0` and creates on Windows
      D3D11 WARP; draw readback, binding integration and game validation remain undone.
- [x] Check captured `ClipToPrevClip` against both frames' unjittered translated projections.
      Maximum absolute matrix error is `1.44e-6` across 472 repeated bindings over two briefing
      frames. This is capture analysis, not a new stationary-camera or flight test.
- [x] Locate AC7's small-object velocity selection gate in the decrypted image. The known dynamic
      loop calls `ShouldRenderVelocity` at RVA `0x10fdc00`; size rejection is `76 38` at
      `0x10fdd0c`, followed by history/transform checks in `HasVelocity` at `0x1183820`.
      Native/source matched with Capstone and named/decompiled in the local Ghidra project.
      Eligibility bypass and affected-scene validation remain pending.
      [Evidence](research/evidence/ac7-velocity-coverage-20260930.json).
- [x] Build opt-in F9 motion capture with guarded engine observations, shader/CB records and
      lossless backend input samples. Windows WARP capture and readback tests pass; the Release
      verification gate passes (30 tests passed, four vendor-environment tests skipped).
      AC7 capture validation remains pending. See [capture procedure](../loader/README.md).
- [x] Analyze six 30 September F9 captures and isolate hangar colour posterization with a
      frozen-input RTX 4070 Laptop experiment. Performance preset K is smooth at unchanged
      800x452 input and 1600x900 output. Forced K was withdrawn because driver overrides can
      defeat it; actual NGX flags preserve HDR and the preset-independent fix remains open.
      R11/FP16/FP32 GPU input comparisons and FP32 output also preserve the same bands;
      widening formats does not solve this captured failure.
      A diagnostic perceptual-SDR input is smooth, but the actual game route is unimplemented.
      Bounded tonemap/scene-effect/HUD/composite snapshots now support that investigation.
      Recognize the captured fullscreen flight HUD producer
      and preserve its promoted output; add final-screen raw/preview captures. Windows Release
      verification passes. Corrected AC7 run and pause/resume validation remain pending.
      [Research](research/ac7-ui-hdr-20260930.md).
- [ ] Prioritize the reported carrier-launch and refuelling weapon/attached-missile omissions,
      then possibly missing moving ground vehicles. Match each visible colour draw to its
      velocity draw or exact rejection reason, separating per-primitive bounds from material/LOD
      sections. Confirm previous world transforms, attachments, bone motion and valid zero values.
      Only then apply a guarded size-gate bypass or class-specific producer change and measure cost.
- [ ] Audit TrueSky cloud motion with a stationary camera and with rotation/translation. The
      installed renderer has wind and cloud-reprojection string leads, not proven output vectors.
      Trace its own time/depth/history inputs; static density still needs camera motion and finite
      cloud depth. Independent drift/evolution and multilayer blending need separate validation.
- [ ] Capture flight particle producers. Stock CPU sprite streams expose `OldPosition` from
      `Particle.OldLocation`, although their velocity helper returns current position. Confirm
      AC7's input layout, history interval and previous billboard/deformation state before adding
      a VS/PS motion output. Beam/trail fill paths sometimes duplicate current into old position.

- [ ] Complete the engine path inventory in flight. The captured temporal and reflection consumers
      are inspected; compute/motion-blur and flight-specific shaders remain. Locate their per-pixel
      camera-motion calculation and establish whether a complete, reusable motion texture exists
      or the result is only an intermediate shader value. Record the pass, inputs, encoding,
      extent, timing, and lifetime. Follow the later correction in
      [ac7-frame-capture.md](research/ac7-frame-capture.md): resource 63083 was identified as a mask,
      not velocity flattening; capture resource IDs are not runtime identifiers.
- [ ] Choose the motion source from that evidence. Prefer reusing a suitable engine-produced
      full-motion texture. If the engine only computes motion inside a consuming shader, evaluate
      exporting that intermediate through a shader change versus retaining Streamline's existing
      resolve. Engine camera transforms still need depth to become per-pixel displacement.
      Do not assume an engine-produced result is more accurate without matching conventions.
- [ ] Establish the exact meaning of written object vectors. Determine whether they already
      use the verified ordinary total-motion path or AC7's unidentified custom projection path.
      Preserve valid zero motion separately from unwritten pixels, and never add camera movement
      twice. Verify the sentinel handling against the Streamline version actually loaded.
- [ ] Reconcile the Rust and live C/C++ motion conversions. `MotionToPixels::unreal` in
      `runtime/backends/rsf-upscaler/src/frame.rs` uses half the viewport extent and a vertical
      sign flip, while `loader/proxy/src/dlss_bridge.c` and
      `runtime/orchestrator/src/dlss_pipeline.cpp` currently default output/backend scales to
      one. Trace clip displacement, normalized UV displacement, pixel units, and temporal
      direction end to end. Measure aircraft or missile displacement on both axes before applying
      the proposed ordinary-path Streamline scale `(0.5, -0.5)`. Rust's helper expresses the
      opposite temporal direction from the FSR/XeSS resolve; distinguish direction from units.
      Matching a numeric range alone does not establish matching units.
- [ ] Verify jitter throughout reprojection. In `games/ac7/src/view_uniforms.cpp`, establish
      whether the captured unjittered `ClipToPrevClip` result holds in stationary and flight views,
      and identify the custom producer's projection/jitter selection.
      Removing jitter from `ViewToClip` alone does not prove this. With a stationary camera and
      changing jitter, unjittered camera motion must remain zero. If necessary, remove both frames'
      jitter transforms and recompute the inverse. Explicitly match the backend's vector-jitter
      flag to the measured object-vector convention.
- [ ] Add motion diagnostics: raw/written-pixel coverage, decoded object vectors, camera-only
      vectors, resolved vectors, and previous-frame reprojection error. Expose the actual submitted
      scales, jitter, frame identity, and reset reason. Separate disocclusions and changing shading
      from vector errors; do not judge correctness from a vector colour plot alone.
- [ ] Propagate camera cuts and interrupted history through the AC7 camera frame. Prefer the
      engine's cut/history state, with evaluated-frame continuity tracking for missed frames.
      Cover mission loads, camera-mode switches, and resource/resolution changes. The pipeline
      currently adds a reset on rebuild; ordinary fast flight must not be treated as a cut.
- [ ] Audit geometry coverage using the diagnostics: aircraft, missiles, animated control surfaces,
      and other independently moving or deforming geometry. Where writes are missing, first look
      for an existing engine velocity path that can be enabled. Otherwise assess a shader change
      or additional velocity pass using previous object transforms or deformation state. Opaque
      depth alone cannot recover independent object movement.
- [ ] Investigate clouds, smoke, and contrails separately. Locate their own depth, history, and
      motion inputs, and assess whether they can supply useful motion for reconstruction.
      Validate sky reprojection at clear/reversed-Z depth separately from ordinary geometry.
      Do not assign opaque background motion to transparent layers as though it were exact.
- [ ] Confirm the screen-droplet/refraction pass and place it after reconstruction where feasible.
      Coordinate this with output reinsertion so the scene is reconstructed before the game's
      grade and HUD composite. Recheck all relevant extents at reduced render scale; full-resolution
      captures alone did not establish an output-resolution HUD path.
- [ ] Compare the existing explicit-sentinel dense resolve with Streamline's sparse resolve for
      DLSS, since the deployed kernel treats valid zero as unwritten. Keep this independent of
      producer/scale changes and measure GPU cost. If submitting a
      complete field, set backend metadata accordingly to prevent another camera-motion resolve.
      Validate depth-aware edge dilation and its metadata without applying it twice.
- [ ] Validate in controlled captures and live motion: stationary camera with jitter, horizontal
      and vertical pans, forward flight near terrain, roll/FOV changes, tracked aircraft, crossing
      missiles, clouds/contrails, and camera cuts at native and reduced render resolution.
      Check colour/depth/motion/camera frame alignment and GPU cost. Record synthetic-tested,
      capture-validated, and game-tested results separately before marking tasks complete.

## Reusable SR orchestrator

- [x] Game-independent SR session and D3D11/D3D12 transfer API. Transactional backend selection,
      requested/effective status, SDK version reporting and history reset/refusal handling.
- [x] FSR2, FSR3 and FSR4 family selection with version overrides and separate SDK runtime paths;
      XeSS D3D12 adapter. FSR2/FSR3/XeSS device-tested on Intel UHD and RTX 4070 Laptop. FSR4
      refusal tested on those adapters; FSR4 evaluation still requires compatible hardware.
- [x] Generic sparse-motion/depth preparation, WARP numeric fixture and controller rollback tests.
- [x] Compatibility path and Insert overlay selector for DLSS/FSR2/FSR3/FSR4/XeSS. Built;
      synthetic hardware sequence DLSS → FSR2 → FSR3 → XeSS → DLSS passed. Engine identity and
      world units are supplied by the integration. Existing AC7 startup still begins with DLSS.
- [ ] Game-test AC7 switching, motion, exposure, transitions and overhead. New FSR/XeSS game
      support is not claimed. Public plugin lifecycle migration, native XeSS D3D11, FG and latency
      remain separate work. [Research, revisions and limits](research/orchestrator-sr-switching.md).

## Engine integration improvements

The observed AC7 FSR/XeSS switch refusals were missing vendor DLLs (Windows error 126).
Signed SR runtimes and notices are now deployed with hash verification. The hardware fixture
passes DLSS -> FSR2 -> FSR3 -> XeSS -> DLSS using the installed paths. This is a deployment fix
and synthetic device evidence; fresh AC7 switching/visual validation remains pending.
[Evidence](research/sr-runtime-deployment.md).

The reusable FG contract, CPU frame ledger, ownership/fallback controller, FSR3/4 and XeSS FG
providers, and early D3D12 Streamline/DLSS-G host are implemented. Synthetic controller tests pass.
DLSS-G, FSR3 and XeFG passed disabled-presentation lifecycle probes on the RTX 4070 Laptop GPU;
FSR4 was refused. Enabled FG, game export/facade integration, independent PCL/pacing transitions,
shared D3D12 DLSS SR and latency/visual validation remain open. [Evidence and limits](research/orchestrator-fg-implementation.md).

Independent PCL and Reflex services now expose cold profiles, shared token ownership and bounded
status without requiring FG. PCL-only API/render/Present checks pass on NVIDIA and Intel with zero
sleep calls; Reflex Off/On/Boost checks pass on NVIDIA with one sleep per source frame. This is SDK
API evidence, not ETW validation or physical latency measurement. [Evidence](research/orchestrator-latency-services.md).

The Ghidra MCP extension and stdio bridge are installed and connected to the retained AC7 dump.
Six CPU boundary names pass byte-span checks and are recorded in a tracked replay manifest.
Auto-analysis finished and the database save succeeded. This is static research, not a
game-tested CPU/render handoff. [Recovery evidence](research/ac7-fg-cpu-boundaries.md#ghidra-recovery-and-repeatable-names).

The reusable render-link registry carries source IDs through engine-owned queued objects without
using the latest CPU frame. Delayed rendering, multiple submissions, capacity refusal and stale
pointer/ticket checks pass synthetic tests. Seven AC7 renderer/family/task functions are named,
byte-checked and saved. Native Release verification passed (31 tests, five opt-in fixtures skipped).
Game hooks, main-view selection, RHI handoff and final FG export remain unwired.
[Ownership trace and limits](research/ac7-render-frame-handoff.md).

The current [frame-generation orchestrator plan](frame-generation-plan.md) defines FG0-FG9 for
DLSS-FG/MFG, XeSS-FG/MFG, FSR3/FSR4 FG, Reflex/XeLL and independent PCL telemetry. Planning/source
inspection is recorded on 30 September 2026; subsequent implementation evidence is linked above.
Reuse the completed root UI work and finish its paired FG export. Shader replacement and
motion-quality improvements belong to the separate motion workstream; FG owns the input contract,
presentation integration and compatibility/acceptance checks.

Use the existing plugin/runtime ownership split. These tasks extend the first working target and
the motion investigations above; none is a claim of completed implementation.

- [ ] Capture view data on the CPU when UE4 constructs or uploads it, with the matching render-frame
      and view identity. Measure the current constant-buffer staging allocation, copy, and immediate
      Map cost before replacing it. Reusing staging allocations is an interim improvement only;
      do not introduce stale camera data to avoid a synchronous readback.
- [ ] Promote discovered AC7 passes into explicit, verified engine-function or shader identities,
      with resource checks as confirmation. Keep broad format/binding discovery as a diagnostic
      mode. Select resources for a known pass, view, and frame rather than the first plausible set.
- [ ] Establish a frame record with render-frame ID, view ID, valid rectangles, camera/exposure
      state, and resource-use boundaries. Distinguish rendered frames, Present calls, and generated
      frames. Define immediate-consumption versus copy-before-reuse obligations; retaining a
      texture's allocation does not preserve its contents.
- [ ] Complete reinsertion at the chosen scene reconstruction boundary before expanding into FG.
      Return an output-resolution result to downstream passes and preserve grading, effects, and
      an output-resolution HUD. Bypass only the original filtering/upscaling that SR replaces;
      investigate cloud-specific temporal accumulation before disabling any temporal pass.
      The substitution and the output-resolution HUD path are built behind F6 and described in the
      first working target above; what remains here is running it against the game, the bloom and
      texel-addressing consequences recorded there, and the cloud temporal question.
- [ ] Verify the engine's exposure and pre-exposure convention, frame alignment, and backend
      conversion. Identifying a 1x1 texture alone is insufficient. Compare against auto-exposure
      during bright/dark transitions and record the source and effective values in diagnostics.
- [ ] Verify texture mip selection at reduced render resolution. Apply any required bias through
      appropriate engine/material paths, restore it when SR is disabled, and compare fine-detail
      recovery against shimmer. Avoid indiscriminate global bias or sharpening as a substitute.
- [ ] Sequence the integration work around CPU-side view capture, explicit pass/frame identification,
      correct reinsertion, and exposure/mip tuning before additional backends and FG. Build the
      egui inspection and capture tools alongside these steps so they can validate each change.

## egui development and validation workflow

Use the prepared egui overlay as the main in-game development and validation interface as well as
the eventual player settings UI. Prioritize effective settings, buffer inspection, frame-time plots,
and capture of the next N frames; prove the capture format before building offline sequence replay.

- [ ] Connect the existing egui DLL, D3D11 renderer, and input hook through the runtime lifecycle.
      Support opening/closing the interface, correct input ownership, graphics-state restoration,
      and clean shutdown. Reuse the existing overlay implementation rather than creating another UI.
- [ ] Implement a bounded command/status path: egui requests changes, the render integration applies
      them at a defined frame boundary, and the UI reports effective settings or a refusal reason.
      Keep vendor calls and per-frame GPU work in the runtime, and game-specific preparation in AC7.
- [ ] Add a Compare view for native game output, native-resolution DLAA, reduced-resolution DLSS,
      and ordinary scaling at the same reduced resolution. Support split-screen or a movable divider
      where inputs are matched. Label a genuine native reference as a separate render or matched
      run; it cannot be recovered from the same low-resolution input.
- [ ] Make comparisons temporally valid. Handle history resets when changing backend or resolution,
      label warm-up periods, and compare settled results. Use independent histories if running
      multiple temporal configurations together. Record the extra cost of comparison mode.
- [ ] Add an Inspect view for scene colour, depth, raw/decoded/resolved motion, camera-only motion,
      written-pixel coverage, exposure, and reprojection error. Include pixel inspection, units,
      display scale, and resource/frame identity. Distinguish disocclusions and shading changes
      from motion errors. Use this view to deliver the motion diagnostics listed above.
- [ ] Separate freezing the displayed diagnostic image from pausing processing. Continue maintaining
      valid live history when only the display is frozen, and define reset/resume behaviour when
      processing stops. Never feed repeated or mismatched frames to a backend accidentally.
- [ ] Add a Capture view with capture-next-N-frames, progress, cancellation, and completion/error
      status. Save matching colour, depth, motion, available exposure, camera matrices, jitter,
      resets, frame IDs/timing, extents, formats, and relevant settings in a versioned sequence
      format. Include game fingerprint, RSF commit/build, backend/SDK version, adapter, driver,
      graphics API, and capture stage so results can be reproduced and compared.
- [ ] Use bounded GPU readback queues with completion tracking and background file writing.
      Configure memory/disk limits and report dropped or incomplete frames and their identities.
      Mark gaps that invalidate temporal replay. Avoid silently stalling gameplay and measure the
      capture overhead; GPU context work stays on its owning thread.
- [ ] Keep diagnostic UI out of captured scene inputs and backend history. Offer a separate
      annotated screenshot/export when overlay information is wanted. Preserve unmodified numeric
      buffer data alongside any tonemapped or colourized previews.
- [ ] Add a Performance view with frame-time history, CPU hook/readback time, GPU decode/SR time,
      resource rebuilds, and diagnostic overhead. Retrieve GPU timings without forcing immediate
      synchronization and identify unavailable/invalid measurements. Distinguish rendered and
      presented/generated rates; do not label either as input latency.
- [ ] Add a Status view showing selected pass/view, actual render/output sizes and valid rectangles,
      requested/effective configuration, frame continuity, history reset reasons, missing inputs,
      backend support, and capture queue/dropped-frame status. Bound logging and graph history.
- [ ] Once live capture is validated, implement offline replay of complete sequences through a
      supported backend with their original order, constants, exposure, and reset state. Reject
      incompatible/incomplete inputs or mark comparison limits explicitly. Use replay to compare
      motion conversions and backend settings without requiring another full game session.
- [ ] Define repeatable visual/performance runs using the motion scenarios above plus exposure
      transitions and fine-texture scenes. Compare native, DLAA, reduced-resolution DLSS, and
      ordinary scaling under matched conditions. Export settings, warm-up rules, capture identity,
      image comparisons, and timing summaries; separate backend cost from tooling overhead.
- [ ] Validate the initial egui workflow in game: change settings and confirm effective values,
      inspect buffers, capture a bounded sequence, exercise cancellation/queue overflow, and check
      that opening, freezing, or closing diagnostics does not corrupt history or game input.
      Record built, synthetic-tested, capture-validated, and game-tested status separately.

- [x] Repair the 30 September mixed proxy/overlay deployment: ABI 4 proxy paired with ABI 3
      panel prevented Insert startup. The rebuilt real Rust panel passes Insert, draw and resize
      checks; matched pair deployed with backups. Add explicit pair deployment/preflight tooling.
      Corrected AC7 run pending. [Research](research/ac7-consumer-session.md).

## Repository review follow-up

The [30 September renderer ownership audit](research/ac7-renderer-roots-20260930.md) changes
the SR integration work: late resource identities can alias an earlier GBuffer. Successful
backend evaluation is not evidence that the supplied scene includes intact lighting/reflections.

- [x] Capture the pooled GBuffer/tonemap alias and premature lighting/reflection SRV replacement.
      Gate composite/chain promotion by phase, refuse MRT promotion/gates and call shared gates
      once per binding. Windows MSVC Release built and synthetic regression passed: 31 tests
      passed, five vendor skips, Rust formatting/lint passed. User confirms lighting/reflections
      restored in the 19:09 game build. Jitter/shimmer returned and remain unresolved.
- [x] Map and name native post-processing, per-view renderer and widget converter roots in the
      decrypted Ghidra project, plus main temporal construction/Process/ComputeOutputDesc,
      composition descriptor gathering/execution, RHI commands and pre-visibility/jitter setup.
      Post-processing/widget F9 scopes game-captured in 14 sessions (42 paired samples). All 42
      backend jitter pairs match native view samples; sampled cuts are zero. CPU scopes end before
      the post-process draws, so TLS alone is insufficient. Expanded graph/RHI capture built and
      deployed with expected-byte guards; runtime validation pending.
- [x] Validate sampled graph-to-RHI command associations: nine completed process-215904 sessions,
      27 paired samples and 1119 matched draw/dispatch records with no native/command drops. Name
      Tonemap/FXAA/material/Nimbus HUD/composite descriptor/processing methods from runtime vtables.
      This sampled result does not establish all queue/Present identities.
- [x] Diagnose the pause-menu F9 crash using an identical-code linker map and minidump: route copy
      used an unretained borrowed translucency texture. Add owned retention/current-frame capture
      checks and pre-execution native checkpoints; emit a Release linker map. Windows MSVC Release
      verified, matching proxy/overlay deployed; corrected crash scene pending game validation.
      [Evidence](research/ac7-graph-capture-crash-20260930.md).
- [ ] Make the identified main temporal node and engine graph own SR inputs/output
      descriptors and rectangles. Flight tonemap/scene-effect still write 928x524 before 1600x900
      final composition; this is a measured extent problem, not the proven whole shimmer cause.
- [ ] Move UI resolution to widget producers and dependent downsample/glow allocations; preserve
      logical layout, refresh timing and real depth occlusion. Retire settling/discovery heuristics
      only after equivalent transition captures pass.
- [ ] Reassess remaining hangar banding using an intact input scene. Keep driver preset selection
      unrestricted. Post-tonemap SR is a possible experiment, not the established correction.

Open defects from the [7 September review](review.md). These are not fixed by the documentation rewrite.

- [ ] Move F8 startup and F7 state changes to a render-thread command boundary; eliminate concurrent immediate-context use.
- [ ] Reject non-finite camera, projection, reprojection, and jitter data in the reader/assembly path; add poisoned-input regressions.
- [ ] Roll back every observer hook after partial installation failure, and quiesce callbacks before teardown.
- [ ] Fix RGBA16F colour selection in the frame tap and test colour/history selection together.
- [x] Parse settings so explicit zero values work. `read_number` uses `strtoul` with base 0, so hexadecimal
      overrides and a present-and-zero value are honoured (7 September). Type/range validation and a
      `config_parse` test land with the representation plan's M5.
- [ ] Gate the debug blit on a successful evaluation of the current frame and reset history after gaps.
- [ ] Preserve input press/release events between frames so egui does not lose short clicks.
- [ ] Add Rust test execution and a pinned SDK-header compile job to CI; keep hardware evaluation a separate gate.
      Planned as the two-leg vendor matrix in the [representation plan](representation-plan.md).
- [ ] Make capture-timeline reads reflect inherited bindings at draws, or explicitly report unsupported tracking.
- [ ] Derive discovery extents from verified render data/backend planning so inputs below 50% can be selected.

The [validation plan](research/validation-plan.md) defines acceptance. Keep build, synthetic, game, and target-device results separate.

Native widget producer continuation: QueueRender/PrepareTargets/DrawScaledWindow now own physical
raster density while preserving logical layout; owned glow siblings resize and replacement painting
bypasses refresh skipping. Native settings stop writing screen-percentage cvars. FSR/XeSS receive
engine exposure with transactional context policy changes. MSVC Release built only; not deployed,
no new tests. Borrowed UI targets, native UI isolation and FG identity remain open.
[Evidence](research/ac7-native-renderer-refactor-20261001.md).

Native UI composition: ABI3 appends leased ui_input from the verified view-owned HUD target.
Runtime now owns demand-driven paired scene/UI/composed GPU snapshots with native scope identity;
F9 writes these layers. MSVC Release built, not deployed/game-tested. Raw UI alpha/effects and
simulation/Present identity remain unproven; these inputs do not authorize FG yet.

Native renderer identity continuation: ABI4 copies input-loop source frame and per-renderer
submission ID through native SR/HUD packets. Bounded constructor bindings retire before native
delete; stop separately drains the outer Tick return path after disabling hooks. Existing
post-SR native jitter removal/uniform rebuild confirmed. MSVC built only, no new tests/deployment.
Final Present association, vendor markers and borrowed UI allocation ownership remain open.

Shared UI producer migration now controls the verified game-instance mono/stereo canvas allocator,
keeps its1920x1080 logical layout and resizes before borrowed widget rendering. Replacement-frame
contributors repaint via native dirty state, with subsequent RefreshFPS preserved. MSVC Release
built only, no tests/deployment/game validation. Dormant/stereo/DPI/glow transitions and final
presentation/latency integration remain open. [Research](research/ac7-native-renderer-refactor-20261001.md).

Native ownership audit correction: renderer retirement drains final RHI pool releases before/after
its native wait/delete helper. Composition accepts only explicitly declared primary-view packets
and requires complete paired identity/rectangle equality; bounded capture counts failures too.
MSVC built only; no new tests/deployment. CPU semantic markers and final presentation remain open.

Native update association: derived/base GameEngine Tick and RedrawViewports hooks bind the
update-to-render phase into renderer identities and queued packets. Long-call guards cover
synchronous Present and retirement waits. MSVC built only, no tests/deployment/game execution.
Actual Reflex/PCL marker/sleep routing and final presentation association remain incomplete.

Native CPU event handoff now uses ABI5 copied callbacks for outer/input/simulation boundaries.
Runtime owns bounded CPU sequence state and a real-time optional consumer; F9 pairs source/QPC
metadata with native composition identity. MSVC built only, no tests/deployment/game execution.
Vendor sleep/marker consumer and final render/submission/Present association remain incomplete.

Main viewport submission provenance now binds through native FViewport Draw and renderer
construction (ABI6 viewport_key), queued packets/composition/F9 metadata. MSVC built only; no
tests/deployment/game execution. Slate's actual window/RHI Present handoff remains separate and
unfinished, along with vendor sleep/marker wiring and the final completion audit.

Pass completion correction: ABI7 separates semantic before/after callbacks from scope-state
restoration, resolves output resources again at end and removes an unused global pass copy.
MSVC built only, no tests/deployment. Slate window/task/RHI end-drawing functions are now mapped
and named; their queued source/swapchain handoff and vendor markers remain unfinished.

Queued Slate window ownership is implemented: bounded task-to-source binding, main-window checks,
leased native swapchain and execution-thread COM identity match at the existing Present observer.
ABI8. MSVC built only; no tests/deployment/game run. Shipped getter/layout, scene-resource agreement,
Present flags/result/generated handling and vendor Reflex/PCL consumer remain unfinished.

Actual Present completion contract implemented (observer ABI7, game ABI8 unchanged): original
interval/flags/HRESULT preserved, test calls bypass per-frame graphics work, runtime stores copied
numeric source/window completion/QPC evidence, and counter records accepted non-test calls.
MSVC built only; no tests/deployment/game run. Present1/generated handling, scene-resource agreement
and real vendor sleep/marker consumption remain incomplete.

Present1 coverage implemented through the shared overlay-aware installer/observer body (observer
ABI8). Original parameters survive forwarding; recursive internal calls do not duplicate events.
Route publication precedes activation and in-flight calls block uninstall. MSVC built only; no
tests/deployment/game execution. Scene agreement, vendor consumer and live coverage remain open.

Scene/window declaration agreement is implemented without capture copies: unique completed
primary composition matches the exact Present source/session/viewport and is rechecked after
return. MSVC built only, no tests/deployment. Actual sampled texture/final HUD-less resources,
vendor sleep/marker setup and full completion validation remain unfinished.

Final family surface producer implemented (ABI9): full native postprocess scope leases the real
family render target through RHI completion, replacing HUD-intermediate identity. No ordinary
GPU copies added. MSVC built only, no tests/deployment/game execution. Exact Slate sampling/late
canvas contributions and vendor sleep/markers remain incomplete.

Ordinary Slate texture binding path implemented (ABI10): candidate-only source-surface filtering,
leased native texture across queued bind commands, exact source/window comparison and separate
texture-bound Present evidence. MSVC built only, no tests/deployment/game run. Other consumers,
late canvas/HUD-less resources and vendor sleep/markers remain unfinished.
