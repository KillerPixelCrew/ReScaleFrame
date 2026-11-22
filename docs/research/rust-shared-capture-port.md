# Shared texture and frame capture

## Source and ownership

The starting revision was `cff013cd`, including the V01 Vulkan SR and V03
D3D12 interop ports. The earlier standalone snapshot audit reproduced a
readback from an abandoned recording. It established the per-snapshot fence
path but kept a frame-wide watermark for other readers. That watermark only
counted reservations and could include copies from other command buffers or
queues. [Standalone capture submission](rust-vulkan-capture-submit-cutover.md)

The shared readback owner now resolves and calls Vulkan buffer, memory, copy,
barrier and event procedures through ash 0.38.0. FSR4 and XeSS D3D12 interop FG
use that owner from the Rust graphics table. The standalone snapshot and DLSS
native Present use the same command-pool and fence procedures. The snapshot
still submits through the caller's serialized engine queue callback. DLSS still
submits with the original WSI waits and keeps its per-family slots.

A successful fence wait certifies only the attached capture group. Godot
stage readbacks and FSR4 SR also use the engine's command retirement callback
after the frame fence and command-pool reset. These ungrouped recordings
carry a per-copy event after the copy and host barrier. Both file outputs
require the same evidence. A full drain can release an abandoned allocation
without publishing it. The old native watermark symbols remain callable for
binary compatibility and issue no certificates.

A Vulkan fence that reports device loss terminates the capture without
publishing pixels and permits teardown. A submit that reports device loss can
still leave its command buffer in use, so the owner retains the pool, fence
and copy until the fence completes or reports loss. A pending fence still
permits a retry. [Vulkan device-loss contract](https://docs.vulkan.org/spec/latest/chapters/devsandqueues.html#devsandqueues-lost-device)

The D3D11 texture owner now uses typed D3D11 staging and mapping and writes
raw and Targa files in Rust. WIC remains the PNG encoder. AC7's observer
passes its configured scale, bias and zero rule to the encoded motion
preview. DLSS captures the stored motion as raw pixels and retains the
separate decoded motion preview. D3D12 frame capture and FG colour-pair
capture use one footprint ABI and shared native allocation, copy and mapping
leaves. Each retains its texture admission, fences and file format.

## G07 findings addressed

- **Preserve capture view and resource generation in legacy filenames.**
  Filenames include session, frame, view, generation, role and ordinal.
  Exclusive creation distinguishes repeats. D3D12 events include the SDK
  sequence. Godot analysis refuses duplicate role inputs when it cannot
  select a unique comparison.
- **Serialize shared legacy capture metadata once for D3D12 and Vulkan.**
  The shared writer escapes role strings and writes identity, dimensions,
  jitter and row-vector matrices. Each caller supplies its layout and
  evaluation or completion facts.
- **Apply the same execution proof to the Vulkan legacy file and the core
  ticket.** Both use the readback's exact group or retired command and event.
- **Use one Rust pixel descriptor for capture publication and input-dump
  ingestion.** Both capture owners and the reader use the same layout.
- **Derive capture completeness from the operations the selected frame
  actually performed.** An SR frame requires its scene colour, depth, motion,
  final image and an observed successful SR result. FG results, when
  observed, require the corresponding guides and HUD-less colour. An
  explicitly native frame can complete with its final image alone.
- **Certify GPU completion for the recordings a submission actually
  contains.** Standalone and Present-time owners certify only their submitted
  copies. The engine retirement callback covers the exact command buffer.
- **Use the same camera availability check when writing and updating
  capture metadata.** Both require a compatible camera ABI and a complete
  struct. Enrichment also requires matching dimensions.
- **Use the same source-provenance rule in every capture artifact.** Record
  metadata and stage and parameter sidecars write a null source for ambiguous
  identity and preserve the native counter as the frame ID.
- **Distinguish device loss from a pending Vulkan capture fence.** Lost
  fences allow cleanup without mapping. Timeouts keep pending resources.
- **Derive the velocity preview from adapter-supplied motion encoding,
  not an Unreal constant.** View 1 uses the supplied bias, scale and optional
  zero-X rule. The original options structure retains its size and version.
- **Share D3D12 readback allocation and copy leaves between frame capture
  and FG color pairs.** The native helpers and the generated footprint layout
  serve both owners.
- **Share the capture-ticket test sink between Vulkan and D3D12.** The
  shared sink records identity, reservation parameters, native and storage
  formats, dimensions, pitch, bytes and failures.
- **Share Vulkan capture command-pool and fence operations.** One typed
  dispatch and implementation serves the snapshot and DLSS Present owners.

## Remaining native surface

The later follow-up ports the D3D12 leaves, D3D11 dump leaves, WIC encoder
and all six G07 fixture bodies. This section records the earlier boundary.
Native capture and fixture port

`native_fg_dx12_vulkan.cpp` was removed. `frame_capture_vk.h` and
`input_dump_vulkan.cpp` retain the native ownership facade and export
forwards. The D3D12 readback and fence calls, WIC PNG interop, and D3D11
predication and hash bridge stay native. Public dump headers remain the C
contracts for the linked producers. The C++ fixtures stay in place to cover
the DLL exports, native device interfaces and WIC. In particular,
`tests/vulkan_interop_fixture.h` still provides the real Vulkan device to
`tests/input_dump_vulkan.cpp` and `tests/native_sr_fsr4_vulkan.cpp`. Keeping
this device fixture preserves source coverage of both consumers while its
coordinated language port stays open.

## Verification

Source checks matched 25 named Vulkan procedures to the installed Vulkan SDK
1.4.357 and pinned ash and compared the native declarations to the Rust
exports. New ABI layouts have size and offset checks on both sides. Existing
generated setup and footprint layouts keep the same fields and sizes. Pinned
Rust 1.94.1 formatting, offline locked Windows Cargo metadata, Python parsing
of the affected analyzers and Git whitespace checks passed. Builds, lint,
fixture execution and device validation remain user-owned.
