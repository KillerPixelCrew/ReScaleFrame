# Implementation tracker

ReScaleFrame is a monorepo. All first-party components share this history and release version. Separate runtime/plugin DLLs do not imply separate repositories.

## Repository foundation

- [x] Native CMake targets for bootstrap, orchestrator, AC7 plugin, and launcher.
- [x] Minimal C ABI for plugin metadata and executable recognition.
- [x] Rust workspace with an egui status surface.
- [x] Architecture, UE4.18 source leads, and presentation research retained in the repository.
- [x] Local Release build, C/C++ SDK compatibility, plugin contract checks, Rust formatting, and Clippy verification.
- [x] GitHub Actions workflow for native and Rust verification; individual run results are tracked in Actions.

## First working target

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
- [x] Live DLSS evaluation. Initial run: 2,176 recognized passes, 2,175 evaluations, no refusals. Mission run on 7 September: 7,917 evaluations, no refusals, 1024×576 input and 2048×1152 output. Recorded images show recovered detail and a complete scene; flight showed no obvious smearing. F7 is a debug display; reinsertion, grading, HUD, and controlled motion validation remain pending. [Evidence](research/ac7-frame-capture.md).
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
      Game-tested on 7 September: the briefing relief draws at 2048×1152 inside a 1024×576 scene
      with a matching depth, and reaches the reconstruction. Flight, the post-mission replay and
      other heavy screens are unverified. [Evidence](research/ac7-frame-capture.md).
- [ ] Reinsert the result. A debug view exists behind F7 and is game-tested: a full screen draw over
      the back buffer from inside the Present hook, with a rough tonemap so linear scene colour is
      viewable. It is what showed the reconstruction moving, which is the only way ghosting and a
      motion vector's sign can be judged. It is not the real path, which reinserts the reconstructed
      scene before the game's own composite so the grade and the interface survive. That is the
      remaining structural piece and the reason the picture is ungraded and has no HUD.

      The whole path is built behind F6 and has been run against the game. It took three runs to get
      there and each failed differently, which is worth keeping because the failures were all the
      same mistake about bindings.

      First run: nothing happened at all. The tail walk had taken a 2048×32 strip as the composite,
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

      Superseded, 7 September. The interface was never a target to promote. AC7 rasterizes it at a
      fixed 1920x1080 and, on the briefing and hangar, draws it as world-space widget quads into its
      own render-resolution `R8G8B8A8` layer, depth-tested against the scene, before compositing it
      itself. Promotion cannot sharpen that; extraction can. The mechanism below is replaced by the
      [representation plan](representation-plan.md): a mod-owned premultiplied UI layer, a HUD-less
      scene, and a composite at present. The account of the three runs stays because the failures
      were all the same mistake about bindings.

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
  - [ ] Shader hashing and the registries that turn these reports into named sets.
  - [ ] Per-screen classification counts in the run log, and the run itself.
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
are investigations and proposed improvements, not confirmed defects or game-tested changes.

- [ ] Investigate the engine path first. Inspect the shaders that consume AC7's scene velocity
      buffer, including temporal filtering and motion blur where present. Locate their per-pixel
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
      include camera movement at those pixels or contain only an object-motion contribution.
      Preserve valid zero motion separately from unwritten pixels, and never add camera movement
      twice. Verify the sentinel handling against the Streamline version actually loaded.
- [ ] Reconcile the Rust and live C/C++ motion conversions. `MotionToPixels::unreal` in
      `runtime/backends/rsf-upscaler/src/frame.rs` uses half the viewport extent and a vertical
      sign flip, while `loader/proxy/src/dlss_bridge.c` and
      `runtime/orchestrator/src/dlss_pipeline.cpp` currently default output/backend scales to
      one. Trace clip displacement, normalized UV displacement, pixel units, and temporal
      direction end to end. Measure aircraft or missile displacement on both axes before changing
      factors or signs; matching a numeric range alone does not establish matching units.
- [ ] Verify jitter throughout reprojection. In `games/ac7/src/view_uniforms.cpp`, establish
      whether the engine's `ClipToPrevClip` remains unjittered after enabling temporal jitter.
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
- [ ] Consider a shared dense-motion resolve only if needed for another backend or diagnostics.
      Extend the existing decode pass with depth and verified camera data when useful for XeSS/FSR,
      rather than making a second camera reconstruction mandatory for DLSS. If submitting a
      complete field, set backend metadata accordingly to prevent another camera-motion resolve.
      Validate depth-aware edge dilation and its metadata without applying it twice.
- [ ] Validate in controlled captures and live motion: stationary camera with jitter, horizontal
      and vertical pans, forward flight near terrain, roll/FOV changes, tracked aircraft, crossing
      missiles, clouds/contrails, and camera cuts at native and reduced render resolution.
      Check colour/depth/motion/camera frame alignment and GPU cost. Record synthetic-tested,
      capture-validated, and game-tested results separately before marking tasks complete.

## Engine integration improvements

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

## Repository review follow-up

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
