# Vulkan readback owner Rust port

The baseline is `56338a5`. `rsf::FrameCaptureVk` in
`runtime/backends/common/frame_capture_vk.h` was a header-only C++ owner used by
every first-party Vulkan SR, FG and dump path. This port moves its state machine
into `runtime/orchestrator/src/frame_capture_vk.rs` and leaves the class as a
façade with the same constructors, methods, default arguments and return values.
No consumer source changed. `native_fg_dlss_vulkan_capture.cpp`, `input_dump.rs`,
`input_dump/abi.rs`, `input_dump.h` and `tests/input_dump_vulkan_cpu.cpp` are
untouched.

## Ownership

| Boundary | Owner |
| --- | --- |
| Selection | Rust reads `RSF_VULKAN_FRAME_CAPTURE_DIR`, `RSF_FRAME_CAPTURE_FIRST` and `RSF_FRAME_CAPTURE_COUNT` with the old `GetEnvironmentVariableW`/`wcstoull` limits (32767-character directory, 31-character counts, count capped at 16). It combines the legacy window with `rsf_input_dump_wants_frame` and admits a core image through `rsf_input_dump_wants`/`rsf_input_dump_reserve`. |
| Records | Rust holds each readback: command buffer, handles, size, format, identity, role, reset, legacy flag, core ticket and name, group, group completion and discard state. |
| Completion proof | Shared-submission owners publish a core image only after `rsf_input_dump_gpu_ready` certifies its ticket, or after the owner completes the exact group. A completed group never consults the shared watermark, and the watermark never certifies an incomplete group. Command-retirement owners map a readback only when its `vkCmdSetEvent` is set, either on retirement of its exact command buffer or on a full drain. Ready-only polls never map command-retirement or legacy-only readbacks. |
| Refusal and abandon | `discard_group` fails each ticket once with `STAGING_FAILED`, keeps the allocation until a full drain and ignores later completion. A full drain fails an incomplete group, an unrecorded allocation or an uncertified shared ticket with the old error codes and frees without mapping. A partial allocation stays pending until retirement. |
| Publishing | Rust sends core pixels through `rsf_input_dump_pixels` with the old DXGI encoding and API 3. It writes legacy `.raw`/`.json` files and `capture-errors.log` with the same names, field order and `%.9g` number text. |
| Native leaves | The C++ façade creates and frees the buffer, memory and optional event, records the barriers, copy and `vkCmdSetEvent`, reads event status and maps memory. It also supplies `capture_name`, `capture_view` and `capture_scale`, so the naming rules stay in `capture_names.h`. |

Rust calls the existing public input dump C functions through the façade table.
These are the same calls the C++ owner made, so the input dump keeps a single
admission per image and its existing platform binding. `input_dump_vulkan.rs`
still drives the standalone snapshot submission and reaches this owner only
through `complete_group`, `discard_group` and `flush`. Its exact group proof is
unchanged.

The Rust entry points are internal `rsf_core_capture_vk_*` symbols in the
existing static Rust archive. They are not DLL exports. Every target that
includes the header already linked that archive through
`ReScaleFrame::FrameDump`, `ReScaleFrame::NativeVulkan` or
`rsf_orchestrator_core`.

## Retained behaviour

The owner still holds one lock for record, poll, group changes and flush, and
calls the input dump and Vulkan leaves under it as before. The callers still
drain referencing submissions before a full flush or destruction. Destruction
released pending allocations without failing their outstanding core tickets
before the follow-up, as the C++ owner did. Such a bundle reached the input dump's 30 second
incomplete timeout instead of reporting a readback failure. This was observed in
source review only and was not changed.

After the merge, destruction now fails each remaining core ticket before
releasing its readback. The caller still drains submissions before destroying
the owner. The test checks immediate failure and release without mapping.

## Validation

Seventeen new Rust tests in `frame_capture_vk/tests.rs` drive the owner with
fake Vulkan leaves and a fake input dump. The fake memory holds `0xef` until a
test marks the copy as executed. The tests cover these cases:

- An unrequested frame does no GPU work.
- A shared ticket is not mapped or published until the watermark certifies it.
  A full drain without that proof fails it unmapped.
- An exact group ignores the shared watermark and publishes only the executed
  bytes after `complete_group`.
- A discarded group fails once, ignores later completion and is freed
  unmapped. An incomplete group fails on drain.
- Command retirement maps only the retired buffer with a set event.
- A partial allocation is retained and then refused.
- Refused sources reserve nothing. An oversized source fails as unsupported.
- Core and legacy outputs publish once each. Legacy files appear only after a
  drain, and the error log keeps its format.
- `%.9g` text and `wcstoull` parsing match the C++ behaviour.

These checks ran on MSVC 19.52 with Visual Studio 2026. The worktree-local
build reused the coordinator's MinHook source and SDK headers. The pinned
yyjson files were copied into the ignored vendor directory.

- `cargo fmt --all --check` and `cargo clippy --workspace --all-targets
  --locked -- -D warnings` passed. `cargo test --workspace --locked` passed
  310 tests.
- All Debug and Release targets built without warnings. This covers every
  header consumer except the Godot extension.
- Debug and Release each passed 20 focused contracts:
  `native_vulkan_contract` (the fake-device command-retirement case),
  `input_dump_vulkan_cpu_linked` and `input_dump_vulkan_cpu_dll` (poisoned and
  stale shared proof, refusal, timeout and drain), `native_vulkan_dll_exports`,
  the DLSS, DLSS FG, XeSS, FSR4 SR, FFX reset and submission, FG dispatch
  (three variants), DX12 interop and split SR (contract, disabled and stub)
  contracts, `capture_metadata_contract`, `failure_capture_policy_contract`
  and `input_dump_d3d12`. The last fixture creates a D3D12 WARP software device.
- Release PE exports matched every name and ordinal in
  `build/rust-migration/baseline-exports`: Runtime 381, AC7 256, Loader 273 and
  Minecraft 267. No `rsf_core_capture_vk_*` symbol is exported. The linked
  CPU fixture has no Runtime DLL dependency.
- A separate Release build with `RSF_FRAME_DUMP_VULKAN_INCLUDE=OFF` exported
  377 names: the baseline without the four `rsf_input_dump_vk_*` exports.
  Its exports, native Vulkan, DLSS, DLSS FG and XeSS contracts passed.

## Gaps

No physical GPU, visible-window or game fixture ran. Actual Vulkan
synchronization, the Godot extension (no `RSF_GODOT_CPP_DIR`) and the
Streamline-enabled native Vulkan configuration were not built in this run.

## Integrated checks

The Streamline-enabled Release Runtime and linked AC7, Loader and
Minecraft targets built at `57c66151ff65`. Their export names and
ordinals matched the 381, 256, 273 and 267 baseline pairs. Ten focused
Release and seven default Debug CPU contracts passed, including both
linkage forms of the Vulkan readback test. All 322 locked Rust tests,
formatting and Clippy passed. See
`build/rust-migration-streamline/evidence/57c66151ff65/` and
`build/rust-migration/worker-evidence/rust-vulkan-readback/` for
the integrated and worker logs.

After the sharpening merge, the Streamline-enabled Godot extension
also built with the native Vulkan services linked statically. Its 398
name and ordinal pairs matched the recorded inventory. The graph handoff
and Vulkan creation CPU contracts passed.

## Offscreen device acceptance

The Streamline-enabled Release `input_dump_vulkan` fixture passed
at `9367f1306570`. It submitted the source image on the Vulkan queue,
read it after the capture fence, and retired the reader. All eight
pixels matched RGBA (64, 128, 255, 0), including the original alpha.
The native-format metadata, PNG and completed manifest are in
`build/rust-migration-streamline/evidence/9367f1306570/Vulkan-readback/`.
Streamline's real WSI generated-present capture remains unverified.

## Shared readback locking

The FidelityFX Vulkan wrapper now uses the readback owner's lock for
recording and flushing. Its outer mutex guarded only those same calls.
The SDK callback and drain rules retain their existing scope.
The enabled Release build passed the two FidelityFX contracts, both
linkage forms of the readback CPU contract and the Runtime DLL export
contract. Runtime and the linked Godot extension matched their 381
and 398 recorded export names and ordinals. The logs are under
`build/rust-migration-streamline/evidence/ffx-capture-lock/`.
