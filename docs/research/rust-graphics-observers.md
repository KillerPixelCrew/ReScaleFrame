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
frame-tap caller in the first G01 revision uses the existing `vtable_patch.h` adapter around the Rust writer.
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

## Typed SDK follow-up

The follow-up branch starts at integrated revision `82b39a63` and reapplies the first G01 commit.
G02 frame tap and G03 graphics state keep their source files and ABI.

- D3D11 creation, shader, layout and Present detour entries now use Rust SDK types. The hook
  offsets come from the generated Windows vtables. COM ownership, presenter selection, resize
  bookkeeping and the capture-to-dump calls are Rust-owned. The guarded probe now receives a
  typed DXGI descriptor prepared by Rust. Rust owns the probe window and checks the complete
  consumed slot range before pinning its module. C++ retains the D3D11 Present/Present1 target
  call, fallible driver probe/query calls, foreign callback invocation, and the existing HDE and
  MinHook route reconstruction. The unguarded original creation methods use an unwind-capable
  SDK calling convention so the port does not turn a forwarded C++ exception into a Rust abort.
- D3D12 method-entry forwarding, per-method reentry, SDK vtable offsets, attachment, debug
  interface selection and DRED breadcrumb traversal are Rust-owned. The original calls preserve
  their native return values and unwind behavior. Native guards still contain exceptions at
  the same optional diagnostics and COM-proxy call sites. The public setup record is generated
  from the observer header and has the original layout.
- The capture and D3D12 platform headers were removed with their callback tables. No code
  referenced `vtable_patch.h` after G02, so it was removed. The device ownership header now
  contains a declaration and typed alias for callers.

The first three Rust findings and both C++ fixtures remain closed. The ten closed inventory paths
are the nine header paths above plus `runtime/graphics/src/vtable_patch.h`. The two still-open
paths are `runtime/graphics/src/d3d11_observer.cpp` for guarded calls and the native route
reconstruction, and `runtime/graphics/src/d3d12_observer.cpp` for guarded SDK calls. Existing
runtime evidence for the D3D11 chain is in [Windows Present hook coexistence](windows-present-hook-coexistence.md).

MSVC Windows SDK `/Zs /W4 /WX` accepted both observer translation units and both C fixtures.
MinGW C11 warnings-as-errors accepted both C fixtures in syntax-only mode. Pinned Rust 1.94.1
formatting, locked offline Cargo metadata and Git whitespace checks passed. Rust no-codegen
source analysis included the observer modules and fixture source with external dependencies
stubbed. The detour declarations also check their parameter and result types against the
Windows SDK Rust vtable types in the real crate. No project build or fixture execution occurred.

Both observer bindings were regenerated with bindgen 0.72.1 from the individual generator
clauses. The unscoped generator stopped at the pre-existing D3D11 SR admission clause, whose
clang include set could not resolve `rescaleframe/game_frame.h`. Its unrelated earlier outputs
were restored.


## Route reconstruction and guarded SDK dispatch

This follow-up starts at integrated revision `e5d73636`. D3D11 now opens the checked disk image,
resolves the genuine slot, follows the outer Present chain, relocates the file-backed prologue and
owns MinHook registration and retirement in Rust. It retains the three observed jump forms
(`E9 rel32`, `FF 25 [rip+disp32]` and `48 B8 imm64; FF E0`), eight-hop limit, 32-byte disk
comparison and independent 14-byte bypass. Live reads use bounded `ReadProcessMemory`. MinHook
still decodes each x64 instruction with its real HDE64 implementation. The packed Rust binding
comes from the pinned v1.3.4 HDE header. Rust pads the decoder's private input and rejects
truncation, unknown RIP displacement layouts and relative control flow. A failed installation
releases the trampoline only when it no longer has a published owner. A successfully published
route keeps the previous process-lifetime trampoline policy. The original Steam and RTSS
evidence remains in [Windows Present hook coexistence](windows-present-hook-coexistence.md).

D3D12 query, queue type, removal, debug, DRED and swap-chain calls now use their SDK signatures
in Rust. Their synchronous private thunk permits foreign unwinding into a native catch frame
and keeps the previous HRESULTs and exception fallbacks. The MSVC SDK's queue descriptor return
uses the generated out-pointer signature. DRED node pointers remain borrowed while the query's
COM reference lives. The registered diagnostics and observer callbacks keep their original ABI.

Both original implementation paths (`runtime/graphics/src/d3d11_observer.cpp` and
`runtime/graphics/src/d3d12_observer.cpp`) are retired. Remaining native code is confined to
`runtime/graphics/src/observer_exception_guards.c` (C11 SEH leaf since the G01 closure, see
port-introduced C++ closure). It holds the D3D11 SDK/proxy and user
callback exception guards, the shared overlay's existing `rsf_observer_install_guarded` entry,
and the D3D12 catch frame. The old observer module bridge header was removed. G02 frame-tap
callback leases and shared native ABI, including the overlay registration caller and G03
output calls, were left as integrated. The graphics CMake target still links pinned MinHook;
Cargo dependencies are unchanged.

Targeted bindgen 0.72.1 used the v1.3.4 `src/hde/hde64.h` from the existing MinHook checkout,
the MSVC Windows x64 target and the Windows 10.0.26100.0 SDK. The allowlists were
`hde64s`, `hde64_disasm` and `F_(ERROR|MODRM|RELATIVE|IMM[0-9]+|DISP32|PREFIX_67)`.
The generated packed record is 37 bytes and puts flags at byte 33. MSVC syntax-only checking
covered the native guard file and the existing HDE64 translation unit. Pinned Rust formatting,
observer source-only type analysis with SDK and unrelated crate stubs, locked offline Cargo
metadata and Git whitespace checks passed. No project build, linked fixture, device run or
foreign-exception runtime test was performed.
