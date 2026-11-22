# Graphics observer ownership port

G01 started from `8adf010b`. The evidence is a source comparison against the review at
`894a55f9` and the native inventory at `de8b7510`. No live game or GPU captures were made.

The D3D11 creation hooks pass the device that created each resource. Capture resolves its COM
identity with `QueryInterface(IID_IUnknown)`, retains that identity with its resources, and keeps
up to four recently observed device inventories before Present. The first presenting device
selects an inventory and retires the others. Later creation from another device cannot replace
a same-sized constant buffer or use the selected texture capacity. A dump request stays pending
if a different device presents. The provisional device and immediate context acquired before
Present are still supported, with the context cleared when selection changes devices.

The WARP/hardware probe previously returned table addresses after releasing all of the temporary
COM references. The native table callback now checks that the complete consumed slot range
resides in committed, readable image storage and pins the owning module before that release.
An object-local required table refuses installation. An unverified optional Present1 table is
omitted. The probe still destroys its temporary window after releasing COM references. The Rust
fixture covers refusal and reference balancing. Module pins intentionally last until process exit.

The observer and Skyrim now share a read-only AMD64 PE32+ parser for on-disk spans. Both consumers
admit only bytes backed by the matching section. Skyrim keeps its file size, timestamp, optional
header size, section-count, image-size, executable-section and exact-byte rules. The observer keeps
its own file-size and sharing rules and uses the preferred image base. Shared vectors cover
truncated section tables, out-of-range spans and virtual tails that overlap populated file bytes.

The device ownership check, D3D11 compare-and-exchange slot writer and status/dump entry points,
and D3D12 MinHook calls run in Rust. The native D3D11 and D3D12 source paths still contain COM
leaves. D3D11 retains the HDE and MinHook Present chain reconstruction and its exception guards.
D3D12 retains the SDK method forwarding and guarded DRED and debug-interface leaves. The temporary
frame-tap caller on this branch uses the existing `vtable_patch.h` adapter around the Rust writer.
G02 removes its last caller. Public observer callback signatures and A03 layout bindings keep the
same ABI. The Skyrim hunk touches only the disk validation path, which S01 also borders when moving
the caller to Rust.

Both CTest observer fixtures now use the Windows SDK C vtables. The D3D11 fixture adds a helper
device that fills its own texture capacity before the real device creates its target and creates a
same-sized constant buffer after Present. The fixture checks the selected device, real texture
dump, and real constant payload. D3D12 keeps the WARP creation, command submission, fence,
nested Present, unchanged vtable, and COM teardown checks.

Source checks used MSVC `/Zs /W4 /WX` with the Windows SDK and the existing MinHook headers,
MinGW C11 `-fsyntax-only`, pinned Rust 1.94.1 formatting and no-code-generation analysis for the
shared PE fixture, locked offline Cargo metadata, and Git whitespace checks. Builds, linked tests,
lint and device execution remain user-owned.

## Checklist handoff

Source closures for the three Rust findings:

- **Associate captured resources with the selected presenting device.** Capture partitions by
  COM identity, selects and retires inventories under the capture mutex, and checks the current
  presenter before consuming a request.
- **Retain or verify the storage behind D3D11 probe vtables.** Required slots are admitted only
  from resident pinned image storage. The optional extended table uses the same gate.
- **Share checked on-disk PE section lookup between the observer and Skyrim compatibility code.**
  Both callers now use `runtime/common/src/pe32_plus.rs` and keep their own policy.

The two C++ fixtures are closed by `tests/d3d11_observer.c` and `tests/d3d12_observer.c`.
The following inventory paths contain only ABI declarations, layout checks or typed C++ call
adaptation. Their implementation ownership is in Rust.

- `runtime/graphics/include/rescaleframe/d3d11_observer.h`
- `runtime/graphics/include/rescaleframe/d3d12_observer.h`
- `runtime/graphics/src/d3d11_observer_callbacks.h`
- `runtime/graphics/src/d3d11_observer_capture.h`
- `runtime/graphics/src/d3d11_observer_hooks.h`
- `runtime/graphics/src/d3d11_observer_module.h`
- `runtime/graphics/src/d3d11_observer_present.h`
- `runtime/graphics/src/d3d12_observer_platform.h`
- `runtime/graphics/src/device_owner.h`

Leave `runtime/graphics/src/d3d11_observer.cpp` and `runtime/graphics/src/d3d12_observer.cpp`
open for their native forwarding and guarded SDK leaves. Remove `runtime/graphics/src/vtable_patch.h`
in integration after confirming the G02 frame-tap leaf and its include are gone.
