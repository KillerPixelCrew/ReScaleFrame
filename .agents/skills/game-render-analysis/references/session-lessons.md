# AC7 session lessons, 30 September to 2 October 2026

These are AC7 findings, not universal engine or GPU rules. The parent transcript and current
source were reviewed before this reference was written. On 2 October the user accepted clean
wing/cloud rendering, then accepted the optimized SR bridge and FSR4 compatibility deployment.
No new numerical in-game FPS comparison accompanied the final acceptance.

## Start from the current owners

| Question | Owner and evidence |
| --- | --- |
| Native view size, UI, graph, queued lifetime | `games/ac7/src/native_renderer.cpp`, `render_scope.cpp`; [native refactor](../../../../docs/research/ac7-native-renderer-refactor-20261001.md) |
| Per-view/runtime coordination | `runtime/orchestrator/src/native_sr.cpp`, `native_regions.cpp`, `plugin_session.cpp`; copied C ABI packets in `sdk/game/include/rescaleframe/game_renderer.h` |
| Sparse velocity validity and camera motion | `runtime/graphics/src/motion_resolve.cpp`; [motion research](../../../../docs/research/ac7-motion-vectors.md) and [stability](../../../../docs/research/ac7-stability-20261001.md) |
| Wing/cloud masking, cloud resolution | Game-owned TrueSky hooks and `games/ac7/src/truesky_depth.cpp`; [aircraft/cloud research](../../../../docs/research/ac7-plane-artifacts-20261002.md) |
| Colour drift and producer precision | Native scene-format policy plus `runtime/graphics/src/colour_fidelity.cpp`; [colour research](../../../../docs/research/ac7-ui-hdr-20260930.md) |
| FSR/XeSS transfer, FSR4 capability | `runtime/orchestrator/src/sr_bridge.cpp`, `runtime/backends/fsr/src/fsr4_compat.cpp`; [interop research](../../../../docs/research/sr-interop-performance-20261002.md) |

## Failed assumptions that changed the design

| Observed failure | Correction that should guide the next investigation |
| --- | --- |
| Missing lighting/reflections after a seemingly valid texture match | A pooled tonemap allocation served as GBuffer earlier. Identify graph role and phase before substitution. |
| Sharp UI briefly after pause, then blurred or wrong-sized menus | Final blits do not fix low-resolution UI production. Own physical widget/target allocation while preserving logical layout and native glow/composition. |
| Ultra rejected or looked unupscaled | Engine-aligned allocation and active backend input rectangle differ. Pass exact supported active dimensions and scope downstream view/uniform rebuild. |
| Hangar crashes at a repeatable camera point | Moving a shared uniform to save it briefly published null to recording workers. Copy/AddRef, publish from a local source, wait for native CPU recorders and retain generated buffers through queued work. |
| Sky up/down jitter while terrain behaves differently | Correct TrueSky's matched-view projection handedness and inspect terrain HS/DS. A blanket jitter sign or zero-jitter override damages other producers. |
| Jittering bloom despite stable scene | Bloom/exposure branches still read the reduced jittered source. Route the matched `SceneColorHalfRes` branch to reconstruction and prevent an exposure dependency cycle. |
| DLSS Perf/Ultra bands despite SDR output | Linear scene colour still exceeds display range. Restore native FP16 production and the measured GPU colour correction; proprietary exposure internals remain unproven. |
| Wing gaps remain stationary across all upscalers | Examine cloud depth precision and coverage before attributing them to velocity, reflection or geometry. Raw depth bounds were coarse UNORM. |
| Clouds disappear after choosing divisor 1 | Shipped mixed-resolution effect has only x2/x3/x4 passes. Divisor changes need an x1 producer and balanced native cleanup. |
| Replacement CS compiles but defects remain | Actual UAV was Texture2D, while the activation guard expected an array. Log verified bindings and actual dispatch, not compilation alone. |
| Distant clouds blocky below Balanced | Preserve native coordinated cloud/history production at scene resolution; no post-SR cloud recomposition was shipped. |
| XeSS/FSR slower than DLSS | User corrected the direction: DLSS was faster. Successful numeric output proved the alternatives ran. Remove CPU transfer stalls; do not infer identical GPU cost. |
| Default FSR4 refuses on RTX | Match the SDK capability predicate and prove INT8 provider/model execution. An internal FSR3 fallback is not FSR4. |
| SDK headers exist but backend load fails | Deploy runtime DLLs and notices beside the game. Header availability does not supply runtime binaries. |

## Accepted and unfinished work

Accepted user-run results include full-resolution menus/HUD/briefing, restored lighting and
reflections, the hangar crash correction, stabilized background/clouds, corrected wing/cloud
production and the final SR compatibility deployment. Retain those owners when changing one
backend or capture route.

F9 already dumps vectors, but engine decision records alone do not identify a particular
rejected missile or vehicle. Carrier launch, refuelling, moving ground vehicles and cloud
history remain targets for independent motion coverage. A shader sidecar only improves draws
that actually reach the velocity pass. Do not advertise full coverage from a compiled probe.

FG/Reflex, HUD-less FG surfaces and latency markers are not established by SR or copied
Tick/Present identity. The partial DLSS native preset API also does not establish a completed
model-selection UI. Keep these boundaries explicit in reports and skills.

Game images, decrypted dumps, shader blobs, replay experiments and references stay untracked.
Research retains permitted fingerprints, equations, runtime sites and uncertainty.
