# Rust orchestrator and Anomaly migration

Branch `rust/orchestrator-anomaly` starts at `2db07f4`. The first cutover replaces the orchestrator behind its existing C interfaces. The second replaces the Anomaly adapter. Each production path has one implementation during the move.

## What the current build and callers require

| Surface | Evidence in this tree | Migration consequence |
| --- | --- | --- |
| Runtime linkage | `runtime/orchestrator/CMakeLists.txt` builds a core object library, a DLL with additional D3D11 and overlay services, and a separately linked native Vulkan archive. `loader/proxy` and the Minecraft target also link core objects. | Check both linkage forms. A successful DLL import test covers only one. Preserve enabled optional exports for each build configuration. |
| Plugin lifetime | `plugin_session.rs` validates the plugin table, copies host services, counts readers and can return a live handle after failed preparation when retirement is busy. Anomaly bootstrap resolves prepare and start by name. | Preserve result and handle together, hold the module through the last callback, and retry retirement. |
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

## C ABI comparison

The GameSDK and runtime contract headers match `2db07f4` byte for byte. GameSDK ABI 24 is unchanged. After removing comments, the orchestrator and presentation header comparison found only four additional linked-runtime declarations and an increase in overlay option capacity from 24 to 32. The existing function signatures and public record layouts match the baseline.

A case-sensitive MSVC Release export comparison matched all names and ordinals for Runtime (381), AC7 (256), Loader (273), and Minecraft (267) in the Streamline-enabled build tree. The Anomaly DLL matched all three entry points in the staged C++ adapter and has no static Runtime DLL dependency.

The Runtime version entry point now returns the workspace version from
Rust with the existing C signature and static string lifetime. On the
pinned Rust toolchain, the rebuilt Streamline-enabled Runtime returned
`0.1.2` on two direct DLL calls, matching `VERSION`. All four Release
export inventories matched again. Five focused MSVC DLL and embedded
contracts, 333 locked Rust tests, formatting and Clippy passed. The
rebuilt Godot extension also matched all 398 export names and ordinals
and kept its embedded Runtime linkage.

Callers still need to account for documented behavior changes. Frame assembly requires the output record to advertise its capacity and refuses a zero or short capacity before writing. Overlay settings callbacks can read the committed values, and a second write returns `BUSY` while the first is active. Vulkan capture and SR recording require completion proof before readbacks can be published or reused. The [implementation tracker](implementation.md) records the fixture coverage for those changes.

## Verification coverage

| Requirement | Recorded evidence | Still open |
| --- | --- | --- |
| Core ownership | Rust owns plugin sessions, frame assembly, capture, provider admission, latency and pacing, presentation buffers, readbacks, paired physical DXGI swapchain and queue, and shared D3D11 panel state. The OpenGL and FidelityFX Vulkan WSI routes also use Rust ownership. The merged Standard clone exercised native rendering, DLSS SR and a DLSS-G to FSR3 and back provider transition. [Implementation tracker](implementation.md) · [Shared overlay host evidence](research/rust-overlay-host.md#integrated-runtime-and-standard-game-run) | Complete the ownership audit and independent device checks. Fullscreen and D3D11-on-12 overlay wrapping remain open. |
| Stable C ABI and linked consumers | The GameSDK and contract headers match the baseline. The merged overlay host build kept all eight checked PE export inventories and the Anomaly DLL's by-name Runtime lookup. The earlier Vulkan Streamline-disabled MSVC build kept all four checked inventories and passed ten CPU and ABI contracts. [Implementation tracker](implementation.md) | Recheck the Streamline-disabled variant and final artifacts after the remaining work. |
| C++ Anomaly baseline | Standard and AVX ran the existing C++ adapter with the Rust Runtime. Standard exercised the overlay, held-key gameplay, SR switching and DLSS-G. Both shut down through the engine. [C++ adapter baseline](research/rust-anomaly-cpp-baseline.md) | The baseline covers the exact recorded executable and payload hashes. |
| Rust Anomaly game acceptance | Earlier builds exercised gameplay mouse input, overlay, target resize, DLSS-G and scripted exit on both profiles, plus refused-SR spatial recovery in Standard. Both profiles reloaded the merged Rust manifest in the foreground. AVX also saved both directions of a Quality and Balanced switch. Standard verified resize and provider transitions using the merged overlay host. [Rust adapter game evidence](research/rust-anomaly-sr-pass.md) · [AVX foreground reload](research/rust-anomaly-shader-manifest.md#avx-foreground-settings-and-pack-reload) · [Standard overlay host](research/rust-overlay-host.md#integrated-runtime-and-standard-game-run) | Verify held-key gameplay delivery and repeat AVX with the merged overlay host. |
| Device and shader evidence | Offscreen SR and sharpening runs, exact Vulkan capture readbacks and live Anomaly results are recorded separately from synthetic contracts. [SR device results](research/rust-anomaly-sr-pass.md#offscreen-device-acceptance) · [Vulkan capture results](research/rust-vulkan-readback-owner.md#offscreen-device-acceptance) · [Vulkan sharpening results](research/rust-vulkan-sharpening-cutover.md#offscreen-device-acceptance) | The OpenGL sharing and D3D12 readback device fixtures and a windowed Vulkan DLSS-G capture remain unverified. |
