# Unity Editor preview resource ownership

The source audit starts at `1790428`, after the preview admission port (`2f6558a`) and the
checked package retirement (`9302dc1`). The question was whether the Editor preview's D3D12 and
DXGI objects could move from C++ globals into the Rust preview record, with the same creation,
recording and release order, the same retirement proof and an unchanged package contract.

## What moved

Before this change `integrations/unity/native/preview.cpp` held `ComPtr` globals for the lent
device and queue, the preview fence, the `IDXGISwapChain3` and three slots of allocator, list and
source texture. `preview.rs` only held the device and queue addresses for an identity check, a
`chain` flag, the fault flag and the ring. Release reset the globals without a check of its own,
and the drained check read the fence outside any lock.

`games/unity-mono/src/preview.rs` now holds every one of these as a transferred reference in its
graphics record (`Gpu` and `Slot`, mirrored by `rsf_unity_preview_gpu` and
`rsf_unity_preview_slot` in `preview_platform.h`, with size and offset assertions on both sides).
`preview.cpp` keeps only typed leaves that borrow their arguments or transfer a new reference:

| Leaf | Typed effect | Ownership |
| --- | --- | --- |
| `bind_gpu` | `CreateFence(0, NONE)`, then AddRef of device and queue | three references, all or none |
| `create_chain` | `CreateDXGIFactory1`, `CreateSwapChainForHwnd` on the queue, `As<IDXGISwapChain3>` | one on success |
| `chain_extent`, `resize_chain` | `GetDesc1`, `ResizeBuffers(3, w, h, R8G8B8A8_UNORM, 0)` | borrow |
| `completed`, `wait` | `GetCompletedValue`; event wait with 10 s timeout | borrow |
| `acquire` | `GetBuffer(GetCurrentBackBufferIndex())` | one back buffer |
| `create_slot` | `CreateCommandAllocator`, `CreateCommandList`, `Close` | each as soon as held |
| `record` | allocator and list `Reset`, four barriers, `CopyResource`, `Close` | borrow, three-way result |
| `execute`, `signal`, `present` | `ExecuteCommandLists`, `Signal`, `rsf_fg12_window` and `Present(0, 0)` | borrow |
| `retain`, `release` | `IUnknown::AddRef`, `IUnknown::Release` (null accepted) | one reference |

Window enumeration, creation, positioning and showing are unchanged leaves.

## Order preserved

- Binding: the fence is created before the device and queue are kept, as before; a refused fence
  keeps nothing and the next prepare retries. The identity check against the lent device and
  queue is unchanged.
- Swapchain: created on first prepare, resized only after the fence reaches the ring's last
  submission, with the same descriptor.
- Recording, per present: describe and format checks, prepare, the wait for the slot's last
  completion value (the per-slot proof), then the back buffer lookup, then the slot's previous
  source reference is dropped, then the slot is created if it has no list, then reset, barriers,
  copy and close, then the source is retained and the list executed, then the back buffer is
  dropped. Signal, the fault on a refused signal, `rsf_fg12_window`, Present, ring advance, the
  base-frame counter and the asynchronous show follow as before.
- Partial construction: a refused fresh slot drops its list, then its allocator, as the C++
  local's destructor did. A refused `Close` drops the slot's allocator, list and source, the order
  of the C++ `slot = {}` assignment, and a refused reset keeps the slot. A refused back buffer
  lookup touches no slot.
- Release: each slot's allocator, list and source in slot order, then swapchain, fence, queue
  and device, then the ring is forgotten. This is the order of `slots = {}; chain.Reset();
  fence.Reset(); queue.Reset(); device.Reset(); rsf_core_unity_preview_reset();`.

## Checked retirement

`rsf_unity_preview_drained` keeps its signature and caller (`rsf_unity_package_bridge_try_close`,
after every bridge route closed). Rust now reads the fence under the graphics mutex, so the fence
cannot be released during the read. The verdict is unchanged: no fault, a completed value other
than `UINT64_MAX`, and the ring drained; without a fence the read is 0. A passing check records a
proof, and any later execution clears it.

`rsf_unity_preview_release` keeps its signature and caller
(`rsf_unity_package_bridge_finish_retirement` on commit). Rust destroys the record only while that
proof holds, under the graphics mutex; otherwise it destroys nothing, as a faulted preview already
did. Every commit follows a successful `bridge_try_close`, which in the Editor includes a passing
drained check, and admission stays closed in between, so the proof holds on that path. A device
removed after the check no longer blocks the release, since the check itself was taken first.
Render-thread callers only try the mutex, so holding it while the leaves release cannot wait on
them. Device shutdown order is unchanged: the preview releases its device and queue references in
`finish_retirement` before the bridge releases its own objects and before Unity's device event
returns.

## Intentional differences

- Release refuses without a drained proof taken since the last submission. The C++ released
  unconditionally when the package committed.
- The drained check's fence read and the release run under the graphics mutex.
- The back buffer is obtained and dropped through leaves; the slot's previous source reference is
  dropped by Rust rather than by `ComPtr::Reset` inside the recording leaf. The order is the same.

## Source deltas

- `games/unity-mono/src/preview.rs`: graphics record, leaves table (13 to 19 entries), checked
  release export `rsf_core_unity_preview_release` replacing `rsf_core_unity_preview_reset`,
  `rsf_core_unity_preview_drained` taking the leaves instead of a completed value, unit tests for
  binding, lazy slot creation, per-slot reuse, partial rollback, close and reset failures, the
  release proof and order, and the fault.
- `games/unity-mono/src/preview_platform.h`: the matching internal boundary.
- `integrations/unity/native/preview.cpp`: globals removed, typed leaves only; the
  `rsf_unity_preview_*` functions in `preview.h` are unchanged.
- `games/unity-mono/src/bridge_interop.rs`: `Interop::drained` is now `pub(crate)`. At `1790428`
  `preview.rs` already called it from a sibling module while it was private, which does not
  compile (E0624); this is the matching edit the existing call needs.
- `games/unity-mono/README.md`: the preview paragraph.

`bridge.cpp`, `package.cpp`, `package_state`, `preview.h`, `rsf_unity_package_platform`,
`rsf_unity_native_api`, the `rsfu_*` exports and the managed package are unchanged.

## Verification scope

Source comparison against `1790428` covered every former global, each leaf's typed calls and
their order, the partial-construction, recording-failure and release paths, the drained and
release callers in `bridge.cpp` and the commit path in `package_state.rs`. rustfmt 1.11.0 (Rust
1.100; the pinned 1.94.1 is not installed) reports the changed Rust files formatted. Locked offline
Cargo metadata and staged whitespace checks were run. Native and Rust compilation and linking,
Clippy, the new unit tests, the Editor preview in a Unity Editor on D3D11 and D3D12, resize,
device removal, a refused retirement and Editor shutdown timing need user verification.

## Runtime presenter and Editor window replacement

U05 starts at `8adf010b`, with the checked package retirement and shared COMMON-source
flag in place. The package still passes the original leased window frame through its
preview callback. Unity's Rust window owner handles enumeration, sizing and visibility.
It validates the cached parent and child relationship and the child's lifetime token
before reusing it. A stale pair clears window admission. A failed coordinate conversion
or position also clears admission and discards the child. A subsequent region update can
enumerate the replacement parent, and each child gets a new generation.

Runtime's `d3d12_window_presenter.rs` owns the device, direct queue, swapchain, fence
and three copy slots per output. The sized/versioned static C interface takes a destination
token and the existing leased render pass. The Windows implementation calls DXGI and D3D12
through the pinned Windows bindings. The package's remaining `preview.cpp` links that
interface to Unity's existing callback and serializes its opaque handle through retirement.
The graphics callback table and managed exports keep their existing layouts.

A new destination makes the previous token ineligible immediately. The presenter retains
the old chain until the copy fence proves completion, then releases it and creates the new
chain. On the same destination it waits for completion before resizing. The queue signal
follows copy execution, before notifying the shared FG host and calling Present. A refused
signal preserves the slot and its leased source reference, blocks further use, and prevents
release. A successful Present to a still-current child permits the Editor to show it. The
unused preview-local frame counter was removed.

The moved CPU fixtures retain the binding, recording and retirement checks. The signal
fixture now calls the production present path with a refusing signal leaf and asserts the
slot index, receipt value, retained source and refused release. The recycled-window fixture
uses the same HWND with a new generation and checks the old-chain wait, stale-token refusal
and recovery. Source checks covered the existing package routing and bridge close/commit
sequence, Windows bindings and C target layout. Rust 1.94.1 formatting, locked offline Cargo
metadata and Git whitespace were checked. Compilation, fixtures, lint, D3D11/D3D12/Vulkan
Editor operation, reparenting and device removal remain user verification.

### Integration with the Unity bridge port

The reconciliation starts at `d539c0f6`. Editor window handling stays in
`games/unity-mono/src/preview.rs` and Runtime's only preview presenter is
`runtime/orchestrator/src/d3d12_window_presenter.rs`. Each output uses the
shared `SubmissionRing` from `runtime/graphics/submission`. Runtime checks
the next completion value before executing a copy, records the slot receipt
before signalling, and advances the ring only on success. Slot reuse,
resize and replacement all use the shared timeline. The shared completion
predicate refuses device removal, and the presenter's fault flag also
refuses retirement if a submitted copy never received a signal.

`a_fault_keeps_every_reference` enters through production `Graphics::present`,
refuses the first signal and checks that no Present or ring advance follows.
The fixture reports a completed fence value anyway and verifies the source
remains retained and the old drain proof cannot authorize release.
`a_recycled_window_waits_for_the_old_chain_and_refuses_stale_presents`
checks a new generation for the same HWND. Pending completion and device
removal both keep the old chain, and a complete copy allows the replacement.

The private package callback ABI and the U02 bridge close and release
notifications are unchanged. U04's Vulkan recording receipts and COMMON
flag, the D3D11 preview flag and Unity's existing routing and
`unity_sr_d3d12_contract` fixture are unchanged. The reconciliation was
checked with pinned Rust 1.94.1 formatting, locked offline Windows Cargo
metadata and staged whitespace. Build, fixture execution, lint and device
verification remain user-owned.
