# D3D12 readback owner Rust port

The baseline is `afaf895`. `rsf::FrameCapture12` in
`runtime/backends/common/frame_capture12.h` was a header-only C++ owner shared
by every first-party D3D12 capture path. This port moves its state into
`runtime/orchestrator/src/frame_capture12.rs` and keeps the class as a façade
with the same constructor, methods, default arguments and return values. It
follows the [Vulkan readback port](rust-vulkan-readback-owner.md). No consumer
source changed.

## Call sites

| Consumer | Calls | Completion proof |
| --- | --- | --- |
| SR12 (`sr12_platform.cpp`, owned by `sr12.rs`) | create with environment, `enabled`, `configure`, `wants`, `force_sr_history_reset`, `record`, `event`, `flush(slot)` | The SR12 owner flushes a slot only after that slot's fence, and drains all slots before destruction. |
| FG12 (`fg12_platform.cpp`) | process-wide owner: `record`, `submitted(queue, all, identity)`, `flush_ready` | A fence signalled after the matching identity's actual submission. |
| Runtime C ABI (`input_dump.cpp`, `rsf_input_dump12_*`) | owner without environment: `record` with caller view and scale, `submitted(queue, slot)`, `flush_ready`, `abandon(slot)`, `pending` | Submission fence; `destroy` refuses while readbacks are pending. |
| FSR3 FG (`fsr_generation.cpp`) | `enabled`, `wants`, `record` from the SDK callback, `event`, `pending`, `flush_after_queue` | A 15 second queue drain after the SDK's callback lists. |
| Godot (`integrations/godot/native/runtime.cpp`) | `wants`, `event`, `flush(slot)`, `record`, `flush()` | The extension's per-slot submission fence, and a drain at shutdown. |

## Ownership

| Boundary | Owner |
| --- | --- |
| Configuration | Rust reads `RSF_FRAME_CAPTURE_DIR`, `RSF_FRAME_CAPTURE_FIRST`, `RSF_FRAME_CAPTURE_COUNT` and `RSF_FRAME_CAPTURE_RESET_SR` with the old limits through the Vulkan owner's `Legacy` window and `wcstoull` parser. `configure` keeps its rules: an absolute UTF-8 path, 1 to 16 frames, no pending readbacks. |
| Admission | Rust polls ready copies, combines the legacy window with `rsf_input_dump_wants`, reserves the core ticket with the shared name or the caller's view and scale, and refuses a source without a ticket or legacy request. |
| Pending identity | Rust holds each copy's record, role, slot, ordinal, dispatch reset, ticket, name, legacy flag and completion fence. |
| Completion | `submitted` signals one fresh fence for unfenced copies that match the slot and, when given, the session, frame, view and generation. `flush_ready` publishes only copies whose fence reached 1. A removed device fails the ticket. `flush(slot)` trusts the caller's proof for that slot. |
| Retirement and refusal | `abandon(slot)` fails unsubmitted copies of the slot with `MAP_FAILED`. A failed map keeps the copy for a later flush. Destruction fails remaining tickets with `STAGING_FAILED`. |
| Leases | Rust holds the readback buffer, the source texture and the shared fence as `ComRef` references until retirement. |
| Publishing | Rust sends core pixels through `rsf_input_dump_pixels` with API 2, the DXGI format and the footprint offset. It writes the legacy `.raw` and `.json` files and the `metadata/*.json` events with the same names, field order and `%.9g` or default `%g` number text. |
| Native leaves | The façade validates the source and creates the readback buffer, records the transitions and footprint copy, maps, unmaps, signals a fence, reads its completed value and drains a queue. It also supplies `capture_name`, `capture_view` and `capture_scale`, so naming stays in `capture_names.h`. |

The internal `rsf_core_capture12_*` symbols live in the existing static Rust
archive. Every consumer already linked it through `ReScaleFrame::FrameDump`,
`rsf_orchestrator_core` or the Runtime DLL. They are not DLL exports.

## Behaviour changes

- Destruction fails each outstanding core ticket with `STAGING_FAILED` before
  it releases the readback. The C++ owner dropped them, so the input dump
  waited for its 30 second incomplete timeout. This matches the Vulkan owner's
  follow-up fix. The process-wide FG12 owner can therefore report a failure
  during static destruction if a capture is in flight at unload.
- `wants`, `enabled`, `force_sr_history_reset` and `event` read the window
  under the owner lock. The C++ owner read the directory and counts without the
  lock that `configure` held while it replaced them.
- `record` holds the owner lock across its poll, admission, allocation and
  recording. The C++ owner released it between the poll and the push. The
  queue drain still runs without the lock.

## Validation

Twenty Rust tests in `frame_capture12/tests.rs` drive the owner with fake COM
objects, fake D3D12 leaves and a fake input dump. Fake memory holds `0xef`
until a test marks the copy executed. They check reference counts of the
source, buffer and fence leases and cover these cases:

- An unrequested frame does no GPU work. A refused source, a missing list, a
  refused reservation or a failed name reserve or record nothing and release
  the buffer.
- A core ticket is not mapped before its submitted fence completes, and then
  publishes the executed bytes, format, pitch and size once.
- Published pixels start at the footprint offset.
- Submission fences only the matching slot and identity, keeps an earlier
  fence, creates nothing without a match and reports a failed signal.
- A removed device fails the ticket without mapping.
- A slot flush publishes only that slot. Abandon fails only unsubmitted copies
  of the slot. A map failure keeps the copy for a later flush.
- Caller view and scale overrides and names longer than the first buffer reach
  the reservation.
- Configuration refuses relative, empty, zero, oversized and busy requests. The
  reset request follows the legacy window.
- Legacy `.raw` and `.json` output keeps the footprint, identity, matrices and
  dispatch reset. Core and legacy outputs share one mapping.
- Events are written only inside the window, with default stream precision.
- A failed queue drain keeps the copies. A successful one publishes them.
- Destruction fails pending tickets and releases every lease.

These checks ran with the pinned Rust 1.94.1 toolchain and MSVC 19.52
(Visual Studio 2026) in an independent Streamline-enabled build with Godot
enabled. Vendor headers were read from the coordinator checkout and the
pinned yyjson files were copied after their lock hashes matched.

- `cargo fmt --all --check` and `cargo clippy --workspace --all-targets
  --locked -- -D warnings` passed. `cargo test --workspace --locked` passed
  353 tests.
- The Release tree built every consumer: Runtime, AC7, Loader, Minecraft,
  Godot, FSR3 and the SR12, FG12 and `input_dump_d3d12` fixtures. The only
  failure was `rsf_engine_sharpening_test`. Its `rsf_srvk_frame` layout
  assertion predates this change (the test was last edited before
  `e7ee7bc` moved the Vulkan SR frame), and this diff does not touch the
  structure.
- The Debug build of the same consumers and fixtures had no warnings.
- Release passed 13 focused CPU and ABI contracts: `sr12_abi`,
  `fg12_abi` (DLL and embedded each), `input_dump_vulkan_cpu_linked`,
  `input_dump_vulkan_cpu_dll`, `native_vulkan_dll_exports`,
  `shared_shim_exports`, `callback_host_abi` (both forms),
  `godot_graph_handoff_contract`, `failure_capture_policy_contract` and
  `capture_metadata_contract`. Debug passed the seven SR12, FG12, Vulkan
  readback and Godot contracts.
- Release PE exports matched every name and ordinal: Runtime 381, AC7 256,
  Loader 273 and Minecraft 267 against `build/rust-migration/baseline-exports`,
  and Godot 398 against the recorded `ffx-capture-lock` inventory. The six
  `rsf_input_dump12_*` exports are unchanged and no `rsf_core_capture12_*`
  symbol is exported.

## Gaps

No device fixture ran. `input_dump_d3d12` was built but not run. It
creates a WARP D3D12 device and is the end-to-end check of the native
leaves through `rsf_input_dump12_*`. Actual GPU copies, fence completion,
device removal, the FSR3 queue drain and the Godot D3D12 path remain
unverified on a device or in a game.
