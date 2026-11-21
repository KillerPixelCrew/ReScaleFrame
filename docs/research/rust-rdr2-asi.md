# RDR2 ASI startup and engine settings in Rust

This note covers the second RDR2 Rust slice. `ReScaleFrame.RDR2.asi` (`rsf_asi_rdr2`) now links
the `rsf-rdr2` static library for its loader-lock startup and engine-settings ownership. It is a
source port. Nothing was built, compiled, linked or tested for it, and no game, device or window
run was made. The first slice, the game DLL, is described in
[rust-rdr2-plugin.md](rust-rdr2-plugin.md).

## Why this slice

The ASI's `DllMain` decision tree, ForceD3D12 and `engine_settings.cpp` form one linked unit: the
settings hooks are installed only after ForceD3D12 has validated the exact decrypted image, the D3D12
trace and every research probe are gated on the same result, and all of them must be armed under
the loader lock before the engine reads its settings. The runtime session worker is a separate
unit that runs later on its own thread and has the same shape as the attach helper, so it stays
C++ for the next slice together with `attach_entry.cpp`.

## What moved

| Former C++ | Rust | Owns |
| --- | --- | --- |
| `asi_entry.cpp` `DllMain` | `asi.rs` | `RDR2.exe` admission (ASCII case-insensitive leaf name), the `ReScaleFrame.ini` path beside the ASI, the `[RDR2]` keys and defaults read under the loader lock (`Enable`, `ForceD3D12`, `SRQuality`, `NativeCapture`, `CameraProbeDirectory`, `FrameProbe`, `FrameProbeLibrary`, `FrameProbePrefix`, `Census`, `CensusDirectory`, `CensusFlags`, `TraceD3D12`, `CameraProbe`, `CameraReference`, `CameraReferenceLibrary`), the order in which owners are armed, the six startup result messages |
| `asi_entry.cpp` `force_d3d12` | `asi.rs`, `layout.rs` | PE identity (`0x65eec92e`, `0x7397600`), the `.data` check for parameter `+57abf10`, the four `mov r64,[rip+disp32]` read sites, the compare-exchange of the static `"d3d12"` string |
| `asi_entry.cpp` `install_d3d12_result_trace` | `asi.rs`, `layout.rs` | the ForceD3D12 gate and the `+268b5c0` helper bytes |
| `engine_settings.cpp`, `engine_settings_policy.h` | `engine_settings.rs`, `layout.rs` | the seven settings hooks (`+20b7f8`, `+2c7c9a4`, `+2c7e6c0`, `+26f5270`, `+26ecc3c`, `+58a908`, `+2c7e8f4`) and three engine function guards (`+2cf31f8`, `+2c76114`, `+2c736f4`), the collection layout and versions 0/37, carrier pinning per vendor and quality, DLSS capability-list choice, refresh-rate storage, the pending-owner pin, the state 9 reconfiguration request, rollback, counters and every log message |

`asi_entry.cpp` lends the crate a separate 27-entry table (`asi_platform.h`, version 1). It does not
reuse the game DLL's `Platform` table because the ASI has its own MinHook instance, shared with the
census and the diagnostic owners, and needs queued enable. The leaves are typed: module paths,
`GetPrivateProfileIntW`/`StringW`, `ReadProcessMemory`, thread id, raw MinHook status calls and
status names, DXGI adapter vendor by index, desktop refresh, three calls into the verified settings
manager, owner and lookup functions, and the native owners that stay C++ (D3D12 trace detour with
its stack capture, native capture, frame probe, census, camera probe and reference, Social Club fix,
worker start). Both sides assert the size and four offsets; a one-off source check compared the
27 field names in order and the initializer count.

`engine_settings_policy.h` now declares the crate's policy C ABI (`rsf_rdr2_dlss_choice_abi`,
`rsf_rdr2_pin_engine_settings_abi`, `rsf_rdr2_storable_refresh_abi`) with inline wrappers of the
former signatures, so `tests/rdr2_engine_settings.cpp` is unchanged; its CMake target links
`rsf_rdr2_rust`, like the camera fixture. `engine_settings.h` declares the crate's
`rsf_rdr2_report_engine_settings` and `rsf_rdr2_engine_quality_tick`, which keep their names, so
`script_input.cpp` is unchanged. The Rust tests restate the fixture and cover admission, the
configuration path, the read-site displacement check and table refusal.

## Preserved behaviour

The ASI file name, output directory, `DllMain` timing and the build-order dependency on the
runtime and game DLL are unchanged. Only `RDR2.exe` arms anything or starts the worker. Every key,
default and gate is the same: no owner is armed unless `Enable` is set (the worker still starts
and checks it again), the settings hooks, native
capture, camera probe and Social Club fix require an applied ForceD3D12, and the trace refuses with
the same message without it. Owners are armed in the former order: ForceD3D12 and settings, native
capture, frame probe, census, trace, camera probe and reference, Social Club fix, then the worker.
The worker logs the same six result lines in the same order and still owns runtime loading,
fingerprinting, `rsf_render_host_create`, the native settings probe, plugin prepare/start,
ScriptHook input and the engine-settings report. The settings hooks are all created, queued and
applied together or removed together; the carrier, refresh and quality-request messages and their
16/32 limits are unchanged. No game patch was broadened and no deployment changed.

## Deliberate differences

- Hook entry bytes, engine function guards, PE headers, the section table and the read sites are
  copied through `ReadProcessMemory` before comparison instead of read in place, as in the game
  DLL slice. Values are the same; an unmapped page refuses instead of faulting.
- A PE header that cannot be read reports `no main image` instead of faulting.
- `TraceD3D12` is read once and reused for the Social Club trace flag instead of read twice.
- The trace detour receives the image size from the crate instead of a C++ constant.
- A configuration path longer than 32768 units disables startup instead of reaching `wcscpy_s`.
- The quality-request log prints the manager as 16 uppercase hex digits, matching MSVC `%p`.

## Remaining RDR2 scope

- Runtime session worker in `asi_entry.cpp` (paths, `Enable`/`DumpModule`/`DumpDirectory`/
  `NativeSettingsProbe` reads, runtime load, SHA-256 probe, render host, prepare/start, logging)
  and `attach_entry.cpp` (explicit opt-in, borrows the already-loaded runtime). They share
  `runtime_setup.h`, whose game policy (window class `sgaWindow`, SR backend choices, engine
  version, application and project IDs, depth and UI mode) is also used by
  `tests/unity_sr_d3d12.cpp`.
- `script_input.cpp` (ScriptHook natives, overlay toggle chord and input guard).
- `social_club.cpp`, `camera_probe.cpp`, `native_settings.cpp` and `native_capture.cpp` (also built
  into `tests/rdr2_capture.cpp`), plus the diagnostics thread and frame-request poller.

## Checks performed

Source review against the removed C++; `cargo fmt -p rsf-rdr2` and `--check` with rustfmt 1.11.0
from Rust 1.100 (the pinned 1.94.1 toolchain is not installed here); `git diff --check`;
`cargo metadata --locked --offline` with `Cargo.lock` unchanged; a Python source comparison of the
C and Rust ASI table order. MSVC, Cargo build, Rust tests, Clippy, CTest and the RDR2 fixtures are
user-owned verification for this change.

## Capture readback leases

`capture_state/gpu_leases.rs` holds the readback buffer, its submitted
queue until a successful covering signal, and the exact completion fence.
The Rust queue table retains its queue and fence through pending reads,
so a refused fence or a recycled queue address cannot tag another object's
work as complete. A failed signal consumes that fence value and leaves the
receipt eligible for a later signal on the same queue. The value range
excludes the device-removal sentinel. When the pending list empties, native
code detaches the idle owners under its capture mutex and drops them outside
it. Ready readbacks keep their own buffer and fence for mapping. A later
capture may start a new fence at value one. Artifact fields and the original
submission identity remain unchanged. Typed D3D12 effects stay native.
Source and layout comparison passed, with build and both GPU fixture runs
still open.

The same GPU owner now publishes the capture swapchain and one to four
backbuffers together. Failure to acquire any buffer discards the partial
batch. Its selected colour target is committed only after the copy is
recorded. The native Present check supplies the exact swapchain and observed
current buffer. It can end the draw phase only for the selected buffer on
the admitted chain. Retirement detaches the buffer, chain and queue owners
after the pending reads empty. The command-list creation hook now passes
its actual output list to the generation reset, correcting an undefined
identifier in the prior source. Capture receipts and artifacts keep their
existing fields. Source and layout checks passed. Build, resize and GPU
fixture verification remain open.

The native capture probe lends MinHook creation and activation, typed
trampolines and the same 16 D3D12 and DXGI detours to
`capture_state/hooks.rs`. The owner publishes each original before
enabling its hook and retains it for the pinned probe's lifetime. A
failed enable used to suppress all later attempts because the original
pointer was already non-null. It now allows a retry for the same target
and detour only. Source comparison matched every role, vtable slot,
callback and forwarded argument. The new refusal case and GPU fixtures
remain unrun. Native resource observations still supply capture metadata.
