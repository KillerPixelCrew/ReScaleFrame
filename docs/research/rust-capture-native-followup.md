# Native capture and fixture port

The follow-up starts at `82b39a63`. The earlier shared capture port established
the recording identity and publication rules. This change moves their
remaining D3D12, D3D11 and WIC operations to typed Windows bindings.
[Shared capture port](rust-shared-capture-port.md)

## Native ownership

D3D12 frame capture and FG colour-pair capture now use the same Rust
implementation for subresource-zero footprint allocation, copy, map and
unmap. The frame capture path still requires a single slice. The FG
colour-pair path keeps its own slice, format and matching-extent admission.
Both restore the supplied source state after recording the copy.

The D3D12 capture table still has eleven entries. The FSR provider receives
that table through its existing C++ accessor, which now returns the Rust
implementation. Provider recording IDs, submission callbacks and resize
retirement rules are unchanged. The capture owner still joins the provider's
submission callback before draining the queue and uses the same bounded
fence wait and device-removed result. No V04 or V05 provider owner changed.

D3D11 input dumps now save and restore the exact predication interface and
BOOL value in Rust. Rust also formats the executable digest and keeps the
existing shared file-hash service and sharing flags. The PNG encoder uses
typed WIC calls, keeps its COM apartment ownership rules, and checks the
actual negotiated pixel format before writing. Native headers and export
facades preserve the public C entry points.

## Fixture and ABI disposition

The fixture bodies in `tests/capture_metadata.cpp`, `tests/input_dump.cpp`,
`tests/input_dump_d3d12.cpp`, `tests/input_dump_vulkan.cpp`,
`tests/input_dump_vulkan_cpu.cpp` and `tests/texture_dump.cpp` now live under
`tests/capture/src/windows/`. C entry points keep the existing executable
and CTest names. Vulkan CPU coverage still runs through both the linked
native archive and the Runtime DLL import. Vulkan live capture uses its own
typed test device, so the V03 `tests/vulkan_interop_fixture.h` remains only
with the existing provider fixture and is outside the G07 replacements.

The D3D11 and D3D12 bodies retain WARP, staging, PNG and debug-message checks.
The Vulkan CPU body retains command and fence failure injection, exact
publication checks, timeout and retry, and both device-loss cases. Vulkan
live capture still fills a real Vulkan image and checks the exact bytes and
the published bundle. Fixture compile, link and execution remain open.

The existing D3D12 footprint binding retains its layout and both consumers.
The dedicated fixture ABI was regenerated from the C headers with
bindgen-cli 0.72.1. Repeated targeted generation produced identical output
with Rust 1.94.1 formatting. Locked offline Windows Cargo metadata resolved
with every fixture feature. Source checks matched the FSR table accessor,
the fixture entry points, the public Vulkan ABI and the typed native method
declarations. They matched 33 Windows method names and 21 mock Vulkan PFN
names to the installed bindings and Vulkan headers. No build, test, lint
or device run was performed.

The public texture and input dump forwarding objects now compile as C11.
Windows once initialization publishes the same static capture service table.
The Anomaly stage binds the Rust pixel-format lookup through its C signature.
Strict MinGW C11 syntax and a source comparison of the forwarding names passed.
Native linking and capture verification remain pending.

## D04 and G07 capture ownership at 6cb0c169

This follows the integrated D04 platform binding at 8a61eba0 and retains the
shared GameSDK policy from 935757d5. The port checklist paths resolve as
follows.

| Path | Source evidence |
| --- | --- |
| runtime/orchestrator/src/sr12_platform.cpp | Removed. sr12.rs supplies the eleven public SR12 symbols and sr12/native.rs calls the Rust capture owner directly. Per-slot flush uses the caller's slot and ready-only zero. Record keeps ordinal, reset, preview and scale. Final flush precedes vendor, capture and sharpening destruction in their existing order. A missing diagnostic owner leaves SR available. |
| runtime/orchestrator/src/fg12_platform.cpp | Retains the native presentation install exception handler and two module lifetime calls. fg12/native.rs owns the capture handle and supplies the fourteen FG12 entry points and CPU service. FG records the given slot, signals submission with the exact frame identity across all slots, and flushes with ready-only set. Startup reads capture preferences at module attach, and teardown drops the capture at its original native lifetime boundary. |
| runtime/backends/common/frame_capture12.h | Contains the opaque platform accessor declaration for the D07 FidelityFX generation gate. frame_capture12.rs keeps the existing completion, device-removal, refusal and retirement paths. FSR still obtains the same Rust table in the SDK-enabled branch. |
| runtime/backends/common/frame_capture_vk.h | Contains the C ABI for the Rust readback owner. The native DLSS SR, FidelityFX FG and XeSS SR SDK adapters keep command retirement. FSR4 SR, DLSS FG and its dedicated present capture keep shared submission. These adapters now pass the opaque handles directly through that ABI. Existing core-source flags, group identities, exact command retirement, full flush and destructor order remain at their call sites. |
| runtime/orchestrator/src/input_dump_vulkan.cpp | Removed. input_dump_vulkan.rs defines the four public symbols. input_dump_vulkan_exports.c supplies link-only directives under the existing RSF_FRAME_DUMP_VULKAN_INCLUDE gate, including for embedded consumers. The Vulkan owner still retries timeout and uncertain submission, and allows destruction on terminal device loss. Its setup size of 128 and submit offset of 112 are asserted by vulkan_sr_abi.rs. |

The Vulkan ABI source reproduced byte for byte. FG12 gained the versioned
CPU-service binding from the existing C declaration. Source comparison
checked public symbols, vendor gates, capture modes, and the order and
arguments of record, submit, flush and retirement operations. Rust 1.94.1
formatting and locked offline Windows Cargo metadata passed. Build, link,
test, lint and device checks remain pending.
