# RTX 40 MFG implementation

Selected MIT-licensed sources from [dashdogy/MFG-Unlocker](https://github.com/dashdogy/MFG-Unlocker),
revision `2b52823e3769c31cf52d3b368c6963cff8fc5344`. Copyright 2026 Michael Robles.
The adjacent LICENSE applies to the adapted code and must ship with it.

The selected sources are adapted in Rust in `runtime/orchestrator/src/`:

| Upstream source | Rust module |
| --- | --- |
| `midpoint_fix.cpp` | `mfg_midpoint.rs` |
| `ngx_mfg_gate.h`, `protected_pointer.h` | `mfg_gate.rs` |
| `dlssg_provider_policy.cpp` | `mfg_provider.rs` |
| `universal_wrapper_profile.h`, wrapper publication in `patcher.cpp` | `mfg_state.rs` |

The midpoint adaptation removes Vulkan admission and the optional OutputPull, Prev2Curr
and intermediate-scatter experiments. The production temporal transform, exact
source/output SHA-256 checks, CUDA LUID/SM89 admission and retained descriptor
publication are unchanged. No vendor kernels, model weights or runtime binaries are
included.

ReScaleFrame's `mfg_state.rs` applies the verified wrapper patch before feature
creation. ReScaleFrame owns presentation, controls and switching.
