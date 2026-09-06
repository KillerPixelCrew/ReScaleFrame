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
- [ ] Jitter sequence length set in the game alongside the render scale. Written and cross-built,
      refused rather than guessed when the object does not hold the engine default. Not yet run in
      the game.
- [ ] DLSS through Streamline: load, device handover, support query, render size planning, resource
      tags and per frame constants, behind the C ABI in `runtime/backends/dlss`. Cross-built
      against the real SDK headers with warnings as errors and contract-tested. Runs under Wine in
      the game's Proton prefix: DLSS reports supported on the RTX 4070 Laptop and returns 1024x576
      for a 2048x1152 output at performance quality. Initialisation and queries only, no upscaled
      pixel yet, and nothing built with MSVC.
- [x] View uniform buffer mapped, including `ClipToPrevClip`, the camera basis and the projection.
      Offsets are checked by an identity that ties five of them together rather than fitted to one
      buffer, and `tools/verify-view-layout.py` re-runs that check over the captured buffers: 11
      perspective views, 0 failures. Game-tested data, offline analysis.
- [x] Motion vector decode pass, required for every backend rather than only the ones that want
      camera motion: they take a scale factor and Unreal's storage carries a bias. A D3D11 compute
      pass with the encoding as parameters, saving and restoring the compute state around its
      dispatch. Cross-built and tested under Wine on DXVK against values encoded with the engine's
      own constants, including the sentinel and an axis flip. Wired into the F10 dump so it can be
      checked against the game's buffer, which has not been done yet.
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
      Two things it does not show. The scene was a hangar with a nearly static camera, so the motion
      vectors were near zero throughout and remain unverified by this. And the colour handed over is
      the lighting pass output rather than the pre-tonemap image the full resolution captures
      described, which is a question of insertion point rather than of the backend.
- [ ] Reinsert the result. Evaluating is not the same as being visible, and the game still presents
      its own upscale.
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

The [validation plan](research/validation-plan.md) defines acceptance. Built does not mean injected, recognized does not mean supported, and a higher presentation counter does not establish lower latency or better handheld performance.
