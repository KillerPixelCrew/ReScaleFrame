# Standalone Vulkan capture submission

The baseline at `40d1112` recorded snapshots into one reused command buffer.
`FrameCaptureVk::record` could insert a readback before `vkEndCommandBuffer`
or the engine's submit callback failed. The error path failed the shared
capture ticket and left the native allocation queued. A later snapshot or
empty drain could map it through the legacy output path after a different
submission finished.

The CPU DLL fixture reproduced this against the pre-port Release Runtime
from the coordinator's `build/rust-migration`. The fixture exited 1 and
found `efefefef` in both `31-32-refused_end-0.raw` and
`31-32-refused_submit-0.raw`. The fake Vulkan mapper writes those bytes when
asked to map a copy that never ran. The completed snapshot contained the
expected `4080ff00`. The reproduced Runtime's SHA-256 was
`fd289dac655a57a1cdb4818c7c2ba2a9bbd504603aa4d83fd00a9ea4ec01f9e6`.

## Ownership and evidence

Rust owns the command pool, its primary command buffer and one reused fence.
It admits snapshots through the existing shared Rust capture request. A call
for an unrequested frame does no GPU work, even while an earlier copy is
pending. Native code resolves Vulkan functions, builds Vulkan call structures,
and uses the existing `FrameCaptureVk` readback and encoding methods.

Each recording receives a distinct nonzero group within its capture owner.
Rust completes that group only after the engine callback accepts the
submission and the fence reset for it signals. The separate shared watermark
still certifies earlier engine captures on the same queue. The standalone
readback uses the exact group for both core and optional legacy publication.

Recording and rejected submissions discard their group before another copy
can record. The owner can free their allocations immediately when no copy
is in use. Device loss during submit can leave a copy in use, so Rust discards
its publication proof and retains its resources while the fence remains
unresolved. Timeout preserves the submitted group, readback, command buffer
and fence. Drain joins that work and uses the same serialized engine callback
for a new empty submission. An empty submission supplies no snapshot proof.
[Vulkan command buffers](https://docs.vulkan.org/spec/latest/chapters/cmdbuffers.html),
[Vulkan device loss](https://docs.vulkan.org/spec/latest/chapters/devsandqueues.html)

## Validation

The two CPU fixtures exercise the production C ABI and `FrameCaptureVk` with
Vulkan function doubles. They check no-request admission, partial creation and
recording cleanup, successful pixels in both output paths, poisoned and stale
shared completion evidence, end and submit refusal, timeout and retry, empty
drain ownership and uncertain submission retirement. One fixture links the
native archive and the other imports the Runtime DLL.

The MSVC 19.52 Windows builds used separate worktree-local build directories
and FetchContent directories. They reused the coordinator's MinHook source
and SDK headers. A fresh yyjson download failed the existing hash check.
The build resumed after copying the three pinned files from the coordinator
checkout into the ignored vendor directory.

- MSVC Debug and Release each passed `input_dump_vulkan_cpu_linked`,
  `input_dump_vulkan_cpu_dll` and `native_vulkan_dll_exports`. Both compiled
  the existing `rsf_input_dump_vulkan` device executable and the capture C
  header fixture.
- Release PE inspection matched every export name and ordinal in the main
  worktree's `build/rust-migration/baseline-exports` for Runtime (381), AC7
  (256), Loader (273) and Minecraft (267). The Runtime places
  `rsf_input_dump_vk_create`, `rsf_input_dump_vk_destroy`,
  `rsf_input_dump_vk_drain` and `rsf_input_dump_vk_snapshot` at ordinals
  198 through 201 respectively. The CPU DLL fixture imports all four.
  The linked CPU and device fixtures have no Runtime DLL dependency.
- A separate Release Runtime build with `RSF_FRAME_DUMP_VULKAN_INCLUDE=OFF`
  omitted the four standalone exports and retained all 377 other names.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked
  -- -D warnings` and `cargo test --workspace --locked` passed. Cargo reported
  211 passing tests.

The optional corrected Streamline package and Godot extension were absent
from these build configurations. Actual GPU synchronization and game capture
remain unverified. The device executable was built only, and visible-window
tests and the full release verification script were outside this run.
