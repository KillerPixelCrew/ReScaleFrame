# Rust orchestrator and Anomaly migration

Branch `rust/orchestrator-anomaly` starts at `2db07f4`. The first cutover replaces the orchestrator behind its existing C interfaces. The second replaces the Anomaly adapter. Each production path has one implementation during the move.

## What the current build and callers require

| Surface | Evidence in this tree | Migration consequence |
| --- | --- | --- |
| Runtime linkage | `runtime/orchestrator/CMakeLists.txt` builds a core object library, a DLL with additional D3D11 and overlay services, and a separately linked native Vulkan archive. `loader/proxy` and the Minecraft target also link core objects. | Check both linkage forms. A successful DLL import test covers only one. Preserve enabled optional exports for each build configuration. |
| Plugin lifetime | `plugin_session.cpp` validates the plugin table, copies host services, counts readers and can return a live handle after failed preparation when retirement is busy. Anomaly bootstrap resolves prepare and start by name. | Preserve result and handle together, hold the module through the last callback, and retry retirement. |
| Input and output | The game SDK describes frame identity, camera, CPU events, passes and resource leases. The Anomaly SR driver supplies scene inputs and consumes an output texture. FG obtains presentation and tagging services separately. | Keep source frame, view, format, extent, ownership and retirement visible at each call. Preserve separate SR output and final presentation ownership. Add an ABI field only for a demonstrated missing fact. |
| Additional Anomaly imports | The SR and motion stages share `frame_dump.rs` to borrow the carrier-loaded Runtime and resolve its capture exports. The SR driver and live overlay use the colour-transport setter. The shader and overlay owners query patching, diagnostic and named texture services. | Check direct lookups as well as table entries. Keep optional-service behavior and the adapter's single Runtime instance. |
| GPU and vendor work | Sessions, provider selection, pacing, presentation, diagnostics and overlay live across core and linked libraries. Native D3D11, D3D12, GL and Vulkan have distinct submission and retirement paths. | Port policy once. Keep foreign SDK calls at their existing owner and retain API-specific synchronization. Anomaly exercises only part of this surface. |
| Game-owned work | Anomaly builds its engine hooks, input handling, shader interception, motion history, SR reinsertion and FG input preparation in the game DLL. Its motion patch pack is built separately. | Retain verified build profiles, live settings, shader pack layout, failure paths and packaging when changing implementation language. |

## Order of work and acceptance

1. Establish the native baseline from the branch revision. Record runtime exports and direct/embedded consumers for matching features. Run the existing focused CPU and C ABI tests. Fix discovered correctness defects at their owner and carry the regression coverage across the cutover.
2. Introduce one workspace crate for core code. Start with SR backend selection, whose two scalar C exports already have a native caller test. Link the same implementation into both forms, retire the corresponding C++ implementation and verify symbol names and behavior. Port plugin sessions and frame sequencing next. Exercise short structs, version refusal, busy shutdown and failed startup.
3. Move settings, diagnostics, overlay services, rendering sessions, latency, graphics and presentation ownership. Keep the published headers authoritative. Inventory each service's callers and failure paths before switching it. Compare exports against the baseline for each enabled backend and exercise existing consumers on their graphics APIs.
4. Run the current C++ Anomaly adapter with the new runtime. Check standard and AVX profiles, overlay changes, quality and backend switches, camera movement, moving geometry, UI, FG, resize, fallback and teardown against recorded acceptance. Capture the evaluation frame and resource identity for rendering comparisons. Recheck other linked and DLL consumers before retiring the old core.
5. Port Anomaly behind its existing game DLL entry and package it with the same runtime. Repeat the fixture, device and game checks on the Rust adapter. Record known pre-existing gaps separately from regressions. Other games retain their current adapters.

The [Anomaly release record](releases/v0.1.2-anomaly.md) distinguishes the shipped payload from later shader-provider work. It supplies historical acceptance only. Record validation of this branch in the [implementation tracker](implementation.md).
