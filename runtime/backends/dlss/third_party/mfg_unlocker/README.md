# RTX 40 MFG implementation

Selected MIT-licensed sources from [dashdogy/MFG-Unlocker](https://github.com/dashdogy/MFG-Unlocker),
revision `2b52823e3769c31cf52d3b368c6963cff8fc5344`. Copyright 2026 Michael Robles.
The adjacent LICENSE applies to these files.

`midpoint_fix.cpp` and its header are adapted to remove Vulkan admission and optional
OutputPull, Prev2Curr and intermediate-scatter experiments. The production temporal
transform, exact source/output SHA-256 checks, CUDA LUID/SM89 admission and retained
descriptor publication are unchanged. No vendor kernels, model weights or runtime
binaries are included. The other files retain upstream source unchanged.

ReScaleFrame's `src/mfg_unlock.cpp` owns initialization and wrapper patching before
feature creation. ReScaleFrame continues to own presentation, controls and switching.
