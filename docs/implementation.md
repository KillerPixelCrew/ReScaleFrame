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
      against the real SDK headers with warnings as errors and contract-tested. Under Wine in the
      game's Proton prefix it reaches NGX, loads `nvngx_dlss.dll` 310.7.0 and loads the DLSS plugin
      for this adapter, then stops at `NvAPI_D3D11_CreateCubinComputeShaderExV2`, which DXVK-NVAPI
      does not implement. No upscaled pixel yet.
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
