# XeSS Vulkan bridge source port

V06 starts at `8adf010b` and follows the existing XeSS FG owner audit in
[rust-vulkan-xess-fg-cutover.md](rust-vulkan-xess-fg-cutover.md). The integrated
route still records Vulkan copies, hands shared images to the DX12 presenter
and sends the engine's frame identity through the bridge. The standalone route
links the same Rust session directly from `runtime/vulkan/xess-fg/build.ps1`.
It still has a separate legacy Present input that carries its own UI texture.

| Finding | Source evidence |
| --- | --- |
| Share the public XeSS FG Present record and status between Rust producer and consumer. | `runtime/vulkan/xess-fg/bridge_abi.rs` generates create, start, Present and status from `bridge_api.h`. Both Rust consumers use those records. The legacy input conversion stays in `session.rs`. |
| Use the generated GameSDK frame layout for XeSS diagnostic capture. | `sdk/game/src/frame_abi.rs` generates frame and camera types from `game_frame.h`. The GameSDK re-exports them and the standalone capture owner includes the same generated file. `capture.rs` copies the accepted record including its camera and provenance fields. Padding follows the C layout and publication uses the named fields. |
| Use one timeline-pair admission rule before queuing a failed bridge handoff. | `runtime/vulkan/timeline_pair.rs` requires a positive ready value and done greater than ready and the last signalled done. XeSS uses it in `session.rs`, FSR4 SR in `runtime/vulkan/fsr4/owner.rs` and FSR4 FG in `runtime/orchestrator/src/fsr4_fg_bridge.rs`. Each bridge retains its queue and return-code rules. The integrated XeSS Present entry still requires ready to follow the last done. |
| Attach XeSS timing samples and denominators to their presenting session. | `session.rs` owns the accumulator through checked retirement. `bridge.cpp` borrows it for XeLL CPU sampling and calibrated GPU samples harvested after the slot's completion. `timing.rs` uses accepted sample counts per phase and per GPU interval. Reports still flush every 120 successful Presents. |
| Read each XeSS capture setting independently. | `capture.rs` reads each UTF-16 setting into a separate bounded value. Empty and overlong values leave that setting at its default. Numeric parsing, the capture count clamp and the exact probe value are retained. |
| Describe an aliased HUD-less fallback texture once. | `session.rs` validates the color description and only queries HUD-less when it is a distinct pointer. Color and HUD-less retain distinct SDK resource roles. |

`bridge.cpp` now forwards integrated create/start admission and legacy and
integrated interpolation count decisions to the session. The session checks
the size word before reading the version or payload. The integrated start
rejects a zero or unsupported count. The standalone setter retains its
clamping and output values, and the session updates its active count only
after SDK acceptance. The CPU preflight in
`tests/native_fg_xess_vulkan_interop.cpp` adds a size-word-only create case.

Rust writes the callback log, capture errors, raw files and JSON receipts.
The SDK callback ABI, provider fingerprint, DX12 footprint, copy, map and
unmap operations stay behind the native leaves. The raw writer keeps the
recorded offset, row pitch and byte length and writes zeros into footprint
padding. It only reads copied texels after the bridge has drained the exact
DX12 queue. Receipt field names, identity values and units remain compatible.
The private platform ABI is 6. The exported bridge ABI and its records remain
at version 1.

The existing integrated FG files under `runtime/orchestrator/src/native_fg_xess_vulkan*`
marshal Vulkan WSI, typed bridge callbacks and D3D12 importer handles into the
existing Rust owner. They retain the shared V03 importer. The XeSS SR
requirements and SR fixture are covered by the V01 owner and the
[XeSS SR cutover](rust-xess-vulkan-sr-cutover.md). The presentation device
fixture keeps its native Vulkan and HWND setup.

The environment-only diagnostic options finding remains open. The shared overlay
only carries numeric and toggle options. Directory and timing-file controls
need shared path settings and an overlay service route. The XeSS configuration
also registers the fingerprint-gated SDK callbacks at presenter startup, so live
configuration must coordinate registration or retirement with V06's session and
G07's shared capture owners. The D3D12 capture and AC7 diagnostic owners are part
of the same finding.

Verification was source-only. Bindgen CLI 0.72.1 regenerated the touched
Windows x64 bindings, Rust 1.94.1 rustfmt checked the changed Rust sources and
`git diff --check` passed. Builds, tests, lint and device execution are user-owned.
