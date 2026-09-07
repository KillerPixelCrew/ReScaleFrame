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

- [x] Decrypted AC7 module capture and import map for offline analysis. Loader diagnostic only:
      it performs no interception and touches no graphics object. Cross-built with mingw-w64 and
      tested under Wine, and run against the installed game on 6 September 2026. Results are in
      [ghidra-tooling.md](research/ghidra-tooling.md). Not yet built with MSVC.
- [x] Render analysis of the installed game: frame timeline, both velocity targets, the temporal
      AA pass identified by its inputs, and the view uniform buffer read live with offsets
      confirmed against engine source. Game-tested. See [ac7-frame-capture.md](research/ac7-frame-capture.md).
- [x] Temporal jitter revived by patching the anti-aliasing gate, verified in the running game:
      every perspective view now carries a sub-pixel offset where all fifty earlier captures were
      zero. Game-tested, not yet built with MSVC.
- [x] Console variables set in the running game, and render scale confirmed working through
      `r.ScreenPercentage`. Game-tested, not yet built with MSVC.
- [x] Vendor-neutral upscaler model in Rust: quality levels, AC7's motion vector encoding taken
      from engine source, and a viability check that reports every blocker rather than the first.
      Unit-tested only. No vendor SDK is vendored and nothing here touches a GPU.
      See [runtime/backends/README.md](../runtime/backends/README.md).
- [x] Per-frame conversions out of Unreal's units: clip space jitter to pixels, screen space motion
      to pixels, and the jitter sequence length a render scale calls for. Unit-tested against the
      offsets and the largest motion recorded from the running game. The vertical sign follows
      engine source and has not been checked against a rendered result.
- [x] Jitter sequence length set in the game alongside the render scale, and the render scale holds
      itself: loading a mission puts the game's own screen percentage back, so it is re-applied on a
      timer, which does nothing while the scale is already ours. Game-tested through a full mission.
- [x] DLSS through Streamline: load, device handover, support query, render size planning, resource
      tags and per frame constants, behind the C ABI in `runtime/backends/dlss`. Runs in the game.
- [x] View uniform buffer mapped, including `ClipToPrevClip`, the camera basis and the projection.
      Offsets are checked by an identity that ties five of them together rather than fitted to one
      buffer, and `tools/verify-view-layout.py` re-runs that check over the captured buffers: 11
      perspective views, 0 failures. Game-tested data, offline analysis.
- [x] Motion vector decode pass, required for every backend rather than only the ones that want
      camera motion: they take a scale factor and Unreal's storage carries a bias. A D3D11 compute
      pass with the encoding as parameters, saving and restoring the compute state around its
      dispatch. Cross-built and tested under Wine on DXVK against values encoded with the engine's
      own constants, including the sentinel and an axis flip. Checked against the game's own
      velocity target too: the raw dump decoded by the reference path and the target this pass
      produced report the same range and the same unwritten fraction.
- [x] View uniform buffer reader in the AC7 plugin: matrices, camera basis, projection, sizes, and
      `ClipToPrevClip` read rather than composed, with `PrevClipToClip` inverted from it. Refuses
      rather than guesses, by checking the relationships that hold in a view buffer and nowhere
      else. Run against the captured buffers it recognises all 50 as view buffers, accepts 10 as
      perspective, and marks the 1016x1016 and 128x93 ones as secondary views. Reads the jitter in
      pixels and hands back the projection with it removed, which 4.18 keeps no copy of.
- [x] `TemporalAAJitter` located at `0x720`, by differencing the captures taken before the
      anti-aliasing gate was patched against those taken after. Confirmed against the two elements
      of `ViewToClip` the engine writes the same values into, and against the pixel offsets
      recorded from a live read.
- [x] Frame assembly in the orchestrator: a plugin fills an engine-neutral camera frame and the
      orchestrator turns it into a backend's structure, refusing a pairing that cannot work rather
      than producing an image that is quietly wrong. Unit-tested only, no GPU involved.
- [x] DLSS on live frames, game-tested. 2176 passes recognised at 1024x576 in one session, 2175
      evaluated, none refused. The result is the scene, reconstructed: at 1:1 against its own input,
      aircraft stencil text is legible where the source is pixelated and panel lines resolve where
      the source stair-steps, so this is reconstruction rather than a smooth rescale.
      Confirmed in a mission on 7 September 2026: the render scale held at 1024x576 for the whole
      flight, 7917 frames evaluated with none refused, and the reconstructed frame is the complete
      scene, sky and cloud layer and terrain and aircraft, at 2048x1152. Under flight there is no
      smearing, which is the first real test of the decoded velocity and of `ClipToPrevClip`.
      What is not done: the result is drawn over the game's frame rather than reinserted into its
      pipeline, so it is ungraded and carries no interface. That is the remaining structural piece.
- [ ] Reinsert the result. A debug view exists behind F7 and is game-tested: a full screen draw over
      the back buffer from inside the Present hook, with a rough tonemap so linear scene colour is
      viewable. It is what showed the reconstruction moving, which is the only way ghosting and a
      motion vector's sign can be judged. It is not the real path, which reinserts the reconstructed
      scene before the game's own composite so the grade and the interface survive. That is the
      remaining structural piece and the reason the picture is ungraded and has no HUD.
- [ ] In-game overlay. The egui crate builds as a Windows DLL exporting its five entry points, the
      D3D11 renderer and the window procedure hook compile, and the observer now offers the Present
      callback they need. Nothing loads or draws them yet.
- [ ] Early loader and orchestrator handshake in the actual AC7 process.
- [ ] Game-plugin detection/preparation/lifecycle and bounded diagnostics.
- [ ] Synthetic DX11/DX12 presentation bridge with correct GPU resource lifetimes.
- [ ] XeLL timing and XeSS FG/MFG on the target Claw.
- [ ] AC7 TAA/input capture, frame identity, and scene/HUD boundaries.
- [ ] Native-resolution XeSS evaluation as an intermediate check.
- [ ] True lower-resolution SR with larger-output reinsertion into AC7 post-processing.
- [ ] Combined SR, XeLL, and every supported MFG setting in AC7.
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

The [validation plan](research/validation-plan.md) defines acceptance. Built does not mean injected, recognized does not mean supported, and a higher presentation counter does not establish lower latency or better handheld performance.
