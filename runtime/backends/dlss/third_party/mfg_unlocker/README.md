# RTX 40 MFG implementation

Selected MIT-licensed sources from [dashdogy/MFG-Unlocker](https://github.com/dashdogy/MFG-Unlocker),
revision `2b52823e3769c31cf52d3b368c6963cff8fc5344`. Copyright 2026 Michael Robles.
The adjacent LICENSE applies to these files.

`midpoint_fix.cpp` and its header are adapted to remove Vulkan admission and optional
OutputPull, Prev2Curr and intermediate-scatter experiments. The production temporal
transform, exact source/output SHA-256 checks, CUDA LUID/SM89 admission and retained
descriptor publication are unchanged. No vendor kernels, model weights or runtime
binaries are included. The native gate and protected publication helpers retain
upstream source unchanged. The provider identity, embedded version eligibility,
temporal profile selection, module retention, and wrapper signature checks are
adapted in the orchestrator's `mfg_provider.rs` and `mfg_state.rs`.

ReScaleFrame's `src/mfg_unlock.cpp` applies the verified wrapper patch before feature
creation. ReScaleFrame owns presentation, controls and switching.
