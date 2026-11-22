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

At the initial port, the environment-only diagnostic options finding remained open. The shared overlay
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

At the SDK record follow-up, the device fixtures, SDK diagnostic registration
and shared editable diagnostic settings still needed work. The saved Unicode
settings landed in `6814dd3e` before the fixture follow-up below.
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

The SR fixture runs requirements preflight, SDK session admission, both
quality cases, the 12-frame evaluation and rejection checks, and shutdown
in Rust. It preserves the return-77 decisions, reset and sharpening
sequence, optional exposure failures, diagnostic counter, known-color
readbacks, and device-before-SDK retirement order. A context that cannot
retire keeps its device alive. Native leaves own Vulkan setup, drawing
operations, readback and pixel comparison, and the separately compiled
typed SDK input-range mock.

### Source boundaries and disposition

| PORT path | Disposition at this follow-up |
| --- | --- |
| `native_fg_xess_vulkan.cpp`, `native_fg_xess_vulkan.h`, `native_fg_xess_vulkan_platform.h` | Rust maps the V03 results for image and fence access and checked destruction. C++ creates the native COM callback table. The private adapter declarations use C linkage for the fixture driver. |
| `native_fg_xess_vulkan_present.cpp` | Retained native binding seam. It selects the existing V02 graphics table and marshals both Present variants through V02. Its entry points are also the substitution seam for the synthetic provider-dispatch fixture. |
| `native_xess_vulkan.cpp`, `native_xess_vulkan.h`, `native_xess_vulkan_requirements.h` | Retained typed platform table, direct SDK calls, requirements ABI and SDK-disabled seam. The integrated base already uses the Rust loader/export, record, transfer and frame owner. The fixture's typed SDK mock still exercises this platform table. |
| `bridge.cpp`, `diagnostic-capture.h` | Rust compares the exact diagnosed module SHA-256 and resolves the private setter on that same module, in that order. Native leaves read the loaded module digest, bind the typed setter and register or unregister the original typed callbacks. Other native leaves still issue DXGI, D3D12 and direct SDK calls. |
| `bridge_api.h`, `capture_api.h`, `session_abi.h` | Retained ABI contracts. Public bridge ABI 1 and all DLL export names stay fixed. The private platform stays 552 bytes and moves to version 8 for its digest and setter-binding slots. |
| `tests/native_fg_xess_vulkan_interop.cpp` | Driver ported. C++ provides the UTF-16 CRT entry point. |
| `tests/native_fg_xess_vulkan_present.cpp` | Driver ported. C++ provides Win32, Vulkan allocation and drawing leaves. |
| `tests/native_xess_vulkan.cpp` | Driver ported. C++ provides Vulkan commands and pixel verification and retains the typed CPU input-range mock. |

The saved Unicode overlay integration at `6814dd3e` supplies the existing
diagnostic profile to both bridge builds. The fingerprint still gates the
private diagnostic export, and queue-completed capture publication remains
in the same owner. The public export list and regular SDK lookup order
compare equal to `9716068b`. The V02 presenter and V03 importer source,
shared synchronization contracts and V05 fixture paths remain outside this
change.

The source checks used Rust 1.94.1 formatting, bindgen 0.72.1 regeneration
for the V06 ABI and pinned XeSS SDK records, locked offline Windows x64
Cargo metadata, and whitespace and source ordering comparisons. Full
workspace binding generation with IDA's libclang hit a bindgen panic in the
V05 fixture header. Scoped V06 regeneration succeeded. Device, game, build,
test and Clippy verification were outside the requested scope.

The interop executable now uses a C11 UTF-16 CRT launcher. The existing C++
Present fixture source keeps the target linker and SDK calls. Strict C11
syntax and all-feature locked Windows metadata passed. Execution remains open.

## Graphics leaves from `3066e6ad`

The XeSS Present fixture now uses the already-loaded ash device to record,
submit and retire Vulkan resources. The C++ fixture source handles the HWND
and message pump. The source-frame semaphore wait still targets TRANSFER,
signals the same binary semaphore, and reports the command only after queue
submission. The R32F and RG16F guide values, the moving rectangle and the
three swapchain image views retain their formats, extents and barriers.
A failed drain retains the original owner and the heap-stable queue
callback user. The views retire before the presenter can destroy its
images. All public bridge and WSI names stay in place.

The integrated DX12 bridge uses the Runtime's typed capture implementation
for resource descriptions, copyable footprints, readback allocation, COM
release, copy, map and unmap. Readback allocation, transition and unmap reuse
the existing helpers. Present description, transition and color copy use
the same typed interfaces and record on the bridge's original command
list. The session still gates publication on its own queue completion, and
the private platform table layout and SDK callback ABI stay fixed. The
standalone bridge continues to link the Rust session directly with its
native DX12 call leaves. Both bridge configurations passed MinGW syntax
checking with the pre-existing FARPROC cast warning left nonfatal.

The SR device fixture now uses ash for the resolved loader and Vulkan
operations. It still chooses the first discrete adapter when present,
requires a graphics-and-compute queue, queries the Vulkan 1.1/1.2 feature
chain in the same order and lets XeSS amend it before device creation.
The two quality cases retain their formats, image barriers, queue and fence
wait, coherently mapped readback, and RGB-only tolerance over the same
pixel region. Its binary16 decoder treats nonfinite values as failures.
The independent C++ typed SDK mock stays byte-for-byte identical, and the
SDK-disabled launcher retains its skip route.

The SR adapter now owns the Vulkan image dispatch, memory-type metadata,
barrier and copy functions in its Rust graphics state. It binds the same
commands in the same order and initializes the same shared capture owner
with the device's reported memory types. The C++ capsule retains the
typed SDK table and opaque Rust handle. Its mock still supplies the
original inert SDK and Vulkan proc callbacks.

Both bridge builds use the same Rust shared-image format plan for the six
original Vulkan formats. The integrated bridge creates and exports the
D3D12 resource through the Rust DX12 bindings. The standalone bridge uses
the plan with its direct COM leaf. Transfer of the NT handle and COM
reference happens after successful export. V03 still imports them and
owns the original timeline and retirement contract.

The FG and SR fixture drivers, integrated capture and Present recording,
and SR Vulkan binding are ported at source level. Native boundaries remain
for the Win32 window callbacks, the independent typed SDK mock, the
conditional V02 entry-point table, SDK function binding, and the standalone
bridge's COM calls. The integrated bridge also retains its native DXGI and
D3D12 creation, queue and GPU timestamp context. V04's cross-provider
public naming is independent. This change preserves the existing private
XeSS adapter entries and does not take any public rename.

Checks used Rust 1.94.1 formatting, bindgen 0.72.1 for the XeSS records and
pinned vendor bindings, locked offline Windows x64 metadata, and MinGW
C++ syntax for both bridge modes, the SR adapter and CPU mock, the SR
launcher modes, and the Present window leaf. Source comparison checked
the export lists, typed mock, V02/V03 owners and fixture queue order.
Build, test and device results remain pending.

## Integrated context and retirement from `f4871e47`

The integrated bridge now creates the DXGI factory, selects the adapter
by LUID, creates the D3D12 device and direct queue, builds the swapchain
descriptor, and creates its recorders and fences through typed Rust calls.
The bridge exports shared fence handles through the same device. The Rust
calls also cover completion polling and arming, queue waits and signals,
backbuffer acquisition, command submission and DXGI Present. The DXGI
Present HRESULT is passed through verbatim. The C++ SDK seam keeps Intel
context creation, configuration, markers and the original swapchain
initialization call and swapchain interface request.

The existing session remains the sole retirement authority. Release uses
the existing Rust COM reference helper after session retirement permits it.
The session still retains the native owner on failed retirement and unloads
the vendor modules after the native resources are gone. Vulkan adapter
matching in the integrated route uses the same already-loaded Vulkan
module and resolves the same typed entry point.

Integrated timestamp state now owns its query heap, readback, post-list
and allocator slots in Rust. It uses three timestamps per slot, the
presenting queue, the original QPC entry and exit samples, and the same
clock calibration. It reads the slot only after the existing allocator
completion wait. The post-list is submitted before the two existing
queue signals and marks the sample submitted only when its list closes
and queues successfully. Failed timestamp initialization drops its partial
objects and retries on a later frame. The standalone bridge keeps the
existing C++ DXGI, D3D12, and timestamp leaves. Both builds retain the
native SDK binding, clock and logging entry points.

The two bridge configurations passed MinGW syntax checks. The standalone
route retains the existing Vulkan FARPROC cast warning. Rust 1.94.1
formatting, bindgen 0.72.1 regeneration of the local and vendor XeSS
records, and locked offline Windows metadata passed. The export lists,
SDK marker names, session retirement order, and bridge ABI were checked
against the base.

The Present fixture now owns its HWND and Win32 message pump in Rust. The
original class, title, geometry and activation calls are preserved, and
failed graphics retirement retains the window along with the queue user.
The FG and SR device fixture entry points are C11 CRT launchers. The SR
launcher retains the SDK-disabled skip decision. The FG launcher uses
MinGW's Unicode CRT option to select its existing wide entry point.
The independent C++ typed SR SDK mock remains byte-for-byte the same
within its conditional. Synthetic provider coverage remains separate.
Strict C11 launcher syntax and C++ mock syntax passed. Build and device
execution remain pending.
