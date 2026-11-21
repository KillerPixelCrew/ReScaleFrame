# Unity D3D11 shared graphics ownership

The source audit starts at `9260a14` and follows the Vulkan
[shared device audit](rust-unity-vulkan-shared-device.md) and the checked package retirement
merged in `e17d1df`. The question was whether the package's D3D11 bridge could create, lend and
retire its shared devices, fences and command slots as one Rust record, under the frame callback
admission and fence proof it already uses, without changing the managed or DLL-facing contract.

## What moved

`games/unity-mono/src/d3d11_state.rs` (boundary `d3d11_state.h`) now owns one record: Unity's
D3D11 device and immediate context as `ID3D11Device5` and `ID3D11DeviceContext4`, the D3D12
device and queue, the completion fence, the `rsf_shared_fence` and one D3D12 allocator and list
for each of the `RSF_UNITY_FRAMES_SLOTS` ring slots. It also owns the single creator, the stopped
state after a failed creation (the C++ `failed11`), the checked close and release.
`integrations/unity/native/bridge.cpp` keeps Unity interface access and the typed COM calls as
leaves (`create_shared`, `destroy_shared`, `create_command_slot`, `destroy_command_slot` and two
non-waiting fence reads), the six shared surfaces, the D3D11 texture leases, recording and
submission. The ring's slot rotation and fence values stay in `bridge_interop.rs`. The shared
surfaces later joined the record; see [shared surface ownership](rust-unity-d3d11-surfaces.md).

## Creation and use

The first render callback that needs the record claims creation in Rust, then the leaves run
outside the state mutex. The create leaf reads Unity's device and context, and takes the Runtime's
D3D11 presentation device and native queue when `rsf_d3d11_present_graphics` reports one, else
creates a D3D12 device and queue on Unity's adapter. It then creates the shared fence and
completion fence. The device selection, the native-queue unwrapping, the fence flags and the
`services.log` routing are those of `create11`. Each reference transfers to the record as soon as
it is held. Rust then creates every command slot, each closed as before, and publishes the full
record in one step. A refusal destroys slots, then the shared fence and references, before the claim
ends, and later callers see `STOPPED` without another report until release. The bridge reports
`RSF_BACKEND_ERROR_INIT_FAILED` with the same message once.

`execute11` borrows the record under `render_event`'s frame callback. The window, Editor preview,
reconstruction and HUD-less paths use the same queue, fences, surfaces, barriers and render-pass
calls in the same order. The order of shared fence values is unchanged: the D3D11 upload signal and
flush, the D3D12 wait, execution, the D3D12 hand-back signal, the D3D11 wait, the optional output
copy fenced with `rsf_unity_interop_d3d11_tail`, then the completion signal with the stop routes
for refused signals. Slot reuse still waits for the slot's completion value first.

The package's graphics lookup (`rsf_unity_package_graphics`, used for Auto's generation backend
and Editor preview setup) used to copy the D3D12 device and queue globals after reading an
unguarded `initialized11`. It now enters a frame callback, borrows the published record and takes
the caller's COM references before leaving.

## Checked close and release

`try_close_shared_bridge` still closes callback admission first under the bridge mutex. For D3D11 it
then calls `rsf_unity_d3d11_try_close`, which closes the record to new borrows and creation and
reads both fences outside the mutex. The verdict is the one the C++ computed: a completion value of
`UINT64_MAX` refuses, the ring must have reached its last completion and the shared fence the last
D3D11 copy. Without a record both reads are 0. A refusal keeps the record closed for the retry.

On commit, `rsf_unity_package_bridge_finish_retirement` destroys the surfaces, then
`rsf_unity_d3d11_release` destroys the slots and then the shared fence, completion fence, queue,
D3D12 device, context and D3D11 device in the C++ order, then forgets the ring. Release refuses,
destroying nothing, unless the record was closed. The record stays closed until the next package
session's `rsf_unity_package_bridge_open`. The Vulkan path keeps releasing the ring itself.

## Intentional differences

- Command slots are created with the device instead of lazily on each slot's first use. A slot
  failure now fails initialization with the existing report and native output, instead of
  silently skipping that frame and retrying.
- A failed creation releases its partial objects immediately instead of holding them until
  release.
- Command lists release before the shared fence rather than after it. Neither references the
  other.
- The graphics lookup requires an open frame callback admission.

## Verification scope

Source comparison covered both D3D12 device sources, every `execute11` route, the fence order,
the retirement proof, rollback and release order, and confirms the `rsf_unity_*` exports,
`rsf_unity_package_platform`, `rsf_unity_native_api` and the managed package are unchanged.
Layouts are asserted on both sides. The formatting check used rustfmt 1.11.0 (Rust 1.100), since
the pinned 1.94.1 toolchain is not installed here. Unchanged neighbouring files also passed.
Locked offline Cargo metadata and staged whitespace checks also passed. Native and Rust
compilation and linking, Clippy, the new unit tests, the D3D11 fixtures, Unity Editor and player
runs, device removal and teardown timing need user verification. The surface table remained native
in this step and moved in the [shared surface step](rust-unity-d3d11-surfaces.md).
