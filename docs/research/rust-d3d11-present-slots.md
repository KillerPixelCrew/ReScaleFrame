# Presentation command slot ownership in Rust

This source port starts at `a5867f2f` and follows the [bridge cutover](rust-d3d11-present-cutover.md),
buffer cutover and physical chain cutover.
The question was whether the three D3D12 allocator/list pairs that the shared presentation facade
kept as `Bridge::slots[3]` could become leases of the existing Rust owner, while every D3D12 call,
the device that creates them, the proxy or native list choice and the teardown order stay as they
were. Completion and retirement fences, events, device and queue identity and the physical chain
are unchanged apart from the plumbing listed below. This was a source port. Nothing was built or
run; see [Unsettled checks](#unsettled-checks).

## Ownership after the port

| Path | Before (`d3d11_present_bridge.cpp`) | After |
| --- | --- | --- |
| Creation | `create_slots` looped over `slots[0..3]` and called `rsf::create_command_list` on `graphics.device` into each member, returning `E_FAIL` at the first refusal. | `Owner::create_command_slots` calls the `create_slot` leaf three times in slot order. The leaf runs the same helper on the same `graphics.device` (Streamline's proxy device when a host exists, otherwise the direct device) and hands back two owned references. Rust adopts them as `ComRef`. |
| Partial creation | Earlier members kept their pairs. The failing iteration's helper had already released its list, then its allocator. `initialize` returned `E_FAIL` and the unpublished facade was destroyed through `chain_destroy`. | The helper behaviour is unchanged. Rust stores the earlier pairs in the owner before returning `E_FAIL`, so the same teardown releases or retains them. |
| Present pin | Leaves indexed `self.slots[index]`. | Present pins the slot set with an `Arc` clone (no COM reference) after `ensure_physical`, the same lease pattern as `Physical`, and passes the slot's allocator and list to the leaves. |
| Reset | `slot_reset`: CPU wait on `complete` for the slot fence (step 0), `allocator->Reset()` (1), `list->Reset(allocator, nullptr)` (2). | Same leaf body and step numbering with the borrowed pointers. The slot fences, fence serial and ring index stay in the owner unchanged. |
| Record | `record_copy` recorded `copy_transitioned` on `slots[index].list`. | Same call on the borrowed list; the `native12` source state still comes from the facade. |
| List selection | `slot_command` returned `provider_list(queue, slots[index].list)`. | The existing `command_list` leaf already called `provider_list(queue, list)`, so the duplicate `slot_command` leaf was removed. A Streamline host and a queue other than `graphics.queue` still select the unwrapped native list; otherwise the proxy list. A null result still faults the frame as `command list unwrap` before prepare. |
| Close and submit | `close_execute` closed `slots[index].list` and, only on success, executed the selected list. | Same order. The leaf takes the queue, the slot list to close and the selected list to execute; it no longer needs the facade. |
| Drain failure | `retain_all` detached the six pair members together with the fences, device, context, factory and queues. | Rust forgets its slot set (submitted lists and allocators can still be GPU-reachable) beside the application buffers and physical chain, then `retain_all` detaches the remaining facade members. |
| Private host teardown | `release_device(1)`: close the events, reset the interop queue, reset each slot's list then allocator for slots 0, 1, 2, reset `complete`, `retired`, context, D3D11 device and factory, then `rsf_streamline_host_destroy`. | `release_device(1)` closes the events and resets the interop queue. Rust then releases slots 0, 1, 2, each list before its allocator. The new `release_host` leaf resets the remaining members and destroys the host. The pairs still end before host shutdown. |
| Shared or absent host | `release_device(0)` closed the events and, with a shared host, detached the interop queue. After `chain_destroy` returned, the facade's member destructors released `slots[2]`, `slots[1]`, `slots[0]` (list before allocator), before `retired`, `complete` and the queues. | `release_device(0)` is unchanged. Rust releases slots 2, 1, 0, list before allocator, as the last step of `retire_owner`. The owner's later drop releases no native object, so nothing native runs between this release and the facade's remaining member destructors. |

`Commands` declares `list` before `allocator` so field drop order gives list-first release. A
`Vec` drops its elements in index order, so last-slot-first teardown reverses the vector before
dropping it.

## Private leaf table changes

`d3d11_present_platform.h` is private to `rsf_presentation` and the Rust owner. The published
`d3d11_present_bridge.h` header, its 33 exports and every public structure are untouched.

- `create_slots(bridge)` became `create_slot(bridge, allocator**, list**)` at the same index.
- `slot_reset` and `record_copy` take the borrowed allocator and list in place of a slot index.
- `slot_command` was removed. `close_execute` became `(queue, list, command)`.
- `release_device`'s argument is now named `private_host`, and `release_host(bridge)` follows it.
- The table stays 71 pointers. `query_buffer_interface`, `drain_queues` and `set_description`
  moved one slot earlier (54, 57, 61), `release_host` is at 67 and `verify_outer` stays at 70.
  `d3d11_present_bridge.cpp` and `d3d11_present_abi.rs` assert the same offsets.

## Differences and assumptions

- Present returns `E_UNEXPECTED` before any callback if it cannot pin all three pairs. Publication
  requires a successful `create_command_slots` and only the facade's final Release takes the set,
  so this guard is not reachable from a published facade.
- Teardown releases the pairs only through `Arc::try_unwrap`. A pin cannot outlive Present, and the
  facade's final Release cannot run during its own Present, so the fallback of releasing with the
  last pin is unreachable. It would only delay the release; it never double-releases.
- The shared-host release now happens at the end of `retire_owner` instead of in the facade's
  member destructors. Between those points the C++ path ran only the owner's `Box` drop, which
  holds no COM object after retirement.
- No fence value, wait, queue join, upload, Present or callback moved. The pin adds no COM
  `AddRef` or `Release`.

## Synthetic coverage added

The fake leaf table now creates fake COM pairs, maps every `slot_reset`, `record_copy` and
`close_execute` back to the pinned pair and asserts the owner holds a reference during the call.
The executed list must be the `command_list` result. `release_device` and `release_host` record
how many pairs the owner still held.

- `creation_admission_publication_and_retirement`: without a private host all three pairs are
  held at `release_device(0)` and released afterwards.
- `failed_retirement_retains_provider_and_inputs`: all three pairs keep their owner reference after
  a failed drain, on both the D3D11 and native D3D12 paths.
- `partial_slot_creation_releases_or_retains_created_pairs` (new): a refused third pair leaves two.
  Without a host they are released after `release_device(0)`. With a private host they are held at
  `release_device(1)` and released before `release_host`. A failed drain retains them.
- `d3d11_present_orders_callbacks_and_rotates_slots` keeps its rotation and fence expectations,
  now through pointer-identified pairs.

The fake cannot observe the order of individual COM releases, so slot order and list-before-
allocator order are source-checked only.

## Checks performed

- Source review of every former `slots` use in the facade against the new leaves and owner paths
  (creation, partial creation, both Present paths, retain, private and shared release).
- `grep` confirmed no remaining `slot_command`, `create_slots` or `Bridge::slots` use in `runtime/`
  and no other consumer of the private table outside the presentation library and its Rust owner.
- The three changed Rust files were formatted with the pinned rustfmt 1.8.0 from Rust 1.94.1,
  edition 2024.

## Unsettled checks

The user owns these runs; none was performed for this change.

- MSVC build of `rsf_presentation` and its consumers, including the new `static_assert` offsets,
  and `cargo build`/`cargo clippy --workspace --all-targets --locked -- -D warnings`.
- `cargo test --workspace --locked`, in particular the four orchestrator tests above.
- Export and header comparison against `a5867f2f` (Runtime, carriers, Minecraft, Unity and Godot
  Native), expected unchanged because only the private table moved.
- The device fixtures `tests/d3d11_present_bridge.cpp` and `tests/fg_present_bridge.cpp`, which
  exercise real allocator and list creation, Reset, Close and Execute on a device.
- Game runs on the D3D11 facade (Anomaly), native D3D12 switching (Unity, RDR2) and a private
  Streamline host teardown, including a failed initialization after host creation.
