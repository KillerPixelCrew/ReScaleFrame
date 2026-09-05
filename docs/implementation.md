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
