# Anomaly overlay probe policy in Rust

The source baseline is Anomaly's overlay hooks at revision
`aa66153c`. The [overlay wiring evidence](anomaly-overlay-wiring.md) connects
the DXGI and DirectInput calls to their engine users. This port moves the
selection and admission of test objects and method addresses into
`games/anomaly/src/overlay_hooks.rs`, which already owns installation and
routing. The C++ callback tables supply native COM and D3D11 operations
and the addresses of the existing detours. Callers continue to use the
existing public overlay and game ABI.

## DirectInput

The probe reads the Windows system directory into a MAX_PATH buffer and
appends the terminated `\dinput8.dll` suffix. It loads that absolute path
using `LOAD_LIBRARY_SEARCH_SYSTEM32` and resolves `DirectInput8Create`.
The native call uses the current executable's instance, version `0x0800`
and no outer object.

| Call order | Interface or object | GUID |
| --- | --- | --- |
| First interface | IDirectInput8A | `BF798030-483A-4DA2-AA99-5D64ED369700` |
| Second interface | IDirectInput8W | `BF798031-483A-4DA2-AA99-5D64ED369700` |
| First test device per interface | System mouse | `6F1D2B60-D5A0-11CF-BFC7-444553540000` |
| Second test device per interface | System keyboard | `6F1D2B61-D5A0-11CF-BFC7-444553540000` |

For each successful interface, the probe admits CreateDevice (slot 3)
before creating test devices. It admits each available device's Acquire,
Unacquire, GetDeviceState, GetDeviceData and SetCooperativeLevel in that
order (slots 7, 8, 9, 10 and 13). Failed creates and null results skip
the corresponding object. A registration refusal stops the probe
immediately and releases the current device and interface. Each test
object is released through slot 2 and at least one test device must
complete admission.

The refusal order is `System32 directory unavailable`,
`system dinput8.dll could not be loaded`, `system DirectInput8Create missing`,
the first registration error, then
`no DirectInput mouse or keyboard device could be probed`. The
CreateDevice detour still classifies the same six system and emulated
mouse and keyboard GUIDs. Its native frame calls the engine object's
own trampoline before registering a successful device with the gate.

## DXGI

The probe loads system `d3d11.dll` and resolves
`D3D11CreateDeviceAndSwapChain`. It creates a hidden 8×8 STATIC popup
from the executable instance. The native WARP call retains the typed
SDK descriptor with one 8×8 R8G8B8A8_UNORM render-target buffer,
one sample, windowed mode and DISCARD swap effect.

After successful chain creation, the native QueryInterface call asks for
the SDK UUID of IDXGISwapChain1, `790A45F7-0D42-4876-983A-0A55CFE6F4AA`.
The probe reads slot 22 when available
and releases that interface before admitting the base methods. It
admits Present, ResizeBuffers and ResizeTarget (slots 8, 13 and 14),
then admits Present1 if slot 22 was nonzero. A failed query, null interface
or null slot skips Present1. A conflict on an available Present1 target
refuses the whole batch. On every return after chain creation starts, the
probe releases any returned chain, context and device in that order and
destroys the window.

The refusal order is `d3d11.dll unavailable` for either load or symbol
failure, `dummy window creation failed`, `dummy swap chain creation failed`
for a failed HRESULT or null chain, then the first registration error.
Native Present and resize detours keep their existing typed frames and
forward through the published trampolines.

## Failure handling and lifetime

The directory capacity check compares the returned length against the
space left after reserving the suffix. It preserves the existing refusal
and avoids unsigned overflow when adding the suffix length. The old
callback protocol supplied `overlay hook discovery failed` as a fallback
if a false result omitted its reason. Every production refusal supplied
a specific reason, and the Rust probe result now requires one.

The existing batch still rejects null and cross-method aliases, accepts
same-method aliases and creates no hooks until discovery completes.
Loader references are released on failure before activation, including
unqueued creation refusal after its rollback. When the complete batch is
published, the module ref and trampolines stay for the process lifetime.
An activation refusal withdraws the attempted gate or sink and queues
the targets off through the existing shared MinHook transaction.

## Verification scope

The GUIDs and slots were compared with the baseline and Windows SDK
10.0.26100.0 headers. Rust 1.94.1 formatting, locked offline Cargo
metadata and whitespace checks passed. The existing Rust fakes retain
duplicate and aliased targets, creation rollback, failed activation
and reentrant routing through the same registry. Build, link, Clippy,
contract execution and live WARP, DirectInput and game validation
remain open.
