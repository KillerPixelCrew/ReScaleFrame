# XeSS Vulkan bridge present inputs in Rust

Baseline `a5867f2f92f9b883b51f6cf93355005d6e20d210`. The question was whether the
shared XeSS session could own device-fact admission and command recording
order while preserving both entry points. Source review traced
`CheckPresentResources` and `RecordPresentInputs` in
`runtime/vulkan/xess-fg/bridge.cpp` through `session.rs`, the integrated
Vulkan/D3D12 producer, and the standalone frame layout. The ABI comparison
used the local XeSS headers at `8fe81bdbbaf00b3c1b733fd0d830c333dc84e6f0`
and Windows SDK 10.0.26100.0.

## Admission

Each frame still reaches admission before timing starts or an allocator is
selected. After the existing frame, pointer and fence checks, the session reads
the D3D12 resource descriptions in the baseline order: color, HUD-less, UI
when supplied, motion, depth. The Rust predicate uses the full 64-bit
resource width and 32-bit height, both in pixels.

| Input | Integrated `BACKBUFFER_HUDLESS` | Standalone `HUDLESS_UITEXTURE` |
| --- | --- | --- |
| Final color and HUD-less | Both match the configured output extent and BGRA8 UNORM (87) | Same |
| UI | The integrated frame supplies a null UI pointer | Required, with output extent and BGRA8 UNORM (87) |
| Motion | RG16F (34), same width and height as depth | Same |
| Depth | R32_FLOAT (41) | D32_FLOAT (40) |

In backbuffer HUD-less mode, a UI pointer supplied via the standalone-shaped
entry point still gets described and tagged. Its dimensions and format are
unrestricted in that mode. Admission compares the motion and depth resource
extents to each other and adds no checks for other descriptor fields. A
mismatch returns `-2` through the existing shared-fence recovery path.

The integrated producer in `vulkan_dx12_fg.rs` supplies the D3D12 pointers
from the selected source for that Present. When interpolation is disabled,
it supplies the completed color pointer for both color and HUD-less, with
private depth and motion guides. The standalone frame keeps the separate
UI pointer in its existing sequential layout. The
[earlier cutover](rust-vulkan-xess-fg-cutover.md) records producer and shader
evidence for depth, motion, postprocessed HUD-less and final color. This
change reads the same resources at Present and contributes no new producer,
shader, binding or moving-frame evidence.

## Recording and ownership

The Rust session builds the transition sequence from the first occurrence of
each non-null resource pointer in color, depth, motion, HUD-less, UI order.
Native code applies one all-subresource barrier per transition. Each resource
goes from COMMON to COPY_SOURCE. The bridge acquires one reference to the
current DXGI backbuffer, transitions it from PRESENT to COPY_DEST, and copies
the final color to it.

The session then requests XeSS tags in depth (1), motion (2), HUD-less (0),
UI (3) order, skipping null pointers. Roles keep their individual tags
when they alias a resource. The native tag call samples each resource
description at tag time and passes the original frame ID, width cast to
`uint32_t`, height, COPY_SOURCE state and ONLY_NOW validity. COMMON and
PRESENT have value 0, COPY_DEST is `0x400` and COPY_SOURCE is `0x800`.
The C++ file checks those values and the format and role values against the
native headers.

| Path | Next effects |
| --- | --- |
| Successful recording, including nonnegative SDK tag results | Restore every unique input to COMMON in first-use order, restore the backbuffer to PRESENT, release its reference, emit the optional end timestamp, close the command list and tag constants. |
| Failed DXGI GetBuffer | Close the command list and return the original HRESULT. No backbuffer reference was transferred to the session. |
| Negative XeSS resource-tag result | Close the command list, release the backbuffer reference and return the first negative SDK result. The recording is discarded without restoring barriers or submission. |

On the failure paths the recording error takes precedence over the cleanup
Close result. The caller retains the same recovery handoff. Comparison of
the remaining source sequence covered allocator wait and reset, GPU
timestamps, constants, shared ready wait, execution, present ID, DXGI
Present, done and local completion signals, status, diagnostics, and XeLL.

## Private ABI and checks

The private Windows x64 platform table moves from ABI 4 to 5 and from 512 to
544 bytes at alignment 8. The two aggregate callbacks become six leaves for
description, transition, acquire, copy, tag and release. The 16-byte
description has width at offset 0, height at 8 and format at 12. Source
comparison matched all 67 callback slots and 18 asserted table offsets in
Rust and C++, through the final diagnostics entry. Runtime CMake still
links the bridge to `rsf_orchestrator_rust` and
the standalone MinGW script still links it to the Rust static library built
from `session.rs`. The public ABI stays at 1. The 27 C++ exports and 11
Rust session C ABI signatures match the baseline source.

Rust 1.94.1 formatting, locked offline Cargo metadata and Git whitespace
checks passed. Compile and link for both consumers, Clippy, Rust and native
tests, device and game verification, deployment and push are user-owned.
