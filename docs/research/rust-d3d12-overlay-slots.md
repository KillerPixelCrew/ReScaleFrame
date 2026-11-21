# Native D3D12 overlay frame slot ownership in Rust

This source port starts at `aa66153c`. It follows the earlier Rust ownership of the native D3D12
overlay's Present ring, fence wait and atlas leases (see `docs/implementation.md`) and uses the
same lease pattern as the [D3D11 presentation command slots](rust-d3d11-present-slots.md).

The question was whether the three `Frame` members of the native renderer in
`runtime/orchestrator/src/overlay_native12.cpp` (direct command allocator, graphics command list,
shader-visible SRV heap and RTV heap per slot) could become leases of the existing Rust owner in
`overlay_native12.rs`, while every D3D12 call, the shader, the root signature, pipeline and texture
creation, barriers and draws stay as they were. The device, queue, root signature and compiled
shader blobs stay in the native renderer. This was a source port. Nothing was built or run; see
[Unsettled checks](#unsettled-checks).

## Ownership after the port

| Path | Before (`overlay_native12.cpp`) | After |
| --- | --- | --- |
| Creation | `initialize` created the fence, compiled both shaders, built the root signature, then looped over `frames[0..3]`: `CreateCommandAllocator`, `CreateCommandList` on it, `Close`, the 64-entry shader-visible SRV heap, the one-entry RTV heap. The fence was handed out last. | `initialize` keeps the fence, shaders and root signature and hands out the fence on success. `Panel::initialize` then calls the new `create_frame` leaf three times in slot order through `Renderer::create_frames`. The leaf runs the same calls with the same descriptors on the renderer's device, in the same order. Rust adopts each object as a `ComRef`. |
| Partial creation | `initialize` returned false at the first refusal. The local fence was released, then `create_renderer`'s `unique_ptr` destroyed the renderer: `frames[2]`, `[1]`, `[0]` (RTV, SRV, list, allocator), then the shader blobs, root, queue and device. | The leaf stops at the first refusal and hands back every object it created. The renderer retains that partial slot and the earlier complete slots. `Panel::initialize` releases the fence first, then the renderer drops the slots in reverse order and destroys the remaining native members. |
| Install failure | The renderer, frames included, was destroyed after the fence was released. | Unchanged: the fence local drops first, then the `Renderer` with its frames. |
| Slot reuse | `begin` reset `frames[slot]` after Rust selected the slot and proved its fence value complete. | Unchanged admission: `Ring::select` and `Fence::wait_for_slot` still gate the slot. Present then borrows the slot's table (`Renderer::frame`) into a `Lease` and passes it to `begin`, `upload_texture`, `record` and `execute`. Allocator and list `Reset`, the copy, transitions, SRV and RTV writes, draws, `Close` and `ExecuteCommandLists` are the same calls on the borrowed pointers. |
| Failure retention | A fault without a completion proof left the renderer, with its frames, in the panel. | The frames live in the `Renderer`, so the same failed panel retains them. |
| Retirement | `retire_failed` dropped the atlas, graphics, UI, then the renderer (frames, blobs, root, queue, device), then the fence, only after the existing completion, event and input-stop proof. | Same proof and order. The `Renderer` drop releases the frames from slot 2 to slot 0 (RTV, SRV, list, allocator), then calls `destroy` for the remaining native members. |

`Frame` declares `rtv`, `srv`, `commands`, `allocator` in that order so field drop order is the
reverse of creation, matching the former C++ member destruction. The merge uses a fixed slot
array with nullable leases, so retaining a partial slot requires no heap allocation. A slot
lends its pointers only when all four leases exist.

## Private leaf table changes

`overlay_native12_platform.h` is private to the orchestrator: only `overlay_native12.cpp` and the
Rust owner use `rsf_overlay12_platform`. `overlay_wrapped12_platform.h` includes the header for
`rsf_overlay12_log` only. The public `overlay_d3d12.h` entry points and the overlay panel ABI are
unchanged.

- New `rsf_overlay12_frame { allocator, commands, srv, rtv }`, four pointers.
- New `create_frame(panel, rsf_overlay12_frame*)` at index 2, after `initialize`.
- `begin(frame, chain, target)` takes the borrowed frame and no longer the renderer.
- `upload_texture`, `record` and `execute` take the borrowed frame in place of the slot index.
- The table grows from 18 to 19 pointers (152 bytes). `overlay_native12.cpp` static-asserts the
  frame size, `create_frame` at 2, `signal` at 18 and the table size. The hand-updated
  `overlay_native12_abi.rs` asserts every offset, and `tools/generate-rust-bindings.py` adds
  `frame` to its allowlist so a regeneration emits the same struct.

## Differences and assumptions

- The fence reference now exists in Rust while the frames are created. On a frame refusal it is
  released before the renderer, as the former local `ComPtr` was.
- Present looks up the slot's frame before clearing the slot's retained atlas and pipeline. The
  lookup cannot fail for an initialized renderer; if it did, the panel faults and keeps the slot's
  retained objects, which is the conservative direction.
- No fence value, wait, event, queue, barrier, descriptor write, draw or upload moved. The borrowed
  table adds no COM `AddRef` or `Release`.

## Synthetic consumers reviewed

- `tests/overlay_native12.cpp` (`overlay_native12_backpressure`) drives only the public
  `rsf_overlay_native12_present` on WARP with the debug layer. It needs no change; it is the fixture
  that would exercise real frame creation, reuse after a blocked queue and the debug layer.
- `overlay_wrapped12.rs` shares the binding module only for `rsf_overlay12_log`.
- The Rust unit fakes now take the frame table. The atlas test checks that each slot's upload and
  draw received that slot's list. Two new tests use fake COM objects for creation in slot order and
  `Renderer::frame` lookup, a third frame refused after its allocator and list (kept with
  the earlier frames for teardown), and that all references are gone when `destroy` runs. The fakes
  cannot observe the order of individual releases, so slot order and reverse creation order are
  source-checked only.

## Checks performed

- Source review of every former `frames` use (creation, `begin`, `upload_texture`, `draw`,
  `record`, `execute`) against the new leaves and owner paths, and of initialization failure,
  install failure, fault retention and retirement order.
- `grep` found no other consumer of `rsf_overlay12_platform`.
- `overlay_native12.rs` and `overlay_native12_abi.rs` were formatted with the pinned rustfmt 1.8.0
  from Rust 1.94.1, edition 2024.
- The merge reproduced `overlay_native12_abi.rs` byte for byte with bindgen 0.72.1.

## Unsettled checks

The user owns these runs; none was performed for this change.

- MSVC build of the orchestrator and `rsf_overlay_native12_test`, including the new
  `static_assert`, and `cargo build` and `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- `cargo test --workspace --locked`, in particular the overlay_native12 tests above.
- Export comparison against `aa66153c` for the Runtime DLL, carriers and Minecraft, expected
  unchanged because only the private table moved.
- The `overlay_native12_backpressure` device fixture and a game run on a path that presents the
  native D3D12 overlay, including a toggle and a forced initialization failure if practical.
