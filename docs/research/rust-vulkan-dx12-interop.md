# Vulkan-to-D3D12 FG interop and guide retirement

The V03 port starts at `9bb9421d`, which includes the V05 external image copy
planner. The existing FSR4 and XeSS bridges share the D3D12 image importer,
presentation ring, source recording ledger and Vulkan graphics queue handoff.
FSR4 Vulkan SR also uses the importer platform table. Unity supplies the
same graphics setup contract through the Runtime and has no direct importer
or private graphics-table call.

## Source changes

- The importer binds each Vulkan procedure directly to its named, typed field.
  Adapter identity is checked before opening imports. All 13 importer lookups
  precede the completeness check. Presentation resolves 22 device functions
  with early refusal on a missing entry, followed by both instance functions.
- The Rust graphics leaves use ash's Vulkan 1.3.281 signatures and descriptors.
  They continue to use the host queue submit and lock callbacks. Vulkan submits
  preserve waits, timeline values, stages and fence order. The Vulkan Present
  reader admits the outer shape before accessing the arrays, and writes
  `pResults[0]` only when the provider replaces the incomplete sentinel.
- The importer still intersects image requirements, Win32 handle types and
  device-local memory types. Dedicated allocation precedes binding, and the
  Win32 handle closes once the import call is finished. Import failure cleans
  the image, memory and D3D12 reference in that order for retryable FG imports.
  The shared SR platform table and its construction cleanup are preserved.
- FSR4 and XeSS retire a reused slot's consumer timeline and its Vulkan source
  fences before releasing the depth/motion pair on a render-extent change.
  Both old handle identities are checked before either is removed. Prepare
  invalidates the old recording on replacement, retains the HUD-less and
  presentation resources and retries any missing guide on the next prepare.
  The next successful guide copy uses first-use layouts for both new images.
  A render-extent change also sets reset on the stored FG frame.
- Vulkan FG inputs use the SR descriptor predicate for device, image, extent,
  format and layout. DX12 and DLSS require copy-source permission in addition.
  Native FidelityFX guides are consumed directly and keep their prior rule.
- The common recording type supplies the capture group to both the DX12 and
  DLSS paths for capture, completion and discard. The group preserves the
  existing sequence and low-bit HUD-less encoding. Presentation first-use is
  tracked through the committed timeline, with zero representing no submit.

## Bindings and evidence

The ash tarball SHA-256 was checked against the Windows dependency entry in
`Cargo.lock`. Source checks matched all 31 distinct typed procedures to the
installed Vulkan SDK 1.4.357 and ash. Locked Windows Cargo metadata resolved,
and Git whitespace checks passed. The private graphics table uses 30 pointer slots,
the importer table uses four, FSR4 embeds both and holds 43 pointers overall,
and XeSS continues to borrow its tables. The importer platform, shared image
and callback records retain their generated layouts. There is no public game
ABI or Vulkan setup version change.

## V03 checklist disposition

The source changes close these review findings:

- **Resolve Vulkan dispatch where the typed function pointers live.**
  The importer and FG dispatch bind each named procedure to its Rust type.
- **Validate Vulkan image descriptors through the existing shared predicate.**
  DX12 FG, DLSS-G and native FidelityFX use the Vulkan SR predicate.
- **Resize retired FG guide images independently of the presentation swapchain.**
  The importer validates and releases the drained pair and prepare rebuilds it.
- **Derive Vulkan FG capture group IDs in the recording ledger.**
  The recording type calculates IDs for both capture paths.
- **Use the committed timeline values to identify first-use images.**
  The presentation owner uses zero timeline values for first use.

The port removed `runtime/orchestrator/src/native_dx12_vulkan_interop.cpp`
and `tests/native_dx12_vulkan_interop.cpp`. The remaining interop headers
`runtime/orchestrator/src/native_dx12_vulkan_interop.h`,
`runtime/orchestrator/src/native_dx12_vulkan_platform.h`, and
`runtime/orchestrator/src/native_fg_dx12_vulkan_platform.h` carry the C ABI
declarations and fixed ABI data.

## Fixtures and remaining native code

The former native interop contract fixture lives in Rust tests. Fake typed
Vulkan callbacks cover missing dispatch, timeline import, mismatched memory
types, partial allocation cleanup and checked guide release while real Win32
event handles exercise transfer ownership. The XeSS preflight check is in the
XeSS Rust tests. The FSR4 state fixture uses the production Present descriptor
reader and exercises reused-slot replacement after both completion proofs.

`native_fg_dx12_vulkan.cpp` retains the FrameCaptureVk adapter used to attach
the existing C++ readback owner to the Rust platform. Porting that last adapter
requires the shared readback leaf work so FSR4 and XeSS can keep the common
capture allocation, encoding and publication path. The C++ device utility in
`tests/vulkan_interop_fixture.h` still serves two live-device fixtures,
`input_dump_vulkan.cpp` and `native_sr_fsr4_vulkan.cpp`, which borrow its typed
device, command procedures and image lifetime. Those remain a coordinated port.

No runtime experiment was performed. Source, lockfile and ABI checks provide
evidence for the port. Compile, link, lint, fixture runs and live device
evaluation are user-owned.
