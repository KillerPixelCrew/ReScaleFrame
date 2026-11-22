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

| V06 inventory | Disposition |
| --- | --- |
| `runtime/orchestrator/src/native_fg_xess_vulkan.cpp`, `native_fg_xess_vulkan.h`, `native_fg_xess_vulkan_platform.h`, `native_fg_xess_vulkan_present.cpp` in that directory | Existing native seams marshal Vulkan WSI, typed bridge callbacks and D3D12 handles into the Rust FG owner. They use the shared V03 importer. |
| `runtime/orchestrator/include/rescaleframe/native_xess_vulkan_requirements.h`, `runtime/orchestrator/src/native_xess_vulkan.cpp`, `runtime/orchestrator/src/native_xess_vulkan.h`, `tests/native_xess_vulkan.cpp` | Existing public ABI and typed SDK leaves delegate to the Rust requirements owner. Final integration depends on V01. See [the XeSS SR cutover](rust-xess-vulkan-sr-cutover.md). |
| `runtime/vulkan/xess-fg/bridge.cpp`, `bridge_api.h`, `capture_api.h`, `diagnostic-capture.h`, `session_abi.h` in that directory | Moved bridge admission, generated-count decisions and publication out of C++. Public ABI headers, the typed SDK table and native GPU calls remain at the boundary. |
| `tests/native_fg_xess_vulkan_interop.cpp`, `tests/native_fg_xess_vulkan_present.cpp` | Added a size-only prefix check to the interop fixture. Both keep native device setup. The presentation fixture keeps its Win32 window and Vulkan queue and resource setup. |

The environment-only diagnostic options finding remains open. The shared overlay
only carries numeric and toggle options. Directory and timing-file controls
need shared path settings and an overlay service route. The XeSS configuration
also registers the fingerprint-gated SDK callbacks at presenter startup, so live
configuration must coordinate registration or retirement with V06's session and
G07's shared capture owners. The D3D12 capture and AC7 diagnostic owners are part
of the same finding.

Verification was source-only. Bindgen CLI 0.72.1 regenerated the touched
Windows x64 bindings byte for byte. Rust 1.94.1 rustfmt checked the changed Rust
sources, `git diff --check` passed, and the source list of bridge DLL exports
was identical. Builds, tests, lint and device execution are user-owned.

## SDK record and fixture follow-up from `821642ae`

This follow-up moved SDK record construction and export admission into the
existing Rust owners. The comparison used Intel XeSS v3.0.2 headers and
Vulkan SDK 1.4.357.0 headers. The standalone MinGW build and the Runtime
include the same `session.rs`. Its SDK records are the generated D3D12
XeSS-FG and XeLL records used by the regular D3D12 owner.
[Intel XeSS SDK revision](https://github.com/intel/xess/tree/8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0/inc)

| Path | Source port and native boundary |
| --- | --- |
| Vulkan SR requirements and SDK calls | `native_xess_vulkan_requirements.h` generates the setup and ABI constant. `vulkan_sr/xess.rs` now prepares SDK initialization, image-view and execute records and reads the SDK size and version records. `native_xess_vulkan.cpp` retains typed SDK calls, disabled-export routing, Vulkan recording, capture and sharpening leaves. `native_xess_vulkan.h` selects the typed table or its null disabled form. |
| Integrated XeSS FG | `native_fg_xess_vulkan.h` generates the integrated options record. `native_fg_xess_vulkan.cpp` adapts COM image and fence handles to the V03 importer. `native_fg_xess_vulkan_present.cpp` binds the V02 WSI marshaller and graphics table to `vulkan_xess_fg.rs`. `native_fg_xess_vulkan_platform.h` declares the native binding and C linkage. |
| Standalone and integrated FG bridge | `session.rs` resolves five XeLL and thirteen XeSS-FG exports in the original module and order, and rejects an incomplete table before DXGI or SDK startup with the original stage and result. `bridge.cpp` retains wide-path DLL loading, typed slot binding, adapter LUID matching, COM calls, timestamp queries, and SDK callback and logging entry points. The private table moves to ABI 7 and 552 bytes. Bridge ABI 1 and the 216-byte legacy and integrated Present records retain their wire layouts. The legacy record in `bridge_api.h` generates the standalone Rust layout, including the reserved word that may be uninitialized. |
| FG Present and capture | Rust fills SDK init, property, resource, constant, status and sleep records. It uses UINT32_MAX as the default interpolation sentinel and the calling route's UI mode. Tags use the source texture extent, zero base, ONLY_NOW and COPY_SOURCE, in depth/motion/HUD-less/UI order. Camera matrices, jitter, motion scale and reset use the accepted frame, with SDK frame render time zero. `session_records.h` generates the capture and Present descriptions and the module pair. `capture_api.h` declares the GameSDK capture contract. `diagnostic-capture.h` binds the fingerprint-gated callback table, computes the fingerprint and provides typed readback, copy and map calls. `capture.rs` reads resource role and state at the SDK callback and publishes after the DX12 queue drain. |
| Fixtures | Rust FG session tests cover the native interop fixture's short, wrong-version and relative-path create checks and place a size-word-only create record at a protected page. `tests/native_fg_xess_vulkan_interop.cpp` retains its public-export smoke and real shared-image/fence exercise. `tests/native_fg_xess_vulkan_present.cpp` retains the Win32 window and two-queue Present exercise. The Rust SR fixture observes the SDK records and `tests/native_xess_vulkan.cpp` checks colour-aspect mip/layer ranges, zero bases and null optional textures through the typed SDK mock. That C++ fixture retains inert SDK/Vulkan setup and the optional device route. |

The SR view records describe the existing maximum-size guide allocations.
Active dimensions come from the accepted frame record. The baseline called a
missing `texture(maximum)` method on the shared image record. The port reads
the extent from the actual allocation through the local view constructor.
The producer, shaders, copy command order, fence ownership, public exports
and V02/V03 table layouts follow the existing paths.

The native device fixtures, SDK diagnostic registration and shared editable
diagnostic settings still need follow-up. The settings work belongs to O02.
Source checks cover binding regeneration with bindgen 0.72.1, Rust 1.94.1
formatting, locked offline Windows x64 Cargo metadata, whitespace, bridge
export names and required symbol order. Build and runtime validation remain
pending.

## Device fixture follow-up from `9716068b`

The FG interop executable keeps its UTF-16 command line and uses the original
integrated bridge create export and V03 importer. Rust runs the four ABI
preflight checks without loading Vulkan, then on explicit request selects the
first physical device and graphics family, reads the DXGI-compatible LUID,
and requests Vulkan 1.2 with Win32 external memory and semaphore extensions
and timeline support. It imports BGRA8, R32F and RG16F at 640 × 360. Both
queue drains still gate interop and bridge destruction. The JSON keys and
status meanings are preserved. When the optional presenter reports failed
retirement, the caller also retains the Vulkan loader and device.

The optional Present route now sequences the original 12-frame workload in
Rust. It keeps the one Vulkan graphics queue, 10-second acquire timeout,
three pre-submit XeLL markers, post-submit marker and notification, and
shader-read handoff. The C++ leaves own the HWND, Vulkan allocations and
draw commands for the moving rectangle and constant guides. The Rust driver
passes the existing frame and Present records to V02 and issues the same
receipt. An unsuccessful drain retains the views and owner. After drain,
views retire before the presenter releases the underlying images. The
remaining fixture allocations retire only after checked presenter release.
